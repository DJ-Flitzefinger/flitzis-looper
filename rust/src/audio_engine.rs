use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Sample};
use env_logger::Builder;
use pyo3::prelude::*;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{Arc, Mutex};

use crate::messages::{AudioMessage, ControlMessage};

/// AudioEngine provides minimal audio output capabilities using cpal
#[pyclass]
pub struct AudioEngine {
    stream: Option<cpal::Stream>,
    is_playing: bool,
    producer: Option<Arc<Mutex<Producer<ControlMessage>>>>,
    consumer: Option<Arc<Mutex<Consumer<AudioMessage>>>>,
}

#[pymethods]
impl AudioEngine {
    /// Create a new AudioEngine instance with default audio device.
    #[new]
    pub fn new() -> PyResult<Self> {
        Ok(AudioEngine {
            stream: None,
            is_playing: false,
            producer: None,
            consumer: None,
        })
    }

    /// Initialize and run the audio engine.
    pub fn run(&mut self) -> PyResult<()> {
        if self.stream.is_some() {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "AudioEngine already running",
            ));
        }

        self.setup_logger();

        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(device) => device,
            None => {
                return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "No audio device found",
                ));
            }
        };

        let config = match device.default_output_config() {
            Ok(config) => config,
            Err(_) => {
                return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "No default output config",
                ));
            }
        };

        let sample_rate = config.sample_rate();
        let channels = config.channels();

        log::info!(
            "Starting AudioEngine... ({} ch@{} Hz)",
            channels,
            sample_rate.0
        );

        // Create ring buffer for incoming messages (Python->Rust)
        let (producer_in, mut consumer_in) = RingBuffer::new(1024);
        self.producer = Some(Arc::new(Mutex::new(producer_in)));

        // Create ring buffer for outgoing messages (Rust->Python)
        let (mut producer_out, consumer_out) = RingBuffer::new(1024);
        self.consumer = Some(Arc::new(Mutex::new(consumer_out)));

        // Create audio stream (creates a thread), also process messages
        let stream = match device.build_output_stream(
            &cpal::StreamConfig {
                channels,
                sample_rate,
                buffer_size: BufferSize::Fixed(512),
            },
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // Process incoming messages
                while let Ok(message) = consumer_in.pop() {
                    match message {
                        ControlMessage::Ping() => {
                            if let Err(e) = producer_out.push(AudioMessage::Pong()) {
                                log::error!("Failed to send Pong response: {:?}", e);
                            }
                        }
                        ControlMessage::Play() => {
                            log::info!("Received Play message");
                        }
                        ControlMessage::Stop() => {
                            log::info!("Received Stop message");
                        }
                        ControlMessage::SetVolume(volume) => {
                            log::info!("Setting volume to {}", volume);
                        }
                        ControlMessage::LoadSample { id } => {
                            log::info!("Loading sample {}", id);
                        }
                        ControlMessage::PlaySample { id, velocity } => {
                            log::info!("Playing sample {} with velocity {}", id, velocity);
                        }
                    }
                }

                // Fill buffer with silence
                for sample in data.iter_mut() {
                    *sample = Sample::EQUILIBRIUM;
                }
            },
            |err| eprintln!("Audio error: {}", err),
            None,
        ) {
            Ok(stream) => stream,
            Err(e) => {
                return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                    "Failed to create audio stream: {}",
                    e
                )));
            }
        };

        match stream.play() {
            Ok(()) => {}
            Err(e) => {
                return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                    "Failed to play audio stream: {}",
                    e
                )));
            }
        };

        self.stream = Some(stream);
        self.is_playing = true;
        Ok(())
    }

    /// Shut down the audio engine.
    pub fn shut_down(&mut self) -> PyResult<()> {
        self.stream = None;
        self.is_playing = false;
        Ok(())
    }

    /// Send a ping message to the audio thread.
    pub fn ping(&mut self) -> PyResult<()> {
        if let Some(ref producer) = self.producer {
            let mut producer_guard = producer.lock().map_err(|_| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("Failed to acquire producer lock")
            })?;

            match producer_guard.push(ControlMessage::Ping()) {
                Ok(()) => Ok(()),
                Err(_) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    "Failed to send ping message - buffer may be full",
                )),
            }
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Audio engine not initialized",
            ))
        }
    }

    /// Receive a message from the audio thread.
    pub fn receive_msg(&mut self) -> PyResult<Option<AudioMessage>> {
        if let Some(ref consumer) = self.consumer {
            let mut consumer_guard = consumer.lock().map_err(|_| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("Failed to acquire consumer lock")
            })?;

            match consumer_guard.pop() {
                Ok(msg) => Ok(Some(msg)),
                Err(_) => Ok(None),
            }
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Audio engine not initialized",
            ))
        }
    }
}

impl AudioEngine {
    fn setup_logger(&mut self) {
        Builder::new()
            .format_timestamp(None)
            .filter_level(log::LevelFilter::max())
            .try_init()
            .unwrap_or_else(|_| {
                // Ignore during tests...
            });
    }
}

#[cfg(test)]
mod tests {
    use std::{
        thread::sleep,
        time::{Duration, Instant},
    };

    use super::*;

    fn wait_for_msg(engine: &mut AudioEngine) -> Option<AudioMessage> {
        let deadline = Instant::now() + Duration::from_millis(100);

        while Instant::now() < deadline {
            match engine.receive_msg() {
                Ok(Some(msg)) => return Some(msg),
                Ok(None) => {}
                Err(e) => panic!("receive_msg failed: {e:?}"),
            }
            sleep(Duration::from_millis(1));
        }

        None
    }

    #[test]
    fn test_audio_engine_creation() {
        let engine = AudioEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_audio_engine_play_stop() {
        let mut engine = AudioEngine::new().unwrap();

        let result = engine.run();
        assert!(result.is_ok());

        let result = engine.shut_down();
        assert!(result.is_ok());
    }

    #[test]
    fn test_ring_buffer_operations() {
        let mut engine = AudioEngine::new().unwrap();

        let result = engine.ping();
        assert!(result.is_err());

        let result = engine.run();
        assert!(result.is_ok());

        let result = engine.ping();
        assert!(result.is_ok());

        let msg = wait_for_msg(&mut engine);
        assert!(matches!(msg, Some(AudioMessage::Pong())));

        let result = engine.shut_down();
        assert!(result.is_ok());
    }

    #[test]
    fn test_message_sending_receiving() {
        let mut engine = AudioEngine::new().unwrap();
        let result = engine.run();
        assert!(result.is_ok());

        for _ in 0..5 {
            let result = engine.ping();
            assert!(result.is_ok());
        }

        for _ in 0..5 {
            let msg = wait_for_msg(&mut engine);
            assert!(matches!(msg, Some(AudioMessage::Pong())));
        }

        let result = engine.shut_down();
        assert!(result.is_ok());
    }
}
