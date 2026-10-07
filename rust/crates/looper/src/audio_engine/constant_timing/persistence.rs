//! Complete, content-verified saved evidence and fresh guarded runtime adoption.
//! JSON, original-file hashing, PCM conversion and evidence reconstruction are
//! non-realtime. Historical job tokens remain provenance, never current identity.

use super::*;
use crate::audio_engine::prepared_source::file_sha256;
use flitzis_looper_analysis::QmRawAnalysis;
use serde_json::json;
use std::path::Path;

const ENCODING: &str = "accepted-constant-timing-qm-raw-v1";

/// Fresh source/request capture made before background restore work begins.
#[pyclass(frozen)]
pub struct SavedConstantTimingTicket {
    id: usize,
    request_id: u64,
    source_generation: u64,
    sample_rate_hz: u32,
    source_digest: String,
    source_path: String,
    sample: SampleBuffer,
    epoch: Arc<AtomicU64>,
    captured_epoch: u64,
    record: Value,
    pcm_budget: PcmBudget,
}

fn object<'a>(
    value: &'a Value,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let fields = value.as_object().ok_or("invalid saved timing object")?;
    if fields.len() != keys.len() || keys.iter().any(|key| !fields.contains_key(*key)) {
        return Err("unsupported or incomplete saved timing fields".into());
    }
    Ok(fields)
}

fn text(value: &Value) -> Result<String, String> {
    value
        .as_str()
        .filter(|v| !v.trim().is_empty() && v.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| "invalid saved timing text".into())
}

fn number(value: &Value) -> Result<u64, String> {
    value
        .as_u64()
        .ok_or_else(|| "invalid saved timing integer".into())
}

fn rate(value: &Value) -> Result<u32, String> {
    u32::try_from(number(value)?).map_err(|_| "invalid saved timing rate".into())
}

fn dimension(value: &Value) -> Result<usize, String> {
    usize::try_from(number(value)?).map_err(|_| "invalid saved timing extent".into())
}

fn array(value: &Value) -> Result<&Vec<Value>, String> {
    value
        .as_array()
        .filter(|v| v.len() <= flitzis_looper_analysis::tempo_summary::MAX_RAW_POSITIONS)
        .ok_or_else(|| "invalid complete saved QM array".into())
}

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn float(value: &Value) -> Result<f64, String> {
    let encoded = value
        .as_str()
        .filter(|v| {
            v.len() == 16
                && v.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or("invalid binary64 saved timing bits")?;
    let value =
        f64::from_bits(u64::from_str_radix(encoded, 16).map_err(|_| "invalid binary64 bits")?);
    if !value.is_finite() {
        return Err("nonfinite saved timing value".into());
    }
    Ok(value)
}

fn job(value: &JobIdentity) -> Value {
    json!({"pad_id":value.pad_id,"request_id":value.request_id,"source_id":value.source_id,"source_generation":value.source_generation})
}

fn parse_job(value: &Value) -> Result<JobIdentity, String> {
    object(
        value,
        &["pad_id", "request_id", "source_id", "source_generation"],
    )?;
    Ok(JobIdentity {
        pad_id: number(&value["pad_id"])?,
        request_id: number(&value["request_id"])?,
        source_id: text(&value["source_id"])?,
        source_generation: number(&value["source_generation"])?,
    })
}

fn encode(timing: &AcceptedConstantTiming) -> Result<Value, String> {
    if timing.refinement().is_some() || timing.independent_quarters().is_some() {
        return Err("unsupported saved refinement evidence".into());
    }
    let BackendEvidence::Qm { raw, input } = timing.evidence().backend() else {
        return Err("unsupported saved timing backend evidence".into());
    };
    let binding = timing.evidence().binding();
    let config = raw.configuration();
    let transform = match &input.transform {
        QmInputTransform::Identity => json!({"kind":"identity"}),
        QmInputTransform::Rubato44100 {
            revision,
            provenance,
        } => json!({"kind":"rubato44100","revision":revision,"provenance":provenance}),
    };
    let hypotheses: Vec<Value> = timing.summary().hypotheses.iter().map(|h| json!({
        "id":h.id,"provenance":h.provenance,"verification":match h.verification { QuarterNoteVerification::Verified=>"verified", QuarterNoteVerification::Unverified=>"unverified" },
        "quarter_note_denominator":h.quarter_note_denominator,"quarter_counts":h.quarter_counts,
    })).collect();
    Ok(json!({"schema_version":1,"encoding":ENCODING,"record":{
        "accepted_revision":timing.revision(), "period_bits":bits(timing.period_seconds_per_quarter()),
        "origin":{"seconds_bits":bits(timing.origin().seconds),"provenance":timing.origin().provenance},
        "decision":{"policy_version":timing.decision().policy_version,"provenance":timing.decision().provenance},
        "hypotheses":hypotheses,
        "evidence":{
            "binding":{"job":job(&binding.job),"source_sha256":binding.source_sha256,"source_provenance":binding.source_provenance,
                "pcm_sha256":binding.pcm_sha256,"sample_rate_hz":binding.sample_rate_hz,"frame_count":binding.frame_count,
                "source_zero_bits":bits(binding.origin_seconds),"mono_revision":binding.mono_revision},
            "timing_bound":{"halfwidth_bits":bits(timing.evidence().timing_bound().halfwidth_seconds),"provenance":timing.evidence().timing_bound().provenance},
            "independent_origin_bits":bits(timing.evidence().raw_evidence().independent_origin_seconds),
            "qm":{"beat_frame_bits":raw.beat_frames().iter().copied().map(bits).collect::<Vec<_>>(),"downbeat_raw_indices":raw.downbeat_raw_indices(),
                "odf_hop_samples":raw.odf_hop_samples(),
                "input":{"job":job(&input.job),"pcm_sha256":input.pcm_sha256,"input_sha256":input.input_sha256,"sample_rate_hz":input.sample_rate_hz,"frame_count":input.frame_count,
                    "source_zero_bits":bits(input.origin_seconds),"transform":transform},
                "configuration":{"step_secs_bits":bits(config.step_secs),"max_bin_hz_bits":bits(config.max_bin_hz),"input_tempo_bits":bits(config.input_tempo),"alpha_bits":bits(config.alpha),"tightness_bits":bits(config.tightness),"viterbi_sigma_bits":bits(config.viterbi_sigma),"window_length":config.window_length,"hop_size":config.hop_size}
            }
        }
    }}))
}

fn parse_binding(value: &Value) -> Result<PcmBindingMetadata, String> {
    object(
        value,
        &[
            "job",
            "source_sha256",
            "source_provenance",
            "pcm_sha256",
            "sample_rate_hz",
            "frame_count",
            "source_zero_bits",
            "mono_revision",
        ],
    )?;
    Ok(PcmBindingMetadata {
        job: parse_job(&value["job"])?,
        source_sha256: text(&value["source_sha256"])?,
        source_provenance: text(&value["source_provenance"])?,
        pcm_sha256: text(&value["pcm_sha256"])?,
        sample_rate_hz: rate(&value["sample_rate_hz"])?,
        frame_count: number(&value["frame_count"])?,
        origin_seconds: float(&value["source_zero_bits"])?,
        mono_revision: text(&value["mono_revision"])?,
    })
}

fn parse_input(value: &Value) -> Result<QmInputDescriptor, String> {
    object(
        value,
        &[
            "job",
            "pcm_sha256",
            "input_sha256",
            "sample_rate_hz",
            "frame_count",
            "source_zero_bits",
            "transform",
        ],
    )?;
    let transform = &value["transform"];
    let transform = match transform["kind"].as_str() {
        Some("identity") => {
            object(transform, &["kind"])?;
            QmInputTransform::Identity
        }
        Some("rubato44100") => {
            object(transform, &["kind", "revision", "provenance"])?;
            QmInputTransform::Rubato44100 {
                revision: text(&transform["revision"])?,
                provenance: text(&transform["provenance"])?,
            }
        }
        _ => return Err("unsupported saved QM transform".into()),
    };
    Ok(QmInputDescriptor {
        job: parse_job(&value["job"])?,
        pcm_sha256: text(&value["pcm_sha256"])?,
        input_sha256: text(&value["input_sha256"])?,
        sample_rate_hz: rate(&value["sample_rate_hz"])?,
        frame_count: number(&value["frame_count"])?,
        origin_seconds: float(&value["source_zero_bits"])?,
        transform,
    })
}

fn decode(
    record: &Value,
    mono: &[f32],
    analyzer: &[f64],
    actual_digest: &str,
    actual_rate: u32,
) -> Result<AcceptedConstantTiming, String> {
    object(
        record,
        &[
            "accepted_revision",
            "period_bits",
            "origin",
            "decision",
            "hypotheses",
            "evidence",
        ],
    )?;
    let evidence = &record["evidence"];
    object(
        evidence,
        &["binding", "timing_bound", "independent_origin_bits", "qm"],
    )?;
    let metadata = parse_binding(&evidence["binding"])?;
    if metadata.source_sha256 != actual_digest
        || metadata.sample_rate_hz != actual_rate
        || metadata.source_provenance != SOURCE_PROVENANCE
    {
        return Err(
            "saved source digest, rate or provenance differs from actual loaded source".into(),
        );
    }
    let binding = PcmBinding::verify(mono, metadata).map_err(|e| e.to_string())?;
    let qm = &evidence["qm"];
    object(
        qm,
        &[
            "beat_frame_bits",
            "downbeat_raw_indices",
            "odf_hop_samples",
            "input",
            "configuration",
        ],
    )?;
    let config = &qm["configuration"];
    object(
        config,
        &[
            "step_secs_bits",
            "max_bin_hz_bits",
            "input_tempo_bits",
            "alpha_bits",
            "tightness_bits",
            "viterbi_sigma_bits",
            "window_length",
            "hop_size",
        ],
    )?;
    let config = AnalysisConfig {
        step_secs: float(&config["step_secs_bits"])?,
        max_bin_hz: float(&config["max_bin_hz_bits"])?,
        input_tempo: float(&config["input_tempo_bits"])?,
        alpha: float(&config["alpha_bits"])?,
        tightness: float(&config["tightness_bits"])?,
        viterbi_sigma: float(&config["viterbi_sigma_bits"])?,
        window_length: dimension(&config["window_length"])?,
        hop_size: dimension(&config["hop_size"])?,
    };
    let frames = array(&qm["beat_frame_bits"])?
        .iter()
        .map(float)
        .collect::<Result<Vec<_>, _>>()?;
    let downbeats = array(&qm["downbeat_raw_indices"])?
        .iter()
        .map(dimension)
        .collect::<Result<Vec<_>, _>>()?;
    let input = parse_input(&qm["input"])?;
    let raw = QmRawAnalysis::from_complete_capture(
        frames,
        downbeats,
        input.sample_rate_hz,
        dimension(&qm["input"]["frame_count"])?,
        dimension(&qm["odf_hop_samples"])?,
        config,
    )?;
    let bound = &evidence["timing_bound"];
    object(bound, &["halfwidth_bits", "provenance"])?;
    let evidence = BoundTempoEvidence::from_qm(
        &binding,
        raw,
        analyzer,
        input,
        TimingBound {
            halfwidth_seconds: float(&bound["halfwidth_bits"])?,
            provenance: text(&bound["provenance"])?,
        },
        float(&evidence["independent_origin_bits"])?,
    )
    .map_err(|e| e.to_string())?;
    let hypotheses = parse_hypotheses(&record["hypotheses"].to_string())?;
    let borrowed: Vec<_> = hypotheses
        .iter()
        .map(|h| QuarterNoteHypothesis {
            id: &h.id,
            provenance: &h.provenance,
            verification: h.verification,
            quarter_note_denominator: h.denominator,
            quarter_counts: &h.counts,
        })
        .collect();
    let origin = &record["origin"];
    object(origin, &["seconds_bits", "provenance"])?;
    let decision = &record["decision"];
    object(decision, &["policy_version", "provenance"])?;
    let accepted = AcceptedConstantTiming::from_raw(
        evidence,
        &borrowed,
        IndependentTimingOrigin {
            seconds: float(&origin["seconds_bits"])?,
            provenance: text(&origin["provenance"])?,
        },
        TimingAcceptanceDecision {
            policy_version: text(&decision["policy_version"])?,
            provenance: text(&decision["provenance"])?,
        },
    )
    .map_err(|e| e.to_string())?;
    if accepted.revision() != text(&record["accepted_revision"])?
        || accepted.period_seconds_per_quarter().to_bits()
            != float(&record["period_bits"])?.to_bits()
    {
        return Err("saved complete accepted identity or binary64 period mismatch".into());
    }
    Ok(accepted)
}

fn verify(
    engine: &AudioEngine,
    ticket: &SavedConstantTimingTicket,
) -> Result<
    (
        AcceptedConstantTiming,
        PcmBindingMetadata,
        TimingAdoptionGuard,
    ),
    String,
> {
    let pcm_limit_bytes = ticket.pcm_budget.limit_bytes();
    let cancelled = || ticket.epoch.load(Ordering::Acquire) != ticket.captured_epoch;
    if cancelled() {
        return Err("saved timing restore cancelled".into());
    }
    let actual_digest = file_sha256(Path::new(&ticket.source_path))?;
    if actual_digest != ticket.source_digest {
        return Err("actual source file differs from loaded source".into());
    }
    check_timing_pcm_geometry(&ticket.sample, ticket.sample_rate_hz, ticket.pcm_budget)?;
    let (complete, retained_reference_bytes) =
        complete_timing_sample(engine, ticket.id, &ticket.sample, ticket.pcm_budget)?;
    if cancelled() {
        return Err("saved timing restore cancelled".into());
    }
    let snapshot = LoadedPcmSnapshot::new(
        complete,
        ticket.sample_rate_hz,
        PcmIdentity {
            pad_id: ticket.id,
            request_id: ticket.request_id,
            source_id: format!("loaded-{}-{}", ticket.id, ticket.source_generation),
            source_generation: ticket.source_generation,
        },
        pcm_limit_bytes,
    )
    .map_err(|e| e.to_string())?;
    let mono_budget = pcm_limit_bytes
        .checked_sub(snapshot.retained_bytes())
        .and_then(|bytes| bytes.checked_sub(retained_reference_bytes))
        .ok_or("timing PCM byte limit exceeded")?;
    let mono = snapshot
        .prepare_complete_mono(mono_budget, &cancelled)
        .map_err(|e| e.to_string())?;
    let retained = snapshot
        .retained_bytes()
        .checked_add(retained_reference_bytes)
        .and_then(|bytes| bytes.checked_add(mono.capacity() * 4))
        .ok_or("timing PCM bytes overflow")?;
    let budget = pcm_limit_bytes
        .checked_sub(retained)
        .ok_or("timing PCM byte limit exceeded")?;
    if mono
        .len()
        .checked_mul(size_of::<f32>())
        .is_none_or(|bytes| bytes > budget)
    {
        return Err("timing PCM byte limit exceeded".into());
    }
    let converted = resample_mono_cancellable(
        mono.clone(),
        ticket.sample_rate_hz,
        44_100,
        budget,
        &cancelled,
    )
    .map_err(|e| e.to_string())?;
    if retained as u128 + converted.capacity() as u128 * 4 + converted.len() as u128 * 8
        > pcm_limit_bytes as u128
    {
        return Err("timing PCM byte limit exceeded".into());
    }
    let analyzer: Vec<f64> = converted.into_iter().map(f64::from).collect();
    let accepted = decode(
        &ticket.record,
        &mono,
        &analyzer,
        &actual_digest,
        ticket.sample_rate_hz,
    )?;
    if file_sha256(Path::new(&ticket.source_path))? != actual_digest {
        return Err("source file changed while verifying saved evidence".into());
    }
    if cancelled() {
        return Err("saved timing restore cancelled".into());
    }
    let mut fresh = accepted.evidence().binding().clone();
    fresh.job = JobIdentity {
        pad_id: ticket.id as u64,
        request_id: ticket.request_id,
        source_id: format!("loaded-{}-{}", ticket.id, ticket.source_generation),
        source_generation: ticket.source_generation,
    };
    let actual_binding = PcmBinding::verify(&mono, fresh.clone()).map_err(|e| e.to_string())?;
    let guard = TimingAdoptionGuard::new(&actual_binding, TimingIntent::Automatic)
        .map_err(|e| e.to_string())?;
    Ok((accepted, fresh, guard))
}

fn validate_capture(
    engine: &AudioEngine,
    ticket: &SavedConstantTimingTicket,
) -> Result<(), String> {
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| "request lock poisoned")?;
    if !Arc::ptr_eq(&ticket.epoch, &engine.prepared_source_epochs[ticket.id])
        || requests[ticket.id] != ticket.request_id
        || ticket.epoch.load(Ordering::Acquire) != ticket.captured_epoch
    {
        return Err("stale or foreign saved timing capture".into());
    }
    if engine
        .timing_intents
        .lock()
        .map_err(|_| "timing intent lock poisoned")?[ticket.id]
        != TimingIntent::Automatic
    {
        return Err("current timing intent is not automatic".into());
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| "sample cache lock poisoned")?;
    let source = cache[ticket.id].as_ref().ok_or("source unavailable")?;
    if !source.same_source(&ticket.sample)
        || engine
            .loaded_source_generations
            .lock()
            .map_err(|_| "source generation lock poisoned")?[ticket.id]
            != (ticket.source_generation, ticket.sample_rate_hz)
        || engine
            .loaded_source_digests
            .lock()
            .map_err(|_| "source digest lock poisoned")?[ticket.id]
            .as_ref()
            != Some(&ticket.source_digest)
    {
        return Err("current source ownership differs from saved timing capture".into());
    }
    Ok(())
}

pub(in crate::audio_engine) fn capture_saved(
    engine: &AudioEngine,
    id: usize,
    json: &str,
    source_path: String,
) -> Result<SavedConstantTimingTicket, String> {
    if id >= NUM_SAMPLES
        || json.len() > MAX_HYPOTHESIS_JSON_BYTES
        || source_path.is_empty()
        || source_path.len() > 4096
    {
        return Err("invalid saved timing admission".into());
    }
    let envelope: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    object(&envelope, &["schema_version", "encoding", "record"])?;
    if envelope["schema_version"].as_u64() != Some(1)
        || envelope["encoding"].as_str() != Some(ENCODING)
    {
        return Err("unsupported saved timing schema or evidence".into());
    }
    let record = envelope["record"].clone();
    object(
        &record,
        &[
            "accepted_revision",
            "period_bits",
            "origin",
            "decision",
            "hypotheses",
            "evidence",
        ],
    )?;
    let mut requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| "request lock poisoned")?;
    if engine
        .timing_intents
        .lock()
        .map_err(|_| "timing intent lock poisoned")?[id]
        != TimingIntent::Automatic
    {
        return Err("current timing intent is not automatic".into());
    }
    if engine
        .loading_sample_ids
        .lock()
        .map_err(|_| "loading lock poisoned")?
        .contains(&id)
    {
        return Err("sample is currently loading".into());
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| "sample cache lock poisoned")?;
    let sample = cache[id].clone().ok_or("sample is not loaded")?;
    let (source_generation, sample_rate_hz) = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| "source generation lock poisoned")?[id];
    let source_digest = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| "source digest lock poisoned")?[id]
        .clone()
        .ok_or("source digest unavailable")?;
    if source_generation == 0 || requests[id] == 0 {
        return Err("loaded source identity unavailable".into());
    }
    let request_id =
        PadRequestAdvance::prepare(&mut requests[id], &engine.prepared_source_epochs[id])?.commit();
    Ok(SavedConstantTimingTicket {
        id,
        request_id,
        source_generation,
        sample_rate_hz,
        source_digest,
        source_path,
        sample,
        epoch: engine.prepared_source_epochs[id].clone(),
        captured_epoch: engine.prepared_source_epochs[id].load(Ordering::Acquire),
        record,
        pcm_budget: PcmBudget::default(),
    })
}

pub(in crate::audio_engine) fn restore_saved(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    saved: &SavedConstantTimingTicket,
) -> PyResult<ConstantTimingTicket> {
    if engine
        .constant_timing_busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(PyValueError::new_err("constant timing preparation busy"));
    }
    let _lease = PreparationLease(&engine.constant_timing_busy);
    validate_capture(engine, saved).map_err(PyValueError::new_err)?;
    let (accepted, binding, mut guard) = verify(engine, saved).map_err(PyValueError::new_err)?;
    validate_capture(engine, saved).map_err(PyValueError::new_err)?;
    // Fresh verified binding initializes the guard; historical accepted evidence
    // stays immutable and is admitted only through source-verified adoption.
    let adoption_ticket = guard
        .issue_ticket()
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let ticket = ConstantTimingTicket {
        id: saved.id,
        request_id: saved.request_id,
        source_generation: saved.source_generation,
        sample_rate_hz: saved.sample_rate_hz,
        source_digest: saved.source_digest.clone(),
        sample: saved.sample.clone(),
        epoch: saved.epoch.clone(),
        captured_epoch: saved.captured_epoch,
        evidence: accepted.evidence().clone(),
        binding,
        guard: Mutex::new(guard),
        adoption_ticket,
        publication: Mutex::new(None),
        pcm_budget: saved.pcm_budget,
    };
    publish_record(engine, producer, &ticket, accepted, true)?;
    Ok(ticket)
}

pub(in crate::audio_engine) fn export_current(
    engine: &AudioEngine,
    id: usize,
    source_path: String,
) -> Result<Option<String>, String> {
    if id >= NUM_SAMPLES {
        return Err("id out of range".into());
    }
    let captured = {
        let requests = engine
            .pad_request_ids
            .lock()
            .map_err(|_| "request lock poisoned")?;
        if engine
            .timing_intents
            .lock()
            .map_err(|_| "timing intent lock poisoned")?[id]
            != TimingIntent::Automatic
        {
            return Ok(None);
        }
        let cache = engine
            .sample_cache
            .lock()
            .map_err(|_| "sample cache lock poisoned")?;
        let Some(sample) = cache[id].as_ref() else {
            return Ok(None);
        };
        let generations = engine
            .loaded_source_generations
            .lock()
            .map_err(|_| "source generation lock poisoned")?;
        let digests = engine
            .loaded_source_digests
            .lock()
            .map_err(|_| "source digest lock poisoned")?;
        let mut all = engine
            .current_constant_timing
            .lock()
            .map_err(|_| "current timing lock poisoned")?;
        retire_old_current_records(engine, id, &mut all[id]);
        let Some(current) = current_record_for_source(
            engine,
            id,
            sample,
            generations[id],
            digests[id].as_deref(),
            &all[id],
        ) else {
            return Ok(None);
        };
        let Ok(envelope) = encode(&current.accepted) else {
            return Ok(None);
        };
        (
            SavedConstantTimingTicket {
                id,
                request_id: requests[id],
                source_generation: generations[id].0,
                sample_rate_hz: generations[id].1,
                source_digest: current.binding.source_sha256.clone(),
                source_path,
                sample: sample.clone(),
                epoch: engine.prepared_source_epochs[id].clone(),
                captured_epoch: engine.prepared_source_epochs[id].load(Ordering::Acquire),
                record: envelope["record"].clone(),
                pcm_budget: current.pcm_budget,
            },
            current.publication_epoch,
            envelope,
        )
    };
    if engine
        .constant_timing_busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("constant timing preparation busy".into());
    }
    let _lease = PreparationLease(&engine.constant_timing_busy);
    // A failed content/source verification cannot save previously accepted data.
    verify(engine, &captured.0)?;
    validate_capture(engine, &captured.0)?;
    if engine.current_timing_acknowledgements.current_epoch(id) != captured.1 {
        return Ok(None);
    }
    let encoded = captured.2.to_string();
    if encoded.len() > MAX_HYPOTHESIS_JSON_BYTES {
        return Err("complete saved timing byte limit exceeded".into());
    }
    Ok(Some(encoded))
}

#[cfg(test)]
#[path = "../constant_timing_persistence_tests.rs"]
mod tests;
