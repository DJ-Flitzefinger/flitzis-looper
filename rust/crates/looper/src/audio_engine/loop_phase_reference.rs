//! Copied phase metadata, never source or publication authority.
//! Intentional launch history and requested grid axes have distinct purposes.

use super::source_beatmap::CanonicalBeatSegment;
use super::transport::TransportTimeline;

/// Requested loaded-frame grid zero plus its captured Native timing authority.
/// This copied identity is not an acknowledgement of source or timing adoption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GridPhaseOrigin {
    pub(crate) frame_bits: u64,
    pub(crate) authority_revision: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GridPhaseTarget {
    pub(crate) origin: GridPhaseOrigin,
    pub(crate) inverse: CanonicalBeatSegment,
}

impl GridPhaseTarget {
    pub(crate) fn matches_exact(self, other: Self) -> bool {
        self.origin == other.origin && self.inverse.matches_exact(other.inverse)
    }
}

pub(crate) fn same_grid_target(a: Option<GridPhaseTarget>, b: Option<GridPhaseTarget>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.matches_exact(b),
        _ => false,
    }
}

/// Existing work-owner data: future M(T) is a prediction until actual effect.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LaunchQualification {
    projection: Option<super::transport::TransportBeatProjection>,
    admission_frame: u64,
}

impl LaunchQualification {
    pub(crate) fn capture(timeline: Option<TransportTimeline>, admission_frame: u64) -> Self {
        Self {
            projection: timeline.and_then(|value| value.beat_projection()),
            admission_frame,
        }
    }

    pub(crate) fn at_effect(
        self,
        reference: LoopPhaseReference,
        timeline: Option<TransportTimeline>,
        execution: u64,
    ) -> bool {
        let Some(timeline) = timeline else {
            return false;
        };
        let Some((_, original, _)) = reference.parts() else {
            return false;
        };
        original >= self.admission_frame
            && execution >= original
            && reference.current(&timeline)
            && self
                .projection
                .is_some_and(|projection| projection.matches_timeline(&timeline))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum LoopPhaseReference {
    #[default]
    Unavailable,
    Launch {
        generation: u64,
        frame: u64,
        master: f64,
    },
    Mapped {
        generation: u64,
        frame: u64,
        master: f64,
        start_inverse: CanonicalBeatSegment,
    },
}

impl LoopPhaseReference {
    /// Only the original scheduler/launch claimant may install these coordinates.
    pub(crate) fn capture(timeline: Option<TransportTimeline>, frame: u64) -> Self {
        let Some(timeline) = timeline else {
            return Self::Unavailable;
        };
        let Some(master) = timeline
            .beat_position_at_frame(frame)
            .filter(|m| m.is_finite())
        else {
            return Self::Unavailable;
        };
        Self::Launch {
            generation: timeline.phase_generation(),
            frame,
            master,
        }
    }

    pub(crate) fn parts(self) -> Option<(u64, u64, f64)> {
        match self {
            Self::Unavailable => None,
            Self::Launch {
                generation,
                frame,
                master,
            }
            | Self::Mapped {
                generation,
                frame,
                master,
                ..
            } => Some((generation, frame, master)),
        }
    }

    pub(crate) fn current(self, timeline: &TransportTimeline) -> bool {
        self.parts().is_some_and(|(generation, _, master)| {
            generation == timeline.phase_generation()
                && master.is_finite()
                && timeline.beat_projection().is_some()
        })
    }

    pub(crate) fn qualify(self, start_inverse: CanonicalBeatSegment) -> Self {
        let Some((generation, frame, master)) = self.parts() else {
            return Self::Unavailable;
        };
        Self::Mapped {
            generation,
            frame,
            master,
            start_inverse,
        }
    }

    pub(crate) fn mapped_parts(self) -> Option<(f64, CanonicalBeatSegment)> {
        match self {
            Self::Mapped {
                master,
                start_inverse,
                ..
            } => Some((master, start_inverse)),
            _ => None,
        }
    }

    pub(crate) fn rebind_birth(
        self,
        previous: &TransportTimeline,
        next: &TransportTimeline,
        at: u64,
    ) -> Self {
        match self {
            Self::Launch {
                generation, frame, ..
            } if generation == previous.phase_generation()
                && frame == at
                && generation != next.phase_generation() =>
            {
                Self::capture(Some(*next), at)
            }
            _ => self,
        }
    }

    pub(crate) fn matches_exact(self, other: Self) -> bool {
        let same_parts = match (self.parts(), other.parts()) {
            (None, None) => true,
            (Some((ag, af, am)), Some((bg, bf, bm))) => {
                ag == bg && af == bf && am.to_bits() == bm.to_bits()
            }
            _ => false,
        };
        same_parts
            && match (self, other) {
                (Self::Unavailable, Self::Unavailable)
                | (Self::Launch { .. }, Self::Launch { .. }) => true,
                (
                    Self::Mapped {
                        start_inverse: a, ..
                    },
                    Self::Mapped {
                        start_inverse: b, ..
                    },
                ) => a.matches_exact(b),
                _ => false,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn future_launch_requires_the_admitted_projection_at_actual_effect() {
        let mut timeline = TransportTimeline::new(48_000);
        timeline.advance_by_rendered_frames(17_000);
        let reference = LoopPhaseReference::capture(Some(timeline), 24_000);
        let qualification = LaunchQualification::capture(Some(timeline), 17_000);
        assert!(qualification.at_effect(reference, Some(timeline), 24_000));
        assert!(!qualification.at_effect(reference, Some(timeline), 23_999));
        let generation = timeline.phase_generation();
        timeline.advance_by_rendered_frames(1_000);
        assert!(timeline.set_master_period_preserving_beat_position_at_frame(0.75, 18_000));
        assert_eq!(timeline.phase_generation(), generation);
        assert!(
            reference.current(&timeline),
            "continuous tempo retains coordinate origin"
        );
        assert!(
            !qualification.at_effect(reference, Some(timeline), 24_000),
            "future M(T) changed; a copied prediction cannot certify the actual launch"
        );
        assert!(!qualification.at_effect(reference, None, 24_000));
    }

    #[test]
    fn late_admission_and_coordinate_aba_cannot_qualify_old_launch() {
        let mut timeline = TransportTimeline::new(48_000);
        let reference = LoopPhaseReference::capture(Some(timeline), 0);
        let qualified = LaunchQualification::capture(Some(timeline), 0);
        assert!(qualified.at_effect(reference, Some(timeline), 8_192));
        assert!(!LaunchQualification::capture(Some(timeline), 1).at_effect(
            reference,
            Some(timeline),
            8_192
        ));
        assert!(timeline.anchor_beat_position_at_frame(64.0, 0));
        assert!(timeline.anchor_beat_position_at_frame(0.0, 0));
        assert_eq!(timeline.beat_position(), Some(0.0));
        assert!(!reference.current(&timeline));
        assert!(!qualified.at_effect(reference, Some(timeline), 8_192));
    }

    #[test]
    fn completed_launch_survives_later_continuous_tempo_but_not_relabel() {
        let mut timeline = TransportTimeline::new(48_000);
        let reference = LoopPhaseReference::capture(Some(timeline), 0);
        assert!(LaunchQualification::capture(Some(timeline), 0).at_effect(
            reference,
            Some(timeline),
            0
        ));
        timeline.advance_by_rendered_frames(137);
        assert!(timeline.set_master_period_preserving_beat_position_at_frame(0.75, 137));
        assert!(reference.current(&timeline));
        assert!(timeline.anchor_beat_position_at_frame(64.0, 137));
        assert!(!reference.current(&timeline));
    }
}
