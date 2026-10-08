//! Bounded selected-backend aggregation and representative metadata, offline.
//! Complete OLS, global G2 diagnostics and local region fits retain separate
//! authority. No source binding, musical certification or runtime adoption.

mod regions;
mod types;

pub use types::*;

use crate::tempo_summary::{
    MAX_RAW_POSITIONS, QuarterNoteHypothesis, QuarterNoteVerification, TempoSummaryError,
    fit::diagnose_numerical,
    math::{Line, Point, least_squares},
};

/// Summarize every selected-backend position without cropping inference.
/// Explicit rational counts remain an unverified caller assertion. Ordinal
/// input never infers missing beats, excludes extras or rounds toward integers.
pub fn summarize_selected_bpm(
    input: SelectedBpmInput<'_>,
) -> Result<SelectedBpmSummary, TempoSummaryError> {
    validate(input)?;
    let counts: Vec<_> = input.quarter_counts.map_or_else(
        || {
            (0..input.beat_seconds.len())
                .map(|i| Some(i as i64))
                .collect()
        },
        <[Option<i64>]>::to_vec,
    );
    let points: Vec<_> = counts
        .iter()
        .enumerate()
        .filter_map(|(raw_index, count)| {
            count.map(|quarter| Point {
                raw_index,
                quarter,
                seconds: input.beat_seconds[raw_index],
            })
        })
        .collect();
    let complete_line = least_squares(&points, TIMING_HALFWIDTH_SECONDS);
    let complete_fit =
        complete_line.and_then(|line| aggregate_fit(line, &points, input.quarter_note_denominator));
    let mut complete_residual_seconds = vec![None; counts.len()];
    if let Some(line) = complete_line.filter(|_| complete_fit.is_some()) {
        for &point in &points {
            complete_residual_seconds[point.raw_index] = Some(line.residual(point));
        }
    }
    let duration = input.frame_count as f64 / f64::from(input.sample_rate_hz);
    let global = diagnose_numerical(
        input.beat_seconds,
        duration,
        TIMING_HALFWIDTH_SECONDS,
        &QuarterNoteHypothesis {
            id: "selected-backend-counts-v1",
            provenance: input.count_provenance,
            verification: QuarterNoteVerification::Unverified,
            quarter_note_denominator: input.quarter_note_denominator,
            quarter_counts: &counts,
        },
    );
    let regions = regions::evaluate(
        input.beat_seconds,
        &points,
        duration,
        input.quarter_note_denominator,
    );
    let selected = regions::select(&regions);
    let selected_region_id = selected.map(|region| region.id);
    let representative_bpm = selected.and_then(|region| region.fit.as_ref().map(|fit| fit.bpm));
    let alternatives_bpm = [0.5, 1.0, 2.0].map(|factor| {
        complete_fit
            .as_ref()
            .and_then(|fit| finite_positive(fit.bpm * factor))
    });
    let local_interval_seconds: Vec<_> =
        input.beat_seconds.windows(2).map(|w| w[1] - w[0]).collect();
    let local_interval_bpm = local_interval_seconds
        .iter()
        .enumerate()
        .map(|(index, interval)| {
            counts[index]
                .zip(counts[index + 1])
                .and_then(|(first, last)| {
                    finite_positive(
                        60.0 * (last - first) as f64
                            / f64::from(input.quarter_note_denominator)
                            / interval,
                    )
                })
        })
        .collect();
    Ok(SelectedBpmSummary {
        beat_unit: if input.quarter_counts.is_some() {
            "explicit-quarter-note-count-assertion"
        } else {
            "quarter-note-assumption"
        },
        count_provenance: input.count_provenance.to_owned(),
        sample_rate_hz: input.sample_rate_hz,
        frame_count: input.frame_count,
        raw_beat_seconds: input.beat_seconds.to_vec(),
        quarter_counts: counts,
        quarter_note_denominator: input.quarter_note_denominator,
        local_interval_seconds,
        local_interval_bpm,
        complete_fit,
        complete_residual_seconds,
        global,
        regions,
        selected_region_id,
        representative_bpm,
        alternatives_bpm,
    })
}

pub(crate) fn finite_positive(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

fn validate(input: SelectedBpmInput<'_>) -> Result<(), TempoSummaryError> {
    if !(8_000..=768_000).contains(&input.sample_rate_hz)
        || input.frame_count == 0
        || input.frame_count > (1_u64 << 53)
        || !(1..=64).contains(&input.quarter_note_denominator)
        || input.count_provenance.trim().is_empty()
        || input.count_provenance.len() > 4096
        || input.beat_seconds.len() > MAX_RAW_POSITIONS
        || (input.quarter_counts.is_none() && input.quarter_note_denominator != 1)
    {
        return Err(TempoSummaryError(
            "invalid selected BPM dimensions or count policy",
        ));
    }
    let duration = input.frame_count as f64 / f64::from(input.sample_rate_hz);
    let mut previous = -1.0;
    for &time in input.beat_seconds {
        if !time.is_finite() || time < 0.0 || time >= duration || time <= previous {
            return Err(TempoSummaryError(
                "invalid or unordered complete beat positions",
            ));
        }
        previous = time;
    }
    if let Some(counts) = input.quarter_counts {
        if counts.len() != input.beat_seconds.len() {
            return Err(TempoSummaryError(
                "explicit count extent differs from complete raw sequence",
            ));
        }
        let mut previous = None;
        let mut first = None;
        for &count in counts.iter().flatten() {
            if count.unsigned_abs() > (1_u64 << 52)
                || previous.is_some_and(|prior| count <= prior)
                || first.is_some_and(|start| count - start > (1_i64 << 52))
            {
                return Err(TempoSummaryError(
                    "invalid or unordered quarter-note counts",
                ));
            }
            first.get_or_insert(count);
            previous = Some(count);
        }
    }
    Ok(())
}

pub(crate) fn numerical_tolerance(points: &[Point]) -> f64 {
    points.last().map_or(1.0, |point| point.seconds.max(1.0)) * 64.0 * f64::EPSILON
}

pub(crate) fn aggregate_fit(
    line: Line,
    points: &[Point],
    denominator: u32,
) -> Option<AggregateFit> {
    let period = finite_positive(line.period * f64::from(denominator))?;
    let bpm = finite_positive(60.0 / period)?;
    let (minimum, maximum) =
        points
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |range, point| {
                let residual = line.residual(*point);
                (range.0.min(residual), range.1.max(residual))
            });
    let intercept = line.mean_seconds - line.period * (line.reference as f64 + line.mean_quarters);
    if !minimum.is_finite() || !maximum.is_finite() || !intercept.is_finite() {
        return None;
    }
    Some(AggregateFit {
        assigned_observations: points.len(),
        period_seconds_per_quarter: period,
        bpm,
        reference_count_numerator: line.reference,
        quarter_note_denominator: denominator,
        fitted_seconds_at_reference: line.seconds_at(line.reference),
        diagnostic_intercept_seconds: intercept,
        period_sensitivity_bound_seconds: line.sensitivity * f64::from(denominator),
        max_abs_residual_seconds: minimum.abs().max(maximum.abs()),
        residual_range_seconds: maximum - minimum,
        numerical_tolerance_seconds: numerical_tolerance(points),
    })
}
