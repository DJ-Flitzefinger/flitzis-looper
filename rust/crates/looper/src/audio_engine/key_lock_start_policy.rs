//! One synchronous stopped-only global choice; no parameter queue or live transition.

use super::content_placement::PlacementError;
use super::input_runtime_binding::InputRuntimeOwnership;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum KeyLockStartBehavior {
    #[default]
    FirstSoundAtGrid = 0,
    AttackAtGrid = 1,
}

impl KeyLockStartBehavior {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "first_sound_at_grid" => Some(Self::FirstSoundAtGrid),
            "attack_at_grid" => Some(Self::AttackAtGrid),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::FirstSoundAtGrid => "first_sound_at_grid",
            Self::AttackAtGrid => "attack_at_grid",
        }
    }

    fn from_stored(value: u8) -> Self {
        match value {
            1 => Self::AttackAtGrid,
            _ => Self::FirstSoundAtGrid,
        }
    }
}

#[derive(Debug)]
pub(crate) struct KeyLockStartPolicy {
    behavior: AtomicU8,
    admission_revision: AtomicU64,
    callback_idle: AtomicBool,
    pending_owners: AtomicU64,
}

impl Default for KeyLockStartPolicy {
    fn default() -> Self {
        Self {
            behavior: AtomicU8::new(KeyLockStartBehavior::FirstSoundAtGrid as u8),
            admission_revision: AtomicU64::new(1),
            // A newly constructed engine has neither callback voices nor accepted starts.
            callback_idle: AtomicBool::new(true),
            pending_owners: AtomicU64::new(0),
        }
    }
}

impl KeyLockStartPolicy {
    pub(crate) fn behavior(&self) -> KeyLockStartBehavior {
        KeyLockStartBehavior::from_stored(self.behavior.load(Ordering::Acquire))
    }

    /// Launch/transport mutation already owns the common publication marker. Saturation
    /// permanently closes admission rather than recycling a callback-drain observation.
    pub(crate) fn invalidate_admission(&self) {
        self.callback_idle.store(false, Ordering::Release);
        let previous = self.admission_revision.load(Ordering::Acquire);
        self.admission_revision
            .store(previous.saturating_add(1), Ordering::Release);
    }

    pub(crate) fn begin_callback_drain(&self) -> Option<u64> {
        let revision = self.admission_revision.load(Ordering::Acquire);
        (revision != u64::MAX).then_some(revision)
    }

    fn stopped(&self) -> bool {
        self.callback_idle.load(Ordering::Acquire)
            && self.pending_owners.load(Ordering::Acquire) == 0
            && self.admission_revision.load(Ordering::Acquire) != u64::MAX
    }
}

#[derive(Debug)]
struct KeyLockStartLeaseOwner {
    policy: Arc<KeyLockStartPolicy>,
    behavior: AtomicU8,
    active: AtomicBool,
}

impl KeyLockStartLeaseOwner {
    fn finish(&self) {
        if self.active.swap(false, Ordering::AcqRel) {
            let previous = self.policy.pending_owners.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous > 0);
        }
    }
}

impl Drop for KeyLockStartLeaseOwner {
    fn drop(&mut self) {
        self.finish();
    }
}

/// One genuine accepted capture, shared through its own Native owner, worker and queue.
/// Registration allocates off RT. Terminal completion is fixed scalar work and idempotent.
#[derive(Clone, Debug)]
pub(crate) struct KeyLockStartLease {
    owner: Arc<KeyLockStartLeaseOwner>,
}

impl KeyLockStartLease {
    pub(crate) fn behavior(&self) -> KeyLockStartBehavior {
        KeyLockStartBehavior::from_stored(self.owner.behavior.load(Ordering::Acquire))
    }

    /// Apply callers must first publish actual busy playback inside their existing marker.
    /// Reject/cancel callers first make the original start permanently unable to execute.
    pub(crate) fn finish(&self) {
        self.owner.finish();
    }
}

impl InputRuntimeOwnership {
    pub(crate) fn key_lock_start_behavior(&self) -> KeyLockStartBehavior {
        self.key_lock_start_policy.behavior()
    }

    pub(crate) fn can_change_key_lock_start_behavior(&self) -> bool {
        let Some(token) = self.placement_read_token() else {
            return false;
        };
        self.key_lock_start_policy.stopped() && self.placement_read_token() == Some(token)
    }

    pub(crate) fn set_key_lock_start_behavior(&self, behavior: KeyLockStartBehavior) -> bool {
        let Some(anchor) = self.slot_snapshot(0) else {
            return false;
        };
        let Ok(write) = self.prepare_slot_write_fixed(anchor, anchor.lifetime()) else {
            return false;
        };
        self.commit_slot_write_fixed(
            &write,
            || self.key_lock_start_policy.stopped(),
            |_| {
                self.key_lock_start_policy
                    .behavior
                    .store(behavior as u8, Ordering::Release)
            },
        )
        .is_ok()
    }

    /// Capture owners call this before any residency wait or queue publication. The same
    /// marker atomically freezes accepted policy and prevents a concurrent settings edit.
    pub(crate) fn register_key_lock_start_owner(
        &self,
    ) -> Result<KeyLockStartLease, PlacementError> {
        let lease = KeyLockStartLease {
            owner: Arc::new(KeyLockStartLeaseOwner {
                policy: self.key_lock_start_policy.clone(),
                behavior: AtomicU8::new(KeyLockStartBehavior::FirstSoundAtGrid as u8),
                active: AtomicBool::new(false),
            }),
        };
        let anchor = self.slot_snapshot(0).ok_or(PlacementError::Pending)?;
        let write = self.prepare_slot_write_fixed(anchor, anchor.lifetime())?;
        self.commit_slot_write_fixed(
            &write,
            || {
                self.key_lock_start_policy
                    .pending_owners
                    .load(Ordering::Acquire)
                    != u64::MAX
            },
            |_| {
                self.key_lock_start_policy
                    .pending_owners
                    .fetch_add(1, Ordering::AcqRel);
                lease
                    .owner
                    .behavior
                    .store(self.key_lock_start_behavior() as u8, Ordering::Release);
                lease.owner.active.store(true, Ordering::Release);
            },
        )?;
        Ok(lease)
    }

    /// Only the real callback supplies drained-queue, scheduler, voice and Hold truth.
    /// A producer that won after the first queue read invalidates this exact revision.
    pub(crate) fn finish_key_lock_start_callback(&self, revision: Option<u64>, idle: bool) {
        if !idle {
            self.key_lock_start_policy
                .callback_idle
                .store(false, Ordering::Release);
            return;
        }
        let Some(revision) = revision else {
            return;
        };
        // An unchanged true proof needs no publication. A later admission only closes it;
        // the false -> true transition below still requires the original common marker.
        if self
            .key_lock_start_policy
            .callback_idle
            .load(Ordering::Acquire)
            && self
                .key_lock_start_policy
                .admission_revision
                .load(Ordering::Acquire)
                == revision
        {
            return;
        }
        let Some(anchor) = self.slot_snapshot(0) else {
            return;
        };
        let Ok(write) = self.prepare_slot_write_fixed(anchor, anchor.lifetime()) else {
            return;
        };
        let _ = self.commit_slot_write_fixed(
            &write,
            || {
                self.key_lock_start_policy
                    .admission_revision
                    .load(Ordering::Acquire)
                    == revision
            },
            |_| {
                self.key_lock_start_policy
                    .callback_idle
                    .store(true, Ordering::Release)
            },
        );
    }
}
