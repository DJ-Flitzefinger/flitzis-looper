//! Control-side source-bound KEYLOCK admission and callback feedback projection.
//! PCM and voice authority remain with the existing runtime and mixer owners.

use super::AudioEngine;
use super::input_runtime_binding;
use crate::messages::ControlMessage;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::atomic::Ordering;

pub(super) fn enqueue_with_source(
    engine: &AudioEngine,
    id: usize,
    enabled: bool,
    expected_source_generation: Option<u64>,
) -> PyResult<u64> {
    enqueue_with_source_before_commit(engine, id, enabled, expected_source_generation, || {})
}

fn enqueue_with_source_before_commit(
    engine: &AudioEngine,
    id: usize,
    enabled: bool,
    expected_source_generation: Option<u64>,
    before_commit: impl FnOnce(),
) -> PyResult<u64> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(id)?;
    let ownership = &engine.input_runtime_ownership;
    // None from the loaded binding resolver can also mean Pending. Capture the
    // actual empty or loaded slot first; only its exact empty incarnation is saved intent.
    let placement = ownership
        .slot_snapshot(id)
        .ok_or_else(|| PyValueError::new_err("content placement is pending or changed"))?;
    let binding = input_runtime_binding::capture_under_request_lock(engine, id)?;
    input_runtime_binding::validate_expected_source_generation(
        binding.as_ref(),
        expected_source_generation,
    )?;
    if binding.as_ref().is_some_and(|binding| {
        !binding
            .binding
            .placement
            .is_some_and(|captured| captured.same_projection(placement))
    }) || (binding.is_none()
        && (placement.content_key().is_some() || !ownership.source_cell_empty(placement.cell())))
    {
        return Err(PyValueError::new_err("current complete source changed"));
    }
    engine.admit_resident_key_lock(id, enabled)?;
    let prepared = ownership
        .prepare_slot_write_fixed(placement, placement.lifetime())
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    let previous_request = ownership
        .key_lock_request_for_cell(placement.cell())
        .load(Ordering::Acquire);
    let request_id = if binding.is_some() {
        previous_request
            .checked_add(1)
            .ok_or_else(|| PyRuntimeError::new_err("Key Lock request epoch exhausted"))?
    } else {
        0
    };
    let restore_guards = engine
        .resident_restore_guards
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident intent lock poisoned"))?;
    let cold_request = engine.cold_loading[id].load(Ordering::Acquire);
    let restore_guard = restore_guards[id]
        .as_ref()
        .filter(|guard| guard.request == cold_request);
    let message = if let Some(binding) = &binding {
        ControlMessage::SetPadKeyLockRequest {
            id,
            enabled,
            request_id,
            binding: binding.binding,
            source_generation: binding.source_generation,
        }
    } else {
        ControlMessage::SetPadKeyLockSavedIntent { placement, enabled }
    };
    let mut producer = engine
        .control_producer()?
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "Failed to send SetPadKeyLock - buffer may be full",
        ));
    }
    before_commit();
    ownership
        .commit_slot_write_fixed(
            &prepared,
            || {
                ownership.slot_snapshot_fields_current(placement)
                    && ownership.copy_reservation(id) == 0
                    && ownership
                        .key_lock_request_for_cell(placement.cell())
                        .load(Ordering::Acquire)
                        == previous_request
                    && engine.cold_loading[id].load(Ordering::Acquire) == cold_request
                    && binding.as_ref().map_or_else(
                        || ownership.source_cell_empty(placement.cell()),
                        |binding| binding.control_publication_current(),
                    )
            },
            |cell| {
                if let Some(guard) = restore_guard {
                    guard.key_lock_intent(enabled);
                }
                if request_id != 0 {
                    ownership
                        .key_lock_request_for_cell(cell)
                        .store(request_id, Ordering::Release);
                }
                // Capacity is held by the sole producer; its consumer only frees slots.
                let sent = producer.push(message);
                debug_assert!(sent.is_ok());
            },
        )
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    Ok(request_id)
}

#[cfg(test)]
#[path = "key_lock_control_tests.rs"]
mod key_lock_control_tests;

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

pub(super) fn source_sync_status(
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
    let Some(status) = engine.input_runtime_ownership.source_sync_status(id) else {
        return Ok(None);
    };
    if !binding.current()
        || status.source_generation != binding.source_generation
        || status.source_address != binding.binding.source_address
        || status.window_revision != binding.binding.resident.map_or(0, |window| window.revision)
    {
        return Ok(None);
    }
    let result = PyDict::new(py);
    result.set_item("source_generation", status.source_generation)?;
    result.set_item("source_identity", status.source_address)?;
    result.set_item("window_revision", status.window_revision)?;
    result.set_item("active", status.active)?;
    let fixed_effective = status.policy.is_some_and(|policy| {
        policy.request.mode == super::source_sync_policy::SourceSyncMode::Fixed && policy.settled
    });
    result.set_item("effective", status.witness.is_some() || fixed_effective)?;
    result.set_item(
        "policy_mode",
        status.policy.map(|policy| policy.request.mode.name()),
    )?;
    result.set_item(
        "policy_revision",
        status
            .policy
            .and_then(|policy| policy.request.global_revision),
    )?;
    result.set_item(
        "policy_sync",
        status.policy.and_then(|policy| policy.request.policy_sync),
    )?;
    result.set_item(
        "policy_realtime_bpm",
        status
            .policy
            .and_then(|policy| policy.request.policy_realtime_bpm),
    )?;
    result.set_item("settled", status.policy.map(|policy| policy.settled))?;
    result.set_item(
        "intent_revision",
        status
            .policy
            .map(|policy| policy.request.intent_revision)
            .or(status.witness.map(|witness| witness.intent_revision)),
    )?;
    result.set_item(
        "map_revision",
        status.witness.map(|witness| {
            witness
                .map_revision
                .iter()
                .map(|word| format!("{word:016x}"))
                .collect::<String>()
        }),
    )?;
    // Dictionary allocation is control-only. A replacement during it must not
    // project the old callback witness onto a new source or content incarnation.
    Ok(
        (binding.current()
            && engine.input_runtime_ownership.source_sync_status(id) == Some(status))
        .then(|| result.unbind()),
    )
}
