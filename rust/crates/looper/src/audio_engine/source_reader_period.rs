//! Copied effective reader geometry, independent of its source and map owners.

use super::general_source_numerics::{MappedReaderPeriod, PhysicalMapQuarters};
use super::source_reader::FrameRange;

/// Physical, legacy musical and exact mapped circles remain distinct contracts.
/// A mapped certificate is only arithmetic; it grants no source or history permit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceReaderPeriod {
    Physical,
    ScalarMusical(u64),
    MappedRational(MappedReaderPeriod),
    /// Exact quarter extent Q, not a source-frame period. Reader P is the
    /// associated domain's physical H; complete identity still retains Q.
    MappedPhysicalQuarters(PhysicalMapQuarters),
}

impl SourceReaderPeriod {
    pub(crate) fn from_scalar_period(period: Option<f64>) -> Self {
        period.map_or(Self::Physical, |period| {
            Self::ScalarMusical(period.to_bits())
        })
    }

    /// Only the legacy scalar class has an optional binary64 musical period.
    /// Callers must reject the mapped class before using a physical default.
    pub(crate) fn scalar_period(self) -> Option<f64> {
        match self {
            Self::ScalarMusical(bits) => Some(f64::from_bits(bits)),
            Self::Physical | Self::MappedRational(_) | Self::MappedPhysicalQuarters(_) => None,
        }
    }

    pub(crate) fn is_mapped(self) -> bool {
        matches!(
            self,
            Self::MappedRational(_) | Self::MappedPhysicalQuarters(_)
        )
    }

    /// Resolve the effective source-frame reader period, preserving Q's separate
    /// quarter-note units. A failed mapped resolution never means legacy Physical.
    pub(crate) fn mapped_period(self, region: FrameRange) -> Option<MappedReaderPeriod> {
        match self {
            Self::MappedRational(period) => Some(period),
            Self::MappedPhysicalQuarters(quarters) if quarters.is_valid() => {
                MappedReaderPeriod::physical(region.len())
            }
            Self::MappedPhysicalQuarters(_) => None,
            Self::Physical | Self::ScalarMusical(_) => None,
        }
    }

    pub(crate) fn physical_quarters(self) -> Option<PhysicalMapQuarters> {
        match self {
            Self::MappedPhysicalQuarters(quarters) => Some(quarters),
            Self::Physical | Self::ScalarMusical(_) | Self::MappedRational(_) => None,
        }
    }
}
