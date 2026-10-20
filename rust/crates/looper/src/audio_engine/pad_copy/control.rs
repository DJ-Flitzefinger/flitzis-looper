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
use crate::audio_engine::resident_relocation::ResidentStemCache;
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
    previous_sample: Option<SampleBuffer>,
    previous_lease: Option<CommittedColdLease>,
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
    engine_asset_owner: Arc<()>,
    lane: ColdJobsHandle,
}

/// Only a rejected Copy may release its fresh subscriber. The actual callback
/// phase and cleanup-epoch owner remain retained through uncertain adoption.
struct CopySubscription {
    lease: CommittedColdLease,
    guard: CopyRtGuard,
    assets: Arc<crate::audio_engine::project_assets::ProjectAssets>,
    _engine_asset_owner: Arc<()>,
    transferred: bool,
}

impl CopySubscription {
    fn transfer_to_target(&mut self) {
        self.transferred = true;
    }
}

impl Drop for CopySubscription {
    fn drop(&mut self) {
        if self.transferred {
            return;
        }
        // Reject an unpublished/unclaimed payload before revoking its exact ID.
        // Phase 1 or 2 may already own effective audio and cannot be rolled back.
        self.guard.reject();
        if self.guard.phase.load(Ordering::Acquire) == 3 {
            self.assets.orphan_cold(&self.lease);
            self.lease.rollback_unadopted_cache();
        }
    }
}

struct PreparedCopyOwners {
    sample: SampleBuffer,
    lease: CommittedColdLease,
    subscription: CopySubscription,
    timing: Option<CurrentConstantTimingRecord>,
    selection: Option<PreparedCopySelection>,
    selection_ticket: Option<CommittedStemSetTicket>,
    rt: Option<Box<PadCopyRtWork>>,
    _retired_resident: Option<ResidentStemCache>,
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
            "original_target_lifetime": self.state.guard.target_placement.lifetime(),
            "content_lifetime": self.state.capture.lifetime, "status": self.state.status(),
            "control_mirrored": self.state.mirrored.load(Ordering::Acquire), "components_available": components_available}).to_string()
    }
}

pub(in crate::audio_engine) fn capture(
    engine: &AudioEngine,
    binding: &InputRuntimePadBinding,
    target: usize,
    intent_json: &str,
    target_binding: Option<&InputRuntimePadBinding>,
) -> PyResult<CapturedPadCopy> {
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let state = capture_image(
        engine,
        binding,
        target,
        intent_json,
        target_binding,
        &requests,
        None,
    )?;
    let lifetime = state.capture.lifetime;
    let target_placement = state.guard.target_placement;
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
        .reserve_copy_target(target_placement, lifetime)?;
    if !state.guard.current() {
        engine
            .input_runtime_ownership
            .release_copy(target, lifetime)?;
        return Err(PyValueError::new_err(
            "copy participants changed before reservation",
        ));
    }
    states[target] = Some(state.clone());
    Ok(CapturedPadCopy { state })
}

fn capture_image(
    engine: &AudioEngine,
    binding: &InputRuntimePadBinding,
    target: usize,
    intent_json: &str,
    target_binding: Option<&InputRuntimePadBinding>,
    requests: &[u64],
    parent: Option<(u64, Arc<AtomicU8>)>,
) -> PyResult<Arc<PadCopyState>> {
    if target >= NUM_SAMPLES || binding.id >= NUM_SAMPLES || target == binding.id {
        return Err(PyValueError::new_err(
            "copy requires distinct valid source and target",
        ));
    }
    let intent = CopyIntent::parse(intent_json).map_err(PyValueError::new_err)?;
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(target)?;
    let current = input_runtime_binding::capture_under_request_lock(engine, binding.id)?
        .filter(|current| current.available() && current.current())
        .ok_or_else(|| PyValueError::new_err("copy source is unavailable"))?;
    if !binding.current()
        || !binding.binding.same_projection(&current.binding)
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
    let original_target = match target_binding {
        Some(expected) => {
            let current = input_runtime_binding::capture_under_request_lock(engine, target)?
                .filter(InputRuntimePadBinding::current)
                .ok_or_else(|| PyValueError::new_err("copy overwrite target is unavailable"))?;
            if expected.id != target
                || !Arc::ptr_eq(&expected.ownership, &engine.input_runtime_ownership)
                || !expected.current()
                || !expected.binding.same_projection(&current.binding)
                || expected.source_generation != current.source_generation
            {
                return Err(PyValueError::new_err(
                    "stale or foreign copy overwrite target binding",
                ));
            }
            Some(current)
        }
        None => None,
    };
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    if (original_target.is_none()
        && (cache[target].is_some() || !engine.input_runtime_ownership.copy_target_empty(target)))
        || engine
            .loading_sample_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?
            .contains(&target)
        || engine.cold_loading[target].load(Ordering::Acquire) != 0
    {
        return Err(PyValueError::new_err(
            "copy requires its original target without pending source work",
        ));
    }
    let source = cache[binding.id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("copy source disappeared"))?;
    let leases = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?;
    let lease = leases[binding.id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("copy requires verified complete source custody"))?;
    let previous_sample = cache[target].clone();
    let previous_lease = leases[target].clone();
    if original_target.is_some() && (previous_sample.is_none() || previous_lease.is_none()) {
        return Err(PyValueError::new_err(
            "copy overwrite target custody is unavailable",
        ));
    }
    drop(leases);
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
    let source_epoch = engine
        .prepared_source_epoch(binding.id)
        .map_err(PyRuntimeError::new_err)?
        .clone();
    let target_epoch = engine
        .prepared_source_epoch(target)
        .map_err(PyRuntimeError::new_err)?
        .clone();
    let future_epoch = next_epoch(&target_epoch).map_err(PyRuntimeError::new_err)?;
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
    let selection_fences = Some(
        committed_stem_residency::capture_copy_selection_fences(&owners[target])
            .map_err(PyValueError::new_err)?,
    );
    let mut target_selection_fences =
        committed_stem_residency::capture_pair_selection_fences(&owners[target]);
    let target_placement = engine
        .input_runtime_ownership
        .slot_snapshot(target)
        .ok_or_else(|| PyRuntimeError::new_err("copy target placement is pending"))?;
    target_selection_fences.extend(
        engine
            .resident_proposals
            .capture_pair_fences_for_cell(target_placement.cell())
            .ok_or_else(|| PyRuntimeError::new_err("copy target proposals are changing"))?,
    );
    if target_selection_fences.len() > crate::audio_engine::pad_pair::MAX_PAIR_SELECTION_FENCES {
        return Err(PyRuntimeError::new_err(
            "copy target selection guard capacity unavailable",
        ));
    }
    let target_selection_fences =
        std::array::from_fn(|index| target_selection_fences.get(index).cloned());
    let source_selection_fences =
        committed_stem_residency::capture_copy_source_selection_fences(&owners[binding.id]);
    drop(owners);
    let lifetime =
        crate::audio_engine::content_lifetime::reserve().map_err(PyRuntimeError::new_err)?;
    // Public control bindings deliberately omit the MIDI publication revision. Copy must
    // capture its actual current value under this same request mutex, rather than treating
    // that control-only zero as the admitted runtime. Later runtime edits still fence Copy.
    let mut source_binding = current.binding;
    source_binding.runtime_revision = engine.input_runtime_ownership.runtime_revision(binding.id);
    let target_binding = original_target.as_ref().map(|current| {
        let mut binding = current.binding;
        binding.runtime_revision = engine.input_runtime_ownership.runtime_revision(target);
        binding
    });
    let guard = CopyRtGuard {
        source: binding.id,
        target,
        source_binding,
        source_generation: binding.source_generation,
        captured_source_epoch: source_epoch.load(Ordering::Acquire),
        source_epoch,
        source_selection_fences: Some(source_selection_fences),
        captured_target_epoch: target_epoch.load(Ordering::Acquire),
        target_epoch,
        target_authority: engine.input_runtime_ownership.authority_revision(target),
        target_runtime: engine.input_runtime_ownership.runtime_revision(target),
        target_placement,
        target_binding,
        target_source_generation: original_target
            .as_ref()
            .map_or(0, |binding| binding.source_generation),
        target_selection_fences,
        reservation: parent.as_ref().map_or(lifetime, |(owner, _)| *owner),
        source_reservation: parent.as_ref().map_or(0, |(owner, _)| *owner),
        phase: parent.map_or_else(|| Arc::new(AtomicU8::new(0)), |(_, phase)| phase),
        ownership: engine.input_runtime_ownership.clone(),
        acknowledgements: engine.current_timing_acknowledgements.clone(),
    };
    let state = Arc::new(PadCopyState {
        guard,
        capture: CopyCapture {
            source,
            lease,
            previous_sample,
            previous_lease,
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
            engine_asset_owner: engine.project_asset_engine_owner.clone(),
            lane: engine.cold_jobs.handle(),
        },
        prepared: Mutex::new(None),
        stage: AtomicU8::new(0),
        mirrored: AtomicBool::new(false),
        error: Mutex::new(None),
    });
    Ok(state)
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
    let subscription = CopySubscription {
        lease: lease.clone(),
        guard: state.guard.clone(),
        assets: capture.resident.assets.clone(),
        _engine_asset_owner: capture.engine_asset_owner.clone(),
        transferred: false,
    };
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
    // Consume this source read's own reserved reader slot before timing or stem
    // preparation can append other registrations to the same resident context.
    let mut registration = capture.resident.take_registration()?;
    capture
        .resident
        .assets
        .retain_cold_reserved(
            lease.clone(),
            &sample,
            Arc::downgrade(&capture.engine_asset_owner),
            &mut registration,
        )
        .map_err(|error| error.to_string())?;
    let publication =
        PreparedSourcePermit::new(state.guard.target_epoch.clone(), capture.future_epoch);
    let mut timing = capture
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
    let placement = state
        .guard
        .ownership
        .prepare_slot_write(state.guard.target_placement, capture.lifetime)?
        .after();
    if let Some((record, prepared)) = &mut timing {
        record.content_key = placement.content_key();
        prepared.content_key = placement.content_key();
    }
    let projection = timing.as_ref().map(|(_, timing)| timing.projection);
    let mut intent = capture
        .intent
        .rt_intent(&sample, capture.rate, projection)?;
    let binding = InputPadBinding {
        placement: Some(placement),
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
        content_key: placement
            .content_key()
            .ok_or("copy target lifetime is missing")?,
        target: state.guard.target,
        sample: sample.clone(),
        publication,
        stems,
        selection: selection.as_ref().map(|selected| selected.selected),
        selection_fences: selection
            .as_ref()
            .map(|selected| selected.selection_fences.clone())
            .or_else(|| capture.selection_fences.clone()),
        projection,
        source_generation: capture.request,
        content_lifetime: capture.lifetime,
        prepared_epoch: capture.future_epoch,
        authority_revision: capture.future_authority,
        guard: state.guard.clone(),
        intent,
        retired_receipt: None,
        retired_sample: None,
        retired_stems: None,
        retired_outgoing: None,
    });
    Ok(PreparedCopyOwners {
        sample,
        lease,
        subscription,
        timing: timing.map(|(record, _)| record),
        selection,
        selection_ticket: None,
        rt: Some(rt),
        _retired_resident: None,
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
    let original_sample_current = match (&capture.previous_sample, &cache[target]) {
        (Some(captured), Some(current)) => {
            captured.same_source(current) && captured.same_window(current)
        }
        (None, None) => true,
        _ => false,
    };
    let original_lease_current = match (&capture.previous_lease, &leases[target]) {
        (Some(captured), Some(current)) => {
            captured.assignment_id() == current.assignment_id() && !current.assignment_retired()
        }
        (None, None) => true,
        _ => false,
    };
    if requests[target] != capture.base_request
        || !original_sample_current
        || !original_lease_current
        || state.guard.ownership.copy_reservation(target) != capture.lifetime
        || state.guard.target_epoch.load(Ordering::Acquire) != capture.future_epoch
        || state.guard.ownership.authority_revision(target) != capture.future_authority
        || state
            .guard
            .ownership
            .source_generation(target, &owners.sample, capture.rate)
            != Some(capture.request)
        || engine
            .current_timing_acknowledgements
            .current_for_slot(&engine.input_runtime_ownership, target)
            != owners
                .timing
                .as_ref()
                .map_or(0, |record| record.projection.publication_epoch)
    {
        return Err(PyRuntimeError::new_err(
            "acknowledged copy target ownership changed; settlement remains fenced",
        ));
    }
    // Finish every fallible new selection check against a detached successor.
    // Old target readers remain owned until all locks/checks and own ACK agree.
    let mut replacement = committed_stem_residency::prepare_copy_replacement(&resident[target]);
    let selection_ticket = if let Some(selection) = &owners.selection {
        Some(
            committed_stem_residency::reconcile_copy_selection(
                &mut replacement,
                selection.clone(),
                &state.guard.phase,
            )
            .map_err(PyRuntimeError::new_err)?,
        )
    } else {
        None
    };
    owners.selection_ticket = selection_ticket;
    owners._retired_resident = Some(std::mem::replace(&mut resident[target], replacement));
    requests[target] = capture.request;
    cache[target] = Some(owners.sample.clone());
    generations[target] = (capture.request, capture.rate);
    digests[target] = Some(capture.digest.clone());
    leases[target] = Some(owners.lease.clone());
    engine.cold_lease_generations[target].store(capture.request, Ordering::Release);
    intents[target] = capture.intent.timing_intent;
    records[target] = owners.timing.iter().cloned().collect();
    owners.subscription.transfer_to_target();
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

pub(crate) mod bank;
