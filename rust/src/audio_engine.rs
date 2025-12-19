use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use pyo3::prelude::*;

/// AudioEngine provides minimal audio output capabilities using cpal
#[pyclass]
pub struct AudioEngine {
    stream: Option<cpal::Stream>,
    is_playing: bool,
}

#[pymethods]
impl AudioEngine {
    /// Create a new AudioEngine instance with default audio device
    #[new]
    pub fn new() -> PyResult<Self> {
        Ok(AudioEngine {
            stream: None,
            is_playing: false,
        })
    }

    /// Initialize and start the audio engine
    pub fn play(&mut self) -> PyResult<()> {
        if self.stream.is_some() {
            return Ok(());
        }

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
        let channels = config.channels() as usize;

        // Create a stream with a simple silence callback
        let stream = match device.build_output_stream(
            &cpal::StreamConfig {
                channels: channels as u16,
                sample_rate,
                buffer_size: cpal::BufferSize::Fixed(512),
            },
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // Fill buffer with silence
                for sample in data.iter_mut() {
                    *sample = 0.0;
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

    /// Stop the audio engine
    pub fn stop(&mut self) -> PyResult<()> {
        self.stream = None;
        self.is_playing = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_engine_creation() {
        let engine = AudioEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_audio_engine_play_stop() {
        let mut engine = AudioEngine::new().unwrap();

        // Play should succeed
        let result = engine.play();
        assert!(result.is_ok());

        // Stop should succeed
        let result = engine.stop();
        assert!(result.is_ok());
    }
}
