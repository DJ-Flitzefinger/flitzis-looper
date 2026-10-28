//! Bounded scalar estimates with explicit inferred ordinals, never playback authority.
use super::integer::FractionalCheck;
use serde::Serialize;

pub(super) const POLICY: &str = "neural-inferred-ordinals-v5";
const MAX_COARSE_SEEDS: usize = 4096;
// Duplicates spend original-emission fuel; refinement can emit one extra row.
const ORIGINAL_FUEL: usize = 4096;
const CENSUS_KEYS: usize = 2 * ORIGINAL_FUEL;
const SEARCH_FITS: usize = MAX_COARSE_SEEDS;
const MAX_FITS: usize = SEARCH_FITS + 2;
const FIXED_RESERVE: usize = 8 * 1024 * 1024;
const MAX_COARSE_FAMILIES: usize = 256;
const SAMPLE: usize = 64;
const PHASE_BINS: usize = 128;
const SECTIONS: usize = 5;
const MIN_PERIOD: f64 = 0.2;
const MAX_PERIOD: f64 = 2.0;
const EXACT_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum OrdinalFit {
    Budget(BudgetUnavailable),
    Evaluated(Evidence),
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct BudgetUnavailable {
    pub outcome: &'static str,
    pub required_bytes: usize,
    pub available_bytes: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Evidence {
    pub outcome: &'static str,
    pub evidence_tier: &'static str,
    pub candidate_limit_reached: bool,
    pub evaluated_candidate_count: usize,
    pub winner: Option<CandidateSummary>,
    pub assignments: Vec<(usize, i64)>,
    pub alternatives: Vec<CandidateSummary>,
    pub probe_windows: Vec<[f64; 2]>,
    pub fractional_check: Option<FractionalCheck>,
    pub integer_check: Option<FractionalCheck>,
    pub integer_candidate_bpm: Option<f64>,
    pub integer_preferred: bool,
    pub conflicting_sections: Vec<usize>,
    pub harmonics: Vec<HarmonicOutcome>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct HarmonicOutcome {
    pub factor: f64,
    pub seed_period_seconds: f64,
    pub seed_anchor_seconds: f64,
    pub outcome: &'static str,
    pub alternative_index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct CandidateSummary {
    pub period_seconds: f64,
    pub anchor_seconds: f64,
    pub pivot_ordinal: f64,
    pub pivot_seconds: f64,
    pub global: SpanDiagnostics,
    pub sections: Vec<SpanDiagnostics>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct SpanDiagnostics {
    pub window_seconds: [f64; 2],
    pub raw_count: usize,
    pub expected_slots: usize,
    pub matched: usize,
    pub observed_span_seconds: Option<f64>,
    pub fitted_bpm: Option<f64>,
    pub max_residual_seconds: Option<f64>,
    pub rms_residual_seconds: Option<f64>,
    pub max_gap_seconds: f64,
}

impl SpanDiagnostics {
    fn score(&self) -> f64 {
        let denominator = self.raw_count as f64 + self.expected_slots as f64;
        if denominator == 0.0 {
            0.0
        } else {
            2.0 * self.matched as f64 / denominator
        }
    }

    fn band(&self) -> (f64, f64) {
        let denominator = self.raw_count as f64 + self.expected_slots as f64;
        if denominator == 0.0 {
            return (0.0, 0.0);
        }
        let q = (self.matched as f64).sqrt().ceil() as usize;
        (
            2.0 * self.matched.saturating_sub(q) as f64 / denominator,
            2.0 * self
                .matched
                .saturating_add(q)
                .min(self.raw_count)
                .min(self.expected_slots) as f64
                / denominator,
        )
    }

    fn sustained(&self) -> bool {
        self.matched >= 4
            && self.observed_span_seconds.is_some_and(|span| {
                span >= 10.0_f64.min(0.6 * (self.window_seconds[1] - self.window_seconds[0]))
            })
    }
}

#[derive(Clone, Copy, Debug)]
struct Point {
    raw: usize,
    ordinal: i64,
    seconds: f64,
}

#[derive(Clone, Copy)]
struct Seed {
    period: f64,
    anchor: f64,
    matched: usize,
    sections: usize,
    upper: f64,
    local_upper: [f64; SECTIONS],
    local_sustained: [bool; SECTIONS],
    family: usize,
    source_pair: [usize; 2],
    source_count: i64,
    // A transient parent need not be present in the admitted candidate pool.
    refined_from: Option<(u64, u64)>,
}

struct Discovery {
    seeds: Vec<Seed>,
    #[cfg(test)]
    representatives: Vec<usize>,
    limited: bool,
    #[cfg(test)]
    family_count: usize,
}

struct Geometry {
    periods: Vec<ObservedInterval>,
    families: Vec<IntervalFamily>,
    full_counts: [usize; SECTIONS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Visit {
    Continue,
    Restart,
    Limited,
}

struct Census {
    keys: Vec<(u64, u64)>,
    representatives: Vec<Seed>,
    limited: bool,
}

impl Census {
    fn new() -> Result<Self, SearchFailure> {
        Ok(Self {
            keys: allocate(CENSUS_KEYS)?,
            representatives: allocate(MAX_COARSE_FAMILIES)?,
            limited: false,
        })
    }

    fn accept(&mut self, mut seed: Seed) -> Result<Option<Seed>, SearchFailure> {
        let key = (seed.period.to_bits(), seed.anchor.to_bits());
        if self.keys.contains(&key) {
            return Ok(None);
        }
        if self.keys.len() == CENSUS_KEYS {
            self.limited = true;
            return Ok(None);
        }
        self.keys.push(key);
        let family = self
            .representatives
            .iter()
            .position(|old| same_family(old.period, seed.period));
        let family = match family {
            Some(index) => index,
            None => self.representatives.len(),
        };
        if family == MAX_COARSE_FAMILIES {
            self.limited = true;
            return Ok(None);
        }
        seed.family = family;
        if family == self.representatives.len() {
            self.representatives.push(seed);
        } else if seed_better(&seed, &self.representatives[family]) {
            self.representatives[family] = seed;
        }
        Ok(Some(seed))
    }
}

struct ObservedInterval {
    period: f64,
    canonical_period: f64,
    contributor: usize,
}

struct IntervalFamily {
    period: f64,
    contributors: usize,
    lower: f64,
    upper: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchFailure {
    Allocation,
    Cancelled,
}

pub(super) struct Control<'a> {
    probe: &'a dyn Fn() -> bool,
    latched: std::cell::Cell<bool>,
}

impl<'a> Control<'a> {
    pub(super) fn new(probe: &'a dyn Fn() -> bool) -> Self {
        Self {
            probe,
            latched: std::cell::Cell::new(false),
        }
    }

    pub(super) fn cancelled(&self) -> bool {
        if !self.latched.get() && (self.probe)() {
            self.latched.set(true);
        }
        self.latched.get()
    }

    fn check(&self) -> Result<(), SearchFailure> {
        if self.cancelled() {
            Err(SearchFailure::Cancelled)
        } else {
            Ok(())
        }
    }

    fn block(&self, index: usize) -> Result<(), SearchFailure> {
        if index % 1024 == 0 {
            self.check()?;
        }
        Ok(())
    }
}

#[cfg(test)]
fn never_cancel() -> bool {
    false
}

fn required<T>(value: Option<T>) -> Result<T, SearchFailure> {
    value.ok_or(SearchFailure::Allocation)
}

fn allocate<T>(count: usize) -> Result<Vec<T>, SearchFailure> {
    required(reserve(count))
}

pub(super) fn preparation_bytes(beats: usize, downbeats: usize) -> Option<usize> {
    beats
        .checked_mul(1024)?
        .checked_add(downbeats.checked_mul(512)?)?
        .checked_add(8 * 1024 * 1024)
}

pub(super) fn unavailable(required_bytes: usize, available_bytes: usize) -> OrdinalFit {
    OrdinalFit::Budget(BudgetUnavailable {
        outcome: "budget_unavailable",
        required_bytes,
        available_bytes,
    })
}

fn reserve<T>(count: usize) -> Option<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).ok()?;
    if std::mem::size_of::<T>() != 0 && values.capacity() > count {
        return None;
    }
    Some(values)
}

fn limit(period: f64) -> f64 {
    0.025_f64.min(0.05 * period)
}

fn ordinal(value: f64) -> Option<i64> {
    (value.is_finite() && value.abs() <= EXACT_INTEGER).then(|| value.round_ties_even() as i64)
}

fn inside(value: f64, window: [f64; 2]) -> bool {
    value >= window[0] && value < window[1]
}

type FitResult<T> = Result<Option<T>, SearchFailure>;

fn assignment_checked(
    beats: &[f64],
    duration: f64,
    period: f64,
    anchor: f64,
    control: &Control<'_>,
) -> FitResult<Vec<Point>> {
    assignment_iter(
        beats.iter().copied().enumerate(),
        beats.len(),
        duration,
        period,
        anchor,
        control,
    )
}

fn assignment_iter(
    values: impl Iterator<Item = (usize, f64)>,
    count: usize,
    duration: f64,
    period: f64,
    anchor: f64,
    control: &Control<'_>,
) -> FitResult<Vec<Point>> {
    control.check()?;
    if !period.is_finite() || !(MIN_PERIOD..=MAX_PERIOD).contains(&period) || !anchor.is_finite() {
        return Ok(None);
    }
    let mut points: Vec<Point> = allocate(count)?;
    for (position, (raw, seconds)) in values.enumerate() {
        control.block(position)?;
        let Some(k) = ordinal((seconds - anchor) / period) else {
            return Ok(None);
        };
        let slot = anchor + period * k as f64;
        if !inside(slot, [0.0, duration]) || (seconds - slot).abs() > limit(period) {
            continue;
        }
        let next = Point {
            raw,
            ordinal: k,
            seconds,
        };
        if let Some(previous) = points.last_mut().filter(|point| point.ordinal == k) {
            let old_error = (previous.seconds - slot).abs();
            if (seconds - slot).abs() < old_error {
                *previous = next;
            }
        } else {
            points.push(next);
        }
    }
    Ok(Some(points))
}

#[cfg(test)]
fn assignment(beats: &[f64], duration: f64, period: f64, anchor: f64) -> Option<Vec<Point>> {
    assignment_checked(
        beats,
        duration,
        period,
        anchor,
        &Control::new(&never_cancel),
    )
    .ok()
    .flatten()
}

fn line_checked(points: &[Point], control: &Control<'_>) -> FitResult<(f64, f64, f64)> {
    control.check()?;
    if points.len() < 2 {
        return Ok(None);
    }
    let mut mx = 0.0;
    let mut my = 0.0;
    for (index, point) in points.iter().enumerate() {
        control.block(index)?;
        mx += point.ordinal as f64;
        my += point.seconds;
    }
    mx /= points.len() as f64;
    my /= points.len() as f64;
    let mut xx = 0.0;
    let mut xy = 0.0;
    for (index, point) in points.iter().enumerate() {
        control.block(index)?;
        let x = point.ordinal as f64 - mx;
        xx += x * x;
        xy += x * (point.seconds - my);
    }
    let period = xy / xx;
    Ok(
        (xx > 0.0 && period.is_finite() && (MIN_PERIOD..=MAX_PERIOD).contains(&period))
            .then_some((mx, my, period)),
    )
}

#[cfg(test)]
fn line(points: &[Point]) -> Option<(f64, f64, f64)> {
    line_checked(points, &Control::new(&never_cancel)).expect("non-cancelling line fit")
}

fn slots(window: [f64; 2], period: f64, anchor: f64) -> Option<usize> {
    let left = ((window[0] - anchor) / period).ceil();
    let right = ((window[1] - anchor) / period).ceil();
    if !left.is_finite()
        || !right.is_finite()
        || left.abs() > EXACT_INTEGER
        || right.abs() > EXACT_INTEGER
        || right < left
    {
        return None;
    }
    usize::try_from((right - left) as u64).ok()
}

fn span_checked(
    beats: &[f64],
    points: &[Point],
    period: f64,
    anchor: f64,
    window: [f64; 2],
    control: &Control<'_>,
) -> FitResult<SpanDiagnostics> {
    control.check()?;
    let mut local = allocate(points.len())?;
    let mut maximum = 0.0_f64;
    let mut squared = 0.0;
    let mut previous = window[0];
    let mut gap = 0.0_f64;
    for (index, point) in points.iter().enumerate() {
        control.block(index)?;
        let slot = anchor + period * point.ordinal as f64;
        if !inside(point.seconds, window) || !inside(slot, window) {
            continue;
        }
        let residual = point.seconds - slot;
        maximum = maximum.max(residual.abs());
        squared += residual * residual;
        gap = gap.max(point.seconds - previous);
        previous = point.seconds;
        local.push(*point);
    }
    gap = gap.max(window[1] - previous);
    let mut raw_count = 0;
    for (index, &t) in beats.iter().enumerate() {
        control.block(index)?;
        raw_count += usize::from(inside(t, window));
    }
    let Some(expected_slots) = slots(window, period, anchor) else {
        return Ok(None);
    };
    if local.len() > raw_count || local.len() > expected_slots {
        return Ok(None);
    }
    Ok(Some(SpanDiagnostics {
        window_seconds: window,
        raw_count,
        expected_slots,
        matched: local.len(),
        observed_span_seconds: local
            .first()
            .zip(local.last())
            .map(|(a, b)| b.seconds - a.seconds),
        fitted_bpm: line_checked(&local, control)?.map(|(_, _, p)| 60.0 / p),
        max_residual_seconds: (!local.is_empty()).then_some(maximum),
        rms_residual_seconds: (!local.is_empty()).then(|| (squared / local.len() as f64).sqrt()),
        max_gap_seconds: gap,
    }))
}

#[cfg(test)]
fn span(
    beats: &[f64],
    points: &[Point],
    period: f64,
    anchor: f64,
    window: [f64; 2],
) -> Option<SpanDiagnostics> {
    span_checked(
        beats,
        points,
        period,
        anchor,
        window,
        &Control::new(&never_cancel),
    )
    .ok()
    .flatten()
}

fn summary(
    beats: &[f64],
    duration: f64,
    points: &[Point],
    period: f64,
    anchor: f64,
    control: &Control<'_>,
) -> FitResult<CandidateSummary> {
    let Some((mx, my, fitted)) = line_checked(points, control)? else {
        return Ok(None);
    };
    if fitted.to_bits() != period.to_bits() {
        return Ok(None);
    }
    let mut sections = allocate(SECTIONS)?;
    for section in 0..SECTIONS {
        let Some(observed) = span_checked(
            beats,
            points,
            period,
            anchor,
            [
                section as f64 * duration / 5.0,
                (section + 1) as f64 * duration / 5.0,
            ],
            control,
        )?
        else {
            return Ok(None);
        };
        sections.push(observed);
    }
    let Some(global) = span_checked(beats, points, period, anchor, [0.0, duration], control)?
    else {
        return Ok(None);
    };
    Ok(Some(CandidateSummary {
        period_seconds: period,
        anchor_seconds: anchor,
        pivot_ordinal: mx,
        pivot_seconds: my,
        global,
        sections,
    }))
}

#[cfg(test)]
fn fit(beats: &[f64], duration: f64, period: f64, anchor: f64) -> FitResult<CandidateSummary> {
    fit_control(
        beats,
        duration,
        period,
        anchor,
        &Control::new(&never_cancel),
    )
}

fn fit_control(
    beats: &[f64],
    duration: f64,
    period: f64,
    anchor: f64,
    control: &Control<'_>,
) -> FitResult<CandidateSummary> {
    let Some(points) = assignment_checked(beats, duration, period, anchor, control)? else {
        return Ok(None);
    };
    let Some((points, period, anchor)) = fixed_point(
        points,
        |period, anchor| assignment_checked(beats, duration, period, anchor, control),
        control,
    )?
    else {
        return Ok(None);
    };
    summary(beats, duration, &points, period, anchor, control)
}

fn fixed_point(
    mut points: Vec<Point>,
    mut reassign: impl FnMut(f64, f64) -> FitResult<Vec<Point>>,
    control: &Control<'_>,
) -> FitResult<(Vec<Point>, f64, f64)> {
    for _ in 0..6 {
        control.check()?;
        if points.len() < 4 {
            return Ok(None);
        }
        let first = points[0].ordinal;
        for (index, point) in points.iter_mut().enumerate() {
            control.block(index)?;
            let Some(normalized) = point.ordinal.checked_sub(first) else {
                return Ok(None);
            };
            point.ordinal = normalized;
        }
        let Some((mx, my, next_period)) = line_checked(&points, control)? else {
            return Ok(None);
        };
        let next_anchor = my - next_period * mx;
        let Some(next) = reassign(next_period, next_anchor)? else {
            return Ok(None);
        };
        let mut identical = next.len() == points.len();
        if identical {
            for (index, (a, b)) in next.iter().zip(&points).enumerate() {
                control.block(index)?;
                if a.raw != b.raw || a.ordinal != b.ordinal {
                    identical = false;
                    break;
                }
            }
        }
        if identical {
            return Ok(Some((next, next_period, next_anchor)));
        }
        points = next;
    }
    Ok(None)
}

fn refine_sample(
    beats: &[f64],
    sample: &[usize],
    duration: f64,
    period: f64,
    anchor: f64,
    control: &Control<'_>,
) -> FitResult<(f64, f64)> {
    let assign = |period, anchor| {
        assignment_iter(
            sample.iter().map(|&raw| (raw, beats[raw])),
            sample.len(),
            duration,
            period,
            anchor,
            control,
        )
    };
    let Some(points) = assign(period, anchor)? else {
        return Ok(None);
    };
    Ok(fixed_point(points, assign, control)?.map(|(_, period, anchor)| (period, anchor)))
}

fn equivalent(a: &CandidateSummary, b: &CandidateSummary, duration: f64) -> bool {
    if (a.period_seconds / b.period_seconds - 1.0).abs() > 0.0001 {
        return false;
    }
    let tolerance = limit(a.period_seconds).min(limit(b.period_seconds));
    let Some(winding) = ordinal((a.anchor_seconds - b.anchor_seconds) / b.period_seconds) else {
        return false;
    };
    let drift = (a.period_seconds - b.period_seconds).abs() * duration / a.period_seconds;
    if !drift.is_finite() || drift > tolerance {
        return false;
    }
    let initial = a.anchor_seconds - b.anchor_seconds - b.period_seconds * winding as f64;
    [0.0, duration].into_iter().all(|t| {
        let k = (t - a.anchor_seconds) / a.period_seconds;
        (initial + (a.period_seconds - b.period_seconds) * k).abs() <= tolerance
    })
}

fn section_stride(a: &CandidateSummary, b: &CandidateSummary, window: [f64; 2]) -> Option<i64> {
    let (fine, coarse) = if a.period_seconds <= b.period_seconds {
        (a, b)
    } else {
        (b, a)
    };
    let pf = fine.period_seconds;
    let pc = coarse.period_seconds;
    if !pf.is_finite()
        || !pc.is_finite()
        || pf <= 0.0
        || pc <= 0.0
        || !window.iter().all(|t| t.is_finite())
        || window[1] < window[0]
    {
        return None;
    }
    let stride = ordinal(pc / pf)?;
    if !(1..=10).contains(&stride) {
        return None;
    }
    let step = pc - stride as f64 * pf;
    let phase = coarse.anchor_seconds - fine.anchor_seconds;
    let mid = (window[0] + window[1]) * 0.5;
    let jmid = (mid - coarse.anchor_seconds) / pc;
    let winding = ordinal((phase + jmid * step) / pf)?;
    let epsilon = limit(pf).min(limit(pc));
    window
        .into_iter()
        .all(|t| {
            let residual = phase - winding as f64 * pf + ((t - coarse.anchor_seconds) / pc) * step;
            residual.is_finite() && residual.abs() <= epsilon
        })
        .then_some(stride)
}

fn sample_indices(beats: &[f64], duration: f64) -> Option<Vec<usize>> {
    let mut indices = reserve(SAMPLE)?;
    if beats.is_empty() {
        return Some(indices);
    }
    for i in 0..SAMPLE {
        let t = i as f64 * duration / (SAMPLE - 1) as f64;
        let index = beats
            .partition_point(|&value| value < t)
            .min(beats.len() - 1);
        if indices.last() != Some(&index) {
            indices.push(index);
        }
    }
    Some(indices)
}

fn long_pairs(sample: &[usize]) -> Option<[(usize, usize); 2]> {
    if sample.len() < 4 {
        return None;
    }
    Some([
        (sample[0], sample[sample.len() - 1]),
        (sample[sample.len() / 5], sample[4 * sample.len() / 5]),
    ])
}

fn canonical(mut period: f64) -> f64 {
    while period < 0.5 {
        period *= 2.0;
    }
    while period >= 1.0 {
        period *= 0.5;
    }
    period
}

fn screen(
    beats: &[f64],
    sample: &[usize],
    duration: f64,
    period: f64,
    anchor: f64,
    full_counts: &[usize; SECTIONS],
) -> Option<Seed> {
    let mut retained: usize = 0;
    let mut mask = 0_u8;
    let mut previous = None;
    let mut sampled = [0_usize; SECTIONS];
    let mut matched = [0_usize; SECTIONS];
    let mut first = [None; SECTIONS];
    let mut last = [None; SECTIONS];
    for &raw in sample {
        let t = beats[raw];
        let section = ((t / duration * 5.0).floor() as usize).min(4);
        sampled[section] += 1;
        let k = ordinal((t - anchor) / period)?;
        let slot = anchor + k as f64 * period;
        if !inside(slot, [0.0, duration]) || (t - slot).abs() > limit(period) || previous == Some(k)
        {
            continue;
        }
        previous = Some(k);
        retained += 1;
        mask |= 1 << section;
        matched[section] += 1;
        if first[section].is_none() {
            first[section] = Some(t);
        }
        last[section] = Some(t);
    }
    let q = (retained as f64).sqrt().ceil() as usize;
    let expected = slots([0.0, duration], period, anchor)?;
    let upper_count = ((retained.saturating_add(q).min(sample.len()) as f64 / sample.len() as f64)
        * beats.len() as f64)
        .ceil()
        .min(beats.len() as f64)
        .min(expected as f64);
    let denominator = beats.len() as f64 + expected as f64;
    let mut local_upper = [0.0; SECTIONS];
    let mut local_sustained = [false; SECTIONS];
    for section in 0..SECTIONS {
        if sampled[section] == 0 {
            continue;
        }
        let window = [
            section as f64 * duration / 5.0,
            (section + 1) as f64 * duration / 5.0,
        ];
        let expected = slots(window, period, anchor)?;
        let q = (matched[section] as f64).sqrt().ceil() as usize;
        let upper_count = ((matched[section].saturating_add(q).min(sampled[section]) as f64
            / sampled[section] as f64)
            * full_counts[section] as f64)
            .ceil()
            .min(full_counts[section] as f64)
            .min(expected as f64);
        let denominator = full_counts[section] as f64 + expected as f64;
        local_upper[section] = if denominator == 0.0 {
            0.0
        } else {
            2.0 * upper_count / denominator
        };
        local_sustained[section] = matched[section] >= 4
            && first[section]
                .zip(last[section])
                .is_some_and(|(a, b)| b - a >= 10.0_f64.min(0.6 * (window[1] - window[0])));
    }
    Some(Seed {
        period,
        anchor,
        matched: retained,
        sections: mask.count_ones() as usize,
        upper: if denominator == 0.0 {
            0.0
        } else {
            2.0 * upper_count / denominator
        },
        local_upper,
        local_sustained,
        family: 0,
        source_pair: [0; 2],
        source_count: 0,
        refined_from: None,
    })
}

fn seed_better(a: &Seed, b: &Seed) -> bool {
    a.sections > b.sections
        || (a.sections == b.sections
            && (a.matched > b.matched
                || (a.matched == b.matched
                    && a.refined_from.is_some()
                    && b.refined_from.is_none())))
}

fn same_family(a: f64, b: f64) -> bool {
    let ratio = canonical(a) / canonical(b);
    (ratio - 1.0)
        .abs()
        .min((ratio * 2.0 - 1.0).abs())
        .min((ratio * 0.5 - 1.0).abs())
        <= 0.01
}

fn constant_storage_bytes() -> Option<usize> {
    // Geometry remains live during fitting. Census identities and family rows
    // are a separate peak and drop before the candidate/model pool is allocated.
    let search = MAX_COARSE_SEEDS
        .checked_mul(std::mem::size_of::<Seed>())?
        .checked_add(MAX_COARSE_FAMILIES.checked_mul(std::mem::size_of::<usize>())?)?
        .checked_add((SAMPLE * 4 * 16).checked_mul(std::mem::size_of::<ObservedInterval>())?)?
        .checked_add(MAX_COARSE_FAMILIES.checked_mul(std::mem::size_of::<IntervalFamily>())?)?
        .checked_add(std::mem::size_of::<[u64; SAMPLE * 4 / 64]>())?
        .checked_add(SEARCH_FITS.checked_mul(std::mem::size_of::<(u64, u64)>())?)?
        .checked_add((MAX_FITS + 12).checked_mul(
            std::mem::size_of::<CandidateSummary>()
                + SECTIONS * std::mem::size_of::<SpanDiagnostics>(),
        )?)?
        .checked_add(64 * 1024)?;
    let census = CENSUS_KEYS
        .checked_mul(std::mem::size_of::<(u64, u64)>())?
        .checked_add(MAX_COARSE_FAMILIES.checked_mul(std::mem::size_of::<Seed>())?)?
        .checked_add((SAMPLE * 4 * 16).checked_mul(std::mem::size_of::<ObservedInterval>())?)?
        .checked_add(MAX_COARSE_FAMILIES.checked_mul(std::mem::size_of::<IntervalFamily>())?)?
        .checked_add(64 * 1024 + std::mem::size_of::<[u64; SAMPLE * 4 / 64]>())?;
    Some(search.max(census))
}

fn retain_seed(discovery: &mut Discovery, seed: Seed) -> Option<usize> {
    if let Some(index) = discovery.seeds.iter().position(|old| {
        old.period.to_bits() == seed.period.to_bits()
            && old.anchor.to_bits() == seed.anchor.to_bits()
    }) {
        return Some(index);
    }
    if discovery.seeds.len() == MAX_COARSE_SEEDS {
        discovery.limited = true;
        return None;
    }
    let index = discovery.seeds.len();
    discovery.seeds.push(seed);
    Some(index)
}

fn interval_families(
    observed: &mut [ObservedInterval],
    control: &Control<'_>,
) -> Result<Vec<IntervalFamily>, SearchFailure> {
    control.check()?;
    observed.sort_unstable_by(|a, b| {
        a.canonical_period
            .total_cmp(&b.canonical_period)
            .then(a.contributor.cmp(&b.contributor))
    });
    let mut families: Vec<IntervalFamily> = allocate(MAX_COARSE_FAMILIES)?;
    let mut begin = 0;
    while begin < observed.len() {
        control.check()?;
        let first = observed[begin].canonical_period;
        let mut end = begin + 1;
        while end < observed.len() && observed[end].canonical_period / first - 1.0 <= 0.01 {
            end += 1;
        }
        let mut contributors = [0_u64; SAMPLE * 4 / 64];
        for interval in &observed[begin..end] {
            contributors[interval.contributor / 64] |= 1 << (interval.contributor % 64);
        }
        let middle = begin + (end - begin) / 2;
        let period = if (end - begin) % 2 == 0 {
            (observed[middle - 1].canonical_period + observed[middle].canonical_period) / 2.0
        } else {
            observed[middle].canonical_period
        };
        if families.len() == MAX_COARSE_FAMILIES {
            return Err(SearchFailure::Allocation);
        }
        families.push(IntervalFamily {
            period,
            lower: observed[begin].canonical_period,
            upper: observed[end - 1].canonical_period,
            contributors: contributors
                .iter()
                .map(|bits| bits.count_ones() as usize)
                .sum(),
        });
        begin = end;
    }
    families.sort_unstable_by(|a, b| {
        b.contributors
            .cmp(&a.contributors)
            .then(a.period.total_cmp(&b.period))
    });
    Ok(families)
}

fn visit_observed_start(
    beats: &[f64],
    sample: &[usize],
    duration: f64,
    period: f64,
    anchor: f64,
    pair: [usize; 2],
    count: i64,
    full_counts: &[usize; SECTIONS],
    control: &Control<'_>,
    fuel: &mut usize,
    visitor: &mut impl FnMut(Seed) -> Result<Visit, SearchFailure>,
) -> Result<Visit, SearchFailure> {
    control.check()?;
    if *fuel == 0 {
        return Ok(Visit::Limited);
    }
    *fuel -= 1;
    if !(MIN_PERIOD..=MAX_PERIOD).contains(&period) {
        return Ok(Visit::Continue);
    }
    let mut candidate = required(screen(beats, sample, duration, period, anchor, full_counts))?;
    candidate.family = usize::MAX;
    candidate.source_pair = pair;
    candidate.source_count = count;
    let visited = visitor(candidate)?;
    if visited != Visit::Continue {
        return Ok(visited);
    }
    // A failed or differently phased refinement never erases its original witness.
    if let Some((period, anchor)) = refine_sample(beats, sample, duration, period, anchor, control)?
    {
        if period.to_bits() != candidate.period.to_bits()
            || anchor.to_bits() != candidate.anchor.to_bits()
        {
            let mut refined =
                required(screen(beats, sample, duration, period, anchor, full_counts))?;
            refined.source_pair = pair;
            refined.source_count = count;
            refined.family = usize::MAX;
            refined.refined_from = Some((candidate.period.to_bits(), candidate.anchor.to_bits()));
            let visited = visitor(refined)?;
            if visited != Visit::Continue {
                return Ok(visited);
            }
        }
    }
    Ok(Visit::Continue)
}

#[cfg(test)]
fn discover(beats: &[f64], duration: f64, sample: &[usize]) -> Option<Discovery> {
    discover_control(beats, duration, sample, &Control::new(&never_cancel)).ok()
}

fn admissible_counts(gap: f64, lower: f64, upper: f64) -> Option<std::ops::RangeInclusive<i64>> {
    if !gap.is_finite()
        || gap <= 0.0
        || !lower.is_finite()
        || !upper.is_finite()
        || lower < MIN_PERIOD
        || upper > MAX_PERIOD
        || lower > upper
    {
        return None;
    }
    let padding = (2.0 * limit(upper).next_up()).next_up();
    // Enclose the observed subtraction and every subsequent interval operation.
    let first = (((gap.next_down() - padding).next_down() / upper.next_up()).next_down())
        .ceil()
        .max(1.0);
    let last = (((gap.next_up() + padding).next_up() / lower.next_down()).next_up()).floor();
    Some(ordinal(first)?..=ordinal(last)?)
}

fn octave_bands(lower: f64, upper: f64) -> impl Iterator<Item = (f64, f64)> {
    let valid = lower.is_finite() && upper.is_finite() && lower > 0.0 && lower <= upper;
    [0.25, 0.5, 1.0, 2.0, 4.0]
        .into_iter()
        .filter_map(move |factor| {
            let lo = (lower * factor).max(MIN_PERIOD);
            let hi = (upper * factor).min(MAX_PERIOD);
            (valid && lo <= hi).then_some((lo, hi))
        })
}

fn build_geometry(
    beats: &[f64],
    duration: f64,
    sample: &[usize],
    control: &Control<'_>,
) -> Result<Geometry, SearchFailure> {
    control.check()?;
    if required(constant_storage_bytes())? > FIXED_RESERVE {
        return Err(SearchFailure::Allocation);
    }
    let mut full_counts = [0_usize; SECTIONS];
    for (index, &t) in beats.iter().enumerate() {
        control.block(index)?;
        full_counts[((t / duration * 5.0).floor() as usize).min(4)] += 1;
    }
    let mut periods: Vec<ObservedInterval> = allocate(SAMPLE * 4 * 16)?;
    for (position, &raw) in sample.iter().enumerate() {
        control.check()?;
        for (lag_index, lag) in [1, 2, 4, 8].into_iter().enumerate() {
            let Some(&later) = beats.get(raw + lag) else {
                continue;
            };
            let gap = later - beats[raw];
            for count in 1..=16 {
                let period = gap / count as f64;
                if (MIN_PERIOD..=MAX_PERIOD).contains(&period) {
                    periods.push(ObservedInterval {
                        period,
                        canonical_period: canonical(period),
                        contributor: position * 4 + lag_index,
                    });
                }
            }
        }
    }
    let interval_families = interval_families(&mut periods, control)?;
    periods.sort_unstable_by(|a, b| a.period.total_cmp(&b.period));
    Ok(Geometry {
        periods,
        families: interval_families,
        full_counts,
    })
}

fn visit_proposals(
    beats: &[f64],
    duration: f64,
    sample: &[usize],
    geometry: &Geometry,
    control: &Control<'_>,
    visitor: &mut impl FnMut(Seed) -> Result<Visit, SearchFailure>,
) -> Result<Visit, SearchFailure> {
    let Some(pairs) = long_pairs(sample) else {
        return Ok(Visit::Continue);
    };
    let periods = &geometry.periods;
    let interval_families = &geometry.families;
    let full_counts = &geometry.full_counts;
    let mut fuel = ORIGINAL_FUEL;
    let mut begin = 0;
    while begin < periods.len() {
        control.check()?;
        let first = periods[begin].period;
        let mut end = begin + 1;
        while end < periods.len() && periods[end].period / first - 1.0 <= 0.01 {
            end += 1;
        }
        let approximate = periods[(begin + end) / 2].period;
        for (a, b) in pairs {
            let extent = beats[b] - beats[a];
            let count = required(ordinal(extent / approximate))?;
            for offset in [-1, 0, 1] {
                let count = required(count.checked_add(offset))?;
                if count <= 0 {
                    continue;
                }
                let period = extent / count as f64;
                if !(MIN_PERIOD..=MAX_PERIOD).contains(&period) {
                    continue;
                }
                let visited = visit_observed_start(
                    beats,
                    sample,
                    duration,
                    period,
                    beats[a],
                    [a, b],
                    count,
                    full_counts,
                    control,
                    &mut fuel,
                    visitor,
                )?;
                if visited != Visit::Continue {
                    return Ok(visited);
                }
            }
        }
        begin = end;
    }
    // Interval support chooses the same four-family allowance before endpoint
    // phase can bias it. Every mechanical half-span pair supplies a real phase.
    let half = sample.len() / 2;
    for family in interval_families.iter().take(4) {
        for i in 0..half {
            let (a, b) = (sample[i], sample[i + half]);
            let extent = beats[b] - beats[a];
            let nearest = required(ordinal(extent / family.period))?;
            for offset in [-1, 0, 1] {
                let count = required(nearest.checked_add(offset))?;
                if count <= 0 {
                    continue;
                }
                let period = extent / count as f64;
                if !(MIN_PERIOD..=MAX_PERIOD).contains(&period) {
                    continue;
                }
                let visited = visit_observed_start(
                    beats,
                    sample,
                    duration,
                    period,
                    beats[a],
                    [a, b],
                    count,
                    full_counts,
                    control,
                    &mut fuel,
                    visitor,
                )?;
                if visited != Visit::Continue {
                    return Ok(visited);
                }
            }
            for (lower, upper) in octave_bands(family.lower, family.upper) {
                let Some(counts) = admissible_counts(extent, lower, upper) else {
                    return Ok(Visit::Limited);
                };
                // Empty inclusive ranges consume no fuel. Preflight prevents
                // enormous but finite ordinal ranges from becoming long loops.
                let length = if counts.is_empty() {
                    0
                } else {
                    required(
                        counts
                            .end()
                            .checked_sub(*counts.start())
                            .and_then(|n| n.checked_add(1)),
                    )?
                };
                if usize::try_from(length)
                    .ok()
                    .is_none_or(|length| length > fuel)
                {
                    return Ok(Visit::Limited);
                }
                for count in counts {
                    control.check()?;
                    let period = extent / count as f64;
                    let visited = visit_observed_start(
                        beats,
                        sample,
                        duration,
                        period,
                        beats[a],
                        [a, b],
                        count,
                        full_counts,
                        control,
                        &mut fuel,
                        visitor,
                    )?;
                    if visited != Visit::Continue {
                        return Ok(visited);
                    }
                }
            }
        }
    }
    Ok(Visit::Continue)
}

fn rank_representatives(seeds: &mut [Seed]) {
    seeds.sort_unstable_by(|a, b| {
        b.sections
            .cmp(&a.sections)
            .then(b.matched.cmp(&a.matched))
            .then(b.refined_from.is_some().cmp(&a.refined_from.is_some()))
            .then(a.period.total_cmp(&b.period))
            .then(a.anchor.total_cmp(&b.anchor))
    });
}

fn representative_census(
    beats: &[f64],
    duration: f64,
    sample: &[usize],
    geometry: &Geometry,
    control: &Control<'_>,
) -> Result<([Option<Seed>; 4], usize, bool), SearchFailure> {
    let mut census = Census::new()?;
    let status = visit_proposals(beats, duration, sample, geometry, control, &mut |seed| {
        census.accept(seed)?;
        Ok(if census.limited {
            Visit::Limited
        } else {
            Visit::Continue
        })
    })?;
    rank_representatives(&mut census.representatives);
    let selected = std::array::from_fn(|index| census.representatives.get(index).copied());
    let family_count = census.representatives.len();
    let limited = census.limited || status != Visit::Continue;
    // Census identities never coexist with the full candidate/model allocation.
    drop(census);
    Ok((selected, family_count, limited))
}

#[cfg(test)]
fn discover_control(
    beats: &[f64],
    duration: f64,
    sample: &[usize],
    control: &Control<'_>,
) -> Result<Discovery, SearchFailure> {
    let geometry = build_geometry(beats, duration, sample, control)?;
    let mut census = Census::new()?;
    let mut seeds = allocate(CENSUS_KEYS)?;
    let status = visit_proposals(beats, duration, sample, &geometry, control, &mut |seed| {
        if let Some(seed) = census.accept(seed)? {
            seeds.push(seed);
        }
        Ok(if census.limited {
            Visit::Limited
        } else {
            Visit::Continue
        })
    })?;
    rank_representatives(&mut census.representatives);
    let mut representatives = allocate(MAX_COARSE_FAMILIES)?;
    for representative in &census.representatives {
        representatives.push(required(seeds.iter().position(|seed| {
            seed.period.to_bits() == representative.period.to_bits()
                && seed.anchor.to_bits() == representative.anchor.to_bits()
        }))?);
    }
    Ok(Discovery {
        seeds,
        representatives,
        family_count: census.representatives.len(),
        limited: census.limited || status != Visit::Continue,
    })
}

fn visit_legacy(
    beats: &[f64],
    duration: f64,
    sample: &[usize],
    geometry: &Geometry,
    selected: &[Option<Seed>; 4],
    control: &Control<'_>,
    visitor: &mut impl FnMut(Seed) -> Result<Visit, SearchFailure>,
) -> Result<Visit, SearchFailure> {
    let Some(pairs) = long_pairs(sample) else {
        return Ok(Visit::Continue);
    };
    for family in selected.iter().flatten() {
        for harmonic in [0.5, 1.0, 2.0] {
            for (a, b) in pairs {
                let extent = beats[b] - beats[a];
                let count = required(ordinal(extent / (family.period * harmonic)))?;
                for offset in [-1, 0, 1] {
                    let count = required(count.checked_add(offset))?;
                    if count <= 0 {
                        continue;
                    }
                    let period = extent / count as f64;
                    if !(MIN_PERIOD..=MAX_PERIOD).contains(&period) {
                        continue;
                    }
                    for anchor in [Some(beats[a]), companion(beats, period, beats[a], control)?]
                        .into_iter()
                        .flatten()
                    {
                        let mut seed = required(screen(
                            beats,
                            sample,
                            duration,
                            period,
                            anchor,
                            &geometry.full_counts,
                        ))?;
                        seed.source_pair = [a, b];
                        seed.source_count = count;
                        seed.family = family.family;
                        let visited = visitor(seed)?;
                        if visited != Visit::Continue {
                            return Ok(visited);
                        }
                    }
                }
            }
        }
    }
    Ok(Visit::Continue)
}

/// The full raw stream supplies phase companions even when the 64-point sample aliases.
fn companion(beats: &[f64], period: f64, anchor: f64, control: &Control<'_>) -> FitResult<f64> {
    let mut bins = [0_usize; PHASE_BINS];
    for (position, &t) in beats.iter().enumerate() {
        control.block(position)?;
        let phase = (t - anchor).rem_euclid(period);
        let index = ((phase / period * PHASE_BINS as f64).floor() as usize).min(PHASE_BINS - 1);
        bins[index] += 1;
    }
    let mut best = None;
    for (index, &count) in bins.iter().enumerate() {
        let phase = (index as f64 + 0.5) * period / PHASE_BINS as f64;
        if phase.min(period - phase) <= limit(period) || count == 0 {
            continue;
        }
        if best.is_none_or(|(_, old)| count > old) {
            best = Some((phase, count));
        }
    }
    Ok(best.map(|(phase, _)| anchor + phase))
}

fn eligible(candidate: &CandidateSummary, duration: f64) -> bool {
    if duration < 30.0 {
        candidate.global.matched >= 4
            && candidate
                .global
                .observed_span_seconds
                .is_some_and(|span| span >= (3.0 * candidate.period_seconds).max(0.6 * duration))
    } else {
        candidate.global.matched >= 8
            && candidate.sections[1..4]
                .iter()
                .all(SpanDiagnostics::sustained)
    }
}

fn check(
    points: &[Point],
    windows: &[[f64; 2]],
    pivot: (f64, f64, f64),
    bpm: f64,
    source_period: f64,
    source_anchor: f64,
    control: &Control<'_>,
) -> FitResult<FractionalCheck> {
    control.check()?;
    if points.len() < 2 || windows.len() != 2 {
        return Ok(None);
    }
    let (mx, my, free) = pivot;
    let period = 60.0 / bpm;
    let mut maximum = 0.0_f64;
    let mut squared = 0.0;
    let mut free_squared = 0.0;
    let mut offsets = [0.0; 2];
    let mut counts = [0_usize; 2];
    for (index, point) in points.iter().enumerate() {
        control.block(index)?;
        let x = point.ordinal as f64 - mx;
        let residual = point.seconds - my - period * x;
        let free_residual = point.seconds - my - free * x;
        maximum = maximum.max(residual.abs());
        squared += residual * residual;
        free_squared += free_residual * free_residual;
        let slot = source_anchor + source_period * point.ordinal as f64;
        for (side, &window) in windows.iter().enumerate() {
            if inside(point.seconds, window) && inside(slot, window) {
                offsets[side] += residual;
                counts[side] += 1;
            }
        }
    }
    if counts.contains(&0) {
        return Ok(None);
    }
    let rms = (squared / points.len() as f64).sqrt();
    let penalty = (rms - (free_squared / points.len() as f64).sqrt()).max(0.0);
    let phase = (offsets[1] / counts[1] as f64 - offsets[0] / counts[0] as f64).abs();
    Ok(Some(FractionalCheck {
        status: if maximum <= limit(period) && penalty <= 0.00025 && phase <= 0.005 {
            "pass"
        } else {
            "fail"
        },
        max_residual_seconds: Some(maximum),
        rms_residual_seconds: Some(rms),
        rms_penalty_seconds: Some(penalty),
        window_phase_difference_seconds: Some(phase),
    }))
}

pub(super) fn phase_only_period(
    beats: &[f64],
    duration: f64,
    evidence: &Evidence,
    control: &Control<'_>,
) -> Result<Option<f64>, super::Cancelled> {
    if control.cancelled() {
        return Err(super::Cancelled);
    }
    let Some(winner) = &evidence.winner else {
        return Ok(None);
    };
    if evidence.outcome != "insufficient"
        || evidence.candidate_limit_reached
        || !evidence.conflicting_sections.is_empty()
        || evidence.evidence_tier != "two_ten_second"
        || duration < 30.0
        || evidence.probe_windows.len() != 2
        || !eligible(winner, duration)
    {
        return Ok(None);
    }
    let Ok(mut points) = allocate(evidence.assignments.len()) else {
        return Ok(None);
    };
    for (index, &(raw, ordinal)) in evidence.assignments.iter().enumerate() {
        if control.block(index).is_err() {
            return Err(super::Cancelled);
        }
        let Some(&seconds) = beats.get(raw) else {
            return Ok(None);
        };
        points.push(Point {
            raw,
            ordinal,
            seconds,
        });
    }
    let candidate_bpm = 60.0 / winner.period_seconds;
    let checked = check(
        &points,
        &evidence.probe_windows,
        (
            winner.pivot_ordinal,
            winner.pivot_seconds,
            winner.period_seconds,
        ),
        candidate_bpm,
        winner.period_seconds,
        winner.anchor_seconds,
        control,
    );
    if control.cancelled() {
        return Err(super::Cancelled);
    }
    let Ok(Some(checked)) = checked else {
        return Ok(None);
    };
    Ok(phase_failure_only(&checked, 60.0 / candidate_bpm).then_some(winner.period_seconds))
}

fn phase_failure_only(checked: &FractionalCheck, effective_period: f64) -> bool {
    checked
        .max_residual_seconds
        .is_some_and(|n| n <= limit(effective_period))
        && checked.rms_penalty_seconds.is_some_and(|n| n <= 0.00025)
        && checked
            .window_phase_difference_seconds
            .is_some_and(|n| n > 0.005)
}

fn probes(
    beats: &[f64],
    duration: f64,
    winner: &CandidateSummary,
    points: &[Point],
    control: &Control<'_>,
) -> Result<Vec<[f64; 2]>, SearchFailure> {
    let mut windows = allocate(5)?;
    if duration < 30.0 {
        for window in [[0.0, duration / 2.0], [duration / 2.0, duration]] {
            let Some(observed) = span_checked(
                beats,
                points,
                winner.period_seconds,
                winner.anchor_seconds,
                window,
                control,
            )?
            else {
                return Ok(Vec::new());
            };
            if observed.matched < 2 {
                return Ok(Vec::new());
            }
            windows.push(window);
        }
        return Ok(windows);
    }
    let width = (duration / 5.0).clamp(16.0, 32.0);
    for section in 0..5 {
        let center = (section as f64 + 0.5) * duration / 5.0;
        let window = [
            (center - width / 2.0).max(0.0),
            (center + width / 2.0).min(duration),
        ];
        let Some(observed) = span_checked(
            beats,
            points,
            winner.period_seconds,
            winner.anchor_seconds,
            window,
            control,
        )?
        else {
            return Ok(Vec::new());
        };
        if observed.matched >= 4 && observed.observed_span_seconds.is_some_and(|s| s >= 10.0) {
            windows.push(window);
        }
    }
    let mut pair = None;
    let mut gap = -1.0;
    for (a, &left) in windows.iter().enumerate() {
        for &right in &windows[a + 1..] {
            if left[1] <= right[0] && right[0] - left[1] > gap {
                gap = right[0] - left[1];
                pair = Some([left, right]);
            }
        }
    }
    let mut selected = allocate(2)?;
    if let Some(pair) = pair {
        selected.extend(pair);
    }
    Ok(selected)
}

fn add_critical(indices: &mut Vec<usize>, index: Option<usize>) {
    if let Some(index) = index.filter(|index| !indices.contains(index)) {
        indices.push(index);
    }
}

#[cfg(test)]
fn coarse_could_veto(candidate: &Seed, winner: &CandidateSummary) -> bool {
    (candidate.sections >= 3 && candidate.upper >= winner.global.band().0)
        || (0..5).any(|section| {
            candidate.local_sustained[section]
                && candidate.local_upper[section] > winner.sections[section].band().1
        })
}

fn coarse_margin(candidate: &Seed, winner: Option<&CandidateSummary>) -> Option<f64> {
    let mut margin = None;
    if candidate.sections >= 3 {
        let value = candidate.upper - winner.map_or(0.0, |winner| winner.global.band().0);
        if value >= 0.0 {
            margin = Some(value);
        }
    }
    for section in 0..SECTIONS {
        if candidate.local_sustained[section] {
            let value = candidate.local_upper[section]
                - winner.map_or(0.0, |winner| winner.sections[section].band().1);
            if value > 0.0 {
                margin = Some(margin.map_or(value, |old: f64| old.max(value)));
            }
        }
    }
    margin
}

fn winner_index(pool: &[CandidateSummary], duration: f64) -> Option<usize> {
    pool.iter()
        .enumerate()
        .filter(|(_, candidate)| eligible(candidate, duration))
        .max_by(|(ia, a), (ib, b)| {
            a.global
                .score()
                .total_cmp(&b.global.score())
                .then(ib.cmp(ia))
        })
        .or_else(|| {
            pool.iter().enumerate().max_by(|(ia, a), (ib, b)| {
                a.global
                    .score()
                    .total_cmp(&b.global.score())
                    .then(ib.cmp(ia))
            })
        })
        .map(|(index, _)| index)
}

fn was_attempted(attempts: &[(u64, u64)], period: f64, anchor: f64) -> bool {
    attempts.contains(&(period.to_bits(), anchor.to_bits()))
}

fn search_attempt(
    beats: &[f64],
    duration: f64,
    period: f64,
    anchor: f64,
    evidence: &mut Evidence,
    pool: &mut Vec<CandidateSummary>,
    attempts: &mut Vec<(u64, u64)>,
    control: &Control<'_>,
) -> Result<bool, SearchFailure> {
    control.check()?;
    if was_attempted(attempts, period, anchor) {
        #[cfg(test)]
        tests::observe_search_memo(period, anchor);
        return Ok(true);
    }
    if evidence.evaluated_candidate_count == SEARCH_FITS {
        #[cfg(test)]
        tests::observe_search_capacity(period, anchor);
        return Ok(false);
    }
    attempts.push((period.to_bits(), anchor.to_bits()));
    evidence.evaluated_candidate_count += 1;
    let fitted = fit_control(beats, duration, period, anchor, control);
    #[cfg(test)]
    tests::observe_search_fit(
        period,
        anchor,
        evidence.evaluated_candidate_count,
        pool.len(),
        &fitted,
    );
    if let Some(candidate) = fitted? {
        pool.push(candidate);
    }
    Ok(true)
}

fn copy_summary(candidate: &CandidateSummary) -> Result<CandidateSummary, SearchFailure> {
    let mut sections = allocate(SECTIONS)?;
    sections.extend(candidate.sections.iter().cloned());
    Ok(CandidateSummary {
        period_seconds: candidate.period_seconds,
        anchor_seconds: candidate.anchor_seconds,
        pivot_ordinal: candidate.pivot_ordinal,
        pivot_seconds: candidate.pivot_seconds,
        global: candidate.global.clone(),
        sections,
    })
}

fn retain_direct(
    discovery: &mut Discovery,
    beats: &[f64],
    sample: &[usize],
    duration: f64,
    period: f64,
    anchor: f64,
    pair: [usize; 2],
    count: i64,
    full_counts: &[usize; SECTIONS],
    family: usize,
    control: &Control<'_>,
) -> Result<Option<usize>, SearchFailure> {
    control.check()?;
    let mut seed = required(screen(beats, sample, duration, period, anchor, full_counts))?;
    seed.source_pair = pair;
    seed.source_count = count;
    seed.family = family;
    Ok(retain_seed(discovery, seed))
}

fn octave_periods(period: f64) -> impl Iterator<Item = f64> {
    std::iter::once(period)
        .chain(
            std::iter::successors(Some(period * 0.5), |period| Some(*period * 0.5))
                .take_while(|period| *period >= MIN_PERIOD),
        )
        .chain(
            std::iter::successors(Some(period * 2.0), |period| Some(*period * 2.0))
                .take_while(|period| *period <= MAX_PERIOD),
        )
}

fn strongest(
    pool: &[CandidateSummary],
    winner: &CandidateSummary,
    duration: f64,
    section: Option<usize>,
) -> Option<usize> {
    pool.iter()
        .enumerate()
        .filter(|(_, candidate)| {
            !equivalent(candidate, winner, duration)
                && section.map_or_else(
                    || candidate.global.sustained(),
                    |i| {
                        candidate.sections[i].sustained()
                            && section_stride(candidate, winner, winner.sections[i].window_seconds)
                                .is_none()
                    },
                )
        })
        .max_by(|(ia, a), (ib, b)| {
            // Persist the actual veto statistic, not a point-score surrogate. A
            // weaker point score can still own the strongest count-scaled band.
            let veto = |candidate: &CandidateSummary| {
                section.map_or_else(
                    || candidate.global.band().1,
                    |i| candidate.sections[i].band().0,
                )
            };
            veto(a).total_cmp(&veto(b)).then(ib.cmp(ia))
        })
        .map(|(index, _)| index)
}

fn empty_evidence() -> Evidence {
    Evidence {
        outcome: "insufficient",
        evidence_tier: "inconclusive",
        candidate_limit_reached: false,
        evaluated_candidate_count: 0,
        winner: None,
        assignments: Vec::new(),
        alternatives: Vec::new(),
        probe_windows: Vec::new(),
        fractional_check: None,
        integer_check: None,
        integer_candidate_bpm: None,
        integer_preferred: false,
        conflicting_sections: Vec::new(),
        harmonics: Vec::new(),
    }
}

#[cfg(test)]
fn evaluate(beats: &[f64], duration: f64) -> Option<(Option<f64>, Evidence)> {
    evaluate_control(beats, duration, &Control::new(&never_cancel)).ok()
}

fn replay_seed(
    seed: Seed,
    basis: Option<&CandidateSummary>,
    beats: &[f64],
    duration: f64,
    discovery: &mut Discovery,
    evidence: &mut Evidence,
    pool: &mut Vec<CandidateSummary>,
    attempts: &mut Vec<(u64, u64)>,
    control: &Control<'_>,
) -> Result<Visit, SearchFailure> {
    control.check()?;
    if discovery.seeds.iter().any(|old| {
        old.period.to_bits() == seed.period.to_bits()
            && old.anchor.to_bits() == seed.anchor.to_bits()
    }) {
        return Ok(Visit::Continue);
    }
    let screening = basis.filter(|winner| eligible(winner, duration));
    if coarse_margin(&seed, screening).is_none() {
        return Ok(Visit::Continue);
    }
    let Some(index) = retain_seed(discovery, seed) else {
        return Ok(Visit::Limited);
    };
    #[cfg(test)]
    tests::observe_search_stage("replay_witness", Some(index));
    if !search_attempt(
        beats,
        duration,
        seed.period,
        seed.anchor,
        evidence,
        pool,
        attempts,
        control,
    )? {
        discovery.limited = true;
        return Ok(Visit::Limited);
    }
    let chosen = winner_index(pool, duration).map(|index| &pool[index]);
    Ok(if chosen == basis {
        Visit::Continue
    } else {
        Visit::Restart
    })
}

fn evaluate_control(
    beats: &[f64],
    duration: f64,
    control: &Control<'_>,
) -> Result<(Option<f64>, Evidence), SearchFailure> {
    control.check()?;
    let mut evidence = empty_evidence();
    evidence.alternatives = allocate(10)?;
    evidence.conflicting_sections = allocate(5)?;
    let sampled = required(sample_indices(beats, duration))?;
    if long_pairs(&sampled).is_none() {
        return Ok((None, evidence));
    }
    let geometry = build_geometry(beats, duration, &sampled, control)?;
    let (selected_families, _family_count, limited) =
        representative_census(beats, duration, &sampled, &geometry, control)?;
    let mut discovery = Discovery {
        seeds: allocate(MAX_COARSE_SEEDS)?,
        limited,
        #[cfg(test)]
        representatives: Vec::new(),
        #[cfg(test)]
        family_count: _family_count,
    };
    let mut pool: Vec<CandidateSummary> = allocate(MAX_FITS)?;
    let mut attempts = allocate(SEARCH_FITS)?;
    let mut priorities: [Option<(usize, usize)>; 32] = [None; 32];
    let mut priority_count = 0;
    for (rank, family) in selected_families.iter().flatten().copied().enumerate() {
        // Exact octave starts preserve the full period range after canonical
        // interval ranking. Their actual phase precedes endpoint resnapping.
        for period in octave_periods(family.period) {
            for anchor in [
                Some(family.anchor),
                companion(beats, period, family.anchor, control)?,
            ]
            .into_iter()
            .flatten()
            {
                if let Some(index) = retain_direct(
                    &mut discovery,
                    beats,
                    &sampled,
                    duration,
                    period,
                    anchor,
                    family.source_pair,
                    family.source_count,
                    &geometry.full_counts,
                    family.family,
                    control,
                )? {
                    if !priorities.iter().flatten().any(|(_, old)| *old == index) {
                        priorities[priority_count] = Some((rank, index));
                        priority_count += 1;
                    }
                }
            }
        }
    }
    for (_rank, index) in priorities.into_iter().flatten() {
        #[cfg(test)]
        tests::observe_search_stage("exact_octave", Some(_rank));
        let seed = discovery.seeds[index];
        search_attempt(
            beats,
            duration,
            seed.period,
            seed.anchor,
            &mut evidence,
            &mut pool,
            &mut attempts,
            control,
        )?;
    }
    // A completed replay is relative to one fixed winner. Any changed value
    // restarts immediately, since its local bands need not strengthen.
    while !discovery.limited {
        control.check()?;
        let basis = winner_index(&pool, duration)
            .map(|index| copy_summary(&pool[index]))
            .transpose()?;
        let _before = attempts.len();
        let status = {
            let mut visitor = |seed| {
                replay_seed(
                    seed,
                    basis.as_ref(),
                    beats,
                    duration,
                    &mut discovery,
                    &mut evidence,
                    &mut pool,
                    &mut attempts,
                    control,
                )
            };
            let legacy = visit_legacy(
                beats,
                duration,
                &sampled,
                &geometry,
                &selected_families,
                control,
                &mut visitor,
            )?;
            if legacy == Visit::Continue {
                visit_proposals(beats, duration, &sampled, &geometry, control, &mut visitor)?
            } else {
                legacy
            }
        };
        #[cfg(test)]
        tests::observe_replay(status, _before, attempts.len(), discovery.seeds.len());
        match status {
            Visit::Continue => break,
            Visit::Limited => {
                discovery.limited = true;
                break;
            }
            Visit::Restart => debug_assert!(attempts.len() > _before),
        }
    }
    let selected_winner = winner_index(&pool, duration);
    #[cfg(test)]
    tests::observe_search_snapshot(&discovery, &attempts, &pool, selected_winner);
    let Some(winner_index) = selected_winner else {
        evidence.candidate_limit_reached = discovery.limited;
        if evidence.candidate_limit_reached {
            evidence.outcome = "search_limited";
        }
        return Ok((None, evidence));
    };
    let winner_copy = copy_summary(&pool[winner_index])?;
    let winner = &winner_copy;
    let points = required(assignment_checked(
        beats,
        duration,
        winner.period_seconds,
        winner.anchor_seconds,
        control,
    )?)?;
    if points.first().is_none_or(|point| point.ordinal != 0) {
        return Ok((None, evidence));
    }
    evidence.candidate_limit_reached = discovery.limited;
    let mut harmonic_indices = [None; 2];
    evidence.harmonics = allocate(2)?;
    for (side, factor) in [0.5, 2.0].into_iter().enumerate() {
        let period = winner.period_seconds * factor;
        let mut declared = HarmonicOutcome {
            factor,
            seed_period_seconds: period,
            seed_anchor_seconds: winner.anchor_seconds,
            outcome: "out_of_range",
            alternative_index: None,
        };
        if (MIN_PERIOD..=MAX_PERIOD).contains(&period) {
            evidence.evaluated_candidate_count += 1;
            let fitted = fit_control(beats, duration, period, winner.anchor_seconds, control);
            #[cfg(test)]
            tests::observe_harmonic_fit(
                side,
                period,
                winner.anchor_seconds,
                Some(evidence.evaluated_candidate_count),
                Some(&fitted),
            );
            if let Some(candidate) = fitted? {
                let index = if let Some(index) = pool.iter().position(|old| old == &candidate) {
                    index
                } else {
                    let index = pool.len();
                    pool.push(candidate);
                    index
                };
                harmonic_indices[side] = Some(index);
                declared.outcome = "fitted";
            } else {
                declared.outcome = "no_fixed_fit";
            }
        } else {
            #[cfg(test)]
            tests::observe_harmonic_fit(side, period, winner.anchor_seconds, None, None);
        }
        evidence.harmonics.push(declared);
    }
    let mut critical = allocate(10)?;
    let global_rival = strongest(&pool, winner, duration, None);
    add_critical(&mut critical, global_rival);
    for section in 0..5 {
        let rival = strongest(&pool, winner, duration, Some(section));
        if rival
            .is_some_and(|i| pool[i].sections[section].band().0 > winner.sections[section].band().1)
        {
            evidence.conflicting_sections.push(section);
        }
        add_critical(&mut critical, rival);
    }
    for index in harmonic_indices {
        add_critical(&mut critical, index);
    }
    let phase_rival = pool
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            (candidate.period_seconds / winner.period_seconds - 1.0).abs() <= 0.0001
                && !equivalent(candidate, winner, duration)
        })
        .max_by(|(ia, a), (ib, b)| {
            a.global
                .score()
                .total_cmp(&b.global.score())
                .then(ib.cmp(ia))
        })
        .map(|(index, _)| index);
    add_critical(&mut critical, phase_rival);
    let density = pool
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            !equivalent(candidate, winner, duration)
                && candidate.global.sustained()
                && (0..SECTIONS).any(|i| {
                    candidate.sections[i].sustained()
                        && winner.sections[i].sustained()
                        && section_stride(candidate, winner, winner.sections[i].window_seconds)
                            .is_some_and(|stride| stride >= 2)
                })
        })
        .max_by(|(ia, a), (ib, b)| {
            a.global
                .score()
                .total_cmp(&b.global.score())
                .then(ib.cmp(ia))
        })
        .map(|(index, _)| index);
    add_critical(&mut critical, density);
    for index in critical {
        if !evidence.alternatives.contains(&pool[index]) {
            evidence.alternatives.push(copy_summary(&pool[index])?);
        }
    }
    for (side, index) in harmonic_indices.into_iter().enumerate() {
        evidence.harmonics[side].alternative_index = index.and_then(|index| {
            evidence
                .alternatives
                .iter()
                .position(|candidate| candidate == &pool[index])
        });
    }
    let ambiguous =
        global_rival.is_some_and(|index| pool[index].global.band().1 >= winner.global.band().0);
    evidence.probe_windows = probes(beats, duration, winner, &points, control)?;
    let base = 60.0 / winner.period_seconds;
    let integer = (base + 0.5).floor().clamp(30.0, 300.0);
    let pivot = (
        winner.pivot_ordinal,
        winner.pivot_seconds,
        winner.period_seconds,
    );
    evidence.fractional_check = check(
        &points,
        &evidence.probe_windows,
        pivot,
        base,
        winner.period_seconds,
        winner.anchor_seconds,
        control,
    )?;
    evidence.integer_check = check(
        &points,
        &evidence.probe_windows,
        pivot,
        integer,
        winner.period_seconds,
        winner.anchor_seconds,
        control,
    )?;
    evidence.integer_candidate_bpm = Some(integer);
    evidence.integer_preferred = evidence
        .fractional_check
        .as_ref()
        .is_some_and(|c| c.status == "pass")
        && evidence
            .integer_check
            .as_ref()
            .is_some_and(|c| c.status == "pass")
        && (integer - base).abs() <= 0.005;
    if evidence.probe_windows.len() == 2 {
        evidence.evidence_tier = if duration < 30.0 {
            "short_source"
        } else if winner.global.max_gap_seconds > 10.0_f64.max(8.0 * winner.period_seconds) {
            "gapped_support"
        } else {
            "two_ten_second"
        };
    }
    evidence.outcome = if evidence.candidate_limit_reached {
        "search_limited"
    } else if !evidence.conflicting_sections.is_empty() {
        "conflicting_sections"
    } else if ambiguous {
        "ambiguous"
    } else if eligible(winner, duration)
        && evidence
            .fractional_check
            .as_ref()
            .is_some_and(|check| check.status == "pass")
    {
        "supported"
    } else {
        "insufficient"
    };
    let bpm = (evidence.outcome == "supported").then_some(if evidence.integer_preferred {
        integer
    } else {
        base
    });
    evidence.winner = Some(winner_copy);
    drop(pool);
    drop(discovery);
    drop(attempts);
    drop(geometry);
    evidence.assignments = allocate(points.len())?;
    for (index, point) in points.iter().enumerate() {
        control.block(index)?;
        evidence.assignments.push((point.raw, point.ordinal));
    }
    control.check()?;
    Ok((bpm, evidence))
}

pub(super) fn recover(
    beats: &[f64],
    duration: f64,
    required_bytes: usize,
    available_bytes: usize,
    control: &Control<'_>,
) -> Result<(Option<f64>, OrdinalFit), super::Cancelled> {
    if control.cancelled() {
        return Err(super::Cancelled);
    }
    if required_bytes > available_bytes {
        return Ok((None, unavailable(required_bytes, available_bytes)));
    }
    if duration / MIN_PERIOD > EXACT_INTEGER {
        return Ok((None, OrdinalFit::Evaluated(empty_evidence())));
    }
    let evaluated = evaluate_control(beats, duration, control);
    if control.cancelled() {
        return Err(super::Cancelled);
    }
    match evaluated {
        Ok((bpm, evidence)) => Ok((bpm, OrdinalFit::Evaluated(evidence))),
        Err(SearchFailure::Cancelled) => Err(super::Cancelled),
        Err(SearchFailure::Allocation) => Ok((
            None,
            OrdinalFit::Budget(BudgetUnavailable {
                outcome: "allocation_unavailable",
                required_bytes,
                available_bytes,
            }),
        )),
    }
}

#[cfg(test)]
#[path = "inferred_ordinals_tests.rs"]
mod tests;
