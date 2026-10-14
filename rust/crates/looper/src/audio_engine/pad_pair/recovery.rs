//! Fresh acceptance of an already restored afterimage. Never performs another Move/Swap.

use super::PairParticipantGuard;
use crate::audio_engine::AudioEngine;
use crate::audio_engine::cold_store::CommittedColdLease;
use crate::audio_engine::committed_stem_residency;
use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::content_placement::{PlacementObservation, SlotSnapshot};
use crate::audio_engine::input_runtime_binding::{
    self, InputRuntimeOwnership, InputRuntimePadBinding,
};
use crate::audio_engine::resident_relocation::StemOwner;
use crate::messages::SampleBuffer;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(in crate::audio_engine) struct RecoveredCustody {
    binding: Option<InputRuntimePadBinding>,
    _sample: Option<SampleBuffer>,
    _lease: Option<CommittedColdLease>,
    _stems: Option<StemOwner>,
}

impl RecoveredCustody {
    pub(in crate::audio_engine) fn new(
        binding: Option<InputRuntimePadBinding>,
        sample: Option<SampleBuffer>,
        lease: Option<CommittedColdLease>,
        stems: Option<StemOwner>,
    ) -> Self {
        Self {
            binding,
            _sample: sample,
            _lease: lease,
            _stems: stems,
        }
    }

    /// Shared restored participant custody, placement and actual source/request acceptance.
    /// Callers additionally validate their exact complete participant and reservation guards.
    pub(in crate::audio_engine) fn current_under_request_lock(
        &self,
        ownership: &InputRuntimeOwnership,
        placement: SlotSnapshot,
        requests: &[u64],
        request: u64,
        request_incarnation: Option<u64>,
    ) -> bool {
        requests.get(placement.slot()) == Some(&request)
            && placement.content_key().is_none_or(|key| {
                matches!(ownership.observe_control_key(key),
                    PlacementObservation::Current(current) if current.same_projection(placement))
            })
            && ownership
                .slot_snapshot(placement.slot())
                .is_some_and(|current| current.same_projection(placement))
            && ownership.control_slot_for_cell(placement.cell()) == placement.slot()
            && match self.binding.as_ref() {
                Some(binding) => {
                    ownership.binding_source_adoption_accepted(placement.slot(), binding.binding)
                        && placement
                            .content_key()
                            .and_then(|key| ownership.key_source_request_incarnation(key))
                            == request_incarnation
                }
                None => placement.lifetime() == 0 && request_incarnation.is_none(),
            }
    }
}

/// Both restored original participants are retained under one existing Pair reservation.
/// It is process-local current acceptance; journal JSON cannot recreate this owner.
#[pyclass(frozen)]
pub struct RecoveredPairAcceptance {
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    participants: [PairParticipantGuard; 2],
    custody: [RecoveredCustody; 2],
    requests: [u64; 2],
    request_incarnations: [Option<u64>; 2],
    reservation: u64,
    released: AtomicBool,
}

impl RecoveredPairAcceptance {
    fn snapshots(&self) -> [SlotSnapshot; 2] {
        self.participants
            .each_ref()
            .map(PairParticipantGuard::placement)
    }

    fn belongs_to(&self, engine: &AudioEngine) -> PyResult<()> {
        if !Arc::ptr_eq(&self.ownership, &engine.input_runtime_ownership)
            || !Arc::ptr_eq(
                &self.acknowledgements,
                &engine.current_timing_acknowledgements,
            )
        {
            return Err(PyValueError::new_err("foreign recovered Pair acceptance"));
        }
        Ok(())
    }

    fn current_under_request_lock(&self, requests: &[u64]) -> bool {
        !self.released.load(Ordering::Acquire)
            && self
                .participants
                .iter()
                .enumerate()
                .all(|(index, participant)| {
                    let placement = participant.placement();
                    self.custody[index].current_under_request_lock(
                        &self.ownership,
                        placement,
                        requests,
                        self.requests[index],
                        self.request_incarnations[index],
                    ) && participant.current(
                        &self.ownership,
                        &self.acknowledgements,
                        self.reservation,
                    )
                })
    }
}

#[pymethods]
impl RecoveredPairAcceptance {
    /// Diagnostic original current capture; serialized fields never grant acceptance.
    pub fn metadata_json(&self) -> String {
        let [source, target] = self.snapshots();
        json!({
            "source_slot": source.slot(), "target_slot": target.slot(),
            "source_occupied": source.lifetime() != 0,
            "target_occupied": target.lifetime() != 0,
            "source_generation": self.participants[0].control.source_generation(),
            "target_generation": self.participants[1].control.source_generation(),
            "released": self.released.load(Ordering::Acquire),
        })
        .to_string()
    }
}

/// Capture fresh restored bindings and actual callback source/timing ACKs. A Move
/// afterimage has an empty source and an occupied target; Swap has both occupied.
pub(crate) fn capture_recovered(
    engine: &AudioEngine,
    source: usize,
    target: usize,
) -> PyResult<RecoveredPairAcceptance> {
    if source >= NUM_SAMPLES || target >= NUM_SAMPLES || source == target {
        return Err(PyValueError::new_err(
            "recovered Pair requires two distinct valid slots",
        ));
    }
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let ownership = &engine.input_runtime_ownership;
    let [Some(source_snapshot), Some(target_snapshot)] =
        [source, target].map(|slot| ownership.slot_snapshot(slot))
    else {
        return Err(PyRuntimeError::new_err(
            "recovered Pair placement is pending",
        ));
    };
    if target_snapshot.lifetime() == 0 {
        return Err(PyValueError::new_err("recovered Pair target is empty"));
    }
    let snapshots = [source_snapshot, target_snapshot];
    let bindings = [
        input_runtime_binding::capture_under_request_lock(engine, source)?,
        input_runtime_binding::capture_under_request_lock(engine, target)?,
    ];
    let loading = engine
        .loading_sample_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?;
    if [source, target].iter().any(|slot| {
        loading.contains(slot) || engine.cold_loading[*slot].load(Ordering::Acquire) != 0
    }) {
        return Err(PyRuntimeError::new_err(
            "recovered Pair participant source adoption is pending",
        ));
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let leases = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cold custody lock poisoned"))?;
    let resident = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident custody lock poisoned"))?;
    let mut participants = Vec::with_capacity(2);
    let mut custody = Vec::with_capacity(2);
    let mut incarnations = [None; 2];
    for (index, slot) in [source, target].into_iter().enumerate() {
        let occupied = snapshots[index].lifetime() != 0;
        let current = bindings[index].as_ref();
        if cache[slot].is_some() != occupied || current.is_some() != occupied {
            return Err(PyRuntimeError::new_err(
                "recovered Pair Native/control occupancy is settling",
            ));
        }
        if occupied
            && current.is_none_or(|binding| {
                !binding.available()
                    || !binding.current()
                    || !ownership.binding_source_adoption_accepted(slot, binding.binding)
            })
        {
            return Err(PyValueError::new_err(
                "recovered Pair lacks actual current Native source/timing acceptance",
            ));
        }
        let binding = current.map(|current| {
            let mut binding = current.binding;
            binding.runtime_revision = ownership.runtime_revision(slot);
            binding
        });
        incarnations[index] =
            binding.and_then(|binding| ownership.binding_source_request_incarnation(slot, binding));
        if occupied && incarnations[index].is_none() {
            return Err(PyValueError::new_err(
                "recovered Pair source request incarnation unavailable",
            ));
        }
        let mut fences = committed_stem_residency::capture_pair_selection_fences(&resident[slot]);
        fences.extend(
            engine
                .resident_proposals
                .capture_pair_fences_for_cell(snapshots[index].cell())
                .ok_or_else(|| {
                    PyRuntimeError::new_err("recovered Pair proposal guards unavailable")
                })?,
        );
        participants.push(
            PairParticipantGuard::capture(
                ownership,
                &engine.current_timing_acknowledgements,
                snapshots[index],
                binding,
                engine
                    .prepared_source_epoch(slot)
                    .map_err(PyRuntimeError::new_err)?
                    .clone(),
                fences,
            )
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?,
        );
        custody.push(RecoveredCustody::new(
            current.cloned(),
            cache[slot].clone(),
            leases[slot].clone(),
            resident[slot].accepted.clone(),
        ));
    }
    let participants = participants
        .try_into()
        .map_err(|_| PyRuntimeError::new_err("recovered Pair participant count changed"))?;
    let custody = custody
        .try_into()
        .map_err(|_| PyRuntimeError::new_err("recovered Pair custody count changed"))?;
    let reservation =
        crate::audio_engine::content_lifetime::reserve().map_err(PyRuntimeError::new_err)?;
    let captured = RecoveredPairAcceptance {
        ownership: ownership.clone(),
        acknowledgements: engine.current_timing_acknowledgements.clone(),
        participants,
        custody,
        requests: [requests[source], requests[target]],
        request_incarnations: incarnations,
        reservation,
        released: AtomicBool::new(false),
    };
    ownership
        .reserve_pair(&snapshots, reservation)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    // Return the same owner even if an earlier admitted callback changes a field
    // immediately afterward. Validation rejects it while release retains exact custody.
    Ok(captured)
}

pub(crate) fn validate_recovered(
    engine: &AudioEngine,
    captured: &RecoveredPairAcceptance,
) -> PyResult<bool> {
    captured.belongs_to(engine)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    Ok(captured.current_under_request_lock(&requests))
}

/// Final durable settlement or abandoned recovery releases only this actual reservation.
/// Pending/errors retain the original owner for retry; already-released is idempotent.
pub(crate) fn release_recovered(
    engine: &AudioEngine,
    captured: &RecoveredPairAcceptance,
) -> PyResult<bool> {
    captured.belongs_to(engine)?;
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    if captured.released.load(Ordering::Acquire) {
        return Ok(true);
    }
    captured
        .ownership
        .release_pair(&captured.snapshots(), captured.reservation)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    captured.released.store(true, Ordering::Release);
    Ok(true)
}

#[cfg(test)]
mod tests;
