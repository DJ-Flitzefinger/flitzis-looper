use super::*;
use crate::audio_engine::input_mapping::InputRuntime;

fn start(fixture: &mut Fixture, id: usize) -> crate::audio_engine::CapturedPadStart {
    let start = fixture
        .engine
        .capture_pad_start(id, 0.125, Some(3.0), 1.0, false, None, None)
        .unwrap();
    fixture.drain(); // Capture the original target before publishing its ordinary work.
    assert!(fixture.engine.publish_pad_start(&start, None).unwrap());
    fixture.drain();
    assert!(start.effective_frame().is_some(), "{:?}", start.reason());
    start
}

fn runtime(fixture: &Fixture) -> InputRuntime {
    InputRuntime::new_with_ownership(
        fixture.producer.clone(),
        crate::audio_engine::timing::InputClock::new(),
        fixture.engine.input_runtime_ownership.clone(),
    )
}

fn image(
    fixture: &Fixture,
    runtime: &InputRuntime,
    multi_loop: bool,
) -> Result<Option<GlobalPlaybackBatchTicket>, String> {
    let bindings: Vec<_> = (0..NUM_SAMPLES)
        .map(|id| input_runtime_binding::capture(&fixture.engine, id).unwrap())
        .collect();
    runtime.set_runtime_state_with_pitch_projection(
        multi_loop,
        bindings.iter().map(Option::is_some).collect(),
        vec![0.125; NUM_SAMPLES],
        vec![Some(3.0); NUM_SAMPLES],
        bindings.iter().map(Option::as_ref).collect(),
        None,
    )
}

fn active(fixture: &Fixture) -> Vec<usize> {
    fixture
        .mixer
        .voices
        .iter()
        .filter(|voice| voice.active)
        .map(|voice| voice.sample_id)
        .collect()
}

#[test]
fn last_effective_explicit_choice_orders_cross_content_and_survives_stop() {
    let mut fixture = Fixture::new();
    fixture.load(2);
    let a = start(&mut fixture, 0);
    let old_target = fixture
        .engine
        .current_explicit_playback_target()
        .unwrap()
        .unwrap();
    let _b = start(&mut fixture, 1);
    let c = start(&mut fixture, 2);
    assert!(
        fixture
            .engine
            ._capture_pad_start_guarded(
                0,
                0.125,
                Some(3.0),
                1.0,
                true,
                None,
                None,
                Some(&old_target)
            )
            .is_err()
    );
    let target = fixture
        .engine
        .current_explicit_playback_target()
        .unwrap()
        .unwrap();
    assert_eq!(target.current_control_slot().unwrap(), Some(2));
    assert_eq!(target.original_sequence(), c.sequence());
    let ownership = &fixture.engine.input_runtime_ownership;
    ownership.record_explicit_playback(a.content_key(), target.effective_frame(), a.sequence());
    assert_eq!(
        fixture
            .engine
            .current_explicit_playback_target()
            .unwrap()
            .unwrap()
            .current_control_slot()
            .unwrap(),
        Some(2)
    );
    let stop = fixture.engine.stop_explicit_playback().unwrap();
    fixture.drain();
    assert_eq!(stop.publication_status(), "accepted");
    assert_eq!(active(&fixture), vec![0, 1]);
    assert_eq!(target.current_control_slot().unwrap(), Some(2));
    let restarted = fixture
        .engine
        ._capture_pad_start_guarded(2, 0.125, Some(3.0), 1.0, true, None, None, Some(&target))
        .unwrap();
    fixture.drain();
    assert!(fixture.engine.publish_pad_start(&restarted, None).unwrap());
    fixture.drain();
    assert_eq!(active(&fixture), vec![2]);
}

#[test]
fn singleton_stop_fences_pending_launches_without_stopping_active_peers_or_their_pause() {
    let mut fixture = Fixture::new();
    fixture.load(2);
    start(&mut fixture, 0);
    start(&mut fixture, 2);
    let pause = fixture.engine.pause_sample(0).unwrap();
    fixture.drain();
    let pending = fixture
        .engine
        .capture_pad_start(1, 0.125, Some(3.0), 1.0, false, None, None)
        .unwrap();
    fixture.drain();
    let stop = fixture.engine.stop_explicit_playback().unwrap();
    assert!(
        !fixture
            .engine
            .publish_pad_start(&pending, None)
            .unwrap_or(false)
    );
    fixture.drain();
    assert_eq!(stop.publication_status(), "accepted");
    assert_eq!(active(&fixture), vec![0]);
    assert!(
        pause
            .current_observation()
            .unwrap()
            .unwrap()
            .ordinary_paused()
    );
}

#[test]
fn absent_history_stop_has_zero_audible_members_and_group_resume_origins_are_distinct() {
    let mut fixture = Fixture::new();
    let group = fixture.enqueue(&[0, 1], true);
    fixture.drain();
    assert_eq!(group.publication_status(), "accepted");
    assert!(
        fixture
            .engine
            .current_explicit_playback_target()
            .unwrap()
            .is_none()
    );
    let pending = fixture
        .engine
        .capture_pad_start(1, 0.125, Some(3.0), 1.0, false, None, None)
        .unwrap();
    fixture.drain();
    let stop = fixture.engine.stop_explicit_playback().unwrap();
    fixture.drain();
    assert_eq!(stop.publication_status(), "accepted");
    assert_eq!(active(&fixture), vec![0, 1]);
    assert!(
        !fixture
            .engine
            .publish_pad_start(&pending, None)
            .unwrap_or(false)
    );
    fixture.engine.resume_sample(0).unwrap();
    fixture.drain();
    assert!(
        fixture
            .engine
            .current_explicit_playback_target()
            .unwrap()
            .is_none(),
        "NO_EFFECT is not an explicit choice"
    );
    fixture.engine.pause_sample(1).unwrap();
    fixture.drain();
    fixture.engine.resume_sample(1).unwrap();
    fixture.drain();
    assert_eq!(
        fixture
            .engine
            .current_explicit_playback_target()
            .unwrap()
            .unwrap()
            .current_control_slot()
            .unwrap(),
        Some(1)
    );
}

#[test]
fn three_pad_mode_collapse_preserves_only_playable_target_without_a_new_birth() {
    for state in ["playing", "paused", "held", "stopped", "none"] {
        let mut fixture = Fixture::new();
        fixture.load(2);
        let group = fixture.enqueue(&[0, 1, 2], true);
        fixture.drain();
        assert_eq!(group.publication_status(), "accepted");
        if state != "none" {
            start(&mut fixture, 2);
        }
        let runtime = runtime(&fixture);
        assert!(image(&fixture, &runtime, true).unwrap().is_none());
        if state == "paused" {
            fixture.engine.pause_sample(2).unwrap();
            fixture.drain();
        }
        let hold = if state == "held" {
            let hold = crate::audio_engine::hold_action::control::pause_for_producer(
                &fixture.engine,
                &fixture.producer,
                2,
                71,
                0,
            )
            .unwrap();
            fixture.drain();
            Some(hold)
        } else {
            None
        };
        if state == "stopped" {
            fixture.engine.stop_sample(2).unwrap();
            fixture.drain();
        }
        let before = fixture
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == 2)
            .map(|voice| (voice.generation, voice.frame_pos));
        let target = fixture.engine.current_explicit_playback_target().unwrap();
        let ticket = image(&fixture, &runtime, false).unwrap().unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        fixture.drain();
        assert_eq!(ticket.publication_status(), "accepted");
        if state != "playing" {
            assert!(active(&fixture).is_empty());
        } else {
            assert_eq!(active(&fixture), vec![2]);
            assert_eq!(
                fixture
                    .mixer
                    .voices
                    .iter()
                    .find(|voice| voice.active)
                    .map(|voice| (voice.generation, voice.frame_pos)),
                before
            );
        }
        if let Some(target) = target {
            assert_eq!(target.current_control_slot().unwrap(), Some(2));
        }
        if let Some(hold) = hold {
            assert!(
                crate::audio_engine::hold_action::control::release_for_producer(
                    &fixture.engine,
                    &fixture.producer,
                    &hold
                )
                .unwrap()
            );
            assert!(
                active(&fixture).is_empty(),
                "a retired Hold release cannot restart the collapsed target"
            );
        }
    }
}

#[test]
fn same_qualified_lock_reference_after_both_mode_changes_preserves_master_and_clock_epoch() {
    let mut fixture = Fixture::new();
    start(&mut fixture, 0);
    let runtime = runtime(&fixture);
    fixture.mixer.set_bpm_lock(true);
    fixture.mixer.set_master_period(PERIOD);
    assert!(fixture.transport.set_master_period(PERIOD));
    assert!(fixture.transport.anchor_beat_position_at_frame(3.25, 0));
    fixture.transport.advance_by_rendered_frames(137);
    let before = fixture.transport;
    let owner = fixture.engine.input_runtime_ownership.clone();
    let original = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let identity = crate::audio_engine::master_provenance::source_identity(&original).unwrap();
    let key = original.binding.placement.unwrap().content_key().unwrap();
    for multi_loop in [true, false] {
        let transition = image(&fixture, &runtime, multi_loop).unwrap();
        if transition.is_some() {
            fixture.drain();
        }
        let binding = input_runtime_binding::capture(&fixture.engine, 0)
            .unwrap()
            .unwrap();
        assert_eq!(binding.binding.placement.unwrap().content_key(), Some(key));
        let revision = owner.global_timing_revision.load(Ordering::Acquire) + 1;
        owner
            .global_timing_revision
            .store(revision, Ordering::Release);
        owner
            .global_rate_status
            .publish_bpm_requested(revision, true);
        fixture
            .mixer
            .stage_bpm_lock(crate::audio_engine::mixer::BpmLockTarget {
                enabled: true,
                revision,
                anchor: Some((0, binding.binding, key)),
                source_identity: Some(identity),
                hierarchy_stamp: owner.hierarchy_stamp.load(Ordering::Acquire),
            });
        fixture
            .mixer
            .finish_global_control_drain(Some(revision), true);
        fixture
            .mixer
            .finish_global_parameter_drain(Some(revision), true);
        fixture.mixer.service_bpm_lock(137, &mut fixture.transport);
        assert!(owner.global_rate_status.read_bpm().unwrap().1.applied);
        assert_eq!(
            owner
                .global_rate_status
                .read()
                .unwrap()
                .effect
                .effective
                .period,
            Some(PERIOD)
        );
        assert!(fixture.transport.matches_clock_epoch(&before));
        assert_eq!(format!("{:?}", fixture.transport), format!("{before:?}"));
    }
}

#[test]
fn transition_queue_refusal_preserves_the_complete_old_image_and_launch_fences() {
    let mut fixture = Fixture::with_control_capacity(1);
    let runtime = runtime(&fixture);
    image(&fixture, &runtime, true).unwrap();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let before: Vec<_> = (0..NUM_SAMPLES)
        .map(|id| ownership.launch_revision(id))
        .collect();
    let hierarchy = ownership.hierarchy_stamp.load(Ordering::Acquire);
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert!(image(&fixture, &runtime, false).is_err());
    assert_eq!(
        before,
        (0..NUM_SAMPLES)
            .map(|id| ownership.launch_revision(id))
            .collect::<Vec<_>>()
    );
    assert_eq!(hierarchy, ownership.hierarchy_stamp.load(Ordering::Acquire));
    fixture.drain();
    assert!(
        image(&fixture, &runtime, false).unwrap().is_some(),
        "the refused transition did not falsely publish OFF"
    );
}

#[test]
fn pending_retirement_is_a_barrier_before_later_control_effects() {
    let mut fixture = Fixture::new();
    start(&mut fixture, 0);
    let ticket = fixture.engine.stop_explicit_playback().unwrap();
    let mut retirement = RetainedBatchRetirement {
        slots: 0,
        batches: Vec::new(),
        samples: Vec::new(),
        revoke: None,
        calls: 0,
    };
    let command = fixture.consumer.pop().unwrap();
    fixture.apply(command, &mut retirement);
    assert_eq!(ticket.publication_status(), "pending");
    assert_eq!(active(&fixture), vec![0]);
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert_eq!(
        drain_control_messages(
            &mut fixture.consumer,
            &mut fixture.scheduler,
            0,
            &mut fixture.mode,
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut fixture.messages,
            &mut retirement
        ),
        0
    );
    assert!(!fixture.consumer.is_empty());
    retirement.slots = usize::MAX;
    drain_control_messages(
        &mut fixture.consumer,
        &mut fixture.scheduler,
        0,
        &mut fixture.mode,
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.messages,
        &mut retirement,
    );
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(active(&fixture).is_empty());
}

#[test]
fn exhausted_explicit_metadata_refuses_new_start_and_resume_without_losing_old_history() {
    let mut fixture = Fixture::new();
    start(&mut fixture, 0);
    let old = fixture
        .engine
        .current_explicit_playback_target()
        .unwrap()
        .unwrap();
    fixture
        .engine
        .input_runtime_ownership
        .exhaust_explicit_target_for_test();
    assert!(
        fixture
            .engine
            .input_runtime_ownership
            .explicit_playback_available(
                old.value.key,
                old.effective_frame(),
                old.original_sequence() - 1
            ),
        "an intentionally older effect does not need another target publication"
    );
    let attempted = fixture
        .engine
        .capture_pad_start(1, 0.125, Some(3.0), 1.0, false, None, None)
        .unwrap();
    fixture.drain();
    assert!(fixture.engine.publish_pad_start(&attempted, None).unwrap());
    fixture.drain();
    assert_eq!(attempted.state(), "rejected");
    assert_eq!(active(&fixture), vec![0]);
    assert_eq!(
        fixture
            .engine
            .current_explicit_playback_target()
            .unwrap()
            .unwrap()
            .original_sequence(),
        old.original_sequence()
    );
    fixture.engine.pause_sample(0).unwrap();
    fixture.drain();
    let resume = fixture.engine.resume_sample(0).unwrap();
    fixture.drain();
    let observation = resume.current_observation().unwrap().unwrap();
    assert_eq!(observation.outcome(), Some("refused"));
    assert!(observation.ordinary_paused());
    assert!(
        fixture
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .all(|voice| voice.paused)
    );
    assert_eq!(
        fixture
            .engine
            .current_explicit_playback_target()
            .unwrap()
            .unwrap()
            .original_sequence(),
        old.original_sequence()
    );
}
