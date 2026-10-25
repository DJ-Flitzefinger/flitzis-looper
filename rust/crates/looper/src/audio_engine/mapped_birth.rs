//! Fresh mapped source beginnings. Numeric coordinates never grant source custody.

use super::key_lock_start_policy::KeyLockStartBehavior;
use super::loop_phase_reference::LoopPhaseReference;
use super::mapped_source_clock::MappedLoopGeometry;
use super::native_history_permit::NativeHistoryPermitIdentity;
use super::pitch_actions::PitchActionReject;
use super::pitch_intent::PitchIntent;
use super::prepared_native_history::{NativeHistoryRequest, SourceReadCheckpoint};
use super::source_playback::SourcePlayback;
use super::transport::TransportTimeline;

pub(crate) fn playback_at_birth(
    geometry: &MappedLoopGeometry,
    marker: u64,
    timeline: TransportTimeline,
    scalar: SourceReadCheckpoint,
    execution: u64,
    reference: LoopPhaseReference,
) -> Result<SourceReadCheckpoint, PitchActionReject> {
    let position = scalar.playback.position();
    if scalar.playback.is_mapped()
        || scalar.playback.has_phase_program()
        || position.frame != scalar.plan.loop_region.start
        || position.fraction.to_bits() != 0.0_f64.to_bits()
        || scalar.plan.transition.is_active()
        || scalar.plan.seek_mode != super::source_reader::ExplicitSeekMode::Normal
        || scalar.plan.loop_region != geometry.domain().region
    {
        return Err(PitchActionReject::MappedTimingUnavailable);
    }
    let mut playback = if geometry.has_general_continuation() {
        SourcePlayback::mapped_general_continuation(
            geometry,
            marker,
            timeline,
            scalar.plan.domain(),
            position,
            execution,
        )
    } else {
        SourcePlayback::mapped_anchored_compact_continuation(
            geometry,
            marker,
            timeline,
            scalar.plan.domain(),
            position,
            execution,
        )
    }
    .map_err(|_| PitchActionReject::MappedTimingUnavailable)?;
    playback.install_launch_reference(reference);
    Ok(SourceReadCheckpoint {
        playback,
        plan: super::source_reader::SourceReadPlan {
            loop_period: playback.reader_period(),
            ..scalar.plan
        },
    })
}

/// Exact physical-birth readiness, separate from scalar attack measurement.
#[derive(Clone, Copy)]
pub(crate) struct MappedFullBeginningReady {
    permit: NativeHistoryPermitIdentity,
    origin: SourceReadCheckpoint,
    request: u64,
    epoch: u64,
    execution: u64,
    pitch: PitchIntent,
    key_lock: bool,
}

impl MappedFullBeginningReady {
    pub(crate) fn capture(request: &NativeHistoryRequest) -> Option<Self> {
        request
            .mapped_input
            .as_ref()
            .filter(|input| input.matches_request(request))?;
        Some(Self {
            permit: request.permit.scalar_identity(),
            origin: SourceReadCheckpoint {
                playback: request.playback,
                plan: request.plan,
            },
            request: request.request_id,
            epoch: request.epoch,
            execution: request.target_output_frame,
            pitch: request.pitch_intent,
            key_lock: request.key_lock,
        })
    }

    pub(crate) fn matches(&self, request: &NativeHistoryRequest) -> bool {
        self.permit == request.permit.scalar_identity()
            && self.request == request.request_id
            && self.epoch == request.epoch
            && self.execution == request.target_output_frame
            && self.pitch == request.pitch_intent
            && self.key_lock == request.key_lock
            && self.origin.matches_exact(SourceReadCheckpoint {
                playback: request.playback,
                plan: request.plan,
            })
            && request
                .mapped_input
                .as_ref()
                .is_some_and(|input| input.matches_request(request) && input.pending_current())
    }
}

/// Fixed wire data. Mixed observations never borrow one contributor's coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BirthStatus {
    pub(crate) requested: KeyLockStartBehavior,
    pub(crate) mixed: bool,
    pub(crate) original: Option<u64>,
    pub(crate) execution: Option<u64>,
    pub(crate) reference_qualified: bool,
    pub(crate) emitted: bool,
}

impl BirthStatus {
    pub(crate) fn mixed() -> Self {
        Self {
            requested: KeyLockStartBehavior::FirstSoundAtGrid,
            mixed: true,
            original: None,
            execution: None,
            reference_qualified: false,
            emitted: false,
        }
    }
    pub(crate) fn requested_name(self) -> &'static str {
        if self.mixed {
            "mixed"
        } else {
            self.requested.as_str()
        }
    }
    pub(crate) fn alignment(self) -> &'static str {
        if self.mixed {
            "mixed"
        } else if self.emitted && self.requested == KeyLockStartBehavior::FirstSoundAtGrid {
            "earliest_bound_met"
        } else {
            "pending"
        }
    }
    pub(crate) fn reason(self) -> Option<&'static str> {
        if self.mixed {
            Some("mixed")
        } else if self.requested == KeyLockStartBehavior::AttackAtGrid {
            Some("no_qualified_anchor")
        } else if !self.emitted {
            Some("output_unconfirmed")
        } else {
            None
        }
    }
}

/// First-interval eligibility is consumed even when that render fails.
#[derive(Clone, Copy)]
pub(crate) struct MappedBirth {
    pub(crate) status: BirthStatus,
    pub(crate) key: super::content_placement::ContentKey,
    pub(crate) source_generation: u64,
    pub(crate) source_request_incarnation: u64,
    pub(crate) source_start: usize,
    pub(crate) native: bool,
    pub(crate) key_lock_request: u64,
    first_pending: bool,
}

impl MappedBirth {
    pub(crate) fn first_pending(&self) -> bool {
        self.first_pending
    }
    pub(crate) fn new(
        status: BirthStatus,
        key: super::content_placement::ContentKey,
        source_generation: u64,
        source_request_incarnation: u64,
        source_start: usize,
        native: bool,
        key_lock_request: u64,
    ) -> Self {
        Self {
            status,
            key,
            source_generation,
            source_request_incarnation,
            source_start,
            native,
            key_lock_request,
            first_pending: true,
        }
    }
    pub(crate) fn take_first_interval(&mut self, frame: Option<u64>) -> bool {
        let pending = std::mem::replace(&mut self.first_pending, false);
        pending
            && frame == self.status.execution
            && self
                .status
                .original
                .zip(frame)
                .is_some_and(|(original, execution)| execution >= original)
    }
}
