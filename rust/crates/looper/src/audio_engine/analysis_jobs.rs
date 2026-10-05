//! Optional offline adapter ownership. Never called from the audio callback.
//!
//! One admitted request, including retiring native KeyNet work, owns its reservation
//! until both components settle. No queue and no replacement threads behind a busy slot.

use super::analysis_pcm::{
    KEY_PREPROCESSING, LoadedPcmSnapshot, MONO_RULE, PcmIdentity, SharedMono,
};
use super::analysis_predictions::validate_predictions;
use super::{current_pad_request_id, next_pad_request_id, pad_request_matches};
use crate::messages::{BackgroundTaskKind, LoaderEvent, SampleBuffer};
use flitzis_looper_analysis as analysis;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc::Sender};

const MAX_PCM_BYTES: usize = 512 * 1024 * 1024;
const MAX_RESULT_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct OfflineJobs {
    current: Mutex<Weak<JobState>>,
    busy: Arc<AtomicBool>,
}

impl OfflineJobs {
    pub fn cancel(&self, pad: Option<usize>) {
        if let Ok(slot) = self.current.lock()
            && let Some(job) = slot.upgrade()
            && pad.is_none_or(|id| id == job.identity.pad_id)
        {
            job.cancel_request();
        }
    }

    pub fn has_pad(&self, id: usize) -> bool {
        self.current.lock().is_ok_and(|slot| {
            slot.upgrade().is_some_and(|job| {
                job.identity.pad_id == id && !job.finished.load(Ordering::Acquire)
            })
        })
    }

    pub fn begin(
        &self,
        id: usize,
        sample: SampleBuffer,
        rate: u32,
        source_generation: u64,
        request_ids: Arc<Mutex<Vec<u64>>>,
        tx: Sender<LoaderEvent>,
    ) -> Result<OfflineAnalysisJob, String> {
        let mut slot = self
            .current
            .lock()
            .map_err(|_| "offline job lock poisoned")?;
        if self.busy.load(Ordering::Acquire) {
            return Err("offline analysis busy (running or retiring)".into());
        }
        let frames = sample
            .samples
            .len()
            .checked_div(sample.channels)
            .unwrap_or(0);
        let identity = PcmIdentity {
            pad_id: id,
            request_id: current_pad_request_id(&request_ids, id)?,
            source_id: format!("loaded-{id}-{source_generation}"),
            source_generation,
        };
        // Validate before changing accepted request intent.
        let snapshot = LoadedPcmSnapshot::new(sample, rate, identity.clone(), MAX_PCM_BYTES)
            .map_err(|e| e.to_string())?;
        if snapshot.working_pcm_bytes().map_err(|e| e.to_string())? > MAX_PCM_BYTES {
            return Err("offline analysis PCM byte limit exceeded".into());
        }
        let working_bytes = MAX_PCM_BYTES - snapshot.retained_bytes();
        let request_id = next_pad_request_id(&request_ids, id)?;
        let identity = PcmIdentity {
            request_id,
            ..identity
        };
        let state = Arc::new(JobState {
            identity,
            rate,
            frames,
            channels: snapshot.source_channels(),
            working_bytes,
            snapshot: Mutex::new(Some(snapshot)),
            mono: Mutex::new(None),
            request_ids,
            tx,
            cancelled: AtomicBool::new(false),
            preparing: AtomicBool::new(false),
            key_started: AtomicBool::new(false),
            key_running: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            transitions: Mutex::new(()),
            busy: self.busy.clone(),
            retirement_started: AtomicBool::new(false),
            released: AtomicBool::new(false),
        });
        self.busy.store(true, Ordering::Release);
        *slot = Arc::downgrade(&state);
        state.progress("Preparing offline analysis");
        Ok(OfflineAnalysisJob { state: Some(state) })
    }
}

struct JobState {
    identity: PcmIdentity,
    rate: u32,
    frames: usize,
    channels: usize,
    working_bytes: usize,
    snapshot: Mutex<Option<LoadedPcmSnapshot>>,
    mono: Mutex<Option<SharedMono>>,
    request_ids: Arc<Mutex<Vec<u64>>>,
    tx: Sender<LoaderEvent>,
    cancelled: AtomicBool,
    preparing: AtomicBool,
    key_started: AtomicBool,
    key_running: AtomicBool,
    finished: AtomicBool,
    transitions: Mutex<()>,
    busy: Arc<AtomicBool>,
    retirement_started: AtomicBool,
    released: AtomicBool,
}

impl JobState {
    fn cancel_request(&self) {
        // Serialize cancellation against publication. Invalidating the request
        // also rejects a queued completion if cancellation wins the race.
        if let Ok(mut requests) = self.request_ids.lock()
            && !self.finished.load(Ordering::Acquire)
        {
            self.cancelled.store(true, Ordering::Release);
            if let Some(current) = requests.get_mut(self.identity.pad_id)
                && *current == self.identity.request_id
            {
                *current = current.wrapping_add(1).max(1);
            }
        }
    }

    fn release_reservation(&self) {
        if !self.released.swap(true, Ordering::AcqRel) {
            self.busy.store(false, Ordering::Release);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || !pad_request_matches(
                &self.request_ids,
                self.identity.pad_id,
                self.identity.request_id,
            )
    }

    fn progress(&self, stage: &str) {
        if !self.cancelled() && !self.finished.load(Ordering::Acquire) {
            let _ = self.tx.send(LoaderEvent::TaskProgress {
                id: self.identity.pad_id,
                request_id: self.identity.request_id,
                task: BackgroundTaskKind::OfflineAnalysis,
                percent: 0.0,
                stage: stage.to_owned(),
            });
        }
    }

    fn prepare(&self, path: &str) -> Result<(), String> {
        let transition = self
            .transitions
            .lock()
            .map_err(|_| "offline transition lock poisoned")?;
        if self.finished.load(Ordering::Acquire)
            || self.cancelled()
            || self.retirement_started.load(Ordering::Acquire)
            || self.preparing.swap(true, Ordering::AcqRel)
        {
            return Err("offline preparation is unavailable".into());
        }
        let _running = RunningGuard(&self.preparing);
        drop(transition);
        let mut output = self.mono.lock().map_err(|_| "offline mono lock poisoned")?;
        if output.is_some() {
            return Err("offline PCM already prepared".into());
        }
        let snapshot = self
            .snapshot
            .lock()
            .map_err(|_| "offline snapshot lock poisoned")?;
        let snapshot = snapshot.as_ref().ok_or("offline PCM retired")?;
        let mut mono = snapshot
            .prepare_mono(MAX_PCM_BYTES, &|| self.cancelled())
            .map_err(|e| e.to_string())?;
        mono.identity = self.identity.clone();
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let mut writer = BufWriter::new(file);
        mono.write_f32_le(&mut writer, MAX_PCM_BYTES, &|| self.cancelled())
            .map_err(|e| e.to_string())?;
        std::io::Write::flush(&mut writer).map_err(|e| e.to_string())?;
        *output = Some(mono);
        Ok(())
    }

    fn key(&self) -> Result<String, String> {
        let transition = self
            .transitions
            .lock()
            .map_err(|_| "offline transition lock poisoned")?;
        if self.finished.load(Ordering::Acquire)
            || self.cancelled()
            || self.retirement_started.load(Ordering::Acquire)
            || self.key_started.swap(true, Ordering::AcqRel)
        {
            return Err("offline key branch already started or retired".into());
        }
        self.key_running.store(true, Ordering::Release);
        let _running = RunningGuard(&self.key_running);
        drop(transition);
        let result = (|| {
            let mono = self
                .mono
                .lock()
                .map_err(|_| "offline mono lock poisoned".to_owned())?;
            let mono = mono.as_ref().ok_or("offline PCM not prepared")?;
            let key_input = mono
                .key_input(self.working_bytes, &|| self.cancelled())
                .map_err(|e| e.to_string())?;
            if self.cancelled() {
                return Err("cancelled".into());
            }
            // CQT and ORT are noninterruptible here. This lease remains running
            // until the actual call returns; cancellation only invalidates publication.
            analysis::detect_key(&key_input, 44_100)
                .map(|r| r.key_name)
                .map_err(|e| format!("{e:?}"))
        })();
        let (status, key, detail) = if self.cancelled() {
            ("cancelled", "unknown".to_owned(), "cancelled".to_owned())
        } else {
            match result {
                Ok(key) => ("ready", key, String::new()),
                Err(error) => ("failed", "unknown".to_owned(), error),
            }
        };
        Ok(json!({"status":status,"key":key,"detail":detail,
            "provenance":format!("rust-keynet/{MONO_RULE}/{KEY_PREPROCESSING}")})
        .to_string())
    }

    fn finish(&self, result: &str) -> Result<bool, String> {
        let _transition = self
            .transitions
            .lock()
            .map_err(|_| "offline transition lock poisoned")?;
        if self.preparing.load(Ordering::Acquire) || self.key_running.load(Ordering::Acquire) {
            return Err("offline native work is still retiring".into());
        }
        let parsed = validate_envelope(
            result,
            &self.identity,
            self.frames as f64 / f64::from(self.rate),
        )?;
        if self.finished.load(Ordering::Acquire) {
            return Err("offline request already retired".into());
        }
        // Called by the supervisor only after process reaping and key join. Drop
        // all large native owners off-thread before releasing the admission slot.
        self.mono
            .lock()
            .map_err(|_| "offline mono lock poisoned")?
            .take();
        self.snapshot
            .lock()
            .map_err(|_| "offline snapshot lock poisoned")?
            .take();
        // Hold request identity through enqueue; invalidation and publication serialize.
        let requests = self
            .request_ids
            .lock()
            .map_err(|_| "request lock poisoned")?;
        let accepted = !self.cancelled.load(Ordering::Acquire)
            && requests.get(self.identity.pad_id) == Some(&self.identity.request_id);
        if accepted {
            let _ = self.tx.send(LoaderEvent::OfflineAnalysisCompleted {
                id: self.identity.pad_id,
                request_id: self.identity.request_id,
                result_json: parsed.to_string(),
            });
        }
        self.finished.store(true, Ordering::Release);
        self.release_reservation();
        Ok(accepted)
    }
}

struct RunningGuard<'a>(&'a AtomicBool);
impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn validate_envelope(result: &str, identity: &PcmIdentity, duration: f64) -> Result<Value, String> {
    if result.len() > MAX_RESULT_BYTES {
        return Err("offline result limit exceeded".into());
    }
    let parsed: Value = serde_json::from_str(result).map_err(|e| e.to_string())?;
    let schema_version = parsed["schema_version"]
        .as_u64()
        .filter(|version| matches!(version, 1 | 2))
        .ok_or("unsupported offline result schema")?;
    let got = &parsed["identity"];
    if got["pad_id"] != identity.pad_id
        || got["request_id"] != identity.request_id
        || got["source_id"] != identity.source_id
        || got["source_generation"] != identity.source_generation
    {
        return Err("offline result identity mismatch".into());
    }
    for component in ["beat", "key"] {
        if !matches!(
            parsed[component]["status"].as_str(),
            Some("ready" | "unavailable" | "failed" | "cancelled")
        ) {
            return Err("offline result component is not terminal".into());
        }
    }
    let beat = &parsed["beat"];
    if !beat["identity"].is_null() && beat["identity"] != *got {
        return Err("offline beat component identity mismatch".into());
    }
    if beat["resources_released"] == false {
        return Err("offline beat resources are still retiring".into());
    }
    if beat["status"] == "ready" {
        validate_ready_beats(beat, got, schema_version, duration)?;
    } else if schema_version == 2 && !beat["predictions"].is_null() {
        return Err("unsuccessful beat component has predictions".into());
    }
    if parsed["key"]["status"] == "ready"
        && !parsed["key"]["key"].as_str().is_some_and(valid_key_name)
    {
        return Err("invalid ready key component".into());
    }
    Ok(parsed)
}

fn valid_key_name(key: &str) -> bool {
    // Preserve previously accepted flat aliases while using the producer's
    // authoritative key names. The published result retains its supplied name.
    let canonical = match key {
        "Abm" => "G#m",
        "Ebm" => "D#m",
        "Bbm" => "A#m",
        "Ab" => "G#",
        "Eb" => "D#",
        "Bb" => "A#",
        key => key,
    };
    (0..24).any(|index| analysis::camelot_index_to_key(index) == Some(canonical))
}

fn validate_ready_beats(
    beat: &Value,
    identity: &Value,
    schema_version: u64,
    duration: f64,
) -> Result<(), String> {
    let model = &beat["model"];
    if beat["identity"] != *identity
        || beat["resources_released"] != true
        || model["package_version"] != "1.1.0"
        || model["checkpoint"] != "final0"
        || model["postprocessor"] != "minimal"
        || model["device"] != "cpu"
        || model["precision"] != "float32"
        || !model["sha256"].as_str().is_some_and(|hash| {
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        })
    {
        return Err("invalid ready beat identity/model/resources".into());
    }
    validate_predictions(&beat["predictions"], schema_version, duration)
}

/// Diagnostic adapter handle; all heavyweight methods release the GIL.
#[pyclass]
pub struct OfflineAnalysisJob {
    state: Option<Arc<JobState>>,
}

#[pymethods]
impl OfflineAnalysisJob {
    pub fn metadata(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("pad_id", self.state().identity.pad_id)?;
        dict.set_item("request_id", self.state().identity.request_id)?;
        dict.set_item("source_id", &self.state().identity.source_id)?;
        dict.set_item("source_generation", self.state().identity.source_generation)?;
        dict.set_item("sample_rate_hz", self.state().rate)?;
        dict.set_item("frame_count", self.state().frames)?;
        dict.set_item("channels", self.state().channels)?;
        dict.set_item("origin_seconds", 0.0)?;
        dict.set_item("mono_rule", MONO_RULE)?;
        Ok(dict.into_any().unbind())
    }
    pub fn prepare_export(&self, py: Python<'_>, path: String) -> PyResult<()> {
        py.detach(|| self.state().prepare(&path))
            .map_err(PyRuntimeError::new_err)
    }
    pub fn analyze_key(&self, py: Python<'_>) -> PyResult<String> {
        py.detach(|| self.state().key())
            .map_err(PyRuntimeError::new_err)
    }
    pub fn is_cancelled(&self) -> bool {
        self.state().cancelled()
    }
    pub fn cancel(&self) {
        self.state().cancel_request();
    }
    /// Retire admission after Python metadata/thread startup failure, off-thread.
    pub fn abort_unstarted(&self) -> PyResult<()> {
        let state = self
            .state
            .as_ref()
            .expect("live offline analysis handle")
            .clone();
        let transition = state
            .transitions
            .lock()
            .map_err(|_| PyRuntimeError::new_err("offline transition lock poisoned"))?;
        if state.preparing.load(Ordering::Acquire)
            || state.key_started.load(Ordering::Acquire)
            || state.finished.load(Ordering::Acquire)
            || state
                .mono
                .lock()
                .map_err(|_| PyRuntimeError::new_err("offline mono lock poisoned"))?
                .is_some()
        {
            return Err(PyRuntimeError::new_err("offline work already started"));
        }
        if state.retirement_started.swap(true, Ordering::AcqRel) {
            return Err(PyRuntimeError::new_err("offline work already retiring"));
        }
        state.cancel_request();
        let id = &state.identity;
        let envelope = json!({"schema_version":1,"identity":{"pad_id":id.pad_id,
            "request_id":id.request_id,"source_id":id.source_id,"source_generation":id.source_generation},
            "beat":{"status":"cancelled"},"key":{"status":"cancelled"}}).to_string();
        drop(transition);
        std::thread::spawn(move || {
            let _ = state.finish(&envelope);
        });
        Ok(())
    }
    pub fn progress(&self, stage: String) -> PyResult<()> {
        if stage.len() > 128 {
            return Err(PyValueError::new_err("offline stage too long"));
        }
        self.state().progress(&stage);
        Ok(())
    }
    pub fn finish(&self, py: Python<'_>, result_json: String) -> PyResult<bool> {
        py.detach(|| self.state().finish(&result_json))
            .map_err(PyRuntimeError::new_err)
    }
}

impl OfflineAnalysisJob {
    fn state(&self) -> &JobState {
        self.state.as_deref().expect("live offline analysis handle")
    }
}

impl Drop for OfflineAnalysisJob {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            state.cancel_request();
            if state.finished.load(Ordering::Acquire)
                || state.retirement_started.swap(true, Ordering::AcqRel)
            {
                return;
            }
            // Move the actual final owner, never race a cloned owner against UI Drop.
            std::thread::spawn(move || {
                if let Ok(mut mono) = state.mono.lock() {
                    mono.take();
                }
                if let Ok(mut snapshot) = state.snapshot.lock() {
                    snapshot.take();
                }
                state.release_reservation();
                drop(state);
            });
        }
    }
}

#[cfg(test)]
#[path = "analysis_job_tests.rs"]
mod tests;
