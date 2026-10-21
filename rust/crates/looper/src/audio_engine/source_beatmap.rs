//! Immutable, control-only variable source/quarter-note coordinates.
//!
//! Parsing proves supported structure, digest consistency and numeric geometry.
//! It does not prove decoded-source custody, musical quality, current pad
//! ownership, callback acknowledgement or permission to prepare/adopt audio.
//! Construction, serialization and all queries belong outside realtime.

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

pub(super) mod editor;
mod query;
mod record;

use query::{Evaluation, Projection};
use record::ValidatedMap;

pub(super) const MAX_ANCHORS: usize = 250_000;
pub(super) const MAX_RECORD_BYTES: usize = 64 * 1024 * 1024;
pub(super) const MAX_PROJECTION_POINTS: usize = 2_048;

/// Shared validated geometry, never a current Native source or audio permit.
#[pyclass(frozen)]
#[derive(Clone)]
pub struct SourceBeatmap {
    inner: Arc<ValidatedMap>,
}

impl SourceBeatmap {
    pub(super) fn canonical_record(&self) -> &str {
        &self.inner.encoded
    }

    pub(super) fn map_revision(&self) -> &str {
        &self.inner.revision
    }

    pub(super) fn parse(encoded: &str) -> Result<Self, String> {
        Ok(Self {
            inner: Arc::new(record::parse(encoded)?),
        })
    }
}

impl std::fmt::Debug for SourceBeatmap {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceBeatmap")
            .field("revision", &self.inner.revision)
            .field("source_identity", &self.inner.source_identity)
            .finish_non_exhaustive()
    }
}

#[pymethods]
impl SourceBeatmap {
    /// Validate data only; actual source acceptance requires separate custody checks.
    #[staticmethod]
    fn from_json(py: Python<'_>, record_json: String) -> PyResult<Self> {
        // Owned input keeps detached construction entirely on the Rust side.
        // Construct the Python exception only after the GIL is reacquired.
        py.detach(move || Self::parse(&record_json))
            .map_err(PyValueError::new_err)
    }

    /// Return a canonical supported envelope without any runtime authority.
    fn record_json(&self) -> &str {
        &self.inner.encoded
    }

    fn revision(&self) -> &str {
        &self.inner.revision
    }

    fn editor_info(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let result = PyDict::new(py);
        result.set_item("map_revision", &self.inner.revision)?;
        result.set_item("sample_rate_hz", self.inner.sample_rate_hz)?;
        result.set_item("source_duration_seconds", self.inner.source_duration)?;
        result.set_item("anchor_count", self.inner.anchors.len())?;
        result.set_item("can_undo", self.inner.can_undo)?;
        Ok(result.unbind())
    }

    #[pyo3(signature=(start_s,end_s,loop_start_s,px_per_s,max_points=2048))]
    fn editor_view(
        &self,
        py: Python<'_>,
        start_s: f64,
        end_s: f64,
        loop_start_s: f64,
        px_per_s: f64,
        max_points: usize,
    ) -> PyResult<Py<PyDict>> {
        let view = self.viewport(start_s, end_s, loop_start_s, px_per_s, max_points);
        let result = PyDict::new(py);
        result.set_item("map_revision", &self.inner.revision)?;
        result.set_item("status", view.status)?;
        result.set_item("step_beats", view.step)?;
        let lines = PyList::empty(py);
        for line in view.lines {
            let row = PyDict::new(py);
            row.set_item("source_seconds", line.seconds)?;
            row.set_item("loop_beat", line.loop_beat)?;
            row.set_item("major", line.major)?;
            row.set_item("reference", line.reference)?;
            row.set_item("meter_status", line.meter)?;
            lines.append(row)?;
        }
        result.set_item("lines", lines)?;
        let anchors = PyList::empty(py);
        for (seconds, beat) in view.anchors {
            let row = PyDict::new(py);
            row.set_item("source_seconds", seconds)?;
            row.set_item("beat", beat)?;
            let sample = seconds * f64::from(self.inner.sample_rate_hz);
            let sample_index = (sample >= 0.0 && sample < self.inner.source_frames as f64)
                .then(|| sample.round() as u64);
            row.set_item("sample_index", sample_index)?;
            anchors.append(row)?;
        }
        result.set_item("anchors", anchors)?;
        Ok(result.unbind())
    }

    #[pyo3(signature=(source_seconds,beats,require_four_four=true))]
    fn source_after_beats(
        &self,
        py: Python<'_>,
        source_seconds: f64,
        beats: f64,
        require_four_four: bool,
    ) -> PyResult<Py<PyDict>> {
        evaluation_dict(
            py,
            &self.inner,
            self.inner
                .source_after_beats(source_seconds, beats, require_four_four),
        )
    }

    pub(super) fn source_identity(&self) -> &str {
        &self.inner.source_identity
    }

    fn evaluate_source(&self, py: Python<'_>, source_seconds: f64) -> PyResult<Py<PyDict>> {
        evaluation_dict(py, &self.inner, self.inner.at_source(source_seconds))
    }

    fn evaluate_beat(&self, py: Python<'_>, beat: f64) -> PyResult<Py<PyDict>> {
        evaluation_dict(py, &self.inner, self.inner.at_beat(beat))
    }

    fn local_tempo_at_source(&self, py: Python<'_>, source_seconds: f64) -> PyResult<Py<PyDict>> {
        evaluation_dict(py, &self.inner, self.inner.at_source(source_seconds))
    }

    #[pyo3(signature = (source_seconds, step_beats = 0.0625))]
    fn nearest_grid_source(
        &self,
        py: Python<'_>,
        source_seconds: f64,
        step_beats: f64,
    ) -> PyResult<Py<PyDict>> {
        evaluation_dict(
            py,
            &self.inner,
            self.inner.nearest_grid(source_seconds, step_beats),
        )
    }

    #[pyo3(signature = (first_beat, last_beat, step_beats, max_points = 2048))]
    fn project_beats(
        &self,
        py: Python<'_>,
        first_beat: f64,
        last_beat: f64,
        step_beats: f64,
        max_points: usize,
    ) -> PyResult<Py<PyDict>> {
        projection_dict(
            py,
            &self.inner,
            self.inner
                .project(first_beat, last_beat, step_beats, max_points),
        )
    }

    fn diagnostic_extrapolate_source(
        &self,
        py: Python<'_>,
        source_seconds: f64,
    ) -> PyResult<Py<PyDict>> {
        evaluation_dict(
            py,
            &self.inner,
            self.inner.extrapolate_source(source_seconds),
        )
    }

    fn diagnostic_extrapolate_beat(&self, py: Python<'_>, beat: f64) -> PyResult<Py<PyDict>> {
        evaluation_dict(py, &self.inner, self.inner.extrapolate_beat(beat))
    }
}

fn evaluation_dict(py: Python<'_>, map: &ValidatedMap, value: Evaluation) -> PyResult<Py<PyDict>> {
    let result = PyDict::new(py);
    result.set_item("map_revision", &map.revision)?;
    result.set_item("source_seconds", value.source_seconds)?;
    result.set_item("beat", value.beat)?;
    result.set_item("local_bpm", value.local_bpm)?;
    result.set_item("coordinate_status", value.coordinate_status)?;
    result.set_item("interval_status", value.interval_status)?;
    result.set_item("segment_index", value.segment_index)?;
    result.set_item("physical_domain", value.physical_domain)?;
    result.set_item("meter_status", value.meter_status)?;
    // No field in this DTO represents actual source custody or audible adoption.
    Ok(result.unbind())
}

fn projection_dict(py: Python<'_>, map: &ValidatedMap, value: Projection) -> PyResult<Py<PyDict>> {
    let result = PyDict::new(py);
    result.set_item("map_revision", &map.revision)?;
    result.set_item("status", value.status)?;
    let points = PyList::empty(py);
    for point in value.points {
        points.append(evaluation_dict(py, map, point)?)?;
    }
    result.set_item("points", points)?;
    Ok(result.unbind())
}

#[cfg(test)]
mod tests;
