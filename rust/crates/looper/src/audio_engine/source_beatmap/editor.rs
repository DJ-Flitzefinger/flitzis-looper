//! Cold immutable edits and bounded viewport queries; never audio authority.

use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};

use super::super::cold_store::descriptor_digest;
use super::{
    MAX_ANCHORS, MAX_PROJECTION_POINTS, MAX_RECORD_BYTES, SourceBeatmap, budget, interval_edits,
    record, review_index::RawIndex, review_window,
};

pub(in crate::audio_engine) const MAX_EDIT_BYTES: usize = 8192;

pub(super) struct EditIntent {
    operation: EditOperation,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum EditOperation {
    ReviewRawWindow(review_window::ReviewWindow),
    MoveBeat {
        schema_version: u32,
        beat: f64,
        sample_index: u64,
        sample_rate_hz: u32,
    },
    Translate {
        schema_version: u32,
        sample_offset: i64,
        sample_rate_hz: u32,
    },
    Undo {
        schema_version: u32,
    },
    SetCount {
        schema_version: u32,
        first_anchor_index: usize,
        last_anchor_index: usize,
        quarter_notes: f64,
    },
    SetMeter {
        schema_version: u32,
        first_anchor_index: usize,
        last_anchor_index: usize,
        meter_status: record::MeterStatus,
    },
    SetDownbeat {
        schema_version: u32,
        first_anchor_index: usize,
        last_anchor_index: usize,
        downbeat_label: Option<i64>,
    },
    SetCoverage {
        schema_version: u32,
        first_anchor_index: usize,
        last_anchor_index: usize,
        coverage: record::Coverage,
        reviewed_quarter_notes: Option<f64>,
    },
}

pub(in crate::audio_engine) fn validate_intent(encoded: &str) -> Result<(), String> {
    parse_intent(encoded).map(|_| ())
}

fn parse_intent(encoded: &str) -> Result<EditIntent, String> {
    if encoded.len() > MAX_EDIT_BYTES {
        return Err("beatmap_edit_byte_limit".into());
    }
    let operation: EditOperation =
        serde_json::from_str(encoded).map_err(|error| error.to_string())?;
    let version = match &operation {
        EditOperation::MoveBeat { schema_version, .. }
        | EditOperation::Translate { schema_version, .. }
        | EditOperation::SetCount { schema_version, .. }
        | EditOperation::SetMeter { schema_version, .. }
        | EditOperation::SetDownbeat { schema_version, .. }
        | EditOperation::SetCoverage { schema_version, .. }
        | EditOperation::Undo { schema_version } => *schema_version,
        EditOperation::ReviewRawWindow(window) => window.schema_version,
    };
    if version != 1 {
        return Err("unsupported_beatmap_edit_schema".into());
    }
    if let EditOperation::ReviewRawWindow(window) = &operation {
        window.validate()?;
    }
    let value: Value = serde_json::from_str(encoded).map_err(|error| error.to_string())?;
    if (matches!(operation, EditOperation::SetDownbeat { .. })
        && value
            .as_object()
            .is_none_or(|object| !object.contains_key("downbeat_label")))
        || (matches!(operation, EditOperation::SetCoverage { .. })
            && value
                .as_object()
                .is_none_or(|object| !object.contains_key("reviewed_quarter_notes")))
    {
        return Err("beatmap_edit_field_missing".into());
    }
    Ok(EditIntent { operation })
}

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

#[cfg(test)]
pub(in crate::audio_engine) fn seal(mut value: Value) -> Result<SourceBeatmap, String> {
    seal_with_context(&mut value, 0, 0)
}

pub(in crate::audio_engine) fn seal_with_context(
    value: &mut Value,
    held: usize,
    retained: usize,
) -> Result<SourceBeatmap, String> {
    budget::value_admission(value, held, retained)?;
    check_record_size(value)?;
    value["record"]["raw"]
        .as_object_mut()
        .ok_or("raw_not_object")?
        .remove("raw_revision");
    value["record"]["raw"]["raw_revision"] =
        json!(descriptor_digest(&value["record"]["raw"]).map_err(|error| error.to_string())?);
    value["record"]["accepted"]
        .as_object_mut()
        .ok_or("accepted_not_object")?
        .remove("map_revision");
    value["record"]["accepted"]["map_revision"] =
        json!(descriptor_digest(&value["record"]).map_err(|error| error.to_string())?);
    SourceBeatmap::parse_with_context(&value.to_string(), held, retained)
}

/// Bound serialization before digest helpers allocate their canonical encoding.
fn check_record_size(value: &Value) -> Result<(), String> {
    struct ByteLimit(usize);
    impl Write for ByteLimit {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|size| *size <= MAX_RECORD_BYTES)
                .ok_or_else(|| io::Error::other("beatmap_byte_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut ByteLimit(0), value).map_err(|_| "beatmap_byte_limit".into())
}

impl SourceBeatmap {
    #[cfg(test)]
    pub(in crate::audio_engine) fn manual(
        descriptor: Value,
        origin: f64,
        period: f64,
    ) -> Result<Self, String> {
        Self::manual_with_held(descriptor, origin, period, 0)
    }

    pub(in crate::audio_engine) fn manual_with_held(
        descriptor: Value,
        origin: f64,
        period: f64,
        held: usize,
    ) -> Result<Self, String> {
        let rate = descriptor["pcm"]["rate_hz"]
            .as_u64()
            .ok_or("decoder_rate_missing")?;
        let frames = descriptor["pcm"]["full_frames"]
            .as_u64()
            .ok_or("decoder_extent_missing")?;
        let duration = frames as f64 / rate as f64;
        if !origin.is_finite()
            || !period.is_finite()
            || period <= 0.0
            || !duration.is_finite()
            || duration <= 0.0
        {
            return Err("manual_source_timing_invalid".into());
        }
        let first_beat = ((0.0 - origin) / period).floor() - 1.0;
        let last_beat = ((duration - origin) / period).ceil() + 1.0;
        let first = origin + first_beat * period;
        let last = origin + last_beat * period;
        let anchors: Vec<Value> = [(first, first_beat), (last, last_beat)].into_iter().enumerate().map(|(index, (seconds, beat))| json!({
            "id":format!("manual-bound-{index}"),"source_seconds_bits":bits(seconds),"beat_bits":bits(beat),
            "raw_beat_index":null,"review":"reviewed","sample_edit":null,
        })).collect();
        let mut value = json!({"schema_version":1,"encoding":"source-beatmap-pwa-v1","record":{
            "source":{"decoder_identity":descriptor_digest(&descriptor).map_err(|error|error.to_string())?,"decoder_descriptor":descriptor},
            "raw":{"raw_revision":"0".repeat(64),"detector":{"detector_id":"explicit-manual-source-grid","code_revision":"manual-source-grid-v1", "configuration":{"origin_seconds_bits":bits(origin),"period_seconds_bits":bits(period)},"model":null,"preprocessing":{}},"analysis_input":null,"timestamp_resolution_seconds_bits":null,"beat_seconds_bits":[],"downbeat_seconds_bits":[],"uncertainty":"Manual timing and 4/4 interpretation; no detector or musical acceptance"},
            "accepted":{"map_revision":"0".repeat(64),"parent_revision":null,"beat_unit":"quarter_note","interpolation_policy":"pwa-source-seconds-quarter-v1","boundary_policy":"right-interior-left-final-gap-unavailable-v1","snap_policy":"nearest-quarter-subdivision-later-tie-v1","anchors":anchors,"intervals":[{"coverage":"trusted","meter":{"status":"quarter_note_4_4_verified","downbeat_label":0,"evidence":"Explicit manual quarter-note 4/4 interpretation"}}],"lineage":[]}
        }});
        seal_with_context(&mut value, held, 0)
    }

    #[cfg(test)]
    pub(in crate::audio_engine) fn edited(&self, encoded: &str) -> Result<Self, String> {
        self.edited_with_held(encoded, 0)
    }

    pub(in crate::audio_engine) fn edited_with_held(
        &self,
        encoded: &str,
        held: usize,
    ) -> Result<Self, String> {
        let intent = parse_intent(encoded)?;
        let retained = self.inner.retained_bytes()?;
        budget::scan(self.canonical_record(), held, retained)?;
        budget::edit(
            budget::edit_bound(self.canonical_record().len())?,
            held,
            retained,
        )?;
        let mut value: Value =
            serde_json::from_str(self.canonical_record()).map_err(|error| error.to_string())?;
        // Count existing geometry and any complete executable inverse by borrowing
        // it. A tiny wire intent cannot conceal arbitrarily long old provenance.
        let accepted = &value["record"]["accepted"];
        let inverse_bytes = accepted["lineage"]
            .as_array()
            .ok_or("lineage_missing")?
            .iter()
            .try_fold(0_usize, |largest, row| {
                Ok::<_, String>(largest.max(budget::serialized_len(&row["payload"]["inverse"])?))
            })?;
        let prospective = budget::sum(&[
            self.canonical_record().len(),
            budget::serialized_len(&accepted["anchors"])?,
            budget::serialized_len(&accepted["intervals"])?,
            inverse_bytes,
            MAX_EDIT_BYTES,
            budget::EDIT_ALLOWANCE,
        ])?;
        budget::edit(prospective, held, retained)?;
        // Arbitrary imported payloads can be far denser than their byte text.
        // Borrow-count every possible old inverse and geometry before copying.
        let inverse_tree = accepted["lineage"]
            .as_array()
            .ok_or("lineage_missing")?
            .iter()
            .try_fold(0_usize, |largest, row| {
                Ok::<_, String>(largest.max(budget::value_tree(&row["payload"]["inverse"])?))
            })?;
        let prospective_tree = budget::sum(&[
            budget::value_tree(&value)?,
            budget::value_tree(&accepted["anchors"])?,
            budget::value_tree(&accepted["intervals"])?,
            inverse_tree,
            budget::scan(encoded, 0, 0)?,
            64 * 8192 + 16 * 1024,
        ])?;
        budget::tree_admission(prospective, prospective_tree, held, retained)?;
        let operation = match intent.operation {
            EditOperation::ReviewRawWindow(window) => {
                let raw = self
                    .inner
                    .raw_index
                    .as_ref()
                    .ok_or("beatmap_window_complete_capture_unavailable")?;
                let predecessor_parent_revision =
                    value["record"]["accepted"]["parent_revision"].clone();
                let inverse = review_window::apply(
                    &mut value["record"]["accepted"],
                    raw,
                    &window,
                    &self.inner.revision,
                    self.inner.sample_rate_hz,
                    self.inner.source_frames,
                )?;
                (
                    "refinement",
                    json!({"intent":serde_json::from_str::<Value>(encoded).map_err(|e|e.to_string())?,
                    "inverse":inverse,"predecessor_parent_revision":predecessor_parent_revision}),
                )
            }
            EditOperation::MoveBeat {
                beat,
                sample_index,
                sample_rate_hz,
                ..
            } => {
                self.validate_rate(sample_rate_hz)?;
                let seconds = sample_index as f64 / f64::from(sample_rate_hz);
                if !beat.is_finite()
                    || !seconds.is_finite()
                    || sample_index >= self.inner.source_frames
                {
                    return Err("beatmap_edit_sample_outside_source".into());
                }
                let index = self
                    .inner
                    .anchors
                    .partition_point(|anchor| anchor.beat < beat);
                let anchors = value["record"]["accepted"]["anchors"]
                    .as_array_mut()
                    .ok_or("anchors_missing")?;
                let existing = self
                    .inner
                    .anchors
                    .get(index)
                    .is_some_and(|anchor| anchor.beat == beat);
                if !existing && !self.inner.at_beat(beat).supported() {
                    return Err("beatmap_edit_unsupported_beat".into());
                }
                let inverse = if existing {
                    json!({"action":"restore_anchor","index":index,"anchor":anchors[index]})
                } else {
                    json!({"action":"remove_anchor","index":index})
                };
                let mut anchor = if existing {
                    anchors[index].clone()
                } else {
                    if anchors.len() == MAX_ANCHORS {
                        return Err("beatmap_count_limit".into());
                    }
                    json!({"id":format!("edit-{}-{index}",&self.inner.revision[..16]),"beat_bits":bits(beat),"raw_beat_index":null,"review":"reviewed"})
                };
                anchor["source_seconds_bits"] = json!(bits(seconds));
                anchor["sample_edit"] = json!({"sample_index":sample_index,"sample_rate_hz":sample_rate_hz,"evidence":"Explicit source-sample positioning","author":"performer"});
                if existing {
                    anchors[index] = anchor;
                } else {
                    anchors.insert(index, anchor);
                    let intervals = value["record"]["accepted"]["intervals"]
                        .as_array_mut()
                        .ok_or("intervals_missing")?;
                    intervals.insert(index, intervals[index - 1].clone());
                }
                (
                    "source_position",
                    json!({"intent":serde_json::from_str::<Value>(encoded).map_err(|error|error.to_string())?,"inverse":inverse}),
                )
            }
            EditOperation::Translate {
                sample_offset,
                sample_rate_hz,
                ..
            } => {
                self.validate_rate(sample_rate_hz)?;
                let offset = sample_offset as f64 / f64::from(sample_rate_hz);
                let anchors = value["record"]["accepted"]["anchors"]
                    .as_array_mut()
                    .ok_or("anchors_missing")?;
                let before: Vec<Value> = anchors
                    .iter()
                    .map(|anchor| anchor["source_seconds_bits"].clone())
                    .collect();
                for anchor in anchors {
                    let previous = record::bits(
                        anchor["source_seconds_bits"]
                            .as_str()
                            .ok_or("anchor_bits_missing")?,
                    )?;
                    anchor["source_seconds_bits"] = json!(bits(previous + offset));
                }
                (
                    "whole_map_translation",
                    json!({"intent":serde_json::from_str::<Value>(encoded).map_err(|error|error.to_string())?,"inverse":{"action":"restore_positions","positions":before}}),
                )
            }
            EditOperation::Undo { .. } => {
                let raw_count = self.inner.raw_count;
                let last = undo_operation(
                    &value["record"],
                    self.inner.sample_rate_hz,
                    self.inner.source_frames,
                    raw_count,
                    self.inner.raw_index.as_ref(),
                )
                .ok_or("beatmap_no_undo")?;
                let inverse = last["payload"]["inverse"].clone();
                let undone_id = last["id"].clone();
                let kind = last["kind"].as_str().ok_or("beatmap_no_undo")?.to_owned();
                let redo = apply_inverse(&mut value["record"]["accepted"], &inverse)?;
                (
                    match kind.as_str() {
                        "whole_map_translation" => "whole_map_translation",
                        "beat_count" => "beat_count",
                        "downbeat_meter" => "downbeat_meter",
                        "coverage" => "coverage",
                        "refinement" => "refinement",
                        _ => "source_position",
                    },
                    json!({"intent":{"schema_version":1,"operation":"undo"},"undoes_operation_id":undone_id,"inverse":redo}),
                )
            }
            operation => {
                let kind = interval_edits::kind(operation.clone()).ok_or("beatmap_edit_invalid")?;
                let predecessor_parent_revision =
                    value["record"]["accepted"]["parent_revision"].clone();
                let inverse = interval_edits::apply(&mut value["record"]["accepted"], operation)?;
                (
                    kind,
                    json!({"intent":serde_json::from_str::<Value>(encoded).map_err(|error|error.to_string())?,"inverse":inverse,"predecessor_parent_revision":predecessor_parent_revision}),
                )
            }
        };
        let accepted = &mut value["record"]["accepted"];
        accepted["parent_revision"] = json!(self.inner.revision);
        let lineage = accepted["lineage"]
            .as_array_mut()
            .ok_or("lineage_missing")?;
        lineage.push(json!({"id":format!("editor-{}-{}",&self.inner.revision[..16],lineage.len()),"parent_revision":self.inner.revision,"kind":operation.0,"payload":operation.1,"evidence":"Explicit immutable editor correction; raw evidence retained","author":"performer"}));
        seal_with_context(&mut value, held, retained)
    }

    fn validate_rate(&self, rate: u32) -> Result<(), String> {
        if rate == self.inner.sample_rate_hz {
            Ok(())
        } else {
            Err("beatmap_edit_source_rate_mismatch".into())
        }
    }
}

fn exact_keys(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
    })
}

fn valid_text(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| !text.is_empty() && text.len() <= 4096)
}

fn inverse_shape(inverse: &Value, kind: &str, undo: bool) -> bool {
    let Some(action) = inverse["action"].as_str() else {
        return false;
    };
    if action == "restore_review_window" {
        return review_window::inverse_shape(inverse, kind);
    }
    if action == "restore_interval_edit" {
        return interval_edits::inverse_shape(inverse, kind);
    }
    if action == "restore_positions" {
        return kind == "whole_map_translation"
            && exact_keys(inverse, &["action", "positions"])
            && inverse["positions"].as_array().is_some_and(|positions| {
                (2..=MAX_ANCHORS).contains(&positions.len())
                    && positions.iter().all(|position| {
                        position
                            .as_str()
                            .is_some_and(|bits| record::bits(bits).is_ok())
                    })
                    && positions.windows(2).all(|pair| {
                        record::bits(pair[0].as_str().unwrap()).unwrap()
                            < record::bits(pair[1].as_str().unwrap()).unwrap()
                    })
            });
    }
    if kind != "source_position"
        || inverse["index"]
            .as_u64()
            .is_none_or(|index| index >= MAX_ANCHORS as u64)
    {
        return false;
    }
    match action {
        "remove_anchor" => exact_keys(inverse, &["action", "index"]),
        "restore_anchor" => {
            exact_keys(inverse, &["action", "index", "anchor"]) && anchor_shape(&inverse["anchor"])
        }
        "insert_anchor" => {
            undo && exact_keys(inverse, &["action", "index", "anchor", "interval"])
                && anchor_shape(&inverse["anchor"])
                && interval_shape(&inverse["interval"])
        }
        _ => false,
    }
}

pub(super) fn anchor_shape(anchor: &Value) -> bool {
    exact_keys(
        anchor,
        &[
            "id",
            "source_seconds_bits",
            "beat_bits",
            "raw_beat_index",
            "review",
            "sample_edit",
        ],
    ) && valid_text(&anchor["id"])
        && ["source_seconds_bits", "beat_bits"].iter().all(|key| {
            anchor[*key]
                .as_str()
                .is_some_and(|bits| record::bits(bits).is_ok())
        })
        && (anchor["raw_beat_index"].is_null()
            || anchor["raw_beat_index"]
                .as_u64()
                .is_some_and(|index| index < MAX_ANCHORS as u64))
        && matches!(anchor["review"].as_str(), Some("estimated" | "reviewed"))
        && (anchor["sample_edit"].is_null() || {
            let edit = &anchor["sample_edit"];
            exact_keys(
                edit,
                &["sample_index", "sample_rate_hz", "evidence", "author"],
            ) && edit["sample_index"].as_u64().is_some()
                && edit["sample_rate_hz"]
                    .as_u64()
                    .is_some_and(|rate| rate > 0 && rate <= u64::from(u32::MAX))
                && valid_text(&edit["evidence"])
                && valid_text(&edit["author"])
        })
}

pub(super) fn interval_shape(interval: &Value) -> bool {
    exact_keys(interval, &["coverage", "meter"])
        && matches!(
            interval["coverage"].as_str(),
            Some("trusted" | "uncertain" | "gap_unknown_count")
        )
        && exact_keys(
            &interval["meter"],
            &["status", "downbeat_label", "evidence"],
        )
        && matches!(
            interval["meter"]["status"].as_str(),
            Some("unknown" | "quarter_note_4_4_verified")
        )
        && (interval["meter"]["downbeat_label"].is_null()
            || interval["meter"]["downbeat_label"].as_i64().is_some())
        && valid_text(&interval["meter"]["evidence"])
}

/// Closed editor lineage is distinct from generic durable provenance. Historical
/// unknown operations remain valid records but can never become executable undo.
fn editor_operation(
    operation: &Value,
    index: usize,
    rate: u32,
    frames: u64,
) -> Option<EditOperation> {
    if !exact_keys(
        operation,
        &[
            "id",
            "parent_revision",
            "kind",
            "payload",
            "evidence",
            "author",
        ],
    ) || operation["author"] != "performer"
        || operation["evidence"] != "Explicit immutable editor correction; raw evidence retained"
    {
        return None;
    }
    let parent = operation["parent_revision"].as_str()?;
    if parent.len() != 64
        || operation["id"].as_str()? != format!("editor-{}-{index}", parent.get(..16)?)
    {
        return None;
    }
    let payload = &operation["payload"];
    let intent = parse_intent(&payload["intent"].to_string()).ok()?.operation;
    let kind = operation["kind"].as_str()?;
    let undo = matches!(intent, EditOperation::Undo { .. });
    if !exact_keys(
        payload,
        if undo {
            &["intent", "inverse", "undoes_operation_id"]
        } else if interval_edits::kind(intent.clone()).is_some()
            || matches!(intent, EditOperation::ReviewRawWindow(_))
        {
            &["intent", "inverse", "predecessor_parent_revision"]
        } else {
            &["intent", "inverse"]
        },
    ) || !inverse_shape(&payload["inverse"], kind, undo)
    {
        return None;
    }
    match &intent {
        EditOperation::ReviewRawWindow(window)
            if kind == "refinement" && window.map_revision == parent => {}
        EditOperation::MoveBeat {
            beat,
            sample_index,
            sample_rate_hz,
            ..
        } if kind == "source_position"
            && beat.is_finite()
            && *sample_index < frames
            && *sample_rate_hz == rate => {}
        EditOperation::Translate { sample_rate_hz, .. }
            if kind == "whole_map_translation" && *sample_rate_hz == rate => {}
        EditOperation::Undo { .. } if payload["undoes_operation_id"].as_str().is_some() => {}
        operation if interval_edits::kind((*operation).clone()) == Some(kind) => {}
        _ => return None,
    }
    Some(intent)
}

pub(super) fn undo_operation<'a>(
    record_value: &'a Value,
    rate: u32,
    frames: u64,
    raw_count: usize,
    raw_index: Option<&RawIndex>,
) -> Option<&'a Value> {
    let accepted = &record_value["accepted"];
    let lineage = accepted["lineage"].as_array()?;
    let ids: HashMap<&str, usize> = lineage
        .iter()
        .enumerate()
        .filter_map(|(index, operation)| operation["id"].as_str().map(|id| (id, index)))
        .collect();
    let mut undone = HashSet::new();
    for (index, operation) in lineage.iter().enumerate().rev() {
        let id = operation["id"].as_str()?;
        let intent = editor_operation(operation, index, rate, frames)?;
        if let EditOperation::Undo { .. } = intent {
            let target = operation["payload"]["undoes_operation_id"].as_str()?;
            let target_index = *ids.get(target)?;
            if target_index >= index
                || lineage[target_index]["kind"] != operation["kind"]
                || matches!(
                    editor_operation(&lineage[target_index], target_index, rate, frames)?,
                    EditOperation::Undo { .. }
                )
            {
                return None;
            }
            if !undone.insert(target) {
                return None;
            }
            continue;
        }
        if undone.contains(id) {
            continue;
        }
        let inverse = &operation["payload"]["inverse"];
        let anchors = accepted["anchors"].as_array()?;
        let mut preview = json!({"lineage":[]});
        for key in [
            "map_revision",
            "parent_revision",
            "beat_unit",
            "interpolation_policy",
            "boundary_policy",
            "snap_policy",
            "anchors",
            "intervals",
        ] {
            preview[key] = accepted[key].clone();
        }
        let interval_edit = interval_edits::kind(intent.clone()).is_some();
        let window_edit = matches!(intent, EditOperation::ReviewRawWindow(_));
        match intent {
            EditOperation::ReviewRawWindow(window) => {
                if !review_window::matches_current(
                    accepted,
                    &mut preview,
                    inverse,
                    raw_index?,
                    &window,
                    operation["parent_revision"].as_str()?,
                    (rate, frames),
                ) {
                    return None;
                }
            }
            EditOperation::MoveBeat {
                beat,
                sample_index,
                sample_rate_hz,
                ..
            } => {
                let target_index = usize::try_from(inverse["index"].as_u64()?).ok()?;
                let target = anchors.get(target_index)?;
                if record::bits(target["beat_bits"].as_str()?).ok()? != beat
                    || record::bits(target["source_seconds_bits"].as_str()?).ok()?
                        != sample_index as f64 / f64::from(sample_rate_hz)
                    || target["sample_edit"]["sample_index"].as_u64() != Some(sample_index)
                    || target["sample_edit"]["sample_rate_hz"].as_u64() != Some(u64::from(rate))
                {
                    return None;
                }
                if inverse["action"] == "restore_anchor"
                    && ["id", "beat_bits", "raw_beat_index", "review"]
                        .iter()
                        .any(|key| inverse["anchor"][*key] != target[*key])
                {
                    return None;
                }
                if inverse["action"] == "remove_anchor" {
                    let intervals = accepted["intervals"].as_array()?;
                    let parent = operation["parent_revision"].as_str()?;
                    if target_index == 0
                        || target_index + 1 >= anchors.len()
                        || intervals.get(target_index - 1)? != intervals.get(target_index)?
                        || !target["raw_beat_index"].is_null()
                        || target["review"] != "reviewed"
                        || target["id"].as_str()?
                            != format!("edit-{}-{target_index}", parent.get(..16)?)
                    {
                        return None;
                    }
                }
            }
            EditOperation::Translate {
                sample_offset,
                sample_rate_hz,
                ..
            } => {
                let before = inverse["positions"].as_array()?;
                if before.len() != anchors.len() {
                    return None;
                }
                let delta = sample_offset as f64 / f64::from(sample_rate_hz);
                for (before, anchor) in before.iter().zip(anchors) {
                    if (record::bits(before.as_str()?).ok()? + delta).to_bits()
                        != record::bits(anchor["source_seconds_bits"].as_str()?)
                            .ok()?
                            .to_bits()
                    {
                        return None;
                    }
                }
            }
            EditOperation::Undo { .. } => return None,
            operation => {
                if !interval_edits::matches_current(accepted, &mut preview, inverse, operation) {
                    return None;
                }
            }
        }
        if !interval_edit && !window_edit {
            apply_inverse(&mut preview, inverse).ok()?;
        }
        record::validate_editor_geometry(&preview, raw_count).ok()?;
        if interval_edit || window_edit {
            preview["parent_revision"] =
                operation["payload"]["predecessor_parent_revision"].clone();
            preview["lineage"] = Value::Array(lineage[..index].to_vec());
            // Generic payload JSON is not a validated predecessor. Install its
            // actual revision before checking the complete restored metadata,
            // including null-or-digest parent shape and the self-parent guard.
            preview["map_revision"] = operation["parent_revision"].clone();
            record::validate_editor_geometry(&preview, raw_count).ok()?;
            preview.as_object_mut()?.remove("map_revision");
            let predecessor = json!({"source":record_value["source"],"raw":record_value["raw"],"accepted":preview});
            check_record_size(&predecessor).ok()?;
            if descriptor_digest(&predecessor).ok()?.as_str()
                != operation["parent_revision"].as_str()?
            {
                return None;
            }
        }
        return Some(operation);
    }
    None
}

pub(super) fn apply_inverse(accepted: &mut Value, inverse: &Value) -> Result<Value, String> {
    let action = inverse["action"].as_str().ok_or("beatmap_no_undo")?;
    if action == "restore_review_window" {
        return review_window::apply_inverse(accepted, inverse);
    }
    if action == "restore_interval_edit" {
        return interval_edits::apply_inverse(accepted, inverse);
    }
    if action == "restore_positions" {
        let positions = inverse["positions"]
            .as_array()
            .ok_or("beatmap_inverse_invalid")?;
        let anchors = accepted["anchors"]
            .as_array_mut()
            .ok_or("anchors_missing")?;
        if positions.len() != anchors.len() {
            return Err("beatmap_inverse_invalid".into());
        }
        let redo: Vec<Value> = anchors
            .iter()
            .map(|anchor| anchor["source_seconds_bits"].clone())
            .collect();
        for (anchor, position) in anchors.iter_mut().zip(positions) {
            anchor["source_seconds_bits"] = position.clone();
        }
        return Ok(json!({"action":"restore_positions","positions":redo}));
    }
    let index = inverse["index"]
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .ok_or("beatmap_inverse_invalid")?;
    let anchor_count = accepted["anchors"]
        .as_array()
        .ok_or("anchors_missing")?
        .len();
    if index >= anchor_count {
        return Err("beatmap_inverse_invalid".into());
    }
    match action {
        "restore_anchor" => {
            let anchors = accepted["anchors"]
                .as_array_mut()
                .ok_or("anchors_missing")?;
            let redo = json!({"action":"restore_anchor","index":index,"anchor":anchors[index]});
            anchors[index] = inverse["anchor"].clone();
            Ok(redo)
        }
        "remove_anchor" => {
            if index == 0 || index + 1 == anchor_count {
                return Err("beatmap_inverse_invalid".into());
            }
            if accepted["intervals"]
                .as_array()
                .is_none_or(|intervals| index >= intervals.len())
            {
                return Err("beatmap_inverse_invalid".into());
            }
            let anchors = accepted["anchors"]
                .as_array_mut()
                .ok_or("anchors_missing")?;
            let anchor = anchors.remove(index);
            let intervals = accepted["intervals"]
                .as_array_mut()
                .ok_or("intervals_missing")?;
            let interval = intervals.remove(index);
            Ok(json!({"action":"insert_anchor","index":index,"anchor":anchor,"interval":interval}))
        }
        "insert_anchor" => {
            if index == 0
                || anchor_count >= MAX_ANCHORS
                || accepted["intervals"]
                    .as_array()
                    .is_none_or(|intervals| index > intervals.len())
            {
                return Err("beatmap_inverse_invalid".into());
            }
            let anchors = accepted["anchors"]
                .as_array_mut()
                .ok_or("anchors_missing")?;
            anchors.insert(index, inverse["anchor"].clone());
            accepted["intervals"]
                .as_array_mut()
                .ok_or("intervals_missing")?
                .insert(index, inverse["interval"].clone());
            Ok(json!({"action":"remove_anchor","index":index}))
        }
        _ => Err("beatmap_inverse_invalid".into()),
    }
}

pub(super) struct EditorLine {
    pub(super) seconds: f64,
    pub(super) loop_beat: f64,
    pub(super) major: bool,
    pub(super) downbeat: bool,
    pub(super) reference: bool,
    pub(super) meter: &'static str,
}
pub(super) struct EditorView {
    pub(super) status: &'static str,
    pub(super) step: f64,
    pub(super) lines: Vec<EditorLine>,
    pub(super) anchors: Vec<(f64, f64)>,
}

impl SourceBeatmap {
    pub(super) fn viewport(
        &self,
        start: f64,
        end: f64,
        loop_start: f64,
        pixels: f64,
        limit: usize,
    ) -> EditorView {
        let refused = |status| EditorView {
            status,
            step: 0.0,
            lines: Vec::new(),
            anchors: Vec::new(),
        };
        if !start.is_finite()
            || !end.is_finite()
            || !loop_start.is_finite()
            || end < start
            || !pixels.is_finite()
            || pixels <= 0.0
            || limit == 0
            || limit > MAX_PROJECTION_POINTS
        {
            return refused("invalid");
        }
        let lower = start.max(0.0).max(self.inner.anchors[0].seconds);
        let upper = end
            .min(self.inner.source_duration)
            .min(self.inner.anchors.last().unwrap().seconds);
        if upper < lower {
            return refused("outside");
        }
        let first = self
            .inner
            .anchors
            .partition_point(|anchor| anchor.seconds < lower);
        let after = self
            .inner
            .anchors
            .partition_point(|anchor| anchor.seconds <= upper);
        if after - first > limit {
            return refused("projection_limit");
        }
        let anchors: Vec<(f64, f64)> = self.inner.anchors[first..after]
            .iter()
            .map(|anchor| (anchor.seconds, anchor.beat))
            .collect();
        let reference = self.inner.at_source(loop_start);
        if !reference.supported() {
            return EditorView {
                status: "unsupported_reference",
                step: 0.0,
                lines: Vec::new(),
                anchors,
            };
        }
        let origin = reference.beat.unwrap();
        let first_segment = self
            .inner
            .anchors
            .partition_point(|anchor| anchor.seconds <= lower)
            .saturating_sub(1)
            .min(self.inner.segments.len() - 1);
        let after_segment = self
            .inner
            .anchors
            .partition_point(|anchor| anchor.seconds < upper)
            .max(first_segment + 1)
            .min(self.inner.segments.len());
        let coverage_status = if self.inner.gap_prefix[first_segment]
            != self.inner.gap_prefix[after_segment]
        {
            "gap"
        } else if self.inner.unsupported_prefix[first_segment]
            != self.inner.unsupported_prefix[after_segment]
        {
            "uncertain"
        } else if self.inner.meter_prefix[first_segment] != self.inner.meter_prefix[after_segment] {
            "unknown_meter"
        } else if self.inner.unknown_phase_prefix[first_segment]
            != self.inner.unknown_phase_prefix[after_segment]
        {
            "unknown_downbeat"
        } else {
            "complete"
        };
        let available = limit - anchors.len();
        if available == 0 {
            return refused("projection_limit");
        }
        let coordinate = |source: f64| {
            let index = self
                .inner
                .anchors
                .partition_point(|anchor| anchor.seconds <= source)
                .saturating_sub(1)
                .min(self.inner.segments.len() - 1);
            let left = self.inner.anchors[index];
            let right = self.inner.anchors[index + 1];
            left.beat
                + (source - left.seconds) / (right.seconds - left.seconds)
                    * (right.beat - left.beat)
        };
        for step in [0.0625, 0.125, 0.25, 0.5, 1.0, 4.0, 16.0] {
            let projection = if step < 4.0 {
                self.inner
                    .project(coordinate(lower), coordinate(upper), step, available)
            } else {
                self.project_visible_phase(
                    coordinate(lower),
                    coordinate(upper),
                    first_segment,
                    after_segment,
                    step,
                    available,
                )
            };
            if projection.status != "complete" {
                continue;
            }
            let mut lines = Vec::new();
            let mut previous = None;
            let mut too_close = false;
            let mut partial = false;
            for point in projection.points {
                if !point.supported() {
                    partial = true;
                    continue;
                }
                let seconds = point.source_seconds.unwrap();
                if previous.is_some_and(|before| (seconds - before) * pixels < 12.0) {
                    too_close = true;
                    if step != 16.0 {
                        break;
                    }
                }
                previous = Some(seconds);
                let beat = point.beat.unwrap();
                let loop_beat = 1.0 + beat - origin;
                let phase = point.downbeat_label.map(|label| label.rem_euclid(4) as f64);
                let downbeat = phase.is_some_and(|phase| beat.rem_euclid(4.0) == phase);
                let major = if step < 1.0 {
                    point.meter_status == "quarter_note_4_4_verified" && beat.rem_euclid(1.0) == 0.0
                } else if step == 1.0 {
                    downbeat
                } else {
                    phase.is_some_and(|phase| beat.rem_euclid(16.0) == phase)
                };
                lines.push(EditorLine {
                    seconds,
                    loop_beat,
                    major,
                    downbeat,
                    reference: loop_beat == 1.0,
                    meter: point.meter_status,
                });
            }
            if !too_close || step == 16.0 {
                return EditorView {
                    status: if partial && coverage_status == "complete" {
                        "unsupported_coverage"
                    } else {
                        coverage_status
                    },
                    step,
                    lines,
                    anchors,
                };
            }
        }
        refused("projection_limit")
    }

    /// The caller already bounds visible anchors, so at most limit+1 segments are visited.
    fn project_visible_phase(
        &self,
        first: f64,
        last: f64,
        first_segment: usize,
        after_segment: usize,
        step: f64,
        limit: usize,
    ) -> super::query::Projection {
        let refused = |status| super::query::Projection {
            status,
            points: Vec::new(),
        };
        let mut points: Vec<super::query::Evaluation> = Vec::new();
        if points.try_reserve_exact(limit).is_err() {
            return refused("allocation_refused");
        }
        for index in first_segment..after_segment {
            let segment = self.inner.segments[index];
            let Some(label) = segment.downbeat_label else {
                continue;
            };
            if segment.coverage != record::Coverage::Trusted {
                continue;
            }
            let phase = label.rem_euclid(4) as f64;
            let start = first.max(self.inner.anchors[index].beat);
            let end = last.min(self.inner.anchors[index + 1].beat);
            if end < start {
                continue;
            }
            let projection = self.inner.project_lattice(start, end, step, phase, limit);
            if projection.status != "complete" {
                return refused(projection.status);
            }
            for point in projection.points {
                if !point.supported()
                    || point
                        .downbeat_label
                        .is_none_or(|value| value.rem_euclid(4) != label.rem_euclid(4))
                {
                    continue;
                }
                if points
                    .last()
                    .is_some_and(|previous| previous.beat == point.beat)
                {
                    continue;
                }
                if points.len() == limit {
                    return refused("projection_limit");
                }
                points.push(point);
            }
        }
        super::query::Projection {
            status: "complete",
            points,
        }
    }
}
