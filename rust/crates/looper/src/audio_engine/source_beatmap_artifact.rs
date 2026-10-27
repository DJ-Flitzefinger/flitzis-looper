//! Immutable out-of-line map data. No selected-map, audio or project authority.

use super::cold_store::sealed_reader;
use super::material_paths::{self, AssetKind};
use super::project_assets::{self, FileIdentity, ProjectAssetLease, ProjectAssets, VerifiedFile};
use super::source_beatmap::{MAX_RECORD_BYTES, SourceBeatmap};
use super::source_beatmap_capture::RawCapture;
use super::source_beatmap_raw::RawReviewLease;
use super::source_beatmap_raw_data::MAX_RAW_BYTES;
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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

/// Compact historical metadata only. Current source qualification stays on the worker.
pub(super) fn validate_intent_value(value: &serde_json::Value) -> Result<(), String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Intent {
        schema_version: u32,
        revision: u64,
        selected: Option<Receipt>,
        #[serde(default)]
        automatic: bool,
    }
    let object = value
        .as_object()
        .ok_or("source beatmap intent must be an object")?;
    if !(object.len() == 3 || (object.len() == 4 && object.contains_key("automatic")))
        || !["schema_version", "revision", "selected"]
            .iter()
            .all(|key| object.contains_key(*key))
    {
        return Err("source beatmap intent must contain its exact complete schema".into());
    }
    let intent: Intent = serde_json::from_value(value.clone()).map_err(error)?;
    if intent.schema_version != 1 {
        return Err("unsupported source beatmap intent version".into());
    }
    let _ = intent.revision;
    if intent.automatic && intent.selected.is_none() {
        return Err("automatic map provenance requires a selected receipt".into());
    }
    if let Some(receipt) = intent.selected {
        let encoded = serde_json::to_string(&receipt).map_err(error)?;
        Receipt::parse(&encoded)?;
    }
    Ok(())
}

pub(super) fn receipt_evidence(encoded: &str) -> Result<(String, u64, String, String), String> {
    let receipt = Receipt::parse(encoded)?;
    let canonical = serde_json::to_string(&receipt).map_err(error)?;
    Ok((receipt.reference, receipt.bytes, receipt.sha256, canonical))
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
    slot: Option<project_assets::MapRetirementSlot>,
}

impl Drop for Birth {
    fn drop(&mut self) {
        self.guards.clear();
        if let Some(slot) = self.slot.take() {
            slot.queue(
                self.root.clone(),
                self.path.clone(),
                self.identity.clone(),
                (self.file_identity.clone(), self.proof.clone()),
                !self.durable,
                Vec::new(),
            );
        }
    }
}

struct Resources {
    map: Option<SourceBeatmap>,
    raw: Option<Arc<RawCapture>>,
    file: Option<File>,
    guards: Vec<File>,
    pin: Option<ProjectAssetLease>,
    birth: Option<Birth>,
    release: Option<(
        project_assets::MapRetirementSlot,
        PathBuf,
        PathBuf,
        FileIdentity,
    )>,
    raw_review: Option<RawReviewLease>,
}

impl Drop for Resources {
    fn drop(&mut self) {
        let mut readers = project_assets::DeferredMapReaders {
            _map: self.map.take(),
            _raw: self.raw.take(),
            _file: self.file.take(),
            _guards: std::mem::take(&mut self.guards),
            _pin: self.pin.take(),
            _birth_guards: Vec::new(),
            _birth_pin: None,
            _raw_review: self.raw_review.take(),
        };
        if let Some(mut birth) = self.birth.take() {
            // The birth's duplicate sealed handles/pin must also retire before
            // the shared raw lease. Never let queue processing race their Drop.
            readers._birth_guards = std::mem::take(&mut birth.guards);
            readers._birth_pin = birth.pin.take();
            let slot = birth
                .slot
                .take()
                .expect("map birth reserved cleanup before creation");
            slot.queue(
                birth.root.clone(),
                birth.path.clone(),
                birth.identity.clone(),
                (birth.file_identity.clone(), birth.proof.clone()),
                !birth.durable,
                vec![readers],
            );
        } else if let Some((slot, root, path, identity)) = self.release.take() {
            slot.queue(root, path, identity, (None, None), false, vec![readers]);
        }
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
        // The same pre-reserved asset queue releases actual readers/geometry and
        // retries exact rollback off UI, including after worker/engine shutdown.
        drop(resources);
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
    /// Return an unselected new export to its original pre-reserved cleanup owner.
    /// Restored artifacts have no birth deletion rights and remain unchanged.
    pub fn retire_unselected(&self) {
        if self
            .inner
            .resources
            .as_ref()
            .is_some_and(|resources| resources.birth.is_some())
        {
            self.inner.acknowledged.store(false, Ordering::Release);
        }
    }

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

    pub(super) fn restore_with_context(
        root: &Path,
        encoded_receipt: &str,
        context: (usize, usize),
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self, String> {
        check_cancelled(cancelled)?;
        let root = fs::canonicalize(root).map_err(error)?;
        let root = root.as_path();
        let receipt = Receipt::parse(encoded_receipt)?;
        let release_slot = ProjectAssets::shared()
            .reserve_map_retirement()
            .map_err(error)?;
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
        let directory_identity =
            project_assets::file_identity(guards.first().ok_or("map directory guard missing")?)
                .map_err(error)?;
        require_only_map(directory)?;
        let mut file = sealed_reader(&checked.path).map_err(error)?;
        let identity = project_assets::file_identity(&file).map_err(error)?;
        let pin = ProjectAssets::shared()
            .acquire_verified_pin(root, &checked.path, &identity)
            .map_err(error)?;
        let map = read_map(&mut file, &receipt, context, cancelled)?;
        Self::from_parts(
            receipt,
            Resources {
                map: Some(map),
                raw: None,
                file: Some(file),
                guards,
                pin: Some(pin),
                birth: None,
                release: Some((
                    release_slot,
                    root.to_owned(),
                    directory.to_owned(),
                    directory_identity,
                )),
                raw_review: None,
            },
        )
    }

    pub(super) fn store_with_context(
        root: &Path,
        original: &Path,
        map: SourceBeatmap,
        context: (usize, usize),
        cancelled: &impl Fn() -> bool,
        observe: &mut impl FnMut(StorePoint, &Path) -> Result<(), String>,
    ) -> Result<Self, String> {
        let (receipt, resources) = store_payload(
            root,
            original,
            ArtifactPayload::Accepted(map),
            None,
            context,
            cancelled,
            observe,
        )?;
        Self::from_parts(receipt, resources)
    }
}

/// A raw birth never acquires durable/export acknowledgement. Its final reader
/// uses precisely the accepted artifact's pre-reserved off-UI retirement owner.
struct RawArtifactInner {
    resources: Resources,
}

#[derive(Clone)]
pub(crate) struct SourceBeatmapRawArtifact {
    inner: Arc<RawArtifactInner>,
}

impl std::fmt::Debug for SourceBeatmapRawArtifact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceBeatmapRawArtifact")
            .field("raw_revision", &self.raw().revision())
            .finish_non_exhaustive()
    }
}

impl SourceBeatmapRawArtifact {
    pub(super) fn raw(&self) -> &RawCapture {
        self.inner
            .resources
            .raw
            .as_ref()
            .expect("raw artifact owns its immutable capture")
    }

    pub(super) fn store(
        root: &Path,
        original: &Path,
        raw: RawCapture,
        lease: RawReviewLease,
        cancelled: &impl Fn() -> bool,
        observe: &mut impl FnMut(StorePoint, &Path) -> Result<(), String>,
    ) -> Result<Self, String> {
        let (_, resources) = store_payload(
            root,
            original,
            ArtifactPayload::Raw(Arc::new(raw)),
            Some(lease),
            (0, 0),
            cancelled,
            observe,
        )?;
        Ok(Self {
            inner: Arc::new(RawArtifactInner { resources }),
        })
    }
}

enum ArtifactPayload {
    Accepted(SourceBeatmap),
    Raw(Arc<RawCapture>),
}

impl ArtifactPayload {
    fn encoded(&self) -> Result<std::borrow::Cow<'_, [u8]>, String> {
        match self {
            Self::Accepted(map) => Ok(std::borrow::Cow::Borrowed(
                map.canonical_record().as_bytes(),
            )),
            Self::Raw(raw) => raw.encode().map(std::borrow::Cow::Owned),
        }
    }

    fn read(
        &self,
        file: &mut File,
        receipt: &Receipt,
        context: (usize, usize),
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self, String> {
        match self {
            Self::Accepted(map) => {
                let retained =
                    super::source_beatmap::budget::sum(&[context.1, map.retained_bytes()?])?;
                read_map(file, receipt, (context.0, retained), cancelled).map(Self::Accepted)
            }
            Self::Raw(raw) => {
                if receipt.kind != raw.artifact_kind() || receipt.bytes > MAX_RAW_BYTES as u64 {
                    return Err("unsupported raw artifact encoding or extent".into());
                }
                let bytes = read_verified_bytes(file, receipt.bytes, &receipt.sha256, cancelled)?;
                let reopened = RawCapture::decode(&bytes)?;
                if reopened.artifact_kind() != raw.artifact_kind()
                    || reopened.revision() != raw.revision()
                    || reopened.decoder_identity() != raw.decoder_identity()
                {
                    return Err(
                        "raw artifact revision or decoder differs from actual capture".into(),
                    );
                }
                Ok(Self::Raw(Arc::new(reopened)))
            }
        }
    }
}

fn store_payload(
    root: &Path,
    original: &Path,
    payload: ArtifactPayload,
    raw_review: Option<RawReviewLease>,
    context: (usize, usize),
    cancelled: &impl Fn() -> bool,
    observe: &mut impl FnMut(StorePoint, &Path) -> Result<(), String>,
) -> Result<(Receipt, Resources), String> {
    check_cancelled(cancelled)?;
    let root = fs::canonicalize(root).map_err(error)?;
    let root = root.as_path();
    // Capacity belongs to the existing asset queue before any generation birth.
    let slot = ProjectAssets::shared()
        .reserve_map_retirement()
        .map_err(error)?;
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
        slot: Some(slot),
    };
    observe(StorePoint::AfterBirth, &staging)?;
    let canonical = payload.encoded()?;
    let (kind, revision, decoder_identity) = match &payload {
        ArtifactPayload::Accepted(map) => (
            "source-beatmap-v1",
            map.map_revision(),
            map.source_identity(),
        ),
        ArtifactPayload::Raw(raw) => (raw.artifact_kind(), raw.revision(), raw.decoder_identity()),
    };
    let receipt = Receipt {
        schema_version: 1,
        kind: kind.into(),
        reference: reference(root, &ready.join("map.json"))?,
        bytes: canonical.len() as u64,
        sha256: format!("{:x}", Sha256::digest(canonical.as_ref())),
        // This struct stays private for Raw and is never accepted by
        // Receipt::parse/validate or exposed as an accepted map receipt.
        map_revision: revision.into(),
        decoder_identity: decoder_identity.into(),
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
        drop(payload.read(&mut reader, &receipt, context, cancelled)?);
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
    let reopened = payload.read(&mut file, &receipt, context, cancelled)?;
    let pin = ProjectAssets::shared()
        .acquire_verified_pin(root, &path, &file_identity)
        .map_err(error)?;
    birth.pin = Some(pin.clone());
    let (map, raw) = match reopened {
        ArtifactPayload::Accepted(map) => (Some(map), None),
        ArtifactPayload::Raw(raw) => (None, Some(raw)),
    };
    Ok((
        receipt,
        Resources {
            map,
            raw,
            file: Some(file),
            guards,
            pin: Some(pin),
            birth: Some(birth),
            release: None,
            raw_review,
        },
    ))
}

fn read_verified_bytes(
    file: &mut File,
    bytes: u64,
    sha256: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<u8>, String> {
    if file.metadata().map_err(error)?.len() != bytes || bytes > MAX_RAW_BYTES as u64 {
        return Err("raw artifact extent differs from its private capture".into());
    }
    let mut encoded = Vec::new();
    encoded.try_reserve_exact(bytes as usize).map_err(error)?;
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
            .is_none_or(|length| length > bytes as usize)
        {
            return Err("raw artifact exceeds private extent".into());
        }
        encoded.extend_from_slice(&buffer[..count]);
        digest.update(&buffer[..count]);
    }
    if encoded.len() as u64 != bytes || format!("{:x}", digest.finalize()) != sha256 {
        return Err("raw artifact bytes differ from actual producer".into());
    }
    Ok(encoded)
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
    context: (usize, usize),
    cancelled: &impl Fn() -> bool,
) -> Result<SourceBeatmap, String> {
    if file.metadata().map_err(error)?.len() != receipt.bytes {
        return Err("source beatmap artifact extent differs from receipt".into());
    }
    super::source_beatmap::budget::edit(receipt.bytes as usize, context.0, context.1)?;
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
    let map = SourceBeatmap::parse_with_context(text, context.0, context.1)?;
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
