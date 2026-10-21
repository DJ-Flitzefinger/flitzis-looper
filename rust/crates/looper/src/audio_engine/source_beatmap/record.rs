//! Strict durable data and numeric construction; no source/adoption authority.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use serde::de::{self, DeserializeOwned, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::super::cold_store::{canonical, descriptor_digest};
use super::{MAX_ANCHORS, MAX_RECORD_BYTES};

const ENCODING: &str = "source-beatmap-pwa-v1";
const ENCODING_V2: &str = "source-beatmap-pwa-v2";
const INTERPOLATION: &str = "pwa-source-seconds-quarter-v1";
const BOUNDARY: &str = "right-interior-left-final-gap-unavailable-v1";
const SNAP: &str = "nearest-quarter-subdivision-later-tie-v1";
const MAX_TEXT_BYTES: usize = 4096;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope<R> {
    schema_version: u32,
    encoding: String,
    record: Record<R>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record<R> {
    source: Source,
    raw: R,
    accepted: Accepted,
}

#[derive(Serialize)]
#[serde(untagged)]
enum VersionedRaw {
    Legacy(Raw),
    Capture(CapturedRaw),
}

/// Inspect only the small discriminator while skipping the bounded record.
/// Direct typed dispatch preserves each version's precise bounded-array errors;
/// an untagged fallback must not hide a genuine V1 refusal behind enum mismatch.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeHeader {
    schema_version: u32,
    encoding: String,
    #[serde(rename = "record")]
    _record: IgnoredAny,
}

fn typed_envelope<R: DeserializeOwned>(
    encoded: &str,
    wrap: impl FnOnce(R) -> VersionedRaw,
) -> Result<Envelope<VersionedRaw>, String> {
    let envelope: Envelope<R> = serde_json::from_str(encoded).map_err(|error| error.to_string())?;
    Ok(Envelope {
        schema_version: envelope.schema_version,
        encoding: envelope.encoding,
        record: Record {
            source: envelope.record.source,
            raw: wrap(envelope.record.raw),
            accepted: envelope.record.accepted,
        },
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CapturedRaw {
    raw_revision: String,
    capture: super::super::source_beatmap_raw_data::Envelope,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Source {
    decoder_identity: String,
    decoder_descriptor: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    raw_revision: String,
    detector: Detector,
    analysis_input: Option<AnalysisInput>,
    timestamp_resolution_seconds_bits: Option<String>,
    #[serde(deserialize_with = "bounded_items")]
    beat_seconds_bits: Vec<String>,
    #[serde(deserialize_with = "bounded_items")]
    downbeat_seconds_bits: Vec<String>,
    uncertainty: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Detector {
    detector_id: String,
    code_revision: String,
    configuration: Value,
    model: Option<Model>,
    preprocessing: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Model {
    model_id: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AnalysisInput {
    pcm_sha256: String,
    sample_rate_hz: u32,
    frame_count: u64,
    source_zero_bits: String,
    preprocessing_identity: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Accepted {
    map_revision: String,
    parent_revision: Option<String>,
    beat_unit: String,
    interpolation_policy: String,
    boundary_policy: String,
    snap_policy: String,
    #[serde(deserialize_with = "bounded_items")]
    anchors: Vec<AnchorRecord>,
    #[serde(deserialize_with = "bounded_items")]
    intervals: Vec<Interval>,
    #[serde(deserialize_with = "bounded_items")]
    lineage: Vec<LineageOperation>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AnchorRecord {
    id: String,
    source_seconds_bits: String,
    beat_bits: String,
    raw_beat_index: Option<usize>,
    review: Review,
    sample_edit: Option<SampleEdit>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Review {
    Estimated,
    Reviewed,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SampleEdit {
    sample_index: u64,
    sample_rate_hz: u32,
    evidence: String,
    author: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Coverage {
    Trusted,
    Uncertain,
    GapUnknownCount,
}

impl Coverage {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Uncertain => "uncertain",
            Self::GapUnknownCount => "gap_unknown_count",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Interval {
    coverage: Coverage,
    meter: Meter,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Meter {
    status: MeterStatus,
    downbeat_label: Option<i64>,
    evidence: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum MeterStatus {
    Unknown,
    #[serde(rename = "quarter_note_4_4_verified")]
    QuarterNote4_4Verified,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LineageOperation {
    id: String,
    parent_revision: Option<String>,
    kind: OperationKind,
    payload: Value,
    evidence: String,
    author: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum OperationKind {
    SourcePosition,
    BeatCount,
    DownbeatMeter,
    WholeMapTranslation,
    Refinement,
    Coverage,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Anchor {
    pub(super) seconds: f64,
    pub(super) beat: f64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Segment {
    pub(super) coverage: Coverage,
    pub(super) bpm: f64,
    pub(super) four_four: bool,
    /// Effective phase only: a historical label with unknown meter is not a bar claim.
    pub(super) downbeat_label: Option<i64>,
}

pub(super) struct ValidatedMap {
    pub(super) encoded: Arc<str>,
    pub(super) revision: String,
    pub(super) source_identity: String,
    pub(super) source_duration: f64,
    pub(super) sample_rate_hz: u32,
    pub(super) source_frames: u64,
    pub(super) can_undo: bool,
    pub(super) raw_count: usize,
    pub(super) anchors: Box<[Anchor]>,
    pub(super) segments: Box<[Segment]>,
    /// Derived immutable coverage index, not another map or timing authority.
    pub(super) unsupported_prefix: Box<[u32]>,
    pub(super) meter_prefix: Box<[u32]>,
    pub(super) gap_prefix: Box<[u32]>,
    pub(super) coverage_changes: Box<[u32]>,
    pub(super) meter_changes: Box<[u32]>,
    pub(super) phase_changes: Box<[u32]>,
    pub(super) unknown_phase_prefix: Box<[u32]>,
}

/// Bound arrays during typed deserialization, not after collecting arbitrary input.
fn bounded_items<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Bounded<T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an array containing at most 250000 items")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            if seq.size_hint().is_some_and(|count| count > MAX_ANCHORS) {
                return Err(de::Error::custom("beatmap_count_limit"));
            }
            let mut result = Vec::new();
            while let Some(item) = seq.next_element()? {
                if result.len() == MAX_ANCHORS {
                    return Err(de::Error::custom("beatmap_count_limit"));
                }
                result.try_reserve(1).map_err(de::Error::custom)?;
                result.push(item);
            }
            Ok(result)
        }
    }
    deserializer.deserialize_seq(Bounded(std::marker::PhantomData))
}

pub(super) fn parse(encoded: &str) -> Result<ValidatedMap, String> {
    if encoded.len() > MAX_RECORD_BYTES {
        return Err("beatmap_byte_limit".into());
    }
    let header: EnvelopeHeader =
        serde_json::from_str(encoded).map_err(|error| error.to_string())?;
    let mut envelope = match (header.schema_version, header.encoding.as_str()) {
        (1, ENCODING) => typed_envelope(encoded, VersionedRaw::Legacy)?,
        (2, ENCODING_V2) => typed_envelope(encoded, VersionedRaw::Capture)?,
        _ => return Err("unsupported_beatmap_envelope".into()),
    };
    let version = envelope.schema_version;
    let encoding = envelope.encoding.clone();
    let record = &mut envelope.record;
    let source_duration = source_duration(&record.source)?;
    let raw_count = match (&mut record.raw, version, encoding.as_str()) {
        (VersionedRaw::Legacy(raw), 1, ENCODING) => {
            validate_raw(raw, source_duration)?;
            raw.beat_seconds_bits.len()
        }
        (VersionedRaw::Capture(raw), 2, ENCODING_V2) => {
            digest_text(&raw.raw_revision)?;
            let capture = raw.capture.validate()?;
            if capture.decoder_identity != record.source.decoder_identity
                || capture.source_descriptor != record.source.decoder_descriptor
            {
                return Err("capture_source_identity_mismatch".into());
            }
            capture.beat_frames.len()
        }
        _ => return Err("unsupported_beatmap_envelope".into()),
    };
    let (anchors, segments) = geometry(&record.accepted, raw_count)?;
    let mut unsupported_prefix = Vec::new();
    let mut meter_prefix = Vec::new();
    let mut gap_prefix = Vec::new();
    unsupported_prefix
        .try_reserve_exact(segments.len() + 1)
        .map_err(|error| error.to_string())?;
    unsupported_prefix.push(0_u32);
    meter_prefix.push(0_u32);
    gap_prefix.push(0_u32);
    let mut unknown_phase_prefix = Vec::new();
    unknown_phase_prefix
        .try_reserve_exact(segments.len() + 1)
        .map_err(|error| error.to_string())?;
    unknown_phase_prefix.push(0_u32);
    let mut coverage_changes = Vec::new();
    let mut meter_changes = Vec::new();
    let mut phase_changes = Vec::new();
    for changes in [
        &mut coverage_changes,
        &mut meter_changes,
        &mut phase_changes,
    ] {
        changes
            .try_reserve_exact(segments.len())
            .map_err(|error| error.to_string())?;
        changes.push(0_u32);
    }
    for pair in segments.windows(2) {
        coverage_changes.push(
            coverage_changes.last().copied().unwrap_or(0)
                + u32::from(pair[0].coverage != pair[1].coverage),
        );
        meter_changes.push(
            meter_changes.last().copied().unwrap_or(0)
                + u32::from(pair[0].four_four != pair[1].four_four),
        );
        phase_changes.push(
            phase_changes.last().copied().unwrap_or(0)
                + u32::from(
                    pair[0].downbeat_label.map(|label| label.rem_euclid(4))
                        != pair[1].downbeat_label.map(|label| label.rem_euclid(4)),
                ),
        );
    }
    for segment in &segments {
        unknown_phase_prefix.push(
            unknown_phase_prefix.last().copied().unwrap_or(0)
                + u32::from(segment.downbeat_label.is_none()),
        );
        let count = unsupported_prefix.last().copied().unwrap_or(0);
        unsupported_prefix.push(count + u32::from(segment.coverage != Coverage::Trusted));
        meter_prefix
            .push(meter_prefix.last().copied().unwrap_or(0) + u32::from(!segment.four_four));
        gap_prefix.push(
            gap_prefix.last().copied().unwrap_or(0)
                + u32::from(segment.coverage == Coverage::GapUnknownCount),
        );
    }
    let revision = record.accepted.map_revision.clone();
    let source_identity = record.source.decoder_identity.clone();
    let sample_rate_hz = record.source.decoder_descriptor["pcm"]["rate_hz"]
        .as_u64()
        .ok_or("decoder_rate_missing")? as u32;
    let source_frames = record.source.decoder_descriptor["pcm"]["full_frames"]
        .as_u64()
        .ok_or("decoder_extent_missing")?;

    // Move the typed payload into one JSON tree after preparing small geometry.
    // Existing canonical/digest helpers remain the only hash authority. These
    // bounded operations are off realtime; peak RAM still requires measurement.
    let mut value = serde_json::to_value(envelope.record).map_err(|error| error.to_string())?;
    let can_undo =
        super::editor::undo_operation(&value, sample_rate_hz, source_frames, raw_count).is_some();
    verify_revision(&mut value["raw"], "raw_revision")?;
    let declared = value["accepted"]
        .as_object_mut()
        .and_then(|accepted| accepted.remove("map_revision"))
        .ok_or("missing_map_revision")?;
    let expected = descriptor_digest(&value).map_err(|error| error.to_string())?;
    if declared.as_str() != Some(expected.as_str()) {
        return Err("map_revision_mismatch".into());
    }
    value["accepted"]["map_revision"] = declared;
    let envelope_value = serde_json::json!({
        "schema_version": version, "encoding": encoding, "record": value,
    });
    let canonical_value = canonical(&envelope_value);
    let encoded = serde_json::to_string(&canonical_value).map_err(|error| error.to_string())?;
    if encoded.len() > MAX_RECORD_BYTES {
        return Err("beatmap_byte_limit".into());
    }
    Ok(ValidatedMap {
        encoded: Arc::from(encoded),
        revision,
        source_identity,
        source_duration,
        sample_rate_hz,
        source_frames,
        can_undo,
        raw_count,
        anchors: anchors.into_boxed_slice(),
        segments: segments.into_boxed_slice(),
        unsupported_prefix: unsupported_prefix.into_boxed_slice(),
        meter_prefix: meter_prefix.into_boxed_slice(),
        gap_prefix: gap_prefix.into_boxed_slice(),
        coverage_changes: coverage_changes.into_boxed_slice(),
        meter_changes: meter_changes.into_boxed_slice(),
        phase_changes: phase_changes.into_boxed_slice(),
        unknown_phase_prefix: unknown_phase_prefix.into_boxed_slice(),
    })
}

fn verify_revision(value: &mut Value, field: &str) -> Result<(), String> {
    let object = value.as_object_mut().ok_or("revision_payload_not_object")?;
    let declared = object.remove(field).ok_or("missing_revision")?;
    let declared_text = declared.as_str().ok_or("revision_not_text")?;
    digest_text(declared_text)?;
    let expected = descriptor_digest(value).map_err(|error| error.to_string())?;
    if declared_text != expected {
        return Err("raw_revision_mismatch".into());
    }
    value[field] = declared;
    Ok(())
}

fn source_duration(source: &Source) -> Result<f64, String> {
    digest_text(&source.decoder_identity)?;
    let descriptor = &source.decoder_descriptor;
    if descriptor_digest(descriptor).map_err(|error| error.to_string())? != source.decoder_identity
    {
        return Err("decoder_identity_mismatch".into());
    }
    let original = &descriptor["original"];
    digest_text(
        original["sha256"]
            .as_str()
            .ok_or("original_digest_missing")?,
    )?;
    if original["bytes"].as_u64().is_none_or(|bytes| bytes == 0) {
        return Err("original_extent_invalid".into());
    }
    let pcm = &descriptor["pcm"];
    if descriptor["schema_version"].as_u64() != Some(1)
        || pcm["format"] != "f32-le-interleaved-v1"
        || pcm["channel_layout"] != "interleaved-channel-index-order-v1"
        || pcm["source_zero_bits"] != "0000000000000000"
        || !pcm["provenance"].is_object()
    {
        return Err("decoder_descriptor_shape_invalid".into());
    }
    for key in ["interleaved_sha256", "mono_sha256"] {
        digest_text(pcm[key].as_str().ok_or("decoder_pcm_digest_missing")?)?;
    }
    text(
        pcm["mono_revision"]
            .as_str()
            .ok_or("mono_revision_missing")?,
    )?;
    let rate = pcm["rate_hz"].as_u64().ok_or("decoder_rate_missing")?;
    let channels = pcm["channels"].as_u64().ok_or("decoder_channels_missing")?;
    let frames = pcm["full_frames"]
        .as_u64()
        .ok_or("decoder_extent_missing")?;
    if rate == 0 || rate > u64::from(u32::MAX) || channels == 0 || frames == 0 {
        return Err("decoder_geometry_invalid".into());
    }
    let bytes = frames
        .checked_mul(channels)
        .and_then(|samples| samples.checked_mul(4))
        .ok_or("decoder_extent_overflow")?;
    if pcm["full_bytes"].as_u64() != Some(bytes) {
        return Err("decoder_byte_extent_mismatch".into());
    }
    let duration = frames as f64 / rate as f64;
    if !duration.is_finite() || duration <= 0.0 {
        return Err("decoder_duration_invalid".into());
    }
    Ok(duration)
}

fn validate_raw(raw: &Raw, source_duration: f64) -> Result<(), String> {
    digest_text(&raw.raw_revision)?;
    text(&raw.detector.detector_id)?;
    text(&raw.detector.code_revision)?;
    text(&raw.uncertainty)?;
    if !raw.detector.configuration.is_object() || !raw.detector.preprocessing.is_object() {
        return Err("detector_provenance_invalid".into());
    }
    if let Some(model) = &raw.detector.model {
        text(&model.model_id)?;
        digest_text(&model.sha256)?;
    }
    let has_events = !raw.beat_seconds_bits.is_empty() || !raw.downbeat_seconds_bits.is_empty();
    if has_events
        && (raw.analysis_input.is_none() || raw.timestamp_resolution_seconds_bits.is_none())
    {
        return Err("raw_event_provenance_missing".into());
    }
    if let Some(input) = &raw.analysis_input {
        digest_text(&input.pcm_sha256)?;
        text(&input.preprocessing_identity)?;
        if input.sample_rate_hz == 0
            || input.frame_count == 0
            || bits(&input.source_zero_bits)? != 0.0
        {
            return Err("analysis_input_geometry_invalid".into());
        }
    }
    if let Some(resolution) = &raw.timestamp_resolution_seconds_bits
        && bits(resolution)? <= 0.0
    {
        return Err("raw_resolution_invalid".into());
    }
    for positions in [&raw.beat_seconds_bits, &raw.downbeat_seconds_bits] {
        let mut previous = None;
        for encoded in positions {
            let current = bits(encoded)?;
            if current < 0.0
                || current >= source_duration
                || previous.is_some_and(|before| before >= current)
            {
                return Err("raw_source_positions_invalid".into());
            }
            previous = Some(current);
        }
    }
    Ok(())
}

fn geometry(accepted: &Accepted, raw_count: usize) -> Result<(Vec<Anchor>, Vec<Segment>), String> {
    digest_text(&accepted.map_revision)?;
    optional_digest(accepted.parent_revision.as_deref())?;
    if accepted.parent_revision.as_deref() == Some(accepted.map_revision.as_str()) {
        return Err("map_parent_is_self".into());
    }
    if accepted.beat_unit != "quarter_note"
        || accepted.interpolation_policy != INTERPOLATION
        || accepted.boundary_policy != BOUNDARY
        || accepted.snap_policy != SNAP
        || accepted.anchors.len() < 2
        || accepted.intervals.len() != accepted.anchors.len() - 1
    {
        return Err("map_policy_or_shape_invalid".into());
    }
    let mut ids = HashSet::new();
    let mut anchors = Vec::new();
    anchors
        .try_reserve_exact(accepted.anchors.len())
        .map_err(|error| error.to_string())?;
    for anchor in &accepted.anchors {
        text(&anchor.id)?;
        if !ids.insert(anchor.id.as_str()) {
            return Err("duplicate_anchor_id".into());
        }
        if anchor
            .raw_beat_index
            .is_some_and(|index| index >= raw_count)
        {
            return Err("raw_anchor_association_invalid".into());
        }
        if let Some(edit) = &anchor.sample_edit {
            text(&edit.evidence)?;
            text(&edit.author)?;
            if edit.sample_rate_hz == 0 {
                return Err("sample_edit_rate_invalid".into());
            }
            // This is retained provenance, not another transform or readable index.
            let position = edit.sample_index as f64 / f64::from(edit.sample_rate_hz);
            if !position.is_finite() {
                return Err("sample_edit_position_invalid".into());
            }
        }
        anchors.push(Anchor {
            seconds: bits(&anchor.source_seconds_bits)?,
            beat: bits(&anchor.beat_bits)?,
        });
    }
    let mut segments = Vec::new();
    segments
        .try_reserve_exact(accepted.intervals.len())
        .map_err(|error| error.to_string())?;
    for (pair, interval) in anchors.windows(2).zip(&accepted.intervals) {
        text(&interval.meter.evidence)?;
        let seconds = checked_difference(pair[0].seconds, pair[1].seconds)?;
        let beats = checked_difference(pair[0].beat, pair[1].beat)?;
        let forward = beats / seconds;
        let inverse = seconds / beats;
        let bpm = 60.0 * forward;
        if [forward, inverse, bpm]
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err("map_slope_invalid".into());
        }
        segments.push(Segment {
            coverage: interval.coverage,
            bpm,
            four_four: matches!(interval.meter.status, MeterStatus::QuarterNote4_4Verified),
            downbeat_label: matches!(interval.meter.status, MeterStatus::QuarterNote4_4Verified)
                .then_some(interval.meter.downbeat_label)
                .flatten(),
        });
    }
    let mut operations = HashSet::new();
    for operation in &accepted.lineage {
        text(&operation.id)?;
        text(&operation.author)?;
        text(&operation.evidence)?;
        optional_digest(operation.parent_revision.as_deref())?;
        if !operation.payload.is_object() || !operations.insert(operation.id.as_str()) {
            return Err("lineage_invalid".into());
        }
    }
    Ok((anchors, segments))
}

/// Reuse the actual record geometry validator for a cold editor inverse preview.
/// Historical generic lineage remains accepted data, never executable permission.
pub(super) fn validate_editor_geometry(value: &Value, raw_count: usize) -> Result<(), String> {
    let accepted: Accepted =
        serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    geometry(&accepted, raw_count).map(|_| ())
}

fn checked_difference(left: f64, right: f64) -> Result<f64, String> {
    let delta = right - left;
    let minimum = 16.0 * ulp(left).max(ulp(right));
    if !delta.is_finite() || delta <= 0.0 || !minimum.is_finite() || delta < minimum {
        return Err("map_spacing_invalid".into());
    }
    Ok(delta)
}

fn ulp(value: f64) -> f64 {
    let magnitude = value.abs();
    let next = magnitude.next_up();
    if next.is_finite() {
        next - magnitude
    } else {
        magnitude - magnitude.next_down()
    }
}

pub(super) fn bits(encoded: &str) -> Result<f64, String> {
    if encoded.len() != 16 || !encoded.bytes().all(lower_hex) {
        return Err("binary64_bits_invalid".into());
    }
    let value =
        f64::from_bits(u64::from_str_radix(encoded, 16).map_err(|error| error.to_string())?);
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| "binary64_nonfinite".into())
}

fn digest_text(value: &str) -> Result<(), String> {
    if value.len() == 64 && value.bytes().all(lower_hex) {
        Ok(())
    } else {
        Err("complete_digest_invalid".into())
    }
}

fn lower_hex(value: u8) -> bool {
    value.is_ascii_digit() || (b'a'..=b'f').contains(&value)
}

fn optional_digest(value: Option<&str>) -> Result<(), String> {
    value.map_or(Ok(()), digest_text)
}

fn text(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES {
        Err("provenance_text_invalid".into())
    } else {
        Ok(())
    }
}
