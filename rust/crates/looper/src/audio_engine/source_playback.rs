//! Copied source clock shared by live resampling and non-realtime preparation.
//!
//! A constant-rate epoch is addressed by its active output-frame count, never by callback
//! endpoints. Rebases preserve the fractional source position. All operations are bounded and
//! allocation-free; pausing simply stops advancing this clock.

use super::constants::{SPEED_MAX, SPEED_MIN};
use super::loop_phase_reference::LoopPhaseReference;
use super::mapped_source_clock::{
    CompactLocalSpanView, CompactSpanView, GeneralLocalSpanView, MappedClockEpoch,
    MappedClockError, MappedLoopGeometry, MappedSourceSpan, PhaseLocalSpanView,
};
#[cfg(test)]
use super::source_reader::FrameRange;
use super::source_reader::{
    ExplicitSeekMode, SourceLoopDomain, SourceReaderPeriod, advance_playback_position,
    playhead_before_render,
};
use super::static_source_clock::{StaticClockError, StaticPhaseClock};
use super::transport::TransportBeatProjection;

const TEMPO_STEP: f64 = 0.05;
const TEMPO_STEP_OUTPUT_FRAMES: usize = 512;
const MUSICAL_PAIR_SCALE: u128 = 1u128 << 53;
const MUSICAL_PAIR_MAX_PERIOD: f64 = 18_446_744_073_709_551_616.0;

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

/// Mutually exclusive copied trajectories reuse the existing mapped storage.
/// Neither variant retains an owning source, map, reader or permission.
#[derive(Clone, Copy, Debug)]
enum CopiedClock {
    Mapped(MappedCopyState),
    Static(StaticPhaseClock),
}

/// Temporary coordinates borrowing the caller's original retained geometry.
/// This is not a source/custody permit and never enters stored copied playback.
pub(crate) struct CompactPlaybackView<'a> {
    span: CompactSpanView<'a>,
    intended_output_frame: u64,
}

impl CompactPlaybackView<'_> {
    pub(crate) fn position_at(&self, offset: usize) -> Option<FractionalSourcePosition> {
        self.span
            .position_at(self.intended_output_frame.checked_add(offset as u64)?)
    }
}

/// Prepared coordinates from an already admitted compact span. This contains no
/// geometry owner and is discarded after the reader chunk or copied query.
pub(crate) struct CompactLocalPlaybackView {
    span: LocalMappedSpanView,
    intended_output_frame: u64,
}

enum LocalMappedSpanView {
    Intersection(CompactLocalSpanView),
    General(GeneralLocalSpanView),
    Phase(PhaseLocalSpanView),
}

impl CompactLocalPlaybackView {
    pub(crate) fn position_at(&self, offset: usize) -> Option<FractionalSourcePosition> {
        let frame = self.intended_output_frame.checked_add(offset as u64)?;
        match &self.span {
            LocalMappedSpanView::Intersection(span) => span.position_at(frame),
            LocalMappedSpanView::General(span) => span.position_at(frame),
            LocalMappedSpanView::Phase(span) => span.position_at(frame),
        }
    }
}

/// Physical None is a valid original scalar domain and cannot alias a missing
/// original-domain witness on a generic mapped construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OriginalScalarPeriod {
    Unavailable,
    Physical,
    Musical(u64),
}

impl OriginalScalarPeriod {
    fn from_domain(domain: SourceLoopDomain) -> Self {
        match domain.musical_period {
            SourceReaderPeriod::Physical => Self::Physical,
            SourceReaderPeriod::ScalarMusical(bits) => Self::Musical(bits),
            SourceReaderPeriod::MappedRational(_)
            | SourceReaderPeriod::MappedPhysicalQuarters(_) => Self::Unavailable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScalarProjection {
    Legacy,
    PairPhysical,
    PairMusical,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SourcePlayback {
    origin: FractionalSourcePosition,
    domain: Option<SourceLoopDomain>,
    original_scalar_period: OriginalScalarPeriod,
    elapsed_output_frames: u64,
    ratio: f64,
    target: f64,
    frames_until_step: usize,
    // Before this epoch, only an intentional start/seek defines reconstructible source history.
    history_reconstructible: bool,
    scalar_projection: ScalarProjection,
    phase_reference: LoopPhaseReference,
    mapped: Option<CopiedClock>,
}

impl SourcePlayback {
    /// Called only for the selected-A birth already owned by the launch claim.
    pub(crate) fn install_launch_reference(&mut self, reference: LoopPhaseReference) {
        self.phase_reference = if self.domain.is_some_and(|domain| {
            self.position().frame == domain.region.start
                && self.position().fraction == 0.0
                && self.position().seek_mode == ExplicitSeekMode::Normal
                && self.elapsed_output_frames == 0
        }) {
            reference
        } else {
            LoopPhaseReference::Unavailable
        };
    }

    /// Same-source conversion authority remains in the caller's complete claim.
    pub(crate) fn carry_launch_reference_from(&mut self, previous: &Self) {
        self.phase_reference = match (self.domain, previous.domain) {
            (Some(a), Some(b)) if a.sample_frames == b.sample_frames && a.region == b.region => {
                previous.phase_reference
            }
            _ => LoopPhaseReference::Unavailable,
        };
    }

    pub(crate) fn phase_reference_current(
        &self,
        timeline: &super::transport::TransportTimeline,
    ) -> bool {
        self.phase_reference.current(timeline)
    }

    pub(crate) fn validate_mapped_reference(&self, geometry: &MappedLoopGeometry) -> bool {
        self.mapped_epoch().is_some_and(|epoch| {
            geometry
                .validate_epoch_with_reference(epoch, self.phase_reference)
                .is_ok()
        })
    }

    /// Caller first excludes Native/retained/source-history owners. Only a still
    /// unadvanced genuine birth can follow a same-boundary coordinate bootstrap.
    pub(crate) fn rebind_phase_birth(
        &mut self,
        previous: &super::transport::TransportTimeline,
        next: &super::transport::TransportTimeline,
        frame: u64,
    ) {
        if self.mapped.is_none()
            && self.elapsed_output_frames == 0
            && self.domain.is_some_and(|domain| {
                let raw = self.position();
                raw.frame == domain.region.start
                    && raw.fraction == 0.0
                    && raw.seek_mode == ExplicitSeekMode::Normal
            })
        {
            self.phase_reference = self.phase_reference.rebind_birth(previous, next, frame);
        }
    }

    fn mapped_state(&self) -> Option<MappedCopyState> {
        match self.mapped {
            Some(CopiedClock::Mapped(state)) => Some(state),
            _ => None,
        }
    }

    fn static_state(&self) -> Option<StaticPhaseClock> {
        match self.mapped {
            Some(CopiedClock::Static(state)) => Some(state),
            _ => None,
        }
    }

    /// Coordinates only, admitted at actual E by the original complete caller claim.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn static_continuation(
        previous: &Self,
        timeline: super::transport::TransportTimeline,
        execution: u64,
        marker: u64,
        frames_per_beat: f64,
        origin_frame: f64,
        quarters: f64,
    ) -> Result<Self, StaticClockError> {
        if !previous.phase_reference.current(&timeline) {
            return Err(StaticClockError::InvalidEpoch);
        }
        let projection = timeline
            .beat_projection()
            .ok_or(StaticClockError::InvalidEpoch)?;
        let (_, _, launch_master) = previous
            .phase_reference
            .parts()
            .ok_or(StaticClockError::InvalidEpoch)?;
        if previous.is_mapped()
            || previous.is_static_phase()
            || previous.ratio.to_bits() != previous.target.to_bits()
        {
            return Err(StaticClockError::InvalidEpoch);
        }
        let domain = previous.domain.ok_or(StaticClockError::InvalidGeometry)?;
        let raw = previous.position();
        let valid = match domain.musical_period {
            SourceReaderPeriod::Physical => valid_physical_pair(raw, domain),
            SourceReaderPeriod::ScalarMusical(_) => valid_musical_pair(raw, domain),
            SourceReaderPeriod::MappedRational(_)
            | SourceReaderPeriod::MappedPhysicalQuarters(_) => false,
        };
        if !valid {
            return Err(StaticClockError::InvalidGeometry);
        }
        let clock = StaticPhaseClock::new(
            raw,
            domain,
            projection,
            execution,
            marker,
            frames_per_beat,
            origin_frame,
            quarters,
            previous.target,
            launch_master,
        )?;
        Ok(Self {
            origin: raw,
            domain: Some(domain),
            original_scalar_period: OriginalScalarPeriod::from_domain(domain),
            elapsed_output_frames: 0,
            ratio: clock.ratio(),
            target: previous.target,
            frames_until_step: 0,
            history_reconstructible: false,
            scalar_projection: ScalarProjection::Legacy,
            mapped: Some(CopiedClock::Static(clock)),
            phase_reference: previous.phase_reference,
        })
    }

    pub(crate) fn is_static_phase(&self) -> bool {
        self.static_state().is_some()
    }
    pub(crate) fn static_output_frame(&self) -> Option<u64> {
        self.static_state().map(|clock| clock.intended_frame())
    }
    pub(crate) fn static_first_frame(&self) -> Option<u64> {
        self.static_state().map(|clock| clock.first_frame())
    }
    pub(crate) fn static_marker(&self) -> Option<u64> {
        self.static_state().map(|clock| clock.marker())
    }
    pub(crate) fn static_master_period(&self) -> Option<f64> {
        self.static_state().map(|clock| clock.master_period())
    }
    pub(crate) fn static_matches_projection(&self, projection: TransportBeatProjection) -> bool {
        self.static_state()
            .is_some_and(|clock| clock.matches_projection(projection))
    }
    pub(crate) fn static_grid_matches(&self, f: f64, o: f64, q: f64) -> bool {
        self.static_state()
            .is_some_and(|clock| clock.grid_matches(f, o, q))
    }
    pub(crate) fn static_phase_settled(&self) -> bool {
        self.static_state().is_some_and(|clock| clock.settled())
    }
    /// Even captured actual-M coordinates are not a constant-rate scalar history.
    pub(crate) fn has_constant_scalar_trajectory(&self) -> bool {
        self.mapped.is_none() && self.ratio.to_bits() == self.target.to_bits()
    }

    /// Caller custody owns the effect. Preserve the current pair and derivative;
    /// no earlier scalar history can be reconstructed from this new endpoint.
    pub(crate) fn dissolve_static(&mut self) -> bool {
        let Some(clock) = self.static_state() else {
            return true;
        };
        let Some(domain) = self.domain else {
            return false;
        };
        let position = self.origin;
        let construct = match domain.musical_period {
            SourceReaderPeriod::Physical => Self::scalar_continuation,
            SourceReaderPeriod::ScalarMusical(_) => Self::scalar_musical_continuation,
            SourceReaderPeriod::MappedRational(_)
            | SourceReaderPeriod::MappedPhysicalQuarters(_) => return false,
        };
        let Some(mut replacement) =
            construct(position, domain, clock.ratio(), clock.nominal_ratio())
        else {
            return false;
        };
        replacement.carry_launch_reference_from(self);
        *self = replacement;
        true
    }

    /// The installed coordinate programme is not a source, phase-read or Native permit.
    pub(crate) fn is_mapped_phase(&self) -> bool {
        self.mapped_state()
            .is_some_and(|state| state.span.epoch().is_common_phase())
    }

    /// These dry phase trajectories have not been integrated with Native preparation.
    pub(crate) fn has_phase_program(&self) -> bool {
        self.is_static_phase() || self.is_mapped_phase()
    }

    pub(crate) fn mapped_phase_settled(
        &self,
        geometry: &MappedLoopGeometry,
    ) -> Result<bool, MappedClockError> {
        let state = self.mapped_state().ok_or(MappedClockError::InvalidEpoch)?;
        if !state.span.epoch().is_common_phase() || self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        state.span.epoch().common_phase_settled(
            geometry,
            state.intended_output_frame,
            self.phase_reference,
        )
    }

    /// Reuse an already mapped lane's separately retained scalar witness. Its current
    /// pair may be outside that older circle until a genuine scalar reverse qualifies.
    fn retained_scalar_for_mapped_recovery(
        geometry: &MappedLoopGeometry,
        previous: &Self,
    ) -> Result<OriginalScalarPeriod, MappedClockError> {
        let original = previous
            .original_scalar_domain()
            .ok_or(MappedClockError::InvalidGeometry)?;
        let anchor = FractionalSourcePosition {
            frame: original.region.start,
            fraction: 0.0,
            seek_mode: ExplicitSeekMode::Normal,
        };
        let valid = match original.musical_period {
            SourceReaderPeriod::Physical => {
                Self::scalar_continuation(anchor, original, 1.0, 1.0).is_some()
            }
            SourceReaderPeriod::ScalarMusical(_) => {
                Self::scalar_musical_continuation(anchor, original, 1.0, 1.0).is_some()
            }
            SourceReaderPeriod::MappedRational(_)
            | SourceReaderPeriod::MappedPhysicalQuarters(_) => false,
        };
        if !previous.is_mapped()
            || previous.domain != Some(geometry.domain())
            || !valid
            || !original.is_valid()
            || original.sample_frames != geometry.domain().sample_frames
            || original.region != geometry.domain().region
        {
            return Err(MappedClockError::InvalidGeometry);
        }
        Ok(previous.original_scalar_period)
    }

    /// Coordinates only. The original all-participant claim separately owns the
    /// actual source/map/reader/full request and exact first-PCM admission.
    pub(crate) fn mapped_phase_continuation(
        geometry: &MappedLoopGeometry,
        marker: u64,
        timeline: super::transport::TransportTimeline,
        previous: &Self,
        output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        let original = Self::retained_scalar_for_mapped_recovery(geometry, previous)?;
        if previous.mapped_output_frame() != Some(output_frame) {
            return Err(MappedClockError::InvalidEpoch);
        }
        let reference = geometry.qualify_phase_reference(previous.phase_reference, &timeline)?;
        let epoch = MappedClockEpoch::common_phase(
            marker,
            timeline,
            geometry,
            previous.position(),
            output_frame,
            reference,
        )?;
        let mut replacement =
            Self::mapped_with_reference(geometry, epoch, output_frame, reference)?;
        if !same_position(previous.position(), replacement.position()) {
            return Err(MappedClockError::NumericalUnavailable);
        }
        replacement.original_scalar_period = original;
        Ok(replacement)
    }

    /// Rebase a CURRENT mapped phase under its caller's original actual-E claim.
    pub(crate) fn mapped_ordinary_recovery_at(
        geometry: &MappedLoopGeometry,
        marker: u64,
        timeline: super::transport::TransportTimeline,
        previous: &Self,
        output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        if previous.mapped_output_frame() != Some(output_frame) {
            return Err(MappedClockError::InvalidEpoch);
        }
        Self::mapped_resume_recovery_at(geometry, marker, timeline, previous, output_frame)
    }

    /// The original paused/cause cohort must prove suspension before calling.
    /// Keep the frozen pair, use LIVE M(R), and never shift the old phase clock.
    pub(crate) fn mapped_resume_recovery_at(
        geometry: &MappedLoopGeometry,
        marker: u64,
        timeline: super::transport::TransportTimeline,
        previous: &Self,
        output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        let original = Self::retained_scalar_for_mapped_recovery(geometry, previous)?;
        if !previous.is_mapped_phase()
            || previous
                .mapped_output_frame()
                .is_none_or(|frozen| output_frame < frozen)
        {
            return Err(MappedClockError::InvalidEpoch);
        }
        let epoch = MappedClockEpoch::ordinary_mapped_recovery(
            marker,
            timeline,
            geometry,
            previous.position(),
            output_frame,
        )?;
        let mut replacement = Self::mapped(geometry, epoch, output_frame)?;
        if !same_position(previous.position(), replacement.position()) {
            return Err(MappedClockError::NumericalUnavailable);
        }
        replacement.original_scalar_period = original;
        replacement.carry_launch_reference_from(previous);
        Ok(replacement)
    }

    /// An already mapped voice may move to a different prepared intersection
    /// circle without losing its separately retained original scalar circle.
    /// Physical/source ownership and first-PCM equality remain the caller's claim.
    pub(crate) fn mapped_compact_replacement(
        geometry: &MappedLoopGeometry,
        marker: u64,
        timeline: super::transport::TransportTimeline,
        previous: &Self,
        output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        let effective = previous.domain.ok_or(MappedClockError::InvalidGeometry)?;
        let original = previous
            .original_scalar_domain()
            .ok_or(MappedClockError::InvalidGeometry)?;
        let target = geometry.domain();
        // Validate historical scalar metadata at its own canonical start. The
        // current mapped pair need not lie in that older circle until reverse.
        let original_anchor = FractionalSourcePosition {
            frame: original.region.start,
            fraction: 0.0,
            seek_mode: ExplicitSeekMode::Normal,
        };
        let original_valid = match original.musical_period {
            SourceReaderPeriod::Physical => {
                Self::scalar_continuation(original_anchor, original, 1.0, 1.0).is_some()
            }
            SourceReaderPeriod::ScalarMusical(_) => {
                Self::scalar_musical_continuation(original_anchor, original, 1.0, 1.0).is_some()
            }
            SourceReaderPeriod::MappedRational(_)
            | SourceReaderPeriod::MappedPhysicalQuarters(_) => false,
        };
        if !previous.is_mapped()
            || previous.mapped_output_frame() != Some(output_frame)
            || !effective.is_valid()
            || !original.is_valid()
            || !original_valid
            || original.musical_period.is_mapped()
            || effective.sample_frames != target.sample_frames
            || effective.region != target.region
            || original.sample_frames != target.sample_frames
            || original.region != target.region
            || target.musical_period.is_mapped()
            || !geometry.has_compact_continuation()
        {
            return Err(MappedClockError::InvalidGeometry);
        }
        let mut replacement = Self::mapped_anchored_compact_continuation(
            geometry,
            marker,
            timeline,
            target,
            previous.position(),
            output_frame,
        )?;
        replacement.original_scalar_period = previous.original_scalar_period;
        replacement.carry_launch_reference_from(previous);
        Ok(replacement)
    }

    /// Productive general coordinates retain their original scalar circle
    /// independently of the effective rational reader certificate.
    pub(crate) fn mapped_general_continuation(
        geometry: &MappedLoopGeometry,
        marker: u64,
        timeline: super::transport::TransportTimeline,
        original_scalar_domain: SourceLoopDomain,
        position: FractionalSourcePosition,
        output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        let original = OriginalScalarPeriod::from_domain(original_scalar_domain);
        if original == OriginalScalarPeriod::Unavailable {
            return Err(MappedClockError::InvalidGeometry);
        }
        let epoch = MappedClockEpoch::general_compact(
            marker,
            timeline,
            geometry,
            original_scalar_domain,
            position,
            output_frame,
        )?;
        let mut replacement = Self::mapped(geometry, epoch, output_frame)?;
        if !same_position(position, replacement.origin) {
            return Err(MappedClockError::NumericalUnavailable);
        }
        replacement.original_scalar_period = original;
        Ok(replacement)
    }

    /// Bounded actual-E coordinates. Original source/map/ACK custody and the
    /// complete live cohort claim remain the caller's separate responsibility.
    pub(crate) fn mapped_anchored_compact_continuation(
        geometry: &MappedLoopGeometry,
        marker: u64,
        timeline: super::transport::TransportTimeline,
        original_domain: SourceLoopDomain,
        position: FractionalSourcePosition,
        output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        let epoch = MappedClockEpoch::anchored_compact(
            marker,
            timeline,
            geometry,
            original_domain,
            position,
            output_frame,
        )?;
        let mut replacement = Self::mapped(geometry, epoch, output_frame)?;
        if !same_position(position, replacement.origin) {
            return Err(MappedClockError::NumericalUnavailable);
        }
        replacement.original_scalar_period = OriginalScalarPeriod::from_domain(original_domain);
        Ok(replacement)
    }

    /// A tagged span has already passed whole-original-geometry admission.
    /// Untagged legacy and explicit wide reference trajectories retain their path.
    pub(crate) fn local_compact_view(
        &self,
    ) -> Result<Option<CompactLocalPlaybackView>, MappedClockError> {
        let Some(state) = self.mapped_state().filter(|state| {
            state.span.epoch().is_compact_anchored()
                || state.span.epoch().is_general_compact()
                || state.span.epoch().is_common_phase()
        }) else {
            return Ok(None);
        };
        let span = if state.span.epoch().is_common_phase() {
            LocalMappedSpanView::Phase(state.span.local_phase_view(
                self.domain.ok_or(MappedClockError::InvalidGeometry)?,
                self.phase_reference,
            )?)
        } else if state.span.epoch().is_general_compact() {
            LocalMappedSpanView::General(
                state
                    .span
                    .local_general_view(self.domain.ok_or(MappedClockError::InvalidGeometry)?)?,
            )
        } else {
            LocalMappedSpanView::Intersection(state.span.local_compact_view()?)
        };
        let view = CompactLocalPlaybackView {
            span,
            intended_output_frame: state.intended_output_frame,
        };
        if !view
            .position_at(0)
            .is_some_and(|position| same_position(position, self.origin))
        {
            return Err(MappedClockError::InvalidGeometry);
        }
        Ok(Some(view))
    }

    /// Reference anchored map coordinates only; no productive caller or realtime-cost permit.
    /// The caller retains the same qualified original owning geometry and reader custody.
    pub(crate) fn mapped_anchored_continuation(
        geometry: &MappedLoopGeometry,
        marker: u64,
        timeline: super::transport::TransportTimeline,
        original_domain: SourceLoopDomain,
        position: FractionalSourcePosition,
        output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        let epoch = MappedClockEpoch::anchored(
            marker,
            timeline,
            geometry,
            original_domain,
            position,
            output_frame,
        )?;
        let mut replacement = Self::mapped(geometry, epoch, output_frame)?;
        if !same_position(position, replacement.origin) {
            return Err(MappedClockError::NumericalUnavailable);
        }
        replacement.original_scalar_period = OriginalScalarPeriod::from_domain(original_domain);
        Ok(replacement)
    }

    /// The caller separately retains and qualifies the original immutable map
    /// owner. This value contains only a copied trajectory and identity markers.
    pub(crate) fn mapped(
        geometry: &MappedLoopGeometry,
        epoch: MappedClockEpoch,
        intended_output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        Self::mapped_with_reference(
            geometry,
            epoch,
            intended_output_frame,
            LoopPhaseReference::Unavailable,
        )
    }

    fn mapped_with_reference(
        geometry: &MappedLoopGeometry,
        epoch: MappedClockEpoch,
        intended_output_frame: u64,
        reference: LoopPhaseReference,
    ) -> Result<Self, MappedClockError> {
        let span = geometry.refresh_span_with_reference(epoch, intended_output_frame, reference)?;
        let origin = span
            .position_in_domain_with_reference(geometry.domain(), intended_output_frame, reference)
            .ok_or(MappedClockError::NumericalUnavailable)?;
        let ratio = span.local_rate().ok_or(MappedClockError::UnsupportedRate)?;
        Ok(Self {
            origin,
            domain: Some(geometry.domain()),
            original_scalar_period: OriginalScalarPeriod::from_domain(geometry.domain()),
            elapsed_output_frames: 0,
            ratio,
            target: ratio,
            frames_until_step: 0,
            history_reconstructible: false,
            scalar_projection: ScalarProjection::Legacy,
            mapped: Some(CopiedClock::Mapped(MappedCopyState {
                span,
                intended_output_frame,
            })),
            phase_reference: reference,
        })
    }

    /// Explicit compact-only coordinates. Epoch construction and original source
    /// qualification remain separate off-realtime prerequisites; no wide fallback.
    pub(crate) fn mapped_compact(
        geometry: &MappedLoopGeometry,
        epoch: MappedClockEpoch,
        intended_output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        Self::mapped_compact_with_reference(
            geometry,
            epoch,
            intended_output_frame,
            LoopPhaseReference::Unavailable,
        )
    }

    fn mapped_compact_with_reference(
        geometry: &MappedLoopGeometry,
        epoch: MappedClockEpoch,
        intended_output_frame: u64,
        reference: LoopPhaseReference,
    ) -> Result<Self, MappedClockError> {
        if epoch.is_general_compact() || epoch.is_common_phase() {
            return Self::mapped_with_reference(geometry, epoch, intended_output_frame, reference);
        }
        let span = geometry.refresh_span_compact(epoch, intended_output_frame)?;
        let origin = span
            .compact_view(geometry)?
            .position_at(intended_output_frame)
            .ok_or(MappedClockError::NumericalUnavailable)?;
        let ratio = span.local_rate().ok_or(MappedClockError::UnsupportedRate)?;
        Ok(Self {
            origin,
            domain: Some(geometry.domain()),
            original_scalar_period: OriginalScalarPeriod::from_domain(geometry.domain()),
            elapsed_output_frames: 0,
            ratio,
            target: ratio,
            frames_until_step: 0,
            history_reconstructible: false,
            scalar_projection: ScalarProjection::Legacy,
            mapped: Some(CopiedClock::Mapped(MappedCopyState {
                span,
                intended_output_frame,
            })),
            phase_reference: reference,
        })
    }

    /// Borrow numeric context only; equal geometry does not establish map custody.
    pub(crate) fn compact_view<'a>(
        &self,
        geometry: &'a MappedLoopGeometry,
    ) -> Result<CompactPlaybackView<'a>, MappedClockError> {
        let state = self.mapped_state().ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let span = state
            .span
            .compact_view_with_reference(geometry, self.phase_reference)?;
        if !span
            .position_at(state.intended_output_frame)
            .is_some_and(|position| same_position(position, self.origin))
        {
            return Err(MappedClockError::InvalidGeometry);
        }
        Ok(CompactPlaybackView {
            span,
            intended_output_frame: state.intended_output_frame,
        })
    }

    pub(crate) fn position_at_compact(
        &self,
        geometry: &MappedLoopGeometry,
        output_offset: usize,
    ) -> Option<FractionalSourcePosition> {
        self.compact_view(geometry).ok()?.position_at(output_offset)
    }

    /// Refusal leaves the complete prior copied state intact.
    pub(crate) fn advance_compact(
        &mut self,
        geometry: &MappedLoopGeometry,
        output_frames: usize,
    ) -> bool {
        let Some(mut state) = self.mapped_state() else {
            return false;
        };
        let Some(next) = state
            .intended_output_frame
            .checked_add(output_frames as u64)
        else {
            return false;
        };
        let Some(position) = self.position_at_compact(geometry, output_frames) else {
            return false;
        };
        state.intended_output_frame = next;
        self.mapped = Some(CopiedClock::Mapped(state));
        self.origin = position;
        true
    }

    pub(crate) fn refresh_mapped_span_compact(
        &mut self,
        geometry: &MappedLoopGeometry,
    ) -> Result<(), MappedClockError> {
        let state = self.mapped_state().ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let mut replacement = Self::mapped_compact_with_reference(
            geometry,
            state.span.epoch(),
            state.intended_output_frame,
            self.phase_reference,
        )?;
        if !same_position(self.origin, replacement.origin) {
            return Err(MappedClockError::InvalidGeometry);
        }
        replacement.original_scalar_period = self.original_scalar_period;
        *self = replacement;
        Ok(())
    }

    pub(crate) fn project_mapped_compact_to(
        &mut self,
        geometry: &MappedLoopGeometry,
        intended_output_frame: u64,
    ) -> Result<(), MappedClockError> {
        let state = self.mapped_state().ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let mut replacement = Self::mapped_compact_with_reference(
            geometry,
            state.span.epoch(),
            intended_output_frame,
            self.phase_reference,
        )?;
        replacement.original_scalar_period = self.original_scalar_period;
        *self = replacement;
        Ok(())
    }

    pub(crate) fn replace_mapped_epoch_compact(
        &mut self,
        geometry: &MappedLoopGeometry,
        epoch: MappedClockEpoch,
    ) -> Result<(), MappedClockError> {
        let state = self.mapped_state().ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        if !state
            .span
            .epoch()
            .replacement_anchor_matches(epoch, state.intended_output_frame)
        {
            return Err(MappedClockError::InvalidEpoch);
        }
        let mut replacement = Self::mapped_compact_with_reference(
            geometry,
            epoch,
            state.intended_output_frame,
            self.phase_reference,
        )?;
        if !same_position(self.origin, replacement.origin) {
            return Err(MappedClockError::InvalidEpoch);
        }
        replacement.original_scalar_period = self.original_scalar_period;
        *self = replacement;
        Ok(())
    }

    pub(crate) fn resume_mapped_compact_at(
        &mut self,
        geometry: &MappedLoopGeometry,
        actual: &super::transport::TransportTimeline,
        output_frame: u64,
    ) -> Result<(), MappedClockError> {
        if self.is_mapped_phase() {
            return self.resume_mapped_at(geometry, actual, output_frame);
        }
        let state = self.mapped_state().ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let epoch = state
            .span
            .epoch()
            .resumed_at(actual, state.intended_output_frame, output_frame)
            .ok_or(MappedClockError::InvalidEpoch)?;
        let mut replacement = Self::mapped_compact_with_reference(
            geometry,
            epoch,
            output_frame,
            self.phase_reference,
        )?;
        if !same_position(self.origin, replacement.origin) {
            return Err(MappedClockError::InvalidEpoch);
        }
        replacement.original_scalar_period = self.original_scalar_period;
        *self = replacement;
        Ok(())
    }

    pub(crate) fn is_mapped(&self) -> bool {
        self.mapped_state().is_some()
    }

    pub(crate) fn is_compact_mapped(&self) -> bool {
        self.mapped_state().is_some_and(|state| {
            state.span.epoch().is_compact_anchored()
                || state.span.epoch().is_general_compact()
                || state.span.epoch().is_common_phase()
        })
    }

    pub(crate) fn is_general_mapped(&self) -> bool {
        self.mapped_state()
            .is_some_and(|state| state.span.epoch().is_general_compact())
    }

    pub(crate) fn effective_domain(&self) -> Option<SourceLoopDomain> {
        self.domain
    }

    /// Reconstruct the retained original scalar circle. A generic general map
    /// without that proof cannot manufacture a Physical fallback.
    pub(crate) fn original_scalar_domain(&self) -> Option<SourceLoopDomain> {
        let domain = self.domain?;
        let musical_period = if !self.is_mapped() {
            if domain.musical_period.is_mapped() {
                return None;
            }
            domain.musical_period
        } else {
            match self.original_scalar_period {
                OriginalScalarPeriod::Unavailable => return None,
                OriginalScalarPeriod::Physical => SourceReaderPeriod::Physical,
                OriginalScalarPeriod::Musical(bits) => SourceReaderPeriod::ScalarMusical(bits),
            }
        };
        Some(SourceLoopDomain {
            musical_period,
            ..domain
        })
    }

    /// Stable renderer configuration only, never playback progress or permission.
    pub(crate) fn retry_domain(&self) -> Option<SourceLoopDomain> {
        self.domain
    }

    fn is_pair_scalar(&self) -> bool {
        matches!(
            self.scalar_projection,
            ScalarProjection::PairPhysical | ScalarProjection::PairMusical
        )
    }

    pub(crate) fn mapped_epoch(&self) -> Option<MappedClockEpoch> {
        self.mapped_state().map(|state| state.span.epoch())
    }

    pub(crate) fn mapped_output_frame(&self) -> Option<u64> {
        self.mapped_state().map(|state| state.intended_output_frame)
    }

    /// Include the canonical exclusive endpoint, but never invent a later span.
    pub(crate) fn can_read_frames(&self, output_frames: usize) -> bool {
        if let Some(clock) = self.static_state() {
            return clock
                .intended_frame()
                .checked_add(output_frames as u64)
                .and_then(|frame| clock.position_at(self.domain?, frame))
                .is_some();
        }
        if self.is_pair_scalar() {
            return self
                .elapsed_output_frames
                .checked_add(output_frames as u64)
                .is_some();
        }
        self.mapped_state().is_none_or(|state| {
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
        let state = self.mapped_state().ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let mut replacement = Self::mapped_with_reference(
            geometry,
            state.span.epoch(),
            state.intended_output_frame,
            self.phase_reference,
        )?;
        if !same_position(self.origin, replacement.origin) {
            return Err(MappedClockError::InvalidGeometry);
        }
        replacement.original_scalar_period = self.original_scalar_period;
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
        let state = self.mapped_state().ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let mut replacement = Self::mapped_with_reference(
            geometry,
            state.span.epoch(),
            intended_output_frame,
            self.phase_reference,
        )?;
        replacement.original_scalar_period = self.original_scalar_period;
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
        let state = self.mapped_state().ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        if !state
            .span
            .epoch()
            .replacement_anchor_matches(epoch, state.intended_output_frame)
        {
            return Err(MappedClockError::InvalidEpoch);
        }
        let mut replacement = Self::mapped_with_reference(
            geometry,
            epoch,
            state.intended_output_frame,
            self.phase_reference,
        )?;
        if !same_position(self.origin, replacement.origin) {
            return Err(MappedClockError::InvalidEpoch);
        }
        replacement.original_scalar_period = self.original_scalar_period;
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
        if self.is_mapped_phase() {
            let marker = self
                .mapped_epoch()
                .ok_or(MappedClockError::InvalidEpoch)?
                .marker();
            let replacement =
                Self::mapped_resume_recovery_at(geometry, marker, *actual, self, output_frame)?;
            *self = replacement;
            return Ok(());
        }
        let state = self.mapped_state().ok_or(MappedClockError::InvalidEpoch)?;
        if self.domain != Some(geometry.domain()) {
            return Err(MappedClockError::InvalidGeometry);
        }
        let epoch = state
            .span
            .epoch()
            .resumed_at(actual, state.intended_output_frame, output_frame)
            .ok_or(MappedClockError::InvalidEpoch)?;
        let mut replacement = Self::mapped(geometry, epoch, output_frame)?;
        replacement.carry_launch_reference_from(self);
        if !same_position(self.origin, replacement.origin) {
            return Err(MappedClockError::InvalidEpoch);
        }
        replacement.original_scalar_period = self.original_scalar_period;
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
        if domain.musical_period.is_mapped()
            || !coordinate.is_finite()
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
            original_scalar_period: OriginalScalarPeriod::Unavailable,
            elapsed_output_frames: 0,
            ratio,
            target: ratio,
            frames_until_step: 0,
            history_reconstructible: true,
            scalar_projection: ScalarProjection::Legacy,
            phase_reference: LoopPhaseReference::Unavailable,
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
            original_scalar_period: OriginalScalarPeriod::from_domain(domain),
            elapsed_output_frames: 0,
            ratio: initial_ratio,
            target,
            frames_until_step: 0,
            history_reconstructible: false,
            scalar_projection: ScalarProjection::PairPhysical,
            phase_reference: LoopPhaseReference::Unavailable,
            mapped: None,
        })
    }

    /// Coordinates only, with the exact original musical domain and reader semantics.
    /// An invalid anchor is refused, never wrapped into a different actual-E cursor.
    /// This grants no source, conversion, reader, adoption or earlier-history permission.
    pub(crate) fn scalar_musical_continuation(
        position: FractionalSourcePosition,
        domain: SourceLoopDomain,
        initial_ratio: f64,
        target: f64,
    ) -> Option<Self> {
        if !valid_musical_pair(position, domain)
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
            original_scalar_period: OriginalScalarPeriod::from_domain(domain),
            elapsed_output_frames: 0,
            ratio: initial_ratio,
            target,
            frames_until_step: 0,
            history_reconstructible: false,
            scalar_projection: ScalarProjection::PairMusical,
            phase_reference: LoopPhaseReference::Unavailable,
            mapped: None,
        })
    }

    /// Copy the current source phase into a constant-rate preparation epoch.
    ///
    /// Pending live smoothing is removed only from this value. Its logical counterpart retains
    /// the accepted target and active-frame step interval.
    #[cfg(test)]
    pub(crate) fn at_constant_ratio(mut self, ratio: f64) -> Self {
        if self.is_mapped() || self.is_static_phase() {
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
        self.static_state()
            .map_or(self.ratio, |clock| clock.ratio())
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
        if self.is_mapped() || self.is_static_phase() {
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
        if self.is_pair_scalar() {
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
            let period = domain
                .musical_period
                .scalar_period()
                .unwrap_or(domain.region.len() as f64);
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
        if self.is_mapped() || self.is_static_phase() || self.is_pair_scalar() {
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
        if domain.musical_period.is_mapped() {
            return None;
        }
        let period = domain
            .musical_period
            .scalar_period()
            .unwrap_or(domain.region.len() as f64);
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
        if !self.phase_reference.matches_exact(other.phase_reference) {
            return false;
        }
        match (self.static_state(), other.static_state()) {
            (Some(left), Some(right)) => {
                return self.domain == other.domain
                    && self.original_scalar_period == other.original_scalar_period
                    && left.matches_exact(right);
            }
            (Some(_), None) | (None, Some(_)) => return false,
            (None, None) => {}
        }
        match (self.mapped_state(), other.mapped_state()) {
            (Some(left), Some(right)) => {
                return self.domain == other.domain
                    && self.original_scalar_period == other.original_scalar_period
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
            && self.original_scalar_period == other.original_scalar_period
            && self.elapsed_output_frames == other.elapsed_output_frames
            && self.ratio.to_bits() == other.ratio.to_bits()
            && self.target.to_bits() == other.target.to_bits()
            && self.frames_until_step == other.frames_until_step
            && self.history_reconstructible == other.history_reconstructible
            && self.scalar_projection == other.scalar_projection
    }

    /// Check pending preparation control intent while its copied smoothing clock advances.
    pub(crate) fn matches_rate_target(&self, other: &Self) -> bool {
        if !self.phase_reference.matches_exact(other.phase_reference) {
            return false;
        }
        match (self.static_state(), other.static_state()) {
            (Some(left), Some(right)) => {
                return self.domain == other.domain
                    && self.original_scalar_period == other.original_scalar_period
                    && left.matches_program(right);
            }
            (Some(_), None) | (None, Some(_)) => return false,
            (None, None) => {}
        }
        match (self.mapped_state(), other.mapped_state()) {
            (Some(left), Some(right)) => {
                return self.domain == other.domain
                    && self.original_scalar_period == other.original_scalar_period
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
        self.domain
            .and_then(|domain| domain.musical_period.scalar_period())
    }

    /// Actual reader plans retain the complete effective period certificate.
    pub(crate) fn reader_period(&self) -> SourceReaderPeriod {
        self.domain
            .map_or(SourceReaderPeriod::Physical, |domain| domain.musical_period)
    }

    pub(crate) fn matches_domain(&self, domain: SourceLoopDomain) -> bool {
        self.domain == Some(domain)
    }

    #[must_use]
    pub(crate) fn configure_domain(&mut self, domain: SourceLoopDomain) -> bool {
        if self.domain == Some(domain) {
            return true;
        }
        if domain.musical_period.is_mapped() {
            return false;
        }
        if self.is_static_phase() {
            let mut replacement = *self;
            if !replacement.dissolve_static() {
                return false;
            }
            // A real edit resumes the established scalar domain-edit policy.
            replacement.scalar_projection = ScalarProjection::Legacy;
            if !replacement.configure_domain(domain) {
                return false;
            }
            *self = replacement;
            return true;
        }
        // A new domain needs its own future integration contract. An unchanged
        // pair refresh retains this exact epoch; do not clamp, wrap or rebase it.
        if self.is_pair_scalar() {
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
        self.phase_reference = LoopPhaseReference::Unavailable;
        self.original_scalar_period = OriginalScalarPeriod::from_domain(domain);
        let outside = if let Some(period) = domain.musical_period.scalar_period() {
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
        // Both pair modes are already Normal. An unchanged refresh retains the epoch.
        if self.is_mapped() || self.is_static_phase() || self.is_pair_scalar() {
            return;
        }
        self.rebase();
        let was_explicit = self.origin.seek_mode != ExplicitSeekMode::Normal;
        self.origin.seek_mode = ExplicitSeekMode::Normal;
        // Exiting intro/tail normalizes on the next configuration. An already normal
        // cursor retains its geometry so a same-marker timing refresh cannot discard
        // the virtual seam's fractional residue.
        if was_explicit {
            self.phase_reference = LoopPhaseReference::Unavailable;
            self.domain = None;
            self.original_scalar_period = OriginalScalarPeriod::Unavailable;
            self.history_reconstructible = false;
        }
    }

    #[must_use]
    pub(crate) fn set_target(&mut self, target: f64) -> bool {
        if self.is_mapped() {
            return false;
        }
        let target = checked_ratio(target);
        if self.is_static_phase() {
            if target.to_bits() == self.target.to_bits() {
                return true;
            }
            if !self.dissolve_static() {
                return false;
            }
        }
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
        if self.is_static_phase() && !self.dissolve_static() {
            return false;
        }
        self.origin = FractionalSourcePosition {
            frame,
            fraction: 0.0,
            seek_mode,
        };
        self.phase_reference = LoopPhaseReference::Unavailable;
        self.domain = None;
        self.original_scalar_period = OriginalScalarPeriod::Unavailable;
        self.elapsed_output_frames = 0;
        self.history_reconstructible = true;
        self.scalar_projection = ScalarProjection::Legacy;
        true
    }

    /// Begin a constant-ratio chunk, bounded by the next active-output-frame smoothing step.
    pub(crate) fn chunk(&mut self, max_frames: usize) -> (usize, f64) {
        if let Some(clock) = self.static_state() {
            return clock.chunk(max_frames);
        }
        if let Some(state) = self.mapped_state() {
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
            self.frames_until_step = if self.is_pair_scalar() && self.ratio == self.target {
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
        if self.is_mapped() || self.is_static_phase() {
            self.origin
        } else {
            match self.scalar_projection {
                ScalarProjection::Legacy => self.scalar_position_at(0),
                ScalarProjection::PairPhysical => {
                    self.pair_physical_position_at(self.elapsed_output_frames)
                }
                ScalarProjection::PairMusical => {
                    self.pair_musical_position_at(self.elapsed_output_frames)
                }
            }
        }
    }

    pub(crate) fn position_at(&self, output_offset: usize) -> Option<FractionalSourcePosition> {
        if let Some(clock) = self.static_state() {
            return clock.position_at(
                self.domain?,
                clock.intended_frame().checked_add(output_offset as u64)?,
            );
        }
        if let Some(state) = self.mapped_state() {
            return state.span.position_in_domain_with_reference(
                self.domain?,
                state
                    .intended_output_frame
                    .checked_add(output_offset as u64)?,
                self.phase_reference,
            );
        }
        if self.is_pair_scalar() {
            let elapsed = self
                .elapsed_output_frames
                .checked_add(output_offset as u64)?;
            return Some(match self.scalar_projection {
                ScalarProjection::PairMusical => self.pair_musical_position_at(elapsed),
                _ => self.pair_physical_position_at(elapsed),
            });
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

    fn pair_musical_position_at(&self, elapsed: u64) -> FractionalSourcePosition {
        let domain = self.domain.expect("musical pair has its admitted domain");
        let period = musical_pair_units(
            domain
                .musical_period
                .scalar_period()
                .expect("musical pair period"),
        );
        // The fixed common grid H=2^53 exactly represents every admitted period
        // and rate. C=P*H <=2^117; D=n*rate*H <2^118 for the entire u64 epoch.
        let displacement = u128::from(elapsed) * musical_pair_units(self.ratio);
        let relative = (self.origin.frame - domain.region.start) as u128;
        let base = relative * MUSICAL_PAIR_SCALE + displacement % period;
        // The canonical anchor gives base<2C. Keep f separate even when it is
        // subnormal, including when f closes a sub-frame gap across the seam.
        let wrap = base >= period
            || (period - base < MUSICAL_PAIR_SCALE
                && self.origin.fraction >= (period - base) as f64 / MUSICAL_PAIR_SCALE as f64);
        let residual = base as i128 - if wrap { period as i128 } else { 0 };
        let scale = MUSICAL_PAIR_SCALE as i128;
        let mut whole = residual.div_euclid(scale);
        let remainder = residual.rem_euclid(scale) as u128;
        // Generic integer displacement preserves all raw f bits, including -0,
        // at every exact revisit. This is not an actual-E-only cursor override.
        let mut fraction = self.origin.fraction;
        if remainder != 0 {
            let addend = remainder as f64 / MUSICAL_PAIR_SCALE as f64;
            // Error-free TwoSum. Classify the exact carry before rounding the
            // canonical fractional remainder; a rounded sum of 1 may lie below 1.
            let sum = fraction + addend;
            let virtual_addend = sum - fraction;
            let error = (fraction - (sum - virtual_addend)) + (addend - virtual_addend);
            if sum > 1.0 || (sum == 1.0 && error >= 0.0) {
                whole += 1;
                // sum-1 is exact (Sterbenz); this is one final fractional RN.
                fraction = (sum - 1.0) + error;
            } else {
                fraction = sum;
            }
            if fraction == 1.0 {
                whole += 1;
                fraction = 0.0;
            }
            // Rounding below P can reach exactly P. Since its fractional part
            // is itself exactly representable, it cannot round beyond P.
            if !canonical_musical_phase(whole as u128, fraction, period) {
                whole = 0;
                fraction = 0.0;
            }
        }
        // Only the canonical fraction was rounded, with circular error <=2^-54
        // source frame per constant epoch, not for an ideal unrounded whole glide.
        FractionalSourcePosition {
            frame: domain.region.start + whole as usize,
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
            && let Some(period) = domain.musical_period.scalar_period()
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
        if !self.try_advance_coordinates(output_frames) {
            return false;
        }
        true
    }

    fn try_advance_coordinates(&mut self, output_frames: usize) -> bool {
        if let Some(mut clock) = self.static_state() {
            if !clock.advance(output_frames) {
                return false;
            }
            let Some(position) = self
                .domain
                .and_then(|domain| clock.position_at(domain, clock.intended_frame()))
            else {
                return false;
            };
            self.origin = position;
            self.ratio = clock.ratio();
            self.mapped = Some(CopiedClock::Static(clock));
            return true;
        }
        if let Some(mut state) = self.mapped_state() {
            let Some(next) = state
                .intended_output_frame
                .checked_add(output_frames as u64)
            else {
                return false;
            };
            let Some(domain) = self.domain else {
                return false;
            };
            let Some(position) =
                state
                    .span
                    .position_in_domain_with_reference(domain, next, self.phase_reference)
            else {
                return false;
            };
            state.intended_output_frame = next;
            self.mapped = Some(CopiedClock::Mapped(state));
            self.origin = position;
            return true;
        }
        if self.is_pair_scalar() {
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
    domain.musical_period == SourceReaderPeriod::Physical
        && domain.region.start < domain.region.end
        && domain.region.end <= domain.sample_frames
        && position.seek_mode == ExplicitSeekMode::Normal
        && position.frame >= domain.region.start
        && position.frame < domain.region.end
        && position.fraction.is_finite()
        && (0.0..1.0).contains(&position.fraction)
}

/// Exact units on H=2^53 for a validated normal value in [.5,2^64].
fn musical_pair_units(value: f64) -> u128 {
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as u32;
    let significand = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
    u128::from(significand) << (exponent - 1022)
}

fn canonical_musical_phase(whole: u128, fraction: f64, period: u128) -> bool {
    let whole_units = whole * MUSICAL_PAIR_SCALE;
    if whole_units >= period {
        return false;
    }
    let gap = period - whole_units;
    gap >= MUSICAL_PAIR_SCALE || fraction < gap as f64 / MUSICAL_PAIR_SCALE as f64
}

fn valid_musical_pair(position: FractionalSourcePosition, domain: SourceLoopDomain) -> bool {
    let Some(period) = domain.musical_period.scalar_period() else {
        return false;
    };
    if !period.is_finite()
        || !(0.5..=MUSICAL_PAIR_MAX_PERIOD).contains(&period)
        || domain.region.start >= domain.region.end
        || domain.region.end > domain.sample_frames
        || position.seek_mode != ExplicitSeekMode::Normal
        || position.frame < domain.region.start
        || !position.fraction.is_finite()
        || !(0.0..1.0).contains(&position.fraction)
    {
        return false;
    }
    let period = musical_pair_units(period);
    let length = domain.region.len() as u128 * MUSICAL_PAIR_SCALE;
    period.abs_diff(length) <= MUSICAL_PAIR_SCALE
        && canonical_musical_phase(
            (position.frame - domain.region.start) as u128,
            position.fraction,
            period,
        )
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
#[path = "source_playback_musical_tests.rs"]
mod source_playback_musical_tests;

#[cfg(test)]
#[path = "source_playback_anchored_tests.rs"]
mod anchored_tests;

#[cfg(test)]
#[path = "source_playback_compact_tests.rs"]
mod compact_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_reference_is_immutable_through_virtual_history_and_invalidated_by_real_seek_or_domain()
     {
        let timeline = super::super::transport::TransportTimeline::new(48_000);
        let region = FrameRange {
            start: 20,
            end: 100,
        };
        let mut playback = SourcePlayback::new(20, ExplicitSeekMode::Normal, 1.0);
        assert!(playback.configure(120, region));
        playback.install_launch_reference(LoopPhaseReference::capture(Some(timeline), 0));
        let reference = playback.phase_reference;
        assert!(playback.advance(97));
        let endpoint = playback;
        let (mut history, silence) = playback.history_before_constant(64).unwrap();
        assert_eq!(silence, 0);
        assert!(history.phase_reference.matches_exact(reference));
        assert!(history.advance(64));
        assert!(history.matches_exact(&endpoint));
        assert!(playback.configure(120, region));
        assert!(playback.phase_reference.matches_exact(reference));
        let mut seek = playback;
        assert!(seek.seek(seek.position().frame, ExplicitSeekMode::Normal));
        assert!(!seek.phase_reference_current(&timeline));
        assert!(playback.configure(
            120,
            FrameRange {
                start: 21,
                end: 100
            }
        ));
        assert!(playback.configure(120, region));
        assert!(
            !playback.phase_reference_current(&timeline),
            "domain ABA cannot restore launch custody"
        );
    }

    #[test]
    fn original_scalar_period_is_a_small_copy_witness_with_explicit_unavailability() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<OriginalScalarPeriod>();
        assert!(std::mem::size_of::<OriginalScalarPeriod>() <= 16);
        assert!(!std::mem::needs_drop::<OriginalScalarPeriod>());
        assert_ne!(
            OriginalScalarPeriod::Unavailable,
            OriginalScalarPeriod::Physical
        );
        println!(
            "original_scalar_period_layout: bytes={} unavailable_distinct_from_physical=true",
            std::mem::size_of::<OriginalScalarPeriod>(),
        );
    }

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
