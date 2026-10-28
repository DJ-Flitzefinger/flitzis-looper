//! Bounded observation of requested global rate and its separate callback effect.
//! Neither record grants source authority. Each record has exactly one writer.

use super::master_provenance::MasterProvenance;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RateTuple {
    pub(crate) speed: f64,
    pub(crate) period: Option<f64>,
}

impl Default for RateTuple {
    fn default() -> Self {
        Self {
            speed: 1.0,
            period: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RateState {
    Preparing,
    Applied,
    Rejected,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RateError {
    #[default]
    None,
    SourceChanged,
    HistoryUnavailable,
    Capacity,
    Superseded,
    TimingChanged,
    MissedBoundary,
    OtherRatePending,
    InvalidTransport,
    ParticipantChanged,
}

impl RateError {
    pub(crate) fn message(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::SourceChanged => Some("Rate change refused: source changed"),
            Self::HistoryUnavailable => Some("Rate change refused: source history unavailable"),
            Self::Capacity => Some("Rate change refused: preparation capacity unavailable"),
            Self::Superseded => Some("Rate change superseded by a newer request"),
            Self::TimingChanged => Some("Rate change refused: timing changed"),
            Self::MissedBoundary => Some("Rate change refused: preparation was not ready"),
            Self::OtherRatePending => Some("Rate change refused: another rate edit is preparing"),
            Self::InvalidTransport => Some("Rate change refused: invalid master timing"),
            Self::ParticipantChanged => Some("Rate change refused: playing voices changed"),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RateEffect {
    pub(crate) revision: u64,
    pub(crate) requested: RateTuple,
    pub(crate) effective: RateTuple,
    pub(crate) state: RateState,
    pub(crate) execution_frame: Option<u64>,
    pub(crate) error: RateError,
}

struct RateRecord {
    sequence: AtomicU64,
    revision: AtomicU64,
    speed: AtomicU64,
    period: AtomicU64,
    requested_speed: AtomicU64,
    requested_period: AtomicU64,
    execution: AtomicU64,
    state: AtomicU8,
    error: AtomicU8,
    master_kind: AtomicU8,
    master_source: [AtomicU64; 4],
    master_accepted: [AtomicU64; 4],
    master_has_accepted: AtomicU8,
}

impl Default for RateRecord {
    fn default() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            revision: AtomicU64::new(0),
            speed: AtomicU64::new(1.0_f64.to_bits()),
            period: AtomicU64::new(0),
            requested_speed: AtomicU64::new(1.0_f64.to_bits()),
            requested_period: AtomicU64::new(0),
            execution: AtomicU64::new(u64::MAX),
            state: AtomicU8::new(0),
            error: AtomicU8::new(0),
            master_kind: AtomicU8::new(0),
            master_source: std::array::from_fn(|_| AtomicU64::new(0)),
            master_accepted: std::array::from_fn(|_| AtomicU64::new(0)),
            master_has_accepted: AtomicU8::new(0),
        }
    }
}

impl RateRecord {
    fn publish(&self, value: RateEffect, master_provenance: MasterProvenance) {
        let sequence = self.sequence.load(Ordering::SeqCst);
        let Some(next) = sequence.checked_add(2).filter(|_| sequence & 1 == 0) else {
            // A permanently invalid observation is safer than reusing a wrapped version.
            self.sequence.store(1, Ordering::SeqCst);
            return;
        };
        self.sequence.store(next - 1, Ordering::SeqCst);
        self.revision.store(value.revision, Ordering::SeqCst);
        self.speed
            .store(value.effective.speed.to_bits(), Ordering::SeqCst);
        self.period.store(
            value.effective.period.map_or(0, f64::to_bits),
            Ordering::SeqCst,
        );
        self.requested_speed
            .store(value.requested.speed.to_bits(), Ordering::SeqCst);
        self.requested_period.store(
            value.requested.period.map_or(0, f64::to_bits),
            Ordering::SeqCst,
        );
        self.execution
            .store(value.execution_frame.unwrap_or(u64::MAX), Ordering::SeqCst);
        self.state.store(value.state as u8, Ordering::SeqCst);
        self.error.store(value.error as u8, Ordering::SeqCst);
        let (kind, source, accepted) = match master_provenance {
            MasterProvenance::Absent => (0, [0; 32], None),
            MasterProvenance::Unknown => (1, [0; 32], None),
            MasterProvenance::Qualified {
                source_identity,
                accepted_revision,
            } => (2, source_identity, accepted_revision),
        };
        self.master_kind.store(kind, Ordering::SeqCst);
        self.master_has_accepted
            .store(u8::from(accepted.is_some()), Ordering::SeqCst);
        let accepted = accepted.unwrap_or([0; 32]);
        for index in 0..4 {
            let offset = index * 8;
            self.master_source[index].store(
                u64::from_le_bytes(
                    source[offset..offset + 8]
                        .try_into()
                        .expect("fixed source digest"),
                ),
                Ordering::SeqCst,
            );
            self.master_accepted[index].store(
                u64::from_le_bytes(
                    accepted[offset..offset + 8]
                        .try_into()
                        .expect("fixed accepted digest"),
                ),
                Ordering::SeqCst,
            );
        }
        self.sequence.store(next, Ordering::SeqCst);
    }

    fn read(&self) -> Option<(u64, RateEffect, MasterProvenance)> {
        let sequence = self.sequence.load(Ordering::SeqCst);
        if sequence == 0 || sequence & 1 != 0 {
            return None;
        }
        let period = self.period.load(Ordering::SeqCst);
        let requested_period = self.requested_period.load(Ordering::SeqCst);
        let execution = self.execution.load(Ordering::SeqCst);
        let state = match self.state.load(Ordering::SeqCst) {
            0 => RateState::Preparing,
            1 => RateState::Applied,
            _ => RateState::Rejected,
        };
        let error = match self.error.load(Ordering::SeqCst) {
            0 => RateError::None,
            1 => RateError::SourceChanged,
            2 => RateError::HistoryUnavailable,
            3 => RateError::Capacity,
            4 => RateError::Superseded,
            5 => RateError::TimingChanged,
            6 => RateError::MissedBoundary,
            7 => RateError::OtherRatePending,
            8 => RateError::InvalidTransport,
            _ => RateError::ParticipantChanged,
        };
        let value = RateEffect {
            revision: self.revision.load(Ordering::SeqCst),
            requested: RateTuple {
                speed: f64::from_bits(self.requested_speed.load(Ordering::SeqCst)),
                period: (requested_period != 0).then(|| f64::from_bits(requested_period)),
            },
            effective: RateTuple {
                speed: f64::from_bits(self.speed.load(Ordering::SeqCst)),
                period: (period != 0).then(|| f64::from_bits(period)),
            },
            state,
            execution_frame: (execution != u64::MAX).then_some(execution),
            error,
        };
        let mut source_identity = [0; 32];
        let mut accepted_revision = [0; 32];
        for index in 0..4 {
            let offset = index * 8;
            source_identity[offset..offset + 8].copy_from_slice(
                &self.master_source[index]
                    .load(Ordering::SeqCst)
                    .to_le_bytes(),
            );
            accepted_revision[offset..offset + 8].copy_from_slice(
                &self.master_accepted[index]
                    .load(Ordering::SeqCst)
                    .to_le_bytes(),
            );
        }
        let master_provenance = match self.master_kind.load(Ordering::SeqCst) {
            0 => MasterProvenance::Absent,
            1 => MasterProvenance::Unknown,
            2 => MasterProvenance::Qualified {
                source_identity,
                accepted_revision: (self.master_has_accepted.load(Ordering::SeqCst) != 0)
                    .then_some(accepted_revision),
            },
            _ => return None,
        };
        (self.sequence.load(Ordering::SeqCst) == sequence).then_some((
            sequence,
            value,
            master_provenance,
        ))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct GlobalRateStatus {
    pub(crate) request_revision: u64,
    pub(crate) requested: RateTuple,
    pub(crate) effect: RateEffect,
    pub(crate) master_provenance: MasterProvenance,
}

pub(crate) struct GlobalRateStatusSlot {
    requested: RateRecord,
    effect: RateRecord,
    bpm_requested: BpmRecord,
    bpm_effect: BpmRecord,
}

impl Default for GlobalRateStatusSlot {
    fn default() -> Self {
        let requested = RateRecord::default();
        let initial = RateEffect {
            revision: 0,
            requested: RateTuple::default(),
            effective: RateTuple::default(),
            state: RateState::Applied,
            execution_frame: None,
            error: RateError::None,
        };
        requested.publish(initial, MasterProvenance::Absent);
        let effect = RateRecord::default();
        effect.publish(initial, MasterProvenance::Absent);
        let bpm_requested = BpmRecord::default();
        let bpm_effect = BpmRecord::default();
        let initial_bpm = BpmEffect {
            revision: 0,
            requested: false,
            effective: false,
            applied: true,
            reason: BpmReason::None,
            execution_frame: None,
        };
        bpm_requested.publish(initial_bpm);
        bpm_effect.publish(initial_bpm);
        Self {
            requested,
            effect,
            bpm_requested,
            bpm_effect,
        }
    }
}

impl GlobalRateStatusSlot {
    /// Control only, serialized by the existing parameter producer lock.
    pub(crate) fn publish_requested(&self, revision: u64, speed: Option<f64>, period: Option<f64>) {
        let Some((_, previous, _)) = self.requested.read() else {
            return;
        };
        let base = self
            .effect
            .read()
            .map(|(_, effect, _)| effect)
            .filter(|effect| {
                effect.revision == previous.revision
                    && matches!(effect.state, RateState::Applied | RateState::Rejected)
            })
            .map_or(previous.effective, |effect| effect.effective);
        let requested = RateTuple {
            speed: speed.unwrap_or(base.speed),
            period: period.or(base.period),
        };
        self.requested.publish(
            RateEffect {
                revision,
                requested,
                effective: requested,
                state: RateState::Preparing,
                execution_frame: None,
                error: RateError::None,
            },
            MasterProvenance::Absent,
        );
    }

    /// Sole callback writer. Publishing never allocates, blocks or retries.
    #[cfg(test)]
    pub(crate) fn publish_effect(&self, effect: RateEffect) {
        self.publish_effect_with_master(effect, None);
    }

    /// The callback replaces provenance only after an actual master publication.
    pub(crate) fn publish_effect_with_master(
        &self,
        effect: RateEffect,
        replacement: Option<MasterProvenance>,
    ) {
        if let Some((_, _, previous)) = self.effect.read() {
            self.effect.publish(effect, replacement.unwrap_or(previous));
        }
    }

    /// Existing callback controls may change the tuple without a new speed intent.
    /// Keep that actual effect visible while retaining the original receipt identity.
    #[cfg(test)]
    pub(crate) fn refresh_effective(&self, effective: RateTuple) {
        self.refresh_effective_with_master(effective, None);
    }

    pub(crate) fn refresh_effective_with_master(
        &self,
        effective: RateTuple,
        replacement: Option<MasterProvenance>,
    ) {
        if let Some((_, previous, provenance)) = self.effect.read()
            && (previous.effective != effective
                || replacement.is_some_and(|value| value != provenance))
        {
            self.effect.publish(
                RateEffect {
                    effective,
                    ..previous
                },
                replacement.unwrap_or(provenance),
            );
        }
    }

    pub(crate) fn requested_revision(&self) -> u64 {
        self.requested.revision.load(Ordering::SeqCst)
    }

    /// Readers do one bounded snapshot and reject overlapping writers.
    pub(crate) fn read(&self) -> Option<GlobalRateStatus> {
        let (sequence, requested, _) = self.requested.read()?;
        let (_, effect, master_provenance) = self.effect.read()?;
        (effect.revision <= requested.revision
            && self.requested.sequence.load(Ordering::SeqCst) == sequence)
            .then_some(GlobalRateStatus {
                request_revision: requested.revision,
                requested: if requested.revision == effect.revision {
                    effect.requested
                } else {
                    requested.effective
                },
                effect,
                master_provenance,
            })
    }
}

/// Scalar BPM intent on the existing control/callback status channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BpmReason {
    #[default]
    None,
    RatePending,
    DrainPending,
    HistoryPending,
    ReferenceChanged,
}
impl BpmReason {
    pub(crate) fn name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::RatePending => Some("rate_pending"),
            Self::DrainPending => Some("drain_pending"),
            Self::HistoryPending => Some("history_pending"),
            Self::ReferenceChanged => Some("reference_changed"),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BpmEffect {
    pub(crate) revision: u64,
    pub(crate) requested: bool,
    pub(crate) effective: bool,
    pub(crate) applied: bool,
    pub(crate) reason: BpmReason,
    pub(crate) execution_frame: Option<u64>,
}

#[derive(Default)]
struct BpmRecord {
    sequence: AtomicU64,
    revision: AtomicU64,
    flags: AtomicU8,
    reason: AtomicU8,
    execution: AtomicU64,
}
impl BpmRecord {
    fn publish(&self, value: BpmEffect) {
        let sequence = self.sequence.load(Ordering::SeqCst);
        let Some(next) = sequence.checked_add(2).filter(|_| sequence & 1 == 0) else {
            self.sequence.store(1, Ordering::SeqCst);
            return;
        };
        self.sequence.store(next - 1, Ordering::SeqCst);
        self.revision.store(value.revision, Ordering::SeqCst);
        self.flags.store(
            u8::from(value.requested)
                | (u8::from(value.effective) << 1)
                | (u8::from(value.applied) << 2),
            Ordering::SeqCst,
        );
        self.reason.store(value.reason as u8, Ordering::SeqCst);
        self.execution
            .store(value.execution_frame.unwrap_or(u64::MAX), Ordering::SeqCst);
        self.sequence.store(next, Ordering::SeqCst);
    }
    fn read(&self) -> Option<(u64, BpmEffect)> {
        let sequence = self.sequence.load(Ordering::SeqCst);
        if sequence == 0 || sequence & 1 != 0 {
            return None;
        }
        let flags = self.flags.load(Ordering::SeqCst);
        let execution = self.execution.load(Ordering::SeqCst);
        let reason = match self.reason.load(Ordering::SeqCst) {
            0 => BpmReason::None,
            1 => BpmReason::RatePending,
            2 => BpmReason::DrainPending,
            3 => BpmReason::HistoryPending,
            _ => BpmReason::ReferenceChanged,
        };
        let value = BpmEffect {
            revision: self.revision.load(Ordering::SeqCst),
            requested: flags & 1 != 0,
            effective: flags & 2 != 0,
            applied: flags & 4 != 0,
            reason,
            execution_frame: (execution != u64::MAX).then_some(execution),
        };
        (self.sequence.load(Ordering::SeqCst) == sequence).then_some((sequence, value))
    }
}
impl GlobalRateStatusSlot {
    /// Control writer only, under the original command producer lock.
    pub(crate) fn publish_bpm_requested(&self, revision: u64, enabled: bool) {
        self.bpm_requested.publish(BpmEffect {
            revision,
            requested: enabled,
            effective: false,
            applied: false,
            reason: BpmReason::None,
            execution_frame: None,
        });
    }
    /// Sole callback writer; no allocation, queue or new counter.
    pub(crate) fn publish_bpm_effect(&self, effect: BpmEffect) {
        self.bpm_effect.publish(effect);
    }
    pub(crate) fn read_bpm(&self) -> Option<(u64, BpmEffect)> {
        let (sequence, requested) = self.bpm_requested.read()?;
        let (_, effect) = self.bpm_effect.read()?;
        if effect.revision > requested.revision
            || self.bpm_requested.sequence.load(Ordering::SeqCst) != sequence
        {
            return None;
        }
        Some((
            requested.revision,
            BpmEffect {
                requested: requested.requested,
                ..effect
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(slot: &GlobalRateStatusSlot, state: RateState, effective: RateTuple) {
        slot.publish_effect(RateEffect {
            revision: 1,
            requested: RateTuple {
                speed: 1.8,
                period: Some(0.625),
            },
            effective,
            state,
            execution_frame: Some(41),
            error: if state == RateState::Rejected {
                RateError::HistoryUnavailable
            } else {
                RateError::None
            },
        });
    }

    #[test]
    fn queued_speed_after_matching_applied_off_uses_the_actual_absent_period() {
        let slot = GlobalRateStatusSlot::default();
        slot.publish_requested(1, Some(1.8), Some(0.625));
        effect(
            &slot,
            RateState::Applied,
            RateTuple {
                speed: 1.8,
                period: Some(0.625),
            },
        );
        slot.refresh_effective(RateTuple {
            speed: 1.8,
            period: None,
        });
        slot.publish_requested(2, Some(1.6), None);
        let status = slot.read().unwrap();
        assert_eq!(
            status.requested,
            RateTuple {
                speed: 1.6,
                period: None
            }
        );
        assert_eq!(status.effect.revision, 1);
        assert_eq!(status.effect.requested.period, Some(0.625));
        assert_eq!(status.effect.execution_frame, Some(41));
    }

    #[test]
    fn queued_master_after_matching_applied_refresh_uses_actual_speed_and_explicit_override() {
        let slot = GlobalRateStatusSlot::default();
        slot.publish_requested(1, Some(1.8), Some(0.625));
        effect(
            &slot,
            RateState::Applied,
            RateTuple {
                speed: 1.7,
                period: Some(0.75),
            },
        );
        slot.publish_requested(2, Some(1.6), None);
        assert_eq!(
            slot.read().unwrap().requested,
            RateTuple {
                speed: 1.6,
                period: Some(0.75)
            }
        );
        // A subsequent unseen prefix still owns its explicit speed.
        slot.publish_requested(3, None, Some(0.875));
        assert_eq!(
            slot.read().unwrap().requested,
            RateTuple {
                speed: 1.6,
                period: Some(0.875)
            }
        );
        let explicit = GlobalRateStatusSlot::default();
        explicit.publish_requested(1, Some(1.8), Some(0.625));
        effect(
            &explicit,
            RateState::Applied,
            RateTuple {
                speed: 1.7,
                period: Some(0.75),
            },
        );
        explicit.publish_requested(2, None, Some(0.875));
        assert_eq!(
            explicit.read().unwrap().requested,
            RateTuple {
                speed: 1.7,
                period: Some(0.875)
            }
        );
    }

    #[test]
    fn matching_rejected_request_coalesces_from_actual_tuple() {
        let slot = GlobalRateStatusSlot::default();
        slot.publish_requested(1, Some(1.8), Some(0.625));
        effect(
            &slot,
            RateState::Rejected,
            RateTuple {
                speed: 1.0,
                period: None,
            },
        );
        slot.publish_requested(2, None, Some(0.75));
        assert_eq!(
            slot.read().unwrap().requested,
            RateTuple {
                speed: 1.0,
                period: Some(0.75)
            }
        );
    }

    #[test]
    fn preparing_and_newer_unobserved_prefixes_keep_their_requested_coalescing_base() {
        let slot = GlobalRateStatusSlot::default();
        slot.publish_requested(1, Some(1.8), Some(0.625));
        effect(
            &slot,
            RateState::Preparing,
            RateTuple {
                speed: 1.0,
                period: None,
            },
        );
        slot.publish_requested(2, Some(1.6), None);
        assert_eq!(
            slot.read().unwrap().requested,
            RateTuple {
                speed: 1.6,
                period: Some(0.625)
            }
        );
        effect(
            &slot,
            RateState::Applied,
            RateTuple {
                speed: 1.0,
                period: None,
            },
        );
        slot.publish_requested(3, None, Some(0.75));
        assert_eq!(
            slot.read().unwrap().requested,
            RateTuple {
                speed: 1.6,
                period: Some(0.75)
            }
        );
        // An overlapping effect read also preserves the latest requested prefix.
        slot.effect.sequence.store(1, Ordering::SeqCst);
        slot.publish_requested(4, Some(1.4), None);
        assert_eq!(
            slot.requested.read().unwrap().1.effective,
            RateTuple {
                speed: 1.4,
                period: Some(0.75)
            }
        );
    }

    #[test]
    fn actual_provenance_is_not_replaced_by_requested_preparing_or_rejected_receipts() {
        let slot = GlobalRateStatusSlot::default();
        let actual = RateTuple {
            speed: 1.0,
            period: Some(0.5),
        };
        let accepted = MasterProvenance::Qualified {
            source_identity: [0x53; 32],
            accepted_revision: Some([0xa7; 32]),
        };
        slot.refresh_effective_with_master(actual, Some(accepted));
        for state in [RateState::Preparing, RateState::Rejected] {
            slot.publish_requested(1, Some(1.8), Some(0.625));
            assert_eq!(slot.read().unwrap().master_provenance, accepted);
            effect(&slot, state, actual);
            let status = slot.read().unwrap();
            assert_eq!(status.master_provenance, accepted);
            assert_eq!(status.effect.effective, actual);
        }
        // A later applied speed-only receipt and an ordinary tuple observation
        // still describe the same last actual qualified master publication.
        effect(
            &slot,
            RateState::Applied,
            RateTuple {
                speed: 1.8,
                ..actual
            },
        );
        slot.refresh_effective(RateTuple {
            speed: 1.6,
            ..actual
        });
        assert_eq!(slot.read().unwrap().master_provenance, accepted);
    }

    #[test]
    fn same_tuple_manual_unknown_and_absent_provenance_are_distinct_whole_images() {
        let slot = GlobalRateStatusSlot::default();
        let actual = RateTuple {
            speed: 1.0,
            period: Some(0.5),
        };
        let source_identity = std::array::from_fn(|index| index as u8);
        let accepted = MasterProvenance::Qualified {
            source_identity,
            accepted_revision: Some(std::array::from_fn(|index| (255 - index) as u8)),
        };
        slot.refresh_effective_with_master(actual, Some(accepted));
        let accepted_sequence = slot.effect.sequence.load(Ordering::SeqCst);
        let manual = MasterProvenance::Qualified {
            source_identity,
            accepted_revision: None,
        };
        slot.refresh_effective_with_master(actual, Some(manual));
        assert!(slot.effect.sequence.load(Ordering::SeqCst) > accepted_sequence);
        assert_eq!(slot.read().unwrap().master_provenance, manual);
        slot.refresh_effective_with_master(actual, Some(MasterProvenance::Unknown));
        let unknown = slot.read().unwrap();
        assert_eq!(unknown.master_provenance, MasterProvenance::Unknown);
        assert_eq!(unknown.effect.effective, actual);
        slot.refresh_effective_with_master(RateTuple::default(), Some(MasterProvenance::Absent));
        let absent = slot.read().unwrap();
        assert_eq!(absent.master_provenance, MasterProvenance::Absent);
        assert_eq!(absent.effect.effective.period, None);
    }

    #[test]
    fn overlapping_or_exhausted_provenance_publication_cannot_return_a_mixed_image() {
        let slot = GlobalRateStatusSlot::default();
        slot.effect.sequence.store(3, Ordering::SeqCst);
        slot.effect.master_kind.store(2, Ordering::SeqCst);
        slot.effect.master_source[0].store(u64::MAX, Ordering::SeqCst);
        assert!(slot.read().is_none());
        slot.effect.sequence.store(u64::MAX - 1, Ordering::SeqCst);
        slot.refresh_effective_with_master(
            RateTuple {
                speed: 1.0,
                period: Some(0.5),
            },
            Some(MasterProvenance::Unknown),
        );
        assert!(slot.read().is_none());
    }
}
