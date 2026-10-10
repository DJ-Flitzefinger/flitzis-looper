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

struct Fixture {
    engine: AudioEngine,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    parameter_producer: rtrb::Producer<ControlParameterMessage>,
    parameter_consumer: rtrb::Consumer<ControlParameterMessage>,
    mixer: RtMixer,
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
                .publish_source(0, &sample, RATE, 7);
            engine.sample_cache.lock().unwrap()[0] = Some(sample);
        }
        let mut mixer = acknowledged_mixer(&engine);
        mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
        let (producer, consumer) = queue(128);
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
        super::super::super::push_global_timing_message(
            &self.engine.global_timing_revision,
            &mut self.parameter_producer,
            message,
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
            .publish_source(0, &replacement, RATE, 7);
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
    fixture.process_next();
    assert_eq!(fixture.state(), before);
    assert_eq!(fixture.scheduler.peek_next_target_frame(), Some(1));
    // A subsequent immediate control drain cannot execute the deferred effect at frame zero.
    fixture.apply(ControlMessage::Ping(), &mut ImmediateAudioBufferRetirement);
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
        .publish_source(1, &other, RATE, 1);
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
    let mut fixture = Fixture::new();
    for _ in 0..65 {
        fixture.global_parameter(ControlParameterMessage::SetMasterPeriod(0.75));
    }
    let before_loop = fixture.mixer.loop_region_frames(0);
    let ticket = fixture.refresh(Some(PERIOD));
    fixture.process_next();
    fixture.parameters();
    assert!(!fixture.parameter_consumer.is_empty());
    fixture.render(2);
    assert_eq!(ticket.publication_status(), "rejected");
    assert_eq!(fixture.mixer.loop_region_frames(0), before_loop);
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.75));
    fixture.parameters();
    let retry = fixture.refresh(Some(PERIOD));
    fixture.process_next();
    fixture.parameters();
    fixture.render(2);
    assert!(retry.is_current());
    assert_eq!(fixture.transport.master_period_seconds(), Some(PERIOD));
    fixture.parameters();
    fixture.render(2);
    assert_eq!(fixture.transport.master_period_seconds(), Some(PERIOD));
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
        fixture.process_next();
        fixture.parameters();
        fixture.render(2);
        assert_eq!(ticket.publication_status(), "rejected");
        assert_eq!(fixture.state(), before);
    }
    let mut fixture = Fixture::new();
    fixture.global_parameter(ControlParameterMessage::SetSpeed(1.25));
    let ticket = fixture.refresh(Some(PERIOD / 1.25));
    fixture.process_next();
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
            .publish_source(0, &replacement, RATE, 7);
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
    fixture.engine.input_runtime_ownership.authority[0].store(u64::MAX, Ordering::Release);
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
    execute_scheduled_command(
        &mut fixture.mixer,
        &mut fixture.transport,
        1,
        event.command,
        &mut fixture.messages,
        &mut retirement,
    );
    assert_eq!(ticket.publication_status(), "rejected");
    assert_eq!(fixture.state(), before);
    assert!(weak.upgrade().is_some());
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
