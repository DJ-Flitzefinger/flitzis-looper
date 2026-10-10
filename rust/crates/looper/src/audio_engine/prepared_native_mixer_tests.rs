//! Actual mixer adoption, deadline splitting and current native source/authority rejection.

use super::*;
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
use crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES;
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::rubberband_backend::{RubberBandLiveShifter, pitch_scale_for_tempo_ratio};
use crate::audio_engine::source_reader::{
    TapReadObservation, reset_tap_observation_for_test, tap_observation_for_test,
};
use crate::messages::ResidentContext;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

#[path = "finite_stem_transition_tests.rs"]
mod finite_stem_transition_tests;

#[path = "finite_lifecycle_tests.rs"]
pub(crate) mod finite_lifecycle_tests;

const RATE: u32 = 8_000;

fn source() -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(
            (0..16_000)
                .map(|n| (n as f32 * 0.073).sin() * 0.5)
                .collect::<Vec<_>>(),
        ),
    }
}

fn projection() -> AcceptedTimingProjection {
    AcceptedTimingProjection {
        revision: [17; 32],
        period_seconds: 0.500_123_456_789_012_3,
        origin_seconds: -0.0,
        sample_rate_hz: RATE,
        publication_epoch: 9,
    }
}

fn fixture(sample: &SampleBuffer) -> RtMixer {
    let mut mixer = RtMixer::new(1, RATE as f32);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_source(0, sample, RATE, 3);
    mixer.set_input_runtime_ownership(ownership);
    mixer.load_sample(0, sample.clone());
    let publication = PreparedSourcePermit::unrestricted();
    publication.mark_pending().unwrap();
    assert!(mixer.publish_constant_timing_rt(
        0,
        PreparedConstantTiming {
            reference: sample.clone(),
            publication,
            projection: projection(),
        },
        &mut ImmediateAudioBufferRetirement
    ));
    mixer.set_speed(1.371_234_567_890_123);
    mixer.set_pad_key_lock(0, true);
    mixer.set_pad_loop_region(0, 0.003125, Some(0.300125));
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

fn voice(mixer: &RtMixer) -> &VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn voice_mut(mixer: &mut RtMixer) -> &mut VoiceSlot {
    mixer
        .voices
        .iter_mut()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn render(mixer: &mut RtMixer, start: Option<u64>, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames];
    let mut peaks = [0.0; NUM_SAMPLES];
    if let Some(start) = start {
        mixer.render_at_output_frame(start, &mut output, &mut peaks);
    } else {
        mixer.render(&mut output, &mut peaks);
    }
    output
}

#[track_caller]
fn wait_ready(mixer: &mut RtMixer) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !voice_mut(mixer).stretch.source_preparation_ready() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(voice_mut(mixer).stretch.source_preparation_ready());
}

/// Independent algebraic fractional loop addressing and raw native shifting; no SourceReadPlan,
/// SourcePlayback, prepared adapter or worker is used to construct expected samples.
fn raw_native_suffix(sample: &SampleBuffer, suffix_frames: usize) -> Vec<f32> {
    let ratio = 1.371_234_567_890_123;
    let mut native = RubberBandLiveShifter::new(RATE, 1).unwrap();
    native
        .prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
        .unwrap();
    let block = native.block_size();
    let mut output = vec![vec![0.0; block]];
    let mut stream = vec![0.0; block - 1];
    let total = (PREPARED_HISTORY_FRAMES + suffix_frames).div_ceil(block) * block;
    for start in (0..total).step_by(block) {
        let feed = vec![
            (start..start + block)
                .map(|frame| {
                    let distance = frame as f64 * ratio;
                    let whole = distance.floor() as usize;
                    let left = sample.samples[25 + whole % 2376];
                    let right = sample.samples[25 + (whole + 1) % 2376];
                    left + (right - left) * (distance - whole as f64) as f32
                })
                .collect::<Vec<_>>(),
        ];
        native.shift(&feed, &mut output).unwrap();
        stream.extend_from_slice(&output[0]);
    }
    stream[PREPARED_HISTORY_FRAMES..PREPARED_HISTORY_FRAMES + suffix_frames].to_vec()
}

/// Configure the finite range while stopped, then enable actual KEYLOCK. The independent
/// complete PCM belongs only to the test oracle; this mixer and its worker receive the crop.
fn finite_mixer(
    sample: &SampleBuffer,
    rate: u32,
    timing: AcceptedTimingProjection,
    start: usize,
    end: usize,
    ratio: f64,
) -> RtMixer {
    assert_eq!(sample.resident_start(), start);
    assert_eq!(sample.resident_end(), end);
    assert_eq!(sample.samples.len(), (end - start) * sample.channels);
    let mut mixer = RtMixer::new(sample.channels, rate as f32);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_source(0, sample, rate, 3);
    mixer.set_input_runtime_ownership(ownership);
    mixer.load_sample(0, sample.clone());
    let publication = PreparedSourcePermit::unrestricted();
    publication.mark_pending().unwrap();
    assert!(mixer.publish_constant_timing_rt(
        0,
        PreparedConstantTiming {
            reference: sample.clone(),
            publication,
            projection: timing,
        },
        &mut ImmediateAudioBufferRetirement,
    ));
    mixer.set_speed(ratio);
    mixer.set_pad_loop_region(
        0,
        start as f64 / f64::from(rate),
        Some(end as f64 / f64::from(rate)),
    );
    mixer.set_pad_key_lock(0, true);
    assert!(mixer.pad_key_lock_enabled[0]);
    assert!(mixer.can_play_sample(0, 1.0));
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

fn assert_finite_taps(reads: TapReadObservation, start: usize, end: usize, frames: usize) {
    assert_eq!(reads.left_reads, frames);
    assert!(reads.right_reads <= frames);
    assert_eq!(reads.missing_reads, 0);
    assert!(reads.min_frame.is_some_and(|frame| frame >= start));
    assert!(reads.max_frame.is_some_and(|frame| frame < end));
    assert_ne!(reads.address_checksum, 0);
}

fn render_finite(
    mixer: &mut RtMixer,
    frame: u64,
    frames: usize,
    start: usize,
    end: usize,
) -> Vec<f32> {
    reset_tap_observation_for_test();
    let output = render(mixer, Some(frame), frames);
    let reads = tap_observation_for_test();
    assert_finite_taps(reads, start, end, frames);
    output
}

fn physical_oracle_tap_checksum(frames: usize, ratio: f64) -> u64 {
    let mut checksum = 0_u64;
    for frame in 0..frames {
        let distance = frame as f64 * ratio;
        let whole = distance.floor() as usize;
        for (tap, right) in [
            (Some(25 + whole % 2376), false),
            (
                (distance.fract() != 0.0).then_some(25 + (whole + 1) % 2376),
                true,
            ),
        ] {
            if let Some(tap) = tap {
                checksum = checksum
                    .wrapping_mul(1_099_511_628_211)
                    .wrapping_add(tap as u64)
                    .wrapping_mul(1_099_511_628_211)
                    .wrapping_mul(1_099_511_628_211)
                    .wrapping_add(u64::from(right));
            }
        }
    }
    checksum
}

#[test]
fn finite_prepared_native_mixer_splits_irregular_callback_and_moves_actual_handle_and_fifos() {
    let full = source().with_complete_source(RATE);
    let finite = full
        .window(25, 2401, 2, ResidentContext::KeyLockFiniteLoop)
        .unwrap();
    assert!(!Arc::ptr_eq(&finite.samples, &full.samples));
    let expected = raw_native_suffix(&full, 12_853);
    let mut actual = finite_mixer(&finite, RATE, projection(), 25, 2401, 1.371_234_567_890_123);
    let mut reference = fixture(&full);
    let old_native = voice(&actual).stretch.native_state_address();
    assert_ne!(old_native, 0);
    assert_eq!(
        render(&mut actual, Some(0), 13),
        render(&mut reference, Some(0), 13)
    );
    wait_ready(&mut actual);
    wait_ready(&mut reference);
    let prepared_reads = voice_mut(&mut actual)
        .stretch
        .prepared_tap_observation()
        .unwrap();
    assert_finite_taps(prepared_reads, 25, 2401, 4096);
    assert_eq!(
        prepared_reads.address_checksum,
        physical_oracle_tap_checksum(4096, 1.371_234_567_890_123)
    );
    let prepared_native = voice_mut(&mut actual).stretch.prepared_native_address();
    assert_ne!(prepared_native, old_native);
    assert_eq!(
        voice_mut(&mut actual).stretch.prepared_fifo_frames(),
        Some((0, 511))
    );
    assert_eq!(
        voice_mut(&mut actual)
            .stretch
            .prepared_target_output_frame(),
        Some(4096)
    );
    let mut frame = 13;
    for frames in [31, 777, 1024, 1, 2247] {
        assert_eq!(
            render(&mut actual, Some(frame), frames),
            render(&mut reference, Some(frame), frames)
        );
        frame += frames as u64;
    }
    assert_eq!(frame, 4093);
    let join = render(&mut actual, Some(frame), 37);
    assert_eq!(join, render(&mut reference, Some(frame), 37));
    assert_eq!(&join[3..], &expected[..34]);
    assert_eq!(
        voice(&actual).stretch.native_state_address(),
        prepared_native
    );
    assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
    let mut elapsed = 34;
    for requested in [1, 127, 384, 96, 257, 512, 31].into_iter().cycle() {
        if elapsed == expected.len() {
            break;
        }
        let frames = requested.min(expected.len() - elapsed);
        let output = render_finite(&mut actual, 4096 + elapsed as u64, frames, 25, 2401);
        assert_eq!(output, expected[elapsed..elapsed + frames]);
        assert_eq!(
            output,
            render(&mut reference, Some(4096 + elapsed as u64), frames)
        );
        elapsed += frames;
        assert_eq!(
            voice(&actual).stretch.native_state_address(),
            prepared_native
        );
        assert_eq!(
            voice(&actual).stretch.native_tap_observation(),
            Some(prepared_reads)
        );
        let history = voice(&actual).stretch.productive_history().unwrap();
        assert_eq!(history.fed_output_frames, (4096 + elapsed) as u64);
        assert_eq!(
            history.next_position,
            voice(&actual).source_playback.position()
        );
        assert!(
            voice(&actual)
                .source_playback
                .matches_exact(&voice(&reference).source_playback)
        );
        assert_eq!(voice(&actual).sample.as_ref().unwrap().samples.len(), 2376);
    }
    assert!(expected.iter().any(|value| value.abs() > 0.02));
}

#[test]
fn prepared_native_mixer_splits_irregular_callback_and_moves_actual_handle_and_fifos() {
    let sample = source();
    let expected_suffix = raw_native_suffix(&sample, 12_853);
    let mut actual = fixture(&sample);
    let mut prior = fixture(&sample);
    let old_native = voice(&actual).stretch.native_state_address();
    assert_eq!(
        render(&mut actual, Some(0), 13),
        render(&mut prior, None, 13)
    );
    wait_ready(&mut actual);
    assert_eq!(
        voice_mut(&mut actual)
            .stretch
            .prepared_target_output_frame(),
        Some(PREPARED_HISTORY_FRAMES as u64)
    );
    let prepared_native = voice_mut(&mut actual).stretch.prepared_native_address();
    assert_ne!(prepared_native, old_native);
    assert_eq!(
        voice_mut(&mut actual).stretch.prepared_fifo_frames(),
        Some((0, 511))
    );
    assert_eq!(voice(&actual).stretch.native_state_address(), old_native);
    let mut frame = 13;
    for frames in [31, 777, 1024, 1, 2247] {
        assert_eq!(
            render(&mut actual, Some(frame), frames),
            render(&mut prior, None, frames)
        );
        frame += frames as u64;
    }
    assert_eq!(frame, 4093);
    let before = voice(&actual).source_playback;
    let output = render(&mut actual, Some(frame), 37);
    let prior_output = render(&mut prior, None, 37);
    assert_eq!(&output[..3], &prior_output[..3]);
    assert!(output[3..].iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(&output[3..], &expected_suffix[..34]);
    assert_eq!(
        voice(&actual).stretch.native_state_address(),
        prepared_native
    );
    assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
    assert!(
        voice(&actual)
            .source_playback
            .matches_exact(&voice(&prior).source_playback)
    );
    assert_ne!(voice(&actual).source_playback.position(), before.position());
    let history = voice(&actual).stretch.productive_history().unwrap();
    assert_eq!(history.binding.accepted, Some(projection()));
    assert_eq!(
        history.binding.accepted.unwrap().origin_seconds.to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(
        history.next_position,
        voice(&actual).source_playback.position()
    );
    assert_eq!(history.fed_output_frames, 4096 + 34);
    let pattern = [1, 127, 384, 96, 257, 512, 31];
    let mut elapsed = 34;
    let mut index = 0;
    while elapsed < expected_suffix.len() {
        let frames = pattern[index % pattern.len()].min(expected_suffix.len() - elapsed);
        let output = render(&mut actual, Some(4096 + elapsed as u64), frames);
        assert_eq!(output, expected_suffix[elapsed..elapsed + frames]);
        assert_eq!(
            voice(&actual).stretch.native_state_address(),
            prepared_native
        );
        elapsed += frames;
        index += 1;
    }
    assert!(expected_suffix.iter().any(|sample| sample.abs() > 0.02));
}

#[test]
fn prepared_native_mixer_rejects_actual_source_request_authority_and_control_changes() {
    for case in 0..15 {
        let sample = source();
        let mut actual = fixture(&sample);
        let mut prior = fixture(&sample);
        assert_eq!(
            render(&mut actual, Some(0), 127),
            render(&mut prior, None, 127)
        );
        wait_ready(&mut actual);
        let native = voice(&actual).stretch.native_state_address();
        for mixer in [&mut actual, &mut prior] {
            match case {
                0 => {
                    mixer.prepared_source_epochs[0].fetch_add(1, Ordering::AcqRel);
                }
                1 => {
                    mixer.input_runtime_ownership.authority[0].fetch_add(1, Ordering::AcqRel);
                }
                2 => {
                    mixer.input_runtime_ownership.runtime[0].fetch_add(1, Ordering::AcqRel);
                }
                3 => mixer.input_runtime_ownership.revoke_source(0),
                4 => mixer
                    .input_runtime_ownership
                    .publish_source(0, &sample, RATE, 4),
                5 => mixer
                    .input_runtime_ownership
                    .publish_source(0, &sample, RATE * 2, 3),
                6 => mixer.set_speed(1.371_234_567_890_123), // equal edit still fences preparation
                7 => mixer.set_pad_loop_region(0, 0.003125, Some(0.300125)),
                8 => {
                    mixer.pause_sample(0);
                    mixer.resume_sample(0);
                }
                9 => {
                    mixer.set_pad_key_lock(0, false);
                    mixer.set_pad_key_lock(0, true);
                }
                10 => {
                    let publication = PreparedSourcePermit::unrestricted();
                    publication.mark_pending().unwrap();
                    assert!(mixer.publish_constant_timing_rt(
                        0,
                        PreparedConstantTiming {
                            reference: sample.clone(),
                            publication,
                            projection: AcceptedTimingProjection {
                                revision: [18; 32],
                                publication_epoch: 10,
                                ..projection()
                            },
                        },
                        &mut ImmediateAudioBufferRetirement
                    ));
                }
                11 => {
                    let replacement = source();
                    mixer
                        .input_runtime_ownership
                        .publish_source(0, &replacement, RATE, 4);
                    mixer.load_sample(0, replacement);
                }
                12 => mixer.current_timing_acknowledgements.clear(0),
                13 | 14 => {
                    let publication = PreparedSourcePermit::unrestricted();
                    publication.mark_pending().unwrap();
                    let mut changed = projection();
                    changed.publication_epoch = 10;
                    if case == 13 {
                        changed.period_seconds =
                            f64::from_bits(changed.period_seconds.to_bits() + 1);
                    } else {
                        changed.origin_seconds = 0.0;
                    }
                    assert!(mixer.publish_constant_timing_rt(
                        0,
                        PreparedConstantTiming {
                            reference: sample.clone(),
                            publication,
                            projection: changed,
                        },
                        &mut ImmediateAudioBufferRetirement
                    ));
                }
                _ => unreachable!(),
            }
        }
        let output = render(&mut actual, Some(127), 4000);
        assert_eq!(output, render(&mut prior, None, 4000), "case {case}");
        assert!(
            output.iter().any(|sample| sample.abs() > 0.001),
            "case {case}"
        );
        assert_eq!(
            voice(&actual).stretch.native_state_address(),
            native,
            "case {case}"
        );
        assert!(
            voice(&actual).stretch.adopted_request_id().is_none(),
            "case {case}"
        );
    }
}

#[test]
fn prepared_native_mixer_stem_selection_ramp_defers_then_adopts_actual_stem_history() {
    let sample = source();
    let mut mixer = fixture(&sample);
    let stems = PreparedStemSet {
        complete_set_identity: std::sync::Arc::new([0; 32]),
        reference_samples: sample.samples.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        accepted_timing: Some(projection()),
        source_version_hash: 37,
        sample_rate_hz: RATE,
        channels: 1,
        frame_count: sample.samples.len(),
        available_mask: ((1_u16 << crate::messages::STEM_BUFFER_COUNT) - 1) as u8,
        stems: std::array::from_fn(|index| SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(
                sample
                    .samples
                    .iter()
                    .map(|value| *value * (index + 1) as f32 * 0.1)
                    .collect::<Vec<_>>(),
            ),
        }),
    };
    mixer.stop_sample(0);
    assert!(mixer.publish_prepared_stems(0, stems));
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 37));
    assert!(mixer.stem_transitions[0].is_active());
    render(&mut mixer, Some(0), 512);
    assert!(!voice_mut(&mut mixer).stretch.source_preparation_ready());
    assert!(!mixer.stem_transitions[0].is_active());
    render(&mut mixer, Some(512), 13);
    wait_ready(&mut mixer);
    let prepared = voice_mut(&mut mixer).stretch.prepared_native_address();
    assert_eq!(
        voice_mut(&mut mixer).stretch.prepared_target_output_frame(),
        Some(4608)
    );
    render(&mut mixer, Some(525), 4083);
    let output = render(&mut mixer, Some(4608), 113);
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(voice(&mixer).stretch.native_state_address(), prepared);
    assert_eq!(voice(&mixer).stretch.adopted_request_id(), Some(1));
    assert_eq!(
        voice(&mixer)
            .stretch
            .productive_history()
            .unwrap()
            .binding
            .accepted,
        Some(projection())
    );
    // A changed mask invalidates the prepared-history contract without resetting ongoing native history.
    assert!(mixer.set_stem_enabled_mask(0, 1, 37));
    render(&mut mixer, Some(4721), 512);
    assert_eq!(voice(&mixer).stretch.native_state_address(), prepared);
    render(&mut mixer, Some(5233), 1);
    wait_ready(&mut mixer);
    let old = voice(&mixer).stretch.native_state_address();
    assert!(mixer.set_stem_enabled_mask(0, 2, 37));
    let output = render(&mut mixer, Some(5234), 4100);
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(voice(&mixer).stretch.native_state_address(), old);
}

#[test]
fn prepared_native_mixer_late_or_failed_adoption_keeps_previous_shifted_output() {
    for fail_worker in [false, true] {
        let sample = source();
        let mut actual = fixture(&sample);
        let mut prior = fixture(&sample);
        assert_eq!(
            render(&mut actual, Some(0), 1657),
            render(&mut prior, None, 1657)
        );
        wait_ready(&mut actual);
        let native = voice(&actual).stretch.native_state_address();
        if fail_worker {
            voice(&actual).stretch.fail_preparation_worker();
        }
        // One case misses the deadline; the other reaches it with a failed worker.
        let frames = if fail_worker { 4096 - 1657 + 113 } else { 113 };
        let start = if fail_worker { 1657 } else { 10_000 };
        let output = render(&mut actual, Some(start), frames);
        assert_eq!(output, render(&mut prior, None, frames));
        assert!(output.iter().any(|sample| sample.abs() > 0.001));
        assert_eq!(voice(&actual).stretch.native_state_address(), native);
        assert!(voice(&actual).stretch.adopted_request_id().is_none());
    }
}

#[test]
fn prepared_native_mixer_adopts_while_canonical_rate_smoothing_is_still_active() {
    let sample = source();
    let mut actual = fixture(&sample);
    let mut prior = fixture(&sample);
    for mixer in [&mut actual, &mut prior] {
        mixer.set_speed(0.5);
        assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
        mixer.set_speed(2.0);
    }
    assert_eq!(render(&mut actual, Some(0), 1), render(&mut prior, None, 1));
    wait_ready(&mut actual);
    let prepared = voice_mut(&mut actual).stretch.prepared_native_address();
    for (start, frames) in [(1, 31), (32, 1024), (1056, 3040)] {
        assert_eq!(
            render(&mut actual, Some(start), frames),
            render(&mut prior, None, frames)
        );
    }
    let output = render(&mut actual, Some(4096), 137);
    render(&mut prior, None, 137);
    assert_eq!(voice(&actual).stretch.native_state_address(), prepared);
    assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    assert!(voice(&actual).source_playback.tempo_ratio() < 1.0);
    assert!(
        voice(&actual)
            .source_playback
            .matches_exact(&voice(&prior).source_playback)
    );
    // Another step continues the same copied canonical ramp after native/FIFO adoption.
    render(&mut actual, Some(4233), 777);
    render(&mut prior, None, 777);
    assert!(
        voice(&actual)
            .source_playback
            .matches_exact(&voice(&prior).source_playback)
    );
    assert_eq!(voice(&actual).stretch.native_state_address(), prepared);
}

#[test]
fn finite_prepared_native_mixer_adopts_while_canonical_rate_smoothing_is_still_active() {
    let full = source().with_complete_source(RATE);
    let finite = full
        .window(25, 2401, 2, ResidentContext::KeyLockFiniteLoop)
        .unwrap();
    let mut actual = finite_mixer(&finite, RATE, projection(), 25, 2401, 0.5);
    let mut reference = fixture(&full);
    reference.set_speed(0.5);
    assert!(reference.play_sample_at_output_frame(0, 1.0, 0));
    for mixer in [&mut actual, &mut reference] {
        mixer.set_speed(2.0);
    }
    assert_eq!(
        render(&mut actual, Some(0), 1),
        render(&mut reference, Some(0), 1)
    );
    wait_ready(&mut actual);
    wait_ready(&mut reference);
    let prepared = voice_mut(&mut actual).stretch.prepared_native_address();
    assert_ne!(prepared, voice(&actual).stretch.native_state_address());
    assert_finite_taps(
        voice_mut(&mut actual)
            .stretch
            .prepared_tap_observation()
            .unwrap(),
        25,
        2401,
        4096,
    );
    assert_eq!(
        voice_mut(&mut actual).stretch.prepared_fifo_frames(),
        Some((0, 511))
    );
    for (start, frames) in [(1, 31), (32, 1024), (1056, 3040), (4096, 137), (4233, 777)] {
        let output = render_finite(&mut actual, start, frames, 25, 2401);
        assert_eq!(output, render(&mut reference, Some(start), frames));
        assert!(
            voice(&actual)
                .source_playback
                .matches_exact(&voice(&reference).source_playback)
        );
        if start >= 4096 {
            assert_eq!(voice(&actual).stretch.native_state_address(), prepared);
            assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
            assert!(output.iter().any(|value| value.abs() > 0.001));
            if start == 4096 {
                assert!(voice(&actual).source_playback.tempo_ratio() < 1.0);
            }
        }
    }
    assert_eq!(voice(&actual).sample.as_ref().unwrap().samples.len(), 2376);
}

#[test]
fn prepared_native_mixer_unload_releases_preparing_ready_and_adopted_pins_on_worker() {
    for phase in 0..3 {
        let sample = source();
        let weak = Arc::downgrade(&sample.samples);
        let mut mixer = fixture(&sample);
        render(&mut mixer, Some(0), 1);
        if phase > 0 {
            wait_ready(&mut mixer);
        }
        if phase == 2 {
            render(&mut mixer, Some(1), 4095);
            render(&mut mixer, Some(4096), 1);
            assert!(voice(&mixer).stretch.adopted_request_id().is_some());
        }
        assert!(weak.upgrade().is_some());
        drop(sample);
        mixer.input_runtime_ownership.revoke_source(0);
        mixer.unload_sample(0);
        assert!(!mixer.voices.iter().any(|voice| voice.active));
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut frame = 5000;
        while weak.upgrade().is_some() && Instant::now() < deadline {
            // The engine continues callbacks, but no inactive voice ever renders source again.
            render(&mut mixer, Some(frame), 16);
            frame += 16;
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            weak.upgrade().is_none(),
            "phase {phase}: cancelled source pin survived actual worker retirement"
        );
    }
}

#[test]
fn prepared_native_mixer_same_phase_seek_fences_prepared_candidate() {
    let sample = source();
    let mut mixer = fixture(&sample);
    render(&mut mixer, Some(0), 512);
    wait_ready(&mut mixer);
    let candidate = voice_mut(&mut mixer).stretch.prepared_native_address();
    let current = voice(&mixer).source_playback.position();
    assert!(mixer.seek_sample_at_output_frame(0, current.frame as f64 / f64::from(RATE), 512));
    render(&mut mixer, Some(512), 3585);
    assert_ne!(voice(&mixer).stretch.native_state_address(), candidate);
    assert!(voice(&mixer).stretch.adopted_request_id().is_none());
}

const MUSICAL_START: usize = 25;
const MUSICAL_END: usize = 1525;

fn musical_projection(rate: u32, period: f64) -> AcceptedTimingProjection {
    AcceptedTimingProjection {
        revision: [61; 32],
        // The admitted physical loop is one sixteenth of this accepted quarter.
        period_seconds: period * 16.0 / f64::from(rate),
        origin_seconds: -0.217_123_456_789,
        sample_rate_hz: rate,
        publication_epoch: 17,
    }
}

fn admitted_musical_period(rate: u32, declared_period: f64) -> f64 {
    // Retain actual binary64 seconds -> loaded frames -> logical beats order independently.
    // This is distinct from the rational fixture period and physical marker rounding.
    musical_projection(rate, declared_period).period_seconds * f64::from(rate) / 16.0
}

fn musical_mixer(sample: &SampleBuffer, rate: u32, period: f64, ratio: f64, wet: bool) -> RtMixer {
    musical_mixer_with_stems(sample, rate, period, ratio, wet, None)
}

fn musical_mixer_with_stems(
    sample: &SampleBuffer,
    rate: u32,
    period: f64,
    ratio: f64,
    wet: bool,
    stems: Option<PreparedStemSet>,
) -> RtMixer {
    let mut mixer = RtMixer::new(1, rate as f32);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_source(0, sample, rate, 3);
    mixer.set_input_runtime_ownership(ownership);
    mixer.load_sample(0, sample.clone());
    if let Some(stems) = stems {
        // Admit authentic nonaccepted stem ownership while the source is inactive. The
        // following actual accepted publication refreshes the retained same-source set.
        assert!(mixer.publish_prepared_stems(0, stems));
    }
    let publication = PreparedSourcePermit::unrestricted();
    publication.mark_pending().unwrap();
    assert!(mixer.publish_constant_timing_rt(
        0,
        PreparedConstantTiming {
            reference: sample.clone(),
            publication,
            projection: musical_projection(rate, period),
        },
        &mut ImmediateAudioBufferRetirement,
    ));
    mixer.set_speed(ratio);
    mixer.set_pad_key_lock(0, wet);
    mixer.set_pad_loop_region(
        0,
        MUSICAL_START as f64 / f64::from(rate),
        Some(MUSICAL_END as f64 / f64::from(rate)),
    );
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

/// Source knot addresses are independently computed from continuous output time. Endpoints
/// remain exact integers and no input ever reads the exclusive physical end at virtual P>H.
fn musical_sample(sample: &SampleBuffer, period: f64, distance: f64) -> f32 {
    musical_sample_from_knots(period, distance, |frame| sample.samples[frame])
}

fn musical_sample_from_knots(period: f64, distance: f64, read: impl Fn(usize) -> f32) -> f32 {
    let phase = distance.rem_euclid(period);
    let last = (period.ceil() as usize - 1).min(MUSICAL_END - MUSICAL_START - 1);
    let (left, right, fraction) = if phase >= last as f64 {
        (
            MUSICAL_START + last,
            MUSICAL_START,
            (phase - last as f64) / (period - last as f64),
        )
    } else {
        let left = MUSICAL_START + phase.floor() as usize;
        (left, left + 1, phase.fract())
    };
    assert!((MUSICAL_START..MUSICAL_END).contains(&left));
    assert!((MUSICAL_START..MUSICAL_END).contains(&right));
    let lower = read(left);
    lower + (read(right) - lower) * fraction as f32
}

fn musical_raw_native_suffix(
    sample: &SampleBuffer,
    rate: u32,
    period: f64,
    ratio: f64,
    frames: usize,
) -> Vec<f32> {
    let period = admitted_musical_period(rate, period);
    let mut native = RubberBandLiveShifter::new(rate, 1).unwrap();
    native
        .prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
        .unwrap();
    let block = native.block_size();
    let mut stream = vec![0.0; block - 1];
    let mut shifted = vec![vec![0.0; block]];
    let total = (PREPARED_HISTORY_FRAMES + frames).div_ceil(block) * block;
    for start in (0..total).step_by(block) {
        let input = vec![
            (start..start + block)
                .map(|frame| musical_sample(sample, period, frame as f64 * ratio))
                .collect::<Vec<_>>(),
        ];
        native.shift(&input, &mut shifted).unwrap();
        stream.extend_from_slice(&shifted[0]);
    }
    stream[PREPARED_HISTORY_FRAMES..PREPARED_HISTORY_FRAMES + frames].to_vec()
}

#[test]
fn productive_musical_worker_adopts_actual_native_and_copied_fifo_domain() {
    let rate = 48_000;
    let sample = source();
    for period in [1499.75, 1500.25] {
        for ratio in [0.73, 1.371_234_567_890_123] {
            let admitted_period = admitted_musical_period(rate, period);
            let expected = musical_raw_native_suffix(&sample, rate, period, ratio, 12_853);
            let mut mixer = musical_mixer(&sample, rate, period, ratio, true);
            render(&mut mixer, Some(0), 13);
            assert_eq!(
                voice(&mixer).source_playback.loop_period(),
                Some(admitted_period)
            );
            wait_ready(&mut mixer);
            let native = voice_mut(&mut mixer).stretch.prepared_native_address();
            assert_eq!(
                voice_mut(&mut mixer).stretch.prepared_target_output_frame(),
                Some(4096)
            );
            render(&mut mixer, Some(13), 4080);
            let join = render(&mut mixer, Some(4093), 37);
            assert_eq!(&join[3..], &expected[..34]);
            assert_eq!(voice(&mixer).stretch.native_state_address(), native);
            assert_eq!(voice(&mixer).stretch.adopted_request_id(), Some(1));
            let mut elapsed = 34;
            for requested in [1, 127, 384, 96, 257, 512, 31].into_iter().cycle() {
                if elapsed == expected.len() {
                    break;
                }
                let frames = requested.min(expected.len() - elapsed);
                let actual = render(&mut mixer, Some(4096 + elapsed as u64), frames);
                assert_eq!(actual, expected[elapsed..elapsed + frames]);
                elapsed += frames;
                let history = voice(&mixer).stretch.productive_history().unwrap();
                assert_eq!(
                    history.binding.accepted,
                    Some(musical_projection(rate, period))
                );
                assert_eq!(history.fed_output_frames, (4096 + elapsed) as u64);
                assert_eq!(
                    history.next_position,
                    voice(&mixer).source_playback.position()
                );
                assert_eq!(voice(&mixer).stretch.native_state_address(), native);
            }
            assert_eq!(mixer.pad_loop_start_frame[0], MUSICAL_START);
            assert_eq!(mixer.pad_loop_end_frame[0], Some(MUSICAL_END));
        }
    }
}

#[test]
fn finite_productive_musical_worker_adopts_actual_native_and_copied_fifo_domain() {
    let rate = 48_000;
    let full = source().with_complete_source(rate);
    let finite = full
        .window(
            MUSICAL_START,
            MUSICAL_END,
            2,
            ResidentContext::KeyLockFiniteLoop,
        )
        .unwrap();
    for period in [1499.75, 1500.25] {
        for ratio in [0.73, 1.371_234_567_890_123] {
            let expected = musical_raw_native_suffix(&full, rate, period, ratio, 12_853);
            let mut actual = finite_mixer(
                &finite,
                rate,
                musical_projection(rate, period),
                MUSICAL_START,
                MUSICAL_END,
                ratio,
            );
            let mut reference = musical_mixer(&full, rate, period, ratio, true);
            assert_eq!(
                render(&mut actual, Some(0), 13),
                render(&mut reference, Some(0), 13)
            );
            wait_ready(&mut actual);
            wait_ready(&mut reference);
            let prepared_reads = voice_mut(&mut actual)
                .stretch
                .prepared_tap_observation()
                .unwrap();
            assert_finite_taps(prepared_reads, MUSICAL_START, MUSICAL_END, 4096);
            let prepared = voice_mut(&mut actual).stretch.prepared_native_address();
            assert_ne!(prepared, voice(&actual).stretch.native_state_address());
            assert_eq!(
                voice_mut(&mut actual).stretch.prepared_fifo_frames(),
                Some((0, 511))
            );
            assert_eq!(
                voice_mut(&mut actual)
                    .stretch
                    .prepared_target_output_frame(),
                Some(4096)
            );
            assert_eq!(
                render(&mut actual, Some(13), 4080),
                render(&mut reference, Some(13), 4080)
            );
            let join = render(&mut actual, Some(4093), 37);
            assert_eq!(join, render(&mut reference, Some(4093), 37));
            assert_eq!(&join[3..], &expected[..34]);
            let mut elapsed = 34;
            for requested in [1, 127, 384, 96, 257, 512, 31].into_iter().cycle() {
                if elapsed == expected.len() {
                    break;
                }
                let frames = requested.min(expected.len() - elapsed);
                let output = render_finite(
                    &mut actual,
                    4096 + elapsed as u64,
                    frames,
                    MUSICAL_START,
                    MUSICAL_END,
                );
                assert_eq!(output, expected[elapsed..elapsed + frames]);
                assert_eq!(
                    output,
                    render(&mut reference, Some(4096 + elapsed as u64), frames)
                );
                elapsed += frames;
                let history = voice(&actual).stretch.productive_history().unwrap();
                assert_eq!(
                    history.binding.accepted,
                    Some(musical_projection(rate, period))
                );
                assert_eq!(history.fed_output_frames, (4096 + elapsed) as u64);
                assert_eq!(
                    history.next_position,
                    voice(&actual).source_playback.position()
                );
                assert_eq!(voice(&actual).stretch.native_state_address(), prepared);
                assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
                assert_eq!(
                    voice(&actual).stretch.native_tap_observation(),
                    Some(prepared_reads)
                );
                assert!(
                    voice(&actual)
                        .source_playback
                        .matches_exact(&voice(&reference).source_playback)
                );
            }
            assert!(expected.iter().any(|value| value.abs() > 0.02));
            assert_eq!(
                voice(&actual).sample.as_ref().unwrap().samples.len(),
                MUSICAL_END - MUSICAL_START
            );
        }
    }
}

#[test]
fn musical_full_stem_transition_and_continuous_filter_output_share_one_fractional_domain() {
    let sample = source();
    let ratio = 0.73;
    let prefix = 333;
    let total = 25_017;
    for rate in [44_100, 48_000, 96_000] {
        for period in [1499.75, 1500.25] {
            let admitted_period = admitted_musical_period(rate, period);
            let mut reference_output = None;
            for pattern in [&[512][..], &[1][..], &[1, 127, 384, 96, 257, 512, 31][..]] {
                let stems = PreparedStemSet {
                    complete_set_identity: std::sync::Arc::new([0; 32]),
                    reference_samples: sample.samples.clone(),
                    publication: PreparedSourcePermit::unrestricted(),
                    accepted_timing: None,
                    source_version_hash: 37,
                    sample_rate_hz: rate,
                    channels: 1,
                    frame_count: sample.samples.len(),
                    available_mask: ((1_u16 << crate::messages::STEM_BUFFER_COUNT) - 1) as u8,
                    stems: std::array::from_fn(|index| SampleBuffer {
                        residency: None,
                        channels: 1,
                        samples: sample
                            .samples
                            .iter()
                            .map(|value| *value * (index + 1) as f32 * 0.1)
                            .collect::<Vec<_>>()
                            .into(),
                    }),
                };
                let selected = SampleBuffer {
                    residency: None,
                    channels: 1,
                    samples: (0..sample.samples.len())
                        .map(|frame| stems.stems[1].samples[frame] + stems.stems[3].samples[frame])
                        .collect::<Vec<_>>()
                        .into(),
                };
                let mut mixer =
                    musical_mixer_with_stems(&sample, rate, period, ratio, false, Some(stems));
                assert_eq!(
                    mixer.prepared_stems[0].as_ref().unwrap().accepted_timing,
                    Some(musical_projection(rate, period))
                );
                mixer.stop_sample(0);
                mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
                assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
                let mut filter = PerPadDspChain::new(0, rate as f32, 1);
                for (slot, db) in [
                    (DspParameterSlot::Slot0, -12.0),
                    (DspParameterSlot::Slot1, -3.0),
                    (DspParameterSlot::Slot2, 4.0),
                ] {
                    assert!(filter.set_parameter(
                        DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
                        pad_eq_db_to_normalized(db),
                    ));
                }
                filter.reset();
                let mut elapsed = 0;
                let mut step = 0;
                let mut collected = Vec::with_capacity(total);
                while elapsed < total {
                    if elapsed == prefix {
                        assert!(mixer.set_stem_enabled_mask(0, 0b1010, 37));
                        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 37));
                    }
                    let boundary = if elapsed < prefix { prefix } else { total };
                    let frames = pattern[step % pattern.len()].min(boundary - elapsed);
                    let output = render(&mut mixer, Some(elapsed as u64), frames);
                    let feed = voice(&mixer).stretch.varispeed_buffers();
                    for offset in 0..frames {
                        let frame = elapsed + offset;
                        let distance = frame as f64 * ratio;
                        let to_gain = if frame < prefix {
                            0.0
                        } else {
                            (((frame - prefix) as f64 * ratio) / 128.0).min(1.0) as f32
                        };
                        // The crossfade belongs to each physical source knot before the
                        // final interpolation. Preserve that binary32 operation order:
                        // mixing separately interpolated streams introduces different
                        // rounding which recursive filter state can later amplify.
                        let expected_feed =
                            musical_sample_from_knots(admitted_period, distance, |knot| {
                                sample.samples[knot] * (1.0 - to_gain)
                                    + selected.samples[knot] * to_gain
                            });
                        assert_eq!(
                            feed[0][offset], expected_feed,
                            "full/stem same-domain rate={rate} P={period} frame={frame}"
                        );
                        // This independent continuously running filter is never rebound/reset
                        // at loop wraps, virtual physical-end phases or the stem transition.
                        filter.begin_frame();
                        let expected_output = filter.process_sample(0, expected_feed);
                        assert_eq!(
                            output[offset], expected_output,
                            "filter continuity rate={rate} P={period} frame={frame}"
                        );
                    }
                    collected.extend_from_slice(&output);
                    elapsed += frames;
                    step += 1;
                }
                if let Some(reference) = &reference_output {
                    assert_eq!(&collected, reference);
                } else {
                    reference_output = Some(collected);
                }
                assert_eq!(
                    voice(&mixer).source_playback.loop_period(),
                    Some(admitted_period)
                );
                assert_eq!(voice(&mixer).source_playback.tempo_ratio(), ratio);
                assert_eq!(mixer.pad_loop_start_frame[0], MUSICAL_START);
                assert_eq!(mixer.pad_loop_end_frame[0], Some(MUSICAL_END));
            }
        }
    }
}

#[test]
fn replacement_bank_cannot_relabel_pinned_musical_source_domain_or_actual_pcm() {
    let sample = source();
    let replacement = SampleBuffer {
        residency: None,
        channels: 1,
        samples: sample
            .samples
            .iter()
            .map(|value| *value * -0.47)
            .collect::<Vec<_>>()
            .into(),
    };
    let rate = 48_000;
    let old_period = 1500.25;
    let new_period = 1499.75;
    let admitted_old_period = admitted_musical_period(rate, old_period);
    let admitted_new_period = admitted_musical_period(rate, new_period);
    let ratio = 0.73;
    let mut mixer = musical_mixer(&sample, rate, old_period, ratio, false);
    render(&mut mixer, Some(0), 2053);
    let old_position = voice(&mixer).source_playback.position();
    mixer
        .input_runtime_ownership
        .publish_source(0, &replacement, rate, 4);
    mixer.load_sample(0, replacement.clone());
    let publication = PreparedSourcePermit::unrestricted();
    publication.mark_pending().unwrap();
    let accepted = AcceptedTimingProjection {
        revision: [62; 32],
        publication_epoch: 18,
        ..musical_projection(rate, new_period)
    };
    assert!(mixer.publish_constant_timing_rt(
        0,
        PreparedConstantTiming {
            reference: replacement.clone(),
            publication,
            projection: accepted
        },
        &mut ImmediateAudioBufferRetirement,
    ));
    assert_eq!(voice(&mixer).source_playback.position(), old_position);
    assert!(Arc::ptr_eq(
        &voice(&mixer).sample.as_ref().unwrap().samples,
        &sample.samples
    ));
    let mut elapsed = 0;
    for frames in [1, 127, 384, 96, 257, 512, 31]
        .into_iter()
        .cycle()
        .take(100)
    {
        let output = render(&mut mixer, Some((2053 + elapsed) as u64), frames);
        let expected = (2053 + elapsed..2053 + elapsed + frames)
            .map(|frame| musical_sample(&sample, admitted_old_period, frame as f64 * ratio))
            .collect::<Vec<_>>();
        assert_eq!(output, expected);
        assert_eq!(
            voice(&mixer).source_playback.loop_period(),
            Some(admitted_old_period)
        );
        assert_eq!(
            voice(&mixer).source_timing.accepted,
            Some(musical_projection(rate, old_period))
        );
        elapsed += frames;
    }
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 25_000));
    let output = render(&mut mixer, Some(25_000), 113);
    let expected = (0..113)
        .map(|frame| musical_sample(&replacement, admitted_new_period, frame as f64 * ratio))
        .collect::<Vec<_>>();
    assert_eq!(output, expected);
    assert_eq!(
        voice(&mixer).source_playback.loop_period(),
        Some(admitted_new_period)
    );
    assert_eq!(voice(&mixer).source_timing.accepted, Some(accepted));
}

#[test]
fn period_only_virtual_seam_refresh_and_clear_retain_actual_native_fifo_and_filter_chronology() {
    prove_virtual_seam_native_and_filter_chronology(false);
}

#[test]
fn direct_accepted_clear_at_virtual_seam_retains_actual_native_fifo_and_filter_chronology() {
    prove_virtual_seam_native_and_filter_chronology(true);
}

fn prove_virtual_seam_native_and_filter_chronology(direct_clear: bool) {
    let rate = 48_000;
    let sample = source();
    let ratio = if direct_clear { 0.625 } else { 1.25 };
    let old_period = admitted_musical_period(rate, 1500.25);
    let new_period = admitted_musical_period(rate, 1499.75);
    let refresh_frame = 7201;
    let clear_frame = if direct_clear { refresh_frame } else { 10_543 };
    let total = clear_frame + 12_853;
    let old_phase_at_refresh = (refresh_frame as f64 * ratio).rem_euclid(old_period);
    let physical_period = (MUSICAL_END - MUSICAL_START) as f64;
    assert_eq!(
        old_phase_at_refresh,
        if direct_clear { 1500.125 } else { 1500.0 }
    );
    let edited_period = if direct_clear {
        physical_period
    } else {
        new_period
    };
    let new_origin = old_phase_at_refresh.rem_euclid(edited_period);
    assert_eq!(new_origin, if direct_clear { 0.125 } else { 0.25 });
    let phase_at_clear = if direct_clear {
        new_origin
    } else {
        (new_origin + (clear_frame - refresh_frame) as f64 * ratio).rem_euclid(new_period)
    };

    // The raw native reference receives independently generated chronological PCM through
    // the declared ownership edit(s). It never resets native history, discards FIFO data or restarts
    // source phase. A filter reset at either edit would fail the continuous reference below.
    let source_at = |frame: usize| {
        let (period, phase) = if frame < refresh_frame {
            (old_period, frame as f64 * ratio)
        } else if frame < clear_frame {
            (
                new_period,
                new_origin + (frame - refresh_frame) as f64 * ratio,
            )
        } else {
            (
                physical_period,
                phase_at_clear + (frame - clear_frame) as f64 * ratio,
            )
        };
        musical_sample(&sample, period, phase)
    };
    let mut raw = RubberBandLiveShifter::new(rate, 1).unwrap();
    raw.prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
        .unwrap();
    let block = raw.block_size();
    let mut native_reference = vec![0.0; block - 1];
    let mut shifted = vec![vec![0.0; block]];
    for start in (0..total.div_ceil(block) * block).step_by(block) {
        let input = vec![(start..start + block).map(source_at).collect::<Vec<_>>()];
        raw.shift(&input, &mut shifted).unwrap();
        native_reference.extend_from_slice(&shifted[0]);
    }
    // A genuine native/adapter restart supplies a different waveform, so equality to the
    // chronological reference below cannot pass on an accidentally silent fixture.
    let mut restarted = RubberBandLiveShifter::new(rate, 1).unwrap();
    restarted
        .prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
        .unwrap();
    let reset_feed = vec![
        (refresh_frame..refresh_frame + block)
            .map(source_at)
            .collect::<Vec<_>>(),
    ];
    restarted.shift(&reset_feed, &mut shifted).unwrap();
    let mut reset_reference = vec![0.0; block - 1];
    reset_reference.extend_from_slice(&shifted[0]);
    assert_ne!(
        &native_reference[refresh_frame..refresh_frame + block],
        &reset_reference[..block]
    );

    let mut mixer = musical_mixer(&sample, rate, 1500.25, ratio, true);
    mixer.stop_sample(0);
    mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    let mut filter = PerPadDspChain::new(0, rate as f32, 1);
    for (slot, db) in [
        (DspParameterSlot::Slot0, -12.0),
        (DspParameterSlot::Slot1, -3.0),
        (DspParameterSlot::Slot2, 4.0),
    ] {
        assert!(filter.set_parameter(
            DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
            pad_eq_db_to_normalized(db),
        ));
    }
    filter.reset();
    let mut elapsed = 0;
    let mut step = 0;
    let mut adopted_native = 0;
    let mut binding = Some(musical_projection(rate, 1500.25));
    let changed = AcceptedTimingProjection {
        revision: [63; 32],
        publication_epoch: 18,
        ..musical_projection(rate, 1499.75)
    };
    while elapsed < total {
        if elapsed == 1 {
            wait_ready(&mut mixer);
            adopted_native = voice_mut(&mut mixer).stretch.prepared_native_address();
        }
        if elapsed == refresh_frame {
            let old = voice(&mixer).source_playback.position();
            assert_eq!(old.frame, MUSICAL_END);
            assert_eq!(old.fraction, old_phase_at_refresh.fract());
            if !direct_clear {
                let publication = PreparedSourcePermit::unrestricted();
                publication.mark_pending().unwrap();
                assert!(mixer.publish_constant_timing_rt(
                    0,
                    PreparedConstantTiming {
                        reference: sample.clone(),
                        publication,
                        projection: changed
                    },
                    &mut ImmediateAudioBufferRetirement,
                ));
                // AcceptedLoopRefresh also republishes these unchanged physical markers.
                // That control route must retain the old virtual seam phase until configure
                // admits the new period, rather than discarding the domain as an explicit seek.
                mixer.set_pad_loop_region(
                    0,
                    MUSICAL_START as f64 / f64::from(rate),
                    Some(MUSICAL_END as f64 / f64::from(rate)),
                );
                binding = Some(changed);
            }
            let mut restarted_filter = PerPadDspChain::new(0, rate as f32, 1);
            for (slot, db) in [
                (DspParameterSlot::Slot0, -12.0),
                (DspParameterSlot::Slot1, -3.0),
                (DspParameterSlot::Slot2, 4.0),
            ] {
                assert!(restarted_filter.set_parameter(
                    DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
                    pad_eq_db_to_normalized(db),
                ));
            }
            restarted_filter.reset();
            let reset_output = restarted_filter.process_sample(0, native_reference[elapsed]);
            // Clone the reference so this negative control does not consume chronology.
            let mut retained_filter = filter.clone();
            retained_filter.begin_frame();
            assert_ne!(
                reset_output,
                retained_filter.process_sample(0, native_reference[elapsed])
            );
        }
        if elapsed == clear_frame {
            let through_epoch = if direct_clear {
                musical_projection(rate, 1500.25).publication_epoch
            } else {
                changed.publication_epoch
            };
            mixer.clear_constant_timing(0, through_epoch);
            binding = None;
        }
        let boundary = [1, 4096, refresh_frame, clear_frame, total]
            .into_iter()
            .find(|boundary| *boundary > elapsed)
            .unwrap();
        let frames = [1, 127, 384, 96, 257, 512, 31][step % 7].min(boundary - elapsed);
        let output = render(&mut mixer, Some(elapsed as u64), frames);
        if elapsed == 4096 {
            assert_eq!(voice(&mixer).stretch.native_state_address(), adopted_native);
            assert_eq!(voice(&mixer).stretch.adopted_request_id(), Some(1));
            // Isolate continuity of the actually adopted native/FIFO owner. Later accepted
            // edits may otherwise schedule another legitimate source-specific history owner;
            // the separate productive worker matrix proves those adoptions independently.
            voice(&mixer).stretch.fail_preparation_worker();
        }
        let upstream = &voice(&mixer).stretch.output_buffers()[0][..frames];
        if elapsed >= 4096 {
            assert_eq!(
                voice(&mixer).stretch.native_state_address(),
                adopted_native,
                "a different native owner entered the chronology fixture at {elapsed}"
            );
            assert_eq!(
                upstream,
                &native_reference[elapsed..elapsed + frames],
                "chronological native/FIFO PCM diverged at frame {elapsed}"
            );
            let history = voice(&mixer).stretch.productive_history().unwrap();
            assert_eq!(history.binding.accepted, binding);
            assert_eq!(history.fed_output_frames, (elapsed + frames) as u64);
        }
        for offset in 0..frames {
            // Initial live warmup is explicit actual native output. After prepared adoption,
            // every reference sample is independently produced by raw native chronology.
            let input = if elapsed < 4096 {
                upstream[offset]
            } else {
                native_reference[elapsed + offset]
            };
            filter.begin_frame();
            assert_eq!(
                output[offset],
                filter.process_sample(0, input),
                "continuous filter was reset at output frame {}",
                elapsed + offset
            );
        }
        if elapsed == refresh_frame {
            let expected_phase = (new_origin + frames as f64 * ratio).rem_euclid(edited_period);
            let actual = voice(&mixer).source_playback.position();
            assert_eq!(
                actual.frame,
                MUSICAL_START + expected_phase.floor() as usize
            );
            assert_eq!(actual.fraction, expected_phase.fract());
            assert_eq!(
                voice(&mixer).source_playback.loop_period(),
                if direct_clear { None } else { Some(new_period) }
            );
        }
        if elapsed == clear_frame {
            assert_eq!(voice(&mixer).source_playback.loop_period(), None);
        }
        elapsed += frames;
        step += 1;
    }
    assert_eq!(mixer.pad_loop_start_frame[0], MUSICAL_START);
    assert_eq!(mixer.pad_loop_end_frame[0], Some(MUSICAL_END));
}
