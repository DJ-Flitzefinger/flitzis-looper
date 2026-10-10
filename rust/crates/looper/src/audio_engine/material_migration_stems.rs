//! Verified, copy-only WAV migration. Publication and persistent aligned PCM are separate.

use super::cold_store::sealed_reader;
use super::material_migration::PreparedMigrationMaterial;
use super::material_paths::{self, AssetKind};
use super::project_assets::{self, FileIdentity, file_identity};
use super::stem_cache::{STEM_FILE_NAMES, prepare_stem_buffers_from_cache_at_project_root};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const CHUNK_BYTES: usize = 64 * 1024;
const MAX_MARKER_BYTES: u64 = 256 * 1024;
const MAX_CANONICAL_STEM_ENTRIES: usize = 256;
const MARKER: &str = ".complete.json";

#[derive(Debug)]
pub(super) struct CopiedStemGeneration {
    pub cache_reference: String,
    pub created: bool,
}

fn check_cancelled(cancelled: &impl Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("stem migration cancelled".into())
    } else {
        Ok(())
    }
}

fn persisted(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "stem migration escaped samples root")?;
    Ok(format!(
        "samples/{}",
        relative.to_string_lossy().replace('\\', "/")
    ))
}

fn digest(reader: &mut File, cancelled: &impl Fn() -> bool) -> Result<(String, u64), String> {
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut bytes = [0; CHUNK_BYTES];
    let mut total = 0_u64;
    loop {
        check_cancelled(cancelled)?;
        let count = reader.read(&mut bytes).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
        total = total
            .checked_add(count as u64)
            .ok_or("stem byte extent overflow")?;
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    check_cancelled(cancelled)?;
    Ok((format!("{:x}", hash.finalize()), total))
}

fn verify_source(
    root: &Path,
    expected_path: &Path,
    expected_digest: &str,
    expected_bytes: u64,
    version: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<(Vec<File>, File), String> {
    let (path, claimed_digest) = version
        .rsplit_once("|sha256-v1:")
        .ok_or("stem source version has no complete original digest")?;
    if claimed_digest != expected_digest || claimed_digest.len() != 64 {
        return Err("stem source version does not match prepared original digest".into());
    }
    let resolved =
        material_paths::resolve(root, Path::new(path)).map_err(|error| error.to_string())?;
    if !matches!(resolved.kind, AssetKind::Original { .. }) || resolved.path != expected_path {
        return Err("stem source version does not match actual typed original path".into());
    }
    let guards =
        project_assets::directory_guards(expected_path.parent().ok_or("original parent missing")?)
            .map_err(|error| error.to_string())?;
    project_assets::reject_links(expected_path).map_err(|error| error.to_string())?;
    let mut reader = sealed_reader(expected_path).map_err(|error| error.to_string())?;
    if digest(&mut reader, cancelled)? != (expected_digest.to_owned(), expected_bytes) {
        return Err("stem source original differs from verified material".into());
    }
    Ok((guards, reader))
}

pub(super) struct SealedSet {
    pub(super) _guards: Vec<File>,
    pub(super) _files: Vec<File>,
    identities: Vec<FileIdentity>,
    pub(super) marker: Value,
}

fn read_marker(directory: &Path) -> Result<(File, Value), String> {
    let marker_path = directory.join(MARKER);
    project_assets::reject_links(&marker_path).map_err(|error| error.to_string())?;
    let mut marker_reader = sealed_reader(&marker_path).map_err(|error| error.to_string())?;
    let marker_bytes = marker_reader
        .metadata()
        .map_err(|error| error.to_string())?
        .len();
    if marker_bytes == 0 || marker_bytes > MAX_MARKER_BYTES {
        return Err("stem complete marker exceeds bounded schema extent".into());
    }
    let mut bytes = vec![0; marker_bytes as usize];
    marker_reader
        .read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    if marker_reader
        .read(&mut [0])
        .map_err(|error| error.to_string())?
        != 0
    {
        return Err("stem complete marker EOF mismatch".into());
    }
    let marker: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    Ok((marker_reader, marker))
}

fn verify_set(
    directory: &Path,
    source_version: &str,
    maximum_wav_bytes: u64,
    cancelled: &impl Fn() -> bool,
) -> Result<SealedSet, String> {
    let guards = project_assets::directory_guards(directory).map_err(|error| error.to_string())?;
    let (marker_reader, marker) = read_marker(directory)?;
    let mut hashes = serde_json::Map::new();
    let mut files = Vec::with_capacity(6);
    let mut identities = Vec::with_capacity(6);
    for name in STEM_FILE_NAMES {
        check_cancelled(cancelled)?;
        let path = directory.join(format!("{name}.wav"));
        project_assets::reject_links(&path).map_err(|error| error.to_string())?;
        let mut reader = sealed_reader(&path).map_err(|error| error.to_string())?;
        if reader.metadata().map_err(|error| error.to_string())?.len() > maximum_wav_bytes {
            return Err("stem WAV exceeds complete geometry input bound".into());
        }
        let (hash, _) = digest(&mut reader, cancelled)?;
        hashes.insert(name.into(), json!(hash));
        identities.push(file_identity(&reader).map_err(|error| error.to_string())?);
        files.push(reader);
    }
    let expected =
        json!({"schema":"stem-set-sha256-v1", "source_version":source_version, "stems":hashes});
    if marker != expected {
        return Err("stem marker/source/five complete digests mismatch".into());
    }
    identities.push(file_identity(&marker_reader).map_err(|error| error.to_string())?);
    files.push(marker_reader);
    Ok(SealedSet {
        _guards: guards,
        _files: files,
        identities,
        marker,
    })
}

/// Reuse the same complete five-SHA/marker verifier for aligned pair preparation.
pub(super) fn verify_complete_wav_set(
    directory: &Path,
    source_version: &str,
    maximum_wav_bytes: u64,
    cancelled: &impl Fn() -> bool,
) -> Result<SealedSet, String> {
    exact_ready_files(directory)?;
    verify_set(directory, source_version, maximum_wav_bytes, cancelled)
}

struct OwnedGeneration {
    path: PathBuf,
    identity: FileIdentity,
    files: Vec<(String, FileIdentity)>,
    guards: Vec<File>,
    readers: Vec<File>,
    accepted: bool,
}

impl Drop for OwnedGeneration {
    fn drop(&mut self) {
        if self.accepted {
            return;
        }
        self.readers.clear();
        self.guards.clear();
        let Some(parent) = self.path.parent() else {
            return;
        };
        let Ok(_guards) = project_assets::directory_guards(parent) else {
            return;
        };
        let Ok(generation_guards) = project_assets::directory_guards(&self.path) else {
            return;
        };
        if generation_guards
            .first()
            .and_then(|guard| file_identity(guard).ok())
            .as_ref()
            != Some(&self.identity)
        {
            return;
        }
        for (name, identity) in &self.files {
            // Exact handle identities only; unknown/replaced leaves are preserved.
            let _ = project_assets::remove_owned_file(&self.path.join(name), Some(identity));
        }
        drop(generation_guards);
        let _ = project_assets::remove_empty_directory(&self.path, &self.identity);
    }
}

fn validate_geometry(
    root: &Path,
    directory: &Path,
    material: &PreparedMigrationMaterial,
    version: &str,
    rate: u32,
) -> Result<(), String> {
    let reference = persisted(root, directory)?;
    let project_root = root.parent().ok_or("samples project root missing")?;
    let prepared = prepare_stem_buffers_from_cache_at_project_root(
        version,
        &material.sample,
        rate,
        &reference,
        project_root,
    )?;
    drop(prepared);
    Ok(())
}

fn exact_ready_files(directory: &Path) -> Result<(), String> {
    let mut count = 0;
    for entry in fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .take(7)
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
            || !std::iter::once(MARKER.to_owned())
                .chain(STEM_FILE_NAMES.map(|stem| format!("{stem}.wav")))
                .any(|expected| name == std::ffi::OsStr::new(&expected))
        {
            return Err("unknown target generation contents preserve collision".into());
        }
        count += 1;
    }
    if count != 6 {
        return Err("target generation is not exactly one complete set".into());
    }
    Ok(())
}

pub(super) fn copy_stem_generation(
    samples_root: &Path,
    material: &PreparedMigrationMaterial,
    old_cache: &Path,
    old_source_version: &str,
    new_source_version: &str,
    generation_id: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<CopiedStemGeneration, String> {
    check_cancelled(cancelled)?;
    if !material_paths::valid_id(generation_id) {
        return Err("stem generation ID must be 32 lowercase hex characters".into());
    }
    if !samples_root.is_absolute()
        || samples_root
            .to_string_lossy()
            .split(['/', '\\'])
            .any(|part| [".", ".."].contains(&part))
    {
        return Err("stem migration requires an absolute rooted samples directory".into());
    }
    let _root_guards =
        project_assets::directory_guards(samples_root).map_err(|error| error.to_string())?;
    let root = fs::canonicalize(samples_root).map_err(|error| error.to_string())?;
    let old = material_paths::resolve(&root, old_cache).map_err(|error| error.to_string())?;
    if !matches!(old.kind, AssetKind::StemDirectory { .. })
        || old
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".generation-"))
    {
        return Err("migration requires a typed published old stem set".into());
    }
    material
        .lease
        .verify_reference(&material.sample)
        .map_err(|error| error.to_string())?;
    let material_id = material
        .lease
        .material_id
        .as_deref()
        .ok_or("migration material ID missing")?;
    let new_original = material_paths::resolve(&root, &material.lease.original_path)
        .map_err(|error| error.to_string())?;
    if new_original.kind
        != (AssetKind::Original {
            material: Some(material_id.to_owned()),
        })
    {
        return Err("migration new original material binding mismatch".into());
    }
    let original = &material.lease.manifest.descriptor["decoder"]["original"];
    let original_digest = original["sha256"]
        .as_str()
        .ok_or("material original digest missing")?;
    let original_bytes = original["bytes"]
        .as_u64()
        .ok_or("material original bytes missing")?;
    // Interrupted configs can already contain a canonical subscriber alongside
    // a legacy one. Each set binds its own actual typed source and full bytes.
    let old_reference = old_source_version
        .rsplit_once("|sha256-v1:")
        .ok_or("old source version has no complete digest binding")?
        .0;
    let old_original = material_paths::resolve(&root, Path::new(old_reference))
        .map_err(|error| error.to_string())?;
    let _old_source = verify_source(
        &root,
        &old_original.path,
        original_digest,
        original_bytes,
        old_source_version,
        cancelled,
    )?;
    let _new_source = verify_source(
        &root,
        &new_original.path,
        original_digest,
        original_bytes,
        new_source_version,
        cancelled,
    )?;
    let rate = u32::try_from(
        material.lease.manifest.descriptor["playback"]["pcm"]["rate_hz"]
            .as_u64()
            .ok_or("material playback rate missing")?,
    )
    .map_err(|_| "material playback rate overflow")?;
    if super::stem_cache::admitted_stem_pcm_bytes(
        material.sample.frame_count(),
        material.sample.channels,
    )? > super::cold_jobs::PCM_LIMIT_BYTES
    {
        return Err("stem migration exceeds complete transient PCM admission".into());
    }
    let maximum_wav_bytes = material
        .sample
        .frame_count()
        .checked_mul(material.sample.channels)
        .and_then(|count| count.checked_mul(2))
        .and_then(|bytes| bytes.checked_add(1024 * 1024))
        .ok_or("stem migration input geometry overflow")? as u64;
    let old_set = verify_set(&old.path, old_source_version, maximum_wav_bytes, cancelled)?;
    validate_geometry(&root, &old.path, material, old_source_version, rate)?;
    check_cancelled(cancelled)?;
    let marker = json!({"schema":"stem-set-sha256-v1", "source_version":new_source_version, "stems":old_set.marker["stems"]});
    let material_root = new_original
        .path
        .parent()
        .and_then(Path::parent)
        .ok_or("material root missing")?;
    let _material_guards =
        project_assets::directory_guards(material_root).map_err(|error| error.to_string())?;
    let stems = material_root.join("stems");
    match fs::create_dir(&stems) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.to_string()),
    }
    let _parents = project_assets::directory_guards(&stems).map_err(|error| error.to_string())?;
    let _preparation = material
        .lease
        .stem_preparation_gate(cancelled)
        .map_err(|error| error.to_string())?;
    let ready = stems.join(format!(".ready-{generation_id}"));
    if ready.try_exists().map_err(|error| error.to_string())? {
        exact_ready_files(&ready)?;
        let existing = verify_set(&ready, new_source_version, maximum_wav_bytes, cancelled)?;
        if existing.marker != marker {
            return Err("canonical stem generation lineage collision".into());
        }
        validate_geometry(&root, &ready, material, new_source_version, rate)?;
        check_cancelled(cancelled)?;
        return Ok(CopiedStemGeneration {
            cache_reference: persisted(&root, &ready)?,
            created: false,
        });
    }
    // Keep the requested-ID collision rule above even when another generation
    // matches. Sibling reuse is material/set identity, never a slot or UUID hint.
    let entries = fs::read_dir(&stems)
        .map_err(|error| error.to_string())?
        .take(MAX_CANONICAL_STEM_ENTRIES + 1)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if entries.len() > MAX_CANONICAL_STEM_ENTRIES {
        return Err("canonical stem candidate admission capacity full".into());
    }
    for entry in &entries {
        check_cancelled(cancelled)?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(material_paths::ready_generation)
        {
            continue;
        }
        let candidate = entry.path();
        let compatible = (|| -> Result<Option<SealedSet>, String> {
            let resolved =
                material_paths::resolve(&root, &candidate).map_err(|error| error.to_string())?;
            if !matches!(&resolved.kind, AssetKind::StemDirectory { material: Some(id), .. } if id == material_id)
            {
                return Ok(None);
            }
            let _candidate_guards = project_assets::directory_guards(&resolved.path)
                .map_err(|error| error.to_string())?;
            exact_ready_files(&resolved.path)?;
            // A sealed bounded marker filters different valid sets before their
            // full WAV reads. Matching candidates still receive all five hashes
            // and the authoritative central decoder/geometry/EOF verifier.
            let (_marker_reader, candidate_marker) = read_marker(&resolved.path)?;
            if candidate_marker != marker {
                return Ok(None);
            }
            let existing = verify_set(
                &resolved.path,
                new_source_version,
                maximum_wav_bytes,
                cancelled,
            )?;
            if existing.marker != marker {
                return Ok(None);
            }
            validate_geometry(&root, &resolved.path, material, new_source_version, rate)?;
            check_cancelled(cancelled)?;
            Ok(Some(existing))
        })();
        check_cancelled(cancelled)?;
        if let Ok(Some(_existing)) = compatible {
            return Ok(CopiedStemGeneration {
                cache_reference: persisted(&root, &candidate)?,
                created: false,
            });
        }
        // Unknown/replaced/extra/incomplete generations remain untouched and
        // never become an adopted cache or a deletion/rollback target.
    }
    if entries.len() == MAX_CANONICAL_STEM_ENTRIES {
        return Err("canonical stem candidate admission capacity full".into());
    }
    let staging = stems.join(format!(".generation-{generation_id}"));
    fs::create_dir(&staging).map_err(|error| error.to_string())?;
    let guards = project_assets::directory_guards(&staging).map_err(|error| error.to_string())?;
    let identity = file_identity(guards.first().ok_or("stem staging guard missing")?)
        .map_err(|error| error.to_string())?;
    let mut owned = OwnedGeneration {
        path: staging,
        identity,
        files: Vec::with_capacity(6),
        guards,
        readers: Vec::with_capacity(6),
        accepted: false,
    };
    for (index, name) in STEM_FILE_NAMES.into_iter().enumerate() {
        check_cancelled(cancelled)?;
        let leaf = format!("{name}.wav");
        let mut input = sealed_reader(&old.path.join(&leaf)).map_err(|error| error.to_string())?;
        if file_identity(&input).map_err(|error| error.to_string())? != old_set.identities[index] {
            return Err("old stem FileID changed after complete verification".into());
        }
        let path = owned.path.join(&leaf);
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        owned.files.push((
            leaf,
            file_identity(&output).map_err(|error| error.to_string())?,
        ));
        let mut bytes = [0; CHUNK_BYTES];
        loop {
            check_cancelled(cancelled)?;
            let count = input.read(&mut bytes).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            output
                .write_all(&bytes[..count])
                .map_err(|error| error.to_string())?;
        }
        output.sync_all().map_err(|error| error.to_string())?;
        drop(output);
        let mut reopened = sealed_reader(&path).map_err(|error| error.to_string())?;
        if digest(&mut reopened, cancelled)?.0
            != old_set.marker["stems"][name]
                .as_str()
                .ok_or("old complete stem digest missing")?
        {
            return Err("copied stem differs from verified immutable WAV".into());
        }
        owned.readers.push(reopened);
    }
    check_cancelled(cancelled)?;
    let marker_path = owned.path.join(MARKER);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker_path)
        .map_err(|error| error.to_string())?;
    owned.files.push((
        MARKER.into(),
        file_identity(&output).map_err(|error| error.to_string())?,
    ));
    output
        .write_all(&serde_json::to_vec(&marker).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    output.sync_all().map_err(|error| error.to_string())?;
    drop(output);
    let marker_reader = sealed_reader(&marker_path).map_err(|error| error.to_string())?;
    owned.readers.push(marker_reader);
    check_cancelled(cancelled)?;
    // Windows rename requires closing this generation's sealed descendants.
    // Parent guards and the old set remain held. Reopened identities must match.
    owned.readers.clear();
    owned.guards.clear();
    if ready.try_exists().map_err(|error| error.to_string())? {
        return Err("canonical stem generation already exists".into());
    }
    fs::rename(&owned.path, &ready).map_err(|error| error.to_string())?;
    owned.path = ready;
    owned.guards =
        project_assets::directory_guards(&owned.path).map_err(|error| error.to_string())?;
    if file_identity(owned.guards.first().ok_or("stem ready guard missing")?)
        .map_err(|error| error.to_string())?
        != owned.identity
    {
        return Err("stem generation directory replaced during rename".into());
    }
    let reopened = verify_set(
        &owned.path,
        new_source_version,
        maximum_wav_bytes,
        cancelled,
    )?;
    if reopened.marker != marker
        || reopened
            .identities
            .iter()
            .zip(&owned.files)
            .any(|(actual, (_, expected))| actual != expected)
    {
        return Err("committed stem generation identity changed".into());
    }
    exact_ready_files(&owned.path)?;
    validate_geometry(&root, &owned.path, material, new_source_version, rate)?;
    check_cancelled(cancelled)?;
    let reference = persisted(&root, &owned.path)?;
    owned.accepted = true;
    Ok(CopiedStemGeneration {
        cache_reference: reference,
        created: true,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::audio_engine::material_migration::prepare_material;
    use std::cell::Cell;

    const GENERATION: &str = "0123456789abcdef0123456789abcdef";

    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        old: PathBuf,
        material: PreparedMigrationMaterial,
        old_version: String,
        new_version: String,
    }

    fn write_wav(path: &Path, rate: u32, frames: u32) {
        let mut file = File::create(path).unwrap();
        let bytes = frames * 4;
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(bytes + 36).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x02\0").unwrap();
        file.write_all(&rate.to_le_bytes()).unwrap();
        file.write_all(&(rate * 4).to_le_bytes()).unwrap();
        file.write_all(&4_u16.to_le_bytes()).unwrap();
        file.write_all(&16_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&bytes.to_le_bytes()).unwrap();
        for frame in 0..frames {
            file.write_all(&((frame as i16 - 32) * 32).to_le_bytes())
                .unwrap();
            file.write_all(&((32 - frame as i16) * 16).to_le_bytes())
                .unwrap();
        }
    }

    fn write_marker(directory: &Path, source_version: &str) {
        let hashes = STEM_FILE_NAMES
            .into_iter()
            .map(|stem| {
                let bytes = fs::read(directory.join(format!("{stem}.wav"))).unwrap();
                (
                    stem.to_owned(),
                    json!(format!("{:x}", Sha256::digest(bytes))),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        fs::write(
            directory.join(MARKER),
            serde_json::to_vec(&json!({
                "schema":"stem-set-sha256-v1", "source_version":source_version, "stems":hashes
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn duplicate_legacy_directory(fixture: &Fixture) -> PathBuf {
        let other = fixture.root.join("stems/#216");
        fs::create_dir(&other).unwrap();
        for leaf in std::iter::once(MARKER.to_owned())
            .chain(STEM_FILE_NAMES.map(|stem| format!("{stem}.wav")))
        {
            fs::copy(fixture.old.join(&leaf), other.join(leaf)).unwrap();
        }
        other
    }

    fn copy_from(fixture: &Fixture, source: &Path, generation: &str) -> CopiedStemGeneration {
        copy_stem_generation(
            &fixture.root,
            &fixture.material,
            source,
            &fixture.old_version,
            &fixture.new_version,
            generation,
            &|| false,
        )
        .unwrap()
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("samples");
            let old = root.join("stems/#1");
            fs::create_dir_all(&old).unwrap();
            let original = root.join("old.wav");
            write_wav(&original, 48_000, 64);
            let material = prepare_material(&root, &original, 48_000, 2, &|| false).unwrap();
            let sha = format!("{:x}", Sha256::digest(fs::read(&original).unwrap()));
            let old_version = format!("samples/old.wav|sha256-v1:{sha}");
            let new_version = format!(
                "{}|sha256-v1:{sha}",
                material.metadata()["new_reference"].as_str().unwrap()
            );
            for stem in STEM_FILE_NAMES {
                write_wav(&old.join(format!("{stem}.wav")), 48_000, 64);
            }
            let fixture = Self {
                _temp: temp,
                root,
                old,
                material,
                old_version,
                new_version,
            };
            fixture.marker();
            fixture
        }

        fn marker(&self) {
            write_marker(&self.old, &self.old_version);
        }

        fn stems_root(&self) -> PathBuf {
            self.material
                .lease
                .original_path
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("stems")
        }

        fn copy(&self) -> Result<CopiedStemGeneration, String> {
            copy_stem_generation(
                &self.root,
                &self.material,
                &self.old,
                &self.old_version,
                &self.new_version,
                GENERATION,
                &|| false,
            )
        }
    }

    #[test]
    fn complete_wav_migration_binds_new_marker_preserves_old_and_reuses_same_generation() {
        let fixture = Fixture::new();
        let old_marker = fs::read(fixture.old.join(MARKER)).unwrap();
        let old_wavs =
            STEM_FILE_NAMES.map(|stem| fs::read(fixture.old.join(format!("{stem}.wav"))).unwrap());
        let copied = fixture.copy().unwrap();
        assert!(copied.created);
        let reference = copied.cache_reference;
        let ready = material_paths::resolve(&fixture.root, Path::new(&reference))
            .unwrap()
            .path;
        assert!(reference.starts_with("samples/materials/M"));
        assert!(reference.ends_with(&format!("stems/.ready-{GENERATION}")));
        assert_eq!(fs::read_dir(&ready).unwrap().count(), 6);
        let marker: Value = serde_json::from_slice(&fs::read(ready.join(MARKER)).unwrap()).unwrap();
        assert_eq!(marker["source_version"], fixture.new_version);
        assert_eq!(marker["schema"], "stem-set-sha256-v1");
        for (index, name) in STEM_FILE_NAMES.into_iter().enumerate() {
            assert_eq!(
                fs::read(ready.join(format!("{name}.wav"))).unwrap(),
                old_wavs[index]
            );
            assert_eq!(
                fs::read(fixture.old.join(format!("{name}.wav"))).unwrap(),
                old_wavs[index]
            );
            assert_eq!(
                marker["stems"][name],
                format!("{:x}", Sha256::digest(&old_wavs[index]))
            );
        }
        assert_eq!(fs::read(fixture.old.join(MARKER)).unwrap(), old_marker);
        let identity =
            file_identity(&project_assets::directory_guards(&ready).unwrap()[0]).unwrap();
        let reused = fixture.copy().unwrap();
        assert!(!reused.created);
        assert_eq!(reused.cache_reference, reference);
        assert_eq!(
            file_identity(&project_assets::directory_guards(&ready).unwrap()[0]).unwrap(),
            identity
        );
        assert_eq!(fs::read_dir(fixture.stems_root()).unwrap().count(), 1);
        assert!(
            !fixture
                .stems_root()
                .join(format!(".generation-{GENERATION}"))
                .exists()
        );
    }

    #[test]
    fn distinct_legacy_slots_and_private_ids_reuse_one_verified_canonical_set() {
        let fixture = Fixture::new();
        let last = duplicate_legacy_directory(&fixture);
        let old_marker = fs::read(fixture.old.join(MARKER)).unwrap();
        let first = copy_from(&fixture, &fixture.old, GENERATION);
        let ready = material_paths::resolve(&fixture.root, Path::new(&first.cache_reference))
            .unwrap()
            .path;
        let identity =
            file_identity(&project_assets::directory_guards(&ready).unwrap()[0]).unwrap();
        let second_id = "11111111111111111111111111111111";
        let last_copy = copy_from(&fixture, &last, second_id);
        assert!(first.created);
        assert!(!last_copy.created);
        assert_eq!(last_copy.cache_reference, first.cache_reference);
        assert_eq!(
            file_identity(&project_assets::directory_guards(&ready).unwrap()[0]).unwrap(),
            identity
        );
        assert_eq!(fs::read_dir(fixture.stems_root()).unwrap().count(), 1);
        assert!(
            !fixture
                .stems_root()
                .join(format!(".ready-{second_id}"))
                .exists()
        );
        assert!(
            !fixture
                .stems_root()
                .join(format!(".generation-{second_id}"))
                .exists()
        );
        assert_eq!(fs::read(&fixture.old.join(MARKER)).unwrap(), old_marker);
        assert_eq!(fs::read(last.join(MARKER)).unwrap(), old_marker);
        for stem in STEM_FILE_NAMES {
            let leaf = format!("{stem}.wav");
            assert_eq!(
                fs::read(ready.join(&leaf)).unwrap(),
                fs::read(last.join(&leaf)).unwrap()
            );
            assert_eq!(
                fs::read(fixture.old.join(&leaf)).unwrap(),
                fs::read(last.join(leaf)).unwrap()
            );
        }
    }

    #[test]
    fn different_valid_set_bytes_keep_distinct_canonical_generations() {
        let fixture = Fixture::new();
        let last = duplicate_legacy_directory(&fixture);
        let vocals = last.join("vocals.wav");
        let mut changed = fs::read(&vocals).unwrap();
        changed[45] ^= 1; // Valid PCM16/geometry, but a different immutable set.
        fs::write(&vocals, &changed).unwrap();
        write_marker(&last, &fixture.old_version);
        let first = copy_from(&fixture, &fixture.old, GENERATION);
        let second = copy_from(&fixture, &last, "22222222222222222222222222222222");
        assert!(first.created && second.created);
        assert_ne!(first.cache_reference, second.cache_reference);
        assert_eq!(fs::read_dir(fixture.stems_root()).unwrap().count(), 2);
        let first_path = material_paths::resolve(&fixture.root, Path::new(&first.cache_reference))
            .unwrap()
            .path;
        let second_path =
            material_paths::resolve(&fixture.root, Path::new(&second.cache_reference))
                .unwrap()
                .path;
        assert_eq!(
            fs::read(first_path.join("vocals.wav")).unwrap(),
            fs::read(fixture.old.join("vocals.wav")).unwrap()
        );
        assert_eq!(fs::read(second_path.join("vocals.wav")).unwrap(), changed);
        let reuse = copy_from(&fixture, &last, "33333333333333333333333333333333");
        assert!(!reuse.created);
        assert_eq!(reuse.cache_reference, second.cache_reference);
        assert_eq!(fs::read_dir(fixture.stems_root()).unwrap().count(), 2);
    }

    #[test]
    fn concurrent_verified_equal_legacy_sets_publish_exactly_one_canonical_generation() {
        let fixture = Fixture::new();
        let last = duplicate_legacy_directory(&fixture);
        let start = std::sync::Barrier::new(2);
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                start.wait();
                copy_from(&fixture, &fixture.old, GENERATION)
            });
            let second = scope.spawn(|| {
                start.wait();
                copy_from(&fixture, &last, "44444444444444444444444444444444")
            });
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_ne!(first.created, second.created);
        assert_eq!(first.cache_reference, second.cache_reference);
        assert_eq!(fs::read_dir(fixture.stems_root()).unwrap().count(), 1);
        let ready = material_paths::resolve(&fixture.root, Path::new(&first.cache_reference))
            .unwrap()
            .path;
        assert_eq!(fs::read_dir(&ready).unwrap().count(), 6);
        for stem in STEM_FILE_NAMES {
            let leaf = format!("{stem}.wav");
            assert_eq!(
                fs::read(ready.join(&leaf)).unwrap(),
                fs::read(last.join(&leaf)).unwrap()
            );
            assert_eq!(
                fs::read(fixture.old.join(&leaf)).unwrap(),
                fs::read(last.join(leaf)).unwrap()
            );
        }
    }

    #[test]
    fn bounded_candidate_scan_preserves_unknown_and_incomplete_generations() {
        for damage in ["extra", "missing", "hash", "geometry"] {
            let fixture = Fixture::new();
            let prior = fixture.copy().unwrap();
            let bad = material_paths::resolve(&fixture.root, Path::new(&prior.cache_reference))
                .unwrap()
                .path;
            match damage {
                "extra" => fs::write(bad.join("private.keep"), "unknown owner").unwrap(),
                "missing" => fs::remove_file(bad.join("melody.wav")).unwrap(),
                "hash" => {
                    let mut bytes = fs::read(bad.join("vocals.wav")).unwrap();
                    bytes[45] ^= 1;
                    fs::write(bad.join("vocals.wav"), bytes).unwrap();
                }
                "geometry" => {
                    write_wav(&bad.join("vocals.wav"), 44_100, 64);
                    write_marker(&bad, &fixture.new_version);
                }
                _ => unreachable!(),
            }
            let preserved = fs::read_dir(&bad)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    (entry.file_name(), fs::read(entry.path()).unwrap())
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            let fresh = copy_from(&fixture, &fixture.old, "55555555555555555555555555555555");
            assert!(
                fresh.created,
                "{damage} candidate must not gain reuse authority"
            );
            assert_ne!(fresh.cache_reference, prior.cache_reference);
            let after = fs::read_dir(&bad)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    (entry.file_name(), fs::read(entry.path()).unwrap())
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(after, preserved);
            assert_eq!(fs::read_dir(fixture.stems_root()).unwrap().count(), 2);
        }
        let fixture = Fixture::new();
        fs::create_dir(fixture.stems_root()).unwrap();
        for ordinal in 0..MAX_CANONICAL_STEM_ENTRIES {
            fs::write(
                fixture.stems_root().join(format!("unknown-{ordinal}")),
                "foreign",
            )
            .unwrap();
        }
        assert!(fixture.copy().unwrap_err().contains("capacity full"));
        assert_eq!(
            fs::read_dir(fixture.stems_root()).unwrap().count(),
            MAX_CANONICAL_STEM_ENTRIES
        );
        assert!(
            !fixture
                .stems_root()
                .join(format!(".ready-{GENERATION}"))
                .exists()
        );
        assert!(
            !fixture
                .stems_root()
                .join(format!(".generation-{GENERATION}"))
                .exists()
        );
        for ordinal in 0..MAX_CANONICAL_STEM_ENTRIES {
            assert_eq!(
                fs::read(fixture.stems_root().join(format!("unknown-{ordinal}"))).unwrap(),
                b"foreign"
            );
        }
    }

    #[test]
    fn invalid_five_wav_integrity_rate_frames_or_missing_file_rejects_before_target_write() {
        for damage in ["hash", "rate", "frames", "missing", "marker"] {
            let fixture = Fixture::new();
            let vocals = fixture.old.join("vocals.wav");
            match damage {
                "hash" => {
                    let mut bytes = fs::read(&vocals).unwrap();
                    bytes[45] ^= 1;
                    fs::write(&vocals, bytes).unwrap();
                }
                "rate" => {
                    write_wav(&vocals, 44_100, 64);
                    fixture.marker();
                }
                "frames" => {
                    write_wav(&vocals, 48_000, 63);
                    fixture.marker();
                }
                "missing" => {
                    fs::remove_file(fixture.old.join("bass.wav")).unwrap();
                }
                "marker" => {
                    let mut marker: Value =
                        serde_json::from_slice(&fs::read(fixture.old.join(MARKER)).unwrap())
                            .unwrap();
                    marker["stems"]["unexpected"] = json!("0".repeat(64));
                    fs::write(
                        fixture.old.join(MARKER),
                        serde_json::to_vec(&marker).unwrap(),
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            let preserved = fs::read(&vocals).unwrap();
            assert!(
                fixture.copy().is_err(),
                "{damage} must fail complete verification"
            );
            assert_eq!(fs::read(&vocals).unwrap(), preserved);
            assert!(
                !fixture.stems_root().exists(),
                "{damage} wrote target before verification"
            );
        }
    }

    #[test]
    fn source_hash_path_generation_and_unpublished_old_set_cannot_authorize_copy() {
        let fixture = Fixture::new();
        let wrong_hash = format!("samples/old.wav|sha256-v1:{}", "0".repeat(64));
        let wrong_path = fixture
            .old_version
            .replace("samples/old.wav", "samples/other.wav");
        for version in [
            wrong_hash,
            wrong_path,
            "old-string-with-no-complete-hash".into(),
        ] {
            assert!(
                copy_stem_generation(
                    &fixture.root,
                    &fixture.material,
                    &fixture.old,
                    &version,
                    &fixture.new_version,
                    GENERATION,
                    &|| false
                )
                .is_err()
            );
        }
        assert!(
            copy_stem_generation(
                &fixture.root,
                &fixture.material,
                &fixture.old,
                &fixture.old_version,
                &fixture.old_version,
                GENERATION,
                &|| false
            )
            .is_err()
        );
        assert!(
            copy_stem_generation(
                &fixture.root,
                &fixture.material,
                &fixture.root.join("stems/#1/../../outside"),
                &fixture.old_version,
                &fixture.new_version,
                GENERATION,
                &|| false
            )
            .is_err()
        );
        assert!(
            copy_stem_generation(
                &fixture.root,
                &fixture.material,
                &fixture.old,
                &fixture.old_version,
                &fixture.new_version,
                "../bad",
                &|| false
            )
            .is_err()
        );
        let private = fixture.old.join(format!(".generation-{GENERATION}"));
        fs::create_dir(&private).unwrap();
        assert!(
            copy_stem_generation(
                &fixture.root,
                &fixture.material,
                &private,
                &fixture.old_version,
                &fixture.new_version,
                GENERATION,
                &|| false
            )
            .is_err()
        );
        assert!(!fixture.stems_root().exists());
    }

    #[test]
    fn interrupted_canonical_and_legacy_source_lineages_reuse_the_verified_same_set() {
        let fixture = Fixture::new();
        let copied = fixture.copy().unwrap();
        let canonical =
            material_paths::resolve(&fixture.root, Path::new(&copied.cache_reference)).unwrap();
        let restored = copy_stem_generation(
            &fixture.root,
            &fixture.material,
            &canonical.path,
            &fixture.new_version,
            &fixture.new_version,
            "77777777777777777777777777777777",
            &|| false,
        )
        .unwrap();
        assert!(!restored.created);
        assert_eq!(restored.cache_reference, copied.cache_reference);
        assert_eq!(fs::read_dir(fixture.stems_root()).unwrap().count(), 1);
        assert!(fixture.old.join(".complete.json").is_file());
    }

    #[test]
    fn unknown_ready_collision_is_preserved_without_staging_or_overwrite() {
        for complete in [false, true] {
            let fixture = Fixture::new();
            if complete {
                fixture.copy().unwrap();
            }
            let ready = fixture.stems_root().join(format!(".ready-{GENERATION}"));
            fs::create_dir_all(&ready).unwrap();
            fs::write(ready.join("private.keep"), b"foreign owner").unwrap();
            assert!(fixture.copy().is_err());
            assert_eq!(
                fs::read(ready.join("private.keep")).unwrap(),
                b"foreign owner"
            );
            assert_eq!(
                fs::read_dir(&ready).unwrap().count(),
                if complete { 7 } else { 1 }
            );
            assert!(
                !fixture
                    .stems_root()
                    .join(format!(".generation-{GENERATION}"))
                    .exists()
            );
        }
    }

    #[test]
    fn different_complete_target_lineage_is_not_overwritten_or_adopted_as_retry() {
        let fixture = Fixture::new();
        let reference = fixture.copy().unwrap().cache_reference;
        let ready = material_paths::resolve(&fixture.root, Path::new(&reference))
            .unwrap()
            .path;
        let vocals = ready.join("vocals.wav");
        let mut bytes = fs::read(&vocals).unwrap();
        bytes[45] ^= 1;
        fs::write(&vocals, &bytes).unwrap();
        let mut marker: Value =
            serde_json::from_slice(&fs::read(ready.join(MARKER)).unwrap()).unwrap();
        marker["stems"]["vocals"] = json!(format!("{:x}", Sha256::digest(&bytes)));
        let preserved_marker = serde_json::to_vec(&marker).unwrap();
        fs::write(ready.join(MARKER), &preserved_marker).unwrap();
        assert!(fixture.copy().unwrap_err().contains("lineage collision"));
        assert_eq!(fs::read(&vocals).unwrap(), bytes);
        assert_eq!(fs::read(ready.join(MARKER)).unwrap(), preserved_marker);
        assert_eq!(fs::read_dir(fixture.stems_root()).unwrap().count(), 1);
    }

    #[test]
    fn cancellation_during_copy_removes_only_exact_own_files_and_preserves_unknown_children() {
        for unknown in [false, true] {
            let fixture = Fixture::new();
            let old_marker = fs::read(fixture.old.join(MARKER)).unwrap();
            let private = fixture
                .stems_root()
                .join(format!(".generation-{GENERATION}"));
            let inserted = Cell::new(false);
            let cancelled = || {
                if !private.join("melody.wav").exists() {
                    return false;
                }
                if unknown && !inserted.replace(true) {
                    fs::write(private.join("private.keep"), b"preserved unknown").unwrap();
                }
                true
            };
            let result = copy_stem_generation(
                &fixture.root,
                &fixture.material,
                &fixture.old,
                &fixture.old_version,
                &fixture.new_version,
                GENERATION,
                &cancelled,
            );
            assert!(result.unwrap_err().contains("cancelled"));
            assert!(
                !fixture
                    .stems_root()
                    .join(format!(".ready-{GENERATION}"))
                    .exists()
            );
            assert!(!private.join("vocals.wav").exists());
            assert!(!private.join("melody.wav").exists());
            assert!(!private.join(MARKER).exists());
            if unknown {
                assert_eq!(
                    fs::read(private.join("private.keep")).unwrap(),
                    b"preserved unknown"
                );
                assert_eq!(fs::read_dir(&private).unwrap().count(), 1);
            } else {
                assert!(!private.exists());
            }
            assert_eq!(fs::read(fixture.old.join(MARKER)).unwrap(), old_marker);
            for stem in STEM_FILE_NAMES {
                assert!(fixture.old.join(format!("{stem}.wav")).exists());
            }
        }
    }
}
