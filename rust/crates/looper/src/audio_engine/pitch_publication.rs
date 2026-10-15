//! Single-consumer transfer of control-allocated, non-cloneable native work.

use super::pitch_actions::PitchActionWork;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};

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
