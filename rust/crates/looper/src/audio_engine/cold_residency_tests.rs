//! Productive finite source preparation, real native command ACK and independent
//! original-PCM output oracles. No stream, device, app or synthetic timing owner.

#[path = "resident_control_worker_tests.rs"]
mod resident_control_worker_tests;

use super::audio_stream::drain_control_messages;
use super::buffer_retirement::{
    AudioBufferRetirement, AudioBufferRetirementWorker, RtAudioBufferRetirement,
    create_audio_buffer_retirement,
};
use super::cold_load::admit_for_format_selected;
use super::cold_residency::ResidentLoadHint;
use super::constants::NUM_SAMPLES;
use super::mixer::RtMixer;
use super::scheduler::FixedCapacityScheduler;
use super::transport::TransportTimeline;
use super::{AudioEngine, ControlMessage, LoaderEvent, SampleBuffer};
use crate::messages::{AudioMessage, ResidentContext, TriggerQuantization};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// Inject only bounded capacity pressure. Every retired native owner still uses
// the productive queue and worker, rather than an immediate-drop test sink.
struct RetirementGate {
    actual: RtAudioBufferRetirement,
    capacity: usize,
}

impl AudioBufferRetirement for RetirementGate {
    fn retire_resident_capture(&mut self, value: Arc<super::resident_seek::ResidentSeekCapture>) {
        self.actual.retire_resident_capture(value);
    }
    fn retire_resident_cancellation(&mut self, value: Arc<AtomicBool>) {
        self.actual.retire_resident_cancellation(value);
    }
    fn retire_resident_transaction(&mut self, value: Box<crate::messages::ResidentTransaction>) {
        self.actual.retire_resident_transaction(value);
    }
    fn retire_cold_adoption(&mut self, value: Arc<AtomicU8>) {
        self.actual.retire_cold_adoption(value);
    }
    fn retire_sample(&mut self, value: SampleBuffer) {
        self.actual.retire_sample(value);
    }
    fn retire_prepared_stems(&mut self, value: crate::messages::PreparedStemSet) {
        self.actual.retire_prepared_stems(value);
    }
    fn retire_constant_timing(&mut self, value: super::constant_timing::PreparedConstantTiming) {
        self.actual.retire_constant_timing(value);
    }
    fn retire_global_playback_batch(
        &mut self,
        value: Arc<super::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        self.actual.retire_global_playback_batch(value);
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        value: Arc<super::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        self.actual.retire_accepted_timing_refresh(value);
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.capacity.min(self.actual.available_retirement_slots())
    }
}

struct Callback {
    mixer: RtMixer,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    quantization: TriggerQuantization,
    feedback: rtrb::Producer<AudioMessage>,
    feedback_rx: rtrb::Consumer<AudioMessage>,
    retirement: RetirementGate,
    _retirement_worker: AudioBufferRetirementWorker,
}

impl Callback {
    fn new(engine: &AudioEngine, rate: u32) -> Self {
        let (actual, worker) = create_audio_buffer_retirement();
        let (feedback, feedback_rx) = rtrb::RingBuffer::new(1);
        let mut mixer = RtMixer::new(2, rate as f32);
        mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
        mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
        mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
        Self {
            mixer,
            scheduler: FixedCapacityScheduler::new(),
            transport: TransportTimeline::new(rate),
            quantization: TriggerQuantization::Immediate,
            feedback,
            feedback_rx,
            retirement: RetirementGate {
                actual,
                capacity: usize::MAX,
            },
            _retirement_worker: worker,
        }
    }

    fn drain(&mut self, consumer: &mut rtrb::Consumer<ControlMessage>) -> usize {
        drain_control_messages(
            consumer,
            &mut self.scheduler,
            0,
            &mut self.quantization,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut self.retirement,
        )
    }

    fn assert_old_voice(&self, previous: &SampleBuffer) {
        let voice = self
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &previous.samples
        ));
    }

    fn render_oracle(&mut self, mono_window: &[f32], start: usize, rate: u32) -> String {
        assert!(self.mixer.play_sample_rt(0, 1.0, &mut self.retirement));
        let mut output = vec![0.0; (mono_window.len() * 2 + 7) * 2];
        self.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            0,
            &mut self.retirement,
        );
        // An independent modulo over source integers exercises two whole loop
        // seams; the oracle never calls any native source/read-plan helper.
        for (frame, actual) in output.chunks_exact(2).enumerate() {
            let expected = mono_window[frame % mono_window.len()];
            for sample in actual {
                assert_eq!(sample.to_bits(), expected.to_bits(), "frame {frame}");
            }
        }
        let playhead = self.mixer.pad_playhead_seconds(0).unwrap();
        assert!(playhead >= start as f64 / f64::from(rate));
        assert!(playhead < (start + mono_window.len()) as f64 / f64::from(rate));
        pcm_hash(&output)
    }

    fn render_continuation(&mut self, mono_window: &[f32], output_start: usize, frames: usize) {
        let mut output = vec![0.0; frames * 2];
        self.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            output_start as u64,
            &mut self.retirement,
        );
        for (frame, actual) in output.chunks_exact(2).enumerate() {
            let expected = mono_window[(output_start + frame) % mono_window.len()];
            for sample in actual {
                assert_eq!(
                    sample.to_bits(),
                    expected.to_bits(),
                    "continuation {output_start}+{frame}"
                );
            }
        }
    }
}

fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "cold worker synchronization timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn terminal(engine: &AudioEngine, request: u64) -> LoaderEvent {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(event) = engine.loader_rx.lock().unwrap().try_recv()
            && matches!(&event, LoaderEvent::Success { request_id, .. } | LoaderEvent::Error { request_id, .. } if *request_id == request)
        {
            return event;
        }
        assert!(Instant::now() < deadline, "finite source terminal timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn assert_no_terminal(engine: &AudioEngine) {
    while let Ok(event) = engine.loader_rx.lock().unwrap().try_recv() {
        assert!(
            !matches!(
                event,
                LoaderEvent::Success { .. } | LoaderEvent::Error { .. }
            ),
            "{event:?}"
        );
    }
}

fn selected_load(
    engine: &AudioEngine,
    path: &Path,
    root: &Path,
    rate: u32,
    hint: ResidentLoadHint,
) -> (u64, rtrb::Consumer<ControlMessage>) {
    let (producer, consumer) = rtrb::RingBuffer::new(4);
    let request = admit_for_format_selected(
        engine,
        0,
        path.to_string_lossy().into_owned(),
        (false, false, true, Some(hint)),
        Arc::new(Mutex::new(producer)),
        (2, rate, root.to_owned()),
    )
    .unwrap();
    (request, consumer)
}

fn pcm_hash(samples: &[f32]) -> String {
    let mut hash = Sha256::new();
    for sample in samples {
        hash.update(sample.to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn full_hash(path: &Path) -> String {
    let mut reader = fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut bytes = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut bytes).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    format!("{:x}", hash.finalize())
}

fn wav(path: &Path, rate: u32) -> Vec<f32> {
    let integers: Vec<i16> = (0..256)
        .map(|frame| ((frame * 211 + 37) % 16_384 - 8_192) as i16)
        .collect();
    write_pcm16(path, rate, 1, &integers);
    integers
        .into_iter()
        .map(|integer| f32::from(integer) / 32_768.0)
        .collect()
}

fn write_pcm16(path: &Path, rate: u32, channels: u16, integers: &[i16]) {
    let data = (integers.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2 * u32::from(channels)).to_le_bytes());
    bytes.extend_from_slice(&(2 * channels).to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for integer in integers {
        bytes.extend_from_slice(&integer.to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

fn assert_residency(
    engine: &AudioEngine,
    full_frames: usize,
    start: usize,
    end: usize,
    rate: u32,
    mono_window: &[f32],
    context: ResidentContext,
) -> SampleBuffer {
    let sample = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    assert_eq!(sample.frame_count(), full_frames);
    assert_eq!(sample.source_sample_count(), full_frames * 2);
    assert_eq!(
        (sample.resident_start(), sample.resident_end()),
        (start, end)
    );
    assert_eq!(sample.samples.len(), (end - start) * 2);
    assert_eq!(sample.window_revision(), 1);
    let view = sample.residency.as_ref().unwrap();
    assert_eq!(view.context, context);
    assert_eq!(view.source.sample_rate_hz, rate);
    assert_eq!(view.source.frame_count, full_frames);
    assert_eq!(view.source.source_zero_frame, 0);
    assert!(
        engine
            .input_runtime_ownership
            .source_current(0, &sample, rate)
    );
    for (actual, reference) in sample.samples.chunks_exact(2).zip(mono_window) {
        assert_eq!(actual[0].to_bits(), reference.to_bits());
        assert_eq!(actual[1].to_bits(), reference.to_bits());
    }
    let history = engine.cold_pcm_history.lock().unwrap();
    let live: Vec<_> = history[0]
        .iter()
        .filter_map(std::sync::Weak::upgrade)
        .collect();
    assert_eq!(live.len(), 1);
    assert!(
        Arc::ptr_eq(&live[0], &sample.samples),
        "history tracked discarded full PCM"
    );
    assert_eq!(live[0].len(), (end - start) * 2);
    sample
}

#[test]
fn productive_finite_startup_preserves_absolute_metadata_and_matches_original_pcm() {
    for rate in [44_100, 48_000, 96_000] {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("external.wav");
        let mono = wav(&source, rate);
        let engine = AudioEngine::new().unwrap();
        let mut callback = Callback::new(&engine, rate);
        let hint = ResidentLoadHint {
            start_s: 32.0 / f64::from(rate),
            end_s: 64.0 / f64::from(rate),
            key_lock: false,
        };
        let (request, mut consumer) = selected_load(
            &engine,
            &source,
            &directory.path().join("samples"),
            rate,
            hint,
        );
        wait_until(Duration::from_secs(10), || consumer.peek().is_ok());
        assert_no_terminal(&engine);
        assert_eq!(
            engine.input_runtime_ownership.cold_status(0, request),
            Some(0)
        );
        assert_eq!(callback.drain(&mut consumer), 1);
        let event = terminal(&engine, request);
        let LoaderEvent::Success {
            duration_s,
            cached_path,
            ..
        } = event
        else {
            panic!("{event:?}");
        };
        assert_eq!(duration_s, 256.0 / f64::from(rate));
        wait_until(Duration::from_secs(10), || {
            engine.cold_loading[0].load(Ordering::Acquire) == 0
        });
        assert_eq!(
            fs::read(directory.path().join(cached_path)).unwrap(),
            fs::read(&source).unwrap()
        );
        let sample = assert_residency(
            &engine,
            256,
            32,
            64,
            rate,
            &mono[32..64],
            ResidentContext::FiniteLoop,
        );
        let lease = engine.cold_leases.lock().unwrap();
        let lease = lease[0].as_ref().unwrap();
        let full_stereo: Vec<_> = mono.iter().flat_map(|value| [*value, *value]).collect();
        assert_eq!(
            lease.manifest.descriptor["playback"]["pcm"]["full_frames"],
            256
        );
        assert_eq!(
            lease.manifest.descriptor["playback"]["pcm"]["interleaved_sha256"],
            pcm_hash(&full_stereo)
        );
        assert_eq!(
            full_hash(&lease.cache_path.join("playback.f32le")),
            pcm_hash(&full_stereo)
        );
        assert_eq!(
            lease.live_cached_pcm_samples(),
            None,
            "finite load retained complete cache PCM"
        );
        assert_ne!(pcm_hash(&sample.samples), pcm_hash(&full_stereo));
        callback.render_oracle(&mono[32..64], 32, rate);
    }
}

#[test]
fn productive_saved_key_lock_uses_explicit_complete_context() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("external.wav");
    let mono = wav(&source, 48_000);
    let engine = AudioEngine::new().unwrap();
    let mut callback = Callback::new(&engine, 48_000);
    let hint = ResidentLoadHint {
        start_s: 32.0 / 48_000.0,
        end_s: 64.0 / 48_000.0,
        key_lock: true,
    };
    let (request, mut consumer) = selected_load(
        &engine,
        &source,
        &directory.path().join("samples"),
        48_000,
        hint,
    );
    wait_until(Duration::from_secs(10), || consumer.peek().is_ok());
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(matches!(
        terminal(&engine, request),
        LoaderEvent::Success { .. }
    ));
    wait_until(Duration::from_secs(10), || {
        engine.cold_loading[0].load(Ordering::Acquire) == 0
    });
    assert_residency(
        &engine,
        256,
        0,
        256,
        48_000,
        &mono,
        ResidentContext::KeyLockFullTrack,
    );
}

fn seed_old(engine: &AudioEngine, callback: &mut Callback) -> SampleBuffer {
    let previous = SampleBuffer {
        channels: 2,
        samples: Arc::from([0.25; 64]),
        residency: None,
    };
    engine.sample_cache.lock().unwrap()[0] = Some(previous.clone());
    engine.loaded_source_generations.lock().unwrap()[0] = (7, 48_000);
    engine.loaded_source_digests.lock().unwrap()[0] = Some("old".into());
    engine.pad_request_ids.lock().unwrap()[0] = 7;
    engine
        .input_runtime_ownership
        .publish_source(0, &previous, 48_000, 7);
    callback
        .mixer
        .load_sample_rt(0, previous.clone(), &mut callback.retirement);
    assert!(
        callback
            .mixer
            .play_sample_rt(0, 1.0, &mut callback.retirement)
    );
    previous
}

#[test]
fn pending_finite_intent_cancellation_preserves_native_old_voice_and_metadata() {
    for key_lock_change in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("external.wav");
        wav(&source, 48_000);
        let engine = AudioEngine::new().unwrap();
        let mut callback = Callback::new(&engine, 48_000);
        let previous = seed_old(&engine, &mut callback);
        let hint = ResidentLoadHint {
            start_s: 32.0 / 48_000.0,
            end_s: 64.0 / 48_000.0,
            key_lock: false,
        };
        let (request, mut consumer) = selected_load(
            &engine,
            &source,
            &directory.path().join("samples"),
            48_000,
            hint,
        );
        wait_until(Duration::from_secs(10), || consumer.peek().is_ok());
        callback.assert_old_voice(&previous);
        assert_no_terminal(&engine);
        // Use the productive native intent guard, after command enqueue and
        // before the actual callback is allowed to claim the source/window.
        if key_lock_change {
            engine.note_resident_key_lock_intent(0, true).unwrap();
        } else {
            engine
                .note_resident_loop_intent(0, 33.0 / 48_000.0, Some(64.0 / 48_000.0))
                .unwrap();
        }
        assert_eq!(callback.drain(&mut consumer), 1);
        assert!(matches!(
            terminal(&engine, request),
            LoaderEvent::Error { .. }
        ));
        wait_until(Duration::from_secs(10), || {
            engine.cold_loading[0].load(Ordering::Acquire) == 0
        });
        callback.assert_old_voice(&previous);
        assert!(
            engine
                .input_runtime_ownership
                .source_current(0, &previous, 48_000)
        );
        assert_eq!(
            engine.loaded_source_generations.lock().unwrap()[0],
            (7, 48_000)
        );
        assert_eq!(
            engine.loaded_source_digests.lock().unwrap()[0].as_deref(),
            Some("old")
        );
        assert!(engine.cold_leases.lock().unwrap()[0].is_none());
        assert!(Arc::ptr_eq(
            &engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .samples,
            &previous.samples
        ));
    }
}

#[test]
fn finite_replacement_waits_for_actual_native_retirement_feedback_and_ack() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("external.wav");
    let mono = wav(&source, 48_000);
    let engine = AudioEngine::new().unwrap();
    let mut callback = Callback::new(&engine, 48_000);
    let previous = seed_old(&engine, &mut callback);
    let hint = ResidentLoadHint {
        start_s: 32.0 / 48_000.0,
        end_s: 64.0 / 48_000.0,
        key_lock: false,
    };
    let (request, mut consumer) = selected_load(
        &engine,
        &source,
        &directory.path().join("samples"),
        48_000,
        hint,
    );
    wait_until(Duration::from_secs(10), || consumer.peek().is_ok());
    callback.retirement.capacity = 0;
    assert_eq!(callback.drain(&mut consumer), 0);
    callback.assert_old_voice(&previous);
    assert_no_terminal(&engine);
    callback.retirement.capacity = usize::MAX;
    callback.feedback.push(AudioMessage::Pong()).unwrap();
    assert_eq!(callback.drain(&mut consumer), 0);
    callback.assert_old_voice(&previous);
    assert_no_terminal(&engine);
    assert_eq!(
        engine.input_runtime_ownership.cold_status(0, request),
        Some(0)
    );
    assert!(matches!(
        callback.feedback_rx.pop().unwrap(),
        AudioMessage::Pong()
    ));
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(matches!(
        terminal(&engine, request),
        LoaderEvent::Success { .. }
    ));
    wait_until(Duration::from_secs(10), || {
        engine.cold_loading[0].load(Ordering::Acquire) == 0
    });
    assert!(matches!(
        callback.feedback_rx.pop().unwrap(),
        AudioMessage::SampleStopped { id: 0 }
    ));
    assert!(
        !callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert_residency(
        &engine,
        256,
        32,
        64,
        48_000,
        &mono[32..64],
        ResidentContext::FiniteLoop,
    );
    callback.render_oracle(&mono[32..64], 32, 48_000);
}

fn exercise_productive_relocation(directory: &Path, with_stems: bool) {
    use super::prepared_source::enqueue_current_prepared_stems;
    use super::resident_relocation::{reconcile, relocate_with_producer};
    use super::stem_cache::{
        STEM_FILE_NAMES, prepare_stem_buffers_from_cache, source_version_hash,
    };
    use crate::messages::StemMixMode;

    let source = directory.join("external.wav");
    let mono = wav(&source, 48_000);
    let mut rendered_mono = mono.clone();
    let engine = AudioEngine::new().unwrap();
    let mut callback = Callback::new(&engine, 48_000);
    let hint = ResidentLoadHint {
        start_s: 32.0 / 48_000.0,
        end_s: 64.0 / 48_000.0,
        key_lock: false,
    };
    let (request, mut load_consumer) =
        selected_load(&engine, &source, &directory.join("samples"), 48_000, hint);
    wait_until(Duration::from_secs(10), || load_consumer.peek().is_ok());
    assert_eq!(callback.drain(&mut load_consumer), 1);
    let event = terminal(&engine, request);
    let LoaderEvent::Success { cached_path, .. } = event else {
        panic!("{event:?}");
    };
    wait_until(Duration::from_secs(10), || {
        engine.cold_loading[0].load(Ordering::Acquire) == 0
    });
    let old = assert_residency(
        &engine,
        256,
        32,
        64,
        48_000,
        &mono[32..64],
        ResidentContext::FiniteLoop,
    );
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    let mut accepted_set_identity = None;
    if with_stems {
        let cache_dir = PathBuf::from("samples/stems/relocation-oracle");
        let generation = directory.join(&cache_dir);
        fs::create_dir_all(&generation).unwrap();
        // A complete five-file deterministic fixture: the vocal contains the
        // original PCM integers and the other components are exact silence.
        // This proves productive storage/identity acceptance, not separation.
        let integers: Vec<i16> = mono
            .iter()
            .flat_map(|value| [(*value * 32_768.0) as i16; 2])
            .collect();
        for (index, name) in STEM_FILE_NAMES.iter().enumerate() {
            let values = if index == 0 {
                integers.clone()
            } else {
                vec![0; integers.len()]
            };
            write_pcm16(&generation.join(format!("{name}.wav")), 48_000, 2, &values);
        }
        // Derive the stem oracle from serialized stereo PCM16 integer words.
        // The legacy stem contract maps MAX to +1 (unlike original /32768 PCM).
        // Keep exact comparisons; never read expected values from native buffers.
        let vocal_bytes = fs::read(generation.join("vocals.wav")).unwrap();
        assert_eq!(vocal_bytes.len(), 44 + integers.len() * 2);
        rendered_mono = vocal_bytes[44..]
            .chunks_exact(4)
            .map(|frame| {
                let left = i16::from_le_bytes([frame[0], frame[1]]);
                let right = i16::from_le_bytes([frame[2], frame[3]]);
                assert_eq!(left, right);
                if left == i16::MIN {
                    -1.0
                } else {
                    f32::from(left) / 32_767.0
                }
            })
            .collect();
        let version = format!("{cached_path}|sha256-v1:{}", full_hash(&source));
        let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
        let complete = engine
            .complete_sample(0, &old, super::cold_jobs::PCM_LIMIT_BYTES)
            .unwrap();
        let mut set = prepare_stem_buffers_from_cache(
            &version,
            &complete,
            48_000,
            cache_dir.to_str().unwrap(),
        )
        .unwrap()
        .window_for(&ticket.sample)
        .unwrap();
        let complete_backing = Arc::downgrade(&complete.samples);
        drop(complete);
        assert!(
            complete_backing.upgrade().is_none(),
            "finite prepared stems retained complete source"
        );
        set.publication = ticket.publication.clone();
        set.accepted_timing = ticket.publication.accepted_projection();
        assert_eq!(set.frame_count, 256);
        assert!(set.stems.iter().all(|stem| stem.samples.len() == 64));
        accepted_set_identity = Some(set.complete_set_identity.clone());
        engine
            .project_assets
            .retain_stems(generation.clone(), &set)
            .unwrap();
        enqueue_current_prepared_stems(&engine, &producer, &ticket, &version, set.clone()).unwrap();
        engine
            .record_stems(0, set, version.clone(), cache_dir, generation)
            .unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        assert_eq!(callback.drain(&mut consumer), 1);
        assert_eq!(ticket.publication_status(), "accepted");
        reconcile(&engine).unwrap();
        callback
            .mixer
            .set_stem_mix_mode(0, StemMixMode::AllStems, source_version_hash(&version));
        callback
            .mixer
            .set_stem_enabled_mask(0, 0b0001, source_version_hash(&version));
    }
    assert!(
        callback
            .mixer
            .play_sample_rt(0, 1.0, &mut callback.retirement)
    );
    callback.render_continuation(&rendered_mono[32..64], 0, 7);
    let before = callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
        .source_playback;
    let before_binding = super::input_runtime_binding::capture(&engine, 0)
        .unwrap()
        .unwrap();
    let relocation = relocate_with_producer(
        &engine,
        0,
        16.0 / 48_000.0,
        80.0 / 48_000.0,
        producer.clone(),
    )
    .unwrap();
    wait_until(Duration::from_secs(10), || {
        consumer.peek().is_ok() || matches!(relocation.publication_status(), "failed" | "cancelled")
    });
    assert_eq!(
        relocation.publication_status(),
        "pending",
        "{:?}",
        relocation.error().unwrap()
    );
    assert!(
        engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .same_window(&old)
    );
    let ControlMessage::RelocateResident(transaction) = consumer.peek().unwrap() else {
        panic!("resident command");
    };
    assert_eq!(transaction.sample.window_revision(), 2);
    assert!(transaction.sample.same_source(&old));
    if let Some(identity) = &accepted_set_identity {
        let set = transaction.stems.as_ref().unwrap();
        assert!(Arc::ptr_eq(&set.complete_set_identity, identity));
        assert_eq!(set.frame_count, 256);
        assert!(
            set.stems
                .iter()
                .all(|stem| stem.samples.len() == 128 && stem.same_window(&transaction.sample))
        );
        assert!(Arc::ptr_eq(
            &set.reference_samples,
            &transaction.sample.samples
        ));
    } else {
        assert!(transaction.stems.is_none());
    }
    callback.retirement.capacity = 0;
    assert_eq!(callback.drain(&mut consumer), 0);
    assert_eq!(relocation.publication_status(), "pending");
    assert!(before_binding.current());
    callback.retirement.capacity = usize::MAX;
    assert_eq!(callback.drain(&mut consumer), 1);
    assert_eq!(relocation.publication_status(), "accepted");
    assert!(
        !before_binding.current(),
        "old resident preparation fence survived window ACK"
    );
    reconcile(&engine).unwrap();
    let next = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    assert_eq!(
        (
            next.resident_start(),
            next.resident_end(),
            next.window_revision()
        ),
        (16, 80, 2)
    );
    assert_eq!(next.frame_count(), 256);
    assert!(next.same_source(&old));
    assert!(
        engine
            .input_runtime_ownership
            .source_current(0, &next, 48_000)
    );
    let voice = callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert!(voice.sample.as_ref().unwrap().same_window(&next));
    assert!(
        voice.source_playback.matches_exact(&before),
        "storage relocation reset live reader state"
    );
    assert_eq!(
        engine.cold_leases.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .live_cached_pcm_samples(),
        None
    );
    callback.render_continuation(&rendered_mono[32..64], 7, 137);
    // The same productive worker can be cancelled after enqueue without
    // changing its already-accepted source/window or active reader.
    let cancelled = relocate_with_producer(
        &engine,
        0,
        8.0 / 48_000.0,
        88.0 / 48_000.0,
        producer.clone(),
    )
    .unwrap();
    wait_until(Duration::from_secs(10), || {
        consumer.peek().is_ok() || matches!(cancelled.publication_status(), "failed" | "cancelled")
    });
    assert_eq!(cancelled.publication_status(), "pending");
    assert!(cancelled.cancel());
    assert_eq!(callback.drain(&mut consumer), 1);
    assert_eq!(cancelled.publication_status(), "cancelled");
    reconcile(&engine).unwrap();
    assert!(
        engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .same_window(&next)
    );
    callback.render_continuation(&rendered_mono[32..64], 144, 113);
    if with_stems {
        // This actual old finite voice carries its accepted complete set across
        // a different cold assignment. Its next outside-loop seek must prepare
        // the original sealed lease and historical stem descriptor, not bank B.
        let replacement = directory.join("replacement.wav");
        write_pcm16(&replacement, 48_000, 1, &vec![8192; 1000]);
        let (load_producer, mut load_consumer) = rtrb::RingBuffer::new(4);
        let request = admit_for_format_selected(
            &engine,
            0,
            replacement.to_string_lossy().into_owned(),
            (
                false,
                false,
                false,
                Some(ResidentLoadHint {
                    start_s: 100.0 / 48_000.0,
                    end_s: 300.0 / 48_000.0,
                    key_lock: false,
                }),
            ),
            Arc::new(Mutex::new(load_producer)),
            (2, 48_000, directory.join("samples")),
        )
        .unwrap();
        wait_until(Duration::from_secs(10), || !load_consumer.is_empty());
        assert_eq!(callback.drain(&mut load_consumer), 1);
        assert!(matches!(
            terminal(&engine, request),
            LoaderEvent::Success { .. }
        ));
        wait_until(Duration::from_secs(10), || {
            engine.cold_loading[0].load(Ordering::Acquire) == 0
        });
        let replacement_bank = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
        assert!(!replacement_bank.same_source(&next));
        assert_eq!(
            (
                replacement_bank.frame_count(),
                replacement_bank.resident_start(),
                replacement_bank.resident_end()
            ),
            (1000, 100, 300)
        );
        // New launch selection must not rewrite the pinned old voice's selection.
        callback.mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0);
        callback.mixer.set_stem_enabled_mask(0, 0, 0);
        callback.render_continuation(&rendered_mono[32..64], 257, 29);
        let seek = super::resident_relocation::prepare_window_with_producer(
            &engine,
            0,
            super::resident_relocation::WindowRequest {
                seek_position_s: Some(250.0 / 48_000.0),
                ..super::resident_relocation::WindowRequest::default()
            },
            producer,
        )
        .unwrap();
        assert!(matches!(
            consumer.peek(),
            Ok(ControlMessage::CaptureResidentSeek(_))
        ));
        assert_eq!(callback.drain(&mut consumer), 1);
        wait_until(Duration::from_secs(10), || {
            !consumer.is_empty() || matches!(seek.publication_status(), "failed" | "cancelled")
        });
        assert_eq!(
            seek.publication_status(),
            "pending",
            "{:?}",
            seek.error().unwrap()
        );
        let ControlMessage::RelocateResident(transaction) = consumer.peek().unwrap() else {
            panic!("actual prepared old pin command")
        };
        let captured = transaction.seek_pin.as_ref().unwrap();
        assert!(captured.sample.same_window(&next));
        let prepared = transaction.stems.as_ref().unwrap();
        assert!(Arc::ptr_eq(
            &prepared.complete_set_identity,
            accepted_set_identity.as_ref().unwrap()
        ));
        assert_eq!(
            (
                transaction.sample.frame_count(),
                transaction.sample.resident_start(),
                transaction.sample.resident_end()
            ),
            (256, 0, 256)
        );
        assert!(
            prepared
                .stems
                .iter()
                .all(|stem| stem.same_window(&transaction.sample))
        );
        callback.retirement.capacity = 0;
        assert_eq!(callback.drain(&mut consumer), 0);
        callback.render_continuation(&rendered_mono[32..64], 286, 31);
        callback.retirement.capacity = usize::MAX;
        assert_eq!(callback.drain(&mut consumer), 1);
        assert_eq!(seek.publication_status(), "accepted");
        assert_eq!(seek.effective_seek_seconds(), Some(250.0 / 48_000.0));
        assert!(seek.is_current());
        reconcile(&engine).unwrap();
        assert!(
            engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .same_window(&replacement_bank)
        );
        let old_voice = callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        assert!(old_voice.sample.as_ref().unwrap().same_source(&next));
        let old_set = old_voice
            .frozen_stems
            .as_ref()
            .unwrap()
            .set
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(
            &old_set.complete_set_identity,
            accepted_set_identity.as_ref().unwrap()
        ));
        let complete_pin = Arc::downgrade(&old_voice.sample.as_ref().unwrap().samples);
        let stem_pin = Arc::downgrade(&old_set.stems[0].samples);
        let mut output = vec![0.0; 117 * 2];
        callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            317,
            &mut callback.retirement,
        );
        for (index, actual) in output.chunks_exact(2).enumerate() {
            let frame = if index < 6 {
                250 + index
            } else {
                32 + (index - 6) % 32
            };
            assert_eq!(
                actual, [rendered_mono[frame]; 2],
                "serialized old stem oracle frame {index}"
            );
        }
        callback.mixer.stop_sample_rt(0, &mut callback.retirement);
        wait_until(Duration::from_secs(10), || {
            complete_pin.upgrade().is_none() && stem_pin.upgrade().is_none()
        });
    }
}

#[test]
fn productive_worker_fullmix_relocation_preserves_native_ack_and_live_reader() {
    let directory = tempfile::tempdir().unwrap();
    exercise_productive_relocation(directory.path(), false);
}

#[test]
#[ignore = "isolated process cwd for productive project-relative accepted complete StemSet files"]
fn productive_worker_stem_relocation_keeps_complete_set_and_live_reader() {
    let directory = tempfile::tempdir().unwrap();
    let previous_directory = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(directory.path()).unwrap();
    exercise_productive_relocation(directory.path(), true);
    drop(previous_directory);
}

struct Pcm24Oracle {
    decoder_hash: String,
    playback_hash: String,
    window: Vec<f32>,
}

// Streams raw PCM24 integer words from the unchanged WAV. No native decoder,
// prepared bank, cache descriptor or source-reader calculation supplies values.
fn pcm24_oracle(path: &Path, start: usize, end: usize) -> Pcm24Oracle {
    let mut file = fs::File::open(path).unwrap();
    let mut header = [0_u8; 12];
    file.read_exact(&mut header).unwrap();
    assert_eq!(&header[..4], b"RIFF");
    assert_eq!(&header[8..], b"WAVE");
    let mut decoder = Sha256::new();
    let mut playback = Sha256::new();
    let mut found_format = false;
    loop {
        let mut chunk = [0_u8; 8];
        file.read_exact(&mut chunk).unwrap();
        let length = u32::from_le_bytes(chunk[4..].try_into().unwrap());
        if &chunk[..4] == b"fmt " {
            assert!(length >= 16);
            let mut format = [0_u8; 16];
            file.read_exact(&mut format).unwrap();
            assert_eq!(u16::from_le_bytes(format[..2].try_into().unwrap()), 1);
            assert_eq!(u16::from_le_bytes(format[2..4].try_into().unwrap()), 1);
            assert_eq!(u32::from_le_bytes(format[4..8].try_into().unwrap()), 48_000);
            assert_eq!(u16::from_le_bytes(format[14..16].try_into().unwrap()), 24);
            file.seek(SeekFrom::Current(i64::from(length - 16 + length % 2)))
                .unwrap();
            found_format = true;
        } else if &chunk[..4] == b"data" {
            assert!(found_format);
            assert_eq!(length, 86_400_000);
            let mut remaining = length as usize;
            let mut bytes = [0_u8; 64 * 1024 * 3];
            let mut frame = 0_usize;
            let mut window = Vec::with_capacity(end - start);
            while remaining > 0 {
                let count = remaining.min(bytes.len());
                file.read_exact(&mut bytes[..count]).unwrap();
                for raw in bytes[..count].chunks_exact(3) {
                    let integer =
                        (i32::from(raw[0]) | i32::from(raw[1]) << 8 | i32::from(raw[2]) << 16) << 8
                            >> 8;
                    let sample = integer as f32 / 8_388_608.0;
                    decoder.update(sample.to_le_bytes());
                    playback.update(sample.to_le_bytes());
                    playback.update(sample.to_le_bytes());
                    if (start..end).contains(&frame) {
                        window.push(sample);
                    }
                    frame += 1;
                }
                remaining -= count;
            }
            assert_eq!(frame, 28_800_000);
            assert_eq!(window.len(), end - start);
            return Pcm24Oracle {
                decoder_hash: format!("{:x}", decoder.finalize()),
                playback_hash: format!("{:x}", playback.finalize()),
                window,
            };
        } else {
            file.seek(SeekFrom::Current(i64::from(length + length % 2)))
                .unwrap();
        }
    }
}

struct CurrentDirectory(PathBuf);
impl Drop for CurrentDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

#[test]
#[ignore = "isolated genuine 600s saved short-loop cold/fresh warm native ACK and full integrity vs H read"]
fn c2a_productive_saved_finite_warm_restore_identity_bytes() {
    const RATE: u32 = 48_000;
    const FULL: usize = 28_800_000;
    const START: usize = 42 * RATE as usize;
    const END: usize = START + RATE as usize / 2;
    let retained = PathBuf::from(
        std::env::var_os("FLITZI_COLD_LONG_SOURCE").expect("retained long source path"),
    );
    assert!(retained.is_absolute());
    let original_hash = full_hash(&retained);
    let oracle = pcm24_oracle(&retained, START, END);
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("unchanged-acceptance-source.wav");
    fs::copy(&retained, &source).unwrap();
    assert_eq!(full_hash(&source), original_hash);
    let samples_root = directory.path().join("samples");
    let previous_directory = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(directory.path()).unwrap();
    let hint = ResidentLoadHint {
        start_s: 42.0,
        end_s: 42.5,
        key_lock: false,
    };
    let mut cold = AudioEngine::new().unwrap();
    let mut callback = Callback::new(&cold, RATE);
    let (request, mut consumer) = selected_load(&cold, &source, &samples_root, RATE, hint);
    wait_until(Duration::from_secs(300), || consumer.peek().is_ok());
    assert_no_terminal(&cold);
    assert_eq!(callback.drain(&mut consumer), 1);
    let event = terminal(&cold, request);
    let LoaderEvent::Success {
        cached_path,
        duration_s,
        ..
    } = event
    else {
        panic!("{event:?}");
    };
    assert_eq!(duration_s, 600.0);
    wait_until(Duration::from_secs(10), || {
        cold.cold_loading[0].load(Ordering::Acquire) == 0
    });
    let cold_sample = assert_residency(
        &cold,
        FULL,
        START,
        END,
        RATE,
        &oracle.window,
        ResidentContext::FiniteLoop,
    );
    let cold_render_hash = callback.render_oracle(&oracle.window, START, RATE);
    let cold_manifest: serde_json::Value =
        serde_json::from_str(&cold.cold_source_manifest(0).unwrap().unwrap()).unwrap();
    assert_eq!(
        cold.cold_leases.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .live_cached_pcm_samples(),
        None
    );
    assert_eq!(
        cold_manifest["descriptor"]["playback"]["pcm"]["full_frames"],
        FULL
    );
    assert_eq!(
        cold_manifest["descriptor"]["decoder"]["original"]["sha256"],
        original_hash
    );
    assert_eq!(
        cold_manifest["descriptor"]["decoder"]["pcm"]["interleaved_sha256"],
        oracle.decoder_hash
    );
    assert_eq!(
        cold_manifest["descriptor"]["playback"]["pcm"]["interleaved_sha256"],
        oracle.playback_hash
    );
    let mut assignment = cold
        .acquire_project_asset_lease(cached_path.clone())
        .unwrap();
    cold.shut_down().unwrap();
    drop(callback);
    drop(cold_sample);
    drop(consumer);
    drop(cold);
    assert!(directory.path().join(&cached_path).is_file());
    let mut warm = AudioEngine::new().unwrap();
    let mut callback = Callback::new(&warm, RATE);
    let (request, mut consumer) =
        selected_load(&warm, Path::new(&cached_path), &samples_root, RATE, hint);
    wait_until(Duration::from_secs(300), || consumer.peek().is_ok());
    assert_no_terminal(&warm);
    assert_eq!(callback.drain(&mut consumer), 1);
    let event = terminal(&warm, request);
    let LoaderEvent::Success {
        cached_path: restored,
        duration_s,
        ..
    } = event
    else {
        panic!("{event:?}");
    };
    assert_eq!(restored, cached_path);
    assert_eq!(duration_s, 600.0);
    wait_until(Duration::from_secs(10), || {
        warm.cold_loading[0].load(Ordering::Acquire) == 0
    });
    let warm_sample = assert_residency(
        &warm,
        FULL,
        START,
        END,
        RATE,
        &oracle.window,
        ResidentContext::FiniteLoop,
    );
    let warm_render_hash = callback.render_oracle(&oracle.window, START, RATE);
    assert_eq!(cold_render_hash, warm_render_hash);
    let warm_manifest: serde_json::Value =
        serde_json::from_str(&warm.cold_source_manifest(0).unwrap().unwrap()).unwrap();
    assert_eq!(
        warm.cold_leases.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .live_cached_pcm_samples(),
        None
    );
    assert_eq!(warm_manifest["cache_path"], cold_manifest["cache_path"]);
    assert_eq!(warm_manifest["identity"], cold_manifest["identity"]);
    assert_eq!(warm_manifest["descriptor"], cold_manifest["descriptor"]);
    let metrics = &warm_manifest["integrity"];
    assert_eq!(metrics["warm"], true);
    let original_bytes = fs::metadata(&source).unwrap().len();
    assert_eq!(metrics["source_copied_bytes"], original_bytes);
    assert_eq!(metrics["snapshot_verify_bytes"], original_bytes);
    // As in cold_store/warm_tests.rs, restore hashes the sealed saved original
    // while copying, then verifies the entire snapshot. This separate counter
    // covers import-original copies; the complete original hashes below remain.
    assert_eq!(metrics["original_verify_bytes"], 0);
    assert_eq!(metrics["decoder_verify_bytes"], 115_200_000_u64);
    assert_eq!(metrics["playback_verify_bytes"], 230_400_000_u64);
    assert_eq!(metrics["playback_read_bytes"], 192_000_u64);
    assert_eq!(metrics["original_copied_bytes"], 0);
    assert_eq!(metrics["assignment_copy_bytes"], 0);
    assert_eq!(
        full_hash(&directory.path().join(&cached_path)),
        original_hash
    );
    assert_eq!(full_hash(&retained), original_hash);
    assert_eq!(
        pcm_hash(&warm_sample.samples),
        pcm_hash(
            &oracle
                .window
                .iter()
                .flat_map(|value| [*value, *value])
                .collect::<Vec<_>>()
        )
    );
    let evidence = serde_json::json!({
        "test":"c2a-productive-saved-finite-cold-fresh-warm-v1",
        "original_sha256":original_hash,
        "independent_decoder_sha256":oracle.decoder_hash,
        "independent_complete_playback_sha256":oracle.playback_hash,
        "source_frames":FULL,"duration_seconds":duration_s,"source_zero_frame":0,
        "sample_rate_hz":RATE,"channels":2,"resident_start_frame":START,"resident_end_frame":END,
        "resident_frames":END-START,"resident_pcm_bytes":warm_sample.samples.len()*4,
        "context":"FiniteLoop","window_revision":warm_sample.window_revision(),
        "cold_actual_native_ack":true,"fresh_warm_actual_native_ack":true,
        "cold_cached_full_pcm_pin":false,"fresh_warm_cached_full_pcm_pin":false,
        "cold_render_sha256":cold_render_hash,"fresh_warm_render_sha256":warm_render_hash,
        "cache_identity":warm_manifest["identity"],"warm_integrity":metrics,
        "scope":"byte integrity, native residency and hardware-free original-PCM render; no RAM, startup or hearing claim"
    });
    let output = PathBuf::from(
        std::env::var_os("FLITZI_C2A_FINITE_EVIDENCE").expect("finite evidence output path"),
    );
    assert!(output.is_absolute());
    fs::write(output, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    warm.shut_down().unwrap();
    assignment.release();
    drop(callback);
    drop(warm_sample);
    drop(consumer);
    drop(warm);
    drop(previous_directory);
}
