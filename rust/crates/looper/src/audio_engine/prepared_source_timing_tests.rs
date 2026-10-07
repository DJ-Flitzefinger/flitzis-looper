//! Productive preparation capture/enqueue/adoption using current native accepted records.

use super::*;
use crate::audio_engine::input_runtime_binding;
use crate::audio_engine::prepared_source::{PreparedSourceTicket, enqueue_current_prepared_stems};
use crate::audio_engine::stem_cache::source_version_hash;
use crate::messages::{PreparedStemSet, STEM_BUFFER_COUNT};

fn version() -> String {
    format!("samples/source.wav|sha256-v1:{}", "a".repeat(64))
}

fn prepared(ticket: &PreparedSourceTicket) -> PreparedStemSet {
    PreparedStemSet {
        complete_set_identity: std::sync::Arc::new([0; 32]),
        reference_samples: ticket.sample.samples.clone(),
        publication: ticket.publication.clone(),
        accepted_timing: ticket.publication.accepted_projection(),
        source_version_hash: source_version_hash(&ticket.source_version),
        sample_rate_hz: ticket.sample_rate_hz,
        channels: ticket.sample.channels,
        frame_count: ticket.sample.samples.len() / ticket.sample.channels,
        available_mask: ((1_u16 << STEM_BUFFER_COUNT) - 1) as u8,
        stems: std::array::from_fn(|_| ticket.sample.clone()),
    }
}

fn accept(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    mixer: &mut RtMixer,
    decision: TimingAcceptanceDecision,
) {
    publish(
        engine,
        producer,
        &synthetic_ticket(engine),
        &hypotheses(),
        origin(),
        decision,
    )
    .unwrap();
    assert!(accept_message(mixer, consumer.pop().unwrap()));
}

#[test]
fn prepared_capture_uses_complete_current_revision_and_exact_signed_projection() {
    let engine = test_engine();
    let (producer, mut consumer) = queue(8);
    let mut mixer = acknowledged_mixer(&engine);
    accept(&engine, &producer, &mut consumer, &mut mixer, decision());
    let first = engine.capture_prepared_source(0, version()).unwrap();
    let current = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    let projected = first.publication.accepted_projection().unwrap();
    assert!(
        first.publication.resident_launch_permit().is_none(),
        "resident launch snapshots must not discard a productive accepted-timing guard"
    );
    assert_eq!(Some(projected), current.binding.accepted);
    assert_eq!(projected.origin_seconds, origin().seconds);
    assert_eq!(projected.sample_rate_hz, RATE);

    let mut changed = decision();
    changed
        .provenance
        .push_str("; independent replacement decision");
    accept(&engine, &producer, &mut consumer, &mut mixer, changed);
    let replacement = engine.capture_prepared_source(0, version()).unwrap();
    let next = replacement.publication.accepted_projection().unwrap();
    assert_eq!(next.period_seconds, projected.period_seconds);
    assert_eq!(next.origin_seconds, projected.origin_seconds);
    assert_ne!(next.revision, projected.revision);
    assert_ne!(next.publication_epoch, projected.publication_epoch);
    assert!(!first.publication.current());
    assert!(replacement.publication.current());
}

#[test]
fn prepared_capture_rejects_unavailable_automatic_but_preserves_manual_tap_legacy_intent() {
    let engine = test_engine();
    let (producer, mut consumer) = queue(8);
    assert!(engine.capture_prepared_source(0, version()).is_err());
    let mut mixer = acknowledged_mixer(&engine);
    accept(&engine, &producer, &mut consumer, &mut mixer, decision());
    let mut previous: Option<PreparedSourceTicket> = None;
    for intent in [
        TimingIntent::Manual,
        TimingIntent::Tap,
        TimingIntent::Legacy,
    ] {
        set_intent(&engine, &producer, 0, intent).unwrap();
        let ControlMessage::ClearPadConstantTiming { id, through_epoch } = consumer.pop().unwrap()
        else {
            panic!("timing authority clear");
        };
        mixer.clear_constant_timing(id, through_epoch);
        if let Some(previous) = previous.take() {
            assert!(!previous.publication.current());
        }
        let ticket = engine.capture_prepared_source(0, version()).unwrap();
        assert_eq!(ticket.publication.accepted_projection(), None);
        assert!(ticket.publication.current());
        enqueue_current_prepared_stems(&engine, &producer, &ticket, &version(), prepared(&ticket))
            .unwrap();
        let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
            panic!("prepared stems publication");
        };
        assert!(mixer.publish_prepared_stems(id, stems));
        assert_eq!(ticket.publication_status(), "accepted");
        previous = Some(ticket);
    }
    set_intent(&engine, &producer, 0, TimingIntent::Automatic).unwrap();
    assert!(engine.capture_prepared_source(0, version()).is_err());
    assert!(!previous.unwrap().publication.current());
}

#[test]
fn pending_replacement_capture_stays_current_after_rejection_and_uses_previous_record() {
    let engine = test_engine();
    let (producer, mut consumer) = queue(8);
    let mut mixer = acknowledged_mixer(&engine);
    accept(&engine, &producer, &mut consumer, &mut mixer, decision());
    let previous = input_runtime_binding::capture(&engine, 0)
        .unwrap()
        .unwrap()
        .binding
        .accepted;
    publish(
        &engine,
        &producer,
        &synthetic_ticket(&engine),
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
    let pending = consumer.pop().unwrap();
    let captured = engine.capture_prepared_source(0, version()).unwrap();
    assert_eq!(captured.publication.accepted_projection(), previous);
    assert!(captured.publication.current());
    // Corrupt only the queued replacement's bounded scalar projection. Rejection
    // must not change the acknowledged current record or retire this capture.
    let ControlMessage::PublishConstantTiming { id, mut timing } = pending else {
        panic!("timing publication");
    };
    timing.projection.sample_rate_hz += 1;
    assert!(!mixer.publish_constant_timing_rt(id, timing, &mut ImmediateAudioBufferRetirement));
    assert!(captured.publication.current());
    enqueue_current_prepared_stems(
        &engine,
        &producer,
        &captured,
        &version(),
        prepared(&captured),
    )
    .unwrap();
    let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
        panic!("prepared stems publication");
    };
    assert!(mixer.publish_prepared_stems(id, stems));
    assert_eq!(captured.publication_status(), "accepted");
}

#[test]
fn queued_prepared_stems_reject_accepted_replacement_without_a_new_preparation_epoch() {
    let engine = test_engine();
    let (producer, mut consumer) = queue(8);
    let mut mixer = acknowledged_mixer(&engine);
    accept(&engine, &producer, &mut consumer, &mut mixer, decision());
    let mut changed = decision();
    changed.provenance.push_str("; replacement after capture");
    publish(
        &engine,
        &producer,
        &synthetic_ticket(&engine),
        &hypotheses(),
        origin(),
        changed,
    )
    .unwrap();
    let replacement = consumer.pop().unwrap();
    // Preparation captures the old effective accepted record, after the queued
    // replacement has advanced the shared preparation epoch.
    let captured = engine.capture_prepared_source(0, version()).unwrap();
    let captured_epoch = captured.publication.expected;
    enqueue_current_prepared_stems(
        &engine,
        &producer,
        &captured,
        &version(),
        prepared(&captured),
    )
    .unwrap();
    assert!(accept_message(&mut mixer, replacement));
    assert_eq!(
        engine.prepared_source_epochs[0].load(Ordering::Acquire),
        captured_epoch
    );
    assert!(!captured.publication.current());
    let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
        panic!("prepared stems publication");
    };
    assert!(!mixer.publish_prepared_stems(id, stems));
    assert_eq!(captured.publication_status(), "rejected");
}

#[test]
fn prepared_enqueue_rejects_changed_acknowledgement_before_consuming_the_ticket() {
    let engine = test_engine();
    let (producer, mut consumer) = queue(8);
    let mut mixer = acknowledged_mixer(&engine);
    accept(&engine, &producer, &mut consumer, &mut mixer, decision());
    publish(
        &engine,
        &producer,
        &synthetic_ticket(&engine),
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
    let captured = engine.capture_prepared_source(0, version()).unwrap();
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert!(
        enqueue_current_prepared_stems(
            &engine,
            &producer,
            &captured,
            &version(),
            prepared(&captured),
        )
        .is_err()
    );
    assert_eq!(captured.publication_status(), "captured");
    assert!(consumer.pop().is_err());
}

#[test]
fn same_legacy_authority_edit_retires_preparation_without_promoting_accepted_evidence() {
    let engine = test_engine();
    let (producer, mut consumer) = queue(8);
    set_intent(&engine, &producer, 0, TimingIntent::Legacy).unwrap();
    consumer.pop().unwrap();
    let captured = engine.capture_prepared_source(0, version()).unwrap();
    assert_eq!(captured.publication.accepted_projection(), None);
    set_intent(&engine, &producer, 0, TimingIntent::Legacy).unwrap();
    assert!(!captured.publication.current());
    consumer.pop().unwrap();
    assert!(
        enqueue_current_prepared_stems(
            &engine,
            &producer,
            &captured,
            &version(),
            prepared(&captured),
        )
        .is_err()
    );
    assert_eq!(captured.publication_status(), "captured");
    let current = engine.capture_prepared_source(0, version()).unwrap();
    assert_eq!(current.publication.accepted_projection(), None);
    assert!(current.publication.current());
}

#[test]
fn accepted_preparation_revalidates_loaded_generation_digest_rate_and_immutable_source_owner() {
    for changed in 0..4 {
        let engine = test_engine();
        let (producer, mut consumer) = queue(8);
        let mut mixer = acknowledged_mixer(&engine);
        accept(&engine, &producer, &mut consumer, &mut mixer, decision());
        // Independently vary loader-retained source facts while leaving sample
        // values/shape and the acknowledged record unchanged. None can authorize
        // Automatic preparation using a historical record for another source.
        match changed {
            0 => engine.loaded_source_generations.lock().unwrap()[0].0 += 1,
            1 => engine.loaded_source_generations.lock().unwrap()[0].1 += 1,
            2 => engine.loaded_source_digests.lock().unwrap()[0] = Some("c".repeat(64)),
            _ => engine.sample_cache.lock().unwrap()[0] = Some(source()),
        }
        assert!(engine.capture_prepared_source(0, version()).is_err());
        assert!(consumer.pop().is_err());
    }
}

#[test]
fn queued_prepared_stems_reject_source_revocation_before_old_callback_bank_changes() {
    for replacement in [false, true] {
        let engine = test_engine();
        let (producer, mut consumer) = queue(8);
        let mut mixer = acknowledged_mixer(&engine);
        accept(&engine, &producer, &mut consumer, &mut mixer, decision());
        let captured = engine.capture_prepared_source(0, version()).unwrap();
        enqueue_current_prepared_stems(
            &engine,
            &producer,
            &captured,
            &version(),
            prepared(&captured),
        )
        .unwrap();
        // Execute the production checked request/authority transitions used by
        // load/unload admission, before their bank update reaches the callback.
        {
            let mut requests = engine.pad_request_ids.lock().unwrap();
            let advance =
                PadRequestAdvance::prepare(&mut requests[0], &engine.prepared_source_epochs[0])
                    .unwrap();
            let authority = engine.input_runtime_ownership.next_authority(0).unwrap();
            engine.input_runtime_ownership.revoke(0, authority);
            advance.commit();
            engine.sample_cache.lock().unwrap()[0] = None;
            engine.loaded_source_digests.lock().unwrap()[0] = None;
        }
        if replacement {
            mixer.load_sample(0, source());
        }
        let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
            panic!("prepared stems publication");
        };
        assert!(!mixer.publish_prepared_stems(id, stems));
        assert_eq!(captured.publication_status(), "rejected");
        assert!(engine.capture_prepared_source(0, version()).is_err());
    }
}

#[test]
fn legacy_parameter_lane_gap_rejects_fresh_preparation_and_keeps_previous_stem_audio() {
    let engine = test_engine();
    let (producer, mut consumer) = queue(8);
    let mut mixer = acknowledged_mixer(&engine);
    accept(&engine, &producer, &mut consumer, &mut mixer, decision());
    let admitted = engine.capture_prepared_source(0, version()).unwrap();
    let mut stems = prepared(&admitted);
    stems.stems[0].samples = Arc::from(
        admitted
            .sample
            .samples
            .iter()
            .map(|sample| sample * 0.25)
            .collect::<Vec<_>>(),
    );
    enqueue_current_prepared_stems(&engine, &producer, &admitted, &version(), stems).unwrap();
    let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
        panic!("prepared stems publication");
    };
    assert!(mixer.publish_prepared_stems(id, stems));
    let source_hash = source_version_hash(&version());
    assert!(mixer.set_stem_mix_mode(0, crate::messages::StemMixMode::AllStems, source_hash));
    assert!(mixer.set_stem_enabled_mask(0, crate::messages::STEM_MASK_VOCALS, source_hash));
    // Admit the actual old source/stem voice while its accepted authority is still current.
    assert!(mixer.play_sample(0, 1.0));

    let (parameters, mut parameter_consumer) = rtrb::RingBuffer::new(1);
    let parameters = Arc::new(Mutex::new(parameters));
    publish_legacy_bpm(&engine, &parameters, 0, Some(120.0)).unwrap();
    let early = engine.capture_prepared_source(0, version()).unwrap();
    assert_eq!(early.publication.accepted_projection(), None);
    enqueue_current_prepared_stems(&engine, &producer, &early, &version(), prepared(&early))
        .unwrap();
    let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
        panic!("prepared stems publication");
    };
    assert!(!mixer.publish_prepared_stems(id, stems));
    assert_eq!(early.publication_status(), "rejected");
    assert_eq!(admitted.publication_status(), "accepted");
    // Revocation fences new admission while preserving already effective source/stem audio.
    assert!(!mixer.play_sample(0, 1.0));
    let mut output = [0.0; 4];
    mixer.render(&mut output, &mut [0.0; NUM_SAMPLES]);
    assert_eq!(output, [0.25, -0.125, 0.0625, -0.03125]);
    mixer.stop_sample(0);

    let ControlParameterMessage::SetLegacyPadBpm {
        id,
        bpm,
        through_epoch,
    } = parameter_consumer.pop().unwrap()
    else {
        panic!("legacy BPM parameter");
    };
    mixer.clear_constant_timing(id, through_epoch);
    mixer.set_pad_bpm(id, bpm);
    let current = engine.capture_prepared_source(0, version()).unwrap();
    enqueue_current_prepared_stems(&engine, &producer, &current, &version(), prepared(&current))
        .unwrap();
    let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
        panic!("prepared stems publication");
    };
    assert!(mixer.publish_prepared_stems(id, stems));
    assert_eq!(current.publication_status(), "accepted");
}
