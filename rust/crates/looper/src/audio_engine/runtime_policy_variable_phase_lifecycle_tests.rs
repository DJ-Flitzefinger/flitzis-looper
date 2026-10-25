//! Neighbor interactions on the actual copied variable phase and original owners.
use super::*;
use crate::audio_engine::source_reader::SourceReaderPeriod;

fn verify_first_read(fixture: &VariablePath, id: usize, raw: FractionalSourcePosition) {
    let voice = fixture.dry.path.voice(id);
    let mut history = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut history));
    let span = history.spans().last().unwrap();
    assert_eq!(span.frames, 1);
    assert_position(span.playback.position(), raw);
    let sample = voice.sample.as_ref().unwrap();
    let taps = span.plan.try_fractional_taps(raw.fraction).unwrap();
    let mut expected = [0.; 2];
    for channel in 0..2 {
        let a = sample.samples[(taps.left - sample.resident_start()) * 2 + channel];
        expected[channel] = taps.right.map_or(a, |right| {
            let b = sample.samples[(right - sample.resident_start()) * 2 + channel];
            a + (b - a) * taps.right_gain
        });
    }
    for checked in [false, true] {
        let mut actual = [vec![17.; 1], vec![19.; 1]];
        if checked {
            span.plan.fill_fractional_buffers_checked_with_readers(
                sample,
                StemReadSets::default(),
                &span.playback,
                &mut actual,
                1,
            )
        } else {
            span.plan.fill_fractional_buffers_with_readers(
                sample,
                StemReadSets::default(),
                &span.playback,
                &mut actual,
                1,
            )
        }
        .unwrap();
        assert_eq!(
            (actual[0][0].to_bits(), actual[1][0].to_bits()),
            (expected[0].to_bits(), expected[1].to_bits())
        );
    }
}

#[test]
fn full_fraction_real_pause_resume_and_continuous_master_change_keep_first_pair_and_pcm() {
    let mut fixture = VariablePath::new(
        &[(&[(0., 0.), (0.37, 1.), (0.91, 2.)], 43680, Some(2.))],
        24000.,
        12000,
    );
    fixture.install();
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .mapped_epoch()
            .unwrap()
            .has_full_fraction_recovery()
    );
    for pass in 0..3 {
        fixture.dry.path.render(17);
        let raw = fixture.dry.path.voice(0).source_playback.position();
        assert_ne!(raw.fraction.to_bits(), 0);
        fixture.dry.path.source.engine.pause_sample(0).unwrap();
        fixture.drain();
        assert!(fixture.dry.path.voice(0).paused);
        let frozen = fixture.dry.path.voice(0).source_playback;
        fixture.dry.path.render(29);
        assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
        let live = fixture.dry.path.timeline.output_frame();
        fixture.dry.path.source.engine.resume_sample(0).unwrap();
        fixture.drain();
        let ordinary = fixture.dry.path.voice(0).source_playback;
        assert!(ordinary.mapped_epoch().unwrap().is_full_fraction_general());
        assert_eq!(ordinary.mapped_output_frame(), Some(live));
        assert_position(ordinary.position(), raw);
        assert_eq!(
            ordinary.original_scalar_domain(),
            frozen.original_scalar_domain()
        );
        fixture.dry.path.render(1);
        verify_first_read(&fixture, 0, raw);
        if pass == 0 {
            // A genuine global tempo effect uses a continuous master anchor;
            // it must preserve the newly rounded pair in ordinary FF mapping.
            fixture.dry.path.render(11);
            let raw = fixture.dry.path.voice(0).source_playback.position();
            fixture
                .dry
                .path
                .source
                .engine
                .set_speed_and_master_period(1., 25000. / f64::from(RATE))
                .unwrap();
            fixture.drain();
            fixture.dry.path.render(0);
            assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
            assert!(
                fixture
                    .dry
                    .path
                    .voice(0)
                    .source_playback
                    .mapped_epoch()
                    .unwrap()
                    .is_full_fraction_general()
            );
            assert_eq!(
                fixture
                    .dry
                    .path
                    .timeline
                    .master_period_seconds()
                    .unwrap()
                    .to_bits(),
                (25000. / f64::from(RATE)).to_bits()
            );
            fixture.dry.path.render(1);
            verify_first_read(&fixture, 0, raw);
        }
        fixture.effect();
        assert!(fixture.dry.path.voice(0).source_playback.is_mapped_phase());
        assert!(fixture.dry.path.feedback(0)["grid_phase_confirmed"].is_null());
    }
}

#[test]
fn variable_cancel_after_actual_phase_keeps_complete_snapshot_pin_and_pcm_without_requeue() {
    let mut fixture = VariablePath::small(2, 23);
    let mut reference = VariablePath::small(2, 23);
    let (snapshot, _) = fixture.install();
    reference.install();
    let source = fixture
        .dry
        .path
        .voice(0)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .clone();
    let playback = fixture.dry.path.voice(0).source_playback;
    assert!(playback.is_mapped_phase());
    fixture.dry.jobs[0].cancel();
    assert!(!snapshot.map_current(0));
    let observed = snapshot.observation();
    for _ in 0..64 {
        assert!(Arc::ptr_eq(&snapshot, &automatic_mapped(&fixture.dry.path)));
        assert_eq!(snapshot.observation(), observed);
        assert!(fixture.dry.path.source.consumer.is_empty());
        assert!(
            fixture
                .dry
                .path
                .voice(0)
                .source_playback
                .matches_exact(&playback)
        );
    }
    assert!(Arc::ptr_eq(
        fixture
            .dry
            .path
            .voice(0)
            .mapped_start
            .as_ref()
            .unwrap()
            .effective_source()
            .unwrap(),
        &source
    ));
    assert_eq!(
        fixture.dry.path.render(1537),
        reference.dry.path.render(1537)
    );
    assert!(fixture.dry.path.feedback(0)["grid_phase_confirmed"].is_null());
}

#[test]
fn variable_pause_noeffect_real_hold_and_live_resume_preserve_fine_cursor_then_requalify() {
    let mut fixture = VariablePath::small(1, 23);
    fixture.install();
    fixture.dry.path.render(4_400);
    let raw = fixture.dry.path.voice(0).source_playback.position();
    assert_ne!(
        raw.fraction.to_bits(),
        0,
        "an emitted fine fraction must be recovered"
    );
    let pause = fixture.dry.path.source.engine.pause_sample(0).unwrap();
    fixture.drain();
    assert!(
        pause
            .current_observation()
            .unwrap()
            .unwrap()
            .ordinary_paused()
    );
    let paused = fixture.dry.path.voice(0).source_playback;
    fixture.dry.path.source.engine.pause_sample(0).unwrap();
    fixture.drain();
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .matches_exact(&paused)
    );
    fixture.dry.path.render(997);
    assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
    let resumed = fixture.dry.path.timeline.output_frame();
    fixture.dry.path.source.engine.resume_sample(0).unwrap();
    fixture.drain();
    assert!(!fixture.dry.path.voice(0).paused);
    assert!(!fixture.dry.path.voice(0).source_playback.is_mapped_phase());
    assert_eq!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .mapped_output_frame(),
        Some(resumed)
    );
    assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
    fixture.dry.path.render(1);
    verify_first_read(&fixture, 0, raw);
    fixture.effect();
    assert!(fixture.dry.path.voice(0).source_playback.is_mapped_phase());
    fixture.dry.path.render(19);
    let held_raw = fixture.dry.path.voice(0).source_playback.position();
    let ticket = Python::attach(|py| {
        fixture
            .dry
            .path
            .source
            .engine
            .pause_waveform_hold(0, 9551_u64.into_pyobject(py).unwrap().as_any(), None)
            .unwrap()
    });
    fixture.drain();
    assert!(ticket.is_current());
    fixture.dry.path.render(513);
    assert_position(
        fixture.dry.path.voice(0).source_playback.position(),
        held_raw,
    );
    assert!(
        fixture
            .dry
            .path
            .source
            .engine
            .release_waveform_hold(&ticket)
            .unwrap()
    );
    fixture.drain();
    assert_eq!(ticket.publication_status(), "resumed");
    assert_position(
        fixture.dry.path.voice(0).source_playback.position(),
        held_raw,
    );
    assert!(!fixture.dry.path.voice(0).source_playback.is_mapped_phase());
    fixture.dry.path.render(1);
    verify_first_read(&fixture, 0, held_raw);
}

#[test]
fn variable_real_master_effect_recovers_current_fraction_but_true_noop_retains_programme() {
    let mut fixture = VariablePath::small(2, 23);
    fixture.install();
    fixture.dry.path.render(4400);
    let clocks = scalar_state(&fixture.dry.path);
    fixture
        .dry
        .path
        .source
        .engine
        .set_speed_and_master_period(1., fixture.fpb / f64::from(RATE))
        .unwrap();
    fixture.drain();
    fixture.dry.path.render(0);
    unchanged(&fixture.dry.path, &clocks);
    let before = scalar_state(&fixture.dry.path);
    let timeline = fixture.dry.path.timeline;
    let ownership = fixture
        .dry
        .path
        .source
        .engine
        .input_runtime_ownership
        .clone();
    let rate_before = ownership.global_rate_status.read().unwrap();
    let timing_revision = ownership
        .global_timing_revision
        .load(std::sync::atomic::Ordering::Acquire);
    let custody = (0..2)
        .map(|id| {
            let voice = fixture.dry.path.voice(id);
            let owner = voice.mapped_start.as_ref().unwrap();
            (
                owner.effective_source().unwrap().clone(),
                owner.original_launch().cloned(),
                voice.phase_policy_revision,
                voice.sample.as_ref().unwrap().clone(),
            )
        })
        .collect::<Vec<_>>();
    // The saturated .5/2 geometry honestly refuses a changed tempo outside its
    // whole-map bounds; no phase programme or original source may partly change.
    fixture
        .dry
        .path
        .source
        .engine
        .set_speed_and_master_period(1.25, fixture.fpb * 1.25 / f64::from(RATE))
        .unwrap();
    // Request admission advances the original publication fence even when its
    // later audio effect is refused. Preserve that admitted request truthfully.
    let admitted_revision = ownership
        .global_timing_revision
        .load(std::sync::atomic::Ordering::Acquire);
    assert_eq!(admitted_revision, timing_revision.checked_add(1).unwrap());
    fixture.drain();
    fixture.dry.path.render(0);
    unchanged(&fixture.dry.path, &before);
    assert!(fixture.dry.path.timeline.matches_clock_epoch(&timeline));
    assert_eq!(
        fixture.dry.path.timeline.output_frame(),
        timeline.output_frame()
    );
    assert_eq!(
        ownership
            .global_timing_revision
            .load(std::sync::atomic::Ordering::Acquire),
        admitted_revision
    );
    for (id, (source, original, phase_revision, sample)) in custody.iter().enumerate() {
        let voice = fixture.dry.path.voice(id);
        let owner = voice.mapped_start.as_ref().unwrap();
        assert!(voice.source_playback.is_mapped_phase());
        assert!(Arc::ptr_eq(owner.effective_source().unwrap(), source));
        match (owner.original_launch(), original.as_ref()) {
            (None, None) => (),
            (Some(actual), Some(expected)) => assert!(actual.same_owner(expected)),
            _ => panic!("a rejected master effect changed genuine launch custody"),
        }
        assert_eq!(voice.phase_policy_revision, *phase_revision);
        assert!(voice.sample.as_ref().unwrap().same_window(sample));
    }
    let rejected = ownership.global_rate_status.read().unwrap();
    assert_eq!(
        rejected.effect.state,
        crate::audio_engine::global_rate_status::RateState::Rejected
    );
    assert_eq!(
        rejected.effect.error,
        crate::audio_engine::global_rate_status::RateError::MappedTimingUnavailable
    );
    assert_eq!(
        rejected.effect.requested.speed.to_bits(),
        1.25_f64.to_bits()
    );
    assert_eq!(
        rejected.effect.requested.period.map(f64::to_bits),
        Some((fixture.fpb * 1.25 / f64::from(RATE)).to_bits())
    );
    assert_eq!(rejected.effect.effective, rate_before.effect.effective);
    assert_eq!(rejected.master_provenance, rate_before.master_provenance);
}

#[test]
fn variable_actual_general_physical_and_declared_routes_keep_genuine_birth_domains() {
    for physical in [true, false] {
        let extended = &[(0., 0.), (2., 8.), (10., 24.), (18., 32.), (19., 33.)][..];
        let shape = if physical {
            (extended, 864004, None)
        } else {
            (extended, 864004, Some(32.))
        };
        let mut fixture =
            VariablePath::with_details(&[shape], 24000., 13, &[1], !physical, !physical);
        let birth = fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .original_scalar_domain()
            .unwrap();
        if physical {
            assert_eq!(birth.musical_period, SourceReaderPeriod::Physical);
        }
        // Exact B(A)=1/12000 is non-binary64, so original exact-endpoint
        // qualification must refuse compact preparation. Physical Q remains
        // exactly (32+1/12000)-1/12000=32 through the original inverse.
        fixture.publish(Some(wire(true, true, 2)), true, true, None, None);
        let mode = request(&fixture.dry.path);
        assert_eq!(mode.map_preparation(0).class, RuntimeMapClass::Ready);
        let prepared = mode.prepared_map(0).unwrap().geometry();
        assert!(prepared.has_general_continuation());
        assert!(!prepared.has_compact_continuation());
        assert!(prepared.has_common_phase());
        assert_eq!(
            prepared.domain().region,
            FrameRange {
                start: 1,
                end: 864004
            }
        );
        fixture.drain();
        fixture.dry.path.render(0);
        assert_eq!(
            mode.observation().unwrap().state,
            "applied",
            "{:?}",
            mode.observation()
        );
        assert!(
            fixture
                .dry
                .path
                .voice(0)
                .source_playback
                .is_general_mapped(),
            "the actual preparation must choose the genuine general route"
        );
        assert!(mode.prepared_map(0).unwrap().geometry().has_common_phase());
        if physical {
            assert!(matches!(
                fixture.dry.path.voice(0).source_playback.reader_period(),
                SourceReaderPeriod::MappedPhysicalQuarters(_)
            ));
        }
        let raw = fixture.dry.path.voice(0).source_playback.position();
        let anchor = OracleAnchor::capture(&fixture, 0);
        let phase = fixture.effect();
        assert!(Arc::ptr_eq(&mode, &phase));
        assert_eq!(
            phase.observation().unwrap().state,
            "applied",
            "{:?}",
            phase.observation()
        );
        assert!(fixture.dry.path.voice(0).source_playback.is_mapped_phase());
        assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
        assert_eq!(
            fixture
                .dry
                .path
                .voice(0)
                .source_playback
                .original_scalar_domain(),
            Some(birth)
        );
        fixture.dry.path.render(257);
        assert_eq!(verify_reads(&fixture, 0, anchor), 257);
    }
}

#[test]
fn variable_parent_and_intent_aba_stales_pending_birth_and_preserves_actual_cursor() {
    let mut fixture = VariablePath::small(1, 23);
    fixture.install();
    fixture.dry.path.render(67);
    let raw = fixture.dry.path.voice(0).source_playback.position();
    let future = fixture
        .dry
        .path
        .source
        .engine
        .capture_pad_start(
            0,
            0.,
            Some(fixture.ends[0] as f64 / f64::from(RATE)),
            1.,
            false,
            None,
            None,
        )
        .unwrap();
    fixture.drain();
    fixture.dry.intent_revision = 8;
    fixture.publish(Some(wire(false, false, 3)), true, true, None, None);
    assert_eq!(fixture.effect().observation().unwrap().state, "applied");
    assert!(!fixture.dry.path.voice(0).source_playback.is_mapped());
    assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
    fixture.dry.intent_revision = 9;
    fixture.publish(Some(wire(true, true, 4)), true, true, None, None);
    assert!(
        fixture
            .dry
            .path
            .source
            .engine
            .publish_pad_start(&future, None)
            .is_err()
    );
    let mode = fixture.effect();
    assert_eq!(mode.observation().unwrap().state, "applied");
    assert!(!fixture.dry.path.voice(0).source_playback.is_mapped_phase());
    assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
    fixture.dry.path.render(1);
    fixture.effect();
    assert!(fixture.dry.path.voice(0).source_playback.is_mapped_phase());
}

#[test]
fn variable_competing_real_global_lease_defers_original_phase_without_partial_effects() {
    let (mut fixture, snapshot) = pending_phase(2);
    fixture.drain();
    let before = scalar_state(&fixture.dry.path);
    let ownership = fixture
        .dry
        .path
        .source
        .engine
        .input_runtime_ownership
        .clone();
    let lease =
        crate::audio_engine::GlobalTimingLease::acquire(&ownership.global_timing_publishing)
            .unwrap();
    fixture.dry.path.render(0);
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    unchanged(&fixture.dry.path, &before);
    let ScheduledCommand::AdoptRuntimePolicy(work) =
        &fixture.dry.path.scheduler.deferred_due().unwrap().command
    else {
        panic!("the original phase work must be deferred")
    };
    assert!(std::ptr::eq(&**work, snapshot.as_ref()));
    drop(lease);
    fixture.dry.path.render(1);
    let preceding = scalar_state(&fixture.dry.path);
    fixture.drain();
    fixture.dry.path.render(0);
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    for (id, _, old, _) in preceding {
        assert!(fixture.dry.path.voice(id).source_playback.is_mapped_phase());
        assert_position(
            fixture.dry.path.voice(id).source_playback.position(),
            old.position(),
        );
    }
}

#[test]
fn variable_cancelled_map_at_final_phase_claim_preserves_the_whole_mapped_cohort() {
    let (mut fixture, snapshot) = pending_phase(2);
    fixture.drain();
    let before = scalar_state(&fixture.dry.path);
    let job = fixture.dry.jobs.remove(0);
    crate::audio_engine::mixer::RtMixer::with_runtime_policy_before_claim_for_test(
        move || job.cancel(),
        || fixture.dry.path.render(0),
    );
    unchanged(&fixture.dry.path, &before);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Stale)
    );
    assert!((0..2).all(|id| !fixture.dry.path.voice(id).source_playback.is_mapped_phase()));
}

#[test]
fn variable_full_and_checked_reader_fail_before_writing_with_incompatible_source_domain() {
    let mut fixture = VariablePath::small(1, 23);
    fixture.install();
    fixture.dry.path.render(31);
    let voice = fixture.dry.path.voice(0);
    let mut history = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut history));
    let span = history.spans().last().unwrap();
    assert!(span.playback.is_mapped_phase());
    let sample = voice.sample.as_ref().unwrap();
    let bad = SourceReadPlan {
        sample_frames: span.plan.sample_frames - 1,
        ..span.plan
    };
    for checked in [false, true] {
        let mut actual = [vec![17.; span.frames], vec![19.; span.frames]];
        let before = actual.clone();
        let result = if checked {
            bad.fill_fractional_buffers_checked_with_readers(
                sample,
                StemReadSets::default(),
                &span.playback,
                &mut actual,
                span.frames,
            )
        } else {
            bad.fill_fractional_buffers_with_readers(
                sample,
                StemReadSets::default(),
                &span.playback,
                &mut actual,
                span.frames,
            )
        };
        assert!(result.is_err());
        assert_eq!(actual, before);
    }
}

#[test]
fn variable_phase_even_after_numeric_capture_cannot_certify_constant_native_history() {
    let mut fixture = VariablePath::small(1, 23);
    fixture.install();
    fixture.dry.path.render(20_000);
    let voice = fixture.dry.path.voice(0);
    let geometry = voice
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .geometry();
    assert!(
        voice
            .source_playback
            .mapped_phase_settled(geometry)
            .unwrap()
    );
    assert!(!voice.source_playback.has_constant_scalar_trajectory());
    assert!(voice.source_playback.history_before_constant(17).is_none());
    assert!(fixture.dry.path.feedback(0)["grid_phase_confirmed"].is_null());
}

#[test]
fn variable_terminal_native_preferences_leave_real_off_escape_and_fresh_pitch_retrigger_progress() {
    let mut fixture = VariablePath::small(1, 23);
    fixture.install();
    fixture.dry.path.render(5000);
    let before = fixture.dry.path.voice(0).source_playback;
    let idle_native = fixture.dry.path.voice(0).stretch.native_state_address();
    assert!(
        !fixture
            .dry
            .path
            .voice(0)
            .stretch
            .has_active_native_ownership()
    );
    assert!(
        !fixture
            .dry
            .path
            .voice(0)
            .stretch
            .has_retained_source_input()
    );
    fixture
        .dry
        .path
        .source
        .engine
        .set_pad_key_lock(0, true)
        .unwrap();
    fixture.drain();
    let key_state = Python::attach(|py| {
        fixture
            .dry
            .path
            .source
            .engine
            .pad_key_lock_status(py, 0)
            .unwrap()
            .unwrap()
            .bind(py)
            .get_item("state")
            .unwrap()
            .unwrap()
            .extract::<String>()
            .unwrap()
    });
    assert_eq!(key_state, "error");
    let blocked = fixture.capture_pitch("pitch.extra:1:set_retrigger");
    assert!(
        fixture
            .dry
            .path
            .source
            .engine
            .publish_pitch_action(&blocked)
            .unwrap()
    );
    fixture.drain();
    fixture.dry.path.render(0);
    assert_eq!(blocked.state(), "rejected");
    assert_eq!(blocked.reason(), Some("mapped_timing_unavailable"));
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .matches_exact(&before)
    );
    assert_eq!(
        fixture.dry.path.voice(0).stretch.native_state_address(),
        idle_native
    );
    assert!(
        !fixture
            .dry
            .path
            .voice(0)
            .stretch
            .has_active_native_ownership()
    );
    assert!(
        !fixture
            .dry
            .path
            .voice(0)
            .stretch
            .has_retained_source_input()
    );
    // Only terminal REQUESTED preferences are ignored for this real OFF effect;
    // the actual Native actor remains absent throughout the conversion.
    fixture.publish(Some(wire(false, false, 3)), true, true, None, Some(0));
    let off = fixture.effect();
    assert_eq!(
        off.observation().unwrap().state,
        "applied",
        "{:?}",
        off.observation()
    );
    assert!(!fixture.dry.path.voice(0).source_playback.is_mapped());
    assert_position(
        fixture.dry.path.voice(0).source_playback.position(),
        before.position(),
    );
    assert_eq!(
        fixture.dry.path.voice(0).stretch.native_state_address(),
        idle_native
    );
    assert!(
        !fixture
            .dry
            .path
            .voice(0)
            .stretch
            .has_active_native_ownership()
    );
    assert!(
        !fixture
            .dry
            .path
            .voice(0)
            .stretch
            .has_retained_source_input()
    );
    fixture
        .dry
        .path
        .source
        .engine
        .set_pad_key_lock(0, false)
        .unwrap();
    fixture.drain();
    fixture.publish(Some(wire(false, false, 4)), true, true, None, None);
    fixture.effect();
    fixture.dry.path.render(16_384);
    let action = fixture.capture_pitch("pitch.extra:1:set_retrigger");
    assert!(
        fixture
            .dry
            .path
            .source
            .engine
            .publish_pitch_action(&action)
            .unwrap()
    );
    fixture.drain();
    wait_for(|| {
        fixture
            .dry
            .path
            .source
            .mixer
            .pitch_runtime
            .as_mut()
            .unwrap()
            .poll();
        assert_ne!(action.state(), "rejected", "{:?}", action.reason());
        fixture
            .dry
            .path
            .source
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(action.sequence())
    });
    while fixture.dry.path.timeline.output_frame() < action.execution_target() {
        fixture.dry.path.render(
            (action.execution_target() - fixture.dry.path.timeline.output_frame()).min(512)
                as usize,
        );
    }
    fixture.dry.path.render(1);
    assert_eq!(action.state(), "applied", "{:?}", action.reason());
    assert!(action.effective());
    assert_ne!(fixture.dry.path.voice(0).stretch.native_state_address(), 0);
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .stretch
            .has_active_native_ownership()
    );
}

#[test]
fn variable_all_32_mixed_grid_full_fraction_claims_are_atomic_and_retry_after_last_map_repair() {
    let nonclosing = &[(0., 0.), (0.125, 31.5)][..];
    let full_fraction = &[(0., 0.), (0.001, 0.25), (9. / 64., 32.)][..];
    let mut shapes = vec![(SMALL_ANCHORS, 6750, Some(32.)); 32];
    for id in (1..31).step_by(2) {
        shapes[id] = (full_fraction, 6750, Some(32.));
    }
    shapes[31] = (nonclosing, 6000, Some(31.5));
    let accepted_quarters = vec![32.; 32];
    let mut fixture = VariablePath::with_accepted_quarters(
        &shapes,
        187.5,
        23,
        &vec![0; 32],
        true,
        true,
        Some(&accepted_quarters),
    );
    assert_eq!(
        fixture
            .dry
            .path
            .voice(31)
            .source_playback
            .original_scalar_domain()
            .unwrap()
            .musical_period
            .scalar_period(),
        Some(6000.)
    );
    fixture.publish(Some(wire(true, true, 2)), true, true, None, None);
    let mode = fixture.effect();
    assert_eq!(
        mode.observation().unwrap().state,
        "applied",
        "{:?}",
        mode.observation()
    );
    assert!(
        (0..32).all(|id| fixture.dry.path.voice(id).source_playback.is_mapped()
            && !fixture.dry.path.voice(id).source_playback.is_mapped_phase())
    );
    assert!((0..32).all(|id| mode.map_preparation(id).class == RuntimeMapClass::Ready));
    assert!((0..31).all(|id| mode.prepared_map(id).unwrap().geometry().has_common_phase()));
    assert!(
        !mode.prepared_map(31).unwrap().geometry().has_common_phase(),
        "the original noninteger Q has ordinary geometry but no quarter closure"
    );
    let before = scalar_state(&fixture.dry.path);
    let retained = (0..32)
        .map(|id| {
            let voice = fixture.dry.path.voice(id);
            let mut history = SourceFeedSnapshot::new();
            assert!(voice.source_feed_history.capture_into(&mut history));
            (
                voice
                    .mapped_start
                    .as_ref()
                    .unwrap()
                    .effective_source()
                    .unwrap()
                    .clone(),
                voice.sample.as_ref().unwrap().clone(),
                voice.phase_policy_revision,
                history,
            )
        })
        .collect::<Vec<_>>();
    assert!(fixture.dry.path.source.consumer.is_empty());
    assert!(fixture.dry.parameters.is_empty());
    let actual_e = fixture.dry.path.timeline.output_frame();
    let phase = fixture.effect();
    assert!(Arc::ptr_eq(&mode, &phase));
    assert_eq!(phase.observation().unwrap().state, "refused");
    assert_eq!(
        phase.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    assert_eq!(fixture.dry.path.timeline.output_frame(), actual_e);
    assert!((0..32).all(|id| !fixture.dry.path.voice(id).source_playback.is_mapped_phase()));
    unchanged(&fixture.dry.path, &before);
    assert!(fixture.dry.path.source.consumer.is_empty());
    assert!(fixture.dry.parameters.is_empty());
    for (id, (source, sample, revision, old)) in retained.iter().enumerate() {
        let voice = fixture.dry.path.voice(id);
        assert!(Arc::ptr_eq(
            voice
                .mapped_start
                .as_ref()
                .unwrap()
                .effective_source()
                .unwrap(),
            source
        ));
        assert!(voice.sample.as_ref().unwrap().same_window(sample));
        assert_eq!(voice.phase_policy_revision, *revision);
        let mut actual = SourceFeedSnapshot::new();
        assert!(voice.source_feed_history.capture_into(&mut actual));
        assert_eq!(
            (
                actual.active_origin(),
                actual.active_frames(),
                actual.captured_epoch()
            ),
            (
                old.active_origin(),
                old.active_frames(),
                old.captured_epoch()
            )
        );
        assert!(Arc::ptr_eq(
            actual.continuity_owner().unwrap(),
            old.continuity_owner().unwrap()
        ));
        assert_eq!(actual.spans().count(), old.spans().count());
        for (a, b) in actual.spans().zip(old.spans()) {
            assert_eq!((a.active_start, a.frames), (b.active_start, b.frames));
            assert!(a.playback.matches_exact(&b.playback));
            assert!(a.plan.matches_exact(b.plan));
            assert!(a.context.same_source(b.context));
        }
    }
    // Repair the genuine last map through its existing publication/ACK route;
    // the original accepted scalar timing, T and source custody stay retained.
    let repaired = &[(0., 0.), (0.125, 32.)][..];
    let (job, delivery) = fixture.dry.map_anchors(31, repaired);
    fixture.dry.jobs.push(job);
    fixture.dry.deliveries[31] = delivery;
    fixture.anchors[31] = repaired.to_vec();
    fixture.quarters[31] = Some(32.);
    fixture.publish(Some(wire(true, true, 3)), true, true, None, None);
    let repaired_mode = fixture.effect();
    assert_eq!(repaired_mode.observation().unwrap().state, "applied");
    assert!((0..32).all(|id| !fixture.dry.path.voice(id).source_playback.is_mapped_phase()));
    assert!((0..32).all(|id| repaired_mode.map_preparation(id).class == RuntimeMapClass::Ready));
    let ordinary = scalar_state(&fixture.dry.path);
    let retry = fixture.effect();
    assert!(Arc::ptr_eq(&repaired_mode, &retry));
    assert_eq!(
        retry.observation().unwrap().state,
        "applied",
        "{:?}",
        retry.observation()
    );
    for (id, generation, prior, _) in &ordinary {
        let voice = fixture.dry.path.voice(*id);
        assert_eq!(voice.generation, *generation);
        assert!(voice.source_playback.is_mapped_phase());
        assert_position(voice.source_playback.position(), prior.position());
        assert_eq!(
            voice.source_playback.original_scalar_domain(),
            prior.original_scalar_domain()
        );
        assert_eq!(
            voice
                .source_playback
                .mapped_epoch()
                .unwrap()
                .has_full_fraction_recovery(),
            *id < 31 && id % 2 == 1
        );
        assert!(fixture.dry.path.feedback(*id)["grid_phase_confirmed"].is_null());
    }
}

#[test]
fn variable_real_general_to_compact_b_c_preserves_distinct_birth_period_through_phase_recovery_and_off()
 {
    let general = &[(0., 0.), (0.001, 0.25), (9. / 64., 32.)][..];
    let b = &[
        (0., 0.),
        (1. / 32., 8.),
        (5. / 64., 24.),
        (9. / 64. + 1. / 262144., 32.),
    ][..];
    let c = &[
        (0., 0.),
        (1. / 32., 8.),
        (5. / 64., 24.),
        (9. / 64. + 1. / 131072., 32.),
    ][..];
    let mut fixture = VariablePath::new(&[(general, 6750, Some(32.))], 200., 23);
    let birth = fixture
        .dry
        .path
        .voice(0)
        .source_playback
        .original_scalar_domain()
        .unwrap();
    assert_eq!(birth.musical_period.scalar_period(), Some(6750.));
    fixture.publish(Some(wire(true, true, 2)), true, true, None, None);
    let first = fixture.effect();
    assert_eq!(
        first.observation().unwrap().state,
        "applied",
        "{:?}",
        first.observation()
    );
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .is_general_mapped()
    );
    for (anchors, revision) in [(b, 3), (c, 4)] {
        fixture.dry.path.render(17);
        let raw = fixture.dry.path.voice(0).source_playback.position();
        let (job, delivery) = fixture.dry.map_anchors(0, anchors);
        fixture.dry.jobs.push(job);
        fixture.dry.deliveries[0] = delivery;
        fixture.anchors[0] = anchors.to_vec();
        fixture.publish(Some(wire(true, true, revision)), true, true, None, None);
        let replaced = fixture.effect();
        assert_eq!(replaced.map_preparation(0).class, RuntimeMapClass::Ready);
        assert_eq!(
            replaced.map_preparation(0).domain.unwrap().region,
            FrameRange {
                start: 0,
                end: 6750
            }
        );
        assert_eq!(
            replaced.observation().unwrap().state,
            "applied",
            "{:?}",
            replaced.observation()
        );
        let current = fixture.dry.path.voice(0).source_playback;
        assert!(current.is_compact_mapped());
        assert!(!current.is_general_mapped());
        assert_position(current.position(), raw);
        assert_eq!(current.original_scalar_domain(), Some(birth));
        assert_ne!(current.reader_period(), birth.musical_period);
        let geometry = replaced.prepared_map(0).unwrap().geometry();
        assert!(
            SourcePlayback::mapped_anchored_compact_continuation(
                geometry,
                replaced.revision(),
                fixture.dry.path.timeline,
                birth,
                raw,
                fixture.dry.path.timeline.output_frame()
            )
            .is_err(),
            "pure scalar birth must not accept a retained historical P as current geometry proof"
        );
    }
    fixture.dry.path.render(129);
    let before = fixture.dry.path.voice(0).source_playback;
    let phase = fixture.effect();
    assert_eq!(
        phase.observation().unwrap().state,
        "applied",
        "{:?}",
        phase.observation()
    );
    assert!(fixture.dry.path.voice(0).source_playback.is_mapped_phase());
    assert_position(
        fixture.dry.path.voice(0).source_playback.position(),
        before.position(),
    );
    fixture.dry.path.render(73);
    let raw = fixture.dry.path.voice(0).source_playback.position();
    let pause = fixture.dry.path.source.engine.pause_sample(0).unwrap();
    fixture.drain();
    assert!(
        pause
            .current_observation()
            .unwrap()
            .unwrap()
            .ordinary_paused()
    );
    assert!(fixture.dry.path.voice(0).paused);
    fixture.dry.path.render(997);
    fixture.dry.path.source.engine.resume_sample(0).unwrap();
    fixture.drain();
    assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
    assert_eq!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .original_scalar_domain(),
        Some(birth)
    );
    assert!(!fixture.dry.path.voice(0).source_playback.is_mapped_phase());
    fixture.dry.path.render(1);
    verify_first_read(&fixture, 0, raw);
    fixture.effect();
    assert!(fixture.dry.path.voice(0).source_playback.is_mapped_phase());
    let raw = fixture.dry.path.voice(0).source_playback.position();
    fixture
        .dry
        .path
        .source
        .engine
        .set_speed_and_master_period(1.05, 210. / f64::from(RATE))
        .unwrap();
    fixture.drain();
    fixture.dry.path.render(0);
    assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
    assert!(!fixture.dry.path.voice(0).source_playback.is_mapped_phase());
    assert_eq!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .original_scalar_domain(),
        Some(birth)
    );
    fixture.publish(Some(wire(false, false, 5)), true, true, None, None);
    let off = fixture.effect();
    assert_eq!(
        off.observation().unwrap().state,
        "applied",
        "{:?}",
        off.observation()
    );
    let scalar = fixture.dry.path.voice(0).source_playback;
    assert!(!scalar.is_mapped());
    assert_eq!(scalar.effective_domain(), Some(birth));
    assert_position(scalar.position(), raw);
    fixture.dry.path.render(1);
    verify_first_read(&fixture, 0, raw);
}

#[test]
fn variable_requested_only_off_escape_keeps_a_genuine_future_native_actor_as_whole_cohort_guard() {
    let mut fixture = VariablePath::small(2, 23);
    fixture.mode();
    fixture.publish(Some(wire(true, true, 3)), true, true, Some(1), None);
    assert_eq!(fixture.effect().observation().unwrap().state, "applied");
    fixture.dry.path.render(17_000);
    fixture.effect();
    assert!(fixture.dry.path.voice(0).source_playback.is_mapped_phase());
    assert!(!fixture.dry.path.voice(1).source_playback.is_mapped());
    fixture
        .dry
        .path
        .source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_pitch_runtime_state(
            (0..NUM_SAMPLES)
                .map(|id| PitchPadProjection {
                    sync: true,
                    key_lock: false,
                    quantize_step_64ths: (id == 1).then_some(16),
                    ..Default::default()
                })
                .collect(),
        )
        .unwrap();
    let pending = fixture.capture_pitch_for(1, "pitch.extra:0:set_retrigger");
    assert!(
        fixture
            .dry
            .path
            .source
            .engine
            .publish_pitch_action(&pending)
            .unwrap()
    );
    fixture.drain();
    assert!(pending.execution_target() > fixture.dry.path.timeline.output_frame());
    assert!(
        fixture
            .dry
            .path
            .source
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_pending_content(pending.content_key())
    );
    let before = scalar_state(&fixture.dry.path);
    fixture.publish(Some(wire(false, false, 4)), true, true, Some(1), None);
    let off = fixture.effect();
    assert!(matches!(
        off.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported | RuntimePolicyError::Busy)
    ));
    unchanged(&fixture.dry.path, &before);
    assert!(!pending.effective());
    assert!(
        fixture
            .dry
            .path
            .source
            .engine
            .cancel_pitch_action(&pending)
            .unwrap()
    );
    fixture.drain();
    assert!(
        !fixture
            .dry
            .path
            .source
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_pending_content(pending.content_key())
    );
}

#[test]
fn variable_phase_capable_but_ordinary_general_native_refusal_keeps_a_working_off_escape() {
    let general = &[(0., 0.), (0.001, 0.25), (9. / 64., 32.)][..];
    let mut fixture = VariablePath::new(&[(general, 6750, Some(32.))], 200., 23);
    fixture.publish(Some(wire(true, true, 2)), true, true, None, None);
    let mode = fixture.effect();
    assert_eq!(mode.observation().unwrap().state, "applied");
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .is_general_mapped()
    );
    assert!(mode.prepared_map(0).unwrap().geometry().has_common_phase());
    assert!(!fixture.dry.path.voice(0).source_playback.is_mapped_phase());
    fixture.dry.path.render(71);
    let before = fixture.dry.path.voice(0).source_playback;
    assert!(before.is_general_mapped());
    assert!(!before.is_mapped_phase());
    let source = fixture
        .dry
        .path
        .voice(0)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .clone();
    let raw = fixture.dry.path.voice(0).source_playback.position();
    fixture
        .dry
        .path
        .source
        .engine
        .set_pad_key_lock(0, true)
        .unwrap();
    fixture.drain();
    let blocked = fixture.capture_pitch("pitch.extra:1:set_retrigger");
    assert!(
        fixture
            .dry
            .path
            .source
            .engine
            .publish_pitch_action(&blocked)
            .unwrap()
    );
    fixture.drain();
    fixture.dry.path.render(0);
    assert_eq!(blocked.state(), "rejected");
    assert_eq!(blocked.reason(), Some("mapped_timing_unavailable"));
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .matches_exact(&before)
    );
    assert!(Arc::ptr_eq(
        fixture
            .dry
            .path
            .voice(0)
            .mapped_start
            .as_ref()
            .unwrap()
            .effective_source()
            .unwrap(),
        &source
    ));
    fixture.publish(Some(wire(false, false, 3)), true, true, None, Some(0));
    let off = fixture.effect();
    assert_eq!(
        off.observation().unwrap().state,
        "applied",
        "{:?}",
        off.observation()
    );
    assert!(!fixture.dry.path.voice(0).source_playback.is_mapped());
    assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
    fixture.dry.path.render(1);
    verify_first_read(&fixture, 0, raw);
    fixture
        .dry
        .path
        .source
        .engine
        .set_pad_key_lock(0, false)
        .unwrap();
    fixture.drain();
    fixture.publish(Some(wire(false, false, 4)), true, true, None, None);
    fixture.effect();
    let birth = fixture.start(0);
    assert_eq!(birth.state(), "applied");
    assert!(!fixture.dry.path.voice(0).source_playback.is_mapped());
}

#[test]
fn variable_zero_debt_and_captured_actual_checkpoints_never_enter_constant_control_epochs() {
    use crate::audio_engine::prepared_native_history::SourceReadCheckpoint;
    use crate::audio_engine::source_control_epoch::{SourceControlBirth, SourceControlEpoch};
    for zero_debt in [true, false] {
        let mut fixture = VariablePath::small(1, if zero_debt { 0 } else { 23 });
        if zero_debt {
            let mode = fixture.mode();
            assert!(Arc::ptr_eq(&mode, &fixture.effect()));
            let voice = fixture.dry.path.voice(0);
            assert!(voice.source_playback.is_mapped_phase());
            assert!(
                voice
                    .source_playback
                    .mapped_phase_settled(
                        voice
                            .mapped_start
                            .as_ref()
                            .unwrap()
                            .mapped_geometry()
                            .unwrap()
                    )
                    .unwrap()
            );
            fixture.dry.path.render(64);
        } else {
            fixture.install();
            fixture.dry.path.render(20_000);
        }
        let voice = fixture.dry.path.voice(0);
        let mut history = SourceFeedSnapshot::new();
        assert!(voice.source_feed_history.capture_into(&mut history));
        let actual = history.spans().last().unwrap();
        let actual = actual.slice(actual.frames - 1, 1).unwrap();
        let phase = SourceReadCheckpoint {
            playback: actual.playback,
            plan: actual.plan,
        };
        assert!(phase.is_consistent());
        assert!(phase.playback.is_mapped_phase());
        assert!(
            !phase.plan.loop_period.is_mapped(),
            "this compact ScalarMusical tag must reach the explicit phase guard"
        );
        assert!(
            phase
                .playback
                .mapped_phase_settled(
                    voice
                        .mapped_start
                        .as_ref()
                        .unwrap()
                        .mapped_geometry()
                        .unwrap()
                )
                .unwrap()
        );
        assert_eq!(
            phase.playback.tempo_ratio().to_bits(),
            phase.playback.rate_target().to_bits()
        );
        let frame = phase.playback.mapped_output_frame().unwrap();
        assert!(frame >= 32);
        let context = actual.context;
        let mut rejected = SourceControlEpoch::default();
        rejected.record(Some(frame), 32, phase, context, 7);
        assert!(!rejected.covers(frame + 1, frame + 32, phase, context, 7));
        rejected.record_paused(Some(frame + 32), 32, phase, context, 7);
        assert!(!rejected.covers_paused(frame + 33, frame + 64, phase, context, 7));

        // Coordinate-only positive controls use the SAME genuine source/context,
        // pair, domain and rate. They certify only this isolated epoch API test.
        let scalar = SourceReadCheckpoint {
            playback: SourcePlayback::scalar_musical_continuation(
                phase.playback.position(),
                phase.plan.domain(),
                phase.playback.tempo_ratio(),
                phase.playback.rate_target(),
            )
            .unwrap(),
            plan: phase.plan,
        };
        assert!(scalar.is_consistent());
        let mut paused = SourceControlEpoch::default();
        paused.record_paused(Some(frame), 32, scalar, context, 7);
        assert!(paused.covers_paused(frame + 1, frame + 32, scalar, context, 7));
        assert!(!paused.covers_paused(frame + 1, frame + 32, phase, context, 7));
        let mut rendered = SourceControlEpoch::default();
        rendered.record(Some(frame - 32), 32, scalar, context, 7);
        let mut projected = scalar;
        assert!(projected.project_output_frames(32));
        assert!(rendered.covers(frame - 1, frame, projected, context, 7));
        let key = fixture
            .dry
            .path
            .source
            .engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap()
            .content_key()
            .unwrap();
        let previous = SourceControlBirth {
            key,
            checkpoint: projected,
            context,
            revision: 7,
        };
        let next_scalar = SourceControlBirth {
            key,
            checkpoint: scalar,
            context,
            revision: 7,
        };
        assert!(
            rendered
                .continue_independent_birth(frame, previous, next_scalar)
                .is_some()
        );
        let next_phase = SourceControlBirth {
            key,
            checkpoint: phase,
            context,
            revision: 7,
        };
        assert!(
            rendered
                .continue_independent_birth(frame, previous, next_phase)
                .is_none()
        );
    }
}

#[test]
fn variable_valid_domain_late_resident_tap_failure_keeps_full_and_checked_buffers_untouched() {
    let mut fixture = VariablePath::small(1, 23);
    fixture.install();
    fixture.dry.path.render(31);
    let voice = fixture.dry.path.voice(0);
    let mut history = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut history));
    let span = history.spans().last().unwrap();
    assert!(span.playback.is_mapped_phase());
    assert!(span.plan.domain().is_valid());
    assert!(span.playback.matches_domain(span.plan.domain()));
    assert!(!span.plan.loop_period.is_mapped());
    assert!(span.frames >= 16);
    let source = voice.sample.as_ref().unwrap();
    let first = span.playback.position();
    let first_taps = span.plan.try_fractional_taps(first.fraction).unwrap();
    let low = first_taps
        .left
        .min(first_taps.right.unwrap_or(first_taps.left));
    let end = first_taps
        .left
        .max(first_taps.right.unwrap_or(first_taps.left))
        + 1;
    let resident = source
        .window(
            low,
            end,
            source.window_revision().checked_add(1).unwrap(),
            crate::messages::ResidentContext::FiniteLoop,
        )
        .unwrap();
    assert!(resident.valid_residency(RATE, 2));
    assert!(resident.same_source(source));
    assert_eq!(resident.frame_count(), span.plan.sample_frames);
    assert_eq!(resident.channels, span.plan.channels);
    let later = (1..16)
        .find(|offset| {
            let position = span.playback.position_at(*offset).unwrap();
            let taps = SourceReadPlan {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                ..span.plan
            }
            .try_fractional_taps(position.fraction)
            .unwrap();
            taps.left < resident.resident_start()
                || taps.left >= resident.resident_end()
                || taps.right.is_some_and(|right| {
                    right < resident.resident_start() || right >= resident.resident_end()
                })
        })
        .expect("a real later tap must leave the valid original source window");
    assert!(later > 0);
    assert!(span.playback.can_read_frames(16));
    for checked in [false, true] {
        let mut first_output = [vec![17.; 1], vec![19.; 1]];
        if checked {
            span.plan.fill_fractional_buffers_checked_with_readers(
                &resident,
                StemReadSets::default(),
                &span.playback,
                &mut first_output,
                1,
            )
        } else {
            span.plan.fill_fractional_buffers_with_readers(
                &resident,
                StemReadSets::default(),
                &span.playback,
                &mut first_output,
                1,
            )
        }
        .unwrap();
        assert_ne!(
            first_output,
            [vec![17.; 1], vec![19.; 1]],
            "the first sample is genuinely readable"
        );
        let mut output = [vec![17.; 16], vec![19.; 16]];
        let before = output.clone();
        crate::audio_engine::source_reader::reset_tap_observation_for_test();
        let result = if checked {
            span.plan.fill_fractional_buffers_checked_with_readers(
                &resident,
                StemReadSets::default(),
                &span.playback,
                &mut output,
                16,
            )
        } else {
            span.plan.fill_fractional_buffers_with_readers(
                &resident,
                StemReadSets::default(),
                &span.playback,
                &mut output,
                16,
            )
        };
        assert!(result.is_err());
        assert!(
            crate::audio_engine::source_reader::tap_observation_for_test().missing_reads > 0,
            "the valid-domain call must reach the real unavailable later tap"
        );
        assert_eq!(
            output, before,
            "later tap refusal must precede every output write"
        );
    }
}
