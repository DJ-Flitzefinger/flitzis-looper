//! Actual productive lane/kernel with virtual native commands; no audio device.
use super::super::buffer_retirement::ImmediateAudioBufferRetirement;
use super::super::mixer::RtMixer;
use super::*;
use crate::audio_engine::audio_stream::{AudioMessageSink, drain_control_messages};
use crate::audio_engine::buffer_retirement::{AudioBufferRetirement, RetiredAudioBuffer};
use crate::audio_engine::constants::MAX_VOICES;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

fn cache_entries(samples: &Path) -> Vec<PathBuf> {
    let mut roots = vec![samples.join(".pcm-cache/v1")];
    if let Ok(materials) = fs::read_dir(samples.join("materials")) {
        roots.extend(materials.map(|entry| entry.unwrap().path().join(".pcm-cache/v1")));
    }
    roots
        .into_iter()
        .filter_map(|root| fs::read_dir(root).ok())
        .flatten()
        .map(|entry| entry.unwrap().path())
        .collect()
}

fn original_files(samples: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(samples)
        .into_iter()
        .flatten()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect();
    if let Ok(materials) = fs::read_dir(samples.join("materials")) {
        for material in materials {
            if let Ok(originals) = fs::read_dir(material.unwrap().path().join("original")) {
                files.extend(originals.map(|entry| entry.unwrap().path()));
            }
        }
    }
    files
}

fn expected_imported_original(samples: &Path, filename: &str, bytes: &[u8]) -> PathBuf {
    let digest = format!("{:x}", Sha256::digest(bytes));
    let id = format!(
        "{:x}",
        Sha256::digest(format!("{digest}|{filename}").as_bytes())
    )[..32]
        .to_owned();
    samples
        .join("materials")
        .join(format!("M{id}"))
        .join("original")
        .join(filename)
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    wait_until_for(Duration::from_secs(10), &mut condition);
}
fn wait_until_for(timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "cold worker synchronization timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[cfg(windows)]
#[ignore = "isolated complete 600s cold/import then fresh warm/restore integrity measurement"]
fn c1b_productive_long_warm_integrity_cost() {
    use std::io::{Read, Seek, SeekFrom};
    fn full_hash(path: &std::path::Path) -> String {
        let mut file = fs::File::open(path).unwrap();
        let mut hash = Sha256::new();
        let mut bytes = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut bytes).unwrap();
            if count == 0 {
                break;
            }
            hash.update(&bytes[..count]);
        }
        format!("{:x}", hash.finalize())
    }
    // Independent integer PCM24 oracle: no productive decoder, resampler or
    // manifest helper participates in deriving complete expected sample bytes.
    fn pcm24_oracle(path: &std::path::Path) -> (String, String) {
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
                while remaining > 0 {
                    let count = remaining.min(bytes.len());
                    file.read_exact(&mut bytes[..count]).unwrap();
                    for raw in bytes[..count].chunks_exact(3) {
                        let integer =
                            (i32::from(raw[0]) | i32::from(raw[1]) << 8 | i32::from(raw[2]) << 16)
                                << 8
                                >> 8;
                        let sample = (integer as f32 / 8_388_608.0).to_le_bytes();
                        decoder.update(sample);
                        playback.update(sample);
                        playback.update(sample);
                    }
                    remaining -= count;
                }
                return (
                    format!("{:x}", decoder.finalize()),
                    format!("{:x}", playback.finalize()),
                );
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
    fn cpu_nanos() -> u64 {
        #[repr(C)]
        #[derive(Default)]
        struct Time {
            low: u32,
            high: u32,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
            fn GetProcessTimes(
                handle: *mut std::ffi::c_void,
                creation: *mut Time,
                exit: *mut Time,
                kernel: *mut Time,
                user: *mut Time,
            ) -> i32;
        }
        let (mut creation, mut exit, mut kernel, mut user) = (
            Time::default(),
            Time::default(),
            Time::default(),
            Time::default(),
        );
        // SAFETY: all FILETIME destinations are live, correctly sized values.
        assert_ne!(
            unsafe {
                GetProcessTimes(
                    GetCurrentProcess(),
                    &mut creation,
                    &mut exit,
                    &mut kernel,
                    &mut user,
                )
            },
            0
        );
        ((u64::from(kernel.high) << 32 | u64::from(kernel.low))
            + (u64::from(user.high) << 32 | u64::from(user.low)))
            * 100
    }
    let source = PathBuf::from(
        std::env::var_os("FLITZI_COLD_LONG_SOURCE").expect("retained long source path"),
    );
    assert!(source.is_absolute());
    let original_hash = full_hash(&source);
    let (decoder_oracle, playback_oracle) = pcm24_oracle(&source);
    let directory = tempfile::tempdir().unwrap();
    let samples_root = directory.path().join("samples");
    let previous_directory = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(directory.path()).unwrap();
    let mut cold = AudioEngine::new().unwrap();
    let previous = old(&cold);
    let mut callback = Callback::new(&cold, &previous);
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let request = admit_for_format(
        &cold,
        0,
        source.to_string_lossy().into(),
        false,
        false,
        true,
        Arc::new(Mutex::new(producer)),
        2,
        48_000,
        samples_root.clone(),
    )
    .unwrap();
    wait_until_for(Duration::from_secs(300), || consumer.peek().is_ok());
    assert_eq!(callback.drain(&mut consumer), 1);
    let LoaderEvent::Success { cached_path, .. } = terminal(&cold, request) else {
        panic!("cold import failed");
    };
    wait_until(|| cold.cold_loading[0].load(Ordering::Acquire) == 0);
    let manifest = cold.cold_source_manifest(0).unwrap().unwrap();
    let cold_manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    let mut assignment = cold
        .acquire_project_asset_lease(cached_path.clone())
        .unwrap();
    cold.shut_down().unwrap();
    drop(callback);
    drop(previous);
    drop(cold);
    // The durable assignment protects originals/caches across engine shutdown.
    assert!(directory.path().join(&cached_path).is_file());
    let warm = AudioEngine::new().unwrap();
    let previous = old(&warm);
    let mut callback = Callback::new(&warm, &previous);
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let wall = Instant::now();
    let cpu = cpu_nanos();
    let request = admit_for_format(
        &warm,
        0,
        cached_path.clone(),
        false,
        false,
        false,
        Arc::new(Mutex::new(producer)),
        2,
        48_000,
        samples_root.clone(),
    )
    .unwrap();
    wait_until_for(Duration::from_secs(300), || consumer.peek().is_ok());
    assert_eq!(callback.drain(&mut consumer), 1);
    let LoaderEvent::Success {
        cached_path: restored,
        duration_s,
        ..
    } = terminal(&warm, request)
    else {
        panic!("warm restore failed");
    };
    wait_until(|| warm.cold_loading[0].load(Ordering::Acquire) == 0);
    assert_eq!(restored, cached_path);
    assert_eq!(duration_s, 600.0);
    let wall_nanos = wall.elapsed().as_nanos();
    let process_cpu_nanos = cpu_nanos() - cpu;
    let warm_manifest: serde_json::Value =
        serde_json::from_str(&warm.cold_source_manifest(0).unwrap().unwrap()).unwrap();
    assert_eq!(warm_manifest["cache_path"], cold_manifest["cache_path"]);
    assert_eq!(warm_manifest["identity"], cold_manifest["identity"]);
    assert_eq!(warm_manifest["descriptor"], cold_manifest["descriptor"]);
    let cache_path = PathBuf::from(warm_manifest["cache_path"].as_str().unwrap());
    assert_eq!(full_hash(&cache_path.join("decoder.f32le")), decoder_oracle);
    assert_eq!(
        full_hash(&cache_path.join("playback.f32le")),
        playback_oracle
    );
    assert_eq!(
        full_hash(&directory.path().join(&cached_path)),
        original_hash
    );
    assert_eq!(
        warm_manifest["descriptor"]["decoder"]["pcm"]["interleaved_sha256"],
        decoder_oracle
    );
    assert_eq!(
        warm_manifest["descriptor"]["playback"]["pcm"]["interleaved_sha256"],
        playback_oracle
    );
    let mut actual_native_pcm = Sha256::new();
    for sample in warm.sample_cache.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .samples
        .iter()
    {
        actual_native_pcm.update(sample.to_le_bytes());
    }
    assert_eq!(
        format!("{:x}", actual_native_pcm.finalize()),
        playback_oracle
    );
    let metrics = &warm_manifest["integrity"];
    assert_eq!(metrics["warm"], true);
    assert_eq!(
        metrics["source_copied_bytes"],
        fs::metadata(&source).unwrap().len()
    );
    assert_eq!(
        metrics["snapshot_verify_bytes"],
        fs::metadata(&source).unwrap().len()
    );
    assert_eq!(metrics["decoder_verify_bytes"], 115_200_000_u64);
    assert_eq!(metrics["playback_verify_bytes"], 230_400_000_u64);
    assert_eq!(metrics["playback_read_bytes"], 230_400_000_u64);
    assert_eq!(metrics["original_copied_bytes"], 0);
    let evidence = serde_json::json!({
        "test":"c1b-productive-complete-warm-restore-v1", "seconds":duration_s,
        "actual_native_bank_ack":true, "wall_nanos_capture_through_ack":wall_nanos,
        "process_cpu_nanos_capture_through_ack":process_cpu_nanos,
        "independent_original_sha256":original_hash,
        "independent_pcm24_decoder_sha256":decoder_oracle,
        "independent_pcm24_stereo_playback_sha256":playback_oracle,
        "warm":warm_manifest, "cold":cold_manifest,
        "scope":"fresh engine saved original restore; full immutable integrity; no app automatic restore, device or performance acceptance",
    });
    let evidence_path =
        PathBuf::from(std::env::var_os("FLITZI_C1B_WARM_EVIDENCE").expect("evidence output path"));
    fs::write(evidence_path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    drop(callback);
    drop(previous);
    drop(warm);
    assignment.release();
    drop(previous_directory);
}

#[test]
#[cfg(windows)]
#[ignore = "explicit retained 600s PCM24 source via FLITZI_COLD_LONG_SOURCE"]
fn productive_long_cold_restore_has_full_decoder_and_playback_artifacts_before_actual_ack() {
    use std::io::Read;
    fn hash_file(path: &std::path::Path) -> String {
        let mut source = fs::File::open(path).unwrap();
        let mut hash = Sha256::new();
        let mut chunk = [0_u8; 64 * 1024];
        loop {
            let count = source.read(&mut chunk).unwrap();
            if count == 0 {
                break;
            }
            hash.update(&chunk[..count]);
        }
        format!("{:x}", hash.finalize())
    }
    let source = PathBuf::from(
        std::env::var_os("FLITZI_COLD_LONG_SOURCE")
            .expect("set FLITZI_COLD_LONG_SOURCE to the retained 600s/48k mono PCM24 WAV fixture"),
    );
    assert!(source.is_absolute() && source.is_file());
    let digest = hash_file(&source);
    let original_bytes = fs::metadata(&source).unwrap().len();
    let directory = tempfile::tempdir().unwrap();
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let started = Instant::now();
    let request = admit_for_format(
        &engine,
        0,
        source.to_string_lossy().into(),
        false,
        false,
        false,
        Arc::new(Mutex::new(producer)),
        2,
        48_000,
        directory.path().join("samples"),
    )
    .unwrap();
    wait_until_for(Duration::from_secs(300), || {
        while let Ok(event) = engine.loader_rx.lock().unwrap().try_recv() {
            assert!(
                !matches!(
                    event,
                    LoaderEvent::Error { .. } | LoaderEvent::Success { .. }
                ),
                "{event:?}"
            );
        }
        consumer.peek().is_ok()
    });
    assert!(engine.cold_leases.lock().unwrap()[0].is_none());
    callback.assert_old_voice(&previous);
    assert_eq!(callback.drain(&mut consumer), 1);
    let LoaderEvent::Success { duration_s, .. } = terminal(&engine, request) else {
        panic!("long cold restore did not succeed");
    };
    assert_eq!(duration_s, 600.0);
    let leases = engine.cold_leases.lock().unwrap();
    let lease = leases[0].as_ref().unwrap();
    let descriptor = &lease.manifest.descriptor;
    assert_eq!(descriptor["decoder"]["original"]["sha256"], digest);
    assert_eq!(descriptor["decoder"]["original"]["bytes"], original_bytes);
    assert_eq!(descriptor["decoder"]["pcm"]["rate_hz"], 48_000);
    assert_eq!(descriptor["decoder"]["pcm"]["channels"], 1);
    assert_eq!(descriptor["decoder"]["pcm"]["full_frames"], 28_800_000);
    assert_eq!(descriptor["playback"]["pcm"]["rate_hz"], 48_000);
    assert_eq!(descriptor["playback"]["pcm"]["channels"], 2);
    assert_eq!(descriptor["playback"]["pcm"]["full_frames"], 28_800_000);
    assert_eq!(
        fs::metadata(lease.cache_path.join("decoder.f32le"))
            .unwrap()
            .len(),
        115_200_000
    );
    assert_eq!(
        fs::metadata(lease.cache_path.join("playback.f32le"))
            .unwrap()
            .len(),
        230_400_000
    );
    assert_eq!(hash_file(&lease.original_path), digest);
    assert_eq!(hash_file(&source), digest);
    let decoder_digest = hash_file(&lease.cache_path.join("decoder.f32le"));
    let playback_digest = hash_file(&lease.cache_path.join("playback.f32le"));
    assert_eq!(
        descriptor["decoder"]["pcm"]["interleaved_sha256"],
        decoder_digest
    );
    assert_eq!(
        descriptor["playback"]["pcm"]["interleaved_sha256"],
        playback_digest
    );
    if let Some(evidence_path) = std::env::var_os("FLITZI_COLD_LONG_EVIDENCE") {
        fs::write(
            evidence_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "test":"productive-long-cold-restore-v1",
                "original_sha256":digest,
                "original_bytes":original_bytes,
                "decoder_interleaved_sha256":decoder_digest,
                "playback_interleaved_sha256":playback_digest,
                "duration_seconds":duration_s,
                "callback_acknowledged":true,
                "elapsed_seconds":started.elapsed().as_secs_f64(),
                "identity":lease.manifest.identity,
                "descriptor":descriptor,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    eprintln!(
        "long cold restore verified: original_bytes={original_bytes}, decoder_frames=28800000, playback_frames=28800000, elapsed_s={:.3}, identity={}",
        started.elapsed().as_secs_f64(),
        lease.manifest.identity
    );
    drop(leases);
}

struct Feedback {
    slots: usize,
    events: Vec<AudioMessage>,
}
impl AudioMessageSink for Feedback {
    fn available_audio_message_slots(&mut self) -> usize {
        self.slots
    }
    fn push_audio_message(&mut self, event: AudioMessage) {
        assert!(self.slots > 0);
        self.slots -= 1;
        self.events.push(event);
    }
}
struct Retirement {
    slots: usize,
    retired: Vec<RetiredAudioBuffer>,
}
impl Retirement {
    fn push(&mut self, value: RetiredAudioBuffer) {
        assert!(self.slots > 0);
        self.slots -= 1;
        self.retired.push(value);
    }
}
impl AudioBufferRetirement for Retirement {
    fn retire_resident_capture(
        &mut self,
        _: std::sync::Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: std::sync::Arc<std::sync::atomic::AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, value: Arc<std::sync::atomic::AtomicU8>) {
        self.push(RetiredAudioBuffer::ColdAdoption(value));
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.slots
    }
    fn retire_sample(&mut self, value: SampleBuffer) {
        self.push(RetiredAudioBuffer::Sample(value));
    }
    fn retire_prepared_stems(&mut self, value: crate::messages::PreparedStemSet) {
        self.push(RetiredAudioBuffer::PreparedStems(value));
    }
    fn retire_constant_timing(
        &mut self,
        value: crate::audio_engine::constant_timing::PreparedConstantTiming,
    ) {
        self.push(RetiredAudioBuffer::ConstantTiming(value));
    }
    fn retire_global_playback_batch(
        &mut self,
        value: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        self.push(RetiredAudioBuffer::GlobalPlaybackBatch(value));
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        value: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        self.push(RetiredAudioBuffer::AcceptedTimingRefresh(value));
    }
}
struct Callback {
    mixer: RtMixer,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    quantization: TriggerQuantization,
    feedback: Feedback,
    retirement: Retirement,
}
impl Callback {
    fn new(engine: &AudioEngine, previous: &SampleBuffer) -> Self {
        let mut value = Self {
            mixer: RtMixer::new(2, 48_000.0),
            scheduler: FixedCapacityScheduler::new(),
            transport: TransportTimeline::new(48_000),
            quantization: TriggerQuantization::Immediate,
            feedback: Feedback {
                slots: usize::MAX,
                events: Vec::new(),
            },
            retirement: Retirement {
                slots: usize::MAX,
                retired: Vec::new(),
            },
        };
        value
            .mixer
            .set_input_runtime_ownership(engine.input_runtime_ownership.clone());
        value
            .mixer
            .set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
        value
            .mixer
            .load_sample_rt(0, previous.clone(), &mut value.retirement);
        assert!(value.mixer.play_sample(0, 1.0));
        value
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
}

fn wav(path: &std::path::Path, rate: u32) -> Vec<u8> {
    let values = [0_i16, 8192, -16384, 32767, 0, -4096, 16384];
    let data = (values.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fs::write(path, &bytes).unwrap();
    bytes
}

fn terminal(engine: &AudioEngine, request: u64) -> LoaderEvent {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(event) = engine.loader_rx.lock().unwrap().try_recv()
            && matches!(&event, LoaderEvent::Success { request_id, .. } | LoaderEvent::Error { request_id, .. } if *request_id == request)
        {
            return event;
        }
        assert!(Instant::now() < deadline, "cold terminal timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn old(engine: &AudioEngine) -> SampleBuffer {
    let sample = SampleBuffer {
        residency: None,
        channels: 2,
        samples: Arc::from([0.25; 64]),
    };
    engine.sample_cache.lock().unwrap()[0] = Some(sample.clone());
    engine.loaded_source_generations.lock().unwrap()[0] = (7, 48_000);
    engine.loaded_source_digests.lock().unwrap()[0] = Some("old".into());
    engine.pad_request_ids.lock().unwrap()[0] = 7;
    engine
        .input_runtime_ownership
        .publish_source(0, &sample, 48_000, 7);
    sample
}

#[test]
#[cfg(windows)]
fn productive_cold_load_commits_exact_original_full_artifacts_and_guarded_command() {
    for rate in [44_100, 48_000, 96_000] {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("external.wav");
        let original = wav(&source, rate);
        let engine = AudioEngine::new().unwrap();
        let previous = old(&engine);
        let mut callback = Callback::new(&engine, &previous);
        let (producer, mut consumer) = rtrb::RingBuffer::new(4);
        let request = admit_for_format(
            &engine,
            0,
            source.to_string_lossy().into(),
            false,
            false,
            true,
            Arc::new(Mutex::new(producer)),
            2,
            48_000,
            directory.path().join("samples"),
        )
        .unwrap();
        wait_until(|| consumer.peek().is_ok());
        assert_eq!(
            engine.input_runtime_ownership.cold_status(0, request),
            Some(0)
        );
        assert!(engine.cold_leases.lock().unwrap()[0].is_none());
        callback.assert_old_voice(&previous);
        while let Ok(event) = engine.loader_rx.lock().unwrap().try_recv() {
            assert!(!matches!(
                event,
                LoaderEvent::Success { .. } | LoaderEvent::Error { .. }
            ));
        }
        assert_eq!(callback.drain(&mut consumer), 1);
        assert_eq!(
            engine.input_runtime_ownership.cold_status(0, request),
            Some(2)
        );
        let event = terminal(&engine, request);
        let LoaderEvent::Success {
            cached_path,
            duration_s,
            ..
        } = event
        else {
            panic!("{event:?}");
        };
        assert_eq!(
            fs::read(directory.path().join(cached_path)).unwrap(),
            original
        );
        assert_eq!(fs::read(&source).unwrap(), original);
        let leases = engine.cold_leases.lock().unwrap();
        let lease = leases[0].as_ref().unwrap();
        let manifest = &lease.manifest.descriptor;
        assert_eq!(
            manifest["decoder"]["original"]["sha256"],
            format!("{:x}", Sha256::digest(original))
        );
        assert_eq!(manifest["decoder"]["pcm"]["rate_hz"], rate);
        assert_eq!(manifest["decoder"]["pcm"]["full_frames"], 7);
        let frames = (7_u64 * 48_000).div_ceil(u64::from(rate));
        assert_eq!(manifest["playback"]["pcm"]["full_frames"], frames);
        assert_eq!(duration_s, frames as f64 / 48_000.0);
        assert_eq!(
            fs::metadata(lease.cache_path.join("decoder.f32le"))
                .unwrap()
                .len(),
            28
        );
        assert_eq!(
            fs::metadata(lease.cache_path.join("playback.f32le"))
                .unwrap()
                .len(),
            frames * 8
        );
        let sample = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
        assert!(!Arc::ptr_eq(&previous.samples, &sample.samples));
        assert!(callback.mixer.play_sample(0, 1.0));
        let voice = callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &sample.samples
        ));
        assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
        drop(leases);
    }
}

#[test]
#[cfg(windows)]
fn full_native_queue_rolls_back_artifacts_and_preserves_all_old_source_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("external.wav");
    wav(&source, 48_000);
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let (mut producer, _consumer) = rtrb::RingBuffer::new(1);
    producer.push(ControlMessage::Ping()).unwrap();
    let request = admit_for_format(
        &engine,
        0,
        source.to_string_lossy().into(),
        false,
        false,
        false,
        Arc::new(Mutex::new(producer)),
        2,
        48_000,
        directory.path().join("samples"),
    )
    .unwrap();
    let LoaderEvent::Error { error, .. } = terminal(&engine, request) else {
        panic!("expected queue rejection");
    };
    assert!(error.contains("buffer may be full"));
    assert!(Arc::ptr_eq(
        &previous.samples,
        &engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[0],
        (7, 48_000)
    );
    assert_eq!(
        engine.loaded_source_digests.lock().unwrap()[0].as_deref(),
        Some("old")
    );
    assert!(
        engine
            .input_runtime_ownership
            .source_current(0, &previous, 48_000)
    );
    assert!(engine.cold_leases.lock().unwrap()[0].is_none());
    wait_until(|| {
        cache_entries(&directory.path().join("samples")).is_empty()
            && original_files(&directory.path().join("samples")).is_empty()
    });
}

#[test]
fn worker_failure_preserves_source_and_reports_error() {
    let directory = tempfile::tempdir().unwrap();
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let (producer, _consumer) = rtrb::RingBuffer::new(2);
    let request = admit_for_format(
        &engine,
        0,
        directory
            .path()
            .join("missing.wav")
            .to_string_lossy()
            .into(),
        false,
        false,
        false,
        Arc::new(Mutex::new(producer)),
        2,
        48_000,
        directory.path().join("samples"),
    )
    .unwrap();
    assert!(matches!(
        terminal(&engine, request),
        LoaderEvent::Error { .. }
    ));
    assert!(
        engine
            .input_runtime_ownership
            .source_current(0, &previous, 48_000)
    );
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[0],
        (7, 48_000)
    );
}

#[test]
fn guarded_callback_preserves_existing_voice_when_source_is_revoked() {
    let engine = AudioEngine::new().unwrap();
    let sample = old(&engine);
    let mut mixer = RtMixer::new(2, 48_000.0);
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    mixer.load_sample_rt(0, sample.clone(), &mut ImmediateAudioBufferRetirement);
    engine.input_runtime_ownership.revoke_source(0);
    assert!(!mixer.cold_source_current(0, &sample, 7));
}

#[path = "cold_adoption_tests.rs"]
mod adoption;

#[cfg(windows)]
#[path = "cold_offline_tests.rs"]
mod offline;
