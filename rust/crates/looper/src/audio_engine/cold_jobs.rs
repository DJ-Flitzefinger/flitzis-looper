//! Fixed cold-load lane. Reservations precede request mutations or PCM allocation.

#[cfg(test)]
#[path = "cold_queued_cancellation_tests.rs"]
mod queued_cancellation_tests;

use std::cell::Cell;
use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

thread_local! { static ON_WORKER: Cell<usize> = const { Cell::new(0) }; }

pub(super) const WORKERS: usize = 2;
pub(super) const QUEUED_JOBS: usize = 32;
pub(super) const PCM_LIMIT_BYTES: usize = 1024 * 1024 * 1024;
type Job = Box<dyn FnOnce() + Send>;
type Cancellation = Arc<dyn Fn() -> bool + Send + Sync>;
type QueuedRetirement = Box<dyn FnOnce(Job) + Send>;
struct AdmittedJob {
    job: Option<Job>,
    queued_retirement: Option<QueuedRetirement>,
    cancelled: Option<Cancellation>,
    reservation: Option<Reservation>,
    heavy: Option<super::heavy_work::Ticket>,
}

impl Drop for AdmittedJob {
    fn drop(&mut self) {
        // Queued cancellation and worker unwind retire captured physical owners
        // before making either admission visible as free. Never under a lock.
        if let Some(job) = self.job.take() {
            if let Some(retire) = self.queued_retirement.take() {
                retire(job);
            } else {
                drop(job);
            }
        }
        drop(self.queued_retirement.take());
        drop(self.cancelled.take());
        drop(self.reservation.take());
        drop(self.heavy.take());
    }
}

/// An off-lock observation borrows the SAME complete admission until its own
/// predicate owners end, even if a worker claims and finishes the original.
struct QueuedCancellationProbe {
    cancelled: Option<Cancellation>,
    reservation: Option<ReservationRetirementGuard>,
    heavy: Option<super::heavy_work::Ticket>,
}

impl Drop for QueuedCancellationProbe {
    fn drop(&mut self) {
        drop(self.cancelled.take());
        drop(self.reservation.take());
        drop(self.heavy.take());
    }
}

#[derive(Default)]
struct State {
    jobs: VecDeque<AdmittedJob>,
    active: usize,
    admitted: usize,
    stopped: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}

pub(super) struct ColdJobs {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

/// Clone only lane admission, never worker ownership or an additional pool.
#[derive(Clone)]
pub(super) struct ColdJobsHandle {
    shared: Arc<Shared>,
}

/// Queued retirement must destroy the kernel's captured owners before the caller observes
/// disconnection. Closure capture-field order alone does not provide that guarantee.
struct OwnedDelivery<T, F> {
    operation: Option<F>,
    sender: Option<mpsc::SyncSender<Result<T, String>>>,
}

impl<T, F> Drop for OwnedDelivery<T, F> {
    fn drop(&mut self) {
        drop(self.operation.take());
        drop(self.sender.take());
    }
}

impl ColdJobsHandle {
    pub(super) fn ensure_open(&self) -> Result<(), String> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| "cold admission lock poisoned")?;
        if state.stopped {
            return Err("cold source lane stopped".into());
        }
        Ok(())
    }
    pub(super) fn reserve(&self) -> Result<Reservation, String> {
        reserve_shared(&self.shared)
    }

    /// The same lane and shutdown fence, without retaining its Engine owner.
    pub(super) fn submit(
        &self,
        reservation: Reservation,
        job: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        self.submit_classified(reservation, job, None, None)
    }

    fn submit_classified(
        &self,
        reservation: Reservation,
        job: impl FnOnce() + Send + 'static,
        heavy: Option<super::heavy_work::Ticket>,
        cancelled: Option<Cancellation>,
    ) -> Result<(), String> {
        self.submit_retiring_classified(reservation, job, heavy, cancelled, None)
    }

    fn submit_retiring_classified(
        &self,
        reservation: Reservation,
        job: impl FnOnce() + Send + 'static,
        heavy: Option<super::heavy_work::Ticket>,
        cancelled: Option<Cancellation>,
        queued_retirement: Option<QueuedRetirement>,
    ) -> Result<(), String> {
        let mut admitted = AdmittedJob {
            job: Some(Box::new(job)),
            queued_retirement: None,
            cancelled,
            reservation: Some(reservation),
            heavy,
        };
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| "cold queue lock poisoned")?;
        if state.stopped {
            drop(state);
            return Err("cold lane stopped".into());
        }
        // Only an accepted queue owns terminal retirement. A rejected caller
        // may still hold its request/metadata locks and performs its own cleanup.
        admitted.queued_retirement = queued_retirement;
        state.jobs.push_back(admitted);
        self.shared.ready.notify_one();
        Ok(())
    }
    /// Direct public preparation enters the same bounded workers. Nested
    /// kernels already running there execute under their enclosing admission.
    pub(super) fn execute_owned<T: Send + 'static>(
        &self,
        cancelled: impl Fn() -> bool,
        operation: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        self.execute_owned_delivery(cancelled, operation, false, false)
    }

    /// After admission, cancellation is sticky but never certifies an outstanding kernel as
    /// terminal. Wait for its actual result or queued-owner retirement, then discard a cancelled
    /// result here, off realtime, before returning. This uses the same bounded worker admission.
    pub(super) fn execute_owned_until_terminal<T: Send + 'static>(
        &self,
        cancelled: impl Fn() -> bool,
        operation: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        self.execute_owned_delivery(cancelled, operation, true, false)
    }

    pub(super) fn execute_heavy_owned_until_terminal<T: Send + 'static>(
        &self,
        cancelled: impl Fn() -> bool,
        operation: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        self.execute_owned_delivery(cancelled, operation, true, true)
    }

    fn waiting_error(
        &self,
        cancelled: &impl Fn() -> bool,
        await_terminal: bool,
    ) -> Option<&'static str> {
        let cancelled = cancelled();
        if cancelled && !await_terminal {
            return Some("cold preparation proposal cancelled while waiting");
        }
        match self.shared.state.lock() {
            Ok(state) if cancelled || state.stopped => {
                Some("cold preparation proposal cancelled while waiting")
            }
            Ok(_) => None,
            Err(poisoned) => {
                if await_terminal {
                    // A poisoned lane cannot execute its queued kernels. Close it permanently,
                    // but retire each captured operation off realtime before delivery disconnects.
                    // Strict cancellation must inspect this even after cancellation is sticky.
                    let mut state = poisoned.into_inner();
                    state.stopped = true;
                    self.shared.ready.notify_all();
                    let queued = std::mem::take(&mut state.jobs);
                    drop(state);
                    drop(queued);
                }
                Some("cold admission lock poisoned")
            }
        }
    }

    fn retire_queued(&self, receipt: &Arc<AtomicBool>) {
        let queued = {
            // waiting_error owns poisoned-lane retirement. If poison races this
            // lookup, the next observation still drains it before disconnection.
            let Ok(mut state) = self.shared.state.lock() else {
                return;
            };
            let index = state.jobs.iter().position(|job| {
                job.reservation
                    .as_ref()
                    .is_some_and(|reservation| Arc::ptr_eq(&reservation.owner.retired, receipt))
            });
            index.and_then(|index| state.jobs.remove(index))
        };
        // A worker may already own this original. Only a queued match is ours
        // to retire; its captured readers may themselves need the queue mutex.
        drop(queued);
    }

    /// Inspect only queued asynchronous requests; predicates and every captured
    /// destructor run off the queue mutex. The originals remain visible to
    /// workers and terminal waiters throughout this bounded observation.
    pub(super) fn retire_cancelled(&self) {
        let mut probes: [Option<QueuedCancellationProbe>; QUEUED_JOBS] =
            std::array::from_fn(|_| None);
        {
            let Ok(state) = self.shared.state.lock() else {
                return;
            };
            for (slot, job) in probes
                .iter_mut()
                .zip(state.jobs.iter().filter(|job| job.cancelled.is_some()))
            {
                *slot = Some(QueuedCancellationProbe {
                    cancelled: job.cancelled.clone(),
                    reservation: Some(
                        job.reservation
                            .as_ref()
                            .expect("reserved job")
                            .retain_until_retired(),
                    ),
                    heavy: job.heavy.clone(),
                });
            }
        }
        for probe in probes.into_iter().flatten() {
            if probe.cancelled.as_ref().expect("queued predicate")() {
                self.retire_queued(
                    &probe
                        .reservation
                        .as_ref()
                        .expect("observed admission")
                        .owner
                        .retired,
                );
            }
        }
    }

    fn execute_owned_delivery<T: Send + 'static>(
        &self,
        cancelled: impl Fn() -> bool,
        operation: impl FnOnce() -> Result<T, String> + Send + 'static,
        await_terminal: bool,
        heavy: bool,
    ) -> Result<T, String> {
        if cancelled() {
            return Err("cold preparation proposal cancelled before admission".into());
        }
        let borrowed_heavy = heavy && super::heavy_work::current().is_some();
        if borrowed_heavy || ON_WORKER.with(Cell::get) == Arc::as_ptr(&self.shared) as usize {
            if heavy && super::heavy_work::current().is_none() {
                return Err("heavy nested preparation has no admitted owner".into());
            }
            // The same physical job already occupies a bounded offline worker.
            // Its continuation must not reserve behind independent heavy jobs
            // that cannot run until this owner retires (including a full queue).
            self.ensure_open()?;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
                .unwrap_or_else(|_| Err("cold preparation worker failed".into()));
            if await_terminal && let Some(error) = self.waiting_error(&cancelled, await_terminal) {
                drop(result);
                return Err(error.into());
            }
            return result;
        }
        let reservation = reserve_shared(&self.shared)?;
        // This unique receipt retains neither admission nor captured owners.
        let mut queued_owner = await_terminal.then(|| reservation.owner.retired.clone());
        let heavy = if heavy {
            Some(match super::heavy_work::current() {
                Some(owner) => owner,
                None => super::heavy_work::ticket()?,
            })
        } else {
            None
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| "cold queue lock poisoned")?;
            if state.stopped {
                return Err("cold lane stopped".into());
            }
            #[cfg(test)]
            let preparation_observer = super::native_owned_work::take_preparation_observer();
            let delivery = OwnedDelivery {
                operation: Some(operation),
                sender: Some(sender),
            };
            state.jobs.push_back(AdmittedJob {
                queued_retirement: None,
                job: Some(Box::new(move || {
                    let mut delivery = delivery;
                    #[cfg(test)]
                    let _observation =
                        super::native_owned_work::PreparationObservationScope::install(
                            preparation_observer,
                        );
                    let operation = delivery.operation.take().expect("owned kernel is unique");
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
                        .unwrap_or_else(|_| Err("cold preparation worker failed".into()));
                    // Legacy cancellation may drop its receiver. Terminal delivery retains it;
                    // either path retires rejected owned results off realtime.
                    let _ = delivery
                        .sender
                        .take()
                        .expect("owned delivery is unique")
                        .send(result);
                })),
                reservation: Some(reservation),
                heavy,
                cancelled: None,
            });
            self.shared.ready.notify_one();
        }
        let mut terminal_error = None;
        loop {
            let observed = self.waiting_error(&cancelled, await_terminal);
            if await_terminal {
                terminal_error = terminal_error.or(observed);
                if terminal_error.is_some()
                    && let Some(receipt) = queued_owner.take()
                {
                    self.retire_queued(&receipt);
                }
            } else if let Some(error) = observed {
                return Err(error.into());
            }
            match receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => {
                    if await_terminal {
                        terminal_error =
                            terminal_error.or(self.waiting_error(&cancelled, await_terminal));
                        if let Some(error) = terminal_error {
                            drop(result);
                            return Err(error.into());
                        }
                    }
                    return result;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(terminal_error
                        .unwrap_or("cold preparation worker retired before delivery")
                        .into());
                }
            }
        }
    }
}

fn reserve_shared(shared: &Arc<Shared>) -> Result<Reservation, String> {
    let mut state = shared
        .state
        .lock()
        .map_err(|_| "cold admission lock poisoned")?;
    if state.stopped {
        return Err("cold source lane stopped".into());
    }
    if state.admitted - state.active >= QUEUED_JOBS {
        return Err("cold source queue full (2 workers, 32 queued jobs)".into());
    }
    state.admitted += 1;
    Ok(Reservation {
        owner: Arc::new(ReservationOwner {
            shared: shared.clone(),
            running: AtomicBool::new(false),
            retired: Arc::new(AtomicBool::new(false)),
        }),
    })
}

/// An unused reservation rolls back without advancing source/request ownership.
pub(super) struct Reservation {
    owner: Arc<ReservationOwner>,
}

struct ReservationOwner {
    shared: Arc<Shared>,
    running: AtomicBool,
    retired: Arc<AtomicBool>,
}

/// Retain the same admission through captured-owner retirement, never another job.
pub(super) struct ReservationRetirementGuard {
    owner: Arc<ReservationOwner>,
}

impl Reservation {
    pub(super) fn retain_until_retired(&self) -> ReservationRetirementGuard {
        ReservationRetirementGuard {
            owner: self.owner.clone(),
        }
    }

    fn mark_running(&self) {
        let previous = self.owner.running.swap(true, Ordering::AcqRel);
        debug_assert!(!previous, "one original reservation enters one worker");
    }
}

impl ReservationRetirementGuard {
    pub(super) fn is_running(&self) -> bool {
        self.owner.running.load(Ordering::Acquire)
    }

    /// Observation only; retaining this flag retains no lane admission.
    pub(super) fn retirement_receipt(&self) -> Arc<AtomicBool> {
        self.owner.retired.clone()
    }
}

impl Drop for ReservationOwner {
    fn drop(&mut self) {
        {
            // Cleanup retains valid accounting even after an unrelated kernel
            // poisoned admission. Exactly one final Arc owns this decrement.
            let mut state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.admitted -= 1;
            if self.running.load(Ordering::Acquire) {
                state.active -= 1;
            }
        }
        self.retired.store(true, Ordering::Release);
    }
}

impl ColdJobs {
    pub(super) fn new() -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let mut lane = Self {
            shared,
            workers: Vec::new(),
        };
        for index in 0..WORKERS {
            let shared = lane.shared.clone();
            lane.workers.push(
                thread::Builder::new()
                    .name(format!("cold-source-{index}"))
                    .spawn(move || {
                        let handle = ColdJobsHandle {
                            shared: shared.clone(),
                        };
                        loop {
                            handle.retire_cancelled();
                            let job = {
                                let Ok(mut state) = shared.state.lock() else {
                                    return;
                                };
                                if state.stopped {
                                    return;
                                }
                                let eligible = state.jobs.iter().position(|job| {
                                    job.heavy.as_ref().is_none_or(|ticket| {
                                        // Error retires the queued callable below; it
                                        // never runs without an active capability.
                                        ticket.try_acquire().unwrap_or(true)
                                    })
                                });
                                if let Some(index) = eligible {
                                    let job =
                                        state.jobs.remove(index).expect("eligible bounded job");
                                    state.active += 1;
                                    job.reservation
                                        .as_ref()
                                        .expect("reserved job")
                                        .mark_running();
                                    Some(job)
                                } else {
                                    // A Python owner releases the same native gate without
                                    // touching this queue. Wait off realtime, with no worker
                                    // occupied by a blocked heavy callable; ordinary work
                                    // can still become eligible on either existing worker.
                                    let Ok((next, _)) =
                                        shared.ready.wait_timeout(state, Duration::from_millis(20))
                                    else {
                                        return;
                                    };
                                    // Revisit asynchronous cancellation outside the mutex
                                    // before another eligibility scan.
                                    drop(next);
                                    None
                                }
                            };
                            let Some(mut job) = job else {
                                continue;
                            };
                            // A panic retires the reservation and does not destroy a worker slot.
                            ON_WORKER.with(|worker| worker.set(Arc::as_ptr(&shared) as usize));
                            let operation = job.job.take().expect("unique cold callable");
                            if let Some(ticket) = &job.heavy {
                                let _ = ticket.with_current(|| {
                                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                        operation,
                                    ));
                                });
                            } else {
                                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                    operation,
                                ));
                            }
                            ON_WORKER.with(|worker| worker.set(0));
                            drop(job);
                        }
                    })?,
            );
        }
        Ok(lane)
    }

    /// Worker-free control admission shares the lane's shutdown fence without
    /// reserving one of its PCM preparation jobs. Callers retain their native
    /// request/producer/ownership fences until their command is enqueued.
    pub(super) fn ensure_open(&self) -> Result<(), String> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| "cold admission lock poisoned")?;
        if state.stopped {
            return Err("cold source lane stopped".into());
        }
        Ok(())
    }

    pub(super) fn reserve(&self) -> Result<Reservation, String> {
        reserve_shared(&self.shared)
    }
    pub(super) fn handle(&self) -> ColdJobsHandle {
        ColdJobsHandle {
            shared: self.shared.clone(),
        }
    }

    /// Enqueue under the same reserved admission. No thread is spawned per pad.
    pub(super) fn submit(
        &self,
        reservation: Reservation,
        job: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        self.handle().submit(reservation, job)
    }

    /// A queued Native analysis request has no receiving waiter. Existing
    /// workers and the control pump observe its original cancellation authority.
    pub(super) fn submit_heavy_cancellable(
        &self,
        reservation: Reservation,
        cancelled: impl Fn() -> bool + Send + Sync + 'static,
        job: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        let ticket = match super::heavy_work::current() {
            Some(owner) => owner,
            None => super::heavy_work::ticket()?,
        };
        self.handle()
            .submit_classified(reservation, job, Some(ticket), Some(Arc::new(cancelled)))
    }

    pub(super) fn submit_analysis_cancellable(
        &self,
        reservation: Reservation,
        analysis: bool,
        cancelled: impl Fn() -> bool + Send + Sync + 'static,
        queued_retirement: impl FnOnce(Job) + Send + 'static,
        job: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        let heavy = if analysis {
            Some(match super::heavy_work::current() {
                Some(owner) => owner,
                None => super::heavy_work::ticket()?,
            })
        } else {
            None
        };
        self.handle().submit_retiring_classified(
            reservation,
            job,
            heavy,
            Some(Arc::new(cancelled)),
            Some(Box::new(queued_retirement)),
        )
    }

    /// Close worker claims before releasing active jobs through cancellation.
    pub(super) fn close_admission(&self) {
        let queued = if let Ok(mut state) = self.shared.state.lock() {
            state.stopped = true;
            self.shared.ready.notify_all();
            std::mem::take(&mut state.jobs)
        } else {
            VecDeque::new()
        };
        // Job destructors can take the admission mutex; drop outside that lock.
        drop(queued);
    }

    #[cfg(test)]
    pub(super) fn counts_for_test(&self) -> (usize, usize, usize) {
        let state = self.shared.state.lock().unwrap();
        (state.active, state.jobs.len(), state.admitted)
    }

    pub(super) fn shutdown(&mut self) {
        self.close_admission();
        if self.workers.is_empty() {
            return;
        }
        // Joined cold work may still need Python for bridge cancellation and
        // reader/process retirement. Never wait while retaining its caller's
        // interpreter attachment. try_attach does not initialize an interpreter.
        // SAFETY: these are nonblocking C-API state queries. The supported App
        // uses standard-GIL CPython in one interpreter. Never attach a detached
        // Rust caller: its GIL-owning coordinator may already be waiting for it.
        let caller_holds_gil =
            unsafe { pyo3::ffi::Py_IsInitialized() != 0 && pyo3::ffi::PyGILState_Check() != 0 };
        if caller_holds_gil
            && pyo3::Python::try_attach(|py| py.detach(|| self.join_workers())).is_some()
        {
            return;
        }
        self.join_workers();
    }

    fn join_workers(&mut self) {
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

impl Drop for ColdJobs {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn queued_heavy_work_leaves_ordinary_workers_available_and_drops_before_successor() {
        let gate = Arc::new(super::super::heavy_work::Gate::default());
        let held = gate.ticket().unwrap();
        assert!(held.try_acquire().unwrap());
        let mut lane = ColdJobs::new().unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        struct Reader(Arc<AtomicBool>);
        impl Drop for Reader {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let reader = Reader(dropped.clone());
        let (first_tx, first_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        lane.handle()
            .submit_classified(
                lane.reserve().unwrap(),
                move || {
                    let _reader = reader;
                    first_tx.send(()).unwrap();
                    let _ = release_rx.recv();
                },
                Some(gate.ticket().unwrap()),
                None,
            )
            .unwrap();
        let (next_tx, next_rx) = mpsc::channel();
        lane.handle()
            .submit_classified(
                lane.reserve().unwrap(),
                move || {
                    assert!(
                        dropped.load(Ordering::Acquire),
                        "prior captured reader is still alive"
                    );
                    next_tx.send(()).unwrap();
                },
                Some(gate.ticket().unwrap()),
                None,
            )
            .unwrap();
        let (ordinary_tx, ordinary_rx) = mpsc::channel();
        lane.submit(lane.reserve().unwrap(), move || {
            ordinary_tx.send(()).unwrap();
        })
        .unwrap();
        ordinary_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(first_rx.try_recv().is_err());
        drop(held);
        first_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(next_rx.try_recv().is_err());
        release_tx.send(()).unwrap();
        next_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        lane.shutdown();
        assert_eq!(lane.counts_for_test(), (0, 0, 0));
    }

    #[test]
    fn explicit_same_job_preparation_borrows_without_readmitting_or_starving() {
        let gate = Arc::new(super::super::heavy_work::Gate::default());
        let owner = gate.ticket().unwrap();
        assert!(owner.try_acquire().unwrap());
        let successor = gate.ticket().unwrap();
        let mut lane = ColdJobs::new().unwrap();
        let handle = lane.handle();
        let nested = handle.clone();
        let (tx, rx) = mpsc::channel();
        handle
            .submit_classified(
                lane.reserve().unwrap(),
                move || {
                    assert!(super::super::heavy_work::current().is_some());
                    let value = nested
                        .execute_heavy_owned_until_terminal(|| false, || Ok(42))
                        .unwrap();
                    tx.send(value).unwrap();
                },
                Some(owner.clone()),
                None,
            )
            .unwrap();
        drop(owner);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 42);
        lane.shutdown();
        assert!(successor.try_acquire().unwrap());
    }

    #[test]
    fn active_external_job_continues_even_when_independent_heavy_queue_is_full() {
        let gate = Arc::new(super::super::heavy_work::Gate::default());
        let owner = gate.ticket().unwrap();
        assert!(owner.try_acquire().unwrap());
        let mut lane = ColdJobs::new().unwrap();
        for _ in 0..QUEUED_JOBS {
            lane.handle()
                .submit_classified(
                    lane.reserve().unwrap(),
                    || {},
                    Some(gate.ticket().unwrap()),
                    None,
                )
                .unwrap();
        }
        assert!(lane.reserve().is_err());
        let handle = lane.handle();
        let caller = std::thread::current().id();
        owner
            .with_current(|| {
                assert_eq!(
                    handle
                        .execute_heavy_owned_until_terminal(
                            || false,
                            move || {
                                assert_eq!(std::thread::current().id(), caller);
                                Ok(42)
                            }
                        )
                        .unwrap(),
                    42
                );
                assert!(
                    handle
                        .execute_heavy_owned_until_terminal::<()>(
                            || false,
                            || panic!("controlled same-owner continuation panic"),
                        )
                        .is_err()
                );
                struct Reader(Arc<AtomicBool>);
                impl Drop for Reader {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::Release);
                    }
                }
                let retired = Arc::new(AtomicBool::new(false));
                let reader = Reader(retired.clone());
                let cancelled = Arc::new(AtomicBool::new(false));
                let cancel_during_work = cancelled.clone();
                assert!(
                    handle
                        .execute_heavy_owned_until_terminal(
                            || cancelled.load(Ordering::Acquire),
                            move || {
                                cancel_during_work.store(true, Ordering::Release);
                                Ok(reader)
                            },
                        )
                        .is_err()
                );
                assert!(retired.load(Ordering::Acquire));
                assert!(super::super::heavy_work::current().is_some());
            })
            .unwrap();
        assert_eq!(lane.counts_for_test(), (0, QUEUED_JOBS, QUEUED_JOBS));
        lane.shutdown();
        drop(owner);
        assert_eq!(lane.counts_for_test(), (0, 0, 0));
    }

    #[test]
    fn shutdown_and_drop_release_python_for_live_worker_retirement() {
        const CHILD: &str = "FLITZI_COLD_GIL_JOIN_CHILD";
        const TEST: &str = "audio_engine::cold_jobs::tests::shutdown_and_drop_release_python_for_live_worker_retirement";
        let Ok(mode) = std::env::var(CHILD) else {
            for mode in ["shutdown", "drop", "detached"] {
                let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
                    .env(CHILD, mode)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(15);
                while child.try_wait().unwrap().is_none() {
                    if std::time::Instant::now() >= deadline {
                        child.kill().unwrap();
                        let output = child.wait_with_output().unwrap();
                        panic!(
                            "owned {mode} child hung joining Python work: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let output = child.wait_with_output().unwrap();
                assert!(
                    output.status.success(),
                    "{mode}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            return;
        };
        pyo3::Python::initialize();
        if mode == "detached" {
            let (made_tx, made_rx) = mpsc::channel();
            let (go_tx, go_rx) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                let mut engine = crate::audio_engine::AudioEngine::new().unwrap();
                made_tx.send(()).unwrap();
                go_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                engine.shut_down().unwrap();
                drop(engine);
            });
            made_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            pyo3::Python::attach(|_| {
                go_tx.send(()).unwrap();
                worker.join().unwrap();
            });
            return;
        }
        let mut engine = crate::audio_engine::AudioEngine::new().unwrap();
        let (go_tx, go_rx) = mpsc::channel();
        let (before_tx, before_rx) = mpsc::channel();
        let retired = Arc::new(AtomicBool::new(false));
        let attached = Arc::new(AtomicBool::new(false));
        let worker_retired = retired.clone();
        let worker_attached = attached.clone();
        let reservation = engine.cold_jobs.reserve().unwrap();
        engine
            .cold_jobs
            .submit(reservation, move || {
                struct Retired(Arc<AtomicBool>);
                impl Drop for Retired {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::Release);
                    }
                }
                let _retired = Retired(worker_retired);
                go_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                before_tx.send(()).unwrap();
                pyo3::Python::attach(|_| worker_attached.store(true, Ordering::Release));
            })
            .unwrap();
        pyo3::Python::attach(|_| {
            go_tx.send(()).unwrap();
            before_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            if mode == "shutdown" {
                engine.shut_down().unwrap();
                engine.shut_down().unwrap();
                assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
                assert!(engine.cold_jobs.reserve().is_err());
            } else {
                drop(engine);
            }
            assert!(attached.load(Ordering::Acquire));
            assert!(retired.load(Ordering::Acquire));
        });
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum OwnedEvent {
        KernelStarted,
        CancelPolled,
        PoisonPolled,
        OwnedResultDropped,
        CallerDone { error: bool, drop_seen: bool },
    }

    struct OwnedDropWitness {
        dropped: Arc<AtomicBool>,
        events: mpsc::Sender<OwnedEvent>,
    }

    impl Drop for OwnedDropWitness {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
            let _ = self.events.send(OwnedEvent::OwnedResultDropped);
        }
    }

    #[test]
    fn terminal_owned_cancellation_waits_for_running_kernel_and_drops_result_before_return() {
        let mut lane = ColdJobs::new().unwrap();
        let handle = lane.handle();
        let cancelled = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let (events, observations) = mpsc::channel();
        let (release, kernel_release) = mpsc::channel();
        let cancel_flag = cancelled.clone();
        let caller_dropped = dropped.clone();
        let caller_events = events.clone();
        let cancel_events = events.clone();
        let kernel_events = events.clone();
        let kernel_dropped = dropped.clone();
        let caller = thread::spawn(move || {
            let result = handle.execute_owned_until_terminal(
                move || {
                    let cancelled = cancel_flag.load(Ordering::Acquire);
                    if cancelled {
                        let _ = cancel_events.send(OwnedEvent::CancelPolled);
                    }
                    cancelled
                },
                move || {
                    let owned = OwnedDropWitness {
                        dropped: kernel_dropped,
                        events: kernel_events.clone(),
                    };
                    let _ = kernel_events.send(OwnedEvent::KernelStarted);
                    kernel_release
                        .recv()
                        .map_err(|_| "kernel release disconnected")?;
                    Ok(owned)
                },
            );
            let error = result.is_err();
            let drop_seen = caller_dropped.load(Ordering::Acquire);
            let _ = caller_events.send(OwnedEvent::CallerDone { error, drop_seen });
            (error, drop_seen)
        });

        // These are event-order barriers, not a caller-latency measurement. Even a failed
        // observation always releases and joins both owners before making assertions.
        let started = observations.recv_timeout(Duration::from_secs(5));
        cancelled.store(true, Ordering::Release);
        let first = observations.recv_timeout(Duration::from_secs(5));
        let second = observations.recv_timeout(Duration::from_secs(5));
        let _ = release.send(());
        let joined = caller.join();
        lane.shutdown();
        let mut history: Vec<_> = [started, first, second].into_iter().flatten().collect();
        history.extend(observations.try_iter());

        assert_eq!(history.first(), Some(&OwnedEvent::KernelStarted));
        assert_eq!(history.get(1), Some(&OwnedEvent::CancelPolled));
        assert_eq!(history.get(2), Some(&OwnedEvent::CancelPolled));
        assert_eq!(joined.unwrap(), (true, true));
        let result_drop = history
            .iter()
            .position(|event| *event == OwnedEvent::OwnedResultDropped)
            .unwrap();
        let returned = history
            .iter()
            .position(|event| matches!(event, OwnedEvent::CallerDone { .. }))
            .unwrap();
        assert!(
            result_drop < returned,
            "cancelled owned result must retire before caller completion: {history:?}"
        );
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(lane.counts_for_test(), (0, 0, 0));
    }

    #[test]
    fn terminal_owned_queued_shutdown_drops_captured_owner_before_disconnection() {
        let mut lane = ColdJobs::new().unwrap();
        let (started_sender, started_receiver) = mpsc::channel();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        for _ in 0..WORKERS {
            let gate = gate.clone();
            let started_sender = started_sender.clone();
            lane.submit(lane.reserve().unwrap(), move || {
                let _ = started_sender.send(());
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            })
            .unwrap();
        }
        let started: Vec<_> = (0..WORKERS)
            .map(|_| started_receiver.recv_timeout(Duration::from_secs(5)))
            .collect();
        let handle = lane.handle();
        let dropped = Arc::new(AtomicBool::new(false));
        let ran = Arc::new(AtomicBool::new(false));
        let (events, observations) = mpsc::channel();
        let owner = OwnedDropWitness {
            dropped: dropped.clone(),
            events: events.clone(),
        };
        let caller_dropped = dropped.clone();
        let kernel_ran = ran.clone();
        let caller = thread::spawn(move || {
            let result = handle.execute_owned_until_terminal(
                || false,
                move || {
                    let _owner = owner;
                    kernel_ran.store(true, Ordering::Release);
                    Ok(())
                },
            );
            let error = result.is_err();
            let drop_seen = caller_dropped.load(Ordering::Acquire);
            let _ = events.send(OwnedEvent::CallerDone { error, drop_seen });
            (error, drop_seen)
        });
        // The queue's existing condition variable witnesses actual admission while both
        // workers remain occupied. A five-second timeout only bounds a broken test setup.
        let queued = match lane.shared.state.lock() {
            Ok(state) => lane
                .shared
                .ready
                .wait_timeout_while(state, Duration::from_secs(5), |state| state.jobs.is_empty())
                .map(|(state, _)| !state.jobs.is_empty())
                .unwrap_or(false),
            Err(_) => false,
        };
        lane.close_admission();
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let joined = caller.join();
        lane.shutdown();
        let history: Vec<_> = observations.try_iter().collect();

        assert!(started.iter().all(Result::is_ok));
        assert!(
            queued,
            "the owned kernel must really enter the existing queue"
        );
        assert_eq!(joined.unwrap(), (true, true));
        assert!(!ran.load(Ordering::Acquire));
        assert_eq!(history.first(), Some(&OwnedEvent::OwnedResultDropped));
        assert_eq!(
            history.get(1),
            Some(&OwnedEvent::CallerDone {
                error: true,
                drop_seen: true
            })
        );
        assert_eq!(lane.counts_for_test(), (0, 0, 0));
    }

    #[test]
    fn terminal_owned_poison_retires_queued_captures_before_return() {
        for cancel_on_poison in [false, true] {
            let mut lane = ColdJobs::new().unwrap();
            let gate = Arc::new((Mutex::new(false), Condvar::new()));
            let (started_sender, started_receiver) = mpsc::channel();
            for _ in 0..WORKERS {
                let gate = gate.clone();
                let started_sender = started_sender.clone();
                lane.submit(lane.reserve().unwrap(), move || {
                    let _ = started_sender.send(());
                    let mut open = gate.0.lock().unwrap();
                    while !*open {
                        open = gate.1.wait(open).unwrap();
                    }
                })
                .unwrap();
            }
            let started: Vec<_> = (0..WORKERS)
                .map(|_| started_receiver.recv_timeout(Duration::from_secs(5)))
                .collect();
            let handle = lane.handle();
            let dropped = Arc::new(AtomicBool::new(false));
            let poisoned = Arc::new(AtomicBool::new(false));
            let (events, observations) = mpsc::channel();
            let owner = OwnedDropWitness {
                dropped: dropped.clone(),
                events: events.clone(),
            };
            let caller_dropped = dropped.clone();
            let poison_observed = poisoned.clone();
            let poison_events = events.clone();
            let ran = Arc::new(AtomicBool::new(false));
            let kernel_ran = ran.clone();
            let caller = thread::spawn(move || {
                let result = handle.execute_owned_until_terminal(
                    move || {
                        let observed_poison = poison_observed.load(Ordering::Acquire);
                        if observed_poison {
                            let _ = poison_events.send(OwnedEvent::PoisonPolled);
                        }
                        cancel_on_poison && observed_poison
                    },
                    move || {
                        let _owner = owner;
                        kernel_ran.store(true, Ordering::Release);
                        Ok(())
                    },
                );
                let result = (result.is_err(), caller_dropped.load(Ordering::Acquire));
                let _ = events.send(OwnedEvent::CallerDone {
                    error: result.0,
                    drop_seen: result.1,
                });
                result
            });
            let queued = match lane.shared.state.lock() {
                Ok(state) => lane
                    .shared
                    .ready
                    .wait_timeout_while(state, Duration::from_secs(5), |state| {
                        state.jobs.is_empty()
                    })
                    .map(|(state, _)| !state.jobs.is_empty())
                    .unwrap_or(false),
                Err(_) => false,
            };
            let poison_shared = lane.shared.clone();
            let poisoner = thread::spawn(move || {
                let _guard = poison_shared.state.lock().unwrap();
                poisoned.store(true, Ordering::Release);
                panic!("injected queued owned-delivery poison");
            });
            let was_poisoned = poisoner.join().is_err();
            // Two events distinguish actual queued retirement from another waiter poll.
            // Even the broken implementation is explicitly drained before joins/assertions.
            let before_cleanup: Vec<_> = (0..2)
                .filter_map(|_| observations.recv_timeout(Duration::from_secs(5)).ok())
                .collect();
            let leftovers = {
                let mut state = lane
                    .shared
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.stopped = true;
                lane.shared.ready.notify_all();
                std::mem::take(&mut state.jobs)
            };
            drop(leftovers);
            *gate.0.lock().unwrap() = true;
            gate.1.notify_all();
            let joined = caller.join();
            lane.shutdown();
            let mut history = before_cleanup.clone();
            history.extend(observations.try_iter());
            assert!(started.iter().all(Result::is_ok));
            assert!(queued);
            assert!(was_poisoned);
            assert!(
                before_cleanup.contains(&OwnedEvent::OwnedResultDropped),
                "strict poison observation must itself retire queued custody: {before_cleanup:?}"
            );
            assert_eq!(joined.unwrap(), (true, true));
            assert!(!ran.load(Ordering::Acquire));
            let retired = history
                .iter()
                .position(|event| *event == OwnedEvent::OwnedResultDropped)
                .unwrap();
            let returned = history
                .iter()
                .position(|event| matches!(event, OwnedEvent::CallerDone { .. }))
                .unwrap();
            assert!(retired < returned);
            assert!(lane.reserve().is_err(), "poisoned admission remains closed");
        }
    }

    #[test]
    fn terminal_owned_nested_kernel_stays_inline_and_retires_cancelled_result() {
        let lane = ColdJobs::new().unwrap();
        let handle = lane.handle();
        let nested = handle.clone();
        let shared = lane.shared.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_poll = cancelled.clone();
        let dropped = Arc::new(AtomicBool::new(false));
        let (events, observations) = mpsc::channel();
        let owner = OwnedDropWitness {
            dropped: dropped.clone(),
            events,
        };
        let result = handle.execute_owned(
            || false,
            move || {
                nested.execute_owned_until_terminal(
                    move || cancel_poll.load(Ordering::Acquire),
                    move || {
                        assert_eq!(ON_WORKER.with(Cell::get), Arc::as_ptr(&shared) as usize);
                        {
                            let state = shared.state.lock().unwrap();
                            assert_eq!(state.active, 1);
                            assert_eq!(state.admitted, 1);
                            assert!(state.jobs.is_empty());
                        }
                        cancelled.store(true, Ordering::Release);
                        Ok(owner)
                    },
                )
            },
        );
        assert_eq!(
            result.err().as_deref(),
            Some("cold preparation proposal cancelled while waiting")
        );
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(
            observations.recv_timeout(Duration::from_secs(5)).unwrap(),
            OwnedEvent::OwnedResultDropped
        );
    }

    #[test]
    fn admission_and_worker_limits_are_real_and_release_on_terminal() {
        let mut lane = ColdJobs::new().unwrap();
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        for index in 0..WORKERS + QUEUED_JOBS {
            if index == WORKERS {
                for _ in 0..WORKERS {
                    rx.recv_timeout(Duration::from_secs(5)).unwrap();
                }
            }
            let (active, peak, gate, tx) = (active.clone(), peak.clone(), gate.clone(), tx.clone());
            lane.submit(lane.reserve().unwrap(), move || {
                let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(count, Ordering::SeqCst);
                tx.send(()).unwrap();
                let (lock, ready) = &*gate;
                let mut open = lock.lock().unwrap();
                while !*open {
                    open = ready.wait(open).unwrap();
                }
                active.fetch_sub(1, Ordering::SeqCst);
            })
            .unwrap();
        }
        assert_eq!(
            lane.reserve().err().as_deref(),
            Some("cold source queue full (2 workers, 32 queued jobs)")
        );
        assert_eq!(peak.load(Ordering::SeqCst), WORKERS);
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        lane.shutdown();
        assert_eq!(lane.shared.state.lock().unwrap().admitted, 0);
        assert_eq!(
            lane.reserve().err().as_deref(),
            Some("cold source lane stopped")
        );
    }

    #[test]
    fn close_admission_retires_queued_ownership_before_active_jobs_are_released() {
        struct QueuedOwner(Arc<AtomicUsize>);
        impl Drop for QueuedOwner {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let mut lane = ColdJobs::new().unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        for _ in 0..WORKERS {
            let (gate, tx) = (gate.clone(), tx.clone());
            lane.submit(lane.reserve().unwrap(), move || {
                tx.send(()).unwrap();
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            })
            .unwrap();
        }
        for _ in 0..WORKERS {
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let ran = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let owner = QueuedOwner(dropped.clone());
        let ran_job = ran.clone();
        lane.submit(lane.reserve().unwrap(), move || {
            let _owner = owner;
            ran_job.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        let reserved_before_close = lane.reserve().unwrap();
        assert_eq!(lane.counts_for_test(), (WORKERS, 1, WORKERS + 2));

        // Active jobs remain held while queued job destructors release reservations.
        lane.close_admission();
        assert_eq!(lane.counts_for_test(), (WORKERS, 0, WORKERS + 1));
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(ran.load(Ordering::SeqCst), 0);
        assert_eq!(
            lane.reserve().err().as_deref(),
            Some("cold source lane stopped")
        );
        assert!(lane.submit(reserved_before_close, || {}).is_err());
        assert_eq!(lane.counts_for_test(), (WORKERS, 0, WORKERS));

        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        lane.shutdown();
        assert_eq!(lane.counts_for_test(), (0, 0, 0));
        assert_eq!(ran.load(Ordering::SeqCst), 0);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn stopped_submission_releases_reservation_without_deadlock() {
        let mut lane = ColdJobs::new().unwrap();
        let reservation = lane.reserve().unwrap();
        lane.shutdown();
        assert!(lane.submit(reservation, || {}).is_err());
        assert_eq!(lane.shared.state.lock().unwrap().admitted, 0);
    }

    #[test]
    fn unsubmitted_reservations_and_panics_release_capacity() {
        let lane = ColdJobs::new().unwrap();
        drop(lane.reserve().unwrap());
        let (tx, rx) = mpsc::channel();
        lane.submit(lane.reserve().unwrap(), move || {
            tx.send(()).unwrap();
            panic!("injected worker failure");
        })
        .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (tx, rx) = mpsc::channel();
        lane.submit(lane.reserve().unwrap(), move || {
            tx.send(()).unwrap();
        })
        .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn direct_owned_preparation_runs_on_existing_worker_and_nested_kernel_stays_inline() {
        use crate::audio_engine::native_owned_work::{
            WorkPoint, observe_preparation, set_preparation_observer,
        };

        let lane = ColdJobs::new().unwrap();
        let handle = lane.handle();
        let nested = handle.clone();
        let shared = lane.shared.clone();
        let caller = thread::current().id();
        let (observer_sender, observer_receiver) = mpsc::channel();
        set_preparation_observer(Some(Box::new(move |point| {
            observer_sender
                .send((point, thread::current().id()))
                .unwrap();
        })));
        let value = handle
            .execute_owned(
                || false,
                move || {
                    assert_eq!(ON_WORKER.with(Cell::get), Arc::as_ptr(&shared) as usize);
                    assert_eq!(shared.state.lock().unwrap().active, 1);
                    observe_preparation(WorkPoint::LegacyStemFiles);
                    nested.execute_owned(
                        || false,
                        || {
                            assert_ne!(ON_WORKER.with(Cell::get), 0);
                            observe_preparation(WorkPoint::StemPcmFlush);
                            Ok(37)
                        },
                    )
                },
            )
            .unwrap();
        assert_eq!(value, 37);
        let (outer, outer_thread) = observer_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let (inner, inner_thread) = observer_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(outer, WorkPoint::LegacyStemFiles);
        assert_eq!(inner, WorkPoint::StemPcmFlush);
        assert_eq!(outer_thread, inner_thread);
        assert_ne!(outer_thread, caller);
        assert!(matches!(
            observer_receiver.recv_timeout(Duration::from_secs(5)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        assert_eq!(
            handle
                .execute_owned(
                    || false,
                    || {
                        observe_preparation(WorkPoint::LegacyStemFiles);
                        Ok(43)
                    },
                )
                .unwrap(),
            43
        );
    }

    #[test]
    fn direct_owned_closed_lane_and_cancel_reject_without_running_kernel() {
        let mut lane = ColdJobs::new().unwrap();
        let ran = Arc::new(AtomicUsize::new(0));
        let cancelled_ran = ran.clone();
        assert!(
            lane.handle()
                .execute_owned(
                    || true,
                    move || {
                        cancelled_ran.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                )
                .is_err()
        );
        lane.shutdown();
        let closed_ran = ran.clone();
        assert!(
            lane.handle()
                .execute_owned(
                    || false,
                    move || {
                        closed_ran.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                )
                .is_err()
        );
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }
}
