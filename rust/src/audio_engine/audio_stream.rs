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

use crate::audio_engine::mixer::RtMixer;
use crate::messages::{AudioMessage, ControlMessage};

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
    let channels = config.channels();

    log::info!(
        "Starting AudioEngine... ({} ch@{} Hz)",
        channels,
        sample_rate
    );

    // Create ring buffer for incoming messages (Python->Rust)
    let (producer_in, mut consumer_in) = RingBuffer::new(1024);

    // Create ring buffer for outgoing messages (Rust->Python)
    let (mut producer_out, consumer_out) = RingBuffer::new(1024);

    let mut mixer = RtMixer::new(channels as usize);

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
            // Process incoming messages in real-time
            while let Ok(message) = consumer_in.pop() {
                match message {
                    ControlMessage::Ping() => {
                        let _ = producer_out.push(AudioMessage::Pong());
                    }
                    ControlMessage::LoadSample { id, sample } => {
                        mixer.load_sample(id, sample);
                    }
                    ControlMessage::PlaySample { id, volume } => {
                        mixer.play_sample(id, volume);
                    }
                    ControlMessage::StopSample { id } => {
                        mixer.stop_sample(id);
                    }
                    ControlMessage::StopAll() => {
                        mixer.stop_all();
                    }
                    ControlMessage::UnloadSample { id } => {
                        mixer.unload_sample(id);
                    }
                    ControlMessage::SetSpeed(speed) => {
                        mixer.set_speed(speed);
                    }
                    ControlMessage::SetBpmLock(enabled) => {
                        mixer.set_bpm_lock(enabled);
                    }
                    ControlMessage::SetKeyLock(enabled) => {
                        mixer.set_key_lock(enabled);
                    }
                    ControlMessage::SetMasterBpm(bpm) => {
                        mixer.set_master_bpm(bpm);
                    }
                    ControlMessage::SetPadBpm { id, bpm } => {
                        mixer.set_pad_bpm(id, bpm);
                    }
                    ControlMessage::SetVolume(volume) => {
                        mixer.set_volume(volume);
                    }
                }
            }

            // Render audio
            mixer.render(data);
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
        output_sample_rate: sample_rate,
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
}
