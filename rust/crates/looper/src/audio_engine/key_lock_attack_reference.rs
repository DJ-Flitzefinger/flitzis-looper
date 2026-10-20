//! Bounded worker-only correspondence to an independently declared source attack.
//!
//! This produces spectral-feature candidates, never perceived-attack acceptance.
//! The caller selects the source event before inspecting wet output and binds both
//! buffers to its exact source/rate/pitch/history/reset/block-phase contract.
//! ComplexSD novelty is computed independently per channel, then pooled without
//! phase-cancelling mono conversion. A supported match is not an isolated-response
//! retention proof or permission to replace the original acoustic criteria.

use super::source_reader::FrameRange;
use flitzis_looper_analysis::{AnalysisConfig, DetectionFunction};

const MAX_FRAMES: usize = 262_144;
const MAX_CHANNELS: usize = 8;
const MAX_CORRELATION_TERMS: usize = 16_777_216;
const MIN_PATTERN_FEATURES: usize = 8;
const FEATURE_STEP_SECONDS: f64 = 0.001;
const FEATURE_BIN_HZ: f64 = 50.0;

// Fixed engineering distinguishability criteria, not musical/listening gates.
// They neither replace nor relax the existing dry-timing/retention thresholds.
const MIN_CORRELATION_SCORE: f64 = 0.75;
const MIN_INDEPENDENT_MATCH_MARGIN: f64 = 0.10;
const SUPPORT_SCORE_LOSS: f64 = 0.025;
const MIN_RELATIVE_FEATURE_RANGE: f64 = 0.05;
pub(crate) const REVISION: &str = "source-declared-channel-complexsd-correspondence-v1";

#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceAttackReference {
    /// Independently declared event coordinate in the supplied dry frame domain.
    pub frame: usize,
    /// Complete local pattern, including real analysis-window/history support.
    pub support: FrameRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AttackLagRange {
    pub min: i64,
    pub max: i64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AttackCorrespondenceCandidate {
    pub source_frame: usize,
    pub wet_frame: i64,
    pub lag_frames: i64,
    /// Conservative feature-support extent, not a statistical confidence interval
    /// or a bound on perceived attack error. Includes both windows and hop phase.
    pub lag_support: AttackLagRange,
    pub score: f64,
    /// Best independent competing match; -1 means none outside the support lobe.
    pub runner_up_score: f64,
    pub feature_hop_frames: usize,
    pub feature_window_frames: usize,
    pub revision: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AttackCorrespondence {
    SupportedCandidate(AttackCorrespondenceCandidate),
    Ambiguous,
    Absent,
    Unavailable,
}

struct FeatureGeometry {
    hop: usize,
    window: usize,
    first: usize,
    end: usize,
    first_lag: i64,
    last_lag: i64,
}

/// Compare one declared source pattern against uncropped continuous wet output.
///
/// All allocation, FFT and search work belongs to a non-realtime worker. Bounds
/// and the complete requested search extent are checked before feature work.
/// Lags are searched on the actual integer ODF hop. The returned support retains
/// that quantization and window ambiguity instead of claiming sample precision.
pub(crate) fn compare_attack_reference(
    dry: &[Vec<f32>],
    wet: &[Vec<f32>],
    sample_rate_hz: u32,
    reference: SourceAttackReference,
    search: AttackLagRange,
) -> AttackCorrespondence {
    let Some(geometry) = validate(dry, wet, sample_rate_hz, reference, search) else {
        return AttackCorrespondence::Unavailable;
    };
    let Some(dry_features) = pooled_novelty(dry, sample_rate_hz, &geometry) else {
        return AttackCorrespondence::Unavailable;
    };
    let Some(wet_features) = pooled_novelty(wet, sample_rate_hz, &geometry) else {
        return AttackCorrespondence::Unavailable;
    };
    let Some(pattern) = CenteredPattern::new(&dry_features[geometry.first..geometry.end]) else {
        return AttackCorrespondence::Absent;
    };

    let mut scores = Vec::with_capacity((geometry.last_lag - geometry.first_lag + 1) as usize);
    let mut best = None;
    for lag in geometry.first_lag..=geometry.last_lag {
        let start = (geometry.first as i64 + lag) as usize;
        let end = start + pattern.values.len();
        let score = pattern.score(&wet_features[start..end]);
        if let Some(score) = score
            && best.is_none_or(|(_, previous)| score > previous)
        {
            best = Some((lag, score));
        }
        scores.push((lag, score));
    }
    let Some((best_lag, best_score)) = best else {
        return AttackCorrespondence::Absent;
    };
    if best_score < MIN_CORRELATION_SCORE {
        return AttackCorrespondence::Absent;
    }

    // Correlated adjacent offsets describe one finite-resolution feature lobe.
    // A separated comparable lobe is a competing event, never an arbitrary tie.
    let lobe_frames = geometry.window + 2 * geometry.hop;
    let runner_up_score = scores
        .iter()
        .filter(|(lag, _)| {
            (lag - best_lag).unsigned_abs() * geometry.hop as u64 > lobe_frames as u64
        })
        .filter_map(|(_, score)| *score)
        .fold(-1.0_f64, f64::max);
    if best_score - runner_up_score < MIN_INDEPENDENT_MATCH_MARGIN {
        return AttackCorrespondence::Ambiguous;
    }

    let mut support_min = best_lag;
    let mut support_max = best_lag;
    for &(lag, score) in &scores {
        if score.is_some_and(|score| score >= best_score - SUPPORT_SCORE_LOSS) {
            support_min = support_min.min(lag);
            support_max = support_max.max(lag);
        }
    }
    // Difference of two forward-window supports plus one hop of grid phase.
    // Do not clip this support to a requested search bound to manufacture precision.
    let support_radius = (geometry.window + 3 * geometry.hop) as i64;
    let lag_support = AttackLagRange {
        min: support_min * geometry.hop as i64 - support_radius,
        max: support_max * geometry.hop as i64 + support_radius,
    };
    if best_lag == geometry.first_lag
        || best_lag == geometry.last_lag
        || lag_support.min < search.min
        || lag_support.max > search.max
    {
        return AttackCorrespondence::Ambiguous;
    }
    let lag_frames = best_lag * geometry.hop as i64;
    AttackCorrespondence::SupportedCandidate(AttackCorrespondenceCandidate {
        source_frame: reference.frame,
        wet_frame: reference.frame as i64 + lag_frames,
        lag_frames,
        lag_support,
        score: best_score,
        runner_up_score,
        feature_hop_frames: geometry.hop,
        feature_window_frames: geometry.window,
        revision: REVISION,
    })
}

fn feature_config() -> AnalysisConfig {
    AnalysisConfig {
        step_secs: FEATURE_STEP_SECONDS,
        max_bin_hz: FEATURE_BIN_HZ,
        ..AnalysisConfig::default()
    }
}

fn validate(
    dry: &[Vec<f32>],
    wet: &[Vec<f32>],
    rate: u32,
    reference: SourceAttackReference,
    search: AttackLagRange,
) -> Option<FeatureGeometry> {
    if !(8_000..=768_000).contains(&rate)
        || dry.is_empty()
        || dry.len() > MAX_CHANNELS
        || wet.len() != dry.len()
        || search.min > search.max
    {
        return None;
    }
    let frames = dry[0].len();
    if frames == 0
        || frames > MAX_FRAMES
        || dry.iter().chain(wet).any(|channel| {
            channel.len() != frames || channel.iter().any(|sample| !sample.is_finite())
        })
        || reference.support.start >= reference.support.end
        || reference.support.end > frames
        || !(reference.support.start..reference.support.end).contains(&reference.frame)
        || search.min < -(MAX_FRAMES as i64)
        || search.max > MAX_FRAMES as i64
        || reference.support.start as i64 + search.min < 0
        || reference.support.end as i64 + search.max > frames as i64
    {
        return None;
    }
    let detector = DetectionFunction::new(rate, &feature_config());
    let hop = detector.step_size_samples();
    let window = detector.frame_length_samples();
    let event_context = window + 2 * hop;
    if reference.frame.checked_sub(reference.support.start)? < event_context
        || reference.support.end.checked_sub(reference.frame)? < event_context
    {
        // An event at an input/support edge has no complete attack neighborhood.
        return None;
    }
    // A ComplexSD value needs its current forward Hann window plus two actual
    // preceding phase windows. No zero-padded source/wet edge enters the pattern.
    let first = reference.support.start.checked_add(2 * hop)?.div_ceil(hop);
    let end = reference.support.end.checked_sub(window)? / hop + 1;
    if end.checked_sub(first)? < MIN_PATTERN_FEATURES {
        return None;
    }
    let first_lag =
        search.min.div_euclid(hop as i64) + i64::from(search.min.rem_euclid(hop as i64) != 0);
    let last_lag = search.max.div_euclid(hop as i64);
    let lag_count = usize::try_from(last_lag.checked_sub(first_lag)?.checked_add(1)?).ok()?;
    if lag_count == 0
        || lag_count.checked_mul(end - first)? > MAX_CORRELATION_TERMS
        || first as i64 + first_lag < 2
        || (end as i64 - 1 + last_lag) * hop as i64 + window as i64 > frames as i64
    {
        return None;
    }
    Some(FeatureGeometry {
        hop,
        window,
        first,
        end,
        first_lag,
        last_lag,
    })
}

fn pooled_novelty(
    channels: &[Vec<f32>],
    rate: u32,
    geometry: &FeatureGeometry,
) -> Option<Vec<f64>> {
    let mut pooled = Vec::new();
    for channel in channels {
        let input: Vec<f64> = channel.iter().map(|&sample| f64::from(sample)).collect();
        let mut detector = DetectionFunction::new(rate, &feature_config());
        debug_assert_eq!(detector.step_size_samples(), geometry.hop);
        debug_assert_eq!(detector.frame_length_samples(), geometry.window);
        let features = detector.process(&input);
        if pooled.is_empty() {
            pooled.resize(features.len(), 0.0);
        }
        if pooled.len() != features.len() {
            return None;
        }
        for (sum, feature) in pooled.iter_mut().zip(features) {
            if !feature.is_finite() || feature < 0.0 {
                return None;
            }
            *sum += feature;
        }
    }
    pooled
        .iter()
        .all(|value| value.is_finite())
        .then_some(pooled)
}

struct CenteredPattern {
    values: Vec<f64>,
    energy: f64,
}

impl CenteredPattern {
    fn new(features: &[f64]) -> Option<Self> {
        let scale = features.iter().copied().fold(0.0_f64, f64::max);
        if scale <= 0.0 {
            return None;
        }
        let minimum = features.iter().copied().fold(f64::INFINITY, f64::min);
        if (scale - minimum) / scale < MIN_RELATIVE_FEATURE_RANGE {
            return None;
        }
        let mean = features.iter().map(|value| value / scale).sum::<f64>() / features.len() as f64;
        let values: Vec<_> = features.iter().map(|value| value / scale - mean).collect();
        let energy = values.iter().map(|value| value * value).sum::<f64>();
        (energy > 0.0 && energy.is_finite()).then_some(Self { values, energy })
    }

    fn score(&self, candidate: &[f64]) -> Option<f64> {
        let scale = candidate.iter().copied().fold(0.0_f64, f64::max);
        if scale <= 0.0 {
            return None;
        }
        let mean =
            candidate.iter().map(|value| value / scale).sum::<f64>() / candidate.len() as f64;
        let mut dot = 0.0;
        let mut energy = 0.0;
        for (reference, feature) in self.values.iter().zip(candidate) {
            let centered = feature / scale - mean;
            dot += reference * centered;
            energy += centered * centered;
        }
        let denominator = (self.energy * energy).sqrt();
        (denominator > 0.0 && denominator.is_finite()).then(|| (dot / denominator).clamp(-1.0, 1.0))
    }
}

#[cfg(test)]
#[path = "key_lock_attack_reference_tests.rs"]
mod tests;
