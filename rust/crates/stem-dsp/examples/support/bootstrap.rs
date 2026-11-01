//! Rust pre-seal ownership only; native API/runtime/process costs remain unknown.
use std::alloc::Layout;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub live_regions: usize,
    pub payload_bytes: usize,
    pub mapped_bytes: usize,
}

pub struct Bootstrap {
    #[cfg(windows)]
    stats: Stats,
}

impl Bootstrap {
    pub const fn new() -> Self {
        Self {
            #[cfg(windows)]
            stats: Stats {
                live_regions: 0,
                payload_bytes: 0,
                mapped_bytes: 0,
            },
        }
    }

    pub fn stats(&self) -> Option<Stats> {
        #[cfg(windows)]
        {
            Some(self.stats)
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    #[cfg(not(windows))]
    pub unsafe fn allocate(&mut self, layout: Layout) -> *mut u8 {
        use std::alloc::{GlobalAlloc, System};
        unsafe { System.alloc(layout) }
    }

    #[cfg(not(windows))]
    pub unsafe fn release(&mut self, address: *mut u8, layout: Layout) -> bool {
        use std::alloc::{GlobalAlloc, System};
        unsafe { System.dealloc(address, layout) };
        true
    }
}

#[cfg(windows)]
#[derive(Clone, Copy)]
#[repr(C)]
struct Header {
    base: *mut u8,
    mapped_bytes: usize,
}

#[cfg(windows)]
fn mapping_layout(layout: Layout) -> Option<Layout> {
    let alignment = layout.align().max(align_of::<Header>());
    let bytes = size_of::<Header>()
        .checked_add(alignment - 1)?
        .checked_add(layout.size())?;
    // The existing backing adapter promises the arena's ALIGN, not arbitrary
    // caller alignment. This adapter aligns the payload INSIDE that region.
    Layout::from_size_align(bytes, super::ALIGN).ok()
}

#[cfg(windows)]
impl Bootstrap {
    pub unsafe fn allocate(&mut self, layout: Layout) -> *mut u8 {
        unsafe { self.allocate_with(layout, super::backing::allocate) }
    }

    unsafe fn allocate_with(
        &mut self,
        layout: Layout,
        allocate: impl FnOnce(Layout) -> Result<super::backing::Allocation, &'static str>,
    ) -> *mut u8 {
        let Some(mapping) = mapping_layout(layout) else {
            return std::ptr::null_mut();
        };
        let Ok(owner) = allocate(mapping) else {
            return std::ptr::null_mut();
        };
        // The Windows backend always returns Some. A violated internal backend
        // contract cannot unwind/reenter a global allocator while locked.
        let Some(mapped_bytes) = owner.mapped_bytes else {
            std::process::abort()
        };
        let alignment = layout.align().max(align_of::<Header>());
        let start = unsafe { owner.base.add(size_of::<Header>()) };
        let payload = unsafe { start.add(start.align_offset(alignment)) };
        unsafe {
            payload
                .sub(size_of::<Header>())
                .cast::<Header>()
                .write(Header {
                    base: owner.base,
                    mapped_bytes,
                });
        }
        // Live regions are disjoint portions of this process's address space;
        // their extent sum, payload sum and count therefore fit usize.
        self.stats.live_regions += 1;
        self.stats.payload_bytes += layout.size();
        self.stats.mapped_bytes += mapped_bytes;
        payload
    }

    pub unsafe fn release(&mut self, address: *mut u8, layout: Layout) -> bool {
        unsafe { self.release_with(address, layout, |base| super::backing::try_release(base)) }
    }

    unsafe fn release_with(
        &mut self,
        address: *mut u8,
        layout: Layout,
        release: impl FnOnce(*mut u8) -> bool,
    ) -> bool {
        // GlobalAlloc requires an original live pointer and matching Layout,
        // after its last reader. Read the header BEFORE releasing its region.
        let header = unsafe { address.sub(size_of::<Header>()).cast::<Header>().read() };
        if !release(header.base) {
            // Do not falsely settle an owner. The GlobalAlloc caller aborts
            // without formatting/unwinding; a local test can inspect this state.
            return false;
        }
        self.stats.live_regions -= 1;
        self.stats.payload_bytes -= layout.size();
        self.stats.mapped_bytes -= header.mapped_bytes;
        true
    }
}

#[cfg(all(windows, test))]
#[path = "bootstrap_tests.rs"]
mod tests;
