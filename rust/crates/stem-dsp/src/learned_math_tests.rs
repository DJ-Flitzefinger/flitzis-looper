//! Operator controls, never a learned release-network or process admission.
use super::*;

fn close(actual: f32, previous: f32, absolute: f32, relative: f32) {
    if previous.is_nan() {
        assert!(actual.is_nan());
    } else if previous.is_infinite() {
        assert_eq!(actual, previous);
    } else {
        assert!(actual.is_finite());
        assert!(
            (actual - previous).abs() <= absolute + relative * previous.abs(),
            "actual={actual:?} previous={previous:?}"
        );
    }
}

#[test]
fn rust_transcendentals_match_previous_math_classes_and_finite_samples() {
    let check = |x: f32| {
        let (sin, cos) = libm::sincosf(x);
        let (old_sin, old_cos) = x.sin_cos();
        close(sin, old_sin, 2e-7, 2e-6);
        close(cos, old_cos, 2e-7, 2e-6);
        close(libm::tanhf(x), x.tanh(), 2e-7, 2e-6);
        close(libm::expf(x), x.exp(), f32::from_bits(2), 2e-6);
    };
    for x in [
        0.,
        -0.,
        f32::from_bits(1),
        -f32::from_bits(1),
        f32::MIN_POSITIVE,
        -f32::MIN_POSITIVE,
        f32::MAX,
        f32::MIN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ] {
        check(x);
    }
    // Both signs and adjacent floats at polynomial/range-reduction/saturation,
    // overflow and underflow boundaries in the pinned kernels.
    for value in [
        0x0080_0000u32,
        0x3980_0000,
        0x3e82_c578,
        0x3f0c_9f54,
        0x3f49_0fda,
        0x4016_cbe3,
        0x407b_53d1,
        0x40af_eddf,
        0x40e2_31d5,
        0x4120_0000,
        0x42ae_ac50,
        0x42b1_7218,
        0x42cf_f1b5,
        0x4dc9_0fdb,
    ] {
        for bits in value - 2..=value + 2 {
            check(f32::from_bits(bits));
            check(-f32::from_bits(bits));
        }
    }
    let mut state = 0x3f81_7265u32;
    for _ in 0..100_000 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        check(f32::from_bits(state));
    }
    // Preserve exact zero signs and exp/sigmoid saturation behavior.
    for x in [0.0f32, -0.0] {
        assert_eq!(libm::sincosf(x).0.to_bits(), x.to_bits());
        assert_eq!(libm::tanhf(x).to_bits(), x.to_bits());
        assert_eq!(libm::expf(x), 1.);
        assert_eq!(sigmoid(x), 0.5);
    }
    assert_eq!(sigmoid(f32::MAX), 1.);
    assert_eq!(sigmoid(f32::MIN), 0.);
    assert!(sigmoid(f32::NAN).is_nan());
}

#[test]
fn rust_rotary_matches_previous_all_original_positions_and_large_angles() {
    let frequencies = std::array::from_fn::<_, { HEAD / 2 }, _>(|i| match i % 8 {
        0 => 0.,
        1 => f32::from_bits(1),
        2 => 0.001 * i as f32,
        3 => -0.17,
        4 => std::f32::consts::FRAC_PI_2,
        5 => 100_000_000.,
        6 => 1e30,
        _ => -1e30,
    });
    let mut actual: Vec<_> = (0..MAX_STEPS * INNER * 3)
        .map(|i| (i % 101) as f32 * 0.013 - 0.65)
        .collect();
    let source = actual.clone();
    rotate(&mut actual, &frequencies, &|| false).unwrap();
    for position in 0..MAX_STEPS {
        for head in 0..HEADS {
            for (pair, frequency) in frequencies.iter().enumerate() {
                let (sin, cos) = (position as f32 * frequency).sin_cos();
                for offset in [0, INNER] {
                    let i = position * INNER * 3 + offset + head * HEAD + 2 * pair;
                    close(actual[i], source[i] * cos - source[i + 1] * sin, 2e-7, 2e-6);
                    close(
                        actual[i + 1],
                        source[i] * sin + source[i + 1] * cos,
                        2e-7,
                        2e-6,
                    );
                }
            }
            let v = position * INNER * 3 + 2 * INNER + head * HEAD;
            assert_eq!(&actual[v..v + HEAD], &source[v..v + HEAD]);
        }
    }
    // A finite loaded frequency may still overflow when multiplied by position.
    // As before, the entire private attempt must reject the nonfinite rotation.
    let mut invalid = vec![1.; 3 * INNER * 3];
    assert_eq!(
        rotate(&mut invalid, &[f32::MAX; HEAD / 2], &|| false),
        Err(Error::NonFinite)
    );
    assert_eq!(
        rotate(&mut invalid, &frequencies, &|| true),
        Err(Error::Cancelled)
    );
}

#[test]
fn nonuniform_tiled_attention_matches_previous_dense_exp_and_sigmoid() {
    // Independent dense scalar dot/value loops, old std exp/sigmoid, all8 heads,
    // complete noncausal rows and query-tile tails1/15. No release model/weights.
    for length in [17, 31] {
        let qkv: Vec<_> = (0..length * INNER * 3)
            .map(|i| ((i * 17 + i / 13) % 113) as f32 * 0.009 - 0.5)
            .collect();
        let gates: Vec<_> = (0..length * HEADS)
            .map(|i| (i % 19) as f32 * 0.5 - 4.5)
            .collect();
        let actual = attend_heads(&qkv, &gates, &|| false).unwrap();
        for query in 0..length {
            for head in 0..HEADS {
                let mut scores = Vec::with_capacity(length);
                for key in 0..length {
                    let mut score = 0.;
                    for d in 0..HEAD {
                        score += qkv[query * INNER * 3 + head * HEAD + d]
                            * qkv[key * INNER * 3 + INNER + head * HEAD + d];
                    }
                    scores.push(score * 0.125);
                }
                let maximum = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let mut total = 0.;
                for score in &mut scores {
                    *score = (*score - maximum).exp();
                    total += *score;
                }
                for score in &mut scores {
                    *score /= total;
                }
                let gate = 1. / (1. + (-gates[query * HEADS + head]).exp());
                for d in 0..HEAD {
                    let mut value = 0.;
                    for key in 0..length {
                        value += scores[key] * qkv[key * INNER * 3 + 2 * INNER + head * HEAD + d];
                    }
                    close(
                        actual[query * INNER + head * HEAD + d],
                        value * gate,
                        2e-7,
                        2e-6,
                    );
                }
            }
        }
    }
}
