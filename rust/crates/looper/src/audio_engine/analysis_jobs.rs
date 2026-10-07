//! Optional offline adapter ownership. Never called from the audio callback.
//!
//! One admitted request, including retiring native KeyNet work, owns its reservation
//! until both components settle. No queue and no replacement threads behind a busy slot.

use super::analysis_pcm::{
    KEY_PREPROCESSING, LoadedPcmSnapshot, MONO_RULE, PcmIdentity, PcmStagingPlan,
    key_input_from_f32_le, staging_plan,
};
use super::analysis_predictions::validate_predictions;
use super::complete_context::{CompleteSourceGuard, CompleteSourceReader};
use super::{PadRequestAdvance, current_pad_request_id, next_pad_request_id, pad_request_matches};
use crate::messages::{BackgroundTaskKind, CompleteSourceIdentity, LoaderEvent, SampleBuffer};
use flitzis_looper_analysis as analysis;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc::Sender};

const MAX_PCM_BYTES: usize = 512 * 1024 * 1024;
const MAX_RESULT_BYTES: usize = 1024 * 1024;

/// Shared current-request publication boundary and its actual callback freshness epoch.
pub(super) struct OfflineRequestOwner {
    pub request_ids: Arc<Mutex<Vec<u64>>>,
    pub prepared_epoch: Arc<AtomicU64>,
}

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
            let _ = job.cancel_request();
        }
    }

    pub fn has_pad(&self, id: usize) -> bool {
        self.current.lock().is_ok_and(|slot| {
            slot.upgrade().is_some_and(|job| {
                job.identity.pad_id == id && !job.finished.load(Ordering::Acquire)
            })
        })
    }

    #[cfg(test)]
    pub fn begin(
        &self,
        id: usize,
        sample: SampleBuffer,
        rate: u32,
        source_generation: u64,
        owner: OfflineRequestOwner,
        tx: Sender<LoaderEvent>,
    ) -> Result<OfflineAnalysisJob, String> {
        self.begin_input(id, sample, rate, source_generation, owner, tx, None)
    }

    pub(super) fn begin_complete(
        &self,
        id: usize,
        reader: CompleteSourceReader,
        rate: u32,
        source_generation: u64,
        owner: OfflineRequestOwner,
        tx: Sender<LoaderEvent>,
    ) -> Result<OfflineAnalysisJob, String> {
        if reader.guard.as_ref().is_some_and(|guard| {
            !guard.current() || guard.generation != source_generation || guard.rate != rate
        }) {
            return Err("complete analysis source assignment changed before admission".into());
        }
        self.begin_input(
            id,
            reader.reference.clone(),
            rate,
            source_generation,
            owner,
            tx,
            Some(reader),
        )
    }

    #[allow(clippy::too_many_arguments)] // Existing offline identity + owner remain explicit at admission.
    fn begin_input(
        &self,
        id: usize,
        sample: SampleBuffer,
        rate: u32,
        source_generation: u64,
        owner: OfflineRequestOwner,
        tx: Sender<LoaderEvent>,
        reader: Option<CompleteSourceReader>,
    ) -> Result<OfflineAnalysisJob, String> {
        let OfflineRequestOwner {
            request_ids,
            prepared_epoch,
        } = owner;
        let mut slot = self
            .current
            .lock()
            .map_err(|_| "offline job lock poisoned")?;
        if self.busy.load(Ordering::Acquire) {
            return Err("offline analysis busy (running or retiring)".into());
        }
        let frames = sample.frame_count();
        let identity = PcmIdentity {
            pad_id: id,
            request_id: current_pad_request_id(&request_ids, id)?,
            source_id: format!("loaded-{id}-{source_generation}"),
            source_generation,
        };
        // Validate before changing accepted request intent.
        let source_guard = reader.as_ref().and_then(|reader| reader.guard.clone());
        let complete_source = sample.residency.as_ref().map(|view| view.source.clone());
        let snapshot = if let Some(reader) = reader {
            if sample
                .residency
                .as_ref()
                .is_some_and(|view| view.source.sample_rate_hz != rate)
            {
                return Err("complete analysis rate differs from immutable source".into());
            }
            OfflineInput::Complete(Box::new(reader))
        } else {
            OfflineInput::Loaded(
                LoadedPcmSnapshot::new(sample, rate, identity.clone(), MAX_PCM_BYTES)
                    .map_err(|e| e.to_string())?,
            )
        };
        let staging = snapshot.staging_plan(rate)?;
        if staging.export_peak_bytes.max(staging.key_peak_bytes) > MAX_PCM_BYTES
            || staging.export_bytes > MAX_PCM_BYTES
        {
            return Err("offline analysis PCM byte limit exceeded".into());
        }
        let retained_bytes = snapshot.retained_bytes()?;
        let request_id = next_pad_request_id(&request_ids, id, &prepared_epoch)?;
        let identity = PcmIdentity {
            request_id,
            ..identity
        };
        let state = Arc::new(JobState {
            identity,
            rate,
            frames,
            channels: snapshot.source_channels(),
            staging,
            snapshot: Mutex::new(Some(snapshot)),
            staged_pcm: Mutex::new(None),
            retained_source_bytes: AtomicUsize::new(retained_bytes),
            observed_export_peak_bytes: AtomicUsize::new(0),
            observed_key_peak_bytes: AtomicUsize::new(0),
            pcm_retired: AtomicBool::new(false),
            request_ids,
            prepared_epoch,
            source_guard,
            complete_source,
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

enum OfflineInput {
    Loaded(LoadedPcmSnapshot),
    Complete(Box<CompleteSourceReader>),
}

impl OfflineInput {
    fn retained_bytes(&self) -> Result<usize, String> {
        match self {
            Self::Loaded(snapshot) => Ok(snapshot.retained_bytes()),
            Self::Complete(reader) => reader.held_bytes(),
        }
    }
    fn source_channels(&self) -> usize {
        match self {
            Self::Loaded(snapshot) => snapshot.source_channels(),
            Self::Complete(reader) => reader.reference.channels,
        }
    }
    fn staging_plan(&self, rate: u32) -> Result<PcmStagingPlan, String> {
        match self {
            Self::Loaded(snapshot) => snapshot.staging_plan().map_err(|e| e.to_string()),
            Self::Complete(reader) => {
                let mut plan =
                    staging_plan(reader.held_bytes()?, reader.reference.frame_count(), rate)
                        .map_err(|e| e.to_string())?;
                plan.export_peak_bytes = reader.admit_scan(MAX_PCM_BYTES, 0)?;
                Ok(plan)
            }
        }
    }
    fn stream_f32_le(
        &self,
        output: &mut File,
        max_working_bytes: usize,
        max_export_bytes: usize,
        cancelled: &impl Fn() -> bool,
    ) -> Result<(), String> {
        match self {
            Self::Loaded(snapshot) => snapshot
                .stream_f32_le(output, max_working_bytes, max_export_bytes, cancelled)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Self::Complete(reader) => {
                reader.admit_scan(max_working_bytes, 0)?;
                if reader
                    .reference
                    .frame_count()
                    .checked_mul(4)
                    .is_none_or(|n| n > max_export_bytes)
                {
                    return Err("complete analysis export byte limit exceeded".into());
                }
                reader
                    .stream_mono(output, cancelled)
                    .map_err(|e| e.to_string())
            }
        }
    }
}

struct JobState {
    identity: PcmIdentity,
    rate: u32,
    frames: usize,
    channels: usize,
    staging: PcmStagingPlan,
    snapshot: Mutex<Option<OfflineInput>>,
    staged_pcm: Mutex<Option<File>>,
    retained_source_bytes: AtomicUsize,
    observed_export_peak_bytes: AtomicUsize,
    observed_key_peak_bytes: AtomicUsize,
    pcm_retired: AtomicBool,
    request_ids: Arc<Mutex<Vec<u64>>>,
    prepared_epoch: Arc<AtomicU64>,
    source_guard: Option<CompleteSourceGuard>,
    complete_source: Option<Arc<CompleteSourceIdentity>>,
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
    fn cancel_request(&self) -> Result<(), String> {
        // Serialize cancellation against publication. Invalidating the request
        // also rejects a queued completion if cancellation wins the race.
        if self.finished.load(Ordering::Acquire) {
            return Ok(());
        }
        // Teardown still cancels native results when a counter cannot advance.
        // Such a rejected transition preserves both shared counters, never wrapping.
        self.cancelled.store(true, Ordering::Release);
        let mut requests = self
            .request_ids
            .lock()
            .map_err(|_| "request lock poisoned")?;
        if !self.finished.load(Ordering::Acquire)
            && let Some(current) = requests.get_mut(self.identity.pad_id)
            && *current == self.identity.request_id
        {
            PadRequestAdvance::prepare(current, &self.prepared_epoch)?.commit();
        }
        Ok(())
    }

    fn release_reservation(&self) {
        if !self.released.swap(true, Ordering::AcqRel) {
            self.busy.store(false, Ordering::Release);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self
                .source_guard
                .as_ref()
                .is_some_and(|guard| !guard.current())
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
            || self.pcm_retired.load(Ordering::Acquire)
            || self.preparing.swap(true, Ordering::AcqRel)
        {
            return Err("offline preparation is unavailable".into());
        }
        let _running = RunningGuard(&self.preparing);
        drop(transition);
        let mut output = self
            .staged_pcm
            .lock()
            .map_err(|_| "offline staged PCM lock poisoned")?;
        if output.is_some() {
            return Err("offline PCM already prepared".into());
        }
        let mut snapshot = self
            .snapshot
            .lock()
            .map_err(|_| "offline snapshot lock poisoned")?;
        let source = snapshot.as_ref().ok_or("offline PCM retired")?;
        let mut file = create_staged_pcm(path).map_err(|e| e.to_string())?;
        self.observed_export_peak_bytes
            .store(self.staging.export_peak_bytes, Ordering::Release);
        source
            .stream_f32_le(&mut file, MAX_PCM_BYTES, MAX_PCM_BYTES, &|| {
                self.cancelled()
            })
            .map_err(|e| e.to_string())?;
        file.rewind().map_err(|e| e.to_string())?;
        // The complete file now owns the common mono. Release only this analysis
        // pin, on the preparation thread, before a complete key vector may exist.
        // The playback engine retains its own immutable source independently.
        snapshot.take();
        self.retained_source_bytes.store(0, Ordering::Release);
        *output = Some(file);
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
            || self.pcm_retired.load(Ordering::Acquire)
            || self.key_started.swap(true, Ordering::AcqRel)
        {
            return Err("offline key branch already started or retired".into());
        }
        self.key_running.store(true, Ordering::Release);
        let _running = RunningGuard(&self.key_running);
        drop(transition);
        let result = (|| {
            let key_input = {
                let mut staged = self
                    .staged_pcm
                    .lock()
                    .map_err(|_| "offline staged PCM lock poisoned".to_owned())?;
                let file = staged.as_mut().ok_or("offline PCM not prepared")?;
                if file.metadata().map_err(|e| e.to_string())?.len()
                    != self.staging.export_bytes as u64
                {
                    return Err("staged analysis PCM length changed".to_owned());
                }
                file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                key_input_from_f32_le(file, self.frames, self.rate, MAX_PCM_BYTES, &|| {
                    self.cancelled()
                })
                .map_err(|e| e.to_string())?
            };
            self.observed_key_peak_bytes
                .store(key_input.peak_bytes, Ordering::Release);
            if self.cancelled() {
                return Err("cancelled".into());
            }
            // CQT and ORT are noninterruptible here. This lease remains running
            // until the actual call returns; cancellation only invalidates publication.
            analysis::detect_key(&key_input.samples, 44_100)
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
        self.drop_pcm();
        // Hold request identity through enqueue; invalidation and publication serialize.
        let requests = self
            .request_ids
            .lock()
            .map_err(|_| "request lock poisoned")?;
        let accepted = !self.cancelled.load(Ordering::Acquire)
            && self
                .source_guard
                .as_ref()
                .is_none_or(|guard| guard.current())
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

    fn drop_pcm(&self) {
        // Cleanup only: even a poisoned mutex still owns valid Rust values.
        // Recover it solely to destroy those owners on this background thread.
        self.staged_pcm
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.retained_source_bytes.store(0, Ordering::Release);
    }

    /// Close native PCM owners before filesystem cleanup, retaining admission.
    /// The supervisor calls this only after both branch readers have stopped.
    fn retire_pcm(&self) -> Result<(), String> {
        let _transition = self
            .transitions
            .lock()
            .map_err(|_| "offline transition lock poisoned")?;
        if self.preparing.load(Ordering::Acquire) || self.key_running.load(Ordering::Acquire) {
            return Err("offline native work is still retiring".into());
        }
        self.pcm_retired.store(true, Ordering::Release);
        self.drop_pcm();
        Ok(())
    }
}

fn create_staged_pcm(path: &str) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ: allow the beat worker to read, while preventing a
        // writer or path replacement until both branches retire this handle.
        options.share_mode(0x0000_0001);
    }
    options.open(path)
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
        if let Some(source) = self.state().complete_source.as_ref() {
            let digest = |bytes: &[u8; 32]| {
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            };
            dict.set_item("complete_source_identity", digest(&source.transform_sha256))?;
            dict.set_item("original_sha256", digest(&source.original_sha256))?;
            dict.set_item("complete_playback_sha256", digest(&source.playback_sha256))?;
            dict.set_item("complete_mono_sha256", digest(&source.mono_sha256))?;
            dict.set_item("source_zero_frame", source.source_zero_frame)?;
        }
        Ok(dict.into_any().unbind())
    }
    pub fn prepare_export(&self, py: Python<'_>, path: String) -> PyResult<()> {
        py.detach(|| self.state().prepare(&path))
            .map_err(PyRuntimeError::new_err)
    }
    pub fn staging_stats(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let state = self.state();
        let dict = PyDict::new(py);
        dict.set_item("limit_bytes", MAX_PCM_BYTES)?;
        dict.set_item("export_file_bytes", state.staging.export_bytes)?;
        dict.set_item(
            "admitted_export_peak_bytes",
            state.staging.export_peak_bytes,
        )?;
        dict.set_item("admitted_key_peak_bytes", state.staging.key_peak_bytes)?;
        dict.set_item(
            "admitted_peak_bytes",
            state
                .staging
                .export_peak_bytes
                .max(state.staging.key_peak_bytes),
        )?;
        dict.set_item(
            "observed_export_peak_bytes",
            state.observed_export_peak_bytes.load(Ordering::Acquire),
        )?;
        dict.set_item(
            "observed_key_peak_bytes",
            state.observed_key_peak_bytes.load(Ordering::Acquire),
        )?;
        dict.set_item(
            "retained_source_bytes",
            state.retained_source_bytes.load(Ordering::Acquire),
        )?;
        Ok(dict.into_any().unbind())
    }
    pub fn analyze_key(&self, py: Python<'_>) -> PyResult<String> {
        py.detach(|| self.state().key())
            .map_err(PyRuntimeError::new_err)
    }
    pub fn is_cancelled(&self) -> bool {
        self.state().cancelled()
    }
    pub fn cancel(&self) {
        let _ = self.state().cancel_request();
    }
    pub fn retire_pcm(&self, py: Python<'_>) -> PyResult<()> {
        py.detach(|| self.state().retire_pcm())
            .map_err(PyRuntimeError::new_err)
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
                .staged_pcm
                .lock()
                .map_err(|_| PyRuntimeError::new_err("offline staged PCM lock poisoned"))?
                .is_some()
        {
            return Err(PyRuntimeError::new_err("offline work already started"));
        }
        if state.retirement_started.swap(true, Ordering::AcqRel) {
            return Err(PyRuntimeError::new_err("offline work already retiring"));
        }
        let _ = state.cancel_request();
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
            let _ = state.cancel_request();
            if state.finished.load(Ordering::Acquire)
                || state.retirement_started.swap(true, Ordering::AcqRel)
            {
                return;
            }
            // Move the actual final owner, never race a cloned owner against UI Drop.
            std::thread::spawn(move || {
                state.drop_pcm();
                state.release_reservation();
                drop(state);
            });
        }
    }
}

#[cfg(test)]
#[path = "analysis_job_tests.rs"]
mod tests;
