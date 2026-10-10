//! Freeze the actual guard immediately after its terminal Release, before join.
use super::super::cold_jobs::{ColdJobs, WORKERS};
use super::*;
use std::sync::{Condvar, mpsc};

pub(super) struct TerminalObservation {
    observed: mpsc::SyncSender<()>,
    resume: mpsc::Receiver<()>,
}

impl TerminalObservation {
    pub(super) fn observe(self) {
        // This hook also runs while a worker unwinds. Observer loss must not
        // panic in Drop; its sender disappearing releases this wait.
        if self.observed.send(()).is_ok() {
            let _ = self.resume.recv();
        }
    }
}

struct ActiveGateRelease(Arc<(Mutex<bool>, Condvar)>);

impl Drop for ActiveGateRelease {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

fn observed_state() -> (Arc<WindowState>, mpsc::Receiver<()>, mpsc::SyncSender<()>) {
    let (observed, observer) = mpsc::sync_channel(1);
    let (resume, resumer) = mpsc::sync_channel(1);
    let state = Arc::new(WindowState::default());
    *state.terminal_observation.lock().unwrap() = Some(TerminalObservation {
        observed,
        resume: resumer,
    });
    (state, observer, resume)
}

fn guard(state: &Arc<WindowState>, ownership: &Arc<InputRuntimeOwnership>) -> JobGuard {
    let publication = PreparedSourcePermit::unrestricted();
    ownership.begin_resident_control(0, publication.expected);
    JobGuard {
        publication,
        state: state.clone(),
        id: 0,
        ownership: ownership.clone(),
        enqueued: false,
    }
}

#[test]
fn queued_shutdown_terminal_observation_has_settled_only_its_exact_owner() {
    for replacement_before in [false, true] {
        let lane = ColdJobs::new().unwrap();
        let active_gate = Arc::new((Mutex::new(false), Condvar::new()));
        let _release_on_unwind = ActiveGateRelease(active_gate.clone());
        let (entered, started) = mpsc::channel();
        for _ in 0..WORKERS {
            let gate = active_gate.clone();
            let entered = entered.clone();
            lane.submit(lane.reserve().unwrap(), move || {
                entered.send(()).unwrap();
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            })
            .unwrap();
        }
        for _ in 0..WORKERS {
            started.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let (state, observed, resume) = observed_state();
        let ownership = Arc::new(InputRuntimeOwnership::default());
        let mut job = guard(&state, &ownership);
        let publication = job.publication.clone();
        let ran = Arc::new(AtomicBool::new(false));
        let ran_job = ran.clone();
        if replacement_before {
            ownership.begin_resident_control(0, publication.expected + 1);
        }
        lane.submit(lane.reserve().unwrap(), move || {
            ran_job.store(true, Ordering::Release);
            job.mark_enqueued();
        })
        .unwrap();
        let closing = std::thread::spawn(move || {
            lane.close_admission();
            lane
        });
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        let terminal = state.terminal.load(Ordering::Acquire);
        let pending_at_terminal = ownership.resident_control_pending(0);
        let status = publication.status();
        // A new owner arriving after terminal must also survive the old destructor.
        ownership.begin_resident_control(0, publication.expected + 2);
        resume.send(()).unwrap();
        let mut lane = closing.join().unwrap();
        let pending_after_drop = ownership.resident_control_pending(0);
        let counts = lane.counts_for_test();
        *active_gate.0.lock().unwrap() = true;
        active_gate.1.notify_all();
        lane.shutdown();
        // Release all gates before assertions: a negative run cannot deadlock Drop.
        assert_eq!(terminal, 2);
        assert_eq!(status, "rejected");
        assert_eq!(
            pending_at_terminal, replacement_before,
            "terminal was visible before its exact pending owner settled"
        );
        assert!(
            pending_after_drop,
            "old guard cleared the replacement owner"
        );
        assert_eq!(counts, (WORKERS, 0, WORKERS));
        assert_eq!(lane.counts_for_test(), (0, 0, 0));
        assert!(!ran.load(Ordering::Acquire));
    }
}

#[test]
fn worker_panic_terminal_observation_precedes_join_and_preserves_replacement() {
    for replacement_before in [false, true] {
        let mut lane = ColdJobs::new().unwrap();
        let (state, observed, resume) = observed_state();
        let ownership = Arc::new(InputRuntimeOwnership::default());
        let job = guard(&state, &ownership);
        let expected = job.publication.expected;
        if replacement_before {
            ownership.begin_resident_control(0, expected + 1);
        }
        lane.submit(lane.reserve().unwrap(), move || {
            let _job = job;
            panic!("injected preparation unwind before native enqueue");
        })
        .unwrap();
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        let terminal = state.terminal.load(Ordering::Acquire);
        let pending_at_terminal = ownership.resident_control_pending(0);
        ownership.begin_resident_control(0, expected + 2);
        resume.send(()).unwrap();
        lane.shutdown();
        assert_eq!(terminal, 2);
        assert_eq!(
            pending_at_terminal, replacement_before,
            "panic terminal preceded exact owner settlement"
        );
        assert!(ownership.resident_control_pending(0));
        assert_eq!(lane.counts_for_test(), (0, 0, 0));
    }
}

#[test]
fn stopped_submission_drops_guard_and_settles_owner_without_running_body() {
    let mut lane = ColdJobs::new().unwrap();
    let reservation = lane.reserve().unwrap();
    lane.close_admission();
    let state = Arc::new(WindowState::default());
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let mut job = guard(&state, &ownership);
    let ran = Arc::new(AtomicBool::new(false));
    let ran_job = ran.clone();
    assert!(
        lane.submit(reservation, move || {
            ran_job.store(true, Ordering::Release);
            job.mark_enqueued();
        })
        .is_err()
    );
    assert_eq!(state.terminal.load(Ordering::Acquire), 2);
    assert!(!ownership.resident_control_pending(0));
    assert!(!ran.load(Ordering::Acquire));
    lane.shutdown();
    assert_eq!(lane.counts_for_test(), (0, 0, 0));
}

#[test]
fn enqueued_or_claimed_guard_cannot_terminalize_or_clear_native_owned_work() {
    for claimed in [false, true] {
        let state = Arc::new(WindowState::default());
        let ownership = Arc::new(InputRuntimeOwnership::default());
        let mut job = guard(&state, &ownership);
        let publication = job.publication.clone();
        publication.mark_pending().unwrap();
        if claimed {
            assert!(publication.claim_resident());
        } else {
            job.mark_enqueued();
        }
        drop(job);
        assert_eq!(state.terminal.load(Ordering::Acquire), 0);
        assert!(ownership.resident_control_pending(0));
        assert_eq!(
            publication.status(),
            if claimed { "adopting" } else { "pending" }
        );
    }
}

#[test]
fn worker_failure_terminal_settles_scalar_owner_without_releasing_pcm_pin() {
    for cancelled in [false, true] {
        let mut lane = ColdJobs::new().unwrap();
        let (state, observed, resume) = observed_state();
        let ownership = Arc::new(InputRuntimeOwnership::default());
        let job = guard(&state, &ownership);
        let expected = job.publication.expected;
        let sample = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(vec![0.25; 1000]),
        }
        .with_complete_source(48_000);
        let pcm = Arc::downgrade(&sample.samples);
        let (read_value, read_result) = mpsc::channel();
        let read_gate = Arc::new((Mutex::new(false), Condvar::new()));
        let _release_on_unwind = ActiveGateRelease(read_gate.clone());
        let worker_gate = read_gate.clone();
        let worker_state = state.clone();
        let worker_ownership = ownership.clone();
        lane.submit(lane.reserve().unwrap(), move || {
            let _job = job;
            worker_state.fail(
                &worker_ownership,
                0,
                expected,
                "injected bounded preparation failure".into(),
                cancelled,
            );
            let mut open = worker_gate.0.lock().unwrap();
            while !*open {
                open = worker_gate.1.wait(open).unwrap();
            }
            // Actual worker-owned source PCM remains readable until its body ends.
            read_value.send(sample.samples[0]).unwrap();
        })
        .unwrap();
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        let terminal = state.terminal.load(Ordering::Acquire);
        let pending = ownership.resident_control_pending(0);
        let readable_at_terminal = pcm.upgrade().is_some();
        resume.send(()).unwrap();
        *read_gate.0.lock().unwrap() = true;
        read_gate.1.notify_all();
        lane.shutdown();
        assert_eq!(terminal, if cancelled { 2 } else { 1 });
        assert!(!pending);
        assert!(readable_at_terminal);
        assert_eq!(
            read_result.recv_timeout(Duration::from_secs(5)).unwrap(),
            0.25
        );
        assert!(pcm.upgrade().is_none());
        assert_eq!(lane.counts_for_test(), (0, 0, 0));
    }
}
