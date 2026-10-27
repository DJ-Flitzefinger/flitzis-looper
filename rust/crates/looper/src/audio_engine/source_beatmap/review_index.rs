//! Compact immutable readback derived once from validated capture/accepted data.

use super::super::source_beatmap_capture::beat_this::BeatThisRawData;
use super::super::source_beatmap_raw_data::RawData;
use super::budget;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

pub(super) struct RawIndex {
    pub(super) revision: String,
    pub(super) encoding: &'static str,
    pub(super) backend: &'static str,
    pub(super) seconds: Box<[f64]>,
    pub(super) downbeats: Box<[u8]>,
}

#[derive(Clone, Copy)]
pub(super) struct AcceptedMetadata {
    pub(super) raw_index: Option<usize>,
    pub(super) reviewed: bool,
    pub(super) sample_edit: bool,
}

impl RawIndex {
    pub(super) fn qm(raw: &RawData) -> Result<Self, String> {
        let mut seconds = Vec::new();
        let mut downbeats = Vec::new();
        seconds
            .try_reserve_exact(raw.beat_frames.len())
            .map_err(|e| e.to_string())?;
        downbeats
            .try_reserve_exact(raw.beat_frames.len())
            .map_err(|e| e.to_string())?;
        for (index, frame) in raw.beat_frames.iter().enumerate() {
            seconds.push(raw.source_seconds(*frame));
            downbeats.push(u8::from(raw.downbeat_indices.binary_search(&index).is_ok()));
        }
        Ok(Self {
            revision: raw.raw_revision.clone(),
            encoding: "source-beatmap-unreviewed-qm-raw-v1",
            backend: "legacy_qm",
            seconds: seconds.into_boxed_slice(),
            downbeats: downbeats.into_boxed_slice(),
        })
    }

    pub(super) fn beat_this(raw: &BeatThisRawData) -> Result<Self, String> {
        let mut seconds = Vec::new();
        let mut downbeats = Vec::new();
        seconds
            .try_reserve_exact(raw.event_count())
            .map_err(|e| e.to_string())?;
        downbeats
            .try_reserve_exact(raw.event_count())
            .map_err(|e| e.to_string())?;
        for index in 0..raw.event_count() {
            let value = raw
                .observation_seconds(index)
                .ok_or("capture_index_missing")?;
            seconds.push(value);
            downbeats.push(u8::from(raw.observation_downbeat(value)));
        }
        Ok(Self {
            revision: raw.capture_revision().to_owned(),
            encoding: "source-beatmap-unreviewed-beat-this-raw-v2",
            backend: "beat_this",
            seconds: seconds.into_boxed_slice(),
            downbeats: downbeats.into_boxed_slice(),
        })
    }

    pub(super) fn bytes(&self) -> Result<usize, String> {
        budget::sum(&[
            self.seconds
                .len()
                .checked_mul(8)
                .ok_or("beatmap_review_budget_overflow")?,
            self.downbeats.len(),
            self.revision.capacity(),
            size_of::<Self>(),
        ])
    }
}

pub(super) fn prospective_bytes(raw_count: usize, anchors: usize) -> Result<usize, String> {
    budget::sum(&[
        raw_count
            .checked_mul(9)
            .ok_or("beatmap_review_budget_overflow")?,
        anchors
            .checked_mul(size_of::<AcceptedMetadata>())
            .ok_or("beatmap_review_budget_overflow")?,
        size_of::<RawIndex>() + 64,
    ])
}

impl super::record::ValidatedMap {
    pub(super) fn retained_bytes(&self) -> Result<usize, String> {
        let geometry = self
            .anchors
            .len()
            .checked_mul(size_of::<super::record::Anchor>())
            .ok_or("beatmap_review_budget_overflow")?;
        let segments = self
            .segments
            .len()
            .checked_mul(size_of::<super::record::Segment>())
            .ok_or("beatmap_review_budget_overflow")?;
        let prefixes = [
            self.unsupported_prefix.len(),
            self.estimated_prefix.len(),
            self.meter_prefix.len(),
            self.gap_prefix.len(),
            self.coverage_changes.len(),
            self.meter_changes.len(),
            self.phase_changes.len(),
            self.unknown_phase_prefix.len(),
        ]
        .into_iter()
        .try_fold(0_usize, |n, len| {
            len.checked_mul(4).and_then(|bytes| n.checked_add(bytes))
        })
        .ok_or("beatmap_review_budget_overflow")?;
        budget::sum(&[
            self.encoded.len(),
            geometry,
            segments,
            prefixes,
            self.accepted_metadata
                .len()
                .checked_mul(size_of::<AcceptedMetadata>())
                .ok_or("beatmap_review_budget_overflow")?,
            self.raw_index.as_ref().map_or(Ok(0), RawIndex::bytes)?,
            self.revision.capacity(),
            self.source_identity.capacity(),
            size_of::<Self>(),
        ])
    }
}

fn header(result: &Bound<'_, PyDict>, map: &super::SourceBeatmap) -> PyResult<()> {
    let raw = map.inner.raw_index.as_ref();
    result.set_item(
        "status",
        if raw.is_some() {
            "ready"
        } else {
            "unavailable"
        },
    )?;
    result.set_item(
        "reason",
        if raw.is_some() {
            None
        } else {
            Some("Complete embedded capture is unavailable for this legacy map")
        },
    )?;
    result.set_item("map_revision", &map.inner.revision)?;
    result.set_item("source_identity", &map.inner.source_identity)?;
    result.set_item("capture_revision", raw.map(|r| r.revision.as_str()))?;
    Ok(())
}

pub(super) fn info(map: &super::SourceBeatmap, py: Python<'_>) -> PyResult<Py<PyDict>> {
    let result = PyDict::new(py);
    header(&result, map)?;
    let raw = map.inner.raw_index.as_ref();
    result.set_item("capture_encoding", raw.map(|r| r.encoding))?;
    result.set_item("backend", raw.map(|r| r.backend))?;
    result.set_item("event_count", raw.map_or(0, |r| r.seconds.len()))?;
    result.set_item("anchor_count", map.inner.anchors.len())?;
    result.set_item("source_sample_rate_hz", map.inner.sample_rate_hz)?;
    result.set_item("source_frame_count", map.inner.source_frames)?;
    Ok(result.unbind())
}

pub(super) fn page(
    map: &super::SourceBeatmap,
    py: Python<'_>,
    first: i64,
    last: i64,
    start: i64,
    maximum: i64,
) -> PyResult<Py<PyDict>> {
    let (first, last) = usize::try_from(first)
        .ok()
        .zip(usize::try_from(last).ok())
        .filter(|(first, last)| {
            first <= last && *last < map.inner.anchors.len() && last - first < 64
        })
        .ok_or_else(|| {
            PyValueError::new_err("review accepted range must contain 1..=64 current anchors")
        })?;
    let maximum = usize::try_from(maximum)
        .ok()
        .filter(|n| (1..=128).contains(n))
        .ok_or_else(|| PyValueError::new_err("review max_events must be within 1..=128"))?;
    let raw = map.inner.raw_index.as_ref();
    let count = raw.map_or(0, |r| r.seconds.len());
    let start = usize::try_from(start)
        .ok()
        .filter(|n| *n <= count)
        .ok_or_else(|| {
            PyValueError::new_err("review start_index must be within 0..=event_count")
        })?;
    let end = start
        .checked_add(maximum)
        .ok_or_else(|| PyValueError::new_err("review page range overflow"))?
        .min(count);
    let result = PyDict::new(py);
    header(&result, map)?;
    result.set_item("first_anchor_index", first)?;
    result.set_item("last_anchor_index", last)?;
    result.set_item("start_index", start)?;
    result.set_item("returned_count", end - start)?;
    result.set_item("total_event_count", count)?;
    result.set_item("has_more", raw.is_some() && end < count)?;
    let anchors = PyList::empty(py);
    let events = PyList::empty(py);
    if let Some(raw) = raw {
        for index in first..=last {
            let anchor = map.inner.anchors[index];
            let meta = map.inner.accepted_metadata[index];
            let row = PyDict::new(py);
            row.set_item("index", index)?;
            row.set_item("source_seconds", anchor.seconds)?;
            row.set_item(
                "source_seconds_bits",
                format!("{:016x}", anchor.seconds.to_bits()),
            )?;
            row.set_item("beat", anchor.beat)?;
            row.set_item("beat_bits", format!("{:016x}", anchor.beat.to_bits()))?;
            row.set_item("raw_beat_index", meta.raw_index)?;
            row.set_item(
                "review",
                if meta.reviewed {
                    "reviewed"
                } else {
                    "estimated"
                },
            )?;
            row.set_item("sample_index", map.inner.sample_index(anchor.seconds))?;
            row.set_item("has_sample_edit", meta.sample_edit)?;
            anchors.append(row)?;
        }
        for index in start..end {
            let seconds = raw.seconds[index];
            let row = PyDict::new(py);
            row.set_item("raw_index", index)?;
            row.set_item("source_seconds", seconds)?;
            row.set_item("source_seconds_bits", format!("{:016x}", seconds.to_bits()))?;
            row.set_item("detected_downbeat", raw.downbeats[index] != 0)?;
            row.set_item(
                "physical_domain",
                if seconds < map.inner.source_duration {
                    "inside"
                } else if seconds == map.inner.source_duration {
                    "exclusive_end"
                } else {
                    "outside"
                },
            )?;
            events.append(row)?;
        }
    }
    result.set_item("anchors", anchors)?;
    result.set_item("events", events)?;
    Ok(result.unbind())
}
