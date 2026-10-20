//! One Native bank owner. Child images are private preparation, never independent transactions.
use super::*;
use crate::audio_engine::constants::{MAX_VOICES, NUM_BANKS, NUM_PADS};
use crate::audio_engine::content_placement::{PreparedSlotSetWrite, SlotSnapshot};
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
use crate::audio_engine::pad_pair::{PairParticipantGuard, PreparedPairEmptyImage};
use std::cell::UnsafeCell;
use std::sync::atomic::AtomicU64;

pub(crate) mod recovery;
pub use recovery::RecoveredBankAcceptance;

/// Original complete admission witnesses. This owns scalar guards only: no reservation,
/// prepared audio, source lease, publication phase or right to mutate a participant.
#[pyclass(frozen)]
pub struct BankParticipants {
    source_bank: Option<usize>,
    target_bank: usize,
    participants: Box<[PairParticipantGuard]>,
    requests: Box<[u64]>,
    physical_requests: Box<[u64]>,
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<constant_timing::CurrentTimingAcknowledgements>,
}

impl BankParticipants {
    fn current(&self, engine: &AudioEngine, requests: &[u64]) -> bool {
        Arc::ptr_eq(&self.ownership, &engine.input_runtime_ownership)
            && Arc::ptr_eq(
                &self.acknowledgements,
                &engine.current_timing_acknowledgements,
            )
            && self
                .participants
                .iter()
                .enumerate()
                .all(|(index, participant)| {
                    participant.current(&self.ownership, &self.acknowledgements, 0)
                        && requests[participant.placement().slot()] == self.physical_requests[index]
                        && self
                            .ownership
                            .source_request_for_cell(participant.placement().cell())
                            == self.requests[index]
                })
    }
}

pub(crate) struct BankRtGuard {
    pub(crate) participants: Box<[PairParticipantGuard]>,
    requests: Box<[u64]>,
    pub(crate) reservations: Box<[SlotSnapshot]>,
    pub(crate) targets: [SlotSnapshot; NUM_PADS],
    pub(crate) owner: u64,
    pub(crate) phase: Arc<AtomicU8>,
    pub(crate) revision: AtomicU64,
    pub(crate) ownership: Arc<InputRuntimeOwnership>,
    pub(crate) acknowledgements: Arc<constant_timing::CurrentTimingAcknowledgements>,
}

impl BankRtGuard {
    pub(crate) fn current(&self, reserved: bool) -> bool {
        self.participants
            .iter()
            .zip(&self.requests)
            .all(|(participant, request)| {
                (if reserved {
                    participant.current_for_copy(
                        &self.ownership,
                        &self.acknowledgements,
                        self.owner,
                    )
                } else {
                    participant.current(&self.ownership, &self.acknowledgements, 0)
                }) && self
                    .ownership
                    .source_request_for_cell(participant.placement().cell())
                    == *request
            })
    }

    pub(crate) fn claim(&self) -> bool {
        self.current(true)
            && self
                .phase
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    pub(crate) fn reject(&self) {
        let _ = self
            .phase
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub(crate) struct BankEmptyRtImage {
    pub(crate) target: usize,
    pub(crate) authority: u64,
    pub(crate) epoch: u64,
    pub(crate) epoch_owner: Arc<AtomicU64>,
    pub(crate) empty: PreparedPairEmptyImage,
    pub(crate) retired_sample: Option<SampleBuffer>,
    pub(crate) retired_stems: Option<crate::messages::PreparedStemSet>,
    pub(crate) retired_outgoing: Option<crate::messages::PreparedStemSet>,
    pub(crate) retired_receipt:
        Option<crate::audio_engine::committed_stem_residency::StemModeReceipt>,
}

pub(crate) enum BankRtImage {
    Copy(Box<PadCopyRtWork>),
    Empty(Box<BankEmptyRtImage>),
}

pub(crate) struct BankMutationRtWork {
    pub(crate) guard: Arc<BankRtGuard>,
    pub(crate) writes: PreparedSlotSetWrite<NUM_PADS>,
    pub(crate) images: Box<[BankRtImage; NUM_PADS]>,
    pub(crate) publication: Option<Arc<BankMutationPublication>>,
}

pub(crate) struct BankMutationPublication {
    claimed: AtomicBool,
    work: UnsafeCell<Option<Box<BankMutationRtWork>>>,
}
// The sole successful atomic claimant transfers the control-initialized owner.
unsafe impl Send for BankMutationPublication {}
unsafe impl Sync for BankMutationPublication {}
impl std::fmt::Debug for BankMutationPublication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BankMutationPublication")
            .finish_non_exhaustive()
    }
}
impl BankMutationPublication {
    pub(crate) fn take(&self) -> Option<Box<BankMutationRtWork>> {
        self.claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        unsafe { (&mut *self.work.get()).take() }
    }
    /// The sole callback consumer may return an entirely unclaimed payload on marker contention.
    pub(crate) fn restore_unclaimed(&self, work: Box<BankMutationRtWork>) {
        debug_assert!(matches!(work.guard.phase.load(Ordering::Acquire), 0 | 3));
        debug_assert!(work.publication.is_none());
        unsafe {
            *self.work.get() = Some(work);
        }
        self.claimed.store(false, Ordering::Release);
    }
}
impl Drop for BankMutationPublication {
    fn drop(&mut self) {
        if let Some(work) = self.work.get_mut().as_ref() {
            work.guard.reject();
        }
    }
}

impl std::fmt::Debug for BankMutationRtWork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BankMutationRtWork")
            .field("owner", &self.guard.owner)
            .finish_non_exhaustive()
    }
}

impl BankMutationRtWork {
    pub(crate) fn retirement_slots_needed(&self) -> usize {
        3 * MAX_VOICES + NUM_PADS + 1
    }
}

struct TargetCapture {
    request: u64,
    next_request: u64,
    next_epoch: u64,
    authority: u64,
    epoch: Arc<AtomicU64>,
    previous_sample: Option<SampleBuffer>,
    previous_lease: Option<CommittedColdLease>,
}

struct BankOwners {
    copies: Vec<Option<PreparedCopyOwners>>,
    rt: Option<Box<BankMutationRtWork>>,
    retired_resident: Vec<ResidentStemCache>,
}

pub(in crate::audio_engine) struct BankMutationState {
    pub(crate) guard: Arc<BankRtGuard>,
    source_bank: Option<usize>,
    target_bank: usize,
    format: (u32, usize),
    copies: Vec<Option<Arc<PadCopyState>>>,
    targets: [TargetCapture; NUM_PADS],
    stage: AtomicU8,
    worker_running: AtomicBool,
    mirrored: AtomicBool,
    prepared: Mutex<Option<BankOwners>>,
}

impl BankMutationState {
    fn belongs_to(&self, engine: &AudioEngine) -> PyResult<()> {
        if Arc::ptr_eq(&self.guard.ownership, &engine.input_runtime_ownership) {
            Ok(())
        } else {
            Err(PyValueError::new_err("foreign bank transaction"))
        }
    }

    fn status(&self) -> &'static str {
        match self.guard.phase.load(Ordering::Acquire) {
            1 => "adopting",
            2 => "accepted",
            3 if self.worker_running.load(Ordering::Acquire) => "cancelling",
            3 => "rejected",
            _ => match self.stage.load(Ordering::Acquire) {
                1 => "preparing",
                2 => "prepared",
                3 => "pending",
                _ => "captured",
            },
        }
    }
}

/// Off-realtime completion is later than every owned preparation result and cleanup.
struct BankPreparationCompletion(Arc<BankMutationState>);
impl Drop for BankPreparationCompletion {
    fn drop(&mut self) {
        if self.0.stage.load(Ordering::Acquire) == 1 {
            self.0.guard.reject();
            self.0.stage.store(4, Ordering::Release);
        }
        self.0.worker_running.store(false, Ordering::Release);
    }
}

#[pyclass(frozen)]
pub struct CapturedBankMutation {
    state: Arc<BankMutationState>,
}
#[pymethods]
impl CapturedBankMutation {
    pub fn publication_status(&self) -> &'static str {
        self.state.status()
    }
}

#[pyclass(frozen)]
pub struct BankMutationTicket {
    state: Arc<BankMutationState>,
}
#[pyclass(frozen)]
pub struct BankMutationAcknowledgement {
    state: Arc<BankMutationState>,
}

#[pymethods]
impl BankMutationTicket {
    pub fn publication_status(&self) -> &'static str {
        self.state.status()
    }
    pub fn acknowledgement(&self) -> Option<BankMutationAcknowledgement> {
        (self.state.guard.phase.load(Ordering::Acquire) == 2).then(|| BankMutationAcknowledgement {
            state: self.state.clone(),
        })
    }
    pub fn metadata_json(&self) -> String {
        // Like Copy, availability follows the actual prepared component publication,
        // independently of a registration ticket. Observe every row under one lock.
        let owners = self.state.prepared.lock().ok();
        let rows: Vec<_> = self.state.guard.targets.iter().enumerate().map(|(index, before)| {
            let source = self.state.source_bank.map(|_| {
                let participant = &self.state.guard.participants[index];
                let snapshot = participant.placement();
                json!({"slot":snapshot.slot(), "cell":snapshot.cell().index(), "lifetime":snapshot.lifetime(),
                    "binding_epoch":snapshot.binding_epoch(), "source_generation":participant.control.source_generation(),
                    "authority_revision":participant.control.authority_revision(), "runtime_revision":participant.control.runtime_revision(),
                    "request_incarnation":self.state.guard.requests[index], "prepared_epoch":participant.captured_preparation_epoch,
                    "selection_fences":participant.selection_fences.iter().map(|(_, expected)| *expected).collect::<Vec<_>>()})
            });
            let lifetime = self.state.copies[index].as_ref().map_or(0, |copy| copy.capture.lifetime);
            let components_available = owners.as_ref()
                .and_then(|owners| owners.as_ref())
                .and_then(|owners| owners.copies[index].as_ref())
                .and_then(|copy| copy.selection.as_ref())
                .and_then(|selection| selection.stem_owner.as_ref())
                .is_some_and(|owner| owner.set.publication.status() == "accepted");
            json!({"slot":before.slot(), "cell":before.cell().index(), "before_lifetime":before.lifetime(),
                "before_binding_epoch":before.binding_epoch(), "after_lifetime":lifetime,
                "after_binding_epoch":before.binding_epoch() + u64::from(before.lifetime() != lifetime),
                "source_generation":self.state.copies[index].as_ref().map_or(0, |copy| copy.capture.request),
                "resolved_key_lock":self.state.copies[index].as_ref().map(|copy| copy.capture.intent.key_lock),
                "components_available":components_available,
                "source":source})
        }).collect();
        json!({"owner":self.state.guard.owner, "status":self.state.status(), "source_bank":self.state.source_bank,
            "target_bank":self.state.target_bank, "layout_revision":self.state.guard.revision.load(Ordering::Acquire),
            "control_mirrored":self.state.mirrored.load(Ordering::Acquire), "targets":rows}).to_string()
    }
    pub fn committed_stem_tickets(&self) -> PyResult<Vec<Option<CommittedStemSetTicket>>> {
        if !self.state.mirrored.load(Ordering::Acquire) {
            return Err(PyValueError::new_err("bank mirror is pending"));
        }
        let owners = self
            .state
            .prepared
            .lock()
            .map_err(|_| PyRuntimeError::new_err("bank preparation lock poisoned"))?;
        Ok(owners
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("bank owners missing"))?
            .copies
            .iter()
            .map(|copy| copy.as_ref().and_then(|copy| copy.selection_ticket.clone()))
            .collect())
    }
}

fn participant(engine: &AudioEngine, id: usize) -> PyResult<PairParticipantGuard> {
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(id)?;
    let current = input_runtime_binding::capture_under_request_lock(engine, id)?;
    let snapshot = engine
        .input_runtime_ownership
        .slot_snapshot(id)
        .ok_or_else(|| PyValueError::new_err("bank placement pending"))?;
    let binding = current.as_ref().map(|current| {
        let mut binding = current.binding;
        binding.runtime_revision = engine.input_runtime_ownership.runtime_revision(id);
        binding
    });
    if snapshot.lifetime() == 0 {
        let cache = engine
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("cache lock poisoned"))?;
        let leases = engine
            .cold_leases
            .lock()
            .map_err(|_| PyRuntimeError::new_err("lease lock poisoned"))?;
        if cache[id].is_some() || leases[id].is_some() {
            return Err(PyValueError::new_err(
                "bank empty control custody is unsettled",
            ));
        }
    }
    if current
        .as_ref()
        .is_some_and(|binding| !binding.current() || !binding.available())
        || engine.cold_loading[id].load(Ordering::Acquire) != 0
        || engine
            .loading_sample_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?
            .contains(&id)
    {
        return Err(PyValueError::new_err(
            "bank participant has pending source work",
        ));
    }
    let resident = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident lock poisoned"))?;
    let mut fences = committed_stem_residency::capture_pair_selection_fences(&resident[id]);
    fences.extend(
        engine
            .resident_proposals
            .capture_pair_fences_for_cell(snapshot.cell())
            .ok_or_else(|| PyValueError::new_err("bank proposals changing"))?,
    );
    PairParticipantGuard::capture(
        &engine.input_runtime_ownership,
        &engine.current_timing_acknowledgements,
        snapshot,
        binding,
        engine
            .prepared_source_epoch(id)
            .map_err(PyRuntimeError::new_err)?
            .clone(),
        fences,
    )
    .map_err(|error| PyValueError::new_err(error.to_string()))
}

fn validate_banks(source_bank: Option<usize>, target_bank: usize) -> PyResult<()> {
    if target_bank >= NUM_BANKS
        || source_bank.is_some_and(|bank| bank >= NUM_BANKS || bank == target_bank)
    {
        return Err(PyValueError::new_err(
            "bank participants require distinct valid banks",
        ));
    }
    Ok(())
}

fn capture_participants_under_request_lock(
    engine: &AudioEngine,
    source_bank: Option<usize>,
    target_bank: usize,
    requests: &[u64],
) -> PyResult<BankParticipants> {
    let token = engine
        .input_runtime_ownership
        .placement_read_token()
        .ok_or_else(|| PyValueError::new_err("bank participant placement pending"))?;
    let mut participants = Vec::with_capacity(2 * NUM_PADS);
    if let Some(bank) = source_bank {
        for index in 0..NUM_PADS {
            participants.push(participant(engine, bank * NUM_PADS + index)?);
        }
    }
    for index in 0..NUM_PADS {
        participants.push(participant(engine, target_bank * NUM_PADS + index)?);
    }
    let captured = BankParticipants {
        source_bank,
        target_bank,
        requests: participants
            .iter()
            .map(|participant| {
                engine
                    .input_runtime_ownership
                    .source_request_for_cell(participant.placement().cell())
            })
            .collect(),
        physical_requests: participants
            .iter()
            .map(|participant| requests[participant.placement().slot()])
            .collect(),
        participants: participants.into_boxed_slice(),
        ownership: engine.input_runtime_ownership.clone(),
        acknowledgements: engine.current_timing_acknowledgements.clone(),
    };
    if !captured.current(engine, requests)
        || engine.input_runtime_ownership.placement_read_token() != Some(token)
    {
        return Err(PyValueError::new_err(
            "bank participants changed during capture",
        ));
    }
    Ok(captured)
}

pub(in crate::audio_engine) fn capture_participants(
    engine: &AudioEngine,
    source_bank: Option<usize>,
    target_bank: usize,
) -> PyResult<BankParticipants> {
    validate_banks(source_bank, target_bank)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    capture_participants_under_request_lock(engine, source_bank, target_bank, &requests)
}

pub(in crate::audio_engine) fn capture(
    engine: &AudioEngine,
    source_bank: Option<usize>,
    target_bank: usize,
    intents: Vec<Option<String>>,
) -> PyResult<CapturedBankMutation> {
    capture_with_participants(engine, source_bank, target_bank, intents, None)
}

pub(in crate::audio_engine) fn capture_with_participants(
    engine: &AudioEngine,
    source_bank: Option<usize>,
    target_bank: usize,
    intents: Vec<Option<String>>,
    expected: Option<&BankParticipants>,
) -> PyResult<CapturedBankMutation> {
    validate_banks(source_bank, target_bank)?;
    if intents.len() != NUM_PADS {
        return Err(PyValueError::new_err(
            "bank transaction requires distinct valid banks and exactly36 intents",
        ));
    }
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let token = engine
        .input_runtime_ownership
        .placement_read_token()
        .ok_or_else(|| PyValueError::new_err("bank participant placement pending"))?;
    let fresh;
    let original = if let Some(expected) = expected {
        if expected.source_bank != source_bank
            || expected.target_bank != target_bank
            || !expected.current(engine, &requests)
            || engine.input_runtime_ownership.placement_read_token() != Some(token)
        {
            return Err(PyValueError::new_err(
                "bank participants are foreign, mismatched or stale",
            ));
        }
        expected
    } else {
        fresh =
            capture_participants_under_request_lock(engine, source_bank, target_bank, &requests)?;
        &fresh
    };
    // Keep the original guards, rather than substituting a recaptured equal-looking
    // source or Empty. The complete placement marker validates these again below.
    let participants = original.participants.to_vec();
    let format = engine.ordinary_start_render_format()?;
    let owner =
        crate::audio_engine::content_lifetime::reserve().map_err(PyRuntimeError::new_err)?;
    let phase = Arc::new(AtomicU8::new(0));
    let target_offset = participants.len() - NUM_PADS;
    let snapshots = std::array::from_fn(|index| participants[target_offset + index].placement());
    let mut copies = Vec::with_capacity(NUM_PADS);
    let mut targets = Vec::with_capacity(NUM_PADS);
    for (index, intent) in intents.iter().enumerate() {
        let target = target_bank * NUM_PADS + index;
        let source = source_bank.map(|bank| bank * NUM_PADS + index);
        let copy = if let Some(source) =
            source.filter(|_| participants[index].placement().lifetime() != 0)
        {
            let binding = input_runtime_binding::capture_under_request_lock(engine, source)?
                .ok_or_else(|| PyValueError::new_err("bank source disappeared"))?;
            let target_binding = input_runtime_binding::capture_under_request_lock(engine, target)?;
            Some(capture_image(
                engine,
                &binding,
                target,
                intent.as_deref().ok_or_else(|| {
                    PyValueError::new_err("occupied bank source lacks musical intent")
                })?,
                target_binding.as_ref(),
                &requests,
                Some((owner, phase.clone())),
            )?)
        } else {
            if intent.is_some() {
                return Err(PyValueError::new_err(
                    "empty bank source has an occupied intent",
                ));
            }
            None
        };
        let epoch = engine
            .prepared_source_epoch(target)
            .map_err(PyRuntimeError::new_err)?
            .clone();
        let cache = engine
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("cache lock poisoned"))?;
        let leases = engine
            .cold_leases
            .lock()
            .map_err(|_| PyRuntimeError::new_err("lease lock poisoned"))?;
        if snapshots[index].lifetime() != 0 && (cache[target].is_none() || leases[target].is_none())
        {
            return Err(PyValueError::new_err("bank target custody missing"));
        }
        targets.push(TargetCapture {
            request: requests[target],
            next_request: requests[target]
                .checked_add(1)
                .filter(|request| *request <= u64::MAX / 4)
                .ok_or_else(|| PyValueError::new_err("bank request exhausted"))?,
            next_epoch: next_epoch(&epoch).map_err(PyRuntimeError::new_err)?,
            authority: engine.input_runtime_ownership.next_authority(target)?,
            epoch,
            previous_sample: cache[target].clone(),
            previous_lease: leases[target].clone(),
        });
        copies.push(copy);
    }
    let guard = Arc::new(BankRtGuard {
        requests: original.requests.clone(),
        reservations: participants
            .iter()
            .map(PairParticipantGuard::placement)
            .collect(),
        participants: participants.into_boxed_slice(),
        targets: snapshots,
        owner,
        phase,
        revision: AtomicU64::new(0),
        ownership: engine.input_runtime_ownership.clone(),
        acknowledgements: engine.current_timing_acknowledgements.clone(),
    });
    let state = Arc::new(BankMutationState {
        guard,
        source_bank,
        target_bank,
        format,
        copies,
        targets: targets
            .try_into()
            .map_err(|_| PyValueError::new_err("bank target count"))?,
        stage: AtomicU8::new(0),
        worker_running: AtomicBool::new(false),
        mirrored: AtomicBool::new(false),
        prepared: Mutex::new(None),
    });
    let mut states = engine
        .bank_mutations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("bank ownership lock poisoned"))?;
    engine
        .input_runtime_ownership
        .reserve_copy_set(&state.guard.reservations, owner, || {
            original.current(engine, &requests) && state.guard.current(false)
        })
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    states.push(state.clone());
    Ok(CapturedBankMutation { state })
}

pub(in crate::audio_engine) fn prepare(
    engine: &AudioEngine,
    captured: &CapturedBankMutation,
) -> PyResult<Arc<BankMutationState>> {
    let state = &captured.state;
    state.belongs_to(engine)?;
    // Admission, cancellation and terminal result publication share this boundary.
    let _prepared = state
        .prepared
        .lock()
        .map_err(|_| PyRuntimeError::new_err("bank preparation lock poisoned"))?;
    if state.guard.phase.load(Ordering::Acquire) != 0 || !state.guard.current(true) {
        return Err(PyValueError::new_err("bank preparation stale or cancelled"));
    }
    state
        .stage
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| PyValueError::new_err("bank preparation already started"))?;
    state.worker_running.store(true, Ordering::Release);
    Ok(state.clone())
}

pub(in crate::audio_engine) fn prepare_detached(
    state: Arc<BankMutationState>,
) -> Result<BankMutationTicket, String> {
    let _completion = BankPreparationCompletion(state.clone());
    let result = (|| {
        let mut copies = Vec::with_capacity(NUM_PADS);
        let mut images = Vec::with_capacity(NUM_PADS);
        // The existing Copy worker lane owns each bounded preparation; none is published separately.
        for (index, copy) in state.copies.iter().enumerate() {
            if !state.guard.current(true) || state.guard.phase.load(Ordering::Acquire) != 0 {
                return Err("bank preparation stale".into());
            }
            if let Some(copy) = copy {
                let operation = copy.clone();
                let mut owners = copy.capture.lane.execute_owned_until_terminal(
                    || state.guard.phase.load(Ordering::Acquire) != 0 || !state.guard.current(true),
                    move || prepare_owned(&operation),
                )?;
                images.push(BankRtImage::Copy(
                    owners.rt.take().ok_or("bank copy work missing")?,
                ));
                copies.push(Some(owners));
            } else {
                let (rate, channels) = state.format;
                images.push(BankRtImage::Empty(Box::new(BankEmptyRtImage {
                    target: state.guard.targets[index].slot(),
                    authority: state.targets[index].authority,
                    epoch: state.targets[index].next_epoch,
                    epoch_owner: state.targets[index].epoch.clone(),
                    empty: PreparedPairEmptyImage::new(
                        state.guard.targets[index].slot(),
                        rate,
                        channels,
                    )
                    .map_err(|error| error.to_string())?,
                    retired_sample: None,
                    retired_stems: None,
                    retired_outgoing: None,
                    retired_receipt: None,
                })));
                copies.push(None);
            }
        }
        let lifetimes = std::array::from_fn(|index| {
            state.copies[index]
                .as_ref()
                .map_or(0, |copy| copy.capture.lifetime)
        });
        let writes = state
            .guard
            .ownership
            .prepare_slot_set_write(state.guard.targets, lifetimes)
            .map_err(|error| error.to_string())?;
        let rt = Box::new(BankMutationRtWork {
            guard: state.guard.clone(),
            writes,
            images: images
                .into_boxed_slice()
                .try_into()
                .map_err(|_| "bank image count")?,
            publication: None,
        });
        Ok(BankOwners {
            copies,
            rt: Some(rt),
            retired_resident: Vec::with_capacity(NUM_PADS),
        })
    })();
    match result {
        Ok(owners) => {
            let mut prepared = state
                .prepared
                .lock()
                .map_err(|_| "bank preparation lock poisoned")?;
            if state.guard.phase.load(Ordering::Acquire) != 0 || !state.guard.current(true) {
                state.guard.reject();
                state.stage.store(4, Ordering::Release);
                drop(prepared);
                drop(owners);
                return Err("bank preparation stale or cancelled before publication".into());
            }
            *prepared = Some(owners);
            state.stage.store(2, Ordering::Release);
            drop(prepared);
            Ok(BankMutationTicket { state })
        }
        Err(error) => {
            state.guard.reject();
            state.stage.store(4, Ordering::Release);
            Err(error)
        }
    }
}

pub(in crate::audio_engine) fn publish(
    engine: &AudioEngine,
    ticket: &BankMutationTicket,
) -> PyResult<()> {
    let state = &ticket.state;
    state.belongs_to(engine)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    if state.stage.load(Ordering::Acquire) != 2
        || state.guard.phase.load(Ordering::Acquire) != 0
        || !state.guard.current(true)
        || state
            .guard
            .targets
            .iter()
            .enumerate()
            .any(|(index, target)| requests[target.slot()] != state.targets[index].request)
    {
        return Err(PyValueError::new_err("bank publication stale"));
    }
    let mut producer = engine
        .control_producer()?
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err("bank publication queue full"));
    }
    let mut prepared = state
        .prepared
        .lock()
        .map_err(|_| PyRuntimeError::new_err("bank preparation lock poisoned"))?;
    let work = prepared
        .as_mut()
        .and_then(|owners| owners.rt.take())
        .ok_or_else(|| PyValueError::new_err("bank work already published"))?;
    for image in work.images.iter() {
        if let BankRtImage::Copy(copy) = image {
            copy.publication
                .mark_pending()
                .map_err(PyRuntimeError::new_err)?;
        }
    }
    state.stage.store(3, Ordering::Release);
    producer
        .push(ControlMessage::BankMutation(Arc::new(
            BankMutationPublication {
                claimed: AtomicBool::new(false),
                work: UnsafeCell::new(Some(work)),
            },
        )))
        .map_err(|_| PyRuntimeError::new_err("reserved bank capacity lost"))
}

pub(in crate::audio_engine) fn validate(
    engine: &AudioEngine,
    ticket: &BankMutationTicket,
    ack: &BankMutationAcknowledgement,
) -> PyResult<bool> {
    ticket.state.belongs_to(engine)?;
    if !Arc::ptr_eq(&ticket.state, &ack.state) {
        return Err(PyValueError::new_err("foreign bank acknowledgement"));
    }
    let state = &ticket.state;
    if state.guard.phase.load(Ordering::Acquire) != 2 {
        return Ok(false);
    }
    let ownership = &state.guard.ownership;
    let prepared = state
        .prepared
        .lock()
        .map_err(|_| PyRuntimeError::new_err("bank preparation lock poisoned"))?;
    let owners = prepared
        .as_ref()
        .ok_or_else(|| PyRuntimeError::new_err("bank custody missing"))?;
    let Some(token) = ownership.placement_read_token() else {
        return Ok(false);
    };
    let current = state
        .guard
        .targets
        .iter()
        .enumerate()
        .all(|(index, before)| {
            let lifetime = state.copies[index]
                .as_ref()
                .map_or(0, |copy| copy.capture.lifetime);
            ownership.slot_snapshot(before.slot()).is_some_and(|after| {
                after.cell() == before.cell()
                    && after.lifetime() == lifetime
                    && after.binding_epoch()
                        == before.binding_epoch() + u64::from(before.lifetime() != lifetime)
                    && state.targets[index].epoch.load(Ordering::Acquire)
                        == state.targets[index].next_epoch
                    && ownership.authority_revision(before.slot()) == state.targets[index].authority
                    && match &owners.copies[index] {
                        Some(copy) => {
                            ownership.source_generation(
                                before.slot(),
                                &copy.sample,
                                state.copies[index].as_ref().unwrap().capture.rate,
                            ) == Some(state.targets[index].next_request)
                        }
                        None => ownership.source_cell_empty(after.cell()),
                    }
            })
        });
    Ok(current && ownership.placement_read_token() == Some(token))
}

pub(in crate::audio_engine) fn reconcile(
    engine: &AudioEngine,
    ticket: &BankMutationTicket,
    ack: &BankMutationAcknowledgement,
) -> PyResult<bool> {
    if !validate(engine, ticket, ack)? {
        return Ok(false);
    }
    let state = &ticket.state;
    if state.mirrored.load(Ordering::Acquire) {
        return Ok(true);
    }
    // Every fallible lock and every successor selection is obtained before the first mirror store.
    let mut requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut prepared = state
        .prepared
        .lock()
        .map_err(|_| PyRuntimeError::new_err("bank preparation lock poisoned"))?;
    let owners = prepared
        .as_mut()
        .ok_or_else(|| PyRuntimeError::new_err("bank custody missing"))?;
    let mut cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cache lock poisoned"))?;
    let mut generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("generation lock poisoned"))?;
    let mut digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("digest lock poisoned"))?;
    let mut leases = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("lease lock poisoned"))?;
    let mut intents = engine
        .timing_intents
        .lock()
        .map_err(|_| PyRuntimeError::new_err("intent lock poisoned"))?;
    let mut records = engine
        .current_constant_timing
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing lock poisoned"))?;
    let mut resident = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident lock poisoned"))?;
    let mut replacements = Vec::with_capacity(NUM_PADS);
    let mut selections = Vec::with_capacity(NUM_PADS);
    for index in 0..NUM_PADS {
        let target = state.guard.targets[index].slot();
        let capture = &state.targets[index];
        let previous_sample = match (&capture.previous_sample, &cache[target]) {
            (Some(a), Some(b)) => a.same_source(b) && a.same_window(b),
            (None, None) => true,
            _ => false,
        };
        let previous_lease = match (&capture.previous_lease, &leases[target]) {
            (Some(a), Some(b)) => a.assignment_id() == b.assignment_id() && !b.assignment_retired(),
            (None, None) => true,
            _ => false,
        };
        let expected_lifetime = state.copies[index]
            .as_ref()
            .map_or(0, |copy| copy.capture.lifetime);
        if requests[target] != capture.request
            || !previous_sample
            || !previous_lease
            || state.guard.ownership.copy_reservation(target) != state.guard.owner
            || state.guard.ownership.content_lifetime(target) != expected_lifetime
            || capture.epoch.load(Ordering::Acquire) != capture.next_epoch
            || state.guard.ownership.authority_revision(target) != capture.authority
        {
            return Err(PyRuntimeError::new_err(
                "bank own ACK no longer matches all targets; settlement remains fenced",
            ));
        }
        let mut replacement = committed_stem_residency::prepare_copy_replacement(&resident[target]);
        let selection = if let Some(copy) = &owners.copies[index] {
            if state.guard.ownership.source_generation(
                target,
                &copy.sample,
                state.copies[index].as_ref().unwrap().capture.rate,
            ) != Some(capture.next_request)
                || engine
                    .current_timing_acknowledgements
                    .current_for_slot(&engine.input_runtime_ownership, target)
                    != copy
                        .timing
                        .as_ref()
                        .map_or(0, |record| record.projection.publication_epoch)
            {
                return Err(PyRuntimeError::new_err("bank copied source ACK changed"));
            }
            copy.selection
                .as_ref()
                .map(|selection| {
                    committed_stem_residency::reconcile_copy_selection(
                        &mut replacement,
                        selection.clone(),
                        &state.guard.phase,
                    )
                })
                .transpose()
                .map_err(PyRuntimeError::new_err)?
        } else {
            None
        };
        replacements.push(replacement);
        selections.push(selection);
    }
    for (index, (replacement, selection)) in replacements.into_iter().zip(selections).enumerate() {
        let target = state.guard.targets[index].slot();
        let capture = &state.targets[index];
        owners
            .retired_resident
            .push(std::mem::replace(&mut resident[target], replacement));
        requests[target] = capture.next_request;
        if let Some(copy) = &mut owners.copies[index] {
            copy.selection_ticket = selection;
            cache[target] = Some(copy.sample.clone());
            generations[target] = (
                capture.next_request,
                state.copies[index].as_ref().unwrap().capture.rate,
            );
            digests[target] = Some(state.copies[index].as_ref().unwrap().capture.digest.clone());
            leases[target] = Some(copy.lease.clone());
            engine.cold_lease_generations[target].store(capture.next_request, Ordering::Release);
            intents[target] = state.copies[index]
                .as_ref()
                .unwrap()
                .capture
                .intent
                .timing_intent;
            records[target] = copy.timing.iter().cloned().collect();
            copy.subscription.transfer_to_target();
        } else {
            cache[target] = None;
            generations[target] = (0, 0);
            digests[target] = None;
            leases[target] = None;
            engine.cold_lease_generations[target].store(0, Ordering::Release);
            intents[target] = flitzis_looper_analysis::tempo_acceptance::TimingIntent::Legacy;
            records[target].clear();
        }
    }
    state.mirrored.store(true, Ordering::Release);
    Ok(true)
}

fn settle(engine: &AudioEngine, state: &Arc<BankMutationState>) -> PyResult<()> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut states = engine
        .bank_mutations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("bank ownership lock poisoned"))?;
    if let Some(index) = states
        .iter()
        .position(|current| Arc::ptr_eq(current, state))
    {
        state
            .guard
            .ownership
            .release_copy_set(&state.guard.reservations, state.guard.owner)
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        states.remove(index);
    }
    Ok(())
}

pub(in crate::audio_engine) fn finish(
    engine: &AudioEngine,
    ticket: &BankMutationTicket,
    ack: &BankMutationAcknowledgement,
) -> PyResult<()> {
    if !validate(engine, ticket, ack)? || !ticket.state.mirrored.load(Ordering::Acquire) {
        return Err(PyValueError::new_err("bank own ACK/mirror pending"));
    }
    settle(engine, &ticket.state)
}

pub(in crate::audio_engine) fn cancel(
    engine: &AudioEngine,
    captured: &CapturedBankMutation,
) -> PyResult<bool> {
    captured.state.belongs_to(engine)?;
    let waiting_for_worker = {
        let _prepared = captured
            .state
            .prepared
            .lock()
            .map_err(|_| PyRuntimeError::new_err("bank preparation lock poisoned"))?;
        captured.state.guard.reject();
        if captured.state.guard.phase.load(Ordering::Acquire) != 3 {
            return Ok(false);
        }
        captured.state.worker_running.load(Ordering::Acquire)
            || captured.state.stage.load(Ordering::Acquire) == 1
    };
    if waiting_for_worker {
        return Ok(false);
    }
    // Never wait for the request mutex while holding the preparation mutex.
    settle(engine, &captured.state)?;
    Ok(true)
}
