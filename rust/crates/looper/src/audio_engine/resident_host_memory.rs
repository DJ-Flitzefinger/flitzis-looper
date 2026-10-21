//! Prospective resident allocation promises. OS queries and this gate are never realtime.
//!
//! Existing PCM already occupies host memory. Only bytes not yet committed to an
//! actual backing are promised here, across every engine/account/worker.
//! A partially initialized backing remains promised until owner registration;
//! another admission may conservatively count those bytes both in the OS image
//! and here. Queries are sequential observations, never allocation guarantees.

use std::sync::{Arc, Mutex, OnceLock};

const MIB: u64 = 1024 * 1024;
const PHYSICAL_RESERVE: u64 = 128 * MIB;
const COMMIT_RESERVE: u64 = 256 * MIB;

#[derive(Clone, Copy, Debug)]
pub(super) struct HostHeadroom {
    pub available_physical: u64,
    pub caller_available_commit: u64,
    pub system_available_commit: u64,
}

impl HostHeadroom {
    pub(super) fn available_commit(self) -> u64 {
        self.caller_available_commit
            .min(self.system_available_commit)
    }

    pub(super) fn allowance(self) -> u64 {
        self.available_physical
            .saturating_sub(PHYSICAL_RESERVE)
            .min(self.available_commit().saturating_sub(COMMIT_RESERVE))
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum HostAdmissionError {
    #[error(
        "[resident_memory_pressure] resident allocation needs {requested_bytes} bytes with {pending_bytes} pending and {available_bytes} host bytes available after reserve"
    )]
    Pressure {
        requested_bytes: u64,
        pending_bytes: u64,
        available_bytes: u64,
    },
    #[error("[resident_memory_unavailable] {0}")]
    Unavailable(String),
}

#[derive(Default)]
struct Promises {
    bytes: u64,
}

impl Promises {
    fn admit(&mut self, bytes: u64, image: HostHeadroom) -> Result<(), HostAdmissionError> {
        let available = image.allowance();
        let total = self.bytes.checked_add(bytes).ok_or_else(|| {
            HostAdmissionError::Unavailable("resident allocation promise overflow".into())
        })?;
        if total > available {
            return Err(HostAdmissionError::Pressure {
                requested_bytes: bytes,
                pending_bytes: self.bytes,
                available_bytes: available,
            });
        }
        self.bytes = total;
        Ok(())
    }
}

fn promises() -> &'static Arc<Mutex<Promises>> {
    static PROMISES: OnceLock<Arc<Mutex<Promises>>> = OnceLock::new();
    PROMISES.get_or_init(|| Arc::new(Mutex::new(Promises::default())))
}

pub(super) fn pending_bytes() -> Result<u64, HostAdmissionError> {
    promises()
        .lock()
        .map(|state| state.bytes)
        .map_err(|_| HostAdmissionError::Unavailable("resident host gate poisoned".into()))
}

/// Off-thread RAII ownership, released as actual PCM replaces prospective bytes.
pub(super) struct HostReservation {
    gate: Arc<Mutex<Promises>>,
    bytes: u64,
}

impl HostReservation {
    pub(super) fn reserve_with(
        bytes: usize,
        query: impl FnOnce() -> Result<HostHeadroom, String>,
    ) -> Result<Self, HostAdmissionError> {
        Self::reserve_on(promises().clone(), bytes, query)
    }

    fn reserve_on(
        gate: Arc<Mutex<Promises>>,
        bytes: usize,
        query: impl FnOnce() -> Result<HostHeadroom, String>,
    ) -> Result<Self, HostAdmissionError> {
        let bytes = u64::try_from(bytes).map_err(|_| {
            HostAdmissionError::Unavailable("resident allocation extent overflow".into())
        })?;
        if bytes != 0 {
            let mut state = gate.lock().map_err(|_| {
                HostAdmissionError::Unavailable("resident host gate poisoned".into())
            })?;
            // Query while promises are serialized. A later worker cannot use
            // the same observed headroom before this promise becomes visible.
            state.admit(bytes, query().map_err(HostAdmissionError::Unavailable)?)?;
        }
        Ok(Self { gate, bytes })
    }

    pub(super) fn commit(&mut self, bytes: usize) -> Result<(), HostAdmissionError> {
        let bytes = u64::try_from(bytes).map_err(|_| {
            HostAdmissionError::Unavailable("resident allocation extent overflow".into())
        })?;
        let remaining = self.bytes.checked_sub(bytes).ok_or_else(|| {
            HostAdmissionError::Unavailable("actual allocation exceeds host promise".into())
        })?;
        if bytes != 0 {
            let mut state = self.gate.lock().map_err(|_| {
                HostAdmissionError::Unavailable("resident host gate poisoned".into())
            })?;
            state.bytes = state.bytes.checked_sub(bytes).ok_or_else(|| {
                HostAdmissionError::Unavailable("resident host promise accounting mismatch".into())
            })?;
        }
        self.bytes = remaining;
        Ok(())
    }
}

impl Drop for HostReservation {
    fn drop(&mut self) {
        if self.bytes != 0
            && let Ok(mut state) = self.gate.lock()
        {
            state.bytes = state.bytes.saturating_sub(self.bytes);
        }
    }
}

#[cfg(windows)]
pub(super) fn host_headroom() -> Result<HostHeadroom, String> {
    #[repr(C)]
    #[derive(Default)]
    struct MemoryStatus {
        length: u32,
        load: u32,
        total_physical: u64,
        available_physical: u64,
        commit_limit: u64,
        available_commit: u64,
        total_virtual: u64,
        available_virtual: u64,
        available_extended_virtual: u64,
    }
    #[repr(C)]
    #[derive(Default)]
    struct PerformanceInformation {
        size: u32,
        commit_total: usize,
        commit_limit: usize,
        commit_peak: usize,
        physical_total: usize,
        physical_available: usize,
        system_cache: usize,
        kernel_total: usize,
        kernel_paged: usize,
        kernel_nonpaged: usize,
        page_size: usize,
        handles: u32,
        processes: u32,
        threads: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(status: *mut MemoryStatus) -> i32;
        fn K32GetPerformanceInfo(information: *mut PerformanceInformation, size: u32) -> i32;
    }
    let mut caller = MemoryStatus {
        length: size_of::<MemoryStatus>() as u32,
        ..MemoryStatus::default()
    };
    let mut system = PerformanceInformation {
        size: size_of::<PerformanceInformation>() as u32,
        ..PerformanceInformation::default()
    };
    // SAFETY: documented C layouts, correct sizes, valid writable stack storage;
    // these synchronous read-only queries open no handles and run off realtime.
    unsafe {
        if GlobalMemoryStatusEx(&mut caller) == 0 {
            return Err(format!(
                "GlobalMemoryStatusEx: {}",
                std::io::Error::last_os_error()
            ));
        }
        if K32GetPerformanceInfo(&mut system, size_of::<PerformanceInformation>() as u32) == 0 {
            return Err(format!(
                "GetPerformanceInfo: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    if caller.total_physical == 0
        || caller.available_physical > caller.total_physical
        || caller.commit_limit == 0
        || caller.available_commit > caller.commit_limit
        || system.page_size == 0
        || system.commit_limit == 0
        || system.commit_total > system.commit_limit
    {
        return Err("host memory query returned invalid headroom geometry".into());
    }
    let available = (system.commit_limit - system.commit_total)
        .checked_mul(system.page_size)
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or("system available commit extent overflow")?;
    Ok(HostHeadroom {
        available_physical: caller.available_physical,
        caller_available_commit: caller.available_commit,
        system_available_commit: available,
    })
}

#[cfg(not(windows))]
pub(super) fn host_headroom() -> Result<HostHeadroom, String> {
    Err("resident host headroom monitoring is unavailable on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headroom(allowance: u64) -> HostHeadroom {
        HostHeadroom {
            available_physical: allowance + PHYSICAL_RESERVE,
            caller_available_commit: allowance + COMMIT_RESERVE,
            system_available_commit: allowance + COMMIT_RESERVE,
        }
    }

    #[test]
    fn prospective_long_ranges_use_real_headroom_without_gigabyte_cutoff() {
        let mut state = Promises::default();
        let bytes = 3 * 1024 * MIB;
        state.admit(bytes, headroom(bytes)).unwrap();
        assert_eq!(state.bytes, bytes);
        assert!(state.admit(1, headroom(bytes)).is_err());
        assert_eq!(state.bytes, bytes);
    }

    #[test]
    fn each_host_headroom_channel_limits_admission_and_checked_overflow_fails() {
        for image in [
            HostHeadroom {
                available_physical: PHYSICAL_RESERVE,
                ..headroom(1024)
            },
            HostHeadroom {
                caller_available_commit: COMMIT_RESERVE,
                ..headroom(1024)
            },
            HostHeadroom {
                system_available_commit: COMMIT_RESERVE,
                ..headroom(1024)
            },
        ] {
            assert!(Promises::default().admit(1, image).is_err());
        }
        assert!(
            Promises { bytes: u64::MAX }
                .admit(1, headroom(1024))
                .is_err()
        );
    }

    #[test]
    fn shared_promises_release_only_committed_or_terminal_bytes_and_reuse_needs_no_query() {
        let gate = Arc::new(Mutex::new(Promises::default()));
        let mut first =
            HostReservation::reserve_on(gate.clone(), 600, || Ok(headroom(1000))).unwrap();
        assert!(HostReservation::reserve_on(gate.clone(), 401, || Ok(headroom(1000))).is_err());
        let reuse = HostReservation::reserve_on(gate.clone(), 0, || panic!("reuse queried host"));
        assert!(reuse.is_ok());
        first.commit(200).unwrap();
        assert_eq!(gate.lock().unwrap().bytes, 400);
        let second = HostReservation::reserve_on(gate.clone(), 600, || Ok(headroom(1000))).unwrap();
        assert_eq!(gate.lock().unwrap().bytes, 1000);
        drop(first);
        assert_eq!(gate.lock().unwrap().bytes, 600);
        drop(second);
        assert_eq!(gate.lock().unwrap().bytes, 0);
        assert!(
            HostReservation::reserve_on(gate.clone(), 1, || Err("query failed".into())).is_err()
        );
        assert_eq!(gate.lock().unwrap().bytes, 0);
    }
}
