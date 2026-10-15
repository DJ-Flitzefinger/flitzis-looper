//! Validated musical intent, independent of the source clock and native readiness.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PitchIntent {
    base_shift: i8,
    extra_shift: i8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum PitchIntentError {
    #[error("base_shift must be an integer between -5 and 6")]
    InvalidBaseShift,
    #[error("extra_shift must be an integer between -18 and 18")]
    InvalidExtraShift,
    #[error("source rate must be finite and greater than zero")]
    InvalidSourceRate,
    #[error("combined native pitch scale must be finite and greater than zero")]
    InvalidNativePitchScale,
}

impl PitchIntent {
    pub(crate) fn new(base_shift: i32, extra_shift: i32) -> Result<Self, PitchIntentError> {
        if !(-5..=6).contains(&base_shift) {
            return Err(PitchIntentError::InvalidBaseShift);
        }
        if !(-18..=18).contains(&extra_shift) {
            return Err(PitchIntentError::InvalidExtraShift);
        }
        Ok(Self {
            base_shift: base_shift as i8,
            extra_shift: extra_shift as i8,
        })
    }

    pub(crate) const fn base_shift(self) -> i32 {
        self.base_shift as i32
    }

    pub(crate) const fn extra_shift(self) -> i32 {
        self.extra_shift as i32
    }

    pub(crate) const fn total_semitones(self) -> i32 {
        self.base_shift as i32 + self.extra_shift as i32
    }

    pub(crate) fn frequency_ratio(self) -> f64 {
        2.0_f64.powf(f64::from(self.total_semitones()) / 12.0)
    }

    /// One pitch pass over the existing output-domain varispeed feed.
    ///
    /// This validates the requested numeric domain; successful arithmetic alone says nothing
    /// about source-specific preparation or the backend's measured musical quality.
    pub(crate) fn native_pitch_scale(
        self,
        source_rate: f64,
        key_lock: bool,
    ) -> Result<f64, PitchIntentError> {
        if !source_rate.is_finite() || source_rate <= 0.0 {
            return Err(PitchIntentError::InvalidSourceRate);
        }
        let ratio = self.frequency_ratio();
        let pitch = if key_lock { ratio / source_rate } else { ratio };
        if !pitch.is_finite() || pitch <= 0.0 {
            return Err(PitchIntentError::InvalidNativePitchScale);
        }
        Ok(pitch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_domain_is_exact_and_never_clamps() {
        for base in -5..=6 {
            for extra in -18..=18 {
                let intent = PitchIntent::new(base, extra).unwrap();
                assert_eq!(intent.base_shift(), base);
                assert_eq!(intent.extra_shift(), extra);
                assert_eq!(intent.total_semitones(), base + extra);
            }
        }
        for base in [i32::MIN, -6, 7, i32::MAX] {
            assert_eq!(
                PitchIntent::new(base, 0),
                Err(PitchIntentError::InvalidBaseShift)
            );
        }
        for extra in [i32::MIN, -19, 19, i32::MAX] {
            assert_eq!(
                PitchIntent::new(0, extra),
                Err(PitchIntentError::InvalidExtraShift)
            );
        }
        assert_eq!(PitchIntent::new(-5, -18).unwrap().total_semitones(), -23);
        assert_eq!(PitchIntent::new(6, 18).unwrap().total_semitones(), 24);
    }

    #[test]
    fn pitch_and_source_rate_are_independent() {
        for rate in [0.5, 1.0, 2.0] {
            for intent in [
                PitchIntent::default(),
                PitchIntent::new(-5, -18).unwrap(),
                PitchIntent::new(6, 18).unwrap(),
            ] {
                let h = intent.frequency_ratio();
                assert_eq!(intent.native_pitch_scale(rate, true).unwrap(), h / rate);
                assert_eq!(intent.native_pitch_scale(rate, false).unwrap(), h);
            }
        }
        assert_eq!(
            PitchIntent::new(6, 18)
                .unwrap()
                .native_pitch_scale(0.5, true)
                .unwrap(),
            8.0
        );
        assert_eq!(
            PitchIntent::new(0, 12)
                .unwrap()
                .native_pitch_scale(2.0, true)
                .unwrap(),
            1.0
        );
        for rate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                PitchIntent::default().native_pitch_scale(rate, true),
                Err(PitchIntentError::InvalidSourceRate)
            );
        }
    }
}
