//! Complete selected-model observations, separate from musical interpretation.

use super::super::analysis_pcm::MONO_RULE;
use super::super::cold_store::canonical;
use super::super::source_beatmap_raw_data::{
    MAX_RAW_BYTES, MAX_RAW_EVENTS, bounded_descriptor, validate_source_descriptor,
};
use super::codec;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(in crate::audio_engine) const ENCODING: &str = "source-beatmap-unreviewed-beat-this-raw-v2";
pub(in crate::audio_engine) const WORKER_ENCODING: &str = "source-map-beat-this-worker-v1";
const CODE_REVISION: &str = "native-beat-this-original-complete-v1";
const MODEL_SHA256: &str = "8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331";
const ENVIRONMENT_ID: &str =
    "uv-lock-sha256:e7d180bd53a02736693e0054dfb62a868da77478922b8ca3027c07fe12f98a75";
const FRONTEND_ID: &str = "beat-this-1.1.0-soxr-hq-logmel-v1";
const MAX_REQUEST_BYTES: usize = 32 * 1024;

/// Actual Native stream metadata; construction remains private to the map job.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::audio_engine) struct BeatThisSourceInput {
    pub(in crate::audio_engine) decoder_identity: String,
    #[serde(deserialize_with = "bounded_descriptor")]
    pub(in crate::audio_engine) source_descriptor: Value,
    pub(in crate::audio_engine) source_sample_rate_hz: u32,
    pub(in crate::audio_engine) source_frame_count: usize,
    pub(in crate::audio_engine) source_zero_frame: u64,
    pub(in crate::audio_engine) mono_sha256: String,
    pub(in crate::audio_engine) mono_rule: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    pad_id: u64,
    request_id: u64,
    source_id: String,
    source_generation: u64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Model {
    sha256: String,
    frontend_id: String,
    environment_id: String,
    package_version: String,
    checkpoint: String,
    postprocessor: String,
    device: String,
    precision: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Pcm {
    path: String,
    sample_rate_hz: u32,
    frame_count: usize,
    origin_seconds: f64,
    dtype: String,
    channels: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    identity: Identity,
    pcm: Pcm,
    model: Model,
    schema_version: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Input {
    sample_rate_hz: u32,
    frame_count: usize,
    source_zero_frame: u64,
    mono_sha256: String,
    dtype: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::audio_engine) struct Predictions {
    encoding: String,
    #[serde(with = "codec::binary64")]
    pub(in crate::audio_engine) beat_seconds: Vec<f64>,
    #[serde(with = "codec::binary64")]
    pub(in crate::audio_engine) downbeat_seconds: Vec<f64>,
    #[serde(with = "codec::binary64")]
    pub(super) beat_logits: Vec<f64>,
    #[serde(with = "codec::binary64")]
    pub(super) downbeat_logits: Vec<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkerResult {
    schema_version: u32,
    encoding: String,
    identity: Identity,
    input: Input,
    model: Option<Model>,
    status: String,
    reason: String,
    resources_released: bool,
    predictions: Option<Predictions>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::audio_engine) struct BeatThisRawData {
    pub(super) source: BeatThisSourceInput,
    request: Request,
    result: WorkerResult,
    code_revision: String,
    #[serde(skip)]
    pub(super) raw_revision: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::audio_engine) struct Envelope {
    schema_version: u32,
    encoding: String,
    raw_revision: String,
    data: BeatThisRawData,
}

#[derive(Serialize)]
struct Encoded<'a> {
    schema_version: u32,
    encoding: &'static str,
    raw_revision: &'a str,
    data: &'a BeatThisRawData,
}

pub(in crate::audio_engine) fn pinned_model() -> Value {
    json!({"sha256":MODEL_SHA256,"frontend_id":FRONTEND_ID,"environment_id":ENVIRONMENT_ID,
        "package_version":"1.1.0","checkpoint":"final0","postprocessor":"minimal",
        "device":"cpu","precision":"float32"})
}

pub(in crate::audio_engine) fn expected_logit_count(
    frames: usize,
    rate: u32,
) -> Result<usize, String> {
    if frames == 0 || !(8_000..=384_000).contains(&rate) {
        return Err("invalid Beat This complete input extent".into());
    }
    let resampled = (2 * frames as u128 * 22_050 + u128::from(rate)) / (2 * u128::from(rate));
    if resampled <= 512 {
        return Err("Beat This input is too short for reference padding".into());
    }
    let count =
        usize::try_from(resampled / 441 + 1).map_err(|_| "Beat This logit extent overflow")?;
    if count > MAX_RAW_EVENTS {
        return Err("Beat This complete logits exceed 250000 values".into());
    }
    Ok(count)
}

impl BeatThisRawData {
    pub(in crate::audio_engine) fn from_worker(
        mut source: BeatThisSourceInput,
        expected_request_json: &str,
        result_json: &str,
    ) -> Result<Self, String> {
        if expected_request_json.is_empty()
            || expected_request_json.len() > MAX_REQUEST_BYTES
            || result_json.is_empty()
            || result_json.len() > MAX_RAW_BYTES
        {
            return Err("Beat This capture request or result byte limit".into());
        }
        source.source_descriptor = canonical(&source.source_descriptor);
        let request: Request =
            serde_json::from_str(expected_request_json).map_err(|e| e.to_string())?;
        let result: WorkerResult = serde_json::from_str(result_json).map_err(|e| e.to_string())?;
        if result.status != "ready"
            && matches!(
                result.status.as_str(),
                "unavailable" | "failed" | "cancelled"
            )
            && result.reason.len() <= 1024
        {
            return Err(format!("Beat This {}: {}", result.status, result.reason));
        }
        let mut data = Self {
            source,
            request,
            result,
            code_revision: CODE_REVISION.into(),
            raw_revision: String::new(),
        };
        data.validate()?;
        data.raw_revision = codec::revision(ENCODING, &data)?;
        data.encoded_len()?;
        Ok(data)
    }

    pub(in crate::audio_engine) fn predictions(&self) -> &Predictions {
        self.result
            .predictions
            .as_ref()
            .expect("validated complete Beat This predictions")
    }

    pub(super) fn model(&self) -> Value {
        serde_json::to_value(&self.request.model).expect("typed model")
    }
    pub(super) fn frontend(&self) -> &str {
        &self.request.model.frontend_id
    }
    pub(super) fn code_revision(&self) -> &str {
        &self.code_revision
    }
    pub(in crate::audio_engine) fn decoder_identity(&self) -> &str {
        &self.source.decoder_identity
    }
    pub(in crate::audio_engine) fn source_descriptor(&self) -> &Value {
        &self.source.source_descriptor
    }
    pub(in crate::audio_engine) fn event_count(&self) -> usize {
        self.predictions().beat_seconds.len()
    }

    pub(in crate::audio_engine) fn observation_seconds(&self, index: usize) -> Option<f64> {
        self.predictions().beat_seconds.get(index).copied()
    }

    pub(in crate::audio_engine) fn capture_revision(&self) -> &str {
        &self.raw_revision
    }

    pub(in crate::audio_engine) fn observation_downbeat(&self, seconds: f64) -> bool {
        self.predictions()
            .downbeat_seconds
            .binary_search_by(|value| value.total_cmp(&seconds))
            .is_ok()
    }

    fn validate(&self) -> Result<(), String> {
        validate_source_descriptor(
            &self.source.source_descriptor,
            &self.source.decoder_identity,
        )?;
        let source = &self.source;
        let request = &self.request;
        let result = &self.result;
        if request.schema_version != 1
            || !codec::text(&request.identity.source_id)
            || request.identity.request_id == 0
            || request.identity.source_generation == 0
            || !codec::text(&request.pcm.path)
            || !std::path::Path::new(&request.pcm.path).is_absolute()
            || request.pcm.origin_seconds.to_bits() != 0
            || request.pcm.dtype != "float32-le"
            || request.pcm.channels != 1
            || request.pcm.sample_rate_hz != source.source_sample_rate_hz
            || request.pcm.frame_count != source.source_frame_count
            || source.source_zero_frame != 0
            || !codec::digest(&source.mono_sha256)
            || source.mono_rule != MONO_RULE
            || serde_json::to_value(&request.model).map_err(|e| e.to_string())? != pinned_model()
            || self.code_revision != CODE_REVISION
        {
            return Err("Beat This capture source, request or pinned model differs".into());
        }
        let logits = expected_logit_count(source.source_frame_count, source.source_sample_rate_hz)?;
        if (source.source_frame_count as u128) * 4 > 512 * 1024 * 1024 {
            return Err("Beat This staged PCM exceeds worker limit".into());
        }
        if result.schema_version != 1
            || result.encoding != WORKER_ENCODING
            || result.identity != request.identity
            || result.model.as_ref() != Some(&request.model)
            || !result.resources_released
            || result.status != "ready"
            || !codec::text(&result.reason)
            || result.input.sample_rate_hz != source.source_sample_rate_hz
            || result.input.frame_count != source.source_frame_count
            || result.input.source_zero_frame != 0
            || result.input.mono_sha256 != source.mono_sha256
            || result.input.dtype != "float32-le"
        {
            return Err(format!(
                "Beat This capture worker outcome unavailable or mismatched: {}",
                result.status
            ));
        }
        let p = result
            .predictions
            .as_ref()
            .ok_or("Beat This complete predictions missing")?;
        if p.encoding != "float64-le/base64"
            || p.beat_logits.len() != logits
            || p.downbeat_logits.len() != logits
        {
            return Err("Beat This complete logit extent differs".into());
        }
        let duration = source.source_frame_count as f64 / f64::from(source.source_sample_rate_hz);
        for positions in [&p.beat_seconds, &p.downbeat_seconds] {
            if positions.len() > MAX_RAW_EVENTS
                || positions
                    .iter()
                    .any(|v| !v.is_finite() || *v < 0.0 || *v >= duration)
                || positions.windows(2).any(|v| v[0] >= v[1])
            {
                return Err("Beat This complete source positions invalid".into());
            }
        }
        if p.beat_logits
            .iter()
            .chain(&p.downbeat_logits)
            .any(|v| !v.is_finite())
        {
            return Err("Beat This logits contain nonfinite values".into());
        }
        Ok(())
    }

    fn envelope(&self) -> Encoded<'_> {
        Encoded {
            schema_version: 2,
            encoding: ENCODING,
            raw_revision: &self.raw_revision,
            data: self,
        }
    }

    pub(super) fn encoded_len(&self) -> Result<usize, String> {
        codec::encoded_len(&self.envelope())
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        if !codec::digest(&self.raw_revision)
            || codec::revision(ENCODING, self)? != self.raw_revision
        {
            return Err("Beat This capture revision differs".into());
        }
        codec::encode(&self.envelope())
    }

    pub(super) fn embedded_value(&self) -> Result<Value, String> {
        self.validate()?;
        if codec::revision(ENCODING, self)? != self.raw_revision {
            return Err("Beat This capture revision differs".into());
        }
        self.encoded_len()?;
        serde_json::to_value(self.envelope()).map_err(|e| e.to_string())
    }

    pub(super) fn decode(input: &[u8]) -> Result<Self, String> {
        if input.is_empty() || input.len() > MAX_RAW_BYTES {
            return Err("Beat This raw byte limit".into());
        }
        let mut envelope: Envelope = serde_json::from_slice(input).map_err(|e| e.to_string())?;
        envelope.validate()?;
        if envelope.data.encode()? != input {
            return Err("Beat This raw encoding is not canonical".into());
        }
        Ok(envelope.data)
    }
}

impl Envelope {
    pub(in crate::audio_engine) fn validate(&mut self) -> Result<&BeatThisRawData, String> {
        if self.schema_version != 2 || self.encoding != ENCODING {
            return Err("Beat This raw envelope version invalid".into());
        }
        self.data.source.source_descriptor = canonical(&self.data.source.source_descriptor);
        self.data.raw_revision = self.raw_revision.clone();
        self.data.validate()?;
        if !codec::digest(&self.raw_revision)
            || codec::revision(ENCODING, &self.data)? != self.raw_revision
        {
            return Err("Beat This capture revision differs".into());
        }
        self.data.encoded_len()?;
        Ok(&self.data)
    }
}
