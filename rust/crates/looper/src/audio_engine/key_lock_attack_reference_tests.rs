use super::*;

const RATE: u32 = 48_000;
const FRAMES: usize = 16_384;
const EVENTS: [usize; 3] = [5632, 7168, 9408];

fn pattern(frames: usize, events: &[usize], delay: i64, gain: f64, frequency: f64) -> Vec<f32> {
    let mut result = vec![0.0_f32; frames];
    for (index, &event) in events.iter().enumerate() {
        for offset in 0..384 {
            let frame = event as i64 + delay + offset as i64;
            if !(0..frames as i64).contains(&frame) {
                continue;
            }
            let time = offset as f64 / f64::from(RATE);
            let envelope = (-6.0 * offset as f64 / 384.0).exp();
            let carrier = (std::f64::consts::TAU * frequency * time + 0.37).sin();
            result[frame as usize] +=
                (gain * (1.0 - index as f64 * 0.18) * envelope * carrier) as f32;
        }
    }
    result
}

fn reference() -> SourceAttackReference {
    SourceAttackReference {
        frame: EVENTS[1],
        support: FrameRange {
            start: 4096,
            end: 11_776,
        },
    }
}

fn search() -> AttackLagRange {
    AttackLagRange {
        min: -3072,
        max: 3072,
    }
}

fn candidate(result: AttackCorrespondence) -> AttackCorrespondenceCandidate {
    match result {
        AttackCorrespondence::SupportedCandidate(candidate) => candidate,
        other => panic!("expected a distinct feature candidate, received {other:?}"),
    }
}

#[test]
fn known_delayed_pattern_is_measured_without_gain_or_fitted_time_axis() {
    let dry = vec![pattern(FRAMES, &EVENTS, 0, 0.7, 1500.0)];
    for delay in [-384, 384] {
        let wet = vec![pattern(FRAMES, &EVENTS, delay, 0.21, 1500.0)];
        let measured = candidate(compare_attack_reference(
            &dry,
            &wet,
            RATE,
            reference(),
            search(),
        ));
        assert_eq!(measured.source_frame, EVENTS[1]);
        assert_eq!(measured.lag_frames, delay);
        assert_eq!(measured.wet_frame, EVENTS[1] as i64 + delay);
        assert!(measured.score > 0.99);
        assert!(measured.score - measured.runner_up_score >= MIN_INDEPENDENT_MATCH_MARGIN);
        assert_eq!(measured.feature_hop_frames, 48);
        assert_eq!(measured.feature_window_frames, 1024);
        assert!(measured.lag_support.min < delay - measured.feature_hop_frames as i64);
        assert!(measured.lag_support.max > delay + measured.feature_hop_frames as i64);
        assert_eq!(measured.revision, REVISION);
    }
}

#[test]
fn pitch_changed_carrier_uses_source_pattern_correspondence_not_pcm_peak() {
    let dry = vec![pattern(FRAMES, &EVENTS, 0, 0.7, 1500.0)];
    let wet = vec![pattern(FRAMES, &EVENTS, 384, 0.21, 3000.0)];
    let measured = candidate(compare_attack_reference(
        &dry,
        &wet,
        RATE,
        reference(),
        search(),
    ));
    assert!(measured.lag_support.min <= 384 && measured.lag_support.max >= 384);
    assert!((measured.lag_frames - 384).abs() <= measured.feature_hop_frames as i64);
}

#[test]
fn non_hop_delay_retains_broad_feature_support_instead_of_sample_precision() {
    let dry = vec![pattern(FRAMES, &EVENTS, 0, 0.7, 1500.0)];
    let wet = vec![pattern(FRAMES, &EVENTS, 407, 0.21, 1500.0)];
    let measured = candidate(compare_attack_reference(
        &dry,
        &wet,
        RATE,
        reference(),
        search(),
    ));
    assert!(measured.lag_support.min <= 407 && measured.lag_support.max >= 407);
    assert_eq!(measured.lag_frames % measured.feature_hop_frames as i64, 0);
    assert!(
        measured.lag_support.max - measured.lag_support.min
            >= 2 * measured.feature_window_frames as i64
    );
}

#[test]
fn antiphase_stereo_retains_the_same_measured_event_as_each_audible_channel() {
    let dry = pattern(FRAMES, &EVENTS, 0, 0.7, 1500.0);
    let wet = pattern(FRAMES, &EVENTS, 384, 0.21, 1500.0);
    let mono = candidate(compare_attack_reference(
        std::slice::from_ref(&dry),
        std::slice::from_ref(&wet),
        RATE,
        reference(),
        search(),
    ));
    let dry_opposite = dry.iter().map(|sample| -sample).collect();
    let wet_opposite = wet.iter().map(|sample| -sample).collect();
    let stereo = candidate(compare_attack_reference(
        &[dry, dry_opposite],
        &[wet, wet_opposite],
        RATE,
        reference(),
        search(),
    ));
    assert_eq!(stereo.lag_frames, mono.lag_frames);
    assert_eq!(stereo.lag_support, mono.lag_support);
    assert!((stereo.score - mono.score).abs() < 1.0e-10);
}

#[test]
fn silence_supplies_no_attack_or_zero_lag_fallback() {
    let silent = vec![vec![0.0; FRAMES]];
    let signal = vec![pattern(FRAMES, &EVENTS, 0, 0.7, 1500.0)];
    for (dry, wet) in [(&silent, &silent), (&silent, &signal), (&signal, &silent)] {
        assert!(matches!(
            compare_attack_reference(dry, wet, RATE, reference(), search()),
            AttackCorrespondence::Absent
        ));
    }
}

#[test]
fn two_equal_event_matches_remain_ambiguous_instead_of_choosing_the_first() {
    let frames = 32_768;
    let source_event = 16_384;
    let dry = vec![pattern(frames, &[source_event], 0, 0.7, 1500.0)];
    let wet = vec![pattern(
        frames,
        &[source_event - 4096, source_event + 4096],
        0,
        0.7,
        1500.0,
    )];
    let reference = SourceAttackReference {
        frame: source_event,
        support: FrameRange {
            start: 14_336,
            end: 18_432,
        },
    };
    assert!(matches!(
        compare_attack_reference(
            &dry,
            &wet,
            RATE,
            reference,
            AttackLagRange {
                min: -6144,
                max: 6144
            },
        ),
        AttackCorrespondence::Ambiguous
    ));
}

#[test]
fn match_at_requested_boundary_cannot_certify_an_uncensored_match() {
    let dry = vec![pattern(FRAMES, &EVENTS, 0, 0.7, 1500.0)];
    let wet = vec![pattern(FRAMES, &EVENTS, 3072, 0.21, 1500.0)];
    assert!(matches!(
        compare_attack_reference(&dry, &wet, RATE, reference(), search()),
        AttackCorrespondence::Ambiguous
    ));
}

#[test]
fn unavailable_bounds_nonfinite_input_and_excessive_work_are_refused() {
    let dry = vec![pattern(FRAMES, &EVENTS, 0, 0.7, 1500.0)];
    let wet = dry.clone();
    let bad_references = [
        SourceAttackReference {
            frame: EVENTS[1],
            support: FrameRange {
                start: 8000,
                end: 4000,
            },
        },
        SourceAttackReference {
            frame: FRAMES,
            support: FrameRange {
                start: 4096,
                end: 11_776,
            },
        },
        SourceAttackReference {
            frame: EVENTS[1],
            support: FrameRange {
                start: 4096,
                end: FRAMES + 1,
            },
        },
        SourceAttackReference {
            frame: 4096,
            support: FrameRange {
                start: 4096,
                end: 11_776,
            },
        },
    ];
    for reference in bad_references {
        assert!(matches!(
            compare_attack_reference(&dry, &wet, RATE, reference, search()),
            AttackCorrespondence::Unavailable
        ));
    }
    for search in [
        AttackLagRange { min: 1, max: -1 },
        AttackLagRange {
            min: i64::MIN,
            max: i64::MAX,
        },
        AttackLagRange {
            min: -5000,
            max: 3072,
        },
    ] {
        assert!(matches!(
            compare_attack_reference(&dry, &wet, RATE, reference(), search),
            AttackCorrespondence::Unavailable
        ));
    }
    let mut nonfinite = wet.clone();
    nonfinite[0][0] = f32::NAN;
    assert!(matches!(
        compare_attack_reference(&dry, &nonfinite, RATE, reference(), search()),
        AttackCorrespondence::Unavailable
    ));
    let excessive = vec![vec![0.0; MAX_FRAMES]; 1];
    assert!(matches!(
        compare_attack_reference(
            &excessive,
            &excessive,
            8000,
            SourceAttackReference {
                frame: 131_072,
                support: FrameRange {
                    start: 65_536,
                    end: 196_608
                }
            },
            AttackLagRange {
                min: -65_536,
                max: 65_536
            },
        ),
        AttackCorrespondence::Unavailable
    ));
    for rate in [0, 7999, 768001] {
        assert!(matches!(
            compare_attack_reference(&dry, &wet, rate, reference(), search()),
            AttackCorrespondence::Unavailable
        ));
    }
    let many_channels = vec![vec![0.0; FRAMES]; MAX_CHANNELS + 1];
    assert!(matches!(
        compare_attack_reference(&many_channels, &many_channels, RATE, reference(), search()),
        AttackCorrespondence::Unavailable
    ));
    let too_long = vec![vec![0.0; MAX_FRAMES + 1]];
    assert!(matches!(
        compare_attack_reference(&too_long, &too_long, RATE, reference(), search()),
        AttackCorrespondence::Unavailable
    ));
}
