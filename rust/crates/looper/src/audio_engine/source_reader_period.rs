//! Copied effective reader geometry, independent of its source and map owners.

use super::general_source_numerics::MappedReaderPeriod;

/// Physical, legacy musical and exact mapped circles remain distinct contracts.
/// A mapped certificate is only arithmetic; it grants no source or history permit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceReaderPeriod {
    Physical,
    ScalarMusical(u64),
    MappedRational(MappedReaderPeriod),
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
            Self::Physical | Self::MappedRational(_) => None,
        }
    }

    pub(crate) fn is_mapped(self) -> bool {
        matches!(self, Self::MappedRational(_))
    }

    pub(crate) fn mapped_period(self) -> Option<MappedReaderPeriod> {
        match self {
            Self::MappedRational(period) => Some(period),
            Self::Physical | Self::ScalarMusical(_) => None,
        }
    }
}
