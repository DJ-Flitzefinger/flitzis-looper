//! Coupled derived controls from actual acknowledged timing; no evidence enters RT.

use super::AudioEngine;
use super::constant_timing::CurrentTimingAcknowledgements;
use super::input_runtime_binding::{
    self, InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding,
};
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

pub(crate) struct AcceptedTimingRefresh {
    pub(crate) id: usize,
    pub(crate) binding: InputPadBinding,
    pub(crate) start_s: f64,
    pub(crate) end_s: Option<f64>,
    pub(crate) master_period_seconds: Option<f64>,
    pub(crate) master_provenance: super::master_provenance::MasterProvenance,
    pub(crate) global_revision: u64,
    pub(crate) global_owner: Arc<AtomicU64>,
    pub(crate) ownership: Arc<InputRuntimeOwnership>,
    pub(crate) hierarchy_stamp: u64,
    pub(crate) launch_revision: u64,
    status: Arc<AtomicU8>,
}

impl std::fmt::Debug for AcceptedTimingRefresh {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcceptedTimingRefresh")
            .field("id", &self.id)
            .field("binding", &self.binding)
            .field("start_s", &self.start_s)
            .field("end_s", &self.end_s)
            .field("master_period_seconds", &self.master_period_seconds)
            .field("global_revision", &self.global_revision)
            .field("hierarchy_stamp", &self.hierarchy_stamp)
            .field("launch_revision", &self.launch_revision)
            .finish_non_exhaustive()
    }
}

impl PartialEq for AcceptedTimingRefresh {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl AcceptedTimingRefresh {
    pub(crate) fn global_current(&self) -> bool {
        self.ownership.hierarchy_stamp.load(Ordering::Acquire) == self.hierarchy_stamp
            && launch_current(&self.ownership, self.binding, self.launch_revision)
            && (self.master_period_seconds.is_none()
                || self.global_owner.load(Ordering::Acquire) == self.global_revision)
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
    hierarchy_stamp: u64,
    launch_revision: u64,
}

#[pymethods]
impl AcceptedTimingRefreshTicket {
    /// Observe the retained original lifetime's placement gap without granting
    /// fresh authority or treating a retired original content as Pending.
    pub fn placement_pending(&self) -> bool {
        self.binding
            .placement
            .and_then(|snapshot| snapshot.content_key())
            .is_some_and(|key| {
                self.ownership.content_key_current(key)
                    && matches!(
                        self.ownership.observe_control_key(key),
                        super::content_placement::PlacementObservation::Pending
                    )
                    && self.ownership.content_key_current(key)
            })
    }

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
            && self.ownership.hierarchy_stamp.load(Ordering::Acquire) == self.hierarchy_stamp
            && launch_current(&self.ownership, self.binding, self.launch_revision)
            && self.ownership.authority_current(self.id, self.binding)
            && self.ownership.binding_source_current(self.id, self.binding)
            && self
                .acknowledgements
                .current_for_binding(&self.ownership, self.id, self.binding)
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
    // Retain the supplied capture's cancellation witness before geometry/authority rebasing.
    let launch_revision = binding.source_map_launch_snapshot().launch_revision();
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
        || !binding.binding.same_projection(&current.binding)
        || !current.current()
        || !current.available()
        || !launch_current(
            &engine.input_runtime_ownership,
            binding.binding,
            launch_revision,
        )
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
    let next = engine.input_runtime_ownership.next_authority(binding.id)?;
    if !binding.current() {
        return Err(PyRuntimeError::new_err(
            "accepted timing refresh binding changed during admission",
        ));
    }
    let _lease =
        super::GlobalTimingLease::acquire(&engine.input_runtime_ownership.global_timing_publishing)
            .ok_or_else(|| PyRuntimeError::new_err("global timing publication busy"))?;
    if !binding.current()
        || !launch_current(
            &engine.input_runtime_ownership,
            binding.binding,
            launch_revision,
        )
    {
        return Err(PyRuntimeError::new_err(
            "accepted timing refresh binding changed during admission",
        ));
    }
    let global_revision = engine.global_timing_revision.load(Ordering::Acquire);
    let hierarchy_stamp = engine
        .input_runtime_ownership
        .hierarchy_stamp
        .load(Ordering::Acquire);
    let mut revised = current.binding;
    revised.authority_revision = next;
    let status = Arc::new(AtomicU8::new(PENDING));
    let refresh = Arc::new(AcceptedTimingRefresh {
        id: binding.id,
        binding: revised,
        start_s,
        end_s,
        master_period_seconds,
        master_provenance: super::master_provenance::MasterProvenance::qualified(
            super::master_provenance::source_identity(&current).ok_or_else(|| {
                PyRuntimeError::new_err("accepted refresh requires original source identity")
            })?,
            revised,
        ),
        global_revision,
        global_owner: engine.global_timing_revision.clone(),
        ownership: engine.input_runtime_ownership.clone(),
        hierarchy_stamp,
        launch_revision,
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
        hierarchy_stamp,
        launch_revision,
    })
}

fn launch_current(
    ownership: &InputRuntimeOwnership,
    binding: InputPadBinding,
    revision: u64,
) -> bool {
    revision != u64::MAX
        && binding
            .placement
            .and_then(|placement| placement.content_key())
            .is_some_and(|key| {
                ownership.content_key_current(key)
                    && ownership
                        .launch_revision_for_cell(key.cell())
                        .load(Ordering::Acquire)
                        == revision
            })
}
