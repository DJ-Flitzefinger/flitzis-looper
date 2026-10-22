//! Borrowed preflight for the existing caused-pause cohort. No source/map owner
//! or Native preparation permit is created by a copied continuation.

use super::RtMixer;
use crate::audio_engine::constants::MAX_VOICES;
use crate::audio_engine::content_placement::ContentKey;
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::source_reader::{
    ExplicitSeekMode, SourceReadPlan, StemReadSets, prepared_stem_set_for_render,
};
use crate::audio_engine::transport::TransportTimeline;

impl RtMixer {
    /// Only changed lanes participate. An already paused lane keeps its original
    /// cursor, capture fences and history even when another lane changes now.
    pub(super) fn mapped_hold_continuations_rt(
        &self,
        id: usize,
        key: ContentKey,
        changed_mask: u32,
        paused_at: Option<u64>,
        output_frame: u64,
        transport: Option<&TransportTimeline>,
    ) -> Option<[Option<SourcePlayback>; MAX_VOICES]> {
        let mut replacements = [None; MAX_VOICES];
        for (index, replacement) in replacements.iter_mut().enumerate() {
            let voice = &self.voices[index];
            if changed_mask & (1u32 << index) == 0 || !voice.source_playback.is_mapped() {
                continue;
            }
            let transport = transport?;
            let expected_frame = paused_at.unwrap_or(output_frame);
            let (pitch, key_lock) = self.effective_voice_tuple(index);
            if !Self::hold_voice_belongs_to_content_rt(voice, id, key)
                || voice.source_playback.mapped_output_frame() != Some(expected_frame)
                || key_lock
                || pitch.total_semitones() != 0
                || voice.stretch.has_active_native_ownership()
                || voice.stretch.has_retained_source_input()
                || self.pending_key_lock_tuples[id].is_some()
                || self.queued_key_lock_tuples[id].is_some()
            {
                return None;
            }
            let owner = voice.mapped_start.as_ref()?;
            let geometry = owner.mapped_geometry()?;
            let sample = voice.sample.as_ref()?;
            let epoch = voice.source_playback.mapped_epoch()?;
            let generation = self.input_runtime_ownership.callback_source_generation(
                id,
                sample,
                self.sample_rate_hz as u32,
            )?;
            if !owner.belongs_to(&self.input_runtime_ownership)
                || owner.content_key() != key
                || owner.source_sync_witness(key, generation).is_none()
                || !epoch.matches_timeline(transport)
                || geometry.validate_epoch(epoch).is_err()
                || !voice.source_playback.matches_domain(geometry.domain())
                || !voice.source_admission.as_ref()?.current_for_birth(
                    &self.input_runtime_ownership,
                    &self.current_timing_acknowledgements,
                    sample,
                    self.sample_rate_hz as u32,
                    false,
                )
            {
                return None;
            }
            let timing = self.timing_for_voice(voice);
            let region = voice
                .source_loop_region
                .or_else(|| self.effective_loop_region(id, sample.frame_count()))?;
            if region != geometry.domain().region {
                return None;
            }
            let frozen = voice.frozen_stems.as_ref().filter(|_| {
                !self.sample_bank[id]
                    .as_ref()
                    .is_some_and(|bank| bank.same_source(sample))
            });
            let stems = prepared_stem_set_for_render(
                frozen.map_or(self.prepared_stems[id].as_ref(), |view| view.set.as_ref()),
                sample,
                self.channels,
                self.sample_rate_hz,
                sample.frame_count(),
                timing.accepted,
            );
            let outgoing = frozen.map_or(self.outgoing_stems[id].as_ref(), |view| {
                view.outgoing.as_ref()
            });
            let selection =
                frozen.map_or_else(|| self.stem_render_selection(id), |view| view.selection);
            let transition = frozen.map_or(self.stem_transitions[id], |view| view.transition);
            let mut playback = voice.source_playback;
            // An exact endpoint can still carry the preceding span's derivative.
            // Refresh the copy before checking the resumed read, never the live lane.
            playback.refresh_mapped_span(geometry).ok()?;
            if paused_at.is_some() {
                playback
                    .resume_mapped_at(geometry, transport, output_frame)
                    .ok()?;
                *replacement = Some(playback);
            }
            let position = playback.position();
            let plan = SourceReadPlan {
                channels: self.channels,
                sample_frames: sample.frame_count(),
                frame_pos: position.frame,
                loop_region: region,
                loop_period: playback.loop_period(),
                seek_mode: position.seek_mode,
                selection,
                transition,
            };
            if position.seek_mode != ExplicitSeekMode::Normal
                || voice.frame_pos != position.frame
                || transition.is_active()
                || !super::normal_loop_feed_available_with_readers(
                    sample,
                    StemReadSets::new(stems, outgoing),
                    self.sample_rate_hz as u32,
                    timing.accepted,
                    plan,
                    &playback,
                )
            {
                return None;
            }
        }
        Some(replacements)
    }
}
