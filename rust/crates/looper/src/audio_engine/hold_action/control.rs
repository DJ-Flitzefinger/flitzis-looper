//! Request-owned internal Hold admission. Public waveform routing is integrated separately.
//!
//! The producer mutex reserves queue capacity before any admission store. Pause advances
//! exactly one captured transport intent; release consumes the original owning ticket and
//! grants no new source, slot, transport or scheduler authority.

use super::{HoldActionRtWork, HoldActionState, HoldActionTicket, NativeHoldAction};
use crate::audio_engine::AudioEngine;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::content_placement::{PlacementError, PlacementObservation};
use crate::messages::ControlMessage;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use std::sync::{Arc, Mutex};

pub(crate) fn pause_for_producer(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    id: usize,
    physical_press_id: u64,
    received_at_ns: u64,
) -> PyResult<HoldActionTicket> {
    if id >= NUM_SAMPLES || physical_press_id == 0 {
        return Err(PyValueError::new_err(
            "hold requires a valid slot and nonzero physical press",
        ));
    }
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let original_request = requests[id];
    let ownership = &engine.input_runtime_ownership;
    ownership.ensure_no_copy_reservation(id)?;
    let captured = ownership
        .slot_snapshot(id)
        .ok_or_else(|| PyRuntimeError::new_err("hold content placement is pending"))?;
    if captured.content_key().is_none() {
        return Err(PyValueError::new_err("hold content is empty"));
    }
    let prepared = ownership
        .prepare_transport_intent(captured)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    let action = NativeHoldAction::prepare(
        ownership.clone(),
        captured,
        prepared.revision(),
        physical_press_id,
        received_at_ns,
    )
    .map_err(PyValueError::new_err)?;
    let mut work = Some(HoldActionRtWork::pause(action.clone()));
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("hold producer lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "Hold pause queue is full; no action admitted",
        ));
    }
    ownership
        .commit_transport_intent(
            prepared,
            || {
                requests[id] == original_request
                    && ownership.copy_reservation(id) == 0
                    && ownership.pair_reservation(id) == 0
                    && action.state() == HoldActionState::Prepared
            },
            || {
                assert!(
                    action.mark_admitted(),
                    "private prepared Hold action is unique"
                );
                producer
                    .push(ControlMessage::HoldAction(work.take().unwrap()))
                    .expect("reserved single-producer Hold pause capacity");
            },
        )
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    Ok(HoldActionTicket { action })
}

/// True means this original obligation is terminal or already admitted, including this call.
/// False retains its sticky request and owning ticket for a later bounded control-poll retry.
pub(crate) fn release_for_producer(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ticket: &HoldActionTicket,
) -> PyResult<bool> {
    let action = &ticket.action;
    let ownership = &engine.input_runtime_ownership;
    if !Arc::ptr_eq(action.ownership(), ownership) {
        return Err(PyValueError::new_err("foreign Native Hold ticket"));
    }
    if action.state().terminal() || action.release_admitted() {
        return Ok(true);
    }
    action.request_release();
    if action.state() == HoldActionState::Prepared {
        return Err(PyValueError::new_err("Hold pause was never admitted"));
    }
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let placement = match ownership.observe_content_key(action.key()) {
        PlacementObservation::Current(placement) => Some(placement),
        PlacementObservation::Retired => None,
        PlacementObservation::Pending | PlacementObservation::Stale => return Ok(false),
    };
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("hold producer lock poisoned"))?;
    if action.state().terminal() || action.release_admitted() {
        return Ok(true);
    }
    if producer.is_full() {
        return Ok(false);
    }
    // Allocate only after capacity is available, always before the same-content marker.
    let mut work = Some(HoldActionRtWork::release(action.clone()));
    let mut publish = || {
        // Retired lifetime settlement can race this control-only enqueue. If its callback
        // already ended the obligation, do not enqueue a duplicate terminal observer.
        if !action.mark_release_admitted() {
            return;
        }
        producer
            .push(ControlMessage::HoldAction(work.take().unwrap()))
            .expect("reserved single-producer Hold release capacity");
    };
    let Some(placement) = placement else {
        // Its nonreused lifetime is already removed. Queue the original owner solely for
        // native terminal ACK/cleanup, never claim the cell's replacement or grant authority.
        publish();
        return Ok(true);
    };
    let write = match ownership.prepare_slot_write_fixed(placement, placement.lifetime()) {
        Ok(write) => write,
        Err(PlacementError::Pending | PlacementError::Stale | PlacementError::Retired) => {
            return Ok(false);
        }
        Err(error) => return Err(PyRuntimeError::new_err(error.to_string())),
    };
    let result = ownership.commit_slot_write_fixed(
        &write,
        || {
            ownership.slot_snapshot_fields_current(placement)
                && placement.content_key() == Some(action.key())
                && !action.state().terminal()
                && !action.release_admitted()
        },
        |_| publish(),
    );
    match result {
        Ok(_) => Ok(true),
        Err(_) if action.state().terminal() || action.release_admitted() => Ok(true),
        Err(
            PlacementError::Pending
            | PlacementError::Stale
            | PlacementError::Retired
            | PlacementError::ClaimRejected,
        ) => Ok(false),
        Err(error) => Err(PyRuntimeError::new_err(error.to_string())),
    }
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
