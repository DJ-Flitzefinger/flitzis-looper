//! One fixed logical control interval observed by the actual callback.
//!
//! This is neither PCM history nor a Native-input journal. It only certifies a past
//! constant launch contract; the existing worker still prepares its own original T.

use super::content_placement::ContentKey;
use super::prepared_native_history::SourceReadCheckpoint;
use super::source_feed_history::SourceFeedContext;
use super::source_reader::ExplicitSeekMode;

#[derive(Clone, Copy)]
struct ConstantInterval {
    start: u64,
    end: u64,
    revision: u64,
    next: SourceReadCheckpoint,
    context: SourceFeedContext,
    kind: ObservationKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObservationKind {
    Rendered,
    Paused,
}

/// Scalar program identity on either side of one completely claimed Normal birth.
/// The caller separately retains current content/source authority and permits no
/// ambiguous multiple active voices. This value owns no PCM or Native history.
#[derive(Clone, Copy)]
pub(crate) struct SourceControlBirth {
    pub(crate) key: ContentKey,
    pub(crate) checkpoint: SourceReadCheckpoint,
    pub(crate) context: SourceFeedContext,
    pub(crate) revision: u64,
}

#[derive(Default)]
pub(crate) struct SourceControlEpoch {
    interval: Option<ConstantInterval>,
    last_output_end: Option<u64>,
    clock_reversed: bool,
}

impl SourceControlEpoch {
    /// A new voice lifecycle starts a new observation, with no inherited past proof.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Control/Pause invalidation cannot reopen an interval across an output-clock reversal.
    pub(crate) fn invalidate(&mut self) {
        self.interval = None;
    }

    /// Carry only already rendered constant controls across an independently
    /// claimed same-content phase reset. All voice, source-feed and Native/FIFO
    /// lifecycle resets still happen; only this scalar observation is rebased.
    pub(crate) fn continue_independent_birth(
        &self,
        frame: u64,
        previous: SourceControlBirth,
        next: SourceControlBirth,
    ) -> Option<Self> {
        let interval = self.interval?;
        if self.clock_reversed
            || self.last_output_end != Some(frame)
            || interval.end != frame
            || interval.kind != ObservationKind::Rendered
            || previous.key != next.key
            || previous.revision == u64::MAX
            || previous.revision != next.revision
            || interval.revision != previous.revision
            || interval.context != previous.context
            || previous.context != next.context
            || !constant_normal(previous.checkpoint)
            || !constant_normal(next.checkpoint)
            || !interval.next.matches_exact(previous.checkpoint)
            || !previous
                .checkpoint
                .plan
                .matches_source_contract(next.checkpoint.plan)
            || previous.checkpoint.playback.tempo_ratio().to_bits()
                != next.checkpoint.playback.tempo_ratio().to_bits()
        {
            return None;
        }
        Some(Self {
            interval: Some(ConstantInterval {
                next: next.checkpoint,
                ..interval
            }),
            last_output_end: self.last_output_end,
            clock_reversed: false,
        })
    }

    fn output_interval(&mut self, output_frame: Option<u64>, frames: usize) -> Option<(u64, u64)> {
        let Some(start) = output_frame else {
            self.invalidate();
            return None;
        };
        let Some(end) = start.checked_add(frames as u64).filter(|end| *end > start) else {
            self.invalidate();
            return None;
        };
        if self
            .last_output_end
            .is_some_and(|previous| start < previous)
        {
            self.clock_reversed = true;
        }
        self.last_output_end = Some(end);
        if self.clock_reversed {
            self.invalidate();
            return None;
        }
        Some((start, end))
    }

    pub(crate) fn gap(&mut self, output_frame: Option<u64>, frames: usize) {
        let _ = self.output_interval(output_frame, frames);
        self.invalidate();
    }

    /// Called only after a successful render receipt, using its separate LOGICAL cursor.
    /// Fixed scalar copies/projected checkpoints add no owners, allocations or replay to RT.
    pub(crate) fn record(
        &mut self,
        output_frame: Option<u64>,
        frames: usize,
        checkpoint: SourceReadCheckpoint,
        context: SourceFeedContext,
        revision: u64,
    ) {
        self.record_observation(
            output_frame,
            frames,
            checkpoint,
            context,
            revision,
            ObservationKind::Rendered,
        );
    }

    /// A real paused callback observes stationary controls and available immutable
    /// source/readers. This certifies no PCM read and never extends a rendered interval.
    pub(crate) fn record_paused(
        &mut self,
        output_frame: Option<u64>,
        frames: usize,
        checkpoint: SourceReadCheckpoint,
        context: SourceFeedContext,
        revision: u64,
    ) {
        self.record_observation(
            output_frame,
            frames,
            checkpoint,
            context,
            revision,
            ObservationKind::Paused,
        );
    }

    fn record_observation(
        &mut self,
        output_frame: Option<u64>,
        frames: usize,
        checkpoint: SourceReadCheckpoint,
        context: SourceFeedContext,
        revision: u64,
        kind: ObservationKind,
    ) {
        let Some((start, end)) = self.output_interval(output_frame, frames) else {
            return;
        };
        if revision == u64::MAX || !constant_normal(checkpoint) {
            self.invalidate();
            return;
        }
        let mut next = checkpoint;
        if kind == ObservationKind::Rendered && !next.project_output_frames(frames as u64) {
            self.invalidate();
            return;
        }
        let first = self.interval.filter(|previous| {
            previous.end == start
                && previous.kind == kind
                && previous.revision == revision
                && previous.context == context
                && previous.next.matches_exact(checkpoint)
        });
        self.interval = Some(ConstantInterval {
            start: first.map_or(start, |previous| previous.start),
            end,
            revision,
            next,
            context,
            kind,
        });
    }

    /// The first boundary stays unavailable: it cannot order an input against a
    /// same-frame control ABA. Interior T through the exact observed F is certified.
    /// A Copy result is consumed at admission; no predecessor lifetime guards the new launch.
    pub(crate) fn covers(
        &self,
        original: u64,
        callback: u64,
        checkpoint: SourceReadCheckpoint,
        context: SourceFeedContext,
        revision: u64,
    ) -> bool {
        self.covers_observation(
            original,
            callback,
            checkpoint,
            context,
            revision,
            ObservationKind::Rendered,
        )
    }

    pub(crate) fn covers_paused(
        &self,
        original: u64,
        callback: u64,
        checkpoint: SourceReadCheckpoint,
        context: SourceFeedContext,
        revision: u64,
    ) -> bool {
        self.covers_observation(
            original,
            callback,
            checkpoint,
            context,
            revision,
            ObservationKind::Paused,
        )
    }

    fn covers_observation(
        &self,
        original: u64,
        callback: u64,
        checkpoint: SourceReadCheckpoint,
        context: SourceFeedContext,
        revision: u64,
        kind: ObservationKind,
    ) -> bool {
        !self.clock_reversed
            && original < callback
            && revision != u64::MAX
            && constant_normal(checkpoint)
            && self.interval.is_some_and(|interval| {
                original > interval.start
                    && interval.kind == kind
                    && interval.end == callback
                    && interval.revision == revision
                    && interval.context == context
                    && interval.next.matches_exact(checkpoint)
            })
    }
}

fn constant_normal(checkpoint: SourceReadCheckpoint) -> bool {
    checkpoint.is_consistent()
        && !checkpoint.playback.has_phase_program()
        && checkpoint.plan.seek_mode == ExplicitSeekMode::Normal
        && checkpoint.playback.position().seek_mode == ExplicitSeekMode::Normal
        && !checkpoint.plan.transition.is_active()
        && checkpoint.playback.tempo_ratio().to_bits()
            == checkpoint.playback.rate_target().to_bits()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::content_placement::{ContentCell, OwnerIdentity};
    use crate::audio_engine::productive_source_history::ProductiveSourceBinding;
    use crate::audio_engine::source_playback::SourcePlayback;
    use crate::audio_engine::source_reader::{
        FrameRange, SourceLoopDomain, SourceReadPlan, StemReadSets, StemRenderSelection,
        StemTransition,
    };
    use crate::messages::SampleBuffer;
    use std::sync::Arc;

    fn source() -> (SampleBuffer, SourceReadCheckpoint, SourceFeedContext) {
        let sample = SampleBuffer {
            samples: Arc::from([0.25; 4096]),
            channels: 1,
            residency: None,
        };
        let region = FrameRange {
            start: 100,
            end: 3100,
        };
        let mut playback = SourcePlayback::new(1000, ExplicitSeekMode::Normal, 0.73);
        assert!(playback.configure_domain(SourceLoopDomain::physical(4096, region)));
        let checkpoint = SourceReadCheckpoint {
            playback,
            plan: SourceReadPlan {
                channels: 1,
                sample_frames: 4096,
                frame_pos: 1000,
                loop_region: region,
                loop_period: crate::audio_engine::source_reader::SourceReaderPeriod::Physical,
                seek_mode: ExplicitSeekMode::Normal,
                selection: StemRenderSelection::full_mix(),
                transition: StemTransition::default(),
            },
        };
        let context = SourceFeedContext::new(
            ProductiveSourceBinding::new(&sample, 48_000, None),
            StemReadSets::default(),
        );
        (sample, checkpoint, context)
    }

    fn projected(mut checkpoint: SourceReadCheckpoint, frames: u64) -> SourceReadCheckpoint {
        assert!(checkpoint.project_output_frames(frames));
        checkpoint
    }

    fn birth(
        key: ContentKey,
        checkpoint: SourceReadCheckpoint,
        context: SourceFeedContext,
    ) -> SourceControlBirth {
        SourceControlBirth {
            key,
            checkpoint,
            context,
            revision: 7,
        }
    }

    fn reset_phase(mut checkpoint: SourceReadCheckpoint) -> SourceReadCheckpoint {
        assert!(
            checkpoint
                .playback
                .seek(checkpoint.plan.loop_region.start, ExplicitSeekMode::Normal)
        );
        assert!(
            checkpoint
                .playback
                .configure_domain(checkpoint.plan.domain())
        );
        checkpoint.plan.frame_pos = checkpoint.plan.loop_region.start;
        checkpoint.plan.seek_mode = ExplicitSeekMode::Normal;
        checkpoint
    }

    #[test]
    fn independent_birth_retains_control_proof_across_phase_reset_without_voice_history() {
        let owner = Arc::new(());
        let key = ContentKey::new(
            OwnerIdentity::of(&owner),
            ContentCell::from_index(0).unwrap(),
            1,
        )
        .unwrap();
        let (_sample, initial, context) = source();
        let mut old = SourceControlEpoch::default();
        old.record(Some(100), 512, initial, context, 7);
        let previous = birth(key, projected(initial, 512), context);
        let next = birth(key, reset_phase(previous.checkpoint), context);
        let mut continued = old.continue_independent_birth(612, previous, next).unwrap();
        // The predecessor can retire. Only the scalar control observation survives.
        old.reset();
        continued.record(Some(612), 31, next.checkpoint, context, 7);
        let current = projected(next.checkpoint, 31);
        assert!(continued.covers(611, 643, current, context, 7));
        assert!(continued.covers(612, 643, current, context, 7));
        assert!(!continued.covers(100, 643, current, context, 7));
    }

    #[test]
    fn independent_birth_refuses_foreign_content_source_loop_rate_or_control_authority() {
        let owner = Arc::new(());
        let foreign_owner = Arc::new(());
        let key = ContentKey::new(
            OwnerIdentity::of(&owner),
            ContentCell::from_index(0).unwrap(),
            1,
        )
        .unwrap();
        let (_sample, initial, context) = source();
        let mut epoch = SourceControlEpoch::default();
        epoch.record(Some(100), 512, initial, context, 7);
        let previous = birth(key, projected(initial, 512), context);
        let next = birth(key, reset_phase(previous.checkpoint), context);
        for foreign_key in [
            ContentKey::new(OwnerIdentity::of(&foreign_owner), key.cell(), 1).unwrap(),
            ContentKey::new(OwnerIdentity::of(&owner), key.cell(), 2).unwrap(),
            ContentKey::new(
                OwnerIdentity::of(&owner),
                ContentCell::from_index(1).unwrap(),
                1,
            )
            .unwrap(),
        ] {
            assert!(
                epoch
                    .continue_independent_birth(
                        612,
                        previous,
                        SourceControlBirth {
                            key: foreign_key,
                            ..next
                        }
                    )
                    .is_none()
            );
        }
        let mut foreign = next;
        foreign.context.binding.source_address ^= 1;
        assert!(
            epoch
                .continue_independent_birth(612, previous, foreign)
                .is_none()
        );
        let mut changed_loop = next;
        changed_loop.checkpoint.plan.loop_region.start += 1;
        changed_loop.checkpoint = reset_phase(changed_loop.checkpoint);
        assert!(
            epoch
                .continue_independent_birth(612, previous, changed_loop)
                .is_none()
        );
        let mut intro = next;
        assert!(intro.checkpoint.playback.seek(
            intro.checkpoint.plan.loop_region.start - 1,
            ExplicitSeekMode::BeforeLoop,
        ));
        assert!(
            intro
                .checkpoint
                .playback
                .configure_domain(intro.checkpoint.plan.domain())
        );
        intro.checkpoint.plan.frame_pos = intro.checkpoint.plan.loop_region.start - 1;
        intro.checkpoint.plan.seek_mode = ExplicitSeekMode::BeforeLoop;
        assert!(
            epoch
                .continue_independent_birth(612, previous, intro)
                .is_none()
        );
        let mut gliding = next;
        assert!(gliding.checkpoint.playback.set_target(1.43));
        assert!(
            epoch
                .continue_independent_birth(612, previous, gliding)
                .is_none()
        );
        let mut other_rate = next;
        other_rate.checkpoint.playback = SourcePlayback::new(
            other_rate.checkpoint.plan.frame_pos,
            ExplicitSeekMode::Normal,
            1.43,
        );
        assert!(
            other_rate
                .checkpoint
                .playback
                .configure_domain(other_rate.checkpoint.plan.domain())
        );
        assert!(
            epoch
                .continue_independent_birth(612, previous, other_rate)
                .is_none()
        );
        for revision in [8, u64::MAX] {
            assert!(
                epoch
                    .continue_independent_birth(
                        612,
                        previous,
                        SourceControlBirth { revision, ..next }
                    )
                    .is_none()
            );
        }
        let advanced = SourceControlBirth {
            checkpoint: projected(previous.checkpoint, 1),
            ..previous
        };
        assert!(
            epoch
                .continue_independent_birth(612, advanced, next)
                .is_none()
        );
        assert!(
            epoch
                .continue_independent_birth(613, previous, next)
                .is_none()
        );
    }

    #[test]
    fn independent_birth_cannot_restore_stopped_paused_gap_or_control_aba_history() {
        let owner = Arc::new(());
        let key = ContentKey::new(
            OwnerIdentity::of(&owner),
            ContentCell::from_index(0).unwrap(),
            1,
        )
        .unwrap();
        let (_sample, initial, context) = source();
        let previous = birth(key, projected(initial, 512), context);
        let next = birth(key, reset_phase(previous.checkpoint), context);
        let mut epoch = SourceControlEpoch::default();
        assert!(
            epoch
                .continue_independent_birth(612, previous, next)
                .is_none()
        );
        epoch.record_paused(Some(100), 512, initial, context, 7);
        assert!(
            epoch
                .continue_independent_birth(612, birth(key, initial, context), next)
                .is_none()
        );
        epoch.reset();
        epoch.record(Some(100), 512, initial, context, 7);
        epoch.gap(Some(612), 31);
        assert!(
            epoch
                .continue_independent_birth(612, previous, next)
                .is_none()
        );
        epoch.reset();
        assert!(
            epoch
                .continue_independent_birth(612, previous, next)
                .is_none()
        );
        epoch.record(Some(100), 512, initial, context, 7);
        epoch.invalidate();
        epoch.record(Some(612), 31, previous.checkpoint, context, 9);
        let previous = SourceControlBirth {
            checkpoint: projected(previous.checkpoint, 31),
            revision: 9,
            ..previous
        };
        let next = SourceControlBirth {
            checkpoint: reset_phase(previous.checkpoint),
            revision: 9,
            ..next
        };
        let mut continued = epoch
            .continue_independent_birth(643, previous, next)
            .unwrap();
        continued.record(Some(643), 31, next.checkpoint, context, 9);
        let current = projected(next.checkpoint, 31);
        assert!(!continued.covers(611, 674, current, context, 9));
        assert!(!continued.covers(612, 674, current, context, 9));
        assert!(continued.covers(613, 674, current, context, 9));
        continued.gap(Some(600), 1);
        assert!(
            continued
                .continue_independent_birth(601, previous, next)
                .is_none()
        );
    }

    #[test]
    fn exact_adjacent_partitions_certify_interior_t_and_exact_observed_f_only() {
        let (_sample, mut checkpoint, context) = source();
        let mut epoch = SourceControlEpoch::default();
        let mut frame = 100;
        for count in [1, 31, 127, 17] {
            epoch.record(Some(frame), count, checkpoint, context, 7);
            frame += count as u64;
            checkpoint = projected(checkpoint, count as u64);
        }
        assert!(!epoch.covers(100, frame, checkpoint, context, 7));
        assert!(epoch.covers(101, frame, checkpoint, context, 7));
        assert!(epoch.covers(frame - 1, frame, checkpoint, context, 7));
        assert!(!epoch.covers(frame, frame, checkpoint, context, 7));
        assert!(!epoch.covers(frame - 1, frame + 1, checkpoint, context, 7));
        assert!(!epoch.covers(101, frame, checkpoint, context, 8));
        let mut wrong = checkpoint;
        wrong.plan.frame_pos += 1;
        assert!(!epoch.covers(101, frame, wrong, context, 7));
    }

    #[test]
    fn control_aba_and_new_reader_contract_cannot_relabel_an_older_interval() {
        let (_sample, mut checkpoint, context) = source();
        let mut epoch = SourceControlEpoch::default();
        epoch.record(Some(100), 512, checkpoint, context, 7);
        checkpoint = projected(checkpoint, 512);
        assert!(epoch.covers(611, 612, checkpoint, context, 7));
        epoch.invalidate();
        epoch.record(Some(612), 31, checkpoint, context, 9);
        checkpoint = projected(checkpoint, 31);
        assert!(!epoch.covers(611, 643, checkpoint, context, 9));
        assert!(!epoch.covers(612, 643, checkpoint, context, 9));
        assert!(epoch.covers(613, 643, checkpoint, context, 9));
        let mut changed = context;
        changed.binding.sample_rate_hz = 44_100;
        epoch.record(Some(643), 31, checkpoint, changed, 9);
        checkpoint = projected(checkpoint, 31);
        assert!(!epoch.covers(613, 674, checkpoint, changed, 9));
        assert!(!epoch.covers(644, 674, checkpoint, context, 9));
        assert!(epoch.covers(644, 674, checkpoint, changed, 9));
    }

    #[test]
    fn missing_reads_clock_gaps_and_reverse_output_never_bridge_the_original_t() {
        let (_sample, mut checkpoint, context) = source();
        let mut epoch = SourceControlEpoch::default();
        epoch.record(Some(100), 512, checkpoint, context, 7);
        checkpoint = projected(checkpoint, 512);
        epoch.gap(Some(612), 31);
        checkpoint = projected(checkpoint, 31);
        epoch.record(Some(643), 31, checkpoint, context, 7);
        checkpoint = projected(checkpoint, 31);
        assert!(!epoch.covers(611, 674, checkpoint, context, 7));
        epoch.record(Some(700), 31, checkpoint, context, 7);
        checkpoint = projected(checkpoint, 31);
        assert!(!epoch.covers(673, 731, checkpoint, context, 7));
        epoch.record(Some(600), 31, checkpoint, context, 7);
        checkpoint = projected(checkpoint, 31);
        assert!(!epoch.covers(601, 631, checkpoint, context, 7));
        epoch.invalidate();
        epoch.record(Some(731), 31, checkpoint, context, 7);
        checkpoint = projected(checkpoint, 31);
        assert!(!epoch.covers(732, 762, checkpoint, context, 7));
        epoch.reset();
        epoch.record(None, 31, checkpoint, context, 7);
        assert!(!epoch.covers(732, 762, checkpoint, context, 7));
        epoch.record(Some(u64::MAX - 1), 31, checkpoint, context, 7);
        assert!(!epoch.covers(u64::MAX - 2, u64::MAX, checkpoint, context, 7));
    }

    #[test]
    fn paused_observations_certify_stationary_interior_without_claiming_rendered_pcm() {
        let (_sample, checkpoint, context) = source();
        let mut epoch = SourceControlEpoch::default();
        epoch.record_paused(Some(100), 512, checkpoint, context, 7);
        epoch.record_paused(Some(612), 31, checkpoint, context, 7);
        assert!(!epoch.covers_paused(100, 643, checkpoint, context, 7));
        assert!(epoch.covers_paused(101, 643, checkpoint, context, 7));
        assert!(epoch.covers_paused(642, 643, checkpoint, context, 7));
        assert!(!epoch.covers(642, 643, checkpoint, context, 7));
        assert!(!epoch.covers_paused(642, 644, checkpoint, context, 7));
        assert!(!epoch.covers_paused(642, 643, projected(checkpoint, 1), context, 7));
    }

    #[test]
    fn rendered_and_paused_observations_never_merge_across_a_kind_boundary() {
        let (_sample, checkpoint, context) = source();
        let mut epoch = SourceControlEpoch::default();
        epoch.record(Some(100), 512, checkpoint, context, 7);
        let checkpoint = projected(checkpoint, 512);
        epoch.record_paused(Some(612), 31, checkpoint, context, 7);
        assert!(!epoch.covers_paused(611, 643, checkpoint, context, 7));
        assert!(!epoch.covers_paused(612, 643, checkpoint, context, 7));
        assert!(epoch.covers_paused(613, 643, checkpoint, context, 7));
        epoch.record(Some(643), 31, checkpoint, context, 7);
        let next = projected(checkpoint, 31);
        assert!(!epoch.covers(642, 674, next, context, 7));
        assert!(!epoch.covers(643, 674, next, context, 7));
        assert!(epoch.covers(644, 674, next, context, 7));
    }

    #[test]
    fn paused_control_aba_clock_gaps_and_rate_glides_cannot_certify_prior_t() {
        let (_sample, checkpoint, context) = source();
        let mut epoch = SourceControlEpoch::default();
        epoch.record_paused(Some(100), 512, checkpoint, context, 7);
        epoch.record_paused(Some(612), 31, checkpoint, context, 9);
        assert!(!epoch.covers_paused(611, 643, checkpoint, context, 9));
        assert!(epoch.covers_paused(613, 643, checkpoint, context, 9));
        epoch.gap(Some(643), 31);
        epoch.record_paused(Some(674), 31, checkpoint, context, 9);
        assert!(!epoch.covers_paused(642, 705, checkpoint, context, 9));
        let mut gliding = checkpoint;
        assert!(gliding.playback.set_target(1.37));
        epoch.record_paused(Some(705), 31, gliding, context, 9);
        assert!(!epoch.covers_paused(704, 736, gliding, context, 9));
        epoch.record_paused(Some(736), 31, checkpoint, context, 9);
        epoch.record_paused(Some(700), 31, checkpoint, context, 9);
        assert!(!epoch.covers_paused(701, 731, checkpoint, context, 9));
    }
}
