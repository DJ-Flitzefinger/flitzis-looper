//! Test executable allocator observations, distinct from process RSS/Commit.
//! Always counts System allocations; isolated long-source children reset peaks
//! at phase boundaries. Production extensions do not include this allocator.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

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
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            added(layout.size());
        }
        result
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
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
