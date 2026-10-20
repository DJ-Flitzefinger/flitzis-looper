//! Fresh verification of a fixed restored bank. No content installation or mutation ACK.
use super::*;
use crate::audio_engine::committed_stem_residency::{
    RecoveredSelectionCustody, SelectedStemIdentity, SelectionPermit,
};
use crate::audio_engine::pad_copy::CopyRtIntent;
use crate::audio_engine::pad_pair::recovery::RecoveredCustody;
use crate::messages::PreparedStemSet;

const READY: u8 = 0;
const QUEUED: u8 = 1;
const CHECKING: u8 = 2;
const ACCEPTED: u8 = 3;
const WAITING_RESTORE: u8 = 4;
const STALE: u8 = 5;
const RELEASED: u8 = 6;

pub(crate) struct RecoveredBankOccupied {
    pub(in crate::audio_engine) intent: CopyRtIntent,
    pub(crate) selection: Option<SelectedStemIdentity>,
    pub(crate) selection_permit: Option<SelectionPermit>,
    pub(crate) stems: Option<PreparedStemSet>,
}

struct BankRecoveryCustody {
    source: RecoveredCustody,
    selection: Option<RecoveredSelectionCustody>,
    _subscription: Option<RecoverySourceSubscription>,
}

/// This subscriber is never adopted as a slot assignment. Every error and final
/// destruction retires only its own assignment through the existing off-RT path.
struct RecoverySourceSubscription {
    lease: CommittedColdLease,
}
impl Drop for RecoverySourceSubscription {
    fn drop(&mut self) {
        self.lease.release_preparation_assignment();
    }
}

pub(crate) struct RecoveredBankState {
    pub(crate) guard: BankRtGuard,
    pub(crate) images: Box<[Option<RecoveredBankOccupied>; NUM_PADS]>,
    custody: Box<[BankRecoveryCustody; NUM_PADS]>,
    physical_requests: Box<[u64]>,
    request_incarnations: Box<[Option<u64>]>,
    phase: AtomicU8,
    nonce: AtomicU64,
    accepted_nonce: AtomicU64,
    accepted_frame: AtomicU64,
}

impl RecoveredBankState {
    fn belongs_to(&self, engine: &AudioEngine) -> PyResult<()> {
        if !Arc::ptr_eq(&self.guard.ownership, &engine.input_runtime_ownership)
            || !Arc::ptr_eq(
                &self.guard.acknowledgements,
                &engine.current_timing_acknowledgements,
            )
        {
            return Err(PyValueError::new_err("foreign recovered Bank acceptance"));
        }
        Ok(())
    }

    fn current_under_request_lock(&self, requests: &[u64]) -> bool {
        self.guard.current(true)
            && self.custody.iter().enumerate().all(|(index, custody)| {
                custody.source.current_under_request_lock(
                    &self.guard.ownership,
                    self.guard.targets[index],
                    requests,
                    self.physical_requests[index],
                    self.request_incarnations[index],
                ) && custody
                    .selection
                    .as_ref()
                    .is_none_or(RecoveredSelectionCustody::current_reserved)
            })
    }

    pub(crate) fn begin_probe(&self, nonce: u64) -> bool {
        self.nonce.load(Ordering::Acquire) == nonce
            && self
                .phase
                .compare_exchange(QUEUED, CHECKING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    pub(crate) fn checking(&self, nonce: u64) -> bool {
        self.phase.load(Ordering::Acquire) == CHECKING
            && self.nonce.load(Ordering::Acquire) == nonce
    }

    /// Only the callback writes a successful receipt, inside the complete marker.
    pub(crate) fn accept(&self, nonce: u64, frame: u64) {
        self.accepted_frame.store(frame, Ordering::Relaxed);
        self.accepted_nonce.store(nonce, Ordering::Relaxed);
        self.phase.store(ACCEPTED, Ordering::Release);
    }

    pub(crate) fn finish_probe(&self, nonce: u64, stale: bool) {
        if self.nonce.load(Ordering::Acquire) == nonce {
            let _ = self.phase.compare_exchange(
                CHECKING,
                if stale { STALE } else { WAITING_RESTORE },
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }

    pub(crate) fn defer_probe(&self, nonce: u64) {
        if self.nonce.load(Ordering::Acquire) == nonce {
            let _ =
                self.phase
                    .compare_exchange(CHECKING, QUEUED, Ordering::AcqRel, Ordering::Acquire);
        }
    }

    fn status(&self) -> &'static str {
        match self.phase.load(Ordering::Acquire) {
            QUEUED => "pending",
            CHECKING => "checking",
            ACCEPTED => "accepted",
            WAITING_RESTORE => "waiting_restore",
            STALE => "stale",
            RELEASED => "released",
            _ => "captured",
        }
    }
}

/// A whole queued probe always leaves through the existing off-realtime worker.
#[derive(Clone)]
pub(crate) struct RecoveredBankProbe {
    pub(crate) state: Arc<RecoveredBankState>,
    pub(crate) nonce: u64,
}
impl std::fmt::Debug for RecoveredBankProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveredBankProbe")
            .field("owner", &self.state.guard.owner)
            .field("nonce", &self.nonce)
            .finish_non_exhaustive()
    }
}
impl Drop for RecoveredBankProbe {
    fn drop(&mut self) {
        // Shutdown/queued retirement is not successful callback verification.
        // The callback's terminal phases make this an inert off-thread tail.
        if self.state.nonce.load(Ordering::Acquire) == self.nonce {
            let _ = self.state.phase.compare_exchange(
                QUEUED,
                STALE,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}

/// Process-local original guards, independent source/pair custody and fresh receipt.
/// Metadata and historical journals cannot construct or replace this owner.
#[pyclass(frozen)]
pub struct RecoveredBankAcceptance {
    pub(crate) state: Arc<RecoveredBankState>,
}

#[pymethods]
impl RecoveredBankAcceptance {
    pub fn metadata_json(&self) -> String {
        let state = &self.state;
        json!({"owner":state.guard.owner, "target_bank":state.guard.targets[0].slot() / NUM_PADS,
            "status":state.status(), "nonce":state.nonce.load(Ordering::Acquire),
            "accepted_nonce":state.accepted_nonce.load(Ordering::Acquire),
            "accepted_output_frame":(state.phase.load(Ordering::Acquire) == ACCEPTED)
                .then(|| state.accepted_frame.load(Ordering::Acquire)),
            "released":state.phase.load(Ordering::Acquire) == RELEASED})
        .to_string()
    }
}

fn occupied_image(
    engine: &AudioEngine,
    participant: &PairParticipantGuard,
    json: &str,
) -> PyResult<(RecoveredBankOccupied, BankRecoveryCustody, Option<u64>)> {
    let id = participant.placement().slot();
    let current = input_runtime_binding::capture_under_request_lock(engine, id)?
        .filter(|binding| binding.available() && binding.current())
        .ok_or_else(|| PyValueError::new_err("recovered Bank source unavailable"))?;
    if !engine
        .input_runtime_ownership
        .binding_source_adoption_accepted(id, current.binding)
    {
        return Err(PyValueError::new_err(
            "recovered Bank source/timing is not callback accepted",
        ));
    }
    let intent = CopyIntent::parse(json).map_err(PyValueError::new_err)?;
    let (timing_intent, timing) = current.copy_timing_context();
    if intent.timing_intent != timing_intent {
        return Err(PyValueError::new_err(
            "recovered Bank timing intent differs",
        ));
    }
    if timing_intent == flitzis_looper_analysis::tempo_acceptance::TimingIntent::Automatic {
        let saved = &intent.snapshot["analysis"]["accepted_timing"];
        if timing
            .as_ref()
            .is_none_or(|record| !record.recovered_image_matches_saved(saved).unwrap_or(false))
        {
            return Err(PyValueError::new_err(
                "recovered Bank saved/current Automatic evidence differs",
            ));
        }
    }
    let sample = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?[id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("recovered Bank source custody missing"))?;
    let subscription = RecoverySourceSubscription {
        lease: engine
            .cold_leases
            .lock()
            .map_err(|_| PyRuntimeError::new_err("cold custody lock poisoned"))?[id]
            .as_ref()
            .ok_or_else(|| PyValueError::new_err("recovered Bank source lease missing"))?
            .subscribe_verified()
            .map_err(|error| PyValueError::new_err(error.to_string()))?,
    };
    let lease = &subscription.lease;
    let source_identity = &intent.snapshot["source_identity"];
    if !source_identity["material_creation"].is_null()
        && source_identity["material_creation"]
            != serde_json::to_value(lease.material_creation_receipt())
                .map_err(|error| PyValueError::new_err(error.to_string()))?
    {
        return Err(PyValueError::new_err(
            "recovered Bank historical material birth differs",
        ));
    }
    if source_identity["material_id"]
        .as_str()
        .is_some_and(|expected| lease.material_id.as_deref() != Some(expected))
    {
        return Err(PyValueError::new_err(
            "recovered Bank material identity differs",
        ));
    }
    let saved_path = intent.snapshot["source_path"]
        .as_str()
        .ok_or_else(|| PyValueError::new_err("recovered Bank source path missing"))?;
    let resolved = crate::audio_engine::material_paths::resolve(
        lease.samples_root(),
        std::path::Path::new(saved_path),
    )
    .map_err(|error| PyValueError::new_err(error.to_string()))?;
    if resolved.path != lease.original_path {
        return Err(PyValueError::new_err(
            "recovered Bank source path differs from held custody",
        ));
    }
    let descriptor = intent.descriptor().map_err(PyValueError::new_err)?;
    let expected_version = intent.snapshot["stem_cache"]["source_version"].as_str();
    let selection = committed_stem_residency::capture_recovered_selection(
        engine,
        &current,
        descriptor,
        expected_version,
    )
    .map_err(PyValueError::new_err)?;
    let mut rt = intent
        .rt_intent(
            &sample,
            current.binding.sample_rate_hz,
            current.binding.accepted,
        )
        .map_err(PyValueError::new_err)?;
    let stems = selection
        .as_ref()
        .and_then(RecoveredSelectionCustody::components);
    if rt.stem_mix_mode == StemMixMode::AllStems {
        rt.source_version_hash = selection
            .as_ref()
            .map_or(0, RecoveredSelectionCustody::source_version_hash);
        if stems.is_none() || rt.source_version_hash == 0 {
            return Err(PyValueError::new_err(
                "recovered Bank selected components are not accepted",
            ));
        }
    }
    let incarnation = engine
        .input_runtime_ownership
        .binding_source_request_incarnation(id, current.binding)
        .ok_or_else(|| PyValueError::new_err("recovered Bank request incarnation missing"))?;
    let image = RecoveredBankOccupied {
        intent: rt,
        selection: selection.as_ref().map(RecoveredSelectionCustody::selected),
        selection_permit: selection.as_ref().map(RecoveredSelectionCustody::permit),
        stems: stems.clone(),
    };
    let custody = BankRecoveryCustody {
        source: RecoveredCustody::new(Some(current), Some(sample), Some(lease.clone()), None),
        selection,
        _subscription: Some(subscription),
    };
    Ok((image, custody, Some(incarnation)))
}

pub(in crate::audio_engine) fn capture(
    engine: &AudioEngine,
    original: &BankParticipants,
    intents: Vec<Option<String>>,
) -> PyResult<RecoveredBankAcceptance> {
    if original.source_bank.is_some()
        || original.participants.len() != NUM_PADS
        || intents.len() != NUM_PADS
    {
        return Err(PyValueError::new_err(
            "recovered Bank requires original target-only participants and exactly36 images",
        ));
    }
    if !Arc::ptr_eq(&original.ownership, &engine.input_runtime_ownership)
        || !Arc::ptr_eq(
            &original.acknowledgements,
            &engine.current_timing_acknowledgements,
        )
    {
        return Err(PyValueError::new_err("foreign recovered Bank participants"));
    }
    // Reconcile established collectors before capture, never reconstruct or prepare a pair.
    crate::audio_engine::resident_relocation::reconcile(engine)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let ownership = &engine.input_runtime_ownership;
    let token = ownership
        .placement_read_token()
        .ok_or_else(|| PyValueError::new_err("recovered Bank placement pending"))?;
    if !original.current(engine, &requests) {
        return Err(PyValueError::new_err(
            "recovered Bank participants are foreign or stale",
        ));
    }
    if ownership
        .admitted_launch_ids()
        .iter()
        .any(|slot| (*slot / NUM_PADS) == original.target_bank)
    {
        return Err(PyValueError::new_err(
            "recovered Bank has admitted target playback",
        ));
    }
    let mut images = Vec::with_capacity(NUM_PADS);
    let mut custody = Vec::with_capacity(NUM_PADS);
    let mut incarnations = Vec::with_capacity(NUM_PADS);
    for (index, participant) in original.participants.iter().enumerate() {
        let slot = participant.placement().slot();
        if slot != original.target_bank * NUM_PADS + index
            || ownership.resident_control_pending(slot)
        {
            return Err(PyValueError::new_err(
                "recovered Bank participant has unsettled restore work",
            ));
        }
        if !engine
            .resident_stem_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident custody lock poisoned"))?[slot]
            .recovered_custody_settled()
        {
            return Err(PyValueError::new_err(
                "recovered Bank window custody is unsettled",
            ));
        }
        match (&intents[index], participant.placement().lifetime()) {
            (Some(json), lifetime) if lifetime != 0 => {
                let (image, held, incarnation) = occupied_image(engine, participant, json)?;
                images.push(Some(image));
                custody.push(held);
                incarnations.push(incarnation);
            }
            (None, 0) => {
                // These are genuine control Empty incarnations; callback absence is checked later.
                super::participant(engine, slot)?;
                committed_stem_residency::capture_recovered_empty_selection(engine, slot)
                    .map_err(PyValueError::new_err)?;
                images.push(None);
                custody.push(BankRecoveryCustody {
                    source: RecoveredCustody::new(None, None, None, None),
                    selection: None,
                    _subscription: None,
                });
                incarnations.push(None);
            }
            _ => {
                return Err(PyValueError::new_err(
                    "recovered Bank fixed image occupancy differs",
                ));
            }
        }
    }
    let owner =
        crate::audio_engine::content_lifetime::reserve().map_err(PyRuntimeError::new_err)?;
    let targets: [SlotSnapshot; NUM_PADS] = original
        .participants
        .iter()
        .map(PairParticipantGuard::placement)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| PyValueError::new_err("recovered Bank target count"))?;
    let state = Arc::new(RecoveredBankState {
        guard: BankRtGuard {
            participants: original.participants.clone(),
            requests: original.requests.clone(),
            reservations: targets.to_vec().into_boxed_slice(),
            targets,
            owner,
            phase: Arc::new(AtomicU8::new(0)),
            revision: AtomicU64::new(0),
            ownership: ownership.clone(),
            acknowledgements: original.acknowledgements.clone(),
        },
        images: images
            .try_into()
            .map_err(|_| PyValueError::new_err("recovered Bank image count"))?,
        custody: custody
            .try_into()
            .map_err(|_| PyValueError::new_err("recovered Bank custody count"))?,
        physical_requests: original.physical_requests.clone(),
        request_incarnations: incarnations.into_boxed_slice(),
        phase: AtomicU8::new(READY),
        nonce: AtomicU64::new(1),
        accepted_nonce: AtomicU64::new(0),
        accepted_frame: AtomicU64::new(0),
    });
    let probe = Box::new(RecoveredBankProbe {
        state: state.clone(),
        nonce: 1,
    });
    let producer = engine.control_producer()?;
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "recovered Bank command queue is full",
        ));
    }
    if ownership.placement_read_token() != Some(token) || !original.current(engine, &requests) {
        return Err(PyValueError::new_err(
            "recovered Bank participants changed during capture",
        ));
    }
    ownership
        .reserve_copy_set(&state.guard.reservations, owner, || {
            original.current(engine, &requests)
        })
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    state.phase.store(QUEUED, Ordering::Release);
    producer
        .push(ControlMessage::VerifyRecoveredBank(probe))
        .expect("reserved single producer capacity");
    Ok(RecoveredBankAcceptance { state })
}

pub(in crate::audio_engine) fn validate(
    engine: &AudioEngine,
    acceptance: &RecoveredBankAcceptance,
) -> PyResult<bool> {
    let state = &acceptance.state;
    state.belongs_to(engine)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let Some(token) = state.guard.ownership.placement_read_token() else {
        return Ok(false);
    };
    let valid = state.phase.load(Ordering::Acquire) == ACCEPTED
        && state.accepted_nonce.load(Ordering::Acquire) == state.nonce.load(Ordering::Acquire)
        && state.current_under_request_lock(&requests);
    Ok(valid && state.guard.ownership.placement_read_token() == Some(token))
}

pub(in crate::audio_engine) fn retry(
    engine: &AudioEngine,
    acceptance: &RecoveredBankAcceptance,
) -> PyResult<bool> {
    let state = &acceptance.state;
    state.belongs_to(engine)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    if state.phase.load(Ordering::Acquire) != WAITING_RESTORE {
        return Ok(false);
    }
    if !state.current_under_request_lock(&requests) {
        state.phase.store(STALE, Ordering::Release);
        return Ok(false);
    }
    let nonce = state
        .nonce
        .load(Ordering::Acquire)
        .checked_add(1)
        .ok_or_else(|| PyRuntimeError::new_err("recovered Bank probe nonce exhausted"))?;
    let probe = Box::new(RecoveredBankProbe {
        state: state.clone(),
        nonce,
    });
    let producer = engine.control_producer()?;
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "recovered Bank command queue is full",
        ));
    }
    let anchor = state
        .guard
        .ownership
        .slot_snapshot(state.guard.targets[0].slot())
        .ok_or_else(|| PyRuntimeError::new_err("recovered Bank retry placement pending"))?;
    let write = state
        .guard
        .ownership
        .prepare_slot_write_fixed(anchor, anchor.lifetime())
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    state
        .guard
        .ownership
        .commit_slot_write_fixed(
            &write,
            || state.guard.current(true) && state.phase.load(Ordering::Acquire) == WAITING_RESTORE,
            |_| {
                state.nonce.store(nonce, Ordering::Relaxed);
                state.phase.store(QUEUED, Ordering::Release);
            },
        )
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    producer
        .push(ControlMessage::VerifyRecoveredBank(probe))
        .expect("reserved single producer capacity");
    Ok(true)
}

pub(in crate::audio_engine) fn release(
    engine: &AudioEngine,
    acceptance: &RecoveredBankAcceptance,
) -> PyResult<bool> {
    let state = &acceptance.state;
    state.belongs_to(engine)?;
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    match state.phase.load(Ordering::Acquire) {
        RELEASED => return Ok(true),
        QUEUED | CHECKING => return Ok(false),
        _ => {}
    }
    state
        .guard
        .ownership
        .release_copy_set(&state.guard.reservations, state.guard.owner)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    state.phase.store(RELEASED, Ordering::Release);
    Ok(true)
}
