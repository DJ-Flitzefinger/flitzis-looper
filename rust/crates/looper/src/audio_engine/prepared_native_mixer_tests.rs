//! Actual mixer adoption, deadline splitting and current native source/authority rejection.

use super::*;
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
use crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES;
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::rubberband_backend::{RubberBandLiveShifter, pitch_scale_for_tempo_ratio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const RATE: u32 = 8_000;

fn source() -> SampleBuffer {
    SampleBuffer {
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
        reference_samples: sample.samples.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        accepted_timing: Some(projection()),
        source_version_hash: 37,
        sample_rate_hz: RATE,
        channels: 1,
        frame_count: sample.samples.len(),
        available_mask: 31,
        stems: std::array::from_fn(|index| SampleBuffer {
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
