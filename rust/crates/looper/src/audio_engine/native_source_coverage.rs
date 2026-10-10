//! Bounded authority for the actual NormalLoop source taps supplied to native history.
//!
//! Every phase, including both virtual seam taps, addresses a physical loop knot. This proof
//! covers that complete finite physical range; the native preparation horizon is not a halo.
//! Source/window/history permissions remain with their existing independent owners.

use super::constant_timing::AcceptedTimingProjection;
use super::productive_source_history::ProductiveSourceBinding;
use super::source_playback::SourcePlayback;
use super::source_reader::{
    ExplicitSeekMode, FrameRange, SourceLoopDomain, SourceReadPlan,
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

/// Prove all possible NormalLoop tap addresses from the real held physical range.
/// A resident context tag without a valid layout and both physical bounds is insufficient.
pub(crate) fn normal_loop_context_available(
    sample: &SampleBuffer,
    region: FrameRange,
    mode: ExplicitSeekMode,
) -> bool {
    sample_layout_available(sample)
        && mode == ExplicitSeekMode::Normal
        && region.start < region.end
        && region.end <= sample.frame_count()
        && sample.resident_start() <= region.start
        && sample.resident_end() >= region.end
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
    if !normal_loop_context_available(sample, plan.loop_region, plan.seek_mode)
        || plan.channels != sample.channels
        || plan.sample_frames != sample.frame_count()
        || !sample.valid_residency(sample_rate_hz, plan.channels)
        || plan.transition.is_active()
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
    if position.seek_mode != ExplicitSeekMode::Normal
        || position.frame != plan.frame_pos
        || !position.fraction.is_finite()
        || !(0.0..1.0).contains(&position.fraction)
    {
        return false;
    }
    if let Some(hash) = plan.selection.required_stem_hash() {
        let Some(stems) = stems else {
            return false;
        };
        if stems.source_version_hash != hash
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
    // Use the same address calculation as actual reads, including virtual P > H positions
    // at the physical exclusive end. All other Normal phases map inside the same knots.
    let taps = plan.fractional_taps(position.fraction);
    taps.left >= plan.loop_region.start
        && taps.left < plan.loop_region.end
        && taps
            .right
            .is_none_or(|right| right >= plan.loop_region.start && right < plan.loop_region.end)
}

#[derive(Clone)]
struct StemCoverage {
    complete_set_identity: Arc<[u8; 32]>,
    reference_address: usize,
    bindings: [Option<ResidentBinding>; STEM_BUFFER_COUNT],
    addresses: [usize; STEM_BUFFER_COUNT],
}

impl StemCoverage {
    fn capture(stems: &PreparedStemSet) -> Self {
        Self {
            complete_set_identity: stems.complete_set_identity.clone(),
            reference_address: stems.reference_samples.as_ptr() as usize,
            bindings: std::array::from_fn(|index| stems.stems[index].resident_binding()),
            addresses: std::array::from_fn(|index| stems.stems[index].samples.as_ptr() as usize),
        }
    }

    fn matches(&self, stems: &PreparedStemSet, exact_window: bool) -> bool {
        Arc::ptr_eq(&self.complete_set_identity, &stems.complete_set_identity)
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
    stems: Option<StemCoverage>,
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
        if !normal_loop_feed_available(sample, stems, sample_rate_hz, accepted, plan, playback) {
            return None;
        }
        Some(Self {
            source: sample.residency.as_ref()?.source.clone(),
            binding: ProductiveSourceBinding::new(sample, sample_rate_hz, accepted),
            resident: sample.resident_binding()?,
            plan,
            playback: *playback,
            stems: stems.map(StemCoverage::capture),
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
        let binding = ProductiveSourceBinding::new(sample, sample_rate_hz, accepted);
        sample
            .residency
            .as_ref()
            .is_some_and(|view| Arc::ptr_eq(&self.source, &view.source))
            && self.binding.same_source(binding)
            && self.binding.accepted == accepted
            && (!exact_window || sample.resident_binding() == Some(self.resident))
            && self.plan.matches_source_contract(plan)
            && self.playback.matches_rate_target(playback)
            && normal_loop_feed_available(sample, stems, sample_rate_hz, accepted, plan, playback)
            && match (&self.stems, stems) {
                (None, None) => true,
                (Some(owner), Some(stems)) => owner.matches(stems, exact_window),
                _ => false,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
    use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
    use crate::audio_engine::native_history_permit::NativeHistoryContext;
    use crate::audio_engine::prepared_native_history::NativeHistoryRequest;
    use crate::audio_engine::prepared_source::PreparedSourcePermit;
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
            playback.configure_domain(
                SourceLoopDomain::musical(full.frame_count(), REGION, period).unwrap(),
            );
            playback.advance(17);
            playback.set_target(1.371234567890123);
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
            ownership.publish_source(0, &sample, RATE, 7);
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

        fn request(&self) -> NativeHistoryRequest {
            NativeHistoryRequest {
                sample: self.sample.clone(),
                stems: Some(self.stems.clone()),
                permit: NativeHistoryContext {
                    id: 0,
                    ownership: &self.ownership,
                    acknowledgements: &self.acknowledgements,
                    preparation_epoch: &self.epoch,
                }
                .capture(&self.sample, RATE, None)
                .unwrap(),
                coverage: Some(self.coverage()),
                binding: ProductiveSourceBinding::new(&self.sample, RATE, None),
                playback: self.playback,
                plan: self.plan,
                target_output_frame: 4096,
                request_id: 1,
                epoch: 0,
            }
        }

        fn feed(&self) -> ProductiveSourceFeed<'_> {
            ProductiveSourceFeed {
                sample: &self.sample,
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
                playback.advance(frames);
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
            playback.configure_domain(SourceLoopDomain::physical(
                fixture.full.frame_count(),
                REGION,
            ));
            playback.advance(1);
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
            SourceReadPlan {
                transition: StemTransition::start(StemRenderSelection::full_mix(), 128),
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
        changed_rate.set_target(1.5);
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
            stems: Some(&refreshed_stems),
            ..fixture.feed()
        };
        assert!(!request.matches_contract(&next, 0));
        assert!(request.matches_effective_contract(&next, 0));
        fixture.epoch.fetch_add(1, Ordering::AcqRel);
        assert!(!request.matches_effective_contract(&next, 0));
    }
}
