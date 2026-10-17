//! Windows test-only observation of completed productive Native lifetimes.
//!
//! Enable once in a fresh isolated child, before creating its Native owners.
//! Hooks write fixed scalar cells only; decoding and allocation are control-only.

use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};

pub(crate) const EVENT_CAPACITY: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum NativeEventKind {
    Created = 1,
    DeleteCompleted = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum CallbackRole {
    Outside = 0,
    Render = 1,
    Close = 2,
    Calibration = 3,
}

thread_local! {
    static ROLE: Cell<u8> = const { Cell::new(CallbackRole::Outside as u8) };
}

pub(crate) struct RoleGuard {
    previous: u8,
    // A TLS guard must be restored on the thread where it was entered.
    _same_thread: PhantomData<*mut ()>,
}

pub(crate) fn enter_callback_role(role: CallbackRole) -> RoleGuard {
    RoleGuard {
        previous: ROLE.with(|current| current.replace(role as u8)),
        _same_thread: PhantomData,
    }
}

impl Drop for RoleGuard {
    fn drop(&mut self) {
        ROLE.with(|current| current.set(self.previous));
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "GetCurrentThreadId"]
    fn get_current_thread_id() -> u32;
}

pub(crate) fn current_windows_thread_id() -> u32 {
    // SAFETY: this Windows call takes no pointers and is valid on every caller.
    unsafe { get_current_thread_id() }
}

struct EventCell {
    complete: AtomicBool,
    kind: AtomicU8,
    native_id: AtomicU64,
    handle: AtomicUsize,
    thread: AtomicU32,
    role: AtomicU8,
}

impl EventCell {
    const fn new() -> Self {
        Self {
            complete: AtomicBool::new(false),
            kind: AtomicU8::new(0),
            native_id: AtomicU64::new(0),
            handle: AtomicUsize::new(0),
            thread: AtomicU32::new(0),
            role: AtomicU8::new(0),
        }
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static NEXT_NATIVE: AtomicU64 = AtomicU64::new(1);
static NEXT_EVENT: AtomicUsize = AtomicUsize::new(0);
static OVERFLOWED: AtomicBool = AtomicBool::new(false);
static EVENTS: [EventCell; EVENT_CAPACITY] = [const { EventCell::new() }; EVENT_CAPACITY];

/// Control-only setup. There is deliberately no reset while owners can exist.
pub(crate) fn enable() {
    assert!(
        ENABLED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok(),
        "enable Native lifecycle observation once per isolated child"
    );
}

fn record(kind: NativeEventKind, native_id: u64, handle: usize) {
    let index = NEXT_EVENT.fetch_add(1, Ordering::AcqRel);
    let Some(cell) = EVENTS.get(index) else {
        OVERFLOWED.store(true, Ordering::Release);
        return;
    };
    cell.kind.store(kind as u8, Ordering::Relaxed);
    cell.native_id.store(native_id, Ordering::Relaxed);
    cell.handle.store(handle, Ordering::Relaxed);
    cell.thread
        .store(current_windows_thread_id(), Ordering::Relaxed);
    cell.role.store(ROLE.with(Cell::get), Ordering::Relaxed);
    cell.complete.store(true, Ordering::Release);
}

/// Called only after the productive C ABI returned a non-null actual handle.
pub(crate) fn native_created(handle: usize) -> u64 {
    if !ENABLED.load(Ordering::Acquire) {
        return 0;
    }
    let native_id = NEXT_NATIVE.fetch_add(1, Ordering::Relaxed);
    record(NativeEventKind::Created, native_id, handle);
    native_id
}

/// Called only after the productive C ABI deletion has actually returned.
pub(crate) fn native_delete_completed(native_id: u64, handle: usize) {
    if native_id != 0 {
        record(NativeEventKind::DeleteCompleted, native_id, handle);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeEvent {
    pub sequence: u64,
    pub kind: NativeEventKind,
    pub native_id: u64,
    pub opaque_handle: usize,
    pub windows_thread_id: u32,
    pub role: CallbackRole,
}

pub(crate) struct Snapshot {
    pub events: Vec<NativeEvent>,
    pub reserved: usize,
    pub incomplete: usize,
    pub overflowed: bool,
}

impl Snapshot {
    /// Control-only lookup. Match lifetime IDs, never the last event by pointer:
    /// another thread may reuse an address before the old delete hook records.
    /// Ambiguous in-flight reuse returns None; retry only outside the callback.
    pub(crate) fn live_id(&self, handle: usize) -> Option<u64> {
        let mut live = self.events.iter().filter(|event| {
            event.kind == NativeEventKind::Created
                && event.opaque_handle == handle
                && !self.events.iter().any(|deleted| {
                    deleted.kind == NativeEventKind::DeleteCompleted
                        && deleted.native_id == event.native_id
                })
        });
        let id = live.next()?.native_id;
        live.next().is_none().then_some(id)
    }
}

/// Control-only allocation/decoding; never wait or decode inside a callback.
pub(crate) fn snapshot() -> Snapshot {
    let reserved = NEXT_EVENT.load(Ordering::Acquire);
    let mut result = Snapshot {
        events: Vec::with_capacity(reserved.min(EVENT_CAPACITY)),
        reserved,
        incomplete: 0,
        overflowed: reserved > EVENT_CAPACITY || OVERFLOWED.load(Ordering::Acquire),
    };
    for (index, cell) in EVENTS.iter().take(reserved).enumerate() {
        if !cell.complete.load(Ordering::Acquire) {
            result.incomplete += 1;
            continue;
        }
        let kind = match cell.kind.load(Ordering::Relaxed) {
            1 => NativeEventKind::Created,
            2 => NativeEventKind::DeleteCompleted,
            _ => panic!("invalid completed Native event kind"),
        };
        let role = match cell.role.load(Ordering::Relaxed) {
            0 => CallbackRole::Outside,
            1 => CallbackRole::Render,
            2 => CallbackRole::Close,
            3 => CallbackRole::Calibration,
            _ => panic!("invalid completed Native event role"),
        };
        result.events.push(NativeEvent {
            sequence: index as u64 + 1,
            kind,
            native_id: cell.native_id.load(Ordering::Relaxed),
            opaque_handle: cell.handle.load(Ordering::Relaxed),
            windows_thread_id: cell.thread.load(Ordering::Relaxed),
            role,
        });
    }
    result
}
