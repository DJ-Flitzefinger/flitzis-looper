//! Small independent legacy-array comparisons. Long resource evidence is separate.
use super::streamed::AlignedStemStream;
use super::wav::Pcm16Wav;
use super::*;
use crate::audio_engine::complete_context::CompleteSourceReader;
use std::sync::Arc;

fn write_words(path: &Path, rate: u32, channels: usize, values: &[i16], rf64: bool) {
    let data = (values.len() as u64) * 2;
    let mut file = File::create(path).unwrap();
    if rf64 {
        file.write_all(b"RF64").unwrap();
        file.write_all(&u32::MAX.to_le_bytes()).unwrap();
        file.write_all(b"WAVEds64").unwrap();
        file.write_all(&28_u32.to_le_bytes()).unwrap();
        file.write_all(&(data + 72).to_le_bytes()).unwrap();
        file.write_all(&data.to_le_bytes()).unwrap();
        file.write_all(&(values.len() as u64 / channels as u64).to_le_bytes())
            .unwrap();
        file.write_all(&0_u32.to_le_bytes()).unwrap();
    } else {
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(data as u32 + 36).to_le_bytes()).unwrap();
        file.write_all(b"WAVE").unwrap();
    }
    file.write_all(b"fmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&(channels as u16).to_le_bytes()).unwrap();
    file.write_all(&rate.to_le_bytes()).unwrap();
    file.write_all(&(rate * channels as u32 * 2).to_le_bytes())
        .unwrap();
    file.write_all(&(channels as u16 * 2).to_le_bytes())
        .unwrap();
    file.write_all(&16_u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&(if rf64 { u32::MAX } else { data as u32 }).to_le_bytes())
        .unwrap();
    for word in values {
        file.write_all(&word.to_le_bytes()).unwrap();
    }
    file.sync_all().unwrap();
}

fn fixture(
    rate: u32,
    channels: usize,
    offset: isize,
    silent: bool,
) -> (
    tempfile::TempDir,
    CompleteSourceReader,
    Vec<File>,
    Vec<SampleBuffer>,
) {
    let temp = tempfile::tempdir().unwrap();
    let frames = rate as usize * 12 + 93;
    let mut components = Vec::new();
    for index in 0..5 {
        let mut words = vec![0_i16; frames * channels];
        if !silent {
            // The much later, stronger peak determines the first 5% crossing.
            // Distinct signs/channel layouts exercise abs-onset versus signed mono.
            for (origin, amplitude) in [
                (31, 40_i16),
                (rate as usize * 9, 6000),
                (rate as usize * 9 + 29, 5000),
            ] {
                let destination = origin.checked_add_signed(offset).unwrap();
                for channel in 0..channels {
                    let sign = if channel % 2 == 0 { 1 } else { -1 };
                    words[destination * channels + channel] = amplitude * sign * (index as i16 + 1);
                }
            }
            // Include both PCM16 endpoints and tail data in one derivative,
            // outside the four-second window, without changing component onset.
            if index == 4 {
                words[(frames - 2) * channels] = i16::MIN;
                words[(frames - 1) * channels] = i16::MAX;
            }
        }
        let path = temp.path().join(format!("{index}.wav"));
        write_words(&path, rate, channels, &words, index % 2 == 1);
        components.push(SampleBuffer {
            residency: None,
            channels,
            samples: words
                .into_iter()
                .map(pcm16_to_float)
                .collect::<Vec<_>>()
                .into_boxed_slice()
                .into(),
        });
    }
    let mix = mixed_component_stem_samples(&components, channels, frames);
    let mut reference = SampleBuffer {
        residency: None,
        channels,
        samples: Arc::from(mix),
    };
    shift_sample_buffer(&mut reference, frames, offset);
    let files = (0..5)
        .map(|index| File::open(temp.path().join(format!("{index}.wav"))).unwrap())
        .collect();
    (
        temp,
        CompleteSourceReader::new(reference, None).unwrap(),
        files,
        components,
    )
}

fn onset_fixture(
    rate: u32,
    reference: Vec<f32>,
    candidate: &[i16],
) -> (
    tempfile::TempDir,
    CompleteSourceReader,
    Vec<File>,
    Vec<SampleBuffer>,
) {
    assert_eq!(reference.len(), candidate.len());
    let temp = tempfile::tempdir().unwrap();
    let mut buffers = Vec::new();
    let mut files = Vec::new();
    for index in 0..5 {
        let silent = vec![0_i16; candidate.len()];
        let words = if index == 0 || index == 4 {
            candidate
        } else {
            &silent
        };
        let path = temp.path().join(format!("onset-{index}.wav"));
        write_words(&path, rate, 1, words, index % 2 == 1);
        files.push(File::open(path).unwrap());
        buffers.push(SampleBuffer {
            residency: None,
            channels: 1,
            samples: words
                .iter()
                .copied()
                .map(pcm16_to_float)
                .collect::<Vec<_>>()
                .into_boxed_slice()
                .into(),
        });
    }
    let reader = CompleteSourceReader::new(
        SampleBuffer {
            residency: None,
            channels: 1,
            samples: reference.into_boxed_slice().into(),
        },
        None,
    )
    .unwrap();
    (temp, reader, files, buffers)
}

fn assert_productive_onset_matches_legacy(
    reader: &CompleteSourceReader,
    files: &[File],
    buffers: &[SampleBuffer],
    rate: u32,
    expected_offset: isize,
) {
    let frames = reader.reference.frame_count();
    let legacy_offset = detected_stem_alignment_offset(&reader.reference, buffers, rate, frames);
    assert_eq!(legacy_offset, expected_offset);
    let mut aligned = buffers.to_vec();
    for buffer in &mut aligned {
        shift_sample_buffer(buffer, frames, legacy_offset);
    }
    for partition in [1, 3, 17, 4096] {
        let actual =
            AlignedStemStream::prepare_partitioned(reader, files, rate, &|| false, partition)
                .unwrap();
        assert_eq!(actual.offset_frames, legacy_offset, "partition {partition}");
        for (index, legacy) in aligned.iter().enumerate() {
            let expected: Vec<u8> = legacy
                .samples
                .iter()
                .flat_map(|value| value.to_bits().to_le_bytes())
                .collect();
            let mut output = Vec::new();
            let (digest, extent) = actual.convert(index, &mut output, &|| false).unwrap();
            assert_eq!(output, expected, "component {index}, partition {partition}");
            assert_eq!(digest, format!("{:x}", Sha256::digest(&expected)));
            assert_eq!(extent, expected.len() as u64);
        }
    }
}

#[test]
fn bounded_alignment_matches_frozen_full_arrays_for_global_peak_partitions_layouts_and_signed_tail()
{
    for (rate, channels, offset) in [(128, 1, -7), (1000, 2, 37), (128, 3, 0)] {
        let (_temp, reader, files, originals) = fixture(rate, channels, offset, false);
        let frames = reader.reference.frame_count();
        let expected = detected_stem_alignment_offset(&reader.reference, &originals, rate, frames);
        assert_eq!(expected, offset);
        let mut aligned = originals.clone();
        for buffer in &mut aligned {
            shift_sample_buffer(buffer, frames, expected);
        }
        for partition in [1, 7, 31, 4096 / channels] {
            let actual =
                AlignedStemStream::prepare_partitioned(&reader, &files, rate, &|| false, partition)
                    .unwrap();
            assert_eq!(actual.offset_frames, expected, "partition {partition}");
            for index in 0..5 {
                let expected: Vec<u8> = aligned[index]
                    .samples
                    .iter()
                    .flat_map(|n| n.to_bits().to_le_bytes())
                    .collect();
                let mut bytes = Vec::new();
                let (digest, extent) = actual.convert(index, &mut bytes, &|| false).unwrap();
                assert_eq!(bytes, expected, "component {index}, partition {partition}");
                assert_eq!(digest, format!("{:x}", Sha256::digest(&expected)));
                assert_eq!(extent, expected.len() as u64);
            }
        }
    }
}

#[test]
fn all_silent_and_equal_correlation_ties_keep_legacy_ascending_lag_behavior() {
    let (_temp, reader, files, _) = fixture(128, 2, 0, true);
    let stream =
        AlignedStemStream::prepare_partitioned(&reader, &files, 128, &|| false, 3).unwrap();
    assert_eq!(stream.offset_frames, 0);
    let best = best_lag_in_range(&[1.0; 32], &[1.0; 32], -4, 4);
    assert_eq!(best.lag, -4); // Strict > preserves the first equal-scoring lag.

    let mut reference = vec![0.0_f32; 640];
    reference[128] = 1.0;
    let mut candidate = vec![0_i16; 640];
    candidate[123] = i16::MAX;
    candidate[133] = i16::MAX;
    let (_temp, reader, files, buffers) = onset_fixture(128, reference, &candidate);
    let reference_onset = onset_signal(&reader.reference.samples, 1, 640);
    let candidate_mix = mixed_component_stem_samples(&buffers, 1, 640);
    let candidate_onset = onset_signal(&candidate_mix, 1, 640);
    let analysis = alignment_analysis_range(&reference_onset, 128, 640);
    let reference_window = &reference_onset[analysis.start..analysis.end];
    let candidate_window = &candidate_onset[analysis.start..analysis.end];
    let negative = correlation_for_lag(reference_window, candidate_window, -5);
    let positive = correlation_for_lag(reference_window, candidate_window, 5);
    assert_eq!(negative.to_bits(), positive.to_bits());
    assert!(negative >= STEM_ALIGNMENT_MIN_SCORE);
    assert_eq!(
        correlation_for_lag(reference_window, candidate_window, 0),
        0.0
    );
    // Both maxima score identically. The actual streaming coarse/refine path
    // must retain the first ascending lag, including its aligned output bits.
    assert_productive_onset_matches_legacy(&reader, &files, &buffers, 128, -5);
}

#[test]
fn threshold_adjacent_early_attack_uses_the_full_global_peak() {
    let signal = [0.049_999_997_f32, 0.05, 0.0, 0.0, 1.0];
    let range = alignment_analysis_range(&signal, 2, signal.len());
    assert_eq!(range.start, 0); // threshold crossing at frame 1, one pre-onset frame.
    let mut below = signal;
    below[1] = 0.049_999_997;
    assert_eq!(alignment_analysis_range(&below, 2, below.len()).start, 3);

    let threshold = STEM_ALIGNMENT_ONSET_THRESHOLD_RATIO;
    for early in [
        f32::from_bits(threshold.to_bits() - 1),
        threshold,
        f32::from_bits(threshold.to_bits() + 1),
    ] {
        let mut reference = vec![0.0_f32; 1800];
        reference[31] = early;
        reference[1200] = 1.0;
        let mut candidate = vec![0_i16; 1800];
        candidate[38] = 1638; // Early attack has an independent +7-frame lag.
        candidate[1191] = i16::MAX; // Later global peak has a -9-frame lag.
        let (_temp, reader, files, buffers) = onset_fixture(128, reference, &candidate);
        let onsets = onset_signal(&reader.reference.samples, 1, 1800);
        let legacy_window = alignment_analysis_range(&onsets, 128, 1800);
        let expected_offset = if early >= threshold {
            assert_eq!(legacy_window.start, 0);
            7
        } else {
            assert_eq!(legacy_window.start, 1136);
            -9
        };
        // Adjacent f32 values select different real streaming analysis windows
        // and different signed offsets, then all five complete converted outputs.
        assert_productive_onset_matches_legacy(&reader, &files, &buffers, 128, expected_offset);
    }
}

#[test]
fn riff_rf64_actual_payload_endpoints_geometry_and_declared_eof_are_checked() {
    let temp = tempfile::tempdir().unwrap();
    for rf64 in [false, true] {
        let path = temp.path().join(format!("{rf64}.wav"));
        write_words(&path, 48000, 2, &[i16::MIN, i16::MAX, -1, 1], rf64);
        let mut file = File::open(&path).unwrap();
        let layout = Pcm16Wav::inspect(&file, &|| false).unwrap();
        layout.validate_geometry(48000, 2, 2).unwrap();
        let mut values = [0.0; 4];
        layout
            .read_frames(&file, 0, &mut values, &|| false)
            .unwrap();
        assert_eq!(values, [-1.0, 1.0, -1.0 / 32767.0, 1.0 / 32767.0]);
        verify_pcm16_wav_geometry_cancellable(&mut file, 48000, 2, 2, &|| false).unwrap();
        drop(file);
        let intact = fs::read(&path).unwrap();
        for corrupt in ["truncate", "extra", "header"] {
            let mut bytes = intact.clone();
            match corrupt {
                "truncate" => {
                    bytes.pop();
                }
                "extra" => bytes.push(0),
                "header" => bytes[4] ^= 1,
                _ => unreachable!(),
            }
            fs::write(&path, &bytes).unwrap();
            assert!(
                Pcm16Wav::inspect(&File::open(&path).unwrap(), &|| false).is_err(),
                "{rf64} {corrupt}"
            );
        }
    }
}

#[test]
fn actual_extensible_multichannel_pcm_guid_reads_the_payload_and_rejects_float_guid() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("extensible.wav");
    write_words(&path, 44100, 3, &[i16::MIN, 0, i16::MAX, -1, 1, 100], false);
    let mut bytes = fs::read(&path).unwrap();
    let mut extension = Vec::new();
    extension.extend(22_u16.to_le_bytes());
    extension.extend(16_u16.to_le_bytes());
    extension.extend(7_u32.to_le_bytes());
    extension.extend([
        1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
    ]);
    bytes.splice(36..36, extension);
    let size = bytes.len() as u32 - 8;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    bytes[16..20].copy_from_slice(&40_u32.to_le_bytes());
    bytes[20..22].copy_from_slice(&0xfffe_u16.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    let file = File::open(&path).unwrap();
    let layout = Pcm16Wav::inspect(&file, &|| false).unwrap();
    layout.validate_geometry(44100, 3, 2).unwrap();
    let mut values = [0.0; 6];
    layout
        .read_frames(&file, 0, &mut values, &|| false)
        .unwrap();
    assert_eq!(
        values,
        [
            -1.0,
            0.0,
            1.0,
            -1.0 / 32767.0,
            1.0 / 32767.0,
            100.0 / 32767.0
        ]
    );
    drop(file);
    bytes[44] = 3; // IEEE-float subtype is not a PCM16 stem artifact.
    fs::write(&path, &bytes).unwrap();
    assert!(Pcm16Wav::inspect(&File::open(&path).unwrap(), &|| false).is_err());
}

#[test]
fn conversion_cancel_after_a_returning_write_and_precise_writer_error_are_visible() {
    use std::cell::Cell;
    struct Writer<'a> {
        cancelled: &'a Cell<bool>,
        bytes: usize,
        fail: bool,
    }
    impl Write for Writer<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes += bytes.len();
            if self.fail {
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "isolated injected block write ENOSPC",
                ));
            }
            self.cancelled.set(true);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let (_temp, reader, files, _) = fixture(1000, 2, 37, false);
    let stream = AlignedStemStream::prepare(&reader, &files, 1000, &|| false).unwrap();
    for fail in [false, true] {
        let cancelled = Cell::new(false);
        let mut output = Writer {
            cancelled: &cancelled,
            bytes: 0,
            fail,
        };
        let error = stream
            .convert(0, &mut output, &|| cancelled.get())
            .unwrap_err();
        assert!(output.bytes > 0 && output.bytes < reader.reference.frame_count() * 2 * 4);
        if fail {
            assert_eq!(error.kind(), io::ErrorKind::StorageFull);
        } else {
            assert!(error.to_string().contains("cancelled"));
        }
    }
}
