//! Test executable allocator observations, distinct from process RSS/Commit.
//! Always counts System allocations; isolated long-source children reset peaks
//! at phase boundaries. Production extensions do not include this allocator.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "test_memory/thread_tests.rs"]
mod thread_tests;

/// Requested Rust allocator operations on one observed thread, including failed
/// attempts. Reallocation bytes are the full requested new size, not net growth.
/// This observes neither deallocation nor Native/C++ allocator operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ThreadAllocations {
    pub(crate) allocations: usize,
    pub(crate) zeroed_allocations: usize,
    pub(crate) reallocations: usize,
    pub(crate) requested_bytes: usize,
}

thread_local! {
    // Const, Copy-only storage does not allocate or register a TLS destructor.
    static THREAD_ALLOCATIONS: Cell<Option<ThreadAllocations>> = const { Cell::new(None) };
}

enum AllocationKind {
    Allocate,
    Zeroed,
    Reallocate,
}

fn record_thread_allocation(kind: AllocationKind, bytes: usize) {
    // GlobalAlloc must not unwind, including during thread-local teardown.
    let _ = THREAD_ALLOCATIONS.try_with(|counter| {
        if let Some(mut counts) = counter.get() {
            let operations = match kind {
                AllocationKind::Allocate => &mut counts.allocations,
                AllocationKind::Zeroed => &mut counts.zeroed_allocations,
                AllocationKind::Reallocate => &mut counts.reallocations,
            };
            *operations = operations.saturating_add(1);
            counts.requested_bytes = counts.requested_bytes.saturating_add(bytes);
            counter.set(Some(counts));
        }
    });
}

struct ThreadAllocationScope;

impl ThreadAllocationScope {
    fn enter() -> Self {
        THREAD_ALLOCATIONS.with(|counter| {
            assert!(
                counter.get().is_none(),
                "allocation observation cannot nest"
            );
            counter.set(Some(ThreadAllocations::default()));
        });
        Self
    }
}

impl Drop for ThreadAllocationScope {
    fn drop(&mut self) {
        let _ = THREAD_ALLOCATIONS.try_with(|counter| counter.set(None));
    }
}

/// Observe only this thread while `action` runs; the RAII guard also closes on
/// unwind. Nesting rejects before replacing the outer observation. Snapshotting,
/// assertions, worker waits and test-output allocation belong outside the scope.
pub(crate) fn observe_current_thread<R>(action: impl FnOnce() -> R) -> (R, ThreadAllocations) {
    let scope = ThreadAllocationScope::enter();
    let result = action();
    let counts = THREAD_ALLOCATIONS.with(|counter| {
        counter
            .get()
            .expect("current thread allocation observation is active")
    });
    drop(scope);
    (result, counts)
}

struct ObservedSystem;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static LARGEST: AtomicUsize = AtomicUsize::new(0);

fn added(bytes: usize) {
    let held = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(held, Ordering::Relaxed);
    LARGEST.fetch_max(bytes, Ordering::Relaxed);
}

// SAFETY: every operation delegates the original pointer/layout to System;
// counters never access payloads and use no allocation or blocking operation.
unsafe impl GlobalAlloc for ObservedSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_thread_allocation(AllocationKind::Allocate, layout.size());
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            added(layout.size());
        }
        result
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_thread_allocation(AllocationKind::Zeroed, layout.size());
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() {
            added(layout.size());
        }
        result
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_thread_allocation(AllocationKind::Reallocate, size);
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            added(size);
        }
        result
    }
}

#[global_allocator]
static ALLOCATOR: ObservedSystem = ObservedSystem;

pub(crate) fn reset_phase() -> usize {
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    LARGEST.store(0, Ordering::Relaxed);
    baseline
}

pub(crate) fn snapshot(baseline: usize) -> (usize, usize, usize) {
    (
        LIVE.load(Ordering::Relaxed),
        PEAK.load(Ordering::Relaxed).saturating_sub(baseline),
        LARGEST.load(Ordering::Relaxed),
    )
}
