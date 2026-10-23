//! Productive admission, callback queues, scheduler, render and current-ownership proofs.

use super::*;
use crate::audio_engine::accepted_timing_refresh::{self, AcceptedTimingRefreshTicket};
use crate::audio_engine::audio_stream::{
    drain_control_messages, drain_parameter_messages, execute_scheduled_command,
    process_control_message, render_scheduled_audio,
};
use crate::audio_engine::buffer_retirement::AudioBufferRetirement;
use crate::audio_engine::input_runtime_binding::{self, InputRuntimePadBinding};
use crate::audio_engine::scheduler::{FixedCapacityScheduler, ScheduledCommand};
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, ControlParameterMessage, TriggerQuantization};

#[path = "master_provenance_tests.rs"]
mod master_provenance_tests;

struct Fixture {
    engine: AudioEngine,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    parameter_producer: rtrb::Producer<ControlParameterMessage>,
    parameter_consumer: rtrb::Consumer<ControlParameterMessage>,
    mixer: Box<RtMixer>,
    transport: TransportTimeline,
    scheduler: FixedCapacityScheduler<4>,
    mode: TriggerQuantization,
    messages: Vec<AudioMessage>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_source_identity(false)
    }

    fn with_source_identity(complete: bool) -> Self {
        let engine = test_engine();
        if complete {
            let sample = engine.sample_cache.lock().unwrap()[0]
                .clone()
                .unwrap()
                .with_complete_source(RATE);
            engine
                .input_runtime_ownership
                .publish_test_source(0, &sample, RATE, 7);
            engine.sample_cache.lock().unwrap()[0] = Some(sample);
        }
        let mut mixer = acknowledged_mixer(&engine);
        mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
        let (producer, consumer) = queue(128);
        let engine = with_control_producer(engine, &producer);
        let (parameter_producer, parameter_consumer) = rtrb::RingBuffer::new(128);
        let mut fixture = Self {
            engine,
            producer,
            consumer,
            parameter_producer,
            parameter_consumer,
            mixer,
            transport: TransportTimeline::new(RATE),
            scheduler: FixedCapacityScheduler::new(),
            mode: TriggerQuantization::Immediate,
            messages: Vec::new(),
        };
        fixture.publish("initial");
        fixture.process_next();
        fixture.mixer.set_bpm_lock(true);
        fixture.mixer.set_pad_loop_region(0, 0.25, Some(2.0));
        fixture
    }

    fn publish(&self, provenance: &str) -> ConstantTimingTicket {
        let ticket = synthetic_ticket(&self.engine);
        let mut assertion = decision();
        assertion.provenance.push_str(provenance);
        publish(
            &self.engine,
            &self.producer,
            &ticket,
            &hypotheses(),
            origin(),
            assertion,
        )
        .unwrap();
        ticket
    }

    fn binding(&self) -> InputRuntimePadBinding {
        input_runtime_binding::capture(&self.engine, 0)
            .unwrap()
            .unwrap()
    }

    fn refresh(&self, master: Option<f64>) -> AcceptedTimingRefreshTicket {
        accepted_timing_refresh::enqueue(
            &self.engine,
            &self.producer,
            &self.binding(),
            0.125,
            Some(1.75),
            master,
        )
        .unwrap()
    }

    fn apply(&mut self, message: ControlMessage, retirement: &mut impl AudioBufferRetirement) {
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

    fn process_next(&mut self) {
        let message = self.consumer.pop().unwrap();
        self.apply(message, &mut ImmediateAudioBufferRetirement);
    }

    fn controls(&mut self) -> usize {
        drain_control_messages(
            &mut self.consumer,
            &mut self.scheduler,
            self.transport.output_frame(),
            &mut self.mode,
            &mut self.transport,
            &mut self.mixer,
            &mut self.messages,
            &mut ImmediateAudioBufferRetirement,
        )
    }

    fn parameters(&mut self) {
        drain_parameter_messages(
            &mut self.parameter_consumer,
            &mut self.mixer,
            &mut self.transport,
        );
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let frame = self.transport.output_frame();
        let mut output = vec![0.0; frames];
        render_scheduled_audio(
            &mut self.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            frame,
            1,
            &mut self.transport,
            &mut self.messages,
            &mut ImmediateAudioBufferRetirement,
        );
        self.transport.advance_by_rendered_frames(frames);
        output
    }

    fn global_parameter(&mut self, message: ControlParameterMessage) {
        let (speed, period) = match message {
            ControlParameterMessage::SetSpeed(speed) => (Some(speed), None),
            ControlParameterMessage::SetMasterPeriod(period) => (None, Some(period)),
            ControlParameterMessage::SetSpeedAndMasterPeriod {
                speed,
                period_seconds,
            } => (Some(speed), Some(period_seconds)),
            _ => panic!("fixture requires a global rate parameter"),
        };
        super::super::super::push_global_rate_message(
            &self.engine.global_timing_revision,
            &self.engine.input_runtime_ownership.global_timing_publishing,
            &self.engine.input_runtime_ownership.global_rate_status,
            &mut self.parameter_producer,
            speed,
            period,
            "fixture-global-parameter",
        )
        .unwrap();
    }

    fn state(&self) -> ((usize, Option<usize>), Option<f64>) {
        (
            self.mixer.loop_region_frames(0),
            self.transport.master_period_seconds(),
        )
    }
}

fn finite_refresh_fixture() -> Fixture {
    let mut fixture = Fixture::with_source_identity(true);
    fixture.mixer.set_bpm_lock(false);
    fixture.mixer.set_pad_loop_region(0, 0.125, Some(1.75));
    fixture.mixer.set_speed(0.73);
    let full = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let finite = full
        .window(
            1_000,
            14_000,
            2,
            crate::messages::ResidentContext::KeyLockFiniteLoop,
        )
        .unwrap();
    let publication = PreparedSourcePermit::unrestricted();
    publication.mark_pending().unwrap();
    fixture.apply(
        ControlMessage::RelocateResident(Box::new(crate::messages::ResidentTransaction {
            id: 0,
            sample: finite.clone(),
            stems: None,
            binding: fixture.binding().binding,
            publication: publication.clone(),
            expected_window_revision: 1,
            intent: crate::messages::ResidentControlIntent {
                key_lock: Some(true),
                ..Default::default()
            },
            seek_pin: None,
        })),
        &mut ImmediateAudioBufferRetirement,
    );
    assert_eq!(publication.status(), "accepted");
    fixture.engine.sample_cache.lock().unwrap()[0] = Some(finite);
    assert!(fixture.binding().current() && fixture.binding().available());
    assert!(fixture.mixer.play_sample_at_output_frame_rt(
        0,
        1.0,
        0,
        &mut ImmediateAudioBufferRetirement
    ));
    fixture.render(13);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !fixture.mixer.voices[0].stretch.source_preparation_ready()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(fixture.mixer.voices[0].stretch.source_preparation_ready());
    fixture.render(4083);
    fixture.render(257);
    assert!(
        fixture.mixer.voices[0]
            .stretch
            .adopted_request_id()
            .is_some()
    );
    fixture
}

#[test]
fn finite_accepted_refresh_executor_acks_fixed_geometry_for_active_and_paused_native_history() {
    for paused in [false, true] {
        let mut fixture = finite_refresh_fixture();
        if paused {
            fixture
                .mixer
                .pause_sample_at_output_frame(0, fixture.transport.output_frame());
        }
        let position = fixture.mixer.voices[0].source_playback.position();
        let native = fixture.mixer.voices[0].stretch.native_state_address();
        let history = fixture.mixer.voices[0]
            .stretch
            .productive_history()
            .unwrap();
        let ticket = fixture.refresh(None);
        fixture.process_next();
        fixture.render(2);
        assert_eq!(ticket.publication_status(), "accepted");
        assert!(ticket.is_current());
        assert_eq!(fixture.mixer.loop_region_frames(0), (1_000, Some(14_000)));
        assert_eq!(
            fixture.mixer.voices[0].stretch.native_state_address(),
            native
        );
        assert_eq!(
            fixture.mixer.voices[0]
                .stretch
                .productive_history()
                .unwrap()
                .binding,
            history.binding
        );
        if paused {
            assert_eq!(fixture.mixer.voices[0].source_playback.position(), position);
        }
        // The same public executor cannot turn a physical marker edit into a timing alias.
        let cut = accepted_timing_refresh::enqueue(
            &fixture.engine,
            &fixture.producer,
            &fixture.binding(),
            0.126,
            Some(1.75),
            None,
        )
        .unwrap();
        fixture.process_next();
        fixture.render(2);
        assert_eq!(cut.publication_status(), "rejected");
        assert_eq!(fixture.mixer.loop_region_frames(0), (1_000, Some(14_000)));
        assert_eq!(
            fixture.mixer.voices[0].stretch.native_state_address(),
            native
        );
    }
}

#[test]
fn finite_accepted_refresh_executor_rejects_old_active_and_paused_native_source_before_effects() {
    for paused in [false, true] {
        let mut fixture = finite_refresh_fixture();
        if paused {
            fixture
                .mixer
                .pause_sample_at_output_frame(0, fixture.transport.output_frame());
        }
        let native = fixture.mixer.voices[0].stretch.native_state_address();
        let position = fixture.mixer.voices[0].source_playback.position();
        let old_source = fixture.mixer.voices[0].sample.as_ref().unwrap().clone();
        let replacement = source().with_complete_source(RATE);
        fixture.engine.sample_cache.lock().unwrap()[0] = Some(replacement.clone());
        fixture
            .engine
            .input_runtime_ownership
            .publish_test_source(0, &replacement, RATE, 7);
        fixture.mixer.load_sample(0, replacement);
        fixture.publish("finite-old-source-rejection");
        fixture.process_next();
        let before = fixture.state();
        let ticket = fixture.refresh(None);
        fixture.process_next();
        fixture.render(2);
        assert_eq!(ticket.publication_status(), "rejected");
        assert_eq!(fixture.state(), before);
        assert!(
            fixture.mixer.voices[0]
                .sample
                .as_ref()
                .unwrap()
                .same_window(&old_source)
        );
        assert_eq!(
            fixture.mixer.voices[0].stretch.native_state_address(),
            native
        );
        if paused {
            assert_eq!(fixture.mixer.voices[0].source_playback.position(), position);
        }
    }
}

#[test]
fn accepted_refresh_is_one_acknowledged_loop_master_effect_after_parameter_drain() {
    let mut fixture = Fixture::new();
    fixture.global_parameter(ControlParameterMessage::SetMasterPeriod(0.75));
    let before = fixture.state();
    let accepted_before = fixture.binding().binding.accepted.unwrap();
    let ticket = fixture.refresh(Some(PERIOD));
    assert_eq!(ticket.publication_status(), "pending");
    assert!(!ticket.is_current());
    fixture.controls();
    assert_eq!(fixture.state(), before);
    assert_eq!(fixture.scheduler.peek_next_target_frame(), Some(1));
    // A subsequent immediate control drain cannot execute the deferred effect at frame zero.
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    fixture.controls();
    assert_eq!(ticket.publication_status(), "pending");
    fixture.parameters();
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.75));
    fixture.render(4);
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(ticket.is_current());
    assert_eq!(fixture.state(), ((1_000, Some(14_000)), Some(PERIOD)));
    assert_eq!(fixture.binding().binding.accepted, Some(accepted_before));
    assert_eq!(
        accepted_before.origin_seconds.to_bits(),
        origin().seconds.to_bits()
    );
    assert_eq!(fixture.transport.bootstrap_reference(), Some(0));
}

#[test]
fn accepted_refresh_coupled_master_replaces_foreign_pending_bootstrap_before_source_becomes_ready()
{
    let mut fixture = Fixture::new();
    let other = source();
    fixture.engine.sample_cache.lock().unwrap()[1] = Some(other.clone());
    fixture.engine.loaded_source_generations.lock().unwrap()[1] = (1, RATE);
    fixture.engine.loaded_source_digests.lock().unwrap()[1] = Some("a".repeat(64));
    fixture
        .engine
        .input_runtime_ownership
        .publish_test_source(1, &other, RATE, 1);
    fixture.mixer.load_sample(1, other);
    fixture.mixer.set_pad_bpm(1, Some(80.0));
    fixture.transport.request_bootstrap(1);
    let expected_beat = fixture.transport.beat_position_at_frame(1).unwrap();
    let ticket = fixture.refresh(Some(PERIOD));
    fixture.process_next();
    // Pad1 is unavailable for bootstrap at frame0. It starts after refresh at frame1,
    // so an unrelated pending reference must not anchor the acknowledged master phase.
    fixture
        .scheduler
        .schedule(
            1,
            ScheduledCommand::PlaySample {
                expected_source_generation: None,
                id: 1,
                volume: 1.0,
                received_at_ns: None,
            },
        )
        .unwrap();
    fixture.parameters();
    fixture.render(4);
    assert!(ticket.is_current());
    assert!(
        fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 1)
    );
    assert_eq!(fixture.transport.master_period_seconds(), Some(PERIOD));
    assert!((fixture.transport.beat_position_at_frame(1).unwrap() - expected_beat).abs() < 1e-12);
    assert_eq!(fixture.transport.bootstrap_reference(), Some(0));
}

#[test]
fn accepted_refresh_coupled_master_preserves_completed_bootstrap_and_beat_epoch() {
    let mut fixture = Fixture::new();
    fixture.transport.request_bootstrap(1);
    assert!(
        fixture
            .transport
            .bootstrap_from_source_period_at_frame(0.75, -3.25, 0)
    );
    assert_eq!(fixture.transport.bootstrap_reference(), None);
    let expected_beat = fixture.transport.beat_position_at_frame(1).unwrap();
    let ticket = fixture.refresh(Some(PERIOD));
    fixture.process_next();
    fixture.parameters();
    fixture.render(4);
    assert!(ticket.is_current());
    assert_eq!(fixture.transport.bootstrap_reference(), None);
    assert_eq!(fixture.transport.master_period_seconds(), Some(PERIOD));
    assert!((fixture.transport.beat_position_at_frame(1).unwrap() - expected_beat).abs() < 1e-12);
}

#[test]
fn accepted_refresh_old_parameter_backlog_rejects_whole_effect_then_fresh_retry_wins() {
    use crate::audio_engine::global_rate_status::{
        GlobalRateStatus, RateError, RateState, RateTuple,
    };

    let mut fixture = Fixture::new();
    let effect_image = |status: &GlobalRateStatus| {
        (
            status.effect.revision,
            status.effect.requested,
            status.effect.effective,
            status.effect.state,
            status.effect.execution_frame,
            status.effect.error,
            status.master_provenance,
        )
    };
    let before_rate = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    let expected_rate = RateTuple {
        speed: 1.0,
        period: Some(0.75),
    };
    let before_revision = fixture
        .engine
        .global_timing_revision
        .load(Ordering::Acquire);
    for _ in 0..65 {
        fixture.global_parameter(ControlParameterMessage::SetMasterPeriod(0.75));
    }
    let final_request_revision = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .requested_revision();
    assert_eq!(final_request_revision, before_revision + 65);
    let before_loop = fixture.mixer.loop_region_frames(0);
    let ticket = fixture.refresh(Some(PERIOD));
    fixture.controls();
    fixture.parameters();
    assert!(!fixture.parameter_consumer.is_empty());
    fixture.render(2);
    assert_eq!(ticket.publication_status(), "rejected");
    assert_eq!(fixture.mixer.loop_region_frames(0), before_loop);
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.5));
    let prefix = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert_eq!(prefix.request_revision, final_request_revision);
    assert!(prefix.effect.revision < final_request_revision);
    assert_eq!(prefix.requested, expected_rate);
    // The newer original message must drain before terminal cleanup can consume
    // its retained prefix. No receipt or provenance from that prefix is actual.
    assert_eq!(effect_image(&prefix), effect_image(&before_rate));
    assert_eq!(fixture.parameter_consumer.slots(), 1);
    assert!(fixture.mixer.has_pending_global_rate());
    assert_eq!(fixture.mixer.next_global_rate_frame(), None);
    // The final original stamp must apply before a fresh refresh can claim its fence.
    fixture.parameters();
    assert!(fixture.parameter_consumer.is_empty());
    assert!(!fixture.mixer.has_pending_global_rate());
    assert_eq!(fixture.mixer.loop_region_frames(0), before_loop);
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.75));
    let final_effect = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert_eq!(final_effect.request_revision, final_request_revision);
    assert_eq!(final_effect.effect.revision, final_request_revision);
    assert_eq!(final_effect.requested, expected_rate);
    assert_eq!(final_effect.effect.requested, expected_rate);
    assert_eq!(final_effect.effect.effective, expected_rate);
    assert_eq!(final_effect.effect.state, RateState::Applied);
    assert_eq!(final_effect.effect.error, RateError::None);
    assert_eq!(final_effect.effect.execution_frame, Some(2));
    let retry = fixture.refresh(Some(PERIOD));
    fixture.controls();
    fixture.parameters();
    fixture.render(2);
    assert!(retry.is_current());
    assert_eq!(fixture.transport.master_period_seconds(), Some(PERIOD));
    assert_eq!(fixture.mixer.loop_region_frames(0), (1_000, Some(14_000)));
    let retry_rate = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert!(retry_rate.master_provenance.accepted_revision().is_some());
    let retry_beat_epoch = fixture.transport.beat_position_at_frame(4).unwrap();
    fixture.parameters();
    fixture.render(2);
    assert_eq!(fixture.transport.master_period_seconds(), Some(PERIOD));
    assert_eq!(fixture.mixer.loop_region_frames(0), (1_000, Some(14_000)));
    assert_eq!(
        fixture.transport.beat_position_at_frame(4),
        Some(retry_beat_epoch)
    );
    let later_rate = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert_eq!(later_rate.request_revision, retry_rate.request_revision);
    assert_eq!(later_rate.requested, retry_rate.requested);
    assert_eq!(effect_image(&later_rate), effect_image(&retry_rate));
    assert_eq!(ticket.publication_status(), "rejected");
    assert!(!ticket.is_current());
}

#[test]
fn accepted_refresh_newer_global_parameter_or_lock_edit_rejects_loop_and_master() {
    for change in 0..5 {
        let mut fixture = Fixture::new();
        let before_loop = fixture.mixer.loop_region_frames(0);
        let ticket = fixture.refresh(Some(PERIOD));
        fixture.process_next();
        match change {
            0 => fixture.global_parameter(ControlParameterMessage::SetSpeed(1.25)),
            1 => fixture.global_parameter(ControlParameterMessage::SetMasterPeriod(0.8)),
            2 => fixture.global_parameter(ControlParameterMessage::SetSpeedAndMasterPeriod {
                speed: 1.25,
                period_seconds: 0.8,
            }),
            3 => {
                super::super::super::push_global_timing_message(
                    &fixture.engine.global_timing_revision,
                    &fixture
                        .engine
                        .input_runtime_ownership
                        .global_timing_publishing,
                    &mut fixture.producer.lock().unwrap(),
                    ControlMessage::SetBpmLock(false),
                    "SetBpmLock",
                )
                .unwrap();
                fixture.process_next();
            }
            _ => {
                super::super::super::push_global_timing_message(
                    &fixture.engine.global_timing_revision,
                    &fixture
                        .engine
                        .input_runtime_ownership
                        .global_timing_publishing,
                    &mut fixture.producer.lock().unwrap(),
                    ControlMessage::BootstrapTransportFromPad { id: 1 },
                    "BootstrapTransportFromPad",
                )
                .unwrap();
                fixture.process_next();
            }
        }
        fixture.parameters();
        fixture.render(2);
        assert_eq!(ticket.publication_status(), "rejected");
        assert!(!ticket.is_current());
        assert_eq!(fixture.mixer.loop_region_frames(0), before_loop);
        if change == 1 || change == 2 {
            assert_eq!(fixture.transport.master_period_seconds(), Some(0.8));
        }
    }
}

#[test]
fn accepted_refresh_actual_speed_and_lock_reject_stale_calculation_at_admission_interleaving() {
    for (speed, lock, supplied) in [
        (1.25, true, PERIOD),
        (1.0, false, PERIOD),
        (1.0, true, f64::from_bits(PERIOD.to_bits() + 1)),
    ] {
        let mut fixture = Fixture::new();
        let before = fixture.state();
        fixture.mixer.set_speed(speed);
        fixture.mixer.set_bpm_lock(lock);
        // Current global revision is captured after the changed native control; values still
        // must be derived from the actual native speed/lock at callback execution.
        let ticket = fixture.refresh(Some(supplied));
        fixture.controls();
        fixture.parameters();
        fixture.render(2);
        assert_eq!(ticket.publication_status(), "rejected");
        assert_eq!(fixture.state(), before);
    }
    let mut fixture = Fixture::new();
    fixture.global_parameter(ControlParameterMessage::SetSpeed(1.25));
    let ticket = fixture.refresh(Some(PERIOD / 1.25));
    fixture.controls();
    fixture.parameters();
    fixture.render(2);
    assert!(ticket.is_current());
    assert_eq!(
        fixture.transport.master_period_seconds(),
        Some(PERIOD / 1.25)
    );
}

#[test]
fn accepted_refresh_loop_only_remains_independent_of_global_parameter_backlog_and_edits() {
    let mut fixture = Fixture::new();
    for _ in 0..65 {
        fixture.global_parameter(ControlParameterMessage::SetSpeed(1.25));
    }
    let before_master = fixture.transport.master_period_seconds();
    let ticket = fixture.refresh(None);
    fixture.process_next();
    fixture.global_parameter(ControlParameterMessage::SetSpeed(1.5));
    fixture.parameters();
    fixture.render(2);
    assert!(ticket.is_current());
    assert_eq!(fixture.mixer.loop_region_frames(0), (1_000, Some(14_000)));
    assert_eq!(fixture.transport.master_period_seconds(), before_master);
}

#[test]
fn accepted_refresh_capacity_failure_preserves_authority_and_current_timing() {
    let fixture = Fixture::new();
    let binding = fixture.binding();
    let before = fixture.state();
    let before_authority = binding.binding.authority_revision;
    let (producer, mut consumer) = queue(1);
    producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert!(
        accepted_timing_refresh::enqueue(
            &fixture.engine,
            &producer,
            &binding,
            0.125,
            Some(1.75),
            Some(PERIOD)
        )
        .is_err()
    );
    assert_eq!(
        fixture.binding().binding.authority_revision,
        before_authority
    );
    assert_eq!(fixture.state(), before);
    assert!(matches!(consumer.pop().unwrap(), ControlMessage::Ping()));
    assert_eq!(fixture.binding().binding.accepted, binding.binding.accepted);
}

#[test]
fn accepted_refresh_unavailable_manual_tap_legacy_foreign_and_stale_bindings_have_no_effect() {
    for intent in [
        TimingIntent::Manual,
        TimingIntent::Tap,
        TimingIntent::Legacy,
        TimingIntent::Automatic,
    ] {
        let fixture = Fixture::new();
        fixture.engine.timing_intents.lock().unwrap()[0] = intent;
        fixture
            .engine
            .input_runtime_ownership
            .set_timing_intent(0, intent);
        if intent == TimingIntent::Automatic {
            fixture.engine.current_timing_acknowledgements.clear(0);
        }
        let binding = fixture.binding();
        let before = fixture.state();
        assert!(
            accepted_timing_refresh::enqueue(
                &fixture.engine,
                &fixture.producer,
                &binding,
                0.125,
                Some(1.75),
                None
            )
            .is_err()
        );
        assert_eq!(fixture.state(), before);
    }
    let fixture = Fixture::new();
    let foreign = Fixture::new().binding();
    assert!(
        accepted_timing_refresh::enqueue(
            &fixture.engine,
            &fixture.producer,
            &foreign,
            0.125,
            Some(1.75),
            None
        )
        .is_err()
    );
    let stale = fixture.binding();
    let next = fixture
        .engine
        .input_runtime_ownership
        .next_authority(0)
        .unwrap();
    fixture.engine.input_runtime_ownership.revoke(0, next);
    assert!(
        accepted_timing_refresh::enqueue(
            &fixture.engine,
            &fixture.producer,
            &stale,
            0.125,
            Some(1.75),
            None
        )
        .is_err()
    );
}

#[test]
fn accepted_refresh_pending_replacement_uses_previous_current_then_rejects_after_new_ack() {
    let mut fixture = Fixture::new();
    let before = fixture.state();
    let old_projection = fixture.binding().binding.accepted;
    let replacement = fixture.publish("replacement");
    assert_eq!(replacement.publication_status().unwrap(), "pending");
    assert_eq!(fixture.binding().binding.accepted, old_projection);
    let ticket = fixture.refresh(Some(PERIOD));
    fixture.process_next(); // New accepted publication is ordered before the old-current refresh.
    fixture.process_next();
    assert_eq!(ticket.publication_status(), "rejected");
    assert_eq!(fixture.state(), before);
    assert_ne!(fixture.binding().binding.accepted, old_projection);
    let retry = fixture.refresh(Some(PERIOD));
    fixture.process_next();
    fixture.parameters();
    fixture.render(2);
    assert!(retry.is_current());
}

#[test]
fn accepted_refresh_rejected_replacement_retains_previous_current_and_refreshes_it() {
    let mut fixture = Fixture::new();
    let old_projection = fixture.binding().binding.accepted;
    let replacement = fixture.publish("rejected");
    let newer_request = synthetic_ticket(&fixture.engine);
    // A newer preparation request retires only the pending publication, never prior ACK.
    let epoch = next_epoch(&fixture.engine.prepared_source_epochs[0]).unwrap();
    fixture.engine.prepared_source_epochs[0].store(epoch, Ordering::Release);
    drop(newer_request);
    fixture.process_next();
    assert_eq!(replacement.publication_status().unwrap(), "rejected");
    assert_eq!(fixture.binding().binding.accepted, old_projection);
    let ticket = fixture.refresh(None);
    fixture.process_next();
    fixture.parameters();
    fixture.render(2);
    assert!(ticket.is_current());
}

#[test]
fn accepted_refresh_delayed_control_budget_is_revoked_before_queued_global_change_executes() {
    let mut fixture = Fixture::new();
    for _ in 0..65 {
        fixture
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
    }
    let before = fixture.state();
    let ticket = fixture.refresh(Some(PERIOD));
    let frame = fixture.transport.output_frame();
    assert_eq!(
        drain_control_messages(
            &mut fixture.consumer,
            &mut fixture.scheduler,
            frame,
            &mut fixture.mode,
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut fixture.messages,
            &mut ImmediateAudioBufferRetirement
        ),
        64
    );
    assert_eq!(ticket.publication_status(), "pending");
    fixture.global_parameter(ControlParameterMessage::SetMasterPeriod(0.8));
    fixture.parameters();
    fixture.process_next();
    fixture.process_next();
    assert_eq!(ticket.publication_status(), "rejected");
    assert_eq!(fixture.mixer.loop_region_frames(0), before.0);
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.8));
}

#[test]
fn accepted_refresh_ack_becomes_noncurrent_on_equal_loop_authority_source_or_global_edits() {
    for mutation in 0..4 {
        let mut fixture = Fixture::new();
        let ticket = fixture.refresh(Some(PERIOD));
        fixture.process_next();
        fixture.parameters();
        fixture.render(2);
        assert!(ticket.is_current());
        match mutation {
            0 => fixture.engine.input_runtime_ownership.revoke(
                0,
                fixture
                    .engine
                    .input_runtime_ownership
                    .next_authority(0)
                    .unwrap(),
            ),
            1 => fixture.engine.input_runtime_ownership.revoke_source(0),
            2 => fixture.engine.current_timing_acknowledgements.clear(0),
            _ => fixture.global_parameter(ControlParameterMessage::SetSpeed(1.0)),
        }
        assert_eq!(ticket.publication_status(), "accepted");
        assert!(!ticket.is_current());
    }
}

#[test]
fn accepted_refresh_scheduled_source_authority_or_accepted_replacement_rejects_before_effect() {
    for mutation in 0..3 {
        let mut fixture = Fixture::new();
        let before = fixture.state();
        let ticket = fixture.refresh(Some(PERIOD));
        fixture.process_next();
        match mutation {
            0 => fixture.engine.input_runtime_ownership.revoke_source(0),
            1 => fixture.engine.input_runtime_ownership.revoke(
                0,
                fixture
                    .engine
                    .input_runtime_ownership
                    .next_authority(0)
                    .unwrap(),
            ),
            _ => {
                fixture.publish("new-ack-before-scheduled-refresh");
                fixture.process_next();
            }
        }
        fixture.parameters();
        fixture.render(2);
        assert_eq!(ticket.publication_status(), "rejected");
        assert_eq!(fixture.state(), before);
    }
}

#[test]
fn accepted_refresh_unexecuted_queue_teardown_reports_rejection_without_callback_work() {
    let fixture = Fixture::new();
    let ticket = fixture.refresh(None);
    assert_eq!(ticket.publication_status(), "pending");
    drop(fixture);
    assert_eq!(ticket.publication_status(), "rejected");
    assert!(!ticket.is_current());
}

#[test]
fn accepted_refresh_rejects_old_active_and_paused_source_pins_after_current_bank_replacement() {
    for paused in [false, true] {
        let mut fixture = Fixture::new();
        fixture.mixer.set_bpm_lock(false);
        assert!(fixture.mixer.play_sample_at_output_frame_rt(
            0,
            1.0,
            0,
            &mut ImmediateAudioBufferRetirement
        ));
        if paused {
            fixture.mixer.pause_sample_at_output_frame(0, 0);
        }
        let old_timing = fixture.mixer.voices[0].source_timing.accepted;
        let old_pin = fixture.mixer.voices[0]
            .sample
            .as_ref()
            .unwrap()
            .samples
            .clone();
        let old_position = fixture.mixer.voices[0].frame_pos;
        let replacement = source();
        fixture.engine.sample_cache.lock().unwrap()[0] = Some(replacement.clone());
        fixture
            .engine
            .input_runtime_ownership
            .publish_test_source(0, &replacement, RATE, 7);
        fixture.mixer.load_sample(0, replacement);
        fixture.publish("new-current-source");
        fixture.process_next();
        let before = fixture.state();
        let ticket = fixture.refresh(None);
        fixture.process_next();
        assert_eq!(ticket.publication_status(), "rejected");
        assert_eq!(fixture.state(), before);
        assert!(Arc::ptr_eq(
            &fixture.mixer.voices[0].sample.as_ref().unwrap().samples,
            &old_pin
        ));
        assert_eq!(fixture.mixer.voices[0].source_timing.accepted, old_timing);
        assert_eq!(fixture.mixer.voices[0].frame_pos, old_position);
        assert_eq!(fixture.mixer.voices[0].paused, paused);
    }
}

#[test]
fn accepted_refresh_same_source_active_and_paused_pins_follow_new_current_acceptance() {
    for paused in [false, true] {
        let mut fixture = Fixture::new();
        fixture.mixer.set_bpm_lock(false);
        assert!(fixture.mixer.play_sample_at_output_frame_rt(
            0,
            1.0,
            0,
            &mut ImmediateAudioBufferRetirement
        ));
        if paused {
            fixture.mixer.pause_sample_at_output_frame(0, 0);
        }
        let pin = fixture.mixer.voices[0]
            .sample
            .as_ref()
            .unwrap()
            .samples
            .clone();
        let previous = fixture.binding().binding.accepted;
        fixture.publish("same-source-new-current");
        fixture.process_next();
        assert_ne!(fixture.binding().binding.accepted, previous);
        let ticket = fixture.refresh(None);
        fixture.process_next();
        fixture.parameters();
        fixture.render(2);
        assert!(ticket.is_current());
        assert!(Arc::ptr_eq(
            &fixture.mixer.voices[0].sample.as_ref().unwrap().samples,
            &pin
        ));
        assert_eq!(fixture.mixer.voices[0].paused, paused);
        assert_eq!(fixture.mixer.loop_region_frames(0), (1_000, Some(14_000)));
    }
}

#[test]
fn accepted_refresh_invalid_values_or_epoch_overflow_preserve_admitted_ownership() {
    let fixture = Fixture::new();
    let binding = fixture.binding();
    let original = binding.binding.authority_revision;
    for (start, end, master) in [
        (f64::NAN, Some(1.75), None),
        (-1.0, Some(1.75), None),
        (0.125, Some(f64::INFINITY), None),
        (0.125, Some(1.75), Some(0.0)),
        (0.125, Some(1.75), Some(f64::MAX)),
    ] {
        assert!(
            accepted_timing_refresh::enqueue(
                &fixture.engine,
                &fixture.producer,
                &binding,
                start,
                end,
                master
            )
            .is_err()
        );
        assert_eq!(fixture.binding().binding.authority_revision, original);
    }
    fixture
        .engine
        .input_runtime_ownership
        .authority_for_cell(
            fixture
                .engine
                .input_runtime_ownership
                .slot_snapshot(0)
                .unwrap()
                .cell(),
        )
        .store(u64::MAX, Ordering::Release);
    let maximum = fixture.binding();
    assert!(
        accepted_timing_refresh::enqueue(
            &fixture.engine,
            &fixture.producer,
            &maximum,
            0.125,
            Some(1.75),
            None
        )
        .is_err()
    );
    assert_eq!(fixture.binding().binding.authority_revision, u64::MAX);
}

#[test]
fn accepted_refresh_global_capacity_or_revision_failure_preserves_current_ack_and_queued_value() {
    let mut fixture = Fixture::new();
    let ticket = fixture.refresh(Some(PERIOD));
    fixture.process_next();
    fixture.parameters();
    fixture.render(2);
    assert!(ticket.is_current());
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    producer
        .push(ControlParameterMessage::SetSpeed(1.0))
        .unwrap();
    let revision = fixture
        .engine
        .global_timing_revision
        .load(Ordering::Acquire);
    assert!(
        super::super::super::push_global_timing_message(
            &fixture.engine.global_timing_revision,
            &fixture
                .engine
                .input_runtime_ownership
                .global_timing_publishing,
            &mut producer,
            ControlParameterMessage::SetSpeed(1.25),
            "SetSpeed"
        )
        .is_err()
    );
    assert_eq!(
        fixture
            .engine
            .global_timing_revision
            .load(Ordering::Acquire),
        revision
    );
    assert!(ticket.is_current());
    assert_eq!(
        consumer.pop().unwrap(),
        ControlParameterMessage::SetSpeed(1.0)
    );
    fixture
        .engine
        .global_timing_revision
        .store(u64::MAX, Ordering::Release);
    assert!(
        super::super::super::push_global_timing_message(
            &fixture.engine.global_timing_revision,
            &fixture
                .engine
                .input_runtime_ownership
                .global_timing_publishing,
            &mut producer,
            ControlParameterMessage::SetSpeed(1.25),
            "SetSpeed"
        )
        .is_err()
    );
    assert_eq!(
        fixture
            .engine
            .global_timing_revision
            .load(Ordering::Acquire),
        u64::MAX
    );
    assert!(consumer.pop().is_err());
}

struct RetainedRefresh {
    slots: usize,
    refreshes: Vec<Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>>,
}

impl AudioBufferRetirement for RetainedRefresh {
    fn retire_resident_capture(
        &mut self,
        _: std::sync::Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: std::sync::Arc<std::sync::atomic::AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
    fn retire_sample(&mut self, _: SampleBuffer) {
        unreachable!()
    }
    fn retire_prepared_stems(&mut self, _: crate::messages::PreparedStemSet) {
        unreachable!()
    }
    fn retire_constant_timing(&mut self, _: PreparedConstantTiming) {
        unreachable!()
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        unreachable!()
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        refresh: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        self.refreshes.push(refresh);
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.slots
    }
}

#[test]
fn accepted_refresh_retirement_backpressure_defers_admission_and_failed_execution_retires_payload()
{
    let mut fixture = Fixture::new();
    let before = fixture.state();
    let ticket = fixture.refresh(None);
    let mut retirement = RetainedRefresh {
        slots: 0,
        refreshes: Vec::new(),
    };
    assert_eq!(
        drain_control_messages(
            &mut fixture.consumer,
            &mut fixture.scheduler,
            0,
            &mut fixture.mode,
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut fixture.messages,
            &mut retirement
        ),
        0
    );
    assert_eq!(ticket.publication_status(), "pending");
    assert_eq!(fixture.state(), before);
    retirement.slots = 1;
    let message = fixture.consumer.pop().unwrap();
    fixture.apply(message, &mut retirement);
    let event = fixture.scheduler.pop_due_through(0, 1).unwrap();
    let ScheduledCommand::RefreshAcceptedTiming(ref payload) = event.command else {
        panic!()
    };
    let weak = Arc::downgrade(payload);
    retirement.slots = 0;
    let result = execute_scheduled_command(
        &mut fixture.mixer,
        &mut fixture.transport,
        1,
        event.command,
        &mut fixture.messages,
        &mut retirement,
    );
    let crate::audio_engine::audio_stream::ScheduledExecution::Pending(command) = result else {
        panic!("retirement pressure must retain the original scheduled owner")
    };
    assert_eq!(ticket.publication_status(), "pending");
    assert_eq!(fixture.state(), before);
    assert!(weak.upgrade().is_some());
    retirement.slots = 1;
    let result = execute_scheduled_command(
        &mut fixture.mixer,
        &mut fixture.transport,
        2,
        command,
        &mut fixture.messages,
        &mut retirement,
    );
    assert!(matches!(
        result,
        crate::audio_engine::audio_stream::ScheduledExecution::Complete
    ));
    assert_eq!(ticket.publication_status(), "accepted");
    retirement.refreshes.clear(); // Control-side destruction of the immutable owner.
    assert!(weak.upgrade().is_none());
}

#[test]
fn accepted_refresh_scheduler_full_and_frame_overflow_reject_without_mutation() {
    for overflow in [false, true] {
        let mut fixture = Fixture::new();
        if overflow {
            fixture
                .transport
                .advance_by_rendered_frames(u64::MAX as usize);
        } else {
            for _ in 0..4 {
                fixture
                    .scheduler
                    .schedule(100, ScheduledCommand::StopAll)
                    .unwrap();
            }
        }
        let before = fixture.state();
        let ticket = fixture.refresh(None);
        fixture.process_next();
        assert_eq!(ticket.publication_status(), "rejected");
        assert_eq!(fixture.state(), before);
    }
}

#[test]
fn accepted_capture_binds_automatic_unavailable_source_and_request_before_worker() {
    let fixture = Fixture::new();
    fixture.engine.current_timing_acknowledgements.clear(0);
    let binding = fixture.binding();
    assert!(!binding.available());
    let before = fixture.engine.pad_request_ids.lock().unwrap()[0];
    let capture = capture_preparation(
        &fixture.engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.001,
            provenance: "independent fixture error".into(),
        },
        Some(&binding),
    )
    .unwrap();
    assert_eq!(capture.request_id, before + 1);
    assert!(Arc::ptr_eq(
        &capture.sample.samples,
        &fixture.engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    let captured_epoch = capture.captured_epoch;
    let _next = capture_preparation(
        &fixture.engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.001,
            provenance: "newer explicit request".into(),
        },
        Some(&fixture.binding()),
    )
    .unwrap();
    assert_ne!(
        fixture.engine.prepared_source_epochs[0].load(Ordering::Acquire),
        captured_epoch
    );
    assert!(prepare_captured(&fixture.engine, &capture).is_err());
}

#[test]
fn accepted_capture_rejects_stale_source_authority_and_manual_before_advancing_request() {
    for mutation in 0..3 {
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let before = fixture.engine.pad_request_ids.lock().unwrap()[0];
        match mutation {
            0 => fixture.engine.input_runtime_ownership.revoke_source(0),
            1 => fixture.engine.input_runtime_ownership.revoke(
                0,
                fixture
                    .engine
                    .input_runtime_ownership
                    .next_authority(0)
                    .unwrap(),
            ),
            _ => fixture.engine.timing_intents.lock().unwrap()[0] = TimingIntent::Manual,
        }
        assert!(
            capture_preparation(
                &fixture.engine,
                0,
                TimingBound {
                    halfwidth_seconds: 0.001,
                    provenance: "independent fixture error".into()
                },
                Some(&binding)
            )
            .is_err()
        );
        assert_eq!(fixture.engine.pad_request_ids.lock().unwrap()[0], before);
    }
}

#[test]
fn accepted_captured_worker_prepares_actual_native_qm_without_recapturing_or_publishing() {
    let fixture = Fixture::new();
    let capture = capture_preparation(
        &fixture.engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.001,
            provenance: "independent complete fixture error".into(),
        },
        Some(&fixture.binding()),
    )
    .unwrap();
    let request_id = capture.request_id;
    let ticket = prepare_captured(&fixture.engine, &capture).unwrap();
    assert_eq!(
        fixture.engine.pad_request_ids.lock().unwrap()[0],
        request_id
    );
    assert_eq!(ticket.request_id, request_id);
    assert!(Arc::ptr_eq(&ticket.sample.samples, &capture.sample.samples));
    let BackendEvidence::Qm { raw, input } = ticket.evidence.backend() else {
        panic!()
    };
    assert_eq!(
        input.frame_count,
        (capture.sample.samples.len() as u64 * 44_100).div_ceil(u64::from(RATE))
    );
    assert!(!raw.beat_frames().is_empty());
    assert_eq!(ticket.publication_status().unwrap(), "captured");
    assert!(fixture.consumer.is_empty());
}

#[test]
fn accepted_refresh_accepts_same_source_binding_after_unrelated_publication_revision() {
    let mut fixture = Fixture::new();
    let binding = fixture.binding();
    let ownership = &fixture.engine.input_runtime_ownership;
    let other = ownership.slot_snapshot(215).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(other, other.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(&write, || true, |_| {})
        .unwrap();
    assert!(binding.is_current());
    let fresh = fixture.binding();
    assert_ne!(binding.binding, fresh.binding);
    assert!(binding.binding.same_projection(&fresh.binding));
    let ticket = accepted_timing_refresh::enqueue(
        &fixture.engine,
        &fixture.producer,
        &binding,
        0.125,
        Some(1.75),
        None,
    )
    .unwrap();
    fixture.process_next();
    fixture.render(2);
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(ticket.is_current());
    assert_eq!(fixture.mixer.loop_region_frames(0), (1_000, Some(14_000)));
}

fn drain_bpm_fixture(fixture: &mut Fixture) {
    drain_control_messages(
        &mut fixture.consumer,
        &mut fixture.scheduler,
        fixture.transport.output_frame(),
        &mut fixture.mode,
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.messages,
        &mut ImmediateAudioBufferRetirement,
    );
    fixture.parameters();
    fixture.render(0);
}

#[test]
fn bpm_status_separates_admission_from_actual_application_and_requalifies_original_ack() {
    let mut fixture = Fixture::new();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let initial = ownership.global_rate_status.read_bpm().unwrap();
    assert_eq!((initial.0, initial.1.revision), (0, 0));
    let original = fixture.binding();
    fixture
        .engine
        .set_bpm_lock_with_binding(true, Some(&original))
        .unwrap();
    let queued = ownership.global_rate_status.read_bpm().unwrap();
    assert!(queued.0 > queued.1.revision);
    // A real source-authority revocation after enqueue cannot be recaptured by the target.
    let next = ownership.next_authority(0).unwrap();
    ownership.revoke(0, next);
    drain_bpm_fixture(&mut fixture);
    let stale = ownership.global_rate_status.read_bpm().unwrap();
    assert_eq!(stale.0, stale.1.revision);
    assert!(!stale.1.applied);
    assert_eq!(
        stale.1.reason,
        crate::audio_engine::global_rate_status::BpmReason::ReferenceChanged
    );
    assert!(stale.1.effective);
    let qualified = fixture.binding();
    fixture
        .engine
        .set_bpm_lock_with_binding(true, Some(&qualified))
        .unwrap();
    drain_bpm_fixture(&mut fixture);
    let applied = ownership.global_rate_status.read_bpm().unwrap();
    assert!(applied.1.applied && applied.1.effective);
    assert!(applied.0 > stale.0);
    assert_eq!(applied.0, applied.1.revision);
    assert_eq!(applied.1.execution_frame, Some(0));
}

#[test]
fn bpm_full_busy_and_exhausted_admission_have_no_receipt_or_queue_prefix() {
    for refusal in 0..3 {
        let fixture = Fixture::new();
        let ownership = &fixture.engine.input_runtime_ownership;
        if refusal == 0 {
            let mut producer = fixture.producer.lock().unwrap();
            while !producer.is_full() {
                producer.push(ControlMessage::Ping()).unwrap();
            }
        } else if refusal == 1 {
            ownership
                .global_timing_publishing
                .store(true, Ordering::Release);
        } else {
            fixture
                .engine
                .global_timing_revision
                .store(u64::MAX, Ordering::Release);
        }
        let before = fixture
            .engine
            .global_timing_revision
            .load(Ordering::Acquire);
        let receipt = ownership.global_rate_status.read_bpm().unwrap().0;
        let slots = fixture.consumer.slots();
        assert!(
            fixture
                .engine
                .set_bpm_lock_with_binding(false, None)
                .is_err()
        );
        assert_eq!(
            fixture
                .engine
                .global_timing_revision
                .load(Ordering::Acquire),
            before
        );
        assert_eq!(ownership.global_rate_status.read_bpm().unwrap().0, receipt);
        assert_eq!(fixture.consumer.slots(), slots);
        ownership
            .global_timing_publishing
            .store(false, Ordering::Release);
    }
}

#[test]
fn original_launch_cancellation_witness_rejects_before_admission_and_after_enqueue_including_aba() {
    for after_enqueue in [false, true] {
        let mut fixture = Fixture::new();
        let captured = fixture.binding();
        let before = fixture.state();
        let ticket = after_enqueue.then(|| {
            accepted_timing_refresh::enqueue(
                &fixture.engine,
                &fixture.producer,
                &captured,
                0.125,
                Some(1.75),
                None,
            )
            .unwrap()
        });
        fixture.engine.cancel_pad_launches(0).unwrap();
        fixture.engine.cancel_pad_launches(0).unwrap();
        if let Some(ticket) = ticket {
            drain_bpm_fixture(&mut fixture);
            fixture.render(2);
            assert_eq!(ticket.publication_status(), "rejected");
        } else {
            let authority = fixture.binding().binding.authority_revision;
            assert!(
                accepted_timing_refresh::enqueue(
                    &fixture.engine,
                    &fixture.producer,
                    &captured,
                    0.125,
                    Some(1.75),
                    None
                )
                .is_err()
            );
            assert_eq!(fixture.binding().binding.authority_revision, authority);
        }
        assert_eq!(fixture.state(), before);
    }
}

#[test]
fn accepted_refresh_global_cas_contention_preserves_original_owner_and_applies_at_retry_frame() {
    let mut fixture = Fixture::new();
    let before = fixture.state();
    let ticket = fixture.refresh(None);
    fixture.process_next();
    let event = fixture.scheduler.pop_due_through(0, 1).unwrap();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    ownership
        .global_timing_publishing
        .store(true, Ordering::Release);
    let result = execute_scheduled_command(
        &mut fixture.mixer,
        &mut fixture.transport,
        1,
        event.command,
        &mut fixture.messages,
        &mut ImmediateAudioBufferRetirement,
    );
    let crate::audio_engine::audio_stream::ScheduledExecution::Pending(command) = result else {
        panic!("global lease must retain pending refresh")
    };
    assert_eq!(fixture.state(), before);
    assert_eq!(ticket.publication_status(), "pending");
    ownership
        .global_timing_publishing
        .store(false, Ordering::Release);
    let result = execute_scheduled_command(
        &mut fixture.mixer,
        &mut fixture.transport,
        9,
        command,
        &mut fixture.messages,
        &mut ImmediateAudioBufferRetirement,
    );
    assert!(matches!(
        result,
        crate::audio_engine::audio_stream::ScheduledExecution::Complete
    ));
    assert!(ticket.is_current());
}

#[test]
fn callback_global_lease_winner_refuses_control_publication_without_receipt_or_partial_refresh() {
    let mut fixture = Fixture::new();
    let ticket = fixture.refresh(None);
    fixture.process_next();
    let event = fixture.scheduler.pop_due_through(0, 1).unwrap();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let revision = fixture.engine.global_timing_revision.clone();
    let producer = fixture.producer.clone();
    let before = revision.load(Ordering::Acquire);
    crate::audio_engine::audio_stream::with_refresh_global_claim_for_test(
        move || {
            let mut producer = producer.lock().unwrap();
            let slots = producer.slots();
            assert!(
                crate::audio_engine::push_global_timing_message(
                    &revision,
                    &ownership.global_timing_publishing,
                    &mut producer,
                    ControlMessage::Ping(),
                    "race-control"
                )
                .is_err()
            );
            assert_eq!(revision.load(Ordering::Acquire), before);
            assert_eq!(producer.slots(), slots);
        },
        || {
            execute_scheduled_command(
                &mut fixture.mixer,
                &mut fixture.transport,
                1,
                event.command,
                &mut fixture.messages,
                &mut ImmediateAudioBufferRetirement,
            )
        },
    );
    assert!(ticket.is_current());
    assert_eq!(fixture.mixer.loop_region_frames(0), (1000, Some(14000)));
}

fn publish_refresh_runtime(fixture: &mut Fixture, sync: bool, intent_revision: u64) {
    use crate::audio_engine::input_mapping::{InputRuntime, PitchPadProjection};
    use crate::audio_engine::source_map_admission::SourceMapRow;
    if fixture.engine.input_runtime.is_none() {
        fixture.engine.input_runtime = Some(InputRuntime::new_with_ownership(
            fixture.producer.clone(),
            crate::audio_engine::timing::InputClock::new(),
            fixture.engine.input_runtime_ownership.clone(),
        ));
    }
    let binding = fixture.binding();
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(binding);
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = sync;
    maps[0].intent_revision = intent_revision;
    fixture
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id == 0).collect(),
            vec![0.25; NUM_SAMPLES],
            vec![Some(2.0); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                PitchPadProjection {
                    sync: true,
                    key_lock: false,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(maps),
        )
        .unwrap();
}

#[test]
fn pad_sync_off_and_aba_runtime_publication_cancel_original_queued_refresh_geometry() {
    for aba in [false, true] {
        let mut fixture = Fixture::new();
        publish_refresh_runtime(&mut fixture, true, 1);
        let ticket = fixture.refresh(None);
        fixture.process_next();
        let before = fixture.state();
        publish_refresh_runtime(&mut fixture, false, 2);
        if aba {
            publish_refresh_runtime(&mut fixture, true, 3);
        }
        fixture.render(2);
        assert_eq!(ticket.publication_status(), "rejected");
        assert_eq!(fixture.state(), before);
    }
}

#[test]
fn ordinary_trigger_and_own_binding_rebase_keep_precise_refresh_witness_live() {
    let mut fixture = Fixture::new();
    publish_refresh_runtime(&mut fixture, false, 0);
    let ticket = fixture.refresh(None);
    let launch = fixture.engine.input_runtime_ownership.launch_revision(0);
    // This complete row publication carries the refresh's own authority-rebased binding.
    publish_refresh_runtime(&mut fixture, false, 0);
    assert_eq!(
        fixture.engine.input_runtime_ownership.launch_revision(0),
        launch
    );
    assert!(
        fixture
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .trigger_pad(0, 0)
    );
    drain_bpm_fixture(&mut fixture);
    assert_eq!(
        fixture.engine.input_runtime_ownership.launch_revision(0),
        launch
    );
    fixture.render(2);
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(ticket.is_current());
}

#[test]
fn qualified_initial_bpm_on_applies_flag_master_and_one_bootstrap_before_ack() {
    let mut fixture = Fixture::new();
    fixture.mixer.set_bpm_lock(false);
    fixture.transport.clear_master_bpm();
    assert_eq!(fixture.transport.master_period_seconds(), None);
    // Observe the callback-owned tuple through its existing actual receipt.
    assert_eq!(
        fixture
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap()
            .effect
            .effective
            .period,
        None
    );
    let binding = fixture.binding();
    let before = fixture.transport;
    fixture
        .engine
        .set_bpm_lock_with_binding(true, Some(&binding))
        .unwrap();
    assert!(fixture.transport.matches_clock_epoch(&before));
    assert_eq!(fixture.transport.master_period_seconds(), None);
    drain_bpm_fixture(&mut fixture);
    let ownership = &fixture.engine.input_runtime_ownership;
    let bpm = ownership.global_rate_status.read_bpm().unwrap().1;
    let rate = ownership.global_rate_status.read().unwrap();
    assert!(bpm.applied && bpm.effective);
    assert_eq!(bpm.execution_frame, Some(0));
    assert_eq!(rate.effect.effective.period, Some(PERIOD));
    assert_eq!(fixture.transport.master_period_seconds(), Some(PERIOD));
    assert_eq!(fixture.transport.bootstrap_reference(), Some(0));
    assert!(fixture.consumer.is_empty() && fixture.parameter_consumer.is_empty());
}

fn qualified_rate(fixture: &mut Fixture, speed: Option<f64>, period: f64) {
    let binding = fixture.binding();
    let anchor = crate::audio_engine::qualify_timing_anchor(&fixture.engine, &binding).unwrap();
    crate::audio_engine::push_global_rate_message_qualified(
        &fixture.engine.global_timing_revision,
        &fixture
            .engine
            .input_runtime_ownership
            .global_timing_publishing,
        &fixture.engine.input_runtime_ownership.global_rate_status,
        &mut fixture.parameter_producer,
        speed,
        Some(period),
        Some(anchor),
        "qualified-test-rate",
    )
    .unwrap();
}

#[test]
fn qualified_fast_master_rejects_source_and_hierarchy_changes_without_reviving_rejected_master() {
    for hierarchy in [false, true] {
        let mut fixture = Fixture::new();
        fixture.transport.clear_master_bpm();
        assert_eq!(fixture.transport.master_period_seconds(), None);
        // Observe the callback-owned tuple through its existing actual receipt.
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .global_rate_status
                .read()
                .unwrap()
                .effect
                .effective
                .period,
            None
        );
        qualified_rate(&mut fixture, None, PERIOD);
        if hierarchy {
            publish_refresh_runtime(&mut fixture, false, 0);
        } else {
            let ownership = &fixture.engine.input_runtime_ownership;
            ownership.revoke(0, ownership.next_authority(0).unwrap());
        }
        drain_bpm_fixture(&mut fixture);
        let rejected = fixture
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        assert_eq!(
            rejected.effect.state,
            crate::audio_engine::global_rate_status::RateState::Rejected
        );
        assert_eq!(fixture.transport.master_period_seconds(), None);
        fixture.global_parameter(ControlParameterMessage::SetSpeed(1.16));
        drain_bpm_fixture(&mut fixture);
        let speed = fixture
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        assert_eq!(speed.effect.effective.speed, 1.16);
        assert_eq!(speed.effect.effective.period, None);
        assert_eq!(fixture.transport.master_period_seconds(), None);
    }
}

#[test]
fn qualified_master_speed_only_coalescing_preserves_original_guard_and_new_master_can_supersede() {
    let mut fixture = Fixture::new();
    qualified_rate(&mut fixture, None, PERIOD);
    fixture.global_parameter(ControlParameterMessage::SetSpeed(1.16));
    // The old master would be inconsistent with the new speed; its qualification cannot vanish.
    drain_bpm_fixture(&mut fixture);
    let refused = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert_eq!(
        refused.effect.state,
        crate::audio_engine::global_rate_status::RateState::Rejected
    );
    assert_eq!(refused.effect.effective.speed, 1.0);
    qualified_rate(&mut fixture, Some(1.16), PERIOD / 1.16);
    drain_bpm_fixture(&mut fixture);
    let applied = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert_eq!(
        applied.effect.state,
        crate::audio_engine::global_rate_status::RateState::Applied
    );
    assert_eq!(applied.effect.effective.period, Some(PERIOD / 1.16));
    assert_eq!(
        fixture.transport.master_period_seconds(),
        Some(PERIOD / 1.16)
    );
}

fn fixed_bpm_reference_fixture() -> Fixture {
    use crate::audio_engine::input_mapping::PitchPadProjection;
    use crate::audio_engine::source_map_admission::SourceMapRow;
    use crate::audio_engine::source_sync_policy::GlobalTimingPolicy;
    let mut fixture = Fixture::with_source_identity(true);
    fixture.engine.test_output_format = Some((RATE, 1));
    fixture.mixer.set_pad_key_lock(0, false);
    fixture.mixer.set_bpm_lock(false);
    let original = fixture.binding();
    fixture
        .engine
        .set_bpm_lock_with_binding(true, Some(&original))
        .unwrap();
    drain_bpm_fixture(&mut fixture);
    publish_refresh_runtime(&mut fixture, false, 0);
    let runtime = fixture.engine.input_runtime.as_ref().unwrap();
    runtime.set_render_format(RATE, 1).unwrap();
    let original = fixture.binding();
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(original);
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = true;
    maps[0].intent_revision = 1;
    runtime
        .set_runtime_state_with_policy(
            true,
            (0..NUM_SAMPLES).map(|id| id == 0).collect(),
            vec![0.25; NUM_SAMPLES],
            vec![Some(2.0); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                PitchPadProjection {
                    sync: true,
                    key_lock: false,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(maps),
            GlobalTimingPolicy::parse(Some(
                r#"{"schema_version":1,"sync":true,"realtime_bpm":false,"revision":1}"#,
            ))
            .unwrap(),
        )
        .unwrap();
    let start = fixture
        .engine
        .capture_pad_start(0, 0.25, Some(2.0), 1.0, false, None, None)
        .unwrap();
    drain_bpm_fixture(&mut fixture);
    assert!(fixture.engine.publish_pad_start(&start, None).unwrap());
    drain_bpm_fixture(&mut fixture);
    assert_eq!(start.state(), "applied", "{:?}", start.reason());
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap()
            .sync_request
            .mode,
        crate::audio_engine::source_sync_policy::SourceSyncMode::Fixed
    );
    fixture.render(13);
    assert!(
        fixture.transport.bootstrap_reference().is_none(),
        "real initial bootstrap must complete before requalification"
    );
    fixture
}

#[test]
fn qualified_on_after_blocked_off_reinitializes_different_reference_even_when_actual_flag_stayed_on()
 {
    let mut fixture = fixed_bpm_reference_fixture();
    fixture
        .engine
        .set_bpm_lock_with_binding(false, None)
        .unwrap();
    drain_bpm_fixture(&mut fixture);
    let blocked = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap()
        .1;
    assert!(!blocked.applied && blocked.effective);
    assert_eq!(
        blocked.reason,
        crate::audio_engine::global_rate_status::BpmReason::PolicyPending
    );
    assert_eq!(fixture.transport.master_period_seconds(), Some(PERIOD));

    let other = source().with_complete_source(RATE);
    fixture.engine.sample_cache.lock().unwrap()[1] = Some(other.clone());
    fixture.engine.pad_request_ids.lock().unwrap()[1] = 1;
    fixture.engine.loaded_source_generations.lock().unwrap()[1] = (1, RATE);
    fixture.engine.loaded_source_digests.lock().unwrap()[1] = Some("b".repeat(64));
    fixture
        .engine
        .input_runtime_ownership
        .publish_test_source(1, &other, RATE, 1);
    fixture.mixer.load_sample(1, other);
    fixture.mixer.set_pad_bpm(1, Some(80.0));
    let other = input_runtime_binding::capture(&fixture.engine, 1)
        .unwrap()
        .unwrap();
    let frame = fixture.transport.output_frame();
    let beat = fixture.transport.beat_position_at_frame(frame).unwrap();
    let position = fixture
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap()
        .source_playback
        .position();
    fixture
        .engine
        .set_bpm_lock_with_binding(true, Some(&other))
        .unwrap();
    drain_bpm_fixture(&mut fixture);
    let applied = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap()
        .1;
    assert!(applied.applied && applied.effective && applied.revision > blocked.revision);
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.75));
    assert_eq!(
        fixture
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap()
            .effect
            .effective
            .period,
        Some(0.75)
    );
    assert!((fixture.transport.beat_position_at_frame(frame).unwrap() - beat).abs() < 1e-12);
    assert!(fixture.transport.bootstrap_reference().is_none());
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap()
            .source_playback
            .position(),
        position
    );
}

#[test]
fn qualified_equal_on_reinitializes_same_content_new_accepted_period_and_same_period_is_noop() {
    let mut fixture = fixed_bpm_reference_fixture();
    let original = fixture.binding();
    let key = original.binding.placement.unwrap().content_key().unwrap();
    let timing = crate::audio_engine::constant_timing::l3_constant_timing_fixture::publish_synthetic_loaded_timing(
        &fixture.engine, &fixture.producer, 0, 0.75);
    fixture.process_next();
    assert_eq!(timing.publication_status().unwrap(), "accepted");
    let current = fixture.binding();
    assert_eq!(current.binding.placement.unwrap().content_key(), Some(key));
    assert_ne!(current.binding.accepted, original.binding.accepted);
    let target = current.binding.accepted.unwrap().period_seconds;
    assert!((target - 0.75).abs() < 1e-12);
    let frame = fixture.transport.output_frame();
    let beat = fixture.transport.beat_position_at_frame(frame).unwrap();
    fixture
        .engine
        .set_bpm_lock_with_binding(true, Some(&current))
        .unwrap();
    drain_bpm_fixture(&mut fixture);
    assert_eq!(fixture.transport.master_period_seconds(), Some(target));
    assert!((fixture.transport.beat_position_at_frame(frame).unwrap() - beat).abs() < 1e-12);
    assert!(fixture.transport.bootstrap_reference().is_none());

    let before = fixture.transport;
    let rate_before = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap()
        .effect
        .effective;
    fixture
        .engine
        .set_bpm_lock_with_binding(true, Some(&fixture.binding()))
        .unwrap();
    drain_bpm_fixture(&mut fixture);
    let effect = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap()
        .1;
    assert!(effect.applied && effect.effective);
    assert!(fixture.transport.matches_clock_epoch(&before));
    assert_eq!(
        fixture
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap()
            .effect
            .effective,
        rate_before
    );
}

fn install_declared_retained_feed(fixture: &mut Fixture) {
    use crate::audio_engine::native_history_permit::NativeHistoryContext;
    use crate::audio_engine::pitch_intent::PitchIntent;
    use crate::audio_engine::prepared_native_history::{
        NativeAdapterState, RetainedNativeStartCut,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
    use crate::audio_engine::stretch_processor::ProductiveSourceFeed;

    const REQUEST: u64 = 0x5245_5441_494e;
    let snapshot = fixture.mixer.capture_pitch_voice_snapshot(0).unwrap();
    let logical = snapshot.playback;
    let plan = snapshot.plan;
    let sample = snapshot.sample.clone();
    let accepted = snapshot.accepted;
    let rate = snapshot.sample_rate_hz;
    let channels = sample.channels;
    assert_eq!(rate, RATE);
    assert_eq!(channels, 1);
    assert!(snapshot.stems.is_none());
    let epoch_owner = fixture.engine.prepared_source_epoch(0).unwrap().clone();
    let epoch = fixture.mixer.voices[0].stretch.preparation_epoch();
    let permit = NativeHistoryContext {
        id: 0,
        ownership: &fixture.engine.input_runtime_ownership,
        acknowledgements: &fixture.engine.current_timing_acknowledgements,
        preparation_epoch: &epoch_owner,
    }
    .capture(&sample, rate, accepted)
    .unwrap();
    let target = fixture.transport.output_frame();
    let pitch = PitchIntent::default();
    let scale = pitch
        .native_pitch_scale(logical.tempo_ratio(), true)
        .unwrap();
    let request = snapshot
        .into_request(permit.clone(), pitch, true, target, REQUEST, epoch)
        .unwrap();
    let mut native = RubberBandLiveShifter::new(rate, channels).unwrap();
    native.prepare_exact_pitch(scale).unwrap();
    let history = native.source_history_frames(scale).unwrap();
    let mut candidate = Box::new(NativeAdapterState::new(Some(native), channels));
    // This fixed raw-stream coordinate exercises a real displaced feed. It is
    // neither a calibration value nor a musical/ordinary-start alignment claim.
    candidate
        .prepare_retained_source_start_checked(
            request,
            Some(RetainedNativeStartCut {
                history_output_frames: history,
                discarded_raw_output_frames: history.checked_add(137).unwrap(),
            }),
        )
        .unwrap();
    let (actual, _) = candidate
        .retained_start_feed(&logical, plan)
        .unwrap()
        .unwrap();
    assert_ne!(actual.position(), logical.position());
    let feed = ProductiveSourceFeed {
        pitch_intent: pitch,
        key_lock: true,
        sample: &sample,
        stems: None,
        outgoing_stems: None,
        sample_rate_hz: rate,
        accepted,
        plan,
        playback: &logical,
        permit: Some(&permit),
        output_frame: Some(target),
    };
    let dsp_before = fixture.mixer.dsp_source_history_for_test(0).unwrap();
    assert!(fixture.mixer.voices[0].stretch.adopt_prepared_pitch(
        &mut candidate,
        &feed,
        epoch,
        REQUEST,
    ));
    assert!(fixture.mixer.voices[0].stretch.has_retained_source_input());
    assert_eq!(
        fixture.mixer.voices[0].stretch.adopted_request_id(),
        Some(REQUEST)
    );
    fixture.render(17);
    let voice = &fixture.mixer.voices[0];
    assert!(voice.stretch.has_retained_source_input());
    assert_eq!(
        voice.source_playback.position(),
        logical.position_at(17).unwrap()
    );
    assert_eq!(
        voice.stretch.productive_history().unwrap().next_position,
        actual.position_at(17).unwrap()
    );
    assert_ne!(
        voice.source_playback.position(),
        voice.stretch.productive_history().unwrap().next_position
    );
    let productive = voice.stretch.productive_history().unwrap();
    let mut reads = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut reads));
    let span = reads.spans().last().unwrap();
    assert!(span.frames > 0);
    assert_eq!(span.context.binding, productive.binding);
    assert_eq!(span.plan.frame_pos, span.playback.position().frame);
    assert_eq!(
        span.playback.position_at(span.frames).unwrap(),
        actual.position_at(17).unwrap()
    );
    assert_eq!(
        span.playback.position_at(span.frames).unwrap(),
        productive.next_position
    );
    // The filter consumes emitted Native output on the logical voice trajectory;
    // the input journal above records the distinct, already processed source feed.
    let dsp = fixture.mixer.dsp_source_history_for_test(0).unwrap();
    assert_eq!(dsp.next_position, logical.position_at(17).unwrap());
    assert_ne!(dsp.next_position, productive.next_position);
    assert_eq!(dsp.binding, dsp_before.binding);
    assert_eq!(dsp.domain, Some(plan.domain()));
    assert_eq!(dsp.fed_output_frames, dsp_before.fed_output_frames + 17);
}

fn complete_native_history_refresh_fixture(declared_retained_feed: bool) -> Fixture {
    let mut fixture = Fixture::with_source_identity(true);
    fixture.engine.test_output_format = Some((RATE, 1));
    fixture.mixer.set_bpm_lock(false);
    fixture.mixer.set_pad_loop_region(0, 0.125, Some(1.75));
    fixture.mixer.set_speed(0.73);
    fixture.mixer.set_pad_key_lock(0, true);
    let start = fixture
        .engine
        .capture_pad_start(0, 0.125, Some(1.75), 1.0, false, None, None)
        .unwrap();
    fixture.controls();
    assert!(fixture.engine.publish_pad_start(&start, None).unwrap());
    fixture.controls();
    let before_wait = fixture.transport.output_frame();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.has_ready_action(start.sequence()) {
            break;
        }
        assert_ne!(start.state(), "rejected", "{:?}", start.reason());
        assert!(
            std::time::Instant::now() < deadline,
            "original ordinary-start worker must become ready"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), before_wait);
    let execution = start.execution_frame().unwrap();
    fixture.render((execution - before_wait) as usize);
    fixture.render(257);
    assert_eq!(start.state(), "applied", "{:?}", start.reason());
    assert_eq!(start.effective_frame(), Some(execution));
    assert_eq!(
        fixture.mixer.voices[0].stretch.adopted_request_id(),
        Some(start.sequence())
    );
    assert!(!fixture.mixer.voices[0].stretch.has_retained_source_input());
    assert!(
        fixture.mixer.voices[0]
            .stretch
            .has_active_native_ownership()
    );
    assert_ne!(fixture.mixer.voices[0].stretch.native_state_address(), 0);
    assert_ne!(
        fixture.mixer.voices[0].stretch.pending_fifo_frames(),
        (0, 0)
    );
    assert!(
        fixture.mixer.voices[0]
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames
            > 0
    );
    let original = fixture.binding();
    fixture
        .engine
        .set_bpm_lock_with_binding(true, Some(&original))
        .unwrap();
    drain_bpm_fixture(&mut fixture);
    assert!(
        fixture
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read_bpm()
            .unwrap()
            .1
            .applied
    );
    if declared_retained_feed {
        install_declared_retained_feed(&mut fixture);
    }
    fixture.engine.pause_sample(0).unwrap();
    drain_bpm_fixture(&mut fixture);
    assert!(fixture.mixer.voices[0].active && fixture.mixer.voices[0].paused);
    assert!(
        fixture.mixer.voices[0]
            .stretch
            .has_active_native_ownership()
    );
    assert_eq!(
        fixture.mixer.voices[0].stretch.has_retained_source_input(),
        declared_retained_feed
    );
    assert!(fixture.mixer.voices[0].stretch.owns_pitch_contract(
        crate::audio_engine::pitch_intent::PitchIntent::default(),
        true,
    ));
    assert_ne!(fixture.mixer.voices[0].stretch.native_state_address(), 0);
    assert_ne!(
        fixture.mixer.voices[0].stretch.pending_fifo_frames(),
        (0, 0)
    );
    assert!(
        fixture.mixer.voices[0]
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames
            > 0
    );
    assert!(
        fixture
            .mixer
            .dsp_source_history_for_test(0)
            .unwrap()
            .fed_output_frames
            > 0
    );
    fixture
}

fn assert_productive_history_unchanged(
    actual: crate::audio_engine::productive_source_history::ProductiveSourceHistory,
    before: crate::audio_engine::productive_source_history::ProductiveSourceHistory,
) {
    assert_eq!(actual.binding, before.binding);
    assert_eq!(actual.next_position, before.next_position);
    assert_eq!(actual.domain, before.domain);
    assert_eq!(actual.fed_output_frames, before.fed_output_frames);
}

#[test]
fn qualified_rate_delayed_with_paused_productive_native_history_rejects_replaced_anchor_without_tuple_prefix()
 {
    let mut fixture = complete_native_history_refresh_fixture(false);
    let native = fixture.mixer.voices[0].stretch.native_state_address();
    let adopted = fixture.mixer.voices[0].stretch.adopted_request_id();
    let epoch = fixture.mixer.voices[0].stretch.source_epoch_owner().clone();
    let epoch_value = epoch.load(Ordering::Acquire);
    let fifo = fixture.mixer.voices[0].stretch.pending_fifo_frames();
    let playback = fixture.mixer.voices[0].source_playback;
    let history = fixture.mixer.voices[0]
        .stretch
        .productive_history()
        .unwrap();
    let dsp = fixture.mixer.dsp_source_history_for_test(0).unwrap();
    let tuple = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap()
        .effect
        .effective;
    let timeline = fixture.transport;
    qualified_rate(&mut fixture, Some(1.16), PERIOD / 1.16);
    fixture
        .engine
        .input_runtime_ownership
        .global_timing_publishing
        .store(true, Ordering::Release);
    fixture.parameters();
    assert!(
        fixture.mixer.has_pending_global_rate(),
        "the original qualified tuple must remain held before effect"
    );
    fixture
        .engine
        .input_runtime_ownership
        .global_timing_publishing
        .store(false, Ordering::Release);
    let replacement = source().with_complete_source(RATE);
    fixture.engine.sample_cache.lock().unwrap()[0] = Some(replacement.clone());
    fixture.engine.loaded_source_generations.lock().unwrap()[0] = (8, RATE);
    fixture.engine.loaded_source_digests.lock().unwrap()[0] = Some("b".repeat(64));
    fixture
        .engine
        .input_runtime_ownership
        .publish_test_source(0, &replacement, RATE, 8);
    drain_bpm_fixture(&mut fixture);
    let effect = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap()
        .effect;
    assert_eq!(
        effect.state,
        crate::audio_engine::global_rate_status::RateState::Rejected
    );
    assert_eq!(
        effect.error,
        crate::audio_engine::global_rate_status::RateError::SourceChanged
    );
    assert_eq!(effect.effective, tuple);
    assert!(fixture.transport.matches_clock_epoch(&timeline));
    let voice = &fixture.mixer.voices[0];
    assert!(voice.active && voice.paused && voice.stretch.has_active_native_ownership());
    assert!(!voice.stretch.has_retained_source_input());
    assert_eq!(voice.stretch.native_state_address(), native);
    assert_eq!(voice.stretch.adopted_request_id(), adopted);
    assert_eq!(epoch.load(Ordering::Acquire), epoch_value);
    assert_eq!(voice.stretch.pending_fifo_frames(), fifo);
    assert!(voice.source_playback.matches_exact(&playback));
    assert_productive_history_unchanged(voice.stretch.productive_history().unwrap(), history);
    assert_productive_history_unchanged(fixture.mixer.dsp_source_history_for_test(0).unwrap(), dsp);
}

#[test]
fn master_and_loop_cut_preserve_productive_native_and_displaced_feed_owners_without_epoch_or_clock_prefix()
 {
    for declared_retained_feed in [false, true] {
        let mut fixture = complete_native_history_refresh_fixture(declared_retained_feed);
        let timing = crate::audio_engine::constant_timing::l3_constant_timing_fixture::publish_synthetic_loaded_timing(
        &fixture.engine, &fixture.producer, 0, 0.75);
        fixture.process_next();
        assert_eq!(timing.publication_status().unwrap(), "accepted");
        let current = fixture.binding();
        let period = current.binding.accepted.unwrap().period_seconds / 0.73;
        assert_ne!(fixture.transport.master_period_seconds(), Some(period));
        let next_start = 0.75;
        assert!(
            fixture.mixer.voices[0].source_playback.position().frame
                < (next_start * f64::from(RATE)) as usize
        );
        assert!(
            fixture
                .mixer
                .loop_intent_context_available(0, next_start, Some(1.75))
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while fixture
            .mixer
            .prepare_loop_edit(0, next_start, Some(1.75))
            .is_none()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let edit = fixture
            .mixer
            .prepare_loop_edit(0, next_start, Some(1.75))
            .unwrap();
        assert!(
            !fixture.mixer.refresh_combined_history_ready(edit, period),
            "the actual Native/FIFO history owner must fence this physical cut plus new master"
        );
        let ticket = accepted_timing_refresh::enqueue(
            &fixture.engine,
            &fixture.producer,
            &current,
            next_start,
            Some(1.75),
            Some(period),
        )
        .unwrap();
        fixture.process_next();
        fixture.parameters();
        let first_frame = fixture.transport.output_frame() + 1;
        let event = fixture
            .scheduler
            .pop_due_through(fixture.transport.output_frame(), first_frame)
            .unwrap();
        let ScheduledCommand::RefreshAcceptedTiming(ref payload) = event.command else {
            panic!("original scheduled refresh required")
        };
        let owner = Arc::downgrade(payload);
        let mut command = event.command;
        let native = fixture.mixer.voices[0].stretch.native_state_address();
        let adopted = fixture.mixer.voices[0].stretch.adopted_request_id();
        let epoch = fixture.mixer.voices[0].stretch.source_epoch_owner().clone();
        let epoch_value = epoch.load(Ordering::Acquire);
        let fifo = fixture.mixer.voices[0].stretch.pending_fifo_frames();
        let history = fixture.mixer.voices[0]
            .stretch
            .productive_history()
            .unwrap();
        let dsp = fixture.mixer.dsp_source_history_for_test(0).unwrap();
        let playback = fixture.mixer.voices[0].source_playback;
        let loops = fixture.mixer.loop_region_frames(0);
        let timeline = fixture.transport;
        let tuple = fixture
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap()
            .effect
            .effective;
        let key = current.binding.placement.unwrap().content_key().unwrap();
        let source_control = fixture.mixer.source_control_revision(key).unwrap();
        for frame in [first_frame, first_frame + 17] {
            let result = execute_scheduled_command(
                &mut fixture.mixer,
                &mut fixture.transport,
                frame,
                command,
                &mut fixture.messages,
                &mut ImmediateAudioBufferRetirement,
            );
            let crate::audio_engine::audio_stream::ScheduledExecution::Pending(next) = result
            else {
                panic!("productive history pressure must retain pending refresh")
            };
            let ScheduledCommand::RefreshAcceptedTiming(ref payload) = next else {
                panic!("same original refresh required")
            };
            assert!(Arc::ptr_eq(payload, &owner.upgrade().unwrap()));
            command = next;
            assert_eq!(ticket.publication_status(), "pending");
            assert_eq!(fixture.mixer.loop_region_frames(0), loops);
            assert!(fixture.transport.matches_clock_epoch(&timeline));
            assert_eq!(
                fixture
                    .engine
                    .input_runtime_ownership
                    .global_rate_status
                    .read()
                    .unwrap()
                    .effect
                    .effective,
                tuple
            );
            assert_eq!(
                fixture.mixer.source_control_revision(key).unwrap(),
                source_control
            );
            let voice = &fixture.mixer.voices[0];
            assert!(voice.active && voice.paused && voice.stretch.has_active_native_ownership());
            assert_eq!(
                voice.stretch.has_retained_source_input(),
                declared_retained_feed
            );
            assert_eq!(voice.stretch.native_state_address(), native);
            assert_eq!(voice.stretch.adopted_request_id(), adopted);
            assert_eq!(epoch.load(Ordering::Acquire), epoch_value);
            assert_eq!(voice.stretch.pending_fifo_frames(), fifo);
            assert!(voice.source_playback.matches_exact(&playback));
            assert_productive_history_unchanged(
                voice.stretch.productive_history().unwrap(),
                history,
            );
            assert_productive_history_unchanged(
                fixture.mixer.dsp_source_history_for_test(0).unwrap(),
                dsp,
            );
        }
        fixture.engine.cancel_pad_launches(0).unwrap();
        let result = execute_scheduled_command(
            &mut fixture.mixer,
            &mut fixture.transport,
            first_frame + 31,
            command,
            &mut fixture.messages,
            &mut ImmediateAudioBufferRetirement,
        );
        assert!(matches!(
            result,
            crate::audio_engine::audio_stream::ScheduledExecution::Complete
        ));
        assert_eq!(
            ticket.publication_status(),
            "rejected",
            "the original explicit cancellation must settle the held owner"
        );
        assert_eq!(fixture.mixer.loop_region_frames(0), loops);
        assert!(fixture.transport.matches_clock_epoch(&timeline));
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .global_rate_status
                .read()
                .unwrap()
                .effect
                .effective,
            tuple
        );
    }
}

#[test]
fn qualified_master_only_after_terminal_unqualified_speed_rejection_uses_actual_speed() {
    let mut fixture = fixed_bpm_reference_fixture();
    fixture.engine.set_pad_timing_intent(0, "manual").unwrap();
    drain_bpm_fixture(&mut fixture);
    fixture.mixer.set_pad_bpm(0, None);
    let current = fixture.binding();
    assert!(current.current() && current.available() && current.binding.accepted.is_none());
    fixture.global_parameter(ControlParameterMessage::SetSpeed(1.16));
    drain_bpm_fixture(&mut fixture);
    let rejected = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert_eq!(
        rejected.effect.state,
        crate::audio_engine::global_rate_status::RateState::Rejected
    );
    assert_eq!(
        rejected.effect.error,
        crate::audio_engine::global_rate_status::RateError::HistoryUnavailable
    );
    assert_eq!(rejected.effect.requested.speed, 1.16);
    assert_eq!(rejected.effect.effective.speed, 1.0);
    assert!(!fixture.mixer.has_pending_global_rate());
    let (_, bpm) = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap();
    assert!(bpm.applied && bpm.effective);
    // Restoring a manual source period has no global-rate observation side effect.
    fixture.mixer.set_pad_bpm(0, Some(80.0));
    let before = fixture.transport;
    qualified_rate(&mut fixture, None, 0.75);
    drain_bpm_fixture(&mut fixture);
    let applied = fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert_eq!(
        applied.effect.state,
        crate::audio_engine::global_rate_status::RateState::Applied
    );
    assert_eq!(applied.effect.requested.speed, 1.0);
    assert_eq!(applied.effect.effective.speed, 1.0);
    assert_eq!(applied.effect.effective.period, Some(0.75));
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.75));
    assert_eq!(fixture.transport.output_frame(), before.output_frame());
    assert_eq!(
        fixture.transport.bootstrap_reference(),
        before.bootstrap_reference()
    );
}

#[test]
fn master_refresh_real_callback_cas_retry_keeps_same_owner_and_applies_at_second_callback_frame() {
    let mut fixture = Fixture::new();
    fixture.global_parameter(ControlParameterMessage::SetMasterPeriod(0.75));
    fixture.controls();
    fixture.parameters();
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.75));
    let before = fixture.state();
    let ticket = fixture.refresh(Some(PERIOD));
    let message = fixture.consumer.pop().unwrap();
    let ControlMessage::RefreshAcceptedTiming(ref payload) = message else {
        panic!("original refresh message required")
    };
    let original = Arc::downgrade(payload);
    fixture.apply(message, &mut ImmediateAudioBufferRetirement);
    fixture.controls();
    fixture.parameters();
    let revision = fixture
        .engine
        .global_timing_revision
        .load(Ordering::Acquire);
    assert!(fixture.mixer.global_drains_claimed(revision));
    let key = fixture
        .binding()
        .binding
        .placement
        .unwrap()
        .content_key()
        .unwrap();
    let control = fixture.mixer.source_control_revision(key).unwrap();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    ownership
        .global_timing_publishing
        .store(true, Ordering::Release);
    fixture.render(2);
    assert_eq!(ticket.publication_status(), "pending");
    assert_eq!(fixture.state(), before);
    assert_eq!(fixture.mixer.source_control_revision(key).unwrap(), control);
    let deferred = fixture
        .scheduler
        .deferred_due()
        .expect("first real callback must retain its original due event");
    let ScheduledCommand::RefreshAcceptedTiming(ref payload) = deferred.command else {
        panic!("same original refresh required")
    };
    assert!(Arc::ptr_eq(payload, &original.upgrade().unwrap()));
    assert_eq!(deferred.execution_frame, 1);
    assert!(fixture.consumer.is_empty() && fixture.mixer.deferred_control().is_none());
    ownership
        .global_timing_publishing
        .store(false, Ordering::Release);
    let retry_frame = fixture.transport.output_frame();
    assert_eq!(retry_frame, 2);
    let beat = fixture
        .transport
        .beat_position_at_frame(retry_frame)
        .unwrap();
    fixture.controls();
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(ticket.is_current());
    assert!(fixture.scheduler.deferred_due().is_none());
    assert_eq!(fixture.transport.output_frame(), retry_frame);
    assert_eq!(fixture.state(), ((1_000, Some(14_000)), Some(PERIOD)));
    assert!(
        (fixture
            .transport
            .beat_position_at_frame(retry_frame)
            .unwrap()
            - beat)
            .abs()
            < 1e-12
    );
    assert!(fixture.mixer.global_drains_claimed(revision));
    assert!(
        original.upgrade().is_none(),
        "only the settled original may retire after the second callback effect"
    );
}

#[test]
fn master_refresh_deferred_retry_cannot_borrow_completed_drain_when_same_revision_ping_is_unread() {
    let mut fixture = Fixture::new();
    fixture.global_parameter(ControlParameterMessage::SetMasterPeriod(0.75));
    fixture.controls();
    fixture.parameters();
    let before = fixture.state();
    let ticket = fixture.refresh(Some(PERIOD));
    fixture.controls();
    fixture.parameters();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let revision = fixture
        .engine
        .global_timing_revision
        .load(Ordering::Acquire);
    assert!(fixture.mixer.global_drains_claimed(revision));
    ownership
        .global_timing_publishing
        .store(true, Ordering::Release);
    fixture.render(2);
    assert_eq!(ticket.publication_status(), "pending");
    assert!(fixture.scheduler.deferred_due().is_some());
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert_eq!(
        fixture
            .engine
            .global_timing_revision
            .load(Ordering::Acquire),
        revision
    );
    assert!(!fixture.consumer.is_empty());
    ownership
        .global_timing_publishing
        .store(false, Ordering::Release);
    assert_eq!(
        fixture.controls(),
        1,
        "the unread original Ping must progress after the stale drain basis rejects refresh"
    );
    assert_eq!(ticket.publication_status(), "rejected");
    assert!(!ticket.is_current());
    assert_eq!(fixture.state(), before);
    assert!(fixture.consumer.is_empty());
    assert!(fixture.mixer.deferred_control().is_none());
    assert!(fixture.scheduler.deferred_due().is_none());
    assert!(fixture.mixer.global_drains_claimed(revision));
}

#[test]
fn master_refresh_competing_rate_rejects_only_refresh_and_same_foreign_receipt_later_applies() {
    use crate::audio_engine::global_rate_status::{RateError, RateState};
    let mut fixture = Fixture::new();
    let before = fixture.state();
    fixture.global_parameter(ControlParameterMessage::SetMasterPeriod(0.75));
    let ticket = fixture.refresh(Some(PERIOD));
    fixture.controls();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    ownership
        .global_timing_publishing
        .store(true, Ordering::Release);
    fixture.parameters();
    let preparing = ownership.global_rate_status.read().unwrap();
    assert_eq!(preparing.effect.state, RateState::Preparing);
    assert_eq!(preparing.effect.error, RateError::None);
    assert_eq!(preparing.effect.requested.period, Some(0.75));
    assert!(fixture.mixer.has_pending_global_rate());
    fixture.render(2);
    assert_eq!(ticket.publication_status(), "rejected");
    assert!(!ticket.is_current());
    assert_eq!(fixture.state(), before);
    assert!(fixture.scheduler.deferred_due().is_none());
    let held = ownership.global_rate_status.read().unwrap();
    assert_eq!(held.effect.revision, preparing.effect.revision);
    assert_eq!(held.effect.state, RateState::Preparing);
    assert_eq!(held.effect.error, RateError::None);
    assert_eq!(held.effect.effective, preparing.effect.effective);
    assert!(fixture.mixer.has_pending_global_rate());
    ownership
        .global_timing_publishing
        .store(false, Ordering::Release);
    fixture.controls();
    fixture.parameters();
    fixture.render(0);
    let applied = ownership.global_rate_status.read().unwrap();
    assert_eq!(applied.request_revision, preparing.request_revision);
    assert_eq!(applied.effect.revision, preparing.effect.revision);
    assert_eq!(applied.effect.state, RateState::Applied);
    assert_eq!(applied.effect.error, RateError::None);
    assert_eq!(applied.effect.execution_frame, Some(2));
    assert_eq!(applied.effect.effective.period, Some(0.75));
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.75));
    assert_eq!(fixture.mixer.loop_region_frames(0), before.0);
    assert!(!fixture.mixer.has_pending_global_rate());
    assert_eq!(ticket.publication_status(), "rejected");
}
