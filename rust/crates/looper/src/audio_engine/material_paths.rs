//! One typed, contained project-asset namespace. Resolution is not ownership:
//! readers and deletion still capture guarded handles and exact file identities.

use super::constants::NUM_SAMPLES;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::io;
use std::path::{Component, Path, PathBuf};

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
    SlotMembership,
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
    match parts {
        [name]
            if legacy_original(name)
                && !["materials", "stems"].contains(name)
                && !name.starts_with('#') =>
        {
            Ok(AssetKind::Original { material: None })
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
    let kind = classify(&relative)?;
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
        AssetKind::SlotMembership => ("slot_membership", None),
    };
    let path = resolved.path.to_string_lossy();
    // Python pathlib uses ordinary Win32 spelling for cwd and durable references.
    // The native ownership registry continues to use its canonical handle path.
    let path = if let Some(unc) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        path.strip_prefix("\\\\?\\").unwrap_or(&path).to_owned()
    };
    Ok((kind.to_owned(), path, material))
}

/// Best-effort empty-only pruning, after exact artifact guards have closed.
/// Every parent is guarded, each deleted directory uses its opened identity,
/// and any unknown child or concurrent writer keeps the container intact.
pub(super) fn material_root(path: &Path) -> Option<&Path> {
    path.ancestors().take(7).find(|candidate| {
        candidate
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.strip_prefix('M').is_some_and(valid_id))
            && candidate
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "materials")
    })
}

pub(super) fn prune_empty_material(path: &Path) {
    let Some(material) = material_root(path) else {
        return;
    };
    let Some(samples) = material.parent().and_then(Path::parent) else {
        return;
    };
    if samples.file_name().is_none_or(|name| name != "samples") || resolve(samples, path).is_err() {
        return;
    }
    for candidate in [
        material.join("original"),
        material.join("stems"),
        material.join(".pcm-cache/v1"),
        material.join(".pcm-cache"),
        material.to_owned(),
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
}
