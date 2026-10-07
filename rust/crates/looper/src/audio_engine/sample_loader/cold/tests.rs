use super::*;
use std::cell::Cell;

fn decoded(samples: Vec<f32>, channels: usize, rate_hz: u32) -> DecodedAudio {
    DecodedAudio {
        samples,
        channels,
        rate_hz,
        decoder: DecoderDescriptor {
            codec: "fixture".into(),
            container: "fixture".into(),
            default_track_id: 0,
            declared_max_packet_frames: None,
            codec_config_sha256: None,
            codec_block_frames: None,
            declared_frames: None,
            delay_frames: None,
            padding_frames: None,
            silenced_frames: 0,
            skipped_packets: 0,
        },
    }
}

/// Independent fully padded raw-FFT oracle: no partial-input calls, finite-tail
/// helper, in-place movement, progress trim or production frame calculations.
fn padded_reference(
    samples: &[f32],
    channels: usize,
    source_rate: u32,
    target_rate: u32,
) -> Vec<f32> {
    if source_rate == target_rate {
        return samples.to_vec();
    }
    let frames = samples.len() / channels;
    let expected = ((frames as u64 * u64::from(target_rate) + u64::from(source_rate) - 1)
        / u64::from(source_rate)) as usize;
    let padded_frames = (frames + 16_384).div_ceil(1024) * 1024;
    let mut padded = vec![0.0; padded_frames * channels];
    padded[..samples.len()].copy_from_slice(samples);
    let input = InterleavedOwned::new_from(padded, channels, padded_frames).unwrap();
    let mut resampler = Fft::<f32>::new(
        source_rate as usize,
        target_rate as usize,
        1024,
        1,
        channels,
        FixedSync::Input,
    )
    .unwrap();
    let output_frames = padded_frames * 4 + 65_536;
    let mut output =
        InterleavedOwned::new_from(vec![0.0; output_frames * channels], channels, output_frames)
            .unwrap();
    let delay = resampler.output_delay();
    let mut indexing = Indexing {
        input_offset: 0,
        output_offset: 0,
        partial_len: None,
        active_channels_mask: None,
    };
    while indexing.output_offset < delay + expected {
        let (consumed, produced) = resampler
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .unwrap();
        assert!(consumed > 0);
        indexing.input_offset += consumed;
        indexing.output_offset += produced;
    }
    output.take_data()[delay * channels..(delay + expected) * channels].to_vec()
}

#[test]
fn playback_matches_independent_full_zero_padded_fft_for_all_boundaries() {
    for source_rate in [44_100, 48_000, 96_000] {
        for target_rate in [44_100, 48_000, 96_000] {
            for channels in [1, 2] {
                for frames in [1, 17, 1023, 1024, 1025, 4095, 4096, 11_023] {
                    let mut input = vec![0.0; frames * channels];
                    for marker in [0, frames / 2, frames - 1] {
                        for channel in 0..channels {
                            input[marker * channels + channel] += (channel as f32 + 1.0) * 0.25;
                        }
                    }
                    let expected = padded_reference(&input, channels, source_rate, target_rate);
                    let source = decoded(input, channels, source_rate);
                    let (playback, transform) = prepare_playback(
                        &source,
                        channels,
                        target_rate,
                        usize::MAX,
                        &|| false,
                        |_| {},
                    )
                    .unwrap();
                    assert_eq!(playback.samples.len(), expected.len());
                    assert_eq!(transform.source_frames, frames);
                    assert_eq!(transform.output_frames, expected.len() / channels);
                    for (index, (actual, expected)) in
                        playback.samples.iter().zip(&expected).enumerate()
                    {
                        assert!(
                            (actual - expected).abs() < 2e-6,
                            "source={source_rate} target={target_rate} channels={channels} frames={frames} sample={index}: {actual} != {expected}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn playback_impulse_origin_tail_and_exact_integer_ceiling_hold_at_all_rates() {
    for source_rate in [44_100, 48_000, 96_000] {
        for target_rate in [44_100, 48_000, 96_000] {
            let frames = 11_023;
            for marker in [0, 17, 1023, frames - 1] {
                let mut input = vec![0.0; frames];
                input[marker] = 0.75;
                let (samples, _) = resample_bounded(
                    input,
                    1,
                    source_rate,
                    target_rate,
                    usize::MAX,
                    &|| false,
                    |_| {},
                )
                .unwrap();
                let expected_frames =
                    ((frames as u64 * u64::from(target_rate) + u64::from(source_rate) - 1)
                        / u64::from(source_rate)) as usize;
                assert_eq!(samples.len(), expected_frames);
                let position = (marker as f64 * f64::from(target_rate) / f64::from(source_rate))
                    .round() as usize;
                let position = position.min(samples.len() - 1);
                let (peak, value) = samples
                    .iter()
                    .enumerate()
                    .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
                    .unwrap();
                assert!(
                    peak.abs_diff(position) <= 1,
                    "source={source_rate},target={target_rate},marker={marker},peak={peak},expected={position}"
                );
                assert!(*value > 0.1, "actual tail impulse was lost");
                // The pinned FFT reports integer output delay. Fractional
                // conversion can retain less than one output frame of phase;
                // the independent raw oracle above proves that exact phase.
                if marker == 0 {
                    assert!(peak <= 1, "source zero moved beyond integer-delay phase");
                }
            }
        }
    }
}

#[test]
fn empty_and_silence_keep_exact_extent_same_rate_is_bit_exact() {
    for source_rate in [44_100, 48_000, 96_000] {
        for target_rate in [44_100, 48_000, 96_000] {
            assert!(
                resample_bounded(
                    vec![],
                    2,
                    source_rate,
                    target_rate,
                    usize::MAX,
                    &|| false,
                    |_| {}
                )
                .unwrap()
                .0
                .is_empty()
            );
            let (silence, _) = resample_bounded(
                vec![0.0; 2 * 1025],
                2,
                source_rate,
                target_rate,
                usize::MAX,
                &|| false,
                |_| {},
            )
            .unwrap();
            assert_eq!(
                silence.len(),
                2 * ((1025_u64 * u64::from(target_rate) + u64::from(source_rate) - 1)
                    / u64::from(source_rate)) as usize
            );
            assert!(silence.iter().all(|sample| *sample == 0.0));
        }
        let original = vec![0.0, -0.0, -0.75, 0.25, 0.0, f32::MIN_POSITIVE];
        let source = decoded(original.clone(), 2, source_rate);
        let (sample, _) =
            prepare_playback(&source, 2, source_rate, usize::MAX, &|| false, |_| {}).unwrap();
        assert_eq!(
            sample
                .samples
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            original
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(source.samples, original);
    }
}

#[test]
fn decoder_wav_full_origin_rate_channels_and_zero_delay_are_real_samples() {
    for rate in [44_100, 48_000, 96_000] {
        for channels in [1_u16, 2] {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("full.wav");
            let mut pcm = vec![0_i16; 11_023 * usize::from(channels)];
            pcm[0] = 16_384;
            let last = pcm.len() - 1;
            pcm[last] = -16_384;
            super::super::tests::write_pcm16_wav(&path, channels, rate, &pcm).unwrap();
            let source = decode_audio_snapshot(
                File::open(&path).unwrap(),
                &path,
                rate,
                usize::MAX,
                &|| false,
                |_| {},
            )
            .unwrap();
            assert_eq!(source.rate_hz, rate);
            assert_eq!(source.channels, usize::from(channels));
            assert_eq!(source.samples.len(), pcm.len());
            for (actual, expected) in source.samples.iter().zip(pcm) {
                assert_eq!(*actual, f32::from(expected) / 32768.0);
            }
            let policy = source.decoder.to_json();
            assert_eq!(policy["format_options"]["enable_gapless"], false);
            assert_eq!(policy["origin_frames"], 0);
            assert_eq!(policy["trimmed_delay_frames"], 0);
            assert_eq!(policy["trimmed_padding_frames"], 0);
            assert_eq!(policy["packet_error_silence_frames"], 0);
        }
    }
}

#[test]
fn bounded_conversion_and_decode_reject_before_unadmitted_pcm_and_cancel_cooperatively() {
    let source = decoded(vec![0.0; 30_000], 1, 48_000);
    assert!(matches!(
        prepare_playback(
            &source,
            2,
            44_100,
            source.samples.len() * 4,
            &|| false,
            |_| {}
        ),
        Err(SampleLoadError::Limit(_))
    ));
    assert!(matches!(
        prepare_playback(&source, 2, 44_100, usize::MAX, &|| true, |_| {}),
        Err(SampleLoadError::Cancelled)
    ));
    for stop in [3, 11, 20] {
        let calls = Cell::new(0);
        let result = prepare_playback(
            &source,
            2,
            44_100,
            usize::MAX,
            &|| {
                calls.set(calls.get() + 1);
                calls.get() == stop
            },
            |_| {},
        );
        assert!(
            matches!(result, Err(SampleLoadError::Cancelled)),
            "stop={stop}"
        );
    }
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("full.wav");
    super::super::tests::write_pcm16_wav(&path, 1, 48_000, &vec![0; 30_000]).unwrap();
    assert!(matches!(
        decode_audio_snapshot(
            File::open(&path).unwrap(),
            &path,
            48_000,
            1,
            &|| false,
            |_| {}
        ),
        Err(SampleLoadError::Limit(_))
    ));
    let calls = Cell::new(0);
    assert!(matches!(
        decode_audio_snapshot(
            File::open(&path).unwrap(),
            &path,
            48_000,
            usize::MAX,
            &|| {
                calls.set(calls.get() + 1);
                calls.get() == 5
            },
            |_| {}
        ),
        Err(SampleLoadError::Cancelled)
    ));
}

#[test]
fn small_declared_packets_cannot_reduce_codec_workspace_admission() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("tiny.wav");
    super::super::tests::write_pcm16_wav(&path, 1, 48_000, &[16_384]).unwrap();
    // This real file declares a small packet and decodes to one frame. The
    // conservative codec workspace must still be admitted before construction.
    assert!(matches!(
        decode_audio_snapshot(
            File::open(&path).unwrap(),
            &path,
            48_000,
            1024 * 1024,
            &|| false,
            |_| {}
        ),
        Err(SampleLoadError::Limit(_))
    ));
    let source = decode_audio_snapshot(
        File::open(&path).unwrap(),
        &path,
        48_000,
        8 * 1024 * 1024,
        &|| false,
        |_| {},
    )
    .unwrap();
    assert_eq!(source.samples, [0.5]);
    assert!(
        source.decoder.to_json()["declared_max_packet_frames"]
            .as_u64()
            .unwrap()
            < FALLBACK_PACKET_FRAMES as u64
    );
}

#[test]
fn malformed_dimensions_and_nonfinite_pcm_fail_without_a_partial_playback_buffer() {
    for source in [
        decoded(vec![0.0], 2, 48_000),
        decoded(vec![0.0], 0, 48_000),
        decoded(vec![0.0], 1, 0),
        decoded(vec![f32::NAN], 1, 48_000),
        decoded(vec![f32::INFINITY], 1, 48_000),
    ] {
        assert!(prepare_playback(&source, 2, 48_000, usize::MAX, &|| false, |_| {}).is_err());
    }
    let source = decoded(vec![f32::MAX, f32::MAX], 2, 48_000);
    assert!(matches!(
        prepare_playback(&source, 1, 48_000, usize::MAX, &|| false, |_| {}),
        Err(SampleLoadError::InvalidInput(_))
    ));
}

#[test]
fn actual_empty_corrupt_and_nonfinite_decoder_inputs_fail_safely() {
    use std::io::Write;
    let temporary = tempfile::tempdir().unwrap();
    let empty = temporary.path().join("empty.wav");
    super::super::tests::write_pcm16_wav(&empty, 1, 48_000, &[]).unwrap();
    assert!(matches!(
        decode_audio_snapshot(
            File::open(&empty).unwrap(),
            &empty,
            48_000,
            512 * 1024 * 1024,
            &|| false,
            |_| {}
        ),
        Err(SampleLoadError::NoDecodedFrames) | Err(SampleLoadError::Decode(_))
    ));
    let corrupt = temporary.path().join("corrupt.wav");
    std::fs::write(&corrupt, b"RIFF\0\0\0\0WAVEfmt ").unwrap();
    assert!(
        decode_audio_snapshot(
            File::open(&corrupt).unwrap(),
            &corrupt,
            48_000,
            512 * 1024 * 1024,
            &|| false,
            |_| {}
        )
        .is_err()
    );
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let path = temporary.path().join("float.wav");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&44_u32.to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&3_u16.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&96_000_u32.to_le_bytes()).unwrap();
        file.write_all(&384_000_u32.to_le_bytes()).unwrap();
        file.write_all(&4_u16.to_le_bytes()).unwrap();
        file.write_all(&32_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&8_u32.to_le_bytes()).unwrap();
        file.write_all(&0.25_f32.to_le_bytes()).unwrap();
        file.write_all(&invalid.to_le_bytes()).unwrap();
        drop(file);
        assert!(matches!(
            decode_audio_snapshot(
                File::open(&path).unwrap(),
                &path,
                96_000,
                512 * 1024 * 1024,
                &|| false,
                |_| {}
            ),
            Err(SampleLoadError::InvalidInput("non-finite decoder sample"))
        ));
    }
}

/// Independent codec files and FFmpeg/ffprobe evidence are generated offline in
/// workspace scratch. MP3 supports <=48k; the96k boundary uses FLAC/AIFF/Vorbis/AAC.
#[test]
#[ignore = "requires explicit independently generated codec fixture manifest"]
fn independent_codec_boundary_probe() {
    use sha2::{Digest, Sha256};
    let manifest = std::env::var("FLITZI_COLD_DECODER_MANIFEST").expect("codec manifest");
    let output = std::env::var("FLITZI_COLD_DECODER_OUTPUT").expect("codec evidence output");
    let records: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let mut evidence = Vec::new();
    for fixture in records.as_array().unwrap() {
        let path = Path::new(fixture["path"].as_str().unwrap());
        let rate = fixture["rate_hz"].as_u64().unwrap() as u32;
        let source = decode_audio_snapshot(
            File::open(path).unwrap(),
            path,
            rate,
            512 * 1024 * 1024,
            &|| false,
            |_| {},
        )
        .unwrap();
        if fixture["format"] == "alac" {
            let mut forged = std::fs::read(path).unwrap();
            let atom = forged
                .windows(8)
                .position(|bytes| bytes == b"\0\0\0\x24alac")
                .expect("independent FFmpeg ALAC configuration atom");
            assert!(atom + 36 <= forged.len());
            // A gigantic cookie frame length used to allocate inside make()
            // before any decoded-buffer capacity became observable to us.
            forged[atom + 12..atom + 16].copy_from_slice(&0x7fff_ffff_u32.to_be_bytes());
            let temporary = tempfile::tempdir().unwrap();
            let forged_path = temporary.path().join("oversized-alac.m4a");
            std::fs::write(&forged_path, forged).unwrap();
            assert!(matches!(
                decode_audio_snapshot(
                    File::open(&forged_path).unwrap(),
                    &forged_path,
                    rate,
                    1024 * 1024 * 1024,
                    &|| false,
                    |_| {}
                ),
                Err(SampleLoadError::Limit(_))
            ));
        }
        let reference_bytes = std::fs::read(fixture["ffmpeg_pcm"].as_str().unwrap()).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(path).unwrap())),
            fixture["original_sha256"].as_str().unwrap()
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(&reference_bytes)),
            fixture["ffmpeg_pcm_sha256"].as_str().unwrap()
        );
        let reference: Vec<f32> = reference_bytes
            .chunks_exact(4)
            .map(|value| f32::from_le_bytes(value.try_into().unwrap()))
            .collect();
        let policy = source.decoder.to_json();
        assert_eq!(source.rate_hz, rate);
        assert_eq!(
            source.channels,
            fixture["channels"].as_u64().unwrap() as usize
        );
        assert_eq!(policy["trimmed_delay_frames"], 0);
        assert_eq!(policy["trimmed_padding_frames"], 0);
        let skip = fixture["skip_samples"].as_u64().unwrap() as usize;
        let mut candidates = vec![0, skip];
        if let Some(delay) = policy["declared_delay_frames"].as_u64() {
            candidates.push(delay as usize);
        }
        let mut comparisons = Vec::new();
        for start in candidates {
            let count = reference
                .len()
                .min(source.samples.len().saturating_sub(start));
            if count == 0 {
                continue;
            }
            let squared_error: f64 = source.samples[start..start + count]
                .iter()
                .zip(&reference)
                .map(|(left, right)| (f64::from(*left) - f64::from(*right)).powi(2))
                .sum();
            let maximum_error = source.samples[start..start + count]
                .iter()
                .zip(&reference)
                .map(|(left, right)| (*left - *right).abs())
                .fold(0.0_f32, f32::max);
            comparisons.push(json!({"start":start,"count":count,"rms_error":(squared_error/count as f64).sqrt(),"maximum_error":maximum_error}));
        }
        let peak = source
            .samples
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
            .unwrap()
            .0;
        if matches!(
            fixture["format"].as_str().unwrap(),
            "flac" | "aiff" | "alac"
        ) {
            assert_eq!(
                source.samples.len(),
                fixture["input_frames"].as_u64().unwrap() as usize
            );
            assert_eq!(source.samples, reference);
            assert_eq!(source.samples[0], 0.5);
            assert_eq!(*source.samples.last().unwrap(), -0.5);
        }
        if fixture["format"] == "mp3" {
            assert_eq!(
                source.samples.len(),
                fixture["raw_frames"].as_u64().unwrap() as usize
            );
            assert_eq!(
                source.samples.len(),
                reference.len() + skip + fixture["discard_padding"].as_u64().unwrap() as usize
            );
            let matched = comparisons
                .iter()
                .find(|comparison| comparison["start"].as_u64() == Some(skip as u64))
                .unwrap();
            assert!(
                matched["maximum_error"].as_f64().unwrap() < 1e-4,
                "MP3 independent decoder mismatch: {matched}"
            );
        }
        if fixture["format"] == "ogg" {
            assert_eq!(source.samples.len(), reference.len());
            let matched = comparisons
                .iter()
                .find(|comparison| comparison["start"] == 0)
                .unwrap();
            assert!(
                matched["maximum_error"].as_f64().unwrap() < 1e-5,
                "Vorbis independent decoder mismatch: {matched}"
            );
        }
        if fixture["format"] == "m4a" {
            assert_eq!(
                source.samples.len(),
                reference.len() + skip,
                "AAC disabled gapless retains exactly independent packet priming"
            );
            let matched = comparisons
                .iter()
                .find(|comparison| comparison["start"].as_u64() == Some(skip as u64))
                .unwrap();
            assert_eq!(matched["count"].as_u64().unwrap(), reference.len() as u64);
            assert!(
                matched["maximum_error"].as_f64().unwrap() < 1e-5,
                "AAC independent decoder mismatch: {matched}"
            );
        }
        for target_rate in [44_100, 48_000, 96_000] {
            let (playback, transform) = prepare_playback(
                &source,
                source.channels,
                target_rate,
                512 * 1024 * 1024,
                &|| false,
                |_| {},
            )
            .unwrap();
            let oracle = padded_reference(
                &source.samples,
                source.channels,
                source.rate_hz,
                target_rate,
            );
            assert_eq!(playback.samples.len(), oracle.len());
            assert!(
                playback
                    .samples
                    .iter()
                    .zip(&oracle)
                    .all(|(left, right)| (*left - *right).abs() < 2e-6)
            );
            assert_eq!(transform.to_json()["origin_frames"], 0);
        }
        let mut pcm_digest = Sha256::new();
        for sample in &source.samples {
            pcm_digest.update(sample.to_le_bytes());
        }
        let decoded_pcm_digest = format!("{:x}", pcm_digest.finalize());
        evidence.push(json!({"fixture":fixture,"decoder":policy,
            "decoded_pcm_sha256":decoded_pcm_digest,
            "oversized_codec_configuration_rejected": fixture["format"] == "alac",
            "decoded_frames":source.samples.len()/source.channels,"peak_frame":peak,
            "independent_comparisons":comparisons,"playback_rates_verified":[44100,48000,96000]}));
    }
    std::fs::write(
        output,
        serde_json::to_vec_pretty(&json!({"version":1,"cases":evidence})).unwrap(),
    )
    .unwrap();
}
