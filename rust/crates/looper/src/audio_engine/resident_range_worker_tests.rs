//! Real, device-free 216-pad Source/Pair/Window callback adoption.
//! Two small verified materials exercise shared and distinct content with the
//! unchanged native worker/queue limits. Timing is explicitly Legacy/off here;
//! no automatic-timing, simultaneous-216-voice, RSS or hearing claim is made.

use super::*;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_runtime_binding::{
    InputPadBinding, capture, enqueue_stop_with_producer,
};
use crate::audio_engine::resident_relocation::{
    ResidentWindowTicket, WindowRequest, launch_with_producer, prepare_window_with_producer,
    reconcile,
};
use crate::messages::StemMixMode;
use std::mem::size_of;

struct PadSnapshot {
    sample: SampleBuffer,
    stems: PreparedStemSet,
    sample_bits: Vec<u32>,
    stem_bits: [Vec<u32>; 4],
    binding: InputPadBinding,
    source_generation: u64,
    request: u64,
    loaded_generation: (u64, u32),
    digest: String,
    preparation_epoch: u64,
    timing_epoch: u64,
    stem_demand: (StemMixMode, u8, u64),
}

fn bits(sample: &SampleBuffer) -> Vec<u32> {
    sample.samples.iter().map(|value| value.to_bits()).collect()
}

fn acknowledge_full_mix_window(h: &mut Harness, id: usize, frames: usize) {
    assert!(h.callback.mixer.stems_for_measurement()[id].is_none());
    let before = h.callback.mixer.bank_for_measurement()[id]
        .as_ref()
        .unwrap()
        .clone();
    let ticket = prepare_window_with_producer(
        &h.engine,
        id,
        WindowRequest {
            storage_range: Some((0.0, frames as f64 / f64::from(RATE))),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    assert_eq!(ticket.publication_status(), "pending");
    assert!(ticket.read_observation_for_test().is_none());
    match h.consumer.peek().unwrap() {
        ControlMessage::RelocateResident(transaction) => {
            assert_eq!(transaction.id, id);
            assert!(transaction.stems.is_none());
            assert!(Arc::ptr_eq(&before.samples, &transaction.sample.samples));
            assert_eq!(
                (
                    transaction.sample.resident_start(),
                    transaction.sample.resident_end()
                ),
                (0, frames)
            );
            assert_eq!(
                transaction.sample.residency.as_ref().unwrap().context,
                ResidentContext::FullTrack
            );
        }
        _ => panic!("pad {id}: expected actual FULL MIX window transaction"),
    }
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(ticket.is_current());
    reconcile(&h.engine).unwrap();
    assert!(h.callback.mixer.stems_for_measurement()[id].is_none());
    let binding = capture(&h.engine, id).unwrap().unwrap();
    assert!(binding.current() && binding.available());
    assert!(binding.binding.accepted.is_none());
    assert_eq!(
        h.engine.current_timing_acknowledgements.current_epoch(id),
        0
    );
    assert!(Arc::ptr_eq(
        &before.samples,
        &h.callback.mixer.bank_for_measurement()[id]
            .as_ref()
            .unwrap()
            .samples
    ));
}

fn snapshot(h: &Harness, id: usize) -> PadSnapshot {
    let sample = h.callback.mixer.bank_for_measurement()[id]
        .as_ref()
        .unwrap()
        .clone();
    let stems = h.callback.mixer.stems_for_measurement()[id]
        .as_ref()
        .unwrap()
        .clone();
    let binding = capture(&h.engine, id).unwrap().unwrap();
    assert!(binding.current() && binding.available(), "pad {id}");
    assert!(binding.binding.accepted.is_none());
    assert!(stems.accepted_timing.is_none());
    assert_eq!(stems.publication.status(), "accepted");
    assert_eq!(stems.available_mask, 0b1111);
    let stem_demand = h.callback.mixer.stem_demand_for_measurement(id);
    assert_eq!(
        stem_demand,
        (StemMixMode::AllStems, 0b1111, stems.source_version_hash)
    );
    assert_eq!(
        h.engine.current_timing_acknowledgements.current_epoch(id),
        0
    );
    assert!(Arc::ptr_eq(&sample.samples, &stems.reference_samples));
    assert!(stems.stems.iter().all(|stem| stem.same_window(&sample)));
    assert!(Arc::ptr_eq(
        &sample.samples,
        &h.engine.sample_cache.lock().unwrap()[id]
            .as_ref()
            .unwrap()
            .samples,
    ));
    PadSnapshot {
        sample_bits: bits(&sample),
        stem_bits: std::array::from_fn(|component| bits(&stems.stems[component])),
        sample,
        stems,
        binding: binding.binding,
        source_generation: binding.source_generation,
        request: h.engine.pad_request_ids.lock().unwrap()[id],
        loaded_generation: h.engine.loaded_source_generations.lock().unwrap()[id],
        digest: h.engine.loaded_source_digests.lock().unwrap()[id]
            .clone()
            .unwrap(),
        preparation_epoch: h.engine.prepared_source_epochs[id].load(Ordering::Acquire),
        timing_epoch: h.engine.current_timing_acknowledgements.current_epoch(id),
        stem_demand,
    }
}

fn assert_preserved(h: &Harness, id: usize, before: &PadSnapshot) {
    let after = snapshot(h, id);
    assert!(
        Arc::ptr_eq(&before.sample.samples, &after.sample.samples),
        "pad {id}"
    );
    assert!(Arc::ptr_eq(
        &before.sample.residency.as_ref().unwrap().source,
        &after.sample.residency.as_ref().unwrap().source,
    ));
    assert_eq!(
        before.sample.resident_binding(),
        after.sample.resident_binding()
    );
    assert_eq!(before.sample_bits, after.sample_bits, "pad {id}");
    assert_eq!(
        before.stems.source_version_hash,
        after.stems.source_version_hash
    );
    assert_eq!(before.stems.sample_rate_hz, after.stems.sample_rate_hz);
    assert_eq!(before.stems.channels, after.stems.channels);
    assert_eq!(before.stems.frame_count, after.stems.frame_count);
    assert_eq!(before.stems.available_mask, after.stems.available_mask);
    assert!(Arc::ptr_eq(
        &before.stems.complete_set_identity,
        &after.stems.complete_set_identity,
    ));
    for component in 0..4 {
        assert!(
            Arc::ptr_eq(
                &before.stems.stems[component].samples,
                &after.stems.stems[component].samples,
            ),
            "pad {id}, component {component}"
        );
        assert_eq!(
            before.stems.stems[component].resident_binding(),
            after.stems.stems[component].resident_binding(),
        );
        assert_eq!(before.stem_bits[component], after.stem_bits[component]);
    }
    assert_eq!(before.binding, after.binding, "pad {id}");
    assert_eq!(before.source_generation, after.source_generation);
    assert_eq!(before.request, after.request);
    assert_eq!(before.loaded_generation, after.loaded_generation);
    assert_eq!(before.digest, after.digest);
    assert_eq!(before.preparation_epoch, after.preparation_epoch);
    assert_eq!(before.timing_epoch, after.timing_epoch);
    assert_eq!(before.stem_demand, after.stem_demand);
}

fn finite_window(h: &mut Harness, id: usize, start: usize, end: usize) -> ResidentWindowTicket {
    assert!(h.consumer.peek().is_err());
    let ticket = prepare_window_with_producer(
        &h.engine,
        id,
        WindowRequest {
            loop_region: Some((
                start as f64 / f64::from(RATE),
                Some(end as f64 / f64::from(RATE)),
            )),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        h.consumer.peek().is_ok() || matches!(ticket.publication_status(), "failed" | "cancelled")
    });
    assert_eq!(
        ticket.publication_status(),
        "pending",
        "pad {id}: {:?}",
        ticket.error().unwrap()
    );
    let (sample, stems) = match h.consumer.peek().unwrap() {
        ControlMessage::RelocateResident(transaction) => {
            assert_eq!(transaction.id, id);
            assert_eq!(transaction.sample.resident_start(), start);
            assert_eq!(transaction.sample.resident_end(), end);
            assert_eq!(
                transaction.sample.residency.as_ref().unwrap().context,
                ResidentContext::FiniteLoop,
            );
            let stems = transaction.stems.as_ref().unwrap();
            assert_eq!(stems.stems.len(), 4);
            assert!(
                stems
                    .stems
                    .iter()
                    .all(|stem| stem.same_window(&transaction.sample))
            );
            (transaction.sample.clone(), stems.clone())
        }
        _ => panic!("pad {id}: expected actual window command"),
    };
    let observation = ticket.read_observation_for_test().unwrap();
    let bytes = (end - start) * 2 * size_of::<f32>();
    assert_eq!(observation.source.read_bytes, bytes as u64, "pad {id}");
    assert_eq!(observation.source.allocated_bytes, bytes);
    assert_eq!(
        (observation.source.start_frame, observation.source.end_frame),
        (start, end)
    );
    assert_eq!(
        observation.stems.read_bytes,
        [bytes as u64, bytes as u64, bytes as u64, bytes as u64, 0]
    );
    assert_eq!(observation.stems.allocated_bytes, bytes * 4);
    assert_eq!(observation.stems.fresh_opens, 0);
    assert_eq!(observation.stems.integrity_bytes, 0);
    let byte_start = start * 2 * size_of::<f32>();
    assert_eq!(
        observation.stems.byte_ranges,
        [
            Some((byte_start, byte_start + bytes)),
            Some((byte_start, byte_start + bytes)),
            Some((byte_start, byte_start + bytes)),
            Some((byte_start, byte_start + bytes)),
            None,
        ]
    );
    assert!(observation.admitted_peak_bytes < crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES);
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(ticket.is_current());
    reconcile(&h.engine).unwrap();
    let actual = snapshot(h, id);
    assert!(Arc::ptr_eq(&sample.samples, &actual.sample.samples));
    for component in 0..4 {
        assert!(Arc::ptr_eq(
            &stems.stems[component].samples,
            &actual.stems.stems[component].samples
        ));
    }
    ticket
}

fn render_usable(h: &mut Harness, id: usize, ticket: &ResidentWindowTicket, before: &PadSnapshot) {
    assert_preserved(h, id, before);
    assert!(ticket.is_current());
    assert!(h.callback.mixer.voices.iter().all(|voice| !voice.active));
    assert!(launch_with_producer(&h.engine, ticket, false, 1, &h.producer).unwrap());
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    let voice = h
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(id))
        .unwrap();
    assert!(Arc::ptr_eq(
        &voice.sample.as_ref().unwrap().samples,
        &before.sample.samples
    ));
    assert_eq!(
        h.callback
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        1
    );
    let mut output = [0.0; 4];
    let mut peaks = [0.0; NUM_SAMPLES];
    h.callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut peaks,
        0,
        &mut h.callback.retirement,
    );
    assert!(output.iter().all(|value| value.is_finite()), "pad {id}");
    assert!(output.iter().any(|value| *value != 0.0), "pad {id} silent");
    assert!(peaks[id] > 0.0, "pad {id} missing actual render activity");
    assert!(enqueue_stop_with_producer(
        &h.engine.input_runtime_ownership,
        &mut h.producer.lock().unwrap(),
        Some(id),
    ));
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert!(h.callback.mixer.voices.iter().all(|voice| !voice.active));
    assert_preserved(h, id, before);
}

#[test]
fn all_216_actual_pair_windows_preserve_other_215_on_both_endpoint_edits() {
    assert_eq!(NUM_SAMPLES, 216);
    let mut h = Harness::new();
    // Reopen a genuine checked preparation: Harness released its creator-only
    // registration after its initial source ACKs. Never clone that retired right.
    let primary =
        prepare_material(&h.root, &h.material.lease.original_path, RATE, 2, &|| false).unwrap();
    let second_path = h.root.join("second.wav");
    fs::write(&second_path, stereo_wav(11)).unwrap();
    let secondary = prepare_material(&h.root, &second_path, RATE, 2, &|| false).unwrap();
    let _secondary_original_owner = h
        .engine
        .project_assets
        .acquire(&h.root, &secondary.lease.original_path)
        .unwrap();
    let versions = [canonical_version(&primary), canonical_version(&secondary)];
    assert_ne!(
        primary.metadata()["material_id"],
        secondary.metadata()["material_id"]
    );
    assert_ne!(
        primary.metadata()["original"]["sha256"],
        secondary.metadata()["original"]["sha256"]
    );
    assert_eq!(
        (primary.sample.frame_count(), secondary.sample.frame_count()),
        (7, 11)
    );
    let frames = [primary.sample.frame_count(), secondary.sample.frame_count()];
    let references = [
        h.wav_reference.clone(),
        format!(
            "samples/materials/M{}/stems/.ready-{GENERATION}",
            secondary.metadata()["material_id"].as_str().unwrap(),
        ),
    ];
    write_complete_wavs(
        &h.root.parent().unwrap().join(&references[1]),
        &stereo_wav(frames[1]),
        &versions[1],
    );
    let preparations = [
        control::prepared_for_test(&h.engine, primary, &h.root).unwrap(),
        control::prepared_for_test(&h.engine, secondary, &h.root).unwrap(),
    ];
    let mut source_tickets = Vec::new();
    // Serial admission preserves the production two-worker/32-job limits and
    // the existing eight-command ring; this is all-bank readiness, not load stress.
    for id in 0..NUM_SAMPLES {
        let ticket = control::adopt_for_format(
            &h.engine,
            id,
            &preparations[id % 2],
            h.producer.clone(),
            (2, RATE, h.root.clone()),
        )
        .unwrap();
        wait_until(|| h.consumer.peek().is_ok());
        assert_eq!(ticket.sample_id(), id);
        assert_eq!(ticket.phase().unwrap(), "pending");
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        match terminal(&h.engine, ticket.request_id()) {
            LoaderEvent::Success {
                id: actual_id,
                request_id,
                ..
            } => {
                assert_eq!(actual_id, id);
                assert_eq!(request_id, ticket.request_id());
            }
            event => panic!("pad {id}: source failed: {event:?}"),
        }
        wait_until(|| h.engine.cold_loading[id].load(Ordering::Acquire) == 0);
        assert_eq!(ticket.phase().unwrap(), "acknowledged");
        assert!(ticket.is_current().unwrap());
        source_tickets.push(ticket);
    }
    for preparation in &preparations {
        preparation.release_preparation().unwrap();
    }
    for id in 0..NUM_SAMPLES {
        acknowledge_full_mix_window(&mut h, id, frames[id % 2]);
        assert!(source_tickets[id].is_current().unwrap());
    }

    let mut saved_owners = Vec::new();
    for id in 0..NUM_SAMPLES {
        let group = id % 2;
        let ticket = h
            .engine
            .capture_prepared_source(id, versions[group].clone())
            .unwrap();
        assert_eq!(ticket.publication_status(), "captured");
        let pair = h
            .engine
            .prepare_stem_pair_at_root(
                &h.root,
                id,
                &versions[group],
                &references[group],
                &ticket,
                true,
                None,
            )
            .unwrap();
        assert!(pair.has_components());
        assert_complete_selection(&h.root, &selection(&pair));
        if id < 2 {
            saved_owners.push(h.save(&pair));
        } else {
            pair.select();
        }
        h.engine
            .publish_stem_pair_with_producer(&pair, &ticket, &h.producer)
            .unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        let queued = queued_stems(&mut h.consumer);
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(ticket.publication_status(), "accepted");
        reconcile(&h.engine).unwrap();
        // Actual ALL demand, including all four component bits, is delivered
        // through the ordinary fixed commands; availability alone is insufficient.
        {
            let mut producer = h.producer.lock().unwrap();
            producer
                .push(ControlMessage::SetStemMixMode {
                    id,
                    mode: StemMixMode::AllStems,
                    source_version_hash: queued.source_version_hash,
                })
                .unwrap();
            producer
                .push(ControlMessage::SetStemEnabledMask {
                    id,
                    enabled_stem_mask: 0b1111,
                    source_version_hash: queued.source_version_hash,
                })
                .unwrap();
        }
        assert_eq!(h.callback.drain(&mut h.consumer), 2);
        let actual = h.callback.mixer.stems_for_measurement()[id]
            .as_ref()
            .unwrap();
        for component in 0..4 {
            assert!(Arc::ptr_eq(
                &queued.stems[component].samples,
                &actual.stems[component].samples
            ));
            if id >= 2 {
                let shared = h.callback.mixer.stems_for_measurement()[group]
                    .as_ref()
                    .unwrap();
                assert!(Arc::ptr_eq(
                    &shared.stems[component].samples,
                    &actual.stems[component].samples
                ));
            }
        }
    }
    assert_eq!(saved_owners.len(), 2);
    assert!(!Arc::ptr_eq(
        &h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity,
        &h.callback.mixer.stems_for_measurement()[1]
            .as_ref()
            .unwrap()
            .complete_set_identity,
    ));
    let mut windows = Vec::new();
    for id in 0..NUM_SAMPLES {
        windows.push(finite_window(&mut h, id, 1, frames[id % 2] - 1));
        assert!(source_tickets[id].is_current().unwrap());
    }
    assert_eq!(windows.len(), 216);
    let held_bytes: usize = (0..NUM_SAMPLES)
        .map(|id| {
            let sample = h.callback.mixer.bank_for_measurement()[id]
                .as_ref()
                .unwrap();
            let stems = h.callback.mixer.stems_for_measurement()[id]
                .as_ref()
                .unwrap();
            (sample.samples.len()
                + stems
                    .stems
                    .iter()
                    .map(|stem| stem.samples.len())
                    .sum::<usize>())
                * size_of::<f32>()
        })
        .sum();
    // Even counting shared buffers repeatedly, the actual tiny stereo test fits
    // the supported 128-MiB minimum. This is allocated PCM, never RSS evidence.
    assert!(held_bytes < 128 * 1024 * 1024);

    for edited in [0, 215] {
        let before: Vec<_> = (0..NUM_SAMPLES).map(|id| snapshot(&h, id)).collect();
        let changed = finite_window(&mut h, edited, 2, frames[edited % 2] - 1);
        let after = snapshot(&h, edited);
        assert!(!Arc::ptr_eq(
            &before[edited].sample.samples,
            &after.sample.samples
        ));
        assert_eq!(
            after.sample.window_revision(),
            before[edited].sample.window_revision() + 1
        );
        assert!(Arc::ptr_eq(
            &before[edited].stems.complete_set_identity,
            &after.stems.complete_set_identity
        ));
        assert_eq!(before[edited].source_generation, after.source_generation);
        assert_eq!(before[edited].digest, after.digest);
        windows[edited] = changed;
        for id in 0..NUM_SAMPLES {
            assert!(source_tickets[id].is_current().unwrap());
            if id != edited {
                assert_preserved(&h, id, &before[id]);
            }
        }
        for id in 0..NUM_SAMPLES {
            if id != edited {
                render_usable(&mut h, id, &windows[id], &before[id]);
            }
        }
    }

    // An exact eligible view takes the actual worker-free Arc-sharing branch,
    // yet still needs this new ticket's ordinary pending -> callback ACK.
    let before = snapshot(&h, 2);
    let reused = prepare_window_with_producer(
        &h.engine,
        2,
        WindowRequest {
            loop_region: Some((
                1.0 / f64::from(RATE),
                Some((frames[0] - 1) as f64 / f64::from(RATE)),
            )),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    assert_eq!(reused.publication_status(), "pending");
    assert!(reused.read_observation_for_test().is_none());
    match h.consumer.peek().unwrap() {
        ControlMessage::RelocateResident(transaction) => {
            assert!(Arc::ptr_eq(
                &before.sample.samples,
                &transaction.sample.samples
            ));
            for component in 0..4 {
                assert!(Arc::ptr_eq(
                    &before.stems.stems[component].samples,
                    &transaction.stems.as_ref().unwrap().stems[component].samples
                ));
            }
        }
        _ => panic!("expected own reused-view native command"),
    }
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(reused.publication_status(), "accepted");
    assert!(reused.is_current());
    reconcile(&h.engine).unwrap();
    let after = snapshot(&h, 2);
    assert!(Arc::ptr_eq(&before.sample.samples, &after.sample.samples));
    assert_eq!(
        before.sample.window_revision(),
        after.sample.window_revision()
    );
}
