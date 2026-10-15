//! Bounded ownership lanes for complete per-action native work.

use super::{PitchActionReject, PitchActionWork};
use crate::audio_engine::ordinary_endpoint_reserve::EndpointReserveWork;
use crate::audio_engine::ordinary_start::OrdinaryStartWork;
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
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
    pub(crate) fn sequence(&self) -> u64 {
        match self {
            Self::Pitch(work) => work.action.sequence(),
            Self::Ordinary(work) => work.sequence(),
        }
    }

    pub(crate) fn prepare(&mut self) -> Result<(), PitchActionReject> {
        match self {
            Self::Pitch(work) => work.prepare(),
            Self::Ordinary(work) => work.prepare(),
        }
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
            Self::Pitch(work) => !work.preparation_required() || work.action.state() == "prepared",
            Self::Ordinary(work) => work.is_ready(),
        }
    }
}

pub(crate) struct PitchActionPreparation {
    requests: Producer<PreparedActionWork>,
    ready: Consumer<PreparedActionWork>,
    retirement: Producer<PreparedActionWork>,
    endpoint_requests: Producer<Box<EndpointReserveWork>>,
    endpoint_ready: Consumer<Box<EndpointReserveWork>>,
}

pub(crate) struct PitchActionPreparationWorker {
    running: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl PitchActionPreparation {
    /// Control-only channel/thread allocation. Capacity is the existing action budget,
    /// not a per-pad latest slot. Failed actions return intact for terminal feedback.
    pub(crate) fn new(capacity: usize) -> std::io::Result<(Self, PitchActionPreparationWorker)> {
        if capacity == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "pitch action capacity must be positive",
            ));
        }
        let (requests, mut input) = RingBuffer::<PreparedActionWork>::new(capacity);
        let (mut output, ready) = RingBuffer::<PreparedActionWork>::new(capacity);
        let (retirement, mut retired) = RingBuffer::<PreparedActionWork>::new(capacity);
        // One background owner is separate from all accepted-action request cells.
        let (endpoint_requests, mut endpoint_input) =
            RingBuffer::<Box<EndpointReserveWork>>::new(1);
        let (mut endpoint_output, endpoint_ready) = RingBuffer::<Box<EndpointReserveWork>>::new(1);
        let running = Arc::new(AtomicBool::new(true));
        let worker_running = running.clone();
        let join = thread::Builder::new()
            .name("flitzis-pitch-actions".into())
            .spawn(move || {
                let mut pending = None;
                let mut pending_endpoint = None;
                while worker_running.load(Ordering::Acquire) {
                    let mut worked = false;
                    if let Ok(work) = retired.pop() {
                        drop(work);
                        worked = true;
                    }
                    if let Some(work) = pending.take() {
                        match output.push(work) {
                            Ok(()) => worked = true,
                            Err(PushError::Full(work)) => pending = Some(work),
                        }
                    }
                    if pending.is_none()
                        && output.slots() != 0
                        && let Ok(mut work) = input.pop()
                    {
                        if let Err(reason) = work.prepare() {
                            work.reject(reason);
                        }
                        if let Err(PushError::Full(work)) = output.push(work) {
                            pending = Some(work);
                        }
                        worked = true;
                    }
                    if let Some(work) = pending_endpoint.take() {
                        match endpoint_output.push(work) {
                            Ok(()) => worked = true,
                            Err(PushError::Full(work)) => pending_endpoint = Some(work),
                        }
                    }
                    // Accepted work always wins at the next job boundary. A stale queued
                    // endpoint validates its own epoch before Native construction/preparation.
                    if pending.is_none()
                        && input.is_empty()
                        && pending_endpoint.is_none()
                        && endpoint_output.slots() != 0
                        && let Ok(mut work) = endpoint_input.pop()
                    {
                        work.prepare();
                        if let Err(PushError::Full(work)) = endpoint_output.push(work) {
                            pending_endpoint = Some(work);
                        }
                        worked = true;
                    }
                    if !worked {
                        thread::sleep(Duration::from_millis(1));
                    }
                }
                // Stream teardown has stopped the callback before joining this worker.
                // All values still on its lanes release source/native pins off realtime.
                drop(pending);
                drop(pending_endpoint);
                while let Ok(work) = input.pop() {
                    drop(work);
                }
                while let Ok(work) = retired.pop() {
                    drop(work);
                }
                while let Ok(work) = endpoint_input.pop() {
                    drop(work);
                }
            })?;
        Ok((
            Self {
                requests,
                ready,
                retirement,
                endpoint_requests,
                endpoint_ready,
            },
            PitchActionPreparationWorker {
                running,
                join: Some(join),
            },
        ))
    }

    pub(crate) fn preparation_available(&self) -> bool {
        self.preparation_slots() != 0
    }
    pub(crate) fn preparation_slots(&self) -> usize {
        self.requests.slots()
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

    /// Single callback producer. A failed publish leaves the complete owner with caller.
    pub(crate) fn publish_prepared(
        &mut self,
        work: &mut Option<PreparedActionWork>,
    ) -> Result<bool, PitchActionReject> {
        if self.requests.slots() == 0 {
            return Err(PitchActionReject::Capacity);
        }
        let Some(value) = work.take() else {
            return Ok(false);
        };
        match self.requests.push(value) {
            Ok(()) => Ok(true),
            Err(PushError::Full(value)) => {
                *work = Some(value);
                Err(PitchActionReject::Capacity)
            }
        }
    }

    /// Returns the original work including failed owners. Caller retains or retires it;
    /// polling never applies pitch and never calls native code.
    pub(crate) fn poll_prepared(&mut self) -> Option<PreparedActionWork> {
        self.ready.pop().ok()
    }

    pub(crate) fn retire_prepared(&mut self, work: &mut Option<PreparedActionWork>) -> bool {
        if self.retirement.slots() == 0 {
            return false;
        }
        let Some(value) = work.take() else {
            return true;
        };
        match self.retirement.push(value) {
            Ok(()) => true,
            Err(PushError::Full(value)) => {
                *work = Some(value);
                false
            }
        }
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

impl Drop for PitchActionPreparationWorker {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
