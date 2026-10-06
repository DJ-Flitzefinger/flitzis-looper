//! Borrowed, complete mono PCM with verified content and explicit source lineage.

use sha2::{Digest, Sha256};
use std::{error::Error, fmt};

/// Existing immutable loaded-source channel-mean rule.
pub const MONO_REVISION: &str = "arithmetic-channel-mean-f64-v1";
const MAX_PCM_BYTES: usize = 512 * 1024 * 1024;
const HASH_CHUNK_SAMPLES: usize = 4096;

/// The existing engine-local loaded-source and request association.
///
/// These tokens are independent of the original and PCM content digests. They
/// cannot establish persisted source identity or musical correctness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobIdentity {
    pub pad_id: u64,
    pub request_id: u64,
    pub source_id: String,
    pub source_generation: u64,
}

/// Caller-established original identity and expected complete loaded-mono extent.
///
/// `source_provenance` must explain how the original digest was established and
/// associated with this loaded source. The constructor cannot inspect the file
/// or certify that assertion. `pcm_sha256` is checked against every actual mono
/// sample, encoded as unmodified IEEE-754 float32 little-endian bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct PcmBindingMetadata {
    pub job: JobIdentity,
    pub source_sha256: String,
    pub source_provenance: String,
    pub pcm_sha256: String,
    pub sample_rate_hz: u32,
    pub frame_count: u64,
    pub origin_seconds: f64,
    pub mono_revision: String,
}

/// A verified borrowed immutable PCM input; construction never copies its samples.
#[derive(Debug)]
pub struct PcmBinding<'a> {
    samples: &'a [f32],
    metadata: PcmBindingMetadata,
}

impl<'a> PcmBinding<'a> {
    /// Check complete dimensions, source/request metadata and the actual PCM digest.
    ///
    /// Original-file provenance remains the caller's explicit assertion. No disk
    /// access, source-origin correction or analyzer execution occurs here.
    pub fn verify(
        samples: &'a [f32],
        metadata: PcmBindingMetadata,
    ) -> Result<Self, TempoEvidenceError> {
        validate_job(&metadata.job)?;
        if !valid_digest(&metadata.source_sha256)
            || !valid_digest(&metadata.pcm_sha256)
            || !valid_text(&metadata.source_provenance)
            || metadata.mono_revision != MONO_REVISION
            || !(8_000..=384_000).contains(&metadata.sample_rate_hz)
            || metadata.frame_count != samples.len() as u64
            || metadata.origin_seconds.to_bits() != 0.0_f64.to_bits()
        {
            return Err(TempoEvidenceError("invalid PCM binding metadata"));
        }
        validate_samples(samples)?;
        if f32_pcm_sha256(samples) != metadata.pcm_sha256 {
            return Err(TempoEvidenceError("PCM content digest mismatch"));
        }
        Ok(Self { samples, metadata })
    }

    /// Borrow the exact complete loaded-rate mono samples whose bytes were checked.
    pub fn samples(&self) -> &'a [f32] {
        self.samples
    }

    /// Borrow immutable source identity, original provenance and input dimensions.
    pub fn metadata(&self) -> &PcmBindingMetadata {
        &self.metadata
    }
}

/// SHA-256 of complete unmodified float32 little-endian samples, including signed zero.
pub fn f32_pcm_sha256(samples: &[f32]) -> String {
    let mut digest = Sha256::new();
    let mut bytes = [0_u8; HASH_CHUNK_SAMPLES * size_of::<f32>()];
    for chunk in samples.chunks(HASH_CHUNK_SAMPLES) {
        for (sample, destination) in chunk.iter().zip(bytes.chunks_exact_mut(4)) {
            destination.copy_from_slice(&sample.to_le_bytes());
        }
        digest.update(&bytes[..size_of_val(chunk)]);
    }
    format!("{:x}", digest.finalize())
}

/// SHA-256 of complete unmodified float64 little-endian analyzer input samples.
pub fn f64_input_sha256(samples: &[f64]) -> String {
    let mut digest = Sha256::new();
    let mut bytes = [0_u8; HASH_CHUNK_SAMPLES * size_of::<f64>()];
    for chunk in samples.chunks(HASH_CHUNK_SAMPLES) {
        for (sample, destination) in chunk.iter().zip(bytes.chunks_exact_mut(8)) {
            destination.copy_from_slice(&sample.to_le_bytes());
        }
        digest.update(&bytes[..size_of_val(chunk)]);
    }
    format!("{:x}", digest.finalize())
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096
}

pub(super) fn validate_job(job: &JobIdentity) -> Result<(), TempoEvidenceError> {
    if job.request_id == 0 || job.source_generation == 0 || !valid_text(&job.source_id) {
        return Err(TempoEvidenceError("invalid source/request identity"));
    }
    Ok(())
}

fn validate_samples(samples: &[f32]) -> Result<(), TempoEvidenceError> {
    if samples.is_empty()
        || samples.len() > MAX_PCM_BYTES / size_of::<f32>()
        || samples.iter().any(|sample| !sample.is_finite())
    {
        return Err(TempoEvidenceError("invalid complete loaded mono PCM"));
    }
    Ok(())
}

/// Invalid identity, content or timebase; separate from unsupported musical evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempoEvidenceError(pub(super) &'static str);

impl fmt::Display for TempoEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for TempoEvidenceError {}
