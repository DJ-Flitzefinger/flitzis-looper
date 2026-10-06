//! Lossless backend ownership with canonical identities and explicit timebase lineage.

use super::binding::{valid_digest, valid_text};
use super::{JobIdentity, PcmBinding, PcmBindingMetadata, TempoEvidenceError, f64_input_sha256};
use crate::canonical_digest::CanonicalDigest;
use crate::tempo_summary::{MAX_RAW_POSITIONS, RawTempoEvidence, SourceIdentity};
use crate::{AnalysisConfig, QmRawAnalysis};

const RAW_REVISION: &str = "source-bound-raw-v1";
const QM_BACKEND: &str = "qm-dsp-lossless-capture-v1";
const RUBATO_REVISION: &str = "rubato-fft-1.0-44100-delay-trim-tail-flush-v1";

/// An explicitly declared engineering bound, never supplied by a good fit.
#[derive(Debug, Clone, PartialEq)]
pub struct TimingBound {
    pub halfwidth_seconds: f64,
    pub provenance: String,
}

/// Explicit complete-input mapping from the verified loaded mono to QM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QmInputTransform {
    /// Exact float32-to-float64 promotion at the unchanged loaded rate.
    Identity,
    /// Standard complete 44100-Hz conversion; caller attests the actual execution.
    ///
    /// This adapter checks exact ceiling length and recorded source association.
    /// It cannot reproduce or independently prove the resampler's sample values.
    Rubato44100 {
        revision: String,
        provenance: String,
    },
}

/// Expected actual QM input bytes and complete source-to-analyzer lineage.
#[derive(Debug, Clone, PartialEq)]
pub struct QmInputDescriptor {
    pub job: JobIdentity,
    pub pcm_sha256: String,
    pub input_sha256: String,
    pub sample_rate_hz: u32,
    pub frame_count: u64,
    pub origin_seconds: f64,
    pub transform: QmInputTransform,
}

/// Complete selected Beat This model/configuration identity from its validated request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeatThisModelIdentity {
    pub sha256: String,
    pub frontend_id: String,
    pub environment_id: String,
    pub package_version: String,
    pub checkpoint: String,
    pub postprocessor: String,
    pub device: String,
    pub precision: String,
}

/// Metadata supplied by the retained validated worker request and echoed response.
///
/// The ephemeral PCM path is retained as provenance; its spelling is never a
/// content identity. `pcm_sha256` must match the checked complete loaded-mono bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct BeatThisRequestIdentity {
    pub job: JobIdentity,
    pub pcm_sha256: String,
    pub pcm_path: String,
    pub sample_rate_hz: u32,
    pub frame_count: u64,
    pub origin_seconds: f64,
    pub dtype: String,
    pub channels: u32,
    pub schema_version: u32,
    pub model: BeatThisModelIdentity,
}

/// Every original binary64 prediction array with expected and echoed request metadata.
///
/// Downbeats stay independent arrays: the existing wire contract does not promise
/// that they are beat-index subsets. Logits are uncalibrated model evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct BeatThisRawEvidence {
    pub expected_request: BeatThisRequestIdentity,
    pub response_job: JobIdentity,
    pub response_model: BeatThisModelIdentity,
    pub response_schema_version: u32,
    pub beat_seconds: Vec<f64>,
    pub downbeat_seconds: Vec<f64>,
    pub beat_logits: Vec<f64>,
    pub downbeat_logits: Vec<f64>,
}

/// Original backend capture retained alongside source-relative summary coordinates.
#[derive(Debug, Clone)]
pub enum BackendEvidence {
    Qm {
        raw: QmRawAnalysis,
        input: Box<QmInputDescriptor>,
    },
    BeatThis(Box<BeatThisRawEvidence>),
}

/// An owned source-bound raw result; fields are immutable after verification.
#[derive(Debug, Clone)]
pub struct BoundTempoEvidence {
    binding: PcmBindingMetadata,
    source: SourceIdentity,
    timing_bound: TimingBound,
    independent_origin_seconds: f64,
    backend: BackendEvidence,
    beat_seconds: Vec<f64>,
}

impl BoundTempoEvidence {
    /// Bind retained QM frames to verified loaded mono and the actual complete f64 input.
    ///
    /// Input/configuration/raw arrays are retained without cropping, sorting or
    /// binary32 widening. The raw object does not retain its original input bytes;
    /// the caller attests that this descriptor and input belong to that execution.
    pub fn from_qm(
        binding: &PcmBinding<'_>,
        raw: QmRawAnalysis,
        analyzer_input: &[f64],
        input: QmInputDescriptor,
        timing_bound: TimingBound,
        independent_origin_seconds: f64,
    ) -> Result<Self, TempoEvidenceError> {
        validate_bound(binding, &timing_bound, independent_origin_seconds)?;
        validate_qm_input(binding, &raw, analyzer_input, &input)?;
        let beat_seconds: Vec<f64> = raw.beat_seconds().collect();
        validate_positions(&beat_seconds, duration(binding.metadata()))?;
        validate_qm_configuration(raw.configuration())?;
        let mut configuration = CanonicalDigest::new("qm-configuration-v1");
        hash_qm_configuration(&mut configuration, raw.configuration());
        configuration.text(QM_BACKEND);
        hash_qm_input(&mut configuration, &input);
        let configuration_revision = configuration.finish();
        let mut revision = CanonicalDigest::new(RAW_REVISION);
        hash_binding(&mut revision, binding.metadata());
        revision.text(QM_BACKEND);
        hash_qm_input(&mut revision, &input);
        hash_qm_configuration(&mut revision, raw.configuration());
        revision.number(raw.odf_hop_samples() as u64);
        revision.float_array(raw.beat_frames());
        revision.number(raw.downbeat_raw_indices().len() as u64);
        for index in raw.downbeat_raw_indices() {
            revision.number(*index as u64);
        }
        let source = source_identity(
            binding.metadata(),
            QM_BACKEND.into(),
            configuration_revision,
            revision.finish(),
            timing_bound.halfwidth_seconds,
        );
        Ok(Self {
            binding: binding.metadata().clone(),
            source,
            timing_bound,
            independent_origin_seconds,
            backend: BackendEvidence::Qm {
                raw,
                input: Box::new(input),
            },
            beat_seconds,
        })
    }

    /// Bind all lossless Beat This evidence to the exact retained request and mono bytes.
    ///
    /// This is a typed offline adapter for the existing validated publication
    /// reader. It does not run inference, reopen scratch PCM or adopt analysis.
    pub fn from_beat_this(
        binding: &PcmBinding<'_>,
        raw: BeatThisRawEvidence,
        timing_bound: TimingBound,
        independent_origin_seconds: f64,
    ) -> Result<Self, TempoEvidenceError> {
        validate_bound(binding, &timing_bound, independent_origin_seconds)?;
        validate_beat_this(binding, &raw)?;
        let mut configuration = CanonicalDigest::new("beat-this-configuration-v1");
        hash_beat_model(&mut configuration, &raw.expected_request.model);
        let configuration_revision = configuration.finish();
        let mut revision = CanonicalDigest::new(RAW_REVISION);
        hash_binding(&mut revision, binding.metadata());
        revision.text("beat-this-1.1.0-lossless-v1");
        hash_beat_request(&mut revision, &raw.expected_request);
        hash_job(&mut revision, &raw.response_job);
        hash_beat_model(&mut revision, &raw.response_model);
        revision.number(u64::from(raw.response_schema_version));
        revision.float_array(&raw.beat_seconds);
        revision.float_array(&raw.downbeat_seconds);
        revision.float_array(&raw.beat_logits);
        revision.float_array(&raw.downbeat_logits);
        let source = source_identity(
            binding.metadata(),
            "beat-this-1.1.0-lossless-v1".into(),
            configuration_revision,
            revision.finish(),
            timing_bound.halfwidth_seconds,
        );
        Ok(Self {
            binding: binding.metadata().clone(),
            source,
            timing_bound,
            independent_origin_seconds,
            beat_seconds: raw.beat_seconds.clone(),
            backend: BackendEvidence::BeatThis(Box::new(raw)),
        })
    }

    /// Borrow the immutable complete source binding and caller original provenance.
    pub fn binding(&self) -> &PcmBindingMetadata {
        &self.binding
    }

    /// Borrow source identity and the canonical complete backend raw revision.
    pub fn source_identity(&self) -> &SourceIdentity {
        &self.source
    }

    /// Borrow the explicitly supplied uncertainty declaration and its provenance.
    pub fn timing_bound(&self) -> &TimingBound {
        &self.timing_bound
    }

    /// Borrow every complete source-relative binary64 beat timestamp.
    pub fn beat_seconds(&self) -> &[f64] {
        &self.beat_seconds
    }

    /// Borrow original raw frames/indices/configuration or all worker predictions.
    pub fn backend(&self) -> &BackendEvidence {
        &self.backend
    }

    /// Borrow the G2a assessment input without discarding the retained backend evidence.
    pub fn raw_evidence(&self) -> RawTempoEvidence<'_> {
        RawTempoEvidence {
            source: &self.source,
            beat_seconds: &self.beat_seconds,
            independent_origin_seconds: self.independent_origin_seconds,
        }
    }

    /// Bind every retained backend field, including caller timing/origin assertions.
    pub(crate) fn hash_canonical(&self, hash: &mut CanonicalDigest) {
        hash_binding(hash, &self.binding);
        hash.float(self.independent_origin_seconds);
        hash.float(self.timing_bound.halfwidth_seconds);
        hash.text(&self.timing_bound.provenance);
        hash.float_array(&self.beat_seconds);
        match &self.backend {
            BackendEvidence::Qm { raw, input } => {
                hash.text(QM_BACKEND);
                hash_qm_input(hash, input);
                hash_qm_configuration(hash, raw.configuration());
                hash.number(u64::from(raw.input_sample_rate_hz()));
                hash.number(raw.input_frame_count() as u64);
                hash.number(raw.odf_hop_samples() as u64);
                hash.float_array(raw.beat_frames());
                hash.sequence(raw.downbeat_raw_indices(), |hash, index| {
                    hash.number(*index as u64);
                });
            }
            BackendEvidence::BeatThis(raw) => {
                hash.text("beat-this-1.1.0-lossless-v1");
                hash_beat_request(hash, &raw.expected_request);
                hash_job(hash, &raw.response_job);
                hash_beat_model(hash, &raw.response_model);
                hash.number(u64::from(raw.response_schema_version));
                hash.float_array(&raw.beat_seconds);
                hash.float_array(&raw.downbeat_seconds);
                hash.float_array(&raw.beat_logits);
                hash.float_array(&raw.downbeat_logits);
            }
        }
    }
}

fn duration(metadata: &PcmBindingMetadata) -> f64 {
    metadata.frame_count as f64 / f64::from(metadata.sample_rate_hz)
}

fn validate_bound(
    binding: &PcmBinding<'_>,
    timing: &TimingBound,
    origin: f64,
) -> Result<(), TempoEvidenceError> {
    if !timing.halfwidth_seconds.is_finite()
        || !(0.0..=duration(binding.metadata())).contains(&timing.halfwidth_seconds)
        || !valid_text(&timing.provenance)
        || !origin.is_finite()
    {
        return Err(TempoEvidenceError(
            "invalid explicit timing bound or grid origin",
        ));
    }
    Ok(())
}

fn validate_qm_input(
    binding: &PcmBinding<'_>,
    raw: &QmRawAnalysis,
    samples: &[f64],
    input: &QmInputDescriptor,
) -> Result<(), TempoEvidenceError> {
    let metadata = binding.metadata();
    if input.job != metadata.job
        || input.pcm_sha256 != metadata.pcm_sha256
        || input.origin_seconds.to_bits() != 0.0_f64.to_bits()
        || input.frame_count != samples.len() as u64
        || input.frame_count != raw.input_frame_count() as u64
        || input.sample_rate_hz != raw.input_sample_rate_hz()
        || !valid_digest(&input.input_sha256)
        || samples.iter().any(|sample| !sample.is_finite())
        || raw.beat_frames().len() > MAX_RAW_POSITIONS
        || raw.downbeat_raw_indices().len() > MAX_RAW_POSITIONS
    {
        return Err(TempoEvidenceError("QM input/source binding mismatch"));
    }
    match &input.transform {
        QmInputTransform::Identity => {
            if input.sample_rate_hz != metadata.sample_rate_hz
                || input.frame_count != metadata.frame_count
                || samples
                    .iter()
                    .zip(binding.samples())
                    .any(|(actual, source)| actual.to_bits() != f64::from(*source).to_bits())
            {
                return Err(TempoEvidenceError(
                    "QM identity input differs from loaded mono",
                ));
            }
        }
        QmInputTransform::Rubato44100 {
            revision,
            provenance,
        } => {
            let frames = (u128::from(metadata.frame_count) * 44_100)
                .div_ceil(u128::from(metadata.sample_rate_hz));
            if input.sample_rate_hz != 44_100
                || input.frame_count as u128 != frames
                || metadata.sample_rate_hz == 44_100
                || revision != RUBATO_REVISION
                || !valid_text(provenance)
            {
                return Err(TempoEvidenceError("invalid complete QM resampling lineage"));
            }
        }
    }
    if f64_input_sha256(samples) != input.input_sha256 {
        return Err(TempoEvidenceError("QM analyzer input digest mismatch"));
    }
    Ok(())
}

fn validate_qm_configuration(config: &AnalysisConfig) -> Result<(), TempoEvidenceError> {
    if [
        config.step_secs,
        config.max_bin_hz,
        config.input_tempo,
        config.alpha,
        config.tightness,
        config.viterbi_sigma,
    ]
    .iter()
    .any(|value| !value.is_finite())
    {
        return Err(TempoEvidenceError("nonfinite requested QM configuration"));
    }
    Ok(())
}

fn validate_beat_this(
    binding: &PcmBinding<'_>,
    raw: &BeatThisRawEvidence,
) -> Result<(), TempoEvidenceError> {
    let metadata = binding.metadata();
    let request = &raw.expected_request;
    if request.job != metadata.job
        || request.pcm_sha256 != metadata.pcm_sha256
        || request.sample_rate_hz != metadata.sample_rate_hz
        || request.frame_count != metadata.frame_count
        || request.origin_seconds.to_bits() != 0.0_f64.to_bits()
        || request.dtype != "float32-le"
        || request.channels != 1
        || request.schema_version != 1
        || !valid_text(&request.pcm_path)
        || raw.response_job != request.job
        || raw.response_model != request.model
        || raw.response_schema_version != 1
    {
        return Err(TempoEvidenceError(
            "Beat This request/source binding mismatch",
        ));
    }
    let model = &request.model;
    if !valid_digest(&model.sha256)
        || !valid_text(&model.frontend_id)
        || !valid_text(&model.environment_id)
        || model.package_version != "1.1.0"
        || model.checkpoint != "final0"
        || model.postprocessor != "minimal"
        || model.device != "cpu"
        || model.precision != "float32"
    {
        return Err(TempoEvidenceError(
            "invalid selected Beat This model identity",
        ));
    }
    validate_positions(&raw.beat_seconds, duration(metadata))?;
    validate_positions(&raw.downbeat_seconds, duration(metadata))?;
    if raw.beat_logits.len() != raw.downbeat_logits.len()
        || raw.beat_logits.len() > MAX_RAW_POSITIONS
        || raw
            .beat_logits
            .iter()
            .chain(&raw.downbeat_logits)
            .any(|value| !value.is_finite())
    {
        return Err(TempoEvidenceError("invalid complete Beat This logits"));
    }
    Ok(())
}

fn validate_positions(times: &[f64], duration: f64) -> Result<(), TempoEvidenceError> {
    if times.len() > MAX_RAW_POSITIONS {
        return Err(TempoEvidenceError("complete raw position limit exceeded"));
    }
    let mut previous = -1.0;
    for time in times {
        if !time.is_finite() || *time < 0.0 || *time >= duration || *time <= previous {
            return Err(TempoEvidenceError(
                "invalid complete source-relative raw positions",
            ));
        }
        previous = *time;
    }
    Ok(())
}

fn source_identity(
    binding: &PcmBindingMetadata,
    backend_revision: String,
    configuration_revision: String,
    raw_revision: String,
    timing_error_halfwidth_seconds: f64,
) -> SourceIdentity {
    SourceIdentity {
        source_sha256: binding.source_sha256.clone(),
        pcm_sha256: binding.pcm_sha256.clone(),
        loaded_sample_rate_hz: binding.sample_rate_hz,
        loaded_frame_count: binding.frame_count,
        backend_revision,
        configuration_revision,
        raw_revision,
        timing_error_halfwidth_seconds,
    }
}

fn hash_job(hash: &mut CanonicalDigest, job: &JobIdentity) {
    hash.number(job.pad_id);
    hash.number(job.request_id);
    hash.text(&job.source_id);
    hash.number(job.source_generation);
}

fn hash_binding(hash: &mut CanonicalDigest, binding: &PcmBindingMetadata) {
    hash_job(hash, &binding.job);
    hash.text(&binding.source_sha256);
    hash.text(&binding.source_provenance);
    hash.text(&binding.pcm_sha256);
    hash.number(u64::from(binding.sample_rate_hz));
    hash.number(binding.frame_count);
    hash.float(binding.origin_seconds);
    hash.text(&binding.mono_revision);
}

fn hash_qm_input(hash: &mut CanonicalDigest, input: &QmInputDescriptor) {
    hash_job(hash, &input.job);
    hash.text(&input.pcm_sha256);
    hash.text(&input.input_sha256);
    hash.number(u64::from(input.sample_rate_hz));
    hash.number(input.frame_count);
    hash.float(input.origin_seconds);
    match &input.transform {
        QmInputTransform::Identity => hash.text("identity-f32-to-f64-v1"),
        QmInputTransform::Rubato44100 {
            revision,
            provenance,
        } => {
            hash.text("standard-rubato-44100-v1");
            hash.text(revision);
            hash.text(provenance);
        }
    }
}

fn hash_qm_configuration(hash: &mut CanonicalDigest, config: &AnalysisConfig) {
    hash.float(config.step_secs);
    hash.float(config.max_bin_hz);
    hash.float(config.input_tempo);
    hash.float(config.alpha);
    hash.float(config.tightness);
    hash.float(config.viterbi_sigma);
    hash.number(config.window_length as u64);
    hash.number(config.hop_size as u64);
}

fn hash_beat_model(hash: &mut CanonicalDigest, model: &BeatThisModelIdentity) {
    hash.text(&model.sha256);
    hash.text(&model.frontend_id);
    hash.text(&model.environment_id);
    hash.text(&model.package_version);
    hash.text(&model.checkpoint);
    hash.text(&model.postprocessor);
    hash.text(&model.device);
    hash.text(&model.precision);
}

fn hash_beat_request(hash: &mut CanonicalDigest, request: &BeatThisRequestIdentity) {
    hash_job(hash, &request.job);
    hash.text(&request.pcm_sha256);
    hash.text(&request.pcm_path);
    hash.number(u64::from(request.sample_rate_hz));
    hash.number(request.frame_count);
    hash.float(request.origin_seconds);
    hash.text(&request.dtype);
    hash.number(u64::from(request.channels));
    hash.number(u64::from(request.schema_version));
    hash_beat_model(hash, &request.model);
}
