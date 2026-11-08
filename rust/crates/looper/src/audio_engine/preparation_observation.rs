//! Request-carried test observations; only fixed atomic stamps touch the callback.

use super::loader_overlap_observation::current_worker;
use std::cell::{Cell, RefCell};
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use std::time::Instant;

thread_local! {
    static WORKERS: Cell<usize> = const { Cell::new(0) };
    static FAIL_SPAWN: Cell<Option<usize>> = const { Cell::new(None) };
    static SPAWNED: RefCell<Option<Vec<std::os::windows::io::OwnedHandle>>> = const { RefCell::new(None) };
}

pub(super) fn workers(default: usize) -> usize {
    WORKERS.with(|workers| match workers.get() {
        0 => default,
        count => count,
    })
}

pub(super) fn with_workers<T>(count: usize, run: impl FnOnce() -> T) -> T {
    assert!((1..=2).contains(&count));
    struct Restore(usize);
    impl Drop for Restore {
        fn drop(&mut self) {
            WORKERS.with(|workers| workers.set(self.0));
        }
    }
    let _restore = Restore(WORKERS.with(|workers| workers.replace(count)));
    run()
}

pub(super) fn before_spawn(index: usize) -> std::io::Result<()> {
    if FAIL_SPAWN.with(|failure| failure.get() == Some(index)) {
        Err(std::io::Error::other(
            "injected action worker spawn failure",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn after_spawn(join: &std::thread::JoinHandle<()>) {
    use std::os::windows::io::AsHandle;
    SPAWNED.with_borrow_mut(|spawned| {
        if let Some(spawned) = spawned {
            spawned.push(join.as_handle().try_clone_to_owned().unwrap());
        }
    });
}

#[derive(Default)]
pub(super) struct PoolObservation {
    checks: AtomicU64,
    started: AtomicU64,
    pending_at_check: AtomicUsize,
}
impl PoolObservation {
    pub(super) fn endpoint_started(&self) {
        self.started.fetch_add(1, Ordering::Relaxed);
    }
    pub(super) fn endpoint_checked(&self, pending: usize) {
        self.pending_at_check.store(pending, Ordering::Relaxed);
        self.checks.fetch_add(1, Ordering::Release);
    }
    pub(super) fn snapshot(&self) -> (u64, u64, usize) {
        let checks = self.checks.load(Ordering::Acquire);
        (
            checks,
            self.started.load(Ordering::Acquire),
            self.pending_at_check.load(Ordering::Acquire),
        )
    }
}

pub(super) struct SpawnFailureProbe;
impl SpawnFailureProbe {
    pub(super) fn begin(index: usize) -> Self {
        FAIL_SPAWN.with(|failure| assert!(failure.replace(Some(index)).is_none()));
        SPAWNED.with(|spawned| assert!(spawned.replace(Some(Vec::new())).is_none()));
        Self
    }
    pub(super) fn finish(self) -> Vec<std::os::windows::io::OwnedHandle> {
        SPAWNED.with(|spawned| spawned.take().unwrap())
    }
}
impl Drop for SpawnFailureProbe {
    fn drop(&mut self) {
        FAIL_SPAWN.with(|failure| failure.set(None));
        SPAWNED.with(|spawned| {
            spawned.take();
        });
    }
}

pub(super) struct Timing {
    pub(super) sequence: u64,
    base: Instant,
    enqueue_before: AtomicU64,
    enqueue_after: AtomicU64,
    start: AtomicU64,
    end: AtomicU64,
    ready_before: AtomicU64,
    ready_after: AtomicU64,
    polled: AtomicU64,
    cpu_start: AtomicU64,
    cpu_end: AtomicU64,
    worker: AtomicU64,
    birth: AtomicU64,
    native: AtomicUsize,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct Snapshot {
    pub(super) sequence: u64,
    pub(super) enqueue_before_ns: u64,
    pub(super) enqueue_after_ns: u64,
    pub(super) start_ns: u64,
    pub(super) end_ns: u64,
    pub(super) ready_before_ns: u64,
    pub(super) ready_after_ns: u64,
    pub(super) polled_ns: u64,
    pub(super) cpu_ns: u64,
    pub(super) cpu_start_ns: u64,
    pub(super) cpu_end_ns: u64,
    pub(super) worker_id: u64,
    pub(super) worker_birth_100ns: u64,
    pub(super) native: usize,
}

impl Timing {
    fn new(sequence: u64, base: Instant) -> Self {
        Self {
            sequence,
            base,
            enqueue_before: AtomicU64::new(0),
            enqueue_after: AtomicU64::new(0),
            start: AtomicU64::new(0),
            end: AtomicU64::new(0),
            ready_before: AtomicU64::new(0),
            ready_after: AtomicU64::new(0),
            polled: AtomicU64::new(0),
            cpu_start: AtomicU64::new(0),
            cpu_end: AtomicU64::new(0),
            worker: AtomicU64::new(0),
            birth: AtomicU64::new(0),
            native: AtomicUsize::new(0),
        }
    }

    fn stamp(&self, target: &AtomicU64) {
        target.store(
            u64::try_from(self.base.elapsed().as_nanos()).unwrap() + 1,
            Ordering::Release,
        );
    }
    pub(super) fn enqueue_before(&self) {
        self.stamp(&self.enqueue_before);
    }
    pub(super) fn enqueue_after(&self) {
        self.stamp(&self.enqueue_after);
    }
    pub(super) fn ready_before(&self) {
        self.stamp(&self.ready_before);
    }
    pub(super) fn ready_after(&self) {
        self.stamp(&self.ready_after);
    }
    pub(super) fn polled(&self) {
        self.stamp(&self.polled);
    }

    pub(super) fn start(&self) {
        self.stamp(&self.start);
        let worker = current_worker();
        self.worker.store(u64::from(worker.id), Ordering::Relaxed);
        self.birth.store(worker.birth_100ns, Ordering::Relaxed);
        self.cpu_start.store(worker.cpu_ns, Ordering::Relaxed);
    }

    pub(super) fn end(&self, native: usize) {
        self.cpu_end
            .store(current_worker().cpu_ns, Ordering::Relaxed);
        self.native.store(native, Ordering::Relaxed);
        self.stamp(&self.end);
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        let load = |v: &AtomicU64| v.load(Ordering::Acquire);
        Snapshot {
            sequence: self.sequence,
            enqueue_before_ns: load(&self.enqueue_before),
            enqueue_after_ns: load(&self.enqueue_after),
            start_ns: load(&self.start),
            end_ns: load(&self.end),
            ready_before_ns: load(&self.ready_before),
            ready_after_ns: load(&self.ready_after),
            polled_ns: load(&self.polled),
            cpu_ns: load(&self.cpu_end).saturating_sub(load(&self.cpu_start)),
            cpu_start_ns: load(&self.cpu_start),
            cpu_end_ns: load(&self.cpu_end),
            worker_id: load(&self.worker),
            worker_birth_100ns: load(&self.birth),
            native: self.native.load(Ordering::Acquire),
        }
    }
}

struct Collection {
    base: Instant,
    timings: Vec<Arc<Timing>>,
}
thread_local! { static ACTIVE: RefCell<Option<Collection>> = const { RefCell::new(None) }; }

pub(super) struct CollectionScope;
impl CollectionScope {
    pub(super) fn begin() -> Self {
        ACTIVE.with(|active| {
            assert!(
                active
                    .replace(Some(Collection {
                        base: Instant::now(),
                        timings: Vec::new()
                    }))
                    .is_none()
            )
        });
        Self
    }
    pub(super) fn finish(self) -> Vec<Arc<Timing>> {
        ACTIVE.with(|active| active.take().unwrap().timings)
    }
}
impl Drop for CollectionScope {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.take();
        });
    }
}

/// Called only by ordinary Work's existing control-only constructor.
pub(super) fn capture(sequence: u64) -> Option<Arc<Timing>> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let active = active.as_mut()?;
        let timing = Arc::new(Timing::new(sequence, active.base));
        active.timings.push(timing.clone());
        Some(timing)
    })
}
