//! Off-thread access to already fully verified immutable complete PCM.
use super::*;
use crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES;
use crate::messages::{ResidentContext, ResidentSourceView};

impl ColdTransaction {
    pub(in crate::audio_engine) fn manifest(&self) -> io::Result<&ColdManifest> {
        self.manifest
            .as_ref()
            .ok_or_else(|| invalid("no complete manifest"))
    }
}

impl CommittedColdLease {
    pub(in crate::audio_engine) fn verify_reference(
        &self,
        reference: &SampleBuffer,
    ) -> io::Result<()> {
        let view = reference
            .residency
            .as_ref()
            .ok_or_else(|| invalid("source descriptor missing"))?;
        let expected = crate::audio_engine::cold_residency::identity(&self.manifest)
            .map_err(io::Error::other)?;
        if view.source.transform_sha256 != expected.transform_sha256
            || view.source.original_sha256 != expected.original_sha256
            || view.source.playback_sha256 != expected.playback_sha256
            || view.source.mono_sha256 != expected.mono_sha256
            || view.source.frame_count != expected.frame_count
            || view.source.channels != expected.channels
            || view.source.sample_rate_hz != expected.sample_rate_hz
            || view.source.source_zero_frame != expected.source_zero_frame
        {
            return Err(invalid("complete lease does not belong to resident source"));
        }
        Ok(())
    }

    pub(in crate::audio_engine) fn open_complete_reader(
        &self,
        reference: &SampleBuffer,
    ) -> io::Result<File> {
        self.verify_reference(reference)?;
        let path = self.cache_path.join("playback.f32le");
        reject_links(&path)?;
        let file = sealed_reader(&path)?;
        let expected_bytes = reference
            .frame_count()
            .checked_mul(reference.channels)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| invalid("complete reader extent overflow"))?;
        if self.cache.file_identities.get(1) != Some(&file_identity(&file)?)
            || file.metadata()?.len() != expected_bytes as u64
        {
            return Err(invalid(
                "complete reader FileID/extent differs from immutable lease",
            ));
        }
        Ok(file)
    }
    /// The same immutable FileID under held write/delete-excluding file/directory
    /// guards permits bounded rereads without claiming a mutable pathname is content.
    pub(in crate::audio_engine) fn read_complete(
        &self,
        reference: &SampleBuffer,
        maximum: usize,
    ) -> io::Result<SampleBuffer> {
        self.read_complete_cancellable(reference, maximum, &|| false)
    }

    pub(in crate::audio_engine) fn read_complete_cancellable(
        &self,
        reference: &SampleBuffer,
        maximum: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<SampleBuffer> {
        if cancelled() {
            return Err(invalid("complete source read cancelled"));
        }
        if reference.resident_start() == 0 && reference.resident_end() == reference.frame_count() {
            return Ok(reference.clone());
        }
        let view = reference
            .residency
            .as_ref()
            .ok_or_else(|| invalid("source descriptor missing"))?;
        let expected = crate::audio_engine::cold_residency::identity(&self.manifest)
            .map_err(io::Error::other)?;
        self.verify_reference(reference)?;
        let count = expected
            .frame_count
            .checked_mul(expected.channels)
            .ok_or_else(|| invalid("full extent overflow"))?;
        let held = reference
            .samples
            .len()
            .checked_mul(4)
            .ok_or_else(|| invalid("resident extent overflow"))?;
        let budget = maximum
            .min(PCM_LIMIT_BYTES)
            .checked_sub(held)
            .ok_or_else(|| invalid("complete reader resident budget"))?;
        if count.checked_mul(8).is_none_or(|n| n > budget) {
            return Err(invalid("complete reader exceeds transient PCM limit"));
        }
        let mut file = self.open_complete_reader(reference)?;
        let samples = warm::read_playback(
            &mut file,
            expected.frame_count,
            expected.channels,
            budget,
            &|| cancelled(),
            &mut IntegrityMetrics::default(),
        )?;
        Ok(SampleBuffer {
            channels: expected.channels,
            samples,
            residency: Some(Arc::new(ResidentSourceView {
                source: view.source.clone(),
                start_frame: 0,
                window_revision: view.window_revision,
                context: ResidentContext::FullTrack,
            })),
        })
    }

    #[cfg(test)]
    pub(in crate::audio_engine) fn live_cached_pcm_samples(&self) -> Option<usize> {
        self.cache
            .pcm
            .lock()
            .unwrap()
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .map(|samples| samples.len())
    }
}
