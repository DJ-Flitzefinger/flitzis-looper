//! Current native acceptance coupled to productive MIDI queue/callback/scheduler.

use super::*;
use crate::audio_engine::audio_stream::{execute_scheduled_command, process_control_message};
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::input_runtime_binding::{self, InputRuntimePadBinding};
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::timing::InputClock;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};

struct RuntimeFixture {
    engine: AudioEngine,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    runtime: InputRuntime,
    mixer: RtMixer,
    transport: TransportTimeline,
    scheduler: FixedCapacityScheduler<8>,
    mode: TriggerQuantization,
    messages: Vec<AudioMessage>,
    multi_loop: bool,
    preserved_loop_phase_start: Option<usize>,
}

impl RuntimeFixture {
    fn new() -> Self {
        let engine = test_engine();
        let (producer, consumer) = queue(8);
        let runtime = InputRuntime::new_with_ownership(
            producer.clone(),
            InputClock::new(),
            engine.input_runtime_ownership.clone(),
        );
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
            messages: Vec::new(),
            multi_loop: false,
            preserved_loop_phase_start: None,
        };
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
        self.consumer.pop().unwrap()
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
        assert!(self.messages.is_empty());
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
            .publish_source(id, &sample, RATE, 1);
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
    assert!(matches!(
        message,
        ControlMessage::TriggerInputPad {
            received_at_ns: 42,
            ..
        }
    ));
    fixture.apply(message);
    assert!(matches!(
        fixture.messages.as_slice(),
        [AudioMessage::SampleStarted { id: 0 }]
    ));
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
    assert!(matches!(
        fixture.messages.as_slice(),
        [AudioMessage::SampleStarted { id: 0 }]
    ));
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
        let trigger = fixture.trigger_message();
        if scheduled {
            fixture.transport.advance_by_rendered_frames(1);
            fixture.mode = TriggerQuantization::Grid { step_64ths: 4 };
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
fn loop_refresh_rejects_scheduled_old_endpoints_but_identical_refresh_preserves_them() {
    for changed in [false, true] {
        let mut fixture = RuntimeFixture::new();
        fixture.transport.advance_by_rendered_frames(1);
        fixture.mode = TriggerQuantization::Grid { step_64ths: 4 };
        let trigger = fixture.trigger_message();
        fixture.apply(trigger);
        fixture
            .refresh(if changed { 0.25 } else { 0.125 }, Some(3.0))
            .unwrap();
        fixture.preserve_new_loop();
        fixture.execute_due();
        if changed {
            fixture.assert_no_trigger_effect();
        } else {
            assert!(matches!(
                fixture.messages.as_slice(),
                [AudioMessage::SampleStarted { id: 0 }]
            ));
        }
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
    let old_revision = fixture.engine.input_runtime_ownership.runtime[0].load(Ordering::Acquire);
    assert!(fixture.refresh(-1.0, Some(3.0)).is_err());
    assert_eq!(
        fixture.engine.input_runtime_ownership.runtime[0].load(Ordering::Acquire),
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
