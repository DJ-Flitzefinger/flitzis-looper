//! Ordinary off-thread complete-pair preparation and independent pad publication.

use super::AudioEngine;
use super::cold_jobs::PCM_LIMIT_BYTES;
use super::complete_context::CompleteSourceReader;
use super::material_migration::{PreparedMigrationMaterial, prepare_material};
use super::material_migration_stems::copy_stem_generation;
use super::material_paths::{self, AssetKind};
use super::prepared_source::{
    PreparedSourcePermit, PreparedSourceTicket, enqueue_current_prepared_pair_with_owner,
    validate_prepared_ticket,
};
use super::stem_cache::source_version_hash;
use super::stem_pair::{VerifiedStemPair, open_verified_pair, prepare_complete_pair_with_reader};
use crate::messages::{ControlMessage, PreparedStemSet, SampleBuffer};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Disk selection and temporary component readers; no files enter the callback.
#[pyclass(frozen)]
pub struct PreparedStemPair {
    selection: String,
    wav_path: PathBuf,
    source_version: String,
    reference: SampleBuffer,
    _identity: Arc<[u8; 32]>,
    stems: Option<PreparedStemSet>,
    assets: Arc<super::project_assets::ProjectAssets>,
    root: PathBuf,
    rollback: Vec<(PathBuf, bool)>,
    selected: AtomicBool,
    discarded: AtomicBool,
    source_lease: super::cold_store::CommittedColdLease,
    descriptor_reference: String,
}

#[pymethods]
impl PreparedStemPair {
    /// Durable references only, without source, timing, request or ACK rights.
    pub fn selection_json(&self) -> &str {
        &self.selection
    }
    /// FullMix preparation retains no resident component or Instrumental buffers.
    pub fn has_components(&self) -> bool {
        self.stems.is_some()
    }
    /// The caller acquired the complete saved assignment before choosing this selection.
    pub fn select(&self) {
        self.selected.store(true, Ordering::Release);
    }
    /// Retire only this producer's new targets, never any reused member of the pair.
    pub fn discard(&self) -> PyResult<()> {
        if self.selected.load(Ordering::Acquire) || self.discarded.load(Ordering::Acquire) {
            return Ok(());
        }
        for (path, recursive) in &self.rollback {
            self.assets
                .retire(&self.root, path, *recursive)
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        }
        self.discarded.store(true, Ordering::Release);
        Ok(())
    }
    /// Explicit offline access: this reader holds both areas, never a fifth live layer.
    pub fn instrumental_reader(&self, py: Python<'_>) -> PyResult<InstrumentalStemReader> {
        let selection: serde_json::Value = serde_json::from_str(&self.selection)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let descriptor = selection["descriptor_reference"]
            .as_str()
            .ok_or_else(|| PyValueError::new_err("instrumental pair reference missing"))?;
        let material = PreparedMigrationMaterial::from_current(
            &self.root,
            self.reference.clone(),
            self.source_lease.clone(),
        )
        .map_err(PyValueError::new_err)?;
        let pair = py
            .detach(|| open_verified_pair(&self.root, descriptor, &material, &|| false))
            .map_err(PyValueError::new_err)?;
        Ok(InstrumentalStemReader {
            pair,
            reference: self.reference.clone(),
            _identity: self._identity.clone(),
        })
    }
}

/// Sealed complete pair and one source-bound offline window; no callback payload.
#[pyclass(frozen)]
pub struct InstrumentalStemReader {
    pair: VerifiedStemPair,
    reference: SampleBuffer,
    _identity: Arc<[u8; 32]>,
}
#[pymethods]
impl InstrumentalStemReader {
    /// Source-frame origin, frame extent, output Hz and channel layout.
    pub fn geometry(&self) -> (usize, usize, u32, usize) {
        (
            self.reference.resident_start(),
            self.reference.resident_end() - self.reference.resident_start(),
            self.pair.descriptor.content.source.sample_rate_hz,
            self.reference.channels,
        )
    }
    /// Read actual Instrumental f32le on an offline worker, with the same complete pair pins.
    pub fn read_f32le(&self, py: Python<'_>) -> PyResult<Py<pyo3::types::PyBytes>> {
        let encoded = py
            .detach(|| -> Result<Vec<u8>, String> {
                let sample = self.pair.read_instrumental(&self.reference)?;
                let mut bytes = Vec::with_capacity(sample.samples.len() * 4);
                for value in sample.samples.iter() {
                    bytes.extend(value.to_bits().to_le_bytes());
                }
                Ok(bytes)
            })
            .map_err(PyValueError::new_err)?;
        Ok(pyo3::types::PyBytes::new(py, &encoded).unbind())
    }
}

impl Drop for PreparedStemPair {
    fn drop(&mut self) {
        // Registry retirement stays off-thread and keeps all pins until this result drops.
        // A queue-admission error stays visible in the registry; no direct file deletion.
        let _ = self.discard();
    }
}

fn canonical_version(material: &PreparedMigrationMaterial) -> Result<String, String> {
    let metadata = material.metadata();
    Ok(format!(
        "{}|sha256-v1:{}",
        metadata["new_reference"]
            .as_str()
            .ok_or("canonical source reference missing")?,
        metadata["original"]["sha256"]
            .as_str()
            .ok_or("original digest missing")?
    ))
}

impl AudioEngine {
    /// Run on the bounded ordinary stem worker, outside UI and callback threads.
    pub(super) fn prepare_stem_pair_current(
        &self,
        id: usize,
        source_version: &str,
        cache_dir: &str,
        ticket: &PreparedSourceTicket,
        components: bool,
        descriptor_reference: Option<&str>,
    ) -> Result<PreparedStemPair, String> {
        let root = self
            .project_assets_root()
            .map_err(|error| error.to_string())?;
        self.prepare_stem_pair_at_root(
            &root,
            id,
            source_version,
            cache_dir,
            ticket,
            components,
            descriptor_reference,
        )
    }

    /// Capture exact current immutable readers before releasing the Python engine borrow.
    pub(super) fn capture_stem_pair_work(
        &self,
        root: &Path,
        id: usize,
        source_version: &str,
        ticket: &PreparedSourceTicket,
    ) -> Result<StemPairPreparationWork, String> {
        if id >= super::constants::NUM_SAMPLES {
            return Err("id out of range".into());
        }
        let (sample, lease) = {
            let requests = self
                .pad_request_ids
                .lock()
                .map_err(|_| "request lock poisoned")?;
            let cache = self
                .sample_cache
                .lock()
                .map_err(|_| "sample cache lock poisoned")?;
            let sample = cache[id].as_ref().ok_or("source is not loaded")?.clone();
            validate_prepared_ticket(
                ticket,
                id,
                source_version,
                requests[id],
                &self.prepared_source_epochs[id],
                &sample,
            )?;
            let lease = self
                .cold_leases
                .lock()
                .map_err(|_| "cold lease lock poisoned")?[id]
                .clone()
                .ok_or("complete source lease unavailable")?;
            (sample, lease)
        };
        Ok(StemPairPreparationWork {
            root: root.to_owned(),
            sample,
            lease,
            source_version: source_version.to_owned(),
            sample_rate_hz: ticket.sample_rate_hz,
            publication: ticket.publication.clone(),
            cold_cancelled: self.cold_cancelled.clone(),
            assets: self.project_assets.clone(),
        })
    }

    /// The same ordinary kernel with an explicit verified root for device-free execution.
    pub(super) fn prepare_stem_pair_at_root(
        &self,
        root: &Path,
        id: usize,
        source_version: &str,
        cache_dir: &str,
        ticket: &PreparedSourceTicket,
        components: bool,
        descriptor_reference: Option<&str>,
    ) -> Result<PreparedStemPair, String> {
        self.capture_stem_pair_work(root, id, source_version, ticket)?
            .prepare(cache_dir, components, descriptor_reference)
    }

    /// Current subscriber binds its own publication and accepted timing, never another ACK.
    pub(super) fn publish_stem_pair_with_producer(
        &self,
        prepared: &PreparedStemPair,
        ticket: &PreparedSourceTicket,
        producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ) -> PyResult<()> {
        let mut stems = prepared
            .stems
            .clone()
            .ok_or_else(|| PyValueError::new_err("pair has no live component request"))?;
        if !ticket.sample.same_window(&prepared.reference) {
            return Err(PyValueError::new_err("prepared pair window is stale"));
        }
        stems.publication = ticket.publication.clone();
        stems.accepted_timing = ticket.publication.accepted_projection();
        enqueue_current_prepared_pair_with_owner(
            self,
            producer,
            ticket,
            &prepared.source_version,
            stems,
            prepared.wav_path.clone(),
            super::resident_relocation::StemPairOwner {
                root: prepared.root.clone(),
                descriptor_reference: prepared.descriptor_reference.clone(),
                source_lease: prepared.source_lease.clone(),
            },
        )
    }
}

/// An admitted worker owns source readers and scalar fences, never a Python engine borrow.
pub(super) struct StemPairPreparationWork {
    root: PathBuf,
    sample: SampleBuffer,
    lease: super::cold_store::CommittedColdLease,
    source_version: String,
    sample_rate_hz: u32,
    publication: PreparedSourcePermit,
    cold_cancelled: Arc<AtomicBool>,
    assets: Arc<super::project_assets::ProjectAssets>,
}

impl StemPairPreparationWork {
    pub(super) fn prepare(
        self,
        cache_dir: &str,
        components: bool,
        descriptor_reference: Option<&str>,
    ) -> Result<PreparedStemPair, String> {
        let Self {
            root,
            sample,
            lease,
            source_version,
            sample_rate_hz,
            publication,
            cold_cancelled,
            assets,
        } = self;
        let root = root.as_path();
        let source_version = source_version.as_str();
        // Capture immutable readers before another pad assignment can replace them.
        let reader = CompleteSourceReader::new(sample.clone(), Some(lease.clone()))?;
        let cancelled =
            || cold_cancelled.load(std::sync::atomic::Ordering::Acquire) || !publication.current();
        if components
            && super::stem_pair::admitted_component_view_bytes(sample.samples.len())?
                > PCM_LIMIT_BYTES
        {
            return Err("four-component worker transient admission exceeded".into());
        }
        let material = if lease.material_id.is_some() {
            PreparedMigrationMaterial::from_current(&root, sample.clone(), lease)?
        } else {
            let prepared = prepare_material(
                &root,
                &lease.original_path,
                sample_rate_hz,
                sample.channels,
                &cancelled,
            )?;
            prepared
                .lease
                .verify_reference(&sample)
                .map_err(|error| error.to_string())?;
            prepared
        };
        let canonical_source = canonical_version(&material)?;
        let mut created_wav = false;
        let wav_reference = if descriptor_reference.is_some() {
            String::new() // Existing selection is checked by the complete verifier below.
        } else {
            let typed = material_paths::resolve(&root, Path::new(cache_dir))
                .map_err(|error| error.to_string())?;
            if matches!(typed.kind,AssetKind::StemDirectory {material:Some(ref value),generation:true}
                if Some(value)==material.lease.material_id.as_ref())
                && source_version == canonical_source
            {
                cache_dir.to_owned()
            } else {
                // Existing verified copy/gate handles legacy layouts and set deduplication.
                let generation = format!("{:x}", Sha256::digest(cache_dir.as_bytes()));
                let copied = copy_stem_generation(
                    &root,
                    &material,
                    Path::new(cache_dir),
                    source_version,
                    &canonical_source,
                    &generation[..32],
                    &cancelled,
                )?;
                created_wav = copied.created;
                // A fully verified copied half-pair is a recognized retry target
                // if later preparation fails. It gains no common eligibility.
                copied.cache_reference
            }
        };
        let pair = if let Some(reference) = descriptor_reference {
            open_verified_pair(&root, reference, &material, &cancelled)?
        } else {
            prepare_complete_pair_with_reader(
                &root,
                &material,
                &canonical_source,
                &wav_reference,
                &cancelled,
                || {
                    if material.sample.resident_start() == 0
                        && material.sample.resident_end() == material.sample.frame_count()
                    {
                        Ok(material.sample.clone())
                    } else {
                        reader.materialize(PCM_LIMIT_BYTES, &cancelled)
                    }
                },
            )?
        };
        let identity_bytes: [u8; 32] = hex_identity(&pair.descriptor.stem_set_identity)?;
        let mut identity = Arc::new(identity_bytes);
        let selection = json!({"schema_version":1,
            "descriptor_reference":pair.descriptor_reference,
            "stem_set_identity":pair.descriptor.stem_set_identity,
            "wav_generation":pair.descriptor.wav_generation,
            "pcm_generation":pair.descriptor.pcm_generation})
        .to_string();
        let descriptor_reference = pair.descriptor_reference.clone();
        let wav_path = material_paths::resolve(&root, Path::new(&pair.descriptor.wav_generation))
            .map_err(|error| error.to_string())?
            .path;
        let stems = if components {
            // Shared backing is reused only for this same verified complete pair/window.
            match assets
                .shared_stems(&wav_path, &sample, source_version, sample_rate_hz)
                .map_err(|error| error.to_string())?
            {
                Some(shared) if *shared.complete_set_identity == *identity => {
                    identity = shared.complete_set_identity.clone();
                    Some(shared)
                }
                _ => Some(PreparedStemSet {
                    complete_set_identity: identity.clone(),
                    accepted_timing: None,
                    reference_samples: sample.samples.clone(),
                    publication: PreparedSourcePermit::unbound(),
                    source_version_hash: source_version_hash(source_version),
                    sample_rate_hz,
                    channels: sample.channels,
                    frame_count: sample.frame_count(),
                    available_mask: 0b1111,
                    stems: pair.prepare_component_views(&sample)?,
                }),
            }
        } else {
            None
        };
        let mut rollback = Vec::new();
        let project_root = root.parent().ok_or("project root missing")?;
        if created_wav {
            // This producer created the exact WAV target. Coupled registry
            // retirement preflights all three areas atomically; reused WAVs
            // never acquire this producer's rollback right.
            rollback.push((wav_path.clone(), true));
        }
        if pair.created_pcm {
            rollback.push((project_root.join(&pair.descriptor.pcm_generation), true));
        }
        if pair.created_descriptor {
            rollback.push((project_root.join(&pair.descriptor_reference), false));
        }
        assets
            .retain_stem_pair(
                wav_path.clone(),
                &sample,
                source_version,
                &identity,
                stems.as_ref(),
                pair.into_pins(root.parent().ok_or("project root missing")?),
                material.lease.clone(),
            )
            .map_err(|error| error.to_string())?;
        Ok(PreparedStemPair {
            selection,
            wav_path,
            source_version: source_version.into(),
            reference: sample,
            _identity: identity,
            stems,
            assets: assets.clone(),
            root: root.to_owned(),
            rollback,
            selected: AtomicBool::new(false),
            discarded: AtomicBool::new(false),
            source_lease: material.lease.clone(),
            descriptor_reference,
        })
    }
}

fn hex_identity(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 {
        return Err("stem pair identity length differs".into());
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| "invalid stem pair identity")?;
    }
    Ok(bytes)
}

impl AudioEngine {
    /// Enqueue already prepared four-component data without disk work on the control thread.
    pub(super) fn publish_stem_pair_api(
        &self,
        prepared: &PreparedStemPair,
        source_ticket: &PreparedSourceTicket,
    ) -> PyResult<()> {
        let handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
        self.publish_stem_pair_with_producer(prepared, source_ticket, &handle.producer)
    }
}
