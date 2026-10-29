//! A fresh transport gesture publishes its own content and intent before queue admission.
use super::AudioEngine;
use crate::messages::{ControlMessage, TransportContentEffect};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use rtrb::Producer;
#[cfg(test)]
#[path = "transport_intent_control_tests.rs"]
mod tests;

pub(super) fn publish_under_request_lock(
    engine: &AudioEngine,
    producer: &mut Producer<ControlMessage>,
    id: usize,
    effect: TransportContentEffect,
) -> PyResult<super::ordinary_transport::OrdinaryTransportAdmission> {
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(id)?;
    if producer.slots() == 0 {
        return Err(PyRuntimeError::new_err("transport queue is full"));
    }
    let placement = engine
        .input_runtime_ownership
        .slot_snapshot(id)
        .ok_or_else(|| PyRuntimeError::new_err("content placement is pending"))?;
    let prepared = engine
        .input_runtime_ownership
        .prepare_transport_intent(placement)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    if !engine
        .input_runtime_ownership
        .ordinary_available(placement.cell())
    {
        return Err(PyRuntimeError::new_err(
            "ordinary transport observation unavailable",
        ));
    }
    let mut enqueued = false;
    let mut admission = None;
    let source_epoch = engine
        .input_runtime_ownership
        .ordinary_source_epoch(placement.cell())
        .unwrap_or(0);
    let original_sequence = super::scheduler::reserve_input_order()
        .ok_or_else(|| PyRuntimeError::new_err("transport input sequence exhausted"))?;
    engine
        .input_runtime_ownership
        .commit_ordinary_transport_intent(
            prepared,
            || {
                engine.input_runtime_ownership.copy_reservation(id) == 0
                    && engine.input_runtime_ownership.pair_reservation(id) == 0
                    && engine
                        .input_runtime_ownership
                        .ordinary_source_epoch(placement.cell())
                        .unwrap_or(0)
                        == source_epoch
            },
            || {
                enqueued = producer
                    .push(ControlMessage::TransportForPlacement {
                        placement,
                        transport_revision: prepared.revision(),
                        source_epoch,
                        original_sequence,
                        effect,
                    })
                    .is_ok();
                if enqueued {
                    admission = Some(super::ordinary_transport::OrdinaryTransportAdmission::new(
                        engine.input_runtime_ownership.clone(),
                        placement.content_key(),
                        source_epoch,
                        prepared.revision(),
                        effect,
                    ));
                }
                enqueued
            },
        )
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    if !enqueued {
        return Err(PyRuntimeError::new_err("reserved transport capacity lost"));
    }
    admission.ok_or_else(|| PyRuntimeError::new_err("reserved transport admission lost"))
}
