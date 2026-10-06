//! Explicit caller assertions and failure states for offline timing acceptance.

use std::{error::Error, fmt};

use crate::tempo_refinement::TempoRefinementError;
use crate::tempo_summary::{SummaryStatus, TempoSummaryError};

/// Canonical revision domain and encoding version for accepted constant timing.
pub const TIMING_REVISION_VERSION: &str = "accepted-constant-timing-v1";

/// An independently selected grid origin, separate from the fitted intercept.
///
/// Signed finite seconds are valid. The provenance explains this explicit
/// choice; the API checks its shape and cannot prove its musical correctness.
#[derive(Debug, Clone, PartialEq)]
pub struct IndependentTimingOrigin {
    pub seconds: f64,
    pub provenance: String,
}

/// An explicit caller assertion accepting timing under a named policy version.
///
/// A supported fit does not manufacture this decision. There is no default
/// decision or built-in automatic musical acceptance policy in this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimingAcceptanceDecision {
    pub policy_version: String,
    pub provenance: String,
}

/// Invalid assertions or unsupported evidence prevent construction entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TempoAcceptanceError {
    InvalidOrigin,
    InvalidDecision,
    BindingMismatch,
    Summary(TempoSummaryError),
    Refinement(TempoRefinementError),
    CandidateStatus(SummaryStatus),
    UnsupportedRefinement,
    InvalidSupportedFit,
}

impl fmt::Display for TempoAcceptanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOrigin => formatter.write_str("invalid independent timing origin"),
            Self::InvalidDecision => {
                formatter.write_str("invalid explicit timing acceptance decision")
            }
            Self::BindingMismatch => {
                formatter.write_str("accepted timing source/PCM binding mismatch")
            }
            Self::Summary(error) => write!(formatter, "constant timing assessment failed: {error}"),
            Self::Refinement(error) => {
                write!(formatter, "constant timing refinement failed: {error}")
            }
            Self::CandidateStatus(status) => write!(
                formatter,
                "constant timing requires a supported verified candidate, received {status:?}"
            ),
            Self::UnsupportedRefinement => formatter.write_str("PCM refinement is unsupported"),
            Self::InvalidSupportedFit => {
                formatter.write_str("invalid supported constant-period fit")
            }
        }
    }
}

impl Error for TempoAcceptanceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Summary(error) => Some(error),
            Self::Refinement(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TempoSummaryError> for TempoAcceptanceError {
    fn from(error: TempoSummaryError) -> Self {
        Self::Summary(error)
    }
}

impl From<TempoRefinementError> for TempoAcceptanceError {
    fn from(error: TempoRefinementError) -> Self {
        Self::Refinement(error)
    }
}
