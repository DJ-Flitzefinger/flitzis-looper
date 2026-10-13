//! Exact two-generation reading uses an independent PCM/interpolation oracle.

use super::source_playback::SourcePlayback;
use super::source_reader::{
    ExplicitSeekMode, FrameRange, STEM_TRANSITION_RAMP_FRAMES, SourceReadPlan, StemReadSets,
    StemRenderSelection, StemTransition, full_stem_available_mask,
};
use crate::messages::{PreparedStemSet, STEM_COMPONENT_MASK, SampleBuffer, StemMixMode};
use std::sync::Arc;

const FRAMES: usize = 13;
const CHANNELS: usize = 2;
const REGION: FrameRange = FrameRange { start: 3, end: 11 };

fn pcm(scale: f32, offset: f32) -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels: CHANNELS,
        samples: Arc::from(
            (0..FRAMES)
                .flat_map(|frame| {
                    (0..CHANNELS)
                        .map(move |channel| (frame as f32 + channel as f32 * 0.25) * scale + offset)
                })
                .collect::<Vec<_>>(),
        ),
    }
}

fn stems(reference: &SampleBuffer, generation: f32) -> PreparedStemSet {
    PreparedStemSet {
        // Equal digest bytes deliberately do not identify equal complete-set owners.
        complete_set_identity: Arc::new([61; 32]),
        accepted_timing: None,
        reference_samples: reference.samples.clone(),
        reference_residency: reference.residency.clone(),
        publication: super::prepared_source::PreparedSourcePermit::unrestricted(),
        source_version_hash: 42,
        sample_rate_hz: 48_000,
        channels: CHANNELS,
        frame_count: FRAMES,
        available_mask: full_stem_available_mask(),
        stems: std::array::from_fn(|index| pcm(generation * (index + 1) as f32, generation * -0.5)),
    }
}

fn plan(current: &PreparedStemSet, outgoing: &PreparedStemSet, mask: u8) -> SourceReadPlan {
    SourceReadPlan {
        channels: CHANNELS,
        sample_frames: FRAMES,
        frame_pos: 9,
        loop_region: REGION,
        loop_period: None,
        seek_mode: ExplicitSeekMode::Normal,
        selection: StemRenderSelection::for_stems(current, mask),
        transition: StemTransition::start(
            StemRenderSelection::for_stems(outgoing, mask),
            STEM_TRANSITION_RAMP_FRAMES,
        ),
    }
}

fn selected_pcm(frame: usize, channel: usize, generation: f32, mask: u8) -> f32 {
    (0..4)
        .filter(|index| mask & (1 << index) != 0)
        .map(|index| {
            (frame as f32 + channel as f32 * 0.25) * generation * (index + 1) as f32
                - generation * 0.5
        })
        .sum()
}

#[test]
fn exact_owner_distinguishes_equal_hash_geometry_mask_and_identity_bytes() {
    let sample = pcm(100.0, 30.0);
    let v1 = stems(&sample, 1.0);
    let v2 = stems(&sample, 7.0);
    let copy_v1 = v1.clone();
    let selection_v1 = StemRenderSelection::for_stems(&v1, 0b0101);
    let selection_v2 = StemRenderSelection::for_stems(&v2, 0b0101);
    assert_ne!(selection_v1, selection_v2);
    assert_eq!(
        selection_v1,
        StemRenderSelection::for_stems(&copy_v1, 0b0101)
    );
    let readers = StemReadSets::new(Some(&v2), Some(&v1));
    assert!(std::ptr::eq(
        readers.for_selection(selection_v1).unwrap(),
        &v1
    ));
    assert!(std::ptr::eq(
        readers.for_selection(selection_v2).unwrap(),
        &v2
    ));
    assert!(
        StemReadSets::new(Some(&v2), None)
            .for_selection(selection_v1)
            .is_none()
    );
    let v1_plan = plan(&v1, &v1, 0b0101);
    let v2_plan = plan(&v2, &v1, 0b0101);
    assert!(!v1_plan.matches_exact(v2_plan));
    assert!(!v1_plan.matches_source_contract(v2_plan));
    let held = v2_plan.required_read_sets(readers);
    assert!(std::ptr::eq(held.current.unwrap(), &v2));
    assert!(std::ptr::eq(held.outgoing.unwrap(), &v1));
    let actual = v2_plan.sample_fractional_with_readers(&sample, readers, 0.375, 64.0, 1);
    let old = selected_pcm(9, 1, 1.0, 0b0101)
        + (selected_pcm(10, 1, 1.0, 0b0101) - selected_pcm(9, 1, 1.0, 0b0101)) * 0.375;
    let new = selected_pcm(9, 1, 7.0, 0b0101)
        + (selected_pcm(10, 1, 7.0, 0b0101) - selected_pcm(9, 1, 7.0, 0b0101)) * 0.375;
    assert!((actual - (old + new) * 0.5).abs() < 1.0e-5);
}

#[test]
fn missing_exact_outgoing_rejects_before_fill_even_with_an_empty_mask() {
    let sample = pcm(100.0, 30.0);
    let v1 = stems(&sample, 1.0);
    let v2 = stems(&sample, 7.0);
    let mut playback = SourcePlayback::new(9, ExplicitSeekMode::Normal, 0.625);
    playback.configure(FRAMES, REGION);
    playback.advance(1);
    for mask in [0, 0b0001, STEM_COMPONENT_MASK] {
        for full_mix in [false, true] {
            let mut reader = plan(&v2, &v1, mask);
            if full_mix {
                reader.selection = StemRenderSelection::full_mix();
            }
            let mut buffers = [vec![-999.0; 8], vec![-999.0; 8]];
            assert!(
                reader
                    .fill_fractional_buffers_checked_with_readers(
                        &sample,
                        StemReadSets::new(Some(&v2), None),
                        &playback,
                        &mut buffers,
                        8,
                    )
                    .is_err()
            );
            assert!(buffers.iter().flatten().all(|value| *value == -999.0));
            reader
                .fill_fractional_buffers_checked_with_readers(
                    &sample,
                    StemReadSets::new(Some(&v2), Some(&v1)),
                    &playback,
                    &mut buffers,
                    8,
                )
                .unwrap();
        }
    }
}

#[test]
fn actual_outgoing_fractional_tap_must_be_resident_before_any_output_changes() {
    let sample = pcm(100.0, 30.0);
    let mut v1 = stems(&sample, 1.0);
    let v2 = stems(&sample, 7.0);
    v1.stems[0].samples = Arc::from(&v1.stems[0].samples[..10 * CHANNELS]);
    let reader = plan(&v2, &v1, 0b0001);
    let mut playback = SourcePlayback::new(9, ExplicitSeekMode::Normal, 0.625);
    playback.configure(FRAMES, REGION);
    playback.advance(1);
    let readers = StemReadSets::new(Some(&v2), Some(&v1));
    assert!(reader.tap_available_with_readers(&sample, readers, 9, 0));
    assert!(!reader.tap_available_with_readers(&sample, readers, 10, 0));
    let mut buffers = [vec![-999.0; 1], vec![-999.0; 1]];
    assert!(
        reader
            .fill_fractional_buffers_checked_with_readers(
                &sample,
                readers,
                &playback,
                &mut buffers,
                1,
            )
            .is_err()
    );
    assert_eq!(buffers, [vec![-999.0], vec![-999.0]]);
}

fn render_partitions(
    sample: &SampleBuffer,
    v1: &PreparedStemSet,
    v2: &PreparedStemSet,
    ratio: f64,
    partitions: &[usize],
    mask: u8,
) -> (Vec<f32>, SourcePlayback, SourceReadPlan) {
    let mut reader = plan(v2, v1, mask);
    let mut playback = SourcePlayback::new(9, ExplicitSeekMode::Normal, ratio);
    playback.configure(FRAMES, REGION);
    let mut output = Vec::new();
    let mut remaining = 240;
    let mut partition = 0;
    while remaining > 0 {
        let frames = remaining.min(partitions[partition % partitions.len()]);
        let mut buffers = [vec![0.0; frames], vec![0.0; frames]];
        let position = playback.position();
        reader.frame_pos = position.frame;
        reader.seek_mode = position.seek_mode;
        reader
            .fill_fractional_buffers_checked_with_readers(
                sample,
                StemReadSets::new(Some(v2), Some(v1)),
                &playback,
                &mut buffers,
                frames,
            )
            .unwrap();
        for frame in 0..frames {
            output.extend([buffers[0][frame], buffers[1][frame]]);
        }
        playback.advance(frames);
        reader.transition.advance_fractional(frames as f64 * ratio);
        remaining -= frames;
        partition += 1;
    }
    (output, playback, reader)
}

#[test]
fn two_exact_sets_preserve_fractional_taps_and_128_source_frame_ramp_across_partitions() {
    let sample = pcm(100.0, 30.0);
    let v1 = stems(&sample, 1.0);
    let v2 = stems(&sample, 7.0);
    for ratio in [0.63, 1.17, 1.91] {
        for mask in [0, 0b0101, STEM_COMPONENT_MASK] {
            let (whole, whole_clock, whole_plan) =
                render_partitions(&sample, &v1, &v2, ratio, &[240], mask);
            let (split, split_clock, split_plan) =
                render_partitions(&sample, &v1, &v2, ratio, &[1, 17, 3, 64, 5, 11], mask);
            assert!(whole_clock.matches_exact(&split_clock));
            assert!(!whole_plan.transition.is_active());
            assert!(!split_plan.transition.is_active());
            let held = split_plan.required_read_sets(StemReadSets::new(Some(&v2), Some(&v1)));
            assert!(held.current.is_some());
            assert!(held.outgoing.is_none());
            for frame in 0..240 {
                let distance = frame as f64 * ratio;
                let left = REGION.start + (6 + distance.floor() as usize) % REGION.len();
                let right = REGION.start + (left - REGION.start + 1) % REGION.len();
                let fraction = distance.fract() as f32;
                let gain = (distance / 128.0).min(1.0) as f32;
                for channel in 0..CHANNELS {
                    let old_left = selected_pcm(left, channel, 1.0, mask);
                    let new_left = selected_pcm(left, channel, 7.0, mask);
                    let old_right = selected_pcm(right, channel, 1.0, mask);
                    let new_right = selected_pcm(right, channel, 7.0, mask);
                    let mixed_left = old_left * (1.0 - gain) + new_left * gain;
                    let mixed_right = old_right * (1.0 - gain) + new_right * gain;
                    let expected = mixed_left + (mixed_right - mixed_left) * fraction;
                    let index = frame * CHANNELS + channel;
                    assert!((whole[index] - expected).abs() < 1.0e-4);
                    assert!((split[index] - expected).abs() < 1.0e-4);
                }
            }
        }
    }
}

fn render_changing_rate(
    sample: &SampleBuffer,
    v1: &PreparedStemSet,
    v2: &PreparedStemSet,
    partitions: &[usize],
) -> Vec<f32> {
    let mut reader = plan(v2, v1, 0b0001);
    let mut playback = SourcePlayback::new(9, ExplicitSeekMode::Normal, 0.63);
    playback.configure(FRAMES, REGION);
    let mut output = Vec::new();
    let mut consumed = 0;
    let mut partition = 0;
    while consumed < 300 {
        if consumed == 37 {
            playback.set_target(1.25);
        } else if consumed == 81 {
            playback.set_target(0.7);
        }
        let boundary = if consumed < 37 {
            37
        } else if consumed < 81 {
            81
        } else {
            300
        };
        let requested = partitions[partition % partitions.len()].min(boundary - consumed);
        let (frames, ratio) = playback.chunk(requested);
        let mut buffers = [vec![0.0; frames], vec![0.0; frames]];
        let position = playback.position();
        reader.frame_pos = position.frame;
        reader.seek_mode = position.seek_mode;
        reader
            .fill_fractional_buffers_checked_with_readers(
                sample,
                StemReadSets::new(Some(v2), Some(v1)),
                &playback,
                &mut buffers,
                frames,
            )
            .unwrap();
        output.extend_from_slice(&buffers[0]);
        playback.advance(frames);
        reader.transition.advance_fractional(frames as f64 * ratio);
        consumed += frames;
        partition += 1;
    }
    assert!(!reader.transition.is_active());
    output
}

#[test]
fn rate_changes_advance_both_exact_readers_by_the_actual_smoothed_source_distance() {
    let sample = pcm(100.0, 30.0);
    let v1 = stems(&sample, 1.0);
    let v2 = stems(&sample, 7.0);
    let whole = render_changing_rate(&sample, &v1, &v2, &[300]);
    let split = render_changing_rate(&sample, &v1, &v2, &[3, 1, 17, 9, 44]);
    for frame in 0..300 {
        // Two control boundaries: the first applies one 0.05 smoothing step, the second
        // reaches 0.7. No output-frame count is substituted for consumed source distance.
        let distance = if frame < 37 {
            frame as f64 * 0.63
        } else if frame < 81 {
            37.0 * 0.63 + (frame - 37) as f64 * (0.63 + 0.05)
        } else {
            37.0 * 0.63 + 44.0 * (0.63 + 0.05) + (frame - 81) as f64 * 0.7
        };
        let left = REGION.start + (6 + distance.floor() as usize) % REGION.len();
        let right = REGION.start + (left - REGION.start + 1) % REGION.len();
        let fraction = distance.fract() as f32;
        let gain = (distance / 128.0).min(1.0) as f32;
        let old_left = selected_pcm(left, 0, 1.0, 0b0001);
        let new_left = selected_pcm(left, 0, 7.0, 0b0001);
        let old_right = selected_pcm(right, 0, 1.0, 0b0001);
        let new_right = selected_pcm(right, 0, 7.0, 0b0001);
        let mixed_left = old_left * (1.0 - gain) + new_left * gain;
        let mixed_right = old_right * (1.0 - gain) + new_right * gain;
        let expected = mixed_left + (mixed_right - mixed_left) * fraction;
        assert!((whole[frame] - expected).abs() < 1.0e-4);
        assert!((split[frame] - expected).abs() < 1.0e-4);
    }
}

#[test]
fn legacy_hash_state_resolves_only_corresponding_current_reader() {
    let sample = pcm(100.0, 30.0);
    let v1 = stems(&sample, 1.0);
    let v2 = stems(&sample, 7.0);
    let legacy = StemRenderSelection::from_state(StemMixMode::AllStems, 42, 0b0001);
    assert!(
        StemReadSets::new(None, Some(&v1))
            .for_selection(legacy)
            .is_none()
    );
    assert!(std::ptr::eq(
        StemReadSets::new(Some(&v2), Some(&v1))
            .for_selection(legacy)
            .unwrap(),
        &v2,
    ));
}
