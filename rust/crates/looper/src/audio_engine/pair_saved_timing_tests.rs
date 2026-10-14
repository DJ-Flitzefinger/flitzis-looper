//! Original saved timing work follows actual Native Pair ACKs without numeric recapture.

use super::*;
use crate::audio_engine::AudioEngine;
use crate::audio_engine::constant_timing::{self, SavedTimingExportWork, SavedTimingRestoreWork};
use crate::audio_engine::pad_pair::{self, PadPairTicket};
use crate::audio_engine::prepared_source::file_sha256;
use crate::audio_engine::sample_loader::decode_audio_file_to_sample_buffer;
use crate::messages::SampleBuffer;
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use std::path::Path;
use std::sync::atomic::AtomicU8;

const RATE: u32 = 8_000;

fn process(mixer: &mut RtMixer, message: ControlMessage) {
    process_control_message(
        message,
        &mut FixedCapacityScheduler::<8>::new(),
        0,
        &mut TriggerQuantization::Immediate,
        &mut TransportTimeline::new(RATE),
        mixer,
        &mut Vec::<AudioMessage>::new(),
        &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
    );
}

fn install_source(
    engine: &AudioEngine,
    mixer: &mut RtMixer,
    slot: usize,
    sample: SampleBuffer,
    generation: u64,
    request: u64,
    digest: String,
    intent: TimingIntent,
) {
    let ownership = &engine.input_runtime_ownership;
    let before = ownership.slot_snapshot(slot).unwrap();
    let lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
    let write = ownership
        .prepare_slot_write_fixed(before, lifetime)
        .unwrap();
    let publication = ownership
        .publish_source_write(
            &write,
            &sample,
            RATE,
            generation,
            ownership.next_authority(slot).unwrap(),
            Some(intent),
            true,
            || true,
        )
        .unwrap();
    engine.sample_cache.lock().unwrap()[slot] = Some(sample.clone());
    engine.loaded_source_generations.lock().unwrap()[slot] = (generation, RATE);
    engine.loaded_source_digests.lock().unwrap()[slot] = Some(digest);
    engine.pad_request_ids.lock().unwrap()[slot] = request;
    engine.timing_intents.lock().unwrap()[slot] = intent;
    let adoption = Arc::new(AtomicU8::new(0));
    process(
        mixer,
        ControlMessage::LoadColdSample {
            publication,
            id: slot,
            sample,
            source_generation: generation,
            content_lifetime: lifetime,
            adoption: adoption.clone(),
            epoch: None,
            captured_epoch: 0,
            replace_assignment: false,
            loop_region: None,
            resident_cancelled: None,
            incoming_defaults: None,
        },
    );
    assert_eq!(
        adoption.load(Ordering::Acquire),
        2,
        "actual source adoption ACK"
    );
}

fn pair_ack(
    engine: &AudioEngine,
    consumer: &mut Consumer<ControlMessage>,
    mixer: &mut RtMixer,
) -> PadPairTicket {
    let captured = engine._capture_pad_pair(0, 1).unwrap();
    let ticket = pad_pair::prepare(engine, &captured, RATE, 1).unwrap();
    engine._publish_pad_pair(&ticket).unwrap();
    process(mixer, consumer.pop().unwrap());
    assert!(ticket.own_acknowledgement().is_some());
    ticket
}

fn finish_pair(engine: &AudioEngine, ticket: &PadPairTicket) {
    let own = ticket.own_acknowledgement().unwrap();
    assert!(engine._reconcile_pad_pair(ticket).unwrap());
    assert!(
        engine
            ._validate_pad_pair_acknowledgement(ticket, &own)
            .unwrap()
    );
    engine._finish_pad_pair(ticket, &own).unwrap();
    for slot in [0, 1] {
        assert_eq!(engine.input_runtime_ownership.pair_reservation(slot), 0);
    }
}

#[test]
fn original_detached_saved_export_and_restore_follow_pair_then_reject_new_source_request() {
    let (path, envelope) = constant_timing::migration_test_fixture();
    let sample = decode_audio_file_to_sample_buffer(Path::new(&path), 1, RATE, |_| {}).unwrap();
    let digest = file_sha256(Path::new(&path)).unwrap();
    let mut engine = AudioEngine::new().unwrap();
    let (producer, mut consumer) = RingBuffer::new(4);
    let producer = Arc::new(Mutex::new(producer));
    let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
    engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback))));
    let mut mixer = RtMixer::new(1, RATE as f32);
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
    install_source(
        &engine,
        &mut mixer,
        0,
        sample.clone(),
        71,
        91,
        digest,
        TimingIntent::Automatic,
    );
    install_source(
        &engine,
        &mut mixer,
        1,
        SampleBuffer {
            samples: Arc::from([-0.5; 128]),
            channels: 1,
            residency: None,
        }
        .with_complete_source(RATE),
        81,
        17,
        "b".repeat(64),
        TimingIntent::Manual,
    );
    let original_key = engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap()
        .content_key()
        .unwrap();
    let first_saved = constant_timing::capture_saved(&engine, 0, &envelope, path.clone()).unwrap();
    let initial = SavedTimingRestoreWork::capture(&engine, &first_saved)
        .unwrap()
        .prepare()
        .unwrap()
        .finish(&engine, &producer)
        .unwrap();
    process(&mut mixer, consumer.pop().unwrap());
    assert_eq!(initial.publication_status().unwrap(), "accepted");
    let export = SavedTimingExportWork::capture(&engine, 0, path.clone())
        .unwrap()
        .unwrap()
        .prepare()
        .unwrap();
    let requests = engine.pad_request_ids.lock().unwrap().clone();
    let incarnation = engine
        .input_runtime_ownership
        .key_source_request_incarnation(original_key)
        .unwrap();
    let first_pair = pair_ack(&engine, &mut consumer, &mut mixer);
    assert!(matches!(
        engine
            .input_runtime_ownership
            .observe_control_key(original_key),
        crate::audio_engine::content_placement::PlacementObservation::Pending
    ));
    finish_pair(&engine, &first_pair);
    assert_eq!(
        engine
            .input_runtime_ownership
            .slot_snapshot(1)
            .unwrap()
            .content_key(),
        Some(original_key)
    );
    assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests);
    assert_eq!(
        engine
            .input_runtime_ownership
            .key_source_request_incarnation(original_key),
        Some(incarnation)
    );
    assert_eq!(
        export.finish(&engine).unwrap(),
        Some(envelope.clone()),
        "original exported source and timing ACK resolve the moved owner"
    );

    let saved = constant_timing::capture_saved(&engine, 1, &envelope, path.clone()).unwrap();
    let restoration = SavedTimingRestoreWork::capture(&engine, &saved)
        .unwrap()
        .prepare()
        .unwrap();
    let requests = engine.pad_request_ids.lock().unwrap().clone();
    let incarnation = engine
        .input_runtime_ownership
        .key_source_request_incarnation(original_key)
        .unwrap();
    let second_pair = pair_ack(&engine, &mut consumer, &mut mixer);
    finish_pair(&engine, &second_pair);
    assert_eq!(
        engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap()
            .content_key(),
        Some(original_key)
    );
    assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests);
    assert_eq!(
        engine
            .input_runtime_ownership
            .key_source_request_incarnation(original_key),
        Some(incarnation)
    );
    let restored = restoration.finish(&engine, &producer).unwrap();
    let message = consumer.pop().unwrap();
    assert!(
        matches!(
            &message,
            ControlMessage::PublishConstantTiming { id: 0, .. }
        ),
        "opaque original restore publishes to its resolved owner, not captured slot 1"
    );
    process(&mut mixer, message);
    assert_eq!(restored.publication_status().unwrap(), "accepted");
    assert!(
        engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .same_source(&sample)
    );
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[0],
        (71, RATE)
    );
    assert_eq!(engine.pad_timing_intent(0).unwrap(), "automatic");
    assert_eq!(engine.pad_timing_intent(1).unwrap(), "manual");
    assert_eq!(
        constant_timing::export_current(&engine, 0, path.clone()).unwrap(),
        Some(envelope.clone())
    );

    let still_original =
        constant_timing::capture_saved(&engine, 0, &envelope, path.clone()).unwrap();
    let obsolete_work = SavedTimingRestoreWork::capture(&engine, &still_original)
        .unwrap()
        .prepare()
        .unwrap();
    let _successor = constant_timing::capture_saved(&engine, 0, &envelope, path).unwrap();
    assert_eq!(
        engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap()
            .content_key(),
        Some(original_key)
    );
    assert!(
        obsolete_work.finish(&engine, &producer).is_err(),
        "later logical request supersedes otherwise equal owner/source metadata"
    );
    assert!(SavedTimingRestoreWork::capture(&engine, &still_original).is_err());
    assert!(consumer.is_empty());
}
