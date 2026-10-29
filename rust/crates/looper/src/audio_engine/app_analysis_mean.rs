//! Last approximate scalar fallback on complete, source-bound observations.
use super::qm_corroboration::{Choice, CompletedObservation, Failure};
use super::*;
use crate::audio_engine::{app_tempo, neural_analysis::budget};

const SMALL: usize = 1024 * 1024;

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
    let total = budget::sum(parts).map_err(unavailable)?;
    if total > LIMIT {
        Err(unavailable(
            "tempo fallback exceeds 512 MiB preparation budget",
        ))
    } else {
        Ok(total)
    }
}
fn local_failure(error: app_tempo::mean::Failure) -> Failure {
    match error {
        app_tempo::mean::Failure::Cancelled => Failure::Cancelled,
        other => unavailable(format!("interior mean preparation unavailable: {other:?}")),
    }
}

pub(super) fn attempt(
    neural_beats: &[f64],
    duration: f64,
    range: Option<BpmRange>,
    tree: &mut Value,
    base: &String,
    base_core: usize,
    source_live: usize,
    raw_workspace: usize,
    grid_bytes: usize,
    cancelled: &impl Fn() -> bool,
    observe: impl FnOnce(usize) -> Result<CompletedObservation, Failure>,
) -> Result<Choice, Failure> {
    check(cancelled)?;
    // This completed independent precise attempt precedes every mean decision.
    // Its existing owner verifies full PCM/NN mono/source identity and retires DSP.
    let observed = observe(admit(&[base_core, SMALL])?)?;
    check(cancelled)?;
    let CompletedObservation {
        source,
        qm,
        bpm,
        status,
        mean: qm_mean,
        ..
    } = observed;
    let source_qm_bytes = budget::sum(&[
        budget::value_tree(&source).map_err(unavailable)?,
        budget::value_tree(&qm).map_err(unavailable)?,
    ])
    .map_err(unavailable)?;
    let completed_mean_bytes = qm_mean
        .as_ref()
        .map_or(Some(0), app_tempo::mean::InteriorMean::retained_bytes)
        .ok_or_else(|| unavailable("QM mean extent overflow"))?;
    // Both branches hold the original source/QM trees before envelope assembly.
    let observed_core = admit(&[base_core, source_qm_bytes, completed_mean_bytes, SMALL])?;
    let (selected_bpm, selected_status, kind, mean) = if let Some(bpm) = bpm {
        drop(qm_mean);
        (bpm, status, "precise_qm", None)
    } else {
        let Some(qm_mean) = qm_mean else {
            return Err(Failure::NoChoice);
        };
        let qm_bytes = qm_mean
            .retained_bytes()
            .ok_or_else(|| unavailable("QM mean extent overflow"))?;
        let retained = observed_core;
        let available = LIMIT
            .checked_sub(retained)
            .ok_or_else(|| unavailable("mean workspace unavailable"))?;
        let Some(neural_mean) =
            app_tempo::mean::interior(neural_beats, duration, available, cancelled)
                .map_err(local_failure)?
        else {
            return Err(Failure::NoChoice);
        };
        let Some((factor, ratio)) = app_tempo::mean::agreement(&neural_mean, &qm_mean) else {
            return Err(Failure::NoChoice);
        };
        let bpm = neural_mean.bpm;
        let local_bytes = budget::sum(&[
            qm_bytes,
            neural_mean
                .retained_bytes()
                .ok_or_else(|| unavailable("NN mean extent overflow"))?,
        ])
        .map_err(unavailable)?;
        // Each new scalar/exclusion JSON value is bounded independently before
        // typed/local JSON coexistence. The factor never reads the BPM range.
        let json_reserve = local_bytes
            .checked_mul(16)
            .and_then(|n| n.checked_add(SMALL))
            .ok_or_else(|| unavailable("mean JSON extent overflow"))?;
        admit(&[retained, local_bytes, json_reserve])?;
        let block = json!({"window_seconds":[duration/5.0,4.0*duration/5.0],
            "neural_regions":neural_mean.regions,"qm_regions":qm_mean.regions,
            "neural_mean_bpm":bpm,"qm_mean_bpm":qm_mean.bpm,"qm_bpm_factor":factor,
            "adjusted_qm_mean_bpm":factor*qm_mean.bpm,"maximum_period_ratio":ratio});
        admit(&[
            retained,
            local_bytes,
            budget::value_tree(&block).map_err(unavailable)?,
        ])?;
        drop(neural_mean);
        drop(qm_mean);
        (bpm, "mean_estimate", "mean_estimate", Some(block))
    };
    if !selected_bpm.is_finite() || !(30.0..=300.0).contains(&selected_bpm) {
        return Err(Failure::NoChoice);
    }
    if let Some(range) = range {
        if !range.contains(selected_bpm) {
            return Err(unavailable(range.diagnostic(
                "last automatic tempo result outside the requested range",
            )));
        }
    }
    check(cancelled)?;
    let mean_bytes = match mean.as_ref() {
        Some(value) => budget::value_tree(value).map_err(unavailable)?,
        None => 0,
    };
    // Pre-admit the one envelope/header allocation. Move the already-owned
    // source/QM/mean values instead of cloning their complete JSON trees.
    admit(&[base_core, source_qm_bytes, mean_bytes, SMALL])?;
    let neural_result = json!({
        "schema_version":tree["schema_version"],"backend":tree["backend"],
        "policy":tree["policy"],"status":tree["status"],"bpm":null});
    let mut fields = serde_json::Map::new();
    fields.insert("kind".into(), json!(kind));
    fields.insert("neural_result".into(), neural_result);
    fields.insert("source".into(), source);
    fields.insert("qm".into(), qm);
    if let Some(mean) = mean {
        fields.insert("mean".into(), mean);
    }
    if let Some(range) = range {
        fields.insert(
            "range_constraint".into(),
            json!({"minimum_bpm":range.minimum_bpm,"maximum_bpm":range.maximum_bpm}),
        );
    }
    complete(
        tree,
        Value::Object(fields),
        selected_bpm,
        selected_status,
        base,
        source_live,
        raw_workspace,
        grid_bytes,
        cancelled,
    )
}

fn complete(
    tree: &mut Value,
    block: Value,
    bpm: f64,
    status: &str,
    base: &String,
    source_live: usize,
    raw_workspace: usize,
    grid_bytes: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<Choice, Failure> {
    check(cancelled)?;
    let fields = tree
        .as_object_mut()
        .ok_or_else(|| unavailable("tempo fallback envelope missing"))?;
    fields.insert("schema_version".into(), json!(10));
    fields.insert("backend".into(), json!("beat_this_qm_dsp"));
    fields.insert("policy".into(), json!("neural-qm-tempo-fallback-v10"));
    fields.insert("status".into(), json!(status));
    fields.insert("bpm".into(), json!(bpm));
    fields.insert("tempo_fallback".into(), block);
    let tree_bytes = budget::value_tree(tree).map_err(unavailable)?;
    let length = budget::serialized_len(tree).map_err(unavailable)?;
    let strings = length
        .checked_mul(2)
        .ok_or_else(|| unavailable("tempo fallback serialization overflow"))?;
    admit(&[
        source_live,
        raw_workspace,
        tree_bytes,
        strings,
        base.capacity(),
        grid_bytes,
        size_of::<LoadedAnalysis>(),
    ])?;
    check(cancelled)?;
    let method_json = serde_json::to_string(tree).map_err(unavailable)?;
    admit(&[
        source_live,
        raw_workspace,
        tree_bytes,
        method_json.capacity(),
        base.capacity(),
        grid_bytes,
        size_of::<LoadedAnalysis>(),
    ])?;
    check(cancelled)?;
    Ok(Choice { bpm, method_json })
}

#[cfg(test)]
#[path = "app_analysis_mean_tests.rs"]
mod tests;
