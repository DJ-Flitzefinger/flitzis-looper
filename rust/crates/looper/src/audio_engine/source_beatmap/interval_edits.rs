//! Cold interval corrections and their closed, exact inverses.

use serde_json::{Value, json};

use super::MAX_ANCHORS;
use super::editor::{EditOperation, interval_shape};
use super::record::{self, Coverage, MeterStatus};

pub(super) fn kind(operation: EditOperation) -> Option<&'static str> {
    match operation {
        EditOperation::SetCount { .. } => Some("beat_count"),
        EditOperation::SetMeter { .. } | EditOperation::SetDownbeat { .. } => {
            Some("downbeat_meter")
        }
        EditOperation::SetCoverage { .. } => Some("coverage"),
        _ => None,
    }
}

fn range(operation: EditOperation) -> Option<(usize, usize)> {
    match operation {
        EditOperation::SetCount {
            first_anchor_index,
            last_anchor_index,
            ..
        }
        | EditOperation::SetMeter {
            first_anchor_index,
            last_anchor_index,
            ..
        }
        | EditOperation::SetDownbeat {
            first_anchor_index,
            last_anchor_index,
            ..
        }
        | EditOperation::SetCoverage {
            first_anchor_index,
            last_anchor_index,
            ..
        } => Some((first_anchor_index, last_anchor_index)),
        _ => None,
    }
}

fn anchor_beat(anchor: &Value) -> Result<f64, String> {
    record::bits(anchor["beat_bits"].as_str().ok_or("anchor_bits_missing")?)
}

fn encoded_bits(value: f64) -> Value {
    json!(format!("{:016x}", value.to_bits()))
}

/// The same deterministic transform serves execution and closed Undo correspondence.
pub(super) fn apply(accepted: &mut Value, operation: EditOperation) -> Result<Value, String> {
    let (first, last) = range(operation.clone()).ok_or("beatmap_edit_invalid")?;
    let anchors = accepted["anchors"].as_array().ok_or("anchors_missing")?;
    if first >= last || last >= anchors.len() {
        return Err("beatmap_edit_invalid_selection".into());
    }
    let left = anchor_beat(&anchors[first])?;
    let old_count = anchor_beat(&anchors[last])? - left;
    if !old_count.is_finite() || old_count <= 0.0 {
        return Err("beatmap_edit_count_invalid".into());
    }
    let count_edit = matches!(operation, EditOperation::SetCount { .. });
    let interval_end = if count_edit { anchors.len() - 1 } else { last };
    let before_beats: Vec<Value> = if count_edit {
        anchors[first..]
            .iter()
            .map(|anchor| anchor["beat_bits"].clone())
            .collect()
    } else {
        Vec::new()
    };
    let intervals = accepted["intervals"]
        .as_array()
        .ok_or("intervals_missing")?;
    if interval_end > intervals.len() {
        return Err("beatmap_edit_invalid_selection".into());
    }
    let before_intervals = intervals[first..interval_end].to_vec();
    let inverse = json!({
        "action":"restore_interval_edit", "first_anchor_index":first,
        "last_anchor_index":last, "beat_bits":before_beats,
        "intervals":before_intervals,
        "count_phase_policy":if count_edit { json!("selected-clear-suffix-integral-mod-four-v1") } else { Value::Null },
    });
    match operation {
        EditOperation::SetCount { quarter_notes, .. } => {
            if !quarter_notes.is_finite() || quarter_notes <= 0.0 {
                return Err("beatmap_edit_count_invalid".into());
            }
            if quarter_notes != old_count {
                let delta = quarter_notes - old_count;
                let anchors = accepted["anchors"]
                    .as_array_mut()
                    .ok_or("anchors_missing")?;
                for (index, anchor) in anchors.iter_mut().enumerate().skip(first + 1) {
                    let before = anchor_beat(anchor)?;
                    let after = if index < last {
                        left + (before - left) * quarter_notes / old_count
                    } else if index == last {
                        left + quarter_notes
                    } else {
                        before + delta
                    };
                    if !after.is_finite() {
                        return Err("beatmap_edit_count_invalid".into());
                    }
                    anchor["beat_bits"] = encoded_bits(after);
                }
                let intervals = accepted["intervals"]
                    .as_array_mut()
                    .ok_or("intervals_missing")?;
                for (index, interval) in intervals.iter_mut().enumerate().skip(first) {
                    let label = (interval["meter"]["status"] == "quarter_note_4_4_verified")
                        .then(|| interval["meter"]["downbeat_label"].as_i64())
                        .flatten();
                    interval["meter"]["downbeat_label"] = if index < last || delta.fract() != 0.0 {
                        Value::Null
                    } else {
                        label.map_or(Value::Null, |label| {
                            // Reduce the integer before any float conversion, including i64::MAX.
                            json!(
                                (label.rem_euclid(4) + delta.rem_euclid(4.0) as i64).rem_euclid(4)
                            )
                        })
                    };
                }
            }
        }
        EditOperation::SetMeter { meter_status, .. } => {
            let target = match meter_status {
                MeterStatus::Unknown => "unknown",
                MeterStatus::QuarterNote4_4Verified => "quarter_note_4_4_verified",
            };
            for interval in accepted["intervals"]
                .as_array_mut()
                .ok_or("intervals_missing")?
                .iter_mut()
                .take(last)
                .skip(first)
            {
                if target == "unknown" || interval["meter"]["status"] != target {
                    interval["meter"]["downbeat_label"] = Value::Null;
                }
                interval["meter"]["status"] = json!(target);
            }
        }
        EditOperation::SetDownbeat { downbeat_label, .. } => {
            let intervals = accepted["intervals"]
                .as_array_mut()
                .ok_or("intervals_missing")?;
            if downbeat_label.is_some()
                && intervals[first..last]
                    .iter()
                    .any(|interval| interval["meter"]["status"] != "quarter_note_4_4_verified")
            {
                return Err("beatmap_edit_downbeat_requires_meter".into());
            }
            for interval in &mut intervals[first..last] {
                interval["meter"]["downbeat_label"] = json!(downbeat_label);
            }
        }
        EditOperation::SetCoverage {
            coverage,
            reviewed_quarter_notes,
            ..
        } => {
            if coverage == Coverage::Estimated {
                return Err("estimated_coverage_is_constructor_owned".into());
            }
            if coverage != Coverage::GapUnknownCount
                && reviewed_quarter_notes
                    .is_none_or(|count| !count.is_finite() || count <= 0.0 || count != old_count)
            {
                return Err("beatmap_edit_count_assertion_required".into());
            }
            for interval in accepted["intervals"]
                .as_array_mut()
                .ok_or("intervals_missing")?
                .iter_mut()
                .take(last)
                .skip(first)
            {
                interval["coverage"] = json!(coverage.label());
            }
        }
        _ => return Err("beatmap_edit_invalid".into()),
    }
    Ok(inverse)
}

pub(super) fn inverse_shape(inverse: &Value, kind: &str) -> bool {
    let Some(object) = inverse.as_object() else {
        return false;
    };
    let keys = [
        "action",
        "first_anchor_index",
        "last_anchor_index",
        "beat_bits",
        "intervals",
        "count_phase_policy",
    ];
    if object.len() != keys.len()
        || keys.iter().any(|key| !object.contains_key(*key))
        || inverse["action"] != "restore_interval_edit"
    {
        return false;
    }
    let (Some(first), Some(last), Some(beats), Some(intervals)) = (
        inverse["first_anchor_index"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok()),
        inverse["last_anchor_index"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok()),
        inverse["beat_bits"].as_array(),
        inverse["intervals"].as_array(),
    ) else {
        return false;
    };
    if first >= last
        || last >= MAX_ANCHORS
        || intervals.len() > MAX_ANCHORS
        || !intervals.iter().all(interval_shape)
    {
        return false;
    }
    if kind == "beat_count" {
        inverse["count_phase_policy"] == "selected-clear-suffix-integral-mod-four-v1"
            && beats.len() > last - first
            && beats.len() <= MAX_ANCHORS - first
            && intervals.len() + 1 == beats.len()
            && beats.iter().all(|value| {
                value
                    .as_str()
                    .is_some_and(|bits| record::bits(bits).is_ok())
            })
    } else {
        matches!(kind, "downbeat_meter" | "coverage")
            && beats.is_empty()
            && intervals.len() == last - first
            && inverse["count_phase_policy"].is_null()
    }
}

pub(super) fn apply_inverse(accepted: &mut Value, inverse: &Value) -> Result<Value, String> {
    let first = inverse["first_anchor_index"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or("beatmap_inverse_invalid")?;
    let last = inverse["last_anchor_index"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or("beatmap_inverse_invalid")?;
    let beats = inverse["beat_bits"]
        .as_array()
        .ok_or("beatmap_inverse_invalid")?;
    let intervals = inverse["intervals"]
        .as_array()
        .ok_or("beatmap_inverse_invalid")?;
    let count_edit = !beats.is_empty();
    let inferred_kind = if count_edit {
        "beat_count"
    } else {
        "downbeat_meter"
    };
    if !inverse_shape(inverse, inferred_kind) {
        return Err("beatmap_inverse_invalid".into());
    }
    let anchors = accepted["anchors"].as_array().ok_or("anchors_missing")?;
    let current_intervals = accepted["intervals"]
        .as_array()
        .ok_or("intervals_missing")?;
    let interval_end = first
        .checked_add(intervals.len())
        .ok_or("beatmap_inverse_invalid")?;
    if last >= anchors.len()
        || interval_end > current_intervals.len()
        || (count_edit && beats.len() != anchors.len() - first)
    {
        return Err("beatmap_inverse_invalid".into());
    }
    let redo_beats: Vec<Value> = if count_edit {
        anchors[first..]
            .iter()
            .map(|anchor| anchor["beat_bits"].clone())
            .collect()
    } else {
        Vec::new()
    };
    let redo = json!({"action":"restore_interval_edit","first_anchor_index":first,
        "last_anchor_index":last,"beat_bits":redo_beats,
        "intervals":current_intervals[first..interval_end],"count_phase_policy":inverse["count_phase_policy"]});
    if count_edit {
        for (anchor, beat) in accepted["anchors"]
            .as_array_mut()
            .ok_or("anchors_missing")?[first..]
            .iter_mut()
            .zip(beats)
        {
            anchor["beat_bits"] = beat.clone();
        }
    }
    accepted["intervals"]
        .as_array_mut()
        .ok_or("intervals_missing")?[first..interval_end]
        .clone_from_slice(intervals);
    Ok(redo)
}

pub(super) fn matches_current(
    accepted: &Value,
    predecessor: &mut Value,
    inverse: &Value,
    operation: EditOperation,
) -> bool {
    let Some((first, last)) = range(operation.clone()) else {
        return false;
    };
    if inverse["first_anchor_index"].as_u64() != Some(first as u64)
        || inverse["last_anchor_index"].as_u64() != Some(last as u64)
    {
        return false;
    }
    if apply_inverse(predecessor, inverse).is_err() {
        return false;
    }
    let Ok(rebuilt_inverse) = apply(predecessor, operation) else {
        return false;
    };
    if rebuilt_inverse != *inverse
        || predecessor["anchors"] != accepted["anchors"]
        || predecessor["intervals"] != accepted["intervals"]
    {
        return false;
    }
    apply_inverse(predecessor, inverse).is_ok()
}
