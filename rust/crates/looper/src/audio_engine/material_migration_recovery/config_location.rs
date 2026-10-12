//! A verified Portable projection changes config locations, never migration authority.

use super::super::{material_paths, project_assets};
use project_assets::FileIdentity;
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::File,
    io,
    path::{Component, Path, PathBuf},
};

const PATH_LIMIT: usize = 8192;
pub(super) const BINDING_LIMIT: usize = 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocationProjection {
    pub(super) schema_version: u32,
    pub(super) samples_identity: FileIdentity,
    pub(super) bindings: Vec<LocationBinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocationBinding {
    pub(super) config_reference: String,
    pub(super) config_path: String,
    /// Capture/move invariant only; subsequent atomic saves may replace this file.
    pub(super) config_identity: FileIdentity,
    pub(super) parent_identity: FileIdentity,
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

pub(super) fn plain_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(unc) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        path.strip_prefix("\\\\?\\").unwrap_or(&path).to_owned()
    }
}

pub(super) fn reference_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        plain_path(path).replace('/', "\\").to_lowercase()
    }
    #[cfg(not(windows))]
    {
        plain_path(path)
    }
}

fn ordinary_absolute(reference: &Path) -> io::Result<()> {
    if !reference.is_absolute()
        || reference.to_string_lossy().len() > PATH_LIMIT
        || reference.components().any(|part| {
            matches!(part, Component::ParentDir | Component::CurDir)
                || matches!(part, Component::Normal(name) if name.to_str().is_none_or(|name| !material_paths::filename(name)))
        })
        || reference
            .to_string_lossy()
            .split(['/', '\\'])
            .any(|part| [".", ".."].contains(&part))
    {
        return Err(invalid(
            "config reference must be an ordinary absolute project path",
        ));
    }
    Ok(())
}

fn relative_config(reference: &str) -> io::Result<&Path> {
    let path = Path::new(reference);
    if reference.is_empty()
        || reference.len() > PATH_LIMIT
        || !path.components().all(|part| {
            matches!(part, Component::Normal(name) if name.to_str().is_some_and(material_paths::filename))
        })
        || reference
            .split(['/', '\\'])
            .any(|part| [".", ".."].contains(&part))
    {
        return Err(invalid(
            "relocated config must have an exact ordinary Samples-relative address",
        ));
    }
    Ok(path)
}

fn projected_path(root: &Path, binding: &LocationBinding) -> io::Result<PathBuf> {
    let path = root.join(relative_config(&binding.config_path)?);
    project_assets::reject_links(&path)?;
    let path = project_assets::canonical_with_missing(&path)?;
    if path == root || !path.starts_with(root) || !path.is_file() {
        return Err(invalid(
            "relocated config is missing or leaves its actual Samples directory",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("relocated config parent missing"))?;
    if project_assets::capture_identity(parent)?.as_ref() != Some(&binding.parent_identity) {
        return Err(invalid("relocated config parent-directory FileID changed"));
    }
    Ok(path)
}

fn validate_projection(root: &Path, projection: &LocationProjection) -> io::Result<Vec<File>> {
    if projection.schema_version != 1
        || projection.bindings.len() > BINDING_LIMIT
        || project_assets::capture_identity(root)?.as_ref() != Some(&projection.samples_identity)
    {
        return Err(invalid(
            "Portable config projection Samples identity or schema changed",
        ));
    }
    let mut references = HashSet::new();
    let mut parents = HashSet::new();
    let mut guards = Vec::new();
    for binding in &projection.bindings {
        let historical = Path::new(&binding.config_reference);
        ordinary_absolute(historical)?;
        if !references.insert(reference_key(historical)) {
            return Err(invalid(
                "Portable config projection has duplicate historical bindings",
            ));
        }
        let path = projected_path(root, binding)?;
        let parent = path
            .parent()
            .ok_or_else(|| invalid("relocated config parent missing"))?;
        if parents.insert(reference_key(parent)) {
            guards.extend(project_assets::directory_guards(parent)?);
        }
        // Recheck after obtaining parent custody rather than trusting a pathname ABA.
        projected_path(root, binding)?;
    }
    Ok(guards)
}

/// Keep the verified Samples/config-parent directory custody through a bounded operation.
pub(super) struct ConfigLocationResolver {
    root: PathBuf,
    projection: Option<LocationProjection>,
    _guards: Vec<File>,
}

impl ConfigLocationResolver {
    pub(super) fn new(root: &Path) -> io::Result<Self> {
        let (root, mut guards) = super::samples_root(root)?;
        // Only this Native custody verifier can admit the durable projection.
        // Deserializing its JSON, SHA or asserted FileIDs is not authorization.
        let projection = super::portable_relocation::verified_projection(&root)?;
        if let Some(projection) = &projection {
            guards.extend(validate_projection(&root, projection)?);
        }
        Ok(Self {
            root,
            projection,
            _guards: guards,
        })
    }

    pub(super) fn resolve(&self, reference: &Path) -> io::Result<PathBuf> {
        ordinary_absolute(reference)?;
        if let Some(binding) = self.projection.as_ref().and_then(|projection| {
            projection.bindings.iter().find(|binding| {
                reference_key(Path::new(&binding.config_reference)) == reference_key(reference)
            })
        }) {
            return projected_path(&self.root, binding);
        }
        project_assets::reject_links(reference)?;
        let project = self
            .root
            .parent()
            .ok_or_else(|| invalid("samples project parent missing"))?;
        let path = project_assets::canonical_with_missing(reference)?;
        if path == project || !path.starts_with(project) || path.exists() && !path.is_file() {
            return Err(invalid(
                "config reference escaped project root or is not an ordinary file",
            ));
        }
        Ok(path)
    }

    pub(super) fn matches(&self, current: &Path, historical: &str) -> io::Result<bool> {
        let current = self.resolve(current)?;
        let historical = self.resolve(Path::new(historical))?;
        Ok(reference_key(&current) == reference_key(&historical))
    }
}

pub(super) fn resolve(root: &Path, reference: &Path) -> io::Result<PathBuf> {
    ConfigLocationResolver::new(root)?.resolve(reference)
}

pub(super) fn matches(root: &Path, current: &Path, historical: &str) -> io::Result<bool> {
    ConfigLocationResolver::new(root)?.matches(current, historical)
}

#[pyfunction]
pub fn resolve_migration_config_reference(
    py: Python<'_>,
    samples_root: String,
    config_reference: String,
) -> PyResult<String> {
    py.detach(|| {
        resolve(Path::new(&samples_root), Path::new(&config_reference))
            .map(|path| plain_path(&path))
    })
    .map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;

    fn projection(root: &Path, historical: &Path, relative: &str) -> LocationProjection {
        let path = root.join(relative);
        LocationProjection {
            schema_version: 1,
            samples_identity: project_assets::capture_identity(root).unwrap().unwrap(),
            bindings: vec![LocationBinding {
                config_reference: plain_path(historical),
                config_path: relative.to_owned(),
                config_identity: project_assets::capture_identity(&path).unwrap().unwrap(),
                parent_identity: project_assets::capture_identity(path.parent().unwrap())
                    .unwrap()
                    .unwrap(),
            }],
        }
    }

    fn fixture_resolver(root: &Path, projection: LocationProjection) -> ConfigLocationResolver {
        // These geometry tests bypass only the separate ADS verifier, which the
        // production constructor always invokes. They are not custody receipts.
        let root = fs::canonicalize(root).unwrap();
        let guards = validate_projection(&root, &projection).unwrap();
        ConfigLocationResolver {
            root,
            projection: Some(projection),
            _guards: guards,
        }
    }

    #[test]
    fn relocated_config_location_survives_actual_newer_atomic_file_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let current = root.join("selected.config.json");
        let other = root.join("equal.config.json");
        fs::write(&current, b"old current").unwrap();
        fs::write(&other, b"old current").unwrap();
        let historical = temp
            .path()
            .join("old-checkout/samples/selected.config.json");
        let projection = projection(&root, &historical, "selected.config.json");
        let captured = projection.bindings[0].config_identity.clone();
        let resolver = fixture_resolver(&root, projection);
        let replacement = root.join("new-current.tmp");
        fs::write(&replacement, b"newer current remains authoritative").unwrap();
        fs::rename(&replacement, &current).unwrap();
        assert_ne!(
            project_assets::capture_identity(&current).unwrap().unwrap(),
            captured
        );
        assert!(
            resolver
                .matches(&current, &plain_path(&historical))
                .unwrap()
        );
        assert!(!resolver.matches(&other, &plain_path(&historical)).unwrap());
        assert_eq!(
            fs::read(&current).unwrap(),
            b"newer current remains authoritative"
        );
    }

    #[test]
    fn copied_samples_or_changed_parent_identity_never_admit_projection() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        let copied = temp.path().join("copied/samples");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&copied).unwrap();
        fs::write(root.join("selected.config.json"), b"equal bytes").unwrap();
        fs::write(copied.join("selected.config.json"), b"equal bytes").unwrap();
        let root = fs::canonicalize(root).unwrap();
        let copied = fs::canonicalize(copied).unwrap();
        let historical = temp.path().join("old/samples/selected.config.json");
        let mut projection = projection(&root, &historical, "selected.config.json");
        assert!(validate_projection(&copied, &projection).is_err());
        projection.bindings[0].parent_identity =
            project_assets::capture_identity(&copied).unwrap().unwrap();
        assert!(validate_projection(&root, &projection).is_err());
    }

    #[test]
    fn traversal_and_unknown_external_reference_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("selected.config.json"), b"current").unwrap();
        let root = fs::canonicalize(root).unwrap();
        let historical = temp.path().join("old/samples/selected.config.json");
        let projection = projection(&root, &historical, "selected.config.json");
        let resolver = fixture_resolver(&root, projection.clone());
        assert!(
            resolver
                .resolve(&temp.path().join("../foreign.config.json"))
                .is_err()
        );
        assert!(
            resolver
                .resolve(&temp.path().parent().unwrap().join("foreign.config.json"))
                .is_err()
        );
        let mut invalid = projection;
        invalid.bindings[0].config_path = "../selected.config.json".into();
        assert!(validate_projection(&root, &invalid).is_err());
    }
}
