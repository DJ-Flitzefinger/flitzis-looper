use super::{ALLOCATOR, ThreadAllocations, observe_current_thread};
use std::alloc::{GlobalAlloc, Layout, handle_alloc_error};
use std::any::Any;
use std::hint::{black_box, spin_loop};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

// Invoke the existing allocator directly and use its payload, so calibration
// cannot be satisfied by an unused source-level allocation optimized away.
struct UsedAllocation {
    pointer: NonNull<u8>,
    layout: Layout,
}

impl UsedAllocation {
    fn new(bytes: usize, zeroed: bool) -> Self {
        assert!(bytes > 0);
        let layout = Layout::array::<u8>(bytes).expect("valid calibration layout");
        // SAFETY: the layout is valid and nonzero; this block owns the result.
        let pointer = unsafe {
            if zeroed {
                ALLOCATOR.alloc_zeroed(layout)
            } else {
                ALLOCATOR.alloc(layout)
            }
        };
        let pointer = NonNull::new(pointer).unwrap_or_else(|| handle_alloc_error(layout));
        Self { pointer, layout }
    }

    fn resize(&mut self, bytes: usize) {
        assert!(bytes > 0);
        let layout = Layout::array::<u8>(bytes).expect("valid calibration resize");
        // SAFETY: this block exclusively owns the original allocation and layout.
        let pointer = unsafe { ALLOCATOR.realloc(self.pointer.as_ptr(), self.layout, bytes) };
        let pointer = NonNull::new(pointer).unwrap_or_else(|| handle_alloc_error(layout));
        self.pointer = pointer;
        self.layout = layout;
    }

    fn write_and_read(&mut self, index: usize, value: u8) -> u8 {
        assert!(index < self.layout.size());
        // SAFETY: this is an in-bounds byte of the exclusively owned allocation.
        unsafe {
            self.pointer.as_ptr().add(index).write_volatile(value);
            self.pointer.as_ptr().add(index).read_volatile()
        }
    }

    // SAFETY: the selected byte must already be initialized.
    unsafe fn read(&self, index: usize) -> u8 {
        assert!(index < self.layout.size());
        // SAFETY: the index is in bounds and callers guarantee initialization.
        unsafe { self.pointer.as_ptr().add(index).read_volatile() }
    }
}

impl Drop for UsedAllocation {
    fn drop(&mut self) {
        // SAFETY: match the same allocator and current layout, including after resize.
        unsafe { ALLOCATOR.dealloc(self.pointer.as_ptr(), self.layout) };
    }
}

#[test]
fn used_alloc_zeroed_and_realloc_requests_have_exact_counts_and_bytes() {
    let (used_bytes, observed) = observe_current_thread(|| {
        let mut allocation = UsedAllocation::new(16, false);
        let initial = allocation.write_and_read(15, 0xa5);
        allocation.resize(48);
        // SAFETY: successful realloc preserves the byte written above.
        let retained = unsafe { allocation.read(15) };
        let grown = allocation.write_and_read(47, 0x3c);
        let zeroed = UsedAllocation::new(24, true);
        // SAFETY: alloc_zeroed initializes every byte in this allocation.
        let zeroed_bytes = unsafe { (zeroed.read(0), zeroed.read(23)) };
        (initial, retained, grown, zeroed_bytes.0, zeroed_bytes.1)
    });

    assert_eq!(used_bytes, (0xa5, 0xa5, 0x3c, 0, 0));
    assert_eq!(
        observed,
        ThreadAllocations {
            allocations: 1,
            zeroed_allocations: 1,
            reallocations: 1,
            requested_bytes: 16 + 48 + 24,
        }
    );
    let ((), next) = observe_current_thread(|| ());
    assert_eq!(next, ThreadAllocations::default());
}

#[test]
fn freeing_an_existing_block_is_not_an_allocation_request() {
    let allocation = UsedAllocation::new(32, false);
    let ((), observed) = observe_current_thread(|| drop(allocation));
    assert_eq!(observed, ThreadAllocations::default());
}

#[test]
fn another_threads_used_large_allocation_is_excluded() {
    const BYTES: usize = 1024 * 1024;
    let handshake = Arc::new((AtomicBool::new(false), AtomicBool::new(false)));
    let worker_handshake = Arc::clone(&handshake);
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !worker_handshake.0.load(Ordering::Acquire) {
            if Instant::now() >= deadline {
                return None;
            }
            spin_loop();
        }
        let measured = observe_current_thread(|| {
            let mut allocation = UsedAllocation::new(BYTES, false);
            (
                allocation.write_and_read(0, 0x2d),
                allocation.write_and_read(BYTES - 1, 0x6e),
            )
        });
        worker_handshake.1.store(true, Ordering::Release);
        Some(measured)
    });

    // This handshake tests thread isolation only; it is not an audio callback.
    // Host timeout diagnoses a failed worker rather than hanging the test runner.
    let deadline = Instant::now() + Duration::from_secs(5);
    let (completed, caller) = observe_current_thread(|| {
        handshake.0.store(true, Ordering::Release);
        while !handshake.1.load(Ordering::Acquire) {
            if Instant::now() >= deadline {
                return false;
            }
            spin_loop();
        }
        true
    });
    let result = worker.join().expect("allocation worker completed");
    assert!(completed, "allocation worker exceeded its host wait");
    let (used_bytes, worker) = result.expect("allocation worker received its start signal");

    assert_eq!(caller, ThreadAllocations::default());
    assert_eq!(used_bytes, (0x2d, 0x6e));
    assert_eq!(
        worker,
        ThreadAllocations {
            allocations: 1,
            requested_bytes: BYTES,
            ..ThreadAllocations::default()
        }
    );
}

#[test]
fn unwinding_closes_and_clears_the_current_thread_observation() {
    let payload: Box<dyn Any + Send> = Box::new(17usize);
    let failure = catch_unwind(AssertUnwindSafe(|| {
        observe_current_thread::<()>(|| {
            let mut allocation = UsedAllocation::new(32, false);
            black_box(allocation.write_and_read(31, 0x5a));
            resume_unwind(payload)
        })
    }));
    assert!(failure.is_err());
    drop(failure);

    let ((), observed) = observe_current_thread(|| ());
    assert_eq!(observed, ThreadAllocations::default());
}

#[test]
fn rejected_nested_scopes_leave_the_outer_scope_active_and_then_close() {
    let ((first_rejected, second_rejected), _) = observe_current_thread(|| {
        let first = catch_unwind(|| observe_current_thread(|| ()));
        let second = catch_unwind(|| observe_current_thread(|| ()));
        (first.is_err(), second.is_err())
    });

    assert!(first_rejected);
    assert!(second_rejected);
    let ((), observed) = observe_current_thread(|| ());
    assert_eq!(observed, ThreadAllocations::default());
}

static THREAD_EXIT_ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct AllocateAtThreadExit;

impl Drop for AllocateAtThreadExit {
    fn drop(&mut self) {
        let mut allocation = UsedAllocation::new(4096, false);
        black_box(allocation.write_and_read(4095, 0x7e));
        THREAD_EXIT_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

thread_local! {
    static ALLOCATE_AT_THREAD_EXIT: AllocateAtThreadExit = const { AllocateAtThreadExit };
}

#[test]
fn allocation_from_a_thread_local_destructor_remains_safe() {
    let before = THREAD_EXIT_ALLOCATIONS.load(Ordering::Relaxed);
    let worker = thread::spawn(|| {
        ALLOCATE_AT_THREAD_EXIT.with(|_| ());
        observe_current_thread(|| ()).1
    });

    let observed = worker.join().expect("thread-local destructor completed");
    assert_eq!(observed, ThreadAllocations::default());
    assert_eq!(THREAD_EXIT_ALLOCATIONS.load(Ordering::Relaxed), before + 1);
}
