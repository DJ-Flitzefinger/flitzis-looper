//! Runtime-only content lifetimes, scoped by the receiving Native ownership instance.
//!
//! Reserve outside the audio callback. Failed preparation may consume a number;
//! neither rollback nor reopening is permitted to recycle it into another content.

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CONTENT_LIFETIME: AtomicU64 = AtomicU64::new(1);

fn reserve_from(counter: &AtomicU64) -> Result<u64, &'static str> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_add(1)
        })
        .map_err(|_| "content lifetime exhausted")
}

pub(super) fn reserve() -> Result<u64, &'static str> {
    reserve_from(&NEXT_CONTENT_LIFETIME)
}

pub(super) fn for_assignment(current: u64, replacement: bool) -> Result<u64, &'static str> {
    if replacement || current == 0 {
        reserve()
    } else {
        Ok(current)
    }
}

#[cfg(test)]
#[path = "content_lifetime_tests.rs"]
mod tests;
