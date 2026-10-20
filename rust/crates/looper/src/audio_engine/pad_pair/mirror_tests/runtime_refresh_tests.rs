//! Actual Pair callback ACK plus the installed MIDI runtime, without a device.
//! These regressions do not certify Python Config, close, music or Human acceptance.

use super::*;
use crate::audio_engine::input_mapping::{InputRuntime, PitchPadProjection};

struct RuntimeRows {
    loaded: Vec<bool>,
    starts: Vec<f64>,
    ends: Vec<Option<f64>>,
    bindings: Vec<Option<InputRuntimePadBinding>>,
}

impl RuntimeRows {
    fn install(fixture: &mut LaunchFixture) -> Self {
        assert!(fixture.engine.input_runtime.is_none());
        let runtime = InputRuntime::new_with_ownership(
            fixture.producer.clone(),
            fixture.engine.input_clock,
            fixture.engine.input_runtime_ownership.clone(),
        );
        runtime.set_render_format(48_000, 1).unwrap();
        fixture.engine.input_runtime = Some(runtime);
        let mut rows = Self {
            loaded: vec![false; NUM_SAMPLES],
            starts: vec![0.0; NUM_SAMPLES],
            ends: vec![None; NUM_SAMPLES],
            bindings: (0..NUM_SAMPLES).map(|_| None).collect(),
        };
        for id in [0, 1] {
            rows.loaded[id] = true;
            rows.starts[id] = 16.0 / 48_000.0;
            rows.ends[id] = Some(96.0 / 48_000.0);
            rows.bindings[id] = input_runtime_binding::capture(&fixture.engine, id).unwrap();
        }
        rows.publish(&fixture.engine, true);
        for id in [0, 1] {
            let runtime = fixture.engine.input_runtime.as_ref().unwrap();
            let binding = runtime.pair_binding_for_test(id).unwrap();
            assert!(fixture.engine.input_runtime_ownership.current(id, binding));
            assert_ne!(binding.runtime_revision, 0);
        }
        rows
    }

    fn publish(&self, engine: &AudioEngine, multi_loop: bool) {
        // This is the full production publication path, including pitch rows.
        // Retain the published Python-side before rows while participants are fenced.
        engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_pitch(
                multi_loop,
                self.loaded.clone(),
                self.starts.clone(),
                self.ends.clone(),
                self.bindings.iter().map(Option::as_ref).collect(),
                vec![PitchPadProjection::default(); NUM_SAMPLES],
            )
            .unwrap();
    }
}

fn dispatch_pair(
    fixture: &mut LaunchFixture,
    target: usize,
    before_ack: impl FnOnce(&AudioEngine),
) -> (CapturedPadPair, PadPairTicket) {
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let captured = capture(&fixture.engine, 0, target).unwrap();
    assert!(captured.state.runtime.is_some());
    let ticket = prepare(&fixture.engine, &captured, 48_000, 1).unwrap();
    publish(&fixture.engine, &ticket).unwrap();
    assert!(ticket.native_acknowledgement().is_none());
    before_ack(&fixture.engine);
    let message = fixture.consumer.pop().unwrap();
    assert!(matches!(&message, ControlMessage::PadPair(_)));
    dispatch(
        &mut mixer,
        message,
        &mut FixedCapacityScheduler::<8>::new(),
        &mut TransportTimeline::new(48_000),
        &mut TriggerQuantization::Immediate,
        &mut Vec::new(),
    );
    assert!(fixture.consumer.is_empty());
    (captured, ticket)
}

fn reconcile_and_finish(fixture: &LaunchFixture, target: usize, ticket: &PadPairTicket) {
    let native_ack = ticket.native_acknowledgement().unwrap();
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(!ticket.state.mirrored.load(Ordering::Acquire));
    assert!(!ticket.state.finished.load(Ordering::Acquire));
    let ack = ticket.own_acknowledgement().unwrap();
    assert!(finish(&fixture.engine, ticket, &ack).is_err());
    for slot in [0, target] {
        assert_ne!(
            fixture
                .engine
                .input_runtime_ownership
                .pair_reservation(slot),
            0
        );
    }
    assert!(reconcile(&fixture.engine, ticket).unwrap());
    assert!(reconcile(&fixture.engine, ticket).unwrap());
    assert!(validate_acknowledgement(&fixture.engine, ticket, &ack).unwrap());
    let runtime = fixture.engine.input_runtime.as_ref().unwrap();
    for slot in [0, target] {
        let placement = fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(slot)
            .unwrap();
        let binding = runtime.pair_binding_for_test(slot);
        if placement.lifetime() == 0 {
            assert!(binding.is_none());
        } else {
            let binding = binding.unwrap();
            assert!(
                binding
                    .placement
                    .is_some_and(|original| { original.same_projection(placement) })
            );
            assert!(
                fixture
                    .engine
                    .input_runtime_ownership
                    .current(slot, binding)
            );
        }
    }
    finish(&fixture.engine, ticket, &ack).unwrap();
    finish(&fixture.engine, ticket, &ack).unwrap();
    assert!(ticket.state.finished.load(Ordering::Acquire));
    assert_eq!(ticket.native_acknowledgement(), Some(native_ack));
    for slot in [0, target] {
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .pair_reservation(slot),
            0
        );
    }
}

#[test]
fn installed_runtime_pair_move_and_swap_reconcile_and_finish_without_refresh() {
    for target in [1, 2] {
        let mut fixture = fixture();
        let _rows = RuntimeRows::install(&mut fixture);
        let (_, ticket) = dispatch_pair(&mut fixture, target, |_| {});
        reconcile_and_finish(&fixture, target, &ticket);
    }
}

#[test]
fn installed_runtime_pair_periodic_refresh_before_and_after_ack_preserves_own_rows() {
    for target in [1, 2] {
        let mut fixture = fixture();
        let rows = RuntimeRows::install(&mut fixture);
        let (_, ticket) = dispatch_pair(&mut fixture, target, |engine| rows.publish(engine, true));
        assert!(ticket.native_acknowledgement().is_some());
        rows.publish(&fixture.engine, true);
        reconcile_and_finish(&fixture, target, &ticket);
    }
}

#[test]
fn installed_runtime_pair_explicit_multi_loop_after_ack_forward_settles_same_owner() {
    for target in [1, 2] {
        let mut fixture = fixture();
        let mut rows = RuntimeRows::install(&mut fixture);
        // Give the two content cells different runtime epochs, then restore the
        // original geometry before capture. Slot-based rebasing must not pass
        // accidentally just because both participants started at the same epoch.
        let original_start = rows.starts[0];
        rows.starts[0] = original_start + 1.0 / 48_000.0;
        rows.publish(&fixture.engine, true);
        rows.starts[0] = original_start;
        rows.publish(&fixture.engine, true);
        let ownership = fixture.engine.input_runtime_ownership.clone();
        let originals = [0, target].map(|slot| ownership.slot_snapshot(slot).unwrap());
        let original_revisions = originals.map(|slot| {
            ownership
                .runtime_for_cell(slot.cell())
                .load(Ordering::Acquire)
        });
        assert_ne!(original_revisions[0], original_revisions[1]);
        let original_bindings = [0, target].map(|slot| {
            fixture
                .engine
                .input_runtime
                .as_ref()
                .unwrap()
                .pair_binding_for_test(slot)
        });
        let (_, ticket) = dispatch_pair(&mut fixture, target, |_| {});
        assert!(ticket.native_acknowledgement().is_some());
        for (index, multi_loop) in [false, true, false, true].into_iter().enumerate() {
            rows.publish(&fixture.engine, multi_loop);
            for (original, revision) in originals.iter().zip(original_revisions) {
                assert_eq!(
                    ownership
                        .runtime_for_cell(original.cell())
                        .load(Ordering::Acquire),
                    revision + index as u64 + 1
                );
            }
            for (slot, original) in [0, target].into_iter().zip(original_bindings) {
                assert_eq!(
                    fixture
                        .engine
                        .input_runtime
                        .as_ref()
                        .unwrap()
                        .pair_binding_for_test(slot),
                    original
                );
            }
        }
        reconcile_and_finish(&fixture, target, &ticket);
    }
}

#[test]
fn installed_runtime_pair_explicit_multi_loop_before_ack_rejects_and_cancels_original() {
    for target in [1, 2] {
        let mut fixture = fixture();
        let rows = RuntimeRows::install(&mut fixture);
        let (captured, ticket) =
            dispatch_pair(&mut fixture, target, |engine| rows.publish(engine, false));
        assert!(ticket.native_acknowledgement().is_none());
        assert_eq!(ticket.publication_status(), "rejected");
        assert!(!reconcile(&fixture.engine, &ticket).unwrap());
        assert!(cancel(&fixture.engine, &captured).unwrap());
        for slot in [0, target] {
            assert_eq!(
                fixture
                    .engine
                    .input_runtime_ownership
                    .pair_reservation(slot),
                0
            );
        }
        assert!(!ticket.state.mirrored.load(Ordering::Acquire));
    }
}

#[test]
fn installed_runtime_pair_foreign_source_custody_stays_fenced_then_retries_original_owner() {
    for target in [1, 2] {
        let mut fixture = fixture();
        let _rows = RuntimeRows::install(&mut fixture);
        let (captured, ticket) = dispatch_pair(&mut fixture, target, |_| {});
        let original = fixture.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        let mut foreign = original.clone();
        foreign.residency = None;
        foreign.samples = Arc::from([0.25_f32; 128]);
        assert!(!foreign.same_source(&original));
        fixture.engine.sample_cache.lock().unwrap()[0] = Some(foreign);
        let error = reconcile(&fixture.engine, &ticket).unwrap_err().to_string();
        assert!(
            error.contains("pair MIDI immutable source owner changed"),
            "{error}"
        );
        assert!(!ticket.state.mirrored.load(Ordering::Acquire));
        assert!(!cancel(&fixture.engine, &captured).unwrap());
        for slot in [0, target] {
            assert_ne!(
                fixture
                    .engine
                    .input_runtime_ownership
                    .pair_reservation(slot),
                0
            );
        }
        fixture.engine.sample_cache.lock().unwrap()[0] = Some(original);
        reconcile_and_finish(&fixture, target, &ticket);
    }
}
