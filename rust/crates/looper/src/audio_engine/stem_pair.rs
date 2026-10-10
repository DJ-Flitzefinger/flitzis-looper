//! Off-thread complete stem pair storage. Durable descriptions carry no pad authority.

use super::cold_store::sealed_reader;
use super::material_migration::PreparedMigrationMaterial;
use super::material_migration_stems::verify_complete_wav_set;
use super::material_paths::{self, AssetKind};
use super::project_assets::{self, FileIdentity};
use super::stem_cache::{
    STEM_FILE_NAMES, admitted_stem_pcm_bytes, prepare_complete_stems_at_project_root,
    verify_pcm16_wav_geometry,
};
use super::stem_pair_descriptor::*;
use crate::messages::{ResidentSourceView, SampleBuffer};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const JSON_LIMIT: usize = 256 * 1024;
const PAIR_LIMIT: usize = 256;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StemPairFault {
    PcmFlush,
    StagingReopen,
    PcmRename,
    ReadyReopen,
    CommonFlush,
    CommonReopen,
}
#[cfg(test)]
thread_local! {
    static TEST_FAULT: std::cell::Cell<Option<StemPairFault>> = const { std::cell::Cell::new(None) };
    static TEST_IO_OBSERVER: std::cell::RefCell<Option<Box<dyn Fn(StemPairFault)>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(super) fn set_io_observer_for_test(observer: Option<Box<dyn Fn(StemPairFault)>>) {
    TEST_IO_OBSERVER.with(|current| *current.borrow_mut() = observer);
}
#[cfg(test)]
pub(super) fn set_fault_for_test(fault: Option<StemPairFault>) {
    TEST_FAULT.with(|current| current.set(fault));
}
#[cfg(test)]
fn fail_at(point: StemPairFault) -> Result<(), String> {
    TEST_IO_OBSERVER.with(|observer| {
        if let Some(observer) = observer.borrow().as_ref() {
            observer(point);
        }
    });
    if TEST_FAULT.with(|current| current.get()) == Some(point) {
        Err(format!("injected stem pair I/O failure at {point:?}"))
    } else {
        Ok(())
    }
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}
fn check_cancelled(cancelled: &impl Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("complete stem pair preparation cancelled".into())
    } else {
        Ok(())
    }
}
fn reference(root: &Path, path: &Path) -> Result<String, String> {
    let (root, path) = project_assets::owned_path(root, path).map_err(error)?;
    Ok(format!(
        "samples/{}",
        path.strip_prefix(&root)
            .map_err(error)?
            .to_string_lossy()
            .replace('\\', "/")
    ))
}

fn guarded_root(
    root: &Path,
    material: &PreparedMigrationMaterial,
) -> Result<(PathBuf, Vec<File>), String> {
    if !root.is_absolute()
        || root.file_name().is_none_or(|name| name != "samples")
        || root
            .to_string_lossy()
            .split(['/', '\\'])
            .any(|part| [".", ".."].contains(&part))
    {
        return Err("pair root must be an absolute ordinary samples directory".into());
    }
    let guards = project_assets::directory_guards(root).map_err(error)?;
    let root = fs::canonicalize(root).map_err(error)?;
    if project_assets::capture_identity(&root)
        .map_err(error)?
        .as_ref()
        != guards
            .first()
            .and_then(|file| project_assets::file_identity(file).ok())
            .as_ref()
        || !material.lease.original_path.starts_with(&root)
    {
        return Err("pair source/samples root mismatch".into());
    }
    Ok((root, guards))
}
fn exact_children(path: &Path, expected: &[String]) -> Result<(), String> {
    let mut count = 0;
    for entry in fs::read_dir(path).map_err(error)?.take(expected.len() + 1) {
        let entry = entry.map_err(error)?;
        if !expected
            .iter()
            .any(|name| entry.file_name() == name.as_str())
        {
            return Err("complete stem pair has an unknown child".into());
        }
        count += 1;
    }
    if count != expected.len() {
        return Err("complete stem pair is incomplete".into());
    }
    Ok(())
}
fn bytes(file: &mut File, limit: usize) -> Result<Vec<u8>, String> {
    if file.metadata().map_err(error)?.len() > limit as u64 {
        return Err("stem pair JSON exceeds bound".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(error)?;
    let mut result = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut result)
        .map_err(error)?;
    if result.len() > limit {
        return Err("stem pair JSON exceeds bound".into());
    }
    Ok(result)
}
fn full_hash(file: &mut File, cancelled: &impl Fn() -> bool) -> Result<(String, u64), String> {
    file.seek(SeekFrom::Start(0)).map_err(error)?;
    let extent = file.metadata().map_err(error)?.len();
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        check_cancelled(cancelled)?;
        let count = file.read(&mut buffer).map_err(error)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or("stem extent overflow")?;
        if total > extent {
            return Err("sealed stem grew during verification".into());
        }
        hash.update(&buffer[..count]);
    }
    if total != extent {
        return Err("sealed stem EOF mismatch".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(error)?;
    Ok((format!("{:x}", hash.finalize()), total))
}
fn source(material: &PreparedMigrationMaterial) -> Result<StemPairSource, String> {
    material
        .lease
        .verify_reference(&material.sample)
        .map_err(error)?;
    let view = material
        .sample
        .residency
        .as_ref()
        .ok_or("complete native source descriptor missing")?;
    let identity = &view.source;
    let original = &material.lease.manifest.descriptor["decoder"]["original"];
    if original["sha256"].as_str() != Some(hex(&identity.original_sha256).as_str()) {
        return Err("pair native source/original digest mismatch".into());
    }
    Ok(StemPairSource {
        frame_count: identity.frame_count as u64,
        channels: u32::try_from(identity.channels).map_err(error)?,
        sample_rate_hz: identity.sample_rate_hz,
        original_bytes: original["bytes"]
            .as_u64()
            .ok_or("original extent missing")?,
        original_sha256: hex(&identity.original_sha256),
        playback_sha256: hex(&identity.playback_sha256),
        mono_sha256: hex(&identity.mono_sha256),
        transform_sha256: hex(&identity.transform_sha256),
        source_zero_frame: identity.source_zero_frame as u64,
    })
}
fn hex(value: &[u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// These sealed files remain off-thread; component buffers alone enter callbacks.
pub(super) struct VerifiedStemPair {
    pub descriptor: StemPairDescriptor,
    pub descriptor_reference: String,
    pub created_pcm: bool,
    pub created_descriptor: bool,
    pcm: [File; 5],
    pub files: Vec<File>,
}

impl VerifiedStemPair {
    /// Move every sealed reader to the existing off-thread ownership registry.
    pub(super) fn into_pins(
        self,
        project_root: &Path,
    ) -> super::project_assets::stem_readers::PairPins {
        let mut files = self.files;
        files.extend(self.pcm);
        super::project_assets::stem_readers::PairPins {
            files,
            pcm_path: project_root.join(self.descriptor.pcm_generation),
            descriptor_path: project_root.join(self.descriptor_reference),
        }
    }
    fn read_range(&self, index: usize, current: &SampleBuffer) -> Result<SampleBuffer, String> {
        let identity = &self.descriptor.content.source;
        let view = current
            .residency
            .as_ref()
            .ok_or("native stem window source missing")?;
        if !current.valid_residency(identity.sample_rate_hz, identity.channels as usize)
            || current.frame_count() as u64 != identity.frame_count
            || current.channels as u32 != identity.channels
            || view.source.sample_rate_hz != identity.sample_rate_hz
            || hex(&view.source.original_sha256) != identity.original_sha256
            || hex(&view.source.playback_sha256) != identity.playback_sha256
            || hex(&view.source.mono_sha256) != identity.mono_sha256
            || hex(&view.source.transform_sha256) != identity.transform_sha256
            || view.source.source_zero_frame as u64 != identity.source_zero_frame
        {
            return Err("stem pair/current native source geometry mismatch".into());
        }
        let frames = current
            .resident_end()
            .checked_sub(current.resident_start())
            .ok_or("invalid source window")?;
        let length = frames
            .checked_mul(current.channels)
            .and_then(|n| n.checked_mul(4))
            .ok_or("stem window extent overflow")?;
        if length > super::cold_jobs::PCM_LIMIT_BYTES {
            return Err("stem window exceeds admission".into());
        }
        let start = current
            .resident_start()
            .checked_mul(current.channels)
            .and_then(|n| n.checked_mul(4))
            .ok_or("stem window origin overflow")?;
        let mut encoded = vec![0_u8; length];
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            let mut read = 0;
            while read < length {
                let count = self.pcm[index]
                    .seek_read(&mut encoded[read..], (start + read) as u64)
                    .map_err(error)?;
                if count == 0 {
                    return Err("stem range EOF mismatch".into());
                }
                read += count;
            }
        }
        #[cfg(not(windows))]
        {
            return Err("stem range requires tested positioned Windows read".into());
        }
        let mut values = Vec::with_capacity(length / 4);
        for chunk in encoded.chunks_exact(4) {
            let value = f32::from_bits(u32::from_le_bytes(chunk.try_into().unwrap()));
            if !value.is_finite() {
                return Err("aligned stem PCM is nonfinite".into());
            }
            values.push(value);
        }
        Ok(SampleBuffer {
            channels: current.channels,
            samples: Arc::from(values),
            residency: Some(Arc::new(ResidentSourceView {
                source: view.source.clone(),
                start_frame: view.start_frame,
                window_revision: view.window_revision,
                context: view.context,
            })),
        })
    }
    /// Read four actual component ranges, never the instrumental live layer.
    pub(super) fn prepare_component_views(
        &self,
        current: &SampleBuffer,
    ) -> Result<[SampleBuffer; 4], String> {
        let maximum = admitted_component_view_bytes(current.samples.len())?;
        if maximum > super::cold_jobs::PCM_LIMIT_BYTES {
            return Err("component transient PCM admission exceeded".into());
        }
        let mut buffers = Vec::with_capacity(4);
        for index in 0..4 {
            buffers.push(self.read_range(index, current)?);
        }
        buffers
            .try_into()
            .map_err(|_| "component set incomplete".into())
    }
    /// Explicit offline access pins this same complete pair during the read.
    pub(super) fn read_instrumental(&self, current: &SampleBuffer) -> Result<SampleBuffer, String> {
        let maximum = current
            .samples
            .len()
            .checked_mul(4)
            .and_then(|n| n.checked_mul(4))
            .ok_or("instrumental transient extent overflow")?;
        if maximum > super::cold_jobs::PCM_LIMIT_BYTES {
            return Err("instrumental transient PCM admission exceeded".into());
        }
        self.read_range(4, current)
    }
}

/// Open only a common descriptor plus both complete sealed generations.
pub(super) fn open_verified_pair(
    root: &Path,
    pair_reference: &str,
    material: &PreparedMigrationMaterial,
    cancelled: &impl Fn() -> bool,
) -> Result<VerifiedStemPair, String> {
    let (root, root_guards) = guarded_root(root, material)?;
    let root = root.as_path();
    let typed = material_paths::resolve(root, Path::new(pair_reference)).map_err(error)?;
    let Some(material_id) = material.lease.material_id.as_ref() else {
        return Err("pair material identity missing".into());
    };
    if typed.kind
        != (AssetKind::StemPairDescriptor {
            material: material_id.clone(),
        })
    {
        return Err("pair descriptor typed material mismatch".into());
    }
    let mut files = root_guards;
    files.extend(
        project_assets::directory_guards(typed.path.parent().ok_or("pair parent missing")?)
            .map_err(error)?,
    );
    let mut descriptor_file = sealed_reader(&typed.path).map_err(error)?;
    let descriptor: StemPairDescriptor =
        serde_json::from_slice(&bytes(&mut descriptor_file, JSON_LIMIT)?).map_err(error)?;
    descriptor.validate()?;
    if descriptor.content.source != source(material)?
        || descriptor.content.material_id != *material_id
    {
        return Err("pair/source content mismatch".into());
    }
    let mut set = verified_wavs(
        root,
        &descriptor.content,
        &descriptor.wav_generation,
        cancelled,
    )?;
    let (marker_hash, _) = full_hash(&mut set._files[5], cancelled)?;
    if marker_hash != descriptor.wav_manifest_sha256 {
        return Err("WAV complete marker changed".into());
    }
    files.append(&mut set._guards);
    files.append(&mut set._files);
    let pcm_path = material_paths::resolve(root, Path::new(&descriptor.pcm_generation))
        .map_err(error)?
        .path;
    let mut complete = verified_pcm_directory(&pcm_path, &descriptor.content, cancelled)?;
    if complete.manifest.wav_generation != descriptor.wav_generation
        || complete.manifest_sha256 != descriptor.pcm_manifest_sha256
    {
        return Err("joint PCM/WAV manifest binding mismatch".into());
    }
    files.append(&mut complete.files);
    files.push(descriptor_file);
    Ok(VerifiedStemPair {
        descriptor,
        descriptor_reference: reference(root, &typed.path)?,
        created_pcm: false,
        created_descriptor: false,
        pcm: complete.pcm,
        files,
    })
}

fn verified_wavs(
    root: &Path,
    content: &StemPairContent,
    wav_reference: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<super::material_migration_stems::SealedSet, String> {
    let path = material_paths::resolve(root, Path::new(wav_reference)).map_err(error)?;
    if path.kind
        != (AssetKind::StemDirectory {
            material: Some(content.material_id.clone()),
            generation: true,
        })
        || !path
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(material_paths::ready_generation)
    {
        return Err("pair WAV generation is not committed in its material".into());
    }
    let frames = usize::try_from(content.source.frame_count).map_err(error)?;
    let channels = usize::try_from(content.source.channels).map_err(error)?;
    if admitted_stem_pcm_bytes(frames, channels)? > super::cold_jobs::PCM_LIMIT_BYTES {
        return Err("complete pair geometry exceeds PCM admission".into());
    }
    let maximum = content
        .source
        .frame_count
        .checked_mul(u64::from(content.source.channels))
        .and_then(|n| n.checked_mul(2))
        .and_then(|n| n.checked_add(1024 * 1024))
        .ok_or("stem WAV bound overflow")?;
    let mut set = verify_complete_wav_set(&path.path, &content.source_version, maximum, cancelled)?;
    for (index, artifact) in content.artifacts.iter().enumerate() {
        if set.marker["stems"][&artifact.name].as_str() != Some(artifact.wav_sha256.as_str())
            || set._files[index].metadata().map_err(error)?.len() != artifact.wav_bytes
        {
            return Err("WAV pair artifact mismatch".into());
        }
        check_cancelled(cancelled)?;
        verify_pcm16_wav_geometry(
            &mut set._files[index],
            content.source.sample_rate_hz,
            channels,
            frames,
        )?;
    }
    Ok(set)
}

struct VerifiedPcm {
    manifest: StemPcmManifest,
    manifest_sha256: String,
    pcm: [File; 5],
    files: Vec<File>,
}
fn verified_pcm_directory(
    pcm_path: &Path,
    content: &StemPairContent,
    cancelled: &impl Fn() -> bool,
) -> Result<VerifiedPcm, String> {
    let mut files = project_assets::directory_guards(pcm_path).map_err(error)?;
    let mut names: Vec<String> = STEM_FILE_NAMES
        .iter()
        .map(|name| format!("{name}.f32le"))
        .collect();
    names.push("manifest.json".into());
    exact_children(pcm_path, &names)?;
    let mut manifest_file = sealed_reader(&pcm_path.join("manifest.json")).map_err(error)?;
    let encoded = bytes(&mut manifest_file, JSON_LIMIT)?;
    let manifest: StemPcmManifest = serde_json::from_slice(&encoded).map_err(error)?;
    manifest.validate()?;
    if manifest.content != *content {
        return Err("complete PCM content binding mismatch".into());
    }
    let mut pcm = Vec::with_capacity(5);
    for artifact in &content.artifacts {
        let mut reader =
            sealed_reader(&pcm_path.join(format!("{}.f32le", artifact.name))).map_err(error)?;
        verify_pcm(&mut reader, artifact, cancelled)?;
        pcm.push(reader);
    }
    files.push(manifest_file);
    Ok(VerifiedPcm {
        manifest,
        manifest_sha256: format!("{:x}", Sha256::digest(&encoded)),
        pcm: pcm.try_into().map_err(|_| "complete PCM set incomplete")?,
        files,
    })
}

fn verify_pcm(
    reader: &mut File,
    artifact: &StemPairArtifact,
    cancelled: &impl Fn() -> bool,
) -> Result<(), String> {
    if reader.metadata().map_err(error)?.len() != artifact.pcm_bytes {
        return Err("complete aligned PCM extent mismatch".into());
    }
    reader.seek(SeekFrom::Start(0)).map_err(error)?;
    let mut digest = Sha256::new();
    let mut remaining = artifact.pcm_bytes;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining != 0 {
        check_cancelled(cancelled)?;
        let count = remaining.min(buffer.len() as u64) as usize;
        reader.read_exact(&mut buffer[..count]).map_err(error)?;
        if count % 4 != 0
            || buffer[..count].chunks_exact(4).any(|word| {
                !f32::from_bits(u32::from_le_bytes(word.try_into().unwrap())).is_finite()
            })
        {
            return Err("complete aligned PCM finite/EOF mismatch".into());
        }
        digest.update(&buffer[..count]);
        remaining -= count as u64;
    }
    if reader.read(&mut [0_u8; 1]).map_err(error)? != 0
        || format!("{:x}", digest.finalize()) != artifact.pcm_sha256
    {
        return Err("complete aligned PCM digest/EOF mismatch".into());
    }
    reader.seek(SeekFrom::Start(0)).map_err(error)?;
    Ok(())
}

/// Fresh complete disk proof for migration/history; no pad or callback authority.
pub(super) fn verify_recovery_area(root: &Path, reference: &Path) -> Result<Vec<File>, String> {
    let typed = material_paths::resolve(root, reference).map_err(error)?;
    let expected_material = match &typed.kind {
        AssetKind::StemPcmDirectory { material, .. }
        | AssetKind::StemPairDescriptor { material } => material.clone(),
        _ => return Err("recovery requires a canonical stem pair area".into()),
    };
    let mut guards = project_assets::directory_guards(
        if matches!(typed.kind, AssetKind::StemPairDescriptor { .. }) {
            typed
                .path
                .parent()
                .ok_or("pair descriptor parent missing")?
        } else {
            &typed.path
        },
    )
    .map_err(error)?;
    let (content, wav_reference, pcm_path, descriptor) = match typed.kind {
        AssetKind::StemPcmDirectory {
            generation: true, ..
        } if typed
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(material_paths::ready_generation) =>
        {
            let mut file = sealed_reader(&typed.path.join("manifest.json")).map_err(error)?;
            let manifest: StemPcmManifest =
                serde_json::from_slice(&bytes(&mut file, JSON_LIMIT)?).map_err(error)?;
            manifest.validate()?;
            guards.push(file);
            (manifest.content, manifest.wav_generation, typed.path, None)
        }
        AssetKind::StemPairDescriptor { .. } => {
            let mut file = sealed_reader(&typed.path).map_err(error)?;
            let descriptor: StemPairDescriptor =
                serde_json::from_slice(&bytes(&mut file, JSON_LIMIT)?).map_err(error)?;
            descriptor.validate()?;
            let pcm_path = material_paths::resolve(root, Path::new(&descriptor.pcm_generation))
                .map_err(error)?
                .path;
            guards.push(file);
            (
                descriptor.content.clone(),
                descriptor.wav_generation.clone(),
                pcm_path,
                Some(descriptor),
            )
        }
        _ => return Err("recovery requires committed stem PCM or a pair descriptor".into()),
    };
    if content.material_id != expected_material {
        return Err("pair recovery area/material mismatch".into());
    }
    let (original_reference, _) = content
        .source_version
        .split_once("|sha256-v1:")
        .ok_or("pair source version missing")?;
    let original = material_paths::resolve(root, Path::new(original_reference)).map_err(error)?;
    if !matches!(original.kind, AssetKind::Original {material: Some(ref material)}
        if material == &content.material_id)
    {
        return Err("pair recovery original/material mismatch".into());
    }
    guards.extend(
        project_assets::directory_guards(
            original
                .path
                .parent()
                .ok_or("pair original parent missing")?,
        )
        .map_err(error)?,
    );
    let mut original_file = sealed_reader(&original.path).map_err(error)?;
    let (digest, extent) = full_hash(&mut original_file, &|| false)?;
    if digest != content.source.original_sha256 || extent != content.source.original_bytes {
        return Err("pair recovery original changed".into());
    }
    let wavs = verified_wavs(root, &content, &wav_reference, &|| false)?;
    let pcm = verified_pcm_directory(&pcm_path, &content, &|| false)?;
    if pcm.manifest.wav_generation != wav_reference {
        return Err("pair recovery direct WAV manifest binding mismatch".into());
    }
    let mut marker = wavs
        ._files
        .last()
        .ok_or("WAV marker missing")?
        .try_clone()
        .map_err(error)?;
    let (wav_manifest_sha256, _) = full_hash(&mut marker, &|| false)?;
    if let Some(descriptor) = descriptor {
        if descriptor.pcm_manifest_sha256 != pcm.manifest_sha256
            || descriptor.wav_manifest_sha256 != wav_manifest_sha256
            || descriptor.stem_set_identity != pcm.manifest.stem_set_identity
        {
            return Err("pair recovery common/manifest binding mismatch".into());
        }
    } else {
        guards.extend(recovery_common_readers(
            root,
            &pcm_path,
            &pcm.manifest,
            &pcm.manifest_sha256,
            &wav_manifest_sha256,
        )?);
    }
    guards.push(original_file);
    guards.extend(wavs._guards);
    guards.extend(wavs._files);
    guards.extend(pcm.files);
    guards.extend(pcm.pcm);
    Ok(guards)
}

fn recovery_common_readers(
    root: &Path,
    pcm_path: &Path,
    manifest: &StemPcmManifest,
    pcm_manifest_sha256: &str,
    wav_manifest_sha256: &str,
) -> Result<Vec<File>, String> {
    // Physical names are read from verified metadata, never inferred from the
    // logical identity. Unknown descriptors remain untouched and ineligible.
    let pairs = pcm_path
        .parent()
        .ok_or("PCM parent missing")?
        .join(".pairs");
    if !pairs.exists() {
        return Ok(Vec::new());
    }
    let mut files = project_assets::directory_guards(&pairs).map_err(error)?;
    let entries = fs::read_dir(&pairs)
        .map_err(error)?
        .take(PAIR_LIMIT + 1)
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(error)?;
    if entries.len() > PAIR_LIMIT {
        return Err("pair recovery descriptor inventory exceeds bound".into());
    }
    let pcm_reference = reference(root, pcm_path)?;
    for entry in entries {
        let Ok(typed) = material_paths::resolve(root, &entry.path()) else {
            continue;
        };
        if !matches!(typed.kind, AssetKind::StemPairDescriptor { .. }) {
            continue;
        }
        let Ok(mut file) = sealed_reader(&typed.path) else {
            continue;
        };
        let Ok(encoded) = bytes(&mut file, JSON_LIMIT) else {
            continue;
        };
        let Ok(common) = serde_json::from_slice::<StemPairDescriptor>(&encoded) else {
            continue;
        };
        if common.validate().is_ok()
            && common.content == manifest.content
            && common.pcm_generation == pcm_reference
            && common.wav_generation == manifest.wav_generation
            && common.pcm_manifest_sha256 == pcm_manifest_sha256
            && common.wav_manifest_sha256 == wav_manifest_sha256
        {
            files.push(file);
        }
    }
    Ok(files)
}

struct CreatedGeneration {
    path: PathBuf,
    identity: FileIdentity,
    files: Vec<(PathBuf, FileIdentity)>,
    guards: Vec<File>,
    accepted: bool,
}
impl Drop for CreatedGeneration {
    fn drop(&mut self) {
        if self.accepted {
            return;
        }
        self.guards.clear();
        let Some(parent) = self.path.parent() else {
            return;
        };
        let Ok(_parents) = project_assets::directory_guards(parent) else {
            return;
        };
        let Ok(guards) = project_assets::directory_guards(&self.path) else {
            return;
        };
        if guards
            .first()
            .and_then(|file| project_assets::file_identity(file).ok())
            .as_ref()
            != Some(&self.identity)
        {
            return;
        }
        for (path, identity) in &self.files {
            let _ = project_assets::remove_owned_file(path, Some(identity));
        }
        drop(guards);
        let _ = project_assets::remove_empty_directory(&self.path, &self.identity);
    }
}

struct CreatedDescriptor {
    path: PathBuf,
    identity: FileIdentity,
    accepted: bool,
}
impl Drop for CreatedDescriptor {
    fn drop(&mut self) {
        if !self.accepted {
            if let Some(parent) = self.path.parent() {
                if let Ok(_parents) = project_assets::directory_guards(parent) {
                    let _ = project_assets::remove_owned_file(&self.path, Some(&self.identity));
                }
            }
        }
    }
}

fn guarded_base(material_root: &Path) -> Result<(PathBuf, Vec<File>), String> {
    let mut guards = project_assets::directory_guards(material_root).map_err(error)?;
    let mut path = material_root.to_owned();
    for name in [".pcm-cache", "stems", "v1", ".pairs"] {
        path.push(name);
        match fs::create_dir(&path) {
            Ok(()) => (),
            Err(value) if value.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(value) => return Err(error(value)),
        }
        // The actual parent stays held before creating or opening its child.
        guards.extend(project_assets::directory_guards(&path).map_err(error)?);
    }
    Ok((path.parent().ok_or("pair root missing")?.to_owned(), guards))
}
fn write_new(path: &Path, data: &[u8], created: &mut CreatedGeneration) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(error)?;
    created.files.push((
        path.to_owned(),
        project_assets::file_identity(&file).map_err(error)?,
    ));
    file.write_all(data).map_err(error)?;
    file.sync_all().map_err(error)?;
    Ok(())
}

/// Produce or reuse one complete eligible immutable pair under the existing gate.
#[cfg(test)]
pub(super) fn prepare_complete_pair(
    root: &Path,
    material: &PreparedMigrationMaterial,
    source_version: &str,
    wav_reference: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<VerifiedStemPair, String> {
    prepare_complete_pair_with_reader(
        root,
        material,
        source_version,
        wav_reference,
        cancelled,
        || Ok(material.sample.clone()),
    )
}

/// Existing source, four final views and encoded/Vec/Arc-copy overlap for the last view.
pub(super) fn admitted_component_view_bytes(sample_count: usize) -> Result<usize, String> {
    sample_count
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_mul(7))
        .ok_or_else(|| "component transient extent overflow".into())
}

/// Warm lookup precedes full-source materialization and five-buffer conversion.
pub(super) fn prepare_complete_pair_with_reader(
    root: &Path,
    material: &PreparedMigrationMaterial,
    source_version: &str,
    wav_reference: &str,
    cancelled: &impl Fn() -> bool,
    read_complete: impl FnOnce() -> Result<SampleBuffer, String>,
) -> Result<VerifiedStemPair, String> {
    let (root, _root_guards) = guarded_root(root, material)?;
    let root = root.as_path();
    let current = source(material)?;
    let material_id = material
        .lease
        .material_id
        .as_ref()
        .ok_or("pair material identity missing")?;
    let wav = material_paths::resolve(root, Path::new(wav_reference)).map_err(error)?;
    if wav.kind
        != (AssetKind::StemDirectory {
            material: Some(material_id.clone()),
            generation: true,
        })
        || !wav
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(material_paths::ready_generation)
    {
        return Err("pair requires a committed canonical five-WAV generation".into());
    }
    let _gate = material
        .lease
        .stem_preparation_gate(cancelled)
        .map_err(error)?;
    check_cancelled(cancelled)?;
    let maximum = current
        .frame_count
        .checked_mul(u64::from(current.channels))
        .and_then(|n| n.checked_mul(2))
        .and_then(|n| n.checked_add(1024 * 1024))
        .ok_or("stem WAV bound overflow")?;
    let wav_set = verify_complete_wav_set(&wav.path, source_version, maximum, cancelled)?;
    let material_root = material
        .lease
        .original_path
        .parent()
        .and_then(Path::parent)
        .ok_or("material root missing")?;
    let (base, _base_guards) = guarded_base(material_root)?;
    let mut candidates = 0;
    for entry in fs::read_dir(base.join(".pairs"))
        .map_err(error)?
        .take(PAIR_LIMIT + 1)
    {
        candidates += 1;
        if candidates > PAIR_LIMIT {
            return Err("stem pair inventory capacity exceeded".into());
        }
        let entry = entry.map_err(error)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name
            .strip_suffix(".json")
            .is_some_and(material_paths::valid_id)
        {
            continue;
        }
        let pair_ref = reference(root, &entry.path())?;
        let Ok(pair) = open_verified_pair(root, &pair_ref, material, cancelled) else {
            continue;
        };
        if pair.descriptor.content.source_version == source_version
            && pair.descriptor.content.artifacts.iter().all(|artifact| {
                wav_set.marker["stems"][&artifact.name].as_str()
                    == Some(artifact.wav_sha256.as_str())
            })
        {
            return Ok(pair);
        }
    }
    let complete_source = read_complete()?;
    material
        .lease
        .verify_reference(&complete_source)
        .map_err(error)?;
    let complete = prepare_complete_stems_at_project_root(
        &complete_source,
        current.sample_rate_hz,
        wav_reference,
        root.parent().ok_or("project root missing")?,
    )?;
    let mut artifacts = Vec::with_capacity(5);
    for index in 0..5 {
        let samples = &complete.stems[index].samples;
        let mut hash = Sha256::new();
        for value in samples.iter() {
            hash.update(value.to_bits().to_le_bytes());
        }
        artifacts.push(StemPairArtifact {
            name: STEM_FILE_NAMES[index].into(),
            wav_sha256: wav_set.marker["stems"][STEM_FILE_NAMES[index]]
                .as_str()
                .ok_or("verified WAV digest missing")?
                .into(),
            wav_bytes: wav_set._files[index].metadata().map_err(error)?.len(),
            pcm_sha256: format!("{:x}", hash.finalize()),
            pcm_bytes: (samples.len() as u64) * 4,
        });
    }
    let artifacts = artifacts
        .try_into()
        .map_err(|_| "complete artifact set missing")?;
    let content = StemPairContent {
        material_id: material_id.clone(),
        source_version: source_version.into(),
        source: current,
        conversion: StemPairConversion {
            schema_version: 1,
            policy: "pcm16-exact-geometry-v1".into(),
        },
        alignment: StemPairAlignment {
            schema_version: 1,
            policy: "shared-onset-v1".into(),
            frame_domain: "complete-playback-frames".into(),
            offset_frames: i64::try_from(complete.offset_frames).map_err(error)?,
        },
        artifacts,
    };
    let identity = content.logical_identity()?;
    let generation = identity[..32].to_owned();
    let ready = base.join(format!(".ready-{generation}"));
    let staging = base.join(format!(".generation-{generation}"));
    let manifest = StemPcmManifest {
        schema_version: 1,
        encoding: "aligned-stem-pcm-v1".into(),
        stem_set_identity: identity.clone(),
        wav_generation: reference(root, &wav.path)?,
        content: content.clone(),
    };
    let manifest_bytes = manifest.canonical_bytes()?;
    let mut created = None;
    if !ready.exists() {
        fs::create_dir(&staging).map_err(error)?;
        let guards = project_assets::directory_guards(&staging).map_err(error)?;
        let mut transaction = CreatedGeneration {
            identity: project_assets::file_identity(guards.first().ok_or("new PCM guard missing")?)
                .map_err(error)?,
            path: staging.clone(),
            files: Vec::new(),
            guards,
            accepted: false,
        };
        for (index, name) in STEM_FILE_NAMES.iter().enumerate() {
            check_cancelled(cancelled)?;
            let path = staging.join(format!("{name}.f32le"));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(error)?;
            transaction
                .files
                .push((path, project_assets::file_identity(&file).map_err(error)?));
            let mut chunk = Vec::with_capacity(64 * 1024);
            for values in complete.stems[index].samples.chunks(16 * 1024) {
                chunk.clear();
                for value in values {
                    chunk.extend_from_slice(&value.to_bits().to_le_bytes());
                }
                file.write_all(&chunk).map_err(error)?;
                check_cancelled(cancelled)?;
            }
            #[cfg(test)]
            fail_at(StemPairFault::PcmFlush)?;
            file.sync_all().map_err(error)?;
        }
        write_new(
            &staging.join("manifest.json"),
            &manifest_bytes,
            &mut transaction,
        )?;
        // Full sealed reopen precedes rename; all new leaf identities stay owned.
        #[cfg(test)]
        fail_at(StemPairFault::StagingReopen)?;
        let verified = verified_pcm_directory(&staging, &content, cancelled)?;
        drop(verified);
        check_cancelled(cancelled)?;
        transaction.guards.clear();
        #[cfg(test)]
        fail_at(StemPairFault::PcmRename)?;
        fs::rename(&staging, &ready).map_err(error)?;
        transaction.path = ready.clone();
        for (path, _) in &mut transaction.files {
            *path = ready.join(path.file_name().ok_or("PCM leaf missing")?);
        }
        transaction.guards = project_assets::directory_guards(&ready).map_err(error)?;
        if project_assets::file_identity(
            transaction
                .guards
                .first()
                .ok_or("ready PCM guard missing")?,
        )
        .map_err(error)?
            != transaction.identity
        {
            return Err("renamed PCM generation identity changed".into());
        }
        created = Some(transaction);
    }
    // A recognized crash-after-rename generation is reused only after complete verification.
    // Unknown, incomplete or inconsistent existing generations remain untouched.
    #[cfg(test)]
    fail_at(StemPairFault::ReadyReopen)?;
    let complete_pcm = verified_pcm_directory(&ready, &content, cancelled)?;
    let mut committed_wav = verified_wavs(
        root,
        &content,
        &complete_pcm.manifest.wav_generation,
        cancelled,
    )?;
    let (wav_manifest_sha256, _) = full_hash(&mut committed_wav._files[5], cancelled)?;
    let descriptor = StemPairDescriptor {
        schema_version: 1,
        encoding: "aligned-stem-pair-v1".into(),
        content,
        stem_set_identity: identity,
        wav_generation: complete_pcm.manifest.wav_generation.clone(),
        pcm_generation: reference(root, &ready)?,
        wav_manifest_sha256,
        pcm_manifest_sha256: complete_pcm.manifest_sha256.clone(),
    };
    let pair_path = base.join(".pairs").join(format!("{generation}.json"));
    let encoded = descriptor.canonical_bytes()?;
    // Both immutable areas are now held fully verified; publish common eligibility last.
    // Interrupted/unknown leaves are never overwritten by a follower.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pair_path)
        .map_err(error)?;
    let mut common = CreatedDescriptor {
        path: pair_path.clone(),
        identity: project_assets::file_identity(&file).map_err(error)?,
        accepted: false,
    };
    let written = file.write_all(&encoded).and_then(|()| {
        #[cfg(test)]
        fail_at(StemPairFault::CommonFlush).map_err(std::io::Error::other)?;
        file.sync_all()
    });
    drop(file);
    written.map_err(error)?;
    #[cfg(test)]
    fail_at(StemPairFault::CommonReopen)?;
    let result = open_verified_pair(root, &reference(root, &pair_path)?, material, cancelled);
    match result {
        Ok(mut pair) => {
            pair.created_pcm = created.is_some();
            if let Some(created) = &mut created {
                created.accepted = true;
            }
            common.accepted = true;
            pair.created_descriptor = true;
            Ok(pair)
        }
        Err(value) => Err(value),
    }
}
