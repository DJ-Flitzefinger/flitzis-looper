//! Real empty-slot BPM messages retain their own fences across unrelated source publication.

use super::*;
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
use crate::messages::SampleBuffer;
use pyo3::Python;
use std::sync::atomic::Ordering;

const TARGET: usize = 36;
const OTHER: usize = 215;
const RATE: u32 = 48_000;

fn parameter_fixture() -> (
    super::super::AudioEngine,
    Box<RtMixer>,
    Consumer<ControlParameterMessage>,
) {
    let (mut engine, mut mixer) = fixture();
    let source = engine.sample_cache.lock().unwrap()[OTHER]
        .as_ref()
        .unwrap()
        .clone();
    mixer.load_sample_rt(
        OTHER,
        source,
        &mut super::super::buffer_retirement::ImmediateAudioBufferRetirement,
    );
    let (producer, consumer) = RingBuffer::new(8);
    engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
    (engine, mixer, consumer)
}

fn move_for_parameter_test(engine: &super::super::AudioEngine, mixer: &mut RtMixer) {
    let owner = &engine.input_runtime_ownership;
    let pair = owner
        .prepare_placement_pair(
            owner.slot_snapshot(OTHER).unwrap(),
            owner.slot_snapshot(TARGET).unwrap(),
        )
        .unwrap();
    owner
        .commit_placement_pair(
            &pair,
            true,
            || true,
            || mixer.swap_occupied_images_for_key_lock_test(OTHER, TARGET),
        )
        .unwrap();
}

#[test]
fn actual_gain_eq_admissions_follow_original_content_before_record_and_before_apply() {
    for recorded in [false, true] {
        let (mut engine, mut mixer, mut consumer) = parameter_fixture();
        let key = engine
            .input_runtime_ownership
            .slot_snapshot(OTHER)
            .unwrap()
            .content_key()
            .unwrap();
        engine.set_pad_gain(OTHER, -9.0).unwrap();
        engine.set_pad_eq(OTHER, -6.0, 1.0, 0.0).unwrap();
        let mut pending = PendingControlParameters::default();
        if recorded {
            while let Ok(message) = consumer.pop() {
                pending.record(message, &mixer);
            }
        }
        move_for_parameter_test(&engine, &mut mixer);
        assert_eq!(
            engine.input_runtime_ownership.callback_key_slot(key),
            Some(TARGET)
        );
        // Refill the old position with equal PCM and an equal numeric parameter epoch.
        // Its new logical key cannot adopt either admitted edit.
        let source = engine.sample_cache.lock().unwrap()[OTHER]
            .as_ref()
            .unwrap()
            .clone();
        assign(&engine.input_runtime_ownership, OTHER, &source);
        mixer.load_sample_rt(
            OTHER,
            source,
            &mut super::super::buffer_retirement::ImmediateAudioBufferRetirement,
        );
        assert_ne!(
            engine
                .input_runtime_ownership
                .slot_snapshot(OTHER)
                .unwrap()
                .content_key(),
            Some(key)
        );
        assert_eq!(
            engine.input_runtime_ownership.copy_parameter_epoch(OTHER),
            engine.input_runtime_ownership.copy_parameter_epoch(TARGET)
        );
        while let Ok(message) = consumer.pop() {
            pending.record(message, &mixer);
        }
        assert_eq!(
            pending.apply_to(&mut mixer, &mut TransportTimeline::new(RATE)),
            2
        );
        assert_eq!(mixer.parameter_targets_for_test(TARGET).0, -9.0);
        assert_ne!(mixer.parameter_targets_for_test(TARGET).1, [0.5; 3]);
        assert_eq!(mixer.parameter_targets_for_test(OTHER), (0.0, [0.5; 3]));
    }
}

#[test]
fn obsolete_empty_parameters_cannot_replace_current_loaded_edits_with_equal_epoch() {
    let (mut engine, mut mixer, mut consumer) = parameter_fixture();
    engine.set_pad_gain(TARGET, -18.0).unwrap();
    engine.set_pad_eq(TARGET, -10.0, -5.0, 0.0).unwrap();
    let stale = [consumer.pop().unwrap(), consumer.pop().unwrap()];
    move_for_parameter_test(&engine, &mut mixer);
    engine.set_pad_gain(TARGET, -3.0).unwrap();
    engine.set_pad_eq(TARGET, 1.0, 0.0, 2.0).unwrap();
    let mut pending = PendingControlParameters::default();
    while let Ok(message) = consumer.pop() {
        pending.record(message, &mixer);
    }
    for message in stale {
        pending.record(message, &mixer);
    }
    assert_eq!(
        pending.apply_to(&mut mixer, &mut TransportTimeline::new(RATE)),
        2
    );
    assert_eq!(mixer.parameter_targets_for_test(TARGET).0, -3.0);
    let current = mixer.parameter_targets_for_test(TARGET).1;
    assert!(current[0] > 0.5 && current[1] == 0.5 && current[2] > 0.5);
}

fn fixture() -> (super::super::AudioEngine, Box<RtMixer>) {
    Python::initialize();
    let engine = super::super::AudioEngine::new().unwrap();
    let source = SampleBuffer {
        samples: Arc::from([0.25_f32; 8]),
        channels: 1,
        residency: None,
    }
    .with_complete_source(RATE);
    assign(&engine.input_runtime_ownership, OTHER, &source);
    engine.sample_cache.lock().unwrap()[OTHER] = Some(source);
    let mut mixer = create_stream_mixer(
        1,
        RATE,
        engine.current_timing_acknowledgements.clone(),
        engine.input_runtime_ownership.clone(),
        engine.prepared_source_epochs.clone(),
    )
    .unwrap();
    mixer.set_pad_bpm(TARGET, Some(60.0));
    (engine, mixer)
}

fn assign(ownership: &Arc<InputRuntimeOwnership>, id: usize, source: &SampleBuffer) {
    let before = ownership.slot_snapshot(id).unwrap();
    let life = super::super::content_lifetime::reserve().unwrap();
    let write = ownership.prepare_slot_write(before, life).unwrap();
    ownership
        .publish_source_write(
            &write,
            source,
            RATE,
            7,
            ownership.next_authority(id).unwrap(),
            None,
            false,
            || true,
        )
        .unwrap();
}

fn queued_bpm(engine: &super::super::AudioEngine) -> Consumer<ControlParameterMessage> {
    let (producer, consumer) = RingBuffer::new(1);
    super::super::constant_timing::publish_legacy_bpm(
        engine,
        &Arc::new(Mutex::new(producer)),
        TARGET,
        Some(97.0),
    )
    .unwrap();
    consumer
}

#[test]
fn empty_slot_bpm_survives_pending_before_drain_after_pop_and_before_apply() {
    for stage in 0..3 {
        let (engine, mut mixer) = fixture();
        let mut consumer = queued_bpm(&engine);
        let ownership = &engine.input_runtime_ownership;
        let target = ownership.slot_snapshot(TARGET).unwrap();
        assert_eq!(target.lifetime(), 0);
        let other = ownership.slot_snapshot(OTHER).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(other, other.lifetime())
            .unwrap();
        let mut pending = PendingControlParameters::default();
        let mut popped = None;
        if stage != 0 {
            popped = Some(consumer.pop().unwrap());
        }
        if stage == 2 {
            pending.record(popped.take().unwrap(), &mixer);
        }
        let mut transport = TransportTimeline::new(RATE);
        ownership
            .commit_slot_write_fixed(
                &write,
                || true,
                |_| {
                    assert!(ownership.placement_pending());
                    assert!(!ownership.slot_snapshot_current(target));
                    if stage == 0 {
                        let result =
                            drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
                        assert_eq!(result.messages_drained, 1);
                        assert_eq!(result.parameters_applied, 1);
                    } else {
                        if stage == 1 {
                            pending.record(popped.take().unwrap(), &mixer);
                        }
                        assert_eq!(
                            std::mem::take(&mut pending).apply_to(&mut mixer, &mut transport),
                            1
                        );
                    }
                    assert!((mixer.output_bpm_for_sample_id(TARGET).unwrap() - 97.0).abs() < 1e-10);
                },
            )
            .unwrap();
        assert!(consumer.pop().is_err());
        let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
        assert_eq!(result.messages_drained, 0);
        assert_eq!(result.parameters_applied, 0);
        assert_eq!(ownership.slot_snapshot(TARGET).unwrap().content_key(), None);
        assert_eq!(
            engine
                .current_timing_acknowledgements
                .current_for_slot(ownership, TARGET),
            0
        );
    }
}

#[test]
fn empty_slot_bpm_rejects_real_replacement_and_empty_slot_aba_before_record_or_apply() {
    for aba in [false, true] {
        for already_recorded in [false, true] {
            let (engine, mut mixer) = fixture();
            let mut consumer = queued_bpm(&engine);
            let message = consumer.pop().unwrap();
            let mut pending = PendingControlParameters::default();
            let mut message = Some(message);
            if already_recorded {
                pending.record(message.take().unwrap(), &mixer);
            }
            let ownership = &engine.input_runtime_ownership;
            let original = ownership.slot_snapshot(TARGET).unwrap();
            let source = SampleBuffer {
                samples: Arc::from([0.5_f32; 8]),
                channels: 1,
                residency: None,
            }
            .with_complete_source(RATE);
            assign(ownership, TARGET, &source);
            let current = ownership.slot_snapshot(TARGET).unwrap();
            let key = current.content_key().unwrap();
            assert!(
                engine
                    .current_timing_acknowledgements
                    .acknowledge_for_key(ownership, key, 7)
            );
            if aba {
                let write = ownership.prepare_slot_write(current, 0).unwrap();
                ownership
                    .retire_source_write(&write, ownership.next_authority(TARGET).unwrap(), || true)
                    .unwrap();
            }
            assert!(!ownership.slot_snapshot_fields_current(original));
            assert!(
                ownership.slot_snapshot(TARGET).unwrap().binding_epoch() > original.binding_epoch()
            );
            let expected_epoch = engine
                .prepared_source_epoch(TARGET)
                .unwrap()
                .load(Ordering::Acquire);
            if !already_recorded {
                pending.record(message.take().unwrap(), &mixer);
            }
            let mut transport = TransportTimeline::new(RATE);
            assert_eq!(pending.apply_to(&mut mixer, &mut transport), 0);
            assert_eq!(mixer.output_bpm_for_sample_id(TARGET), Some(60.0));
            assert_eq!(
                engine
                    .prepared_source_epoch(TARGET)
                    .unwrap()
                    .load(Ordering::Acquire),
                expected_epoch
            );
            assert_eq!(
                engine
                    .current_timing_acknowledgements
                    .current_for_slot(ownership, TARGET),
                if aba { 0 } else { 7 }
            );
        }
    }
}
