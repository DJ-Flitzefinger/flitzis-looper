//! Non-realtime ownership of project originals and immutable stem generations.
//!
//! A path is retired only under the same gate used to admit owners/readers. PCM
//! weak references cover every cloned bank, voice, queued command and native job
//! handle without introducing a file-lease destructor into the audio callback.

use super::cold_store::CommittedColdLease;
pub(super) mod stem_readers;
use crate::messages::{PreparedStemSet, SampleBuffer};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::Duration;
use stem_readers::SharedStemReaders;

const MAX_RETIREMENTS: usize = 1024;
const MAX_REPORTED_ERRORS: usize = 16;
const RETIREMENTS_PER_POLL: usize = 8;
const MAX_READER_RECORDS: usize = 1024;
const MAX_OWNER_RECORDS: usize = 4096;
const MAX_HISTORY_BOOKS: usize = 1024;
const STEM_FILES: [&str; 6] = [
    "vocals.wav",
    "melody.wav",
    "bass.wav",
    "drums.wav",
    "instrumental.wav",
    ".complete.json",
];
const STEM_PCM_FILES: [&str; 6] = [
    "vocals.f32le",
    "melody.f32le",
    "bass.f32le",
    "drums.f32le",
    "instrumental.f32le",
    "manifest.json",
];

pub(super) type PcmHistory = Mutex<Vec<Vec<Weak<[f32]>>>>;

fn backing_contains(backing: &[Arc<[f32]>], pcm: &Weak<[f32]>) -> bool {
    backing.iter().any(|held| pcm.ptr_eq(&Arc::downgrade(held)))
}

fn push_unique_backing(backing: &mut Vec<Arc<[f32]>>, samples: Arc<[f32]>) {
    if !backing.iter().any(|held| Arc::ptr_eq(held, &samples)) {
        backing.push(samples);
    }
}

#[derive(Debug)]
struct PathOwner {
    root: PathBuf,
    path: PathBuf,
    identity: Option<FileIdentity>,
    saved_assignment: AtomicBool,
}

impl PathOwner {
    fn owns_original(&self, lease: &CommittedColdLease) -> bool {
        self.saved_assignment.load(Ordering::Acquire)
            && self.path == lease.original_path
            && self
                .identity
                .as_ref()
                .is_some_and(|identity| lease.original_identity_matches(identity))
    }
}

/// An assignment or Python/background job pin; release performs no file I/O.
#[pyclass]
#[derive(Clone, Debug)]
pub struct ProjectAssetLease {
    owner: Option<Arc<PathOwner>>,
}

#[pymethods]
impl ProjectAssetLease {
    /// Promote a reserved delivered original without another owner allocation.
    /// The load worker reserved this pin before irreversible native adoption.
    pub fn acknowledge(&self, expected_path: String) -> PyResult<()> {
        let owner = self
            .owner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("asset lease released"))?;
        if super::material_paths::resolve(&owner.root, Path::new(&expected_path))
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
            .path
            != owner.path
        {
            return Err(PyRuntimeError::new_err(
                "delivered lease does not match its original reference",
            ));
        }
        ProjectAssets::shared()
            .acknowledge_delivered(owner)
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }
    pub fn release(&mut self) {
        self.owner = None;
    }

    /// Restore this already reserved stem assignment without another owner admission.
    pub fn reclaim_stems(&self, expected_path: String) -> PyResult<()> {
        let owner = self
            .owner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("asset lease released"))?;
        let typed = super::material_paths::resolve(&owner.root, Path::new(&expected_path))
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        if typed.path != owner.path
            || !matches!(
                typed.kind,
                super::material_paths::AssetKind::StemDirectory { .. }
                    | super::material_paths::AssetKind::StemPcmDirectory {
                        generation: true,
                        ..
                    }
                    | super::material_paths::AssetKind::StemPairDescriptor { .. }
            )
        {
            return Err(PyRuntimeError::new_err(
                "reserved lease does not match its stem assignment",
            ));
        }
        ProjectAssets::shared()
            .reclaim_stems(owner)
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }

    #[getter]
    pub fn released(&self) -> bool {
        self.owner.is_none()
    }
}

struct Reader {
    path: PathBuf,
    pcm: Vec<Weak<[f32]>>,
    /// Actual FullMix backing of a component reader; accounting only, never native authority.
    source_pcm: Option<Weak<[f32]>>,
    cold: Option<CommittedColdLease>,
    pending_assignment: bool,
    saved_claim: bool,
    engine: Weak<()>,
    shared_stems: Option<SharedStemReaders>,
}

struct Retirement {
    root: PathBuf,
    recursive: bool,
    identity: Option<FileIdentity>,
    files: Option<Vec<VerifiedFile>>,
    pcm: bool,
    protection: Option<Arc<RetirementProtection>>,
}

impl Retirement {
    fn preserved(&self, reason: &str) {
        if let Some(protection) = self.protection.as_ref() {
            protection.outcome.finish(Err(reason.to_owned()));
        }
    }
}

/// Non-realtime queue ownership lasts through physical deletion, including PCM handoff.
pub(super) struct RetirementProtection {
    pub _inventory: Arc<super::material_migration_recovery::InventoryHandle>,
    pub outcome: Arc<RetirementOutcome>,
}

#[derive(Default)]
pub(super) struct RetirementOutcome(Mutex<Option<Result<(), String>>>);

impl RetirementOutcome {
    pub fn finish(&self, result: Result<(), String>) {
        if let Ok(mut value) = self.0.lock() {
            *value = Some(result.map_err(|error| error.chars().take(2048).collect()));
        }
    }
    pub fn status(&self) -> io::Result<(&'static str, Option<String>)> {
        let value = self
            .0
            .lock()
            .map_err(|_| io::Error::other("retirement outcome poisoned"))?;
        Ok(match &*value {
            None => ("pending", None),
            Some(Ok(())) => ("complete", None),
            Some(Err(error)) => ("error", Some(error.clone())),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct FileIdentity(pub(super) [u64; 3]);

/// Complete immutable-leaf evidence, retained by the existing retirement queues.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VerifiedFile {
    pub name: String,
    pub identity: FileIdentity,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Default)]
struct State {
    owners: Vec<Weak<PathOwner>>,
    readers: Vec<Reader>,
    retiring: HashMap<PathBuf, Retirement>,
    deleted: u64,
    errors: Vec<String>,
    histories: Vec<Weak<PcmHistory>>,
    retirement_cursor: usize,
}

/// One process-wide worker survives engine shutdown and late Python job release.
/// It owns no PCM and performs all file/lease retirement outside realtime code.
pub(super) struct ProjectAssets {
    state: Mutex<State>,
}

impl ProjectAssets {
    fn reclaim_stems(&self, owner: &Arc<PathOwner>) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        let expected = owner
            .identity
            .as_ref()
            .ok_or_else(|| io::Error::other("reserved stem generation never existed"))?;
        let typed = super::material_paths::resolve(&owner.root, &owner.path)?;
        let _guards = directory_guards(
            if matches!(
                typed.kind,
                super::material_paths::AssetKind::StemPairDescriptor { .. }
            ) {
                owner
                    .path
                    .parent()
                    .ok_or_else(|| io::Error::other("common descriptor parent missing"))?
            } else {
                &owner.path
            },
        )?;
        if capture_identity(&owner.path)?.as_ref() != Some(expected) {
            return Err(io::Error::other(
                "reserved stem generation was replaced; preserved",
            ));
        }
        owner.saved_assignment.store(true, Ordering::Release);
        let related = stem_readers::related_pair_paths(&state, &owner.path, Some(expected));
        let legacy = owner.path.parent() == Some(owner.root.join("stems").as_path())
            && owner.path.file_name().is_some_and(is_pad_name);
        state.retiring.retain(|target, retirement| {
            let keep = target != &owner.path
                && !related.contains(target)
                && !(legacy
                    && target.parent() == Some(owner.path.as_path())
                    && target
                        .file_name()
                        .is_some_and(|name| STEM_FILES.contains(&name.to_string_lossy().as_ref())));
            if !keep {
                retirement.preserved("new saved stem assignment cancels retirement; preserved");
            }
            keep
        });
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn isolated() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::default(),
        })
    }
    pub(super) fn prune_material_if_unowned(&self, path: &Path) {
        let Some(root) = super::material_paths::material_root(path) else {
            return;
        };
        let Ok(state) = self.state.lock() else { return };
        if state
            .owners
            .iter()
            .filter_map(Weak::upgrade)
            .any(|owner| intersects(&owner.path, root))
            || state
                .readers
                .iter()
                .any(|reader| reader.protects_path(root))
            || state.retiring.keys().any(|path| intersects(path, root))
        {
            return;
        }
        super::material_paths::prune_empty_material(path);
    }
    fn acknowledge_delivered(&self, owner: &Arc<PathOwner>) -> io::Result<()> {
        if !matches!(
            super::material_paths::resolve(&owner.root, &owner.path)?.kind,
            super::material_paths::AssetKind::Original { .. }
        ) {
            return Err(io::Error::other(
                "only a typed original can acknowledge delivery",
            ));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        if capture_identity(&owner.path)? != owner.identity {
            return Err(io::Error::other(
                "delivered original was replaced; preserved",
            ));
        }
        super::cold_store::admit_original_owner(&owner.path)?;
        owner.saved_assignment.store(true, Ordering::Release);
        if let Some(retirement) = state.retiring.remove(&owner.path) {
            retirement.preserved("new delivered original cancels retirement; preserved");
        }
        for reader in &mut state.readers {
            if reader.path == owner.path
                && reader
                    .cold
                    .as_ref()
                    .is_some_and(|lease| owner.owns_original(lease))
            {
                reader.pending_assignment = false;
                reader.saved_claim = true;
            }
        }
        Ok(())
    }
    pub(super) fn watch_history(&self, history: Weak<PcmHistory>) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        state.histories.retain(|history| history.strong_count() > 0);
        if state.histories.len() >= MAX_HISTORY_BOOKS {
            return Err(io::Error::other("project asset PCM history registry full"));
        }
        state.histories.push(history);
        Ok(())
    }
    pub(super) fn shared() -> Arc<Self> {
        static SERVICE: OnceLock<Arc<ProjectAssets>> = OnceLock::new();
        SERVICE
            .get_or_init(|| {
                let service = Arc::new(Self {
                    state: Mutex::default(),
                });
                let worker = service.clone();
                thread::Builder::new()
                    .name("flitzis-project-asset-retirement".into())
                    .spawn(move || {
                        loop {
                            worker.collect();
                            thread::sleep(Duration::from_millis(10));
                        }
                    })
                    .expect("project asset retirement worker must start");
                service
            })
            .clone()
    }

    pub(super) fn acquire(&self, root: &Path, path: &Path) -> io::Result<ProjectAssetLease> {
        self.acquire_internal(root, path, true, None)
    }

    /// A transient native source job protects reads without acknowledging a
    /// delivered metadata assignment or cancelling its orphan rollback rights.
    pub(super) fn acquire_pin(&self, root: &Path, path: &Path) -> io::Result<ProjectAssetLease> {
        self.acquire_internal(root, path, false, None)
    }

    pub(super) fn acquire_verified_pin(
        &self,
        root: &Path,
        path: &Path,
        identity: &FileIdentity,
    ) -> io::Result<ProjectAssetLease> {
        self.acquire_internal(root, path, false, Some(identity))
    }

    fn acquire_internal(
        &self,
        root: &Path,
        path: &Path,
        claim_pending: bool,
        expected: Option<&FileIdentity>,
    ) -> io::Result<ProjectAssetLease> {
        let (root, path) = owned_path(root, path)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        state.owners.retain(|owner| owner.strong_count() > 0);
        if state.owners.len() >= MAX_OWNER_RECORDS {
            return Err(io::Error::other("project asset owner registry full"));
        }
        let identity = capture_identity(&path)?;
        if expected.is_some_and(|expected| identity.as_ref() != Some(expected)) {
            return Err(io::Error::other(
                "verified asset was replaced before admission",
            ));
        }
        let original = matches!(
            super::material_paths::resolve(&root, &path)?.kind,
            super::material_paths::AssetKind::Original { .. }
        );
        if claim_pending && original && path.exists() {
            super::cold_store::admit_original_owner(&path)?;
        }
        if claim_pending {
            let related = stem_readers::related_pair_paths(&state, &path, identity.as_ref());
            for reader in &mut state.readers {
                if reader.path == path {
                    reader.pending_assignment = false;
                    if reader.cold.as_ref().is_some_and(|lease| {
                        identity
                            .as_ref()
                            .is_some_and(|identity| lease.original_identity_matches(identity))
                    }) {
                        reader.saved_claim = true;
                    }
                }
            }
            let legacy_pad = path.parent() == Some(root.join("stems").as_path())
                && path.file_name().is_some_and(is_pad_name);
            state.retiring.retain(|target, retirement| {
                let keep = target != &path
                    && !related.contains(target)
                    && !(legacy_pad
                        && target.parent() == Some(path.as_path())
                        && target.file_name().is_some_and(|name| {
                            STEM_FILES.contains(&name.to_string_lossy().as_ref())
                        }));
                if !keep {
                    retirement.preserved("new saved assignment cancels retirement; preserved");
                }
                keep
            });
        }
        // Admission and deletion serialize. A new exact owner cancels its pending
        // retirement before the worker can remove bytes; ancestor pins defer it.
        let owner = Arc::new(PathOwner {
            root: root.to_path_buf(),
            path,
            identity,
            saved_assignment: AtomicBool::new(claim_pending),
        });
        state.owners.push(Arc::downgrade(&owner));
        let _ = root;
        Ok(ProjectAssetLease { owner: Some(owner) })
    }

    pub(super) fn retain_cold(
        &self,
        lease: CommittedColdLease,
        sample: &SampleBuffer,
        engine: Weak<()>,
    ) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        if state.readers.len() >= MAX_READER_RECORDS {
            return Err(io::Error::other("project asset reader registry full"));
        }
        let saved_claim = state
            .owners
            .iter()
            .filter_map(Weak::upgrade)
            .any(|owner| owner.owns_original(&lease));
        state.readers.push(Reader {
            path: lease.original_path.clone(),
            pcm: vec![Arc::downgrade(&sample.samples)],
            source_pcm: None,
            cold: Some(lease),
            pending_assignment: true,
            saved_claim,
            engine,
            shared_stems: None,
        });
        Ok(())
    }

    pub(super) fn retain_stems(&self, path: PathBuf, stems: &PreparedStemSet) -> io::Result<()> {
        let pcm = stems
            .stems
            .iter()
            .map(|stem| Arc::downgrade(&stem.samples))
            .collect();
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        if state.readers.len() >= MAX_READER_RECORDS {
            return Err(io::Error::other("project asset reader registry full"));
        }
        state.readers.push(Reader {
            path,
            pcm,
            source_pcm: Some(Arc::downgrade(&stems.reference_samples)),
            cold: None,
            pending_assignment: false,
            saved_claim: false,
            engine: Weak::new(),
            shared_stems: None,
        });
        Ok(())
    }

    /// Extend an existing assignment with a real off-thread reader, without
    /// inventing another pending metadata delivery or durable assignment ID.
    pub(super) fn retain_cold_reader(
        &self,
        lease: &CommittedColdLease,
        sample: &SampleBuffer,
    ) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        let reader = state
            .readers
            .iter_mut()
            .find(|reader| {
                reader
                    .cold
                    .as_ref()
                    .is_some_and(|owned| owned.assignment_id() == lease.assignment_id())
            })
            .ok_or_else(|| io::Error::other("complete reader has no native assignment owner"))?;
        reader.pcm.retain(|pcm| pcm.strong_count() > 0);
        if reader
            .pcm
            .iter()
            .any(|pcm| pcm.ptr_eq(&Arc::downgrade(&sample.samples)))
        {
            return Ok(());
        }
        if reader.pcm.len() >= 128 {
            return Err(io::Error::other("assignment live reader bound exceeded"));
        }
        reader.pcm.push(Arc::downgrade(&sample.samples));
        Ok(())
    }

    /// Resolve the retained immutable lease from an actual captured native PCM
    /// owner. Pointer and full descriptor are both checked; paths confer no proof.
    pub(super) fn cold_lease_for_reader(
        &self,
        sample: &SampleBuffer,
    ) -> io::Result<CommittedColdLease> {
        let state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        let weak = Arc::downgrade(&sample.samples);
        let lease = state
            .readers
            .iter()
            .find_map(|reader| {
                reader
                    .pcm
                    .iter()
                    .any(|pcm| pcm.ptr_eq(&weak))
                    .then(|| reader.cold.clone())
                    .flatten()
            })
            .ok_or_else(|| {
                io::Error::other("captured voice has no retained complete source lease")
            })?;
        lease.verify_reference(sample)?;
        Ok(lease)
    }

    /// Snapshot actual live backing in the existing reader registry off-thread.
    /// Native history, old voices and queued jobs keep these Weak entries live
    /// after their original window has left the control cache. The snapshot
    /// keeps each unique allocation pinned through this preparation's admission.
    #[cfg(test)]
    pub(super) fn held_reader_backings(
        &self,
        lease: &CommittedColdLease,
        stem_generation: Option<&Path>,
    ) -> io::Result<Vec<Arc<[f32]>>> {
        let state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        let mut backing = Vec::new();
        for reader in &state.readers {
            let source = reader.cold.as_ref().is_some_and(|owned| {
                owned.assignment_id() == lease.assignment_id()
                    || (owned.original_path == lease.original_path
                        && owned.cache_path == lease.cache_path)
            });
            if source || stem_generation.is_some_and(|path| path == reader.path) {
                for samples in reader.pcm.iter().filter_map(Weak::upgrade) {
                    if !backing.iter().any(|held| Arc::ptr_eq(held, &samples)) {
                        backing.push(samples);
                    }
                }
            }
        }
        Ok(backing)
    }

    /// Snapshot only this pad's real source/voice/job/history allocations, then resolve their
    /// existing assignment readers and corresponding components. Another pad with equal files
    /// or content does not enter through path matching. These pins last through worker admission.
    /// An actually shared source Arc can resolve several pad assignments; their component
    /// backings are conservatively charged, without claiming exact pad-exclusive attribution.
    pub(super) fn held_same_pad_reader_backings(
        &self,
        history: &PcmHistory,
        id: usize,
        lease: &CommittedColdLease,
    ) -> io::Result<Vec<Arc<[f32]>>> {
        // The collector takes the asset gate before a history lock. Copy and release this lock
        // first; never reverse that order while waiting for the asset gate.
        let mut backing = {
            let pads = history
                .lock()
                .map_err(|_| io::Error::other("PCM history lock poisoned"))?;
            let pad = pads
                .get(id)
                .ok_or_else(|| io::Error::other("same-pad PCM history missing"))?;
            let mut backing = Vec::new();
            for samples in pad.iter().filter_map(Weak::upgrade) {
                push_unique_backing(&mut backing, samples);
            }
            backing
        };
        let state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        let mut sources = vec![lease.clone()];
        for reader in &state.readers {
            if let Some(source) = &reader.cold
                && reader.pcm.iter().any(|pcm| backing_contains(&backing, pcm))
                && !sources
                    .iter()
                    .any(|held| held.assignment_id() == source.assignment_id())
            {
                sources.push(source.clone());
            }
        }
        for reader in &state.readers {
            if reader.cold.as_ref().is_some_and(|source| {
                sources
                    .iter()
                    .any(|held| held.assignment_id() == source.assignment_id())
            }) {
                for samples in reader.pcm.iter().filter_map(Weak::upgrade) {
                    push_unique_backing(&mut backing, samples);
                }
            }
        }
        for reader in &state.readers {
            let source_backing = reader
                .source_pcm
                .as_ref()
                .is_some_and(|pcm| backing_contains(&backing, pcm));
            let paired_source = reader
                .shared_stems
                .as_ref()
                .map(|shared| shared.belongs_to_assignments(&sources))
                .transpose()?
                .unwrap_or(false);
            if source_backing || paired_source {
                for samples in reader.pcm.iter().filter_map(Weak::upgrade) {
                    push_unique_backing(&mut backing, samples);
                }
            }
        }
        Ok(backing)
    }

    pub(super) fn retire(&self, root: &Path, path: &Path, recursive: bool) -> io::Result<()> {
        let (root, path) = owned_path(root, path)?;
        validate_target(&root, &path, recursive)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        let identity = capture_identity(&path)?;
        if let Some(targets) = state
            .readers
            .iter()
            .find_map(|reader| reader.paired_retirements(&path, identity.as_ref()))
        {
            if targets
                .iter()
                .find(|(target, _)| target == &path)
                .is_none_or(|(_, target)| target.recursive != recursive)
            {
                return Err(io::Error::other("paired target retirement shape mismatch"));
            }
            for (target_path, target) in &targets {
                if target.root != root || capture_identity(target_path)? != target.identity {
                    return Err(io::Error::other(
                        "paired retirement object changed; preserved",
                    ));
                }
                if let Some(previous) = state.retiring.get(target_path)
                    && (previous.identity != target.identity
                        || previous
                            .files
                            .as_ref()
                            .is_some_and(|files| Some(files) != target.files.as_ref()))
                {
                    return Err(io::Error::other(
                        "paired retirement evidence changed; preserved",
                    ));
                }
                let proof = target.files.as_ref().expect("complete pair proof");
                if target.recursive {
                    stem_readers::check_pair_children(target_path, proof)?;
                }
                for file in proof {
                    // Admission is bounded metadata work. The already sealed
                    // proof is hashed again by the physical cleanup worker.
                    let leaf = if target.recursive {
                        target_path.join(&file.name)
                    } else {
                        target_path.clone()
                    };
                    let opened = super::cold_store::sealed_reader(&leaf)?;
                    if file_identity(&opened)? != file.identity
                        || opened.metadata()?.len() != file.bytes
                    {
                        return Err(io::Error::other(
                            "paired retirement leaf changed; preserved",
                        ));
                    }
                }
            }
            let needed = targets
                .iter()
                .filter(|(target, _)| !state.retiring.contains_key(target))
                .count();
            if state.retiring.len() + needed > MAX_RETIREMENTS {
                return Err(io::Error::other("project asset retirement queue full"));
            }
            state
                .retiring
                .try_reserve(needed)
                .map_err(|_| io::Error::other("paired retirement queue reservation failed"))?;
            for reader in &mut state.readers {
                if reader.path == path {
                    reader.pending_assignment = false;
                    reader.saved_claim = false;
                }
            }
            for (target_path, target) in targets {
                state
                    .retiring
                    .entry(target_path)
                    .and_modify(|previous| {
                        // Upgrade ordinary same-object evidence while retaining an
                        // existing inventory/outcome Arc and its exact identity.
                        if previous.files.is_none() {
                            previous.files = target.files.clone();
                        }
                    })
                    .or_insert(target);
            }
            return Ok(());
        }
        // Coalesce without replacing the originally captured object identity.
        // A repeated request after pathname ABA must not authorize the new leaf.
        if state.retiring.contains_key(&path) {
            return Ok(());
        }
        for owner in state.owners.iter().filter_map(Weak::upgrade) {
            if owner.path == path && owner.identity.is_some() && owner.identity != identity {
                return Err(io::Error::other("assigned asset was replaced; preserved"));
            }
        }
        // A contained pathname alone grants no deletion rights. The caller must
        // still own its original assignment/job or an actual native reader.
        if !state
            .owners
            .iter()
            .filter_map(Weak::upgrade)
            .any(|owner| owner.path == path || (!recursive && path.starts_with(&owner.path)))
            && !state
                .readers
                .iter()
                .any(|reader| reader.protects_path(&path))
            && !state.retiring.contains_key(&path)
        {
            if identity.is_none() {
                return Ok(());
            }
            return Err(io::Error::other(
                "asset has no recognized assignment or reader ownership",
            ));
        }
        if matches!(
            super::material_paths::resolve(&root, &path)?.kind,
            super::material_paths::AssetKind::StemPcmDirectory { .. }
                | super::material_paths::AssetKind::StemPairDescriptor { .. }
        ) {
            return Err(io::Error::other(
                "paired artifact retirement requires complete verified evidence",
            ));
        }
        if !state.retiring.contains_key(&path) && state.retiring.len() >= MAX_RETIREMENTS {
            return Err(io::Error::other("project asset retirement queue full"));
        }
        for reader in &mut state.readers {
            if reader.path == path {
                reader.pending_assignment = false;
                reader.saved_claim = false;
            }
        }
        state.retiring.insert(
            path,
            Retirement {
                root,
                recursive,
                identity,
                files: None,
                pcm: false,
                protection: None,
            },
        );
        Ok(())
    }

    /// Recovery may retire only the fully sealed objects owned by this exact pin.
    #[cfg(test)]
    pub(super) fn retire_verified(
        &self,
        lease: &ProjectAssetLease,
        files: Vec<VerifiedFile>,
    ) -> io::Result<()> {
        self.retire_verified_guarded(lease, files, None).map(|_| ())
    }

    pub(super) fn retire_verified_guarded(
        &self,
        lease: &ProjectAssetLease,
        files: Vec<VerifiedFile>,
        protection: Option<Arc<RetirementProtection>>,
    ) -> io::Result<Option<Arc<RetirementOutcome>>> {
        let owner = lease
            .owner
            .as_ref()
            .ok_or_else(|| io::Error::other("asset pin released"))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        if !state
            .owners
            .iter()
            .any(|entry| entry.ptr_eq(&Arc::downgrade(owner)))
            || capture_identity(&owner.path)? != owner.identity
        {
            return Err(io::Error::other("verified asset owner changed"));
        }
        let kind = super::material_paths::resolve(&owner.root, &owner.path)?.kind;
        let pcm = matches!(kind, super::material_paths::AssetKind::PcmDirectory);
        let recursive = !matches!(
            kind,
            super::material_paths::AssetKind::Original { .. }
                | super::material_paths::AssetKind::StemPairDescriptor { .. }
        );
        let names: Vec<&str> = match &kind {
            super::material_paths::AssetKind::Original { .. } => vec![
                owner
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| io::Error::other("original leaf name invalid"))?,
            ],
            super::material_paths::AssetKind::PcmDirectory => {
                vec!["decoder.f32le", "playback.f32le", "manifest.json"]
            }
            super::material_paths::AssetKind::StemPcmDirectory {
                generation: true, ..
            } if owner
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(super::material_paths::ready_generation) =>
            {
                STEM_PCM_FILES.to_vec()
            }
            super::material_paths::AssetKind::StemPairDescriptor { .. } => vec![
                owner
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| io::Error::other("common descriptor leaf invalid"))?,
            ],
            super::material_paths::AssetKind::StemDirectory {
                material,
                generation,
            } => {
                if (material.is_some() && !generation)
                    || (*generation
                        && owner.path.file_name().is_none_or(|name| {
                            !super::material_paths::ready_generation(&name.to_string_lossy())
                        }))
                {
                    return Err(io::Error::other("staging/container retirement denied"));
                }
                STEM_FILES.to_vec()
            }
            _ => {
                return Err(io::Error::other(
                    "verified retirement kind is not an owned asset",
                ));
            }
        };
        if files.len() != names.len()
            || files.iter().zip(names).any(|(file, name)| {
                let path = if recursive {
                    owner.path.join(name)
                } else {
                    owner.path.clone()
                };
                file.name != name
                    || file.sha256.len() != 64
                    || !file
                        .sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    || capture_identity(&path).ok().flatten().as_ref() != Some(&file.identity)
            })
        {
            return Err(io::Error::other(
                "verified retirement leaf evidence changed",
            ));
        }
        if let Some(previous) = state.retiring.get(&owner.path) {
            if previous.identity != owner.identity
                || previous
                    .files
                    .as_ref()
                    .is_some_and(|previous| previous != &files)
            {
                return Err(io::Error::other("retirement evidence changed; preserved"));
            }
            if let Some(previous) = previous.protection.as_ref() {
                return Ok(Some(previous.outcome.clone()));
            }
        } else if state.retiring.len() >= MAX_RETIREMENTS {
            return Err(io::Error::other("project asset retirement queue full"));
        }
        for reader in &mut state.readers {
            if reader.path == owner.path {
                reader.pending_assignment = false;
                reader.saved_claim = false;
            }
        }
        state.retiring.insert(
            owner.path.clone(),
            Retirement {
                root: owner.root.clone(),
                recursive,
                identity: owner.identity.clone(),
                files: Some(files),
                pcm,
                protection: protection.clone(),
            },
        );
        Ok(protection.map(|value| value.outcome.clone()))
    }

    pub(super) fn orphan_cold(&self, orphan: &CommittedColdLease) {
        if let Ok(mut state) = self.state.lock() {
            for reader in &mut state.readers {
                if reader
                    .cold
                    .as_ref()
                    .is_some_and(|lease| lease.assignment_id() == orphan.assignment_id())
                {
                    reader.pending_assignment = false;
                }
            }
        }
    }

    pub(super) fn status(&self) -> Result<(usize, usize, u64, Vec<String>), String> {
        let state = self.state.lock().map_err(|_| "asset gate poisoned")?;
        Ok((
            state.retiring.len(),
            state.readers.len(),
            state.deleted,
            state.errors.clone(),
        ))
    }

    #[cfg(test)]
    pub(super) fn collect_for_test(&self) {
        self.collect();
    }

    fn collect(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.owners.retain(|owner| owner.strong_count() > 0);
        state.histories.retain(|history| history.strong_count() > 0);
        for history in state.histories.iter().filter_map(Weak::upgrade) {
            if let Ok(mut pads) = history.lock() {
                for pad in pads.iter_mut() {
                    pad.retain(|pcm| pcm.strong_count() > 0);
                }
            }
        }
        let owners: Vec<_> = state.owners.iter().filter_map(Weak::upgrade).collect();
        let retiring: Vec<_> = state.retiring.keys().cloned().collect();
        let mut retained = Vec::new();
        let mut assigned_files: HashMap<(PathBuf, PathBuf), usize> = HashMap::new();
        let mut assigned_stems = HashMap::new();
        for mut reader in state.readers.drain(..) {
            // Weak<[f32]> keeps the allocation itself alive after final PCM Arc.
            // Durable file/pending metadata must retain no dead PCM allocation.
            reader.pcm.retain(|pcm| pcm.strong_count() > 0);
            if reader
                .source_pcm
                .as_ref()
                .is_some_and(|pcm| pcm.strong_count() == 0)
            {
                reader.source_pcm = None;
            }
            if let Some(lease) = reader.cold.as_ref() {
                lease.prune_dead_pcm();
            }
            let native_live = !reader.pcm.is_empty() || reader.shared_native_live();
            let assigned = owners.iter().any(|owner| reader.assigned_by(&owner.path));
            let retiring_original = retiring.iter().any(|path| path == &reader.path);
            if !retiring_original
                && reader
                    .cold
                    .as_ref()
                    .is_some_and(|lease| owners.iter().any(|owner| owner.owns_original(lease)))
            {
                reader.saved_claim = true;
            }
            if reader.pending_assignment && reader.engine.strong_count() == 0 && !assigned {
                reader.pending_assignment = false;
                if let Some(lease) = reader.cold.as_ref() {
                    lease.rollback_unadopted_original();
                    lease.rollback_unadopted_cache();
                }
            }
            if native_live || reader.pending_assignment {
                retained.push(reader);
            } else if assigned && reader.shared_stems.is_some() {
                let key = reader
                    .shared_file_key()
                    .expect("shared file representative");
                if let std::collections::hash_map::Entry::Vacant(entry) = assigned_stems.entry(key)
                {
                    entry.insert(retained.len());
                    retained.push(reader);
                }
            } else if (assigned || (reader.saved_claim && !retiring_original))
                && reader.cold.is_some()
            {
                // Saved ownership needs one immutable file lease per original /
                // cache generation, not an unbounded history of dead native loads.
                // Failed attempts cannot represent a later valid saved ID. A
                // prior warm generation still needs one checked descriptor for
                // eventual explicit retirement of its saved original; failed
                // cold creations retain no such fallback ownership.
                let lease = reader.cold.as_ref().expect("cold reader checked above");
                if lease.assignment_retired()
                    && !lease.created_cache()
                    && !retiring_original
                    && reader.saved_claim
                {
                    // This is an independent saved-file ID, never revival of
                    // the failed publication's ID or an acknowledgement from a
                    // transient source-job pin. Another original's retirement
                    // cannot remove this surviving saved assignment.
                    reader.cold = Some(lease.fork_saved_assignment());
                }
                let lease = reader.cold.as_ref().expect("cold file ownership retained");
                let retired = lease.assignment_retired();
                if retired && lease.created_cache() {
                    continue;
                }
                let key = (reader.path.clone(), lease.cache_path.clone());
                if let Some(retained_index) = assigned_files.get(&key).copied() {
                    let representative = retained[retained_index]
                        .cold
                        .as_ref()
                        .expect("assigned file representative has cold lease");
                    if !retired && representative.assignment_retired() {
                        // Prefer the live saved ID regardless of registration
                        // order, without promoting an orphan's retired ID.
                        retained[retained_index] = reader;
                    } else if !retired && representative.assignment_id() != lease.assignment_id() {
                        lease.retire_cache();
                    }
                    // A clone of the retained logical assignment only releases
                    // its handle; retiring its shared ID would revoke that owner.
                } else {
                    assigned_files.insert(key, retained.len());
                    retained.push(reader);
                }
            } else if let Some(lease) = reader.cold {
                if retiring_original {
                    lease.retire_cache();
                }
                // Reader/file handles and optional cache retirement are always
                // released by this worker, never by the last callback PCM Arc.
                drop(lease);
            }
        }
        state.readers = retained;
        // The bounded metadata scan selects eligible work before applying the
        // I/O budget, so permanently pinned targets cannot starve later cleanup.
        let mut eligible: Vec<_> = retiring
            .into_iter()
            .filter(|path| {
                !owners.iter().any(|owner| intersects(&owner.path, path))
                    && !state
                        .readers
                        .iter()
                        .any(|reader| reader.protects_path(path))
            })
            .collect();
        // Stable round-robin selection also rotates attempts that Windows keeps
        // blocked by external sharing handles; eight retries cannot starve nine.
        eligible.sort();
        if !eligible.is_empty() {
            let count = eligible.len();
            eligible.rotate_left(state.retirement_cursor % count);
            let attempted = count.min(RETIREMENTS_PER_POLL);
            state.retirement_cursor = (state.retirement_cursor + attempted) % count;
            eligible.truncate(attempted);
        }
        for path in eligible {
            let target = state
                .retiring
                .get(&path)
                .expect("retirement remains under gate");
            let remove_empty_pad = target.recursive;
            let result = if target.pcm {
                CommittedColdLease::queue_verified_retirement(
                    &path,
                    target.identity.as_ref().expect("verified PCM identity"),
                    target.files.as_ref().expect("verified PCM leaves"),
                    target.protection.clone(),
                )
            } else {
                delete_owned(
                    &target.root,
                    &path,
                    target.recursive,
                    target.identity.as_ref(),
                    target.files.as_deref(),
                )
            };
            match result {
                Ok(()) => {
                    let completion = (!target.pcm).then(|| target.protection.clone()).flatten();
                    state.retiring.remove(&path);
                    state.deleted += 1;
                    if let Some(material) = super::material_paths::material_root(&path)
                        && !owners.iter().any(|owner| intersects(&owner.path, material))
                        && !state
                            .readers
                            .iter()
                            .any(|reader| reader.protects_path(material))
                        && !state.retiring.keys().any(|path| intersects(path, material))
                    {
                        super::material_paths::prune_empty_material(&path);
                    }
                    if remove_empty_pad {
                        let pad = path.parent().expect("validated generation has pad parent");
                        if !owners.iter().any(|owner| intersects(&owner.path, pad))
                            && !state.readers.iter().any(|reader| reader.protects_path(pad))
                            && !state.retiring.keys().any(|target| intersects(target, pad))
                        {
                            // An empty container has no unique data. New generation
                            // admission uses this same gate; unknown/new children
                            // make remove_dir fail safely without recursion.
                            let _ = remove_empty_pad_container(pad);
                        }
                    }
                    // Keep the inventory gate through physical deletion and the
                    // bounded empty-container pass before exposing completion.
                    if let Some(protection) = completion {
                        protection.outcome.finish(Ok(()));
                    }
                }
                Err(error) if retryable(&error) => {}
                Err(error) => {
                    if let Some(protection) = target.protection.as_ref() {
                        protection.outcome.finish(Err(error.to_string()));
                    }
                    state.retiring.remove(&path);
                    if state.errors.len() < MAX_REPORTED_ERRORS {
                        state.errors.push(format!("{}: {error}", path.display()));
                    }
                }
            }
        }
    }
}

fn remove_empty_pad_container(pad: &Path) -> io::Result<()> {
    reject_links(pad)?;
    let parent = pad
        .parent()
        .ok_or_else(|| io::Error::other("stem pad parent missing"))?;
    let _guards = directory_guards(parent)?;
    if fs::read_dir(pad)?.next().is_none() {
        fs::remove_dir(pad)?;
    }
    Ok(())
}

fn intersects(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

pub(super) fn reject_links(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() {
            return Err(io::Error::other("project asset path contains a link"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(io::Error::other(
                    "project asset path contains a reparse point",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn owned_path(root: &Path, path: &Path) -> io::Result<(PathBuf, PathBuf)> {
    if !root.is_absolute() {
        return Err(io::Error::other("asset root must be absolute"));
    }
    reject_links(root)?;
    let root = fs::canonicalize(root)?;
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        root.parent()
            .ok_or_else(|| io::Error::other("samples parent missing"))?
            .join(path)
    };
    if absolute
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(io::Error::other("asset path contains traversal"));
    }
    let path = canonical_with_missing(&absolute)?;
    if path == root || !path.starts_with(&root) {
        return Err(io::Error::other("asset path is outside owned samples"));
    }
    Ok((root, path))
}

pub(super) fn canonical_with_missing(absolute: &Path) -> io::Result<PathBuf> {
    reject_links(absolute)?;
    // Resolve the nearest existing ancestor, preserving a missing exact leaf.
    let mut existing = absolute;
    let mut missing = Vec::new();
    while !existing.exists() {
        missing.push(
            existing
                .file_name()
                .ok_or_else(|| io::Error::other("asset path missing"))?
                .to_owned(),
        );
        existing = existing
            .parent()
            .ok_or_else(|| io::Error::other("asset parent missing"))?;
    }
    let mut path = fs::canonicalize(existing)?;
    for part in missing.into_iter().rev() {
        path.push(part);
    }
    Ok(path)
}

fn is_generation(name: &str) -> bool {
    [".generation-", ".ready-"].iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|id| {
            id.len() == 32
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    })
}

pub(super) fn is_pad_name(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| super::material_paths::slot_number(name).is_some())
}

fn validate_target(root: &Path, path: &Path, recursive: bool) -> io::Result<()> {
    let typed = super::material_paths::resolve(root, path)?;
    if matches!(
        typed.kind,
        super::material_paths::AssetKind::Original { .. }
    ) && !recursive
    {
        return Ok(());
    }
    if matches!(
        typed.kind,
        super::material_paths::AssetKind::StemDirectory {
            material: Some(_),
            generation: true
        }
    ) && recursive
    {
        return Ok(());
    }
    if matches!(
        typed.kind,
        super::material_paths::AssetKind::StemPcmDirectory {
            generation: true,
            ..
        }
    ) && recursive
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(super::material_paths::ready_generation)
    {
        return Ok(());
    }
    if matches!(
        typed.kind,
        super::material_paths::AssetKind::StemPairDescriptor { .. }
    ) && !recursive
    {
        return Ok(());
    }
    if matches!(typed.kind, super::material_paths::AssetKind::StemArtifact) && !recursive {
        return Ok(());
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::Error::other("asset outside samples"))?;
    let parts: Vec<_> = relative.iter().collect();
    if recursive {
        if parts.len() != 3
            || parts[0] != "stems"
            || !is_pad_name(parts[1])
            || !is_generation(&parts[2].to_string_lossy())
        {
            return Err(io::Error::other(
                "recursive retirement requires an exact owned stem generation",
            ));
        }
    } else if parts.first().is_some_and(|part| *part == "stems") {
        if parts.len() != 3 || !STEM_FILES.contains(&parts[2].to_string_lossy().as_ref()) {
            return Err(io::Error::other(
                "legacy stem retirement requires a declared exact file",
            ));
        }
    } else if parts.len() != 1 {
        return Err(io::Error::other(
            "original retirement requires an exact samples file",
        ));
    } else if parts[0].to_string_lossy().starts_with('.')
        || parts[0].to_string_lossy().ends_with(".config.json")
        || parts[0] == "config.json"
    {
        return Err(io::Error::other(
            "reserved project metadata is not an original artifact",
        ));
    }
    Ok(())
}

fn delete_owned(
    root: &Path,
    path: &Path,
    recursive: bool,
    identity: Option<&FileIdentity>,
    proof: Option<&[VerifiedFile]>,
) -> io::Result<()> {
    let (_, resolved) = owned_path(root, path)?;
    if proof.is_none() {
        validate_target(root, &resolved, recursive)?;
    } else if !matches!(
        super::material_paths::resolve(root, &resolved)?.kind,
        super::material_paths::AssetKind::Original { .. }
            | super::material_paths::AssetKind::StemDirectory { .. }
            | super::material_paths::AssetKind::StemPcmDirectory {
                generation: true,
                ..
            }
            | super::material_paths::AssetKind::StemPairDescriptor { .. }
    ) {
        return Err(io::Error::other("verified retirement target changed kind"));
    }
    // Guard all directories from the volume root down to the target's parent.
    // FILE_SHARE_DELETE is excluded: rename/junction replacement cannot redirect
    // a checked path after containment validation and before leaf removal.
    let _ancestors = directory_guards(
        resolved
            .parent()
            .ok_or_else(|| io::Error::other("asset parent missing"))?,
    )?;
    if !resolved.exists() {
        return Ok(());
    }
    if !recursive {
        return if let Some(proof) = proof {
            remove_verified_file(&resolved, &proof[0])
        } else {
            remove_owned_file(&resolved, identity)
        };
    }
    let generation_guard = directory_guards(&resolved)?;
    if let Some(identity) = identity
        && capture_identity(&resolved)?.as_ref() != Some(identity)
    {
        return Err(io::Error::other(
            "retired generation was replaced; preserved",
        ));
    }
    let pcm = matches!(
        super::material_paths::resolve(root, &resolved)?.kind,
        super::material_paths::AssetKind::StemPcmDirectory { .. }
    );
    if pcm && proof.is_none() {
        return Err(io::Error::other("complete stem PCM proof required"));
    }
    let known = if pcm { &STEM_PCM_FILES } else { &STEM_FILES };
    let mut files = Vec::new();
    for entry in fs::read_dir(&resolved)?.take(13) {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !entry.file_type()?.is_file()
            || proof.is_some_and(|proof| !proof.iter().any(|file| file.name == name))
            || !(known.contains(&name.as_ref())
                || (!pcm && STEM_FILES.iter().any(|file| name == format!("{file}.tmp"))))
        {
            return Err(io::Error::other(
                "generation contains an unknown file; preserved",
            ));
        }
        reject_links(&entry.path())?;
        if let Some(proof) = proof {
            let file = proof
                .iter()
                .find(|file| file.name == name)
                .expect("verified leaf name");
            if capture_identity(&entry.path())?.as_ref() != Some(&file.identity) {
                return Err(io::Error::other("generation leaf was replaced; preserved"));
            }
            verify_file_proof(&entry.path(), file)?;
        }
        files.push(entry.path());
    }
    for file in files {
        if let Some(proof) = proof {
            let proof = proof
                .iter()
                .find(|proof| {
                    file.file_name()
                        .is_some_and(|name| name == proof.name.as_str())
                })
                .expect("verified leaf");
            remove_verified_file(&file, proof)?;
        } else {
            remove_owned_file(&file, None)?;
        }
    }
    drop(generation_guard);
    fs::remove_dir(&resolved)
}

pub(super) fn remove_owned_file(path: &Path, identity: Option<&FileIdentity>) -> io::Result<()> {
    remove_exact_object(path, identity, false, None)
}

pub(super) fn remove_verified_file(path: &Path, proof: &VerifiedFile) -> io::Result<()> {
    remove_exact_object(path, Some(&proof.identity), false, Some(proof))
}

/// Preflight every known leaf before a multi-leaf retirement mutates anything.
pub(super) fn verify_file_proof(path: &Path, proof: &VerifiedFile) -> io::Result<()> {
    use std::io::Read;
    let mut file = super::cold_store::sealed_reader(path)?;
    if file_identity(&file)? != proof.identity || file.metadata()?.len() != proof.bytes {
        return Err(io::Error::other(
            "verified leaf identity or extent changed; preserved",
        ));
    }
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("verified leaf extent overflow"))?;
        if bytes > proof.bytes {
            return Err(io::Error::other("verified leaf changed extent; preserved"));
        }
        digest.update(&buffer[..count]);
    }
    if bytes != proof.bytes || format!("{:x}", digest.finalize()) != proof.sha256 {
        return Err(io::Error::other(
            "verified leaf contents changed; preserved",
        ));
    }
    Ok(())
}

pub(super) fn remove_empty_directory(path: &Path, identity: &FileIdentity) -> io::Result<()> {
    remove_exact_object(path, Some(identity), true, None)
}

fn remove_exact_object(
    path: &Path,
    identity: Option<&FileIdentity>,
    directory: bool,
    proof: Option<&VerifiedFile>,
) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use std::os::windows::io::AsRawHandle;
        // DELETE + FILE_READ_ATTRIBUTES, no shared write/delete. Delete the
        // validated opened object by handle, never a later pathname replacement.
        let mut file = fs::OpenOptions::new()
            .access_mode(0x0001_0080 | if proof.is_some() { 0x8000_0000 } else { 0 })
            .share_mode(1)
            .custom_flags(0x0020_0000 | if directory { 0x0200_0000 } else { 0 })
            .open(path)?;
        let metadata = file.metadata()?;
        if (if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        }) || metadata.file_attributes() & 0x400 != 0
        {
            return Err(io::Error::other("retired original is not an ordinary file"));
        }
        let opened_identity = file_identity(&file)?;
        if identity.is_some_and(|identity| &opened_identity != identity) {
            return Err(io::Error::other("retired original was replaced; preserved"));
        }
        if let Some(proof) = proof {
            use std::io::Read;
            let mut digest = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            let mut bytes = 0_u64;
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                bytes = bytes
                    .checked_add(count as u64)
                    .ok_or_else(|| io::Error::other("leaf extent overflow"))?;
                if bytes > proof.bytes {
                    return Err(io::Error::other("retired leaf changed length; preserved"));
                }
                digest.update(&buffer[..count]);
            }
            if bytes != proof.bytes || format!("{:x}", digest.finalize()) != proof.sha256 {
                return Err(io::Error::other("retired leaf changed contents; preserved"));
            }
        }
        if directory && fs::read_dir(path)?.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::DirectoryNotEmpty,
                "unknown children preserve container",
            ));
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn SetFileInformationByHandle(
                handle: *mut std::ffi::c_void,
                class: i32,
                information: *const std::ffi::c_void,
                size: u32,
            ) -> i32;
        }
        #[repr(C)]
        struct FileDispositionInfo {
            delete_file: u8,
        }
        let delete = FileDispositionInfo { delete_file: 1 };
        // SAFETY: `file` owns a live Windows handle, and FILE_DISPOSITION_INFO
        // consists of one BOOLEAN whose pointer and size remain valid for the call.
        let result = unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                4,
                std::ptr::from_ref(&delete).cast(),
                std::mem::size_of::<FileDispositionInfo>() as u32,
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if proof.is_some() {
            return Err(io::Error::other(
                "verified retirement requires sealed Windows handles",
            ));
        }
        if identity.is_some_and(|identity| {
            capture_identity(path).ok().flatten().as_ref() != Some(identity)
        }) {
            return Err(io::Error::other("retired original was replaced; preserved"));
        }
        if directory {
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        }
    }
}

pub(super) fn capture_identity(path: &Path) -> io::Result<Option<FileIdentity>> {
    if !path.exists() {
        return Ok(None);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let file = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(path)?;
        file_identity(&file).map(Some)
    }
    #[cfg(not(windows))]
    {
        file_identity(&fs::File::open(path)?).map(Some)
    }
}

pub(super) fn file_identity(file: &fs::File) -> io::Result<FileIdentity> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        #[derive(Default)]
        struct Information {
            attributes: u32,
            creation: [u32; 2],
            access: [u32; 2],
            write: [u32; 2],
            volume: u32,
            size_high: u32,
            size_low: u32,
            links: u32,
            index_high: u32,
            index_low: u32,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetFileInformationByHandle(
                handle: *mut std::ffi::c_void,
                information: *mut Information,
            ) -> i32;
        }
        let mut info = Information::default();
        // SAFETY: the native layout is BY_HANDLE_FILE_INFORMATION and `file`
        // keeps its live handle throughout this synchronous kernel call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(FileIdentity([
            u64::from(info.volume),
            u64::from(info.index_high),
            u64::from(info.index_low),
        ]))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        Ok(FileIdentity([metadata.dev(), metadata.ino(), 0]))
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = file;
        Err(io::Error::other("asset file identity unavailable"))
    }
}

pub(super) fn directory_guards(parent: &Path) -> io::Result<Vec<fs::File>> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let mut guards = Vec::new();
        for ancestor in parent.ancestors() {
            let guard = fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .custom_flags(0x0200_0000 | 0x0020_0000)
                .open(ancestor)?;
            let metadata = guard.metadata()?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
                return Err(io::Error::other(
                    "asset ancestor is not an ordinary directory",
                ));
            }
            guards.push(guard);
        }
        Ok(guards)
    }
    #[cfg(not(windows))]
    {
        reject_links(parent)?;
        Ok(Vec::new())
    }
}

fn retryable(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied || matches!(error.raw_os_error(), Some(32 | 33))
}

impl super::AudioEngine {
    pub(super) fn project_assets_root(&self) -> PyResult<PathBuf> {
        std::env::current_dir()
            .map(|path| path.join("samples"))
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> ProjectAssets {
        ProjectAssets {
            state: Mutex::default(),
        }
    }

    #[test]
    fn canonical_original_last_owner_cleanup_prunes_only_empty_known_material_containers() {
        for unknown in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("samples");
            let material = root.join("materials/M0123456789abcdef0123456789abcdef");
            let original = material.join("original/Take.wav");
            fs::create_dir_all(original.parent().unwrap()).unwrap();
            fs::create_dir_all(material.join(".pcm-cache/v1")).unwrap();
            fs::create_dir_all(material.join("stems")).unwrap();
            fs::write(&original, b"immutable original").unwrap();
            if unknown {
                fs::write(material.join("private.keep"), b"preserved").unwrap();
            }
            let assets = service();
            let mut owner = assets.acquire(&root, &original).unwrap();
            assets.retire(&root, &original, false).unwrap();
            assets.collect();
            assert!(original.exists());
            owner.release();
            assets.collect();
            assert!(!original.exists());
            assert_eq!(material.exists(), unknown);
            if unknown {
                assert_eq!(
                    fs::read(material.join("private.keep")).unwrap(),
                    b"preserved"
                );
            }
        }
    }

    #[test]
    fn last_assignment_and_native_reader_both_protect_exact_original() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let path = root.join("original.wav");
        fs::write(&path, "unchanged").unwrap();
        let assets = service();
        let mut first = assets.acquire(&root, &path).unwrap();
        let mut second = assets.acquire(&root, &path).unwrap();
        let pcm: Arc<[f32]> = Arc::from([1.0_f32]);
        assets.state.lock().unwrap().readers.push(Reader {
            path: fs::canonicalize(&path).unwrap(),
            pcm: vec![Arc::downgrade(&pcm)],
            source_pcm: None,
            cold: None,
            pending_assignment: false,
            saved_claim: false,
            engine: Weak::new(),
            shared_stems: None,
        });
        assets.retire(&root, &path, false).unwrap();
        first.release();
        assets.collect();
        assert!(path.exists());
        second.release();
        assets.collect();
        assert!(path.exists());
        let queued_or_voice = pcm.clone();
        drop(pcm);
        assets.collect();
        assert!(path.exists());
        drop(queued_or_voice);
        assets.collect();
        assert!(!path.exists());
        assert_eq!(assets.status().unwrap().0, 0);
    }

    #[test]
    fn admission_cancels_pending_retirement_and_unknown_generation_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        let generation = root.join("stems/#2/.ready-0123456789abcdef0123456789abcdef");
        fs::create_dir_all(&generation).unwrap();
        fs::write(generation.join("vocals.wav"), "stem").unwrap();
        let assets = service();
        let mut original_owner = assets.acquire(&root, &generation).unwrap();
        assets.retire(&root, &generation, true).unwrap();
        original_owner.release();
        let mut owner = assets.acquire(&root, &generation).unwrap();
        owner.release();
        assets.collect();
        assert!(generation.exists());
        fs::write(generation.join("private.bin"), "preserve").unwrap();
        let mut known_owner = assets.acquire(&root, &generation).unwrap();
        assets.retire(&root, &generation, true).unwrap();
        known_owner.release();
        assets.collect();
        assert!(generation.join("vocals.wav").exists());
        assert!(generation.join("private.bin").exists());
        assert_eq!(assets.status().unwrap().3.len(), 1);
        assert!(
            assets
                .retire(&root, generation.parent().unwrap(), true)
                .is_err()
        );
        assert!(
            assets
                .retire(&root, &temp.path().join("private.wav"), false)
                .is_err()
        );
    }

    #[test]
    fn restored_legacy_pad_cancels_only_declared_leaves_and_survives_saved_shutdown() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        let pad = root.join("stems/#7");
        let generation = pad.join(".generation-0123456789abcdef0123456789abcdef");
        fs::create_dir_all(&generation).unwrap();
        for name in STEM_FILES {
            fs::write(pad.join(name), "legacy").unwrap();
        }
        fs::write(generation.join("vocals.wav"), "failed private output").unwrap();
        let assets = service();
        let mut original = assets.acquire(&root, &pad).unwrap();
        let mut job = assets.acquire_pin(&root, &generation).unwrap();
        for name in STEM_FILES {
            assets.retire(&root, &pad.join(name), false).unwrap();
        }
        assets.retire(&root, &generation, true).unwrap();
        original.release();
        let mut transient = assets.acquire_pin(&root, &pad).unwrap();
        assert_eq!(assets.status().unwrap().0, STEM_FILES.len() + 1);
        transient.release();
        let mut restored = assets.acquire(&root, &pad).unwrap();
        assert_eq!(assets.status().unwrap().0, 1);
        job.release();
        assets.collect();
        assert!(generation.exists());
        // Closing a saved project releases its process owner without revoking
        // the six restored legacy leaves. The unrelated failed generation still
        // has its original retirement request and can now drain.
        restored.release();
        assets.collect();
        assert!(!generation.exists());
        assert!(STEM_FILES.iter().all(|name| pad.join(name).exists()));
        assert_eq!(assets.status().unwrap().0, 0);
        let mut final_owner = assets.acquire(&root, &pad).unwrap();
        for name in STEM_FILES {
            assets.retire(&root, &pad.join(name), false).unwrap();
        }
        final_owner.release();
        assets.collect();
        assert!(STEM_FILES.iter().all(|name| !pad.join(name).exists()));
        assert_eq!(assets.status().unwrap().0, 0);
    }

    #[test]
    #[cfg(windows)]
    fn windows_sharing_defers_and_retries_after_final_file_reader() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let path = root.join("source.wav");
        fs::write(&path, "original").unwrap();
        let reader = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let assets = service();
        let mut owner = assets.acquire(&root, &path).unwrap();
        assets.retire(&root, &path, false).unwrap();
        owner.release();
        assets.collect();
        assert!(path.exists());
        assert_eq!(assets.status().unwrap().0, 1);
        drop(reader);
        assets.collect();
        assert!(!path.exists());
    }

    #[test]
    fn replaced_original_generation_is_preserved_after_retirement_was_requested() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let path = root.join("source.wav");
        fs::write(&path, "old original").unwrap();
        let assets = service();
        let mut owner = assets.acquire(&root, &path).unwrap();
        assets.retire(&root, &path, false).unwrap();
        owner.release();
        // Preserve old object so its identity cannot be recycled for the new file.
        fs::rename(&path, root.join("retired-object.wav")).unwrap();
        fs::write(&path, "new unknown bytes").unwrap();
        assets.collect();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new unknown bytes");
        assert_eq!(assets.status().unwrap().3.len(), 1);
        assert!(
            assets
                .retire(&root, &root.join("config.json"), false)
                .is_err()
        );
        assert!(
            assets
                .retire(&root, &root.join("flitzis_looper.config.json"), false)
                .is_err()
        );
    }

    #[test]
    #[cfg(windows)]
    fn replaced_samples_junction_never_redirects_cleanup_into_foreign_originals() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        let foreign = temp.path().join("foreign");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&foreign).unwrap();
        let path = root.join("source.wav");
        fs::write(&path, "owned").unwrap();
        fs::write(foreign.join("source.wav"), "foreign").unwrap();
        let assets = service();
        let mut owner = assets.acquire(&root, &path).unwrap();
        assets.retire(&root, &path, false).unwrap();
        owner.release();
        fs::rename(&root, temp.path().join("old-samples")).unwrap();
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&root)
            .arg(&foreign)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "contained junction creation failed: {:?}",
            output.status
        );
        assets.collect();
        assert_eq!(
            fs::read_to_string(foreign.join("source.wav")).unwrap(),
            "foreign"
        );
        assert_eq!(assets.status().unwrap().3.len(), 1);
        // Remove only the junction itself; fs::remove_dir never traverses it.
        fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn eligible_cleanup_is_not_starved_by_eight_pinned_retirements() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let assets = service();
        let mut pinned = Vec::new();
        for index in 0..RETIREMENTS_PER_POLL + 1 {
            let path = root.join(format!("source-{index}.wav"));
            fs::write(&path, "owned").unwrap();
            let mut owner = assets.acquire(&root, &path).unwrap();
            assets.retire(&root, &path, false).unwrap();
            if index == RETIREMENTS_PER_POLL {
                owner.release();
            } else {
                pinned.push(owner);
            }
        }
        assets.collect();
        assert!(
            !root
                .join(format!("source-{}.wav", RETIREMENTS_PER_POLL))
                .exists()
        );
        assert_eq!(assets.status().unwrap().0, RETIREMENTS_PER_POLL);
    }

    #[test]
    fn generation_cleanup_preserves_new_generation_and_removes_final_empty_pad_only() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        let pad = root.join("stems/#3");
        let old = pad.join(".ready-00000000000000000000000000000001");
        let new = pad.join(".ready-00000000000000000000000000000002");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(old.join("vocals.wav"), "old").unwrap();
        fs::write(new.join("vocals.wav"), "new").unwrap();
        let assets = service();
        let mut old_owner = assets.acquire(&root, &old).unwrap();
        let mut new_owner = assets.acquire(&root, &new).unwrap();
        assets.retire(&root, &old, true).unwrap();
        old_owner.release();
        assets.collect();
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(new.join("vocals.wav")).unwrap(), "new");
        assets.retire(&root, &new, true).unwrap();
        assets.collect();
        assert!(new.exists());
        new_owner.release();
        assets.collect();
        assert!(!new.exists() && !pad.exists());
    }

    #[test]
    fn off_thread_history_releases_dead_pcm_allocation_weak_pins_without_a_new_load() {
        let assets = service();
        let historical: Arc<[f32]> = Arc::from([1.0_f32; 128]);
        let current: Arc<[f32]> = Arc::from([2.0_f32; 128]);
        let history = Arc::new(Mutex::new(vec![vec![
            Arc::downgrade(&historical),
            Arc::downgrade(&current),
        ]]));
        assets.watch_history(Arc::downgrade(&history)).unwrap();
        assert_eq!(Arc::weak_count(&current), 1);
        assets.collect();
        assert_eq!(history.lock().unwrap()[0].len(), 2);
        drop(historical);
        assets.collect();
        let history = history.lock().unwrap();
        assert_eq!(
            history[0].len(),
            1,
            "dead Weak allocation owner must retire without another load"
        );
        assert!(Arc::ptr_eq(&history[0][0].upgrade().unwrap(), &current));
    }

    #[test]
    fn retained_pending_metadata_releases_dead_pcm_weak_allocation() {
        let assets = service();
        let engine = Arc::new(());
        let pcm: Arc<[f32]> = Arc::from([1.0_f32; 128]);
        assets.state.lock().unwrap().readers.push(Reader {
            path: PathBuf::from("pending-original"),
            pcm: vec![Arc::downgrade(&pcm)],
            source_pcm: None,
            cold: None,
            pending_assignment: true,
            saved_claim: false,
            engine: Arc::downgrade(&engine),
            shared_stems: None,
        });
        assert_eq!(Arc::weak_count(&pcm), 1);
        drop(pcm);
        assets.collect();
        let state = assets.state.lock().unwrap();
        assert_eq!(state.readers.len(), 1);
        assert!(state.readers[0].pcm.is_empty());
    }

    #[test]
    fn transient_source_pin_does_not_acknowledge_pending_success() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let path = root.join("pending.wav");
        fs::write(&path, "owned").unwrap();
        let assets = service();
        let engine = Arc::new(());
        let (_, path) = owned_path(&root, &path).unwrap();
        assets.state.lock().unwrap().readers.push(Reader {
            path: path.clone(),
            pcm: Vec::new(),
            source_pcm: None,
            cold: None,
            pending_assignment: true,
            saved_claim: false,
            engine: Arc::downgrade(&engine),
            shared_stems: None,
        });
        let mut pin = assets.acquire_pin(&root, &path).unwrap();
        assert!(
            !pin.owner
                .as_ref()
                .unwrap()
                .saved_assignment
                .load(Ordering::Acquire)
        );
        pin.release();
        assets.collect();
        assert!(assets.state.lock().unwrap().readers[0].pending_assignment);
        let _assignment = assets.acquire(&root, &path).unwrap();
        assert!(
            _assignment
                .owner
                .as_ref()
                .unwrap()
                .saved_assignment
                .load(Ordering::Acquire)
        );
        assert!(!assets.state.lock().unwrap().readers[0].pending_assignment);
    }

    #[cfg(windows)]
    #[test]
    fn nine_eligible_targets_progress_past_eight_external_sharing_retries() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let assets = service();
        let mut blockers = Vec::new();
        for index in 0..RETIREMENTS_PER_POLL + 1 {
            let path = root.join(format!("source-{index}.wav"));
            fs::write(&path, "owned").unwrap();
            let mut owner = assets.acquire(&root, &path).unwrap();
            assets.retire(&root, &path, false).unwrap();
            owner.release();
            if index < RETIREMENTS_PER_POLL {
                blockers.push(
                    fs::OpenOptions::new()
                        .read(true)
                        .share_mode(1)
                        .open(&path)
                        .unwrap(),
                );
            }
        }
        assets.collect();
        assert_eq!(assets.status().unwrap().0, RETIREMENTS_PER_POLL + 1);
        assets.collect();
        assert!(
            !root
                .join(format!("source-{}.wav", RETIREMENTS_PER_POLL))
                .exists()
        );
        assert_eq!(assets.status().unwrap().0, RETIREMENTS_PER_POLL);
        drop(blockers);
        assets.collect();
        assert_eq!(assets.status().unwrap().0, 0);
    }

    #[test]
    fn original_assignment_and_duplicate_retirement_preserve_replaced_file() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let path = root.join("source.wav");
        fs::write(&path, "first").unwrap();
        let assets = service();
        let mut owner = assets.acquire(&root, &path).unwrap();
        fs::rename(&path, root.join("displaced.wav")).unwrap();
        fs::write(&path, "replacement").unwrap();
        assert!(assets.retire(&root, &path, false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
        owner.release();

        let mut replacement_owner = assets.acquire(&root, &path).unwrap();
        assets.retire(&root, &path, false).unwrap();
        replacement_owner.release();
        fs::rename(&path, root.join("replacement-displaced.wav")).unwrap();
        fs::write(&path, "third").unwrap();
        assets.retire(&root, &path, false).unwrap();
        assets.collect();
        assert_eq!(fs::read_to_string(&path).unwrap(), "third");
        assert_eq!(assets.status().unwrap().3.len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn dead_same_original_history_compacts_and_shared_original_remains_durable() {
        use super::super::cold_jobs::PCM_LIMIT_BYTES;
        use super::super::cold_store::{ColdTransaction, PcmArtifactInput};
        use super::super::sample_loader::{decode_audio_snapshot, prepare_playback};
        fn prepared(
            samples: &Path,
            source: &Path,
            import: bool,
            channels: usize,
        ) -> (SampleBuffer, CommittedColdLease) {
            let mut transaction =
                ColdTransaction::capture(samples, source, import, &|| false).unwrap();
            let _gate = transaction
                .preparation_gate(48_000, channels, &|| false)
                .unwrap();
            let sample = if let Some(sample) = transaction
                .try_reuse(48_000, channels, PCM_LIMIT_BYTES, &|| false)
                .unwrap()
            {
                sample
            } else {
                let decoded = decode_audio_snapshot(
                    transaction.snapshot_file().unwrap(),
                    source,
                    48_000,
                    PCM_LIMIT_BYTES,
                    &|| false,
                    |_| {},
                )
                .unwrap();
                let (sample, transform) = prepare_playback(
                    &decoded,
                    channels,
                    48_000,
                    PCM_LIMIT_BYTES,
                    &|| false,
                    |_| {},
                )
                .unwrap();
                transaction
                    .write_pcm_artifacts(
                        PcmArtifactInput {
                            samples: &decoded.samples,
                            rate_hz: decoded.rate_hz,
                            channels: decoded.channels,
                            provenance: decoded.decoder.to_json(),
                        },
                        PcmArtifactInput {
                            samples: &sample.samples,
                            rate_hz: 48_000,
                            channels,
                            provenance: serde_json::json!({"processing":"full-buffer-playback-v1"}),
                        },
                        transform.to_json(),
                        &|| false,
                    )
                    .unwrap();
                sample
            };
            transaction.commit(&|| false).unwrap();
            transaction.bind_pcm(&sample.samples);
            (sample, transaction.into_lease())
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        let source = temp.path().join("external.wav");
        let mut wav = b"RIFF".to_vec();
        wav.extend(40_u32.to_le_bytes());
        wav.extend(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
        wav.extend(48_000_u32.to_le_bytes());
        wav.extend(96_000_u32.to_le_bytes());
        wav.extend(2_u16.to_le_bytes());
        wav.extend(16_u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(4_u32.to_le_bytes());
        wav.extend([0_u8, 0, 0, 32]);
        fs::write(&source, &wav).unwrap();
        // Keep the legacy distinct-original/shared-cache contract explicit.
        // Canonical imports intentionally reuse one material binding instead.
        fs::create_dir(&root).unwrap();
        let legacy_first = root.join("legacy-first.wav");
        let legacy_other = root.join("legacy-other.wav");
        fs::write(&legacy_first, &wav).unwrap();
        fs::write(&legacy_other, &wav).unwrap();
        let (first_pcm, first) = prepared(&root, &legacy_first, false, 2);
        let (repeat_pcm, repeat) = prepared(&root, &first.original_path, false, 2);
        let (other_pcm, other) = prepared(&root, &legacy_other, false, 2);
        assert_ne!(first.original_path, other.original_path);
        assert_eq!(first.cache_path, other.cache_path);
        assert_ne!(first.assignment_id(), repeat.assignment_id());
        let first_path = first.original_path.clone();
        let other_path = other.original_path.clone();
        let cache_path = first.cache_path.clone();
        let assets = service();
        let mut first_owner = assets.acquire(&root, &first_path).unwrap();
        let mut other_owner = assets.acquire(&root, &other_path).unwrap();
        {
            let mut state = assets.state.lock().unwrap();
            for index in 0..MAX_READER_RECORDS {
                let lease = if index == MAX_READER_RECORDS - 2 {
                    repeat.clone()
                } else if index == MAX_READER_RECORDS - 1 {
                    other.clone()
                } else {
                    first.clone()
                };
                state.readers.push(Reader {
                    path: lease.original_path.clone(),
                    pcm: vec![Arc::downgrade(&first_pcm.samples)],
                    source_pcm: None,
                    cold: Some(lease),
                    pending_assignment: false,
                    saved_claim: false,
                    engine: Weak::new(),
                    shared_stems: None,
                });
            }
        }
        drop((first_pcm, repeat_pcm, other_pcm, first, repeat, other));
        assets.collect();
        assert_eq!(
            assets.status().unwrap().1,
            2,
            "dead history must leave only one file lease per original/cache"
        );
        assets.retire(&root, &first_path, false).unwrap();
        first_owner.release();
        assets.collect();
        assert!(!first_path.exists());
        assert!(other_path.exists() && cache_path.exists());
        assets.retire(&root, &other_path, false).unwrap();
        other_owner.release();
        assets.collect();
        assert!(!other_path.exists());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while cache_path.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "final shared cache owner did not retire"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fs::read(&source).unwrap().len(), 48);

        // Canonical same-binding imports have distinct assignment authority but
        // compact to one original/cache descriptor and survive either owner.
        let (first_pcm, first) = prepared(&root, &source, true, 2);
        let (second_pcm, second) = prepared(&root, &source, true, 2);
        assert_eq!(first.original_path, second.original_path);
        assert_eq!(first.cache_path, second.cache_path);
        assert_ne!(first.assignment_id(), second.assignment_id());
        let shared_path = first.original_path.clone();
        let shared_cache = first.cache_path.clone();
        assets
            .retain_cold(first.clone(), &first_pcm, Weak::new())
            .unwrap();
        assets
            .retain_cold(second.clone(), &second_pcm, Weak::new())
            .unwrap();
        // Delivery ACK follows native reader registration, as in production.
        let mut first_owner = assets.acquire(&root, &shared_path).unwrap();
        let mut second_owner = assets.acquire(&root, &shared_path).unwrap();
        drop((first_pcm, second_pcm, first, second));
        assets.collect();
        assert_eq!(assets.status().unwrap().1, 1);
        assets.retire(&root, &shared_path, false).unwrap();
        first_owner.release();
        assets.collect();
        assert!(shared_path.exists() && shared_cache.exists());
        second_owner.release();
        assets.collect();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while shared_path.exists() || shared_cache.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "canonical last owner did not retire"
            );
            thread::sleep(Duration::from_millis(10));
        }

        // A failed prepare in another playback layout must not become the dead
        // representative ahead of a later successful layout-B warm assignment.
        fs::write(&legacy_first, &wav).unwrap();
        let (saved_pcm, saved) = prepared(&root, &legacy_first, false, 2);
        let saved_path = saved.original_path.clone();
        let (orphan_pcm, orphan) = prepared(&root, &saved_path, false, 1);
        orphan.retire_cache();
        let (valid_pcm, valid) = prepared(&root, &saved_path, false, 1);
        assert!(valid.integrity.warm);
        let saved_layout_cache = valid.cache_path.clone();
        let mut saved_owner = assets.acquire(&root, &saved_path).unwrap();
        {
            let mut state = assets.state.lock().unwrap();
            for lease in [orphan, valid] {
                state.readers.push(Reader {
                    path: lease.original_path.clone(),
                    pcm: Vec::new(),
                    source_pcm: None,
                    cold: Some(lease),
                    pending_assignment: false,
                    saved_claim: false,
                    engine: Weak::new(),
                    shared_stems: None,
                });
            }
        }
        drop((saved_pcm, saved, orphan_pcm, valid_pcm));
        assets.collect();
        // Ordinary saved shutdown releases process pins without retirement.
        saved_owner.release();
        assets.collect();
        thread::sleep(Duration::from_millis(200));
        assert!(saved_path.exists());
        assert!(
            saved_layout_cache.exists(),
            "orphan-first compaction revoked a later saved cache assignment"
        );
        assert_eq!(assets.status().unwrap().1, 1);
        // A process exit closes the bounded file-only saved descriptor, while
        // preserving its already assigned immutable cache generation on disk.
        drop(assets);
        let assets = service();
        let (_, restored) = prepared(&root, &saved_path, false, 1);
        assert!(
            restored.integrity.warm,
            "fresh saved restore must reuse layout-B cache"
        );
        assert_eq!(restored.cache_path, saved_layout_cache);
        // Model a fresh process: every prior cache/original lease and PCM reader
        // is gone, but the saved disk assets and restored project token remain.
        drop(restored);
        assert_eq!(assets.status().unwrap().1, 0);

        // A transient native job pin protects reads but never promotes a failed
        // warm publication into a saved file assignment or cancels rollback.
        let mut transient_pin = assets.acquire_pin(&root, &saved_path).unwrap();
        let (pin_pcm, pin_attempt) = prepared(&root, &saved_path, false, 1);
        let failed_pin_id = pin_attempt.assignment_id();
        assets
            .retain_cold(pin_attempt.clone(), &pin_pcm, Weak::new())
            .unwrap();
        assets.orphan_cold(&pin_attempt);
        pin_attempt.rollback_unadopted_original();
        pin_attempt.rollback_unadopted_cache();
        drop((pin_pcm, pin_attempt));
        assets.collect();
        {
            let state = assets.state.lock().unwrap();
            assert_eq!(state.readers.len(), 1);
            let descriptor = state.readers[0].cold.as_ref().unwrap();
            assert_eq!(descriptor.assignment_id(), failed_pin_id);
            assert!(descriptor.assignment_retired());
        }
        transient_pin.release();
        assets.collect();
        assert_eq!(assets.status().unwrap().1, 0);
        assert!(saved_layout_cache.exists());

        let mut restored_owner = assets.acquire(&root, &saved_path).unwrap();
        let (rejected_pcm, rejected) = prepared(&root, &saved_path, false, 1);
        let rejected_id = rejected.assignment_id();
        assert!(rejected.integrity.warm);
        assert!(rejected.integrity.playback_read_bytes > 0);
        assets
            .retain_cold(rejected.clone(), &rejected_pcm, Weak::new())
            .unwrap();
        // Exact rejected-publication guard behavior after productive preparation.
        assets.orphan_cold(&rejected);
        rejected.rollback_unadopted_original();
        rejected.rollback_unadopted_cache();
        drop((rejected_pcm, rejected));
        assets.collect();
        thread::sleep(Duration::from_millis(200));
        assert!(saved_path.exists());
        assert!(
            saved_layout_cache.exists(),
            "failed fresh-registry warm subscriber deleted another saved assignment's cache"
        );

        {
            let state = assets.state.lock().unwrap();
            assert_eq!(state.readers.len(), 1);
            let saved_descriptor = state.readers[0].cold.as_ref().unwrap();
            assert_ne!(saved_descriptor.assignment_id(), rejected_id);
            assert!(!saved_descriptor.assignment_retired());
            assert!(!saved_descriptor.created_cache());
            assert!(state.readers[0].pcm.is_empty());
            assert!(!state.readers[0].pending_assignment);
        }

        // Another original can explicitly retire its own ID in this same cache.
        // The surviving saved A file assignment must still outlive a subsequent
        // ordinary shutdown, even though B requested generation retirement.
        fs::write(&legacy_other, &wav).unwrap();
        let (shared_pcm, shared) = prepared(&root, &legacy_other, false, 1);
        assert_ne!(shared.original_path, saved_path);
        assert_eq!(shared.cache_path, saved_layout_cache);
        let shared_path = shared.original_path.clone();
        assets
            .retain_cold(shared.clone(), &shared_pcm, Weak::new())
            .unwrap();
        let mut shared_owner = assets.acquire(&root, &shared_path).unwrap();
        drop(shared_pcm);
        assets.collect();
        restored_owner.release();
        assets.collect();
        for _ in 0..32 {
            let mut cycle_owner = assets.acquire(&root, &saved_path).unwrap();
            let (cycle_pcm, cycle) = prepared(&root, &saved_path, false, 1);
            assert!(cycle.integrity.warm);
            assets
                .retain_cold(cycle.clone(), &cycle_pcm, Weak::new())
                .unwrap();
            assets.orphan_cold(&cycle);
            cycle.rollback_unadopted_original();
            cycle.rollback_unadopted_cache();
            // The saved-claim history survives this owner release before the
            // worker observes and compacts the failed warm subscriber.
            cycle_owner.release();
            drop((cycle_pcm, cycle));
            assets.collect();
            assert_eq!(assets.status().unwrap().1, 2);
            assert_eq!(
                shared.cache_assignment_count(),
                2,
                "ordinary saved shutdown left unmapped cache assignment IDs"
            );
            let state = assets.state.lock().unwrap();
            assert!(
                state
                    .readers
                    .iter()
                    .all(|reader| reader.saved_claim && reader.pcm.is_empty())
            );
        }
        drop(shared);
        assets.retire(&root, &shared_path, false).unwrap();
        shared_owner.release();
        assets.collect();
        assert!(!shared_path.exists());
        assert!(saved_path.exists() && saved_layout_cache.exists());
        restored_owner.release();
        assets.collect();
        thread::sleep(Duration::from_millis(200));
        assert_eq!(assets.status().unwrap().1, 1);
        assert!(saved_path.exists());
        assert!(
            saved_layout_cache.exists(),
            "another original's retirement erased a saved warm assignment at ordinary shutdown"
        );

        // A fresh restored assignment can still explicitly revoke A and retire
        // the known warm generation after its failed native subscriber drains.
        let mut restored_owner = assets.acquire(&root, &saved_path).unwrap();
        let (rejected_pcm, rejected) = prepared(&root, &saved_path, false, 1);
        assert!(rejected.integrity.warm);
        assets
            .retain_cold(rejected.clone(), &rejected_pcm, Weak::new())
            .unwrap();
        assets.orphan_cold(&rejected);
        rejected.rollback_unadopted_original();
        rejected.rollback_unadopted_cache();
        drop((rejected_pcm, rejected));
        assets.collect();
        assets.retire(&root, &saved_path, false).unwrap();
        restored_owner.release();
        assets.collect();
        assert!(!saved_path.exists());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while saved_layout_cache.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "final saved original revocation did not retire known warm cache"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
