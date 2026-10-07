//! Bounded immutable complete-source readers. No playback bank owns this context.
//!
//! A reader pins its sealed cache lease through every read, even if its pad unloads.
//! Projection/export scans fixed chunks rather than allocating complete playback PCM.
use super::cold_jobs;
use super::cold_jobs::PCM_LIMIT_BYTES;
use super::cold_store::CommittedColdLease;
use super::progress::LoadProgressStage;
use super::{
    AudioEngine, PadTaskGuard, admit_sample_analysis, analyze_sample, pad_request_matches,
};
use crate::messages::SampleBuffer;
use crate::messages::{BackgroundTaskKind, LoaderEvent};
use pyo3::exceptions::PyRuntimeError;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

const CHUNK_SAMPLES: usize = 4096;
pub(super) const READER_SCRATCH_BYTES: usize = CHUNK_SAMPLES * 12;

pub(super) struct CompleteSourceReader {
    pub(super) reference: SampleBuffer,
    lease: Option<CommittedColdLease>,
    pub(super) guard: Option<CompleteSourceGuard>,
}

#[derive(Clone)]
pub(super) struct CompleteSourceGuard {
    pub(super) generation: u64,
    pub(super) rate: u32,
    id: usize,
    generations: Arc<Mutex<Vec<(u64, u32)>>>,
}

impl CompleteSourceGuard {
    pub(super) fn current(&self) -> bool {
        self.generations
            .lock()
            .is_ok_and(|values| values.get(self.id) == Some(&(self.generation, self.rate)))
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
        if engine
            .input_runtime_ownership
            .source_generation(id, &reference, rate)
            != Some(generation)
            || generation == 0
        {
            return Err("complete reader source assignment is stale or unacknowledged".into());
        }
        let mut reader = Self::new(reference, lease)?;
        reader.guard = Some(CompleteSourceGuard {
            generation,
            rate,
            id,
            generations: engine.loaded_source_generations.clone(),
        });
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
    let reservation = engine
        .cold_jobs
        .reserve()
        .map_err(PyRuntimeError::new_err)?;
    let (reader, request_id, output_sample_rate) = admit_sample_analysis(engine, id)?;

    let loader_tx = engine.loader_tx.clone();
    let active_tasks = engine.active_tasks.clone();
    let pad_request_ids = engine.pad_request_ids.clone();

    let task_guard = PadTaskGuard {
        id,
        task: BackgroundTaskKind::Analysis,
        active_tasks,
    };
    let shutdown = engine.cold_cancelled.clone();
    engine
        .cold_jobs
        .submit(reservation, move || {
            let _task_guard = task_guard;
            let cancelled = || {
                shutdown.load(Ordering::Acquire)
                    || !pad_request_matches(&pad_request_ids, id, request_id)
                    || reader.guard.as_ref().is_some_and(|guard| !guard.current())
            };

            let _ = loader_tx.send(LoaderEvent::TaskStarted {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
            });

            let stage = LoadProgressStage::Analyzing.stage_label().to_string();
            let _ = loader_tx.send(LoaderEvent::TaskProgress {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
                percent: 0.0,
                stage: stage.clone(),
            });

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                reader
                    .materialize(cold_jobs::PCM_LIMIT_BYTES, &cancelled)
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
                        let _ = loader_tx.send(LoaderEvent::TaskError {
                            id,
                            request_id,
                            task: BackgroundTaskKind::Analysis,
                            error: "Analysis superseded by another pad request".into(),
                        });
                        return;
                    }
                    let _ = loader_tx.send(LoaderEvent::TaskError {
                        id,
                        request_id,
                        task: BackgroundTaskKind::Analysis,
                        error,
                    });
                    return;
                }
            };

            if cancelled() {
                let _ = loader_tx.send(LoaderEvent::TaskError {
                    id,
                    request_id,
                    task: BackgroundTaskKind::Analysis,
                    error: "Analysis superseded by another pad request".into(),
                });
                return;
            }

            let _ = loader_tx.send(LoaderEvent::TaskProgress {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
                percent: 1.0,
                stage,
            });

            let _ = loader_tx.send(LoaderEvent::TaskSuccess {
                id,
                request_id,
                task: BackgroundTaskKind::Analysis,
                analysis: Some(analysis),
            });
        })
        .map_err(PyRuntimeError::new_err)?;

    Ok(request_id)
}
