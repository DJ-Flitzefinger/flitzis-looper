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

fn queued_capture_after_public_stop(stopped_id: usize) {
    Python::initialize();
    let mut p = Productive::new(false, 0.73);
    wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    let samples = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap()
        .samples
        .clone();
    let owners_before = Arc::strong_count(&samples);
    let selection = p.pair.selection_json().to_owned();
    let source = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap()
        .resident_binding();
    let held = HeldWorkers::new(&p.h.engine);
    let pending = p.request("all_stems", 0b1111);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    let captured_owners = Arc::strong_count(&samples);
    let peer_runs = Arc::new(AtomicUsize::new(0));
    for _ in 0..QUEUED_JOBS - 1 {
        let ran = peer_runs.clone();
        p.h.engine
            .cold_jobs
            .submit(p.h.engine.cold_jobs.reserve().unwrap(), move || {
                ran.fetch_add(1, Ordering::AcqRel);
            })
            .unwrap();
    }
    let before = p.h.engine.cold_jobs.counts_for_test();
    let launch_before = p.h.engine.input_runtime_ownership.launch_revision(0);
    // The public API cancels launch authority before its queued callback STOP.
    // Pump in that real interval so voice retirement cannot hide the pin owner.
    p.h.engine.stop_sample(stopped_id).unwrap();
    let launch_after = p.h.engine.input_runtime_ownership.launch_revision(0);
    let before_status = pending.residency_status();
    let before_current = pending.is_current();
    Python::attach(|py| assert!(p.h.engine.poll_loader_events(py).unwrap().is_none()));
    let after = p.h.engine.cold_jobs.counts_for_test();
    let owners_after = Arc::strong_count(&samples);
    let status = pending.residency_status();
    let current = pending.is_current();
    let unchanged = p.selected.is_current()
        && p.pair.selection_json() == selection
        && p.h
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
        && p.h.callback.mixer.stems_for_measurement()[0].is_none()
        && p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .is_some_and(|sample| {
                Arc::ptr_eq(&sample.samples, &samples) && sample.resident_binding() == source
            });
    let peers_before_release = peer_runs.load(Ordering::Acquire);
    let stop_queued = p.h.consumer.peek().is_ok_and(|message| {
        matches!(message, ControlMessage::StopSampleForPlacement { placement }
            if placement.slot() == stopped_id)
    });
    let replacement_runs = Arc::new(AtomicUsize::new(0));
    let replacement_admitted = match p.h.engine.cold_jobs.reserve() {
        Ok(reservation) => {
            let ran = replacement_runs.clone();
            p.h.engine
                .cold_jobs
                .submit(reservation, move || {
                    ran.fetch_add(1, Ordering::AcqRel);
                })
                .unwrap();
            true
        }
        Err(_) => false,
    };
    eprintln!(
        "committed queued public STOP id={stopped_id}: before={before:?} after={after:?} \
         PCMowners={owners_before}/{captured_owners}/{owners_after} \
         status={before_status}/{status} current={before_current}/{current} \
         unchanged={unchanged} peer_before={peers_before_release} \
         replacement={replacement_admitted}"
    );
    // Consume the genuine STOP, then release and join every held worker before
    // final assertions. The unaffected control must still publish its own ACK.
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    drop(held);
    wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    if stopped_id != 0 {
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        assert_eq!(pending.residency_status(), "accepted");
        p.select_reference("all_stems", 0b1111);
        assert!(
            p.render(257, false)
                .iter()
                .any(|sample| sample.abs() > 0.001)
        );
    } else {
        assert!(p.h.consumer.is_empty());
        assert!(
            !p.h.callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
        );
    }
    p.h.engine.cold_jobs.shutdown();
    let stopped_target = stopped_id == 0;
    assert_eq!(captured_owners, owners_before + 1, "actual callback pin");
    assert_eq!(owners_after, owners_before + usize::from(!stopped_target));
    assert_eq!(before, (WORKERS, QUEUED_JOBS, WORKERS + QUEUED_JOBS));
    assert_eq!(launch_after, launch_before + u64::from(stopped_target));
    let queued_after = QUEUED_JOBS - usize::from(stopped_target);
    assert_eq!(after, (WORKERS, queued_after, WORKERS + queued_after));
    assert_eq!(before_status, "captured");
    assert_eq!(before_current, !stopped_target);
    assert_eq!(
        status,
        if stopped_target {
            "rejected"
        } else {
            "captured"
        }
    );
    assert_eq!(current, !stopped_target);
    assert!(unchanged);
    assert!(stop_queued);
    assert_eq!(peers_before_release, 0);
    assert_eq!(replacement_admitted, stopped_target);
    assert_eq!(peer_runs.load(Ordering::Acquire), QUEUED_JOBS - 1);
    assert_eq!(
        replacement_runs.load(Ordering::Acquire),
        usize::from(stopped_target)
    );
    assert!(p.selected.is_current());
    assert_eq!(p.pair.selection_json(), selection);
}

#[test]
fn public_stop_retires_queued_committed_capture_before_workers_or_callback_stop() {
    queued_capture_after_public_stop(0);
}

#[test]
fn other_pad_public_stop_preserves_queued_committed_capture_and_own_ack() {
    queued_capture_after_public_stop(215);
}
