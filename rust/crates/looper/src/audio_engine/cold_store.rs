//! Copy-first cold PCM transactions. Every operation here runs on a load worker.
//! Successful entries remain durable; shared reuse and last-owner deletion are C1b.

use super::analysis_pcm::MONO_RULE;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const CHUNK_BYTES: usize = 64 * 1024;
const SCHEMA_VERSION: u64 = 1;
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Complete PCM in its own rate/layout domain, before any residency selection.
pub(super) struct PcmArtifactInput<'a> {
    pub samples: &'a [f32],
    pub rate_hz: u32,
    pub channels: usize,
    /// Actual decoder or processing configuration, supplied by the executing loader.
    pub provenance: Value,
}

#[derive(Clone, Debug)]
pub(super) struct ColdManifest {
    pub identity: String,
    pub decoder_identity: String,
    pub descriptor: Value,
}

impl ColdManifest {
    fn encoded(&self) -> Value {
        json!({"schema_version":SCHEMA_VERSION,"identity":self.identity,
            "decoder_identity":self.decoder_identity,"descriptor":self.descriptor})
    }
}

/// Non-realtime engine ownership of sealed complete files. It never enters the callback.
pub(super) struct CommittedColdLease {
    pub manifest: ColdManifest,
    pub cache_path: PathBuf,
    pub original_path: PathBuf,
    // These handles enforce the same immutable objects that were fully verified.
    _readers: Vec<File>,
    _directories: Vec<File>,
}

/// Until adopted, Drop reverses only this transaction's exclusive creations.
pub(super) struct ColdTransaction {
    cache_root: PathBuf,
    staging: Option<PathBuf>,
    committed_cache: Option<PathBuf>,
    snapshot_path: PathBuf,
    snapshot: Option<File>,
    source_reader: Option<File>,
    original_reader: Option<File>,
    original_path: PathBuf,
    owned_original: bool,
    import: bool,
    source_digest: String,
    source_bytes: u64,
    extension: Option<String>,
    manifest: Option<ColdManifest>,
    artifact_readers: Vec<File>,
    directories: Vec<File>,
    committed_directory: Option<File>,
    verified: bool,
    adopted: bool,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn check_cancelled(cancelled: &impl Fn() -> bool) -> io::Result<()> {
    if cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "cold load cancelled",
        ))
    } else {
        Ok(())
    }
}

/// Portable metadata/open protocols cannot exclude a writer or pathname ABA.
/// Until a platform-specific protocol exists, other platforms fail safely.
fn sealed_reader(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ only: another reader may coexist, writes/deletes may not.
        // OPEN_REPARSE_POINT verifies the opened leaf itself instead of following
        // a replacement link between path inspection and handle acquisition.
        let reader = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0020_0000)
            .open(path)?;
        if reader.metadata()?.file_attributes() & 0x400 != 0 {
            return Err(invalid("immutable reader is a reparse point"));
        }
        Ok(reader)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "stable cold capture requires a tested platform sharing protocol",
        ))
    }
}

fn reject_links(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() {
            return Err(invalid("managed PCM path contains a symbolic link"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(invalid("managed PCM path contains a reparse point"));
            }
        }
    }
    Ok(())
}

fn reject_existing_links(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => reject_links(ancestor)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn directory_guard(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_BACKUP_SEMANTICS opens a directory; sharing excludes rename/delete.
        let guard = OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(path)?;
        let metadata = guard.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid(
                "managed directory guard is not an ordinary directory",
            ));
        }
        Ok(guard)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

fn ensure_owned_root(samples: &Path) -> io::Result<(PathBuf, Vec<File>)> {
    let samples = if samples.is_absolute() {
        samples.to_owned()
    } else {
        std::env::current_dir()?.join(samples)
    };
    reject_existing_links(&samples)?;
    fs::create_dir_all(&samples)?;
    reject_links(&samples)?;
    let samples = fs::canonicalize(&samples)?;
    let mut directories = vec![directory_guard(&samples)?];
    let mut root = samples;
    for component in [".pcm-cache", "v1"] {
        root = root.join(component);
        match fs::create_dir(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        reject_links(&root)?;
        if !root.is_dir() {
            return Err(invalid("managed PCM root is not a directory"));
        }
        directories.push(directory_guard(&root)?);
    }
    Ok((root, directories))
}

fn unique_staging(root: &Path) -> io::Result<PathBuf> {
    for _ in 0..1024 {
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!(".staging-{}-{generation}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "exclusive PCM staging exhausted",
    ))
}

fn copy_hashed(
    input: &mut File,
    output: &mut File,
    cancelled: &impl Fn() -> bool,
) -> io::Result<(String, u64)> {
    input.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    let mut bytes = [0_u8; CHUNK_BYTES];
    loop {
        check_cancelled(cancelled)?;
        let count = input.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        output.write_all(&bytes[..count])?;
        hash.update(&bytes[..count]);
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| invalid("snapshot length overflow"))?;
    }
    check_cancelled(cancelled)?;
    output.sync_all()?;
    Ok((format!("{:x}", hash.finalize()), total))
}

fn verify_file(
    file: &mut File,
    expected_digest: &str,
    expected_bytes: u64,
    cancelled: &impl Fn() -> bool,
) -> io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut digest = Sha256::new();
    let mut length = 0_u64;
    let mut bytes = [0_u8; CHUNK_BYTES];
    loop {
        check_cancelled(cancelled)?;
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        length = length
            .checked_add(count as u64)
            .ok_or_else(|| invalid("verified file length overflow"))?;
        digest.update(&bytes[..count]);
    }
    if length != expected_bytes || format!("{:x}", digest.finalize()) != expected_digest {
        return Err(invalid("complete cold file digest or length mismatch"));
    }
    file.seek(SeekFrom::Start(0))?;
    check_cancelled(cancelled)
}

/// serde_json's default Map is sorted; explicit recursion also protects that rule
/// if its preserve_order feature is enabled by another crate later.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut keys: Vec<_> = fields.keys().collect();
            keys.sort();
            let mut sorted = serde_json::Map::new();
            for key in keys {
                sorted.insert(key.clone(), canonical(&fields[key]));
            }
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

fn descriptor_digest(value: &Value) -> io::Result<String> {
    let bytes = serde_json::to_vec(&canonical(value)).map_err(io::Error::other)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn write_pcm(
    path: &Path,
    input: PcmArtifactInput<'_>,
    cancelled: &impl Fn() -> bool,
) -> io::Result<(Value, File)> {
    if input.channels == 0
        || input.channels > 32
        || input.rate_hz == 0
        || input.samples.is_empty()
        || !input.samples.len().is_multiple_of(input.channels)
        || !input.provenance.is_object()
    {
        return Err(invalid("invalid complete PCM dimensions or provenance"));
    }
    let full_bytes = input
        .samples
        .len()
        .checked_mul(4)
        .and_then(|n| u64::try_from(n).ok())
        .ok_or_else(|| invalid("PCM byte extent overflow"))?;
    let frames = input.samples.len() / input.channels;
    let mut writer = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut interleaved = Sha256::new();
    let mut mono = Sha256::new();
    let mut bytes = [0_u8; CHUNK_BYTES];
    // Complete frames ensure the mono sum is independent of write chunk boundaries.
    let chunk_samples = (CHUNK_BYTES / 4 / input.channels) * input.channels;
    for chunk in input.samples.chunks(chunk_samples) {
        check_cancelled(cancelled)?;
        for (sample, destination) in chunk.iter().zip(bytes.chunks_exact_mut(4)) {
            if !sample.is_finite() {
                return Err(invalid("nonfinite complete PCM sample"));
            }
            destination.copy_from_slice(&sample.to_le_bytes());
        }
        let encoded = &bytes[..chunk.len() * 4];
        writer.write_all(encoded)?;
        interleaved.update(encoded);
        for frame in chunk.chunks_exact(input.channels) {
            let sum: f64 = frame.iter().copied().map(f64::from).sum();
            mono.update(((sum / input.channels as f64) as f32).to_le_bytes());
        }
    }
    check_cancelled(cancelled)?;
    writer.sync_all()?;
    drop(writer);
    let digest = format!("{:x}", interleaved.finalize());
    let mut reader = sealed_reader(path)?;
    verify_file(&mut reader, &digest, full_bytes, cancelled)?;
    let descriptor = json!({"format":"f32-le-interleaved-v1","channel_layout":"interleaved-channel-index-order-v1",
        "rate_hz":input.rate_hz,"channels":input.channels,"full_frames":frames,"full_bytes":full_bytes,
        "interleaved_sha256":digest,"mono_sha256":format!("{:x}",mono.finalize()),
        "mono_revision":MONO_RULE,"source_zero_bits":"0000000000000000","provenance":input.provenance});
    Ok((descriptor, reader))
}

impl ColdTransaction {
    pub(super) fn capture(
        samples_dir: &Path,
        source_path: &Path,
        import: bool,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<Self> {
        check_cancelled(cancelled)?;
        // Establish write/delete exclusion before creating staging or reading any bytes.
        let mut source = sealed_reader(source_path)?;
        if !source.metadata()?.is_file() {
            return Err(invalid("source snapshot requires a regular file"));
        }
        let (cache_root, directories) = ensure_owned_root(samples_dir)?;
        if !import {
            let samples_root = cache_root
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| invalid("missing project samples root"))?;
            let checked_source = if source_path.is_absolute() {
                source_path.to_owned()
            } else {
                std::env::current_dir()?.join(source_path)
            };
            reject_links(&checked_source)?;
            let resolved_source = fs::canonicalize(&checked_source)?;
            if !resolved_source.starts_with(samples_root)
                || resolved_source.starts_with(cache_root.parent().expect("cache parent"))
            {
                return Err(invalid(
                    "restored original is outside the managed samples root",
                ));
            }
        }
        let staging = unique_staging(&cache_root)?;
        let snapshot_path = staging.join("snapshot.original");
        let mut transaction = Self {
            cache_root,
            staging: Some(staging),
            committed_cache: None,
            snapshot_path,
            snapshot: None,
            source_reader: None,
            original_reader: None,
            original_path: source_path.to_owned(),
            owned_original: false,
            import,
            source_digest: String::new(),
            source_bytes: 0,
            extension: source_path
                .extension()
                .and_then(|s| s.to_str())
                .map(str::to_owned),
            manifest: None,
            artifact_readers: Vec::new(),
            directories,
            committed_directory: None,
            verified: false,
            adopted: false,
        };
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&transaction.snapshot_path)?;
        let (digest, bytes) = copy_hashed(&mut source, &mut writer, cancelled)?;
        drop(writer);
        let mut snapshot = sealed_reader(&transaction.snapshot_path)?;
        verify_file(&mut snapshot, &digest, bytes, cancelled)?;
        transaction.source_digest = digest;
        transaction.source_bytes = bytes;
        transaction.snapshot = Some(snapshot);
        // Restore retains the actual project original under the same lease. Import
        // may release the external path now: subsequent work reads only the snapshot.
        if !import {
            transaction.source_reader = Some(source);
        }
        Ok(transaction)
    }

    pub(super) fn snapshot_file(&self) -> io::Result<File> {
        let mut reader = self
            .snapshot
            .as_ref()
            .ok_or_else(|| invalid("snapshot already retired"))?
            .try_clone()?;
        reader.seek(SeekFrom::Start(0))?;
        Ok(reader)
    }

    pub(super) fn source_digest(&self) -> &str {
        &self.source_digest
    }
    #[cfg(test)]
    pub(super) fn source_bytes(&self) -> u64 {
        self.source_bytes
    }
    pub(super) fn original_path(&self) -> &Path {
        &self.original_path
    }

    pub(super) fn write_pcm_artifacts(
        &mut self,
        decoder: PcmArtifactInput<'_>,
        playback: PcmArtifactInput<'_>,
        playback_transform: Value,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<ColdManifest> {
        check_cancelled(cancelled)?;
        if self.manifest.is_some() || !playback_transform.is_object() {
            return Err(invalid(
                "cold artifacts already prepared or missing transform",
            ));
        }
        let staging = self
            .staging
            .as_ref()
            .ok_or_else(|| invalid("cold transaction already committed"))?;
        let (decoder_pcm, decoder_reader) =
            write_pcm(&staging.join("decoder.f32le"), decoder, cancelled)?;
        self.artifact_readers.push(decoder_reader);
        let decoder_descriptor = json!({"schema_version":SCHEMA_VERSION,
            "original":{"sha256":self.source_digest,"bytes":self.source_bytes},"pcm":decoder_pcm});
        let decoder_identity = descriptor_digest(&decoder_descriptor)?;
        let (playback_pcm, playback_reader) =
            write_pcm(&staging.join("playback.f32le"), playback, cancelled)?;
        self.artifact_readers.push(playback_reader);
        let descriptor = json!({"schema_version":SCHEMA_VERSION,"decoder":decoder_descriptor,
            "playback":{"parent_identity":decoder_identity,"pcm":playback_pcm,"transform":playback_transform}});
        let manifest = ColdManifest {
            identity: descriptor_digest(&descriptor)?,
            decoder_identity,
            descriptor,
        };
        let bytes =
            serde_json::to_vec(&canonical(&manifest.encoded())).map_err(io::Error::other)?;
        let manifest_path = staging.join("manifest.json");
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&manifest_path)?;
        check_cancelled(cancelled)?;
        writer.write_all(&bytes)?;
        writer.sync_all()?;
        drop(writer);
        let mut reader = sealed_reader(&manifest_path)?;
        verify_file(
            &mut reader,
            &format!("{:x}", Sha256::digest(&bytes)),
            bytes.len() as u64,
            cancelled,
        )?;
        self.artifact_readers.push(reader);
        self.manifest = Some(manifest.clone());
        Ok(manifest)
    }

    fn create_original(&mut self, cancelled: &impl Fn() -> bool) -> io::Result<()> {
        if !self.import {
            self.original_reader = self.source_reader.take();
            return Ok(());
        }
        let samples = self
            .cache_root
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("invalid samples root"))?;
        let stem = self
            .original_path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("sample")
            .to_owned();
        let extension = self
            .extension
            .as_ref()
            .map(|ext| format!(".{ext}"))
            .unwrap_or_else(|| ".unknown".into());
        // Match the established same-stem/different-extension collision behavior.
        let prefix = format!("{stem}_");
        let dot_prefix = format!("{stem}.");
        let base_name = format!("{stem}{extension}");
        let collision = fs::read_dir(samples)?.try_fold(false, |found, entry| {
            let name = entry?.file_name();
            let name = name.to_string_lossy();
            Ok::<_, io::Error>(
                found
                    || name == base_name
                    || name.starts_with(&prefix)
                    || name.starts_with(&dot_prefix),
            )
        })?;
        for index in 0..=1000 {
            check_cancelled(cancelled)?;
            let name = if index == 0 && !collision {
                base_name.clone()
            } else {
                format!(
                    "{stem}_{}{extension}",
                    if collision { index } else { index - 1 }
                )
            };
            let candidate = samples.join(name);
            let mut writer = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            self.original_path = candidate;
            self.owned_original = true;
            let mut snapshot = self.snapshot_file()?;
            let (digest, bytes) = copy_hashed(&mut snapshot, &mut writer, cancelled)?;
            drop(writer);
            if digest != self.source_digest || bytes != self.source_bytes {
                return Err(invalid("project original differs from immutable snapshot"));
            }
            let mut reader = sealed_reader(&self.original_path)?;
            verify_file(&mut reader, &digest, bytes, cancelled)?;
            self.original_reader = Some(reader);
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "project original name collisions exhausted",
        ))
    }

    pub(super) fn commit(&mut self, cancelled: &impl Fn() -> bool) -> io::Result<PathBuf> {
        check_cancelled(cancelled)?;
        let manifest = self
            .manifest
            .as_ref()
            .ok_or_else(|| invalid("incomplete cold artifacts"))?
            .clone();
        if self.committed_cache.is_some() {
            return Err(invalid("cold transaction already committed"));
        }
        self.create_original(cancelled)?;
        // Snapshot is not a durable PCM entry. All decoder reads have completed.
        self.snapshot.take();
        fs::remove_file(&self.snapshot_path)?;
        let staging = self
            .staging
            .as_ref()
            .ok_or_else(|| invalid("missing exclusive staging"))?;
        // Windows prohibits renaming a directory with descendants opened without
        // FILE_SHARE_DELETE. Close only after verification, immediately before rename;
        // reopen and verify committed artifacts before allowing pad publication.
        self.artifact_readers.clear();
        let generation = staging
            .file_name()
            .ok_or_else(|| invalid("missing staging generation"))?
            .to_string_lossy();
        let destination = self.cache_root.join(format!(
            "{}-{}",
            manifest.identity,
            generation.trim_start_matches(".staging-")
        ));
        check_cancelled(cancelled)?;
        if destination.try_exists()? {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "cold destination already exists",
            ));
        }
        fs::rename(staging, &destination)?;
        self.committed_cache = Some(destination.clone());
        self.staging = None;
        self.verify_committed(cancelled)?;
        self.artifact_readers.reserve(1);
        self.directories.reserve(1);
        self.verified = true;
        Ok(destination)
    }

    fn verify_committed(&mut self, cancelled: &impl Fn() -> bool) -> io::Result<()> {
        let destination = self
            .committed_cache
            .as_ref()
            .ok_or_else(|| invalid("missing committed PCM directory"))?;
        reject_links(destination)?;
        self.committed_directory = Some(directory_guard(destination)?);
        let manifest = self
            .manifest
            .as_ref()
            .ok_or_else(|| invalid("missing manifest"))?;
        for (file_name, descriptor) in [
            ("decoder.f32le", &manifest.descriptor["decoder"]["pcm"]),
            ("playback.f32le", &manifest.descriptor["playback"]["pcm"]),
        ] {
            let path = destination.join(file_name);
            reject_links(&path)?;
            let mut reader = sealed_reader(&path)?;
            verify_file(
                &mut reader,
                descriptor["interleaved_sha256"]
                    .as_str()
                    .ok_or_else(|| invalid("missing PCM digest"))?,
                descriptor["full_bytes"]
                    .as_u64()
                    .ok_or_else(|| invalid("missing PCM bytes"))?,
                cancelled,
            )?;
            self.artifact_readers.push(reader);
        }
        let expected =
            serde_json::to_vec(&canonical(&manifest.encoded())).map_err(io::Error::other)?;
        let manifest_path = destination.join("manifest.json");
        reject_links(&manifest_path)?;
        let mut reader = sealed_reader(&manifest_path)?;
        verify_file(
            &mut reader,
            &format!("{:x}", Sha256::digest(&expected)),
            expected.len() as u64,
            cancelled,
        )?;
        self.artifact_readers.push(reader);
        Ok(())
    }

    /// A successful commit establishes every invariant before native queue admission.
    /// Adoption only moves preallocated ownership and cannot return a late failure.
    pub(super) fn into_lease(mut self) -> CommittedColdLease {
        assert!(self.verified && self.artifact_readers.len() == 3);
        let manifest = self.manifest.take().expect("verified cold manifest");
        let cache_path = self
            .committed_cache
            .take()
            .expect("verified cold directory");
        let original = self
            .original_reader
            .take()
            .expect("verified original reader");
        let mut readers = std::mem::take(&mut self.artifact_readers);
        readers.push(original);
        let mut directories = std::mem::take(&mut self.directories);
        directories.push(
            self.committed_directory
                .take()
                .expect("verified directory guard"),
        );
        self.adopted = true;
        CommittedColdLease {
            manifest,
            cache_path,
            original_path: std::mem::take(&mut self.original_path),
            _readers: readers,
            _directories: directories,
        }
    }
}

impl Drop for ColdTransaction {
    fn drop(&mut self) {
        if self.adopted {
            return;
        }
        self.snapshot.take();
        self.source_reader.take();
        self.original_reader.take();
        self.artifact_readers.clear();
        self.committed_directory.take();
        if let Some(path) = self.staging.take() {
            let _ = fs::remove_dir_all(path);
        }
        if let Some(path) = self.committed_cache.take() {
            let _ = fs::remove_dir_all(path);
        }
        if self.owned_original {
            let _ = fs::remove_file(&self.original_path);
        }
        self.directories.clear();
    }
}

#[cfg(test)]
mod tests;
