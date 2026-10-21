//! Immutable out-of-line map data. No selected-map, audio or project authority.

use super::cold_store::sealed_reader;
use super::material_paths::{self, AssetKind};
use super::project_assets::{self, FileIdentity, ProjectAssetLease, ProjectAssets, VerifiedFile};
use super::source_beatmap::{MAX_RECORD_BYTES, SourceBeatmap};
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub(super) const MAX_RECEIPT_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    kind: String,
    reference: String,
    bytes: u64,
    sha256: String,
    map_revision: String,
    decoder_identity: String,
}

fn digest_text(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl Receipt {
    fn parse(encoded: &str) -> Result<Self, String> {
        if encoded.len() > MAX_RECEIPT_BYTES {
            return Err("source beatmap receipt exceeds byte limit".into());
        }
        let receipt: Self = serde_json::from_str(encoded).map_err(|error| error.to_string())?;
        receipt.validate()?;
        Ok(receipt)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.kind != "source-beatmap-v1"
            || self.bytes == 0
            || self.bytes > MAX_RECORD_BYTES as u64
            || !digest_text(&self.sha256)
            || !digest_text(&self.map_revision)
            || !digest_text(&self.decoder_identity)
        {
            return Err("source beatmap receipt has unsupported fields".into());
        }
        if self.reference.len() > 4096
            || !self.reference.starts_with("samples/")
            || self.reference.contains('\\')
            || self
                .reference
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || !matches!(
                material_paths::classify(Path::new(&self.reference)),
                Ok(AssetKind::SourceBeatmapArtifact { .. })
            )
        {
            return Err("source beatmap receipt requires a normalized typed reference".into());
        }
        Ok(())
    }
}

/// Per-call observation/failure seam; production uses the same path with a no-op.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StorePoint {
    AfterBirth,
    AfterWrite,
    BeforeRename,
    AfterRename,
}

/// A new generation is rollback-owned until the genuine artifact is resolved.
struct Birth {
    root: PathBuf,
    path: PathBuf,
    identity: FileIdentity,
    file_identity: Option<FileIdentity>,
    proof: Option<VerifiedFile>,
    guards: Vec<File>,
    durable: bool,
    pin: Option<ProjectAssetLease>,
}

impl Drop for Birth {
    fn drop(&mut self) {
        self.guards.clear();
        if self.durable {
            return;
        }
        // Cleanup is conservative and exact: never recursively remove an unknown
        // child, replacement, changed published leaf or reparse target.
        let _ = ProjectAssets::shared().retire_source_beatmap_birth(
            &self.root,
            &self.path,
            self.pin.as_ref(),
            || {
                let _parents = project_assets::directory_guards(
                    self.path
                        .parent()
                        .ok_or_else(|| std::io::Error::other("map birth parent missing"))?,
                )?;
                let guards = project_assets::directory_guards(&self.path)?;
                if guards
                    .first()
                    .map(project_assets::file_identity)
                    .transpose()?
                    .as_ref()
                    != Some(&self.identity)
                {
                    return Err(std::io::Error::other("map birth replaced; preserved"));
                }
                let entries = fs::read_dir(&self.path)?
                    .take(2)
                    .collect::<std::io::Result<Vec<_>>>()?;
                if entries.iter().any(|entry| {
                    entry.file_name() != "map.json"
                        || !entry.file_type().is_ok_and(|kind| kind.is_file())
                }) || entries.len() > 1
                {
                    return Err(std::io::Error::other(
                        "map birth contains unknown children; preserved",
                    ));
                }
                if !entries.is_empty() {
                    let path = self.path.join("map.json");
                    match (&self.proof, &self.file_identity) {
                        (Some(proof), _) => project_assets::remove_verified_file(&path, proof)?,
                        (_, Some(identity)) => {
                            project_assets::remove_owned_file(&path, Some(identity))?
                        }
                        _ => {
                            return Err(std::io::Error::other("map birth leaf unowned; preserved"));
                        }
                    }
                }
                drop(guards);
                project_assets::remove_empty_directory(&self.path, &self.identity)
            },
        );
    }
}

struct Resources {
    map: Option<SourceBeatmap>,
    file: Option<File>,
    guards: Vec<File>,
    pin: Option<ProjectAssetLease>,
    birth: Option<Birth>,
}

impl Drop for Resources {
    fn drop(&mut self) {
        // Release actual readers before exact rollback can acquire DELETE access.
        drop(self.file.take());
        self.guards.clear();
        drop(self.map.take());
        drop(self.birth.take());
        // The existing owner remains observable until actual cleanup finishes.
        drop(self.pin.take());
    }
}

struct ArtifactInner {
    receipt: Receipt,
    encoded_receipt: String,
    resources: Option<Resources>,
    acknowledged: AtomicBool,
}

impl Drop for ArtifactInner {
    fn drop(&mut self) {
        let Some(mut resources) = self.resources.take() else {
            return;
        };
        if let Some(birth) = resources.birth.as_mut() {
            birth.durable = self.acknowledged.load(Ordering::Acquire);
        }
        if super::cold_jobs::is_worker_thread() {
            // Failure/panic cleanup stays on the same running worker before its
            // original reservation retires. Output owns no lane admission.
            drop(resources);
        } else {
            // The accepted output may outlive its worker and engine. Final UI
            // release transfers its actual readers and rollback work off UI.
            let retained = Arc::new(Mutex::new(Some(resources)));
            let work = retained.clone();
            if std::thread::Builder::new()
                .name("source-map-artifact-retirement".into())
                .spawn(move || {
                    drop(
                        work.lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .take(),
                    )
                })
                .is_err()
            {
                // If the OS cannot create a retirement thread, preserve the
                // bounded owner rather than perform file/hash work on the UI.
                std::mem::forget(retained);
            }
        }
    }
}

/// Genuine immutable worker output. Construction and durable ACK are private.
#[pyclass(frozen)]
#[derive(Clone)]
pub struct SourceBeatmapArtifact {
    inner: Arc<ArtifactInner>,
}

impl std::fmt::Debug for SourceBeatmapArtifact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceBeatmapArtifact")
            .field("reference", &self.inner.receipt.reference)
            .finish_non_exhaustive()
    }
}

#[pymethods]
impl SourceBeatmapArtifact {
    pub fn receipt_json(&self) -> &str {
        &self.inner.encoded_receipt
    }
    pub fn reference(&self) -> &str {
        &self.inner.receipt.reference
    }
    pub fn map(&self) -> SourceBeatmap {
        self.inner
            .resources
            .as_ref()
            .expect("live artifact owns custody")
            .map
            .as_ref()
            .expect("live artifact owns map")
            .clone()
    }
}

impl SourceBeatmapArtifact {
    pub(super) fn acknowledge_export(&self) {
        self.inner.acknowledged.store(true, Ordering::Release);
    }

    fn from_parts(receipt: Receipt, resources: Resources) -> Result<Self, String> {
        receipt.validate()?;
        let encoded_receipt = serde_json::to_string(&receipt).map_err(|error| error.to_string())?;
        Ok(Self {
            inner: Arc::new(ArtifactInner {
                receipt,
                encoded_receipt,
                resources: Some(resources),
                acknowledged: AtomicBool::new(false),
            }),
        })
    }

    pub(super) fn restore(
        root: &Path,
        encoded_receipt: &str,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self, String> {
        check_cancelled(cancelled)?;
        let receipt = Receipt::parse(encoded_receipt)?;
        let checked =
            material_paths::resolve(root, Path::new(&receipt.reference)).map_err(error)?;
        if !matches!(checked.kind, AssetKind::SourceBeatmapArtifact { .. }) {
            return Err("source beatmap artifact has wrong type".into());
        }
        let directory = checked
            .path
            .parent()
            .ok_or("source beatmap directory missing")?;
        let guards = project_assets::directory_guards(directory).map_err(error)?;
        require_only_map(directory)?;
        let mut file = sealed_reader(&checked.path).map_err(error)?;
        let identity = project_assets::file_identity(&file).map_err(error)?;
        let pin = ProjectAssets::shared()
            .acquire_verified_pin(root, &checked.path, &identity)
            .map_err(error)?;
        let map = read_map(&mut file, &receipt, cancelled)?;
        Self::from_parts(
            receipt,
            Resources {
                map: Some(map),
                file: Some(file),
                guards,
                pin: Some(pin),
                birth: None,
            },
        )
    }

    pub(super) fn store(
        root: &Path,
        original: &Path,
        map: SourceBeatmap,
        cancelled: &impl Fn() -> bool,
        observe: &mut impl FnMut(StorePoint, &Path) -> Result<(), String>,
    ) -> Result<Self, String> {
        check_cancelled(cancelled)?;
        let material = material_paths::material_root_for_original(root, original).map_err(error)?;
        let mut guards = project_assets::directory_guards(&material).map_err(error)?;
        let mut base = material;
        for name in [".pcm-cache", "source-beatmaps", "v1"] {
            base.push(name);
            match fs::create_dir(&base) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(value) => return Err(error(value)),
            }
            guards.extend(project_assets::directory_guards(&base).map_err(error)?);
        }
        let generation = generation_id()?;
        let staging = base.join(format!(".generation-{generation}"));
        let ready = base.join(format!(".ready-{generation}"));
        fs::create_dir(&staging).map_err(error)?;
        let staging_guards = project_assets::directory_guards(&staging).map_err(error)?;
        let identity =
            project_assets::file_identity(staging_guards.first().ok_or("map birth guard missing")?)
                .map_err(error)?;
        let mut birth = Birth {
            root: root.to_owned(),
            path: staging.clone(),
            identity,
            file_identity: None,
            proof: None,
            guards: staging_guards,
            durable: false,
            pin: None,
        };
        observe(StorePoint::AfterBirth, &staging)?;
        let canonical = map.canonical_record().as_bytes();
        let receipt = Receipt {
            schema_version: 1,
            kind: "source-beatmap-v1".into(),
            reference: reference(root, &ready.join("map.json"))?,
            bytes: canonical.len() as u64,
            sha256: format!("{:x}", Sha256::digest(canonical)),
            map_revision: map.map_revision().into(),
            decoder_identity: map.source_identity().into(),
        };
        let leaf = staging.join("map.json");
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&leaf)
            .map_err(error)?;
        let file_identity = project_assets::file_identity(&writer).map_err(error)?;
        birth.file_identity = Some(file_identity.clone());
        for chunk in canonical.chunks(64 * 1024) {
            check_cancelled(cancelled)?;
            writer.write_all(chunk).map_err(error)?;
        }
        writer.sync_all().map_err(error)?;
        drop(writer);
        birth.proof = Some(VerifiedFile {
            name: "map.json".into(),
            identity: file_identity.clone(),
            bytes: receipt.bytes,
            sha256: receipt.sha256.clone(),
        });
        observe(StorePoint::AfterWrite, &staging)?;
        {
            require_only_map(&staging)?;
            let mut reader = sealed_reader(&leaf).map_err(error)?;
            if project_assets::file_identity(&reader).map_err(error)? != file_identity {
                return Err("source beatmap staging leaf identity changed".into());
            }
            drop(read_map(&mut reader, &receipt, cancelled)?);
        }
        observe(StorePoint::BeforeRename, &staging)?;
        check_cancelled(cancelled)?;
        birth.guards.clear();
        // Generation collision is never overwritten or reused.
        if ready.exists() {
            return Err("source beatmap ready collision; preserved".into());
        }
        fs::rename(&staging, &ready).map_err(error)?;
        birth.path = ready.clone();
        birth.guards = project_assets::directory_guards(&ready).map_err(error)?;
        if project_assets::file_identity(birth.guards.first().ok_or("ready map guard missing")?)
            .map_err(error)?
            != birth.identity
        {
            return Err("source beatmap ready directory identity changed".into());
        }
        observe(StorePoint::AfterRename, &ready)?;
        require_only_map(&ready)?;
        let path = ready.join("map.json");
        let mut file = sealed_reader(&path).map_err(error)?;
        if project_assets::file_identity(&file).map_err(error)? != file_identity {
            return Err("source beatmap ready leaf identity changed".into());
        }
        // Reopen/hash/parse qualifies the artifact, not a second original scan.
        let reopened = read_map(&mut file, &receipt, cancelled)?;
        let pin = ProjectAssets::shared()
            .acquire_verified_pin(root, &path, &file_identity)
            .map_err(error)?;
        birth.pin = Some(pin.clone());
        Self::from_parts(
            receipt,
            Resources {
                map: Some(reopened),
                file: Some(file),
                guards,
                pin: Some(pin),
                birth: Some(birth),
            },
        )
    }
}

fn check_cancelled(cancelled: &impl Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("source beatmap artifact cancelled".into())
    } else {
        Ok(())
    }
}

fn require_only_map(path: &Path) -> Result<(), String> {
    let entries = fs::read_dir(path)
        .map_err(error)?
        .take(2)
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(error)?;
    if entries.len() != 1
        || entries[0].file_name() != "map.json"
        || !entries[0].file_type().map_err(error)?.is_file()
    {
        return Err("source beatmap directory is incomplete or has unknown children".into());
    }
    Ok(())
}

fn read_map(
    file: &mut File,
    receipt: &Receipt,
    cancelled: &impl Fn() -> bool,
) -> Result<SourceBeatmap, String> {
    if file.metadata().map_err(error)?.len() != receipt.bytes {
        return Err("source beatmap artifact extent differs from receipt".into());
    }
    let mut encoded = Vec::with_capacity(receipt.bytes as usize);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        check_cancelled(cancelled)?;
        let count = file.read(&mut buffer).map_err(error)?;
        if count == 0 {
            break;
        }
        if encoded
            .len()
            .checked_add(count)
            .is_none_or(|bytes| bytes > receipt.bytes as usize)
        {
            return Err("source beatmap artifact exceeds receipt extent".into());
        }
        encoded.extend_from_slice(&buffer[..count]);
        digest.update(&buffer[..count]);
    }
    if encoded.len() as u64 != receipt.bytes || format!("{:x}", digest.finalize()) != receipt.sha256
    {
        return Err("source beatmap artifact hash differs from receipt".into());
    }
    let text = std::str::from_utf8(&encoded).map_err(error)?;
    let map = SourceBeatmap::parse(text)?;
    if map.map_revision() != receipt.map_revision
        || map.source_identity() != receipt.decoder_identity
        || map.canonical_record().as_bytes() != encoded
    {
        return Err("source beatmap artifact differs from canonical map receipt".into());
    }
    check_cancelled(cancelled)?;
    Ok(map)
}

fn reference(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path.strip_prefix(root).map_err(error)?;
    Ok(format!(
        "samples/{}",
        relative.to_string_lossy().replace('\\', "/")
    ))
}

fn generation_id() -> Result<String, String> {
    static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
    let serial = NEXT_GENERATION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| "source beatmap generation counter exhausted")?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(error)?
        .as_nanos();
    let digest = Sha256::digest(
        format!("source-beatmap-v1:{}:{nanos}:{serial}", std::process::id()).as_bytes(),
    );
    // Names supply no identity authority. Exclusive staging creation and ready
    // collision refusal remain mandatory even across processes or clock reuse.
    Ok(format!("{digest:x}")[..32].to_owned())
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}
