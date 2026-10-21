use super::*;
use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
use crate::audio_engine::native_history_permit::NativeHistoryContext;
use crate::audio_engine::prepared_native_history::tests::fixture;
use crate::audio_engine::source_reader::{ExplicitSeekMode, StemRenderSelection, StemTransition};
use flitzis_looper_analysis::{AnalysisConfig, DetectionFunction};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

#[test]
fn receipt_has_no_drop_owners_and_scalar_geometry_matches_the_actual_detector() {
    assert!(!std::mem::needs_drop::<PreparedAttackMeasurement>());
    for rate in [44_100, 48_000, 96_000] {
        let request = fixture(2, rate, 0.73, ExplicitSeekMode::Normal, None);
        let geometry = MeasurementGeometry::new(&request, 4096, 512, 0).unwrap();
        let detector = DetectionFunction::new(
            rate,
            &AnalysisConfig {
                step_secs: FEATURE_STEP_SECONDS,
                max_bin_hz: FEATURE_BIN_HZ,
                ..AnalysisConfig::default()
            },
        );
        assert_eq!(geometry.feature_hop_frames, detector.step_size_samples());
        assert_eq!(
            geometry.feature_window_frames,
            detector.frame_length_samples()
        );
        assert_eq!(geometry.reference.frame, geometry.history_output_frames);
        assert_eq!(geometry.search, AttackLagRange { min: 0, max: 4096 });
        assert_eq!(geometry.capture_frames % 512, 0);
        assert!(geometry.capture_frames <= MAX_FRAMES);
        assert!(geometry.matches_request(&request));
        assert!(MeasurementGeometry::new(&request, MAX_FRAMES, 512, 0).is_none());
        assert!(MeasurementGeometry::new(&request, 4096, 0, 0).is_none());
    }
}

/// Algebraic interpolation and loop/intro addressing, without the product source reader.
fn independent_dry(
    request: &NativeHistoryRequest,
    geometry: MeasurementGeometry,
    stem_mask: Option<u8>,
) -> Vec<Vec<f32>> {
    let origin = geometry.source_origin.playback.position();
    let region = request.plan.loop_region;
    let ratio = request.playback.tempo_ratio();
    let address = |offset: usize| {
        let absolute = origin.frame + offset;
        match origin.seek_mode {
            ExplicitSeekMode::Normal => {
                region.start + (origin.frame - region.start + offset) % region.len()
            }
            ExplicitSeekMode::BeforeLoop if absolute < region.start => absolute,
            ExplicitSeekMode::BeforeLoop => region.start + (absolute - region.start) % region.len(),
            ExplicitSeekMode::AfterLoop if absolute < request.plan.sample_frames => absolute,
            ExplicitSeekMode::AfterLoop => {
                region.start + (absolute - request.plan.sample_frames) % region.len()
            }
        }
    };
    let read = |frame: usize, channel: usize| {
        stem_mask.map_or_else(
            || request.sample.samples[frame * request.plan.channels + channel],
            |mask| {
                request
                    .stems
                    .as_ref()
                    .unwrap()
                    .stems
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| mask & (1_u8 << *index) != 0)
                    .map(|(_, stem)| stem.samples[frame * request.plan.channels + channel])
                    .sum()
            },
        )
    };
    (0..request.plan.channels)
        .map(|channel| {
            (0..geometry.capture_frames)
                .map(|frame| {
                    if frame < geometry.silent_prefix_frames {
                        return 0.0;
                    }
                    let distance =
                        origin.fraction + (frame - geometry.silent_prefix_frames) as f64 * ratio;
                    let offset = distance.floor() as usize;
                    let left = read(address(offset), channel);
                    let right = read(address(offset + 1), channel);
                    left + (right - left) * distance.fract() as f32
                })
                .collect()
        })
        .collect()
}

#[test]
fn actual_source_and_uncropped_native_capture_match_independent_raw_reference() {
    for (ratio, mode, stem_mask) in [
        (0.73, ExplicitSeekMode::Normal, None),
        (1.37, ExplicitSeekMode::Normal, Some(0b1010)),
        (0.73, ExplicitSeekMode::BeforeLoop, None),
    ] {
        let request = fixture(2, 48_000, ratio, mode, stem_mask);
        let contract = RequestContract::capture(&request).unwrap();
        let captured = capture_request(&request, &contract, 0).unwrap();
        let dry = independent_dry(&request, captured.geometry, stem_mask);
        assert_eq!(captured.dry, dry);
        if mode == ExplicitSeekMode::BeforeLoop {
            assert!(captured.geometry.silent_prefix_frames > 0);
            for channel in &dry {
                assert!(
                    channel[..captured.geometry.silent_prefix_frames]
                        .iter()
                        .all(|sample| *sample == 0.0)
                );
            }
        }
        let pitch = request
            .pitch_intent
            .native_pitch_scale(ratio, true)
            .unwrap();
        let mut native = RubberBandLiveShifter::new(48_000, 2).unwrap();
        native.prepare_exact_pitch(pitch).unwrap();
        assert_eq!(
            native.source_history_frames(pitch).unwrap(),
            captured.geometry.history_output_frames
        );
        let block = native.block_size();
        let mut expected = vec![Vec::new(), Vec::new()];
        let mut output = vec![vec![0.0; block]; 2];
        for start in (0..captured.geometry.capture_frames).step_by(block) {
            let input = dry
                .iter()
                .map(|channel| channel[start..start + block].to_vec())
                .collect::<Vec<_>>();
            native.shift(&input, &mut output).unwrap();
            for channel in 0..2 {
                expected[channel].extend_from_slice(&output[channel]);
            }
        }
        assert_eq!(captured.wet, expected);
        assert!(contract.matches_request(&request));
        assert!(contract.logical.matches_exact(SourceReadCheckpoint {
            playback: request.playback,
            plan: request.plan,
        }));
    }
}

#[test]
fn actual_native_isolated_source_pattern_requires_a_supported_engineering_candidate() {
    const RATE: u32 = 48_000;
    const SOURCE_EVENT: usize = 172;
    const DRY_EVENT: usize = 4096;
    const BURST_FRAMES: usize = 384;
    // Freeze the source event and complete detector neighborhood before capturing wet PCM.
    // This is an isolated source-pattern engineering candidate, never principal/perceived
    // attack, musical C, dry-timing, retention or device/listening acceptance.
    let expected_reference = SourceAttackReference {
        frame: DRY_EVENT,
        support: FrameRange {
            start: DRY_EVENT - 1120,
            end: DRY_EVENT + 2240,
        },
    };
    let expected_search = AttackLagRange { min: 0, max: 4096 };
    let burst = (0..BURST_FRAMES)
        .map(|offset| {
            let time = offset as f64 / f64::from(RATE);
            let envelope = (-6.0 * offset as f64 / BURST_FRAMES as f64).exp();
            let carrier = (std::f64::consts::TAU * 1500.0 * time + 0.37).sin();
            (0.7 * envelope * carrier) as f32
        })
        .collect::<Vec<_>>();
    let mut request = fixture(1, RATE, 1.0, ExplicitSeekMode::Normal, None);
    assert_eq!(request.plan.frame_pos, SOURCE_EVENT);
    assert_eq!(request.playback.position().fraction, 0.0);
    assert_eq!(request.playback.tempo_ratio(), 1.0);
    assert_eq!(request.playback.rate_target(), 1.0);
    request.pitch_intent = PitchIntent::new(0, 1).unwrap();
    assert_ne!(
        request.pitch_intent.native_pitch_scale(1.0, true).unwrap(),
        1.0
    );
    let mut pcm = vec![0.0; request.sample.samples.len()];
    pcm[SOURCE_EVENT..SOURCE_EVENT + BURST_FRAMES].copy_from_slice(&burst);
    request.sample.samples = pcm.into();
    request.binding = ProductiveSourceBinding::new(&request.sample, RATE, None);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &request.sample, RATE, 13);
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let epoch = Arc::new(AtomicU64::new(7));
    request.permit = NativeHistoryContext {
        id: 0,
        ownership: &ownership,
        acknowledgements: &acknowledgements,
        preparation_epoch: &epoch,
    }
    .capture(&request.sample, RATE, None)
    .unwrap();

    let contract = RequestContract::capture(&request).unwrap();
    let captured = capture_request(&request, &contract, 0).unwrap();
    let geometry = captured.geometry;
    assert_eq!(geometry.reference.frame, expected_reference.frame);
    assert_eq!(geometry.reference.support, expected_reference.support);
    assert_eq!(geometry.search, expected_search);
    assert_eq!(geometry.silent_prefix_frames, 0);
    let region = request.plan.loop_region;
    let expected_origin = region.end - (DRY_EVENT - (SOURCE_EVENT - region.start));
    assert_eq!(
        geometry.source_origin.playback.position().frame,
        expected_origin
    );
    assert_eq!(geometry.source_origin.playback.position().fraction, 0.0);
    assert!(expected_origin > SOURCE_EVENT);
    assert!(geometry.capture_frames < region.len());
    // The sole burst occurs at the independently known dry coordinate, including the
    // source-loop wrap. No wet peak or fitted time axis participates in this oracle.
    let mut expected_dry = vec![0.0; geometry.capture_frames];
    expected_dry[DRY_EVENT..DRY_EVENT + BURST_FRAMES].copy_from_slice(&burst);
    assert_eq!(captured.dry, vec![expected_dry]);
    assert_eq!(captured.dry, independent_dry(&request, geometry, None));

    let measurement = measure_attack_reference_for_request(&request);
    assert!(measurement.matches_request(&request));
    let candidate = match measurement.result {
        AttackCorrespondence::SupportedCandidate(candidate) => candidate,
        other => panic!("isolated actual Native pattern requires a candidate, received {other:?}"),
    };
    assert_eq!(candidate.source_frame, DRY_EVENT);
    assert_eq!(candidate.wet_frame, DRY_EVENT as i64 + candidate.lag_frames);
    assert_eq!(candidate.feature_hop_frames, 48);
    assert_eq!(candidate.feature_window_frames, 1024);
    assert_eq!(candidate.revision, REVISION);
    assert!(geometry.validates_candidate(candidate));
    // The existing score, independent-margin and uncensored support rules remain intact.
    // No particular Native lag or musical acceptance is inferred from this candidate.
    assert!(candidate.score >= 0.75);
    assert!(candidate.score - candidate.runner_up_score >= 0.10);
    assert!(candidate.lag_support.min >= expected_search.min);
    assert!(candidate.lag_support.max <= expected_search.max);
}

#[test]
fn measurement_seals_original_source_readers_pitch_rate_and_action_coordinates() {
    let mut request = fixture(2, 48_000, 0.73, ExplicitSeekMode::Normal, Some(0b1010));
    let measurement = measure_attack_reference_for_request(&request);
    assert!(measurement.geometry.is_some());
    assert!(measurement.matches_request(&request));

    let playback = request.playback;
    assert!(request.playback.set_target(1.37));
    assert!(!measurement.matches_request(&request));
    request.playback = playback;
    let pitch = request.pitch_intent;
    request.pitch_intent = PitchIntent::new(0, 1).unwrap();
    assert!(!measurement.matches_request(&request));
    request.pitch_intent = pitch;
    request.key_lock = false;
    assert!(!measurement.matches_request(&request));
    request.key_lock = true;
    request.plan.frame_pos += 1;
    assert!(!measurement.matches_request(&request));
    request.plan.frame_pos -= 1;
    request.target_output_frame += 1;
    assert!(!measurement.matches_request(&request));
    request.target_output_frame -= 1;
    request.request_id += 1;
    assert!(!measurement.matches_request(&request));
    request.request_id -= 1;
    request.epoch += 1;
    assert!(!measurement.matches_request(&request));
    request.epoch -= 1;
    let identity = request
        .stems
        .as_ref()
        .unwrap()
        .complete_set_identity
        .clone();
    request.stems.as_mut().unwrap().complete_set_identity = Arc::new([8; 32]);
    assert!(!measurement.matches_request(&request));
    request.stems.as_mut().unwrap().complete_set_identity = identity;
    assert!(measurement.matches_request(&request));
}

#[test]
fn receipt_cannot_borrow_another_current_permit_with_equal_epoch_counters() {
    let mut request = fixture(1, 48_000, 0.73, ExplicitSeekMode::Normal, None);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &request.sample, 48_000, 13);
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let original_epoch = Arc::new(AtomicU64::new(7));
    let other_epoch = Arc::new(AtomicU64::new(7));
    let capture = |epoch: &Arc<AtomicU64>| {
        NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: epoch,
        }
        .capture(&request.sample, 48_000, None)
        .unwrap()
    };
    let original = capture(&original_epoch);
    let other = capture(&other_epoch);
    request.permit = original.clone();
    let measurement = measure_attack_reference_for_request(&request);
    assert!(measurement.matches_request(&request));
    request.permit = other;
    assert!(request.permit.current(&request.sample, 48_000));
    assert!(
        !measurement.matches_request(&request),
        "equal current counters must not relabel the original action's measurement"
    );
    request.permit = original;
    assert!(measurement.matches_request(&request));
}

#[test]
fn unsupported_trajectories_stay_unavailable_without_rejecting_exact_metadata() {
    let mut request = fixture(1, 48_000, 0.73, ExplicitSeekMode::Normal, None);
    let mut actual = SourceReadCheckpoint {
        playback: request.playback,
        plan: request.plan,
    };
    assert!(actual.project_output_frames(1));
    request.actual_feed = Some(actual);
    let offset = measure_attack_reference_for_request(&request);
    assert!(matches!(offset.result, AttackCorrespondence::Unavailable));
    assert!(offset.geometry.is_none());
    assert!(offset.matches_request(&request));
    request.actual_feed = None;
    request.plan.transition = StemTransition::start(StemRenderSelection::full_mix(), 128);
    let ramp = measure_attack_reference_for_request(&request);
    assert!(matches!(ramp.result, AttackCorrespondence::Unavailable));
    assert!(ramp.matches_request(&request));
    request.plan.transition.clear();
    assert!(request.playback.set_target(1.37));
    let rate = measure_attack_reference_for_request(&request);
    assert!(matches!(rate.result, AttackCorrespondence::Unavailable));
    assert!(rate.matches_request(&request));
}

#[test]
fn scalar_validation_refuses_malformed_candidate_and_geometry() {
    let request = fixture(1, 48_000, 0.73, ExplicitSeekMode::Normal, None);
    let geometry = MeasurementGeometry::new(&request, 4096, 512, 0).unwrap();
    // This synthetic receipt exercises scalar integrity only, never musical acceptance.
    let candidate = AttackCorrespondenceCandidate {
        source_frame: 4096,
        wet_frame: 6112,
        lag_frames: 2016,
        lag_support: AttackLagRange {
            min: 848,
            max: 3184,
        },
        score: 0.9,
        runner_up_score: 0.4,
        feature_hop_frames: 48,
        feature_window_frames: 1024,
        revision: REVISION,
    };
    let original = PreparedAttackMeasurement {
        context: MeasurementContext::SourceHistory,
        contract: RequestContract::capture(&request),
        geometry: Some(geometry),
        reference_kind: CAPTURED_ENDPOINT_REFERENCE_KIND,
        result: AttackCorrespondence::SupportedCandidate(candidate),
    };
    assert!(original.matches_request(&request));
    let tampers: [fn(&mut AttackCorrespondenceCandidate); 5] = [
        |candidate: &mut AttackCorrespondenceCandidate| candidate.score = f64::NAN,
        |candidate: &mut AttackCorrespondenceCandidate| candidate.wet_frame += 1,
        |candidate: &mut AttackCorrespondenceCandidate| candidate.lag_support.max = 5000,
        |candidate: &mut AttackCorrespondenceCandidate| candidate.feature_hop_frames = 1,
        |candidate: &mut AttackCorrespondenceCandidate| candidate.revision = "unrelated",
    ];
    for tamper in tampers {
        let mut altered = original;
        let mut changed_candidate = candidate;
        tamper(&mut changed_candidate);
        altered.result = AttackCorrespondence::SupportedCandidate(changed_candidate);
        assert!(!altered.matches_request(&request));
    }
    let mut altered = original;
    altered.geometry.as_mut().unwrap().capture_frames += 1;
    assert!(!altered.matches_request(&request));
}

#[test]
fn late_capture_uses_the_original_launch_epoch_after_its_initial_history() {
    let mut request = fixture(1, 48_000, 0.73, ExplicitSeekMode::Normal, None);
    let mut pcm = vec![1.0; request.sample.samples.len()];
    pcm[request.plan.loop_region.start] = 1.0e-8;
    request.sample.samples = pcm.into();
    request.binding = ProductiveSourceBinding::new(&request.sample, 48_000, None);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &request.sample, 48_000, 13);
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let epoch = Arc::new(AtomicU64::new(7));
    request.permit = NativeHistoryContext {
        id: 0,
        ownership: &ownership,
        acknowledgements: &acknowledgements,
        preparation_epoch: &epoch,
    }
    .capture(&request.sample, 48_000, None)
    .unwrap();
    let mut playback = super::super::source_playback::SourcePlayback::new(
        request.plan.loop_region.start,
        ExplicitSeekMode::Normal,
        0.73,
    );
    assert!(playback.configure(request.plan.sample_frames, request.plan.loop_region));
    let catchup = 8192;
    assert!(playback.advance(catchup));
    request.playback = playback;
    request.plan.frame_pos = playback.position().frame;
    let contract = RequestContract::capture(&request).unwrap();
    let captured = capture_request(&request, &contract, catchup as u64).unwrap();
    let original_t = captured.geometry.history_output_frames - catchup;
    assert_eq!(
        captured.dry[0][original_t], 1.0e-8,
        "rewound history must not replace the canonical launch epoch's direct first tap"
    );
}

#[test]
fn actual_silence_produces_absent_instead_of_a_zero_lag_guess() {
    let mut request = fixture(1, 48_000, 0.73, ExplicitSeekMode::Normal, None);
    request.sample.samples = vec![0.0; request.sample.samples.len()].into();
    request.binding = ProductiveSourceBinding::new(&request.sample, 48_000, None);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &request.sample, 48_000, 13);
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let epoch = Arc::new(AtomicU64::new(7));
    request.permit = NativeHistoryContext {
        id: 0,
        ownership: &ownership,
        acknowledgements: &acknowledgements,
        preparation_epoch: &epoch,
    }
    .capture(&request.sample, 48_000, None)
    .unwrap();
    let measurement = measure_attack_reference_for_request(&request);
    assert!(matches!(measurement.result, AttackCorrespondence::Absent));
    assert!(measurement.matches_request(&request));
}

fn physical_birth_fixture(ratio: f64, remote_tail: bool) -> NativeHistoryRequest {
    let mut request = fixture(2, 48_000, ratio, ExplicitSeekMode::Normal, None);
    let region = request.plan.loop_region;
    let mut pcm = vec![0.0; request.sample.samples.len()];
    for offset in 0..192 {
        // A unique nonperiodic beginning, fixed before any Native output is observed.
        let value = 0.16 + ((offset * offset * 17 + offset * 29 + 7) % 251) as f32 / 1024.0;
        pcm[(region.start + offset) * 2] = value;
        pcm[(region.start + offset) * 2 + 1] = -0.61 * value;
    }
    if remote_tail {
        for frame in region.end - 4096..region.end {
            let value = if frame % 2 == 0 { 0.27 } else { -0.19 };
            pcm[frame * 2] = value;
            pcm[frame * 2 + 1] = value * 0.43;
        }
    }
    request.sample.samples = pcm.into();
    request.binding = ProductiveSourceBinding::new(&request.sample, 48_000, None);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &request.sample, 48_000, 13);
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let epoch = Arc::new(AtomicU64::new(7));
    request.permit = NativeHistoryContext {
        id: 0,
        ownership: &ownership,
        acknowledgements: &acknowledgements,
        preparation_epoch: &epoch,
    }
    .capture(&request.sample, 48_000, None)
    .unwrap();
    request.playback = super::super::source_playback::SourcePlayback::new(
        region.start,
        ExplicitSeekMode::Normal,
        ratio,
    );
    assert!(
        request
            .playback
            .configure(request.plan.sample_frames, region)
    );
    request.plan.frame_pos = region.start;
    request.plan.seek_mode = ExplicitSeekMode::Normal;
    request
}

#[test]
fn birth_capture_uses_zero_negative_input_and_complete_independent_native_reference() {
    const HISTORY: usize = 4096;
    const CAPTURE: usize = 11_776;
    for ratio in [0.80, 1.0, 1.16] {
        let request = physical_birth_fixture(ratio, true);
        let original = SourceReadCheckpoint {
            playback: request.playback,
            plan: request.plan,
        };
        let contract = RequestContract::capture(&request).unwrap();
        let captured =
            capture_in_context(&request, &contract, 0, MeasurementContext::PhysicalBirth).unwrap();
        let geometry = captured.geometry;
        assert_eq!(geometry.context, MeasurementContext::PhysicalBirth);
        assert!(geometry.source_origin.matches_exact(original));
        assert_eq!(geometry.silent_prefix_frames, HISTORY);
        assert_eq!(geometry.history_output_frames, HISTORY);
        assert_eq!(geometry.launch_catchup_frames, 0);
        assert!(geometry.source_rebase.is_none());
        assert_eq!(geometry.reference.frame, HISTORY);
        assert_eq!(geometry.capture_frames, CAPTURE);
        assert!(geometry.matches_request(&request));
        assert!(
            MeasurementGeometry::in_context(
                &request,
                HISTORY,
                512,
                1,
                MeasurementContext::PhysicalBirth,
            )
            .is_none()
        );
        // Independently construct the whole input axis, including negative physical time.
        // No rewound DUT phase, wet peak, fitted offset or cropped output defines this input.
        let region = request.plan.loop_region;
        let dry = (0..2)
            .map(|channel| {
                (0..CAPTURE)
                    .map(|frame| {
                        if frame < HISTORY {
                            return 0.0;
                        }
                        let distance = (frame - HISTORY) as f64 * ratio;
                        let offset = distance.floor() as usize;
                        let left = region.start + offset % region.len();
                        let right = region.start + (offset + 1) % region.len();
                        let a = request.sample.samples[left * 2 + channel];
                        let b = request.sample.samples[right * 2 + channel];
                        a + (b - a) * distance.fract() as f32
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(captured.dry, dry);
        assert_eq!(
            captured.dry[0][HISTORY],
            request.sample.samples[region.start * 2]
        );
        let pitch = request
            .pitch_intent
            .native_pitch_scale(ratio, true)
            .unwrap();
        let mut native = RubberBandLiveShifter::new(48_000, 2).unwrap();
        native.prepare_exact_pitch(pitch).unwrap();
        assert_eq!(native.source_history_frames(pitch).unwrap(), HISTORY);
        let block = native.block_size();
        assert_eq!(block, 512);
        let mut wet = vec![Vec::new(), Vec::new()];
        let mut output = vec![vec![0.0; block]; 2];
        for start in (0..CAPTURE).step_by(block) {
            let input = dry
                .iter()
                .map(|channel| channel[start..start + block].to_vec())
                .collect::<Vec<_>>();
            native.shift(&input, &mut output).unwrap();
            for channel in 0..2 {
                wet[channel].extend_from_slice(&output[channel]);
            }
        }
        assert_eq!(captured.wet, wet);
        assert!(wet.iter().flatten().any(|sample| sample.abs() > 1.0e-4));
        assert!(contract.matches_request(&request));
        assert!(original.matches_exact(SourceReadCheckpoint {
            playback: request.playback,
            plan: request.plan,
        }));
    }
}

#[test]
fn birth_capture_and_receipts_do_not_borrow_the_unread_remote_loop_tail() {
    for ratio in [0.80, 1.16] {
        let first = physical_birth_fixture(ratio, false);
        let other = physical_birth_fixture(ratio, true);
        let capture = |request: &NativeHistoryRequest, context| {
            capture_in_context(
                request,
                &RequestContract::capture(request).unwrap(),
                0,
                context,
            )
            .unwrap()
        };
        let a = capture(&first, MeasurementContext::PhysicalBirth);
        let b = capture(&other, MeasurementContext::PhysicalBirth);
        assert!(
            a.geometry
                .source_origin
                .matches_exact(SourceReadCheckpoint {
                    playback: first.playback,
                    plan: first.plan,
                })
        );
        assert!(
            b.geometry
                .source_origin
                .matches_exact(SourceReadCheckpoint {
                    playback: other.playback,
                    plan: other.plan,
                })
        );
        // The complete positive capture never reaches either distinguishable remote tail.
        let last_source = first.plan.loop_region.start
            + ((a.geometry.capture_frames - a.geometry.silent_prefix_frames) as f64 * ratio).ceil()
                as usize;
        assert!(last_source < first.plan.loop_region.end - 4096);
        assert_eq!(a.dry, b.dry);
        assert_eq!(a.wet, b.wet);
        let generic_a = capture(&first, MeasurementContext::SourceHistory);
        let generic_b = capture(&other, MeasurementContext::SourceHistory);
        assert_ne!(
            &generic_a.dry[0][..generic_a.geometry.history_output_frames],
            &generic_b.dry[0][..generic_b.geometry.history_output_frames],
            "the control must expose the existing wrapped-history distinction"
        );
        let receipt = measure_attack_reference_for_birth(&first);
        assert_eq!(receipt.reference_kind, PHYSICAL_BIRTH_REFERENCE_KIND);
        assert!(receipt.geometry.is_some());
        assert!(receipt.matches_birth_request(&first));
        assert!(!receipt.matches_request(&first));
        assert!(!receipt.matches_birth_request(&other));
        let generic = measure_attack_reference_for_request(&first);
        assert!(generic.matches_request(&first));
        assert!(!generic.matches_birth_request(&first));
        // Identical sound does not make different source ownership interchangeable.
        assert_ne!(first.binding, other.binding);
    }
}

#[test]
fn birth_unavailable_metadata_is_truthful_mode_specific_and_bound_to_the_original_action() {
    for unsupported in 0..3 {
        let mut request = physical_birth_fixture(0.80, false);
        match unsupported {
            0 => {
                let mut actual = SourceReadCheckpoint {
                    playback: request.playback,
                    plan: request.plan,
                };
                assert!(actual.project_output_frames(1));
                request.actual_feed = Some(actual);
            }
            1 => {
                request.plan.transition =
                    StemTransition::start(StemRenderSelection::full_mix(), 128)
            }
            2 => assert!(request.playback.set_target(1.16)),
            _ => unreachable!(),
        }
        let birth = measure_attack_reference_for_birth(&request);
        let generic = measure_attack_reference_for_request(&request);
        assert!(matches!(birth.result, AttackCorrespondence::Unavailable));
        assert!(birth.geometry.is_none());
        assert!(birth.matches_birth_request(&request));
        assert!(!birth.matches_request(&request));
        assert!(generic.matches_request(&request));
        assert!(!generic.matches_birth_request(&request));
        request.target_output_frame += 1;
        assert!(!birth.matches_birth_request(&request));
        request.target_output_frame -= 1;
        request.request_id += 1;
        assert!(!birth.matches_birth_request(&request));
        request.request_id -= 1;
        request.epoch += 1;
        assert!(!birth.matches_birth_request(&request));
        request.epoch -= 1;
        let pitch = request.pitch_intent;
        request.pitch_intent = PitchIntent::new(0, 1).unwrap();
        assert!(!birth.matches_birth_request(&request));
        request.pitch_intent = pitch;
        assert!(birth.matches_birth_request(&request));
        let mut relabelled = generic;
        relabelled.reference_kind = PHYSICAL_BIRTH_REFERENCE_KIND;
        assert!(!relabelled.matches_birth_request(&request));
        relabelled = birth;
        relabelled.reference_kind = CAPTURED_ENDPOINT_REFERENCE_KIND;
        assert!(!relabelled.matches_birth_request(&request));
    }
}

#[test]
fn birth_receipt_retains_current_permit_and_scalar_context_guards() {
    let mut request = physical_birth_fixture(0.80, false);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &request.sample, 48_000, 13);
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let epoch = Arc::new(AtomicU64::new(7));
    let other_epoch = Arc::new(AtomicU64::new(7));
    let capture = |preparation_epoch: &Arc<AtomicU64>| {
        NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch,
        }
        .capture(&request.sample, 48_000, None)
        .unwrap()
    };
    let original = capture(&epoch);
    let other = capture(&other_epoch);
    request.permit = original.clone();
    let receipt = measure_attack_reference_for_birth(&request);
    assert!(receipt.matches_birth_request(&request));
    assert!(receipt.geometry.is_some());
    let mut malformed = receipt;
    malformed.geometry.as_mut().unwrap().context = MeasurementContext::SourceHistory;
    assert!(!malformed.matches_birth_request(&request));
    malformed = receipt;
    malformed.geometry.as_mut().unwrap().silent_prefix_frames -= 1;
    assert!(!malformed.matches_birth_request(&request));
    request.permit = other;
    assert!(request.permit.current(&request.sample, 48_000));
    assert!(!receipt.matches_birth_request(&request));
    request.permit = original;
    assert!(receipt.matches_birth_request(&request));
}
