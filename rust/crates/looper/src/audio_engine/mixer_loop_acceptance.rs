//! Fixed effective-voice observation, separated from productive mixer rendering.

use super::*;

impl RtMixer {
    /// Demand-only effective-voice observation; fixed words and one bounded voice scan.
    pub(crate) fn loop_acceptance_snapshot(
        &self,
        request: u64,
        observed_ns: u64,
        next_output_frame: u64,
        callback_frames: usize,
    ) -> crate::audio_engine::loop_acceptance::LoopAcceptanceSnapshot {
        let mut result = crate::audio_engine::loop_acceptance::LoopAcceptanceSnapshot::empty(
            request,
            observed_ns,
            next_output_frame,
            callback_frames,
        );
        let id = (request & 255) as usize;
        if id >= NUM_SAMPLES {
            return result;
        }
        let mut voices = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.active && voice.sample_id == id);
        let Some((index, voice)) = voices.next() else {
            return result;
        };
        if voices.next().is_some() {
            result.0[0] = 2;
            return result;
        }
        result.0[0] = 3;
        let Some(sample) = voice.sample.as_ref() else {
            return result;
        };
        let Some(accepted) = voice.source_timing.accepted else {
            return result;
        };
        let Some(generation) =
            self.input_runtime_ownership
                .source_generation(id, sample, accepted.sample_rate_hz)
        else {
            return result;
        };
        let Some(region) = effective_loop_region(
            self.pad_loop_start_frame[id],
            self.pad_loop_end_frame[id],
            sample.source_sample_count() / sample.channels,
        ) else {
            return result;
        };
        let position = voice.source_playback.position();
        let (native_active, pitch_scale, input_fifo, output_fifo, block_size) =
            voice.stretch.loop_acceptance_state();
        let (eq_current, eq_target) = self.pad_dsp_chains[id].loop_acceptance_parameters();
        let stems = prepared_stem_set_for_render(
            self.prepared_stems[id].as_ref(),
            sample,
            self.channels,
            self.sample_rate_hz,
            sample.source_sample_count() / sample.channels,
            voice.source_timing.accepted,
        );
        let stems_ready = stems.is_some();
        let stem_all_applied = self.stem_mix_mode[id] == StemMixMode::AllStems
            && self.stem_mix_source_version_hash[id] != 0
            && stems.is_some_and(|stems| {
                stems.source_version_hash == self.stem_mix_source_version_hash[id]
            });
        result.0[0] = 1;
        result.0[6..15].copy_from_slice(&[
            sample.source_address() as u64,
            sample.source_sample_count() as u64,
            sample.channels as u64,
            generation,
            self.input_runtime_ownership.authority[id].load(std::sync::atomic::Ordering::Acquire),
            accepted.publication_epoch,
            accepted.period_seconds.to_bits(),
            accepted.origin_seconds.to_bits(),
            u64::from(accepted.sample_rate_hz),
        ]);
        for (word, bytes) in result.0[15..19]
            .iter_mut()
            .zip(accepted.revision.chunks_exact(8))
        {
            *word = u64::from_le_bytes(bytes.try_into().expect("eight revision bytes"));
        }
        result.0[19..].copy_from_slice(&[
            index as u64,
            u64::from(voice.paused),
            position.frame as u64,
            position.fraction.to_bits(),
            region.start as u64,
            region.end as u64,
            voice
                .source_playback
                .loop_period()
                .unwrap_or(f64::NAN)
                .to_bits(),
            voice.source_playback.tempo_ratio().to_bits(),
            voice.source_playback.rate_target().to_bits(),
            u64::from(self.pad_key_lock_enabled[id]),
            u64::from(native_active),
            pitch_scale.to_bits(),
            u64::from(self.stem_mix_mode[id] == StemMixMode::AllStems),
            self.stem_mix_source_version_hash[id],
            u64::from(self.stem_enabled_mask[id]),
            u64::from(stems_ready),
            u64::from(self.stem_transitions[id].is_active()),
            f64::from(eq_current[0]).to_bits(),
            f64::from(eq_current[1]).to_bits(),
            f64::from(eq_current[2]).to_bits(),
            f64::from(eq_target[0]).to_bits(),
            f64::from(eq_target[1]).to_bits(),
            f64::from(eq_target[2]).to_bits(),
            f64::from(self.pad_gain_smoothers[id].current).to_bits(),
            f64::from(self.pad_gain_smoothers[id].target).to_bits(),
            f64::from(self.volume).to_bits(),
            f64::from(voice.volume).to_bits(),
            u64::from(self.bpm_lock_enabled),
            self.master_period_seconds.unwrap_or(f64::NAN).to_bits(),
            input_fifo as u64,
            output_fifo as u64,
            block_size as u64,
            u64::from(stem_all_applied),
            match position.seek_mode {
                crate::audio_engine::source_reader::ExplicitSeekMode::Normal => 0,
                crate::audio_engine::source_reader::ExplicitSeekMode::BeforeLoop => 1,
                crate::audio_engine::source_reader::ExplicitSeekMode::AfterLoop => 2,
            },
            u64::from(
                voice.source_playback.tempo_ratio().to_bits()
                    == voice.source_playback.rate_target().to_bits(),
            ),
        ]);
        result
    }
}
