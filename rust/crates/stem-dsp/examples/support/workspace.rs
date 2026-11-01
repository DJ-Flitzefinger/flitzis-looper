//! Dedicated diagnostic-process allocation ownership, not whole-process admission.
//!
//! After sealing, EVERY Rust allocation uses this fixed backing, including the
//! pinned SGEMM packing and FFT planner allocations. No exhausted request falls
//! back to System. Windows pre-seal Rust pointers own individual mappings;
//! other platforms retain their original System ownership.
//! The backing deliberately survives until process exit, after all readers.
#[path = "backing.rs"]
mod backing;
#[path = "bootstrap.rs"]
mod bootstrap;

#[cfg(test)]
use std::alloc::System;

use std::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

const ALIGN: usize = align_of::<Header>();
const HEADER: usize = size_of::<Header>();
const END: usize = usize::MAX;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub(super) struct Header {
    // Free: extent + next free offset. Used: extent + original block offset.
    len: usize,
    link: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub capacity: usize,
    pub live: usize,
    pub peak: usize,
    pub allocations: usize,
    pub failures: usize,
}

// Storage alone varies in the prospective proof. Fit/split/free/coalescing and
// absolute-address arithmetic below are the SAME production implementation.
pub(super) trait HeaderStore {
    unsafe fn read(&self, base: *mut u8, offset: usize) -> Header;
    unsafe fn write(&mut self, base: *mut u8, offset: usize, header: Header);
}

struct NativeHeaders;
impl HeaderStore for NativeHeaders {
    unsafe fn read(&self, base: *mut u8, offset: usize) -> Header {
        unsafe { base.add(offset).cast::<Header>().read() }
    }
    unsafe fn write(&mut self, base: *mut u8, offset: usize, header: Header) {
        unsafe { base.add(offset).cast::<Header>().write(header) };
    }
}

struct Pool<S = NativeHeaders> {
    base: *mut u8,
    first: usize,
    stats: Stats,
    headers: S,
}

impl Pool {
    const fn empty() -> Self {
        Self {
            base: ptr::null_mut(),
            first: END,
            stats: Stats {
                capacity: 0,
                live: 0,
                peak: 0,
                allocations: 0,
                failures: 0,
            },
            headers: NativeHeaders,
        }
    }
}

impl<S: HeaderStore> Pool<S> {
    fn contains(&self, address: *mut u8) -> bool {
        !self.base.is_null()
            && (address as usize).wrapping_sub(self.base as usize) < self.stats.capacity
    }

    // SAFETY for header helpers: offsets identify aligned, initialized headers
    // inside the exclusively locked backing. Payload is never read as a header.
    unsafe fn read(&self, offset: usize) -> Header {
        unsafe { self.headers.read(self.base, offset) }
    }

    unsafe fn write(&mut self, offset: usize, header: Header) {
        unsafe { self.headers.write(self.base, offset, header) };
    }

    unsafe fn allocate(&mut self, layout: Layout) -> *mut u8 {
        let mut previous = END;
        let mut offset = self.first;
        while offset != END {
            let block = unsafe { self.read(offset) };
            // Align the ABSOLUTE address, including over-aligned callers.
            let address = (self.base as usize)
                .checked_add(offset)
                .and_then(|n| n.checked_add(HEADER))
                .and_then(|n| round_up(n, layout.align().max(ALIGN)));
            let end = address
                .and_then(|n| n.checked_add(layout.size().max(1)))
                .and_then(|n| round_up(n, ALIGN))
                .and_then(|n| n.checked_sub(self.base as usize));
            if let (Some(address), Some(end)) = (address, end) {
                let required = end - offset;
                if required <= block.len {
                    // A tiny tail belongs to this allocation, never an invalid
                    // free-list node. All alignment padding is returned on free.
                    let split = block.len - required >= HEADER + ALIGN;
                    let used = if split { required } else { block.len };
                    let next = if split { offset + used } else { block.link };
                    if split {
                        unsafe {
                            self.write(
                                next,
                                Header {
                                    len: block.len - used,
                                    link: block.link,
                                },
                            )
                        };
                    }
                    if previous == END {
                        self.first = next;
                    } else {
                        let mut before = unsafe { self.read(previous) };
                        before.link = next;
                        unsafe { self.write(previous, before) };
                    }
                    let header = address - self.base as usize - HEADER;
                    unsafe {
                        self.write(
                            header,
                            Header {
                                len: used,
                                link: offset,
                            },
                        )
                    };
                    self.stats.live += used;
                    self.stats.peak = self.stats.peak.max(self.stats.live);
                    self.stats.allocations = self.stats.allocations.saturating_add(1);
                    return address as *mut u8;
                }
            }
            previous = offset;
            offset = block.link;
        }
        self.stats.failures = self.stats.failures.saturating_add(1);
        ptr::null_mut()
    }

    unsafe fn release(&mut self, address: *mut u8) {
        let used = unsafe { self.read(address as usize - self.base as usize - HEADER) };
        let offset = used.link;
        self.stats.live -= used.len;
        let mut previous = END;
        let mut next = self.first;
        while next != END && next < offset {
            previous = next;
            next = unsafe { self.read(next) }.link;
        }
        let mut free = Header {
            len: used.len,
            link: next,
        };
        if next != END && offset + free.len == next {
            let after = unsafe { self.read(next) };
            free.len += after.len;
            free.link = after.link;
        }
        if previous == END {
            self.first = offset;
        } else {
            let mut before = unsafe { self.read(previous) };
            if previous + before.len == offset {
                before.len += free.len;
                before.link = free.link;
                unsafe { self.write(previous, before) };
                return;
            }
            before.link = offset;
            unsafe { self.write(previous, before) };
        }
        unsafe { self.write(offset, free) };
    }
}

fn round_up(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment - 1)
        .map(|n| n & !(alignment - 1))
}

pub struct WorkspaceAllocator {
    locked: AtomicBool,
    pool: UnsafeCell<Pool>,
    mapped_bytes: UnsafeCell<Option<usize>>,
    bootstrap: UnsafeCell<bootstrap::Bootstrap>,
}

// SAFETY: all access to Pool (including initialization) holds the allocation-free
// atomic lock, also protecting mapped_bytes/bootstrap. No state reference
// escapes it. Native acquisition occurs before sealing, while pre-seal owner
// disposal can follow sealing. Neither backing adapter reenters this allocator.
unsafe impl Sync for WorkspaceAllocator {}

struct Guard<'a>(&'a WorkspaceAllocator);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.locked.store(false, Ordering::Release);
    }
}

impl WorkspaceAllocator {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            pool: UnsafeCell::new(Pool::empty()),
            mapped_bytes: UnsafeCell::new(None),
            bootstrap: UnsafeCell::new(bootstrap::Bootstrap::new()),
        }
    }

    fn lock(&self) -> Guard<'_> {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        Guard(self)
    }

    /// Allocate/zero the fixed backing BEFORE any learned execution. This does
    /// not authorize a model run or account for non-Rust process allocations.
    /// There is deliberately no unseal, resize or return-to-System operation.
    pub fn seal(&self, bytes: usize) -> Result<(), &'static str> {
        self.seal_with(bytes, backing::allocate)
    }

    fn seal_with(
        &self,
        bytes: usize,
        allocate: impl FnOnce(Layout) -> Result<backing::Allocation, &'static str>,
    ) -> Result<(), &'static str> {
        let _guard = self.lock();
        // SAFETY: exclusive lock; no reference escapes.
        let pool = unsafe { &mut *self.pool.get() };
        if !pool.base.is_null() {
            return Err("workspace already sealed");
        }
        let layout = Self::validate_extent(bytes)?;
        // Obtain backing directly, outside GlobalAlloc. Publish no pool state
        // on failure. Windows uses a dedicated page-rounded mapping; other
        // platforms retain System ownership. Neither proves physical residency.
        let allocation = allocate(layout)?;
        pool.base = allocation.base;
        unsafe { *self.mapped_bytes.get() = allocation.mapped_bytes };
        pool.first = 0;
        pool.stats.capacity = bytes;
        unsafe {
            pool.write(
                0,
                Header {
                    len: bytes,
                    link: END,
                },
            )
        };
        Ok(())
    }

    /// Share the exact extent validation with pre-seal I/O preparation. Invalid
    /// requests must reject before creating any private directory or files.
    pub fn validate_extent(bytes: usize) -> Result<Layout, &'static str> {
        if bytes < HEADER + ALIGN || !bytes.is_multiple_of(ALIGN) {
            return Err("invalid aligned workspace extent");
        }
        Layout::from_size_align(bytes, ALIGN).map_err(|_| "workspace extent overflow")
    }

    pub fn stats(&self) -> Stats {
        let _guard = self.lock();
        // SAFETY: lock protects this copy; no allocator activity is needed.
        unsafe { (*self.pool.get()).stats }
    }

    /// Extent of this Windows mapping ONLY; None for an unsealed or System
    /// backing. Page padding is never usable pool capacity. This is not a
    /// native/process allowance and must not be added again inside PrivateUsage.
    pub fn mapped_bytes(&self) -> Option<usize> {
        let _guard = self.lock();
        // SAFETY: the same lock protects publication and this copy.
        unsafe { *self.mapped_bytes.get() }
    }

    /// Current individually owned Windows pre-seal Rust regions ONLY. This
    /// excludes native API/runtime/stack/images and is already in PrivateUsage.
    /// None on other platforms; not a prospective complete process allowance.
    pub fn bootstrap_stats(&self) -> Option<bootstrap::Stats> {
        let _guard = self.lock();
        unsafe { (*self.bootstrap.get()).stats() }
    }
}

// SAFETY: Pool validates layout arithmetic and maintains disjoint aligned live
// extents. Each deallocation returns its entire extent with adjacent coalescing.
// The backing is never released while any caller/reader could retain a pointer.
unsafe impl GlobalAlloc for WorkspaceAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _guard = self.lock();
        let pool = unsafe { &mut *self.pool.get() };
        if pool.base.is_null() {
            unsafe { (*self.bootstrap.get()).allocate(layout) }
        } else {
            unsafe { pool.allocate(layout) }
        }
    }

    unsafe fn dealloc(&self, address: *mut u8, layout: Layout) {
        let _guard = self.lock();
        let pool = unsafe { &mut *self.pool.get() };
        if pool.contains(address) {
            unsafe { pool.release(address) };
        } else if !unsafe { (*self.bootstrap.get()).release(address, layout) } {
            // GlobalAlloc has no deallocation error channel. A failed
            // native release is terminal HOST failure, not settled success.
            std::process::abort();
        }
    }
    // GlobalAlloc defaults implement alloc_zeroed via alloc+write_bytes and
    // realloc via alloc+copy+dealloc, preserving old storage on allocation failure.
    // Thus neither operation can escape the sealed arena via System.realloc.
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "allocation_proof.rs"]
mod allocation_proof;
