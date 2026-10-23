//! Copied source clock shared by live resampling and non-realtime preparation.
//!
//! A constant-rate epoch is addressed by its active output-frame count, never by callback
//! endpoints. Rebases preserve the fractional source position. All operations are bounded and
//! allocation-free; pausing simply stops advancing this clock.

use super::constants::{SPEED_MAX, SPEED_MIN};
use super::mapped_source_clock::{
    MappedClockEpoch, MappedClockError, MappedLoopGeometry, MappedSourceSpan,
};
#[cfg(test)]
use super::source_reader::FrameRange;
use super::source_reader::{
    ExplicitSeekMode, SourceLoopDomain, advance_playback_position, playhead_before_render,
};

const TEMPO_STEP: f64 = 0.05;
const TEMPO_STEP_OUTPUT_FRAMES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FractionalSourcePosition {
    pub(crate) frame: usize,
    pub(crate) fraction: f64,
    pub(crate) seek_mode: ExplicitSeekMode,
}

#[derive(Clone, Copy, Debug)]
struct MappedCopyState {
    span: MappedSourceSpan,
    intended_output_frame: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScalarProjection {
    Legacy,
    PairPhysical,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SourcePlayback {
    origin: FractionalSourcePosition,
    domain: Option<SourceLoopDomain>,
    elapsed_output_frames: u64,
    ratio: f64,
    target: f64,
    frames_until_step: usize,
    // Before this epoch, only an intentional start/seek defines reconstructible source history.
    history_reconstructible: bool,
    scalar_projection: ScalarProjection,
    mapped: Option<MappedCopyState>,
}

impl SourcePlayback {
    /// The caller separately retains and qualifies the original immutable map
    /// owner. This value contains only a copied trajectory and identity markers.
    pub(crate) fn mapped(
        geometry: &MappedLoopGeometry,
        epoch: MappedClockEpoch,
        intended_output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        let span = geometry.refresh_span(epoch, intended_output_frame)?;
        let origin = span
            .position_at(intended_output_frame)
            .ok_or(MappedClockError::NumericalUnavailable)?;
        let ratio = span.local_rate().ok_or(MappedClockError::UnsupportedRate)?;
        Ok(Self {
            origin,
            domain: Some(geometry.domain()),
            elapsed_output_frames: 0,
            ratio,
            target: ratio,
            frames_until_step: 0,
            history_reconstructible: false,
            scalar_projection: ScalarProjection::Legacy,
            mapped: Some(MappedCopyState {
                span,
                intended_output_frame,
            }),
        })
    }

    pub(crate) fn is_mapped(&self) -> bool {
        self.mapped.is_some()
    }

    pub(crate) fn mapped_epoch(&self) -> Option<MappedClockEpoch> {
        self.mapped.map(|state| state.span.epoch())
    }

    pub(crate) fn mapped_output_frame(&self) -> Option<u64> {
        self.mapped.map(|state| state.intended_output_frame)
    }

    /// Include the canonical exclusive endpoint, but never invent a later span.
    pub(crate) fn can_read_frames(&self, output_frames: usize) -> bool {
        if self.scalar_projection == ScalarProjection::PairPhysical {
            return self
                .elapsed_output_frames
                .checked_add(output_frames as u64)
                .is_some();
        }
        self.mapped.is_none_or(|state| {
            state
                .intended_output_frame
                .checked_add(output_frames as u64)
                .is_some_and(|end| end <= state.span.end_frame())
        })
    }

    /// Refresh derived geometry at this exact cursor. The caller must supply
    /// the original qualified owning map; equal geometry is not that permission.
    pub(crate) fn refresh_mapped_span(
        &mut self,
        geometry: &MappedLoopGeometry,
    ) -> Result<(), MappedClockError> {
        let state = self.mapped.ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let replacement = Self::mapped(geometry, state.span.epoch(), state.intended_output_frame)?;
        if !same_position(self.origin, replacement.origin) {
            return Err(MappedClockError::InvalidGeometry);
        }
        *self = replacement;
        Ok(())
    }

    /// Direct canonical F-to-E projection with the retained owner. This operation
    /// supplies coordinates only and does not certify actual historical reads.
    pub(crate) fn project_mapped_to(
        &mut self,
        geometry: &MappedLoopGeometry,
        intended_output_frame: u64,
    ) -> Result<(), MappedClockError> {
        let state = self.mapped.ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let replacement = Self::mapped(geometry, state.span.epoch(), intended_output_frame)?;
        *self = replacement;
        Ok(())
    }

    /// A caller's atomic master-epoch claim may replace the frozen projection
    /// only at the same intended frame and exact canonical source position.
    pub(crate) fn replace_mapped_epoch(
        &mut self,
        geometry: &MappedLoopGeometry,
        epoch: MappedClockEpoch,
    ) -> Result<(), MappedClockError> {
        let state = self.mapped.ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let replacement = Self::mapped(geometry, epoch, state.intended_output_frame)?;
        if !same_position(self.origin, replacement.origin) {
            return Err(MappedClockError::InvalidEpoch);
        }
        *self = replacement;
        Ok(())
    }

    /// Resume only a copied mapped trajectory, borrowing the original geometry
    /// and actual callback master. Source/content/reader ownership is a separate
    /// mixer preflight. This projection creates no actual read or Native permit.
    pub(crate) fn resume_mapped_at(
        &mut self,
        geometry: &MappedLoopGeometry,
        actual: &super::transport::TransportTimeline,
        output_frame: u64,
    ) -> Result<(), MappedClockError> {
        let state = self.mapped.ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let epoch = state
            .span
            .epoch()
            .resumed_at(actual, state.intended_output_frame, output_frame)
            .ok_or(MappedClockError::InvalidEpoch)?;
        let replacement = Self::mapped(geometry, epoch, output_frame)?;
        if !same_position(self.origin, replacement.origin) {
            return Err(MappedClockError::InvalidEpoch);
        }
        *self = replacement;
        Ok(())
    }

    /// Isolated map diagnostics start an exact affine epoch, without the live scalar glide.
    /// This constructs no source, timing, preparation or continuation permission.
    #[cfg(test)]
    pub(crate) fn diagnostic_affine_epoch(
        coordinate: f64,
        ratio: f64,
        domain: SourceLoopDomain,
    ) -> Option<Self> {
        if !coordinate.is_finite()
            || coordinate < domain.region.start as f64
            || coordinate >= domain.region.end as f64
            || !ratio.is_finite()
            || !(SPEED_MIN..=SPEED_MAX).contains(&ratio)
        {
            return None;
        }
        let mut playback = Self::new(coordinate.floor() as usize, ExplicitSeekMode::Normal, ratio);
        playback.origin.fraction = coordinate - coordinate.floor();
        playback.domain = Some(domain);
        Some(playback)
    }

    // At most one .05 source-rate step per segment, plus the terminal constant segment.
    // The extra two segments cover binary64 accumulation at an inclusive endpoint.
    pub(crate) const MAX_PROJECTION_SEGMENTS: usize =
        ((SPEED_MAX - SPEED_MIN) / TEMPO_STEP) as usize + 3;

    pub(crate) fn new(frame: usize, seek_mode: ExplicitSeekMode, ratio: f64) -> Self {
        let ratio = checked_ratio(ratio);
        Self {
            origin: FractionalSourcePosition {
                frame,
                fraction: 0.0,
                seek_mode,
            },
            domain: None,
            elapsed_output_frames: 0,
            ratio,
            target: ratio,
            frames_until_step: 0,
            history_reconstructible: true,
            scalar_projection: ScalarProjection::Legacy,
            mapped: None,
        }
    }

    /// Coordinates-only prerequisite for an exact physical source pair. No live adoption
    /// route calls this yet, and construction grants no source, reader or history permission.
    pub(crate) fn scalar_continuation(
        position: FractionalSourcePosition,
        domain: SourceLoopDomain,
        initial_ratio: f64,
        target: f64,
    ) -> Option<Self> {
        if !valid_physical_pair(position, domain)
            || !initial_ratio.is_finite()
            || !(SPEED_MIN..=SPEED_MAX).contains(&initial_ratio)
            || !target.is_finite()
            || !(SPEED_MIN..=SPEED_MAX).contains(&target)
        {
            return None;
        }
        Some(Self {
            origin: position,
            domain: Some(domain),
            elapsed_output_frames: 0,
            ratio: initial_ratio,
            target,
            frames_until_step: 0,
            history_reconstructible: false,
            scalar_projection: ScalarProjection::PairPhysical,
            mapped: None,
        })
    }

    /// Copy the current source phase into a constant-rate preparation epoch.
    ///
    /// Pending live smoothing is removed only from this value. Its logical counterpart retains
    /// the accepted target and active-frame step interval.
    #[cfg(test)]
    pub(crate) fn at_constant_ratio(mut self, ratio: f64) -> Self {
        if self.is_mapped() {
            return self;
        }
        self.rebase();
        self.ratio = checked_ratio(ratio);
        self.target = self.ratio;
        self.frames_until_step = 0;
        self.history_reconstructible = self.scalar_projection == ScalarProjection::Legacy;
        self
    }

    pub(crate) fn tempo_ratio(&self) -> f64 {
        self.ratio
    }

    /// Observe the pending target separately from the applied rate without advancing it.
    pub(crate) fn rate_target(&self) -> f64 {
        self.target
    }

    /// Copy a constant source trajectory's past for off-thread endpoint priming.
    /// The live clock is untouched. A pending smoothing trajectory cannot be invented from
    /// its endpoint and is explicitly unavailable; ordinary forward catch-up retains it.
    /// The second result counts silence before physical source zero for Intro/Tail history.
    pub(crate) fn history_before_constant(&self, output_frames: usize) -> Option<(Self, usize)> {
        if self.is_mapped() {
            return None;
        }
        if self.ratio.to_bits() != self.target.to_bits() {
            return None;
        }
        if self.elapsed_output_frames >= output_frames as u64 {
            let mut history = *self;
            history.elapsed_output_frames -= output_frames as u64;
            return Some((history, 0));
        }
        if self.scalar_projection == ScalarProjection::PairPhysical {
            return None;
        }
        if !self.history_reconstructible {
            return None;
        }
        let domain = self.domain?;
        // Rewind the original epoch, not its wrapped endpoint. An Intro/Tail may already
        // have reached Normal at E while its actual prefix still belongs to this history.
        let position = self.origin;
        let missing_frames = output_frames - self.elapsed_output_frames as usize;
        let distance = missing_frames as f64 * self.ratio;
        let (coordinate, silent_prefix) = if position.seek_mode == ExplicitSeekMode::Normal {
            let period = domain.musical_period.unwrap_or(domain.region.len() as f64);
            let phase = (position.frame.saturating_sub(domain.region.start) as f64
                + position.fraction
                - distance)
                .rem_euclid(period);
            (domain.region.start as f64 + phase, 0)
        } else {
            let endpoint = position.frame as f64 + position.fraction;
            let silent = ((distance - endpoint).max(0.0) / self.ratio).ceil() as usize;
            let silent = silent.min(missing_frames);
            (
                endpoint - (missing_frames - silent) as f64 * self.ratio,
                silent,
            )
        };
        if !coordinate.is_finite() || coordinate < 0.0 {
            return None;
        }
        let whole = coordinate.floor();
        let mut history = *self;
        history.origin = FractionalSourcePosition {
            frame: whole as usize,
            fraction: coordinate - whole,
            seek_mode: position.seek_mode,
        };
        history.elapsed_output_frames = 0;
        history.frames_until_step = 0;
        Some((history, silent_prefix))
    }

    /// Worker-only phase translation for a declared new Normal-start history.
    /// Keep the original rate epoch and its elapsed count, so slicing one epoch
    /// never introduces a new rounding anchor. This is derived input, not a claim
    /// that the live voice actually read these shifted source positions.
    pub(crate) fn shifted_normal_history(mut self, shift: f64) -> Option<Self> {
        if self.is_mapped() || self.scalar_projection == ScalarProjection::PairPhysical {
            return None;
        }
        let domain = self.domain?;
        if self.origin.seek_mode != ExplicitSeekMode::Normal
            || self.origin.frame < domain.region.start
            || !shift.is_finite()
            || !self.origin.fraction.is_finite()
            || !(0.0..1.0).contains(&self.origin.fraction)
            || domain.region.start >= domain.region.end
            || domain.region.end > domain.sample_frames
        {
            return None;
        }
        let period = domain.musical_period.unwrap_or(domain.region.len() as f64);
        if !period.is_finite() || period <= 0.0 || (period - domain.region.len() as f64).abs() > 1.0
        {
            return None;
        }
        let phase =
            ((self.origin.frame - domain.region.start) as f64 + self.origin.fraction + shift)
                .rem_euclid(period);
        let whole = phase.floor();
        self.origin.frame = domain.region.start.checked_add(whole as usize)?;
        self.origin.fraction = phase - whole;
        Some(self)
    }

    /// Compare the complete copied trajectory at a prepared native adoption boundary.
    ///
    /// Equal current source phase is insufficient: a pending rate target or a different active
    /// smoothing interval changes future feed. The copied worker cursor must reach the same
    /// canonical epoch and exact binary64 state, rather than merely a wrapped position.
    pub(crate) fn matches_exact(&self, other: &Self) -> bool {
        match (self.mapped, other.mapped) {
            (Some(left), Some(right)) => {
                return self.domain == other.domain
                    && left.intended_output_frame == right.intended_output_frame
                    && left.span.matches_exact(right.span);
            }
            (Some(_), None) | (None, Some(_)) => return false,
            (None, None) => {}
        }
        self.origin.frame == other.origin.frame
            && self.origin.fraction.to_bits() == other.origin.fraction.to_bits()
            && self.origin.seek_mode == other.origin.seek_mode
            && self.domain == other.domain
            && self.elapsed_output_frames == other.elapsed_output_frames
            && self.ratio.to_bits() == other.ratio.to_bits()
            && self.target.to_bits() == other.target.to_bits()
            && self.frames_until_step == other.frames_until_step
            && self.history_reconstructible == other.history_reconstructible
            && self.scalar_projection == other.scalar_projection
    }

    /// Check pending preparation control intent while its copied smoothing clock advances.
    pub(crate) fn matches_rate_target(&self, other: &Self) -> bool {
        match (self.mapped, other.mapped) {
            (Some(left), Some(right)) => {
                return self.domain == other.domain
                    && left.span.epoch().matches_exact(right.span.epoch());
            }
            (Some(_), None) | (None, Some(_)) => return false,
            (None, None) => {}
        }
        self.scalar_projection == other.scalar_projection
            && self.target.to_bits() == other.target.to_bits()
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn configure(&mut self, sample_frames: usize, region: FrameRange) -> bool {
        self.configure_domain(SourceLoopDomain::physical(sample_frames, region))
    }

    pub(crate) fn loop_period(&self) -> Option<f64> {
        self.domain.and_then(|domain| domain.musical_period)
    }

    pub(crate) fn matches_domain(&self, domain: SourceLoopDomain) -> bool {
        self.domain == Some(domain)
    }

    #[must_use]
    pub(crate) fn configure_domain(&mut self, domain: SourceLoopDomain) -> bool {
        if self.domain == Some(domain) {
            return true;
        }
        // A new domain needs its own future integration contract. An unchanged
        // physical refresh retains this exact epoch; do not clamp, wrap or rebase it.
        if self.scalar_projection == ScalarProjection::PairPhysical {
            return false;
        }
        if self.is_mapped() {
            return false;
        }
        let same_geometry = self.domain.is_some_and(|previous| {
            previous.sample_frames == domain.sample_frames && previous.region == domain.region
        });
        if self.domain.is_some() {
            self.history_reconstructible = false;
        }
        self.rebase();
        self.domain = Some(domain);
        let outside = if let Some(period) = domain.musical_period {
            let offset =
                self.origin.frame.saturating_sub(domain.region.start) as f64 + self.origin.fraction;
            self.origin.seek_mode == ExplicitSeekMode::Normal
                && (self.origin.frame < domain.region.start || offset >= period)
        } else {
            playhead_before_render(self.origin.frame, domain.region, self.origin.seek_mode)
                != self.origin.frame
        };
        // A timing-only domain change wraps the retained virtual origin naturally.
        // Actual loop/extent edits keep the established out-of-range clamp policy.
        if outside && !same_geometry {
            self.origin.frame = domain.region.start;
            self.origin.fraction = 0.0;
        }
        true
    }

    pub(crate) fn clear_explicit_seek(&mut self) {
        // PairPhysical is already Normal. An unchanged refresh must retain its epoch.
        if self.is_mapped() || self.scalar_projection == ScalarProjection::PairPhysical {
            return;
        }
        self.rebase();
        let was_explicit = self.origin.seek_mode != ExplicitSeekMode::Normal;
        self.origin.seek_mode = ExplicitSeekMode::Normal;
        // Exiting intro/tail normalizes on the next configuration. An already normal
        // cursor retains its geometry so a same-marker timing refresh cannot discard
        // the virtual seam's fractional residue.
        if was_explicit {
            self.domain = None;
            self.history_reconstructible = false;
        }
    }

    #[must_use]
    pub(crate) fn set_target(&mut self, target: f64) -> bool {
        if self.is_mapped() {
            return false;
        }
        let target = checked_ratio(target);
        if target != self.target {
            self.target = target;
            self.frames_until_step = 0;
        }
        true
    }

    /// A seek changes source phase, while the accepted rate target and smoothing clock continue.
    #[must_use]
    pub(crate) fn seek(&mut self, frame: usize, seek_mode: ExplicitSeekMode) -> bool {
        if self.is_mapped() {
            return false;
        }
        self.origin = FractionalSourcePosition {
            frame,
            fraction: 0.0,
            seek_mode,
        };
        self.domain = None;
        self.elapsed_output_frames = 0;
        self.history_reconstructible = true;
        self.scalar_projection = ScalarProjection::Legacy;
        true
    }

    /// Begin a constant-ratio chunk, bounded by the next active-output-frame smoothing step.
    pub(crate) fn chunk(&mut self, max_frames: usize) -> (usize, f64) {
        if let Some(state) = self.mapped {
            let remaining = state.span.end_frame() - state.intended_output_frame;
            return (
                max_frames.min(usize::try_from(remaining).unwrap_or(usize::MAX)),
                self.ratio,
            );
        }
        if self.frames_until_step == 0 && self.ratio != self.target {
            self.rebase();
            self.history_reconstructible = false;
            let delta = (self.target - self.ratio).clamp(-TEMPO_STEP, TEMPO_STEP);
            self.ratio = if (self.target - self.ratio).abs() <= TEMPO_STEP {
                self.target
            } else {
                (self.ratio + delta).clamp(SPEED_MIN, SPEED_MAX)
            };
            // Once the new pair mode reaches its target this timer is inactive.
            // Canonical zero lets constant in-epoch rewind/replay retain exact state.
            self.frames_until_step = if self.scalar_projection == ScalarProjection::PairPhysical
                && self.ratio == self.target
            {
                0
            } else {
                TEMPO_STEP_OUTPUT_FRAMES
            };
        }
        let frames = if self.ratio == self.target {
            max_frames
        } else {
            max_frames.min(self.frames_until_step)
        };
        (frames, self.ratio)
    }

    pub(crate) fn position(&self) -> FractionalSourcePosition {
        if self.is_mapped() {
            self.origin
        } else if self.scalar_projection == ScalarProjection::PairPhysical {
            self.pair_physical_position_at(self.elapsed_output_frames)
        } else {
            self.scalar_position_at(0)
        }
    }

    pub(crate) fn position_at(&self, output_offset: usize) -> Option<FractionalSourcePosition> {
        if let Some(state) = self.mapped {
            return state.span.position_at(
                state
                    .intended_output_frame
                    .checked_add(output_offset as u64)?,
            );
        }
        if self.scalar_projection == ScalarProjection::PairPhysical {
            let elapsed = self
                .elapsed_output_frames
                .checked_add(output_offset as u64)?;
            return Some(self.pair_physical_position_at(elapsed));
        }
        Some(self.scalar_position_at(output_offset))
    }

    fn pair_physical_position_at(&self, elapsed: u64) -> FractionalSourcePosition {
        let domain = self.domain.expect("physical pair has its admitted domain");
        let length = domain.region.len() as u128;
        let bits = self.ratio.to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as u32;
        // Admitted rates are normal [0.5, 2.0]: exponent 1022..=1024,
        // ratio = significand / 2^shift, shift 51..=53. The complete u64
        // count times a <=53-bit significand fits in 117 bits, without f64 narrowing.
        let significand = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
        let shift = 1075 - exponent;
        let product = u128::from(elapsed) * u128::from(significand);
        let whole = product >> shift;
        let remainder = product & ((1u128 << shift) - 1);
        let fraction_advance = remainder as f64 / (1u64 << shift) as f64;
        let relative = (self.origin.frame - domain.region.start) as u128;
        let mut relative = (relative + whole % length) % length;
        // Generic fractional additive identity, including signed zero, at every revisit.
        let mut fraction = if remainder == 0 {
            self.origin.fraction
        } else {
            self.origin.fraction + fraction_advance
        };
        // This sum of fractions below one is the only rounding. Rounded carry is
        // normalized in the physical circle, whose error is <=2^-53 source frame.
        if fraction >= 1.0 {
            fraction -= 1.0;
            relative = (relative + 1) % length;
        }
        FractionalSourcePosition {
            frame: domain.region.start + relative as usize,
            fraction,
            seek_mode: ExplicitSeekMode::Normal,
        }
    }

    fn scalar_position_at(&self, output_offset: usize) -> FractionalSourcePosition {
        let distance = self.origin.fraction
            + self
                .elapsed_output_frames
                .saturating_add(output_offset as u64) as f64
                * self.ratio;
        if let Some(domain) = self.domain
            && let Some(period) = domain.musical_period
            && period != domain.region.len() as f64
        {
            return self.musical_position_at(distance, domain, period);
        }
        let whole = distance.floor() as usize;
        let (frame, seek_mode) = self.domain.map_or(
            (
                self.origin.frame.saturating_add(whole),
                self.origin.seek_mode,
            ),
            |domain| {
                advance_playback_position(
                    self.origin.frame,
                    whole,
                    domain.sample_frames,
                    domain.region,
                    self.origin.seek_mode,
                )
            },
        );
        FractionalSourcePosition {
            frame,
            fraction: distance - whole as f64,
            seek_mode,
        }
    }

    /// Retain virtual phase in source-frame units, including the fractional seam.
    /// Physical interpolation is the reader's responsibility; rebases never project
    /// this value to a PCM tap and therefore cannot lose musical phase.
    fn musical_position_at(
        &self,
        distance: f64,
        domain: SourceLoopDomain,
        period: f64,
    ) -> FractionalSourcePosition {
        let (phase, mode) = match self.origin.seek_mode {
            ExplicitSeekMode::Normal => (
                (self.origin.frame.saturating_sub(domain.region.start) as f64 + distance)
                    .rem_euclid(period),
                ExplicitSeekMode::Normal,
            ),
            ExplicitSeekMode::BeforeLoop | ExplicitSeekMode::AfterLoop => {
                let boundary = if self.origin.seek_mode == ExplicitSeekMode::BeforeLoop {
                    domain.region.start
                } else {
                    domain.sample_frames
                };
                let prefix = boundary.saturating_sub(self.origin.frame) as f64;
                if distance < prefix {
                    let whole = distance.floor() as usize;
                    return FractionalSourcePosition {
                        frame: self.origin.frame.saturating_add(whole),
                        fraction: distance - whole as f64,
                        seek_mode: self.origin.seek_mode,
                    };
                }
                (
                    (distance - prefix).rem_euclid(period),
                    ExplicitSeekMode::Normal,
                )
            }
        };
        let whole = phase.floor() as usize;
        FractionalSourcePosition {
            frame: domain.region.start + whole,
            fraction: phase - whole as f64,
            seek_mode: mode,
        }
    }

    #[must_use]
    pub(crate) fn advance(&mut self, output_frames: usize) -> bool {
        self.try_advance(output_frames)
    }

    #[must_use]
    pub(crate) fn try_advance(&mut self, output_frames: usize) -> bool {
        if let Some(mut state) = self.mapped {
            let Some(next) = state
                .intended_output_frame
                .checked_add(output_frames as u64)
            else {
                return false;
            };
            let Some(position) = state.span.position_at(next) else {
                return false;
            };
            state.intended_output_frame = next;
            self.mapped = Some(state);
            self.origin = position;
            return true;
        }
        if self.scalar_projection == ScalarProjection::PairPhysical {
            let Some(elapsed) = self.elapsed_output_frames.checked_add(output_frames as u64) else {
                return false;
            };
            self.elapsed_output_frames = elapsed;
            self.frames_until_step = self.frames_until_step.saturating_sub(output_frames);
            return true;
        }
        self.elapsed_output_frames = self
            .elapsed_output_frames
            .saturating_add(output_frames as u64);
        self.frames_until_step = self.frames_until_step.saturating_sub(output_frames);
        true
    }

    fn rebase(&mut self) {
        self.origin = self.position();
        self.elapsed_output_frames = 0;
    }
}

fn valid_physical_pair(position: FractionalSourcePosition, domain: SourceLoopDomain) -> bool {
    domain.musical_period.is_none()
        && domain.region.start < domain.region.end
        && domain.region.end <= domain.sample_frames
        && position.seek_mode == ExplicitSeekMode::Normal
        && position.frame >= domain.region.start
        && position.frame < domain.region.end
        && position.fraction.is_finite()
        && (0.0..1.0).contains(&position.fraction)
}

fn same_position(left: FractionalSourcePosition, right: FractionalSourcePosition) -> bool {
    left.frame == right.frame
        && left.fraction.to_bits() == right.fraction.to_bits()
        && left.seek_mode == right.seek_mode
}

fn checked_ratio(ratio: f64) -> f64 {
    if ratio.is_finite() {
        ratio.clamp(SPEED_MIN, SPEED_MAX)
    } else {
        1.0
    }
}

#[cfg(test)]
#[path = "source_pair_continuation_tests.rs"]
mod source_pair_continuation_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_history_keeps_the_original_intro_or_tail_after_its_endpoint_wraps() {
        let region = FrameRange { start: 20, end: 40 };
        for musical in [false, true] {
            let domain = if musical {
                SourceLoopDomain::musical(60, region, 20.25).unwrap()
            } else {
                SourceLoopDomain::physical(60, region)
            };
            let period = if musical { 20.25 } else { 20.0 };
            for (mode, start, boundary) in [
                (ExplicitSeekMode::BeforeLoop, 5, 20),
                (ExplicitSeekMode::AfterLoop, 45, 60),
            ] {
                for rate in [0.5, 1.3, 2.0] {
                    let mut endpoint = SourcePlayback::new(start, mode, rate);
                    assert!(endpoint.configure_domain(domain));
                    assert!(endpoint.advance(110));
                    assert_eq!(endpoint.position().seek_mode, ExplicitSeekMode::Normal);
                    let (history, silence) = endpoint.history_before_constant(256).unwrap();
                    // Independent physical prefix and musical-loop arithmetic. In particular,
                    // E being Normal must not replace actual pre-E Intro/Tail with loop PCM.
                    for frame in 0..256 {
                        let raw = start as f64 + (frame as f64 - 146.0) * rate;
                        if raw < -1.0e-12 {
                            assert!(frame < silence);
                            continue;
                        }
                        assert!(frame >= silence);
                        let observed = history.position_at(frame - silence).unwrap();
                        let (expected, expected_mode) = if raw < boundary as f64 {
                            (raw, mode)
                        } else {
                            (
                                20.0 + (raw - boundary as f64).rem_euclid(period),
                                ExplicitSeekMode::Normal,
                            )
                        };
                        let actual = observed.frame as f64 + observed.fraction;
                        assert!(
                            (actual - expected).abs() < 1.0e-10,
                            "mode={mode:?} rate={rate} musical={musical} n={frame} actual={actual} expected={expected}"
                        );
                        assert_eq!(observed.seek_mode, expected_mode);
                    }
                }
            }
        }
    }

    #[test]
    fn pitch_history_never_invents_the_past_of_a_newly_settled_rate_or_domain() {
        let region = FrameRange { start: 10, end: 90 };
        let mut playback = SourcePlayback::new(10, ExplicitSeekMode::Normal, 1.0);
        assert!(playback.configure(100, region));
        assert!(playback.set_target(1.05));
        assert_eq!(playback.chunk(1), (1, 1.05));
        assert!(playback.advance(1));
        assert_eq!(playback.tempo_ratio(), playback.rate_target());
        assert!(playback.history_before_constant(4096).is_none());
        assert!(playback.advance(4095));
        let (history, silence) = playback.history_before_constant(4096).unwrap();
        assert_eq!(silence, 0);
        assert_eq!(history.position_at(4096).unwrap(), playback.position());
        assert!(playback.configure_domain(SourceLoopDomain::musical(100, region, 80.25).unwrap()));
        assert!(playback.history_before_constant(4096).is_none());
        assert!(playback.advance(4096));
        assert!(playback.history_before_constant(4096).is_some());
    }

    #[test]
    fn musical_domain_preserves_virtual_seam_phase_through_rebase_rate_seek_and_edits() {
        let region = FrameRange { start: 2, end: 5 };
        let domain = SourceLoopDomain::musical(6, region, 3.25).unwrap();
        let mut playback = SourcePlayback::new(2, ExplicitSeekMode::Normal, 0.5);
        assert!(playback.configure_domain(domain));
        assert!(playback.advance(6));
        assert_eq!(playback.position().frame, 5); // Virtual, exclusive physical end.
        assert_eq!(playback.position().fraction, 0.0);
        let before = playback.position();
        assert!(playback.set_target(0.55));
        assert_eq!(playback.chunk(1), (1, 0.55));
        assert_eq!(playback.position(), before);
        assert!(playback.advance(1));
        assert!((playback.position().fraction - 0.3).abs() < 1.0e-14);
        assert_eq!(playback.position().frame, 2);
        let retained = playback.position();
        assert!(playback.configure_domain(SourceLoopDomain::musical(6, region, 2.75).unwrap()));
        assert_eq!(playback.position(), retained);
        assert!(playback.seek(5, ExplicitSeekMode::AfterLoop));
        assert!(playback.configure_domain(domain));
        assert_eq!(playback.position().seek_mode, ExplicitSeekMode::AfterLoop);
        assert!(playback.advance(4));
        assert_eq!(playback.position().seek_mode, ExplicitSeekMode::Normal);
        assert!((playback.position().fraction - 0.2).abs() < 1.0e-14);
        playback.clear_explicit_seek();
        assert!(playback.configure_domain(domain));
        assert!((playback.position().fraction - 0.2).abs() < 1.0e-14);
    }

    #[test]
    fn subframe_period_change_wraps_fraction_without_changing_integer_frame() {
        let region = FrameRange { start: 2, end: 3 };
        let mut playback = SourcePlayback::new(2, ExplicitSeekMode::Normal, 0.7);
        assert!(playback.configure(4, region));
        assert!(playback.advance(1));
        assert_eq!(playback.position().fraction, 0.7);
        assert!(playback.configure_domain(SourceLoopDomain::musical(4, region, 0.25).unwrap()));
        assert_eq!(playback.position().frame, 2);
        assert!((playback.position().fraction - 0.2).abs() < 1.0e-14);
        assert!(playback.advance(13));
        assert_eq!(playback.position().frame, 2);
        assert!((playback.position().fraction - 0.05).abs() < 1.0e-14);
    }

    #[test]
    fn same_geometry_period_shrink_and_clear_preserve_wrapped_fractional_residue() {
        let region = FrameRange {
            start: 10,
            end: 1510,
        };
        let initial = SourceLoopDomain::musical(1600, region, 1500.25).unwrap();
        let shorter = SourceLoopDomain::musical(1600, region, 1499.75).unwrap();
        // The power-of-two divisor reaches this exact binary64 seam at a legal rate.
        let mut playback = SourcePlayback::new(10, ExplicitSeekMode::Normal, 1500.125 / 2048.0);
        assert!(playback.configure_domain(initial));
        assert!(playback.advance(2048));
        let before = playback.position();
        assert_eq!(before.frame, 1510);
        assert_eq!(before.fraction, 0.125);
        let mut cleared = playback;
        playback.clear_explicit_seek(); // Same physical marker reapplication.
        assert!(playback.configure_domain(shorter));
        assert_eq!(playback.position().frame, 10);
        assert_eq!(playback.position().fraction, 0.375);
        assert_eq!(
            shorter.project_normal_position(before),
            Some(playback.position())
        );
        cleared.clear_explicit_seek();
        assert!(cleared.configure(1600, region));
        assert_eq!(cleared.position().frame, 10);
        assert_eq!(cleared.position().fraction, 0.125);
        assert_eq!(
            SourceLoopDomain::physical(1600, region).project_normal_position(before),
            Some(cleared.position())
        );
        let changed_geometry = SourceLoopDomain::musical(
            1600,
            FrameRange {
                start: 20,
                end: 1520,
            },
            1500.25,
        )
        .unwrap();
        playback.clear_explicit_seek();
        assert!(playback.configure_domain(changed_geometry));
        assert_eq!(playback.position().frame, 20);
        assert_eq!(playback.position().fraction, 0.0);
    }

    #[test]
    fn clearing_real_intro_or_tail_seek_still_normalizes_outside_loop_to_start() {
        let region = FrameRange { start: 2, end: 5 };
        let domain = SourceLoopDomain::musical(6, region, 3.25).unwrap();
        for (frame, mode) in [
            (0, ExplicitSeekMode::BeforeLoop),
            (5, ExplicitSeekMode::AfterLoop),
        ] {
            let mut playback = SourcePlayback::new(frame, mode, 0.73);
            assert!(playback.configure_domain(domain));
            assert!(playback.advance(1));
            assert_eq!(playback.position().seek_mode, mode);
            playback.clear_explicit_seek();
            assert!(playback.configure_domain(domain));
            assert_eq!(playback.position().frame, 2);
            assert_eq!(playback.position().fraction, 0.0);
            assert_eq!(playback.position().seek_mode, ExplicitSeekMode::Normal);
        }
    }

    #[test]
    fn copied_musical_domain_requires_exact_period_and_preserves_integer_control_arithmetic() {
        let region = FrameRange { start: 7, end: 101 };
        let mut physical = SourcePlayback::new(7, ExplicitSeekMode::Normal, 0.73);
        assert!(physical.configure(101, region));
        let mut musical = SourcePlayback::new(7, ExplicitSeekMode::Normal, 0.73);
        assert!(musical.configure_domain(SourceLoopDomain::musical(101, region, 94.0).unwrap()));
        for frames in [1, 31, 96, 257, 512, 1_000_001] {
            assert!(physical.advance(frames));
            assert!(musical.advance(frames));
            assert_eq!(physical.position(), musical.position());
        }
        let mut changed = musical;
        assert!(changed.configure_domain(
            SourceLoopDomain::musical(101, region, f64::from_bits(94.0_f64.to_bits() + 1)).unwrap(),
        ));
        assert!(!musical.matches_exact(&changed));
        assert_eq!(musical.loop_period(), Some(94.0));
    }

    #[test]
    fn intro_and_tail_traverse_physical_extent_once_then_use_fractional_musical_period() {
        let region = FrameRange { start: 13, end: 71 };
        let domain = SourceLoopDomain::musical(100, region, 58.25).unwrap();
        for (frame, mode, prefix) in [
            (0, ExplicitSeekMode::BeforeLoop, 13.0),
            (89, ExplicitSeekMode::AfterLoop, 11.0),
        ] {
            let mut playback = SourcePlayback::new(frame, mode, 0.73);
            assert!(playback.configure_domain(domain));
            assert!(playback.advance(7));
            let earlier = playback.position();
            assert_eq!(earlier.seek_mode, mode);
            assert!((earlier.frame as f64 + earlier.fraction - frame as f64 - 5.11).abs() < 1e-12);
            assert!(playback.advance(93));
            let phase = (100.0_f64 * 0.73 - prefix).rem_euclid(58.25);
            let position = playback.position();
            assert_eq!(position.seek_mode, ExplicitSeekMode::Normal);
            assert_eq!(position.frame, 13 + phase.floor() as usize);
            assert_eq!(position.fraction, phase.fract());
        }
    }

    #[test]
    fn musical_rate_epochs_and_copied_clock_are_partition_invariant_on_both_sides_of_physical_end()
    {
        let region = FrameRange { start: 7, end: 101 };
        for period in [93.75, 94.25] {
            let domain = SourceLoopDomain::musical(110, region, period).unwrap();
            let run = |partition: &[usize]| {
                let mut playback =
                    SourcePlayback::new(7, ExplicitSeekMode::Normal, 0.9345678901234568);
                assert!(playback.configure_domain(domain));
                assert!(playback.advance(17));
                assert!(playback.set_target(1.1234567890123457));
                let mut trace = Vec::new();
                let mut rendered = 0;
                let mut segment = 0;
                while rendered < 8_321 {
                    let requested = partition[segment % partition.len()].min(8_321 - rendered);
                    let mut remaining = requested;
                    while remaining > 0 {
                        let (frames, ratio) = playback.chunk(remaining);
                        trace.extend(
                            (0..frames)
                                .map(|offset| (ratio, playback.position_at(offset).unwrap())),
                        );
                        assert!(playback.advance(frames));
                        remaining -= frames;
                    }
                    rendered += requested;
                    segment += 1;
                }
                (trace, playback)
            };
            let (reference, checkpoint) = run(&[1]);
            let mut independent_distance: f64 = 17.0 * 0.9345678901234568;
            for (ratio, position) in &reference {
                let phase = independent_distance.rem_euclid(period);
                let actual = position.frame as f64 + position.fraction - 7.0;
                assert!((actual - phase).abs() < 1.0e-8);
                independent_distance += ratio;
            }
            for partition in [&[512][..], &[31, 257, 1, 96, 777][..]] {
                let (actual, copied) = run(partition);
                assert_eq!(actual, reference);
                assert!(checkpoint.matches_exact(&copied));
            }
        }
    }

    #[test]
    fn prepared_adoption_requires_equal_future_rate_target_and_smoothing_interval() {
        let region = FrameRange { start: 7, end: 101 };
        let mut current = SourcePlayback::new(7, ExplicitSeekMode::Normal, 1.0);
        assert!(current.configure(101, region));
        let mut changed_target = current;
        assert!(changed_target.set_target(1.0 + 2.0_f64.powi(-24)));
        assert_eq!(current.position(), changed_target.position());
        assert_eq!(current.tempo_ratio(), changed_target.tempo_ratio());
        assert!(!current.matches_exact(&changed_target));
        assert!(!current.matches_rate_target(&changed_target));
        assert_ne!(current.chunk(1).1, changed_target.chunk(1).1);

        let ramp = |frames| {
            let mut playback = SourcePlayback::new(7, ExplicitSeekMode::Normal, 1.0);
            assert!(playback.configure(101, region));
            assert!(playback.set_target(1.2));
            playback.chunk(frames);
            assert!(playback.advance(frames));
            assert!(playback.seek(47, ExplicitSeekMode::Normal));
            assert!(playback.configure(101, region));
            playback
        };
        let mut later_step = ramp(100);
        let mut earlier_step = ramp(101);
        assert_eq!(later_step.position(), earlier_step.position());
        assert_eq!(later_step.tempo_ratio(), earlier_step.tempo_ratio());
        assert!(!later_step.matches_exact(&earlier_step));
        assert!(later_step.matches_rate_target(&earlier_step));
        assert_eq!(later_step.chunk(512).0, 412);
        assert_eq!(earlier_step.chunk(512).0, 411);
    }

    #[test]
    fn copied_prepared_trajectory_matches_exactly_across_canonical_rate_steps_and_partitions() {
        let mut initial = SourcePlayback::new(95, ExplicitSeekMode::AfterLoop, 0.73);
        assert!(initial.configure(100, FrameRange { start: 13, end: 71 }));
        assert!(initial.advance(2));
        assert!(initial.set_target(1.371_234_567_890_123));
        let advance = |mut playback: SourcePlayback, partition: &[usize]| {
            let mut elapsed = 0;
            let mut segment = 0;
            while elapsed < 8_321 {
                let requested = partition[segment % partition.len()].min(8_321 - elapsed);
                let mut remaining = requested;
                while remaining > 0 {
                    let (frames, _) = playback.chunk(remaining);
                    assert!(playback.advance(frames));
                    remaining -= frames;
                }
                elapsed += requested;
                segment += 1;
            }
            playback
        };
        let reference = advance(initial, &[1]);
        for partition in [&[512][..], &[31, 257, 1, 96, 777][..]] {
            let prepared = advance(initial, partition);
            assert!(reference.matches_exact(&prepared));
            assert_eq!(reference.position(), prepared.position());
        }
    }

    #[test]
    fn long_constant_epoch_uses_exact_count_instead_of_segment_rounding() {
        let ratio = 123.45_f64 / 97.3_f64;
        let mut playback = SourcePlayback::new(7, ExplicitSeekMode::Normal, ratio);
        assert!(playback.configure(101, FrameRange { start: 7, end: 101 }));
        // Thirty minutes at 96 kHz, advanced with unrelated chunk sizes.
        let total = 96_000 * 60 * 30;
        for frames in [1, 31, 512, total - 544] {
            assert!(playback.advance(frames));
        }
        let distance = total as f64 * ratio;
        let position = playback.position();
        assert_eq!(position.frame, 7 + distance.floor() as usize % 94);
        assert_eq!(position.fraction, distance.fract());
        let narrowed_distance = total as f64 * f64::from(ratio as f32);
        assert_ne!(distance.floor(), narrowed_distance.floor());
    }

    #[test]
    fn binary64_target_change_below_one_binary32_step_retains_fractional_source_epoch() {
        // Both rates round to 1.0 in binary32, but this target must change the live source rate.
        let target = 1.0 + 2.0_f64.powi(-24);
        assert_eq!(target as f32, 1.0);
        let mut playback = SourcePlayback::new(7, ExplicitSeekMode::Normal, 1.0);
        assert!(playback.configure(101, FrameRange { start: 7, end: 101 }));
        assert!(playback.advance(17));
        let before = playback.position();
        assert!(playback.set_target(target));
        assert_eq!(playback.chunk(8_388_608), (8_388_608, target));
        assert_eq!(playback.position(), before);
        assert!(playback.advance(8_388_608));
        let position = playback.position();
        assert_eq!(position.frame, 7 + (17 + 8_388_608) % 94);
        assert_eq!(position.fraction, 0.5);

        let next_target = target + 2.0_f64.powi(-25);
        assert!(playback.set_target(next_target));
        assert_eq!(playback.chunk(1), (1, next_target));
        assert_eq!(playback.position(), position);
        assert!(playback.advance(1));
        assert_eq!(playback.position().fraction, (0.5 + next_target).fract());
    }

    #[test]
    fn binary64_ramp_and_position_are_identical_under_unrelated_partitions() {
        let start = 0.9345678901234568;
        let target = 1.1234567890123457;
        let total = 2309;
        let region = FrameRange { start: 7, end: 101 };
        let run = |partition: &[usize]| {
            let mut playback = SourcePlayback::new(7, ExplicitSeekMode::Normal, start);
            assert!(playback.configure(101, region));
            assert!(playback.advance(17));
            assert!(playback.set_target(target));
            let mut trace = Vec::with_capacity(total);
            let mut rendered = 0;
            let mut segment = 0;
            while rendered < total {
                let requested = partition[segment % partition.len()].min(total - rendered);
                let mut remaining = requested;
                while remaining > 0 {
                    let (frames, ratio) = playback.chunk(remaining);
                    for offset in 0..frames {
                        trace.push((ratio, playback.position_at(offset).unwrap()));
                    }
                    assert!(playback.advance(frames));
                    remaining -= frames;
                }
                rendered += requested;
                segment += 1;
            }
            (trace, playback.position())
        };
        let (reference, final_position) = run(&[1]);
        for partition in [&[512][..], &[1024][..], &[31, 257, 1, 96, 777][..]] {
            let (trace, position) = run(partition);
            assert_eq!(trace, reference);
            assert_eq!(position, final_position);
        }
        let ratios = [
            start + 0.05,
            start + 0.05 + 0.05,
            start + 0.05 + 0.05 + 0.05,
            target,
        ];
        let mut distance = 17.0 * start;
        for (frame, (ratio, position)) in reference.iter().enumerate() {
            let stage = (frame / 512).min(3);
            assert_eq!(*ratio, ratios[stage]);
            // Independent scalar integral: every accepted stage spans active output frames.
            assert_eq!(position.frame, 7 + distance.floor() as usize % 94);
            assert!((position.fraction - distance.fract()).abs() < 1.0e-9);
            distance += ratios[stage];
        }
        assert_eq!(final_position.frame, 7 + distance.floor() as usize % 94);
        assert!((final_position.fraction - distance.fract()).abs() < 1.0e-9);
    }

    #[test]
    fn copied_preparation_clock_and_live_continuation_have_identical_positions() {
        let mut live = SourcePlayback::new(0, ExplicitSeekMode::BeforeLoop, 0.73);
        assert!(live.configure(100, FrameRange { start: 13, end: 71 }));
        assert!(live.advance(17));
        let mut prepared = live;
        for frames in [1, 31, 96, 257, 512] {
            for offset in 0..frames {
                assert_eq!(live.position_at(offset).unwrap(), prepared.position());
                assert!(prepared.advance(1));
            }
            assert!(live.advance(frames));
            assert_eq!(live.position(), prepared.position());
        }
    }

    #[test]
    fn exact_preparation_copy_retains_phase_and_does_not_consume_live_smoothing() {
        let mut logical = SourcePlayback::new(95, ExplicitSeekMode::AfterLoop, 0.73);
        assert!(logical.configure(100, FrameRange { start: 13, end: 71 }));
        assert!(logical.advance(2));
        assert!(logical.set_target(1.25));
        assert_eq!(logical.chunk(3).0, 3);
        assert!(logical.advance(3));
        let position = logical.position();
        let remaining_step_frames = logical.frames_until_step;
        let exact_ratio = 1.37_f64;
        let mut feed = logical.at_constant_ratio(exact_ratio);

        assert_eq!(feed.position(), position);
        assert_eq!(feed.tempo_ratio(), exact_ratio);
        assert_eq!(feed.chunk(777), (777, exact_ratio));
        assert!(feed.advance(7));
        let distance = position.fraction + 7.0 * exact_ratio;
        let expected_frame = 13 + (position.frame + distance.floor() as usize - 100) % 58;
        assert_eq!(feed.position().frame, expected_frame);
        assert_eq!(feed.position().fraction, distance.fract());
        assert_eq!(feed.position().seek_mode, ExplicitSeekMode::Normal);

        assert_eq!(logical.position(), position);
        assert_eq!(logical.target, 1.25);
        assert_eq!(logical.chunk(777).0, remaining_step_frames);
        assert_ne!(logical.tempo_ratio(), exact_ratio);
    }

    #[test]
    fn seek_retains_the_remaining_active_frames_before_a_rate_step() {
        let mut playback = SourcePlayback::new(0, ExplicitSeekMode::Normal, 1.0);
        assert!(playback.configure(
            1000,
            FrameRange {
                start: 10,
                end: 900,
            },
        ));
        assert!(playback.set_target(1.5));
        assert_eq!(playback.chunk(100), (100, 1.05));
        assert!(playback.advance(100));
        assert!(playback.seek(47, ExplicitSeekMode::Normal));
        assert!(playback.configure(
            1000,
            FrameRange {
                start: 10,
                end: 900,
            },
        ));
        assert!(playback.set_target(1.5));
        assert_eq!(playback.position().frame, 47);
        assert_eq!(playback.position().fraction, 0.0);
        assert_eq!(playback.chunk(512), (412, 1.05));
        assert!(playback.advance(412));
        let (frames, ratio) = playback.chunk(1);
        assert_eq!(frames, 1);
        assert!((ratio - 1.1).abs() < 1e-6);
    }
}
