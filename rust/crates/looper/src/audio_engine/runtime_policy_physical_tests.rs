//! Real physical-marker None publications and an independent original-knot oracle.
//! Generated PCM and bounded engineering coordinates are not music/device acceptance.

use super::*;
use crate::audio_engine::general_source_numerics::{
    Integer, MappedReaderPeriod, PhysicalMapQuarters,
};
use crate::audio_engine::source_reader::SourceLoopDomain;

const CROSS_ENDPOINT_ANCHORS: &[(f64, f64)] =
    &[(0.0, 0.0), (0.001, 0.125), (0.031, 2.0), (0.062, 4.0)];

fn physical(lanes: usize, initial_frames: usize) -> GeneralPath {
    GeneralPath::with_extent(lanes, initial_frames, CROSS_ENDPOINT_ANCHORS, None)
}

fn read_plan(fixture: &GeneralPath, id: usize, playback: SourcePlayback) -> SourceReadPlan {
    let sample = fixture.dry.path.voice(id).sample.as_ref().unwrap();
    let domain = playback.effective_domain().unwrap();
    SourceReadPlan {
        channels: sample.channels,
        sample_frames: sample.frame_count(),
        frame_pos: playback.position().frame,
        loop_region: domain.region,
        loop_period: playback.reader_period(),
        seek_mode: playback.position().seek_mode,
        selection: StemRenderSelection::full_mix(),
        transition: StemTransition::default(),
    }
}

fn assert_physical_domain(playback: SourcePlayback, original: SourceLoopDomain) {
    assert!(playback.is_general_mapped());
    assert!(matches!(
        playback.reader_period(),
        SourceReaderPeriod::MappedPhysicalQuarters(_)
    ));
    assert_eq!(playback.original_scalar_domain(), Some(original));
    assert_eq!(original.musical_period.scalar_period(), Some(1500.0));
    let domain = playback.effective_domain().unwrap();
    assert_eq!(domain.region, original.region);
    assert_ne!(domain, original);
    assert_eq!(
        domain.musical_period.mapped_period(domain.region).unwrap(),
        MappedReaderPeriod::physical(END - START).unwrap()
    );
}

#[test]
fn genuine_none_original_decimal_endpoints_adopt_exact_quarters_and_preserve_first_pcm() {
    for (anchors, expected_n, expected_d) in [
        (
            GENERAL_ANCHORS,
            288239383350966491_u128,
            144115188075855875_u128,
        ),
        (
            CROSS_ENDPOINT_ANCHORS,
            973596225794487503992543076417536_u128,
            482883607843738976046606013087875_u128,
        ),
    ] {
        let mut fixture = GeneralPath::with_extent(2, 23, anchors, None);
        let mut unchanged_reference = GeneralPath::with_extent(2, 23, anchors, None);
        assert!(fixture.quarters.iter().all(Option::is_none));
        let anchor = oracle_anchor(&fixture, 0);
        let before = fixture.dry.path.voice(0).source_playback;
        let original = before.effective_domain().unwrap();
        let old_taps = read_plan(&fixture, 0, before)
            .try_fractional_taps(anchor.raw.fraction)
            .unwrap();
        let snapshot = fixture.adopt(true, 3);
        assert_eq!(snapshot.map_preparation(0).class, RuntimeMapClass::Ready);
        assert_eq!(
            snapshot.observation().unwrap().execution_frame,
            Some(anchor.first)
        );
        let voice = fixture.dry.path.voice(0);
        let after = voice.source_playback;
        assert_physical_domain(after, original);
        assert_position(after.position(), anchor.raw);
        assert_eq!(
            after.reader_period().physical_quarters().unwrap().parts(),
            (
                Integer::from_u128(expected_n).unwrap(),
                Integer::from_u128(expected_d).unwrap(),
            )
        );
        if expected_d > u128::from(u64::MAX) {
            let (_, denominator) = after.reader_period().physical_quarters().unwrap().parts();
            assert_eq!(denominator.magnitude.bit_width(), 109);
        }
        assert_eq!(
            read_plan(&fixture, 0, after)
                .try_fractional_taps(anchor.raw.fraction)
                .unwrap(),
            old_taps
        );
        assert!(
            voice
                .mapped_start
                .as_ref()
                .unwrap()
                .original_launch()
                .is_none()
        );
        assert_eq!(
            fixture.dry.path.render(1),
            unchanged_reference.dry.path.render(1)
        );
        fixture.dry.path.render(4097);
        verify_general_reads(&fixture, anchor, 0);
    }
}

#[test]
fn physical_bound_plan_rejects_scalar_equal_period_and_wrong_quarters_before_output() {
    let mut fixture = physical(2, 23);
    fixture.adopt(true, 3);
    let playback = fixture.dry.path.voice(0).source_playback;
    let domain = playback.effective_domain().unwrap();
    let (n, d) = playback
        .reader_period()
        .physical_quarters()
        .unwrap()
        .parts();
    let wrong_quarters =
        PhysicalMapQuarters::new(n.add(Integer::from_u128(1).unwrap()).unwrap(), d).unwrap();
    let wrong_physical =
        SourceLoopDomain::mapped_physical(domain.sample_frames, domain.region, wrong_quarters)
            .unwrap();
    let equal_source_period = SourceLoopDomain::mapped(
        domain.sample_frames,
        domain.region,
        domain.musical_period.mapped_period(domain.region).unwrap(),
    )
    .unwrap();
    let scalar_equal = SourceLoopDomain::musical(
        domain.sample_frames,
        domain.region,
        domain.region.len() as f64,
    )
    .unwrap();
    let physical_equal = SourceLoopDomain::physical(domain.sample_frames, domain.region);
    let plan = read_plan(&fixture, 0, playback);
    let sample = fixture.dry.path.voice(0).sample.as_ref().unwrap();
    for wrong in [
        wrong_physical,
        equal_source_period,
        scalar_equal,
        physical_equal,
    ] {
        assert_ne!(domain, wrong);
        let mut unchanged = playback;
        assert!(!unchanged.configure_domain(wrong));
        assert!(unchanged.matches_exact(&playback));
        let wrong_plan = plan.with_effective_domain(wrong).unwrap();
        assert!(!wrong_plan.matches_source_contract(plan));
        for checked in [false, true] {
            let mut output = [vec![17.0; 1], vec![19.0; 1]];
            let before = output.clone();
            let result = if checked {
                wrong_plan.fill_fractional_buffers_checked_with_readers(
                    sample,
                    StemReadSets::default(),
                    &playback,
                    &mut output,
                    1,
                )
            } else {
                wrong_plan.fill_fractional_buffers_with_readers(
                    sample,
                    StemReadSets::default(),
                    &playback,
                    &mut output,
                    1,
                )
            };
            assert!(result.is_err());
            assert_eq!(output, before);
        }
    }
    // A detached numerical span has no original-domain association permit. The
    // assertions above exercise the actual bound playback/plan, not a forged span.
}

#[test]
fn physical_declared_intersection_replacements_and_reverse_retain_original_scalar_circle() {
    let mut fixture = GeneralPath::with_extent(2, 23, GENERAL_ANCHORS, None);
    let original = fixture
        .dry
        .path
        .voice(0)
        .source_playback
        .effective_domain()
        .unwrap();
    fixture.adopt(true, 3);
    let before = fixture.dry.path.voice(0).source_playback;
    fixture.dry.intent_revision += 1;
    fixture.adopt(true, 4);
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .matches_exact(&before)
    );
    let first_owner = fixture
        .dry
        .path
        .voice(0)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .clone();
    let mut retired = HeldRetirement::new(64);
    for (revision, anchors, quarters, general, physical_mode) in [
        (5, GENERAL_ANCHORS.to_vec(), Some(QUARTERS), true, false),
        (
            6,
            intersection_anchors(4101.0).to_vec(),
            Some(QUARTERS),
            false,
            false,
        ),
        (
            7,
            intersection_anchors(4102.0).to_vec(),
            Some(QUARTERS),
            false,
            false,
        ),
        (8, GENERAL_ANCHORS.to_vec(), None, true, true),
    ] {
        if fixture.anchors != anchors {
            fixture.replace_maps(&anchors);
        }
        fixture.quarters.fill(quarters);
        fixture.publish(true, revision, None);
        let snapshot = request(&fixture.dry.path);
        let raw = fixture.dry.path.voice(0).source_playback.position();
        let old_taps = read_plan(&fixture, 0, fixture.dry.path.voice(0).source_playback)
            .try_fractional_taps(raw.fraction)
            .unwrap();
        drain_and_render(&mut fixture.dry, &mut [], &mut retired);
        assert_eq!(
            snapshot.observation().unwrap().state,
            "applied",
            "{:?}",
            snapshot.observation()
        );
        let playback = fixture.dry.path.voice(0).source_playback;
        assert_position(playback.position(), raw);
        assert_eq!(playback.original_scalar_domain(), Some(original));
        assert_eq!(playback.is_general_mapped(), general);
        assert_eq!(
            playback.reader_period().physical_quarters().is_some(),
            physical_mode
        );
        assert_eq!(
            read_plan(&fixture, 0, playback)
                .try_fractional_taps(raw.fraction)
                .unwrap(),
            old_taps
        );
    }
    assert!(
        retired
            .maps
            .iter()
            .any(|owner| Arc::ptr_eq(owner.effective_source().unwrap(), &first_owner))
    );
    let raw = fixture.dry.path.voice(0).source_playback.position();
    fixture.adopt(false, 9);
    let scalar = fixture.dry.path.voice(0).source_playback;
    assert!(!scalar.is_mapped());
    assert_eq!(scalar.effective_domain(), Some(original));
    assert_position(scalar.position(), raw);
}

#[test]
fn physical_seam_admission_and_reverse_refusals_recover_through_real_progress() {
    verify_general_seam_retry(physical(2, 23));
    let mut fixture = physical(2, 23);
    fixture.adopt(true, 3);
    let playback = fixture.dry.path.voice(0).source_playback;
    let original = playback.original_scalar_domain().unwrap();
    let epoch = playback.mapped_epoch().unwrap();
    let first = fixture.dry.path.timeline.output_frame();
    let owner = fixture
        .dry
        .path
        .voice(0)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .clone();
    let elapsed = (1..=16_384)
        .find(|elapsed| {
            let raw = owner
                .geometry()
                .position_at(epoch, first + *elapsed as u64)
                .unwrap();
            SourcePlayback::scalar_musical_continuation(raw, original, 1.0, 1.0).is_none()
        })
        .expect("the genuine physical trajectory visits outside the historical scalar circle");
    fixture.dry.path.render(elapsed);
    fixture.publish(false, 4, None);
    let snapshot = automatic_mapped(&fixture.dry.path);
    let before = scalar_state(&fixture.dry.path);
    service(&mut fixture.dry.path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&fixture.dry.path, &before);
    let failed = snapshot.observation().unwrap();
    assert!(Arc::ptr_eq(&snapshot, &automatic_mapped(&fixture.dry.path)));
    assert_eq!(snapshot.observation().unwrap().attempt, failed.attempt + 1);
    service(&mut fixture.dry.path);
    unchanged(&fixture.dry.path, &before);
    fixture.dry.path.render(2);
    let raw = fixture.dry.path.voice(0).source_playback.position();
    assert!(Arc::ptr_eq(&snapshot, &automatic_mapped(&fixture.dry.path)));
    service(&mut fixture.dry.path);
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    let scalar = fixture.dry.path.voice(0).source_playback;
    assert!(!scalar.is_mapped());
    assert_eq!(scalar.effective_domain(), Some(original));
    assert_position(scalar.position(), raw);
}

#[test]
fn physical_declared_intersection_scalar_cohort_refuses_one_bad_member_then_repairs() {
    let mut fixture = physical(4, 23);
    let originals = (0..4)
        .map(|id| {
            fixture
                .dry
                .path
                .voice(id)
                .source_playback
                .effective_domain()
                .unwrap()
        })
        .collect::<Vec<_>>();
    fixture.adopt(true, 3);
    for (id, anchors) in [
        (1, GENERAL_ANCHORS.to_vec()),
        (2, intersection_anchors(4101.0).to_vec()),
    ] {
        let (job, delivery) = fixture.dry.map_anchors(id, &anchors);
        fixture.dry.jobs.push(job);
        fixture.dry.deliveries[id] = delivery;
        fixture.quarters[id] = Some(QUARTERS);
    }
    fixture.publish_with_scalar(true, 4, None, Some(3));
    let mixed = request(&fixture.dry.path);
    service(&mut fixture.dry.path);
    assert_eq!(
        mixed.observation().unwrap().state,
        "applied",
        "{:?}",
        mixed.observation()
    );
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .reader_period()
            .physical_quarters()
            .is_some()
    );
    assert!(matches!(
        fixture.dry.path.voice(1).source_playback.reader_period(),
        SourceReaderPeriod::MappedRational(_)
    ));
    assert!(
        fixture
            .dry
            .path
            .voice(2)
            .source_playback
            .is_compact_mapped()
    );
    assert!(
        !fixture
            .dry
            .path
            .voice(2)
            .source_playback
            .is_general_mapped()
    );
    assert!(!fixture.dry.path.voice(3).source_playback.is_mapped());
    assert_eq!(
        fixture.dry.path.voice(3).sync_request.mode,
        SourceSyncMode::Legacy
    );
    let before = scalar_state(&fixture.dry.path);
    let owners = (0..3)
        .map(|id| {
            fixture
                .dry
                .path
                .voice(id)
                .mapped_start
                .as_ref()
                .unwrap()
                .effective_source()
                .unwrap()
                .clone()
        })
        .collect::<Vec<_>>();
    fixture.dry.intent_revision += 1;
    fixture.publish_with_scalar(true, 5, Some(3), Some(3));
    let failed = request(&fixture.dry.path);
    service(&mut fixture.dry.path);
    assert_eq!(
        failed.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&fixture.dry.path, &before);
    for (id, owner) in owners.iter().enumerate() {
        assert!(Arc::ptr_eq(
            fixture
                .dry
                .path
                .voice(id)
                .mapped_start
                .as_ref()
                .unwrap()
                .effective_source()
                .unwrap(),
            owner
        ));
    }
    fixture.publish_with_scalar(true, 6, None, Some(3));
    let repaired = request(&fixture.dry.path);
    service(&mut fixture.dry.path);
    assert_eq!(
        repaired.observation().unwrap().state,
        "applied",
        "{:?}",
        repaired.observation()
    );
    for (id, generation, playback, _) in before {
        let voice = fixture.dry.path.voice(id);
        assert_eq!(voice.generation, generation);
        assert!(voice.source_playback.matches_exact(&playback));
        assert_eq!(
            voice.source_playback.original_scalar_domain(),
            Some(originals[id])
        );
    }
}

fn replace_metadata(fixture: &mut GeneralPath, coverage: Option<&[&str]>, verified_meter: bool) {
    for id in 0..fixture.dry.lanes {
        let (job, delivery) =
            fixture
                .dry
                .map_anchors_with_metadata(id, &fixture.anchors, coverage, verified_meter);
        fixture.dry.jobs.push(job);
        fixture.dry.deliveries[id] = delivery;
    }
}

#[test]
fn manual_unknown_meter_none_is_productive_while_declared_bars_and_real_gap_refuse() {
    let mut fixture = GeneralPath::with_extent(2, 23, GENERAL_ANCHORS, None);
    replace_metadata(&mut fixture, None, false);
    let anchor = oracle_anchor(&fixture, 0);
    fixture.adopt(true, 3);
    fixture.dry.path.render(513);
    verify_general_reads(&fixture, anchor, 0);
    fixture.quarters.fill(Some(QUARTERS));
    fixture.publish(true, 4, None);
    let declared = request(&fixture.dry.path);
    assert_eq!(
        declared.map_preparation(0).class,
        RuntimeMapClass::Unsupported
    );
    let before = scalar_state(&fixture.dry.path);
    service(&mut fixture.dry.path);
    assert_eq!(
        declared.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&fixture.dry.path, &before);
    fixture.quarters.fill(None);
    replace_metadata(
        &mut fixture,
        Some(&["trusted", "gap_unknown_count", "trusted"]),
        false,
    );
    fixture.publish(true, 5, None);
    let gap = request(&fixture.dry.path);
    assert_eq!(gap.map_preparation(0).class, RuntimeMapClass::Unsupported);
    service(&mut fixture.dry.path);
    assert_eq!(
        gap.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&fixture.dry.path, &before);
}

#[test]
fn physical_end_or_none_original_eof_past_last_map_anchor_refuses_original_cohort() {
    for end in [Some(3500.0 / f64::from(RATE)), None] {
        let mut fixture = physical(2, 23);
        let bindings = (0..NUM_SAMPLES)
            .map(|id| {
                (id < fixture.dry.lanes).then(|| {
                    fixture
                        .dry
                        .path
                        .source
                        .engine
                        .current_input_runtime_pad_binding(id)
                        .unwrap()
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
        for (id, row) in rows.iter_mut().enumerate().take(fixture.dry.lanes) {
            *row = SourceMapRow {
                sync_requested: true,
                intent_revision: fixture.dry.intent_revision,
                delivery: Some(fixture.dry.deliveries[id].clone()),
                exact_quarters: None,
            };
        }
        // Original source extent is4096; the final trusted anchor is at2976.
        // Publish the real physical end (including original EOF via None) through
        // the same runtime entry point, without changing source/map provenance.
        fixture
            .dry
            .path
            .source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_policy(
                true,
                (0..NUM_SAMPLES).map(|id| id < fixture.dry.lanes).collect(),
                vec![START as f64 / f64::from(RATE); NUM_SAMPLES],
                vec![end; NUM_SAMPLES],
                bindings.iter().map(Option::as_ref).collect(),
                Some(vec![
                    PitchPadProjection {
                        sync: true,
                        key_lock: false,
                        ..Default::default()
                    };
                    NUM_SAMPLES
                ]),
                Some(rows),
                GlobalTimingPolicy::parse(Some(&wire(true, true, 3).to_string())).unwrap(),
            )
            .unwrap();
        let snapshot = request(&fixture.dry.path);
        for id in 0..fixture.dry.lanes {
            assert!(snapshot.row_current(id) && snapshot.map_current(id));
            assert_eq!(
                snapshot.map_preparation(id).class,
                RuntimeMapClass::Unsupported
            );
            assert!(snapshot.prepared_map(id).is_none());
        }
        let before = scalar_state(&fixture.dry.path);
        service(&mut fixture.dry.path);
        assert_eq!(
            snapshot.observation().unwrap().reason,
            Some(RuntimePolicyError::Unsupported)
        );
        unchanged(&fixture.dry.path, &before);
    }
}

#[test]
fn physical_pause_hold_rate_history_claim_cancellation_lease_and_retirement_keep_contracts() {
    verify_general_pause_hold(physical(2, 23));
    verify_general_claim_cancellation(physical(2, 23));
    verify_general_lease_contention(physical(2, 23));
    verify_general_retirement_pressure(physical(2, 23));
}

#[test]
fn physical_original_knot_oracle_covers_large_cast_plateaus_and_signed_zero() {
    let fixture = physical(1, 23);
    let original = fixture
        .dry
        .path
        .voice(0)
        .source_playback
        .effective_domain()
        .unwrap();
    fixture.publish(true, 3, None);
    let snapshot = request(&fixture.dry.path);
    let source = snapshot.prepared_map(0).unwrap();
    assert!(source.geometry().has_general_continuation());
    Python::attach(|py| {
        let code = CString::new(include_str!("runtime_policy_general_oracle.py")).unwrap();
        let module = PyModule::from_code(
            py,
            &code,
            c"runtime_policy_general_oracle.py",
            c"physical_large_oracle",
        )
        .unwrap();
        for first in [(1_u64 << 53) - 2, u64::MAX - 8192] {
            let mut timeline = TransportTimeline::new(RATE);
            assert!(
                timeline.set_master_period_and_anchor_beat_position_at_frame(MASTER, -7.25, first)
            );
            let mut queries = vec![
                first,
                first + 1,
                first + 2,
                first + 97,
                u64::MAX - 2049,
                u64::MAX - 1,
                u64::MAX,
            ];
            queries.sort_unstable();
            queries.dedup();
            let masters = queries
                .iter()
                .map(|frame| timeline.beat_position_at_frame(*frame).unwrap())
                .collect::<Vec<_>>();
            let master0 = timeline.beat_position_at_frame(first).unwrap();
            for fraction in [0.0_f64, -0.0_f64] {
                let raw = FractionalSourcePosition {
                    frame: 35,
                    fraction,
                    seek_mode: ExplicitSeekMode::Normal,
                };
                // Numeric queries reuse the original owner and scalar domain. They
                // do not manufacture an attack, adoption or history receipt at MAX.
                let copied = SourcePlayback::mapped_general_continuation(
                    source.geometry(),
                    91,
                    timeline,
                    original,
                    raw,
                    first,
                )
                .unwrap();
                assert_position(copied.position(), raw);
                let epoch = copied.mapped_epoch().unwrap();
                let expected = module
                    .getattr("evaluate")
                    .unwrap()
                    .call1((
                        fixture.anchors.clone(),
                        RATE,
                        START,
                        END - START,
                        None::<f64>,
                        raw.frame,
                        raw.fraction,
                        master0,
                        masters.clone(),
                    ))
                    .unwrap()
                    .extract::<Vec<(usize, u64, usize, Option<usize>, u32)>>()
                    .unwrap();
                for (frame, (whole, fraction_bits, left, right, gain_bits)) in
                    queries.iter().copied().zip(expected)
                {
                    let actual = source.geometry().position_at(epoch, frame).unwrap();
                    assert_eq!(actual.frame, whole, "E={first} n={frame}");
                    assert_eq!(
                        actual.fraction.to_bits(),
                        fraction_bits,
                        "E={first} n={frame}"
                    );
                    let plan = SourceReadPlan {
                        frame_pos: actual.frame,
                        ..read_plan(&fixture, 0, copied)
                    };
                    let taps = plan.try_fractional_taps(actual.fraction).unwrap();
                    assert_eq!(
                        (taps.left, taps.right, taps.right_gain.to_bits()),
                        (left, right, gain_bits)
                    );
                }
            }
        }
    });
}

#[test]
fn physical_unequal_consuming_partitions_keep_pcm_coordinates_and_nested_journal_slices() {
    let mut reference = physical(2, 23);
    let mut partitioned = physical(2, 23);
    let anchor = oracle_anchor(&partitioned, 0);
    reference.adopt(true, 3);
    partitioned.adopt(true, 3);
    let partitions = [1, 7, 511, 3, 1025, 2, 4097, 17, 2530];
    let frames = partitions.iter().sum();
    let expected = reference.dry.path.render(frames);
    let mut actual = Vec::with_capacity(expected.len());
    for frames in partitions {
        actual.extend(partitioned.dry.path.render(frames));
    }
    assert_eq!(actual, expected);
    for id in 0..2 {
        let a = reference.dry.path.voice(id).source_playback;
        let b = partitioned.dry.path.voice(id).source_playback;
        assert_position(a.position(), b.position());
        assert_eq!(a.effective_domain(), b.effective_domain());
        assert_eq!(a.original_scalar_domain(), b.original_scalar_domain());
    }
    verify_general_reads(&partitioned, anchor, 0);
}

#[test]
fn physical_productive_constructor_and_actual_renderer_fit_two_mib_without_rust_requests() {
    verify_general_two_mib(CROSS_ENDPOINT_ANCHORS, None);
}

#[test]
#[ignore = "actual physical-map productive Release profile; run explicitly with --release --ignored --nocapture"]
fn descriptive_physical_productive_constructor_renderer_cohorts_report_actual_cost_and_limits() {
    use crate::audio_engine::source_reader::{
        reset_tap_observation_for_test, tap_observation_for_test,
    };
    const REPEATS: usize = 3;
    let nominal = BLOCK as f64 / f64::from(RATE);
    for lanes in [2, 4, 6, 10] {
        let mut fixture = physical(lanes, 23);
        let mut seconds = [0.0_f64; REPEATS];
        let mut output = [0.0; BLOCK * 2];
        let mut taps_read = 0;
        for repeat in 0..=REPEATS {
            if repeat != 0 {
                fixture.adopt(false, (repeat * 2 + 2) as u64);
            }
            let anchor = oracle_anchor(&fixture, 0);
            fixture.publish(true, (repeat * 2 + 3) as u64, None);
            let snapshot = request(&fixture.dry.path);
            let mut retired = HeldRetirement::new(64);
            reset_tap_observation_for_test();
            let start = Instant::now();
            let ((_, queries), allocations) = observe_current_thread(|| {
                observe_compact_queries_for_test(|| {
                    drain_and_render(&mut fixture.dry, &mut output, &mut retired);
                })
            });
            let elapsed = start.elapsed().as_secs_f64();
            let taps = tap_observation_for_test();
            assert_eq!(allocations, ThreadAllocations::default());
            assert_eq!(queries.1, 0);
            assert_eq!(taps.missing_reads, 0);
            assert!(taps.left_reads >= lanes * BLOCK * 2);
            assert_eq!(
                snapshot.observation().unwrap().state,
                "applied",
                "{:?}",
                snapshot.observation()
            );
            assert_eq!(
                snapshot.observation().unwrap().execution_frame,
                Some(anchor.first)
            );
            for id in 0..lanes {
                assert!(
                    fixture
                        .dry
                        .path
                        .voice(id)
                        .source_playback
                        .reader_period()
                        .physical_quarters()
                        .is_some()
                );
            }
            assert!(output.iter().all(|sample| sample.is_finite()));
            assert!(output.iter().any(|sample| *sample != 0.0));
            verify_general_reads(&fixture, anchor, 0);
            if repeat != 0 {
                seconds[repeat - 1] = elapsed;
                taps_read += taps.left_reads + taps.right_reads;
            }
        }
        seconds.sort_by(f64::total_cmp);
        println!(
            "physical_general_productive_profile: shape=physical_decimal_both_endpoint_inverses_Q110_109_H1501_original_scalar1500 active={} fixed_capacity=32 frames_per_lane={} warmups=1 repeats={} median_ms={:.4} max_ms={:.4} nominal_512_at_48k_ms={:.7} median_interval_ratio={:.3} max_interval_ratio={:.3} within_nominal={} actual_tap_reads={} wide_entries=0; includes original queue drain, due-frame preflight/claim, physical actual-E construction, full dry renderer/DSP, history, copied advance, span refresh and checked physical-equivalent taps; fixture/map/oracle/output/sink allocation precedes timing; Rust allocation/zeroed/reallocation attempts=0, Native allocation/deallocation unobserved; independent original-knot Fraction pairs/taps/gain/PCM verified outside timing; declared-Q and physical2/P2.25 negative profiles remain separate; bounded generated-PCM workload, no worstcase/device/music/whole-RT acceptance",
            lanes,
            BLOCK,
            REPEATS,
            seconds[1] * 1000.0,
            seconds[2] * 1000.0,
            nominal * 1000.0,
            seconds[1] / nominal,
            seconds[2] / nominal,
            seconds[2] <= nominal,
            taps_read,
        );
    }
}
