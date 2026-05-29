use crate::{
    audio_engine::channels::map_channels,
    messages::{SampleAnalysis, SampleBuffer},
};
use stratum_dsp::{AnalysisConfig, AnalysisResult, analyze_audio};

const TEMPO_CANDIDATE_TOP_N: usize = 25;
const TEMPO_CANDIDATE_CONFIDENCE_THRESHOLD: f32 = 0.20;
const TEMPO_CANDIDATE_RAW_CLUSTER_RATIO: f64 = 0.030;
const TEMPO_CANDIDATE_FAMILY_RATIO: f64 = 0.030;
const TEMPO_CANDIDATE_DECISIVE_RATIO: f64 = 1.25;
const TEMPO_CANDIDATE_PREFERRED_MIN_BPM: f64 = 70.0;
const TEMPO_CANDIDATE_PREFERRED_MAX_BPM: f64 = 160.0;
const TEMPO_CANDIDATE_INTEGER_SNAP_TOLERANCE: f64 = 0.35;
const FIXED_TEMPO_TRANSIENT_THRESHOLD_RATIO: f32 = 0.35;
const FIXED_TEMPO_MIN_TRANSIENTS: usize = 16;
const FIXED_TEMPO_MIN_SPAN_S: f64 = 8.0;
const FIXED_TEMPO_MAX_REFERENCE_DEVIATION: f64 = 0.03;
const FIXED_TEMPO_MAX_RMS_RESIDUAL_S: f64 = 0.010;
const FIXED_TEMPO_MAX_INTERVAL_RESIDUAL_RATIO: f64 = 0.03;

/// Analyze audio using stratum-dsp.
pub fn analyze_sample(
    sample: &SampleBuffer,
    sample_rate_hz: u32,
) -> Result<SampleAnalysis, String> {
    let mono = map_channels(sample.samples.to_vec(), sample.channels, 1)
        .map_err(|err| format!("analysis failed: {err}"))?;

    let result = analyze_audio(&mono, sample_rate_hz, analysis_config())
        .map_err(|err| format!("analysis failed: {err}"))?;

    let candidate_bpm = candidate_family_consensus_bpm(&result).unwrap_or(result.bpm);
    let bpm = refined_fixed_tempo_bpm(&mono, sample_rate_hz, candidate_bpm)
        .unwrap_or_else(|| round_bpm_to_cent(f64::from(candidate_bpm)));

    Ok(SampleAnalysis {
        bpm,
        key: result.key.name(),
        beat_grid: result.beat_grid,
    })
}

fn analysis_config() -> AnalysisConfig {
    let mut config = AnalysisConfig::default();
    config.emit_tempogram_candidates = true;
    config.tempogram_candidates_top_n = TEMPO_CANDIDATE_TOP_N;
    config
}

#[derive(Debug, Clone, Copy)]
struct TempoCandidate {
    bpm: f64,
    score: f64,
}

#[derive(Debug, Clone, Copy)]
struct TempoCluster {
    bpm: f64,
    support: f64,
}

fn candidate_family_consensus_bpm(result: &AnalysisResult) -> Option<f32> {
    let candidates = result.metadata.tempogram_candidates.as_ref()?;
    let candidates = candidates
        .iter()
        .map(|candidate| TempoCandidate {
            bpm: f64::from(candidate.bpm),
            score: f64::from(candidate.score),
        })
        .collect::<Vec<_>>();

    tempo_candidate_family_consensus_bpm(
        f64::from(result.bpm),
        f64::from(result.bpm_confidence),
        &candidates,
    )
}

fn tempo_candidate_family_consensus_bpm(
    primary_bpm: f64,
    primary_confidence: f64,
    candidates: &[TempoCandidate],
) -> Option<f32> {
    if !primary_bpm.is_finite() || primary_bpm <= 0.0 {
        return None;
    }

    let raw_clusters = cluster_tempo_candidates(candidates);
    if raw_clusters.is_empty() {
        return None;
    }

    let families = group_tempo_families(&raw_clusters);
    let primary_family = families
        .iter()
        .position(|family| {
            family
                .iter()
                .any(|cluster| same_tempo_family(cluster.bpm, primary_bpm))
        })
        .unwrap_or(0);
    let best_family = families
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            tempo_family_support(left)
                .partial_cmp(&tempo_family_support(right))
                .unwrap_or(std::cmp::Ordering::Equal)
        })?
        .0;

    let primary_support = tempo_family_support(&families[primary_family]);
    let best_support = tempo_family_support(&families[best_family]);
    let primary_is_best = primary_family == best_family;
    let low_confidence = primary_confidence <= f64::from(TEMPO_CANDIDATE_CONFIDENCE_THRESHOLD);
    let decisive =
        primary_support <= 0.0 || best_support >= primary_support * TEMPO_CANDIDATE_DECISIVE_RATIO;
    if !primary_is_best && (!low_confidence || !decisive) {
        return None;
    }

    let bpm = canonical_tempo_family_bpm(&families[best_family])?;
    Some(round_bpm_to_cent(snap_bpm_to_near_integer(bpm)))
}

fn cluster_tempo_candidates(candidates: &[TempoCandidate]) -> Vec<TempoCluster> {
    let mut sorted = candidates
        .iter()
        .copied()
        .filter(|candidate| {
            candidate.bpm.is_finite()
                && candidate.bpm > 0.0
                && candidate.score.is_finite()
                && candidate.score > 0.0
        })
        .collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut clusters: Vec<TempoCluster> = Vec::new();
    for candidate in sorted {
        if clusters.iter().any(|cluster| {
            tempo_ratio_distance(cluster.bpm, candidate.bpm) <= TEMPO_CANDIDATE_RAW_CLUSTER_RATIO
        }) {
            continue;
        }
        clusters.push(TempoCluster {
            bpm: candidate.bpm,
            support: candidate.score,
        });
    }
    clusters
}

fn group_tempo_families(clusters: &[TempoCluster]) -> Vec<Vec<TempoCluster>> {
    let mut families: Vec<Vec<TempoCluster>> = Vec::new();
    for cluster in clusters {
        if let Some(family) = families.iter_mut().find(|family| {
            family
                .iter()
                .any(|existing| same_tempo_family(existing.bpm, cluster.bpm))
        }) {
            family.push(*cluster);
        } else {
            families.push(vec![*cluster]);
        }
    }
    families
}

fn tempo_family_support(family: &[TempoCluster]) -> f64 {
    family.iter().map(|cluster| cluster.support).sum()
}

fn canonical_tempo_family_bpm(family: &[TempoCluster]) -> Option<f64> {
    let direct = family
        .iter()
        .filter(|cluster| preferred_bpm_contains(cluster.bpm))
        .max_by(|left, right| {
            left.support
                .partial_cmp(&right.support)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    if let Some(cluster) = direct {
        return Some(cluster.bpm);
    }

    let mut weighted_bpm = 0.0;
    let mut total_support = 0.0;
    for cluster in family {
        let variant = preferred_bpm_variant(cluster.bpm)?;
        weighted_bpm += variant * cluster.support;
        total_support += cluster.support;
    }
    if total_support <= 0.0 {
        return None;
    }
    Some(weighted_bpm / total_support)
}

fn preferred_bpm_variant(bpm: f64) -> Option<f64> {
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    if preferred_bpm_contains(bpm) {
        return Some(bpm);
    }

    let mut best: Option<(f64, f64)> = None;
    for octave in -3..=3 {
        let candidate = bpm * 2.0_f64.powi(octave);
        if !preferred_bpm_contains(candidate) {
            continue;
        }
        let center = (TEMPO_CANDIDATE_PREFERRED_MIN_BPM + TEMPO_CANDIDATE_PREFERRED_MAX_BPM) * 0.5;
        let distance = (candidate - center).abs();
        if best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((candidate, distance));
        }
    }
    best.map(|(candidate, _)| candidate)
}

fn preferred_bpm_contains(bpm: f64) -> bool {
    (TEMPO_CANDIDATE_PREFERRED_MIN_BPM..=TEMPO_CANDIDATE_PREFERRED_MAX_BPM).contains(&bpm)
}

fn same_tempo_family(left_bpm: f64, right_bpm: f64) -> bool {
    if !left_bpm.is_finite() || !right_bpm.is_finite() || left_bpm <= 0.0 || right_bpm <= 0.0 {
        return false;
    }
    let octave = (left_bpm / right_bpm).log2().round();
    let normalized_right = right_bpm * 2.0_f64.powf(octave);
    tempo_ratio_distance(left_bpm, normalized_right) <= TEMPO_CANDIDATE_FAMILY_RATIO
}

fn tempo_ratio_distance(left: f64, right: f64) -> f64 {
    ((left - right).abs()) / left.max(right).max(f64::MIN_POSITIVE)
}

fn snap_bpm_to_near_integer(bpm: f64) -> f64 {
    let rounded = bpm.round();
    if (bpm - rounded).abs() <= TEMPO_CANDIDATE_INTEGER_SNAP_TOLERANCE {
        rounded
    } else {
        bpm
    }
}

#[derive(Debug, Clone, Copy)]
struct IntervalFit {
    interval_s: f64,
    rms_residual_s: f64,
}

fn refined_fixed_tempo_bpm(mono: &[f32], sample_rate_hz: u32, reference_bpm: f32) -> Option<f32> {
    if mono.is_empty() || sample_rate_hz == 0 || !reference_bpm.is_finite() || reference_bpm <= 0.0
    {
        return None;
    }

    let transient_starts =
        strong_transient_starts_s(mono, sample_rate_hz, f64::from(reference_bpm))?;
    if transient_starts.len() < FIXED_TEMPO_MIN_TRANSIENTS {
        return None;
    }

    let span_s = transient_starts.last()? - transient_starts.first()?;
    if span_s < FIXED_TEMPO_MIN_SPAN_S {
        return None;
    }

    let fit = fit_constant_interval(&transient_starts)?;
    let max_rms_residual_s = FIXED_TEMPO_MAX_RMS_RESIDUAL_S
        .max(fit.interval_s * FIXED_TEMPO_MAX_INTERVAL_RESIDUAL_RATIO);
    if fit.rms_residual_s > max_rms_residual_s {
        return None;
    }

    let fitted_bpm = 60.0 / fit.interval_s;
    let normalized_bpm = normalize_bpm_near_reference(fitted_bpm, f64::from(reference_bpm))?;
    let relative_deviation = (normalized_bpm - f64::from(reference_bpm)).abs()
        / f64::from(reference_bpm).max(f64::MIN_POSITIVE);
    if relative_deviation > FIXED_TEMPO_MAX_REFERENCE_DEVIATION {
        return None;
    }

    Some(round_bpm_to_cent(normalized_bpm))
}

fn strong_transient_starts_s(
    mono: &[f32],
    sample_rate_hz: u32,
    reference_bpm: f64,
) -> Option<Vec<f64>> {
    let max_abs = mono
        .iter()
        .map(|sample| sample.abs())
        .filter(|value| value.is_finite())
        .fold(0.0_f32, f32::max);
    if max_abs <= 0.0 {
        return None;
    }

    let threshold = max_abs * FIXED_TEMPO_TRANSIENT_THRESHOLD_RATIO;
    let reference_interval_s = 60.0 / reference_bpm;
    let min_gap_samples = transient_min_gap_samples(sample_rate_hz, reference_interval_s);
    let mut starts = Vec::new();
    let mut in_cluster = false;
    let mut last_start: Option<usize> = None;

    for (index, sample) in mono.iter().enumerate() {
        let above_threshold = sample.abs() >= threshold;
        if above_threshold && !in_cluster {
            let far_enough = match last_start {
                Some(previous) => index.saturating_sub(previous) >= min_gap_samples,
                None => true,
            };
            if far_enough {
                starts.push(index as f64 / f64::from(sample_rate_hz));
                last_start = Some(index);
            }
            in_cluster = true;
        } else if !above_threshold {
            in_cluster = false;
        }
    }

    Some(starts)
}

fn transient_min_gap_samples(sample_rate_hz: u32, reference_interval_s: f64) -> usize {
    let sample_rate = f64::from(sample_rate_hz);
    let floor = (sample_rate * 0.050).round().max(1.0) as usize;
    let ceiling = (sample_rate * 0.250).round().max(floor as f64) as usize;
    let desired = (reference_interval_s * sample_rate * 0.35).round().max(1.0) as usize;
    desired.clamp(floor, ceiling)
}

fn fit_constant_interval(times_s: &[f64]) -> Option<IntervalFit> {
    let count = times_s.len();
    if count < 2 {
        return None;
    }

    let mean_index = (count as f64 - 1.0) * 0.5;
    let mean_time = times_s.iter().sum::<f64>() / count as f64;
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for (index, time_s) in times_s.iter().enumerate() {
        let index_offset = index as f64 - mean_index;
        denominator += index_offset * index_offset;
        numerator += index_offset * (time_s - mean_time);
    }

    if denominator <= 0.0 {
        return None;
    }

    let interval_s = numerator / denominator;
    if !interval_s.is_finite() || interval_s <= 0.0 {
        return None;
    }

    let intercept_s = mean_time - interval_s * mean_index;
    let residual_sq = times_s
        .iter()
        .enumerate()
        .map(|(index, time_s)| {
            let expected_s = intercept_s + interval_s * index as f64;
            let residual_s = time_s - expected_s;
            residual_s * residual_s
        })
        .sum::<f64>();
    let rms_residual_s = (residual_sq / count as f64).sqrt();
    if !rms_residual_s.is_finite() {
        return None;
    }

    Some(IntervalFit {
        interval_s,
        rms_residual_s,
    })
}

fn normalize_bpm_near_reference(observed_bpm: f64, reference_bpm: f64) -> Option<f64> {
    if !observed_bpm.is_finite()
        || observed_bpm <= 0.0
        || !reference_bpm.is_finite()
        || reference_bpm <= 0.0
    {
        return None;
    }

    let mut best = observed_bpm;
    let mut best_error = (best / reference_bpm).ln().abs();
    for octave in -3..=3 {
        let candidate = observed_bpm * 2.0_f64.powi(octave);
        if !candidate.is_finite() || candidate <= 0.0 {
            continue;
        }
        let error = (candidate / reference_bpm).ln().abs();
        if error < best_error {
            best = candidate;
            best_error = error;
        }
    }

    Some(best)
}

fn round_bpm_to_cent(bpm: f64) -> f32 {
    ((bpm * 100.0).round() / 100.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_tempo_refinement_recovers_exact_metronome_bpm() {
        let sample_rate_hz = 44_100;
        let samples = synthetic_click_track(sample_rate_hz, 120.0, 240);

        let bpm = refined_fixed_tempo_bpm(&samples, sample_rate_hz, 119.86).expect("refined bpm");

        assert!((bpm - 120.0).abs() < 0.005);
    }

    #[test]
    fn fixed_tempo_refinement_rejects_sparse_transients() {
        let sample_rate_hz = 44_100;
        let samples = synthetic_click_track(sample_rate_hz, 120.0, 8);

        assert!(refined_fixed_tempo_bpm(&samples, sample_rate_hz, 119.86).is_none());
    }

    #[test]
    fn fixed_tempo_refinement_rejects_unstable_transients() {
        let sample_rate_hz = 44_100;
        let mut samples = vec![0.0; sample_rate_hz as usize * 30];
        let mut position_s = 0.0;
        for beat in 0..48 {
            write_click(&mut samples, sample_rate_hz, position_s);
            position_s += if beat % 2 == 0 { 0.50 } else { 0.62 };
        }

        assert!(refined_fixed_tempo_bpm(&samples, sample_rate_hz, 119.86).is_none());
    }

    #[test]
    fn tempo_candidate_consensus_corrects_half_time_primary() {
        let candidates = [
            TempoCandidate {
                bpm: 44.86,
                score: 0.6243,
            },
            TempoCandidate {
                bpm: 72.00,
                score: 0.5986,
            },
            TempoCandidate {
                bpm: 118.00,
                score: 0.5970,
            },
            TempoCandidate {
                bpm: 179.90,
                score: 0.5322,
            },
            TempoCandidate {
                bpm: 89.95,
                score: 0.3611,
            },
        ];

        let bpm = tempo_candidate_family_consensus_bpm(44.86, 0.04, &candidates)
            .expect("candidate consensus bpm");

        assert!((bpm - 90.0).abs() < 0.005);
    }

    #[test]
    fn tempo_candidate_consensus_outvotes_subdivision_primary() {
        let candidates = [
            TempoCandidate {
                bpm: 110.08,
                score: 0.7692,
            },
            TempoCandidate {
                bpm: 43.84,
                score: 0.7625,
            },
            TempoCandidate {
                bpm: 176.00,
                score: 0.6871,
            },
            TempoCandidate {
                bpm: 154.24,
                score: 0.6861,
            },
            TempoCandidate {
                bpm: 88.32,
                score: 0.6149,
            },
            TempoCandidate {
                bpm: 86.50,
                score: 0.6080,
            },
        ];

        let bpm = tempo_candidate_family_consensus_bpm(110.08, 0.01, &candidates)
            .expect("candidate consensus bpm");

        assert!((bpm - 88.0).abs() < 0.005);
    }

    #[test]
    fn tempo_candidate_consensus_keeps_high_confidence_primary() {
        let candidates = [
            TempoCandidate {
                bpm: 124.0,
                score: 0.90,
            },
            TempoCandidate {
                bpm: 82.0,
                score: 0.95,
            },
        ];

        assert!(tempo_candidate_family_consensus_bpm(124.0, 0.75, &candidates).is_none());
    }

    fn synthetic_click_track(sample_rate_hz: u32, bpm: f64, beats: usize) -> Vec<f32> {
        let interval_s = 60.0 / bpm;
        let duration_s = interval_s * beats as f64 + 1.0;
        let mut samples = vec![0.0; (duration_s * f64::from(sample_rate_hz)).ceil() as usize];
        for beat in 0..beats {
            write_click(&mut samples, sample_rate_hz, interval_s * beat as f64);
        }
        samples
    }

    fn write_click(samples: &mut [f32], sample_rate_hz: u32, start_s: f64) {
        let start = (start_s * f64::from(sample_rate_hz)).round() as usize;
        for offset in 0..64 {
            let index = start + offset;
            if index >= samples.len() {
                return;
            }
            samples[index] = 1.0 - offset as f32 / 64.0;
        }
    }
}
