//! Genuine dry policy publication with an independent original-knot Fraction oracle.
//! Generated PCM and accepted fixture labels do not establish music/device acceptance.

#[path = "runtime_policy_physical_tests.rs"]
mod physical_tests;

use super::*;
use crate::audio_engine::source_reader::SourceReaderPeriod;
use pyo3::types::PyModule;
use std::ffi::CString;

const START: usize = 1;
const END: usize = 1502;
const QUARTERS: f64 = 2.0;
const GENERAL_ANCHORS: &[(f64, f64)] = &[(0.0, 0.0), (0.001, 0.125), (0.03125, 2.0), (0.0625, 4.0)];

struct GeneralPath {
    dry: DryMappedPath,
    anchors: Vec<(f64, f64)>,
    quarters: Vec<Option<f64>>,
}

impl GeneralPath {
    fn new(lanes: usize, initial_frames: usize) -> Self {
        Self::with_extent(lanes, initial_frames, GENERAL_ANCHORS, Some(QUARTERS))
    }

    fn with_extent(
        lanes: usize,
        initial_frames: usize,
        anchors: &[(f64, f64)],
        quarters: Option<f64>,
    ) -> Self {
        let mut result = Self {
            dry: DryMappedPath::new(lanes, 0),
            anchors: anchors.to_vec(),
            quarters: vec![quarters; lanes],
        };
        for id in 0..lanes {
            result.dry.path.source.engine.stop_sample(id).unwrap();
            result.dry.path.drain();
        }
        result.replace_maps(anchors);
        result.publish(false, 2, None);
        for id in 0..lanes {
            let start = result
                .dry
                .path
                .source
                .engine
                .capture_pad_start(
                    id,
                    START as f64 / f64::from(RATE),
                    Some(END as f64 / f64::from(RATE)),
                    1.0,
                    false,
                    None,
                    None,
                )
                .unwrap();
            result.dry.path.drain();
            assert!(
                result
                    .dry
                    .path
                    .source
                    .engine
                    .publish_pad_start(&start, None)
                    .unwrap()
            );
            result.dry.path.drain();
            assert_eq!(start.state(), "applied", "{:?}", start.reason());
            assert!(!result.dry.path.voice(id).source_playback.is_mapped());
        }
        result.dry.path.render(initial_frames);
        result
    }

    fn replace_maps(&mut self, anchors: &[(f64, f64)]) {
        self.anchors = anchors.to_vec();
        for id in 0..self.dry.lanes {
            let (job, delivery) = self.dry.map_anchors(id, anchors);
            self.dry.jobs.push(job);
            self.dry.deliveries[id] = delivery;
        }
    }

    fn publish(&self, mapped: bool, revision: u64, bad_domain: Option<usize>) {
        self.publish_with_scalar(mapped, revision, bad_domain, None);
    }

    fn publish_with_scalar(
        &self,
        mapped: bool,
        revision: u64,
        bad_domain: Option<usize>,
        scalar_legacy: Option<usize>,
    ) {
        let bindings = (0..NUM_SAMPLES)
            .map(|id| {
                (id < self.dry.lanes).then(|| {
                    self.dry
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
        for (id, row) in rows.iter_mut().enumerate().take(self.dry.lanes) {
            *row = SourceMapRow {
                phase_origin_seconds: None,
                operational_four_quarter_bars: false,
                sync_requested: scalar_legacy != Some(id),
                intent_revision: self.dry.intent_revision,
                delivery: Some(self.dry.deliveries[id].clone()),
                ordinary_pending: false,
                exact_quarters: self.quarters[id],
            };
        }
        let mut ends = vec![Some(END as f64 / f64::from(RATE)); NUM_SAMPLES];
        if let Some(id) = bad_domain {
            ends[id] = Some((END - 3) as f64 / f64::from(RATE));
        }
        self.dry
            .path
            .source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_policy(
                true,
                (0..NUM_SAMPLES).map(|id| id < self.dry.lanes).collect(),
                vec![START as f64 / f64::from(RATE); NUM_SAMPLES],
                ends,
                bindings.iter().map(Option::as_ref).collect(),
                Some(
                    (0..NUM_SAMPLES)
                        .map(|id| PitchPadProjection {
                            sync: scalar_legacy != Some(id),
                            key_lock: false,
                            ..Default::default()
                        })
                        .collect(),
                ),
                Some(rows),
                GlobalTimingPolicy::parse(Some(&wire(true, mapped, revision).to_string())).unwrap(),
            )
            .unwrap();
    }

    fn adopt(&mut self, mapped: bool, revision: u64) -> Arc<RuntimePolicySnapshot> {
        self.publish(mapped, revision, None);
        let snapshot = request(&self.dry.path);
        service(&mut self.dry.path);
        assert_eq!(
            snapshot.observation().unwrap().state,
            "applied",
            "{:?}",
            snapshot.observation()
        );
        snapshot
    }
}

#[derive(Clone, Copy)]
struct OracleAnchor {
    raw: FractionalSourcePosition,
    first: u64,
    master: f64,
    timeline: TransportTimeline,
}

fn oracle_anchor(fixture: &GeneralPath, id: usize) -> OracleAnchor {
    let first = fixture.dry.path.timeline.output_frame();
    OracleAnchor {
        raw: fixture.dry.path.voice(id).source_playback.position(),
        first,
        master: fixture
            .dry
            .path
            .timeline
            .beat_position_at_frame(first)
            .unwrap(),
        timeline: fixture.dry.path.timeline,
    }
}

fn verify_general_reads(fixture: &GeneralPath, anchor: OracleAnchor, id: usize) {
    let mut history = SourceFeedSnapshot::new();
    assert!(
        fixture
            .dry
            .path
            .voice(id)
            .source_feed_history
            .capture_into(&mut history)
    );
    let voice = fixture.dry.path.voice(id);
    let sample = voice.sample.as_ref().unwrap();
    Python::attach(|py| {
        let code = CString::new(include_str!("runtime_policy_general_oracle.py")).unwrap();
        let module = PyModule::from_code(
            py,
            &code,
            c"runtime_policy_general_oracle.py",
            c"runtime_policy_general_oracle",
        )
        .unwrap();
        if let Some(quarters) = fixture.quarters[id] {
            let metadata = module
                .getattr("metadata")
                .unwrap()
                .call1((fixture.anchors.clone(), RATE, START, quarters))
                .unwrap()
                .extract::<(String, String, String, String)>()
                .unwrap();
            assert_ne!(
                metadata.1, "1",
                "the exact physical-start inverse is rational"
            );
            assert_ne!(metadata.3, "1", "the effective mapped period is rational");
        } else {
            let metadata = module
                .getattr("physical_metadata")
                .unwrap()
                .call1((fixture.anchors.clone(), RATE, START, END))
                .unwrap()
                .extract::<(String, String, String, String, String, String)>()
                .unwrap();
            let numerator = metadata.2.parse::<u128>().unwrap();
            let denominator = metadata.3.parse::<u128>().unwrap();
            assert!(!denominator.is_power_of_two(), "Q is genuinely non-dyadic");
            assert_eq!(metadata.4, (END - START).to_string());
            assert_eq!(metadata.5, "1");
            let quarters = voice
                .source_playback
                .reader_period()
                .physical_quarters()
                .unwrap();
            assert_eq!(
                quarters.parts(),
                (
                    crate::audio_engine::general_source_numerics::Integer::from_u128(numerator)
                        .unwrap(),
                    crate::audio_engine::general_source_numerics::Integer::from_u128(denominator)
                        .unwrap(),
                )
            );
        }
        let mut observed = 0;
        for span in history.spans().filter(|span| {
            span.playback.is_general_mapped()
                && span
                    .playback
                    .mapped_output_frame()
                    .is_some_and(|frame| frame >= anchor.first)
        }) {
            let first = span.playback.mapped_output_frame().unwrap();
            assert!(first >= anchor.first);
            let masters = (0..=span.frames)
                .map(|offset| {
                    anchor
                        .timeline
                        .beat_position_at_frame(first + offset as u64)
                        .unwrap()
                })
                .collect::<Vec<_>>();
            let expected = module
                .getattr("evaluate")
                .unwrap()
                .call1((
                    fixture.anchors.clone(),
                    RATE,
                    START,
                    END - START,
                    fixture.quarters[id],
                    anchor.raw.frame,
                    anchor.raw.fraction,
                    anchor.master,
                    masters,
                ))
                .unwrap()
                .extract::<Vec<(usize, u64, usize, Option<usize>, u32)>>()
                .unwrap();
            let mut expected_pcm = [vec![0.0; span.frames], vec![0.0; span.frames]];
            for (offset, (frame, fraction_bits, left, right, gain_bits)) in
                expected.into_iter().enumerate()
            {
                let actual = span.playback.position_at(offset).unwrap();
                assert_eq!(actual.frame, frame, "n={}", first + offset as u64);
                assert_eq!(
                    actual.fraction.to_bits(),
                    fraction_bits,
                    "n={}",
                    first + offset as u64
                );
                let plan = SourceReadPlan {
                    frame_pos: actual.frame,
                    ..span.plan
                };
                let taps = plan.try_fractional_taps(actual.fraction).unwrap();
                assert_eq!(taps.left, left);
                assert_eq!(taps.right, right);
                assert_eq!(taps.right_gain.to_bits(), gain_bits);
                if offset < span.frames {
                    for channel in 0..2 {
                        let a = sample.samples[(left - sample.resident_start()) * 2 + channel];
                        expected_pcm[channel][offset] = right.map_or(a, |right| {
                            let b = sample.samples[(right - sample.resident_start()) * 2 + channel];
                            a + (b - a) * f32::from_bits(gain_bits)
                        });
                    }
                }
            }
            for checked in [false, true] {
                let mut actual_pcm = [vec![17.0; span.frames], vec![19.0; span.frames]];
                let result = if checked {
                    span.plan.fill_fractional_buffers_checked_with_readers(
                        sample,
                        StemReadSets::default(),
                        &span.playback,
                        &mut actual_pcm,
                        span.frames,
                    )
                } else {
                    span.plan.fill_fractional_buffers_with_readers(
                        sample,
                        StemReadSets::default(),
                        &span.playback,
                        &mut actual_pcm,
                        span.frames,
                    )
                };
                result.unwrap();
                assert_eq!(actual_pcm, expected_pcm);
            }
            assert!(span.slice(0, span.frames).is_some());
            if span.frames > 2 {
                let nested = span.slice(1, span.frames - 1).unwrap().slice(1, 1).unwrap();
                let direct = span.slice(2, 1).unwrap();
                assert!(nested.playback.matches_exact(&direct.playback));
                assert!(nested.plan.matches_exact(direct.plan));
            }
            observed += span.frames;
        }
        assert!(observed > 0);
    });
}

#[test]
fn genuine_declared_decimal_map_switches_reader_domain_at_actual_e_with_exact_original_knot_pcm() {
    let mut fixture = GeneralPath::new(2, 23);
    let mut unchanged_reference = GeneralPath::new(2, 23);
    let anchor = oracle_anchor(&fixture, 0);
    let scalar_domain = fixture
        .dry
        .path
        .voice(0)
        .source_playback
        .effective_domain()
        .unwrap();
    let snapshot = fixture.adopt(true, 3);
    assert_eq!(
        snapshot.observation().unwrap().execution_frame,
        Some(anchor.first)
    );
    let voice = fixture.dry.path.voice(0);
    assert!(voice.source_playback.is_general_mapped());
    assert!(voice.source_playback.is_compact_mapped());
    assert_eq!(
        voice.source_playback.original_scalar_domain(),
        Some(scalar_domain)
    );
    assert_ne!(
        voice.source_playback.effective_domain(),
        Some(scalar_domain)
    );
    assert!(matches!(
        voice.source_playback.reader_period(),
        SourceReaderPeriod::MappedRational(_)
    ));
    assert_position(voice.source_playback.position(), anchor.raw);
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

#[test]
fn general_full_and_checked_readers_refuse_a_missing_actual_right_tap_before_output() {
    let mut fixture = GeneralPath::new(2, 23);
    fixture.adopt(true, 3);
    let voice = fixture.dry.path.voice(0);
    let playback = voice.source_playback;
    let position = playback.position();
    let plan = SourceReadPlan {
        channels: 2,
        sample_frames: PCM_FRAMES,
        frame_pos: position.frame,
        loop_region: FrameRange {
            start: START,
            end: END,
        },
        loop_period: playback.reader_period(),
        seek_mode: position.seek_mode,
        selection: StemRenderSelection::full_mix(),
        transition: StemTransition::default(),
    };
    let right = plan
        .try_fractional_taps(position.fraction)
        .unwrap()
        .right
        .unwrap();
    let sample = voice.sample.as_ref().unwrap();
    let incomplete = sample
        .window(
            sample.resident_start(),
            right,
            91,
            crate::messages::ResidentContext::FiniteLoop,
        )
        .unwrap();
    assert_eq!(incomplete.frame_count(), sample.frame_count());
    assert!(plan.tap_available_with_readers(
        &incomplete,
        StemReadSets::default(),
        position.frame,
        0
    ));
    assert!(!plan.tap_available_with_readers(&incomplete, StemReadSets::default(), right, 0));
    for checked in [false, true] {
        let mut output = [vec![17.0; 1], vec![19.0; 1]];
        let original = output.clone();
        let result = if checked {
            plan.fill_fractional_buffers_checked_with_readers(
                &incomplete,
                StemReadSets::default(),
                &playback,
                &mut output,
                1,
            )
        } else {
            plan.fill_fractional_buffers_with_readers(
                &incomplete,
                StemReadSets::default(),
                &playback,
                &mut output,
                1,
            )
        };
        assert!(result.is_err());
        assert_eq!(output, original);
    }
}

#[test]
fn general_copied_plan_layout_and_wrong_compatible_certificate_refuse_without_output_mutation() {
    use crate::audio_engine::general_source_numerics::{Integer, MappedReaderPeriod};
    use crate::audio_engine::source_reader::SourceLoopDomain;
    let mut fixture = GeneralPath::new(2, 23);
    fixture.adopt(true, 3);
    let voice = fixture.dry.path.voice(0);
    let playback = voice.source_playback;
    let position = playback.position();
    let domain = playback.effective_domain().unwrap();
    let source = voice.sample.as_ref().unwrap();
    let plan = SourceReadPlan {
        channels: source.channels,
        sample_frames: source.frame_count(),
        frame_pos: position.frame,
        loop_region: domain.region,
        loop_period: playback.reader_period(),
        seek_mode: position.seek_mode,
        selection: StemRenderSelection::full_mix(),
        transition: StemTransition::default(),
    };
    let wrong_period = MappedReaderPeriod::new(
        Integer::from_u128((END - START) as u128).unwrap(),
        Integer::from_u128(1).unwrap(),
    )
    .unwrap();
    let wrong_domain =
        SourceLoopDomain::mapped(source.frame_count(), domain.region, wrong_period).unwrap();
    assert_ne!(wrong_domain, domain);
    let mut unchanged_clock = playback;
    assert!(!unchanged_clock.configure_domain(wrong_domain));
    assert!(unchanged_clock.matches_exact(&playback));
    let wrong_plan = plan.with_effective_domain(wrong_domain).unwrap();
    assert!(!wrong_plan.matches_source_contract(plan));
    for checked in [false, true] {
        for case in 0..5 {
            let mut copied_plan = plan;
            let mut sample = source.clone();
            let mut output = [vec![17.0; 1], vec![19.0; 1]];
            match case {
                0 => output[1].clear(),
                1 => copied_plan.channels = 1,
                2 => copied_plan.sample_frames += 1,
                3 => copied_plan = wrong_plan,
                4 => sample.samples = Arc::from(Vec::<f32>::new()),
                _ => unreachable!(),
            }
            let original = output.clone();
            let result = if checked {
                copied_plan.fill_fractional_buffers_checked_with_readers(
                    &sample,
                    StemReadSets::default(),
                    &playback,
                    &mut output,
                    1,
                )
            } else {
                copied_plan.fill_fractional_buffers_with_readers(
                    &sample,
                    StemReadSets::default(),
                    &playback,
                    &mut output,
                    1,
                )
            };
            assert!(result.is_err(), "checked={checked} case={case}");
            assert_eq!(output, original);
        }
    }
}

#[test]
fn general_original_knot_fraction_oracle_retains_large_u64_master_plateaus_and_signed_zero() {
    let fixture = GeneralPath::new(1, 23);
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
            c"runtime_policy_general_large_oracle",
        )
        .unwrap();
        for first in [(1_u64 << 53) - 2, u64::MAX - 8192] {
            let mut timeline = TransportTimeline::new(RATE);
            assert!(
                timeline.set_master_period_and_anchor_beat_position_at_frame(MASTER, -7.25, first,)
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
                // Original owning map and the genuine scalar domain are reused
                // for NUMERIC queries only. No attack, adoption, read or history
                // receipt is manufactured at this artificial output epoch.
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
                        QUARTERS,
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
                        channels: 2,
                        sample_frames: PCM_FRAMES,
                        frame_pos: actual.frame,
                        loop_region: original.region,
                        loop_period: copied.reader_period(),
                        seek_mode: actual.seek_mode,
                        selection: StemRenderSelection::full_mix(),
                        transition: StemTransition::default(),
                    };
                    let taps = plan.try_fractional_taps(actual.fraction).unwrap();
                    assert_eq!(taps.left, left);
                    assert_eq!(taps.right, right);
                    assert_eq!(taps.right_gain.to_bits(), gain_bits);
                }
            }
        }
    });
}

#[test]
fn same_general_map_intent_keeps_the_complete_trajectory_and_original_scalar_circle() {
    let mut fixture = GeneralPath::new(2, 23);
    fixture.adopt(true, 3);
    fixture.dry.path.render(37);
    let before = fixture.dry.path.voice(0).source_playback;
    fixture.dry.intent_revision += 1;
    let snapshot = fixture.adopt(true, 4);
    let after = fixture.dry.path.voice(0).source_playback;
    assert!(before.matches_exact(&after));
    assert_eq!(
        after.original_scalar_domain(),
        before.original_scalar_domain()
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
        snapshot.prepared_map(0).unwrap(),
    ));
}

fn intersection_anchors(k: f64) -> [(f64, f64); 3] {
    [
        (0.0, 0.0),
        (1.0 / 32768.0, 375.0 / 131072.0),
        (k / 131072.0, 1025.0 / 512.0),
    ]
}

#[test]
fn general_then_two_intersection_replacements_then_scalar_retain_original_period_and_custody() {
    let mut fixture = GeneralPath::new(2, 23);
    let original = fixture
        .dry
        .path
        .voice(0)
        .source_playback
        .effective_domain()
        .unwrap();
    fixture.adopt(true, 3);
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
    for (revision, k) in [(4, 4101.0), (5, 4102.0)] {
        fixture.replace_maps(&intersection_anchors(k));
        fixture.publish(true, revision, None);
        let snapshot = request(&fixture.dry.path);
        assert!(
            !snapshot
                .prepared_map(0)
                .unwrap()
                .geometry()
                .has_general_continuation()
        );
        assert!(
            snapshot
                .prepared_map(0)
                .unwrap()
                .geometry()
                .has_compact_continuation()
        );
        let before = fixture.dry.path.voice(0).source_playback.position();
        drain_and_render(&mut fixture.dry, &mut [], &mut retired);
        assert_eq!(
            snapshot.observation().unwrap().state,
            "applied",
            "{:?}",
            snapshot.observation()
        );
        assert_position(fixture.dry.path.voice(0).source_playback.position(), before);
        assert_eq!(
            fixture
                .dry
                .path
                .voice(0)
                .source_playback
                .original_scalar_domain(),
            Some(original)
        );
    }
    assert!(
        retired
            .maps
            .iter()
            .any(|owner| Arc::ptr_eq(owner.effective_source().unwrap(), &first_owner))
    );
    let before = fixture.dry.path.voice(0).source_playback.position();
    fixture.adopt(false, 6);
    let scalar = fixture.dry.path.voice(0).source_playback;
    assert!(!scalar.is_mapped());
    assert_eq!(scalar.effective_domain(), Some(original));
    assert_position(scalar.position(), before);
}

#[test]
fn general_replacement_outside_historical_scalar_circle_succeeds_then_reverse_recovers_by_progress()
{
    let mut fixture = GeneralPath::new(2, 23);
    fixture.adopt(true, 3);
    let original = fixture
        .dry
        .path
        .voice(0)
        .source_playback
        .original_scalar_domain()
        .unwrap();
    assert_eq!(original.musical_period.scalar_period(), Some(1500.0));
    let mut found = false;
    for _ in 0..2048 {
        let playback = fixture.dry.path.voice(0).source_playback;
        if SourcePlayback::scalar_musical_continuation(
            playback.position(),
            original,
            playback.tempo_ratio(),
            playback.tempo_ratio(),
        )
        .is_none()
        {
            found = true;
            break;
        }
        fixture.dry.path.render(1);
    }
    assert!(
        found,
        "the independent extent has a genuinely visited extra seam interval"
    );
    let before = fixture.dry.path.voice(0).source_playback.position();
    // The added interior knot changes the original map but leaves both exact
    // boundary segments and therefore this rational seam/first PCM unchanged.
    fixture.replace_maps(&[
        (0.0, 0.0),
        (0.001, 0.125),
        (0.015625, 1.0),
        (0.03125, 2.0),
        (0.0625, 4.0),
    ]);
    fixture.adopt(true, 4);
    assert_position(fixture.dry.path.voice(0).source_playback.position(), before);
    assert_eq!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .original_scalar_domain(),
        Some(original)
    );
    fixture.publish(false, 5, None);
    let snapshot = automatic_mapped(&fixture.dry.path);
    let copies = scalar_state(&fixture.dry.path);
    service(&mut fixture.dry.path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&fixture.dry.path, &copies);
    let failed = snapshot.observation().unwrap();
    assert!(Arc::ptr_eq(&snapshot, &automatic_mapped(&fixture.dry.path)));
    assert_eq!(snapshot.observation().unwrap().attempt, failed.attempt + 1);
    service(&mut fixture.dry.path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    fixture.dry.path.render(2);
    let resumed = fixture.dry.path.voice(0).source_playback.position();
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
    assert_position(scalar.position(), resumed);
}

#[test]
fn scalar_to_general_first_seam_tap_refusal_retries_the_same_request_after_natural_wrap() {
    verify_general_seam_retry(GeneralPath::new(2, 23));
}

fn verify_general_seam_retry(mut fixture: GeneralPath) {
    fixture
        .dry
        .path
        .source
        .engine
        .seek_sample(0, (END - 3) as f64 / f64::from(RATE))
        .unwrap();
    fixture.dry.path.drain();
    fixture.dry.path.render(1);
    let raw = fixture.dry.path.voice(0).source_playback.position();
    assert_eq!(raw.frame, END - 2);
    assert!(raw.fraction > 0.0);
    fixture.publish(true, 3, None);
    let snapshot = automatic_mapped(&fixture.dry.path);
    assert_eq!(snapshot.map_preparation(0).class, RuntimeMapClass::Ready);
    let before = scalar_state(&fixture.dry.path);
    service(&mut fixture.dry.path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::PhaseChanged)
    );
    unchanged(&fixture.dry.path, &before);
    let failed = snapshot.observation().unwrap();
    assert!(Arc::ptr_eq(&snapshot, &automatic_mapped(&fixture.dry.path)));
    assert_eq!(snapshot.observation().unwrap().attempt, failed.attempt + 1);
    service(&mut fixture.dry.path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::PhaseChanged)
    );
    fixture.dry.path.render(1);
    let raw = fixture.dry.path.voice(0).source_playback.position();
    assert!(Arc::ptr_eq(&snapshot, &automatic_mapped(&fixture.dry.path)));
    service(&mut fixture.dry.path);
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    assert_position(fixture.dry.path.voice(0).source_playback.position(), raw);
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .is_general_mapped()
    );
}

#[test]
fn one_incompatible_original_domain_refuses_all_32_general_lanes_then_real_policy_repair_recovers()
{
    let mut fixture = GeneralPath::new(32, 23);
    let before = scalar_state(&fixture.dry.path);
    fixture.publish(true, 3, Some(31));
    let failed = request(&fixture.dry.path);
    service(&mut fixture.dry.path);
    assert_eq!(
        failed.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&fixture.dry.path, &before);
    fixture.adopt(true, 4);
    for id in 0..32 {
        assert!(
            fixture
                .dry
                .path
                .voice(id)
                .source_playback
                .is_general_mapped()
        );
    }
}

#[test]
fn heterogeneous_general_intersection_and_scalar_cohort_refuses_one_bad_member_then_repairs_atomically()
 {
    let mut fixture = GeneralPath::new(3, 23);
    let original = (0..3)
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
    let (job, delivery) = fixture.dry.map_anchors(1, &intersection_anchors(4101.0));
    fixture.dry.jobs.push(job);
    fixture.dry.deliveries[1] = delivery;
    // Global realtime intent remains enabled. The real per-pad BPM and source
    // SYNC requests are disabled for lane2, producing honest scalar Legacy mode.
    fixture.publish_with_scalar(true, 4, None, Some(2));
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
            .is_general_mapped()
    );
    assert!(
        fixture
            .dry
            .path
            .voice(1)
            .source_playback
            .is_compact_mapped()
    );
    assert!(
        !fixture
            .dry
            .path
            .voice(1)
            .source_playback
            .is_general_mapped()
    );
    assert!(!fixture.dry.path.voice(2).source_playback.is_mapped());
    assert_eq!(
        fixture.dry.path.voice(2).sync_request.mode,
        SourceSyncMode::Legacy
    );
    for id in 0..3 {
        assert_eq!(
            fixture
                .dry
                .path
                .voice(id)
                .source_playback
                .original_scalar_domain(),
            Some(original[id])
        );
    }
    let before = scalar_state(&fixture.dry.path);
    let owners = (0..2)
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
    fixture.publish_with_scalar(true, 5, Some(2), Some(2));
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
            owner,
        ));
    }
    // A real current publication repairs the last member. Newly owned request
    // witnesses must not restart either already effective mapped trajectory.
    fixture.publish_with_scalar(true, 6, None, Some(2));
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
            Some(original[id])
        );
    }
    for id in 0..2 {
        let voice = fixture.dry.path.voice(id);
        assert!(
            voice
                .mapped_start
                .as_ref()
                .unwrap()
                .original_launch()
                .is_none()
        );
        assert!(Arc::ptr_eq(
            voice
                .mapped_start
                .as_ref()
                .unwrap()
                .effective_source()
                .unwrap(),
            repaired.prepared_map(id).unwrap(),
        ));
    }
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .is_general_mapped()
    );
    assert!(
        fixture
            .dry
            .path
            .voice(1)
            .source_playback
            .is_compact_mapped()
    );
    assert!(
        !fixture
            .dry
            .path
            .voice(1)
            .source_playback
            .is_general_mapped()
    );
    assert!(!fixture.dry.path.voice(2).source_playback.is_mapped());
    assert_eq!(
        fixture.dry.path.voice(2).sync_request.mode,
        SourceSyncMode::Legacy
    );
}

#[test]
fn general_pause_hold_resume_master_change_and_journal_keep_the_effective_certificate() {
    verify_general_pause_hold(GeneralPath::new(2, 23));
}

fn verify_general_pause_hold(mut fixture: GeneralPath) {
    fixture.adopt(true, 3);
    let scalar = fixture
        .dry
        .path
        .voice(0)
        .source_playback
        .original_scalar_domain();
    let domain = fixture
        .dry
        .path
        .voice(0)
        .source_playback
        .effective_domain()
        .unwrap();
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
    fixture.dry.path.render(17);
    let before = fixture.dry.path.voice(0).source_playback.position();
    let pause = fixture.dry.path.source.engine.pause_sample(0).unwrap();
    fixture.dry.path.drain();
    assert!(
        pause
            .current_observation()
            .unwrap()
            .unwrap()
            .ordinary_paused()
    );
    fixture.dry.path.render(31);
    assert_position(fixture.dry.path.voice(0).source_playback.position(), before);
    fixture.dry.path.source.engine.resume_sample(0).unwrap();
    fixture.dry.path.drain();
    assert!(!fixture.dry.path.voice(0).paused);
    assert_position(fixture.dry.path.voice(0).source_playback.position(), before);
    fixture.dry.path.render(19);
    let before = fixture.dry.path.voice(0).source_playback.position();
    let ticket = Python::attach(|py| {
        fixture
            .dry
            .path
            .source
            .engine
            .pause_waveform_hold(0, 987_u64.into_pyobject(py).unwrap().as_any(), None)
            .unwrap()
    });
    fixture.dry.path.drain();
    assert!(ticket.is_current() && fixture.dry.path.voice(0).paused);
    fixture.dry.path.render(37);
    assert_position(fixture.dry.path.voice(0).source_playback.position(), before);
    assert!(
        fixture
            .dry
            .path
            .source
            .engine
            .release_waveform_hold(&ticket)
            .unwrap()
    );
    fixture.dry.path.drain();
    assert_eq!(ticket.publication_status(), "resumed");
    assert_position(fixture.dry.path.voice(0).source_playback.position(), before);
    let (producer, consumer) = rtrb::RingBuffer::new(16);
    fixture.dry.parameters = consumer;
    fixture.dry.path.source.engine.test_parameter_producer =
        Some(Arc::new(std::sync::Mutex::new(producer)));
    fixture
        .dry
        .path
        .source
        .engine
        .set_speed_and_master_period(1.25, MASTER * 1.25)
        .unwrap();
    fixture.dry.path.drain();
    drain_parameter_messages(
        &mut fixture.dry.parameters,
        &mut fixture.dry.path.source.mixer,
        &mut fixture.dry.path.timeline,
    );
    fixture.dry.path.render(0);
    assert_position(fixture.dry.path.voice(0).source_playback.position(), before);
    let playback = fixture.dry.path.voice(0).source_playback;
    assert!(
        playback
            .mapped_epoch()
            .unwrap()
            .matches_timeline(&fixture.dry.path.timeline)
    );
    assert_eq!(playback.original_scalar_domain(), scalar);
    assert_eq!(playback.effective_domain(), Some(domain));
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
        &owner,
    ));
    fixture.dry.path.render(1537);
    let mut history = SourceFeedSnapshot::new();
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_feed_history
            .capture_into(&mut history)
    );
    let mut general_frames = 0;
    for span in history
        .spans()
        .filter(|span| span.playback.is_general_mapped())
    {
        assert_eq!(span.plan.domain(), domain);
        assert_eq!(span.playback.original_scalar_domain(), scalar);
        let first = span.slice(0, 1).unwrap();
        let last = span.slice(span.frames - 1, 1).unwrap();
        assert_eq!(first.plan.loop_period, domain.musical_period);
        assert_eq!(last.plan.loop_period, domain.musical_period);
        general_frames += span.frames;
    }
    assert_eq!(general_frames, 17 + 19 + 1537);
    assert_eq!(history.active_frames(), (23 + 17 + 19 + 1537) as u64);
}

#[test]
fn general_original_map_cancellation_at_final_claim_refuses_all_lanes() {
    verify_general_claim_cancellation(GeneralPath::new(2, 23));
}

fn verify_general_claim_cancellation(mut fixture: GeneralPath) {
    fixture.publish(true, 3, None);
    let snapshot = request(&fixture.dry.path);
    fixture.dry.path.drain();
    drain_parameter_messages(
        &mut fixture.dry.parameters,
        &mut fixture.dry.path.source.mixer,
        &mut fixture.dry.path.timeline,
    );
    let before = scalar_state(&fixture.dry.path);
    // The initial DryMappedPath maps remain retained history. Cancel the actual
    // second generation currently selected by this general snapshot.
    let job = fixture.dry.jobs.remove(fixture.dry.lanes);
    crate::audio_engine::mixer::RtMixer::with_runtime_policy_before_claim_for_test(
        move || {
            job.cancel();
        },
        || {
            fixture.dry.path.render(0);
        },
    );
    unchanged(&fixture.dry.path, &before);
    assert!(!snapshot.map_current(0));
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Stale)
    );
    assert!(fixture.dry.path.voice(0).mapped_start.is_none());
}

#[test]
fn general_competing_global_lease_retains_exact_work_then_retries_at_later_actual_e() {
    verify_general_lease_contention(GeneralPath::new(2, 23));
}

fn verify_general_lease_contention(mut fixture: GeneralPath) {
    fixture.publish(true, 3, None);
    let snapshot = request(&fixture.dry.path);
    fixture.dry.path.drain();
    settle_parameters(&mut fixture.dry.path);
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
    unchanged(&fixture.dry.path, &before);
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    let ScheduledCommand::AdoptRuntimePolicy(work) =
        &fixture.dry.path.scheduler.deferred_due().unwrap().command
    else {
        panic!("the original general work must remain pending");
    };
    assert!(std::ptr::eq(&**work, snapshot.as_ref()));
    let attempt = snapshot.observation().unwrap().attempt;
    drop(lease);
    fixture.dry.path.render(7);
    let before = fixture.dry.path.voice(0).source_playback.position();
    fixture.dry.path.render(0);
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    assert_eq!(snapshot.observation().unwrap().attempt, attempt);
    assert_eq!(snapshot.observation().unwrap().execution_frame, Some(30));
    assert_position(fixture.dry.path.voice(0).source_playback.position(), before);
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_playback
            .is_general_mapped()
    );
}

#[test]
fn general_replacement_reserves_all_displaced_map_owners_before_effect_and_worker_retirement() {
    verify_general_retirement_pressure(GeneralPath::new(2, 23));
}

fn verify_general_retirement_pressure(mut fixture: GeneralPath) {
    fixture.adopt(true, 3);
    let original = fixture
        .dry
        .path
        .voice(0)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .clone();
    fixture.replace_maps(&[
        (0.0, 0.0),
        (0.001, 0.125),
        (0.015625, 1.0),
        (0.03125, 2.0),
        (0.0625, 4.0),
    ]);
    fixture.publish(true, 4, None);
    let snapshot = request(&fixture.dry.path);
    let before = scalar_state(&fixture.dry.path);
    let mut retired = HeldRetirement::new(2);
    drain_and_render(&mut fixture.dry, &mut [], &mut retired);
    unchanged(&fixture.dry.path, &before);
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    assert!(retired.maps.is_empty());
    let ScheduledCommand::AdoptRuntimePolicy(work) =
        &fixture.dry.path.scheduler.deferred_due().unwrap().command
    else {
        panic!("capacity pressure retains the exact original work");
    };
    assert!(std::ptr::eq(&**work, snapshot.as_ref()));
    let attempt = snapshot.observation().unwrap().attempt;
    fixture.dry.render_into(&mut [0.0; 14], &mut retired);
    let before = fixture.dry.path.voice(0).source_playback.position();
    retired.slots = 64;
    drain_and_render(&mut fixture.dry, &mut [], &mut retired);
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    assert_eq!(snapshot.observation().unwrap().attempt, attempt);
    assert_eq!(snapshot.observation().unwrap().execution_frame, Some(30));
    assert_position(fixture.dry.path.voice(0).source_playback.position(), before);
    assert_eq!(retired.maps.len(), 2);
    assert!(
        retired
            .maps
            .iter()
            .any(|owner| Arc::ptr_eq(owner.effective_source().unwrap(), &original))
    );
}

#[test]
fn general_productive_constructor_and_actual_renderer_fit_two_mib_without_rust_requests() {
    verify_general_two_mib(GENERAL_ANCHORS, Some(QUARTERS));
}

fn verify_general_two_mib(anchors: &'static [(f64, f64)], quarters: Option<f64>) {
    fn assert_copy<T: Copy>() {}
    assert_copy::<SourcePlayback>();
    assert_copy::<SourceReaderPeriod>();
    assert!(!std::mem::needs_drop::<SourcePlayback>());
    assert!(!std::mem::needs_drop::<SourceReaderPeriod>());
    assert!(
        std::mem::size_of::<crate::audio_engine::mapped_source_clock::MappedSourceSpan>() <= 288
    );
    assert!(std::mem::size_of::<[SourcePlayback; 32]>() <= 16 * 1024);
    assert!(std::mem::size_of::<[Option<SourcePlayback>; 32]>() <= 16 * 1024);
    println!(
        "general_layout: epoch={} span={} playback={} optional32={} reader_period={} source_domain={} read_plan={}; copied span carries no independent reader certificate/owner",
        std::mem::size_of::<crate::audio_engine::mapped_source_clock::MappedClockEpoch>(),
        std::mem::size_of::<crate::audio_engine::mapped_source_clock::MappedSourceSpan>(),
        std::mem::size_of::<SourcePlayback>(),
        std::mem::size_of::<[Option<SourcePlayback>; 32]>(),
        std::mem::size_of::<SourceReaderPeriod>(),
        std::mem::size_of::<crate::audio_engine::source_reader::SourceLoopDomain>(),
        std::mem::size_of::<SourceReadPlan>(),
    );
    let child = std::thread::Builder::new()
        .name("general-productive-two-mib".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let mut fixture = GeneralPath::with_extent(6, 23, anchors, quarters);
            let anchor = oracle_anchor(&fixture, 0);
            fixture.publish(true, 3, None);
            let snapshot = request(&fixture.dry.path);
            let mut output = [0.0; BLOCK * 2];
            let mut retired = HeldRetirement::new(64);
            let ((_, queries), allocations) = observe_current_thread(|| {
                observe_compact_queries_for_test(|| {
                    drain_and_render(&mut fixture.dry, &mut output, &mut retired);
                })
            });
            assert_eq!(queries.1, 0);
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
            assert!(output.iter().all(|sample| sample.is_finite()));
            assert!(output.iter().any(|sample| *sample != 0.0));
            for id in 0..6 {
                assert!(
                    fixture
                        .dry
                        .path
                        .voice(id)
                        .source_playback
                        .is_general_mapped()
                );
            }
            verify_general_reads(&fixture, anchor, 0);
            allocations
        })
        .unwrap();
    assert_eq!(child.join().unwrap(), ThreadAllocations::default());
}

#[test]
#[ignore = "actual general-map productive Release profile; run explicitly with --release --ignored --nocapture"]
fn descriptive_general_productive_constructor_renderer_cohorts_report_actual_cost_and_limits() {
    use crate::audio_engine::source_reader::{
        reset_tap_observation_for_test, tap_observation_for_test,
    };
    const REPEATS: usize = 3;
    let nominal = BLOCK as f64 / f64::from(RATE);
    // Ten is the unconditional synthetic stress cohort above the primary six.
    for lanes in [2, 4, 6, 10] {
        let mut fixture = GeneralPath::new(lanes, 23);
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
                        .is_general_mapped()
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
            "general_productive_profile: shape=declared_decimal_physical1501_original_scalar1500 active={} fixed_capacity=32 frames_per_lane={} warmups=1 repeats={} median_ms={:.4} max_ms={:.4} nominal_512_at_48k_ms={:.7} median_interval_ratio={:.3} max_interval_ratio={:.3} within_nominal={} actual_tap_reads={} wide_entries=0; includes original queue drain, due-frame preflight/claim, general actual-E construction, full dry renderer/DSP, history, copied advance, span refresh and exact rational tap preflight; fixture/map/oracle/output/sink allocation precedes timing; Rust allocation/zeroed/reallocation attempts=0, Native allocation/deallocation unobserved; independent original-knot Fraction pairs/taps/gain/PCM verified outside timing; existing physical2/P2.25 seam profile remains a separate required measurement; bounded generated-PCM workload, no worstcase/device/music/whole-RT acceptance",
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
