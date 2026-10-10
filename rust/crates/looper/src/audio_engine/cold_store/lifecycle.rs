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
    _slot: CleanupSlot,
}

#[derive(Default)]
pub(super) struct StoreState {
    preparing: HashSet<PreparationKey>,
    opening: HashSet<PathBuf>,
    pub(super) caches: HashMap<PathBuf, Weak<CacheReaders>>,
    pub(super) originals: HashMap<PathBuf, Weak<OriginalReader>>,
    pub(super) deleting: VecDeque<DeleteTask>,
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
                    if state.deleting.is_empty() {
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
                                prunes.push(task.path.clone());
                            }
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                                state.deleted += 1
                            }
                            Err(error)
                                if error.kind() == io::ErrorKind::PermissionDenied
                                    || matches!(error.raw_os_error(), Some(32 | 33)) =>
                            {
                                state.deleting.push_back(task)
                            }
                            Err(error) => {
                                if state.errors.len() < MAX_REPORTED_ERRORS {
                                    state
                                        .errors
                                        .push(format!("{}: {error}", task.path.display()));
                                }
                            }
                        }
                    }
                    drop(state);
                    for path in prunes {
                        super::super::project_assets::ProjectAssets::shared()
                            .prune_material_if_unowned(&path);
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            })
            .expect("PCM retirement worker creation");
        shared
    })
}

pub(in crate::audio_engine) fn cleanup_status() -> io::Result<(usize, u64, Vec<String>)> {
    let state = store()
        .state
        .lock()
        .map_err(|_| invalid("store cleanup status poisoned"))?;
    Ok((state.deleting.len(), state.deleted, state.errors.clone()))
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
    state
        .deleting
        .retain(|task| task.cache || task.path != path);
    let original = state.originals.get(&path).and_then(Weak::upgrade);
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
        for (index, name) in ["decoder.f32le", "playback.f32le", "manifest.json"]
            .into_iter()
            .enumerate()
        {
            match crate::audio_engine::project_assets::remove_owned_file(
                &task.path.join(name),
                task.files.get(index),
            ) {
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

impl CommittedColdLease {
    pub(in crate::audio_engine) fn prune_dead_pcm(&self) {
        if let Ok(mut pcm) = self.cache.pcm.lock()
            && pcm.as_ref().is_some_and(|pcm| pcm.strong_count() == 0)
        {
            *pcm = None;
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
    /// Install sealed committed ownership and the PCM weak reference before
    /// releasing the digest gate. Followers can then share PCM even while the
    /// producer is waiting for its independent native ACK.
    pub(in crate::audio_engine) fn bind_pcm(&mut self, samples: &Arc<[f32]>) {
        assert!(self.verified);
        let complete = self.manifest.as_ref().is_some_and(|manifest| {
            manifest.descriptor["playback"]["pcm"]["full_bytes"].as_u64()
                == samples.len().checked_mul(4).map(|n| n as u64)
        });
        let cache = self.reused_cache.clone().unwrap_or_else(|| {
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
        let key = PreparationKey {
            root: self.cache_root.clone(),
            digest: self.source_digest.clone(),
            rate,
            channels,
        };
        let shared = store();
        let mut state = shared
            .state
            .lock()
            .map_err(|_| invalid("preparation gate poisoned"))?;
        loop {
            check_cancelled(cancelled)?;
            state.caches.retain(|_, cache| cache.strong_count() > 0);
            state
                .originals
                .retain(|_, original| original.strong_count() > 0);
            if state.deleting.len() >= CLEANUP_ADMISSION_LIMIT {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "PCM retirement backlog admission limit",
                ));
            }
            if state.preparing.insert(key.clone()) {
                return Ok(PreparationGuard { key });
            }
            state = shared
                .changed
                .wait_timeout(state, Duration::from_millis(10))
                .map_err(|_| invalid("preparation gate poisoned"))?
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let result = pool.reserve::<3>().and_then(|slots| {
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
