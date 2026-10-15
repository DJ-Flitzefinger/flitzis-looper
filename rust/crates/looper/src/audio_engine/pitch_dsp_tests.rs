//! Actual-PCM checks for the requested pitch domain. These are software evidence only.

use super::*;
use crate::audio_engine::mixer::{PadPitchApplyError, RtMixer};
use crate::audio_engine::native_history_permit::NativeHistoryContext;
use crate::audio_engine::native_source_coverage::NormalLoopCoverage;
use crate::audio_engine::prepared_native_history::tests::fixture;
use crate::audio_engine::source_reader::ExplicitSeekMode;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

const RATE: f64 = 48_000.0;
const SOURCE_HZ: f64 = 160.0;
const MAX_PITCH_ERROR_CENTS: f64 = 15.0;

fn requested_pitch_domain() -> impl Iterator<Item = (i32, i32)> {
    (-18..=18)
        .map(|extra| (0, extra))
        .chain((-23..=-19).map(|total| (-5, total + 5)))
        .chain((19..=24).map(|total| (6, total - 6)))
}

fn frequency(pcm: &[f32]) -> f64 {
    let crossings = pcm
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] <= 0.0 && pair[1] > 0.0)
        .map(|(index, pair)| index as f64 - f64::from(pair[0]) / f64::from(pair[1] - pair[0]))
        .collect::<Vec<_>>();
    assert!(crossings.len() > 8, "insufficient audible cycles");
    (crossings.len() - 1) as f64 * RATE / (crossings.last().unwrap() - crossings[0])
}

fn render(intent: PitchIntent, rate: f64, lock: bool, partitions: &[usize]) -> Vec<f32> {
    let mut processor = StretchProcessor::new(1);
    let pitch = intent.native_pitch_scale(rate, lock).unwrap();
    // Cold setup belongs outside render: use the same exact-pitch preparation as the worker.
    processor
        .native
        .rubberband
        .as_mut()
        .unwrap()
        .prepare_exact_pitch(pitch)
        .unwrap();
    processor.native.pitch_scale = pitch;
    let total = 96_000;
    let mut output = Vec::with_capacity(total);
    let mut cursor = 0;
    let mut partition = 0;
    while cursor < total {
        let frames = partitions[partition % partitions.len()].min(total - cursor);
        for (offset, sample) in processor.resampled_buffers_mut(frames)[0][..frames]
            .iter_mut()
            .enumerate()
        {
            *sample = (((cursor + offset) as f64 * rate * SOURCE_HZ * std::f64::consts::TAU / RATE)
                .sin()
                * 0.25) as f32;
        }
        processor.process_resampled_with_pitch(frames, rate, lock, intent);
        output.extend_from_slice(&processor.output_buffers()[0][..frames]);
        cursor += frames;
        partition += 1;
    }
    assert!(output.iter().all(|sample| sample.is_finite()));
    output
}

#[test]
fn every_extra_and_all_48_totals_produce_the_intended_frequency_for_both_locks_and_rates() {
    let mut worst = (0.0_f64, PitchIntent::default(), 1.0_f64, false);
    for (base, extra) in requested_pitch_domain() {
        let intent = PitchIntent::new(base, extra).unwrap();
        for rate in [0.5, 1.0, 2.0] {
            for lock in [false, true] {
                let pcm = render(intent, rate, lock, &[512]);
                let measured = frequency(&pcm[48_000..]);
                // The signal oracle derives its target from raw requested semitones;
                // it must not inherit a wrong/clipped production h helper.
                let expected = SOURCE_HZ
                    * 2.0_f64.powf(f64::from(base + extra) / 12.0)
                    * if lock { 1.0 } else { rate };
                let cents = 1200.0 * (measured / expected).log2();
                if cents.abs() > worst.0 {
                    worst = (cents.abs(), intent, rate, lock);
                }
                assert!(
                    cents.abs() < MAX_PITCH_ERROR_CENTS,
                    "pitch={intent:?} rate={rate} lock={lock} measured={measured} expected={expected} cents={cents}"
                );
                let energy = pcm[48_000..]
                    .iter()
                    .map(|sample| f64::from(*sample).powi(2))
                    .sum::<f64>()
                    / 48_000.0;
                assert!(
                    energy > 0.001,
                    "inaudible requested pitch={intent:?} rate={rate} lock={lock} energy={energy}"
                );
            }
        }
    }
    println!(
        "pitch_domain max_abs_cents={} intent={:?} rate={} key_lock={}",
        worst.0, worst.1, worst.2, worst.3
    );
}

#[test]
fn exact_target_preparation_from_constructor_and_reuse_has_identical_audible_first_blocks() {
    let mut max_history = 0;
    for (base, extra) in requested_pitch_domain() {
        let intent = PitchIntent::new(base, extra).unwrap();
        for rate in [0.5, 1.0, 2.0] {
            for lock in [false, true] {
                let pitch = intent.native_pitch_scale(rate, lock).unwrap();
                let mut fresh = RubberBandLiveShifter::new(48_000, 1).unwrap();
                let mut reused = RubberBandLiveShifter::new(48_000, 1).unwrap();
                let block = fresh.block_size();
                let mut input = vec![vec![0.0; block]];
                let mut fresh_output = vec![vec![0.0; block]];
                let mut reused_output = vec![vec![0.0; block]];
                reused.prepare_exact_pitch(0.73).unwrap();
                input[0].fill(0.25);
                for _ in 0..8 {
                    reused.shift(&input, &mut reused_output).unwrap();
                }
                fresh.prepare_exact_pitch(pitch).unwrap();
                reused.prepare_exact_pitch(pitch).unwrap();
                let history = fresh.source_history_frames(pitch).unwrap();
                assert_eq!(history, reused.source_history_frames(pitch).unwrap());
                max_history = max_history.max(history);
                let blocks = history.div_ceil(block);
                for index in 0..blocks + 2 {
                    for (offset, sample) in input[0].iter_mut().enumerate() {
                        let frame = index * block + offset;
                        *sample = (frame as f64 * rate * SOURCE_HZ * std::f64::consts::TAU / RATE)
                            .sin() as f32
                            * 0.25;
                    }
                    fresh.shift(&input, &mut fresh_output).unwrap();
                    reused.shift(&input, &mut reused_output).unwrap();
                    assert_eq!(
                        fresh_output, reused_output,
                        "pitch={intent:?} rate={rate} lock={lock} block={index}"
                    );
                    if index >= blocks {
                        let energy = fresh_output[0]
                            .iter()
                            .map(|sample| f64::from(*sample).powi(2))
                            .sum::<f64>()
                            / block as f64;
                        assert!(
                            energy > 0.001,
                            "prepared first-block silence: pitch={intent:?} rate={rate} lock={lock} history={history} energy={energy}"
                        );
                    }
                }
            }
        }
    }
    println!("pitch_native constructor_reuse max_primed_output_frames={max_history}");
}

#[test]
fn pitch_unity_bypasses_at_nonunit_rate_and_nonzero_pitch_processes_at_unit_rate() {
    let intent = PitchIntent::new(0, 12).unwrap();
    let mut processor = StretchProcessor::new(1);
    let input = (0..512)
        .map(|n| (n as f32 * 0.043).sin() * 0.3)
        .collect::<Vec<_>>();
    processor.resampled_buffers_mut(512)[0][..512].copy_from_slice(&input);
    processor.process_resampled_with_pitch(512, 2.0, true, intent);
    assert_eq!(&processor.output_buffers()[0][..512], input.as_slice());
    assert!(!processor.native.active);
    processor.process_resampled_with_pitch(512, 1.0, false, intent);
    assert!(processor.native.active);
    assert_eq!(processor.native.pitch_scale, 2.0);
}

#[test]
fn near_unity_native_pitch_is_never_silently_replaced_with_dry() {
    let intent = PitchIntent::new(0, 7).unwrap();
    for factor in [1.0 - 0.000_1, 1.0 + 0.000_1] {
        let rate = intent.frequency_ratio() / factor;
        let mut processor = StretchProcessor::new(1);
        processor.resampled_buffers_mut(512)[0][..512].fill(0.25);
        processor.process_resampled_with_pitch(512, rate, true, intent);
        assert!(processor.native.active);
        assert_eq!(
            processor.native.pitch_scale,
            intent.native_pitch_scale(rate, true).unwrap()
        );
    }
}

#[test]
fn prepared_pitch_crossing_native_unity_retains_its_fixed_history_and_adapter() {
    let intent = PitchIntent::new(0, 7).unwrap();
    let mut request = fixture(1, 48_000, 1.7, ExplicitSeekMode::Normal, None);
    request.pitch_intent = intent;
    let mut processor = StretchProcessor::new(1);
    processor.native.prepare_source(request).unwrap();
    let address = processor.native_state_address();
    for rate in [
        intent.frequency_ratio() - 0.000_1,
        intent.frequency_ratio(),
        intent.frequency_ratio() + 0.000_1,
    ] {
        let input = processor.resampled_buffers_mut(512);
        for (frame, sample) in input[0][..512].iter_mut().enumerate() {
            *sample =
                (frame as f64 * rate * SOURCE_HZ * std::f64::consts::TAU / RATE).sin() as f32 * 0.2;
        }
        processor.process_resampled_with_pitch(512, rate, true, intent);
        assert_eq!(processor.native_state_address(), address);
        assert!(processor.native.active);
        assert_eq!(
            processor.native.pitch_scale,
            intent.native_pitch_scale(rate, true).unwrap()
        );
        let (input, output) = processor.pending_fifo_frames();
        assert_eq!(input + output, processor.adapter_delay_frames());
        assert!(
            processor.output_buffers()[0][..512]
                .iter()
                .all(|sample| sample.is_finite())
        );
    }
}

#[test]
fn nonzero_pitch_adapter_preserves_exact_pcm_under_irregular_partitions() {
    for (intent, rate, lock) in [
        (PitchIntent::new(-5, -18).unwrap(), 2.0, true),
        (PitchIntent::new(6, 18).unwrap(), 0.5, true),
        (PitchIntent::new(0, 7).unwrap(), 1.0, false),
    ] {
        let regular = render(intent, rate, lock, &[512]);
        let irregular = render(intent, rate, lock, &[1, 127, 384, 96, 257, 512, 31]);
        assert_eq!(
            regular, irregular,
            "pitch={intent:?} rate={rate} lock={lock}"
        );
    }
}

#[test]
fn prepared_history_rejects_a_different_frozen_pitch_or_lock_tuple() {
    let mut request = fixture(1, 48_000, 1.0, ExplicitSeekMode::Normal, None);
    request.pitch_intent = PitchIntent::new(0, 7).unwrap();
    request.key_lock = false;
    let mut native =
        NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 1).unwrap()), 1);
    native.prepare_source(request).unwrap();
    let source = native.source.as_ref().unwrap();
    let feed = ProductiveSourceFeed {
        sample: &source.sample,
        stems: None,
        outgoing_stems: None,
        sample_rate_hz: 48_000,
        accepted: None,
        plan: source.plan,
        playback: &source.playback,
        permit: Some(&source.permit),
        output_frame: Some(source.target_output_frame),
        pitch_intent: source.pitch_intent,
        key_lock: false,
    };
    assert!(source.matches_adoption(&feed, source.epoch, source.request_id));
    let wrong_pitch = ProductiveSourceFeed {
        pitch_intent: PitchIntent::new(0, 8).unwrap(),
        ..feed
    };
    assert!(!source.matches_contract(&wrong_pitch, source.epoch));
    let wrong_lock = ProductiveSourceFeed {
        key_lock: true,
        ..wrong_pitch
    };
    assert!(!source.matches_effective_contract(&wrong_lock, source.epoch));
}

#[test]
fn endpoint_preparation_keeps_normal_launch_coordinate_and_current_source_time() {
    for mode in [
        ExplicitSeekMode::Normal,
        ExplicitSeekMode::BeforeLoop,
        ExplicitSeekMode::AfterLoop,
    ] {
        let mut request = fixture(1, 48_000, 0.5, mode, None);
        request.pitch_intent = PitchIntent::new(-5, -18).unwrap();
        let endpoint = request.playback;
        let endpoint_plan = request.plan;
        let target = request.target_output_frame;
        let mut native =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 1).unwrap()), 1);
        native.prepare_source_at_endpoint(request).unwrap();
        let source = native.source.as_ref().unwrap();
        assert!(source.playback.matches_exact(&endpoint));
        assert!(source.plan.matches_exact(endpoint_plan));
        assert_eq!(source.target_output_frame, target);
        assert!(
            native
                .output
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
    }
}

#[test]
fn finite_normal_loop_pitch_history_matches_complete_pcm_for_every_extra_and_all_48_totals() {
    for (base, extra) in requested_pitch_domain() {
        let intent = PitchIntent::new(base, extra).unwrap();
        for lock in [false, true] {
            let mut complete = fixture(1, 48_000, 2.0, ExplicitSeekMode::Normal, None);
            complete.pitch_intent = intent;
            complete.key_lock = lock;
            let mut finite = fixture(1, 48_000, 2.0, ExplicitSeekMode::Normal, None);
            finite.pitch_intent = intent;
            finite.key_lock = lock;
            finite.sample = finite
                .sample
                .with_complete_source(48_000)
                .window(
                    finite.plan.loop_region.start,
                    finite.plan.loop_region.end,
                    17,
                    crate::messages::ResidentContext::KeyLockFiniteLoop,
                )
                .unwrap();
            finite.binding = ProductiveSourceBinding::new(&finite.sample, 48_000, None);
            let ownership = Arc::new(
                crate::audio_engine::input_runtime_binding::InputRuntimeOwnership::tracked(),
            );
            ownership.publish_test_source(0, &finite.sample, 48_000, 13);
            let acknowledgements = Arc::new(
                crate::audio_engine::constant_timing::CurrentTimingAcknowledgements::default(),
            );
            let epoch = Arc::new(AtomicU64::new(3));
            finite.permit = NativeHistoryContext {
                id: 0,
                ownership: &ownership,
                acknowledgements: &acknowledgements,
                preparation_epoch: &epoch,
            }
            .capture(&finite.sample, 48_000, None)
            .unwrap();
            finite.coverage = Some(
                NormalLoopCoverage::capture(
                    &finite.sample,
                    None,
                    48_000,
                    None,
                    finite.plan,
                    &finite.playback,
                )
                .unwrap(),
            );
            let mut full_state =
                NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 1).unwrap()), 1);
            let mut finite_state =
                NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 1).unwrap()), 1);
            full_state.prepare_source_at_endpoint(complete).unwrap();
            finite_state.prepare_source_at_endpoint(finite).unwrap();
            assert_eq!(
                full_state.output, finite_state.output,
                "pitch={intent:?} lock={lock}"
            );
            assert_eq!(full_state.pitch_scale, finite_state.pitch_scale);
            assert!(
                finite_state
                    .source
                    .as_ref()
                    .unwrap()
                    .permit
                    .current(&finite_state.source.as_ref().unwrap().sample, 48_000)
            );
        }
    }
}

#[test]
fn unready_active_pitch_rejects_atomically_and_other_pad_clock_and_pcm_stay_identical() {
    let source = SampleBuffer {
        channels: 1,
        residency: None,
        samples: Arc::from(
            (0..96_000)
                .map(|n| (n as f64 * SOURCE_HZ * std::f64::consts::TAU / RATE).sin() as f32 * 0.2)
                .collect::<Vec<_>>(),
        ),
    };
    let mut control = RtMixer::new(1, RATE as f32);
    let mut changed = RtMixer::new(1, RATE as f32);
    for mixer in [&mut control, &mut changed] {
        mixer.load_sample(0, source.clone());
        mixer.load_sample(1, source.clone());
        assert!(mixer.play_sample(0, 1.0));
        assert!(mixer.play_sample(1, 1.0));
    }
    let wanted = PitchIntent::new(0, 9).unwrap();
    assert_eq!(
        changed.set_pad_pitch_intent(0, wanted),
        Err(PadPitchApplyError::PreparationUnavailable)
    );
    assert_eq!(changed.pad_pitch_intent(0), Some(PitchIntent::default()));
    let mut control_pcm = vec![0.0; 512];
    let mut changed_pcm = vec![0.0; 512];
    let mut peaks = [0.0; crate::audio_engine::constants::NUM_SAMPLES];
    for _ in 0..16 {
        control.render(&mut control_pcm, &mut peaks);
        changed.render(&mut changed_pcm, &mut peaks);
        assert_eq!(control_pcm, changed_pcm);
        for (left, right) in control.voices.iter().zip(&changed.voices) {
            assert!(left.source_playback.matches_exact(&right.source_playback));
        }
    }
}
