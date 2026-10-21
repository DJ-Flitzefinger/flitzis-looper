//! One typed, contained project-asset namespace. Resolution is not ownership:
//! readers and deletion still capture guarded handles and exact file identities.

use super::constants::NUM_SAMPLES;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::io;
use std::path::{Component, Path, PathBuf};
mod readable;
pub(super) use readable::{MaterialBinding, read_binding, read_binding_held, readable_name};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AssetKind {
    Original {
        material: Option<String>,
    },
    StemDirectory {
        material: Option<String>,
        generation: bool,
    },
    StemArtifact,
    PcmDirectory,
    PcmArtifact,
    SourceBeatmapDirectory {
        material: Option<String>,
    },
    SourceBeatmapArtifact {
        material: Option<String>,
    },
    StemPcmDirectory {
        material: String,
        generation: bool,
    },
    StemPcmArtifact,
    StemPairDescriptor {
        material: String,
    },
    SlotMembership,
    MigrationJournal,
}

#[derive(Clone, Debug)]
pub(super) struct ResolvedAsset {
    pub path: PathBuf,
    pub kind: AssetKind,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub(super) fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn slot_number(name: &str) -> Option<usize> {
    let digits = name.strip_prefix('#')?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<usize>()
        .ok()
        .filter(|slot| (1..=NUM_SAMPLES).contains(slot))
}

pub(super) fn ready_generation(name: &str) -> bool {
    name.strip_prefix(".ready-").is_some_and(valid_id)
}

pub(super) fn stem_generation(name: &str) -> bool {
    [".ready-", ".generation-"]
        .iter()
        .any(|prefix| name.strip_prefix(prefix).is_some_and(valid_id))
}

pub(super) fn pcm_generation(name: &str) -> bool {
    if ready_generation(name) {
        return true;
    }
    let mut parts = name.split('-');
    let Some(digest) = parts.next() else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        && parts.clone().count() == 2
        && parts.all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
}

pub(super) fn filename(name: &str) -> bool {
    let device = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&device.as_str())
        || ["COM", "LPT"].iter().any(|prefix| {
            device.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
    !name.is_empty()
        && !reserved
        && ![".", ".."].contains(&name)
        && !name.ends_with(['.', ' '])
        && !name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
}

fn legacy_original(name: &str) -> bool {
    filename(name)
        && !name.starts_with('.')
        && !name.ends_with(".config.json")
        && name != "config.json"
}

fn stem_file(name: &str) -> bool {
    [
        "vocals.wav",
        "melody.wav",
        "bass.wav",
        "drums.wav",
        "instrumental.wav",
        ".complete.json",
    ]
    .contains(&name)
}

/// Classify a project-relative reference without resolving away lexical traversal.
pub(super) fn classify(path: &Path) -> io::Result<AssetKind> {
    if path
        .to_string_lossy()
        .split(['/', '\\'])
        .any(|part| [".", ".."].contains(&part))
    {
        return Err(invalid("asset path contains traversal"));
    }
    let parts = path
        .components()
        .map(|part| match part {
            Component::Normal(name) => name
                .to_str()
                .ok_or_else(|| invalid("asset name is not UTF-8")),
            _ => Err(invalid(
                "asset path contains traversal or an absolute component",
            )),
        })
        .collect::<io::Result<Vec<_>>>()?;
    if parts.first() != Some(&"samples") {
        return Err(invalid("asset reference must start with samples"));
    }
    let parts = &parts[1..];
    // Map sidecars are a distinct typed namespace, including explicit legacy roots.
    // Only published ready generations are externally addressable.
    let map_parts = match parts {
        [".pcm-cache", "source-beatmaps", "v1", rest @ ..] => Some((None, rest)),
        [owner, ".pcm-cache", "source-beatmaps", "v1", rest @ ..]
            if readable::readable_container(owner) =>
        {
            Some((Some(String::new()), rest))
        }
        [
            "materials",
            owner,
            ".pcm-cache",
            "source-beatmaps",
            "v1",
            rest @ ..,
        ] if owner.strip_prefix('M').is_some_and(valid_id) => {
            Some((Some(owner[1..].to_owned()), rest))
        }
        _ => None,
    };
    if let Some((material, tail)) = map_parts {
        return match tail {
            [generation] if ready_generation(generation) => {
                Ok(AssetKind::SourceBeatmapDirectory { material })
            }
            [generation, "map.json"] if ready_generation(generation) => {
                Ok(AssetKind::SourceBeatmapArtifact { material })
            }
            _ => Err(invalid(
                "source beatmap reference requires an exact ready artifact",
            )),
        };
    }
    match parts {
        [owner, name]
            if readable::readable_container(owner)
                && filename(name)
                && !["stems", ".pcm-cache", ".material.json"].contains(name) =>
        {
            Ok(AssetKind::Original {
                material: Some(String::new()),
            })
        }
        [owner, "stems"] if readable::readable_container(owner) => Ok(AssetKind::StemDirectory {
            material: Some(String::new()),
            generation: false,
        }),
        [owner, "stems", generation]
            if readable::readable_container(owner) && stem_generation(generation) =>
        {
            Ok(AssetKind::StemDirectory {
                material: Some(String::new()),
                generation: true,
            })
        }
        [owner, "stems", generation, name]
            if readable::readable_container(owner)
                && stem_generation(generation)
                && stem_file(name) =>
        {
            Ok(AssetKind::StemArtifact)
        }
        [owner, "stems", name] if readable::readable_container(owner) && stem_file(name) => {
            Ok(AssetKind::StemArtifact)
        }
        [owner, ".pcm-cache", "v1", generation]
            if readable::readable_container(owner) && pcm_generation(generation) =>
        {
            Ok(AssetKind::PcmDirectory)
        }
        [owner, ".pcm-cache", "v1", generation, name]
            if readable::readable_container(owner)
                && pcm_generation(generation)
                && pcm_file(name) =>
        {
            Ok(AssetKind::PcmArtifact)
        }
        [owner, ".pcm-cache", "stems", "v1", generation]
            if readable::readable_container(owner) && stem_generation(generation) =>
        {
            Ok(AssetKind::StemPcmDirectory {
                material: String::new(),
                generation: true,
            })
        }
        [owner, ".pcm-cache", "stems", "v1", generation, name]
            if readable::readable_container(owner)
                && stem_generation(generation)
                && stem_pcm_file(name) =>
        {
            Ok(AssetKind::StemPcmArtifact)
        }
        [owner, ".pcm-cache", "stems", "v1", ".pairs", name]
            if readable::readable_container(owner)
                && name.strip_suffix(".json").is_some_and(valid_id) =>
        {
            Ok(AssetKind::StemPairDescriptor {
                material: String::new(),
            })
        }
        [name]
            if legacy_original(name)
                && !["materials", "stems"].contains(name)
                && !name.starts_with('#') =>
        {
            Ok(AssetKind::Original { material: None })
        }
        [".material-migrations", transaction] if valid_id(transaction) => {
            Ok(AssetKind::MigrationJournal)
        }
        [".material-migrations", transaction, name]
            if valid_id(transaction)
                && name.len() == 7
                && name.ends_with(".json")
                && name.as_bytes()[..2]
                    .iter()
                    .all(|byte| byte.is_ascii_digit()) =>
        {
            Ok(AssetKind::MigrationJournal)
        }
        [slot, "membership.json"] if slot_number(slot).is_some() => Ok(AssetKind::SlotMembership),
        ["materials", owner, "original", name]
            if owner.strip_prefix('M').is_some_and(valid_id) && filename(name) =>
        {
            Ok(AssetKind::Original {
                material: Some(owner[1..].to_owned()),
            })
        }
        ["materials", owner, "stems"] if owner.strip_prefix('M').is_some_and(valid_id) => {
            Ok(AssetKind::StemDirectory {
                material: Some(owner[1..].to_owned()),
                generation: false,
            })
        }
        ["materials", owner, "stems", generation]
            if owner.strip_prefix('M').is_some_and(valid_id) && stem_generation(generation) =>
        {
            Ok(AssetKind::StemDirectory {
                material: Some(owner[1..].to_owned()),
                generation: true,
            })
        }
        ["materials", owner, "stems", generation, name]
            if owner.strip_prefix('M').is_some_and(valid_id)
                && stem_generation(generation)
                && stem_file(name) =>
        {
            Ok(AssetKind::StemArtifact)
        }
        ["materials", owner, "stems", name]
            if owner.strip_prefix('M').is_some_and(valid_id) && stem_file(name) =>
        {
            Ok(AssetKind::StemArtifact)
        }
        ["stems", slot] if legacy_stem_container(slot) => Ok(AssetKind::StemDirectory {
            material: None,
            generation: false,
        }),
        ["stems", slot, generation]
            if slot_number(slot).is_some() && stem_generation(generation) =>
        {
            Ok(AssetKind::StemDirectory {
                material: None,
                generation: true,
            })
        }
        ["stems", slot, name] if legacy_stem_container(slot) && stem_file(name) => {
            Ok(AssetKind::StemArtifact)
        }
        ["stems", slot, generation, name]
            if slot_number(slot).is_some() && stem_generation(generation) && stem_file(name) =>
        {
            Ok(AssetKind::StemArtifact)
        }
        [".pcm-cache", "v1", generation] if pcm_generation(generation) => {
            Ok(AssetKind::PcmDirectory)
        }
        [".pcm-cache", "v1", generation, name] if pcm_generation(generation) && pcm_file(name) => {
            Ok(AssetKind::PcmArtifact)
        }
        ["materials", owner, ".pcm-cache", "v1", generation]
            if owner.strip_prefix('M').is_some_and(valid_id) && pcm_generation(generation) =>
        {
            Ok(AssetKind::PcmDirectory)
        }
        ["materials", owner, ".pcm-cache", "v1", generation, name]
            if owner.strip_prefix('M').is_some_and(valid_id)
                && pcm_generation(generation)
                && pcm_file(name) =>
        {
            Ok(AssetKind::PcmArtifact)
        }
        ["materials", owner, ".pcm-cache", "stems", "v1", generation]
            if owner.strip_prefix('M').is_some_and(valid_id) && stem_generation(generation) =>
        {
            Ok(AssetKind::StemPcmDirectory {
                material: owner[1..].to_owned(),
                generation: true,
            })
        }
        [
            "materials",
            owner,
            ".pcm-cache",
            "stems",
            "v1",
            generation,
            name,
        ] if owner.strip_prefix('M').is_some_and(valid_id)
            && stem_generation(generation)
            && stem_pcm_file(name) =>
        {
            Ok(AssetKind::StemPcmArtifact)
        }
        [
            "materials",
            owner,
            ".pcm-cache",
            "stems",
            "v1",
            ".pairs",
            name,
        ] if owner.strip_prefix('M').is_some_and(valid_id)
            && name.strip_suffix(".json").is_some_and(valid_id) =>
        {
            Ok(AssetKind::StemPairDescriptor {
                material: owner[1..].to_owned(),
            })
        }
        _ => Err(invalid(
            "asset path does not name a declared project artifact",
        )),
    }
}

fn legacy_stem_container(name: &str) -> bool {
    if name.starts_with('#') {
        slot_number(name).is_some()
    } else {
        filename(name)
    }
}

fn pcm_file(name: &str) -> bool {
    ["decoder.f32le", "playback.f32le", "manifest.json"].contains(&name)
}

fn stem_pcm_file(name: &str) -> bool {
    name == "manifest.json"
        || name
            .strip_suffix(".f32le")
            .is_some_and(|stem| super::stem_cache::STEM_FILE_NAMES.contains(&stem))
}

/// Resolve an exact typed reference. The shared ownership resolver rejects
/// links/reparse ancestors and changed roots before returning a contained path.
pub(super) fn resolve(root: &Path, path: &Path) -> io::Result<ResolvedAsset> {
    // Validate syntax before canonicalize/abspath can erase a ParentDir.
    if path
        .to_string_lossy()
        .split(['/', '\\'])
        .any(|part| [".", ".."].contains(&part))
    {
        return Err(invalid("asset path contains traversal"));
    }
    let (root, path) = if root.exists() {
        super::project_assets::owned_path(root, path)?
    } else {
        if !root.is_absolute() || root.file_name().is_none_or(|name| name != "samples") {
            return Err(invalid("asset root must be an absolute samples directory"));
        }
        let parent = root
            .parent()
            .ok_or_else(|| invalid("samples parent missing"))?;
        super::project_assets::reject_links(parent)?;
        let parent = std::fs::canonicalize(parent)?;
        let root = parent.join("samples");
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            parent.join(path)
        };
        let path = super::project_assets::canonical_with_missing(&path)?;
        (root, path)
    };
    let relative = Path::new("samples").join(
        path.strip_prefix(&root)
            .map_err(|_| invalid("asset escaped samples"))?,
    );
    let mut kind = classify(&relative)?;
    if let Some(material) = material_root(&path)
        && material.parent() == Some(root.as_path())
    {
        let binding = read_binding(material)?;
        let id = binding.material_id();
        match &mut kind {
            AssetKind::Original { material: value } => {
                if path != material.join(&binding.original_leaf) {
                    return Err(invalid("original does not match readable canonical leaf"));
                }
                *value = Some(id);
            }
            AssetKind::StemDirectory {
                material: value, ..
            }
            | AssetKind::SourceBeatmapDirectory { material: value }
            | AssetKind::SourceBeatmapArtifact { material: value } => *value = Some(id),
            AssetKind::StemPcmDirectory {
                material: value, ..
            }
            | AssetKind::StemPairDescriptor { material: value } => *value = id,
            _ => {}
        }
    }
    Ok(ResolvedAsset { path, kind })
}

#[pyfunction]
pub fn resolve_project_asset(
    samples_root: String,
    path: String,
) -> PyResult<(String, String, Option<String>)> {
    let root = Path::new(&samples_root);
    let resolved = resolve(root, Path::new(&path))
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    let (kind, material) = match resolved.kind {
        AssetKind::Original { material } => ("original", material),
        AssetKind::StemDirectory { material, .. } => ("stem_directory", material),
        AssetKind::StemArtifact => ("stem_artifact", None),
        AssetKind::PcmDirectory => ("pcm_directory", None),
        AssetKind::PcmArtifact => ("pcm_artifact", None),
        AssetKind::SourceBeatmapDirectory { material } => ("source_beatmap_directory", material),
        AssetKind::SourceBeatmapArtifact { material } => ("source_beatmap_artifact", material),
        AssetKind::StemPcmDirectory { material, .. } => ("stem_pcm_directory", Some(material)),
        AssetKind::StemPcmArtifact => ("stem_pcm_artifact", None),
        AssetKind::StemPairDescriptor { material } => ("stem_pair_descriptor", Some(material)),
        AssetKind::SlotMembership => ("slot_membership", None),
        AssetKind::MigrationJournal => ("migration_journal", None),
    };
    Ok((
        kind.to_owned(),
        ordinary_windows_path(&resolved.path),
        material,
    ))
}

pub(super) fn ordinary_windows_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    // Python pathlib uses ordinary Win32 spelling for cwd and durable references.
    // The native ownership registry continues to use its canonical handle path.
    if let Some(unc) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        path.strip_prefix("\\\\?\\").unwrap_or(&path).to_owned()
    }
}

/// Best-effort empty-only pruning, after exact artifact guards have closed.
/// Every parent is guarded, each deleted directory uses its opened identity,
/// and any unknown child or concurrent writer keeps the container intact.
pub(super) fn material_root(path: &Path) -> Option<&Path> {
    path.ancestors().take(8).find(|candidate| {
        (candidate
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.strip_prefix('M').is_some_and(valid_id))
            && candidate
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "materials"))
            || ((candidate != &path || candidate.is_dir())
                && candidate
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "samples")
                && candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(readable::readable_container))
    })
}

pub(super) fn material_root_for_original(samples: &Path, original: &Path) -> io::Result<PathBuf> {
    let checked = resolve(samples, original)?;
    if !matches!(checked.kind, AssetKind::Original { .. }) {
        return Err(invalid("reference is not an original"));
    }
    Ok(material_root(&checked.path).unwrap_or(samples).to_owned())
}

pub(super) fn readable_original(samples: &Path, original: &Path) -> io::Result<bool> {
    let root = material_root_for_original(samples, original)?;
    Ok(root.parent().is_some_and(|parent| {
        parent == samples || std::fs::canonicalize(samples).is_ok_and(|samples| parent == samples)
    }))
}

/// Syntax only, for immutable descriptor checks; resolve() supplies actual binding authority.
pub(super) fn material_reference_root(reference: &Path) -> Option<PathBuf> {
    let parts: Vec<_> = reference
        .components()
        .map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    match parts.as_slice() {
        ["samples", "materials", owner, ..] if owner.strip_prefix('M').is_some_and(valid_id) => {
            Some(Path::new("samples/materials").join(owner))
        }
        ["samples", owner, _, ..] if readable::readable_container(owner) => {
            Some(Path::new("samples").join(owner))
        }
        _ => None,
    }
}

pub(super) fn canonical_original_root_reference(
    reference: &Path,
    expected_material_id: &str,
) -> io::Result<PathBuf> {
    let raw = reference
        .to_str()
        .ok_or_else(|| invalid("canonical original is not UTF-8"))?;
    let normalized: PathBuf = reference.components().collect();
    if raw.contains('\\') || normalized.to_string_lossy().replace('\\', "/") != raw {
        return Err(invalid("canonical original spelling mismatch"));
    }
    if !valid_id(expected_material_id)
        || !matches!(
            classify(reference)?,
            AssetKind::Original { material: Some(_) }
        )
    {
        return Err(invalid("canonical original syntax mismatch"));
    }
    let root =
        material_reference_root(reference).ok_or_else(|| invalid("canonical root missing"))?;
    if root.parent() == Some(Path::new("samples/materials"))
        && root.file_name().and_then(|name| name.to_str())
            != Some(format!("M{expected_material_id}").as_str())
    {
        return Err(invalid("material ID root mismatch"));
    }
    Ok(root)
}

#[pyfunction]
pub fn resolve_project_asset_details(
    samples_root: String,
    path: String,
) -> PyResult<(String, String, Option<String>, String, String)> {
    let root = Path::new(&samples_root);
    let resolved = resolve(root, Path::new(&path))
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    let material = material_root(&resolved.path);
    let layout = match material {
        None => "legacy",
        Some(root)
            if root
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "materials") =>
        {
            "material_id"
        }
        Some(_) => "readable",
    };
    let material_path = material.map(ordinary_windows_path).unwrap_or_default();
    let (kind, path, id) = resolve_project_asset(samples_root, path)?;
    Ok((kind, path, id, layout.to_owned(), material_path))
}

pub(super) fn prune_empty_material(path: &Path) {
    let Some(material) = material_root(path) else {
        return;
    };
    // ProjectAssets holds its existing current owner/reader/retirement gate.
    // A native creation receipt supplies exact root + complete marker authority;
    // restored/unreceipted markers continue through empty-only scaffolding.
    if super::cold_store::prune_created_material(material) {
        return;
    }
    let Some(samples) = material
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "samples"))
    else {
        return;
    };
    if samples.file_name().is_none_or(|name| name != "samples") || resolve(samples, path).is_err() {
        return;
    }
    for candidate in [
        material.join("original"),
        material.join("stems"),
        material.join(".pcm-cache/stems/v1/.pairs"),
        material.join(".pcm-cache/stems/v1"),
        material.join(".pcm-cache/stems"),
        material.join(".pcm-cache/v1"),
        material.join(".pcm-cache"),
    ] {
        let result = (|| -> io::Result<()> {
            let parent = candidate
                .parent()
                .ok_or_else(|| invalid("container parent missing"))?;
            let _guards = super::project_assets::directory_guards(parent)?;
            super::project_assets::reject_links(&candidate)?;
            let Some(identity) = super::project_assets::capture_identity(&candidate)? else {
                return Ok(());
            };
            super::project_assets::remove_empty_directory(&candidate, &identity)
        })();
        let _ = result; // Empty-container pruning never widens or replaces artifact retirement.
    }
    let result = (|| -> io::Result<()> {
        let _guards = super::project_assets::directory_guards(material)?;
        // The artifact queue does not carry an exact binding receipt. A fresh
        // schema read grants no deletion authority, even in an otherwise empty
        // root: the marker may have changed after the original retired. Preserve
        // it and its root until a separately receipted binding retirement exists.
        drop(_guards);
        let _parent = super::project_assets::directory_guards(
            material
                .parent()
                .ok_or_else(|| invalid("material parent missing"))?,
        )?;
        if let Some(identity) = super::project_assets::capture_identity(material)? {
            super::project_assets::remove_empty_directory(material, &identity)?;
        }
        Ok(())
    })();
    let _ = result;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_namespace_rejects_wrong_artifacts_traversal_and_ids() {
        let id = "0123456789abcdef0123456789abcdef";
        for slot in ["#1", "#216"] {
            assert!(slot_number(slot).is_some());
        }
        for slot in ["#0", "#217", "#01", "#-1"] {
            assert!(slot_number(slot).is_none());
        }
        for leaf in [
            "Take.wav:stream",
            "Take.wav.",
            "Take.wav ",
            "NUL.wav",
            "COM1.wav",
            "LPT¹.wav",
        ] {
            assert!(!filename(leaf));
        }
        assert!(filename("Mélodie 東京.flac"));
        for path in [
            "samples/../outside.wav".to_owned(),
            "samples/#0/membership.json".to_owned(),
            "samples/#217/membership.json".to_owned(),
            "samples/materials/Mbad/original/x.wav".to_owned(),
            format!("samples/materials/M{id}/manifest.json"),
            format!("samples/materials/M{id}/original/../manifest.json"),
            format!("samples/materials/M{id}/stems/.ready-bad/vocals.wav"),
        ] {
            assert!(classify(Path::new(&path)).is_err(), "{path}");
        }
        assert!(matches!(
            classify(Path::new(&format!(
                "samples/materials/M{id}/original/Take.flac"
            )))
            .unwrap(),
            AssetKind::Original { material: Some(_) }
        ));
        assert!(matches!(
            classify(Path::new(&format!(
                "samples/materials/M{id}/.pcm-cache/v1/.ready-{id}/manifest.json"
            )))
            .unwrap(),
            AssetKind::PcmArtifact
        ));
        assert!(matches!(
            classify(Path::new(&format!(
                "samples/materials/M{id}/stems/.ready-{id}"
            )))
            .unwrap(),
            AssetKind::StemDirectory {
                generation: true,
                ..
            }
        ));
    }

    #[test]
    fn readable_directory_names_keep_safe_prefix_and_disambiguation_space() {
        assert_eq!(
            ordinary_windows_path(Path::new(r"\\?\UNC\server\share\samples\Take.wav")),
            r"\\server\share\samples\Take.wav"
        );
        assert_eq!(readable_name("Mélodie 東京.flac"), "Mélodie 東京.flac");
        for leaf in [
            "NUL.wav",
            "COM¹.wav",
            "stems",
            "Materials",
            ".Take.wav",
            "#12.wav",
            "bad?name.wav",
            "trailing.wav. ",
        ] {
            let name = readable_name(leaf);
            assert!(readable::readable_container(&name), "{leaf}: {name}");
        }
        let long = format!("{}.wav", "東京".repeat(100));
        let name = readable_name(&long);
        assert!(name.encode_utf16().count() <= 96);
        assert!(readable::readable_container(&name));
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            canonical_original_root_reference(
                Path::new("samples/Mélodie 東京.flac/Mélodie 東京.flac"),
                id
            )
            .unwrap(),
            Path::new("samples/Mélodie 東京.flac")
        );
        assert!(
            canonical_original_root_reference(
                Path::new("samples/Mélodie 東京.flac//Mélodie 東京.flac"),
                id
            )
            .is_err()
        );
    }

    #[test]
    fn legacy_details_do_not_turn_samples_into_a_canonical_material_root() {
        let temp = tempfile::tempdir().unwrap();
        let samples = temp.path().join("samples");
        std::fs::create_dir(&samples).unwrap();
        std::fs::write(samples.join("old.wav"), b"owned legacy fixture").unwrap();
        let details = resolve_project_asset_details(
            samples.to_string_lossy().into_owned(),
            "samples/old.wav".to_owned(),
        )
        .unwrap();
        assert_eq!(details.0, "original");
        assert_eq!(details.2, None);
        assert_eq!(details.3, "legacy");
        assert_eq!(details.4, "");
        let stems = resolve_project_asset_details(
            samples.to_string_lossy().into_owned(),
            "samples/stems/#7".to_owned(),
        )
        .unwrap();
        assert_eq!(stems.0, "stem_directory");
        assert_eq!(stems.3, "legacy");
        assert_eq!(stems.4, "");
        let id = "0123456789abcdef0123456789abcdef";
        let material = samples.join("materials").join(format!("M{id}"));
        std::fs::create_dir_all(material.join("original")).unwrap();
        std::fs::write(
            material.join("original/old.wav"),
            b"owned old material fixture",
        )
        .unwrap();
        let details = resolve_project_asset_details(
            samples.to_string_lossy().into_owned(),
            format!("samples/materials/M{id}/original/old.wav"),
        )
        .unwrap();
        assert_eq!(details.2.as_deref(), Some(id));
        assert_eq!(details.3, "material_id");
        assert_eq!(Path::new(&details.4), material.as_path());
    }
}
