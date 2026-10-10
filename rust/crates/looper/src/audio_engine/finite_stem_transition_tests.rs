//! Finite wet selection ramps versus complete PCM, algebraic knots/raw native and continuous EQ.
use super::*;
use std::collections::VecDeque;

struct RawOracle {
    native: RubberBandLiveShifter,
    input: VecDeque<f32>,
    output: VecDeque<f32>,
}

impl RawOracle {
    fn warmed(rate: u32, ratio: f64) -> Self {
        let mut native = RubberBandLiveShifter::new(rate, 1).unwrap();
        native.prepare_for_reuse().unwrap();
        native
            .set_pitch_scale(pitch_scale_for_tempo_ratio(ratio))
            .unwrap();
        let output = VecDeque::from(vec![0.0; native.block_size() - 1]);
        Self {
            native,
            input: VecDeque::new(),
            output,
        }
    }

    fn new(rate: u32, ratio: f64) -> Self {
        let mut native = RubberBandLiveShifter::new(rate, 1).unwrap();
        native
            .prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
            .unwrap();
        let output = VecDeque::from(vec![0.0; native.block_size() - 1]);
        Self {
            native,
            input: VecDeque::new(),
            output,
        }
    }

    fn render(&mut self, feed: &[f32]) -> Vec<f32> {
        self.input.extend(feed.iter().copied());
        let block = self.native.block_size();
        while self.input.len() >= block {
            let input = vec![self.input.drain(..block).collect::<Vec<_>>()];
            let mut output = vec![vec![0.0; block]];
            self.native.shift(&input, &mut output).unwrap();
            self.output.extend(output[0].iter().copied());
        }
        assert!(self.output.len() >= feed.len());
        self.output.drain(..feed.len()).collect()
    }
}

fn components(full: &SampleBuffer, rate: u32) -> PreparedStemSet {
    PreparedStemSet {
        complete_set_identity: Arc::new([91; 32]),
        reference_samples: full.samples.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        accepted_timing: None,
        source_version_hash: 37,
        sample_rate_hz: rate,
        channels: 1,
        frame_count: full.frame_count(),
        available_mask: crate::audio_engine::source_reader::full_stem_available_mask(),
        stems: std::array::from_fn(|index| SampleBuffer {
            residency: full.residency.clone(),
            channels: 1,
            samples: (0..full.frame_count())
                .map(|frame| {
                    // Four independently recognizable waveforms, rather than scaled FullMix.
                    (frame as f32 * (0.031 + index as f32 * 0.017)).sin()
                        * (0.04 + index as f32 * 0.025)
                })
                .collect::<Vec<_>>()
                .into(),
        }),
    }
}

fn transition_mixer(
    sample: &SampleBuffer,
    stems: PreparedStemSet,
    rate: u32,
    period: f64,
    ratio: f64,
) -> RtMixer {
    // Genuine inactive native publication leaves FullMix selected with Some resident owner.
    let mut mixer = musical_mixer_with_stems(sample, rate, period, ratio, false, Some(stems));
    assert_eq!(mixer.stem_mix_mode[0], StemMixMode::FullMix);
    mixer.stop_sample(0);
    mixer.set_pad_key_lock(0, true);
    assert!(mixer.pad_key_lock_enabled[0]);
    mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

fn eq(rate: u32) -> PerPadDspChain {
    let mut filter = PerPadDspChain::new(0, rate as f32, 1);
    for (slot, db) in [
        (DspParameterSlot::Slot0, -12.0),
        (DspParameterSlot::Slot1, -3.0),
        (DspParameterSlot::Slot2, 4.0),
    ] {
        assert!(filter.set_parameter(
            DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
            pad_eq_db_to_normalized(db)
        ));
    }
    filter.reset();
    filter
}

fn knot(full: &SampleBuffer, stems: &PreparedStemSet, mask: Option<u8>, frame: usize) -> f32 {
    mask.map_or(full.samples[frame], |mask| {
        stems
            .stems
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) != 0)
            .fold(0.0, |sum, (_, stem)| sum + stem.samples[frame])
    })
}

#[test]
fn finite_wet_stem_ramps_and_adoptions_match_raw_native_and_chronological_filter() {
    let rate = 48_000;
    for period in [1499.75, 1500.25] {
        for ratio in [0.73, 1.371_234_567_890_123] {
            let full = source().with_complete_source(rate);
            let finite = full
                .window(
                    MUSICAL_START,
                    MUSICAL_END,
                    2,
                    ResidentContext::KeyLockFiniteLoop,
                )
                .unwrap();
            let stems = components(&full, rate);
            let finite_stems = stems.clone().window_for(&finite).unwrap();
            assert!(finite_stems.stems.iter().all(|s| s.samples.len() == 1500));
            let mut actual = transition_mixer(&finite, finite_stems, rate, period, ratio);
            let mut reference = transition_mixer(&full, stems.clone(), rate, period, ratio);
            let mut raw = RawOracle::warmed(rate, ratio);
            let mut filter = eq(rate);
            let period = admitted_musical_period(rate, period);
            let mut frame = 0;
            let mut target: Option<u8> = None;
            let mut from = None;
            let mut ramp_start = 0;
            let mut audible = false;
            let mut candidate = None;
            let mut candidate_address = 0;
            let mut adoption = usize::MAX;
            let mut adopted = 0;
            // Includes both directions, empty and nonempty masks and interruption at56 frames.
            for (event, selection) in [
                (333, Some(10)),
                (389, Some(1)),
                (721, Some(0)),
                (1901, None),
                (2401, Some(5)),
                (18_000, None),
                (30_000, None),
            ] {
                let boundary = event;
                while frame < boundary {
                    let request_start = if frame < 18_000 { 3000 } else { 19_000 };
                    let boundary = if frame < request_start {
                        boundary.min(request_start)
                    } else {
                        boundary
                    };
                    let frames = [1, 127, 384, 96, 257, 512, 31][frame % 7].min(boundary - frame);
                    let feed = (frame..frame + frames)
                        .map(|n| {
                            let gain = if from == target {
                                1.0
                            } else {
                                (((n - ramp_start) as f64 * ratio) / 128.0).min(1.0) as f32
                            };
                            musical_sample_from_knots(period, n as f64 * ratio, |k| {
                                if gain == 1.0 {
                                    knot(&full, &stems, target, k)
                                } else {
                                    knot(&full, &stems, from, k) * (1.0 - gain)
                                        + knot(&full, &stems, target, k) * gain
                                }
                            })
                        })
                        .collect::<Vec<_>>();
                    // Deliberately request only after the real live ramp has completed. Ramps
                    // retain one productive handle and FIFOs; each endpoint then adopts real work.
                    let timestamp = (frame >= request_start).then_some(frame as u64);
                    let native_before = voice(&actual).stretch.native_state_address();
                    reset_tap_observation_for_test();
                    let output = render(&mut actual, timestamp, frames);
                    assert_finite_taps(
                        tap_observation_for_test(),
                        MUSICAL_START,
                        MUSICAL_END,
                        frames,
                    );
                    assert_eq!(output, render(&mut reference, timestamp, frames));
                    if !(frame < adoption && frame + frames > adoption) {
                        let observed = voice(&actual).stretch.varispeed_buffers();
                        for (read, expected) in observed[0][..frames].iter().zip(&feed) {
                            assert_eq!(
                                read, expected,
                                "feed P={period} ratio={ratio} frame={frame}"
                            );
                        }
                    }
                    let raw_output = if frame + frames > adoption {
                        let prefix = adoption.saturating_sub(frame);
                        let mut output = raw.render(&feed[..prefix]);
                        raw = candidate.take().unwrap();
                        output.extend(raw.render(&feed[prefix..]));
                        assert_eq!(
                            voice(&actual).stretch.native_state_address(),
                            candidate_address
                        );
                        adopted += 1;
                        adoption = usize::MAX;
                        output
                    } else {
                        raw.render(&feed)
                    };
                    for (read, expected) in output.iter().zip(raw_output) {
                        filter.begin_frame();
                        let expected = filter.process_sample(0, expected);
                        assert_eq!(
                            *read, expected,
                            "raw/filter P={period} ratio={ratio} frame={frame}"
                        );
                    }
                    audible |= output.iter().any(|s| s.abs() > 0.005);
                    if frame < request_start {
                        assert_eq!(voice(&actual).stretch.native_state_address(), native_before);
                        assert!(!voice_mut(&mut actual).stretch.source_preparation_ready());
                    }
                    if frame == request_start {
                        wait_ready(&mut actual);
                        wait_ready(&mut reference);
                        adoption = request_start + PREPARED_HISTORY_FRAMES;
                        assert_eq!(
                            voice_mut(&mut actual)
                                .stretch
                                .prepared_target_output_frame(),
                            Some(adoption as u64)
                        );
                        candidate_address =
                            voice_mut(&mut actual).stretch.prepared_native_address();
                        assert_ne!(candidate_address, native_before);
                        assert_finite_taps(
                            voice_mut(&mut actual)
                                .stretch
                                .prepared_tap_observation()
                                .unwrap(),
                            MUSICAL_START,
                            MUSICAL_END,
                            PREPARED_HISTORY_FRAMES,
                        );
                        let mut prepared = RawOracle::new(rate, ratio);
                        let input = (request_start..adoption)
                            .map(|n| {
                                musical_sample_from_knots(period, n as f64 * ratio, |k| {
                                    knot(&full, &stems, target, k)
                                })
                            })
                            .collect::<Vec<_>>();
                        prepared.render(&input);
                        candidate = Some(prepared);
                    }
                    assert!(
                        voice(&actual)
                            .source_playback
                            .matches_exact(&voice(&reference).source_playback)
                    );
                    assert_eq!(
                        voice(&actual).stretch.pending_fifo_frames(),
                        voice(&reference).stretch.pending_fifo_frames()
                    );
                    frame += frames;
                }
                from = target;
                target = selection;
                ramp_start = frame;
                for mixer in [&mut actual, &mut reference] {
                    if let Some(mask) = selection {
                        assert!(mixer.set_stem_enabled_mask(0, mask, 37));
                        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 37));
                    } else {
                        assert!(mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0));
                    }
                }
            }
            assert!(audible);
            assert_eq!(adopted, 2);
        }
    }
}

#[test]
fn finite_stem_selection_fences_stale_candidate_and_adopts_while_rate_still_smooths() {
    let rate = 48_000;
    for period in [1499.75, 1500.25] {
        let full = source().with_complete_source(rate);
        let finite = full
            .window(
                MUSICAL_START,
                MUSICAL_END,
                2,
                ResidentContext::KeyLockFiniteLoop,
            )
            .unwrap();
        let stems = components(&full, rate);
        let mut actual = transition_mixer(
            &finite,
            stems.clone().window_for(&finite).unwrap(),
            rate,
            period,
            0.73,
        );
        let mut reference = transition_mixer(&full, stems, rate, period, 0.73);
        assert_eq!(
            render(&mut actual, Some(0), 13),
            render(&mut reference, Some(0), 13)
        );
        wait_ready(&mut actual);
        let stale = voice_mut(&mut actual).stretch.prepared_native_address();
        let live = voice(&actual).stretch.native_state_address();
        for mixer in [&mut actual, &mut reference] {
            assert!(!mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 99));
            assert!(!mixer.set_stem_enabled_mask(0, 16, 37));
            assert!(!mixer.set_stem_enabled_mask(0, 1, 99));
            assert_eq!(mixer.stem_mix_mode[0], StemMixMode::FullMix);
            assert!(mixer.set_stem_enabled_mask(0, 10, 37));
            assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 37));
            mixer.set_speed(1.97);
        }
        let mut frame = 13;
        for frames in [1, 31, 127, 96] {
            let output = render(&mut actual, None, frames);
            assert_eq!(output, render(&mut reference, None, frames));
            frame += frames as u64;
            assert_eq!(voice(&actual).stretch.native_state_address(), live);
            assert_ne!(voice(&actual).stretch.native_state_address(), stale);
            assert!(
                voice(&actual)
                    .source_playback
                    .matches_exact(&voice(&reference).source_playback)
            );
        }
        assert!(!actual.stem_transitions[0].is_active());
        assert_eq!(
            render(&mut actual, Some(frame), 1),
            render(&mut reference, Some(frame), 1)
        );
        wait_ready(&mut actual);
        wait_ready(&mut reference);
        let target = voice_mut(&mut actual)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        let candidate = voice_mut(&mut actual).stretch.prepared_native_address();
        let prepared_taps = voice_mut(&mut actual)
            .stretch
            .prepared_tap_observation()
            .unwrap();
        assert_finite_taps(prepared_taps, MUSICAL_START, MUSICAL_END, 4096);
        frame += 1;
        while frame < target - 3 {
            let frames = [1, 127, 384, 96, 257, 512, 31][frame as usize % 7]
                .min((target - 3 - frame) as usize);
            assert_eq!(
                render(&mut actual, Some(frame), frames),
                render(&mut reference, Some(frame), frames)
            );
            frame += frames as u64;
            assert_eq!(voice(&actual).stretch.native_state_address(), live);
        }
        let output = render(&mut actual, Some(frame), 37);
        assert_eq!(output, render(&mut reference, Some(frame), 37));
        assert!(output.iter().any(|s| s.abs() > 0.001));
        assert_eq!(voice(&actual).stretch.native_state_address(), candidate);
        assert_ne!(voice(&actual).source_playback.tempo_ratio(), 1.97);
        assert_eq!(
            voice(&actual).stretch.pending_fifo_frames(),
            voice(&reference).stretch.pending_fifo_frames()
        );
        assert!(
            voice(&actual)
                .source_playback
                .matches_exact(&voice(&reference).source_playback)
        );
    }
}

#[test]
fn finite_stem_commands_require_current_source_and_timing_ack_without_resetting_audio() {
    let rate = 48_000;
    for revoke_source in [false, true] {
        let full = source().with_complete_source(rate);
        let finite = full
            .window(
                MUSICAL_START,
                MUSICAL_END,
                2,
                ResidentContext::KeyLockFiniteLoop,
            )
            .unwrap();
        let mut actual = transition_mixer(
            &finite,
            components(&full, rate).window_for(&finite).unwrap(),
            rate,
            1500.25,
            0.73,
        );
        render(&mut actual, Some(0), 13);
        wait_ready(&mut actual);
        let native = voice(&actual).stretch.native_state_address();
        let fifo = voice(&actual).stretch.pending_fifo_frames();
        if revoke_source {
            actual.input_runtime_ownership.revoke_source(0);
        } else {
            actual.current_timing_acknowledgements.clear(0);
        }
        assert!(!actual.set_stem_mix_mode(0, StemMixMode::AllStems, 37));
        assert_eq!(actual.stem_mix_mode[0], StemMixMode::FullMix);
        assert_eq!(voice(&actual).stretch.native_state_address(), native);
        assert_eq!(voice(&actual).stretch.pending_fifo_frames(), fifo);
        let output = render(&mut actual, Some(13), 5000);
        assert!(output.iter().any(|s| s.abs() > 0.001));
        assert_eq!(voice(&actual).stretch.native_state_address(), native);
        assert!(voice(&actual).stretch.adopted_request_id().is_none());
    }
}
