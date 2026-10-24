//! Observation-only callback births and condition tokens. Registry access and
//! ownership replacement belong to control; a callback writes only its own slot.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[cfg(test)]
thread_local! {
    static BEFORE_CONDITION_CAS: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetryToken {
    pub(crate) birth: u64,
    pub(crate) condition: u64,
    pub(crate) revision: u64,
    pub(crate) timing_revision: u64,
}

pub(crate) struct RetryObservationSlot {
    birth: u64,
    revision: AtomicU64,
    timing_revision: AtomicU64,
    condition: AtomicU64,
}

impl RetryObservationSlot {
    pub(crate) fn invalidate(&self) {
        self.condition.store(0, Ordering::Release);
    }

    pub(crate) fn publish(
        &self,
        revision: u64,
        timing_revision: u64,
        condition: u64,
    ) -> RetryToken {
        self.invalidate();
        self.revision.store(revision, Ordering::Relaxed);
        self.timing_revision
            .store(timing_revision, Ordering::Relaxed);
        self.condition.store(condition, Ordering::Release);
        RetryToken {
            birth: self.birth,
            condition,
            revision,
            timing_revision,
        }
    }

    fn read(&self) -> Option<RetryToken> {
        let condition = self.condition.load(Ordering::Acquire);
        if condition == 0 {
            return None;
        }
        let revision = self.revision.load(Ordering::Acquire);
        let timing_revision = self.timing_revision.load(Ordering::Acquire);
        (self.condition.load(Ordering::Acquire) == condition).then_some(RetryToken {
            birth: self.birth,
            condition,
            revision,
            timing_revision,
        })
    }

    pub(crate) fn matches(&self, token: RetryToken) -> bool {
        self.read() == Some(token)
    }
}

struct RegistryState {
    next_birth: u64,
    current: Option<Arc<RetryObservationSlot>>,
}

pub(crate) struct RuntimePolicyRetryRegistry {
    state: Mutex<RegistryState>,
    // Shared across every callback birth. Zero is sticky exhaustion.
    condition: AtomicU64,
}

impl Default for RuntimePolicyRetryRegistry {
    fn default() -> Self {
        Self {
            state: Mutex::new(RegistryState {
                next_birth: 1,
                current: None,
            }),
            condition: AtomicU64::new(1),
        }
    }
}

impl RuntimePolicyRetryRegistry {
    pub(crate) fn available(&self) -> bool {
        self.condition.load(Ordering::Acquire) != 0
    }
    /// Control only, after the complete mixer construction/rebind has succeeded.
    /// Poisoning or exhaustion disables observation, never audio construction.
    pub(crate) fn register(&self) -> Option<Arc<RetryObservationSlot>> {
        let mut state = self.state.lock().ok()?;
        let birth = state.next_birth;
        if birth == 0 {
            state.current = None;
            return None;
        }
        state.next_birth = birth.checked_add(1).unwrap_or(0);
        let slot = Arc::new(RetryObservationSlot {
            birth,
            revision: AtomicU64::new(0),
            timing_revision: AtomicU64::new(0),
            condition: AtomicU64::new(0),
        });
        state.current = Some(slot.clone());
        Some(slot)
    }

    /// Control only. Old callbacks never access or replace this current owner.
    pub(crate) fn current(&self) -> Option<RetryToken> {
        if self.condition.load(Ordering::Acquire) == 0 {
            return None;
        }
        let state = self.state.lock().ok()?;
        state.current.as_ref()?.read()
    }

    /// One bounded CAS, not a retry loop. A collision leaves this observation
    /// unqualified; its callback keeps the shape dirty until a later checkpoint.
    pub(crate) fn next_condition(&self) -> Option<u64> {
        let previous = self.condition.load(Ordering::Acquire);
        if previous == 0 {
            return None;
        }
        let next = previous.checked_add(1).unwrap_or(0);
        #[cfg(test)]
        if let Some(hook) = BEFORE_CONDITION_CAS.with(|hook| hook.borrow_mut().take()) {
            hook();
        }
        self.condition
            .compare_exchange(previous, next, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .filter(|_| next != 0)
            .map(|_| next)
    }

    #[cfg(test)]
    pub(crate) fn exhaust_for_test(&self) {
        self.condition.store(0, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn invalidate_current_for_test(&self) {
        if let Some(slot) = self.state.lock().unwrap().current.as_ref() {
            slot.invalidate();
        }
    }

    #[cfg(test)]
    pub(crate) fn current_birth_for_test(&self) -> Option<u64> {
        self.state
            .lock()
            .unwrap()
            .current
            .as_ref()
            .map(|slot| slot.birth)
    }

    #[cfg(test)]
    pub(crate) fn with_before_condition_cas_for_test<T>(
        hook: Box<dyn FnOnce()>,
        action: impl FnOnce() -> T,
    ) -> T {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                BEFORE_CONDITION_CAS.with(|hook| *hook.borrow_mut() = None);
            }
        }
        BEFORE_CONDITION_CAS.with(|slot| *slot.borrow_mut() = Some(hook));
        let _reset = Reset;
        action()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superseded_callback_cannot_publish_or_invalidate_the_current_slot() {
        let registry = RuntimePolicyRetryRegistry::default();
        let a = registry.register().unwrap();
        a.publish(9, 3, registry.next_condition().unwrap());
        let b = registry.register().unwrap();
        assert!(registry.current().is_none());
        let actual = b.publish(9, 3, registry.next_condition().unwrap());
        a.publish(9, 3, registry.next_condition().unwrap());
        a.invalidate();
        assert_eq!(registry.current(), Some(actual));
        assert_ne!(a.birth, b.birth);
    }

    #[test]
    fn exhausted_birth_and_condition_disable_only_observation_without_recycling() {
        let registry = RuntimePolicyRetryRegistry::default();
        let a = registry.register().unwrap();
        a.publish(7, 3, registry.next_condition().unwrap());
        registry.state.lock().unwrap().next_birth = u64::MAX;
        let last = registry.register().unwrap();
        assert_eq!(last.birth, u64::MAX);
        assert!(registry.register().is_none());
        assert!(registry.register().is_none());
        assert!(registry.current().is_none());
        registry.condition.store(u64::MAX, Ordering::Release);
        assert!(registry.next_condition().is_none());
        assert_eq!(registry.condition.load(Ordering::Acquire), 0);
        assert!(registry.next_condition().is_none());
    }
}
