//! Recognized crash staging only. Unknown files and live/reused process owners
//! never grant deletion rights. Recovery is bounded and shares admission exclusion.
use super::*;

const OWNED_NAMES: [&str; 5] = [
    "owner.json",
    "snapshot.original",
    "decoder.f32le",
    "playback.f32le",
    "manifest.json",
];

#[cfg(windows)]
#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn GetProcessTimes(
        process: *mut std::ffi::c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn GetExitCodeProcess(process: *mut std::ffi::c_void, code: *mut u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

#[cfg(windows)]
fn process_start(handle: *mut std::ffi::c_void) -> io::Result<u64> {
    let mut creation = FileTime { low: 0, high: 0 };
    let mut exit = FileTime { low: 0, high: 0 };
    let mut kernel = FileTime { low: 0, high: 0 };
    let mut user = FileTime { low: 0, high: 0 };
    // SAFETY: valid current/opened process handle and four initialized FILETIME
    // outputs; this synchronous control path performs no realtime work.
    if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((u64::from(creation.high) << 32) | u64::from(creation.low))
}

fn current_start() -> io::Result<u64> {
    #[cfg(windows)]
    {
        // SAFETY: GetCurrentProcess returns a process-lifetime pseudo handle.
        process_start(unsafe { GetCurrentProcess() })
    }
    #[cfg(not(windows))]
    {
        Ok(0)
    }
}

fn creator_alive(pid: u32, started: u64) -> bool {
    #[cfg(windows)]
    {
        // SAFETY: query-only process handle; failure/denied query is conservative.
        let process = unsafe { OpenProcess(0x1000, 0, pid) };
        if process.is_null() {
            return io::Error::last_os_error().raw_os_error() != Some(87);
        }
        let mut code = 259_u32;
        // SAFETY: process handle is live and code points to initialized storage.
        let queried = unsafe { GetExitCodeProcess(process, &mut code) } != 0;
        let start = process_start(process);
        // SAFETY: close this independently opened handle exactly once.
        unsafe {
            CloseHandle(process);
        }
        !queried || start.is_err() || (code == 259 && start.ok() == Some(started))
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, started);
        true
    }
}

pub(super) fn record_owner(root: &Path, path: &Path) -> io::Result<()> {
    let owner = json!({"kind":"flitzi-pcm-staging-owner-v1","root":root,
        "generation":path.file_name().and_then(|v| v.to_str()),
        "pid":std::process::id(),"process_start":current_start()?});
    let bytes = serde_json::to_vec(&canonical(&owner)).map_err(io::Error::other)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.join("owner.json"))?;
    file.write_all(&bytes)?;
    file.sync_all()
}

/// Exact known leaves only, including incomplete writes. Parent and generation
/// handles pin their real ordinary directories while child deletes are performed.
pub(super) fn remove_generation(
    root: &Path,
    path: &Path,
    identity: Option<&FileIdentity>,
) -> io::Result<()> {
    reject_links(root)?;
    let root = fs::canonicalize(root)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
        Ok(_) => reject_links(path)?,
    }
    if !path.is_absolute() || fs::canonicalize(path)?.parent() != Some(root.as_path()) {
        return Err(invalid("generation rollback escaped its owned root"));
    }
    let _root_guard = crate::audio_engine::project_assets::directory_guards(&root)?;
    let generation_guard = directory_guard(path)?;
    if identity
        .is_some_and(|expected| file_identity(&generation_guard).ok().as_ref() != Some(expected))
    {
        return Err(invalid("staging generation was replaced; preserved"));
    }
    let entries = fs::read_dir(path)?
        .take(OWNED_NAMES.len() + 1)
        .collect::<io::Result<Vec<_>>>()?;
    if entries.len() > OWNED_NAMES.len()
        || entries.iter().any(|entry| {
            !entry
                .file_name()
                .to_str()
                .is_some_and(|name| OWNED_NAMES.contains(&name))
        })
    {
        return Err(invalid("unknown generation files prevent cleanup"));
    }
    for entry in &entries {
        reject_links(&entry.path())?;
        if !entry.file_type()?.is_file() {
            return Err(invalid("generation child is not ordinary file"));
        }
    }
    // Preserve the marker until the other owned leaves are gone, so a sharing
    // failure still leaves recognized recovery evidence for the next admission.
    for name in [
        "snapshot.original",
        "decoder.f32le",
        "playback.f32le",
        "manifest.json",
        "owner.json",
    ] {
        match crate::audio_engine::project_assets::remove_owned_file(&path.join(name), None) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    drop(generation_guard);
    fs::remove_dir(path)
}

pub(super) fn recover(root: &Path) -> io::Result<()> {
    let mut admission = lifecycle::store()
        .state
        .lock()
        .map_err(|_| invalid("staging recovery admission poisoned"))?;
    for entry in fs::read_dir(root)?.take(4096) {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if !name
            .to_str()
            .is_some_and(|name| name.starts_with(".staging-"))
        {
            continue;
        }
        let result = (|| -> io::Result<()> {
            reject_links(&path)?;
            let _directory = directory_guard(&path)?;
            let owner_path = path.join("owner.json");
            reject_links(&owner_path)?;
            let mut reader = sealed_reader(&owner_path)?;
            let length = reader.metadata()?.len();
            if length == 0 || length > 4096 {
                return Err(invalid("staging owner marker bound"));
            }
            let mut bytes = vec![0; length as usize];
            reader.read_exact(&mut bytes)?;
            let owner: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            if owner["kind"] != "flitzi-pcm-staging-owner-v1"
                || owner["root"] != json!(root)
                || owner["generation"] != json!(name.to_str())
            {
                return Err(invalid("unrecognized staging ownership"));
            }
            let pid = owner["pid"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| invalid("staging pid"))?;
            let start = owner["process_start"]
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or_else(|| invalid("staging start"))?;
            if creator_alive(pid, start) {
                return Ok(());
            }
            let identity = file_identity(&_directory)?;
            let [slot] = lifecycle::reserve_cleanup::<1>()?;
            drop(reader);
            drop(_directory);
            match remove_generation(root, &path, Some(&identity)) {
                Ok(()) => Ok(()),
                Err(error)
                    if error.kind() == io::ErrorKind::PermissionDenied
                        || matches!(error.raw_os_error(), Some(32 | 33)) =>
                {
                    lifecycle::queue_generation_under_gate(
                        &mut admission,
                        path.clone(),
                        root.to_owned(),
                        identity,
                        slot,
                    );
                    Ok(())
                }
                Err(error) => Err(error),
            }
        })();
        // Unknown, currently open, sharing-blocked or replaced generations remain.
        // Recovery never makes a questionable entry eligible for publication.
        if let Err(error) = result {
            if error.kind() == io::ErrorKind::WouldBlock {
                break;
            }
            continue;
        }
    }
    Ok(())
}
