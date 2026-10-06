//! Exact productive callback drains, independently of a live audio device.

use super::*;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::constant_timing::{AcceptedTimingProjection, PreparedConstantTiming};
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::source_grid::SourceGrid;
use crate::messages::{PadTimingMetadata, PreparedStemSet, SampleBuffer};
use std::sync::atomic::{AtomicU64, Ordering};

const RATE: u32 = 8_000;
const PRECISE_PERIOD: f64 = 60.0 / 119.999;
const ORIGIN: f64 = -0.125_012_3;

fn sample() -> SampleBuffer {
    SampleBuffer {
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
    mixer: RtMixer,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    quantization: TriggerQuantization,
    messages: Vec<AudioMessage>,
}

impl CallbackState {
    fn new(source: SampleBuffer) -> Self {
        let mut mixer = RtMixer::new(1, RATE as f32);
        mixer.load_sample(0, source);
        mixer.set_pad_bpm(0, Some(120.0));
        Self {
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
}
impl AudioBufferRetirement for LimitedRetirement {
    fn retire_sample(&mut self, _: SampleBuffer) {}
    fn retire_prepared_stems(&mut self, _: PreparedStemSet) {}
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
    };
    assert_eq!(state.drain(&mut consumer, &mut retirement), 0);
    assert_eq!(permit.status(), "pending");
    assert!(consumer.peek().is_ok());
    retirement.slots = 1;
    assert_eq!(state.drain(&mut consumer, &mut retirement), 1);
    assert_eq!(permit.status(), "accepted");
    assert_eq!(retirement.retired.len(), 1);
    assert_eq!(retirement.retired[0].projection.revision, [0x42; 32]);
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
