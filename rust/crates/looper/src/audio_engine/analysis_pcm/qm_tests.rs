use super::*;
use std::cell::Cell;

fn sample(values: Vec<f32>, channels: usize) -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels,
        samples: values.into(),
    }
}
fn nn_hash(sample: &SampleBuffer) -> String {
    let mut hash = Sha256::new();
    for frame in sample.samples.chunks_exact(sample.channels) {
        let mut sum = 0.0_f64;
        for &value in frame {
            sum += f64::from(value);
        }
        hash.update(((sum / sample.channels as f64) as f32).to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}

#[test]
fn optional_qm_input_matches_the_complete_normal_converter() {
    for channels in [1, 2] {
        for rate in [44_100, 48_000] {
            let input = sample(
                (0..rate / 2 + 17)
                    .flat_map(|i| {
                        (0..channels).map(move |channel| {
                            ((i as usize * 13 + channel * 37) as f32 * 0.007).sin() * 0.4
                        })
                    })
                    .collect(),
                channels,
            );
            let expected = super::super::default::prepare_default_mono_f64(&input, rate).unwrap();
            let prepared =
                prepare_qm_input(&input, rate, &nn_hash(&input), 512 * 1024 * 1024, &|| false)
                    .unwrap();
            assert_eq!(prepared.samples, expected);
            assert_eq!(
                prepared.samples.len(),
                (input.frame_count() as u128 * 44_100).div_ceil(u128::from(rate)) as usize
            );
            assert_eq!(
                prepared.resampler,
                if rate == 44_100 {
                    "identity-44100-v1"
                } else {
                    "rubato-fft-1.0-44100-delay-trim-tail-flush-v1"
                }
            );
            assert!(prepared.preparation_peak_bytes <= 512 * 1024 * 1024);
        }
    }
}

#[test]
fn mono_signed_zero_uses_distinct_normal_and_exact_nn_rules() {
    let input = sample(vec![-0.0, 0.0, 0.5, -0.5], 1);
    let prepared = prepare_qm_input(&input, 44_100, &nn_hash(&input), 32 * 1024 * 1024, &|| {
        false
    })
    .unwrap();
    assert_eq!(prepared.samples[0].to_bits(), (-0.0_f64).to_bits());
    assert_ne!(prepared.normal_mono_sha256, nn_hash(&input));
    assert!(matches!(
        prepare_qm_input(&input, 44_100, &"0".repeat(64), 32 * 1024 * 1024, &|| false),
        Err(PcmError::InvalidInput(_))
    ));
}

#[test]
fn qm_budget_and_cancellation_are_distinct_before_and_during_preparation() {
    let input = sample(vec![0.1; 24_019], 1);
    assert!(matches!(
        prepare_qm_input(&input, 48_001, &nn_hash(&input), 1024 * 1024, &|| false),
        Err(PcmError::Limit(_))
    ));
    assert!(matches!(
        prepare_qm_input(&input, 48_000, &nn_hash(&input), 512 * 1024 * 1024, &|| {
            true
        }),
        Err(PcmError::Cancelled)
    ));
    let calls = Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() >= 4
    };
    assert!(matches!(
        prepare_qm_input(
            &input,
            48_000,
            &nn_hash(&input),
            512 * 1024 * 1024,
            &cancelled
        ),
        Err(PcmError::Cancelled)
    ));
    assert!(calls.get() >= 4);
}
