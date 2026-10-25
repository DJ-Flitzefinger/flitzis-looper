//! One native selected-content projection for GUI and the serialized MIDI dispatcher.

use super::{InputRuntime, RuntimePadState, RuntimeState};
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::content_placement::{ContentKey, PlacementObservation};
use crate::audio_engine::input_runtime_binding::{
    InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding,
};
use crate::audio_engine::pitch_actions::{
    CapturedPitchAction, FrozenPitchRender, PitchActionAdmission, PitchActionReject,
    PitchActionShadow, PitchActionSpec, PitchKeyMode,
};
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::timing::SharedOutputClock;
use crate::audio_engine::transport::QuantizeGrid;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PitchPadProjection {
    pub(crate) source_root: Option<u8>,
    pub(crate) source_mode: Option<PitchKeyMode>,
    pub(crate) correction_root: Option<u8>,
    pub(crate) correction_mode: Option<PitchKeyMode>,
    pub(crate) correction_unknown: bool,
    // Control-owned pending Original Key marker; never restored from Python.
    pub(crate) correction_reset_sequence: u64,
    pub(crate) intent: PitchIntent,
    pub(crate) key_lock: bool,
    pub(crate) quantize_step_64ths: Option<u32>,
    pub(crate) sync: bool,
    pub(crate) request_sequence: u64,
}

impl Default for PitchPadProjection {
    fn default() -> Self {
        Self {
            source_root: None,
            source_mode: None,
            correction_root: None,
            correction_mode: None,
            correction_unknown: false,
            correction_reset_sequence: 0,
            intent: PitchIntent::default(),
            key_lock: true,
            quantize_step_64ths: None,
            sync: false,
            request_sequence: 0,
        }
    }
}

impl PitchPadProjection {
    pub(super) fn validate(self) -> Result<(), PitchActionReject> {
        for (root, mode) in [
            (self.source_root, self.source_mode),
            (self.correction_root, self.correction_mode),
        ] {
            if root.is_some() != mode.is_some() || root.is_some_and(|root| root > 11) {
                return Err(PitchActionReject::InvalidAction);
            }
        }
        if self.correction_unknown && self.correction_root.is_some() {
            return Err(PitchActionReject::InvalidAction);
        }
        if self
            .quantize_step_64ths
            .is_some_and(|step| grid_for_step(step).is_none())
        {
            return Err(PitchActionReject::InvalidAction);
        }
        Ok(())
    }

    fn source_key(self) -> Option<(u8, PitchKeyMode)> {
        if self.correction_unknown {
            return None;
        }
        self.correction_root
            .zip(self.correction_mode)
            .or_else(|| self.source_root.zip(self.source_mode))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RequestedKeyCorrection {
    root: Option<u8>,
    mode: Option<PitchKeyMode>,
    unknown: bool,
    reset_sequence: u64,
}

impl RequestedKeyCorrection {
    fn from_projection(projection: PitchPadProjection) -> Self {
        Self {
            root: projection.correction_root,
            mode: projection.correction_mode,
            unknown: projection.correction_unknown,
            reset_sequence: projection.correction_reset_sequence,
        }
    }

    fn apply(self, projection: &mut PitchPadProjection) {
        projection.correction_root = self.root;
        projection.correction_mode = self.mode;
        projection.correction_unknown = self.unknown;
        projection.correction_reset_sequence = self.reset_sequence;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct RequestedPitchAnchor {
    intent: PitchIntent,
    correction: RequestedKeyCorrection,
    sequence: u64,
}

impl RequestedPitchAnchor {
    fn from_row(row: &RuntimePadState) -> Self {
        Self {
            intent: row.pitch.intent,
            correction: RequestedKeyCorrection::from_projection(row.pitch),
            sequence: row.latest_request_sequence,
        }
    }

    fn restore(self, row: &mut RuntimePadState) {
        row.pitch.intent = self.intent;
        // Ordinary numeric requests do not own metadata. Only restoring across
        // a pending Original Key boundary may replace the current correction.
        if row.pitch.correction_reset_sequence != self.correction.reset_sequence {
            self.correction.apply(&mut row.pitch);
        }
        row.pitch.request_sequence = self.sequence;
        row.latest_request_sequence = self.sequence;
    }
}

// This is a separate CONTROL bound, chosen to match the existing 64 admitted
// action slots. It is not a bound on control-side Native candidates or total RAM.
const REQUESTED_PITCH_CAPACITY: usize = 64;

#[derive(Clone)]
struct RequestedPitchRecord {
    key: ContentKey,
    sequence: u64,
    intent: PitchIntent,
    correction: RequestedKeyCorrection,
    previous: RequestedPitchAnchor,
    observation: PitchActionShadow,
}

impl RequestedPitchRecord {
    fn current(&self, row: &RuntimePadState, id: usize, ownership: &InputRuntimeOwnership) -> bool {
        row.loaded
            && row
                .binding
                .zip(row.acknowledgements.as_deref())
                .is_some_and(|(binding, acknowledgements)| {
                    self.observation
                        .current(id, binding, ownership, acknowledgements)
                })
    }
}

#[derive(Clone)]
pub(super) struct RequestedPitchJournal {
    records: Box<[Option<RequestedPitchRecord>]>,
}

impl Default for RequestedPitchJournal {
    fn default() -> Self {
        Self {
            records: (0..REQUESTED_PITCH_CAPACITY).map(|_| None).collect(),
        }
    }
}

impl RequestedPitchJournal {
    fn has_capacity(&self) -> bool {
        self.records.iter().any(Option::is_none)
    }

    fn insert(
        &mut self,
        action: &CapturedPitchAction,
        previous_correction: RequestedKeyCorrection,
        correction: RequestedKeyCorrection,
    ) {
        let (intent, sequence) = action.previous_requested();
        *self
            .records
            .iter_mut()
            .find(|record| record.is_none())
            .expect("reserved requested-pitch capacity") = Some(RequestedPitchRecord {
            key: action.content_key(),
            sequence: action.sequence(),
            intent: action.render().pitch,
            correction,
            previous: RequestedPitchAnchor {
                intent,
                correction: previous_correction,
                sequence,
            },
            observation: action.requested_shadow(),
        });
    }

    /// An accepted Python requested projection is an anchor even if its eventual
    /// audio action fails. Old/equal captures cannot later roll that model back.
    pub(super) fn anchor(&mut self, key: ContentKey, anchor: RequestedPitchAnchor) {
        for entry in &mut self.records {
            let Some(record) = entry else { continue };
            if record.key != key {
                continue;
            }
            if record.sequence <= anchor.sequence {
                *entry = None;
            } else if record.previous.sequence <= anchor.sequence {
                record.previous = anchor;
            }
        }
    }

    pub(super) fn anchor_row(&mut self, row: &RuntimePadState) {
        if let Some((key, anchor)) = row
            .binding
            .and_then(|binding| binding.placement?.content_key())
            .zip(row.pitch_anchor)
        {
            if row.pitch.correction_reset_sequence == 0 {
                let correction = RequestedKeyCorrection::from_projection(row.pitch);
                // Metadata can change independently of numeric request order.
                // Update only acknowledged correction history, retaining any
                // newer pending Original clear as its own component owner.
                for record in self.records.iter_mut().flatten() {
                    if record.key != key {
                        continue;
                    }
                    if record.correction.reset_sequence <= anchor.sequence {
                        record.correction = correction;
                    }
                    if record.previous.correction.reset_sequence <= anchor.sequence {
                        record.previous.correction = correction;
                    }
                }
            }
            self.anchor(key, anchor);
        }
    }

    /// Initialize a fresh source/timing model from the last accepted scalar
    /// intent, rather than inheriting an unresolved capture from its predecessor
    /// source. A same-source physical window/runtime update retains the chain.
    pub(super) fn refresh_row(
        &self,
        row: &mut RuntimePadState,
        id: usize,
        ownership: &InputRuntimeOwnership,
    ) {
        let Some(binding) = row.binding.filter(|_| row.loaded) else {
            return;
        };
        if self.records.iter().flatten().any(|record| {
            record.sequence == row.latest_request_sequence
                && binding.placement.and_then(|p| p.content_key()) == Some(record.key)
                && !record.current(row, id, ownership)
        }) && let Some(anchor) = row.pitch_anchor
        {
            anchor.restore(row);
        }
    }

    /// At most 64 records and 64 scalar predecessor rewrites per record, only
    /// under the existing control mutex. No callback calls this journal.
    pub(super) fn reconcile(
        &mut self,
        pads: &mut [RuntimePadState],
        ownership: &InputRuntimeOwnership,
    ) -> bool {
        let mut changed = false;
        for index in 0..REQUESTED_PITCH_CAPACITY {
            let Some(record) = &self.records[index] else {
                continue;
            };
            // A reserved content row must retain its exact requested chain until
            // the arrangement has reconciled its ACK and control mirror. Match
            // the held row before resolving the key: Clear may already have
            // retired it while that original row still belongs to the fence.
            if pads.iter().enumerate().any(|(id, row)| {
                row.binding
                    .and_then(|binding| binding.placement?.content_key())
                    == Some(record.key)
                    && (ownership.copy_reservation(id) != 0 || ownership.pair_reservation(id) != 0)
            }) {
                continue;
            }
            let placement = match ownership.observe_control_key(record.key) {
                PlacementObservation::Current(placement) => Some(placement),
                PlacementObservation::Pending => continue,
                PlacementObservation::Stale | PlacementObservation::Retired => None,
            };
            let current = placement.is_some_and(|placement| {
                let row = &pads[placement.slot()];
                record.current(row, placement.slot(), ownership)
            });
            if current && !record.observation.rejected() && !record.observation.applied() {
                continue;
            }
            let record = self.records[index].take().expect("observed record");
            if !current {
                // Initialize a refreshed row from its accepted requested-model
                // anchor, never from the stale original's predecessor. An
                // authoritative replacement already removed/anchored this record.
                if let Some(placement) = placement {
                    let row = &mut pads[placement.slot()];
                    if row.latest_request_sequence == record.sequence
                        && let Some(anchor) = row.pitch_anchor
                    {
                        let before = (row.pitch, row.latest_request_sequence, row.pitch_anchor);
                        anchor.restore(row);
                        changed |=
                            before != (row.pitch, row.latest_request_sequence, row.pitch_anchor);
                    }
                }
                continue;
            }
            let row = &mut pads[placement.expect("current placement").slot()];
            let before = (row.pitch, row.latest_request_sequence, row.pitch_anchor);
            if record.observation.rejected() {
                let owns_correction_reset = record.correction.reset_sequence == record.sequence;
                if owns_correction_reset {
                    if row.pitch.correction_reset_sequence == record.sequence {
                        record.previous.correction.apply(&mut row.pitch);
                    }
                    if let Some(anchor) = &mut row.pitch_anchor
                        && anchor.correction.reset_sequence == record.sequence
                    {
                        anchor.correction = record.previous.correction;
                    }
                }
                if row.latest_request_sequence == record.sequence {
                    record.previous.restore(row);
                }
                // Remove the tombstone immediately, including when a later
                // capture is current. Arbitrary cancellation order is bounded.
                for successor in self.records.iter_mut().flatten() {
                    if owns_correction_reset && successor.key == record.key {
                        if successor.correction.reset_sequence == record.sequence {
                            successor.correction = record.previous.correction;
                        }
                        if successor.previous.correction.reset_sequence == record.sequence {
                            successor.previous.correction = record.previous.correction;
                        }
                    }
                    if successor.key == record.key && successor.previous.sequence == record.sequence
                    {
                        successor.previous = record.previous;
                    }
                }
            } else if record.observation.applied()
                && row
                    .pitch_anchor
                    .is_none_or(|anchor| anchor.sequence < record.sequence)
            {
                // A successor already contains this applied tuple as its scalar
                // predecessor; it needs no reference to the terminal action.
                row.pitch_anchor = Some(RequestedPitchAnchor {
                    intent: record.intent,
                    correction: record.correction,
                    sequence: record.sequence,
                });
            }
            changed |= before != (row.pitch, row.latest_request_sequence, row.pitch_anchor);
        }
        changed
    }
}

impl RuntimeState {
    fn reconcile_policy_pitch(&mut self, ownership: &InputRuntimeOwnership) {
        if self.pitch_shadow.reconcile(&mut self.pads, ownership) {
            ownership.publish_runtime_policy_revision();
        }
    }
}

pub(super) fn apply_pitch_projection(
    row: &mut RuntimePadState,
    projection: PitchPadProjection,
) -> bool {
    let requested = row.pitch.intent;
    let correction = RequestedKeyCorrection::from_projection(row.pitch);
    row.pitch = projection;
    // A stale numeric projection may carry a legitimate independent metadata
    // edit. Only a pending Original clear fences that metadata component.
    let correction_acknowledged = projection.request_sequence >= correction.reset_sequence;
    if !correction_acknowledged {
        correction.apply(&mut row.pitch);
    }
    if projection.request_sequence < row.latest_request_sequence {
        row.pitch.intent = requested;
        row.pitch.request_sequence = row.latest_request_sequence;
        if row
            .pitch_anchor
            .is_none_or(|anchor| anchor.sequence < projection.request_sequence)
        {
            // A projected predecessor is authoritative requested intent even
            // while a newer original owns the current shadow. Anchor that
            // predecessor's own tuple; preserving the newer row is independent.
            row.pitch_anchor = Some(RequestedPitchAnchor {
                intent: projection.intent,
                correction: RequestedKeyCorrection::from_projection(projection),
                sequence: projection.request_sequence,
            });
            true
        } else {
            if correction_acknowledged && let Some(anchor) = &mut row.pitch_anchor {
                anchor.correction = RequestedKeyCorrection::from_projection(row.pitch);
            }
            correction_acknowledged
        }
    } else {
        row.latest_request_sequence = projection.request_sequence;
        row.pitch_anchor = Some(RequestedPitchAnchor::from_row(row));
        true
    }
}

fn grid_for_step(step: u32) -> Option<QuantizeGrid> {
    u16::try_from(step)
        .ok()
        .and_then(QuantizeGrid::from_step_64ths)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RuntimeSelectedContent {
    key: ContentKey,
    binding: InputPadBinding,
    source_generation: u64,
    source_request_incarnation: u64,
}

impl RuntimeSelectedContent {
    fn capture(
        id: usize,
        binding: InputPadBinding,
        ownership: &InputRuntimeOwnership,
    ) -> Result<Self, PitchActionReject> {
        let key = binding
            .placement
            .and_then(|placement| placement.content_key())
            .ok_or(PitchActionReject::StaleContent)?;
        let selected = Self {
            key,
            binding,
            source_generation: ownership
                .binding_source_generation(id, binding)
                .ok_or(PitchActionReject::StaleContent)?,
            source_request_incarnation: ownership
                .key_source_request_incarnation(key)
                .ok_or(PitchActionReject::StaleContent)?,
        };
        selected
            .matches_current_source(id, binding, ownership)
            .then_some(selected)
            .ok_or(PitchActionReject::StaleContent)
    }

    fn matches_current_source(
        self,
        id: usize,
        binding: InputPadBinding,
        ownership: &InputRuntimeOwnership,
    ) -> bool {
        let request_current = || {
            ownership.binding_source_generation(id, binding) == Some(self.source_generation)
                && ownership.key_source_request_incarnation(self.key)
                    == Some(self.source_request_incarnation)
        };
        binding
            .placement
            .and_then(|placement| placement.content_key())
            == Some(self.key)
            && binding.source_address == self.binding.source_address
            && binding.sample_count == self.binding.sample_count
            && binding.channels == self.binding.channels
            && binding.sample_rate_hz == self.binding.sample_rate_hz
            && request_current()
            && ownership.admission_current(id, binding)
            && request_current()
    }
}

fn selection_for_pad(
    id: usize,
    state: &RuntimeState,
    ownership: &Arc<InputRuntimeOwnership>,
) -> Result<Option<RuntimeSelectedContent>, PitchActionReject> {
    let pad = state.pads.get(id).ok_or(PitchActionReject::InvalidAction)?;
    if !pad.loaded {
        // Empty selection is a valid UI operation. It removes audio selection authority.
        return Ok(None);
    }
    let binding = pad.binding.ok_or(PitchActionReject::StaleContent)?;
    RuntimeSelectedContent::capture(id, binding, ownership).map(Some)
}

pub(super) fn select_pad(
    id: usize,
    runtime: &Arc<Mutex<RuntimeState>>,
    ownership: &Arc<InputRuntimeOwnership>,
) -> Result<u64, PitchActionReject> {
    let mut state = runtime
        .lock()
        .map_err(|_| PitchActionReject::PlacementPending)?;
    if id >= state.pads.len() {
        return Err(PitchActionReject::InvalidAction);
    }
    let revision = state
        .selection_revision
        .checked_add(1)
        .ok_or(PitchActionReject::SequenceExhausted)?;
    state.selection_revision = revision;
    let (selected, result) = match selection_for_pad(id, &state, ownership) {
        Ok(selected) => (selected, Ok(revision)),
        Err(reason) => (None, Err(reason)),
    };
    state.selected = selected;
    ownership.publish_endpoint_selection(revision, selected.map(|selected| selected.key));
    result
}

/// The dispatcher delivers only its original receipt here. Its nonblocking channel
/// admission shares the runtime mutex with selected_projection and pitch capture.
pub(super) fn select_pad_with_delivery(
    id: usize,
    runtime: &Arc<Mutex<RuntimeState>>,
    ownership: &Arc<InputRuntimeOwnership>,
    deliver: impl FnOnce(Result<u64, PitchActionReject>) -> bool,
) {
    let mut state = match runtime.lock() {
        Ok(state) => state,
        Err(_) => {
            deliver(Err(PitchActionReject::PlacementPending));
            return;
        }
    };
    let selected = match selection_for_pad(id, &state, ownership) {
        Ok(selected) => selected,
        Err(reason) => {
            deliver(Err(reason));
            return;
        }
    };
    let Some(revision) = state.selection_revision.checked_add(1) else {
        deliver(Err(PitchActionReject::SequenceExhausted));
        return;
    };
    if deliver(Ok(revision)) {
        // Everything after accepted delivery is an infallible scalar publication.
        // A receipt consumer cannot validate it until this mutex is released.
        state.selected = selected;
        state.selection_revision = revision;
        ownership.publish_endpoint_selection(revision, selected.map(|selected| selected.key));
    }
}

fn selected_row(
    state: &RuntimeState,
    ownership: &Arc<InputRuntimeOwnership>,
) -> Result<usize, PitchActionReject> {
    let selected = state.selected.ok_or(PitchActionReject::NoSelectedContent)?;
    let placement = match ownership.observe_control_key(selected.key) {
        PlacementObservation::Current(placement) => placement,
        PlacementObservation::Pending => return Err(PitchActionReject::PlacementPending),
        PlacementObservation::Stale | PlacementObservation::Retired => {
            return Err(PitchActionReject::StaleContent);
        }
    };
    let pad = &state.pads[placement.slot()];
    let binding = pad.binding.ok_or(PitchActionReject::StaleContent)?;
    // A new gesture uses this row's current control authority for the same original
    // source request. Previously captured actions keep their own immutable authority.
    // Source replacement, including reused PCM in a new request, still requires selection.
    if !pad.loaded || !selected.matches_current_source(placement.slot(), binding, ownership) {
        return Err(PitchActionReject::StaleContent);
    }
    Ok(placement.slot())
}

type InputOwners<'a> = Option<(
    &'a Arc<AtomicBool>,
    &'a Arc<AtomicBool>,
    &'a Arc<AtomicBool>,
)>;

fn capture_row(
    spec: PitchActionSpec,
    received_at_ns: u64,
    now_ns: u64,
    state: &mut RuntimeState,
    id: usize,
    ownership: &Arc<InputRuntimeOwnership>,
    input_owners: InputOwners<'_>,
) -> Result<CapturedPitchAction, PitchActionReject> {
    if input_owners.is_some_and(|(mapping, enabled, learn)| {
        !mapping.load(Ordering::Acquire)
            || !enabled.load(Ordering::Acquire)
            || learn.load(Ordering::Acquire)
    }) {
        return Err(PitchActionReject::InputDisabled);
    }
    state.reconcile_policy_pitch(ownership);
    if !state.pitch_shadow.has_capacity() {
        return Err(PitchActionReject::Capacity);
    }
    let pad = state.pads.get(id).ok_or(PitchActionReject::InvalidAction)?;
    let binding = pad.binding.ok_or(PitchActionReject::StaleContent)?;
    if !pad.loaded || !ownership.admission_current(id, binding) {
        return Err(PitchActionReject::StaleContent);
    }
    let key = binding
        .placement
        .and_then(|placement| placement.content_key())
        .ok_or(PitchActionReject::StaleContent)?;
    let projection = pad.pitch;
    projection.validate()?;
    let pitch = spec.resolve(projection.source_key(), projection.intent)?;
    let acknowledgements = pad
        .acknowledgements
        .as_ref()
        .ok_or(PitchActionReject::TimingUnavailable)?
        .clone();
    let clock = state
        .pitch_clock
        .as_ref()
        .and_then(|clock| clock.read())
        .ok_or(PitchActionReject::TimingUnavailable)?;
    if !clock.is_fresh(now_ns) || clock.sample_rate_hz == 0 || received_at_ns > now_ns {
        return Err(PitchActionReject::TimingUnavailable);
    }
    // Use the one existing nearest-input-grid function. A retry has no clock access.
    let due_frame = if let Some(step) = projection.quantize_step_64ths {
        let grid = grid_for_step(step).ok_or(PitchActionReject::InvalidAction)?;
        if clock.master_period_seconds.is_some() {
            clock
                .input_target_frame(Some(received_at_ns), now_ns, grid)
                .ok_or(PitchActionReject::TimingUnavailable)?
        } else {
            // Existing no-master launch policy is immediate, rather than an invented grid.
            clock.output_frame
        }
    } else {
        clock.output_frame
    };
    let action = CapturedPitchAction::capture(PitchActionAdmission {
        key_lock_request: ownership
            .key_lock_request_for_cell(key.cell())
            .load(Ordering::Acquire),
        sync_request: pad.source_map.request(),
        source_map: pad.source_map.snapshot(ownership, binding),
        key,
        binding,
        ownership: ownership.clone(),
        spec,
        acknowledgements,
        render: FrozenPitchRender {
            pitch,
            key_lock: projection.key_lock,
        },
        received_at_ns,
        due_frame,
        quantize_step_64ths: projection.quantize_step_64ths,
        sync: projection.sync,
        exclusive: !state.multi_loop,
        loop_start_s: pad.loop_start_s,
        loop_end_s: pad.loop_end_s,
        input_owners: input_owners
            .map(|(mapping, enabled, learn)| (mapping.clone(), enabled.clone(), learn.clone())),
        previous_requested: projection.intent,
        previous_sequence: pad.latest_request_sequence,
    })?;
    if state.pads[id].pitch_anchor.is_none() {
        state.pads[id].pitch_anchor = Some(RequestedPitchAnchor::from_row(&state.pads[id]));
    }
    let previous_correction = RequestedKeyCorrection::from_projection(projection);
    let correction = if action.resets_key_correction() {
        RequestedKeyCorrection {
            reset_sequence: action.sequence(),
            ..RequestedKeyCorrection::default()
        }
    } else {
        previous_correction
    };
    state
        .pitch_shadow
        .insert(&action, previous_correction, correction);
    // This is requested control intent only. Native/effective tuning stays untouched
    // while preparation is pending, and every event keeps its independently frozen tuple.
    state.pads[id].pitch.intent = pitch;
    correction.apply(&mut state.pads[id].pitch);
    state.pads[id].pitch.request_sequence = action.sequence();
    state.pads[id].latest_request_sequence = action.sequence();
    ownership.publish_runtime_policy_revision();
    Ok(action)
}

pub(super) fn reject_undelivered_capture(
    action: &CapturedPitchAction,
    runtime: &Arc<Mutex<RuntimeState>>,
    ownership: &Arc<InputRuntimeOwnership>,
) {
    action.reject(PitchActionReject::Capacity);
    let Ok(mut state) = runtime.lock() else {
        return;
    };
    state.reconcile_policy_pitch(ownership);
}

pub(super) fn capture_selected(
    spec: PitchActionSpec,
    received_at_ns: u64,
    now_ns: u64,
    runtime: &Arc<Mutex<RuntimeState>>,
    ownership: &Arc<InputRuntimeOwnership>,
    input_owners: InputOwners<'_>,
) -> Result<CapturedPitchAction, PitchActionReject> {
    let mut state = runtime
        .lock()
        .map_err(|_| PitchActionReject::PlacementPending)?;
    let id = selected_row(&state, ownership)?;
    let action = capture_row(
        spec,
        received_at_ns,
        now_ns,
        &mut state,
        id,
        ownership,
        input_owners,
    )?;
    // A logical source request can advance without taking the runtime mutex.
    // Never return a fresh action that crossed into such a replacement request.
    if let Err(reason) = selected_row(&state, ownership) {
        action.reject(reason);
        state.reconcile_policy_pitch(ownership);
        return Err(reason);
    }
    Ok(action)
}

impl InputRuntime {
    /// Control-publish failures cancel the original owner and restore only its own
    /// requested shadow. The callback never locks this runtime or calls this helper.
    pub(crate) fn cancel_pitch_capture(
        &self,
        action: &CapturedPitchAction,
        reason: PitchActionReject,
    ) -> Result<bool, PitchActionReject> {
        if !action.belongs_to(&self.ownership) {
            return Err(PitchActionReject::StaleContent);
        }
        let mut state = self
            .runtime_state
            .lock()
            .map_err(|_| PitchActionReject::PlacementPending)?;
        state.reconcile_policy_pitch(&self.ownership);
        if action.effective() || action.state() == "rejected" {
            return Ok(false);
        }
        // Control may cross the finite capture/schedule/worker phase transitions.
        // Callback rejection itself stays one CAS; four control attempts cover
        // their three forward transitions and still stop at the applying claim.
        if !(0..4).any(|_| action.reject(reason)) {
            return Ok(false);
        }
        state.reconcile_policy_pitch(&self.ownership);
        Ok(true)
    }

    pub(crate) fn attach_pitch_clock(&self, clock: Arc<SharedOutputClock>) -> Result<(), String> {
        self.runtime_state
            .lock()
            .map_err(|_| "input runtime lock poisoned")?
            .pitch_clock = Some(clock);
        Ok(())
    }

    pub(crate) fn set_render_format(&self, rate: u32, channels: usize) -> Result<(), String> {
        if rate == 0 || channels == 0 {
            return Err("invalid output format".into());
        }
        self.runtime_state
            .lock()
            .map_err(|_| "input runtime lock poisoned")?
            .render_format = Some((rate, channels));
        Ok(())
    }

    pub(crate) fn set_selected_pad(&self, id: usize) -> Result<u64, PitchActionReject> {
        select_pad(id, &self.runtime_state, &self.ownership)
    }

    /// GUI selection uses the same actual source owner before a subsequent input can capture.
    pub(crate) fn set_selected_binding(
        &self,
        binding: &InputRuntimePadBinding,
    ) -> Result<u64, PitchActionReject> {
        if !Arc::ptr_eq(&binding.ownership, &self.ownership) || !binding.current() {
            return Err(PitchActionReject::StaleContent);
        }
        let mut state = self
            .runtime_state
            .lock()
            .map_err(|_| PitchActionReject::PlacementPending)?;
        let row = state
            .pads
            .get(binding.id)
            .ok_or(PitchActionReject::InvalidAction)?;
        let current = row.binding.ok_or(PitchActionReject::StaleContent)?;
        if !row.loaded
            || !same_source(current, binding.binding)
            || !self.ownership.admission_current(binding.id, current)
        {
            return Err(PitchActionReject::StaleContent);
        }
        let revision = state
            .selection_revision
            .checked_add(1)
            .ok_or(PitchActionReject::SequenceExhausted)?;
        state.selected = Some(RuntimeSelectedContent::capture(
            binding.id,
            current,
            &self.ownership,
        )?);
        state.selection_revision = revision;
        self.ownership
            .publish_endpoint_selection(revision, state.selected.map(|selected| selected.key));
        Ok(revision)
    }

    /// Receipts may project only this revision. GUI input increments the same native
    /// counter, so an older MIDI receipt cannot replace a newer GUI/native selection.
    pub(crate) fn selected_projection(&self) -> Result<(u64, Option<usize>), PitchActionReject> {
        let state = self
            .runtime_state
            .lock()
            .map_err(|_| PitchActionReject::PlacementPending)?;
        let slot = selected_row(&state, &self.ownership).ok();
        Ok((state.selection_revision, slot))
    }

    pub(crate) fn pitch_request_sequence(&self, id: usize) -> Result<u64, PitchActionReject> {
        let state = self
            .runtime_state
            .lock()
            .map_err(|_| PitchActionReject::PlacementPending)?;
        state
            .pads
            .get(id)
            .map(|row| row.latest_request_sequence)
            .ok_or(PitchActionReject::InvalidAction)
    }

    /// Pitch intent does not advance the unrelated runtime/source revision. Every older
    /// admitted action owns its frozen tuple. A full source/loop publication still fences.
    #[cfg(test)]
    pub(crate) fn set_pitch_runtime_state(
        &self,
        projections: Vec<PitchPadProjection>,
    ) -> Result<(), PitchActionReject> {
        if projections.len() != NUM_SAMPLES {
            return Err(PitchActionReject::InvalidAction);
        }
        for projection in &projections {
            projection.validate()?;
        }
        let mut state = self
            .runtime_state
            .lock()
            .map_err(|_| PitchActionReject::PlacementPending)?;
        state.reconcile_policy_pitch(&self.ownership);
        let RuntimeState {
            pitch_shadow, pads, ..
        } = &mut *state;
        let mut changed = false;
        for (row, projection) in pads.iter_mut().zip(projections) {
            let before = (row.pitch, row.latest_request_sequence, row.pitch_anchor);
            if apply_pitch_projection(row, projection) {
                pitch_shadow.anchor_row(row);
            }
            changed |= before != (row.pitch, row.latest_request_sequence, row.pitch_anchor);
        }
        let multi_loop = state.multi_loop;
        for row in &mut state.pads {
            row.source_map.remask_parents(multi_loop, row.pitch.sync);
        }
        if changed {
            self.ownership.publish_runtime_policy_revision();
        }
        Ok(())
    }

    pub(crate) fn capture_selected_pitch(
        &self,
        action_key: &str,
        received_at_ns: u64,
    ) -> Result<CapturedPitchAction, PitchActionReject> {
        let spec = PitchActionSpec::parse(action_key).ok_or(PitchActionReject::InvalidAction)?;
        capture_selected(
            spec,
            received_at_ns,
            self.clock.capture_ns(),
            &self.runtime_state,
            &self.ownership,
            None,
        )
    }

    pub(crate) fn capture_gui_pitch(
        &self,
        binding: &InputRuntimePadBinding,
        action_key: &str,
        received_at_ns: u64,
    ) -> Result<CapturedPitchAction, PitchActionReject> {
        if !Arc::ptr_eq(&binding.ownership, &self.ownership) || !binding.current() {
            return Err(PitchActionReject::StaleContent);
        }
        let spec = PitchActionSpec::parse(action_key).ok_or(PitchActionReject::InvalidAction)?;
        let mut state = self
            .runtime_state
            .lock()
            .map_err(|_| PitchActionReject::PlacementPending)?;
        let row = state
            .pads
            .get(binding.id)
            .ok_or(PitchActionReject::StaleContent)?;
        if row
            .binding
            .is_none_or(|current| !same_source(current, binding.binding))
        {
            return Err(PitchActionReject::StaleContent);
        }
        capture_row(
            spec,
            received_at_ns,
            self.clock.capture_ns(),
            &mut state,
            binding.id,
            &self.ownership,
            None,
        )
    }
}

fn same_source(left: InputPadBinding, right: InputPadBinding) -> bool {
    left.placement.and_then(|p| p.content_key()) == right.placement.and_then(|p| p.content_key())
        && left.source_address == right.source_address
        && left.sample_count == right.sample_count
        && left.channels == right.channels
        && left.sample_rate_hz == right.sample_rate_hz
        && left.authority_revision == right.authority_revision
        && left.accepted == right.accepted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::input_mapping::{
        InputAction, MidiNormalizer, RuntimePadState, dispatch_action, parse_input_action,
    };
    use crate::audio_engine::timing::OutputClockSnapshot;
    use crate::messages::ControlMessage;
    use rtrb::RingBuffer;

    fn fixture() -> (Arc<Mutex<RuntimeState>>, Arc<InputRuntimeOwnership>) {
        let ownership = Arc::new(InputRuntimeOwnership::default());
        let clock = Arc::new(SharedOutputClock::new());
        clock.publish(OutputClockSnapshot {
            valid: true,
            observed_at_ns: 10_000_000,
            audible_at_ns: 10_000_000,
            output_frame: 24_000,
            sample_rate_hz: 48_000,
            master_period_seconds: Some(0.5),
            downbeat_frame: 0,
            master_beat: 1.0,
            freshness_ns: 100_000_000,
        });
        let mut state = RuntimeState {
            pitch_clock: Some(clock),
            ..RuntimeState::default()
        };
        for id in [0, 1] {
            let native = InputRuntimePadBinding::for_test(id, ownership.clone());
            state.pads[id] = RuntimePadState {
                source_map: Default::default(),
                loaded: true,
                binding: Some(native.binding),
                acknowledgements: Some(native.acknowledgements.clone()),
                pitch: PitchPadProjection {
                    source_root: Some(4),
                    source_mode: Some(PitchKeyMode::Minor),
                    ..PitchPadProjection::default()
                },
                ..RuntimePadState::default()
            };
        }
        (Arc::new(Mutex::new(state)), ownership)
    }

    #[test]
    fn native_selection_then_pitch_keeps_original_content_after_next_selection() {
        let (state, ownership) = fixture();
        let (producer, mut consumer) = RingBuffer::<ControlMessage>::new(2);
        let producer = Arc::new(Mutex::new(producer));
        let selection = parse_input_action("ui.select_pad:0");
        assert!(matches!(selection, InputAction::SelectPad { id: 0 }));
        let projection =
            dispatch_action(&selection, 10_000_000, &state, &ownership, &producer, None);
        assert!(projection.dispatched && !projection.direct);
        let action = capture_selected(
            PitchActionSpec::parse("pitch.base:0:minor:set_retrigger").unwrap(),
            10_000_000,
            10_000_000,
            &state,
            &ownership,
            None,
        )
        .unwrap();
        select_pad(1, &state, &ownership).unwrap();
        assert_eq!(action.control_guard(None).unwrap().id, 0);
        assert_eq!(action.render().pitch.base_shift(), -4);
        assert_eq!(action.original_due_frame(), 24_000);
        assert_eq!(action.received_timestamp(), 10_000_000);
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn quantized_target_is_frozen_and_future_projection_does_not_stale_attack() {
        let (state, ownership) = fixture();
        select_pad(0, &state, &ownership).unwrap();
        state.lock().unwrap().pads[0].pitch.quantize_step_64ths = Some(4);
        let spec = PitchActionSpec::parse("pitch.extra:2:set_retrigger").unwrap();
        let action =
            capture_selected(spec, 34_000_000, 34_000_000, &state, &ownership, None).unwrap();
        assert_eq!(action.original_due_frame(), 24_000);
        action.mark_published().unwrap();
        state.lock().unwrap().pads[0].pitch = PitchPadProjection {
            intent: PitchIntent::new(6, -18).unwrap(),
            key_lock: false,
            sync: false,
            quantize_step_64ths: Some(1),
            ..PitchPadProjection::default()
        };
        assert!(action.control_guard(None).is_ok());
        assert_eq!(action.render().pitch.extra_shift(), 2);
        assert!(action.render().key_lock);
        assert_eq!(action.original_due_frame(), 24_000);
    }

    #[test]
    fn missing_or_stale_clock_is_explicit_rejection_and_empty_selection_clears_authority() {
        let (state, ownership) = fixture();
        select_pad(0, &state, &ownership).unwrap();
        let spec = PitchActionSpec::parse("pitch.extra:0:set").unwrap();
        assert!(matches!(
            capture_selected(spec, 300_000_000, 300_000_000, &state, &ownership, None),
            Err(PitchActionReject::TimingUnavailable)
        ));
        state.lock().unwrap().pitch_clock = None;
        assert!(matches!(
            capture_selected(spec, 10_000_000, 10_000_000, &state, &ownership, None),
            Err(PitchActionReject::TimingUnavailable)
        ));
        select_pad(2, &state, &ownership).unwrap();
        assert!(matches!(
            capture_selected(spec, 10_000_000, 10_000_000, &state, &ownership, None),
            Err(PitchActionReject::NoSelectedContent)
        ));
    }

    #[test]
    fn pitch_note_releases_never_reach_selection_or_action_capture() {
        let mut normalizer = MidiNormalizer::default();
        assert!(normalizer.normalize(&[0x90, 60, 127], 11).is_some());
        assert!(normalizer.normalize(&[0x80, 60, 0], 12).is_none());
        assert!(normalizer.normalize(&[0x90, 60, 0], 13).is_none());
    }

    #[test]
    fn rapid_extra_then_base_uses_native_requested_shadow_before_python_polling() {
        let (state, ownership) = fixture();
        select_pad(0, &state, &ownership).unwrap();
        let extra = capture_selected(
            PitchActionSpec::parse("pitch.extra:2:set_retrigger").unwrap(),
            10_000_000,
            10_000_000,
            &state,
            &ownership,
            None,
        )
        .unwrap();
        let base = capture_selected(
            PitchActionSpec::parse("pitch.base:0:minor:set_retrigger").unwrap(),
            10_000_000,
            10_000_000,
            &state,
            &ownership,
            None,
        )
        .unwrap();
        assert_eq!(extra.render().pitch, PitchIntent::new(0, 2).unwrap());
        assert_eq!(base.render().pitch, PitchIntent::new(-4, 2).unwrap());
        assert!(base.sequence() > extra.sequence());
        assert_eq!(
            state.lock().unwrap().pads[0].latest_request_sequence,
            base.sequence()
        );
        assert!(extra.control_guard(None).is_ok());
    }

    #[test]
    fn gui_and_native_selection_share_revision_and_empty_content_projects_no_audio_slot() {
        let (state, ownership) = fixture();
        let first = select_pad(0, &state, &ownership).unwrap();
        let second = select_pad(1, &state, &ownership).unwrap();
        assert!(second > first);
        assert_eq!(state.lock().unwrap().selection_revision, second);
        let empty = select_pad(2, &state, &ownership).unwrap();
        assert!(empty > second);
        let guard = state.lock().unwrap();
        assert_eq!(guard.selection_revision, empty);
        assert!(matches!(
            selected_row(&guard, &ownership),
            Err(PitchActionReject::NoSelectedContent)
        ));
    }

    #[test]
    fn dropped_pitch_receipt_rolls_back_only_its_own_shadow_and_keeps_newer_intent() {
        let (state, ownership) = fixture();
        select_pad(0, &state, &ownership).unwrap();
        let first = capture_selected(
            PitchActionSpec::parse("pitch.extra:2:set").unwrap(),
            10_000_000,
            10_000_000,
            &state,
            &ownership,
            None,
        )
        .unwrap();
        reject_undelivered_capture(&first, &state, &ownership);
        assert_eq!(
            state.lock().unwrap().pads[0].pitch.intent,
            PitchIntent::default()
        );
        let second = capture_selected(
            PitchActionSpec::parse("pitch.extra:4:set").unwrap(),
            10_000_000,
            10_000_000,
            &state,
            &ownership,
            None,
        )
        .unwrap();
        let third = capture_selected(
            PitchActionSpec::parse("pitch.extra:6:set").unwrap(),
            10_000_000,
            10_000_000,
            &state,
            &ownership,
            None,
        )
        .unwrap();
        reject_undelivered_capture(&second, &state, &ownership);
        assert_eq!(state.lock().unwrap().pads[0].pitch.intent.extra_shift(), 6);
        assert_eq!(
            state.lock().unwrap().pads[0].latest_request_sequence,
            third.sequence()
        );
    }

    #[test]
    fn old_python_pitch_projection_cannot_undo_new_native_shadow() {
        let (state, ownership) = fixture();
        let (producer, _consumer) = RingBuffer::<ControlMessage>::new(2);
        let runtime = InputRuntime::new_with_ownership(
            Arc::new(Mutex::new(producer)),
            crate::audio_engine::timing::InputClock::new(),
            ownership.clone(),
        );
        *runtime.runtime_state.lock().unwrap() = state.lock().unwrap().clone();
        select_pad(0, &runtime.runtime_state, &ownership).unwrap();
        let action = capture_selected(
            PitchActionSpec::parse("pitch.extra:2:set_retrigger").unwrap(),
            10_000_000,
            10_000_000,
            &runtime.runtime_state,
            &ownership,
            None,
        )
        .unwrap();
        let before = ownership
            .runtime_for_cell(action.content_key().cell())
            .load(Ordering::Acquire);
        let mut projections = vec![PitchPadProjection::default(); NUM_SAMPLES];
        projections[0].intent = PitchIntent::new(6, -18).unwrap();
        projections[0].correction_root = Some(7);
        projections[0].correction_mode = Some(PitchKeyMode::Minor);
        runtime
            .set_pitch_runtime_state(projections.clone())
            .unwrap();
        let guard = runtime.runtime_state.lock().unwrap();
        assert_eq!(guard.pads[0].pitch.intent.extra_shift(), 2);
        assert_eq!(guard.pads[0].pitch.correction_root, Some(7));
        drop(guard);
        assert_eq!(
            runtime.pitch_request_sequence(0).unwrap(),
            action.sequence()
        );
        assert_eq!(
            ownership
                .runtime_for_cell(action.content_key().cell())
                .load(Ordering::Acquire),
            before
        );
        projections[0].request_sequence = action.sequence();
        runtime.set_pitch_runtime_state(projections).unwrap();
        assert_eq!(
            runtime.runtime_state.lock().unwrap().pads[0].pitch.intent,
            PitchIntent::new(6, -18).unwrap()
        );
    }

    #[test]
    fn control_cancel_restores_only_its_shadow_and_rejects_foreign_or_terminal_owners() {
        let (state, ownership) = fixture();
        let (producer, _consumer) = RingBuffer::<ControlMessage>::new(2);
        let runtime = InputRuntime::new_with_ownership(
            Arc::new(Mutex::new(producer)),
            crate::audio_engine::timing::InputClock::new(),
            ownership.clone(),
        );
        *runtime.runtime_state.lock().unwrap() = state.lock().unwrap().clone();
        select_pad(0, &runtime.runtime_state, &ownership).unwrap();
        let first = capture_selected(
            PitchActionSpec::parse("pitch.extra:2:set").unwrap(),
            10_000_000,
            10_000_000,
            &runtime.runtime_state,
            &ownership,
            None,
        )
        .unwrap();
        let (other_producer, _other_consumer) = RingBuffer::<ControlMessage>::new(2);
        let foreign = InputRuntime::new(
            Arc::new(Mutex::new(other_producer)),
            crate::audio_engine::timing::InputClock::new(),
        );
        assert_eq!(
            foreign.cancel_pitch_capture(&first, PitchActionReject::Capacity),
            Err(PitchActionReject::StaleContent)
        );
        assert_eq!(first.state(), "captured");
        assert_eq!(
            runtime.cancel_pitch_capture(&first, PitchActionReject::Capacity),
            Ok(true)
        );
        assert_eq!(first.reason(), Some("capacity"));
        assert_eq!(
            runtime.runtime_state.lock().unwrap().pads[0].pitch.intent,
            PitchIntent::default()
        );
        assert_eq!(
            runtime.cancel_pitch_capture(&first, PitchActionReject::PreparationFailed),
            Ok(false)
        );
        let second = capture_selected(
            PitchActionSpec::parse("pitch.extra:4:set").unwrap(),
            10_000_000,
            10_000_000,
            &runtime.runtime_state,
            &ownership,
            None,
        )
        .unwrap();
        let third = capture_selected(
            PitchActionSpec::parse("pitch.extra:6:set").unwrap(),
            10_000_000,
            10_000_000,
            &runtime.runtime_state,
            &ownership,
            None,
        )
        .unwrap();
        assert_eq!(
            runtime.cancel_pitch_capture(&second, PitchActionReject::Capacity),
            Ok(true)
        );
        assert_eq!(
            runtime.runtime_state.lock().unwrap().pads[0]
                .pitch
                .intent
                .extra_shift(),
            6
        );
        assert_eq!(runtime.pitch_request_sequence(0).unwrap(), third.sequence());
        third.mark_scheduled();
        assert!(third.claim_effect());
        assert_eq!(
            runtime.cancel_pitch_capture(&third, PitchActionReject::Capacity),
            Ok(false)
        );
        assert_eq!(runtime.pitch_request_sequence(0).unwrap(), third.sequence());
        third.mark_effective(third.execution_target());
        assert_eq!(
            runtime.cancel_pitch_capture(&third, PitchActionReject::Capacity),
            Ok(false)
        );
        assert!(third.effective());
    }

    fn cancellation_runtime() -> InputRuntime {
        let (state, ownership) = fixture();
        let (producer, _consumer) = RingBuffer::<ControlMessage>::new(2);
        let runtime = InputRuntime::new_with_ownership(
            Arc::new(Mutex::new(producer)),
            crate::audio_engine::timing::InputClock::new(),
            ownership,
        );
        *runtime.runtime_state.lock().unwrap() = state.lock().unwrap().clone();
        runtime.set_selected_pad(0).unwrap();
        runtime
    }

    fn capture_for_cancel(runtime: &InputRuntime, key: &str) -> CapturedPitchAction {
        capture_selected(
            PitchActionSpec::parse(key).unwrap(),
            10_000_000,
            10_000_000,
            &runtime.runtime_state,
            &runtime.ownership,
            None,
        )
        .unwrap()
    }

    fn assert_next_base_extra(runtime: &InputRuntime, extra: i32) {
        let next = capture_for_cancel(runtime, "pitch.base:7:minor:set");
        assert_eq!(next.render().pitch, PitchIntent::new(3, extra).unwrap());
        assert!(
            runtime
                .cancel_pitch_capture(&next, PitchActionReject::InputDisabled)
                .unwrap()
        );
    }

    #[test]
    fn original_key_shadow_clears_correction_and_survives_a_stale_python_projection() {
        for unknown in [false, true] {
            let runtime = cancellation_runtime();
            let before = PitchPadProjection {
                source_root: Some(4),
                source_mode: Some(PitchKeyMode::Minor),
                correction_root: (!unknown).then_some(9),
                correction_mode: (!unknown).then_some(PitchKeyMode::Minor),
                correction_unknown: unknown,
                intent: PitchIntent::new(12, 2).unwrap(),
                ..PitchPadProjection::default()
            };
            let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
            rows[0] = before;
            runtime.set_pitch_runtime_state(rows.clone()).unwrap();
            let original = capture_for_cancel(&runtime, "pitch.original:set_retrigger");
            assert!(original.resets_key_correction());
            assert_eq!(original.render().pitch, PitchIntent::default());
            runtime.set_pitch_runtime_state(rows).unwrap();
            {
                let guard = runtime.runtime_state.lock().unwrap();
                let row = &guard.pads[0];
                assert_eq!(row.pitch.intent, PitchIntent::default());
                assert_eq!(row.pitch.source_key(), Some((4, PitchKeyMode::Minor)));
                assert_eq!(row.pitch.correction_root, None);
                assert!(!row.pitch.correction_unknown);
                assert_eq!(row.latest_request_sequence, original.sequence());
            }
            // A fast successor resolves against the retained detected reference,
            // before Python has projected the original reset's requested tuple.
            assert_next_base_extra(&runtime, 0);
            assert!(
                runtime
                    .cancel_pitch_capture(&original, PitchActionReject::Capacity)
                    .unwrap()
            );
            assert_eq!(runtime.runtime_state.lock().unwrap().pads[0].pitch, before);
        }
    }

    #[test]
    fn original_key_projection_anchors_correction_clear_when_its_audio_later_refuses() {
        let runtime = cancellation_runtime();
        let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
        rows[0] = PitchPadProjection {
            source_root: Some(4),
            source_mode: Some(PitchKeyMode::Minor),
            correction_root: Some(9),
            correction_mode: Some(PitchKeyMode::Minor),
            intent: PitchIntent::new(-12, 2).unwrap(),
            ..PitchPadProjection::default()
        };
        runtime.set_pitch_runtime_state(rows.clone()).unwrap();
        let original = capture_for_cancel(&runtime, "pitch.original:set");
        rows[0].correction_root = None;
        rows[0].correction_mode = None;
        rows[0].intent = PitchIntent::default();
        rows[0].request_sequence = original.sequence();
        runtime.set_pitch_runtime_state(rows.clone()).unwrap();
        let successor = capture_for_cancel(&runtime, "pitch.note:12:set");
        assert!(original.reject(PitchActionReject::PreparationFailed));
        assert!(
            runtime
                .cancel_pitch_capture(&successor, PitchActionReject::Capacity)
                .unwrap()
        );
        assert_next_base_extra(&runtime, 0);
        assert_eq!(
            runtime.runtime_state.lock().unwrap().pads[0]
                .pitch
                .correction_root,
            None
        );
    }

    #[test]
    fn ordinary_numeric_cancel_preserves_an_independent_metadata_projection() {
        let runtime = cancellation_runtime();
        let action = capture_for_cancel(&runtime, "pitch.extra:2:set");
        let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
        rows[0] = PitchPadProjection {
            source_root: Some(4),
            source_mode: Some(PitchKeyMode::Minor),
            correction_root: Some(7),
            correction_mode: Some(PitchKeyMode::Minor),
            ..PitchPadProjection::default()
        };
        runtime.set_pitch_runtime_state(rows).unwrap();
        assert!(
            runtime
                .cancel_pitch_capture(&action, PitchActionReject::Capacity)
                .unwrap()
        );
        let guard = runtime.runtime_state.lock().unwrap();
        assert_eq!(guard.pads[0].pitch.intent, PitchIntent::default());
        assert_eq!(guard.pads[0].pitch.correction_root, Some(7));
        assert_eq!(guard.pads[0].pitch.correction_reset_sequence, 0);
    }

    #[test]
    fn original_key_then_note_protects_only_the_unacknowledged_correction_clear() {
        let runtime = cancellation_runtime();
        let before = PitchPadProjection {
            source_root: Some(4),
            source_mode: Some(PitchKeyMode::Minor),
            correction_root: Some(9),
            correction_mode: Some(PitchKeyMode::Minor),
            intent: PitchIntent::new(2, 3).unwrap(),
            ..PitchPadProjection::default()
        };
        let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
        rows[0] = before;
        runtime.set_pitch_runtime_state(rows.clone()).unwrap();
        let original = capture_for_cancel(&runtime, "pitch.original:set");
        let note = capture_for_cancel(&runtime, "pitch.note:12:set");
        runtime.set_pitch_runtime_state(rows.clone()).unwrap();
        {
            let guard = runtime.runtime_state.lock().unwrap();
            assert_eq!(guard.pads[0].pitch.intent, PitchIntent::new(12, 0).unwrap());
            assert_eq!(guard.pads[0].pitch.correction_root, None);
            assert_eq!(
                guard.pads[0].pitch.correction_reset_sequence,
                original.sequence()
            );
        }
        rows[0].intent = original.render().pitch;
        rows[0].correction_root = None;
        rows[0].correction_mode = None;
        rows[0].request_sequence = original.sequence();
        runtime.set_pitch_runtime_state(rows.clone()).unwrap();
        // After Python acknowledges the reset, correction metadata is again
        // independent of the newer pending numeric note's request sequence.
        rows[0].correction_root = Some(7);
        rows[0].correction_mode = Some(PitchKeyMode::Minor);
        runtime.set_pitch_runtime_state(rows).unwrap();
        assert!(
            runtime
                .cancel_pitch_capture(&note, PitchActionReject::Capacity)
                .unwrap()
        );
        let guard = runtime.runtime_state.lock().unwrap();
        assert_eq!(guard.pads[0].pitch.intent, PitchIntent::default());
        assert_eq!(guard.pads[0].pitch.correction_root, Some(7));
        assert_eq!(guard.pads[0].pitch.correction_reset_sequence, 0);
    }

    #[test]
    fn cancelled_original_clear_does_not_leave_a_later_note_owning_that_metadata_change() {
        let runtime = cancellation_runtime();
        let before = PitchPadProjection {
            source_root: Some(4),
            source_mode: Some(PitchKeyMode::Minor),
            correction_root: Some(9),
            correction_mode: Some(PitchKeyMode::Minor),
            intent: PitchIntent::new(2, 3).unwrap(),
            ..PitchPadProjection::default()
        };
        let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
        rows[0] = before;
        runtime.set_pitch_runtime_state(rows).unwrap();
        let original = capture_for_cancel(&runtime, "pitch.original:set");
        let note = capture_for_cancel(&runtime, "pitch.note:12:set");
        assert!(
            runtime
                .cancel_pitch_capture(&original, PitchActionReject::Capacity)
                .unwrap()
        );
        {
            let guard = runtime.runtime_state.lock().unwrap();
            assert_eq!(guard.pads[0].pitch.intent, note.render().pitch);
            assert_eq!(guard.pads[0].pitch.correction_root, before.correction_root);
            assert_eq!(guard.pads[0].pitch.correction_reset_sequence, 0);
        }
        assert!(
            runtime
                .cancel_pitch_capture(&note, PitchActionReject::Capacity)
                .unwrap()
        );
        assert_eq!(runtime.runtime_state.lock().unwrap().pads[0].pitch, before);
    }

    #[test]
    fn unsupported_piano_sum_does_not_change_the_requested_tuple_or_sequence() {
        let runtime = cancellation_runtime();
        let first = capture_for_cancel(&runtime, "pitch.extra:18:set");
        let refused = capture_selected(
            PitchActionSpec::parse("pitch.note:23:set").unwrap(),
            10_000_000,
            10_000_000,
            &runtime.runtime_state,
            &runtime.ownership,
            None,
        );
        assert!(matches!(refused, Err(PitchActionReject::UnsupportedPitch)));
        let guard = runtime.runtime_state.lock().unwrap();
        assert_eq!(guard.pads[0].pitch.intent, first.render().pitch);
        assert_eq!(guard.pads[0].latest_request_sequence, first.sequence());
    }

    #[test]
    fn rapid_relative_steps_and_repeated_notes_each_keep_one_original_capture() {
        let runtime = cancellation_runtime();
        let first = capture_for_cancel(&runtime, "pitch.extra.step:1:set");
        let second = capture_for_cancel(&runtime, "pitch.extra.step:1:set");
        assert_eq!(first.render().pitch, PitchIntent::new(0, 1).unwrap());
        assert_eq!(second.render().pitch, PitchIntent::new(0, 2).unwrap());
        let note = capture_for_cancel(&runtime, "pitch.note:12:set_retrigger");
        let repeated = capture_for_cancel(&runtime, "pitch.note:12:set_retrigger");
        assert_eq!(note.render().pitch, PitchIntent::new(12, 2).unwrap());
        assert_eq!(repeated.render().pitch, note.render().pitch);
        assert!(first.sequence() < second.sequence());
        assert!(second.sequence() < note.sequence());
        assert!(note.sequence() < repeated.sequence());
        assert_eq!(
            note.variant(),
            crate::audio_engine::pitch_actions::PitchActionVariant::SetRetrigger
        );
        assert_eq!(repeated.original_due_frame(), note.original_due_frame());
        assert_eq!(repeated.received_timestamp(), note.received_timestamp());
        assert_eq!(repeated.loop_region(), note.loop_region());
        assert_eq!(repeated.sync(), note.sync());
    }

    #[test]
    fn control_cancel_two_and_three_captures_in_every_order_skips_rejected_predecessors() {
        for order in [
            vec![0, 1],
            vec![1, 0],
            vec![0, 1, 2],
            vec![0, 2, 1],
            vec![1, 0, 2],
            vec![1, 2, 0],
            vec![2, 0, 1],
            vec![2, 1, 0],
        ] {
            let runtime = cancellation_runtime();
            let actions: Vec<_> = [2, 5, 7]
                .into_iter()
                .take(order.len())
                .map(|extra| capture_for_cancel(&runtime, &format!("pitch.extra:{extra}:set")))
                .collect();
            let mut discarded = vec![false; actions.len()];
            for index in order {
                assert!(
                    runtime
                        .cancel_pitch_capture(&actions[index], PitchActionReject::InputDisabled)
                        .unwrap()
                );
                discarded[index] = true;
                let expected = actions
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(i, _)| !discarded[*i]);
                let state = runtime.runtime_state.lock().unwrap();
                assert_eq!(
                    state.pads[0].pitch.intent,
                    expected.map_or(PitchIntent::default(), |(_, action)| action.render().pitch)
                );
                assert_eq!(
                    state.pads[0].latest_request_sequence,
                    expected.map_or(0, |(_, action)| action.sequence())
                );
                drop(state);
                // Each cancellation may be a different Python polling batch.
                // Older metadata projection must not resurrect a discarded shadow.
                let mut projection = vec![PitchPadProjection::default(); NUM_SAMPLES];
                projection[0].source_root = Some(4);
                projection[0].source_mode = Some(PitchKeyMode::Minor);
                runtime.set_pitch_runtime_state(projection).unwrap();
            }
            assert!(actions.iter().all(|action| action.state() == "rejected"));
            assert_next_base_extra(&runtime, 0);
        }
    }

    #[test]
    fn control_cancel_reclaims_all_64_records_and_reuses_capacity_without_shadow_mutation_on_overflow()
     {
        let runtime = cancellation_runtime();
        let actions: Vec<_> = (0..REQUESTED_PITCH_CAPACITY)
            .map(|index| {
                capture_for_cancel(&runtime, &format!("pitch.extra:{}:set", index % 18 + 1))
            })
            .collect();
        let last = actions.last().unwrap();
        assert!(matches!(
            capture_selected(
                PitchActionSpec::parse("pitch.extra:0:set").unwrap(),
                10_000_000,
                10_000_000,
                &runtime.runtime_state,
                &runtime.ownership,
                None
            ),
            Err(PitchActionReject::Capacity)
        ));
        assert_eq!(runtime.pitch_request_sequence(0).unwrap(), last.sequence());
        assert_eq!(
            runtime.runtime_state.lock().unwrap().pads[0].pitch.intent,
            last.render().pitch
        );
        assert!(actions.iter().all(|action| action.state() == "captured"));
        assert!(
            runtime
                .cancel_pitch_capture(&actions[0], PitchActionReject::Capacity)
                .unwrap()
        );
        let reused = capture_for_cancel(&runtime, "pitch.extra:7:set");
        for action in actions.iter().skip(1) {
            assert!(
                runtime
                    .cancel_pitch_capture(action, PitchActionReject::Capacity)
                    .unwrap()
            );
            assert_eq!(
                runtime.pitch_request_sequence(0).unwrap(),
                reused.sequence()
            );
        }
        assert!(
            runtime
                .cancel_pitch_capture(&reused, PitchActionReject::Capacity)
                .unwrap()
        );
        assert_eq!(
            runtime
                .runtime_state
                .lock()
                .unwrap()
                .pitch_shadow
                .records
                .iter()
                .flatten()
                .count(),
            0
        );
        assert_next_base_extra(&runtime, 0);
    }

    #[test]
    fn control_cancel_reconciles_already_rejected_receipts_before_next_capture() {
        let runtime = cancellation_runtime();
        let first = capture_for_cancel(&runtime, "pitch.extra:2:set");
        let second = capture_for_cancel(&runtime, "pitch.extra:5:set");
        assert!(first.reject(PitchActionReject::PreparationFailed));
        // A callback/worker may have already rejected the exact owner before
        // the controller drains that receipt. Repeated cancel stays false.
        assert!(
            !runtime
                .cancel_pitch_capture(&first, PitchActionReject::InputDisabled)
                .unwrap()
        );
        assert!(second.reject(PitchActionReject::PreparationFailed));
        assert_next_base_extra(&runtime, 0);
        assert_eq!(first.reason(), Some("preparation_failed"));
        assert_eq!(second.reason(), Some("preparation_failed"));
    }

    #[test]
    fn control_cancel_preserves_retained_newer_and_applied_scalar_anchor() {
        let runtime = cancellation_runtime();
        let first = capture_for_cancel(&runtime, "pitch.extra:2:set");
        let second = capture_for_cancel(&runtime, "pitch.extra:5:set");
        assert!(
            runtime
                .cancel_pitch_capture(&first, PitchActionReject::InputDisabled)
                .unwrap()
        );
        assert_next_base_extra(&runtime, 5);
        second.mark_scheduled();
        assert!(second.claim_effect());
        second.mark_effective(second.execution_target());
        let third = capture_for_cancel(&runtime, "pitch.extra:7:set");
        assert!(
            !runtime
                .cancel_pitch_capture(&second, PitchActionReject::Capacity)
                .unwrap()
        );
        assert!(
            runtime
                .cancel_pitch_capture(&third, PitchActionReject::Capacity)
                .unwrap()
        );
        assert_next_base_extra(&runtime, 5);
        assert_eq!(
            runtime.pitch_request_sequence(0).unwrap(),
            second.sequence()
        );
    }

    #[test]
    fn control_cancel_old_and_equal_captures_cannot_undo_authoritative_python_anchor() {
        for newer_projection in [false, true] {
            let runtime = cancellation_runtime();
            let first = capture_for_cancel(&runtime, "pitch.extra:2:set");
            let second = capture_for_cancel(&runtime, "pitch.extra:5:set");
            let mut projection = vec![PitchPadProjection::default(); NUM_SAMPLES];
            projection[0] = PitchPadProjection {
                source_root: Some(4),
                source_mode: Some(PitchKeyMode::Minor),
                intent: PitchIntent::new(0, 9).unwrap(),
                request_sequence: second.sequence() + u64::from(newer_projection),
                ..PitchPadProjection::default()
            };
            let before = runtime
                .ownership
                .runtime_for_cell(second.content_key().cell())
                .load(Ordering::Acquire);
            runtime.set_pitch_runtime_state(projection.clone()).unwrap();
            assert!(
                runtime
                    .cancel_pitch_capture(&first, PitchActionReject::InputDisabled)
                    .unwrap()
            );
            assert!(
                runtime
                    .cancel_pitch_capture(&second, PitchActionReject::InputDisabled)
                    .unwrap()
            );
            assert_eq!(
                runtime.runtime_state.lock().unwrap().pads[0].pitch.intent,
                projection[0].intent
            );
            assert_eq!(
                runtime.pitch_request_sequence(0).unwrap(),
                projection[0].request_sequence
            );
            assert_eq!(
                runtime
                    .ownership
                    .runtime_for_cell(second.content_key().cell())
                    .load(Ordering::Acquire),
                before
            );
            assert_next_base_extra(&runtime, 9);
        }
    }

    #[test]
    fn control_cancel_pending_successor_restores_projected_predecessor_after_its_audio_rejection() {
        let runtime = cancellation_runtime();
        let first = capture_for_cancel(&runtime, "pitch.extra:2:set");
        first.mark_published().unwrap();
        let second = capture_for_cancel(&runtime, "pitch.extra:5:set");
        let key = first.content_key();
        let runtime_before = runtime
            .ownership
            .runtime_for_cell(key.cell())
            .load(Ordering::Acquire);
        let authority_before = runtime
            .ownership
            .authority_for_cell(key.cell())
            .load(Ordering::Acquire);
        let source_before = runtime.ownership.key_source_request_incarnation(key);
        let mut projection = vec![PitchPadProjection::default(); NUM_SAMPLES];
        projection[0] = PitchPadProjection {
            source_root: Some(4),
            source_mode: Some(PitchKeyMode::Minor),
            intent: first.render().pitch,
            request_sequence: first.sequence(),
            ..PitchPadProjection::default()
        };
        runtime.set_pitch_runtime_state(projection.clone()).unwrap();
        assert_eq!(
            runtime.pitch_request_sequence(0).unwrap(),
            second.sequence()
        );
        assert_eq!(
            runtime.runtime_state.lock().unwrap().pads[0].pitch.intent,
            second.render().pitch
        );
        assert!(first.reject(PitchActionReject::PreparationFailed));
        assert!(
            runtime
                .cancel_pitch_capture(&second, PitchActionReject::InputDisabled)
                .unwrap()
        );
        assert_eq!(runtime.pitch_request_sequence(0).unwrap(), first.sequence());
        assert_eq!(
            runtime.runtime_state.lock().unwrap().pads[0].pitch.intent,
            first.render().pitch
        );
        // No new Python projection is necessary: its A signature is unchanged.
        // An older projection cannot replace that accepted predecessor anchor.
        projection[0].request_sequence = 0;
        projection[0].intent = PitchIntent::default();
        runtime.set_pitch_runtime_state(projection).unwrap();
        assert_next_base_extra(&runtime, 2);
        assert_eq!(runtime.pitch_request_sequence(0).unwrap(), first.sequence());
        assert_eq!(
            runtime
                .ownership
                .runtime_for_cell(key.cell())
                .load(Ordering::Acquire),
            runtime_before
        );
        assert_eq!(
            runtime
                .ownership
                .authority_for_cell(key.cell())
                .load(Ordering::Acquire),
            authority_before
        );
        assert_eq!(
            runtime.ownership.key_source_request_incarnation(key),
            source_before
        );
        assert_eq!(first.reason(), Some("preparation_failed"));
        assert_eq!(second.reason(), Some("input_disabled"));
    }

    fn publish_single_runtime(
        runtime: &InputRuntime,
        binding: &InputRuntimePadBinding,
        multi_loop: bool,
    ) {
        let mut loaded = vec![false; NUM_SAMPLES];
        loaded[binding.id] = true;
        let mut bindings = vec![None; NUM_SAMPLES];
        bindings[binding.id] = Some(binding);
        runtime
            .set_runtime_state(
                multi_loop,
                loaded,
                vec![0.0; NUM_SAMPLES],
                vec![None; NUM_SAMPLES],
                bindings,
            )
            .unwrap();
        runtime.set_selected_pad(binding.id).unwrap();
    }

    #[test]
    fn control_cancel_source_aba_and_authority_refresh_anchor_next_composition() {
        for replace_lifetime in [false, true] {
            let runtime = cancellation_runtime();
            let old = capture_for_cancel(&runtime, "pitch.extra:2:set");
            if replace_lifetime {
                runtime.ownership.publish_content_lifetime(
                    0,
                    crate::audio_engine::content_lifetime::reserve().unwrap(),
                );
            } else {
                let authority = runtime.ownership.next_authority(0).unwrap();
                runtime.ownership.revoke(0, authority);
                runtime
                    .ownership
                    .prepare_source_request_advance(0)
                    .unwrap()
                    .commit();
            }
            let replacement = InputRuntimePadBinding::for_test(0, runtime.ownership.clone());
            publish_single_runtime(&runtime, &replacement, false);
            assert_eq!(
                runtime.runtime_state.lock().unwrap().pads[0].pitch.intent,
                PitchIntent::default()
            );
            assert_eq!(runtime.pitch_request_sequence(0).unwrap(), 0);
            assert!(matches!(
                old.control_guard(None),
                Err(PitchActionReject::StaleContent | PitchActionReject::SourceChanged)
            ));
            assert!(
                runtime
                    .cancel_pitch_capture(&old, PitchActionReject::SourceChanged)
                    .unwrap()
            );
            // Exercise a fresh composition after the complete native runtime
            // publication; merely leaving the replacement untouched is too weak.
            let mut projection = vec![PitchPadProjection::default(); NUM_SAMPLES];
            projection[0].source_root = Some(4);
            projection[0].source_mode = Some(PitchKeyMode::Minor);
            runtime.set_pitch_runtime_state(projection).unwrap();
            assert_next_base_extra(&runtime, 0);
            assert!(old.control_guard(None).is_err());
        }
    }

    #[test]
    fn control_cancel_timing_refresh_does_not_leave_unpublished_shadow_for_next_base() {
        use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
        let runtime = cancellation_runtime();
        let old = capture_for_cancel(&runtime, "pitch.extra:2:set");
        runtime.ownership.set_timing_intent(0, TimingIntent::Manual);
        assert!(matches!(
            old.control_guard(None),
            Err(PitchActionReject::TimingChanged)
        ));
        assert!(
            runtime
                .cancel_pitch_capture(&old, PitchActionReject::TimingChanged)
                .unwrap()
        );
        assert_next_base_extra(&runtime, 0);
        assert_eq!(
            runtime.ownership.binding_timing_intent(
                0,
                runtime.runtime_state.lock().unwrap().pads[0]
                    .binding
                    .unwrap()
            ),
            Some(2)
        );
    }

    #[test]
    fn control_cancel_same_source_runtime_refresh_preserves_independent_requested_intent() {
        let runtime = cancellation_runtime();
        let first = capture_for_cancel(&runtime, "pitch.extra:2:set");
        let second = capture_for_cancel(&runtime, "pitch.extra:5:set");
        let binding = InputRuntimePadBinding::for_test(0, runtime.ownership.clone());
        publish_single_runtime(&runtime, &binding, true);
        assert_eq!(
            runtime.pitch_request_sequence(0).unwrap(),
            second.sequence()
        );
        assert_next_base_extra(&runtime, 5);
        assert!(
            runtime
                .cancel_pitch_capture(&first, PitchActionReject::InputDisabled)
                .unwrap()
        );
        assert!(
            runtime
                .cancel_pitch_capture(&second, PitchActionReject::InputDisabled)
                .unwrap()
        );
        assert_next_base_extra(&runtime, 0);
    }

    #[test]
    fn control_cancel_journal_follows_content_through_complete_native_move() {
        let runtime = cancellation_runtime();
        let sample = crate::messages::SampleBuffer {
            channels: 1,
            samples: Arc::from([0.25_f32]),
            residency: None,
        };
        runtime.runtime_state.lock().unwrap().pads[0]
            .binding
            .as_mut()
            .unwrap()
            .source_address = sample.source_address();
        runtime.set_selected_pad(0).unwrap();
        let first = capture_for_cancel(&runtime, "pitch.extra:2:set");
        let second = capture_for_cancel(&runtime, "pitch.extra:5:set");
        let ownership = &runtime.ownership;
        let original = [
            ownership.slot_snapshot(0).unwrap(),
            ownership.slot_snapshot(2).unwrap(),
        ];
        let captured_rows = runtime.capture_pair_rows(original).unwrap();
        let prepared = ownership
            .prepare_placement_pair(original[0], original[1])
            .unwrap();
        let moved = ownership
            .commit_placement_pair(&prepared, true, || true, || {})
            .unwrap();
        assert!(matches!(
            ownership.observe_control_key(first.content_key()),
            PlacementObservation::Pending
        ));
        let after = [moved.source_after, moved.target_after];
        let mut mirror = runtime
            .prepare_pair_mirror(&captured_rows, after, &[Some(sample.clone()), None])
            .unwrap();
        let original_current = mirror.original_current();
        let bindings = mirror.publication_bindings();
        let write = ownership
            .prepare_slot_write_fixed(after[0], after[0].lifetime())
            .unwrap();
        // Production completes its row mirror and control-cell map together.
        // Audio placement alone deliberately keeps original owners Pending.
        ownership
            .commit_slot_write_fixed(
                &write,
                || {
                    original_current
                        && after
                            .iter()
                            .zip(bindings)
                            .all(|(snapshot, (loaded, binding))| {
                                ownership.slot_snapshot_fields_current(*snapshot)
                                    && (!loaded
                                        || binding.is_some_and(|binding| {
                                            binding.placement == Some(*snapshot)
                                                && ownership.current(snapshot.slot(), binding)
                                        }))
                            })
                },
                |_| {
                    mirror.apply();
                    ownership.publish_pair_control_slots(&after);
                },
            )
            .unwrap();
        drop(mirror);
        assert!(
            matches!(ownership.observe_control_key(first.content_key()), PlacementObservation::Current(current) if current.slot() == 2 && current.content_key() == Some(first.content_key()))
        );
        let binding = runtime.pair_binding_for_test(2).unwrap();
        assert_eq!(
            binding.source_address,
            first.control_guard(None).unwrap().binding.source_address
        );
        assert!(ownership.admission_current(2, binding));
        assert!(
            runtime
                .cancel_pitch_capture(&first, PitchActionReject::InputDisabled)
                .unwrap()
        );
        assert_eq!(
            runtime.pitch_request_sequence(2).unwrap(),
            second.sequence()
        );
        assert_eq!(runtime.pitch_request_sequence(0).unwrap(), 0);
        assert_next_base_extra(&runtime, 5);
        assert!(
            runtime
                .cancel_pitch_capture(&second, PitchActionReject::InputDisabled)
                .unwrap()
        );
        assert_next_base_extra(&runtime, 0);
    }
}
