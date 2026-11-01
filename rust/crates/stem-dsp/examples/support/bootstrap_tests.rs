use super::*;

#[test]
fn checked_prefix_and_refused_mapping_publish_no_owner() {
    let mut bootstrap = Bootstrap::new();
    let huge = Layout::from_size_align(isize::MAX as usize, 1).unwrap();
    assert!(mapping_layout(huge).is_none());
    assert!(
        unsafe { bootstrap.allocate_with(huge, |_| panic!("must not call backing")) }.is_null()
    );
    let layout = Layout::from_size_align(17, 32).unwrap();
    assert!(unsafe { bootstrap.allocate_with(layout, |_| Err("controlled refusal")) }.is_null());
    assert_eq!(bootstrap.stats(), Some(Stats::default()));
    let address = unsafe { bootstrap.allocate(layout) };
    assert!(!address.is_null());
    assert!(unsafe { bootstrap.release(address, layout) });
    assert_eq!(bootstrap.stats(), Some(Stats::default()));
}

#[test]
fn failed_release_keeps_owner_and_payload_until_actual_release() {
    let mut bootstrap = Bootstrap::new();
    let layout = Layout::from_size_align(257, 128).unwrap();
    let address = unsafe { bootstrap.allocate(layout) };
    assert!(!address.is_null());
    unsafe { address.write_bytes(0x5a, layout.size()) };
    let before = bootstrap.stats();
    assert!(!unsafe { bootstrap.release_with(address, layout, |_| false) });
    assert_eq!(bootstrap.stats(), before);
    assert!(
        unsafe { std::slice::from_raw_parts(address, layout.size()) }
            .iter()
            .all(|&x| x == 0x5a)
    );
    assert!(unsafe { bootstrap.release(address, layout) });
    assert_eq!(bootstrap.stats(), Some(Stats::default()));
}

#[cfg(target_pointer_width = "64")]
#[test]
fn actual_windows_bootstrap_alignment_zeroing_region_and_release() {
    use std::ffi::c_void;
    #[repr(C, align(16))]
    struct MemoryInfo {
        base: u64,
        allocation_base: u64,
        allocation_protect: u32,
        alignment1: u32,
        region_bytes: u64,
        state: u32,
        protect: u32,
        kind: u32,
        alignment2: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualQuery(address: *const c_void, info: *mut MemoryInfo, bytes: usize) -> usize;
    }
    let mut bootstrap = Bootstrap::new();
    let mut owners = Vec::new();
    for (i, alignment) in [1, 2, 8, 16, 32, 4096, 131072, 262144]
        .into_iter()
        .enumerate()
    {
        let layout = Layout::from_size_align(17 + i * 503, alignment).unwrap();
        let address = unsafe { bootstrap.allocate(layout) };
        assert!(!address.is_null());
        assert_eq!(address as usize % alignment, 0);
        assert!(
            unsafe { std::slice::from_raw_parts(address, layout.size()) }
                .iter()
                .all(|&x| x == 0)
        );
        let header = unsafe { address.sub(size_of::<Header>()).cast::<Header>().read() };
        let mut info: MemoryInfo = unsafe { std::mem::zeroed() };
        assert_eq!(size_of::<MemoryInfo>(), 48);
        assert_eq!(
            unsafe { VirtualQuery(header.base.cast(), &mut info, size_of::<MemoryInfo>()) },
            size_of::<MemoryInfo>()
        );
        assert_eq!(info.base, header.base as u64);
        assert_eq!(info.allocation_base, header.base as u64);
        assert_eq!(info.region_bytes as usize, header.mapped_bytes);
        assert_eq!(
            (info.allocation_protect, info.protect, info.state, info.kind),
            (0x04, 0x04, 0x1000, 0x20000)
        );
        assert!(address as usize + layout.size() <= header.base as usize + header.mapped_bytes);
        unsafe { address.write_bytes(i as u8 + 1, layout.size()) };
        owners.push((address, layout));
    }
    let stats = bootstrap.stats().unwrap();
    assert_eq!(stats.live_regions, owners.len());
    assert_eq!(
        stats.payload_bytes,
        owners
            .iter()
            .map(|(_, layout)| layout.size())
            .sum::<usize>()
    );
    assert!(stats.mapped_bytes > stats.payload_bytes);
    for (i, (address, layout)) in owners.into_iter().enumerate() {
        assert!(
            unsafe { std::slice::from_raw_parts(address, layout.size()) }
                .iter()
                .all(|&x| x == i as u8 + 1)
        );
        assert!(unsafe { bootstrap.release(address, layout) });
        // Successful VirtualFree and settled counters establish release. A later
        // VirtualQuery cannot require MEM_FREE: another thread may reuse the VA.
    }
    assert_eq!(bootstrap.stats(), Some(Stats::default()));
}
