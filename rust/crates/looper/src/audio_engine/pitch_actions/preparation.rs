//! Bounded ownership lanes for complete per-action native work.

use super::{PitchActionReject, PitchActionWork};
use crate::audio_engine::ordinary_endpoint_reserve::EndpointReserveWork;
use crate::audio_engine::ordinary_start::OrdinaryStartWork;
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Existing fixed worker custody shared by pitch changes and ordinary attacks.
/// The enum moves already allocated owners; it adds no callback allocation.
pub(crate) enum PreparedActionWork {
    Pitch(Box<PitchActionWork>),
    Ordinary(Box<OrdinaryStartWork>),
}

impl PreparedActionWork {
    #[cfg(all(test, windows))]
    fn observation(&self) -> Option<&Arc<crate::audio_engine::preparation_observation::Timing>> {
        match self {
            Self::Ordinary(work) => work.preparation_observation.as_ref(),
            Self::Pitch(_) => None,
        }
    }
    pub(crate) fn sequence(&self) -> u64 {
        match self {
            Self::Pitch(work) => work.action.sequence(),
            Self::Ordinary(work) => work.sequence(),
        }
    }

    pub(crate) fn prepare(&mut self) -> Result<(), PitchActionReject> {
        #[cfg(all(test, windows))]
        let observation = self.observation().cloned();
        #[cfg(all(test, windows))]
        if let Some(observation) = &observation {
            observation.start();
        }
        let result = match self {
            Self::Pitch(work) => work.prepare(),
            Self::Ordinary(work) => work.prepare(),
        };
        #[cfg(all(test, windows))]
        if let Some(observation) = observation {
            let native = match self {
                Self::Ordinary(work) => work
                    .candidate
                    .as_ref()
                    .and_then(|candidate| candidate.rubberband.as_ref())
                    .map_or(0, |native| native.state_address()),
                Self::Pitch(_) => 0,
            };
            observation.end(native);
        }
        result
    }

    pub(crate) fn reject(&self, reason: PitchActionReject) {
        match self {
            Self::Pitch(work) => {
                work.action.reject(reason);
            }
            Self::Ordinary(work) => work.reject(reason),
        }
    }

    pub(crate) fn preparation_required(&self) -> bool {
        match self {
            Self::Pitch(work) => work.preparation_required(),
            Self::Ordinary(work) => work.preparation_required(),
        }
    }

    pub(crate) fn is_ready(&self) -> bool {
        match self {
            Self::Pitch(work) => work.is_ready(),
            Self::Ordinary(work) => work.is_ready(),
        }
    }
}

#[inline]
fn publish_ready(
    output: &mut Producer<PreparedActionWork>,
    work: PreparedActionWork,
    accepted_pending: &AtomicUsize,
) -> Result<(), PushError<PreparedActionWork>> {
    #[cfg(all(test, windows))]
    let observation = work.observation().cloned();
    #[cfg(all(test, windows))]
    if let Some(observation) = &observation {
        observation.ready_before();
    }
    let result = output.push(work);
    #[cfg(all(test, windows))]
    if result.is_ok()
        && let Some(observation) = observation
    {
        observation.ready_after();
    }
    if result.is_ok() {
        accepted_pending.fetch_sub(1, Ordering::SeqCst);
    }
    result
}

struct CallbackPreparationLane {
    requests: Producer<PreparedActionWork>,
    ready: Consumer<PreparedActionWork>,
    // Retain the worker's retirement producer in every build.
    _retirement: Producer<PreparedActionWork>,
}

pub(crate) struct PitchActionPreparation {
    #[cfg(all(test, windows))]
    observation: Arc<crate::audio_engine::preparation_observation::PoolObservation>,
    lanes: Vec<CallbackPreparationLane>,
    publish_next: usize,
    poll_next: usize,
    _retire_next: usize,
    accepted_pending: Arc<AtomicUsize>,
    endpoint_requests: Producer<Box<EndpointReserveWork>>,
    endpoint_ready: Consumer<Box<EndpointReserveWork>>,
}

pub(crate) struct PitchActionPreparationWorker {
    running: Arc<AtomicBool>,
    joins: Vec<JoinHandle<()>>,
}

struct EndpointWorkerLane {
    input: Consumer<Box<EndpointReserveWork>>,
    output: Producer<Box<EndpointReserveWork>>,
    pending: Option<Box<EndpointReserveWork>>,
}

struct ActionWorkerLane {
    #[cfg(all(test, windows))]
    observation: Arc<crate::audio_engine::preparation_observation::PoolObservation>,
    input: Consumer<PreparedActionWork>,
    output: Producer<PreparedActionWork>,
    retired: Consumer<PreparedActionWork>,
    pending: Option<PreparedActionWork>,
}

impl ActionWorkerLane {
    fn run(
        mut self,
        running: Arc<AtomicBool>,
        accepted_pending: Arc<AtomicUsize>,
        mut endpoint: Option<EndpointWorkerLane>,
    ) {
        while running.load(Ordering::Acquire) {
            let mut worked = false;
            if let Ok(work) = self.retired.pop() {
                drop(work);
                worked = true;
            }
            if let Some(work) = self.pending.take() {
                match publish_ready(&mut self.output, work, &accepted_pending) {
                    Ok(()) => worked = true,
                    Err(PushError::Full(work)) => self.pending = Some(work),
                }
            }
            if self.pending.is_none()
                && self.output.slots() != 0
                && let Ok(mut work) = self.input.pop()
            {
                #[cfg(all(test, windows))]
                crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::before_worker(&work);
                let result = work.prepare();
                #[cfg(all(test, windows))]
                crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::after_worker(
                    &work,
                    result.as_ref().err().copied(),
                );
                if let Err(reason) = result {
                    work.reject(reason);
                }
                if let Err(PushError::Full(work)) =
                    publish_ready(&mut self.output, work, &accepted_pending)
                {
                    self.pending = Some(work);
                }
                worked = true;
            }
            if let Some(endpoint) = endpoint.as_mut() {
                if let Some(work) = endpoint.pending.take() {
                    match endpoint.output.push(work) {
                        Ok(()) => worked = true,
                        Err(PushError::Full(work)) => endpoint.pending = Some(work),
                    }
                }
                // One aggregate boundary covers queued, executing and unpublished ready
                // actions on every shard. Already-started endpoint work is not preempted.
                let pending = accepted_pending.load(Ordering::SeqCst);
                if pending == 0
                    && endpoint.pending.is_none()
                    && endpoint.output.slots() != 0
                    && let Ok(mut work) = endpoint.input.pop()
                {
                    #[cfg(all(test, windows))]
                    self.observation.endpoint_started();
                    work.prepare();
                    if let Err(PushError::Full(work)) = endpoint.output.push(work) {
                        endpoint.pending = Some(work);
                    }
                    worked = true;
                }
                #[cfg(all(test, windows))]
                self.observation.endpoint_checked(pending);
            }
            if !worked {
                thread::sleep(Duration::from_millis(1));
            }
        }
        // Rendering has stopped. Retain the original off-realtime destruction path.
        if let Some(work) = self.pending.take() {
            accepted_pending.fetch_sub(1, Ordering::SeqCst);
            drop(work);
        }
        while let Ok(work) = self.input.pop() {
            accepted_pending.fetch_sub(1, Ordering::SeqCst);
            drop(work);
        }
        while let Ok(work) = self.retired.pop() {
            drop(work);
        }
        if let Some(mut endpoint) = endpoint {
            drop(endpoint.pending.take());
            while let Ok(work) = endpoint.input.pop() {
                drop(work);
            }
        }
    }
}

impl PitchActionPreparation {
    /// Allocate fixed aggregate lanes off realtime; every action retains its own Work.
    pub(crate) fn new(capacity: usize) -> std::io::Result<(Self, PitchActionPreparationWorker)> {
        if capacity == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "pitch action capacity must be positive",
            ));
        }
        // Leave headroom for rendering without claiming a dedicated CPU reservation.
        let worker_count =
            thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).clamp(1, 2));
        #[cfg(all(test, windows))]
        let worker_count = crate::audio_engine::preparation_observation::workers(worker_count);
        let worker_count = worker_count.min(capacity);
        let running = Arc::new(AtomicBool::new(true));
        let accepted_pending = Arc::new(AtomicUsize::new(0));
        #[cfg(all(test, windows))]
        let observation =
            Arc::new(crate::audio_engine::preparation_observation::PoolObservation::default());
        // This guard owns each successful spawn before another fallible operation.
        let mut worker = PitchActionPreparationWorker {
            running: running.clone(),
            joins: Vec::with_capacity(worker_count),
        };
        let (endpoint_requests, endpoint_input) = RingBuffer::new(1);
        let (endpoint_output, endpoint_ready) = RingBuffer::new(1);
        let mut endpoint = Some(EndpointWorkerLane {
            input: endpoint_input,
            output: endpoint_output,
            pending: None,
        });
        let mut lanes = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let cells = capacity / worker_count + usize::from(index < capacity % worker_count);
            let (requests, input) = RingBuffer::new(cells);
            let (output, ready) = RingBuffer::new(cells);
            let (retirement, retired) = RingBuffer::new(cells);
            let lane = ActionWorkerLane {
                #[cfg(all(test, windows))]
                observation: observation.clone(),
                input,
                output,
                retired,
                pending: None,
            };
            let thread_running = running.clone();
            let thread_pending = accepted_pending.clone();
            let endpoint = endpoint.take();
            #[cfg(all(test, windows))]
            crate::audio_engine::preparation_observation::before_spawn(index)?;
            let join = thread::Builder::new()
                .name(format!("flitzis-pitch-actions-{index}"))
                .spawn(move || lane.run(thread_running, thread_pending, endpoint))?;
            worker.joins.push(join);
            #[cfg(all(test, windows))]
            crate::audio_engine::preparation_observation::after_spawn(worker.joins.last().unwrap());
            lanes.push(CallbackPreparationLane {
                requests,
                ready,
                _retirement: retirement,
            });
        }
        Ok((
            Self {
                #[cfg(all(test, windows))]
                observation,
                lanes,
                publish_next: 0,
                poll_next: 0,
                _retire_next: 0,
                accepted_pending,
                endpoint_requests,
                endpoint_ready,
            },
            worker,
        ))
    }

    #[cfg(all(test, windows))]
    pub(in crate::audio_engine) fn observation(
        &self,
    ) -> Arc<crate::audio_engine::preparation_observation::PoolObservation> {
        self.observation.clone()
    }

    pub(crate) fn preparation_available(&self) -> bool {
        self.preparation_slots() != 0
    }
    pub(crate) fn preparation_slots(&self) -> usize {
        self.lanes.iter().map(|lane| lane.requests.slots()).sum()
    }
    pub(crate) fn endpoint_available(&self) -> bool {
        self.endpoint_requests.slots() != 0
    }
    pub(crate) fn publish_endpoint(&mut self, work: &mut Option<Box<EndpointReserveWork>>) {
        if !self.endpoint_available() {
            return;
        }
        if let Some(value) = work.take()
            && let Err(PushError::Full(value)) = self.endpoint_requests.push(value)
        {
            *work = Some(value);
        }
    }
    pub(crate) fn poll_endpoint(&mut self) -> Option<Box<EndpointReserveWork>> {
        self.endpoint_ready.pop().ok()
    }

    /// The single callback producer selects an available lane after aggregate preflight.
    pub(crate) fn publish_prepared(
        &mut self,
        work: &mut Option<PreparedActionWork>,
    ) -> Result<bool, PitchActionReject> {
        if self.preparation_slots() == 0 {
            return Err(PitchActionReject::Capacity);
        }
        let Some(value) = work.take() else {
            return Ok(false);
        };
        let mut value = Some(value);
        for offset in 0..self.lanes.len() {
            let index = (self.publish_next + offset) % self.lanes.len();
            if self.lanes[index].requests.slots() == 0 {
                continue;
            }
            #[cfg(all(test, windows))]
            let observation = value
                .as_ref()
                .and_then(PreparedActionWork::observation)
                .cloned();
            #[cfg(all(test, windows))]
            if let Some(observation) = &observation {
                observation.enqueue_before();
            }
            // Count before publication so the endpoint cannot observe an accepted gap.
            self.accepted_pending.fetch_add(1, Ordering::SeqCst);
            match self.lanes[index].requests.push(value.take().unwrap()) {
                Ok(()) => {
                    self.publish_next = (index + 1) % self.lanes.len();
                    #[cfg(all(test, windows))]
                    if let Some(observation) = observation {
                        observation.enqueue_after();
                    }
                    return Ok(true);
                }
                Err(PushError::Full(returned)) => {
                    self.accepted_pending.fetch_sub(1, Ordering::SeqCst);
                    value = Some(returned);
                }
            }
        }
        *work = value;
        Err(PitchActionReject::Capacity)
    }

    /// Return intact original owners; the runtime still matches and orders their sequence.
    pub(crate) fn poll_prepared(&mut self) -> Option<PreparedActionWork> {
        for offset in 0..self.lanes.len() {
            let index = (self.poll_next + offset) % self.lanes.len();
            if let Ok(work) = self.lanes[index].ready.pop() {
                self.poll_next = (index + 1) % self.lanes.len();
                #[cfg(all(test, windows))]
                if let Some(observation) = work.observation() {
                    observation.polled();
                }
                return Some(work);
            }
        }
        None
    }

    #[cfg(test)]
    pub(crate) fn retire_prepared(&mut self, work: &mut Option<PreparedActionWork>) -> bool {
        for offset in 0..self.lanes.len() {
            let index = (self._retire_next + offset) % self.lanes.len();
            if self.lanes[index]._retirement.slots() == 0 {
                continue;
            }
            let Some(value) = work.take() else {
                return true;
            };
            match self.lanes[index]._retirement.push(value) {
                Ok(()) => {
                    self._retire_next = (index + 1) % self.lanes.len();
                    return true;
                }
                Err(PushError::Full(value)) => *work = Some(value),
            }
        }
        false
    }
    #[cfg(test)]
    pub(crate) fn publish(
        &mut self,
        work: &mut Option<Box<PitchActionWork>>,
    ) -> Result<bool, PitchActionReject> {
        let mut prepared = work.take().map(PreparedActionWork::Pitch);
        let result = self.publish_prepared(&mut prepared);
        *work = prepared.map(|work| match work {
            PreparedActionWork::Pitch(work) => work,
            PreparedActionWork::Ordinary(_) => unreachable!("pitch-only test publication"),
        });
        result
    }

    #[cfg(test)]
    pub(crate) fn poll(&mut self) -> Option<Box<PitchActionWork>> {
        self.poll_prepared().map(|work| match work {
            PreparedActionWork::Pitch(work) => work,
            PreparedActionWork::Ordinary(_) => unreachable!("pitch-only test publication"),
        })
    }

    #[cfg(test)]
    pub(crate) fn retire(&mut self, work: &mut Option<Box<PitchActionWork>>) -> bool {
        let mut prepared = work.take().map(PreparedActionWork::Pitch);
        let retired = self.retire_prepared(&mut prepared);
        *work = prepared.map(|work| match work {
            PreparedActionWork::Pitch(work) => work,
            PreparedActionWork::Ordinary(_) => unreachable!("pitch-only test publication"),
        });
        retired
    }
}

#[cfg(all(test, windows))]
impl PitchActionPreparationWorker {
    pub(crate) fn thread_handles_for_test(&self) -> Vec<std::os::windows::io::OwnedHandle> {
        use std::os::windows::io::AsHandle;
        self.joins
            .iter()
            .map(|join| join.as_handle().try_clone_to_owned().unwrap())
            .collect()
    }
}

impl Drop for PitchActionPreparationWorker {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        for join in self.joins.drain(..) {
            let _ = join.join();
        }
    }
}
