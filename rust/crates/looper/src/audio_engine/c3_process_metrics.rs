//! Windows process counters for isolated hardware-free measurements only.
use serde_json::{Value, json};
use std::ffi::c_void;

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
