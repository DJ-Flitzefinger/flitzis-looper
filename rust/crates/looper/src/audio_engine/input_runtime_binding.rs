//! Native source/timing admission for MIDI snapshots; no PCM/evidence owner enters RT.

use super::AudioEngine;
use super::constant_timing::{
    self, AcceptedTimingProjection, CurrentConstantTimingRecord, CurrentTimingAcknowledgements,
};
use super::constants::NUM_SAMPLES;
use super::prepared_source::next_epoch;
use crate::messages::SampleBuffer;
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};

/// One control-owned current PCM publication. Zero generation fences all new starts.
/// Readers perform one bounded check and recheck; they never spin or dereference its address.
struct CurrentSourceFence {
    generation: AtomicU64,
    address: AtomicUsize,
    count: AtomicUsize,
    channels: AtomicUsize,
    rate: AtomicU32,
    resident_address: AtomicUsize,
    resident_start: AtomicUsize,
    resident_end: AtomicUsize,
    window_revision: AtomicU64,
    resident_context: AtomicU8,
}

impl Default for CurrentSourceFence {
    fn default() -> Self {
        Self {
            generation: AtomicU64::new(0),
            address: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
            channels: AtomicUsize::new(0),
            rate: AtomicU32::new(0),
            resident_address: AtomicUsize::new(0),
            resident_start: AtomicUsize::new(0),
            resident_end: AtomicUsize::new(0),
            window_revision: AtomicU64::new(0),
            resident_context: AtomicU8::new(0),
        }
    }
}

/// Control publishes revocation before queued callback work. Preparation requests
/// deliberately do not change these source/authority epochs.
pub(crate) struct InputRuntimeOwnership {
    pub(crate) authority: [AtomicU64; NUM_SAMPLES],
    pub(crate) runtime: [AtomicU64; NUM_SAMPLES],
    timing_intent: [AtomicU8; NUM_SAMPLES],
    source_tracking: bool,
    sources: [CurrentSourceFence; NUM_SAMPLES],
    cold_adoptions: [AtomicU64; NUM_SAMPLES],
    resident_controls: [AtomicU64; NUM_SAMPLES],
    launch_revisions: [AtomicU64; NUM_SAMPLES],
    admitted_launches: [AtomicBool; NUM_SAMPLES],
    migration_holds: [AtomicU64; NUM_SAMPLES],
}

impl Default for InputRuntimeOwnership {
    fn default() -> Self {
        Self {
            authority: std::array::from_fn(|_| AtomicU64::new(1)),
            runtime: std::array::from_fn(|_| AtomicU64::new(0)),
            timing_intent: std::array::from_fn(|_| AtomicU8::new(0)),
            source_tracking: false,
            sources: std::array::from_fn(|_| CurrentSourceFence::default()),
            cold_adoptions: std::array::from_fn(|_| AtomicU64::new(0)),
            resident_controls: std::array::from_fn(|_| AtomicU64::new(0)),
            launch_revisions: std::array::from_fn(|_| AtomicU64::new(0)),
            admitted_launches: std::array::from_fn(|_| AtomicBool::new(false)),
            migration_holds: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl InputRuntimeOwnership {
    pub(crate) fn mark_launch_admitted(&self, id: usize) {
        self.admitted_launches[id].store(true, Ordering::Release);
    }

    pub(super) fn clear_launch_admitted(&self, id: usize) {
        self.admitted_launches[id].store(false, Ordering::Release);
    }

    pub(super) fn clear_all_launches_admitted(&self) {
        for id in 0..NUM_SAMPLES {
            self.clear_launch_admitted(id);
        }
    }

    pub(super) fn admitted_launch_ids(&self) -> Vec<usize> {
        (0..NUM_SAMPLES)
            .filter(|&id| self.admitted_launches[id].load(Ordering::Acquire))
            .collect()
    }

    pub(crate) fn launch_revision(&self, id: usize) -> u64 {
        self.launch_revisions[id].load(Ordering::Acquire)
    }

    pub(crate) fn launch_current(&self, id: usize, revision: u64) -> bool {
        id < NUM_SAMPLES
            && revision != u64::MAX
            && self.migration_holds[id].load(Ordering::Acquire) == 0
            && self.launch_revision(id) == revision
    }

    pub(super) fn migration_hold(&self, id: usize) -> u64 {
        self.migration_holds[id].load(Ordering::Acquire)
    }

    pub(super) fn hold_migration(&self, id: usize, owner: u64) {
        self.cancel_launches(id);
        self.migration_holds[id].store(owner, Ordering::Release);
    }

    pub(super) fn release_migration(&self, id: usize, owner: u64) -> Result<(), String> {
        if self.migration_hold(id) != owner {
            return Err("migration hold owner changed".into());
        }
        // Starts admitted while held cannot become eligible after the hold opens.
        self.cancel_launches(id);
        self.migration_holds[id]
            .compare_exchange(owner, 0, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "migration hold owner changed".into())
            .map(|_| ())
    }

    /// A stop revokes earlier starts without changing adopted source/window/timing.
    /// One CAS suffices: a concurrent successful cancellation already revoked this value.
    pub(super) fn cancel_launches(&self, id: usize) {
        let revision = self.launch_revision(id);
        let _ = self.launch_revisions[id].compare_exchange(
            revision,
            revision.saturating_add(1),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub(super) fn cancel_all_launches(&self) {
        for id in 0..NUM_SAMPLES {
            self.cancel_launches(id);
        }
    }

    pub(super) fn begin_resident_control(&self, id: usize, intent: u64) {
        self.resident_controls[id].store(intent, Ordering::Release);
    }

    pub(crate) fn finish_resident_control(&self, id: usize, intent: u64) {
        let _ = self.resident_controls[id].compare_exchange(
            intent,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub(crate) fn resident_control_pending(&self, id: usize) -> bool {
        id < NUM_SAMPLES && self.resident_controls[id].load(Ordering::Acquire) != 0
    }

    pub(super) fn begin_cold(&self, id: usize, generation: u64) {
        self.cold_adoptions[id].store(generation * 4, Ordering::Release);
    }

    /// Fixed scalar callback acknowledgement; no per-job object is retired here.
    pub(crate) fn claim_cold(&self, id: usize, generation: u64) -> bool {
        self.cold_adoptions[id]
            .compare_exchange(
                generation * 4,
                generation * 4 + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub(crate) fn accept_cold(&self, id: usize, generation: u64) {
        let _ = self.cold_adoptions[id].compare_exchange(
            generation * 4 + 1,
            generation * 4 + 2,
            Ordering::Release,
            Ordering::Acquire,
        );
    }

    pub(crate) fn reject_claimed_cold(&self, id: usize, generation: u64) {
        let _ = self.cold_adoptions[id].compare_exchange(
            generation * 4 + 1,
            generation * 4 + 3,
            Ordering::Release,
            Ordering::Acquire,
        );
    }

    pub(super) fn cancel_cold(&self, id: usize, generation: u64) -> bool {
        self.cold_adoptions[id]
            .compare_exchange(
                generation * 4,
                generation * 4 + 3,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    #[cfg(test)]
    pub(super) fn cold_status(&self, id: usize, generation: u64) -> Option<u8> {
        let status = self.cold_adoptions[id].load(Ordering::Acquire);
        (status / 4 == generation).then_some((status % 4) as u8)
    }

    /// AudioEngine tracks actual native control-cache ownership; standalone mixers are untracked.
    pub(super) fn tracked() -> Self {
        Self {
            source_tracking: true,
            ..Self::default()
        }
    }

    pub(super) fn revoke_source(&self, id: usize) {
        self.sources[id].generation.store(0, Ordering::SeqCst);
        self.resident_controls[id].store(0, Ordering::Release);
    }

    pub(super) fn revoke_all_sources(&self) {
        for id in 0..NUM_SAMPLES {
            self.revoke_source(id);
        }
    }

    /// Called only after successful queue/cache/generation/digest publication under request owner.
    pub(super) fn publish_source(
        &self,
        id: usize,
        sample: &SampleBuffer,
        rate: u32,
        generation: u64,
    ) {
        let source = &self.sources[id];
        source.generation.store(0, Ordering::SeqCst);
        source
            .address
            .store(sample.source_address(), Ordering::SeqCst);
        source
            .count
            .store(sample.source_sample_count(), Ordering::SeqCst);
        source.channels.store(sample.channels, Ordering::SeqCst);
        source.rate.store(rate, Ordering::SeqCst);
        self.publish_window(id, sample);
        source.generation.store(generation, Ordering::SeqCst);
    }

    /// Actual callback adoption is the window linearization point. Complete source
    /// generation/authority stays unchanged; stale MIDI/preparation views are fenced.
    pub(crate) fn publish_window(&self, id: usize, sample: &SampleBuffer) {
        let source = &self.sources[id];
        source.window_revision.store(0, Ordering::SeqCst);
        source
            .resident_address
            .store(sample.samples.as_ptr() as usize, Ordering::SeqCst);
        source
            .resident_start
            .store(sample.resident_start(), Ordering::SeqCst);
        source
            .resident_end
            .store(sample.resident_end(), Ordering::SeqCst);
        source.resident_context.store(
            sample
                .residency
                .as_ref()
                .map_or(0, |view| match view.context {
                    crate::messages::ResidentContext::FiniteLoop => 1,
                    crate::messages::ResidentContext::FullTrack => 2,
                    crate::messages::ResidentContext::KeyLockFullTrack => 3,
                    crate::messages::ResidentContext::KeyLockFiniteLoop => 4,
                }),
            Ordering::SeqCst,
        );
        source
            .window_revision
            .store(sample.window_revision(), Ordering::SeqCst);
    }

    pub(crate) fn binding_window_current(&self, id: usize, binding: InputPadBinding) -> bool {
        if !self.source_tracking {
            return true;
        }
        let source = &self.sources[id];
        let Some(resident) = binding.resident else {
            return source.window_revision.load(Ordering::SeqCst) == 0;
        };
        let context = match resident.context {
            crate::messages::ResidentContext::FiniteLoop => 1,
            crate::messages::ResidentContext::FullTrack => 2,
            crate::messages::ResidentContext::KeyLockFullTrack => 3,
            crate::messages::ResidentContext::KeyLockFiniteLoop => 4,
        };
        source.window_revision.load(Ordering::SeqCst) == resident.revision
            && source.resident_address.load(Ordering::SeqCst) == resident.address
            && source.resident_start.load(Ordering::SeqCst) == resident.start
            && source.resident_end.load(Ordering::SeqCst) == resident.end
            && source.resident_context.load(Ordering::SeqCst) == context
            && source.window_revision.load(Ordering::SeqCst) == resident.revision
    }

    pub(crate) fn source_current(&self, id: usize, sample: &SampleBuffer, rate: u32) -> bool {
        self.source_generation(id, sample, rate).is_some()
    }

    /// Capture the actual loaded request generation with one bounded source check/recheck.
    /// Standalone mixers do not have control-cache tracking; their immutable pin is authoritative.
    pub(crate) fn source_generation(
        &self,
        id: usize,
        sample: &SampleBuffer,
        rate: u32,
    ) -> Option<u64> {
        if !self.source_tracking {
            return Some(1);
        }
        let source = &self.sources[id];
        let generation = source.generation.load(Ordering::SeqCst);
        (generation != 0
            && source.address.load(Ordering::SeqCst) == sample.source_address()
            && source.count.load(Ordering::SeqCst) == sample.source_sample_count()
            && source.channels.load(Ordering::SeqCst) == sample.channels
            && source.rate.load(Ordering::SeqCst) == rate
            && source.generation.load(Ordering::SeqCst) == generation)
            .then_some(generation)
    }

    /// Successful control admission publishes declared authority before its callback clear.
    pub(super) fn set_timing_intent(&self, id: usize, intent: TimingIntent) {
        let value = match intent {
            TimingIntent::Legacy => 0,
            TimingIntent::Automatic => 1,
            TimingIntent::Manual => 2,
            TimingIntent::Tap => 3,
        };
        self.timing_intent[id].store(value, Ordering::Release);
    }

    /// New voice admission cannot reinterpret unavailable Automatic as Legacy fallback.
    /// Existing effective source/history keeps rendering while a successful clear is pending.
    pub(crate) fn source_timing_available(
        &self,
        id: usize,
        accepted: Option<AcceptedTimingProjection>,
        acknowledgements: &CurrentTimingAcknowledgements,
    ) -> bool {
        if let Some(accepted) = accepted {
            acknowledgements.current_epoch(id) == accepted.publication_epoch
        } else {
            self.timing_intent[id].load(Ordering::Acquire) != 1
        }
    }

    pub(super) fn next_authority(&self, id: usize) -> PyResult<u64> {
        next_epoch(&self.authority[id]).map_err(PyRuntimeError::new_err)
    }

    pub(super) fn revoke(&self, id: usize, next: u64) {
        self.authority[id].store(next, Ordering::Release);
    }

    pub(crate) fn current(&self, id: usize, binding: InputPadBinding) -> bool {
        self.authority_current(id, binding)
            && self.binding_window_current(id, binding)
            && self.runtime[id].load(Ordering::Acquire) == binding.runtime_revision
    }

    pub(crate) fn authority_current(&self, id: usize, binding: InputPadBinding) -> bool {
        id < NUM_SAMPLES && self.authority[id].load(Ordering::Acquire) == binding.authority_revision
    }

    pub(crate) fn binding_source_current(&self, id: usize, binding: InputPadBinding) -> bool {
        self.binding_source_generation(id, binding).is_some()
            && self.binding_window_current(id, binding)
    }

    /// Read-only fixed source fence, including its checked generation for evidence consumers.
    pub(crate) fn binding_source_generation(
        &self,
        id: usize,
        binding: InputPadBinding,
    ) -> Option<u64> {
        if id >= NUM_SAMPLES {
            return None;
        }
        if !self.source_tracking {
            return Some(1);
        }
        let source = &self.sources[id];
        let generation = source.generation.load(Ordering::SeqCst);
        (generation != 0
            && source.address.load(Ordering::SeqCst) == binding.source_address
            && source.count.load(Ordering::SeqCst) == binding.sample_count
            && source.channels.load(Ordering::SeqCst) == binding.channels
            && source.rate.load(Ordering::SeqCst) == binding.sample_rate_hz
            && source.generation.load(Ordering::SeqCst) == generation)
            .then_some(generation)
    }
}

/// Capture every earlier admitted target and revoke its old launch revision at one
/// admission boundary. A live caller supplies the same producer mutex used by
/// UI, MIDI and global starts. None is only for an engine without a stream.
/// Pure cancellation keeps the flags set: a start may have passed its final RT
/// guard already and still needs an ordered STOP before feedback reaches Python.
pub(super) fn cancel_launches_with_producer(
    ownership: &InputRuntimeOwnership,
    producer: Option<&Arc<Mutex<rtrb::Producer<crate::messages::ControlMessage>>>>,
    id: Option<usize>,
) -> PyResult<Vec<usize>> {
    if id.is_some_and(|id| id >= NUM_SAMPLES) {
        return Err(pyo3::exceptions::PyValueError::new_err("id out of range"));
    }
    let _producer = producer
        .map(|producer| {
            producer
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))
        })
        .transpose()?;
    let targets = if let Some(id) = id {
        ownership.admitted_launches[id]
            .load(Ordering::Acquire)
            .then_some(id)
            .into_iter()
            .collect()
    } else {
        ownership.admitted_launch_ids()
    };
    if let Some(id) = id {
        ownership.cancel_launches(id);
    } else {
        ownership.cancel_all_launches();
    }
    Ok(targets)
}

/// Runtime admission is disabled before shutdown calls this control-only fence.
/// Recover a poisoned producer solely to revoke queued starts before worker joins;
/// public admission/cancellation still reports a poisoned producer as an error.
pub(super) fn cancel_launches_before_shutdown(
    ownership: &InputRuntimeOwnership,
    producer: Option<&Arc<Mutex<rtrb::Producer<crate::messages::ControlMessage>>>>,
) {
    let _producer = producer.map(|producer| {
        producer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    });
    ownership.cancel_all_launches();
}

/// Producer capacity is reserved before changing launch ownership. This helper is
/// shared by public stops and direct MIDI stops; callers hold the producer lock.
pub(super) fn enqueue_stop_with_producer(
    ownership: &InputRuntimeOwnership,
    producer: &mut rtrb::Producer<crate::messages::ControlMessage>,
    id: Option<usize>,
) -> bool {
    if producer.is_full() || id.is_some_and(|id| id >= NUM_SAMPLES) {
        return false;
    }
    let message = if let Some(id) = id {
        ownership.cancel_launches(id);
        ownership.clear_launch_admitted(id);
        crate::messages::ControlMessage::StopSample { id }
    } else {
        ownership.cancel_all_launches();
        ownership.clear_all_launches_admitted();
        crate::messages::ControlMessage::StopAll()
    };
    // The sole producer owns an available slot; the consumer can only free more.
    producer.push(message).is_ok()
}

/// Fixed scheduler value. Pointer is compared only; monotonic authority fences
/// reject source replacement/unload before the old callback bank is retired.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InputPadBinding {
    pub(crate) resident: Option<crate::messages::ResidentBinding>,
    pub(crate) source_address: usize,
    pub(crate) sample_count: usize,
    pub(crate) channels: usize,
    pub(crate) sample_rate_hz: u32,
    pub(crate) authority_revision: u64,
    pub(crate) runtime_revision: u64,
    pub(crate) accepted: Option<AcceptedTimingProjection>,
}

#[pyclass(frozen)]
pub struct InputRuntimePadBinding {
    pub(super) id: usize,
    pub(super) source_generation: u64,
    source_digest: String,
    sample_rate_hz: u32,
    intent: TimingIntent,
    pub(super) binding: InputPadBinding,
    pub(super) ownership: Arc<InputRuntimeOwnership>,
    pub(super) acknowledgements: Arc<CurrentTimingAcknowledgements>,
    accepted: Option<CurrentConstantTimingRecord>,
}

impl InputRuntimePadBinding {
    #[cfg(test)]
    pub(super) fn for_test(id: usize, ownership: Arc<InputRuntimeOwnership>) -> Self {
        Self {
            id,
            source_generation: 1,
            source_digest: "a".repeat(64),
            sample_rate_hz: 48_000,
            intent: TimingIntent::Legacy,
            binding: InputPadBinding {
                resident: None,
                source_address: 0,
                sample_count: 1,
                channels: 1,
                sample_rate_hz: 48_000,
                authority_revision: ownership.authority[id].load(Ordering::Acquire),
                runtime_revision: 0,
                accepted: None,
            },
            ownership,
            acknowledgements: Arc::default(),
            accepted: None,
        }
    }
    pub(super) fn current(&self) -> bool {
        self.ownership.authority_current(self.id, self.binding)
            && self.ownership.binding_source_current(self.id, self.binding)
            && self.acknowledgements.current_epoch(self.id)
                == self
                    .binding
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
            && self.ownership.authority_current(self.id, self.binding)
    }

    pub(super) fn available(&self) -> bool {
        self.intent != TimingIntent::Automatic || self.binding.accepted.is_some()
    }
}

#[pymethods]
impl InputRuntimePadBinding {
    /// Fresh copies of complete native metadata; historical bindings confer no authority.
    pub fn metadata(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let metadata = PyDict::new(py);
        metadata.set_item("pad_id", self.id)?;
        metadata.set_item(
            "source_id",
            format!("loaded-{}-{}", self.id, self.source_generation),
        )?;
        metadata.set_item("source_generation", self.source_generation)?;
        metadata.set_item("source_sha256", &self.source_digest)?;
        metadata.set_item("sample_rate_hz", self.sample_rate_hz)?;
        metadata.set_item(
            "frame_count",
            self.binding.sample_count / self.binding.channels,
        )?;
        metadata.set_item("channels", self.binding.channels)?;
        metadata.set_item(
            "window_revision",
            self.binding.resident.map_or(0, |view| view.revision),
        )?;
        metadata.set_item(
            "resident_start_frame",
            self.binding.resident.map_or(0, |view| view.start),
        )?;
        metadata.set_item(
            "resident_end_frame",
            self.binding
                .resident
                .map_or(self.binding.sample_count / self.binding.channels, |view| {
                    view.end
                }),
        )?;
        metadata.set_item(
            "resident_pcm_identity",
            self.binding
                .resident
                .map_or(self.binding.source_address, |view| view.address),
        )?;
        metadata.set_item(
            "resident_context",
            self.binding
                .resident
                .map_or("legacy-full-track", |view| match view.context {
                    crate::messages::ResidentContext::FiniteLoop => "finite-loop",
                    crate::messages::ResidentContext::FullTrack => "full-track",
                    crate::messages::ResidentContext::KeyLockFullTrack => "key-lock-full-track",
                    crate::messages::ResidentContext::KeyLockFiniteLoop => "key-lock-finite-loop",
                }),
        )?;
        metadata.set_item("authority_revision", self.binding.authority_revision)?;
        metadata.set_item(
            "intent",
            match self.intent {
                TimingIntent::Automatic => "automatic",
                TimingIntent::Manual => "manual",
                TimingIntent::Tap => "tap",
                TimingIntent::Legacy => "legacy",
            },
        )?;
        metadata.set_item(
            "accepted_timing",
            self.accepted
                .as_ref()
                .map(|record| record.metadata(py))
                .transpose()?,
        )?;
        Ok(metadata.into_any().unbind())
    }
}

/// Same lock order and actual current-record predicate as current_constant_timing.
pub(super) fn capture(engine: &AudioEngine, id: usize) -> PyResult<Option<InputRuntimePadBinding>> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    capture_under_request_lock(engine, id)
}

/// Shared current-source resolver for preparation and MIDI. The caller retains
/// the pad request lock through its own source/admission transaction.
pub(super) fn capture_under_request_lock(
    engine: &AudioEngine,
    id: usize,
) -> PyResult<Option<InputRuntimePadBinding>> {
    super::resident_relocation::reconcile_under_request_lock(engine)?;
    let intents = engine
        .timing_intents
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing intent lock poisoned"))?;
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
    let digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
    let mut all_records = engine
        .current_constant_timing
        .lock()
        .map_err(|_| PyRuntimeError::new_err("current timing lock poisoned"))?;
    let Some(source) = &cache[id] else {
        return Ok(None);
    };
    let (generation, rate) = generations[id];
    let Some(digest) = &digests[id] else {
        return Ok(None);
    };
    if generation == 0 || rate == 0 || source.channels == 0 {
        return Ok(None);
    }
    let records = &mut all_records[id];
    constant_timing::retire_old_current_records(engine, id, records);
    let acknowledged = engine.current_timing_acknowledgements.current_epoch(id);
    let accepted = if intents[id] == TimingIntent::Automatic {
        constant_timing::current_record_for_source(
            engine,
            id,
            source,
            generations[id],
            Some(digest),
            records,
        )
        .cloned()
    } else {
        None
    };
    if engine.current_timing_acknowledgements.current_epoch(id) != acknowledged {
        return Ok(None);
    }
    Ok(Some(InputRuntimePadBinding {
        id,
        source_generation: generation,
        source_digest: digest.clone(),
        sample_rate_hz: rate,
        intent: intents[id],
        binding: InputPadBinding {
            resident: source.resident_binding(),
            source_address: source.source_address(),
            sample_count: source.source_sample_count(),
            channels: source.channels,
            sample_rate_hz: rate,
            authority_revision: engine.input_runtime_ownership.authority[id]
                .load(Ordering::Acquire),
            runtime_revision: 0,
            accepted: accepted.as_ref().map(|record| record.projection),
        },
        ownership: engine.input_runtime_ownership.clone(),
        acknowledgements: engine.current_timing_acknowledgements.clone(),
        accepted,
    }))
}
