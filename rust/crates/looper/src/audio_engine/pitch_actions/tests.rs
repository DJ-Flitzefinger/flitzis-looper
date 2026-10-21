use super::*;
use crate::audio_engine::input_runtime_binding::InputRuntimePadBinding;

fn action(ownership: Arc<InputRuntimeOwnership>, key: &str) -> CapturedPitchAction {
    let native = InputRuntimePadBinding::for_test(0, ownership.clone());
    let binding = native.binding;
    let spec = PitchActionSpec::parse(key).unwrap();
    CapturedPitchAction::capture(PitchActionAdmission {
        key: binding.placement.unwrap().content_key().unwrap(),
        binding,
        ownership,
        acknowledgements: native.acknowledgements.clone(),
        spec,
        render: FrozenPitchRender {
            pitch: spec.resolve(None, PitchIntent::default()).unwrap(),
            key_lock: true,
        },
        received_at_ns: 11,
        due_frame: 700,
        quantize_step_64ths: Some(4),
        sync: false,
        exclusive: true,
        loop_start_s: 0.25,
        loop_end_s: Some(2.0),
        input_owners: None,
        previous_requested: PitchIntent::default(),
        previous_sequence: 0,
    })
    .unwrap()
}

fn source_action(
    ownership: &Arc<InputRuntimeOwnership>,
    acknowledgements: &Arc<CurrentTimingAcknowledgements>,
    sample: &SampleBuffer,
) -> CapturedPitchAction {
    let binding = InputPadBinding {
        placement: ownership.slot_snapshot(0),
        resident: sample.resident_binding(),
        source_address: sample.source_address(),
        sample_count: sample.source_sample_count(),
        channels: sample.channels,
        sample_rate_hz: 48_000,
        authority_revision: ownership.authority_revision(0),
        runtime_revision: 0,
        accepted: None,
    };
    CapturedPitchAction::capture(PitchActionAdmission {
        key: binding.placement.unwrap().content_key().unwrap(),
        binding,
        ownership: ownership.clone(),
        acknowledgements: acknowledgements.clone(),
        spec: PitchActionSpec::parse("pitch.extra:2:set_retrigger").unwrap(),
        render: FrozenPitchRender {
            pitch: PitchIntent::new(0, 2).unwrap(),
            key_lock: true,
        },
        received_at_ns: 11,
        due_frame: 700,
        quantize_step_64ths: None,
        sync: false,
        exclusive: false,
        loop_start_s: 0.0,
        loop_end_s: None,
        input_owners: None,
        previous_requested: PitchIntent::default(),
        previous_sequence: 0,
    })
    .unwrap()
}

#[test]
fn every_absolute_target_uses_shortest_same_mode_distance_with_positive_tritone() {
    for mode in [PitchKeyMode::Major, PitchKeyMode::Minor] {
        for source in 0..12 {
            for target in 0..12 {
                let spec = PitchActionSpec {
                    target: PitchActionTarget::Base { root: target, mode },
                    variant: PitchActionVariant::Set,
                };
                let before = PitchIntent::new(2, 13).unwrap();
                let after = spec.resolve(Some((source, mode)), before).unwrap();
                let distance = (i32::from(target) - i32::from(source)).rem_euclid(12);
                assert_eq!(
                    after.base_shift(),
                    if distance > 6 {
                        distance - 12
                    } else {
                        distance
                    }
                );
                assert_eq!(after.extra_shift(), 13);
                assert_eq!(spec.resolve(Some((source, mode)), after), Ok(after));
            }
        }
    }
}

#[test]
fn unknown_and_different_mode_absolute_targets_reject_but_relative_and_reset_remain_valid() {
    let absolute = PitchActionSpec::parse("pitch.base:0:minor:set").unwrap();
    assert_eq!(
        absolute.resolve(None, PitchIntent::default()),
        Err(PitchActionReject::UnknownSourceKey)
    );
    assert_eq!(
        absolute.resolve(Some((0, PitchKeyMode::Major)), PitchIntent::default()),
        Err(PitchActionReject::ModeMismatch)
    );
    for extra in -18..=18 {
        let spec = PitchActionSpec::parse(&format!("pitch.extra:{extra}:set_retrigger")).unwrap();
        assert_eq!(
            spec.resolve(None, PitchIntent::new(-5, 4).unwrap()),
            Ok(PitchIntent::new(-5, extra).unwrap())
        );
        assert_eq!(spec.variant, PitchActionVariant::SetRetrigger);
    }
    let reset = PitchActionSpec::parse("pitch.base.reset:set").unwrap();
    assert_eq!(
        reset.resolve(None, PitchIntent::new(6, -18).unwrap()),
        Ok(PitchIntent::new(0, -18).unwrap())
    );
}

#[test]
fn piano_notes_preserve_octaves_and_the_existing_total_envelope() {
    for base in -23..=23 {
        let spec = PitchActionSpec::parse(&format!("pitch.note:{base}:set")).unwrap();
        assert_eq!(
            spec.resolve(None, PitchIntent::default()),
            Ok(PitchIntent::new(base, 0).unwrap())
        );
    }
    for base in [-12, 0, 12] {
        let note = PitchActionSpec::parse(&format!("pitch.note:{base}:set_retrigger")).unwrap();
        assert_eq!(
            note.resolve(None, PitchIntent::default())
                .unwrap()
                .base_shift(),
            base
        );
        assert_eq!(note.variant, PitchActionVariant::SetRetrigger);
    }
    let extreme = PitchActionSpec::parse("pitch.note:23:set").unwrap();
    assert_eq!(
        extreme.resolve(None, PitchIntent::new(0, 18).unwrap()),
        Err(PitchActionReject::UnsupportedPitch)
    );
    let extra = PitchActionSpec::parse("pitch.extra:-1:set").unwrap();
    assert_eq!(
        extra.resolve(None, PitchIntent::new(-23, 0).unwrap()),
        Err(PitchActionReject::UnsupportedPitch)
    );
}

#[test]
fn piano_and_offset_steps_resolve_at_admission_without_clamping() {
    for step in [-12, -1, 1, 12] {
        let note = PitchActionSpec::parse(&format!("pitch.note.step:{step}:set")).unwrap();
        assert_eq!(
            note.resolve(None, PitchIntent::new(0, 2).unwrap()),
            Ok(PitchIntent::new(step, 2).unwrap())
        );
        let extra =
            PitchActionSpec::parse(&format!("pitch.extra.step:{step}:set_retrigger")).unwrap();
        assert_eq!(
            extra.resolve(None, PitchIntent::new(2, 0).unwrap()),
            Ok(PitchIntent::new(2, step).unwrap())
        );
    }
    for (key, before) in [
        ("pitch.note.step:1:set", PitchIntent::new(23, 0).unwrap()),
        ("pitch.extra.step:1:set", PitchIntent::new(6, 18).unwrap()),
        ("pitch.extra.step:-1:set", PitchIntent::new(-23, 0).unwrap()),
    ] {
        assert_eq!(
            PitchActionSpec::parse(key).unwrap().resolve(None, before),
            Err(PitchActionReject::UnsupportedPitch)
        );
    }
}

#[test]
fn original_key_is_one_unknown_source_safe_action_with_a_reset_marker() {
    let original = PitchActionSpec::parse("pitch.original:set_retrigger").unwrap();
    assert_eq!(
        original.resolve(None, PitchIntent::new(12, 7).unwrap()),
        Ok(PitchIntent::default())
    );
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let captured = action(ownership.clone(), "pitch.original:set_retrigger");
    assert!(captured.resets_key_correction());
    assert_eq!(captured.variant(), PitchActionVariant::SetRetrigger);
    assert_eq!(captured.render().pitch, PitchIntent::default());
    assert!(!action(ownership, "pitch.extra:0:set").resets_key_correction());
}

#[test]
fn encoder_actions_require_original_midi_resolution_and_preserve_explicit_modes() {
    for mode in ["absolute", "twos_complement", "offset_binary"] {
        for lane in ["note", "extra"] {
            for step in [-12, -1, 1, 12] {
                for variant in ["set", "set_retrigger"] {
                    let key = format!("pitch.{lane}.encoder:{step}:{mode}:{variant}");
                    let spec = PitchActionSpec::parse(&key).unwrap();
                    assert_eq!(
                        spec.resolve(None, PitchIntent::default()),
                        Err(PitchActionReject::InvalidAction)
                    );
                    assert_eq!(
                        spec.variant,
                        if variant == "set" {
                            PitchActionVariant::Set
                        } else {
                            PitchActionVariant::SetRetrigger
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn malformed_and_outside_domain_actions_never_become_python_fallback_semantics() {
    for key in [
        "pitch.base:12:minor:set",
        "pitch.base:-1:minor:set",
        "pitch.base:0:unknown:set",
        "pitch.extra:19:set",
        "pitch.extra:-19:set",
        "pitch.extra:1.0:set",
        "pitch.extra:0:toggle",
        "pitch.extra:0:set:ignored",
        "pitch.base.reset:minor:set",
        "pitch.extra:+2:set",
        "pitch.extra:01:set",
        "pitch.extra:-0:set",
        "pitch.base:01:minor:set",
        "pitch.note:24:set",
        "pitch.note:-24:set",
        "pitch.note:+1:set",
        "pitch.note:01:set",
        "pitch.note:-0:set",
        "pitch.note.step:0:set",
        "pitch.note.step:2:set",
        "pitch.note.step:+12:set",
        "pitch.extra.step:2:set",
        "pitch.original:0:set",
        "pitch.note.encoder:1:unknown:set",
        "pitch.note.encoder:2:absolute:set",
        "pitch.extra.encoder:+1:absolute:set",
        "pitch.extra.encoder:1:button:set",
        "pitch.extra.encoder:01:twos_complement:set",
        "pitch.extra.encoder:1:twos_complement:set:extra",
    ] {
        assert!(PitchActionSpec::parse(key).is_none(), "{key}");
    }
}

#[test]
fn equal_retrigger_attacks_are_independent_owners_and_publish_is_not_effective_pitch() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let first = action(ownership.clone(), "pitch.extra:0:set_retrigger");
    let second = action(ownership, "pitch.extra:0:set_retrigger");
    assert!(second.sequence() > first.sequence());
    assert!(!Arc::ptr_eq(&first.owner, &second.owner));
    assert!(!Arc::ptr_eq(
        first.preparation_epoch(),
        second.preparation_epoch()
    ));
    assert_eq!(first.render(), second.render());
    assert!(!first.accepted());
    first.mark_published().unwrap();
    assert!(first.is_published());
    assert!(
        !first.accepted(),
        "control queue publication alone is not scheduler admission"
    );
    first.mark_scheduled();
    assert!(first.accepted());
    assert!(!first.effective());
    assert_eq!(first.effective_output_frame(), None);
    assert_eq!(first.state(), "scheduled");
    assert_eq!(
        first.mark_published(),
        Err(PitchActionReject::AlreadyPublished)
    );
    second.mark_published().unwrap();
    assert!(first.control_guard(None).is_ok());
    assert!(first.claim_effect());
    assert!(first.mark_effective(705));
    assert_eq!(first.state(), "applied");
    assert_eq!(first.effective_output_frame(), Some(705));
    assert_eq!(first.original_due_frame(), 700);
    assert_eq!(first.received_timestamp(), 11);
    assert!(!second.effective());
}

#[test]
fn source_request_and_removed_lifetime_fence_the_original_action_without_retargeting() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let first = action(ownership.clone(), "pitch.extra:2:set_retrigger");
    first.mark_published().unwrap();
    ownership
        .prepare_source_request_advance(0)
        .unwrap()
        .commit();
    assert!(matches!(
        first.control_guard(None),
        Err(PitchActionReject::SourceChanged)
    ));
    assert_eq!(
        first.preparation_guard(None),
        Err(PitchActionReject::SourceChanged)
    );
    let second = action(ownership.clone(), "pitch.extra:-2:set_retrigger");
    second.mark_published().unwrap();
    let placement = ownership.slot_snapshot(0).unwrap();
    let remove = ownership.prepare_slot_write_fixed(placement, 0).unwrap();
    ownership
        .commit_slot_write_fixed(&remove, || true, |_| {})
        .unwrap();
    assert_eq!(
        second.preparation_guard(None),
        Err(PitchActionReject::StaleContent)
    );
    let refill = ownership
        .prepare_slot_write_fixed(
            ownership.slot_snapshot(0).unwrap(),
            crate::audio_engine::content_lifetime::reserve().unwrap(),
        )
        .unwrap();
    ownership
        .commit_slot_write_fixed(&refill, || true, |_| {})
        .unwrap();
    assert!(matches!(
        second.control_guard(None),
        Err(PitchActionReject::StaleContent)
    ));
    assert!(matches!(
        second.callback_guard(None),
        Err(PitchActionReject::StaleContent)
    ));
    assert_eq!(
        second.preparation_guard(None),
        Err(PitchActionReject::StaleContent)
    );
}

#[test]
fn pitch_runtime_revision_does_not_globally_stale_prior_actions_and_local_reject_is_independent() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let first = action(ownership.clone(), "pitch.extra:2:set_retrigger");
    let second = action(ownership.clone(), "pitch.extra:-2:set_retrigger");
    first.mark_published().unwrap();
    second.mark_published().unwrap();
    ownership
        .runtime_for_cell(first.content_key().cell())
        .fetch_add(1, Ordering::AcqRel);
    assert!(first.control_guard(None).is_ok());
    assert!(second.control_guard(None).is_ok());
    assert_eq!(first.preparation_guard(None), Ok(()));
    assert_eq!(second.preparation_guard(None), Ok(()));
    second.reject(PitchActionReject::PreparationFailed);
    assert_eq!(second.reason(), Some("preparation_failed"));
    assert_eq!(second.epoch(), 0);
    assert_eq!(first.epoch(), 1);
    assert!(first.control_guard(None).is_ok());
    assert_eq!(first.preparation_guard(None), Ok(()));
    assert_eq!(
        second.preparation_guard(None),
        Err(PitchActionReject::StaleContent)
    );
}

#[test]
fn admitted_content_follows_swap_and_native_control_gap_remains_pending() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let original = action(ownership.clone(), "pitch.extra:2:set_retrigger");
    InputRuntimePadBinding::for_test(1, ownership.clone());
    original.mark_published().unwrap();
    let captured = (
        original.render(),
        original.original_due_frame(),
        original.execution_target(),
        original.received_timestamp(),
        original.sequence(),
        original.epoch(),
    );
    let pair = ownership
        .prepare_placement_pair(
            ownership.slot_snapshot(0).unwrap(),
            ownership.slot_snapshot(1).unwrap(),
        )
        .unwrap();
    let commit = ownership
        .commit_placement_pair(
            &pair,
            true,
            || {
                assert_eq!(original.preparation_guard(None), Ok(()));
                assert!(matches!(
                    original.control_guard(None),
                    Err(PitchActionReject::PlacementPending)
                ));
                assert!(matches!(
                    original.callback_guard(None),
                    Err(PitchActionReject::PlacementPending)
                ));
                true
            },
            || {},
        )
        .unwrap();
    assert_eq!(original.preparation_guard(None), Ok(()));
    assert_eq!(
        original.preparation_guard(Some(InputPadBinding {
            placement: Some(commit.source_after),
            ..original.owner.admission.binding
        })),
        Ok(())
    );
    assert!(matches!(
        original.control_guard(None),
        Err(PitchActionReject::PlacementPending)
    ));
    assert_eq!(original.callback_guard(None).unwrap().id, 1);
    ownership.publish_pair_control_slots(&[commit.source_after, commit.target_after]);
    assert_eq!(original.control_guard(None).unwrap().id, 1);
    assert_eq!(original.render().pitch.extra_shift(), 2);
    assert_eq!(original.original_due_frame(), 700);
    assert_eq!(
        (
            original.render(),
            original.original_due_frame(),
            original.execution_target(),
            original.received_timestamp(),
            original.sequence(),
            original.epoch(),
        ),
        captured
    );
    super::super::input_runtime_binding::cancel_launches_with_producer(&ownership, None, Some(0))
        .unwrap();
    assert_eq!(original.preparation_guard(None), Ok(()));
    super::super::input_runtime_binding::cancel_launches_with_producer(&ownership, None, Some(1))
        .unwrap();
    assert_eq!(
        original.preparation_guard(None),
        Err(PitchActionReject::LaunchCancelled)
    );
}

#[test]
fn accepted_action_survives_later_learn_changes_but_uncaptured_publication_does_not() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let mut pending = action(ownership.clone(), "pitch.extra:2:set");
    let mapping = Arc::new(AtomicBool::new(true));
    let enabled = Arc::new(AtomicBool::new(true));
    let learn = Arc::new(AtomicBool::new(false));
    Arc::get_mut(&mut pending.owner)
        .unwrap()
        .admission
        .input_owners = Some((mapping.clone(), enabled.clone(), learn.clone()));
    pending.mark_published().unwrap();
    mapping.store(false, Ordering::Release);
    enabled.store(false, Ordering::Release);
    learn.store(true, Ordering::Release);
    assert!(pending.control_guard(None).is_ok());
    assert_eq!(pending.preparation_guard(None), Ok(()));
    let mut unadmitted = action(ownership, "pitch.extra:2:set");
    Arc::get_mut(&mut unadmitted.owner)
        .unwrap()
        .admission
        .input_owners = Some((mapping, enabled, learn));
    assert_eq!(
        unadmitted.preparation_guard(None),
        Err(PitchActionReject::InputDisabled)
    );
    assert_eq!(
        unadmitted.mark_published(),
        Err(PitchActionReject::InputDisabled)
    );
}

#[test]
fn newer_actual_timing_ack_cannot_reuse_old_static_binding_numbers() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let original = action(ownership.clone(), "pitch.extra:2:set");
    original.mark_published().unwrap();
    // The old static binding has no accepted timing. A new actual ACK on the
    // same source/lifetime must fence it even when timing mode did not change.
    original
        .owner
        .admission
        .acknowledgements
        .acknowledge_for_key(&ownership, original.content_key(), 8);
    assert!(matches!(
        original.control_guard(None),
        Err(PitchActionReject::TimingChanged)
    ));
    assert!(matches!(
        original.callback_guard(None),
        Err(PitchActionReject::TimingChanged)
    ));
    assert_eq!(
        original.preparation_guard(None),
        Err(PitchActionReject::TimingChanged)
    );
}

#[test]
fn preparation_keeps_tracked_source_generation_authority_and_window_fences() {
    let sample = SampleBuffer {
        samples: Arc::from([0.25; 128]),
        channels: 1,
        residency: None,
    }
    .with_complete_source(48_000);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &sample, 48_000, 3);
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let generation_action = source_action(&ownership, &acknowledgements, &sample);
    assert_eq!(generation_action.preparation_guard(None), Ok(()));
    ownership.publish_source(0, &sample, 48_000, 4);
    assert!(ownership.content_key_current(generation_action.content_key()));
    assert_eq!(
        generation_action.preparation_guard(None),
        Err(PitchActionReject::SourceChanged)
    );

    let authority_action = source_action(&ownership, &acknowledgements, &sample);
    ownership
        .authority_for_cell(authority_action.content_key().cell())
        .fetch_add(1, Ordering::AcqRel);
    assert_eq!(
        authority_action.preparation_guard(None),
        Err(PitchActionReject::SourceChanged)
    );

    let window_action = source_action(&ownership, &acknowledgements, &sample);
    let next_window = SampleBuffer {
        residency: Some(Arc::new(crate::messages::ResidentSourceView {
            window_revision: 2,
            ..sample.residency.as_ref().unwrap().as_ref().clone()
        })),
        ..sample.clone()
    };
    ownership.publish_window(0, &next_window);
    assert_eq!(
        window_action.preparation_guard(None),
        Err(PitchActionReject::WindowPending)
    );
    let own_window = InputPadBinding {
        resident: next_window.resident_binding(),
        ..window_action.owner.admission.binding
    };
    assert_eq!(window_action.preparation_guard(Some(own_window)), Ok(()));
    assert_eq!(
        window_action.preparation_guard(Some(InputPadBinding {
            source_address: own_window.source_address + 1,
            ..own_window
        })),
        Err(PitchActionReject::SourceChanged)
    );
}

#[test]
fn worker_returns_failed_original_owner_and_callback_lane_retires_intact() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let original = action(ownership, "pitch.extra:2:set_retrigger");
    let work = Box::new(PitchActionWork {
        action: original.clone(),
        candidate: None,
        render_format: (48_000, 1),
        request: None,
        attack_measurement: None,
        captured: false,
        execution_frozen: false,
        launch_delay_frames: 0,
        source_control_revision: None,
        own_window: None,
        preparation_mode: PitchActionPreparationMode::Endpoint,
        preparation_required: true,
        native_required: true,
        dsp_intent: None,
        normal_launch_origin: None,
        late_replan: None,
        source_feed: SourceFeedSnapshot::new(),
        source_feed_future: None,
        source_feed_voice: None,
        endpoint_reset_history: None,
        _custody: super::super::pitch_publication::PitchWorkCapacity::default()
            .claim()
            .unwrap(),
    });
    let (mut lane, worker) = PitchActionPreparation::new(1).unwrap();
    let mut retained = Some(work);
    assert_eq!(lane.publish(&mut retained), Ok(true));
    assert!(retained.is_none());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let returned = loop {
        if let Some(work) = lane.poll() {
            break work;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "worker did not return terminal owner"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert!(Arc::ptr_eq(&returned.action.owner, &original.owner));
    assert_eq!(original.state(), "rejected");
    assert_eq!(original.reason(), Some("prepared_mismatch"));
    let mut retained = Some(returned);
    assert!(lane.retire(&mut retained));
    assert!(retained.is_none());
    drop(worker);
}

#[test]
fn deferred_execution_coordinate_is_once_only_and_original_input_target_is_unchanged() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let original = action(ownership, "pitch.extra:2:set_retrigger");
    let mut work = PitchActionWork {
        action: original.clone(),
        candidate: None,
        render_format: (48_000, 1),
        request: None,
        attack_measurement: None,
        captured: false,
        execution_frozen: false,
        launch_delay_frames: 0,
        source_control_revision: None,
        own_window: None,
        preparation_mode: PitchActionPreparationMode::Endpoint,
        preparation_required: true,
        native_required: true,
        dsp_intent: None,
        normal_launch_origin: None,
        late_replan: None,
        source_feed: SourceFeedSnapshot::new(),
        source_feed_future: None,
        source_feed_voice: None,
        endpoint_reset_history: None,
        _custody: super::super::pitch_publication::PitchWorkCapacity::default()
            .claim()
            .unwrap(),
    };
    assert_eq!(
        work.freeze_execution_frame(699),
        Err(PitchActionReject::PreparedMismatch)
    );
    work.freeze_execution_frame(4096).unwrap();
    assert_eq!(original.original_due_frame(), 700);
    assert_eq!(original.execution_target(), 4096);
    assert_eq!(original.received_timestamp(), 11);
    assert_eq!(
        work.freeze_execution_frame(8192),
        Err(PitchActionReject::PreparedMismatch)
    );
}

#[test]
fn action_capture_requires_own_epoch_arc_even_when_all_scalar_pitch_source_and_due_fields_match() {
    use crate::audio_engine::productive_source_history::ProductiveSourceBinding;
    use crate::audio_engine::source_playback::SourcePlayback;
    use crate::audio_engine::source_reader::{
        ExplicitSeekMode, FrameRange, SourceReadPlan, StemRenderSelection, StemTransition,
    };
    let sample = SampleBuffer {
        samples: Arc::from([0.25; 128]),
        channels: 1,
        residency: None,
    }
    .with_complete_source(48_000);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &sample, 48_000, 3);
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let first = source_action(&ownership, &acknowledgements, &sample);
    let second = source_action(&ownership, &acknowledgements, &sample);
    let permit = first.capture_history_permit(&sample, 48_000, None).unwrap();
    let region = FrameRange { start: 0, end: 128 };
    let mut playback = SourcePlayback::new(0, ExplicitSeekMode::Normal, 1.0);
    assert!(playback.configure(128, region));
    let plan = SourceReadPlan {
        channels: 1,
        sample_frames: 128,
        frame_pos: 0,
        loop_region: region,
        loop_period: None,
        seek_mode: ExplicitSeekMode::Normal,
        selection: StemRenderSelection::full_mix(),
        transition: StemTransition::default(),
    };
    let request = NativeHistoryRequest {
        pitch_intent: second.render().pitch,
        key_lock: second.render().key_lock,
        sample: sample.clone(),
        stems: None,
        permit,
        coverage: None,
        binding: ProductiveSourceBinding::new(&sample, 48_000, None),
        playback,
        plan,
        actual_feed: None,
        target_output_frame: second.execution_target(),
        request_id: second.sequence(),
        epoch: second.epoch(),
    };
    let mut work = PitchActionWork {
        action: second,
        candidate: None,
        render_format: (48_000, 1),
        request: None,
        attack_measurement: None,
        captured: false,
        execution_frozen: false,
        launch_delay_frames: 0,
        source_control_revision: None,
        own_window: None,
        preparation_mode: PitchActionPreparationMode::Endpoint,
        preparation_required: true,
        native_required: true,
        dsp_intent: None,
        normal_launch_origin: None,
        late_replan: None,
        source_feed: SourceFeedSnapshot::new(),
        source_feed_future: None,
        source_feed_voice: None,
        endpoint_reset_history: None,
        _custody: super::super::pitch_publication::PitchWorkCapacity::default()
            .claim()
            .unwrap(),
    };
    let rejected = match work.rt_capture(request) {
        Err(request) => request,
        Ok(()) => panic!("other action epoch owner was relabeled"),
    };
    assert!(work.capture_available());
    assert!(work.retain_rejected_capture(rejected).is_ok());
    assert!(!work.capture_available());
}

#[test]
fn no_preparation_completion_keeps_set_pending_until_actual_application() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let original = action(ownership, "pitch.extra:2:set");
    let mut work = PitchActionWork {
        action: original.clone(),
        candidate: None,
        render_format: (48_000, 1),
        request: None,
        attack_measurement: None,
        captured: false,
        execution_frozen: false,
        launch_delay_frames: 0,
        source_control_revision: None,
        own_window: None,
        preparation_mode: PitchActionPreparationMode::Endpoint,
        preparation_required: true,
        native_required: true,
        dsp_intent: None,
        normal_launch_origin: None,
        late_replan: None,
        source_feed: SourceFeedSnapshot::new(),
        source_feed_future: None,
        source_feed_voice: None,
        endpoint_reset_history: None,
        _custody: super::super::pitch_publication::PitchWorkCapacity::default()
            .claim()
            .unwrap(),
    };
    work.freeze_execution_frame(705).unwrap();
    assert!(work.preparation_required());
    work.complete_without_preparation().unwrap();
    assert!(!work.preparation_required());
    assert_eq!(original.state(), "prepared");
    assert!(!original.effective());
    assert!(work.rt_ready(705).is_ok());
    assert!(matches!(
        work.rt_ready(706),
        Err(PitchActionReject::PreparedMismatch)
    ));
    assert!(work.candidate.is_none());
    assert_eq!(
        work.set_preparation_mode(PitchActionPreparationMode::ForwardHistory),
        Err(PitchActionReject::PreparedMismatch)
    );
}

#[test]
fn fixed_claim_guard_remains_valid_inside_existing_placement_marker() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let original = action(ownership.clone(), "pitch.extra:2:set");
    let work = PitchActionWork {
        action: original,
        candidate: None,
        render_format: (48_000, 1),
        request: None,
        attack_measurement: None,
        captured: false,
        execution_frozen: false,
        launch_delay_frames: 0,
        source_control_revision: None,
        own_window: None,
        preparation_mode: PitchActionPreparationMode::Endpoint,
        preparation_required: true,
        native_required: true,
        dsp_intent: None,
        normal_launch_origin: None,
        late_replan: None,
        source_feed: SourceFeedSnapshot::new(),
        source_feed_future: None,
        source_feed_voice: None,
        endpoint_reset_history: None,
        _custody: super::super::pitch_publication::PitchWorkCapacity::default()
            .claim()
            .unwrap(),
    };
    let guard = work.current_guard().unwrap();
    let placement = guard.binding.placement.unwrap();
    let write = ownership
        .prepare_slot_write_fixed(placement, placement.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(&write, || work.claim_current(guard), |_| {})
        .unwrap();
}

#[test]
fn history_mode_is_explicit_and_cannot_be_changed_after_preparation_free_completion() {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let original = action(ownership, "pitch.extra:0:set_retrigger");
    let mut work = PitchActionWork {
        action: original,
        candidate: None,
        render_format: (48_000, 1),
        request: None,
        attack_measurement: None,
        captured: false,
        execution_frozen: false,
        launch_delay_frames: 0,
        source_control_revision: None,
        own_window: None,
        preparation_mode: PitchActionPreparationMode::Endpoint,
        preparation_required: true,
        native_required: true,
        dsp_intent: None,
        normal_launch_origin: None,
        late_replan: None,
        source_feed: SourceFeedSnapshot::new(),
        source_feed_future: None,
        source_feed_voice: None,
        endpoint_reset_history: None,
        _custody: super::super::pitch_publication::PitchWorkCapacity::default()
            .claim()
            .unwrap(),
    };
    work.set_preparation_mode(PitchActionPreparationMode::ForwardHistory)
        .unwrap();
    assert_eq!(
        work.preparation_mode,
        PitchActionPreparationMode::ForwardHistory
    );
    work.complete_without_preparation().unwrap();
    assert_eq!(
        work.set_preparation_mode(PitchActionPreparationMode::Endpoint),
        Err(PitchActionReject::PreparedMismatch)
    );
}
