//! One unverified legacy-QM review. All construction, queries and retirement are off realtime.

use super::complete_context::NativeAnalysisOwner;
use super::project_assets::ProjectAssets;
use super::source_beatmap_artifact::SourceBeatmapRawArtifact;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub(super) struct RawReviewReceipt {
    retired: AtomicBool,
}

impl RawReviewReceipt {
    pub(super) fn retired(&self) -> bool {
        self.retired.load(Ordering::Acquire)
    }
}

struct RawReviewLeaseInner {
    assets: Arc<ProjectAssets>,
    receipt: Arc<RawReviewReceipt>,
}

impl Drop for RawReviewLeaseInner {
    fn drop(&mut self) {
        // The final lease follows actual temporary/raw reader destruction. The
        // observer owns only its receipt, never this admission or its readers.
        // DeferredMapReaders drop while the existing asset gate is held. An
        // atomic admission avoids recursively taking that same worker mutex.
        self.assets.raw_review_busy.store(false, Ordering::Release);
        self.receipt.retired.store(true, Ordering::Release);
    }
}

#[derive(Clone)]
pub(super) struct RawReviewLease(Arc<RawReviewLeaseInner>);

impl RawReviewLease {
    pub(super) fn reserve() -> Result<Self, String> {
        let assets = ProjectAssets::shared();
        assets
            .raw_review_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "raw source review is busy or retiring")?;
        Ok(Self(Arc::new(RawReviewLeaseInner {
            assets,
            receipt: Arc::new(RawReviewReceipt::default()),
        })))
    }

    pub(super) fn receipt(&self) -> Arc<RawReviewReceipt> {
        self.0.receipt.clone()
    }
}

/// No constructor/import/receipt/map/ACK API exists. Even equal captured bytes
/// remain tied to their original Native admission and its cancellation token.
#[pyclass(frozen)]
#[derive(Clone)]
pub struct SourceBeatmapRawCandidate {
    artifact: SourceBeatmapRawArtifact,
    owner: NativeAnalysisOwner,
    shutdown: Arc<AtomicBool>,
}

impl SourceBeatmapRawCandidate {
    pub(super) fn review_artifact(
        &self,
        engine: &super::AudioEngine,
        id: usize,
        generation: u64,
        intent: &super::source_beatmap::interpretation::Interpretation,
    ) -> PyResult<(SourceBeatmapRawArtifact, super::InputRuntimePadBinding)> {
        if id >= super::NUM_SAMPLES || generation == 0 {
            return Err(PyValueError::new_err("raw review source is out of range"));
        }
        let _requests = engine
            .pad_request_ids
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("request lock poisoned"))?;
        let binding = super::input_runtime_binding::capture_under_request_lock(engine, id)?
            .ok_or_else(|| PyValueError::new_err("raw review source is unavailable"))?;
        if self.status() != "ready"
            || binding.source_generation != generation
            || !self.owner.source_map_admission_current(&binding)
            || !matches!(self.owner.observe_control(), super::content_placement::PlacementObservation::Current(place) if place.slot() == id)
        {
            return Err(PyValueError::new_err(
                "raw review original candidate is stale or placement pending",
            ));
        }
        intent
            .validate_for(self.artifact.raw())
            .map_err(PyValueError::new_err)?;
        // Clone the actual artifact reader/lease, never the old cancellation owner.
        Ok((self.artifact.clone(), binding))
    }

    pub(super) fn new(
        artifact: SourceBeatmapRawArtifact,
        owner: NativeAnalysisOwner,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            artifact,
            owner,
            shutdown,
        }
    }

    pub(super) fn status_for_review(&self) -> &'static str {
        self.status()
    }

    fn status(&self) -> &'static str {
        use super::content_placement::PlacementObservation;
        if self.shutdown.load(Ordering::Acquire) || self.owner.cancellation_requested() {
            return "cancelled";
        }
        match self.owner.observe_control() {
            PlacementObservation::Current(_) => "ready",
            PlacementObservation::Pending => "placement_pending",
            PlacementObservation::Stale | PlacementObservation::Retired => "stale",
        }
    }
}

#[pymethods]
impl SourceBeatmapRawCandidate {
    #[classattr]
    const RAW_REVIEW_API_VERSION: u32 = 1;

    fn info(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let before = self.status();
        let data = self.artifact.raw();
        let result = PyDict::new(py);
        result.set_item("raw_revision", &data.raw_revision)?;
        result.set_item("decoder_identity", &data.decoder_identity)?;
        result.set_item("source_sample_rate_hz", data.source_sample_rate_hz)?;
        result.set_item("source_frame_count", data.source_frame_count)?;
        result.set_item("source_zero_frame", data.source_zero_frame)?;
        result.set_item("mono_sha256", &data.mono_sha256)?;
        result.set_item("mono_rule", &data.mono_rule)?;
        result.set_item("analyzer_sha256", &data.analyzer_sha256)?;
        result.set_item("analyzer_sample_rate_hz", data.analyzer_sample_rate_hz)?;
        result.set_item("analyzer_frame_count", data.analyzer_frame_count)?;
        result.set_item("conversion_revision", &data.conversion_revision)?;
        result.set_item("odf_hop_samples", data.odf_hop_samples)?;
        result.set_item("detector_id", &data.detector_id)?;
        result.set_item("code_revision", &data.code_revision)?;
        let configuration = PyDict::new(py);
        let value = data.configuration.value();
        for (key, value) in value.as_object().expect("typed detector configuration") {
            if let Some(value) = value.as_u64() {
                configuration.set_item(key, value)?;
            } else if let Some(value) = value.as_f64() {
                configuration.set_item(key, value)?;
            } else {
                return Err(PyValueError::new_err(
                    "unsupported detector configuration value",
                ));
            }
        }
        result.set_item("configuration", configuration)?;
        result.set_item("event_count", data.beat_frames.len())?;
        result.set_item("detected_downbeat_count", data.downbeat_indices.len())?;
        result.set_item(
            "outside_source_event_count",
            data.outside_source_event_count(),
        )?;
        result.set_item("interpretation", "unverified")?;
        result.set_item("timing_uncertainty", "unknown")?;
        let after = self.status();
        result.set_item("status", if before == "ready" { after } else { before })?;
        Ok(result.unbind())
    }

    #[pyo3(signature=(start_index,max_events=128))]
    fn inspect(&self, py: Python<'_>, start_index: i64, max_events: i64) -> PyResult<Py<PyDict>> {
        let data = self.artifact.raw();
        let start = usize::try_from(start_index)
            .ok()
            .filter(|start| *start <= data.beat_frames.len())
            .ok_or_else(|| PyValueError::new_err("raw event start is outside 0..=event_count"))?;
        let maximum = usize::try_from(max_events)
            .ok()
            .filter(|maximum| (1..=128).contains(maximum))
            .ok_or_else(|| {
                PyValueError::new_err("raw inspection max_events must be within 1..=128")
            })?;
        let before = self.status();
        let end = start.saturating_add(maximum).min(data.beat_frames.len());
        let rows = PyList::empty(py);
        if before == "ready" {
            for index in start..end {
                let frame = data.beat_frames[index];
                let seconds = data.source_seconds(frame);
                let duration =
                    data.source_frame_count as f64 / f64::from(data.source_sample_rate_hz);
                let row = PyDict::new(py);
                row.set_item("raw_index", index)?;
                row.set_item("odf_frame", frame)?;
                row.set_item("source_seconds", seconds)?;
                row.set_item(
                    "detected_downbeat",
                    data.downbeat_indices.binary_search(&index).is_ok(),
                )?;
                row.set_item(
                    "physical_domain",
                    if seconds < duration {
                        "inside"
                    } else if seconds == duration {
                        "exclusive_end"
                    } else {
                        "outside"
                    },
                )?;
                rows.append(row)?;
            }
        }
        let after = self.status();
        let status = if before == "ready" { after } else { before };
        if status != "ready" {
            rows.del_slice(0, rows.len())?;
        }
        let result = PyDict::new(py);
        result.set_item("status", status)?;
        result.set_item("raw_revision", &data.raw_revision)?;
        result.set_item("start_index", start)?;
        result.set_item("returned_count", rows.len())?;
        result.set_item("total_event_count", data.beat_frames.len())?;
        result.set_item(
            "has_more",
            status == "ready" && end < data.beat_frames.len(),
        )?;
        result.set_item("events", rows)?;
        Ok(result.unbind())
    }
}
