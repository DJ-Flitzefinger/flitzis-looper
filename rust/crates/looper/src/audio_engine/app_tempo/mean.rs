//! Independent approximate interior pulse consensus, never a constant-phase claim.
use super::{RESIDUAL_SECONDS, RegionEstimate, median};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Cancelled,
    Allocation,
    Budget,
}

fn check(cancelled: &dyn Fn() -> bool) -> Result<(), Failure> {
    if cancelled() {
        Err(Failure::Cancelled)
    } else {
        Ok(())
    }
}

fn collect<T>(
    count: usize,
    values: impl Iterator<Item = T>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<T>, Failure> {
    select(count, values.map(Some), cancelled)
}

fn select<T>(
    count: usize,
    values: impl Iterator<Item = Option<T>>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<T>, Failure> {
    check(cancelled)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| Failure::Allocation)?;
    if result.capacity() > count {
        return Err(Failure::Allocation);
    }
    for (i, value) in values.enumerate() {
        if i.is_multiple_of(1024) {
            check(cancelled)?;
        }
        if let Some(value) = value {
            if result.len() == count {
                return Err(Failure::Allocation);
            }
            result.push(value);
        }
    }
    check(cancelled)?;
    Ok(result)
}

pub(super) fn line(
    points: &[(usize, f64)],
    keep: &[bool],
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<(f64, f64, f64)>, Failure> {
    check(cancelled)?;
    let mut count = 0;
    for (i, &yes) in keep.iter().enumerate() {
        if i.is_multiple_of(1024) {
            check(cancelled)?;
        }
        count += usize::from(yes);
    }
    if count < 2 {
        return Ok(None);
    }
    let mut mx = 0.0;
    let mut my = 0.0;
    // Keep the original per-component source-order sums and centered fit.
    for (i, (point, k)) in points.iter().zip(keep).enumerate() {
        if i.is_multiple_of(1024) {
            check(cancelled)?;
        }
        if *k {
            mx += point.0 as f64;
        }
    }
    mx /= count as f64;
    for (i, (point, k)) in points.iter().zip(keep).enumerate() {
        if i.is_multiple_of(1024) {
            check(cancelled)?;
        }
        if *k {
            my += point.1;
        }
    }
    my /= count as f64;
    let (mut xx, mut xy) = (0.0, 0.0);
    for (i, (point, k)) in points.iter().zip(keep).enumerate() {
        if i.is_multiple_of(1024) {
            check(cancelled)?;
        }
        if *k {
            let x = point.0 as f64 - mx;
            xx += x * x;
            xy += x * (point.1 - my);
        }
    }
    let period = xy / xx;
    Ok((period.is_finite() && (0.2..=2.0).contains(&period)).then_some((mx, my, period)))
}

/// Shared with the established v4 fit. Numeric operations, masks and guards are
/// unchanged; allocation and long scans now have explicit failure boundaries.
pub(super) fn region_checked(
    beats: &[f64],
    lower: f64,
    upper: f64,
    minimum_points: usize,
    minimum_span: f64,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<RegionEstimate>, Failure> {
    let mut points = collect(beats.len(), std::iter::empty(), cancelled)?;
    for (i, &t) in beats.iter().enumerate() {
        if i.is_multiple_of(1024) {
            check(cancelled)?;
        }
        if t >= lower && t < upper {
            points.push((i, t));
        }
    }
    if points.len() < minimum_points {
        return Ok(None);
    }
    let mut keep = collect(
        points.len(),
        std::iter::repeat_n(true, points.len()),
        cancelled,
    )?;
    for _ in 0..5 {
        let Some((mx, my, period)) = line(&points, &keep, cancelled)? else {
            return Ok(None);
        };
        let residual = collect(
            points.len(),
            points
                .iter()
                .map(|&(i, t)| t - my - period * (i as f64 - mx)),
            cancelled,
        )?;
        let mut retained = select(
            points.len(),
            residual.iter().zip(&keep).map(|(r, k)| (*k).then_some(*r)),
            cancelled,
        )?;
        let center = median(&mut retained);
        let mut deviations = collect(
            retained.len(),
            retained.iter().map(|r| (r - center).abs()),
            cancelled,
        )?;
        let limit = RESIDUAL_SECONDS.min(0.05 * period);
        let threshold = (3.5 * 1.4826 * median(&mut deviations)).clamp(0.010, limit);
        let next = collect(
            points.len(),
            residual.iter().map(|r| (r - center).abs() <= threshold),
            cancelled,
        )?;
        if next == keep {
            break;
        }
        keep = next;
    }
    let excluded = select(
        points.len(),
        points.iter().zip(&keep).map(|(p, k)| (!*k).then_some(p.0)),
        cancelled,
    )?;
    if excluded.len() * 20 > points.len() || keep.windows(2).any(|k| !k[0] && !k[1]) {
        return Ok(None);
    }
    let retained = select(
        points.len(),
        points.iter().zip(&keep).map(|(p, k)| (*k).then_some(*p)),
        cancelled,
    )?;
    if retained.len() < minimum_points {
        return Ok(None);
    }
    let first = retained.first().unwrap().1;
    let last = retained.last().unwrap().1;
    if last - first < minimum_span
        || last - first < 0.6 * (upper - lower)
        || first > lower + 0.2 * (upper - lower)
        || last < upper - 0.2 * (upper - lower)
    {
        return Ok(None);
    }
    let Some((mx, my, period)) = line(&points, &keep, cancelled)? else {
        return Ok(None);
    };
    let residual = collect(
        retained.len(),
        retained
            .iter()
            .map(|&(i, t)| t - my - period * (i as f64 - mx)),
        cancelled,
    )?;
    let maximum = residual.iter().copied().map(f64::abs).fold(0.0, f64::max);
    if maximum > RESIDUAL_SECONDS.min(0.05 * period) {
        return Ok(None);
    }
    check(cancelled)?;
    Ok(Some(RegionEstimate {
        window_seconds: [lower, upper],
        bpm: 60.0 / period,
        retained: retained.len(),
        excluded_raw_indices: excluded,
        max_residual_seconds: maximum,
        rms_residual_seconds: (residual.iter().map(|r| r * r).sum::<f64>() / residual.len() as f64)
            .sqrt(),
    }))
}

#[derive(Debug, Serialize)]
pub(crate) struct InteriorMean {
    pub regions: Vec<RegionEstimate>,
    pub bpm: f64,
}

impl InteriorMean {
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        self.regions.iter().try_fold(
            self.regions
                .capacity()
                .checked_mul(size_of::<RegionEstimate>())?,
            |n, r| {
                n.checked_add(
                    r.excluded_raw_indices
                        .capacity()
                        .checked_mul(size_of::<usize>())?,
                )
            },
        )
    }
}

pub(crate) fn preparation_bytes(count: usize) -> Option<usize> {
    count.checked_mul(64)?.checked_add(1024 * 1024)
}

pub(crate) fn interior(
    beats: &[f64],
    duration: f64,
    available: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<InteriorMean>, Failure> {
    check(cancelled)?;
    if preparation_bytes(beats.len()).is_none_or(|n| n > available) {
        return Err(Failure::Budget);
    }
    if !duration.is_finite() || duration <= 0.0 || beats.len() > 250_000 {
        return Ok(None);
    }
    for (i, &t) in beats.iter().enumerate() {
        if i.is_multiple_of(1024) {
            check(cancelled)?;
        }
        if !t.is_finite() || t < 0.0 || t >= duration || (i > 0 && beats[i - 1] >= t) {
            return Ok(None);
        }
    }
    let mut regions = collect(3, std::iter::empty(), cancelled)?;
    let (mut numerator, mut denominator) = (0.0, 0.0);
    for i in 1..4 {
        let lower = i as f64 * duration / 5.0;
        let upper = (i + 1) as f64 * duration / 5.0;
        let Some(region) = region_checked(beats, lower, upper, 24, 30.0, cancelled)? else {
            return Ok(None);
        };
        numerator += (upper - lower) * region.bpm;
        denominator += upper - lower;
        regions.push(region);
    }
    let bpm = numerator / denominator;
    check(cancelled)?;
    Ok((bpm.is_finite() && (30.0..=300.0).contains(&bpm)).then_some(InteriorMean { regions, bpm }))
}

pub(crate) fn agreement(neural: &InteriorMean, qm: &InteriorMean) -> Option<(f64, f64)> {
    if neural.regions.len() != 3 || qm.regions.len() != 3 {
        return None;
    }
    let mut selected = None;
    for factor in [0.5, 1.0, 2.0] {
        let mut periods = [0.0_f64; 6];
        let mut valid = true;
        for i in 0..3 {
            let a = neural.regions[i].bpm;
            let b = factor * qm.regions[i].bpm;
            valid &= a.is_finite()
                && b.is_finite()
                && (30.0..=300.0).contains(&a)
                && (30.0..=300.0).contains(&b);
            periods[i] = 60.0 / a;
            periods[i + 3] = 60.0 / b;
        }
        let adjusted = factor * qm.bpm;
        valid &= neural.bpm.is_finite()
            && adjusted.is_finite()
            && (30.0..=300.0).contains(&neural.bpm)
            && (30.0..=300.0).contains(&adjusted);
        let ratio = periods.into_iter().fold(f64::NEG_INFINITY, f64::max)
            / periods.into_iter().fold(f64::INFINITY, f64::min);
        if valid && ratio <= 1.01 {
            if selected.is_some() {
                return None;
            }
            selected = Some((factor, ratio));
        }
    }
    selected
}

#[cfg(test)]
#[path = "mean_tests.rs"]
mod tests;
