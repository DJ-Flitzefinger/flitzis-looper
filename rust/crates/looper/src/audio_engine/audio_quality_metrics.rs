//! Shared fixed-criterion offline audio metrics for test-only probes.

pub(super) const TIMING_BUDGET_MS: f64 = 2.0;
pub(super) const MAX_DISCARDED_ENERGY: f64 = 0.001;
pub(super) const MAX_CAPTURE_TAIL_ENERGY: f64 = 0.000_001;

pub(super) struct Response {
    pub(super) peak_frame: usize,
    pub(super) peak: f32,
    pub(super) onset: Option<usize>,
    pub(super) significant_onset: Option<usize>,
}

pub(super) fn response(samples: &[f32]) -> Response {
    assert!(!samples.is_empty());
    assert!(samples.iter().all(|sample| sample.is_finite()));
    // Select the first maximum, including a flat retained/clipped response.
    let mut peak_frame = 0;
    let mut peak = 0.0_f32;
    for (frame, sample) in samples.iter().enumerate() {
        if sample.abs() > peak {
            peak_frame = frame;
            peak = sample.abs();
        }
    }
    let detected = peak > 1.0e-7;
    Response {
        peak_frame,
        peak,
        onset: detected.then(|| {
            samples
                .iter()
                .position(|sample| sample.abs() > 1.0e-7)
                .unwrap()
        }),
        significant_onset: detected.then(|| {
            samples
                .iter()
                .position(|sample| sample.abs() >= peak * 0.01)
                .unwrap()
        }),
    }
}

pub(super) fn stereo_peak(channels: &[Vec<f32>]) -> (usize, f32) {
    let mut result = (0, 0.0_f32);
    for (frame, (left, right)) in channels[0].iter().zip(&channels[1]).enumerate() {
        let value = left.abs().max(right.abs());
        if value > result.1 {
            result = (frame, value);
        }
    }
    result
}

pub(super) fn stereo_response(channels: &[Vec<f32>]) -> Response {
    let magnitudes = channels[0]
        .iter()
        .zip(&channels[1])
        .map(|(left, right)| left.abs().max(right.abs()))
        .collect::<Vec<_>>();
    response(&magnitudes)
}

pub(super) fn stereo_energy(channels: &[Vec<f32>]) -> Vec<f64> {
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0].len(), channels[1].len());
    channels[0]
        .iter()
        .zip(&channels[1])
        .map(|(left, right)| f64::from(*left).powi(2) + f64::from(*right).powi(2))
        .collect()
}

pub(super) fn energy_time(energy: &[f64], fraction: f64) -> usize {
    let total = energy.iter().sum::<f64>();
    assert!(total.is_finite() && total > 0.0);
    let target = total * fraction;
    let mut cumulative = 0.0;
    for (frame, value) in energy.iter().enumerate() {
        cumulative += value;
        if cumulative >= target {
            return frame;
        }
    }
    energy.len() - 1
}

/// Centered 0.5-ms box energy envelope. Its origin is the original sample timeline, with zero
/// extension at capture edges, so no causal-window latency is added to the reported times.
pub(super) fn envelope_energy(energy: &[f64], rate: u32) -> Vec<f64> {
    let half_window = (f64::from(rate) * 0.000_25).round() as usize;
    let mut prefix = Vec::with_capacity(energy.len() + 1);
    prefix.push(0.0);
    for value in energy {
        prefix.push(prefix.last().unwrap() + value);
    }
    // Include negative/after-capture envelope centers. Truncation at zero would lose half
    // the envelope energy of a marker at zero and bias the reference quantiles.
    (0..energy.len() + 2 * half_window)
        .map(|frame| {
            let start = frame.saturating_sub(2 * half_window).min(energy.len());
            let end = (frame + 1).min(energy.len());
            (prefix[end] - prefix[start]) / (2 * half_window + 1) as f64
        })
        .collect()
}

pub(super) fn energy_times(energy: &[f64], rate: u32) -> [i64; 3] {
    let envelope = envelope_energy(energy, rate);
    let half_window = (f64::from(rate) * 0.000_25).round() as i64;
    [0.1, 0.5, 0.9].map(|fraction| energy_time(&envelope, fraction) as i64 - half_window)
}

/// Largest raw discard D for which energy in [0,D) is within the declared budget.
pub(super) fn maximum_energy_discard(energy: &[f64]) -> usize {
    let budget = energy.iter().sum::<f64>() * MAX_DISCARDED_ENERGY;
    let mut cumulative = 0.0;
    for (frame, value) in energy.iter().enumerate() {
        cumulative += value;
        if cumulative > budget {
            return frame;
        }
    }
    energy.len()
}
