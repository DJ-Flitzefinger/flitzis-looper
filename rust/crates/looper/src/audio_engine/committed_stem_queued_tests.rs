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

fn queued_capture_after_full_mix(admit_full_mix: bool) {
    use crate::audio_engine::committed_stem_residency::control_publication_tests::committed_pending_pcm_addresses;
    use crate::audio_engine::resident_relocation::set_stem_pair_full_mix_with_producer;

    Python::initialize();
    let mut p = Productive::new(false, 0.73);
    let initial = p.request("all_stems", 0b1111);
    p.capture_then_wait(&initial);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(initial.residency_status(), "accepted");
    assert!(initial.is_current());
    p.select_reference("all_stems", 0b1111);
    assert!(p.render(257, false).iter().any(|value| value.abs() > 0.001));
    reconcile(&p.h.engine).unwrap();
    wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));

    // Weak observations retain neither the original source nor any component reader.
    let samples = Arc::downgrade(
        &p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples,
    );
    let components = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .stems
        .each_ref()
        .map(|component| Arc::downgrade(&component.samples));
    let addresses = [
        samples.as_ptr() as *const f32 as usize,
        components[0].as_ptr() as *const f32 as usize,
        components[1].as_ptr() as *const f32 as usize,
        components[2].as_ptr() as *const f32 as usize,
        components[3].as_ptr() as *const f32 as usize,
    ];
    assert_eq!(
        committed_pending_pcm_addresses(&p.h.engine, 0, &initial),
        Some(addresses)
    );
    let owners_before = samples.strong_count();
    let components_before = components
        .each_ref()
        .map(|component| component.strong_count());
    let selection = p.pair.selection_json().to_owned();
    let source = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap()
        .resident_binding();
    let held = HeldWorkers::new(&p.h.engine);
    let pending = p.request("all_stems", 0b1010);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    let owners_captured = samples.strong_count();
    let components_captured = components
        .each_ref()
        .map(|component| component.strong_count());
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
    let intent_before = p.h.engine.resident_stem_cache.lock().unwrap()[0]
        .intent_epoch
        .load(Ordering::Acquire);
    let blocked_controls = if admit_full_mix {
        0
    } else {
        let mut producer = p.h.producer.lock().unwrap();
        let mut count = 0;
        while !producer.is_full() {
            producer.push(ControlMessage::Ping()).unwrap();
            count += 1;
        }
        count
    };
    // This is the exact helper used by the public AudioEngine FullMix API.
    // GUI committed-mode selection has a separate proposal-cancellation route.
    let error = set_stem_pair_full_mix_with_producer(&p.h.engine, 0, &p.h.producer)
        .err()
        .map(|error| error.to_string());
    let intent_after = p.h.engine.resident_stem_cache.lock().unwrap()[0]
        .intent_epoch
        .load(Ordering::Acquire);
    let owners_after_admission = samples.strong_count();
    let components_after_admission = components
        .each_ref()
        .map(|component| component.strong_count());
    Python::attach(|py| assert!(p.h.engine.poll_loader_events(py).unwrap().is_none()));
    let after = p.h.engine.cold_jobs.counts_for_test();
    let owners_after = samples.strong_count();
    let components_after = components
        .each_ref()
        .map(|component| component.strong_count());
    let original_mirror = committed_pending_pcm_addresses(&p.h.engine, 0, &initial);
    let status = pending.residency_status();
    let current = pending.is_current();
    let unchanged = p.selected.is_current()
        && p.pair.selection_json() == selection
        && p.h.callback.mixer.stem_demand_for_measurement(0).0 == StemMixMode::AllStems
        && p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .is_some_and(|sample| {
                std::ptr::eq(Arc::as_ptr(&sample.samples), samples.as_ptr())
                    && sample.resident_binding() == source
            });
    let peers_before_release = peer_runs.load(Ordering::Acquire);
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
        "committed queued FullMix={admit_full_mix}: before={before:?} after={after:?} \
         PCMowners={owners_before}/{owners_captured}/{owners_after_admission}/{owners_after} \
         components={components_before:?}/{components_captured:?}/{components_after_admission:?}/{components_after:?} \
         original_mirror={} intent={intent_before}/{intent_after} status={status} \
         current={current} unchanged={unchanged} peer_before={peers_before_release} \
         replacement={replacement_admitted} control_error={error:?}",
        original_mirror.is_some()
    );

    // Consume the actual FullMix command, or all original queue-full sentinels.
    assert_eq!(
        p.h.callback.drain(&mut p.h.consumer),
        if admit_full_mix { 1 } else { blocked_controls }
    );
    assert_eq!(
        p.h.callback.mixer.stem_demand_for_measurement(0).0,
        if admit_full_mix {
            StemMixMode::FullMix
        } else {
            StemMixMode::AllStems
        }
    );
    drop(held);
    wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    if admit_full_mix {
        p.select_reference("full_mix", 0b1111);
        assert!(p.render(257, false).iter().any(|value| value.abs() > 0.001));
        assert!(
            !p.h.callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
    }
    // A successful direct FullMix needs a fresh committed request and its own ACK.
    // Refusal must preserve the original request's exact requested mask and ACK.
    let surviving = if admit_full_mix {
        let fresh = p.request("all_stems", 0b0101);
        p.capture_then_wait(&fresh);
        fresh
    } else {
        pending
    };
    wait_until(|| p.h.consumer.peek().is_ok() || surviving.error().is_some());
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    let accepted = surviving.residency_status() == "accepted" && surviving.is_current();
    assert!(accepted, "{:?}", surviving.error());
    let mask = if admit_full_mix { 0b0101 } else { 0b1010 };
    assert_eq!(p.h.callback.mixer.stem_demand_for_measurement(0).1, mask);
    p.select_reference("all_stems", mask);
    assert!(p.render(257, false).iter().any(|value| value.abs() > 0.001));
    eprintln!("committed FullMix={admit_full_mix} surviving ownACK={accepted} mask={mask}");
    p.h.engine.cold_jobs.shutdown();

    // The pin holds sample + set.reference_samples and each of four components.
    // The old accepted Pending mirror independently loses one reader on polling.
    assert_eq!(owners_captured, owners_before + 2);
    assert_eq!(owners_after_admission, owners_captured);
    assert_eq!(
        owners_after,
        owners_before - usize::from(admit_full_mix) + 2 * usize::from(!admit_full_mix)
    );
    assert_eq!(components_after_admission, components_captured);
    for ((baseline, captured), after) in components_before
        .iter()
        .zip(&components_captured)
        .zip(&components_after)
    {
        assert_eq!(*captured, baseline + 1);
        assert_eq!(
            *after,
            baseline - usize::from(admit_full_mix) + usize::from(!admit_full_mix)
        );
    }
    assert_eq!(original_mirror, (!admit_full_mix).then_some(addresses));
    assert_eq!(before, (WORKERS, QUEUED_JOBS, WORKERS + QUEUED_JOBS));
    let remaining = QUEUED_JOBS - usize::from(admit_full_mix);
    assert_eq!(after, (WORKERS, remaining, WORKERS + remaining));
    assert_eq!(intent_after, intent_before + u64::from(admit_full_mix));
    assert_eq!(
        status,
        if admit_full_mix {
            "rejected"
        } else {
            "captured"
        }
    );
    assert_eq!(current, !admit_full_mix);
    assert!(unchanged);
    assert_eq!(peers_before_release, 0);
    assert_eq!(replacement_admitted, admit_full_mix);
    assert_eq!(peer_runs.load(Ordering::Acquire), QUEUED_JOBS - 1);
    assert_eq!(
        replacement_runs.load(Ordering::Acquire),
        usize::from(admit_full_mix)
    );
    assert_eq!(error.is_none(), admit_full_mix);
    assert!(error.is_none_or(|error| error.contains("stem full-mix queue is full")));
}

#[test]
fn full_mix_retires_queued_committed_capture_before_callback_and_preserves_fresh_own_ack() {
    queued_capture_after_full_mix(true);
}

#[test]
fn refused_full_mix_preserves_queued_committed_capture_and_original_own_ack() {
    queued_capture_after_full_mix(false);
}
