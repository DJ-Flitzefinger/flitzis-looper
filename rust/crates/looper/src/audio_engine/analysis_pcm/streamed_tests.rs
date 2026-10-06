use super::super::{PcmIdentity, resample_mono_cancellable};
use super::*;
use crate::messages::SampleBuffer;
use std::cell::Cell;
use std::io::{Cursor, Error, ErrorKind};

fn loaded(samples: Vec<f32>, channels: usize, rate_hz: u32) -> LoadedPcmSnapshot {
    LoadedPcmSnapshot::new(
        SampleBuffer {
            channels,
            samples: samples.into(),
        },
        rate_hz,
        PcmIdentity {
            pad_id: 0,
            request_id: 1,
            source_id: "stream-fixture".into(),
            source_generation: 1,
        },
        usize::MAX,
    )
    .unwrap()
}

fn bytes(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|value| value.to_bits()).collect()
}

#[test]
fn r01_remainder_flushes_past_a_zero_output_call_without_changing_origin_or_tail() {
    // The private R01 length 57650783 has this remainder modulo lcm(1024,1280).
    let mut mono = vec![0.0_f32; 4703];
    mono[0] = 0.75;
    mono[4702] = -0.9375;
    let standard =
        resample_mono_cancellable(mono.clone(), 96_000, KEY_RATE_HZ, usize::MAX, &|| false)
            .unwrap();
    let streamed = key_input_from_f32_le(
        &mut Cursor::new(bytes(&mono)),
        mono.len(),
        96_000,
        usize::MAX,
        &|| false,
    )
    .unwrap();
    let expected = explicit_zero_padding_reference(&mono, 96_000, KEY_RATE_HZ);
    assert_eq!(
        streamed.samples.len(),
        (4703_usize * 44_100).div_ceil(96_000)
    );
    assert_eq!(bits(&streamed.samples), bits(&expected));
    assert_eq!(bits(&standard), bits(&expected));
    assert!(streamed.samples[0].abs() > 0.1);
    assert!(streamed.samples[streamed.samples.len() - 1].abs() > 0.1);
    assert_eq!(
        streamed.peak_bytes,
        key_peak_bytes(mono.len(), 96_000).unwrap()
    );
    assert_eq!(
        tail_call_budget(2455 - 2352, 96_000, KEY_RATE_HZ).unwrap(),
        2
    );
    let mut converter = Fft::<f32>::new(96_000, 44_100, 1024, 1, 1, FixedSync::Input).unwrap();
    let source = InterleavedOwned::new_from(vec![0.25_f32; 1024], 1, 1024).unwrap();
    let mut output = InterleavedOwned::new_from(vec![0.0_f32; 588], 1, 588).unwrap();
    let mut actual = Vec::new();
    for partial_len in [None, None, None, None, Some(607), Some(0), Some(0)] {
        actual.push(
            converter
                .process_into_buffer(
                    &source,
                    &mut output,
                    Some(&Indexing {
                        input_offset: 0,
                        output_offset: 0,
                        partial_len,
                        active_channels_mask: None,
                    }),
                )
                .unwrap(),
        );
    }
    assert_eq!(
        actual,
        [
            (1024, 0),
            (1024, 588),
            (1024, 588),
            (1024, 588),
            (1024, 588),
            (1024, 0),
            (1024, 588)
        ]
    );
}

#[test]
fn every_96khz_input_remainder_preserves_complete_reference_pcm() {
    for frames in 1..=5120 {
        let mut mono: Vec<_> = (0..frames)
            .map(|index| ((index % 97) as f32 - 48.0) / 64.0)
            .collect();
        mono[0] = 0.75;
        mono[frames - 1] = -0.9375;
        let reference = explicit_zero_padding_reference(&mono, 96_000, KEY_RATE_HZ);
        let standard =
            resample_mono_cancellable(mono.clone(), 96_000, KEY_RATE_HZ, usize::MAX, &|| false)
                .unwrap();
        let streamed = key_input_from_f32_le(
            &mut Cursor::new(bytes(&mono)),
            frames,
            96_000,
            usize::MAX,
            &|| false,
        )
        .unwrap();
        let expected_len = (frames * 44_100).div_ceil(96_000);
        assert_eq!(streamed.samples.len(), expected_len);
        assert_eq!(bits(&streamed.samples), bits(&reference), "frames={frames}");
        assert_eq!(
            bits(&standard),
            bits(&reference),
            "standard frames={frames}"
        );
        assert_eq!(streamed.peak_bytes, key_peak_bytes(frames, 96_000).unwrap());
    }
}

/// Full-buffer test reference with an explicit finite zero suffix. It runs only
/// full input blocks, eliminating partial/flush control-flow shared with the
/// standard and streamed paths, including repeated zero-output padding cases.
fn explicit_zero_padding_reference(mono: &[f32], rate_hz: u32, target_rate: u32) -> Vec<f32> {
    let mut converter = Fft::<f32>::new(
        rate_hz as usize,
        target_rate as usize,
        1024,
        1,
        1,
        FixedSync::Input,
    )
    .unwrap();
    let expected = (mono.len() * target_rate as usize).div_ceil(rate_hz as usize);
    let delay = converter.output_delay();
    let input_chunk = converter.input_frames_max();
    let output_unit = converter.output_frames_max();
    let input_unit = output_unit * rate_hz as usize / target_rate as usize;
    let units_needed = (delay + expected).div_ceil(output_unit);
    let input_frames = (units_needed * input_unit).div_ceil(input_chunk) * input_chunk;
    let output_frames = (units_needed + 1) * output_unit;
    let mut padded = mono.to_vec();
    padded.resize(input_frames, 0.0);
    let input = InterleavedOwned::new_from(padded, 1, input_frames).unwrap();
    let mut output =
        InterleavedOwned::new_from(vec![0.0; output_frames], 1, output_frames).unwrap();
    let mut indexing = Indexing {
        input_offset: 0,
        output_offset: 0,
        partial_len: None,
        active_channels_mask: None,
    };
    for _ in 0..input_frames / input_chunk {
        let (consumed, produced) = converter
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .unwrap();
        assert_eq!(consumed, input_chunk);
        indexing.input_offset += consumed;
        indexing.output_offset += produced;
    }
    assert!(indexing.output_offset >= delay + expected);
    output.take_data()[delay..delay + expected].to_vec()
}

#[test]
fn coprime_rates_flush_multiple_zero_output_calls_with_finite_budget() {
    for rate in [8001, 44_099, 44_101, 383_999] {
        for frames in [1, 17, 1025] {
            let mut mono = vec![0.0; frames];
            mono[0] = 0.75;
            mono[frames - 1] = -0.9375;
            let reference = explicit_zero_padding_reference(&mono, rate, KEY_RATE_HZ);
            let standard =
                resample_mono_cancellable(mono.clone(), rate, KEY_RATE_HZ, usize::MAX, &|| false)
                    .unwrap();
            let streamed = key_input_from_f32_le(
                &mut Cursor::new(bytes(&mono)),
                frames,
                rate,
                key_peak_bytes(frames, rate).unwrap(),
                &|| false,
            )
            .unwrap();
            assert_eq!(
                bits(&streamed.samples),
                bits(&reference),
                "rate={rate}, frames={frames}"
            );
            assert_eq!(streamed.peak_bytes, key_peak_bytes(frames, rate).unwrap());
            assert_eq!(
                bits(&standard),
                bits(&reference),
                "standard rate={rate}, frames={frames}"
            );
        }
    }
}

#[test]
fn standard_converter_preserves_explicit_padding_at_other_target_rates() {
    for (source, target) in [(96_000, 48_000), (44_100, 96_000), (44_101, 48_000)] {
        for frames in [1, 1024, 4703] {
            let mut mono = vec![0.0; frames];
            mono[0] = 0.75;
            mono[frames - 1] = -0.9375;
            let reference = explicit_zero_padding_reference(&mono, source, target);
            let standard =
                resample_mono_cancellable(mono, source, target, usize::MAX, &|| false).unwrap();
            assert_eq!(
                bits(&standard),
                bits(&reference),
                "{source}->{target}, frames={frames}"
            );
        }
    }
}

#[test]
fn standard_zero_output_tail_preserves_cancellation_and_exact_pcm_limit() {
    let converter = Fft::<f32>::new(96_000, 44_100, 1024, 1, 1, FixedSync::Input).unwrap();
    let input = vec![0.25; 4703];
    let expected = (4703_usize * 44_100).div_ceil(96_000);
    let cap = pcm_bytes(input.capacity()).unwrap()
        + pcm_bytes(converter.output_delay() + expected + converter.output_frames_max()).unwrap();
    assert!(matches!(
        resample_mono_cancellable(input.clone(), 96_000, 44_100, cap - 1, &|| false),
        Err(PcmError::Limit(_))
    ));
    assert_eq!(
        resample_mono_cancellable(input.clone(), 96_000, 44_100, cap, &|| false)
            .unwrap()
            .len(),
        expected
    );
    // Entry/setup, four full blocks and one partial block precede the two
    // padding calls. Check cancellation before the zero-output call and after it.
    for cancel_at in [8, 9] {
        let calls = Cell::new(0);
        assert!(matches!(
            resample_mono_cancellable(input.clone(), 96_000, 44_100, cap, &|| {
                calls.set(calls.get() + 1);
                calls.get() >= cancel_at
            }),
            Err(PcmError::Cancelled)
        ));
        assert_eq!(calls.get(), cancel_at);
    }
}

#[test]
fn streamed_channel_mean_matches_existing_full_buffer_bit_for_bit() {
    for channels in [1, 2, 3, 32] {
        let frames = CHUNK_FRAMES * 2 + 19;
        let mut samples: Vec<_> = (0..frames * channels)
            .map(|index| ((index % 127) as f32 - 64.0) / 63.0)
            .collect();
        samples[..channels].fill(-0.0);
        samples[channels..channels * 2].fill(f32::MAX);
        samples[(frames - 1) * channels..].fill(-0.75);
        let snapshot = loaded(samples, channels, 96_000);
        let playback = snapshot.sample.samples.clone();
        let expected = snapshot.prepare_mono(usize::MAX, &|| false).unwrap();
        let mut output = Vec::new();
        let plan = snapshot.staging_plan().unwrap();
        assert_eq!(
            snapshot
                .stream_f32_le(
                    &mut output,
                    plan.export_peak_bytes,
                    plan.export_bytes,
                    &|| false
                )
                .unwrap(),
            frames * 4
        );
        assert_eq!(output, bytes(expected.samples()));
        drop(snapshot);
        assert_eq!(playback.len(), frames * channels);
        assert_eq!(playback[playback.len() - 1], -0.75);
    }
}

#[test]
fn streamed_resampling_preserves_complete_existing_output_bits_at_supported_rates() {
    for rate in [8000, 22_050, 44_100, 48_000, 96_000, 384_000] {
        for frames in [1, 17, 1023, 1024, 1025, 4097, rate as usize + 17] {
            for silence in [false, true] {
                let mut mono: Vec<_> = (0..frames)
                    .map(|index| {
                        if silence {
                            0.0
                        } else {
                            ((index % 97) as f32 - 48.0) / 64.0
                        }
                    })
                    .collect();
                if !silence {
                    mono[0] = -0.0;
                    mono[frames / 2] = 0.875;
                    mono[frames - 1] = -0.9375;
                }
                let expected =
                    resample_mono_cancellable(mono.clone(), rate, KEY_RATE_HZ, usize::MAX, &|| {
                        false
                    })
                    .unwrap();
                let mut input = Cursor::new(bytes(&mono));
                let reservation = key_peak_bytes(frames, rate).unwrap();
                let actual =
                    key_input_from_f32_le(&mut input, frames, rate, reservation, &|| false)
                        .unwrap();
                assert_eq!(
                    bits(&actual.samples),
                    bits(&expected),
                    "rate={rate}, frames={frames}, silence={silence}"
                );
                assert_eq!(actual.samples.len(), output_frames(frames, rate).unwrap());
                assert_eq!(actual.peak_bytes, reservation);
                assert_eq!(actual.samples.capacity(), actual.samples.len());
                assert_eq!(input.position(), (frames * 4) as u64);
            }
        }
    }
}

#[test]
fn streamed_first_and_last_impulses_preserve_converter_origin_and_tail() {
    for rate in [8000, 22_050, 44_100, 48_000, 96_000, 384_000] {
        let frames = rate as usize + 17;
        for marker in [0, 137, frames - 1] {
            let mut mono = vec![0.0; frames];
            mono[marker] = 0.75;
            let expected =
                resample_mono_cancellable(mono.clone(), rate, KEY_RATE_HZ, usize::MAX, &|| false)
                    .unwrap();
            let actual = key_input_from_f32_le(
                &mut Cursor::new(bytes(&mono)),
                frames,
                rate,
                usize::MAX,
                &|| false,
            )
            .unwrap();
            assert_eq!(
                bits(&actual.samples),
                bits(&expected),
                "rate={rate}, marker={marker}"
            );
            assert!(actual.samples.iter().any(|value| *value != 0.0));
        }
    }
}

#[test]
fn arithmetic_dimensions_match_the_pinned_rubato_configuration() {
    for rate in [
        8000, 8001, 11_025, 22_050, 44_099, 44_100, 44_101, 48_000, 88_200, 96_000, 192_000,
        383_999, 384_000,
    ] {
        let converter = Fft::<f32>::new(
            rate as usize,
            KEY_RATE_HZ as usize,
            RESAMPLE_CHUNK_FRAMES,
            1,
            1,
            FixedSync::Input,
        )
        .unwrap();
        assert_eq!(
            converter_dimensions(rate),
            (converter.input_frames_max(), converter.output_frames_max()),
            "rate={rate}"
        );
    }
}

#[test]
fn separate_stages_admit_complete_long_96khz_stereo_without_source_key_overlap() {
    let frames = 96_000 * 660;
    let source_bytes = frames * 2 * 4;
    let cap = 512 * 1024 * 1024;
    let plan = staging_plan(source_bytes, frames, 96_000).unwrap();
    assert!(source_bytes + plan.export_bytes > cap);
    assert!(source_bytes + output_frames(frames, 96_000).unwrap() * 4 > cap);
    assert!(plan.export_peak_bytes <= cap);
    assert!(plan.key_peak_bytes <= cap);
    assert!(plan.export_bytes <= cap);
    assert_eq!(plan.export_peak_bytes, source_bytes + IO_CHUNK_BYTES);
    assert_eq!(plan.export_bytes, frames * 4);
}

#[test]
fn export_and_key_limits_reject_before_writing_or_reading() {
    let snapshot = loaded(vec![0.5; 10], 2, 48_000);
    let plan = snapshot.staging_plan().unwrap();
    for (working, export) in [
        (plan.export_peak_bytes - 1, plan.export_bytes),
        (plan.export_peak_bytes, plan.export_bytes - 1),
    ] {
        let mut output = Vec::new();
        assert!(matches!(
            snapshot.stream_f32_le(&mut output, working, export, &|| false),
            Err(PcmError::Limit(_))
        ));
        assert!(output.is_empty());
    }
    for rate in [8000, 44_100, 96_000] {
        let mut input = Cursor::new(bytes(&[0.0; 5]));
        assert!(matches!(
            key_input_from_f32_le(
                &mut input,
                5,
                rate,
                key_peak_bytes(5, rate).unwrap() - 1,
                &|| false
            ),
            Err(PcmError::Limit(_))
        ));
        assert_eq!(input.position(), 0);
    }
}

#[test]
fn reservation_math_checks_overflows_and_malformed_metadata() {
    for (retained, frames, rate) in [
        (usize::MAX, 1, 48_000),
        (0, usize::MAX, 384_000),
        (0, usize::MAX, 8000),
    ] {
        assert!(matches!(
            staging_plan(retained, frames, rate),
            Err(PcmError::Limit(_))
        ));
    }
    for (frames, rate) in [(0, 44_100), (1, 0), (1, 7999), (1, 384_001)] {
        let mut input = Cursor::new(Vec::<u8>::new());
        assert!(matches!(
            key_input_from_f32_le(&mut input, frames, rate, usize::MAX, &|| false),
            Err(PcmError::InvalidInput(_))
        ));
        assert_eq!(input.position(), 0);
    }
}

#[test]
fn source_export_rejects_nonfinite_samples_without_success() {
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let snapshot = loaded(vec![0.0, invalid], 2, 48_000);
        let mut output = Vec::new();
        assert!(matches!(
            snapshot.stream_f32_le(&mut output, usize::MAX, usize::MAX, &|| false),
            Err(PcmError::InvalidInput(_))
        ));
        assert!(output.is_empty());
    }
}

#[test]
fn staged_key_rejects_short_trailing_and_nonfinite_files() {
    for rate in [44_100, 48_000] {
        for short in [vec![], vec![0; 3], vec![0; 7]] {
            assert!(
                matches!(key_input_from_f32_le(&mut Cursor::new(short), 2, rate, usize::MAX, &|| false), Err(PcmError::Io(error)) if error.kind() == ErrorKind::UnexpectedEof)
            );
        }
        assert!(matches!(
            key_input_from_f32_le(&mut Cursor::new(vec![0; 9]), 2, rate, usize::MAX, &|| false),
            Err(PcmError::InvalidInput(_))
        ));
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(matches!(
                key_input_from_f32_le(
                    &mut Cursor::new(bytes(&[0.0, invalid])),
                    2,
                    rate,
                    usize::MAX,
                    &|| false
                ),
                Err(PcmError::InvalidInput(_))
            ));
        }
    }
}

struct FailingIo;

impl Read for FailingIo {
    fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        Err(Error::other("read fixture failure"))
    }
}

impl Write for FailingIo {
    fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
        Err(Error::other("write fixture failure"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn io_errors_propagate_without_claiming_complete_pcm() {
    let snapshot = loaded(vec![0.0; 4], 1, 44_100);
    assert!(matches!(
        snapshot.stream_f32_le(&mut FailingIo, usize::MAX, usize::MAX, &|| false),
        Err(PcmError::Io(_))
    ));
    for rate in [44_100, 48_000] {
        assert!(matches!(
            key_input_from_f32_le(&mut FailingIo, 4, rate, usize::MAX, &|| false),
            Err(PcmError::Io(_))
        ));
    }
}

#[test]
fn streamed_export_cancels_before_start_and_between_chunks() {
    let snapshot = loaded(vec![0.25; CHUNK_FRAMES * 3], 1, 44_100);
    let mut output = Vec::new();
    assert!(matches!(
        snapshot.stream_f32_le(&mut output, usize::MAX, usize::MAX, &|| true),
        Err(PcmError::Cancelled)
    ));
    assert!(output.is_empty());
    let calls = Cell::new(0);
    assert!(matches!(
        snapshot.stream_f32_le(&mut output, usize::MAX, usize::MAX, &|| {
            calls.set(calls.get() + 1);
            calls.get() >= 3
        }),
        Err(PcmError::Cancelled)
    ));
    assert_eq!(output.len(), IO_CHUNK_BYTES);
}

#[test]
fn streamed_key_cancels_before_read_and_during_bounded_read_conversion_or_tail() {
    for rate in [8000, 44_100, 96_000, 384_000] {
        let frames = CHUNK_FRAMES * 3 + 17;
        let mut input = Cursor::new(bytes(&vec![0.5; frames]));
        assert!(matches!(
            key_input_from_f32_le(&mut input, frames, rate, usize::MAX, &|| true),
            Err(PcmError::Cancelled)
        ));
        assert_eq!(input.position(), 0);
        let calls = Cell::new(0);
        assert!(matches!(
            key_input_from_f32_le(&mut input, frames, rate, usize::MAX, &|| {
                calls.set(calls.get() + 1);
                calls.get() >= 5
            }),
            Err(PcmError::Cancelled)
        ));
        assert!(input.position() < (frames * 4) as u64);

        // Visit each cooperative boundary for a short source including final
        // partial input, zero padding, delay discard and final success check.
        let checks = Cell::new(0);
        key_input_from_f32_le(
            &mut Cursor::new(bytes(&[0.5; 17])),
            17,
            rate,
            usize::MAX,
            &|| {
                checks.set(checks.get() + 1);
                false
            },
        )
        .unwrap();
        for stop_at in 1..=checks.get() {
            let calls = Cell::new(0);
            assert!(
                matches!(
                    key_input_from_f32_le(
                        &mut Cursor::new(bytes(&[0.5; 17])),
                        17,
                        rate,
                        usize::MAX,
                        &|| {
                            calls.set(calls.get() + 1);
                            calls.get() == stop_at
                        }
                    ),
                    Err(PcmError::Cancelled)
                ),
                "rate={rate}, check={stop_at}"
            );
        }
    }
}
