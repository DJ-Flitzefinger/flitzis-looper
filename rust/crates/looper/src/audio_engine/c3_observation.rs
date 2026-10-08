//! Test-only off-thread observations. These count actual successful native owners
//! and per-operation owned PCM at declared points, not simultaneous process totals.
use std::sync::atomic::{AtomicUsize, Ordering};

static CREATED: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK_LIVE: AtomicUsize = AtomicUsize::new(0);
static OBSERVED_PCM: AtomicUsize = AtomicUsize::new(0);

pub(super) fn native_created() {
    CREATED.fetch_add(1, Ordering::Relaxed);
    let live = LIVE.fetch_add(1, Ordering::Relaxed) + 1;
    PEAK_LIVE.fetch_max(live, Ordering::Relaxed);
}

pub(super) fn native_deleted() {
    LIVE.fetch_sub(1, Ordering::Relaxed);
}

pub(super) fn owned_pcm(bytes: usize) {
    OBSERVED_PCM.fetch_max(bytes, Ordering::Relaxed);
}

pub(super) fn snapshot() -> (usize, usize, usize, usize) {
    (
        CREATED.load(Ordering::Relaxed),
        LIVE.load(Ordering::Relaxed),
        PEAK_LIVE.load(Ordering::Relaxed),
        OBSERVED_PCM.load(Ordering::Relaxed),
    )
}
