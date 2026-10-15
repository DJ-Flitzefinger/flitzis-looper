//! Strict control-side projection parsing; no Python data enters the callback.

use super::input_mapping::PitchPadProjection;
use super::pitch_actions::PitchKeyMode;
use super::pitch_intent::PitchIntent;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyInt};

fn required<'py>(row: &Bound<'py, PyDict>, key: &str) -> PyResult<Bound<'py, PyAny>> {
    row.get_item(key)?
        .ok_or_else(|| PyValueError::new_err(format!("pitch projection missing {key}")))
}

fn integer<'py>(row: &Bound<'py, PyDict>, key: &str) -> PyResult<Bound<'py, PyAny>> {
    let value = required(row, key)?;
    if value.is_instance_of::<PyBool>() || !value.is_instance_of::<PyInt>() {
        return Err(PyValueError::new_err(format!(
            "pitch {key} must be an integer"
        )));
    }
    Ok(value)
}

fn optional_root(row: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<u8>> {
    if required(row, key)?.is_none() {
        return Ok(None);
    }
    let root: u8 = integer(row, key)?.extract()?;
    if root > 11 {
        return Err(PyValueError::new_err("pitch root is outside 0..11"));
    }
    Ok(Some(root))
}

fn optional_mode(row: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<PitchKeyMode>> {
    let value = required(row, key)?;
    if value.is_none() {
        return Ok(None);
    }
    PitchKeyMode::parse(&value.extract::<String>()?)
        .map(Some)
        .ok_or_else(|| PyValueError::new_err("pitch mode must be major or minor"))
}

pub(super) fn parse_rows(
    py: Python<'_>,
    rows: Vec<Py<PyDict>>,
) -> PyResult<Vec<PitchPadProjection>> {
    rows.iter()
        .map(|row| {
            let row = row.bind(py);
            let quantize_step_64ths = if required(row, "quantize_step_64ths")?.is_none() {
                None
            } else {
                Some(integer(row, "quantize_step_64ths")?.extract::<u32>()?)
            };
            let intent = PitchIntent::new(
                integer(row, "base_shift")?.extract()?,
                integer(row, "extra_shift")?.extract()?,
            )
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
            Ok(PitchPadProjection {
                source_root: optional_root(row, "source_root")?,
                source_mode: optional_mode(row, "source_mode")?,
                correction_root: optional_root(row, "correction_root")?,
                correction_mode: optional_mode(row, "correction_mode")?,
                intent,
                key_lock: required(row, "key_lock")?.extract()?,
                quantize_step_64ths,
                sync: required(row, "sync")?.extract()?,
                request_sequence: integer(row, "request_sequence")?.extract()?,
            })
        })
        .collect()
}
