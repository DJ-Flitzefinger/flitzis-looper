//! Executed Original-clock evidence. JSON and path locators grant no authority.
use super::{
    cold_store::sealed_reader, material_paths, project_assets,
    resident_accounting::ResidentAccount, sample_loader::StreamDecodePlan,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) struct OriginalClockProof {
    root: PathBuf,
    path: PathBuf,
    root_identity: project_assets::FileIdentity,
    original_identity: project_assets::FileIdentity,
    material: String,
    sha256: String,
    bytes: u64,
    frames: usize,
    rate: u32,
    channels: usize,
    decoder_policy: String,
    _original: File,
    _guards: Vec<File>,
}

/// Persisted comparison data only. Deserialization grants no clock authority.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OriginalClockBinding {
    original_reference: String,
    material_id: String,
    original_identity: project_assets::FileIdentity,
    sha256: String,
    bytes: u64,
    frames: u64,
    rate_hz: u32,
    channels: u32,
    decoder_policy: String,
}

fn hash(file: &mut File, cancelled: &impl Fn() -> bool) -> Result<(String, u64), String> {
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        if cancelled() {
            return Err("Original clock proof cancelled".into());
        }
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or("Original byte overflow")?;
        hash.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    Ok((format!("{:x}", hash.finalize()), bytes))
}

impl OriginalClockProof {
    pub(super) fn acquire(
        root: &Path,
        original: &Path,
        account: &ResidentAccount,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<Self>, String> {
        let typed = material_paths::resolve(root, original).map_err(|e| e.to_string())?;
        let material = match typed.kind {
            material_paths::AssetKind::Original {
                material: Some(material),
            } => material,
            _ => return Err("Original clock requires a canonical material Original".into()),
        };
        let guards =
            project_assets::directory_guards(typed.path.parent().ok_or("Original parent missing")?)
                .map_err(|e| e.to_string())?;
        let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
        let root_identity = project_assets::capture_identity(&root)
            .map_err(|e| e.to_string())?
            .ok_or("samples identity missing")?;
        let mut original = sealed_reader(&typed.path).map_err(|e| e.to_string())?;
        let original_identity =
            project_assets::file_identity(&original).map_err(|e| e.to_string())?;
        let (sha256, bytes) = hash(&mut original, cancelled)?;
        let plan = StreamDecodePlan::probe(
            original.try_clone().map_err(|e| e.to_string())?,
            &typed.path,
        )
        .map_err(|e| e.to_string())?;
        if plan.working_peak_bytes() > super::cold_jobs::PCM_LIMIT_BYTES {
            return Err("Original decoder workspace exceeds admission".into());
        }
        let reserved = plan.reserve(account).map_err(|e| e.to_string())?;
        // This same promise survives decoder errors and the final anonymous
        // DELETE_ON_CLOSE PCM reader. No path-based unlink can outlive the job.
        let promise = reserved.retain_promise();
        let decoded = reserved.execute_temporary(cancelled);
        let dimensions = decoded
            .as_ref()
            .map(|d| {
                (
                    d.frames,
                    d.rate_hz,
                    d.channels,
                    d.artifact.descriptor["provenance"]["processing"]
                        .as_str()
                        .map(str::to_owned),
                )
            })
            .map_err(|e| e.to_string());
        drop(decoded);
        drop(promise);
        let (frames, rate, channels, policy) = dimensions?;
        let decoder_policy = policy.ok_or("executed decoder policy missing")?;
        if cancelled() {
            return Err("Original clock proof cancelled".into());
        }
        let result = Arc::new(Self {
            root,
            path: typed.path,
            root_identity,
            original_identity,
            material,
            sha256,
            bytes,
            frames,
            rate,
            channels,
            decoder_policy,
            _original: original,
            _guards: guards,
        });
        result.verify(
            &result.root,
            &result.path,
            &result.material,
            &result.sha256,
            result.bytes,
            cancelled,
        )?;
        Ok(result)
    }

    pub(super) fn clock(&self) -> super::stem_cache::rate_reader::SourceClock {
        super::stem_cache::rate_reader::SourceClock {
            frames: self.frames,
            rate: self.rate,
        }
    }
    pub(super) fn channels(&self) -> usize {
        self.channels
    }

    pub(super) fn binding(&self) -> Result<OriginalClockBinding, String> {
        let relative = self
            .path
            .strip_prefix(&self.root)
            .map_err(|e| e.to_string())?;
        Ok(OriginalClockBinding {
            original_reference: format!(
                "samples/{}",
                relative.to_string_lossy().replace('\\', "/")
            ),
            material_id: self.material.clone(),
            original_identity: self.original_identity.clone(),
            sha256: self.sha256.clone(),
            bytes: self.bytes,
            frames: self.frames as u64,
            rate_hz: self.rate,
            channels: self.channels as u32,
            decoder_policy: self.decoder_policy.clone(),
        })
    }

    pub(super) fn verify_keeper(
        &self,
        identity: &project_assets::FileIdentity,
    ) -> Result<(), String> {
        if &self.original_identity != identity {
            return Err("Original clock differs from held keeper FileID".into());
        }
        Ok(())
    }

    pub(super) fn verify(
        &self,
        root: &Path,
        path: &Path,
        material: &str,
        sha256: &str,
        bytes: u64,
        cancelled: &impl Fn() -> bool,
    ) -> Result<(), String> {
        let typed = material_paths::resolve(root, path).map_err(|e| e.to_string())?;
        if !matches!(typed.kind,material_paths::AssetKind::Original{material:Some(ref id)} if id==material)
            || std::fs::canonicalize(root).map_err(|e| e.to_string())? != self.root
            || typed.path != self.path
            || material != self.material
            || sha256 != self.sha256
            || bytes != self.bytes
            || project_assets::capture_identity(&self.root)
                .map_err(|e| e.to_string())?
                .as_ref()
                != Some(&self.root_identity)
        {
            return Err("Original clock material/root binding changed".into());
        }
        let mut file = sealed_reader(&typed.path).map_err(|e| e.to_string())?;
        if project_assets::file_identity(&file).map_err(|e| e.to_string())?
            != self.original_identity
            || hash(&mut file, cancelled)? != (self.sha256.clone(), self.bytes)
        {
            return Err("Original clock file identity or complete bytes changed".into());
        }
        Ok(())
    }
}
