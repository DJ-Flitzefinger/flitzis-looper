//! Observation only: rows share a completed callback boundary.
use super::*;
use crate::audio_engine::sync_diagnostics::{DiagnosticSnapshot, HEADER_WORDS, ROW_WORDS};
use crate::audio_engine::transport::TransportTimeline;

impl RtMixer {
    pub(crate) fn begin_sync_diagnostic_capture(&mut self, request: u64) {
        self.diagnostic_request_id = request;
        if request != 0 {
            for voice in &mut self.voices {
                voice.diagnostic_read = None;
            }
        }
    }

    pub(crate) fn end_sync_diagnostic_capture(&mut self) {
        self.diagnostic_request_id = 0;
    }

    pub(crate) fn sync_diagnostic_snapshot(
        &self,
        request: u64,
        birth: u64,
        timeline: &TransportTimeline,
        ns: u64,
        frames: usize,
    ) -> Option<DiagnosticSnapshot> {
        if request == 0 || self.diagnostic_request_id != request {
            return None;
        }
        let mut result = DiagnosticSnapshot([0; HEADER_WORDS + MAX_VOICES * ROW_WORDS]);
        result.0[..HEADER_WORDS].copy_from_slice(&[
            request,
            birth,
            ns,
            timeline.output_frame(),
            frames as u64,
            self.sample_rate_hz as u64,
            self.channels as u64,
            timeline.beat_position().unwrap_or(f64::NAN).to_bits(),
            timeline
                .master_period_seconds()
                .unwrap_or(f64::NAN)
                .to_bits(),
            timeline.phase_generation(),
            self.global_rate_timing_revision(),
            0,
        ]);
        let mut count = 0;
        for (index, voice) in self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.active)
        {
            let row = &mut result.0
                [HEADER_WORDS + count * ROW_WORDS..HEADER_WORDS + (count + 1) * ROW_WORDS];
            count += 1;
            self.fill_sync_diagnostic_row(index, voice, timeline.output_frame(), row);
        }
        result.0[11] = count as u64;
        Some(result)
    }

    fn fill_sync_diagnostic_row(&self, index: usize, voice: &VoiceSlot, end: u64, row: &mut [u64]) {
        for word in [9, 12, 13, 14, 15, 23, 24, 42] {
            row[word] = f64::NAN.to_bits();
        }
        row[0] = voice.sample_id as u64;
        row[1] = index as u64;
        row[2] = voice.generation;
        let position = voice.source_playback.position();
        row[8] = position.frame as u64;
        row[9] = position.fraction.to_bits();
        let timing = voice.source_timing;
        row[12] = timing.period_seconds().unwrap_or(f64::NAN).to_bits();
        row[13] = timing
            .accepted
            .map_or(timing.legacy_origin_frame, |timing| {
                timing.origin_seconds * f64::from(timing.sample_rate_hz)
            })
            .to_bits();
        row[14] = voice.source_playback.tempo_ratio().to_bits();
        row[15] = voice.source_playback.rate_target().to_bits();
        row[42] = voice
            .source_playback
            .loop_period()
            .unwrap_or(f64::NAN)
            .to_bits();
        let (native, pitch, input, output, block) = voice.stretch.loop_acceptance_state();
        row[24] = pitch.to_bits();
        row[25] = input as u64;
        row[26] = output as u64;
        row[27] = block as u64;
        let (pitch_intent, key_lock) = self.effective_rate_voice_tuple(index);
        row[28] = pitch_intent.total_semitones() as i64 as u64;
        let mut flags = u64::from(voice.paused)
            | (u64::from(native) << 1)
            | (u64::from(voice.phase_launch_qualified) << 2)
            | (u64::from(voice.phase_read_current()) << 3)
            | (u64::from(voice.source_playback.has_phase_program()) << 4)
            | (u64::from(timing.accepted.is_some()) << 5)
            | (u64::from(voice.source_admission.is_some()) << 6)
            | (u64::from(voice.source_playback.is_mapped()) << 10)
            | (u64::from(voice.source_playback.is_static_phase()) << 11)
            | (u64::from(key_lock) << 12)
            | (u64::from(self.pad_key_lock_enabled[voice.sample_id]) << 13)
            | (u64::from(voice.phase_birth_unadvanced) << 14);
        flags |= u64::from(voice.source_playback.is_physical_normalized_static()) << 15;
        row[17] = match voice.sync_request.mode {
            crate::audio_engine::source_sync_policy::SourceSyncMode::Legacy => 0,
            crate::audio_engine::source_sync_policy::SourceSyncMode::Off => 1,
            crate::audio_engine::source_sync_policy::SourceSyncMode::Fixed => 2,
            crate::audio_engine::source_sync_policy::SourceSyncMode::Mapped => 3,
        };
        row[19] = voice.sync_request.intent_revision;
        if let Some(revision) = voice.sync_request.global_revision {
            flags |= 1 << 16;
            row[18] = revision;
        }
        if let Some((revision, fence)) = voice.phase_policy_revision {
            flags |= 1 << 17;
            row[20] = revision;
            row[21] = fence;
        }
        if let Some((generation, frame, beat)) = voice.source_playback.diagnostic_launch_reference()
        {
            flags |= 1 << 18;
            row[22] = frame;
            row[23] = beat.to_bits();
            row[44] = generation;
        }
        if let Some(sample) = &voice.sample {
            row[3] = sample.source_address() as u64;
            row[6] = sample.window_revision();
            row[40] = (sample.source_sample_count() / sample.channels) as u64;
            row[41] = self.sample_rate_hz as u64;
            if let Some(region) = voice.source_loop_region.or_else(|| {
                effective_loop_region(
                    self.pad_loop_start_frame[voice.sample_id],
                    self.pad_loop_end_frame[voice.sample_id],
                    sample.frame_count(),
                )
            }) {
                row[10] = region.start as u64;
                row[11] = region.end as u64;
            }
            if let Some(admission) = &voice.source_admission {
                row[4] = admission.retry_identity().source_generation;
                if let Some(key) = admission.content_key() {
                    row[5] = key.lifetime();
                    row[7] = self
                        .input_runtime_ownership
                        .key_source_request_incarnation(key)
                        .unwrap_or(0);
                }
            }
        }
        row[43] = timing.accepted.map_or(0, |timing| timing.publication_epoch);
        if let Some(read) = voice
            .diagnostic_read
            .filter(|read| read.generation == voice.generation && read.output_end == end)
        {
            flags |= 1 << 7;
            row[29..40].copy_from_slice(&[
                read.output_start,
                read.output_end,
                read.source_frame as u64,
                read.source_fraction.to_bits(),
                read.domain,
                read.loop_start as u64,
                read.loop_end as u64,
                read.stems.0,
                read.stems.1,
                read.stems.2,
                u64::from(read.transition),
            ]);
        }
        row[16] = flags;
    }
}
