//! Complete material preparation for migration, without source publication or analysis.

use super::cold_jobs::PCM_LIMIT_BYTES;
use super::cold_residency::{self, ResidentLoadHint};
use super::cold_store::{ColdTransaction, CommittedColdLease};
use super::material_paths::{self, AssetKind};
use super::sample_loader::{
    SampleLoadProgress, decode_audio_snapshot_streamed, prepare_playback_streamed,
};
use crate::messages::SampleBuffer;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

pub(super) struct PreparedMigrationMaterial {
    /// Complete identity plus bounded preparation view. Subscribers load their
    /// own actual loop/ALL demand from the sealed lease before publication.
    pub sample: SampleBuffer,
    pub lease: CommittedColdLease,
    old_reference: String,
    new_reference: String,
    cache_reference: String,
}

impl PreparedMigrationMaterial {
    /// Borrow existing admitted immutable material for off-thread derivative preparation.
    /// Cloning the held lease grants no new subscriber or source/ACK authority.
    pub(super) fn from_current(
        samples_root: &Path,
        sample: SampleBuffer,
        lease: CommittedColdLease,
    ) -> Result<Self, String> {
        lease
            .verify_reference(&sample)
            .map_err(|error| error.to_string())?;
        if lease.material_id.is_none() {
            return Err("canonical material is required for current stem pair".into());
        }
        let new_reference = reference(samples_root, &lease.original_path)?;
        let cache_reference = reference(samples_root, &lease.cache_path)?;
        Ok(Self {
            sample,
            lease,
            old_reference: new_reference.clone(),
            new_reference,
            cache_reference,
        })
    }
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
            "artifact_ledger_schema": 1,
            "created_original": self.lease.created_original(),
            "created_cache": self.lease.created_cache(),
            "material_creation": self.lease.material_creation_receipt(),
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
    prepare_disk_pcm(
        transaction,
        source,
        output_rate,
        output_channels,
        maximum,
        cancelled,
        progress,
    )?;
    transaction
        .read_prepared_selection(resident_hint, maximum, cancelled)
        .map_err(|error| error.to_string())
}

/// Prepare and verify complete immutable disk PCM without allocating a live view.
/// Callers can perform activity/analysis preparation before selecting actual demand.
pub(super) fn prepare_disk_pcm(
    transaction: &mut ColdTransaction,
    source: &Path,
    output_rate: u32,
    output_channels: usize,
    maximum: usize,
    cancelled: &impl Fn() -> bool,
    progress: &mut impl FnMut(SampleLoadProgress),
) -> Result<(), String> {
    if transaction
        .try_reuse_prepared(output_rate, output_channels, maximum, cancelled)
        .map_err(|error| error.to_string())?
    {
        return Ok(());
    }
    let (decoder_path, playback_path) = transaction
        .streamed_artifact_paths()
        .map_err(|error| error.to_string())?;
    let mut decoded = decode_audio_snapshot_streamed(
        transaction
            .snapshot_file()
            .map_err(|error| error.to_string())?,
        source,
        &decoder_path,
        output_rate,
        maximum,
        cancelled,
        &mut *progress,
    )
    .map_err(|error| error.to_string())?;
    let (playback, transform) = prepare_playback_streamed(
        &mut decoded,
        &playback_path,
        output_channels,
        output_rate,
        maximum,
        cancelled,
        &mut *progress,
    )
    .map_err(|error| error.to_string())?;
    transaction.record_streaming_scratch(decoded.scratch_reserved_bytes);
    transaction
        .finish_streamed_artifacts(decoded.artifact, playback, transform.to_json(), cancelled)
        .map_err(|error| error.to_string())?;
    Ok(())
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
    // Preparation carries complete immutable identity and one actual frame;
    // playable subscribers separately request their saved region or explicit ALL.
    prepare_material_selected(
        samples_root,
        source,
        output_rate,
        output_channels,
        cancelled,
        Some(ResidentLoadHint {
            start_s: 0.0,
            end_s: 1.0 / f64::from(output_rate),
            key_lock: false,
        }),
    )
}

pub(super) fn prepare_material_for_reference(
    samples_root: &Path,
    source: &Path,
    output_rate: u32,
    output_channels: usize,
    cancelled: &impl Fn() -> bool,
    reference: &SampleBuffer,
) -> Result<PreparedMigrationMaterial, String> {
    if !reference.valid_residency(output_rate, output_channels) {
        return Err("material preparation reference has invalid residency".into());
    }
    let hint = Some(ResidentLoadHint {
        start_s: reference.resident_start() as f64 / f64::from(output_rate),
        end_s: reference.resident_end() as f64 / f64::from(output_rate),
        key_lock: false,
    });
    prepare_material_selected(
        samples_root,
        source,
        output_rate,
        output_channels,
        cancelled,
        hint,
    )
}

/// Existing small independent fixtures explicitly demand all of their PCM.
/// Productive preparation always uses the bounded seed/reference entry points.
#[cfg(test)]
pub(super) fn prepare_material_complete_for_test(
    samples_root: &Path,
    source: &Path,
    output_rate: u32,
    output_channels: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<PreparedMigrationMaterial, String> {
    let mut material = prepare_material(
        samples_root,
        source,
        output_rate,
        output_channels,
        cancelled,
    )?;
    material.sample = material
        .lease
        .read_complete_cancellable(&material.sample, PCM_LIMIT_BYTES, cancelled)
        .map_err(|error| error.to_string())?;
    material.lease.bind_pcm(&material.sample.samples);
    Ok(material)
}

fn prepare_material_selected(
    samples_root: &Path,
    source: &Path,
    output_rate: u32,
    output_channels: usize,
    cancelled: &impl Fn() -> bool,
    resident_hint: Option<ResidentLoadHint>,
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
            resident_hint,
            cancelled,
        )
        .map_err(|error| error.to_string())?
    {
        sample
    } else if let Some(sample) = transaction
        .try_copy_legacy_pcm_selected(
            samples_root,
            &old_original,
            output_rate,
            output_channels,
            PCM_LIMIT_BYTES,
            resident_hint,
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
            resident_hint,
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
        old_reference,
        new_reference,
        cache_reference,
    })
}
