//! Unclaimed committed work must release its actual callback capture off realtime.
use super::*;
use crate::audio_engine::cold_jobs::{QUEUED_JOBS, WORKERS};
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;

struct HeldWorkers(Vec<mpsc::Sender<()>>);
impl HeldWorkers {
    fn new(engine: &AudioEngine) -> Self {
        let mut held = Self(Vec::new());
        for _ in 0..WORKERS {
            let (started, entered) = mpsc::channel();
            let (release, resumed) = mpsc::channel();
            engine
                .cold_jobs
                .submit(engine.cold_jobs.reserve().unwrap(), move || {
                    started.send(()).unwrap();
                    let _ = resumed.recv();
                })
                .unwrap();
            held.0.push(release);
            entered.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        held
    }
}
impl Drop for HeldWorkers {
    fn drop(&mut self) {
        for release in self.0.drain(..) {
            let _ = release.send(());
        }
    }
}

fn queued_capture(close: bool) {
    Python::initialize();
    let mut p = Productive::new(false, 0.73);
    wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    let samples = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap()
        .samples
        .clone();
    // This window still has legitimate mixer/control owners. Measure the actual
    // capture's extra reader; do not call its shared backing's last reader ended.
    let owners_before = Arc::strong_count(&samples);
    let held = HeldWorkers::new(&p.h.engine);
    let pending = p.request("all_stems", 0b1111);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    let captured_owners = Arc::strong_count(&samples);
    let peer_runs = Arc::new(AtomicUsize::new(0));
    let peers = if close { 1 } else { QUEUED_JOBS - 1 };
    for _ in 0..peers {
        let ran = peer_runs.clone();
        p.h.engine
            .cold_jobs
            .submit(p.h.engine.cold_jobs.reserve().unwrap(), move || {
                ran.fetch_add(1, Ordering::AcqRel);
            })
            .unwrap();
    }
    let admitted_before = p.h.engine.cold_jobs.counts_for_test();
    if close {
        // Uncancelled close is a positive control for the existing job guard.
        p.h.engine.cold_jobs.close_admission();
    } else {
        assert!(pending.cancel());
    }
    Python::attach(|py| assert!(p.h.engine.poll_loader_events(py).unwrap().is_none()));
    let after_pump = p.h.engine.cold_jobs.counts_for_test();
    let owners_after_pump = Arc::strong_count(&samples);
    let status = pending.residency_status();
    let effective = pending.effective_mode_status();
    let selected_current = p.selected.is_current();
    let full_mix_unchanged = p.h.callback.mixer.stems_for_measurement()[0].is_none();
    let peers_before_release = peer_runs.load(Ordering::Acquire);
    let replacement_runs = Arc::new(AtomicUsize::new(0));
    let replacement_admitted = if close {
        false
    } else {
        let ran = replacement_runs.clone();
        match p.h.engine.cold_jobs.reserve() {
            Ok(reservation) => {
                p.h.engine
                    .cold_jobs
                    .submit(reservation, move || {
                        ran.fetch_add(1, Ordering::AcqRel);
                    })
                    .unwrap();
                true
            }
            Err(_) => false,
        }
    };
    eprintln!(
        "committed queued close={close}: before={admitted_before:?} after={after_pump:?} \
         PCMowners={owners_before}/{captured_owners}/{owners_after_pump} \
         status={status}/{effective} selected={selected_current} \
         peer_before={peers_before_release} replacement={replacement_admitted}"
    );
    // Always release and join the real workers before evaluating the regression.
    drop(held);
    wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    p.h.engine.cold_jobs.shutdown();
    assert_eq!(captured_owners, owners_before + 1, "real callback capture");
    assert_eq!(
        owners_after_pump, owners_before,
        "queued capture reader ends"
    );
    assert_eq!(admitted_before, (WORKERS, peers + 1, WORKERS + peers + 1));
    assert_eq!(
        after_pump,
        if close {
            (WORKERS, 0, WORKERS)
        } else {
            (WORKERS, peers, WORKERS + peers)
        }
    );
    assert_eq!(status, "rejected");
    assert_eq!(effective, "rejected");
    assert!(selected_current);
    assert!(full_mix_unchanged);
    assert_eq!(peers_before_release, 0);
    assert_eq!(replacement_admitted, !close);
    assert_eq!(
        peer_runs.load(Ordering::Acquire),
        if close { 0 } else { peers }
    );
    assert_eq!(
        replacement_runs.load(Ordering::Acquire),
        usize::from(!close)
    );
}

#[test]
fn cancelled_unclaimed_committed_capture_releases_reader_and_full_lane_capacity() {
    queued_capture(false);
}

#[test]
fn uncancelled_close_retires_unclaimed_committed_capture_and_settles_guard() {
    queued_capture(true);
}
