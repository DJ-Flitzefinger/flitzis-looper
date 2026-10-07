//! Fixed cold-load lane. Reservations precede request mutations or PCM allocation.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

pub(super) const WORKERS: usize = 2;
pub(super) const QUEUED_JOBS: usize = 32;
pub(super) const PCM_LIMIT_BYTES: usize = 1024 * 1024 * 1024;
type Job = Box<dyn FnOnce() + Send>;
struct AdmittedJob {
    job: Job,
    reservation: Reservation,
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

/// An unused reservation rolls back without advancing source/request ownership.
pub(super) struct Reservation {
    shared: Arc<Shared>,
    running: bool,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.admitted -= 1;
            if self.running {
                state.active -= 1;
            }
        }
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
                        loop {
                            let job = {
                                let Ok(mut state) = shared.state.lock() else {
                                    return;
                                };
                                while state.jobs.is_empty() && !state.stopped {
                                    let Ok(next) = shared.ready.wait(state) else {
                                        return;
                                    };
                                    state = next;
                                }
                                if state.stopped {
                                    return;
                                }
                                {
                                    let mut job =
                                        state.jobs.pop_front().expect("nonempty bounded queue");
                                    state.active += 1;
                                    job.reservation.running = true;
                                    job
                                }
                            };
                            // A panic retires the reservation and does not destroy a worker slot.
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job.job));
                            drop(job.reservation);
                        }
                    })?,
            );
        }
        Ok(lane)
    }

    pub(super) fn reserve(&self) -> Result<Reservation, String> {
        let mut state = self
            .shared
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
            shared: self.shared.clone(),
            running: false,
        })
    }

    /// Enqueue under the same reserved admission. No thread is spawned per pad.
    pub(super) fn submit(
        &self,
        reservation: Reservation,
        job: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| "cold queue lock poisoned")?;
        if state.stopped {
            drop(state);
            return Err("cold lane stopped".into());
        }
        state.jobs.push_back(AdmittedJob {
            job: Box::new(job),
            reservation,
        });
        self.shared.ready.notify_one();
        Ok(())
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

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
}
