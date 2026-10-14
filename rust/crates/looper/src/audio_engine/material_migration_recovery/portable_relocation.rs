//! Installer-only physical custody. No Engine or audio authority is created here.
use super::config_location::{LocationBinding, LocationProjection, reference_key};
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const PROJECTION: &str = ".portable-location-v1.json";
const STREAM: &str = ":flitzis-portable-location-v1";
const CAPTURE_STREAM: &str = ":flitzis-portable-relocation-v1";
const MAX_ENTRIES: usize = 8192;
const MAX_BYTES: u64 = 512 * 1024 * 1024 * 1024;
const RECORD_BYTES: u64 = 16 * 1024 * 1024;

mod journal;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    identity: FileIdentity,
    directory: bool,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Custody {
    schema_version: u32,
    samples_identity: FileIdentity,
    projection_identity: FileIdentity,
    projection_sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    schema_version: u32,
    root_identity: FileIdentity,
    samples_identity: FileIdentity,
    source: String,
    target: String,
    entries: Vec<Entry>,
    bindings: Vec<LocationBinding>,
    previous_projection: Option<String>,
    previous_custody: Option<String>,
    previous_capture: Option<String>,
    phase: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureSeal {
    schema_version: u32,
    samples_identity: FileIdentity,
    root_identity: FileIdentity,
    record_name: String,
    payload_sha256: String,
    complete: bool,
}

fn capture_stream(root: &Path) -> PathBuf {
    PathBuf::from(format!("{}{CAPTURE_STREAM}", root.to_string_lossy()))
}

fn capture_digest(capture: &Capture) -> io::Result<String> {
    let mut value = serde_json::to_value(capture).map_err(io::Error::other)?;
    value
        .as_object_mut()
        .ok_or_else(|| invalid("capture object missing"))?
        .remove("phase");
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value).map_err(io::Error::other)?)
    ))
}

fn write_capture_seal(
    source: &Path,
    record: &Path,
    capture: &Capture,
    complete: bool,
) -> io::Result<()> {
    let seal = CaptureSeal {
        schema_version: 1,
        samples_identity: capture.samples_identity.clone(),
        root_identity: capture.root_identity.clone(),
        record_name: record
            .file_name()
            .ok_or_else(|| invalid("record name missing"))?
            .to_string_lossy()
            .into(),
        payload_sha256: capture_digest(capture)?,
        complete,
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(capture_stream(source))?;
    file.write_all(&serde_json::to_vec(&seal).map_err(io::Error::other)?)?;
    file.sync_all()
}

fn verify_capture_seal(source: &Path, record: &Path, capture: &Capture) -> io::Result<()> {
    let seal: CaptureSeal =
        serde_json::from_slice(&bounded_bytes(&capture_stream(source), JSON_LIMIT as u64)?)
            .map_err(io::Error::other)?;
    if seal.schema_version != 1
        || seal.samples_identity != capture.samples_identity
        || seal.root_identity != capture.root_identity
        || seal.payload_sha256 != capture_digest(capture)?
        || record
            .file_name()
            .is_none_or(|name| name != seal.record_name.as_str())
    {
        return Err(invalid(
            "Native portable capture custody changed; preserved",
        ));
    }
    Ok(())
}

fn bounded_bytes(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    project_assets::reject_links(path)?;
    let reader = sealed_reader(path)?;
    if reader.metadata()?.len() > limit {
        return Err(invalid("portable metadata exceeds bound; preserved"));
    }
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("portable metadata exceeds bound; preserved"));
    }
    Ok(bytes)
}

fn stream_path(root: &Path) -> PathBuf {
    PathBuf::from(format!("{}{STREAM}", root.to_string_lossy()))
}

pub(super) fn verified_projection(root: &Path) -> io::Result<Option<LocationProjection>> {
    project_assets::reject_links(root)?;
    let path = root.join(PROJECTION);
    let stream = stream_path(root);
    match (path.exists(), stream.exists()) {
        (false, false) => return Ok(None),
        (true, true) => (),
        _ => {
            return Err(invalid(
                "incomplete Native portable location custody; preserved",
            ));
        }
    }
    let bytes = bounded_bytes(&path, JSON_LIMIT as u64)?;
    let custody: Custody = serde_json::from_slice(&bounded_bytes(&stream, JSON_LIMIT as u64)?)
        .map_err(io::Error::other)?;
    let projection: LocationProjection =
        serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if custody.schema_version != 1
        || projection.schema_version != 1
        || project_assets::capture_identity(root)?.as_ref() != Some(&custody.samples_identity)
        || projection.samples_identity != custody.samples_identity
        || project_assets::capture_identity(&path)?.as_ref() != Some(&custody.projection_identity)
        || format!("{:x}", Sha256::digest(&bytes)) != custody.projection_sha256
    {
        return Err(invalid(
            "Native portable location custody mismatch; preserved",
        ));
    }
    Ok(Some(projection))
}

fn ordinary_absolute(path: &Path) -> io::Result<()> {
    if !path.is_absolute()
        || path.to_string_lossy().len() > PATH_LIMIT
        || path
            .to_string_lossy()
            .split(['/', '\\'])
            .any(|part| [".", ".."].contains(&part))
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid("portable path is not an ordinary absolute path"));
    }
    project_assets::reject_links(path)
}

fn paths(root: &Path, source: &Path, target: &Path, record: &Path) -> io::Result<PathBuf> {
    for path in [root, source, target, record] {
        ordinary_absolute(path)?;
    }
    let root = fs::canonicalize(root)?;
    let marker: Value = serde_json::from_slice(&bounded_bytes(
        &root.join("portable-root.json"),
        JSON_LIMIT as u64,
    )?)
    .map_err(io::Error::other)?;
    if marker["schema_version"] != 1 || marker["app"] != "flitzis-looper" {
        return Err(invalid(
            "portable Native hook requires an owned installation root",
        ));
    }
    for path in [source, target, record] {
        let resolved = project_assets::canonical_with_missing(path)?;
        if resolved == root || !resolved.starts_with(&root) {
            return Err(invalid("portable path escaped installation root"));
        }
    }
    if source.file_name().is_none_or(|name| name != "samples")
        || target.file_name().is_none_or(|name| name != "samples")
        || project_assets::canonical_with_missing(target)? != root.join("flitzis-looper/samples")
        || record
            .parent()
            .map(project_assets::canonical_with_missing)
            .transpose()?
            != Some(root.join("cache"))
    {
        return Err(invalid("portable relocation path convention mismatch"));
    }
    let operation = record
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("relocation-"))
        .and_then(|name| name.strip_suffix(".json"))
        .filter(|id| material_paths::valid_id(id));
    if operation.is_none() {
        return Err(invalid("portable relocation operation ID invalid"));
    }
    Ok(root)
}

#[cfg(windows)]
fn move_guard(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    // DELETE access conflicts with every Native no-delete directory guard.
    // Sharing all accesses lets this owner inspect descendants and rename by its
    // existing handle; new Native guards deny DELETE and consequently cannot join.
    let file = OpenOptions::new()
        .access_mode(0x8000_0000 | 0x0001_0000)
        .share_mode(7)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err(invalid("portable source is not an ordinary directory"));
    }
    Ok(file)
}

#[cfg(not(windows))]
fn move_guard(_path: &Path) -> io::Result<File> {
    Err(invalid(
        "portable relocation requires Windows Native share-mode custody",
    ))
}

#[cfg(windows)]
fn identity(path: &Path) -> io::Result<FileIdentity> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    let file = OpenOptions::new()
        .read(true)
        .share_mode(7)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)?;
    if file.metadata()?.file_attributes() & 0x400 != 0 {
        return Err(invalid("portable reparse object; preserved"));
    }
    project_assets::file_identity(&file)
}
#[cfg(not(windows))]
fn identity(path: &Path) -> io::Result<FileIdentity> {
    project_assets::capture_identity(path)?.ok_or_else(|| invalid("portable identity missing"))
}

#[cfg(windows)]
fn rename_guard(file: &File, target: &Path) -> io::Result<()> {
    use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetFileInformationByHandle(
            handle: *mut std::ffi::c_void,
            class: i32,
            information: *const std::ffi::c_void,
            bytes: u32,
        ) -> i32;
    }
    let target = project_assets::canonical_with_missing(target)?;
    let name: Vec<u16> = target.as_os_str().encode_wide().collect();
    let bytes = 22_usize
        .checked_add(name.len() * 2)
        .ok_or_else(|| invalid("rename path overflow"))?;
    let mut storage = vec![0_u64; bytes.div_ceil(8)];
    let pointer = storage.as_mut_ptr().cast::<u8>();
    // SAFETY: FILE_RENAME_INFO on Windows x64 has RootDirectory at byte8,
    // FileNameLength at16 and UTF16 FileName at20. Aligned storage is large
    // enough for the complete structure and terminating UTF16 zero; the name
    // length excludes that terminator. No replacement is permitted.
    unsafe {
        pointer
            .add(16)
            .cast::<u32>()
            .write_unaligned((name.len() * 2) as u32);
        std::ptr::copy_nonoverlapping(name.as_ptr().cast::<u8>(), pointer.add(20), name.len() * 2);
        if SetFileInformationByHandle(file.as_raw_handle(), 3, pointer.cast(), bytes as u32) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
#[cfg(not(windows))]
fn rename_guard(_file: &File, _target: &Path) -> io::Result<()> {
    Err(invalid("portable physical rename is Windows-only"))
}

fn tree(root: &Path, inventory: Option<&File>) -> io::Result<(Vec<Entry>, Vec<File>)> {
    let mut pending = vec![root.to_path_buf()];
    let mut entries = Vec::new();
    let mut readers = Vec::new();
    let mut total = 0_u64;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if entries.len() >= MAX_ENTRIES {
                return Err(invalid(
                    "portable samples inventory exceeds 8192 entries; preserved",
                ));
            }
            project_assets::reject_links(&path)?;
            let relative = path.strip_prefix(root).map_err(io::Error::other)?;
            if relative.components().any(|part| matches!(part, Component::Normal(name) if name.to_str().is_none_or(|name| !material_paths::filename(name)))) {
                return Err(invalid("nonordinary samples child; preserved"));
            }
            let metadata = fs::symlink_metadata(&path)?;
            let is_directory = metadata.is_dir();
            let (bytes, sha256) = if is_directory {
                pending.push(path.clone());
                (0, String::new())
            } else if metadata.is_file() {
                total = total
                    .checked_add(metadata.len())
                    .ok_or_else(|| invalid("portable extent overflow"))?;
                if total > MAX_BYTES {
                    return Err(invalid(
                        "portable samples extent exceeds bounded512GiB; preserved",
                    ));
                }
                // The inventory lock is already held exclusively by this
                // operation and intentionally has no simultaneous reader.
                if relative == Path::new(".migration-projects/.inventory.lock") {
                    if metadata.len() != 0 {
                        return Err(invalid("unknown inventory lock bytes; preserved"));
                    }
                    (0, format!("{:x}", Sha256::digest([])))
                } else {
                    let mut reader = sealed_reader(&path)?;
                    let proof = hash_file(&mut reader)?;
                    readers.push(reader);
                    proof
                }
            } else {
                return Err(invalid("unknown samples object; preserved"));
            };
            let entry_identity = if relative == Path::new(".migration-projects/.inventory.lock") {
                project_assets::file_identity(
                    inventory.ok_or_else(|| invalid("inventory lock custody missing"))?,
                )?
            } else {
                identity(&path)?
            };
            entries.push(Entry {
                path: relative.to_string_lossy().replace('\\', "/"),
                identity: entry_identity,
                directory: is_directory,
                bytes,
                sha256,
            });
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((entries, readers))
}

fn inventory_lock(source: &Path) -> io::Result<Option<File>> {
    let inventory = source.join(".migration-projects");
    if !inventory.exists() {
        return Ok(None);
    }
    project_assets::reject_links(&inventory)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .custom_flags(0x0020_0000)
            .open(inventory.join(".inventory.lock"))?;
        if !lock.metadata()?.is_file() || lock.metadata()?.len() != 0 {
            return Err(invalid("unknown inventory lock object; preserved"));
        }
        Ok(Some(lock))
    }
    #[cfg(not(windows))]
    {
        Err(invalid("portable inventory custody requires Windows"))
    }
}

fn config_bindings(source: &Path) -> io::Result<Vec<LocationBinding>> {
    let mut references = BTreeSet::new();
    let project = source.join(".migration-projects");
    if project.exists() {
        let gate = inventory_gate(source)?;
        for encoded in gate.records_native()? {
            let record: Value = serde_json::from_str(&encoded).map_err(io::Error::other)?;
            if !record["error"].is_null() || record["live"] != false {
                return Err(invalid(
                    "live or unknown project registration blocks portable relocation; preserved",
                ));
            }
            references.insert(
                record["config_reference"]
                    .as_str()
                    .ok_or_else(|| invalid("project config binding missing"))?
                    .to_owned(),
            );
        }
    }
    let journals = source.join(".material-migrations");
    if journals.exists() {
        let mut count = 0;
        let mut histories = BTreeMap::new();
        for entry in fs::read_dir(&journals)? {
            count += 1;
            if count > RECORD_LIMIT {
                return Err(invalid("portable journal inventory exceeds256; preserved"));
            }
            let id = entry?
                .file_name()
                .into_string()
                .map_err(|_| invalid("journal name invalid"))?;
            let store =
                super::super::material_migration_journal::MaterialMigrationJournalStore::open(
                    source.to_string_lossy().into(),
                    id.clone(),
                )
                .map_err(|error| invalid(error.to_string()))?;
            let encoded = store
                .history()
                .map_err(|error| invalid(error.to_string()))?;
            histories.insert(
                id.clone(),
                journal::History::validate(source, &id, &encoded)?,
            );
        }
        journal::validate_successors(source, &histories)?;
        references.extend(
            histories
                .into_values()
                .map(|history| history.config_reference),
        );
    }
    let mut bindings: BTreeMap<String, LocationBinding> = BTreeMap::new();
    if let Some(previous) = verified_projection(source)? {
        for binding in previous.bindings {
            references.insert(binding.config_reference);
        }
    }
    for reference in references {
        let config = super::config_location::resolve(source, Path::new(&reference))?;
        let relative = config
            .strip_prefix(fs::canonicalize(source)?)
            .map_err(|_| {
                invalid(format!(
                    "known config outside moved Samples; preserved: {reference}"
                ))
            })?;
        if !config.is_file() {
            return Err(invalid(format!(
                "known config unavailable; preserved: {reference}"
            )));
        }
        let parent = config
            .parent()
            .ok_or_else(|| invalid("config parent missing"))?;
        let binding = LocationBinding {
            config_reference: reference,
            config_path: relative.to_string_lossy().replace('\\', "/"),
            config_identity: identity(&config)?,
            parent_identity: identity(parent)?,
        };
        insert_binding(&mut bindings, binding)?;
    }
    Ok(bindings.into_values().collect())
}

fn atomic_record(path: &Path, capture: &Capture, create: bool) -> io::Result<()> {
    let bytes = serde_json::to_vec(capture).map_err(io::Error::other)?;
    if bytes.len() as u64 > RECORD_BYTES {
        return Err(invalid("portable capture exceeds record bound"));
    }
    let temporary = path.with_extension("pending");
    if create && (temporary.exists() || path.exists()) {
        return Err(invalid("portable journal collision; preserved"));
    }
    if temporary.exists() {
        // Update callers already hold verified physical Samples custody. A
        // flushed own update can finish its rename; foreign bytes stay intact.
        let previous = load_record(path)?;
        let pending = pending_update(path, &previous)?
            .ok_or_else(|| invalid("portable pending update disappeared; preserved"))?;
        if serde_json::to_vec(&pending).map_err(io::Error::other)? != bytes {
            return Err(invalid("portable pending update conflicts; preserved"));
        }
        return fs::rename(&temporary, path);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

fn load_record(path: &Path) -> io::Result<Capture> {
    let capture: Capture =
        serde_json::from_slice(&bounded_bytes(path, RECORD_BYTES)?).map_err(io::Error::other)?;
    if capture.schema_version != 1
        || capture.entries.len() > MAX_ENTRIES
        || capture.bindings.len() > 1024
        || !["captured", "moving", "moved", "rolled_back"].contains(&capture.phase.as_str())
    {
        return Err(invalid("unrecognized portable capture; preserved"));
    }
    Ok(capture)
}

fn pending_update(path: &Path, previous: &Capture) -> io::Result<Option<Capture>> {
    let temporary = path.with_extension("pending");
    if !temporary.exists() {
        return Ok(None);
    }
    let pending = load_record(&temporary)?;
    let allowed = match previous.phase.as_str() {
        "captured" => ["moving", "moved", "rolled_back"].contains(&pending.phase.as_str()),
        "moving" => ["moving", "moved", "rolled_back"].contains(&pending.phase.as_str()),
        "moved" => ["moved", "rolled_back"].contains(&pending.phase.as_str()),
        _ => false,
    };
    if !allowed
        || capture_digest(&pending)? != capture_digest(previous)?
        || bounded_bytes(&temporary, RECORD_BYTES)?
            != serde_json::to_vec(&pending).map_err(io::Error::other)?
    {
        return Err(invalid("unrecognized portable pending update; preserved"));
    }
    Ok(Some(pending))
}

fn finish_pending_update(
    record: &Path,
    previous: &Capture,
    pending: &Capture,
    source: &Path,
    target: &Path,
) -> io::Result<()> {
    let at_source = source.is_dir() && identity(source)? == previous.samples_identity;
    let at_target = target.is_dir() && identity(target)? == previous.samples_identity;
    if !at_source && !at_target || at_source && at_target && source != target {
        return Err(invalid(
            "portable pending custody missing/ambiguous; preserved",
        ));
    }
    let current = if at_target { target } else { source };
    let moved = match pending.phase.as_str() {
        "moving" if at_source => false,
        "moved" if at_target => {
            if previous.phase == "captured"
                && reference_key(Path::new(&previous.source))
                    != reference_key(Path::new(&previous.target))
            {
                return Err(invalid("portable pending phase skipped move; preserved"));
            }
            if verified_projection(target)?.is_none()
                || bounded_bytes(&target.join(PROJECTION), JSON_LIMIT as u64)?
                    != projection_bytes(target, previous)?
            {
                return Err(invalid(
                    "portable pending moved projection changed; preserved",
                ));
            }
            true
        }
        "rolled_back" if at_source => false,
        _ => {
            return Err(invalid(
                "portable pending phase/location conflicts; preserved",
            ));
        }
    };
    verify_capture_seal(current, record, previous)?;
    let guard = move_guard(current)?;
    if project_assets::file_identity(&guard)? != previous.samples_identity {
        return Err(invalid("portable pending root custody changed; preserved"));
    }
    let inventory = inventory_lock(current)?;
    let readers = verify_tree(current, previous, moved, inventory.as_ref())?;
    drop(readers);
    if pending.phase == "rolled_back" {
        let projection = if source.join(PROJECTION).exists() {
            Some(fs::read_to_string(source.join(PROJECTION))?)
        } else {
            None
        };
        let custody = if stream_path(source).exists() {
            Some(fs::read_to_string(stream_path(source))?)
        } else {
            None
        };
        if projection != previous.previous_projection || custody != previous.previous_custody {
            return Err(invalid(
                "portable pending rollback custody changed; preserved",
            ));
        }
    }
    // The inventory stays exclusive while only external metadata is renamed.
    // Samples itself and its ordinary files have already been reverified.
    atomic_record(record, pending, false)
}

fn verify_tree(
    source: &Path,
    capture: &Capture,
    moved: bool,
    inventory: Option<&File>,
) -> io::Result<Vec<File>> {
    let (actual, readers) = tree(source, inventory)?;
    let filtered = |entries: Vec<Entry>| {
        entries
            .into_iter()
            .filter(|entry| !moved || entry.path != PROJECTION)
            .collect::<Vec<_>>()
    };
    if filtered(actual) != filtered(capture.entries.clone()) {
        return Err(invalid(
            "portable captured files/IDs/bytes changed; preserved",
        ));
    }
    for binding in &capture.bindings {
        let config = source.join(&binding.config_path);
        if identity(&config)? != binding.config_identity
            || identity(
                config
                    .parent()
                    .ok_or_else(|| invalid("config parent missing"))?,
            )? != binding.parent_identity
        {
            return Err(invalid(
                "portable captured config custody changed; preserved",
            ));
        }
    }
    Ok(readers)
}

fn insert_binding(
    bindings: &mut BTreeMap<String, LocationBinding>,
    binding: LocationBinding,
) -> io::Result<()> {
    let key = reference_key(Path::new(&binding.config_reference));
    if let Some(previous) = bindings.get(&key) {
        if reference_key(Path::new(&previous.config_path))
            != reference_key(Path::new(&binding.config_path))
            || previous.config_identity != binding.config_identity
            || previous.parent_identity != binding.parent_identity
        {
            return Err(invalid(
                "conflicting portable config address custody; preserved",
            ));
        }
    } else {
        bindings.insert(key, binding);
    }
    Ok(())
}

fn projection_bytes(source: &Path, capture: &Capture) -> io::Result<Vec<u8>> {
    let mut bindings = BTreeMap::new();
    for binding in &capture.bindings {
        insert_binding(&mut bindings, binding.clone())?;
    }
    for binding in &capture.bindings {
        let reference = plain_path(&source.join(&binding.config_path));
        let mut final_alias = binding.clone();
        final_alias.config_reference = reference;
        insert_binding(&mut bindings, final_alias)?;
    }
    if bindings.len() > 1024 {
        return Err(invalid(
            "portable location binding capacity exceeded; preserved",
        ));
    }
    let projection = LocationProjection {
        schema_version: 1,
        samples_identity: capture.samples_identity.clone(),
        bindings: bindings.into_values().collect(),
    };
    let bytes = serde_json::to_vec(&projection).map_err(io::Error::other)?;
    if bytes.len() > JSON_LIMIT {
        return Err(invalid(
            "portable location exceeds metadata bound; preserved",
        ));
    }
    Ok(bytes)
}

fn write_projection(source: &Path, capture: &Capture) -> io::Result<()> {
    let bytes = projection_bytes(source, capture)?;
    let path = source.join(PROJECTION);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    let projection_identity = project_assets::file_identity(&file)?;
    drop(file);
    let custody = Custody {
        schema_version: 1,
        samples_identity: capture.samples_identity.clone(),
        projection_identity,
        projection_sha256: format!("{:x}", Sha256::digest(&bytes)),
    };
    let mut stream = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(stream_path(source))?;
    stream.write_all(&serde_json::to_vec(&custody).map_err(io::Error::other)?)?;
    stream.sync_all()
}

fn restore_projection(source: &Path, capture: &Capture) -> io::Result<()> {
    for (path, previous) in [
        (source.join(PROJECTION), &capture.previous_projection),
        (stream_path(source), &capture.previous_custody),
    ] {
        if let Some(bytes) = previous {
            let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
            file.write_all(bytes.as_bytes())?;
            file.sync_all()?;
        } else if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn restore_capture_stream(source: &Path, capture: &Capture) -> io::Result<()> {
    let path = capture_stream(source);
    if let Some(previous) = &capture.previous_capture {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.write_all(previous.as_bytes())?;
        file.sync_all()?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn run(
    root: &Path,
    source: &Path,
    target: &Path,
    record: &Path,
    action: &str,
) -> io::Result<String> {
    let root = paths(root, source, target, record)?;
    let _installation_guards = project_assets::directory_guards(&root)?;
    let _record_guards = project_assets::directory_guards(&root.join("cache"))?;
    if action == "capture" {
        if !source.is_dir() || record.exists() {
            return Err(invalid(
                "portable capture source/collision invalid; preserved",
            ));
        }
        let bindings = config_bindings(source)?;
        let previous = verified_projection(source)?;
        let previous_projection = previous
            .as_ref()
            .map(|_| fs::read_to_string(source.join(PROJECTION)))
            .transpose()?;
        let previous_custody = previous
            .as_ref()
            .map(|_| fs::read_to_string(stream_path(source)))
            .transpose()?;
        let previous_capture = if capture_stream(source).exists() {
            let bytes = bounded_bytes(&capture_stream(source), JSON_LIMIT as u64)?;
            let seal: CaptureSeal = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            if seal.schema_version != 1
                || project_assets::capture_identity(source)?.as_ref()
                    != Some(&seal.samples_identity)
                || identity(&root)? != seal.root_identity
            {
                return Err(invalid(
                    "unfinished or unknown earlier Native relocation; preserved",
                ));
            }
            if seal.complete {
                if previous.is_none() {
                    return Err(invalid(
                        "completed capture has no actual location custody; preserved",
                    ));
                }
                Some(String::from_utf8(bytes).map_err(io::Error::other)?)
            } else {
                // Capture can finish just before the installer's flag is flushed.
                // An exact sealed, still-unmoved capture can be superseded after
                // the installer preserved its failed record; no tree bytes or
                // location projection are altered by this metadata transition.
                let operation = seal
                    .record_name
                    .strip_prefix("relocation-")
                    .and_then(|name| name.strip_suffix(".json"))
                    .filter(|id| material_paths::valid_id(id))
                    .ok_or_else(|| invalid("earlier capture operation invalid"))?;
                let original = root.join("cache").join(&seal.record_name);
                let preserved = root
                    .join("cache")
                    .join(format!("failed-relocation-{operation}.json"));
                let old = load_record(if original.exists() {
                    &original
                } else {
                    &preserved
                })?;
                if old.phase != "captured"
                    || old.source != plain_path(&fs::canonicalize(source)?)
                    || old.samples_identity != seal.samples_identity
                    || old.root_identity != seal.root_identity
                    || capture_digest(&old)? != seal.payload_sha256
                {
                    return Err(invalid(
                        "earlier capture may have moved or changed; preserved",
                    ));
                }
                old.previous_capture
            }
        } else {
            None
        };
        let guard = move_guard(source)?;
        let samples_identity = project_assets::file_identity(&guard)?;
        let inventory = inventory_lock(source)?;
        let (entries, _readers) = tree(source, inventory.as_ref())?;
        if identity(&root)?.0[0] != samples_identity.0[0] {
            return Err(invalid("portable relocation crosses volume; preserved"));
        }
        let capture = Capture {
            schema_version: 1,
            root_identity: identity(&root)?,
            samples_identity,
            source: plain_path(&fs::canonicalize(source)?),
            target: plain_path(&project_assets::canonical_with_missing(target)?),
            entries,
            bindings,
            previous_projection,
            previous_custody,
            previous_capture,
            phase: "captured".into(),
        };
        // Reject capacity/serialized-size overflow before the installer can
        // swap checkout/runtime or physically move Samples.
        projection_bytes(target, &capture)?;
        atomic_record(record, &capture, true)?;
        if let Err(error) = write_capture_seal(source, record, &capture, false) {
            // No physical data/code swap has occurred. Restore only the prior
            // capture stream; keep this external record as failed diagnostics.
            restore_capture_stream(source, &capture)?;
            return Err(invalid(format!(
                "Native capture seal unavailable; no data moved, incomplete diagnostic record preserved: {error}"
            )));
        }
        return Ok(json!({"schema_version":1,"phase":"captured"}).to_string());
    }
    if !["move", "recover", "rollback"].contains(&action) {
        return Err(invalid("unknown portable relocation action"));
    }
    let mut capture = load_record(record)?;
    if capture.root_identity != identity(&root)?
        || capture.target != plain_path(&project_assets::canonical_with_missing(target)?)
    {
        return Err(invalid(
            "portable relocation record belongs to another installation; preserved",
        ));
    }
    let at_target = target.is_dir() && identity(target)? == capture.samples_identity;
    let at_source = source.is_dir() && identity(source)? == capture.samples_identity;
    if !at_target && !at_source || at_target && at_source && source != target {
        return Err(invalid("portable custody missing/ambiguous; preserved"));
    }
    let current = if at_target { target } else { source };
    if let Some(pending) = pending_update(record, &capture)? {
        if pending.phase == "rolled_back" && action == "move" {
            return Err(invalid(
                "portable pending rollback requires recovery; preserved",
            ));
        }
        finish_pending_update(record, &capture, &pending, source, target)?;
        capture = pending;
    }
    if capture.phase == "rolled_back" && at_source && ["rollback", "recover"].contains(&action) {
        // A repeated rollback is read-only. The original capture seal has been
        // removed/restored already; do not use its historical JSON to grant a
        // second write or move. Verify original physical custody and bytes.
        let current_seal = if capture_stream(source).exists() {
            Some(fs::read_to_string(capture_stream(source))?)
        } else {
            None
        };
        let pending_cleanup = current_seal != capture.previous_capture;
        if pending_cleanup {
            verify_capture_seal(source, record, &capture)?;
        }
        let guard = move_guard(source)?;
        if project_assets::file_identity(&guard)? != capture.samples_identity {
            return Err(invalid("rollback root identity changed; preserved"));
        }
        let inventory = inventory_lock(source)?;
        let _readers = verify_tree(source, &capture, false, inventory.as_ref())?;
        if pending_cleanup {
            restore_capture_stream(source, &capture)?;
        }
        return Ok(json!({"schema_version":1,"phase":"rolled_back"}).to_string());
    }
    verify_capture_seal(current, record, &capture)?;
    let destination = if action == "rollback" { source } else { target };
    let _destination_guards = project_assets::directory_guards(
        destination
            .parent()
            .ok_or_else(|| invalid("portable destination parent missing"))?,
    )?;
    let guard = move_guard(current)?;
    if project_assets::file_identity(&guard)? != capture.samples_identity {
        return Err(invalid("portable root ID changed; preserved"));
    }
    let inventory = inventory_lock(current)?;
    let moved = capture.phase == "moved" || at_target && capture.phase == "moving";
    let readers = verify_tree(current, &capture, moved, inventory.as_ref())?;
    drop(readers);
    // Windows will not rename a directory with an open descendant, even when
    // that descendant shares DELETE. The live DELETE-bearing root handle still
    // prevents Native samples_root/directory_guards (and consequently inventory
    // owners) from joining until this operation ends.
    drop(inventory);
    if action == "rollback" {
        if current != destination {
            if destination.exists() {
                return Err(invalid("portable rollback destination occupied; preserved"));
            }
            rename_guard(&guard, destination)?;
        }
        restore_projection(destination, &capture)?;
        capture.phase = "rolled_back".into();
    } else if ["move", "recover"].contains(&action) {
        if current != destination {
            if destination.exists() {
                return Err(invalid(
                    "portable relocation destination occupied; preserved",
                ));
            }
            if identity(
                destination
                    .parent()
                    .ok_or_else(|| invalid("target parent missing"))?,
            )?
            .0[0]
                != capture.samples_identity.0[0]
            {
                return Err(invalid("portable relocation crosses volume; preserved"));
            }
            capture.phase = "moving".into();
            atomic_record(record, &capture, false)?;
            rename_guard(&guard, destination)?;
        }
        let inventory = inventory_lock(destination)?;
        let _verified = verify_tree(destination, &capture, moved, inventory.as_ref())?;
        drop(_verified);
        drop(inventory);
        write_projection(destination, &capture)?;
        capture.phase = "moved".into();
    } else {
        return Err(invalid("unknown portable relocation action"));
    }
    atomic_record(record, &capture, false)?;
    if capture.phase == "moved" {
        write_capture_seal(destination, record, &capture, true)?;
    }
    if capture.phase == "rolled_back" {
        restore_capture_stream(destination, &capture)?;
    }
    Ok(json!({"schema_version":1,"phase":capture.phase}).to_string())
}

/// Perform an installer-owned physical Samples transaction without creating an Engine.
#[pyfunction]
pub fn portable_relocate_data(
    py: Python<'_>,
    root: String,
    source: String,
    target: String,
    record: String,
    action: String,
) -> PyResult<String> {
    py.detach(|| {
        run(
            Path::new(&root),
            Path::new(&source),
            Path::new(&target),
            Path::new(&record),
            &action,
        )
    })
    .map_err(py_error)
}

#[cfg(test)]
mod tests;
