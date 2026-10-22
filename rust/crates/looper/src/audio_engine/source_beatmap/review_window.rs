//! Bounded deterministic splices over the existing accepted-map authority.

use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;

use super::editor::{anchor_shape, interval_shape};
use super::{MAX_ANCHORS, budget, record, review_index::RawIndex};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    ExtendEnd,
    ExtendStart,
    ReplaceInterior,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewWindow {
    pub(super) schema_version: u32,
    pub(super) map_revision: String,
    pub(super) capture_revision: String,
    pub(super) mode: Mode,
    pub(super) first_anchor_index: usize,
    pub(super) last_anchor_index: usize,
    pub(super) rows: Vec<Row>,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Row {
    Raw {
        raw_index: usize,
        quarter_bits: String,
    },
    Existing {
        anchor_index: usize,
        quarter_bits: String,
    },
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ReviewWindow {
    pub(super) fn validate(&self) -> Result<(), String> {
        if !digest(&self.map_revision) || !digest(&self.capture_revision) {
            return Err("beatmap_window_revision_invalid".into());
        }
        let first = self.first_anchor_index;
        let last = self.last_anchor_index;
        if first >= MAX_ANCHORS || last >= MAX_ANCHORS {
            return Err("beatmap_window_selection_invalid".into());
        }
        match self.mode {
            Mode::ExtendEnd | Mode::ExtendStart => {
                if first != last
                    || !(1..=63).contains(&self.rows.len())
                    || self.rows.iter().any(|row| !matches!(row, Row::Raw { .. }))
                {
                    return Err("beatmap_window_extension_invalid".into());
                }
            }
            Mode::ReplaceInterior => {
                if first >= last || last - first > 63 || self.rows.len() > 62 {
                    return Err("beatmap_window_replacement_invalid".into());
                }
            }
        }
        let mut raw_ids = HashSet::new();
        let mut existing_ids = HashSet::new();
        for row in &self.rows {
            let bits = match row {
                Row::Raw {
                    raw_index,
                    quarter_bits,
                } => {
                    if *raw_index >= MAX_ANCHORS || !raw_ids.insert(*raw_index) {
                        return Err("beatmap_window_duplicate_or_invalid_raw_index".into());
                    }
                    quarter_bits
                }
                Row::Existing {
                    anchor_index,
                    quarter_bits,
                } => {
                    if *anchor_index <= first
                        || *anchor_index >= last
                        || !existing_ids.insert(*anchor_index)
                    {
                        return Err("beatmap_window_existing_outside_interior_or_duplicate".into());
                    }
                    quarter_bits
                }
            };
            record::bits(bits)?;
        }
        Ok(())
    }
}

fn uncertain_interval() -> Value {
    json!({"coverage":"uncertain", "meter":{"status":"unknown", "downbeat_label":null,
        "evidence":"Explicit review window; no meter or phase qualification supplied"}})
}

/// Execution and closed replay call this same transform. Budget the old window
/// by counting borrowed rows before cloning any provenance or growing arrays.
pub(super) fn apply(
    accepted: &mut Value,
    raw: &RawIndex,
    operation: &ReviewWindow,
    parent_revision: &str,
    rate: u32,
    frames: u64,
) -> Result<Value, String> {
    operation.validate()?;
    if operation.map_revision != parent_revision || operation.capture_revision != raw.revision {
        return Err("beatmap_window_revision_changed".into());
    }
    let first = operation.first_anchor_index;
    let last = operation.last_anchor_index;
    let anchors = accepted["anchors"].as_array().ok_or("anchors_missing")?;
    let intervals = accepted["intervals"]
        .as_array()
        .ok_or("intervals_missing")?;
    if last >= anchors.len()
        || intervals.len() + 1 != anchors.len()
        || (matches!(operation.mode, Mode::ExtendStart) && first != 0)
        || (matches!(operation.mode, Mode::ExtendEnd) && last + 1 != anchors.len())
    {
        return Err("beatmap_window_selection_invalid".into());
    }
    let before_count = last - first + 1;
    let after_count = match operation.mode {
        Mode::ReplaceInterior => operation.rows.len() + 2,
        _ => operation.rows.len() + 1,
    };
    anchors
        .len()
        .checked_sub(before_count)
        .and_then(|n| n.checked_add(after_count))
        .filter(|n| *n <= MAX_ANCHORS)
        .ok_or("beatmap_count_limit")?;
    // This count also covers IDs and complete existing sample-edit strings.
    budget::serialized_len(&(&anchors[first..=last], &intervals[first..last]))?;
    let existing_ids: HashSet<&str> = anchors.iter().filter_map(|a| a["id"].as_str()).collect();
    let mut selected_raw = HashSet::new();
    for (index, anchor) in anchors.iter().enumerate() {
        // Corrected source positions do not release their detector association.
        // Every retained row, including both fixed endpoints, owns that index.
        if (index <= first || index >= last)
            && let Some(raw_index) = anchor["raw_beat_index"].as_u64()
            && !selected_raw
                .insert(usize::try_from(raw_index).map_err(|_| "raw_anchor_association_invalid")?)
        {
            return Err("beatmap_window_duplicate_observation".into());
        }
    }
    let mut rows = Vec::new();
    rows.try_reserve_exact(after_count)
        .map_err(|e| e.to_string())?;
    if !matches!(operation.mode, Mode::ExtendStart) {
        rows.push(anchors[first].clone());
    }
    for (ordinal, row) in operation.rows.iter().enumerate() {
        let anchor = match row {
            Row::Raw {
                raw_index,
                quarter_bits,
            } => {
                let seconds = *raw
                    .seconds
                    .get(*raw_index)
                    .ok_or("beatmap_window_raw_index_outside_capture")?;
                if !seconds.is_finite()
                    || seconds < 0.0
                    || seconds >= frames as f64 / f64::from(rate)
                {
                    return Err("beatmap_window_raw_position_outside_source".into());
                }
                if !selected_raw.insert(*raw_index) {
                    return Err("beatmap_window_duplicate_observation".into());
                }
                let id = format!(
                    "window-{}-{}-{raw_index}-{ordinal}",
                    parent_revision, raw.revision
                );
                if existing_ids.contains(id.as_str()) {
                    return Err("duplicate_anchor_id".into());
                }
                json!({"id":id,"source_seconds_bits":format!("{:016x}",seconds.to_bits()),
                    "beat_bits":quarter_bits,"raw_beat_index":raw_index,"review":"estimated","sample_edit":null})
            }
            Row::Existing {
                anchor_index,
                quarter_bits,
            } => {
                let original = &anchors[*anchor_index];
                if let Some(index) = original["raw_beat_index"].as_u64()
                    && !selected_raw.insert(
                        usize::try_from(index).map_err(|_| "raw_anchor_association_invalid")?,
                    )
                {
                    return Err("beatmap_window_duplicate_observation".into());
                }
                let mut result = original.clone();
                result["beat_bits"] = json!(quarter_bits);
                result
            }
        };
        rows.push(anchor);
    }
    if !matches!(operation.mode, Mode::ExtendEnd) {
        rows.push(anchors[last].clone());
    }
    let new_intervals: Vec<Value> = (1..after_count).map(|_| uncertain_interval()).collect();
    if rows == anchors[first..=last] {
        return Err("beatmap_window_no_change".into());
    }
    // Geometry validation before mutating the real arrays retains the strict ULP
    // spacing rule and validates all new slopes, including corrected Existing rows.
    let preview = json!({"map_revision":"0".repeat(64),"parent_revision":null,
        "beat_unit":"quarter_note","interpolation_policy":"pwa-source-seconds-quarter-v1",
        "boundary_policy":"right-interior-left-final-gap-unavailable-v1","snap_policy":"nearest-quarter-subdivision-later-tie-v1",
        "anchors":rows,"intervals":new_intervals,"lineage":[]});
    record::validate_editor_geometry(&preview, raw.seconds.len())?;
    let inverse = json!({"action":"restore_review_window","first_anchor_index":first,
        "after_anchor_count":after_count,"anchors":anchors[first..=last],"intervals":intervals[first..last]});
    accepted["anchors"]
        .as_array_mut()
        .ok_or("anchors_missing")?
        .splice(first..=last, rows);
    accepted["intervals"]
        .as_array_mut()
        .ok_or("intervals_missing")?
        .splice(first..last, new_intervals);
    Ok(inverse)
}

pub(super) fn inverse_shape(value: &Value, kind: &str) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let keys = [
        "action",
        "first_anchor_index",
        "after_anchor_count",
        "anchors",
        "intervals",
    ];
    if kind != "refinement"
        || object.len() != keys.len()
        || keys.iter().any(|k| !object.contains_key(*k))
        || value["action"] != "restore_review_window"
    {
        return false;
    }
    let (Some(first), Some(after), Some(anchors), Some(intervals)) = (
        value["first_anchor_index"].as_u64(),
        value["after_anchor_count"].as_u64(),
        value["anchors"].as_array(),
        value["intervals"].as_array(),
    ) else {
        return false;
    };
    first < MAX_ANCHORS as u64
        && (1..=64).contains(&after)
        && (1..=64).contains(&anchors.len())
        && intervals.len() + 1 == anchors.len()
        && anchors.iter().all(anchor_shape)
        && intervals.iter().all(interval_shape)
}

pub(super) fn apply_inverse(accepted: &mut Value, inverse: &Value) -> Result<Value, String> {
    if !inverse_shape(inverse, "refinement") {
        return Err("beatmap_inverse_invalid".into());
    }
    let first = usize::try_from(
        inverse["first_anchor_index"]
            .as_u64()
            .ok_or("beatmap_inverse_invalid")?,
    )
    .map_err(|_| "beatmap_inverse_invalid")?;
    let after = usize::try_from(
        inverse["after_anchor_count"]
            .as_u64()
            .ok_or("beatmap_inverse_invalid")?,
    )
    .map_err(|_| "beatmap_inverse_invalid")?;
    let end = first.checked_add(after).ok_or("beatmap_inverse_invalid")?;
    let old = inverse["anchors"]
        .as_array()
        .ok_or("beatmap_inverse_invalid")?;
    let old_intervals = inverse["intervals"]
        .as_array()
        .ok_or("beatmap_inverse_invalid")?;
    let anchors = accepted["anchors"].as_array().ok_or("anchors_missing")?;
    let intervals = accepted["intervals"]
        .as_array()
        .ok_or("intervals_missing")?;
    if end > anchors.len()
        || end == 0
        || end - 1 > intervals.len()
        || anchors
            .len()
            .checked_sub(after)
            .and_then(|n| n.checked_add(old.len()))
            .is_none_or(|n| !(2..=MAX_ANCHORS).contains(&n))
    {
        return Err("beatmap_inverse_invalid".into());
    }
    budget::serialized_len(&(&anchors[first..end], &intervals[first..end - 1]))?;
    let redo = json!({"action":"restore_review_window","first_anchor_index":first,
        "after_anchor_count":old.len(),"anchors":anchors[first..end],"intervals":intervals[first..end-1]});
    accepted["anchors"]
        .as_array_mut()
        .ok_or("anchors_missing")?
        .splice(first..end, old.iter().cloned());
    accepted["intervals"]
        .as_array_mut()
        .ok_or("intervals_missing")?
        .splice(first..end - 1, old_intervals.iter().cloned());
    Ok(redo)
}

pub(super) fn matches_current(
    accepted: &Value,
    predecessor: &mut Value,
    inverse: &Value,
    raw: &RawIndex,
    operation: &ReviewWindow,
    parent: &str,
    source_extent: (u32, u64),
) -> bool {
    let (rate, frames) = source_extent;
    if apply_inverse(predecessor, inverse).is_err() {
        return false;
    }
    let Ok(rebuilt) = apply(predecessor, raw, operation, parent, rate, frames) else {
        return false;
    };
    if rebuilt != *inverse
        || predecessor["anchors"] != accepted["anchors"]
        || predecessor["intervals"] != accepted["intervals"]
    {
        return false;
    }
    apply_inverse(predecessor, inverse).is_ok()
}
