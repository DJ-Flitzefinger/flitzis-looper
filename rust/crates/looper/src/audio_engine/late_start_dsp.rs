//! Fixed late-birth filter custody shared by ordinary and piano preparation workers.
use super::content_placement::ContentKey;
use super::dsp::PerPadDspChain;
use super::native_source_coverage::normal_loop_feed_available;
use super::prepared_native_history::{NativeHistoryRequest, SourceReadCheckpoint};
use super::productive_source_history::ProductiveSourceBinding;
use super::rubberband_backend::RubberBandError;
use super::stretch_processor::DEFAULT_BLOCK_SAMPLES;

pub(crate) const INLINE_DSP_MAX_FRAMES: usize = 32;
pub(crate) const INLINE_DSP_CHANNEL_FRAME_BUDGET: usize = 128;

/// Allocated with the original control-owned work, never by callback capture.
pub(crate) struct InlineDspScratch(Vec<Vec<f32>>);
impl InlineDspScratch {
    pub(crate) fn new(channels: usize) -> Self {
        Self(
            (0..channels.min(8))
                .map(|_| vec![0.0; INLINE_DSP_MAX_FRAMES])
                .collect(),
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DspInputIntent {
    pub(crate) key: ContentKey,
    pub(crate) revision: u64,
    pub(crate) stable_since: u64,
    pub(crate) targets: [f32; super::dsp::DSP_PARAMETER_SLOTS],
    pub(crate) gain: f32,
}

pub(crate) struct LateStartDspWork {
    pub(crate) intent: DspInputIntent,
    original_frame: u64,
    output_frame: u64,
    next: SourceReadCheckpoint,
    endpoint: SourceReadCheckpoint,
    remaining: u64,
    chain: Option<PerPadDspChain>,
}

impl LateStartDspWork {
    pub(crate) fn new(
        intent: DspInputIntent,
        original_frame: u64,
        endpoint: SourceReadCheckpoint,
        frames: u64,
    ) -> Option<Self> {
        if frames == 0
            || intent.stable_since > original_frame
            || frames > super::source_feed_history::SOURCE_FEED_HISTORY_MAX_FRAMES as u64
        {
            return None;
        }
        let (playback, silence) = endpoint
            .playback
            .history_before_constant(usize::try_from(frames).ok()?)?;
        if silence != 0 {
            return None;
        }
        let position = playback.position();
        let next = SourceReadCheckpoint {
            playback,
            plan: super::source_reader::SourceReadPlan {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                ..endpoint.plan
            },
        };
        Self::from_origin(intent, original_frame, next, endpoint, frames)
    }

    pub(crate) fn from_origin(
        intent: DspInputIntent,
        original_frame: u64,
        next: SourceReadCheckpoint,
        endpoint: SourceReadCheckpoint,
        frames: u64,
    ) -> Option<Self> {
        if frames == 0
            || frames > super::source_feed_history::SOURCE_FEED_HISTORY_MAX_FRAMES as u64
            || intent.stable_since > original_frame
        {
            return None;
        }
        let mut projected = next;
        if !next.is_consistent()
            || !endpoint.is_consistent()
            || !projected.project_output_frames(frames)
            || !projected.matches_exact(endpoint)
        {
            return None;
        }
        Some(Self {
            intent,
            original_frame,
            output_frame: original_frame,
            next,
            endpoint,
            remaining: frames,
            chain: None,
        })
    }

    pub(crate) fn prepare(&mut self, rate: u32, channels: usize) {
        let mut chain = PerPadDspChain::new(0, rate as f32, channels);
        chain.install_late_start_intent(self.intent);
        chain.reset();
        self.chain = Some(chain);
    }

    pub(crate) fn consume(
        &mut self,
        request: &NativeHistoryRequest,
        output: &[Vec<f32>],
        frames: usize,
    ) -> Result<(), RubberBandError> {
        let checkpoint = SourceReadCheckpoint {
            playback: request.playback,
            plan: request.plan,
        };
        if frames as u64 > self.remaining
            || !checkpoint.matches_exact(self.next)
            || output.len() < request.plan.channels
            || output
                .iter()
                .take(request.plan.channels)
                .any(|channel| channel.len() < frames)
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let mut next = checkpoint;
        if !next.project_output_frames(frames as u64) {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let chain = self
            .chain
            .as_mut()
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        chain.bind_source(
            ProductiveSourceBinding::new(
                &request.sample,
                request.binding.sample_rate_hz,
                request.binding.accepted,
            ),
            checkpoint.playback.position(),
            next.playback.position(),
            frames,
            Some(checkpoint.plan.domain()),
        );
        for frame in 0..frames {
            chain.begin_frame();
            for (channel, values) in output.iter().enumerate().take(request.plan.channels) {
                chain.process_sample(channel, values[frame] * self.intent.gain);
            }
        }
        self.next = next;
        self.remaining -= frames as u64;
        self.output_frame = self
            .output_frame
            .checked_add(frames as u64)
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        chain.observe_input_intent(self.intent.key, self.output_frame, Some(self.intent.gain));
        Ok(())
    }

    pub(crate) fn prepare_dry(
        &mut self,
        request: &mut NativeHistoryRequest,
    ) -> Result<(), RubberBandError> {
        let mut output = (0..request.plan.channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect::<Vec<_>>();
        self.prepare_dry_with_scratch(request, &mut output)
    }

    pub(crate) fn prepare_inline(
        &mut self,
        template: &PerPadDspChain,
        request: &mut NativeHistoryRequest,
        scratch: &mut InlineDspScratch,
    ) -> Result<(), RubberBandError> {
        if self.remaining > INLINE_DSP_MAX_FRAMES as u64
            || request.plan.channels > scratch.0.len()
            || self
                .remaining
                .checked_mul(request.plan.channels as u64)
                .is_none_or(|cost| cost > INLINE_DSP_CHANNEL_FRAME_BUDGET as u64)
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        // The existing live format already owns its coefficients. Clone only
        // fixed Rust storage; reset removes all old-source filter history.
        let mut chain = template.clone();
        chain.install_late_start_intent(self.intent);
        chain.reset();
        self.chain = Some(chain);
        self.prepare_dry_with_scratch(request, &mut scratch.0)
    }

    fn prepare_dry_with_scratch(
        &mut self,
        request: &mut NativeHistoryRequest,
        output: &mut [Vec<f32>],
    ) -> Result<(), RubberBandError> {
        if !request
            .actual_feed_checkpoint()
            .matches_exact(SourceReadCheckpoint {
                playback: request.playback,
                plan: request.plan,
            })
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let endpoint = (request.playback, request.plan);
        request.playback = self.next.playback;
        request.plan = self.next.plan;
        let block = output
            .iter()
            .take(request.plan.channels)
            .map(Vec::len)
            .min()
            .filter(|block| *block > 0)
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        while self.remaining != 0 {
            if !request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
                || !normal_loop_feed_available(
                    &request.sample,
                    request.stems.as_ref(),
                    request.binding.sample_rate_hz,
                    request.binding.accepted,
                    request.plan,
                    &request.playback,
                )
            {
                return Err(RubberBandError::UnavailableSourceHistory);
            }
            let frames = request
                .playback
                .chunk(self.remaining.min(block as u64) as usize)
                .0;
            if frames == 0 {
                return Err(RubberBandError::UnavailableSourceHistory);
            }
            request.plan.fill_fractional_buffers_checked(
                &request.sample,
                request.stems.as_ref(),
                &request.playback,
                output,
                frames,
            )?;
            self.consume(request, output, frames)?;
            request.playback = self.next.playback;
            request.plan = self.next.plan;
        }
        request.playback = endpoint.0;
        request.plan = endpoint.1;
        Ok(())
    }

    pub(crate) fn ready(&self) -> bool {
        self.remaining == 0 && self.chain.is_some() && self.next.matches_exact(self.endpoint)
    }

    pub(crate) fn original_frame(&self) -> u64 {
        self.original_frame
    }

    pub(crate) fn adopt_claimed(&mut self, live: &mut PerPadDspChain, id: usize) {
        let prepared = self.chain.as_mut().expect("preflighted late filter owner");
        prepared.retarget_pad_id(id as u16);
        std::mem::swap(live, prepared);
    }
}
