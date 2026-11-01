//! Cancellation retires only an unclaimed original, including its captured readers.

use super::*;
use std::sync::mpsc;
use std::time::Instant;

struct HeldCapture {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    dropped: Arc<AtomicBool>,
}

impl Drop for HeldCapture {
    fn drop(&mut self) {
        let _ = self.entered.send(());
        let _ = self.release.recv_timeout(Duration::from_secs(10));
        self.dropped.store(true, Ordering::Release);
    }
}

fn queued(lane: &ColdJobs) -> bool {
    let state = lane.shared.state.lock().unwrap();
    lane.shared
        .ready
        .wait_timeout_while(state, Duration::from_secs(5), |state| state.jobs.is_empty())
        .map(|(state, _)| !state.jobs.is_empty())
        .unwrap_or(false)
}

fn poisoned_shutdown_retires_queued_owner(with_retirement: bool) {
    let mut lane = ColdJobs::new().unwrap();
    let running = Arc::new(());
    let workers = Arc::new((Mutex::new(false), Condvar::new()));
    let (entered, started) = mpsc::channel();
    for _ in 0..WORKERS {
        let running = running.clone();
        let workers = workers.clone();
        let entered = entered.clone();
        lane.submit(lane.reserve().unwrap(), move || {
            let _running = running;
            entered.send(()).unwrap();
            let mut released = workers.0.lock().unwrap();
            while !*released {
                released = workers.1.wait(released).unwrap();
            }
        })
        .unwrap();
    }
    for _ in 0..WORKERS {
        started.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    let gate = Arc::new(super::super::heavy_work::Gate::default());
    let owner = gate.ticket().unwrap();
    assert!(owner.try_acquire().unwrap());
    let successor = gate.ticket().unwrap();
    let capture = Arc::new(());
    let captured = Arc::downgrade(&capture);
    let ran = Arc::new(AtomicBool::new(false));
    let executed = ran.clone();
    let payload = move || {
        executed.store(true, Ordering::Release);
        drop(capture);
    };
    let (callback, callback_started) = mpsc::channel();
    let (release, callback_release) = mpsc::channel();
    owner
        .with_current(|| {
            let reservation = lane.reserve().unwrap();
            if with_retirement {
                let observed = captured.clone();
                let shared = lane.shared.clone();
                lane.submit_analysis_cancellable(
                    reservation,
                    true,
                    || false,
                    move |job| {
                        let off_lock = match shared.state.try_lock() {
                            Ok(guard) => {
                                drop(guard);
                                true
                            }
                            Err(std::sync::TryLockError::Poisoned(error)) => {
                                drop(error.into_inner());
                                true
                            }
                            Err(std::sync::TryLockError::WouldBlock) => false,
                        };
                        drop(job);
                        callback.send((off_lock, observed.strong_count())).unwrap();
                        callback_release
                            .recv_timeout(Duration::from_secs(5))
                            .unwrap();
                    },
                    payload,
                )
                .unwrap();
            } else {
                lane.submit_heavy_cancellable(reservation, || false, payload)
                    .unwrap();
            }
        })
        .unwrap();
    drop(owner);
    assert_eq!(lane.counts_for_test(), (WORKERS, 1, WORKERS + 1));
    assert!(!successor.try_acquire().unwrap());
    assert!(
        std::panic::catch_unwind(|| {
            let _state = lane.shared.state.lock().unwrap();
            panic!("controlled queued shutdown poison");
        })
        .is_err()
    );
    let shared = lane.shared.clone();
    let counts = || {
        let state = shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            state.active,
            state.jobs.len(),
            state.admitted,
            state.stopped,
        )
    };
    let callback_boundary = thread::scope(|scope| {
        let close = scope.spawn(|| lane.close_admission());
        let boundary = with_retirement.then(|| {
            let receipt = callback_started.recv_timeout(Duration::from_secs(5));
            let retained = !successor.try_acquire().unwrap();
            let during = counts();
            let _ = release.send(());
            (receipt, retained, during)
        });
        close.join().unwrap();
        boundary
    });
    let after = counts();
    let queued_owners = captured.strong_count();
    let running_owners = Arc::strong_count(&running);
    let next_admitted = successor.try_acquire().unwrap();
    let closed = lane.ensure_open().is_err() && lane.reserve().is_err();
    let poisoned = lane.shared.state.is_poisoned();

    // Repair only this fixture after taking RED facts, so the unchanged-source
    // failure cannot leak captured owners or strand a test worker at teardown.
    let retained = {
        let mut state = lane
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stopped = true;
        std::mem::take(&mut state.jobs)
    };
    drop(retained);
    *workers.0.lock().unwrap() = true;
    workers.1.notify_all();
    lane.shutdown();

    assert_eq!(after, (WORKERS, 0, WORKERS, true));
    assert_eq!(
        queued_owners, 0,
        "poisoned shutdown retained queued captures"
    );
    assert_eq!(
        running_owners,
        WORKERS + 1,
        "shutdown released claimed readers"
    );
    assert!(next_admitted, "poisoned shutdown retained the heavy owner");
    assert!(
        closed && poisoned,
        "cleanup must not revive poisoned admission"
    );
    assert!(
        !ran.load(Ordering::Acquire),
        "shutdown executed queued work"
    );
    assert_eq!(counts(), (0, 0, 0, true));
    assert_eq!(Arc::strong_count(&running), 1);
    if let Some((receipt, retained, during)) = callback_boundary {
        assert_eq!(receipt.unwrap(), (true, 0));
        assert!(retained, "heavy admission escaped queued terminal cleanup");
        assert_eq!(during, (WORKERS, 0, WORKERS + 1, true));
    }
}

#[test]
fn poisoned_shutdown_retires_plain_queued_capture_and_heavy_owner() {
    poisoned_shutdown_retires_queued_owner(false);
}

#[test]
fn poisoned_shutdown_waits_queued_terminal_cleanup_before_releasing_admission() {
    poisoned_shutdown_retires_queued_owner(true);
}

#[test]
fn rejected_queue_never_invokes_retirement_under_callers_commit_lock() {
    let mut lane = ColdJobs::new().unwrap();
    let reservation = lane.reserve().unwrap();
    lane.close_admission();
    let commit = Arc::new(Mutex::new(()));
    let _commit = commit.lock().unwrap();
    let invoked = Arc::new(AtomicBool::new(false));
    let flag = invoked.clone();
    let callback_commit = commit.clone();
    let owner = Arc::new(());
    let capture = owner.clone();
    assert!(
        lane.handle()
            .submit_retiring_classified(
                reservation,
                move || drop(capture),
                None,
                None,
                Some(Box::new(move |job| {
                    flag.store(true, Ordering::Release);
                    // The real retirement callback needs this caller-owned lock.
                    // Detect an erroneous call without hanging a failing fixture.
                    drop(callback_commit.try_lock());
                    drop(job);
                })),
            )
            .is_err()
    );
    assert!(!invoked.load(Ordering::Acquire));
    assert_eq!(Arc::strong_count(&owner), 1);
    assert_eq!(lane.counts_for_test(), (0, 0, 0));
    lane.shutdown();
}

#[test]
fn accepted_queue_retirement_waits_commit_then_drops_capture_before_callback_and_admission() {
    let mut lane = ColdJobs::new().unwrap();
    let blocker = super::super::heavy_work::ticket().unwrap();
    assert!(blocker.try_acquire().unwrap());
    let commit = Arc::new(Mutex::new(()));
    let committing = commit.lock().unwrap();
    let callback_commit = commit.clone();
    let owner = Arc::new(());
    let capture = owner.clone();
    let observed = owner.clone();
    let handle = lane.handle();
    let callback_handle = handle.clone();
    let (entered, started) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let ticket = super::super::heavy_work::ticket().unwrap();
    handle
        .submit_retiring_classified(
            lane.reserve().unwrap(),
            move || drop(capture),
            Some(ticket),
            None,
            Some(Box::new(move |job| {
                entered.send(()).unwrap();
                drop(callback_commit.lock().unwrap());
                drop(job);
                done.send((
                    Arc::strong_count(&observed),
                    callback_handle.reserve().is_ok(),
                ))
                .unwrap();
            })),
        )
        .unwrap();
    let (held, before, premature, owners, queue_open) = thread::scope(|scope| {
        let closer = scope.spawn(|| lane.close_admission());
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        let held = Arc::strong_count(&owner);
        let before = lane.counts_for_test();
        let premature = finished.try_recv().is_ok();
        drop(committing);
        let (owners, queue_open) = finished.recv_timeout(Duration::from_secs(5)).unwrap();
        closer.join().unwrap();
        (held, before, premature, owners, queue_open)
    });
    assert!(!premature);
    assert_eq!(held, 3);
    assert_eq!(before, (0, 0, 1));
    assert_eq!(owners, 2);
    assert!(!queue_open, "callback reentry did not observe closed lane");
    assert_eq!(Arc::strong_count(&owner), 1);
    assert_eq!(lane.counts_for_test(), (0, 0, 0));
    assert!(blocker.is_active());
    drop(blocker);
    lane.shutdown();
}

#[test]
fn claimed_payload_disarms_queued_retirement() {
    let mut lane = ColdJobs::new().unwrap();
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let invoked = Arc::new(AtomicBool::new(false));
    let flag = invoked.clone();
    lane.submit_analysis_cancellable(
        lane.reserve().unwrap(),
        false,
        || false,
        move |job| {
            flag.store(true, Ordering::Release);
            drop(job);
        },
        move || {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(5)).unwrap();
        },
    )
    .unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    lane.close_admission();
    assert!(!invoked.load(Ordering::Acquire));
    assert_eq!(lane.counts_for_test(), (1, 0, 1));
    release.send(()).unwrap();
    lane.shutdown();
    assert!(!invoked.load(Ordering::Acquire));
    assert_eq!(lane.counts_for_test(), (0, 0, 0));
}

#[test]
fn terminal_queued_heavy_cancellation_drops_before_predecessor_ends() {
    let mut lane = ColdJobs::new().unwrap();
    let blocker = super::super::heavy_work::ticket().unwrap();
    assert!(blocker.try_acquire().unwrap());
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = cancelled.clone();
    let dropped = Arc::new(AtomicBool::new(false));
    let ran = Arc::new(AtomicBool::new(false));
    let kernel_ran = ran.clone();
    let (drop_tx, drop_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let capture = HeldCapture {
        entered: drop_tx,
        release: release_rx,
        dropped: dropped.clone(),
    };
    let (done_tx, done_rx) = mpsc::channel();
    let handle = lane.handle();
    let caller = thread::spawn(move || {
        let result = handle.execute_heavy_owned_until_terminal(
            || cancel.load(Ordering::Acquire),
            move || {
                let _capture = capture;
                kernel_ran.store(true, Ordering::Release);
                Ok(())
            },
        );
        let _ = done_tx.send(result.is_err());
    });
    let admitted = queued(&lane);
    let successor = super::super::heavy_work::ticket().unwrap();
    cancelled.store(true, Ordering::Release);
    let dropping_before_release = drop_rx.recv_timeout(Duration::from_secs(5)).is_ok();

    // A destructor may need the queue lock itself. Observe that lock from another
    // thread so even an erroneous under-lock drop is released and joined below.
    let (probe_tx, probe_rx) = mpsc::channel();
    let probe_handle = lane.handle();
    let probe = thread::spawn(move || {
        let reservation = probe_handle.reserve().unwrap();
        let _ = probe_tx.send(());
        drop(reservation);
    });
    let queue_available = probe_rx.recv_timeout(Duration::from_secs(5)).is_ok();
    let caller_waited = done_rx.try_recv().is_err();
    drop(blocker);
    let retained_during_drop = !successor.try_acquire().unwrap();
    let _ = release_tx.send(());
    let returned = done_rx.recv_timeout(Duration::from_secs(5));
    let joined = caller.join();
    let probe_joined = probe.join();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !successor.try_acquire().unwrap() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    let successor_admitted = successor.try_acquire().unwrap();
    lane.shutdown();

    assert!(admitted, "the original must actually enter the cold queue");
    assert!(
        dropping_before_release,
        "cancellation waited for the heavy predecessor"
    );
    assert!(
        queue_available,
        "capture destruction held the admission mutex"
    );
    assert!(
        caller_waited,
        "cancellation returned before captured readers ended"
    );
    assert!(
        retained_during_drop,
        "heavy capacity escaped captured-owner retirement"
    );
    assert!(returned.unwrap());
    assert!(joined.is_ok());
    assert!(probe_joined.is_ok());
    assert!(!ran.load(Ordering::Acquire), "cancelled queued payload ran");
    assert!(dropped.load(Ordering::Acquire));
    assert!(successor_admitted);
    assert_eq!(lane.counts_for_test(), (0, 0, 0));
}

#[test]
fn terminal_queued_cancellation_preserves_an_independent_waiter() {
    let mut lane = ColdJobs::new().unwrap();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (started_tx, started_rx) = mpsc::channel();
    for _ in 0..WORKERS {
        let gate = gate.clone();
        let started = started_tx.clone();
        lane.submit(lane.reserve().unwrap(), move || {
            let _ = started.send(());
            let mut open = gate.0.lock().unwrap();
            while !*open {
                open = gate.1.wait(open).unwrap();
            }
        })
        .unwrap();
    }
    let started: Vec<_> = (0..WORKERS)
        .map(|_| started_rx.recv_timeout(Duration::from_secs(5)))
        .collect();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = cancelled.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let handle = lane.handle();
    let caller = thread::spawn(move || {
        let result = handle.execute_owned_until_terminal::<()>(
            || cancel.load(Ordering::Acquire),
            || panic!("cancelled queued operation must never run"),
        );
        let _ = done_tx.send(result.is_err());
    });
    let admitted = queued(&lane);
    let (peer_tx, peer_rx) = mpsc::channel();
    lane.submit(lane.reserve().unwrap(), move || {
        let _ = peer_tx.send(());
    })
    .unwrap();
    cancelled.store(true, Ordering::Release);
    let before_workers_release = done_rx.recv_timeout(Duration::from_secs(5));
    let counts = lane.counts_for_test();
    let peer_waited = peer_rx.try_recv().is_err();
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    let joined = caller.join();
    let peer = peer_rx.recv_timeout(Duration::from_secs(5));
    lane.shutdown();

    assert!(started.iter().all(Result::is_ok));
    assert!(admitted);
    assert!(before_workers_release.unwrap());
    assert_eq!(counts, (WORKERS, 1, WORKERS + 1));
    assert!(peer_waited);
    assert!(peer.is_ok());
    assert!(joined.is_ok());
    assert_eq!(lane.counts_for_test(), (0, 0, 0));
}

fn occupy_workers(lane: &ColdJobs) -> Vec<mpsc::Sender<()>> {
    (0..WORKERS)
        .map(|_| {
            let (entered, started) = mpsc::channel();
            let (release, resumed) = mpsc::channel();
            lane.submit(lane.reserve().unwrap(), move || {
                entered.send(()).unwrap();
                let _ = resumed.recv_timeout(Duration::from_secs(10));
            })
            .unwrap();
            started.recv_timeout(Duration::from_secs(5)).unwrap();
            release
        })
        .collect()
}

struct ProbeOwner {
    lane: ColdJobsHandle,
    reentered: Arc<AtomicBool>,
    _capture: Mutex<HeldCapture>,
}

impl Drop for ProbeOwner {
    fn drop(&mut self) {
        drop(self.lane.reserve().unwrap());
        self.reentered.store(true, Ordering::Release);
    }
}

#[test]
fn async_observation_retains_whole_admission_through_final_predicate_drop() {
    let mut lane = ColdJobs::new().unwrap();
    let releases = occupy_workers(&lane);
    let blocker = super::super::heavy_work::ticket().unwrap();
    assert!(blocker.try_acquire().unwrap());
    let (entered, observing) = mpsc::channel();
    let (resume, resumed) = mpsc::channel();
    let resumed = Mutex::new(resumed);
    let first = AtomicBool::new(true);
    let reentered = Arc::new(AtomicBool::new(false));
    let (dropping, drop_started) = mpsc::channel();
    let (finish_drop, drop_resumed) = mpsc::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let predicate_owner = ProbeOwner {
        lane: lane.handle(),
        reentered: reentered.clone(),
        _capture: Mutex::new(HeldCapture {
            entered: dropping,
            release: drop_resumed,
            dropped: dropped.clone(),
        }),
    };
    let (finished, payload_ended) = mpsc::channel();
    lane.submit_heavy_cancellable(
        lane.reserve().unwrap(),
        move || {
            // The callback itself must also be able to reenter lane admission.
            let owner = &predicate_owner;
            drop(owner.lane.reserve().unwrap());
            if first.swap(false, Ordering::AcqRel) {
                entered.send(()).unwrap();
                let _ = resumed
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10));
            }
            false
        },
        move || {
            finished.send(()).unwrap();
        },
    )
    .unwrap();
    let handle = lane.handle();
    let observer = thread::spawn(move || handle.retire_cancelled());
    let observing = observing.recv_timeout(Duration::from_secs(5)).is_ok();
    for release in releases {
        let _ = release.send(());
    }
    drop(blocker);
    let completed = payload_ended.recv_timeout(Duration::from_secs(5)).is_ok();
    let (peer, peer_ran) = mpsc::channel();
    lane.submit(lane.reserve().unwrap(), move || {
        peer.send(()).unwrap();
    })
    .unwrap();
    let ordinary_progress = peer_ran.recv_timeout(Duration::from_secs(5)).is_ok();
    let successor = super::super::heavy_work::ticket().unwrap();
    let held_during_observation = !successor.try_acquire().unwrap();
    let _ = resume.send(());
    let final_drop = drop_started.recv_timeout(Duration::from_secs(5)).is_ok();
    let held_during_drop = !successor.try_acquire().unwrap();
    let still_admitted = lane.counts_for_test().2 != 0;
    let _ = finish_drop.send(());
    let joined = observer.join();
    lane.shutdown();
    let successor_admitted = successor.try_acquire().unwrap();

    assert!(observing && completed && ordinary_progress);
    assert!(held_during_observation && final_drop && held_during_drop && still_admitted);
    assert!(
        reentered.load(Ordering::Acquire),
        "predicate destructor held the queue mutex"
    );
    assert!(dropped.load(Ordering::Acquire) && joined.is_ok() && successor_admitted);
    assert_eq!(lane.counts_for_test(), (0, 0, 0));
}

#[test]
fn async_stale_positive_observation_cannot_retire_a_claimed_payload() {
    let mut lane = ColdJobs::new().unwrap();
    let releases = occupy_workers(&lane);
    let blocker = super::super::heavy_work::ticket().unwrap();
    assert!(blocker.try_acquire().unwrap());
    let (entered, observing) = mpsc::channel();
    let (resume, resumed) = mpsc::channel();
    let resumed = Mutex::new(resumed);
    let first = AtomicBool::new(true);
    let (payload, started) = mpsc::channel();
    let (finish, finished) = mpsc::channel();
    lane.submit_heavy_cancellable(
        lane.reserve().unwrap(),
        move || {
            if first.swap(false, Ordering::AcqRel) {
                entered.send(()).unwrap();
                let _ = resumed
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10));
                true
            } else {
                false
            }
        },
        move || {
            payload.send(()).unwrap();
            let _ = finished.recv_timeout(Duration::from_secs(10));
        },
    )
    .unwrap();
    let handle = lane.handle();
    let observer = thread::spawn(move || handle.retire_cancelled());
    let observed = observing.recv_timeout(Duration::from_secs(5)).is_ok();
    for release in releases {
        let _ = release.send(());
    }
    drop(blocker);
    let claimed = started.recv_timeout(Duration::from_secs(5)).is_ok();
    let successor = super::super::heavy_work::ticket().unwrap();
    let _ = resume.send(());
    let joined = observer.join();
    let deadline = Instant::now() + Duration::from_secs(5);
    while lane.counts_for_test() != (1, 0, 1) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    let still_running = lane.counts_for_test() == (1, 0, 1);
    let retained = !successor.try_acquire().unwrap();
    let (peer, ran) = mpsc::channel();
    lane.submit(lane.reserve().unwrap(), move || {
        peer.send(()).unwrap();
    })
    .unwrap();
    let peer_ran = ran.recv_timeout(Duration::from_secs(5)).is_ok();
    let _ = finish.send(());
    lane.shutdown();
    let successor_admitted = successor.try_acquire().unwrap();

    assert!(observed && claimed && joined.is_ok());
    assert!(still_running && retained && peer_ran && successor_admitted);
    assert_eq!(lane.counts_for_test(), (0, 0, 0));
}
