//! Durable evidence is reverified off-thread; it never restores runtime authority.
use super::{cold_store::sealed_reader, material_paths, project_assets};
use material_paths::AssetKind;
use project_assets::{FileIdentity, ProjectAssetLease, ProjectAssets, VerifiedFile};
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const JSON_LIMIT: usize = 256 * 1024;
const PATH_LIMIT: usize = 8192;
const RECORD_LIMIT: usize = 256;
const STEM_NAMES: [&str; 6] = [
    "vocals.wav",
    "melody.wav",
    "bass.wav",
    "drums.wav",
    "instrumental.wav",
    ".complete.json",
];
const PCM_NAMES: [&str; 3] = ["decoder.f32le", "playback.f32le", "manifest.json"];
const STEM_PCM_NAMES: [&str; 6] = [
    "vocals.f32le",
    "melody.f32le",
    "bass.f32le",
    "drums.f32le",
    "instrumental.f32le",
    "manifest.json",
];

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
fn py_error(error: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(error.to_string())
}
fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ArtifactKind {
    Original,
    StemDirectory,
    PcmDirectory,
    StemPcmDirectory,
    StemPairDescriptor,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    kind: ArtifactKind,
    samples_identity: FileIdentity,
    reference: String,
    identity: FileIdentity,
    files: Vec<VerifiedFile>,
}

struct SealedArtifact {
    receipt: Receipt,
    root: PathBuf,
    path: PathBuf,
    _files: Vec<File>,
    _guards: Vec<File>,
}

fn samples_root(path: &Path) -> io::Result<(PathBuf, Vec<File>)> {
    if !path.is_absolute() || path.file_name().is_none_or(|name| name != "samples") {
        return Err(invalid(
            "recovery root must be an absolute samples directory",
        ));
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || path
            .to_string_lossy()
            .split(['/', '\\'])
            .any(|part| [".", ".."].contains(&part))
    {
        return Err(invalid("recovery samples root contains traversal"));
    }
    project_assets::reject_links(path)?;
    let guards = project_assets::directory_guards(path)?;
    let root = fs::canonicalize(path)?;
    if !root.is_dir() {
        return Err(invalid("samples root is not a directory"));
    }
    Ok((root, guards))
}

fn persisted_reference(root: &Path, path: &Path) -> io::Result<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| invalid("artifact escaped samples root"))?;
    let reference = format!("samples/{}", relative.to_string_lossy().replace('\\', "/"));
    if reference.len() > PATH_LIMIT {
        return Err(invalid("artifact reference exceeds bound"));
    }
    Ok(reference)
}

fn hash_file(file: &mut File) -> io::Result<(u64, String)> {
    file.seek(SeekFrom::Start(0))?;
    let expected = file.metadata()?.len();
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or_else(|| invalid("artifact extent overflow"))?;
        if bytes > expected {
            return Err(invalid("sealed artifact changed length"));
        }
        digest.update(&buffer[..count]);
    }
    if bytes != expected {
        return Err(invalid("sealed artifact ended early"));
    }
    file.seek(SeekFrom::Start(0))?;
    Ok((bytes, format!("{:x}", digest.finalize())))
}

fn json_bytes(file: &mut File) -> io::Result<Vec<u8>> {
    if file.metadata()?.len() > JSON_LIMIT as u64 {
        return Err(invalid("artifact JSON exceeds bound"));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(JSON_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > JSON_LIMIT {
        return Err(invalid("artifact JSON exceeds bound"));
    }
    Ok(bytes)
}

fn json_file(file: &mut File) -> io::Result<Value> {
    serde_json::from_slice(&json_bytes(file)?).map_err(io::Error::other)
}

fn exact_keys(value: &Value, keys: &[&str]) -> io::Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("artifact metadata must be an object"))?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(invalid("unknown or missing artifact metadata fields"));
    }
    Ok(())
}

fn verify_pcm_metadata(path: &Path, files: &mut [File], proof: &[VerifiedFile]) -> io::Result<()> {
    let manifest = json_file(&mut files[2])?;
    exact_keys(
        &manifest,
        &[
            "schema_version",
            "identity",
            "decoder_identity",
            "descriptor",
        ],
    )?;
    let descriptor = &manifest["descriptor"];
    exact_keys(descriptor, &["schema_version", "decoder", "playback"])?;
    exact_keys(
        &descriptor["decoder"],
        &["schema_version", "original", "pcm"],
    )?;
    exact_keys(&descriptor["decoder"]["original"], &["sha256", "bytes"])?;
    exact_keys(
        &descriptor["playback"],
        &["parent_identity", "pcm", "transform"],
    )?;
    let digest = |value: &Value| -> io::Result<String> {
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(value).map_err(io::Error::other)?)
        ))
    };
    if manifest["schema_version"] != 1
        || descriptor["schema_version"] != 1
        || descriptor["decoder"]["schema_version"] != 1
        || descriptor["playback"]["parent_identity"] != manifest["decoder_identity"]
        || !descriptor["decoder"]["original"]["sha256"]
            .as_str()
            .is_some_and(digest_valid)
        || descriptor["decoder"]["original"]["bytes"]
            .as_u64()
            .is_none()
        || !descriptor["playback"]["transform"].is_object()
        || manifest["identity"].as_str() != Some(digest(descriptor)?.as_str())
        || manifest["decoder_identity"].as_str() != Some(digest(&descriptor["decoder"])?.as_str())
    {
        return Err(invalid("PCM manifest identity mismatch"));
    }
    let generation = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("PCM generation name missing"))?;
    if !material_paths::ready_generation(generation)
        && generation.split('-').next() != manifest["identity"].as_str()
    {
        return Err(invalid(
            "legacy PCM generation does not bind its complete identity",
        ));
    }
    if serde_json::to_vec(&manifest).map_err(io::Error::other)? != json_bytes(&mut files[2])? {
        return Err(invalid("PCM manifest is not complete canonical JSON"));
    }
    if !super::sample_loader::validate_decoder_cache_provenance(
        &descriptor["decoder"]["pcm"]["provenance"],
    ) || descriptor["playback"]["pcm"]["provenance"]
        != json!({"processing":"full-buffer-playback-v1"})
    {
        return Err(invalid("unsupported executing decoder/playback provenance"));
    }
    let dimension = |branch: &str, key: &str| -> io::Result<usize> {
        usize::try_from(
            descriptor[branch]["pcm"][key]
                .as_u64()
                .ok_or_else(|| invalid("PCM dimension missing"))?,
        )
        .map_err(|_| invalid("PCM dimension overflow"))
    };
    let source_rate = u32::try_from(dimension("decoder", "rate_hz")?)
        .map_err(|_| invalid("decoder rate overflow"))?;
    let output_rate = u32::try_from(dimension("playback", "rate_hz")?)
        .map_err(|_| invalid("playback rate overflow"))?;
    let transform = super::sample_loader::playback_cache_transform(
        source_rate,
        dimension("decoder", "channels")?,
        dimension("decoder", "full_frames")?,
        output_rate,
        dimension("playback", "channels")?,
        super::cold_jobs::PCM_LIMIT_BYTES,
    )
    .map_err(io::Error::other)?;
    if descriptor["playback"]["transform"] != transform
        || transform["output_frames"].as_u64()
            != descriptor["playback"]["pcm"]["full_frames"].as_u64()
    {
        return Err(invalid("unsupported executing playback transform"));
    }
    for (index, branch) in ["decoder", "playback"].into_iter().enumerate() {
        let pcm = &descriptor[branch]["pcm"];
        exact_keys(
            pcm,
            &[
                "format",
                "channel_layout",
                "rate_hz",
                "channels",
                "full_frames",
                "full_bytes",
                "interleaved_sha256",
                "mono_sha256",
                "mono_revision",
                "source_zero_bits",
                "provenance",
            ],
        )?;
        let channels = pcm["channels"]
            .as_u64()
            .ok_or_else(|| invalid("PCM channels missing"))?;
        let frames = pcm["full_frames"]
            .as_u64()
            .ok_or_else(|| invalid("PCM frames missing"))?;
        let extent = frames
            .checked_mul(channels)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| invalid("PCM extent overflow"))?;
        if !(1..=32).contains(&channels)
            || frames == 0
            || pcm["rate_hz"]
                .as_u64()
                .is_none_or(|rate| rate == 0 || rate > u32::MAX as u64)
            || pcm["format"] != "f32-le-interleaved-v1"
            || pcm["channel_layout"] != "interleaved-channel-index-order-v1"
            || pcm["mono_revision"] != super::analysis_pcm::MONO_RULE
            || pcm["source_zero_bits"] != "0000000000000000"
            || pcm["full_bytes"].as_u64() != Some(extent)
            || proof[index].bytes != extent
            || pcm["interleaved_sha256"].as_str() != Some(proof[index].sha256.as_str())
        {
            return Err(invalid("complete PCM manifest geometry or digest mismatch"));
        }
        let mut mono = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let frame_bytes = channels as usize * 4;
        let chunk_bytes = buffer.len() / frame_bytes * frame_bytes;
        files[index].seek(SeekFrom::Start(0))?;
        let mut remaining = extent;
        while remaining > 0 {
            let count = remaining.min(chunk_bytes as u64) as usize;
            files[index].read_exact(&mut buffer[..count])?;
            for frame in buffer[..count].chunks_exact(frame_bytes) {
                let mut sum = 0_f64;
                for bytes in frame.chunks_exact(4) {
                    let sample = f32::from_le_bytes(bytes.try_into().expect("float extent"));
                    if !sample.is_finite() {
                        return Err(invalid("nonfinite sealed PCM"));
                    }
                    sum += f64::from(sample);
                }
                mono.update(((sum / channels as f64) as f32).to_le_bytes());
            }
            remaining -= count as u64;
        }
        if pcm["mono_sha256"].as_str() != Some(format!("{:x}", mono.finalize()).as_str()) {
            return Err(invalid("complete PCM mono digest mismatch"));
        }
    }
    Ok(())
}

fn verify_stem_metadata(files: &mut [File], proof: &[VerifiedFile]) -> io::Result<()> {
    let marker = json_file(&mut files[5])?;
    exact_keys(&marker, &["schema", "source_version", "stems"])?;
    let source = marker["source_version"]
        .as_str()
        .ok_or_else(|| invalid("stem source version missing"))?;
    if marker["schema"] != "stem-set-sha256-v1" || source.is_empty() || source.len() > PATH_LIMIT {
        return Err(invalid("stem completion schema or source version invalid"));
    }
    exact_keys(
        &marker["stems"],
        &["vocals", "melody", "bass", "drums", "instrumental"],
    )?;
    for file in &proof[..5] {
        let name = file.name.strip_suffix(".wav").expect("known stem leaf");
        if marker["stems"][name].as_str() != Some(file.sha256.as_str()) {
            return Err(invalid("complete stem digest mismatch"));
        }
    }
    Ok(())
}

fn seal_artifact(root: &Path, reference: &Path) -> io::Result<SealedArtifact> {
    let (root, mut guards) = samples_root(root)?;
    let samples_identity = project_assets::capture_identity(&root)?
        .ok_or_else(|| invalid("samples identity missing"))?;
    let typed = material_paths::resolve(&root, reference)?;
    let path = typed.path;
    let kind = match typed.kind {
        AssetKind::Original { .. } => ArtifactKind::Original,
        AssetKind::PcmDirectory => ArtifactKind::PcmDirectory,
        AssetKind::StemPcmDirectory {
            generation: true, ..
        } if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(material_paths::ready_generation) =>
        {
            ArtifactKind::StemPcmDirectory
        }
        AssetKind::StemPairDescriptor { .. } => ArtifactKind::StemPairDescriptor,
        AssetKind::StemDirectory {
            material,
            generation,
        } => {
            if (material.is_some() && !generation)
                || (generation
                    && path.file_name().is_none_or(|name| {
                        !material_paths::ready_generation(&name.to_string_lossy())
                    }))
            {
                return Err(invalid(
                    "only complete published stem generations can be receipted",
                ));
            }
            ArtifactKind::StemDirectory
        }
        _ => return Err(invalid("artifact reference has the wrong type")),
    };
    let original = kind == ArtifactKind::Original;
    let leaf = original || kind == ArtifactKind::StemPairDescriptor;
    guards.extend(project_assets::directory_guards(if leaf {
        path.parent()
            .ok_or_else(|| invalid("original parent missing"))?
    } else {
        &path
    })?);
    let identity = project_assets::capture_identity(&path)?
        .ok_or_else(|| invalid("artifact identity missing"))?;
    let names = if leaf {
        vec![
            path.file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| invalid("original leaf is not Unicode"))?
                .to_owned(),
        ]
    } else {
        let known: &[&str] = match kind {
            ArtifactKind::PcmDirectory => &PCM_NAMES,
            ArtifactKind::StemPcmDirectory => &STEM_PCM_NAMES,
            _ => &STEM_NAMES,
        };
        let entries = fs::read_dir(&path)?
            .take(known.len() + 1)
            .collect::<io::Result<Vec<_>>>()?;
        if entries.len() != known.len()
            || entries.iter().any(|entry| {
                !entry.file_type().is_ok_and(|kind| kind.is_file())
                    || entry
                        .file_name()
                        .to_str()
                        .is_none_or(|name| !known.contains(&name))
            })
        {
            return Err(invalid(
                "artifact directory is incomplete or contains unknown children",
            ));
        }
        known.iter().map(|name| (*name).to_owned()).collect()
    };
    let mut files = Vec::with_capacity(names.len());
    let mut proof = Vec::with_capacity(names.len());
    for name in names {
        let file_path = if leaf { path.clone() } else { path.join(&name) };
        let mut file = sealed_reader(&file_path)?;
        let file_identity = project_assets::file_identity(&file)?;
        let (bytes, sha256) = hash_file(&mut file)?;
        proof.push(VerifiedFile {
            name,
            identity: file_identity,
            bytes,
            sha256,
        });
        files.push(file);
    }
    if leaf && proof[0].identity != identity {
        return Err(invalid("original identity changed during sealing"));
    }
    match kind {
        ArtifactKind::PcmDirectory => verify_pcm_metadata(&path, &mut files, &proof)?,
        ArtifactKind::StemDirectory => verify_stem_metadata(&mut files, &proof)?,
        ArtifactKind::StemPcmDirectory | ArtifactKind::StemPairDescriptor => {
            files.extend(super::stem_pair::verify_recovery_area(&root, &path).map_err(invalid)?);
        }
        ArtifactKind::Original => (),
    }
    let receipt = Receipt {
        schema_version: 1,
        kind,
        samples_identity,
        reference: persisted_reference(&root, &path)?,
        identity,
        files: proof,
    };
    Ok(SealedArtifact {
        receipt,
        root,
        path,
        _files: files,
        _guards: guards,
    })
}

fn parse_receipt(encoded: &str) -> io::Result<Receipt> {
    if encoded.len() > JSON_LIMIT {
        return Err(invalid("receipt exceeds bound"));
    }
    let receipt: Receipt = serde_json::from_str(encoded).map_err(io::Error::other)?;
    if receipt.schema_version != 1
        || receipt.reference.len() > PATH_LIMIT
        || !receipt.reference.starts_with("samples/")
        || receipt.reference.contains('\\')
        || receipt.files.is_empty()
        || receipt.files.len() > 6
        || receipt.files.iter().any(|file| {
            !digest_valid(&file.sha256)
                || file.name.is_empty()
                || file.name.encode_utf16().count() > 255
                || file.name.contains(['/', '\\', ':'])
        })
    {
        return Err(invalid("invalid bounded artifact receipt"));
    }
    Ok(receipt)
}

#[pyclass]
pub struct MigrationArtifactLease {
    sealed: Option<SealedArtifact>,
    pin: Option<ProjectAssetLease>,
    outcome: Option<Arc<project_assets::RetirementOutcome>>,
}

impl MigrationArtifactLease {
    fn capture_native(root: &Path, reference: &Path) -> io::Result<Self> {
        let sealed = seal_artifact(root, reference)?;
        let pin = ProjectAssets::shared().acquire_verified_pin(
            &sealed.root,
            &sealed.path,
            &sealed.receipt.identity,
        )?;
        Ok(Self {
            sealed: Some(sealed),
            pin: Some(pin),
            outcome: None,
        })
    }
    fn reopen_native(root: &Path, encoded: &str) -> io::Result<Self> {
        let receipt = parse_receipt(encoded)?;
        let sealed = seal_artifact(root, Path::new(&receipt.reference))?;
        if sealed.receipt != receipt {
            return Err(invalid(
                "artifact/root identity, complete bytes or type changed; preserved",
            ));
        }
        let pin = ProjectAssets::shared().acquire_verified_pin(
            &sealed.root,
            &sealed.path,
            &receipt.identity,
        )?;
        Ok(Self {
            sealed: Some(sealed),
            pin: Some(pin),
            outcome: None,
        })
    }
    fn retire_native(&mut self, inventory: &MigrationInventoryLease) -> io::Result<()> {
        let sealed = self
            .sealed
            .as_ref()
            .ok_or_else(|| invalid("artifact lease released"))?;
        let handle = inventory
            .handle
            .as_ref()
            .ok_or_else(|| invalid("inventory lease released"))?;
        if handle.root != sealed.root
            || project_assets::capture_identity(&handle.root)?.as_ref()
                != Some(&sealed.receipt.samples_identity)
        {
            return Err(invalid(
                "retirement inventory belongs to another samples root",
            ));
        }
        handle.validate()?;
        let current = seal_artifact(&sealed.root, &sealed.path)?;
        if current.receipt != sealed.receipt {
            return Err(invalid("retirement evidence changed; preserved"));
        }
        let protection = Arc::new(project_assets::RetirementProtection {
            _inventory: handle.clone(),
            outcome: Arc::new(project_assets::RetirementOutcome::default()),
        });
        self.outcome = ProjectAssets::shared().retire_verified_guarded(
            self.pin
                .as_ref()
                .ok_or_else(|| invalid("artifact pin released"))?,
            sealed.receipt.files.clone(),
            Some(protection),
        )?;
        Ok(())
    }
}

#[pymethods]
impl MigrationArtifactLease {
    #[staticmethod]
    pub fn capture(py: Python<'_>, samples_root: String, reference: String) -> PyResult<Self> {
        py.detach(|| Self::capture_native(Path::new(&samples_root), Path::new(&reference)))
            .map_err(py_error)
    }
    #[staticmethod]
    pub fn reopen(py: Python<'_>, samples_root: String, receipt_json: String) -> PyResult<Self> {
        py.detach(|| Self::reopen_native(Path::new(&samples_root), &receipt_json))
            .map_err(py_error)
    }
    pub fn receipt_json(&self) -> PyResult<String> {
        serde_json::to_string(
            &self
                .sealed
                .as_ref()
                .ok_or_else(|| py_error("artifact lease released"))?
                .receipt,
        )
        .map_err(py_error)
    }
    pub fn retire(&mut self, py: Python<'_>, inventory: &MigrationInventoryLease) -> PyResult<()> {
        py.detach(|| self.retire_native(inventory))
            .map_err(py_error)
    }
    pub fn retirement_status(&self) -> PyResult<&'static str> {
        self.outcome.as_ref().map_or(Ok("idle"), |outcome| {
            outcome.status().map(|value| value.0).map_err(py_error)
        })
    }
    pub fn retirement_error(&self) -> PyResult<Option<String>> {
        self.outcome.as_ref().map_or(Ok(None), |outcome| {
            outcome.status().map(|value| value.1).map_err(py_error)
        })
    }
    pub fn release(&mut self) {
        self.sealed = None;
        self.pin = None;
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectRecord {
    schema_version: u32,
    instance_id: String,
    config_reference: String,
}

fn ordinary_config(root: &Path, reference: &Path) -> io::Result<PathBuf> {
    if !reference.is_absolute()
        || reference.to_string_lossy().len() > PATH_LIMIT
        || reference
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || reference
            .to_string_lossy()
            .split(['/', '\\'])
            .any(|part| [".", ".."].contains(&part))
    {
        return Err(invalid(
            "config reference must be an ordinary absolute project path",
        ));
    }
    project_assets::reject_links(reference)?;
    let project = root
        .parent()
        .ok_or_else(|| invalid("samples project parent missing"))?;
    let path = project_assets::canonical_with_missing(reference)?;
    if path == project || !path.starts_with(project)
        || reference.components().any(|part| matches!(part, Component::Normal(name) if name.to_str().is_none_or(|name| !material_paths::filename(name))))
        || path.exists() && !path.is_file() {
        return Err(invalid("config reference escaped project root or is not an ordinary file"));
    }
    Ok(path)
}

fn plain_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(unc) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        path.strip_prefix("\\\\?\\").unwrap_or(&path).to_owned()
    }
}

fn fresh_id() -> io::Result<String> {
    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(
                algorithm: *mut std::ffi::c_void,
                buffer: *mut u8,
                length: u32,
                flags: u32,
            ) -> i32;
        }
        let mut bytes = [0_u8; 16];
        // SAFETY: Windows fills this initialized fixed buffer using its system RNG.
        if unsafe { BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), 16, 2) } != 0 {
            return Err(invalid("project instance RNG failed"));
        }
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
    #[cfg(not(windows))]
    {
        Err(invalid(
            "project inventory requires Windows share-mode locking",
        ))
    }
}

fn inventory_gate(root: &Path) -> io::Result<MigrationInventoryLease> {
    let (root, mut guards) = samples_root(root)?;
    let path = root.join(".migration-projects");
    match fs::create_dir(&path) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    project_assets::reject_links(&path)?;
    guards.extend(project_assets::directory_guards(&path)?);
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let lock_path = path.join(".inventory.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(0x0020_0000)
            .open(&lock_path)?;
        let metadata = lock.metadata()?;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 || metadata.len() != 0 {
            return Err(invalid("unknown inventory lock object; preserved"));
        }
        let identity = project_assets::file_identity(&lock)?;
        Ok(MigrationInventoryLease {
            handle: Some(Arc::new(InventoryHandle {
                root,
                path,
                lock,
                identity,
                _guards: guards,
            })),
        })
    }
    #[cfg(not(windows))]
    {
        let _ = guards;
        Err(invalid(
            "project inventory requires Windows share-mode locking",
        ))
    }
}

#[pyclass]
pub struct MigrationInventoryLease {
    handle: Option<Arc<InventoryHandle>>,
}

pub(super) struct InventoryHandle {
    root: PathBuf,
    path: PathBuf,
    lock: File,
    identity: FileIdentity,
    _guards: Vec<File>,
}

impl InventoryHandle {
    fn validate(&self) -> io::Result<()> {
        if project_assets::file_identity(&self.lock)? != self.identity {
            return Err(invalid("inventory lock was replaced"));
        }
        Ok(())
    }
    fn records_native(&self) -> io::Result<Vec<String>> {
        // The exclusive no-share handle plus guarded ancestors prevent pathname ABA.
        self.validate()?;
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.path)?.take(RECORD_LIMIT + 2) {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".inventory.lock" {
                continue;
            }
            if records.len() >= RECORD_LIMIT {
                return Err(invalid("project inventory exceeds 256 records; preserved"));
            }
            let id = name.strip_suffix(".json").unwrap_or(&name);
            let result = self.record(&entry.path(), id);
            let value = match result {
                Ok((record, live)) => {
                    json!({"schema_version":1,"instance_id":record.instance_id,"config_reference":record.config_reference,"live":live,"error":null})
                }
                Err(error) => {
                    json!({"schema_version":1,"instance_id":id,"config_reference":null,"live":null,"error":error.to_string()})
                }
            };
            records.push(serde_json::to_string(&value).map_err(io::Error::other)?);
        }
        records.sort();
        Ok(records)
    }
    fn record(&self, path: &Path, id: &str) -> io::Result<(ProjectRecord, bool)> {
        if !material_paths::valid_id(id)
            || path
                .file_name()
                .is_none_or(|name| name != format!("{id}.json").as_str())
        {
            return Err(invalid("unknown project inventory child; preserved"));
        }
        project_assets::reject_links(path)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let (mut file, live) = match OpenOptions::new()
                .read(true)
                .share_mode(0)
                .custom_flags(0x0020_0000)
                .open(path)
            {
                Ok(file) => (file, false),
                Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => {
                    (sealed_reader(path)?, true)
                }
                Err(error) => return Err(error),
            };
            if !file.metadata()?.is_file() || file.metadata()?.len() > JSON_LIMIT as u64 {
                return Err(invalid("project record is not bounded ordinary JSON"));
            }
            let record: ProjectRecord =
                serde_json::from_slice(&json_bytes(&mut file)?).map_err(io::Error::other)?;
            if record.schema_version != 1
                || record.instance_id != id
                || plain_path(&ordinary_config(
                    &self.root,
                    Path::new(&record.config_reference),
                )?) != record.config_reference
            {
                return Err(invalid("project inventory binding is invalid"));
            }
            Ok((record, live))
        }
        #[cfg(not(windows))]
        {
            Err(invalid(
                "project inventory requires Windows share-mode locking",
            ))
        }
    }
}

impl MigrationInventoryLease {
    fn records_native(&self) -> io::Result<Vec<String>> {
        self.handle
            .as_ref()
            .ok_or_else(|| invalid("inventory lease released"))?
            .records_native()
    }
}

#[pymethods]
impl MigrationInventoryLease {
    pub fn records(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        py.detach(|| self.records_native()).map_err(py_error)
    }
    pub fn release(&mut self) {
        self.handle = None;
    }
}

#[pyclass]
pub struct MigrationProjectGuard {
    root: PathBuf,
    instance: String,
    reader: Option<File>,
    guards: Vec<File>,
}

impl MigrationProjectGuard {
    fn create_native(root: &Path, config: &Path) -> io::Result<Self> {
        Self::create_at(root, config, &|_| Ok(()))
    }

    fn create_at(
        root: &Path,
        config: &Path,
        checkpoint: &impl Fn(&str) -> io::Result<()>,
    ) -> io::Result<Self> {
        let (root, mut guards) = samples_root(root)?;
        guards.extend(project_assets::directory_guards(
            config
                .parent()
                .ok_or_else(|| invalid("config parent missing"))?,
        )?);
        let config = ordinary_config(&root, config)?;
        let gate = inventory_gate(&root)?;
        let gate_handle = gate.handle.as_ref().expect("fresh inventory gate");
        // Unknown/error records also consume admission capacity; they are never deleted.
        let inventory = gate.records_native()?;
        let mut obsolete: Vec<(PathBuf, File, VerifiedFile)> = Vec::new();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            for status in &inventory {
                let status: Value = serde_json::from_str(status).map_err(io::Error::other)?;
                if status["live"] != false
                    || !status["error"].is_null()
                    || status["config_reference"].as_str() != Some(plain_path(&config).as_str())
                {
                    continue;
                }
                let id = status["instance_id"]
                    .as_str()
                    .ok_or_else(|| invalid("inactive instance ID missing"))?;
                let path = gate_handle.path.join(format!("{id}.json"));
                let mut reader = OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .custom_flags(0x0020_0000)
                    .open(&path)?;
                let record: ProjectRecord =
                    serde_json::from_slice(&json_bytes(&mut reader)?).map_err(io::Error::other)?;
                if record.schema_version != 1
                    || record.instance_id != id
                    || record.config_reference != plain_path(&config)
                {
                    return Err(invalid(
                        "inactive project binding changed before compaction",
                    ));
                }
                let (bytes, sha256) = hash_file(&mut reader)?;
                let proof = VerifiedFile {
                    name: format!("{id}.json"),
                    identity: project_assets::file_identity(&reader)?,
                    bytes,
                    sha256,
                };
                obsolete.push((path, reader, proof));
            }
        }
        if inventory.len().saturating_sub(obsolete.len()) >= RECORD_LIMIT {
            return Err(invalid("project inventory full (256 records)"));
        }
        checkpoint("before_new_record")?;
        let instance = fresh_id()?;
        let path = gate_handle.path.join(format!("{instance}.json"));
        let record = ProjectRecord {
            schema_version: 1,
            instance_id: instance.clone(),
            config_reference: plain_path(&config),
        };
        let bytes = serde_json::to_vec(&record).map_err(io::Error::other)?;
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        writer.write_all(&bytes)?;
        writer.sync_all()?;
        let identity = project_assets::file_identity(&writer)?;
        drop(writer);
        let mut reader = sealed_reader(&path)?;
        if project_assets::file_identity(&reader)? != identity
            || json_file(&mut reader)? != serde_json::to_value(&record).map_err(io::Error::other)?
        {
            return Err(invalid(
                "new project record changed before lock acquisition",
            ));
        }
        checkpoint("new_record_sealed")?;
        // A fresh complete binding exists before exact recognized inactive duplicates
        // are removed. Failure leaves both bindings visible; foreign/unknown/live
        // records never enter this list.
        for (path, previous_reader, proof) in obsolete {
            drop(previous_reader);
            project_assets::remove_verified_file(&path, &proof)?;
        }
        guards.extend(project_assets::directory_guards(&gate_handle.path)?);
        drop(gate);
        Ok(Self {
            root,
            instance,
            reader: Some(reader),
            guards,
        })
    }
}

#[pymethods]
impl MigrationProjectGuard {
    #[new]
    pub fn new(py: Python<'_>, samples_root: String, config_reference: String) -> PyResult<Self> {
        py.detach(|| Self::create_native(Path::new(&samples_root), Path::new(&config_reference)))
            .map_err(py_error)
    }
    #[getter]
    pub fn instance_id(&self) -> String {
        self.instance.clone()
    }
    pub fn lock_inventory(&self, py: Python<'_>) -> PyResult<MigrationInventoryLease> {
        if self.reader.is_none() {
            return Err(py_error("project guard released"));
        }
        py.detach(|| inventory_gate(&self.root)).map_err(py_error)
    }
    #[staticmethod]
    pub fn inventory(py: Python<'_>, samples_root: String) -> PyResult<Vec<String>> {
        py.detach(|| inventory_gate(Path::new(&samples_root))?.records_native())
            .map_err(py_error)
    }
    pub fn release(&mut self) {
        self.reader = None;
        self.guards.clear();
    }
}

#[cfg(test)]
#[path = "material_migration_recovery_tests.rs"]
mod tests;
