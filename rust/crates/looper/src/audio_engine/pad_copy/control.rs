//! Control-owned capture, prepared custody and complete Copy acknowledgement.
use super::intent::CopyIntent;
use super::{CopyRtGuard, PadCopyRtWork};
use crate::audio_engine::AudioEngine;
use crate::audio_engine::cold_jobs::ColdJobsHandle;
use crate::audio_engine::cold_store::CommittedColdLease;
use crate::audio_engine::committed_stem_residency::{
    self, CapturedCopySelection, CommittedStemSetTicket, PreparedCopySelection,
};
use crate::audio_engine::constant_timing::{self, CurrentConstantTimingRecord};
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_runtime_binding::{self, InputPadBinding, InputRuntimePadBinding};
use crate::audio_engine::prepared_source::{PreparedSourcePermit, next_epoch};
use crate::audio_engine::resident_proposal::ResidentReadContext;
use crate::messages::{ControlMessage, SampleBuffer, StemMixMode};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

struct CopyCapture {
    source: SampleBuffer,
    lease: CommittedColdLease,
    digest: String,
    rate: u32,
    intent: CopyIntent,
    timing: Option<CurrentConstantTimingRecord>,
    selection: Option<CapturedCopySelection>,
    selection_fences: Option<super::CopySelectionFences>,
    request: u64,
    base_request: u64,
    future_epoch: u64,
    future_authority: u64,
    lifetime: u64,
    resident: ResidentReadContext,
    lane: ColdJobsHandle,
}

struct PreparedCopyOwners {
    sample: SampleBuffer,
    lease: CommittedColdLease,
    timing: Option<CurrentConstantTimingRecord>,
    selection: Option<PreparedCopySelection>,
    selection_ticket: Option<CommittedStemSetTicket>,
    rt: Option<Box<PadCopyRtWork>>,
}

pub(in crate::audio_engine) struct PadCopyState {
    guard: CopyRtGuard,
    capture: CopyCapture,
    prepared: Mutex<Option<PreparedCopyOwners>>,
    stage: AtomicU8,
    mirrored: AtomicBool,
    error: Mutex<Option<String>>,
}

impl PadCopyState {
    fn status(&self) -> &'static str {
        match self.guard.phase.load(Ordering::Acquire) {
            1 => "adopting",
            2 => "accepted",
            3 => "rejected",
            _ => match self.stage.load(Ordering::Acquire) {
                1 => "preparing",
                3 => "pending",
                _ => "captured",
            },
        }
    }
    fn belongs_to(&self, engine: &AudioEngine) -> PyResult<()> {
        if !Arc::ptr_eq(&self.guard.ownership, &engine.input_runtime_ownership) {
            return Err(PyValueError::new_err("foreign pad copy transaction"));
        }
        Ok(())
    }
}

#[pyclass(frozen)]
pub struct CapturedPadCopy {
    pub(in crate::audio_engine) state: Arc<PadCopyState>,
}
#[pymethods]
impl CapturedPadCopy {
    pub fn publication_status(&self) -> &'static str {
        self.state.status()
    }
}

#[pyclass(frozen)]
pub struct PadCopyTicket {
    state: Arc<PadCopyState>,
}
#[pymethods]
impl PadCopyTicket {
    pub fn publication_status(&self) -> &'static str {
        self.state.status()
    }
    pub fn committed_stem_ticket(&self) -> PyResult<Option<CommittedStemSetTicket>> {
        if self.state.guard.phase.load(Ordering::Acquire) != 2
            || !self.state.mirrored.load(Ordering::Acquire)
        {
            return Ok(None);
        }
        let owners = self
            .state
            .prepared
            .lock()
            .map_err(|_| PyRuntimeError::new_err("copy preparation lock poisoned"))?;
        Ok(owners
            .as_ref()
            .and_then(|owners| owners.selection_ticket.as_ref())
            .filter(|ticket| ticket.publication_status() == "accepted" && ticket.is_current())
            .cloned())
    }

    pub fn metadata_json(&self) -> String {
        let components_available = self.state.prepared.lock().ok().is_some_and(|owners| {
            owners
                .as_ref()
                .and_then(|owners| owners.selection.as_ref())
                .and_then(|selection| selection.stem_owner.as_ref())
                .is_some_and(|owner| owner.set.publication.status() == "accepted")
        });
        json!({"target_id": self.state.guard.target, "source_generation": self.state.capture.request,
            "content_lifetime": self.state.capture.lifetime, "status": self.state.status(),
            "control_mirrored": self.state.mirrored.load(Ordering::Acquire), "components_available": components_available}).to_string()
    }
}

pub(in crate::audio_engine) fn capture(
    engine: &AudioEngine,
    binding: &InputRuntimePadBinding,
    target: usize,
    intent_json: &str,
) -> PyResult<CapturedPadCopy> {
    if target >= NUM_SAMPLES || binding.id >= NUM_SAMPLES || target == binding.id {
        return Err(PyValueError::new_err(
            "copy requires distinct valid source and target",
        ));
    }
    let intent = CopyIntent::parse(intent_json).map_err(PyValueError::new_err)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(target)?;
    let current = input_runtime_binding::capture_under_request_lock(engine, binding.id)?
        .filter(|current| current.available() && current.current())
        .ok_or_else(|| PyValueError::new_err("copy source is unavailable"))?;
    if !binding.current()
        || binding.binding != current.binding
        || binding.source_generation != current.source_generation
        || !Arc::ptr_eq(&binding.ownership, &engine.input_runtime_ownership)
    {
        return Err(PyValueError::new_err(
            "stale or foreign copy source binding",
        ));
    }
    let (timing_intent, timing) = current.copy_timing_context();
    if intent.timing_intent != timing_intent {
        return Err(PyValueError::new_err(
            "copy timing intent differs from its current source",
        ));
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    if cache[target].is_some()
        || !engine.input_runtime_ownership.copy_target_empty(target)
        || engine
            .loading_sample_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?
            .contains(&target)
        || engine.cold_loading[target].load(Ordering::Acquire) != 0
    {
        return Err(PyValueError::new_err(
            "copy requires an unloaded target without pending work",
        ));
    }
    let source = cache[binding.id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("copy source disappeared"))?;
    let lease = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?[binding.id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("copy requires verified complete source custody"))?;
    if engine.cold_lease_generations[binding.id].load(Ordering::Acquire)
        != binding.source_generation
        || intent.snapshot["source_identity"]["material_id"] != json!(lease.material_id)
    {
        return Err(PyValueError::new_err(
            "copy snapshot differs from current immutable material custody",
        ));
    }
    let key = engine
        .input_runtime_ownership
        .key_lock_status(binding.id)
        .ok_or_else(|| PyValueError::new_err("copy source mode acknowledgement unavailable"))?;
    if key.source_generation != binding.source_generation || key.effective != intent.key_lock {
        return Err(PyValueError::new_err(
            "copy source Key Lock mode has not acknowledged this intent",
        ));
    }
    let rate = binding.binding.sample_rate_hz;
    let digest = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?[binding.id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("copy source digest unavailable"))?;
    let base_request = requests[target];
    let request = base_request
        .checked_add(1)
        .filter(|request| *request <= u64::MAX / 4)
        .ok_or_else(|| PyRuntimeError::new_err("copy request identity exhausted"))?;
    let future_epoch =
        next_epoch(&engine.prepared_source_epochs[target]).map_err(PyRuntimeError::new_err)?;
    let future_authority = engine.input_runtime_ownership.next_authority(target)?;
    drop(cache);
    let selection = committed_stem_residency::capture_copy_selection(
        engine,
        &current,
        intent.descriptor().map_err(PyValueError::new_err)?,
    )
    .map_err(PyValueError::new_err)?;
    let owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
    let selection_fences = if selection.is_some() {
        Some(
            committed_stem_residency::capture_copy_selection_fences(&owners[target])
                .map_err(PyValueError::new_err)?,
        )
    } else {
        None
    };
    let source_selection_fences =
        committed_stem_residency::capture_copy_source_selection_fences(&owners[binding.id]);
    drop(owners);
    let lifetime =
        crate::audio_engine::content_lifetime::reserve().map_err(PyRuntimeError::new_err)?;
    let guard = CopyRtGuard {
        source: binding.id,
        target,
        source_binding: binding.binding,
        source_generation: binding.source_generation,
        source_epoch: engine.prepared_source_epochs[binding.id].clone(),
        captured_source_epoch: engine.prepared_source_epochs[binding.id].load(Ordering::Acquire),
        source_selection_fences: Some(source_selection_fences),
        target_epoch: engine.prepared_source_epochs[target].clone(),
        captured_target_epoch: engine.prepared_source_epochs[target].load(Ordering::Acquire),
        target_authority: engine.input_runtime_ownership.authority[target].load(Ordering::Acquire),
        target_runtime: engine.input_runtime_ownership.runtime[target].load(Ordering::Acquire),
        reservation: lifetime,
        phase: Arc::new(AtomicU8::new(0)),
        ownership: engine.input_runtime_ownership.clone(),
        acknowledgements: engine.current_timing_acknowledgements.clone(),
    };
    let state = Arc::new(PadCopyState {
        guard,
        capture: CopyCapture {
            source,
            lease,
            digest,
            rate,
            intent,
            timing,
            selection,
            selection_fences,
            request,
            base_request,
            future_epoch,
            future_authority,
            lifetime,
            resident: ResidentReadContext::new(
                engine.project_assets.clone(),
                engine.resident_account.clone(),
            )
            .with_history(engine.cold_pcm_history.clone(), target),
            lane: engine.cold_jobs.handle(),
        },
        prepared: Mutex::new(None),
        stage: AtomicU8::new(0),
        mirrored: AtomicBool::new(false),
        error: Mutex::new(None),
    });
    let mut states = engine
        .pad_copies
        .lock()
        .map_err(|_| PyRuntimeError::new_err("copy ownership lock poisoned"))?;
    if states[target].is_some() {
        return Err(PyRuntimeError::new_err(
            "copy target already has a transaction",
        ));
    }
    engine
        .input_runtime_ownership
        .reserve_copy(target, lifetime)?;
    states[target] = Some(state.clone());
    Ok(CapturedPadCopy { state })
}

pub(in crate::audio_engine) fn prepare(
    engine: &AudioEngine,
    captured: &CapturedPadCopy,
) -> PyResult<Arc<PadCopyState>> {
    captured.state.belongs_to(engine)?;
    captured
        .state
        .stage
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| PyValueError::new_err("copy preparation already started"))?;
    Ok(captured.state.clone())
}

pub(in crate::audio_engine) fn prepare_detached(
    state: Arc<PadCopyState>,
) -> Result<PadCopyTicket, String> {
    let operation = state.clone();
    let result = state.capture.lane.execute_owned(
        || state.guard.phase.load(Ordering::Acquire) == 3 || !state.guard.current(),
        move || prepare_owned(&operation),
    );
    match result {
        Ok(prepared) => {
            if state.guard.phase.load(Ordering::Acquire) != 0 || !state.guard.current() {
                state.guard.reject();
                return Err("copy preparation became stale".into());
            }
            *state
                .prepared
                .lock()
                .map_err(|_| "copy preparation lock poisoned")? = Some(prepared);
            state.stage.store(2, Ordering::Release);
            Ok(PadCopyTicket { state })
        }
        Err(error) => {
            state.guard.reject();
            if let Ok(mut stored) = state.error.lock() {
                *stored = Some(error.clone());
            }
            Err(error)
        }
    }
}

fn prepare_owned(state: &Arc<PadCopyState>) -> Result<PreparedCopyOwners, String> {
    let capture = &state.capture;
    let cancelled = || state.guard.phase.load(Ordering::Acquire) == 3 || !state.guard.current();
    let expected_birth = &capture.intent.snapshot["source_identity"]["material_creation"];
    if !expected_birth.is_null()
        && *expected_birth
            != serde_json::to_value(capture.lease.material_creation_receipt())
                .map_err(|error| error.to_string())?
    {
        return Err("copy historical material birth differs from verified source custody".into());
    }
    let saved_path = capture.intent.snapshot["source_path"]
        .as_str()
        .ok_or("copy source reference missing")?;
    let resolved = crate::audio_engine::material_paths::resolve(
        capture.lease.samples_root(),
        std::path::Path::new(saved_path),
    )
    .map_err(|error| error.to_string())?;
    if resolved.path != capture.lease.original_path {
        return Err("copy source reference differs from held original custody".into());
    }
    let lease = capture
        .lease
        .subscribe_verified()
        .map_err(|error| error.to_string())?;
    let sample = lease
        .read_window_resident(
            &capture.source,
            capture.source.resident_start(),
            capture.source.resident_end(),
            capture.future_epoch,
            capture
                .source
                .residency
                .as_ref()
                .ok_or("copy source descriptor missing")?
                .context,
            crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES,
            &cancelled,
            Some(&capture.resident),
        )
        .map_err(|error| error.to_string())?;
    let publication =
        PreparedSourcePermit::new(state.guard.target_epoch.clone(), capture.future_epoch);
    let timing = capture
        .timing
        .as_ref()
        .map(|record| {
            constant_timing::rebind_copy_timing(
                record,
                &sample,
                &lease,
                state.guard.target,
                capture.request,
                capture.request,
                capture.future_epoch,
                publication.clone(),
            )
        })
        .transpose()?;
    let projection = timing.as_ref().map(|(_, timing)| timing.projection);
    let mut intent = capture
        .intent
        .rt_intent(&sample, capture.rate, projection)?;
    let binding = InputPadBinding {
        resident: sample.resident_binding(),
        source_address: sample.source_address(),
        sample_count: sample.source_sample_count(),
        channels: sample.channels,
        sample_rate_hz: capture.rate,
        authority_revision: capture.future_authority,
        runtime_revision: state.guard.target_runtime,
        accepted: projection,
    };
    let selection = capture
        .selection
        .as_ref()
        .map(|selected| {
            committed_stem_residency::prepare_copy_selection(
                selected,
                &sample,
                &lease,
                committed_stem_residency::CopySelectionTarget {
                    id: state.guard.target,
                    source_generation: capture.request,
                    binding,
                    source_epoch: state.guard.target_epoch.clone(),
                    future_epoch: capture.future_epoch,
                    ownership: state.guard.ownership.clone(),
                    selection_fences: capture
                        .selection_fences
                        .clone()
                        .ok_or("copy selection fences missing")?,
                    publication: publication.clone(),
                    projection,
                    mode: intent.stem_mix_mode,
                },
                &capture.resident,
                &cancelled,
            )
        })
        .transpose()?;
    let stems = selection
        .as_ref()
        .and_then(|selected| selected.stem_owner.as_ref())
        .map(|owner| owner.set.clone());
    if intent.stem_mix_mode == StemMixMode::AllStems {
        if let Some(stems) = &stems {
            intent.source_version_hash = stems.source_version_hash;
        } else {
            intent.stem_mix_mode = StemMixMode::FullMix;
        }
    }
    if cancelled() {
        return Err("copy preparation cancelled or stale".into());
    }
    let rt = Box::new(PadCopyRtWork {
        target: state.guard.target,
        sample: sample.clone(),
        publication,
        stems,
        selection: selection.as_ref().map(|selected| selected.selected),
        selection_fences: selection
            .as_ref()
            .map(|selected| selected.selection_fences.clone()),
        projection,
        source_generation: capture.request,
        content_lifetime: capture.lifetime,
        prepared_epoch: capture.future_epoch,
        authority_revision: capture.future_authority,
        guard: state.guard.clone(),
        intent,
        retired_receipt: None,
        retired_stems: None,
        retired_outgoing: None,
    });
    Ok(PreparedCopyOwners {
        sample,
        lease,
        timing: timing.map(|(record, _)| record),
        selection,
        selection_ticket: None,
        rt: Some(rt),
    })
}

pub(in crate::audio_engine) fn publish(
    engine: &AudioEngine,
    ticket: &PadCopyTicket,
) -> PyResult<()> {
    let state = &ticket.state;
    state.belongs_to(engine)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    if state.stage.load(Ordering::Acquire) != 2
        || state.guard.phase.load(Ordering::Acquire) != 0
        || requests[state.guard.target] != state.capture.base_request
        || !state.guard.current()
    {
        return Err(PyValueError::new_err(
            "copy preparation is stale or already published",
        ));
    }
    let mut producer = engine
        .control_producer()?
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err("copy publication queue is full"));
    }
    let mut prepared = state
        .prepared
        .lock()
        .map_err(|_| PyRuntimeError::new_err("copy preparation lock poisoned"))?;
    let owners = prepared
        .as_mut()
        .ok_or_else(|| PyValueError::new_err("copy has not prepared its owners"))?;
    let work = owners
        .rt
        .take()
        .ok_or_else(|| PyValueError::new_err("copy payload already published"))?;
    work.publication
        .mark_pending()
        .map_err(PyRuntimeError::new_err)?;
    state.stage.store(3, Ordering::Release);
    producer
        .push(ControlMessage::PadCopy(work))
        .map_err(|_| PyRuntimeError::new_err("reserved single-producer copy capacity lost"))
}

pub(in crate::audio_engine) fn reconcile(
    engine: &AudioEngine,
    ticket: &PadCopyTicket,
) -> PyResult<bool> {
    let state = &ticket.state;
    state.belongs_to(engine)?;
    if state.guard.phase.load(Ordering::Acquire) != 2 {
        return Ok(false);
    }
    if state.mirrored.load(Ordering::Acquire) {
        return Ok(true);
    }
    let target = state.guard.target;
    let capture = &state.capture;
    // Acquire every fallible control lock before changing a mirror. A failed
    // settlement retains all custody and its reservation; it never rolls Native back.
    let mut requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut prepared = state
        .prepared
        .lock()
        .map_err(|_| PyRuntimeError::new_err("copy preparation lock poisoned"))?;
    let owners = prepared
        .as_mut()
        .ok_or_else(|| PyRuntimeError::new_err("acknowledged copy custody missing"))?;
    let mut cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let mut generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
    let mut digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
    let mut leases = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?;
    let mut intents = engine
        .timing_intents
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing intent lock poisoned"))?;
    let mut records = engine
        .current_constant_timing
        .lock()
        .map_err(|_| PyRuntimeError::new_err("current timing lock poisoned"))?;
    let mut resident = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
    if requests[target] != capture.base_request
        || cache[target].is_some()
        || state.guard.ownership.copy_reservation(target) != capture.lifetime
        || state.guard.target_epoch.load(Ordering::Acquire) != capture.future_epoch
        || state.guard.ownership.authority[target].load(Ordering::Acquire)
            != capture.future_authority
        || state
            .guard
            .ownership
            .source_generation(target, &owners.sample, capture.rate)
            != Some(capture.request)
        || engine.current_timing_acknowledgements.current_epoch(target)
            != owners
                .timing
                .as_ref()
                .map_or(0, |record| record.projection.publication_epoch)
    {
        return Err(PyRuntimeError::new_err(
            "acknowledged copy target ownership changed; settlement remains fenced",
        ));
    }
    if let Some(selection) = &owners.selection {
        owners.selection_ticket = Some(
            committed_stem_residency::reconcile_copy_selection(
                &mut resident[target],
                selection.clone(),
                &state.guard.phase,
            )
            .map_err(PyRuntimeError::new_err)?,
        );
    }
    requests[target] = capture.request;
    cache[target] = Some(owners.sample.clone());
    generations[target] = (capture.request, capture.rate);
    digests[target] = Some(capture.digest.clone());
    leases[target] = Some(owners.lease.clone());
    engine.cold_lease_generations[target].store(capture.request, Ordering::Release);
    intents[target] = capture.intent.timing_intent;
    records[target] = owners.timing.iter().cloned().collect();
    state.mirrored.store(true, Ordering::Release);
    Ok(true)
}

pub(in crate::audio_engine) fn finish(
    engine: &AudioEngine,
    ticket: &PadCopyTicket,
) -> PyResult<()> {
    let state = &ticket.state;
    state.belongs_to(engine)?;
    if state.guard.phase.load(Ordering::Acquire) != 2 || !state.mirrored.load(Ordering::Acquire) {
        return Err(PyValueError::new_err(
            "copy has not acknowledged and reconciled its complete target",
        ));
    }
    settle(engine, state)
}

pub(in crate::audio_engine) fn cancel(
    engine: &AudioEngine,
    captured: &CapturedPadCopy,
) -> PyResult<bool> {
    let state = &captured.state;
    state.belongs_to(engine)?;
    state.guard.reject();
    if state.guard.phase.load(Ordering::Acquire) != 3 {
        return Ok(false);
    }
    settle(engine, state)?;
    Ok(true)
}

fn settle(engine: &AudioEngine, state: &Arc<PadCopyState>) -> PyResult<()> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut states = engine
        .pad_copies
        .lock()
        .map_err(|_| PyRuntimeError::new_err("copy ownership lock poisoned"))?;
    let target = state.guard.target;
    if states[target]
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, state))
    {
        state
            .guard
            .ownership
            .release_copy(target, state.capture.lifetime)?;
        states[target] = None;
        return Ok(());
    }
    // Completion/cancellation is idempotent only after this exact reservation left.
    if state.guard.ownership.copy_reservation(target) == 0 {
        return Ok(());
    }
    Err(PyRuntimeError::new_err(
        "copy settlement no longer owns its target reservation",
    ))
}
