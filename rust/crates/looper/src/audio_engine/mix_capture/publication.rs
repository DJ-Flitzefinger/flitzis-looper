//! Single-claim transfer of the original control-prepared capture owner.

use super::CaptureRt;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};

/// Queue clones carry the same session and never duplicate its pool or writer.
/// Every callback-consumed Arc must use the existing off-RT retirement lane,
/// including an already claimed clone. Unconsumed publication teardown is off RT.
pub(crate) struct CapturePublication {
    session_id: u64,
    claimed: AtomicBool,
    work: UnsafeCell<Option<Box<CaptureRt>>>,
}

// Initialization precedes queue publication. Only the successful atomic claim
// accesses the cell; Arc lifetime prevents teardown during that exclusive access.
unsafe impl Send for CapturePublication {}
unsafe impl Sync for CapturePublication {}

impl CapturePublication {
    pub(crate) fn new(work: Box<CaptureRt>) -> Self {
        Self {
            session_id: work.session_id(),
            claimed: AtomicBool::new(false),
            work: UnsafeCell::new(Some(work)),
        }
    }

    pub(crate) fn session_id(&self) -> u64 {
        self.session_id
    }

    pub(crate) fn claim(&self) -> Option<Box<CaptureRt>> {
        if self
            .claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        // Exactly this successful claim owns the cell for the remainder of its lifetime.
        unsafe { (&mut *self.work.get()).take() }
    }
}

impl std::fmt::Debug for CapturePublication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapturePublication")
            .field("session_id", &self.session_id)
            .field("claimed", &self.claimed.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}
