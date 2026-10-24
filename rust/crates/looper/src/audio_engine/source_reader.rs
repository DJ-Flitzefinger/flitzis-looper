//! Shared source-domain loop, seek and prepared-stem reading policy.
//!
//! These helpers borrow already loaded immutable buffers and use only scalar state. The mixer
//! and non-realtime preparation code can therefore read the same source without sharing a voice,
//! advancing transport, or changing persisted loop markers.

use super::source_playback::{FractionalSourcePosition, SourcePlayback};
pub(crate) use super::source_reader_period::SourceReaderPeriod;
use crate::messages::{
    PreparedStemSet, STEM_BUFFER_COUNT, STEM_COMPONENT_MASK, SampleBuffer, StemMixMode,
};

pub(crate) const STEM_TRANSITION_RAMP_FRAMES: usize = 128;

/// Actual physical reads and interpolation weight for one fractional source position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FractionalSourceTaps {
    pub(crate) left: usize,
    pub(crate) right: Option<usize>,
    pub(crate) right_gain: f32,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TapReadObservation {
    pub(crate) left_reads: usize,
    pub(crate) right_reads: usize,
    pub(crate) min_frame: Option<usize>,
    pub(crate) max_frame: Option<usize>,
    pub(crate) missing_reads: usize,
    pub(crate) address_checksum: u64,
}

#[cfg(test)]
thread_local! {
    static TAP_READ_OBSERVATION: std::cell::Cell<TapReadObservation> =
        std::cell::Cell::new(TapReadObservation {
            left_reads: 0, right_reads: 0, min_frame: None, max_frame: None,
            missing_reads: 0, address_checksum: 0,
        });
}

#[cfg(test)]
pub(crate) fn reset_tap_observation_for_test() {
    TAP_READ_OBSERVATION.set(TapReadObservation::default());
}

#[cfg(test)]
pub(crate) fn tap_observation_for_test() -> TapReadObservation {
    TAP_READ_OBSERVATION.get()
}

#[cfg(test)]
fn observe_tap(frame: usize, channel: usize, right: bool, available: bool) {
    TAP_READ_OBSERVATION.with(|cell| {
        let mut observation = cell.get();
        if right {
            observation.right_reads += 1;
        } else {
            observation.left_reads += 1;
        }
        observation.min_frame = Some(observation.min_frame.map_or(frame, |old| old.min(frame)));
        observation.max_frame = Some(observation.max_frame.map_or(frame, |old| old.max(frame)));
        observation.missing_reads += usize::from(!available);
        observation.address_checksum = observation
            .address_checksum
            .wrapping_mul(1_099_511_628_211)
            .wrapping_add(frame as u64)
            .wrapping_mul(1_099_511_628_211)
            .wrapping_add(channel as u64)
            .wrapping_mul(1_099_511_628_211)
            .wrapping_add(u64::from(right));
        cell.set(observation);
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExplicitSeekMode {
    Normal,
    BeforeLoop,
    AfterLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl FrameRange {
    pub(crate) fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

/// Both dry and native reads require the whole admitted physical source context.
/// Normal needs its loop knots, Intro the prefix plus loop, and Tail the real source remainder
/// plus loop return. Complete views remain eligible explicit exceptions under the same proof.
pub(crate) fn resident_read_context_available(
    sample: &SampleBuffer,
    region: FrameRange,
    mode: ExplicitSeekMode,
    _key_lock: bool,
) -> bool {
    super::native_source_coverage::normal_loop_context_available(sample, region, mode)
}

/// One copied source-addressing domain. Musical duration is in loaded source frames;
/// the admitted physical PCM range and its persisted integer endpoints stay unchanged.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceLoopDomain {
    pub(crate) sample_frames: usize,
    pub(crate) region: FrameRange,
    pub(crate) musical_period: SourceReaderPeriod,
}

impl PartialEq for SourceLoopDomain {
    fn eq(&self, other: &Self) -> bool {
        self.sample_frames == other.sample_frames
            && self.region == other.region
            && self.musical_period == other.musical_period
    }
}

impl SourceLoopDomain {
    pub(crate) fn is_valid(self) -> bool {
        if self.region.start >= self.region.end || self.region.end > self.sample_frames {
            return false;
        }
        match self.musical_period {
            SourceReaderPeriod::Physical => true,
            SourceReaderPeriod::ScalarMusical(bits) => {
                Self::musical(self.sample_frames, self.region, f64::from_bits(bits)).is_some()
            }
            SourceReaderPeriod::MappedRational(period) => period.validate_physical(self.region),
        }
    }

    /// Canonicalize an already owned normal virtual source phase in this domain.
    /// Callers separately guard source, physical geometry and timing ownership;
    /// this projection alone never authorizes native/filter-history continuation.
    pub(crate) fn project_normal_position(
        self,
        position: FractionalSourcePosition,
    ) -> Option<FractionalSourcePosition> {
        if self.musical_period.is_mapped()
            || position.seek_mode != ExplicitSeekMode::Normal
            || !position.fraction.is_finite()
            || !(0.0..1.0).contains(&position.fraction)
            || position.frame < self.region.start
            || self.region.start >= self.region.end
            || self.region.end > self.sample_frames
        {
            return None;
        }
        let period = self
            .musical_period
            .scalar_period()
            .unwrap_or(self.region.len() as f64);
        if !period.is_finite() || period <= 0.0 || (period - self.region.len() as f64).abs() > 1.0 {
            return None;
        }
        if period == self.region.len() as f64 {
            return Some(FractionalSourcePosition {
                frame: self.region.start + (position.frame - self.region.start) % self.region.len(),
                ..position
            });
        }
        let phase =
            ((position.frame - self.region.start) as f64 + position.fraction).rem_euclid(period);
        let whole = phase.floor() as usize;
        Some(FractionalSourcePosition {
            frame: self.region.start + whole,
            fraction: phase - whole as f64,
            seek_mode: ExplicitSeekMode::Normal,
        })
    }

    pub(crate) fn physical(sample_frames: usize, region: FrameRange) -> Self {
        debug_assert!(region.start < region.end && region.end <= sample_frames);
        Self {
            sample_frames,
            region,
            musical_period: SourceReaderPeriod::Physical,
        }
    }

    /// Accept only a bounded rounding mismatch. Accepted timing ownership and the
    /// compatible logical beat count are resolved by the productive caller.
    pub(crate) fn musical(sample_frames: usize, region: FrameRange, period: f64) -> Option<Self> {
        if region.start >= region.end
            || region.end > sample_frames
            || !period.is_finite()
            || period <= 0.0
            || (period - region.len() as f64).abs() > 1.0
        {
            return None;
        }
        Some(Self {
            sample_frames,
            region,
            musical_period: SourceReaderPeriod::from_scalar_period(Some(period)),
        })
    }

    /// Arithmetic certificate only. The owning caller separately qualifies PCM,
    /// original map, timing, source lifetime and the complete actual-E effect.
    pub(crate) fn mapped(
        sample_frames: usize,
        region: FrameRange,
        period: super::general_source_numerics::MappedReaderPeriod,
    ) -> Option<Self> {
        (region.start < region.end
            && region.end <= sample_frames
            && period.validate_physical(region))
        .then_some(Self {
            sample_frames,
            region,
            musical_period: SourceReaderPeriod::MappedRational(period),
        })
    }
}

pub(crate) fn effective_loop_region(
    start: usize,
    end: Option<usize>,
    sample_frames: usize,
) -> Option<FrameRange> {
    if sample_frames == 0 {
        return None;
    }

    let start = start.min(sample_frames);
    let end = end.unwrap_or(sample_frames).min(sample_frames);
    if end <= start {
        Some(FrameRange {
            start: 0,
            end: sample_frames,
        })
    } else {
        Some(FrameRange { start, end })
    }
}

pub(crate) fn playhead_before_render(
    frame: usize,
    region: FrameRange,
    mode: ExplicitSeekMode,
) -> usize {
    if mode == ExplicitSeekMode::Normal && (frame < region.start || frame >= region.end) {
        region.start
    } else {
        frame
    }
}

pub(crate) fn explicit_seek_mode_for_frame(
    frame: usize,
    loop_region: FrameRange,
    sample_frames: usize,
) -> ExplicitSeekMode {
    if frame < loop_region.start {
        ExplicitSeekMode::BeforeLoop
    } else if frame >= loop_region.end && loop_region.end < sample_frames {
        ExplicitSeekMode::AfterLoop
    } else {
        ExplicitSeekMode::Normal
    }
}

pub(crate) fn source_frame_for_playback(
    frame_pos: usize,
    offset: usize,
    sample_frames: usize,
    loop_region: FrameRange,
    seek_mode: ExplicitSeekMode,
) -> usize {
    let loop_len = loop_region.len();
    debug_assert!(loop_len > 0);

    match seek_mode {
        ExplicitSeekMode::Normal => {
            let base = frame_pos.saturating_sub(loop_region.start);
            loop_region.start + ((base + offset) % loop_len)
        }
        ExplicitSeekMode::BeforeLoop => {
            let frame = frame_pos.saturating_add(offset);
            if frame < loop_region.start {
                frame
            } else {
                loop_region.start + ((frame - loop_region.start) % loop_len)
            }
        }
        ExplicitSeekMode::AfterLoop => {
            let frame = frame_pos.saturating_add(offset);
            if frame < sample_frames {
                frame
            } else {
                loop_region.start + ((frame - sample_frames) % loop_len)
            }
        }
    }
}

pub(crate) fn advance_playback_position(
    frame_pos: usize,
    input_frames: usize,
    sample_frames: usize,
    loop_region: FrameRange,
    seek_mode: ExplicitSeekMode,
) -> (usize, ExplicitSeekMode) {
    let loop_len = loop_region.len();
    debug_assert!(loop_len > 0);

    match seek_mode {
        ExplicitSeekMode::Normal => {
            let base = frame_pos.saturating_sub(loop_region.start);
            (
                loop_region.start + ((base + input_frames) % loop_len),
                ExplicitSeekMode::Normal,
            )
        }
        ExplicitSeekMode::BeforeLoop => {
            let frame = frame_pos.saturating_add(input_frames);
            if frame < loop_region.start {
                (frame, ExplicitSeekMode::BeforeLoop)
            } else {
                (
                    loop_region.start + ((frame - loop_region.start) % loop_len),
                    ExplicitSeekMode::Normal,
                )
            }
        }
        ExplicitSeekMode::AfterLoop => {
            let frame = frame_pos.saturating_add(input_frames);
            if frame < sample_frames {
                (frame, ExplicitSeekMode::AfterLoop)
            } else {
                (
                    loop_region.start + ((frame - sample_frames) % loop_len),
                    ExplicitSeekMode::Normal,
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StemRenderSelection {
    mode: StemMixMode,
    source_version_hash: u64,
    enabled_mask: u8,
    /// Comparison only: every copied plan must separately retain this immutable owner.
    /// The address is never dereferenced and cannot outlive the held complete-set identity.
    complete_set_owner: Option<usize>,
}

impl StemRenderSelection {
    pub(crate) fn required_stem_hash(self) -> Option<u64> {
        (self.mode == StemMixMode::AllStems).then_some(self.source_version_hash)
    }

    pub(crate) fn full_mix() -> Self {
        Self {
            mode: StemMixMode::FullMix,
            source_version_hash: 0,
            enabled_mask: STEM_COMPONENT_MASK,
            complete_set_owner: None,
        }
    }

    pub(crate) fn for_stems(stems: &PreparedStemSet, enabled_mask: u8) -> Self {
        Self {
            mode: StemMixMode::AllStems,
            source_version_hash: stems.source_version_hash,
            enabled_mask: enabled_mask & STEM_COMPONENT_MASK,
            complete_set_owner: Some(std::sync::Arc::as_ptr(&stems.complete_set_identity) as usize),
        }
    }

    fn matches_stems(self, stems: &PreparedStemSet) -> bool {
        self.mode == StemMixMode::AllStems
            && self.source_version_hash != 0
            && self.source_version_hash == stems.source_version_hash
            && self.complete_set_owner.is_none_or(|owner| {
                owner == std::sync::Arc::as_ptr(&stems.complete_set_identity) as usize
            })
    }

    /// Match a copied historical descriptor only. The caller must still resolve and
    /// validate its real physical owner before this selection can read any PCM.
    pub(crate) fn matches_stem_descriptor(self, owner: usize, source_version_hash: u64) -> bool {
        self.mode == StemMixMode::AllStems
            && self.source_version_hash != 0
            && self.source_version_hash == source_version_hash
            && self
                .complete_set_owner
                .is_none_or(|expected| expected == owner)
    }

    #[cfg(test)]
    pub(crate) fn from_state(
        mode: StemMixMode,
        source_version_hash: u64,
        enabled_mask: u8,
    ) -> StemRenderSelection {
        match mode {
            StemMixMode::FullMix => StemRenderSelection::full_mix(),
            StemMixMode::AllStems => StemRenderSelection {
                mode,
                source_version_hash,
                enabled_mask: enabled_mask & STEM_COMPONENT_MASK,
                complete_set_owner: None,
            },
        }
    }
}

/// Fixed borrowed readers for the actual current and unfinished outgoing selection.
/// A scalar selection token has authority only while its corresponding owner is held.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StemReadSets<'a> {
    pub(crate) current: Option<&'a PreparedStemSet>,
    pub(crate) outgoing: Option<&'a PreparedStemSet>,
}

impl<'a> StemReadSets<'a> {
    pub(crate) fn new(
        current: Option<&'a PreparedStemSet>,
        outgoing: Option<&'a PreparedStemSet>,
    ) -> Self {
        Self { current, outgoing }
    }

    pub(crate) fn for_selection(
        self,
        selection: StemRenderSelection,
    ) -> Option<&'a PreparedStemSet> {
        self.current
            .filter(|stems| selection.matches_stems(stems))
            .or_else(|| {
                // Legacy hash-only state identifies only the corresponding current set.
                // It must never alias a different generation in the outgoing slot.
                self.outgoing.filter(|stems| {
                    selection.complete_set_owner.is_some() && selection.matches_stems(stems)
                })
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StemTransition {
    from: StemRenderSelection,
    elapsed_anchor: f64,
    output_frames: u64,
    output_ratio: f64,
    total_frames: usize,
}

impl StemTransition {
    fn inactive() -> Self {
        Self {
            from: StemRenderSelection::full_mix(),
            elapsed_anchor: 0.0,
            output_frames: 0,
            output_ratio: 0.0,
            total_frames: 0,
        }
    }

    pub(crate) fn start(from: StemRenderSelection, total_frames: usize) -> Self {
        if total_frames == 0 {
            return Self::inactive();
        }

        Self {
            from,
            elapsed_anchor: 0.0,
            output_frames: 0,
            output_ratio: 0.0,
            total_frames,
        }
    }

    pub(crate) fn is_active(self) -> bool {
        self.total_frames > 0 && self.elapsed_frames() < self.total_frames as f64
    }

    pub(crate) fn required_from_selection(self) -> Option<StemRenderSelection> {
        self.is_active().then_some(self.from)
    }

    pub(crate) fn matches_source_contract(self, other: Self) -> bool {
        self.from == other.from && self.total_frames == other.total_frames
    }

    /// Preserve complete source-selection ramp state at a prepared adoption boundary.
    pub(crate) fn matches_exact(self, other: Self) -> bool {
        self.from == other.from
            && self.elapsed_anchor.to_bits() == other.elapsed_anchor.to_bits()
            && self.output_frames == other.output_frames
            && self.output_ratio.to_bits() == other.output_ratio.to_bits()
            && self.total_frames == other.total_frames
    }

    fn elapsed_frames(self) -> f64 {
        self.elapsed_anchor + self.output_frames as f64 * self.output_ratio
    }

    /// Evaluate the same constant-rate epoch at an output offset, before rounding its phase.
    /// A copied reader may have a new applied rate before its first advance in that rate.
    fn elapsed_at_output_offset(self, frames: usize, ratio: f64) -> f64 {
        debug_assert!((super::constants::SPEED_MIN..=super::constants::SPEED_MAX).contains(&ratio));
        if self.output_ratio.to_bits() == ratio.to_bits() {
            let count = match self.output_frames.checked_add(frames as u64) {
                Some(count) => count as f64,
                // Normal rendering retains only a short active epoch. Keep an oversized
                // read-only offset correct without a wide conversion on the ordinary path.
                None => (u128::from(self.output_frames) + frames as u128) as f64,
            };
            self.elapsed_anchor + count * ratio
        } else {
            self.elapsed_frames() + frames as f64 * ratio
        }
    }

    fn gains_at_elapsed(self, elapsed: f64) -> (f32, f32) {
        if !self.is_active() {
            return (0.0, 1.0);
        }
        let to_gain = elapsed.min(self.total_frames as f64) as f32 / self.total_frames as f32;
        (1.0 - to_gain, to_gain)
    }

    fn gains_at_output_offset(self, frames: usize, ratio: f64) -> (f32, f32) {
        if !self.is_active() {
            return (0.0, 1.0);
        }
        self.gains_at_elapsed(self.elapsed_at_output_offset(frames, ratio))
    }

    fn gains_at(self, frame_offset: f64) -> (f32, f32) {
        self.gains_at_elapsed(self.elapsed_frames() + frame_offset)
    }

    #[cfg(test)]
    pub(crate) fn advance(&mut self, frames: usize) {
        self.advance_fractional(frames as f64);
    }

    /// Advance one applied-rate epoch by its integer output count, independently of partitions.
    /// Actual source rates are .5..2, so a production 128-source-frame ramp retains at most 256
    /// output frames in an epoch (the larger resource fixtures are bounded in the same way).
    pub(crate) fn advance_output_frames(&mut self, frames: usize, ratio: f64) {
        if frames == 0 || !self.is_active() {
            return;
        }
        debug_assert!((super::constants::SPEED_MIN..=super::constants::SPEED_MAX).contains(&ratio));
        if self.output_ratio.to_bits() != ratio.to_bits() {
            self.elapsed_anchor = self.elapsed_frames();
            self.output_frames = 0;
            self.output_ratio = ratio;
        }
        // A large completing call must clear before converting the retained active count.
        let count = u128::from(self.output_frames) + frames as u128;
        if self.elapsed_anchor + count as f64 * ratio >= self.total_frames as f64 {
            self.clear();
        } else {
            self.output_frames = u64::try_from(count)
                .expect("bounded active source ramp retains an integer output count");
        }
    }

    /// Explicit source-distance probes do not define a rate epoch for later output reads.
    #[cfg(test)]
    pub(crate) fn advance_fractional(&mut self, frames: f64) {
        if frames == 0.0 || !self.is_active() {
            return;
        }
        self.elapsed_anchor = self.elapsed_frames() + frames;
        self.output_frames = 0;
        self.output_ratio = 0.0;
        if self.elapsed_anchor >= self.total_frames as f64 {
            self.clear();
        }
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::inactive();
    }
}

impl Default for StemTransition {
    fn default() -> Self {
        Self::inactive()
    }
}

pub(crate) fn full_stem_available_mask() -> u8 {
    ((1_u16 << STEM_BUFFER_COUNT) - 1) as u8
}

pub(crate) fn stem_index_mask(index: usize) -> u8 {
    if index >= u8::BITS as usize {
        return 0;
    }

    1_u8 << index
}

pub(crate) fn prepared_stem_set_matches_sample(
    stems: &PreparedStemSet,
    sample: &SampleBuffer,
    channels: usize,
    sample_rate_hz: f32,
    sample_frames: usize,
) -> bool {
    if channels == 0
        || sample_frames == 0
        || sample.source_sample_count() != sample_frames * channels
    {
        return false;
    }

    let rounded_sample_rate_hz = sample_rate_hz.round();
    if !rounded_sample_rate_hz.is_finite()
        || rounded_sample_rate_hz <= 0.0
        || stems.sample_rate_hz != rounded_sample_rate_hz as u32
    {
        return false;
    }

    if stems.channels != channels
        || stems.frame_count != sample_frames
        || stems.available_mask != full_stem_available_mask()
        || stems.source_version_hash == 0
        || !std::sync::Arc::ptr_eq(&stems.reference_samples, &sample.samples)
    {
        return false;
    }

    stems.stems.iter().all(|stem| {
        stem.channels == channels
            && stem.samples.len() == sample.samples.len()
            && (sample.residency.is_none() || stem.same_window(sample))
    })
}

pub(crate) fn prepared_stem_set_for_render<'a>(
    stems: Option<&'a PreparedStemSet>,
    sample: &SampleBuffer,
    channels: usize,
    sample_rate_hz: f32,
    sample_frames: usize,
    accepted_timing: Option<super::constant_timing::AcceptedTimingProjection>,
) -> Option<&'a PreparedStemSet> {
    stems.filter(|stems| {
        stems.accepted_timing == accepted_timing
            && prepared_stem_set_matches_sample(
                stems,
                sample,
                channels,
                sample_rate_hz,
                sample_frames,
            )
    })
}

pub(crate) fn render_source_sample(
    sample: &SampleBuffer,
    stems: Option<&PreparedStemSet>,
    enabled_stem_mask: u8,
    frame: usize,
    channels: usize,
    channel: usize,
) -> f32 {
    debug_assert_eq!(channels, sample.channels);
    if let Some(stems) = stems {
        let enabled_stem_mask = enabled_stem_mask & STEM_COMPONENT_MASK;
        stems
            .stems
            .iter()
            .enumerate()
            .filter(|(stem_index, _)| enabled_stem_mask & stem_index_mask(*stem_index) != 0)
            .map(|(_, stem)| stem.sample_at_absolute(frame, channel))
            .sum()
    } else {
        sample.sample_at_absolute(frame, channel)
    }
}

fn render_source_selection_sample_with_readers(
    sample: &SampleBuffer,
    readers: StemReadSets<'_>,
    selection: StemRenderSelection,
    frame: usize,
    channels: usize,
    channel: usize,
) -> f32 {
    match selection.mode {
        StemMixMode::FullMix => sample.sample_at_absolute(frame, channel),
        StemMixMode::AllStems => {
            let matching_stems = readers.for_selection(selection);
            if matching_stems.is_none() && selection.complete_set_owner.is_some() {
                // Checked feeds reject this missing owner before mutation. An unchecked dry
                // read must still never substitute another generation or FullMix for it.
                return 0.0;
            }
            render_source_sample(
                sample,
                matching_stems,
                selection.enabled_mask,
                frame,
                channels,
                channel,
            )
        }
    }
}

/// One immutable view of source selection and source-frame addressing for a render segment.
///
/// Advancing a voice or a transition remains the caller's responsibility. This keeps a worker's
/// source reads independent from live transport and makes repeated reads deterministic.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceReadPlan {
    pub(crate) channels: usize,
    pub(crate) sample_frames: usize,
    pub(crate) frame_pos: usize,
    pub(crate) loop_region: FrameRange,
    /// Virtual compatible musical duration; never an additional playback-rate ratio.
    pub(crate) loop_period: SourceReaderPeriod,
    pub(crate) seek_mode: ExplicitSeekMode,
    pub(crate) selection: StemRenderSelection,
    pub(crate) transition: StemTransition,
}

/// Ephemeral once-per-chunk seam constants. The copied plan remains the complete
/// domain witness; this context holds no source, map owner or mutable voice state.
enum PreparedSourceTapPlan {
    Legacy,
    Mapped(super::general_source_numerics::PreparedMappedReaderPeriod),
}

impl PreparedSourceTapPlan {
    fn new(plan: SourceReadPlan) -> Option<Self> {
        match plan.loop_period.mapped_period() {
            Some(period) => Some(Self::Mapped(period.prepare(plan.loop_region)?)),
            None => Some(Self::Legacy),
        }
    }

    fn taps(&self, plan: SourceReadPlan, fraction: f64) -> Option<FractionalSourceTaps> {
        match self {
            Self::Legacy => plan.try_fractional_taps(fraction),
            Self::Mapped(period) => {
                if plan.seek_mode != ExplicitSeekMode::Normal {
                    return None;
                }
                let taps = period.seam_taps(plan.frame_pos, fraction)?;
                Some(FractionalSourceTaps {
                    left: taps.left,
                    right: taps.right,
                    right_gain: taps.right_gain,
                })
            }
        }
    }
}

impl SourceReadPlan {
    /// Keep the exact readers used by either side, including an empty selected mask.
    pub(crate) fn required_read_sets<'a>(self, readers: StemReadSets<'a>) -> StemReadSets<'a> {
        let current = readers.for_selection(self.selection);
        let outgoing = self
            .transition
            .required_from_selection()
            .and_then(|selection| readers.for_selection(selection));
        let used = |stems: &PreparedStemSet| {
            current
                .into_iter()
                .chain(outgoing)
                .any(|held| std::ptr::eq(stems, held))
        };
        StemReadSets::new(
            readers.current.filter(|stems| used(stems)),
            readers.outgoing.filter(|stems| used(stems)),
        )
    }
    pub(crate) fn domain(self) -> SourceLoopDomain {
        SourceLoopDomain {
            sample_frames: self.sample_frames,
            region: self.loop_region,
            musical_period: self.loop_period,
        }
    }

    /// Change only the explicit reader circle, retaining the same physical PCM
    /// and selection. Adoption still proves canonical phase and first taps.
    pub(crate) fn with_effective_domain(self, domain: SourceLoopDomain) -> Option<Self> {
        (self.sample_frames == domain.sample_frames
            && self.loop_region == domain.region
            && domain.is_valid())
        .then_some(Self {
            loop_period: domain.musical_period,
            ..self
        })
    }

    /// Compare actual source addressing and selection, including an unfinished transition.
    ///
    /// Source pins and current timing authority are checked separately by the owning permit.
    pub(crate) fn matches_exact(self, other: Self) -> bool {
        self.channels == other.channels
            && self.sample_frames == other.sample_frames
            && self.frame_pos == other.frame_pos
            && self.loop_region == other.loop_region
            && self.loop_period == other.loop_period
            && self.seek_mode == other.seek_mode
            && self.selection == other.selection
            && self.transition.matches_exact(other.transition)
    }

    /// Check source selection and boundaries while a pending copied trajectory advances.
    ///
    /// Frame/seek phase and transition progress are compared against the advanced checkpoint
    /// only at adoption. An explicit seek is separately fenced by the owning preparation epoch.
    pub(crate) fn matches_source_contract(self, other: Self) -> bool {
        self.channels == other.channels
            && self.sample_frames == other.sample_frames
            && self.loop_region == other.loop_region
            && self.loop_period == other.loop_period
            && self.selection == other.selection
    }

    /// Fill fixed output-domain storage from a configured constant-ratio source chunk.
    ///
    /// Neither the source clock nor this plan's selection transition advances. The caller owns
    /// their progression, so a worker can fill the same feed using independent copied state.
    pub(crate) fn fill_fractional_buffers(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        playback: &SourcePlayback,
        buffers: &mut [Vec<f32>],
        output_frames: usize,
    ) -> Result<(), super::rubberband_backend::RubberBandError> {
        self.fill_fractional_buffers_with_readers(
            sample,
            StemReadSets::new(stems, None),
            playback,
            buffers,
            output_frames,
        )
    }

    pub(crate) fn fill_fractional_buffers_with_readers(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        playback: &SourcePlayback,
        buffers: &mut [Vec<f32>],
        output_frames: usize,
    ) -> Result<(), super::rubberband_backend::RubberBandError> {
        if let Some(view) = playback
            .local_compact_view()
            .map_err(|_| super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?
        {
            return self.fill_fractional_buffers_queried(
                sample,
                readers,
                playback,
                buffers,
                output_frames,
                |frame| view.position_at(frame),
            );
        }
        self.fill_fractional_buffers_queried(
            sample,
            readers,
            playback,
            buffers,
            output_frames,
            |frame| playback.position_at(frame),
        )
    }

    /// Explicit compact coordinates through the original interpolation loop.
    /// The caller still retains and qualifies original PCM/map reader custody.
    pub(crate) fn fill_fractional_buffers_compact_with_readers(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        playback: &SourcePlayback,
        geometry: &super::mapped_source_clock::MappedLoopGeometry,
        buffers: &mut [Vec<f32>],
        output_frames: usize,
    ) -> Result<(), super::rubberband_backend::RubberBandError> {
        let view = playback
            .compact_view(geometry)
            .map_err(|_| super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?;
        self.fill_fractional_buffers_queried(
            sample,
            readers,
            playback,
            buffers,
            output_frames,
            |frame| view.position_at(frame),
        )
    }

    fn fill_fractional_buffers_queried(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        playback: &SourcePlayback,
        buffers: &mut [Vec<f32>],
        output_frames: usize,
        position_at: impl Fn(usize) -> Option<FractionalSourcePosition>,
    ) -> Result<(), super::rubberband_backend::RubberBandError> {
        if self.loop_period.is_mapped()
            && (!self.domain().is_valid()
                || !playback.matches_domain(self.domain())
                || self.channels == 0
                || self.channels != sample.channels
                || self.sample_frames != sample.frame_count()
                || buffers.len() < self.channels
                || buffers
                    .iter()
                    .take(self.channels)
                    .any(|buffer| buffer.len() < output_frames))
        {
            return Err(super::rubberband_backend::RubberBandError::InvalidBlockSize);
        }
        let tap_plan = PreparedSourceTapPlan::new(self)
            .ok_or(super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?;
        debug_assert!(self.channels > 0 && buffers.len() >= self.channels);
        debug_assert!(playback.matches_domain(self.domain()));
        debug_assert_eq!(sample.frame_count(), self.sample_frames);
        debug_assert!(
            buffers
                .iter()
                .take(self.channels)
                .all(|buffer| buffer.len() >= output_frames)
        );
        if !playback.can_read_frames(output_frames) {
            return Err(super::rubberband_backend::RubberBandError::UnavailableSourceHistory);
        }
        // The exact mapped seam is fallible at this public copied-plan boundary.
        // Validate the entire requested chunk before changing any output buffer.
        // The admitted scalar path retains its preceding single-pass work.
        if self.loop_period.is_mapped() {
            for offset in 0..output_frames {
                let position = position_at(offset)
                    .ok_or(super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?;
                let plan = Self {
                    frame_pos: position.frame,
                    seek_mode: position.seek_mode,
                    ..self
                };
                let taps = tap_plan
                    .taps(plan, position.fraction)
                    .ok_or(super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?;
                for channel in 0..self.channels {
                    if !plan.tap_available_with_readers(sample, readers, taps.left, channel) {
                        #[cfg(test)]
                        observe_tap(taps.left, channel, false, false);
                        return Err(super::rubberband_backend::RubberBandError::InvalidBlockSize);
                    }
                    if let Some(right) = taps.right
                        && !plan.tap_available_with_readers(sample, readers, right, channel)
                    {
                        #[cfg(test)]
                        observe_tap(right, channel, true, false);
                        return Err(super::rubberband_backend::RubberBandError::InvalidBlockSize);
                    }
                }
            }
        }
        let tempo_ratio = playback.tempo_ratio();
        for frame in 0..output_frames {
            let position = position_at(frame)
                .ok_or(super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?;
            let plan = Self {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                ..self
            };
            let taps = tap_plan
                .taps(plan, position.fraction)
                .ok_or(super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?;
            let transition_gains = self.transition.gains_at_output_offset(frame, tempo_ratio);
            for (channel, buffer) in buffers.iter_mut().enumerate().take(self.channels) {
                buffer[frame] = plan.sample_at_taps_with_gains(
                    sample,
                    readers,
                    taps,
                    transition_gains,
                    channel,
                );
            }
        }
        Ok(())
    }

    /// Reject unavailable actual taps before feeding any native/FIFO state.
    /// Validation borrows only already held PCM and fixed scalar addressing state.
    pub(crate) fn fill_fractional_buffers_checked(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        playback: &SourcePlayback,
        buffers: &mut [Vec<f32>],
        output_frames: usize,
    ) -> Result<(), super::rubberband_backend::RubberBandError> {
        self.fill_fractional_buffers_checked_with_readers(
            sample,
            StemReadSets::new(stems, None),
            playback,
            buffers,
            output_frames,
        )
    }

    pub(crate) fn fill_fractional_buffers_checked_with_readers(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        playback: &SourcePlayback,
        buffers: &mut [Vec<f32>],
        output_frames: usize,
    ) -> Result<(), super::rubberband_backend::RubberBandError> {
        if let Some(view) = playback
            .local_compact_view()
            .map_err(|_| super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?
        {
            return self.fill_fractional_buffers_checked_queried(
                sample,
                readers,
                playback,
                buffers,
                output_frames,
                |frame| view.position_at(frame),
            );
        }
        self.fill_fractional_buffers_checked_queried(
            sample,
            readers,
            playback,
            buffers,
            output_frames,
            |frame| playback.position_at(frame),
        )
    }

    pub(crate) fn fill_fractional_buffers_checked_compact_with_readers(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        playback: &SourcePlayback,
        geometry: &super::mapped_source_clock::MappedLoopGeometry,
        buffers: &mut [Vec<f32>],
        output_frames: usize,
    ) -> Result<(), super::rubberband_backend::RubberBandError> {
        let view = playback
            .compact_view(geometry)
            .map_err(|_| super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?;
        self.fill_fractional_buffers_checked_queried(
            sample,
            readers,
            playback,
            buffers,
            output_frames,
            |frame| view.position_at(frame),
        )
    }

    fn fill_fractional_buffers_checked_queried(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        playback: &SourcePlayback,
        buffers: &mut [Vec<f32>],
        output_frames: usize,
        position_at: impl Fn(usize) -> Option<FractionalSourcePosition>,
    ) -> Result<(), super::rubberband_backend::RubberBandError> {
        if self.channels == 0
            || sample.channels != self.channels
            || sample.frame_count() != self.sample_frames
            || !playback.matches_domain(self.domain())
            || buffers.len() < self.channels
            || buffers
                .iter()
                .take(self.channels)
                .any(|buffer| buffer.len() < output_frames)
        {
            return Err(super::rubberband_backend::RubberBandError::InvalidBlockSize);
        }
        if !playback.can_read_frames(output_frames) {
            return Err(super::rubberband_backend::RubberBandError::UnavailableSourceHistory);
        }
        let tap_plan = PreparedSourceTapPlan::new(self)
            .ok_or(super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?;
        for offset in 0..output_frames {
            let position = position_at(offset)
                .ok_or(super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?;
            let plan = Self {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                ..self
            };
            let taps = tap_plan
                .taps(plan, position.fraction)
                .ok_or(super::rubberband_backend::RubberBandError::UnavailableSourceHistory)?;
            for channel in 0..self.channels {
                if !plan.tap_available_with_readers(sample, readers, taps.left, channel) {
                    #[cfg(test)]
                    observe_tap(taps.left, channel, false, false);
                    return Err(super::rubberband_backend::RubberBandError::InvalidBlockSize);
                }
                if let Some(right) = taps.right
                    && !plan.tap_available_with_readers(sample, readers, right, channel)
                {
                    #[cfg(test)]
                    observe_tap(right, channel, true, false);
                    return Err(super::rubberband_backend::RubberBandError::InvalidBlockSize);
                }
            }
        }
        self.fill_fractional_buffers_queried(
            sample,
            readers,
            playback,
            buffers,
            output_frames,
            position_at,
        )
    }

    pub(crate) fn tap_available_with_readers(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        frame: usize,
        channel: usize,
    ) -> bool {
        let held = |buffer: &SampleBuffer| {
            channel < buffer.channels
                && frame
                    .checked_sub(buffer.resident_start())
                    .and_then(|local| local.checked_mul(buffer.channels))
                    .and_then(|base| base.checked_add(channel))
                    .is_some_and(|index| index < buffer.samples.len())
        };
        let selection_available = |selection: StemRenderSelection| {
            let matching = readers.for_selection(selection);
            matching.map_or_else(
                || selection.complete_set_owner.is_none() && held(sample),
                |set| {
                    set.stems
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| selection.enabled_mask & stem_index_mask(*index) != 0)
                        .all(|(_, component)| held(component))
                },
            )
        };
        selection_available(self.selection)
            && (!self.transition.is_active() || selection_available(self.transition.from))
    }

    #[cfg(test)]
    pub(crate) fn sample_at_offset(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        source_offset: usize,
        channel: usize,
    ) -> f32 {
        self.sample_at_offset_with_progress(
            sample,
            stems,
            source_offset,
            source_offset as f64,
            channel,
        )
    }

    #[cfg(test)]
    fn sample_at_offset_with_progress(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        source_offset: usize,
        transition_progress: f64,
        channel: usize,
    ) -> f32 {
        let frame = source_frame_for_playback(
            self.frame_pos,
            source_offset,
            self.sample_frames,
            self.loop_region,
            self.seek_mode,
        );
        self.sample_at_frame_with_progress(
            sample,
            StemReadSets::new(stems, None),
            frame,
            transition_progress,
            channel,
        )
    }

    #[cfg(test)]
    fn sample_at_frame_with_progress(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        frame: usize,
        transition_progress: f64,
        channel: usize,
    ) -> f32 {
        self.sample_at_frame_with_gains(
            sample,
            readers,
            frame,
            self.transition.gains_at(transition_progress),
            channel,
        )
    }

    fn sample_at_frame_with_gains(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        frame: usize,
        transition_gains: (f32, f32),
        channel: usize,
    ) -> f32 {
        if self.transition.is_active() {
            let from_sample = render_source_selection_sample_with_readers(
                sample,
                readers,
                self.transition.from,
                frame,
                self.channels,
                channel,
            );
            let to_sample = render_source_selection_sample_with_readers(
                sample,
                readers,
                self.selection,
                frame,
                self.channels,
                channel,
            );
            let (from_gain, to_gain) = transition_gains;
            from_sample * from_gain + to_sample * to_gain
        } else {
            render_source_selection_sample_with_readers(
                sample,
                readers,
                self.selection,
                frame,
                self.channels,
                channel,
            )
        }
    }

    /// Interpolate two independently addressed source taps at the same crossfade progress.
    /// Lookahead follows loop/intro/tail policy without advancing playback or the ramp.
    #[cfg(test)]
    pub(crate) fn sample_fractional(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        fraction: f64,
        transition_progress: f64,
        channel: usize,
    ) -> f32 {
        self.sample_fractional_with_readers(
            sample,
            StemReadSets::new(stems, None),
            fraction,
            transition_progress,
            channel,
        )
    }

    /// The bounded live-policy preflight uses this same interpolation path after
    /// validating coverage. It does not record a productive source-read receipt.
    pub(crate) fn try_sample_fractional_with_readers(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        fraction: f64,
        transition_progress: f64,
        channel: usize,
    ) -> Option<f32> {
        let taps = self.try_fractional_taps(fraction)?;
        Some(self.sample_at_taps_with_gains(
            sample,
            readers,
            taps,
            self.transition.gains_at(transition_progress),
            channel,
        ))
    }

    /// Historical scalar fixtures use the same checked implementation. Productive
    /// readers and conversion preflight propagate failure explicitly instead.
    #[cfg(test)]
    pub(crate) fn sample_fractional_with_readers(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        fraction: f64,
        transition_progress: f64,
        channel: usize,
    ) -> f32 {
        assert!(!self.loop_period.is_mapped());
        self.try_sample_fractional_with_readers(
            sample,
            readers,
            fraction,
            transition_progress,
            channel,
        )
        .expect("valid legacy scalar fixture taps")
    }

    fn sample_at_taps_with_gains(
        self,
        sample: &SampleBuffer,
        readers: StemReadSets<'_>,
        taps: FractionalSourceTaps,
        transition_gains: (f32, f32),
        channel: usize,
    ) -> f32 {
        #[cfg(test)]
        observe_tap(
            taps.left,
            channel,
            false,
            self.tap_available_with_readers(sample, readers, taps.left, channel),
        );
        let left =
            self.sample_at_frame_with_gains(sample, readers, taps.left, transition_gains, channel);
        let Some(right_frame) = taps.right else {
            return left;
        };
        #[cfg(test)]
        observe_tap(
            right_frame,
            channel,
            true,
            self.tap_available_with_readers(sample, readers, right_frame, channel),
        );
        let right = self.sample_at_frame_with_gains(
            sample,
            readers,
            right_frame,
            transition_gains,
            channel,
        );
        left + (right - left) * taps.right_gain
    }

    /// The single authority for both executed interpolation addresses and coverage checks.
    pub(crate) fn try_fractional_taps(self, fraction: f64) -> Option<FractionalSourceTaps> {
        if let Some(period) = self.loop_period.mapped_period() {
            if self.seek_mode != ExplicitSeekMode::Normal {
                return None;
            }
            let taps = period.seam_taps(self.frame_pos, fraction, self.loop_region)?;
            return Some(FractionalSourceTaps {
                left: taps.left,
                right: taps.right,
                right_gain: taps.right_gain,
            });
        }
        if let Some(period) = self.loop_period.scalar_period()
            && period != self.loop_region.len() as f64
            && self.seek_mode == ExplicitSeekMode::Normal
        {
            // Integer knots retain their absolute physical source addresses. Only the
            // final interval joins the last admitted knot to the first at virtual P.
            // A longer P never reads the exclusive physical end; a shorter P never
            // interpolates past the next virtual wrap.
            let phase = (self.frame_pos.saturating_sub(self.loop_region.start) as f64 + fraction)
                .rem_euclid(period);
            let last = (period.ceil() as usize)
                .saturating_sub(1)
                .min(self.loop_region.len() - 1);
            if phase >= last as f64 {
                let alpha = (phase - last as f64) / (period - last as f64);
                return Some(FractionalSourceTaps {
                    left: self.loop_region.start + last,
                    right: Some(self.loop_region.start),
                    right_gain: alpha as f32,
                });
            }
        }
        let frame = |offset| {
            source_frame_for_playback(
                self.frame_pos,
                offset,
                self.sample_frames,
                self.loop_region,
                self.seek_mode,
            )
        };
        Some(FractionalSourceTaps {
            left: frame(0),
            right: (fraction != 0.0).then(|| frame(1)),
            right_gain: fraction as f32,
        })
    }

    #[cfg(test)]
    pub(crate) fn fractional_taps(self, fraction: f64) -> FractionalSourceTaps {
        assert!(!self.loop_period.is_mapped());
        self.try_fractional_taps(fraction)
            .expect("valid legacy scalar fixture taps")
    }

    /// Fills preallocated planar buffers with integer source-frame reads, before varispeed.
    ///
    /// The sample and optional stem set must have the validated channel/frame layout, and every
    /// channel buffer must have room for `input_frames`. No interpolation or timeline advancement
    /// is performed here; those keep their existing processor/mixer ownership.
    #[cfg(test)]
    pub(crate) fn fill_buffers(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        input: &mut [Vec<f32>],
        input_frames: usize,
    ) {
        let channels = self.channels;
        debug_assert!(channels > 0 && input.len() >= channels);
        debug_assert_eq!(sample.frame_count(), self.sample_frames);
        for (channel, buffer) in input.iter_mut().enumerate().take(channels) {
            debug_assert!(buffer.len() >= input_frames);
            for (offset, sample_ref) in buffer.iter_mut().enumerate().take(input_frames) {
                *sample_ref = self.sample_at_offset(sample, stems, offset, channel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{STEM_MASK_MELODY, STEM_MASK_VOCALS};
    use std::sync::Arc;

    fn stereo_sample(gain: f32) -> SampleBuffer {
        SampleBuffer {
            residency: None,
            channels: 2,
            samples: Arc::from(
                (0..6)
                    .flat_map(|frame| [frame as f32 * gain, (frame + 10) as f32 * gain])
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn prepared_stems(reference: &SampleBuffer) -> PreparedStemSet {
        PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            accepted_timing: None,
            reference_samples: reference.samples.clone(),
            reference_residency: reference.residency.clone(),
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
            source_version_hash: 42,
            sample_rate_hz: 48_000,
            channels: 2,
            frame_count: 6,
            available_mask: full_stem_available_mask(),
            stems: [1.0, 10.0, 0.0, 0.0].map(stereo_sample),
        }
    }

    fn plan(frame_pos: usize, seek_mode: ExplicitSeekMode) -> SourceReadPlan {
        SourceReadPlan {
            channels: 2,
            sample_frames: 6,
            frame_pos,
            loop_region: FrameRange { start: 2, end: 5 },
            loop_period: crate::audio_engine::source_reader::SourceReaderPeriod::Physical,
            seek_mode,
            selection: StemRenderSelection::full_mix(),
            transition: StemTransition::default(),
        }
    }

    #[test]
    fn source_transition_output_epochs_keep_exact_state_across_partitions_and_rate_changes() {
        for ratio in [0.5, 0.61, 0.73, 1.371234567890123, 2.0] {
            let mut initial = StemTransition::start(StemRenderSelection::full_mix(), 128);
            initial.advance_fractional(0.25);
            let mut whole = initial;
            whole.advance_output_frames(17, ratio);
            for parts in [&[3, 14][..], &[1, 4, 2, 7, 3][..]] {
                let mut divided = initial;
                for frames in parts {
                    divided.advance_output_frames(*frames, ratio);
                }
                assert!(
                    whole.matches_exact(divided),
                    "ratio={ratio} parts={parts:?}"
                );
                assert_eq!(
                    whole.elapsed_frames().to_bits(),
                    divided.elapsed_frames().to_bits()
                );
                assert_eq!(
                    initial.gains_at_output_offset(29, ratio),
                    divided.gains_at_output_offset(12, ratio)
                );
            }
        }

        let mut whole = StemTransition::start(StemRenderSelection::full_mix(), 128);
        let mut divided = whole;
        for (frames, ratio) in [(17, 0.73), (11, 1.371234567890123), (13, 0.61), (19, 0.61)] {
            whole.advance_output_frames(frames, ratio);
            divided.advance_output_frames(3, ratio);
            divided.advance_output_frames(frames - 3, ratio);
            assert!(whole.matches_exact(divided));
        }
        let mut equal_elapsed = StemTransition::start(StemRenderSelection::full_mix(), 128);
        equal_elapsed.advance_fractional(whole.elapsed_frames());
        assert_eq!(
            whole.elapsed_frames().to_bits(),
            equal_elapsed.elapsed_frames().to_bits()
        );
        assert!(
            !whole.matches_exact(equal_elapsed),
            "future rounding retains the epoch identity"
        );
    }

    #[test]
    fn source_transition_zero_calls_preserve_epochs_and_completion_clears_exactly() {
        let mut transition = StemTransition::start(StemRenderSelection::full_mix(), 128);
        let initial = transition;
        transition.advance_output_frames(0, 0.73);
        assert!(transition.matches_exact(initial));
        transition.advance_output_frames(17, 0.73);
        let captured = transition;
        transition.advance_output_frames(0, 1.371234567890123);
        transition.advance_fractional(0.0);
        assert!(transition.matches_exact(captured));

        let mut whole = initial;
        whole.advance_output_frames(256, 0.5);
        transition = initial;
        transition.advance_output_frames(255, 0.5);
        assert!(transition.is_active());
        transition.advance_output_frames(1, 0.5);
        assert!(transition.matches_exact(whole));
        assert!(transition.matches_exact(StemTransition::default()));
        transition.advance_output_frames(17, 2.0);
        assert!(transition.matches_exact(whole));

        let mut oversized = captured;
        oversized.advance_output_frames(usize::MAX, 0.5);
        assert!(oversized.matches_exact(StemTransition::default()));
    }

    #[test]
    fn source_transition_gain_rounding_uses_one_canonical_output_coordinate() {
        // At output frame five, the old 3*r + 2*r grouping crosses an f32 rounding tie.
        let ratio = f64::from_bits(0x3fe1_9999_a666_6667);
        assert_ne!((5.0 * ratio) as f32, (3.0 * ratio + 2.0 * ratio) as f32);
        let whole = StemTransition::start(StemRenderSelection::full_mix(), 128);
        let mut divided = whole;
        divided.advance_output_frames(3, ratio);
        assert_eq!(
            whole.gains_at_output_offset(5, ratio),
            divided.gains_at_output_offset(2, ratio)
        );
        assert_eq!(
            divided.gains_at_output_offset(2, ratio).1.to_bits(),
            ((5.0 * ratio) as f32 / 128.0).to_bits()
        );
    }

    #[test]
    fn copied_read_plan_requires_exact_virtual_period_identity() {
        let original = SourceReadPlan {
            loop_period: crate::audio_engine::source_reader::SourceReaderPeriod::from_scalar_period(
                Some(3.25),
            ),
            ..plan(3, ExplicitSeekMode::Normal)
        };
        for period in [
            None,
            Some(3.0),
            Some(f64::from_bits(3.25_f64.to_bits() + 1)),
        ] {
            let changed = SourceReadPlan {
                loop_period: SourceReaderPeriod::from_scalar_period(period),
                ..original
            };
            assert!(!original.matches_exact(changed));
            assert!(!original.matches_source_contract(changed));
        }
        assert_eq!(original.domain().musical_period.scalar_period(), Some(3.25));
        assert!(original.matches_exact(original));
    }

    fn rational_period(numerator: u128, denominator: u128) -> SourceReaderPeriod {
        use crate::audio_engine::general_source_numerics::{Integer, MappedReaderPeriod};
        SourceReaderPeriod::MappedRational(
            MappedReaderPeriod::new(
                Integer::from_u128(numerator).unwrap(),
                Integer::from_u128(denominator).unwrap(),
            )
            .unwrap(),
        )
    }

    #[test]
    fn rational_reader_uses_its_exact_seam_for_full_mix_and_each_stem_selection() {
        let sample = stereo_sample(1.0);
        let stems = prepared_stems(&sample);
        let rational = SourceReadPlan {
            frame_pos: 5,
            loop_period: rational_period(31, 10),
            ..plan(2, ExplicitSeekMode::Normal)
        };
        let scalar = SourceReadPlan {
            loop_period: SourceReaderPeriod::from_scalar_period(Some(3.25)),
            ..rational
        };
        // phase=3+1/16; last=2; alpha=(17/16)/(11/10)=85/88.
        let expected_gain = (85.0_f64 / 88.0) as f32;
        let taps = rational.try_fractional_taps(0.0625).unwrap();
        assert_eq!(taps.left, 4);
        assert_eq!(taps.right, Some(2));
        assert_eq!(taps.right_gain.to_bits(), expected_gain.to_bits());
        assert_ne!(taps, scalar.try_fractional_taps(0.0625).unwrap());
        assert!(!rational.matches_source_contract(scalar));
        assert!(!rational.matches_exact(scalar));
        for mask in [0, STEM_MASK_MELODY, full_stem_available_mask()] {
            for selection in [
                StemRenderSelection::full_mix(),
                StemRenderSelection::for_stems(&stems, mask),
            ] {
                let selected = SourceReadPlan {
                    selection,
                    ..rational
                };
                for channel in 0..2 {
                    let readers = StemReadSets::new(Some(&stems), None);
                    let left = selected.sample_at_frame_with_gains(
                        &sample,
                        readers,
                        4,
                        (0.0, 1.0),
                        channel,
                    );
                    let right = selected.sample_at_frame_with_gains(
                        &sample,
                        readers,
                        2,
                        (0.0, 1.0),
                        channel,
                    );
                    let expected = left + (right - left) * expected_gain;
                    assert_eq!(
                        selected
                            .try_sample_fractional_with_readers(
                                &sample, readers, 0.0625, 0.0, channel,
                            )
                            .unwrap()
                            .to_bits(),
                        expected.to_bits(),
                    );
                }
            }
        }
    }

    #[test]
    fn rational_reader_refuses_bad_pairs_and_preserves_physical_equal_period_taps() {
        let physical = plan(4, ExplicitSeekMode::Normal);
        let rational = SourceReadPlan {
            loop_period: rational_period(3, 1),
            ..physical
        };
        for fraction in [0.0, -0.0, 0.25, 0.75] {
            assert_eq!(
                physical.try_fractional_taps(fraction),
                rational.try_fractional_taps(fraction),
            );
        }
        assert!(rational.try_fractional_taps(0.0).unwrap().right.is_none());
        assert!(!rational.matches_exact(physical));
        assert!(
            rational
                .domain()
                .project_normal_position(FractionalSourcePosition {
                    frame: 4,
                    fraction: 0.0,
                    seek_mode: ExplicitSeekMode::Normal,
                })
                .is_none()
        );
        let shorter = SourceReadPlan {
            frame_pos: 5,
            loop_period: rational_period(31, 10),
            ..physical
        };
        assert!(shorter.try_fractional_taps(0.125).is_none());
        assert!(shorter.try_fractional_taps(f64::NAN).is_none());
        assert!(
            SourceReadPlan {
                seek_mode: ExplicitSeekMode::BeforeLoop,
                ..shorter
            }
            .try_fractional_taps(0.0)
            .is_none()
        );
    }

    #[test]
    fn rational_full_and_checked_readers_reject_a_different_copied_domain_before_output() {
        let sample = stereo_sample(1.0);
        let read = SourceReadPlan {
            loop_period: rational_period(31, 10),
            ..plan(3, ExplicitSeekMode::Normal)
        };
        let mut scalar = SourcePlayback::new(3, ExplicitSeekMode::Normal, 1.0);
        assert!(scalar.configure(6, read.loop_region));
        for checked in [false, true] {
            let mut buffers = [vec![17.0; 4], vec![19.0; 4]];
            let original = buffers.clone();
            let result = if checked {
                read.fill_fractional_buffers_checked_with_readers(
                    &sample,
                    StemReadSets::default(),
                    &scalar,
                    &mut buffers,
                    4,
                )
            } else {
                read.fill_fractional_buffers_with_readers(
                    &sample,
                    StemReadSets::default(),
                    &scalar,
                    &mut buffers,
                    4,
                )
            };
            assert!(result.is_err());
            assert_eq!(buffers, original);
        }
    }

    #[test]
    fn virtual_seam_uses_only_admitted_physical_knots_for_full_stems_and_transition() {
        let sample = stereo_sample(1.0);
        let stems = prepared_stems(&sample);
        for (period, frame, fraction, seam_alpha, last) in [
            (3.25, 4, 0.9, 0.9 / 1.25, 4),
            (3.25, 5, 0.2, 1.2 / 1.25, 4),
            (2.75, 4, 0.5, 0.5 / 0.75, 4),
            (2.0, 3, 0.5, 0.5, 3),
        ] {
            let selection =
                StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_MELODY);
            for transition in [
                StemTransition::default(),
                StemTransition::start(selection, 128),
            ] {
                let read_plan = SourceReadPlan {
                    frame_pos: frame,
                    loop_period:
                        crate::audio_engine::source_reader::SourceReaderPeriod::from_scalar_period(
                            Some(period),
                        ),
                    transition,
                    ..plan(2, ExplicitSeekMode::Normal)
                };
                let progress = 31.25;
                for channel in 0..2 {
                    let left = (last + channel * 10) as f32;
                    let right = (2 + channel * 10) as f32;
                    let dry = left + (right - left) * seam_alpha as f32;
                    let expected = if transition.is_active() {
                        let (from, to) = transition.gains_at(progress);
                        dry * 10.0 * from + dry * to
                    } else {
                        dry
                    };
                    let actual = read_plan.sample_fractional(
                        &sample,
                        Some(&stems),
                        fraction,
                        progress,
                        channel,
                    );
                    assert!((actual - expected).abs() < 1.0e-5);
                }
            }
        }
    }

    #[test]
    fn one_frame_physical_loop_and_subframe_virtual_period_read_only_its_single_knot() {
        let sample = stereo_sample(1.0);
        for period in [0.25, 1.0, 1.75] {
            let read_plan = SourceReadPlan {
                loop_region: FrameRange { start: 2, end: 3 },
                loop_period:
                    crate::audio_engine::source_reader::SourceReaderPeriod::from_scalar_period(
                        Some(period),
                    ),
                ..plan(2, ExplicitSeekMode::Normal)
            };
            for phase in [0.0, period * 0.25, period * 0.999] {
                let positioned = SourceReadPlan {
                    frame_pos: 2 + phase.floor() as usize,
                    ..read_plan
                };
                assert_eq!(
                    positioned.sample_fractional(&sample, None, phase.fract(), 0.0, 0),
                    2.0
                );
                assert_eq!(
                    positioned.sample_fractional(&sample, None, phase.fract(), 0.0, 1),
                    12.0
                );
            }
        }
    }

    #[test]
    fn prepared_plan_rejects_equal_phase_with_different_future_loop_or_selection_ramp() {
        let sample = stereo_sample(1.0);
        let original = plan(3, ExplicitSeekMode::Normal);
        assert!(original.matches_exact(original));
        let different_loop = SourceReadPlan {
            loop_region: FrameRange { start: 2, end: 4 },
            ..original
        };
        assert_eq!(original.frame_pos, different_loop.frame_pos);
        assert!(!original.matches_exact(different_loop));
        assert!(!original.matches_source_contract(different_loop));
        assert_ne!(
            original.sample_fractional(&sample, None, 0.5, 0.0, 0),
            different_loop.sample_fractional(&sample, None, 0.5, 0.0, 0)
        );

        let stems = prepared_stems(&sample);
        let mut transition = StemTransition::start(StemRenderSelection::full_mix(), 128);
        let transitioning = SourceReadPlan {
            selection: StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_MELODY),
            transition,
            ..original
        };
        transition.advance_fractional(0.73);
        let advanced_transition = SourceReadPlan {
            transition,
            ..transitioning
        };
        assert!(!transitioning.matches_exact(advanced_transition));
        assert!(transitioning.matches_source_contract(advanced_transition));
        assert_ne!(
            transitioning.sample_fractional(&sample, Some(&stems), 0.0, 0.0, 0),
            advanced_transition.sample_fractional(&sample, Some(&stems), 0.0, 0.0, 0)
        );
        let different_selection = SourceReadPlan {
            selection: StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_VOCALS),
            ..transitioning
        };
        assert!(!transitioning.matches_exact(different_selection));
        assert!(!transitioning.matches_source_contract(different_selection));
        let advanced_phase = SourceReadPlan {
            frame_pos: 4,
            seek_mode: ExplicitSeekMode::AfterLoop,
            ..original
        };
        assert!(original.matches_source_contract(advanced_phase));
        assert!(!original.matches_exact(advanced_phase));
    }

    #[test]
    fn loop_resolution_and_pre_render_normalization_keep_live_edit_policy() {
        assert_eq!(effective_loop_region(0, None, 0), None);
        assert_eq!(
            effective_loop_region(2, Some(100), 6),
            Some(FrameRange { start: 2, end: 6 })
        );
        for (start, end) in [(6, Some(4)), (2, Some(2)), (100, None)] {
            assert_eq!(
                effective_loop_region(start, end, 6),
                Some(FrameRange { start: 0, end: 6 })
            );
        }
        let region = FrameRange { start: 2, end: 5 };
        assert_eq!(
            playhead_before_render(3, region, ExplicitSeekMode::Normal),
            3
        );
        assert_eq!(
            playhead_before_render(5, region, ExplicitSeekMode::Normal),
            2
        );
        assert_eq!(
            playhead_before_render(1, region, ExplicitSeekMode::BeforeLoop),
            1
        );
        assert_eq!(
            playhead_before_render(5, region, ExplicitSeekMode::AfterLoop),
            5
        );
    }

    #[test]
    fn explicit_seek_reads_track_intro_or_tail_once_then_wraps_the_loop() {
        let sample = stereo_sample(1.0);
        let cases = [
            (
                1,
                ExplicitSeekMode::BeforeLoop,
                [1.0, 2.0, 3.0, 4.0, 2.0, 3.0],
            ),
            (3, ExplicitSeekMode::Normal, [3.0, 4.0, 2.0, 3.0, 4.0, 2.0]),
            (
                5,
                ExplicitSeekMode::AfterLoop,
                [5.0, 2.0, 3.0, 4.0, 2.0, 3.0],
            ),
        ];
        for (frame, mode, expected) in cases {
            let plan = plan(frame, mode);
            assert_eq!(
                explicit_seek_mode_for_frame(frame, plan.loop_region, 6),
                mode
            );
            let mut output = [vec![0.0; 6], vec![0.0; 6]];
            plan.fill_buffers(&sample, None, &mut output, 6);
            assert_eq!(output[0], expected);
            assert_eq!(
                output[1],
                expected.map(|value| value + 10.0),
                "source channels must retain their own stride"
            );
        }
    }

    #[test]
    fn playback_advance_matches_reads_across_split_intro_tail_and_loop_segments() {
        let region = FrameRange { start: 2, end: 5 };
        for (start, mode) in [
            (1, ExplicitSeekMode::BeforeLoop),
            (3, ExplicitSeekMode::Normal),
            (5, ExplicitSeekMode::AfterLoop),
        ] {
            for split in 0..16 {
                let (next, next_mode) = advance_playback_position(start, split, 6, region, mode);
                for offset in 0..16 {
                    assert_eq!(
                        source_frame_for_playback(start, split + offset, 6, region, mode),
                        source_frame_for_playback(next, offset, 6, region, next_mode),
                        "start={start} split={split} offset={offset} mode={mode:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn prepared_stem_selection_keeps_stereo_wrap_and_excludes_instrumental() {
        let sample = stereo_sample(100.0);
        let stems = prepared_stems(&sample);
        let validated = prepared_stem_set_for_render(Some(&stems), &sample, 2, 48_000.0, 6, None);
        let mut plan = plan(4, ExplicitSeekMode::Normal);
        plan.selection = StemRenderSelection::from_state(StemMixMode::AllStems, 42, u8::MAX);
        let mut output = [vec![0.0; 4], vec![0.0; 4]];
        plan.fill_buffers(&sample, validated, &mut output, 4);
        assert_eq!(output[0], [44.0, 22.0, 33.0, 44.0]);
        assert_eq!(output[1], [154.0, 132.0, 143.0, 154.0]);
    }

    #[test]
    fn missing_or_stale_stems_fall_back_but_an_empty_matching_mask_is_silent() {
        let sample = stereo_sample(100.0);
        let stems = prepared_stems(&sample);
        let mut plan = plan(4, ExplicitSeekMode::Normal);
        plan.selection =
            StemRenderSelection::from_state(StemMixMode::AllStems, 7, STEM_MASK_VOCALS);
        assert_eq!(plan.sample_at_offset(&sample, Some(&stems), 0, 1), 1400.0);
        plan.selection =
            StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_VOCALS);
        assert_eq!(plan.sample_at_offset(&sample, None, 0, 1), 1400.0);
        assert_eq!(plan.sample_at_offset(&sample, Some(&stems), 0, 1), 14.0);
        plan.selection = StemRenderSelection::from_state(StemMixMode::AllStems, 42, 0);
        assert_eq!(plan.sample_at_offset(&sample, Some(&stems), 0, 1), 0.0);
    }

    #[test]
    fn transition_across_segments_reads_both_sides_at_the_same_wrapped_frame() {
        let sample = stereo_sample(100.0);
        let stems = prepared_stems(&sample);
        let mut plan = plan(4, ExplicitSeekMode::Normal);
        plan.selection =
            StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_MELODY);
        plan.transition = StemTransition::start(
            StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_VOCALS),
            4,
        );
        let mut whole = [vec![0.0; 6], vec![0.0; 6]];
        plan.fill_buffers(&sample, Some(&stems), &mut whole, 6);
        let mut first = [vec![0.0; 2], vec![0.0; 2]];
        plan.fill_buffers(&sample, Some(&stems), &mut first, 2);
        plan.frame_pos = source_frame_for_playback(4, 2, 6, plan.loop_region, plan.seek_mode);
        plan.transition.advance(2);
        let mut second = [vec![0.0; 4], vec![0.0; 4]];
        plan.fill_buffers(&sample, Some(&stems), &mut second, 4);
        for channel in 0..2 {
            first[channel].extend_from_slice(&second[channel]);
            assert_eq!(whole[channel], first[channel]);
        }
        assert_eq!(whole[0], [4.0, 6.5, 16.5, 31.0, 20.0, 30.0]);
        assert_eq!(whole[1], [14.0, 39.0, 71.5, 108.5, 120.0, 130.0]);
        plan.transition.advance(4);
        assert!(!plan.transition.is_active());
    }

    #[test]
    fn fractional_feed_uses_clock_ratio_and_borrows_source_transition_progress() {
        let sample = stereo_sample(100.0);
        let stems = prepared_stems(&sample);
        let mut read_plan = plan(1, ExplicitSeekMode::BeforeLoop);
        read_plan.selection =
            StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_VOCALS);
        read_plan.transition = StemTransition::start(StemRenderSelection::full_mix(), 4);
        read_plan.transition.advance_fractional(0.5);
        let transition = read_plan.transition;
        let mut playback = SourcePlayback::new(3, ExplicitSeekMode::Normal, 0.73);
        assert!(playback.configure(6, read_plan.loop_region));
        assert!(playback.advance(2));
        assert!(playback.set_target(1.25));
        let (frames, ratio) = playback.chunk(12);
        assert_eq!(frames, 12);
        let position = playback.position();
        let mut buffers = [vec![-99.0; 13], vec![-99.0; 13]];
        read_plan
            .fill_fractional_buffers(&sample, Some(&stems), &playback, &mut buffers, frames)
            .expect("known scalar test source read");

        // Independent address/interpolation oracle: after two 0.73 frames the base is frame 4,
        // fraction 0.46. The newly accepted target has made only its first 0.05 ratio step.
        let initial_fraction = (2.0 * 0.73_f64).fract();
        for frame in 0..frames {
            let source_progress = frame as f64 * ratio;
            let distance = initial_fraction + source_progress;
            let lower = 2 + (2 + distance.floor() as usize) % 3;
            let upper = 2 + (lower - 2 + 1) % 3;
            let fraction = distance.fract() as f32;
            let to_gain = ((0.5 + source_progress).min(4.0) / 4.0) as f32;
            for (channel, buffer) in buffers.iter().enumerate() {
                let lower_sample = (lower + channel * 10) as f32;
                let upper_sample = (upper + channel * 10) as f32;
                let interpolated = lower_sample + (upper_sample - lower_sample) * fraction;
                let expected = interpolated * (100.0 * (1.0 - to_gain) + to_gain);
                assert!((buffer[frame] - expected).abs() < 2.0e-4);
            }
        }
        assert_eq!(buffers[0][frames], -99.0);
        assert_eq!(buffers[1][frames], -99.0);
        assert_eq!(playback.position(), position);
        assert_eq!(read_plan.transition, transition);
    }

    #[test]
    fn binary64_source_rate_reaches_fractional_taps_for_full_mix_and_prepared_stems() {
        let sample = stereo_sample(1.0);
        let stems = prepared_stems(&sample);
        let ratio = 1.0 + 2.0_f64.powi(-24);
        let elapsed = 8_388_608;
        assert_eq!(ratio as f32, 1.0);
        let mut playback = SourcePlayback::new(2, ExplicitSeekMode::Normal, ratio);
        assert!(playback.configure(6, FrameRange { start: 2, end: 5 }));
        assert!(playback.advance(elapsed));
        assert_eq!(playback.position().fraction, 0.5);

        for (selection, gain) in [
            (StemRenderSelection::full_mix(), 1.0),
            (
                StemRenderSelection::from_state(StemMixMode::AllStems, 42, u8::MAX),
                11.0,
            ),
        ] {
            let read_plan = SourceReadPlan {
                selection,
                ..plan(2, ExplicitSeekMode::Normal)
            };
            let mut buffers = [vec![-99.0; 4], vec![-99.0; 4]];
            read_plan
                .fill_fractional_buffers(&sample, Some(&stems), &playback, &mut buffers, 3)
                .expect("known scalar test source read");
            // Source integral and both wrapped taps are algebraic, independent of reader helpers.
            for frame in 0..3 {
                let distance = (elapsed + frame) as f64 * ratio;
                let left = 2 + distance.floor() as usize % 3;
                let right = 2 + (distance.floor() as usize + 1) % 3;
                for (channel, buffer) in buffers.iter().enumerate() {
                    let lower = (left + channel * 10) as f32 * gain;
                    let upper = (right + channel * 10) as f32 * gain;
                    let expected = lower + (upper - lower) * distance.fract() as f32;
                    assert_eq!(buffer[frame], expected);
                }
            }
            assert_eq!(buffers[0][0], 3.0 * gain);
            assert_ne!(
                buffers[0][0],
                4.0 * gain,
                "a binary32 rate loses this phase"
            );
            assert_eq!(buffers[0][3], -99.0);
            assert_eq!(buffers[1][3], -99.0);
        }
    }

    #[test]
    fn prepared_render_requires_complete_current_accepted_projection() {
        use super::super::constant_timing::AcceptedTimingProjection;
        let sample = stereo_sample(100.0);
        let mut stems = prepared_stems(&sample);
        let projection = AcceptedTimingProjection {
            revision: [0x13; 32],
            period_seconds: 60.0 / 119.999_123_456,
            origin_seconds: -0.123_456_789,
            sample_rate_hz: 48_000,
            publication_epoch: 7,
        };
        stems.accepted_timing = Some(projection);
        let renderable = |current| {
            prepared_stem_set_for_render(Some(&stems), &sample, 2, 48_000.0, 6, current).is_some()
        };
        assert!(renderable(Some(projection)));
        assert!(!renderable(None));
        for stale in [
            AcceptedTimingProjection {
                revision: [0x14; 32],
                ..projection
            },
            AcceptedTimingProjection {
                period_seconds: f64::from_bits(projection.period_seconds.to_bits() + 1),
                ..projection
            },
            AcceptedTimingProjection {
                origin_seconds: -projection.origin_seconds,
                ..projection
            },
            AcceptedTimingProjection {
                sample_rate_hz: 44_100,
                ..projection
            },
            AcceptedTimingProjection {
                publication_epoch: 8,
                ..projection
            },
        ] {
            assert!(!renderable(Some(stale)));
        }
        let negative_zero = AcceptedTimingProjection {
            origin_seconds: -0.0,
            ..projection
        };
        stems.accepted_timing = Some(negative_zero);
        assert!(
            prepared_stem_set_for_render(
                Some(&stems),
                &sample,
                2,
                48_000.0,
                6,
                Some(negative_zero)
            )
            .is_some()
        );
        assert!(
            prepared_stem_set_for_render(
                Some(&stems),
                &sample,
                2,
                48_000.0,
                6,
                Some(AcceptedTimingProjection {
                    origin_seconds: 0.0,
                    ..negative_zero
                })
            )
            .is_none()
        );
        stems.accepted_timing = None;
        assert!(
            prepared_stem_set_for_render(Some(&stems), &sample, 2, 48_000.0, 6, None).is_some()
        );
        assert!(
            prepared_stem_set_for_render(Some(&stems), &sample, 2, 48_000.0, 6, Some(projection))
                .is_none()
        );
    }

    #[test]
    fn prepared_source_validation_rejects_incomplete_or_mismatched_buffers() {
        let sample = stereo_sample(100.0);
        let stems = prepared_stems(&sample);
        assert!(prepared_stem_set_matches_sample(
            &stems, &sample, 2, 48_000.0, 6
        ));
        let same_values_new_source = stereo_sample(100.0);
        assert!(!prepared_stem_set_matches_sample(
            &stems,
            &same_values_new_source,
            2,
            48_000.0,
            6
        ));
        for (channels, rate, frames) in [(1, 48_000.0, 6), (2, 44_100.0, 6), (2, 48_000.0, 5)] {
            assert!(!prepared_stem_set_matches_sample(
                &stems, &sample, channels, rate, frames
            ));
        }
        for missing_component in 0..STEM_BUFFER_COUNT {
            let mut incomplete = stems.clone();
            incomplete.available_mask = STEM_COMPONENT_MASK & !(1 << missing_component);
            assert!(
                prepared_stem_set_for_render(Some(&incomplete), &sample, 2, 48_000.0, 6, None)
                    .is_none(),
                "missing live component {missing_component} must reject render eligibility"
            );
        }
        let mut stale = stems.clone();
        stale.source_version_hash = 0;
        assert!(
            prepared_stem_set_for_render(Some(&stale), &sample, 2, 48_000.0, 6, None).is_none()
        );
        let mut truncated = stems.clone();
        truncated.stems[0].samples = Arc::from(vec![0.0; 10]);
        assert!(
            prepared_stem_set_for_render(Some(&truncated), &sample, 2, 48_000.0, 6, None).is_none()
        );
    }
}
