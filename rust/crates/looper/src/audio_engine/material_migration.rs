//! Complete material preparation for migration, without source publication or analysis.

use super::cold_jobs::PCM_LIMIT_BYTES;
use super::cold_residency::{self, ResidentLoadHint};
use super::cold_store::{ColdTransaction, CommittedColdLease, PcmArtifactInput};
use super::material_paths::{self, AssetKind};
use super::sample_loader::{SampleLoadProgress, decode_audio_snapshot, prepare_playback};
use crate::messages::SampleBuffer;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) struct PreparedMigrationMaterial {
    pub sample: SampleBuffer,
    pub lease: CommittedColdLease,
    pub old_original: PathBuf,
    old_reference: String,
    new_reference: String,
    cache_reference: String,
}

impl PreparedMigrationMaterial {
    /// Durable evidence only. Runtime assignment IDs, publication and ACK stay outside JSON.
    pub(super) fn metadata(&self) -> Value {
        json!({
            "original": self.lease.manifest.descriptor["decoder"]["original"],
            "old_reference": self.old_reference,
            "new_reference": self.new_reference,
            "material_id": self.lease.material_id,
            "decoder_identity": self.lease.manifest.decoder_identity,
            "playback_identity": self.lease.manifest.identity,
            "descriptor": self.lease.manifest.descriptor,
            "cache_path": self.cache_reference,
        })
    }
}

/// The ordinary loader and migration execute the same decoder and transform policy.
pub(super) fn prepare_pcm(
    transaction: &mut ColdTransaction,
    source: &Path,
    output_rate: u32,
    output_channels: usize,
    maximum: usize,
    resident_hint: Option<ResidentLoadHint>,
    cancelled: &impl Fn() -> bool,
    progress: &mut impl FnMut(SampleLoadProgress),
) -> Result<SampleBuffer, String> {
    if let Some(sample) = transaction
        .try_reuse_selected(
            output_rate,
            output_channels,
            maximum,
            resident_hint,
            cancelled,
        )
        .map_err(|error| error.to_string())?
    {
        return Ok(sample);
    }
    let decoded = decode_audio_snapshot(
        transaction
            .snapshot_file()
            .map_err(|error| error.to_string())?,
        source,
        output_rate,
        maximum,
        cancelled,
        &mut *progress,
    )
    .map_err(|error| error.to_string())?;
    let (sample, transform) = prepare_playback(
        &decoded,
        output_channels,
        output_rate,
        maximum,
        cancelled,
        &mut *progress,
    )
    .map_err(|error| error.to_string())?;
    transaction
        .write_pcm_artifacts(
            PcmArtifactInput {
                samples: &decoded.samples,
                rate_hz: decoded.rate_hz,
                channels: decoded.channels,
                provenance: decoded.decoder.to_json(),
            },
            PcmArtifactInput {
                samples: &sample.samples,
                rate_hz: output_rate,
                channels: sample.channels,
                provenance: json!({"processing":"full-buffer-playback-v1"}),
            },
            transform.to_json(),
            cancelled,
        )
        .map_err(|error| error.to_string())?;
    Ok(sample)
}

fn reference(samples_root: &Path, path: &Path) -> Result<String, String> {
    let root = std::fs::canonicalize(samples_root).map_err(|error| error.to_string())?;
    let resolved = material_paths::resolve(&root, path).map_err(|error| error.to_string())?;
    let relative = resolved
        .path
        .strip_prefix(&root)
        .map_err(|_| "migration reference escaped samples root")?;
    Ok(format!(
        "samples/{}",
        relative.to_string_lossy().replace('\\', "/")
    ))
}

pub(super) fn prepare_material(
    samples_root: &Path,
    source: &Path,
    output_rate: u32,
    output_channels: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<PreparedMigrationMaterial, String> {
    // Resolve the actual old typed original before import creates any directories.
    let old = material_paths::resolve(samples_root, source).map_err(|error| error.to_string())?;
    if !matches!(old.kind, AssetKind::Original { .. }) {
        return Err("migration source is not a typed project original".into());
    }
    let old_original = old.path;
    let old_reference = reference(samples_root, &old_original)?;
    let mut transaction = ColdTransaction::capture(samples_root, &old_original, true, cancelled)
        .map_err(|error| error.to_string())?;
    let _preparation = transaction
        .preparation_gate(output_rate, output_channels, cancelled)
        .map_err(|error| error.to_string())?;
    // Retry and concurrent followers first verify the canonical target under
    // the same material/transform gate; old backing is copied only once.
    let mut sample = if let Some(sample) = transaction
        .try_reuse_selected(
            output_rate,
            output_channels,
            PCM_LIMIT_BYTES,
            None,
            cancelled,
        )
        .map_err(|error| error.to_string())?
    {
        sample
    } else if let Some(sample) = transaction
        .try_copy_legacy_pcm(
            samples_root,
            &old_original,
            output_rate,
            output_channels,
            PCM_LIMIT_BYTES,
            cancelled,
        )
        .map_err(|error| error.to_string())?
    {
        sample
    } else {
        prepare_pcm(
            &mut transaction,
            &old_original,
            output_rate,
            output_channels,
            PCM_LIMIT_BYTES,
            None,
            cancelled,
            &mut |_| {},
        )?
    };
    cold_residency::attach(
        &mut sample,
        transaction.manifest().map_err(|error| error.to_string())?,
    )?;
    // A new pathname/source assignment always gets new backing, even when a warm
    // candidate's immutable PCM is also retained by the old source generation.
    let overlap = sample
        .samples
        .len()
        .checked_mul(8)
        .ok_or("migration PCM extent overflow")?;
    if overlap > PCM_LIMIT_BYTES {
        return Err("migration fresh PCM exceeds transient byte limit".into());
    }
    if cancelled() {
        return Err("material migration cancelled".into());
    }
    sample.samples = Arc::from(sample.samples.as_ref());
    transaction.record_assignment_copy((overlap / 2) as u64);
    transaction
        .commit(cancelled)
        .map_err(|error| error.to_string())?;
    // Keep the transaction's rollback ownership until every fallible reference
    // construction has finished. into_lease then only transfers sealed ownership.
    let new_reference = reference(samples_root, transaction.original_path_for_migration())?;
    let cache_reference = reference(
        samples_root,
        transaction
            .cache_path_for_migration()
            .map_err(|error| error.to_string())?,
    )?;
    transaction.bind_pcm(&sample.samples);
    let lease = transaction.into_lease();
    Ok(PreparedMigrationMaterial {
        sample,
        lease,
        old_original,
        old_reference,
        new_reference,
        cache_reference,
    })
}
