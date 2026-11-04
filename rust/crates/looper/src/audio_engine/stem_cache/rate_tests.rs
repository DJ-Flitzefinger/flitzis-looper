//! Full-output oracle for the Legacy cache's target-clock conversion.
use super::streamed::AlignedStemStream;
use super::streamed_tests::write_words;
use super::*;
use crate::audio_engine::{complete_context::CompleteSourceReader, sample_loader};
use std::cell::Cell;
use std::sync::Arc;

fn raw_values(frames: usize, index: usize) -> Vec<i16> {
    (0..frames * 2)
        .map(|sample| {
            let frame = sample / 2;
            let sign = if sample % 2 == 0 { 1 } else { -1 };
            let value = if [0, 31, frames / 2, frames - 1].contains(&frame) {
                5000 + index * 1000
            } else {
                (frame * 37 + index * 103) % 257
            };
            (value as i16) * sign
        })
        .collect()
}

fn reference(outputs: &[Vec<f32>]) -> CompleteSourceReader {
    let mut mixed = vec![0.0; outputs[0].len()];
    for component in &outputs[..4] {
        for (mix, value) in mixed.iter_mut().zip(component) {
            *mix += *value;
        }
    }
    CompleteSourceReader::new(
        SampleBuffer {
            residency: None,
            channels: 2,
            samples: Arc::from(mixed),
        },
        None,
    )
    .unwrap()
}

#[test]
fn legacy_rate_conversion_matches_independent_full_output_and_preserves_five_wavs() {
    for (input_rate, target_rate, frames) in [
        (48_000, 48_000, 11023),
        (48_000, 96_000, 11023),
        (96_000, 48_000, 11023),
        (44_100, 48_000, 11023),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut outputs = Vec::new();
        let mut hashes = Vec::new();
        let mut files = Vec::new();
        for index in 0..5 {
            let path = temp.path().join(format!("{index}.wav"));
            let words = raw_values(frames, index);
            write_words(&path, input_rate, 2, &words, index % 2 != 0);
            hashes.push(Sha256::digest(fs::read(&path).unwrap()));
            outputs.push(sample_loader::padded_reference(
                &words.into_iter().map(pcm16_to_float).collect::<Vec<_>>(),
                2,
                input_rate,
                target_rate,
            ));
            files.push(File::open(path).unwrap());
        }
        let source = reference(&outputs);
        for partition in [17, 2048] {
            let mut stream = AlignedStemStream::prepare_partitioned(
                &source,
                &files,
                target_rate,
                &|| false,
                partition,
            )
            .unwrap();
            assert_eq!(stream.offset_frames, 0);
            for (index, expected) in outputs.iter().enumerate() {
                let mut bytes = Vec::new();
                let (_, length) = stream.convert(index, &mut bytes, &|| false).unwrap();
                assert_eq!(length as usize, expected.len() * 4);
                let actual = bytes
                    .chunks_exact(4)
                    .map(|word| f32::from_le_bytes(word.try_into().unwrap()))
                    .collect::<Vec<_>>();
                assert!(actual.iter().all(|value| value.is_finite()));
                assert_eq!(
                    actual, *expected,
                    "{input_rate}->{target_rate}, partition{partition}, stem{index}"
                );
            }
            for shift in [-31_isize, 29] {
                stream.offset_frames = shift;
                let mut bytes = Vec::new();
                stream.convert(0, &mut bytes, &|| false).unwrap();
                for (sample, word) in bytes.chunks_exact(4).enumerate() {
                    let frame = sample / 2;
                    let expected = frame
                        .checked_add_signed(shift)
                        .filter(|&n| n < outputs[0].len() / 2)
                        .map_or(0.0, |n| outputs[0][n * 2 + sample % 2]);
                    assert_eq!(f32::from_le_bytes(word.try_into().unwrap()), expected);
                }
            }
        }
        for (index, expected) in hashes.iter().enumerate() {
            assert_eq!(
                &Sha256::digest(fs::read(temp.path().join(format!("{index}.wav"))).unwrap()),
                expected
            );
        }
    }
}

#[test]
fn legacy_fifth_wav_geometry_and_target_extent_fail_closed() {
    let frames = 11_023;
    for (rate, channels, last_frames, target_frames) in [
        (44_100, 2, frames, frames * 2),
        (48_000, 1, frames, frames * 2),
        (48_000, 2, frames - 1, frames * 2),
        (48_000, 2, frames, frames * 2 + 1),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        for index in 0..5 {
            let (rate, channels, count) = if index == 4 {
                (rate, channels, last_frames)
            } else {
                (48_000, 2, frames)
            };
            let words = vec![1000_i16; count * channels];
            let path = temp.path().join(format!("{index}.wav"));
            write_words(&path, rate, channels, &words, false);
            files.push(File::open(path).unwrap());
        }
        let source = reference(&vec![vec![0.0; target_frames * 2]; 5]);
        assert!(AlignedStemStream::prepare(&source, &files, 96_000, &|| false).is_err());
    }
}

#[test]
fn legacy_rate_conversion_rejects_geometry_and_cancels_during_fft() {
    let temp = tempfile::tempdir().unwrap();
    let words = raw_values(11023, 0);
    let output = sample_loader::padded_reference(
        &words
            .iter()
            .copied()
            .map(pcm16_to_float)
            .collect::<Vec<_>>(),
        2,
        48_000,
        96_000,
    );
    let source = reference(&vec![output; 5]);
    let mut files = Vec::new();
    for index in 0..5 {
        let path = temp.path().join(format!("{index}.wav"));
        write_words(
            &path,
            if index == 4 { 44_100 } else { 48_000 },
            2,
            &words,
            false,
        );
        files.push(File::open(path).unwrap());
    }
    assert!(
        AlignedStemStream::prepare(&source, &files, 96_000, &|| false)
            .err()
            .unwrap()
            .contains("sample rate mismatch")
    );
    let calls = Cell::new(0);
    let cancelled = || {
        let n = calls.get() + 1;
        calls.set(n);
        n > 5
    };
    let layout = wav::Pcm16Wav::inspect(&files[0], &|| false).unwrap();
    let mut cursor = rate_reader::StemPcmReader::new(&files[0], layout, 96_000).unwrap();
    let mut samples = vec![0.0; 8192];
    assert!(cursor.read(0, &mut samples, &cancelled).is_err());
    assert!(calls.get() > 5);
    assert!(sample_loader::fft_stream::FftPcmStream::new(11023, 2, 48_000, 96_000, 1).is_err());
    assert!(sample_loader::fft_stream::output_frames(usize::MAX, 8_000, 384_000).is_err());
}

#[test]
fn legacy_original_clock_allows_only_the_derived_nested_ceiling_tail() {
    for original_frames in [1, 44_101] {
        let clock = rate_reader::SourceClock {
            frames: original_frames,
            rate: 44_100,
        };
        let raw_frames = clock.frames_at(48_000).unwrap();
        let target_frames = clock.frames_at(96_000).unwrap();
        assert_eq!(raw_frames * 2, target_frames + 1);
        let temp = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        let mut outputs = Vec::new();
        for index in 0..5 {
            let words = raw_values(raw_frames, index);
            let path = temp.path().join(format!("{index}.wav"));
            write_words(&path, 48_000, 2, &words, false);
            let mut expected = sample_loader::padded_reference(
                &words.into_iter().map(pcm16_to_float).collect::<Vec<_>>(),
                2,
                48_000,
                96_000,
            );
            expected.truncate(target_frames * 2);
            outputs.push(expected);
            files.push(File::open(path).unwrap());
        }
        let source = reference(&outputs);
        assert!(AlignedStemStream::prepare(&source, &files, 96_000, &|| false).is_err());
        let stream =
            AlignedStemStream::prepare_with_clock(&source, &files, 96_000, Some(clock), &|| false)
                .unwrap();
        assert_eq!(stream.offset_frames, 0);
        for (index, expected) in outputs.iter().enumerate() {
            let mut bytes = Vec::new();
            stream.convert(index, &mut bytes, &|| false).unwrap();
            let actual = bytes
                .chunks_exact(4)
                .map(|word| f32::from_le_bytes(word.try_into().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(&actual, expected);
        }
        let wrong_clock = rate_reader::SourceClock {
            frames: original_frames + 1,
            ..clock
        };
        assert!(
            AlignedStemStream::prepare_with_clock(
                &source,
                &files,
                96_000,
                Some(wrong_clock),
                &|| false
            )
            .is_err()
        );
    }
}

#[test]
fn crossrate_conversion_preserves_post_write_cancellation_and_original_io_error() {
    let temp = tempfile::tempdir().unwrap();
    let words = raw_values(11_023, 0);
    let values = words
        .iter()
        .copied()
        .map(pcm16_to_float)
        .collect::<Vec<_>>();
    let converted = sample_loader::padded_reference(&values, 2, 48_000, 96_000);
    let source = reference(&vec![converted; 5]);
    let mut files = Vec::new();
    for index in 0..5 {
        let path = temp.path().join(format!("{index}.wav"));
        write_words(&path, 48_000, 2, &words, false);
        files.push(File::open(path).unwrap());
    }
    let stream = AlignedStemStream::prepare(&source, &files, 96_000, &|| false).unwrap();
    struct CancelWriter<'a>(&'a Cell<bool>, usize);
    impl Write for CancelWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.1 += bytes.len();
            self.0.set(true);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let cancelled = Cell::new(false);
    let mut writer = CancelWriter(&cancelled, 0);
    assert!(stream.convert(0, &mut writer, &|| cancelled.get()).is_err());
    assert_eq!(writer.1, 4096 * 4);
    struct FailedWriter;
    impl Write for FailedWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from_raw_os_error(32))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(
        stream
            .convert(0, &mut FailedWriter, &|| false)
            .unwrap_err()
            .raw_os_error(),
        Some(32)
    );
}

#[cfg(windows)]
#[test]
fn legacy_preflight_reads_named_wav_while_actual_directory_guards_remain_held() {
    let temp = tempfile::tempdir().unwrap();
    let frames = 11_023;
    for (index, name) in STEM_FILE_NAMES.iter().enumerate() {
        write_words(
            &temp.path().join(format!("{name}.wav")),
            48_000,
            2,
            &raw_values(frames, index),
            false,
        );
    }
    let retained =
        crate::audio_engine::project_assets::stem_readers::seal_stem_artifacts(temp.path())
            .unwrap();
    assert!(retained.len() > STEM_FILE_NAMES.len());
    assert!(retained[0].metadata().unwrap().is_dir());
    #[cfg(windows)]
    assert_eq!(
        wav::Pcm16Wav::inspect(&retained[0], &|| false)
            .unwrap_err()
            .raw_os_error(),
        Some(1)
    );
    let peak = legacy_preparation_peak(frames * 2, 2, 96_000, temp.path(), &|| false).unwrap();
    let base = admitted_stem_pcm_bytes(frames * 2, 2).unwrap();
    assert!(peak > base);
    assert!(retained[0].metadata().unwrap().is_dir());
    assert!(legacy_preparation_peak(frames * 2, 2, 96_000, temp.path(), &|| true).is_err());
    drop(retained);
}
