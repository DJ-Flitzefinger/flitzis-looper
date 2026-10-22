use super::*;

#[test]
fn ordinary_transport_unqualified_mapped_refusal_consumes_only_accepted_exact_hold_cause() {
    for request in ["pause", "resume", "seek"] {
        let (mut path, _job, _captured) = qualified();
        path.render(7);
        let held = pause(&path, 1);
        path.drain();
        let index = lane(&path);
        path.source.mixer.voices[index].mapped_start = None;
        let before = path.voice().source_playback;
        let continuity = path.voice().source_feed_history.continuity_revision();
        match request {
            "pause" => {
                path.source.engine.pause_sample(0).unwrap();
            }
            "resume" => {
                path.source.engine.resume_sample(0).unwrap();
            }
            _ => {
                // The genuine qualified launch already adopted/ACKed its 32..64
                // resident window. Use that window, as the existing mapped Seek
                // supersession test does, so admission reaches the callback guard.
                path.source
                    .engine
                    .seek_sample_with_source(0, 40.0 / f64::from(RATE), None)
                    .unwrap();
            }
        }
        path.drain();
        let observation = path
            .source
            .engine
            ._current_ordinary_transport_observation(0)
            .unwrap()
            .unwrap();
        assert_eq!(observation.outcome(), Some("refused"));
        assert_eq!(observation.requested_effect(), Some(request));
        assert!(observation.ordinary_paused());
        assert_eq!(held.action.state(), HoldActionState::Superseded);
        assert!(path.voice().paused);
        assert!(path.voice().source_playback.matches_exact(&before));
        assert_eq!(
            path.voice().source_feed_history.continuity_revision(),
            continuity
        );
        release(&path, &held);
        path.drain();
        assert!(path.voice().paused);
        assert!(path.voice().source_playback.matches_exact(&before));
    }
}

fn repeated_ordinary(partitions: &[usize]) -> Vec<f32> {
    let (mut path, _job, _captured) = qualified();
    let source = path.voice().sample.as_ref().unwrap().clone();
    let mut output = Vec::new();
    let mut active = 0;
    for (running, silent) in [(7, 113), (29, 67), (71, 193)] {
        let rendered = render_partitioned(&mut path, running, partitions);
        assert_pcm(
            &rendered,
            (active..active + running)
                .map(|frame| pcm_at(&source, coordinate(frame as f64 / 64.0))),
        );
        output.extend(rendered);
        active += running;
        let saved = path.voice().source_playback;
        let paused = path.source.engine.pause_sample(0).unwrap();
        path.drain();
        let current = paused.current_observation().unwrap().unwrap();
        assert!(current.matches_admission(&paused));
        assert!(current.ordinary_paused());
        assert!(path.voice().source_playback.matches_exact(&saved));
        let history = snapshot(&path);
        let silent = render_partitioned(&mut path, silent, partitions);
        assert!(silent.iter().all(|value| *value == 0.0));
        output.extend(silent);
        same_spans(&history, &snapshot(&path));
        let resumed = path.source.engine.resume_sample(0).unwrap();
        path.drain();
        let current = resumed.current_observation().unwrap().unwrap();
        assert!(current.matches_admission(&resumed));
        assert!(!current.ordinary_paused());
        exact(saved.position(), path.voice().source_playback.position());
        assert_eq!(
            path.voice().source_playback.mapped_output_frame(),
            Some(path.timeline.output_frame())
        );
        same_spans(&history, &snapshot(&path));
    }
    let rendered = render_partitioned(&mut path, 131, partitions);
    assert_pcm(
        &rendered,
        (active..active + 131).map(|frame| pcm_at(&source, coordinate(frame as f64 / 64.0))),
    );
    output.extend(rendered);
    output
}

#[test]
fn ordinary_transport_real_mapped_public_continuation_pcm_fractional_and_partition_invariant() {
    assert_eq!(
        repeated_ordinary(&[1]),
        repeated_ordinary(&[3, 17, 2, 31, 7])
    );
}

#[test]
fn ordinary_transport_mapped_pause_takes_earlier_hold_cursor_and_resume_uses_that_pivot() {
    let (mut path, _job, _captured) = qualified();
    path.render(7);
    let saved = path.voice().source_playback;
    let ticket = pause(&path, 1);
    path.drain();
    path.render(113);
    let ordinary = path.source.engine.pause_sample(0).unwrap();
    path.drain();
    assert_eq!(
        ordinary.current_observation().unwrap().unwrap().outcome(),
        Some("no_effect")
    );
    assert!(path.voice().source_playback.matches_exact(&saved));
    path.render(67);
    let resumed = path.source.engine.resume_sample(0).unwrap();
    path.drain();
    assert!(!path.voice().paused);
    assert!(
        resumed
            .current_observation()
            .unwrap()
            .unwrap()
            .matches_admission(&resumed)
    );
    exact(saved.position(), path.voice().source_playback.position());
    release(&path, &ticket);
    path.drain();
    assert_eq!(ticket.action.state(), HoldActionState::Superseded);
    let source = path.voice().sample.as_ref().unwrap().clone();
    let rendered = path.render(91);
    assert_pcm(
        &rendered,
        (7..98).map(|frame| pcm_at(&source, coordinate(frame as f64 / 64.0))),
    );
}

#[test]
fn ordinary_transport_mixed_cohort_bad_mapped_epoch_refuses_scalar_pause_atomically() {
    let (mut path, _job, _captured, scalar) = refusal_tests::mixed_cohort(false);
    path.render(7);
    let mapped = lane(&path);
    let before = [
        path.source.mixer.voices[mapped].source_playback,
        path.source.mixer.voices[scalar].source_playback,
    ];
    assert!(
        path.timeline
            .set_master_period_preserving_beat_position_at_frame(PERIOD / 1.16, 7)
    );
    let pause = path.source.engine.pause_sample(0).unwrap();
    path.drain();
    let current = pause.current_observation().unwrap().unwrap();
    assert_eq!(current.outcome(), Some("refused"));
    assert!(!current.ordinary_paused());
    for (offset, index) in [mapped, scalar].into_iter().enumerate() {
        assert!(!path.source.mixer.voices[index].paused);
        assert!(
            path.source.mixer.voices[index]
                .source_playback
                .matches_exact(&before[offset])
        );
    }
}

#[test]
fn ordinary_transport_actual_pyo_admissions_and_observations_follow_qualified_callback() {
    let (mut path, _job, _captured) = qualified();
    path.render(7);
    let saved = path.voice().source_playback;
    let LaunchPath {
        source: custody,
        mut timeline,
        mut scheduler,
        mut feedback,
    } = path;
    let ColdCustody {
        engine,
        mut mixer,
        mut consumer,
        directory,
        ..
    } = custody;
    let (engine, paused) = Python::attach(|py| {
        let engine = Py::new(py, engine).unwrap();
        let admission = engine.call_method1(py, "pause_sample", (0,)).unwrap();
        assert!(
            admission
                .bind(py)
                .getattr("requested_paused")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            admission.bind(py).get_type().call0().is_err(),
            "admission must not be publicly constructible"
        );
        (engine, admission)
    });
    drain_control_messages(
        &mut consumer,
        &mut scheduler,
        timeline.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut timeline,
        &mut mixer,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    Python::attach(|py| {
        let observation = paused.call_method0(py, "current_observation").unwrap();
        assert!(
            observation
                .bind(py)
                .getattr("ordinary_paused")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert_eq!(
            observation
                .call_method0(py, "current_control_slot")
                .unwrap()
                .extract::<Option<usize>>(py)
                .unwrap(),
            Some(0)
        );
        assert!(
            observation
                .call_method0(py, "current_observation")
                .unwrap()
                .bind(py)
                .getattr("ordinary_paused")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            observation
                .call_method1(py, "matches_admission", (paused.clone_ref(py),))
                .unwrap()
                .extract::<bool>(py)
                .unwrap()
        );
    });
    let mut silent = vec![0.0; 113 * 2];
    let start = timeline.output_frame();
    render_scheduled_audio(
        &mut mixer,
        &mut scheduler,
        &mut silent,
        &mut [0.0; NUM_SAMPLES],
        start,
        2,
        &mut timeline,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    timeline.advance_by_rendered_frames(113);
    assert!(silent.iter().all(|value| *value == 0.0));
    let resumed = Python::attach(|py| engine.call_method1(py, "resume_sample", (0,)).unwrap());
    drain_control_messages(
        &mut consumer,
        &mut scheduler,
        timeline.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut timeline,
        &mut mixer,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    let voice = mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert!(!voice.paused);
    exact(saved.position(), voice.source_playback.position());
    Python::attach(|py| {
        let observation = resumed.call_method0(py, "current_observation").unwrap();
        assert!(
            !observation
                .bind(py)
                .getattr("ordinary_paused")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            observation
                .call_method1(py, "settles_admission", (resumed.clone_ref(py),))
                .unwrap()
                .extract::<bool>(py)
                .unwrap()
        );
        drop(engine);
        drop(paused);
        drop(resumed);
    });
    drop(directory);
}
