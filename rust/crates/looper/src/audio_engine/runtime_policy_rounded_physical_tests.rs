//! Human decimal-BPM Legacy loops, through actual source/OwnACK/policy/render.
use super::*;

fn rounded_legacy_fixture() -> StaticPath {
    rounded_legacy_fixture_for(2)
}

fn rounded_legacy_fixture_for(lanes: usize) -> StaticPath {
    let lengths = (0..lanes)
        .map(|id| [1915, 15696][id % 2])
        .collect::<Vec<_>>();
    let mut fixture = StaticPath::new(&lengths);
    for id in 0..lanes {
        fixture.path.source.engine.stop_sample(id).unwrap();
        fixture.drain();
    }
    fixture.drain();
    for id in 0..lanes {
        let bpm = [93.98316955566406 * 512.0, 91.74028778076172 * 512.0][id % 2];
        fixture
            .path
            .source
            .engine
            .set_pad_bpm(id, Some(bpm))
            .unwrap();
        fixture
            .path
            .source
            .engine
            .set_pad_timing_intent(id, "legacy")
            .unwrap();
        fixture.drain();
    }
    fixture.drain();
    let master = (60.0 / 91.74028778076172) / 512.0;
    fixture.path.source.mixer.set_master_period(master);
    assert!(fixture.path.timeline.set_master_period(master));
    fixture.path.timeline.complete_bootstrap();
    fixture.publish(false, 2, 7, None);
    for id in 0..lanes {
        assert_eq!(fixture.start(id).state(), "applied");
    }
    fixture.publish(true, 3, 7, None);
    assert_eq!(fixture.effect().observation().unwrap().state, "applied");
    fixture.path.render(8192);
    fixture
}

#[test]
#[ignore = "descriptive normalized physical callback cost; Root runs serially after frozen POST"]
fn descriptive_normalized_physical_render_cohorts_report_cost_and_limits() {
    for lanes in [2, 4, 6, 10] {
        let mut first = Vec::new();
        let mut steady = Vec::new();
        for repeat in 0..4 {
            let mut fixture = rounded_legacy_fixture_for(lanes);
            for id in 0..lanes {
                fixture.path.source.engine.pause_sample(id).unwrap();
                fixture.drain();
            }
            fixture.drain();
            fixture.path.render(64);
            for id in 0..lanes {
                fixture.path.source.engine.resume_sample(id).unwrap();
                fixture.drain();
            }
            fixture.drain();
            let snapshot = request(&fixture.path);
            let mut out = [0.0; 1024];
            let mut retired = StaticRetirement::new();
            let begin = Instant::now();
            let (_, allocations) = observe_current_thread(|| {
                drain_and_render_static(&mut fixture, &mut out, &mut retired)
            });
            let elapsed = begin.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(allocations, ThreadAllocations::default());
            assert_eq!(snapshot.observation().unwrap().state, "applied");
            assert!((0..lanes).all(|id| {
                fixture
                    .path
                    .voice(id)
                    .source_playback
                    .is_physical_normalized_static()
            }));
            assert!((0..lanes).any(|id| {
                !fixture
                    .path
                    .voice(id)
                    .source_playback
                    .static_phase_settled()
            }));
            fixture.path.render(8192);
            assert!((0..lanes).all(|id| {
                fixture
                    .path
                    .voice(id)
                    .source_playback
                    .static_phase_settled()
            }));
            let begin = Instant::now();
            let (_, allocations) = observe_current_thread(|| {
                drain_and_render_static(&mut fixture, &mut out, &mut retired)
            });
            let elapsed_steady = begin.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(allocations, ThreadAllocations::default());
            assert!(out.iter().all(|sample| sample.is_finite()));
            if repeat != 0 {
                first.push(elapsed);
                steady.push(elapsed_steady);
            }
        }
        first.sort_by(f64::total_cmp);
        steady.sort_by(f64::total_cmp);
        println!(
            "normalized_physical_profile active={lanes} frames=512 warmups=1 repeats=3 correcting_median_ms={:.4} correcting_max_ms={:.4} steady_median_ms={:.4} steady_max_ms={:.4} nominal_ms={:.4}; actual source/OwnACK/queues/claim/dry reader/history, zero Rust allocation; scaled Human periods, no device/music/worstcase acceptance",
            first[1],
            first[2],
            steady[1],
            steady[2],
            512.0 / f64::from(RATE) * 1000.0
        );
    }
}

#[test]
fn rounded_legacy_eight_and_sixty_four_bar_fixed_phase_reads_are_supported() {
    let mut fixture = rounded_legacy_fixture();
    let result = fixture.effect();
    assert_eq!(
        result.observation().unwrap().state,
        "applied",
        "{:?}",
        result.observation()
    );
    for id in 0..2 {
        let voice = fixture.path.voice(id);
        assert!(voice.source_timing.accepted.is_none());
        assert!(voice.phase_launch_qualified);
        assert!(voice.source_playback.is_static_phase(), "PAD{id}");
    }
    let out = fixture.path.render(512);
    assert!(out.iter().any(|sample| *sample != 0.0));
    for id in 0..2 {
        assert!(fixture.path.voice(id).phase_read_current());
        Python::attach(|py| {
            let status = fixture
                .path
                .source
                .engine
                .pad_source_sync_status(py, id)
                .unwrap()
                .unwrap();
            for field in ["phase_program_current", "phase_read_confirmed"] {
                assert_eq!(
                    status
                        .bind(py)
                        .get_item(field)
                        .unwrap()
                        .unwrap()
                        .extract::<Option<bool>>()
                        .unwrap(),
                    Some(true)
                );
            }
        });
    }
    let pairs = (0..2)
        .map(|id| fixture.path.voice(id).source_playback.position())
        .collect::<Vec<_>>();
    fixture.publish(false, 4, 7, None);
    assert_eq!(fixture.effect().observation().unwrap().state, "applied");
    for (id, pair) in pairs.into_iter().enumerate() {
        assert_eq!(fixture.path.voice(id).source_playback.position(), pair);
        assert!(!fixture.path.voice(id).source_playback.is_static_phase());
        Python::attach(|py| {
            let status = fixture
                .path
                .source
                .engine
                .pad_source_sync_status(py, id)
                .unwrap();
            if let Some(status) = status {
                assert_ne!(
                    status
                        .bind(py)
                        .get_item("phase_read_confirmed")
                        .unwrap()
                        .unwrap()
                        .extract::<Option<bool>>()
                        .unwrap(),
                    Some(true)
                );
            }
        });
    }
}

#[test]
fn normalized_physical_one_frame_boundary_keeps_all_thirty_two_lanes_atomic() {
    let mut fixture = StaticPath::new(&[2047; 32]);
    fixture.publish(true, 2, 7, None);
    let snapshot = fixture.effect();
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert!((0..32).all(|id| !fixture.path.voice(id).source_playback.is_static_phase()));
    assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert!((0..32).all(
        |id| fixture.path.voice(id).source_playback.static_grid_matches(
            STATIC_F,
            STATIC_ORIGIN,
            32.0
        )
    ));
    assert!((0..32).all(|id| {
        fixture
            .path
            .voice(id)
            .source_playback
            .is_physical_normalized_static()
    }));
    assert!((0..32).all(|id| fixture.path.voice(id).source_timing.accepted.is_none()));
    let output = fixture.path.render(512);
    assert!(output.iter().all(|value| value.is_finite()));
    assert!((0..32).all(|id| fixture.path.voice(id).phase_read_current()));
}
