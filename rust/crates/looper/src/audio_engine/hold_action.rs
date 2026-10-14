//! One accepted physical waveform hold and its native caused-pause acknowledgement.
//!
//! Preparation, identity reservation and allocation belong to control. The callback only
//! publishes fixed atomic fields and transfers prebuilt owners to the retirement worker.
//! A slot number, a source address or a UI paused projection never authorizes release.
pub(crate) mod control;

use super::constants::MAX_VOICES;
use super::content_placement::{ContentKey, SlotSnapshot};
use super::input_runtime_binding::InputRuntimeOwnership;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

const _: () = assert!(MAX_VOICES <= u32::BITS as usize);
static NEXT_ACTION_ID: AtomicU64 = AtomicU64::new(1);

fn reserve_action_id(counter: &AtomicU64) -> Result<u64, &'static str> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_add(1)
        })
        .map_err(|_| "hold action identity exhausted")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum HoldActionState {
    Prepared = 0,
    Admitted = 1,
    PausedOwned = 2,
    NoEffect = 3,
    Resumed = 4,
    Retired = 5,
    Superseded = 6,
    Exhausted = 7,
    Rejected = 8,
}

impl HoldActionState {
    fn from_raw(value: u8) -> Self {
        match value {
            0 => Self::Prepared,
            1 => Self::Admitted,
            2 => Self::PausedOwned,
            3 => Self::NoEffect,
            4 => Self::Resumed,
            5 => Self::Retired,
            6 => Self::Superseded,
            7 => Self::Exhausted,
            _ => Self::Rejected,
        }
    }

    pub(crate) fn terminal(self) -> bool {
        !matches!(self, Self::Prepared | Self::Admitted | Self::PausedOwned)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Admitted => "pause_pending",
            Self::PausedOwned => "paused_owned",
            Self::NoEffect => "no_effect",
            Self::Resumed => "resumed",
            Self::Retired => "retired",
            Self::Superseded => "superseded",
            Self::Exhausted => "exhausted",
            Self::Rejected => "rejected",
        }
    }
}

/// Immutable after the callback's first effective pause. Lane indices remain fixed when
/// pad images move; generations fence stopped, restarted, sought or reused lanes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativePauseEffect {
    pub(crate) key: ContentKey,
    pub(crate) action_id: u64,
    pub(crate) effect_id: u64,
    pub(crate) transport_revision: u64,
    pub(crate) cohort_mask: u32,
    pub(crate) changed_mask: u32,
    pub(crate) generations: [u64; MAX_VOICES],
    pub(crate) paused_at_output_frame: u64,
}

/// Fixed evidence for a complete Pair acknowledgement, never a transferable authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeHoldProjection {
    pub(crate) key: ContentKey,
    pub(crate) action_id: u64,
    pub(crate) transport_revision: u64,
    pub(crate) physical_press_id: u64,
    pub(crate) state: HoldActionState,
    pub(crate) release_requested: bool,
    pub(crate) release_admitted: bool,
    pub(crate) effect: Option<NativePauseEffect>,
    pub(crate) effect_current: bool,
    pub(crate) settled_at_output_frame: u64,
}

/// The actual owning ticket is retained through queue pressure, delayed ACK and retirement.
pub(crate) struct NativeHoldAction {
    ownership: Arc<InputRuntimeOwnership>,
    captured: SlotSnapshot,
    key: ContentKey,
    action_id: u64,
    transport_revision: u64,
    physical_press_id: u64,
    received_at_ns: u64,
    state: AtomicU8,
    release_requested: AtomicBool,
    release_admitted: AtomicBool,
    effect_published: AtomicBool,
    cohort_mask: AtomicU32,
    changed_mask: AtomicU32,
    generations: [AtomicU64; MAX_VOICES],
    paused_at_output_frame: AtomicU64,
    settled_at_output_frame: AtomicU64,
}

impl std::fmt::Debug for NativeHoldAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeHoldAction")
            .field("key", &self.key)
            .field("action_id", &self.action_id)
            .field("transport_revision", &self.transport_revision)
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl NativeHoldAction {
    /// All fields and owners precede admission. The caller commits the captured revision and
    /// calls `mark_admitted` inside its single same-content producer claim before queueing.
    pub(crate) fn prepare(
        ownership: Arc<InputRuntimeOwnership>,
        captured: SlotSnapshot,
        accepted_transport_revision: u64,
        physical_press_id: u64,
        received_at_ns: u64,
    ) -> Result<Arc<Self>, &'static str> {
        let key = captured.content_key().ok_or("hold content is empty")?;
        if physical_press_id == 0 || accepted_transport_revision == 0 {
            return Err("hold requires nonzero physical and transport identities");
        }
        if !ownership.slot_snapshot_current(captured) {
            return Err("hold content placement changed or is pending");
        }
        Ok(Arc::new(Self {
            ownership,
            captured,
            key,
            action_id: reserve_action_id(&NEXT_ACTION_ID)?,
            transport_revision: accepted_transport_revision,
            physical_press_id,
            received_at_ns,
            state: AtomicU8::new(HoldActionState::Prepared as u8),
            release_requested: AtomicBool::new(false),
            release_admitted: AtomicBool::new(false),
            effect_published: AtomicBool::new(false),
            cohort_mask: AtomicU32::new(0),
            changed_mask: AtomicU32::new(0),
            generations: std::array::from_fn(|_| AtomicU64::new(0)),
            paused_at_output_frame: AtomicU64::new(0),
            settled_at_output_frame: AtomicU64::new(0),
        }))
    }

    pub(crate) fn ownership(&self) -> &Arc<InputRuntimeOwnership> {
        &self.ownership
    }

    pub(crate) fn key(&self) -> ContentKey {
        self.key
    }

    pub(crate) fn captured(&self) -> SlotSnapshot {
        self.captured
    }

    pub(crate) fn action_id(&self) -> u64 {
        self.action_id
    }

    pub(crate) fn transport_revision(&self) -> u64 {
        self.transport_revision
    }

    pub(crate) fn state(&self) -> HoldActionState {
        HoldActionState::from_raw(self.state.load(Ordering::Acquire))
    }

    pub(crate) fn mark_admitted(&self) -> bool {
        self.state
            .compare_exchange(
                HoldActionState::Prepared as u8,
                HoldActionState::Admitted as u8,
                Ordering::Release,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    /// A release is sticky even before pause ACK or admission capacity. Duplicate observations
    /// only return the same outstanding obligation; they never allocate another identity.
    pub(crate) fn request_release(&self) -> bool {
        if self.state().terminal() {
            return false;
        }
        self.release_requested.store(true, Ordering::Release);
        true
    }

    pub(crate) fn release_requested(&self) -> bool {
        self.release_requested.load(Ordering::Acquire)
    }

    pub(crate) fn release_admitted(&self) -> bool {
        self.release_admitted.load(Ordering::Acquire)
    }

    /// Call only after all enqueue capacity and guards are reserved. A failed attempt must
    /// leave this bit clear so polling retries the original complete ticket.
    pub(crate) fn mark_release_admitted(&self) -> bool {
        self.release_requested()
            && !self.state().terminal()
            && self
                .release_admitted
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    pub(crate) fn intent_current(&self) -> bool {
        self.ownership.content_key_current(self.key)
            && self
                .ownership
                .transport_intent_for_cell(self.key.cell())
                .load(Ordering::Acquire)
                == self.transport_revision
    }

    /// Callback-only, inside the same-content mutation claim. The effect image is written
    /// once; publishing its flag last makes even delayed terminal observation coherent.
    pub(crate) fn publish_effect(&self, effect: NativePauseEffect) {
        debug_assert_eq!(self.state(), HoldActionState::Admitted);
        debug_assert_eq!(effect.key, self.key);
        debug_assert_eq!(effect.action_id, self.action_id);
        debug_assert_eq!(effect.transport_revision, self.transport_revision);
        debug_assert_ne!(effect.changed_mask, 0);
        for (target, generation) in self.generations.iter().zip(effect.generations) {
            target.store(generation, Ordering::Relaxed);
        }
        self.cohort_mask
            .store(effect.cohort_mask, Ordering::Relaxed);
        self.changed_mask
            .store(effect.changed_mask, Ordering::Relaxed);
        self.paused_at_output_frame
            .store(effect.paused_at_output_frame, Ordering::Relaxed);
        self.effect_published.store(true, Ordering::Release);
        self.state
            .store(HoldActionState::PausedOwned as u8, Ordering::Release);
    }

    pub(crate) fn effect(&self) -> Option<NativePauseEffect> {
        self.effect_published
            .load(Ordering::Acquire)
            .then(|| NativePauseEffect {
                key: self.key,
                action_id: self.action_id,
                effect_id: self.action_id,
                transport_revision: self.transport_revision,
                cohort_mask: self.cohort_mask.load(Ordering::Relaxed),
                changed_mask: self.changed_mask.load(Ordering::Relaxed),
                generations: std::array::from_fn(|index| {
                    self.generations[index].load(Ordering::Relaxed)
                }),
                paused_at_output_frame: self.paused_at_output_frame.load(Ordering::Relaxed),
            })
    }

    /// Callback-only terminal publication; a consumed/retired claim can never be reopened.
    pub(crate) fn settle(&self, state: HoldActionState, output_frame: u64) {
        debug_assert!(state.terminal());
        if self.state().terminal() {
            return;
        }
        self.settled_at_output_frame
            .store(output_frame, Ordering::Relaxed);
        self.state.store(state as u8, Ordering::Release);
    }

    pub(crate) fn projection(&self) -> NativeHoldProjection {
        let mut state = self.state();
        let mut effect = self.effect();
        let mut settled_at_output_frame = self.settled_at_output_frame.load(Ordering::Relaxed);
        let observed_after = self.state();
        if observed_after != state {
            // Effect and final frame are immutable after their publication. One bounded
            // reread after the acquired new phase prevents a half-published ACK projection.
            state = observed_after;
            effect = self.effect();
            settled_at_output_frame = self.settled_at_output_frame.load(Ordering::Relaxed);
        }
        NativeHoldProjection {
            key: self.key,
            action_id: self.action_id,
            transport_revision: self.transport_revision,
            physical_press_id: self.physical_press_id,
            state,
            release_requested: self.release_requested(),
            release_admitted: self.release_admitted(),
            effect,
            effect_current: state == HoldActionState::PausedOwned && self.intent_current(),
            settled_at_output_frame,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HoldWorkKind {
    Pause,
    Release,
}

/// Prebuilt control owner. The original Pause work is retained in its stable native cell
/// while effective; release and every terminal path transfer boxes through retirement.
#[derive(Debug, Clone)]
pub(crate) struct HoldActionRtWork {
    pub(crate) action: Arc<NativeHoldAction>,
    pub(crate) kind: HoldWorkKind,
}

impl HoldActionRtWork {
    pub(crate) fn pause(action: Arc<NativeHoldAction>) -> Box<Self> {
        Box::new(Self {
            action,
            kind: HoldWorkKind::Pause,
        })
    }

    pub(crate) fn release(action: Arc<NativeHoldAction>) -> Box<Self> {
        Box::new(Self {
            action,
            kind: HoldWorkKind::Release,
        })
    }
}

#[pyclass(frozen)]
pub struct HoldActionTicket {
    pub(crate) action: Arc<NativeHoldAction>,
}

#[pymethods]
impl HoldActionTicket {
    /// Display projection through the original key; never a release permission.
    pub fn current_control_slot(&self) -> Option<usize> {
        if self.action.state().terminal() {
            return None;
        }
        match self
            .action
            .ownership()
            .observe_control_key(self.action.key())
        {
            super::content_placement::PlacementObservation::Current(snapshot) => {
                Some(snapshot.slot())
            }
            _ => None,
        }
    }

    pub fn publication_status(&self) -> &'static str {
        let state = self.action.state();
        if state == HoldActionState::PausedOwned && self.action.release_requested() {
            "release_pending"
        } else {
            state.label()
        }
    }

    pub fn is_current(&self) -> bool {
        !self.action.state().terminal() && self.action.intent_current()
    }

    pub fn release_requested(&self) -> bool {
        self.action.release_requested()
    }

    pub fn release_admitted(&self) -> bool {
        self.action.release_admitted()
    }

    /// Requesting is deliberately separate from guarded native queue admission.
    pub fn request_release(&self) -> bool {
        self.action.request_release()
    }

    pub fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let projection = self.action.projection();
        let metadata = PyDict::new(py);
        metadata.set_item("action_id", projection.action_id)?;
        metadata.set_item("content_cell", projection.key.cell().index())?;
        metadata.set_item("content_lifetime", projection.key.lifetime())?;
        metadata.set_item("transport_revision", projection.transport_revision)?;
        metadata.set_item("physical_press_id", projection.physical_press_id)?;
        metadata.set_item("received_at_ns", self.action.received_at_ns)?;
        metadata.set_item("diagnostic_slot", self.action.captured.slot())?;
        metadata.set_item(
            "diagnostic_binding_epoch",
            self.action.captured.binding_epoch(),
        )?;
        let status =
            if projection.state == HoldActionState::PausedOwned && projection.release_requested {
                "release_pending"
            } else {
                projection.state.label()
            };
        metadata.set_item("status", status)?;
        metadata.set_item("release_requested", projection.release_requested)?;
        metadata.set_item("release_admitted", projection.release_admitted)?;
        metadata.set_item("effect_current", projection.effect_current)?;
        metadata.set_item(
            "settled_at_output_frame",
            projection.settled_at_output_frame,
        )?;
        if let Some(effect) = projection.effect {
            metadata.set_item("effect_id", effect.effect_id)?;
            metadata.set_item("cohort_mask", effect.cohort_mask)?;
            metadata.set_item("changed_mask", effect.changed_mask)?;
            metadata.set_item("voice_generations", effect.generations.to_vec())?;
            metadata.set_item("paused_at_output_frame", effect.paused_at_output_frame)?;
        }
        Ok(metadata)
    }
}

#[cfg(test)]
#[path = "hold_action_tests.rs"]
mod tests;
