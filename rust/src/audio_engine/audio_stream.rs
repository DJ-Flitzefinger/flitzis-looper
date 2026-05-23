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

use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::{
    FixedCapacityScheduler, ScheduledCommand, TransportScheduler,
};
use crate::audio_engine::transport::{QuantizeGrid, TransportTimeline};
use crate::messages::{AudioMessage, ControlMessage, TriggerQuantization};

/// Handle to the audio stream with associated message channels
pub struct AudioStreamHandle {
    pub stream: Stream,
    pub producer: Arc<Mutex<Producer<ControlMessage>>>,
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

fn schedule_immediate_command<const CAPACITY: usize, S: AudioMessageSink>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    command: ScheduledCommand,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
) {
    if scheduler.schedule(callback_start_frame, command).is_ok() {
        drain_scheduler_due_at_callback_start(
            scheduler,
            callback_start_frame,
            mixer,
            audio_messages,
        );
    } else {
        execute_scheduled_command(mixer, command, audio_messages);
    }
}

fn schedule_play_sample_command<const CAPACITY: usize, S: AudioMessageSink>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: TriggerQuantization,
    transport: &TransportTimeline,
    id: usize,
    volume: f32,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
) {
    let command = ScheduledCommand::PlaySample {
        id,
        volume,
        target_bar_phase_beats: None,
    };

    let Some(target_frame) = quantized_target_frame(transport, trigger_quantization) else {
        schedule_immediate_command(
            scheduler,
            callback_start_frame,
            command,
            mixer,
            audio_messages,
        );
        return;
    };

    let command = ScheduledCommand::PlaySample {
        id,
        volume,
        target_bar_phase_beats: transport.bar_phase_beats_at_frame(target_frame),
    };

    if scheduler.schedule(target_frame, command).is_ok() {
        drain_scheduler_due_at_callback_start(
            scheduler,
            callback_start_frame,
            mixer,
            audio_messages,
        );
    }
}

fn schedule_exclusive_play_sample_command<const CAPACITY: usize, S: AudioMessageSink>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: TriggerQuantization,
    transport: &TransportTimeline,
    id: usize,
    volume: f32,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
) {
    let command = ScheduledCommand::StopAllThenPlaySample {
        id,
        volume,
        target_bar_phase_beats: None,
    };

    let Some(target_frame) = quantized_target_frame(transport, trigger_quantization) else {
        schedule_immediate_command(
            scheduler,
            callback_start_frame,
            command,
            mixer,
            audio_messages,
        );
        return;
    };

    let command = ScheduledCommand::StopAllThenPlaySample {
        id,
        volume,
        target_bar_phase_beats: transport.bar_phase_beats_at_frame(target_frame),
    };

    if scheduler.schedule(target_frame, command).is_ok() {
        drain_scheduler_due_at_callback_start(
            scheduler,
            callback_start_frame,
            mixer,
            audio_messages,
        );
    }
}

fn quantized_target_frame(
    transport: &TransportTimeline,
    trigger_quantization: TriggerQuantization,
) -> Option<u64> {
    match trigger_quantization {
        TriggerQuantization::Immediate => None,
        TriggerQuantization::NextBeat => transport.next_grid_frame(QuantizeGrid::Beat),
        TriggerQuantization::NextBar => transport.next_grid_frame(QuantizeGrid::Bar),
    }
}

fn anchor_transport_phase_from_pad(
    mixer: &RtMixer,
    transport: &mut TransportTimeline,
    id: usize,
) -> bool {
    let Some(bar_phase_beats) = mixer.active_pad_bar_phase_beats(id) else {
        return false;
    };

    transport.anchor_downbeat_to_bar_phase(bar_phase_beats)
}

fn drain_scheduler_due_at_callback_start<const CAPACITY: usize, S: AudioMessageSink>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
) {
    while let Some(event) = scheduler.pop_due_at_callback_start(callback_start_frame) {
        execute_scheduled_command(mixer, event.command, audio_messages);
    }
}

fn execute_scheduled_command<S: AudioMessageSink>(
    mixer: &mut RtMixer,
    command: ScheduledCommand,
    audio_messages: &mut S,
) {
    match command {
        ScheduledCommand::PlaySample {
            id,
            volume,
            target_bar_phase_beats,
        } => {
            let started = if let Some(phase) = target_bar_phase_beats {
                mixer.play_sample_phase_aligned(id, volume, phase)
            } else {
                mixer.play_sample(id, volume)
            };

            if started {
                audio_messages.push_audio_message(AudioMessage::SampleStarted { id });
            } else {
                audio_messages.push_audio_message(AudioMessage::SampleStopped { id });
            }
        }
        ScheduledCommand::StopAllThenPlaySample {
            id,
            volume,
            target_bar_phase_beats,
        } => {
            if !mixer.can_play_sample(id, volume) {
                return;
            }

            stop_all_samples(mixer, audio_messages);
            let started = if let Some(phase) = target_bar_phase_beats {
                mixer.play_sample_phase_aligned(id, volume, phase)
            } else {
                mixer.play_sample(id, volume)
            };

            if started {
                audio_messages.push_audio_message(AudioMessage::SampleStarted { id });
            }
        }
        ScheduledCommand::StopSample { id } => {
            mixer.stop_sample(id);
            audio_messages.push_audio_message(AudioMessage::SampleStopped { id });
        }
        ScheduledCommand::StopAll => {
            stop_all_samples(mixer, audio_messages);
        }
    }
}

fn stop_all_samples<S: AudioMessageSink>(mixer: &mut RtMixer, audio_messages: &mut S) {
    for voice in &mut mixer.voices {
        if voice.active {
            voice.stop();
            audio_messages.push_audio_message(AudioMessage::SampleStopped {
                id: voice.sample_id,
            });
        }
    }
}

fn render_scheduled_audio<const CAPACITY: usize, S: AudioMessageSink>(
    mixer: &mut RtMixer,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    output: &mut [f32],
    pad_peaks: &mut [f32; NUM_SAMPLES],
    callback_start_frame: u64,
    channels: usize,
    audio_messages: &mut S,
) {
    output.fill(0.0);
    pad_peaks.fill(0.0);

    drain_scheduler_due_at_callback_start(scheduler, callback_start_frame, mixer, audio_messages);

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
            );
            rendered_until_frame = next_target_frame;
        }

        while let Some(event) =
            scheduler.pop_due_through(callback_start_frame, rendered_until_frame)
        {
            execute_scheduled_command(mixer, event.command, audio_messages);
        }
    }
}

fn render_mixer_segment(
    mixer: &mut RtMixer,
    output: &mut [f32],
    pad_peaks: &mut [f32; NUM_SAMPLES],
    segment_peaks: &mut [f32; NUM_SAMPLES],
    callback_start_frame: u64,
    segment_start_frame: u64,
    segment_end_frame: u64,
    channels: usize,
) {
    if segment_end_frame <= segment_start_frame {
        return;
    }

    let start_frame = (segment_start_frame - callback_start_frame) as usize;
    let end_frame = (segment_end_frame - callback_start_frame) as usize;
    let start = start_frame * channels;
    let end = end_frame * channels;

    mixer.render(&mut output[start..end], segment_peaks);

    for (peak, segment_peak) in pad_peaks.iter_mut().zip(segment_peaks.iter()) {
        *peak = (*peak).max(*segment_peak);
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

    // Create ring buffer for outgoing messages (Rust->Python)
    let (mut producer_out, consumer_out) = RingBuffer::new(1024);

    let mut mixer = RtMixer::new(channels as usize, sample_rate_hz as f32);
    let mut transport = TransportTimeline::new(sample_rate_hz);
    let mut scheduler = TransportScheduler::new();
    let mut trigger_quantization = TriggerQuantization::Immediate;

    let emit_interval_frames: u64 = (sample_rate_hz as u64 / 10).max(1);
    let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
    let mut last_emit_frame = [0_u64; NUM_SAMPLES];

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

            // Process incoming messages in real-time
            while let Ok(message) = consumer_in.pop() {
                match message {
                    ControlMessage::Ping() => {
                        producer_out.push_audio_message(AudioMessage::Pong());
                    }
                    ControlMessage::LoadSample { id, sample } => {
                        mixer.load_sample(id, sample);
                    }
                    ControlMessage::PlaySample { id, volume } => {
                        schedule_play_sample_command(
                            &mut scheduler,
                            buffer_start_frame,
                            trigger_quantization,
                            &transport,
                            id,
                            volume,
                            &mut mixer,
                            &mut producer_out,
                        );
                    }
                    ControlMessage::PlaySampleExclusive { id, volume } => {
                        schedule_exclusive_play_sample_command(
                            &mut scheduler,
                            buffer_start_frame,
                            trigger_quantization,
                            &transport,
                            id,
                            volume,
                            &mut mixer,
                            &mut producer_out,
                        );
                    }
                    ControlMessage::StopSample { id } => {
                        schedule_immediate_command(
                            &mut scheduler,
                            buffer_start_frame,
                            ScheduledCommand::StopSample { id },
                            &mut mixer,
                            &mut producer_out,
                        );
                    }
                    ControlMessage::StopAll() => {
                        schedule_immediate_command(
                            &mut scheduler,
                            buffer_start_frame,
                            ScheduledCommand::StopAll,
                            &mut mixer,
                            &mut producer_out,
                        );
                    }
                    ControlMessage::UnloadSample { id } => {
                        mixer.unload_sample(id);
                    }
                    ControlMessage::SetSpeed(speed) => {
                        mixer.set_speed(speed);
                    }
                    ControlMessage::SetBpmLock(enabled) => {
                        mixer.set_bpm_lock(enabled);
                        if !enabled {
                            transport.clear_master_bpm();
                        }
                    }
                    ControlMessage::SetKeyLock(enabled) => {
                        mixer.set_key_lock(enabled);
                    }
                    ControlMessage::SetMasterBpm(bpm) => {
                        transport.set_master_bpm(bpm);
                        mixer.set_master_bpm(bpm);
                    }
                    ControlMessage::SetPadBpm { id, bpm } => {
                        mixer.set_pad_bpm(id, bpm);
                    }
                    ControlMessage::SetPadTimingMetadata { id, metadata } => {
                        mixer.set_pad_timing_metadata(id, metadata);
                    }
                    ControlMessage::AnchorTransportPhaseFromPad { id } => {
                        anchor_transport_phase_from_pad(&mixer, &mut transport, id);
                    }
                    ControlMessage::SetPadGain { id, gain } => {
                        mixer.set_pad_gain(id, gain);
                    }
                    ControlMessage::SetPadEq {
                        id,
                        low_db,
                        mid_db,
                        high_db,
                    } => {
                        mixer.set_pad_eq(id, low_db, mid_db, high_db);
                    }
                    ControlMessage::SetPadLoopRegion { id, start_s, end_s } => {
                        mixer.set_pad_loop_region(id, start_s, end_s);
                    }
                    ControlMessage::SetTriggerQuantization(mode) => {
                        trigger_quantization = mode;
                    }
                    ControlMessage::PauseSample { id } => {
                        mixer.pause_sample(id);
                    }
                    ControlMessage::ResumeSample { id } => {
                        mixer.resume_sample(id);
                    }
                    ControlMessage::SetVolume(volume) => {
                        mixer.set_volume(volume);
                    }
                }
            }

            // Render audio + compute per-pad peaks.
            render_scheduled_audio(
                &mut mixer,
                &mut scheduler,
                data,
                &mut pad_peaks,
                buffer_start_frame,
                channels as usize,
                &mut producer_out,
            );

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
        },
        |err| {
            log::error!("Audio stream error: {}", err);
        },
        None,
    )?;

    Ok(AudioStreamHandle {
        stream,
        producer: Arc::new(Mutex::new(producer_in)),
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
        let mut messages = Vec::new();

        schedule_immediate_command(
            &mut scheduler,
            12,
            ScheduledCommand::PlaySample {
                id: 0,
                volume: 1.0,
                target_bar_phase_beats: None,
            },
            &mut mixer,
            &mut messages,
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
        let mut messages = Vec::new();

        schedule_immediate_command(
            &mut scheduler,
            12,
            ScheduledCommand::PlaySample {
                id: 0,
                volume: 1.0,
                target_bar_phase_beats: None,
            },
            &mut mixer,
            &mut messages,
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
    fn quantized_play_schedules_next_beat_and_renders_at_target_offset() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(5);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::NextBeat,
            &transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(10));
        assert!(mixer.voices.iter().all(|voice| !voice.active));
        assert!(messages.is_empty());

        let mut output = vec![0.0; 10];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            callback_start_frame,
            1,
            &mut messages,
        );

        assert!(output[..5].iter().all(|sample| *sample == 0.0));
        assert!(
            output[5..]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-5)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_uses_phase_aligned_initial_frame_at_target_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 0.0,
            },
        );
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(5);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::NextBeat,
            &transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(10));

        let mut output = vec![0.0; 10];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            callback_start_frame,
            1,
            &mut messages,
        );

        assert_eq!(active_voice_frame(&mixer, 0), Some(15));
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
            TriggerQuantization::NextBeat,
            &transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
        );

        assert!(scheduler.is_empty());
        assert_eq!(active_voice_frame(&mixer, 0), Some(7));
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_schedules_next_bar_frame() {
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
            TriggerQuantization::NextBar,
            &transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(40));
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
            TriggerQuantization::NextBeat,
            &transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
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
        let transport = TransportTimeline::new(10);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            transport.output_frame(),
            TriggerQuantization::NextBeat,
            &transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
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
    fn bpm_lock_phase_anchor_keeps_transport_downbeat_without_master_bpm() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        transport.set_downbeat_frame(12);

        assert!(!anchor_transport_phase_from_pad(&mixer, &mut transport, 0));

        assert_eq!(transport.downbeat_frame(), 12);
    }

    #[test]
    fn scheduler_full_quantized_play_leaves_current_playback_unchanged() {
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

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::NextBeat,
            &transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
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
        let transport = TransportTimeline::new(10);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            transport.output_frame(),
            TriggerQuantization::Immediate,
            &transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
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
    fn quantized_exclusive_play_uses_phase_aligned_initial_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.load_sample(1, create_test_sample(1, 64, 0.25));
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
            TriggerQuantization::NextBar,
            &transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 0)
        );
        assert_eq!(active_voice_frame(&mixer, 1), Some(20));
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
        transport.advance_by_rendered_frames(5);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::NextBeat,
            &transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(10));
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

        let mut output = vec![0.0; 10];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            callback_start_frame,
            1,
            &mut messages,
        );

        assert!(
            output[..5]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-5)
        );
        assert!(
            output[5..]
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
            TriggerQuantization::NextBeat,
            &transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
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
        let transport = TransportTimeline::new(10);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            transport.output_frame(),
            TriggerQuantization::Immediate,
            &transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
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
        scheduler
            .schedule(
                4,
                ScheduledCommand::PlaySample {
                    id: 0,
                    volume: 1.0,
                    target_bar_phase_beats: None,
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
            &mut messages,
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
    fn scheduled_stop_inside_buffer_silences_after_target_offset() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.play_sample(0, 1.0);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
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
            &mut messages,
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
        scheduler.schedule(0, ScheduledCommand::StopAll).unwrap();
        scheduler
            .schedule(
                0,
                ScheduledCommand::PlaySample {
                    id: 1,
                    volume: 1.0,
                    target_bar_phase_beats: None,
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
            &mut messages,
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
