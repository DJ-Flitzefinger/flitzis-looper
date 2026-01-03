use std::f32::consts::PI;

use crate::audio_engine::constants::PAD_EQ_DB_MIN;

const LOW_MID_CROSSOVER_HZ: f32 = 380.0;
const MID_HIGH_CROSSOVER_HZ: f32 = 2_300.0;

const BUTTERWORTH_Q: f32 = 0.70710677;

#[derive(Clone, Copy)]
pub struct BiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl BiquadCoeffs {
    fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

fn biquad_process(coeffs: BiquadCoeffs, state: &mut BiquadState, x: f32) -> f32 {
    let y = coeffs.b0 * x + state.z1;
    state.z1 = coeffs.b1 * x - coeffs.a1 * y + state.z2;
    state.z2 = coeffs.b2 * x - coeffs.a2 * y;
    y
}

fn clamp_freq_hz(fs_hz: f32, freq_hz: f32) -> f32 {
    if !fs_hz.is_finite() || fs_hz <= 0.0 {
        return freq_hz.max(1.0);
    }

    let nyquist = fs_hz * 0.5;
    let max_hz = (nyquist * 0.9).max(1.0);
    freq_hz.clamp(1.0, max_hz)
}

fn normalize_biquad(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> BiquadCoeffs {
    if !a0.is_finite() || a0.abs() < 1e-12 {
        return BiquadCoeffs::identity();
    }

    let inv_a0 = 1.0 / a0;
    let coeffs = BiquadCoeffs {
        b0: b0 * inv_a0,
        b1: b1 * inv_a0,
        b2: b2 * inv_a0,
        a1: a1 * inv_a0,
        a2: a2 * inv_a0,
    };

    if [coeffs.b0, coeffs.b1, coeffs.b2, coeffs.a1, coeffs.a2]
        .iter()
        .all(|v| v.is_finite())
    {
        coeffs
    } else {
        BiquadCoeffs::identity()
    }
}

fn biquad_low_pass_butterworth(fs_hz: f32, freq_hz: f32) -> BiquadCoeffs {
    let freq_hz = clamp_freq_hz(fs_hz, freq_hz);
    let w0 = 2.0 * PI * freq_hz / fs_hz;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * BUTTERWORTH_Q);

    let b0 = (1.0 - cos_w0) * 0.5;
    let b1 = 1.0 - cos_w0;
    let b2 = (1.0 - cos_w0) * 0.5;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;

    normalize_biquad(b0, b1, b2, a0, a1, a2)
}

fn biquad_high_pass_butterworth(fs_hz: f32, freq_hz: f32) -> BiquadCoeffs {
    let freq_hz = clamp_freq_hz(fs_hz, freq_hz);
    let w0 = 2.0 * PI * freq_hz / fs_hz;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * BUTTERWORTH_Q);

    let b0 = (1.0 + cos_w0) * 0.5;
    let b1 = -(1.0 + cos_w0);
    let b2 = (1.0 + cos_w0) * 0.5;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;

    normalize_biquad(b0, b1, b2, a0, a1, a2)
}

fn db_to_linear_gain(db: f32) -> f32 {
    if !db.is_finite() {
        return 1.0;
    }

    if db <= PAD_EQ_DB_MIN {
        return 0.0;
    }

    10.0_f32.powf(db / 20.0)
}

#[derive(Clone, Copy)]
pub struct Eq3Coeffs {
    pub low_lp: [BiquadCoeffs; 2],
    pub high_hp: [BiquadCoeffs; 2],
    pub low_gain: f32,
    pub mid_gain: f32,
    pub high_gain: f32,
}

impl Eq3Coeffs {
    pub fn identity() -> Self {
        Self {
            low_lp: [BiquadCoeffs::identity(); 2],
            high_hp: [BiquadCoeffs::identity(); 2],
            low_gain: 1.0,
            mid_gain: 1.0,
            high_gain: 1.0,
        }
    }

    pub fn process(&self, state: &mut Eq3State, x: f32) -> f32 {
        let mut low = x;
        for (coeffs, stage) in self.low_lp.iter().zip(state.low_lp.iter_mut()) {
            low = biquad_process(*coeffs, stage, low);
        }

        let mut high = x;
        for (coeffs, stage) in self.high_hp.iter().zip(state.high_hp.iter_mut()) {
            high = biquad_process(*coeffs, stage, high);
        }

        let mid = x - low - high;

        low * self.low_gain + mid * self.mid_gain + high * self.high_gain
    }
}

#[derive(Clone, Copy, Default)]
pub struct Eq3State {
    low_lp: [BiquadState; 2],
    high_hp: [BiquadState; 2],
}

impl Eq3State {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub fn coeffs_for_eq3(fs_hz: f32, low_db: f32, mid_db: f32, high_db: f32) -> Eq3Coeffs {
    if !fs_hz.is_finite() || fs_hz <= 0.0 {
        return Eq3Coeffs::identity();
    }

    let lp = biquad_low_pass_butterworth(fs_hz, LOW_MID_CROSSOVER_HZ);
    let hp = biquad_high_pass_butterworth(fs_hz, MID_HIGH_CROSSOVER_HZ);

    Eq3Coeffs {
        low_lp: [lp; 2],
        high_hp: [hp; 2],
        low_gain: db_to_linear_gain(low_db),
        mid_gain: db_to_linear_gain(mid_db),
        high_gain: db_to_linear_gain(high_db),
    }
}
