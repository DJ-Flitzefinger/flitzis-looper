//! Owned preparation for the two existing prepared-stem publication paths.

use super::{
    AudioEngine, PreparedSourceTicket, ProjectAssetLease, cold_jobs,
    native_owned_work::CompleteSampleWork,
    prepared_source::{try_enqueue_prepared_stems, validate_prepared_ticket},
    project_assets, stem_cache,
    stem_pair_preparation::{PreparedStemPair, StemPairPreparationWork},
};
use crate::messages::{ControlMessage, PreparedStemSet, SampleBuffer};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[cfg(test)]
#[path = "prepared_stems_content_tests.rs"]
mod content_tests;

enum Preparation {
    Pair(Box<StemPairPreparationWork>),
    Legacy(Box<LegacyPreparation>),
}

struct LegacyPreparation {
    sample: SampleBuffer,
    rate: u32,
    path: PathBuf,
    assets: Arc<project_assets::ProjectAssets>,
    source: CompleteSampleWork,
    publication: super::prepared_source::PreparedSourcePermit,
    lease: ProjectAssetLease,
    resident: super::resident_proposal::ResidentReadContext,
}

pub(super) struct PreparedStemsWork {
    restore_scope: Option<super::audio_output_change::RestoreScope>,
    local_cancelled: Option<Arc<std::sync::atomic::AtomicBool>>,
    source_version: String,
    cache_dir: String,
    preparation: Preparation,
    lane: super::cold_jobs::ColdJobsHandle,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    publication: super::prepared_source::PreparedSourcePermit,
}

enum PreparedPublication {
    Pair(Box<PreparedStemPair>),
    Legacy(Box<LegacyPublication>),
}

struct LegacyPublication {
    stems: PreparedStemSet,
    path: PathBuf,
    _lease: ProjectAssetLease,
}

pub(super) struct PreparedStemsPublication {
    source_version: String,
    cache_dir: String,
    publication: PreparedPublication,
}

/// One bounded control-owned result retains the original payload through a placement gap.
pub(super) struct PendingPreparedStems {
    prepared: PreparedStemsPublication,
    ticket: PreparedSourceTicket,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
}

pub(super) fn has_pending(engine: &AudioEngine, ticket: &PreparedSourceTicket) -> PyResult<bool> {
    let cell = ticket
        .publication
        .content_key()
        .ok_or_else(|| PyValueError::new_err("prepared Native key unavailable"))?
        .cell();
    let pending = engine
        .pending_prepared_stems
        .lock()
        .map_err(|_| PyRuntimeError::new_err("prepared result lock poisoned"))?;
    if let Some(pending) = &pending[cell.index()] {
        if !pending
            .ticket
            .publication
            .same_publication(&ticket.publication)
        {
            return Err(PyRuntimeError::new_err(
                "another prepared result still owns this content cell",
            ));
        }
        return Ok(true);
    }
    Ok(false)
}

pub(super) fn reconcile(engine: &AudioEngine) -> PyResult<()> {
    for cell in 0..super::constants::NUM_SAMPLES {
        let pending = engine
            .pending_prepared_stems
            .lock()
            .map_err(|_| PyRuntimeError::new_err("prepared result lock poisoned"))?[cell]
            .take();
        let Some(pending) = pending else {
            continue;
        };
        let PendingPreparedStems {
            prepared,
            ticket,
            producer,
        } = *pending;
        if engine
            .cold_cancelled
            .load(std::sync::atomic::Ordering::Acquire)
            || !ticket.publication.current()
        {
            ticket.publication.mark_rejected();
            continue;
        }
        if prepared.finish(engine, &ticket, &producer).is_err() {
            ticket.publication.mark_rejected();
        }
    }
    Ok(())
}

pub(super) fn cancel_all(engine: &AudioEngine) -> PyResult<()> {
    let retired = {
        let mut pending = engine
            .pending_prepared_stems
            .lock()
            .map_err(|_| PyRuntimeError::new_err("prepared result lock poisoned"))?;
        pending
            .iter_mut()
            .filter_map(Option::take)
            .collect::<Vec<_>>()
    };
    for pending in retired {
        pending.ticket.publication.mark_rejected();
    }
    Ok(())
}

impl PreparedStemsWork {
    pub(super) fn capture(
        engine: &AudioEngine,
        id: usize,
        source_version: String,
        cache_dir: String,
        ticket: &PreparedSourceTicket,
    ) -> PyResult<Self> {
        Self::capture_with_mode(engine, id, source_version, cache_dir, ticket, false)
    }

    pub(super) fn capture_read_existing(
        engine: &AudioEngine,
        id: usize,
        source_version: String,
        cache_dir: String,
        ticket: &PreparedSourceTicket,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> PyResult<Self> {
        let mut work =
            Self::capture_with_mode(engine, id, source_version, cache_dir, ticket, true)?;
        work.local_cancelled = Some(cancelled);
        Ok(work)
    }

    fn capture_with_mode(
        engine: &AudioEngine,
        id: usize,
        source_version: String,
        cache_dir: String,
        ticket: &PreparedSourceTicket,
        read_existing: bool,
    ) -> PyResult<Self> {
        if id >= super::constants::NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if source_version.trim().is_empty() {
            return Err(PyValueError::new_err("source_version must not be empty"));
        }
        stem_cache::project_stem_cache_dir(&cache_dir).map_err(PyValueError::new_err)?;
        if id != ticket.id {
            return Err(PyValueError::new_err(
                "prepared work requires its original ticket selector",
            ));
        }
        let id = match ticket
            .publication
            .control_placement(engine)
            .map_err(PyValueError::new_err)?
        {
            super::content_placement::PlacementObservation::Current(current) => current.slot(),
            super::content_placement::PlacementObservation::Pending => {
                return Err(PyRuntimeError::new_err(
                    super::prepared_source::STEM_PLACEMENT_PENDING,
                ));
            }
            _ => {
                return Err(PyValueError::new_err(
                    "prepared source content key is stale",
                ));
            }
        };
        let sample = {
            let requests = engine
                .pad_request_ids
                .lock()
                .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
            let source_epoch = engine
                .prepared_source_epoch(id)
                .map_err(PyRuntimeError::new_err)?;
            let sample = engine
                .sample_cache
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire sample cache lock"))?
                .get(id)
                .and_then(Clone::clone)
                .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
            validate_prepared_ticket(
                ticket,
                id,
                &source_version,
                requests[id],
                source_epoch,
                &sample,
            )
            .map_err(PyValueError::new_err)?;
            sample
        };
        let paired = engine
            .cold_leases
            .lock()
            .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?[id]
            .is_some();
        let preparation = if paired && !read_existing {
            Preparation::Pair(Box::new(
                engine
                    .capture_stem_pair_work(
                        &engine.project_assets_root()?,
                        id,
                        &source_version,
                        ticket,
                    )
                    .map_err(PyValueError::new_err)?,
            ))
        } else {
            let (_, path) = project_assets::owned_path(
                &engine.project_assets_root()?,
                std::path::Path::new(&cache_dir),
            )
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
            let lease = engine.acquire_project_asset_lease(cache_dir.clone())?;
            let source = CompleteSampleWork::capture_for_legacy(engine, id, &sample)
                .map_err(PyValueError::new_err)?;
            Preparation::Legacy(Box::new(LegacyPreparation {
                sample,
                rate: ticket.sample_rate_hz,
                path,
                assets: engine.project_assets.clone(),
                source,
                publication: ticket.publication.clone(),
                lease,
                resident: super::resident_proposal::ResidentReadContext::reader(
                    engine.project_assets.clone(),
                    engine.resident_account.clone(),
                ),
            }))
        };
        Ok(Self {
            restore_scope: super::audio_output_change::RestoreScope::current(),
            local_cancelled: None,
            source_version,
            cache_dir,
            preparation,
            lane: engine.cold_jobs.handle(),
            cancelled: engine.cold_cancelled.clone(),
            publication: ticket.publication.clone(),
        })
    }

    pub(super) fn prepare(self) -> Result<PreparedStemsPublication, String> {
        let lane = self.lane.clone();
        let shutdown = self.cancelled.clone();
        let publication = self.publication.clone();
        let local = self.local_cancelled.clone();
        super::audio_output_change::RestoreScope::run(self.restore_scope, || {
            lane.execute_heavy_owned_until_terminal(
                || {
                    shutdown.load(std::sync::atomic::Ordering::Acquire)
                        || !publication.current()
                        || local.as_ref().is_some_and(|cancelled| {
                            cancelled.load(std::sync::atomic::Ordering::Acquire)
                        })
                },
                move || self.prepare_inner(),
            )
        })
    }

    fn prepare_inner(self) -> Result<PreparedStemsPublication, String> {
        let Self {
            source_version,
            cache_dir,
            preparation,
            cancelled: shutdown,
            local_cancelled,
            publication: source_publication,
            ..
        } = self;
        let cancelled = || {
            shutdown.load(std::sync::atomic::Ordering::Acquire)
                || !source_publication.current()
                || local_cancelled
                    .as_ref()
                    .is_some_and(|local| local.load(std::sync::atomic::Ordering::Acquire))
        };
        let publication = match preparation {
            Preparation::Pair(work) => {
                PreparedPublication::Pair(Box::new(work.prepare(&cache_dir, true, None, None)?))
            }
            Preparation::Legacy(work) => {
                let LegacyPreparation {
                    sample,
                    rate,
                    path,
                    assets,
                    source,
                    publication,
                    lease,
                    resident,
                } = *work;
                let shared = assets
                    .shared_stems(&path, &sample, &source_version, rate)
                    .map_err(|error| error.to_string())?;
                let mut reused = vec![&sample];
                if let Some(shared) = shared.as_ref() {
                    reused.extend(shared.stems.iter());
                }
                let bytes = if shared.is_some() {
                    0
                } else {
                    sample
                        .samples
                        .len()
                        .checked_mul(16)
                        .ok_or("resident legacy component extent overflow")?
                };
                let mut reservation = resident
                    .reserve(bytes, &reused, 5)
                    .map_err(|error| error.to_string())?;
                reservation
                    .reserve_reader_slots(usize::from(shared.is_none()))
                    .map_err(|error| error.to_string())?;
                let mut transient = None;
                let (mut stems, files) = if let Some(shared) = shared {
                    (shared, None)
                } else {
                    let files = project_assets::stem_readers::seal_stem_artifacts(&path)
                        .map_err(|error| error.to_string())?;
                    #[cfg(test)]
                    super::native_owned_work::observe(
                        super::native_owned_work::WorkPoint::LegacyStemFiles,
                    );
                    let peak = stem_cache::legacy_preparation_peak(
                        sample.frame_count(),
                        sample.channels,
                        rate,
                        &path,
                        &cancelled,
                    )?
                    .checked_add(
                        sample
                            .samples
                            .len()
                            .checked_mul(4)
                            .ok_or("resident stem budget overflow")?,
                    )
                    .ok_or("resident stem budget overflow")?;
                    if peak > cold_jobs::PCM_LIMIT_BYTES {
                        return Err("complete stem preparation with resident window exceeds transient PCM admission".into());
                    }
                    transient = Some(
                        resident
                            .account
                            .reserve_transient(peak)
                            .map_err(|error| error.to_string())?,
                    );
                    let complete =
                        source.materialize_cancellable(cold_jobs::PCM_LIMIT_BYTES, &cancelled)?;
                    let stems = stem_cache::prepare_stem_buffers_from_cache_cancellable(
                        &source_version,
                        &complete,
                        rate,
                        &cache_dir,
                        source.source_clock(),
                        &cancelled,
                    )?;
                    (stems, Some(files))
                };
                stems = stems.window_for(&sample)?;
                if cancelled() {
                    return Err("legacy stem preparation cancelled before publication".into());
                }
                stems.publication = publication.clone();
                stems.accepted_timing = publication.accepted_projection();
                let mut views = vec![&sample];
                views.extend(stems.stems.iter());
                reservation
                    .commit(&views)
                    .map_err(|error| error.to_string())?;
                if let Some(files) = files {
                    assets
                        .retain_shared_stems_reserved(
                            path.clone(),
                            &stems,
                            &sample,
                            &source_version,
                            files,
                            &mut reservation,
                        )
                        .map_err(|error| error.to_string())?;
                }
                drop(transient);
                PreparedPublication::Legacy(Box::new(LegacyPublication {
                    stems,
                    path,
                    _lease: lease,
                }))
            }
        };
        Ok(PreparedStemsPublication {
            source_version,
            cache_dir,
            publication,
        })
    }
}

impl PreparedStemsPublication {
    pub(super) fn finish(
        self,
        engine: &AudioEngine,
        ticket: &PreparedSourceTicket,
        producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ) -> PyResult<()> {
        let (mut stems, registration, source_version) = match &self.publication {
            PreparedPublication::Pair(pair) => {
                if !ticket.sample.same_window(&pair.reference) {
                    return Err(PyValueError::new_err("prepared pair window is stale"));
                }
                let stems = pair
                    .stems
                    .clone()
                    .ok_or_else(|| PyValueError::new_err("pair has no live component request"))?;
                let pair_owner = super::resident_relocation::StemPairOwner {
                    root: pair.root.clone(),
                    descriptor_reference: pair.descriptor_reference.clone(),
                    source_lease: pair.source_lease.clone(),
                    reader: pair.pair_reader.clone(),
                };
                (
                    stems,
                    (
                        pair.wav_path.clone(),
                        pair.wav_path.clone(),
                        Some(pair_owner),
                    ),
                    &pair.source_version,
                )
            }
            PreparedPublication::Legacy(prepared) => (
                prepared.stems.clone(),
                (self.cache_dir.clone().into(), prepared.path.clone(), None),
                &self.source_version,
            ),
        };
        stems.publication = ticket.publication.clone();
        stems.accepted_timing = ticket.publication.accepted_projection();
        if try_enqueue_prepared_stems(
            engine,
            producer,
            ticket,
            source_version,
            stems,
            Some(registration),
        )? {
            return Ok(());
        }
        let cell = ticket
            .publication
            .content_key()
            .ok_or_else(|| PyValueError::new_err("prepared Native key unavailable"))?
            .cell();
        let mut pending = engine
            .pending_prepared_stems
            .lock()
            .map_err(|_| PyRuntimeError::new_err("prepared result lock poisoned"))?;
        if pending[cell.index()].is_some() {
            return Err(PyRuntimeError::new_err(
                "prepared result cell is already owned",
            ));
        }
        pending[cell.index()] = Some(Box::new(PendingPreparedStems {
            prepared: self,
            ticket: ticket.clone(),
            producer: producer.clone(),
        }));
        Ok(())
    }
}
