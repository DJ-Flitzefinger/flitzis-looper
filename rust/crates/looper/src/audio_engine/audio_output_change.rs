//! Narrow authority for model-free source restoration inside one output change.
//! Ordinary starts, recording, arrangements and neural analysis never use it.

use super::AudioEngine;
use super::cold_store::CommittedColdLease;
use super::constants::NUM_SAMPLES;
use super::content_placement::SlotSnapshot;
use super::input_runtime_binding::InputRuntimeOwnership;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::cell::Cell;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::TryRecvError;

#[derive(Clone)]
pub(super) struct SourceRestoreProof {
    epoch: u64,
    placement: SlotSnapshot,
    digest: String,
    original_generation: u64,
    // Exact original/cache assignment readers; no old-rate PCM is retained.
    lease: CommittedColdLease,
}

pub(super) struct OutputRestorePlan {
    epoch: u64,
    _paired_readers: Vec<std::sync::Arc<super::stem_pair::VerifiedStemPair>>,
    sources: Vec<Option<SourceRestoreProof>>,
    legacy_paths: Vec<Option<std::path::PathBuf>>,
    legacy_selections:
        Vec<Option<std::sync::Arc<super::audio_output_legacy::LegacyStemObservation>>>,
    _legacy_readers: Vec<std::fs::File>,
}

impl SourceRestoreProof {
    pub(super) fn validates(
        &self,
        ownership: &InputRuntimeOwnership,
        placement: SlotSnapshot,
        digest: &str,
    ) -> bool {
        self.epoch == ownership.output_change_epoch.load(Ordering::Acquire)
            && ownership.output_restore_allowed.load(Ordering::Acquire)
            && self.placement.same_projection(placement)
            && self.placement.content_key() == placement.content_key()
            && self.digest == digest
    }
}

pub(super) fn capture_restore_proof(
    engine: &AudioEngine,
    id: usize,
    path: &Path,
    source_intent: &str,
    replace_assignment: bool,
) -> Result<Option<SourceRestoreProof>, String> {
    let Some(scope) = RestoreScope::current() else {
        return Ok(None);
    };
    if scope.owner != engine.input_runtime_ownership.owner_identity().diagnostic()
        || scope.epoch
            != engine
                .input_runtime_ownership
                .output_change_epoch
                .load(Ordering::Acquire)
    {
        return Err("output restoration owner is stale".into());
    }
    if source_intent != "restore" || replace_assignment {
        return Err("output restoration admits only model-free existing originals".into());
    }
    let plan = engine
        .output_restore
        .lock()
        .map_err(|_| "output restoration lock poisoned")?;
    let plan = plan
        .as_ref()
        .filter(|plan| plan.epoch == scope.epoch)
        .ok_or("original output restoration plan is unavailable")?;
    let proof = plan
        .sources
        .get(id)
        .ok_or("source id out of range")?
        .clone();
    if let Some(proof) = proof.as_ref()
        && (path != proof.lease.original_path
            || !engine
                .input_runtime_ownership
                .slot_snapshot(id)
                .is_some_and(|slot| slot.same_projection(proof.placement)))
    {
        return Err("restored original differs from the captured output transaction".into());
    }
    Ok(proof)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RestoreScope {
    pub owner: usize,
    pub epoch: u64,
}

thread_local! {
    static RESTORE_SCOPE: Cell<Option<RestoreScope>> = const { Cell::new(None) };
}

struct RestoreGuard(Option<RestoreScope>);

impl Drop for RestoreGuard {
    fn drop(&mut self) {
        RESTORE_SCOPE.set(self.0);
    }
}

impl RestoreScope {
    pub fn current() -> Option<Self> {
        RESTORE_SCOPE.get()
    }

    pub fn run<T>(scope: Option<Self>, operation: impl FnOnce() -> T) -> T {
        let _guard = RestoreGuard(RESTORE_SCOPE.replace(scope));
        operation()
    }
}

/// A genuine scoped control call may hand its exact restore authority to one
/// queued worker. JSON, an odd epoch, or a saved-selection flag cannot create it.
#[pyclass(frozen)]
pub struct OutputRestoreJobScope {
    engine: Py<AudioEngine>,
    scope: RestoreScope,
}

impl OutputRestoreJobScope {
    pub(super) fn capture(slf: &Bound<'_, AudioEngine>) -> PyResult<Option<Self>> {
        let Some(scope) = RestoreScope::current() else {
            return Ok(None);
        };
        {
            let engine = slf.try_borrow()?;
            if restore_scope(&engine, scope.epoch)? != scope {
                return Err(PyRuntimeError::new_err(
                    "output restoration owner is foreign",
                ));
            }
        }
        Ok(Some(Self {
            engine: slf.clone().unbind(),
            scope,
        }))
    }
}

#[pymethods]
impl OutputRestoreJobScope {
    pub fn run(&self, py: Python<'_>, callback: Py<PyAny>) -> PyResult<Py<PyAny>> {
        {
            let engine = self.engine.bind(py).try_borrow()?;
            if restore_scope(&engine, self.scope.epoch)? != self.scope {
                return Err(PyRuntimeError::new_err(
                    "output restoration owner is foreign",
                ));
            }
        }
        // The callback may borrow/detach the Engine itself. Only its Python
        // owner is retained here; no Engine PyRef or lock crosses the callback.
        RestoreScope::run(Some(self.scope), || callback.call0(py))
    }
}

/// Preserve loader delivery order and its original pending-event custody while
/// refusing a change before all existing admissions and publications settle.
fn with_settled_sources<T>(
    engine: &AudioEngine,
    operation: impl FnOnce(&mut Vec<u64>) -> Result<T, String>,
) -> PyResult<T> {
    engine
        .offline_jobs
        .with_all_idle(|| {
            // Same receiver -> request -> pending-event order as loader_events::poll.
            let receiver = engine
                .loader_rx
                .lock()
                .map_err(|_| "loader receiver lock poisoned")?;
            let mut requests = engine
                .pad_request_ids
                .lock()
                .map_err(|_| "request lock poisoned")?;
            let mut pending = engine
                .pending_loader_event
                .lock()
                .map_err(|_| "pending loader event lock poisoned")?;
            if pending.is_none() {
                match receiver.try_recv() {
                    Ok(event) => *pending = Some(event),
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        return Err("loader delivery is disconnected".into());
                    }
                }
            }
            if pending.is_some() {
                return Err("original loader publication is not settled".into());
            }
            let loading = engine
                .loading_sample_ids
                .lock()
                .map_err(|_| "loading lock poisoned")?;
            let tasks = engine
                .active_tasks
                .lock()
                .map_err(|_| "task lock poisoned")?;
            if !loading.is_empty()
                || !tasks.is_empty()
                || engine.constant_timing_busy.load(Ordering::Acquire)
            {
                return Err("original source/preparation work is not settled".into());
            }
            engine.cold_jobs.with_idle(|| operation(&mut requests))
        })
        .map_err(PyRuntimeError::new_err)
}

pub(super) fn begin(engine: &AudioEngine) -> PyResult<u64> {
    engine.with_mix_capture_settled(|| {
        with_settled_sources(engine, |_| {
            let retired = engine
                .stream_handle
                .as_ref()
                .is_some_and(|handle| handle.is_closed())
                || engine
                    .stream_lifecycle
                    .as_ref()
                    .is_some_and(|state| state.quiescence == super::StreamQuiescence::Verified);
            let claim = || {
                engine
                    .input_runtime_ownership
                    .begin_output_change_after_close(retired)
                    .map_err(|error| error.to_string())
            };
            if let Some(runtime) = engine.input_runtime.as_ref() {
                runtime.with_output_change_idle(claim)
            } else {
                claim()
            }
        })
    })
}

pub(super) fn enable_restore(engine: &AudioEngine, epoch: u64) -> PyResult<()> {
    validate_epoch(engine, epoch)?;
    if engine.stream_handle.is_none() || engine.audio_stream_failed() {
        return Err(PyRuntimeError::new_err(
            "a healthy output must be installed before source restoration",
        ));
    }
    engine
        .input_runtime_ownership
        .output_restore_allowed
        .store(true, Ordering::Release);
    Ok(())
}

pub(super) fn validate_epoch(engine: &AudioEngine, epoch: u64) -> PyResult<()> {
    if epoch & 1 == 0
        || epoch == u64::MAX
        || engine
            .input_runtime_ownership
            .output_change_epoch
            .load(Ordering::Acquire)
            != epoch
    {
        return Err(PyRuntimeError::new_err(
            "audio output change owner is stale",
        ));
    }
    Ok(())
}

pub(super) fn restore_scope(engine: &AudioEngine, epoch: u64) -> PyResult<RestoreScope> {
    validate_epoch(engine, epoch)?;
    if !engine
        .input_runtime_ownership
        .output_restore_allowed
        .load(Ordering::Acquire)
        || engine.stream_handle.is_none()
        || engine.audio_stream_failed()
    {
        return Err(PyRuntimeError::new_err(
            "audio output source restoration is unavailable",
        ));
    }
    Ok(RestoreScope {
        owner: engine.input_runtime_ownership.owner_identity().diagnostic(),
        epoch,
    })
}

pub(super) fn finish(engine: &AudioEngine, epoch: u64) -> PyResult<()> {
    validate_epoch(engine, epoch)?;
    if engine.stream_handle.is_none() || engine.audio_stream_failed() {
        return Err(PyRuntimeError::new_err(
            "a healthy output must be installed before Apply completes",
        ));
    }
    let retired = with_settled_sources(engine, |_| {
        let mut plan = engine
            .output_restore
            .lock()
            .map_err(|_| "output restoration lock poisoned")?;
        let plan_ref = plan
            .as_ref()
            .filter(|plan| plan.epoch == epoch)
            .ok_or("original output restoration plan is unavailable")?;
        for (id, proof) in plan_ref.sources.iter().enumerate() {
            let Some(proof) = proof else {
                continue;
            };
            let binding = super::input_runtime_binding::capture_under_request_lock(engine, id)
                .map_err(|error| error.to_string())?
                .ok_or("restored original source is unavailable")?;
            if !binding.current()
                || !binding.available()
                || binding.source_generation == proof.original_generation
                || binding.source_digest() != proof.digest
                || binding.sample_rate_hz()
                    != engine.stream_handle.as_ref().unwrap().output_sample_rate
                || !engine
                    .input_runtime_ownership
                    .slot_snapshot(id)
                    .is_some_and(|slot| slot.content_key() == proof.placement.content_key())
                || !engine
                    .input_runtime_ownership
                    .binding_source_adoption_accepted(id, binding.binding)
            {
                return Err("restored original has no matching fresh source OwnACK".into());
            }
        }
        if plan_ref
            .legacy_selections
            .iter()
            .flatten()
            .any(|selection| !selection.current())
        {
            return Err(
                "restored legacy selection lost its own current mode/mask acknowledgement".into(),
            );
        }
        engine
            .input_runtime_ownership
            .finish_output_change_if(epoch, || {
                !engine
                    .input_runtime_ownership
                    .output_unavailable
                    .load(Ordering::Acquire)
                    && !engine
                        .stream_handle
                        .as_ref()
                        .unwrap()
                        .stream_failed
                        .load(Ordering::Acquire)
                    && plan_ref
                        .legacy_selections
                        .iter()
                        .flatten()
                        .all(|selection| selection.current())
            })
            .map_err(|error| error.to_string())?;
        Ok(plan.take())
    })?;
    // Release the original descriptor/file readers only after fresh source and
    // selection owners have completed the same transaction's accepted restore.
    drop(retired);
    Ok(())
}

pub(super) fn ensure_sources_settled(engine: &AudioEngine, epoch: u64) -> PyResult<()> {
    validate_epoch(engine, epoch)?;
    engine.with_mix_capture_settled(|| with_settled_sources(engine, |_| Ok(())))
}

pub(super) fn ensure_startup_settled(engine: &AudioEngine) -> PyResult<()> {
    if engine.stream_handle.is_none() || engine.audio_stream_failed() {
        return Err(PyRuntimeError::new_err(
            "startup requires a healthy installed output",
        ));
    }
    if engine
        .input_runtime_ownership
        .output_change_epoch
        .load(Ordering::Acquire)
        & 1
        != 0
    {
        return Err(PyRuntimeError::new_err(
            "startup cannot bypass an output transaction",
        ));
    }
    engine.with_mix_capture_settled(|| with_settled_sources(engine, |_| Ok(())))
}

pub(super) fn finish_empty_startup(engine: &AudioEngine, epoch: u64) -> PyResult<()> {
    validate_epoch(engine, epoch)?;
    {
        let plan = engine
            .output_restore
            .lock()
            .map_err(|_| PyRuntimeError::new_err("output restoration lock poisoned"))?;
        if !plan
            .as_ref()
            .is_some_and(|plan| plan.epoch == epoch && plan.sources.iter().all(Option::is_none))
        {
            return Err(PyRuntimeError::new_err(
                "device-only startup requires an originally empty Native image",
            ));
        }
    }
    if engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?
        .iter()
        .any(Option::is_some)
    {
        return Err(PyRuntimeError::new_err(
            "device-only startup already has source owners",
        ));
    }
    finish(engine, epoch)
}

/// Complete every source/file/counter preflight before the old device is touched.
pub(super) fn prepare_plan(
    engine: &AudioEngine,
    epoch: u64,
    _requested_rate: Option<u32>,
) -> PyResult<()> {
    validate_epoch(engine, epoch)?;
    super::resident_relocation::reconcile(engine)?;
    with_settled_sources(engine, |requests| {
        let mut plan = engine
            .output_restore
            .lock()
            .map_err(|_| "output restoration lock poisoned")?;
        if let Some(plan) = plan.as_ref() {
            return if plan.epoch == epoch {
                Ok(())
            } else {
                Err("another output plan remains owned".into())
            };
        }
        let cache = engine
            .sample_cache
            .lock()
            .map_err(|_| "sample cache lock poisoned")?;
        let generations = engine
            .loaded_source_generations
            .lock()
            .map_err(|_| "source generation lock poisoned")?;
        let digests = engine
            .loaded_source_digests
            .lock()
            .map_err(|_| "source digest lock poisoned")?;
        let leases = engine
            .cold_leases
            .lock()
            .map_err(|_| "cold lease lock poisoned")?;
        let owners = engine
            .resident_stem_cache
            .lock()
            .map_err(|_| "resident stem lock poisoned")?;
        let mut sources = Vec::with_capacity(NUM_SAMPLES);
        let mut legacy_readers = Vec::new();
        let mut legacy_paths = vec![None; NUM_SAMPLES];
        let mut paired_readers = Vec::new();
        for id in 0..NUM_SAMPLES {
            let Some(sample) = cache[id].as_ref() else {
                sources.push(None);
                continue;
            };
            let (generation, rate) = generations[id];
            if let Some(owner) = owners[id]
                .accepted
                .as_ref()
                .filter(|owner| owner.pair.is_some())
            {
                if owner.set.sample_rate_hz != rate {
                    return Err("accepted paired-stem source rate is inconsistent".into());
                }
                paired_readers.push(
                    owner
                        .pair
                        .as_ref()
                        .ok_or("Pair reader missing")?
                        .reader
                        .clone(),
                );
            }
            let placement = engine
                .input_runtime_ownership
                .slot_snapshot(id)
                .ok_or("source placement unavailable")?;
            let digest = digests[id]
                .as_ref()
                .filter(|digest| digest.len() == 64)
                .ok_or("loaded digest unavailable")?;
            let lease = leases[id]
                .as_ref()
                .ok_or("original source lease unavailable")?;
            if generation == 0
                || rate == 0
                || engine
                    .input_runtime_ownership
                    .source_generation(id, sample, rate)
                    != Some(generation)
                || engine.cold_lease_generations[id].load(Ordering::Acquire) != generation
            {
                return Err("original source authority is not settled".into());
            }
            lease
                .verify_reference(sample)
                .map_err(|error| error.to_string())?;
            let original = &lease.manifest.descriptor["decoder"]["original"];
            if original["sha256"].as_str() != Some(digest.as_str())
                || original["bytes"].as_u64().is_none_or(|bytes| bytes == 0)
            {
                return Err("original manifest/digest proof disagrees".into());
            }
            sources.push(Some(SourceRestoreProof {
                epoch,
                placement,
                digest: digest.clone(),
                original_generation: generation,
                lease: lease.clone(),
            }));
            if let Some(owner) = owners[id]
                .accepted
                .as_ref()
                .filter(|owner| owner.pair.is_none())
            {
                // Retain the original five sealed legacy files across the PCM
                // drop. This neither writes a descriptor nor carries old PCM.
                legacy_readers.extend(
                    super::project_assets::stem_readers::seal_stem_artifacts(
                        &owner.generation_path,
                    )
                    .map_err(|error| error.to_string())?,
                );
                legacy_paths[id] = Some(owner.generation_path.clone());
            }
        }
        engine
            .input_runtime_ownership
            .preflight_output_retirement()?;
        for request in requests {
            request.checked_add(1).ok_or("source request exhausted")?;
        }
        for epoch in &engine.prepared_source_epochs {
            super::prepared_source::next_epoch(epoch)?;
        }
        for owner in owners.iter() {
            owner.preflight_output_reset()?;
        }
        *plan = Some(OutputRestorePlan {
            epoch,
            _paired_readers: paired_readers,
            sources,
            legacy_paths,
            legacy_selections: vec![None; NUM_SAMPLES],
            _legacy_readers: legacy_readers,
        });
        Ok(())
    })
}

pub(super) fn cancel_before_close(engine: &AudioEngine, epoch: u64) -> PyResult<bool> {
    validate_epoch(engine, epoch)?;
    if !engine
        .stream_handle
        .as_ref()
        .is_some_and(|stream| stream.original_output_untouched(epoch))
    {
        return Ok(false);
    }
    let retired = with_settled_sources(engine, |_| {
        let mut plan = engine
            .output_restore
            .lock()
            .map_err(|_| "output restoration lock poisoned")?;
        engine
            .input_runtime_ownership
            .finish_output_change_if(epoch, || {
                engine
                    .stream_handle
                    .as_ref()
                    .is_some_and(|stream| stream.original_output_untouched(epoch))
                    && !engine
                        .input_runtime_ownership
                        .output_unavailable
                        .load(Ordering::Acquire)
            })
            .map_err(|error| error.to_string())?;
        Ok(plan.take())
    })?;
    drop(retired);
    Ok(true)
}

pub(super) fn validate_legacy_path(
    engine: &AudioEngine,
    epoch: u64,
    id: usize,
    path: &Path,
) -> PyResult<()> {
    let plan = engine
        .output_restore
        .lock()
        .map_err(|_| PyRuntimeError::new_err("output restoration lock poisoned"))?;
    let plan = plan
        .as_ref()
        .filter(|plan| plan.epoch == epoch)
        .ok_or_else(|| {
            PyRuntimeError::new_err("original output restoration plan is unavailable")
        })?;
    let original = plan
        .legacy_paths
        .get(id)
        .ok_or_else(|| PyRuntimeError::new_err("legacy source id out of range"))?;
    if original.as_ref().is_some_and(|original| original != path) {
        return Err(PyRuntimeError::new_err(
            "legacy stem files differ from the original output owner",
        ));
    }
    Ok(())
}

pub(super) fn remember_legacy_selection(
    engine: &AudioEngine,
    epoch: u64,
    id: usize,
    selection: std::sync::Arc<super::audio_output_legacy::LegacyStemObservation>,
) -> PyResult<()> {
    let mut plan = engine
        .output_restore
        .lock()
        .map_err(|_| PyRuntimeError::new_err("output restoration lock poisoned"))?;
    let plan = plan
        .as_mut()
        .filter(|plan| plan.epoch == epoch)
        .ok_or_else(|| {
            PyRuntimeError::new_err("original output restoration plan is unavailable")
        })?;
    let slot = plan
        .legacy_selections
        .get_mut(id)
        .ok_or_else(|| PyRuntimeError::new_err("legacy source id out of range"))?;
    *slot = Some(selection);
    Ok(())
}

pub(super) fn reset_after_retirement(engine: &AudioEngine, epoch: u64) -> PyResult<()> {
    validate_epoch(engine, epoch)?;
    if engine.has_retained_stream_owner()
        || engine.input_runtime.is_some()
        || engine
            .stream_lifecycle
            .as_ref()
            .is_some_and(|state| state.quiescence != super::StreamQuiescence::Verified)
    {
        return Err(PyRuntimeError::new_err(
            "original output/dispatcher retirement is not verified",
        ));
    }
    engine.ensure_mix_capture_settled_before_run()?;
    super::resident_relocation::reconcile(engine)?;
    super::prepared_stems_work::reconcile(engine)?;
    let retired = with_settled_sources(engine, |requests| {
        let plan = engine
            .output_restore
            .lock()
            .map_err(|_| "output restoration lock poisoned")?;
        let copies = engine
            .pad_copies
            .lock()
            .map_err(|_| "copy state lock poisoned")?;
        let banks = engine
            .bank_mutations
            .lock()
            .map_err(|_| "bank state lock poisoned")?;
        if copies.iter().any(Option::is_some) || !banks.is_empty() {
            return Err("original Copy/Bank reservation collectors are not settled".into());
        }
        let mut cache = engine
            .sample_cache
            .lock()
            .map_err(|_| "sample cache lock poisoned")?;
        let mut generations = engine
            .loaded_source_generations
            .lock()
            .map_err(|_| "source generation lock poisoned")?;
        let mut digests = engine
            .loaded_source_digests
            .lock()
            .map_err(|_| "source digest lock poisoned")?;
        let mut leases = engine
            .cold_leases
            .lock()
            .map_err(|_| "cold lease lock poisoned")?;
        let mut owners = engine
            .resident_stem_cache
            .lock()
            .map_err(|_| "resident stem lock poisoned")?;
        let mut incoming = engine
            .incoming_loads
            .lock()
            .map_err(|_| "incoming source lock poisoned")?;
        let mut guards = engine
            .resident_restore_guards
            .lock()
            .map_err(|_| "resident restore lock poisoned")?;
        let pending = engine
            .pending_prepared_stems
            .lock()
            .map_err(|_| "prepared result lock poisoned")?;
        let mut timing = engine
            .current_constant_timing
            .lock()
            .map_err(|_| "current timing lock poisoned")?;
        if pending.iter().any(Option::is_some)
            || incoming
                .iter()
                .flatten()
                .any(|owner| !owner.settled_after_job_end())
        {
            return Err("original source/prepared publication collectors are not settled".into());
        }
        if plan.as_ref().is_some_and(|plan| plan.epoch != epoch) {
            return Err("another output restoration plan remains owned".into());
        }
        if plan.is_none() {
            return Err("original output plan must be prepared before device retirement".into());
        }
        // All fallible counter/collector checks precede the first invalidation.
        let selection_revision = engine
            .input_runtime_ownership
            .preflight_output_retirement()?;
        let next_requests: Vec<_> = requests
            .iter()
            .map(|request| request.checked_add(1).ok_or("source request exhausted"))
            .collect::<Result<_, _>>()?;
        let next_prepared: Vec<_> = engine
            .prepared_source_epochs
            .iter()
            .map(|epoch| super::prepared_source::next_epoch(epoch))
            .collect::<Result<_, _>>()?;
        let next_owners: Vec<_> = owners
            .iter()
            .map(|owner| owner.preflight_output_reset())
            .collect::<Result<_, _>>()?;
        engine
            .input_runtime_ownership
            .retire_output_sources(selection_revision);
        requests.copy_from_slice(&next_requests);
        for (counter, next) in engine.prepared_source_epochs.iter().zip(next_prepared) {
            counter.store(next, Ordering::Release);
        }
        let retired_owners: Vec<_> = owners
            .iter_mut()
            .zip(next_owners)
            .map(|(owner, next)| owner.take_output_owners(next))
            .collect();
        let old_cache: Vec<_> = cache.iter_mut().map(Option::take).collect();
        let old_leases: Vec<_> = leases.iter_mut().map(Option::take).collect();
        let old_guards: Vec<_> = guards.iter_mut().map(Option::take).collect();
        let old_incoming: Vec<_> = incoming.iter_mut().map(Option::take).collect();
        for generation in generations.iter_mut() {
            *generation = (0, 0);
        }
        for digest in digests.iter_mut() {
            *digest = None;
        }
        for generation in engine.cold_lease_generations.iter() {
            generation.store(0, Ordering::Release);
        }
        engine.current_timing_acknowledgements.clear_all();
        engine.loop_acceptance.reset();
        for records in timing.iter_mut() {
            records.clear();
        }
        Ok((
            old_cache,
            old_leases,
            old_guards,
            old_incoming,
            retired_owners,
        ))
    })?;
    // Every possibly final PCM/reader destruction occurs after request, Cold,
    // publication and cache locks have been released, on this control worker.
    drop(retired);
    engine.waveform_requests.cancel_all();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::AudioMessage;
    use std::sync::{Arc, Mutex};

    #[test]
    fn original_feedback_drains_through_python_while_output_owner_has_shared_borrow() {
        Python::initialize();
        Python::attach(|py| {
            let mut engine = AudioEngine::new().unwrap();
            assert!(engine.stream_handle.is_none());
            let (control, _commands) = rtrb::RingBuffer::new(8);
            let (mut feedback, consumer) = rtrb::RingBuffer::new(8);
            feedback.push(AudioMessage::Pong()).unwrap();
            engine.test_control_channels = Some((
                Arc::new(Mutex::new(control)),
                Arc::new(Mutex::new(consumer)),
            ));
            let engine = Py::new(py, engine).unwrap();
            let shared_owner = engine.borrow(py);
            let message = engine.call_method0(py, "receive_msg").unwrap();
            assert!(matches!(
                &*message
                    .bind(py)
                    .extract::<PyRef<'_, AudioMessage>>()
                    .unwrap(),
                AudioMessage::Pong()
            ));
            assert!(
                engine
                    .call_method0(py, "receive_msg")
                    .unwrap()
                    .bind(py)
                    .is_none()
            );
            // The actual shared borrow stays live through BOTH Python dispatches.
            assert!(shared_owner.stream_handle.is_none());
            drop(shared_owner);
            engine.borrow_mut(py).shut_down().unwrap();
        });
    }

    #[test]
    fn restoration_authority_is_scoped_to_the_original_owner_and_call() {
        assert_eq!(RestoreScope::current(), None);
        let scope = RestoreScope {
            owner: 42,
            epoch: 7,
        };
        RestoreScope::run(Some(scope), || {
            assert_eq!(RestoreScope::current(), Some(scope));
            assert_eq!(
                std::thread::spawn(RestoreScope::current).join().unwrap(),
                None
            );
            RestoreScope::run(None, || assert_eq!(RestoreScope::current(), None));
            assert_eq!(RestoreScope::current(), Some(scope));
        });
        assert_eq!(RestoreScope::current(), None);
    }
}
