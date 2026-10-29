//! One latest explicit playable choice, written only at a successful callback effect.
use super::content_placement::{ContentCell, ContentKey, PlacementObservation};
use super::input_runtime_binding::InputRuntimeOwnership;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExplicitTargetSnapshot {
    pub(crate) key: ContentKey,
    pub(crate) source: u64,
    pub(crate) frame: u64,
    pub(crate) order: u64,
}

/// A semantic mismatch is invalid history; an unchanged identity without readiness
/// is a pending publication. Identity observations never grant source permission.
pub(crate) fn classify_source_epochs(
    expected: u64,
    identity: Option<u64>,
    ready: Option<u64>,
) -> Result<bool, &'static str> {
    match identity {
        Some(epoch) if epoch != expected => Ok(false),
        Some(epoch) if ready == Some(epoch) => Ok(true),
        _ => Err("explicit playback source publication pending"),
    }
}

#[derive(Default)]
pub(crate) struct ExplicitTargetSlot {
    sequence: AtomicU64,
    cell: AtomicU64,
    lifetime: AtomicU64,
    source: AtomicU64,
    frame: AtomicU64,
    order: AtomicU64,
}

#[cfg(test)]
thread_local! { static AFTER_SOURCE_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) }; }

#[cfg(test)]
pub(crate) fn with_after_source_read_for_test<T>(
    hook: impl FnOnce() + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            AFTER_SOURCE_READ.with(|slot| *slot.borrow_mut() = None);
        }
    }
    AFTER_SOURCE_READ.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    let _reset = Reset;
    action()
}

impl ExplicitTargetSlot {
    pub(crate) fn read(
        &self,
        owner: &InputRuntimeOwnership,
    ) -> Result<Option<ExplicitTargetSnapshot>, &'static str> {
        let sequence = self.sequence.load(Ordering::SeqCst);
        if sequence == 0 {
            return Ok(None);
        }
        if sequence == u64::MAX || sequence & 1 != 0 {
            return Err("explicit playback target unavailable");
        }
        let lifetime = self.lifetime.load(Ordering::SeqCst);
        if lifetime == 0 {
            return if self.sequence.load(Ordering::SeqCst) == sequence {
                Ok(None)
            } else {
                Err("explicit playback target pending")
            };
        }
        let cell = ContentCell::from_index(self.cell.load(Ordering::SeqCst) as usize)
            .ok_or("explicit playback target unavailable")?;
        let key = ContentKey::new(owner.owner_identity(), cell, lifetime)
            .ok_or("explicit playback target unavailable")?;
        let value = ExplicitTargetSnapshot {
            key,
            source: self.source.load(Ordering::SeqCst),
            frame: self.frame.load(Ordering::SeqCst),
            order: self.order.load(Ordering::SeqCst),
        };
        if self.sequence.load(Ordering::SeqCst) != sequence {
            return Err("explicit playback target pending");
        }
        Ok(Some(value))
    }

    fn available(&self, owner: &InputRuntimeOwnership, value: ExplicitTargetSnapshot) -> bool {
        match self.read(owner) {
            Ok(Some(previous))
                if (previous.frame, previous.order) >= (value.frame, value.order) =>
            {
                true
            }
            Ok(_) => self
                .sequence
                .load(Ordering::SeqCst)
                .checked_add(2)
                .is_some(),
            Err(_) => false,
        }
    }

    /// Off callback, only after verified old-stream quiescence and source revocation.
    /// Emptying this record does not rewind its observation stamp or revive old receipts.
    fn retire_after_quiescence(&self) -> Result<(), &'static str> {
        let previous = self.sequence.load(Ordering::SeqCst);
        let next = previous
            .checked_add(2)
            .filter(|_| previous & 1 == 0)
            .ok_or("explicit playback target publication exhausted")?;
        self.sequence.store(previous + 1, Ordering::SeqCst);
        self.lifetime.store(0, Ordering::SeqCst);
        self.source.store(0, Ordering::SeqCst);
        self.frame.store(0, Ordering::SeqCst);
        self.order.store(0, Ordering::SeqCst);
        self.sequence.store(next, Ordering::SeqCst);
        Ok(())
    }

    fn publish(&self, owner: &InputRuntimeOwnership, value: ExplicitTargetSnapshot) -> bool {
        // Compare even an invalidated last source. Never resurrect an older effective choice.
        match self.read(owner) {
            Ok(Some(previous))
                if (previous.frame, previous.order) >= (value.frame, value.order) =>
            {
                return true;
            }
            Err(_) => return false,
            _ => {}
        }
        let previous = self.sequence.load(Ordering::SeqCst);
        let Some(next) = previous.checked_add(2).filter(|_| previous & 1 == 0) else {
            return false;
        };
        self.sequence.store(previous + 1, Ordering::SeqCst);
        self.cell
            .store(value.key.cell().index() as u64, Ordering::SeqCst);
        self.lifetime.store(value.key.lifetime(), Ordering::SeqCst);
        self.source.store(value.source, Ordering::SeqCst);
        self.frame.store(value.frame, Ordering::SeqCst);
        self.order.store(value.order, Ordering::SeqCst);
        self.sequence.store(next, Ordering::SeqCst);
        true
    }
}

impl InputRuntimeOwnership {
    pub(super) fn retire_explicit_target_after_quiescence(&self) -> Result<(), &'static str> {
        self.explicit_target.retire_after_quiescence()
    }
    pub(crate) fn explicit_playback_available(
        &self,
        key: ContentKey,
        frame: u64,
        order: u64,
    ) -> bool {
        order != 0
            && self.content_key_current(key)
            && self
                .ordinary_source_epoch(key.cell())
                .is_some_and(|source| {
                    self.explicit_target.available(
                        self,
                        ExplicitTargetSnapshot {
                            key,
                            source,
                            frame,
                            order,
                        },
                    )
                })
    }

    #[cfg(test)]
    pub(crate) fn exhaust_explicit_target_for_test(&self) {
        self.explicit_target
            .sequence
            .store(u64::MAX - 1, Ordering::SeqCst);
    }

    pub(crate) fn record_explicit_playback(&self, key: ContentKey, frame: u64, order: u64) -> bool {
        if order == 0 || !self.content_key_current(key) {
            return false;
        }
        if let Some(source) = self.ordinary_source_epoch(key.cell()) {
            return self.explicit_target.publish(
                self,
                ExplicitTargetSnapshot {
                    key,
                    source,
                    frame,
                    order,
                },
            );
        }
        false
    }

    pub(crate) fn explicit_target_snapshot(
        &self,
    ) -> Result<Option<ExplicitTargetSnapshot>, &'static str> {
        self.explicit_target.read(self)
    }

    pub(crate) fn explicit_source_current(&self, value: ExplicitTargetSnapshot) -> bool {
        self.content_key_current(value.key)
            && self.ordinary_source_epoch(value.key.cell()) == Some(value.source)
    }

    pub(crate) fn explicit_target_current(&self, value: Option<ExplicitTargetSnapshot>) -> bool {
        self.explicit_target_snapshot()
            .is_ok_and(|current| current == value)
    }
}

/// Source-only historical identity. A fresh launch still needs ordinary source/timing admission.
#[pyclass(frozen, module = "flitzis_looper_audio")]
#[derive(Clone)]
pub struct ExplicitPlaybackTarget {
    pub(crate) ownership: Arc<InputRuntimeOwnership>,
    pub(crate) value: ExplicitTargetSnapshot,
}

#[pymethods]
impl ExplicitPlaybackTarget {
    pub fn current_control_slot(&self) -> PyResult<Option<usize>> {
        let token = self
            .ownership
            .placement_read_token()
            .ok_or_else(|| PyRuntimeError::new_err("explicit playback placement pending"))?;
        let result = self.observe_slot();
        if self.ownership.placement_read_token() != Some(token) {
            return Err(PyRuntimeError::new_err(
                "explicit playback source publication crossed observation",
            ));
        }
        result
    }
    pub fn content_lifetime(&self) -> u64 {
        self.value.key.lifetime()
    }
    pub fn effective_frame(&self) -> u64 {
        self.value.frame
    }
    pub fn original_sequence(&self) -> u64 {
        self.value.order
    }
}

impl ExplicitPlaybackTarget {
    fn observe_slot(&self) -> PyResult<Option<usize>> {
        if !self
            .ownership
            .ordinary_source_available(self.value.key.cell())
        {
            return Err(PyRuntimeError::new_err(
                "explicit playback source observation unavailable",
            ));
        }
        let slot = match self.ownership.observe_control_key(self.value.key) {
            PlacementObservation::Current(placement) => placement.slot(),
            PlacementObservation::Pending => {
                return Err(PyRuntimeError::new_err(
                    "explicit playback placement pending",
                ));
            }
            _ => return Ok(None),
        };
        let identity_epoch = self
            .ownership
            .ordinary_source_identity_epoch(self.value.key.cell());
        let ready_epoch = self.ownership.ordinary_source_epoch(self.value.key.cell());
        #[cfg(test)]
        if let Some(hook) = AFTER_SOURCE_READ.with(|slot| slot.borrow_mut().take()) {
            hook();
        }
        match self.ownership.observe_control_key(self.value.key) {
            PlacementObservation::Current(current) if current.slot() == slot => {}
            PlacementObservation::Pending => {
                return Err(PyRuntimeError::new_err(
                    "explicit playback placement pending",
                ));
            }
            _ => return Ok(None),
        }
        classify_source_epochs(self.value.source, identity_epoch, ready_epoch)
            .map(|ready| ready.then_some(slot))
            .map_err(PyRuntimeError::new_err)
    }
}

pub(super) fn current(
    owner: &Arc<InputRuntimeOwnership>,
) -> PyResult<Option<ExplicitPlaybackTarget>> {
    let Some(value) = owner
        .explicit_target_snapshot()
        .map_err(PyRuntimeError::new_err)?
    else {
        return Ok(None);
    };
    let target = ExplicitPlaybackTarget {
        ownership: owner.clone(),
        value,
    };
    let slot = target.current_control_slot()?;
    if !owner.explicit_target_current(Some(value)) {
        return Err(PyRuntimeError::new_err(
            "explicit playback target changed during observation",
        ));
    }
    Ok(slot.map(|_| target))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner() -> Arc<InputRuntimeOwnership> {
        let owner = Arc::new(InputRuntimeOwnership::default());
        for id in 0..3 {
            owner.publish_content_lifetime(id, super::super::content_lifetime::reserve().unwrap());
        }
        owner
    }
    #[test]
    fn target_uses_one_total_effect_order_and_never_revives_invalid_older_history() {
        let owner = owner();
        let a = owner.slot_snapshot(0).unwrap().content_key().unwrap();
        let b = owner.slot_snapshot(1).unwrap().content_key().unwrap();
        owner.record_explicit_playback(a, 100, 7);
        owner.record_explicit_playback(b, 99, 900);
        assert_eq!(
            current(&owner)
                .unwrap()
                .unwrap()
                .current_control_slot()
                .unwrap(),
            Some(0)
        );
        owner.record_explicit_playback(b, 100, 8);
        owner.record_explicit_playback(a, 100, 7);
        let target = current(&owner).unwrap().unwrap();
        assert_eq!(target.current_control_slot().unwrap(), Some(1));
        owner.publish_ordinary_lifecycle(b, super::super::ordinary_transport::STOPPED, false);
        let revision = owner
            .prepare_runtime_revision_write(owner.slot_snapshot(1).unwrap(), true)
            .unwrap();
        owner.apply_runtime_revision_write(revision);
        assert_eq!(current(&owner).unwrap().unwrap().original_sequence(), 8);
        owner.revoke_source(1);
        assert!(
            current(&owner).unwrap().is_none(),
            "semantic source revocation alone invalidates the latest target"
        );
        owner.publish_content_lifetime(1, super::super::content_lifetime::reserve().unwrap());
        assert!(current(&owner).unwrap().is_none());
        owner.record_explicit_playback(a, 100, 7);
        assert!(current(&owner).unwrap().is_none());
    }
    #[test]
    fn target_follows_content_placement_and_pending_is_not_absent_history() {
        let owner = owner();
        let a = owner.slot_snapshot(0).unwrap();
        let b = owner.slot_snapshot(1).unwrap();
        owner.record_explicit_playback(a.content_key().unwrap(), 12, 4);
        let target = current(&owner).unwrap().unwrap();
        let pair = owner.prepare_placement_pair(a, b).unwrap();
        let commit = owner
            .commit_placement_pair(&pair, true, || true, || {})
            .unwrap();
        assert!(
            target.current_control_slot().is_err(),
            "Native/control placement gap is a refusal, never selection fallback"
        );
        owner.publish_pair_control_slots(&[commit.source_after, commit.target_after]);
        assert_eq!(target.current_control_slot().unwrap(), Some(1));
        assert_eq!(
            current(&owner).unwrap().unwrap().content_lifetime(),
            a.lifetime()
        );
        let moved = owner.slot_snapshot(1).unwrap();
        let empty = owner.slot_snapshot(3).unwrap();
        let pair = owner.prepare_placement_pair(moved, empty).unwrap();
        let commit = owner
            .commit_placement_pair(&pair, true, || true, || {})
            .unwrap();
        owner.publish_pair_control_slots(&[commit.source_after, commit.target_after]);
        assert_eq!(target.current_control_slot().unwrap(), Some(3));
    }

    #[test]
    fn verified_new_stream_retires_old_frame_order_without_rewinding_publication_stamp() {
        let owner = owner();
        let key = owner.slot_snapshot(0).unwrap().content_key().unwrap();
        assert!(owner.record_explicit_playback(key, 9_000_000, 91));
        let old = current(&owner).unwrap().unwrap();
        let stamp = owner.explicit_target.sequence.load(Ordering::SeqCst);
        // The production caller reaches this point only after verified callback close.
        owner.revoke_all_sources();
        owner.retire_explicit_target_after_quiescence().unwrap();
        assert!(owner.explicit_target.sequence.load(Ordering::SeqCst) > stamp);
        assert!(current(&owner).unwrap().is_none());
        assert!(owner.record_explicit_playback(key, 12, 92));
        let fresh = current(&owner).unwrap().unwrap();
        assert_eq!(fresh.effective_frame(), 12);
        assert_eq!(old.current_control_slot().unwrap(), None);
        assert!(!owner.explicit_target_current(Some(old.value)));
        owner.exhaust_explicit_target_for_test();
        assert!(owner.retire_explicit_target_after_quiescence().is_err());
        assert_eq!(
            current(&owner).unwrap().unwrap().original_sequence(),
            fresh.original_sequence()
        );
    }
}
