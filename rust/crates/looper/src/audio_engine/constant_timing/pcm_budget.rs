//! Explicit non-realtime PCM admission policy, independent of timing acceptance.

use super::MAX_PCM_BYTES;

const MAX_EXPLICIT_PCM_BYTES: usize = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PcmBudget(usize);

impl Default for PcmBudget {
    fn default() -> Self {
        Self(MAX_PCM_BYTES)
    }
}

impl PcmBudget {
    pub(super) fn new(limit_bytes: usize) -> Result<Self, String> {
        if limit_bytes == 0 || limit_bytes > MAX_EXPLICIT_PCM_BYTES {
            return Err("constant timing PCM limit must be in 1..=1073741824 bytes".into());
        }
        Ok(Self(limit_bytes))
    }

    pub(super) fn limit_bytes(self) -> usize {
        self.0
    }

    pub(super) fn check_loaded_geometry(
        self,
        sample_count: usize,
        channels: usize,
        sample_rate_hz: u32,
    ) -> Result<(), String> {
        if pcm_peak_bytes(sample_count, channels, sample_rate_hz)? > self.0 as u128 {
            return Err("constant timing PCM byte limit exceeded".into());
        }
        Ok(())
    }
}

// Admission estimates complete loaded f32 PCM, mono plus its conversion input,
// and converted f32/f64 PCM. Actual FFT output capacity is checked separately
// before allocation and again before the analyzer conversion; this estimate is
// not a promise that a budget equal to it includes resampler padding.
fn pcm_peak_bytes(
    sample_count: usize,
    channels: usize,
    sample_rate_hz: u32,
) -> Result<u128, String> {
    if sample_rate_hz == 0
        || !(1..=32).contains(&channels)
        || sample_count == 0
        || !sample_count.is_multiple_of(channels)
    {
        return Err("invalid constant timing loaded PCM geometry".into());
    }
    let frames = sample_count / channels;
    let converted_frames = (frames as u128 * 44_100).div_ceil(u128::from(sample_rate_hz));
    Ok(sample_count as u128 * 4 + frames as u128 * 8 + converted_frames * 12)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_pcm_limit_preserves_default_and_rejects_zero_or_above_hard_ceiling() {
        assert_eq!(PcmBudget::default().limit_bytes(), 512 * 1024 * 1024);
        assert_eq!(PcmBudget::new(1).unwrap().limit_bytes(), 1);
        assert_eq!(
            PcmBudget::new(MAX_EXPLICIT_PCM_BYTES)
                .unwrap()
                .limit_bytes(),
            MAX_EXPLICIT_PCM_BYTES
        );
        assert!(PcmBudget::new(0).is_err());
        assert!(PcmBudget::new(MAX_EXPLICIT_PCM_BYTES + 1).is_err());
        assert!(PcmBudget::new(usize::MAX).is_err());
    }

    #[test]
    fn complete_fixture_geometry_requires_explicit_budget_without_allocating_pcm() {
        let frames = 600 * 48_000;
        assert_eq!(pcm_peak_bytes(frames, 1, 48_000).unwrap(), 663_120_000);
        assert_eq!(pcm_peak_bytes(frames * 2, 2, 48_000).unwrap(), 778_320_000);
        let explicit = PcmBudget::new(MAX_EXPLICIT_PCM_BYTES).unwrap();
        for channels in [1, 2] {
            assert!(
                PcmBudget::default()
                    .check_loaded_geometry(frames * channels, channels, 48_000)
                    .is_err()
            );
            explicit
                .check_loaded_geometry(frames * channels, channels, 48_000)
                .unwrap();
        }
        assert!(
            explicit
                .check_loaded_geometry(frames * 32, 32, 48_000)
                .is_err()
        );
    }

    #[test]
    fn geometry_uses_ceiling_conversion_and_rejects_invalid_or_extreme_extent() {
        assert_eq!(pcm_peak_bytes(2, 2, 48_000).unwrap(), 28);
        for (samples, channels, rate) in [
            (0, 1, 48_000),
            (1, 0, 48_000),
            (1, 33, 48_000),
            (3, 2, 48_000),
            (1, 1, 0),
        ] {
            assert!(pcm_peak_bytes(samples, channels, rate).is_err());
        }
        assert!(
            PcmBudget::new(MAX_EXPLICIT_PCM_BYTES)
                .unwrap()
                .check_loaded_geometry(usize::MAX, 1, 1)
                .is_err()
        );
    }
}
