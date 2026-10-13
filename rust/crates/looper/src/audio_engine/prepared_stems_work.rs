//! Owned preparation for the two existing prepared-stem publication paths.

use super::{
    AudioEngine, PreparedSourceTicket, ProjectAssetLease, cold_jobs,
    native_owned_work::CompleteSampleWork,
    prepared_source::{enqueue_current_prepared_stems_with_owner, validate_prepared_ticket},
    project_assets, stem_cache,
    stem_pair_preparation::{PreparedStemPair, StemPairPreparationWork},
};
use crate::messages::{ControlMessage, PreparedStemSet, SampleBuffer};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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

impl PreparedStemsWork {
    pub(super) fn capture(
        engine: &AudioEngine,
        id: usize,
        source_version: String,
        cache_dir: String,
        ticket: &PreparedSourceTicket,
    ) -> PyResult<Self> {
        if id >= super::constants::NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if source_version.trim().is_empty() {
            return Err(PyValueError::new_err("source_version must not be empty"));
        }
        stem_cache::project_stem_cache_dir(&cache_dir).map_err(PyValueError::new_err)?;
        let sample = {
            let requests = engine
                .pad_request_ids
                .lock()
                .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
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
                &engine.prepared_source_epochs[id],
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
        let preparation = if paired {
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
            let source =
                CompleteSampleWork::capture(engine, id, &sample).map_err(PyValueError::new_err)?;
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
        lane.execute_owned(
            || shutdown.load(std::sync::atomic::Ordering::Acquire) || !publication.current(),
            move || self.prepare_inner(),
        )
    }

    fn prepare_inner(self) -> Result<PreparedStemsPublication, String> {
        let Self {
            source_version,
            cache_dir,
            preparation,
            ..
        } = self;
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
                let (mut stems, files) = if let Some(shared) = shared {
                    (shared, None)
                } else {
                    let files = project_assets::stem_readers::seal_stem_artifacts(&path)
                        .map_err(|error| error.to_string())?;
                    #[cfg(test)]
                    super::native_owned_work::observe(
                        super::native_owned_work::WorkPoint::LegacyStemFiles,
                    );
                    let peak =
                        stem_cache::admitted_stem_pcm_bytes(sample.frame_count(), sample.channels)?
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
                    let complete = source.materialize(cold_jobs::PCM_LIMIT_BYTES)?;
                    let stems = stem_cache::prepare_stem_buffers_from_cache(
                        &source_version,
                        &complete,
                        rate,
                        &cache_dir,
                    )?;
                    (stems, Some(files))
                };
                stems = stems.window_for(&sample)?;
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
        match self.publication {
            PreparedPublication::Pair(pair) => {
                engine.publish_stem_pair_with_producer(&pair, ticket, producer)
            }
            PreparedPublication::Legacy(prepared) => {
                let LegacyPublication {
                    stems,
                    path,
                    _lease,
                } = *prepared;
                enqueue_current_prepared_stems_with_owner(
                    engine,
                    producer,
                    ticket,
                    &self.source_version,
                    stems,
                    (self.cache_dir.into(), path),
                )
            }
        }
    }
}
