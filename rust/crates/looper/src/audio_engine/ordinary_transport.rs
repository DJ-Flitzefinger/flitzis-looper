//! Durable ordinary transport observations. Only the callback writes each snapshot.
use super::content_placement::{ContentCell, ContentKey, PlacementObservation};
use super::input_runtime_binding::InputRuntimeOwnership;
use crate::messages::{AudioMessage, TransportContentEffect};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const PAUSE: u64 = 1;
pub(crate) const RESUME: u64 = 2;
pub(crate) const SEEK: u64 = 3;
pub(crate) const APPLIED: u64 = 1;
pub(crate) const NO_EFFECT: u64 = 2;
pub(crate) const REFUSED: u64 = 3;
pub(crate) const TRANSPORT: u64 = 0;
pub(crate) const STARTED: u64 = 1;
pub(crate) const STOPPED: u64 = 2;
pub(crate) const SOURCE_ADOPTED: u64 = 3;
pub(crate) const TERMINATED: u64 = 4;

pub(crate) fn request(effect: TransportContentEffect) -> u64 {
    match effect {
        TransportContentEffect::Pause => PAUSE,
        TransportContentEffect::Resume => RESUME,
        TransportContentEffect::Seek { .. } => SEEK,
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct OrdinarySnapshot {
    pub(crate) sequence: u64,
    pub(crate) lifetime: u64,
    pub(crate) source: u64,
    pub(crate) revision: u64,
    pub(crate) request: u64,
    pub(crate) outcome: u64,
    pub(crate) kind: u64,
    pub(crate) active: bool,
    pub(crate) paused: bool,
    frontier_revision: u64,
    frontier_request: u64,
    frontier_outcome: u64,
}

#[derive(Default)]
pub(crate) struct OrdinaryObservationSlot {
    sequence: AtomicU64,
    lifetime: AtomicU64,
    source: AtomicU64,
    revision: AtomicU64,
    request: AtomicU64,
    outcome: AtomicU64,
    kind: AtomicU64,
    flags: AtomicU64,
    frontier_revision: AtomicU64,
    frontier_request: AtomicU64,
    frontier_outcome: AtomicU64,
}

impl OrdinaryObservationSlot {
    fn readable(&self) -> bool {
        self.sequence.load(Ordering::SeqCst) != u64::MAX
    }
    pub(crate) fn available(&self) -> bool {
        let sequence = self.sequence.load(Ordering::SeqCst);
        sequence.checked_add(2).is_some()
    }

    pub(crate) fn read(&self) -> Option<OrdinarySnapshot> {
        let sequence = self.sequence.load(Ordering::SeqCst);
        if sequence == 0 || sequence & 1 != 0 {
            return None;
        }
        let flags = self.flags.load(Ordering::SeqCst);
        let value = OrdinarySnapshot {
            sequence,
            lifetime: self.lifetime.load(Ordering::SeqCst),
            source: self.source.load(Ordering::SeqCst),
            revision: self.revision.load(Ordering::SeqCst),
            request: self.request.load(Ordering::SeqCst),
            outcome: self.outcome.load(Ordering::SeqCst),
            kind: self.kind.load(Ordering::SeqCst),
            active: flags & 1 != 0,
            paused: flags & 2 != 0,
            frontier_revision: self.frontier_revision.load(Ordering::SeqCst),
            frontier_request: self.frontier_request.load(Ordering::SeqCst),
            frontier_outcome: self.frontier_outcome.load(Ordering::SeqCst),
        };
        (self.sequence.load(Ordering::SeqCst) == sequence).then_some(value)
    }

    /// One bounded callback store. Exhaustion permanently closes observation authority.
    fn publish(&self, mut value: OrdinarySnapshot) -> Option<OrdinarySnapshot> {
        let previous = self.sequence.load(Ordering::SeqCst);
        let Some(next) = previous.checked_add(2).filter(|_| previous & 1 == 0) else {
            self.sequence.store(u64::MAX, Ordering::SeqCst);
            return None;
        };
        self.sequence.store(previous + 1, Ordering::SeqCst);
        self.lifetime.store(value.lifetime, Ordering::SeqCst);
        self.source.store(value.source, Ordering::SeqCst);
        self.revision.store(value.revision, Ordering::SeqCst);
        self.request.store(value.request, Ordering::SeqCst);
        self.outcome.store(value.outcome, Ordering::SeqCst);
        self.kind.store(value.kind, Ordering::SeqCst);
        self.flags.store(
            u64::from(value.active) | (u64::from(value.paused) << 1),
            Ordering::SeqCst,
        );
        self.frontier_revision
            .store(value.frontier_revision, Ordering::SeqCst);
        self.frontier_request
            .store(value.frontier_request, Ordering::SeqCst);
        self.frontier_outcome
            .store(value.frontier_outcome, Ordering::SeqCst);
        self.sequence.store(next, Ordering::SeqCst);
        value.sequence = next;
        Some(value)
    }
}

impl InputRuntimeOwnership {
    pub(crate) fn ordinary_available(&self, cell: ContentCell) -> bool {
        self.ordinary_observations[cell.index()].available() && self.ordinary_source_available(cell)
    }
    fn ordinary_readable(&self, cell: ContentCell) -> bool {
        self.ordinary_observations[cell.index()].readable() && self.ordinary_source_available(cell)
    }

    pub(crate) fn ordinary_snapshot(&self, key: ContentKey) -> Option<OrdinarySnapshot> {
        let source = self.ordinary_source_epoch(key.cell())?;
        self.ordinary_observations[key.cell().index()]
            .read()
            .filter(|value| value.lifetime == key.lifetime() && value.source == source)
    }

    pub(crate) fn ordinary_recorded_source(&self, key: ContentKey) -> Option<u64> {
        self.ordinary_recorded_snapshot(key)
            .map(|value| value.source)
    }

    /// Callback-only cause lookup across a semantic-source publication. This is never
    /// receipt validation: old-source admissions still fail the normal source fence.
    pub(crate) fn ordinary_recorded_snapshot(&self, key: ContentKey) -> Option<OrdinarySnapshot> {
        self.ordinary_observations[key.cell().index()]
            .read()
            .filter(|value| value.lifetime == key.lifetime())
    }

    /// Capture the original revision inside the claimed effect, never after it.
    pub(crate) fn publish_ordinary_effect(
        &self,
        key: ContentKey,
        revision: u64,
        effect: TransportContentEffect,
        outcome: u64,
        active: bool,
        paused: bool,
    ) -> Option<AudioMessage> {
        let source = self.ordinary_source_epoch(key.cell())?;
        self.publish_ordinary_effect_for_source(
            key, source, revision, effect, outcome, active, paused,
        )
    }

    /// Callback-only disposition of an original source-bound command. A stale source
    /// never grants an audible effect or a receipt/frontier for its replacement source.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn publish_ordinary_effect_for_source(
        &self,
        key: ContentKey,
        source: u64,
        revision: u64,
        effect: TransportContentEffect,
        outcome: u64,
        active: bool,
        paused: bool,
    ) -> Option<AudioMessage> {
        let previous = self
            .ordinary_recorded_snapshot(key)
            .filter(|value| value.source == source)
            .unwrap_or_default();
        let request = request(effect);
        let frontier = if request == PAUSE || request == RESUME {
            (revision, request, outcome)
        } else {
            (
                previous.frontier_revision,
                previous.frontier_request,
                previous.frontier_outcome,
            )
        };
        let value = self.ordinary_observations[key.cell().index()].publish(OrdinarySnapshot {
            lifetime: key.lifetime(),
            source,
            revision,
            request,
            outcome,
            kind: TRANSPORT,
            active,
            paused,
            frontier_revision: frontier.0,
            frontier_request: frontier.1,
            frontier_outcome: frontier.2,
            ..OrdinarySnapshot::default()
        })?;
        Some(AudioMessage::OrdinaryTransportFeedback {
            id: self.callback_key_slot(key)?,
            owner: self.owner_identity().diagnostic(),
            cell: key.cell().index(),
            lifetime: key.lifetime(),
            transport_revision: revision,
            observation_sequence: value.sequence,
        })
    }

    /// Lifecycle observation has no invented ordinary command origin. Same-source custody
    /// retains its exact completed intent frontier; a new source/content has none.
    pub(crate) fn publish_ordinary_lifecycle(&self, key: ContentKey, kind: u64, active: bool) {
        let previous = self.ordinary_snapshot(key).unwrap_or_default();
        self.publish_ordinary_lifecycle_state(
            key,
            kind,
            active,
            kind == TERMINATED && active && previous.paused,
        );
    }

    pub(crate) fn publish_ordinary_source_adoption(
        &self,
        key: ContentKey,
        active: bool,
        paused: bool,
    ) {
        self.publish_ordinary_lifecycle_state(key, SOURCE_ADOPTED, active, paused);
    }

    fn publish_ordinary_lifecycle_state(
        &self,
        key: ContentKey,
        kind: u64,
        active: bool,
        paused: bool,
    ) {
        let source = self.ordinary_source_epoch(key.cell()).unwrap_or(0);
        let previous = self.ordinary_snapshot(key).unwrap_or_default();
        self.ordinary_observations[key.cell().index()].publish(OrdinarySnapshot {
            lifetime: key.lifetime(),
            source,
            kind,
            active,
            paused,
            frontier_revision: previous.frontier_revision,
            frontier_request: previous.frontier_request,
            frontier_outcome: previous.frontier_outcome,
            ..OrdinarySnapshot::default()
        });
    }
}

/// Opaque enqueue admission; it never asserts an audio effect succeeded.
#[pyclass(frozen, module = "flitzis_looper_audio")]
#[derive(Clone)]
pub struct OrdinaryTransportAdmission {
    ownership: Arc<InputRuntimeOwnership>,
    key: Option<ContentKey>,
    source: u64,
    #[pyo3(get)]
    transport_revision: u64,
    #[pyo3(get)]
    requested_paused: bool,
}

impl std::fmt::Debug for OrdinaryTransportAdmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OrdinaryTransportAdmission")
            .field(
                "content_lifetime",
                &self.key.map_or(0, ContentKey::lifetime),
            )
            .field("transport_revision", &self.transport_revision)
            .field("requested_paused", &self.requested_paused)
            .finish_non_exhaustive()
    }
}

impl OrdinaryTransportAdmission {
    #[cfg(test)]
    pub(crate) fn transport_revision(&self) -> u64 {
        self.transport_revision
    }
    pub(crate) fn new(
        ownership: Arc<InputRuntimeOwnership>,
        key: Option<ContentKey>,
        source: u64,
        revision: u64,
        effect: TransportContentEffect,
    ) -> Self {
        Self {
            ownership,
            key,
            source,
            transport_revision: revision,
            requested_paused: matches!(effect, TransportContentEffect::Pause),
        }
    }
}

#[pymethods]
impl OrdinaryTransportAdmission {
    #[getter]
    pub fn content_lifetime(&self) -> u64 {
        self.key.map_or(0, ContentKey::lifetime)
    }

    pub fn current_control_slot(&self) -> PyResult<Option<usize>> {
        let Some(key) = self.key else {
            return Ok(None);
        };
        current_slot(&self.ownership, key, self.source)
    }

    pub fn current_observation(&self) -> PyResult<Option<OrdinaryTransportObservation>> {
        let Some(key) = self.key else {
            return Ok(None);
        };
        observe(&self.ownership, key, self.source, None)
    }
}

/// An immutable, currently validated observation of ordinary transport authority.
#[pyclass(frozen, module = "flitzis_looper_audio")]
pub struct OrdinaryTransportObservation {
    ownership: Arc<InputRuntimeOwnership>,
    key: ContentKey,
    value: OrdinarySnapshot,
    #[pyo3(get)]
    sample_id: usize,
}

fn effect_name(value: u64) -> Option<&'static str> {
    match value {
        PAUSE => Some("pause"),
        RESUME => Some("resume"),
        SEEK => Some("seek"),
        _ => None,
    }
}

#[pymethods]
impl OrdinaryTransportObservation {
    pub fn current_control_slot(&self) -> PyResult<Option<usize>> {
        current_slot(&self.ownership, self.key, self.value.source)
    }
    pub fn current_observation(&self) -> PyResult<Option<OrdinaryTransportObservation>> {
        observe(&self.ownership, self.key, self.value.source, None)
    }
    #[getter]
    pub fn content_lifetime(&self) -> u64 {
        self.key.lifetime()
    }
    #[getter]
    pub fn transport_revision(&self) -> u64 {
        self.value.revision
    }
    #[getter]
    pub fn observation_sequence(&self) -> u64 {
        self.value.sequence
    }
    #[getter]
    pub fn status_kind(&self) -> &'static str {
        match self.value.kind {
            STARTED => "started",
            STOPPED => "stopped",
            SOURCE_ADOPTED => "source_adopted",
            TERMINATED => "terminated",
            _ => "transport",
        }
    }
    #[getter]
    pub fn requested_effect(&self) -> Option<&'static str> {
        effect_name(self.value.request)
    }
    #[getter]
    pub fn outcome(&self) -> Option<&'static str> {
        match self.value.outcome {
            APPLIED => Some("applied"),
            NO_EFFECT => Some("no_effect"),
            REFUSED => Some("refused"),
            _ => None,
        }
    }
    #[getter]
    pub fn active(&self) -> bool {
        self.value.active
    }
    #[getter]
    pub fn ordinary_paused(&self) -> bool {
        self.value.paused
    }
    pub fn matches_admission(&self, admission: &OrdinaryTransportAdmission) -> bool {
        self.same_admission_content(admission)
            && self.value.revision == admission.transport_revision
            && self.value.request
                == if admission.requested_paused {
                    PAUSE
                } else {
                    RESUME
                }
    }
    pub fn settles_admission(&self, admission: &OrdinaryTransportAdmission) -> bool {
        self.same_admission_content(admission)
            && self.value.frontier_revision == admission.transport_revision
            && matches!(self.value.frontier_outcome, APPLIED | NO_EFFECT | REFUSED)
            && self.value.frontier_request
                == if admission.requested_paused {
                    PAUSE
                } else {
                    RESUME
                }
    }
}

impl OrdinaryTransportObservation {
    fn same_admission_content(&self, admission: &OrdinaryTransportAdmission) -> bool {
        Arc::ptr_eq(&self.ownership, &admission.ownership)
            && admission.key == Some(self.key)
            && self.value.source == admission.source
    }
}

fn current_slot(
    ownership: &InputRuntimeOwnership,
    key: ContentKey,
    source: u64,
) -> PyResult<Option<usize>> {
    if !ownership.ordinary_readable(key.cell()) {
        return Err(PyRuntimeError::new_err(
            "ordinary transport observation unavailable",
        ));
    }
    let slot = match ownership.observe_control_key(key) {
        PlacementObservation::Current(snapshot) => snapshot.slot(),
        PlacementObservation::Pending => {
            return Err(PyRuntimeError::new_err(
                "content feedback placement pending",
            ));
        }
        _ => return Ok(None),
    };
    let current = ownership.ordinary_source_epoch(key.cell());
    // A publication can overlap the fixed source read. Observe placement again rather
    // than interpreting that temporary unavailable source as permanent removal.
    match ownership.observe_control_key(key) {
        PlacementObservation::Current(snapshot) if snapshot.slot() == slot => {}
        PlacementObservation::Pending => {
            return Err(PyRuntimeError::new_err(
                "content feedback placement pending",
            ));
        }
        _ => return Ok(None),
    }
    Ok((source != 0 && current == Some(source)).then_some(slot))
}

fn observe(
    ownership: &Arc<InputRuntimeOwnership>,
    key: ContentKey,
    source: u64,
    expected: Option<(u64, u64)>,
) -> PyResult<Option<OrdinaryTransportObservation>> {
    let Some(slot) = current_slot(ownership, key, source)? else {
        return Ok(None);
    };
    let Some(value) = ownership.ordinary_snapshot(key) else {
        return Ok(None);
    };
    if expected.is_some_and(|(sequence, revision)| {
        value.sequence != sequence || value.revision != revision
    }) {
        return Ok(None);
    }
    if current_slot(ownership, key, source)? != Some(slot)
        || ownership
            .ordinary_snapshot(key)
            .is_none_or(|current| current.sequence != value.sequence)
    {
        return Ok(None);
    }
    Ok(Some(OrdinaryTransportObservation {
        ownership: ownership.clone(),
        key,
        value,
        sample_id: slot,
    }))
}

pub(super) fn current_for_slot(
    ownership: &Arc<InputRuntimeOwnership>,
    id: usize,
) -> PyResult<Option<OrdinaryTransportObservation>> {
    let snapshot = ownership
        .slot_snapshot(id)
        .ok_or_else(|| PyRuntimeError::new_err("content feedback placement pending"))?;
    let Some(key) = snapshot.content_key() else {
        return Ok(None);
    };
    if !ownership.ordinary_readable(key.cell()) {
        return Err(PyRuntimeError::new_err(
            "ordinary transport observation unavailable",
        ));
    }
    let Some(source) = ownership.ordinary_source_epoch(key.cell()) else {
        return Ok(None);
    };
    let value = observe(ownership, key, source, None)?;
    Ok(value.filter(|value| value.sample_id == id))
}

pub(super) fn resolve(
    ownership: &Arc<InputRuntimeOwnership>,
    message: &AudioMessage,
) -> PyResult<Option<OrdinaryTransportObservation>> {
    let AudioMessage::OrdinaryTransportFeedback {
        owner,
        cell,
        lifetime,
        transport_revision,
        observation_sequence,
        ..
    } = message
    else {
        return Ok(None);
    };
    if *owner != ownership.owner_identity().diagnostic() {
        return Ok(None);
    }
    let Some(cell) = ContentCell::from_index(*cell) else {
        return Ok(None);
    };
    let Some(key) = ContentKey::new(ownership.owner_identity(), cell, *lifetime) else {
        return Ok(None);
    };
    if !ownership.ordinary_readable(cell) {
        return Err(PyRuntimeError::new_err(
            "ordinary transport observation unavailable",
        ));
    }
    let Some(source) = ownership.ordinary_source_epoch(cell) else {
        return Ok(None);
    };
    observe(
        ownership,
        key,
        source,
        Some((*observation_sequence, *transport_revision)),
    )
}

#[cfg(test)]
#[path = "ordinary_transport_tests.rs"]
mod tests;
