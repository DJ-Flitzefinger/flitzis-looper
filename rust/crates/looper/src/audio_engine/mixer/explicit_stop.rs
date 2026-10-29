//! Bounded callback effect of an already admitted singleton STOP or mode collapse.
use super::*;
use crate::audio_engine::audio_stream::AudioMessageSink;
use crate::audio_engine::global_playback_batch::ExplicitStopPlan;
use crate::audio_engine::hold_action::HoldActionState;
use crate::messages::AudioMessage;

impl RtMixer {
    /// False means pending capacity/placement; the caller retains the same queue head.
    pub(crate) fn execute_explicit_stop(
        &mut self,
        plan: &ExplicitStopPlan,
        frame: u64,
        messages: &mut impl AudioMessageSink,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        let ownership = self.input_runtime_ownership.clone();
        if ownership.placement_pending()
            || retirement.available_retirement_slots() < 4 * MAX_VOICES + 1
        {
            return false;
        }
        let retained = plan.target.filter(|target| {
            if !plan.collapse || !ownership.explicit_source_current(*target) {
                return false;
            }
            let Some(id) = ownership.callback_key_slot(target.key) else {
                return false;
            };
            let mut found = false;
            for voice in &self.voices {
                if voice.active && voice.sample_id == id {
                    if voice.paused
                        || !Self::hold_voice_belongs_to_content_rt(voice, id, target.key)
                    {
                        return false;
                    }
                    found = true;
                }
            }
            found
                && !self
                    .capture_hold_projection_rt(target.key)
                    .is_some_and(|hold| hold.effect_current)
        });
        let mut stop = [false; NUM_SAMPLES];
        for voice in &self.voices {
            if !voice.active {
                continue;
            }
            let id = voice.sample_id;
            let Some(key) = ownership.callback_content_key(id) else {
                continue;
            };
            if retained.is_some_and(|target| target.key == key) {
                continue;
            }
            if plan.sources.iter().any(|source| {
                source.key == key
                    && ownership.ordinary_source_epoch(key.cell()) == Some(source.source)
                    && Self::hold_voice_belongs_to_content_rt(voice, id, key)
            }) {
                stop[id] = true;
            }
        }
        if messages.available_audio_message_slots() < stop.iter().filter(|stop| **stop).count() {
            return false;
        }
        let Some(anchor) = ownership.slot_snapshot(0) else {
            return false;
        };
        let Ok(write) = ownership.prepare_slot_write_fixed(anchor, anchor.lifetime()) else {
            return false;
        };
        let mut feedback: [Option<(Option<AudioMessage>, usize)>; MAX_VOICES] =
            std::array::from_fn(|_| None);
        let mut count = 0;
        let result = ownership.commit_slot_write_fixed(
            &write,
            || {
                retained.is_none_or(|target| ownership.explicit_source_current(target))
                    && stop.iter().enumerate().all(|(id, stop)| {
                        !*stop || {
                            ownership.callback_content_key(id).is_some_and(|key| {
                                plan.sources.iter().any(|source| {
                                    source.key == key
                                        && ownership.ordinary_source_epoch(key.cell())
                                            == Some(source.source)
                                })
                            })
                        }
                    })
            },
            |_| {
                for (id, stop) in stop.into_iter().enumerate() {
                    if !stop {
                        continue;
                    }
                    feedback[count] = Some((
                        super::pitch_action::captured_feedback(&ownership, id, false),
                        id,
                    ));
                    count += 1;
                    if let Some(key) = ownership.callback_content_key(id) {
                        self.revoke_hold_for_content_rt(
                            key,
                            HoldActionState::Superseded,
                            frame,
                            retirement,
                        );
                    }
                    self.stop_sample_rt(id, retirement);
                }
            },
        );
        if result.is_err() {
            return false;
        }
        for (captured, id) in feedback.into_iter().take(count).flatten() {
            messages.push_captured_pad_feedback(captured, AudioMessage::SampleStopped { id });
        }
        true
    }
}
