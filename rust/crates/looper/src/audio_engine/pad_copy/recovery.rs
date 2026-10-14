//! Fresh acceptance of fixed restored C. Never recopies A or changes placement.

use crate::audio_engine::AudioEngine;
use crate::audio_engine::committed_stem_residency;
use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_runtime_binding::{
    self, InputRuntimeOwnership, InputRuntimePadBinding,
};
use crate::audio_engine::pad_pair::PairParticipantGuard;
use crate::audio_engine::pad_pair::recovery::RecoveredCustody;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Only the current restored successor is reserved. Surviving A is independent.
/// This process-local owner cannot be constructed from persisted journal fields.
#[pyclass(frozen)]
pub struct RecoveredCopyAcceptance {
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    participant: PairParticipantGuard,
    custody: RecoveredCustody,
    request: u64,
    request_incarnation: u64,
    reservation: u64,
    released: AtomicBool,
}

impl RecoveredCopyAcceptance {
    fn belongs_to(&self, engine: &AudioEngine) -> PyResult<()> {
        if !Arc::ptr_eq(&self.ownership, &engine.input_runtime_ownership)
            || !Arc::ptr_eq(
                &self.acknowledgements,
                &engine.current_timing_acknowledgements,
            )
        {
            return Err(PyValueError::new_err("foreign recovered Copy acceptance"));
        }
        Ok(())
    }

    fn current_under_request_lock(&self, requests: &[u64]) -> bool {
        !self.released.load(Ordering::Acquire)
            && self.custody.current_under_request_lock(
                &self.ownership,
                self.participant.placement(),
                requests,
                self.request,
                Some(self.request_incarnation),
            )
            && self.participant.current_for_copy(
                &self.ownership,
                &self.acknowledgements,
                self.reservation,
            )
    }
}

#[pymethods]
impl RecoveredCopyAcceptance {
    /// Diagnostic capture only; serialized fields never grant current permission.
    pub fn metadata_json(&self) -> String {
        json!({
            "target_slot": self.participant.placement().slot(),
            "target_generation": self.participant.control.source_generation(),
            "released": self.released.load(Ordering::Acquire),
        })
        .to_string()
    }
}

/// Capture the caller's exact restored C binding and its genuine callback ACKs.
/// There is no source A participant, new Copy work or restored runtime token.
pub(crate) fn capture_recovered(
    engine: &AudioEngine,
    expected: &InputRuntimePadBinding,
) -> PyResult<RecoveredCopyAcceptance> {
    let target = expected.id;
    if target >= NUM_SAMPLES {
        return Err(PyValueError::new_err("recovered Copy target is invalid"));
    }
    if !Arc::ptr_eq(&expected.ownership, &engine.input_runtime_ownership)
        || !Arc::ptr_eq(
            &expected.acknowledgements,
            &engine.current_timing_acknowledgements,
        )
    {
        return Err(PyValueError::new_err(
            "foreign recovered Copy target binding",
        ));
    }
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let ownership = &engine.input_runtime_ownership;
    let current = input_runtime_binding::capture_under_request_lock(engine, target)?
        .ok_or_else(|| PyValueError::new_err("recovered Copy target is unavailable"))?;
    if !expected.current()
        || !expected.available()
        || !current.current()
        || !current.available()
        || !expected.binding.same_projection(&current.binding)
        || expected.source_generation != current.source_generation
        || !ownership.binding_source_adoption_accepted(target, current.binding)
    {
        return Err(PyValueError::new_err(
            "recovered Copy lacks exact current Native source/timing acceptance",
        ));
    }
    let placement = ownership
        .slot_snapshot(target)
        .filter(|snapshot| snapshot.content_key().is_some())
        .ok_or_else(|| PyRuntimeError::new_err("recovered Copy placement is pending or empty"))?;
    let loading = engine
        .loading_sample_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?;
    if loading.contains(&target) || engine.cold_loading[target].load(Ordering::Acquire) != 0 {
        return Err(PyRuntimeError::new_err(
            "recovered Copy target source adoption is pending",
        ));
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    if cache[target].is_none() {
        return Err(PyRuntimeError::new_err(
            "recovered Copy Native/control occupancy is settling",
        ));
    }
    let leases = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cold custody lock poisoned"))?;
    let resident = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident custody lock poisoned"))?;
    let mut binding = current.binding;
    binding.runtime_revision = ownership.runtime_revision(target);
    let request_incarnation = ownership
        .binding_source_request_incarnation(target, binding)
        .ok_or_else(|| {
            PyValueError::new_err("recovered Copy source request incarnation unavailable")
        })?;
    let mut fences = committed_stem_residency::capture_pair_selection_fences(&resident[target]);
    fences.extend(
        engine
            .resident_proposals
            .capture_pair_fences_for_cell(placement.cell())
            .ok_or_else(|| PyRuntimeError::new_err("recovered Copy proposal guards unavailable"))?,
    );
    let participant = PairParticipantGuard::capture(
        ownership,
        &engine.current_timing_acknowledgements,
        placement,
        Some(binding),
        engine
            .prepared_source_epoch(target)
            .map_err(PyRuntimeError::new_err)?
            .clone(),
        fences,
    )
    .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    let reservation =
        crate::audio_engine::content_lifetime::reserve().map_err(PyRuntimeError::new_err)?;
    let captured = RecoveredCopyAcceptance {
        ownership: ownership.clone(),
        acknowledgements: engine.current_timing_acknowledgements.clone(),
        participant,
        custody: RecoveredCustody::new(
            Some(current),
            cache[target].clone(),
            leases[target].clone(),
            resident[target].accepted.clone(),
        ),
        request: requests[target],
        request_incarnation,
        reservation,
        released: AtomicBool::new(false),
    };
    ownership.reserve_copy_target(placement, reservation)?;
    // An earlier admitted callback may still advance guards. Keep this same owner;
    // validation rejects stale acceptance and release retains exact target custody.
    Ok(captured)
}

pub(crate) fn validate_recovered(
    engine: &AudioEngine,
    captured: &RecoveredCopyAcceptance,
) -> PyResult<bool> {
    captured.belongs_to(engine)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    Ok(captured.current_under_request_lock(&requests))
}

/// Release only the actual target reservation at terminal or abandoned recovery.
/// Foreign owners and failed release retain custody; repeated release is idempotent.
pub(crate) fn release_recovered(
    engine: &AudioEngine,
    captured: &RecoveredCopyAcceptance,
) -> PyResult<bool> {
    captured.belongs_to(engine)?;
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    if captured.released.load(Ordering::Acquire) {
        return Ok(true);
    }
    captured.ownership.release_copy(
        captured.participant.placement().slot(),
        captured.reservation,
    )?;
    captured.released.store(true, Ordering::Release);
    Ok(true)
}

#[cfg(test)]
mod tests;
