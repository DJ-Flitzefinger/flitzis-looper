//! Already resident starts still use real guarded command ACK and native PCM.
use super::*;
use crate::audio_engine::cold_jobs::{QUEUED_JOBS, WORKERS};
use crate::audio_engine::input_runtime_binding::enqueue_stop_with_producer;
use crate::audio_engine::prepared_source::{
    PreparedSourceTicket, enqueue_current_prepared_stems_with_owner,
};
use crate::audio_engine::resident_relocation::launch_with_producer;
use crate::messages::{PreparedStemSet, StemMixMode};
use pyo3::{PyResult, Python};
use std::sync::{Condvar, mpsc};

fn current_content(loaded: &Loaded) -> crate::audio_engine::content_placement::ContentKey {
    loaded
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap()
        .content_key()
        .unwrap()
}

fn captured_start(
    loaded: &Loaded,
    timestamp: u64,
) -> crate::audio_engine::pad_start::CapturedPadStart {
    let Ok(ControlMessage::CapturePadStartTarget(start)) = loaded.consumer.peek() else {
        panic!("resident launch must retain its actual original target capture");
    };
    assert_eq!(start.received_timestamp(), timestamp);
    assert_eq!(start.content_key(), current_content(loaded));
    assert!(start.belongs_to(&loaded.engine.input_runtime_ownership));
    assert!(start.is_published());
    start.clone()
}

fn drain_current(loaded: &mut Loaded) -> usize {
    crate::audio_engine::audio_stream::drain_control_messages(
        &mut loaded.consumer,
        &mut loaded.callback.scheduler,
        loaded.callback.transport.output_frame(),
        &mut loaded.callback.quantization,
        &mut loaded.callback.transport,
        &mut loaded.callback.mixer,
        &mut loaded.callback.feedback,
        &mut loaded.callback.retirement,
    )
}

fn render_current(loaded: &mut Loaded, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames * 2];
    crate::audio_engine::audio_stream::render_scheduled_audio(
        &mut loaded.callback.mixer,
        &mut loaded.callback.scheduler,
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        loaded.callback.transport.output_frame(),
        2,
        &mut loaded.callback.transport,
        &mut loaded.callback.feedback,
        &mut loaded.callback.retirement,
    );
    loaded.callback.transport.advance_by_rendered_frames(frames);
    output
}

/// Actual workers hold a deterministic gate; all 32 remaining jobs are queued.
/// Release on unwind too, so a failed pre-fix assertion cannot hang engine Drop.
struct ColdLaneGate(Arc<(Mutex<bool>, Condvar)>);

impl ColdLaneGate {
    fn occupy(engine: &AudioEngine) -> Self {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (started, received) = mpsc::channel();
        for _ in 0..WORKERS {
            let gate = gate.clone();
            let started = started.clone();
            engine
                .cold_jobs
                .submit(engine.cold_jobs.reserve().unwrap(), move || {
                    started.send(()).unwrap();
                    let mut open = gate.0.lock().unwrap();
                    while !*open {
                        open = gate.1.wait(open).unwrap();
                    }
                })
                .unwrap();
        }
        for _ in 0..WORKERS {
            received.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        for _ in 0..QUEUED_JOBS {
            engine
                .cold_jobs
                .submit(engine.cold_jobs.reserve().unwrap(), || {})
                .unwrap();
        }
        assert_eq!(
            engine.cold_jobs.counts_for_test(),
            (WORKERS, QUEUED_JOBS, WORKERS + QUEUED_JOBS)
        );
        Self(gate)
    }
}

impl Drop for ColdLaneGate {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

fn same_request() -> WindowRequest {
    WindowRequest {
        loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
        key_lock: Some(false),
        ..WindowRequest::default()
    }
}

/// Synthetic separator PCM, accepted through the real queued publication/ACK.
/// The descriptor deliberately has no WAV files: a ready trigger must never
/// reread those artifacts, whereas actual changed storage still needs its worker.
fn prepared_stems(
    loaded: &Loaded,
    identity: u8,
    amplitude: f32,
) -> (PreparedSourceTicket, PreparedStemSet) {
    let reference = loaded.sample();
    let digest = loaded.engine.loaded_source_digests.lock().unwrap()[0]
        .clone()
        .unwrap();
    let ticket = loaded
        .engine
        .capture_prepared_source(0, format!("ready-fixture|sha256-v1:{digest}"))
        .unwrap();
    let set = PreparedStemSet {
        complete_set_identity: Arc::new([identity; 32]),
        accepted_timing: ticket.publication.accepted_projection(),
        reference_samples: reference.samples.clone(),
        reference_residency: reference.residency.clone(),
        publication: ticket.publication.clone(),
        source_version_hash: 17,
        sample_rate_hz: 48_000,
        channels: 2,
        frame_count: reference.frame_count(),
        available_mask: ((1_u16 << crate::messages::STEM_BUFFER_COUNT) - 1) as u8,
        stems: std::array::from_fn(|index| SampleBuffer {
            channels: 2,
            samples: Arc::from(vec![
                amplitude * (index + 1) as f32;
                reference.samples.len()
            ]),
            // Each component owns different PCM; retain the immutable complete source
            // identity and geometry in a distinct resident owner view.
            residency: reference
                .residency
                .as_ref()
                .map(|view| Arc::new((**view).clone())),
        }),
    };
    (ticket, set)
}

fn enqueue_stems(
    loaded: &Loaded,
    ticket: &PreparedSourceTicket,
    set: &PreparedStemSet,
    producer: &Arc<Mutex<rtrb::Producer<ControlMessage>>>,
) -> PyResult<()> {
    enqueue_current_prepared_stems_with_owner(
        &loaded.engine,
        producer,
        ticket,
        &ticket.source_version,
        set.clone(),
        (
            PathBuf::from("samples/stems/ready-fixture"),
            loaded.directory.path().join("absent-ready-fixture"),
        ),
    )
}

fn accepted_stems(loaded: &mut Loaded) -> PreparedStemSet {
    let (ticket, set) = prepared_stems(loaded, 17, 0.125);
    enqueue_stems(loaded, &ticket, &set, &loaded.producer).unwrap();
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    reconcile(&loaded.engine).unwrap();
    set
}

fn launch_and_render(loaded: &mut Loaded, ticket: &ResidentWindowTicket, stem_value: Option<f32>) {
    assert!(loaded.callback.feedback_rx.is_empty());
    let timestamp = loaded.engine.input_clock.capture_ns();
    assert!(
        launch_with_producer(&loaded.engine, ticket, false, timestamp, &loaded.producer).unwrap()
    );
    let original = captured_start(loaded, timestamp);
    assert_eq!(drain_current(loaded), 2);
    assert_eq!(
        original.original_frame(),
        Some(loaded.callback.transport.output_frame())
    );
    assert_eq!(original.effective_frame(), original.original_frame());
    assert_eq!(original.state(), "applied");
    assert!(
        loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    let output = render_current(loaded, 300);
    for (frame, stereo) in output.chunks_exact(2).enumerate().skip(128) {
        let expected = stem_value.unwrap_or(loaded.mono[32 + frame % 32]);
        for value in stereo {
            assert_eq!(value.to_bits(), expected.to_bits(), "frame={frame}");
        }
    }
    let started = loaded.callback.feedback_rx.pop().unwrap();
    ExpectedPadFeedback::capture(&loaded.engine, 0, 1).assert_current(&loaded.engine, &started);
    assert!(loaded.callback.feedback_rx.is_empty());
}

#[test]
fn ready_fullmix_and_stems_start_with_real_ack_while_entire_cold_lane_is_occupied() {
    Python::initialize();
    for mode in [
        Some(StemMixMode::AllStems),
        Some(StemMixMode::FullMix),
        None,
    ] {
        let mut loaded = Loaded::new();
        let original = loaded.sample();
        let stems = mode.map(|mode| {
            let stems = accepted_stems(&mut loaded);
            assert!(loaded.callback.mixer.set_stem_mix_mode(0, mode, 17));
            assert!(loaded.callback.mixer.set_stem_enabled_mask(0, 1, 17));
            stems
        });
        let _blocked = ColdLaneGate::occupy(&loaded.engine);
        let counts = loaded.engine.cold_jobs.counts_for_test();
        let ticket = loaded.prepare(same_request());
        assert_eq!(loaded.engine.cold_jobs.counts_for_test(), counts);
        assert_eq!(ticket.publication_status(), "pending");
        assert!(
            !launch_with_producer(
                &loaded.engine,
                &ticket,
                false,
                loaded.engine.input_clock.capture_ns(),
                &loaded.producer
            )
            .unwrap()
        );
        loaded.callback.retirement.capacity = 0;
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 0);
        assert_eq!(ticket.publication_status(), "pending");
        loaded.callback.retirement.capacity = usize::MAX;
        loaded.adopt(&ticket);
        let adopted = loaded.sample();
        assert!(adopted.same_window(&original));
        assert!(Arc::ptr_eq(&adopted.samples, &original.samples));
        if let Some(stems) = &stems {
            // Actual control-side reconciliation owns the same prepared readers.
            // A subsequent readiness request exposes them in the queued payload.
            let check = loaded.prepare(same_request());
            let Ok(ControlMessage::RelocateResident(transaction)) = loaded.consumer.peek() else {
                panic!("expected bounded ready transaction");
            };
            let native = transaction.stems.as_ref().unwrap();
            assert!(Arc::ptr_eq(
                &native.complete_set_identity,
                &stems.complete_set_identity
            ));
            for (actual, old) in native.stems.iter().zip(&stems.stems) {
                assert!(Arc::ptr_eq(&actual.samples, &old.samples));
            }
            loaded.adopt(&check);
            launch_and_render(
                &mut loaded,
                &check,
                (mode == Some(StemMixMode::AllStems)).then_some(0.125),
            );
        } else {
            launch_and_render(&mut loaded, &ticket, None);
        }
        assert_eq!(loaded.engine.cold_jobs.counts_for_test(), counts);
    }
}

#[test]
fn ready_retrigger_stop_mode_mask_and_new_context_keep_native_guards() {
    Python::initialize();
    let mut loaded = Loaded::new();
    let stems = accepted_stems(&mut loaded);
    let _blocked = ColdLaneGate::occupy(&loaded.engine);
    let ready = loaded.prepare(same_request());
    loaded.adopt(&ready);
    launch_and_render(&mut loaded, &ready, None);
    assert!(enqueue_stop_with_producer(
        &loaded.engine.input_runtime_ownership,
        &mut loaded.producer.lock().unwrap(),
        Some(0)
    ));
    let stopped = ExpectedPadFeedback::capture(&loaded.engine, 0, 2);
    assert_eq!(drain_current(&mut loaded), 1);
    stopped.assert_current(&loaded.engine, &loaded.callback.feedback_rx.pop().unwrap());
    assert!(loaded.callback.feedback_rx.is_empty());
    assert!(ready.is_current(), "STOP must retain ACK readiness");
    assert!(
        !loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert!(ready.cancel_launch());
    let fresh = loaded.prepare(same_request());
    loaded.adopt(&fresh);
    assert!(!ready.is_current(), "older launch intent cannot revive");
    assert!(
        loaded
            .callback
            .mixer
            .set_stem_mix_mode(0, StemMixMode::AllStems, 17)
    );
    assert!(loaded.callback.mixer.set_stem_enabled_mask(0, 2, 17));
    launch_and_render(&mut loaded, &fresh, Some(0.25));
    let check = loaded.prepare(same_request());
    let Ok(ControlMessage::RelocateResident(transaction)) = loaded.consumer.peek() else {
        panic!("expected bounded ready transaction");
    };
    assert!(Arc::ptr_eq(
        &transaction.stems.as_ref().unwrap().complete_set_identity,
        &stems.complete_set_identity
    ));
    loaded.adopt(&check);
    for request in [
        WindowRequest {
            loop_region: Some((80.0 / 48_000.0, Some(111.0 / 48_000.0))),
            ..WindowRequest::default()
        },
        WindowRequest {
            loop_region: Some((0.0, None)),
            ..WindowRequest::default()
        },
        WindowRequest {
            key_lock: Some(true),
            ..same_request()
        },
    ] {
        let error =
            prepare_window_with_producer(&loaded.engine, 0, request, loaded.producer.clone())
                .err()
                .unwrap();
        assert!(error.to_string().contains("cold source queue full"));
        assert!(
            check.is_current(),
            "failed admission cannot supersede ready ownership"
        );
    }
}

#[test]
fn ready_transaction_rejects_stale_timing_source_and_stem_set_before_ack() {
    Python::initialize();
    for change in 0..3 {
        let mut loaded = Loaded::new();
        let _stems = accepted_stems(&mut loaded);
        let _blocked = ColdLaneGate::occupy(&loaded.engine);
        let ticket = loaded.prepare(same_request());
        match change {
            0 => {
                loaded
                    .engine
                    .input_runtime_ownership
                    .authority_for_cell(
                        loaded
                            .engine
                            .input_runtime_ownership
                            .slot_snapshot(0)
                            .unwrap()
                            .cell(),
                    )
                    .fetch_add(1, Ordering::AcqRel);
            }
            1 => {
                loaded.engine.input_runtime_ownership.revoke_source(0);
            }
            _ => {
                let (_replacement_ticket, replacement) = prepared_stems(&loaded, 18, 0.04);
                replacement.publication.mark_pending().unwrap();
                let publication = replacement.publication.clone();
                let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
                producer
                    .push(ControlMessage::PublishPreparedStems {
                        id: 0,
                        stems: replacement,
                    })
                    .unwrap();
                assert_eq!(loaded.callback.drain(&mut consumer), 1);
                assert_eq!(publication.status(), "accepted");
            }
        }
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
        assert_eq!(ticket.publication_status(), "rejected");
        assert!(
            !launch_with_producer(
                &loaded.engine,
                &ticket,
                true,
                loaded.engine.input_clock.capture_ns(),
                &loaded.producer
            )
            .unwrap()
        );
        assert!(
            !loaded
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
        );
    }
}

#[test]
fn ready_queue_pressure_and_scheduled_stop_preserve_ack_and_quantization() {
    Python::initialize();
    let mut loaded = Loaded::new();
    let _blocked = ColdLaneGate::occupy(&loaded.engine);
    let ready = loaded.prepare(same_request());
    loaded.adopt(&ready);
    while !loaded.producer.lock().unwrap().is_full() {
        loaded
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
    }
    let error =
        prepare_window_with_producer(&loaded.engine, 0, same_request(), loaded.producer.clone())
            .err()
            .unwrap();
    assert!(error.to_string().contains("resident command queue is full"));
    assert!(
        ready.is_current(),
        "failed command admission cannot supersede ACK"
    );
    while loaded.consumer.pop().is_ok() {}
    loaded.callback.transport.advance_by_rendered_frames(1);
    loaded.callback.quantization = TriggerQuantization::Grid { step_64ths: 16 };
    let timestamp = loaded.engine.input_clock.capture_ns();
    assert!(
        launch_with_producer(&loaded.engine, &ready, false, timestamp, &loaded.producer).unwrap()
    );
    let original = captured_start(&loaded, timestamp);
    assert_eq!(drain_current(&mut loaded), 2);
    assert_eq!(loaded.callback.scheduler.len(), 1);
    assert!(
        !loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert!(enqueue_stop_with_producer(
        &loaded.engine.input_runtime_ownership,
        &mut loaded.producer.lock().unwrap(),
        Some(0)
    ));
    let stopped = ExpectedPadFeedback::capture(&loaded.engine, 0, 2);
    assert_eq!(drain_current(&mut loaded), 1);
    stopped.assert_current(&loaded.engine, &loaded.callback.feedback_rx.pop().unwrap());
    assert!(loaded.callback.feedback_rx.is_empty());
    assert!(ready.is_current());
    let target = loaded.callback.scheduler.peek_next_target_frame().unwrap();
    let event = loaded
        .callback
        .scheduler
        .pop_due_at_callback_start(target)
        .unwrap();
    assert_eq!(original.original_frame(), Some(target));
    assert_eq!(original.received_timestamp(), timestamp);
    assert_eq!(original.content_key(), current_content(&loaded));
    assert!(matches!(&event.command,
        crate::audio_engine::scheduler::ScheduledCommand::PitchAction { action_sequence, .. }
        if *action_sequence == original.sequence()
    ));
    let result = crate::audio_engine::audio_stream::execute_scheduled_command(
        &mut loaded.callback.mixer,
        &mut loaded.callback.transport,
        event.execution_frame,
        event.command,
        &mut loaded.callback.feedback,
        &mut loaded.callback.retirement,
    );
    assert!(matches!(
        result,
        crate::audio_engine::audio_stream::ScheduledExecution::Complete
    ));
    assert_eq!(original.state(), "rejected");
    assert_eq!(original.reason(), Some("launch_cancelled"));
    assert_eq!(original.effective_frame(), None);
    assert!(loaded.callback.scheduler.is_empty());
    assert!(loaded.callback.feedback_rx.is_empty());
    assert!(
        !loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    let fresh = loaded.prepare(same_request());
    loaded.adopt(&fresh);
    loaded.callback.quantization = TriggerQuantization::Immediate;
    launch_and_render(&mut loaded, &fresh, None);
}

#[test]
fn ready_full_feedback_retains_start_until_real_poll_and_next_callback_with_one_birth() {
    Python::initialize();
    let mut loaded = Loaded::new();
    let _blocked = ColdLaneGate::occupy(&loaded.engine);
    let ready = loaded.prepare(same_request());
    loaded.adopt(&ready);
    let key = current_content(&loaded);
    // A completed real STOP fills the unchanged one-slot feedback ring.
    assert!(enqueue_stop_with_producer(
        &loaded.engine.input_runtime_ownership,
        &mut loaded.producer.lock().unwrap(),
        Some(0)
    ));
    let stopped = ExpectedPadFeedback::capture(&loaded.engine, 0, 2);
    assert_eq!(drain_current(&mut loaded), 1);
    stopped.assert_current(&loaded.engine, loaded.callback.feedback_rx.peek().unwrap());
    let timestamp = loaded.engine.input_clock.capture_ns();
    assert!(
        launch_with_producer(&loaded.engine, &ready, false, timestamp, &loaded.producer).unwrap()
    );
    let original = captured_start(&loaded, timestamp);
    let sequence = original.sequence();
    assert_eq!(drain_current(&mut loaded), 2);
    assert!(loaded.consumer.is_empty());
    assert_eq!(original.original_frame(), Some(0));
    assert_eq!(original.execution_frame(), Some(0));
    assert_eq!(original.effective_frame(), None);
    assert_eq!(original.reason(), None);
    assert!(ready.is_current());
    assert_eq!(ready.publication_status(), "accepted");
    assert!(
        !loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    let pending = loaded.callback.scheduler.deferred_due().unwrap();
    assert_eq!(pending.target_frame, 0);
    assert_eq!(pending.execution_frame, 0);
    assert!(matches!(
        &pending.command,
        crate::audio_engine::scheduler::ScheduledCommand::PitchAction { action_sequence, .. }
            if *action_sequence == sequence
    ));
    assert_eq!(original.content_key(), key);
    assert_eq!(original.received_timestamp(), timestamp);
    let previous_stop = loaded.callback.feedback_rx.pop().unwrap();
    stopped.assert_message(&previous_stop);
    assert_eq!(
        loaded.engine._resolve_pad_feedback(&previous_stop).unwrap(),
        None
    );
    assert!(loaded.callback.feedback_rx.is_empty());
    // Free capacity does not cause a same-callback spin or a second admission.
    assert_eq!(drain_current(&mut loaded), 0);
    assert!(loaded.callback.scheduler.deferred_due().is_some());
    assert_eq!(original.execution_frame(), Some(0));
    assert!(
        !loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert!(
        render_current(&mut loaded, 1)
            .iter()
            .all(|value| *value == 0.0)
    );
    assert_eq!(loaded.callback.transport.output_frame(), 1);
    assert_eq!(drain_current(&mut loaded), 0);
    let execution = 8_193;
    assert_eq!(original.original_frame(), Some(0));
    assert_eq!(original.execution_frame(), Some(execution));
    assert_eq!(original.effective_frame(), None);
    assert_eq!(
        loaded.callback.scheduler.peek_next_target_frame(),
        Some(execution)
    );
    assert!(loaded.callback.scheduler.deferred_due().is_none());
    // Host waiting never advances the product clock or admits a replacement gesture.
    wait_until(Duration::from_secs(5), || {
        assert_ne!(original.state(), "rejected", "{:?}", original.reason());
        let runtime = loaded.callback.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        runtime.has_ready_action(sequence)
    });
    assert_eq!(loaded.callback.transport.output_frame(), 1);
    assert_eq!(original.original_frame(), Some(0));
    assert_eq!(original.execution_frame(), Some(execution));
    assert_eq!(original.effective_frame(), None);
    assert_eq!(original.content_key(), key);
    assert_eq!(original.received_timestamp(), timestamp);
    assert_eq!(original.sequence(), sequence);
    assert!(original.belongs_to(&loaded.engine.input_runtime_ownership));
    assert!(loaded.consumer.is_empty());
    assert!(
        !loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert!(
        render_current(&mut loaded, (execution - 1) as usize)
            .iter()
            .all(|value| *value == 0.0)
    );
    assert_eq!(loaded.callback.transport.output_frame(), execution);
    assert_eq!(original.effective_frame(), None);
    let output = render_current(&mut loaded, 300);
    assert!(loaded.callback.scheduler.is_empty());
    assert_eq!(original.state(), "applied");
    assert_eq!(original.reason(), None);
    assert_eq!(original.original_frame(), Some(0));
    assert_eq!(original.execution_frame(), Some(execution));
    assert_eq!(original.effective_frame(), Some(execution));
    assert_eq!(original.sequence(), sequence);
    assert_eq!(original.content_key(), key);
    assert_eq!(original.received_timestamp(), timestamp);
    let voices: Vec<_> = loaded
        .callback
        .mixer
        .voices
        .iter()
        .filter(|voice| voice.is_playing_sample(0))
        .collect();
    assert_eq!(voices.len(), 1);
    let sample = voices[0].sample.as_ref().unwrap();
    assert!(sample.valid_residency(48_000, 2));
    assert_eq!(sample.frame_count(), loaded.mono.len());
    assert!(sample.same_window(&loaded.sample()));
    assert_eq!((sample.resident_start(), sample.resident_end()), (32, 64));
    assert_eq!(voices[0].source_playback.tempo_ratio(), 1.0);
    assert_eq!(voices[0].source_playback.rate_target(), 1.0);
    assert!(!voices[0].source_playback.is_mapped());
    assert_eq!(voices[0].source_playback.position().frame, 32 + 300 % 32);
    assert_eq!(voices[0].source_playback.position().fraction, 0.0);
    assert_eq!(
        voices[0]
            .source_admission
            .as_ref()
            .unwrap()
            .content_key_for_test(),
        Some(key)
    );
    for (relative_frame, stereo) in output.chunks_exact(2).enumerate().skip(128) {
        // Deferred physical birth begins at the selected source origin, without
        // consuming the unheard interval between original T and actual E.
        let expected = loaded.mono[32 + relative_frame % 32];
        for value in stereo {
            assert_eq!(
                value.to_bits(),
                expected.to_bits(),
                "frame={relative_frame}"
            );
        }
    }
    loaded.assert_feedback(1);
    assert_eq!(drain_current(&mut loaded), 0);
    assert!(loaded.consumer.is_empty());
    assert!(loaded.callback.scheduler.is_empty());
    assert!(loaded.callback.feedback_rx.is_empty());
    assert_eq!(
        loaded
            .callback
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.is_playing_sample(0))
            .count(),
        1
    );
    assert!(ready.is_current());
    assert_eq!(ready.publication_status(), "accepted");
}

#[test]
fn ready_admission_rejects_closed_cold_lane_before_intent_or_command_changes() {
    Python::initialize();
    for with_stems in [false, true] {
        let mut loaded = Loaded::new();
        if with_stems {
            accepted_stems(&mut loaded);
        }
        let original = loaded.sample();
        let binding = crate::audio_engine::input_runtime_binding::capture(&loaded.engine, 0)
            .unwrap()
            .unwrap();
        assert!(binding.current());
        loaded.engine.cold_jobs.close_admission();
        let error = prepare_window_with_producer(
            &loaded.engine,
            0,
            same_request(),
            loaded.producer.clone(),
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("cold source lane stopped"));
        assert!(binding.current());
        assert!(
            !loaded
                .engine
                .input_runtime_ownership
                .resident_control_pending(0)
        );
        assert!(loaded.consumer.is_empty());
        assert!(loaded.sample().same_window(&original));
        assert!(Arc::ptr_eq(&loaded.sample().samples, &original.samples));
    }
}

#[test]
fn ready_transaction_cannot_ack_a_control_cache_source_before_actual_cold_bank_adoption() {
    Python::initialize();
    let mut loaded = Loaded::new();
    // Keep this scenario's explicit feedback pressure using a fresh actual STOP, rather
    // than depending on an initial Cold setup event that the control tick already polled.
    assert!(enqueue_stop_with_producer(
        &loaded.engine.input_runtime_ownership,
        &mut loaded.producer.lock().unwrap(),
        Some(0)
    ));
    let stopped = ExpectedPadFeedback::capture(&loaded.engine, 0, 2);
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    stopped.assert_current(&loaded.engine, loaded.callback.feedback_rx.peek().unwrap());
    let (request, mut source_commands) = selected_load(
        &loaded.engine,
        &loaded.directory.path().join("control.wav"),
        loaded.directory.path(),
        48_000,
        ResidentLoadHint {
            start_s: 32.0 / 48_000.0,
            end_s: 64.0 / 48_000.0,
            key_lock: false,
        },
    );
    wait_until(Duration::from_secs(10), || !source_commands.is_empty());
    assert_eq!(
        loaded.engine.cold_loading[0].load(Ordering::Acquire),
        request
    );
    let binding = crate::audio_engine::input_runtime_binding::capture(&loaded.engine, 0)
        .unwrap()
        .unwrap();
    assert!(
        binding.current(),
        "control-source metadata alone is not a native readiness ACK"
    );
    assert!(
        !loaded
            .callback
            .mixer
            .admitted_source_binding_current(0, binding.binding)
    );
    let ticket = loaded.prepare(same_request());
    loaded.pending(&ticket);
    assert_eq!(ticket.publication_status(), "pending");
    assert!(
        !launch_with_producer(
            &loaded.engine,
            &ticket,
            false,
            loaded.engine.input_clock.capture_ns(),
            &loaded.producer
        )
        .unwrap()
    );
    // This selected queue is deliberately drained ahead of the actual load
    // queue. The real callback still owns its prior bank and must reject it.
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert_eq!(ticket.publication_status(), "rejected");
    assert!(
        !launch_with_producer(
            &loaded.engine,
            &ticket,
            false,
            loaded.engine.input_clock.capture_ns(),
            &loaded.producer
        )
        .unwrap()
    );
    // Replacement cannot claim its actual bank or publish success while its STOP cannot
    // be reported. Poll the real earlier STOP, then allow the queued Native adoption.
    assert_eq!(loaded.callback.drain(&mut source_commands), 0);
    assert_eq!(
        loaded.engine.cold_loading[0].load(Ordering::Acquire),
        request
    );
    assert!(!source_commands.is_empty());
    assert!(
        !loaded
            .callback
            .mixer
            .admitted_source_binding_current(0, binding.binding)
    );
    assert!(
        !loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    let previous_stop = loaded.callback.feedback_rx.pop().unwrap();
    stopped.assert_message(&previous_stop);
    assert_eq!(
        loaded.engine._resolve_pad_feedback(&previous_stop).unwrap(),
        None
    );
    assert!(loaded.callback.feedback_rx.is_empty());
    assert_eq!(loaded.callback.drain(&mut source_commands), 1);
    assert!(matches!(
        terminal(&loaded.engine, request),
        LoaderEvent::Success { .. }
    ));
    wait_until(Duration::from_secs(10), || {
        loaded.engine.cold_loading[0].load(Ordering::Acquire) == 0
    });
    loaded.assert_feedback(2);
    let adopted_binding = crate::audio_engine::input_runtime_binding::capture(&loaded.engine, 0)
        .unwrap()
        .unwrap();
    assert!(adopted_binding.current());
    assert!(
        loaded
            .callback
            .mixer
            .admitted_source_binding_current(0, adopted_binding.binding)
    );
    let fresh = loaded.prepare(same_request());
    loaded.adopt(&fresh);
    launch_and_render(&mut loaded, &fresh, None);
}

#[test]
fn ready_stem_publication_pending_retires_old_ticket_and_fresh_start_recovers_after_ack() {
    Python::initialize();
    // Both an unclaimed queued readiness command and a real ACK awaiting control
    // reconciliation must retire before a replacement publication is visible.
    for old_acknowledged in [false, true] {
        let mut loaded = Loaded::new();
        let original = accepted_stems(&mut loaded);
        assert!(
            loaded
                .callback
                .mixer
                .set_stem_mix_mode(0, StemMixMode::AllStems, 17)
        );
        assert!(loaded.callback.mixer.set_stem_enabled_mask(0, 1, 17));
        let _blocked = ColdLaneGate::occupy(&loaded.engine);
        let counts = loaded.engine.cold_jobs.counts_for_test();
        let old = loaded.prepare(same_request());
        loaded.pending(&old);
        if old_acknowledged {
            assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
            assert_eq!(old.publication_status(), "accepted");
            // Deliberately leave the old transaction in control-side ownership.
        }
        assert!(old.is_current());
        let (source_ticket, replacement) = prepared_stems(&loaded, 18, 0.0625);
        let (publication_producer, mut publication_consumer) = rtrb::RingBuffer::new(1);
        let publication_producer = Arc::new(Mutex::new(publication_producer));
        enqueue_stems(&loaded, &source_ticket, &replacement, &publication_producer).unwrap();
        assert_eq!(source_ticket.publication_status(), "pending");
        assert!(
            !old.is_current(),
            "pending complete-set admission must fence coalescing"
        );
        assert_eq!(
            old.publication_status(),
            if old_acknowledged {
                "accepted"
            } else {
                "cancelled"
            }
        );
        assert!(
            !launch_with_producer(
                &loaded.engine,
                &old,
                false,
                loaded.engine.input_clock.capture_ns(),
                &loaded.producer,
            )
            .unwrap()
        );
        let error = prepare_window_with_producer(
            &loaded.engine,
            0,
            same_request(),
            loaded.producer.clone(),
        )
        .err()
        .unwrap();
        assert!(
            error
                .to_string()
                .contains("resident stem publication is pending")
        );
        if !old_acknowledged {
            // A cancelled unclaimed command cannot revive after the new ACK.
            assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
        }
        assert_eq!(loaded.callback.drain(&mut publication_consumer), 1);
        assert_eq!(source_ticket.publication_status(), "accepted");
        reconcile(&loaded.engine).unwrap();
        assert!(
            !loaded
                .engine
                .input_runtime_ownership
                .resident_control_pending(0),
            "superseded or accepted control must release its exact pending epoch"
        );
        let fresh = loaded.prepare(same_request());
        let Ok(ControlMessage::RelocateResident(transaction)) = loaded.consumer.peek() else {
            panic!("expected readiness transaction after complete-set ACK");
        };
        let fresh_set = transaction.stems.as_ref().unwrap();
        assert!(Arc::ptr_eq(
            &fresh_set.complete_set_identity,
            &replacement.complete_set_identity
        ));
        assert!(!Arc::ptr_eq(
            &fresh_set.complete_set_identity,
            &original.complete_set_identity
        ));
        for (actual, expected) in fresh_set.stems.iter().zip(&replacement.stems) {
            assert!(Arc::ptr_eq(&actual.samples, &expected.samples));
        }
        assert_eq!(loaded.engine.cold_jobs.counts_for_test(), counts);
        loaded.adopt(&fresh);
        launch_and_render(&mut loaded, &fresh, Some(0.0625));
        assert_eq!(loaded.engine.cold_jobs.counts_for_test(), counts);
    }
}

#[test]
fn ready_failed_stem_enqueue_preserves_old_epoch_and_accepted_owner() {
    Python::initialize();
    let mut loaded = Loaded::new();
    let original = accepted_stems(&mut loaded);
    let _blocked = ColdLaneGate::occupy(&loaded.engine);
    let counts = loaded.engine.cold_jobs.counts_for_test();
    let old = loaded.prepare(same_request());
    loaded.adopt(&old);
    let (source_ticket, replacement) = prepared_stems(&loaded, 18, 0.0625);
    let (publication_producer, mut publication_consumer) = rtrb::RingBuffer::new(1);
    let publication_producer = Arc::new(Mutex::new(publication_producer));
    publication_producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    let error = enqueue_stems(&loaded, &source_ticket, &replacement, &publication_producer)
        .err()
        .unwrap();
    assert!(error.to_string().contains("buffer may be full"));
    assert_eq!(source_ticket.publication_status(), "captured");
    assert!(
        old.is_current(),
        "full publication queue must preserve old intent epoch"
    );
    publication_consumer.pop().unwrap();
    let error = enqueue_current_prepared_stems_with_owner(
        &loaded.engine,
        &publication_producer,
        &source_ticket,
        "foreign-version",
        replacement.clone(),
        (PathBuf::from("foreign"), PathBuf::from("foreign")),
    )
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .contains("stale or foreign prepared source ticket")
    );
    assert!(old.is_current());
    assert!(publication_consumer.is_empty());
    assert_eq!(source_ticket.publication_status(), "captured");
    // Failed admission did not register a pending descriptor or replace accepted
    // readers: a same-window request still succeeds with the old complete token.
    let check = loaded.prepare(same_request());
    let Ok(ControlMessage::RelocateResident(transaction)) = loaded.consumer.peek() else {
        panic!("expected prior accepted readers after failed publication");
    };
    assert!(Arc::ptr_eq(
        &transaction.stems.as_ref().unwrap().complete_set_identity,
        &original.complete_set_identity,
    ));
    loaded.adopt(&check);
    assert_eq!(loaded.engine.cold_jobs.counts_for_test(), counts);
    enqueue_stems(&loaded, &source_ticket, &replacement, &publication_producer).unwrap();
    assert!(!check.is_current());
    assert_eq!(loaded.callback.drain(&mut publication_consumer), 1);
    assert_eq!(source_ticket.publication_status(), "accepted");
    reconcile(&loaded.engine).unwrap();
    let fresh = loaded.prepare(same_request());
    let Ok(ControlMessage::RelocateResident(transaction)) = loaded.consumer.peek() else {
        panic!("expected newly accepted readers");
    };
    assert!(Arc::ptr_eq(
        &transaction.stems.as_ref().unwrap().complete_set_identity,
        &replacement.complete_set_identity,
    ));
    loaded.adopt(&fresh);
    assert!(
        loaded
            .callback
            .mixer
            .set_stem_mix_mode(0, StemMixMode::AllStems, 17)
    );
    assert!(loaded.callback.mixer.set_stem_enabled_mask(0, 1, 17));
    launch_and_render(&mut loaded, &fresh, Some(0.0625));
    assert_eq!(loaded.engine.cold_jobs.counts_for_test(), counts);
}

#[test]
fn ready_old_ack_reconciliation_cannot_overwrite_newly_accepted_complete_stem_set() {
    Python::initialize();
    let mut loaded = Loaded::new();
    accepted_stems(&mut loaded);
    let _blocked = ColdLaneGate::occupy(&loaded.engine);
    let counts = loaded.engine.cold_jobs.counts_for_test();
    let old = loaded.prepare(same_request());
    loaded.pending(&old);
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert_eq!(old.publication_status(), "accepted");
    // Its real adoption tail is still in resident_stem_cache. Admit and ACK the
    // replacement before the control owner reconciles either transaction.
    let (source_ticket, replacement) = prepared_stems(&loaded, 18, 0.0625);
    enqueue_stems(&loaded, &source_ticket, &replacement, &loaded.producer).unwrap();
    assert!(!old.is_current());
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert_eq!(source_ticket.publication_status(), "accepted");
    reconcile(&loaded.engine).unwrap();
    assert!(
        !loaded
            .engine
            .input_runtime_ownership
            .resident_control_pending(0)
    );
    let fresh = loaded.prepare(same_request());
    let Ok(ControlMessage::RelocateResident(transaction)) = loaded.consumer.peek() else {
        panic!("expected newly accepted complete-set readers");
    };
    assert!(Arc::ptr_eq(
        &transaction.stems.as_ref().unwrap().complete_set_identity,
        &replacement.complete_set_identity,
    ));
    loaded.adopt(&fresh);
    assert!(
        loaded
            .callback
            .mixer
            .set_stem_mix_mode(0, StemMixMode::AllStems, 17)
    );
    assert!(loaded.callback.mixer.set_stem_enabled_mask(0, 1, 17));
    launch_and_render(&mut loaded, &fresh, Some(0.0625));
    assert_eq!(loaded.engine.cold_jobs.counts_for_test(), counts);
}
