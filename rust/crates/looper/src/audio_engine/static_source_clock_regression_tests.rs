//! Exact static-clock expectations generated independently with Python Fraction.
//! These coordinates convey no source custody, renderer receipt or music acceptance.

use crate::audio_engine::loop_phase_reference::LoopPhaseReference;
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
    launch: u64,
    prior_frames: usize,
    held_frames: u64,
    prior_rate: f64,
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
    let domain = SourceLoopDomain::physical(region.end + 1, region);
    assert_eq!(
        case.launch + case.prior_frames as u64 + case.held_frames,
        case.first
    );
    let mut birth = SourcePlayback::scalar_continuation(
        FractionalSourcePosition {
            frame: case.start,
            fraction: if case.prior_frames == 0 {
                case.fraction
            } else {
                0.0
            },
            seek_mode: ExplicitSeekMode::Normal,
        },
        domain,
        case.prior_rate,
        case.prior_rate,
    )
    .unwrap();
    birth.install_launch_reference(LoopPhaseReference::capture(
        Some(timeline(case)),
        case.launch,
    ));
    assert!(birth.advance(case.prior_frames));
    assert_eq!(birth.position().frame, case.whole);
    assert_eq!(birth.position().fraction.to_bits(), case.fraction.to_bits());
    // A numerical prior trajectory declares advancement and held output frames;
    // production launch/pause custody is covered by the actual runtime tests.
    // The ordinary rate conversion at E carries the original launch reference.
    let mut current = SourcePlayback::scalar_continuation(
        birth.position(),
        domain,
        case.f / case.master_f,
        case.f / case.master_f,
    )
    .unwrap();
    current.carry_launch_reference_from(&birth);
    current
}

fn verify(case: Case, expected: &[Expected]) {
    let master = timeline(case);
    let preceding = preceding(case);
    let source = SourcePlayback::static_continuation(
        &preceding,
        master,
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
        launch: 0,
        prior_frames: 23,
        held_frames: 25,
        prior_rate: 1.0,
        first: 48,
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
        whole: 64,
        fraction: 0.0,
        launch: 0,
        prior_frames: 64,
        held_frames: 0,
        prior_rate: 1.0,
        first: 64,
        anchor_frame: 0,
        anchor: 0.0,
    };
    verify(case, LOWER_RATE_POSITIVE_WINDING);
    verify(
        Case {
            f: 64.0,
            master_f: 32.0,
            whole: 32,
            prior_rate: 0.5,
            ..case
        },
        UPPER_RATE_NEGATIVE_WINDING,
    );
    verify(
        Case {
            master_f: 32.0,
            whole: 0,
            prior_frames: 512,
            prior_rate: 2.0,
            first: 512,
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
            launch: (1_u64 << 53) - 31,
            prior_frames: 19,
            held_frames: 13,
            prior_rate: 0.75,
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
            launch: 0,
            prior_frames: 0,
            held_frames: 0,
            prior_rate: 1.0,
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
        launch: 0,
        prior_frames: 1,
        held_frames: 1,
        prior_rate: 1.0,
        first: 2,
        anchor_frame: 0,
        anchor: 0.0,
    };
    let preceding = preceding(case);
    let master = timeline(case);
    for (f, origin, quarters, first, marker) in [
        (32.0, f64::NAN, 32.0, 2, 7),
        (0.0, 0.0, 32.0, 2, 7),
        (32.0, 0.0, 31.0, 2, 7),
        (32.0, 0.0, 32.0, 2, 0),
        (32.0, 0.0, 32.0, u64::MAX, 7),
        (32.0, 2.0_f64.powi(-126), 32.0, 2, 7),
    ] {
        assert!(
            SourcePlayback::static_continuation(
                &preceding, master, first, marker, f, origin, quarters
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
        SourcePlayback::static_continuation(&gliding, master, case.first, 7, 32.0, 0.0, 32.0)
            .is_err()
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
        launch: 0,
        prior_frames: 1,
        held_frames: 1,
        prior_rate: 1.0,
        first: 2,
        anchor_frame: 0,
        anchor: 0.0,
    };
    let preceding = preceding(case);
    let mut source = SourcePlayback::static_continuation(
        &preceding,
        timeline(case),
        case.first,
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
        launch: 0,
        prior_frames: 1,
        held_frames: 1,
        prior_rate: 1.0,
        first: 2,
        anchor_frame: 0,
        anchor: 0.0,
    };
    let preceding = preceding(case);
    let master = timeline(case);
    let mut source =
        SourcePlayback::static_continuation(&preceding, master, case.first, 7, 32.0, 0.0, 32.0)
            .unwrap();
    let other =
        SourcePlayback::static_continuation(&preceding, master, case.first, 8, 32.0, 0.0, 32.0)
            .unwrap();
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
// Generated by intentional_static_phase_fraction_oracle_v2.py; do not hand-fit values.
// off_grid_q32: debt=['225179981368524809', '9007199254740992'], k=6, first capture=1649
const OFF_GRID_Q32: &[Expected] = &[
    (48, 4607290505191074300, 36, 0, false),
    (49, 4607386581983124870, 37, 4580160821035790890, false),
    (63, 4608731657071832859, 51, 4597612269591855211, false),
    (511, 4622326523146988596, 506, 4597612269591855425, false),
    (1023, 4626836127573862253, 1026, 4597612269591857846, false),
    (1535, 4629805500928175200, 46, 4597612269591860267, false),
    (1647, 4630141769700352197, 159, 4607041681311663265, false),
    (1648, 4630144772100103777, 161, 4401766708696580096, false),
    (1649, 4630147774499855357, 161, 4607182418800016600, true),
    (1650, 4630150776899606938, 163, 4414301691009040384, true),
    (2047, 4631342729600984329, 560, 4406631497893675008, true),
    (4095, 4635847830428230615, 1107, 4607182418800017168, true),
    (12048, 4643229230217490858, 61, 4406772235382030336, true),
    (24048, 4647723822645606613, 61, 4406772235382030336, true),
    (48048, 4652222918673349738, 60, 4607182418799969792, true),
];
// off_grid_q256: debt=['225179981368524809', '9007199254740992'], k=6, first capture=1649
const OFF_GRID_Q256: &[Expected] = &[
    (48, 4607290505191074300, 36, 0, false),
    (49, 4607386581983124870, 37, 4580160821035790890, false),
    (63, 4608731657071832859, 51, 4597612269591855211, false),
    (511, 4622326523146988596, 506, 4597612269591855425, false),
    (1023, 4626836127573862253, 1026, 4597612269591857846, false),
    (1535, 4629805500928175200, 1546, 4597612269591860267, false),
    (1647, 4630141769700352197, 1659, 4607041681311663265, false),
    (1648, 4630144772100103777, 1661, 4401766708696580096, false),
    (1649, 4630147774499855357, 1661, 4607182418800016600, true),
    (1650, 4630150776899606938, 1663, 4414301691009040384, true),
    (2047, 4631342729600984329, 2060, 4406631497893675008, true),
    (4095, 4635847830428230615, 4107, 4607182418800017168, true),
    (12048, 4643229230217490858, 61, 4406772235382030336, true),
    (24048, 4647723822645606613, 61, 4406772235382030336, true),
    (48048, 4652222918673349738, 60, 4607182418799969792, true),
];
// lower_rate_positive_winding: debt=['992', '1'], k=6, first capture=127040
const LOWER_RATE_POSITIVE_WINDING: &[Expected] = &[
    (64, 4607182418800017408, 64, 0, false),
    (65, 4607252787544195072, 64, 4602749187916824576, false),
    (95, 4609363849869524992, 79, 4604860250242154496, false),
    (96, 4609434218613702656, 80, 4598175219545276416, false),
    (1023, 4625188021216477184, 550, 4607112050055839744, false),
    (32767, 4647714540568444928, 286, 4607112050055839744, false),
    (32768, 4647714815446351872, 287, 4602678819172646912, false),
    (65536, 4652218415073722368, 543, 4602678819172646912, false),
    (127039, 4656444869051416576, 31, 4602538081684291584, false),
    (127040, 4656444937770893312, 32, 0, true),
    (127041, 4656445006490370048, 32, 4602678819172646912, true),
    (131000, 4656717066898767872, 988, 0, true),
    (131072, 4656722014701092864, 0, 0, true),
];
// upper_rate_negative_winding: debt=['-928', '1'], k=6, first capture=29760
const UPPER_RATE_NEGATIVE_WINDING: &[Expected] = &[
    (64, 4611686018427387904, 32, 0, false),
    (65, 4611756387171565568, 33, 4606900943823306752, false),
    (95, 4613867449496895488, 93, 4584664420663164928, false),
    (96, 4613937818241073152, 95, 0, false),
    (1023, 4629691620843847680, 896, 4584664420663164928, false),
    (29759, 4651391307451727872, 126, 4584664420663164928, false),
    (29760, 4651391582329634816, 128, 0, true),
    (29761, 4651391857207541760, 130, 0, true),
    (32767, 4652218140195815424, 1022, 0, true),
    (32768, 4652218415073722368, 0, 0, true),
    (65536, 4656722014701092864, 0, 0, true),
    (131000, 4661220666526138368, 880, 0, true),
    (131072, 4661225614328463360, 0, 0, true),
];
// equal_distance_positive_winding: debt=['512', '1'], k=6, first capture=33280
const EQUAL_DISTANCE_POSITIVE_WINDING: &[Expected] = &[
    (512, 4625196817309499392, 0, 0, false),
    (513, 4625205613402521600, 1, 4580160821035794432, false),
    (543, 4625469496193187840, 31, 4602397344195936256, false),
    (544, 4625478292286210048, 32, 4602678819172646912, false),
    (1023, 4629691620843847680, 518, 4607041681311662080, false),
    (32767, 4652218140195815424, 1014, 4607041681311662080, false),
    (32768, 4652218415073722368, 1016, 0, false),
    (33279, 4652288646378946560, 510, 4607041681311662080, false),
    (33280, 4652288783817900032, 512, 0, true),
    (33281, 4652288921256853504, 513, 0, true),
    (65536, 4656722014701092864, 0, 0, true),
    (131000, 4661220666526138368, 952, 0, true),
    (131072, 4661225614328463360, 0, 0, true),
];
// large_master_cast_plateaus: debt=['67', '4'], k=6, first capture=9007199254742063
const LARGE_MASTER_CAST_PLATEAUS: &[Expected] = &[
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
        4598738169498697728,
        false,
    ),
    (
        9007199254740995,
        13835058055282163712,
        21,
        4599301119452119040,
        false,
    ),
    (
        9007199254740996,
        13835058055282163712,
        21,
        4599301119452119040,
        false,
    ),
    (
        9007199254740997,
        13835058055282163712,
        21,
        4599301119452119040,
        false,
    ),
    (
        9007199254741055,
        13817043656772681728,
        82,
        4598175219545276416,
        false,
    ),
    (
        9007199254741056,
        13817043656772681728,
        82,
        4598175219545276416,
        false,
    ),
    (
        9007199254742062,
        4629506902890381312,
        79,
        4606900943823306752,
        false,
    ),
    (9007199254742063, 4629524495076425728, 82, 0, true),
    (9007199254742064, 4629524495076425728, 82, 0, true),
    (9007199254745088, 4638558082610233344, 34, 0, true),
    (9007199254749184, 4643136449028292608, 34, 0, true),
    (9007199254757376, 4647677432051007488, 34, 0, true),
];
// negative_zero_identity: debt=['0', '1'], k=6, first capture=0
const NEGATIVE_ZERO_IDENTITY: &[Expected] = &[
    (0, 0, 0, 9223372036854775808, true),
    (1, 4584664420663164928, 1, 9223372036854775808, true),
    (1024, 4629700416936869888, 0, 9223372036854775808, true),
    (65536, 4656722014701092864, 0, 9223372036854775808, true),
];
// dense_q32: debt=['25', '1'], k=6, first capture=1648
const DENSE_Q32: &[Expected] = &[
    (48, 4604930618986332160, 36, 0, false),
    (49, 4605071356474687488, 37, 4580160821035794432, false),
    (1647, 4627936800285917184, 1659, 4607041681311662080, false),
    (1648, 4627941198332428288, 1661, 0, true),
    (1649, 4627945596378939392, 1662, 0, true),
    (16384, 4643211215818981376, 13, 0, true),
    (32768, 4647714815446351872, 13, 0, true),
    (65584, 4652221713608605696, 61, 0, true),
];
// dense_q256: debt=['25', '1'], k=6, first capture=1648
const DENSE_Q256: &[Expected] = &[
    (48, 4604930618986332160, 36, 0, false),
    (49, 4605071356474687488, 37, 4580160821035794432, false),
    (1647, 4627936800285917184, 1659, 4607041681311662080, false),
    (1648, 4627941198332428288, 1661, 0, true),
    (1649, 4627945596378939392, 1662, 0, true),
    (16384, 4643211215818981376, 13, 0, true),
    (32768, 4647714815446351872, 13, 0, true),
    (65584, 4652221713608605696, 61, 0, true),
];
