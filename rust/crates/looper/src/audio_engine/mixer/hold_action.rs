//! Actual caused-pause ownership inside the callback's existing mutation boundary.

use super::RtMixer;
use crate::audio_engine::buffer_retirement::AudioBufferRetirement;
use crate::audio_engine::constants::MAX_VOICES;
use crate::audio_engine::content_placement::ContentKey;
use crate::audio_engine::hold_action::{
    HoldActionRtWork, HoldActionState, HoldWorkKind, NativeHoldAction, NativeHoldProjection,
    NativePauseEffect,
};
use std::sync::Arc;

impl RtMixer {
    pub(crate) fn apply_transport_content_rt(
        &mut self,
        key: ContentKey,
        effect: crate::messages::TransportContentEffect,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        use crate::messages::TransportContentEffect;
        let Some(id) = self.content_effect_slot(key) else {
            return;
        };
        self.revoke_hold_for_content_rt(key, HoldActionState::Superseded, output_frame, retirement);
        match effect {
            TransportContentEffect::Pause | TransportContentEffect::Resume => {
                for voice in &mut self.voices {
                    if Self::hold_voice_belongs_to_content_rt(voice, id, key) {
                        if matches!(effect, TransportContentEffect::Pause) {
                            voice.pause();
                        } else {
                            voice.resume();
                        }
                    }
                }
                self.publish_key_lock_status(id);
            }
            TransportContentEffect::Seek {
                position_s,
                expected_source_generation,
            } => {
                if self.expected_source_generation_current(id, expected_source_generation)
                    && self
                        .voices
                        .iter()
                        .filter(|voice| voice.is_playing_sample(id))
                        .all(|voice| Self::hold_voice_belongs_to_content_rt(voice, id, key))
                {
                    self.seek_sample_at_output_frame(id, position_s, output_frame);
                }
            }
        }
    }
    /// Actual cohort replacement and source retirement also settle retained tickets.
    /// Capacity pressure keeps the exact owner in its registry for the next bounded retry.
    pub(crate) fn settle_invalid_hold_actions_rt(
        &mut self,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        for cell in 0..self.hold_actions.len() {
            let Some(work) = self.hold_actions[cell].as_ref() else {
                continue;
            };
            let action = &work.action;
            let key = action.key();
            let state = match self.content_effect_slot(key) {
                None => Some(HoldActionState::Retired),
                Some(_) if !action.intent_current() => Some(HoldActionState::Superseded),
                Some(id)
                    if action
                        .effect()
                        .is_none_or(|effect| !self.hold_effect_cohort_current_rt(id, effect)) =>
                {
                    Some(HoldActionState::Superseded)
                }
                _ => None,
            };
            if let Some(state) = state {
                if retirement.available_retirement_slots() == 0 {
                    return;
                }
                self.retire_content_actions_rt(key, state, output_frame, retirement);
            }
        }
    }
    /// The outer callback publisher owns the same-content marker and has already reserved
    /// retirement capacity. Stable-cell registry storage follows content, never pad images.
    pub(crate) fn apply_pause_hold_rt(
        &mut self,
        work: Box<HoldActionRtWork>,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> NativeHoldProjection {
        let action = &work.action;
        let key = action.key();
        let cell = key.cell().index();
        if work.kind != HoldWorkKind::Pause {
            action.settle(HoldActionState::Rejected, output_frame);
            let projection = action.projection();
            retirement.retire_hold_action(work);
            return projection;
        }
        if action.state() != HoldActionState::Admitted {
            // A repeated native observation cannot pause again or reopen a terminal ACK.
            if action.state() == HoldActionState::Prepared {
                action.settle(HoldActionState::Rejected, output_frame);
            }
            let projection = action.projection();
            retirement.retire_hold_action(work);
            return projection;
        }
        if !Arc::ptr_eq(action.ownership(), &self.input_runtime_ownership) {
            action.settle(HoldActionState::Rejected, output_frame);
            let projection = action.projection();
            retirement.retire_hold_action(work);
            return projection;
        }
        let Some(id) = self.content_effect_slot(key) else {
            action.settle(HoldActionState::Retired, output_frame);
            let projection = action.projection();
            retirement.retire_hold_action(work);
            return projection;
        };
        if !action.intent_current() {
            action.settle(HoldActionState::Superseded, output_frame);
            let projection = action.projection();
            retirement.retire_hold_action(work);
            return projection;
        }

        let mut effect = NativePauseEffect {
            key,
            action_id: action.action_id(),
            effect_id: action.action_id(),
            transport_revision: action.transport_revision(),
            cohort_mask: 0,
            changed_mask: 0,
            generations: [0; MAX_VOICES],
            paused_at_output_frame: output_frame,
        };
        for (index, voice) in self.voices.iter().enumerate() {
            if Self::hold_voice_belongs_to_content_rt(voice, id, key) {
                // Voice generation currently saturates. Never mint release authority at its
                // terminal value, where a subsequent lane reuse could compare equal.
                if voice.generation == 0 || voice.generation == u64::MAX {
                    action.settle(HoldActionState::Exhausted, output_frame);
                    let projection = action.projection();
                    retirement.retire_hold_action(work);
                    return projection;
                }
                let bit = 1u32 << index;
                effect.cohort_mask |= bit;
                effect.generations[index] = voice.generation;
                if !voice.paused {
                    effect.changed_mask |= bit;
                }
            }
        }

        // A later accepted physical hold may not inherit another hold's pause. Its producer
        // admission already superseded that older intent; retire only the original owner.
        if let Some(previous) = self.hold_actions[cell].take() {
            let reason = if previous.action.key() == key {
                HoldActionState::Superseded
            } else {
                HoldActionState::Retired
            };
            previous.action.settle(reason, output_frame);
            retirement.retire_hold_action(previous);
        }
        if effect.changed_mask == 0 {
            action.settle(HoldActionState::NoEffect, output_frame);
            let projection = action.projection();
            retirement.retire_hold_action(work);
            return projection;
        }

        for (index, voice) in self.voices.iter_mut().enumerate() {
            if effect.changed_mask & (1u32 << index) != 0 {
                voice.pause();
            }
        }
        action.publish_effect(effect);
        let projection = action.projection();
        self.hold_actions[cell] = Some(work);
        self.publish_key_lock_status(id);
        projection
    }

    /// Resume exactly the lanes that this ticket actually paused, only if the complete
    /// cohort and accepted intent still agree. All validation precedes the first change.
    pub(crate) fn apply_release_hold_rt(
        &mut self,
        work: Box<HoldActionRtWork>,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> NativeHoldProjection {
        let action = &work.action;
        let key = action.key();
        let cell = key.cell().index();
        if work.kind != HoldWorkKind::Release
            || !action.release_requested()
            || !action.release_admitted()
            || !Arc::ptr_eq(action.ownership(), &self.input_runtime_ownership)
        {
            action.settle(HoldActionState::Rejected, output_frame);
            let projection = action.projection();
            retirement.retire_hold_action(work);
            return projection;
        }
        if action.state().terminal() {
            let projection = action.projection();
            self.retire_matching_hold_registry_rt(action, retirement);
            retirement.retire_hold_action(work);
            return projection;
        }
        let state = match self.content_effect_slot(key) {
            None => HoldActionState::Retired,
            Some(_) if !action.intent_current() => HoldActionState::Superseded,
            Some(id) => {
                let same_owner = self.hold_actions[cell].as_ref().is_some_and(|owned| {
                    Arc::ptr_eq(&owned.action, action)
                        && owned.action.state() == HoldActionState::PausedOwned
                });
                match action.effect() {
                    Some(effect)
                        if same_owner && self.hold_effect_cohort_current_rt(id, effect) =>
                    {
                        for (index, voice) in self.voices.iter_mut().enumerate() {
                            if effect.changed_mask & (1u32 << index) != 0 {
                                voice.resume();
                            }
                        }
                        self.publish_key_lock_status(id);
                        HoldActionState::Resumed
                    }
                    _ => HoldActionState::Superseded,
                }
            }
        };
        action.settle(state, output_frame);
        let projection = action.projection();
        self.retire_matching_hold_registry_rt(action, retirement);
        retirement.retire_hold_action(work);
        projection
    }

    fn hold_effect_cohort_current_rt(&self, id: usize, effect: NativePauseEffect) -> bool {
        if effect.changed_mask == 0
            || effect.changed_mask & !effect.cohort_mask != 0
            || self.content_effect_slot(effect.key) != Some(id)
        {
            return false;
        }
        let mut actual_mask = 0u32;
        for (index, voice) in self.voices.iter().enumerate() {
            let bit = 1u32 << index;
            let belongs = Self::hold_voice_belongs_to_content_rt(voice, id, effect.key);
            if belongs {
                actual_mask |= bit;
            }
            if effect.cohort_mask & bit != 0
                && (!belongs
                    || voice.generation != effect.generations[index]
                    || voice.generation == u64::MAX
                    || !voice.paused)
            {
                return false;
            }
        }
        actual_mask == effect.cohort_mask
    }

    fn hold_voice_belongs_to_content_rt(
        voice: &crate::audio_engine::voice_slot::VoiceSlot,
        id: usize,
        key: ContentKey,
    ) -> bool {
        voice.is_playing_sample(id)
            && voice
                .source_admission
                .as_ref()
                .and_then(|admission| admission.content_key())
                == Some(key)
    }

    fn retire_matching_hold_registry_rt(
        &mut self,
        action: &Arc<NativeHoldAction>,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        let cell = action.key().cell().index();
        if self.hold_actions[cell]
            .as_ref()
            .is_some_and(|owned| Arc::ptr_eq(&owned.action, action))
        {
            // Native registry identity is exact; an older release cannot retire a newer hold.
            retirement.retire_hold_action(self.hold_actions[cell].take().unwrap());
        }
    }

    /// Pair ACK copies the native effect at its own claim. Placement does not modify this
    /// registry, its transport revision, effect id or voice lane generations.
    pub(crate) fn capture_hold_projection_rt(
        &self,
        key: ContentKey,
    ) -> Option<NativeHoldProjection> {
        self.hold_actions[key.cell().index()]
            .as_ref()
            .filter(|owned| owned.action.key() == key)
            .map(|owned| {
                let mut projection = owned.action.projection();
                projection.effect_current = projection.effect_current
                    && projection.effect.is_some_and(|effect| {
                        self.content_effect_slot(key)
                            .is_some_and(|id| self.hold_effect_cohort_current_rt(id, effect))
                    });
                projection
            })
    }

    /// Call in the same native claim that removes or supersedes the exact content. A stale
    /// key never touches an action in a reused cell, even if its old numeric slot matches.
    pub(crate) fn retire_content_actions_rt(
        &mut self,
        key: ContentKey,
        state: HoldActionState,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> Option<NativeHoldProjection> {
        debug_assert!(state.terminal());
        let cell = key.cell().index();
        if self.hold_actions[cell]
            .as_ref()
            .is_none_or(|owned| owned.action.key() != key)
        {
            return None;
        }
        let owned = self.hold_actions[cell].take().unwrap();
        owned.action.settle(state, output_frame);
        let projection = owned.action.projection();
        retirement.retire_hold_action(owned);
        Some(projection)
    }

    pub(crate) fn revoke_hold_for_content_rt(
        &mut self,
        key: ContentKey,
        state: HoldActionState,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> Option<NativeHoldProjection> {
        self.retire_content_actions_rt(key, state, output_frame, retirement)
    }
}

#[cfg(test)]
#[path = "hold_action_tests.rs"]
mod tests;
