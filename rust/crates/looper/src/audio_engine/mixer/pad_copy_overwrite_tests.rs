use super::tests::{HoldingRetirement, RATE, SOURCE, TARGET, fixture};
use super::*;
use crate::audio_engine::hold_action::{HoldActionRtWork, HoldActionState, NativeHoldAction};
use crate::audio_engine::input_runtime_binding::InputPadBinding;
use crate::messages::{ControlMessage, TransportContentEffect};

fn occupied_fixture() -> (Box<RtMixer>, Box<PadCopyRtWork>, HoldingRetirement) {
    let (mut mixer, mut work, mut retirement) = fixture();
    let ownership = mixer.input_runtime_ownership.clone();
    ownership.release_copy(TARGET, 17).unwrap();
    let old = SampleBuffer {
        channels: 1,
        samples: Arc::from([0.75; 256]),
        residency: None,
    }
    .with_complete_source(RATE);
    ownership.publish_test_source(TARGET, &old, RATE, 1);
    mixer.load_sample(TARGET, old.clone());
    let before = ownership.slot_snapshot(TARGET).unwrap();
    let binding = InputPadBinding {
        placement: Some(before),
        resident: old.resident_binding(),
        source_address: old.source_address(),
        sample_count: old.source_sample_count(),
        channels: old.channels,
        sample_rate_hz: RATE,
        authority_revision: ownership.authority_revision(TARGET),
        runtime_revision: ownership.runtime_revision(TARGET),
        accepted: None,
    };
    work.guard.target_placement = before;
    work.guard.target_binding = Some(binding);
    work.guard.target_source_generation = 1;
    work.guard.target_authority = binding.authority_revision;
    work.authority_revision = binding.authority_revision + 1;
    work.content_key = before
        .for_lifetime(work.content_lifetime)
        .unwrap()
        .content_key()
        .unwrap();
    ownership.reserve_copy_target(before, 17).unwrap();
    retirement.slots = work.retirement_slots_needed();
    (mixer, work, retirement)
}

#[test]
fn overwrite_stops_only_old_target_and_retires_its_exact_hold_in_the_birth_claim() {
    let (mut mixer, work, mut retirement) = occupied_fixture();
    assert!(mixer.play_sample(SOURCE, 1.0));
    assert!(mixer.play_sample(TARGET, 1.0));
    let source_lane = mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(SOURCE))
        .unwrap();
    let source_generation = mixer.voices[source_lane].generation;
    let source_position = mixer.voices[source_lane].source_playback.position();
    let old_key = work.guard.target_placement.content_key().unwrap();
    let ownership = mixer.input_runtime_ownership.clone();
    let action =
        NativeHoldAction::prepare(ownership.clone(), work.guard.target_placement, 1, 911, 55)
            .unwrap();
    let write = ownership
        .prepare_slot_write_fixed(work.guard.target_placement, old_key.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |cell| {
                ownership
                    .transport_intent_for_cell(cell)
                    .store(1, Ordering::Release);
                assert!(action.mark_admitted());
                mixer.apply_pause_hold_rt(
                    HoldActionRtWork::pause(action.clone()),
                    42,
                    &mut retirement,
                );
            },
        )
        .unwrap();
    assert_eq!(action.state(), HoldActionState::PausedOwned);
    let fresh_key = work.content_key;
    assert!(mixer.copy_pad_at_output_frame_rt(work, 99, &mut retirement));
    assert_eq!(action.state(), HoldActionState::Retired);
    assert_eq!(action.projection().settled_at_output_frame, 99);
    assert_eq!(retirement.holds.len(), 1);
    assert_eq!(
        retirement.samples.len(),
        1,
        "old paused voice reader must retire off RT"
    );
    assert!(retirement.held.as_ref().unwrap().retired_sample.is_some());
    assert_eq!(mixer.voices[source_lane].generation, source_generation);
    assert_eq!(
        mixer.voices[source_lane].source_playback.position(),
        source_position
    );
    assert!(mixer.voices[source_lane].active && !mixer.voices[source_lane].paused);
    assert!(
        !mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(TARGET))
    );
    assert_eq!(
        ownership.slot_snapshot(TARGET).unwrap().content_key(),
        Some(fresh_key)
    );
    assert_ne!(old_key, fresh_key);
    mixer.apply_transport_content_rt(old_key, TransportContentEffect::Pause, 100, &mut retirement);
    action.request_release();
    let released = mixer.apply_release_hold_rt(
        HoldActionRtWork::release(action.clone()),
        101,
        &mut retirement,
    );
    assert_eq!(released.state, HoldActionState::Retired);
    assert!(
        !mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(TARGET))
    );
    assert!(mixer.voices[source_lane].active && !mixer.voices[source_lane].paused);
}

#[test]
fn overwrite_capacity_and_pending_retain_original_participants_and_exact_payload() {
    let (mut mixer, work, mut retirement) = occupied_fixture();
    assert!(mixer.play_sample(SOURCE, 1.0));
    assert!(mixer.play_sample(TARGET, 1.0));
    let ownership = mixer.input_runtime_ownership.clone();
    let before = work.guard.target_placement;
    let phase = work.guard.phase.clone();
    let original_sample = mixer.sample_bank[TARGET].as_ref().unwrap().clone();
    retirement.slots = work.retirement_slots_needed() - 1;
    assert!(!mixer.copy_pad_rt(work, &mut retirement));
    assert_eq!(phase.load(Ordering::Acquire), 0);
    assert!(retirement.samples.is_empty() && retirement.held.is_none());
    assert_eq!(
        ownership.slot_snapshot(TARGET).unwrap().content_key(),
        before.content_key()
    );
    assert!(
        mixer.sample_bank[TARGET]
            .as_ref()
            .unwrap()
            .same_window(&original_sample)
    );
    let Some(ControlMessage::PadCopy(work)) = mixer.take_deferred_control() else {
        panic!("missing exact deferred Copy")
    };
    assert!(Arc::ptr_eq(&phase, &work.guard.phase));
    retirement.slots = work.retirement_slots_needed();
    let write = ownership
        .prepare_slot_write_fixed(before, before.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                assert!(!mixer.copy_pad_rt(work, &mut retirement));
            },
        )
        .unwrap();
    assert_eq!(phase.load(Ordering::Acquire), 0);
    assert!(retirement.samples.is_empty());
    let Some(ControlMessage::PadCopy(work)) = mixer.take_deferred_control() else {
        panic!("Pending lost original Copy")
    };
    assert!(Arc::ptr_eq(&phase, &work.guard.phase));
    assert!(mixer.copy_pad_rt(work, &mut retirement));
    assert_eq!(phase.load(Ordering::Acquire), 2);
}

#[test]
fn overwrite_equal_pcm_new_lifetime_and_cancel_reject_before_any_target_effect() {
    for cancelled in [false, true] {
        let (mut mixer, work, mut retirement) = occupied_fixture();
        assert!(mixer.play_sample(SOURCE, 1.0));
        assert!(mixer.play_sample(TARGET, 1.0));
        let ownership = mixer.input_runtime_ownership.clone();
        let phase = work.guard.phase.clone();
        if cancelled {
            work.guard.reject();
        } else {
            let sample = mixer.sample_bank[TARGET].as_ref().unwrap().clone();
            let fresh_lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
            let write = ownership
                .prepare_slot_write_fixed(work.guard.target_placement, fresh_lifetime)
                .unwrap();
            ownership
                .commit_slot_write_fixed(
                    &write,
                    || true,
                    |cell| {
                        ownership.apply_source_for_cell(
                            cell,
                            &sample,
                            RATE,
                            9,
                            work.guard.target_authority + 1,
                            TimingIntent::Legacy,
                        );
                    },
                )
                .unwrap();
        }
        let before = ownership.slot_snapshot(TARGET).unwrap();
        let voice_state: Vec<_> = mixer
            .voices
            .iter()
            .map(|voice| (voice.active, voice.paused, voice.generation))
            .collect();
        assert!(!mixer.copy_pad_rt(work, &mut retirement));
        assert_eq!(phase.load(Ordering::Acquire), 3);
        assert!(
            mixer.deferred_control().is_none(),
            "genuine staleness must terminally reject"
        );
        assert_eq!(
            ownership.slot_snapshot(TARGET).unwrap().content_key(),
            before.content_key()
        );
        assert_eq!(
            mixer
                .voices
                .iter()
                .map(|voice| (voice.active, voice.paused, voice.generation))
                .collect::<Vec<_>>(),
            voice_state
        );
        assert!(retirement.samples.is_empty() && retirement.holds.is_empty());
        assert!(retirement.held.as_ref().unwrap().retired_sample.is_none());
    }
}
