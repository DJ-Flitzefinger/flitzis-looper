//! Finite lifecycle output against complete PCM, algebraic source knots, raw native and EQ.
use super::*;
use std::collections::VecDeque;

const LIFECYCLE_RATE: u32 = 48_000;

/// An algebraic piecewise source clock. It never calls SourcePlayback or SourceReadPlan.
/// Active output time is independent of the absolute callback clock (including paused gaps).
#[derive(Clone)]
pub(crate) struct AlgebraicClock {
    whole: usize,
    fraction: f64,
    elapsed: usize,
    period: f64,
    ratio: f64,
    target: f64,
    until_step: usize,
    transition: Option<(SampleBuffer, f64)>,
    physical_region: Option<(usize, usize)>,
}

impl AlgebraicClock {
    pub(crate) fn ratio_for_test(&self) -> f64 {
        self.ratio
    }
    pub(crate) fn new(period: f64, ratio: f64) -> Self {
        Self {
            whole: 0,
            fraction: 0.0,
            elapsed: 0,
            period,
            ratio,
            target: ratio,
            until_step: 0,
            transition: None,
            physical_region: None,
        }
    }

    pub(crate) fn physical(start: usize, end: usize, ratio: f64) -> Self {
        Self {
            physical_region: Some((start, end)),
            ..Self::new((end - start) as f64, ratio)
        }
    }

    pub(crate) fn phase(&self, offset: usize) -> f64 {
        (self.whole as f64 + (self.fraction + (self.elapsed + offset) as f64 * self.ratio))
            .rem_euclid(self.period)
    }

    fn rebase(&mut self) {
        let phase = self.phase(0);
        self.whole = phase.floor() as usize;
        self.fraction = phase.fract();
        self.elapsed = 0;
    }

    pub(crate) fn set_target(&mut self, target: f64) {
        self.target = target;
        self.until_step = 0;
    }

    fn set_period(&mut self, period: f64) {
        self.rebase();
        self.period = period;
    }

    pub(crate) fn begin_chunk(&mut self, maximum: usize) -> usize {
        if self.until_step == 0 && self.ratio != self.target {
            self.rebase();
            self.ratio = if (self.target - self.ratio).abs() <= 0.05 {
                self.target
            } else {
                self.ratio + (self.target - self.ratio).clamp(-0.05, 0.05)
            };
            self.until_step = 512;
        }
        if self.ratio == self.target {
            maximum
        } else {
            maximum.min(self.until_step)
        }
    }

    pub(crate) fn advance(&mut self, frames: usize) {
        self.elapsed += frames;
        self.until_step = self.until_step.saturating_sub(frames);
        if let Some((_, consumed)) = &mut self.transition {
            *consumed += frames as f64 * self.ratio;
            if *consumed >= 128.0 {
                self.transition = None;
            }
        }
    }

    fn sample(&self, target: &SampleBuffer, offset: usize) -> f32 {
        if let Some((start, end)) = self.physical_region {
            let phase = self.phase(offset);
            let left = start + phase.floor() as usize;
            let right = if left + 1 == end { start } else { left + 1 };
            let fraction = phase.fract() as f32;
            return target.samples[left]
                + (target.samples[right] - target.samples[left]) * fraction;
        }
        musical_sample_from_knots(self.period, self.phase(offset), |frame| {
            self.transition
                .as_ref()
                .map_or(target.samples[frame], |(outgoing, consumed)| {
                    let gain = ((*consumed + offset as f64 * self.ratio) / 128.0).min(1.0) as f32;
                    outgoing.samples[frame] * (1.0 - gain) + target.samples[frame] * gain
                })
        })
    }

    pub(crate) fn dry(&mut self, source: &SampleBuffer, frames: usize) -> Vec<f32> {
        let mut result = Vec::with_capacity(frames);
        while result.len() < frames {
            let count = self.begin_chunk(frames - result.len());
            result.extend((0..count).map(|offset| self.sample(source, offset)));
            self.advance(count);
        }
        result
    }
}

pub(crate) struct RawChronology {
    native: RubberBandLiveShifter,
    input: VecDeque<f32>,
    output: VecDeque<f32>,
    pitch: f64,
    pub(crate) used: bool,
}

impl RawChronology {
    pub(crate) fn block_size(&self) -> usize {
        self.native.block_size()
    }
    pub(crate) fn new(ratio: f64, prepared: bool) -> Self {
        let mut native = RubberBandLiveShifter::new(LIFECYCLE_RATE, 1).unwrap();
        let pitch = pitch_scale_for_tempo_ratio(ratio);
        if prepared {
            native.prepare_exact_pitch(pitch).unwrap();
        } else {
            native.prepare_for_reuse().unwrap();
            native.set_pitch_scale(pitch).unwrap();
        }
        let output = VecDeque::from(vec![0.0; native.block_size() - 1]);
        Self {
            native,
            input: VecDeque::new(),
            output,
            pitch,
            used: false,
        }
    }

    pub(crate) fn render(
        &mut self,
        source: &SampleBuffer,
        clock: &mut AlgebraicClock,
        frames: usize,
    ) -> Vec<f32> {
        let mut result = Vec::with_capacity(frames);
        while result.len() < frames {
            let count = clock.begin_chunk(frames - result.len());
            let pitch = pitch_scale_for_tempo_ratio(clock.ratio);
            if (pitch - self.pitch).abs() > 0.001 {
                self.native.set_pitch_scale(pitch).unwrap();
                self.pitch = pitch;
            }
            self.input
                .extend((0..count).map(|offset| clock.sample(source, offset)));
            let block = self.native.block_size();
            while self.input.len() >= block {
                let input = vec![self.input.drain(..block).collect::<Vec<_>>()];
                let mut output = vec![vec![0.0; block]];
                self.native.shift(&input, &mut output).unwrap();
                self.used = true;
                self.output.extend(output[0].iter().copied());
            }
            assert!(self.output.len() >= count);
            result.extend(self.output.drain(..count));
            clock.advance(count);
        }
        result
    }
}

/// Applying EQ after a live source cut starts its ordinary parameter smoothing from neutral.
pub(crate) fn lifecycle_filter_after_cut() -> PerPadDspChain {
    let mut filter = PerPadDspChain::new(0, LIFECYCLE_RATE as f32, 1);
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
    filter
}

fn lifecycle_filter() -> PerPadDspChain {
    let mut filter = lifecycle_filter_after_cut();
    filter.reset();
    filter
}

fn lifecycle_mixer(sample: &SampleBuffer, period: f64, ratio: f64) -> RtMixer {
    // Establish the finite physical domain before enabling its guarded KEYLOCK context.
    let mut mixer = musical_mixer(sample, LIFECYCLE_RATE, period, ratio, false);
    mixer.stop_sample(0);
    mixer.set_pad_key_lock(0, true);
    assert!(mixer.pad_key_lock_enabled[0]);
    mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

struct Checkpoint {
    target: u64,
    address: usize,
    raw: RawChronology,
}

struct LifecycleOracle {
    actual: RtMixer,
    complete: RtMixer,
    source: SampleBuffer,
    clock: AlgebraicClock,
    raw: RawChronology,
    filter: PerPadDspChain,
    checkpoint: Option<Checkpoint>,
    output_frame: u64,
    adoptions: usize,
    audible: bool,
    wet: bool,
    dry_warm_remaining: usize,
}

impl LifecycleOracle {
    fn new(period: f64, ratio: f64) -> Self {
        let source = source().with_complete_source(LIFECYCLE_RATE);
        let finite = source
            .window(
                MUSICAL_START,
                MUSICAL_END,
                2,
                ResidentContext::KeyLockFiniteLoop,
            )
            .unwrap();
        Self {
            actual: lifecycle_mixer(&finite, period, ratio),
            complete: lifecycle_mixer(&source, period, ratio),
            source,
            clock: AlgebraicClock::new(admitted_musical_period(LIFECYCLE_RATE, period), ratio),
            raw: RawChronology::new(ratio, false),
            filter: lifecycle_filter(),
            checkpoint: None,
            output_frame: 0,
            adoptions: 0,
            audible: false,
            wet: true,
            dry_warm_remaining: 0,
        }
    }

    fn selected_stems(period: f64, ratio: f64) -> Self {
        Self::resident_stems(period, ratio, true)
    }

    fn resident_stems(period: f64, ratio: f64, selected: bool) -> Self {
        let original = source().with_complete_source(LIFECYCLE_RATE);
        let finite = original
            .window(
                MUSICAL_START,
                MUSICAL_END,
                2,
                ResidentContext::KeyLockFiniteLoop,
            )
            .unwrap();
        let stems = super::finite_stem_transition_tests::components(&original, LIFECYCLE_RATE);
        let mut actual = super::finite_stem_transition_tests::transition_mixer(
            &finite,
            stems.clone().window_for(&finite).unwrap(),
            LIFECYCLE_RATE,
            period,
            ratio,
        );
        let mut complete = super::finite_stem_transition_tests::transition_mixer(
            &original,
            stems.clone(),
            LIFECYCLE_RATE,
            period,
            ratio,
        );
        for mixer in [&mut actual, &mut complete] {
            mixer.stop_sample(0);
            if selected {
                assert!(mixer.set_stem_enabled_mask(0, 10, 37));
                assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 37));
            }
            assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
        }
        // Independent complete algebraic component sum supplies Raw Native, not a resident view.
        let source = if selected {
            SampleBuffer {
                residency: None,
                channels: 1,
                samples: (0..original.frame_count())
                    .map(|frame| stems.stems[1].samples[frame] + stems.stems[3].samples[frame])
                    .collect::<Vec<_>>()
                    .into(),
            }
        } else {
            original
        };
        Self {
            actual,
            complete,
            source,
            clock: AlgebraicClock::new(admitted_musical_period(LIFECYCLE_RATE, period), ratio),
            raw: RawChronology::new(ratio, false),
            filter: lifecycle_filter(),
            checkpoint: None,
            output_frame: 0,
            adoptions: 0,
            audible: false,
            wet: true,
            dry_warm_remaining: 0,
        }
    }

    fn render(&mut self, frames: usize) {
        reset_tap_observation_for_test();
        let actual = render(&mut self.actual, Some(self.output_frame), frames);
        assert_eq!(self.actual.pad_key_lock_enabled[0], self.wet);
        assert_eq!(
            voice(&self.actual).stretch.productive_history().is_some(),
            self.wet,
            "finite lifecycle took a dry path at {}",
            self.output_frame
        );
        assert_finite_taps(
            tap_observation_for_test(),
            MUSICAL_START,
            MUSICAL_END,
            frames,
        );
        assert_eq!(
            actual,
            render(&mut self.complete, Some(self.output_frame), frames),
            "complete PCM at {}",
            self.output_frame
        );
        let mut dry_clock = self.clock.clone();
        let dry = dry_clock.dry(&self.source, frames);
        let prefix = self.checkpoint.as_ref().map_or(frames, |checkpoint| {
            checkpoint
                .target
                .saturating_sub(self.output_frame)
                .min(frames as u64) as usize
        });
        let mut expected = if self.wet {
            self.raw.render(&self.source, &mut self.clock, prefix)
        } else {
            self.clock.dry(&self.source, prefix)
        };
        if prefix < frames {
            let checkpoint = self.checkpoint.take().unwrap();
            self.raw = checkpoint.raw;
            expected.extend(
                self.raw
                    .render(&self.source, &mut self.clock, frames - prefix),
            );
            assert_eq!(
                voice(&self.actual).stretch.native_state_address(),
                checkpoint.address
            );
            self.adoptions += 1;
        }
        let warm = frames.min(self.dry_warm_remaining);
        expected[..warm].copy_from_slice(&dry[..warm]);
        self.dry_warm_remaining -= warm;
        for (offset, (actual, upstream)) in actual.iter().zip(expected).enumerate() {
            self.filter.begin_frame();
            let expected = self.filter.process_sample(0, upstream);
            assert!(
                (*actual - expected).abs() <= 5.0e-5,
                "raw/EQ {}: {actual} != {expected}",
                self.output_frame + offset as u64
            );
        }
        self.audible |= actual.iter().any(|sample| sample.abs() > 0.005);
        assert!(
            voice(&self.actual)
                .source_playback
                .matches_exact(&voice(&self.complete).source_playback)
        );
        assert_eq!(
            voice(&self.actual).stretch.pending_fifo_frames(),
            voice(&self.complete).stretch.pending_fifo_frames()
        );
        let actual_phase = voice(&self.actual).source_playback.position();
        let phase = self.clock.phase(0);
        assert!(
            (actual_phase.frame as f64 - MUSICAL_START as f64 + actual_phase.fraction - phase)
                .abs()
                < 1.0e-8
        );
        self.output_frame += frames as u64;
    }

    fn request(&mut self) -> usize {
        assert!(self.wet);
        assert!(self.checkpoint.is_none());
        self.clock.begin_chunk(1);
        let mut prepared_clock = self.clock.clone();
        let mut raw = RawChronology::new(self.clock.ratio, true);
        raw.render(&self.source, &mut prepared_clock, PREPARED_HISTORY_FRAMES);
        let start = self.output_frame;
        self.render(1);
        wait_ready(&mut self.actual);
        wait_ready(&mut self.complete);
        let target = voice_mut(&mut self.actual)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        assert_eq!(target, start + PREPARED_HISTORY_FRAMES as u64);
        let address = voice_mut(&mut self.actual)
            .stretch
            .prepared_native_address();
        assert_finite_taps(
            voice_mut(&mut self.actual)
                .stretch
                .prepared_tap_observation()
                .unwrap(),
            MUSICAL_START,
            MUSICAL_END,
            PREPARED_HISTORY_FRAMES,
        );
        assert_ne!(address, voice(&self.actual).stretch.native_state_address());
        self.checkpoint = Some(Checkpoint {
            target,
            address,
            raw,
        });
        address
    }

    fn mode(&mut self, enabled: bool) {
        let position = voice(&self.actual).source_playback.clone();
        let generation = voice(&self.actual).generation;
        let region = voice(&self.actual).source_loop_region;
        for mixer in [&mut self.actual, &mut self.complete] {
            mixer.set_pad_key_lock(0, enabled);
            assert_eq!(mixer.pad_key_lock_enabled[0], enabled);
        }
        assert!(voice(&self.actual).source_playback.matches_exact(&position));
        assert_eq!(voice(&self.actual).generation, generation);
        assert_eq!(voice(&self.actual).source_loop_region, region);
        self.checkpoint = None;
        if enabled && !self.wet {
            self.raw = RawChronology::new(self.clock.ratio, false);
            self.dry_warm_remaining = self.raw.block_size();
        } else if !enabled {
            self.dry_warm_remaining = 0;
        }
        self.wet = enabled;
    }

    fn through_checkpoint(&mut self) {
        let target = self.checkpoint.as_ref().unwrap().target;
        let mut step = 0;
        while self.output_frame < target + 777 {
            self.render(
                [1, 127, 384, 96, 257, 512, 31][step % 7]
                    .min((target + 777 - self.output_frame) as usize),
            );
            step += 1;
        }
    }

    fn pause_gap(&mut self, frames: usize) {
        let position = voice(&self.actual).source_playback.position();
        let native = voice(&self.actual).stretch.native_state_address();
        let fifo = voice(&self.actual).stretch.pending_fifo_frames();
        let history = voice(&self.actual).stretch.productive_history().unwrap();
        for mixer in [&mut self.actual, &mut self.complete] {
            mixer.pause_sample_at_output_frame(0, self.output_frame);
        }
        self.checkpoint = None; // A captured candidate never acquires authority from resume.
        for mixer in [&mut self.actual, &mut self.complete] {
            assert!(
                render(mixer, Some(self.output_frame), frames)
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
        }
        assert_eq!(voice(&self.actual).source_playback.position(), position);
        assert_eq!(voice(&self.actual).stretch.native_state_address(), native);
        assert_eq!(voice(&self.actual).stretch.pending_fifo_frames(), fifo);
        assert_eq!(
            voice(&self.actual)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            history.fed_output_frames
        );
        self.output_frame += frames as u64;
        for mixer in [&mut self.actual, &mut self.complete] {
            mixer.resume_sample_at_output_frame(0, self.output_frame);
        }
    }
}

#[test]
fn finite_pause_before_and_after_preparation_keeps_wet_chronology_through_new_rate_adoptions() {
    for period in [1499.75, 1500.25] {
        let mut oracle = LifecycleOracle::new(period, 0.73);
        let stale = oracle.request();
        let live = voice(&oracle.actual).stretch.native_state_address();
        oracle.pause_gap(257);
        let resumed = oracle.request();
        assert_ne!(resumed, live);
        assert_ne!(stale, live);
        assert_eq!(voice(&oracle.actual).stretch.native_state_address(), live);
        oracle.through_checkpoint();
        let adopted = voice(&oracle.actual).stretch.native_state_address();
        oracle.pause_gap(97);
        assert_eq!(
            voice(&oracle.actual).stretch.native_state_address(),
            adopted
        );
        oracle.request();
        oracle.through_checkpoint();
        for mixer in [&mut oracle.actual, &mut oracle.complete] {
            mixer.set_speed(1.97);
        }
        oracle.clock.set_target(1.97);
        oracle.request();
        oracle.through_checkpoint();
        assert_ne!(voice(&oracle.actual).source_playback.tempo_ratio(), 1.97);
        let ramping_ratio = voice(&oracle.actual).source_playback.tempo_ratio();
        oracle.pause_gap(389);
        assert_eq!(
            voice(&oracle.actual).source_playback.tempo_ratio(),
            ramping_ratio
        );
        oracle.request();
        oracle.through_checkpoint();
        for frames in [31, 1, 512, 127, 384, 96, 257].into_iter().cycle().take(60) {
            oracle.render(frames);
        }
        assert_eq!(voice(&oracle.actual).source_playback.tempo_ratio(), 1.97);
        assert_eq!(oracle.adoptions, 4);
        assert!(oracle.audible);
    }
}

#[test]
fn finite_live_modes_selected_stems_bpm_smoothing_and_retained_ramps_keep_raw_eq_chronology() {
    for period in [1499.75, 1500.25] {
        for selected in [false, true] {
            for retained in [false, true] {
                let mut oracle = LifecycleOracle::resident_stems(period, 0.73, selected);
                oracle.mode(false);
                oracle.render(77);
                let source_period = musical_projection(LIFECYCLE_RATE, period).period_seconds;
                let master_period = source_period / 0.73;
                for mixer in [&mut oracle.actual, &mut oracle.complete] {
                    mixer.set_bpm_lock(true);
                    mixer.set_master_period(master_period);
                }
                oracle.clock.set_target(source_period / master_period);
                // Reverse the selected committed four-component mode. Both current ramp
                // sides remain real inputs when KEYLOCK is enabled partway through it.
                let outgoing = oracle.source.clone();
                let target = if selected {
                    oracle.complete.sample_bank[0].as_ref().unwrap().clone()
                } else {
                    let stems = oracle.complete.prepared_stems[0].as_ref().unwrap();
                    SampleBuffer {
                        residency: None,
                        channels: 1,
                        samples: (0..stems.frame_count)
                            .map(|frame| {
                                stems.stems[1].samples[frame] + stems.stems[3].samples[frame]
                            })
                            .collect::<Vec<_>>()
                            .into(),
                    }
                };
                for mixer in [&mut oracle.actual, &mut oracle.complete] {
                    if selected {
                        assert!(mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0));
                    } else {
                        assert!(mixer.set_stem_enabled_mask(0, 10, 37));
                        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 37));
                    }
                }
                oracle.source = target;
                oracle.clock.transition = Some((outgoing, 0.0));
                oracle.render(56);
                if retained {
                    let replacement = SampleBuffer {
                        residency: None,
                        channels: 1,
                        samples: vec![-0.23; 16_000].into(),
                    }
                    .with_complete_source(LIFECYCLE_RATE);
                    for mixer in [&mut oracle.actual, &mut oracle.complete] {
                        mixer.input_runtime_ownership.publish_source(
                            0,
                            &replacement,
                            LIFECYCLE_RATE,
                            4,
                        );
                        assert!(mixer.load_sample_rt(
                            0,
                            replacement.clone(),
                            &mut ImmediateAudioBufferRetirement
                        ));
                        assert!(
                            voice(mixer)
                                .frozen_stems
                                .as_ref()
                                .unwrap()
                                .transition
                                .is_active()
                        );
                    }
                }
                let generation = voice(&oracle.actual).generation;
                let original_source = voice(&oracle.actual)
                    .sample
                    .as_ref()
                    .unwrap()
                    .source_address();
                oracle.mode(true);
                for frames in [31, 40, 96] {
                    oracle.render(frames);
                }
                assert!(oracle.clock.transition.is_none());
                oracle.request();
                oracle.through_checkpoint();
                oracle.mode(false);
                oracle.render(257);
                let faster_master = master_period / 1.6;
                for mixer in [&mut oracle.actual, &mut oracle.complete] {
                    mixer.set_master_period(faster_master);
                    mixer.pause_sample_at_output_frame(0, oracle.output_frame);
                }
                oracle.clock.set_target(source_period / faster_master);
                oracle.mode(true);
                let frozen = voice(&oracle.actual).source_playback;
                for mixer in [&mut oracle.actual, &mut oracle.complete] {
                    assert!(
                        render(mixer, Some(oracle.output_frame), 389)
                            .iter()
                            .all(|sample| *sample == 0.0)
                    );
                }
                assert!(voice(&oracle.actual).source_playback.matches_exact(&frozen));
                oracle.output_frame += 389;
                for mixer in [&mut oracle.actual, &mut oracle.complete] {
                    mixer.resume_sample_at_output_frame(0, oracle.output_frame);
                }
                oracle.request();
                oracle.through_checkpoint();
                assert_eq!(voice(&oracle.actual).generation, generation);
                assert_eq!(
                    voice(&oracle.actual)
                        .sample
                        .as_ref()
                        .unwrap()
                        .source_address(),
                    original_source
                );
                assert_eq!(
                    voice(&oracle.actual).source_timing.accepted,
                    Some(musical_projection(LIFECYCLE_RATE, period))
                );
                assert_eq!(oracle.adoptions, 2);
                assert!(oracle.audible);
            }
        }
    }
}

fn publish_period(
    mixer: &mut RtMixer,
    sample: &SampleBuffer,
    projection: AcceptedTimingProjection,
) {
    let publication = PreparedSourcePermit::unrestricted();
    publication.mark_pending().unwrap();
    assert!(mixer.publish_constant_timing_rt(
        0,
        PreparedConstantTiming {
            reference: sample.clone(),
            publication,
            projection
        },
        &mut ImmediateAudioBufferRetirement
    ));
    mixer.set_pad_loop_region(
        0,
        MUSICAL_START as f64 / f64::from(LIFECYCLE_RATE),
        Some(MUSICAL_END as f64 / f64::from(LIFECYCLE_RATE)),
    );
}

#[test]
fn finite_period_refresh_and_clear_cross_virtual_seams_with_continuous_filter_and_later_native_work()
 {
    for direct_clear in [false, true] {
        let ratio = if direct_clear { 0.625 } else { 1.25 };
        let mut oracle = LifecycleOracle::new(1500.25, ratio);
        oracle.request();
        oracle.through_checkpoint();
        while oracle.output_frame < 7201 {
            oracle.render(127.min((7201 - oracle.output_frame) as usize));
        }
        assert_eq!(
            voice(&oracle.actual).source_playback.position().frame,
            MUSICAL_END
        );
        let retained_native = voice(&oracle.actual).stretch.native_state_address();
        let retained_fifo = voice(&oracle.actual).stretch.pending_fifo_frames();
        let projection = AcceptedTimingProjection {
            revision: [63; 32],
            publication_epoch: 18,
            ..musical_projection(LIFECYCLE_RATE, 1499.75)
        };
        for mixer in [&mut oracle.actual, &mut oracle.complete] {
            if direct_clear {
                mixer.clear_constant_timing(0, 17);
            } else {
                let source = voice(mixer).sample.as_ref().unwrap().clone();
                publish_period(mixer, &source, projection);
            }
        }
        assert_eq!(
            voice(&oracle.actual).stretch.native_state_address(),
            retained_native
        );
        assert_eq!(
            voice(&oracle.actual).stretch.pending_fifo_frames(),
            retained_fifo
        );
        oracle.clock.set_period(if direct_clear {
            1500.0
        } else {
            admitted_musical_period(LIFECYCLE_RATE, 1499.75)
        });
        oracle.request();
        oracle.through_checkpoint();
        if !direct_clear {
            for mixer in [&mut oracle.actual, &mut oracle.complete] {
                mixer.clear_constant_timing(0, 18);
            }
            oracle.clock.set_period(1500.0);
            oracle.request();
            oracle.through_checkpoint();
        }
        for frames in [1, 31, 257, 512, 127, 384, 96].into_iter().cycle().take(40) {
            oracle.render(frames);
        }
        assert_eq!(voice(&oracle.actual).source_playback.loop_period(), None);
        assert_eq!(oracle.adoptions, if direct_clear { 2 } else { 3 });
        assert!(oracle.audible);
    }
}

#[test]
fn finite_old_active_and_paused_voice_rejects_pre_bank_candidate_and_adopts_fresh_own_history() {
    for paused in [false, true] {
        let mut oracle = LifecycleOracle::selected_stems(1500.25, 0.73);
        let stale = oracle.request();
        let old_pin = Arc::downgrade(&voice(&oracle.actual).sample.as_ref().unwrap().samples);
        let old_source_address = voice(&oracle.actual)
            .sample
            .as_ref()
            .unwrap()
            .source_address();
        let live = voice(&oracle.actual).stretch.native_state_address();
        if paused {
            oracle.pause_gap(257);
        }
        let replacement = SampleBuffer {
            residency: None,
            channels: 1,
            samples: oracle
                .source
                .samples
                .iter()
                .map(|sample| sample * -0.47)
                .collect::<Vec<_>>()
                .into(),
        }
        .with_complete_source(LIFECYCLE_RATE);
        for mixer in [&mut oracle.actual, &mut oracle.complete] {
            if paused {
                mixer.pause_sample_at_output_frame(0, oracle.output_frame);
            }
            mixer
                .input_runtime_ownership
                .publish_source(0, &replacement, LIFECYCLE_RATE, 4);
            assert!(mixer.load_sample_rt(
                0,
                replacement.clone(),
                &mut ImmediateAudioBufferRetirement
            ));
            let projection = AcceptedTimingProjection {
                revision: [62; 32],
                publication_epoch: 18,
                ..musical_projection(LIFECYCLE_RATE, 1499.75)
            };
            publish_period(mixer, &replacement, projection);
            assert_eq!(
                voice(mixer).source_timing.accepted,
                Some(musical_projection(LIFECYCLE_RATE, 1500.25))
            );
            if paused {
                mixer.resume_sample_at_output_frame(0, oracle.output_frame);
            }
        }
        oracle.checkpoint = None;
        let fresh = oracle.request();
        assert_ne!(fresh, live);
        assert_ne!(stale, live);
        assert_eq!(voice(&oracle.actual).stretch.native_state_address(), live);
        assert!(old_pin.upgrade().is_some());
        oracle.through_checkpoint();
        assert_eq!(
            voice(&oracle.actual)
                .stretch
                .productive_history()
                .unwrap()
                .binding
                .source_address,
            old_source_address
        );
        assert_eq!(
            voice(&oracle.actual)
                .stretch
                .productive_history()
                .unwrap()
                .binding
                .accepted,
            Some(musical_projection(LIFECYCLE_RATE, 1500.25))
        );
        oracle.pause_gap(97);
        oracle.request();
        oracle.through_checkpoint();
        assert_eq!(oracle.adoptions, 2);
        assert!(oracle.audible);
        assert!(old_pin.upgrade().is_some());
        let frozen = voice(&oracle.actual).frozen_stems.as_ref().unwrap();
        assert!(frozen.set.is_some());
        let component_pins = frozen
            .set
            .as_ref()
            .unwrap()
            .stems
            .iter()
            .map(|stem| Arc::downgrade(&stem.samples))
            .collect::<Vec<_>>();
        assert!(component_pins.iter().all(|pin| pin.upgrade().is_some()));
        let (mut retirement, _worker) =
            crate::audio_engine::buffer_retirement::create_audio_buffer_retirement();
        oracle.actual.stop_sample_rt(0, &mut retirement);
        let deadline = Instant::now() + Duration::from_secs(5);
        while (old_pin.upgrade().is_some()
            || component_pins.iter().any(|pin| pin.upgrade().is_some()))
            && Instant::now() < deadline
        {
            render(&mut oracle.actual, Some(oracle.output_frame), 31);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(old_pin.upgrade().is_none());
        assert!(component_pins.iter().all(|pin| pin.upgrade().is_none()));
    }
}

#[test]
fn finite_old_voice_freezes_both_actual_transition_sides_across_active_and_paused_bank_replacement()
{
    for period in [1499.75, 1500.25] {
        for paused in [false, true] {
            for to_stems in [false, true] {
                let mut oracle = LifecycleOracle::resident_stems(period, 0.73, !to_stems);
                oracle.request();
                oracle.through_checkpoint();
                let native = voice(&oracle.actual).stretch.native_state_address();
                let old_address = voice(&oracle.actual)
                    .sample
                    .as_ref()
                    .unwrap()
                    .source_address();
                let outgoing = oracle.source.clone();
                let complete_source = oracle.complete.sample_bank[0].as_ref().unwrap();
                let target = if to_stems {
                    let stems = oracle.complete.prepared_stems[0].as_ref().unwrap();
                    SampleBuffer {
                        residency: None,
                        channels: 1,
                        samples: (0..complete_source.frame_count())
                            .map(|frame| {
                                stems.stems[1].samples[frame] + stems.stems[3].samples[frame]
                            })
                            .collect::<Vec<_>>()
                            .into(),
                    }
                } else {
                    complete_source.clone()
                };
                for mixer in [&mut oracle.actual, &mut oracle.complete] {
                    if to_stems {
                        assert!(mixer.set_stem_enabled_mask(0, 10, 37));
                        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 37));
                    } else {
                        assert!(mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0));
                    }
                }
                oracle.source = target;
                oracle.clock.transition = Some((outgoing, 0.0));
                oracle.render(56);
                assert_eq!(voice(&oracle.actual).stretch.native_state_address(), native);
                assert!(oracle.actual.stem_transitions[0].is_active());
                let replacement = SampleBuffer {
                    residency: None,
                    channels: 1,
                    samples: (0..16_000)
                        .map(|frame| (frame as f32 * 0.047).sin() * -0.31)
                        .collect::<Vec<_>>()
                        .into(),
                }
                .with_complete_source(LIFECYCLE_RATE);
                for mixer in [&mut oracle.actual, &mut oracle.complete] {
                    if paused {
                        mixer.pause_sample_at_output_frame(0, oracle.output_frame);
                    }
                    mixer.input_runtime_ownership.publish_source(
                        0,
                        &replacement,
                        LIFECYCLE_RATE,
                        4,
                    );
                    assert!(mixer.load_sample_rt(
                        0,
                        replacement.clone(),
                        &mut ImmediateAudioBufferRetirement
                    ));
                    let projection = AcceptedTimingProjection {
                        revision: [62; 32],
                        publication_epoch: 18,
                        ..musical_projection(LIFECYCLE_RATE, 1500.25)
                    };
                    publish_period(mixer, &replacement, projection);
                    let frozen = voice(mixer).frozen_stems.as_ref().unwrap();
                    assert!(frozen.set.is_some());
                    assert!(frozen.transition.is_active());
                    assert_eq!(
                        voice(mixer).source_timing.accepted,
                        Some(musical_projection(LIFECYCLE_RATE, period))
                    );
                }
                if paused {
                    let position = voice(&oracle.actual).source_playback.position();
                    let fifo = voice(&oracle.actual).stretch.pending_fifo_frames();
                    for mixer in [&mut oracle.actual, &mut oracle.complete] {
                        assert!(
                            render(mixer, Some(oracle.output_frame), 257)
                                .iter()
                                .all(|sample| *sample == 0.0)
                        );
                    }
                    assert_eq!(voice(&oracle.actual).source_playback.position(), position);
                    assert_eq!(voice(&oracle.actual).stretch.pending_fifo_frames(), fifo);
                    oracle.output_frame += 257;
                    for mixer in [&mut oracle.actual, &mut oracle.complete] {
                        mixer.resume_sample_at_output_frame(0, oracle.output_frame);
                    }
                }
                for frames in [1, 31, 48, 40] {
                    oracle.render(frames);
                    assert_eq!(voice(&oracle.actual).stretch.native_state_address(), native);
                }
                assert!(oracle.clock.transition.is_none());
                assert!(
                    !voice(&oracle.actual)
                        .frozen_stems
                        .as_ref()
                        .unwrap()
                        .transition
                        .is_active()
                );
                // The settled old A selection acquires fresh own-voice permits and real work.
                // B's new timing and source ACK never substitute for either transition side.
                oracle.request();
                oracle.through_checkpoint();
                assert_eq!(
                    voice(&oracle.actual)
                        .stretch
                        .productive_history()
                        .unwrap()
                        .binding
                        .source_address,
                    old_address
                );
                assert_eq!(
                    voice(&oracle.actual)
                        .stretch
                        .productive_history()
                        .unwrap()
                        .binding
                        .accepted,
                    Some(musical_projection(LIFECYCLE_RATE, period))
                );
                oracle.pause_gap(97);
                oracle.request();
                oracle.through_checkpoint();
                assert_eq!(oracle.adoptions, 3);
                assert!(oracle.audible);
            }
        }
    }
}
