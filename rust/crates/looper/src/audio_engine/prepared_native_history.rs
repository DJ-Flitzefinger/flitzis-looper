//! Source-specific native history and its complete fixed adapter ownership.
//!
//! Preparation runs on the existing Key Lock worker. Rendering and worker catch-up use the same
//! adapter, while every source pin travels with the native/FIFO owner until worker retirement.

use super::native_history_permit::NativeHistoryPermit;
use super::productive_source_history::ProductiveSourceBinding;
use super::rubberband_backend::{
    RubberBandError, RubberBandLiveShifter, pitch_scale_for_tempo_ratio,
};
use super::source_playback::SourcePlayback;
use super::source_reader::SourceReadPlan;
use super::stretch_processor::{DEFAULT_BLOCK_SAMPLES, FixedFifo, ProductiveSourceFeed};
use crate::messages::{PreparedStemSet, SampleBuffer};
use std::sync::Arc;

pub(crate) const PREPARED_HISTORY_FRAMES: usize = 4096;
pub(crate) const PITCH_SCALE_EPSILON: f64 = 0.001;

pub(crate) struct NativeHistoryRequest {
    pub(crate) sample: SampleBuffer,
    pub(crate) stems: Option<PreparedStemSet>,
    pub(crate) permit: NativeHistoryPermit,
    pub(crate) binding: ProductiveSourceBinding,
    pub(crate) playback: SourcePlayback,
    pub(crate) plan: SourceReadPlan,
    pub(crate) target_output_frame: u64,
    pub(crate) request_id: u64,
    pub(crate) epoch: u64,
}

impl NativeHistoryRequest {
    pub(crate) fn matches_contract(&self, feed: &ProductiveSourceFeed<'_>, epoch: u64) -> bool {
        self.epoch == epoch
            && self.binding
                == ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted)
            && self.permit.current(feed.sample, feed.sample_rate_hz)
            && self.permit.matches_projection(feed.accepted)
            && self.plan.matches_source_contract(feed.plan)
            && self.playback.matches_rate_target(feed.playback)
            && same_stem_owners(self.stems.as_ref(), feed.stems)
    }

    pub(crate) fn matches_adoption(
        &self,
        feed: &ProductiveSourceFeed<'_>,
        epoch: u64,
        request_id: u64,
    ) -> bool {
        self.request_id == request_id
            && self.matches_contract(feed, epoch)
            && feed.output_frame == Some(self.target_output_frame)
            && self.playback.matches_exact(feed.playback)
            && self.plan.matches_exact(feed.plan)
    }
}

fn same_stem_owners(left: Option<&PreparedStemSet>, right: Option<&PreparedStemSet>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.source_version_hash == right.source_version_hash
                && left.sample_rate_hz == right.sample_rate_hz
                && left.channels == right.channels
                && left.frame_count == right.frame_count
                && left.available_mask == right.available_mask
                && left.accepted_timing == right.accepted_timing
                && Arc::ptr_eq(&left.reference_samples, &right.reference_samples)
                && left.stems.iter().zip(&right.stems).all(|(left, right)| {
                    left.channels == right.channels && Arc::ptr_eq(&left.samples, &right.samples)
                })
        }
        _ => false,
    }
}

pub(crate) struct NativeAdapterState {
    pub(crate) rubberband: Option<RubberBandLiveShifter>,
    pub(crate) block_size: usize,
    pub(crate) input: Vec<Vec<f32>>,
    pub(crate) output: Vec<Vec<f32>>,
    pub(crate) input_fifo: Vec<FixedFifo>,
    pub(crate) output_fifo: Vec<FixedFifo>,
    pub(crate) active: bool,
    pub(crate) pitch_scale: f64,
    pub(crate) dirty: bool,
    pub(crate) used: bool,
    pub(crate) source: Option<NativeHistoryRequest>,
}

impl NativeAdapterState {
    pub(crate) fn new(rubberband: Option<RubberBandLiveShifter>, channels: usize) -> Self {
        let block_size = rubberband
            .as_ref()
            .map_or(DEFAULT_BLOCK_SAMPLES, RubberBandLiveShifter::block_size);
        Self {
            rubberband,
            block_size,
            input: (0..channels).map(|_| vec![0.0; block_size]).collect(),
            output: (0..channels).map(|_| vec![0.0; block_size]).collect(),
            input_fifo: (0..channels)
                .map(|_| FixedFifo::new(block_size + DEFAULT_BLOCK_SAMPLES))
                .collect(),
            output_fifo: (0..channels)
                .map(|_| FixedFifo::new(block_size * 2 + DEFAULT_BLOCK_SAMPLES))
                .collect(),
            active: false,
            pitch_scale: 1.0,
            dirty: false,
            used: false,
            source: None,
        }
    }

    /// Clears only fixed adapter storage. Source/native owners remain pinned until worker recycling.
    pub(crate) fn reset_adapter(&mut self) {
        self.dirty |= self.used;
        self.input.iter_mut().for_each(|channel| channel.fill(0.0));
        self.output.iter_mut().for_each(|channel| channel.fill(0.0));
        self.input_fifo.iter_mut().for_each(FixedFifo::reset);
        self.output_fifo.iter_mut().for_each(FixedFifo::reset);
        self.active = false;
        self.pitch_scale = 1.0;
    }

    /// Native reset, initial pitch setup, source catch-up and old pin destruction are worker-only.
    pub(crate) fn prepare_source(
        &mut self,
        mut request: NativeHistoryRequest,
    ) -> Result<(), RubberBandError> {
        self.source = None;
        self.reset_adapter();
        let pitch = pitch_scale_for_tempo_ratio(request.playback.tempo_ratio());
        self.rubberband
            .as_mut()
            .expect("prepared adapter owns native state")
            .prepare_exact_pitch(pitch)?;
        self.pitch_scale = pitch;
        self.dirty = false;
        self.used = false;
        let channels = self.input.len();
        let mut feed = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect::<Vec<_>>();
        let mut output = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect::<Vec<_>>();
        let mut remaining = PREPARED_HISTORY_FRAMES;
        while remaining > 0 {
            let (frames, ratio) = request.playback.chunk(remaining.min(DEFAULT_BLOCK_SAMPLES));
            request.plan.fill_fractional_buffers(
                &request.sample,
                request.stems.as_ref(),
                &request.playback,
                &mut feed,
                frames,
            );
            // A trajectory reaching unity still goes through native state during preparation;
            // adoption will reject a dry endpoint rather than changing dry playback semantics.
            self.render(
                &feed,
                &mut output,
                frames,
                pitch_scale_for_tempo_ratio(ratio),
            )?;
            request.playback.advance(frames);
            request
                .plan
                .transition
                .advance_fractional(frames as f64 * ratio);
            let position = request.playback.position();
            request.plan.frame_pos = position.frame;
            request.plan.seek_mode = position.seek_mode;
            remaining -= frames;
        }
        // Live rendering begins a smoothing chunk before passing its cursor to this adapter.
        request.playback.chunk(DEFAULT_BLOCK_SAMPLES);
        self.source = Some(request);
        Ok(())
    }

    /// One common adapter implementation for worker catch-up and bounded callback rendering.
    pub(crate) fn render(
        &mut self,
        feed: &[Vec<f32>],
        output: &mut [Vec<f32>],
        frames: usize,
        pitch: f64,
    ) -> Result<(), RubberBandError> {
        if !self.active {
            for fifo in &mut self.output_fifo {
                fifo.push_silence(self.block_size.saturating_sub(1));
            }
            self.active = true;
        }
        if (pitch - self.pitch_scale).abs() > PITCH_SCALE_EPSILON {
            self.rubberband
                .as_mut()
                .expect("active adapter owns native state")
                .set_pitch_scale(pitch)?;
            self.pitch_scale = pitch;
        }
        for (fifo, channel) in self.input_fifo.iter_mut().zip(feed) {
            if fifo.push_slice(&channel[..frames]) != frames {
                return Err(RubberBandError::InvalidBlockSize);
            }
        }
        let max_blocks = (DEFAULT_BLOCK_SAMPLES / self.block_size).saturating_add(2);
        for _ in 0..max_blocks {
            if !self
                .input_fifo
                .iter()
                .all(|fifo| fifo.len() >= self.block_size)
            {
                break;
            }
            for (fifo, channel) in self.input_fifo.iter_mut().zip(&mut self.input) {
                if fifo.pop_into(&mut channel[..self.block_size]) != self.block_size {
                    return Err(RubberBandError::InvalidBlockSize);
                }
            }
            self.rubberband
                .as_mut()
                .expect("active adapter owns native state")
                .shift(&self.input, &mut self.output)?;
            self.used = true;
            for (fifo, channel) in self.output_fifo.iter_mut().zip(&self.output) {
                if fifo.push_slice(&channel[..self.block_size]) != self.block_size {
                    return Err(RubberBandError::InvalidBlockSize);
                }
            }
        }
        for (fifo, channel) in self.output_fifo.iter_mut().zip(output) {
            let read = fifo.pop_into(&mut channel[..frames]);
            channel[read..frames].fill(0.0);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
    use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
    use crate::audio_engine::native_history_permit::NativeHistoryContext;
    use crate::audio_engine::prepared_source::PreparedSourcePermit;
    use crate::audio_engine::source_reader::{
        ExplicitSeekMode, FrameRange, StemRenderSelection, StemTransition, full_stem_available_mask,
    };
    use crate::messages::StemMixMode;
    use std::sync::atomic::AtomicU64;

    const SOURCE_FRAMES: usize = 24_001;
    const REGION: FrameRange = FrameRange {
        start: 100,
        end: 23_999,
    };

    pub(crate) fn fixture(
        channels: usize,
        rate: u32,
        ratio: f64,
        mode: ExplicitSeekMode,
        stem_mask: Option<u8>,
    ) -> NativeHistoryRequest {
        let pcm = (0..SOURCE_FRAMES)
            .flat_map(|frame| {
                let base = (frame as f64 * 0.057).sin() as f32 * 0.3
                    + (frame as f64 * 0.0037).cos() as f32 * 0.1;
                (0..channels).map(move |channel| if channel == 0 { base } else { base * -0.73 })
            })
            .collect::<Vec<_>>();
        let sample = SampleBuffer {
            channels,
            samples: Arc::from(pcm),
        };
        let stems = stem_mask.map(|_| PreparedStemSet {
            accepted_timing: None,
            reference_samples: sample.samples.clone(),
            publication: PreparedSourcePermit::unrestricted(),
            source_version_hash: 17,
            sample_rate_hz: rate,
            channels,
            frame_count: SOURCE_FRAMES,
            available_mask: full_stem_available_mask(),
            stems: [0.1, 0.2, 0.3, 0.4, 0.7].map(|gain| SampleBuffer {
                channels,
                samples: sample
                    .samples
                    .iter()
                    .map(|value| *value * gain)
                    .collect::<Vec<_>>()
                    .into(),
            }),
        });
        let frame = match mode {
            ExplicitSeekMode::Normal => 171,
            ExplicitSeekMode::BeforeLoop => 23,
            ExplicitSeekMode::AfterLoop => 24_000,
        };
        let mut playback = SourcePlayback::new(frame, mode, ratio);
        playback.configure(SOURCE_FRAMES, REGION);
        playback.advance(1);
        let position = playback.position();
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        ownership.publish_source(0, &sample, rate, 13);
        let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
        let epoch = Arc::new(AtomicU64::new(7));
        let permit = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &epoch,
        }
        .capture(&sample, rate, None)
        .unwrap();
        let plan = SourceReadPlan {
            channels,
            sample_frames: SOURCE_FRAMES,
            frame_pos: position.frame,
            loop_region: REGION,
            seek_mode: position.seek_mode,
            selection: if let Some(mask) = stem_mask {
                StemRenderSelection::from_state(StemMixMode::AllStems, 17, mask)
            } else {
                StemRenderSelection::full_mix()
            },
            transition: StemTransition::default(),
        };
        NativeHistoryRequest {
            binding: ProductiveSourceBinding::new(&sample, rate, None),
            sample,
            stems,
            permit,
            playback,
            plan,
            target_output_frame: PREPARED_HISTORY_FRAMES as u64,
            request_id: 1,
            epoch: 3,
        }
    }

    // Independent algebraic source addressing/interpolation; this oracle does not invoke the
    // production source reader, cursor advancement or adapter FIFO implementation.
    fn oracle_feed(
        request: &NativeHistoryRequest,
        start: usize,
        frames: usize,
        initial_mode: ExplicitSeekMode,
    ) -> Vec<Vec<f32>> {
        let origin_frame = match initial_mode {
            ExplicitSeekMode::Normal => 171,
            ExplicitSeekMode::BeforeLoop => 23,
            ExplicitSeekMode::AfterLoop => 24_000,
        };
        let ratio = request.playback.tempo_ratio();
        let address = |offset: usize| {
            let absolute = origin_frame + offset;
            match initial_mode {
                ExplicitSeekMode::Normal => {
                    REGION.start + (origin_frame - REGION.start + offset) % REGION.len()
                }
                ExplicitSeekMode::BeforeLoop if absolute < REGION.start => absolute,
                ExplicitSeekMode::BeforeLoop => {
                    REGION.start + (absolute - REGION.start) % REGION.len()
                }
                ExplicitSeekMode::AfterLoop if absolute < SOURCE_FRAMES => absolute,
                ExplicitSeekMode::AfterLoop => {
                    REGION.start + (absolute - SOURCE_FRAMES) % REGION.len()
                }
            }
        };
        let read = |frame: usize, channel: usize| {
            if let Some(stems) = &request.stems {
                stems
                    .stems
                    .iter()
                    .enumerate()
                    .take(4)
                    .filter(|(index, _)| {
                        request.plan.selection
                            == StemRenderSelection::from_state(StemMixMode::AllStems, 17, 0b1111)
                            || [1, 3].contains(index)
                    })
                    .map(|(_, stem)| stem.samples[frame * request.sample.channels + channel])
                    .sum()
            } else {
                request.sample.samples[frame * request.sample.channels + channel]
            }
        };
        (0..request.sample.channels)
            .map(|channel| {
                (start..start + frames)
                    .map(|frame| {
                        let distance = (frame + 1) as f64 * ratio;
                        let offset = distance.floor() as usize;
                        let left = read(address(offset), channel);
                        let right = read(address(offset + 1), channel);
                        left + (right - left) * distance.fract() as f32
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn real_prepared_native_fifo_suffix_matches_independent_raw_source_native_oracle() {
        let patterns = [&[512][..], &[1, 127, 384, 96, 257, 512, 31][..]];
        let continuation = 20_017;
        let mut cases = 0;
        for rate in [44_100, 48_000, 96_000] {
            for ratio in [0.73, 1.371_234_567_890_123, 2.0] {
                for mode in [
                    ExplicitSeekMode::Normal,
                    ExplicitSeekMode::BeforeLoop,
                    ExplicitSeekMode::AfterLoop,
                ] {
                    for (channels, stem_mask) in [1, 2].into_iter().flat_map(|channels| {
                        [None, Some(0b1010), Some(0b1111)].map(|mask| (channels, mask))
                    }) {
                        let reference_request = fixture(channels, rate, ratio, mode, stem_mask);
                        let mut raw = RubberBandLiveShifter::new(rate, channels).unwrap();
                        raw.prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
                            .unwrap();
                        let block = raw.block_size();
                        let total =
                            (PREPARED_HISTORY_FRAMES + continuation).div_ceil(block) * block;
                        let mut expected = (0..channels)
                            .map(|_| vec![0.0; block - 1])
                            .collect::<Vec<_>>();
                        let mut raw_output =
                            (0..channels).map(|_| vec![0.0; block]).collect::<Vec<_>>();
                        for start in (0..total).step_by(block) {
                            raw.shift(
                                &oracle_feed(&reference_request, start, block, mode),
                                &mut raw_output,
                            )
                            .unwrap();
                            for channel in 0..channels {
                                expected[channel].extend_from_slice(&raw_output[channel]);
                            }
                        }
                        for channel in &expected {
                            assert!(
                                channel[PREPARED_HISTORY_FRAMES
                                    ..PREPARED_HISTORY_FRAMES + continuation]
                                    .iter()
                                    .any(|sample| sample.abs() > 0.02),
                                "oracle suffix must contain genuine shifted audio at rate={rate} ratio={ratio}"
                            );
                        }
                        for pattern in patterns {
                            let request = fixture(channels, rate, ratio, mode, stem_mask);
                            let mut state = NativeAdapterState::new(
                                Some(RubberBandLiveShifter::new(rate, channels).unwrap()),
                                channels,
                            );
                            let address = state.rubberband.as_ref().unwrap().state_address();
                            state.prepare_source(request).unwrap();
                            assert_eq!(state.rubberband.as_ref().unwrap().state_address(), address);
                            assert!(state.active && state.used && !state.dirty);
                            assert_eq!(
                                state.input_fifo[0].len() + state.output_fifo[0].len(),
                                block - 1
                            );
                            assert_eq!(state.source.as_ref().unwrap().request_id, 1);
                            let mut playback = state.source.as_ref().unwrap().playback;
                            let mut plan = state.source.as_ref().unwrap().plan;
                            let mut feed = (0..channels)
                                .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
                                .collect::<Vec<_>>();
                            let mut output = (0..channels)
                                .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
                                .collect::<Vec<_>>();
                            let mut elapsed = 0;
                            let mut step = 0;
                            while elapsed < continuation {
                                let frames =
                                    pattern[step % pattern.len()].min(continuation - elapsed);
                                let source = state.source.as_ref().unwrap();
                                plan.fill_fractional_buffers(
                                    &source.sample,
                                    source.stems.as_ref(),
                                    &playback,
                                    &mut feed,
                                    frames,
                                );
                                state
                                    .render(
                                        &feed,
                                        &mut output,
                                        frames,
                                        pitch_scale_for_tempo_ratio(ratio),
                                    )
                                    .unwrap();
                                for channel in 0..channels {
                                    assert_eq!(
                                        &output[channel][..frames],
                                        &expected[channel][PREPARED_HISTORY_FRAMES + elapsed
                                            ..PREPARED_HISTORY_FRAMES + elapsed + frames],
                                        "rate={rate} ratio={ratio} mode={mode:?} channels={channels} stems={stem_mask:?} pattern={pattern:?}"
                                    );
                                }
                                playback.advance(frames);
                                let position = playback.position();
                                plan.frame_pos = position.frame;
                                plan.seek_mode = position.seek_mode;
                                elapsed += frames;
                                step += 1;
                                assert_eq!(
                                    state.input_fifo[0].len() + state.output_fifo[0].len(),
                                    block - 1
                                );
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 324);
    }

    #[test]
    fn prepared_owner_pins_real_source_until_worker_style_retirement() {
        let request = fixture(2, 48_000, 1.37, ExplicitSeekMode::Normal, Some(0b1010));
        let weak = Arc::downgrade(&request.sample.samples);
        let mut state =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 2).unwrap()), 2);
        state.prepare_source(request).unwrap();
        assert!(weak.upgrade().is_some());
        state.reset_adapter();
        assert!(
            weak.upgrade().is_some(),
            "adapter reset must retain source/native owner for worker retirement"
        );
        drop(state.source.take());
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn actual_adoption_requires_exact_outstanding_request_identity() {
        let request = fixture(1, 48_000, 1.37, ExplicitSeekMode::Normal, None);
        let mut state =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 1).unwrap()), 1);
        state.prepare_source(request).unwrap();
        let source = state.source.as_ref().unwrap();
        let feed = ProductiveSourceFeed {
            sample: &source.sample,
            stems: source.stems.as_ref(),
            sample_rate_hz: 48_000,
            accepted: None,
            plan: source.plan,
            playback: &source.playback,
            permit: Some(&source.permit),
            output_frame: Some(source.target_output_frame),
        };
        assert!(source.matches_adoption(&feed, 3, 1));
        assert!(!source.matches_adoption(&feed, 3, 2));
    }

    #[test]
    fn smoothed_preparation_and_continuation_match_piecewise_algebraic_raw_native() {
        let continuation = 20_017;
        let mut cases = 0;
        for rate in [44_100, 48_000, 96_000] {
            for channels in [1, 2] {
                for mode in [
                    ExplicitSeekMode::Normal,
                    ExplicitSeekMode::BeforeLoop,
                    ExplicitSeekMode::AfterLoop,
                ] {
                    for stem_mask in [None, Some(0b1010), Some(0b1111)] {
                        let mut request = fixture(channels, rate, 0.73, mode, stem_mask);
                        request.playback.set_target(1.37);
                        request.playback.chunk(DEFAULT_BLOCK_SAMPLES);
                        let mut raw = RubberBandLiveShifter::new(rate, channels).unwrap();
                        let block = raw.block_size();
                        assert_eq!(block, 512);
                        let mut ratio = 0.73 + 0.05;
                        raw.prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
                            .unwrap();
                        let mut frame = match mode {
                            ExplicitSeekMode::Normal => 171,
                            ExplicitSeekMode::BeforeLoop => 23,
                            ExplicitSeekMode::AfterLoop => 24_000,
                        };
                        let mut fraction = 0.73;
                        let mut seek_mode = mode;
                        let mut stable_elapsed = 0_usize;
                        let total =
                            (PREPARED_HISTORY_FRAMES + continuation).div_ceil(block) * block;
                        let mut expected = (0..channels)
                            .map(|_| vec![0.0; block - 1])
                            .collect::<Vec<_>>();
                        let mut feed = (0..channels).map(|_| vec![0.0; block]).collect::<Vec<_>>();
                        let mut shifted =
                            (0..channels).map(|_| vec![0.0; block]).collect::<Vec<_>>();
                        for _ in (0..total).step_by(block) {
                            let address = |offset: usize| {
                                let absolute = frame + offset;
                                match seek_mode {
                                    ExplicitSeekMode::Normal => {
                                        REGION.start
                                            + (frame - REGION.start + offset) % REGION.len()
                                    }
                                    ExplicitSeekMode::BeforeLoop if absolute < REGION.start => {
                                        absolute
                                    }
                                    ExplicitSeekMode::BeforeLoop => {
                                        REGION.start + (absolute - REGION.start) % REGION.len()
                                    }
                                    ExplicitSeekMode::AfterLoop if absolute < SOURCE_FRAMES => {
                                        absolute
                                    }
                                    ExplicitSeekMode::AfterLoop => {
                                        REGION.start + (absolute - SOURCE_FRAMES) % REGION.len()
                                    }
                                }
                            };
                            let read = |frame: usize, channel: usize| {
                                if let (Some(stems), Some(mask)) = (&request.stems, stem_mask) {
                                    stems
                                        .stems
                                        .iter()
                                        .enumerate()
                                        .take(4)
                                        .filter(|(index, _)| mask & (1 << index) != 0)
                                        .map(|(_, stem)| stem.samples[frame * channels + channel])
                                        .sum::<f32>()
                                } else {
                                    request.sample.samples[frame * channels + channel]
                                }
                            };
                            for (channel, output) in feed.iter_mut().enumerate() {
                                for (offset, value) in output.iter_mut().enumerate() {
                                    let distance: f64 =
                                        fraction + (stable_elapsed + offset) as f64 * ratio;
                                    let whole = distance.floor() as usize;
                                    let left = read(address(whole), channel);
                                    *value = left
                                        + (read(address(whole + 1), channel) - left)
                                            * distance.fract() as f32;
                                }
                            }
                            raw.set_pitch_scale(pitch_scale_for_tempo_ratio(ratio))
                                .unwrap();
                            raw.shift(&feed, &mut shifted).unwrap();
                            for channel in 0..channels {
                                expected[channel].extend_from_slice(&shifted[channel]);
                            }
                            let distance = fraction + (stable_elapsed + block) as f64 * ratio;
                            let whole = distance.floor() as usize;
                            let next_frame = address(whole);
                            seek_mode = match seek_mode {
                                ExplicitSeekMode::BeforeLoop if frame + whole >= REGION.start => {
                                    ExplicitSeekMode::Normal
                                }
                                ExplicitSeekMode::AfterLoop if frame + whole >= SOURCE_FRAMES => {
                                    ExplicitSeekMode::Normal
                                }
                                mode => mode,
                            };
                            if ratio == 1.37 {
                                stable_elapsed += block;
                            } else {
                                frame = next_frame;
                                fraction = distance.fract();
                            }
                            ratio = if (1.37_f64 - ratio).abs() <= 0.05 {
                                1.37
                            } else {
                                ratio + 0.05
                            };
                        }
                        assert!(
                            expected[0]
                                [PREPARED_HISTORY_FRAMES..PREPARED_HISTORY_FRAMES + continuation]
                                .iter()
                                .any(|sample| sample.abs() > 0.02)
                        );
                        let mut state = NativeAdapterState::new(
                            Some(RubberBandLiveShifter::new(rate, channels).unwrap()),
                            channels,
                        );
                        state.prepare_source(request).unwrap();
                        let mut playback = state.source.as_ref().unwrap().playback;
                        let mut plan = state.source.as_ref().unwrap().plan;
                        let mut feed = (0..channels)
                            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
                            .collect::<Vec<_>>();
                        let mut output = (0..channels)
                            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
                            .collect::<Vec<_>>();
                        let mut elapsed = 0;
                        for requested in [1, 127, 384, 96, 257, 512, 31].into_iter().cycle() {
                            if elapsed == continuation {
                                break;
                            }
                            let (frames, ratio) =
                                playback.chunk(requested.min(continuation - elapsed));
                            let source = state.source.as_ref().unwrap();
                            plan.fill_fractional_buffers(
                                &source.sample,
                                source.stems.as_ref(),
                                &playback,
                                &mut feed,
                                frames,
                            );
                            state
                                .render(
                                    &feed,
                                    &mut output,
                                    frames,
                                    pitch_scale_for_tempo_ratio(ratio),
                                )
                                .unwrap();
                            for channel in 0..channels {
                                assert_eq!(
                                    &output[channel][..frames],
                                    &expected[channel][PREPARED_HISTORY_FRAMES + elapsed
                                        ..PREPARED_HISTORY_FRAMES + elapsed + frames],
                                    "smoothed raw-native mismatch rate={rate} channels={channels} mode={mode:?} stems={stem_mask:?} n={elapsed}"
                                );
                            }
                            playback.advance(frames);
                            let position = playback.position();
                            plan.frame_pos = position.frame;
                            plan.seek_mode = position.seek_mode;
                            elapsed += frames;
                        }
                        cases += 1;
                    }
                }
            }
        }
        assert_eq!(cases, 54);
    }
}
