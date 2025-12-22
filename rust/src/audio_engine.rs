use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Sample};
use env_logger::Builder;
use pyo3::exceptions::{PyFileNotFoundError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::{Consumer, Producer, RingBuffer};
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};
use symphonia::core::{
    audio::SampleBuffer as SymphoniaSampleBuffer, codecs::DecoderOptions,
    errors::Error as SymphoniaError, formats::FormatOptions, io::MediaSourceStream,
    meta::MetadataOptions, probe::Hint,
};
use symphonia::default::{get_codecs, get_probe};

use crate::messages::{AudioMessage, ControlMessage, SampleBuffer};

const NUM_BANKS: usize = 6;
const GRID_SIZE: usize = 6;
const NUM_PADS: usize = GRID_SIZE.pow(2);
const NUM_SAMPLES: usize = NUM_PADS * NUM_BANKS;
const MAX_VOICES: usize = 32;
const SPEED_MIN: f32 = 0.5;
const SPEED_MAX: f32 = 2.0;
const VOLUME_MIN: f32 = 0.0;
const VOLUME_MAX: f32 = 1.0;

#[derive(Debug)]
struct Voice {
    sample_id: usize,
    sample: SampleBuffer,
    frame_pos: usize,
    volume: f32,
}

impl Voice {
    fn new(sample_id: usize, sample: SampleBuffer, volume: f32) -> Self {
        Self {
            sample_id,
            sample,
            frame_pos: 0,
            volume,
        }
    }
}

struct RtMixer {
    channels: usize,
    volume: f32,
    speed: f32,
    sample_bank: [Option<SampleBuffer>; NUM_SAMPLES],
    voices: [Option<Voice>; MAX_VOICES],
}

impl RtMixer {
    fn new(channels: usize) -> Self {
        Self {
            channels,
            volume: VOLUME_MAX,
            speed: 1.0,
            sample_bank: std::array::from_fn(|_| None),
            voices: std::array::from_fn(|_| None),
        }
    }

    fn load_sample(&mut self, id: usize, sample: SampleBuffer) {
        if id >= NUM_SAMPLES {
            return;
        }

        if sample.channels != self.channels {
            return;
        }

        self.sample_bank[id] = Some(sample);
    }

    fn play_sample(&mut self, id: usize, volume: f32) {
        if id >= NUM_SAMPLES {
            return;
        }

        if !volume.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&volume) {
            return;
        }

        let Some(sample) = self.sample_bank[id].as_ref() else {
            return;
        };
        let sample = sample.clone();

        for voice_slot in &mut self.voices {
            if voice_slot.is_none() {
                *voice_slot = Some(Voice::new(id, sample, volume));
                return;
            }
        }

        // No free voice slot: drop deterministically.
    }

    fn stop_all(&mut self) {
        for voice in &mut self.voices {
            *voice = None;
        }
    }

    fn set_volume(&mut self, volume: f32) {
        if !volume.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&volume) {
            return;
        }

        self.volume = volume;
    }

    fn set_speed(&mut self, speed: f32) {
        if !speed.is_finite() || !(SPEED_MIN..=SPEED_MAX).contains(&speed) {
            return;
        }

        self.speed = speed;
    }

    fn stop_sample(&mut self, id: usize) {
        if id >= NUM_SAMPLES {
            return;
        }

        for voice_slot in &mut self.voices {
            let should_stop = voice_slot
                .as_ref()
                .is_some_and(|voice| voice.sample_id == id);
            if should_stop {
                *voice_slot = None;
            }
        }
    }

    fn unload_sample(&mut self, id: usize) {
        if id >= NUM_SAMPLES {
            return;
        }

        self.stop_sample(id);
        self.sample_bank[id] = None;
    }

    fn render(&mut self, output: &mut [f32]) {
        output.fill(Sample::EQUILIBRIUM);
        let _ = self.speed;

        if self.channels == 0 {
            return;
        }

        let frames = output.len() / self.channels;

        for frame_idx in 0..frames {
            let frame_base = frame_idx * self.channels;

            for voice_slot in &mut self.voices {
                let Some(voice) = voice_slot else {
                    continue;
                };

                let sample_frames = voice.sample.samples.len() / self.channels;
                if sample_frames == 0 {
                    *voice_slot = None;
                    continue;
                }

                if voice.frame_pos >= sample_frames {
                    voice.frame_pos = 0;
                }

                let sample_base = voice.frame_pos * self.channels;

                for channel in 0..self.channels {
                    output[frame_base + channel] +=
                        voice.sample.samples[sample_base + channel] * voice.volume * self.volume;
                }

                voice.frame_pos += 1;
                if voice.frame_pos >= sample_frames {
                    voice.frame_pos = 0;
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum SampleLoadError {
    #[error("failed to open file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to decode audio file: {0}")]
    Decode(#[from] symphonia::core::errors::Error),

    #[error("audio file has no default track")]
    NoDefaultTrack,

    #[error("audio file is missing a sample rate")]
    MissingSampleRate,

    #[error("audio file is missing channel information")]
    MissingChannels,

    #[error(
        "unsupported channel mapping: file has {file_channels} channels, output has {output_channels} channels (only mono↔stereo supported)"
    )]
    UnsupportedChannels {
        file_channels: usize,
        output_channels: usize,
    },

    #[error("sample rate mismatch: file is {file_rate} Hz but output is {output_rate} Hz")]
    SampleRateMismatch { file_rate: u32, output_rate: u32 },
}

fn decode_audio_file_to_sample_buffer(
    path: &Path,
    output_channels: usize,
    output_rate_hz: u32,
) -> Result<SampleBuffer, SampleLoadError> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or(SampleLoadError::NoDefaultTrack)?;
    let file_rate_hz = track
        .codec_params
        .sample_rate
        .ok_or(SampleLoadError::MissingSampleRate)?;
    let file_channels = track
        .codec_params
        .channels
        .ok_or(SampleLoadError::MissingChannels)?
        .count();

    if file_rate_hz != output_rate_hz {
        return Err(SampleLoadError::SampleRateMismatch {
            file_rate: file_rate_hz,
            output_rate: output_rate_hz,
        });
    }

    let mut decoder = get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut decoded: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => return Err(SampleLoadError::Decode(err)),
        };

        let audio_buf = decoder.decode(&packet)?;
        let spec = *audio_buf.spec();
        let duration = audio_buf.capacity() as u64;

        let mut sample_buf = SymphoniaSampleBuffer::<f32>::new(duration, spec);
        sample_buf.copy_interleaved_ref(audio_buf);
        decoded.extend_from_slice(sample_buf.samples());
    }

    let mapped = map_channels(decoded, file_channels, output_channels)?;

    Ok(SampleBuffer {
        channels: output_channels,
        samples: Arc::from(mapped.into_boxed_slice()),
    })
}

fn map_channels(
    samples: Vec<f32>,
    file_channels: usize,
    output_channels: usize,
) -> Result<Vec<f32>, SampleLoadError> {
    if file_channels == output_channels {
        return Ok(samples);
    }

    match (file_channels, output_channels) {
        (1, 2) => {
            let mut out = Vec::with_capacity(samples.len() * 2);
            for s in samples {
                out.push(s);
                out.push(s);
            }
            Ok(out)
        }
        (2, 1) => {
            let mut out = Vec::with_capacity(samples.len() / 2);
            for frame in samples.chunks_exact(2) {
                out.push((frame[0] + frame[1]) * 0.5);
            }
            Ok(out)
        }
        _ => Err(SampleLoadError::UnsupportedChannels {
            file_channels,
            output_channels,
        }),
    }
}

/// AudioEngine provides minimal audio output capabilities using cpal
#[pyclass]
pub struct AudioEngine {
    stream: Option<cpal::Stream>,
    is_playing: bool,
    producer: Option<Arc<Mutex<Producer<ControlMessage>>>>,
    consumer: Option<Arc<Mutex<Consumer<AudioMessage>>>>,
    output_channels: Option<usize>,
    output_sample_rate_hz: Option<u32>,
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
            output_channels: None,
            output_sample_rate_hz: None,
        })
    }

    /// Initialize and run the audio engine.
    pub fn run(&mut self) -> PyResult<()> {
        if self.stream.is_some() {
            return Err(PyRuntimeError::new_err("AudioEngine already running"));
        }

        self.setup_logger();

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| PyRuntimeError::new_err("No audio device found"))?;

        let config = device
            .default_output_config()
            .map_err(|_| PyRuntimeError::new_err("No default output config"))?;

        let sample_rate = config.sample_rate();
        let channels = config.channels();

        self.output_channels = Some(channels as usize);
        self.output_sample_rate_hz = Some(sample_rate);

        log::info!(
            "Starting AudioEngine... ({} ch@{} Hz)",
            channels,
            sample_rate
        );

        // Create ring buffer for incoming messages (Python->Rust)
        let (producer_in, mut consumer_in) = RingBuffer::new(1024);
        self.producer = Some(Arc::new(Mutex::new(producer_in)));

        // Create ring buffer for outgoing messages (Rust->Python)
        let (mut producer_out, consumer_out) = RingBuffer::new(1024);
        self.consumer = Some(Arc::new(Mutex::new(consumer_out)));

        let mut mixer = RtMixer::new(channels as usize);

        // Create audio stream (creates a thread), also process messages
        let stream = device
            .build_output_stream(
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
                            ControlMessage::SetVolume(volume) => {
                                mixer.set_volume(volume);
                            }
                        }
                    }

                    mixer.render(data);
                },
                |_err| {
                    // TODO: Handle error
                },
                None,
            )
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create audio stream: {e}")))?;

        stream
            .play()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to play audio stream: {e}")))?;

        self.stream = Some(stream);
        self.is_playing = true;
        Ok(())
    }

    /// Shut down the audio engine.
    pub fn shut_down(&mut self) -> PyResult<()> {
        self.stream = None;
        self.is_playing = false;
        self.producer = None;
        self.consumer = None;
        self.output_channels = None;
        self.output_sample_rate_hz = None;
        Ok(())
    }

    /// Load an audio file into a sample slot.
    pub fn load_sample(&mut self, id: usize, path: &str) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err(format!(
                "id out of range (expected 0..{}, got {id})",
                NUM_SAMPLES - 1
            )));
        }

        let producer = self
            .producer
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        let output_channels = self
            .output_channels
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        let output_sample_rate_hz = self
            .output_sample_rate_hz
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let sample = match decode_audio_file_to_sample_buffer(
            Path::new(path),
            output_channels,
            output_sample_rate_hz,
        ) {
            Ok(sample) => sample,
            Err(SampleLoadError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(PyFileNotFoundError::new_err(path.to_string()));
            }
            Err(err) => {
                return Err(PyValueError::new_err(err.to_string()));
            }
        };

        let mut producer_guard = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::LoadSample { id, sample })
            .map_err(|_| PyRuntimeError::new_err("Failed to send LoadSample - buffer may be full"))
    }

    /// Trigger playback of a previously loaded sample.
    pub fn play_sample(&mut self, id: usize, volume: f32) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }

        if !volume.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&volume) {
            return Err(PyValueError::new_err("volume out of range"));
        }

        let producer = self
            .producer
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::PlaySample { id, volume })
            .map_err(|_| PyRuntimeError::new_err("Failed to send PlaySample - buffer may be full"))
    }

    /// Stop playback of all active voices.
    pub fn stop_all(&mut self) -> PyResult<()> {
        let producer = self
            .producer
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::StopAll())
            .map_err(|_| PyRuntimeError::new_err("Failed to send Stop - buffer may be full"))
    }

    /// Set the global volume multiplier.
    pub fn set_volume(&mut self, volume: f32) -> PyResult<()> {
        if !volume.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&volume) {
            return Err(PyValueError::new_err("volume out of range"));
        }

        let producer = self
            .producer
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        let _ = producer_guard.push(ControlMessage::SetVolume(volume));
        Ok(())
    }

    /// Set the global speed multiplier.
    pub fn set_speed(&mut self, speed: f32) -> PyResult<()> {
        if !speed.is_finite() || !(SPEED_MIN..=SPEED_MAX).contains(&speed) {
            return Err(PyValueError::new_err("speed out of range"));
        }

        let producer = self
            .producer
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        let _ = producer_guard.push(ControlMessage::SetSpeed(speed));
        Ok(())
    }

    /// Stop playback of a previously triggered sample.
    pub fn stop_sample(&mut self, id: usize) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err(format!(
                "id out of range (expected 0..{}, got {id})",
                NUM_SAMPLES - 1
            )));
        }

        let producer = self
            .producer
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::StopSample { id })
            .map_err(|_| PyRuntimeError::new_err("Failed to send StopSample - buffer may be full"))
    }

    /// Unload a sample slot.
    pub fn unload_sample(&mut self, id: usize) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err(format!(
                "id out of range (expected 0..{}, got {id})",
                NUM_SAMPLES - 1
            )));
        }

        let producer = self
            .producer
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::UnloadSample { id })
            .map_err(|_| {
                PyRuntimeError::new_err("Failed to send UnloadSample - buffer may be full")
            })
    }

    /// Send a ping message to the audio thread.
    pub fn ping(&mut self) -> PyResult<()> {
        let producer = self
            .producer
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut producer_guard = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;

        producer_guard
            .push(ControlMessage::Ping())
            .map_err(|_| PyRuntimeError::new_err("Failed to send Ping - buffer may be full"))
    }

    /// Receive a message from the audio thread.
    pub fn receive_msg(&mut self) -> PyResult<Option<AudioMessage>> {
        let consumer = self
            .consumer
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;

        let mut consumer_guard = consumer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire consumer lock"))?;

        match consumer_guard.pop() {
            Ok(msg) => Ok(Some(msg)),
            Err(_) => Ok(None),
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
        io::Write,
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

    fn write_pcm16_wav(
        path: &Path,
        channels: u16,
        sample_rate_hz: u32,
        samples: &[i16],
    ) -> std::io::Result<()> {
        let bits_per_sample = 16u16;
        let block_align = channels * (bits_per_sample / 8);
        let byte_rate = sample_rate_hz * u32::from(block_align);
        let data_len_bytes = u32::try_from(samples.len() * 2).expect("sample data too large");
        let chunk_size = 36 + data_len_bytes;

        let mut file = File::create(path)?;
        file.write_all(b"RIFF")?;
        file.write_all(&chunk_size.to_le_bytes())?;
        file.write_all(b"WAVE")?;

        file.write_all(b"fmt ")?;
        file.write_all(&16u32.to_le_bytes())?;
        file.write_all(&1u16.to_le_bytes())?; // PCM
        file.write_all(&channels.to_le_bytes())?;
        file.write_all(&sample_rate_hz.to_le_bytes())?;
        file.write_all(&byte_rate.to_le_bytes())?;
        file.write_all(&block_align.to_le_bytes())?;
        file.write_all(&bits_per_sample.to_le_bytes())?;

        file.write_all(b"data")?;
        file.write_all(&data_len_bytes.to_le_bytes())?;
        for sample in samples {
            file.write_all(&sample.to_le_bytes())?;
        }

        Ok(())
    }

    #[test]
    fn test_audio_engine_creation() {
        let engine = AudioEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_audio_engine_play_stop() {
        if cpal::default_host().default_output_device().is_none() {
            return;
        }

        let mut engine = AudioEngine::new().unwrap();
        if engine.run().is_err() {
            return;
        }

        let result = engine.shut_down();
        assert!(result.is_ok());
    }

    #[test]
    fn test_ring_buffer_operations() {
        let mut engine = AudioEngine::new().unwrap();

        let result = engine.ping();
        assert!(result.is_err());

        if engine.run().is_err() {
            return;
        }

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
        if engine.run().is_err() {
            return;
        }

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

    #[test]
    fn test_decode_wav_to_f32_buffer() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.wav");

        let samples = [0i16, 16_384i16, -16_384i16, 32_767i16];
        write_pcm16_wav(&path, 1, 44_100, &samples).unwrap();

        let decoded = decode_audio_file_to_sample_buffer(&path, 1, 44_100).unwrap();
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.samples.len(), samples.len());
        assert!(decoded.samples.iter().all(|s| (-1.0..=1.0).contains(s)));
    }

    #[test]
    fn test_decode_channel_mapping_mono_to_stereo() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.wav");

        let samples = [0i16, 16_384i16, -16_384i16];
        write_pcm16_wav(&path, 1, 44_100, &samples).unwrap();

        let decoded = decode_audio_file_to_sample_buffer(&path, 2, 44_100).unwrap();
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.samples.len(), samples.len() * 2);

        for frame in decoded.samples.chunks_exact(2) {
            assert!((frame[0] - frame[1]).abs() < 1e-6);
        }
    }

    #[test]
    fn test_mixing_loops_sample_buffer() {
        let sample = SampleBuffer {
            channels: 2,
            samples: Arc::from(vec![0.5f32, -0.5f32, 0.25f32, -0.25f32].into_boxed_slice()),
        };

        let mut mixer = RtMixer::new(2);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 0.5);

        let mut out = vec![0.0f32; 8];
        mixer.render(&mut out);

        assert!((out[0] - 0.25).abs() < 1e-6);
        assert!((out[1] + 0.25).abs() < 1e-6);
        assert!((out[2] - 0.125).abs() < 1e-6);
        assert!((out[3] + 0.125).abs() < 1e-6);
        assert!((out[4] - 0.25).abs() < 1e-6);
        assert!((out[5] + 0.25).abs() < 1e-6);
        assert!((out[6] - 0.125).abs() < 1e-6);
        assert!((out[7] + 0.125).abs() < 1e-6);

        let mut out2 = vec![0.0f32; 4];
        mixer.render(&mut out2);
        assert!((out2[0] - 0.25).abs() < 1e-6);
        assert!((out2[1] + 0.25).abs() < 1e-6);
        assert!((out2[2] - 0.125).abs() < 1e-6);
        assert!((out2[3] + 0.125).abs() < 1e-6);
    }

    #[test]
    fn test_mixing_unload_sample_stops_voices_and_clears_slot() {
        let sample = SampleBuffer {
            channels: 2,
            samples: Arc::from(vec![0.5f32, -0.5f32, 0.5f32, -0.5f32].into_boxed_slice()),
        };

        let mut mixer = RtMixer::new(2);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 1.0);

        let mut out = vec![0.0f32; 4];
        mixer.render(&mut out);
        assert!(out.iter().any(|&s| s.abs() > 1e-6));

        mixer.unload_sample(0);

        out.fill(1.0);
        mixer.render(&mut out);
        assert!(out.iter().all(|&s| s.abs() < 1e-6));

        mixer.play_sample(0, 1.0);
        out.fill(1.0);
        mixer.render(&mut out);
        assert!(out.iter().all(|&s| s.abs() < 1e-6));
    }

    #[test]
    fn test_mixing_stop_sample_silences_active_voices() {
        let sample = SampleBuffer {
            channels: 2,
            samples: Arc::from(
                vec![
                    0.5f32, -0.5f32, 0.5f32, -0.5f32, 0.5f32, -0.5f32, 0.5f32, -0.5f32,
                ]
                .into_boxed_slice(),
            ),
        };

        let mut mixer = RtMixer::new(2);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 1.0);
        mixer.play_sample(0, 1.0);

        let mut out = vec![0.0f32; 4];
        mixer.render(&mut out);
        assert!(out.iter().any(|&s| s.abs() > 1e-6));

        mixer.stop_sample(0);
        out.fill(1.0);
        mixer.render(&mut out);
        assert!(out.iter().all(|&s| s.abs() < 1e-6));
    }

    #[test]
    fn test_mixing_stop_all_silences_all_active_voices() {
        let sample_a = SampleBuffer {
            channels: 2,
            samples: Arc::from(vec![0.5f32, -0.5f32, 0.5f32, -0.5f32].into_boxed_slice()),
        };
        let sample_b = SampleBuffer {
            channels: 2,
            samples: Arc::from(vec![0.25f32, -0.25f32, 0.25f32, -0.25f32].into_boxed_slice()),
        };

        let mut mixer = RtMixer::new(2);
        mixer.load_sample(0, sample_a);
        mixer.load_sample(1, sample_b);
        mixer.play_sample(0, 1.0);
        mixer.play_sample(1, 1.0);

        let mut out = vec![0.0f32; 4];
        mixer.render(&mut out);
        assert!(out.iter().any(|&s| s.abs() > 1e-6));

        mixer.stop_all();
        out.fill(1.0);
        mixer.render(&mut out);
        assert!(out.iter().all(|&s| s.abs() < 1e-6));

        mixer.play_sample(0, 1.0);
        mixer.render(&mut out);
        assert!(out.iter().any(|&s| s.abs() > 1e-6));
    }
}
