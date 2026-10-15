//! Immutable pitch input and per-action native preparation ownership.
//!
//! Control allocates the owner and native adapter. Callback capture only moves a fixed
//! request into that owner. The existing scheduler retains each work item separately;
//! preparation or a newer pitch request never replaces another accepted attack.

use super::constant_timing::CurrentTimingAcknowledgements;
use super::content_placement::{ContentKey, PlacementObservation};
use super::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use super::native_history_permit::{NativeHistoryContext, NativeHistoryPermit};
use super::pitch_intent::PitchIntent;
use super::prepared_native_history::{NativeAdapterState, NativeHistoryRequest};
use super::rubberband_backend::{RubberBandError, RubberBandLiveShifter};
use super::source_feed_history::{SourceFeedContext, SourceFeedSnapshot};
use super::source_playback::SourcePlayback;
use super::source_reader::SourceReadPlan;
use crate::messages::SampleBuffer;
use pyo3::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

mod preparation;
pub(crate) use preparation::{
    PitchActionPreparation, PitchActionPreparationWorker, PreparedActionWork,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PitchKeyMode {
    Major,
    Minor,
}

impl PitchKeyMode {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "major" => Some(Self::Major),
            "minor" => Some(Self::Minor),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PitchActionVariant {
    Set,
    SetRetrigger,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PitchActionTarget {
    Base { root: u8, mode: PitchKeyMode },
    ResetBase,
    Extra(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PitchActionSpec {
    pub(crate) target: PitchActionTarget,
    pub(crate) variant: PitchActionVariant,
}

impl PitchActionSpec {
    pub(crate) fn parse(key: &str) -> Option<Self> {
        let fields: Vec<_> = key.split(':').collect();
        let variant = match *fields.last()? {
            "set" => PitchActionVariant::Set,
            "set_retrigger" => PitchActionVariant::SetRetrigger,
            _ => return None,
        };
        let target = match fields.as_slice() {
            ["pitch.base.reset", _] => PitchActionTarget::ResetBase,
            ["pitch.base", root, mode, _] => {
                let root: u8 = root.parse().ok()?;
                if root > 11 || root.to_string() != fields[1] {
                    return None;
                }
                PitchActionTarget::Base {
                    root,
                    mode: PitchKeyMode::parse(mode)?,
                }
            }
            ["pitch.extra", extra, _] => {
                let extra: i32 = extra.parse().ok()?;
                if !(-18..=18).contains(&extra) || extra.to_string() != fields[1] {
                    return None;
                }
                PitchActionTarget::Extra(extra)
            }
            _ => return None,
        };
        Some(Self { target, variant })
    }

    pub(crate) fn resolve(
        self,
        source: Option<(u8, PitchKeyMode)>,
        current: PitchIntent,
    ) -> Result<PitchIntent, PitchActionReject> {
        let (base, extra) = match self.target {
            PitchActionTarget::Base { root, mode } => {
                let (source_root, source_mode) =
                    source.ok_or(PitchActionReject::UnknownSourceKey)?;
                if source_root > 11 || root > 11 {
                    return Err(PitchActionReject::InvalidAction);
                }
                if source_mode != mode {
                    return Err(PitchActionReject::ModeMismatch);
                }
                let mut difference = (i32::from(root) - i32::from(source_root)).rem_euclid(12);
                if difference > 6 {
                    difference -= 12;
                }
                (difference, current.extra_shift())
            }
            PitchActionTarget::Extra(extra) => (current.base_shift(), extra),
            PitchActionTarget::ResetBase => (0, current.extra_shift()),
        };
        PitchIntent::new(base, extra).map_err(|_| PitchActionReject::UnsupportedPitch)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrozenPitchRender {
    pub(crate) pitch: PitchIntent,
    pub(crate) key_lock: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum PitchActionReject {
    InvalidAction = 1,
    NoSelectedContent,
    StaleContent,
    SourceChanged,
    TimingChanged,
    TimingUnavailable,
    UnknownSourceKey,
    ModeMismatch,
    UnsupportedPitch,
    InputDisabled,
    PlacementPending,
    WindowPending,
    Capacity,
    PreparationFailed,
    PreparedMismatch,
    AlreadyPublished,
    SequenceExhausted,
    LaunchCancelled,
}

impl PitchActionReject {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidAction => "invalid_action",
            Self::NoSelectedContent => "no_selected_content",
            Self::StaleContent => "stale_content",
            Self::SourceChanged => "source_changed",
            Self::TimingChanged => "timing_changed",
            Self::TimingUnavailable => "timing_unavailable",
            Self::UnknownSourceKey => "unknown_source_key",
            Self::ModeMismatch => "mode_mismatch",
            Self::UnsupportedPitch => "unsupported_pitch",
            Self::InputDisabled => "input_disabled",
            Self::PlacementPending => "placement_pending",
            Self::WindowPending => "window_pending",
            Self::Capacity => "capacity",
            Self::PreparationFailed => "preparation_failed",
            Self::PreparedMismatch => "prepared_mismatch",
            Self::AlreadyPublished => "already_published",
            Self::SequenceExhausted => "sequence_exhausted",
            Self::LaunchCancelled => "launch_cancelled",
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::InvalidAction,
            2 => Self::NoSelectedContent,
            3 => Self::StaleContent,
            4 => Self::SourceChanged,
            5 => Self::TimingChanged,
            6 => Self::TimingUnavailable,
            7 => Self::UnknownSourceKey,
            8 => Self::ModeMismatch,
            9 => Self::UnsupportedPitch,
            10 => Self::InputDisabled,
            11 => Self::PlacementPending,
            12 => Self::WindowPending,
            13 => Self::Capacity,
            14 => Self::PreparationFailed,
            15 => Self::PreparedMismatch,
            16 => Self::AlreadyPublished,
            17 => Self::SequenceExhausted,
            18 => Self::LaunchCancelled,
            _ => return None,
        })
    }
}

impl std::fmt::Display for PitchActionReject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source/scalar capture is control-owned; it grants no PCM custody by itself.
pub(crate) struct PitchActionAdmission {
    pub(crate) key: ContentKey,
    pub(crate) binding: InputPadBinding,
    pub(crate) ownership: Arc<InputRuntimeOwnership>,
    pub(crate) acknowledgements: Arc<CurrentTimingAcknowledgements>,
    pub(crate) spec: PitchActionSpec,
    pub(crate) render: FrozenPitchRender,
    pub(crate) received_at_ns: u64,
    pub(crate) due_frame: u64,
    pub(crate) quantize_step_64ths: Option<u32>,
    pub(crate) sync: bool,
    pub(crate) exclusive: bool,
    pub(crate) loop_start_s: f64,
    pub(crate) loop_end_s: Option<f64>,
    pub(crate) input_owners: Option<(Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>)>,
    pub(crate) previous_requested: PitchIntent,
    pub(crate) previous_sequence: u64,
}

struct PitchActionOwner {
    admission: PitchActionAdmission,
    sequence: u64,
    source_generation: u64,
    source_request_incarnation: u64,
    timing_intent: u8,
    launch_cancel_revision: u64,
    preparation_epoch: Arc<AtomicU64>,
    published: AtomicBool,
    admitted: AtomicBool,
    status: Arc<AtomicU8>,
    reason: AtomicU8,
    execution_frame: AtomicU64,
    effective_frame: AtomicU64,
}

const CAPTURED: u8 = 0;
const PREPARING: u8 = 1;
const PREPARED: u8 = 2;
const SCHEDULED: u8 = 3;
const APPLIED: u8 = 4;
const REJECTED: u8 = 5;
const APPLYING: u8 = 6;

/// A control journal observes terminal state without retaining the action, Native
/// candidate, preparation permit, source pin, or any predecessor action owner.
#[derive(Clone)]
pub(crate) struct PitchActionShadow {
    binding: InputPadBinding,
    source_generation: u64,
    source_request_incarnation: u64,
    timing_intent: u8,
    status: Arc<AtomicU8>,
}

impl PitchActionShadow {
    pub(crate) fn current(
        &self,
        id: usize,
        binding: InputPadBinding,
        ownership: &InputRuntimeOwnership,
        acknowledgements: &CurrentTimingAcknowledgements,
    ) -> bool {
        let original = self.binding;
        let key = original.placement.and_then(|p| p.content_key());
        key.is_some()
            && binding.placement.and_then(|p| p.content_key()) == key
            && binding.source_address == original.source_address
            && binding.sample_count == original.sample_count
            && binding.channels == original.channels
            && binding.sample_rate_hz == original.sample_rate_hz
            && binding.authority_revision == original.authority_revision
            && binding.accepted == original.accepted
            && ownership.authority_current(id, binding)
            && ownership.binding_source_generation(id, binding) == Some(self.source_generation)
            && ownership.key_source_request_incarnation(key.expect("checked content key"))
                == Some(self.source_request_incarnation)
            && ownership.binding_timing_intent(id, binding) == Some(self.timing_intent)
            && acknowledgements.current_for_binding(ownership, id, binding)
                == original
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
            && ownership.source_timing_available_for_binding(id, binding, acknowledgements)
    }

    pub(crate) fn rejected(&self) -> bool {
        self.status.load(Ordering::Acquire) == REJECTED
    }

    pub(crate) fn applied(&self) -> bool {
        self.status.load(Ordering::Acquire) == APPLIED
    }
}

/// Original GUI/MIDI capture. Public Python fields are diagnostics, never authority.
#[pyclass(frozen)]
#[derive(Clone)]
pub struct CapturedPitchAction {
    owner: Arc<PitchActionOwner>,
}

impl std::fmt::Debug for CapturedPitchAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapturedPitchAction")
            .field("sequence", &self.owner.sequence)
            .field("content", &self.owner.admission.key)
            .field("due_frame", &self.owner.admission.due_frame)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PitchActionGuard {
    pub(crate) id: usize,
    pub(crate) binding: InputPadBinding,
}

impl CapturedPitchAction {
    pub(crate) fn capture(admission: PitchActionAdmission) -> Result<Self, PitchActionReject> {
        let id = admission
            .binding
            .placement
            .ok_or(PitchActionReject::StaleContent)?
            .slot();
        if admission.binding.placement.and_then(|p| p.content_key()) != Some(admission.key)
            || !admission.ownership.admission_current(id, admission.binding)
        {
            return Err(PitchActionReject::StaleContent);
        }
        let source_generation = admission
            .ownership
            .binding_source_generation(id, admission.binding)
            .ok_or(PitchActionReject::SourceChanged)?;
        let source_request_incarnation = admission
            .ownership
            .binding_source_request_incarnation(id, admission.binding)
            .ok_or(PitchActionReject::SourceChanged)?;
        let timing_intent = admission
            .ownership
            .binding_timing_intent(id, admission.binding)
            .ok_or(PitchActionReject::TimingChanged)?;
        let launch_cancel_revision = admission
            .ownership
            .launch_revision_for_cell(admission.key.cell())
            .load(Ordering::Acquire);
        let sequence =
            super::scheduler::reserve_input_order().ok_or(PitchActionReject::SequenceExhausted)?;
        let due_frame = admission.due_frame;
        let action = Self {
            owner: Arc::new(PitchActionOwner {
                admission,
                sequence,
                source_generation,
                source_request_incarnation,
                timing_intent,
                launch_cancel_revision,
                preparation_epoch: Arc::new(AtomicU64::new(1)),
                published: AtomicBool::new(false),
                admitted: AtomicBool::new(false),
                status: Arc::new(AtomicU8::new(CAPTURED)),
                reason: AtomicU8::new(0),
                execution_frame: AtomicU64::new(due_frame),
                effective_frame: AtomicU64::new(0),
            }),
        };
        action.control_guard(None)?;
        Ok(action)
    }

    pub(crate) fn belongs_to(&self, ownership: &Arc<InputRuntimeOwnership>) -> bool {
        Arc::ptr_eq(&self.owner.admission.ownership, ownership)
    }
    pub(crate) fn sequence(&self) -> u64 {
        self.owner.sequence
    }
    pub(crate) fn content_key(&self) -> ContentKey {
        self.owner.admission.key
    }
    pub(crate) fn render(&self) -> FrozenPitchRender {
        self.owner.admission.render
    }
    pub(crate) fn variant(&self) -> PitchActionVariant {
        self.owner.admission.spec.variant
    }
    pub(crate) fn original_due_frame(&self) -> u64 {
        self.owner.admission.due_frame
    }
    pub(crate) fn execution_target(&self) -> u64 {
        self.owner.execution_frame.load(Ordering::Acquire)
    }
    pub(crate) fn received_timestamp(&self) -> u64 {
        self.owner.admission.received_at_ns
    }
    pub(crate) fn previous_requested(&self) -> (PitchIntent, u64) {
        (
            self.owner.admission.previous_requested,
            self.owner.admission.previous_sequence,
        )
    }
    pub(crate) fn requested_shadow(&self) -> PitchActionShadow {
        PitchActionShadow {
            binding: self.owner.admission.binding,
            source_generation: self.owner.source_generation,
            source_request_incarnation: self.owner.source_request_incarnation,
            timing_intent: self.owner.timing_intent,
            status: self.owner.status.clone(),
        }
    }
    pub(crate) fn loop_region(&self) -> (f64, Option<f64>) {
        (
            self.owner.admission.loop_start_s,
            self.owner.admission.loop_end_s,
        )
    }
    pub(crate) fn exclusive(&self) -> bool {
        self.owner.admission.exclusive
    }
    pub(crate) fn sync(&self) -> bool {
        self.owner.admission.sync
    }
    pub(crate) fn preparation_epoch(&self) -> &Arc<AtomicU64> {
        &self.owner.preparation_epoch
    }
    pub(crate) fn epoch(&self) -> u64 {
        self.owner.preparation_epoch.load(Ordering::Acquire)
    }

    fn input_current(&self) -> bool {
        // Once the action is published, a later Learn/mapping edit cannot undo it.
        self.owner.published.load(Ordering::Acquire)
            || self
                .owner
                .admission
                .input_owners
                .as_ref()
                .is_none_or(|(mapping, enabled, learn)| {
                    mapping.load(Ordering::Acquire)
                        && enabled.load(Ordering::Acquire)
                        && !learn.load(Ordering::Acquire)
                })
    }

    fn guard_at(
        &self,
        id: usize,
        mut binding: InputPadBinding,
    ) -> Result<PitchActionGuard, PitchActionReject> {
        let owner = &self.owner;
        let original = owner.admission.binding;
        let ownership = &owner.admission.ownership;
        if owner.status.load(Ordering::Acquire) == REJECTED {
            return Err(PitchActionReject::StaleContent);
        }
        if !self.input_current() {
            return Err(PitchActionReject::InputDisabled);
        }
        // Admit preserves this revision, so equal accepted attacks remain independent.
        // A later explicit Stop/Cancel revokes the original attack through its stable cell.
        if self.variant() == PitchActionVariant::SetRetrigger
            && ownership
                .launch_revision_for_cell(owner.admission.key.cell())
                .load(Ordering::Acquire)
                != owner.launch_cancel_revision
        {
            return Err(PitchActionReject::LaunchCancelled);
        }
        if !ownership.content_key_current(owner.admission.key)
            || binding.placement.and_then(|p| p.content_key()) != Some(owner.admission.key)
        {
            return Err(PitchActionReject::StaleContent);
        }
        if binding.source_address != original.source_address
            || binding.sample_count != original.sample_count
            || binding.channels != original.channels
            || binding.sample_rate_hz != original.sample_rate_hz
            || binding.authority_revision != original.authority_revision
            || !ownership.authority_current(id, binding)
            || ownership.binding_source_generation(id, binding) != Some(owner.source_generation)
            || ownership.key_source_request_incarnation(owner.admission.key)
                != Some(owner.source_request_incarnation)
        {
            return Err(PitchActionReject::SourceChanged);
        }
        if binding.accepted != original.accepted
            || ownership.binding_timing_intent(id, binding) != Some(owner.timing_intent)
            || owner
                .admission
                .acknowledgements
                .current_for_binding(ownership, id, binding)
                != original
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
            || !ownership.source_timing_available_for_binding(
                id,
                binding,
                &owner.admission.acknowledgements,
            )
        {
            return Err(PitchActionReject::TimingChanged);
        }
        if !ownership.binding_window_current(id, binding) {
            return Err(PitchActionReject::WindowPending);
        }
        // Pitch requests have separate epochs. Only the actual current source/window scalar
        // is projected into a new history permit; the captured pitch/attack never changes.
        binding.runtime_revision = ownership
            .runtime_for_cell(owner.admission.key.cell())
            .load(Ordering::Acquire);
        Ok(PitchActionGuard { id, binding })
    }

    pub(crate) fn control_guard(
        &self,
        own_window: Option<InputPadBinding>,
    ) -> Result<PitchActionGuard, PitchActionReject> {
        let placement = match self
            .owner
            .admission
            .ownership
            .observe_control_key(self.content_key())
        {
            PlacementObservation::Current(placement) => placement,
            PlacementObservation::Pending => return Err(PitchActionReject::PlacementPending),
            PlacementObservation::Stale | PlacementObservation::Retired => {
                return Err(PitchActionReject::StaleContent);
            }
        };
        let mut binding = own_window.unwrap_or(self.owner.admission.binding);
        binding.placement = Some(placement);
        self.guard_at(placement.slot(), binding)
    }

    pub(crate) fn callback_guard(
        &self,
        own_window: Option<InputPadBinding>,
    ) -> Result<PitchActionGuard, PitchActionReject> {
        let ownership = &self.owner.admission.ownership;
        let id = ownership
            .callback_key_slot(self.content_key())
            .ok_or_else(|| {
                if ownership.content_key_current(self.content_key()) {
                    PitchActionReject::PlacementPending
                } else {
                    PitchActionReject::StaleContent
                }
            })?;
        let placement = ownership
            .slot_snapshot(id)
            .ok_or(PitchActionReject::PlacementPending)?;
        let mut binding = own_window.unwrap_or(self.owner.admission.binding);
        binding.placement = Some(placement);
        self.guard_at(id, binding)
    }

    /// Fixed callback capture from this action's actual Current bank, shared ACK owner
    /// and independent epoch. Retained older voices cannot authorize a new attack.
    pub(crate) fn capture_history_permit(
        &self,
        sample: &SampleBuffer,
        sample_rate_hz: u32,
        own_window: Option<InputPadBinding>,
    ) -> Option<NativeHistoryPermit> {
        let guard = self.callback_guard(own_window).ok()?;
        NativeHistoryContext {
            id: guard.id,
            ownership: &self.owner.admission.ownership,
            acknowledgements: &self.owner.admission.acknowledgements,
            preparation_epoch: self.preparation_epoch(),
        }
        .capture_admitted(sample, sample_rate_hz, guard.binding)?
        .for_action(
            &self.owner.admission.ownership,
            &self.owner.admission.acknowledgements,
            self.content_key(),
            self.preparation_epoch(),
        )
    }

    fn matches_history_owner(&self, request: &NativeHistoryRequest) -> bool {
        let original = self.owner.admission.binding;
        request.permit.matches_action_owner(
            &self.owner.admission.ownership,
            &self.owner.admission.acknowledgements,
            self.content_key(),
            self.preparation_epoch(),
        ) && request.binding.source_address == original.source_address
            && request.binding.sample_count == original.sample_count
            && request.binding.channels == original.channels
            && request.binding.sample_rate_hz == original.sample_rate_hz
            && request.binding.accepted == original.accepted
    }

    /// Called only after existing producer/scheduler capacity and complete admission preflight.
    pub(crate) fn mark_published(&self) -> Result<(), PitchActionReject> {
        self.control_guard(None)?;
        self.owner
            .published
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| PitchActionReject::AlreadyPublished)
    }
    pub(crate) fn mark_scheduled(&self) -> bool {
        let status = self.owner.status.load(Ordering::Acquire);
        let admitted = matches!(status, CAPTURED | PREPARING | PREPARED)
            && self
                .owner
                .status
                .compare_exchange(status, SCHEDULED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();
        if admitted {
            self.owner.admitted.store(true, Ordering::Release);
        }
        admitted
    }
    pub(crate) fn is_published(&self) -> bool {
        self.owner.published.load(Ordering::Acquire)
    }
    pub(crate) fn reject(&self, reason: PitchActionReject) -> bool {
        let status = self.owner.status.load(Ordering::Acquire);
        if matches!(status, APPLYING | APPLIED | REJECTED)
            || self
                .owner
                .status
                .compare_exchange(status, REJECTED, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return false;
        }
        self.owner.reason.store(reason as u8, Ordering::Release);
        self.owner.preparation_epoch.store(0, Ordering::Release);
        true
    }
    /// Last operation of the complete existing Native mutation claim. A concurrent
    /// cancellation either wins before this CAS or preserves the applying effect.
    pub(crate) fn claim_effect(&self) -> bool {
        let status = self.owner.status.load(Ordering::Acquire);
        matches!(status, PREPARED | SCHEDULED)
            && self
                .owner
                .status
                .compare_exchange(status, APPLYING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }
    /// The caller has applied the complete tuple and optional attack atomically.
    pub(crate) fn mark_effective(&self, output_frame: u64) -> bool {
        if self.owner.status.load(Ordering::Acquire) != APPLYING {
            return false;
        }
        self.owner
            .effective_frame
            .store(output_frame, Ordering::Release);
        self.owner.reason.store(0, Ordering::Release);
        self.owner
            .status
            .compare_exchange(APPLYING, APPLIED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

#[pymethods]
impl CapturedPitchAction {
    #[getter]
    fn accepted_sequence(&self) -> u64 {
        self.sequence()
    }
    #[getter]
    fn received_at_ns(&self) -> u64 {
        self.received_timestamp()
    }
    #[getter]
    fn due_frame(&self) -> u64 {
        self.original_due_frame()
    }
    #[getter]
    fn execution_frame(&self) -> u64 {
        self.execution_target()
    }
    #[getter]
    fn content_lifetime(&self) -> u64 {
        self.content_key().lifetime()
    }
    #[getter]
    fn base_shift(&self) -> i32 {
        self.render().pitch.base_shift()
    }
    #[getter]
    fn extra_shift(&self) -> i32 {
        self.render().pitch.extra_shift()
    }
    #[getter]
    fn key_lock(&self) -> bool {
        self.render().key_lock
    }
    #[getter]
    fn retrigger(&self) -> bool {
        self.variant() == PitchActionVariant::SetRetrigger
    }
    #[getter]
    fn quantize_step_64ths(&self) -> Option<u32> {
        self.owner.admission.quantize_step_64ths
    }
    #[getter]
    fn sync_enabled(&self) -> bool {
        self.sync()
    }
    #[getter]
    pub(crate) fn accepted(&self) -> bool {
        self.owner.admitted.load(Ordering::Acquire)
    }
    #[getter]
    pub(crate) fn effective(&self) -> bool {
        self.owner.status.load(Ordering::Acquire) == APPLIED
    }
    #[getter]
    fn effective_output_frame(&self) -> Option<u64> {
        self.effective()
            .then(|| self.owner.effective_frame.load(Ordering::Acquire))
    }
    #[getter]
    pub(crate) fn state(&self) -> &'static str {
        match self.owner.status.load(Ordering::Acquire) {
            CAPTURED => "captured",
            PREPARING => "preparing",
            PREPARED => "prepared",
            SCHEDULED => "scheduled",
            APPLYING => "applying",
            APPLIED => "applied",
            _ => "rejected",
        }
    }
    #[getter]
    pub(crate) fn reason(&self) -> Option<&'static str> {
        PitchActionReject::from_code(self.owner.reason.load(Ordering::Acquire))
            .map(PitchActionReject::as_str)
    }
    #[getter]
    pub(crate) fn current_control_slot(&self) -> Option<usize> {
        match self
            .owner
            .admission
            .ownership
            .observe_control_key(self.content_key())
        {
            PlacementObservation::Current(placement) => Some(placement.slot()),
            _ => None,
        }
    }
}

/// Off-control allocated scheduler/worker value. It must retire intact on the existing
/// retirement worker after either adoption, cancellation or terminal rejection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum PitchActionPreparationMode {
    #[default]
    Endpoint,
    ForwardHistory,
}

pub(crate) struct PitchActionWork {
    pub(crate) action: CapturedPitchAction,
    pub(crate) candidate: Box<NativeAdapterState>,
    request: Option<NativeHistoryRequest>,
    captured: bool,
    execution_frozen: bool,
    own_window: Option<InputPadBinding>,
    preparation_mode: PitchActionPreparationMode,
    preparation_required: bool,
    pub(crate) source_feed: SourceFeedSnapshot,
    pub(crate) source_feed_future: Option<(SourcePlayback, SourceReadPlan, u64, SourceFeedContext)>,
    pub(crate) source_feed_voice: Option<(Arc<AtomicU64>, u64)>,
}

impl PitchActionWork {
    pub(crate) fn new(
        action: CapturedPitchAction,
        sample_rate_hz: u32,
        channels: usize,
    ) -> Result<Box<Self>, RubberBandError> {
        let native = RubberBandLiveShifter::new(sample_rate_hz, channels)?;
        Ok(Box::new(Self {
            action,
            candidate: Box::new(NativeAdapterState::new(Some(native), channels)),
            request: None,
            captured: false,
            execution_frozen: false,
            own_window: None,
            preparation_mode: PitchActionPreparationMode::Endpoint,
            preparation_required: true,
            source_feed: SourceFeedSnapshot::new(),
            source_feed_future: None,
            source_feed_voice: None,
        }))
    }

    pub(crate) fn current_guard(&self) -> Result<PitchActionGuard, PitchActionReject> {
        self.action.callback_guard(self.own_window)
    }

    /// Existing callback mutation claims already hold the placement marker. This
    /// checks their fixed participants without recapturing an odd/pending snapshot.
    pub(crate) fn claim_current(&self, guard: PitchActionGuard) -> bool {
        let ownership = &self.action.owner.admission.ownership;
        guard.binding.placement.is_some_and(|placement| {
            placement.slot() == guard.id && ownership.slot_snapshot_fields_current(placement)
        }) && ownership.callback_key_slot(self.action.content_key()) == Some(guard.id)
            && self.action.guard_at(guard.id, guard.binding).is_ok()
    }

    pub(crate) fn claim_effect(&self) -> bool {
        self.action.claim_effect()
    }

    /// Caller selects the history interpretation before capture using actual mixer
    /// source coordinates. ForwardHistory starts at E minus the existing warm history;
    /// Endpoint owns the desired E/start coordinate and requires its proved history.
    pub(crate) fn set_preparation_mode(
        &mut self,
        mode: PitchActionPreparationMode,
    ) -> Result<(), PitchActionReject> {
        if !self.capture_available() {
            return Err(PitchActionReject::PreparedMismatch);
        }
        self.preparation_mode = mode;
        Ok(())
    }

    pub(crate) fn preparation_mode(&self) -> PitchActionPreparationMode {
        self.preparation_mode
    }

    pub(crate) fn capture_history_permit(
        &self,
        sample: &SampleBuffer,
        sample_rate_hz: u32,
    ) -> Option<NativeHistoryPermit> {
        self.action
            .capture_history_permit(sample, sample_rate_hz, self.own_window)
    }

    pub(crate) fn preparation_required(&self) -> bool {
        self.preparation_required
    }

    /// Root has proved stopped SET or actual exact unity needs no native adoption.
    /// This is bounded scalar state only; it never changes tuning, position or playback.
    pub(crate) fn complete_without_preparation(&mut self) -> Result<(), PitchActionReject> {
        if !self.capture_available() {
            return Err(PitchActionReject::PreparedMismatch);
        }
        self.action.callback_guard(self.own_window)?;
        self.action
            .owner
            .status
            .compare_exchange(CAPTURED, PREPARED, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PitchActionReject::StaleContent)?;
        self.preparation_required = false;
        self.captured = true;
        self.execution_frozen = true;
        Ok(())
    }

    /// Explicit deferred execution coordinate E; original input/grid target T stays immutable.
    /// E is selected once by existing scheduler policy, never by a second grid calculation.
    pub(crate) fn freeze_execution_frame(&mut self, frame: u64) -> Result<(), PitchActionReject> {
        if self.captured || self.execution_frozen || frame < self.action.original_due_frame() {
            return Err(PitchActionReject::PreparedMismatch);
        }
        self.action
            .owner
            .execution_frame
            .store(frame, Ordering::Release);
        self.execution_frozen = true;
        Ok(())
    }

    /// Bounded callback capture; caller constructed the fixed request from actual mixer
    /// coordinates and an own-epoch Current NativeHistoryPermit. No allocation or owner drop.
    pub(crate) fn rt_capture(
        &mut self,
        request: NativeHistoryRequest,
    ) -> Result<(), NativeHistoryRequest> {
        let render = self.action.render();
        if self.captured
            || self.request.is_some()
            || self.candidate.source.is_some()
            || request.pitch_intent != render.pitch
            || request.key_lock != render.key_lock
            || request.request_id != self.action.sequence()
            || request.epoch != self.action.epoch()
            || request.target_output_frame != self.action.execution_target()
            || !self.action.matches_history_owner(&request)
            || self.action.callback_guard(self.own_window).is_err()
        {
            return Err(request);
        }
        if self
            .action
            .owner
            .status
            .compare_exchange(CAPTURED, PREPARING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(request);
        }
        self.captured = true;
        self.execution_frozen = true;
        self.request = Some(request);
        Ok(())
    }

    /// Capture rollback retains rejected request pins inside the already allocated owner.
    /// Caller must reserve this one slot before constructing a request.
    pub(crate) fn retain_rejected_capture(
        &mut self,
        request: NativeHistoryRequest,
    ) -> Result<(), NativeHistoryRequest> {
        if self.request.is_some() {
            return Err(request);
        }
        self.request = Some(request);
        Ok(())
    }
    pub(crate) fn capture_available(&self) -> bool {
        !self.captured && self.request.is_none() && self.candidate.source.is_none()
    }

    fn source_feed_current(&self) -> bool {
        self.source_feed_future.is_none()
            || (self.source_feed.is_current()
                && self
                    .source_feed_voice
                    .as_ref()
                    .is_some_and(|(owner, epoch)| {
                        *epoch != u64::MAX && owner.load(Ordering::Acquire) == *epoch
                    }))
    }

    /// Worker-only native reset/warm/read/allocate; each action retains its own tuple and epoch.
    pub(crate) fn prepare(&mut self) -> Result<(), PitchActionReject> {
        self.action.control_guard(self.own_window)?;
        if !self.preparation_required {
            return Ok(());
        }
        if !self.source_feed_current() {
            return Err(PitchActionReject::PreparationFailed);
        }
        let request = self
            .request
            .take()
            .ok_or(PitchActionReject::PreparedMismatch)?;
        if request.epoch != self.action.epoch()
            || !self.action.matches_history_owner(&request)
            || !request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
            || !request.permit.matches_projection(request.binding.accepted)
        {
            return Err(PitchActionReject::SourceChanged);
        }
        match self.preparation_mode {
            PitchActionPreparationMode::Endpoint => {
                if let Some(future) = self.source_feed_future {
                    self.candidate.prepare_source_with_feed_history(
                        request,
                        &self.source_feed,
                        future,
                    )
                } else {
                    self.candidate.prepare_source_at_endpoint(request)
                }
            }
            PitchActionPreparationMode::ForwardHistory => self.candidate.prepare_source(request),
        }
        .map_err(|_| PitchActionReject::PreparationFailed)?;
        if !self.source_feed_current() {
            return Err(PitchActionReject::PreparationFailed);
        }
        self.action.control_guard(self.own_window)?;
        let status = self.action.owner.status.load(Ordering::Acquire);
        if !matches!(status, PREPARING | SCHEDULED)
            || self
                .action
                .owner
                .status
                .compare_exchange(status, PREPARED, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(PitchActionReject::StaleContent);
        }
        Ok(())
    }

    /// Preliminary fixed check. Root additionally proves actual endpoint/feed, retirement,
    /// feedback and launch capacity before calling the mixer's allocation-free adoption.
    pub(crate) fn rt_ready(
        &self,
        output_frame: u64,
    ) -> Result<PitchActionGuard, PitchActionReject> {
        let guard = self.action.callback_guard(self.own_window)?;
        if output_frame != self.action.execution_target() {
            return Err(PitchActionReject::PreparedMismatch);
        }
        if !self.preparation_required {
            return Ok(guard);
        }
        if !self.source_feed_current() {
            return Err(PitchActionReject::PreparationFailed);
        }
        let render = self.action.render();
        let source = self
            .candidate
            .source
            .as_ref()
            .ok_or(PitchActionReject::WindowPending)?;
        if output_frame != self.action.execution_target()
            || !self.candidate.active
            || !self.candidate.used
            || self.candidate.dirty
            || source.pitch_intent != render.pitch
            || source.key_lock != render.key_lock
            || source.request_id != self.action.sequence()
            || source.epoch != self.action.epoch()
            || source.target_output_frame != output_frame
            || !self.action.matches_history_owner(source)
            || !source
                .permit
                .current(&source.sample, source.binding.sample_rate_hz)
            || !source.permit.matches_projection(source.binding.accepted)
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        Ok(guard)
    }
}

#[cfg(test)]
mod tests;
