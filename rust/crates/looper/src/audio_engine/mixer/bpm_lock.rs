//! Retained BPM intent, serviced on existing render boundaries without owning work.
use super::*;
use crate::audio_engine::content_placement::ContentKey;
use crate::audio_engine::global_rate_status::{BpmEffect, BpmReason};
use crate::audio_engine::input_runtime_binding::InputPadBinding;
use crate::audio_engine::master_provenance::MasterProvenance;
use crate::audio_engine::source_sync_policy::SourceSyncMode;
use crate::audio_engine::transport::TransportTimeline;
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BpmLockTarget {
    pub(crate) enabled: bool,
    pub(crate) revision: u64,
    pub(crate) anchor: Option<(usize, InputPadBinding, ContentKey)>,
    pub(crate) source_identity: Option<[u8; 32]>,
    pub(crate) hierarchy_stamp: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(mixer: &mut RtMixer, revision: u64, enabled: bool) -> BpmLockTarget {
        mixer
            .input_runtime_ownership
            .global_timing_revision
            .store(revision, Ordering::Release);
        mixer
            .input_runtime_ownership
            .global_rate_status
            .publish_bpm_requested(revision, enabled);
        BpmLockTarget {
            enabled,
            revision,
            anchor: None,
            source_identity: None,
            hierarchy_stamp: mixer
                .input_runtime_ownership
                .hierarchy_stamp
                .load(Ordering::Acquire),
        }
    }
    fn settle(mixer: &mut RtMixer) {
        let revision = mixer.global_rate_timing_revision();
        mixer.finish_global_control_drain(Some(revision), true);
        mixer.finish_global_parameter_drain(Some(revision), true);
    }
    fn reason(mixer: &RtMixer) -> BpmReason {
        mixer
            .input_runtime_ownership
            .global_rate_status
            .read_bpm()
            .unwrap()
            .1
            .reason
    }

    #[test]
    fn retained_target_waits_for_drains_and_unrelated_global_revision_does_not_erase_it() {
        let mut mixer = RtMixer::new(1, 48_000.0);
        let mut transport = TransportTimeline::new(48_000);
        let request = target(&mut mixer, 2, true);
        mixer.stage_bpm_lock(request);
        mixer.service_bpm_lock(11, &mut transport);
        assert_eq!(reason(&mixer), BpmReason::DrainPending);
        assert!(!mixer.bpm_lock_enabled);
        // An unrelated already-drained speed/global edit is not a hierarchy change.
        mixer
            .input_runtime_ownership
            .global_timing_revision
            .store(3, Ordering::Release);
        settle(&mut mixer);
        mixer.service_bpm_lock(13, &mut transport);
        let (revision, effect) = mixer
            .input_runtime_ownership
            .global_rate_status
            .read_bpm()
            .unwrap();
        assert_eq!(
            (revision, effect.revision, effect.execution_frame),
            (2, 2, Some(13))
        );
        assert!(effect.applied && effect.effective);
        assert!(mixer.pending_bpm_lock.is_none());
    }

    #[test]
    fn newer_target_supersedes_and_parent_aba_cannot_apply_an_old_unadmitted_opposite() {
        for initial in [false, true] {
            let mut mixer = RtMixer::new(1, 48_000.0);
            let mut transport = TransportTimeline::new(48_000);
            mixer.set_bpm_lock(initial);
            let old = target(&mut mixer, 2, !initial);
            mixer.stage_bpm_lock(old);
            mixer
                .input_runtime_ownership
                .hierarchy_stamp
                .store(3, Ordering::Release);
            mixer
                .input_runtime_ownership
                .global_timing_revision
                .store(3, Ordering::Release);
            settle(&mut mixer);
            mixer.service_bpm_lock(19, &mut transport);
            assert_eq!(reason(&mixer), BpmReason::ReferenceChanged);
            assert_eq!(mixer.bpm_lock_enabled, initial);
            let replacement = target(&mut mixer, 4, initial);
            mixer.stage_bpm_lock(replacement);
            settle(&mut mixer);
            mixer.service_bpm_lock(20, &mut transport);
            let effect = mixer
                .input_runtime_ownership
                .global_rate_status
                .read_bpm()
                .unwrap()
                .1;
            assert!(effect.applied);
            assert_eq!((effect.revision, effect.effective), (4, initial));
        }
    }

    #[test]
    fn off_waits_for_paused_and_fading_fixed_voice_policy_or_real_retirement() {
        let mut mixer = RtMixer::new(1, 48_000.0);
        let mut transport = TransportTimeline::new(48_000);
        mixer.load_sample(
            0,
            SampleBuffer {
                samples: Arc::from([0.25; 4096]),
                channels: 1,
                residency: None,
            },
        );
        mixer.set_pad_key_lock(0, false);
        mixer.set_bpm_lock(true);
        mixer.set_master_period(0.5);
        mixer.set_pad_bpm(0, Some(120.0));
        assert!(mixer.play_sample(0, 1.0));
        mixer.voices[0].sync_request.mode = SourceSyncMode::Fixed;
        mixer.pause_sample(0);
        let request = target(&mut mixer, 2, false);
        mixer.stage_bpm_lock(request);
        settle(&mut mixer);
        mixer.service_bpm_lock(10, &mut transport);
        assert_eq!(reason(&mixer), BpmReason::PolicyPending);
        assert!(mixer.bpm_lock_enabled);
        mixer.stop_sample(0);
        mixer.service_bpm_lock(11, &mut transport);
        if mixer.voices[0].active {
            assert_eq!(reason(&mixer), BpmReason::PolicyPending);
        }
        mixer.render(&mut [0.0; 1024], &mut [0.0; NUM_SAMPLES]);
        mixer.service_bpm_lock(1035, &mut transport);
        assert!(!mixer.bpm_lock_enabled);
        assert!(
            mixer
                .input_runtime_ownership
                .global_rate_status
                .read_bpm()
                .unwrap()
                .1
                .applied
        );
    }

    #[test]
    fn global_lease_contention_retains_original_target_without_false_effect() {
        let mut mixer = RtMixer::new(1, 48_000.0);
        let mut transport = TransportTimeline::new(48_000);
        let request = target(&mut mixer, 2, true);
        mixer.stage_bpm_lock(request);
        settle(&mut mixer);
        mixer
            .input_runtime_ownership
            .global_timing_publishing
            .store(true, Ordering::Release);
        mixer.service_bpm_lock(23, &mut transport);
        assert!(!mixer.bpm_lock_enabled);
        assert_eq!(mixer.pending_bpm_lock, Some(request));
        mixer
            .input_runtime_ownership
            .global_timing_publishing
            .store(false, Ordering::Release);
        mixer.service_bpm_lock(24, &mut transport);
        assert!(mixer.bpm_lock_enabled);
    }

    #[test]
    fn qualified_bpm_commit_rebinds_only_fresh_births_and_failed_final_claim_keeps_custody() {
        use crate::audio_engine::content_placement::tests::with_before_marker_compare_exchange;
        use crate::audio_engine::input_runtime_binding::capture;
        use crate::audio_engine::ordinary_global_start_tests::phase_group;
        for advanced in [false, true] {
            let (mut fixture, _) = phase_group(false);
            let (mut reference, _) = phase_group(false);
            if advanced {
                assert_eq!(fixture.render(1), reference.render(1));
            }
            let binding = capture(&fixture.control.engine, 0).unwrap().unwrap();
            fixture
                .control
                .engine
                .set_bpm_lock_with_binding(true, Some(&binding))
                .unwrap();
            let crate::messages::ControlMessage::SetBpmLockTarget(request) =
                fixture.control.consumer.pop().unwrap()
            else {
                panic!("actual producer BPM target");
            };
            assert!(request.source_identity.is_some());
            let (id, captured, key) = request.anchor.unwrap();
            assert_eq!(id, 0);
            assert_eq!(captured, binding.binding);
            assert_eq!(
                fixture
                    .control
                    .engine
                    .input_runtime_ownership
                    .callback_key_slot(key),
                Some(0)
            );
            let frame = fixture.transport.output_frame();
            assert_eq!(
                fixture.mixer.bpm_target_ready(request, frame).unwrap().2,
                Some(0)
            );
            fixture.mixer.stage_bpm_lock(request);
            settle(&mut fixture.mixer);
            let before = fixture.transport;
            let source_revision = fixture.mixer.source_control_revision(key).unwrap();
            let copies = [0, 1].map(|id| fixture.voice(id).source_playback);
            let ownership = fixture.control.engine.input_runtime_ownership.clone();
            with_before_marker_compare_exchange(
                move || {
                    ownership
                        .global_timing_revision
                        .fetch_add(1, Ordering::AcqRel);
                },
                || {
                    fixture
                        .mixer
                        .service_bpm_lock(frame, &mut fixture.transport)
                },
            );
            assert_eq!(format!("{:?}", fixture.transport), format!("{before:?}"));
            assert_eq!(fixture.mixer.pending_bpm_lock, Some(request));
            assert_eq!(
                fixture.mixer.global_rate_timing_revision(),
                request.revision + 1
            );
            assert!(
                !fixture
                    .control
                    .engine
                    .input_runtime_ownership
                    .global_rate_status
                    .read_bpm()
                    .unwrap()
                    .1
                    .applied
            );
            assert_eq!(
                fixture.mixer.source_control_revision(key).unwrap(),
                source_revision
            );
            for (id, copy) in copies.iter().enumerate() {
                let voice = fixture.voice(id);
                assert!(voice.source_playback.matches_exact(copy));
                assert!(voice.phase_launch_qualified);
                assert_eq!(voice.phase_birth_unadvanced, !advanced);
            }
            settle(&mut fixture.mixer);
            fixture
                .mixer
                .service_bpm_lock(frame, &mut fixture.transport);
            assert!(fixture.mixer.pending_bpm_lock.is_none());
            assert!(
                fixture
                    .control
                    .engine
                    .input_runtime_ownership
                    .global_rate_status
                    .read_bpm()
                    .unwrap()
                    .1
                    .applied
            );
            assert_ne!(
                fixture.transport.phase_generation(),
                before.phase_generation()
            );
            for (id, copy) in copies.iter().enumerate() {
                assert_eq!(
                    fixture.voice(id).source_playback.position(),
                    copy.position()
                );
                assert_eq!(
                    fixture
                        .voice(id)
                        .source_playback
                        .phase_reference_current(&fixture.transport),
                    !advanced
                );
            }
            assert_eq!(fixture.pcm(513), reference.pcm(513));
        }
    }

    #[test]
    fn unqualified_off_on_keeps_the_complete_existing_nonzero_transport_epoch() {
        for completed_bootstrap in [false, true] {
            let mut mixer = RtMixer::new(1, 48_000.0);
            let mut transport = TransportTimeline::new(48_000);
            mixer.set_bpm_lock(true);
            mixer.set_master_period(0.75);
            assert!(transport.set_master_period(0.75));
            assert!(transport.anchor_beat_position_at_frame(3.25, 17));
            transport.advance_by_rendered_frames(137);
            transport.request_bootstrap(3);
            if completed_bootstrap {
                transport.complete_bootstrap();
            }
            let before = transport;
            let off = target(&mut mixer, 2, false);
            mixer.stage_bpm_lock(off);
            settle(&mut mixer);
            mixer.service_bpm_lock(137, &mut transport);
            assert!(!mixer.bpm_lock_enabled);
            assert_eq!(mixer.master_period_seconds, None);
            assert_eq!(format!("{transport:?}"), format!("{before:?}"));
            // A separate explicit scalar master remains valid for legacy flag-only ON.
            mixer.set_master_period(0.75);
            let on = target(&mut mixer, 3, true);
            mixer.stage_bpm_lock(on);
            settle(&mut mixer);
            mixer.service_bpm_lock(137, &mut transport);
            let (revision, effect) = mixer
                .input_runtime_ownership
                .global_rate_status
                .read_bpm()
                .unwrap();
            assert_eq!(
                (revision, effect.revision, effect.execution_frame),
                (3, 3, Some(137))
            );
            assert!(effect.applied && effect.effective);
            assert_eq!(mixer.master_period_seconds, Some(0.75));
            assert_eq!(format!("{transport:?}"), format!("{before:?}"));
            assert!(transport.matches_clock_epoch(&before));
            assert_eq!(transport.beat_position(), before.beat_position());
        }
    }

    #[test]
    fn explicit_off_clears_an_independent_master_even_when_the_actual_flag_is_already_off() {
        let mut mixer = RtMixer::new(1, 48_000.0);
        let mut transport = TransportTimeline::new(48_000);
        mixer
            .input_runtime_ownership
            .global_rate_status
            .publish_requested(1, Some(1.8), Some(0.625));
        mixer.stage_global_rate(1, Some(1.8), Some(0.625), None, &mut transport);
        assert!(!mixer.bpm_lock_enabled);
        assert_eq!(mixer.master_period_seconds, Some(0.625));
        transport.advance_by_rendered_frames(91);
        let before = transport;
        let old = mixer
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        let off = target(&mut mixer, 2, false);
        mixer.stage_bpm_lock(off);
        mixer.service_bpm_lock(91, &mut transport);
        assert_eq!(reason(&mixer), BpmReason::DrainPending);
        assert_eq!(mixer.master_period_seconds, Some(0.625));
        settle(&mut mixer);
        mixer.service_bpm_lock(91, &mut transport);
        let cleared = mixer
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        let bpm = mixer
            .input_runtime_ownership
            .global_rate_status
            .read_bpm()
            .unwrap()
            .1;
        assert!(bpm.applied && !bpm.effective);
        assert_eq!((bpm.revision, bpm.execution_frame), (2, Some(91)));
        assert_eq!(cleared.effect.revision, old.effect.revision);
        assert_eq!(cleared.effect.requested, old.effect.requested);
        assert_eq!(cleared.effect.effective.speed, 1.8);
        assert_eq!(cleared.effect.effective.period, None);
        assert_eq!(format!("{transport:?}"), format!("{before:?}"));
        mixer
            .input_runtime_ownership
            .global_timing_revision
            .store(3, Ordering::Release);
        mixer
            .input_runtime_ownership
            .global_rate_status
            .publish_requested(3, Some(1.6), None);
        let queued = mixer
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        assert_eq!(queued.requested.period, None);
        mixer.stage_global_rate(3, Some(1.6), None, None, &mut transport);
        assert_eq!(mixer.master_period_seconds, None);
        assert_eq!(mixer.speed, 1.6);
        let applied = mixer
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        assert_eq!((applied.request_revision, applied.effect.revision), (3, 3));
        assert_eq!(applied.effect.effective.period, None);
    }
}

impl RtMixer {
    pub(crate) fn stage_bpm_lock(&mut self, target: BpmLockTarget) {
        if self
            .pending_bpm_lock
            .is_none_or(|old| old.revision <= target.revision)
        {
            self.pending_bpm_lock = Some(target);
            self.publish_bpm_target(target, BpmReason::DrainPending, None);
        }
    }

    fn publish_bpm_target(&self, target: BpmLockTarget, reason: BpmReason, frame: Option<u64>) {
        self.input_runtime_ownership
            .global_rate_status
            .publish_bpm_effect(BpmEffect {
                revision: target.revision,
                requested: target.enabled,
                effective: self.bpm_lock_enabled,
                applied: frame.is_some(),
                reason,
                execution_frame: frame,
            });
    }

    /// Use completed drain fields while owning the global marker itself.
    pub(crate) fn global_drains_claimed(&self, revision: u64) -> bool {
        self.global_commands_drained
            && self.global_parameters_drained
            && self.global_commands_revision == revision
            && self.global_parameters_revision == revision
            && self.global_rate_timing_revision() == revision
    }

    fn bpm_target_ready(
        &self,
        target: BpmLockTarget,
        frame: u64,
    ) -> Result<([Option<f64>; MAX_VOICES], Option<f64>, Option<usize>), BpmReason> {
        if self
            .input_runtime_ownership
            .hierarchy_stamp
            .load(Ordering::Acquire)
            != target.hierarchy_stamp
        {
            return Err(BpmReason::ReferenceChanged);
        }
        let mut anchor_id = None;
        if let Some((original, binding, key)) = target.anchor {
            let Some(id) = self.timing_effect_slot(original, binding.placement) else {
                return Err(BpmReason::ReferenceChanged);
            };
            if self.input_runtime_ownership.callback_key_slot(key) != Some(id)
                || !self.admitted_source_binding_current(id, binding)
            {
                return Err(BpmReason::ReferenceChanged);
            }
            anchor_id = Some(id);
        }
        if !target.enabled && !self.bpm_lock_enabled && self.master_period_seconds.is_none() {
            return Ok(([None; MAX_VOICES], self.master_period_seconds, None));
        }
        // A qualified ON owns its current original reference even when an older
        // blocked OFF left the actual boolean ON. Only an unchanged complete tuple
        // can acknowledge this new receipt without prospective rate preparation.
        let period = if target.enabled {
            match anchor_id {
                Some(id) => Some(
                    self.timing_for_sample_id(id)
                        .period_seconds()
                        .filter(|period| period.is_finite() && *period > 0.0)
                        .ok_or(BpmReason::ReferenceChanged)?
                        / self.speed,
                ),
                None => self.master_period_seconds, // Compatible unqualified legacy call.
            }
        } else {
            None
        };
        if target.enabled == self.bpm_lock_enabled && period == self.master_period_seconds {
            return Ok(([None; MAX_VOICES], period, None));
        }
        if self.has_pending_global_rate() {
            return Err(BpmReason::RatePending);
        }
        if self.has_mapped_voice() {
            return Err(BpmReason::MappedActive);
        }
        if !target.enabled
            && self
                .voices
                .iter()
                .any(|voice| voice.active && voice.sync_request.mode == SourceSyncMode::Fixed)
        {
            return Err(BpmReason::PolicyPending);
        }
        self.prospective_scalar_targets(target.enabled, self.speed, period, frame)
            .map(|targets| (targets, period, anchor_id))
    }

    pub(crate) fn accepted_refresh_master_targets(
        &self,
        period: f64,
        frame: u64,
    ) -> Option<[Option<f64>; MAX_VOICES]> {
        if !self.bpm_lock_enabled || self.has_mapped_voice() || self.has_pending_global_rate() {
            return None;
        }
        self.prospective_scalar_targets(self.bpm_lock_enabled, self.speed, Some(period), frame)
            .ok()
    }

    pub(super) fn prospective_scalar_targets(
        &self,
        enabled: bool,
        speed: f64,
        period: Option<f64>,
        frame: u64,
    ) -> Result<[Option<f64>; MAX_VOICES], BpmReason> {
        let mut targets = [None; MAX_VOICES];
        for (index, destination) in targets.iter_mut().enumerate() {
            let voice = &self.voices[index];
            if !voice.active {
                continue;
            }
            let ratio = voice
                .sync_request
                .scalar_ratio(
                    self.timing_for_voice(voice).period_seconds(),
                    speed,
                    period,
                    enabled,
                )
                .ok_or(BpmReason::HistoryPending)?;
            let (pitch, key_lock) = self.effective_rate_voice_tuple(index);
            if voice.stretch.has_retained_source_input() {
                let snapshot = self
                    .capture_pitch_voice_snapshot(index)
                    .ok_or(BpmReason::HistoryPending)?;
                let readers = snapshot.readers().ok_or(BpmReason::HistoryPending)?;
                let feed = ProductiveSourceFeed {
                    pitch_intent: pitch,
                    key_lock,
                    sample: &snapshot.sample,
                    stems: readers.current.or(snapshot.stems.as_ref()),
                    outgoing_stems: readers.outgoing,
                    sample_rate_hz: snapshot.sample_rate_hz,
                    accepted: snapshot.accepted,
                    playback: &snapshot.playback,
                    plan: snapshot.plan,
                    permit: None,
                    output_frame: Some(frame),
                };
                if !voice.stretch.retained_rate_change_ready(&feed, ratio) {
                    return Err(BpmReason::HistoryPending);
                }
                *destination = Some(ratio);
            } else if voice.stretch.source_lane_busy()
                || (!voice.stretch.owns_pitch_contract(pitch, key_lock)
                    && pitch
                        .native_pitch_scale(ratio, key_lock)
                        .map_or(true, |scale| scale != 1.0))
            {
                // An unsupported current history representation remains productive and pending.
                return Err(BpmReason::HistoryPending);
            }
        }
        Ok(targets)
    }

    pub(crate) fn apply_refresh_master_claimed(
        &mut self,
        period: f64,
        targets: [Option<f64>; MAX_VOICES],
        provenance: MasterProvenance,
    ) {
        for (index, target) in targets.into_iter().enumerate() {
            if let Some(target) = target {
                self.voices[index]
                    .stretch
                    .retarget_retained_rate_claimed(target);
            }
        }
        if self.master_period_seconds != Some(period) {
            self.invalidate_source_for_all();
        }
        self.master_period_seconds = Some(period);
        self.refresh_global_rate_observation_with_master(provenance);
    }

    pub(crate) fn service_bpm_lock(&mut self, frame: u64, transport: &mut TransportTimeline) {
        let Some(target) = self.pending_bpm_lock else {
            return;
        };
        let ownership = self.input_runtime_ownership.clone();
        let revision = self.global_rate_timing_revision();
        if !self.global_rate_drains_settled() {
            self.publish_bpm_target(target, BpmReason::DrainPending, None);
            return;
        }
        let (targets, period, anchor_id) = match self.bpm_target_ready(target, frame) {
            Ok(prepared) => prepared,
            Err(reason) => {
                self.publish_bpm_target(target, reason, None);
                return;
            }
        };
        let mut candidate = *transport;
        if target.enabled
            && anchor_id.is_some()
            && (self.bpm_lock_enabled != target.enabled || self.master_period_seconds != period)
        {
            if let Some(period) = period {
                if !candidate.set_master_period_preserving_beat_position_at_frame(period, frame) {
                    self.publish_bpm_target(target, BpmReason::HistoryPending, None);
                    return;
                }
                if let Some(id) = anchor_id {
                    if let Some(previous) = candidate.bootstrap_reference() {
                        candidate.clear_pending_bootstrap_for_pad(previous);
                    }
                    candidate.request_bootstrap(id);
                    if let Some(beat) = self.active_pad_beat_position(id) {
                        candidate.bootstrap_from_source_period_at_frame(period, beat, frame);
                    }
                }
            }
        }
        let Some(_lease) =
            super::super::GlobalTimingLease::acquire(&ownership.global_timing_publishing)
        else {
            self.publish_bpm_target(target, BpmReason::DrainPending, None);
            return;
        };
        let id = target
            .anchor
            .and_then(|(original, binding, _)| self.timing_effect_slot(original, binding.placement))
            .unwrap_or(0);
        let Some(placement) = ownership.slot_snapshot(id) else {
            self.publish_bpm_target(target, BpmReason::DrainPending, None);
            return;
        };
        let Ok(write) = ownership.prepare_slot_write_fixed(placement, placement.lifetime()) else {
            self.publish_bpm_target(target, BpmReason::DrainPending, None);
            return;
        };
        let mixer = RefCell::new(self);
        let result = ownership.commit_slot_write_fixed(
            &write,
            || {
                let mixer = mixer.borrow();
                mixer.global_drains_claimed(revision)
                    && mixer.bpm_target_ready(target, frame).is_ok()
            },
            |_| {
                let mut mixer = mixer.borrow_mut();
                for (index, ratio) in targets.into_iter().enumerate() {
                    if let Some(ratio) = ratio {
                        mixer.voices[index]
                            .stretch
                            .retarget_retained_rate_claimed(ratio);
                    }
                }
                let changed = mixer.bpm_lock_enabled != target.enabled;
                let tuple_changed = changed || mixer.master_period_seconds != period;
                if tuple_changed {
                    mixer.invalidate_source_for_all();
                }
                mixer.bpm_lock_enabled = target.enabled;
                if tuple_changed {
                    mixer.master_period_seconds = period;
                    mixer.rebind_dry_phase_births(transport, &candidate, frame);
                    *transport = candidate;
                }
                let provenance = if !target.enabled || period.is_none() {
                    MasterProvenance::Absent
                } else {
                    target.anchor.zip(target.source_identity).map_or(
                        MasterProvenance::Unknown,
                        |((_, binding, _), identity)| {
                            MasterProvenance::qualified(identity, binding)
                        },
                    )
                };
                mixer.refresh_global_rate_observation_with_master(provenance);
                mixer.pending_bpm_lock = None;
                mixer.publish_bpm_target(target, BpmReason::None, Some(frame));
            },
        );
        if result.is_err() {
            mixer
                .borrow()
                .publish_bpm_target(target, BpmReason::DrainPending, None);
        }
    }
}
