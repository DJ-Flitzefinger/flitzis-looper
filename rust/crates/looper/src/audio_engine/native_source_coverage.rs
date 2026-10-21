//! Bounded authority for the actual loop/intro/tail taps supplied to native history.
//!
//! Normal phases address physical loop knots; Intro additionally reads the real prefix and Tail
//! the real suffix before returning to those knots. Coverage holds their whole physical union,
//! including both virtual seam taps; the native preparation horizon is not a guessed halo.
//! Source/window/history permissions remain with their existing independent owners.

use super::constant_timing::AcceptedTimingProjection;
use super::productive_source_history::ProductiveSourceBinding;
use super::source_playback::SourcePlayback;
use super::source_reader::{
    ExplicitSeekMode, FrameRange, SourceLoopDomain, SourceReadPlan, StemReadSets, StemTransition,
    prepared_stem_set_matches_sample,
};
use crate::messages::{
    CompleteSourceIdentity, PreparedStemSet, ResidentBinding, STEM_BUFFER_COUNT, SampleBuffer,
};
use std::sync::Arc;

/// The whole advertised complete source is physically held, rather than merely described.
pub(crate) fn complete_view_available(sample: &SampleBuffer) -> bool {
    sample_layout_available(sample)
        && sample.resident_start() == 0
        && sample.resident_end() == sample.frame_count()
}

/// Current Normal-loop input covers every physical tap, including shifted phases.
/// Finite views retain their separately captured exact Native coverage and permits;
/// this check never grants window ownership or authorizes a selection transition.
pub(crate) fn reset_source_feed_available(
    sample: &SampleBuffer,
    readers: StemReadSets<'_>,
    sample_rate_hz: u32,
    accepted: Option<AcceptedTimingProjection>,
    plan: SourceReadPlan,
    playback: &SourcePlayback,
) -> bool {
    plan.seek_mode == ExplicitSeekMode::Normal
        && !plan.transition.is_active()
        && normal_loop_feed_available_with_readers(
            sample,
            readers,
            sample_rate_hz,
            accepted,
            plan,
            playback,
        )
        && std::iter::once(plan.selection)
            .chain(plan.transition.required_from_selection())
            .all(|selection| {
                selection.required_stem_hash().is_none()
                    || readers
                        .for_selection(selection)
                        .is_some_and(|stems| stems.publication.current())
            })
}

fn sample_layout_available(sample: &SampleBuffer) -> bool {
    if sample.channels == 0
        || sample.samples.is_empty()
        || !sample.samples.len().is_multiple_of(sample.channels)
    {
        return false;
    }
    let frames = sample.samples.len() / sample.channels;
    sample.residency.as_ref().is_none_or(|view| {
        view.source.channels == sample.channels
            && view.source.source_zero_frame == 0
            && view.source.sample_rate_hz != 0
            && view.window_revision != 0
            && view
                .source
                .frame_count
                .checked_mul(sample.channels)
                .is_some()
            && view.start_frame.checked_add(frames).is_some_and(|end| {
                view.start_frame < end
                    && end <= view.source.frame_count
                    && (view.context.permits_finite_range()
                        || (view.start_frame == 0 && end == view.source.frame_count))
            })
    })
}

/// Physical union of every possible future tap in this source context.
/// A contiguous Intro view holds the prefix through the loop end; a Tail view holds the loop
/// start through the actual source end. Native lookahead and later smoothing cannot escape it.
pub(crate) fn source_context_range(
    sample_frames: usize,
    region: FrameRange,
    mode: ExplicitSeekMode,
) -> Option<FrameRange> {
    if region.start >= region.end || region.end > sample_frames {
        return None;
    }
    Some(match mode {
        ExplicitSeekMode::Normal => region,
        ExplicitSeekMode::BeforeLoop => FrameRange {
            start: 0,
            end: region.end,
        },
        ExplicitSeekMode::AfterLoop => FrameRange {
            start: region.start,
            end: sample_frames,
        },
    })
}

/// Prove all possible source-context tap addresses from the real held physical range.
/// A resident context tag without a valid layout and both physical bounds is insufficient.
/// The existing name is retained for the NormalLoop preparation and selection callers.
pub(crate) fn normal_loop_context_available(
    sample: &SampleBuffer,
    region: FrameRange,
    mode: ExplicitSeekMode,
) -> bool {
    sample_layout_available(sample)
        && source_context_range(sample.frame_count(), region, mode).is_some_and(|required| {
            sample.resident_start() <= required.start && sample.resident_end() >= required.end
        })
}

/// Check the actual copied source clock, selection and timing against all-phase finite coverage.
/// NativeHistoryPermit separately checks the live source/window/history authority.
pub(crate) fn normal_loop_feed_available(
    sample: &SampleBuffer,
    stems: Option<&PreparedStemSet>,
    sample_rate_hz: u32,
    accepted: Option<AcceptedTimingProjection>,
    plan: SourceReadPlan,
    playback: &SourcePlayback,
) -> bool {
    normal_loop_feed_available_with_readers(
        sample,
        StemReadSets::new(stems, None),
        sample_rate_hz,
        accepted,
        plan,
        playback,
    )
}

/// Both exact complete-set owners must cover every real side of a live handover.
pub(crate) fn normal_loop_feed_available_with_readers(
    sample: &SampleBuffer,
    readers: StemReadSets<'_>,
    sample_rate_hz: u32,
    accepted: Option<AcceptedTimingProjection>,
    plan: SourceReadPlan,
    playback: &SourcePlayback,
) -> bool {
    if !normal_loop_context_available(sample, plan.loop_region, plan.seek_mode)
        || plan.channels != sample.channels
        || plan.sample_frames != sample.frame_count()
        || !sample.valid_residency(sample_rate_hz, plan.channels)
        || !playback.matches_domain(plan.domain())
        || !playback.tempo_ratio().is_finite()
        || !playback.rate_target().is_finite()
    {
        return false;
    }
    if let Some(period) = plan.loop_period
        && SourceLoopDomain::musical(plan.sample_frames, plan.loop_region, period).is_none()
    {
        return false;
    }
    let position = playback.position();
    if position.seek_mode != plan.seek_mode
        || position.frame != plan.frame_pos
        || !position.fraction.is_finite()
        || !(0.0..1.0).contains(&position.fraction)
        || match position.seek_mode {
            ExplicitSeekMode::Normal => false,
            ExplicitSeekMode::BeforeLoop => position.frame >= plan.loop_region.start,
            ExplicitSeekMode::AfterLoop => {
                position.frame < plan.loop_region.end || position.frame >= plan.sample_frames
            }
        }
    {
        return false;
    }
    // Both sides are real inputs until the source-frame ramp ends. A FullMix target
    // cannot stand in for an unavailable outgoing stem selection (including an empty mask).
    for selection in
        std::iter::once(plan.selection).chain(plan.transition.required_from_selection())
    {
        let Some(hash) = selection.required_stem_hash() else {
            continue;
        };
        let Some(stems) = readers.for_selection(selection) else {
            return false;
        };
        if hash == 0
            || stems.source_version_hash != hash
            || stems.accepted_timing != accepted
            || !prepared_stem_set_matches_sample(
                stems,
                sample,
                plan.channels,
                sample_rate_hz as f32,
                plan.sample_frames,
            )
            || !stems.stems.iter().all(|component| {
                component.valid_residency(sample_rate_hz, plan.channels)
                    && normal_loop_context_available(component, plan.loop_region, plan.seek_mode)
            })
        {
            return false;
        }
    }
    // Use the same address calculation as actual reads, including virtual P > H positions,
    // Intro's prefix-to-loop seam and Tail's source-end-to-loop return. Whole-context coverage
    // proves every later feed; checking both actual taps also fences this copied checkpoint.
    let Some(required) = source_context_range(plan.sample_frames, plan.loop_region, plan.seek_mode)
    else {
        return false;
    };
    let taps = plan.fractional_taps(position.fraction);
    taps.left >= required.start
        && taps.left < required.end
        && taps
            .right
            .is_none_or(|right| right >= required.start && right < required.end)
}

#[derive(Clone)]
struct StemCoverage {
    complete_set_identity: Arc<[u8; 32]>,
    complete_pcm: [u8; 32],
    publication_owner: Arc<std::sync::atomic::AtomicU64>,
    publication_generation: u64,
    reference_address: usize,
    bindings: [Option<ResidentBinding>; STEM_BUFFER_COUNT],
    addresses: [usize; STEM_BUFFER_COUNT],
}

impl StemCoverage {
    fn capture(stems: &PreparedStemSet) -> Self {
        Self {
            complete_set_identity: stems.complete_set_identity.clone(),
            complete_pcm: *stems.complete_set_identity,
            publication_owner: stems.publication.epoch.clone(),
            publication_generation: stems.publication.expected,
            reference_address: stems.reference_samples.as_ptr() as usize,
            bindings: std::array::from_fn(|index| stems.stems[index].resident_binding()),
            addresses: std::array::from_fn(|index| stems.stems[index].samples.as_ptr() as usize),
        }
    }

    fn matches(&self, stems: &PreparedStemSet, exact_window: bool) -> bool {
        Arc::ptr_eq(&self.complete_set_identity, &stems.complete_set_identity)
            && self.complete_pcm == *stems.complete_set_identity
            && Arc::ptr_eq(&self.publication_owner, &stems.publication.epoch)
            && self.publication_generation == stems.publication.expected
            // Original publication owner/value identifies admitted data. Mutable receipt
            // counters/status are not its lifetime; the Native permit proves authority.
            && (!exact_window
                || (self.reference_address == stems.reference_samples.as_ptr() as usize
                    && stems.stems.iter().enumerate().all(|(index, component)| {
                        self.bindings[index] == component.resident_binding()
                            && self.addresses[index] == component.samples.as_ptr() as usize
                    })))
    }
}

/// Immutable initial trajectory and real held owners for one finite native history request.
/// The request's existing advanced playback/plan checkpoint performs exact adoption matching.
#[derive(Clone)]
pub(crate) struct NormalLoopCoverage {
    source: Arc<CompleteSourceIdentity>,
    binding: ProductiveSourceBinding,
    resident: ResidentBinding,
    plan: SourceReadPlan,
    playback: SourcePlayback,
    incoming_stems: Option<StemCoverage>,
    outgoing_stems: Option<StemCoverage>,
    #[cfg(test)]
    pub(crate) prepared_taps: super::source_reader::TapReadObservation,
}

impl NormalLoopCoverage {
    pub(crate) fn capture(
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        sample_rate_hz: u32,
        accepted: Option<AcceptedTimingProjection>,
        plan: SourceReadPlan,
        playback: &SourcePlayback,
    ) -> Option<Self> {
        Self::capture_with_readers(
            sample,
            StemReadSets::new(stems, None),
            sample_rate_hz,
            accepted,
            plan,
            playback,
        )
    }

    /// Capture both logical selection roles independently of their physical reader slots.
    /// One real held set may satisfy both roles, including a selected empty mask.
    pub(crate) fn capture_with_readers(
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        sample_rate_hz: u32,
        accepted: Option<AcceptedTimingProjection>,
        plan: SourceReadPlan,
        playback: &SourcePlayback,
    ) -> Option<Self> {
        if !normal_loop_feed_available_with_readers(
            sample,
            readers,
            sample_rate_hz,
            accepted,
            plan,
            playback,
        ) {
            return None;
        }
        Some(Self {
            source: sample.residency.as_ref()?.source.clone(),
            binding: ProductiveSourceBinding::new(sample, sample_rate_hz, accepted),
            resident: sample.resident_binding()?,
            plan,
            playback: *playback,
            incoming_stems: readers
                .for_selection(plan.selection)
                .map(StemCoverage::capture),
            outgoing_stems: plan
                .transition
                .required_from_selection()
                .and_then(|selection| readers.for_selection(selection))
                .map(StemCoverage::capture),
            #[cfg(test)]
            prepared_taps: super::source_reader::TapReadObservation::default(),
        })
    }

    pub(crate) fn matches(
        &self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        sample_rate_hz: u32,
        accepted: Option<AcceptedTimingProjection>,
        plan: SourceReadPlan,
        playback: &SourcePlayback,
        exact_window: bool,
    ) -> bool {
        self.matches_with_readers(
            sample,
            StemReadSets::new(stems, None),
            sample_rate_hz,
            accepted,
            plan,
            playback,
            exact_window,
        )
    }

    /// Pending preparation retains its original transition contract. Exact adoption also
    /// compares the advanced plan/clock outside this finite physical-coverage proof.
    pub(crate) fn matches_with_readers(
        &self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        sample_rate_hz: u32,
        accepted: Option<AcceptedTimingProjection>,
        plan: SourceReadPlan,
        playback: &SourcePlayback,
        exact_window: bool,
    ) -> bool {
        self.plan
            .transition
            .matches_source_contract(plan.transition)
            && self.playback.matches_rate_target(playback)
            && self.matches_reader_data(
                sample,
                readers,
                sample_rate_hz,
                accepted,
                plan,
                playback,
                exact_window,
            )
    }

    /// An originally active adopted ramp needs the caller's independently advanced scalar
    /// transition proof, including after completion. Missing proof never relaxes its contract.
    pub(crate) fn matches_effective_with_readers(
        &self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        sample_rate_hz: u32,
        accepted: Option<AcceptedTimingProjection>,
        plan: SourceReadPlan,
        playback: &SourcePlayback,
        expected_transition: Option<StemTransition>,
    ) -> bool {
        self.playback.matches_rate_target(playback)
            && self.effective_transition_matches(plan, expected_transition)
            && self.matches_reader_data(
                sample,
                readers,
                sample_rate_hz,
                accepted,
                plan,
                playback,
                false,
            )
    }

    fn effective_transition_matches(
        &self,
        plan: SourceReadPlan,
        expected_transition: Option<StemTransition>,
    ) -> bool {
        if self.plan.transition.is_active() {
            expected_transition.is_some_and(|expected| expected.matches_exact(plan.transition))
        } else {
            self.plan
                .transition
                .matches_source_contract(plan.transition)
        }
    }

    /// Only the already productive Native owner may use this after proving exact next-phase
    /// continuity. Current physical taps and all source/reader/timing guards still apply.
    pub(crate) fn matches_productive_with_readers(
        &self,
        feed: &super::stretch_processor::ProductiveSourceFeed<'_>,
        expected_transition: Option<StemTransition>,
    ) -> bool {
        self.effective_transition_matches(feed.plan, expected_transition)
            && self.matches_reader_data(
                feed.sample,
                StemReadSets::new(feed.stems, feed.outgoing_stems),
                feed.sample_rate_hz,
                feed.accepted,
                feed.plan,
                feed.playback,
                false,
            )
    }

    /// Caller proves transition continuity before a completed outgoing role may disappear.
    /// Every role still read by this plan retains its own captured physical identity.
    fn matches_reader_data(
        &self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        sample_rate_hz: u32,
        accepted: Option<AcceptedTimingProjection>,
        plan: SourceReadPlan,
        playback: &SourcePlayback,
        exact_window: bool,
    ) -> bool {
        let matches_role =
            |owner: Option<&StemCoverage>, stems: Option<&PreparedStemSet>| match (owner, stems) {
                (None, None) => true,
                (Some(owner), Some(stems)) => owner.matches(stems, exact_window),
                _ => false,
            };
        let binding = ProductiveSourceBinding::new(sample, sample_rate_hz, accepted);
        sample
            .residency
            .as_ref()
            .is_some_and(|view| Arc::ptr_eq(&self.source, &view.source))
            && self.binding.same_source(binding)
            && self.binding.accepted == accepted
            && (!exact_window || sample.resident_binding() == Some(self.resident))
            && self.plan.matches_source_contract(plan)
            && normal_loop_feed_available_with_readers(
                sample,
                readers,
                sample_rate_hz,
                accepted,
                plan,
                playback,
            )
            && matches_role(
                self.incoming_stems.as_ref(),
                readers.for_selection(plan.selection),
            )
            && plan
                .transition
                .required_from_selection()
                .is_none_or(|selection| {
                    matches_role(
                        self.outgoing_stems.as_ref(),
                        readers.for_selection(selection),
                    )
                })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
    use crate::audio_engine::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
    use crate::audio_engine::native_history_permit::NativeHistoryContext;
    use crate::audio_engine::prepared_native_history::{NativeAdapterState, NativeHistoryRequest};
    use crate::audio_engine::prepared_source::PreparedSourcePermit;
    use crate::audio_engine::rubberband_backend::{
        RubberBandLiveShifter, pitch_scale_for_tempo_ratio,
    };
    use crate::audio_engine::source_reader::{
        StemRenderSelection, StemTransition, full_stem_available_mask,
        reset_tap_observation_for_test, tap_observation_for_test,
    };
    use crate::audio_engine::stretch_processor::ProductiveSourceFeed;
    use crate::messages::{ResidentContext, STEM_COMPONENT_MASK, StemMixMode};
    use std::sync::atomic::{AtomicU64, Ordering};

    const REGION: FrameRange = FrameRange {
        start: 100,
        end: 1100,
    };
    const RATE: u32 = 48_000;

    struct Fixture {
        full: SampleBuffer,
        sample: SampleBuffer,
        stems: PreparedStemSet,
        plan: SourceReadPlan,
        playback: SourcePlayback,
        ownership: Arc<InputRuntimeOwnership>,
        acknowledgements: Arc<CurrentTimingAcknowledgements>,
        epoch: Arc<AtomicU64>,
    }

    impl Fixture {
        fn new(period: f64) -> Self {
            let full = SampleBuffer {
                channels: 2,
                residency: None,
                samples: (0..4096 * 2)
                    .map(|n| n as f32 / 8192.0)
                    .collect::<Vec<_>>()
                    .into(),
            }
            .with_complete_source(RATE);
            let sample = full
                .window(REGION.start, REGION.end, 2, ResidentContext::FiniteLoop)
                .unwrap();
            let stems = PreparedStemSet {
                complete_set_identity: Arc::new([19; 32]),
                accepted_timing: None,
                reference_samples: full.samples.clone(),
                reference_residency: full.residency.clone(),
                publication: PreparedSourcePermit::unrestricted(),
                source_version_hash: 17,
                sample_rate_hz: RATE,
                channels: 2,
                frame_count: full.frame_count(),
                available_mask: full_stem_available_mask(),
                stems: std::array::from_fn(|index| SampleBuffer {
                    channels: 2,
                    residency: full.residency.clone(),
                    samples: full
                        .samples
                        .iter()
                        .map(|value| value * (index + 1) as f32 / 8.0)
                        .collect::<Vec<_>>()
                        .into(),
                }),
            }
            .window_for(&sample)
            .unwrap();
            let mut playback = SourcePlayback::new(REGION.start, ExplicitSeekMode::Normal, 0.73);
            assert!(playback.configure_domain(
                SourceLoopDomain::musical(full.frame_count(), REGION, period).unwrap(),
            ));
            assert!(playback.advance(17));
            assert!(playback.set_target(1.371234567890123));
            let plan = SourceReadPlan {
                channels: 2,
                sample_frames: full.frame_count(),
                frame_pos: playback.position().frame,
                loop_region: REGION,
                loop_period: Some(period),
                seek_mode: ExplicitSeekMode::Normal,
                selection: StemRenderSelection::from_state(
                    StemMixMode::AllStems,
                    17,
                    STEM_COMPONENT_MASK,
                ),
                transition: StemTransition::default(),
            };
            let ownership = Arc::new(InputRuntimeOwnership::tracked());
            ownership.publish_test_source(0, &sample, RATE, 7);
            Self {
                full,
                sample,
                stems,
                plan,
                playback,
                ownership,
                acknowledgements: Arc::default(),
                epoch: Arc::new(AtomicU64::new(3)),
            }
        }

        fn coverage(&self) -> NormalLoopCoverage {
            NormalLoopCoverage::capture(
                &self.sample,
                Some(&self.stems),
                RATE,
                None,
                self.plan,
                &self.playback,
            )
            .unwrap()
        }

        fn complete_stems(&self) -> PreparedStemSet {
            let mut stems = self.stems.clone();
            stems.reference_samples = self.full.samples.clone();
            stems.reference_residency = self.full.residency.clone();
            stems.stems = std::array::from_fn(|index| SampleBuffer {
                channels: 2,
                residency: self.full.residency.clone(),
                samples: self
                    .full
                    .samples
                    .iter()
                    .map(|value| value * (index + 1) as f32 / 8.0)
                    .collect::<Vec<_>>()
                    .into(),
            });
            stems
        }

        fn in_context(mut self, mode: ExplicitSeekMode) -> Self {
            let required = source_context_range(self.full.frame_count(), REGION, mode).unwrap();
            self.sample = self
                .full
                .window(
                    required.start,
                    required.end,
                    2,
                    ResidentContext::KeyLockFiniteLoop,
                )
                .unwrap();
            self.stems = self.complete_stems().window_for(&self.sample).unwrap();
            let frame = match mode {
                ExplicitSeekMode::Normal => REGION.start,
                ExplicitSeekMode::BeforeLoop => 0,
                ExplicitSeekMode::AfterLoop => self.full.frame_count() - 2,
            };
            self.playback = SourcePlayback::new(frame, mode, 0.73);
            assert!(self.playback.configure_domain(self.plan.domain()));
            assert!(self.playback.set_target(1.371234567890123));
            self.plan.frame_pos = frame;
            self.plan.seek_mode = mode;
            self.ownership.publish_window(0, &self.sample);
            self
        }

        fn request(&self) -> NativeHistoryRequest {
            self.request_with_accepted(None)
        }

        fn request_with_accepted(
            &self,
            accepted: Option<AcceptedTimingProjection>,
        ) -> NativeHistoryRequest {
            NativeHistoryRequest {
                pitch_intent: super::super::pitch_intent::PitchIntent::default(),
                key_lock: true,
                sample: self.sample.clone(),
                stems: Some(self.stems.clone()),
                permit: NativeHistoryContext {
                    id: 0,
                    ownership: &self.ownership,
                    acknowledgements: &self.acknowledgements,
                    preparation_epoch: &self.epoch,
                }
                .capture(&self.sample, RATE, accepted)
                .unwrap(),
                coverage: Some(
                    NormalLoopCoverage::capture(
                        &self.sample,
                        Some(&self.stems),
                        RATE,
                        accepted,
                        self.plan,
                        &self.playback,
                    )
                    .unwrap(),
                ),
                binding: ProductiveSourceBinding::new(&self.sample, RATE, accepted),
                playback: self.playback,
                plan: self.plan,
                actual_feed: None,
                mapped_input: None,
                target_output_frame: 4096,
                request_id: 1,
                epoch: 0,
            }
        }

        fn feed(&self) -> ProductiveSourceFeed<'_> {
            ProductiveSourceFeed {
                pitch_intent: super::super::pitch_intent::PitchIntent::default(),
                key_lock: true,
                sample: &self.sample,
                outgoing_stems: None,
                stems: Some(&self.stems),
                sample_rate_hz: RATE,
                accepted: None,
                plan: self.plan,
                playback: &self.playback,
                permit: None,
                output_frame: Some(0),
            }
        }
    }

    #[test]
    fn finite_coverage_accepts_actual_virtual_seam_taps_and_copied_rate_ramp() {
        for period in [999.75, 1000.0, 1000.25] {
            let fixture = Fixture::new(period);
            let coverage = fixture.coverage();
            assert!(coverage.matches(
                &fixture.sample,
                Some(&fixture.stems),
                RATE,
                None,
                fixture.plan,
                &fixture.playback,
                true
            ));
            let mut playback = fixture.playback;
            let mut plan = fixture.plan;
            let mut feed = [vec![0.0; 512], vec![0.0; 512]];
            for _ in 0..16 {
                let (frames, _) = playback.chunk(512);
                assert!(coverage.matches(
                    &fixture.sample,
                    Some(&fixture.stems),
                    RATE,
                    None,
                    plan,
                    &playback,
                    true
                ));
                reset_tap_observation_for_test();
                plan.fill_fractional_buffers_checked(
                    &fixture.sample,
                    Some(&fixture.stems),
                    &playback,
                    &mut feed,
                    frames,
                )
                .unwrap();
                let observed = tap_observation_for_test();
                assert_eq!(observed.left_reads, frames * 2);
                assert_eq!(observed.missing_reads, 0);
                assert!(observed.min_frame.unwrap() >= REGION.start);
                assert!(observed.max_frame.unwrap() < REGION.end);
                assert!(playback.advance(frames));
                plan.frame_pos = playback.position().frame;
            }
            let seam = SourceReadPlan {
                frame_pos: REGION.end - 1,
                ..fixture.plan
            };
            let taps = seam.fractional_taps(0.5);
            assert_eq!(taps.left, REGION.end - 1);
            assert_eq!(taps.right, Some(REGION.start));
            let expected_gain = if period == 1000.0 {
                0.5
            } else {
                (0.5 / (period - 999.0)) as f32
            };
            assert_eq!(taps.right_gain, expected_gain);
        }
    }

    #[test]
    fn finite_intro_tail_cover_actual_feed_and_ramped_return_to_virtual_loop() {
        for period in [999.75, 1000.0, 1000.25] {
            for mode in [ExplicitSeekMode::BeforeLoop, ExplicitSeekMode::AfterLoop] {
                for mask in [0, 1, 10, 15] {
                    let mut fixture = Fixture::new(period).in_context(mode);
                    let stem = StemRenderSelection::from_state(StemMixMode::AllStems, 17, mask);
                    for (from, to) in [
                        (StemRenderSelection::full_mix(), stem),
                        (stem, StemRenderSelection::full_mix()),
                    ] {
                        fixture.plan.selection = to;
                        fixture.plan.transition = StemTransition::start(from, 128);
                        let coverage = fixture.coverage();
                        let mut playback = fixture.playback;
                        let mut plan = fixture.plan;
                        let mut feed = [vec![0.0; 512], vec![0.0; 512]];
                        let mut returned = false;
                        let mut settled_fenced = false;
                        let mut right_reads = 0;
                        for _ in 0..24 {
                            let (frames, ratio) = playback.chunk(512);
                            assert!(normal_loop_feed_available(
                                &fixture.sample,
                                Some(&fixture.stems),
                                RATE,
                                None,
                                plan,
                                &playback,
                            ));
                            // The productive feed covers both ramp sides and its settled target.
                            // Pending coverage keeps its original transition contract; clearing
                            // the ramp requires a fresh request, never a relaxed adoption fence.
                            assert_eq!(
                                coverage.matches(
                                    &fixture.sample,
                                    Some(&fixture.stems),
                                    RATE,
                                    None,
                                    plan,
                                    &playback,
                                    true,
                                ),
                                plan.transition.is_active()
                            );
                            settled_fenced |= !plan.transition.is_active();
                            reset_tap_observation_for_test();
                            plan.fill_fractional_buffers_checked(
                                &fixture.sample,
                                Some(&fixture.stems),
                                &playback,
                                &mut feed,
                                frames,
                            )
                            .unwrap();
                            let observed = tap_observation_for_test();
                            assert_eq!(observed.missing_reads, 0);
                            assert_eq!(observed.left_reads, frames * 2);
                            assert!(observed.min_frame.unwrap() >= fixture.sample.resident_start());
                            assert!(observed.max_frame.unwrap() < fixture.sample.resident_end());
                            right_reads += observed.right_reads;
                            assert!(playback.advance(frames));
                            plan.transition.advance_output_frames(frames, ratio);
                            let position = playback.position();
                            plan.frame_pos = position.frame;
                            plan.seek_mode = position.seek_mode;
                            returned |= position.seek_mode == ExplicitSeekMode::Normal;
                        }
                        assert!(returned);
                        assert!(settled_fenced);
                        assert!(right_reads > 0);
                        assert_eq!(playback.tempo_ratio(), playback.rate_target());
                    }
                }
            }
        }
    }

    #[test]
    fn finite_intro_tail_require_prefix_end_and_both_boundary_taps() {
        for (mode, start, end, frame, missing, right_reads) in [
            (ExplicitSeekMode::BeforeLoop, 1, REGION.end, 0, 0, 0),
            (
                ExplicitSeekMode::AfterLoop,
                REGION.start,
                4095,
                4094,
                4095,
                1,
            ),
        ] {
            let fixture = Fixture::new(1000.25).in_context(mode);
            let incomplete = fixture
                .full
                .window(start, end, 9, ResidentContext::KeyLockFiniteLoop)
                .unwrap();
            assert!(!normal_loop_context_available(&incomplete, REGION, mode));
            let mut playback = SourcePlayback::new(frame, mode, 0.5);
            assert!(playback.configure_domain(fixture.plan.domain()));
            assert!(playback.advance(1));
            let position = playback.position();
            let plan = SourceReadPlan {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                selection: StemRenderSelection::full_mix(),
                ..fixture.plan
            };
            assert!(
                NormalLoopCoverage::capture(&incomplete, None, RATE, None, plan, &playback)
                    .is_none()
            );
            let mut feed = [vec![7.0], vec![7.0]];
            reset_tap_observation_for_test();
            assert!(
                plan.fill_fractional_buffers_checked(&incomplete, None, &playback, &mut feed, 1)
                    .is_err()
            );
            assert_eq!(feed, [vec![7.0], vec![7.0]]);
            let observed = tap_observation_for_test();
            assert_eq!(observed.missing_reads, 1);
            assert_eq!(observed.min_frame, Some(missing));
            assert_eq!(observed.right_reads, right_reads);
        }
    }

    #[test]
    fn finite_intro_tail_prepare_real_native_and_continue_like_independent_complete_pcm() {
        for period in [999.75, 1000.0, 1000.25] {
            for mode in [ExplicitSeekMode::BeforeLoop, ExplicitSeekMode::AfterLoop] {
                for mask in [1, 10] {
                    let mut finite = Fixture::new(period).in_context(mode);
                    finite.plan.selection =
                        StemRenderSelection::from_state(StemMixMode::AllStems, 17, mask);
                    let mut complete = Fixture::new(period).in_context(mode);
                    complete.sample = complete.full.clone();
                    complete.stems = complete.complete_stems();
                    complete.plan.selection = finite.plan.selection;
                    complete.ownership.publish_window(0, &complete.sample);
                    assert!(!Arc::ptr_eq(&finite.full.samples, &complete.full.samples));
                    let mut expected_request = complete.request();
                    expected_request.coverage = None;
                    let mut actual = NativeAdapterState::new(
                        Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
                        2,
                    );
                    let mut expected = NativeAdapterState::new(
                        Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
                        2,
                    );
                    actual.prepare_source(finite.request()).unwrap();
                    expected.prepare_source(expected_request).unwrap();
                    assert!(actual.used);
                    assert!(actual.active);
                    let source = actual.source.as_ref().unwrap();
                    let observed = source.coverage.as_ref().unwrap().prepared_taps;
                    assert_eq!(observed.left_reads, 4096 * 2);
                    assert!(observed.right_reads > 0);
                    assert_eq!(observed.missing_reads, 0);
                    assert!(observed.min_frame.unwrap() >= finite.sample.resident_start());
                    assert!(observed.max_frame.unwrap() < finite.sample.resident_end());
                    let mut plan = source.plan;
                    let mut playback = source.playback;
                    assert_eq!(plan.seek_mode, ExplicitSeekMode::Normal);
                    assert!(playback.matches_exact(&expected.source.as_ref().unwrap().playback));
                    let mut actual_feed = [vec![0.0; 512], vec![0.0; 512]];
                    let mut expected_feed = actual_feed.clone();
                    let mut actual_output = actual_feed.clone();
                    let mut expected_output = actual_feed.clone();
                    let mut nontrivial = false;
                    for max_frames in [1, 127, 384, 96, 257, 512, 31].into_iter().cycle().take(64) {
                        let (frames, ratio) = playback.chunk(max_frames);
                        plan.fill_fractional_buffers_checked(
                            &finite.sample,
                            Some(&finite.stems),
                            &playback,
                            &mut actual_feed,
                            frames,
                        )
                        .unwrap();
                        plan.fill_fractional_buffers(
                            &complete.sample,
                            Some(&complete.stems),
                            &playback,
                            &mut expected_feed,
                            frames,
                        )
                        .expect("known scalar test source read");
                        assert_eq!(actual_feed, expected_feed);
                        let pitch = pitch_scale_for_tempo_ratio(ratio);
                        actual
                            .render(&actual_feed, &mut actual_output, frames, pitch)
                            .unwrap();
                        expected
                            .render(&expected_feed, &mut expected_output, frames, pitch)
                            .unwrap();
                        for channel in 0..2 {
                            for frame in 0..frames {
                                assert!(
                                    (actual_output[channel][frame]
                                        - expected_output[channel][frame])
                                        .abs()
                                        < 1.0e-7
                                );
                                nontrivial |= actual_output[channel][frame].abs() > 1.0e-4;
                            }
                            assert_eq!(
                                actual.input_fifo[channel].len(),
                                expected.input_fifo[channel].len()
                            );
                            assert_eq!(
                                actual.output_fifo[channel].len(),
                                expected.output_fifo[channel].len()
                            );
                        }
                        assert!(playback.advance(frames));
                        let position = playback.position();
                        plan.frame_pos = position.frame;
                        plan.seek_mode = position.seek_mode;
                    }
                    assert!(nontrivial);
                }
            }
        }
    }

    #[test]
    fn complete_intro_tail_history_retains_original_pins_when_loop_only_storage_returns() {
        for mode in [ExplicitSeekMode::BeforeLoop, ExplicitSeekMode::AfterLoop] {
            let mut fixture = Fixture::new(1000.25).in_context(mode);
            fixture.sample = fixture.full.clone();
            fixture.stems = fixture.complete_stems();
            fixture.ownership.publish_window(0, &fixture.sample);
            let mut request = fixture.request();
            request.coverage = None;
            let finite = fixture
                .full
                .window(
                    REGION.start,
                    REGION.end,
                    8,
                    ResidentContext::KeyLockFiniteLoop,
                )
                .unwrap();
            let stems = fixture.stems.clone().window_for(&finite).unwrap();
            let mut playback = fixture.playback;
            for _ in 0..8 {
                let (frames, _) = playback.chunk(512);
                assert!(playback.advance(frames));
            }
            let position = playback.position();
            assert_eq!(position.seek_mode, ExplicitSeekMode::Normal);
            let plan = SourceReadPlan {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                ..fixture.plan
            };
            fixture.ownership.publish_window(0, &finite);
            let feed = ProductiveSourceFeed {
                sample: &finite,
                outgoing_stems: None,
                stems: Some(&stems),
                plan,
                playback: &playback,
                ..fixture.feed()
            };
            assert!(!request.matches_contract(&feed, 0));
            assert!(request.matches_effective_contract(&feed, 0));
            assert_eq!(request.sample.resident_start(), 0);
            assert_eq!(request.sample.resident_end(), fixture.full.frame_count());
            fixture.epoch.fetch_add(1, Ordering::AcqRel);
            assert!(!request.matches_effective_contract(&feed, 0));
        }
    }

    #[test]
    fn finite_tag_does_not_authorize_missing_left_or_right_tap() {
        let fixture = Fixture::new(1000.0);
        for (start, end, frame, expected_missing, right_reads) in [
            (REGION.start + 1, REGION.end, REGION.start, REGION.start, 0),
            (
                REGION.start,
                REGION.end - 1,
                REGION.end - 2,
                REGION.end - 1,
                1,
            ),
        ] {
            let incomplete = fixture
                .full
                .window(start, end, 9, ResidentContext::KeyLockFiniteLoop)
                .unwrap();
            assert!(!normal_loop_context_available(
                &incomplete,
                REGION,
                ExplicitSeekMode::Normal
            ));
            let mut playback = SourcePlayback::new(frame, ExplicitSeekMode::Normal, 0.5);
            assert!(playback.configure_domain(SourceLoopDomain::physical(
                fixture.full.frame_count(),
                REGION,
            )));
            assert!(playback.advance(1));
            let plan = SourceReadPlan {
                frame_pos: frame,
                loop_period: None,
                selection: StemRenderSelection::full_mix(),
                ..fixture.plan
            };
            assert!(
                NormalLoopCoverage::capture(&incomplete, None, RATE, None, plan, &playback)
                    .is_none()
            );
            let mut feed = [vec![7.0; 1], vec![7.0; 1]];
            reset_tap_observation_for_test();
            assert!(
                plan.fill_fractional_buffers_checked(&incomplete, None, &playback, &mut feed, 1)
                    .is_err()
            );
            assert_eq!(feed, [vec![7.0], vec![7.0]]);
            let observed = tap_observation_for_test();
            assert_eq!(observed.missing_reads, 1);
            assert_eq!(observed.min_frame, Some(expected_missing));
            assert_eq!(observed.right_reads, right_reads);
        }
    }

    #[test]
    fn finite_coverage_fences_source_rate_period_selection_and_unsupported_context() {
        let fixture = Fixture::new(1000.25);
        let coverage = fixture.coverage();
        let capture = |plan, playback: &SourcePlayback, rate, stems| {
            NormalLoopCoverage::capture(&fixture.sample, stems, rate, None, plan, playback)
        };
        assert!(
            capture(
                fixture.plan,
                &fixture.playback,
                RATE + 1,
                Some(&fixture.stems)
            )
            .is_none()
        );
        assert!(capture(fixture.plan, &fixture.playback, RATE, None).is_none());
        for plan in [
            SourceReadPlan {
                loop_period: Some(1001.01),
                ..fixture.plan
            },
            SourceReadPlan {
                seek_mode: ExplicitSeekMode::BeforeLoop,
                ..fixture.plan
            },
            SourceReadPlan {
                seek_mode: ExplicitSeekMode::AfterLoop,
                ..fixture.plan
            },
            SourceReadPlan {
                selection: StemRenderSelection::from_state(
                    StemMixMode::AllStems,
                    18,
                    STEM_COMPONENT_MASK,
                ),
                ..fixture.plan
            },
        ] {
            assert!(capture(plan, &fixture.playback, RATE, Some(&fixture.stems)).is_none());
        }
        let replacement = fixture.sample.clone().with_complete_source(RATE);
        assert!(!coverage.matches(
            &replacement,
            Some(&fixture.stems),
            RATE,
            None,
            fixture.plan,
            &fixture.playback,
            false
        ));
        let mut changed_rate = fixture.playback;
        assert!(changed_rate.set_target(1.5));
        assert!(!coverage.matches(
            &fixture.sample,
            Some(&fixture.stems),
            RATE,
            None,
            fixture.plan,
            &changed_rate,
            true
        ));
        let mut changed_set = fixture.stems.clone();
        changed_set.complete_set_identity = Arc::new(*fixture.stems.complete_set_identity);
        assert!(!coverage.matches(
            &fixture.sample,
            Some(&changed_set),
            RATE,
            None,
            fixture.plan,
            &fixture.playback,
            false
        ));
    }

    #[test]
    fn finite_coverage_requires_real_outgoing_and_incoming_selections_even_with_empty_masks() {
        let fixture = Fixture::new(1000.25);
        for mask in [0, 1, 10, 15] {
            let stem = StemRenderSelection::from_state(StemMixMode::AllStems, 17, mask);
            for (from, to) in [
                (stem, StemRenderSelection::full_mix()),
                (StemRenderSelection::full_mix(), stem),
            ] {
                let plan = SourceReadPlan {
                    selection: to,
                    transition: StemTransition::start(from, 128),
                    ..fixture.plan
                };
                let capture = |stems: Option<&PreparedStemSet>| {
                    NormalLoopCoverage::capture(
                        &fixture.sample,
                        stems,
                        RATE,
                        None,
                        plan,
                        &fixture.playback,
                    )
                };
                let coverage = capture(Some(&fixture.stems)).unwrap();
                assert!(capture(None).is_none());
                let mut stale = fixture.stems.clone();
                stale.source_version_hash += 1;
                assert!(capture(Some(&stale)).is_none());
                let changed = SourceReadPlan {
                    transition: StemTransition::start(
                        StemRenderSelection::from_state(StemMixMode::AllStems, 17, 2),
                        128,
                    ),
                    ..plan
                };
                assert!(!coverage.matches(
                    &fixture.sample,
                    Some(&fixture.stems),
                    RATE,
                    None,
                    changed,
                    &fixture.playback,
                    true
                ));
                for index in 0..STEM_BUFFER_COUNT {
                    let mut missing = fixture.stems.clone();
                    missing.stems[index].samples = Arc::from([0.0; 2]);
                    assert!(capture(Some(&missing)).is_none());
                }
            }
        }
    }

    #[test]
    fn finite_coverage_retains_logical_roles_from_real_outgoing_and_shared_reader_slots() {
        fn capture(
            fixture: &Fixture,
            readers: StemReadSets<'_>,
            plan: SourceReadPlan,
        ) -> Option<NormalLoopCoverage> {
            NormalLoopCoverage::capture_with_readers(
                &fixture.sample,
                readers,
                RATE,
                None,
                plan,
                &fixture.playback,
            )
        }

        let fixture = Fixture::new(1000.25);
        let empty = StemRenderSelection::for_stems(&fixture.stems, 0);
        let selected = StemRenderSelection::for_stems(&fixture.stems, 10);
        for (from, to, readers) in [
            (
                empty,
                StemRenderSelection::full_mix(),
                StemReadSets::new(None, Some(&fixture.stems)),
            ),
            (
                StemRenderSelection::full_mix(),
                empty,
                StemReadSets::new(Some(&fixture.stems), None),
            ),
            (
                empty,
                selected,
                StemReadSets::new(Some(&fixture.stems), Some(&fixture.stems)),
            ),
        ] {
            let plan = SourceReadPlan {
                selection: to,
                transition: StemTransition::start(from, 128),
                ..fixture.plan
            };
            let coverage = capture(&fixture, readers, plan).unwrap();
            assert!(coverage.matches_with_readers(
                &fixture.sample,
                readers,
                RATE,
                None,
                plan,
                &fixture.playback,
                true,
            ));
            assert!(capture(&fixture, StemReadSets::default(), plan).is_none());
            let mut foreign = fixture.stems.clone();
            foreign.complete_set_identity = Arc::new(*fixture.stems.complete_set_identity);
            let mut republished = fixture.stems.clone();
            republished.publication.expected += 1;
            for replacement in [&foreign, &republished] {
                let changed = StemReadSets::new(
                    readers.current.map(|_| replacement),
                    readers.outgoing.map(|_| replacement),
                );
                for exact_window in [false, true] {
                    assert!(!coverage.matches_with_readers(
                        &fixture.sample,
                        changed,
                        RATE,
                        None,
                        plan,
                        &fixture.playback,
                        exact_window,
                    ));
                }
            }
            let foreign_readers = StemReadSets::new(
                readers.current.map(|_| &foreign),
                readers.outgoing.map(|_| &foreign),
            );
            assert!(capture(&fixture, foreign_readers, plan).is_none());
            for component in 0..STEM_BUFFER_COUNT {
                let mut truncated = fixture.stems.clone();
                truncated.stems[component].samples = Arc::from([0.0; 2]);
                let changed = StemReadSets::new(
                    readers.current.map(|_| &truncated),
                    readers.outgoing.map(|_| &truncated),
                );
                assert!(capture(&fixture, changed, plan).is_none());
                assert!(!coverage.matches_with_readers(
                    &fixture.sample,
                    changed,
                    RATE,
                    None,
                    plan,
                    &fixture.playback,
                    false,
                ));
            }
        }
    }

    #[test]
    fn effective_active_coverage_requires_exact_progress_and_only_releases_completed_outgoing_role()
    {
        let fixture = Fixture::new(1000.25);
        let empty = StemRenderSelection::for_stems(&fixture.stems, 0);
        for (from, to) in [
            (empty, StemRenderSelection::full_mix()),
            (StemRenderSelection::full_mix(), empty),
        ] {
            let mut plan = SourceReadPlan {
                selection: to,
                transition: StemTransition::start(from, 128),
                ..fixture.plan
            };
            plan.transition.advance_fractional(27.25);
            let readers = StemReadSets::new(Some(&fixture.stems), Some(&fixture.stems));
            let coverage = NormalLoopCoverage::capture_with_readers(
                &fixture.sample,
                readers,
                RATE,
                None,
                plan,
                &fixture.playback,
            )
            .unwrap();
            let matches = |plan: SourceReadPlan, playback: &SourcePlayback, readers, expected| {
                coverage.matches_effective_with_readers(
                    &fixture.sample,
                    readers,
                    RATE,
                    None,
                    plan,
                    playback,
                    expected,
                )
            };
            assert!(!matches(plan, &fixture.playback, readers, None));
            assert!(matches(
                plan,
                &fixture.playback,
                readers,
                Some(plan.transition)
            ));
            let initial_transition = plan.transition;
            let mut wrong_phase = initial_transition;
            wrong_phase.advance_fractional(0.25);
            assert!(!matches(
                plan,
                &fixture.playback,
                readers,
                Some(wrong_phase)
            ));
            let premature_clear = SourceReadPlan {
                transition: StemTransition::default(),
                ..plan
            };
            assert!(!matches(
                premature_clear,
                &fixture.playback,
                StemReadSets::default(),
                Some(initial_transition),
            ));
            assert!(!matches(
                plan,
                &fixture.playback,
                StemReadSets::default(),
                Some(initial_transition),
            ));

            let mut playback = fixture.playback;
            for frames in [1, 73, 511] {
                let (frames, ratio) = playback.chunk(frames);
                assert!(playback.advance(frames));
                plan.transition.advance_output_frames(frames, ratio);
                let position = playback.position();
                plan.frame_pos = position.frame;
                plan.seek_mode = position.seek_mode;
                assert!(!matches(plan, &playback, readers, None));
                assert!(matches(plan, &playback, readers, Some(plan.transition)));
                assert_eq!(
                    coverage.matches_with_readers(
                        &fixture.sample,
                        readers,
                        RATE,
                        None,
                        plan,
                        &playback,
                        true,
                    ),
                    plan.transition.is_active(),
                );
            }
            assert!(!plan.transition.is_active());
            assert!(!matches(plan, &playback, readers, Some(initial_transition)));
            let settled_readers =
                StemReadSets::new(to.required_stem_hash().map(|_| &fixture.stems), None);
            assert!(matches(
                plan,
                &playback,
                settled_readers,
                Some(plan.transition),
            ));
            assert_eq!(
                matches(
                    plan,
                    &playback,
                    StemReadSets::default(),
                    Some(plan.transition),
                ),
                to.required_stem_hash().is_none(),
                "an empty incoming mask still requires its original held set",
            );
            let different_selection = SourceReadPlan {
                selection: StemRenderSelection::for_stems(&fixture.stems, 2),
                ..plan
            };
            assert!(!matches(
                different_selection,
                &playback,
                readers,
                Some(plan.transition),
            ));
        }
    }

    #[test]
    fn finite_native_request_requires_its_own_window_and_history_permit() {
        let fixture = Fixture::new(1000.25);
        let request = fixture.request();
        assert!(request.matches_contract(&fixture.feed(), 0));
        let mut missing = fixture.request();
        missing.coverage = None;
        assert!(!missing.matches_contract(&fixture.feed(), 0));
        assert!(!missing.matches_effective_contract(&fixture.feed(), 0));
        // The same PCM and complete descriptor get a new actual storage ACK. Pending work
        // stays fenced, while adopted native/history may retain its original finite pins.
        let mut refreshed = fixture.sample.clone();
        let mut view = (**refreshed.residency.as_ref().unwrap()).clone();
        view.window_revision += 1;
        refreshed.residency = Some(Arc::new(view));
        let refreshed_stems = fixture.stems.clone().window_for(&refreshed).unwrap();
        fixture.ownership.publish_window(0, &refreshed);
        let next = ProductiveSourceFeed {
            sample: &refreshed,
            outgoing_stems: None,
            stems: Some(&refreshed_stems),
            ..fixture.feed()
        };
        assert!(!request.matches_contract(&next, 0));
        assert!(request.matches_effective_contract(&next, 0));
        fixture.epoch.fetch_add(1, Ordering::AcqRel);
        assert!(!request.matches_effective_contract(&next, 0));
    }

    fn tracked_stem_window_fixture() -> (Fixture, AcceptedTimingProjection, Arc<AtomicU64>) {
        let mut fixture = Fixture::new(1000.25);
        fixture
            .ownership
            .publish_test_source(0, &fixture.sample, RATE, 7);
        let placement = fixture.ownership.slot_snapshot(0).unwrap();
        let accepted = AcceptedTimingProjection {
            revision: [67; 32],
            period_seconds: 1000.25 / f64::from(RATE),
            origin_seconds: REGION.start as f64 / f64::from(RATE),
            sample_rate_hz: RATE,
            publication_epoch: 11,
        };
        assert!(fixture.acknowledgements.acknowledge_for_key(
            &fixture.ownership,
            placement.content_key().unwrap(),
            accepted.publication_epoch,
        ));
        let source_epoch = Arc::new(AtomicU64::new(5));
        fixture.stems.accepted_timing = Some(accepted);
        fixture.stems.publication = PreparedSourcePermit::for_native_binding_for_test(
            0,
            InputPadBinding {
                placement: Some(placement),
                resident: fixture.sample.resident_binding(),
                source_address: fixture.sample.source_address(),
                sample_count: fixture.sample.source_sample_count(),
                channels: fixture.sample.channels,
                sample_rate_hz: RATE,
                authority_revision: fixture.ownership.authority_revision(0),
                runtime_revision: fixture.ownership.runtime_revision(0),
                accepted: Some(accepted),
            },
            fixture.ownership.clone(),
            fixture.acknowledgements.clone(),
            Arc::new(AtomicU64::new(3)),
        )
        .unwrap()
        .with_source_epoch(source_epoch.clone(), 5);
        fixture.stems.publication.mark_accepted();
        assert!(fixture.stems.publication.current());
        (fixture, accepted, source_epoch)
    }

    #[test]
    fn tracked_stem_window_ack_keeps_data_stamp_but_requires_own_native_window_and_revocation() {
        use crate::audio_engine::prepared_native_history::PitchSourceSnapshot;
        use crate::audio_engine::source_feed_history::SourceFeedContext;

        for revoke in [
            "none",
            "operation_epoch",
            "source_publication_epoch",
            "receipt_status",
            "publication_owner",
            "publication_expected",
            "source",
            "timing",
            "authority",
            "native_epoch",
        ] {
            let (fixture, accepted, source_epoch) = tracked_stem_window_fixture();
            let original = fixture.request_with_accepted(Some(accepted));
            let original_feed = ProductiveSourceFeed {
                accepted: Some(accepted),
                output_frame: Some(4096),
                ..fixture.feed()
            };
            assert!(original.matches_adoption(&original_feed, 0, 1));
            let mut refreshed = fixture.sample.clone();
            let mut view = (**refreshed.residency.as_ref().unwrap()).clone();
            view.window_revision += 1;
            refreshed.residency = Some(Arc::new(view));
            let refreshed_stems = fixture.stems.clone().window_for(&refreshed).unwrap();
            fixture.ownership.publish_window(0, &refreshed);
            assert!(!fixture.stems.publication.current());
            assert!(Arc::ptr_eq(
                &fixture.stems.publication.epoch,
                &refreshed_stems.publication.epoch
            ));
            let next = ProductiveSourceFeed {
                sample: &refreshed,
                stems: Some(&refreshed_stems),
                ..original_feed
            };
            assert!(!original.matches_contract(&next, 0));
            assert!(!original.matches_adoption(&next, 0, 1));
            assert!(original.matches_effective_contract(&next, 0));
            let context = NativeHistoryContext {
                id: 0,
                ownership: &fixture.ownership,
                acknowledgements: &fixture.acknowledgements,
                preparation_epoch: &fixture.epoch,
            };
            assert!(
                context
                    .capture(&fixture.sample, RATE, Some(accepted))
                    .is_none()
            );
            let fresh_permit = context.capture(&refreshed, RATE, Some(accepted)).unwrap();
            let snapshot = PitchSourceSnapshot {
                sample: refreshed.clone(),
                stems: Some(refreshed_stems.clone()),
                accepted: Some(accepted),
                sample_rate_hz: RATE,
                paused: false,
                playback: fixture.playback,
                plan: fixture.plan,
                actual_feed: None,
                source_feed_context: SourceFeedContext::new(
                    ProductiveSourceBinding::new(&refreshed, RATE, Some(accepted)),
                    fixture
                        .plan
                        .required_read_sets(StemReadSets::new(Some(&refreshed_stems), None)),
                ),
            };
            assert!(snapshot.readers().is_some());
            let fresh = snapshot
                .into_request(fresh_permit, original.pitch_intent, true, 4096, 2, 0)
                .unwrap();
            assert!(
                fresh.matches_adoption(&next, 0, 2),
                "fresh own-window Native authority must admit retained stem data"
            );
            let operation_only = match revoke {
                "none" => continue,
                "operation_epoch" => {
                    fixture
                        .stems
                        .publication
                        .epoch
                        .fetch_add(1, Ordering::AcqRel);
                    true
                }
                "source_publication_epoch" => {
                    source_epoch.fetch_add(1, Ordering::AcqRel);
                    true
                }
                // A scalar operation-outcome perturbation supplies no PCM lifetime
                // authority. This unit does not claim an accepted public receipt
                // normally transitions to rejected while its readers stay installed.
                "receipt_status" => {
                    fixture.stems.publication.mark_rejected();
                    true
                }
                "publication_owner" | "publication_expected" => {
                    let mut replaced = refreshed_stems.clone();
                    if revoke == "publication_owner" {
                        replaced.publication.epoch =
                            Arc::new(AtomicU64::new(replaced.publication.expected));
                    } else {
                        replaced.publication.expected += 1;
                    }
                    let replaced_feed = ProductiveSourceFeed {
                        stems: Some(&replaced),
                        ..next
                    };
                    assert!(
                        !original.matches_effective_contract(&replaced_feed, 0),
                        "{revoke}"
                    );
                    assert!(
                        !fresh.matches_effective_contract(&replaced_feed, 0),
                        "{revoke}"
                    );
                    assert!(!fresh.matches_adoption(&replaced_feed, 0, 2), "{revoke}");
                    continue;
                }
                "source" => {
                    fixture.ownership.revoke_source(0);
                    false
                }
                "timing" => {
                    assert!(
                        fixture.acknowledgements.revoke_for_key(
                            &fixture.ownership,
                            fixture
                                .ownership
                                .slot_snapshot(0)
                                .unwrap()
                                .content_key()
                                .unwrap(),
                            accepted.publication_epoch,
                        )
                    );
                    false
                }
                "authority" => {
                    fixture
                        .ownership
                        .revoke(0, fixture.ownership.authority_revision(0) + 1);
                    false
                }
                "native_epoch" => {
                    fixture.epoch.fetch_add(1, Ordering::AcqRel);
                    false
                }
                _ => unreachable!(),
            };
            if operation_only {
                assert!(original.permit.current_effective_source(&refreshed, RATE));
                assert!(fresh.permit.current(&refreshed, RATE));
                assert!(original.matches_effective_contract(&next, 0), "{revoke}");
                assert!(fresh.matches_effective_contract(&next, 0), "{revoke}");
                assert!(fresh.matches_adoption(&next, 0, 2), "{revoke}");
                continue;
            }
            assert!(!original.matches_effective_contract(&next, 0), "{revoke}");
            assert!(!fresh.matches_effective_contract(&next, 0), "{revoke}");
            assert!(!fresh.matches_adoption(&next, 0, 2), "{revoke}");
        }
    }
}
