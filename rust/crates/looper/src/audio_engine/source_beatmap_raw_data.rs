//! Complete, unreviewed legacy QM capture. All work is on the existing cold worker.
//!
//! This record has no quarter-note, meter, coverage, timing-bound or map authority.
//! Detector coordinates remain original binary64 values, including positions
//! outside the physical source; the control inspector classifies those positions.

use super::analysis_pcm::{MONO_RULE, fft, resample_mono_cancellable};
use super::cold_store::{canonical, descriptor_digest};
use super::complete_context::{CompleteSourceReader, READER_SCRATCH_BYTES};
use flitzis_looper_analysis::{AnalysisConfig, analyze_bpm_raw, default_bpm_working_bytes};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::{self, Write};

pub(super) const MAX_RAW_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_RAW_EVENTS: usize = 250_000;
const MAX_JOB_BYTES: usize = 512 * 1024 * 1024;
const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;
const METADATA_WORK_BYTES: usize = MAX_DESCRIPTOR_BYTES * 128;
const HASH_SCRATCH_BYTES: usize = 4096 * 8;
const SCHEMA_VERSION: u32 = 1;
const ENCODING: &str = "source-beatmap-unreviewed-qm-raw-v1";
const ANALYZER_RATE_HZ: u32 = 44_100;
const DETECTOR_ID: &str = "legacy-qm-dsp";
const CODE_REVISION: &str = "native-qm-complete-raw-integer-hop-v1";
const CONVERSION_REVISION: &str =
    "rubato-fft-1.0-44100-delay-trim-tail-flush-v1+exact-f32-to-f64-v1";

/// Every supplied detector setting, including settings unused by legacy QM.
/// Their persisted representation uses binary64 bits rather than decimal text.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawConfiguration {
    #[serde(with = "float_bits")]
    pub(super) step_secs: f64,
    #[serde(with = "float_bits")]
    pub(super) max_bin_hz: f64,
    #[serde(with = "float_bits")]
    pub(super) input_tempo: f64,
    #[serde(with = "float_bits")]
    pub(super) alpha: f64,
    #[serde(with = "float_bits")]
    pub(super) tightness: f64,
    #[serde(with = "float_bits")]
    pub(super) viterbi_sigma: f64,
    pub(super) window_length: usize,
    pub(super) hop_size: usize,
}

impl RawConfiguration {
    fn capture(configuration: &AnalysisConfig) -> Self {
        Self {
            step_secs: configuration.step_secs,
            max_bin_hz: configuration.max_bin_hz,
            input_tempo: configuration.input_tempo,
            alpha: configuration.alpha,
            tightness: configuration.tightness,
            viterbi_sigma: configuration.viterbi_sigma,
            window_length: configuration.window_length,
            hop_size: configuration.hop_size,
        }
    }

    fn matches_default(&self) -> bool {
        let default = AnalysisConfig::default();
        self.step_secs.to_bits() == default.step_secs.to_bits()
            && self.max_bin_hz.to_bits() == default.max_bin_hz.to_bits()
            && self.input_tempo.to_bits() == default.input_tempo.to_bits()
            && self.alpha.to_bits() == default.alpha.to_bits()
            && self.tightness.to_bits() == default.tightness.to_bits()
            && self.viterbi_sigma.to_bits() == default.viterbi_sigma.to_bits()
            && self.window_length == default.window_length
            && self.hop_size == default.hop_size
    }

    pub(super) fn value(&self) -> Value {
        json!({"step_secs":self.step_secs,"max_bin_hz":self.max_bin_hz,
            "input_tempo":self.input_tempo,"alpha":self.alpha,"tightness":self.tightness,
            "viterbi_sigma":self.viterbi_sigma,"window_length":self.window_length,
            "hop_size":self.hop_size})
    }
}

/// Immutable after construction; only the private Native candidate owns this data.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawData {
    pub(super) decoder_identity: String,
    #[serde(deserialize_with = "bounded_descriptor")]
    pub(super) source_descriptor: Value,
    pub(super) source_sample_rate_hz: u32,
    pub(super) source_frame_count: usize,
    pub(super) source_zero_frame: u64,
    pub(super) mono_sha256: String,
    pub(super) mono_rule: String,
    pub(super) analyzer_sha256: String,
    pub(super) analyzer_sample_rate_hz: u32,
    pub(super) analyzer_frame_count: usize,
    pub(super) conversion_revision: String,
    pub(super) detector_id: String,
    pub(super) code_revision: String,
    pub(super) odf_hop_samples: usize,
    pub(super) configuration: RawConfiguration,
    #[serde(skip)]
    pub(super) raw_revision: String,
    #[serde(with = "frame_bits")]
    pub(super) beat_frames: Vec<f64>,
    #[serde(deserialize_with = "bounded_indices")]
    pub(super) downbeat_indices: Vec<usize>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope {
    schema_version: u32,
    encoding: String,
    raw_revision: String,
    data: RawData,
}

impl Envelope {
    /// Nested captures follow the map's object order, while retaining their
    /// original typed revision and every detector bit.
    pub(super) fn validate(&mut self) -> Result<&RawData, String> {
        if self.schema_version != SCHEMA_VERSION || self.encoding != ENCODING {
            return Err("raw acquisition encoding version invalid".into());
        }
        self.data.raw_revision = self.raw_revision.clone();
        self.data.source_descriptor = canonical(&self.data.source_descriptor);
        self.data.validate()?;
        if !digest_text(&self.raw_revision) || self.data.revision()? != self.raw_revision {
            return Err("raw acquisition revision differs".into());
        }
        self.data.encoded_len()?;
        Ok(&self.data)
    }
}

#[derive(Serialize)]
struct Encoded<'a> {
    schema_version: u32,
    encoding: &'static str,
    raw_revision: &'a str,
    data: &'a RawData,
}

struct Budget {
    converted_frames: usize,
    fft_workspace_bytes: usize,
    peak_bytes: usize,
}

fn sum(values: &[usize]) -> Result<usize, String> {
    values
        .iter()
        .try_fold(0_usize, |total, value| total.checked_add(*value))
        .ok_or_else(|| "raw acquisition byte extent overflow".into())
}

fn bytes(count: usize, width: usize) -> Result<usize, String> {
    count
        .checked_mul(width)
        .ok_or_else(|| "raw acquisition byte extent overflow".into())
}

fn admit(peak: usize) -> Result<(), String> {
    if peak > MAX_JOB_BYTES {
        Err("raw acquisition exceeds 512-MiB staged job peak".into())
    } else {
        Ok(())
    }
}

fn budget(frames: usize, rate: u32, held: usize) -> Result<Budget, String> {
    if frames == 0 || !(8_000..=384_000).contains(&rate) {
        return Err("raw acquisition source rate or extent is invalid".into());
    }
    let converted_frames = (frames as u128)
        .checked_mul(u128::from(ANALYZER_RATE_HZ))
        .ok_or("raw acquisition converted extent overflow")?
        .div_ceil(u128::from(rate));
    let converted_frames = usize::try_from(converted_frames)
        .map_err(|_| "raw acquisition converted extent overflow")?;
    let mono = bytes(frames, 4)?;
    let analyzer = bytes(converted_frames, 8)?;
    let base = sum(&[held, METADATA_WORK_BYTES, HASH_SCRATCH_BYTES])?;
    // visit_region's fixed storage, stream_mono's own buffer and the original
    // verification's chunk are charged together, even though their peaks differ.
    let scratch = sum(&[READER_SCRATCH_BYTES, 16 * 1024, 64 * 1024])?;
    let scan = sum(&[base, scratch, mono])?;
    let (conversion, widening, fft_workspace_bytes) = if rate == ANALYZER_RATE_HZ {
        (sum(&[base, mono])?, sum(&[base, mono, analyzer])?, 0)
    } else {
        let (input, output) = fft::fft_dimensions(rate, ANALYZER_RATE_HZ);
        // The same pinned FFT dimensional allowance as default_analysis_peak:
        // input/output scratch/plans, with full coprime-rate units accounted for.
        let workspace = bytes(sum(&[input, output])?, 64 * 4)?;
        let output_capacity = bytes(sum(&[converted_frames, bytes(output, 2)?])?, 4)?;
        (
            sum(&[base, mono, output_capacity, workspace])?,
            sum(&[base, output_capacity, analyzer])?,
            workspace,
        )
    };
    let qm = default_bpm_working_bytes(converted_frames)
        .ok_or("raw acquisition QM working extent overflow")?;
    let inference = sum(&[base, analyzer, qm])?;
    // Detector Vec capacities, the retained copies and a sealed reparse all stay
    // charged. Three complete byte buffers cover encoding, sealed read and its
    // canonical comparison. No whole serde_json::Value copy of arrays is built.
    let raw_arrays = bytes(MAX_RAW_EVENTS, 2 * (8 + size_of::<usize>()))?;
    let serialization = sum(&[base, bytes(raw_arrays, 3)?, bytes(MAX_RAW_BYTES, 3)?])?;
    let peak_bytes = scan
        .max(conversion)
        .max(widening)
        .max(inference)
        .max(serialization);
    admit(peak_bytes)?;
    Ok(Budget {
        converted_frames,
        fft_workspace_bytes,
        peak_bytes,
    })
}

/// Arithmetic-only hook for focused overflow, coprime FFT and finite-view checks.
#[cfg(test)]
pub(super) fn acquisition_peak_bytes(
    frames: usize,
    rate: u32,
    held: usize,
) -> Result<usize, String> {
    Ok(budget(frames, rate, held)?.peak_bytes)
}

fn cancellation(cancelled: &impl Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("raw acquisition cancelled".into())
    } else {
        Ok(())
    }
}

struct MonoCollector {
    samples: Vec<f32>,
    expected: usize,
    digest: Sha256,
}

impl Write for MonoCollector {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let count = input.len() / 4;
        if !input.len().is_multiple_of(4)
            || self
                .samples
                .len()
                .checked_add(count)
                .is_none_or(|n| n > self.expected)
        {
            return Err(io::Error::other("raw acquisition mono extent changed"));
        }
        // Exact reservation happened before the first complete-source read.
        self.digest.update(input);
        for value in input.chunks_exact(4) {
            let value = f32::from_le_bytes(value.try_into().expect("complete f32 bytes"));
            if !value.is_finite() {
                return Err(io::Error::other("raw acquisition nonfinite mono"));
            }
            self.samples.push(value);
        }
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl RawData {
    pub(super) fn acquire(
        reader: &CompleteSourceReader,
        expected_generation: u64,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self, String> {
        cancellation(cancelled)?;
        reader.require_map_custody()?;
        let guard = reader
            .guard
            .as_ref()
            .ok_or("raw acquisition source owner unavailable")?;
        if expected_generation == 0 || guard.generation != expected_generation {
            return Err("raw acquisition source generation is stale".into());
        }
        let source_sample_rate_hz = guard.rate;
        let source_frame_count = reader.reference.frame_count();
        let held = reader.held_bytes()?;
        let plan = budget(source_frame_count, source_sample_rate_hz, held)?;
        admit(plan.peak_bytes)?;
        let (source_descriptor, decoder_identity) =
            reader.raw_source_descriptor(expected_generation, cancelled)?;
        validate_source_descriptor(&source_descriptor, &decoder_identity)?;
        let source_descriptor = canonical(&source_descriptor);
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(source_frame_count)
            .map_err(|error| error.to_string())?;
        admit(sum(&[
            held,
            METADATA_WORK_BYTES,
            HASH_SCRATCH_BYTES,
            READER_SCRATCH_BYTES,
            16 * 1024,
            bytes(samples.capacity(), 4)?,
        ])?)?;
        let mut collector = MonoCollector {
            samples,
            expected: source_frame_count,
            digest: Sha256::new(),
        };
        reader
            .stream_mono(&mut collector, cancelled)
            .map_err(|error| error.to_string())?;
        cancellation(cancelled)?;
        if collector.samples.len() != source_frame_count {
            return Err("raw acquisition complete mono extent differs".into());
        }
        let mono_sha256 = format!("{:x}", collector.digest.finalize());
        let resample_limit = MAX_JOB_BYTES
            .checked_sub(sum(&[
                held,
                METADATA_WORK_BYTES,
                HASH_SCRATCH_BYTES,
                plan.fft_workspace_bytes,
            ])?)
            .ok_or("raw acquisition resampler budget exhausted")?;
        let converted = resample_mono_cancellable(
            collector.samples,
            source_sample_rate_hz,
            ANALYZER_RATE_HZ,
            resample_limit,
            cancelled,
        )
        .map_err(|error| error.to_string())?;
        if converted.len() != plan.converted_frames {
            return Err("raw acquisition converted complete extent differs".into());
        }
        admit(sum(&[
            held,
            METADATA_WORK_BYTES,
            HASH_SCRATCH_BYTES,
            bytes(converted.capacity(), 4)?,
            bytes(converted.len(), 8)?,
        ])?)?;
        let mut analyzer = Vec::new();
        analyzer
            .try_reserve_exact(converted.len())
            .map_err(|error| error.to_string())?;
        admit(sum(&[
            held,
            METADATA_WORK_BYTES,
            HASH_SCRATCH_BYTES,
            bytes(converted.capacity(), 4)?,
            bytes(analyzer.capacity(), 8)?,
        ])?)?;
        let mut hash = Sha256::new();
        let mut hash_bytes = [0_u8; HASH_SCRATCH_BYTES];
        for chunk in converted.chunks(4096) {
            cancellation(cancelled)?;
            for (sample, destination) in chunk.iter().zip(hash_bytes.chunks_exact_mut(8)) {
                if !sample.is_finite() {
                    return Err("raw acquisition nonfinite analyzer input".into());
                }
                let promoted = f64::from(*sample);
                analyzer.push(promoted);
                destination.copy_from_slice(&promoted.to_le_bytes());
            }
            hash.update(&hash_bytes[..chunk.len() * 8]);
        }
        let analyzer_sha256 = format!("{:x}", hash.finalize());
        drop(converted);
        cancellation(cancelled)?;
        let configuration = AnalysisConfig::default();
        admit(sum(&[
            held,
            METADATA_WORK_BYTES,
            HASH_SCRATCH_BYTES,
            bytes(analyzer.capacity(), 8)?,
            default_bpm_working_bytes(analyzer.len())
                .ok_or("raw acquisition QM working extent overflow")?,
        ])?)?;
        // QM is currently synchronous. The source and lane remain held until it
        // really returns, even if cancellation becomes true during this call.
        let result = analyze_bpm_raw(&analyzer, ANALYZER_RATE_HZ, &configuration);
        drop(analyzer);
        cancellation(cancelled)?;
        let raw = result?;
        if raw.beat_frames().len() > MAX_RAW_EVENTS
            || raw.downbeat_raw_indices().len() > MAX_RAW_EVENTS
        {
            return Err("raw acquisition detector array limit exceeded".into());
        }
        let mut beat_frames = Vec::new();
        beat_frames
            .try_reserve_exact(raw.beat_frames().len())
            .map_err(|error| error.to_string())?;
        beat_frames.extend_from_slice(raw.beat_frames());
        let mut downbeat_indices = Vec::new();
        downbeat_indices
            .try_reserve_exact(raw.downbeat_raw_indices().len())
            .map_err(|error| error.to_string())?;
        downbeat_indices.extend_from_slice(raw.downbeat_raw_indices());
        let mut captured = Self {
            decoder_identity,
            source_descriptor,
            source_sample_rate_hz,
            source_frame_count,
            source_zero_frame: 0,
            mono_sha256,
            mono_rule: MONO_RULE.into(),
            analyzer_sha256,
            analyzer_sample_rate_hz: raw.input_sample_rate_hz(),
            analyzer_frame_count: raw.input_frame_count(),
            conversion_revision: CONVERSION_REVISION.into(),
            detector_id: DETECTOR_ID.into(),
            code_revision: CODE_REVISION.into(),
            odf_hop_samples: raw.odf_hop_samples(),
            configuration: RawConfiguration::capture(raw.configuration()),
            raw_revision: String::new(),
            beat_frames,
            downbeat_indices,
        };
        drop(raw);
        captured.validate()?;
        captured.raw_revision = captured.revision()?;
        reader.require_map_custody()?;
        if reader
            .guard
            .as_ref()
            .is_none_or(|guard| guard.generation != expected_generation)
        {
            return Err("raw acquisition source generation is stale".into());
        }
        cancellation(cancelled)?;
        Ok(captured)
    }

    pub(super) fn source_seconds(&self, frame: f64) -> f64 {
        frame * (self.odf_hop_samples as f64 / f64::from(self.analyzer_sample_rate_hz))
    }
    /// Includes the exclusive physical end, which cannot address a source frame.
    pub(super) fn outside_source_event_count(&self) -> usize {
        let end = self.source_frame_count as f64 / f64::from(self.source_sample_rate_hz);
        self.beat_frames.len()
            - self
                .beat_frames
                .partition_point(|frame| self.source_seconds(*frame) < end)
    }

    fn validate(&self) -> Result<(), String> {
        validate_source_descriptor(&self.source_descriptor, &self.decoder_identity)?;
        if !(8_000..=384_000).contains(&self.source_sample_rate_hz)
            || self.source_frame_count == 0
            || self.source_zero_frame != 0
            || !digest_text(&self.mono_sha256)
            || !digest_text(&self.analyzer_sha256)
            || self.mono_rule != MONO_RULE
            || self.conversion_revision != CONVERSION_REVISION
            || self.detector_id != DETECTOR_ID
            || self.code_revision != CODE_REVISION
            || self.analyzer_sample_rate_hz != ANALYZER_RATE_HZ
            || !self.configuration.matches_default()
            || self.odf_hop_samples
                != (f64::from(ANALYZER_RATE_HZ) * self.configuration.step_secs) as usize
            || self.odf_hop_samples == 0
        {
            return Err("invalid raw acquisition provenance".into());
        }
        let converted = (self.source_frame_count as u128 * u128::from(ANALYZER_RATE_HZ))
            .div_ceil(u128::from(self.source_sample_rate_hz));
        if converted != self.analyzer_frame_count as u128
            || self.beat_frames.len() > MAX_RAW_EVENTS
            || self.downbeat_indices.len() > MAX_RAW_EVENTS
            || self.downbeat_indices.len() > self.beat_frames.len()
            || self.beat_frames.iter().any(|frame| {
                !frame.is_finite() || *frame < 0.0 || !self.source_seconds(*frame).is_finite()
            })
            || self.beat_frames.windows(2).any(|pair| pair[0] >= pair[1])
            || self
                .downbeat_indices
                .iter()
                .any(|index| *index >= self.beat_frames.len())
            || self
                .downbeat_indices
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("invalid complete raw detector arrays or timebase".into());
        }
        Ok(())
    }

    fn revision(&self) -> Result<String, String> {
        let mut hash = HashWriter(Sha256::new());
        hash.0.update(ENCODING.as_bytes());
        hash.0.update([0]);
        serde_json::to_writer(&mut hash, self).map_err(|error| error.to_string())?;
        Ok(format!("{:x}", hash.0.finalize()))
    }

    pub(super) fn embedded_value(&self) -> Result<Value, String> {
        self.validate()?;
        if self.revision()? != self.raw_revision {
            return Err("raw acquisition revision differs".into());
        }
        serde_json::to_value(Encoded {
            schema_version: SCHEMA_VERSION,
            encoding: ENCODING,
            raw_revision: &self.raw_revision,
            data: self,
        })
        .map_err(|error| error.to_string())
    }

    /// Count all provenance before allocating an enclosing serialization tree.
    pub(super) fn encoded_len(&self) -> Result<usize, String> {
        struct Counter(usize);
        impl Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0 = self
                    .0
                    .checked_add(bytes.len())
                    .filter(|n| *n <= MAX_RAW_BYTES)
                    .ok_or_else(|| io::Error::other("raw acquisition encoding exceeds 16 MiB"))?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut count = Counter(0);
        serde_json::to_writer(
            &mut count,
            &Encoded {
                schema_version: SCHEMA_VERSION,
                encoding: ENCODING,
                raw_revision: &self.raw_revision,
                data: self,
            },
        )
        .map_err(|error| error.to_string())?;
        Ok(count.0)
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        if !digest_text(&self.raw_revision) || self.revision()? != self.raw_revision {
            return Err("raw acquisition revision differs".into());
        }
        let mut output = BoundedBytes(Vec::new());
        output
            .0
            .try_reserve_exact(MAX_RAW_BYTES)
            .map_err(|error| error.to_string())?;
        serde_json::to_writer(
            &mut output,
            &Encoded {
                schema_version: SCHEMA_VERSION,
                encoding: ENCODING,
                raw_revision: &self.raw_revision,
                data: self,
            },
        )
        .map_err(|error| error.to_string())?;
        Ok(output.0)
    }

    pub(super) fn decode(input: &[u8]) -> Result<Self, String> {
        if input.is_empty() || input.len() > MAX_RAW_BYTES {
            return Err("raw acquisition encoding size invalid".into());
        }
        let envelope: Envelope =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        if envelope.schema_version != SCHEMA_VERSION || envelope.encoding != ENCODING {
            return Err("raw acquisition encoding version invalid".into());
        }
        let mut data = envelope.data;
        data.raw_revision = envelope.raw_revision;
        data.source_descriptor = canonical(&data.source_descriptor);
        data.validate()?;
        if data.encode()? != input {
            return Err("raw acquisition encoding is not canonical complete data".into());
        }
        Ok(data)
    }
}

struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > MAX_RAW_BYTES)
        {
            return Err(io::Error::other("raw acquisition encoding exceeds 16 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct DescriptorCounter(usize);
impl Write for DescriptorCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("raw descriptor size overflow"))?;
        if self.0 > MAX_DESCRIPTOR_BYTES {
            return Err(io::Error::other("raw descriptor exceeds 64 KiB"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn digest_text(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn validate_source_descriptor(descriptor: &Value, identity: &str) -> Result<(), String> {
    let mut descriptor_budget = MAX_DESCRIPTOR_BYTES;
    validate_descriptor_tree(descriptor, 0, &mut descriptor_budget)?;
    serde_json::to_writer(&mut DescriptorCounter(0), descriptor)
        .map_err(|error| error.to_string())?;
    if !digest_text(identity)
        || descriptor_digest(descriptor).map_err(|error| error.to_string())? != identity
    {
        return Err("raw source decoder identity differs".into());
    }
    let original = &descriptor["original"];
    let pcm = &descriptor["pcm"];
    let frames = pcm["full_frames"]
        .as_u64()
        .ok_or("raw decoder extent missing")?;
    let channels = pcm["channels"]
        .as_u64()
        .ok_or("raw decoder channels missing")?;
    let full_bytes = frames.checked_mul(channels).and_then(|n| n.checked_mul(4));
    if descriptor["schema_version"] != 1
        || original["sha256"]
            .as_str()
            .is_none_or(|value| !digest_text(value))
        || original["bytes"].as_u64().is_none_or(|value| value == 0)
        || pcm["format"] != "f32-le-interleaved-v1"
        || pcm["channel_layout"] != "interleaved-channel-index-order-v1"
        || pcm["source_zero_bits"] != "0000000000000000"
        || !pcm["provenance"].is_object()
        || pcm["mono_revision"].as_str().is_none_or(str::is_empty)
        || pcm["rate_hz"]
            .as_u64()
            .is_none_or(|value| value == 0 || value > u64::from(u32::MAX))
        || !(1..=32).contains(&channels)
        || frames == 0
        || full_bytes.is_none()
        || pcm["full_bytes"].as_u64() != full_bytes
        || ["interleaved_sha256", "mono_sha256"]
            .iter()
            .any(|key| pcm[*key].as_str().is_none_or(|value| !digest_text(value)))
    {
        return Err("raw source decoder descriptor invalid".into());
    }
    Ok(())
}

fn validate_descriptor_tree(
    value: &Value,
    depth: usize,
    remaining: &mut usize,
) -> Result<(), String> {
    if depth > 32 {
        return Err("raw descriptor nesting bound exceeded".into());
    }
    *remaining = remaining
        .checked_sub(16)
        .ok_or("raw descriptor allocation bound exceeded")?;
    match value {
        Value::String(text) => {
            *remaining = remaining
                .checked_sub(text.len())
                .ok_or("raw descriptor allocation bound exceeded")?;
        }
        Value::Array(values) => {
            for value in values {
                validate_descriptor_tree(value, depth + 1, remaining)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                let key_bytes = key
                    .len()
                    .checked_add(32)
                    .ok_or("raw descriptor allocation bound exceeded")?;
                *remaining = remaining
                    .checked_sub(key_bytes)
                    .ok_or("raw descriptor allocation bound exceeded")?;
                validate_descriptor_tree(value, depth + 1, remaining)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn grow<T, E: de::Error>(values: &mut Vec<T>) -> Result<(), E> {
    if values.len() >= MAX_RAW_EVENTS {
        return Err(E::custom("raw detector array exceeds 250000 entries"));
    }
    if values.len() == values.capacity() {
        let next = values
            .capacity()
            .max(512)
            .saturating_mul(2)
            .min(MAX_RAW_EVENTS);
        values
            .try_reserve_exact(next - values.len())
            .map_err(E::custom)?;
    }
    Ok(())
}

mod float_bits {
    use super::*;
    pub(super) fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{:016x}", value.to_bits()))
    }
    pub(super) fn parse<E: de::Error>(value: &str) -> Result<f64, E> {
        if value.len() != 16
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(E::custom(
                "raw binary64 must be sixteen lowercase hex digits",
            ));
        }
        u64::from_str_radix(value, 16)
            .map(f64::from_bits)
            .map_err(E::custom)
    }
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        parse(&String::deserialize(deserializer)?)
    }
}

mod frame_bits {
    use super::*;
    pub(super) fn serialize<S: Serializer>(
        values: &[f64],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(values.len()))?;
        for value in values {
            sequence.serialize_element(&format!("{:016x}", value.to_bits()))?;
        }
        sequence.end()
    }
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<f64>, D::Error> {
        struct Frames;
        impl<'de> Visitor<'de> for Frames {
            type Value = Vec<f64>;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("at most 250000 original binary64 ODF frames")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<String>()? {
                    grow::<_, A::Error>(&mut values)?;
                    values.push(float_bits::parse(&value)?);
                }
                Ok(values)
            }
        }
        deserializer.deserialize_seq(Frames)
    }
}

fn bounded_indices<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<usize>, D::Error> {
    struct Indices;
    impl<'de> Visitor<'de> for Indices {
        type Value = Vec<usize>;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("at most 250000 original detected-downbeat indices")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = sequence.next_element::<usize>()? {
                grow::<_, A::Error>(&mut values)?;
                values.push(value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Indices)
}

// Deserialize only the small descriptor as Value. Raw arrays never use Value.
// The node/text allowance bounds allocation even for malicious nested metadata;
// the byte counter then enforces the independent 64-KiB serialized maximum.
pub(super) fn bounded_descriptor<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Value, D::Error> {
    let mut remaining = MAX_DESCRIPTOR_BYTES;
    DescriptorSeed {
        remaining: &mut remaining,
        depth: 0,
    }
    .deserialize(deserializer)
}

struct DescriptorSeed<'a> {
    remaining: &'a mut usize,
    depth: usize,
}
impl DescriptorSeed<'_> {
    fn spend<E: de::Error>(&mut self, bytes: usize) -> Result<(), E> {
        *self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or_else(|| E::custom("raw descriptor allocation bound exceeded"))?;
        Ok(())
    }
}
impl<'de> DeserializeSeed<'de> for DescriptorSeed<'_> {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(mut self, deserializer: D) -> Result<Value, D::Error> {
        if self.depth > 32 {
            return Err(de::Error::custom("raw descriptor nesting bound exceeded"));
        }
        self.spend::<D::Error>(16)?;
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for DescriptorSeed<'_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("bounded source decoder descriptor")
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("nonfinite descriptor number"))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_str<E: de::Error>(mut self, value: &str) -> Result<Value, E> {
        self.spend::<E>(value.len())?;
        Ok(Value::String(value.into()))
    }
    fn visit_string<E: de::Error>(mut self, value: String) -> Result<Value, E> {
        self.spend::<E>(value.len())?;
        Ok(Value::String(value))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(DescriptorSeed {
            remaining: self.remaining,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            self.spend::<A::Error>(key.len().saturating_add(32))?;
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate raw descriptor field"));
            }
            let value = map.next_value_seed(DescriptorSeed {
                remaining: self.remaining,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod interpretation_tests {
        include!("source_beatmap/interpretation_tests.rs");
    }

    fn capture() -> RawData {
        let descriptor = json!({"schema_version":1,"original":{"sha256":"a".repeat(64),"bytes":128},
            "pcm":{"format":"f32-le-interleaved-v1","channel_layout":"interleaved-channel-index-order-v1",
                "rate_hz":48000,"channels":2,"full_frames":48000,"full_bytes":384000,
                "interleaved_sha256":"b".repeat(64),"mono_sha256":"c".repeat(64),
                "mono_revision":MONO_RULE,"source_zero_bits":"0000000000000000","provenance":{"decoder":"fixture"}}});
        let mut data = RawData {
            decoder_identity: descriptor_digest(&descriptor).unwrap(),
            source_descriptor: descriptor,
            source_sample_rate_hz: 48000,
            source_frame_count: 48000,
            source_zero_frame: 0,
            mono_sha256: "d".repeat(64),
            mono_rule: MONO_RULE.into(),
            analyzer_sha256: "e".repeat(64),
            analyzer_sample_rate_hz: ANALYZER_RATE_HZ,
            analyzer_frame_count: 44100,
            conversion_revision: CONVERSION_REVISION.into(),
            detector_id: DETECTOR_ID.into(),
            code_revision: CODE_REVISION.into(),
            odf_hop_samples: 512,
            configuration: RawConfiguration::capture(&AnalysisConfig::default()),
            raw_revision: String::new(),
            beat_frames: vec![-0.0, f64::from_bits(0x3ff0000000000001), 44100.0],
            downbeat_indices: vec![0, 2],
        };
        data.raw_revision = data.revision().unwrap();
        data
    }

    #[test]
    fn encoding_preserves_original_bits_associations_and_outside_source_events() {
        let data = capture();
        let encoded = data.encode().unwrap();
        let restored = RawData::decode(&encoded).unwrap();
        assert_eq!(
            restored
                .beat_frames
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            data.beat_frames
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(restored.downbeat_indices, [0, 2]);
        assert!(restored.source_seconds(restored.beat_frames[2]) > 1.0);
        assert_eq!(
            restored.configuration.step_secs.to_bits(),
            AnalysisConfig::default().step_secs.to_bits()
        );
        assert_eq!(restored.encode().unwrap(), encoded);
        assert_eq!(restored.raw_revision, data.raw_revision);
    }

    #[test]
    fn tampering_noncanonical_or_accepted_encodings_are_refused() {
        let data = capture();
        let encoded = data.encode().unwrap();
        let mut changed = encoded.clone();
        let position = changed
            .windows(16)
            .position(|value| value == b"3ff0000000000001")
            .unwrap();
        changed[position + 15] = b'2';
        assert!(RawData::decode(&changed).is_err());
        let mut spaced = b" ".to_vec();
        spaced.extend_from_slice(&encoded);
        assert!(RawData::decode(&spaced).is_err());
        assert!(
            RawData::decode(br#"{"schema_version":1,"encoding":"source-beatmap-pwa-v1"}"#).is_err()
        );
        let mut invalid = capture();
        invalid.downbeat_indices.push(3);
        invalid.raw_revision = invalid.revision().unwrap();
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn staged_budget_charges_held_pcm_coprime_fft_and_serialization_and_refuses_overflow() {
        let regular = acquisition_peak_bytes(48000 * 4 + 37, 48000, 4096 * 4).unwrap();
        let coprime = acquisition_peak_bytes(48000 * 4 + 37, 383981, 4096 * 4).unwrap();
        assert!(coprime > regular);
        assert!(regular >= MAX_RAW_BYTES * 3);
        assert!(acquisition_peak_bytes(48000, 48000, MAX_JOB_BYTES).is_err());
        assert!(acquisition_peak_bytes(usize::MAX, 8000, 0).is_err());
        assert!(acquisition_peak_bytes(0, 48000, 0).is_err());
        assert!(acquisition_peak_bytes(48000, 7999, 0).is_err());
    }

    #[test]
    fn mono_collector_hashes_actual_streamed_binary32_without_changing_bits() {
        let values = [0.0_f32, -0.0_f32, f32::from_bits(0x3f000001), -1.0];
        let encoded: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let mut collector = MonoCollector {
            samples: Vec::with_capacity(values.len()),
            expected: values.len(),
            digest: Sha256::new(),
        };
        collector.write_all(&encoded[..4]).unwrap();
        collector.write_all(&encoded[4..]).unwrap();
        assert_eq!(
            collector
                .samples
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            values
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            collector.digest.clone().finalize(),
            Sha256::digest(&encoded)
        );
        assert!(collector.write_all(&[0; 4]).is_err());
    }

    #[test]
    fn bounded_raw_arrays_and_descriptor_refuse_before_encoding_or_unbounded_reparse() {
        let mut data = capture();
        data.beat_frames = (0..=MAX_RAW_EVENTS).map(|index| index as f64).collect();
        assert!(data.encode().is_err());
        let mut input = "[".to_string();
        input.push_str(&"0,".repeat(MAX_RAW_EVENTS));
        input.push_str("0]");
        let mut deserializer = serde_json::Deserializer::from_str(&input);
        assert!(
            bounded_indices(&mut deserializer)
                .unwrap_err()
                .to_string()
                .contains("250000")
        );
        let mut data = capture();
        data.source_descriptor["pcm"]["provenance"]["oversized"] =
            Value::String("x".repeat(MAX_DESCRIPTOR_BYTES));
        data.decoder_identity = descriptor_digest(&data.source_descriptor).unwrap();
        assert!(data.encode().is_err());
    }
}
