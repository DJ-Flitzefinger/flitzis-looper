//! Bounded immutable complete-source readers. No playback bank owns this context.
//!
//! A reader pins its sealed cache lease through every read, even if its pad unloads.
//! Projection/export scans fixed chunks rather than allocating complete playback PCM.
use super::cold_jobs::PCM_LIMIT_BYTES;
use super::cold_store::CommittedColdLease;
use super::content_placement::{ContentKey, PlacementObservation, SlotSnapshot};
use super::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership, SourcePublication};
use super::progress::LoadProgressStage;
use super::{AudioEngine, PadTaskGuard, admit_sample_analysis, analyze_sample};
use crate::messages::SampleBuffer;
use crate::messages::{BackgroundTaskKind, LoaderEvent};
use pyo3::exceptions::PyRuntimeError;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
#[path = "source_custody_tests.rs"]
pub(super) mod source_custody_tests;

const CHUNK_SAMPLES: usize = 4096;
pub(super) const READER_SCRATCH_BYTES: usize = CHUNK_SAMPLES * 12;

pub(super) struct CompleteSourceReader {
    pub(super) reference: SampleBuffer,
    lease: Option<CommittedColdLease>,
    pub(super) guard: Option<CompleteSourceGuard>,
    pub(super) analysis_owner: Option<NativeAnalysisOwner>,
}

/// One admitted Native request owns its original content and cell freshness epoch.
/// Backend identities remain diagnostic; placement is resolved only at control delivery.
#[derive(Clone)]
pub(crate) struct NativeAnalysisOwner {
    source: CompleteSourceGuard,
    epoch: Option<(Arc<AtomicU64>, u64)>,
}

impl std::fmt::Debug for NativeAnalysisOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeAnalysisOwner")
            .field("key", &self.source.key())
            .field(
                "admitted_epoch",
                &self.epoch.as_ref().map(|(_, expected)| *expected),
            )
            .finish_non_exhaustive()
    }
}

impl NativeAnalysisOwner {
    pub(super) fn same_admission(&self, other: &Self) -> bool {
        let same_epoch = match (&self.epoch, &other.epoch) {
            (Some((left, expected)), Some((right, other_expected))) => {
                Arc::ptr_eq(left, right) && expected == other_expected
            }
            (None, None) => true,
            _ => false,
        };
        same_epoch
            && Arc::ptr_eq(&self.source.ownership, &other.source.ownership)
            && self.source.binding.same_projection(&other.source.binding)
            && self.source.generation == other.source.generation
            && self.source.rate == other.source.rate
    }
    /// Called under the existing request lock, immediately after its accepted advance.
    pub(super) fn new(source: CompleteSourceGuard, epoch: Arc<AtomicU64>) -> Self {
        let admitted_epoch = epoch.load(Ordering::Acquire);
        Self {
            source,
            epoch: Some((epoch, admitted_epoch)),
        }
    }

    /// The actual source publisher carries its original Native permit into delivery.
    /// Later timing requests may stale its analysis without losing source metadata.
    pub(super) fn from_publication(
        publication: &SourcePublication,
        sample: &SampleBuffer,
        rate: u32,
    ) -> Self {
        Self {
            source: CompleteSourceGuard {
                generation: publication.source_generation,
                rate,
                id: publication.placement.slot(),
                binding: publication.binding(sample, rate),
                ownership: publication.ownership.clone(),
            },
            epoch: None,
        }
    }

    pub(super) fn current(&self) -> bool {
        self.source.current()
            && self
                .epoch
                .as_ref()
                .is_none_or(|(epoch, expected)| epoch.load(Ordering::Acquire) == *expected)
    }

    pub(super) fn belongs_to(&self, ownership: &Arc<InputRuntimeOwnership>) -> bool {
        Arc::ptr_eq(&self.source.ownership, ownership)
    }

    pub(super) fn observe_control(&self) -> PlacementObservation<SlotSnapshot> {
        if !self.current() {
            return PlacementObservation::Stale;
        }
        self.source.observe_control()
    }

    /// Retiring work still reserves its own source even after request cancellation.
    pub(super) fn observe_source_control(&self) -> PlacementObservation<SlotSnapshot> {
        self.source.observe_control()
    }

    /// Busy/cancellation address only this original content's Native and control rows.
    /// A short mirror gap never makes unrelated stable cells part of its reservation.
    pub(super) fn blocks_pad(&self, id: usize) -> bool {
        if !self.source.current() {
            return false;
        }
        match self.observe_source_control() {
            PlacementObservation::Current(snapshot) => snapshot.slot() == id,
            PlacementObservation::Pending => {
                let Some(key) = self.source.key() else {
                    return false;
                };
                let ownership = &self.source.ownership;
                let native_matches = matches!(ownership.observe_content_key(key),
                    PlacementObservation::Current(snapshot) if snapshot.slot() == id);
                let control_matches = ownership.control_slot_for_cell(key.cell()) == id;
                self.source.current() && (native_matches || control_matches)
            }
            PlacementObservation::Stale | PlacementObservation::Retired => false,
        }
    }

    pub(super) fn event(&self, event: LoaderEvent) -> LoaderEvent {
        LoaderEvent::NativeOwned {
            owner: self.clone(),
            event: Box::new(event),
        }
    }
}

#[derive(Clone)]
pub(super) struct CompleteSourceGuard {
    pub(super) generation: u64,
    pub(super) rate: u32,
    pub(super) id: usize,
    pub(super) binding: InputPadBinding,
    pub(super) ownership: Arc<InputRuntimeOwnership>,
}

impl CompleteSourceGuard {
    /// Fixed-slot admission captures the original Native incarnation before any worker starts.
    pub(super) fn capture(
        engine: &AudioEngine,
        id: usize,
        reference: &SampleBuffer,
        generation: u64,
        rate: u32,
    ) -> Result<Self, String> {
        let ownership = &engine.input_runtime_ownership;
        ownership
            .ensure_no_copy_reservation(id)
            .map_err(|error| error.to_string())?;
        let placement = ownership
            .slot_snapshot(id)
            .filter(|snapshot| snapshot.content_key().is_some())
            .ok_or("complete reader content placement is unavailable")?;
        let key = placement
            .content_key()
            .ok_or("complete reader content key unavailable")?;
        if !matches!(ownership.observe_control_key(key), PlacementObservation::Current(current)
            if current.same_projection(placement))
        {
            return Err("complete reader control placement is pending or changed".into());
        }
        let binding = InputPadBinding {
            placement: Some(placement),
            resident: reference.resident_binding(),
            source_address: reference.source_address(),
            sample_count: reference.source_sample_count(),
            channels: reference.channels,
            sample_rate_hz: rate,
            authority_revision: ownership.authority_revision(id),
            runtime_revision: 0,
            accepted: None,
        };
        if generation == 0
            || rate == 0
            || !ownership.admission_source_current(id, binding)
            || ownership.binding_source_generation(id, binding) != Some(generation)
        {
            return Err("complete reader source assignment is stale or unacknowledged".into());
        }
        Ok(Self {
            generation,
            rate,
            id,
            binding,
            ownership: ownership.clone(),
        })
    }

    /// Immutable readers keep their source permission through timing/window maintenance.
    /// Native key/cell/lifetime and complete backing geometry still fence real replacement.
    pub(super) fn current(&self) -> bool {
        self.ownership
            .binding_source_generation(self.id, self.binding)
            == Some(self.generation)
            && self
                .key()
                .is_some_and(|key| self.ownership.content_key_current(key))
    }

    pub(super) fn key(&self) -> Option<ContentKey> {
        self.binding
            .placement
            .and_then(|placement| placement.content_key())
    }

    /// Control consumers wait for the complete cache mirror, retaining the original owner.
    pub(super) fn observe_control(&self) -> PlacementObservation<SlotSnapshot> {
        self.key().map_or(PlacementObservation::Stale, |key| {
            self.ownership.observe_control_key(key)
        })
    }
}

impl CompleteSourceReader {
    /// Admission performs arithmetic and ownership capture, without file I/O.
    pub(super) fn capture(
        engine: &AudioEngine,
        id: usize,
        reference: SampleBuffer,
    ) -> Result<Self, String> {
        let finite =
            reference.resident_start() != 0 || reference.resident_end() != reference.frame_count();
        let lease = if finite {
            Some(
                engine
                    .cold_leases
                    .lock()
                    .map_err(|_| "complete lease lock poisoned")?
                    .get(id)
                    .and_then(Clone::clone)
                    .ok_or("complete source lease unavailable")?,
            )
        } else {
            None
        };
        let (generation, rate) = engine
            .loaded_source_generations
            .lock()
            .map_err(|_| "source generation lock poisoned")?[id];
        let guard = CompleteSourceGuard::capture(engine, id, &reference, generation, rate)?;
        let mut reader = Self::new(reference, lease)?;
        reader.guard = Some(guard);
        Ok(reader)
    }

    pub(super) fn new(
        reference: SampleBuffer,
        lease: Option<CommittedColdLease>,
    ) -> Result<Self, String> {
        if reference.channels == 0
            || reference.channels > 32
            || reference.samples.is_empty()
            || !reference.samples.len().is_multiple_of(reference.channels)
        {
            return Err("complete reader has invalid PCM geometry".into());
        }
        if reference.residency.as_ref().is_some_and(|view| {
            view.source.channels != reference.channels
                || view.source.frame_count == 0
                || view.source.sample_rate_hz == 0
                || view.source.source_zero_frame != 0
                || reference.resident_end() > view.source.frame_count
        }) {
            return Err("complete reader descriptor disagrees with resident geometry".into());
        }
        if reference.resident_start() != 0 || reference.resident_end() != reference.frame_count() {
            let lease = lease.as_ref().ok_or("complete source lease unavailable")?;
            lease
                .verify_reference(&reference)
                .map_err(|error| error.to_string())?;
        }
        Ok(Self {
            reference,
            lease,
            guard: None,
            analysis_owner: None,
        })
    }

    pub(super) fn held_bytes(&self) -> Result<usize, String> {
        self.reference
            .samples
            .len()
            .checked_mul(4)
            .ok_or_else(|| "complete reader extent overflow".into())
    }

    pub(super) fn admit_scan(&self, maximum: usize, output_bytes: usize) -> Result<usize, String> {
        let peak = self
            .held_bytes()?
            .checked_add(READER_SCRATCH_BYTES)
            .and_then(|n| n.checked_add(output_bytes))
            .ok_or("complete scan budget overflow")?;
        if peak > maximum.min(PCM_LIMIT_BYTES) {
            return Err("complete source scan exceeds transient PCM limit".into());
        }
        Ok(peak)
    }

    /// Visit complete-source coordinates using the same immutable content authority.
    /// Lease + reference remain owned until the visitor and final read return.
    pub(super) fn visit_region(
        &self,
        region: Range<usize>,
        cancelled: &dyn Fn() -> bool,
        mut visit: impl FnMut(usize, &[f32]) -> io::Result<()>,
    ) -> io::Result<()> {
        if region.start > region.end || region.end > self.reference.frame_count() {
            return Err(io::Error::other("complete reader range is outside source"));
        }
        let check = || {
            if cancelled() || self.guard.as_ref().is_some_and(|guard| !guard.current()) {
                Err(io::Error::other("complete source read cancelled"))
            } else {
                Ok(())
            }
        };
        check()?;
        let channels = self.reference.channels;
        let frames_per_chunk = CHUNK_SAMPLES / channels;
        if let Some(lease) = self.lease.as_ref() {
            let mut file = lease.open_complete_reader(&self.reference)?;
            let offset = region
                .start
                .checked_mul(channels)
                .and_then(|n| n.checked_mul(4))
                .ok_or_else(|| io::Error::other("complete reader offset overflow"))?;
            file.seek(SeekFrom::Start(offset as u64))?;
            let mut bytes = [0_u8; CHUNK_SAMPLES * 4];
            let mut samples = Vec::with_capacity(CHUNK_SAMPLES);
            for first in (region.start..region.end).step_by(frames_per_chunk) {
                check()?;
                let count = (region.end - first).min(frames_per_chunk) * channels;
                file.read_exact(&mut bytes[..count * 4])?;
                samples.clear();
                samples.extend(
                    bytes[..count * 4]
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes(b.try_into().expect("f32 bytes"))),
                );
                visit(first, &samples)?;
            }
        } else {
            for first in (region.start..region.end).step_by(frames_per_chunk) {
                check()?;
                let end = (first + frames_per_chunk).min(region.end);
                visit(
                    first,
                    &self.reference.samples[first * channels..end * channels],
                )?;
            }
        }
        check()
    }

    pub(super) fn stream_mono(
        &self,
        output: &mut impl Write,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<()> {
        let channels = self.reference.channels;
        let mut bytes = [0_u8; CHUNK_SAMPLES * 4];
        self.visit_region(0..self.reference.frame_count(), cancelled, |_, chunk| {
            for (frame, destination) in chunk.chunks_exact(channels).zip(bytes.chunks_exact_mut(4))
            {
                if frame.iter().any(|value| !value.is_finite()) {
                    return Err(io::Error::other("non-finite complete PCM"));
                }
                let mean =
                    (frame.iter().map(|&n| f64::from(n)).sum::<f64>() / channels as f64) as f32;
                destination.copy_from_slice(&mean.to_le_bytes());
            }
            output.write_all(&bytes[..chunk.len() / channels * 4])?;
            Ok(())
        })?;
        output.flush()
    }

    pub(super) fn materialize(
        &self,
        maximum: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SampleBuffer, String> {
        if cancelled() || self.guard.as_ref().is_some_and(|guard| !guard.current()) {
            return Err("complete source read cancelled".into());
        }
        match self.lease.as_ref() {
            Some(lease) => lease
                .read_complete_cancellable(&self.reference, maximum, &|| {
                    cancelled() || self.guard.as_ref().is_some_and(|guard| !guard.current())
                })
                .map_err(|error| error.to_string()),
            None => Ok(self.reference.clone()),
        }
    }
}

/// Productive bounded worker path shared by the API and device-free execution.
pub(super) fn start_analysis(engine: &AudioEngine, id: usize) -> pyo3::PyResult<u64> {
    start_analysis_with(engine, id, || {})
}

fn start_analysis_with(
    engine: &AudioEngine,
    id: usize,
    after_admission: impl FnOnce() + Send + 'static,
) -> pyo3::PyResult<u64> {
    let reservation = engine
        .cold_jobs
        .reserve()
        .map_err(PyRuntimeError::new_err)?;
    let (reader, request_id, output_sample_rate) = admit_sample_analysis(engine, id)?;
    let owner = reader
        .analysis_owner
        .clone()
        .ok_or_else(|| PyRuntimeError::new_err("analysis admission owner unavailable"))?;

    let loader_tx = engine.loader_tx.clone();
    let active_tasks = engine.active_tasks.clone();
    let task_id = reader
        .guard
        .as_ref()
        .and_then(CompleteSourceGuard::key)
        .ok_or_else(|| PyRuntimeError::new_err("analysis task content key unavailable"))?
        .cell()
        .index();
    let task_token = engine
        .analysis_task_tokens
        .lock()
        .map_err(|_| PyRuntimeError::new_err("analysis task token lock poisoned"))?
        .get(&(task_id, BackgroundTaskKind::Analysis))
        .cloned()
        .ok_or_else(|| PyRuntimeError::new_err("analysis task token unavailable"))?;

    let task_guard = PadTaskGuard {
        id: task_id,
        task: BackgroundTaskKind::Analysis,
        active_tasks,
        task_token,
        task_tokens: engine.analysis_task_tokens.clone(),
    };
    let shutdown = engine.cold_cancelled.clone();
    engine
        .cold_jobs
        .submit(reservation, move || {
            let _task_guard = task_guard;
            after_admission();
            let cancelled = || {
                shutdown.load(Ordering::Acquire)
                    || !owner.current()
                    || reader.guard.as_ref().is_some_and(|guard| !guard.current())
            };

            let _ = loader_tx.send(owner.event(LoaderEvent::TaskStarted {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
            }));

            let stage = LoadProgressStage::Analyzing.stage_label().to_string();
            let _ = loader_tx.send(owner.event(LoaderEvent::TaskProgress {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
                percent: 0.0,
                stage: stage.clone(),
            }));

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                reader
                    .materialize(super::constant_timing::MAX_PCM_BYTES, &cancelled)
                    .and_then(|sample| {
                        if cancelled() {
                            return Err("Analysis superseded by another pad request".into());
                        }
                        analyze_sample(&sample, output_sample_rate)
                    })
            }))
            .unwrap_or_else(|_| Err("Analysis worker failed".into()));
            let analysis = match result {
                Ok(result) => result,
                Err(error) => {
                    if cancelled() {
                        let _ = loader_tx.send(owner.event(LoaderEvent::TaskError {
                            id,
                            request_id,
                            task: BackgroundTaskKind::Analysis,
                            error: "Analysis superseded by another pad request".into(),
                        }));
                        return;
                    }
                    let _ = loader_tx.send(owner.event(LoaderEvent::TaskError {
                        id,
                        request_id,
                        task: BackgroundTaskKind::Analysis,
                        error,
                    }));
                    return;
                }
            };

            if cancelled() {
                let _ = loader_tx.send(owner.event(LoaderEvent::TaskError {
                    id,
                    request_id,
                    task: BackgroundTaskKind::Analysis,
                    error: "Analysis superseded by another pad request".into(),
                }));
                return;
            }

            let _ = loader_tx.send(owner.event(LoaderEvent::TaskProgress {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
                percent: 1.0,
                stage,
            }));

            let _ = loader_tx.send(owner.event(LoaderEvent::TaskSuccess {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
                analysis: Some(analysis),
            }));
        })
        .map_err(PyRuntimeError::new_err)?;

    Ok(request_id)
}

#[cfg(test)]
#[path = "analysis_owner_tests.rs"]
mod analysis_owner_tests;
