//! A bounded optional transaction; the caller retains its exact base NN string.
use super::*;
use crate::audio_engine::{analysis_pcm, app_tempo, cold_store, neural_analysis::budget};
use flitzis_looper_analysis::{AnalysisConfig, analyze_bpm_raw};

const SMALL: usize = 1024 * 1024;

#[derive(Debug)]
pub(super) enum Failure {
    Cancelled,
    NoChoice,
    Unavailable(String),
}
impl std::fmt::Display for Failure {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => out.write_str("analysis superseded"),
            Self::NoChoice => out.write_str("QM has no independently admitted tempo"),
            Self::Unavailable(reason) => out.write_str(reason),
        }
    }
}
impl std::error::Error for Failure {}
pub(super) struct Choice {
    pub bpm: f64,
    pub method_json: String,
}

pub(super) struct Observation {
    pub source: Value,
    pub qm: Value,
    pub bpm: f64,
    pub status: &'static str,
    pub period_drift_seconds: f64,
}

pub(super) struct CompletedObservation {
    pub source: Value,
    pub qm: Value,
    pub bpm: Option<f64>,
    pub status: &'static str,
    pub mean: Option<app_tempo::mean::InteriorMean>,
    pub period_drift_seconds: f64,
}

fn local_failure(error: app_tempo::mean::Failure) -> Failure {
    match error {
        app_tempo::mean::Failure::Cancelled => Failure::Cancelled,
        other => unavailable(format!("interior mean preparation unavailable: {other:?}")),
    }
}

fn unavailable(reason: impl std::fmt::Display) -> Failure {
    Failure::Unavailable(reason.to_string().chars().take(240).collect())
}
fn check(cancelled: &impl Fn() -> bool) -> Result<(), Failure> {
    if cancelled() {
        Err(Failure::Cancelled)
    } else {
        Ok(())
    }
}
fn admit(parts: &[usize]) -> Result<usize, Failure> {
    let value = budget::sum(parts).map_err(unavailable)?;
    if value > LIMIT {
        return Err(unavailable("512 MiB budget unavailable"));
    }
    Ok(value)
}
fn remaining(parts: &[usize]) -> Result<usize, Failure> {
    LIMIT
        .checked_sub(admit(parts)?)
        .ok_or_else(|| unavailable("512 MiB budget unavailable"))
}
fn array_bytes(n: usize) -> Result<usize, Failure> {
    n.checked_mul(8)
        .ok_or_else(|| unavailable("QM array extent overflow"))
}
fn seconds(
    values: impl ExactSizeIterator<Item = f64>,
    live: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<f64>, Failure> {
    let bytes = values
        .len()
        .checked_mul(8)
        .ok_or_else(|| unavailable("QM seconds overflow"))?;
    admit(&[live, bytes])?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(values.len())
        .map_err(unavailable)?;
    if result.capacity() > values.len() {
        return Err(unavailable("QM seconds capacity differs"));
    }
    for (index, value) in values.enumerate() {
        if index.is_multiple_of(4096) {
            check(cancelled)?;
        }
        result.push(value);
    }
    Ok(result)
}

/// All QM PCM, raw arrays, scalar-estimate objects and preparation temporaries
/// retire on return, before the final joint-tree/string ownership check.
pub(super) fn observation(
    sample: &SampleBuffer,
    rate: u32,
    context: &NeuralContext,
    nn_mono: &str,
    decoder_identity: &str,
    base_core: usize,
    cancelled: &impl Fn() -> bool,
    agreement: impl FnOnce(f64, f64) -> Result<f64, Failure>,
) -> Result<Observation, Failure> {
    let mut agreement = Some(agreement);
    let completed = observe_completed(
        sample,
        rate,
        context,
        nn_mono,
        decoder_identity,
        base_core,
        cancelled,
        false,
        |bpm, extent| agreement.take().unwrap()(bpm, extent),
    )?;
    Ok(Observation {
        source: completed.source,
        qm: completed.qm,
        bpm: completed.bpm.ok_or(Failure::NoChoice)?,
        status: completed.status,
        period_drift_seconds: completed.period_drift_seconds,
    })
}

/// Preserve a completed strict refusal for the separate approximate policy.
/// Existing successful-only callers still use `observation` and the same gate.
pub(super) fn completed_observation(
    sample: &SampleBuffer,
    rate: u32,
    context: &NeuralContext,
    nn_mono: &str,
    decoder_identity: &str,
    base_core: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<CompletedObservation, Failure> {
    observe_completed(
        sample,
        rate,
        context,
        nn_mono,
        decoder_identity,
        base_core,
        cancelled,
        true,
        |_, _| Ok(0.0),
    )
}

fn observe_completed(
    sample: &SampleBuffer,
    rate: u32,
    context: &NeuralContext,
    nn_mono: &str,
    decoder_identity: &str,
    base_core: usize,
    cancelled: &impl Fn() -> bool,
    mean_requested: bool,
    mut agreement: impl FnMut(f64, f64) -> Result<f64, Failure>,
) -> Result<CompletedObservation, Failure> {
    check(cancelled)?;
    let available = remaining(&[base_core])?;
    let prepared = analysis_pcm::prepare_qm_input(sample, rate, nn_mono, available, cancelled)
        .map_err(|error| match error {
            analysis_pcm::PcmError::Cancelled => Failure::Cancelled,
            other => unavailable(other),
        })?;
    let analysis_pcm::QmPcm {
        samples,
        loaded_interleaved_sha256,
        normal_mono_sha256,
        analyzer_sha256,
        resampler,
        qm_working_bytes,
        preparation_peak_bytes,
    } = prepared;
    admit(&[base_core, preparation_peak_bytes])?;
    let analyzer_frames = samples.len();
    let config = AnalysisConfig::default();
    check(cancelled)?;
    // This unchanged DSP call has no cooperative cancellation hook. Its source
    // owner remains held; supersession is checked before and after the call.
    let raw = analyze_bpm_raw(&samples, 44_100, &config).map_err(unavailable)?;
    check(cancelled)?;
    if raw.input_frame_count() != analyzer_frames || raw.input_sample_rate_hz() != 44_100 {
        return Err(unavailable("QM analyzer geometry differs"));
    }
    drop(samples);
    let raw_live = admit(&[base_core, qm_working_bytes, SMALL])?;
    let reserve = app_tempo::preparation_reservation(
        raw.beat_frames().len(),
        raw.downbeat_raw_indices().len(),
    )
    .ok_or_else(|| unavailable("QM estimate reserve overflow"))?;
    let seconds_bytes = raw
        .beat_frames()
        .len()
        .checked_add(raw.downbeat_raw_indices().len())
        .and_then(|n| n.checked_mul(8))
        .ok_or_else(|| unavailable("QM seconds extent overflow"))?;
    admit(&[raw_live, seconds_bytes, reserve])?;
    let beats = seconds(raw.beat_seconds(), raw_live, cancelled)?;
    let downbeats = seconds(
        raw.downbeat_seconds(),
        admit(&[raw_live, array_bytes(beats.capacity())?])?,
        cancelled,
    )?;
    let fit_available = remaining(&[
        raw_live,
        array_bytes(beats.capacity())?,
        array_bytes(downbeats.capacity())?,
    ])?;
    let qm_estimate = app_tempo::estimate_with_budget_cancelled(
        &beats,
        analyzer_frames as f64 / 44_100.0,
        downbeats.len(),
        fit_available,
        cancelled,
    )
    .map_err(|_| Failure::Cancelled)?;
    check(cancelled)?;
    let bpm = qm_estimate.bpm;
    if bpm.is_none() && app_tempo::optional_resource_refusal(&qm_estimate) {
        return Err(unavailable("QM has no independently admitted tempo"));
    }
    if bpm.is_none() && !mean_requested {
        return Err(Failure::NoChoice);
    }
    let status = qm_estimate.status;
    let source_extent = sample.frame_count() as f64 / f64::from(rate);
    let extent = source_extent.min(analyzer_frames as f64 / 44_100.0);
    let drift = if let Some(bpm) = bpm {
        agreement(bpm, extent)?
    } else {
        0.0
    };
    let mean = if mean_requested && app_tempo::completed_conflict(&qm_estimate) {
        let workspace = app_tempo::mean::preparation_bytes(beats.len())
            .ok_or_else(|| unavailable("QM mean extent overflow"))?;
        admit(&[
            raw_live,
            array_bytes(beats.capacity())?,
            array_bytes(downbeats.capacity())?,
            reserve,
            workspace,
        ])?;
        app_tempo::mean::interior(&beats, source_extent, workspace, cancelled)
            .map_err(local_failure)?
    } else {
        None
    };
    let retained_mean = mean
        .as_ref()
        .map_or(Some(0), app_tempo::mean::InteriorMean::retained_bytes)
        .ok_or_else(|| unavailable("QM mean metadata overflow"))?;
    drop(beats);
    drop(downbeats);
    // The admitted scalar reserve covers typed estimate / new JSON coexistence;
    // no NN arrays are cloned here. The caller holds one mutable base tree.
    admit(&[raw_live, reserve, retained_mean])?;
    let kernel_version = app_tempo::schema_version(&qm_estimate);
    let mut own = serde_json::to_value(&qm_estimate).map_err(unavailable)?;
    drop(qm_estimate);
    let own_fields = own
        .as_object_mut()
        .ok_or_else(|| unavailable("QM estimate envelope missing"))?;
    own_fields.remove("policy");
    own_fields.insert("kernel_version".into(), json!(kernel_version));
    let decoder_json =
        serde_json::to_string(&cold_store::canonical(&context.descriptor)).map_err(unavailable)?;
    if decoder_json.len() > 65_536
        || format!("{:x}", Sha256::digest(decoder_json.as_bytes())) != decoder_identity
    {
        return Err(unavailable("QM decoder binding differs"));
    }
    let source = json!({"source_sha256":context.descriptor["original"]["sha256"],
            "decoder_identity":decoder_identity,"decoder_descriptor_json":decoder_json,
            "loaded_sample_rate_hz":rate,"loaded_channels":sample.channels,
            "loaded_frame_count":sample.frame_count(),"loaded_origin_seconds":0.0,
            "loaded_interleaved_sha256":loaded_interleaved_sha256,"verified_nn_mono_sha256":nn_mono});
    let qm = json!({"backend":"qm_dsp","device":"cpu",
            "configuration":{"alpha":config.alpha,"hop_size":config.hop_size,"input_tempo":config.input_tempo,
                "max_bin_hz":config.max_bin_hz,"step_secs":config.step_secs,"tightness":config.tightness,
                "viterbi_sigma":config.viterbi_sigma,"window_length":config.window_length},
            "normal_mono_f32_sha256":normal_mono_sha256,
            "downmix_revision":if sample.channels==1 {"normal-analysis-mono-identity-f32-v1"} else {"normal-analysis-stereo-f32-add-multiply-half-v1"},
            "analyzer":{"sample_rate_hz":44_100,"frame_count":analyzer_frames,"origin_seconds":0.0,
                "f64_le_sha256":analyzer_sha256,"resampler":resampler},
            "odf_hop_samples":raw.odf_hop_samples(),"raw_beat_frames":raw.beat_frames(),
            "raw_downbeat_indices":raw.downbeat_raw_indices(),"estimate":own});
    check(cancelled)?;
    Ok(CompletedObservation {
        source,
        qm,
        bpm,
        status,
        mean,
        period_drift_seconds: drift,
    })
}

pub(super) fn attempt(
    sample: &SampleBuffer,
    rate: u32,
    context: &NeuralContext,
    nn_mono: &str,
    decoder_identity: &str,
    nn_period: f64,
    tree: &mut Value,
    base_string: &String,
    base_core: usize,
    source_live: usize,
    raw_workspace: usize,
    grid_bytes: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<Choice, Failure> {
    check(cancelled)?;
    admit(&[base_core, SMALL])?;
    let neural_result = neural_result_header(tree);
    let observed = observation(
        sample,
        rate,
        context,
        nn_mono,
        decoder_identity,
        base_core,
        cancelled,
        |bpm, extent| {
            let qm_period = 60.0 / bpm;
            let drift = (qm_period - nn_period).abs() * (extent / nn_period);
            if !qm_period.is_finite()
                || qm_period <= 0.0
                || !nn_period.is_finite()
                || nn_period <= 0.0
                || !drift.is_finite()
                || drift > 0.005
            {
                return Err(unavailable("QM same-period agreement failed"));
            }
            Ok(drift)
        },
    )?;
    let Observation {
        source,
        qm,
        bpm,
        status,
        period_drift_seconds,
    } = observed;
    let mut block = serde_json::Map::new();
    block.insert("neural_result".into(), neural_result);
    block.insert("source".into(), source);
    block.insert("qm".into(), qm);
    block.insert("period_drift_seconds".into(), json!(period_drift_seconds));
    complete(
        tree,
        Value::Object(block),
        bpm,
        status,
        base_string,
        source_live,
        raw_workspace,
        grid_bytes,
        cancelled,
    )
}

fn neural_result_header(tree: &Value) -> Value {
    json!({"schema_version":tree["schema_version"],"policy":tree["policy"],"status":tree["status"],"bpm":null})
}

fn complete(
    tree: &mut Value,
    block: Value,
    bpm: f64,
    status: &'static str,
    base_string: &String,
    source_live: usize,
    raw_workspace: usize,
    grid_bytes: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<Choice, Failure> {
    check(cancelled)?;
    let fields = tree
        .as_object_mut()
        .ok_or_else(|| unavailable("joint method envelope missing"))?;
    fields.insert("schema_version".into(), json!(6));
    fields.insert("backend".into(), json!("beat_this_qm_dsp"));
    fields.insert("policy".into(), json!("neural-qm-same-period-v6"));
    fields.insert("status".into(), json!(status));
    fields.insert("bpm".into(), json!(bpm));
    fields.insert("qm_corroboration".into(), block);
    let tree_bytes = budget::value_tree(tree).map_err(unavailable)?;
    let length = budget::serialized_len(tree).map_err(unavailable)?;
    let string_bound = length
        .checked_mul(2)
        .ok_or_else(|| unavailable("joint serialization overflow"))?;
    admit(&[
        source_live,
        raw_workspace,
        tree_bytes,
        string_bound,
        base_string.capacity(),
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
        base_string.capacity(),
        grid_bytes,
        size_of::<LoadedAnalysis>(),
    ])?;
    check(cancelled)?;
    Ok(Choice { bpm, method_json })
}

#[cfg(test)]
#[path = "app_analysis_qm_tests.rs"]
mod tests;
