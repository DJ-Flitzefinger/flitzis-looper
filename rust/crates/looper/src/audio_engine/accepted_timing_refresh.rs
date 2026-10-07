//! Coupled derived controls from actual acknowledged timing; no evidence enters RT.

use super::AudioEngine;
use super::constant_timing::CurrentTimingAcknowledgements;
use super::input_runtime_binding::{
    self, InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding,
};
use super::prepared_source::next_epoch;
use crate::messages::ControlMessage;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, AtomicU64, Ordering},
};

const PENDING: u8 = 0;
const ACCEPTED: u8 = 1;
const REJECTED: u8 = 2;

#[derive(Debug)]
pub(crate) struct AcceptedTimingRefresh {
    pub(crate) id: usize,
    pub(crate) binding: InputPadBinding,
    pub(crate) start_s: f64,
    pub(crate) end_s: Option<f64>,
    pub(crate) master_period_seconds: Option<f64>,
    pub(crate) global_revision: u64,
    pub(crate) global_owner: Arc<AtomicU64>,
    status: Arc<AtomicU8>,
}

impl PartialEq for AcceptedTimingRefresh {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl AcceptedTimingRefresh {
    pub(crate) fn global_current(&self) -> bool {
        self.master_period_seconds.is_none()
            || self.global_owner.load(Ordering::Acquire) == self.global_revision
    }

    pub(crate) fn finish(&self, accepted: bool) {
        self.status.store(
            if accepted { ACCEPTED } else { REJECTED },
            Ordering::Release,
        );
    }
}

impl Drop for AcceptedTimingRefresh {
    fn drop(&mut self) {
        let _ =
            self.status
                .compare_exchange(PENDING, REJECTED, Ordering::AcqRel, Ordering::Acquire);
    }
}

#[pyclass(frozen)]
pub struct AcceptedTimingRefreshTicket {
    status: Arc<AtomicU8>,
    id: usize,
    binding: InputPadBinding,
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    global_owner: Arc<AtomicU64>,
    global_revision: Option<u64>,
}

#[pymethods]
impl AcceptedTimingRefreshTicket {
    pub fn publication_status(&self) -> &'static str {
        match self.status.load(Ordering::Acquire) {
            ACCEPTED => "accepted",
            REJECTED => "rejected",
            _ => "pending",
        }
    }

    /// Historical acceptance alone never authorizes a control/session refresh.
    pub fn is_current(&self) -> bool {
        self.publication_status() == "accepted"
            && self.ownership.authority_current(self.id, self.binding)
            && self.ownership.binding_source_current(self.id, self.binding)
            && self.acknowledgements.current_epoch(self.id)
                == self
                    .binding
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
            && self
                .global_revision
                .is_none_or(|revision| self.global_owner.load(Ordering::Acquire) == revision)
            && self.ownership.authority_current(self.id, self.binding)
    }
}

pub(super) fn enqueue(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    binding: &InputRuntimePadBinding,
    start_s: f64,
    end_s: Option<f64>,
    master_period_seconds: Option<f64>,
) -> PyResult<AcceptedTimingRefreshTicket> {
    if !start_s.is_finite()
        || start_s < 0.0
        || end_s.is_some_and(|end| !end.is_finite() || end < 0.0)
        || master_period_seconds.is_some_and(|period| {
            !period.is_finite()
                || period <= 0.0
                || !(period * f64::from(binding.binding.sample_rate_hz) * 4.0).is_finite()
        })
    {
        return Err(PyValueError::new_err(
            "accepted timing refresh values out of range",
        ));
    }
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let current = input_runtime_binding::capture_under_request_lock(engine, binding.id)?
        .ok_or_else(|| PyRuntimeError::new_err("accepted timing refresh source unavailable"))?;
    if !Arc::ptr_eq(&binding.ownership, &engine.input_runtime_ownership)
        || binding.binding.accepted.is_none()
        || !binding.current()
        || binding.binding != current.binding
        || !current.current()
        || !current.available()
    {
        return Err(PyRuntimeError::new_err(
            "stale or unavailable accepted timing refresh binding",
        ));
    }
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "Failed to send accepted timing refresh - buffer may be full",
        ));
    }
    let next = next_epoch(&engine.input_runtime_ownership.authority[binding.id])
        .map_err(PyRuntimeError::new_err)?;
    if !binding.current() {
        return Err(PyRuntimeError::new_err(
            "accepted timing refresh binding changed during admission",
        ));
    }
    let global_revision = engine.global_timing_revision.load(Ordering::Acquire);
    let mut revised = current.binding;
    revised.authority_revision = next;
    let status = Arc::new(AtomicU8::new(PENDING));
    let refresh = Arc::new(AcceptedTimingRefresh {
        id: binding.id,
        binding: revised,
        start_s,
        end_s,
        master_period_seconds,
        global_revision,
        global_owner: engine.global_timing_revision.clone(),
        status: status.clone(),
    });
    engine.input_runtime_ownership.revoke(binding.id, next);
    producer
        .push(ControlMessage::RefreshAcceptedTiming(refresh))
        .map_err(|_| PyRuntimeError::new_err("reserved single-producer capacity lost"))?;
    Ok(AcceptedTimingRefreshTicket {
        status,
        id: binding.id,
        binding: revised,
        ownership: engine.input_runtime_ownership.clone(),
        acknowledgements: engine.current_timing_acknowledgements.clone(),
        global_owner: engine.global_timing_revision.clone(),
        global_revision: master_period_seconds.map(|_| global_revision),
    })
}
