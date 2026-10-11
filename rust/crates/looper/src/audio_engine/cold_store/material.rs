//! Full-content claim precedes the derivative preparation gate. Names are locators only.
use super::*;
use crate::audio_engine::material_paths::{self, MaterialBinding};

#[cfg(test)]
std::thread_local! {
    static CONTENTION_OBSERVER: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
    static SELECTION_OBSERVER: std::cell::RefCell<Option<Box<dyn FnOnce(&Path)>>> = std::cell::RefCell::new(None);
    static BIRTH_OBSERVER: std::cell::RefCell<Option<Box<dyn FnOnce(&Path)>>> = std::cell::RefCell::new(None);
}

#[cfg(test)]
pub(super) fn observe_contention_for_test(observer: impl FnOnce() + 'static) {
    CONTENTION_OBSERVER.with(|current| {
        assert!(current.borrow_mut().replace(Box::new(observer)).is_none());
    });
}

#[cfg(test)]
pub(super) fn observe_selection_for_test(observer: impl FnOnce(&Path) + 'static) {
    SELECTION_OBSERVER.with(|current| {
        assert!(current.borrow_mut().replace(Box::new(observer)).is_none());
    });
}

#[cfg(test)]
pub(super) fn observe_birth_for_test(observer: impl FnOnce(&Path) + 'static) {
    BIRTH_OBSERVER.with(|current| {
        assert!(current.borrow_mut().replace(Box::new(observer)).is_none());
    });
}

/// Create the directory and obtain its rename-excluding handle in one OS call.
/// A mkdir followed by pathname-open cannot prove which directory was created.
fn create_directory_guard(parent: &File, leaf: &str) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        #[repr(C)]
        struct UnicodeString {
            length: u16,
            maximum_length: u16,
            buffer: *mut u16,
        }
        #[repr(C)]
        struct ObjectAttributes {
            length: u32,
            root_directory: *mut std::ffi::c_void,
            object_name: *mut UnicodeString,
            attributes: u32,
            security_descriptor: *mut std::ffi::c_void,
            security_quality: *mut std::ffi::c_void,
        }
        #[repr(C)]
        struct IoStatusBlock {
            status: usize,
            information: usize,
        }
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn NtCreateFile(
                handle: *mut *mut std::ffi::c_void,
                access: u32,
                attributes: *mut ObjectAttributes,
                status: *mut IoStatusBlock,
                allocation: *mut i64,
                file_attributes: u32,
                share: u32,
                disposition: u32,
                options: u32,
                ea: *mut std::ffi::c_void,
                ea_length: u32,
            ) -> i32;
            fn RtlNtStatusToDosError(status: i32) -> u32;
        }
        let mut encoded: Vec<u16> = leaf.encode_utf16().collect();
        let length = u16::try_from(
            encoded
                .len()
                .checked_mul(2)
                .ok_or_else(|| invalid("material name extent"))?,
        )
        .map_err(|_| invalid("material name exceeds Windows extent"))?;
        let mut name = UnicodeString {
            length,
            maximum_length: length,
            buffer: encoded.as_mut_ptr(),
        };
        let mut attributes = ObjectAttributes {
            length: std::mem::size_of::<ObjectAttributes>() as u32,
            root_directory: parent.as_raw_handle(),
            object_name: &mut name,
            attributes: 0x40, // OBJ_CASE_INSENSITIVE; name is one validated child.
            security_descriptor: std::ptr::null_mut(),
            security_quality: std::ptr::null_mut(),
        };
        let mut status = IoStatusBlock {
            status: 0,
            information: 0,
        };
        let mut handle = std::ptr::null_mut();
        // SAFETY: repr(C) NT structures, the UTF-16 child buffer, and the actual
        // parent handle remain live throughout synchronous NtCreateFile. FILE_CREATE
        // never opens/replaces occupancy. DIRECTORY_FILE + OPEN_REPARSE_POINT,
        // FILE_SHARE_READ (no WRITE/DELETE) seals the actual newborn directory.
        let result = unsafe {
            NtCreateFile(
                &mut handle,
                0x0010_00a1,
                &mut attributes,
                &mut status,
                std::ptr::null_mut(),
                0x80,
                1,
                2,
                0x0020_0021,
                std::ptr::null_mut(),
                0,
            )
        };
        if result < 0 {
            // SAFETY: pure NTSTATUS-to-Win32 conversion has no pointer arguments.
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(result) } as i32,
            ));
        }
        if handle.is_null() {
            return Err(invalid("material creation returned no handle"));
        }
        // SAFETY: a successful NtCreateFile returned one owned process handle.
        let file = unsafe { File::from_raw_handle(handle) };
        let metadata = file.metadata()?;
        if status.information != 2 || !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0
        {
            return Err(invalid(
                "material creation did not create an ordinary directory",
            ));
        }
        Ok(file)
    }
    #[cfg(not(windows))]
    {
        let _ = (parent, leaf);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "material birth requires a tested handle creation protocol",
        ))
    }
}

fn selected(selection: MaterialSelection) -> MaterialSelection {
    #[cfg(test)]
    SELECTION_OBSERVER.with(|current| {
        if let Some(observer) = current.borrow_mut().take() {
            observer(&selection.root);
        }
    });
    selection
}

pub(super) fn immutable_root_guard(root: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let guard = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(root)?;
        let metadata = guard.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid("canonical root is not an ordinary directory"));
        }
        Ok(guard)
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "material root custody requires tested sharing",
        ))
    }
}

pub(super) struct MaterialSelection {
    pub root: PathBuf,
    pub binding: MaterialBinding,
    pub original: Option<File>,
    pub binding_reader: Option<File>,
    pub claim: Option<File>,
    pub claim_directory: Option<File>,
    pub directories: Vec<File>,
    pub creation: Option<lifecycle::MaterialCreation>,
}

impl Drop for MaterialSelection {
    fn drop(&mut self) {
        self.original.take();
        self.binding_reader.take();
        self.directories.clear();
        if let Some(creation) = self.creation.take() {
            let path = creation.root.join(&creation.original_leaf);
            lifecycle::retain_material_creation(creation);
            super::super::project_assets::ProjectAssets::shared().prune_material_if_unowned(&path);
        }
    }
}

fn content_claim(
    samples: &Path,
    digest: &str,
    cancelled: &impl Fn() -> bool,
) -> io::Result<(File, File)> {
    let _scaffolding = guarded_child(samples, ".material-claims")?;
    let guard = immutable_root_guard(&samples.join(".material-claims"))?;
    // At most 256 durable empty locks. Prefix collisions only serialize cold
    // decisions; the complete SHA remains the binding and verification identity.
    let path = samples
        .join(".material-claims")
        .join(format!("{}.lock", &digest[..2]));
    reject_existing_links(&path)?;
    loop {
        check_cancelled(cancelled)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0).custom_flags(0x0020_0000);
        }
        match options.open(&path) {
            Ok(file) => {
                reject_links(&path)?;
                if !file.metadata()?.is_file() || file.metadata()?.len() != 0 {
                    return Err(invalid("material claim is occupied by unknown data"));
                }
                return Ok((file, guard));
            }
            Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => {
                #[cfg(test)]
                CONTENTION_OBSERVER.with(|current| {
                    if let Some(observer) = current.borrow_mut().take() {
                        observer();
                    }
                });
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn select(
    samples: &Path,
    source: &Path,
    digest: &str,
    bytes: u64,
    cancelled: &impl Fn() -> bool,
    material_slot: lifecycle::CleanupSlot,
) -> io::Result<MaterialSelection> {
    let leaf = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("source name is not UTF-8"))?;
    if !material_paths::filename(leaf) {
        return Err(invalid("unsafe original leaf"));
    }
    let (claim, claim_directory) = content_claim(samples, digest, cancelled)?;
    let samples_guard = directory_guard(samples)?;
    // Complete binding scan is bounded by the namespace capacity; no partial scan may claim a miss.
    let mut inspected = 0usize;
    for entry in fs::read_dir(samples)? {
        check_cancelled(cancelled)?;
        let entry = entry?;
        inspected += 1;
        if inspected > 4096 {
            return Err(invalid("material namespace inventory exceeds bound"));
        }
        let root = entry.path();
        // Keep the actual canonical ancestors/root sealed before reading its
        // binding, through full original verification and the caller's transfer.
        let Ok(mut directories) = super::super::project_assets::directory_guards(&root) else {
            continue;
        };
        let Ok(root_guard) = immutable_root_guard(&root) else {
            continue;
        };
        directories.push(root_guard);
        let Ok((binding, binding_reader)) = material_paths::read_binding_held(&root) else {
            continue;
        };
        if binding.sha256 != digest || binding.bytes != bytes {
            continue;
        }
        let original_path = root.join(&binding.original_leaf);
        let verified = (|| -> io::Result<File> {
            let resolved = material_paths::resolve(samples, &original_path)?;
            if resolved.path != fs::canonicalize(&original_path)? {
                return Err(invalid("canonical source path changed"));
            }
            let mut reader = sealed_reader(&original_path)?;
            verify_file(&mut reader, digest, bytes, cancelled)?;
            Ok(reader)
        })();
        match verified {
            Ok(original) => {
                return Ok(selected(MaterialSelection {
                    root: fs::canonicalize(root)?,
                    binding,
                    original: Some(original),
                    binding_reader: Some(binding_reader),
                    claim: Some(claim),
                    claim_directory: Some(claim_directory),
                    directories,
                    creation: None,
                }));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
            Err(_) => {} // Corrupt/unknown old roots survive; they cannot bind incoming content.
        }
    }
    let base = material_paths::readable_name(leaf);
    let binding = MaterialBinding {
        schema_version: 1,
        sha256: digest.to_owned(),
        bytes,
        original_leaf: leaf.to_owned(),
    };
    for suffix in [0, 12, 16, 24, 32, 48, 64] {
        check_cancelled(cancelled)?;
        let name = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}~{}", &digest[..suffix])
        };
        let root = samples.join(&name);
        reject_existing_links(&root)?;
        match create_directory_guard(&samples_guard, &name) {
            Ok(guard) => {
                #[cfg(test)]
                BIRTH_OBSERVER.with(|current| {
                    if let Some(observer) = current.borrow_mut().take() {
                        observer(&root);
                    }
                });
                let identity = file_identity(&guard)?;
                let mut directories = vec![guard, samples_guard];
                directories.extend(super::super::project_assets::directory_guards(samples)?);
                let root = fs::canonicalize(root)?;
                // Receipt identity comes from the actual creation handle, which
                // has never allowed pathname replacement since directory birth.
                return Ok(selected(MaterialSelection {
                    root: root.clone(),
                    binding,
                    original: None,
                    binding_reader: None,
                    claim: Some(claim),
                    claim_directory: Some(claim_directory),
                    directories,
                    creation: Some(lifecycle::MaterialCreation {
                        root,
                        original_leaf: leaf.to_owned(),
                        identity,
                        binding: None,
                        pending: false,
                        protection: None,
                        _slot: material_slot,
                    }),
                }));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "readable full-SHA namespace collision",
    ))
}

pub(super) fn publish_binding(
    root: &Path,
    binding: &MaterialBinding,
) -> io::Result<(File, crate::audio_engine::project_assets::VerifiedFile)> {
    binding.validate()?;
    let path = root.join(".material.json");
    let bytes = serde_json::to_vec(binding).map_err(io::Error::other)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let mut writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let identity = file_identity(&writer)?;
    let result = writer.write_all(&bytes).and_then(|()| writer.sync_all());
    drop(writer);
    if let Err(error) = result {
        let _ = super::super::project_assets::remove_owned_file(&path, Some(&identity));
        return Err(error);
    }
    let (reopened, mut reader) = material_paths::read_binding_held(root)?;
    if reopened.sha256 != binding.sha256
        || reopened.bytes != binding.bytes
        || reopened.original_leaf != binding.original_leaf
        || file_identity(&reader)? != identity
    {
        return Err(invalid("published readable binding changed"));
    }
    verify_file(&mut reader, &sha256, bytes.len() as u64, &|| false)?;
    Ok((
        reader,
        crate::audio_engine::project_assets::VerifiedFile {
            name: ".material.json".to_owned(),
            identity,
            bytes: bytes.len() as u64,
            sha256,
        },
    ))
}

pub(in crate::audio_engine) fn material_creation_receipt(
    samples: &Path,
    original: &Path,
) -> io::Result<Option<MaterialCreationReceipt>> {
    let checked = material_paths::resolve(samples, original)?;
    let Some(root) = material_paths::material_root(&checked.path) else {
        return Ok(None);
    };
    let Some((identity, Some(binding), original_leaf)) = lifecycle::known_material_creation(root)?
    else {
        return Ok(None);
    };
    if checked.path.file_name().and_then(|name| name.to_str()) != Some(original_leaf.as_str()) {
        return Err(invalid("creation receipt names another original"));
    }
    let samples = fs::canonicalize(samples)?;
    if root.parent() != Some(samples.as_path()) {
        return Ok(None);
    }
    let owner = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("material root name"))?;
    Ok(Some(MaterialCreationReceipt {
        schema_version: 1,
        root_reference: format!("samples/{owner}"),
        root_identity: identity,
        binding,
    }))
}

/// Read-only historical verification. Held returned handles grant no cleanup
/// rights; the native recovery caller must supply its current journal/ACK gate.
pub(in crate::audio_engine) fn verify_material_creation(
    samples: &Path,
    original: &Path,
    creation: &MaterialCreationReceipt,
    original_file: &crate::audio_engine::project_assets::VerifiedFile,
) -> io::Result<Vec<File>> {
    verify_material_creation_state(samples, original, creation, original_file, false)
}

/// Recovery-only typed path derivation. An absent binding is never accepted by
/// ordinary resolution or warm loading; the caller must separately hold the
/// exact created-target/original-terminal journal and current Inventory gates.
pub(in crate::audio_engine) fn retired_material_original_path(
    samples: &Path,
    original: &Path,
    creation: &MaterialCreationReceipt,
    original_file: &crate::audio_engine::project_assets::VerifiedFile,
) -> io::Result<PathBuf> {
    let (samples, path) = super::super::project_assets::owned_path(samples, original)?;
    let reference = Path::new("samples").join(
        path.strip_prefix(&samples)
            .map_err(|_| invalid("retired material escaped samples"))?,
    );
    if !matches!(
        material_paths::classify(&reference)?,
        material_paths::AssetKind::Original { material: Some(_) }
    ) {
        return Err(invalid(
            "retired creation requires a typed readable original",
        ));
    }
    let root = path
        .parent()
        .ok_or_else(|| invalid("retired creation root missing"))?;
    let owner = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("retired creation root name"))?;
    if root.parent() != Some(samples.as_path())
        || creation.root_reference != format!("samples/{owner}")
        || !material_paths::filename(&original_file.name)
        || path != root.join(&original_file.name)
    {
        return Err(invalid(
            "retired material birth/reference/leaf changed; preserved",
        ));
    }
    Ok(path)
}

fn verify_material_creation_state(
    samples: &Path,
    original: &Path,
    creation: &MaterialCreationReceipt,
    original_file: &crate::audio_engine::project_assets::VerifiedFile,
    original_retired: bool,
) -> io::Result<Vec<File>> {
    if creation.schema_version != 1
        || creation.binding.name != ".material.json"
        || creation.binding.bytes == 0
        || creation.binding.bytes > 8192
        || creation.binding.sha256.len() != 64
        || !creation
            .binding
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid("invalid historical material creation receipt"));
    }
    let samples = fs::canonicalize(samples)?;
    let path = if original_retired {
        retired_material_original_path(&samples, original, creation, original_file)?
    } else {
        let checked = material_paths::resolve(&samples, original)?;
        if !matches!(
            checked.kind,
            material_paths::AssetKind::Original { material: Some(_) }
        ) {
            return Err(invalid("creation receipt requires canonical original"));
        }
        checked.path
    };
    let root =
        material_paths::material_root(&path).ok_or_else(|| invalid("creation root missing"))?;
    let owner = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("creation root name"))?;
    if root.parent() != Some(samples.as_path())
        || creation.root_reference != format!("samples/{owner}")
    {
        return Err(invalid("historical material root reference changed"));
    }
    let mut guards = super::super::project_assets::directory_guards(root)?;
    guards.push(immutable_root_guard(root)?);
    if file_identity(
        guards
            .first()
            .ok_or_else(|| invalid("creation root guard missing"))?,
    )? != creation.root_identity
    {
        return Err(invalid(
            "historical material root FileID changed; preserved",
        ));
    }
    let (binding, mut reader) = match material_paths::read_binding_held(root) {
        Ok(held) => held,
        Err(error) if original_retired && error.kind() == io::ErrorKind::NotFound => {
            if path.try_exists()? {
                return Err(invalid("retired original path is occupied; preserved"));
            }
            if fs::read_dir(root)?.next().transpose()?.is_some() {
                return Err(invalid(
                    "absent marker requires exact empty material root; preserved",
                ));
            }
            // Both leaves may have been physically retired before process exit.
            // Exact birth FileID plus truly empty root and durable original
            // terminal scope authorize only this same empty directory.
            return Ok(guards);
        }
        Err(error) => return Err(error),
    };
    if file_identity(&reader)? != creation.binding.identity {
        return Err(invalid(
            "historical material marker FileID changed; preserved",
        ));
    }
    verify_file(
        &mut reader,
        &creation.binding.sha256,
        creation.binding.bytes,
        &|| false,
    )?;
    if original_file.name != binding.original_leaf
        || original_file.sha256 != binding.sha256
        || original_file.bytes != binding.bytes
        || path != root.join(&binding.original_leaf)
    {
        return Err(invalid(
            "historical material marker differs from complete original receipt",
        ));
    }
    if original_retired {
        if path.try_exists()? {
            return Err(invalid("retired original path is occupied; preserved"));
        }
    } else {
        let mut source = sealed_reader(&path)?;
        if file_identity(&source)? != original_file.identity {
            return Err(invalid(
                "historical material original FileID changed; preserved",
            ));
        }
        verify_file(
            &mut source,
            &original_file.sha256,
            original_file.bytes,
            &|| false,
        )?;
        guards.push(source);
    }
    guards.push(reader);
    Ok(guards)
}

/// Only the recovery caller's exact durable original-terminal ledger, current
/// config image and held inventory may authorize this absent-leaf path.
pub(in crate::audio_engine) fn restore_material_creation_after_original_retired(
    samples: &Path,
    original: &Path,
    creation: &MaterialCreationReceipt,
    original_file: &crate::audio_engine::project_assets::VerifiedFile,
    protection: Arc<crate::audio_engine::project_assets::RetirementProtection>,
) -> io::Result<Arc<crate::audio_engine::project_assets::RetirementOutcome>> {
    let [slot] = lifecycle::reserve_cleanup::<1>()?;
    let _guards = verify_material_creation_state(samples, original, creation, original_file, true)?;
    let path = retired_material_original_path(samples, original, creation, original_file)?;
    let root = material_paths::material_root(&path)
        .ok_or_else(|| invalid("retired creation root missing"))?;
    let protection = Arc::new(crate::audio_engine::project_assets::RetirementProtection {
        _inventory: protection._inventory.clone(),
        outcome: Arc::new(crate::audio_engine::project_assets::RetirementOutcome::default()),
    });
    lifecycle::restore_verified_material_creation(lifecycle::MaterialCreation {
        root: root.to_owned(),
        original_leaf: original_file.name.clone(),
        identity: creation.root_identity.clone(),
        binding: Some(creation.binding.clone()),
        pending: false,
        protection: Some(protection),
        _slot: slot,
    })?
    .ok_or_else(|| invalid("restored root retirement outcome missing"))
}

pub(in crate::audio_engine) struct MaterialRetirementAdmission {
    root: PathBuf,
    outcome: Arc<crate::audio_engine::project_assets::RetirementOutcome>,
    own_protection: bool,
    committed: bool,
    _guards: Vec<File>,
}

impl MaterialRetirementAdmission {
    pub(in crate::audio_engine) fn commit(
        mut self,
    ) -> io::Result<Arc<crate::audio_engine::project_assets::RetirementOutcome>> {
        if self.own_protection {
            lifecycle::finish_retained_material_retirement(&self.root, &self.outcome, true)?;
        }
        self.committed = true;
        Ok(self.outcome.clone())
    }
}

impl Drop for MaterialRetirementAdmission {
    fn drop(&mut self) {
        if self.own_protection && !self.committed {
            let _ =
                lifecycle::finish_retained_material_retirement(&self.root, &self.outcome, false);
        }
    }
}

pub(in crate::audio_engine) fn prepare_retained_material_retirement(
    samples: &Path,
    original: &Path,
    creation: &MaterialCreationReceipt,
    original_file: &crate::audio_engine::project_assets::VerifiedFile,
    protection: Arc<crate::audio_engine::project_assets::RetirementProtection>,
) -> io::Result<MaterialRetirementAdmission> {
    let guards = verify_material_creation(samples, original, creation, original_file)?;
    let checked = material_paths::resolve(samples, original)?;
    let root = material_paths::material_root(&checked.path)
        .ok_or_else(|| invalid("retained creation root missing"))?;
    let protection = Arc::new(crate::audio_engine::project_assets::RetirementProtection {
        _inventory: protection._inventory.clone(),
        outcome: Arc::new(crate::audio_engine::project_assets::RetirementOutcome::default()),
    });
    let (outcome, own_protection) = lifecycle::prepare_retained_material_retirement(
        root,
        &creation.root_identity,
        &creation.binding,
        &original_file.name,
        protection,
    )?;
    Ok(MaterialRetirementAdmission {
        root: root.to_owned(),
        outcome,
        own_protection,
        committed: false,
        _guards: guards,
    })
}

#[cfg(all(test, windows))]
mod retirement_admission_tests {
    use super::*;
    use crate::audio_engine::{
        material_migration,
        material_migration_recovery::{MigrationArtifactLease, MigrationProjectGuard},
        project_assets::ProjectAssets,
    };
    use sha2::{Digest, Sha256};

    const CAPACITY_CHILD: &str = "FLITZI_MATERIAL_RETIREMENT_CAPACITY_CHILD";
    const CAPACITY_TEST: &str = "audio_engine::cold_store::material::retirement_admission_tests::full_cleanup_pool_reuses_birth_slot_and_protected_original_disarms_root_inventory";

    fn run_capacity_child() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let scratch = workspace.join("scratch/goal-readable-material-layout-20261011");
        fs::create_dir_all(&scratch).unwrap();
        let fixture = tempfile::tempdir_in(&scratch).unwrap();
        let output_path = fixture.path().join("capacity-child-output.txt");
        let output = fs::File::create(&output_path).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CAPACITY_TEST, "--nocapture"])
            .env(CAPACITY_CHILD, fixture.path())
            .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
            .stderr(std::process::Stdio::from(output))
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                let kept = fixture.keep();
                panic!(
                    "capacity child timed out; retained {}: {}",
                    kept.display(),
                    fs::read_to_string(output_path).unwrap()
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let output = fs::read_to_string(&output_path).unwrap();
        let kept = fixture.keep();
        assert!(
            status.success() && output.contains("1 passed"),
            "capacity child failed; retained {}: {output}",
            kept.display()
        );
        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(kept.join("capacity-child-verified.json")).unwrap())
                .unwrap();
        assert_eq!(receipt["process_id"], u64::from(child.id()));
        assert_ne!(child.id(), std::process::id());
        assert_eq!(receipt["full_cleanup_capacity"], 4096);
        assert_eq!(receipt["protected_original_status"], "idle");
        assert_eq!(receipt["admitted_original_status_at_capacity"], "pending");
        assert_eq!(receipt["aborted_root_inventory_reacquired"], true);
        fs::write(kept.join("capacity-execution-evidence.json"), serde_json::to_vec_pretty(&serde_json::json!({"schema":"material-retirement-capacity-fresh-process-v1", "parent_process_id":std::process::id(), "fixture_directory":kept, "child":receipt, "scope":"fresh exact native test process isolates only process-global pool exhaustion; all4096 reservation and protected/admitted source custody assertions are actual"})).unwrap()).unwrap();
    }

    #[test]
    fn full_cleanup_pool_reuses_birth_slot_and_protected_original_disarms_root_inventory() {
        let Some(fixture) = std::env::var_os(CAPACITY_CHILD).map(PathBuf::from) else {
            run_capacity_child();
            return;
        };
        pyo3::Python::initialize();
        let samples = fixture.join("samples");
        fs::create_dir(&samples).unwrap();
        let old = samples.join("old.wav");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&44_u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&96_000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&8_u32.to_le_bytes());
        for value in [1_i16, 2, 3, 4] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        fs::write(&old, bytes).unwrap();
        let material =
            material_migration::prepare_material(&samples, &old, 48_000, 2, &|| false).unwrap();
        let original = material.lease.original_path.clone();
        let protected = fs::read(&original).unwrap();
        let creation = material.lease.material_creation_receipt().unwrap();
        let config = fixture.join("project.config.json");
        fs::write(&config, b"{}").unwrap();
        pyo3::Python::attach(|py| {
            let mut guard = MigrationProjectGuard::new(
                py,
                samples.to_string_lossy().into(),
                config.to_string_lossy().into(),
            )
            .unwrap();
            let mut inventory = guard.lock_inventory(py).unwrap();
            let mut artifact = MigrationArtifactLease::capture(
                py,
                samples.to_string_lossy().into(),
                original.to_string_lossy().into(),
            )
            .unwrap();
            let original_receipt: serde_json::Value =
                serde_json::from_str(&artifact.receipt_json().unwrap()).unwrap();
            let mut owner = ProjectAssets::shared()
                .acquire(&samples, &original)
                .unwrap();
            let mut slots = Vec::new();
            loop {
                match lifecycle::reserve_cleanup::<1>() {
                    Ok([slot]) => slots.push(slot),
                    Err(error) => {
                        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
                        break;
                    }
                }
            }
            assert_eq!(lifecycle::cleanup_admission_status(), (4096, 4096));
            artifact.retire(py, &inventory).unwrap();
            assert_eq!(artifact.retirement_status().unwrap(), "idle");
            assert_eq!(fs::read(&original).unwrap(), protected);
            assert_eq!(
                material_creation_receipt(&samples, &original).unwrap(),
                Some(creation.clone())
            );
            // Drop the caller's inventory; the aborted dormant admission must
            // not retain a hidden registration lock at capacity.
            inventory.release();
            let mut next = guard.lock_inventory(py).unwrap();
            owner.release();
            // The same exact birth slot is reused, with no capacity-dependent
            // operation after original queue admission and a queryable outcome.
            artifact.retire(py, &next).unwrap();
            assert_eq!(artifact.retirement_status().unwrap(), "pending");
            next.release();
            artifact.release();
            assert_eq!(fs::read(&original).unwrap(), protected);
            let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .unwrap();
            let mut source_hashes = serde_json::Map::new();
            for relative in [
                "rust/crates/looper/src/audio_engine/cold_store/material.rs",
                "rust/crates/looper/src/audio_engine/cold_store/lifecycle.rs",
                "rust/crates/looper/src/audio_engine/project_assets.rs",
                "rust/crates/looper/src/audio_engine/material_migration_recovery.rs",
            ] {
                source_hashes.insert(
                    relative.into(),
                    serde_json::json!(format!(
                        "{:x}",
                        Sha256::digest(fs::read(repo.join(relative)).unwrap())
                    )),
                );
            }
            let executable = std::env::current_exe().unwrap();
            fs::write(fixture.join("capacity-child-verified.json"), serde_json::to_vec(&serde_json::json!({"process_id":std::process::id(), "test_executable":executable, "test_executable_sha256":format!("{:x}", Sha256::digest(fs::read(&executable).unwrap())), "source_sha256":source_hashes, "full_cleanup_capacity":lifecycle::cleanup_admission_status().0, "protected_original_status":"idle", "admitted_original_status_at_capacity":artifact.retirement_status().unwrap(), "aborted_root_inventory_reacquired":true, "original_path":original, "original_sha256":format!("{:x}",Sha256::digest(&protected)), "material_creation":creation, "original_receipt":original_receipt})).unwrap()).unwrap();
            drop(slots);
            guard.release();
        });
        ProjectAssets::shared().orphan_cold(&material.lease);
        material.lease.release_preparation_assignment();
        drop(material);
        ProjectAssets::shared().collect_for_test();
    }
}
