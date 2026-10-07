//! Exact productive callback drains, independently of a live audio device.

use super::*;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::constant_timing::{
    AcceptedTimingProjection, CurrentTimingAcknowledgements, PreparedConstantTiming,
};
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::source_grid::SourceGrid;
use crate::messages::{PadTimingMetadata, PreparedStemSet, SampleBuffer};
use std::sync::atomic::{AtomicU64, Ordering};

const RATE: u32 = 8_000;
const PRECISE_PERIOD: f64 = 60.0 / 119.999;
const ORIGIN: f64 = -0.125_012_3;

fn productive_source(value: f32) -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(
            (0..RATE)
                .map(|frame| value * (frame as f32 * 0.13).sin())
                .collect::<Vec<_>>(),
        ),
    }
}

fn productive_voice(state: &CallbackState) -> &crate::audio_engine::voice_slot::VoiceSlot {
    state
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

#[test]
fn productive_history_native_loaded_source_publication_fences_new_launch_during_missing_and_pending_cache()
 {
    use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
    use crate::audio_engine::{LoadedSourcePublication, publish_loaded_sample};
    use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
    let old_source = productive_source(0.5);
    let new_source = productive_source(0.2);
    let other_source = productive_source(0.4);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let cache = Arc::new(std::sync::Mutex::new(vec![None; NUM_SAMPLES]));
    let (producer, mut consumer) = RingBuffer::new(4);
    let producer = Arc::new(std::sync::Mutex::new(producer));
    let mut generation = (0, 0);
    let mut digest = None;
    let mut state = CallbackState::new(old_source.clone());
    state.mixer.set_input_runtime_ownership(ownership.clone());
    assert!(!state.mixer.can_play_sample(0, 1.0)); // tracked engine starts unavailable
    publish_loaded_sample(
        &producer,
        &cache,
        0,
        old_source.clone(),
        LoadedSourcePublication {
            cold: false,
            cold_epoch: None,
            cold_adoption: None,
            replace_assignment: false,
            loop_region: None,
            resident_cancelled: None,
            intent: None,
            ownership: &ownership,
            generation: 1,
            rate: RATE,
            generation_slot: &mut generation,
            digest_slot: &mut digest,
            digest: "a".repeat(64),
        },
    )
    .unwrap();
    assert_eq!(generation, (1, RATE));
    assert!(ownership.source_current(0, &old_source, RATE));
    assert!(!ownership.source_current(0, &old_source, RATE + 1));
    assert_eq!(
        state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement),
        1
    );
    let epoch = Arc::new(AtomicU64::new(2));
    let (accepted, _) = publication(&old_source, &epoch, 2);
    producer.lock().unwrap().push(accepted).unwrap();
    state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement);
    ownership.set_timing_intent(0, TimingIntent::Automatic);
    state.mixer.set_speed(1.37);
    state.mixer.set_pad_key_lock(0, true);
    assert!(state.mixer.play_sample(0, 1.0));
    let mut other_generation = (0, 0);
    let mut other_digest = None;
    publish_loaded_sample(
        &producer,
        &cache,
        1,
        other_source,
        LoadedSourcePublication {
            cold: false,
            cold_epoch: None,
            cold_adoption: None,
            replace_assignment: false,
            loop_region: None,
            resident_cancelled: None,
            intent: None,
            ownership: &ownership,
            generation: 1,
            rate: RATE,
            generation_slot: &mut other_generation,
            digest_slot: &mut other_digest,
            digest: "c".repeat(64),
        },
    )
    .unwrap();
    state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement);
    assert!(state.mixer.play_sample(1, 1.0));
    state
        .mixer
        .render(&mut vec![0.0; 1657], &mut [0.0; NUM_SAMPLES]);
    let history = productive_voice(&state)
        .stretch
        .productive_history()
        .unwrap();
    let native = productive_voice(&state).stretch.native_state_address();

    // Exactly the native load-request transaction: fence before losing the control-cache pin.
    ownership.revoke_source(0);
    cache.lock().unwrap()[0] = None;
    assert_eq!(state.acknowledgements.current_epoch(0), 2); // old effective timing/audio survives
    assert!(!state.mixer.play_sample(0, 1.0));
    let launch = ScheduledCommand::StopAllThenPlaySample {
        id: 0,
        volume: 1.0,
        received_at_ns: None,
    };
    execute_scheduled_command(
        &mut state.mixer,
        &mut state.transport,
        2,
        launch.clone(),
        &mut state.messages,
        &mut ImmediateAudioBufferRetirement,
    );
    assert!(
        state
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(1))
    );
    assert_eq!(
        productive_voice(&state)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames,
        history.fed_output_frames
    );
    state.mixer.render(&mut [0.0; 37], &mut [0.0; NUM_SAMPLES]);
    assert_eq!(
        productive_voice(&state)
            .stretch
            .productive_history()
            .unwrap()
            .binding,
        history.binding
    );

    // Real native load publication completes control ownership BEFORE callback bank adoption.
    publish_loaded_sample(
        &producer,
        &cache,
        0,
        new_source.clone(),
        LoadedSourcePublication {
            cold: false,
            cold_epoch: None,
            cold_adoption: None,
            replace_assignment: false,
            loop_region: None,
            resident_cancelled: None,
            intent: None,
            ownership: &ownership,
            generation: 3,
            rate: RATE,
            generation_slot: &mut generation,
            digest_slot: &mut digest,
            digest: "b".repeat(64),
        },
    )
    .unwrap();
    assert_eq!(generation, (3, RATE));
    assert_eq!(digest, Some("b".repeat(64)));
    assert!(!state.mixer.play_sample(0, 1.0)); // new control pin cannot authorize old callback PCM
    let mut blocked = LimitedRetirement {
        slots: 1,
        retired: Vec::new(),
        stems: Vec::new(),
    };
    assert_eq!(state.drain(&mut consumer, &mut blocked), 0);
    blocked.slots = MAX_VOICES + 2;
    assert_eq!(state.drain(&mut consumer, &mut blocked), 1);
    assert!(!state.mixer.play_sample(0, 1.0)); // Automatic requires fresh new-source acceptance
    state.mixer.render(&mut [0.0; 37], &mut [0.0; NUM_SAMPLES]);
    assert_eq!(
        productive_voice(&state)
            .stretch
            .productive_history()
            .unwrap()
            .binding,
        history.binding
    );
    assert_eq!(
        productive_voice(&state).stretch.native_state_address(),
        native
    );
    ownership.set_timing_intent(0, TimingIntent::Legacy);
    assert!(state.mixer.play_sample(0, 1.0));
    assert!(Arc::ptr_eq(
        &productive_voice(&state).sample.as_ref().unwrap().samples,
        &new_source.samples
    ));
    state.mixer.render(&mut [0.0; 37], &mut [0.0; NUM_SAMPLES]);
    assert_eq!(
        productive_voice(&state)
            .stretch
            .productive_history()
            .unwrap()
            .binding
            .accepted,
        None
    );
    assert_eq!(
        productive_voice(&state)
            .stretch
            .productive_history()
            .unwrap()
            .binding
            .source_address,
        new_source.samples.as_ptr() as usize
    );
}

#[test]
fn productive_history_callback_pending_rejected_and_same_drain_source_replace_keep_real_history_owned()
 {
    let source = productive_source(0.5);
    let epoch = Arc::new(AtomicU64::new(2));
    let (initial, initial_permit) = publication(&source, &epoch, 2);
    let (mut producer, mut consumer) = RingBuffer::new(3);
    producer.push(initial).unwrap();
    let mut state = CallbackState::new(source.clone());
    state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement);
    assert_eq!(initial_permit.status(), "accepted");
    state.mixer.set_speed(1.37);
    state.mixer.set_pad_key_lock(0, true);
    assert!(state.mixer.play_sample(0, 1.0));
    state
        .mixer
        .render(&mut vec![0.0; 1657], &mut [0.0; NUM_SAMPLES]);
    let first = productive_voice(&state)
        .stretch
        .productive_history()
        .unwrap();
    let native = productive_voice(&state).stretch.native_state_address();
    epoch.store(3, Ordering::Release);
    let (pending, pending_permit) = publication(&source, &epoch, 3);
    producer.push(pending).unwrap();
    let mut blocked = LimitedRetirement {
        slots: 0,
        retired: Vec::new(),
        stems: Vec::new(),
    };
    assert_eq!(state.drain(&mut consumer, &mut blocked), 0);
    assert_eq!(pending_permit.status(), "pending");
    state.mixer.render(&mut [0.0; 37], &mut [0.0; NUM_SAMPLES]);
    assert_eq!(
        productive_voice(&state)
            .stretch
            .productive_history()
            .unwrap()
            .binding,
        first.binding
    );
    epoch.store(4, Ordering::Release);
    blocked.slots = 1;
    assert_eq!(state.drain(&mut consumer, &mut blocked), 1);
    assert_eq!(pending_permit.status(), "rejected");
    let (mut latest, latest_permit) = publication(&source, &epoch, 4);
    let ControlMessage::PublishConstantTiming { timing, .. } = &mut latest else {
        unreachable!()
    };
    timing.projection.origin_seconds = -0.0;
    timing.projection.revision = [0x44; 32];
    let latest_projection = timing.projection;
    producer.push(latest).unwrap();
    producer
        .push(ControlMessage::LoadSample {
            id: 0,
            sample: productive_source(0.2),
        })
        .unwrap();
    state.mixer.pause_sample(0);
    assert_eq!(
        state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement),
        2
    );
    assert_eq!(latest_permit.status(), "accepted");
    state.mixer.resume_sample(0);
    state.mixer.render(&mut [0.0; 37], &mut [0.0; NUM_SAMPLES]);
    let history = productive_voice(&state)
        .stretch
        .productive_history()
        .unwrap();
    assert_eq!(history.binding.accepted, Some(latest_projection));
    assert_eq!(
        history.binding.source_address,
        source.samples.as_ptr() as usize
    );
    assert_eq!(history.fed_output_frames, first.fed_output_frames + 74);
    assert_eq!(
        productive_voice(&state).stretch.native_state_address(),
        native
    );
}

struct AuthorityRaceRetirement {
    ownership: Arc<crate::audio_engine::input_runtime_binding::InputRuntimeOwnership>,
    calls: usize,
    revoke_at_preflight: Option<usize>,
    revoke_at_retire: bool,
    samples: Vec<SampleBuffer>,
    slots: usize,
}

impl AudioBufferRetirement for AuthorityRaceRetirement {
    fn retire_resident_capture(
        &mut self,
        _: std::sync::Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: std::sync::Arc<std::sync::atomic::AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<super::super::global_playback_batch::GlobalPlaybackBatch>,
    ) {
    }
    fn retire_sample(&mut self, source: SampleBuffer) {
        if self.revoke_at_retire {
            self.ownership.set_timing_intent(
                0,
                flitzis_looper_analysis::tempo_acceptance::TimingIntent::Automatic,
            );
        }
        self.samples.push(source);
    }
    fn retire_prepared_stems(&mut self, _: PreparedStemSet) {
        unreachable!()
    }
    fn retire_constant_timing(&mut self, _: PreparedConstantTiming) {
        unreachable!()
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.calls += 1;
        if self.revoke_at_preflight == Some(self.calls) {
            self.ownership.set_timing_intent(
                0,
                flitzis_looper_analysis::tempo_acceptance::TimingIntent::Automatic,
            );
        }
        self.slots
    }
}

#[test]
fn productive_history_exclusive_preflight_failure_preserves_loop_and_other_audio_and_rechecks_retirement()
 {
    use crate::audio_engine::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
    let original = productive_source(0.5);
    let replacement = productive_source(0.2);
    let mut state = CallbackState::new(original.clone());
    let ownership = Arc::new(InputRuntimeOwnership::default());
    state.mixer.set_input_runtime_ownership(ownership.clone());
    state.mixer.set_speed(1.37);
    state.mixer.set_pad_key_lock(0, true);
    assert!(state.mixer.play_sample(0, 1.0));
    state
        .mixer
        .render(&mut vec![0.0; 1657], &mut [0.0; NUM_SAMPLES]);
    state.mixer.load_sample(0, replacement.clone());
    state.mixer.load_sample(1, productive_source(0.4));
    assert!(state.mixer.play_sample(1, 1.0));
    let history = productive_voice(&state)
        .stretch
        .productive_history()
        .unwrap();
    let frame = productive_voice(&state).frame_pos;
    let launch = ScheduledCommand::TriggerInputPad {
        id: 0,
        start_s: 0.2,
        end_s: Some(0.7),
        exclusive: true,
        binding: InputPadBinding {
            resident: None,
            source_address: replacement.samples.as_ptr() as usize,
            sample_count: replacement.samples.len(),
            channels: 1,
            sample_rate_hz: RATE,
            authority_revision: 1,
            runtime_revision: 0,
            accepted: None,
        },
        received_at_ns: 0,
        resident_control: None,
        launch_revision: 0,
    };
    let mut retirement = AuthorityRaceRetirement {
        ownership,
        calls: 0,
        revoke_at_preflight: None,
        revoke_at_retire: false,
        samples: Vec::new(),
        slots: 0,
    };
    execute_scheduled_command(
        &mut state.mixer,
        &mut state.transport,
        2,
        launch.clone(),
        &mut state.messages,
        &mut retirement,
    );
    assert_eq!(productive_voice(&state).frame_pos, frame);
    assert!(
        state
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(1))
    );
    retirement.slots = MAX_VOICES;
    retirement.calls = 0;
    retirement.revoke_at_preflight = Some(2); // after guard but before final admitted source snapshot
    execute_scheduled_command(
        &mut state.mixer,
        &mut state.transport,
        3,
        launch,
        &mut state.messages,
        &mut retirement,
    );
    assert_eq!(
        productive_voice(&state)
            .stretch
            .productive_history()
            .unwrap()
            .binding,
        history.binding
    );
    assert_eq!(productive_voice(&state).frame_pos, frame);
    assert_eq!(
        state
            .mixer
            .phase_aligned_initial_sample_frame(0, RATE as usize, 0.0),
        0
    );
    assert!(
        state
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(1))
    );
    assert!(retirement.samples.is_empty());
    assert!(state.messages.is_empty());
}

#[test]
fn productive_history_exclusive_commit_uses_admitted_source_when_authority_changes_during_stop() {
    use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
    let source = productive_source(0.5);
    let mut state = CallbackState::new(source.clone());
    let ownership = Arc::new(InputRuntimeOwnership::default());
    state.mixer.set_input_runtime_ownership(ownership.clone());
    state.mixer.load_sample(1, productive_source(0.4));
    assert!(state.mixer.play_sample(1, 1.0));
    let mut retirement = AuthorityRaceRetirement {
        ownership,
        calls: 0,
        revoke_at_preflight: None,
        revoke_at_retire: true,
        samples: Vec::new(),
        slots: 2 * MAX_VOICES,
    };
    execute_scheduled_command(
        &mut state.mixer,
        &mut state.transport,
        3,
        ScheduledCommand::StopAllThenPlaySample {
            id: 0,
            volume: 1.0,
            received_at_ns: None,
        },
        &mut state.messages,
        &mut retirement,
    );
    assert_eq!(retirement.samples.len(), 1);
    assert!(Arc::ptr_eq(
        &productive_voice(&state).sample.as_ref().unwrap().samples,
        &source.samples
    ));
    assert!(
        state
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.active || voice.sample_id == 0)
    );
    assert!(!state.mixer.can_play_sample(0, 1.0)); // later admission sees unavailable Automatic
    state.mixer.set_speed(1.37);
    state.mixer.set_pad_key_lock(0, true);
    state.mixer.render(&mut [0.0; 37], &mut [0.0; NUM_SAMPLES]);
    assert_eq!(
        productive_voice(&state)
            .stretch
            .productive_history()
            .unwrap()
            .binding
            .accepted,
        None
    );
}

fn sample() -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(vec![0.0_f32; RATE as usize]),
    }
}

fn publication(
    source: &SampleBuffer,
    epoch: &Arc<AtomicU64>,
    expected: u64,
) -> (ControlMessage, PreparedSourcePermit) {
    let permit = PreparedSourcePermit::for_epoch(epoch.clone(), expected);
    permit.mark_pending().unwrap();
    (
        ControlMessage::PublishConstantTiming {
            id: 0,
            timing: PreparedConstantTiming {
                reference: source.clone(),
                publication: permit.clone(),
                projection: AcceptedTimingProjection {
                    revision: [0x42; 32],
                    period_seconds: PRECISE_PERIOD,
                    origin_seconds: ORIGIN,
                    sample_rate_hz: RATE,
                    publication_epoch: expected,
                },
            },
        },
        permit,
    )
}

struct CallbackState {
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    mixer: RtMixer,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    quantization: TriggerQuantization,
    messages: Vec<AudioMessage>,
}

impl CallbackState {
    fn new(source: SampleBuffer) -> Self {
        let mut mixer = RtMixer::new(1, RATE as f32);
        let acknowledgements = Arc::default();
        mixer.set_current_timing_acknowledgements(Arc::clone(&acknowledgements));
        mixer.load_sample(0, source);
        mixer.set_pad_bpm(0, Some(120.0));
        Self {
            acknowledgements,
            mixer,
            scheduler: FixedCapacityScheduler::new(),
            transport: TransportTimeline::new(RATE),
            quantization: TriggerQuantization::Immediate,
            messages: Vec::new(),
        }
    }

    fn drain(
        &mut self,
        consumer: &mut Consumer<ControlMessage>,
        retirement: &mut impl AudioBufferRetirement,
    ) -> usize {
        drain_control_messages(
            consumer,
            &mut self.scheduler,
            0,
            &mut self.quantization,
            &mut self.transport,
            &mut self.mixer,
            &mut self.messages,
            retirement,
        )
    }
}

#[test]
fn constant_timing_actual_callback_drains_protect_newer_grid_from_old_parameter_and_preserve_playhead_loop()
 {
    let source = sample();
    let epoch = Arc::new(AtomicU64::new(2));
    let (message, permit) = publication(&source, &epoch, 2);
    let (mut ordered, mut commands) = RingBuffer::new(1);
    ordered.push(message).unwrap();
    let (mut parameters, mut parameter_messages) = RingBuffer::new(2);
    parameters
        .push(ControlParameterMessage::SetLegacyPadBpm {
            id: 0,
            bpm: Some(60.0),
            through_epoch: 1,
        })
        .unwrap();
    let mut state = CallbackState::new(source);
    state.mixer.set_pad_loop_region(0, 0.1, Some(0.8));
    assert!(state.mixer.play_sample(0, 1.0));
    state.mixer.render(&mut [0.0; 37], &mut [0.0; NUM_SAMPLES]);
    let before = state.mixer.pad_playhead_seconds(0);
    assert_eq!(
        state.drain(&mut commands, &mut ImmediateAudioBufferRetirement),
        1
    );
    assert_eq!(permit.status(), "accepted");
    assert_eq!(state.acknowledgements.current_epoch(0), 2);
    assert_eq!(state.mixer.pad_playhead_seconds(0), before);
    let phase = state.mixer.active_pad_beat_position(0);
    drain_parameter_messages(
        &mut parameter_messages,
        &mut state.mixer,
        &mut state.transport,
    );
    assert_eq!(state.mixer.active_pad_beat_position(0), phase);
    assert_eq!(state.mixer.pad_playhead_seconds(0), before);
    let reference =
        SourceGrid::from_period(PRECISE_PERIOD * f64::from(RATE), ORIGIN * f64::from(RATE))
            .unwrap();
    assert_eq!(
        state
            .mixer
            .phase_aligned_initial_sample_frame(0, RATE as usize, 1.5),
        reference.source_at_master_beat(1.5, 800, 6400).unwrap()
    );
    epoch.store(3, Ordering::Release); // new request invalidates pending work, not accepted timing
    assert_eq!(state.mixer.active_pad_beat_position(0), phase);
    parameters
        .push(ControlParameterMessage::SetLegacyPadBpm {
            id: 0,
            bpm: Some(90.0),
            through_epoch: 3,
        })
        .unwrap();
    drain_parameter_messages(
        &mut parameter_messages,
        &mut state.mixer,
        &mut state.transport,
    );
    assert_ne!(state.mixer.active_pad_beat_position(0), phase);
    assert_eq!(state.acknowledgements.current_epoch(0), 0);
    assert_eq!(state.mixer.pad_playhead_seconds(0), before);
}

#[test]
fn constant_timing_legacy_origin_is_one_effect_at_actual_command_budget_boundary() {
    let source = sample();
    let epoch = Arc::new(AtomicU64::new(2));
    let (message, permit) = publication(&source, &epoch, 2);
    let (mut producer, mut consumer) = RingBuffer::new(MAX_CONTROL_MESSAGES_PER_CALLBACK + 2);
    producer.push(message).unwrap();
    let mut state = CallbackState::new(source);
    state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement);
    assert_eq!(permit.status(), "accepted");
    assert!(state.mixer.play_sample(0, 1.0));
    let accepted_phase = state.mixer.active_pad_beat_position(0);
    for _ in 0..MAX_CONTROL_MESSAGES_PER_CALLBACK {
        producer.push(ControlMessage::Ping()).unwrap();
    }
    producer
        .push(ControlMessage::SetLegacyPadTimingMetadata {
            id: 0,
            metadata: PadTimingMetadata {
                phase_anchor_s: 0.25,
            },
            through_epoch: 3,
        })
        .unwrap();
    assert_eq!(
        state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement),
        MAX_CONTROL_MESSAGES_PER_CALLBACK
    );
    assert_eq!(state.mixer.active_pad_beat_position(0), accepted_phase);
    assert_eq!(
        state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement),
        1
    );
    assert_eq!(state.mixer.active_pad_beat_position(0), Some(-0.5));
}

struct LimitedRetirement {
    slots: usize,
    retired: Vec<PreparedConstantTiming>,
    stems: Vec<PreparedStemSet>,
}
impl AudioBufferRetirement for LimitedRetirement {
    fn retire_resident_capture(
        &mut self,
        _: std::sync::Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: std::sync::Arc<std::sync::atomic::AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<super::super::global_playback_batch::GlobalPlaybackBatch>,
    ) {
    }
    fn retire_sample(&mut self, _: SampleBuffer) {}
    fn retire_prepared_stems(&mut self, stems: PreparedStemSet) {
        self.stems.push(stems);
    }
    fn retire_constant_timing(&mut self, timing: PreparedConstantTiming) {
        self.retired.push(timing);
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.slots
    }
}

#[test]
fn constant_timing_actual_callback_retirement_backpressure_keeps_publication_pending_then_accepts()
{
    let source = sample();
    let epoch = Arc::new(AtomicU64::new(2));
    let (message, permit) = publication(&source, &epoch, 2);
    let (mut producer, mut consumer) = RingBuffer::new(1);
    producer.push(message).unwrap();
    let mut state = CallbackState::new(source);
    let mut retirement = LimitedRetirement {
        slots: 0,
        retired: Vec::new(),
        stems: Vec::new(),
    };
    assert_eq!(state.drain(&mut consumer, &mut retirement), 0);
    assert_eq!(permit.status(), "pending");
    assert_eq!(state.acknowledgements.current_epoch(0), 0);
    assert!(consumer.peek().is_ok());
    retirement.slots = 1;
    assert_eq!(state.drain(&mut consumer, &mut retirement), 1);
    assert_eq!(permit.status(), "accepted");
    assert_eq!(state.acknowledgements.current_epoch(0), 2);
    assert_eq!(retirement.retired.len(), 1);
    assert_eq!(retirement.retired[0].projection.revision, [0x42; 32]);
}

#[test]
fn prepared_stems_actual_callback_backpressure_and_stale_revision_preserve_existing_audio() {
    let source = sample();
    let epoch = Arc::new(AtomicU64::new(2));
    let (message, _) = publication(&source, &epoch, 2);
    let ControlMessage::PublishConstantTiming { timing, .. } = &message else {
        unreachable!()
    };
    let projected = timing.projection;
    let (mut producer, mut consumer) = RingBuffer::new(2);
    producer.push(message).unwrap();
    let mut state = CallbackState::new(source.clone());
    state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement);
    let prepared = |value, projection| {
        let permit = PreparedSourcePermit::unrestricted();
        permit.mark_pending().unwrap();
        let pcm = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(vec![value; RATE as usize]),
        };
        PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            reference_samples: source.samples.clone(),
            publication: permit,
            accepted_timing: Some(projection),
            source_version_hash: 42,
            sample_rate_hz: RATE,
            channels: 1,
            frame_count: RATE as usize,
            available_mask: 31,
            stems: std::array::from_fn(|_| pcm.clone()),
        }
    };
    let first = prepared(0.125, projected);
    let status = first.publication.clone();
    producer
        .push(ControlMessage::PublishPreparedStems {
            id: 0,
            stems: first,
        })
        .unwrap();
    let mut retirement = LimitedRetirement {
        slots: 1,
        retired: Vec::new(),
        stems: Vec::new(),
    };
    assert_eq!(state.drain(&mut consumer, &mut retirement), 0);
    assert_eq!(status.status(), "pending");
    assert!(consumer.peek().is_ok());
    retirement.slots = 2;
    assert_eq!(state.drain(&mut consumer, &mut retirement), 1);
    assert_eq!(status.status(), "accepted");
    assert!(
        state
            .mixer
            .set_stem_mix_mode(0, crate::messages::StemMixMode::AllStems, 42)
    );
    let mut stale = projected;
    stale.revision = [0x43; 32]; // endpoint/period/origin/epoch equality is insufficient
    let replacement = prepared(0.25, stale);
    let rejected = replacement.publication.clone();
    let rejected_pcm = replacement.stems[0].samples.clone();
    producer
        .push(ControlMessage::PublishPreparedStems {
            id: 0,
            stems: replacement,
        })
        .unwrap();
    assert_eq!(state.drain(&mut consumer, &mut retirement), 1);
    assert_eq!(rejected.status(), "rejected");
    assert_eq!(retirement.stems.len(), 1);
    assert!(Arc::ptr_eq(
        &retirement.stems[0].stems[0].samples,
        &rejected_pcm
    ));
    assert_eq!(
        state.acknowledgements.current_epoch(0),
        projected.publication_epoch
    );
    assert!(state.mixer.play_sample(0, 1.0));
    let mut output = [0.0; 31];
    state.mixer.render(&mut output, &mut [0.0; NUM_SAMPLES]);
    assert!(output.iter().all(|value| *value == 0.5));
}

#[test]
fn constant_timing_parameter_coalescing_retains_latest_clear_epoch() {
    let source = sample();
    let epoch = Arc::new(AtomicU64::new(2));
    let (message, _) = publication(&source, &epoch, 2);
    let (mut ordered, mut commands) = RingBuffer::new(1);
    ordered.push(message).unwrap();
    let mut state = CallbackState::new(source);
    state.drain(&mut commands, &mut ImmediateAudioBufferRetirement);
    assert!(state.mixer.play_sample(0, 1.0));
    let (mut producer, mut consumer) = RingBuffer::new(2);
    producer
        .push(ControlParameterMessage::SetLegacyPadBpm {
            id: 0,
            bpm: Some(60.0),
            through_epoch: 1,
        })
        .unwrap();
    producer
        .push(ControlParameterMessage::SetLegacyPadBpm {
            id: 0,
            bpm: Some(90.0),
            through_epoch: 3,
        })
        .unwrap();
    let result = drain_parameter_messages(&mut consumer, &mut state.mixer, &mut state.transport);
    assert_eq!(result.messages_drained, 2);
    assert_eq!(result.parameters_applied, 1);
    assert_eq!(state.mixer.active_pad_beat_position(0), Some(0.0));
    assert_eq!(state.mixer.output_bpm_for_sample_id(0), Some(90.0));
}

#[test]
fn acknowledged_accepted_period_drives_native_bootstrap_reference_and_fractional_bpmlock_trajectory()
 {
    let source = sample();
    let epoch = Arc::new(AtomicU64::new(2));
    let (message, permit) = publication(&source, &epoch, 2);
    let (mut producer, mut consumer) = RingBuffer::new(2);
    let mut state = CallbackState::new(source.clone());
    // Legacy scalar intent is deliberately numerically incompatible with acceptance.
    state.mixer.set_pad_bpm(0, Some(90.0));
    let speed = 1.123_456_789_012_345;
    state.mixer.set_speed(speed);
    producer.push(message).unwrap();
    state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement);
    assert_eq!(permit.status(), "accepted");
    let output_period = PRECISE_PERIOD / speed;
    assert_eq!(
        state.mixer.transport_reference_period_for_sample_id(0),
        Some(output_period)
    );
    assert_eq!(
        state.mixer.output_bpm_for_sample_id(0),
        Some(60.0 / output_period)
    );
    assert!(state.mixer.play_sample(0, 1.0));
    state.transport.request_bootstrap(0);
    assert!(try_bootstrap_transport_from_reference(
        &state.mixer,
        &mut state.transport,
        0
    ));
    assert_eq!(state.transport.master_period_seconds(), Some(output_period));
    assert_eq!(state.transport.master_bpm(), Some(60.0 / output_period));
    assert_eq!(
        state.transport.frames_per_beat(),
        Some(f64::from(RATE) * output_period)
    );
    let binary32_roundtrip_period = 60.0 / f64::from((60.0 / output_period) as f32);
    assert_ne!(
        state.transport.frames_per_beat(),
        Some(f64::from(RATE) * binary32_roundtrip_period)
    );

    // Explicit phase anchoring consumes the same native period as bootstrap.
    assert!(anchor_transport_phase_from_pad_at_frame(
        &state.mixer,
        &mut state.transport,
        0,
        431
    ));
    assert_eq!(state.transport.master_period_seconds(), Some(output_period));
    assert_eq!(
        state.transport.beat_position_at_frame(431),
        state.mixer.active_pad_beat_position(0)
    );

    // BPMLOCK derives source-period/output-period once in binary64 at start/render.
    state.mixer.stop_sample(0);
    let master_bpm = 129.987_654_321_098_76;
    let master_period = 60.0 / master_bpm;
    let ratio = PRECISE_PERIOD / master_period;
    assert_ne!(ratio, f64::from(ratio as f32));
    state.mixer.set_master_bpm(master_bpm);
    state.mixer.set_bpm_lock(true);
    assert_eq!(
        state.mixer.transport_reference_period_for_sample_id(0),
        Some(master_period)
    );
    assert!(state.mixer.play_sample(0, 1.0));
    state
        .mixer
        .render_at_output_frame(0, &mut [0.0; 123], &mut [0.0; NUM_SAMPLES]);
    let source_position =
        state.mixer.active_pad_beat_position(0).unwrap() * PRECISE_PERIOD * f64::from(RATE)
            + ORIGIN * f64::from(RATE);
    assert!((source_position - 123.0 * ratio).abs() < 1.0e-10);
    assert!((source_position - 123.0 * f64::from(ratio as f32)).abs() > 1.0e-6);

    // A new accepted projection never rounds/rebases the active fractional epoch.
    epoch.store(3, Ordering::Release);
    let (mut replacement, replacement_permit) = publication(&source, &epoch, 3);
    let replacement_period = PRECISE_PERIOD * 1.000_1;
    if let ControlMessage::PublishConstantTiming { timing, .. } = &mut replacement {
        timing.projection.period_seconds = replacement_period;
        timing.projection.revision = [0x43; 32];
    }
    producer.push(replacement).unwrap();
    state.drain(&mut consumer, &mut ImmediateAudioBufferRetirement);
    assert_eq!(replacement_permit.status(), "accepted");
    let preserved_position =
        state.mixer.active_pad_beat_position(0).unwrap() * replacement_period * f64::from(RATE)
            + ORIGIN * f64::from(RATE);
    assert!((preserved_position - source_position).abs() < 1.0e-10);
    assert_eq!(state.acknowledgements.current_epoch(0), 3);
}
