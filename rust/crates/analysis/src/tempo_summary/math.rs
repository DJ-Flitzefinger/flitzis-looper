//! Centered deterministic numerical fitting with finite work and no randomness.

const SAMPLE_POSITIONS: usize = 65;
const REFIT_PASSES: usize = 3;

#[derive(Clone, Copy)]
pub(super) struct Point {
    pub raw_index: usize,
    pub quarter: i64,
    pub seconds: f64,
}

#[derive(Clone, Copy)]
pub(super) struct Line {
    pub reference: i64,
    pub mean_quarters: f64,
    pub mean_seconds: f64,
    pub period: f64,
    pub sensitivity: f64,
}

impl Line {
    pub fn residual(self, point: Point) -> f64 {
        point.seconds
            - (self.mean_seconds
                + self.period * ((point.quarter - self.reference) as f64 - self.mean_quarters))
    }

    pub fn seconds_at(self, quarter: i64) -> f64 {
        self.mean_seconds + self.period * ((quarter - self.reference) as f64 - self.mean_quarters)
    }
}

pub(super) fn median(values: &mut [f64]) -> f64 {
    values.sort_unstable_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        values[middle - 1] * 0.5 + values[middle] * 0.5
    } else {
        values[middle]
    }
}

fn sum(values: impl Iterator<Item = f64>) -> f64 {
    let mut total = 0.0;
    let mut correction = 0.0;
    for value in values {
        let adjusted = value - correction;
        let next = total + adjusted;
        correction = (next - total) - adjusted;
        total = next;
    }
    total
}

fn least_squares(points: &[Point], halfwidth: f64) -> Option<Line> {
    let first = *points.first()?;
    if points.len() < 2 {
        return None;
    }
    let count = points.len() as f64;
    let mean_quarters = sum(points.iter().map(|p| (p.quarter - first.quarter) as f64)) / count;
    let mean_offset = sum(points.iter().map(|p| p.seconds - first.seconds)) / count;
    let coordinates = || {
        points.iter().map(|point| {
            (
                (point.quarter - first.quarter) as f64 - mean_quarters,
                (point.seconds - first.seconds) - mean_offset,
            )
        })
    };
    let squares = sum(coordinates().map(|(quarter, _)| quarter * quarter));
    let period = sum(coordinates().map(|(quarter, seconds)| quarter * seconds)) / squares;
    let sensitivity = halfwidth * sum(coordinates().map(|(quarter, _)| quarter.abs())) / squares;
    if squares <= 0.0 || !period.is_finite() || period <= 0.0 || !sensitivity.is_finite() {
        return None;
    }
    Some(Line {
        reference: first.quarter,
        mean_quarters,
        mean_seconds: first.seconds + mean_offset,
        period,
        sensitivity,
    })
}

fn seed_line(points: &[Point]) -> Option<Line> {
    if points.len() < 6 {
        return None;
    }
    let sample_count = points.len().min(SAMPLE_POSITIONS);
    let sample: Vec<_> = (0..sample_count)
        .map(|index| points[index * (points.len() - 1) / (sample_count - 1)])
        .collect();
    let mut slopes = Vec::with_capacity(sample_count * (sample_count - 1) / 2);
    for (index, first) in sample.iter().enumerate() {
        for last in &sample[index + 1..] {
            slopes.push((last.seconds - first.seconds) / (last.quarter - first.quarter) as f64);
        }
    }
    let period = median(&mut slopes);
    if !period.is_finite() || period <= 0.0 {
        return None;
    }
    let reference = points[0].quarter;
    let mut intercepts: Vec<_> = points
        .iter()
        .map(|point| point.seconds - period * (point.quarter - reference) as f64)
        .collect();
    Some(Line {
        reference,
        mean_quarters: 0.0,
        mean_seconds: median(&mut intercepts),
        period,
        sensitivity: 0.0,
    })
}

/// Seed from a bounded pair sample, then refit at most three times on all evidence.
pub(super) fn robust_line(
    seed: &[Point],
    points: &[Point],
    threshold: f64,
    halfwidth: f64,
) -> Option<(Line, Vec<Point>)> {
    let mut line = seed_line(seed)?;
    for _ in 0..REFIT_PASSES - 1 {
        let inliers: Vec<_> = points
            .iter()
            .copied()
            .filter(|point| line.residual(*point).abs() <= threshold)
            .collect();
        if inliers.len() < 6 {
            return None;
        }
        line = least_squares(&inliers, halfwidth)?;
    }
    let inliers: Vec<_> = points
        .iter()
        .copied()
        .filter(|point| line.residual(*point).abs() <= threshold)
        .collect();
    if inliers.len() < 6 {
        return None;
    }
    // The returned sensitivity must describe exactly the returned inlier set.
    // A final membership change is left unsupported rather than iterating forever.
    let final_line = least_squares(&inliers, halfwidth)?;
    if points.iter().any(|point| {
        (line.residual(*point).abs() <= threshold)
            != (final_line.residual(*point).abs() <= threshold)
    }) {
        return None;
    }
    Some((final_line, inliers))
}

/// Test existence of an affine line compatible with every retained timing bound.
///
/// The intercept-strip width is convex in slope. Its active extremes supply a
/// subgradient, so a fixed 64-step bisection over the conditional OLS slope bound
/// suffices up to the separately reported numerical tolerance. This does not
/// enlarge the declared timing bound to fit alternating or correlated variation.
pub(super) fn feasible_timing_bound(
    points: &[Point],
    line: Line,
    halfwidth: f64,
    numerical_tolerance: f64,
) -> bool {
    let span = (points.last().expect("nonempty inliers").quarter - points[0].quarter) as f64;
    let numerical_slope = 4.0 * numerical_tolerance / span;
    let mut lower = line.period - line.sensitivity - numerical_slope;
    let mut upper = line.period + line.sensitivity + numerical_slope;
    for _ in 0..64 {
        let period = lower * 0.5 + upper * 0.5;
        let mut low = (f64::INFINITY, 0_i64);
        let mut high = (f64::NEG_INFINITY, 0_i64);
        for point in points {
            let relative_count = point.quarter - line.reference;
            let intercept = (point.seconds - line.mean_seconds)
                - period * (relative_count as f64 - line.mean_quarters);
            if intercept < low.0 {
                low = (intercept, relative_count);
            }
            if intercept > high.0 {
                high = (intercept, relative_count);
            }
        }
        if high.0 - low.0 <= 2.0 * halfwidth + numerical_tolerance {
            return true;
        }
        if high.1 > low.1 {
            lower = period;
        } else if high.1 < low.1 {
            upper = period;
        } else {
            return false;
        }
    }
    false
}
