//! Primary neural beat estimates on the existing admitted Cold/analysis workers.
//! Scalar tempo/key metadata uses complete immutable PCM off realtime.

use super::analysis_pcm::{LoadedPcmSnapshot, PcmIdentity, key_input_from_f32_le};
use super::neural_analysis::raw::{BeatThisSourceInput, expected_logit_count, pinned_model};
use super::neural_analysis::{MonoWriter, invoke_worker, verify_staged};
use crate::messages::{AnalysisKeyStatus, LoadedAnalysis, SampleBuffer};
use pyo3::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[path = "app_analysis_qm.rs"]
mod qm_corroboration;
#[path = "app_analysis_range.rs"]
mod bpm_range;
pub(super) use bpm_range::BpmRange;
#[path = "app_analysis_prior.rs"]
#[cfg(test)]
mod nominal_prior;

const LIMIT: usize = 512 * 1024 * 1024;

/// Preflight mandatory BPM geometry before the caller materializes complete PCM.
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
            let full = admission_plan(seconds * 48_000, 2, 48_000, 0, false).unwrap();
            let tempo = admission_plan(seconds * 48_000, 2, 48_000, 0, true).unwrap();
            assert_eq!(tempo.primary_peak_bytes, full.primary_peak_bytes);
            assert!(full.primary_peak_bytes < full.key_peak_bytes.unwrap());
            assert!(full.key_peak_bytes.unwrap() <= LIMIT);
            assert!(full.key_skip_reason.is_none());
            assert!(tempo.key_peak_bytes.is_none() && tempo.key_skip_reason.is_none());
        }
        assert!(admission_peak(usize::MAX, 2, 48_000, 0, false).is_err());
        assert!(admission_peak(1, 0, 48_000, 0, false).is_err());
        assert!(admission_peak(1, 2, 1, 0, false).is_err());
        assert!(admission_peak(48_000, 2, 48_000, usize::MAX, false).is_err());
    }

    #[test]
    fn oversized_finite_source_is_refused_before_complete_pcm_read() {
        for tempo_only in [false, true] {
            let read_started = std::cell::Cell::new(false);
            let result = prepare_input(200_000_000, 2, 48_000, 48_000 * 8, tempo_only, || {
                read_started.set(true);
                panic!("oversized complete PCM must not be materialized");
            });
            assert!(result.is_err());
            assert!(!read_started.get());
        }
    }

    #[test]
    fn optional_key_overflow_does_not_refuse_complete_bpm_input() {
        let frames = 600 * 48_000;
        let full = admission_plan(frames, 2, 48_000, 0, false).unwrap();
        assert!(full.primary_peak_bytes < LIMIT);
        assert!(full.key_peak_bytes.unwrap() > LIMIT);
        assert!(full.key_skip_reason.as_deref().unwrap().contains("512 MiB"));
        assert_eq!(
            admission_peak(frames, 2, 48_000, 0, false).unwrap(),
            admission_peak(frames, 2, 48_000, 0, true).unwrap()
        );
        let read_started = std::cell::Cell::new(false);
        let result = prepare_input(frames, 2, 48_000, 0, false, || {
            read_started.set(true);
            Err("read reached after mandatory admission".into())
        });
        assert!(read_started.get());
        assert_eq!(
            result.unwrap_err(),
            "read reached after mandatory admission"
        );
    }

    #[test]
    fn primary_peak_counts_held_and_wire_at_the_exact_limit() {
        let frames = 18 * 48_000;
        let base = admission_plan(frames, 2, 48_000, 0, true).unwrap();
        let extra = LIMIT - base.primary_peak_bytes;
        let at_limit = admission_plan(frames, 2, 48_000, extra, false).unwrap();
        assert_eq!(at_limit.primary_peak_bytes, LIMIT);
        assert!(at_limit.key_skip_reason.is_some());
        assert_eq!(at_limit.held_bytes + at_limit.wire_bytes, LIMIT);
        assert!(admission_plan(frames, 2, 48_000, extra + 1, true).is_err());
        assert!(admission_plan(frames, 2, 48_000, usize::MAX, true).is_err());
    }

    #[test]
    fn skipped_unrequested_failed_and_completed_unknown_keys_remain_distinct() {
        let large = admission_plan(600 * 48_000, 2, 48_000, 0, false).unwrap();
        let skipped = optional_key(&large, false, || panic!("unadmitted key must not run"));
        assert_eq!(skipped.status, AnalysisKeyStatus::SkippedBudget);
        assert!(skipped.key.is_none() && skipped.error.is_some());
        let ordinary = admission_plan(18 * 48_000, 2, 48_000, 0, false).unwrap();
        let unrequested = optional_key(&ordinary, true, || panic!("tempo-only key must not run"));
        assert_eq!(unrequested.status, AnalysisKeyStatus::NotRequested);
        assert!(unrequested.key.is_none() && unrequested.error.is_none());
        let failed = optional_key(&ordinary, false, || Err("model unavailable".into()));
        assert_eq!(failed.status, AnalysisKeyStatus::Failed);
        assert_eq!(failed.error.as_deref(), Some("model unavailable"));
        let panicked = optional_key(&ordinary, false, || panic!("optional frontend panic"));
        assert_eq!(panicked.status, AnalysisKeyStatus::Failed);
        assert_eq!(
            panicked.error.as_deref(),
            Some("Key analysis worker panicked")
        );
        let unknown = optional_key(&ordinary, false, || Ok("unknown".into()));
        assert_eq!(unknown.status, AnalysisKeyStatus::Ready);
        assert_eq!(unknown.key.as_deref(), Some("unknown"));
        assert!(unknown.error.is_none());
    }
}

/// Shared mandatory/optional geometry at both pre-read admission and execution.
pub(super) struct AdmissionPlan {
    pub primary_peak_bytes: usize,
    pub key_peak_bytes: Option<usize>,
    pub key_skip_reason: Option<String>,
    held_bytes: usize,
    wire_bytes: usize,
}

/// Return the mandatory BPM peak; optional key work cannot veto this admission.
pub(super) fn admission_peak(
    frames: usize,
    channels: usize,
    rate: u32,
    extra_held: usize,
    tempo_only: bool,
) -> Result<usize, String> {
    Ok(admission_plan(frames, channels, rate, extra_held, tempo_only)?.primary_peak_bytes)
}

pub(super) fn admission_plan(
    frames: usize,
    channels: usize,
    rate: u32,
    extra_held: usize,
    tempo_only: bool,
) -> Result<AdmissionPlan, String> {
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
    let primary_peak_bytes = retained
        .checked_add(wire)
        .ok_or("neural analysis budget overflow")?
        .max(plan.export_peak_bytes);
    if primary_peak_bytes > LIMIT {
        return Err("neural analysis exceeds complete-source preparation budget".into());
    }
    let mut admission = AdmissionPlan {
        primary_peak_bytes,
        key_peak_bytes: None,
        key_skip_reason: None,
        held_bytes: retained,
        wire_bytes: wire,
    };
    if !tempo_only {
        match optional_key_peak(frames, rate, retained, wire, plan.key_peak_bytes) {
            Ok(peak) => {
                admission.key_peak_bytes = Some(peak);
                if peak > LIMIT {
                    admission.key_skip_reason = Some(format!(
                        "Key analysis skipped: complete-source preparation needs {peak} bytes, exceeding 512 MiB"
                    ));
                }
            }
            Err(error) => {
                admission.key_skip_reason = Some(format!("Key analysis skipped: {error}"));
            }
        }
    }
    Ok(admission)
}

fn optional_key_peak(
    frames: usize,
    rate: u32,
    held: usize,
    wire: usize,
    key_pcm_peak: usize,
) -> Result<usize, String> {
    let converted = usize::try_from((frames as u128 * 44_100).div_ceil(u128::from(rate)))
        .map_err(|_| "neural key extent overflow")?;
    let cqt = flitzis_looper_analysis::key_detection::librosa_cqt::cqt::default_cqt_working_bytes(
        converted,
    )
    .ok_or("neural key workspace overflow")?;
    let (input, output) = super::analysis_pcm::fft::fft_dimensions(rate, 44_100);
    let fft = input
        .checked_add(output)
        .and_then(|v| v.checked_mul(256))
        .ok_or("neural FFT workspace overflow")?;
    let key = key_pcm_peak
        .checked_add(fft)
        .ok_or("neural key workspace overflow")?
        .max(
            converted
                .checked_mul(12)
                .and_then(|v| v.checked_add(cqt))
                .ok_or("neural key workspace overflow")?,
        );
    held.checked_add(wire)
        .and_then(|value| value.checked_add(key))
        .ok_or_else(|| "neural key workspace overflow".into())
}

struct KeyOutcome {
    key: Option<String>,
    status: AnalysisKeyStatus,
    error: Option<String>,
}

fn optional_key(
    admission: &AdmissionPlan,
    tempo_only: bool,
    run: impl FnOnce() -> Result<String, String>,
) -> KeyOutcome {
    if tempo_only {
        return KeyOutcome {
            key: None,
            status: AnalysisKeyStatus::NotRequested,
            error: None,
        };
    }
    if let Some(error) = &admission.key_skip_reason {
        return KeyOutcome {
            key: None,
            status: AnalysisKeyStatus::SkippedBudget,
            error: Some(error.clone()),
        };
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
        .unwrap_or_else(|_| Err("Key analysis worker panicked".into()))
    {
        Ok(key) => KeyOutcome {
            key: Some(key),
            status: AnalysisKeyStatus::Ready,
            error: None,
        },
        Err(error) => KeyOutcome {
            key: None,
            status: AnalysisKeyStatus::Failed,
            error: Some(error),
        },
    }
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
    analyze_with_range(sample, rate, context, tempo_only, None)
}

pub(super) fn analyze_with_range(
    sample: &SampleBuffer,
    rate: u32,
    context: Option<NeuralContext>,
    tempo_only: bool,
    automatic_range: Option<BpmRange>,
) -> Result<LoadedAnalysis, String> {
    let Some(context) = context else {
        // Callers without a configured neural installation retain the scientific backend.
        return super::analyze_sample(sample, rate)
            .map(|result| bpm_range::scientific(automatic_range, result.into()));
    };
    let cancellation = {
        let source = context.cancelled.clone();
        let latched = AtomicBool::new(false);
        Arc::new(move || {
            if latched.load(Ordering::Acquire) { return true; }
            if source() { latched.store(true,Ordering::Release); true } else { false }
        }) as Arc<dyn Fn()->bool + Send + Sync>
    };
    let cancelled = || cancellation();
    let source_digest = context.descriptor["original"]["sha256"]
        .as_str()
        .ok_or("neural analysis original digest unavailable")?
        .to_owned();
    let decoder_identity =
        super::cold_store::descriptor_digest(&context.descriptor).map_err(|e| e.to_string())?;
    super::neural_analysis::descriptor::validate_source_descriptor(
        &context.descriptor,
        &decoder_identity,
    )?;
    let frames = sample.frame_count();
    let admission = admission_plan(
        frames,
        sample.channels,
        rate,
        context.additional_held_bytes,
        tempo_only,
    )?;
    let snapshot = LoadedPcmSnapshot::new(sample.clone(), rate, context.identity.clone(), LIMIT)
        .map_err(|e| e.to_string())?;
    let plan = snapshot.staging_plan().map_err(|e| e.to_string())?;
    // Bounded main-process protocol/tuples/codec workspace, scaled to this actual
    // complete observation extent. Model subprocess RSS is separately bounded by
    // the existing worker adapter; this is not a whole process-tree RAM claim.
    let wire = admission.wire_bytes;
    let held = admission.held_bytes;
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
        let source = BeatThisSourceInput { decoder_identity, source_descriptor: context.descriptor.clone(),
            source_sample_rate_hz: rate, source_frame_count: frames, source_zero_frame: 0,
            mono_sha256: mono_sha256.clone(), mono_rule: super::analysis_pcm::MONO_RULE.into() };
        let identity = json!({"pad_id":context.identity.pad_id,"request_id":context.identity.request_id,
            "source_id":context.identity.source_id,"source_generation":context.identity.source_generation});
        let probe = Python::attach(|py| Py::new(py, AnalysisCancellation(cancellation.clone())))
            .map_err(|e| e.to_string())?.into_any();
        let (beat, key_outcome) = std::thread::scope(|scope| {
            let beat = scope.spawn(|| invoke_worker(source, identity, path_text, &context.installation, probe));
            let key = optional_key(&admission, tempo_only, || {
                let mut key_reader = file.try_clone().map_err(|e| e.to_string())?;
                key_reader.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                let input = key_input_from_f32_le(&mut key_reader, frames, rate, LIMIT - held - wire, &cancelled)
                    .map_err(|e| e.to_string())?;
                let mut widened = Vec::new();
                widened.try_reserve_exact(input.samples.len())
                    .map_err(|_| "Key widened input allocation unavailable".to_owned())?;
                widened.extend(input.samples.iter().map(|&value| f64::from(value)));
                drop(input);
                super::run_key_detection_result(&widened)
            });
            (beat.join().unwrap_or_else(|_| Err("neural worker panicked".into())), key)
        });
        verify_staged(&mut file, plan.export_bytes, &mono_sha256, &cancelled)?;
        if cancelled() { return Err("neural analysis request superseded".into()); }
        let KeyOutcome { key, status: key_status, error: key_error } = key_outcome;
        let (raw, _) = match beat {
            Ok(raw) => raw,
            Err(error) if tempo_only => return Err(error),
            Err(error) => return Ok(LoadedAnalysis { bpm: 0.0, key, key_status, key_error,
                beat_grid: flitzis_looper_analysis::BeatGrid { beats: vec![], downbeats: vec![], bars: vec![] },
                bpm_method_json: None, beat_error: Some(error) }),
        };
        let descriptor_bytes = super::neural_analysis::budget::value_tree(&context.descriptor)?;
        let source_live = super::neural_analysis::budget::sum(&[held, descriptor_bytes,
            source_digest.capacity(), mono_sha256.capacity(), key.as_ref().map_or(0, String::capacity),
            key_error.as_ref().map_or(0, String::capacity)])?;
        let predictions = raw.predictions();
        let raw_workspace = raw.encoding_workspace_bytes()?;
        let available = LIMIT.checked_sub(source_live).and_then(|bytes| bytes.checked_sub(raw_workspace)).unwrap_or(0);
        // Successful v4 estimates retain their established preparation path. New
        // fallback search/typed evidence/JSON storage is admitted before allocation.
        let estimate = super::app_tempo::estimate_with_budget_cancelled(
            &predictions.beat_seconds, frames as f64 / f64::from(rate),
            predictions.downbeat_seconds.len(), available,
            &cancelled,
        ).map_err(|_| "neural analysis request superseded".to_owned())?;
        if cancelled() { return Err("neural analysis request superseded".into()); }
        let phase_period = super::app_tempo::phase_only_period(
            &predictions.beat_seconds,frames as f64 / f64::from(rate),&estimate,&cancelled,
        ).map_err(|_| "neural analysis request superseded".to_owned())?;
        let schema_version = super::app_tempo::schema_version(&estimate);
        let estimate_bpm = estimate.bpm;
        let estimate_status = estimate.status;
        let mut method = serde_json::to_value(&estimate).map_err(|e| e.to_string())?;
        // The admitted preparation reserve covers typed/JSON coexistence above;
        // final capacity accounting below must not retain a second typed method.
        drop(estimate);
        let object = method.as_object_mut().ok_or("neural estimate envelope missing")?;
        object.extend(json!({"schema_version":schema_version,"backend":"beat_this","model_sha256":pinned_model()["sha256"],
            "frontend_id":pinned_model()["frontend_id"],"source_sha256":source_digest,
            "mono_sha256":mono_sha256,"sample_rate_hz":rate,"frame_count":frames,
            "raw_beats":predictions.beat_seconds,"raw_downbeats":predictions.downbeat_seconds}).as_object().unwrap().clone());
        if let Some(execution) = raw.execution() {
            object.insert("execution".into(), execution);
        }
        let method_tree = super::neural_analysis::budget::value_tree(&method)?;
        let method_len = super::neural_analysis::budget::serialized_len(&method)?;
        let grid_bytes = super::neural_analysis::budget::sum(&[
            predictions.beat_seconds.len().max(4).checked_mul(4).ok_or("neural grid budget overflow")?,
            predictions.downbeat_seconds.len().max(4).checked_mul(8).ok_or("neural downbeat budget overflow")?,
        ])?;
        let core = super::neural_analysis::budget::sum(&[source_live, raw_workspace,
            method_tree, method_len.checked_mul(2).ok_or("neural method budget overflow")?, grid_bytes,
            std::mem::size_of::<LoadedAnalysis>()])?.max(super::neural_analysis::budget::sum(&[held, wire])?);
        if core > LIMIT { return Err("neural analysis metadata exceeds preparation budget".into()); }
        if cancelled() { return Err("neural analysis request superseded".into()); }
        // Keep the exact old string and one tree. An optional late failure must
        // never reconstruct floats or partially publish the joint choice.
        let base_method_json = method.to_string();
        let base_string_capacity = base_method_json.capacity();
        let mut selected_bpm = estimate_bpm;
        let mut beat_error = estimate_bpm.is_none().then(||format!("Neural tempo estimate {estimate_status}"));
        let method_json = if let Some(period) = phase_period {
            let attempted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                qm_corroboration::attempt(sample,rate,&context,&mono_sha256,raw.decoder_identity(),period,
                    &mut method,&base_method_json,core,source_live,raw_workspace,grid_bytes,&cancelled)
            }));
            if cancelled() { return Err("neural analysis request superseded".into()); }
            match attempted {
                Ok(Ok(choice)) => { drop(base_method_json); selected_bpm=Some(choice.bpm); beat_error=None; choice.method_json },
                Ok(Err(qm_corroboration::Failure::Cancelled)) => return Err("neural analysis request superseded".into()),
                Ok(Err(qm_corroboration::Failure::NoChoice)) => {
                    beat_error=Some(format!("Neural tempo estimate {estimate_status}; QM corroboration unavailable: QM has no independently admitted tempo"));
                    base_method_json
                },
                Ok(Err(qm_corroboration::Failure::Unavailable(reason))) => {
                    beat_error=Some(format!("Neural tempo estimate {estimate_status}; QM corroboration unavailable: {reason}"));
                    base_method_json
                },
                Err(_) => {
                    beat_error=Some(format!("Neural tempo estimate {estimate_status}; QM corroboration unavailable: analysis panicked"));
                    base_method_json
                },
            }
        } else { base_method_json };
        // Accurate primary period is independent of musical half/double choice.
        // Fresh publication never probes or executes the optional nominal model.
        let method_json = if let Some((range, primary_bpm)) = automatic_range.zip(selected_bpm)
            .filter(|(range,bpm)| !range.contains(*bpm)) {
            let primary_core = super::neural_analysis::budget::value_tree(&method).and_then(|tree| {
                let strings = method_json.capacity().checked_mul(2).ok_or("range method budget overflow")?;
                super::neural_analysis::budget::sum(&[source_live,raw_workspace,tree,strings,
                    base_string_capacity,grid_bytes,std::mem::size_of::<LoadedAnalysis>()])
            }).unwrap_or(usize::MAX).max(core);
            let choice = bpm_range::apply(range,primary_bpm,&mut method,method_json,
                primary_core,source_live,raw_workspace,grid_bytes,&cancelled,|work_core| {
                    qm_corroboration::observation(sample,rate,&context,&mono_sha256,
                        raw.decoder_identity(),work_core,&cancelled,|bpm,extent| {
                            if !range.contains(bpm) {
                                return Err(qm_corroboration::Failure::Unavailable(
                                    "independent QM estimate outside the requested range".into()));
                            }
                            bpm_range::half_double_drift(primary_bpm,bpm,extent)
                        })
                }).map_err(|_| "neural analysis request superseded".to_owned())?;
            selected_bpm=choice.bpm;
            beat_error=choice.beat_error;
            choice.method_json
        } else { Some(method_json) };
        if selected_bpm.is_none() {
            if let Some(range) = automatic_range {
                if beat_error.as_ref().is_none_or(|reason| !reason.starts_with("No automatic BPM within")) {
                    beat_error=Some(range.diagnostic(beat_error.as_deref().unwrap_or("no independently admitted tempo")));
                }
            }
        }
        drop(method);
        let beats = predictions.beat_seconds.iter().map(|&v| v as f32).collect();
        let downbeats: Vec<_> = predictions.downbeat_seconds.iter().map(|&v| v as f32).collect();
        if cancelled() { return Err("neural analysis request superseded".into()); }
        Ok(LoadedAnalysis { bpm: selected_bpm.unwrap_or(0.0), key, key_status, key_error,
            beat_grid: flitzis_looper_analysis::BeatGrid { beats, bars: downbeats.clone(), downbeats },
            bpm_method_json: method_json,
            beat_error })
    })).unwrap_or_else(|_| Err("neural analysis worker panicked".into()));
    match (result, temporary.close()) {
        (result, Ok(())) => result,
        (_, Err(error)) => Err(format!("neural staging cleanup failed: {error}")),
    }
}
