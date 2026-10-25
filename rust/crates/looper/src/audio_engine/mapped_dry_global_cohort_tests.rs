// Each participant owns its own real source-qualified map completion and launch.
// Equal authored PCM is not reused as another content's admission permission.
use super::*;

fn qualified_cohort_map(path: &LaunchPath, id: usize) -> (OfflineAnalysisJob, LoaderEventDelivery) {
    let mut expected = record(&path.source, id);
    let accepted = &mut expected["record"]["accepted"];
    let first = accepted["anchors"][0].clone();
    let mut middle = accepted["anchors"][1].clone();
    middle["id"] = json!("cohort-variable-knot");
    middle["source_seconds_bits"] = json!(bits(48.0 / f64::from(RATE)));
    middle["beat_bits"] = json!(bits(0.6));
    let mut last = accepted["anchors"][1].clone();
    last["source_seconds_bits"] = json!(bits(covered_terminal_seconds(64)));
    let interval = accepted["intervals"][0].clone();
    accepted["anchors"] = json!([first, middle, last]);
    accepted["intervals"] = json!([interval.clone(), interval]);
    seal(&mut expected);
    assert!(maps(&path.source).is_empty());
    // Unlike the older PAD0 helper, this captures this exact content's real job.
    let job = path
        .source
        .engine
        .begin_source_beatmap_store_async(id, expected.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let mut events = maps(&path.source);
    assert_eq!(events.len(), 1);
    let event = events.pop().unwrap();
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            event
                .get_item("id")
                .unwrap()
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            id
        );
        let owner = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            job.resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });
    (job, delivery)
}

fn publish_cohort_maps(
    path: &LaunchPath,
    deliveries: &[LoaderEventDelivery; 2],
    revisions: [u64; 2],
) {
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
    for id in [0, 1] {
        bindings[id] = Some(binding(&path.source, id));
        rows[id] = SourceMapRow {
            sync_requested: true,
            intent_revision: revisions[id],
            delivery: Some(deliveries[id].clone()),
            ordinary_pending: false,
            exact_quarters: None,
        };
    }
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            vec![32.0 / f64::from(RATE); NUM_SAMPLES],
            vec![Some(64.0 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                crate::audio_engine::input_mapping::PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(rows),
        )
        .unwrap();
}

fn cohort_voice(path: &LaunchPath, id: usize) -> &crate::audio_engine::voice_slot::VoiceSlot {
    path.source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == id)
        .unwrap()
}

fn start_cohort_member(path: &mut LaunchPath, id: usize) -> CapturedPadStart {
    let execution = path.timeline.output_frame();
    let captured = path
        .source
        .engine
        .capture_pad_start(
            id,
            32.0 / f64::from(RATE),
            Some(64.0 / f64::from(RATE)),
            0.35,
            false,
            None,
            None,
        )
        .unwrap();
    assert!(captured.mapped_geometry().is_some());
    path.publish_capture(&captured);
    assert_eq!(
        (
            captured.state(),
            captured.target_frame(),
            captured.effective_frame()
        ),
        ("applied", Some(execution), Some(execution))
    );
    let voice = cohort_voice(path, id);
    assert!(
        voice
            .mapped_start
            .as_ref()
            .unwrap()
            .original()
            .same_owner(&captured)
    );
    assert!(voice.source_playback.is_mapped());
    assert!(
        !crate::audio_engine::native_source_coverage::complete_view_available(
            voice.sample.as_ref().unwrap()
        )
    );
    captured
}

fn verify_cohort_member(
    path: &LaunchPath,
    id: usize,
    captured: &CapturedPadStart,
    segments: &[MasterSegment],
) {
    let voice = cohort_voice(path, id);
    let origin = captured.target_frame().unwrap();
    let end = path.timeline.output_frame();
    let mut history = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut history));
    assert_eq!(history.retained_range(), Some(0..end - origin));
    let mut next = origin;
    let mut spans = 0;
    for span in history.spans() {
        let start = span.playback.mapped_output_frame().unwrap();
        assert_eq!(start, next);
        let epoch = segments
            .iter()
            .rev()
            .find(|segment| segment.frame <= start)
            .unwrap();
        assert!(
            span.playback
                .mapped_epoch()
                .unwrap()
                .matches_timeline(&epoch.actual_epoch)
        );
        for offset in 0..=span.frames {
            let position = span.playback.position_at(offset).unwrap();
            let expected = coordinate(segments, start + offset as u64, origin, true);
            assert!(
                (position.frame as f64 + position.fraction - expected).abs() < 1e-9,
                "cohort pad={id} n={} source position",
                start + offset as u64
            );
        }
        next += span.frames as u64;
        spans += 1;
    }
    assert_eq!(next, end);
    assert!(spans > 12);
}

fn two_mapped_cohort(partitions: &[usize]) -> Vec<f32> {
    let mut path = mapped_fixed_pitch_birth_tests::configured(0);
    let (_job0, delivery0) = qualified_cohort_map(&path, 0);
    let (_job1, delivery1) = qualified_cohort_map(&path, 1);
    let deliveries = [delivery0, delivery1];
    publish_cohort_maps(&path, &deliveries, [1, 1]);
    let first = start_cohort_member(&mut path, 0);
    let mut output = Vec::new();
    render_to(&mut path, 13, partitions, &mut output);
    let second = start_cohort_member(&mut path, 1);
    assert_ne!(first.content_key(), second.content_key());
    assert!(!first.same_owner(&second));
    let launches = [
        first.target_frame().unwrap(),
        second.target_frame().unwrap(),
    ];
    assert_eq!(launches, [0, 13]);
    // New requested truth is independent of both immutable effective launch owners.
    publish_cohort_maps(&path, &deliveries, [2, 1]);
    let current = binding(&path.source, 0);
    let requested = current
        .source_map_launch_snapshot()
        .admit(&path.source.engine.input_runtime_ownership, current.binding)
        .unwrap()
        .unwrap();
    assert_eq!(requested.sync_witness().intent_revision, 2);
    assert_eq!(
        first
            .source_sync_witness(first.content_key(), current.source_generation)
            .unwrap()
            .intent_revision,
        1
    );
    let source0 = cohort_voice(&path, 0).sample.as_ref().unwrap().clone();
    let source1 = cohort_voice(&path, 1).sample.as_ref().unwrap().clone();
    let generations = [
        cohort_voice(&path, 0).generation,
        cohort_voice(&path, 1).generation,
    ];
    let mut expected_epochs = [
        cohort_voice(&path, 0)
            .source_playback
            .mapped_epoch()
            .unwrap(),
        cohort_voice(&path, 1)
            .source_playback
            .mapped_epoch()
            .unwrap(),
    ];
    assert!(!expected_epochs[0].matches_exact(expected_epochs[1]));
    let original_transport = path.timeline;
    let mut segments = vec![MasterSegment {
        frame: 0,
        beat: 0.0,
        period: PERIOD,
        actual_epoch: original_transport,
    }];
    let mut parameters = Parameters::new(&mut path);
    // Both boundaries are declared before the requests and result inspections.
    for (execution, rate) in [(30, 1.16), (173, 0.81)] {
        render_to(&mut path, execution, partitions, &mut output);
        let positions = [
            cohort_voice(&path, 0).source_playback.position(),
            cohort_voice(&path, 1).source_playback.position(),
        ];
        assert_ne!(
            positions[0], positions[1],
            "independent creative phases must remain observable"
        );
        let beat = master_at(&segments, execution);
        parameters.coupled(&mut path, rate, PERIOD / rate);
        assert_eq!(
            effect(&path).state,
            RateState::Applied,
            "{:?}",
            effect(&path).error
        );
        assert_eq!(effect(&path).execution_frame, Some(execution));
        assert_eq!(
            path.timeline
                .beat_position_at_frame(execution)
                .unwrap()
                .to_bits(),
            beat.to_bits()
        );
        for (id, captured) in [&first, &second].into_iter().enumerate() {
            expected_epochs[id] = expected_epochs[id]
                .with_timeline_at(path.timeline, execution)
                .unwrap();
            let voice = cohort_voice(&path, id);
            assert_eq!(voice.generation, generations[id]);
            assert_eq!(voice.source_playback.position(), positions[id]);
            assert_eq!(voice.source_playback.mapped_output_frame(), Some(execution));
            assert!(
                voice
                    .mapped_start
                    .as_ref()
                    .unwrap()
                    .original()
                    .same_owner(captured)
            );
            assert!(
                voice
                    .source_playback
                    .mapped_epoch()
                    .unwrap()
                    .matches_exact(expected_epochs[id])
            );
            assert!(!voice.stretch.has_active_native_ownership());
            assert!(!voice.stretch.has_retained_source_input());
            assert_eq!(captured.target_frame(), Some(launches[id]));
            assert_eq!(captured.effective_frame(), Some(launches[id]));
        }
        segments.push(MasterSegment {
            frame: execution,
            beat,
            period: PERIOD / rate,
            actual_epoch: path.timeline,
        });
    }
    render_to(&mut path, 829, partitions, &mut output);
    verify_cohort_member(&path, 0, &first, &segments);
    verify_cohort_member(&path, 1, &second, &segments);
    for frame in 0..829 {
        let first_pcm = pcm_at(&source0, coordinate(&segments, frame, 0, true));
        let second_pcm = if frame >= 13 {
            pcm_at(&source1, coordinate(&segments, frame, 13, true))
        } else {
            [0.0; 2]
        };
        for channel in 0..2 {
            let expected = first_pcm[channel] * 0.35 + second_pcm[channel] * 0.35;
            assert!(
                (output[frame as usize * 2 + channel] - expected).abs() < 4e-6,
                "complete cohort PCM n={frame} channel={channel}"
            );
        }
    }
    output
}

#[test]
fn dry_mapped_two_original_content_owners_with_different_creative_phases_share_one_rate_claim() {
    let single = two_mapped_cohort(&[1]);
    let unequal = two_mapped_cohort(&[127, 1, 383, 17, 256]);
    assert_eq!(single, unequal);
}

#[test]
fn dry_mapped_two_owner_cohort_refuses_one_stale_ack_without_changing_either_participant() {
    let mut path = mapped_fixed_pitch_birth_tests::configured(0);
    let (_job0, delivery0) = qualified_cohort_map(&path, 0);
    let (_job1, delivery1) = qualified_cohort_map(&path, 1);
    publish_cohort_maps(&path, &[delivery0, delivery1], [1, 1]);
    let first = start_cohort_member(&mut path, 0);
    path.render(13);
    let second = start_cohort_member(&mut path, 1);
    path.render(17);
    assert_eq!(path.timeline.output_frame(), 30);
    let transport = path.timeline;
    let before = [
        cohort_voice(&path, 0).source_playback,
        cohort_voice(&path, 1).source_playback,
    ];
    let generations = [
        cohort_voice(&path, 0).generation,
        cohort_voice(&path, 1).generation,
    ];
    assert!(
        path.source
            .engine
            .current_timing_acknowledgements
            .acknowledge_for_key(
                &path.source.engine.input_runtime_ownership,
                second.content_key(),
                1
            )
    );
    let mut parameters = Parameters::new(&mut path);
    parameters.coupled(&mut path, 1.16, PERIOD / 1.16);
    assert_eq!(effect(&path).state, RateState::Rejected);
    assert_eq!(effect(&path).effective.speed, 1.0);
    assert_eq!(effect(&path).effective.period, Some(PERIOD));
    assert!(path.timeline.matches_clock_epoch(&transport));
    for (id, captured) in [&first, &second].into_iter().enumerate() {
        let voice = cohort_voice(&path, id);
        assert_eq!(voice.generation, generations[id]);
        assert!(voice.source_playback.matches_exact(&before[id]));
        assert!(
            voice
                .mapped_start
                .as_ref()
                .unwrap()
                .original()
                .same_owner(captured)
        );
    }
    let output = path.render(97);
    let old = [MasterSegment {
        frame: 0,
        beat: 0.0,
        period: PERIOD,
        actual_epoch: transport,
    }];
    for frame in 30..127 {
        let mut expected = [0.0; 2];
        for (id, launch) in [(0, 0), (1, 13)] {
            let source = cohort_voice(&path, id).sample.as_ref().unwrap();
            let pcm = pcm_at(source, coordinate(&old, frame, launch, true));
            for channel in 0..2 {
                expected[channel] += pcm[channel] * 0.35;
            }
        }
        for channel in 0..2 {
            assert!((output[(frame - 30) as usize * 2 + channel] - expected[channel]).abs() < 4e-6);
        }
    }
}
