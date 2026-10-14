//! Control-only accepted timing ownership; no engine or callback publication.

use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::tempo_evidence::{PcmBinding, PcmBindingMetadata};

use super::AcceptedConstantTiming;

static NEXT_GUARD_ID: AtomicU64 = AtomicU64::new(1);

/// Explicit timing authority. Values and their runtime publication stay with the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingIntent {
    Automatic,
    Manual,
    Tap,
    Legacy,
}

/// Opaque request token, valid only for the issuing guard's current source and intent.
#[derive(Debug, Clone)]
pub struct TimingAdoptionTicket {
    guard_id: u64,
    revision: u64,
}

/// Non-realtime current-source/request/intent ownership foundation.
///
/// A future live owner must serialize actual engine source and timing-intent
/// changes through this guard. This isolated API does not inspect an engine or
/// prove that caller-supplied current metadata is fresh. It never owns PCM.
#[derive(Debug)]
pub struct TimingAdoptionGuard {
    guard_id: u64,
    revision: u64,
    binding: Option<PcmBindingMetadata>,
    intent: TimingIntent,
    accepted: Option<AcceptedConstantTiming>,
}

/// Fully checked adoption held under the caller's exclusive guard borrow.
/// Dropping it leaves the guard unchanged; commit only swaps prepared owners.
pub struct PreparedTimingAdoption<'guard> {
    guard: &'guard mut TimingAdoptionGuard,
    next_revision: u64,
    timing: AcceptedConstantTiming,
}

impl PreparedTimingAdoption<'_> {
    /// Commit after the caller claims its publication boundary. The displaced
    /// record is returned so its potentially large evidence retires after that boundary.
    pub fn commit(self) -> Option<AcceptedConstantTiming> {
        self.guard.revision = self.next_revision;
        self.guard.accepted.replace(self.timing)
    }
}

impl TimingAdoptionGuard {
    /// Retain verified metadata for one source under the caller's explicit intent.
    pub fn new(
        binding: &PcmBinding<'_>,
        intent: TimingIntent,
    ) -> Result<Self, TimingAdoptionError> {
        let guard_id = NEXT_GUARD_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| TimingAdoptionError::RevisionExhausted)?;
        Ok(Self {
            guard_id,
            revision: 1,
            binding: Some(binding.metadata().clone()),
            intent,
            accepted: None,
        })
    }

    /// Admit a new automatic request and invalidate every earlier ticket.
    ///
    /// The last accepted record remains available until another valid result is
    /// adopted or source/intent ownership changes. Admission alone is not adoption.
    pub fn issue_ticket(&mut self) -> Result<TimingAdoptionTicket, TimingAdoptionError> {
        if self.binding.is_none() {
            return Err(TimingAdoptionError::NoSource);
        }
        if self.intent != TimingIntent::Automatic {
            return Err(TimingAdoptionError::IntentNotAutomatic);
        }
        self.advance()?;
        Ok(TimingAdoptionTicket {
            guard_id: self.guard_id,
            revision: self.revision,
        })
    }

    /// Publish new intent, invalidating old work even when the enum value is unchanged.
    ///
    /// A manual value edit, TAP or legacy restore must call this method even when
    /// an earlier value used the same authority. No automatic ticket can revive.
    pub fn set_intent(&mut self, intent: TimingIntent) -> Result<(), TimingAdoptionError> {
        self.advance()?;
        self.intent = intent;
        self.accepted = None;
        Ok(())
    }

    /// Replace the verified current source/request binding and explicit timing intent.
    pub fn replace_source(
        &mut self,
        binding: &PcmBinding<'_>,
        intent: TimingIntent,
    ) -> Result<(), TimingAdoptionError> {
        // Prepare any allocation before changing the guard's current state.
        let metadata = binding.metadata().clone();
        self.advance()?;
        self.binding = Some(metadata);
        self.intent = intent;
        self.accepted = None;
        Ok(())
    }

    /// Retire current timing authority after the caller unloads its source.
    pub fn unload(&mut self) -> Result<(), TimingAdoptionError> {
        self.advance()?;
        self.binding = None;
        self.accepted = None;
        Ok(())
    }

    /// Adopt only a matching current result; every failed check preserves guard state.
    ///
    /// The successful transition consumes the ticket's revision, so reusing a
    /// cloned token cannot publish twice. Large retained records retire here on
    /// the non-realtime caller, never on the audio callback.
    pub fn adopt(
        &mut self,
        ticket: &TimingAdoptionTicket,
        timing: AcceptedConstantTiming,
    ) -> Result<(), TimingAdoptionError> {
        drop(self.prepare_adoption(ticket, timing)?.commit());
        Ok(())
    }

    /// Check a complete adoption before an external publication claim, without
    /// advancing the guard or consuming the ticket until the prepared commit.
    pub fn prepare_adoption(
        &mut self,
        ticket: &TimingAdoptionTicket,
        timing: AcceptedConstantTiming,
    ) -> Result<PreparedTimingAdoption<'_>, TimingAdoptionError> {
        self.prepare_adoption_inner(ticket, timing, false)
    }

    /// Adopt a source-verified saved record under this fresh guard's request.
    ///
    /// Only historical job tokens differ. Complete content, extent, rate, zero,
    /// mono rule and provenance must match the verified current binding. The
    /// original accepted revision remains unchanged and never supplies freshness.
    pub fn adopt_source_verified(
        &mut self,
        ticket: &TimingAdoptionTicket,
        timing: AcceptedConstantTiming,
    ) -> Result<(), TimingAdoptionError> {
        drop(
            self.prepare_source_verified_adoption(ticket, timing)?
                .commit(),
        );
        Ok(())
    }

    /// Prepare the same source-verified saved-record transition before the
    /// caller's claim, preserving the historical accepted revision unchanged.
    pub fn prepare_source_verified_adoption(
        &mut self,
        ticket: &TimingAdoptionTicket,
        timing: AcceptedConstantTiming,
    ) -> Result<PreparedTimingAdoption<'_>, TimingAdoptionError> {
        self.prepare_adoption_inner(ticket, timing, true)
    }

    fn prepare_adoption_inner(
        &mut self,
        ticket: &TimingAdoptionTicket,
        timing: AcceptedConstantTiming,
        source_verified: bool,
    ) -> Result<PreparedTimingAdoption<'_>, TimingAdoptionError> {
        if ticket.guard_id != self.guard_id || ticket.revision != self.revision {
            return Err(TimingAdoptionError::StaleTicket);
        }
        if self.intent != TimingIntent::Automatic {
            return Err(TimingAdoptionError::IntentNotAutomatic);
        }
        let binding = self.binding.as_ref().ok_or(TimingAdoptionError::NoSource)?;
        if source_verified {
            timing.check_source_binding(binding)
        } else {
            timing.check_binding(binding)
        }
        .map_err(|_| TimingAdoptionError::BindingMismatch)?;
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(TimingAdoptionError::RevisionExhausted)?;
        Ok(PreparedTimingAdoption {
            guard: self,
            next_revision,
            timing,
        })
    }

    /// Borrow the currently accepted record without allowing identity mutation.
    pub fn accepted(&self) -> Option<&AcceptedConstantTiming> {
        self.accepted.as_ref()
    }

    /// Inspect current explicit authority, independently of record availability.
    pub fn intent(&self) -> TimingIntent {
        self.intent
    }

    fn advance(&mut self) -> Result<(), TimingAdoptionError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(TimingAdoptionError::RevisionExhausted)?;
        Ok(())
    }
}

/// Rejected control transition; no accepted state was published by that transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingAdoptionError {
    NoSource,
    IntentNotAutomatic,
    StaleTicket,
    BindingMismatch,
    RevisionExhausted,
}

impl fmt::Display for TimingAdoptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NoSource => "no current timing source",
            Self::IntentNotAutomatic => "current timing intent is not automatic",
            Self::StaleTicket => "stale or foreign timing adoption ticket",
            Self::BindingMismatch => {
                "accepted timing does not match current source/request binding"
            }
            Self::RevisionExhausted => "timing adoption revision exhausted",
        })
    }
}

impl Error for TimingAdoptionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_exhaustion_preserves_all_guard_state() {
        let mut guard = TimingAdoptionGuard {
            guard_id: 1,
            revision: u64::MAX,
            binding: None,
            intent: TimingIntent::Manual,
            accepted: None,
        };
        assert_eq!(
            guard.set_intent(TimingIntent::Automatic),
            Err(TimingAdoptionError::RevisionExhausted)
        );
        assert_eq!(guard.intent(), TimingIntent::Manual);
        assert_eq!(guard.unload(), Err(TimingAdoptionError::RevisionExhausted));
        assert_eq!(guard.revision, u64::MAX);
        assert!(guard.binding.is_none());
        assert!(guard.accepted().is_none());
    }
}
