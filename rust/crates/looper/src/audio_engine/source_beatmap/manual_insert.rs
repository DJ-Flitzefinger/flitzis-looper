//! Explicit sample evidence for one detector-missed beat, entirely off realtime.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{MAX_ANCHORS, budget, record, review_index::RawIndex, review_window};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    Interior,
    ExtendStart,
    ExtendEnd,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ManualInsert {
    pub(super) schema_version: u32,
    pub(super) map_revision: String,
    pub(super) capture_revision: String,
    pub(super) mode: Mode,
    pub(super) first_anchor_index: usize,
    pub(super) last_anchor_index: usize,
    pub(super) quarter_bits: String,
    pub(super) sample_index: u64,
    pub(super) sample_rate_hz: u32,
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ManualInsert {
    pub(super) fn validate(&self) -> Result<(), String> {
        if !digest(&self.map_revision) || !digest(&self.capture_revision) {
            return Err("beatmap_manual_revision_invalid".into());
        }
        record::bits(&self.quarter_bits)?;
        let first = self.first_anchor_index;
        let last = self.last_anchor_index;
        if first >= MAX_ANCHORS
            || last >= MAX_ANCHORS
            || match self.mode {
                Mode::Interior => first.checked_add(1) != Some(last),
                Mode::ExtendStart | Mode::ExtendEnd => first != last,
            }
        {
            return Err("beatmap_manual_selection_invalid".into());
        }
        if self.sample_rate_hz == 0 {
            return Err("beatmap_edit_source_rate_mismatch".into());
        }
        Ok(())
    }
}

fn after_count(before: usize) -> Result<usize, String> {
    before
        .checked_add(1)
        .filter(|count| *count <= MAX_ANCHORS)
        .ok_or_else(|| "beatmap_count_limit".into())
}

/// Execution and closed Undo replay use the same transform. The outer editor
/// admits the complete old/new tree, retained index and held source before this
/// bounded splice borrows/counts and then clones any predecessor provenance.
pub(super) fn apply(
    accepted: &mut Value,
    raw: &RawIndex,
    operation: &ManualInsert,
    parent_revision: &str,
    rate: u32,
    frames: u64,
) -> Result<Value, String> {
    operation.validate()?;
    if operation.map_revision != parent_revision || operation.capture_revision != raw.revision {
        return Err("beatmap_manual_revision_changed".into());
    }
    if operation.sample_rate_hz != rate {
        return Err("beatmap_edit_source_rate_mismatch".into());
    }
    if operation.sample_index >= frames {
        return Err("beatmap_edit_sample_outside_source".into());
    }
    let seconds = operation.sample_index as f64 / f64::from(rate);
    if !seconds.is_finite() {
        return Err("beatmap_edit_sample_outside_source".into());
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
        return Err("beatmap_manual_selection_invalid".into());
    }
    after_count(anchors.len())?;
    budget::serialized_len(&(&anchors[first..=last], &intervals[first..last]))?;
    let id = format!(
        "manual-{}-{}-{}-{}",
        parent_revision, raw.revision, operation.sample_index, operation.quarter_bits
    );
    if anchors
        .iter()
        .any(|anchor| anchor["id"].as_str() == Some(id.as_str()))
    {
        return Err("duplicate_anchor_id".into());
    }
    let anchor = json!({"id":id,"source_seconds_bits":format!("{:016x}",seconds.to_bits()),
        "beat_bits":operation.quarter_bits,"raw_beat_index":null,"review":"reviewed",
        "sample_edit":{"sample_index":operation.sample_index,"sample_rate_hz":rate,
            "evidence":"Explicit detector-missed beat source-sample positioning","author":"performer"}});
    let after_count = last - first + 2;
    let mut rows = Vec::new();
    rows.try_reserve_exact(after_count)
        .map_err(|error| error.to_string())?;
    if !matches!(operation.mode, Mode::ExtendStart) {
        rows.push(anchors[first].clone());
    }
    rows.push(anchor);
    if !matches!(operation.mode, Mode::ExtendEnd) {
        rows.push(anchors[last].clone());
    }
    let new_intervals: Vec<Value> = (1..after_count)
        .map(|_| {
            json!({"coverage":"uncertain","meter":{"status":"unknown","downbeat_label":null,
            "evidence":"Explicit detector-missed beat; no meter or phase qualification supplied"}})
        })
        .collect();
    let preview = json!({"map_revision":"0".repeat(64),"parent_revision":null,
        "beat_unit":"quarter_note","interpolation_policy":"pwa-source-seconds-quarter-v1",
        "boundary_policy":"right-interior-left-final-gap-unavailable-v1","snap_policy":"nearest-quarter-subdivision-later-tie-v1",
        "anchors":rows,"intervals":new_intervals,"lineage":[]});
    // The real geometry validator enforces direction, both strict axes, 16 ULP
    // spacing and finite slopes, without a trusted interpolation prerequisite.
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

pub(super) fn matches_current(
    accepted: &Value,
    predecessor: &mut Value,
    inverse: &Value,
    raw: &RawIndex,
    operation: &ManualInsert,
    parent: &str,
    source_extent: (u32, u64),
) -> bool {
    let (rate, frames) = source_extent;
    if review_window::apply_inverse(predecessor, inverse).is_err() {
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
    review_window::apply_inverse(predecessor, inverse).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_insert_anchor_cap_and_overflow_refuse_without_large_allocations() {
        assert_eq!(after_count(MAX_ANCHORS - 1).unwrap(), MAX_ANCHORS);
        assert_eq!(after_count(MAX_ANCHORS).unwrap_err(), "beatmap_count_limit");
        assert_eq!(after_count(usize::MAX).unwrap_err(), "beatmap_count_limit");
    }
}
