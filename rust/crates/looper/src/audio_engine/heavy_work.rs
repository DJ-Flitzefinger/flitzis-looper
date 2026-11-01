//! One offline whole-job permission above the existing queues and child slots.

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Default)]
struct State {
    queued: VecDeque<Weak<Owner>>,
    active: Weak<Owner>,
}

#[derive(Default)]
pub(super) struct Gate {
    state: Mutex<State>,
}

struct Owner {
    gate: Arc<Gate>,
    active: AtomicBool,
}

/// Clones are explicit borrows of the SAME physical job, not new admissions.
#[derive(Clone)]
pub(super) struct Ticket(Arc<Owner>);

thread_local! {
    static CURRENT: RefCell<Option<Ticket>> = const { RefCell::new(None) };
}

fn global() -> &'static Arc<Gate> {
    static GATE: OnceLock<Arc<Gate>> = OnceLock::new();
    GATE.get_or_init(|| Arc::new(Gate::default()))
}

pub(super) fn ticket() -> Result<Ticket> {
    global().ticket()
}

pub(super) fn current() -> Option<Ticket> {
    CURRENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|ticket| ticket.is_active())
            .cloned()
    })
}

impl Gate {
    pub(super) fn ticket(self: &Arc<Self>) -> Result<Ticket> {
        let mut state = self.state.lock().map_err(|_| "heavy work gate poisoned")?;
        // Finish fallible queue preparation before constructing an Owner whose
        // last Drop needs this mutex. The gate itself holds only weak owners.
        state
            .queued
            .try_reserve(1)
            .map_err(|_| "heavy work queue allocation failed")?;
        let owner = Arc::new(Owner {
            gate: self.clone(),
            active: AtomicBool::new(false),
        });
        state.queued.push_back(Arc::downgrade(&owner));
        Ok(Ticket(owner))
    }
}

impl Ticket {
    pub(super) fn is_active(&self) -> bool {
        self.0.active.load(Ordering::Acquire)
    }

    pub(super) fn try_acquire(&self) -> Result<bool> {
        let mut state = self
            .0
            .gate
            .state
            .lock()
            .map_err(|_| "heavy work gate poisoned")?;
        // Never upgrade a weak owner under this lock: a concurrent close could
        // make that temporary Arc the final owner and recursively lock in Drop.
        state.queued.retain(|owner| owner.strong_count() != 0);
        let mine = Arc::as_ptr(&self.0);
        if state.active.strong_count() != 0 {
            return Ok(state.active.as_ptr() == mine);
        }
        state.active = Weak::new();
        if state
            .queued
            .front()
            .is_none_or(|owner| owner.as_ptr() != mine)
        {
            return Ok(false);
        }
        state.queued.pop_front();
        state.active = Arc::downgrade(&self.0);
        self.0.active.store(true, Ordering::Release);
        Ok(true)
    }

    pub(super) fn with_current<T>(&self, operation: impl FnOnce() -> T) -> Result<T> {
        if !self.is_active() {
            return Err("heavy work ticket is not active");
        }
        let previous = CURRENT.with(|slot| {
            let mut current = slot.borrow_mut();
            if current
                .as_ref()
                .is_some_and(|old| !Arc::ptr_eq(&old.0, &self.0))
            {
                return Err("different heavy work owner already installed");
            }
            Ok(current.replace(self.clone()))
        })?;
        let _restore = Scope {
            previous,
            _same_thread: PhantomData,
        };
        Ok(operation())
    }
}

// Only lexical with_current can create a scope; restoration always runs on the
// originating thread, independently of thread-agnostic borrowed capacity.
struct Scope {
    previous: Option<Ticket>,
    _same_thread: PhantomData<Rc<()>>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        let displaced = CURRENT.with(|slot| slot.replace(self.previous.take()));
        drop(displaced);
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        let mine = self as *const Self;
        // Cleanup remains possible after poison; new admission still fails.
        let mut state = self
            .gate
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.queued.retain(|owner| owner.as_ptr() != mine);
        if state.active.as_ptr() == mine {
            state.active = Weak::new();
        }
    }
}

/// Off-realtime admission/borrowing only. Close never releases another borrower.
#[pyclass(immutable_type, module = "flitzis_looper_audio.flitzis_looper_audio")]
pub struct OfflineWorkTicket {
    ticket: Option<Ticket>,
}

#[pymethods]
impl OfflineWorkTicket {
    #[new]
    fn new() -> PyResult<Self> {
        Ok(Self {
            ticket: Some(match current() {
                Some(active) => active,
                None => ticket().map_err(PyRuntimeError::new_err)?,
            }),
        })
    }

    fn try_acquire(&self, py: Python<'_>) -> PyResult<bool> {
        let ticket = self
            .ticket
            .as_ref()
            .ok_or_else(|| PyValueError::new_err("heavy work ticket is closed"))?;
        py.detach(|| ticket.try_acquire())
            .map_err(PyRuntimeError::new_err)
    }

    #[getter]
    fn active(&self) -> bool {
        self.ticket.as_ref().is_some_and(Ticket::is_active)
    }

    fn borrow(&self) -> PyResult<Self> {
        let ticket = self
            .ticket
            .as_ref()
            .filter(|ticket| ticket.is_active())
            .ok_or_else(|| PyValueError::new_err("heavy work ticket is not active"))?;
        Ok(Self {
            ticket: Some(ticket.clone()),
        })
    }

    /// Install only a valid active capability for this lexical Python call.
    fn run(slf: PyRef<'_, Self>, operation: Py<PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let ticket = slf
            .ticket
            .as_ref()
            .cloned()
            .ok_or_else(|| PyValueError::new_err("heavy work ticket is closed"))?;
        // The callback may close/cancel the original Python handle. Capacity is
        // retained by this separate lexical borrow, without a live PyRef.
        drop(slf);
        ticket
            .with_current(|| operation.call0(py))
            .map_err(PyRuntimeError::new_err)?
    }

    fn close(&mut self, py: Python<'_>) {
        let ticket = self.ticket.take();
        py.detach(move || drop(ticket));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_cancelled_front_middle_and_last_borrow_release() {
        let gate = Arc::new(Gate::default());
        let first = gate.ticket().unwrap();
        let cancelled = gate.ticket().unwrap();
        let last = gate.ticket().unwrap();
        assert!(!last.try_acquire().unwrap());
        drop(cancelled);
        assert!(first.try_acquire().unwrap());
        let borrow = first.clone();
        drop(first);
        assert!(!last.try_acquire().unwrap());
        drop(borrow);
        assert!(last.try_acquire().unwrap());
        drop(last);
        let cancelled = gate.ticket().unwrap();
        let successor = gate.ticket().unwrap();
        drop(cancelled);
        assert!(successor.try_acquire().unwrap());
    }

    #[test]
    fn scoped_child_borrow_and_unwind_restore_actual_thread_owner() {
        let gate = Arc::new(Gate::default());
        let owner = gate.ticket().unwrap();
        assert!(owner.try_acquire().unwrap());
        owner
            .with_current(|| {
                let borrow = current().unwrap();
                std::thread::scope(|scope| {
                    scope.spawn(|| {
                        assert!(current().is_none());
                        borrow
                            .with_current(|| assert!(current().unwrap().is_active()))
                            .unwrap();
                        assert!(current().is_none());
                    });
                });
                let panic = std::panic::catch_unwind(|| {
                    owner
                        .with_current(|| panic!("controlled job unwind"))
                        .unwrap();
                });
                assert!(panic.is_err());
                assert!(current().unwrap().is_active());
            })
            .unwrap();
        assert!(current().is_none());
        let next = gate.ticket().unwrap();
        drop(owner);
        assert!(next.try_acquire().unwrap());
    }

    #[test]
    fn poisoned_gate_refuses_new_admission_but_cleanup_does_not_panic() {
        let gate = Arc::new(Gate::default());
        let owner = gate.ticket().unwrap();
        let _ = std::panic::catch_unwind(|| {
            let _lock = gate.state.lock().unwrap();
            panic!("controlled gate poison");
        });
        assert!(gate.ticket().is_err());
        assert!(owner.try_acquire().is_err());
        drop(owner);
    }
}
