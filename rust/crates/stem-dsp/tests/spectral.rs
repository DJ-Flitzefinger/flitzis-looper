use flitzis_stem_dsp::{
    CHANNELS, Error, FFT, Geometry, MAX_CHUNK_FRAMES, MAX_TRANSIENT_BYTES, RATE, STEMS, Spectrum,
};
use std::cell::Cell;

fn signal(frames: usize) -> Vec<f32> {
    (0..CHANNELS * frames)
        .map(|i| ((i * 31 % 103) as f32 - 51.0) / 100.0)
        .collect()
}
fn identity(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| if i % 2 == 0 { 1.0 } else { 0.0 })
        .collect()
}

#[test]
fn complete_reflected_odd_tail_and_stereo_identity() {
    for frames in [1025, 2048, 4099] {
        let mut pcm = signal(frames);
        pcm[0] = 1.0;
        pcm[frames - 1] = -0.9;
        pcm[frames] = -0.8;
        pcm[2 * frames - 1] = 0.7;
        let spectrum = Spectrum::analyze(&pcm, RATE, &|| false).unwrap();
        let output = spectrum
            .reconstruct(&identity(spectrum.geometry().mask_len()), &|| false)
            .unwrap();
        assert_eq!(output.len(), STEMS * CHANNELS * frames);
        for stem in output.chunks_exact(pcm.len()) {
            assert!(stem.iter().zip(&pcm).all(|(a, b)| (a - b).abs() < 2e-6));
        }
    }
}

#[test]
fn four_distinct_real_masks_preserve_source_order_and_zero() {
    let pcm = signal(2501);
    let spectrum = Spectrum::analyze(&pcm, RATE, &|| false).unwrap();
    let mut masks = identity(spectrum.geometry().mask_len());
    for (stem, mask) in masks
        .chunks_exact_mut(spectrum.geometry().feature_len())
        .enumerate()
    {
        for value in mask {
            *value *= [0.0, -1.0, 0.5, 2.0][stem];
        }
    }
    let output = spectrum.reconstruct(&masks, &|| false).unwrap();
    for (stem, chunk) in output.chunks_exact(pcm.len()).enumerate() {
        assert!(
            chunk
                .iter()
                .zip(&pcm)
                .all(|(a, b)| (a - b * [0.0, -1.0, 0.5, 2.0][stem]).abs() < 3e-6)
        );
    }
}

#[test]
fn complete_geometry_is_admitted_before_payloads() {
    for n in [0, 1, FFT / 2, MAX_CHUNK_FRAMES + 1, usize::MAX] {
        assert!(matches!(Geometry::new(n, RATE), Err(Error::Shape)));
    }
    assert!(matches!(Geometry::new(2048, 48_000), Err(Error::Shape)));
    let g = Geometry::new(MAX_CHUNK_FRAMES, RATE).unwrap();
    assert_eq!(g.steps(), 1101);
    assert_eq!(g.feature_len(), 1101 * 4100);
    assert!(g.transient_bytes() < MAX_TRANSIENT_BYTES);
    assert!(matches!(
        Spectrum::analyze(&[0.0; 2051], RATE, &|| false),
        Err(Error::Shape)
    ));
}

#[test]
fn malformed_nonfinite_and_overflowing_values_never_return_output() {
    let mut pcm = signal(2049);
    pcm[0] = f32::NAN;
    assert!(matches!(
        Spectrum::analyze(&pcm, RATE, &|| false),
        Err(Error::NonFinite)
    ));
    let spectrum = Spectrum::analyze(&signal(2049), RATE, &|| false).unwrap();
    let mut masks = identity(spectrum.geometry().mask_len());
    assert!(matches!(
        spectrum.reconstruct(&masks[..masks.len() - 1], &|| false),
        Err(Error::Shape)
    ));
    masks[1] = f32::INFINITY;
    assert!(matches!(
        spectrum.reconstruct(&masks, &|| false),
        Err(Error::NonFinite)
    ));
    masks.fill(f32::MAX);
    assert!(matches!(
        spectrum.reconstruct(&masks, &|| false),
        Err(Error::NonFinite)
    ));
}

#[test]
fn cancellation_is_terminal_before_and_during_both_transforms() {
    let pcm = signal(8193);
    assert!(matches!(
        Spectrum::analyze(&pcm, RATE, &|| true),
        Err(Error::Cancelled)
    ));
    let calls = Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 8
    };
    assert!(matches!(
        Spectrum::analyze(&pcm, RATE, &cancelled),
        Err(Error::Cancelled)
    ));
    let spectrum = Spectrum::analyze(&pcm, RATE, &|| false).unwrap();
    let masks = identity(spectrum.geometry().mask_len());
    assert!(matches!(
        spectrum.reconstruct(&masks, &|| true),
        Err(Error::Cancelled)
    ));
    // Cross the entire input validation before cancelling inside reconstruction.
    calls.set(0);
    let after_validation = masks.len().div_ceil(4096) + spectrum.geometry().steps() + 10;
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > after_validation
    };
    assert!(matches!(
        spectrum.reconstruct(&masks, &cancelled),
        Err(Error::Cancelled)
    ));
}
