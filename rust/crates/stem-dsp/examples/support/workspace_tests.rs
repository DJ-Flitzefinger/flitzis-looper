use super::*;

struct Local(WorkspaceAllocator);
impl Local {
    fn new(bytes: usize) -> Self {
        let allocator = WorkspaceAllocator::new();
        allocator.seal(bytes).unwrap();
        Self(allocator)
    }
}
impl Drop for Local {
    fn drop(&mut self) {
        let pool = self.0.pool.get_mut();
        assert_eq!(pool.stats.live, 0);
        // Only isolated tests release backing, after ALL pointers have ended.
        unsafe {
            System.dealloc(
                pool.base,
                Layout::from_size_align(pool.stats.capacity, ALIGN).unwrap(),
            )
        };
    }
}

#[test]
fn mixed_alignment_and_fragmented_free_coalesce_without_payload_aliasing() {
    let arena = Local::new(64 * 1024);
    let mut values = Vec::new();
    for i in 0..80 {
        let layout = Layout::from_size_align(i * 13 + 1, 1 << (i % 10)).unwrap();
        let address = unsafe { arena.0.alloc(layout) };
        assert!(!address.is_null());
        assert_eq!(address as usize % layout.align(), 0);
        unsafe { address.write_bytes(i as u8, layout.size()) };
        values.push((address, layout, i as u8));
    }
    // Free alternating holes; verify every other live payload after each free.
    for parity in 0..2 {
        for i in (parity..values.len()).step_by(2) {
            let (address, layout, byte) = values[i];
            assert!(
                unsafe { std::slice::from_raw_parts(address, layout.size()) }
                    .iter()
                    .all(|&x| x == byte)
            );
            unsafe { arena.0.dealloc(address, layout) };
        }
    }
    assert_eq!(arena.0.stats().live, 0);
    // Nearly all backing is available again, not just the largest former hole.
    let layout = Layout::from_size_align(64 * 1024 - HEADER, ALIGN).unwrap();
    let address = unsafe { arena.0.alloc(layout) };
    assert!(!address.is_null());
    unsafe { arena.0.dealloc(address, layout) };
}

#[test]
fn recycled_zeroes_and_realloc_preserve_data_and_old_storage_on_failure() {
    let arena = Local::new(4096);
    let layout = Layout::from_size_align(256, 64).unwrap();
    let first = unsafe { arena.0.alloc(layout) };
    assert!(!first.is_null());
    unsafe {
        first.write_bytes(0xa5, 256);
        arena.0.dealloc(first, layout)
    };
    let zero = unsafe { arena.0.alloc_zeroed(layout) };
    assert!(!zero.is_null());
    assert!(
        unsafe { std::slice::from_raw_parts(zero, 256) }
            .iter()
            .all(|&x| x == 0)
    );
    unsafe { zero.write_bytes(0x37, 256) };
    let grown = unsafe { arena.0.realloc(zero, layout, 512) };
    assert!(!grown.is_null());
    assert!(
        unsafe { std::slice::from_raw_parts(grown, 256) }
            .iter()
            .all(|&x| x == 0x37)
    );
    let grown_layout = Layout::from_size_align(512, 64).unwrap();
    let failed = unsafe { arena.0.realloc(grown, grown_layout, 8192) };
    assert!(failed.is_null());
    assert!(
        unsafe { std::slice::from_raw_parts(grown, 256) }
            .iter()
            .all(|&x| x == 0x37)
    );
    let shrunk = unsafe { arena.0.realloc(grown, grown_layout, 128) };
    assert!(!shrunk.is_null());
    assert!(
        unsafe { std::slice::from_raw_parts(shrunk, 128) }
            .iter()
            .all(|&x| x == 0x37)
    );
    unsafe {
        arena
            .0
            .dealloc(shrunk, Layout::from_size_align(128, 64).unwrap())
    };
    assert_eq!(arena.0.stats().failures, 1);
}

#[test]
fn preseal_system_pointers_migrate_on_realloc_and_exhaustion_never_falls_back() {
    let allocator = WorkspaceAllocator::new();
    let layout = Layout::from_size_align(64, 32).unwrap();
    let old = unsafe { allocator.alloc(layout) };
    assert!(!old.is_null());
    unsafe { old.write_bytes(0x19, 64) };
    allocator.seal(1024).unwrap();
    let arena = Local(allocator);
    assert!(arena.0.seal(1024).is_err());
    let migrated = unsafe { arena.0.realloc(old, layout, 128) };
    assert!(!migrated.is_null());
    assert!(unsafe { (*arena.0.pool.get()).contains(migrated) });
    assert!(
        unsafe { std::slice::from_raw_parts(migrated, 64) }
            .iter()
            .all(|&x| x == 0x19)
    );
    let enormous = Layout::from_size_align(isize::MAX as usize - 15, 16).unwrap();
    assert!(unsafe { arena.0.alloc(enormous) }.is_null());
    assert!(unsafe { arena.0.alloc(Layout::from_size_align(2048, 8192).unwrap()) }.is_null());
    unsafe {
        arena
            .0
            .dealloc(migrated, Layout::from_size_align(128, 32).unwrap())
    };
    assert_eq!(arena.0.stats().live, 0);
    assert!(WorkspaceAllocator::new().seal(0).is_err());
    assert!(WorkspaceAllocator::new().seal(33).is_err());
    assert!(WorkspaceAllocator::new().seal(usize::MAX - 15).is_err());
}

#[test]
fn concurrent_callers_preserve_payloads_and_settle_every_extent() {
    let arena = Local::new(64 * 1024);
    std::thread::scope(|scope| {
        for worker in 0..8 {
            let allocator = &arena.0;
            scope.spawn(move || {
                for i in 0..200 {
                    let layout = Layout::from_size_align(128 + i % 64, 64).unwrap();
                    let address = unsafe { allocator.alloc(layout) };
                    assert!(!address.is_null());
                    unsafe { address.write_bytes(worker, layout.size()) };
                    std::thread::yield_now();
                    assert!(
                        unsafe { std::slice::from_raw_parts(address, layout.size()) }
                            .iter()
                            .all(|&x| x == worker)
                    );
                    unsafe { allocator.dealloc(address, layout) };
                }
            });
        }
    });
    assert_eq!(arena.0.stats().live, 0);
    assert_eq!(arena.0.stats().allocations, 1600);
    assert_eq!(arena.0.stats().failures, 0);
}
