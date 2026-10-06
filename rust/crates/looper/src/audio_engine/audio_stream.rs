//! Audio Stream Module
//!
//! This module handles CPAL audio stream management including:
//! - Stream initialization and configuration
//! - Audio callback setup
//! - Real-time message processing
//! - Error handling for audio stream operations

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Stream, StreamConfig};
use env_logger::{Builder, Env};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{Arc, Mutex};

use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirement, AudioBufferRetirementWorker, create_audio_buffer_retirement,
};
use crate::audio_engine::constants::{MAX_VOICES, NUM_SAMPLES};
use crate::audio_engine::mixer::{RtMixer, RtRenderPadActivity};
use crate::audio_engine::scheduler::{
    FixedCapacityScheduler, ScheduledCommand, TransportScheduler,
};
use crate::audio_engine::timing::{
    InputClock, OutputClockMapper, OutputClockSnapshot, SharedOutputClock,
};
use crate::audio_engine::transport::{QuantizeGrid, TransportTimeline};
use crate::messages::{AudioMessage, ControlMessage, ControlParameterMessage, TriggerQuantization};

pub(crate) const MAX_CONTROL_MESSAGES_PER_CALLBACK: usize = 64;
pub(crate) const MAX_PARAMETER_MESSAGES_PER_CALLBACK: usize = 64;

/// Handle to the audio stream with associated message channels
pub struct AudioStreamHandle {
    pub stream: Stream,
    _retirement_worker: AudioBufferRetirementWorker,
    pub producer: Arc<Mutex<Producer<ControlMessage>>>,
    pub(crate) parameter_producer: Arc<Mutex<Producer<ControlParameterMessage>>>,
    pub consumer: Arc<Mutex<Consumer<AudioMessage>>>,
    pub output_channels: usize,
    pub output_sample_rate: u32,
    pub(crate) output_clock: Arc<SharedOutputClock>,
}

/// Setup and configure the logger for audio operations
pub fn setup_logger() {
    // Default to `info` to avoid extremely expensive debug/trace logging during analysis.
    // Users can override via `RUST_LOG`, e.g. `RUST_LOG=debug` when troubleshooting.
    Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .try_init()
        .unwrap_or(()); // Ignore initialization errors
}

trait AudioMessageSink {
    fn push_audio_message(&mut self, message: AudioMessage);
}

impl AudioMessageSink for Producer<AudioMessage> {
    fn push_audio_message(&mut self, message: AudioMessage) {
        let _ = self.push(message);
    }
}

fn schedule_immediate_command<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    command: ScheduledCommand,
    mixer: &mut RtMixer,
    transport: &mut TransportTimeline,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    if scheduler.schedule(callback_start_frame, command).is_ok() {
        drain_scheduler_due_at_callback_start(
            scheduler,
            callback_start_frame,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
    } else {
        execute_scheduled_command(
            mixer,
            transport,
            callback_start_frame,
            command,
            audio_messages,
            retirement,
        );
    }
}

// Keep callback hot-path state borrows explicit instead of hiding them in a context struct.
#[allow(clippy::too_many_arguments)]
fn schedule_play_sample_command<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: TriggerQuantization,
    transport: &mut TransportTimeline,
    id: usize,
    volume: f32,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
    received_at_ns: Option<u64>,
) {
    let command = ScheduledCommand::PlaySample {
        id,
        volume,
        received_at_ns,
    };

    let Some(target_frame) = quantized_target_frame(transport, trigger_quantization) else {
        schedule_immediate_command(
            scheduler,
            callback_start_frame,
            command,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
        return;
    };

    if scheduler.schedule(target_frame, command).is_ok() {
        drain_scheduler_due_at_callback_start(
            scheduler,
            callback_start_frame,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
    }
}

// Keep callback hot-path state borrows explicit instead of hiding them in a context struct.
#[allow(clippy::too_many_arguments)]
fn schedule_exclusive_play_sample_command<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: TriggerQuantization,
    transport: &mut TransportTimeline,
    id: usize,
    volume: f32,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
    received_at_ns: Option<u64>,
) {
    let command = ScheduledCommand::StopAllThenPlaySample {
        id,
        volume,
        received_at_ns,
    };

    let Some(target_frame) = quantized_target_frame(transport, trigger_quantization) else {
        schedule_immediate_command(
            scheduler,
            callback_start_frame,
            command,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
        return;
    };

    if scheduler.schedule(target_frame, command).is_ok() {
        drain_scheduler_due_at_callback_start(
            scheduler,
            callback_start_frame,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
    }
}

fn quantized_target_frame(
    transport: &TransportTimeline,
    trigger_quantization: TriggerQuantization,
) -> Option<u64> {
    match trigger_quantization {
        TriggerQuantization::Immediate => None,
        TriggerQuantization::Grid { step_64ths } => {
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(step_64ths)?)
        }
    }
}

fn anchor_transport_phase_from_pad(
    mixer: &RtMixer,
    transport: &mut TransportTimeline,
    id: usize,
) -> bool {
    anchor_transport_phase_from_pad_at_frame(mixer, transport, id, transport.output_frame())
}

fn anchor_transport_phase_from_pad_at_frame(
    mixer: &RtMixer,
    transport: &mut TransportTimeline,
    id: usize,
    output_frame: u64,
) -> bool {
    let Some(source_beat) = mixer.active_pad_beat_position(id) else {
        return false;
    };
    let Some(bpm) = mixer.transport_reference_bpm_for_sample_id(id) else {
        return false;
    };

    let anchored =
        transport.set_master_bpm_and_anchor_beat_position_at_frame(bpm, source_beat, output_frame);
    if anchored {
        transport.complete_bootstrap();
    }
    anchored
}

/// Called after both command/parameter batches and at render segment boundaries.
/// The source position and output frame therefore describe the same instant.
fn try_bootstrap_transport_from_reference(
    mixer: &RtMixer,
    transport: &mut TransportTimeline,
    output_frame: u64,
) -> bool {
    let Some(id) = transport.bootstrap_reference() else {
        return false;
    };
    let Some(source_beat) = mixer.active_pad_beat_position(id) else {
        return false;
    };
    let Some(bpm) = mixer.transport_reference_bpm_for_sample_id(id) else {
        return false;
    };
    transport.bootstrap_from_source_at_frame(bpm, source_beat, output_frame)
}

fn drain_scheduler_due_at_callback_start<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    mixer: &mut RtMixer,
    transport: &mut TransportTimeline,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    while let Some(event) = scheduler.pop_due_at_callback_start(callback_start_frame) {
        execute_scheduled_command(
            mixer,
            transport,
            event.execution_frame,
            event.command,
            audio_messages,
            retirement,
        );
    }
}

fn execute_scheduled_command<S: AudioMessageSink, R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    _transport: &mut TransportTimeline,
    output_frame: u64,
    command: ScheduledCommand,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    match command {
        ScheduledCommand::PlaySample {
            id,
            volume,
            received_at_ns: _,
        } => {
            let started =
                mixer.play_sample_at_output_frame_rt(id, volume, output_frame, retirement);

            if started {
                audio_messages.push_audio_message(AudioMessage::SampleStarted { id });
            } else {
                audio_messages.push_audio_message(AudioMessage::SampleStopped { id });
            }
        }
        ScheduledCommand::StopAllThenPlaySample {
            id,
            volume,
            received_at_ns: _,
        } => {
            if !mixer.can_play_sample(id, volume) {
                return;
            }

            stop_all_samples(mixer, audio_messages, retirement);
            let started =
                mixer.play_sample_at_output_frame_rt(id, volume, output_frame, retirement);

            if started {
                audio_messages.push_audio_message(AudioMessage::SampleStarted { id });
            }
        }
        ScheduledCommand::StopSample { id } => {
            mixer.stop_sample_rt(id, retirement);
            audio_messages.push_audio_message(AudioMessage::SampleStopped { id });
        }
        ScheduledCommand::StopAll => {
            stop_all_samples(mixer, audio_messages, retirement);
        }
    }
}

fn stop_all_samples<S: AudioMessageSink, R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    for voice in &mut mixer.voices {
        if voice.active {
            let id = voice.sample_id;
            voice.stop_rt(retirement);
            audio_messages.push_audio_message(AudioMessage::SampleStopped { id });
        }
    }
}

// Keep scheduler, transport, mixer, output, and retirement ownership visible in the render path.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn render_scheduled_audio<const CAPACITY: usize, S: AudioMessageSink, R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    output: &mut [f32],
    pad_peaks: &mut [f32; NUM_SAMPLES],
    callback_start_frame: u64,
    channels: usize,
    transport: &mut TransportTimeline,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    let mut pad_activity = RtRenderPadActivity::default();
    render_scheduled_audio_tracking_pads(
        mixer,
        scheduler,
        output,
        pad_peaks,
        &mut pad_activity,
        callback_start_frame,
        channels,
        transport,
        audio_messages,
        retirement,
    );
}

// Keep scheduler, transport, mixer, output, telemetry, and retirement ownership visible.
#[allow(clippy::too_many_arguments)]
fn render_scheduled_audio_tracking_pads<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    mixer: &mut RtMixer,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    output: &mut [f32],
    pad_peaks: &mut [f32; NUM_SAMPLES],
    pad_activity: &mut RtRenderPadActivity,
    callback_start_frame: u64,
    channels: usize,
    transport: &mut TransportTimeline,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    output.fill(0.0);
    pad_peaks.fill(0.0);
    pad_activity.clear();

    drain_scheduler_due_at_callback_start(
        scheduler,
        callback_start_frame,
        mixer,
        transport,
        audio_messages,
        retirement,
    );

    if channels == 0 {
        return;
    }

    let frames = output.len() / channels;
    if frames == 0 {
        return;
    }

    let callback_end_frame = callback_start_frame.saturating_add(frames as u64);
    let mut rendered_until_frame = callback_start_frame;
    let mut segment_peaks = [0.0_f32; NUM_SAMPLES];

    while rendered_until_frame < callback_end_frame {
        try_bootstrap_transport_from_reference(mixer, transport, rendered_until_frame);
        let Some(next_target_frame) = scheduler.peek_next_target_frame() else {
            render_mixer_segment(
                mixer,
                output,
                pad_peaks,
                &mut segment_peaks,
                pad_activity,
                callback_start_frame,
                rendered_until_frame,
                callback_end_frame,
                channels,
                retirement,
            );
            break;
        };

        if next_target_frame >= callback_end_frame {
            render_mixer_segment(
                mixer,
                output,
                pad_peaks,
                &mut segment_peaks,
                pad_activity,
                callback_start_frame,
                rendered_until_frame,
                callback_end_frame,
                channels,
                retirement,
            );
            break;
        }

        if next_target_frame > rendered_until_frame {
            render_mixer_segment(
                mixer,
                output,
                pad_peaks,
                &mut segment_peaks,
                pad_activity,
                callback_start_frame,
                rendered_until_frame,
                next_target_frame,
                channels,
                retirement,
            );
            rendered_until_frame = next_target_frame;
        }

        while let Some(event) =
            scheduler.pop_due_through(callback_start_frame, rendered_until_frame)
        {
            execute_scheduled_command(
                mixer,
                transport,
                event.execution_frame,
                event.command,
                audio_messages,
                retirement,
            );
        }
    }
}

// Keep segment frame bounds and realtime state explicit for in-buffer scheduling tests.
#[allow(clippy::too_many_arguments)]
fn render_mixer_segment<R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    output: &mut [f32],
    pad_peaks: &mut [f32; NUM_SAMPLES],
    segment_peaks: &mut [f32; NUM_SAMPLES],
    pad_activity: &mut RtRenderPadActivity,
    callback_start_frame: u64,
    segment_start_frame: u64,
    segment_end_frame: u64,
    channels: usize,
    retirement: &mut R,
) {
    if segment_end_frame <= segment_start_frame {
        return;
    }

    let start_frame = (segment_start_frame - callback_start_frame) as usize;
    let end_frame = (segment_end_frame - callback_start_frame) as usize;
    let start = start_frame * channels;
    let end = end_frame * channels;

    mixer.render_rt_at_output_frame_tracking_pads(
        &mut output[start..end],
        segment_peaks,
        segment_start_frame,
        pad_activity,
        retirement,
    );

    for id in pad_activity.iter() {
        pad_peaks[id] = pad_peaks[id].max(segment_peaks[id]);
    }
}

fn master_output_peak(output: &[f32]) -> f32 {
    output.iter().fold(0.0_f32, |peak, sample| {
        if sample.is_finite() {
            peak.max(sample.abs())
        } else {
            peak
        }
    })
}

fn publish_master_peak_telemetry<S: AudioMessageSink>(
    audio_messages: &mut S,
    master_peak: f32,
    frame_clock: u64,
    emit_interval_frames: u64,
    last_master_emit_frame: &mut u64,
) {
    if frame_clock.wrapping_sub(*last_master_emit_frame) < emit_interval_frames {
        return;
    }

    *last_master_emit_frame = frame_clock;

    if master_peak > 0.0 && master_peak.is_finite() {
        audio_messages.push_audio_message(AudioMessage::MasterPeak { peak: master_peak });
    }
}

fn publish_pad_telemetry<S: AudioMessageSink>(
    audio_messages: &mut S,
    mixer: &RtMixer,
    pad_peaks: &[f32; NUM_SAMPLES],
    pad_activity: &RtRenderPadActivity,
    frame_clock: u64,
    emit_interval_frames: u64,
    last_pad_emit_frame: &mut u64,
) {
    if frame_clock.wrapping_sub(*last_pad_emit_frame) < emit_interval_frames {
        return;
    }

    *last_pad_emit_frame = frame_clock;

    for id in pad_activity.iter() {
        let peak = pad_peaks[id];
        if peak > 0.0 && peak.is_finite() {
            let peak = peak.clamp(0.0, 1.0);
            audio_messages.push_audio_message(AudioMessage::PadPeak { id, peak });
        }

        if let Some(position_s) = mixer.pad_playhead_seconds(id)
            && position_s.is_finite()
        {
            audio_messages.push_audio_message(AudioMessage::PadPlayhead { id, position_s });
        }
    }
}

fn control_message_retirement_slots_needed(message: &ControlMessage) -> usize {
    match message {
        ControlMessage::LoadSample { .. } | ControlMessage::PublishPreparedStems { .. } => 2,
        ControlMessage::PublishConstantTiming { .. } => 1,
        ControlMessage::StopSample { .. } => MAX_VOICES,
        ControlMessage::UnloadSample { .. } => MAX_VOICES + 2,
        ControlMessage::StopAll() | ControlMessage::PlaySampleExclusive { .. } => MAX_VOICES,
        _ => 0,
    }
}

#[cfg(test)]
#[path = "constant_timing_callback_tests.rs"]
mod constant_timing_callback_tests;

// Keep queue, scheduler, transport, mixer, telemetry, and retirement state explicit in the callback.
#[allow(clippy::too_many_arguments)]
fn drain_control_messages<const CAPACITY: usize, S: AudioMessageSink, R: AudioBufferRetirement>(
    consumer: &mut Consumer<ControlMessage>,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: &mut TriggerQuantization,
    transport: &mut TransportTimeline,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
) -> usize {
    let mut processed = 0;

    while processed < MAX_CONTROL_MESSAGES_PER_CALLBACK {
        let needed_retirement_slots = match consumer.peek() {
            Ok(message) => control_message_retirement_slots_needed(message),
            Err(_) => break,
        };

        if needed_retirement_slots > 0
            && retirement.available_retirement_slots() < needed_retirement_slots
        {
            break;
        }

        let Ok(message) = consumer.pop() else {
            break;
        };

        process_control_message(
            message,
            scheduler,
            callback_start_frame,
            trigger_quantization,
            transport,
            mixer,
            audio_messages,
            retirement,
        );
        processed += 1;
    }

    processed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParameterDrainResult {
    messages_drained: usize,
    parameters_applied: usize,
}

#[derive(Debug, Clone, Copy)]
struct PendingPadBpm {
    id: usize,
    bpm: Option<f32>,
    through_epoch: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct PendingPadGain {
    id: usize,
    gain_db: f32,
}

#[derive(Debug, Clone, Copy)]
struct PendingPadEq {
    id: usize,
    low_db: f32,
    mid_db: f32,
    high_db: f32,
}

struct PendingControlParameters {
    volume: Option<f32>,
    speed: Option<f32>,
    master_bpm: Option<f32>,
    pad_bpm: [PendingPadBpm; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
    pad_bpm_count: usize,
    pad_gain: [PendingPadGain; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
    pad_gain_count: usize,
    pad_eq: [PendingPadEq; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
    pad_eq_count: usize,
}

impl Default for PendingControlParameters {
    fn default() -> Self {
        Self {
            volume: None,
            speed: None,
            master_bpm: None,
            pad_bpm: [PendingPadBpm {
                id: 0,
                bpm: None,
                through_epoch: None,
            }; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
            pad_bpm_count: 0,
            pad_gain: [PendingPadGain {
                id: 0,
                gain_db: 0.0,
            }; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
            pad_gain_count: 0,
            pad_eq: [PendingPadEq {
                id: 0,
                low_db: 0.0,
                mid_db: 0.0,
                high_db: 0.0,
            }; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
            pad_eq_count: 0,
        }
    }
}

impl PendingControlParameters {
    fn record(&mut self, message: ControlParameterMessage) {
        match message {
            ControlParameterMessage::SetVolume(volume) => self.volume = Some(volume),
            ControlParameterMessage::SetSpeed(speed) => self.speed = Some(speed),
            ControlParameterMessage::SetMasterBpm(bpm) => self.master_bpm = Some(bpm),
            #[cfg(test)]
            ControlParameterMessage::SetPadBpm { id, bpm } => {
                self.record_pad_bpm(id, bpm, None);
            }
            ControlParameterMessage::SetLegacyPadBpm {
                id,
                bpm,
                through_epoch,
            } => {
                self.record_pad_bpm(id, bpm, Some(through_epoch));
            }
            ControlParameterMessage::SetPadGain { id, gain_db } => {
                self.record_pad_gain(id, gain_db);
            }
            ControlParameterMessage::SetPadEq {
                id,
                low_db,
                mid_db,
                high_db,
            } => {
                self.record_pad_eq(id, low_db, mid_db, high_db);
            }
        }
    }

    fn record_pad_bpm(&mut self, id: usize, bpm: Option<f32>, through_epoch: Option<u64>) {
        if id >= NUM_SAMPLES {
            return;
        }
        if let Some(pending) = self.pad_bpm[..self.pad_bpm_count]
            .iter_mut()
            .find(|pending| pending.id == id)
        {
            pending.bpm = bpm;
            pending.through_epoch = through_epoch;
            return;
        }
        if self.pad_bpm_count < self.pad_bpm.len() {
            self.pad_bpm[self.pad_bpm_count] = PendingPadBpm {
                id,
                bpm,
                through_epoch,
            };
            self.pad_bpm_count += 1;
        }
    }

    fn record_pad_gain(&mut self, id: usize, gain_db: f32) {
        if id >= NUM_SAMPLES {
            return;
        }
        if let Some(pending) = self.pad_gain[..self.pad_gain_count]
            .iter_mut()
            .find(|pending| pending.id == id)
        {
            pending.gain_db = gain_db;
            return;
        }
        if self.pad_gain_count < self.pad_gain.len() {
            self.pad_gain[self.pad_gain_count] = PendingPadGain { id, gain_db };
            self.pad_gain_count += 1;
        }
    }

    fn record_pad_eq(&mut self, id: usize, low_db: f32, mid_db: f32, high_db: f32) {
        if id >= NUM_SAMPLES {
            return;
        }
        if let Some(pending) = self.pad_eq[..self.pad_eq_count]
            .iter_mut()
            .find(|pending| pending.id == id)
        {
            pending.low_db = low_db;
            pending.mid_db = mid_db;
            pending.high_db = high_db;
            return;
        }
        if self.pad_eq_count < self.pad_eq.len() {
            self.pad_eq[self.pad_eq_count] = PendingPadEq {
                id,
                low_db,
                mid_db,
                high_db,
            };
            self.pad_eq_count += 1;
        }
    }

    fn apply_to(self, mixer: &mut RtMixer, transport: &mut TransportTimeline) -> usize {
        let mut applied = 0;

        if let Some(volume) = self.volume {
            mixer.set_volume(volume);
            applied += 1;
        }
        if let Some(speed) = self.speed {
            mixer.set_speed(speed);
            applied += 1;
        }
        if let Some(bpm) = self.master_bpm {
            mixer.set_master_bpm(bpm);
            transport
                .set_master_bpm_preserving_beat_position_at_frame(bpm, transport.output_frame());
            applied += 1;
        }
        for pending in self.pad_bpm[..self.pad_bpm_count].iter().copied() {
            mixer.set_pad_bpm(pending.id, pending.bpm);
            if let Some(epoch) = pending.through_epoch {
                mixer.clear_constant_timing(pending.id, epoch);
            }
            applied += 1;
        }
        for pending in self.pad_gain[..self.pad_gain_count].iter().copied() {
            mixer.set_pad_gain(pending.id, pending.gain_db);
            applied += 1;
        }
        for pending in self.pad_eq[..self.pad_eq_count].iter().copied() {
            mixer.set_pad_eq(pending.id, pending.low_db, pending.mid_db, pending.high_db);
            applied += 1;
        }

        applied
    }
}

fn drain_parameter_messages(
    consumer: &mut Consumer<ControlParameterMessage>,
    mixer: &mut RtMixer,
    transport: &mut TransportTimeline,
) -> ParameterDrainResult {
    let mut pending = PendingControlParameters::default();
    let mut messages_drained = 0;

    while messages_drained < MAX_PARAMETER_MESSAGES_PER_CALLBACK {
        let Ok(message) = consumer.pop() else {
            break;
        };

        pending.record(message);
        messages_drained += 1;
    }

    ParameterDrainResult {
        messages_drained,
        parameters_applied: pending.apply_to(mixer, transport),
    }
}

// Keep ordered command effects explicit at the realtime boundary.
#[allow(clippy::too_many_arguments)]
fn process_control_message<const CAPACITY: usize, S: AudioMessageSink, R: AudioBufferRetirement>(
    message: ControlMessage,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: &mut TriggerQuantization,
    transport: &mut TransportTimeline,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    match message {
        ControlMessage::Ping() => {
            audio_messages.push_audio_message(AudioMessage::Pong());
        }
        ControlMessage::LoadSample { id, sample } => {
            mixer.load_sample_rt(id, sample, retirement);
        }
        ControlMessage::PublishPreparedStems { id, stems } => {
            mixer.publish_prepared_stems_rt(id, stems, retirement);
        }
        ControlMessage::PublishConstantTiming { id, timing } => {
            mixer.publish_constant_timing_rt(id, timing, retirement);
        }
        ControlMessage::ClearPadConstantTiming { id, through_epoch } => {
            mixer.clear_constant_timing(id, through_epoch);
        }
        ControlMessage::SetStemMixMode {
            id,
            mode,
            source_version_hash,
        } => {
            mixer.set_stem_mix_mode(id, mode, source_version_hash);
        }
        ControlMessage::SetStemEnabledMask {
            id,
            enabled_stem_mask,
            source_version_hash,
        } => {
            mixer.set_stem_enabled_mask(id, enabled_stem_mask, source_version_hash);
        }
        ControlMessage::PlaySample {
            id,
            volume,
            received_at_ns,
        } => {
            schedule_play_sample_command(
                scheduler,
                callback_start_frame,
                *trigger_quantization,
                transport,
                id,
                volume,
                mixer,
                audio_messages,
                retirement,
                received_at_ns,
            );
        }
        ControlMessage::PlaySampleExclusive {
            id,
            volume,
            received_at_ns,
        } => {
            schedule_exclusive_play_sample_command(
                scheduler,
                callback_start_frame,
                *trigger_quantization,
                transport,
                id,
                volume,
                mixer,
                audio_messages,
                retirement,
                received_at_ns,
            );
        }
        ControlMessage::StopSample { id } => {
            schedule_immediate_command(
                scheduler,
                callback_start_frame,
                ScheduledCommand::StopSample { id },
                mixer,
                transport,
                audio_messages,
                retirement,
            );
        }
        ControlMessage::StopAll() => {
            schedule_immediate_command(
                scheduler,
                callback_start_frame,
                ScheduledCommand::StopAll,
                mixer,
                transport,
                audio_messages,
                retirement,
            );
        }
        ControlMessage::UnloadSample { id } => {
            mixer.unload_sample_rt(id, retirement);
            transport.clear_pending_bootstrap_for_pad(id);
        }
        ControlMessage::SetBpmLock(enabled) => {
            mixer.set_bpm_lock(enabled);
        }
        ControlMessage::SetKeyLock(enabled) => {
            mixer.set_key_lock(enabled);
        }
        ControlMessage::SetPadKeyLock { id, enabled } => {
            mixer.set_pad_key_lock(id, enabled);
        }
        #[cfg(test)]
        ControlMessage::SetPadTimingMetadata { id, metadata } => {
            mixer.set_pad_timing_metadata(id, metadata);
        }
        ControlMessage::SetLegacyPadTimingMetadata {
            id,
            metadata,
            through_epoch,
        } => {
            mixer.set_pad_timing_metadata(id, metadata);
            mixer.clear_constant_timing(id, through_epoch);
        }
        ControlMessage::BootstrapTransportFromPad { id } => {
            if id < NUM_SAMPLES {
                transport.request_bootstrap(id);
            }
        }
        ControlMessage::AnchorTransportPhaseFromPad { id } => {
            anchor_transport_phase_from_pad(mixer, transport, id);
        }
        ControlMessage::SetPadLoopRegion { id, start_s, end_s } => {
            mixer.set_pad_loop_region(id, start_s, end_s);
        }
        ControlMessage::SetTriggerQuantization(mode) => {
            *trigger_quantization = mode;
        }
        ControlMessage::PauseSample { id } => {
            mixer.pause_sample_at_output_frame(id, callback_start_frame);
        }
        ControlMessage::ResumeSample { id } => {
            mixer.resume_sample_at_output_frame(id, callback_start_frame);
        }
        ControlMessage::SeekSample { id, position_s } => {
            mixer.seek_sample_at_output_frame(id, position_s, callback_start_frame);
        }
    }
}

/// Create and configure the audio stream
///
/// This function:
/// 1. Sets up the default audio device
/// 2. Configures the stream with appropriate parameters
/// 3. Creates ring buffers for message passing
/// 4. Initializes the mixer
/// 5. Builds and returns the audio stream
pub fn create_audio_stream(
    input_clock: InputClock,
) -> Result<AudioStreamHandle, Box<dyn std::error::Error>> {
    setup_logger();

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No audio device found")?;

    let config = device.default_output_config()?;
    let sample_rate = config.sample_rate();
    let sample_rate_hz = sample_rate;
    let channels = config.channels();

    log::info!(
        "Starting AudioEngine... ({} ch@{} Hz)",
        channels,
        sample_rate_hz
    );

    // Create ring buffer for incoming messages (Python->Rust)
    let (producer_in, mut consumer_in) = RingBuffer::new(1024);

    // Create ring buffer for fast parameter updates (Python->Rust)
    let (parameter_producer_in, mut parameter_consumer_in) = RingBuffer::new(1024);

    // Create ring buffer for outgoing messages (Rust->Python)
    let (mut producer_out, consumer_out) = RingBuffer::new(1024);

    let mut mixer = RtMixer::try_new(channels as usize, sample_rate_hz as f32)?;
    let mut transport = TransportTimeline::new(sample_rate_hz);
    let mut scheduler = TransportScheduler::new();
    let mut trigger_quantization = TriggerQuantization::Immediate;
    let output_clock = Arc::new(SharedOutputClock::new());
    let callback_clock = output_clock.clone();
    let mut clock_mapper = OutputClockMapper::new();
    let (mut retired_buffers, retirement_worker) = create_audio_buffer_retirement();

    let emit_interval_frames: u64 = (sample_rate_hz as u64 / 10).max(1);
    let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
    let mut pad_activity = RtRenderPadActivity::default();
    let mut last_pad_emit_frame = 0_u64;
    let mut last_master_emit_frame = 0_u64;

    // Create stream config
    let stream_config = StreamConfig {
        channels,
        sample_rate,
        buffer_size: BufferSize::Fixed(512),
    };

    // Create audio stream with callback
    let stream = device.build_output_stream(
        &stream_config,
        move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
            let observed_at_ns = input_clock.capture_ns();
            let buffer_start_frame = transport.output_frame();

            let snapshot = clock_mapper.observe_callback(
                info,
                OutputClockSnapshot {
                    valid: true,
                    observed_at_ns,
                    audible_at_ns: observed_at_ns,
                    output_frame: buffer_start_frame,
                    sample_rate_hz,
                    master_bpm: transport.master_bpm(),
                    downbeat_frame: transport.downbeat_frame(),
                    master_beat: transport
                        .beat_position_at_frame(buffer_start_frame)
                        .unwrap_or(0.0),
                    freshness_ns: 0,
                },
                data.len() / channels as usize,
            );

            drain_control_messages(
                &mut consumer_in,
                &mut scheduler,
                buffer_start_frame,
                &mut trigger_quantization,
                &mut transport,
                &mut mixer,
                &mut producer_out,
                &mut retired_buffers,
            );

            drain_parameter_messages(&mut parameter_consumer_in, &mut mixer, &mut transport);

            try_bootstrap_transport_from_reference(&mixer, &mut transport, buffer_start_frame);

            // Publish the same clock estimate with the accepted current grid.
            callback_clock.publish(OutputClockSnapshot {
                master_bpm: transport.master_bpm(),
                downbeat_frame: transport.downbeat_frame(),
                master_beat: transport
                    .beat_position_at_frame(buffer_start_frame)
                    .unwrap_or(0.0),
                ..snapshot
            });

            // Render audio + compute per-pad peaks.
            render_scheduled_audio_tracking_pads(
                &mut mixer,
                &mut scheduler,
                data,
                &mut pad_peaks,
                &mut pad_activity,
                buffer_start_frame,
                channels as usize,
                &mut transport,
                &mut producer_out,
                &mut retired_buffers,
            );
            let master_peak = master_output_peak(data);

            let frames = data.len() / channels as usize;
            transport.advance_by_rendered_frames(frames);
            let frame_clock = transport.output_frame();

            publish_pad_telemetry(
                &mut producer_out,
                &mixer,
                &pad_peaks,
                &pad_activity,
                frame_clock,
                emit_interval_frames,
                &mut last_pad_emit_frame,
            );

            publish_master_peak_telemetry(
                &mut producer_out,
                master_peak,
                frame_clock,
                emit_interval_frames,
                &mut last_master_emit_frame,
            );
        },
        |err| {
            log::error!("Audio stream error: {}", err);
        },
        None,
    )?;

    Ok(AudioStreamHandle {
        stream,
        _retirement_worker: retirement_worker,
        producer: Arc::new(Mutex::new(producer_in)),
        parameter_producer: Arc::new(Mutex::new(parameter_producer_in)),
        consumer: Arc::new(Mutex::new(consumer_out)),
        output_channels: channels as usize,
        output_sample_rate: sample_rate_hz,
        output_clock,
    })
}

/// Start playing the audio stream
pub fn start_stream(stream: &Stream) -> Result<(), Box<dyn std::error::Error>> {
    stream.play()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
    use crate::audio_engine::constants::PAD_EQ_DB_MIN;
    use crate::messages::{PadTimingMetadata, SampleBuffer};
    use std::sync::Arc;

    impl AudioMessageSink for Vec<AudioMessage> {
        fn push_audio_message(&mut self, message: AudioMessage) {
            self.push(message);
        }
    }

    fn create_test_sample(channels: usize, frames: usize, value: f32) -> SampleBuffer {
        let samples = vec![value; channels * frames];
        SampleBuffer {
            channels,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    #[test]
    fn stamped_commands_keep_legacy_quantization_and_retain_capture_in_scheduler() {
        for exclusive in [false, true] {
            for received_at_ns in [None, Some(0), Some(42), Some(u64::MAX)] {
                let mut mixer = RtMixer::new(1, 48_000.0);
                mixer.load_sample(0, create_test_sample(1, 100, 0.5));
                let mut transport = TransportTimeline::new(48_000);
                transport.advance_by_rendered_frames(6_001);
                let mut scheduler = FixedCapacityScheduler::<8>::new();
                let mut mode = TriggerQuantization::Grid { step_64ths: 4 };
                let command = if exclusive {
                    ControlMessage::PlaySampleExclusive {
                        id: 0,
                        volume: 1.0,
                        received_at_ns,
                    }
                } else {
                    ControlMessage::PlaySample {
                        id: 0,
                        volume: 1.0,
                        received_at_ns,
                    }
                };
                let mut messages = Vec::new();
                process_control_message(
                    command,
                    &mut scheduler,
                    6_001,
                    &mut mode,
                    &mut transport,
                    &mut mixer,
                    &mut messages,
                    &mut ImmediateAudioBufferRetirement,
                );

                assert!(messages.is_empty());
                assert_eq!(scheduler.peek_next_target_frame(), Some(12_000));
                let event = scheduler.pop_due_through(6_001, 12_000).unwrap();
                let retained = match event.command {
                    ScheduledCommand::PlaySample { received_at_ns, .. }
                    | ScheduledCommand::StopAllThenPlaySample { received_at_ns, .. } => {
                        received_at_ns
                    }
                    _ => panic!("unexpected scheduled command"),
                };
                assert_eq!(retained, received_at_ns);
                execute_scheduled_command(
                    &mut mixer,
                    &mut transport,
                    event.execution_frame,
                    event.command,
                    &mut messages,
                    &mut ImmediateAudioBufferRetirement,
                );
                let voice = mixer.voices.iter().find(|voice| voice.active).unwrap();
                assert_eq!(voice.frame_pos, 0);
            }
        }
    }

    fn assert_started(messages: &[AudioMessage], index: usize, expected_id: usize) {
        assert!(matches!(
            messages.get(index),
            Some(AudioMessage::SampleStarted { id }) if *id == expected_id
        ));
    }

    fn assert_stopped(messages: &[AudioMessage], index: usize, expected_id: usize) {
        assert!(matches!(
            messages.get(index),
            Some(AudioMessage::SampleStopped { id }) if *id == expected_id
        ));
    }

    fn active_voice_frame(mixer: &RtMixer, id: usize) -> Option<usize> {
        mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == id)
            .map(|voice| voice.frame_pos)
    }

    #[test]
    fn control_drain_respects_per_callback_budget() {
        let total_messages = MAX_CONTROL_MESSAGES_PER_CALLBACK + 3;
        let (mut producer, mut consumer) = RingBuffer::new(total_messages + 1);
        for _ in 0..total_messages {
            producer.push(ControlMessage::Ping()).unwrap();
        }
        let mut mixer = RtMixer::new(1, 44_100.0);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        let mut trigger_quantization = TriggerQuantization::Immediate;
        let mut messages = Vec::new();

        let processed = drain_control_messages(
            &mut consumer,
            &mut scheduler,
            0,
            &mut trigger_quantization,
            &mut transport,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert_eq!(processed, MAX_CONTROL_MESSAGES_PER_CALLBACK);
        assert_eq!(messages.len(), MAX_CONTROL_MESSAGES_PER_CALLBACK);
        assert_eq!(consumer.slots(), 3);
    }

    #[test]
    fn parameter_drain_coalesces_latest_value_per_identity() {
        let (mut producer, mut consumer) = RingBuffer::new(8);
        producer
            .push(ControlParameterMessage::SetPadGain {
                id: 0,
                gain_db: -6.0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadGain {
                id: 0,
                gain_db: 6.0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetVolume(0.5))
            .unwrap();

        let mut mixer = RtMixer::new(1, 44_100.0);
        let mut transport = TransportTimeline::new(44_100);
        mixer.load_sample(0, create_test_sample(1, 8, 1.0));

        let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);

        assert_eq!(
            result,
            ParameterDrainResult {
                messages_drained: 3,
                parameters_applied: 2
            }
        );

        let mut output = vec![0.0; 1];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        assert!(mixer.play_sample(0, 1.0));
        mixer.render(&mut output, &mut pad_peaks);

        let expected = 10.0_f32.powf(6.0 / 20.0) * 0.5;
        assert!((output[0] - expected).abs() < 1e-5);
    }

    #[test]
    fn parameter_drain_coalesces_latest_pad_eq_target() {
        let (mut producer, mut consumer) = RingBuffer::new(4);
        producer
            .push(ControlParameterMessage::SetPadEq {
                id: 0,
                low_db: PAD_EQ_DB_MIN,
                mid_db: PAD_EQ_DB_MIN,
                high_db: PAD_EQ_DB_MIN,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadEq {
                id: 0,
                low_db: 0.0,
                mid_db: 0.0,
                high_db: 0.0,
            })
            .unwrap();

        let mut mixer = RtMixer::new(1, 44_100.0);
        let mut transport = TransportTimeline::new(44_100);
        mixer.load_sample(0, create_test_sample(1, 8, 0.5));

        let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);

        assert_eq!(
            result,
            ParameterDrainResult {
                messages_drained: 2,
                parameters_applied: 1
            }
        );

        assert!(mixer.play_sample(0, 1.0));
        let mut output = vec![0.0; 1];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!((output[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn parameter_drain_ignores_invalid_pad_ids_while_coalescing_touched_ids() {
        let (mut producer, mut consumer) = RingBuffer::new(8);
        producer
            .push(ControlParameterMessage::SetPadGain {
                id: NUM_SAMPLES,
                gain_db: 12.0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadGain {
                id: 1,
                gain_db: -6.0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadGain {
                id: 1,
                gain_db: 6.0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadBpm {
                id: 2,
                bpm: Some(90.0),
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadBpm { id: 2, bpm: None })
            .unwrap();

        let mut mixer = RtMixer::new(1, 44_100.0);
        let mut transport = TransportTimeline::new(44_100);
        mixer.load_sample(1, create_test_sample(1, 8, 1.0));
        mixer.set_pad_bpm(2, Some(100.0));

        let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);

        assert_eq!(
            result,
            ParameterDrainResult {
                messages_drained: 5,
                parameters_applied: 2
            }
        );
        assert_eq!(mixer.output_bpm_for_sample_id(2), None);

        assert!(mixer.play_sample(1, 1.0));
        let mut output = vec![0.0; 1];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        let expected = 10.0_f32.powf(6.0 / 20.0);
        assert!((output[0] - expected).abs() < 1e-5);
    }

    #[test]
    fn master_bpm_parameter_updates_mixer_and_transport_clock() {
        let (mut producer, mut consumer) = RingBuffer::new(4);
        producer
            .push(ControlParameterMessage::SetMasterBpm(120.0))
            .unwrap();

        let mut mixer = RtMixer::new(1, 10.0);
        mixer.set_bpm_lock(true);
        mixer.set_pad_bpm(0, Some(100.0));

        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(10);
        assert_eq!(transport.bar_phase_beats(), Some(1.0));

        let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);

        assert_eq!(
            result,
            ParameterDrainResult {
                messages_drained: 1,
                parameters_applied: 1
            }
        );
        assert_eq!(transport.master_bpm(), Some(120.0));
        assert_eq!(transport.downbeat_frame(), 5);
        assert_eq!(transport.bar_phase_beats(), Some(1.0));
        let output_bpm = mixer.output_bpm_for_sample_id(0).unwrap();
        assert!((output_bpm - 120.0).abs() < 1e-4);
    }

    #[test]
    fn full_parameter_queue_does_not_consume_command_queue_capacity() {
        let (mut command_producer, mut command_consumer) = RingBuffer::<ControlMessage>::new(1);
        let (mut parameter_producer, _parameter_consumer) =
            RingBuffer::<ControlParameterMessage>::new(1);

        parameter_producer
            .push(ControlParameterMessage::SetVolume(0.25))
            .unwrap();
        assert!(
            parameter_producer
                .push(ControlParameterMessage::SetVolume(0.5))
                .is_err()
        );

        assert!(command_producer.push(ControlMessage::StopAll()).is_ok());
        assert!(matches!(
            command_consumer.pop(),
            Ok(ControlMessage::StopAll())
        ));
    }

    #[test]
    fn master_output_peak_is_post_sum_and_post_master_volume() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 16, 0.8));
        mixer.load_sample(1, create_test_sample(1, 16, 0.6));
        mixer.set_volume(0.5);
        assert!(mixer.play_sample(0, 1.0));
        assert!(mixer.play_sample(1, 1.0));

        let mut output = vec![0.0; 16];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|sample| (*sample - 0.7).abs() < 1e-5));
        assert!((master_output_peak(&output) - 0.7).abs() < 1e-5);
        assert!((pad_peaks[0] - 0.8).abs() < 1e-5);
        assert!((pad_peaks[1] - 0.6).abs() < 1e-5);
    }

    #[test]
    fn master_peak_telemetry_preserves_unclamped_value() {
        let (mut producer, mut consumer) = RingBuffer::<AudioMessage>::new(2);
        let mut last_master_emit_frame = 0;

        publish_master_peak_telemetry(
            &mut producer,
            1.25,
            4_410,
            4_410,
            &mut last_master_emit_frame,
        );

        assert_eq!(last_master_emit_frame, 4_410);
        assert!(matches!(
            consumer.pop(),
            Ok(AudioMessage::MasterPeak { peak }) if (peak - 1.25).abs() < 1e-5
        ));
    }

    #[test]
    fn full_audio_message_queue_drops_master_peak_without_blocking() {
        let (mut producer, mut consumer) = RingBuffer::<AudioMessage>::new(1);
        producer.push(AudioMessage::Pong()).unwrap();
        let mut last_master_emit_frame = 0;

        publish_master_peak_telemetry(
            &mut producer,
            0.75,
            4_410,
            4_410,
            &mut last_master_emit_frame,
        );

        assert_eq!(last_master_emit_frame, 4_410);
        assert!(matches!(consumer.pop(), Ok(AudioMessage::Pong())));
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn pad_telemetry_publishes_only_touched_pads_on_shared_interval() {
        let mixer = RtMixer::new(1, 44_100.0);
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        pad_peaks[0] = 0.5;
        pad_peaks[1] = 0.75;
        let mut activity = RtRenderPadActivity::default();
        activity.record(0);
        let mut messages = Vec::new();
        let mut last_pad_emit_frame = 0;

        publish_pad_telemetry(
            &mut messages,
            &mixer,
            &pad_peaks,
            &activity,
            9,
            10,
            &mut last_pad_emit_frame,
        );

        assert!(messages.is_empty());
        assert_eq!(last_pad_emit_frame, 0);

        publish_pad_telemetry(
            &mut messages,
            &mixer,
            &pad_peaks,
            &activity,
            10,
            10,
            &mut last_pad_emit_frame,
        );

        assert_eq!(last_pad_emit_frame, 10);
        assert_eq!(messages.len(), 1);
        assert!(matches!(
            messages[0],
            AudioMessage::PadPeak { id: 0, peak } if (peak - 0.5).abs() < 1e-5
        ));

        messages.clear();
        activity.clear();
        publish_pad_telemetry(
            &mut messages,
            &mixer,
            &pad_peaks,
            &activity,
            20,
            10,
            &mut last_pad_emit_frame,
        );

        assert_eq!(last_pad_emit_frame, 20);
        assert!(messages.is_empty());
    }

    #[test]
    fn retirement_slot_estimate_covers_polyphonic_stop_paths() {
        assert_eq!(
            control_message_retirement_slots_needed(&ControlMessage::StopSample { id: 0 }),
            MAX_VOICES
        );
        assert_eq!(
            control_message_retirement_slots_needed(&ControlMessage::UnloadSample { id: 0 }),
            MAX_VOICES + 2
        );
        assert_eq!(
            control_message_retirement_slots_needed(&ControlMessage::StopAll()),
            MAX_VOICES
        );
        assert_eq!(
            control_message_retirement_slots_needed(&ControlMessage::PlaySampleExclusive {
                id: 0,
                volume: 1.0,
                received_at_ns: None
            }),
            MAX_VOICES
        );
    }

    #[test]
    fn test_logger_setup() {
        // This test just verifies that logger setup doesn't panic
        // Multiple calls should be safe (though only the first takes effect)
        setup_logger();
        setup_logger(); // Should not panic
    }

    #[test]
    fn test_audio_stream_creation() {
        // This is a basic smoke test to ensure the function signature is correct
        // Actual stream creation requires audio hardware
        if cpal::default_host().default_output_device().is_none() {
            return; // Skip test if no audio device available
        }

        let result = create_audio_stream(InputClock::new());
        // We expect this to potentially fail in test environments,
        // but we want to ensure the function exists and has the right signature
        match result {
            Ok(_) => {
                // If it works, that's great
            }
            Err(_) => {
                // Expected in many test environments
            }
        }
    }

    #[test]
    fn immediate_command_uses_current_frame_scheduler_path() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        let mut messages = Vec::new();

        schedule_immediate_command(
            &mut scheduler,
            12,
            ScheduledCommand::PlaySample {
                id: 0,
                volume: 1.0,
                received_at_ns: None,
            },
            &mut mixer,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert_eq!(active_voice_frame(&mixer, 0), Some(0));
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn immediate_command_falls_back_when_scheduler_is_full() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut scheduler = FixedCapacityScheduler::<0>::new();
        let mut transport = TransportTimeline::new(44_100);
        let mut messages = Vec::new();

        schedule_immediate_command(
            &mut scheduler,
            12,
            ScheduledCommand::PlaySample {
                id: 0,
                volume: 1.0,
                received_at_ns: None,
            },
            &mut mixer,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_schedules_supported_grid_and_renders_at_target_offset() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(4);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(5));
        assert!(mixer.voices.iter().all(|voice| !voice.active));
        assert!(messages.is_empty());

        let mut output = vec![0.0; 4];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            callback_start_frame,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert_eq!(output[0], 0.0);
        assert!(
            output[1..]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-5)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_schedules_selected_subdivision_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(7);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(8));
        assert!(mixer.voices.iter().all(|voice| !voice.active));
        assert!(messages.is_empty());
    }

    #[test]
    fn quantized_play_starts_at_loop_start_even_with_phase_metadata() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_loop_region(0, 0.7, Some(5.0));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 2.0,
            },
        );
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(10);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert_eq!(active_voice_frame(&mixer, 0), Some(7));
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_without_pad_metadata_falls_back_to_loop_start() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_loop_region(0, 0.7, Some(5.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(10);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert_eq!(active_voice_frame(&mixer, 0), Some(7));
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_schedules_future_sixteenth_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(4);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(5));
        assert!(mixer.voices.iter().all(|voice| !voice.active));
        assert!(messages.is_empty());
    }

    #[test]
    fn quantized_play_on_grid_boundary_executes_at_current_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(10);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_without_master_bpm_falls_back_to_immediate() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        transport.clear_master_bpm();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            transport.output_frame(),
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_uses_global_masterclock_not_active_pad_phase() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.load_sample(1, create_test_sample(1, 64, 0.25));
        for id in [0, 1] {
            mixer.set_pad_bpm(id, Some(60.0));
            mixer.set_pad_timing_metadata(
                id,
                PadTimingMetadata {
                    phase_anchor_s: 0.0,
                },
            );
        }
        assert!(mixer.play_sample(0, 1.0));

        let mut rendered = vec![0.0; 6];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut rendered, &mut pad_peaks);

        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(6);
        transport.set_downbeat_frame(0);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(8));
        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 0);
        assert_eq!(active_voice_frame(&mixer, 1), None);
        assert!(messages.is_empty());
    }

    #[test]
    fn quantized_late_click_waits_for_future_grid_and_starts_at_loop_start() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.25));
        mixer.set_pad_loop_region(0, 0.7, Some(5.0));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 0.0,
            },
        );
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(6);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(8));
        assert_eq!(active_voice_frame(&mixer, 0), None);

        let mut output = vec![0.0; 3];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            callback_start_frame,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(output[..2].iter().all(|sample| *sample == 0.0));
        assert!((output[2] - 0.25).abs() < 1e-5);
        assert_eq!(active_voice_frame(&mixer, 0), Some(8));
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn stopping_first_started_pad_keeps_masterclock_phase_for_future_triggers() {
        let mut mixer = RtMixer::new(1, 10.0);
        for id in 0..3 {
            mixer.load_sample(id, create_test_sample(1, 64, 0.25));
        }
        assert!(mixer.play_sample(0, 1.0));
        assert!(mixer.play_sample(1, 1.0));

        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.set_downbeat_frame(0);
        transport.advance_by_rendered_frames(6);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_immediate_command(
            &mut scheduler,
            callback_start_frame,
            ScheduledCommand::StopSample { id: 0 },
            &mut mixer,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 0);
        assert_eq!(active_voice_frame(&mixer, 0), None);
        assert!(active_voice_frame(&mixer, 1).is_some());

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            2,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(8));
        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 0);
        assert_eq!(active_voice_frame(&mixer, 2), None);
    }

    #[test]
    fn quantized_two_bar_later_trigger_preserves_global_offset() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.load_sample(1, create_test_sample(1, 64, 0.25));
        mixer.set_pad_loop_region(1, 0.7, Some(5.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            0,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert_eq!(active_voice_frame(&mixer, 0), Some(0));

        transport.advance_by_rendered_frames(80);
        let callback_start_frame = transport.output_frame();
        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(callback_start_frame, 80);
        assert!(scheduler.is_empty());
        assert!(active_voice_frame(&mixer, 0).is_some());
        assert_eq!(active_voice_frame(&mixer, 1), Some(7));
    }

    #[test]
    fn multi_loop_remains_stable_when_any_one_of_five_pads_stops() {
        let mut mixer = RtMixer::new(1, 10.0);
        for id in 0..6 {
            mixer.load_sample(id, create_test_sample(1, 64, 0.25));
        }
        for id in 0..5 {
            assert!(mixer.play_sample(id, 1.0));
        }

        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(6);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_immediate_command(
            &mut scheduler,
            callback_start_frame,
            ScheduledCommand::StopSample { id: 2 },
            &mut mixer,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );
        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            5,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(8));
        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 0);
        for id in [0, 1, 3, 4] {
            assert!(active_voice_frame(&mixer, id).is_some());
        }
        assert_eq!(active_voice_frame(&mixer, 2), None);
        assert_eq!(active_voice_frame(&mixer, 5), None);
    }

    #[test]
    fn bootstrap_retains_accepted_master_bpm_even_when_reference_rate_is_clipped() {
        for master_bpm in [123.45, 400.0] {
            let mut mixer = RtMixer::new(1, 10.0);
            mixer.load_sample(0, create_test_sample(1, 200, 0.5));
            mixer.set_pad_bpm(0, Some(87.65));
            mixer.set_master_bpm(master_bpm);
            mixer.set_bpm_lock(true);
            assert!(mixer.play_sample(0, 1.0));
            let mut transport = TransportTimeline::new(10);
            transport.request_bootstrap(0);
            assert!(try_bootstrap_transport_from_reference(
                &mixer,
                &mut transport,
                0
            ));
            assert_eq!(transport.master_bpm(), Some(master_bpm));
        }
    }

    #[test]
    fn deliberate_sync_retains_complete_source_beat_and_consumes_bootstrap() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 200, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_loop_region(0, 18.5, Some(19.0));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        transport.advance_by_rendered_frames(100);
        transport.request_bootstrap(1);
        assert!(anchor_transport_phase_from_pad(&mixer, &mut transport, 0));
        assert_eq!(transport.beat_position(), Some(18.5));
        assert_eq!(transport.bar_phase_beats(), Some(2.5));
        transport.request_bootstrap(0);
        assert_eq!(transport.bootstrap_reference(), None);
        assert!(!try_bootstrap_transport_from_reference(
            &mixer,
            &mut transport,
            100
        ));
    }

    #[test]
    fn reference_bootstrap_uses_same_loop_edit_position_as_renderer() {
        for (rendered, expected_start) in [(30, 30), (80, 20)] {
            let mut mixer = RtMixer::new(1, 10.0);
            mixer.load_sample(0, create_test_sample(1, 200, 0.5));
            mixer.set_master_bpm(60.0);
            mixer.set_pad_bpm(0, Some(60.0));
            mixer.set_bpm_lock(true);
            assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
            let mut scheduler = FixedCapacityScheduler::<8>::new();
            let mut transport = TransportTimeline::new(10);
            transport.set_master_bpm(60.0);
            let mut messages = Vec::new();
            let mut peaks = [0.0; NUM_SAMPLES];
            render_scheduled_audio(
                &mut mixer,
                &mut scheduler,
                &mut vec![0.0; rendered],
                &mut peaks,
                0,
                1,
                &mut transport,
                &mut messages,
                &mut ImmediateAudioBufferRetirement,
            );
            assert_eq!(active_voice_frame(&mixer, 0), Some(rendered));
            mixer.set_pad_loop_region(0, 2.0, Some(6.0));
            transport.request_bootstrap(0);
            assert!(try_bootstrap_transport_from_reference(
                &mixer,
                &mut transport,
                rendered as u64
            ));
            assert_eq!(
                transport.beat_position_at_frame(rendered as u64),
                Some(expected_start as f64 / 10.0)
            );
            render_scheduled_audio(
                &mut mixer,
                &mut scheduler,
                &mut [0.0; 1],
                &mut peaks,
                rendered as u64,
                1,
                &mut transport,
                &mut messages,
                &mut ImmediateAudioBufferRetirement,
            );
            assert_eq!(active_voice_frame(&mixer, 0), Some(expected_start + 1));
        }
    }

    #[test]
    fn selected_bootstrap_waits_for_parameter_batch_and_keeps_other_playheads() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 200, 0.5));
        mixer.load_sample(1, create_test_sample(1, 200, 0.25));
        mixer.set_pad_loop_region(0, 1.0, Some(9.0));
        mixer.set_pad_loop_region(1, 2.0, Some(10.0));
        assert!(mixer.play_sample(0, 1.0));
        assert!(mixer.play_sample(1, 1.0));
        let mut transport = TransportTimeline::new(10);
        transport.advance_by_rendered_frames(100);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut quantization = TriggerQuantization::Immediate;
        let mut messages = Vec::new();
        for command in [
            ControlMessage::SetBpmLock(true),
            ControlMessage::SetPadTimingMetadata {
                id: 0,
                metadata: PadTimingMetadata {
                    phase_anchor_s: -0.5,
                },
            },
            ControlMessage::BootstrapTransportFromPad { id: 0 },
        ] {
            process_control_message(
                command,
                &mut scheduler,
                100,
                &mut quantization,
                &mut transport,
                &mut mixer,
                &mut messages,
                &mut ImmediateAudioBufferRetirement,
            );
        }
        assert_eq!(transport.master_bpm(), Some(120.0));
        assert_eq!(transport.beat_position(), Some(20.0));
        assert!(!try_bootstrap_transport_from_reference(
            &mixer,
            &mut transport,
            100
        ));
        let (mut parameters, mut consumer) = RingBuffer::new(8);
        parameters
            .push(ControlParameterMessage::SetPadBpm {
                id: 0,
                bpm: Some(60.0),
            })
            .unwrap();
        parameters
            .push(ControlParameterMessage::SetMasterBpm(90.0))
            .unwrap();
        drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
        assert!(try_bootstrap_transport_from_reference(
            &mixer,
            &mut transport,
            100
        ));
        assert_eq!(transport.master_bpm(), Some(90.0));
        assert_eq!(transport.beat_position(), Some(1.5));
        assert_eq!(active_voice_frame(&mixer, 0), Some(10));
        assert_eq!(active_voice_frame(&mixer, 1), Some(20));
        // Subsequent edits, silence and restarts cannot consume another bootstrap.
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 8.0,
            },
        );
        mixer.stop_sample(0);
        mixer.stop_sample(1);
        assert!(mixer.play_sample(0, 1.0));
        transport.request_bootstrap(0);
        assert!(!try_bootstrap_transport_from_reference(
            &mixer,
            &mut transport,
            100
        ));
        assert_eq!(transport.beat_position(), Some(1.5));
    }

    #[test]
    fn stopped_selected_reference_bootstraps_at_its_scheduled_start_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 200, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_master_bpm(60.0);
        mixer.set_bpm_lock(true);
        mixer.set_pad_loop_region(0, 3.0, Some(11.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: -0.5,
            },
        );
        let mut transport = TransportTimeline::new(10);
        transport.set_master_bpm(60.0);
        transport.advance_by_rendered_frames(4);
        transport.request_bootstrap(0);
        assert!(!try_bootstrap_transport_from_reference(
            &mixer,
            &mut transport,
            4
        ));
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        scheduler
            .schedule(
                10,
                ScheduledCommand::PlaySample {
                    id: 0,
                    volume: 1.0,
                    received_at_ns: Some(123),
                },
            )
            .unwrap();
        let mut output = [0.0; 8];
        let mut peaks = [0.0; NUM_SAMPLES];
        let mut messages = Vec::new();
        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut peaks,
            4,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );
        assert_eq!(transport.beat_position_at_frame(10), Some(3.5));
        assert_eq!(transport.output_frame(), 4);
        assert_eq!(active_voice_frame(&mixer, 0), Some(32));
        assert_eq!(transport.bootstrap_reference(), None);
    }

    #[test]
    fn pending_start_retains_target_and_capture_across_bpm_and_grid_edits() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 200, 0.5));
        mixer.set_pad_loop_region(0, 2.0, Some(10.0));
        let mut transport = TransportTimeline::new(10);
        transport.set_master_bpm(60.0);
        transport.advance_by_rendered_frames(4);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let command = ScheduledCommand::PlaySample {
            id: 0,
            volume: 1.0,
            received_at_ns: Some(123),
        };
        let accepted = scheduler
            .schedule(
                transport
                    .next_grid_frame(QuantizeGrid::from_step_64ths(16).unwrap())
                    .unwrap(),
                command,
            )
            .unwrap();
        assert_eq!(accepted.target_frame, 10);
        assert!(transport.set_master_bpm_preserving_beat_position_at_frame(120.0, 4));
        mixer.set_master_bpm(120.0);
        mixer.set_pad_bpm(0, Some(80.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: -0.5,
            },
        );
        mixer.set_pad_loop_region(0, 1.0, Some(9.0));
        assert_eq!(scheduler.peek_next_target_frame(), Some(10));
        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(16).unwrap()),
            Some(7)
        );
        let due = scheduler.pop_due_through(4, 10).unwrap();
        assert_eq!(due.target_frame, accepted.target_frame);
        assert_eq!(due.command, command);
        let mut messages = Vec::new();
        execute_scheduled_command(
            &mut mixer,
            &mut transport,
            due.execution_frame,
            due.command,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );
        assert_eq!(active_voice_frame(&mixer, 0), Some(10));
    }

    #[test]
    fn bpm_lock_phase_anchor_updates_transport_downbeat_from_active_pad() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 0.5,
            },
        );
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 30];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(100);

        assert!(anchor_transport_phase_from_pad(&mixer, &mut transport, 0));

        assert_eq!(transport.downbeat_frame(), 75);
        assert_eq!(transport.bar_phase_beats(), Some(2.5));
    }

    #[test]
    fn bpm_lock_phase_anchor_keeps_transport_downbeat_when_anchor_is_inactive() {
        let mixer = RtMixer::new(1, 10.0);
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.set_downbeat_frame(12);
        transport.advance_by_rendered_frames(100);

        assert!(!anchor_transport_phase_from_pad(&mixer, &mut transport, 0));

        assert_eq!(transport.downbeat_frame(), 12);
    }

    #[test]
    fn phase_anchor_establishes_transport_clock_without_existing_master_bpm() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        transport.clear_master_bpm();
        transport.set_downbeat_frame(12);

        assert!(anchor_transport_phase_from_pad(&mixer, &mut transport, 0));

        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 0);
    }

    #[test]
    fn scheduler_full_quantized_play_leaves_current_playback_unchanged() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(4);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<0>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 1)
        );
        assert!(messages.is_empty());
    }

    #[test]
    fn immediate_exclusive_play_stops_all_then_starts_at_current_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        mixer.set_pad_bpm(1, Some(60.0));
        mixer.set_pad_timing_metadata(
            1,
            PadTimingMetadata {
                phase_anchor_s: 2.0,
            },
        );
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            transport.output_frame(),
            TriggerQuantization::Immediate,
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 1)
        );
        assert_eq!(active_voice_frame(&mixer, 1), Some(0));
        assert_stopped(&messages, 0, 0);
        assert_started(&messages, 1, 1);
    }

    #[test]
    fn quantized_exclusive_play_starts_target_at_loop_start() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.load_sample(1, create_test_sample(1, 64, 0.25));
        mixer.set_pad_loop_region(1, 0.7, Some(5.0));
        mixer.set_pad_bpm(1, Some(60.0));
        mixer.set_pad_timing_metadata(
            1,
            PadTimingMetadata {
                phase_anchor_s: 2.0,
            },
        );
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(40);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 0)
        );
        assert_eq!(active_voice_frame(&mixer, 1), Some(7));
        assert_stopped(&messages, 0, 0);
        assert_started(&messages, 1, 1);
    }

    #[test]
    fn quantized_exclusive_play_switches_pads_at_target_offset() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(4);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(5));
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 1)
        );
        assert!(messages.is_empty());

        let mut output = vec![0.0; 4];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            callback_start_frame,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!((output[0] - 0.5).abs() < 1e-5);
        assert!(
            output[1..]
                .iter()
                .all(|sample| (*sample - 0.25).abs() < 1e-5)
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 1)
        );
        assert_stopped(&messages, 0, 0);
        assert_started(&messages, 1, 1);
    }

    #[test]
    fn scheduler_full_quantized_exclusive_play_leaves_current_playback_unchanged() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(5);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<0>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 1)
        );
        assert!(messages.is_empty());
    }

    #[test]
    fn exclusive_play_rejects_missing_target_without_stopping_current_playback() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            transport.output_frame(),
            TriggerQuantization::Immediate,
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 1)
        );
        assert!(messages.is_empty());
    }

    #[test]
    fn scheduled_start_inside_buffer_renders_at_target_offset() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler
            .schedule(
                4,
                ScheduledCommand::PlaySample {
                    id: 0,
                    volume: 1.0,
                    received_at_ns: None,
                },
            )
            .unwrap();
        let mut output = vec![0.0; 8];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let mut messages = Vec::new();

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            0,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(output[..4].iter().all(|sample| *sample == 0.0));
        assert!(
            output[4..]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-5)
        );
        assert!((pad_peaks[0] - 0.5).abs() < 1e-5);
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn scheduled_start_inside_oversized_buffer_preserves_target_offset() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 1_000, 0.5));
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler
            .schedule(
                600,
                ScheduledCommand::PlaySample {
                    id: 0,
                    volume: 1.0,
                    received_at_ns: None,
                },
            )
            .unwrap();
        let mut output = vec![0.0; 700];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let mut messages = Vec::new();

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            0,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(output[..600].iter().all(|sample| *sample == 0.0));
        assert!(
            output[600..]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-5)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn scheduled_stop_inside_buffer_silences_after_target_offset() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.play_sample(0, 1.0);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler
            .schedule(4, ScheduledCommand::StopSample { id: 0 })
            .unwrap();
        let mut output = vec![0.0; 8];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let mut messages = Vec::new();

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            0,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(
            output[..4]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-5)
        );
        assert!(output[4..].iter().all(|sample| *sample == 0.0));
        assert!((pad_peaks[0] - 0.5).abs() < 1e-5);
        assert_stopped(&messages, 0, 0);
    }

    #[test]
    fn scheduled_render_tracks_pad_activity_across_split_segments() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.play_sample(0, 1.0);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler
            .schedule(4, ScheduledCommand::StopSample { id: 0 })
            .unwrap();
        let mut output = vec![0.0; 8];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let mut pad_activity = RtRenderPadActivity::default();
        let mut messages = Vec::new();

        render_scheduled_audio_tracking_pads(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            &mut pad_activity,
            0,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(pad_activity.contains(0));
        assert_eq!(pad_activity.len(), 1);
        assert!((pad_peaks[0] - 0.5).abs() < 1e-5);
        assert_eq!(mixer.pad_playhead_seconds(0), None);
        assert_stopped(&messages, 0, 0);
    }

    #[test]
    fn same_frame_stop_all_and_start_preserve_stable_order() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.8));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        mixer.play_sample(0, 1.0);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler.schedule(0, ScheduledCommand::StopAll).unwrap();
        scheduler
            .schedule(
                0,
                ScheduledCommand::PlaySample {
                    id: 1,
                    volume: 1.0,
                    received_at_ns: None,
                },
            )
            .unwrap();
        let mut output = vec![0.0; 4];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let mut messages = Vec::new();

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            0,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 1)
        );
        assert!(output.iter().all(|sample| (*sample - 0.25).abs() < 1e-5));
        assert_stopped(&messages, 0, 0);
        assert_started(&messages, 1, 1);
    }
}
