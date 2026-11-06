//! One 96-frame source-only continuation at a genuine optional replay checkpoint.

use super::native_render_observation::Scope;
use super::native_source_coverage::complete_view_available;
use super::prepared_native_history::{NativeAdapterState, SourceReadCheckpoint};
use super::source_feed_history::{SourceFeedContext, SourceReadDomain, SourceReadReceipt};
use super::source_reader::StemReadSets;
use super::stretch_processor::ProductiveSourceFeed;

pub(super) const WET_PROBE_FRAMES: usize = 96;

pub(crate) struct WetSpan {
    pub(super) receipt: SourceReadReceipt,
    pub(super) pcm: [[f32; WET_PROBE_FRAMES]; 2],
    pub(super) before: (usize, usize),
    pub(super) after: (usize, usize),
    pub(super) shifts: u64,
}

impl WetSpan {
    pub(super) fn render(native: &mut NativeAdapterState) -> Option<Self> {
        let source = native.source.as_ref()?;
        let checkpoint = source.actual_feed_checkpoint();
        let mut playback = checkpoint.playback;
        let (frames, ratio) = playback.chunk(WET_PROBE_FRAMES);
        if frames != WET_PROBE_FRAMES
            || source.binding.sample_rate_hz != 96_000
            || checkpoint.plan.channels != 2
            || source.coverage.is_some()
            || checkpoint.plan.transition.is_active()
            || !complete_view_available(&source.sample)
            || !source.permit.current(&source.sample, 96_000)
            || native.has_retained_feed()
        {
            return None;
        }
        let readers = checkpoint
            .plan
            .required_read_sets(StemReadSets::new(source.endpoint_stems(), None));
        let mut input = vec![vec![0.0; frames]; 2];
        checkpoint
            .plan
            .fill_fractional_buffers_checked_with_readers(
                &source.sample,
                readers,
                &playback,
                &mut input,
                frames,
            )
            .ok()?;
        let pitch = source
            .pitch_intent
            .native_pitch_scale(ratio, source.key_lock)
            .ok()?;
        let receipt = SourceReadReceipt {
            playback,
            plan: checkpoint.plan,
            context: SourceFeedContext::new(source.binding, readers),
            output_frames: frames,
            domain: SourceReadDomain::NativeInput,
        };
        let before = (native.input_fifo[0].len(), native.output_fifo[0].len());
        let mut output = vec![vec![0.0; frames]; 2];
        let scope = Scope::begin();
        native.render(&input, &mut output, frames, pitch).ok()?;
        let observation = scope.finish();
        if observation.shifts != 1 || observation.adapter_shortfall_channel_frames != 0 {
            return None;
        }
        Some(Self {
            receipt,
            pcm: std::array::from_fn(|channel| output[channel].as_slice().try_into().unwrap()),
            before,
            after: (native.input_fifo[0].len(), native.output_fifo[0].len()),
            shifts: observation.shifts,
        })
    }

    pub(super) fn matches(&self, feed: &ProductiveSourceFeed<'_>, frames: usize) -> bool {
        let readers = feed
            .plan
            .required_read_sets(StemReadSets::new(feed.stems, feed.outgoing_stems));
        frames == self.receipt.output_frames
            && SourceReadCheckpoint {
                playback: *feed.playback,
                plan: feed.plan,
            }
            .matches_exact(SourceReadCheckpoint {
                playback: self.receipt.playback,
                plan: self.receipt.plan,
            })
            && self.receipt.context
                == SourceFeedContext::new(
                    super::productive_source_history::ProductiveSourceBinding::new(
                        feed.sample,
                        feed.sample_rate_hz,
                        feed.accepted,
                    ),
                    readers,
                )
    }
}
