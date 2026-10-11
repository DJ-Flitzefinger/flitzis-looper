//! Control-side source-bound KEYLOCK admission and callback feedback projection.
//! PCM and voice authority remain with the existing runtime and mixer owners.

use super::AudioEngine;
use super::input_runtime_binding;
use crate::messages::ControlMessage;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

pub(super) fn enqueue(engine: &AudioEngine, id: usize, enabled: bool) -> PyResult<u64> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let binding = input_runtime_binding::capture_under_request_lock(engine, id)?;
    let mut producer = engine
        .control_producer()?
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "Failed to send SetPadKeyLock - buffer may be full",
        ));
    }
    engine.note_resident_key_lock_intent(id, enabled)?;
    let Some(binding) = binding else {
        // Saved-load intent may precede a current source. It has no source-bound
        // acknowledgement; zero must never settle a loaded controller operation.
        super::push_control_message(
            &mut producer,
            ControlMessage::SetPadKeyLock { id, enabled },
            "SetPadKeyLock",
        )?;
        return Ok(0);
    };
    if !binding.current() {
        return Err(PyValueError::new_err("current complete source changed"));
    }
    let request_id = engine.input_runtime_ownership.next_key_lock_request(id)?;
    super::push_control_message(
        &mut producer,
        ControlMessage::SetPadKeyLockRequest {
            id,
            enabled,
            request_id,
            binding: binding.binding,
            source_generation: binding.source_generation,
        },
        "SetPadKeyLock",
    )?;
    Ok(request_id)
}

pub(super) fn status(
    engine: &AudioEngine,
    py: Python<'_>,
    id: usize,
) -> PyResult<Option<Py<PyDict>>> {
    if id >= super::constants::NUM_SAMPLES {
        return Err(PyValueError::new_err("id out of range"));
    }
    let Some(binding) = input_runtime_binding::capture(engine, id)? else {
        return Ok(None);
    };
    let Some(status) = engine.input_runtime_ownership.key_lock_status(id) else {
        return Ok(None);
    };
    let window_revision = binding.binding.resident.map_or(0, |window| window.revision);
    if !binding.current()
        || status.source_generation != binding.source_generation
        || status.source_address != binding.binding.source_address
        || status.window_revision != window_revision
    {
        return Ok(None);
    }
    let result = PyDict::new(py);
    result.set_item(
        "source_id",
        format!("loaded-{id}-{}", status.source_generation),
    )?;
    result.set_item("source_generation", status.source_generation)?;
    result.set_item("source_identity", status.source_address)?;
    result.set_item("window_revision", status.window_revision)?;
    result.set_item("request_id", status.request_id)?;
    result.set_item("effective", status.effective)?;
    result.set_item("ready", status.ready)?;
    result.set_item("state", status.state)?;
    result.set_item("error", status.error)?;
    // Source revocation can race the control-side dictionary allocation. A
    // bounded final recheck prevents that snapshot becoming replacement truth.
    Ok(binding.current().then(|| result.unbind()))
}
