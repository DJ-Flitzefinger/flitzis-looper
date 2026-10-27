//! Primary neural beat estimates on the existing admitted Cold/analysis workers.
//! No manual RAW slot, accepted map, source publication, or RT work is created.

use super::analysis_jobs::source_beatmap_neural::{MonoWriter, invoke_worker, verify_staged};
use super::analysis_pcm::{LoadedPcmSnapshot, PcmIdentity, key_input_from_f32_le};
use super::source_beatmap_capture::beat_this::{
    BeatThisSourceInput, expected_logit_count, pinned_model,
};
use crate::messages::{LoadedAnalysis, SampleBuffer};
use pyo3::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;

const LIMIT: usize = 512 * 1024 * 1024;

/// Preflight known geometry before the caller materializes complete PCM.
pub(super) fn prepare_input(
    frames: usize,
    channels: usize,
    rate: u32,
    held: usize,
    tempo_only: bool,
    read: impl FnOnce() -> Result<SampleBuffer, String>,
) -> Result<SampleBuffer, String> {
    admission_peak(frames, channels, rate, held, tempo_only)?;
    read()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_neural_and_tempo_only_preparations_have_bounded_geometry_admission() {
        for seconds in [18, 109, 297] {
            let full = admission_peak(seconds * 48_000, 2, 48_000, 0, false).unwrap();
            let tempo = admission_peak(seconds * 48_000, 2, 48_000, 0, true).unwrap();
            assert!(tempo < full && full <= LIMIT);
        }
        assert!(admission_peak(usize::MAX, 2, 48_000, 0, false).is_err());
        assert!(admission_peak(1, 0, 48_000, 0, false).is_err());
        assert!(admission_peak(1, 2, 1, 0, false).is_err());
        assert!(admission_peak(48_000, 2, 48_000, usize::MAX, false).is_err());
    }

    #[test]
    fn oversized_finite_source_is_refused_before_complete_pcm_read() {
        let read_started = std::cell::Cell::new(false);
        let result = prepare_input(200_000_000, 2, 48_000, 48_000 * 8, true, || {
            read_started.set(true);
            panic!("oversized complete PCM must not be materialized");
        });
        assert!(result.is_err());
        assert!(!read_started.get());
    }
}

pub(super) fn admission_peak(
    frames: usize,
    channels: usize,
    rate: u32,
    extra_held: usize,
    tempo_only: bool,
) -> Result<usize, String> {
    if frames == 0 || !(1..=32).contains(&channels) || !(8_000..=384_000).contains(&rate) {
        return Err("invalid neural source geometry".into());
    }
    let retained = frames
        .checked_mul(channels)
        .and_then(|v| v.checked_mul(4))
        .and_then(|v| v.checked_add(extra_held))
        .ok_or("neural held budget overflow")?;
    let plan =
        super::analysis_pcm::staging_plan(retained, frames, rate).map_err(|e| e.to_string())?;
    let logits = expected_logit_count(frames, rate)?;
    let wire = logits
        .checked_mul(512)
        .and_then(|v| v.checked_add(16 * 1024 * 1024))
        .ok_or("neural wire budget overflow")?;
    let converted = usize::try_from((frames as u128 * 44_100).div_ceil(u128::from(rate)))
        .map_err(|_| "neural key extent overflow")?;
    let key = if tempo_only {
        0
    } else {
        let cqt =
            flitzis_looper_analysis::key_detection::librosa_cqt::cqt::default_cqt_working_bytes(
                converted,
            )
            .ok_or("neural key workspace overflow")?;
        let (input, output) = super::analysis_pcm::fft::fft_dimensions(rate, 44_100);
        let fft = input
            .checked_add(output)
            .and_then(|v| v.checked_mul(256))
            .ok_or("neural FFT workspace overflow")?;
        plan.key_peak_bytes
            .checked_add(fft)
            .ok_or("neural key workspace overflow")?
            .max(
                converted
                    .checked_mul(12)
                    .and_then(|v| v.checked_add(cqt))
                    .ok_or("neural key workspace overflow")?,
            )
    };
    let peak = retained
        .checked_add(wire)
        .and_then(|v| v.checked_add(key))
        .ok_or("neural analysis budget overflow")?
        .max(plan.export_peak_bytes);
    if peak > LIMIT {
        return Err("neural analysis exceeds complete-source preparation budget".into());
    }
    Ok(peak)
}

#[pyclass(frozen)]
pub(super) struct AnalysisCancellation(pub Arc<dyn Fn() -> bool + Send + Sync>);

#[pymethods]
impl AnalysisCancellation {
    fn is_cancelled(&self) -> bool {
        (self.0)()
    }
}

pub(super) struct NeuralContext {
    pub installation: String,
    pub staging_root: PathBuf,
    pub descriptor: Value,
    pub identity: PcmIdentity,
    pub cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
    pub additional_held_bytes: usize,
}

pub(super) fn analyze(
    sample: &SampleBuffer,
    rate: u32,
    context: Option<NeuralContext>,
    tempo_only: bool,
) -> Result<LoadedAnalysis, String> {
    let Some(context) = context else {
        // Compatibility callers deliberately retain their original backend.
        return super::analyze_sample(sample, rate).map(Into::into);
    };
    let cancelled = || (context.cancelled)();
    let source_digest = context.descriptor["original"]["sha256"]
        .as_str()
        .ok_or("neural analysis original digest unavailable")?
        .to_owned();
    let decoder_identity =
        super::cold_store::descriptor_digest(&context.descriptor).map_err(|e| e.to_string())?;
    super::source_beatmap_raw_data::validate_source_descriptor(
        &context.descriptor,
        &decoder_identity,
    )?;
    let frames = sample.frame_count();
    admission_peak(
        frames,
        sample.channels,
        rate,
        context.additional_held_bytes,
        tempo_only,
    )?;
    let snapshot = LoadedPcmSnapshot::new(sample.clone(), rate, context.identity.clone(), LIMIT)
        .map_err(|e| e.to_string())?;
    let plan = snapshot.staging_plan().map_err(|e| e.to_string())?;
    let logits = expected_logit_count(frames, rate)?;
    // Bounded main-process protocol/tuples/codec workspace, scaled to this actual
    // complete observation extent. Model subprocess RSS is separately bounded by
    // the existing worker adapter; this is not a whole process-tree RAM claim.
    let wire = logits
        .checked_mul(512)
        .and_then(|v| v.checked_add(16 * 1024 * 1024))
        .ok_or("neural wire budget overflow")?;
    let held = snapshot
        .retained_bytes()
        .checked_add(context.additional_held_bytes)
        .ok_or("neural held budget overflow")?;
    let temporary = tempfile::Builder::new()
        .prefix(".beat-this-app-")
        .tempdir_in(&context.staging_root)
        .map_err(|e| e.to_string())?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let path = temporary.path().join("mono.f32le");
        let path_text = path.to_str().ok_or("neural staging path is not representable")?;
        let mut file = super::analysis_jobs::create_staged_pcm(path_text).map_err(|e| e.to_string())?;
        let mut writer = MonoWriter { file: &mut file, digest: Sha256::new(), bytes: 0 };
        snapshot.stream_f32_le(&mut writer, LIMIT, LIMIT, &cancelled).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        if writer.bytes != plan.export_bytes { return Err("neural staged extent differs".into()); }
        let mono_sha256 = format!("{:x}", writer.digest.finalize());
        file.sync_all().map_err(|e| e.to_string())?;
        let mut key_reader = file.try_clone().map_err(|e| e.to_string())?;
        key_reader.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let source = BeatThisSourceInput { decoder_identity, source_descriptor: context.descriptor.clone(),
            source_sample_rate_hz: rate, source_frame_count: frames, source_zero_frame: 0,
            mono_sha256: mono_sha256.clone(), mono_rule: super::analysis_pcm::MONO_RULE.into() };
        let identity = json!({"pad_id":context.identity.pad_id,"request_id":context.identity.request_id,
            "source_id":context.identity.source_id,"source_generation":context.identity.source_generation});
        let probe = Python::attach(|py| Py::new(py, AnalysisCancellation(context.cancelled.clone())))
            .map_err(|e| e.to_string())?.into_any();
        let (beat, key) = std::thread::scope(|scope| {
            let beat = scope.spawn(|| invoke_worker(source, identity, path_text, &context.installation, probe));
            let key = if tempo_only { None } else {
                let input = key_input_from_f32_le(&mut key_reader, frames, rate, LIMIT - held - wire, &cancelled)
                    .map_err(|e| e.to_string());
                Some(match input {
                    Ok(input) => {
                        let widened: Vec<_> = input.samples.iter().map(|&v| f64::from(v)).collect();
                        drop(input);
                        super::run_key_detection(&widened)
                    }
                    Err(_) => "unknown".into(),
                })
            };
            (beat.join().unwrap_or_else(|_| Err("neural worker panicked".into())), key)
        });
        drop(key_reader);
        verify_staged(&mut file, plan.export_bytes, &mono_sha256, &cancelled)?;
        if cancelled() { return Err("neural analysis request superseded".into()); }
        let (raw, _) = match beat {
            Ok(raw) => raw,
            Err(error) if tempo_only => return Err(error),
            Err(error) => return Ok(LoadedAnalysis { bpm: 0.0, key,
                beat_grid: flitzis_looper_analysis::BeatGrid { beats: vec![], downbeats: vec![], bars: vec![] },
                bpm_method_json: None, beat_error: Some(error) }),
        };
        let predictions = raw.predictions();
        let estimate = super::app_tempo::estimate(&predictions.beat_seconds, frames as f64 / f64::from(rate));
        let mut method = serde_json::to_value(&estimate).map_err(|e| e.to_string())?;
        let object = method.as_object_mut().ok_or("neural estimate envelope missing")?;
        object.extend(json!({"schema_version":2,"backend":"beat_this","model_sha256":pinned_model()["sha256"],
            "frontend_id":pinned_model()["frontend_id"],"source_sha256":source_digest,
            "mono_sha256":mono_sha256,"sample_rate_hz":rate,"frame_count":frames,
            "raw_beats":predictions.beat_seconds,"raw_downbeats":predictions.downbeat_seconds}).as_object().unwrap().clone());
        let beats = predictions.beat_seconds.iter().map(|&v| v as f32).collect();
        let downbeats: Vec<_> = predictions.downbeat_seconds.iter().map(|&v| v as f32).collect();
        Ok(LoadedAnalysis { bpm: estimate.bpm.unwrap_or(0.0), key,
            beat_grid: flitzis_looper_analysis::BeatGrid { beats, bars: downbeats.clone(), downbeats },
            bpm_method_json: Some(method.to_string()),
            beat_error: estimate.bpm.is_none().then(|| format!("Neural tempo estimate {}", estimate.status)) })
    })).unwrap_or_else(|_| Err("neural analysis worker panicked".into()));
    match (result, temporary.close()) {
        (result, Ok(())) => result,
        (_, Err(error)) => Err(format!("neural staging cleanup failed: {error}")),
    }
}
