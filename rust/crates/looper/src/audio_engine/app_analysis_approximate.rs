//! Bounded off-RT scalar fallback. No beat/phase or precise acceptance authority.
use super::qm_corroboration::{Choice, Failure};
use super::*;
use flitzis_looper_analysis::{AnalysisConfig, analyze_bpm_raw, calculate_bpm};

const RESERVE: usize = 2 * 1024 * 1024;
const MAX_QM_BEATS: usize = 250_000;

pub(super) fn valid_bpm(bpm: f64) -> bool {
    bpm.is_finite() && (30.0..=300.0).contains(&bpm)
}

pub(super) fn precise_header(tree: &Value) -> serde_json::Map<String, Value> {
    ["schema_version", "backend", "policy", "status", "bpm"]
        .into_iter()
        .map(|key| (key.to_owned(), tree[key].clone()))
        .collect()
}

fn check(cancelled: &impl Fn() -> bool) -> Result<(), Failure> {
    if cancelled() {
        Err(Failure::Cancelled)
    } else {
        Ok(())
    }
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_unstable_by(f64::total_cmp);
    let n = values.len();
    if n.is_multiple_of(2) {
        (values[n / 2 - 1] + values[n / 2]) * 0.5
    } else {
        values[n / 2]
    }
}

fn intervals(beats: &[Value]) -> Option<f64> {
    let mut values: Vec<_> = beats
        .windows(2)
        .filter_map(|pair| {
            let value = pair[1].as_f64()? - pair[0].as_f64()?;
            (0.2..=2.0).contains(&value).then_some(value)
        })
        .collect();
    if values.len() < 8 || values.len() * 2 < beats.len().saturating_sub(1) {
        return None;
    }
    let period = median(&mut values);
    let support = values
        .iter()
        .filter(|&&v| (v - period).abs() <= 0.15 * period)
        .count();
    (support * 5 >= values.len() * 3).then_some(period)
}

fn neural(tree: &Value) -> Option<(&'static str, f64)> {
    let fit = &tree["ordinal_fit"];
    if tree["schema_version"] == 9
        && fit["candidate_limit_reached"] == false
        && fit["evidence_tier"]
            .as_str()
            .is_some_and(|v| v != "inconclusive")
        && fit["assignments"].as_array().is_some_and(|v| v.len() >= 8)
    {
        if let Some(period) = fit["winner"]["period_seconds"]
            .as_f64()
            .filter(|v| v.is_finite() && (0.2..=2.0).contains(v))
        {
            return Some(("neural_period", period));
        }
    }
    intervals(tree["raw_beats"].as_array()?).map(|v| ("neural_intervals", v))
}

fn energy(
    sample: &CompletePcm<'_>,
    rate: u32,
    cancelled: &impl Fn() -> bool,
) -> Result<Option<(f64, Value)>, Failure> {
    let frames = sample.frame_count();
    let channels = sample.channels;
    let hop = rate as usize / 100;
    if channels == 0 || hop == 0 || frames.checked_mul(channels) != Some(sample.samples.len()) {
        return Ok(None);
    }
    let width = (frames / hop).min(2000);
    if width < 800 {
        return Ok(None);
    }
    let remaining = frames - width * hop;
    let mut starts = vec![0, remaining / 2, remaining];
    starts.dedup();
    let mut windows = Vec::new();
    let mut best = (0.0_f64, 0_usize);
    for start in starts {
        let mut novelty = Vec::with_capacity(width);
        let mut previous = 0.0_f64;
        let mut total_level = 0.0_f64;
        for bin in 0..width {
            check(cancelled)?;
            let mut sum = 0.0;
            let begin = (start + bin * hop) * channels;
            for &value in &sample.samples[begin..begin + hop * channels] {
                if !value.is_finite() {
                    return Ok(None);
                }
                sum += f64::from(value).abs();
            }
            let level = sum / (hop * channels) as f64;
            total_level += level;
            novelty.push((level - previous).max(0.0));
            previous = level;
        }
        let peak = novelty.iter().copied().fold(0.0_f64, f64::max);
        if peak > 1e-7 && novelty.iter().filter(|&&v| v > peak * 0.2).count() >= 4 {
            let smooth: Vec<f64> = (0..width)
                .map(|i| {
                    let mut sum = 0.0;
                    for (offset, weight) in [
                        (0_isize, 1.0),
                        (1, 2.0),
                        (2, 3.0),
                        (3, 4.0),
                        (4, 5.0),
                        (5, 4.0),
                        (6, 3.0),
                        (7, 2.0),
                        (8, 1.0),
                    ] {
                        let j = i as isize + offset - 4;
                        if j >= 0 && (j as usize) < width {
                            sum += novelty[j as usize] * weight;
                        }
                    }
                    sum / 25.0
                })
                .collect();
            let mean_level = total_level / width as f64;
            if smooth.iter().copied().fold(0.0_f64, f64::max) < 0.1 * mean_level {
                windows
                    .push(json!({"start_frame":start,"novelty":novelty,"mean_level":mean_level}));
                continue;
            }
            let center = smooth.iter().sum::<f64>() / width as f64;
            let values: Vec<_> = smooth.iter().map(|v| v - center).collect();
            let mut scores = Vec::new();
            for lag in 20..=200.min(width / 4) {
                check(cancelled)?;
                let (mut cross, mut left, mut right) = (0.0, 0.0, 0.0);
                for i in lag..width {
                    cross += values[i] * values[i - lag];
                    left += values[i] * values[i];
                    right += values[i - lag] * values[i - lag];
                }
                let score = if left * right > 0.0 {
                    cross / (left * right).sqrt()
                } else {
                    0.0
                };
                scores.push(score);
            }
            let maximum = scores.iter().copied().fold(0.0_f64, f64::max);
            let local = scores
                .iter()
                .enumerate()
                .find(|&(i, &score)| {
                    score >= 0.35_f64.max(maximum * 0.95)
                        && (i == 0 || score >= scores[i - 1])
                        && (i + 1 == scores.len() || score >= scores[i + 1])
                })
                .map(|(i, &score)| (score, i + 20));
            if let Some(local) = local {
                if local.0 > best.0 {
                    best = local;
                }
            }
        }
        windows.push(
            json!({"start_frame":start,"novelty":novelty,"mean_level":total_level / width as f64}),
        );
    }
    check(cancelled)?;
    Ok((best.1 != 0).then(|| (best.1 as f64 * hop as f64 / f64::from(rate), json!(windows))))
}

pub(super) fn attempt(
    sample: &CompletePcm<'_>,
    rate: u32,
    range: Option<BpmRange>,
    original: &Value,
    rejection: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Choice, Failure> {
    check(cancelled)?;
    // Covers parse tree, intervals, <=6000 novelty values/JSON, encoding coexistence.
    let required = super::super::neural_analysis::budget::value_tree(original)
        .map_err(Failure::Unavailable)?
        .checked_add(RESERVE)
        .ok_or_else(|| Failure::Unavailable("approximate preparation extent overflow".into()))?;
    let _preparation = reserve_workspace(required).map_err(Failure::Unavailable)?;
    if original["bpm"].as_f64().is_some() {
        return Err(Failure::NoChoice);
    }
    let header = if original["policy"] == "neural-unavailable" {
        Value::Null
    } else {
        Value::Object(precise_header(original))
    };
    let (kind, period, windows) = if let Some((kind, period)) = neural(original) {
        (kind, period, json!([]))
    } else if let Some((period, windows)) = energy(sample, rate, cancelled)? {
        ("source_energy", period, windows)
    } else {
        return Err(Failure::NoChoice);
    };
    let bpm = 60.0 / period;
    if !valid_bpm(bpm) || range.is_some_and(|r| !r.contains(bpm)) {
        return Err(Failure::NoChoice);
    }
    encode(
        original.clone(),
        bpm,
        json!({"kind":kind,"original":header,
        "precise_rejection":rejection.chars().take(512).collect::<String>(),
        "period_seconds":period,"energy_windows":windows,
        "range_constraint":range.map(range_evidence)}),
        cancelled,
    )
}

fn range_evidence(range: BpmRange) -> Value {
    json!({"minimum_bpm":range.minimum_bpm,"maximum_bpm":range.maximum_bpm})
}

fn has_onsets(samples: &[f64], cancelled: &impl Fn() -> bool) -> Result<bool, Failure> {
    let mut previous = None;
    let mut increases = 0;
    for frame in samples.chunks_exact(441) {
        check(cancelled)?;
        let level = frame.iter().map(|v| v.abs()).sum::<f64>() / 441.0;
        if previous.is_some_and(|last: f64| level - last > (0.25 * last).max(1e-7)) {
            increases += 1;
            if increases >= 4 {
                return Ok(true);
            }
        }
        previous = Some(level);
    }
    Ok(false)
}

/// Use the legacy QM tempo calculation after the existing policies have declined.
/// Retain its detector positions solely as evidence for the scalar.
pub(super) fn conventional(
    sample: &CompletePcm<'_>,
    rate: u32,
    range: Option<BpmRange>,
    original: &Value,
    rejection: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Choice, Failure> {
    check(cancelled)?;
    let mono = original["mono_sha256"].as_str().ok_or(Failure::NoChoice)?;
    let working = super::super::analysis_pcm::qm_working_bytes(sample.frame_count(), rate)
        .map_err(|err| Failure::Unavailable(err.to_string()))?;
    let _workspace = reserve_workspace(working).map_err(Failure::Unavailable)?;
    let prepared =
        super::super::analysis_pcm::prepare_qm_input(sample, rate, mono, working, cancelled)
            .map_err(|err| match err {
                super::super::analysis_pcm::PcmError::Cancelled => Failure::Cancelled,
                _ => Failure::Unavailable(err.to_string()),
            })?;
    if !has_onsets(&prepared.samples, cancelled)? {
        return Err(Failure::NoChoice);
    }
    check(cancelled)?;
    let raw = std::panic::catch_unwind(|| {
        analyze_bpm_raw(&prepared.samples, 44_100, &AnalysisConfig::default())
    })
    .map_err(|_| Failure::Unavailable("QM analysis panicked".into()))?
    .map_err(Failure::Unavailable)?;
    check(cancelled)?;
    drop(prepared);
    let beats = raw.beat_frames();
    let hop = raw.odf_hop_samples();
    let extent = raw.input_frame_count() as f64 / hop as f64;
    if !(5..=MAX_QM_BEATS).contains(&beats.len())
        || !beats.iter().all(|beat| (0.0..extent).contains(beat))
        || beats.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Failure::NoChoice);
    }
    let bpm = f64::from(calculate_bpm(beats, hop as f64 / 44_100.0));
    if !valid_bpm(bpm) || range.is_some_and(|r| !r.contains(bpm)) {
        return Err(Failure::NoChoice);
    }
    let evidence_bytes = beats.len().checked_mul(64).ok_or(Failure::NoChoice)?;
    let _evidence = reserve_workspace(evidence_bytes).map_err(Failure::Unavailable)?;
    let mut source = json!({
        "model_sha256": original["model_sha256"], "frontend_id": original["frontend_id"],
        "source_sha256": original["source_sha256"], "mono_sha256": mono,
        "sample_rate_hz": rate, "frame_count": sample.frame_count(),
        "selected": null, "regions": [], "raw_beats": [], "raw_downbeats": [],
    });
    if let Some(execution) = original.get("execution") {
        source["execution"] = execution.clone();
    }
    encode(
        source,
        bpm,
        json!({"kind":"qm_beat_intervals","original":null,
        "precise_rejection":rejection.chars().take(512).collect::<String>(),
        "period_seconds":60.0 / bpm,"energy_windows":[],"qm_beat_frames":beats,
        "range_constraint":range.map(range_evidence)}),
        cancelled,
    )
}

fn encode(
    mut tree: Value,
    bpm: f64,
    evidence: Value,
    cancelled: &impl Fn() -> bool,
) -> Result<Choice, Failure> {
    let fields = tree.as_object_mut().ok_or(Failure::NoChoice)?;
    fields.insert("schema_version".into(), json!(11));
    fields.insert("backend".into(), json!("source_approximate"));
    fields.insert("policy".into(), json!("source-approximate-tempo-v11"));
    fields.insert("status".into(), json!("approximate_estimate"));
    fields.insert("bpm".into(), json!(bpm));
    fields.insert("approximate_tempo".into(), evidence);
    check(cancelled)?;
    let length = super::super::neural_analysis::budget::serialized_len(&tree)
        .map_err(Failure::Unavailable)?;
    let _serialization =
        reserve_workspace(length.checked_mul(2).ok_or_else(|| {
            Failure::Unavailable("approximate serialization extent overflow".into())
        })?)
        .map_err(Failure::Unavailable)?;
    let method_json =
        serde_json::to_string(&tree).map_err(|e| Failure::Unavailable(e.to_string()))?;
    check(cancelled)?;
    Ok(Choice { bpm, method_json })
}

#[cfg(test)]
#[path = "app_analysis_approximate_tests.rs"]
mod tests;
