//! One original map job on the existing bounded cold lane, including queued retirement.

use super::source_beatmap_job::MapInput;
use super::{JobState, OfflineAnalysisJob, OfflinePurpose};
use crate::audio_engine::AudioEngine;
use crate::audio_engine::cold_jobs::{Reservation, ReservationRetirementGuard};
use crate::audio_engine::source_beatmap::MAX_RECORD_BYTES;
use crate::audio_engine::source_beatmap_artifact::{MAX_RECEIPT_BYTES, StorePoint};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

const QUEUED: u8 = 1;
const RUNNING: u8 = 2;
const READY: u8 = 3;
const FAILED: u8 = 4;
const CANCELLED: u8 = 5;

enum AsyncMapInput {
    AcquireRaw {
        expected_generation: u64,
    },
    Validate(String),
    Store(String),
    Restore(String),
    Create {
        origin: f64,
        period: f64,
        expected_generation: Option<u64>,
    },
    Edit {
        receipt: String,
        operation: String,
    },
}

impl AsyncMapInput {
    fn borrowed(&self) -> MapInput<'_> {
        match self {
            Self::AcquireRaw {
                expected_generation,
            } => MapInput::AcquireRaw {
                expected_generation: *expected_generation,
            },
            Self::Validate(encoded) => MapInput::Validate(encoded),
            Self::Store(encoded) => MapInput::Store(encoded),
            Self::Restore(receipt) => MapInput::Restore(receipt),
            Self::Create {
                origin,
                period,
                expected_generation,
            } => MapInput::Create {
                origin: *origin,
                period: *period,
                expected_generation: *expected_generation,
            },
            Self::Edit { receipt, operation } => MapInput::Edit { receipt, operation },
        }
    }

    fn bounded(&self) -> bool {
        match self {
            Self::AcquireRaw {
                expected_generation,
            } => *expected_generation > 0,
            Self::Validate(encoded) | Self::Store(encoded) => encoded.len() <= MAX_RECORD_BYTES,
            Self::Restore(receipt) => receipt.len() <= MAX_RECEIPT_BYTES,
            Self::Create { origin, period, .. } => {
                origin.is_finite() && period.is_finite() && *period > 0.0
            }
            Self::Edit { receipt, operation } => {
                receipt.len() <= MAX_RECEIPT_BYTES
                    && operation.len()
                        <= crate::audio_engine::source_beatmap::editor::MAX_EDIT_BYTES
            }
        }
    }
}

/// These scalars observe one existing job; they own no source or lane admission.
pub(super) struct AsyncMapStatus {
    stage: AtomicU8,
    outcome: AtomicU8,
    detail: Mutex<Option<String>>,
    cleanup_done: AtomicBool,
    pool_retired: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
}

impl AsyncMapStatus {
    pub(super) fn shutdown_token(&self) -> Arc<AtomicBool> {
        self.shutdown.clone()
    }
    pub(super) fn shutdown_requested(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    pub(super) fn dictionary(&self, py: Python<'_>, cancelled: bool) -> PyResult<Py<PyDict>> {
        let terminal =
            self.cleanup_done.load(Ordering::Acquire) && self.pool_retired.load(Ordering::Acquire);
        let cancellation_requested = cancelled || self.shutdown_requested();
        let stage = if terminal {
            if cancellation_requested {
                CANCELLED
            } else {
                self.outcome.load(Ordering::Acquire)
            }
        } else {
            self.stage.load(Ordering::Acquire)
        };
        let label = match stage {
            QUEUED => "queued",
            RUNNING => "running",
            READY => "ready",
            FAILED => "failed",
            CANCELLED => "cancelled",
            _ => unreachable!("armed source map lifecycle has a bounded stage"),
        };
        let detail = self
            .detail
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let dict = PyDict::new(py);
        dict.set_item("stage", label)?;
        dict.set_item("terminal", terminal)?;
        dict.set_item("cancellation_requested", cancellation_requested)?;
        dict.set_item("detail", detail)?;
        Ok(dict.unbind())
    }
}

/// The queue closure owns this before publication. Its Drop also covers an
/// uncalled closure discarded by close_admission or a failed submit.
struct AsyncMapWork {
    retirement: Option<AsyncMapRetirement>,
    started: bool,
}

struct AsyncMapRetirement {
    state: Arc<JobState>,
    input: Option<AsyncMapInput>,
    reservation: Option<ReservationRetirementGuard>,
    status: Arc<AsyncMapStatus>,
    outcome: u8,
}

impl AsyncMapWork {
    fn arm(
        job: &OfflineAnalysisJob,
        input: AsyncMapInput,
        reservation: &Reservation,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let state = job
            .state
            .as_ref()
            .ok_or("offline map handle unavailable")?
            .clone();
        state.require_purpose(OfflinePurpose::SourceBeatmap)?;
        let transition = state
            .transitions
            .lock()
            .map_err(|_| "offline transition lock poisoned")?;
        if state.finished.load(Ordering::Acquire)
            || state.retirement_started.load(Ordering::Acquire)
            || state.preparing.load(Ordering::Acquire)
            || state.async_map.get().is_some()
        {
            return Err("source beatmap async admission is unavailable".into());
        }
        let retained = reservation.retain_until_retired();
        let status = Arc::new(AsyncMapStatus {
            stage: AtomicU8::new(QUEUED),
            outcome: AtomicU8::new(CANCELLED),
            detail: Mutex::new(None),
            cleanup_done: AtomicBool::new(false),
            pool_retired: retained.retirement_receipt(),
            shutdown,
        });
        state
            .async_map
            .set(status.clone())
            .map_err(|_| "source beatmap async admission already exists")?;
        // Mark queued ownership under the same transition as direct work. No
        // caller can abort/drop the real snapshot while its closure is queued.
        state.preparing.store(true, Ordering::Release);
        drop(transition);
        Ok(Self {
            retirement: Some(AsyncMapRetirement {
                state,
                input: Some(input),
                reservation: Some(retained),
                status,
                outcome: CANCELLED,
            }),
            started: false,
        })
    }

    fn run(
        mut self,
        after_verify: impl FnOnce(),
        mut observe: impl FnMut(StorePoint, &Path) -> Result<(), String>,
    ) {
        self.started = true;
        let work = self
            .retirement
            .as_mut()
            .expect("original map closure owns work");
        debug_assert!(work.reservation.as_ref().unwrap().is_running());
        work.status.stage.store(RUNNING, Ordering::Release);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            work.state.prepare_source_beatmap_input_body(
                work.input
                    .as_ref()
                    .expect("queued map input remains owned")
                    .borrowed(),
                after_verify,
                &mut observe,
            )?;
            work.state.preparing.store(false, Ordering::Release);
            work.state.finish_source_beatmap_body(false)
        }))
        .unwrap_or_else(|_| Err("source beatmap worker failed".into()));
        let cancelled = work.state.cancelled();
        match result {
            Ok(true) if !cancelled => work.outcome = READY,
            Ok(_) => work.outcome = CANCELLED,
            Err(error) => {
                work.outcome = if cancelled { CANCELLED } else { FAILED };
                // A fixed UTF-8 bound, not a copy of the input or panic payload.
                *work
                    .status
                    .detail
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(error.chars().take(256).collect());
            }
        }
        // Drop below performs all running/panic retirement synchronously on
        // THIS worker, before its original running reservation is released.
    }
}

impl Drop for AsyncMapWork {
    fn drop(&mut self) {
        let Some(work) = self.retirement.take() else {
            return;
        };
        let running = work.reservation.as_ref().unwrap().is_running();
        if self.started || running {
            // A running guard must not outlive its worker body: the lane may
            // claim its next job only after active accounting can be released.
            debug_assert!(running);
            drop(work);
        } else {
            debug_assert!(!work.reservation.as_ref().unwrap().is_running());
            let _ = work.state.cancel_request();
            // Only never-started work may retire off-thread. It retains the
            // SAME non-running lane admission throughout actual owner cleanup.
            // On spawn failure the uncalled closure drops work synchronously;
            // its destructor still retires every real owner exactly once.
            let _ = std::thread::Builder::new()
                .name("source-map-retirement".into())
                .spawn(move || drop(work));
        }
    }
}

impl Drop for AsyncMapRetirement {
    fn drop(&mut self) {
        self.state.retirement_started.store(true, Ordering::Release);
        // This is the unique armed owner after actual work/unwind, or the
        // unique discarded queued owner. Recover poisoned locks for cleanup.
        self.state.drop_pcm();
        self.state.discard_prepared_map();
        drop(self.input.take());
        self.state.preparing.store(false, Ordering::Release);
        self.state.finished.store(true, Ordering::Release);
        self.state.release_reservation();
        drop(self.reservation.take());
        self.status.outcome.store(self.outcome, Ordering::Release);
        self.status.cleanup_done.store(true, Ordering::Release);
        // Status additionally waits for the original lane's final reservation
        // receipt, which can occur only after its accounting decrement.
    }
}

impl AudioEngine {
    pub(in crate::audio_engine) fn begin_source_beatmap_admitted(
        &self,
        id: usize,
        raw_review: Option<crate::audio_engine::source_beatmap_raw::RawReviewLease>,
    ) -> PyResult<OfflineAnalysisJob> {
        use crate::audio_engine::{NUM_SAMPLES, complete_context, has_active_task_for_id};
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if self
            .loading_sample_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?
            .contains(&id)
            || self.cold_loading[id].load(Ordering::Acquire) != 0
        {
            return Err(PyValueError::new_err("sample is currently loading"));
        }
        if has_active_task_for_id(
            &*self
                .active_tasks
                .lock()
                .map_err(|_| PyRuntimeError::new_err("task lock poisoned"))?,
            self.active_task_cell(id).map_err(PyRuntimeError::new_err)?,
        ) {
            return Err(PyValueError::new_err("sample task already running"));
        }
        let sample = self
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?
            .get(id)
            .and_then(Clone::clone)
            .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
        let reader = complete_context::CompleteSourceReader::capture(self, id, sample)
            .map_err(PyRuntimeError::new_err)?;
        let (generation, rate) = self
            .loaded_source_generations
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?[id];
        self.offline_jobs
            .begin_source_beatmap(
                id,
                reader,
                (generation, rate),
                super::OfflineRequestOwner {
                    request_ids: self.pad_request_ids.clone(),
                    prepared_epoch: self
                        .prepared_source_epoch(id)
                        .map_err(PyRuntimeError::new_err)?
                        .clone(),
                },
                self.loader_tx.clone(),
                raw_review,
            )
            .map_err(PyRuntimeError::new_err)
    }

    pub(in crate::audio_engine) fn begin_source_beatmap_raw_async_with(
        &self,
        sample_id: usize,
        expected_generation: u64,
        after_verify: impl FnOnce() + Send + 'static,
    ) -> PyResult<OfflineAnalysisJob> {
        self.queue_source_beatmap_input(
            sample_id,
            AsyncMapInput::AcquireRaw {
                expected_generation,
            },
            after_verify,
            |_, _| Ok(()),
        )
    }

    #[cfg(test)]
    pub(in crate::audio_engine) fn begin_source_beatmap_raw_async_with_hooks(
        &self,
        sample_id: usize,
        expected_generation: u64,
        after_verify: impl FnOnce() + Send + 'static,
        observe: impl FnMut(StorePoint, &Path) -> Result<(), String> + Send + 'static,
    ) -> PyResult<OfflineAnalysisJob> {
        self.queue_source_beatmap_input(
            sample_id,
            AsyncMapInput::AcquireRaw {
                expected_generation,
            },
            after_verify,
            observe,
        )
    }

    #[cfg(test)]
    pub(in crate::audio_engine) fn begin_source_beatmap_raw_admission_fault_for_test(
        &self,
        sample_id: usize,
        expected_generation: u64,
        after_arm: bool,
    ) -> PyResult<OfflineAnalysisJob> {
        self.queue_source_beatmap_input_with_admission_hook(
            sample_id,
            AsyncMapInput::AcquireRaw {
                expected_generation,
            },
            || {},
            |_, _| Ok(()),
            move |engine, job, armed| {
                if armed == after_arm {
                    if after_arm {
                        engine.cold_jobs.close_admission();
                    } else {
                        job.state().preparing.store(true, Ordering::Release);
                    }
                }
            },
        )
    }
    pub(in crate::audio_engine) fn begin_source_beatmap_create_async_with(
        &self,
        sample_id: usize,
        origin: f64,
        period: f64,
        expected_generation: Option<u64>,
        after_verify: impl FnOnce() + Send + 'static,
    ) -> PyResult<OfflineAnalysisJob> {
        self.queue_source_beatmap_input(
            sample_id,
            AsyncMapInput::Create {
                origin,
                period,
                expected_generation,
            },
            after_verify,
            |_, _| Ok(()),
        )
    }

    pub(in crate::audio_engine) fn begin_source_beatmap_edit_async_with(
        &self,
        sample_id: usize,
        receipt: String,
        operation: String,
        after_verify: impl FnOnce() + Send + 'static,
    ) -> PyResult<OfflineAnalysisJob> {
        crate::audio_engine::source_beatmap::editor::validate_intent(&operation)
            .map_err(PyValueError::new_err)?;
        self.queue_source_beatmap_input(
            sample_id,
            AsyncMapInput::Edit { receipt, operation },
            after_verify,
            |_, _| Ok(()),
        )
    }

    /// Same production admission/queue path with one local verification barrier.
    /// Production passes an empty closure; tests never bypass source verification.
    pub(in crate::audio_engine) fn begin_source_beatmap_async_with(
        &self,
        sample_id: usize,
        record_json: String,
        after_verify: impl FnOnce() + Send + 'static,
    ) -> PyResult<OfflineAnalysisJob> {
        self.queue_source_beatmap_input(
            sample_id,
            AsyncMapInput::Validate(record_json),
            after_verify,
            |_, _| Ok(()),
        )
    }

    pub(in crate::audio_engine) fn begin_source_beatmap_store_async_with(
        &self,
        sample_id: usize,
        record_json: String,
        after_verify: impl FnOnce() + Send + 'static,
    ) -> PyResult<OfflineAnalysisJob> {
        self.queue_source_beatmap_input(
            sample_id,
            AsyncMapInput::Store(record_json),
            after_verify,
            |_, _| Ok(()),
        )
    }

    pub(in crate::audio_engine) fn begin_source_beatmap_restore_async_with(
        &self,
        sample_id: usize,
        receipt_json: String,
        after_verify: impl FnOnce() + Send + 'static,
    ) -> PyResult<OfflineAnalysisJob> {
        self.queue_source_beatmap_input(
            sample_id,
            AsyncMapInput::Restore(receipt_json),
            after_verify,
            |_, _| Ok(()),
        )
    }

    #[cfg(test)]
    pub(in crate::audio_engine) fn begin_source_beatmap_store_async_with_hooks(
        &self,
        sample_id: usize,
        record_json: String,
        after_verify: impl FnOnce() + Send + 'static,
        observe: impl FnMut(StorePoint, &Path) -> Result<(), String> + Send + 'static,
    ) -> PyResult<OfflineAnalysisJob> {
        self.queue_source_beatmap_input(
            sample_id,
            AsyncMapInput::Store(record_json),
            after_verify,
            observe,
        )
    }

    fn queue_source_beatmap_input(
        &self,
        sample_id: usize,
        input: AsyncMapInput,
        after_verify: impl FnOnce() + Send + 'static,
        observe: impl FnMut(StorePoint, &Path) -> Result<(), String> + Send + 'static,
    ) -> PyResult<OfflineAnalysisJob> {
        self.queue_source_beatmap_input_with_admission_hook(
            sample_id,
            input,
            after_verify,
            observe,
            |_, _, _| {},
        )
    }

    /// Production has a monomorphized no-op; tests fault the exact original
    /// capture/arm/submit path without manufacturing a second source admission.
    fn queue_source_beatmap_input_with_admission_hook(
        &self,
        sample_id: usize,
        input: AsyncMapInput,
        after_verify: impl FnOnce() + Send + 'static,
        observe: impl FnMut(StorePoint, &Path) -> Result<(), String> + Send + 'static,
        admission: impl Fn(&Self, &OfflineAnalysisJob, bool),
    ) -> PyResult<OfflineAnalysisJob> {
        if !input.bounded() {
            return Err(PyValueError::new_err(
                "source beatmap input exceeds byte limit",
            ));
        }
        // Reserve the existing lane BEFORE offline/source admission. Refusal
        // therefore changes neither source counters nor the offline busy slot.
        let raw_review = if matches!(input, AsyncMapInput::AcquireRaw { .. }) {
            Some(
                crate::audio_engine::source_beatmap_raw::RawReviewLease::reserve()
                    .map_err(PyRuntimeError::new_err)?,
            )
        } else {
            None
        };
        let reservation = self.cold_jobs.reserve().map_err(PyRuntimeError::new_err)?;
        let job = self.begin_source_beatmap_admitted(sample_id, raw_review)?;
        admission(self, &job, false);
        let work = AsyncMapWork::arm(&job, input, &reservation, self.cold_cancelled.clone())
            .map_err(PyRuntimeError::new_err)?;
        admission(self, &job, true);
        self.cold_jobs
            .submit(reservation, move || work.run(after_verify, observe))
            .map_err(PyRuntimeError::new_err)?;
        Ok(job)
    }
}
