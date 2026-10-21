//! Explicit coordinates over immutable observations; never musical qualification.

use super::{MAX_RECORD_BYTES, SourceBeatmap, editor, record};
use crate::audio_engine::source_beatmap_capture::CaptureView;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(in crate::audio_engine) const MAX_REVIEW_BYTES: usize = 8 * 1024;
const MAX_LABELS: usize = 64;
const MAX_BUILD_BYTES: usize = 512 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::audio_engine) struct Interpretation {
    schema_version: u32,
    pub(in crate::audio_engine) raw_revision: String,
    anchors: Vec<Label>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Label {
    raw_index: usize,
    quarter_bits: String,
}

impl Interpretation {
    pub(in crate::audio_engine) fn parse(encoded: &str) -> Result<Self, String> {
        if encoded.is_empty() || encoded.len() > MAX_REVIEW_BYTES {
            return Err("beatmap_review_byte_limit".into());
        }
        let intent: Self = serde_json::from_str(encoded).map_err(|error| error.to_string())?;
        if intent.schema_version != 1 || !(2..=MAX_LABELS).contains(&intent.anchors.len()) {
            return Err("beatmap_review_schema_or_label_count".into());
        }
        if intent.raw_revision.len() != 64
            || !intent
                .raw_revision
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("beatmap_review_capture_revision".into());
        }
        let mut previous = None;
        for label in &intent.anchors {
            let quarter = record::bits(&label.quarter_bits)?;
            if previous.is_some_and(|(index, beat)| label.raw_index <= index || quarter <= beat) {
                return Err("beatmap_review_labels_not_strictly_increasing".into());
            }
            previous = Some((label.raw_index, quarter));
        }
        Ok(intent)
    }

    /// Small geometry only: no source coordinates are accepted from the client.
    fn accepted(&self, raw: &impl CaptureView) -> Result<Value, String> {
        if self.raw_revision != raw.revision() {
            return Err("beatmap_review_capture_revision_changed".into());
        }
        let duration = raw.source_descriptor()["pcm"]["full_frames"]
            .as_u64()
            .ok_or("decoder_extent_missing")? as f64
            / raw.source_descriptor()["pcm"]["rate_hz"]
                .as_u64()
                .ok_or("decoder_rate_missing")? as f64;
        let mut anchors = Vec::with_capacity(self.anchors.len());
        for (ordinal, label) in self.anchors.iter().enumerate() {
            let seconds = raw
                .source_seconds(label.raw_index)
                .ok_or("beatmap_review_raw_index_outside_capture")?;
            let physical_end =
                raw.source_frame_count() as f64 / f64::from(raw.source_sample_rate_hz());
            if !seconds.is_finite()
                || seconds < 0.0
                || seconds >= duration
                || seconds >= physical_end
            {
                return Err("beatmap_review_anchor_outside_source".into());
            }
            anchors.push(json!({"id":format!("raw-label-{ordinal}"),
                "source_seconds_bits":format!("{:016x}",seconds.to_bits()),
                "beat_bits":label.quarter_bits,"raw_beat_index":label.raw_index,
                "review":"estimated","sample_edit":null}));
        }
        let intervals: Vec<Value> = (1..anchors.len()).map(|_| json!({"coverage":"uncertain",
            "meter":{"status":"unknown","downbeat_label":null,"evidence":"No meter qualification supplied"}})).collect();
        Ok(json!({"map_revision":"0".repeat(64),"parent_revision":null,
            "beat_unit":"quarter_note","interpolation_policy":"pwa-source-seconds-quarter-v1",
            "boundary_policy":"right-interior-left-final-gap-unavailable-v1",
            "snap_policy":"nearest-quarter-subdivision-later-tie-v1","anchors":anchors,
            "intervals":intervals,"lineage":[{"id":"explicit-raw-labels-v1","parent_revision":null,
                "kind":"beat_count","payload":{"interpretation":self},
                "evidence":"Explicit quarter-coordinate labels; detector positions remain estimates",
                "author":"source-map-review"}]}))
    }

    pub(in crate::audio_engine) fn validate_for(
        &self,
        raw: &impl CaptureView,
    ) -> Result<(), String> {
        record::validate_editor_geometry(&self.accepted(raw)?, raw.event_count())
    }
}

/// The retained capture, three canonical JSON trees, canonical hash scratch,
/// typed reparse, output strings and geometry coexist during seal/parse/store.
/// The 32x serialized bound covers their node/string/vector capacity overhead;
/// fixed work also charges the bounded source descriptor and label trees. Count
/// before any complete Value tree grows; overflow refuses instead of wrapping.
pub(in crate::audio_engine) fn budget(
    raw_bytes: usize,
    held_source_bytes: usize,
    labels: usize,
) -> Result<usize, String> {
    let record_bound = raw_bytes
        .checked_add(128 * 1024)
        .ok_or("beatmap_review_budget_overflow")?;
    if record_bound > MAX_RECORD_BYTES || labels > MAX_LABELS {
        return Err("beatmap_review_record_limit".into());
    }
    let peak = raw_bytes
        .checked_mul(32)
        .and_then(|n| n.checked_add(held_source_bytes))
        .and_then(|n| n.checked_add(labels.checked_mul(8192)?))
        .and_then(|n| n.checked_add(16 * 1024 * 1024))
        .ok_or("beatmap_review_budget_overflow")?;
    if peak > MAX_BUILD_BYTES {
        return Err("beatmap review exceeds 512-MiB simultaneous job peak".into());
    }
    Ok(peak)
}

/// Data-only seam shared by explicit labels and later qualified evidence. Only
/// already validated interpretation may supply coordinates, never ordinal count.
pub(in crate::audio_engine) fn build(
    raw: &impl CaptureView,
    intent: &Interpretation,
    held_source_bytes: usize,
) -> Result<SourceBeatmap, String> {
    budget(raw.encoded_len()?, held_source_bytes, intent.anchors.len())?;
    intent.validate_for(raw)?;
    let version = raw.map_envelope_version();
    editor::seal(
        json!({"schema_version":version,"encoding":format!("source-beatmap-pwa-v{version}"),"record":{
        "source":{"decoder_identity":raw.decoder_identity(),"decoder_descriptor":raw.source_descriptor()},
        "raw":{"raw_revision":"0".repeat(64),"capture":raw.embedded_value()?},
        "accepted":intent.accepted(raw)?}}),
    )
}
