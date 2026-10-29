//! Request-scoped user bounds select only an independently admitted scalar.
use super::qm_corroboration::{Failure, Observation};
use super::*;
use crate::audio_engine::neural_analysis::budget;

const SMALL: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::audio_engine) struct BpmRange {
    pub minimum_bpm: f64,
    pub maximum_bpm: f64,
}

impl BpmRange {
    pub(in crate::audio_engine) fn configured(
        enabled: bool,
        minimum_bpm: f64,
        maximum_bpm: f64,
    ) -> Result<Option<Self>, String> {
        if !minimum_bpm.is_finite()
            || !maximum_bpm.is_finite()
            || !(30.0..=300.0).contains(&minimum_bpm)
            || !(30.0..=300.0).contains(&maximum_bpm)
            || minimum_bpm >= maximum_bpm
        {
            return Err(
                "Automatic BPM range requires finite 30..300 bounds with minimum below maximum"
                    .into(),
            );
        }
        Ok(enabled.then_some(Self {
            minimum_bpm,
            maximum_bpm,
        }))
    }

    pub(in crate::audio_engine) fn contains(self, bpm: f64) -> bool {
        bpm.is_finite() && (self.minimum_bpm..=self.maximum_bpm).contains(&bpm)
    }

    pub(super) fn diagnostic(self, detail: &str) -> String {
        format!(
            "No automatic BPM within {}..{} BPM; {}",
            self.minimum_bpm,
            self.maximum_bpm,
            detail.chars().take(240).collect::<String>()
        )
    }
}

pub(super) struct RangeResult {
    pub bpm: Option<f64>,
    pub method_json: Option<String>,
    pub beat_error: Option<String>,
}

fn check(cancelled: &impl Fn() -> bool) -> Result<(), Failure> {
    if cancelled() {
        Err(Failure::Cancelled)
    } else {
        Ok(())
    }
}
fn unavailable(reason: impl std::fmt::Display) -> Failure {
    Failure::Unavailable(reason.to_string().chars().take(240).collect())
}
fn admit(parts: &[usize]) -> Result<usize, Failure> {
    let bytes = budget::sum(parts).map_err(unavailable)?;
    if bytes > LIMIT {
        Err(unavailable(
            "range metadata exceeds 512 MiB preparation budget",
        ))
    } else {
        Ok(bytes)
    }
}

pub(super) fn half_double_drift(
    primary_bpm: f64,
    qm_bpm: f64,
    common_extent: f64,
) -> Result<f64, Failure> {
    if !primary_bpm.is_finite()
        || primary_bpm <= 0.0
        || !qm_bpm.is_finite()
        || qm_bpm <= 0.0
        || !common_extent.is_finite()
        || common_extent <= 0.0
    {
        return Err(unavailable("QM half/double geometry unavailable"));
    }
    let p_n = 60.0 / primary_bpm;
    let p_q = 60.0 / qm_bpm;
    let slow = p_n.max(p_q);
    let fast = p_n.min(p_q);
    let drift = (slow - 2.0 * fast).abs() * (common_extent / slow);
    if !slow.is_finite() || fast <= 0.0 || !drift.is_finite() || drift > 0.005 {
        return Err(unavailable("QM half/double agreement failed"));
    }
    Ok(drift)
}

pub(super) fn scientific(range: Option<BpmRange>, mut result: LoadedAnalysis) -> LoadedAnalysis {
    if let Some(range) = range {
        if !range.contains(result.bpm) {
            result.bpm = 0.0;
            result.bpm_method_json = None;
            result.beat_error =
                Some(range.diagnostic("scientific estimate outside the requested range"));
        }
    }
    result
}

fn serialize(
    tree: &Value,
    retained_string: &String,
    source_live: usize,
    raw_workspace: usize,
    grid_bytes: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<String, Failure> {
    check(cancelled)?;
    let tree_bytes = budget::value_tree(tree).map_err(unavailable)?;
    let length = budget::serialized_len(tree).map_err(unavailable)?;
    let string_bound = length
        .checked_mul(2)
        .ok_or_else(|| unavailable("range serialization overflow"))?;
    admit(&[
        source_live,
        raw_workspace,
        tree_bytes,
        string_bound,
        retained_string.capacity(),
        grid_bytes,
        size_of::<LoadedAnalysis>(),
    ])?;
    let encoded = serde_json::to_string(tree).map_err(unavailable)?;
    admit(&[
        source_live,
        raw_workspace,
        tree_bytes,
        encoded.capacity(),
        retained_string.capacity(),
        grid_bytes,
        size_of::<LoadedAnalysis>(),
    ])?;
    check(cancelled)?;
    Ok(encoded)
}

/// The original tree is changed once; rejected bytes are retained before any QM
/// call. A partial selected tree is discarded on failure, never reparsed.
pub(super) fn apply(
    range: BpmRange,
    primary_bpm: f64,
    tree: &mut Value,
    base_string: String,
    base_core: usize,
    source_live: usize,
    raw_workspace: usize,
    grid_bytes: usize,
    cancelled: &impl Fn() -> bool,
    observe: impl FnOnce(usize) -> Result<Observation, Failure>,
) -> Result<RangeResult, Failure> {
    check(cancelled)?;
    if range.contains(primary_bpm) {
        return Ok(RangeResult {
            bpm: Some(primary_bpm),
            method_json: Some(base_string),
            beat_error: None,
        });
    }
    let no_method = |detail: &str| RangeResult {
        bpm: None,
        method_json: None,
        beat_error: Some(range.diagnostic(detail)),
    };
    let eligible = matches!(tree["schema_version"].as_u64(), Some(4 | 5))
        && tree["backend"] == "beat_this"
        && matches!(
            tree["status"].as_str(),
            Some("estimated" | "single_region_estimate")
        );
    let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        check(cancelled)?;
        admit(&[base_core, SMALL])?;
        let primary = json!({"schema_version":tree["schema_version"],"backend":tree["backend"],
            "policy":tree["policy"],"status":tree["status"],"bpm":tree["bpm"]});
        let block = json!({"minimum_bpm":range.minimum_bpm,"maximum_bpm":range.maximum_bpm,
            "primary":primary,"outcome":"no_supported_candidate"});
        let fields = tree
            .as_object_mut()
            .ok_or_else(|| unavailable("range method envelope missing"))?;
        fields.insert("schema_version".into(), json!(7));
        fields.insert("backend".into(), json!("beat_this_bpm_range"));
        fields.insert("policy".into(), json!("neural-user-range-v7"));
        fields.insert("status".into(), json!("insufficient"));
        fields.insert("bpm".into(), Value::Null);
        fields.insert("bpm_range".into(), block);
        serialize(
            tree,
            &base_string,
            source_live,
            raw_workspace,
            grid_bytes,
            cancelled,
        )
    }));
    check(cancelled)?;
    let rejected = match prepared {
        Ok(Ok(encoded)) => encoded,
        Ok(Err(Failure::Cancelled)) => return Err(Failure::Cancelled),
        Ok(Err(Failure::NoChoice)) => return Ok(no_method("QM has no independently admitted tempo")),
        Ok(Err(Failure::Unavailable(reason))) => return Ok(no_method(&reason)),
        Err(_) => return Ok(no_method("range metadata preparation panicked")),
    };
    drop(base_string);
    let attempted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        check(cancelled)?;
        if !eligible {
            return Err(unavailable(
                "primary estimate has no eligible NN half/double fallback",
            ));
        }
        // The old core remains a conservative floor. Keep the rejected string,
        // its small header and the optional observation live simultaneously.
        let work_core = admit(&[base_core, rejected.capacity(), SMALL])?;
        let observed = observe(work_core)?;
        check(cancelled)?;
        if !range.contains(observed.bpm) {
            return Err(unavailable(
                "independent QM estimate outside the requested range",
            ));
        }
        let nn_frames = tree["frame_count"]
            .as_u64()
            .ok_or_else(|| unavailable("range NN extent missing"))?;
        let nn_rate = tree["sample_rate_hz"]
            .as_u64()
            .filter(|&n| n > 0)
            .ok_or_else(|| unavailable("range NN rate missing"))?;
        let qm_frames = observed.qm["analyzer"]["frame_count"]
            .as_u64()
            .ok_or_else(|| unavailable("range QM extent missing"))?;
        let extent = (nn_frames as f64 / nn_rate as f64).min(qm_frames as f64 / 44_100.0);
        let drift = half_double_drift(primary_bpm, observed.bpm, extent)?;
        if drift != observed.period_drift_seconds {
            return Err(unavailable("range QM drift differs"));
        }
        let Observation {
            source,
            qm,
            bpm,
            status,
            period_drift_seconds,
        } = observed;
        let mut selection = serde_json::Map::new();
        selection.insert("source".into(), source);
        selection.insert("qm".into(), qm);
        selection.insert("period_drift_seconds".into(), json!(period_drift_seconds));
        let fields = tree
            .as_object_mut()
            .ok_or_else(|| unavailable("range method envelope missing"))?;
        let block = fields
            .get_mut("bpm_range")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| unavailable("range decision envelope missing"))?;
        block.insert("outcome".into(), json!("qm_half_double"));
        block.insert("selection".into(), Value::Object(selection));
        fields.insert("bpm".into(), json!(bpm));
        fields.insert("status".into(), json!(status));
        let encoded = serialize(
            tree,
            &rejected,
            source_live,
            raw_workspace,
            grid_bytes,
            cancelled,
        )?;
        Ok(RangeResult {
            bpm: Some(bpm),
            method_json: Some(encoded),
            beat_error: None,
        })
    }));
    check(cancelled)?;
    match attempted {
        Ok(Ok(choice)) => Ok(choice),
        Ok(Err(Failure::Cancelled)) => Err(Failure::Cancelled),
        failure => {
            let reason = match failure {
                Ok(Err(Failure::Unavailable(reason))) => reason,
                Ok(Err(Failure::NoChoice)) => "QM has no independently admitted tempo".into(),
                _ => "optional range selection panicked".into(),
            };
            Ok(RangeResult {
                bpm: None,
                method_json: Some(rejected),
                beat_error: Some(range.diagnostic(&reason)),
            })
        }
    }
}

#[cfg(test)]
#[path = "app_analysis_range_tests.rs"]
mod tests;
