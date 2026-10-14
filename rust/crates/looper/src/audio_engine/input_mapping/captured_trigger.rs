//! An admitted MIDI input keeps its actual mapping and original content through fallback.

use super::{InputAction, InputMapping, InputRuntime, RuntimeState};
use crate::audio_engine::AudioEngine;
use crate::audio_engine::content_placement::{ContentKey, PlacementObservation};
use crate::audio_engine::input_runtime_binding::{
    InputPadBinding, InputRuntimeOwnership, LaunchWriteKind,
};
use crate::audio_engine::resident_relocation::{self, ResidentWindowTicket};
use crate::messages::ControlMessage;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct TriggerOwner {
    key: ContentKey,
    binding: InputPadBinding,
    generation: u64,
    request_incarnation: u64,
    timing_intent: u8,
    launch_revision: u64,
    transport_intent: u64,
    mapping: InputMapping,
    enabled: Arc<AtomicBool>,
    learn_capture_active: Arc<AtomicBool>,
    runtime: Arc<Mutex<RuntimeState>>,
    ownership: Arc<InputRuntimeOwnership>,
    loop_start_s: f64,
    loop_end_s: Option<f64>,
    exclusive: bool,
    received_at_ns: u64,
    admitted: AtomicBool,
    window_prepared: AtomicBool,
}

/// No Python slot, mapping text, fresh binding or timestamp can reconstruct this owner.
#[pyclass(frozen)]
#[derive(Clone)]
pub struct CapturedInputPadTrigger {
    owner: Arc<TriggerOwner>,
}

impl std::fmt::Debug for CapturedInputPadTrigger {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedInputPadTrigger")
            .field("key", &self.owner.key)
            .finish_non_exhaustive()
    }
}

pub(in crate::audio_engine) struct CapturedTriggerLaunchGuard {
    trigger: CapturedInputPadTrigger,
    pub(in crate::audio_engine) id: usize,
    pub(in crate::audio_engine) binding: InputPadBinding,
}

impl CapturedTriggerLaunchGuard {
    pub(in crate::audio_engine) fn current(&self) -> bool {
        self.trigger.scalar_current(self.id, self.binding)
    }
    pub(in crate::audio_engine) fn mark_admitted(&self) {
        self.trigger.owner.admitted.store(true, Ordering::Release);
    }
}

impl CapturedInputPadTrigger {
    pub(super) fn capture(
        mapping: &InputMapping,
        received_at_ns: u64,
        runtime: &Arc<Mutex<RuntimeState>>,
        ownership: &Arc<InputRuntimeOwnership>,
        enabled: &Arc<AtomicBool>,
        learn_capture_active: &Arc<AtomicBool>,
    ) -> Option<Self> {
        let InputAction::TriggerPad { id } = &mapping.action else {
            return None;
        };
        let id = *id;
        let state = runtime.lock().ok()?;
        let pad = state.pads.get(id)?;
        let binding = pad.binding?;
        if !pad.loaded || !ownership.admission_current(id, binding) {
            return None;
        }
        let key = binding.placement?.content_key()?;
        let token = Self {
            owner: Arc::new(TriggerOwner {
                key,
                binding,
                generation: ownership.binding_source_generation(id, binding)?,
                request_incarnation: ownership.binding_source_request_incarnation(id, binding)?,
                timing_intent: ownership.binding_timing_intent(id, binding)?,
                launch_revision: ownership
                    .launch_revision_for_cell(key.cell())
                    .load(Ordering::Acquire),
                transport_intent: ownership
                    .transport_intent_for_cell(key.cell())
                    .load(Ordering::Acquire),
                mapping: mapping.clone(),
                enabled: enabled.clone(),
                learn_capture_active: learn_capture_active.clone(),
                runtime: runtime.clone(),
                ownership: ownership.clone(),
                loop_start_s: pad.loop_start_s,
                loop_end_s: pad.loop_end_s,
                exclusive: !state.multi_loop,
                received_at_ns,
                admitted: AtomicBool::new(false),
                window_prepared: AtomicBool::new(false),
            }),
        };
        token.scalar_current(id, binding).then_some(token)
    }

    pub(in crate::audio_engine) fn belongs_to(
        &self,
        ownership: &Arc<InputRuntimeOwnership>,
    ) -> bool {
        Arc::ptr_eq(&self.owner.ownership, ownership)
    }

    pub(in crate::audio_engine) fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner)
    }

    fn scalar_current(&self, id: usize, binding: InputPadBinding) -> bool {
        let owner = &self.owner;
        let ownership = &owner.ownership;
        !owner.admitted.load(Ordering::Acquire)
            && owner.mapping.owner.load(Ordering::Acquire)
            && owner.enabled.load(Ordering::Acquire)
            && !owner.learn_capture_active.load(Ordering::Acquire)
            && binding.placement.and_then(|p| p.content_key()) == Some(owner.key)
            && binding.source_address == owner.binding.source_address
            && binding.sample_count == owner.binding.sample_count
            && binding.channels == owner.binding.channels
            && binding.sample_rate_hz == owner.binding.sample_rate_hz
            && binding.authority_revision == owner.binding.authority_revision
            && binding.accepted == owner.binding.accepted
            && ownership.authority_current(id, binding)
            && ownership.binding_source_generation(id, binding) == Some(owner.generation)
            && ownership.key_source_request_incarnation(owner.key)
                == Some(owner.request_incarnation)
            && ownership.binding_timing_intent(id, binding) == Some(owner.timing_intent)
            && ownership
                .runtime_for_cell(owner.key.cell())
                .load(Ordering::Acquire)
                == binding.runtime_revision
            && ownership
                .launch_revision_for_cell(owner.key.cell())
                .load(Ordering::Acquire)
                == owner.launch_revision
            && ownership
                .transport_intent_for_cell(owner.key.cell())
                .load(Ordering::Acquire)
                == owner.transport_intent
    }

    pub(in crate::audio_engine) fn launch_guard(
        &self,
        own_window: Option<InputPadBinding>,
    ) -> PyResult<Option<CapturedTriggerLaunchGuard>> {
        let ownership = &self.owner.ownership;
        let resolved = match ownership.observe_control_key(self.owner.key) {
            PlacementObservation::Current(snapshot) => snapshot,
            PlacementObservation::Pending => {
                return Err(PyRuntimeError::new_err(
                    "captured MIDI placement is pending",
                ));
            }
            PlacementObservation::Stale | PlacementObservation::Retired => return Ok(None),
        };
        let state = self
            .owner
            .runtime
            .lock()
            .map_err(|_| PyRuntimeError::new_err("input runtime lock poisoned"))?;
        let row = &state.pads[resolved.slot()];
        let Some(row_binding) = row.binding else {
            return Ok(None);
        };
        if !row.loaded
            || state.multi_loop == self.owner.exclusive
            || row.loop_start_s != self.owner.loop_start_s
            || row.loop_end_s != self.owner.loop_end_s
        {
            return Ok(None);
        }
        let mut expected_original = self.owner.binding;
        expected_original.placement = Some(resolved);
        let mut selected = own_window.unwrap_or(expected_original);
        selected.placement = Some(resolved);
        // One runtime publication may project this exact own WindowACK. A changed
        // intent followed by a revert cannot regain the original runtime incarnation.
        let original_runtime = self.owner.binding.runtime_revision;
        let own_runtime = own_window
            .filter(|window| window.resident != self.owner.binding.resident)
            .and_then(|_| original_runtime.checked_add(1));
        if row_binding.runtime_revision != original_runtime
            && Some(row_binding.runtime_revision) != own_runtime
        {
            return Ok(None);
        }
        expected_original.runtime_revision = row_binding.runtime_revision;
        selected.runtime_revision = row_binding.runtime_revision;
        if !row_binding.same_projection(&expected_original)
            && !row_binding.same_projection(&selected)
        {
            return Ok(None);
        }
        let guard = CapturedTriggerLaunchGuard {
            trigger: self.clone(),
            id: resolved.slot(),
            binding: selected,
        };
        Ok(guard.current().then_some(guard))
    }

    pub(in crate::audio_engine) fn preparation_slot(
        &self,
        engine: &AudioEngine,
    ) -> PyResult<usize> {
        if !self.belongs_to(&engine.input_runtime_ownership) {
            return Err(PyValueError::new_err("foreign captured MIDI owner"));
        }
        self.launch_guard(None)?
            .map(|guard| guard.id)
            .ok_or_else(|| PyValueError::new_err("captured MIDI source or intent changed"))
    }

    pub(in crate::audio_engine) fn validates_preparation_binding(
        &self,
        id: usize,
        binding: InputPadBinding,
    ) -> bool {
        let mut original = self.owner.binding;
        original.placement = binding.placement;
        original.runtime_revision = binding.runtime_revision;
        original.same_projection(&binding)
            && self
                .launch_guard(None)
                .is_ok_and(|guard| guard.is_some_and(|guard| guard.id == id))
    }

    pub(in crate::audio_engine) fn loop_region(&self) -> (f64, Option<f64>) {
        (self.owner.loop_start_s, self.owner.loop_end_s)
    }
    pub(in crate::audio_engine) fn exclusive(&self) -> bool {
        self.owner.exclusive
    }
    pub(in crate::audio_engine) fn received_at_ns(&self) -> u64 {
        self.owner.received_at_ns
    }
    pub(in crate::audio_engine) fn note_window_prepared(&self) {
        self.owner.window_prepared.store(true, Ordering::Release);
    }

    pub(super) fn dispatch(
        &self,
        producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ) -> PyResult<bool> {
        let Some(guard) = self.launch_guard(None)? else {
            return Ok(false);
        };
        let Ok(mut producer) = producer.try_lock() else {
            return Ok(false);
        };
        if producer.is_full() {
            return Err(PyRuntimeError::new_err(
                "captured MIDI command queue is full",
            ));
        }
        let ownership = &self.owner.ownership;
        if !ownership.admission_current(guard.id, guard.binding)
            || ownership.resident_control_pending(guard.id)
        {
            return Ok(false);
        }
        if let Some(resident) = guard.binding.resident {
            let frames = guard.binding.sample_count / guard.binding.channels.max(1);
            let rate = f64::from(guard.binding.sample_rate_hz);
            let start = (self.owner.loop_start_s * rate).round() as usize;
            let end = self
                .owner
                .loop_end_s
                .map(|end| (end * rate).round() as usize);
            if crate::audio_engine::source_reader::effective_loop_region(start, end, frames)
                .is_none_or(|region| region.start < resident.start || region.end > resident.end)
            {
                return Ok(false);
            }
        }
        let Some(placement) = guard.binding.placement else {
            return Ok(false);
        };
        let prepared = ownership
            .prepare_launch_write(placement, LaunchWriteKind::Admit)
            .map_err(PyRuntimeError::new_err)?;
        let message = ControlMessage::TriggerInputPad {
            id: guard.id,
            start_s: self.owner.loop_start_s,
            end_s: self.owner.loop_end_s,
            exclusive: self.owner.exclusive,
            binding: guard.binding,
            received_at_ns: self.owner.received_at_ns,
            resident_control: None,
            launch_revision: prepared.previous_revision,
        };
        Ok(ownership
            .commit_launch_write(
                prepared,
                || guard.current() && ownership.admission_fields_current(guard.id, guard.binding),
                || {
                    guard.mark_admitted();
                    producer.push(message).expect("reserved MIDI capacity");
                },
            )
            .is_ok())
    }
}

#[pymethods]
impl CapturedInputPadTrigger {
    /// Current original-owner projection for Controller bookkeeping, never slot authority.
    #[getter]
    pub fn diagnostic_slot(&self) -> Option<usize> {
        match self.owner.ownership.observe_control_key(self.owner.key) {
            PlacementObservation::Current(snapshot) => Some(snapshot.slot()),
            _ => None,
        }
    }
    pub fn is_current(&self) -> bool {
        self.launch_guard(None).is_ok_and(|guard| guard.is_some())
    }

    /// Original scalar owner observation only. Own WindowACK and launch admission
    /// still require trigger_prepared_input_pad with the exact retained ticket.
    pub fn original_owner_status(&self) -> &'static str {
        if self.owner.admitted.load(Ordering::Acquire) {
            return "admitted";
        }
        if !self.owner.mapping.owner.load(Ordering::Acquire)
            || !self.owner.enabled.load(Ordering::Acquire)
            || self.owner.learn_capture_active.load(Ordering::Acquire)
        {
            return "superseded";
        }
        let ownership = &self.owner.ownership;
        let resolved = match ownership.observe_control_key(self.owner.key) {
            PlacementObservation::Current(snapshot) => snapshot,
            PlacementObservation::Pending => return "pending",
            PlacementObservation::Stale | PlacementObservation::Retired => return "retired",
        };
        let mut original = self.owner.binding;
        original.placement = Some(resolved);
        original.runtime_revision = ownership
            .runtime_for_cell(self.owner.key.cell())
            .load(Ordering::Acquire);
        let allowed_refresh = self
            .owner
            .window_prepared
            .load(Ordering::Acquire)
            .then(|| self.owner.binding.runtime_revision.checked_add(1))
            .flatten();
        if (original.runtime_revision != self.owner.binding.runtime_revision
            && Some(original.runtime_revision) != allowed_refresh)
            || !self.scalar_current(resolved.slot(), original)
        {
            return "superseded";
        }
        "current"
    }
}

impl InputRuntime {
    #[cfg(test)]
    pub(in crate::audio_engine) fn capture_trigger_for_test(
        &self,
        binding_key: &str,
        received_at_ns: u64,
    ) -> Option<CapturedInputPadTrigger> {
        let mapping = super::lookup_mapping(&self.mappings, binding_key)?;
        CapturedInputPadTrigger::capture(
            &mapping,
            received_at_ns,
            &self.runtime_state,
            &self.ownership,
            &self.enabled,
            &self.learn_capture_active,
        )
    }
    pub(in crate::audio_engine) fn trigger_captured_pad(
        &self,
        trigger: &CapturedInputPadTrigger,
    ) -> PyResult<bool> {
        if !trigger.belongs_to(&self.ownership)
            || !Arc::ptr_eq(&trigger.owner.runtime, &self.runtime_state)
        {
            return Ok(false);
        }
        trigger.dispatch(&self.audio_producer)
    }
    pub(in crate::audio_engine) fn prepare_captured_trigger(
        &self,
        engine: &AudioEngine,
        trigger: &CapturedInputPadTrigger,
    ) -> PyResult<ResidentWindowTicket> {
        if !trigger.belongs_to(&self.ownership)
            || !Arc::ptr_eq(&trigger.owner.runtime, &self.runtime_state)
        {
            return Err(PyValueError::new_err("foreign captured MIDI runtime"));
        }
        resident_relocation::prepare_captured_control(engine, trigger)
    }
    pub(in crate::audio_engine) fn trigger_prepared_pad(
        &self,
        engine: &AudioEngine,
        trigger: &CapturedInputPadTrigger,
        window: &ResidentWindowTicket,
    ) -> PyResult<bool> {
        if !trigger.belongs_to(&self.ownership)
            || !Arc::ptr_eq(&trigger.owner.runtime, &self.runtime_state)
        {
            return Ok(false);
        }
        resident_relocation::launch_captured_with_producer(
            engine,
            trigger,
            window,
            &self.audio_producer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::complete_context::source_custody_tests::own_pair_ack;
    use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, RATE};
    use crate::audio_engine::timing::InputClock;
    use crate::audio_engine::{input_runtime_binding, pad_pair};
    use std::time::Duration;

    fn runtime_fixture(capacity: usize) -> LaunchFixture {
        let mut fixture = LaunchFixture::new(capacity);
        let runtime = InputRuntime::new_with_ownership(
            fixture.producer.clone(),
            InputClock::new(),
            fixture.engine.input_runtime_ownership.clone(),
        );
        runtime.set_enabled(true);
        runtime.replace_mappings(vec![("midi:note:1:60".into(), "pad.trigger:0".into())]);
        let mut bindings = (0..crate::audio_engine::constants::NUM_SAMPLES)
            .map(|_| None)
            .collect::<Vec<_>>();
        for id in [0, 1] {
            bindings[id] = input_runtime_binding::capture(&fixture.engine, id).unwrap();
        }
        runtime
            .set_runtime_state(
                true,
                bindings.iter().map(Option::is_some).collect(),
                vec![16.0 / f64::from(RATE); bindings.len()],
                vec![Some(96.0 / f64::from(RATE)); bindings.len()],
                bindings.iter().map(Option::as_ref).collect(),
            )
            .unwrap();
        fixture.engine.input_runtime = Some(runtime);
        fixture
    }

    fn original_failed_event(fixture: &mut LaunchFixture) -> CapturedInputPadTrigger {
        // Actual dispatcher pressure returns the same admitted input to Python.
        fixture
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
        let runtime = fixture.engine.input_runtime.as_ref().unwrap();
        assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
        let mut event = None;
        for _ in 0..100 {
            event = runtime.poll_event();
            if event.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let event = event.expect("actual original MIDI event");
        assert!(event.direct && !event.dispatched);
        let trigger = event
            .captured_trigger
            .expect("captured original mapping/source");
        assert_eq!(trigger.received_at_ns(), event.received_at_ns);
        assert!(matches!(
            fixture.consumer.pop().unwrap(),
            ControlMessage::Ping()
        ));
        trigger
    }

    #[test]
    fn original_midi_pressure_event_follows_actual_pair_and_launches_once() {
        let mut fixture = runtime_fixture(1);
        let trigger = original_failed_event(&mut fixture);
        let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
        let mut mixer = fixture.mixer();
        let pair = own_pair_ack(&mut fixture, &mut mixer);
        assert_eq!(trigger.diagnostic_slot(), None);
        assert!(pad_pair::reconcile(&fixture.engine, &pair).unwrap());
        assert_eq!(trigger.diagnostic_slot(), Some(1));
        assert!(
            !fixture
                .engine
                .input_runtime
                .as_ref()
                .unwrap()
                .trigger_captured_pad(&trigger)
                .unwrap()
        );
        assert!(fixture.consumer.is_empty());
        let acknowledgement = pair.own_acknowledgement().unwrap();
        fixture
            .engine
            ._finish_pad_pair(&pair, &acknowledgement)
            .unwrap();
        let runtime = fixture.engine.input_runtime.as_ref().unwrap();
        assert!(runtime.trigger_captured_pad(&trigger).unwrap());
        let ControlMessage::TriggerInputPad {
            id,
            received_at_ns,
            exclusive,
            binding,
            ..
        } = fixture.consumer.pop().unwrap()
        else {
            panic!("original MIDI launch");
        };
        assert_eq!(id, 1);
        assert_eq!(received_at_ns, trigger.received_at_ns());
        assert!(!exclusive);
        assert_eq!(
            binding.placement.unwrap().content_key(),
            Some(trigger.owner.key)
        );
        assert!(!runtime.trigger_captured_pad(&trigger).unwrap());
        assert!(fixture.consumer.is_empty());
        assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    }

    #[test]
    fn original_midi_event_keeps_mapping_incarnation_and_rejects_later_intents() {
        for case in 0..7 {
            let mut fixture = runtime_fixture(1);
            let trigger = original_failed_event(&mut fixture);
            let runtime = fixture.engine.input_runtime.as_ref().unwrap();
            let ownership = &fixture.engine.input_runtime_ownership;
            let key = trigger.owner.key;
            match case {
                0 => {
                    runtime
                        .replace_mappings(vec![("midi:note:1:60".into(), "pad.trigger:1".into())]);
                    runtime
                        .replace_mappings(vec![("midi:note:1:60".into(), "pad.trigger:0".into())]);
                }
                1 => runtime.set_enabled(false),
                2 => runtime.set_learn_capture_active(true),
                3 => ownership
                    .prepare_source_request_advance(0)
                    .unwrap()
                    .commit(),
                4 => {
                    ownership
                        .runtime_for_cell(key.cell())
                        .fetch_add(1, Ordering::AcqRel);
                }
                5 => {
                    ownership
                        .transport_intent_for_cell(key.cell())
                        .fetch_add(1, Ordering::AcqRel);
                }
                6 => fixture.publish(0, 0.25, 7),
                _ => unreachable!(),
            }
            assert!(
                !runtime.trigger_captured_pad(&trigger).unwrap(),
                "case {case}"
            );
            assert!(fixture.consumer.is_empty(), "case {case}");
            assert!(ownership.admitted_launch_ids().is_empty(), "case {case}");
        }
    }

    #[test]
    fn unchanged_mapping_snapshot_preserves_the_original_midi_owner() {
        let mut fixture = runtime_fixture(1);
        let trigger = original_failed_event(&mut fixture);
        let runtime = fixture.engine.input_runtime.as_ref().unwrap();
        runtime.replace_mappings(vec![("midi:note:1:60".into(), "pad.trigger:0".into())]);
        assert!(runtime.trigger_captured_pad(&trigger).unwrap());
        assert!(fixture.consumer.pop().is_ok());
    }
}
