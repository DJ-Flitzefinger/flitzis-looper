//! Explicit fresh-child Windows policy and observed declared backing-file identity.
//! Neither a complete loader closure nor an attestation of relocated memory bytes.
use super::{CheckedFile, package};
use libloading::{Library, os::windows::Library as WindowsLibrary};
use std::{
    collections::HashSet,
    ffi::{OsString, c_void},
    fs::File,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

const SYSTEM32: u32 = 0x800;
const USER_DIRS: u32 = 0x400;
const DLL_LOAD_DIR: u32 = 0x100;
const FROM_ADDRESS: u32 = 4;
const MAX_MODULES: usize = 4096;
const MAX_PATH_UNITS: usize = 32768;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetDllDirectoryW(path: *const u16) -> i32;
    fn SetDefaultDllDirectories(flags: u32) -> i32;
    fn AddDllDirectory(path: *const u16) -> *mut c_void;
    fn RemoveDllDirectory(cookie: *mut c_void) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn K32EnumProcessModules(
        process: *mut c_void,
        modules: *mut usize,
        bytes: u32,
        needed: *mut u32,
    ) -> i32;
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut usize) -> i32;
    fn GetModuleFileNameW(module: usize, path: *mut u16, size: u32) -> u32;
    fn FreeLibrary(module: usize) -> i32;
    fn CompareStringOrdinal(
        left: *const u16,
        left_size: i32,
        right: *const u16,
        right_size: i32,
        ignore_case: i32,
    ) -> i32;
}

fn error() -> String {
    std::io::Error::last_os_error().to_string()
}

fn inside_package(path: &Path, directory: &Path) -> Result<bool, String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let directory: Vec<_> = directory.as_os_str().encode_wide().collect();
    for ancestor in path.ancestors() {
        let candidate: Vec<_> = ancestor.as_os_str().encode_wide().collect();
        if candidate.len() > MAX_PATH_UNITS || directory.len() > MAX_PATH_UNITS {
            return Err("loaded image directory comparison exceeds path bound".into());
        }
        // Windows ordinal case rules, not locale/Unicode lowercasing. Ancestors
        // preserve component boundaries; canonicalization resolves path aliases.
        let result = unsafe {
            CompareStringOrdinal(
                candidate.as_ptr(),
                candidate.len() as i32,
                directory.as_ptr(),
                directory.len() as i32,
                1,
            )
        };
        if result == 0 {
            return Err(error());
        }
        if result == 2 {
            return Ok(true);
        }
    }
    Ok(false)
}

struct Module(usize);
impl Module {
    fn at(address: usize) -> Result<Self, String> {
        let mut handle = 0;
        // FROM_ADDRESS increments a real reference; never use an unowned basename
        // lookup or UNCHANGED_REFCOUNT across module enumeration/identity checks.
        if unsafe { GetModuleHandleExW(FROM_ADDRESS, address as *const u16, &mut handle) } == 0 {
            return Err(error());
        }
        Ok(Self(handle))
    }
    fn path(&self) -> Result<PathBuf, String> {
        let mut path = vec![0u16; MAX_PATH_UNITS];
        let count =
            unsafe { GetModuleFileNameW(self.0, path.as_mut_ptr(), path.len() as u32) } as usize;
        if count == 0 || count >= path.len() || path[..count].contains(&0) {
            return Err("loaded module path query failed or was truncated".into());
        }
        let path = PathBuf::from(OsString::from_wide(&path[..count]));
        if !path.is_absolute() {
            return Err("loaded module path is not absolute".into());
        }
        Ok(path)
    }
}
impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}

fn modules() -> Result<Vec<Module>, String> {
    let mut handles = vec![0usize; MAX_MODULES];
    let mut needed = 0u32;
    let bytes = std::mem::size_of_val(handles.as_slice());
    if unsafe {
        K32EnumProcessModules(
            GetCurrentProcess(),
            handles.as_mut_ptr(),
            bytes as u32,
            &mut needed,
        )
    } == 0
    {
        return Err(error());
    }
    let needed = needed as usize;
    if needed == 0 || needed > bytes || !needed.is_multiple_of(std::mem::size_of::<usize>()) {
        return Err("loaded module enumeration exceeded its complete snapshot bound".into());
    }
    handles.truncate(needed / std::mem::size_of::<usize>());
    // A disappearing/query-failed module invalidates this snapshot, not a partial
    // success. Each acquired reference protects its later path/file queries.
    handles.into_iter().map(Module::at).collect()
}

struct Directory(usize);
impl Directory {
    fn set(path: &Path) -> Result<Self, String> {
        let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // The old SetDllDirectory path can affect children. Reset it explicitly;
        // this mode intentionally changes only this fresh standalone process.
        if unsafe { SetDllDirectoryW(std::ptr::null()) } == 0
            || unsafe { SetDefaultDllDirectories(SYSTEM32 | USER_DIRS) } == 0
        {
            return Err(error());
        }
        let cookie = unsafe { AddDllDirectory(wide.as_ptr()) };
        if cookie.is_null() {
            return Err(error());
        }
        Ok(Self(cookie as usize))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        unsafe {
            RemoveDllDirectory(self.0 as *mut c_void);
        }
    }
}

pub(super) struct Admission {
    // References end before file pins on pre-cache errors. Once ORT can cache a
    // library globally, runtime.rs retains this entire guard until process exit.
    modules: Vec<Module>,
    _directory: Directory,
    _pins: Vec<File>,
    image_pins: Vec<CheckedFile>,
    seen: HashSet<usize>,
    runtime_module: usize,
}

impl Admission {
    pub(super) fn load(
        package: &package::Admission,
        runtime: &super::Admission,
    ) -> Result<(Self, Library), String> {
        for module in modules()? {
            let path = module.path()?;
            if package.declares(&path) || inside_package(&path, package.directory())? {
                return Err(
                    "controlled runtime requires no preloaded declared/package images".into(),
                );
            }
        }
        let pins = package.clone_pins(runtime)?;
        let directory = Directory::set(package.directory())?;
        // Exact documented per-call flags exclude CWD/PATH/application-directory
        // search for this load's imports. Later ORT overrides remain a separate
        // boundary; snapshots below never certify a complete closure.
        let library = unsafe {
            WindowsLibrary::load_with_flags(&runtime.runtime.path, DLL_LOAD_DIR | SYSTEM32)
        }
        .map_err(|e| e.to_string())?;
        let raw = library.into_raw();
        let library: Library = unsafe { WindowsLibrary::from_raw(raw) }.into();
        let module = Module::at(raw as usize)?;
        let (pin, _) = package.check_image(&module.path()?)?;
        let mut admission = Self {
            modules: vec![module],
            _directory: directory,
            _pins: pins,
            image_pins: vec![pin],
            seen: HashSet::from([raw as usize]),
            runtime_module: raw as usize,
        };
        if let Err(error) = admission.snapshot(package) {
            // This local library was declared before admission. End its reference
            // explicitly first, while admission still owns all modules/file pins.
            drop(library);
            return Err(error);
        }
        Ok((admission, library))
    }

    pub(super) fn check_export(
        &mut self,
        address: usize,
        package: &package::Admission,
    ) -> Result<(), String> {
        let module = Module::at(address)?;
        if module.0 != self.runtime_module {
            return Err("ORT API export belongs to another module".into());
        }
        let (pin, _) = package.check_image(&module.path()?)?;
        self.modules.push(module);
        self.image_pins.push(pin);
        Ok(())
    }

    pub(super) fn snapshot(
        &mut self,
        package: &package::Admission,
    ) -> Result<serde_json::Value, String> {
        let mut observed = Vec::new();
        let mut found_runtime = false;
        for module in modules()? {
            let path = module.path()?;
            if package.declares(&path) {
                let (pin, mut receipt) = package.check_image(&path)?;
                receipt["module_base"] = format!("{:x}", module.0).into();
                found_runtime |= module.0 == self.runtime_module;
                observed.push(receipt);
                if self.seen.insert(module.0) {
                    self.modules.push(module);
                    self.image_pins.push(pin);
                }
            } else if inside_package(&path, package.directory())? {
                return Err("loaded undeclared image inside the admitted package directory".into());
            }
        }
        if !found_runtime {
            return Err("admitted runtime is absent from loaded image snapshot".into());
        }
        observed.sort_by_key(|r| r["name"].as_str().unwrap_or_default().to_ascii_lowercase());
        Ok(
            serde_json::json!({"policy":"package-system32","initial_load_flags":DLL_LOAD_DIR|SYSTEM32,
            "default_directory_flags":SYSTEM32|USER_DIRS,"inherited_setdll_directory_reset":true,
            "additional_user_directories":"NOT_ENUMERABLE","observed_declared_images":observed,
            "evidence":"module-handles-and-pinned-backing-files-not-relocated-memory",
            "loader_closure":"UNKNOWN","full_producer":"UNKNOWN",
            "lifetime":"retained-until-standalone-process-exit"}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Run separately in a disposable process with real compiled fixture DLLs.
    /// The ordinary test run must not irreversibly change its own loader policy.
    #[test]
    #[ignore = "requires isolated compiled Windows loader fixtures; no ORT/model execution"]
    fn actual_windows_loader_policy_and_image_identity() {
        let directory = PathBuf::from(std::env::var_os("FLITZIS_LOADER_FIXTURE").unwrap());
        let poison = PathBuf::from(std::env::var_os("FLITZIS_LOADER_POISON").unwrap());
        let mode = std::env::var("FLITZIS_LOADER_CASE").unwrap();
        let sha = |p: &Path| format!("{:x}", Sha256::digest(std::fs::read(p).unwrap()));
        let manifest = directory.join("runtime.json");
        let runtime = super::super::Admission::verify(&manifest, &sha(&manifest)).unwrap();
        let manifest = directory.join("package.json");
        let package = package::Admission::verify(&manifest, &sha(&manifest), &runtime).unwrap();
        let foreign = poison.join("root.dll");
        assert!(
            package.check_image(&foreign).is_err(),
            "same bytes at another file identity"
        );
        if mode == "preloaded" {
            let _foreign =
                unsafe { WindowsLibrary::load_with_flags(&foreign, DLL_LOAD_DIR | SYSTEM32) }
                    .unwrap();
            assert!(
                Admission::load(&package, &runtime)
                    .err()
                    .unwrap()
                    .contains("preloaded")
            );
            return;
        }
        // Model the inherited/earlier SetDllDirectory poison explicitly as well
        // as actual CWD/PATH poison supplied by the outer original owned runner.
        let wide: Vec<_> = poison.as_os_str().encode_wide().chain(Some(0)).collect();
        assert_ne!(unsafe { SetDllDirectoryW(wide.as_ptr()) }, 0);
        let result = Admission::load(&package, &runtime);
        if mode == "missing-initial" || mode == "undeclared" {
            assert!(result.is_err(), "unexpected package dependency resolution");
            return;
        }
        let (mut guard, library) = result.unwrap();
        let get =
            unsafe { library.get::<unsafe extern "C" fn() -> i32>(b"FixtureRoot\0") }.unwrap();
        guard.check_export(*get as usize, &package).unwrap();
        assert_eq!(
            unsafe { get() },
            42,
            "package dependency must beat the poison"
        );
        assert!(
            guard
                .check_export(GetCurrentProcess as *const () as usize, &package)
                .is_err()
        );
        let receipt = guard.snapshot(&package).unwrap();
        assert_eq!(
            receipt["observed_declared_images"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(receipt["loader_closure"], "UNKNOWN");
        for name in ["root.dll", "dependency.dll"] {
            let path = directory.join(name);
            assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
            assert!(std::fs::remove_file(&path).is_err());
        }
        if mode == "bare"
            || mode == "missing-bare"
            || mode == "altered"
            || mode == "missing-altered"
        {
            let late = if mode.ends_with("bare") {
                unsafe { WindowsLibrary::new("late.dll") }
            } else {
                unsafe { WindowsLibrary::load_with_flags(directory.join("late.dll"), 8) }
            };
            if mode.starts_with("missing-") {
                assert!(late.is_err(), "late poison dependency was accepted");
            } else {
                let late = late.unwrap();
                let get =
                    unsafe { late.get::<unsafe extern "C" fn() -> i32>(b"FixtureLate\0") }.unwrap();
                assert_eq!(unsafe { get() }, 73);
                assert_eq!(
                    guard.snapshot(&package).unwrap()["observed_declared_images"]
                        .as_array()
                        .unwrap()
                        .len(),
                    4
                );
            }
        }
        if mode == "case-undeclared" {
            let upper = PathBuf::from(directory.to_str().unwrap().to_uppercase()).join("late.dll");
            let _late = unsafe { WindowsLibrary::load_with_flags(upper, 8) }.unwrap();
            assert!(guard.snapshot(&package).unwrap_err().contains("undeclared"));
        }
        // Successful handles must end before their file pins. The production
        // ORT-cached variant instead retains the whole guard until child exit.
        drop(library);
        drop(guard);
        drop(package);
        drop(runtime);
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(directory.join("root.dll"))
                .is_ok()
        );
    }
}
