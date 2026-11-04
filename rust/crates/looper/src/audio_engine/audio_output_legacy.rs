//! Read existing legacy files during output restoration, without pair migration.

use super::audio_output_change::{self, RestoreScope};
use super::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use super::prepared_source::PreparedSourcePermit;
use super::prepared_stems_work::{PreparedStemsPublication, PreparedStemsWork};
use super::{AudioEngine, PreparedSourceTicket};
use crate::messages::{ControlMessage, STEM_COMPONENT_MASK, StemMixMode};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(test)]
#[path = "audio_output_legacy_tests.rs"]
mod tests;

#[pyclass(frozen)]
pub struct ExistingLegacyStemWork {
    work: Mutex<Option<PreparedStemsWork>>,
    cancelled: Arc<AtomicBool>,
}

#[pymethods]
impl ExistingLegacyStemWork {
    /// Prepare on the existing worker/Cold lane. No Engine/Python borrow or
    /// model invocation is retained; the captured narrow scope travels with work.
    pub fn prepare(&self, py: Python<'_>) -> PyResult<PreparedExistingLegacyStems> {
        let work = self
            .work
            .lock()
            .map_err(|_| PyRuntimeError::new_err("legacy work lock poisoned"))?
            .take()
            .ok_or_else(|| PyRuntimeError::new_err("legacy work was already consumed"))?;
        let prepared = py
            .detach(|| work.prepare())
            .map_err(PyRuntimeError::new_err)?;
        Ok(PreparedExistingLegacyStems {
            prepared: Mutex::new(Some(prepared)),
        })
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

#[pyclass(frozen)]
pub struct PreparedExistingLegacyStems {
    prepared: Mutex<Option<PreparedStemsPublication>>,
}

pub(super) fn capture(
    engine: &AudioEngine,
    epoch: u64,
    id: usize,
    cache_dir: String,
    ticket: &PreparedSourceTicket,
) -> PyResult<ExistingLegacyStemWork> {
    require_scope(engine, epoch)?;
    let (_, path) = super::project_assets::owned_path(
        &engine.project_assets_root()?,
        std::path::Path::new(&cache_dir),
    )
    .map_err(|error| PyValueError::new_err(error.to_string()))?;
    audio_output_change::validate_legacy_path(engine, epoch, id, &path)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let work = PreparedStemsWork::capture_read_existing(
        engine,
        id,
        ticket.source_version.clone(),
        cache_dir,
        ticket,
        cancelled.clone(),
    )?;
    Ok(ExistingLegacyStemWork {
        work: Mutex::new(Some(work)),
        cancelled,
    })
}

pub(super) fn publish(
    engine: &AudioEngine,
    epoch: u64,
    prepared: &PreparedExistingLegacyStems,
    ticket: &PreparedSourceTicket,
) -> PyResult<()> {
    require_scope(engine, epoch)?;
    let producer = engine.control_producer()?;
    {
        let guard = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("control producer lock poisoned"))?;
        if !engine
            .input_runtime_ownership
            .output_publication_live(&guard)
        {
            return Err(PyRuntimeError::new_err(
                "output closed before existing legacy publication",
            ));
        }
    }
    let payload = prepared
        .prepared
        .lock()
        .map_err(|_| PyRuntimeError::new_err("legacy result lock poisoned"))?
        .take()
        .ok_or_else(|| PyRuntimeError::new_err("legacy result was already published"))?;
    payload.finish(engine, ticket, producer)
}

fn require_scope(engine: &AudioEngine, epoch: u64) -> PyResult<()> {
    let expected = audio_output_change::restore_scope(engine, epoch)?;
    if RestoreScope::current() != Some(expected) {
        return Err(PyRuntimeError::new_err(
            "existing legacy restore requires its original scoped output owner",
        ));
    }
    Ok(())
}

pub(crate) struct LegacyStemObservation {
    pub(super) id: usize,
    pub(super) epoch: u64,
    pub(super) ownership: Arc<InputRuntimeOwnership>,
    pub(super) publication: PreparedSourcePermit,
    pub(super) binding: InputPadBinding,
    pub(super) generation: u64,
    pub(super) rate: u32,
    pub(super) source_hash: u64,
    pub(super) mode: StemMixMode,
    pub(super) mask: u8,
    status: AtomicU8,
    effect_revision: AtomicU64,
}

impl std::fmt::Debug for LegacyStemObservation {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("LegacyStemObservation")
            .field("id", &self.id)
            .field("status", &self.status.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl LegacyStemObservation {
    pub(super) fn reject(&self) {
        let _ = self
            .status
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire);
    }

    pub(super) fn settle(&self, accepted: bool) {
        if !accepted {
            self.reject();
            return;
        }
        let Some(key) = self.publication.content_key() else {
            self.reject();
            return;
        };
        let revision = self.ownership.source_observation_revision(key.cell());
        if revision == u64::MAX {
            self.reject();
            return;
        }
        self.effect_revision.store(revision, Ordering::Release);
        let _ = self
            .status
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
    }

    pub(super) fn current(&self) -> bool {
        let Some(key) = self.publication.content_key() else {
            return false;
        };
        self.status.load(Ordering::Acquire) == 1
            && !self.ownership.output_unavailable.load(Ordering::Acquire)
            && self.publication.current()
            && self
                .ownership
                .binding_source_generation(self.id, self.binding)
                == Some(self.generation)
            && self.effect_revision.load(Ordering::Acquire)
                == self.ownership.source_observation_revision(key.cell())
    }
}

#[pyclass(frozen)]
pub struct LegacyStemRestoreReceipt {
    observation: Arc<LegacyStemObservation>,
}

#[pymethods]
impl LegacyStemRestoreReceipt {
    pub fn status(&self) -> &'static str {
        match self.observation.status.load(Ordering::Acquire) {
            0 => "pending",
            1 => "accepted",
            _ => "rejected",
        }
    }
    pub fn is_current(&self) -> bool {
        self.observation.current()
    }
    pub fn cancel(&self) {
        self.observation.reject();
    }
}

pub(super) fn observe(
    engine: &AudioEngine,
    epoch: u64,
    ticket: &PreparedSourceTicket,
    mode: &str,
    mask: u8,
) -> PyResult<LegacyStemRestoreReceipt> {
    require_scope(engine, epoch)?;
    let mode = super::parse_stem_mix_mode(mode)
        .ok_or_else(|| PyValueError::new_err("invalid stem mode"))?;
    if mask & !STEM_COMPONENT_MASK != 0
        || ticket.publication_status() != "accepted"
        || !ticket.is_current()
    {
        return Err(PyValueError::new_err(
            "existing legacy observation requires its original accepted publication",
        ));
    }
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut producer = engine
        .control_producer()?
        .lock()
        .map_err(|_| PyRuntimeError::new_err("control producer lock poisoned"))?;
    if !engine
        .input_runtime_ownership
        .output_publication_live(&producer)
    {
        return Err(PyRuntimeError::new_err(
            "output closed before legacy mode observation",
        ));
    }
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "[resident_stem_interest_busy] legacy output observation queue full",
        ));
    }
    let observation = Arc::new(LegacyStemObservation {
        id: ticket.id,
        epoch,
        ownership: engine.input_runtime_ownership.clone(),
        publication: ticket.publication.clone(),
        binding: ticket.admission.binding,
        generation: ticket.admission.source_generation,
        rate: ticket.sample_rate_hz,
        source_hash: super::stem_cache::source_version_hash(&ticket.source_version),
        mode,
        mask,
        status: AtomicU8::new(0),
        effect_revision: AtomicU64::new(0),
    });
    audio_output_change::remember_legacy_selection(engine, epoch, ticket.id, observation.clone())?;
    if producer
        .push(ControlMessage::ObserveLegacyStemRestore(
            observation.clone(),
        ))
        .is_err()
    {
        observation.reject();
        return Err(PyRuntimeError::new_err(
            "[resident_stem_interest_busy] legacy output observation queue full",
        ));
    }
    Ok(LegacyStemRestoreReceipt { observation })
}
