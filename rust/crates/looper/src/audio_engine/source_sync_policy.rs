//! Captured timing intent and scalar arithmetic, never a source or clock authority.

use super::constants::{SPEED_MAX, SPEED_MIN};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SourceSyncMode {
    #[default]
    Legacy,
    Off,
    Fixed,
    Mapped,
}

impl SourceSyncMode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Off => "off",
            Self::Fixed => "fixed",
            Self::Mapped => "mapped",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceSyncRequest {
    pub(crate) mode: SourceSyncMode,
    pub(crate) global_revision: Option<u64>,
    pub(crate) intent_revision: u64,
    pub(crate) policy_sync: Option<bool>,
    pub(crate) policy_realtime_bpm: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceSyncObservation {
    pub(crate) request: SourceSyncRequest,
    pub(crate) target_current: bool,
    pub(crate) settled: bool,
    /// Exact installed phase custody for service suppression, never a phase/read ACK.
    pub(crate) phase_program_current: bool,
    pub(crate) phase_program_revision: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GlobalTimingPolicy {
    sync: Option<bool>,
    realtime_bpm: Option<bool>,
    revision: u64,
}

// Wrapping Option makes each nullable field REQUIRED to serde, unlike a missing
// Option field which would otherwise silently deserialize as None.
struct Choice(Option<bool>);

impl<'de> Deserialize<'de> for Choice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RequiredChoice;
        impl Visitor<'_> for RequiredChoice {
            type Value = Choice;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a required boolean or null")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Choice, E> {
                Ok(Choice(Some(value)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Choice, E> {
                Ok(Choice(None))
            }
        }
        deserializer.deserialize_any(RequiredChoice)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema_version: u32,
    sync: Choice,
    realtime_bpm: Choice,
    revision: u64,
}

impl GlobalTimingPolicy {
    pub(crate) fn parse(encoded: Option<&str>) -> Result<Self, String> {
        let Some(encoded) = encoded else {
            return Ok(Self::default());
        };
        if encoded.is_empty() || encoded.len() > 1024 {
            return Err("global timing policy exceeds its 1024-byte extent".into());
        }
        let wire: Wire = serde_json::from_str(encoded).map_err(|error| error.to_string())?;
        if wire.schema_version != 1 {
            return Err("unsupported global timing policy schema".into());
        }
        Ok(Self {
            sync: wire.sync.0,
            realtime_bpm: wire.realtime_bpm.0,
            revision: wire.revision,
        })
    }

    pub(crate) fn request(self, requested: bool, intent_revision: u64) -> SourceSyncRequest {
        let mode = if self.sync == Some(false) || (self.sync == Some(true) && !requested) {
            SourceSyncMode::Off
        } else if !requested {
            SourceSyncMode::Legacy
        } else if self.realtime_bpm == Some(false) {
            SourceSyncMode::Fixed
        } else {
            SourceSyncMode::Mapped
        };
        SourceSyncRequest {
            mode,
            global_revision: (self != Self::default()).then_some(self.revision),
            intent_revision,
            policy_sync: self.sync,
            policy_realtime_bpm: self.realtime_bpm,
        }
    }
}

impl SourceSyncRequest {
    /// Nullable pre-control mapped choices retain their original creative offset.
    /// The actual parent projection and claimant separately qualify availability.
    pub(crate) fn requires_common_phase(self) -> bool {
        self.mode == SourceSyncMode::Fixed
            || (self.mode == SourceSyncMode::Mapped
                && (self.policy_sync == Some(true) || self.policy_realtime_bpm == Some(true)))
    }

    pub(crate) fn mapped_matching_enabled(self, legacy_bpm_lock: bool) -> bool {
        legacy_bpm_lock
    }
    /// Preserve the existing scalar clock and glide. Explicit Fixed never calls
    /// a clamped approximation a source/master match.
    pub(crate) fn scalar_ratio(
        self,
        source_period: Option<f64>,
        speed: f64,
        master_period: Option<f64>,
        legacy_bpm_lock: bool,
    ) -> Option<f64> {
        if matches!(self.mode, SourceSyncMode::Fixed | SourceSyncMode::Mapped) && !legacy_bpm_lock {
            return None;
        }
        if self.mode == SourceSyncMode::Fixed {
            let source = source_period.filter(|value| value.is_finite() && *value > 0.0)?;
            let master = master_period.filter(|value| value.is_finite() && *value > 0.0)?;
            let ratio = source / master;
            return (ratio.is_finite() && (SPEED_MIN..=SPEED_MAX).contains(&ratio))
                .then_some(ratio);
        }
        let mut ratio = speed;
        if self.mode != SourceSyncMode::Off
            && self.mapped_matching_enabled(legacy_bpm_lock)
            && let (Some(master), Some(source)) = (master_period, source_period)
        {
            ratio = source / master;
        }
        if !ratio.is_finite() {
            ratio = 1.0;
        }
        Some(ratio.clamp(SPEED_MIN, SPEED_MAX))
    }
}

#[cfg(test)]
mod tests;
