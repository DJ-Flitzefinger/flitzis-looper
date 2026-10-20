//! Current native acceptance coupled to productive MIDI queue/callback/scheduler.

use super::*;
use crate::audio_engine::audio_stream::{
    AudioMessageSink, execute_scheduled_command, process_control_message,
};
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::input_runtime_binding::{self, InputRuntimePadBinding};
use crate::audio_engine::pad_start::CapturedPadStart;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::timing::InputClock;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};
use std::sync::atomic::Ordering;

#[test]
fn resident_context_metadata_and_window_fences_distinguish_every_context() {
    use crate::messages::ResidentContext;

    let engine = AudioEngine::new().unwrap();
    let full = SampleBuffer {
        channels: 1,
        samples: Arc::from(vec![0.25; 32]),
        residency: None,
    }
    .with_complete_source(48_000);
    let contexts = [
        (ResidentContext::FiniteLoop, "finite-loop"),
        (ResidentContext::FullTrack, "full-track"),
        (ResidentContext::KeyLockFullTrack, "key-lock-full-track"),
        (ResidentContext::KeyLockFiniteLoop, "key-lock-finite-loop"),
    ];
    for (context, label) in contexts {
        let mut sample = full.clone();
        Arc::make_mut(sample.residency.as_mut().unwrap()).context = context;
        engine.sample_cache.lock().unwrap()[0] = Some(sample.clone());
        engine.loaded_source_generations.lock().unwrap()[0] = (1, 48_000);
        engine.loaded_source_digests.lock().unwrap()[0] = Some("a".repeat(64));
        engine
            .input_runtime_ownership
            .publish_test_source(0, &sample, 48_000, 1);
        let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
        assert!(binding.current());
        Python::attach(|py| {
            let value = binding.metadata(py).unwrap();
            let metadata = value.bind(py).cast::<PyDict>().unwrap();
            assert_eq!(
                metadata
                    .get_item("resident_context")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                label
            );
        });
        for (published_context, _) in contexts {
            let mut published = sample.clone();
            Arc::make_mut(published.residency.as_mut().unwrap()).context = published_context;
            // Every address, extent and revision stays equal: only context can fence this.
            assert!(Arc::ptr_eq(&sample.samples, &published.samples));
            assert!(sample.same_source(&published));
            engine.input_runtime_ownership.publish_window(0, &published);
            assert_eq!(binding.current(), published_context == context);
        }
    }
}

#[derive(Default)]
struct CapturedFeedback {
    events: Vec<AudioMessage>,
}

impl AudioMessageSink for CapturedFeedback {
    fn push_audio_message(&mut self, message: AudioMessage) {
        self.events.push(message);
    }

    fn available_audio_message_slots(&mut self) -> usize {
        usize::MAX
    }
}

struct RuntimeFixture {
    engine: AudioEngine,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    runtime: InputRuntime,
    mixer: Box<RtMixer>,
    transport: TransportTimeline,
    scheduler: FixedCapacityScheduler<8>,
    mode: TriggerQuantization,
    messages: CapturedFeedback,
    multi_loop: bool,
    preserved_loop_phase_start: Option<usize>,
    captured_start: Option<CapturedPadStart>,
}

impl RuntimeFixture {
    fn new() -> Self {
        let engine = test_engine();
        let (producer, consumer) = queue(8);
        let engine = with_control_producer(engine, &producer);
        let runtime = InputRuntime::new_with_ownership(
            producer.clone(),
            InputClock::new(),
            engine.input_runtime_ownership.clone(),
        );
        runtime.set_render_format(RATE, 1).unwrap();
        let mut mixer = acknowledged_mixer(&engine);
        mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
        let mut fixture = Self {
            engine,
            producer,
            consumer,
            runtime,
            mixer,
            transport: TransportTimeline::new(RATE),
            scheduler: FixedCapacityScheduler::new(),
            mode: TriggerQuantization::Immediate,
            messages: CapturedFeedback::default(),
            multi_loop: false,
            preserved_loop_phase_start: None,
            captured_start: None,
        };
        // Control-side source publication alone grants no callback telemetry.
        // Adopt the actual retained PCM through production LoadSample before
        // the synthetic timing/input kernel below expects content-bound feedback.
        let ownership = fixture.engine.input_runtime_ownership.clone();
        let sample = fixture.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        let (generation, rate) = fixture.engine.loaded_source_generations.lock().unwrap()[0];
        let placement = ownership.slot_snapshot(0).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(placement, placement.lifetime())
            .unwrap();
        let publication = ownership
            .publish_source_write(
                &write,
                &sample,
                rate,
                generation,
                ownership.authority_revision(0),
                None,
                false,
                || true,
            )
            .unwrap();
        fixture.apply(ControlMessage::LoadSample {
            id: 0,
            sample,
            publication: Some(publication),
        });
        assert_eq!(ownership.callback_feedback_key(0), placement.content_key());
        fixture.accept(decision());
        fixture.refresh(0.125, Some(3.0)).unwrap();
        fixture
    }

    fn apply(&mut self, message: ControlMessage) {
        process_control_message(
            message,
            &mut self.scheduler,
            self.transport.output_frame(),
            &mut self.mode,
            &mut self.transport,
            &mut self.mixer,
            &mut self.messages,
            &mut ImmediateAudioBufferRetirement,
        );
    }

    fn accept(&mut self, decision: TimingAcceptanceDecision) {
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
        self.apply(message);
    }

    fn refresh(&self, start: f64, end: Option<f64>) -> Result<(), String> {
        let binding = input_runtime_binding::capture(&self.engine, 0)
            .unwrap()
            .unwrap();
        self.refresh_with(&binding, start, end)
    }

    fn refresh_with(
        &self,
        binding: &InputRuntimePadBinding,
        start: f64,
        end: Option<f64>,
    ) -> Result<(), String> {
        let mut loaded = vec![false; NUM_SAMPLES];
        loaded[0] = true;
        let mut starts = vec![0.0; NUM_SAMPLES];
        starts[0] = start;
        let mut ends = vec![None; NUM_SAMPLES];
        ends[0] = end;
        let mut bindings = vec![None; NUM_SAMPLES];
        bindings[0] = Some(binding);
        self.runtime
            .set_runtime_state(self.multi_loop, loaded, starts, ends, bindings)
    }

    fn trigger_message(&mut self) -> ControlMessage {
        assert!(self.runtime.trigger_pad(0, 42));
        let target = self.consumer.pop().unwrap();
        let ControlMessage::CapturePadStartTarget(origin) = &target else {
            panic!("original ordinary target must precede its prepared start");
        };
        assert_eq!(origin.received_timestamp(), 42);
        assert_eq!(origin.control_guard().unwrap().id, 0);
        assert_eq!(origin.exclusive(), !self.multi_loop);
        let origin = origin.clone();
        self.apply(target);
        assert!(origin.target_frame().is_some());
        self.captured_start = Some(origin);
        let message = self.consumer.pop().unwrap();
        assert!(matches!(&message, ControlMessage::PreparedStart(_)));
        assert!(self.consumer.is_empty());
        message
    }

    fn assert_started_feedback(&self) {
        let [
            event @ AudioMessage::ContentFeedback {
                id: 0,
                owner,
                cell,
                lifetime,
                transport_revision,
                kind: 1,
                value,
            },
        ] = self.messages.events.as_slice()
        else {
            panic!(
                "one original-content start receipt expected: {:?}, origin={:?}",
                self.messages.events,
                self.captured_start
                    .as_ref()
                    .map(|origin| (origin.state(), origin.reason()))
            );
        };
        let binding = input_runtime_binding::capture(&self.engine, 0)
            .unwrap()
            .unwrap();
        assert!(binding.current());
        let placement = binding.binding.placement.unwrap();
        let origin = self.captured_start.as_ref().unwrap();
        assert_eq!(origin.state(), "applied");
        assert_eq!(placement.content_key(), Some(origin.content_key()));
        let ownership = &self.engine.input_runtime_ownership;
        assert_eq!(*owner, ownership.owner_identity().diagnostic());
        assert_eq!(*cell, placement.cell().index());
        assert_eq!(*lifetime, placement.lifetime());
        assert_eq!(
            *transport_revision,
            ownership
                .transport_intent_for_cell(placement.cell())
                .load(Ordering::Acquire)
        );
        assert_eq!(*value, 0.0);
        assert_eq!(self.engine._resolve_pad_feedback(event).unwrap(), Some(0));
    }

    fn execute_due(&mut self) {
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

    fn assert_no_trigger_effect(&mut self) {
        assert!(self.messages.events.is_empty());
        assert!(
            !self
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        if self.mixer.can_play_sample(0, 1.0) {
            assert!(self.mixer.play_sample(0, 1.0));
            assert_eq!(
                self.mixer
                    .voices
                    .iter()
                    .find(|voice| voice.active && voice.sample_id == 0)
                    .unwrap()
                    .frame_pos,
                2_000
            );
        } else {
            assert!(!self.mixer.play_sample(0, 1.0));
            // The guarded old trigger cannot overwrite newer loop intent, even when a new
            // ordinary start is also correctly denied by unavailable source/timing authority.
            assert_eq!(
                self.mixer
                    .phase_aligned_initial_sample_frame(0, RATE as usize * 4, 0.0),
                self.preserved_loop_phase_start.unwrap()
            );
        }
    }

    fn load_fixture_pad(&mut self, id: usize, sample: SampleBuffer) {
        self.engine.sample_cache.lock().unwrap()[id] = Some(sample.clone());
        self.engine.loaded_source_generations.lock().unwrap()[id] = (1, RATE);
        self.engine.loaded_source_digests.lock().unwrap()[id] = Some("a".repeat(64));
        self.engine
            .input_runtime_ownership
            .publish_test_source(id, &sample, RATE, 1);
        self.mixer.load_sample(id, sample);
    }

    fn preserve_new_loop(&mut self) {
        self.mixer.set_pad_loop_region(0, 0.25, Some(4.0));
        self.preserved_loop_phase_start = Some(self.mixer.phase_aligned_initial_sample_frame(
            0,
            RATE as usize * 4,
            0.0,
        ));
    }
}

#[test]
fn current_accepted_midi_binding_publishes_one_effect_with_exact_metadata() {
    let mut fixture = RuntimeFixture::new();
    let binding = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    Python::attach(|py| {
        let value = binding.metadata(py).unwrap();
        let metadata = value.bind(py).cast::<PyDict>().unwrap();
        let accepted = metadata.get_item("accepted_timing").unwrap().unwrap();
        assert!(
            accepted
                .eq(current_metadata(&fixture.engine, py, 0)
                    .unwrap()
                    .unwrap()
                    .bind(py))
                .unwrap()
        );
        assert_eq!(
            metadata
                .get_item("source_id")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "loaded-0-7"
        );
    });
    let message = fixture.trigger_message();
    assert!(matches!(&message, ControlMessage::PreparedStart(_)));
    fixture.apply(message);
    fixture.assert_started_feedback();
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap()
            .frame_pos,
        1_000
    );
}

#[test]
fn midi_pending_rejected_or_new_request_preserves_effective_accepted_binding() {
    let mut fixture = RuntimeFixture::new();
    let old = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
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
    assert!(old.current());
    let mut requests = fixture.engine.pad_request_ids.lock().unwrap();
    PadRequestAdvance::prepare(&mut requests[0], &fixture.engine.prepared_source_epochs[0])
        .unwrap()
        .commit();
    drop(requests);
    fixture.apply(pending_message);
    assert_eq!(pending.publication_status().unwrap(), "rejected");
    assert!(old.current());
    fixture.refresh_with(&old, 0.125, Some(3.0)).unwrap();
    let trigger = fixture.trigger_message();
    fixture.apply(trigger);
    fixture.assert_started_feedback();
}

#[test]
fn revision_only_replacement_rejects_queued_and_quantized_midi_without_loop_or_exclusive_effect() {
    for scheduled in [false, true] {
        let mut fixture = RuntimeFixture::new();
        fixture.load_fixture_pad(1, source());
        assert!(fixture.mixer.play_sample(1, 1.0));
        let old_binding = input_runtime_binding::capture(&fixture.engine, 0)
            .unwrap()
            .unwrap();
        if scheduled {
            fixture.transport.advance_by_rendered_frames(1);
            fixture.mode = TriggerQuantization::Grid { step_64ths: 4 };
        }
        let trigger = fixture.trigger_message();
        if scheduled {
            fixture.apply(trigger.clone());
            assert_eq!(fixture.scheduler.len(), 1);
        }
        let mut changed_decision = decision();
        changed_decision
            .provenance
            .push_str("; second explicit decision");
        fixture.accept(changed_decision);
        assert!(!old_binding.current());
        fixture.preserve_new_loop();
        if scheduled {
            fixture.execute_due();
        } else {
            fixture.apply(trigger);
        }
        assert!(
            fixture
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 1)
        );
        fixture.assert_no_trigger_effect();
    }
}

#[test]
fn manual_tap_legacy_automatic_roundtrip_rejects_old_midi_before_queued_clear() {
    for intent in [
        TimingIntent::Manual,
        TimingIntent::Tap,
        TimingIntent::Legacy,
    ] {
        let mut fixture = RuntimeFixture::new();
        let trigger = fixture.trigger_message();
        set_intent(&fixture.engine, &fixture.producer, 0, intent).unwrap();
        set_intent(
            &fixture.engine,
            &fixture.producer,
            0,
            TimingIntent::Automatic,
        )
        .unwrap();
        fixture.preserve_new_loop();
        fixture.apply(trigger);
        fixture.assert_no_trigger_effect();
        assert!(fixture.refresh(0.125, Some(3.0)).is_err());
    }
}

#[test]
fn source_revocation_rejects_old_bank_during_replacement_or_unload_gap() {
    for replacement in [false, true] {
        let mut fixture = RuntimeFixture::new();
        let trigger = fixture.trigger_message();
        let ownership = fixture.engine.input_runtime_ownership.clone();
        ownership.revoke(0, ownership.next_authority(0).unwrap());
        ownership.revoke_source(0);
        fixture.engine.sample_cache.lock().unwrap()[0] = None;
        fixture.engine.loaded_source_digests.lock().unwrap()[0] = None;
        assert!(
            input_runtime_binding::capture(&fixture.engine, 0)
                .unwrap()
                .is_none()
        );
        if replacement {
            fixture.mixer.load_sample(0, source());
        }
        fixture.preserve_new_loop();
        fixture.apply(trigger);
        fixture.assert_no_trigger_effect();
    }
}

#[test]
fn desired_loop_refresh_preserves_published_original_attack_endpoints_and_target() {
    for changed in [false, true] {
        let mut fixture = RuntimeFixture::new();
        fixture.transport.advance_by_rendered_frames(1);
        fixture.mode = TriggerQuantization::Grid { step_64ths: 4 };
        let trigger = fixture.trigger_message();
        fixture.apply(trigger);
        let original = fixture.captured_start.as_ref().unwrap().clone();
        let target = original.target_frame().unwrap();
        assert_eq!(target, u64::from(RATE) / 8);
        assert_eq!(fixture.scheduler.peek_next_target_frame(), Some(target));
        fixture
            .refresh(if changed { 0.25 } else { 0.125 }, Some(3.0))
            .unwrap();
        fixture.preserve_new_loop();
        fixture.execute_due();
        fixture.assert_started_feedback();
        assert_eq!(original.target_frame(), Some(target));
        assert_eq!(original.effective_frame(), Some(target));
        assert_eq!(fixture.mixer.loop_region_frames(0), (1_000, Some(24_000)));
        let voice = fixture
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap();
        assert_eq!(voice.frame_pos, 1_000);
        assert_eq!(voice.source_playback.position().frame, 1_000);
        assert_eq!(voice.source_playback.position().fraction, 0.0);
    }
}

#[test]
fn same_value_legacy_edits_revoke_midi_and_invalid_refresh_preserves_runtime_revision() {
    let mut fixture = RuntimeFixture::new();
    set_intent(&fixture.engine, &fixture.producer, 0, TimingIntent::Legacy).unwrap();
    let clear = fixture.consumer.pop().unwrap();
    fixture.apply(clear);
    fixture.refresh(0.125, Some(3.0)).unwrap();
    let trigger = fixture.trigger_message();
    publish_legacy_origin(&fixture.engine, &fixture.producer, 0, origin().seconds).unwrap();
    fixture.preserve_new_loop();
    fixture.apply(trigger);
    fixture.assert_no_trigger_effect();
    fixture.mixer.stop_sample(0);
    let origin_message = fixture.consumer.pop().unwrap();
    fixture.apply(origin_message);
    fixture.refresh(0.125, Some(3.0)).unwrap();
    let old_revision = fixture.engine.input_runtime_ownership.runtime_revision(0);
    assert!(fixture.refresh(-1.0, Some(3.0)).is_err());
    assert_eq!(
        fixture.engine.input_runtime_ownership.runtime_revision(0),
        old_revision
    );
    let (parameters, _) = rtrb::RingBuffer::new(1);
    let parameters = Arc::new(Mutex::new(parameters));
    let trigger = fixture.trigger_message();
    publish_legacy_bpm(&fixture.engine, &parameters, 0, Some(120.0)).unwrap();
    fixture.preserve_new_loop();
    fixture.apply(trigger);
    fixture.assert_no_trigger_effect();
}

#[test]
fn midi_voice_capacity_rejection_preserves_loop_and_other_voices() {
    let mut fixture = RuntimeFixture::new();
    fixture.multi_loop = true;
    fixture.refresh(0.125, Some(3.0)).unwrap();
    let sample = source();
    for id in 1..=crate::audio_engine::constants::MAX_VOICES {
        fixture.load_fixture_pad(id, sample.clone());
        assert!(fixture.mixer.play_sample(id, 1.0));
    }
    fixture.preserve_new_loop();
    let trigger = fixture.trigger_message();
    fixture.apply(trigger);
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        crate::audio_engine::constants::MAX_VOICES
    );
    assert!(
        !fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    fixture.mixer.stop_sample(1);
    fixture.assert_no_trigger_effect();
}
