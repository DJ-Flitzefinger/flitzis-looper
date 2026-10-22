use super::*;

fn mixed_cohort(prepaused: bool) -> (LaunchPath, OfflineAnalysisJob, CapturedPadStart, usize) {
    let mut path = mapped_fixed_pitch_birth_tests::configured(0);
    // Capture the existing scalar source admission before the mapped birth.
    // Ordinary productive starts replace one active same-content voice; this
    // bounded cohort fixture uses the same admitted VoiceStartConfig primitive
    // as the established Hold tests and fabricates no mapped map/source owner.
    let config = path
        .source
        .mixer
        .prepare_play_sample_rt(0, 0.25, 0, &mut ImmediateAudioBufferRetirement)
        .unwrap();
    let (job, delivery) = variable_original_delivery(&path);
    publish_original(&path, &delivery, 1);
    let captured = path.start();
    let scalar = path
        .source
        .mixer
        .voices
        .iter()
        .position(|voice| !voice.active)
        .unwrap();
    path.source.mixer.voices[scalar].start_rt(config, &mut ImmediateAudioBufferRetirement);
    assert_eq!(
        path.source.mixer.voices[scalar]
            .source_admission
            .as_ref()
            .unwrap()
            .content_key(),
        Some(captured.content_key())
    );
    assert!(!path.source.mixer.voices[scalar].source_playback.is_mapped());
    if prepaused {
        path.source.mixer.voices[scalar].pause();
    }
    (path, job, captured, scalar)
}

#[test]
fn actual_mixed_hold_releases_only_changed_mapped_lane_and_preserves_prepaused_scalar_fences() {
    let (mut path, _job, _captured, scalar) = mixed_cohort(true);
    path.render(7);
    let mapped = lane(&path);
    let scalar_before = path.source.mixer.voices[scalar].source_playback;
    let scalar_revision = path.source.mixer.voices[scalar]
        .source_feed_history
        .continuity_revision();
    let mapped_before = path.source.mixer.voices[mapped].source_playback;
    let ticket = pause(&path, 1);
    path.drain();
    let effect = ticket.action.effect().unwrap();
    assert_eq!(effect.cohort_mask, (1u32 << mapped) | (1u32 << scalar));
    assert_eq!(effect.changed_mask, 1u32 << mapped);
    assert!(path.source.mixer.voices[mapped].paused);
    assert!(path.source.mixer.voices[scalar].paused);
    assert!(path.render(113).iter().all(|sample| *sample == 0.0));
    release(&path, &ticket);
    path.drain();
    assert_eq!(ticket.action.state(), HoldActionState::Resumed);
    assert!(!path.source.mixer.voices[mapped].paused);
    exact(
        path.source.mixer.voices[mapped].source_playback.position(),
        mapped_before.position(),
    );
    assert!(path.source.mixer.voices[scalar].paused);
    assert!(
        path.source.mixer.voices[scalar]
            .source_playback
            .matches_exact(&scalar_before)
    );
    assert_eq!(
        path.source.mixer.voices[scalar]
            .source_feed_history
            .continuity_revision(),
        scalar_revision
    );
}

#[test]
fn actual_mixed_hold_one_unavailable_mapped_participant_refuses_all_changed_lanes() {
    let (mut path, _job, _captured, scalar) = mixed_cohort(false);
    path.render(7);
    let mapped = lane(&path);
    let before = [
        path.source.mixer.voices[mapped].source_playback,
        path.source.mixer.voices[scalar].source_playback,
    ];
    let revisions = [
        path.source.mixer.voices[mapped]
            .source_feed_history
            .continuity_revision(),
        path.source.mixer.voices[scalar]
            .source_feed_history
            .continuity_revision(),
    ];
    assert!(
        path.timeline
            .set_master_period_preserving_beat_position_at_frame(PERIOD / 1.16, 7)
    );
    let ticket = pause(&path, 1);
    path.drain();
    assert_eq!(ticket.action.state(), HoldActionState::Rejected);
    assert!(ticket.action.effect().is_none());
    for (offset, index) in [mapped, scalar].into_iter().enumerate() {
        assert!(!path.source.mixer.voices[index].paused);
        assert!(
            path.source.mixer.voices[index]
                .source_playback
                .matches_exact(&before[offset])
        );
        assert_eq!(
            path.source.mixer.voices[index]
                .source_feed_history
                .continuity_revision(),
            revisions[offset]
        );
    }
}

#[test]
fn actual_mapped_hold_preflight_refuses_stale_epoch_owner_cursor_source_or_ack_atomically() {
    for case in 0..5 {
        let (mut path, _job, captured) = qualified();
        path.render(7);
        let index = lane(&path);
        match case {
            0 => {
                path.source.mixer.voices[index].mapped_start = None;
            }
            1 => {
                assert!(
                    path.timeline
                        .set_master_period_preserving_beat_position_at_frame(PERIOD / 1.16, 7)
                );
            }
            2 => {
                path.source.mixer.voices[index]
                    .source_playback
                    .project_mapped_to(captured.mapped_geometry().unwrap(), 8)
                    .unwrap();
            }
            3 => {
                let source = path.source.mixer.voices[index]
                    .sample
                    .as_ref()
                    .unwrap()
                    .clone();
                let mut replacement = source;
                replacement.samples = Arc::from(vec![0.0; replacement.samples.len()]);
                path.source.mixer.voices[index].sample = Some(replacement);
            }
            _ => {
                assert!(
                    path.source
                        .engine
                        .current_timing_acknowledgements
                        .acknowledge_for_key(
                            &path.source.engine.input_runtime_ownership,
                            captured.content_key(),
                            1,
                        )
                );
            }
        }
        let before = path.voice().source_playback;
        let revision = path.voice().source_feed_history.continuity_revision();
        let history = snapshot(&path);
        let ticket = pause(&path, case + 1);
        path.drain();
        assert_eq!(
            ticket.action.state(),
            HoldActionState::Rejected,
            "case={case}"
        );
        assert!(ticket.action.effect().is_none());
        assert!(!path.voice().paused);
        assert!(path.voice().source_playback.matches_exact(&before));
        assert_eq!(
            path.voice().source_feed_history.continuity_revision(),
            revision
        );
        same_spans(&history, &snapshot(&path));
    }
}

#[test]
fn actual_mapped_release_refuses_changed_master_or_terminal_output_without_partial_resume() {
    for terminal_output in [false, true] {
        let (mut path, _job, _captured) = qualified();
        path.render(7);
        let ticket = pause(&path, 1);
        path.drain();
        path.render(11);
        if terminal_output {
            path.timeline.advance_by_rendered_frames(usize::MAX);
            assert_eq!(path.timeline.output_frame(), u64::MAX);
        } else {
            assert!(
                path.timeline
                    .set_master_period_preserving_beat_position_at_frame(PERIOD / 1.16, 18)
            );
        }
        let before = path.voice().source_playback;
        let revision = path.voice().source_feed_history.continuity_revision();
        let history = snapshot(&path);
        release(&path, &ticket);
        path.drain();
        assert_eq!(ticket.action.state(), HoldActionState::Rejected);
        assert!(path.voice().paused);
        assert!(path.voice().source_playback.matches_exact(&before));
        assert_eq!(
            path.voice().source_feed_history.continuity_revision(),
            revision
        );
        same_spans(&history, &snapshot(&path));
    }
}

#[test]
fn ordinary_transport_stays_closed_and_newer_seek_cannot_release_a_mapped_hold() {
    let (mut path, _job, _captured) = qualified();
    path.render(7);
    let before = path.voice().source_playback;
    let revision = path.voice().source_feed_history.continuity_revision();
    path.source.engine.pause_sample(0).unwrap();
    path.drain();
    assert!(!path.voice().paused);
    assert!(path.voice().source_playback.matches_exact(&before));
    assert_eq!(
        path.voice().source_feed_history.continuity_revision(),
        revision
    );
    let ticket = pause(&path, 1);
    path.drain();
    path.render(11);
    let before = path.voice().source_playback;
    let revision = path.voice().source_feed_history.continuity_revision();
    path.source
        .engine
        .seek_sample(0, 40.0 / f64::from(RATE))
        .unwrap();
    path.drain();
    release(&path, &ticket);
    path.drain();
    assert_eq!(ticket.action.state(), HoldActionState::Superseded);
    assert!(path.voice().paused);
    assert!(path.voice().source_playback.matches_exact(&before));
    assert_eq!(
        path.voice().source_feed_history.continuity_revision(),
        revision
    );
}

#[test]
fn actual_mapped_hold_stop_or_retrigger_cannot_resume_replacement_generation() {
    for retrigger in [false, true] {
        let (mut path, _job, _captured) = qualified();
        path.render(7);
        let generation = path.voice().generation;
        let ticket = pause(&path, 1);
        path.drain();
        path.render(11);
        if retrigger {
            let next = path.capture();
            path.publish_capture(&next);
            assert_eq!(next.state(), "applied", "{:?}", next.reason());
            assert_ne!(path.voice().generation, generation);
            assert!(!path.voice().paused);
        } else {
            path.source.engine.stop_sample(0).unwrap();
            path.drain();
            assert!(
                !path
                    .source
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(0))
            );
        }
        release(&path, &ticket);
        path.drain();
        assert!(ticket.action.state().terminal());
        assert_ne!(ticket.action.state(), HoldActionState::Resumed);
        if retrigger {
            assert!(!path.voice().paused);
        }
    }
}

#[test]
fn actual_mapped_hold_after_move_uses_original_content_and_keeps_scalar_neighbor_running() {
    let (mut path, _job, captured) = qualified();
    path.render(7);
    // PAD1 retains its independent real original loaded source and scalar path.
    assert!(path.source.mixer.play_sample(1, 0.25));
    let other = path
        .source
        .mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(1))
        .unwrap();
    let other_clock = path.source.mixer.voices[other].source_playback;
    let ticket = pause(&path, 1);
    path.drain();
    assert!(path.voice().paused);
    assert!(!path.source.mixer.voices[other].paused);
    assert!(
        path.source.mixer.voices[other]
            .source_playback
            .matches_exact(&other_clock)
    );
    let index = lane(&path);
    let saved = path.voice().source_playback;
    path.render(11);
    assert!(
        !path.source.mixer.voices[other]
            .source_playback
            .matches_exact(&other_clock)
    );
    let pair = path.source.engine._capture_pad_pair(0, 2).unwrap();
    let pair = path.source.engine._prepare_pad_pair(&pair).unwrap();
    path.source.engine._publish_pad_pair(&pair).unwrap();
    path.drain();
    assert_eq!(pair.publication_status(), "accepted");
    // Native placement is effective, but control display remains pending until
    // the real Pair acknowledgement has been reconciled and finished.
    assert_eq!(ticket.current_control_slot(), None);
    let acknowledgement = pair.own_acknowledgement().unwrap();
    assert!(path.source.engine._reconcile_pad_pair(&pair).unwrap());
    assert!(
        path.source
            .engine
            ._validate_pad_pair_acknowledgement(&pair, &acknowledgement)
            .unwrap()
    );
    path.source
        .engine
        ._finish_pad_pair(&pair, &acknowledgement)
        .unwrap();
    assert_eq!(ticket.current_control_slot(), Some(2));
    assert!(path.source.mixer.voices[index].paused);
    release(&path, &ticket);
    path.drain();
    assert_eq!(ticket.action.state(), HoldActionState::Resumed);
    let voice = &path.source.mixer.voices[index];
    assert_eq!(voice.sample_id, 2);
    assert!(!voice.paused);
    exact(voice.source_playback.position(), saved.position());
    assert!(voice.mapped_start.as_ref().unwrap().same_owner(&captured));
    assert!(!path.source.mixer.voices[other].paused);
}
