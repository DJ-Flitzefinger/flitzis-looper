//! Internal request-owned Pair capture and queue admission, without public settlement shortcuts.

use super::{PadPairRtWork, PairParticipantGuard, PairRtGuard};
use crate::audio_engine::AudioEngine;
use crate::audio_engine::cold_store::CommittedColdLease;
use crate::audio_engine::committed_stem_residency;
use crate::audio_engine::constant_timing::CurrentConstantTimingRecord;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::content_placement::{PlacementError, SlotSnapshot};
use crate::audio_engine::input_mapping::CapturedPairRuntime;
use crate::audio_engine::input_runtime_binding::{self, InputRuntimePadBinding};
use crate::audio_engine::resident_relocation::{ResidentStemCache, StemOwner};
use crate::messages::{ControlMessage, SampleBuffer};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(test)]
#[path = "source_binding_tests.rs"]
mod source_binding_tests;
#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;

/// Existing, verified control custody is retained without importing, decoding or rebinding it.
struct PairControlCustody {
    _sample: Option<SampleBuffer>,
    _lease: Option<CommittedColdLease>,
    _timing: Option<CurrentConstantTimingRecord>,
    _stems: Option<StemOwner>,
    digest: Option<String>,
}

struct PadPairState {
    guard: PairRtGuard,
    _custody: [PairControlCustody; 2],
    captured_requests: [u64; 2],
    stage: AtomicU8,
    work: Mutex<Option<Box<PadPairRtWork>>>,
    runtime: Option<CapturedPairRuntime>,
    mirrored: AtomicBool,
    finished: AtomicBool,
}

impl PadPairState {
    fn belongs_to(&self, engine: &AudioEngine) -> PyResult<()> {
        if !Arc::ptr_eq(&self.guard.ownership, &engine.input_runtime_ownership)
            || !Arc::ptr_eq(
                &self.guard.acknowledgements,
                &engine.current_timing_acknowledgements,
            )
        {
            return Err(PyValueError::new_err("foreign Native pair transaction"));
        }
        Ok(())
    }

    fn snapshots(&self) -> [SlotSnapshot; 2] {
        self.guard
            .participants
            .each_ref()
            .map(|participant| participant.placement())
    }
}

/// Internal owner: callers retain this through rejection or complete outer durable settlement.
#[pyclass(frozen)]
pub struct CapturedPadPair {
    state: Arc<PadPairState>,
}

/// Own ACK is available independently of control/durable reconciliation, which remains separate.
#[pyclass(frozen)]
pub struct PadPairTicket {
    state: Arc<PadPairState>,
}

/// Process-local original transaction ownership; diagnostic data cannot reconstruct it.
#[pyclass(frozen)]
pub struct PadPairAcknowledgement {
    state: Arc<PadPairState>,
    observed: super::PairAcknowledgementSnapshot,
}

#[pymethods]
impl PadPairTicket {
    pub fn publication_status(&self) -> &'static str {
        match self.state.guard.phase() {
            0 => {
                if self.state.stage.load(Ordering::Acquire) == 3 {
                    "pending"
                } else {
                    "captured"
                }
            }
            1 => "adopting",
            2 => "accepted",
            _ => "rejected",
        }
    }

    pub fn own_acknowledgement(&self) -> Option<PadPairAcknowledgement> {
        self.native_acknowledgement()
            .map(|observed| PadPairAcknowledgement {
                state: self.state.clone(),
                observed,
            })
    }

    pub fn metadata_json(&self) -> String {
        self.snapshot_envelope_json()
    }
}

impl PadPairTicket {
    pub(crate) fn native_acknowledgement(&self) -> Option<super::PairAcknowledgementSnapshot> {
        self.state.guard.native_acknowledgement()
    }

    /// Diagnostic JSON cannot recreate Native authority or certify durable settlement.
    pub(crate) fn snapshot_envelope_json(&self) -> String {
        let [source, target] = self.state.snapshots();
        json!({
            "source_slot": source.slot(), "target_slot": target.slot(),
            "source_lifetime": source.lifetime(), "target_lifetime": target.lifetime(),
            "captured_requests": self.state.captured_requests,
            "native_phase": self.state.guard.phase(),
            "own_native_ack": self.native_acknowledgement().is_some(),
            "control_mirrored": self.state.mirrored.load(Ordering::Acquire),
            "durable_finished": self.state.finished.load(Ordering::Acquire),
        })
        .to_string()
    }
}

/// Capture both actual Native occupants and pin their existing control custody before reservation.
pub(crate) fn capture(
    engine: &AudioEngine,
    source: usize,
    target: usize,
) -> PyResult<CapturedPadPair> {
    capture_with_source_binding(engine, source, target, None)
}

/// Join an original gesture source to the current request-owned Pair before reserving either slot.
pub(crate) fn capture_with_source_binding(
    engine: &AudioEngine,
    source: usize,
    target: usize,
    source_binding: Option<&InputRuntimePadBinding>,
) -> PyResult<CapturedPadPair> {
    if source >= NUM_SAMPLES || target >= NUM_SAMPLES || source == target {
        return Err(PyValueError::new_err(
            "pair requires two distinct valid slots",
        ));
    }
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let ownership = &engine.input_runtime_ownership;
    if source_binding.is_some_and(|expected| {
        expected.id != source || !Arc::ptr_eq(&expected.ownership, ownership) || !expected.current()
    }) {
        return Err(PyValueError::new_err(
            "stale or foreign pair source binding",
        ));
    }
    let snapshots = [source, target].map(|slot| ownership.slot_snapshot(slot));
    let [Some(source_snapshot), Some(target_snapshot)] = snapshots else {
        return Err(PyRuntimeError::new_err("pair placement is pending"));
    };
    if source_snapshot.lifetime() == 0 {
        return Err(PyValueError::new_err("pair source is empty"));
    }
    let snapshots = [source_snapshot, target_snapshot];
    // Public source bindings deliberately omit the MIDI runtime revision. Capture the actual
    // current value now, under the request owner shared by Native control writers.
    let bindings = [
        input_runtime_binding::capture_under_request_lock(engine, source)?,
        input_runtime_binding::capture_under_request_lock(engine, target)?,
    ];
    if source_binding.is_some_and(|expected| {
        bindings[0].as_ref().is_none_or(|current| {
            !expected.current()
                || !expected.binding.same_projection(&current.binding)
                || expected.source_generation != current.source_generation
        })
    }) {
        return Err(PyValueError::new_err(
            "stale or foreign pair source binding",
        ));
    }
    let runtime = engine
        .input_runtime
        .as_ref()
        .map(|runtime| runtime.capture_pair_rows(snapshots))
        .transpose()
        .map_err(PyRuntimeError::new_err)?;
    let loading = engine
        .loading_sample_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?;
    if [source, target].iter().any(|slot| {
        loading.contains(slot) || engine.cold_loading[*slot].load(Ordering::Acquire) != 0
    }) {
        return Err(PyRuntimeError::new_err(
            "pair participant has pending source adoption",
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
    let digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
    let mut participants = Vec::with_capacity(2);
    let mut custody = Vec::with_capacity(2);
    for (index, slot) in [source, target].into_iter().enumerate() {
        let current = bindings[index].as_ref();
        if snapshots[index].lifetime() != 0
            && current.is_none_or(|binding| !binding.available() || !binding.current())
        {
            return Err(PyValueError::new_err(
                "pair participant lacks current source/timing",
            ));
        }
        if cache[slot].is_some() != (snapshots[index].lifetime() != 0) {
            return Err(PyRuntimeError::new_err(
                "pair Native/control occupancy is settling",
            ));
        }
        let binding = current.map(|current| {
            let mut binding = current.binding;
            binding.runtime_revision = ownership.runtime_revision(slot);
            binding
        });
        let mut selection_fences =
            committed_stem_residency::capture_pair_selection_fences(&resident[slot]);
        let proposal_fences = engine
            .resident_proposals
            .capture_pair_fences_for_cell(snapshots[index].cell())
            .ok_or_else(|| PyRuntimeError::new_err("pair proposal guard owners unavailable"))?;
        selection_fences.extend(proposal_fences);
        let participant = PairParticipantGuard::capture(
            ownership,
            &engine.current_timing_acknowledgements,
            snapshots[index],
            binding,
            engine
                .prepared_source_epoch(slot)
                .map_err(PyRuntimeError::new_err)?
                .clone(),
            selection_fences,
        )
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        custody.push(PairControlCustody {
            _sample: cache[slot].clone(),
            _lease: leases[slot].clone(),
            _timing: current.and_then(|binding| binding.copy_timing_context().1),
            _stems: resident[slot].accepted.clone(),
            digest: digests[slot].clone(),
        });
        participants.push(participant);
    }
    let participants: [PairParticipantGuard; 2] = participants
        .try_into()
        .map_err(|_| PyRuntimeError::new_err("pair capture participant count changed"))?;
    let custody: [PairControlCustody; 2] = custody
        .try_into()
        .map_err(|_| PyRuntimeError::new_err("pair capture custody count changed"))?;
    let reservation =
        crate::audio_engine::content_lifetime::reserve().map_err(PyRuntimeError::new_err)?;
    let guard = PairRtGuard::new(
        participants,
        ownership.clone(),
        engine.current_timing_acknowledgements.clone(),
        reservation,
    )
    .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    let state = Arc::new(PadPairState {
        guard,
        _custody: custody,
        captured_requests: [requests[source], requests[target]],
        stage: AtomicU8::new(0),
        work: Mutex::new(None),
        runtime,
        mirrored: AtomicBool::new(false),
        finished: AtomicBool::new(false),
    });
    ownership
        .reserve_pair(&snapshots, reservation)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    Ok(CapturedPadPair { state })
}

#[path = "mirror.rs"]
mod mirror;
pub(crate) use mirror::reconcile;

/// Only fixed canonical defaults need preparation; existing living Native images are never copied.
pub(crate) fn prepare(
    engine: &AudioEngine,
    captured: &CapturedPadPair,
    sample_rate_hz: u32,
    channels: usize,
) -> PyResult<PadPairTicket> {
    let state = &captured.state;
    state.belongs_to(engine)?;
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut pending = state
        .work
        .lock()
        .map_err(|_| PyRuntimeError::new_err("pair preparation lock poisoned"))?;
    if state.stage.load(Ordering::Acquire) != 0 || state.guard.phase() != 0 {
        return Err(PyValueError::new_err("pair preparation already started"));
    }
    // reserve_pair is a publication itself. Preparing after it retains its actual current
    // layout revision while validating both original occupant/binding fields.
    let work = PadPairRtWork::new(state.guard.clone(), sample_rate_hz, channels)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    *pending = Some(work);
    state.stage.store(2, Ordering::Release);
    Ok(PadPairTicket {
        state: state.clone(),
    })
}

/// Queue admission is not a Native effect ACK. Queue failure changes no live participant field.
pub(crate) fn publish(engine: &AudioEngine, ticket: &PadPairTicket) -> PyResult<()> {
    let state = &ticket.state;
    state.belongs_to(engine)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    if state.stage.load(Ordering::Acquire) != 2
        || state.guard.phase() != 0
        || state
            .snapshots()
            .iter()
            .enumerate()
            .any(|(index, snapshot)| requests[snapshot.slot()] != state.captured_requests[index])
        || !state.guard.stable_current()
    {
        return Err(PyValueError::new_err(
            "pair preparation is stale or already published",
        ));
    }
    let mut producer = engine
        .control_producer()?
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err("pair publication queue is full"));
    }
    let mut pending = state
        .work
        .lock()
        .map_err(|_| PyRuntimeError::new_err("pair preparation lock poisoned"))?;
    let work = pending
        .take()
        .ok_or_else(|| PyValueError::new_err("pair payload already published"))?;
    producer
        .push(ControlMessage::PadPair(work))
        .map_err(|_| PyRuntimeError::new_err("reserved single-producer pair capacity lost"))?;
    state.stage.store(3, Ordering::Release);
    Ok(())
}

/// Only a terminal preclaim rejection releases reservations. Claim/ACK require outer settlement.
pub(crate) fn cancel(engine: &AudioEngine, captured: &CapturedPadPair) -> PyResult<bool> {
    let state = &captured.state;
    state.belongs_to(engine)?;
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    state.guard.reject();
    if state.guard.phase() != 3 {
        return Ok(false);
    }
    state
        .guard
        .ownership
        .release_pair(&state.snapshots(), state.guard.reservation)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    Ok(true)
}

/// Validate the same process-local receipt against both actual acknowledged participants.
pub(crate) fn validate_acknowledgement(
    engine: &AudioEngine,
    ticket: &PadPairTicket,
    ack: &PadPairAcknowledgement,
) -> PyResult<bool> {
    ticket.state.belongs_to(engine)?;
    if !Arc::ptr_eq(&ticket.state, &ack.state)
        || ticket.native_acknowledgement() != Some(ack.observed)
        || !ticket.state.mirrored.load(Ordering::Acquire)
    {
        return Ok(false);
    }
    let state = &ticket.state;
    let originals = state.snapshots();
    Ok(originals
        .iter()
        .zip([ack.observed.source_after, ack.observed.target_after])
        .all(|(original, after)| {
            state
                .guard
                .ownership
                .slot_snapshot(original.slot())
                .is_some_and(|current| {
                    after.matches(current)
                        && state.guard.ownership.control_slot_for_cell(current.cell())
                            == current.slot()
                        && (state.finished.load(Ordering::Acquire)
                            || state.guard.ownership.pair_reservation(current.slot())
                                == state.guard.reservation)
                })
        }))
}

/// Outer settlement invokes this only after its exact mirror and durable image are complete.
pub(crate) fn finish(
    engine: &AudioEngine,
    ticket: &PadPairTicket,
    ack: &PadPairAcknowledgement,
) -> PyResult<()> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    ticket.state.belongs_to(engine)?;
    if ticket.state.finished.load(Ordering::Acquire) {
        if Arc::ptr_eq(&ticket.state, &ack.state)
            && ticket.native_acknowledgement() == Some(ack.observed)
        {
            return Ok(());
        }
        return Err(PyValueError::new_err(
            "foreign completed pair acknowledgement",
        ));
    }
    if !validate_acknowledgement(engine, ticket, ack)? {
        return Err(PyValueError::new_err(
            "pair finish requires its complete own Native acknowledgement",
        ));
    }
    let state = &ticket.state;
    state
        .guard
        .ownership
        .release_pair(&state.snapshots(), state.guard.reservation)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    state.finished.store(true, Ordering::Release);
    Ok(())
}
