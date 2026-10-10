//! Explicit migration from verified legacy backing into an exclusively owned material.
use super::*;
use crate::audio_engine::material_paths::{self, AssetKind};

impl ColdTransaction {
    pub(in crate::audio_engine) fn original_path_for_migration(&self) -> &Path {
        &self.original_path
    }

    pub(in crate::audio_engine) fn cache_path_for_migration(&self) -> io::Result<&Path> {
        self.committed_cache
            .as_deref()
            .ok_or_else(|| invalid("migration cache is not committed"))
    }

    /// A legacy locator is admitted by its own normal, complete warm verifier.
    /// Nothing is accepted by basename, historical path or a rewritten identity.
    pub(in crate::audio_engine) fn try_copy_legacy_pcm(
        &mut self,
        samples_root: &Path,
        old_original: &Path,
        output_rate: u32,
        output_channels: usize,
        maximum: usize,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<Option<SampleBuffer>> {
        let old = material_paths::resolve(samples_root, old_original)?;
        if !matches!(old.kind, AssetKind::Original { material: None }) {
            return Ok(None);
        }
        if self.material_id.is_none() || self.manifest.is_some() || self.verified {
            return Err(invalid(
                "legacy PCM copy requires a fresh canonical transaction",
            ));
        }
        let mut legacy = ColdTransaction::capture(samples_root, &old.path, false, cancelled)?;
        if legacy.source_digest != self.source_digest || legacy.source_bytes != self.source_bytes {
            return Err(invalid(
                "legacy original differs from captured migration source",
            ));
        }
        let _preparation = legacy.preparation_gate(output_rate, output_channels, cancelled)?;
        let Some(sample) =
            legacy.try_reuse_selected(output_rate, output_channels, maximum, None, cancelled)?
        else {
            // Absent, corrupt or decoder/device-incompatible candidates remain
            // untouched. The caller can visibly perform the ordinary decoder path.
            return Ok(None);
        };
        let manifest = legacy.manifest()?.clone();
        let cache = legacy
            .reused_cache
            .as_ref()
            .ok_or_else(|| invalid("verified legacy cache readers missing"))?;
        let staging = self
            .staging
            .as_ref()
            .ok_or_else(|| invalid("migration staging missing"))?;
        let encoded =
            serde_json::to_vec(&canonical(&manifest.encoded())).map_err(io::Error::other)?;
        let manifest_digest = format!("{:x}", Sha256::digest(&encoded));
        for (index, name, digest, bytes) in [
            (
                0,
                "decoder.f32le",
                manifest.descriptor["decoder"]["pcm"]["interleaved_sha256"]
                    .as_str()
                    .ok_or_else(|| invalid("verified decoder digest missing"))?,
                manifest.descriptor["decoder"]["pcm"]["full_bytes"]
                    .as_u64()
                    .ok_or_else(|| invalid("verified decoder bytes missing"))?,
            ),
            (
                1,
                "playback.f32le",
                manifest.descriptor["playback"]["pcm"]["interleaved_sha256"]
                    .as_str()
                    .ok_or_else(|| invalid("verified playback digest missing"))?,
                manifest.descriptor["playback"]["pcm"]["full_bytes"]
                    .as_u64()
                    .ok_or_else(|| invalid("verified playback bytes missing"))?,
            ),
            (
                2,
                "manifest.json",
                manifest_digest.as_str(),
                encoded.len() as u64,
            ),
        ] {
            check_cancelled(cancelled)?;
            let source = cache.path.join(name);
            reject_links(&source)?;
            let mut reader = sealed_reader(&source)?;
            if cache.file_identities.get(index) != Some(&file_identity(&reader)?) {
                return Err(invalid(
                    "legacy artifact FileID changed after complete verification",
                ));
            }
            let destination = staging.join(name);
            let mut writer = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)?;
            let (copied_digest, copied_bytes) = copy_hashed(&mut reader, &mut writer, cancelled)?;
            drop(writer);
            if copied_digest != digest || copied_bytes != bytes {
                return Err(invalid(
                    "migration copy differs from verified complete legacy artifact",
                ));
            }
            let mut reopened = sealed_reader(&destination)?;
            verify_file(&mut reopened, digest, bytes, cancelled)?;
            self.artifact_readers.push(reopened);
            match index {
                0 => {
                    self.integrity.decoder_verify_bytes =
                        self.integrity.decoder_verify_bytes.saturating_add(bytes)
                }
                1 => {
                    self.integrity.playback_verify_bytes =
                        self.integrity.playback_verify_bytes.saturating_add(bytes)
                }
                _ => {
                    self.integrity.manifest_verify_bytes =
                        self.integrity.manifest_verify_bytes.saturating_add(bytes)
                }
            }
        }
        // Identical sealed bytes preserve the already-verified mono/geometry/EOF,
        // decoder policy and executed transform; commit reopens the renamed files.
        self.manifest = Some(manifest);
        self.integrity.warm = true;
        Ok(Some(sample))
    }
}
