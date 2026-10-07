//! Source fixtures and interpolation expectations independent of the production reader.

use super::*;
use crate::messages::{STEM_MASK_BASS, STEM_MASK_DRUMS, STEM_MASK_MELODY, STEM_MASK_VOCALS};
use std::sync::Arc;

const CHANNELS: usize = 2;
const SOURCE_FRAMES: usize = 971;
const PARTITIONS: [&[usize]; 3] = [&[512], &[1], &[1, 127, 384, 96, 257, 512, 31]];

fn immutable_source(tag: usize) -> SampleBuffer {
    // Distinct channels and nonperiodic frame values expose addressing and stride mistakes.
    let samples: Vec<f32> = (0..SOURCE_FRAMES * CHANNELS)
        .map(|index| {
            let frame = index / CHANNELS;
            let channel = index % CHANNELS;
            let value =
                (frame * frame * 17 + frame * (29 + tag * 11) + channel * 191 + tag * 73) % 1009;
            (value as f32 - 504.0) / 4096.0
        })
        .collect();
    SampleBuffer {
        residency: None,
        channels: CHANNELS,
        samples: Arc::from(samples.into_boxed_slice()),
    }
}

#[derive(Clone, Copy)]
struct SourceOracle {
    start: usize,
    end: usize,
    origin: usize,
    initial_mode: ExplicitSeekMode,
}

impl SourceOracle {
    fn normal(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            origin: start,
            initial_mode: ExplicitSeekMode::Normal,
        }
    }

    // Deliberately do not use source_frame_for_playback or any production position helpers.
    fn address(self, offset: usize) -> usize {
        let position = self.origin + offset;
        match self.initial_mode {
            ExplicitSeekMode::Normal => {
                self.start + (position - self.start) % (self.end - self.start)
            }
            ExplicitSeekMode::BeforeLoop if position < self.start => position,
            ExplicitSeekMode::BeforeLoop => {
                self.start + (position - self.start) % (self.end - self.start)
            }
            ExplicitSeekMode::AfterLoop if position < SOURCE_FRAMES => position,
            ExplicitSeekMode::AfterLoop => {
                self.start + (position - SOURCE_FRAMES) % (self.end - self.start)
            }
        }
    }

    fn sample(self, source: &SampleBuffer, distance: f64, channel: usize) -> f32 {
        let offset = distance.floor() as usize;
        let fraction = (distance - offset as f64) as f32;
        let lower = source.samples[self.address(offset) * CHANNELS + channel];
        let upper = source.samples[self.address(offset + 1) * CHANNELS + channel];
        lower + (upper - lower) * fraction
    }

    fn samples(self, source: &SampleBuffer, distances: &[f64]) -> Vec<f32> {
        distances
            .iter()
            .flat_map(|distance| {
                (0..CHANNELS).map(move |channel| self.sample(source, *distance, channel))
            })
            .collect()
    }

    fn assert_cursor(self, mixer: &RtMixer, distance: f64) {
        let voice = mixer.voices.iter().find(|voice| voice.active).unwrap();
        let actual = voice.source_playback.position();
        let whole = distance.floor() as usize;
        assert_eq!(actual.frame, self.address(whole));
        assert!(
            (actual.fraction - (distance - whole as f64)).abs() < 1e-8,
            "fraction mismatch: actual={}, distance={distance}",
            actual.fraction,
        );
        let expected_mode = match self.initial_mode {
            ExplicitSeekMode::BeforeLoop if self.origin + whole < self.start => {
                ExplicitSeekMode::BeforeLoop
            }
            ExplicitSeekMode::AfterLoop if self.origin + whole < SOURCE_FRAMES => {
                ExplicitSeekMode::AfterLoop
            }
            _ => ExplicitSeekMode::Normal,
        };
        assert_eq!(actual.seek_mode, expected_mode);
    }
}

fn set_loop(mixer: &mut RtMixer, sample_rate: f32, start: usize, end: usize) {
    mixer.set_pad_loop_region(
        0,
        start as f64 / f64::from(sample_rate),
        Some(end as f64 / f64::from(sample_rate)),
    );
}

fn restart(mixer: &mut RtMixer, ratio: f64, key_lock: bool) {
    mixer.stop_sample(0);
    mixer.set_bpm_lock(false);
    mixer.set_speed(ratio);
    mixer.set_key_lock(key_lock);
    assert!(mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0));
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
}

#[derive(Default)]
struct CapturedAudio {
    feed: Vec<f32>,
    output: Vec<f32>,
}

fn render_capture(
    mixer: &mut RtMixer,
    output_frame: &mut u64,
    total_frames: usize,
    partitions: &[usize],
) -> CapturedAudio {
    let mut result = CapturedAudio::default();
    let mut remaining = total_frames;
    let mut index = 0;
    let mut peaks = [0.0; NUM_SAMPLES];
    while remaining > 0 {
        let frames = remaining.min(partitions[index % partitions.len()]);
        assert!(frames > 0 && frames <= 512);
        let mut output = vec![0.0; frames * CHANNELS];
        mixer.render_at_output_frame(*output_frame, &mut output, &mut peaks);
        let voice = mixer.voices.iter().find(|voice| voice.active).unwrap();
        let feed = voice.stretch.varispeed_buffers();
        for frame in 0..frames {
            for channel in feed.iter().take(CHANNELS) {
                result.feed.push(channel[frame]);
            }
        }
        result.output.extend(output);
        *output_frame += frames as u64;
        remaining -= frames;
        index += 1;
    }
    result
}

fn assert_audio_close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() < 2e-6,
            "sample {index}: actual={actual}, expected={expected}",
        );
    }
}

#[test]
fn immutable_stereo_source_feed_and_cursor_are_partition_invariant() {
    let source = immutable_source(0);
    let oracle = SourceOracle::normal(127, 831);
    let frames = 2053;
    let master_bpm = 137.3_f64;
    let pad_bpm = 112.7_f64;
    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut mixer = RtMixer::new(CHANNELS, sample_rate);
        mixer.load_sample(0, source.clone());
        set_loop(&mut mixer, sample_rate, oracle.start, oracle.end);
        for (rate_index, ratio) in [0.5, 0.73, 1.25, 1.999, master_bpm / pad_bpm]
            .into_iter()
            .enumerate()
        {
            let distances: Vec<f64> = (0..frames).map(|frame| frame as f64 * ratio).collect();
            let expected = oracle.samples(&source, &distances);
            let mut reference_feed = None;
            for key_lock in [false, true] {
                for partitions in PARTITIONS {
                    restart(&mut mixer, ratio, key_lock);
                    if rate_index == 4 {
                        mixer.set_bpm_lock(true);
                        mixer.set_master_bpm(master_bpm);
                        mixer.set_pad_bpm(0, Some(pad_bpm));
                    }
                    let actual = render_capture(&mut mixer, &mut 0, frames, partitions);
                    assert_audio_close(&actual.feed, &expected);
                    if let Some(reference) = &reference_feed {
                        assert_eq!(actual.feed, *reference);
                    } else {
                        reference_feed = Some(actual.feed.clone());
                    }
                    if !key_lock {
                        assert_audio_close(&actual.output, &expected);
                    }
                    oracle.assert_cursor(&mixer, frames as f64 * ratio);
                }
            }
        }
    }
}

#[test]
fn explicit_intro_and_tail_seek_taps_and_cursors_are_partition_invariant() {
    let source = immutable_source(2);
    let ratio = 0.73_f64;
    let frames = 1907;
    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut mixer = RtMixer::new(CHANNELS, sample_rate);
        mixer.load_sample(0, source.clone());
        set_loop(&mut mixer, sample_rate, 127, 831);
        for (origin, initial_mode) in [
            (19, ExplicitSeekMode::BeforeLoop),
            (503, ExplicitSeekMode::Normal),
            (899, ExplicitSeekMode::AfterLoop),
            (SOURCE_FRAMES - 1, ExplicitSeekMode::AfterLoop),
        ] {
            let oracle = SourceOracle {
                start: 127,
                end: 831,
                origin,
                initial_mode,
            };
            let distances: Vec<f64> = (0..frames).map(|frame| frame as f64 * ratio).collect();
            let expected = oracle.samples(&source, &distances);
            let mut reference_feed = None;
            for partitions in PARTITIONS {
                restart(&mut mixer, ratio, false);
                assert!(mixer.seek_sample_at_output_frame(
                    0,
                    origin as f64 / f64::from(sample_rate),
                    0,
                ));
                let actual = render_capture(&mut mixer, &mut 0, frames, partitions);
                assert_audio_close(&actual.feed, &expected);
                assert_audio_close(&actual.output, &expected);
                if let Some(reference) = &reference_feed {
                    assert_eq!(actual.feed, *reference);
                } else {
                    reference_feed = Some(actual.feed);
                }
                oracle.assert_cursor(&mixer, frames as f64 * ratio);
            }
        }
    }
}

fn prepared_sources(reference: &SampleBuffer, sample_rate: f32) -> PreparedStemSet {
    PreparedStemSet {
        complete_set_identity: std::sync::Arc::new([0; 32]),
        accepted_timing: None,
        reference_samples: reference.samples.clone(),
        publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
        source_version_hash: 42,
        sample_rate_hz: sample_rate as u32,
        channels: CHANNELS,
        frame_count: SOURCE_FRAMES,
        available_mask: (1 << STEM_BUFFER_COUNT) - 1,
        stems: std::array::from_fn(|index| immutable_source(index + 3)),
    }
}

fn component_sum(stems: &PreparedStemSet, mask: u8) -> SampleBuffer {
    let samples: Vec<f32> = (0..SOURCE_FRAMES * CHANNELS)
        .map(|index| {
            stems.stems[..4]
                .iter()
                .enumerate()
                .filter(|(stem, _)| mask & (1 << stem) != 0)
                .map(|(_, source)| source.samples[index])
                .sum()
        })
        .collect();
    SampleBuffer {
        residency: None,
        channels: CHANNELS,
        samples: Arc::from(samples.into_boxed_slice()),
    }
}

#[test]
fn prepared_stem_masks_interpolate_the_same_fractional_source_addresses() {
    let source = immutable_source(0);
    let ratio = 1.25_f64;
    let oracle = SourceOracle::normal(113, 787);
    let frames = 1493;
    let distances: Vec<f64> = (0..frames).map(|frame| frame as f64 * ratio).collect();
    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut mixer = RtMixer::new(CHANNELS, sample_rate);
        mixer.load_sample(0, source.clone());
        set_loop(&mut mixer, sample_rate, oracle.start, oracle.end);
        let stems = prepared_sources(&source, sample_rate);
        assert!(mixer.publish_prepared_stems(0, stems.clone()));
        for mask in [
            0,
            STEM_MASK_VOCALS,
            STEM_MASK_DRUMS | STEM_MASK_MELODY | STEM_MASK_BASS,
            STEM_COMPONENT_MASK,
        ] {
            let expected = oracle.samples(&component_sum(&stems, mask), &distances);
            let mut reference_feed = None;
            for partitions in PARTITIONS {
                restart(&mut mixer, ratio, false);
                assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
                assert!(mixer.set_stem_enabled_mask(0, mask, 42));
                // A fresh trigger clears source-selection transitions; the fixture is steady.
                assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
                let actual = render_capture(&mut mixer, &mut 0, frames, partitions);
                assert_audio_close(&actual.feed, &expected);
                assert_audio_close(&actual.output, &expected);
                if let Some(reference) = &reference_feed {
                    assert_eq!(actual.feed, *reference);
                } else {
                    reference_feed = Some(actual.feed);
                }
                oracle.assert_cursor(&mixer, frames as f64 * ratio);
            }
        }
    }
}

#[test]
fn accepted_same_source_stems_retain_fractional_trajectory_after_timing_intent_changes() {
    use crate::audio_engine::prepared_source::PreparedSourcePermit;
    use std::sync::atomic::{AtomicU64, Ordering};

    let sample_rate = 48_000.0;
    let source = immutable_source(0);
    let oracle = SourceOracle::normal(113, 787);
    let ratio = 0.73_f64;
    let prefix = 333;
    let frames = 1493;
    let mut reference_feed = None;
    for partitions in PARTITIONS {
        let mut mixer = RtMixer::new(CHANNELS, sample_rate);
        mixer.load_sample(0, source.clone());
        set_loop(&mut mixer, sample_rate, oracle.start, oracle.end);
        let epoch = Arc::new(AtomicU64::new(1));
        let mut stems = prepared_sources(&source, sample_rate);
        stems.publication = PreparedSourcePermit::for_epoch(epoch.clone(), 1);
        let selected = component_sum(&stems, STEM_MASK_VOCALS | STEM_MASK_DRUMS);
        assert!(mixer.publish_prepared_stems(0, stems));
        mixer.set_speed(ratio);
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        assert!(mixer.set_stem_enabled_mask(0, STEM_MASK_VOCALS | STEM_MASK_DRUMS, 42));
        assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
        let mut output_frame = 0;
        render_capture(&mut mixer, &mut output_frame, prefix, partitions);
        oracle.assert_cursor(&mixer, prefix as f64 * ratio);

        // Publication guards reject unfinished work; an accepted source-frame stem set
        // still follows the same source clock after later BPM/origin intent updates.
        epoch.store(2, Ordering::Release);
        mixer.set_pad_bpm(0, Some(123.45));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: -0.217,
            },
        );
        assert!(
            !mixer.prepared_stems[0]
                .as_ref()
                .unwrap()
                .publication
                .current()
        );
        oracle.assert_cursor(&mixer, prefix as f64 * ratio);
        let distances: Vec<f64> = (prefix..prefix + frames)
            .map(|frame| frame as f64 * ratio)
            .collect();
        let expected = oracle.samples(&selected, &distances);
        let actual = render_capture(&mut mixer, &mut output_frame, frames, partitions);
        assert_audio_close(&actual.feed, &expected);
        assert_audio_close(&actual.output, &expected);
        oracle.assert_cursor(&mixer, (prefix + frames) as f64 * ratio);
        if let Some(reference) = &reference_feed {
            assert_eq!(actual.feed, *reference);
        } else {
            reference_feed = Some(actual.feed);
        }
    }
}

#[test]
fn fractional_stem_crossfade_uses_common_addresses_across_partitions() {
    let sample_rate = 48_000.0;
    let source = immutable_source(0);
    let stems = prepared_sources(&source, sample_rate);
    let selected = component_sum(&stems, STEM_MASK_VOCALS | STEM_MASK_DRUMS);
    let oracle = SourceOracle::normal(113, 787);
    let ratio = 0.73_f64;
    let prefix = 333;
    let frames = 911;
    let mut mixer = RtMixer::new(CHANNELS, sample_rate);
    mixer.load_sample(0, source.clone());
    assert!(mixer.publish_prepared_stems(0, stems));
    set_loop(&mut mixer, sample_rate, oracle.start, oracle.end);
    let expected: Vec<f32> = (0..frames)
        .flat_map(|frame| {
            let distance = (prefix + frame) as f64 * ratio;
            let to_gain = (frame as f64 * ratio / 128.0).min(1.0) as f32;
            let source = &source;
            let selected = &selected;
            (0..CHANNELS).map(move |channel| {
                oracle.sample(source, distance, channel) * (1.0 - to_gain)
                    + oracle.sample(selected, distance, channel) * to_gain
            })
        })
        .collect();
    let mut reference_feed = None;
    for partitions in PARTITIONS {
        restart(&mut mixer, ratio, false);
        let mut output_frame = 0;
        render_capture(&mut mixer, &mut output_frame, prefix, partitions);
        assert!(mixer.set_stem_enabled_mask(0, STEM_MASK_VOCALS | STEM_MASK_DRUMS, 42));
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
        let actual = render_capture(&mut mixer, &mut output_frame, frames, partitions);
        assert_audio_close(&actual.feed, &expected);
        assert_audio_close(&actual.output, &expected);
        if let Some(reference) = &reference_feed {
            assert_eq!(actual.feed, *reference);
        } else {
            reference_feed = Some(actual.feed);
        }
        oracle.assert_cursor(&mixer, (prefix + frames) as f64 * ratio);
        assert!(!mixer.stem_transitions[0].is_active());
    }
}

#[test]
fn pause_resume_preserves_fraction_and_excludes_paused_output_time() {
    let sample_rate = 48_000.0;
    let source = immutable_source(1);
    let oracle = SourceOracle::normal(127, 831);
    let ratio = 0.73_f64;
    let prefix = 17;
    let frames = 1779;
    let mut mixer = RtMixer::new(CHANNELS, sample_rate);
    mixer.load_sample(0, source.clone());
    set_loop(&mut mixer, sample_rate, oracle.start, oracle.end);
    let distances: Vec<f64> = (prefix..prefix + frames)
        .map(|frame| frame as f64 * ratio)
        .collect();
    let expected = oracle.samples(&source, &distances);
    let mut reference_feed = None;
    for partitions in PARTITIONS {
        restart(&mut mixer, ratio, false);
        let mut output_frame = 0;
        render_capture(&mut mixer, &mut output_frame, prefix, partitions);
        oracle.assert_cursor(&mixer, prefix as f64 * ratio);
        mixer.pause_sample_at_output_frame(0, output_frame);
        let paused = render_capture(&mut mixer, &mut output_frame, 1301, partitions);
        assert!(paused.output.iter().all(|sample| *sample == 0.0));
        oracle.assert_cursor(&mixer, prefix as f64 * ratio);
        mixer.resume_sample_at_output_frame(0, output_frame);
        let actual = render_capture(&mut mixer, &mut output_frame, frames, partitions);
        assert_audio_close(&actual.feed, &expected);
        assert_audio_close(&actual.output, &expected);
        if let Some(reference) = &reference_feed {
            assert_eq!(actual.feed, *reference);
        } else {
            reference_feed = Some(actual.feed);
        }
        oracle.assert_cursor(&mixer, (prefix + frames) as f64 * ratio);
    }
}

// Rate changes step immediately, then every 512 actively rendered output frames. Computing
// the reference per sample makes callback boundaries irrelevant to the expected trajectory.
fn smoothed_distances(initial: f64, changes: &[(usize, f64)], frames: usize) -> Vec<f64> {
    let mut current = initial;
    let mut target = initial;
    let mut distance = 0.0;
    let mut next_step = 0;
    let mut result = Vec::with_capacity(frames + 1);
    for frame in 0..frames {
        if let Some((_, changed_target)) = changes.iter().find(|(at, _)| *at == frame) {
            target = *changed_target;
            next_step = frame;
        }
        if frame == next_step {
            current += (target - current).clamp(-0.05, 0.05);
            next_step = frame + 512;
        }
        result.push(distance);
        distance += current;
    }
    result.push(distance);
    result
}

#[test]
fn changed_targets_follow_active_output_time_across_callback_partitions() {
    let sample_rate = 48_000.0;
    let source = immutable_source(5);
    let oracle = SourceOracle::normal(127, 831);
    let initial = 0.73_f64;
    let changes = [(137, 1.25), (1430, 0.5), (2201, 1.999)];
    let frames = 4097;
    let distances = smoothed_distances(initial, &changes, frames);
    let expected = oracle.samples(&source, &distances[..frames]);
    let mut mixer = RtMixer::new(CHANNELS, sample_rate);
    mixer.load_sample(0, source.clone());
    set_loop(&mut mixer, sample_rate, oracle.start, oracle.end);
    let mut reference_output = None;
    for partitions in PARTITIONS {
        restart(&mut mixer, initial, false);
        let mut output_frame = 0;
        let mut actual_output = Vec::new();
        let mut index = 0;
        let mut peaks = [0.0; NUM_SAMPLES];
        while output_frame < frames as u64 {
            if let Some((_, target)) = changes.iter().find(|(at, _)| *at == output_frame as usize) {
                mixer.set_speed(*target);
            }
            let next_change = changes
                .iter()
                .map(|(at, _)| *at)
                .find(|at| *at > output_frame as usize)
                .unwrap_or(frames);
            let chunk_frames = partitions[index % partitions.len()]
                .min(next_change - output_frame as usize)
                .min(frames - output_frame as usize);
            let mut output = vec![0.0; chunk_frames * CHANNELS];
            mixer.render_at_output_frame(output_frame, &mut output, &mut peaks);
            // Internal smoothing splits may overwrite the feed accessor; dry output retains
            // every sample and is the canonical native feed when Key Lock is bypassed.
            actual_output.extend(output);
            output_frame += chunk_frames as u64;
            index += 1;
        }
        assert_audio_close(&actual_output, &expected);
        if let Some(reference) = &reference_output {
            assert_eq!(actual_output, *reference);
        } else {
            reference_output = Some(actual_output);
        }
        oracle.assert_cursor(&mixer, distances[frames]);
    }
}

#[test]
fn loop_edits_preserve_in_range_fraction_and_normalize_out_of_range_position() {
    let sample_rate = 48_000.0;
    let source = immutable_source(1);
    let ratio = 0.73_f64;
    let mut mixer = RtMixer::new(CHANNELS, sample_rate);
    mixer.load_sample(0, source.clone());
    let mut reference_feed = None;
    for partitions in PARTITIONS {
        set_loop(&mut mixer, sample_rate, 127, 831);
        restart(&mut mixer, ratio, false);
        let mut output_frame = 0;
        render_capture(&mut mixer, &mut output_frame, 17, partitions);
        // 127 + 17*ratio = 139.41 remains inside this new region.
        set_loop(&mut mixer, sample_rate, 130, 600);
        let oracle = SourceOracle {
            start: 130,
            end: 600,
            origin: 139,
            initial_mode: ExplicitSeekMode::Normal,
        };
        let fraction = 17.0 * ratio - 12.0;
        let distances: Vec<f64> = (0..93)
            .map(|frame| fraction + frame as f64 * ratio)
            .collect();
        let expected = oracle.samples(&source, &distances);
        let actual = render_capture(&mut mixer, &mut output_frame, 93, partitions);
        assert_audio_close(&actual.feed, &expected);
        oracle.assert_cursor(&mixer, fraction + 93.0 * ratio);
        if let Some(reference) = &reference_feed {
            assert_eq!(actual.feed, *reference);
        } else {
            reference_feed = Some(actual.feed);
        }
        set_loop(&mut mixer, sample_rate, 400, 401);
        let actual = render_capture(&mut mixer, &mut output_frame, 37, partitions);
        let single_frame = SourceOracle::normal(400, 401);
        let expected = single_frame.samples(&source, &[0.0; 37]);
        assert_audio_close(&actual.feed, &expected);
        single_frame.assert_cursor(&mixer, 37.0 * ratio);
    }
}

#[test]
fn active_source_beat_query_retains_fraction_and_matches_loop_edit_first_read() {
    let sample_rate = 48_000.0;
    let source = immutable_source(2);
    let ratio = 0.73_f64;
    let bpm = 119.75_f64;
    let mut mixer = RtMixer::new(CHANNELS, sample_rate);
    mixer.load_sample(0, source.clone());
    set_loop(&mut mixer, sample_rate, 127, 831);
    restart(&mut mixer, ratio, false);
    mixer.set_pad_bpm(0, Some(bpm));
    mixer.set_pad_timing_metadata(
        0,
        PadTimingMetadata {
            phase_anchor_s: -3.0 / f64::from(sample_rate),
        },
    );
    let mut output_frame = 0;
    render_capture(&mut mixer, &mut output_frame, 17, &[1, 7, 9]);
    let source_frame = 127.0 + 17.0 * ratio;
    let expected_beat = (source_frame + 3.0) * bpm / (60.0 * f64::from(sample_rate));
    assert!((mixer.active_pad_beat_position(0).unwrap() - expected_beat).abs() < 1e-12,);
    set_loop(&mut mixer, sample_rate, 130, 600);
    assert!((mixer.active_pad_beat_position(0).unwrap() - expected_beat).abs() < 1e-12,);
    // Before rendering a loop edit, the query must already use the renderer's normalization.
    set_loop(&mut mixer, sample_rate, 400, 600);
    let expected_beat = 403.0 * bpm / (60.0 * f64::from(sample_rate));
    assert!((mixer.active_pad_beat_position(0).unwrap() - expected_beat).abs() < 1e-12,);
    let first_read = render_capture(&mut mixer, &mut output_frame, 1, &[1]);
    assert_eq!(
        first_read.feed,
        source.samples[400 * CHANNELS..401 * CHANNELS]
    );
}

fn render_dry_output(
    mixer: &mut RtMixer,
    output_frame: &mut u64,
    total_frames: usize,
    partitions: &[usize],
) -> Vec<f32> {
    let mut result = Vec::with_capacity(total_frames * CHANNELS);
    let mut remaining = total_frames;
    let mut index = 0;
    let mut peaks = [0.0; NUM_SAMPLES];
    while remaining > 0 {
        let frames = remaining.min(partitions[index % partitions.len()]);
        let mut output = vec![0.0; frames * CHANNELS];
        mixer.render_at_output_frame(*output_frame, &mut output, &mut peaks);
        result.extend(output);
        *output_frame += frames as u64;
        remaining -= frames;
        index += 1;
    }
    result
}

#[test]
fn pause_during_smoothing_preserves_remaining_active_frame_interval() {
    let sample_rate = 48_000.0;
    let source = immutable_source(5);
    let oracle = SourceOracle::normal(127, 831);
    let initial = 0.73_f64;
    let change_at = 137;
    let pause_at = 431;
    let frames = 2049;
    let distances = smoothed_distances(initial, &[(change_at, 1.25)], frames);
    let expected = oracle.samples(&source, &distances[..frames]);
    let mut mixer = RtMixer::new(CHANNELS, sample_rate);
    mixer.load_sample(0, source);
    set_loop(&mut mixer, sample_rate, oracle.start, oracle.end);
    let mut reference_output = None;
    for partitions in PARTITIONS {
        restart(&mut mixer, initial, false);
        let mut output_frame = 0;
        let mut actual = render_dry_output(&mut mixer, &mut output_frame, change_at, partitions);
        mixer.set_speed(1.25);
        actual.extend(render_dry_output(
            &mut mixer,
            &mut output_frame,
            pause_at - change_at,
            partitions,
        ));
        oracle.assert_cursor(&mixer, distances[pause_at]);
        mixer.pause_sample_at_output_frame(0, output_frame);
        let paused = render_dry_output(&mut mixer, &mut output_frame, 1301, partitions);
        assert!(paused.iter().all(|sample| *sample == 0.0));
        oracle.assert_cursor(&mixer, distances[pause_at]);
        mixer.resume_sample_at_output_frame(0, output_frame);
        actual.extend(render_dry_output(
            &mut mixer,
            &mut output_frame,
            frames - pause_at,
            partitions,
        ));
        assert_audio_close(&actual, &expected);
        if let Some(reference) = &reference_output {
            assert_eq!(actual, *reference);
        } else {
            reference_output = Some(actual);
        }
        oracle.assert_cursor(&mixer, distances[frames]);
    }
}

#[test]
fn steady_key_lock_wet_output_from_immutable_source_is_partition_invariant() {
    let source = immutable_source(4);
    let oracle = SourceOracle::normal(127, 831);
    let ratio = 0.73_f64;
    let frames = 8193;
    let distances: Vec<f64> = (0..frames).map(|frame| frame as f64 * ratio).collect();
    let expected_feed = oracle.samples(&source, &distances);
    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut reference: Option<CapturedAudio> = None;
        for partitions in PARTITIONS {
            // Every render starts with equivalent unused warm native state. Reusing a dirty
            // handle would make worker reserve availability an unrelated test variable.
            let mut mixer = RtMixer::new(CHANNELS, sample_rate);
            mixer.load_sample(0, source.clone());
            set_loop(&mut mixer, sample_rate, oracle.start, oracle.end);
            restart(&mut mixer, ratio, true);
            // The retained voice continues, but an unpublished current source generation fences
            // new native preparation. This isolates continuous adapter partitioning; productive
            // preparation's ready/late deadline policy has separate real-worker mixer tests.
            mixer.set_input_runtime_ownership(Arc::new(
                crate::audio_engine::input_runtime_binding::InputRuntimeOwnership::tracked(),
            ));
            let actual = render_capture(&mut mixer, &mut 0, frames, partitions);
            assert_audio_close(&actual.feed, &expected_feed);
            assert!(actual.output.iter().all(|sample| sample.is_finite()));
            assert!(actual.output.iter().any(|sample| sample.abs() > 0.001));
            oracle.assert_cursor(&mixer, frames as f64 * ratio);
            if let Some(reference) = &reference {
                assert_eq!(actual.feed, reference.feed);
                assert_eq!(actual.output, reference.output);
            } else {
                reference = Some(actual);
            }
        }
    }
}
