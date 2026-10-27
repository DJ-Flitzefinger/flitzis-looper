//! Ordered loader delivery across short source-placement publications.
//!
//! The private dequeue seam is monomorphized with an empty closure in production. Tests use
//! the same channel/pop/filter path to hold a real source writer between dequeue and lookup.

use super::content_placement::PlacementObservation;
use super::{AudioEngine, resident_relocation, task_to_str};
use crate::messages::{BackgroundTaskKind, LoaderEvent};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::TryRecvError;

/// One-shot export state belonging to a genuine automatic analysis terminal.
#[derive(Clone, Debug)]
struct AutomaticMapDelivery {
    consumed: Arc<AtomicBool>,
    cold: bool,
    /// Only the separately exported delivery may expose map custody. Sharing
    /// the consumed flag must never authorize the original analysis delivery.
    exported: bool,
}

/// Retained Native event custody survives a Python participant-observer fence.
#[pyclass(frozen)]
#[derive(Clone, Debug)]
pub struct LoaderEventDelivery {
    owner: super::complete_context::NativeAnalysisOwner,
    timing_epoch: Option<u64>,
    source_beatmap: Option<super::source_beatmap::SourceBeatmap>,
    source_beatmap_artifact: Option<super::source_beatmap_artifact::SourceBeatmapArtifact>,
    source_beatmap_raw: Option<super::source_beatmap_artifact::SourceBeatmapRawArtifact>,
    automatic_map: Option<AutomaticMapDelivery>,
    automatic_map_error: Option<String>,
}

impl LoaderEventDelivery {
    fn map_access_authorized(&self) -> bool {
        self.automatic_map.as_ref().is_none_or(|automatic| {
            automatic.exported && automatic.consumed.load(Ordering::Acquire)
        })
    }

    /// Effective immutable map custody keeps its genuine original Native owner after admission.
    /// Later job cancellation revokes pending work, not this comparison-only provenance.
    pub(super) fn source_map_belongs_to(
        &self,
        ownership: &std::sync::Arc<super::input_runtime_binding::InputRuntimeOwnership>,
    ) -> bool {
        self.map_access_authorized() && self.owner.belongs_to(ownership)
    }

    pub(super) fn source_beatmap_raw_for(
        &self,
        owner: &super::complete_context::NativeAnalysisOwner,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> PyResult<Option<super::source_beatmap_raw::SourceBeatmapRawCandidate>> {
        if !self.map_access_authorized() || !owner.same_admission(&self.owner) || !owner.current() {
            return Ok(None);
        }
        let Some(artifact) = self.source_beatmap_raw.as_ref() else {
            return Ok(None);
        };
        match self.owner.observe_control() {
            PlacementObservation::Current(_) if owner.current() => Ok(Some(
                super::source_beatmap_raw::SourceBeatmapRawCandidate::new(
                    artifact.clone(),
                    owner.clone(),
                    shutdown,
                ),
            )),
            PlacementObservation::Pending => Err(PyRuntimeError::new_err(
                "loader event content placement pending",
            )),
            _ => Ok(None),
        }
    }
    pub(super) fn source_map_binding_current(
        &self,
        ownership: &std::sync::Arc<super::input_runtime_binding::InputRuntimeOwnership>,
        binding: super::input_runtime_binding::InputPadBinding,
    ) -> bool {
        self.map_access_authorized() && self.owner.source_map_binding_current(ownership, binding)
    }

    /// Called under the existing request lock before a runtime publication is staged.
    pub(super) fn qualify_source_map(
        &self,
        binding: &super::input_runtime_binding::InputRuntimePadBinding,
        exact_quarters: Option<f64>,
    ) -> Result<super::source_map_admission::QualifiedSourceMap, String> {
        if !self.map_access_authorized() {
            return Err("automatic source map requires its one-shot exported delivery".into());
        }
        if !self.owner.source_map_admission_current(binding) {
            return Err(
                "source map original Native admission is stale or belongs to another source".into(),
            );
        }
        let current = match self.owner.observe_control() {
            PlacementObservation::Current(current) => current,
            PlacementObservation::Pending => {
                return Err("source map content placement is pending".into());
            }
            PlacementObservation::Stale | PlacementObservation::Retired => {
                return Err("source map content placement changed".into());
            }
        };
        if !binding
            .binding
            .placement
            .is_some_and(|placement| placement.same_projection(current))
        {
            return Err("source map does not belong to the original runtime row".into());
        }
        let map = self
            .source_beatmap
            .clone()
            .ok_or("source map delivery has no genuine map")?;
        let artifact = self
            .source_beatmap_artifact
            .clone()
            .ok_or("source map delivery has no artifact custody")?;
        if !self.owner.source_map_admission_current(binding) {
            return Err("source map original Native admission changed during qualification".into());
        }
        super::source_map_admission::QualifiedSourceMap::new(
            self.clone(),
            map,
            artifact,
            binding,
            exact_quarters,
        )
    }

    pub(super) fn source_beatmap_artifact_for(
        &self,
        owner: &super::complete_context::NativeAnalysisOwner,
    ) -> PyResult<Option<super::source_beatmap_artifact::SourceBeatmapArtifact>> {
        if !self.map_access_authorized() || !owner.same_admission(&self.owner) || !owner.current() {
            return Ok(None);
        }
        let Some(artifact) = self.source_beatmap_artifact.as_ref() else {
            return Ok(None);
        };
        match self.owner.observe_control() {
            PlacementObservation::Current(_) => {
                // This final successful currentness check is the ACK
                // linearization point. Concurrent cancellation afterwards
                // follows valid resolution; cancellation before it refuses.
                if !owner.current() {
                    return Ok(None);
                }
                artifact.acknowledge_export();
                Ok(Some(artifact.clone()))
            }
            PlacementObservation::Pending => Err(PyRuntimeError::new_err(
                "loader event content placement pending",
            )),
            PlacementObservation::Stale | PlacementObservation::Retired => Ok(None),
        }
    }

    /// Original-job data resolution never consults mutable Python DTO fields.
    pub(super) fn source_beatmap_for(
        &self,
        owner: &super::complete_context::NativeAnalysisOwner,
    ) -> PyResult<Option<super::source_beatmap::SourceBeatmap>> {
        if !self.map_access_authorized() || !owner.same_admission(&self.owner) || !owner.current() {
            return Ok(None);
        }
        let Some(map) = self.source_beatmap.as_ref() else {
            return Ok(None);
        };
        match self.owner.observe_control() {
            PlacementObservation::Current(_) => Ok(Some(map.clone())),
            PlacementObservation::Pending => Err(PyRuntimeError::new_err(
                "loader event content placement pending",
            )),
            PlacementObservation::Stale | PlacementObservation::Retired => Ok(None),
        }
    }
}

#[pymethods]
impl LoaderEventDelivery {
    /// Compare immutable original admissions solely for bounded progress coalescing.
    pub fn same_owner(&self, other: &Self) -> bool {
        self.owner.same_admission(&other.owner)
    }

    /// Historical optional outcome only; never current playback qualification.
    pub fn automatic_source_beatmap_error(&self) -> Option<&str> {
        self.automatic_map_error.as_deref()
    }
}

/// Export one genuine automatic result without recapturing current source custody.
/// Fallible Python wrappers precede the final locked checks and one-shot export.
pub(super) fn consume_automatic_map(
    engine: &AudioEngine,
    py: Python<'_>,
    delivery: &LoaderEventDelivery,
) -> PyResult<Option<Py<PyTuple>>> {
    let Some(automatic) = &delivery.automatic_map else {
        return Ok(None);
    };
    let Some(artifact) = &delivery.source_beatmap_artifact else {
        return Ok(None);
    };
    if delivery.source_beatmap.is_none()
        || !delivery.owner.belongs_to(&engine.input_runtime_ownership)
        || !delivery.owner.current()
        || automatic.consumed.load(Ordering::Acquire)
    {
        return Ok(None);
    }
    let initial = match delivery.owner.observe_control() {
        PlacementObservation::Current(snapshot) => snapshot,
        PlacementObservation::Pending => {
            return Err(PyRuntimeError::new_err(
                "loader event content placement pending",
            ));
        }
        _ => return Ok(None),
    };
    let derived = LoaderEventDelivery {
        owner: delivery.owner.source_only(),
        timing_epoch: None,
        source_beatmap: delivery.source_beatmap.clone(),
        source_beatmap_artifact: Some(artifact.clone()),
        source_beatmap_raw: None,
        automatic_map: Some(AutomaticMapDelivery {
            exported: true,
            ..automatic.clone()
        }),
        automatic_map_error: None,
    };
    let result = (initial.slot(), artifact.clone(), derived)
        .into_pyobject(py)?
        .unbind();
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    if !delivery.owner.belongs_to(&engine.input_runtime_ownership) || !delivery.owner.current() {
        return Ok(None);
    }
    let current = match delivery.owner.observe_control() {
        PlacementObservation::Current(snapshot) if snapshot.same_projection(initial) => snapshot,
        PlacementObservation::Current(_) => {
            return Err(PyRuntimeError::new_err(
                "loader event content placement pending: export target changed",
            ));
        }
        PlacementObservation::Pending => {
            return Err(PyRuntimeError::new_err(
                "loader event content placement pending",
            ));
        }
        _ => return Ok(None),
    };
    if automatic.cold {
        let Some(expected) = delivery.timing_epoch else {
            return Ok(None);
        };
        if engine
            .prepared_source_epoch(current.slot())
            .map_err(PyRuntimeError::new_err)?
            .load(Ordering::Acquire)
            != expected
        {
            return Ok(None);
        }
    } else if !delivery.owner.has_analysis_epoch() {
        return Ok(None);
    }
    // Original strict admission is checked immediately before the shared claim.
    if !delivery.owner.current()
        || !matches!(delivery.owner.observe_control(),
        PlacementObservation::Current(snapshot) if snapshot.same_projection(current))
    {
        return Ok(None);
    }
    if automatic
        .consumed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(None);
    }
    // Cancellation need not take the request lock. A concurrent cancellation
    // retires this one-shot result; it never reopens the claim for replay.
    if delivery.owner.cancellation_requested() {
        artifact.retire_unselected();
        return Ok(None);
    }
    artifact.acknowledge_export();
    Ok(Some(result))
}

pub(super) fn resolve_delivery(
    engine: &AudioEngine,
    delivery: &LoaderEventDelivery,
) -> PyResult<Option<(usize, bool)>> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    if !delivery.owner.belongs_to(&engine.input_runtime_ownership) {
        return Ok(None);
    }
    match delivery.owner.observe_control() {
        PlacementObservation::Current(snapshot) => {
            let id = snapshot.slot();
            let epoch = engine
                .prepared_source_epoch(id)
                .map_err(PyRuntimeError::new_err)?;
            Ok(Some((
                id,
                // A map-only export proves source custody, never fresh timing
                // completion. Its original analysis delivery keeps that role.
                delivery
                    .automatic_map
                    .as_ref()
                    .is_some_and(|automatic| automatic.exported)
                    || delivery
                        .timing_epoch
                        .is_some_and(|expected| epoch.load(Ordering::Acquire) != expected),
            )))
        }
        PlacementObservation::Pending => Err(PyRuntimeError::new_err(
            "loader event content placement pending",
        )),
        PlacementObservation::Stale | PlacementObservation::Retired => Ok(None),
    }
}

fn identity(event: &LoaderEvent) -> (usize, u64) {
    match event {
        LoaderEvent::NativeOwned { event, .. } => identity(event),
        LoaderEvent::Started { id, request_id }
        | LoaderEvent::Progress { id, request_id, .. }
        | LoaderEvent::Success { id, request_id, .. }
        | LoaderEvent::Error { id, request_id, .. }
        | LoaderEvent::TaskStarted { id, request_id, .. }
        | LoaderEvent::TaskProgress { id, request_id, .. }
        | LoaderEvent::TaskSuccess { id, request_id, .. }
        | LoaderEvent::TaskError { id, request_id, .. }
        | LoaderEvent::OfflineAnalysisCompleted { id, request_id, .. }
        | LoaderEvent::SourceBeatmapCompleted { id, request_id, .. }
        | LoaderEvent::SourceBeatmapRawCompleted { id, request_id, .. } => (*id, *request_id),
    }
}

fn payload(event: &mut LoaderEvent) -> &mut LoaderEvent {
    match event {
        LoaderEvent::NativeOwned { event, .. } => payload(event),
        event => event,
    }
}

fn project_slot(event: &mut LoaderEvent, slot: usize) {
    match payload(event) {
        LoaderEvent::Started { id, .. }
        | LoaderEvent::Progress { id, .. }
        | LoaderEvent::Success { id, .. }
        | LoaderEvent::Error { id, .. }
        | LoaderEvent::TaskStarted { id, .. }
        | LoaderEvent::TaskProgress { id, .. }
        | LoaderEvent::TaskSuccess { id, .. }
        | LoaderEvent::TaskError { id, .. }
        | LoaderEvent::OfflineAnalysisCompleted { id, .. }
        | LoaderEvent::SourceBeatmapCompleted { id, .. }
        | LoaderEvent::SourceBeatmapRawCompleted { id, .. } => *id = slot,
        LoaderEvent::NativeOwned { .. } => unreachable!("payload is unwrapped"),
    }
}

pub(super) fn poll(
    engine: &AudioEngine,
    py: Python<'_>,
    mut after_dequeue: impl FnMut(),
) -> PyResult<Option<Py<PyAny>>> {
    resident_relocation::reconcile(engine)?;
    let loader_rx = engine
        .loader_rx
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Failed to acquire loader receiver lock"))?;

    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut pending_event = engine
        .pending_loader_event
        .lock()
        .map_err(|_| PyRuntimeError::new_err("pending loader event lock poisoned"))?;
    let (event, timing_stale, native_owner) = loop {
        if engine.input_runtime_ownership.placement_pending() {
            return Ok(None);
        }
        let mut event = match pending_event.take().map(Ok).unwrap_or_else(|| {
            let received = loader_rx.try_recv();
            if received.is_ok() {
                after_dequeue();
            }
            received
        }) {
            Ok(event) => event,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(None),
        };
        let native_owner = match &event {
            LoaderEvent::NativeOwned { owner, .. } => Some(owner.clone()),
            _ => None,
        };
        if native_owner.is_none()
            && matches!(
                payload(&mut event),
                LoaderEvent::SourceBeatmapCompleted { .. }
                    | LoaderEvent::SourceBeatmapRawCompleted { .. }
            )
        {
            // A data-shaped payload cannot manufacture the original request's
            // verified source custody or content-placement delivery authority.
            continue;
        }
        let (captured_id, request_id) = identity(&event);
        let id = if let Some(owner) = native_owner.as_ref() {
            if !owner.belongs_to(&engine.input_runtime_ownership) {
                continue;
            }
            match owner.observe_control() {
                PlacementObservation::Current(snapshot) => snapshot.slot(),
                PlacementObservation::Pending => {
                    *pending_event = Some(event);
                    return Ok(None);
                }
                PlacementObservation::Stale | PlacementObservation::Retired => continue,
            }
        } else {
            captured_id
        };
        let current_epoch = match engine.prepared_source_epoch(id) {
            Ok(epoch) => epoch,
            Err(_) if engine.input_runtime_ownership.placement_pending() => {
                *pending_event = Some(event);
                return Ok(None);
            }
            Err(error) => return Err(PyRuntimeError::new_err(error)),
        };
        // The immutable backend request identifies evidence. Native work uses its
        // original cell epoch; Pair deliberately keeps numeric request counters fixed.
        let current_stale = native_owner.is_none() && requests.get(id) != Some(&request_id);
        if let Some(owner) = native_owner.as_ref() {
            match owner.observe_control() {
                PlacementObservation::Current(snapshot) if snapshot.slot() == id => {}
                PlacementObservation::Pending => {
                    *pending_event = Some(event);
                    return Ok(None);
                }
                _ => continue,
            }
        }
        let event_payload = payload(&mut event);
        let stale = if native_owner.is_some() {
            false
        } else if matches!(&*event_payload, LoaderEvent::Error { .. }) {
            !engine
                .resident_proposals
                .source_error_matches(id, request_id)
                .unwrap_or(!current_stale)
        } else if matches!(
            &*event_payload,
            LoaderEvent::Started { .. } | LoaderEvent::Progress { .. }
        ) {
            !engine
                .resident_proposals
                .source_event_matches(
                    id,
                    request_id,
                    requests.get(id).copied().unwrap_or(0),
                    current_epoch.load(Ordering::Acquire),
                )
                .unwrap_or(!current_stale)
        } else {
            current_stale
        };
        let timing_stale = stale
            || matches!(&*event_payload, LoaderEvent::Success { timing_epoch: Some(epoch), .. } if current_epoch.load(Ordering::Acquire) != *epoch);
        if stale {
            match &mut *event_payload {
                LoaderEvent::Success {
                    analysis,
                    analysis_error,
                    ..
                } => {
                    let cache = engine
                        .sample_cache
                        .lock()
                        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
                    let generations = engine
                        .loaded_source_generations
                        .lock()
                        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
                    if cache.get(id).is_none_or(Option::is_none)
                        || generations
                            .get(id)
                            .is_none_or(|current| current.0 != request_id)
                    {
                        continue;
                    }
                    *analysis = None;
                    *analysis_error = None;
                }
                LoaderEvent::TaskSuccess {
                    task: BackgroundTaskKind::Analysis,
                    analysis,
                    ..
                } => *analysis = None,
                LoaderEvent::TaskError {
                    task: BackgroundTaskKind::Analysis,
                    ..
                } => {}
                _ => continue,
            }
        }
        if timing_stale
            && !stale
            && let LoaderEvent::Success {
                analysis,
                analysis_error,
                ..
            } = &mut *event_payload
        {
            *analysis = None;
            *analysis_error = None;
        }
        if native_owner.is_some() {
            project_slot(&mut event, id);
        }
        let event = match event {
            LoaderEvent::NativeOwned { event, .. } => *event,
            event => event,
        };
        break (event, timing_stale, native_owner);
    };

    let dict = PyDict::new(py);
    if let Some(owner) = native_owner {
        let timing_epoch = match &event {
            LoaderEvent::Success { timing_epoch, .. } => *timing_epoch,
            _ => None,
        };
        let automatic_analysis = match &event {
            LoaderEvent::Success {
                analysis: Some(analysis),
                ..
            }
            | LoaderEvent::TaskSuccess {
                analysis: Some(analysis),
                ..
            } => Some(analysis),
            _ => None,
        };
        let automatic_stored =
            automatic_analysis.and_then(|analysis| match &analysis.automatic_source_map {
                Some(crate::messages::AutomaticSourceMap::Stored { artifact, consumed }) => {
                    Some((artifact.clone(), consumed.clone()))
                }
                _ => None,
            });
        let source_beatmap = match &event {
            LoaderEvent::SourceBeatmapCompleted { map, .. } => Some(map.clone()),
            _ => automatic_stored
                .as_ref()
                .map(|(artifact, _)| artifact.map()),
        };
        let source_beatmap_artifact = match &event {
            LoaderEvent::SourceBeatmapCompleted { artifact, .. } => artifact.clone(),
            _ => automatic_stored
                .as_ref()
                .map(|(artifact, _)| artifact.clone()),
        };
        let source_beatmap_raw = match &event {
            LoaderEvent::SourceBeatmapRawCompleted { artifact, .. } => Some(artifact.clone()),
            _ => None,
        };
        dict.set_item(
            "_native_event_owner",
            Py::new(
                py,
                LoaderEventDelivery {
                    owner,
                    timing_epoch,
                    source_beatmap,
                    source_beatmap_artifact,
                    source_beatmap_raw,
                    automatic_map: automatic_stored.map(|(_, consumed)| AutomaticMapDelivery {
                        consumed,
                        cold: matches!(&event, LoaderEvent::Success { .. }),
                        exported: false,
                    }),
                    automatic_map_error: automatic_analysis
                        .and_then(|analysis| analysis.automatic_source_map_error.clone()),
                },
            )?,
        )?;
    }
    if timing_stale {
        dict.set_item("timing_stale", true)?;
    }
    match event {
        LoaderEvent::NativeOwned { .. } => {
            unreachable!("Native delivery owner is retained separately")
        }
        LoaderEvent::OfflineAnalysisCompleted {
            id,
            request_id,
            result_json,
        } => {
            dict.set_item("type", "offline_analysis_completed")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
            dict.set_item("result_json", result_json)?;
        }
        LoaderEvent::SourceBeatmapRawCompleted { id, request_id, .. } => {
            dict.set_item("type", "source_beatmap_raw_completed")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
        }
        LoaderEvent::SourceBeatmapCompleted {
            id,
            request_id,
            map,
            artifact,
        } => {
            dict.set_item("type", "source_beatmap_completed")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
            // O(1) native Arc transfer; JSON serialization remains an explicit
            // data query, never ordinary event polling of a possibly64MiB map.
            dict.set_item("map", Py::new(py, map)?)?;
            if let Some(artifact) = artifact {
                // Small historical data only. The genuine output object and
                // durable ACK remain behind original-job private resolution.
                dict.set_item("artifact_receipt", artifact.receipt_json())?;
            }
        }
        LoaderEvent::Started { id, request_id } => {
            dict.set_item("type", "started")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
        }
        LoaderEvent::Progress {
            id,
            request_id,
            percent,
            stage,
        } => {
            dict.set_item("type", "progress")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
            dict.set_item("percent", percent)?;
            dict.set_item("stage", stage)?;
        }
        LoaderEvent::Success {
            id,
            request_id,
            duration_s,
            detected_loop_start_s,
            cached_path,
            analysis,
            analysis_error,
            analysis_tempo_only,
            timing_epoch: _,
            original_lease,
            incoming_defaults,
        } => {
            dict.set_item("type", "success")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
            dict.set_item("duration_s", duration_s)?;
            dict.set_item("detected_loop_start_s", detected_loop_start_s)?;
            dict.set_item("cached_path", cached_path)?;
            if analysis_tempo_only {
                dict.set_item("analysis_kind", "tempo_only")?;
            }
            if let Some(defaults) = incoming_defaults {
                let rate = defaults.sample_rate_hz;
                if rate == 0 {
                    return Err(PyRuntimeError::new_err("incoming source rate unavailable"));
                }
                let receipt = PyDict::new(py);
                receipt.set_item("revision", defaults.revision)?;
                receipt.set_item("key_lock", defaults.key_lock)?;
                receipt.set_item("start_s", defaults.start_frame as f64 / f64::from(rate))?;
                receipt.set_item(
                    "end_s",
                    defaults.end_frame.map(|end| end as f64 / f64::from(rate)),
                )?;
                receipt.set_item("period_seconds", defaults.period_seconds)?;
                receipt.set_item("grid_origin_s", defaults.origin_frame / f64::from(rate))?;
                receipt.set_item("loop_auto", defaults.loop_auto)?;
                receipt.set_item("loop_bars", defaults.loop_bars)?;
                dict.set_item("incoming_defaults", receipt)?;
            }
            if let Some(error) = analysis_error {
                dict.set_item("analysis_error", error)?;
            }
            if let Some(lease) = original_lease {
                dict.set_item("original_lease", Py::new(py, lease)?)?;
            }

            if let Some(analysis) = analysis {
                let analysis_dict = PyDict::new(py);
                analysis_dict.set_item("bpm", analysis.bpm)?;
                if let Some(key) = analysis.key {
                    analysis_dict.set_item("key", key)?;
                }
                if let Some(method) = analysis.bpm_method_json {
                    analysis_dict.set_item("bpm_method", method)?;
                }
                if let Some(error) = analysis.beat_error {
                    dict.set_item("analysis_error", error)?;
                }

                let beat_grid_dict = PyDict::new(py);
                beat_grid_dict.set_item("beats", &analysis.beat_grid.beats)?;
                beat_grid_dict.set_item("downbeats", &analysis.beat_grid.downbeats)?;
                beat_grid_dict.set_item("bars", &analysis.beat_grid.bars)?;
                analysis_dict.set_item("beat_grid", beat_grid_dict)?;

                dict.set_item("analysis", analysis_dict)?;
            }
        }
        LoaderEvent::Error {
            id,
            request_id,
            error,
        } => {
            dict.set_item("type", "error")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
            dict.set_item("msg", error)?;
        }
        LoaderEvent::TaskStarted {
            id,
            request_id,
            task,
        } => {
            dict.set_item("type", "task_started")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
            dict.set_item("task", task_to_str(task))?;
        }
        LoaderEvent::TaskProgress {
            id,
            request_id,
            task,
            percent,
            stage,
        } => {
            dict.set_item("type", "task_progress")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
            dict.set_item("task", task_to_str(task))?;
            dict.set_item("percent", percent)?;
            dict.set_item("stage", stage)?;
        }
        LoaderEvent::TaskSuccess {
            id,
            request_id,
            task,
            analysis,
        } => {
            dict.set_item("type", "task_success")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
            dict.set_item("task", task_to_str(task))?;

            if let Some(analysis) = analysis {
                let analysis_dict = PyDict::new(py);
                analysis_dict.set_item("bpm", analysis.bpm)?;
                if let Some(key) = analysis.key {
                    analysis_dict.set_item("key", key)?;
                }
                if let Some(method) = analysis.bpm_method_json {
                    analysis_dict.set_item("bpm_method", method)?;
                }
                if let Some(error) = analysis.beat_error {
                    dict.set_item("analysis_error", error)?;
                }

                let beat_grid_dict = PyDict::new(py);
                beat_grid_dict.set_item("beats", &analysis.beat_grid.beats)?;
                beat_grid_dict.set_item("downbeats", &analysis.beat_grid.downbeats)?;
                beat_grid_dict.set_item("bars", &analysis.beat_grid.bars)?;
                analysis_dict.set_item("beat_grid", beat_grid_dict)?;

                dict.set_item("analysis", analysis_dict)?;
            }
        }
        LoaderEvent::TaskError {
            id,
            request_id,
            task,
            error,
        } => {
            dict.set_item("type", "task_error")?;
            dict.set_item("id", id)?;
            dict.set_item("request_id", request_id)?;
            dict.set_item("task", task_to_str(task))?;
            dict.set_item("msg", error)?;
        }
    }

    Ok(Some(dict.into_any().unbind()))
}

#[cfg(test)]
#[path = "loader_events_tests.rs"]
mod tests;
