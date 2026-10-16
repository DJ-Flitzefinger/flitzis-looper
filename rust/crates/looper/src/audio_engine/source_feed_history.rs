//! Bounded scalar journal of successful, actual source reads, including dry reads.
//!
//! The callback owns this journal; a worker receives only a copied snapshot. Addresses are
//! comparison markers, never dereferenced owners or fresh source/timing authority. A request
//! must separately pin and admit every historical physical context it intends to replay.

use super::productive_source_history::ProductiveSourceBinding;
use super::source_playback::SourcePlayback;
use super::source_reader::{SourceReadPlan, StemReadSets};
use crate::messages::{PreparedStemSet, ResidentBinding, STEM_BUFFER_COUNT};
use std::ops::Range;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub(crate) const SOURCE_FEED_HISTORY_MAX_SPANS: usize = 1024;
pub(crate) const SOURCE_FEED_HISTORY_MAX_FRAMES: usize = 262_144;
const STEM_OWNER_MARKER_COUNT: usize = STEM_BUFFER_COUNT + 2;

#[derive(Clone, Copy, Debug, PartialEq)]
struct StemFeedIdentity {
    complete_pcm: [u8; 32],
    publication_owner: usize,
    publication_generation: u64,
    source_version_hash: u64,
    sample_rate_hz: u32,
    channels: usize,
    frame_count: usize,
    available_mask: u8,
    reference_resident: Option<ResidentBinding>,
    component_residents: [Option<ResidentBinding>; STEM_BUFFER_COUNT],
}

/// Physical identities used by one actual feed, without retaining any PCM ownership.
/// Historical accepted/resident tags remain data and cannot authorize a new request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SourceFeedContext {
    pub(crate) binding: ProductiveSourceBinding,
    /// Complete-set owner, reference PCM allocation, then the four component allocations.
    pub(crate) current_stem_owners: [usize; STEM_OWNER_MARKER_COUNT],
    pub(crate) outgoing_stem_owners: [usize; STEM_OWNER_MARKER_COUNT],
    // Copied immutable PCM identity and original publication epoch/value prevent address ABA.
    current_stem_generation: Option<StemFeedIdentity>,
    outgoing_stem_generation: Option<StemFeedIdentity>,
}

impl SourceFeedContext {
    /// Pass the actual plan's `required_read_sets` so unused reader slots add no dependency.
    pub(crate) fn new(binding: ProductiveSourceBinding, readers: StemReadSets<'_>) -> Self {
        Self {
            binding,
            current_stem_owners: stem_owner_markers(readers.current),
            outgoing_stem_owners: stem_owner_markers(readers.outgoing),
            current_stem_generation: stem_generation(readers.current),
            outgoing_stem_generation: stem_generation(readers.outgoing),
        }
    }

    /// Compare physical source identities only; this does not validate current authority.
    pub(crate) fn same_source(self, other: Self) -> bool {
        self.binding.same_source(other.binding)
            && self.current_stem_owners == other.current_stem_owners
            && self.outgoing_stem_owners == other.outgoing_stem_owners
            && self.current_stem_generation == other.current_stem_generation
            && self.outgoing_stem_generation == other.outgoing_stem_generation
    }

    /// Keep the original physical slot mapping, dropping only slots this actual plan
    /// no longer reads. Requirements come from the plan even if no descriptor resolves.
    pub(crate) fn for_plan(mut self, plan: SourceReadPlan) -> Self {
        let mut current = false;
        let mut outgoing = false;
        for selection in std::iter::once(plan.selection)
            .chain(plan.transition.required_from_selection())
            .filter(|selection| selection.required_stem_hash().is_some())
        {
            if self.current_stem_generation.is_some_and(|identity| {
                selection.matches_stem_descriptor(
                    self.current_stem_owners[0],
                    identity.source_version_hash,
                )
            }) {
                current = true;
            } else if self.outgoing_stem_generation.is_some_and(|identity| {
                selection.matches_stem_descriptor(
                    self.outgoing_stem_owners[0],
                    identity.source_version_hash,
                )
            }) {
                outgoing = true;
            }
        }
        if !current {
            self.current_stem_owners = [0; STEM_OWNER_MARKER_COUNT];
            self.current_stem_generation = None;
        }
        if !outgoing {
            self.outgoing_stem_owners = [0; STEM_OWNER_MARKER_COUNT];
            self.outgoing_stem_generation = None;
        }
        self
    }

    /// Resolve both required historical roles against one actually retained pin.
    /// Markers never construct ownership, and a missing lookup cannot remove a role.
    pub(crate) fn resolve_readers<'a>(
        self,
        plan: SourceReadPlan,
        held: Option<&'a PreparedStemSet>,
    ) -> Option<StemReadSets<'a>> {
        let context = self.for_plan(plan);
        let resolve = |owners, identity: Option<StemFeedIdentity>| match identity {
            Some(identity) => held
                .filter(|held| {
                    stem_owner_markers(Some(held)) == owners
                        && stem_generation(Some(held)) == Some(identity)
                })
                .map(Some),
            None => Some(None),
        };
        let current = resolve(context.current_stem_owners, context.current_stem_generation)?;
        let outgoing = resolve(
            context.outgoing_stem_owners,
            context.outgoing_stem_generation,
        )?;
        let readers = StemReadSets::new(current, outgoing);
        for selection in
            std::iter::once(plan.selection).chain(plan.transition.required_from_selection())
        {
            if selection.required_stem_hash().is_some()
                && readers.for_selection(selection).is_none()
            {
                return None;
            }
        }
        Some(readers)
    }
}

fn stem_generation(stems: Option<&PreparedStemSet>) -> Option<StemFeedIdentity> {
    stems.map(|stems| StemFeedIdentity {
        complete_pcm: *stems.complete_set_identity,
        publication_owner: Arc::as_ptr(&stems.publication.epoch) as usize,
        publication_generation: stems.publication.expected,
        source_version_hash: stems.source_version_hash,
        sample_rate_hz: stems.sample_rate_hz,
        channels: stems.channels,
        frame_count: stems.frame_count,
        available_mask: stems.available_mask,
        reference_resident: stems
            .reference_residency
            .as_ref()
            .map(|view| ResidentBinding {
                address: stems.reference_samples.as_ptr() as usize,
                start: view.start_frame,
                end: view
                    .start_frame
                    .saturating_add(stems.reference_samples.len() / stems.channels.max(1)),
                revision: view.window_revision,
                context: view.context,
            }),
        component_residents: std::array::from_fn(|index| stems.stems[index].resident_binding()),
    })
}

fn stem_owner_markers(stems: Option<&PreparedStemSet>) -> [usize; STEM_OWNER_MARKER_COUNT] {
    stems.map_or([0; STEM_OWNER_MARKER_COUNT], |stems| {
        std::array::from_fn(|index| match index {
            0 => Arc::as_ptr(&stems.complete_set_identity) as usize,
            1 => stems.reference_samples.as_ptr() as usize,
            _ => stems.stems[index - 2].samples.as_ptr() as usize,
        })
    })
}

/// One successful constant-rate actual read, captured after `chunk` and before `advance`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceFeedSpan {
    pub(crate) active_start: u64,
    pub(crate) frames: usize,
    pub(crate) playback: SourcePlayback,
    pub(crate) plan: SourceReadPlan,
    pub(crate) context: SourceFeedContext,
}

impl SourceFeedSpan {
    pub(crate) fn active_end(self) -> u64 {
        self.active_start + self.frames as u64
    }

    /// Select a real subrange without reconstructing a rate or rebasing its original epoch.
    pub(crate) fn slice(self, offset: usize, frames: usize) -> Option<Self> {
        if frames == 0 || offset.checked_add(frames)? > self.frames {
            return None;
        }
        let mut result = self;
        result.active_start += offset as u64;
        result.frames = frames;
        advance_source(&mut result.playback, &mut result.plan, offset);
        result.context = result.context.for_plan(result.plan);
        Some(result)
    }

    fn continues_with(self, next: Self) -> bool {
        if self.active_end() != next.active_start || self.context != next.context {
            return false;
        }
        let mut playback = self.playback;
        let mut plan = self.plan;
        advance_source(&mut playback, &mut plan, self.frames);
        playback.matches_exact(&next.playback) && plan.matches_exact(next.plan)
    }
}

fn advance_source(playback: &mut SourcePlayback, plan: &mut SourceReadPlan, frames: usize) {
    let ratio = playback.tempo_ratio();
    playback.advance(frames);
    let position = playback.position();
    plan.frame_pos = position.frame;
    plan.seek_mode = position.seek_mode;
    plan.transition.advance_output_frames(frames, ratio);
}

/// Allocate once with the voice, outside the callback. Every later journal operation is bounded
/// by the fixed scalar span count and neither allocates nor retires an owning buffer.
pub(crate) struct SourceFeedHistory {
    spans: Box<[Option<SourceFeedSpan>]>,
    first: usize,
    len: usize,
    active_origin: u64,
    active_frames: u64,
    continuity_owner: Arc<AtomicU64>,
}

impl SourceFeedHistory {
    /// Scalar ABA fence for a pending effect that keeps an existing productive owner.
    pub(crate) fn continuity_revision(&self) -> u64 {
        self.continuity_owner.load(Ordering::Acquire)
    }

    pub(crate) fn new() -> Self {
        Self {
            spans: vec![None; SOURCE_FEED_HISTORY_MAX_SPANS].into_boxed_slice(),
            first: 0,
            len: 0,
            active_origin: 0,
            active_frames: 0,
            continuity_owner: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Fence start/restart/stop/seek before releasing source owners. The attempted-frame clock
    /// stays monotonic; this boundary becomes the new lifecycle's active-frame phase origin.
    /// Pause calls neither this method nor either record method.
    pub(crate) fn reset(&mut self) {
        self.clear_retained();
        self.fence_continuity();
        self.active_origin = self.active_frames;
    }

    /// Invalidate a captured F-to-E projection after source controls change. Keep the genuine
    /// past and its active-frame origin available to later captures. DSP-only preparation does
    /// not call this: another action's Native work cannot alter this source trajectory.
    pub(crate) fn invalidate_captures(&self) {
        self.fence_continuity();
    }

    /// Record only a completely accepted actual read, including a successful dry read.
    /// A chunk longer than retention keeps its exact real suffix, never an invented prefix.
    pub(crate) fn record_success(
        &mut self,
        playback: SourcePlayback,
        plan: SourceReadPlan,
        context: SourceFeedContext,
        frames: usize,
    ) {
        if frames == 0 {
            return;
        }
        let active_start = self.advance_clock(frames);
        let mut span = SourceFeedSpan {
            active_start,
            frames,
            playback,
            plan,
            context,
        };
        if frames > SOURCE_FEED_HISTORY_MAX_FRAMES {
            span = span
                .slice(
                    frames - SOURCE_FEED_HISTORY_MAX_FRAMES,
                    SOURCE_FEED_HISTORY_MAX_FRAMES,
                )
                .expect("the retained suffix is within its successful feed");
        }
        self.trim_before(
            self.active_frames
                .saturating_sub(SOURCE_FEED_HISTORY_MAX_FRAMES as u64),
        );
        if self.len != 0 {
            let last = self.index(self.len - 1);
            let previous = self.spans[last].expect("retained journal slots are initialized");
            if previous.continues_with(span) {
                self.spans[last] = Some(SourceFeedSpan {
                    frames: previous.frames + span.frames,
                    ..previous
                });
                return;
            }
        }
        if self.len == SOURCE_FEED_HISTORY_MAX_SPANS {
            self.pop_front();
        }
        let next = self.index(self.len);
        self.spans[next] = Some(span);
        self.len += 1;
    }

    /// A refused read still advances the attempted active-frame clock, matching the caller's
    /// source-clock advance. Discard the older prefix and fence every captured snapshot; a
    /// worker can never replay across this explicit missing interval. The lifecycle origin is
    /// retained, so a gap cannot silently establish a new Native block phase.
    pub(crate) fn record_gap(&mut self, attempted_frames: usize) {
        if attempted_frames == 0 {
            return;
        }
        self.clear_retained();
        self.fence_continuity();
        self.advance_clock(attempted_frames);
    }

    /// Copy the whole currently retained suffix into empty, already allocated action storage.
    /// Capture clones the epoch owner once. A filled destination is refused before any write or
    /// Arc replacement/drop; the worker/control owner must clear it before another capture.
    pub(crate) fn capture_into(&self, snapshot: &mut SourceFeedSnapshot) -> bool {
        let epoch = self.continuity_owner.load(Ordering::Acquire);
        if snapshot.continuity_owner.is_some() || epoch == 0 {
            return false;
        }
        for offset in 0..self.len {
            snapshot.spans[offset] = self.spans[self.index(offset)];
        }
        snapshot.len = self.len;
        snapshot.active_origin = self.active_origin;
        snapshot.active_frames = self.active_frames;
        snapshot.captured_epoch = epoch;
        snapshot.continuity_owner = Some(Arc::clone(&self.continuity_owner));
        true
    }

    fn index(&self, offset: usize) -> usize {
        (self.first + offset) % SOURCE_FEED_HISTORY_MAX_SPANS
    }

    fn pop_front(&mut self) {
        self.spans[self.first] = None;
        self.first = self.index(1);
        self.len -= 1;
    }

    fn clear_retained(&mut self) {
        // Entries are Copy scalar data; stale unreachable slots retain no resource owners.
        self.first = 0;
        self.len = 0;
    }

    fn trim_before(&mut self, active_start: u64) {
        while self.len != 0 {
            let oldest = self.spans[self.first].expect("retained journal slots are initialized");
            if oldest.active_end() <= active_start {
                self.pop_front();
            } else {
                if oldest.active_start < active_start {
                    let skipped = (active_start - oldest.active_start) as usize;
                    self.spans[self.first] = oldest.slice(skipped, oldest.frames - skipped);
                }
                return;
            }
        }
    }

    fn fence_continuity(&self) {
        let previous = self.continuity_owner.load(Ordering::Relaxed);
        // Exhaustion fails closed permanently rather than allowing an epoch ABA.
        let next = if previous == 0 {
            0
        } else {
            previous.checked_add(1).unwrap_or(0)
        };
        self.continuity_owner.store(next, Ordering::Release);
    }

    fn advance_clock(&mut self, frames: usize) -> u64 {
        let previous = self.active_frames;
        if let Some(next) = previous.checked_add(frames as u64) {
            self.active_frames = next;
            previous
        } else {
            // Counter exhaustion is an explicit origin reset and continuity fence, never a
            // saturated duplicate range. This also invalidates previously copied snapshots.
            self.clear_retained();
            self.fence_continuity();
            self.active_origin = 0;
            self.active_frames = frames as u64;
            0
        }
    }
}

/// Immutable-to-worker scalar copy. Its fixed Box is allocated once with its source lane or
/// action work; recycling keeps that same preallocated storage. Capturing never exposes the
/// callback's journal or mutable voice state to a worker. The descriptors are historical data,
/// never fresh source/window/timing authority.
pub(crate) struct SourceFeedSnapshot {
    spans: Box<[Option<SourceFeedSpan>]>,
    len: usize,
    active_origin: u64,
    active_frames: u64,
    captured_epoch: u64,
    continuity_owner: Option<Arc<AtomicU64>>,
}

/// Small additional adoption fence captured only from a genuine current snapshot and the
/// actual voice lifetime. It carries no source/window/timing authority or replay coverage.
/// Retain it with the Native permit and retire its Arc owners through that permit's existing
/// off-realtime source-owner retirement, including stale or refused preparation.
#[derive(Clone)]
pub(crate) struct SourceFeedGuard {
    continuity_owner: Arc<AtomicU64>,
    captured_epoch: u64,
    lifetime: Arc<AtomicU64>,
    generation: u64,
}

impl SourceFeedGuard {
    pub(crate) fn is_current(&self) -> bool {
        self.captured_epoch != 0
            && self.generation != 0
            && self.generation != u64::MAX
            && self.continuity_owner.load(Ordering::Acquire) == self.captured_epoch
            && self.lifetime.load(Ordering::Acquire) == self.generation
    }
}

impl SourceFeedSnapshot {
    pub(crate) fn new() -> Self {
        Self {
            spans: vec![None; SOURCE_FEED_HISTORY_MAX_SPANS].into_boxed_slice(),
            len: 0,
            active_origin: 0,
            active_frames: 0,
            captured_epoch: 0,
            continuity_owner: None,
        }
    }

    /// Worker/control-only recycling: resetting/dropping the captured Arc is never callback
    /// work. Scalar slots retain no owners and the fixed Box remains allocated for recapture.
    /// The callback must never clear or replace a filled snapshot.
    pub(crate) fn clear_for_reuse(&mut self) {
        self.continuity_owner = None;
        self.captured_epoch = 0;
        self.len = 0;
        self.active_origin = 0;
        self.active_frames = 0;
    }

    /// Retain the snapshot's original SOURCE fence and the actual voice lifetime before the
    /// worker recycles this large copy. Arc clones allocate no new owner; the resulting guard
    /// only narrows a separately captured strong Native permit and retires off realtime. If
    /// the final check loses a race, these clones cannot be final owners: both original Arcs
    /// remain borrowed throughout this call.
    pub(crate) fn guard(
        &self,
        lifetime: &Arc<AtomicU64>,
        generation: u64,
    ) -> Option<SourceFeedGuard> {
        if !self.is_current()
            || generation == 0
            || generation == u64::MAX
            || lifetime.load(Ordering::Acquire) != generation
        {
            return None;
        }
        let guard = SourceFeedGuard {
            continuity_owner: Arc::clone(self.continuity_owner.as_ref()?),
            captured_epoch: self.captured_epoch,
            lifetime: Arc::clone(lifetime),
            generation,
        };
        guard.is_current().then_some(guard)
    }

    pub(crate) fn spans(
        &self,
    ) -> impl ExactSizeIterator<Item = &SourceFeedSpan> + DoubleEndedIterator<Item = &SourceFeedSpan>
    {
        self.spans[..self.len].iter().map(|span| {
            span.as_ref()
                .expect("captured journal slots are initialized")
        })
    }

    pub(crate) fn retained_range(&self) -> Option<Range<u64>> {
        let first = self.spans().next()?;
        let last = self.spans().next_back()?;
        Some(first.active_start..last.active_end())
    }

    /// Monotonic attempted-frame coordinate of the latest lifecycle reset.
    pub(crate) fn active_origin(&self) -> u64 {
        self.active_origin
    }

    /// Monotonic attempted-frame endpoint, including refused reads and excluding Pause.
    pub(crate) fn active_frames(&self) -> u64 {
        self.active_frames
    }

    #[cfg(test)]
    pub(crate) fn captured_epoch(&self) -> u64 {
        self.captured_epoch
    }

    #[cfg(test)]
    pub(crate) fn continuity_owner(&self) -> Option<&Arc<AtomicU64>> {
        self.continuity_owner.as_ref()
    }

    /// Check in addition to the request's genuine source permit and control/lifecycle fences.
    pub(crate) fn is_current(&self) -> bool {
        self.captured_epoch != 0
            && self
                .continuity_owner
                .as_ref()
                .is_some_and(|owner| owner.load(Ordering::Acquire) == self.captured_epoch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::constant_timing::AcceptedTimingProjection;
    use crate::audio_engine::source_reader::{
        ExplicitSeekMode, FrameRange, SourceLoopDomain, StemRenderSelection, StemTransition,
    };

    fn held_stems() -> PreparedStemSet {
        let sample = crate::messages::SampleBuffer {
            channels: 2,
            residency: None,
            samples: vec![0.125; 4006].into(),
        }
        .with_complete_source(48_000);
        PreparedStemSet {
            complete_set_identity: Arc::new([23; 32]),
            accepted_timing: None,
            reference_samples: sample.samples.clone(),
            reference_residency: sample.residency.clone(),
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
            source_version_hash: 97,
            sample_rate_hz: 48_000,
            channels: 2,
            frame_count: 2003,
            available_mask: 15,
            stems: std::array::from_fn(|_| sample.clone()),
        }
    }

    #[test]
    fn held_selection_slices_preserve_required_roles_before_at_and_after_ramp_end() {
        let stems = held_stems();
        let playback = playback(19, ExplicitSeekMode::Normal, 0.5);
        let mut plan = plan(
            playback,
            StemTransition::start(StemRenderSelection::for_stems(&stems, 0), 64),
        );
        plan.selection = StemRenderSelection::full_mix();
        let context =
            SourceFeedContext::new(context().binding, StemReadSets::new(None, Some(&stems)));
        let span = SourceFeedSpan {
            active_start: 300,
            frames: 8192,
            playback,
            plan,
            context,
        };
        for offset in [0, 127] {
            let part = span.slice(offset, 1).unwrap();
            assert!(part.plan.transition.is_active());
            let readers = part
                .context
                .resolve_readers(part.plan, Some(&stems))
                .unwrap();
            assert!(readers.current.is_none());
            assert!(std::ptr::eq(readers.outgoing.unwrap(), &stems));
            assert!(part.context.resolve_readers(part.plan, None).is_none());
        }
        // A projected constant-rate span can be much longer than its source ramp.
        // Slicing a replay origin beyond the ramp must not require the obsolete pin.
        for offset in [128, 129, 4096] {
            let part = span.slice(offset, 512).unwrap();
            assert!(!part.plan.transition.is_active());
            let readers = part.context.resolve_readers(part.plan, None).unwrap();
            assert!(readers.current.is_none() && readers.outgoing.is_none());
            assert!(
                part.context
                    .same_source(SourceFeedContext::new(context.binding, readers))
            );
        }
        let crossing = span.slice(100, 512).unwrap();
        assert!(
            crossing
                .context
                .resolve_readers(crossing.plan, None)
                .is_none()
        );
        assert!(
            crossing
                .slice(28, 484)
                .unwrap()
                .context
                .resolve_readers(crossing.slice(28, 484).unwrap().plan, None,)
                .is_some()
        );
    }

    #[test]
    fn held_selection_missing_empty_and_other_or_republished_owners_refuse() {
        let mut stems = held_stems();
        let playback = playback(19, ExplicitSeekMode::Normal, 0.73);
        let mut plan = plan(playback, StemTransition::default());
        plan.selection = StemRenderSelection::for_stems(&stems, 0);
        let context =
            SourceFeedContext::new(context().binding, StemReadSets::new(Some(&stems), None));
        assert!(context.resolve_readers(plan, Some(&stems)).is_some());
        assert!(context.resolve_readers(plan, None).is_none());
        assert!(
            SourceFeedContext::new(context.binding, StemReadSets::default())
                .resolve_readers(plan, Some(&stems))
                .is_none()
        );
        let mut other = stems.clone();
        other.stems[0].samples = other.stems[0].samples.to_vec().into();
        assert!(context.resolve_readers(plan, Some(&other)).is_none());
        other = stems.clone();
        other.complete_set_identity = Arc::new(*stems.complete_set_identity);
        assert!(context.resolve_readers(plan, Some(&other)).is_none());
        other = stems.clone();
        other.publication =
            crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted();
        assert!(context.resolve_readers(plan, Some(&other)).is_none());
        // The same real allocation with another copied identity cannot pass address checks.
        drop(other);
        let owner = Arc::as_ptr(&stems.complete_set_identity);
        *Arc::get_mut(&mut stems.complete_set_identity).unwrap() = [24; 32];
        assert_eq!(owner, Arc::as_ptr(&stems.complete_set_identity));
        assert!(context.resolve_readers(plan, Some(&stems)).is_none());
        *Arc::get_mut(&mut stems.complete_set_identity).unwrap() = [23; 32];
        stems.publication.expected = 2;
        stems.publication.epoch.store(2, Ordering::Release);
        assert!(stems.publication.current());
        assert!(context.resolve_readers(plan, Some(&stems)).is_none());
        let fresh = SourceFeedContext::new(context.binding, StemReadSets::new(Some(&stems), None));
        assert!(fresh.resolve_readers(plan, Some(&stems)).is_some());
        // The original publication stamp is data provenance. Its mutable operation
        // bookkeeping supplies no data lifetime. Authority is separately checked by
        // the owning Native permit; this is only a scalar operation-state perturbation.
        stems.publication.epoch.store(3, Ordering::Release);
        stems.publication.mark_rejected();
        assert!(fresh.resolve_readers(plan, Some(&stems)).is_some());
    }

    fn context() -> SourceFeedContext {
        SourceFeedContext::new(
            ProductiveSourceBinding {
                resident: None,
                source_address: 17,
                sample_count: 4006,
                channels: 2,
                sample_rate_hz: 48_000,
                accepted: None,
            },
            StemReadSets::default(),
        )
    }

    fn playback(start: usize, mode: ExplicitSeekMode, ratio: f64) -> SourcePlayback {
        let mut playback = SourcePlayback::new(start, mode, ratio);
        playback.configure(
            2003,
            FrameRange {
                start: 19,
                end: 1519,
            },
        );
        playback
    }

    fn plan(playback: SourcePlayback, transition: StemTransition) -> SourceReadPlan {
        let position = playback.position();
        SourceReadPlan {
            channels: 2,
            sample_frames: 2003,
            frame_pos: position.frame,
            loop_region: FrameRange {
                start: 19,
                end: 1519,
            },
            loop_period: playback.loop_period(),
            seek_mode: position.seek_mode,
            selection: StemRenderSelection::full_mix(),
            transition,
        }
    }

    fn capture(history: &SourceFeedHistory) -> SourceFeedSnapshot {
        let mut snapshot = SourceFeedSnapshot::new();
        assert!(history.capture_into(&mut snapshot));
        snapshot
    }

    #[test]
    fn actual_off_grid_controls_retain_complete_clocks_and_plans_across_intro_and_tail() {
        for (start, mode) in [
            (7, ExplicitSeekMode::BeforeLoop),
            (1800, ExplicitSeekMode::AfterLoop),
        ] {
            let mut history = SourceFeedHistory::new();
            let mut playback = playback(start, mode, 0.73);
            playback.configure_domain(
                SourceLoopDomain::musical(
                    2003,
                    FrameRange {
                        start: 19,
                        end: 1519,
                    },
                    1500.25,
                )
                .unwrap(),
            );
            let mut transition = StemTransition::start(StemRenderSelection::full_mix(), 80_000);
            let mut observed = Vec::new();
            let mut active = 0;
            let mut partition = 0;
            let boundaries = [137, 911, 2027, 3500];
            let partitions = [31, 113, 7, 311, 53];
            while active < 3500 {
                match active {
                    137 => playback.set_target(1.19),
                    911 => playback.configure_domain(
                        SourceLoopDomain::musical(
                            2003,
                            FrameRange {
                                start: 19,
                                end: 1519,
                            },
                            1499.75,
                        )
                        .unwrap(),
                    ),
                    2027 => playback.set_target(0.61),
                    _ => {}
                }
                let boundary = *boundaries
                    .iter()
                    .find(|boundary| **boundary > active)
                    .unwrap();
                let requested = (boundary - active).min(partitions[partition % partitions.len()]);
                let (frames, ratio) = playback.chunk(requested);
                let plan = plan(playback, transition);
                let feed = SourceFeedSpan {
                    active_start: active as u64,
                    frames,
                    playback,
                    plan,
                    context: context(),
                };
                observed.push(feed);
                history.record_success(playback, plan, context(), frames);
                playback.advance(frames);
                transition.advance_output_frames(frames, ratio);
                active += frames;
                partition += 1;
            }
            let snapshot = capture(&history);
            assert_eq!(snapshot.retained_range(), Some(0..3500));
            assert!(
                snapshot.spans().len() > 3,
                "actual rate epochs must remain distinct"
            );
            assert_eq!(snapshot.spans().next().unwrap().plan.seek_mode, mode);
            assert!(
                snapshot
                    .spans()
                    .any(|span| span.plan.seek_mode == ExplicitSeekMode::Normal)
            );
            for actual in observed {
                let retained = snapshot
                    .spans()
                    .find(|span| {
                        span.active_start <= actual.active_start
                            && span.active_end() >= actual.active_end()
                    })
                    .unwrap();
                let copied = retained
                    .slice(
                        (actual.active_start - retained.active_start) as usize,
                        actual.frames,
                    )
                    .unwrap();
                assert!(copied.playback.matches_exact(&actual.playback));
                assert!(copied.plan.matches_exact(actual.plan));
                assert_eq!(copied.context, actual.context);
            }
        }
    }

    #[test]
    fn coalescing_requires_exact_epoch_plan_and_physical_context_continuation() {
        let mut history = SourceFeedHistory::new();
        let mut playback = playback(19, ExplicitSeekMode::Normal, 1.0);
        let mut plan = plan(playback, StemTransition::default());
        history.record_success(playback, plan, context(), 13);
        advance_source(&mut playback, &mut plan, 13);
        history.record_success(playback, plan, context(), 19);
        assert_eq!(capture(&history).spans().len(), 1);
        advance_source(&mut playback, &mut plan, 19);

        // A seek/reconfigured epoch at the same visible source position is not the old clock.
        let position = playback.position();
        let mut rebased = SourcePlayback::new(position.frame, position.seek_mode, 1.0);
        rebased.configure_domain(plan.domain());
        assert_eq!(rebased.position(), playback.position());
        assert!(!rebased.matches_exact(&playback));
        history.record_success(rebased, plan, context(), 11);
        assert_eq!(capture(&history).spans().len(), 2);
        advance_source(&mut rebased, &mut plan, 11);

        plan.transition = StemTransition::start(StemRenderSelection::full_mix(), 1000);
        plan.transition.advance_fractional(0.25);
        history.record_success(rebased, plan, context(), 7);
        assert_eq!(capture(&history).spans().len(), 3);
        advance_source(&mut rebased, &mut plan, 7);

        let mut changed = context();
        changed.current_stem_owners[0] = 31;
        history.record_success(rebased, plan, changed, 5);
        assert_eq!(capture(&history).spans().len(), 4);
    }

    #[test]
    fn fractional_ramp_history_slices_and_coalescing_preserve_partitioned_state_and_pcm() {
        fn pcm(
            span: SourceFeedSpan,
            sample: &crate::messages::SampleBuffer,
            stems: &PreparedStemSet,
        ) -> Vec<Vec<f32>> {
            let readers = span
                .context
                .resolve_readers(span.plan, Some(stems))
                .unwrap();
            let mut output = vec![vec![f32::NAN; span.frames]; sample.channels];
            span.plan
                .fill_fractional_buffers_checked_with_readers(
                    sample,
                    readers,
                    &span.playback,
                    &mut output,
                    span.frames,
                )
                .unwrap();
            assert!(output.iter().flatten().all(|value| value.is_finite()));
            output
        }

        fn assert_same(
            actual: SourceFeedSpan,
            expected: SourceFeedSpan,
            sample: &crate::messages::SampleBuffer,
            stems: &PreparedStemSet,
        ) {
            assert_eq!(actual.active_start, expected.active_start);
            assert_eq!(actual.frames, expected.frames);
            assert_eq!(actual.context, expected.context);
            assert!(actual.playback.matches_exact(&expected.playback));
            assert!(actual.plan.matches_exact(expected.plan));
            assert_eq!(pcm(actual, sample, stems), pcm(expected, sample, stems));
        }

        let stems = held_stems();
        let sample = crate::messages::SampleBuffer {
            samples: stems.reference_samples.clone(),
            channels: stems.channels,
            residency: stems.reference_residency.clone(),
        };
        let mut playback = playback(19, ExplicitSeekMode::Normal, 0.9200000000000003);
        let mut plan = plan(
            playback,
            StemTransition::start(StemRenderSelection::full_mix(), 128),
        );
        // FullMix to an empty stem mask makes every ramp gain observable in PCM.
        plan.selection = StemRenderSelection::for_stems(&stems, 0);
        let context = SourceFeedContext::new(
            ProductiveSourceBinding::new(&sample, 48_000, None),
            plan.required_read_sets(StemReadSets::new(Some(&stems), None)),
        );
        let mut history = SourceFeedHistory::new();
        history.record_success(playback, plan, context, 43);
        advance_source(&mut playback, &mut plan, 43);
        let span = SourceFeedSpan {
            active_start: 43,
            frames: 97,
            playback,
            plan,
            context,
        };
        assert_same(span.slice(0, 97).unwrap(), span, &sample, &stems);
        assert!(span.slice(0, 0).is_none());
        assert!(span.slice(97, 1).is_none());

        let direct_endpoint = span.slice(17, 1).unwrap();
        let nested_endpoint = span.slice(3, 94).unwrap().slice(14, 1).unwrap();
        assert_same(direct_endpoint, nested_endpoint, &sample, &stems);
        let whole_pcm = pcm(span.slice(0, 17).unwrap(), &sample, &stems);
        let mut split_pcm = pcm(span.slice(0, 3).unwrap(), &sample, &stems);
        let remainder_pcm = pcm(span.slice(3, 14).unwrap(), &sample, &stems);
        for (channel, remainder) in split_pcm.iter_mut().zip(remainder_pcm) {
            channel.extend(remainder);
        }
        assert_eq!(whole_pcm, split_pcm);
        assert!(whole_pcm.iter().flatten().any(|value| *value != 0.0));

        let mut observed = Vec::new();
        for frames in [3, 14] {
            observed.push(SourceFeedSpan {
                active_start: history.active_frames,
                frames,
                playback,
                plan,
                context,
            });
            history.record_success(playback, plan, context, frames);
            advance_source(&mut playback, &mut plan, frames);
        }
        assert!(playback.matches_exact(&direct_endpoint.playback));
        assert!(plan.matches_exact(direct_endpoint.plan));
        let coalesced = capture(&history);
        assert_eq!(coalesced.spans().len(), 1);
        assert_eq!(coalesced.spans().next().unwrap().frames, 60);
        assert_same(
            coalesced.spans().next().unwrap().slice(43, 17).unwrap(),
            span.slice(0, 17).unwrap(),
            &sample,
            &stems,
        );

        // These are actual off-grid control epochs. They must remain distinct in
        // the journal while arbitrary subdivisions within one epoch may coalesce.
        let mut active = 60;
        let controls = [(67, 1.17), (81, 0.67), (229, 0.81)];
        let partitions = [17, 3, 14, 29, 1, 53];
        let mut partition = 0;
        while active < 629 {
            if let Some((_, target)) = controls.iter().find(|(at, _)| *at == active) {
                playback.set_target(*target);
            }
            let boundary = controls
                .iter()
                .map(|(at, _)| *at)
                .find(|at| *at > active)
                .unwrap_or(629);
            let requested = (boundary - active).min(partitions[partition % partitions.len()]);
            let (frames, _) = playback.chunk(requested);
            let position = playback.position();
            plan.frame_pos = position.frame;
            plan.seek_mode = position.seek_mode;
            let actual = SourceFeedSpan {
                active_start: active as u64,
                frames,
                playback,
                plan,
                context,
            };
            assert_same(actual.slice(0, frames).unwrap(), actual, &sample, &stems);
            assert!(actual.slice(0, 0).is_none());
            observed.push(actual);
            history.record_success(playback, plan, context, frames);
            advance_source(&mut playback, &mut plan, frames);
            active += frames;
            partition += 1;
        }
        let snapshot = capture(&history);
        assert_eq!(snapshot.retained_range(), Some(0..629));
        assert!(snapshot.spans().len() >= 4);
        for actual in observed {
            let retained = snapshot
                .spans()
                .find(|retained| {
                    retained.active_start <= actual.active_start
                        && retained.active_end() >= actual.active_end()
                })
                .unwrap();
            let copied = retained
                .slice(
                    (actual.active_start - retained.active_start) as usize,
                    actual.frames,
                )
                .unwrap();
            assert_same(copied, actual, &sample, &stems);
        }
    }

    #[test]
    fn changed_future_fences_old_capture_without_discarding_actual_past_or_origin() {
        let mut history = SourceFeedHistory::new();
        let mut playback = playback(19, ExplicitSeekMode::Normal, 0.73);
        let mut plan = plan(playback, StemTransition::default());
        history.record_success(playback, plan, context(), 1237);
        let before = capture(&history);
        history.invalidate_captures();
        let after_control = capture(&history);
        assert!(!before.is_current());
        assert!(after_control.is_current());
        assert_eq!(after_control.retained_range(), Some(0..1237));
        assert_eq!(after_control.active_origin(), before.active_origin());
        assert_eq!(after_control.active_frames(), before.active_frames());
        let retained = after_control.spans().next().unwrap();
        assert!(retained.playback.matches_exact(&playback));
        assert!(retained.plan.matches_exact(plan));

        advance_source(&mut playback, &mut plan, 1237);
        history.record_success(playback, plan, context(), 79);
        let later = capture(&history);
        assert!(
            after_control.is_current(),
            "ordinary reads keep a frozen future valid"
        );
        assert_eq!(later.retained_range(), Some(0..1316));
        assert_eq!(later.active_origin(), 0);
        assert_eq!(later.spans().len(), 1);
    }

    #[test]
    fn refusal_is_a_fenced_gap_in_the_monotonic_attempted_clock() {
        let mut history = SourceFeedHistory::new();
        let mut playback = playback(19, ExplicitSeekMode::Normal, 0.73);
        let mut plan = plan(playback, StemTransition::default());
        history.record_success(playback, plan, context(), 41);
        let before = capture(&history);
        history.record_gap(37);
        let missing = capture(&history);
        assert_eq!(missing.retained_range(), None);
        assert_eq!(missing.active_frames(), 78);
        assert_eq!(missing.active_origin(), 0);
        assert!(!before.is_current());
        assert!(missing.is_current());

        advance_source(&mut playback, &mut plan, 78);
        history.record_success(playback, plan, context(), 23);
        let after = capture(&history);
        assert_eq!(after.retained_range(), Some(78..101));
        assert_eq!(after.active_frames(), 101);
        assert_eq!(
            before.retained_range(),
            Some(0..41),
            "snapshots own their scalar copy"
        );

        history.reset();
        let reset = capture(&history);
        assert_eq!(reset.active_origin(), 101);
        assert_eq!(reset.active_frames(), 101);
        assert_eq!(reset.retained_range(), None);
        assert!(!after.is_current());
    }

    #[test]
    fn frame_retention_trims_the_exact_oldest_epoch_and_a_single_oversized_feed() {
        let mut history = SourceFeedHistory::new();
        let mut playback = playback(19, ExplicitSeekMode::Normal, 0.73);
        let initial = playback;
        let mut read_plan = plan(playback, StemTransition::default());
        history.record_success(playback, read_plan, context(), 200_000);
        let frozen = capture(&history);
        advance_source(&mut playback, &mut read_plan, 200_000);
        history.record_success(playback, read_plan, context(), 100_000);
        let retained = capture(&history);
        assert_eq!(retained.retained_range(), Some(37_856..300_000));
        assert_eq!(retained.spans().len(), 1);
        let first = retained.spans().next().unwrap();
        let mut expected = initial;
        expected.advance(37_856);
        assert!(first.playback.matches_exact(&expected));
        assert_eq!(first.frames, SOURCE_FEED_HISTORY_MAX_FRAMES);
        assert!(
            frozen.is_current(),
            "successful continuation does not fence an immutable copy"
        );
        assert_eq!(frozen.retained_range(), Some(0..200_000));

        let mut oversized = SourceFeedHistory::new();
        oversized.record_success(
            initial,
            plan(initial, StemTransition::default()),
            context(),
            SOURCE_FEED_HISTORY_MAX_FRAMES + 123,
        );
        let retained = capture(&oversized);
        assert_eq!(retained.retained_range(), Some(123..262_267));
        let mut expected = initial;
        expected.advance(123);
        assert!(
            retained
                .spans()
                .next()
                .unwrap()
                .playback
                .matches_exact(&expected)
        );
    }

    #[test]
    fn span_capacity_evicts_only_the_oldest_actual_spans_without_fabricating_history() {
        let mut history = SourceFeedHistory::new();
        let mut playback = playback(19, ExplicitSeekMode::Normal, 1.0);
        let mut plan = plan(playback, StemTransition::default());
        for frame in 0..SOURCE_FEED_HISTORY_MAX_SPANS + 17 {
            let mut context = context();
            context.binding.source_address += frame;
            history.record_success(playback, plan, context, 1);
            advance_source(&mut playback, &mut plan, 1);
        }
        let snapshot = capture(&history);
        assert_eq!(snapshot.spans().len(), SOURCE_FEED_HISTORY_MAX_SPANS);
        assert_eq!(snapshot.retained_range(), Some(17..1041));
        for (offset, span) in snapshot.spans().enumerate() {
            assert_eq!(span.active_start, (17 + offset) as u64);
            assert_eq!(span.frames, 1);
            assert_eq!(span.context.binding.source_address, 34 + offset);
        }
    }

    #[test]
    fn capture_reuses_the_box_and_never_replaces_a_filled_snapshot_epoch_owner() {
        let mut history = SourceFeedHistory::new();
        let playback = playback(19, ExplicitSeekMode::Normal, 1.0);
        let plan = plan(playback, StemTransition::default());
        history.record_success(playback, plan, context(), 5);
        let mut snapshot = SourceFeedSnapshot::new();
        let storage = snapshot.spans.as_ptr();
        assert!(history.capture_into(&mut snapshot));
        let epoch = snapshot.captured_epoch();
        assert!(Arc::ptr_eq(
            snapshot.continuity_owner().unwrap(),
            &history.continuity_owner
        ));
        assert_eq!(Arc::strong_count(&history.continuity_owner), 2);
        history.record_gap(3);
        assert!(!history.capture_into(&mut snapshot));
        assert_eq!(snapshot.captured_epoch(), epoch);
        assert_eq!(snapshot.retained_range(), Some(0..5));
        assert_eq!(Arc::strong_count(&history.continuity_owner), 2);
        assert!(!snapshot.is_current());
        snapshot.clear_for_reuse();
        assert_eq!(Arc::strong_count(&history.continuity_owner), 1);
        assert!(history.capture_into(&mut snapshot));
        assert_eq!(snapshot.spans.as_ptr(), storage);
        assert!(snapshot.is_current());
        assert_eq!(snapshot.active_frames(), 8);
    }

    #[test]
    fn guard_keeps_original_source_and_voice_fences_after_snapshot_storage_is_reused() {
        let mut history = SourceFeedHistory::new();
        let playback = playback(19, ExplicitSeekMode::Normal, 1.0);
        history.record_success(
            playback,
            plan(playback, StemTransition::default()),
            context(),
            5,
        );
        let lifetime = Arc::new(AtomicU64::new(11));
        let mut snapshot = capture(&history);
        let storage = snapshot.spans.as_ptr();
        let guard = snapshot.guard(&lifetime, 11).unwrap();
        let retained = guard.clone();
        assert!(guard.is_current());
        snapshot.clear_for_reuse();
        assert!(!snapshot.is_current());
        assert!(snapshot.guard(&lifetime, 11).is_none());
        assert!(guard.is_current());

        let next_history = SourceFeedHistory::new();
        assert!(next_history.capture_into(&mut snapshot));
        assert_eq!(snapshot.spans.as_ptr(), storage);
        assert!(snapshot.is_current());
        assert!(guard.is_current());
        history.invalidate_captures();
        assert!(!guard.is_current());
        assert!(!retained.is_current());
        assert!(
            snapshot.is_current(),
            "recycling cannot relabel the old guard"
        );

        let next_guard = snapshot.guard(&lifetime, 11).unwrap();
        lifetime.store(0, Ordering::Release);
        assert!(!next_guard.is_current());
        lifetime.store(12, Ordering::Release);
        assert!(
            !next_guard.is_current(),
            "another voice generation cannot revive it"
        );
    }

    #[test]
    fn guard_refuses_absent_stale_or_invalid_voice_generation() {
        let history = SourceFeedHistory::new();
        let lifetime = Arc::new(AtomicU64::new(11));
        let mut snapshot = SourceFeedSnapshot::new();
        assert!(snapshot.guard(&lifetime, 11).is_none());
        assert!(history.capture_into(&mut snapshot));
        assert!(snapshot.guard(&lifetime, 0).is_none());
        assert!(snapshot.guard(&lifetime, u64::MAX).is_none());
        assert!(snapshot.guard(&lifetime, 12).is_none());
        lifetime.store(0, Ordering::Release);
        assert!(snapshot.guard(&lifetime, 0).is_none());
        lifetime.store(u64::MAX, Ordering::Release);
        assert!(snapshot.guard(&lifetime, u64::MAX).is_none());
        lifetime.store(11, Ordering::Release);
        assert!(snapshot.guard(&lifetime, 11).is_some());
        history.invalidate_captures();
        assert!(snapshot.guard(&lifetime, 11).is_none());
    }

    #[test]
    fn physical_comparison_does_not_turn_historical_accepted_data_into_authority() {
        let initial = context();
        let mut changed = initial;
        changed.binding.accepted = Some(AcceptedTimingProjection {
            revision: [1; 32],
            period_seconds: 0.5,
            origin_seconds: -0.0,
            sample_rate_hz: 48_000,
            publication_epoch: 7,
        });
        assert!(initial.same_source(changed));
        assert_ne!(initial, changed);
        changed.outgoing_stem_owners[5] = 91;
        assert!(!initial.same_source(changed));
    }

    #[test]
    fn reused_stem_addresses_do_not_replace_original_pcm_or_publication_generation() {
        let mut original = context();
        original.current_stem_owners = [17; STEM_OWNER_MARKER_COUNT];
        let identity = StemFeedIdentity {
            complete_pcm: [3; 32],
            publication_owner: 41,
            publication_generation: 7,
            ..stem_generation(Some(&held_stems())).unwrap()
        };
        original.current_stem_generation = Some(identity);
        let mut reused = original;
        reused.current_stem_generation = Some(StemFeedIdentity {
            complete_pcm: [4; 32],
            ..identity
        });
        assert!(
            !original.same_source(reused),
            "identical addresses can contain different PCM"
        );
        reused.current_stem_generation = Some(StemFeedIdentity {
            publication_generation: 8,
            ..identity
        });
        assert!(
            !original.same_source(reused),
            "new publication is not the old generation"
        );
        reused.current_stem_generation = Some(StemFeedIdentity {
            publication_owner: 42,
            ..identity
        });
        assert!(
            !original.same_source(reused),
            "equal counters from another owner do not match"
        );
        assert!(original.same_source(original));
    }

    #[test]
    fn counter_exhaustion_fences_and_epoch_exhaustion_permanently_refuses_capture() {
        let mut history = SourceFeedHistory::new();
        let before = capture(&history);
        history.active_frames = u64::MAX - 3;
        let playback = playback(19, ExplicitSeekMode::Normal, 1.0);
        history.record_success(
            playback,
            plan(playback, StemTransition::default()),
            context(),
            5,
        );
        let restarted = capture(&history);
        assert!(!before.is_current());
        assert_eq!(restarted.active_origin(), 0);
        assert_eq!(restarted.retained_range(), Some(0..5));
        history.continuity_owner.store(u64::MAX, Ordering::Release);
        history.reset();
        let mut unavailable = SourceFeedSnapshot::new();
        assert!(!history.capture_into(&mut unavailable));
        history.reset();
        assert!(!history.capture_into(&mut unavailable));
    }

    /// Opt-in host resource characterization. This measures scalar capture only; it does not
    /// establish callback deadlines, worker replay bounds, Native behavior or device acceptance.
    #[test]
    #[ignore = "host-specific scalar snapshot resource profile; run explicitly with --nocapture"]
    fn source_feed_history_resource_profile() {
        use crate::audio_engine::constants::MAX_VOICES;
        use std::hint::black_box;
        use std::mem::size_of;
        use std::time::Instant;

        const ACTION_WORKS: usize = 64;
        const WARMUP_CAPTURES: usize = 16;
        const MEASURED_CAPTURES: usize = 512;
        let mut history = SourceFeedHistory::new();
        let mut snapshot = SourceFeedSnapshot::new();
        let mut durations_ns = [0_u128; MEASURED_CAPTURES];
        let slot_bytes = size_of::<Option<SourceFeedSpan>>();
        let journal_box_bytes = history.spans.len() * slot_bytes;
        let action_box_bytes = snapshot.spans.len() * slot_bytes;
        let total_box_bytes = MAX_VOICES * journal_box_bytes + ACTION_WORKS * action_box_bytes;
        let total_inline_bytes = MAX_VOICES * size_of::<SourceFeedHistory>()
            + ACTION_WORKS * size_of::<SourceFeedSnapshot>();

        for iteration in 0..WARMUP_CAPTURES + MEASURED_CAPTURES {
            // All allocation, source-journal refill and snapshot Arc retirement are outside
            // the measured interval. Different scalar owners prevent accidental coalescing.
            snapshot.clear_for_reuse();
            history.reset();
            let mut playback = playback(19, ExplicitSeekMode::Normal, 0.73);
            let mut transition = StemTransition::start(StemRenderSelection::full_mix(), 80_000);
            for span_index in 0..SOURCE_FEED_HISTORY_MAX_SPANS {
                let target = 0.53 + ((span_index * 13 + iteration) % 83) as f64 * 0.017;
                playback.set_target(target);
                let (frames, ratio) = playback.chunk(1 + (span_index + iteration) % 29);
                let read_plan = plan(playback, transition);
                let mut context = context();
                context.binding.source_address += span_index + iteration;
                context.current_stem_owners = std::array::from_fn(|owner| {
                    1 + iteration * SOURCE_FEED_HISTORY_MAX_SPANS + span_index * 7 + owner
                });
                history.record_success(playback, read_plan, context, frames);
                playback.advance(frames);
                transition.advance_output_frames(frames, ratio);
            }
            assert_eq!(history.len, SOURCE_FEED_HISTORY_MAX_SPANS);

            let started = Instant::now();
            let copied = black_box(&history).capture_into(black_box(&mut snapshot));
            let elapsed_ns = started.elapsed().as_nanos();

            assert!(copied);
            assert_eq!(snapshot.spans().len(), SOURCE_FEED_HISTORY_MAX_SPANS);
            assert!(snapshot.is_current());
            // Consume every copied scalar span after timing, keeping the copy observable in
            // optimized builds without adding a checksum traversal to its measured cost.
            black_box(snapshot.spans().fold(0_u64, |checksum, span| {
                checksum
                    .wrapping_add(span.active_start)
                    .wrapping_add(span.frames as u64)
                    .wrapping_add(span.plan.frame_pos as u64)
                    .wrapping_add(span.playback.tempo_ratio().to_bits())
                    .wrapping_add(span.context.current_stem_owners[0] as u64)
            }));
            if iteration >= WARMUP_CAPTURES {
                durations_ns[iteration - WARMUP_CAPTURES] = elapsed_ns;
            }
        }

        snapshot.clear_for_reuse();
        durations_ns.sort_unstable();
        let median_ns = durations_ns[MEASURED_CAPTURES / 2];
        let p99_ns = durations_ns[(MEASURED_CAPTURES * 99).div_ceil(100) - 1];
        eprintln!(
            "source-feed scalar resource profile: profile={} span_bytes={} option_span_bytes={} \
             spans={} journal_box_bytes={} action_box_bytes={} voice_count={} action_count={} \
             total_box_bytes={} journal_inline_bytes={} action_inline_bytes={} \
             total_inline_bytes={} (excludes allocator/Arc overhead and other work storage)",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            size_of::<SourceFeedSpan>(),
            slot_bytes,
            SOURCE_FEED_HISTORY_MAX_SPANS,
            journal_box_bytes,
            action_box_bytes,
            MAX_VOICES,
            ACTION_WORKS,
            total_box_bytes,
            size_of::<SourceFeedHistory>(),
            size_of::<SourceFeedSnapshot>(),
            total_inline_bytes,
        );
        eprintln!(
            "source-feed capture-only host timing: measured={} warmup={} copied_spans={} \
             min_ns={} median_ns={} p99_ns={} max_ns={} \
             (no callback deadline, Native replay or device acceptance claim)",
            MEASURED_CAPTURES,
            WARMUP_CAPTURES,
            SOURCE_FEED_HISTORY_MAX_SPANS,
            durations_ns[0],
            median_ns,
            p99_ns,
            durations_ns[MEASURED_CAPTURES - 1],
        );
    }
}
