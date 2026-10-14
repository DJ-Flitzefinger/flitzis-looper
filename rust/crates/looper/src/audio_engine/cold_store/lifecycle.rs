//! Store admission and bounded off-thread deletion. No callback owns these tokens.
use super::*;

const CLEANUP_ADMISSION_LIMIT: usize = 4096;
const CLEANUP_PER_POLL: usize = 8;
const MAX_REPORTED_ERRORS: usize = 16;

struct CleanupPool {
    used: std::sync::atomic::AtomicUsize,
    limit: usize,
}

pub(super) struct CleanupSlot {
    pool: Arc<CleanupPool>,
}

impl Drop for CleanupSlot {
    fn drop(&mut self) {
        self.pool.used.fetch_sub(1, Ordering::AcqRel);
    }
}

impl CleanupPool {
    fn reserve<const N: usize>(self: &Arc<Self>) -> io::Result<[CleanupSlot; N]> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(N).filter(|next| *next <= self.limit)
            })
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "PCM cleanup capacity full (4096 reserved/live/pending paths)",
                )
            })?;
        Ok(std::array::from_fn(|_| CleanupSlot { pool: self.clone() }))
    }
}

fn cleanup_pool() -> &'static Arc<CleanupPool> {
    static POOL: OnceLock<Arc<CleanupPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        Arc::new(CleanupPool {
            used: std::sync::atomic::AtomicUsize::new(0),
            limit: CLEANUP_ADMISSION_LIMIT,
        })
    })
}

pub(super) fn reserve_cleanup<const N: usize>() -> io::Result<[CleanupSlot; N]> {
    cleanup_pool().reserve()
}

pub(in crate::audio_engine) fn cleanup_admission_status() -> (usize, usize) {
    (
        cleanup_pool().used.load(Ordering::Acquire),
        CLEANUP_ADMISSION_LIMIT,
    )
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct PreparationKey {
    root: PathBuf,
    digest: String,
    rate: u32,
    channels: usize,
}

pub(super) struct DeleteTask {
    pub(super) path: PathBuf,
    root: PathBuf,
    cache: bool,
    staging: bool,
    identity: Option<FileIdentity>,
    files: Vec<FileIdentity>,
    proof: Option<Vec<crate::audio_engine::project_assets::VerifiedFile>>,
    protection: Option<Arc<crate::audio_engine::project_assets::RetirementProtection>>,
    _slot: CleanupSlot,
}

/// Only exclusive native creation produces this receipt. Parsed metadata and
/// restored paths cannot manufacture ownership; restart recovery without the
/// original creation receipt deliberately preserves the marker and root.
pub(super) struct MaterialCreation {
    pub(super) root: PathBuf,
    pub(super) original_leaf: String,
    pub(super) identity: FileIdentity,
    pub(super) binding: Option<crate::audio_engine::project_assets::VerifiedFile>,
    pub(super) pending: bool,
    pub(super) protection: Option<Arc<crate::audio_engine::project_assets::RetirementProtection>>,
    pub(super) _slot: CleanupSlot,
}

#[derive(Default)]
pub(super) struct StoreState {
    preparing: HashSet<PreparationKey>,
    opening: HashSet<PathBuf>,
    pub(super) caches: HashMap<PathBuf, Weak<CacheReaders>>,
    pub(super) originals: HashMap<PathBuf, Weak<OriginalReader>>,
    pub(super) deleting: VecDeque<DeleteTask>,
    materials: HashMap<PathBuf, MaterialCreation>,
    deleted: u64,
    errors: Vec<String>,
}

pub(super) struct Store {
    pub(super) state: Mutex<StoreState>,
    changed: Condvar,
}

pub(super) fn store() -> &'static Arc<Store> {
    static STORE: OnceLock<Arc<Store>> = OnceLock::new();
    STORE.get_or_init(|| {
        let shared = Arc::new(Store {
            state: Mutex::new(StoreState::default()),
            changed: Condvar::new(),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("pcm-file-retirement".into())
            .spawn(move || {
                loop {
                    let Ok(mut state) = worker.state.lock() else {
                        return;
                    };
                    if state.deleting.is_empty()
                        && !state.materials.values().any(|value| value.pending)
                    {
                        let Ok(next) = worker.changed.wait(state) else {
                            return;
                        };
                        state = next;
                    }
                    let count = state.deleting.len().min(CLEANUP_PER_POLL);
                    let mut prunes = Vec::with_capacity(count);
                    for _ in 0..count {
                        let task = state.deleting.pop_front().expect("bounded cleanup count");
                        let occupied = state.opening.contains(&task.path)
                            || if task.cache {
                                state
                                    .caches
                                    .get(&task.path)
                                    .is_some_and(|value| value.strong_count() > 0)
                            } else {
                                state
                                    .originals
                                    .get(&task.path)
                                    .is_some_and(|value| value.strong_count() > 0)
                            };
                        if occupied {
                            state.deleting.push_back(task);
                            continue;
                        }
                        match remove_owned(&task) {
                            Ok(()) => {
                                state.deleted += 1;
                                prunes.push((task.path.clone(), task.protection.clone()));
                            }
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                                state.deleted += 1;
                                prunes.push((task.path.clone(), task.protection.clone()));
                            }
                            Err(error)
                                if error.kind() == io::ErrorKind::PermissionDenied
                                    || matches!(error.raw_os_error(), Some(32 | 33)) =>
                            {
                                state.deleting.push_back(task)
                            }
                            Err(error) => {
                                if let Some(protection) = task.protection.as_ref() {
                                    protection.outcome.finish(Err(error.to_string()));
                                }
                                if state.errors.len() < MAX_REPORTED_ERRORS {
                                    state
                                        .errors
                                        .push(format!("{}: {error}", task.path.display()));
                                }
                            }
                        }
                    }
                    // A failed handle deletion retries through this same bounded
                    // worker; an actual external sealed marker can outlive all
                    // registered PCM/original readers.
                    prunes.extend(
                        state
                            .materials
                            .values()
                            .filter(|value| value.pending)
                            .take(CLEANUP_PER_POLL)
                            .map(|value| (value.root.join(&value.original_leaf), None)),
                    );
                    drop(state);
                    for (path, protection) in prunes {
                        super::super::project_assets::ProjectAssets::shared()
                            .prune_material_if_unowned(&path);
                        if let Some(protection) = protection {
                            protection.outcome.finish(Ok(()));
                        }
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            })
            .expect("PCM retirement worker creation");
        shared
    })
}

pub(super) fn retain_material_creation_under_gate(
    state: &mut StoreState,
    creation: MaterialCreation,
) {
    if let Some(previous) = state.materials.insert(creation.root.clone(), creation)
        && let Some(protection) = previous.protection
    {
        protection.outcome.finish(Err(
            "another exclusive root creation superseded the old receipt; preserved".into(),
        ));
    }
}

pub(super) fn known_material_creation(
    root: &Path,
) -> io::Result<
    Option<(
        FileIdentity,
        Option<crate::audio_engine::project_assets::VerifiedFile>,
        String,
    )>,
> {
    let state = store()
        .state
        .lock()
        .map_err(|_| invalid("material creation gate poisoned"))?;
    Ok(state.materials.get(root).map(|creation| {
        (
            creation.identity.clone(),
            creation.binding.clone(),
            creation.original_leaf.clone(),
        )
    }))
}

pub(super) fn restore_verified_material_creation(
    mut creation: MaterialCreation,
) -> io::Result<Option<Arc<crate::audio_engine::project_assets::RetirementOutcome>>> {
    let mut state = store()
        .state
        .lock()
        .map_err(|_| invalid("material creation gate poisoned"))?;
    if let Some(previous) = state.materials.get_mut(&creation.root) {
        if previous.identity != creation.identity
            || previous.binding != creation.binding
            || previous.original_leaf != creation.original_leaf
        {
            return Err(invalid(
                "historical material creation conflicts with retained receipt; preserved",
            ));
        }
        if let Some(protection) = previous.protection.as_ref() {
            return Ok(Some(protection.outcome.clone()));
        }
        previous.protection = creation.protection.take();
        previous.pending = previous.protection.is_some();
        if previous.pending {
            store().changed.notify_one();
        }
        return Ok(previous
            .protection
            .as_ref()
            .map(|protection| protection.outcome.clone()));
    }
    let outcome = creation
        .protection
        .as_ref()
        .map(|protection| protection.outcome.clone());
    creation.pending = creation.protection.is_some();
    retain_material_creation_under_gate(&mut state, creation);
    if outcome.is_some() {
        store().changed.notify_one();
    }
    Ok(outcome)
}

/// Reuse the retained birth's own bounded cleanup slot before exposing a leaf
/// queue. The caller holds exact canonical guards until commit or rollback.
pub(super) fn prepare_retained_material_retirement(
    root: &Path,
    identity: &FileIdentity,
    binding: &crate::audio_engine::project_assets::VerifiedFile,
    original: &str,
    protection: Arc<crate::audio_engine::project_assets::RetirementProtection>,
) -> io::Result<(
    Arc<crate::audio_engine::project_assets::RetirementOutcome>,
    bool,
)> {
    let mut state = store()
        .state
        .lock()
        .map_err(|_| invalid("material creation gate poisoned"))?;
    let previous = state
        .materials
        .get_mut(root)
        .ok_or_else(|| invalid("material birth is not restored under current native authority"))?;
    if &previous.identity != identity
        || previous.binding.as_ref() != Some(binding)
        || previous.original_leaf != original
    {
        return Err(invalid(
            "retained material birth differs from retirement receipt; preserved",
        ));
    }
    if let Some(existing) = previous.protection.as_ref() {
        return Ok((existing.outcome.clone(), false));
    }
    let outcome = protection.outcome.clone();
    previous.protection = Some(protection);
    previous.pending = false;
    Ok((outcome, true))
}

pub(super) fn finish_retained_material_retirement(
    root: &Path,
    outcome: &Arc<crate::audio_engine::project_assets::RetirementOutcome>,
    commit: bool,
) -> io::Result<()> {
    let mut state = store()
        .state
        .lock()
        .map_err(|_| invalid("material creation gate poisoned"))?;
    let previous = state
        .materials
        .get_mut(root)
        .ok_or_else(|| invalid("prepared material birth disappeared"))?;
    if !previous
        .protection
        .as_ref()
        .is_some_and(|protection| Arc::ptr_eq(&protection.outcome, outcome))
    {
        return Err(invalid(
            "prepared material retirement was cancelled by a new owner; preserved",
        ));
    }
    previous.pending = commit;
    if commit {
        store().changed.notify_one();
    } else {
        previous.protection.take();
        outcome.finish(Err(
            "original retirement was not admitted; material root preserved".into(),
        ));
    }
    Ok(())
}

pub(super) fn retain_material_creation(creation: MaterialCreation) {
    let shared = store();
    if let Ok(mut state) = shared.state.lock() {
        retain_material_creation_under_gate(&mut state, creation);
    }
}

/// Called only under ProjectAssets' current unowned/inventory/reader gate.
/// Return true whenever a creation receipt exists, even when preservation is
/// necessary, so generic scaffolding pruning cannot bypass its root identity.
pub(in crate::audio_engine) fn prune_created_material(root: &Path) -> bool {
    prune_created_material_at(root, &|_| Ok(()))
}

#[cfg(test)]
type MarkerRetiredObserver = Box<dyn FnOnce(&File) -> io::Result<()> + Send>;
#[cfg(test)]
static MARKER_RETIRED_OBSERVER: Mutex<Option<(PathBuf, MarkerRetiredObserver)>> = Mutex::new(None);

/// Exact-process test checkpoint after real verified marker deletion. The normal
/// worker still owns admission, guards, independent Inventory and cleanup slot.
#[cfg(test)]
pub(in crate::audio_engine) fn observe_material_marker_retired_for_test(
    root: &Path,
    observer: impl FnOnce(&File) -> io::Result<()> + Send + 'static,
) {
    assert!(
        MARKER_RETIRED_OBSERVER
            .lock()
            .unwrap()
            .replace((root.to_owned(), Box::new(observer)))
            .is_none()
    );
}

fn prune_created_material_at(root: &Path, checkpoint: &impl Fn(&File) -> io::Result<()>) -> bool {
    let shared = store();
    let Ok(mut state) = shared.state.lock() else {
        return true;
    };
    let Some(creation) = state.materials.get(root) else {
        return false;
    };
    let identity = creation.identity.clone();
    let binding = creation.binding.clone();
    let intersects = |path: &Path| path.starts_with(root) || root.starts_with(path);
    let occupied = state
        .originals
        .iter()
        .any(|(path, reader)| intersects(path) && reader.strong_count() > 0)
        || state
            .caches
            .iter()
            .any(|(path, reader)| intersects(path) && reader.strong_count() > 0)
        || state.opening.iter().any(|path| intersects(path))
        || state.preparing.iter().any(|key| intersects(&key.root))
        || state.deleting.iter().any(|task| intersects(&task.path));
    state
        .materials
        .get_mut(root)
        .expect("captured creation")
        .pending = false;
    if occupied {
        return true;
    }
    let result = (|| -> io::Result<()> {
        let parent = root
            .parent()
            .ok_or_else(|| invalid("material parent missing"))?;
        let _parent = crate::audio_engine::project_assets::directory_guards(parent)?;
        if !root.try_exists()? {
            return Ok(());
        }
        let guard = material::immutable_root_guard(root)?;
        if file_identity(&guard)? != identity {
            return Err(invalid("created material root was replaced; preserved"));
        }
        checkpoint(&guard)?;
        // Exact root identity precedes every child operation. Empty scaffolding
        // has no recursive deletion authority and unknown children block it.
        for path in [
            root.join("stems"),
            root.join(".pcm-cache/stems/v1/.pairs"),
            root.join(".pcm-cache/stems/v1"),
            root.join(".pcm-cache/stems"),
            root.join(".pcm-cache/v1"),
            root.join(".pcm-cache"),
        ] {
            let parent = path
                .parent()
                .ok_or_else(|| invalid("scaffolding parent missing"))?;
            let Ok(_guards) = crate::audio_engine::project_assets::directory_guards(parent) else {
                continue;
            };
            let Some(child_identity) =
                crate::audio_engine::project_assets::capture_identity(&path)?
            else {
                continue;
            };
            match crate::audio_engine::project_assets::remove_empty_directory(
                &path,
                &child_identity,
            ) {
                Ok(()) => (),
                Err(error)
                    if error.kind() == io::ErrorKind::DirectoryNotEmpty
                        || error.kind() == io::ErrorKind::NotFound =>
                {
                    ()
                }
                Err(error) => return Err(error),
            }
        }
        let entries = fs::read_dir(root)?
            .take(2)
            .collect::<io::Result<Vec<_>>>()?;
        if !entries.is_empty() {
            if entries.len() != 1 || entries[0].file_name() != ".material.json" {
                return Err(invalid("unknown material children preserve creation"));
            }
            let proof = binding
                .as_ref()
                .ok_or_else(|| invalid("material marker has no creation receipt; preserved"))?;
            // This existing helper opens DELETE + read access, checks FileID and
            // complete bytes/hash, and deletes that same handle atomically.
            crate::audio_engine::project_assets::remove_verified_file(
                &root.join(".material.json"),
                proof,
            )?;
            #[cfg(test)]
            {
                let observer = {
                    let mut pending = MARKER_RETIRED_OBSERVER
                        .lock()
                        .map_err(|_| invalid("marker retirement test observer poisoned"))?;
                    if pending
                        .as_ref()
                        .is_some_and(|(expected, _)| expected == root)
                    {
                        pending.take().map(|(_, observer)| observer)
                    } else {
                        None
                    }
                };
                if let Some(observer) = observer {
                    observer(&guard)?;
                }
            }
        }
        drop(guard);
        crate::audio_engine::project_assets::remove_empty_directory(root, &identity)
    })();
    match result {
        Ok(()) => {
            state.deleted += 1;
            if let Some(creation) = state.materials.remove(root)
                && let Some(protection) = creation.protection
            {
                protection.outcome.finish(Ok(()));
            }
        }
        Err(error)
            if error.kind() == io::ErrorKind::PermissionDenied
                || matches!(error.raw_os_error(), Some(32 | 33)) =>
        {
            state
                .materials
                .get_mut(root)
                .expect("retained creation")
                .pending = true;
            shared.changed.notify_one();
        }
        Err(error) => {
            if let Some(protection) = state
                .materials
                .get_mut(root)
                .expect("preserved creation")
                .protection
                .take()
            {
                protection.outcome.finish(Err(error.to_string()));
            }
        } // Replaced, unknown, or unreceipted data always survives.
    }
    true
}

pub(in crate::audio_engine) fn cleanup_status() -> io::Result<(usize, u64, Vec<String>)> {
    let state = store()
        .state
        .lock()
        .map_err(|_| invalid("store cleanup status poisoned"))?;
    Ok((
        state.deleting.len()
            + state
                .materials
                .values()
                .filter(|value| value.pending)
                .count(),
        state.deleted,
        state.errors.clone(),
    ))
}

#[cfg(test)]
pub(in crate::audio_engine) fn has_verified_retirement(
    path: &Path,
    outcome: &Arc<crate::audio_engine::project_assets::RetirementOutcome>,
) -> bool {
    store().state.lock().is_ok_and(|state| {
        state.deleting.iter().any(|task| {
            task.path == path
                && task
                    .protection
                    .as_ref()
                    .is_some_and(|value| Arc::ptr_eq(&value.outcome, outcome))
        })
    })
}

/// A durable assignment/job admission cancels a pending unadopted-original
/// rollback under the same gate used by deletion. Existing external files are
/// never added to the rollback queue by this admission.
pub(in crate::audio_engine) fn admit_original_owner(path: &Path) -> io::Result<()> {
    let mut state = store()
        .state
        .lock()
        .map_err(|_| invalid("original admission poisoned"))?;
    reject_links(path)?;
    let path = fs::canonicalize(path)?;
    if !path.is_file() {
        return Err(invalid("original assignment is not a file"));
    }
    state.deleting.retain(|task| {
        let keep = task.cache || task.path != path;
        if !keep && let Some(protection) = task.protection.as_ref() {
            protection.outcome.finish(Err(
                "new original owner cancels retirement; preserved".into()
            ));
        }
        keep
    });
    let original = state.originals.get(&path).and_then(Weak::upgrade);
    if let Some(root) = super::super::material_paths::material_root(&path)
        && let Some(creation) = state.materials.get_mut(root)
        && let Some(protection) = creation.protection.take()
    {
        creation.pending = false;
        protection.outcome.finish(Err(
            "new original owner cancels material retirement; preserved".into(),
        ));
    }
    if let Some(original) = original.as_ref() {
        original.rollback.store(false, Ordering::Release);
    }
    drop(state);
    drop(original);
    Ok(())
}

fn remove_owned(task: &DeleteTask) -> io::Result<()> {
    if task.staging {
        return staging::remove_generation(&task.root, &task.path, task.identity.as_ref());
    }
    // All paths originated from a guarded canonical root. Revalidate every
    // ancestor and exact leaf now; never follow a replaced directory or junction.
    reject_links(&task.root)?;
    let root = fs::canonicalize(&task.root)?;
    if !task.path.is_absolute() || !task.path.starts_with(&root) || task.path == root {
        return Err(invalid("retirement escaped its owned root"));
    }
    match fs::symlink_metadata(&task.path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
        Ok(_) => reject_links(&task.path)?,
    }
    if fs::canonicalize(&task.path)?.parent() != Some(root.as_path()) {
        return Err(invalid("retirement is not an immediate owned child"));
    }
    let _root_guard = crate::audio_engine::project_assets::directory_guards(&root)?;
    if task.cache {
        let name = task
            .path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| invalid("cache name"))?;
        if !super::super::material_paths::pcm_generation(name) {
            return Err(invalid("unrecognized owned cache directory"));
        }
        let entries = fs::read_dir(&task.path)?
            .take(4)
            .collect::<io::Result<Vec<_>>>()?;
        if entries.len() > 3
            || entries.iter().any(|entry| {
                !matches!(
                    entry.file_name().to_str(),
                    Some("decoder.f32le" | "playback.f32le" | "manifest.json")
                )
            })
        {
            return Err(invalid("unknown cache files prevent retirement"));
        }
        // No recursive deletion: exact owned ordinary leaves, then the empty
        // generation. Partial retries tolerate already removed owned leaves.
        for entry in entries {
            reject_links(&entry.path())?;
            if !entry.file_type()?.is_file() {
                return Err(invalid("cache child is not a file"));
            }
        }
        let generation_guard = directory_guard(&task.path)?;
        if task.identity.as_ref() != Some(&file_identity(&generation_guard)?) {
            return Err(invalid("owned cache generation was replaced"));
        }
        if let Some(proof) = &task.proof {
            for file in proof {
                match crate::audio_engine::project_assets::verify_file_proof(
                    &task.path.join(&file.name),
                    file,
                ) {
                    Ok(()) => (),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                    Err(error) => return Err(error),
                }
            }
        }
        for (index, name) in ["decoder.f32le", "playback.f32le", "manifest.json"]
            .into_iter()
            .enumerate()
        {
            let result = if let Some(proof) = &task.proof {
                crate::audio_engine::project_assets::remove_verified_file(
                    &task.path.join(name),
                    &proof[index],
                )
            } else {
                crate::audio_engine::project_assets::remove_owned_file(
                    &task.path.join(name),
                    task.files.get(index),
                )
            };
            match result {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        drop(generation_guard);
        fs::remove_dir(&task.path)
    } else {
        crate::audio_engine::project_assets::remove_owned_file(&task.path, task.identity.as_ref())
    }
}

fn queue_delete(
    path: PathBuf,
    root: PathBuf,
    cache: bool,
    identity: FileIdentity,
    files: Vec<FileIdentity>,
    slot: CleanupSlot,
) {
    let shared = store();
    if let Ok(mut state) = shared.state.lock() {
        if !state.deleting.iter().any(|task| task.path == path) {
            state.deleting.push_back(DeleteTask {
                path,
                root,
                cache,
                staging: false,
                identity: Some(identity),
                files,
                proof: None,
                protection: None,
                _slot: slot,
            });
        }
        shared.changed.notify_one();
    }
}

pub(super) fn queue_generation(
    path: PathBuf,
    root: PathBuf,
    identity: FileIdentity,
    slot: CleanupSlot,
) {
    let shared = store();
    if let Ok(mut state) = shared.state.lock() {
        if !state.deleting.iter().any(|task| task.path == path) {
            state.deleting.push_back(DeleteTask {
                path,
                root,
                cache: false,
                staging: true,
                identity: Some(identity),
                files: Vec::new(),
                proof: None,
                protection: None,
                _slot: slot,
            });
        }
        shared.changed.notify_one();
    }
}

pub(super) fn queue_generation_under_gate(
    state: &mut StoreState,
    path: PathBuf,
    root: PathBuf,
    identity: FileIdentity,
    slot: CleanupSlot,
) {
    if !state.deleting.iter().any(|task| task.path == path) {
        state.deleting.push_back(DeleteTask {
            path,
            root,
            cache: false,
            staging: true,
            identity: Some(identity),
            files: Vec::new(),
            proof: None,
            protection: None,
            _slot: slot,
        });
    }
    store().changed.notify_one();
}

pub(super) fn queue_original(
    path: PathBuf,
    root: PathBuf,
    identity: FileIdentity,
    slot: CleanupSlot,
) {
    queue_delete(path, root, false, identity, Vec::new(), slot);
}

pub(in crate::audio_engine) struct PreparationGuard {
    key: PreparationKey,
}

fn acquire_preparation_gate(
    key: PreparationKey,
    cancelled: &impl Fn() -> bool,
) -> io::Result<PreparationGuard> {
    let shared = store();
    loop {
        // Cancellation can inspect native requests. Keep it outside the store
        // lock, consistently with OpeningGuard's existing admission protocol.
        check_cancelled(cancelled)?;
        let mut state = shared
            .state
            .lock()
            .map_err(|_| invalid("preparation gate poisoned"))?;
        state.caches.retain(|_, cache| cache.strong_count() > 0);
        state
            .originals
            .retain(|_, original| original.strong_count() > 0);
        if state.deleting.len() >= CLEANUP_ADMISSION_LIMIT
            || (state.preparing.len() >= CLEANUP_ADMISSION_LIMIT && !state.preparing.contains(&key))
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "PCM preparation/retirement backlog admission limit",
            ));
        }
        if state.preparing.insert(key.clone()) {
            return Ok(PreparationGuard { key });
        }
        let (state, _) = shared
            .changed
            .wait_timeout(state, Duration::from_millis(10))
            .map_err(|_| invalid("preparation gate poisoned"))?;
        drop(state);
    }
}

impl Drop for PreparationGuard {
    fn drop(&mut self) {
        let shared = store();
        if let Ok(mut state) = shared.state.lock() {
            state.preparing.remove(&self.key);
            shared.changed.notify_all();
        }
    }
}

pub(super) struct OpeningGuard {
    path: PathBuf,
}

impl OpeningGuard {
    pub(super) fn acquire(path: &Path, cancelled: &impl Fn() -> bool) -> io::Result<Self> {
        let shared = store();
        loop {
            // Cancellation may inspect request ownership; do not call it while
            // holding the store mutex. This admission runs only on cold workers.
            check_cancelled(cancelled)?;
            let mut state = shared
                .state
                .lock()
                .map_err(|_| invalid("cache admission poisoned"))?;
            if state.opening.insert(path.to_owned()) {
                return Ok(Self {
                    path: path.to_owned(),
                });
            }
            // A busy candidate can still be the complete compatible cache.
            // Release the mutex while waiting and recheck cancellation/predicate.
            let (state, _) = shared
                .changed
                .wait_timeout(state, Duration::from_millis(10))
                .map_err(|_| invalid("cache admission poisoned"))?;
            drop(state);
        }
    }
}

impl Drop for OpeningGuard {
    fn drop(&mut self) {
        let shared = store();
        if let Ok(mut state) = shared.state.lock() {
            state.opening.remove(&self.path);
            shared.changed.notify_all();
        }
    }
}

impl Drop for CacheReaders {
    fn drop(&mut self) {
        self.readers.clear();
        self.directories.clear();
        if self.retired.load(Ordering::Acquire)
            && self
                .durable_assignments
                .get_mut()
                .is_ok_and(|owners| owners.is_empty())
        {
            queue_delete(
                self.path.clone(),
                self.root.clone(),
                true,
                self.identity.clone(),
                self.file_identities.clone(),
                self.cleanup
                    .take()
                    .expect("cache retirement capacity reserved"),
            );
        }
    }
}

impl Drop for OriginalReader {
    fn drop(&mut self) {
        self.reader.take();
        self._binding_reader.take();
        self.directories.clear();
        if self.owned_creation
            && self.rollback.load(Ordering::Acquire)
            && self
                .durable_assignments
                .get_mut()
                .is_ok_and(|owners| owners.is_empty())
        {
            queue_delete(
                self.path.clone(),
                self.root.clone(),
                false,
                self.identity.clone(),
                Vec::new(),
                self.cleanup
                    .take()
                    .expect("created original retirement capacity reserved"),
            );
        }
    }
}

/// Verified off-thread file custody and reserved cleanup capacity. Consuming
/// this token registers dormant ownership without reopening or reading files.
pub(in crate::audio_engine) struct PreparedMaterialCreationOwnership {
    creation: MaterialCreation,
    _guards: Vec<File>,
}

impl PreparedMaterialCreationOwnership {
    pub(in crate::audio_engine) fn finish(self) -> io::Result<()> {
        restore_verified_material_creation(self.creation)?;
        Ok(())
    }
}

impl CommittedColdLease {
    pub(in crate::audio_engine) fn material_creation_receipt(
        &self,
    ) -> Option<MaterialCreationReceipt> {
        // Every current canonical owner carries the same actual birth fact.
        // This does not transfer this lease's artifact creation/rollback rights.
        if self.assignment_retired() || self.material_id.is_none() {
            return None;
        }
        material::material_creation_receipt(&self.samples_root, &self.original_path)
            .ok()
            .flatten()
    }
    /// Current native ACK/config/journal validation is required at the recovery
    /// caller. This registers dormant historical ownership only: it retains no
    /// inventory lock and grants no original or descendant retirement rights.
    pub(in crate::audio_engine) fn prepare_material_creation_restore(
        &self,
        receipt: &MaterialCreationReceipt,
    ) -> io::Result<PreparedMaterialCreationOwnership> {
        if self.assignment_retired() || self.material_id.is_none() {
            return Err(invalid(
                "creation ownership restore requires current canonical lease",
            ));
        }
        let original = &self.manifest.descriptor["decoder"]["original"];
        let proof = crate::audio_engine::project_assets::VerifiedFile {
            name: self
                .original_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| invalid("restored original leaf"))?
                .to_owned(),
            identity: self._original.identity.clone(),
            bytes: original["bytes"]
                .as_u64()
                .ok_or_else(|| invalid("restored complete original extent"))?,
            sha256: original["sha256"]
                .as_str()
                .ok_or_else(|| invalid("restored complete original digest"))?
                .to_owned(),
        };
        let [slot] = reserve_cleanup::<1>()?;
        let guards = material::verify_material_creation(
            &self.samples_root,
            &self.original_path,
            receipt,
            &proof,
        )?;
        let root = super::super::material_paths::material_root(&self.original_path)
            .ok_or_else(|| invalid("restored readable root missing"))?;
        Ok(PreparedMaterialCreationOwnership {
            creation: MaterialCreation {
                root: root.to_owned(),
                original_leaf: proof.name,
                identity: receipt.root_identity.clone(),
                binding: Some(receipt.binding.clone()),
                pending: false,
                protection: None,
                _slot: slot,
            },
            _guards: guards,
        })
    }
    pub(in crate::audio_engine) fn samples_root(&self) -> &Path {
        &self.samples_root
    }
    #[cfg(test)]
    pub(in crate::audio_engine) fn reader_lifetime_probe_for_test(
        &self,
    ) -> impl Fn() -> bool + use<> {
        let cache = Arc::downgrade(&self.cache);
        let original = Arc::downgrade(&self._original);
        move || cache.strong_count() == 0 && original.strong_count() == 0
    }

    /// Serialize canonical stem preparation for this verified material/format,
    /// independently of the legacy slot and caller's private generation ID.
    pub(in crate::audio_engine) fn stem_preparation_gate(
        &self,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<PreparationGuard> {
        if self.assignment_retired() || self.material_id.is_none() {
            return Err(invalid(
                "stem preparation requires a current material assignment",
            ));
        }
        let material_root = super::super::material_paths::material_root_for_original(
            self.samples_root(),
            &self.original_path,
        )?;
        let stems = material_root.join("stems");
        let _guards = super::super::project_assets::directory_guards(&stems)?;
        let root = fs::canonicalize(stems)?;
        reject_links(&root)?;
        let pcm = &self.manifest.descriptor["playback"]["pcm"];
        let rate = u32::try_from(
            pcm["rate_hz"]
                .as_u64()
                .ok_or_else(|| invalid("stem playback rate missing"))?,
        )
        .map_err(|_| invalid("stem playback rate overflow"))?;
        let channels = usize::try_from(
            pcm["channels"]
                .as_u64()
                .ok_or_else(|| invalid("stem playback channels missing"))?,
        )
        .map_err(|_| invalid("stem playback channels overflow"))?;
        if rate == 0 || channels == 0 {
            return Err(invalid("stem preparation format is empty"));
        }
        acquire_preparation_gate(
            PreparationKey {
                root,
                digest: self.manifest.identity.clone(),
                rate,
                channels,
            },
            cancelled,
        )
    }

    /// Register an independent subscriber against still-sealed current backing.
    /// Clone deliberately retains the old assignment; this method does not.
    pub(in crate::audio_engine) fn subscribe_verified(&self) -> io::Result<Self> {
        let state = store()
            .state
            .lock()
            .map_err(|_| invalid("subscriber store ownership poisoned"))?;
        let mut cache_owners = self
            .cache
            .durable_assignments
            .lock()
            .map_err(|_| invalid("subscriber cache ownership poisoned"))?;
        let mut original_owners = self
            ._original
            .durable_assignments
            .lock()
            .map_err(|_| invalid("subscriber original ownership poisoned"))?;
        if self.assignment_retired()
            || self.cache.retired.load(Ordering::Acquire)
            || !cache_owners.contains(&self.assignment.id)
            || !original_owners.contains(&self.assignment.id)
            || state
                .deleting
                .iter()
                .any(|task| task.path == self.cache_path || task.path == self.original_path)
        {
            return Err(invalid("subscriber source assignment is retired"));
        }
        if cache_owners.len() >= CLEANUP_ADMISSION_LIMIT
            || original_owners.len() >= CLEANUP_ADMISSION_LIMIT
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "subscriber ownership capacity full",
            ));
        }
        let _cache_guards = super::super::project_assets::directory_guards(&self.cache.root)?;
        let _original_guards =
            super::super::project_assets::directory_guards(&self._original.root)?;
        let samples_root = self.samples_root();
        let original_kind =
            super::super::material_paths::resolve(samples_root, &self.original_path)?.kind;
        if original_kind
            != (super::super::material_paths::AssetKind::Original {
                material: self.material_id.clone(),
            })
            || super::super::material_paths::resolve(samples_root, &self.cache_path)?.kind
                != super::super::material_paths::AssetKind::PcmDirectory
        {
            return Err(invalid("subscriber typed material binding changed"));
        }
        reject_links(&self.original_path)?;
        reject_links(&self.cache_path)?;
        let directory = directory_guard(&self.cache_path)?;
        if file_identity(&directory)? != self.cache.identity
            || fs::canonicalize(&self.cache_path)?.parent() != Some(self.cache.root.as_path())
            || self.cache_path != self.cache.path
            || self.original_path != self._original.path
        {
            return Err(invalid("subscriber cache/original binding changed"));
        }
        let original = sealed_reader(&self.original_path)?;
        let held_original = self
            ._original
            .reader
            .as_ref()
            .ok_or_else(|| invalid("subscriber sealed original missing"))?;
        if file_identity(&original)? != self._original.identity
            || file_identity(held_original)? != self._original.identity
            || original.metadata()?.len()
                != self.manifest.descriptor["decoder"]["original"]["bytes"]
                    .as_u64()
                    .ok_or_else(|| invalid("subscriber original extent missing"))?
        {
            return Err(invalid("subscriber original FileID or extent changed"));
        }
        let expected_manifest =
            serde_json::to_vec(&canonical(&self.manifest.encoded())).map_err(io::Error::other)?;
        if expected_manifest.len() > 256 * 1024 || self.cache.readers.len() != 3 {
            return Err(invalid(
                "subscriber manifest/readers exceed complete contract",
            ));
        }
        let mut readers = Vec::with_capacity(3);
        for (index, name) in ["decoder.f32le", "playback.f32le", "manifest.json"]
            .into_iter()
            .enumerate()
        {
            let path = self.cache_path.join(name);
            reject_links(&path)?;
            let mut reader = sealed_reader(&path)?;
            if self.cache.file_identities.get(index) != Some(&file_identity(&reader)?)
                || self.cache.file_identities.get(index)
                    != Some(&file_identity(&self.cache.readers[index])?)
            {
                return Err(invalid("subscriber backing FileID changed"));
            }
            let expected_bytes = match index {
                0 => self.manifest.descriptor["decoder"]["pcm"]["full_bytes"].as_u64(),
                1 => self.manifest.descriptor["playback"]["pcm"]["full_bytes"].as_u64(),
                _ => Some(expected_manifest.len() as u64),
            }
            .ok_or_else(|| invalid("subscriber complete artifact extent missing"))?;
            if reader.metadata()?.len() != expected_bytes {
                return Err(invalid("subscriber complete artifact extent changed"));
            }
            if index == 2 {
                let mut actual = vec![0; expected_manifest.len()];
                reader.read_exact(&mut actual)?;
                if actual != expected_manifest || reader.read(&mut [0])? != 0 {
                    return Err(invalid("subscriber complete manifest binding changed"));
                }
            }
            readers.push(reader);
        }
        // Both locks and both capacities are established before either set is
        // mutated. Allocation/ID exhaustion therefore leaves all existing users intact.
        cache_owners.try_reserve(1).map_err(io::Error::other)?;
        original_owners.try_reserve(1).map_err(io::Error::other)?;
        let id = NEXT_GENERATION
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
                id.checked_add(1).filter(|_| id != 0)
            })
            .map_err(|_| invalid("subscriber assignment ID exhausted"))?;
        if cache_owners.contains(&id) || original_owners.contains(&id) {
            return Err(invalid("subscriber assignment ID already registered"));
        }
        let assignment = Arc::new(CacheAssignment {
            id,
            retired: AtomicBool::new(false),
        });
        cache_owners.insert(id);
        original_owners.insert(id);
        Ok(Self {
            assignment,
            created_original: false,
            created_cache: false,
            ..self.clone()
        })
    }

    /// Release only the preparation parent's registration, without physical rollback.
    pub(in crate::audio_engine) fn release_preparation_assignment(&self) {
        self.retire_assignment(false);
    }

    pub(in crate::audio_engine) fn prune_dead_pcm(&self) {
        if let Ok(mut pcm) = self.cache.pcm.lock()
            && pcm.as_ref().is_some_and(|pcm| pcm.strong_count() == 0)
        {
            *pcm = None;
        }
        if let Ok(mut windows) = self.cache.windows.lock() {
            windows.prune();
        }
    }
    pub(in crate::audio_engine) fn assignment_id(&self) -> u64 {
        self.assignment.id
    }
    #[cfg(test)]
    pub(in crate::audio_engine) fn cache_assignment_count(&self) -> usize {
        self.cache
            .durable_assignments
            .lock()
            .expect("test cache ownership")
            .len()
    }
    pub(in crate::audio_engine) fn assignment_retired(&self) -> bool {
        self.assignment.retired.load(Ordering::Acquire)
    }
    pub(in crate::audio_engine) fn created_cache(&self) -> bool {
        self.created_cache
    }
    pub(in crate::audio_engine) fn created_original(&self) -> bool {
        self.created_original
    }

    /// Fully evidenced recovery uses the existing reader-aware bounded queue.
    /// The caller holds the ProjectAssets admission gate and sealed proof handles.
    pub(in crate::audio_engine) fn queue_verified_retirement(
        path: &Path,
        identity: &FileIdentity,
        proof: &[crate::audio_engine::project_assets::VerifiedFile],
        protection: Option<Arc<crate::audio_engine::project_assets::RetirementProtection>>,
    ) -> io::Result<()> {
        let names = ["decoder.f32le", "playback.f32le", "manifest.json"];
        if proof.len() != names.len()
            || proof
                .iter()
                .zip(names)
                .any(|(file, name)| file.name != name)
        {
            return Err(invalid(
                "PCM retirement requires exact complete three-leaf evidence",
            ));
        }
        let root = path
            .parent()
            .ok_or_else(|| invalid("PCM retirement root missing"))?;
        let _guards = crate::audio_engine::project_assets::directory_guards(path)?;
        let shared = store();
        let mut state = shared
            .state
            .lock()
            .map_err(|_| invalid("store retirement gate poisoned"))?;
        if crate::audio_engine::project_assets::capture_identity(path)?.as_ref() != Some(identity)
            || proof.iter().any(|file| {
                crate::audio_engine::project_assets::capture_identity(&path.join(&file.name))
                    .ok()
                    .flatten()
                    .as_ref()
                    != Some(&file.identity)
            })
        {
            return Err(invalid("verified PCM generation changed before retirement"));
        }
        if let Some(previous) = state.deleting.iter_mut().find(|task| task.path == path) {
            if !previous.cache
                || previous.identity.as_ref() != Some(identity)
                || previous
                    .proof
                    .as_ref()
                    .is_some_and(|previous| previous != proof)
            {
                return Err(invalid("PCM queued retirement evidence changed"));
            }
            previous.proof = Some(proof.to_vec());
            if let Some(current) = previous.protection.as_ref() {
                if protection
                    .as_ref()
                    .is_none_or(|next| !Arc::ptr_eq(&current.outcome, &next.outcome))
                {
                    return Err(invalid("PCM retirement has another held inventory outcome"));
                }
            } else {
                previous.protection = protection;
            }
            return Ok(());
        }
        let [slot] = reserve_cleanup::<1>()?;
        state.deleting.push_back(DeleteTask {
            path: path.to_owned(),
            root: root.to_owned(),
            cache: true,
            staging: false,
            identity: Some(identity.clone()),
            files: proof.iter().map(|file| file.identity.clone()).collect(),
            proof: Some(proof.to_vec()),
            protection,
            _slot: slot,
        });
        shared.changed.notify_one();
        Ok(())
    }
    pub(in crate::audio_engine) fn original_identity_matches(
        &self,
        identity: &FileIdentity,
    ) -> bool {
        self._original.identity.eq(identity)
    }
    /// A surviving saved original can retain already verified warm files with
    /// independent ownership, without reviving a failed native attempt's ID.
    pub(in crate::audio_engine) fn fork_saved_assignment(&self) -> Self {
        assert!(!self.created_cache && self.assignment_retired());
        let assignment = Arc::new(CacheAssignment {
            id: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            retired: AtomicBool::new(false),
        });
        self.cache
            .durable_assignments
            .lock()
            .expect("verified saved cache ownership")
            .insert(assignment.id);
        self._original
            .durable_assignments
            .lock()
            .expect("verified saved original ownership")
            .insert(assignment.id);
        Self {
            assignment,
            created_original: false,
            created_cache: false,
            ..self.clone()
        }
    }
    pub(in crate::audio_engine) fn bind_pcm(&self, samples: &Arc<[f32]>) {
        // The shared warm locator may reference only complete backing PCM.
        if self.manifest.descriptor["playback"]["pcm"]["full_bytes"].as_u64()
            != samples.len().checked_mul(4).map(|n| n as u64)
        {
            return;
        }
        if let Ok(mut pcm) = self.cache.pcm.lock() {
            *pcm = Some(Arc::downgrade(samples));
        }
    }
    pub(in crate::audio_engine) fn bind_sample(&self, sample: &SampleBuffer) -> io::Result<()> {
        self.bind_pcm(&sample.samples);
        self.cache.remember_verified_window(sample)
    }
    pub(in crate::audio_engine) fn retire_cache(&self) {
        self.retire_assignment(true);
    }
    /// Failed subscribers can revoke their assignment, but only their newly
    /// created generation can acquire physical rollback rights from that failure.
    pub(in crate::audio_engine) fn rollback_unadopted_cache(&self) {
        self.retire_assignment(self.created_cache);
    }
    fn retire_assignment(&self, request_cache_retirement: bool) {
        if !self.assignment.retired.swap(true, Ordering::AcqRel) {
            if let Ok(mut owners) = self.cache.durable_assignments.lock() {
                owners.remove(&self.assignment.id);
            }
            if let Ok(mut owners) = self._original.durable_assignments.lock() {
                owners.remove(&self.assignment.id);
            }
        }
        if request_cache_retirement {
            self.cache.retired.store(true, Ordering::Release);
        }
    }
    pub(in crate::audio_engine) fn rollback_unadopted_original(&self) {
        if self.created_original {
            self._original.rollback.store(true, Ordering::Release);
            if let Ok(mut owners) = self._original.durable_assignments.lock() {
                owners.remove(&self.assignment.id);
            }
        }
    }
}

impl ColdTransaction {
    /// Retain only weak locators for the actual independently admitted view.
    pub(in crate::audio_engine) fn bind_sample(&mut self, sample: &SampleBuffer) -> io::Result<()> {
        self.bind_pcm(&sample.samples);
        self.shared_cache
            .as_ref()
            .expect("bound committed cache")
            .remember_verified_window(sample)
    }

    /// Install sealed committed ownership and the PCM weak reference before
    /// releasing the digest gate. Followers can then share PCM even while the
    /// producer is waiting for its independent native ACK.
    pub(in crate::audio_engine) fn bind_pcm(&mut self, samples: &Arc<[f32]>) {
        assert!(self.verified);
        let complete = self.manifest.as_ref().is_some_and(|manifest| {
            manifest.descriptor["playback"]["pcm"]["full_bytes"].as_u64()
                == samples.len().checked_mul(4).map(|n| n as u64)
        });
        let cache = self
            .reused_cache
            .clone()
            .or_else(|| self.shared_cache.clone())
            .unwrap_or_else(|| {
                let mut directories = std::mem::take(&mut self.directories);
                directories.push(self.committed_directory.take().expect("committed guard"));
                let path = self
                    .committed_cache
                    .as_ref()
                    .expect("committed cache")
                    .clone();
                let cache = Arc::new(CacheReaders {
                    path: path.clone(),
                    root: self.cache_root.clone(),
                    readers: std::mem::take(&mut self.artifact_readers),
                    directories,
                    pcm: Mutex::new(None),
                    windows: Mutex::new(Default::default()),
                    retired: AtomicBool::new(false),
                    durable_assignments: Mutex::new(HashSet::new()),
                    identity: self.committed_identity.take().expect("committed identity"),
                    file_identities: std::mem::take(&mut self.artifact_identities),
                    cleanup: self.staging_slot.take(),
                });
                if let Ok(mut state) = store().state.lock() {
                    state.caches.insert(path, Arc::downgrade(&cache));
                }
                cache
            });
        if complete && let Ok(mut pcm) = cache.pcm.lock() {
            *pcm = Some(Arc::downgrade(samples));
        }
        self.shared_cache = Some(cache);
    }
    /// Different assignments keep independent cancellation. A cancelled producer
    /// releases only its reservation; another waiting subscriber can regenerate.
    pub(in crate::audio_engine) fn preparation_gate(
        &self,
        rate: u32,
        channels: usize,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<PreparationGuard> {
        acquire_preparation_gate(
            PreparationKey {
                root: self.cache_root.clone(),
                digest: self.source_digest.clone(),
                rate,
                channels,
            },
            cancelled,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn material_prune_keeps_exact_root_write_custody_until_child_operations_return() {
        use std::cell::Cell;
        use std::os::windows::fs::OpenOptionsExt;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("owned root");
        fs::create_dir(&root).unwrap();
        let unknown = root.join("unknown-private.txt");
        fs::write(&unknown, b"preserved unknown bytes").unwrap();
        let identity = file_identity(&directory_guard(&root).unwrap()).unwrap();
        let [slot] = reserve_cleanup::<1>().unwrap();
        retain_material_creation(MaterialCreation {
            root: root.clone(),
            original_leaf: "source.wav".into(),
            identity: identity.clone(),
            binding: None,
            pending: false,
            protection: None,
            _slot: slot,
        });
        let write_root = || {
            OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(7)
                .custom_flags(0x0200_0000 | 0x0020_0000)
                .open(&root)
        };
        let entered = Cell::new(false);
        assert!(prune_created_material_at(&root, &|guard| {
            entered.set(true);
            assert_eq!(file_identity(guard)?, identity);
            let denied = write_root().unwrap_err();
            assert!(matches!(denied.raw_os_error(), Some(32 | 33)));
            assert!(fs::rename(&root, temp.path().join("replacement")).is_err());
            Ok(())
        }));
        assert!(entered.get());
        // Positive control: the same actual root permits a write-capable open
        // after the pruning guard is released. Unknown bytes blocked deletion.
        assert_eq!(file_identity(&write_root().unwrap()).unwrap(), identity);
        assert_eq!(fs::read(&unknown).unwrap(), b"preserved unknown bytes");
        store().state.lock().unwrap().materials.remove(&root);
    }

    #[test]
    fn cleanup_reservations_bound_live_and_pending_owners_before_capture_writes() {
        let pool = Arc::new(CleanupPool {
            used: std::sync::atomic::AtomicUsize::new(0),
            limit: 6,
        });
        let temp = tempfile::tempdir().unwrap();
        let samples = temp.path().join("samples");
        let source = temp.path().join("source.wav");
        fs::write(&source, b"external unchanged").unwrap();
        let held = pool.reserve::<6>().unwrap();
        let result = pool.reserve::<4>().and_then(|slots| {
            ColdTransaction::capture_reserved(&samples, &source, true, &|| false, slots)
        });
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::WouldBlock);
        assert!(!samples.exists());
        assert_eq!(fs::read(&source).unwrap(), b"external unchanged");
        assert_eq!(pool.used.load(Ordering::Acquire), 6);
        // Transfer, rather than recreate, owner slots to a pending queue.
        let mut pending = VecDeque::from(held);
        assert!(pool.reserve::<1>().is_err());
        drop(pending.pop_front());
        let replacement = pool.reserve::<1>().unwrap();
        assert_eq!(pool.used.load(Ordering::Acquire), 6);
        drop(replacement);
        drop(pending);
        assert_eq!(pool.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn concurrent_atomic_cleanup_admission_never_overshoots_limit() {
        let pool = Arc::new(CleanupPool {
            used: std::sync::atomic::AtomicUsize::new(0),
            limit: 6,
        });
        let admitted = std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..32)
                .map(|_| {
                    let pool = pool.clone();
                    scope.spawn(move || pool.reserve::<1>().ok())
                })
                .collect();
            jobs.into_iter()
                .filter_map(|job| job.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(admitted.len(), 6);
        assert_eq!(pool.used.load(Ordering::Acquire), 6);
        drop(admitted);
        assert_eq!(pool.used.load(Ordering::Acquire), 0);
    }
}
