//! Complete fresh A/C beginnings against the existing independent stereo C ABI.
//! No DUT phase, source reader, history/delay getter or prepared owner defines PCM.

use super::{END, RATE, START};
use crate::audio_engine::dsp::{DspNodeSlot, DspParameterId, DspParameterSlot, PerPadDspChain};
use crate::audio_engine::key_lock_physical_onset_tests::{PROFILES, RawNative};

pub(super) const PREFIX_FRAMES: usize = 16_384;
const HISTORY_FRAMES: usize = 4_096;
const BLOCK_FRAMES: usize = 512;
const ADAPTER_LEAD: usize = BLOCK_FRAMES - 1;
// Only the target's f32 (source + C) - source observation needs rounding slack.
// The standalone source prefix below must match exactly, including startup zeroes.
const MIX_SUBTRACTION_TOLERANCE: f32 = 2.0e-6;
const NEGATIVE_CONTROL_MIN_DIFFERENCE: f32 = 0.001;

// Independently decode the PCM16 words declared by the parent fixture's wav().
// Original and component readers intentionally retain their established distinct
// normalization contracts. No live/resident/DUT-selected sample view supplies this.
fn component_word(frame: usize, channel: usize, layer: usize) -> i16 {
    let phase = frame as f64 * (0.031 + layer as f64 * 0.017) + channel as f64 * 0.41;
    (phase.sin() * (1700.0 + layer as f64 * 500.0)).round() as i16
}

fn source_sample(frame: usize, channel: usize, mask: Option<u8>) -> f32 {
    if let Some(mask) = mask {
        (0..4)
            .filter(|layer| mask & (1 << layer) != 0)
            .map(|layer| f32::from(component_word(frame, channel, layer)) / 32_767.0)
            .sum()
    } else {
        let word: i16 = (0..4)
            .map(|layer| component_word(frame, channel, layer))
            .sum();
        f32::from(word) / 32_768.0
    }
}

fn fresh_filter() -> PerPadDspChain {
    let mut filter = PerPadDspChain::new(0, RATE as f32, 2);
    for (slot, db) in [
        (DspParameterSlot::Slot0, 3.0_f32),
        (DspParameterSlot::Slot1, -4.0),
        (DspParameterSlot::Slot2, 1.5),
    ] {
        let normalized = if db <= 0.0 {
            0.5 * 10.0_f32.powf(db / 20.0)
        } else {
            0.5 + 0.5 * (db / 6.0)
        };
        assert!(filter.set_parameter(
            DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
            normalized
        ));
    }
    filter.reset();
    filter
}

fn reference(semitones: i32, mask: Option<u8>) -> Vec<f32> {
    assert_eq!((RATE, START, END), (48_000, 128, 2_048));
    assert!(matches!(semitones, 7 | 8));
    // Independent fixture rate = speed 1 with BPM lock OFF. Its Manual timing
    // has no accepted musical loop domain: positive source phase is 128+n%1920.
    // Lock ON/OFF therefore use the same independently declared pitch here.
    let pitch = 2.0_f64.powf(f64::from(semitones) / 12.0);
    let input = std::array::from_fn(|channel| {
        (0..HISTORY_FRAMES + PREFIX_FRAMES)
            .map(|frame| {
                if frame < HISTORY_FRAMES {
                    0.0
                } else {
                    let physical = START + (frame - HISTORY_FRAMES) % (END - START);
                    source_sample(physical, channel, mask)
                }
            })
            .collect::<Vec<_>>()
    });
    let (raw, _unused_nominal_delay) = RawNative::capture_at_rate(&input, RATE, pitch, PROFILES[0]);
    // One independently declared adapter lead; no delay/peak fitting, extra
    // silence prefix or slicing of actual emitted PCM. H silent input frames
    // leave raw coordinates H-511..H queued for the first real emission at E.
    let first_raw = HISTORY_FRAMES - ADAPTER_LEAD;
    let gain = 10.0_f32.powf(-7.0 / 20.0);
    let mut filter = fresh_filter();
    let mut expected = Vec::with_capacity(PREFIX_FRAMES * 2);
    for frame in first_raw..first_raw + PREFIX_FRAMES {
        filter.begin_frame();
        for (channel, samples) in raw.iter().enumerate() {
            expected.push(filter.process_sample(channel, samples[frame] * gain));
        }
    }
    assert!(expected.iter().all(|value| value.is_finite()));
    assert!(expected.iter().any(|value| value.abs() > 0.0001));
    expected
}

fn maximum_difference(left: &[f32], right: &[f32]) -> f32 {
    assert_eq!(left.len(), right.len());
    assert!(left.iter().chain(right).all(|value| value.is_finite()));
    left.iter()
        .zip(right)
        .map(|(left, right)| (left - right).abs())
        .fold(0.0, f32::max)
}

fn assert_negative_controls(actual: &[f32], expected: &[f32], semitones: i32, selected: bool) {
    let mask = selected.then_some(15);
    let wrong_pitch = reference(if semitones == 7 { 8 } else { 7 }, mask);
    let wrong_mask = reference(semitones, Some(5));
    for (name, wrong) in [("wrong pitch", wrong_pitch), ("wrong mask 5", wrong_mask)] {
        assert!(
            maximum_difference(expected, &wrong) > NEGATIVE_CONTROL_MIN_DIFFERENCE,
            "independent {name} control cannot distinguish this fixture"
        );
        assert!(
            maximum_difference(actual, &wrong) > NEGATIVE_CONTROL_MIN_DIFFERENCE,
            "actual complete prefix is insensitive to {name}"
        );
    }
    // ALL15 and FullMix contain the same fixture layers (with their established
    // PCM16 normalization difference). PCM is not an independent proof against
    // that near-equivalent fallback; the parent's own selection/reader/lease/
    // descriptor/current-source-wet assertions remain mandatory and unchanged.
}

pub(super) fn assert_source_prefix(pcm: &[f32]) {
    assert_eq!(pcm.len(), PREFIX_FRAMES * 2);
    let expected = reference(7, Some(15));
    assert!(pcm.iter().all(|value| value.is_finite()));
    assert_eq!(
        pcm.iter()
            .zip(&expected)
            .position(|(actual, expected)| actual != expected),
        None,
        "complete source +7 birth differs from direct Native/fresh EQ: maximum={}",
        maximum_difference(pcm, &expected)
    );
    assert!(pcm.iter().any(|value| value.abs() > 0.0001));
    assert_negative_controls(pcm, &expected, 7, true);
}

pub(super) fn assert_target_prefix(mixed: &[f32], source_only: &[f32], selected: bool) {
    assert_eq!(mixed.len(), PREFIX_FRAMES * 2);
    assert_eq!(source_only.len(), mixed.len());
    assert!(
        mixed
            .iter()
            .chain(source_only)
            .all(|value| value.is_finite())
    );
    let target = mixed
        .iter()
        .zip(source_only)
        .map(|(mixed, source)| mixed - source)
        .collect::<Vec<_>>();
    let expected = reference(8, selected.then_some(15));
    let maximum = maximum_difference(&target, &expected);
    assert!(
        maximum < MIX_SUBTRACTION_TOLERANCE,
        "complete copied +8 birth differs from direct Native/fresh EQ: selected={selected} maximum={maximum}"
    );
    // Preserve actual target audibility; neither ready flags nor an all-zero
    // DUT/reference pair can satisfy the complete-prefix musical-content check.
    assert!(target.iter().any(|value| value.abs() > 0.0001));
    assert_negative_controls(&target, &expected, 8, selected);
}
