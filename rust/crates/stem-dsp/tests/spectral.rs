use flitzis_stem_dsp::{
    CHANNELS, Error, FFT, Geometry, MAX_CHUNK_FRAMES, MAX_TRANSIENT_BYTES, RATE, STEMS,
    SpectralWorkspace, Spectrum,
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

#[test]
fn reusable_inverse_writes_exact_stereo_spans_and_distinct_complex_stems() {
    // These comparisons cover split layout/context reuse. The identity/scaling
    // controls above and the previous Release binary supply independent math.
    for frames in [1025, 2501, 4099] {
        let spectrum = Spectrum::analyze(&signal(frames), RATE, &|| false).unwrap();
        let g = spectrum.geometry();
        let masks: Vec<_> = (0..g.mask_len())
            .map(|i| ((i * 17 % 79) as f32 - 39.0) * 0.013)
            .collect();
        let expected = spectrum.reconstruct(&masks, &|| false).unwrap();
        let mut inverse = spectrum.reconstruction(&|| false).unwrap();
        let mut actual = vec![123.0; expected.len() + 2];
        for stem in [3, 1, 0, 2] {
            let begin = 1 + stem * CHANNELS * frames;
            inverse
                .write_stem(
                    &masks[stem * g.feature_len()..(stem + 1) * g.feature_len()],
                    &mut actual[begin..begin + CHANNELS * frames],
                    &|| false,
                )
                .unwrap();
        }
        assert_eq!(actual[0], 123.0);
        assert_eq!(*actual.last().unwrap(), 123.0);
        assert!(
            actual[1..actual.len() - 1]
                .iter()
                .zip(expected)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
    }
}

#[test]
fn inverse_destination_failures_and_late_cancel_invalidate_private_attempt() {
    let frames = 8193;
    let spectrum = Spectrum::analyze(&signal(frames), RATE, &|| false).unwrap();
    let g = spectrum.geometry();
    let mut inverse = spectrum.reconstruction(&|| false).unwrap();
    let mut mask = identity(g.feature_len());
    let mut output = vec![123.0; CHANNELS * frames];
    assert_eq!(
        inverse.write_stem(&mask[..mask.len() - 1], &mut output, &|| false),
        Err(Error::Shape)
    );
    assert_eq!(
        inverse.write_stem(&mask, &mut output[..1], &|| false),
        Err(Error::Shape)
    );
    mask[1] = f32::NAN;
    assert_eq!(
        inverse.write_stem(&mask, &mut output, &|| false),
        Err(Error::NonFinite)
    );
    assert!(output.iter().all(|x| *x == 123.0));
    mask[1] = 0.0;
    // Finish validation and one complete channel, then cancel in channel two.
    let calls = Cell::new(0);
    let limit = mask.len().div_ceil(4096) + g.steps() + frames.div_ceil(4096) + 1;
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > limit
    };
    assert_eq!(
        inverse.write_stem(&mask, &mut output, &cancelled),
        Err(Error::Cancelled)
    );
    assert!(output[..frames].iter().any(|x| *x != 123.0));
    assert!(output[frames..].iter().all(|x| *x == 123.0));
    // A completed earlier stem cannot turn this failed attempt into success.
    mask.fill(f32::MAX);
    assert_eq!(
        inverse.write_stem(&mask, &mut output, &|| false),
        Err(Error::NonFinite)
    );
}

#[test]
fn retained_workspace_rebuilds_actual_envelopes_and_preserves_complete_bits() {
    let mut workspace = SpectralWorkspace::new(MAX_CHUNK_FRAMES, RATE, &|| false).unwrap();
    // Shorter geometry after a long one must not inherit extra trailing windows.
    // Include the original full485100/1101 spectral geometry; no learned network.
    for frames in [4099, 1025, 2501, 4099, MAX_CHUNK_FRAMES, 1025] {
        let pcm = signal(frames);
        let original = Spectrum::analyze(&pcm, RATE, &|| false).unwrap();
        let prepared = workspace.analyze(&pcm, RATE, &|| false).unwrap();
        assert!(
            original
                .features()
                .iter()
                .zip(prepared.features())
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        let g = original.geometry();
        let masks: Vec<_> = (0..g.mask_len())
            .map(|i| ((i * 17 % 79) as f32 - 39.0) * 0.013)
            .collect();
        let expected = original.reconstruct(&masks, &|| false).unwrap();
        let actual = workspace.reconstruct(&prepared, &masks, &|| false).unwrap();
        assert!(
            actual
                .iter()
                .zip(&expected)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        // The same borrowed inverse also serves the single-mask learned path.
        let mut inverse = workspace.reconstruction(&prepared, &|| false).unwrap();
        let mut target = vec![123.0; CHANNELS * frames + 2];
        for stem in [3, 1, 0, 2] {
            inverse
                .write_stem(
                    &masks[stem * g.feature_len()..(stem + 1) * g.feature_len()],
                    &mut target[1..1 + CHANNELS * frames],
                    &|| false,
                )
                .unwrap();
            assert_eq!(target[0], 123.0);
            assert_eq!(*target.last().unwrap(), 123.0);
            assert!(
                target[1..1 + CHANNELS * frames]
                    .iter()
                    .zip(&expected[stem * CHANNELS * frames..(stem + 1) * CHANNELS * frames])
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
        }
    }
}

#[test]
fn retained_workspace_recovers_after_partial_work_and_rejects_extent_changes() {
    assert!(matches!(
        SpectralWorkspace::new(1025, RATE, &|| true),
        Err(Error::Cancelled)
    ));
    let frames = 8193;
    let pcm = signal(frames);
    let mut workspace = SpectralWorkspace::new(frames, RATE, &|| false).unwrap();
    assert!(matches!(
        workspace.analyze(&signal(frames + 1), RATE, &|| false),
        Err(Error::Shape)
    ));
    assert!(matches!(
        workspace.analyze(&pcm[..pcm.len() - 1], RATE, &|| false),
        Err(Error::Shape)
    ));
    assert!(matches!(
        workspace.analyze(&pcm, 48_000, &|| false),
        Err(Error::Shape)
    ));
    let calls = Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 8
    };
    assert!(matches!(
        workspace.analyze(&pcm, RATE, &cancelled),
        Err(Error::Cancelled)
    ));
    let mut bad = pcm.clone();
    bad[frames] = f32::NAN;
    assert!(matches!(
        workspace.analyze(&bad, RATE, &|| false),
        Err(Error::NonFinite)
    ));
    let spectrum = workspace.analyze(&pcm, RATE, &|| false).unwrap();
    let g = spectrum.geometry();
    let masks = identity(g.mask_len());
    // Cancel while rebuilding the borrowed envelope, then rebuild it completely.
    calls.set(0);
    assert!(matches!(
        workspace.reconstruction(&spectrum, &cancelled),
        Err(Error::Cancelled)
    ));
    let mut output = vec![123.0; CHANNELS * frames];
    {
        let mut inverse = workspace.reconstruction(&spectrum, &|| false).unwrap();
        let mask = &masks[..g.feature_len()];
        assert_eq!(
            inverse.write_stem(mask, &mut output[..1], &|| false),
            Err(Error::Shape)
        );
        assert!(output.iter().all(|x| *x == 123.0));
        calls.set(0);
        let limit = mask.len().div_ceil(4096) + g.steps() + frames.div_ceil(4096) + 1;
        let cancel_channel_two = || {
            calls.set(calls.get() + 1);
            calls.get() > limit
        };
        assert_eq!(
            inverse.write_stem(mask, &mut output, &cancel_channel_two),
            Err(Error::Cancelled)
        );
        assert!(output[..frames].iter().any(|x| *x != 123.0));
        assert!(output[frames..].iter().all(|x| *x == 123.0));
        let overflow = vec![f32::MAX; g.feature_len()];
        assert_eq!(
            inverse.write_stem(&overflow, &mut output, &|| false),
            Err(Error::NonFinite)
        );
    }
    let too_large = Spectrum::analyze(&signal(frames + 1), RATE, &|| false).unwrap();
    assert!(matches!(
        workspace.reconstruction(&too_large, &|| false),
        Err(Error::Shape)
    ));
    let expected = spectrum.reconstruct(&masks, &|| false).unwrap();
    let actual = workspace.reconstruct(&spectrum, &masks, &|| false).unwrap();
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
}
