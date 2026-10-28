//! Copied effective physical or accepted constant-tempo reader geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceReaderPeriod {
    Physical,
    ScalarMusical(u64),
}
impl SourceReaderPeriod {
    pub(crate) fn from_scalar_period(period: Option<f64>) -> Self {
        period.map_or(Self::Physical, |period| {
            Self::ScalarMusical(period.to_bits())
        })
    }
    pub(crate) fn scalar_period(self) -> Option<f64> {
        match self {
            Self::Physical => None,
            Self::ScalarMusical(bits) => Some(f64::from_bits(bits)),
        }
    }
}
