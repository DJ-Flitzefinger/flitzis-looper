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
    /// Read final absolute source coordinates under this verified immutable lease.
    /// The independent file cursor and all allocations stay on the calling worker.
    pub(in crate::audio_engine) fn read_window_cancellable(
        &self,
        reference: &SampleBuffer,
        start: usize,
        end: usize,
        revision: u64,
        context: ResidentContext,
        maximum: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<SampleBuffer> {
        if cancelled() {
            return Err(invalid("resident source read cancelled"));
        }
        self.verify_reference(reference)?;
        let view = reference
            .residency
            .as_ref()
            .ok_or_else(|| invalid("source descriptor missing"))?;
        if !(1..=32).contains(&reference.channels)
            || reference.samples.is_empty()
            || !reference.samples.len().is_multiple_of(reference.channels)
            || reference
                .resident_start()
                .checked_add(reference.samples.len() / reference.channels)
                .is_none()
            || !reference.valid_residency(view.source.sample_rate_hz, view.source.channels)
            || start >= end
            || end > view.source.frame_count
            || revision == 0
            || (context != ResidentContext::FiniteLoop
                && (start != 0 || end != view.source.frame_count))
        {
            return Err(invalid(
                "resident range is outside its admitted source/context",
            ));
        }
        let count = (end - start)
            .checked_mul(reference.channels)
            .ok_or_else(|| invalid("resident range sample extent overflow"))?;
        let length = count
            .checked_mul(4)
            .ok_or_else(|| invalid("resident range byte extent overflow"))?;
        start
            .checked_mul(reference.channels)
            .and_then(|n| n.checked_mul(4))
            .and_then(|n| n.checked_add(length))
            .ok_or_else(|| invalid("resident absolute byte extent overflow"))?;
        let held = reference
            .samples
            .len()
            .checked_mul(4)
            .ok_or_else(|| invalid("held resident extent overflow"))?;
        let maximum = maximum.min(PCM_LIMIT_BYTES);
        let same_range = start == reference.resident_start() && end == reference.resident_end();
        let peak = if same_range {
            held
        } else {
            held.checked_add(length)
                .and_then(|n| n.checked_add(CHUNK_BYTES))
                .ok_or_else(|| invalid("resident range peak overflow"))?
        };
        if peak > maximum {
            return Err(invalid("resident range exceeds transient PCM admission"));
        }
        // Even the sharing path validates the actual held FileID and full extent.
        let mut reader = self.open_complete_reader(reference)?;
        if cancelled() {
            return Err(invalid("resident source read cancelled"));
        }
        let samples = if same_range {
            #[cfg(test)]
            warm::observe_window_start(start, end);
            reference.samples.clone()
        } else {
            warm::read_playback_range(
                &mut reader,
                start,
                end - start,
                reference.channels,
                maximum - held,
                &|| cancelled(),
                &mut IntegrityMetrics::default(),
            )?
        };
        if cancelled() {
            return Err(invalid("resident source read cancelled"));
        }
        Ok(SampleBuffer {
            channels: reference.channels,
            samples,
            residency: Some(Arc::new(ResidentSourceView {
                source: view.source.clone(),
                start_frame: start,
                window_revision: revision,
                context,
            })),
        })
    }

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
