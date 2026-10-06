//! Non-realtime Python projection of the existing pure scalar source grid.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use super::source_grid::SourceGrid;

/// Continuous source-seconds projection with a full binary64 effective period.
#[pyclass(frozen)]
pub struct ScalarSourceGrid {
    grid: SourceGrid,
}

#[pymethods]
impl ScalarSourceGrid {
    #[new]
    fn new(origin_s: f64, seconds_per_beat: f64) -> PyResult<Self> {
        let grid = SourceGrid::from_period(seconds_per_beat, origin_s).ok_or_else(|| {
            PyValueError::new_err("origin must be finite and seconds_per_beat finite and positive")
        })?;
        Ok(Self { grid })
    }

    /// Return the continuous quarter-note coordinate, or None for invalid coordinates.
    fn beat_at_source(&self, source_s: f64) -> Option<f64> {
        self.grid.beat_at_source(source_s)
    }

    /// Evaluate one absolute grid position without rounding a beat or source frame.
    fn source_at_beat(&self, beat: f64) -> Option<f64> {
        self.grid.source_at_beat(beat)
    }

    /// Advance any source position by a musical duration while retaining its phase.
    fn source_after_beats(&self, source_s: f64, beats: f64) -> Option<f64> {
        self.grid.source_after_beats(source_s, beats)
    }
}
