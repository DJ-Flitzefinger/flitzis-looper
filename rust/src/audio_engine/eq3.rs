use std::f32::consts::PI;

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

#[derive(Clone, Copy)]
pub struct Eq3Coeffs {
    pub low: BiquadCoeffs,
    pub mid: BiquadCoeffs,
    pub high: BiquadCoeffs,
}

impl Eq3Coeffs {
    pub fn identity() -> Self {
        Self {
            low: BiquadCoeffs::identity(),
            mid: BiquadCoeffs::identity(),
            high: BiquadCoeffs::identity(),
        }
    }

    pub fn process(&self, state: &mut Eq3State, mut x: f32) -> f32 {
        x = biquad_process(self.low, &mut state.low, x);
        x = biquad_process(self.mid, &mut state.mid, x);
        x = biquad_process(self.high, &mut state.high, x);
        x
    }
}

#[derive(Clone, Copy, Default)]
pub struct Eq3State {
    low: BiquadState,
    mid: BiquadState,
    high: BiquadState,
}

impl Eq3State {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

fn db_to_a(db: f32) -> f32 {
    if !db.is_finite() {
        return 1.0;
    }
    10.0_f32.powf(db / 40.0)
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

fn biquad_low_shelf(fs_hz: f32, freq_hz: f32, db_gain: f32) -> BiquadCoeffs {
    let freq_hz = clamp_freq_hz(fs_hz, freq_hz);
    let a = db_to_a(db_gain);
    let w0 = 2.0 * PI * freq_hz / fs_hz;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / 2.0 * 2.0_f32.sqrt();

    let sqrt_a = a.sqrt();

    let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
    let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
    let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
    let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
    let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
    let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;

    normalize_biquad(b0, b1, b2, a0, a1, a2)
}

fn biquad_high_shelf(fs_hz: f32, freq_hz: f32, db_gain: f32) -> BiquadCoeffs {
    let freq_hz = clamp_freq_hz(fs_hz, freq_hz);
    let a = db_to_a(db_gain);
    let w0 = 2.0 * PI * freq_hz / fs_hz;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / 2.0 * 2.0_f32.sqrt();

    let sqrt_a = a.sqrt();

    let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
    let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
    let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
    let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
    let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
    let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;

    normalize_biquad(b0, b1, b2, a0, a1, a2)
}

fn biquad_peaking(fs_hz: f32, freq_hz: f32, q: f32, db_gain: f32) -> BiquadCoeffs {
    let freq_hz = clamp_freq_hz(fs_hz, freq_hz);
    let a = db_to_a(db_gain);
    let q = if q.is_finite() && q > 0.0 { q } else { 0.707 };

    let w0 = 2.0 * PI * freq_hz / fs_hz;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q);

    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cos_w0;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha / a;

    normalize_biquad(b0, b1, b2, a0, a1, a2)
}

pub fn coeffs_for_eq3(fs_hz: f32, low_db: f32, mid_db: f32, high_db: f32) -> Eq3Coeffs {
    if !fs_hz.is_finite() || fs_hz <= 0.0 {
        return Eq3Coeffs::identity();
    }

    Eq3Coeffs {
        low: biquad_low_shelf(fs_hz, 250.0, low_db),
        mid: biquad_peaking(fs_hz, 1_000.0, 0.5, mid_db),
        high: biquad_high_shelf(fs_hz, 3_000.0, high_db),
    }
}
