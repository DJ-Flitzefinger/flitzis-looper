//! Bounded offline constant-period evidence; no analyzer or runtime publication.
//!
//! Policy v1 requires at least 24 raw positions, at least six supported positions
//! in each complete-source temporal third, and at least 60% duration coverage.
//! It permits at most 10% excluded raw positions and at most two consecutively.
//! Counts and their musical interpretation are supplied explicitly; this module
//! never infers quarter units or declares accepted musical timing.

mod fit;
mod math;
mod types;

pub use types::*;

/// Derive bounded source-bound diagnostics from complete immutable raw evidence.
///
/// Positions must be strictly increasing source seconds. Hypotheses must retain
/// one count/exclusion for every raw position, with increasing assigned counts.
/// Distinct viable count interpretations remain ambiguous, even if one is verified.
pub fn summarize_constant_tempo(
    evidence: &RawTempoEvidence<'_>,
    hypotheses: &[QuarterNoteHypothesis<'_>],
) -> Result<TempoSummary, TempoSummaryError> {
    validate(evidence, hypotheses)?;
    let diagnostics: Vec<_> = hypotheses
        .iter()
        .map(|hypothesis| fit::diagnose(evidence, hypothesis))
        .collect();
    let viable: Vec<_> = diagnostics
        .iter()
        .enumerate()
        .filter(|(_, diagnostic)| diagnostic.status != HypothesisStatus::Unsupported)
        .map(|(index, _)| index)
        .collect();
    let ambiguous = viable
        .iter()
        .skip(1)
        .any(|&index| !equivalent_counts(&hypotheses[viable[0]], &hypotheses[index]));
    let supported = viable
        .iter()
        .copied()
        .find(|&index| diagnostics[index].status == HypothesisStatus::SupportedCandidate);
    let (status, supported_hypothesis_index) = if ambiguous {
        (SummaryStatus::Ambiguous, None)
    } else if let Some(index) = supported {
        (SummaryStatus::SupportedCandidate, Some(index))
    } else if viable.is_empty() {
        (SummaryStatus::Unsupported, None)
    } else {
        (SummaryStatus::Unverified, None)
    };
    Ok(TempoSummary {
        source: evidence.source.clone(),
        independent_origin_seconds: evidence.independent_origin_seconds,
        policy_version: POLICY_VERSION,
        raw_position_count: evidence.beat_seconds.len(),
        raw_beat_seconds: evidence.beat_seconds.to_vec(),
        status,
        supported_hypothesis_index,
        hypotheses: diagnostics,
    })
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate(
    evidence: &RawTempoEvidence<'_>,
    hypotheses: &[QuarterNoteHypothesis<'_>],
) -> Result<(), TempoSummaryError> {
    let source = evidence.source;
    if !valid_digest(&source.source_sha256)
        || !valid_digest(&source.pcm_sha256)
        || !valid_text(&source.backend_revision)
        || !valid_text(&source.configuration_revision)
        || !valid_text(&source.raw_revision)
        || !(8_000..=768_000).contains(&source.loaded_sample_rate_hz)
        || source.loaded_frame_count == 0
        || source.loaded_frame_count > (1_u64 << 53)
        || !source.timing_error_halfwidth_seconds.is_finite()
        || source.timing_error_halfwidth_seconds < 0.0
        || !evidence.independent_origin_seconds.is_finite()
    {
        return Err(TempoSummaryError("invalid source identity or timing bound"));
    }
    if evidence.beat_seconds.len() > MAX_RAW_POSITIONS
        || hypotheses.is_empty()
        || hypotheses.len() > MAX_HYPOTHESES
    {
        return Err(TempoSummaryError(
            "raw position or hypothesis limit exceeded",
        ));
    }
    let duration = source.loaded_frame_count as f64 / source.loaded_sample_rate_hz as f64;
    if source.timing_error_halfwidth_seconds > duration {
        return Err(TempoSummaryError("timing bound exceeds source duration"));
    }
    let mut previous_time = -1.0;
    for &time in evidence.beat_seconds {
        if !time.is_finite() || time < 0.0 || time >= duration || time <= previous_time {
            return Err(TempoSummaryError(
                "invalid or unordered raw source positions",
            ));
        }
        previous_time = time;
    }
    for (index, hypothesis) in hypotheses.iter().enumerate() {
        if hypotheses[..index]
            .iter()
            .any(|earlier| earlier.id == hypothesis.id)
        {
            return Err(TempoSummaryError("duplicate hypothesis identity"));
        }
        if !valid_text(hypothesis.id)
            || !valid_text(hypothesis.provenance)
            || !(1..=64).contains(&hypothesis.quarter_note_denominator)
            || hypothesis.quarter_counts.len() != evidence.beat_seconds.len()
        {
            return Err(TempoSummaryError(
                "invalid count hypothesis identity or extent",
            ));
        }
        let mut previous_count = None;
        let mut first_count = None;
        for &count in hypothesis.quarter_counts.iter().flatten() {
            if count.unsigned_abs() > (1_u64 << 52)
                || previous_count.is_some_and(|previous| count <= previous)
                || first_count.is_some_and(|first| count - first > (1_i64 << 52))
            {
                return Err(TempoSummaryError(
                    "invalid or unordered quarter-note counts",
                ));
            }
            first_count.get_or_insert(count);
            previous_count = Some(count);
        }
    }
    Ok(())
}

fn equivalent_counts(left: &QuarterNoteHypothesis<'_>, right: &QuarterNoteHypothesis<'_>) -> bool {
    let mut offset = None;
    left.quarter_counts
        .iter()
        .zip(right.quarter_counts)
        .all(
            |(left_count, right_count)| match (left_count, right_count) {
                (None, None) => true,
                (Some(left_count), Some(right_count)) => {
                    let difference = i128::from(*left_count)
                        * i128::from(right.quarter_note_denominator)
                        - i128::from(*right_count) * i128::from(left.quarter_note_denominator);
                    *offset.get_or_insert(difference) == difference
                }
                _ => false,
            },
        )
}
