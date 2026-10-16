//! Bounded observation of requested global rate and its separate callback effect.
//! Neither record grants source authority. Each record has exactly one writer.

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
        }
    }
}

impl RateRecord {
    fn publish(&self, value: RateEffect) {
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
        self.sequence.store(next, Ordering::SeqCst);
    }

    fn read(&self) -> Option<(u64, RateEffect)> {
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
        (self.sequence.load(Ordering::SeqCst) == sequence).then_some((sequence, value))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct GlobalRateStatus {
    pub(crate) request_revision: u64,
    pub(crate) requested: RateTuple,
    pub(crate) effect: RateEffect,
}

pub(crate) struct GlobalRateStatusSlot {
    requested: RateRecord,
    effect: RateRecord,
}

impl Default for GlobalRateStatusSlot {
    fn default() -> Self {
        let requested = RateRecord::default();
        requested.publish(RateEffect {
            revision: 0,
            requested: RateTuple::default(),
            effective: RateTuple::default(),
            state: RateState::Applied,
            execution_frame: None,
            error: RateError::None,
        });
        Self {
            requested,
            effect: RateRecord::default(),
        }
    }
}

impl GlobalRateStatusSlot {
    /// Control only, serialized by the existing parameter producer lock.
    pub(crate) fn publish_requested(&self, revision: u64, speed: Option<f64>, period: Option<f64>) {
        let Some((_, previous)) = self.requested.read() else {
            return;
        };
        let requested = RateTuple {
            speed: speed.unwrap_or(previous.effective.speed),
            period: period.or(previous.effective.period),
        };
        self.requested.publish(RateEffect {
            revision,
            requested,
            effective: requested,
            state: RateState::Preparing,
            execution_frame: None,
            error: RateError::None,
        });
    }

    /// Sole callback writer. Publishing never allocates, blocks or retries.
    pub(crate) fn publish_effect(&self, effect: RateEffect) {
        self.effect.publish(effect);
    }

    /// Existing callback controls may change the tuple without a new speed intent.
    /// Keep that actual effect visible while retaining the original receipt identity.
    pub(crate) fn refresh_effective(&self, effective: RateTuple) {
        if let Some((_, previous)) = self.effect.read()
            && previous.effective != effective
        {
            self.effect.publish(RateEffect {
                effective,
                ..previous
            });
        }
    }

    pub(crate) fn requested_revision(&self) -> u64 {
        self.requested.revision.load(Ordering::SeqCst)
    }

    /// Readers do one bounded snapshot and reject overlapping writers.
    pub(crate) fn read(&self) -> Option<GlobalRateStatus> {
        let (sequence, requested) = self.requested.read()?;
        let (_, effect) = self.effect.read()?;
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
            })
    }
}
