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

#[cfg(test)]
thread_local! {
    static BEFORE_CLEANUP_INTENT_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn before_cleanup_intent_read() {
    let hook = BEFORE_CLEANUP_INTENT_READ.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

impl RtMixer {
    #[cfg(test)]
    pub(crate) fn with_hold_cleanup_intent_interleaving_for_test<T>(
        hook: impl FnOnce() + 'static,
        action: impl FnOnce() -> T,
    ) -> T {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                BEFORE_CLEANUP_INTENT_READ.with(|slot| *slot.borrow_mut() = None);
            }
        }
        BEFORE_CLEANUP_INTENT_READ.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(Box::new(hook));
        });
        let _reset = Reset;
        let result = action();
        assert!(
            BEFORE_CLEANUP_INTENT_READ.with(|slot| slot.borrow().is_none()),
            "requested Hold cleanup interleaving was never reached"
        );
        result
    }

    #[cfg(test)]
    pub(crate) fn apply_transport_content_rt(
        &mut self,
        key: ContentKey,
        effect: crate::messages::TransportContentEffect,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        self.apply_transport_content_with_timeline_rt(
            key,
            effect,
            output_frame,
            None,
            None,
            retirement,
        );
    }

    pub(crate) fn apply_transport_content_with_timeline_rt(
        &mut self,
        key: ContentKey,
        effect: crate::messages::TransportContentEffect,
        output_frame: u64,
        transport: Option<&crate::audio_engine::transport::TransportTimeline>,
        accepted_revision: Option<u64>,
        retirement: &mut impl AudioBufferRetirement,
    ) -> (u64, bool, bool) {
        use crate::audio_engine::ordinary_transport::{APPLIED, NO_EFFECT, REFUSED};
        use crate::messages::TransportContentEffect;
        let Some(id) = self.content_effect_slot(key) else {
            return (REFUSED, false, false);
        };
        // The original admitted ordinary gesture already superseded this exact Hold's
        // release authority. Consume only its still-valid own effect, even if the
        // requested trajectory change is subsequently refused. Legacy numerical
        // helpers have no admitted revision and cannot manufacture this takeover.
        let accepted_hold_pause = accepted_revision.is_some_and(|revision| {
            self.consume_ordinary_hold_cause_rt(key, revision, output_frame, retirement)
        });
        let previous_paused = accepted_hold_pause
            || self
                .input_runtime_ownership
                .ordinary_snapshot(key)
                .is_some_and(|value| value.paused);
        let mut cohort = 0u32;
        let mut changed = 0u32;
        let mut unowned_lane = false;
        for (index, voice) in self.voices.iter().enumerate() {
            if voice.is_playing_sample(id) {
                if !Self::hold_voice_belongs_to_content_rt(voice, id, key) {
                    unowned_lane = true;
                    continue;
                }
                cohort |= 1u32 << index;
                if match effect {
                    TransportContentEffect::Pause => !voice.paused,
                    TransportContentEffect::Resume => voice.paused,
                    _ => false,
                } {
                    changed |= 1u32 << index;
                }
            }
        }
        if unowned_lane {
            return (REFUSED, cohort != 0, previous_paused);
        }
        match effect {
            TransportContentEffect::Pause | TransportContentEffect::Resume => {
                let Some(replacements) = self.mapped_transport_continuations_rt(
                    id,
                    key,
                    (cohort, changed),
                    matches!(effect, TransportContentEffect::Resume),
                    output_frame,
                    transport,
                ) else {
                    return (REFUSED, cohort != 0, previous_paused);
                };
                self.revoke_hold_for_content_rt(
                    key,
                    HoldActionState::Superseded,
                    output_frame,
                    retirement,
                );
                for (index, voice) in self.voices.iter_mut().enumerate() {
                    if changed & (1u32 << index) != 0 {
                        if let Some(playback) = replacements[index] {
                            voice.source_playback = playback;
                        }
                        if matches!(effect, TransportContentEffect::Pause) {
                            voice.pause();
                        } else {
                            voice.resume();
                        }
                    }
                }
                self.publish_key_lock_status(id);
                (
                    if changed == 0 { NO_EFFECT } else { APPLIED },
                    cohort != 0,
                    cohort != 0 && matches!(effect, TransportContentEffect::Pause),
                )
            }
            TransportContentEffect::Seek {
                position_s,
                expected_source_generation,
            } => {
                if self.hold_content_is_mapped_rt(id, key) {
                    return (REFUSED, cohort != 0, previous_paused);
                }
                let held_cohort = self.hold_actions[key.cell().index()]
                    .as_ref()
                    .filter(|owned| owned.action.key() == key)
                    .and_then(|owned| owned.action.effect())
                    .is_some_and(|held| self.hold_effect_cohort_current_rt(id, held));
                self.revoke_hold_for_content_rt(
                    key,
                    HoldActionState::Superseded,
                    output_frame,
                    retirement,
                );
                let applied = self
                    .expected_source_generation_current(id, expected_source_generation)
                    && self.seek_sample_at_output_frame(id, position_s, output_frame);
                let takeover = held_cohort
                    && cohort != 0
                    && self
                        .voices
                        .iter()
                        .enumerate()
                        .all(|(index, voice)| cohort & (1u32 << index) == 0 || voice.paused);
                (
                    if applied {
                        APPLIED
                    } else if cohort == 0 {
                        NO_EFFECT
                    } else {
                        REFUSED
                    },
                    cohort != 0,
                    previous_paused || takeover,
                )
            }
        }
    }

    fn pending_ordinary_hold_cause_rt(&self, action: &Arc<NativeHoldAction>) -> bool {
        let key = action.key();
        self.input_runtime_ownership
            .pending_ordinary_origin(key.cell())
            > action.transport_revision()
            && self.hold_actions[key.cell().index()]
                .as_ref()
                .is_some_and(|owned| Arc::ptr_eq(&owned.action, action))
            && self.content_effect_slot(key).is_some_and(|id| {
                action
                    .effect()
                    .is_some_and(|effect| self.hold_effect_cohort_current_rt(id, effect))
            })
    }

    /// Admission superseded release authority already. Only its exact, still-current
    /// caused effect can become an ordinary pause cause, including a fenced release
    /// that settled Superseded while the ordinary command waited for feedback space.
    pub(crate) fn consume_ordinary_hold_cause_rt(
        &mut self,
        key: ContentKey,
        revision: u64,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        let accepted = self.hold_actions[key.cell().index()]
            .as_ref()
            .is_some_and(|owned| {
                let action = &owned.action;
                action.key() == key
                    && action.transport_revision() < revision
                    && !action.intent_current()
                    && (action.state() == HoldActionState::PausedOwned
                        || (action.state() == HoldActionState::Superseded
                            && self.pending_ordinary_hold_cause_rt(action)))
                    && self.content_effect_slot(key).is_some_and(|id| {
                        action
                            .effect()
                            .is_some_and(|effect| self.hold_effect_cohort_current_rt(id, effect))
                    })
            });
        if accepted {
            self.revoke_hold_for_content_rt(
                key,
                HoldActionState::Superseded,
                output_frame,
                retirement,
            );
        }
        accepted
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
                Some(id)
                    if action
                        .effect()
                        .is_none_or(|effect| !self.hold_effect_cohort_current_rt(id, effect)) =>
                {
                    Some(HoldActionState::Superseded)
                }
                Some(_) => {
                    // An ordinary admission publishes custody before its Release
                    // transport fence. Observe that fence first: acquiring its new
                    // revision orders the subsequent custody read. The inverse order
                    // could see old zero custody followed by the superseding revision.
                    #[cfg(test)]
                    before_cleanup_intent_read();
                    let intent_current = action.intent_current();
                    let pending_ordinary = self.pending_ordinary_hold_cause_rt(action);
                    (!intent_current && !pending_ordinary).then_some(HoldActionState::Superseded)
                }
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
    #[cfg(test)]
    pub(crate) fn apply_pause_hold_rt(
        &mut self,
        work: Box<HoldActionRtWork>,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> NativeHoldProjection {
        self.apply_pause_hold_with_timeline_rt(work, output_frame, None, retirement)
    }

    pub(crate) fn apply_pause_hold_with_timeline_rt(
        &mut self,
        work: Box<HoldActionRtWork>,
        output_frame: u64,
        transport: Option<&crate::audio_engine::transport::TransportTimeline>,
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
        // Legacy numerical helpers carry no actual callback epoch and remain
        // closed even when their artificial mapped cohort is already paused.
        if transport.is_none() && self.hold_content_is_mapped_rt(id, key) {
            action.settle(HoldActionState::Rejected, output_frame);
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

        if self
            .mapped_hold_continuations_rt(
                id,
                key,
                effect.changed_mask,
                None,
                output_frame,
                transport,
            )
            .is_none()
        {
            action.settle(HoldActionState::Rejected, output_frame);
            let projection = action.projection();
            retirement.retire_hold_action(work);
            return projection;
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
    #[cfg(test)]
    pub(crate) fn apply_release_hold_rt(
        &mut self,
        work: Box<HoldActionRtWork>,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> NativeHoldProjection {
        self.apply_release_hold_with_timeline_rt(work, output_frame, None, retirement)
    }

    pub(crate) fn apply_release_hold_with_timeline_rt(
        &mut self,
        work: Box<HoldActionRtWork>,
        output_frame: u64,
        transport: Option<&crate::audio_engine::transport::TransportTimeline>,
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
            if !self.pending_ordinary_hold_cause_rt(action) {
                self.retire_matching_hold_registry_rt(action, retirement);
            }
            retirement.retire_hold_action(work);
            return projection;
        }
        let state = match self.content_effect_slot(key) {
            None => HoldActionState::Retired,
            Some(_) if !action.intent_current() => HoldActionState::Superseded,
            Some(id) if transport.is_none() && self.hold_content_is_mapped_rt(id, key) => {
                HoldActionState::Rejected
            }
            Some(id) => {
                let same_owner = self.hold_actions[cell].as_ref().is_some_and(|owned| {
                    Arc::ptr_eq(&owned.action, action)
                        && owned.action.state() == HoldActionState::PausedOwned
                });
                match action.effect() {
                    Some(effect)
                        if same_owner && self.hold_effect_cohort_current_rt(id, effect) =>
                    {
                        if let Some(replacements) = self.mapped_hold_continuations_rt(
                            id,
                            key,
                            effect.changed_mask,
                            Some(effect.paused_at_output_frame),
                            output_frame,
                            transport,
                        ) {
                            for (index, voice) in self.voices.iter_mut().enumerate() {
                                if effect.changed_mask & (1u32 << index) != 0 {
                                    if let Some(playback) = replacements[index] {
                                        voice.source_playback = playback;
                                    }
                                    voice.resume();
                                }
                            }
                            self.publish_key_lock_status(id);
                            HoldActionState::Resumed
                        } else {
                            HoldActionState::Rejected
                        }
                    }
                    _ => HoldActionState::Superseded,
                }
            }
        };
        action.settle(state, output_frame);
        let projection = action.projection();
        if !self.pending_ordinary_hold_cause_rt(action) {
            self.retire_matching_hold_registry_rt(action, retirement);
        }
        retirement.retire_hold_action(work);
        projection
    }

    fn hold_content_is_mapped_rt(&self, id: usize, key: ContentKey) -> bool {
        // Ordinary transport stays closed until its durable revision-correct feedback exists.
        self.voices.iter().any(|voice| {
            Self::hold_voice_belongs_to_content_rt(voice, id, key)
                && voice.source_playback.is_mapped()
        })
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

    pub(super) fn hold_voice_belongs_to_content_rt(
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
