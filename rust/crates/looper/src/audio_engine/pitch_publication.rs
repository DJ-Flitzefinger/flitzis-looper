//! Single-consumer transfer of control-allocated, non-cloneable native work.

use super::pitch_actions::PitchActionWork;
use std::cell::UnsafeCell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// Engine-local storage bound, independent of callback admission and receipt lifetime.
#[derive(Default)]
pub(crate) struct PitchWorkCapacity(Arc<PitchWorkUsage>);

#[derive(Default)]
struct PitchWorkUsage {
    used: AtomicUsize,
    #[cfg(test)]
    allocated: AtomicUsize,
    #[cfg(test)]
    native_created: AtomicUsize,
}

impl PitchWorkCapacity {
    /// Control-only reservation, before allocating the original work or history storage.
    pub(crate) fn claim(&self) -> Option<PitchWorkClaim> {
        self.0
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                (used < super::pitch_runtime::MAX_PITCH_ACTIONS).then_some(used + 1)
            })
            .ok()
            .map(|_| PitchWorkClaim(self.0.clone()))
    }

    #[cfg(test)]
    pub(crate) fn used(&self) -> usize {
        self.0.used.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn allocations(&self) -> (usize, usize) {
        (
            self.0.allocated.load(Ordering::Acquire),
            self.0.native_created.load(Ordering::Acquire),
        )
    }
}

/// Never cloned; the final work field releases this only after all owned storage.
pub(crate) struct PitchWorkClaim(Arc<PitchWorkUsage>);

#[cfg(test)]
impl PitchWorkClaim {
    pub(crate) fn record_work_allocation(&self) {
        self.0.allocated.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn record_native_creation(&self) {
        self.0.native_created.fetch_add(1, Ordering::AcqRel);
    }
}

impl Drop for PitchWorkClaim {
    fn drop(&mut self) {
        let previous = self.0.used.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous != 0);
    }
}

pub(crate) struct PitchActionPublication {
    claimed: AtomicBool,
    work: UnsafeCell<Option<Box<PitchActionWork>>>,
}

// Control initializes the value before queue publication. Exactly one consumer wins
// the atomic claim. Losing clones never access the cell; teardown follows queue ownership.
unsafe impl Send for PitchActionPublication {}
unsafe impl Sync for PitchActionPublication {}

impl PitchActionPublication {
    pub(crate) fn new(work: Box<PitchActionWork>) -> Self {
        Self {
            claimed: AtomicBool::new(false),
            work: UnsafeCell::new(Some(work)),
        }
    }

    pub(crate) fn take(&self) -> Option<Box<PitchActionWork>> {
        if self
            .claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        // The successful claim exclusively owns this cell for the rest of its lifetime.
        unsafe { (&mut *self.work.get()).take() }
    }
}

impl std::fmt::Debug for PitchActionPublication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PitchActionPublication")
            .field("claimed", &self.claimed.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}
