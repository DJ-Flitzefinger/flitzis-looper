//! Bounded source-only PCM carried by the prepared Native owner that produced it.

#[cfg(test)]
use super::native_render_observation::Scope;
use super::native_source_coverage::complete_view_available;
use super::prepared_native_history::{NativeAdapterState, SourceReadCheckpoint};
use super::rubberband_backend::RubberBandError;
use super::source_feed_history::{
    SourceFeedContext, SourceFeedSpan, SourceReadDomain, SourceReadReceipt,
};
use super::source_reader::StemReadSets;
use super::stretch_processor::ProductiveSourceFeed;

pub(super) const WET_CALLBACK_FRAMES: usize = 96;
pub(super) const WET_SAMPLE_RATE: u32 = 96_000;

pub(crate) struct WetSpan {
    callback_start: u64,
    pub(super) receipt: SourceReadReceipt,
    pub(super) pcm: [[f32; WET_CALLBACK_FRAMES]; 2],
    pub(super) consumed: usize,
    #[cfg(test)]
    pub(super) before: (usize, usize),
    #[cfg(test)]
    pub(super) after: (usize, usize),
    #[cfg(test)]
    pub(super) shifts: u64,
}

impl WetSpan {
    pub(super) fn prepare_shift(
        native: &mut NativeAdapterState,
        callback_start: u64,
    ) -> Result<Option<Self>, RubberBandError> {
        let Some(effect) = native
            .source
            .as_ref()
            .map(|source| source.target_output_frame)
        else {
            return Ok(None);
        };
        let frames = native.block_size.saturating_sub(native.input_fifo[0].len());
        if !Self::fits_request(callback_start, effect, frames) {
            // The ordinary candidate still owns its original, unshifted FIFO.
            return Ok(None);
        }
        // An interior recording split or STOP may consume fewer than N frames.
        // Those frames must already have been available without the shift.
        if effect != callback_start && native.output_fifo[0].len() < frames.saturating_sub(1) {
            return Ok(None);
        }
        Self::prepare(native, callback_start, frames)
    }

    #[cfg(test)]
    pub(super) fn render(native: &mut NativeAdapterState) -> Option<Self> {
        let callback_start = native.source.as_ref()?.target_output_frame;
        Self::prepare(native, callback_start, WET_CALLBACK_FRAMES)
            .ok()
            .flatten()
    }

    fn prepare(
        native: &mut NativeAdapterState,
        callback_start: u64,
        frames: usize,
    ) -> Result<Option<Self>, RubberBandError> {
        let Some(source) = native.source.as_ref() else {
            return Ok(None);
        };
        let checkpoint = source.actual_feed_checkpoint();
        let mut playback = checkpoint.playback;
        let (count, ratio) = playback.chunk(frames);
        if frames == 0
            || frames > WET_CALLBACK_FRAMES
            || count != frames
            || source.binding.sample_rate_hz != WET_SAMPLE_RATE
            || checkpoint.plan.channels != 2
            || source.coverage.is_some()
            || checkpoint.plan.transition.is_active()
            || !complete_view_available(&source.sample)
            || !source.permit.current(&source.sample, WET_SAMPLE_RATE)
            || native.has_retained_feed()
            || !native.active
        {
            return Ok(None);
        }
        let before = (native.input_fifo[0].len(), native.output_fifo[0].len());
        if before.0 + frames < native.block_size
            || before.0 + frames >= native.block_size * 2
            || before.1 + native.block_size < frames
            || native.input_fifo.iter().any(|fifo| fifo.len() != before.0)
            || native.output_fifo.iter().any(|fifo| fifo.len() != before.1)
        {
            return Ok(None);
        }
        let readers = checkpoint
            .plan
            .required_read_sets(StemReadSets::new(source.endpoint_stems(), None));
        let mut input = vec![vec![0.0; frames]; 2];
        if checkpoint
            .plan
            .fill_fractional_buffers_checked_with_readers(
                &source.sample,
                readers,
                &playback,
                &mut input,
                frames,
            )
            .is_err()
        {
            return Ok(None);
        }
        let Ok(pitch) = source
            .pitch_intent
            .native_pitch_scale(ratio, source.key_lock)
        else {
            return Ok(None);
        };
        let receipt = SourceReadReceipt {
            playback,
            plan: checkpoint.plan,
            context: SourceFeedContext::new(source.binding, readers),
            output_frames: frames,
            domain: SourceReadDomain::NativeInput,
        };
        let mut output = vec![vec![0.0; frames]; 2];
        #[cfg(test)]
        let scope = Scope::begin();
        native.render(&input, &mut output, frames, pitch)?;
        #[cfg(test)]
        let observation = scope.finish();
        #[cfg(test)]
        if observation.shifts != 1 || observation.adapter_shortfall_channel_frames != 0 {
            return Err(RubberBandError::InvalidBlockSize);
        }
        let mut pcm = [[0.0; WET_CALLBACK_FRAMES]; 2];
        for (channel, output) in pcm.iter_mut().zip(output) {
            channel[..frames].copy_from_slice(&output);
        }
        Ok(Some(Self {
            callback_start,
            receipt,
            pcm,
            consumed: 0,
            #[cfg(test)]
            before,
            #[cfg(test)]
            after: (native.input_fifo[0].len(), native.output_fifo[0].len()),
            #[cfg(test)]
            shifts: observation.shifts,
        }))
    }

    pub(super) fn remaining(&self) -> usize {
        self.receipt.output_frames - self.consumed
    }

    fn fits_request(callback_start: u64, effect: u64, frames: usize) -> bool {
        effect.checked_sub(callback_start).is_some_and(|offset| {
            offset < WET_CALLBACK_FRAMES as u64
                && frames as u64 <= WET_CALLBACK_FRAMES as u64 - offset
                && callback_start
                    .checked_add(WET_CALLBACK_FRAMES as u64)
                    .is_some()
        })
    }

    pub(super) fn matches_request(&self, callback: Option<u64>, effect: u64, frame: u64) -> bool {
        callback == Some(self.callback_start)
            && Self::fits_request(self.callback_start, effect, self.receipt.output_frames)
            && effect.checked_add(self.consumed as u64) == Some(frame)
    }

    #[cfg(test)]
    pub(super) fn matches(&self, feed: &ProductiveSourceFeed<'_>, frames: usize) -> bool {
        frames == self.receipt.output_frames && self.slice_for(feed, frames).is_some()
    }

    pub(super) fn slice_for(
        &self,
        feed: &ProductiveSourceFeed<'_>,
        frames: usize,
    ) -> Option<SourceReadReceipt> {
        let slice = SourceFeedSpan {
            active_start: 0,
            frames: self.receipt.output_frames,
            playback: self.receipt.playback,
            plan: self.receipt.plan,
            context: self.receipt.context,
        }
        .slice(self.consumed, frames)?;
        let readers = feed
            .plan
            .required_read_sets(StemReadSets::new(feed.stems, feed.outgoing_stems));
        (SourceReadCheckpoint {
            playback: *feed.playback,
            plan: feed.plan,
        }
        .matches_exact(SourceReadCheckpoint {
            playback: slice.playback,
            plan: slice.plan,
        }) && slice.context
            == SourceFeedContext::new(
                super::productive_source_history::ProductiveSourceBinding::new(
                    feed.sample,
                    feed.sample_rate_hz,
                    feed.accepted,
                ),
                readers,
            ))
        .then_some(SourceReadReceipt {
            playback: slice.playback,
            plan: slice.plan,
            context: slice.context,
            output_frames: frames,
            domain: SourceReadDomain::NativeInput,
        })
    }
}
