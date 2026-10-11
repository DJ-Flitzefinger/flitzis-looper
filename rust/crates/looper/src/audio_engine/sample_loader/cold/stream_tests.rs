//! Productive disk decoder/playback compared with the independent padded FFT.
#![cfg(windows)]
use super::*;
use std::io::Write;

fn source(path: &Path, frames: usize, channels: usize, rate: u32, rf64: bool) {
    let mut writer = File::create(path).unwrap();
    let bytes = frames as u64 * channels as u64 * 2;
    if rf64 {
        writer
            .write_all(b"RF64\xff\xff\xff\xffWAVEds64\x1c\0\0\0")
            .unwrap();
        writer.write_all(&(bytes + 72).to_le_bytes()).unwrap();
        writer.write_all(&bytes.to_le_bytes()).unwrap();
        writer.write_all(&(frames as u64).to_le_bytes()).unwrap();
        writer.write_all(&0_u32.to_le_bytes()).unwrap();
    } else {
        writer.write_all(b"RIFF").unwrap();
        writer
            .write_all(&(bytes as u32 + 36).to_le_bytes())
            .unwrap();
        writer.write_all(b"WAVE").unwrap();
    }
    writer.write_all(b"fmt \x10\0\0\0\x01\0").unwrap();
    writer.write_all(&(channels as u16).to_le_bytes()).unwrap();
    writer.write_all(&rate.to_le_bytes()).unwrap();
    writer
        .write_all(&(rate * channels as u32 * 2).to_le_bytes())
        .unwrap();
    writer
        .write_all(&(channels as u16 * 2).to_le_bytes())
        .unwrap();
    writer.write_all(&16_u16.to_le_bytes()).unwrap();
    writer.write_all(b"data").unwrap();
    writer
        .write_all(&(if rf64 { u32::MAX } else { bytes as u32 }).to_le_bytes())
        .unwrap();
    for frame in 0..frames {
        for channel in 0..channels {
            let sample = if [0, frames / 2, frames - 1].contains(&frame) {
                if channel == 0 { i16::MIN } else { i16::MAX }
            } else {
                ((frame * 37 + channel * 103) % 8193) as i16 - 4096
            };
            writer.write_all(&sample.to_le_bytes()).unwrap();
        }
    }
    writer.sync_all().unwrap();
}

fn read(artifact: &mut CompletedPcmArtifact) -> Vec<f32> {
    artifact.reader.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    artifact.reader.read_to_end(&mut bytes).unwrap();
    bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}

#[test]
fn streamed_complete_decoder_and_continuous_fft_match_independent_oracle() {
    for input_rate in [44_100, 48_000, 96_000] {
        for output_rate in [44_100, 48_000, 96_000] {
            for channels in [1, 2] {
                for frames in [1, 17, 1023, 1024, 1025, 4095, 4096, 11_023] {
                    let temp = tempfile::tempdir().unwrap();
                    let original = temp.path().join("source.wav");
                    source(&original, frames, channels, input_rate, false);
                    let mut decoded = decode_audio_snapshot_streamed(
                        File::open(&original).unwrap(),
                        &original,
                        &temp.path().join("decoder.f32le"),
                        output_rate,
                        1024 * 1024 * 1024,
                        &|| false,
                        |_| {},
                    )
                    .unwrap();
                    assert_eq!(decoded.frames, frames);
                    let input = read(&mut decoded.artifact);
                    let expected = super::super::tests::padded_reference(
                        &input,
                        channels,
                        input_rate,
                        output_rate,
                    );
                    let (mut artifact, transform) = prepare_playback_streamed(
                        &mut decoded,
                        &temp.path().join("playback.f32le"),
                        channels,
                        output_rate,
                        1024 * 1024 * 1024,
                        &|| false,
                        |_| {},
                    )
                    .unwrap();
                    let actual = read(&mut artifact);
                    assert_eq!(actual.len(), expected.len());
                    assert_eq!(transform.output_frames, expected.len() / channels);
                    for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
                        assert!(
                            (actual - expected).abs() < 2e-6,
                            "input_rate={input_rate} output_rate={output_rate} channels={channels} frames={frames} index={index}: {actual} != {expected}"
                        );
                    }
                    let encoded: Vec<u8> = actual
                        .iter()
                        .flat_map(|sample| sample.to_le_bytes())
                        .collect();
                    assert_eq!(
                        artifact.descriptor["interleaved_sha256"],
                        format!("{:x}", Sha256::digest(&encoded))
                    );
                }
            }
        }
    }
}

#[test]
fn rf64_actual_probe_pcm_decoder_and_fresh_selector_preserve_actual_header_identity() {
    for channels in [1, 2, 4] {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("rf64.wav");
        source(&original, 8193, channels, 48_000, true);
        let selector = decoder_cache_selector(File::open(&original).unwrap(), &original).unwrap();
        assert_eq!(selector["container"], "rf64-wave-ds64-flitzi-demux-v1");
        let mut actual = decode_audio_snapshot_streamed(
            File::open(&original).unwrap(),
            &original,
            &temp.path().join("decoder.f32le"),
            48_000,
            256 * 1024 * 1024,
            &|| false,
            |_| {},
        )
        .unwrap();
        assert_eq!(actual.frames, 8193);
        assert_eq!(actual.channels, channels);
        assert_eq!(actual.artifact.descriptor["provenance"], selector);
        let samples = read(&mut actual.artifact);
        assert_eq!(samples[0], -1.0);
        if channels > 1 {
            assert_eq!(samples[1], 32767.0 / 32768.0);
        }
        assert_eq!(samples[(8193 - 1) * channels], -1.0);
    }
}

#[test]
fn streamed_decoder_and_resampler_cancel_during_actual_complete_work() {
    use std::cell::Cell;
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("source.wav");
    source(&original, 100_003, 2, 44_100, false);
    let calls = Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 20
    };
    assert!(matches!(
        decode_audio_snapshot_streamed(
            File::open(&original).unwrap(),
            &original,
            &temp.path().join("cancel-decoder.f32le"),
            48_000,
            1024 * 1024 * 1024,
            &cancelled,
            |_| {}
        ),
        Err(SampleLoadError::Cancelled) | Err(SampleLoadError::Io(_))
    ));
    assert!(
        temp.path()
            .join("cancel-decoder.f32le")
            .metadata()
            .unwrap()
            .len()
            > 0
    );
    let mut decoded = decode_audio_snapshot_streamed(
        File::open(&original).unwrap(),
        &original,
        &temp.path().join("decoder.f32le"),
        48_000,
        1024 * 1024 * 1024,
        &|| false,
        |_| {},
    )
    .unwrap();
    calls.set(0);
    assert!(
        prepare_playback_streamed(
            &mut decoded,
            &temp.path().join("cancel-playback.f32le"),
            2,
            48_000,
            1024 * 1024 * 1024,
            &cancelled,
            |_| {}
        )
        .is_err()
    );
    assert!(
        temp.path()
            .join("cancel-playback.f32le")
            .metadata()
            .unwrap()
            .len()
            > 0
    );
}

#[test]
fn complete_pcm_signed_mono_and_hashes_are_independent_of_arbitrary_block_partitions() {
    for channels in [1, 2, 4, 8] {
        let temp = tempfile::tempdir().unwrap();
        let samples: Vec<f32> = (0..10_003 * channels)
            .map(|index| {
                if index % channels == 0 {
                    -0.75
                } else {
                    (index % 11) as f32 / 32.0
                }
            })
            .collect();
        let mut baseline = None;
        for (attempt, partitions) in [&[1][..], &[17, 4095, 3, 1023, 65537][..], &[4096][..]]
            .iter()
            .enumerate()
        {
            let path = temp.path().join(format!("partition-{attempt}.f32le"));
            let mut writer = PcmArtifactWriter::create(&path, 48_000, channels).unwrap();
            let mut cursor = 0;
            for frames in partitions.iter().cycle() {
                if cursor == samples.len() {
                    break;
                }
                let end = (cursor + frames * channels).min(samples.len());
                writer.append(&samples[cursor..end], &|| false).unwrap();
                cursor = end;
            }
            let mut artifact = writer
                .finish(json!({"partition-oracle":1}), &|| false)
                .unwrap();
            assert_eq!(read(&mut artifact), samples);
            let mono: Vec<u8> = samples
                .chunks_exact(channels)
                .flat_map(|frame| {
                    let sum: f64 = frame.iter().copied().map(f64::from).sum();
                    ((sum / channels as f64) as f32).to_le_bytes()
                })
                .collect();
            assert_eq!(
                artifact.descriptor["mono_sha256"],
                format!("{:x}", Sha256::digest(&mono))
            );
            if let Some(descriptor) = &baseline {
                assert_eq!(&artifact.descriptor, descriptor);
            } else {
                baseline = Some(artifact.descriptor);
            }
        }
    }
}
