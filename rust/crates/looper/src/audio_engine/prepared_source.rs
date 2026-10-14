//! Non-realtime preparation identity and bounded callback publication checks.

use super::AudioEngine;
use super::constant_timing::{AcceptedTimingProjection, CurrentTimingAcknowledgements};
use super::constants::NUM_SAMPLES;
use super::content_placement::{ContentKey, PlacementObservation, SlotSnapshot};
use super::input_runtime_binding::{self, InputPadBinding, InputRuntimeOwnership};
use crate::messages::ControlMessage;
use crate::messages::SampleBuffer;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Opaque engine-owned admission snapshot. Never constructed from Python numbers.
#[pyclass(frozen)]
#[derive(Clone)]
pub struct PreparedSourceTicket {
    pub(super) admission: super::input_runtime_binding::InputRuntimePadBinding,
    pub(super) id: usize,
    pub(super) request_id: u64,
    pub(super) request_incarnation: u64,
    pub(super) sample: SampleBuffer,
    pub(super) sample_rate_hz: u32,
    pub(super) source_version: String,
    pub(super) publication: PreparedSourcePermit,
}

#[pymethods]
impl PreparedSourceTicket {
    /// Bounded callback adoption feedback; polling never drives audio timing.
    pub fn publication_status(&self) -> &'static str {
        self.publication.status()
    }

    /// Fixed callback rejection feedback, never persisted as source authority.
    pub fn rejection_reason(&self) -> Option<&'static str> {
        self.publication.rejection_reason()
    }

    /// Same actual source/request, allowing a freshly acknowledged resident window.
    pub fn same_source_request(&self, other: &PreparedSourceTicket) -> bool {
        Arc::ptr_eq(&self.admission.ownership, &other.admission.ownership)
            && self
                .admission
                .binding
                .placement
                .and_then(|p| p.content_key())
                .is_some()
            && self
                .admission
                .binding
                .placement
                .and_then(|p| p.content_key())
                == other
                    .admission
                    .binding
                    .placement
                    .and_then(|p| p.content_key())
            && self.admission.source_generation == other.admission.source_generation
            && self.request_incarnation == other.request_incarnation
            && self.source_version == other.source_version
            && self.sample_rate_hz == other.sample_rate_hz
            && Arc::ptr_eq(&self.publication.epoch, &other.publication.epoch)
            && self.sample.same_source(&other.sample)
    }

    /// Compare sealed preparation snapshots; equality does not grant an audio ACK.
    pub fn same_preparation_context(&self, other: &PreparedSourceTicket) -> bool {
        self.same_source_request(other)
            && self.publication.expected == other.publication.expected
            && self.publication.current()
            && other.publication.current()
            && self.sample.same_window(&other.sample)
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub(crate) enum PreparedStemRejection {
    Unspecified = 1,
    PadPlaying = 2,
    SourceRequestChanged = 3,
    TimingChanged = 4,
    WindowChanged = 5,
    InvalidGeometry = 6,
}

impl PreparedStemRejection {
    fn name(value: u8) -> &'static str {
        match value {
            2 => "pad-playing",
            3 => "source-request-changed",
            4 => "timing-changed",
            5 => "window-changed",
            6 => "invalid-geometry",
            _ => "unspecified",
        }
    }
}

impl PreparedSourcePermit {
    pub(super) fn same_publication(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.status, &other.status)
            && Arc::ptr_eq(&self.epoch, &other.epoch)
            && self.expected == other.expected
            && self.content_key() == other.content_key()
    }
    pub(super) fn accepted_fixed() -> Self {
        let permit = Self::new(Arc::new(AtomicU64::new(1)), 1);
        permit.mark_accepted();
        permit
    }
    pub(crate) fn status(&self) -> &'static str {
        match self.status.load(Ordering::Acquire) {
            0 => "captured",
            1 => "pending",
            2 | 5 => "accepted",
            4 => "adopting",
            _ => "rejected",
        }
    }
}

/// One bounded atomic check at mixer adoption; accepted stems remain source-frame data.
#[derive(Debug, Clone)]
pub(crate) struct PreparedSourcePermit {
    pub(super) epoch: Arc<AtomicU64>,
    pub(super) expected: u64,
    status: Arc<AtomicU8>,
    rejection: Arc<AtomicU8>,
    resident_seek_bits: Arc<AtomicU64>,
    source_epoch: Option<(Arc<AtomicU64>, u64)>,
    timing: Option<PreparedTimingPermit>,
}

/// Inline resident-launch snapshot of the existing scalar publication guards.
/// Timing-bound permits cannot produce this snapshot; mixer source/timing binding
/// checks remain separate. Cloning creates no new RT-owned heap object.
#[derive(Debug, Clone)]
pub(crate) struct ResidentLaunchPermit {
    pub(super) epoch: Arc<AtomicU64>,
    pub(super) expected: u64,
    status: Arc<AtomicU8>,
    source_epoch: Option<(Arc<AtomicU64>, u64)>,
}

fn publication_epochs_current(
    epoch: &AtomicU64,
    expected: u64,
    source_epoch: &Option<(Arc<AtomicU64>, u64)>,
) -> bool {
    epoch.load(Ordering::Acquire) == expected
        && source_epoch
            .as_ref()
            .is_none_or(|(epoch, expected)| epoch.load(Ordering::Acquire) == *expected)
}

impl ResidentLaunchPermit {
    pub(crate) fn current_accepted(&self) -> bool {
        matches!(self.status.load(Ordering::Acquire), 2 | 5)
            && publication_epochs_current(&self.epoch, self.expected, &self.source_epoch)
    }
}

/// Current accepted authority captured from the same native resolver as MIDI.
/// These shared owners contain fixed atomics only, never PCM or evidence.
#[derive(Clone)]
struct PreparedTimingPermit {
    id: usize,
    binding: InputPadBinding,
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    source_generation: u64,
}

impl std::fmt::Debug for PreparedTimingPermit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedTimingPermit")
            .field("id", &self.id)
            .field("binding", &self.binding)
            .finish_non_exhaustive()
    }
}

impl PreparedTimingPermit {
    fn current(&self) -> bool {
        self.ownership.authority_current(self.id, self.binding)
            && self
                .ownership
                .binding_source_generation(self.id, self.binding)
                == Some(self.source_generation)
            && self.ownership.binding_window_current(self.id, self.binding)
            && self
                .acknowledgements
                .current_for_binding(&self.ownership, self.id, self.binding)
                == self
                    .binding
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
    }
}

impl PreparedSourcePermit {
    pub(crate) fn content_key(&self) -> Option<ContentKey> {
        self.timing
            .as_ref()
            .and_then(|timing| timing.binding.placement)
            .and_then(|placement| placement.content_key())
    }

    pub(crate) fn callback_slot(
        &self,
        ownership: &InputRuntimeOwnership,
        id: usize,
    ) -> Option<usize> {
        if !ownership.is_source_tracked() {
            return (id < NUM_SAMPLES).then_some(id);
        }
        let timing = self.timing.as_ref()?;
        std::ptr::eq(timing.ownership.as_ref(), ownership)
            .then(|| ownership.callback_key_slot(self.content_key()?))
            .flatten()
    }

    pub(super) fn control_placement(
        &self,
        engine: &AudioEngine,
    ) -> Result<PlacementObservation<SlotSnapshot>, String> {
        let timing = self
            .timing
            .as_ref()
            .ok_or("prepared Native source owner unavailable")?;
        if !Arc::ptr_eq(&timing.ownership, &engine.input_runtime_ownership) {
            return Err("foreign prepared Native source owner".into());
        }
        Ok(timing.ownership.observe_control_key(
            self.content_key()
                .ok_or("prepared Native content key unavailable")?,
        ))
    }

    pub(super) fn unbound() -> Self {
        Self::new(Arc::new(AtomicU64::new(0)), 1)
    }

    pub(super) fn new(epoch: Arc<AtomicU64>, expected: u64) -> Self {
        Self {
            epoch,
            expected,
            status: Arc::new(AtomicU8::new(0)),
            rejection: Arc::new(AtomicU8::new(0)),
            resident_seek_bits: Arc::new(AtomicU64::new(f64::NAN.to_bits())),
            source_epoch: None,
            timing: None,
        }
    }

    pub(super) fn mark_pending(&self) -> Result<(), String> {
        self.status
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "prepared source ticket already published".into())
    }

    pub(crate) fn mark_accepted(&self) {
        self.status.store(2, Ordering::Release);
    }

    /// A guarded seek of a previous-source pin changes no current bank storage.
    pub(crate) fn mark_accepted_preserved_window(&self) {
        self.status.store(5, Ordering::Release);
    }

    pub(super) fn preserved_window(&self) -> bool {
        self.status.load(Ordering::Acquire) == 5
    }

    pub(crate) fn record_resident_seek(&self, seconds: Option<f64>) {
        self.resident_seek_bits
            .store(seconds.unwrap_or(f64::NAN).to_bits(), Ordering::Release);
    }

    pub(super) fn resident_seek_seconds(&self) -> Option<f64> {
        let seconds = f64::from_bits(self.resident_seek_bits.load(Ordering::Acquire));
        seconds.is_finite().then_some(seconds)
    }

    pub(crate) fn mark_rejected(&self) {
        self.mark_rejected_reason(PreparedStemRejection::Unspecified);
    }

    pub(crate) fn mark_rejected_reason(&self, reason: PreparedStemRejection) {
        self.rejection.store(reason as u8, Ordering::Relaxed);
        self.status.store(3, Ordering::Release);
    }

    fn rejection_reason(&self) -> Option<&'static str> {
        (self.status.load(Ordering::Acquire) == 3)
            .then(|| PreparedStemRejection::name(self.rejection.load(Ordering::Relaxed)))
    }

    pub(crate) fn current_rejection(&self) -> Option<PreparedStemRejection> {
        if self.timing.as_ref().is_some_and(|timing| {
            timing
                .ownership
                .binding_source_generation(timing.id, timing.binding)
                != Some(timing.source_generation)
        }) {
            Some(PreparedStemRejection::SourceRequestChanged)
        } else if self.timing.as_ref().is_some_and(|timing| !timing.current()) {
            Some(PreparedStemRejection::TimingChanged)
        } else if self.status.load(Ordering::Acquire) == 3
            || !publication_epochs_current(&self.epoch, self.expected, &self.source_epoch)
        {
            Some(PreparedStemRejection::SourceRequestChanged)
        } else {
            None
        }
    }
    pub(crate) fn current(&self) -> bool {
        self.status.load(Ordering::Acquire) != 3
            && publication_epochs_current(&self.epoch, self.expected, &self.source_epoch)
            && self
                .timing
                .as_ref()
                .is_none_or(PreparedTimingPermit::current)
    }

    pub(super) fn resident_launch_permit(&self) -> Option<ResidentLaunchPermit> {
        self.timing.is_none().then(|| ResidentLaunchPermit {
            epoch: self.epoch.clone(),
            expected: self.expected,
            status: self.status.clone(),
            source_epoch: self.source_epoch.clone(),
        })
    }

    pub(super) fn with_source_epoch(mut self, epoch: Arc<AtomicU64>, expected: u64) -> Self {
        self.source_epoch = Some((epoch, expected));
        self
    }

    pub(crate) fn claim_resident(&self) -> bool {
        self.status
            .compare_exchange(1, 4, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// A stopped seek has no PCM payload; still claim before its native no-op ACK.
    pub(crate) fn claim_resident_capture(&self) -> bool {
        self.status
            .compare_exchange(0, 4, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(super) fn cancel_unclaimed(&self) -> bool {
        let state = self.status.load(Ordering::Acquire);
        state <= 1
            && self
                .status
                .compare_exchange(state, 3, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    /// Exact fixed projection from the authoritative current record at capture.
    /// Preparation epochs and raw revisions cannot manufacture this value.
    pub(super) fn accepted_projection(&self) -> Option<AcceptedTimingProjection> {
        self.timing
            .as_ref()
            .and_then(|timing| timing.binding.accepted)
    }

    #[cfg(test)]
    pub(crate) fn for_epoch(epoch: Arc<AtomicU64>, expected: u64) -> Self {
        Self::new(epoch, expected)
    }

    /// Mixer fixtures retain the same actual Native source and timing owners as
    /// productive capture. A contentless fixed permit cannot publish into a tracked bank.
    #[cfg(test)]
    pub(crate) fn for_native_binding_for_test(
        id: usize,
        binding: InputPadBinding,
        ownership: Arc<InputRuntimeOwnership>,
        acknowledgements: Arc<CurrentTimingAcknowledgements>,
        epoch: Arc<AtomicU64>,
    ) -> Result<Self, String> {
        if binding
            .placement
            .and_then(|placement| placement.content_key())
            .is_none()
            || !ownership.current(id, binding)
            || acknowledgements.current_for_binding(&ownership, id, binding)
                != binding
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
        {
            return Err("test publication lacks its actual Native source/timing owner".into());
        }
        let source_generation = ownership
            .binding_source_generation(id, binding)
            .ok_or("test publication source generation is unavailable")?;
        let mut permit = Self::new(epoch.clone(), epoch.load(Ordering::Acquire));
        permit.timing = Some(PreparedTimingPermit {
            id,
            binding,
            ownership,
            acknowledgements,
            source_generation,
        });
        permit.mark_pending()?;
        Ok(permit)
    }

    #[cfg(test)]
    pub(crate) fn unrestricted() -> Self {
        Self::for_epoch(Arc::new(AtomicU64::new(1)), 1)
    }
}

pub(super) fn next_epoch(epoch: &AtomicU64) -> Result<u64, String> {
    epoch
        .load(Ordering::Acquire)
        .checked_add(1)
        .ok_or_else(|| "prepared source publication epoch exhausted".into())
}

pub(super) fn validate_prepared_ticket(
    ticket: &PreparedSourceTicket,
    id: usize,
    version: &str,
    request_id: u64,
    epoch: &Arc<AtomicU64>,
    sample: &SampleBuffer,
) -> Result<(), String> {
    let timing = ticket
        .publication
        .timing
        .as_ref()
        .ok_or("prepared Native source owner unavailable")?;
    let original = timing
        .binding
        .placement
        .ok_or("prepared Native placement unavailable")?;
    let resolved = match timing.ownership.observe_control_key(
        original
            .content_key()
            .ok_or("prepared Native content key unavailable")?,
    ) {
        PlacementObservation::Current(resolved) => resolved,
        PlacementObservation::Pending => {
            return Err("prepared source control placement is pending".into());
        }
        _ => return Err("stale prepared source content key".into()),
    };
    if resolved.slot() != id
        || (original.same_projection(resolved) && ticket.request_id != request_id)
        || ticket.source_version != version
        || !Arc::ptr_eq(&ticket.publication.epoch, epoch)
        || !ticket.publication.current()
        || ticket.sample.channels != sample.channels
        || !ticket.sample.same_window(sample)
    {
        return Err("stale or foreign prepared source ticket".into());
    }
    Ok(())
}

/// Hash full original bytes off-thread; reject observable changes across the read.
/// This does not replace the later immutable copy-first decoding contract.
pub(super) fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let before = file.metadata().map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut bytes = [0_u8; 64 * 1024];
    let mut count = 0_u64;
    loop {
        let read = file.read(&mut bytes).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        count += read as u64;
        digest.update(&bytes[..read]);
    }
    let after = file.metadata().map_err(|error| error.to_string())?;
    let current = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if count != before.len()
        || before.len() != after.len()
        || after.len() != current.len()
        || before.modified().ok() != after.modified().ok()
        || after.modified().ok() != current.modified().ok()
    {
        return Err("source file changed during content hashing".into());
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(super) fn version_matches_digest(version: &str, digest: &str) -> bool {
    version
        .rsplit_once("|sha256-v1:")
        .is_some_and(|(path, supplied)| {
            !path.is_empty() && supplied == digest && supplied.len() == 64
        })
}

/// The locked single producer reserves available capacity before invalidating
/// preparation. A consumer can only free slots, so the subsequent push cannot
/// become full. The epoch is visible before the timing command is visible.
#[cfg(test)]
pub(super) fn push_preparation_epoch_message<T>(
    producer: &mut Producer<T>,
    message: T,
    epoch: &AtomicU64,
    label: &str,
) -> PyResult<()> {
    let next = next_epoch(epoch).map_err(PyRuntimeError::new_err)?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(format!(
            "Failed to send {label} - buffer may be full"
        )));
    }
    epoch.store(next, Ordering::Release);
    producer
        .push(message)
        .map_err(|_| PyRuntimeError::new_err("reserved single-producer capacity lost"))
}

#[cfg(test)]
pub(super) fn enqueue_current_prepared_stems(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ticket: &PreparedSourceTicket,
    source_version: &str,
    stems: crate::messages::PreparedStemSet,
) -> PyResult<()> {
    enqueue_prepared_stems(engine, producer, ticket, source_version, stems, None)
}

pub(super) fn enqueue_current_prepared_stems_with_owner(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ticket: &PreparedSourceTicket,
    source_version: &str,
    stems: crate::messages::PreparedStemSet,
    registration: (PathBuf, PathBuf),
) -> PyResult<()> {
    enqueue_prepared_stems(
        engine,
        producer,
        ticket,
        source_version,
        stems,
        Some((registration.0, registration.1, None)),
    )
}

pub(super) fn enqueue_current_prepared_pair_with_owner(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ticket: &PreparedSourceTicket,
    source_version: &str,
    stems: crate::messages::PreparedStemSet,
    wav_path: PathBuf,
    pair: super::resident_relocation::StemPairOwner,
) -> PyResult<()> {
    enqueue_prepared_stems(
        engine,
        producer,
        ticket,
        source_version,
        stems,
        Some((wav_path.clone(), wav_path, Some(pair))),
    )
}

fn enqueue_prepared_stems(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ticket: &PreparedSourceTicket,
    source_version: &str,
    stems: crate::messages::PreparedStemSet,
    registration: Option<(
        PathBuf,
        PathBuf,
        Option<super::resident_relocation::StemPairOwner>,
    )>,
) -> PyResult<()> {
    if try_enqueue_prepared_stems(
        engine,
        producer,
        ticket,
        source_version,
        stems,
        registration,
    )? {
        Ok(())
    } else {
        Err(PyRuntimeError::new_err(
            "Failed to send PublishPreparedStems - placement pending or buffer may be full",
        ))
    }
}

pub(super) fn try_enqueue_prepared_stems(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ticket: &PreparedSourceTicket,
    source_version: &str,
    stems: crate::messages::PreparedStemSet,
    registration: Option<(
        PathBuf,
        PathBuf,
        Option<super::resident_relocation::StemPairOwner>,
    )>,
) -> PyResult<bool> {
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let id = match ticket
        .publication
        .control_placement(engine)
        .map_err(PyValueError::new_err)?
    {
        PlacementObservation::Current(current) => current.slot(),
        PlacementObservation::Pending => return Ok(false),
        _ => {
            return Err(PyValueError::new_err(
                "prepared source content key is stale",
            ));
        }
    };
    if engine.input_runtime_ownership.copy_reservation(id) != 0 {
        return Err(PyRuntimeError::new_err("pad content copy is pending"));
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let current = cache[id]
        .as_ref()
        .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
    let validation = validate_prepared_ticket(
        ticket,
        id,
        source_version,
        requests[id],
        engine
            .prepared_source_epochs
            .get(
                ticket
                    .publication
                    .content_key()
                    .ok_or_else(|| PyValueError::new_err("prepared Native key unavailable"))?
                    .cell()
                    .index(),
            )
            .ok_or_else(|| PyValueError::new_err("prepared Native epoch owner unavailable"))?,
        current,
    );
    if let Err(error) = validation {
        if matches!(
            ticket.publication.control_placement(engine),
            Ok(PlacementObservation::Pending)
        ) {
            return Ok(false);
        }
        return Err(PyValueError::new_err(error));
    }
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
    if producer.is_full() {
        return Ok(false);
    }
    if !super::resident_relocation::admit_stem_publication(
        engine,
        id,
        &stems,
        source_version,
        registration,
    )? {
        return Ok(false);
    }
    producer
        .push(ControlMessage::PublishPreparedStems { id, stems })
        .map_err(|_| {
            PyRuntimeError::new_err("Failed to send PublishPreparedStems - buffer may be full")
        })?;
    Ok(true)
}

pub(super) fn capture_prepared_source(
    engine: &AudioEngine,
    id: usize,
    source_version: String,
) -> PyResult<PreparedSourceTicket> {
    if id >= NUM_SAMPLES {
        return Err(PyValueError::new_err("id out of range"));
    }
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    capture_prepared_source_under_request_lock(engine, id, source_version, requests[id])
}

/// The caller retains the pad request mutex through its own admission check.
/// Both ordinary capture and offline-stem handoff use this publication kernel.
pub(super) fn capture_prepared_source_under_request_lock(
    engine: &AudioEngine,
    id: usize,
    source_version: String,
    request_id: u64,
) -> PyResult<PreparedSourceTicket> {
    // Reconciliation may adopt an ACKed resident window into the control cache.
    // Capture that window after reconciliation, under the same request guard.
    let timing = input_runtime_binding::capture_under_request_lock(engine, id)?;
    let sample = {
        let cache = engine
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
        cache[id]
            .clone()
            .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?
    };
    let timing = timing
        .filter(|binding| binding.available() && binding.current())
        .ok_or_else(|| PyValueError::new_err("current source timing unavailable"))?;
    let generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
    let digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
    if !digests[id]
        .as_ref()
        .is_some_and(|digest| version_matches_digest(&source_version, digest))
    {
        return Err(PyValueError::new_err(
            "source content does not match loaded source",
        ));
    }
    if generations[id].0 == 0 || generations[id].1 == 0 {
        return Err(PyValueError::new_err("loaded source identity unavailable"));
    }
    if !timing.current()
        || timing
            .ownership
            .binding_source_generation(id, timing.binding)
            != Some(generations[id].0)
    {
        return Err(PyValueError::new_err(
            "current source timing changed during capture",
        ));
    }
    let epoch = engine
        .prepared_source_epoch(id)
        .map_err(PyValueError::new_err)?
        .clone();
    let mut publication = PreparedSourcePermit::new(epoch.clone(), epoch.load(Ordering::Acquire));
    let admission = timing.clone();
    let request_incarnation = timing
        .ownership
        .binding_source_request_incarnation(id, timing.binding)
        .ok_or_else(|| PyValueError::new_err("source request incarnation unavailable"))?;
    publication.timing = Some(PreparedTimingPermit {
        id,
        binding: timing.binding,
        ownership: timing.ownership,
        acknowledgements: timing.acknowledgements,
        source_generation: generations[id].0,
    });
    Ok(PreparedSourceTicket {
        admission,
        id,
        request_id,
        request_incarnation,
        sample,
        sample_rate_hz: generations[id].1,
        source_version,
        publication,
    })
}

#[cfg(test)]
#[path = "prepared_source_content_tests.rs"]
pub(super) mod content_tests;

#[cfg(test)]
mod tests {
    use super::super::mixer;
    use super::super::stem_cache::source_version_hash;
    use super::*;
    use crate::messages::ControlParameterMessage;
    fn prepared_test_engine() -> (AudioEngine, String, SampleBuffer) {
        let engine = AudioEngine::new().unwrap();
        let sample = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.25_f32; 16].as_slice()),
        };
        engine.sample_cache.lock().unwrap()[0] = Some(sample.clone());
        engine.pad_request_ids.lock().unwrap()[0] = 7;
        engine.loaded_source_generations.lock().unwrap()[0] = (7, 44_100);
        let digest = "a".repeat(64);
        engine.loaded_source_digests.lock().unwrap()[0] = Some(digest.clone());
        engine
            .input_runtime_ownership
            .publish_test_source(0, &sample, 44_100, 7);
        (engine, format!("samples/a.wav|sha256-v1:{digest}"), sample)
    }

    fn prepared_test_set(ticket: &PreparedSourceTicket) -> crate::messages::PreparedStemSet {
        crate::messages::PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            reference_samples: ticket.sample.samples.clone(),
            reference_residency: ticket.sample.residency.clone(),
            publication: ticket.publication.clone(),
            accepted_timing: ticket.publication.accepted_projection(),
            source_version_hash: source_version_hash(&ticket.source_version),
            sample_rate_hz: ticket.sample_rate_hz,
            channels: 1,
            frame_count: 16,
            available_mask: ((1_u16 << crate::messages::STEM_BUFFER_COUNT) - 1) as u8,
            stems: std::array::from_fn(|_| ticket.sample.clone()),
        }
    }

    #[test]
    fn loaded_source_digest_is_a_copy_of_actual_loaded_metadata_without_a_file_lookup() {
        let empty = AudioEngine::new().unwrap();
        assert_eq!(empty.loaded_source_digest(0).unwrap(), None);
        assert_eq!(empty.loaded_source_digest(NUM_SAMPLES - 1).unwrap(), None);
        assert!(empty.loaded_source_digest(NUM_SAMPLES).is_err());
        let (engine, version, _) = prepared_test_engine();
        // The fixture is an actual Native source owner, with no original file
        // required by this metadata API. Worker verification remains separate.
        let mut returned = engine.loaded_source_digest(0).unwrap().unwrap();
        assert_eq!(returned, "a".repeat(64));
        returned.clear();
        assert_eq!(
            engine.loaded_source_digest(0).unwrap(),
            Some("a".repeat(64))
        );
        assert_eq!(engine.loaded_source_digest(1).unwrap(), None);
        assert!(engine.capture_prepared_source(0, version).is_ok());
        assert!(
            engine
                .capture_prepared_source(0, format!("samples/a.wav|sha256-v1:{}", "b".repeat(64)))
                .is_err()
        );
    }

    #[test]
    fn loaded_source_digest_does_not_grant_foreign_or_revoked_cache_source_authority() {
        let (engine, version, original) = prepared_test_engine();
        let foreign = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.25_f32; 16].as_slice()),
        };
        engine.sample_cache.lock().unwrap()[0] = Some(foreign);
        assert_eq!(engine.loaded_source_digest(0).unwrap(), None);
        assert!(engine.capture_prepared_source(0, version.clone()).is_err());
        engine.sample_cache.lock().unwrap()[0] = Some(original);
        assert_eq!(
            engine.loaded_source_digest(0).unwrap(),
            Some("a".repeat(64))
        );
        engine.input_runtime_ownership.revoke_source(0);
        assert_eq!(engine.loaded_source_digest(0).unwrap(), None);
        assert!(engine.capture_prepared_source(0, version).is_err());
    }

    #[test]
    fn loaded_source_digest_is_withdrawn_by_actual_unload_even_with_old_generation_metadata() {
        let (engine, version, _) = prepared_test_engine();
        let (producer, mut consumer) = rtrb::RingBuffer::new(1);
        let producer = Arc::new(Mutex::new(producer));
        super::super::cold_load::unload_for_producer(&engine, 0, &producer).unwrap();
        assert!(matches!(
            consumer.pop().unwrap(),
            ControlMessage::UnloadSample { id: 0, .. }
        ));
        assert_eq!(
            engine.loaded_source_generations.lock().unwrap()[0],
            (7, 44_100)
        );
        assert_eq!(engine.loaded_source_digest(0).unwrap(), None);
        assert!(engine.capture_prepared_source(0, version).is_err());
    }

    #[test]
    fn loaded_source_digest_survives_actual_analysis_request_advance_of_the_same_source() {
        let (engine, _, sample) = prepared_test_engine();
        let job = engine
            .offline_jobs
            .begin(
                0,
                sample,
                44_100,
                7,
                super::super::analysis_jobs::OfflineRequestOwner {
                    request_ids: engine.pad_request_ids.clone(),
                    prepared_epoch: engine.prepared_source_epochs[0].clone(),
                },
                engine.loader_tx.clone(),
            )
            .unwrap();
        assert_eq!(engine.pad_request_ids.lock().unwrap()[0], 8);
        assert_eq!(
            engine.loaded_source_generations.lock().unwrap()[0],
            (7, 44_100)
        );
        assert_eq!(
            engine.loaded_source_digest(0).unwrap(),
            Some("a".repeat(64))
        );
        job.cancel();
        assert_eq!(
            engine.loaded_source_digest(0).unwrap(),
            Some("a".repeat(64))
        );
    }

    #[test]
    fn loaded_source_digest_never_advertises_old_source_during_actual_pending_load() {
        let (mut engine, _, old_source) = prepared_test_engine();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("owned-next.wav");
        let mut wav = Vec::with_capacity(44 + 256);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36_u32 + 256).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&44_100_u32.to_le_bytes());
        wav.extend_from_slice(&88_200_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&256_u32.to_le_bytes());
        for _ in 0..128 {
            wav.extend_from_slice(&1024_i16.to_le_bytes());
        }
        std::fs::write(&path, wav).unwrap();
        let (producer, _consumer) = rtrb::RingBuffer::new(1);
        let request = super::super::cold_load::admit_for_format(
            &engine,
            0,
            path.to_string_lossy().into_owned(),
            false,
            false,
            false,
            Arc::new(Mutex::new(producer)),
            1,
            44_100,
            directory.path().join("samples"),
        )
        .unwrap();
        assert_eq!(request, 8);
        // No consumer adopts the new source. Its real loader transaction holds
        // the pending-load fence while old metadata or prepared metadata exist.
        assert_eq!(engine.loaded_source_digest(0).unwrap(), None);
        engine.cold_cancelled.store(true, Ordering::Release);
        engine.cold_jobs.shutdown();
        assert!(
            engine
                .input_runtime_ownership
                .source_current(0, &old_source, 44_100)
        );
        assert_eq!(
            engine.loaded_source_digest(0).unwrap(),
            Some("a".repeat(64))
        );
    }

    #[test]
    fn productive_prepared_enqueue_and_callback_report_actual_adoption_once() {
        let (engine, version, sample) = prepared_test_engine();
        let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
        let (producer, mut consumer) = rtrb::RingBuffer::new(1);
        let producer = Arc::new(Mutex::new(producer));
        producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
        assert!(
            enqueue_current_prepared_stems(
                &engine,
                &producer,
                &ticket,
                &version,
                prepared_test_set(&ticket)
            )
            .is_err()
        );
        assert_eq!(ticket.publication_status(), "captured");
        consumer.pop().unwrap();
        enqueue_current_prepared_stems(
            &engine,
            &producer,
            &ticket,
            &version,
            prepared_test_set(&ticket),
        )
        .unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
            panic!("publication");
        };
        let mut mixer = mixer::RtMixer::new(1, 44_100.0);
        mixer.load_sample(id, sample);
        assert!(mixer.publish_prepared_stems(id, stems));
        assert_eq!(ticket.publication_status(), "accepted");
        assert!(
            enqueue_current_prepared_stems(
                &engine,
                &producer,
                &ticket,
                &version,
                prepared_test_set(&ticket)
            )
            .is_err()
        );
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn production_capture_and_enqueue_reject_content_foreign_and_replaced_sources() {
        let (engine, version, _) = prepared_test_engine();
        assert!(
            engine
                .capture_prepared_source(0, "samples/a.wav|1|2".into())
                .is_err()
        );
        assert!(
            engine
                .capture_prepared_source(0, format!("samples/a.wav|sha256-v1:{}", "b".repeat(64)))
                .is_err()
        );
        let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
        let (other, _, _) = prepared_test_engine();
        let (producer, mut consumer) = rtrb::RingBuffer::new(1);
        let producer = Arc::new(Mutex::new(producer));
        assert!(
            enqueue_current_prepared_stems(
                &other,
                &producer,
                &ticket,
                &version,
                prepared_test_set(&ticket)
            )
            .is_err()
        );
        // Identical values and shape, distinct immutable source owner.
        engine.sample_cache.lock().unwrap()[0] = Some(SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.25_f32; 16].as_slice()),
        });
        assert!(
            enqueue_current_prepared_stems(
                &engine,
                &producer,
                &ticket,
                &version,
                prepared_test_set(&ticket)
            )
            .is_err()
        );
        assert_eq!(ticket.publication_status(), "captured");
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn queued_preparation_reports_late_timing_rejection_from_callback() {
        let (engine, version, sample) = prepared_test_engine();
        let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
        let (producer, mut consumer) = rtrb::RingBuffer::new(1);
        let producer = Arc::new(Mutex::new(producer));
        enqueue_current_prepared_stems(
            &engine,
            &producer,
            &ticket,
            &version,
            prepared_test_set(&ticket),
        )
        .unwrap();
        let (mut parameters, mut parameter_consumer) = rtrb::RingBuffer::new(1);
        push_preparation_epoch_message(
            &mut parameters,
            ControlParameterMessage::SetPadBpm {
                id: 0,
                bpm: Some(120.0),
            },
            &engine.prepared_source_epochs[0],
            "SetPadBpm",
        )
        .unwrap();
        assert!(parameter_consumer.pop().is_ok());
        let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
            panic!("publication");
        };
        let mut mixer = mixer::RtMixer::new(1, 44_100.0);
        mixer.load_sample(id, sample);
        assert!(!mixer.publish_prepared_stems(id, stems));
        assert_eq!(ticket.publication_status(), "rejected");
    }

    #[test]
    fn queued_stems_reject_real_offline_request_admission_and_cancellation() {
        for cancel_existing in [false, true] {
            let (engine, version, sample) = prepared_test_engine();
            let begin = || {
                engine.offline_jobs.begin(
                    0,
                    sample.clone(),
                    44_100,
                    7,
                    super::super::analysis_jobs::OfflineRequestOwner {
                        request_ids: engine.pad_request_ids.clone(),
                        prepared_epoch: engine.prepared_source_epochs[0].clone(),
                    },
                    engine.loader_tx.clone(),
                )
            };
            let existing_job = cancel_existing.then(|| begin().unwrap());
            let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
            let (producer, mut consumer) = rtrb::RingBuffer::new(1);
            let producer = Arc::new(Mutex::new(producer));
            enqueue_current_prepared_stems(
                &engine,
                &producer,
                &ticket,
                &version,
                prepared_test_set(&ticket),
            )
            .unwrap();
            let before_request = engine.pad_request_ids.lock().unwrap()[0];
            let before_epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
            let admitted_job = if let Some(job) = existing_job {
                job.cancel();
                job
            } else {
                begin().unwrap()
            };
            assert_eq!(
                engine.pad_request_ids.lock().unwrap()[0],
                before_request + 1
            );
            assert_eq!(
                engine.prepared_source_epochs[0].load(Ordering::Acquire),
                before_epoch + 1
            );
            let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
                panic!("publication");
            };
            let mut mixer = mixer::RtMixer::new(1, 44_100.0);
            mixer.load_sample(id, sample);
            assert!(!mixer.publish_prepared_stems(id, stems));
            assert_eq!(ticket.publication_status(), "rejected");
            admitted_job.cancel();
        }
    }

    #[test]
    fn timing_epoch_publication_is_fail_closed_and_equal_edits_invalidate() {
        let epoch = AtomicU64::new(1);
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
        let message = ControlParameterMessage::SetPadBpm {
            id: 0,
            bpm: Some(120.0),
        };
        push_preparation_epoch_message(&mut producer, message, &epoch, "SetPadBpm").unwrap();
        assert_eq!(epoch.load(Ordering::Acquire), 2);
        assert!(
            push_preparation_epoch_message(&mut producer, message, &epoch, "SetPadBpm").is_err()
        );
        assert_eq!(epoch.load(Ordering::Acquire), 2);
        consumer.pop().unwrap();
        push_preparation_epoch_message(&mut producer, message, &epoch, "SetPadBpm").unwrap();
        assert_eq!(epoch.load(Ordering::Acquire), 3);
        consumer.pop().unwrap();
        epoch.store(u64::MAX, Ordering::Release);
        assert!(
            push_preparation_epoch_message(&mut producer, message, &epoch, "SetPadBpm").is_err()
        );
        assert!(consumer.pop().is_err());
        assert_eq!(epoch.load(Ordering::Acquire), u64::MAX);
    }
}
