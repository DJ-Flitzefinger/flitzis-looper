//! Scalar stem claims preserve the logical and genuinely ahead Native input clocks.
//! Only copied plans and reader identities are installed; PCM stays in existing slots.

use super::*;
use crate::audio_engine::prepared_native_history::SourceReadCheckpoint;

#[derive(Clone, Copy)]
pub(super) struct RetainedStemVoiceChange {
    logical: SourceReadCheckpoint,
    next_plan: SourceReadPlan,
    pitch_intent: PitchIntent,
    key_lock: bool,
    sample_rate_hz: u32,
    accepted: Option<AcceptedTimingProjection>,
}

impl RetainedStemVoiceChange {
    fn feed<'a>(
        &'a self,
        sample: &'a SampleBuffer,
        readers: StemReadSets<'a>,
        plan: SourceReadPlan,
    ) -> ProductiveSourceFeed<'a> {
        ProductiveSourceFeed {
            pitch_intent: self.pitch_intent,
            key_lock: self.key_lock,
            sample,
            stems: readers.current,
            outgoing_stems: readers.outgoing,
            sample_rate_hz: self.sample_rate_hz,
            accepted: self.accepted,
            plan,
            playback: &self.logical.playback,
            permit: None,
            output_frame: None,
        }
    }

    pub(super) fn available(
        &self,
        voice: &VoiceSlot,
        previous_readers: StemReadSets<'_>,
        next_readers: StemReadSets<'_>,
    ) -> bool {
        let Some(sample) = voice.sample.as_ref() else {
            return false;
        };
        voice.stretch.retained_stem_change_ready(
            &self.feed(sample, previous_readers, self.logical.plan),
            &self.feed(sample, next_readers, self.next_plan),
        )
    }

    pub(super) fn apply_claimed(
        self,
        voice: &mut VoiceSlot,
        previous_readers: StemReadSets<'_>,
        next_readers: StemReadSets<'_>,
    ) {
        let sample = voice.sample.as_ref().expect("preflighted retained source");
        voice.stretch.retain_stem_change_claimed(
            &self.feed(sample, previous_readers, self.logical.plan),
            &self.feed(sample, next_readers, self.next_plan),
        );
    }
}

impl RtMixer {
    pub(super) fn voice_keeps_frozen_stem_source(&self, voice: &VoiceSlot) -> bool {
        voice.frozen_stems.is_some()
            && !voice.sample.as_ref().is_some_and(|sample| {
                self.sample_bank[voice.sample_id]
                    .as_ref()
                    .is_some_and(|bank| bank.same_source(sample))
            })
    }

    pub(super) fn retained_stem_voice_change(
        &self,
        index: usize,
        selection: StemRenderSelection,
        transition: StemTransition,
    ) -> Option<RetainedStemVoiceChange> {
        let voice = &self.voices[index];
        let sample = voice.sample.as_ref()?;
        let id = voice.sample_id;
        let region = voice
            .source_loop_region
            .or_else(|| self.effective_loop_region(id, sample.frame_count()))?;
        let timing = self.timing_for_voice(voice);
        let mut playback = voice.source_playback;
        if !playback.set_target(self.tempo_ratio_for_source_period(timing.period_seconds()))
            || !playback.configure_domain(timing.loop_domain(sample.frame_count(), region))
        {
            return None;
        }
        playback.chunk(DEFAULT_BLOCK_SAMPLES);
        let position = playback.position();
        let plan = SourceReadPlan {
            channels: self.channels,
            sample_frames: sample.frame_count(),
            frame_pos: position.frame,
            loop_region: region,
            loop_period: playback.reader_period(),
            seek_mode: position.seek_mode,
            selection: self.stem_render_selection(id),
            transition: self.stem_transitions[id],
        };
        Some(RetainedStemVoiceChange {
            logical: SourceReadCheckpoint { playback, plan },
            next_plan: SourceReadPlan {
                selection,
                transition,
                ..plan
            },
            pitch_intent: self.pad_pitch_intent[id],
            key_lock: self.pad_key_lock_enabled[id],
            sample_rate_hz: self.sample_rate_hz as u32,
            accepted: timing.accepted,
        })
    }

    pub(super) fn retained_stem_selection_available(
        &self,
        id: usize,
        selection: StemRenderSelection,
        transition: StemTransition,
    ) -> bool {
        let readers = StemReadSets::new(
            self.prepared_stems[id].as_ref(),
            self.outgoing_stems[id].as_ref(),
        );
        self.voices.iter().enumerate().all(|(index, voice)| {
            if !voice.active || voice.sample_id != id {
                return true;
            }
            if voice.source_playback.is_mapped() {
                return false;
            }
            self.voice_keeps_frozen_stem_source(voice)
                || !voice.stretch.has_retained_source_input()
                || (voice.sample.as_ref().is_some_and(|sample| {
                    self.input_runtime_ownership.source_current(
                        id,
                        sample,
                        self.sample_rate_hz as u32,
                    ) && self.input_runtime_ownership.source_timing_available(
                        id,
                        self.timing_for_voice(voice).accepted,
                        &self.current_timing_acknowledgements,
                    )
                }) && self
                    .retained_stem_voice_change(index, selection, transition)
                    .is_some_and(|change| change.available(voice, readers, readers)))
        })
    }

    pub(super) fn retain_current_stem_selection_claimed(
        &mut self,
        id: usize,
        previous: StemRenderSelection,
        next: StemRenderSelection,
    ) {
        if previous == next {
            return;
        }
        let transition = StemTransition::start(previous, STEM_TRANSITION_RAMP_FRAMES);
        // Every participant was preflighted before the first Native or pad scalar store.
        for index in 0..self.voices.len() {
            let voice = &self.voices[index];
            if !voice.active
                || voice.sample_id != id
                || self.voice_keeps_frozen_stem_source(voice)
                || !voice.stretch.has_retained_source_input()
            {
                continue;
            }
            let change = self
                .retained_stem_voice_change(index, next, transition)
                .expect("preflighted retained stem trajectory");
            let readers = StemReadSets::new(
                self.prepared_stems[id].as_ref(),
                self.outgoing_stems[id].as_ref(),
            );
            change.apply_claimed(&mut self.voices[index], readers, readers);
        }
    }

    pub(super) fn retained_pad_stem_reader_required(
        &self,
        id: usize,
        stems: &PreparedStemSet,
    ) -> bool {
        self.voices.iter().any(|voice| {
            voice.active
                && voice.sample_id == id
                && !self.voice_keeps_frozen_stem_source(voice)
                && voice.stretch.retained_stem_reader_required(stems)
        })
    }
}
