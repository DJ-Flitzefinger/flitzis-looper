use super::*;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::content_placement::PlacementObservation;
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
use crate::audio_engine::mapped_source_clock::{MappedClockEpoch, MappedLoopGeometry};
use crate::audio_engine::source_beatmap::SourceBeatmap;
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::source_reader::FrameRange;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::SampleBuffer;
use serde_json::json;
use std::sync::atomic::Ordering;

const SOURCE: usize = 3;
const TARGET: usize = 178;
const EMPTY: usize = 84;
const RATE: u32 = 8_000;

fn sample(value: f32) -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from([value; 512]),
    }
}

fn fixture() -> RtMixer {
    let mut mixer = RtMixer::new(1, RATE as f32);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    mixer.set_input_runtime_ownership(ownership.clone());
    for (id, value) in [(SOURCE, 0.25), (TARGET, 0.5)] {
        let source = sample(value);
        ownership.publish_test_source(id, &source, RATE, 1);
        mixer.load_sample(id, source);
        assert!(mixer.play_sample(id, 1.0));
    }
    mixer
}

fn lane(mixer: &RtMixer, id: usize) -> usize {
    mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(id))
        .unwrap()
}

fn extra_source_lane(mixer: &mut RtMixer) -> usize {
    let config = mixer
        .prepare_play_sample_rt(SOURCE, 1.0, 0, &mut ImmediateAudioBufferRetirement)
        .unwrap();
    let index = mixer.voices.iter().position(|voice| !voice.active).unwrap();
    mixer.voices[index].start_rt(config, &mut ImmediateAudioBufferRetirement);
    index
}

fn mapped_voice_fixture(mixer: &mut RtMixer, index: usize) -> MappedLoopGeometry {
    // Numerical fixture only: these tests exercise rejection, never map admission or render.
    let descriptor = json!({
        "schema_version": 1,
        "original": {"sha256": "a".repeat(64), "bytes": 2048},
        "pcm": {
            "format": "f32-le-interleaved-v1",
            "channel_layout": "interleaved-channel-index-order-v1",
            "rate_hz": RATE, "channels": 1, "full_frames": 512, "full_bytes": 2048,
            "interleaved_sha256": "b".repeat(64), "mono_sha256": "c".repeat(64),
            "mono_revision": "hold-gate-numerical-fixture", "source_zero_bits": "0000000000000000",
            "provenance": {"synthetic_geometry_only": true}
        }
    });
    let period = 512.0 / f64::from(RATE);
    let map = SourceBeatmap::manual(descriptor, 0.0, period).unwrap();
    let geometry = MappedLoopGeometry::new(
        map,
        512,
        RATE,
        FrameRange { start: 0, end: 512 },
        1.0,
        false,
    )
    .unwrap();
    let mut timeline = TransportTimeline::new(RATE);
    assert!(timeline.set_master_period(period));
    let epoch = MappedClockEpoch::new(1, timeline, 0.0).unwrap();
    mixer.voices[index].source_playback = SourcePlayback::mapped(&geometry, epoch, 0).unwrap();
    geometry
}

fn admit(mixer: &RtMixer, slot: usize, press: u64) -> Arc<NativeHoldAction> {
    let ownership = mixer.input_runtime_ownership.clone();
    let captured = ownership.slot_snapshot(slot).unwrap();
    let revision = ownership
        .transport_intent_for_cell(captured.cell())
        .load(Ordering::Acquire)
        .checked_add(1)
        .unwrap();
    let action =
        NativeHoldAction::prepare(ownership.clone(), captured, revision, press, 99).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(captured, captured.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |cell| {
                ownership
                    .transport_intent_for_cell(cell)
                    .store(revision, Ordering::Release);
                assert!(action.mark_admitted());
            },
        )
        .unwrap();
    action
}

fn apply_pause(
    mixer: &mut RtMixer,
    action: &Arc<NativeHoldAction>,
    frame: u64,
) -> NativeHoldProjection {
    let ownership = mixer.input_runtime_ownership.clone();
    let captured = match ownership.observe_content_key(action.key()) {
        PlacementObservation::Current(snapshot) => snapshot,
        _ => panic!("test pause requires live content"),
    };
    let write = ownership
        .prepare_slot_write_fixed(captured, captured.lifetime())
        .unwrap();
    let mut projection = None;
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                projection = Some(mixer.apply_pause_hold_rt(
                    HoldActionRtWork::pause(action.clone()),
                    frame,
                    &mut ImmediateAudioBufferRetirement,
                ));
            },
        )
        .unwrap();
    projection.unwrap()
}

fn release(
    mixer: &mut RtMixer,
    action: &Arc<NativeHoldAction>,
    frame: u64,
) -> NativeHoldProjection {
    action.request_release();
    action.mark_release_admitted();
    let ownership = mixer.input_runtime_ownership.clone();
    let captured = match ownership.observe_content_key(action.key()) {
        PlacementObservation::Current(snapshot) => snapshot,
        _ => {
            return mixer.apply_release_hold_rt(
                HoldActionRtWork::release(action.clone()),
                frame,
                &mut ImmediateAudioBufferRetirement,
            );
        }
    };
    let write = ownership
        .prepare_slot_write_fixed(captured, captured.lifetime())
        .unwrap();
    let mut projection = None;
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                projection = Some(mixer.apply_release_hold_rt(
                    HoldActionRtWork::release(action.clone()),
                    frame,
                    &mut ImmediateAudioBufferRetirement,
                ));
            },
        )
        .unwrap();
    projection.unwrap()
}

fn swap(mixer: &mut RtMixer, source: usize, target: usize) {
    let ownership = mixer.input_runtime_ownership.clone();
    let prepared = ownership
        .prepare_placement_pair(
            ownership.slot_snapshot(source).unwrap(),
            ownership.slot_snapshot(target).unwrap(),
        )
        .unwrap();
    ownership
        .commit_placement_pair(
            &prepared,
            true,
            || true,
            || mixer.swap_occupied_pad_images_rt(source, target),
        )
        .unwrap();
}

#[test]
fn actual_effect_ack_and_release_preserve_position_generation_and_other_playback() {
    let mut mixer = fixture();
    let held_lane = lane(&mixer, SOURCE);
    let other_lane = lane(&mixer, TARGET);
    let generation = mixer.voices[held_lane].generation;
    let position = mixer.voices[held_lane].source_playback.position();
    let history = mixer.voices[held_lane].history_lifetime.clone();
    let action = admit(&mixer, SOURCE, 11);
    assert!(!mixer.voices[held_lane].paused);
    assert!(action.effect().is_none());
    let paused = apply_pause(&mut mixer, &action, 120);
    assert_eq!(paused.state, HoldActionState::PausedOwned);
    assert!(paused.effect_current);
    let effect = paused.effect.unwrap();
    assert_eq!(effect.cohort_mask, 1 << held_lane);
    assert_eq!(effect.changed_mask, 1 << held_lane);
    assert_eq!(effect.generations[held_lane], generation);
    assert_eq!(effect.paused_at_output_frame, 120);
    assert!(mixer.voices[held_lane].paused);
    assert!(!mixer.voices[other_lane].paused);
    assert_eq!(
        release(&mut mixer, &action, 150).state,
        HoldActionState::Resumed
    );
    assert!(!mixer.voices[held_lane].paused);
    assert!(!mixer.voices[other_lane].paused);
    assert_eq!(mixer.voices[held_lane].generation, generation);
    assert_eq!(mixer.voices[held_lane].source_playback.position(), position);
    assert!(Arc::ptr_eq(
        &mixer.voices[held_lane].history_lifetime,
        &history
    ));
    assert!(mixer.capture_hold_projection_rt(action.key()).is_none());
    let settled = action.projection();
    release(&mut mixer, &action, 151);
    assert_eq!(action.projection(), settled);
}

#[test]
fn paused_or_stopped_content_has_no_owned_effect_and_release_never_starts_it() {
    for stopped in [false, true] {
        let mut mixer = fixture();
        let held_lane = lane(&mixer, SOURCE);
        if stopped {
            mixer.stop_sample(SOURCE);
        } else {
            mixer.pause_sample(SOURCE);
        }
        let action = admit(&mixer, SOURCE, 12);
        let paused = apply_pause(&mut mixer, &action, 10);
        assert_eq!(paused.state, HoldActionState::NoEffect);
        assert!(paused.effect.is_none());
        assert_eq!(
            release(&mut mixer, &action, 11).state,
            HoldActionState::NoEffect
        );
        assert_eq!(mixer.voices[held_lane].active, !stopped);
        assert_eq!(mixer.voices[held_lane].paused, !stopped);
    }
}

#[test]
fn release_before_ack_and_duplicate_native_pause_keep_one_effect_and_one_resume() {
    let mut mixer = fixture();
    let action = admit(&mixer, SOURCE, 13);
    assert!(action.request_release());
    assert!(action.mark_release_admitted());
    let first = apply_pause(&mut mixer, &action, 0);
    assert!(first.release_requested);
    assert!(first.effect_current);
    let repeated = apply_pause(&mut mixer, &action, 1);
    assert_eq!(repeated, first);
    assert_eq!(
        release(&mut mixer, &action, 2).state,
        HoldActionState::Resumed
    );
    let first_final = action.projection();
    release(&mut mixer, &action, 3);
    assert_eq!(action.projection(), first_final);
}

#[test]
fn native_swap_in_both_release_orderings_changes_only_the_owned_survivor() {
    for release_first in [false, true] {
        let mut mixer = fixture();
        let held_lane = lane(&mixer, SOURCE);
        let other_lane = lane(&mixer, TARGET);
        mixer.pause_sample(TARGET);
        let action = admit(&mixer, SOURCE, 14);
        apply_pause(&mut mixer, &action, 20);
        let effect = action.effect();
        if release_first {
            release(&mut mixer, &action, 21);
        }
        swap(&mut mixer, SOURCE, TARGET);
        assert_eq!(mixer.voices[held_lane].sample_id, TARGET);
        assert_eq!(mixer.voices[other_lane].sample_id, SOURCE);
        assert!(mixer.voices[other_lane].paused);
        if !release_first {
            let projected = mixer.capture_hold_projection_rt(action.key()).unwrap();
            assert_eq!(projected.effect, effect);
            assert!(projected.effect_current);
            release(&mut mixer, &action, 22);
        }
        assert_eq!(action.state(), HoldActionState::Resumed);
        assert!(!mixer.voices[held_lane].paused);
        assert!(mixer.voices[other_lane].paused);
    }
}

#[test]
fn admitted_pause_itself_follows_move_before_native_pause_application() {
    let mut mixer = fixture();
    let action = admit(&mixer, SOURCE, 15);
    let held_lane = lane(&mixer, SOURCE);
    swap(&mut mixer, SOURCE, EMPTY);
    assert_eq!(mixer.voices[held_lane].sample_id, EMPTY);
    apply_pause(&mut mixer, &action, 30);
    assert!(mixer.voices[held_lane].paused);
    let replacement = sample(0.75);
    mixer
        .input_runtime_ownership
        .publish_test_source(SOURCE, &replacement, RATE, 2);
    mixer.load_sample(SOURCE, replacement);
    assert!(mixer.play_sample(SOURCE, 1.0));
    let replacement_lane = lane(&mixer, SOURCE);
    release(&mut mixer, &action, 31);
    assert!(!mixer.voices[held_lane].paused);
    assert!(!mixer.voices[replacement_lane].paused);
    assert_ne!(held_lane, replacement_lane);
}

#[test]
fn later_accepted_intent_revokes_release_before_its_callback_changes_audio() {
    let mut mixer = fixture();
    let held_lane = lane(&mixer, SOURCE);
    let action = admit(&mixer, SOURCE, 16);
    apply_pause(&mut mixer, &action, 40);
    let current = mixer
        .input_runtime_ownership
        .transport_intent_for_cell(action.key().cell());
    current.store(action.transport_revision() + 1, Ordering::Release);
    assert!(
        !mixer
            .capture_hold_projection_rt(action.key())
            .unwrap()
            .effect_current
    );
    assert_eq!(
        release(&mut mixer, &action, 41).state,
        HoldActionState::Superseded
    );
    assert!(mixer.voices[held_lane].paused);
    assert!(mixer.capture_hold_projection_rt(action.key()).is_none());
}

#[test]
fn cohort_replacement_and_extra_voice_cannot_receive_an_old_release() {
    for extra_voice in [false, true] {
        let mut mixer = fixture();
        let action = admit(&mixer, SOURCE, 17);
        apply_pause(&mut mixer, &action, 50);
        if extra_voice {
            let config = mixer
                .prepare_play_sample_rt(SOURCE, 1.0, 50, &mut ImmediateAudioBufferRetirement)
                .unwrap();
            let extra = mixer.voices.iter_mut().find(|voice| !voice.active).unwrap();
            extra.start_rt(config, &mut ImmediateAudioBufferRetirement);
            extra.pause();
        } else {
            assert!(mixer.play_sample(SOURCE, 1.0));
            mixer.pause_sample(SOURCE);
        }
        assert!(
            !mixer
                .capture_hold_projection_rt(action.key())
                .unwrap()
                .effect_current
        );
        assert_eq!(
            release(&mut mixer, &action, 51).state,
            HoldActionState::Superseded
        );
        assert!(
            mixer
                .voices
                .iter()
                .filter(|voice| voice.is_playing_sample(SOURCE))
                .all(|voice| voice.paused)
        );
    }
}

#[test]
fn effect_masks_only_the_lanes_actually_changed_by_the_hold() {
    let mut mixer = fixture();
    let first = lane(&mixer, SOURCE);
    let config = mixer
        .prepare_play_sample_rt(SOURCE, 1.0, 60, &mut ImmediateAudioBufferRetirement)
        .unwrap();
    let second = mixer.voices.iter().position(|voice| !voice.active).unwrap();
    mixer.voices[second].start_rt(config, &mut ImmediateAudioBufferRetirement);
    mixer.voices[second].pause();
    let action = admit(&mixer, SOURCE, 18);
    let effect = apply_pause(&mut mixer, &action, 60).effect.unwrap();
    assert_eq!(effect.cohort_mask, (1 << first) | (1 << second));
    assert_eq!(effect.changed_mask, 1 << first);
    release(&mut mixer, &action, 61);
    assert!(!mixer.voices[first].paused);
    assert!(mixer.voices[second].paused);
}

#[test]
fn exact_retirement_fences_equal_byte_reassignment_and_delayed_native_work() {
    let mut mixer = fixture();
    let action = admit(&mixer, SOURCE, 19);
    apply_pause(&mut mixer, &action, 70);
    let removed = mixer
        .retire_content_actions_rt(
            action.key(),
            HoldActionState::Retired,
            71,
            &mut ImmediateAudioBufferRetirement,
        )
        .unwrap();
    assert_eq!(removed.state, HoldActionState::Retired);
    mixer.stop_sample(SOURCE);
    let replacement = sample(0.25);
    mixer.input_runtime_ownership.publish_content_lifetime(
        SOURCE,
        crate::audio_engine::content_lifetime::reserve().unwrap(),
    );
    mixer
        .input_runtime_ownership
        .publish_test_source(SOURCE, &replacement, RATE, 2);
    mixer.load_sample(SOURCE, replacement);
    assert!(mixer.play_sample(SOURCE, 1.0));
    let new_lane = lane(&mixer, SOURCE);
    let new_generation = mixer.voices[new_lane].generation;
    mixer.apply_pause_hold_rt(
        HoldActionRtWork::pause(action.clone()),
        72,
        &mut ImmediateAudioBufferRetirement,
    );
    release(&mut mixer, &action, 73);
    assert_eq!(action.state(), HoldActionState::Retired);
    assert!(!mixer.voices[new_lane].paused);
    assert_eq!(mixer.voices[new_lane].generation, new_generation);
}

#[test]
fn exhausted_voice_generation_creates_no_effect_and_does_not_pause_audio() {
    let mut mixer = fixture();
    let held_lane = lane(&mixer, SOURCE);
    mixer.voices[held_lane].generation = u64::MAX;
    let action = admit(&mixer, SOURCE, 20);
    assert_eq!(
        apply_pause(&mut mixer, &action, 80).state,
        HoldActionState::Exhausted
    );
    assert!(action.effect().is_none());
    assert!(!mixer.voices[held_lane].paused);
    assert_eq!(
        release(&mut mixer, &action, 81).state,
        HoldActionState::Exhausted
    );
}

#[test]
fn stale_source_metadata_does_not_revoke_an_exact_surviving_cohort() {
    let mut mixer = fixture();
    let action = admit(&mixer, SOURCE, 21);
    let held_lane = lane(&mixer, SOURCE);
    apply_pause(&mut mixer, &action, 90);
    let source = mixer.sample_bank[SOURCE].as_ref().unwrap().clone();
    mixer
        .input_runtime_ownership
        .publish_test_source(SOURCE, &source, RATE, 7);
    assert!(
        mixer
            .capture_hold_projection_rt(action.key())
            .unwrap()
            .effect_current
    );
    assert_eq!(
        release(&mut mixer, &action, 91).state,
        HoldActionState::Resumed
    );
    assert!(!mixer.voices[held_lane].paused);
}

#[test]
fn retained_old_lifetime_voice_is_not_current_content_pause_authority() {
    let mut mixer = fixture();
    let retained_lane = lane(&mixer, SOURCE);
    let old_key = mixer.voices[retained_lane]
        .source_admission
        .as_ref()
        .unwrap()
        .content_key()
        .unwrap();
    let replacement = sample(0.75);
    mixer
        .input_runtime_ownership
        .publish_test_source(SOURCE, &replacement, RATE, 2);
    mixer.load_sample(SOURCE, replacement);
    // Current bank is fresh content; intentionally retained old PCM keeps playing its own key.
    assert!(mixer.voices[retained_lane].active);
    let current = admit(&mixer, SOURCE, 22);
    assert_ne!(old_key, current.key());
    assert_eq!(
        apply_pause(&mut mixer, &current, 100).state,
        HoldActionState::NoEffect
    );
    assert!(!mixer.voices[retained_lane].paused);
    assert_eq!(
        release(&mut mixer, &current, 101).state,
        HoldActionState::NoEffect
    );
    assert!(mixer.voices[retained_lane].active);
}

#[test]
fn stale_removed_key_cannot_retire_or_resume_a_new_hold_in_the_same_cell() {
    let mut mixer = fixture();
    let old = admit(&mixer, SOURCE, 23);
    apply_pause(&mut mixer, &old, 110);
    mixer.retire_content_actions_rt(
        old.key(),
        HoldActionState::Retired,
        111,
        &mut ImmediateAudioBufferRetirement,
    );
    mixer.stop_sample(SOURCE);
    let replacement = sample(0.75);
    mixer
        .input_runtime_ownership
        .publish_test_source(SOURCE, &replacement, RATE, 2);
    mixer.load_sample(SOURCE, replacement);
    assert!(mixer.play_sample(SOURCE, 1.0));
    let current = admit(&mixer, SOURCE, 24);
    apply_pause(&mut mixer, &current, 112);
    let expected = mixer.capture_hold_projection_rt(current.key()).unwrap();
    assert!(
        mixer
            .retire_content_actions_rt(
                old.key(),
                HoldActionState::Retired,
                113,
                &mut ImmediateAudioBufferRetirement,
            )
            .is_none()
    );
    release(&mut mixer, &old, 114);
    assert_eq!(
        mixer.capture_hold_projection_rt(current.key()),
        Some(expected)
    );
    assert!(mixer.voices[lane(&mixer, SOURCE)].paused);
    assert_eq!(
        release(&mut mixer, &current, 115).state,
        HoldActionState::Resumed
    );
}

#[derive(Default)]
struct HoldRetirement {
    // Keep each original box so this oracle proves transfer, including the callback's last Arc.
    #[allow(clippy::vec_box)]
    held: Vec<Box<HoldActionRtWork>>,
}

impl AudioBufferRetirement for HoldRetirement {
    fn retire_hold_action(&mut self, work: Box<HoldActionRtWork>) {
        self.held.push(work);
    }

    fn retire_resident_capture(
        &mut self,
        _: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: Arc<std::sync::atomic::AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
    fn retire_sample(&mut self, _: SampleBuffer) {}
    fn retire_prepared_stems(&mut self, _: crate::messages::PreparedStemSet) {}
    fn retire_constant_timing(
        &mut self,
        _: crate::audio_engine::constant_timing::PreparedConstantTiming,
    ) {
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }

    fn available_retirement_slots(&mut self) -> usize {
        2
    }
}

#[test]
fn mapped_hold_pause_rejects_complete_cohort_and_retires_exact_incoming_work() {
    let mut mixer = fixture();
    let mapped = lane(&mixer, SOURCE);
    let scalar = extra_source_lane(&mut mixer);
    let _geometry = mapped_voice_fixture(&mut mixer, mapped);
    let before = [
        mixer.voices[mapped].source_playback,
        mixer.voices[scalar].source_playback,
    ];
    let action = admit(&mixer, SOURCE, 26);
    let work = HoldActionRtWork::pause(action.clone());
    let address = std::ptr::from_ref(work.as_ref());
    let mut retirement = HoldRetirement::default();
    let rejected = mixer.apply_pause_hold_rt(work, 130, &mut retirement);
    assert_eq!(rejected.state, HoldActionState::Rejected);
    assert!(rejected.effect.is_none());
    assert_eq!(retirement.held.len(), 1);
    assert_eq!(std::ptr::from_ref(retirement.held[0].as_ref()), address);
    assert!(mixer.capture_hold_projection_rt(action.key()).is_none());
    for (index, playback) in [mapped, scalar].into_iter().zip(before) {
        assert!(!mixer.voices[index].paused);
        assert!(mixer.voices[index].source_playback.matches_exact(&playback));
    }
    assert!(!mixer.voices[lane(&mixer, TARGET)].paused);
}

#[test]
fn mapped_transport_refuses_before_revoking_hold_or_mutating_any_cohort_lane() {
    use crate::messages::TransportContentEffect;
    for effect in [
        TransportContentEffect::Pause,
        TransportContentEffect::Resume,
        TransportContentEffect::Seek {
            position_s: 0.02,
            expected_source_generation: None,
        },
    ] {
        let mut mixer = fixture();
        let mapped = lane(&mixer, SOURCE);
        let scalar = extra_source_lane(&mut mixer);
        let action = admit(&mixer, SOURCE, 27);
        assert_eq!(
            apply_pause(&mut mixer, &action, 140).state,
            HoldActionState::PausedOwned
        );
        let _geometry = mapped_voice_fixture(&mut mixer, mapped);
        let before = [
            mixer.voices[mapped].source_playback,
            mixer.voices[scalar].source_playback,
        ];
        let mut retirement = HoldRetirement::default();
        mixer.apply_transport_content_rt(action.key(), effect, 141, &mut retirement);
        assert_eq!(action.state(), HoldActionState::PausedOwned);
        assert!(Arc::ptr_eq(
            &mixer.hold_actions[action.key().cell().index()]
                .as_ref()
                .unwrap()
                .action,
            &action,
        ));
        assert!(retirement.held.is_empty());
        for (index, playback) in [mapped, scalar].into_iter().zip(before) {
            assert!(mixer.voices[index].paused);
            assert!(mixer.voices[index].source_playback.matches_exact(&playback));
        }
    }
}

#[test]
fn mapped_pause_rejection_preserves_previous_exact_hold_owner() {
    let mut mixer = fixture();
    let mapped = lane(&mixer, SOURCE);
    let previous = admit(&mixer, SOURCE, 28);
    assert_eq!(
        apply_pause(&mut mixer, &previous, 150).state,
        HoldActionState::PausedOwned
    );
    let _geometry = mapped_voice_fixture(&mut mixer, mapped);
    let next = admit(&mixer, SOURCE, 29);
    let mut retirement = HoldRetirement::default();
    let rejected =
        mixer.apply_pause_hold_rt(HoldActionRtWork::pause(next.clone()), 151, &mut retirement);
    assert_eq!(rejected.state, HoldActionState::Rejected);
    assert_eq!(previous.state(), HoldActionState::PausedOwned);
    assert!(Arc::ptr_eq(
        &mixer.hold_actions[previous.key().cell().index()]
            .as_ref()
            .unwrap()
            .action,
        &previous,
    ));
    assert!(mixer.voices[mapped].paused);
    assert_eq!(retirement.held.len(), 1);
    assert!(Arc::ptr_eq(&retirement.held[0].action, &next));
}

#[test]
fn mapped_release_rejects_without_resume_and_retires_both_exact_owners() {
    let mut mixer = fixture();
    let mapped = lane(&mixer, SOURCE);
    let scalar = extra_source_lane(&mut mixer);
    let action = admit(&mixer, SOURCE, 30);
    let pause = HoldActionRtWork::pause(action.clone());
    let pause_address = std::ptr::from_ref(pause.as_ref());
    let mut retirement = HoldRetirement::default();
    assert_eq!(
        mixer.apply_pause_hold_rt(pause, 160, &mut retirement).state,
        HoldActionState::PausedOwned,
    );
    let _geometry = mapped_voice_fixture(&mut mixer, mapped);
    assert!(action.request_release());
    assert!(action.mark_release_admitted());
    let release = HoldActionRtWork::release(action.clone());
    let release_address = std::ptr::from_ref(release.as_ref());
    let rejected = mixer.apply_release_hold_rt(release, 161, &mut retirement);
    assert_eq!(rejected.state, HoldActionState::Rejected);
    assert!(mixer.voices[mapped].paused);
    assert!(mixer.voices[scalar].paused);
    assert!(!mixer.voices[lane(&mixer, TARGET)].paused);
    assert!(mixer.capture_hold_projection_rt(action.key()).is_none());
    assert_eq!(retirement.held.len(), 2);
    assert_eq!(
        std::ptr::from_ref(retirement.held[0].as_ref()),
        pause_address
    );
    assert_eq!(
        std::ptr::from_ref(retirement.held[1].as_ref()),
        release_address
    );
}

#[test]
fn release_transfers_both_prebuilt_owners_before_the_last_action_pin_ends() {
    let mut mixer = fixture();
    let action = admit(&mixer, SOURCE, 25);
    let weak = Arc::downgrade(&action);
    let mut retirement = HoldRetirement::default();
    let pause = HoldActionRtWork::pause(action.clone());
    let pause_address = std::ptr::from_ref(pause.as_ref());
    mixer.apply_pause_hold_rt(pause, 120, &mut retirement);
    assert!(retirement.held.is_empty());
    assert!(action.request_release());
    assert!(action.mark_release_admitted());
    let release = HoldActionRtWork::release(action.clone());
    let release_address = std::ptr::from_ref(release.as_ref());
    mixer.apply_release_hold_rt(release, 121, &mut retirement);
    assert_eq!(retirement.held.len(), 2);
    assert_eq!(
        std::ptr::from_ref(retirement.held[0].as_ref()),
        pause_address
    );
    assert_eq!(
        std::ptr::from_ref(retirement.held[1].as_ref()),
        release_address
    );
    assert!(mixer.capture_hold_projection_rt(action.key()).is_none());
    drop(action);
    assert!(weak.upgrade().is_some());
    retirement.held.clear();
    assert!(weak.upgrade().is_none());
}
