//! Productive controller transaction admission, callback, scheduler and pinned-voice ownership.

use super::*;
use crate::audio_engine::audio_stream::{
    drain_control_messages, execute_scheduled_command, process_control_message,
};
use crate::audio_engine::constants::MAX_VOICES;
use crate::audio_engine::global_playback_batch::{self, GlobalPlaybackBatchTicket};
use crate::audio_engine::input_runtime_binding;
use crate::audio_engine::scheduler::{FixedCapacityScheduler, ScheduledCommand};
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};

struct Fixture {
    engine: AudioEngine,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    mixer: RtMixer,
    transport: TransportTimeline,
    scheduler: FixedCapacityScheduler<4>,
    mode: TriggerQuantization,
    messages: Vec<AudioMessage>,
}

impl Fixture {
    fn new() -> Self {
        let engine = test_engine();
        let mut mixer = acknowledged_mixer(&engine);
        mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
        let (producer, consumer) = queue(8);
        let mut fixture = Self {
            engine,
            producer,
            consumer,
            mixer,
            transport: TransportTimeline::new(RATE),
            scheduler: FixedCapacityScheduler::new(),
            mode: TriggerQuantization::Immediate,
            messages: Vec::new(),
        };
        fixture.accept("first");
        fixture.load(1);
        fixture
    }

    fn load(&mut self, id: usize) {
        let sample = source();
        self.engine.sample_cache.lock().unwrap()[id] = Some(sample.clone());
        self.engine.loaded_source_generations.lock().unwrap()[id] = (1, RATE);
        self.engine.loaded_source_digests.lock().unwrap()[id] = Some("a".repeat(64));
        self.engine
            .input_runtime_ownership
            .publish_source(id, &sample, RATE, 1);
        self.mixer.load_sample(id, sample);
    }

    fn accept(&mut self, provenance: &str) {
        let mut decision = decision();
        decision.provenance.push_str(provenance);
        publish(
            &self.engine,
            &self.producer,
            &synthetic_ticket(&self.engine),
            &hypotheses(),
            origin(),
            decision,
        )
        .unwrap();
        let message = self.consumer.pop().unwrap();
        self.apply(message, &mut ImmediateAudioBufferRetirement);
    }

    fn apply(
        &mut self,
        message: ControlMessage,
        retirement: &mut impl crate::audio_engine::buffer_retirement::AudioBufferRetirement,
    ) {
        process_control_message(
            message,
            &mut self.scheduler,
            self.transport.output_frame(),
            &mut self.mode,
            &mut self.transport,
            &mut self.mixer,
            &mut self.messages,
            retirement,
        );
    }

    fn enqueue(&self, ids: &[usize], start: bool) -> GlobalPlaybackBatchTicket {
        let bindings: Vec<_> = ids
            .iter()
            .map(|id| {
                input_runtime_binding::capture(&self.engine, *id)
                    .unwrap()
                    .unwrap()
            })
            .collect();
        let entries = bindings
            .iter()
            .map(|binding| (binding, 0.125, Some(3.0)))
            .collect();
        global_playback_batch::enqueue(&self.engine, &self.producer, entries, start, Some(42))
            .unwrap()
    }

    fn drain(&mut self) {
        let message = self.consumer.pop().unwrap();
        self.apply(message, &mut ImmediateAudioBufferRetirement);
    }

    fn due(&mut self) {
        let event = self.scheduler.pop_due_through(0, u64::MAX).unwrap();
        execute_scheduled_command(
            &mut self.mixer,
            &mut self.transport,
            event.execution_frame,
            event.command,
            &mut self.messages,
            &mut ImmediateAudioBufferRetirement,
        );
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames];
        self.mixer.render(&mut output, &mut [0.0; NUM_SAMPLES]);
        output
    }

    fn loop_start(&self, id: usize) -> usize {
        self.mixer
            .phase_aligned_initial_sample_frame(id, source().samples.len(), 0.0)
    }
}

#[test]
fn global_batch_current_exact_accepted_and_nonaccepted_start_stop_execute_transactionally() {
    for intent in [
        TimingIntent::Manual,
        TimingIntent::Tap,
        TimingIntent::Legacy,
    ] {
        let mut fixture = Fixture::new();
        fixture.engine.timing_intents.lock().unwrap()[1] = intent;
        fixture
            .engine
            .input_runtime_ownership
            .set_timing_intent(1, intent);
        let ticket = fixture.enqueue(&[0, 1], true);
        assert_eq!(ticket.publication_status(), "pending");
        fixture.drain();
        assert_eq!(ticket.publication_status(), "accepted");
        let automatic = fixture
            .mixer
            .voices
            .iter()
            .find(|v| v.active && v.sample_id == 0)
            .unwrap();
        assert_eq!(automatic.frame_pos, 1_000);
        let projection = automatic.source_timing.accepted.unwrap();
        assert_eq!(projection.period_seconds.to_bits(), PERIOD.to_bits());
        assert_eq!(
            projection.origin_seconds.to_bits(),
            origin().seconds.to_bits()
        );
        assert!(
            fixture
                .mixer
                .voices
                .iter()
                .find(|v| v.active && v.sample_id == 1)
                .unwrap()
                .source_timing
                .accepted
                .is_none()
        );
        fixture.mixer.pause_sample(1);
        fixture.messages.clear();
        let stop = fixture.enqueue(&[0, 1], false);
        fixture.drain();
        assert_eq!(stop.publication_status(), "accepted");
        assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
        assert_eq!(fixture.messages.len(), 2);
    }
}

#[test]
fn global_batch_control_admission_rejects_full_foreign_duplicate_stale_unavailable_and_loading() {
    let fixture = Fixture::new();
    let binding = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let foreign = Fixture::new();
    assert!(
        global_playback_batch::enqueue(
            &foreign.engine,
            &foreign.producer,
            vec![(&binding, 0.0, None)],
            true,
            None
        )
        .is_err()
    );
    assert!(
        global_playback_batch::enqueue(
            &fixture.engine,
            &fixture.producer,
            vec![(&binding, 0.0, None), (&binding, 0.0, None)],
            true,
            None
        )
        .is_err()
    );
    assert!(
        global_playback_batch::enqueue(
            &fixture.engine,
            &fixture.producer,
            vec![(&binding, f64::NAN, None)],
            true,
            None
        )
        .is_err()
    );
    for _ in 0..8 {
        fixture
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
    }
    assert!(
        global_playback_batch::enqueue(
            &fixture.engine,
            &fixture.producer,
            vec![(&binding, 0.0, None)],
            true,
            None
        )
        .is_err()
    );
    assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
    fixture.engine.input_runtime_ownership.revoke(0, 2);
    assert!(
        global_playback_batch::enqueue(
            &fixture.engine,
            &fixture.producer,
            vec![(&binding, 0.0, None)],
            true,
            None
        )
        .is_err()
    );
    let unavailable = input_runtime_binding::capture(&fixture.engine, 1)
        .unwrap()
        .unwrap();
    fixture.engine.timing_intents.lock().unwrap()[1] = TimingIntent::Automatic;
    fixture
        .engine
        .input_runtime_ownership
        .set_timing_intent(1, TimingIntent::Automatic);
    fixture.engine.input_runtime_ownership.revoke(1, 2);
    assert!(
        global_playback_batch::enqueue(
            &fixture.engine,
            &fixture.producer,
            vec![(&unavailable, 0.0, None)],
            true,
            None
        )
        .is_err()
    );
    let unavailable = input_runtime_binding::capture(&fixture.engine, 1)
        .unwrap()
        .unwrap();
    assert!(!unavailable.available());
    assert!(
        global_playback_batch::enqueue(
            &fixture.engine,
            &fixture.producer,
            vec![(&unavailable, 0.0, None)],
            false,
            None
        )
        .is_err()
    );
    fixture.engine.input_runtime_ownership.revoke_source(0);
    let loading = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    assert!(!loading.current());
    assert!(
        global_playback_batch::enqueue(
            &fixture.engine,
            &fixture.producer,
            vec![(&loading, 0.0, None)],
            true,
            None
        )
        .is_err()
    );
}

#[test]
fn global_batch_stale_callback_keeps_real_rendered_history_and_both_prior_loops() {
    let mut fixture = Fixture::new();
    let mut reference = Fixture::new();
    for state in [&mut fixture, &mut reference] {
        state.mixer.set_pad_loop_region(0, 0.25, Some(4.0));
        state.mixer.set_pad_loop_region(1, 0.25, Some(4.0));
        state.mixer.set_pad_eq(0, -3.0, 2.0, -2.0);
        assert!(state.mixer.play_sample(0, 1.0));
        state.render(57);
    }
    let ticket = fixture.enqueue(&[0, 1], true);
    fixture.engine.input_runtime_ownership.revoke(1, 2);
    fixture.drain();
    assert_eq!(ticket.publication_status(), "rejected");
    assert!(fixture.messages.is_empty());
    assert_eq!(fixture.loop_start(0), reference.loop_start(0));
    assert_eq!(fixture.loop_start(1), reference.loop_start(1));
    assert_eq!(fixture.render(513), reference.render(513));
}

#[test]
fn global_batch_quantized_execution_rechecks_complete_new_accepted_revision() {
    let mut fixture = Fixture::new();
    fixture.transport.set_master_period(PERIOD);
    fixture.transport.advance_by_rendered_frames(1);
    fixture.mode = TriggerQuantization::Grid { step_64ths: 4 };
    fixture.mixer.set_pad_loop_region(0, 0.25, Some(4.0));
    fixture.mixer.set_pad_loop_region(1, 0.25, Some(4.0));
    let before = [fixture.loop_start(0), fixture.loop_start(1)];
    let ticket = fixture.enqueue(&[0, 1], true);
    fixture.drain();
    assert_eq!(ticket.publication_status(), "pending");
    fixture.accept("new full accepted provenance but same exact period/origin");
    fixture.due();
    assert_eq!(ticket.publication_status(), "rejected");
    assert_eq!([fixture.loop_start(0), fixture.loop_start(1)], before);
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
}

#[test]
fn global_batch_pending_then_rejected_publication_retains_previous_effective_authority() {
    let mut fixture = Fixture::new();
    let pending = synthetic_ticket(&fixture.engine);
    publish(
        &fixture.engine,
        &fixture.producer,
        &pending,
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
    let pending_message = fixture.consumer.pop().unwrap();
    assert_eq!(pending.publication_status().unwrap(), "pending");
    let ticket = fixture.enqueue(&[0, 1], true);
    fixture.drain();
    assert_eq!(ticket.publication_status(), "accepted");
    fixture.engine.prepared_source_epochs[0].fetch_add(1, Ordering::AcqRel);
    fixture.apply(pending_message, &mut ImmediateAudioBufferRetirement);
    assert_eq!(pending.publication_status().unwrap(), "rejected");
    let stop = fixture.enqueue(&[0, 1], false);
    fixture.drain();
    assert_eq!(stop.publication_status(), "accepted");
}

#[test]
fn global_batch_full_scheduler_and_voice_capacity_preserve_prior_live_state() {
    let mut fixture = Fixture::new();
    assert!(fixture.mixer.play_sample(0, 1.0));
    fixture.render(33);
    let position = fixture
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap()
        .frame_pos;
    for frame in 1..=4 {
        fixture
            .scheduler
            .schedule(frame, ScheduledCommand::StopSample { id: 200 })
            .unwrap();
    }
    let stop = fixture.enqueue(&[0], false);
    fixture.drain();
    assert_eq!(stop.publication_status(), "rejected");
    assert_eq!(fixture.scheduler.len(), 4);
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap()
            .frame_pos,
        position
    );
    while fixture.scheduler.pop_due_through(0, u64::MAX).is_some() {}
    for id in 2..=32 {
        fixture.load(id);
        assert!(fixture.mixer.play_sample(id, 1.0));
    }
    let ticket = fixture.enqueue(&[0, 1], true);
    fixture.drain();
    assert_eq!(ticket.publication_status(), "rejected");
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == 0)
            .unwrap()
            .frame_pos,
        position
    );
    assert!(
        !fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 1)
    );
}

#[test]
fn global_batch_stop_missing_or_replaced_active_pin_is_atomic_and_fail_closed() {
    let mut fixture = Fixture::new();
    let start = fixture.enqueue(&[0, 1], true);
    fixture.drain();
    assert_eq!(start.publication_status(), "accepted");
    fixture.mixer.pause_sample(0);
    fixture.render(17);
    let missing = fixture.enqueue(&[0], false);
    fixture.drain();
    assert_eq!(missing.publication_status(), "rejected");
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        2
    );
    fixture.engine.input_runtime_ownership.revoke(1, 2);
    fixture.load(1); // Current bank now differs from the still-pinned old active voice.
    let replaced = fixture.enqueue(&[0, 1], false);
    fixture.drain();
    assert_eq!(replaced.publication_status(), "rejected");
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        2
    );
    assert!(
        fixture
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == 0)
            .unwrap()
            .paused
    );
}

#[test]
fn global_batch_queue_teardown_rejects_without_effects() {
    let mut fixture = Fixture::new();
    let ticket = fixture.enqueue(&[0, 1], true);
    assert_eq!(ticket.publication_status(), "pending");
    drop(fixture.consumer.pop().unwrap());
    assert_eq!(ticket.publication_status(), "rejected");
    assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
}

#[test]
fn global_batch_quantized_unavailable_target_rejects_and_scheduler_teardown_settles_ticket() {
    let mut fixture = Fixture::new();
    fixture.mode = TriggerQuantization::Grid { step_64ths: 0 };
    fixture.mixer.set_pad_loop_region(0, 0.25, Some(4.0));
    let before = fixture.loop_start(0);
    let unavailable = fixture.enqueue(&[0, 1], true);
    fixture.drain();
    assert_eq!(unavailable.publication_status(), "rejected");
    assert_eq!(fixture.loop_start(0), before);
    assert!(fixture.scheduler.is_empty());
    assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
    fixture.mode = TriggerQuantization::Grid { step_64ths: 4 };
    fixture.transport.advance_by_rendered_frames(usize::MAX);
    let overflow = fixture.enqueue(&[0, 1], true);
    fixture.drain();
    assert_eq!(overflow.publication_status(), "rejected");
    assert_eq!(fixture.loop_start(0), before);
    fixture.transport = TransportTimeline::new(RATE);
    fixture.transport.set_master_period(PERIOD);
    fixture.transport.advance_by_rendered_frames(1);
    let pending = fixture.enqueue(&[0, 1], true);
    fixture.drain();
    assert_eq!(pending.publication_status(), "pending");
    drop(std::mem::replace(
        &mut fixture.scheduler,
        FixedCapacityScheduler::new(),
    ));
    assert_eq!(pending.publication_status(), "rejected");
}

#[test]
fn global_batch_full_projection_checks_exact_signed_origin_and_period_bits() {
    let mut fixture = Fixture::new();
    publish(
        &fixture.engine,
        &fixture.producer,
        &synthetic_ticket(&fixture.engine),
        &hypotheses(),
        IndependentTimingOrigin {
            seconds: 0.0,
            provenance: "explicit positive source-zero fixture origin".into(),
        },
        decision(),
    )
    .unwrap();
    fixture.drain();
    let binding = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let mut changed = binding.binding;
    changed.accepted.as_mut().unwrap().origin_seconds = -0.0;
    assert!(!fixture.mixer.source_binding_current(0, changed));
    changed = binding.binding;
    let projection = changed.accepted.as_mut().unwrap();
    projection.period_seconds = f64::from_bits(projection.period_seconds.to_bits() + 1);
    assert!(!fixture.mixer.source_binding_current(0, changed));
    assert!(fixture.mixer.source_binding_current(0, binding.binding));
}

struct RetainedBatchRetirement {
    slots: usize,
    batches: Vec<Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>>,
    samples: Vec<SampleBuffer>,
    revoke: Option<(Arc<input_runtime_binding::InputRuntimeOwnership>, usize)>,
    calls: usize,
}

impl crate::audio_engine::buffer_retirement::AudioBufferRetirement for RetainedBatchRetirement {
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }
    fn retire_sample(&mut self, sample: SampleBuffer) {
        self.samples.push(sample);
    }
    fn retire_prepared_stems(&mut self, _: crate::messages::PreparedStemSet) {
        unreachable!()
    }
    fn retire_constant_timing(&mut self, _: PreparedConstantTiming) {
        unreachable!()
    }
    fn retire_global_playback_batch(
        &mut self,
        batch: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        self.batches.push(batch);
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.calls += 1;
        if self.calls == 2
            && let Some((ownership, id)) = &self.revoke
        {
            ownership.revoke(*id, ownership.next_authority(*id).unwrap());
        }
        self.slots
    }
}

#[test]
fn global_batch_execution_failures_retire_payload_and_preserve_history_before_commit() {
    for failure in [
        "success",
        "stale",
        "scheduler",
        "retirement",
        "prepare-race",
    ] {
        let mut fixture = Fixture::new();
        fixture.mixer.set_pad_loop_region(0, 0.25, Some(4.0));
        assert!(fixture.mixer.play_sample(0, 1.0));
        fixture.render(25);
        if failure == "prepare-race" {
            assert!(fixture.mixer.play_sample(1, 1.0));
            fixture.engine.input_runtime_ownership.revoke(1, 2);
            fixture.load(1); // The later preparation probes retirement for the old active pin.
        }
        let position = fixture
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap()
            .frame_pos;
        let before = fixture.loop_start(0);
        let ticket = fixture.enqueue(&[0, 1], true);
        let message = fixture.consumer.pop().unwrap();
        let weak = match &message {
            ControlMessage::GlobalPlaybackBatch(batch) => {
                assert_eq!(batch.received_at_ns, Some(42));
                Arc::downgrade(batch)
            }
            _ => unreachable!(),
        };
        let mut retirement = RetainedBatchRetirement {
            slots: MAX_VOICES + 1,
            batches: Vec::new(),
            samples: Vec::new(),
            revoke: None,
            calls: 0,
        };
        if failure == "stale" {
            fixture.engine.input_runtime_ownership.revoke(1, 2);
        }
        if failure == "scheduler" {
            for frame in 1..=4 {
                fixture
                    .scheduler
                    .schedule(frame, ScheduledCommand::StopSample { id: 200 })
                    .unwrap();
            }
        }
        if failure == "retirement" {
            retirement.slots = 1;
        }
        if failure == "prepare-race" {
            retirement.revoke = Some((fixture.engine.input_runtime_ownership.clone(), 1));
        }
        fixture.apply(message, &mut retirement);
        assert_eq!(
            ticket.publication_status(),
            if failure == "success" {
                "accepted"
            } else {
                "rejected"
            }
        );
        if failure != "success" {
            assert_eq!(fixture.loop_start(0), before);
            assert_eq!(
                fixture
                    .mixer
                    .voices
                    .iter()
                    .find(|voice| voice.active && voice.sample_id == 0)
                    .unwrap()
                    .frame_pos,
                position
            );
            assert_eq!(
                fixture
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.active && voice.sample_id == 1),
                failure == "prepare-race"
            );
        }
        assert_eq!(retirement.batches.len(), 1);
        assert!(weak.upgrade().is_some());
        retirement.batches.clear(); // Simulated non-RT consumer performs the final payload drop.
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn global_batch_bounded_feedback_capacity_admits_complete_start_or_preserves_real_history() {
    for available in 0..=2 {
        let mut fixture = Fixture::new();
        let mut reference = Fixture::new();
        for state in [&mut fixture, &mut reference] {
            state.mixer.set_pad_loop_region(0, 0.25, Some(4.0));
            state.mixer.set_pad_loop_region(1, 0.25, Some(4.0));
            state.mixer.set_pad_eq(0, -3.0, 2.0, -2.0);
            assert!(state.mixer.play_sample(0, 1.0));
            assert!(state.mixer.play_sample(1, 1.0));
            state.mixer.pause_sample(1);
            state.render(61);
        }
        let before = [fixture.loop_start(0), fixture.loop_start(1)];
        let ticket = fixture.enqueue(&[0, 1], true);
        let message = fixture.consumer.pop().unwrap();
        let (mut feedback, mut receiver) = rtrb::RingBuffer::<AudioMessage>::new(3);
        for _ in 0..3 - available {
            feedback.push(AudioMessage::Pong()).unwrap();
        }
        process_control_message(
            message,
            &mut fixture.scheduler,
            0,
            &mut fixture.mode,
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut feedback,
            &mut ImmediateAudioBufferRetirement,
        );
        assert_eq!(
            ticket.publication_status(),
            if available < 2 {
                "rejected"
            } else {
                "accepted"
            }
        );
        for _ in 0..3 - available {
            assert!(matches!(receiver.pop().unwrap(), AudioMessage::Pong()));
        }
        if available < 2 {
            assert!(receiver.pop().is_err());
            assert_eq!([fixture.loop_start(0), fixture.loop_start(1)], before);
            assert!(
                fixture
                    .mixer
                    .voices
                    .iter()
                    .find(|voice| voice.active && voice.sample_id == 1)
                    .unwrap()
                    .paused
            );
            let actual = fixture.render(4_097);
            assert!(actual.iter().any(|value| *value != 0.0));
            assert_eq!(actual, reference.render(4_097));
        } else {
            assert!(matches!(
                receiver.pop().unwrap(),
                AudioMessage::SampleStarted { id: 0 }
            ));
            assert!(matches!(
                receiver.pop().unwrap(),
                AudioMessage::SampleStarted { id: 1 }
            ));
            assert!(receiver.pop().is_err());
            assert!(
                fixture
                    .mixer
                    .voices
                    .iter()
                    .filter(|voice| voice.active)
                    .all(|voice| !voice.paused && voice.frame_pos == 1_000)
            );
        }
    }
}

#[test]
fn global_batch_bounded_feedback_capacity_admits_complete_stop_or_preserves_real_history() {
    for available in 0..=2 {
        let mut fixture = Fixture::new();
        let mut reference = Fixture::new();
        for state in [&mut fixture, &mut reference] {
            state.mixer.set_pad_loop_region(0, 0.25, Some(4.0));
            state.mixer.set_pad_loop_region(1, 0.25, Some(4.0));
            state.mixer.set_pad_eq(0, -3.0, 2.0, -2.0);
            assert!(state.mixer.play_sample(0, 1.0));
            assert!(state.mixer.play_sample(1, 1.0));
            state.mixer.pause_sample(1);
            state.render(61);
        }
        let ticket = fixture.enqueue(&[0, 1], false);
        let message = fixture.consumer.pop().unwrap();
        let (mut feedback, mut receiver) = rtrb::RingBuffer::<AudioMessage>::new(3);
        for _ in 0..3 - available {
            feedback.push(AudioMessage::Pong()).unwrap();
        }
        process_control_message(
            message,
            &mut fixture.scheduler,
            0,
            &mut fixture.mode,
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut feedback,
            &mut ImmediateAudioBufferRetirement,
        );
        assert_eq!(
            ticket.publication_status(),
            if available < 2 {
                "rejected"
            } else {
                "accepted"
            }
        );
        for _ in 0..3 - available {
            assert!(matches!(receiver.pop().unwrap(), AudioMessage::Pong()));
        }
        if available < 2 {
            assert!(receiver.pop().is_err());
            assert_eq!(
                fixture
                    .mixer
                    .voices
                    .iter()
                    .filter(|voice| voice.active)
                    .count(),
                2
            );
            assert!(
                fixture
                    .mixer
                    .voices
                    .iter()
                    .find(|voice| voice.active && voice.sample_id == 1)
                    .unwrap()
                    .paused
            );
            let actual = fixture.render(4_097);
            assert!(actual.iter().any(|value| *value != 0.0));
            assert_eq!(actual, reference.render(4_097));
        } else {
            assert!(matches!(
                receiver.pop().unwrap(),
                AudioMessage::SampleStopped { id: 0 }
            ));
            assert!(matches!(
                receiver.pop().unwrap(),
                AudioMessage::SampleStopped { id: 1 }
            ));
            assert!(receiver.pop().is_err());
            assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
        }
    }
}

#[test]
fn global_batch_old_started_then_reliable_unload_feedback_cannot_reactivate_unloaded_pad() {
    let mut fixture = Fixture::new();
    let ticket = fixture.enqueue(&[0], true);
    let message = fixture.consumer.pop().unwrap();
    let (mut feedback, mut receiver) = rtrb::RingBuffer::<AudioMessage>::new(1);
    process_control_message(
        message,
        &mut fixture.scheduler,
        0,
        &mut fixture.mode,
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(
        fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    // Python clears its local active projection on unload before it receives old STARTED.
    let mut projected_active = std::collections::HashSet::<usize>::new();
    fixture.engine.input_runtime_ownership.revoke_source(0);
    fixture.engine.input_runtime_ownership.revoke(0, 2);
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::UnloadSample { id: 0 })
        .unwrap();
    let mut retirement = RetainedBatchRetirement {
        slots: MAX_VOICES + 2,
        batches: Vec::new(),
        samples: Vec::new(),
        revoke: None,
        calls: 0,
    };
    assert_eq!(
        drain_control_messages(
            &mut fixture.consumer,
            &mut fixture.scheduler,
            0,
            &mut fixture.mode,
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut feedback,
            &mut retirement
        ),
        0
    );
    assert!(retirement.samples.is_empty());
    assert!(matches!(
        fixture.consumer.peek().unwrap(),
        ControlMessage::UnloadSample { id: 0 }
    ));
    match receiver.pop().unwrap() {
        AudioMessage::SampleStarted { id } => {
            projected_active.insert(id);
        }
        _ => unreachable!(),
    }
    assert!(projected_active.contains(&0));
    assert_eq!(
        drain_control_messages(
            &mut fixture.consumer,
            &mut fixture.scheduler,
            0,
            &mut fixture.mode,
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut feedback,
            &mut retirement
        ),
        1
    );
    match receiver.pop().unwrap() {
        AudioMessage::SampleStopped { id } => {
            projected_active.remove(&id);
        }
        _ => unreachable!(),
    }
    assert!(projected_active.is_empty());
    assert!(receiver.pop().is_err());
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    assert_eq!(retirement.samples.len(), 2); // Actual pinned voice plus callback bank retire off RT.
    assert!(!fixture.mixer.can_play_sample(0, 1.0));
}

#[test]
fn global_batch_unload_full_feedback_defers_actual_pin_and_paused_history_until_space() {
    let mut fixture = Fixture::new();
    assert!(fixture.mixer.play_sample(1, 1.0));
    fixture.render(117);
    fixture.mixer.pause_sample(1);
    let voice = fixture
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    let position = voice.frame_pos;
    let pin = Arc::downgrade(&voice.sample.as_ref().unwrap().samples);
    fixture.engine.input_runtime_ownership.revoke_source(1);
    fixture.engine.input_runtime_ownership.revoke(1, 2);
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::UnloadSample { id: 1 })
        .unwrap();
    let (mut feedback, mut receiver) = rtrb::RingBuffer::<AudioMessage>::new(1);
    feedback.push(AudioMessage::Pong()).unwrap();
    let mut retirement = RetainedBatchRetirement {
        slots: MAX_VOICES + 2,
        batches: Vec::new(),
        samples: Vec::new(),
        revoke: None,
        calls: 0,
    };
    assert_eq!(
        drain_control_messages(
            &mut fixture.consumer,
            &mut fixture.scheduler,
            0,
            &mut fixture.mode,
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut feedback,
            &mut retirement
        ),
        0
    );
    assert!(retirement.samples.is_empty());
    let voice = fixture
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    assert_eq!(voice.frame_pos, position);
    assert!(voice.paused);
    assert!(pin.upgrade().is_some());
    assert!(matches!(receiver.pop().unwrap(), AudioMessage::Pong()));
    assert_eq!(
        drain_control_messages(
            &mut fixture.consumer,
            &mut fixture.scheduler,
            0,
            &mut fixture.mode,
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut feedback,
            &mut retirement
        ),
        1
    );
    assert!(matches!(
        receiver.pop().unwrap(),
        AudioMessage::SampleStopped { id: 1 }
    ));
    assert!(receiver.pop().is_err());
    assert_eq!(retirement.samples.len(), 2);
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
}
