//! Backing for the diagnostic arena, not a complete native process allowance.
use std::alloc::Layout;

pub(super) struct Allocation {
    pub base: *mut u8,
    // Only a dedicated Windows mapping has a known page-rounded extent here.
    // This excludes native API costs and all other process allocations.
    pub mapped_bytes: Option<usize>,
}

#[cfg(any(windows, test))]
fn page_extent(bytes: usize, page: usize) -> Result<usize, &'static str> {
    if page < super::ALIGN || !page.is_power_of_two() {
        return Err("invalid workspace backing page size");
    }
    super::round_up(bytes, page)
        .filter(|&rounded| rounded <= isize::MAX as usize)
        .ok_or("workspace backing extent overflow")
}

#[cfg(windows)]
mod windows {
    use super::{Allocation, Layout, page_extent};
    use std::{ffi::c_void, ptr};

    // SYSTEM_INFO's first union occupies one DWORD. Pointer-sized fields follow
    // the two DWORDs; repr(C) supplies the target's pointer/structure alignment.
    #[repr(C)]
    struct SystemInfo {
        architecture: u32,
        page_size: u32,
        minimum_address: *mut c_void,
        maximum_address: *mut c_void,
        active_processor_mask: usize,
        processors: u32,
        processor_type: u32,
        allocation_granularity: u32,
        processor_level: u16,
        processor_revision: u16,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemInfo(info: *mut SystemInfo);
        fn VirtualAlloc(address: *mut c_void, bytes: usize, kind: u32, protect: u32)
        -> *mut c_void;
        #[cfg(test)]
        fn VirtualFree(address: *mut c_void, bytes: usize, kind: u32) -> i32;
    }

    pub(in super::super) fn allocate(layout: Layout) -> Result<Allocation, &'static str> {
        // SAFETY: all-zero fields are valid C integers/pointers; GetSystemInfo
        // writes this correctly sized, exclusively borrowed SYSTEM_INFO.
        let mut info: SystemInfo = unsafe { std::mem::zeroed() };
        unsafe { GetSystemInfo(&mut info) };
        // Commitment rounds to dwPageSize, NOT dwAllocationGranularity.
        let bytes = page_extent(layout.size(), info.page_size as usize)?;
        // SAFETY: NULL chooses a fresh process-local, allocation-granularity-
        // aligned region. MEM_RESERVE|MEM_COMMIT and PAGE_READWRITE give private
        // zero-initialized backing. No large-page/locking/privilege options.
        let base = unsafe { VirtualAlloc(ptr::null_mut(), bytes, 0x3000, 0x04) }.cast::<u8>();
        if base.is_null() {
            return Err("workspace backing allocation failed");
        }
        // Windows page/allocation alignment satisfies the arena's 16B Layout.
        // Production deliberately retains the mapping until process exit.
        Ok(Allocation {
            base,
            mapped_bytes: Some(bytes),
        })
    }

    #[cfg(test)]
    pub(in super::super) unsafe fn release(base: *mut u8, _layout: Layout) {
        // SAFETY: test caller owns the original base and has ended ALL readers.
        assert_ne!(unsafe { VirtualFree(base.cast(), 0, 0x8000) }, 0);
    }
}

#[cfg(windows)]
pub(super) use windows::allocate;
#[cfg(all(windows, test))]
pub(super) use windows::release;

#[cfg(not(windows))]
pub(super) fn allocate(layout: Layout) -> Result<Allocation, &'static str> {
    use std::alloc::{GlobalAlloc, System};
    // SAFETY: valid nonzero arena Layout. Other platforms retain their existing
    // System owner; requested Layout bytes do NOT bound its native overhead.
    let base = unsafe { System.alloc_zeroed(layout) };
    if base.is_null() {
        return Err("workspace backing allocation failed");
    }
    Ok(Allocation {
        base,
        mapped_bytes: None,
    })
}

#[cfg(all(not(windows), test))]
pub(super) unsafe fn release(base: *mut u8, layout: Layout) {
    use std::alloc::{GlobalAlloc, System};
    // SAFETY: test caller owns the allocation and has ended ALL readers.
    unsafe { System.dealloc(base, layout) };
}

#[cfg(test)]
mod tests {
    use super::page_extent;

    #[test]
    fn checked_page_rounding_does_not_change_logical_capacity() {
        for page in [4096, 16384, 65536] {
            for bytes in [32, page, page + 16, 768857616] {
                let mapped = page_extent(bytes, page).unwrap();
                assert_eq!(mapped % page, 0);
                assert!(mapped >= bytes && mapped - bytes < page);
            }
        }
        assert_eq!(page_extent(768857616, 4096), Ok(768860160));
        for page in [0, 8, 24] {
            assert!(page_extent(32, page).is_err());
        }
        assert!(page_extent(usize::MAX, 4096).is_err());
        assert!(page_extent(isize::MAX as usize, 4096).is_err());
    }
}
