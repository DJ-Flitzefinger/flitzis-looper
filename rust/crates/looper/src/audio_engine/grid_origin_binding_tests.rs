//! Timing intent cannot orphan physical PCM already adopted by the callback.

use super::*;
use crate::audio_engine::constant_timing::{publish_legacy_bpm, publish_legacy_origin};
use crate::audio_engine::input_runtime_binding::capture;
use crate::audio_engine::resident_relocation::ResidentWindowTicket;

const OLD_END_S: f64 = 16.0; // Eight bars at 120 BPM.
const NEW_START_S: f64 = 15.5;
const NEW_END_S: f64 = 20.0;

fn assert_fresh_binding(h: &Harness, id: usize) -> InputRuntimePadBinding {
    let binding = capture(&h.engine, id).unwrap().unwrap();
    assert!(
        binding.available(),
        "pad {id}: legacy timing remains available"
    );
    assert!(
        binding.current(),
        "pad {id}: source current={} window current={} cached={:?} native={:?}",
        binding
            .ownership
            .binding_source_generation(id, binding.binding)
            .is_some(),
        binding
            .ownership
            .binding_window_current(id, binding.binding),
        binding.binding.resident,
        h.callback.mixer.bank_for_measurement()[id]
            .as_ref()
            .unwrap()
            .resident_binding(),
    );
    binding
}

fn prepared_window(h: &mut Harness, id: usize, start: f64, end: f64) -> ResidentWindowTicket {
    let ticket = prepare_window_with_producer(
        &h.engine,
        id,
        WindowRequest {
            loop_region: Some((start, Some(end))),
            key_lock: Some(false),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        (h.consumer.peek().is_ok() || ticket.error().unwrap().is_some())
            && h.producer.try_lock().is_ok()
    });
    assert_eq!(
        ticket.publication_status(),
        "pending",
        "{:?}",
        ticket.error()
    );
    ticket
}

fn two_pad_stem_harness() -> Harness {
    let mut h = long_harness_frames(RATE as usize * NEW_END_S as usize);
    let (_, feedback) = rtrb::RingBuffer::<crate::messages::AudioMessage>::new(8);
    h.engine.test_output_format = Some((RATE, 2));
    h.engine.test_control_channels = Some((h.producer.clone(), Arc::new(Mutex::new(feedback))));
    let mut saved_selection = None;
    for id in [0, 215] {
        let window = prepared_window(&mut h, id, 0.0, OLD_END_S);
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(window.publication_status(), "accepted");
        reconcile(&h.engine).unwrap();
        assert_fresh_binding(&h, id);
        let source = h.ticket(id);
        let pair = h.prepare(id, &source, false, saved_selection.as_ref());
        // The set is genuine matching signal, prepared from the canonical WAVs,
        // and each pad gets its own real committed-set and resident callback ACK.
        saved_selection = Some(selection(&pair));
        pair.select();
        let committed = h
            .engine
            .register_committed_stem_pair_with_producer(&pair, &source, &h.producer)
            .unwrap();
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(committed.publication_status(), "accepted");
        let stems = h
            .engine
            .request_committed_stem_residency_with_producer(
                &committed,
                "all_stems",
                0b1111,
                &h.producer,
            )
            .unwrap();
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        wait_until(|| {
            (h.consumer.peek().is_ok() || stems.error().is_some()) && h.producer.try_lock().is_ok()
        });
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(stems.residency_status(), "accepted", "{:?}", stems.error());
        h.callback.mixer.finish_committed_stem_mode(id);
        reconcile(&h.engine).unwrap();
        let set = h.callback.mixer.stems_for_measurement()[id]
            .as_ref()
            .unwrap();
        assert_eq!(set.stems.len(), 4);
        assert!(set.stems.iter().all(|stem| {
            stem.resident_start() == 0
                && stem.resident_end() == (OLD_END_S * f64::from(RATE)) as usize
                && stem.samples.iter().any(|sample| sample.abs() > 0.04)
        }));
        assert_fresh_binding(&h, id);
    }
    h
}

#[test]
fn adopted_stem_window_then_rapid_signed_grid_edits_keep_fresh_binding_and_ordinary_capture() {
    let mut h = two_pad_stem_harness();
    let other = assert_fresh_binding(&h, 215);
    let old = h.engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let window = prepared_window(&mut h, 0, NEW_START_S, NEW_END_S);
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(window.publication_status(), "accepted");
    let adopted = h.callback.mixer.bank_for_measurement()[0].clone().unwrap();
    let adopted_stems = h.callback.mixer.stems_for_measurement()[0].clone().unwrap();
    assert_eq!(
        adopted.resident_start(),
        (NEW_START_S * f64::from(RATE)) as usize
    );
    assert_ne!(old.resident_binding(), adopted.resident_binding());
    // Deliberately retain the real callback/control gap. No control-side polling
    // or reconcile is allowed before timing intent changes the scalar authority.
    assert_eq!(
        h.engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .resident_binding(),
        old.resident_binding()
    );
    let (parameters, _parameter_consumer) = rtrb::RingBuffer::new(4);
    let parameters = Arc::new(Mutex::new(parameters));
    publish_legacy_origin(&h.engine, &h.producer, 0, 0.125 - 30.0 / f64::from(RATE)).unwrap();
    publish_legacy_bpm(&h.engine, &parameters, 0, Some(120.0)).unwrap();
    publish_legacy_origin(&h.engine, &h.producer, 0, -0.125).unwrap();

    // These timing edits may revoke the old readiness receipt, but cannot undo
    // already transferred physical storage or strand its control mirror.
    reconcile(&h.engine).unwrap();
    let binding = assert_fresh_binding(&h, 0);
    assert_eq!(binding.binding.resident, adopted.resident_binding());
    {
        let owners = h.engine.resident_stem_cache.lock().unwrap();
        let mirrored = &owners[0].accepted.as_ref().unwrap().set;
        assert!(Arc::ptr_eq(
            &mirrored.complete_set_identity,
            &adopted_stems.complete_set_identity
        ));
        assert!(Arc::ptr_eq(
            &mirrored.reference_samples,
            &adopted_stems.reference_samples
        ));
        assert_eq!(mirrored.stems.len(), adopted_stems.stems.len());
        for (mirrored, native) in mirrored.stems.iter().zip(&adopted_stems.stems) {
            assert!(mirrored.same_window(native));
            assert!(Arc::ptr_eq(&mirrored.samples, &native.samples));
        }
    }
    assert!(other.current(), "the second genuine stem pad is unaffected");
    for _ in 0..3 {
        reconcile(&h.engine).unwrap();
        assert_fresh_binding(&h, 0);
        assert_fresh_binding(&h, 215);
    }
    let start = h
        .engine
        ._capture_pad_start(
            0,
            NEW_START_S,
            Some(NEW_END_S),
            1.0,
            false,
            None,
            Some(binding.source_generation),
        )
        .expect("ordinary GUI start can capture the current complete source after the grid edit");
    assert_eq!(h.callback.drain(&mut h.consumer), 3);
    assert!(start.target_frame().is_some());
    let current = assert_fresh_binding(&h, 0);
    let callback = h.callback.as_mut();
    let config = callback
        .mixer
        .prepare_admitted_play_sample_rt(0, 1.0, 0, current.binding, &mut callback.retirement)
        .expect("fresh start config retains the later signed origin");
    assert_eq!(
        config.source_timing.legacy_origin_frame,
        -0.125 * f64::from(RATE)
    );
    assert_fresh_binding(&h, 215);
}

#[test]
fn adopted_window_mirror_never_restores_the_predecessor_over_a_new_cold_source_generation() {
    let mut h = long_harness();
    let original = assert_fresh_binding(&h, 0);
    let window = prepared_window(
        &mut h,
        0,
        LOOP_START as f64 / f64::from(RATE),
        LOOP_END as f64 / f64::from(RATE),
    );
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(window.publication_status(), "accepted");
    let adopted = h.callback.mixer.bank_for_measurement()[0].clone().unwrap();
    publish_legacy_origin(&h.engine, &h.producer, 0, -0.125).unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);

    // The same immutable bytes are deliberately loaded as a new actual source
    // generation. Pointer/content equality cannot grant the predecessor custody.
    // long_harness released the original preparation assignment. A fresh verified
    // subscriber is required for another real adoption; copying that old lease
    // would merely submit an already released preparation owner to the worker.
    let replacement_lease = h.engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .subscribe_verified()
        .unwrap();
    let preparation = control::prepared_for_test(
        &h.engine,
        PreparedMigrationMaterial::from_current(
            &h.root,
            h.material.sample.clone(),
            replacement_lease,
        )
        .unwrap(),
        &h.root,
    )
    .unwrap();
    let request = control::adopt_for_format(
        &h.engine,
        0,
        &preparation,
        h.producer.clone(),
        (2, RATE, h.root.clone()),
    )
    .unwrap();
    let mut failure = None;
    wait_until(|| {
        while let Ok(event) = h
            .engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
            .map(LoaderEvent::into_test_payload)
        {
            if let LoaderEvent::Error {
                request_id, error, ..
            } = event
                && request_id == request.request_id()
            {
                failure = Some(error);
            }
        }
        (h.consumer.peek().is_ok() && h.producer.try_lock().is_ok()) || failure.is_some()
    });
    assert!(
        failure.is_none(),
        "replacement source request {} failed in phase {}: {:?}",
        request.request_id(),
        request.phase().unwrap(),
        failure
    );
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    source_successes(&h.engine, &[request.request_id()]);
    preparation.release_preparation().unwrap();
    let replacement = h.callback.mixer.bank_for_measurement()[0].clone().unwrap();
    assert_ne!(replacement.resident_binding(), adopted.resident_binding());
    assert!(!original.current());
    for _ in 0..3 {
        reconcile(&h.engine).unwrap();
        let current = assert_fresh_binding(&h, 0);
        assert_ne!(current.source_generation, original.source_generation);
        assert_eq!(current.binding.resident, replacement.resident_binding());
        assert_fresh_binding(&h, 215);
    }
}
