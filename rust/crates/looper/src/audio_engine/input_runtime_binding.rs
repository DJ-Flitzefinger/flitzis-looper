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
    Arc,
    atomic::{AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};

/// One control-owned current PCM publication. Zero generation fences all new starts.
/// Readers perform one bounded check and recheck; they never spin or dereference its address.
struct CurrentSourceFence {
    generation: AtomicU64,
    address: AtomicUsize,
    count: AtomicUsize,
    channels: AtomicUsize,
    rate: AtomicU32,
}

impl Default for CurrentSourceFence {
    fn default() -> Self {
        Self {
            generation: AtomicU64::new(0),
            address: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
            channels: AtomicUsize::new(0),
            rate: AtomicU32::new(0),
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
}

impl Default for InputRuntimeOwnership {
    fn default() -> Self {
        Self {
            authority: std::array::from_fn(|_| AtomicU64::new(1)),
            runtime: std::array::from_fn(|_| AtomicU64::new(0)),
            timing_intent: std::array::from_fn(|_| AtomicU8::new(0)),
            source_tracking: false,
            sources: std::array::from_fn(|_| CurrentSourceFence::default()),
        }
    }
}

impl InputRuntimeOwnership {
    /// AudioEngine tracks actual native control-cache ownership; standalone mixers are untracked.
    pub(super) fn tracked() -> Self {
        Self {
            source_tracking: true,
            ..Self::default()
        }
    }

    pub(super) fn revoke_source(&self, id: usize) {
        self.sources[id].generation.store(0, Ordering::SeqCst);
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
            .store(sample.samples.as_ptr() as usize, Ordering::SeqCst);
        source.count.store(sample.samples.len(), Ordering::SeqCst);
        source.channels.store(sample.channels, Ordering::SeqCst);
        source.rate.store(rate, Ordering::SeqCst);
        source.generation.store(generation, Ordering::SeqCst);
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
            && source.address.load(Ordering::SeqCst) == sample.samples.as_ptr() as usize
            && source.count.load(Ordering::SeqCst) == sample.samples.len()
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
            && self.runtime[id].load(Ordering::Acquire) == binding.runtime_revision
    }

    pub(crate) fn authority_current(&self, id: usize, binding: InputPadBinding) -> bool {
        id < NUM_SAMPLES && self.authority[id].load(Ordering::Acquire) == binding.authority_revision
    }

    pub(crate) fn binding_source_current(&self, id: usize, binding: InputPadBinding) -> bool {
        if id >= NUM_SAMPLES {
            return false;
        }
        if !self.source_tracking {
            return true;
        }
        let source = &self.sources[id];
        let generation = source.generation.load(Ordering::SeqCst);
        generation != 0
            && source.address.load(Ordering::SeqCst) == binding.source_address
            && source.count.load(Ordering::SeqCst) == binding.sample_count
            && source.channels.load(Ordering::SeqCst) == binding.channels
            && source.rate.load(Ordering::SeqCst) == binding.sample_rate_hz
            && source.generation.load(Ordering::SeqCst) == generation
    }
}

/// Fixed scheduler value. Pointer is compared only; monotonic authority fences
/// reject source replacement/unload before the old callback bank is retired.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InputPadBinding {
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
    source_generation: u64,
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
            source_address: source.samples.as_ptr() as usize,
            sample_count: source.samples.len(),
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
