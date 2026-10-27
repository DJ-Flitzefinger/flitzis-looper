use super::*;
use crate::audio_engine::mapped_source_clock::tests::map_with_source;
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::source_reader::{
    FrameRange, SourceReadPlan, StemReadSets, StemRenderSelection, StemTransition,
};
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::SampleBuffer;

fn timeline(fpb: f64, frame: u64, beat: f64) -> TransportTimeline {
    let mut t = TransportTimeline::new(1);
    assert!(t.set_master_period_and_anchor_beat_position_at_frame(fpb, beat, frame));
    t
}
fn geometry(kind: usize) -> MappedLoopGeometry {
    let map = map_with_source(&[(0., 0.), (4., 4.)], &["trusted"], true, 64, 1024);
    let region = FrameRange { start: 0, end: 256 };
    let mut g = match kind {
        0 => MappedLoopGeometry::new(map, 1024, 64, region, 4., true)
            .unwrap()
            .qualify_exact_continuation()
            .unwrap(),
        2 => MappedLoopGeometry::new_general_physical(map, 1024, 64, region, true).unwrap(),
        _ => MappedLoopGeometry::new_general_declared(map, 1024, 64, region, 4., true).unwrap(),
    };
    g.prepare_common_phase();
    g
}
fn birth(
    g: &MappedLoopGeometry,
    kind: usize,
    t: TransportTimeline,
    fraction: f64,
    at: u64,
) -> MappedClockEpoch {
    let raw = FractionalSourcePosition {
        frame: 13,
        fraction,
        seek_mode: ExplicitSeekMode::Normal,
    };
    match kind {
        0 => MappedClockEpoch::anchored_compact(19, t, g, g.domain(), raw, at).unwrap(),
        3 => MappedClockEpoch::full_fraction_ordinary_recovery(19, t, g, raw, at).unwrap(),
        _ => MappedClockEpoch::general_coordinates(19, t, g, raw, at).unwrap(),
    }
}
fn same(a: FractionalSourcePosition, b: FractionalSourcePosition) {
    assert_eq!(a.frame, b.frame);
    assert_eq!(a.fraction.to_bits(), b.fraction.to_bits());
    assert_eq!(a.seek_mode, b.seek_mode);
}

fn compare_unreduced<const N: usize>(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frames: &[u64],
) -> (u32, u32) {
    let reduced = Clock::<N>::new(g, epoch).unwrap();
    let original = Clock::<N>::new_with_basis::<false>(g, epoch).unwrap();
    let removed = original.core.den.magnitude.bit_width() - reduced.core.den.magnitude.bit_width();
    assert_eq!(original.core.den, reduced.core.den.shl(removed).unwrap());
    assert_eq!(
        original.core.initial,
        reduced.core.initial.shl(removed).unwrap()
    );
    assert!(original.core.epoch.matches_exact(reduced.core.epoch));
    for &frame in frames {
        let actual = reduced.position(frame);
        let expected = original.position(frame);
        assert_eq!(actual.is_some(), expected.is_some(), "frame {frame}");
        if let (Some(actual), Some(expected)) = (actual, expected) {
            same(actual, expected);
        }
        let actual = refresh_typed(Clock::<N>::new(g, epoch).unwrap(), frame);
        let expected = refresh_typed(
            Clock::<N>::new_with_basis::<false>(g, epoch).unwrap(),
            frame,
        );
        assert_eq!(actual.is_some(), expected.is_some(), "span at {frame}");
        if let (Some(actual), Some(expected)) = (actual, expected) {
            assert!(actual.matches_exact(expected), "span at {frame}");
            let local = actual.local_general_view(g.domain()).unwrap();
            for at in [
                actual.end_frame,
                frame,
                actual.end_frame - 1,
                actual.start_frame,
            ] {
                same(
                    local.position_at(at).unwrap(),
                    original.position(at).unwrap(),
                );
            }
        }
    }
    (
        removed,
        u32::try_from(reduced.core.beat.exponent() + 126).unwrap(),
    )
}

fn compare_unreduced_family(g: &MappedLoopGeometry, epoch: MappedClockEpoch, frames: &[u64]) {
    if full(epoch) {
        compare_unreduced::<256>(g, epoch, frames);
    } else {
        compare_unreduced::<128>(g, epoch, frames);
    }
}

// Independent IEEE decomposition on the full binary64 grid. The oracle never
// reads the composed payload, inverse, cycle, span or affine implementation.
fn binary(value: f64) -> Num<256> {
    assert!(value.is_finite());
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as u32;
    let significand = (bits & ((1_u64 << 52) - 1)) | if exponent == 0 { 0 } else { 1_u64 << 52 };
    let n = Num::<256>::from_u128(u128::from(significand))
        .unwrap()
        .shl(exponent.saturating_sub(1))
        .unwrap();
    Num::canonical(n.magnitude, bits >> 63 != 0)
}
fn dyadic_pair(n: Num<256>, raw: f64) -> FractionalSourcePosition {
    assert!(!n.negative);
    let unit = bnum::Uint::<256>::try_from(1_u8)
        .unwrap()
        .checked_shl(1074)
        .unwrap();
    let period = unit.checked_shl(8).unwrap();
    let n = n.magnitude.checked_rem(period).unwrap();
    let whole = usize::try_from(n.checked_div(unit).unwrap()).unwrap();
    let rem = n.checked_rem(unit).unwrap();
    let fraction = if rem == binary(raw).magnitude {
        raw
    } else if rem.is_zero() {
        0.
    } else {
        let width = rem.bit_width();
        if width <= 52 {
            f64::from_bits(u64::try_from(rem).unwrap())
        } else {
            let shift = width.saturating_sub(53);
            let mut q = u64::try_from(rem.checked_shr(shift).unwrap()).unwrap();
            if shift != 0 {
                let tail = rem
                    .checked_sub(
                        bnum::Uint::<256>::try_from(q)
                            .unwrap()
                            .checked_shl(shift)
                            .unwrap(),
                    )
                    .unwrap();
                let half = bnum::Uint::<256>::try_from(1_u8)
                    .unwrap()
                    .checked_shl(shift - 1)
                    .unwrap();
                if tail > half || (tail == half && q & 1 != 0) {
                    q += 1;
                }
            }
            let mut exponent = width as i32 - 1 - 1074;
            if q == 1_u64 << 53 {
                q >>= 1;
                exponent += 1;
            }
            f64::from_bits(((exponent + 1023) as u64) << 52 | (q & ((1_u64 << 52) - 1)))
        }
    };
    if fraction == 1. {
        FractionalSourcePosition {
            frame: (whole + 1) % 256,
            fraction: 0.,
            seek_mode: ExplicitSeekMode::Normal,
        }
    } else {
        FractionalSourcePosition {
            frame: whole,
            fraction,
            seek_mode: ExplicitSeekMode::Normal,
        }
    }
}
fn oracle(master: f64, basis: Num<256>, raw: f64) -> FractionalSourcePosition {
    let source = binary(13.)
        .add(binary(raw))
        .unwrap()
        .add(
            binary(master)
                .sub(basis)
                .unwrap()
                .mul(Num::from_i128(64).unwrap())
                .unwrap(),
        )
        .unwrap();
    dyadic_pair(source, raw)
}

#[test]
fn composed_all_ordinary_families_preserve_exact_basis_across_repeated_holds_and_cuts() {
    for (kind, second_resume) in
        (0..4).flat_map(|kind| [186_u64, 1000].map(|resume| (kind, resume)))
    {
        let g = geometry(kind);
        let raw = if kind == 3 { f64::from_bits(1) } else { -0.0 };
        let mut t = timeline(64., 0, 0.);
        let initial = birth(&g, kind, t, raw, 0);
        let mut e = initial.resumed_at(&t, 7, 120).unwrap();
        let mut basis = binary(0.);
        let mut shift = 113;
        let mut saw_non_f64 = false;
        for (execution, fpb, pause) in [
            (131, 64. / 1.16, Some((149, second_resume))),
            (second_resume + 11, 64. / 0.9, None),
            (second_resume + 85, 64. / 1.07, None),
        ] {
            let actual = t.beat_position_at_frame(execution).unwrap();
            let addressed = t.beat_position_at_frame(execution - shift).unwrap();
            let mut next = t;
            assert!(next.set_master_period_preserving_beat_position_at_frame(fpb, execution));
            basis = basis
                .add(binary(actual))
                .unwrap()
                .sub(binary(addressed))
                .unwrap();
            let before = g.position_at(e, execution).unwrap();
            let replacement = e.with_timeline_at(next, execution).unwrap();
            assert!(replacement.is_composed_ordinary());
            assert!(e.replacement_anchor_matches(replacement, execution));
            assert!(replacement.anchor().is_none());
            same(before, g.position_at(replacement, execution).unwrap());
            assert_eq!(replacement.address_shift, 0);
            assert_eq!(replacement.first_output_frame, execution);
            let raw_anchor = anchor(replacement).unwrap();
            assert!(raw_anchor.raw.matches_exact(anchor(initial).unwrap().raw));
            let actual_basis = raw_anchor.basis.number::<256>().unwrap().shl(948).unwrap();
            assert_eq!(actual_basis, basis);
            let width = basis.magnitude.bit_width();
            saw_non_f64 |= width > basis.magnitude.trailing_zeros() + 53;
            let limit = pause.map_or(execution + 193, |(p, _)| p);
            for n in [
                limit - 1,
                execution,
                execution + 1,
                execution + 7,
                limit - 2,
            ] {
                let expected = oracle(next.beat_position_at_frame(n).unwrap(), basis, raw);
                same(g.position_at(replacement, n).unwrap(), expected);
                let span = g.refresh_span(replacement, n).unwrap();
                same(
                    span.local_general_view(g.domain())
                        .unwrap()
                        .position_at(n)
                        .unwrap(),
                    expected,
                );
                same(span.position_in_domain(g.domain(), n).unwrap(), expected);
            }
            if second_resume == 1000 && execution == 1011 {
                compare_unreduced_family(&g, replacement, &[1010, 1011, 1012, 1100, 1512]);
                assert_eq!(
                    g.refresh_span(replacement, execution)
                        .unwrap()
                        .composed_split_for_test(g.domain())
                        .unwrap(),
                    if kind == 3 {
                        (false, 2048)
                    } else {
                        (true, 1024)
                    },
                    "the genuine minimum-subnormal raw anchor must remain wide"
                );
                assert_eq!(
                    basis,
                    Num::<256>::from_i128(9_677_109_699_312_353)
                        .unwrap()
                        .shl(1025)
                        .unwrap()
                );
                for (frame, whole, bits) in [
                    (1011, 64, 0x3fe4_7ae1_47ae_1400),
                    (1012, 65, 0x3fe1_47ae_147a_e400),
                    (1100, 144, 0x3fe7_ae14_7ae1_4400),
                    (1512, 3, 0x3fe1_47ae_147a_dc00),
                ] {
                    let actual = g.position_at(replacement, frame).unwrap();
                    assert_eq!((actual.frame, actual.fraction.to_bits()), (whole, bits));
                }
                assert_ne!(
                    g.position_at(replacement, 1011).unwrap().fraction.to_bits(),
                    0x3fe4_7ae1_47ae_1800,
                    "rounding the basis loses latent phase"
                );
            }
            e = replacement;
            t = next;
            shift = 0;
            if let Some((paused, resumed)) = pause {
                let before = g.position_at(e, paused).unwrap();
                e = e.resumed_at(&t, paused, resumed).unwrap();
                same(before, g.position_at(e, resumed).unwrap());
                shift = resumed - paused;
            }
        }
        if second_resume == 1000 {
            assert!(saw_non_f64, "the hard rational basis must exceed one f64");
        }
    }
}

#[test]
fn composed_basis_reduction_matches_unreduced_signed_zero_partial_and_invalid_controls() {
    for kind in [1, 2, 3] {
        let g = geometry(kind);
        let t = timeline(64., 0, 0.);
        let raw = if kind == 3 { f64::from_bits(1) } else { -0.0 };
        let initial = birth(&g, kind, t, raw, 0).resumed_at(&t, 7, 120).unwrap();
        let mut next = t;
        assert!(next.set_master_period_preserving_beat_position_at_frame(64. / 1.16, 131));
        let epoch = initial.with_timeline_at(next, 131).unwrap();
        let shift = if kind == 3 {
            Clock::<256>::new(&g, epoch).unwrap().core.beat.exponent()
        } else {
            Clock::<128>::new(&g, epoch).unwrap().core.beat.exponent()
        } + 126;
        let shift = u32::try_from(shift).unwrap();
        assert!(shift > 2);
        // Numeric coefficient controls, not fabricated voice/launch authority.
        // Real repeated-cut/non-f64 provenance is exercised above.
        for (coefficient, expected_removed) in [
            (Num::<32>::ZERO, shift),
            (Num::from_i128(1).unwrap(), 0),
            (
                Num::from_i128(3).unwrap().shl(shift / 2).unwrap(),
                shift / 2,
            ),
            (Num::from_i128(1).unwrap().shl(shift).unwrap(), shift),
            (Num::from_i128(-1).unwrap().shl(shift).unwrap(), shift),
        ] {
            let mut controlled = epoch;
            let a = match &mut controlled.mode {
                MappedClockMode::ComposedOrdinary(a) | MappedClockMode::ComposedFullFraction(a) => {
                    a
                }
                _ => unreachable!(),
            };
            a.basis = Basis::from_num(coefficient).unwrap();
            let frames = [130, 131, 132, 258, 386, 643, u64::MAX];
            let (removed, original_shift) = if kind == 3 {
                compare_unreduced::<256>(&g, controlled, &frames)
            } else {
                compare_unreduced::<128>(&g, controlled, &frames)
            };
            assert_eq!((removed, original_shift), (expected_removed, shift));
        }
        // Preserve raw bits exactly when the source coordinate is the raw pair.
        let mut raw_identity = epoch;
        let a = match &mut raw_identity.mode {
            MappedClockMode::ComposedOrdinary(a) | MappedClockMode::ComposedFullFraction(a) => a,
            _ => unreachable!(),
        };
        a.basis = Basis::from_master(next.beat_position_at_frame(131).unwrap()).unwrap();
        compare_unreduced_family(&g, raw_identity, &[131, 132, 386]);
        let actual = g.position_at(raw_identity, 131).unwrap();
        assert_eq!(
            (actual.frame, actual.fraction.to_bits()),
            (13, raw.to_bits())
        );
        for fault in 0..4 {
            let mut invalid = epoch;
            if fault == 3 {
                invalid.address_shift = u64::MAX;
            } else {
                let a = match &mut invalid.mode {
                    MappedClockMode::ComposedOrdinary(a)
                    | MappedClockMode::ComposedFullFraction(a) => a,
                    _ => unreachable!(),
                };
                match fault {
                    0 => {
                        a.basis =
                            Basis::from_num(Num::<32>::from_i128(1).unwrap().shl(251).unwrap())
                                .unwrap()
                    }
                    1 => a.raw.inverse_segment.source_end = 8.,
                    _ => a.raw.fraction = f64::NAN,
                }
            }
            if kind == 3 {
                assert!(Clock::<256>::new(&g, invalid).is_none());
                assert!(Clock::<256>::new_with_basis::<false>(&g, invalid).is_none());
            } else {
                assert!(Clock::<128>::new(&g, invalid).is_none());
                assert!(Clock::<128>::new_with_basis::<false>(&g, invalid).is_none());
            }
        }
    }
}

#[test]
fn composed_reduction_preserves_rn_seam_carry_partitioned_copies_and_projection() {
    let sample = SampleBuffer {
        samples: (0..2048)
            .map(|i| ((i * 37 % 127) as f32 - 63.) / 64.)
            .collect::<Vec<_>>()
            .into(),
        channels: 2,
        residency: None,
    };
    for kind in 0..4 {
        let g = geometry(kind);
        let t = timeline(64., 0, 0.);
        let raw = FractionalSourcePosition {
            frame: 253,
            fraction: 0.5_f64.next_down(),
            seek_mode: ExplicitSeekMode::Normal,
        };
        let initial = match kind {
            0 => MappedClockEpoch::anchored_compact(19, t, &g, g.domain(), raw, 0).unwrap(),
            3 => MappedClockEpoch::full_fraction_ordinary_recovery(19, t, &g, raw, 0).unwrap(),
            _ => MappedClockEpoch::general_coordinates(19, t, &g, raw, 0).unwrap(),
        };
        let mut projected = SourcePlayback::mapped_compact(&g, initial, 0).unwrap();
        projected.project_mapped_compact_to(&g, 17).unwrap();
        assert!(projected.matches_exact(&SourcePlayback::mapped_compact(&g, initial, 17).unwrap()));
        let held = initial.resumed_at(&t, 1, 2).unwrap();
        let mut next = t;
        assert!(next.set_master_period_preserving_beat_position_at_frame(128., 3));
        let epoch = held.with_timeline_at(next, 3).unwrap();
        assert_eq!(
            g.refresh_span(epoch, 3)
                .unwrap()
                .composed_split_for_test(g.domain()),
            Some((true, 1024))
        );
        compare_unreduced_family(&g, epoch, &[3, 4, 5, 514, 515, 516]);
        let carried = g.position_at(epoch, 4).unwrap();
        assert_eq!((carried.frame, carried.fraction.to_bits()), (0, 0));
        for partition in [&[64][..], &[1, 7, 31, 3][..]] {
            let mut copied = SourcePlayback::mapped_compact(&g, epoch, 3).unwrap();
            let mut frame = 3;
            let mut index = 0;
            while frame < 520 {
                let requested = partition[index % partition.len()].min((520 - frame) as usize);
                let (frames, _) = copied.chunk(requested);
                if frames == 0 {
                    assert!(!copied.can_read_frames(1));
                    copied.project_mapped_compact_to(&g, frame).unwrap();
                    continue;
                }
                for offset in [0, frames / 2, frames] {
                    let at = frame + offset as u64;
                    let expected = if kind == 3 {
                        Clock::<256>::new_with_basis::<false>(&g, epoch)
                            .unwrap()
                            .position(at)
                            .unwrap()
                    } else {
                        Clock::<128>::new_with_basis::<false>(&g, epoch)
                            .unwrap()
                            .position(at)
                            .unwrap()
                    };
                    let actual = copied.position_at(offset).unwrap();
                    same(actual, expected);
                    let domain = g.domain();
                    let plan = SourceReadPlan {
                        channels: 2,
                        sample_frames: domain.sample_frames,
                        frame_pos: actual.frame,
                        loop_region: domain.region,
                        loop_period: domain.musical_period,
                        seek_mode: actual.seek_mode,
                        selection: StemRenderSelection::full_mix(),
                        transition: StemTransition::default(),
                    };
                    // Independently interpolate the unchanged integer256-frame
                    // source circle at the original wide coordinate, per channel.
                    for channel in 0..2 {
                        let left = sample.samples[expected.frame * 2 + channel];
                        let right = sample.samples[((expected.frame + 1) % 256) * 2 + channel];
                        let pcm = if expected.fraction == 0. {
                            left
                        } else {
                            left + (right - left) * expected.fraction as f32
                        };
                        assert_eq!(
                            plan.try_sample_fractional_with_readers(
                                &sample,
                                StemReadSets::default(),
                                actual.fraction,
                                0.,
                                channel
                            )
                            .unwrap()
                            .to_bits(),
                            pcm.to_bits()
                        );
                    }
                }
                assert!(copied.advance(frames));
                frame += frames as u64;
                index += 1;
            }
            copied.project_mapped_compact_to(&g, 521).unwrap();
            assert!(copied.matches_exact(&SourcePlayback::mapped_compact(&g, epoch, 521).unwrap()));
        }
    }
}

#[test]
fn composed_negative_master_zero_shift_plateau_and_forged_replacements_fail_closed() {
    let g = geometry(1);
    for beat in [-3.25, -0.0, 2_f64.powi(60)] {
        let t = timeline(64., 0, beat);
        let e = birth(&g, 1, t, -0., 0);
        let cut = 9;
        let mut next = t;
        assert!(next.set_master_period_preserving_beat_position_at_frame(65., cut));
        let unchanged_basis = e.with_timeline_at(next, cut).unwrap();
        assert!(!unchanged_basis.is_composed_ordinary());
        assert!(
            unchanged_basis
                .anchor()
                .unwrap()
                .matches_exact(e.anchor().unwrap())
        );
        assert!(!e.replacement_anchor_matches(
            replace(e, next.beat_projection().unwrap(), cut).unwrap(),
            cut
        ));
        same(
            g.position_at(e, cut).unwrap(),
            g.position_at(unchanged_basis, cut).unwrap(),
        );
        let translated = e.resumed_at(&t, 3, 8).unwrap();
        let c = translated.with_timeline_at(next, cut).unwrap();
        assert!(c.is_composed_ordinary());
        same(
            g.position_at(translated, cut).unwrap(),
            g.position_at(c, cut).unwrap(),
        );
        assert!(c.with_timeline_at(next, cut).unwrap().matches_exact(c));
        let mut forged = c;
        let MappedClockMode::ComposedOrdinary(ref mut a) = forged.mode else {
            unreachable!()
        };
        a.basis = Basis::from_num(
            a.basis
                .number::<32>()
                .unwrap()
                .add(Num::from_i128(1).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(!translated.replacement_anchor_matches(forged, cut));
        let mut foreign = c;
        let MappedClockMode::ComposedOrdinary(ref mut a) = foreign.mode else {
            unreachable!()
        };
        a.raw.inverse_segment.source_end = 8.;
        assert!(!translated.replacement_anchor_matches(foreign, cut));
        assert!(g.position_at(foreign, cut).is_err());
        let stale = timeline(65., cut, t.beat_position_at_frame(cut).unwrap() + 1024.);
        assert!(translated.with_timeline_at(stale, cut).is_none());
        assert!(c.with_timeline_at(t, cut - 1).is_none());
        assert!(translated.with_timeline_at(next, u64::MAX).is_none());
    }
    assert!(Basis::from_master(f64::MAX).is_none());
    assert!(Basis::from_master(f64::from_bits(1)).is_none());
    assert!(Basis::from_num(Num::<64>::from_i128(1).unwrap().shl(252).unwrap()).is_none());
    let at = u64::MAX - 4096;
    let t = timeline(64., at, 0.);
    let e = birth(&g, 1, t, 0., at)
        .resumed_at(&t, at + 1, at + 17)
        .unwrap();
    // At MAX-32, the authoritative downbeat calculation rounds the frame to
    // 2^64 with zero bar phase. Preserve that genuine transport refusal.
    let mut overflow = t;
    assert!(!overflow.set_master_period_preserving_beat_position_at_frame(60., u64::MAX - 32));
    assert!(overflow.matches_clock_epoch(&t));
    assert_eq!(overflow.output_frame(), t.output_frame());
    let cut = u64::MAX - 2048;
    let mut next = t;
    assert!(next.set_master_period_preserving_beat_position_at_frame(60., cut));
    let c = e.with_timeline_at(next, cut).unwrap();
    assert!(c.is_composed_ordinary());
    same(
        g.position_at(e, cut).unwrap(),
        g.position_at(c, cut).unwrap(),
    );
    let first_span = g.refresh_span(c, cut).unwrap();
    same(
        g.position_at(c, first_span.end_frame).unwrap(),
        first_span
            .position_in_domain(g.domain(), first_span.end_frame)
            .unwrap(),
    );
    // The final u64-to-f64 jump can cross several knots. Its canonical span
    // is independent of the first interval at the valid cut.
    let span = g.refresh_span(c, u64::MAX - 1).unwrap();
    same(
        g.position_at(c, u64::MAX).unwrap(),
        span.position_in_domain(g.domain(), u64::MAX).unwrap(),
    );
}

#[test]
fn composed_declared_clock_remains_eligible_for_original_phase_constructor() {
    let g = geometry(1);
    let t = timeline(64., 0, 0.);
    let reference =
        crate::audio_engine::loop_phase_reference::LoopPhaseReference::capture(Some(t), 0)
            .qualify(g.map.canonical_segment_at_beat(0.).unwrap());
    let e = birth(&g, 1, t, 0., 0).resumed_at(&t, 3, 8).unwrap();
    let mut next = t;
    assert!(next.set_master_period_preserving_beat_position_at_frame(60., 9));
    let c = e.with_timeline_at(next, 9).unwrap();
    assert!(c.is_composed_ordinary());
    let raw = g.position_at(c, 9).unwrap();
    // Qualification remains the original reference's job, not the composed tag.
    assert!(reference.current(&next));
    assert!(MappedClockEpoch::common_phase(20, next, &g, raw, 9, reference).is_ok());
    assert!(
        MappedClockEpoch::common_phase(
            20,
            next,
            &g,
            raw,
            9,
            crate::audio_engine::loop_phase_reference::LoopPhaseReference::Unavailable
        )
        .is_err()
    );
}

#[test]
fn composed_borrowed_compact_api_checks_whole_original_span_and_exclusive_endpoint() {
    let g = geometry(0);
    let t = timeline(64., 0, 0.);
    let original = birth(&g, 0, t, -0., 0);
    assert!(original.is_compact_anchored());
    let translated = original.resumed_at(&t, 7, 120).unwrap();
    let mut next = t;
    assert!(next.set_master_period_preserving_beat_position_at_frame(64. / 1.16, 131));
    let epoch = translated.with_timeline_at(next, 131).unwrap();
    assert!(epoch.is_composed_ordinary());
    let span = g.refresh_span_compact(epoch, 131).unwrap();
    let borrowed = span.compact_view(&g).unwrap();
    let with_reference = span
        .compact_view_with_reference(&g, super::super::LoopPhaseReference::Unavailable)
        .unwrap();
    let local = span.local_general_view(g.domain()).unwrap();
    assert!(local.position_at(span.end_frame + 1).is_none());
    if span.start_frame > 0 {
        assert!(local.position_at(span.start_frame - 1).is_none());
    }
    for frame in [
        span.end_frame,
        span.start_frame,
        span.end_frame - 1,
        span.start_frame + 7,
    ] {
        let expected = g.position_at(epoch, frame).unwrap();
        same(g.position_at_compact(epoch, frame).unwrap(), expected);
        same(borrowed.position_at(frame).unwrap(), expected);
        same(with_reference.position_at(frame).unwrap(), expected);
        same(local.position_at(frame).unwrap(), expected);
    }
    let mut wrong_endpoint = span;
    wrong_endpoint.endpoint_fraction = f64::from_bits(span.endpoint_fraction.to_bits() ^ 1);
    assert!(wrong_endpoint.compact_view(&g).is_err());
    for fraction in [f64::NAN, -1.0, 1.0] {
        let mut invalid = span;
        invalid.endpoint_fraction = fraction;
        assert!(invalid.local_general_view(g.domain()).is_err());
    }
    let mut wrong_program = span;
    wrong_program.program.data[0] ^= 1;
    assert!(wrong_program.compact_view(&g).is_err());
    let foreign_map = map_with_source(
        &[(0., 0.), (1., 0.5), (4., 4.)],
        &["trusted"; 2],
        true,
        64,
        1024,
    );
    let foreign = MappedLoopGeometry::new(
        foreign_map,
        1024,
        64,
        FrameRange { start: 0, end: 256 },
        4.,
        true,
    )
    .unwrap()
    .qualify_exact_continuation()
    .unwrap();
    assert_eq!(foreign.domain(), g.domain());
    assert!(span.compact_view(&foreign).is_err());
}
