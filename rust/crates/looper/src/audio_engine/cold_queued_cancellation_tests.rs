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
