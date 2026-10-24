//! Original public runtime/callback/reader regression for automatic service only.
use super::*;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::runtime_policy_retry::RuntimePolicyRetryRegistry;
use std::sync::atomic::Ordering;

fn automatic(path: &Path) -> Arc<RuntimePolicySnapshot> {
    path.source
        .engine
        .request_input_runtime_policy_adoption(true)
        .unwrap();
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .runtime_policy_snapshot()
        .unwrap()
        .unwrap()
}

#[test]
fn unchanged_mapped_unsupported_is_quiet_for_1500_actual_polls_and_reader_renders() {
    let mut path = prepared();
    let mut reference = prepared();
    let old_request = path.voice(0).sync_request;
    let generation = path.voice(0).generation;
    path.publish(Some(wire(true, true, 2)), true, None);
    let snapshot = automatic(&path);
    service(&mut path);
    let refusal = snapshot.observation().unwrap();
    assert_eq!(refusal.reason, Some(RuntimePolicyError::Unsupported));
    for _ in 0..1500 {
        assert!(Arc::ptr_eq(&snapshot, &automatic(&path)));
        assert_eq!(snapshot.observation(), Some(refusal));
        assert!(path.source.consumer.is_empty());
        assert_eq!(path.scheduler.len(), 0);
        assert_eq!(path.render(3), reference.render(3));
        assert_eq!(path.voice(0).sync_request, old_request);
        assert_eq!(path.voice(0).generation, generation);
        assert!(path.timeline.matches_clock_epoch(&reference.timeline));
    }
    assert!(Arc::ptr_eq(&snapshot, &request(&path)));
    assert_eq!(snapshot.observation().unwrap().attempt, refusal.attempt + 1);
    // Deliberate retry still has one original dispatch owner. Neither mode can
    // bypass Pending or its final cleanup reservation and enqueue another Work.
    let pending = snapshot.observation();
    automatic(&path);
    request(&path);
    assert_eq!(snapshot.observation(), pending);
    service(&mut path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
}

#[test]
fn automatic_same_snapshot_recovers_after_real_hold_release_and_resume() {
    for hold in [false, true] {
        let mut path = prepared();
        let ticket = if hold {
            Some(Python::attach(|py| {
                path.source
                    .engine
                    .pause_waveform_hold(0, 811_u64.into_pyobject(py).unwrap().as_any(), None)
                    .unwrap()
            }))
        } else {
            path.source
                .mixer
                .pause_sample_at_output_frame(0, path.timeline.output_frame());
            None
        };
        path.drain();
        path.publish(Some(wire(true, false, 2)), true, None);
        let before = scalar_state(&path);
        let snapshot = automatic(&path);
        service(&mut path);
        let refused = snapshot.observation().unwrap();
        assert_eq!(refused.reason, Some(RuntimePolicyError::Unsupported));
        unchanged(&path, &before);
        for _ in 0..16 {
            automatic(&path);
            path.render(0);
            assert_eq!(snapshot.observation(), Some(refused));
            assert!(path.source.consumer.is_empty());
        }
        if let Some(ticket) = ticket {
            assert!(path.source.engine.release_waveform_hold(&ticket).unwrap());
            path.drain();
            assert_eq!(ticket.publication_status(), "resumed");
        } else {
            path.source
                .mixer
                .resume_sample_at_output_frame(0, path.timeline.output_frame());
        }
        path.render(0);
        assert!(Arc::ptr_eq(&snapshot, &automatic(&path)));
        assert_eq!(snapshot.observation().unwrap().attempt, refused.attempt + 1);
        service(&mut path);
        assert_eq!(snapshot.observation().unwrap().state, "applied");
    }
}

#[test]
fn automatic_same_snapshot_recovers_after_original_renderer_domain_configuration() {
    let mut path = prepared();
    let raw = SourcePlayback::new(23, ExplicitSeekMode::Normal, 1.5);
    path.source
        .mixer
        .voices
        .iter_mut()
        .find(|voice| voice.active)
        .unwrap()
        .source_playback = raw;
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = automatic(&path);
    service(&mut path);
    let refused = snapshot.observation().unwrap();
    assert_eq!(refused.reason, Some(RuntimePolicyError::Unsupported));
    automatic(&path);
    assert_eq!(snapshot.observation(), Some(refused));
    path.render(1);
    assert!(Arc::ptr_eq(&snapshot, &automatic(&path)));
    service(&mut path);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
}

#[test]
fn actual_unsupported_inputs_prevent_free_lane_latch_after_release_and_false_true_false_aba() {
    for originally_busy in [false, true] {
        let mut path = prepared();
        path.publish(Some(wire(true, false, 2)), true, None);
        let ownership = path.source.engine.input_runtime_ownership.clone();
        let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
        ownership
            .resident_control_for_cell(key.cell())
            .store(u64::from(originally_busy), Ordering::Release);
        let before_owner = ownership.clone();
        let after_owner = ownership.clone();
        let snapshot = automatic(&path);
        let before_audio = scalar_state(&path);
        RtMixer::with_runtime_policy_unsupported_hooks_for_test(
            Some(Box::new(move || {
                before_owner
                    .resident_control_for_cell(key.cell())
                    .store(1, Ordering::Release)
            })),
            Some(Box::new(move || {
                after_owner
                    .resident_control_for_cell(key.cell())
                    .store(0, Ordering::Release)
            })),
            || service(&mut path),
        );
        assert_eq!(
            snapshot.observation().unwrap().reason,
            Some(RuntimePolicyError::Unsupported)
        );
        unchanged(&path, &before_audio);
        let refused = snapshot.observation().unwrap();
        assert!(Arc::ptr_eq(&snapshot, &automatic(&path)));
        assert_eq!(snapshot.observation().unwrap().attempt, refused.attempt + 1);
        service(&mut path);
        assert_eq!(snapshot.observation().unwrap().state, "applied");
    }
}

#[test]
fn actual_busy_lane_retries_after_relevant_release_without_request_or_cursor_identity() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let ownership = path.source.engine.input_runtime_ownership.clone();
    let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
    ownership
        .resident_control_for_cell(key.cell())
        .store(1, Ordering::Release);
    let snapshot = automatic(&path);
    service(&mut path);
    let refused = snapshot.observation().unwrap();
    assert_eq!(refused.reason, Some(RuntimePolicyError::Unsupported));
    automatic(&path);
    assert_eq!(snapshot.observation(), Some(refused));
    ownership
        .resident_control_for_cell(key.cell())
        .store(0, Ordering::Release);
    path.render(0);
    automatic(&path);
    service(&mut path);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
}

#[test]
fn observer_exhaustion_and_unreadability_leave_original_automatic_retry_open() {
    for exhausted in [false, true] {
        let mut path = prepared();
        path.publish(Some(wire(true, true, 2)), true, None);
        let snapshot = automatic(&path);
        service(&mut path);
        let refused = snapshot.observation().unwrap();
        let registry = &path
            .source
            .engine
            .input_runtime_ownership
            .runtime_policy_retry;
        if exhausted {
            registry.exhaust_for_test();
        } else {
            registry.invalidate_current_for_test();
        }
        automatic(&path);
        assert_eq!(snapshot.observation().unwrap().attempt, refused.attempt + 1);
        service(&mut path);
        assert_eq!(
            snapshot.observation().unwrap().reason,
            Some(RuntimePolicyError::Unsupported)
        );
        if exhausted {
            automatic(&path);
            assert_eq!(snapshot.observation().unwrap().attempt, refused.attempt + 2);
        }
    }
}

#[test]
fn condition_cas_collision_does_not_commit_dirty_shape_or_strand_future_observation() {
    let mut path = prepared();
    path.publish(Some(wire(true, true, 2)), true, None);
    let ownership = path.source.engine.input_runtime_ownership.clone();
    let snapshot = automatic(&path);
    RuntimePolicyRetryRegistry::with_before_condition_cas_for_test(
        Box::new(move || {
            ownership.runtime_policy_retry.next_condition();
        }),
        || service(&mut path),
    );
    let refused = snapshot.observation().unwrap();
    automatic(&path);
    assert_eq!(snapshot.observation().unwrap().attempt, refused.attempt + 1);
    service(&mut path);
    let current = snapshot.observation();
    for _ in 0..16 {
        automatic(&path);
        path.render(0);
        assert_eq!(snapshot.observation(), current);
        assert!(path.source.consumer.is_empty());
    }
}

#[test]
fn original_qualified_map_delivery_changes_candidate_and_real_retrigger_recovers() {
    let mut path = prepared();
    path.publish(Some(wire(true, true, 2)), true, None);
    let old = automatic(&path);
    service(&mut path);
    let refused = old.observation();
    automatic(&path);
    assert_eq!(old.observation(), refused);
    let (_job, delivery) = path.map();
    // Identical global choices/revision; the original selected delivery alone
    // qualifies another Native runtime candidate without a user toggle.
    path.publish(Some(wire(true, true, 2)), true, Some(&delivery));
    let current = automatic(&path);
    assert!(!Arc::ptr_eq(&old, &current));
    assert_ne!(old.revision(), current.revision());
    let before = scalar_state(&path);
    service(&mut path);
    assert_eq!(
        current.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&path, &before);
    let start = path.start();
    path.render(1);
    assert_eq!(start.state(), "applied");
    assert!(path.voice(0).source_playback.is_mapped());
    assert_eq!(path.feedback(0)["effective"], true);
    assert_eq!(path.feedback(0)["policy_mode"], "mapped");
    let historical_refusal = current.observation().unwrap();
    let before_completion = scalar_state(&path);
    assert!(Arc::ptr_eq(&current, &automatic(&path)));
    let pending = current.observation().unwrap();
    assert_eq!(pending.state, "pending");
    assert_eq!(pending.attempt, historical_refusal.attempt + 1);
    assert!(!path.source.consumer.is_empty());
    assert!(Arc::ptr_eq(&current, &automatic(&path)));
    assert_eq!(current.observation(), Some(pending));
    unchanged(&path, &before_completion);
    assert!(path.feedback(0)["grid_phase_confirmed"].is_null());
    // A mapped birth still requires the original completion claim. This
    // regression proves queued/deduplicated progress, not phase eligibility
    // or a terminal result for this original decimal/physical launch fixture.
}

#[test]
fn full_cohort_observation_includes_an_older_active_slot_of_the_same_pad() {
    let mut path = prepared();
    let binding = path
        .source
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap();
    let old = path.voice(0).source_playback;
    let request_before = path.voice(0).sync_request;
    let original_admission = path.voice(0).source_admission.as_ref().unwrap().clone();
    let original_identity = original_admission.retry_identity();
    let original_key = original_admission.content_key().unwrap();
    // Adversarial cohort fixture: retain a second genuine admitted source pin in
    // another existing voice slot. Public starts normally replace their old slot.
    let mut config = path
        .source
        .mixer
        .prepare_admitted_play_sample_rt(
            0,
            1.0,
            path.timeline.output_frame(),
            binding.binding,
            &mut ImmediateAudioBufferRetirement,
        )
        .unwrap();
    assert!(config.source_admission.is_none());
    assert!(
        config
            .sample
            .same_window(path.voice(0).sample.as_ref().unwrap())
    );
    assert!(original_admission.current_for_birth(
        &path.source.engine.input_runtime_ownership,
        &path.source.engine.current_timing_acknowledgements,
        &config.sample,
        RATE,
        true,
    ));
    // Retain the proof from the actual original claim for this same immutable
    // pin. The fixture does not manufacture or recapture another source birth.
    config.source_admission = Some(original_admission);
    let index = crate::audio_engine::constants::MAX_VOICES - 1;
    path.source.mixer.voices[index].start_rt(config, &mut ImmediateAudioBufferRetirement);
    let retained = path.source.mixer.voices[index]
        .source_admission
        .as_ref()
        .unwrap();
    assert_eq!(retained.retry_identity(), original_identity);
    assert_eq!(retained.content_key(), Some(original_key));
    assert_eq!(
        retained.retry_identity().source_generation,
        original_identity.source_generation
    );
    assert_ne!(path.source.mixer.voices[index].generation, 0);
    path.source.mixer.voices[index].source_playback = old;
    path.source.mixer.voices[index].sync_request = request_before;
    path.source.mixer.voices[index].paused = true;
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = automatic(&path);
    let before = scalar_state(&path);
    service(&mut path);
    let refused = snapshot.observation().unwrap();
    assert_eq!(refused.attempt, 1);
    assert_eq!(refused.state, "refused");
    assert_eq!(refused.reason, Some(RuntimePolicyError::Unsupported));
    assert_eq!(refused.execution_frame, Some(17));
    unchanged(&path, &before);
    for _ in 0..16 {
        automatic(&path);
        path.render(0);
        assert_eq!(snapshot.observation(), Some(refused));
        assert!(path.source.consumer.is_empty());
    }
    path.source.mixer.voices[index].paused = false;
    path.render(0);
    automatic(&path);
    service(&mut path);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert!(
        path.source
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .all(|voice| voice.sync_request.global_revision == Some(2))
    );
}

#[test]
fn same_pad_slot_without_original_admission_keeps_automatic_retry_open_and_pending_deduplicated() {
    let mut path = prepared();
    let binding = path
        .source
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap();
    let old = path.voice(0).source_playback;
    let request_before = path.voice(0).sync_request;
    let config = path
        .source
        .mixer
        .prepare_admitted_play_sample_rt(
            0,
            1.0,
            path.timeline.output_frame(),
            binding.binding,
            &mut ImmediateAudioBufferRetirement,
        )
        .unwrap();
    assert!(config.source_admission.is_none());
    let index = crate::audio_engine::constants::MAX_VOICES - 1;
    path.source.mixer.voices[index].start_rt(config, &mut ImmediateAudioBufferRetirement);
    path.source.mixer.voices[index].source_playback = old;
    path.source.mixer.voices[index].sync_request = request_before;
    path.source.mixer.voices[index].paused = true;
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = automatic(&path);
    let before = scalar_state(&path);
    let execution = path.timeline.output_frame();
    service(&mut path);
    let refused = snapshot.observation().unwrap();
    assert_eq!(refused.attempt, 1);
    assert_eq!(refused.state, "refused");
    assert_eq!(refused.reason, Some(RuntimePolicyError::Unsupported));
    assert_eq!(refused.execution_frame, Some(execution));
    unchanged(&path, &before);
    assert!(Arc::ptr_eq(&snapshot, &automatic(&path)));
    let pending = snapshot.observation().unwrap();
    assert_eq!(pending.attempt, 2);
    assert_eq!(pending.state, "pending");
    assert_eq!(pending.reason, None);
    assert_eq!(pending.execution_frame, None);
    assert!(!path.source.consumer.is_empty());
    unchanged(&path, &before);
    assert!(Arc::ptr_eq(&snapshot, &automatic(&path)));
    assert_eq!(snapshot.observation(), Some(pending));
    service(&mut path);
    unchanged(&path, &before);
    assert_eq!(snapshot.observation().unwrap().attempt, 2);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    assert!(path.source.consumer.is_empty());
}

#[test]
fn closing_the_original_mixer_invalidates_its_cached_retry_observation_off_rt() {
    let mut path = prepared();
    path.publish(Some(wire(true, true, 2)), true, None);
    automatic(&path);
    service(&mut path);
    let ownership = path.source.engine.input_runtime_ownership.clone();
    assert!(ownership.runtime_policy_retry.current().is_some());
    drop(path);
    assert!(ownership.runtime_policy_retry.current().is_none());
}

#[test]
fn independent_source_ack_away_back_recaptures_original_snapshot_for_automatic_service() {
    let mut path = prepared();
    path.publish(Some(wire(true, true, 2)), true, None);
    let original = automatic(&path);
    service(&mut path);
    let ownership = path.source.engine.input_runtime_ownership.clone();
    let acknowledgements = path.source.engine.current_timing_acknowledgements.clone();
    let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
    let before = scalar_state(&path);
    assert!(acknowledgements.acknowledge_for_key(&ownership, key, 7));
    let away = automatic(&path);
    assert!(!Arc::ptr_eq(&original, &away));
    service(&mut path);
    assert_eq!(
        away.observation().unwrap().reason,
        Some(RuntimePolicyError::SourceChanged)
    );
    assert!(acknowledgements.clear_for_key(&ownership, key));
    let back = automatic(&path);
    assert!(!Arc::ptr_eq(&away, &back));
    assert!(!Arc::ptr_eq(&original, &back));
    assert_eq!(back.revision(), original.revision() + 2);
    service(&mut path);
    unchanged(&path, &before);
    let refusal = back.observation();
    assert_eq!(
        refusal.unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    for _ in 0..16 {
        automatic(&path);
        path.render(0);
        assert_eq!(back.observation(), refusal);
        assert!(path.source.consumer.is_empty());
    }
}

#[test]
fn missing_master_keeps_original_retry_and_recovers_without_another_policy_publication() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    path.timeline.clear_master_bpm();
    let snapshot = automatic(&path);
    service(&mut path);
    let first = snapshot.observation().unwrap();
    assert_eq!(first.reason, Some(RuntimePolicyError::MissingMaster));
    for attempt in 1..=3 {
        assert!(Arc::ptr_eq(&snapshot, &automatic(&path)));
        assert_eq!(
            snapshot.observation().unwrap().attempt,
            first.attempt + attempt
        );
        service(&mut path);
        assert_eq!(
            snapshot.observation().unwrap().reason,
            Some(RuntimePolicyError::MissingMaster)
        );
    }
    assert!(path.timeline.set_master_period(MASTER));
    automatic(&path);
    service(&mut path);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
}
