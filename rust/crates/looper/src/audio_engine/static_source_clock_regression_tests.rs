//! Exact static-clock expectations generated independently with Python Fraction.
//! These coordinates convey no source custody, renderer receipt or music acceptance.

use crate::audio_engine::source_playback::{FractionalSourcePosition, SourcePlayback};
use crate::audio_engine::source_reader::{FrameRange, SourceLoopDomain};
use crate::audio_engine::transport::TransportTimeline;
use crate::audio_engine::voice_slot::ExplicitSeekMode;

const RATE: u32 = 48_000;
type Expected = (u64, u64, usize, u64, bool);

#[derive(Clone, Copy)]
struct Case {
    f: f64,
    master_f: f64,
    origin: f64,
    start: usize,
    length: usize,
    whole: usize,
    fraction: f64,
    first: u64,
    anchor_frame: u64,
    anchor: f64,
}

fn timeline(case: Case) -> TransportTimeline {
    let mut result = TransportTimeline::new(RATE);
    assert!(result.set_master_period_and_anchor_beat_position_at_frame(
        case.master_f / f64::from(RATE),
        case.anchor,
        case.anchor_frame,
    ));
    result.complete_bootstrap();
    assert_eq!(
        result.frames_per_beat().unwrap().to_bits(),
        case.master_f.to_bits()
    );
    result
}

fn preceding(case: Case) -> SourcePlayback {
    let region = FrameRange {
        start: case.start,
        end: case.start + case.length,
    };
    SourcePlayback::scalar_continuation(
        FractionalSourcePosition {
            frame: case.whole,
            fraction: case.fraction,
            seek_mode: ExplicitSeekMode::Normal,
        },
        SourceLoopDomain::physical(region.end + 1, region),
        case.f / case.master_f,
        case.f / case.master_f,
    )
    .unwrap()
}

fn verify(case: Case, expected: &[Expected]) {
    let master = timeline(case);
    let preceding = preceding(case);
    let source = SourcePlayback::static_continuation(
        &preceding,
        master.beat_projection().unwrap(),
        case.first,
        77,
        case.f,
        case.origin,
        case.length as f64 / case.f,
    )
    .unwrap();
    assert!(source.is_static_phase());
    assert!(!source.is_mapped());
    assert_eq!(source.static_first_frame(), Some(case.first));
    assert_eq!(source.static_output_frame(), Some(case.first));
    assert_eq!(source.static_marker(), Some(77));
    assert!(source.static_matches_projection(master.beat_projection().unwrap()));
    assert!(source.static_grid_matches(case.f, case.origin, case.length as f64 / case.f));
    assert_eq!(source.effective_domain(), preceding.effective_domain());
    assert_eq!(source.position().frame, preceding.position().frame);
    assert_eq!(
        source.position().fraction.to_bits(),
        preceding.position().fraction.to_bits()
    );
    for &(frame, master_bits, whole, fraction_bits, captured) in expected {
        assert_eq!(
            master.beat_position_at_frame(frame).unwrap().to_bits(),
            master_bits
        );
        let offset = usize::try_from(frame - case.first).unwrap();
        let actual = source.position_at(offset).unwrap();
        assert_eq!(
            (actual.frame, actual.fraction.to_bits()),
            (whole, fraction_bits),
            "frame={frame}"
        );
        let mut copied = source;
        assert!(copied.advance(offset));
        assert_eq!(copied.static_output_frame(), Some(frame));
        assert_eq!(
            (
                copied.position().frame,
                copied.position().fraction.to_bits()
            ),
            (whole, fraction_bits)
        );
        assert_eq!(copied.static_phase_settled(), captured, "frame={frame}");
        assert!(!copied.has_constant_scalar_trajectory());
        assert!(copied.history_before_constant(17).is_none());
        assert!(copied.shifted_normal_history(0.0).is_none());
    }
}

fn ordinary(length: usize) -> Case {
    Case {
        f: 46.875,
        master_f: 46.875,
        origin: -12.0,
        start: 13,
        length,
        whole: 36,
        fraction: 0.0,
        first: 23,
        anchor_frame: 0,
        anchor: 0.0,
    }
}

#[test]
fn independent_fraction_oracle_preserves_off_grid_origins_captures_and_unequal_cycles() {
    verify(ordinary(1500), OFF_GRID_Q32);
    verify(ordinary(12000), OFF_GRID_Q256);
    verify(
        Case {
            f: 64.0,
            master_f: 64.0,
            ..ordinary(2048)
        },
        DENSE_Q32,
    );
    verify(
        Case {
            f: 64.0,
            master_f: 64.0,
            ..ordinary(16384)
        },
        DENSE_Q256,
    );
}

#[test]
fn independent_fraction_oracle_selects_the_feasible_winding_and_positive_tie() {
    let case = Case {
        f: 32.0,
        master_f: 64.0,
        origin: 0.0,
        start: 0,
        length: 1024,
        whole: 1,
        fraction: 0.0,
        first: 0,
        anchor_frame: 0,
        anchor: 0.0,
    };
    verify(case, LOWER_RATE_POSITIVE_WINDING);
    verify(
        Case {
            f: 64.0,
            master_f: 32.0,
            ..case
        },
        UPPER_RATE_NEGATIVE_WINDING,
    );
    verify(
        Case {
            master_f: 32.0,
            whole: 512,
            ..case
        },
        EQUAL_DISTANCE_POSITIVE_WINDING,
    );
}

#[test]
fn independent_fraction_oracle_keeps_actual_master_cast_plateaus_and_raw_negative_zero() {
    verify(
        Case {
            f: 32.0,
            master_f: 32.0,
            origin: -7.0,
            start: 3,
            length: 1024,
            whole: 17,
            fraction: 0.25,
            first: (1_u64 << 53) + 1,
            anchor_frame: (1_u64 << 53) - 32,
            anchor: -3.125,
        },
        LARGE_MASTER_CAST_PLATEAUS,
    );
    verify(
        Case {
            f: 32.0,
            master_f: 32.0,
            origin: 0.0,
            start: 0,
            length: 1024,
            whole: 0,
            fraction: -0.0,
            first: 0,
            anchor_frame: 0,
            anchor: 0.0,
        },
        NEGATIVE_ZERO_IDENTITY,
    );
}

#[test]
fn static_exact_numeric_refusals_preserve_the_preceding_scalar_value() {
    let case = Case {
        f: 32.0,
        master_f: 32.0,
        origin: 0.0,
        start: 0,
        length: 1024,
        whole: 1,
        fraction: 0.0,
        first: 0,
        anchor_frame: 0,
        anchor: 0.0,
    };
    let preceding = preceding(case);
    let projection = timeline(case).beat_projection().unwrap();
    for (f, origin, quarters, first, marker) in [
        (32.0, f64::NAN, 32.0, 0, 7),
        (0.0, 0.0, 32.0, 0, 7),
        (32.0, 0.0, 31.0, 0, 7),
        (32.0, 0.0, 32.0, 0, 0),
        (32.0, 0.0, 32.0, u64::MAX, 7),
        (32.0, 2.0_f64.powi(-126), 32.0, 0, 7),
    ] {
        assert!(
            SourcePlayback::static_continuation(
                &preceding, projection, first, marker, f, origin, quarters
            )
            .is_err()
        );
        assert!(!preceding.is_static_phase());
        assert_eq!(preceding.position().frame, 1);
        assert_eq!(preceding.position().fraction.to_bits(), 0.0_f64.to_bits());
    }
    let mut gliding = preceding;
    assert!(gliding.set_target(1.25));
    assert!(
        SourcePlayback::static_continuation(&gliding, projection, 0, 7, 32.0, 0.0, 32.0).is_err()
    );
}

#[test]
fn copied_static_history_never_becomes_constant_at_unity_after_capture() {
    let case = Case {
        f: 32.0,
        master_f: 32.0,
        origin: 0.0,
        start: 0,
        length: 1024,
        whole: 1,
        fraction: 0.0,
        first: 0,
        anchor_frame: 0,
        anchor: 0.0,
    };
    let preceding = preceding(case);
    let mut source = SourcePlayback::static_continuation(
        &preceding,
        timeline(case).beat_projection().unwrap(),
        0,
        7,
        32.0,
        0.0,
        32.0,
    )
    .unwrap();
    assert!(source.advance(128));
    assert!(source.static_phase_settled());
    assert_eq!(source.tempo_ratio(), 1.0);
    assert_eq!(source.rate_target(), 1.0);
    assert!(!source.has_constant_scalar_trajectory());
    assert!(source.history_before_constant(128).is_none());
    assert!(source.shifted_normal_history(0.0).is_none());
    let raw = source.position();
    assert!(source.dissolve_static());
    assert!(!source.is_static_phase());
    assert_eq!(
        (
            source.position().frame,
            source.position().fraction.to_bits()
        ),
        (raw.frame, raw.fraction.to_bits())
    );
}

#[test]
fn static_program_identity_and_equal_scalar_controls_preserve_the_complete_clock() {
    let case = Case {
        f: 32.0,
        master_f: 32.0,
        origin: 0.0,
        start: 0,
        length: 1024,
        whole: 1,
        fraction: 0.0,
        first: 0,
        anchor_frame: 0,
        anchor: 0.0,
    };
    let preceding = preceding(case);
    let projection = timeline(case).beat_projection().unwrap();
    let mut source =
        SourcePlayback::static_continuation(&preceding, projection, 0, 7, 32.0, 0.0, 32.0).unwrap();
    let other =
        SourcePlayback::static_continuation(&preceding, projection, 0, 8, 32.0, 0.0, 32.0).unwrap();
    assert!(!source.matches_exact(&other));
    assert!(!source.matches_rate_target(&other));
    assert!(source.advance(17));
    let before = source;
    assert!(source.set_target(1.0));
    assert!(source.configure_domain(before.effective_domain().unwrap()));
    assert!(source.matches_exact(&before));
    assert!(!source.static_grid_matches(32.0, 1.0, 32.0));
    assert!(!source.static_grid_matches(32.0, 0.0, 16.0));
    let position = source.position();
    assert!(source.set_target(1.25));
    assert!(!source.is_static_phase());
    assert_eq!(
        (
            source.position().frame,
            source.position().fraction.to_bits()
        ),
        (position.frame, position.fraction.to_bits())
    );
}
// Generated by the retained independent Fraction oracle; do not hand-fit values.
// off_grid_q32: exact debt=['-3602879701896396981', '144115188075855872'], k=6
const OFF_GRID_Q32: &[(u64, u64, usize, u64, bool)] = &[
    (23, 4602510684786558413, 36, 0, false),
    (24, 4602786905563703804, 36, 4607041681311662096, false),
    (63, 4608731657071832859, 75, 4600427019358961756, false),
    (511, 4622326523146988596, 516, 4600427019358961860, false),
    (1023, 4626836127573862253, 1020, 4600427019358963033, false),
    (1535, 4629805500928175200, 24, 4600427019358964206, false),
    (1622, 4630066709706562688, 110, 4580160821035730633, false),
    (1623, 4630069712106314269, 111, 4413175791102197760, true),
    (1624, 4630072714506065849, 111, 4607182418800017400, true),
    (2047, 4631342729600984329, 535, 4406631497893675008, true),
    (4095, 4635847830428230615, 1082, 4607182418800017168, true),
    (12023, 4643219847718267169, 1510, 4607182418800006592, true),
    (24023, 4647719131395994769, 1511, 4429782814728126464, true),
    (48023, 4652220573048543816, 1510, 4607182418799982592, true),
];
// off_grid_q256: exact debt=['-3602879701896396981', '144115188075855872'], k=6
const OFF_GRID_Q256: &[(u64, u64, usize, u64, bool)] = &[
    (23, 4602510684786558413, 36, 0, false),
    (24, 4602786905563703804, 36, 4607041681311662096, false),
    (63, 4608731657071832859, 75, 4600427019358961756, false),
    (511, 4622326523146988596, 516, 4600427019358961860, false),
    (1023, 4626836127573862253, 1020, 4600427019358963033, false),
    (1535, 4629805500928175200, 1524, 4600427019358964206, false),
    (1622, 4630066709706562688, 1610, 4580160821035730633, false),
    (1623, 4630069712106314269, 1611, 4413175791102197760, true),
    (1624, 4630072714506065849, 1611, 4607182418800017400, true),
    (2047, 4631342729600984329, 2035, 4406631497893675008, true),
    (4095, 4635847830428230615, 4082, 4607182418800017168, true),
    (12023, 4643219847718267169, 12010, 4607182418800006592, true),
    (24023, 4647719131395994769, 12011, 4429782814728126464, true),
    (48023, 4652220573048543816, 12010, 4607182418799982592, true),
];
// lower_rate_positive_winding: exact debt=['1023', '1'], k=6
const LOWER_RATE_POSITIVE_WINDING: &[(u64, u64, usize, u64, bool)] = &[
    (0, 0, 1, 0, false),
    (1, 4580160821035794432, 1, 4602749187916824576, false),
    (31, 4602397344195936256, 16, 4604860250242154496, false),
    (32, 4602678819172646912, 17, 4598175219545276416, false),
    (33, 4602819556661002240, 17, 4605000987730509824, false),
    (63, 4607041681311662080, 32, 4607112050055839744, false),
    (64, 4607182418800017408, 33, 4602678819172646912, false),
    (1023, 4625188021216477184, 520, 4602538081684291584, false),
    (32767, 4647714540568444928, 256, 4602538081684291584, false),
    (32768, 4647714815446351872, 257, 0, false),
    (65536, 4652218415073722368, 513, 0, false),
    (131000, 4656717066898767872, 988, 0, true),
    (131072, 4656722014701092864, 0, 0, true),
];
// upper_rate_negative_winding: exact debt=['-1', '1'], k=6
const UPPER_RATE_NEGATIVE_WINDING: &[(u64, u64, usize, u64, bool)] = &[
    (0, 0, 1, 0, false),
    (1, 4584664420663164928, 2, 4606900943823306752, false),
    (31, 4606900943823306752, 62, 4584664420663164928, false),
    (32, 4607182418800017408, 64, 0, true),
    (33, 4607323156288372736, 66, 0, true),
    (63, 4611545280939032576, 126, 0, true),
    (64, 4611686018427387904, 128, 0, true),
    (1023, 4629691620843847680, 1022, 0, true),
    (32767, 4652218140195815424, 1022, 0, true),
    (32768, 4652218415073722368, 0, 0, true),
    (65536, 4656722014701092864, 0, 0, true),
    (131000, 4661220666526138368, 880, 0, true),
    (131072, 4661225614328463360, 0, 0, true),
];
// equal_distance_positive_winding: exact debt=['512', '1'], k=6
const EQUAL_DISTANCE_POSITIVE_WINDING: &[(u64, u64, usize, u64, bool)] = &[
    (0, 0, 512, 0, false),
    (1, 4584664420663164928, 513, 4580160821035794432, false),
    (31, 4606900943823306752, 543, 4602397344195936256, false),
    (32, 4607182418800017408, 544, 4602678819172646912, false),
    (33, 4607323156288372736, 545, 4602819556661002240, false),
    (63, 4611545280939032576, 575, 4607041681311662080, false),
    (64, 4611686018427387904, 577, 0, false),
    (1023, 4629691620843847680, 526, 4607041681311662080, false),
    (32767, 4652218140195815424, 1022, 4607041681311662080, false),
    (32768, 4652218415073722368, 0, 0, true),
    (65536, 4656722014701092864, 0, 0, true),
    (131000, 4661220666526138368, 952, 0, true),
    (131072, 4661225614328463360, 0, 0, true),
];
// large_master_cast_plateaus: exact debt=['-369', '4'], k=6
const LARGE_MASTER_CAST_PLATEAUS: &[(u64, u64, usize, u64, bool)] = &[
    (
        9007199254740993,
        13835339530258874368,
        17,
        4598175219545276416,
        false,
    ),
    (
        9007199254740994,
        13835198792770519040,
        19,
        4597049319638433792,
        false,
    ),
    (
        9007199254740995,
        13835058055282163712,
        21,
        4595923419731591168,
        false,
    ),
    (
        9007199254740996,
        13835058055282163712,
        21,
        4595923419731591168,
        false,
    ),
    (
        9007199254740997,
        13835058055282163712,
        21,
        4595923419731591168,
        false,
    ),
    (
        9007199254741055,
        13817043656772681728,
        80,
        4598175219545276416,
        false,
    ),
    (
        9007199254741056,
        13817043656772681728,
        80,
        4598175219545276416,
        false,
    ),
    (
        9007199254745088,
        4638558082610233344,
        977,
        4598175219545276416,
        false,
    ),
    (9007199254749184, 4643136449028292608, 949, 0, true),
    (9007199254757376, 4647677432051007488, 949, 0, true),
];
// negative_zero_identity: exact debt=['0', '1'], k=6
const NEGATIVE_ZERO_IDENTITY: &[(u64, u64, usize, u64, bool)] = &[
    (0, 0, 0, 9223372036854775808, true),
    (1, 4584664420663164928, 1, 9223372036854775808, true),
    (1024, 4629700416936869888, 0, 9223372036854775808, true),
    (65536, 4656722014701092864, 0, 9223372036854775808, true),
];
// dense_q32: exact debt=['-25', '1'], k=6
const DENSE_Q32: &[(u64, u64, usize, u64, bool)] = &[
    (23, 4600145544382251008, 36, 0, false),
    (24, 4600427019358961664, 36, 4607041681311662080, false),
    (1622, 4627826849123139584, 1610, 4580160821035794432, false),
    (1623, 4627831247169650688, 1611, 0, true),
    (1624, 4627835645216161792, 1612, 0, true),
    (16384, 4643211215818981376, 2036, 0, true),
    (32768, 4647714815446351872, 2036, 0, true),
    (65559, 4652219995621687296, 2059, 0, true),
];
// dense_q256: exact debt=['-25', '1'], k=6
const DENSE_Q256: &[(u64, u64, usize, u64, bool)] = &[
    (23, 4600145544382251008, 36, 0, false),
    (24, 4600427019358961664, 36, 4607041681311662080, false),
    (1622, 4627826849123139584, 1610, 4580160821035794432, false),
    (1623, 4627831247169650688, 1611, 0, true),
    (1624, 4627835645216161792, 1612, 0, true),
    (16384, 4643211215818981376, 16372, 0, true),
    (32768, 4647714815446351872, 16372, 0, true),
    (65559, 4652219995621687296, 16395, 0, true),
];
