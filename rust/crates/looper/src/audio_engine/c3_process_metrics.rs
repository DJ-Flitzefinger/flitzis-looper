//! Windows process counters for isolated hardware-free measurements only.
use serde_json::{Value, json};
use std::ffi::c_void;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[path = "c3_process_metrics/host_tests.rs"]
mod host_tests;

#[repr(C)]
#[derive(Default)]
struct HostCounters {
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

#[repr(C)]
#[derive(Default)]
struct JobBasicLimits {
    process_user_time: i64,
    job_user_time: i64,
    flags: u32,
    minimum_working_set: usize,
    maximum_working_set: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
#[derive(Default)]
struct JobExtendedLimits {
    basic: JobBasicLimits,
    io: [u64; 6],
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}

#[repr(C)]
struct Memory {
    size: u32,
    faults: u32,
    values: [usize; 9],
}

#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn K32GetProcessMemoryInfo(process: *mut c_void, memory: *mut Memory, size: u32) -> i32;
    fn GetProcessTimes(
        process: *mut c_void,
        created: *mut FileTime,
        exited: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn GetProcessIoCounters(process: *mut c_void, counters: *mut [u64; 6]) -> i32;
    fn GetProcessHandleCount(process: *mut c_void, handles: *mut u32) -> i32;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetModuleFileNameW(module: *mut c_void, filename: *mut u16, size: u32) -> u32;
    fn GlobalMemoryStatusEx(status: *mut HostCounters) -> i32;
    fn K32GetPerformanceInfo(information: *mut PerformanceInformation, size: u32) -> i32;
    fn IsProcessInJob(process: *mut c_void, job: *mut c_void, in_job: *mut i32) -> i32;
    fn QueryInformationJobObject(
        job: *mut c_void,
        class: i32,
        information: *mut c_void,
        size: u32,
        returned: *mut u32,
    ) -> i32;
}

fn query_succeeded(api: &str, result: i32) -> Result<(), String> {
    if result == 0 {
        Err(format!("{api}: {}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

fn commit_bytes(total: usize, limit: usize, page_size: usize) -> Result<(u64, u64, u64), String> {
    if page_size == 0 || total > limit {
        return Err("GetPerformanceInfo returned invalid commit geometry".into());
    }
    let bytes = |pages: usize| {
        pages
            .checked_mul(page_size)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| "GetPerformanceInfo commit byte extent overflow".to_string())
    };
    Ok((bytes(total)?, bytes(limit)?, bytes(limit - total)?))
}

fn host_snapshot_with(
    query: impl FnOnce(&mut HostCounters, &mut PerformanceInformation) -> Result<(), String>,
) -> Result<Value, String> {
    let started = Instant::now();
    let observed_at_unix_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("host observation clock: {error}"))?
        .as_nanos();
    let mut caller = HostCounters {
        length: size_of::<HostCounters>() as u32,
        ..HostCounters::default()
    };
    let mut system = PerformanceInformation {
        size: size_of::<PerformanceInformation>() as u32,
        ..PerformanceInformation::default()
    };
    query(&mut caller, &mut system)?;
    let (total, limit, available) =
        commit_bytes(system.commit_total, system.commit_limit, system.page_size)?;
    Ok(json!({
        "observed_at_unix_ns":u64::try_from(observed_at_unix_ns).map_err(|_| "host timestamp overflow")?,
        "observation_span_ns":u64::try_from(started.elapsed().as_nanos()).map_err(|_| "host span overflow")?,
        "total_physical":caller.total_physical,"available_physical":caller.available_physical,
        "commit_limit":caller.commit_limit,"available_commit":caller.available_commit,
        "system_commit_total_bytes":total,"system_commit_limit_bytes":limit,
        "system_available_commit_bytes":available,
        "scope":"sequential read-only queries of this caller and system; no atomic cross-API snapshot",
        "caller_commit_scope":"GlobalMemoryStatusEx: caller committable headroom and smaller caller/system limit",
        "system_commit_scope":"GetPerformanceInfo: (CommitLimit - CommitTotal) * PageSize"
    }))
}

/// Observe caller headroom and true system commit without changing process counters.
pub(super) fn host_snapshot() -> Result<Value, String> {
    host_snapshot_with(|caller, system| {
        // SAFETY: both structures have their documented size and valid writable
        // storage. These synchronous queries open no process or Job handles.
        unsafe {
            query_succeeded("GlobalMemoryStatusEx", GlobalMemoryStatusEx(caller))?;
            query_succeeded(
                "K32GetPerformanceInfo",
                K32GetPerformanceInfo(system, size_of::<PerformanceInformation>() as u32),
            )
        }
    })
}

fn current_job_context_with(
    membership: impl FnOnce() -> Result<bool, String>,
    limits: impl FnOnce() -> Result<JobExtendedLimits, String>,
) -> Value {
    let scope = "this calling process and current Job only; enclosing Job restrictions are not enumerated; raw limits are not admission authority";
    let in_job = match membership() {
        Ok(value) => value,
        Err(error) => return json!({"in_job":null,"status":"unknown","error":error,"scope":scope}),
    };
    if !in_job {
        return json!({"in_job":false,"status":"not_in_job","error":null,"scope":scope});
    }
    match limits() {
        Ok(value) => json!({"in_job":true,"status":"queried","error":null,
            "limit_flags":value.basic.flags,"process_memory_limit_bytes":value.process_memory_limit,
            "job_memory_limit_bytes":value.job_memory_limit,
            "active_process_limit":value.basic.active_process_limit,
            "minimum_working_set_bytes":value.basic.minimum_working_set,
            "maximum_working_set_bytes":value.basic.maximum_working_set,"scope":scope}),
        Err(error) => json!({"in_job":true,"status":"limits_unknown","error":error,"scope":scope}),
    }
}

/// Baseline context only: use pseudo/NULL handles, never enumerate foreign Jobs.
pub(super) fn current_job_context() -> Value {
    current_job_context_with(
        || {
            let mut in_job = 0;
            // SAFETY: the pseudo-handle names this caller; NULL asks membership
            // in any Job. The output BOOL lives through the synchronous call.
            unsafe {
                query_succeeded(
                    "IsProcessInJob(current)",
                    IsProcessInJob(-1_isize as *mut c_void, std::ptr::null_mut(), &mut in_job),
                )?;
            }
            Ok(in_job != 0)
        },
        || {
            let mut limits = JobExtendedLimits::default();
            // SAFETY: NULL observes the current Job only; class 9 is documented
            // extended limit information. No handle is created or retained.
            unsafe {
                query_succeeded(
                    "QueryInformationJobObject(current)",
                    QueryInformationJobObject(
                        std::ptr::null_mut(),
                        9,
                        (&mut limits as *mut JobExtendedLimits).cast(),
                        size_of::<JobExtendedLimits>() as u32,
                        std::ptr::null_mut(),
                    ),
                )?;
            }
            Ok(limits)
        },
    )
}

/// Bind only the running executable and the two already loaded probe runtimes.
/// Call after Python initialization and native-pool construction, while both
/// runtimes remain loaded. This performs no directory or module discovery.
#[cfg(all(test, windows))]
pub(super) fn runtime_identity() -> Result<Value, String> {
    use sha2::{Digest, Sha256};
    use std::fs::File;
    use std::io::Read;
    use std::os::windows::fs::MetadataExt;
    use std::path::PathBuf;

    fn artifact(label: &str, module_name: Option<&str>) -> Result<Value, String> {
        let module = if let Some(name) = module_name {
            let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            // SAFETY: the fixed runtime name is NUL-terminated and lives through
            // the call. The caller keeps this already loaded runtime alive.
            let module = unsafe { GetModuleHandleW(name.as_ptr()) };
            if module.is_null() {
                return Err(format!(
                    "{label}: runtime is not loaded: {}",
                    std::io::Error::last_os_error()
                ));
            }
            module
        } else {
            std::ptr::null_mut()
        };
        // One bounded buffer covers the Windows extended-length UTF-16 path.
        let mut filename = vec![0_u16; 32_768];
        // SAFETY: module is a currently loaded module (or NULL for the running
        // executable), and filename exposes exactly size writable UTF-16 units.
        let length =
            unsafe { GetModuleFileNameW(module, filename.as_mut_ptr(), filename.len() as u32) };
        if length == 0 {
            return Err(format!(
                "{label}: module path query failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if length as usize >= filename.len() {
            return Err(format!("{label}: module path exceeds the bounded buffer"));
        }
        let filename = String::from_utf16(&filename[..length as usize])
            .map_err(|error| format!("{label}: module path is invalid UTF-16: {error}"))?;
        let path = PathBuf::from(filename);
        if !path.is_absolute() {
            return Err(format!("{label}: module path is not absolute"));
        }
        // Read only the exact path returned for this loaded tooling artifact.
        let mut file = File::open(&path)
            .map_err(|error| format!("{label}: cannot read loaded artifact: {error}"))?;
        let before = file
            .metadata()
            .map_err(|error| format!("{label}: cannot inspect loaded artifact: {error}"))?;
        if !before.is_file() {
            return Err(format!("{label}: loaded artifact path is not a file"));
        }
        let mut buffer = [0_u8; 65_536];
        let mut digest = Sha256::new();
        let mut bytes = 0_u64;
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|error| format!("{label}: loaded artifact read failed: {error}"))?;
            if count == 0 {
                break;
            }
            bytes = bytes
                .checked_add(count as u64)
                .ok_or_else(|| format!("{label}: artifact byte count overflow"))?;
            digest.update(&buffer[..count]);
        }
        let after = file
            .metadata()
            .map_err(|error| format!("{label}: final artifact metadata failed: {error}"))?;
        if bytes != before.len()
            || before.len() != after.len()
            || before.last_write_time() != after.last_write_time()
        {
            return Err(format!("{label}: loaded artifact changed during hashing"));
        }
        Ok(json!({"path":path,"bytes":bytes,"sha256":format!("{:x}",digest.finalize())}))
    }

    let before = snapshot();
    let executable = artifact("native executable", None)?;
    let python = artifact("Python runtime", Some("python314.dll"))?;
    let rubberband = artifact("RubberBand runtime", Some("rubberband-3.dll"))?;
    let after = snapshot();
    for key in ["process_id", "process_creation_ticks"] {
        if before[key] != after[key] {
            return Err("process identity changed during runtime binding".into());
        }
    }
    Ok(json!({"process_id":before["process_id"],
        "process_creation_ticks":before["process_creation_ticks"],
        "native_executable":executable,"python314_dll":python,"rubberband_dll":rubberband,
        "scope":"normal-runtime-only reads of exact current executable and already loaded Python/RubberBand paths; no filesystem discovery; paths may be external"}))
}

pub(super) fn snapshot() -> Value {
    let process = -1_isize as *mut c_void;
    let size = size_of::<Memory>() as u32;
    let mut memory = Memory {
        size,
        faults: 0,
        values: [0; 9],
    };
    let (mut created, mut exited, mut kernel, mut user) = (
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
    );
    let mut io = [0_u64; 6];
    let mut handles = 0;
    // SAFETY: this process pseudo-handle and all correctly sized writable output
    // structures remain valid throughout these synchronous Windows calls.
    unsafe {
        assert_ne!(K32GetProcessMemoryInfo(process, &mut memory, size), 0);
        assert_ne!(
            GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user),
            0
        );
        assert_ne!(GetProcessIoCounters(process, &mut io), 0);
        assert_ne!(GetProcessHandleCount(process, &mut handles), 0);
    }
    let ticks = |time: FileTime| (u64::from(time.high) << 32) | u64::from(time.low);
    // SAFETY: this Windows query takes no pointers and returns the calling PID.
    let process_id = unsafe { GetCurrentProcessId() };
    json!({"process_id":process_id,"process_creation_ticks":ticks(created),
        "working_set_bytes":memory.values[1], "peak_working_set_bytes":memory.values[0],
        "private_committed_bytes":memory.values[8], "peak_commit_charge_bytes":memory.values[7],
        "page_faults":memory.faults, "kernel_cpu_ns":ticks(kernel)*100,
        "user_cpu_ns":ticks(user)*100, "handle_count":handles,
        "read_operations":io[0], "write_operations":io[1], "other_operations":io[2],
        "read_transfer_bytes":io[3], "write_transfer_bytes":io[4], "other_transfer_bytes":io[5]})
}

pub(super) fn delta(before: &Value, after: &Value) -> Value {
    let mut result = serde_json::Map::new();
    for (key, value) in after.as_object().unwrap() {
        if matches!(key.as_str(), "process_id" | "process_creation_ticks") {
            assert_eq!(value, &before[key]);
            continue;
        }
        result.insert(
            key.clone(),
            json!(i128::from(value.as_u64().unwrap()) - i128::from(before[key].as_u64().unwrap())),
        );
    }
    Value::Object(result)
}
