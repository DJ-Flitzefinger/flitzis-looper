//! Immutable, control-only variable source/quarter-note coordinates.
//!
//! Parsing proves supported structure, digest consistency and numeric geometry.
//! It does not prove decoded-source custody, musical quality, current pad
//! ownership, callback acknowledgement or permission to prepare/adopt audio.
//! Construction, serialization and Python queries belong outside realtime.
//! Prequalified Rust playback borrows bounded pure geometry without ownership changes.

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

pub(super) mod budget;
pub(super) mod editor;
pub(super) mod interpretation;
mod interval_edits;
mod manual_insert;
mod query;
mod record;
mod review_index;
mod review_window;

pub(crate) use query::{CanonicalBeatSegment, CanonicalLoopInterval, CanonicalPhaseReference};
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
    /// Borrowed pure geometry for already qualified callers; no audio permission.
    pub(crate) fn canonical_segment_at_beat(&self, beat: f64) -> Option<CanonicalBeatSegment> {
        self.inner.canonical_segment_at_beat(beat)
    }

    /// Borrow an original segment using a fallible monotone exact source comparison.
    pub(crate) fn canonical_segment_by_source_ordering(
        &self,
        compare: impl FnMut(f64) -> Option<std::cmp::Ordering>,
    ) -> Option<CanonicalBeatSegment> {
        self.inner.canonical_segment_by_source_ordering(compare)
    }

    /// Borrow an original segment using a fallible monotone exact beat comparison.
    pub(crate) fn canonical_segment_by_beat_ordering(
        &self,
        compare: impl FnMut(f64) -> Option<std::cmp::Ordering>,
    ) -> Option<CanonicalBeatSegment> {
        self.inner.canonical_segment_by_beat_ordering(compare)
    }

    /// Visit original trusted segments with positive overlap, outside realtime.
    /// The borrowed scan allocates nothing and conveys no source or audio permission.
    pub(crate) fn visit_canonical_segments(
        &self,
        beat_start: f64,
        beat_end: f64,
        visitor: impl FnMut(CanonicalBeatSegment) -> Option<()>,
    ) -> Option<()> {
        self.inner
            .visit_canonical_segments(beat_start, beat_end, visitor)
    }

    /// Visit the positive interior between caller-proved ordered exact boundaries.
    /// Comparisons return original knot beat versus the respective exact boundary.
    /// The caller separately proves a strictly positive extent; this off-RT scan
    /// retains original coverage and meter authority without a rounded inverse.
    pub(crate) fn visit_canonical_segments_exact(
        &self,
        compare_start: impl FnMut(f64) -> Option<std::cmp::Ordering>,
        compare_end: impl FnMut(f64) -> Option<std::cmp::Ordering>,
        require_four_four: bool,
        visitor: impl FnMut(CanonicalBeatSegment) -> Option<()>,
    ) -> Option<()> {
        self.inner.visit_canonical_segments_exact(
            compare_start,
            compare_end,
            require_four_four,
            visitor,
        )
    }

    /// Qualify the original positive interior's quarter or bar reference off realtime.
    /// Exact boundary ordering and musical closure remain the caller's proof;
    /// this metadata carries no source, adoption or actual-read permission.
    pub(crate) fn canonical_phase_reference_exact(
        &self,
        compare_start: impl FnMut(f64) -> Option<std::cmp::Ordering>,
        compare_end: impl FnMut(f64) -> Option<std::cmp::Ordering>,
    ) -> Option<CanonicalPhaseReference> {
        self.inner
            .canonical_phase_reference_exact(compare_start, compare_end)
    }

    pub(crate) fn canonical_source_at_beat(&self, beat: f64) -> Option<f64> {
        self.inner.at_beat(beat).source_seconds
    }

    pub(crate) fn canonical_beat_at_source(&self, source_seconds: f64) -> Option<f64> {
        self.inner.at_source(source_seconds).beat
    }

    /// Whole-interval coverage and slope extrema are resolved off realtime.
    pub(crate) fn canonical_loop_interval(
        &self,
        source_start: f64,
        quarters: f64,
        require_four_four: bool,
    ) -> Option<CanonicalLoopInterval> {
        self.inner
            .canonical_loop_interval(source_start, quarters, require_four_four)
    }

    pub(super) fn canonical_record(&self) -> &str {
        &self.inner.encoded
    }

    pub(super) fn map_revision(&self) -> &str {
        &self.inner.revision
    }

    pub(super) fn parse(encoded: &str) -> Result<Self, String> {
        Self::parse_with_context(encoded, 0, 0)
    }

    pub(super) fn parse_with_context(
        encoded: &str,
        held: usize,
        retained: usize,
    ) -> Result<Self, String> {
        Ok(Self {
            inner: Arc::new(record::parse(encoded, held, retained)?),
        })
    }

    pub(super) fn retained_bytes(&self) -> Result<usize, String> {
        self.inner.retained_bytes()
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
    #[classattr]
    const EDITOR_CORRECTION_API_VERSION: u32 = 4;

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

    fn editor_review_info(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        review_index::info(self, py)
    }

    #[pyo3(signature=(first_anchor_index,last_anchor_index,start_index,max_events=128))]
    fn editor_review_page(
        &self,
        py: Python<'_>,
        first_anchor_index: i64,
        last_anchor_index: i64,
        start_index: i64,
        max_events: i64,
    ) -> PyResult<Py<PyDict>> {
        review_index::page(
            self,
            py,
            first_anchor_index,
            last_anchor_index,
            start_index,
            max_events,
        )
    }

    /// Bounded diagnostics remain available even where interpolation is unsupported.
    fn editor_span(
        &self,
        py: Python<'_>,
        first_anchor_index: i64,
        last_anchor_index: i64,
    ) -> PyResult<Py<PyDict>> {
        let result = PyDict::new(py);
        result.set_item("map_revision", &self.inner.revision)?;
        let selection = usize::try_from(first_anchor_index)
            .ok()
            .zip(usize::try_from(last_anchor_index).ok())
            .filter(|(first, last)| first < last && *last < self.inner.anchors.len())
            .filter(|(first, last)| {
                let count = self.inner.anchors[*last].beat - self.inner.anchors[*first].beat;
                count.is_finite() && count > 0.0
            });
        let Some((first, last)) = selection else {
            result.set_item("status", "invalid_selection")?;
            for key in [
                "first_anchor",
                "last_anchor",
                "quarter_notes",
                "downbeat_label",
            ] {
                result.set_item(key, py.None())?;
            }
            result.set_item("interval_count", 0)?;
            for key in ["coverage", "meter_status", "downbeat_status"] {
                result.set_item(key, "unavailable")?;
            }
            return Ok(result.unbind());
        };
        result.set_item("status", "complete")?;
        for (key, index) in [("first_anchor", first), ("last_anchor", last)] {
            let anchor = self.inner.anchors[index];
            let row = PyDict::new(py);
            row.set_item("index", index)?;
            row.set_item("beat", anchor.beat)?;
            row.set_item("source_seconds", anchor.seconds)?;
            row.set_item("sample_index", self.inner.sample_index(anchor.seconds))?;
            result.set_item(key, row)?;
        }
        result.set_item(
            "quarter_notes",
            self.inner.anchors[last].beat - self.inner.anchors[first].beat,
        )?;
        result.set_item("interval_count", last - first)?;
        let segment = self.inner.segments[first];
        let homogeneous = |prefix: &[u32]| prefix[first] == prefix[last - 1];
        result.set_item(
            "coverage",
            if homogeneous(&self.inner.coverage_changes) {
                segment.coverage.label()
            } else {
                "mixed"
            },
        )?;
        result.set_item(
            "meter_status",
            if !homogeneous(&self.inner.meter_changes) {
                "mixed"
            } else if segment.four_four {
                "quarter_note_4_4_verified"
            } else {
                "unknown"
            },
        )?;
        let same_phase = homogeneous(&self.inner.phase_changes);
        result.set_item(
            "downbeat_status",
            if !same_phase {
                "mixed"
            } else if segment.downbeat_label.is_some() {
                "known"
            } else {
                "unknown"
            },
        )?;
        result.set_item(
            "downbeat_label",
            if same_phase {
                segment.downbeat_label
            } else {
                None
            },
        )?;
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
            row.set_item("downbeat", line.downbeat)?;
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
            row.set_item("sample_index", self.inner.sample_index(seconds))?;
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
    result.set_item("downbeat_label", value.downbeat_label)?;
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

#[cfg(test)]
pub(crate) mod render_comparison_tests;
