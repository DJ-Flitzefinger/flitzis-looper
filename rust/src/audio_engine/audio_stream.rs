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
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::{
    FixedCapacityScheduler, ScheduledCommand, TransportScheduler,
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
) {
    let command = ScheduledCommand::PlaySample { id, volume };

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
) {
    let command = ScheduledCommand::StopAllThenPlaySample { id, volume };

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
    let Some(bar_phase_beats) = mixer.active_pad_bar_phase_beats(id) else {
        return false;
    };
    let Some(bpm) = mixer.output_bpm_for_sample_id(id) else {
        return transport.anchor_downbeat_to_bar_phase_at_frame(bar_phase_beats, output_frame);
    };

    transport.set_master_bpm_and_anchor_bar_phase_at_frame(bpm, bar_phase_beats, output_frame)
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
        ScheduledCommand::PlaySample { id, volume } => {
            let started =
                mixer.play_sample_at_output_frame_rt(id, volume, output_frame, retirement);

            if started {
                audio_messages.push_audio_message(AudioMessage::SampleStarted { id });
            } else {
                audio_messages.push_audio_message(AudioMessage::SampleStopped { id });
            }
        }
        ScheduledCommand::StopAllThenPlaySample { id, volume } => {
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
    output.fill(0.0);
    pad_peaks.fill(0.0);

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
        let Some(next_target_frame) = scheduler.peek_next_target_frame() else {
            render_mixer_segment(
                mixer,
                output,
                pad_peaks,
                &mut segment_peaks,
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

    mixer.render_rt_at_output_frame(
        &mut output[start..end],
        segment_peaks,
        segment_start_frame,
        retirement,
    );

    for (peak, segment_peak) in pad_peaks.iter_mut().zip(segment_peaks.iter()) {
        *peak = (*peak).max(*segment_peak);
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

fn control_message_retirement_slots_needed(message: &ControlMessage) -> usize {
    match message {
        ControlMessage::LoadSample { .. } | ControlMessage::PublishPreparedStems { .. } => 2,
        ControlMessage::StopSample { .. } => MAX_VOICES,
        ControlMessage::UnloadSample { .. } => MAX_VOICES + 2,
        ControlMessage::StopAll() | ControlMessage::PlaySampleExclusive { .. } => MAX_VOICES,
        _ => 0,
    }
}

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

struct PendingControlParameters {
    volume: Option<f32>,
    speed: Option<f32>,
    master_bpm: Option<f32>,
    pad_bpm: [Option<Option<f32>>; NUM_SAMPLES],
    pad_gain_db: [Option<f32>; NUM_SAMPLES],
    pad_eq: [Option<(f32, f32, f32)>; NUM_SAMPLES],
}

impl Default for PendingControlParameters {
    fn default() -> Self {
        Self {
            volume: None,
            speed: None,
            master_bpm: None,
            pad_bpm: [None; NUM_SAMPLES],
            pad_gain_db: [None; NUM_SAMPLES],
            pad_eq: [None; NUM_SAMPLES],
        }
    }
}

impl PendingControlParameters {
    fn record(&mut self, message: ControlParameterMessage) {
        match message {
            ControlParameterMessage::SetVolume(volume) => self.volume = Some(volume),
            ControlParameterMessage::SetSpeed(speed) => self.speed = Some(speed),
            ControlParameterMessage::SetMasterBpm(bpm) => self.master_bpm = Some(bpm),
            ControlParameterMessage::SetPadBpm { id, bpm } => {
                if let Some(slot) = self.pad_bpm.get_mut(id) {
                    *slot = Some(bpm);
                }
            }
            ControlParameterMessage::SetPadGain { id, gain_db } => {
                if let Some(slot) = self.pad_gain_db.get_mut(id) {
                    *slot = Some(gain_db);
                }
            }
            ControlParameterMessage::SetPadEq {
                id,
                low_db,
                mid_db,
                high_db,
            } => {
                if let Some(slot) = self.pad_eq.get_mut(id) {
                    *slot = Some((low_db, mid_db, high_db));
                }
            }
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
            transport.set_master_bpm_preserving_bar_phase_at_frame(bpm, transport.output_frame());
            applied += 1;
        }
        for (id, bpm) in self.pad_bpm.into_iter().enumerate() {
            if let Some(bpm) = bpm {
                mixer.set_pad_bpm(id, bpm);
                applied += 1;
            }
        }
        for (id, gain_db) in self.pad_gain_db.into_iter().enumerate() {
            if let Some(gain_db) = gain_db {
                mixer.set_pad_gain(id, gain_db);
                applied += 1;
            }
        }
        for (id, eq) in self.pad_eq.into_iter().enumerate() {
            if let Some((low_db, mid_db, high_db)) = eq {
                mixer.set_pad_eq(id, low_db, mid_db, high_db);
                applied += 1;
            }
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
        ControlMessage::PlaySample { id, volume } => {
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
            );
        }
        ControlMessage::PlaySampleExclusive { id, volume } => {
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
        ControlMessage::SetPadTimingMetadata { id, metadata } => {
            mixer.set_pad_timing_metadata(id, metadata);
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
pub fn create_audio_stream() -> Result<AudioStreamHandle, Box<dyn std::error::Error>> {
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

    let mut mixer = RtMixer::new(channels as usize, sample_rate_hz as f32);
    let mut transport = TransportTimeline::new(sample_rate_hz);
    let mut scheduler = TransportScheduler::new();
    let mut trigger_quantization = TriggerQuantization::Immediate;
    let (mut retired_buffers, retirement_worker) = create_audio_buffer_retirement();

    let emit_interval_frames: u64 = (sample_rate_hz as u64 / 10).max(1);
    let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
    let mut last_emit_frame = [0_u64; NUM_SAMPLES];
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
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let buffer_start_frame = transport.output_frame();

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

            // Render audio + compute per-pad peaks.
            render_scheduled_audio(
                &mut mixer,
                &mut scheduler,
                data,
                &mut pad_peaks,
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

            for (id, peak) in pad_peaks.iter().enumerate() {
                if frame_clock.wrapping_sub(last_emit_frame[id]) < emit_interval_frames {
                    continue;
                }

                last_emit_frame[id] = frame_clock;

                if *peak > 0.0 && peak.is_finite() {
                    let peak = peak.clamp(0.0, 1.0);
                    let _ = producer_out.push(AudioMessage::PadPeak { id, peak });
                }

                if let Some(position_s) = mixer.pad_playhead_seconds(id)
                    && position_s.is_finite()
                {
                    let _ = producer_out.push(AudioMessage::PadPlayhead { id, position_s });
                }
            }

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

        let result = create_audio_stream();
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
            ScheduledCommand::PlaySample { id: 0, volume: 1.0 },
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
            ScheduledCommand::PlaySample { id: 0, volume: 1.0 },
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
            .schedule(4, ScheduledCommand::PlaySample { id: 0, volume: 1.0 })
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
            .schedule(600, ScheduledCommand::PlaySample { id: 0, volume: 1.0 })
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
    fn same_frame_stop_all_and_start_preserve_stable_order() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.8));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        mixer.play_sample(0, 1.0);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler.schedule(0, ScheduledCommand::StopAll).unwrap();
        scheduler
            .schedule(0, ScheduledCommand::PlaySample { id: 1, volume: 1.0 })
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
