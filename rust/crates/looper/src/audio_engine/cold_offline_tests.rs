//! Real cold ACK to diagnostic offline ownership; no CPAL stream or device.
//!
//! These tests enter the productive cold worker and callback drain, then use the
//! production OfflineJobs admission and public OfflineAnalysisJob PyO3 methods.
//! The Python service supervisor remains covered by its own contract tests.

use super::*;
use crate::audio_engine::analysis_jobs::{OfflineAnalysisJob, OfflineRequestOwner};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

struct ColdOffline {
    engine: AudioEngine,
    directory: tempfile::TempDir,
    external: PathBuf,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    callback: Callback,
    loaded: SampleBuffer,
    generation: u64,
}

impl ColdOffline {
    fn new() -> Self {
        Python::initialize();
        let directory = tempfile::tempdir().unwrap();
        let external = directory.path().join("external.wav");
        constant_wav(&external, 8192);
        let engine = AudioEngine::new().unwrap();
        assert!(engine.stream_handle.is_none());
        let previous = old(&engine);
        let callback = Callback::new(&engine, &previous);
        let (producer, consumer) = rtrb::RingBuffer::new(8);
        let mut value = Self {
            engine,
            directory,
            external,
            producer: Arc::new(Mutex::new(producer)),
            consumer,
            callback,
            loaded: previous,
            generation: 7,
        };
        value.load_and_ack(1);
        value
    }

    fn load_and_ack(&mut self, expected_commands: usize) {
        let request = admit_for_format(
            &self.engine,
            0,
            self.external.to_string_lossy().into(),
            false,
            false,
            true,
            self.producer.clone(),
            2,
            48_000,
            self.directory.path().join("samples"),
        )
        .unwrap();
        wait_until(|| self.engine.input_runtime_ownership.cold_status(0, request) == Some(0));
        wait_until(|| self.consumer.slots() >= expected_commands);
        while let Ok(event) = self.engine.loader_rx.lock().unwrap().try_recv() {
            assert!(!matches!(event, LoaderEvent::Success { .. }));
        }
        assert_eq!(self.callback.drain(&mut self.consumer), expected_commands);
        assert_eq!(
            self.engine.input_runtime_ownership.cold_status(0, request),
            Some(2)
        );
        assert!(matches!(
            terminal(&self.engine, request),
            LoaderEvent::Success { .. }
        ));
        wait_until(|| self.engine.cold_loading[0].load(Ordering::Acquire) == 0);
        self.loaded = self.engine.sample_cache.lock().unwrap()[0].clone().unwrap();
        self.generation = request;
        assert_eq!(
            self.engine.loaded_source_generations.lock().unwrap()[0],
            (request, 48_000)
        );
        assert_eq!(self.loaded.channels, 2);
        assert_eq!(self.loaded.samples.len(), 256);
        assert!(self.callback.mixer.play_sample(0, 1.0));
        self.assert_playback_owner();
    }

    fn begin(&self) -> Result<OfflineAnalysisJob, String> {
        let sample = self.engine.sample_cache.lock().unwrap()[0].clone().unwrap();
        let (generation, rate) = self.engine.loaded_source_generations.lock().unwrap()[0];
        self.engine.offline_jobs.begin(
            0,
            sample,
            rate,
            generation,
            OfflineRequestOwner {
                request_ids: self.engine.pad_request_ids.clone(),
                prepared_epoch: self.engine.prepared_source_epochs[0].clone(),
            },
            self.engine.loader_tx.clone(),
        )
    }

    fn assert_playback_owner(&self) {
        let cache = self.engine.sample_cache.lock().unwrap();
        assert!(Arc::ptr_eq(
            &self.loaded.samples,
            &cache[0].as_ref().unwrap().samples
        ));
        assert_eq!(
            self.engine.loaded_source_generations.lock().unwrap()[0],
            (self.generation, 48_000)
        );
        assert!(
            self.engine
                .input_runtime_ownership
                .source_current(0, &self.loaded, 48_000)
        );
        assert!(self.callback.mixer.voices.iter().any(|voice| {
            voice.is_playing_sample(0)
                && voice
                    .sample
                    .as_ref()
                    .is_some_and(|sample| Arc::ptr_eq(&sample.samples, &self.loaded.samples))
        }));
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }
}

fn constant_wav(path: &std::path::Path, value: i16) {
    let mut bytes = wav(path, 48_000);
    bytes.truncate(44);
    let data = 128_u32 * 2;
    bytes[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    bytes[40..44].copy_from_slice(&data.to_le_bytes());
    for _ in 0..128 {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

fn dictionary(job: &OfflineAnalysisJob, stats: bool) -> Value {
    Python::attach(|py| {
        let value = if stats {
            job.staging_stats(py)
        } else {
            job.metadata(py)
        }
        .unwrap();
        let text: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (value,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&text).unwrap()
    })
}

fn identity(job: &OfflineAnalysisJob) -> Value {
    let metadata = dictionary(job, false);
    json!({"pad_id": metadata["pad_id"], "request_id": metadata["request_id"],
        "source_id": metadata["source_id"], "source_generation": metadata["source_generation"]})
}

fn envelope(job: &OfflineAnalysisJob, cancelled: bool) -> Value {
    let (beat, key) = if cancelled {
        (
            json!({"status":"cancelled"}),
            json!({"status":"cancelled","key":"unknown"}),
        )
    } else {
        (
            json!({"status":"unavailable"}),
            json!({"status":"failed","key":"unknown"}),
        )
    };
    json!({"schema_version":1,"identity":identity(job),"beat":beat,"key":key})
}

fn prepare(job: &OfflineAnalysisJob, path: &std::path::Path) {
    Python::attach(|py| {
        job.prepare_export(py, path.to_string_lossy().into())
            .unwrap()
    });
}

fn retire(job: &OfflineAnalysisJob) {
    Python::attach(|py| job.retire_pcm(py).unwrap());
}

fn finish(job: &OfflineAnalysisJob, result: &Value) -> bool {
    Python::attach(|py| job.finish(py, result.to_string()).unwrap())
}

fn completed(engine: &AudioEngine) -> Vec<(u64, Value)> {
    engine
        .loader_rx
        .lock()
        .unwrap()
        .try_iter()
        .filter_map(|event| match event {
            LoaderEvent::OfflineAnalysisCompleted {
                request_id,
                result_json,
                ..
            } => Some((request_id, serde_json::from_str(&result_json).unwrap())),
            _ => None,
        })
        .collect()
}

fn assert_mono_file(path: &std::path::Path, value: f32) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(bytes.len(), 128 * 4);
    assert!(
        bytes.chunks_exact(4).all(
            |bytes| f32::from_le_bytes(bytes.try_into().unwrap()).to_bits() == value.to_bits()
        )
    );
}

fn finish_cancelled(job: &OfflineAnalysisJob) {
    let result = envelope(job, true);
    job.cancel();
    retire(job);
    assert!(!finish(job, &result));
}

#[test]
fn cold_ack_offline_reuses_full_loaded_pcm_after_external_source_removal() {
    let fixture = ColdOffline::new();
    let original = fixture.engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .original_path
        .clone();
    fs::remove_file(&fixture.external).unwrap();
    assert!(fs::remove_file(&original).is_err());
    assert!(original.is_file());
    let playback_owners = Arc::strong_count(&fixture.loaded.samples);
    let job = fixture.begin().unwrap();
    assert_eq!(
        Arc::strong_count(&fixture.loaded.samples),
        playback_owners + 1
    );
    let metadata = dictionary(&job, false);
    assert_eq!(metadata["sample_rate_hz"], 48_000);
    assert_eq!(metadata["frame_count"], 128);
    assert_eq!(metadata["channels"], 2);
    assert_eq!(metadata["origin_seconds"], 0.0);
    assert_eq!(metadata["source_generation"], fixture.generation);
    let path = fixture.path("mono.f32le");
    prepare(&job, &path);
    assert_eq!(Arc::strong_count(&fixture.loaded.samples), playback_owners);
    assert_mono_file(&path, 0.25);
    let key: Value =
        Python::attach(|py| serde_json::from_str(&job.analyze_key(py).unwrap()).unwrap());
    assert_eq!(key["status"], "failed");
    assert_eq!(key["key"], "unknown");
    assert!(key["detail"].as_str().unwrap().contains("InsufficientData"));
    let mut result = envelope(&job, false);
    result["key"] = key;
    assert!(finish(&job, &result));
    assert_eq!(
        completed(&fixture.engine),
        [(metadata["request_id"].as_u64().unwrap(), result)]
    );
    fs::remove_file(path).unwrap();
    fixture.assert_playback_owner();
    let next = fixture.begin().unwrap();
    assert_eq!(
        dictionary(&next, false)["source_generation"],
        fixture.generation
    );
    finish_cancelled(&next);
}

#[test]
fn cold_ack_offline_actual_unload_and_new_ack_keep_stale_slot_busy_until_finish() {
    let mut fixture = ColdOffline::new();
    let job = fixture.begin().unwrap();
    let old_identity = identity(&job);
    let path = fixture.path("old-mono.f32le");
    prepare(&job, &path);
    unload_for_producer(&fixture.engine, 0, &fixture.producer).unwrap();
    assert!(job.is_cancelled());
    constant_wav(&fixture.external, -16384);
    fixture.load_and_ack(2);
    assert_ne!(
        fixture.generation,
        old_identity["source_generation"].as_u64().unwrap()
    );
    assert!(
        fixture
            .begin()
            .err()
            .unwrap()
            .contains("offline analysis busy")
    );
    assert!(completed(&fixture.engine).is_empty());
    retire(&job);
    fs::remove_file(path).unwrap();
    assert!(
        fixture
            .begin()
            .err()
            .unwrap()
            .contains("offline analysis busy")
    );
    assert!(!finish(&job, &envelope(&job, true)));
    assert!(completed(&fixture.engine).is_empty());
    let next = fixture.begin().unwrap();
    let next_identity = identity(&next);
    assert_ne!(next_identity["request_id"], old_identity["request_id"]);
    assert_eq!(next_identity["source_generation"], fixture.generation);
    let replacement_path = fixture.path("new-mono.f32le");
    prepare(&next, &replacement_path);
    assert_mono_file(&replacement_path, -0.5);
    assert!(finish(&next, &envelope(&next, false)));
    assert_eq!(completed(&fixture.engine).len(), 1);
    fs::remove_file(replacement_path).unwrap();
    fixture.assert_playback_owner();
}

#[test]
fn cold_ack_offline_public_stats_bound_source_export_and_complete_key_ownership() {
    let fixture = ColdOffline::new();
    let playback_owners = Arc::strong_count(&fixture.loaded.samples);
    let job = fixture.begin().unwrap();
    assert_eq!(
        Arc::strong_count(&fixture.loaded.samples),
        playback_owners + 1
    );
    let initial = dictionary(&job, true);
    assert_eq!(initial["retained_source_bytes"], 2 * 128 * 4);
    assert_eq!(initial["observed_export_peak_bytes"], 0);
    assert_eq!(initial["observed_key_peak_bytes"], 0);
    assert_eq!(initial["limit_bytes"], 512 * 1024 * 1024);
    assert_eq!(
        initial["admitted_peak_bytes"].as_u64().unwrap(),
        initial["admitted_export_peak_bytes"]
            .as_u64()
            .unwrap()
            .max(initial["admitted_key_peak_bytes"].as_u64().unwrap())
    );
    assert!(
        initial["admitted_peak_bytes"].as_u64().unwrap()
            <= initial["limit_bytes"].as_u64().unwrap()
    );
    let path = fixture.path("mono.f32le");
    prepare(&job, &path);
    assert_eq!(Arc::strong_count(&fixture.loaded.samples), playback_owners);
    let prepared = dictionary(&job, true);
    assert_eq!(prepared["retained_source_bytes"], 0);
    assert_eq!(prepared["export_file_bytes"], 128 * 4);
    assert_mono_file(&path, 0.25);
    assert!(
        prepared["observed_export_peak_bytes"].as_u64().unwrap()
            > initial["retained_source_bytes"].as_u64().unwrap()
    );
    assert!(
        prepared["observed_export_peak_bytes"].as_u64().unwrap()
            <= prepared["admitted_export_peak_bytes"].as_u64().unwrap()
    );
    assert_eq!(prepared["observed_key_peak_bytes"], 0);
    let key: Value =
        Python::attach(|py| serde_json::from_str(&job.analyze_key(py).unwrap()).unwrap());
    assert_eq!(key["status"], "failed");
    let converted = dictionary(&job, true);
    let complete_key_bytes = (128_u64 * 44_100).div_ceil(48_000) * 4;
    assert!(converted["observed_key_peak_bytes"].as_u64().unwrap() >= complete_key_bytes);
    assert!(
        converted["observed_key_peak_bytes"].as_u64().unwrap()
            <= converted["admitted_key_peak_bytes"].as_u64().unwrap()
    );
    assert_eq!(converted["retained_source_bytes"], 0);
    fixture.assert_playback_owner();
    finish_cancelled(&job);
    fs::remove_file(path).unwrap();
    let next = fixture.begin().unwrap();
    finish_cancelled(&next);
}

#[test]
fn cold_ack_offline_windows_staged_file_denies_mutation_until_public_retirement() {
    let fixture = ColdOffline::new();
    let job = fixture.begin().unwrap();
    let path = fixture.path("mono.f32le");
    prepare(&job, &path);
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    assert!(fs::remove_file(&path).is_err());
    assert_mono_file(&path, 0.25);
    job.cancel();
    retire(&job);
    retire(&job);
    assert!(
        fixture
            .begin()
            .err()
            .unwrap()
            .contains("offline analysis busy")
    );
    fs::OpenOptions::new().write(true).open(&path).unwrap();
    fs::remove_file(path).unwrap();
    Python::attach(|py| {
        assert!(
            job.analyze_key(py)
                .unwrap_err()
                .to_string()
                .contains("retired")
        )
    });
    assert!(!finish(&job, &envelope(&job, true)));
    assert!(completed(&fixture.engine).is_empty());
    fixture.assert_playback_owner();
    let next = fixture.begin().unwrap();
    finish_cancelled(&next);
}

#[test]
fn cold_ack_offline_missing_parent_failure_never_starts_key_and_releases_only_analysis_pin() {
    let fixture = ColdOffline::new();
    let playback_owners = Arc::strong_count(&fixture.loaded.samples);
    let job = fixture.begin().unwrap();
    let path = fixture.path("missing/mono.f32le");
    Python::attach(|py| {
        assert!(
            job.prepare_export(py, path.to_string_lossy().into())
                .is_err()
        )
    });
    assert!(!path.exists());
    let failed = dictionary(&job, true);
    assert_eq!(failed["retained_source_bytes"], 2 * 128 * 4);
    assert_eq!(failed["observed_export_peak_bytes"], 0);
    assert_eq!(failed["observed_key_peak_bytes"], 0);
    assert_eq!(
        Arc::strong_count(&fixture.loaded.samples),
        playback_owners + 1
    );
    retire(&job);
    assert_eq!(Arc::strong_count(&fixture.loaded.samples), playback_owners);
    assert_eq!(dictionary(&job, true)["retained_source_bytes"], 0);
    Python::attach(|py| {
        assert!(
            job.analyze_key(py)
                .unwrap_err()
                .to_string()
                .contains("retired")
        )
    });
    assert_eq!(dictionary(&job, true)["observed_key_peak_bytes"], 0);
    assert!(
        fixture
            .begin()
            .err()
            .unwrap()
            .contains("offline analysis busy")
    );
    fixture.assert_playback_owner();
    finish_cancelled(&job);
    assert!(completed(&fixture.engine).is_empty());
    let next = fixture.begin().unwrap();
    finish_cancelled(&next);
}

fn packed(values: &[f64]) -> String {
    STANDARD.encode(
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

#[test]
fn cold_ack_offline_complete_packed_publication_retires_real_pcm_once_and_readmits() {
    let fixture = ColdOffline::new();
    let job = fixture.begin().unwrap();
    let path = fixture.path("mono.f32le");
    prepare(&job, &path);
    assert_mono_file(&path, 0.25);
    let key: Value =
        Python::attach(|py| serde_json::from_str(&job.analyze_key(py).unwrap()).unwrap());
    assert_eq!(key["status"], "failed");
    let logits: Vec<f64> = (0..30_000)
        .map(|index| (f64::from(index) + 0.12345678901234568).sin())
        .collect();
    let reverse: Vec<f64> = logits.iter().rev().copied().collect();
    let beats = [-0.0, 1.0 / 48_000.0, 127.0 / 48_000.0];
    let downbeats = [0.0, 127.0 / 48_000.0];
    let mut result = envelope(&job, false);
    result["schema_version"] = json!(2);
    result["key"] = key;
    result["beat"] = json!({"identity": identity(&job), "status":"ready", "reason":"ready", "resources_released":true,
        "model":{"sha256":"a".repeat(64),"frontend_id":"fixture-reference-frontend","environment_id":"fixture-locked-cpu-environment",
            "package_version":"1.1.0","checkpoint":"final0","postprocessor":"minimal","device":"cpu","precision":"float32"},
        "predictions":{"encoding":"float64-le/base64","beat_seconds":packed(&beats),"downbeat_seconds":packed(&downbeats),
            "beat_logits":packed(&logits),"downbeat_logits":packed(&reverse)}});
    let mut legacy = result.clone();
    legacy["schema_version"] = json!(1);
    legacy["beat"]["predictions"] = json!({"beat_seconds":beats,"downbeat_seconds":downbeats,"beat_logits":logits,"downbeat_logits":reverse});
    assert!(legacy.to_string().len() > 1024 * 1024);
    assert!(result.to_string().len() <= 1024 * 1024);
    let request = identity(&job)["request_id"].as_u64().unwrap();
    assert!(finish(&job, &result));
    Python::attach(|py| assert!(job.finish(py, result.to_string()).is_err()));
    let emitted = completed(&fixture.engine);
    assert_eq!(emitted, [(request, result)]);
    for (name, expected) in [
        ("beat_seconds", beats.as_slice()),
        ("downbeat_seconds", downbeats.as_slice()),
        ("beat_logits", logits.as_slice()),
        ("downbeat_logits", reverse.as_slice()),
    ] {
        let bytes = STANDARD
            .decode(emitted[0].1["beat"]["predictions"][name].as_str().unwrap())
            .unwrap();
        let expected: Vec<u8> = expected
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        assert_eq!(bytes, expected, "packed binary64 changed: {name}");
    }
    assert_eq!(dictionary(&job, true)["retained_source_bytes"], 0);
    fs::remove_file(path).unwrap();
    fixture.assert_playback_owner();
    let next = fixture.begin().unwrap();
    assert_ne!(identity(&next)["request_id"], request);
    assert_eq!(identity(&next)["source_generation"], fixture.generation);
    finish_cancelled(&next);
}
