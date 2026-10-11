//! Immutable original custody for offline stems, separate from live publication.
//! Capture is bounded metadata/ownership work; full verification runs on a worker.

use super::cold_store::CommittedColdLease;
use super::prepared_source::version_matches_digest;
use super::{AudioEngine, constants::NUM_SAMPLES, has_active_task_for_id};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use std::path::Path;
use std::sync::atomic::Ordering;

/// The actual loaded original/cache/marker readers, retained through backend read end.
/// This object grants no assignment, timing, window or stem publication authority.
#[pyclass(frozen)]
pub struct OfflineStemSourceLease {
    id: usize,
    lease: CommittedColdLease,
    source_version: String,
    source_request: u64,
    source_generation: u64,
    target_shape: (u32, usize, usize),
    original_bytes: u64,
}

impl OfflineStemSourceLease {
    fn capture_under_analysis_guard(
        engine: &AudioEngine,
        id: usize,
        source_version: String,
    ) -> PyResult<Self> {
        // Offline analysis owns its slot before requesting the source mutex.
        // The caller holds that same slot order while making this snapshot.
        let requests = engine
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let loading = engine
            .loading_sample_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?;
        if loading.contains(&id) || engine.cold_loading[id].load(Ordering::Acquire) != 0 {
            return Err(PyValueError::new_err("sample is currently loading"));
        }
        let tasks = engine
            .active_tasks
            .lock()
            .map_err(|_| PyRuntimeError::new_err("task lock poisoned"))?;
        if has_active_task_for_id(&tasks, id) {
            return Err(PyValueError::new_err("sample task already running"));
        }
        let cache = engine
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
        let sample = cache[id]
            .as_ref()
            .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
        let generations = engine
            .loaded_source_generations
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
        let (generation, rate) = generations[id];
        if requests[id] == 0
            || generation == 0
            || rate == 0
            || engine
                .input_runtime_ownership
                .source_generation(id, sample, rate)
                != Some(generation)
            || engine.cold_lease_generations[id].load(Ordering::Acquire) != generation
        {
            return Err(PyValueError::new_err(
                "actual loaded source lease unavailable",
            ));
        }
        let digests = engine
            .loaded_source_digests
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
        let digest = digests[id]
            .as_ref()
            .filter(|digest| {
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            })
            .ok_or_else(|| PyValueError::new_err("loaded source digest unavailable"))?;
        if !version_matches_digest(&source_version, digest) {
            return Err(PyValueError::new_err(
                "source content does not match loaded source",
            ));
        }
        let leases = engine
            .cold_leases
            .lock()
            .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?;
        let lease = leases[id]
            .as_ref()
            .ok_or_else(|| PyValueError::new_err("complete source lease unavailable"))?;
        // Select only the exact pad lease. Equal PCM allocations/digests cannot
        // resolve another original/root through the shared reader registry.
        lease.verify_reference(sample).map_err(|error| {
            PyValueError::new_err(format!("loaded source lease disagrees: {error}"))
        })?;
        let original = &lease.manifest.descriptor["decoder"]["original"];
        let original_bytes = original["bytes"]
            .as_u64()
            .filter(|bytes| *bytes != 0)
            .ok_or_else(|| PyValueError::new_err("complete original extent unavailable"))?;
        if original["sha256"].as_str() != Some(digest.as_str()) {
            return Err(PyValueError::new_err(
                "loaded original manifest digest disagrees",
            ));
        }
        let (reference, _) = source_version
            .rsplit_once("|sha256-v1:")
            .ok_or_else(|| PyValueError::new_err("source version is malformed"))?;
        let relative = lease
            .original_path
            .strip_prefix(lease.samples_root())
            .map_err(|_| PyValueError::new_err("loaded original escaped samples root"))?;
        let own_reference = format!("samples/{}", relative.to_string_lossy().replace('\\', "/"));
        let own_absolute = super::material_paths::ordinary_windows_path(&lease.original_path);
        if reference != own_reference
            && Path::new(reference) != lease.original_path
            && Path::new(reference) != Path::new(&own_absolute)
        {
            return Err(PyValueError::new_err(
                "source path does not name this pad's original",
            ));
        }
        let target_shape = (rate, sample.channels, sample.frame_count());
        if target_shape.1 == 0
            || target_shape.2 == 0
            || !sample.valid_residency(rate, sample.channels)
        {
            return Err(PyValueError::new_err("loaded source geometry unavailable"));
        }
        Ok(Self {
            id,
            lease: lease.clone(),
            source_version,
            source_request: requests[id],
            source_generation: generation,
            target_shape,
            original_bytes,
        })
    }
}

pub(super) fn capture(
    engine: &AudioEngine,
    id: usize,
    source_version: String,
) -> PyResult<OfflineStemSourceLease> {
    if id >= NUM_SAMPLES {
        return Err(PyValueError::new_err("id out of range"));
    }
    engine.offline_jobs.with_pad_idle(id, |idle| {
        if !idle {
            return Err(PyValueError::new_err(
                "sample offline analysis already running",
            ));
        }
        OfflineStemSourceLease::capture_under_analysis_guard(engine, id, source_version)
    })
}

/// Current own source admission only. This intentionally ignores timing/window
/// epochs and the captured request number, which can change for the same source.
fn current_under_request_lock(
    engine: &AudioEngine,
    id: usize,
    source: &OfflineStemSourceLease,
    request_id: u64,
) -> PyResult<bool> {
    if id != source.id || request_id == 0 {
        return Ok(false);
    }
    let loading = engine
        .loading_sample_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?;
    if loading.contains(&id) || engine.cold_loading[id].load(Ordering::Acquire) != 0 {
        return Ok(false);
    }
    let tasks = engine
        .active_tasks
        .lock()
        .map_err(|_| PyRuntimeError::new_err("task lock poisoned"))?;
    if has_active_task_for_id(&tasks, id) {
        return Ok(false);
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let Some(sample) = cache[id].as_ref() else {
        return Ok(false);
    };
    let generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
    let (generation, rate) = generations[id];
    if generation != source.source_generation
        || engine.cold_lease_generations[id].load(Ordering::Acquire) != generation
        || (rate, sample.channels, sample.frame_count()) != source.target_shape
        || engine
            .input_runtime_ownership
            .source_generation(id, sample, rate)
            != Some(generation)
    {
        return Ok(false);
    }
    let digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
    if !digests[id]
        .as_ref()
        .is_some_and(|digest| version_matches_digest(&source.source_version, digest))
    {
        return Ok(false);
    }
    let leases = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?;
    Ok(leases[id].as_ref().is_some_and(|lease| {
        lease.assignment_id() == source.lease.assignment_id()
            && lease.original_path == source.lease.original_path
            && lease.samples_root() == source.lease.samples_root()
            && lease.verify_reference(sample).is_ok()
    }))
}

pub(super) fn is_current(
    engine: &AudioEngine,
    id: usize,
    source: &OfflineStemSourceLease,
) -> PyResult<bool> {
    if id >= NUM_SAMPLES {
        return Err(PyValueError::new_err("id out of range"));
    }
    engine.offline_jobs.with_pad_idle(id, |idle| {
        if !idle {
            return Ok(false);
        }
        let requests = engine
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        current_under_request_lock(engine, id, source, requests[id])
    })
}

pub(super) fn capture_prepared(
    engine: &AudioEngine,
    id: usize,
    source: &OfflineStemSourceLease,
) -> PyResult<super::prepared_source::PreparedSourceTicket> {
    if id >= NUM_SAMPLES {
        return Err(PyValueError::new_err("id out of range"));
    }
    engine.offline_jobs.with_pad_idle(id, |idle| {
        if !idle {
            return Err(PyValueError::new_err(
                "sample offline analysis already running",
            ));
        }
        let requests = engine
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        if !current_under_request_lock(engine, id, source, requests[id])? {
            return Err(PyValueError::new_err(
                "offline stem source admission is stale",
            ));
        }
        super::prepared_source::capture_prepared_source_under_request_lock(
            engine,
            id,
            source.source_version.clone(),
            requests[id],
        )
    })
}

#[pymethods]
impl OfflineStemSourceLease {
    #[getter]
    pub fn source_version(&self) -> &str {
        &self.source_version
    }

    #[getter]
    pub fn source_path(&self) -> String {
        super::material_paths::ordinary_windows_path(&self.lease.original_path)
    }

    #[getter]
    pub fn source_request(&self) -> u64 {
        self.source_request
    }

    #[getter]
    pub fn source_generation(&self) -> u64 {
        self.source_generation
    }

    #[getter]
    pub fn target_shape(&self) -> (u32, usize, usize) {
        self.target_shape
    }

    #[getter]
    pub fn original_bytes(&self) -> u64 {
        self.original_bytes
    }

    /// Worker-only full verification of the held original and its exact pathname.
    /// Origin unload/replacement invalidates interests, not these immutable readers.
    pub fn verify(&self, py: Python<'_>) -> PyResult<()> {
        py.detach(|| self.lease.verify_original_for_offline())
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }
}
