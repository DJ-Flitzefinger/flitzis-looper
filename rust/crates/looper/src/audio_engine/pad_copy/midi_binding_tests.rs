//! R2-discovered R1 regression: real MIDI publication must not make Copy's source stale.
//! These cases add coverage; earlier R1 receipts did not execute this MIDI-runtime sequence.

use super::{END, Fixture, NUM_SAMPLES, RATE, START, isolated};
use crate::audio_engine::AudioEngine;
use crate::audio_engine::input_mapping::InputRuntime;
use crate::messages::ControlMessage;
use pyo3::prelude::*;
use std::sync::Arc;
use std::sync::atomic::Ordering;

fn publish_mapped_runtime(fixture: &Fixture, start: usize) -> u64 {
    Python::attach(|py| {
        let mut engine = fixture.engine.borrow_mut(py);
        if engine.input_runtime.is_none() {
            engine.input_runtime = Some(InputRuntime::new_with_ownership(
                fixture.producer.clone(),
                engine.input_clock,
                engine.input_runtime_ownership.clone(),
            ));
            engine
                .set_input_mapping_snapshot(vec![("midi:note:1:60".into(), "pad.trigger:0".into())])
                .unwrap();
            engine.set_input_mapping_enabled(true).unwrap();
        }
        let binding = engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .unwrap();
        assert!(binding.is_current());
        assert_eq!(binding.binding.runtime_revision, 0);
        let mut loaded = vec![false; NUM_SAMPLES];
        loaded[0] = true;
        let mut starts = vec![0.0; NUM_SAMPLES];
        starts[0] = start as f64 / f64::from(RATE);
        let mut ends = vec![None; NUM_SAMPLES];
        ends[0] = Some(END as f64 / f64::from(RATE));
        let mut bindings: Vec<_> = (0..NUM_SAMPLES).map(|_| None).collect();
        bindings[0] = Some(Py::new(py, binding).unwrap());
        engine
            .set_input_runtime_state(py, true, loaded, starts, ends, Some(bindings))
            .unwrap();
        let revision = engine.input_runtime_ownership.runtime_revision(0);
        assert_ne!(revision, 0);
        revision
    })
}

fn admit_actual_mapped_source(fixture: &mut Fixture, revision: u64) {
    fixture.with_engine(|engine| {
        let dispatch = engine
            .input_runtime
            .as_ref()
            .unwrap()
            .capture_mapping_dispatch_for_test("midi:note:1:60", 42)
            .expect("configured MIDI mapping must reach its actual admission path");
        assert!(dispatch());
    });
    match fixture.callback.controls.peek().unwrap() {
        ControlMessage::TriggerInputPad { id, binding, .. } => {
            assert_eq!(*id, 0);
            assert_eq!(binding.runtime_revision, revision);
        }
        other => panic!("expected actual mapped MIDI source admission, got {other:?}"),
    }
    assert_eq!(fixture.callback.drain(), 1);
    fixture.callback.render(17);
    assert!(fixture.source_voice().active);
    assert!(!fixture.source_voice().paused);
}

#[test]
fn published_midi_runtime_copies_with_exact_current_revision_and_own_complete_ack() {
    isolated(
        "midi_binding_tests::published_midi_runtime_copies_with_exact_current_revision_and_own_complete_ack",
        || {
            let mut fixture = Fixture::new(false, false, "manual");
            let revision = publish_mapped_runtime(&fixture, START);
            admit_actual_mapped_source(&mut fixture, revision);
            let generation = fixture.source_voice().generation;
            let position = fixture.source_voice().source_playback.position();
            let source = fixture.callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap()
                .clone();
            let authority =
                fixture.with_engine(|engine| engine.input_runtime_ownership.authority_revision(0));
            let captured = fixture.capture(36);
            let ticket = fixture.prepare(&captured);
            fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
            match fixture.callback.controls.peek().unwrap() {
                ControlMessage::PadCopy(work) => {
                    assert_eq!(work.guard.source_binding.runtime_revision, revision);
                    assert!(work.guard.current());
                    assert_eq!(work.guard.phase.load(Ordering::Acquire), 0);
                    assert!(Arc::ptr_eq(&source.samples, &work.sample.samples));
                }
                other => panic!("expected complete Native Copy admission, got {other:?}"),
            }
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(ticket.publication_status(), "accepted");
            fixture.with_engine(|engine| {
                assert!(engine.reconcile_pad_copy(&ticket).unwrap());
                engine.finish_pad_copy(&ticket).unwrap();
                assert_eq!(engine.input_runtime_ownership.copy_reservation(36), 0);
                assert_eq!(engine.input_runtime_ownership.runtime_revision(0), revision);
                assert_eq!(
                    engine.input_runtime_ownership.authority_revision(0),
                    authority
                );
                assert!(
                    engine
                        .current_input_runtime_pad_binding(36)
                        .unwrap()
                        .unwrap()
                        .is_current()
                );
            });
            assert_eq!(fixture.source_voice().generation, generation);
            assert_eq!(fixture.source_voice().source_playback.position(), position);
            let target = fixture.callback.mixer.bank_for_measurement()[36]
                .as_ref()
                .unwrap();
            assert!(Arc::ptr_eq(&source.samples, &target.samples));
            assert!(
                fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .all(|voice| { voice.sample_id != 36 || (!voice.active && !voice.paused) })
            );
            fixture.callback.render(29);
            assert_eq!(fixture.source_voice().generation, generation);
            assert_ne!(fixture.source_voice().source_playback.position(), position);
        },
    );
}

#[derive(Clone, Copy, Debug)]
enum SupersessionPoint {
    BeforePreparation,
    BeforePublication,
    BeforeCallbackClaim,
}

#[test]
fn later_actual_midi_revision_rejects_copy_before_claim_without_target_or_voice_mutation() {
    isolated(
        "midi_binding_tests::later_actual_midi_revision_rejects_copy_before_claim_without_target_or_voice_mutation",
        || {
            for point in [
                SupersessionPoint::BeforePreparation,
                SupersessionPoint::BeforePublication,
                SupersessionPoint::BeforeCallbackClaim,
            ] {
                let mut fixture = Fixture::new(false, false, "manual");
                let revision = publish_mapped_runtime(&fixture, START);
                admit_actual_mapped_source(&mut fixture, revision);
                let target = 215;
                let generation = fixture.source_voice().generation;
                let position = fixture.source_voice().source_playback.position();
                let source = fixture.callback.mixer.bank_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .clone();
                let (
                    source_authority,
                    target_before,
                    target_authority,
                    target_epoch,
                    target_runtime,
                ) = fixture.with_engine(|engine| {
                    (
                        engine.input_runtime_ownership.authority_revision(0),
                        engine
                            .input_runtime_ownership
                            .slot_snapshot(target)
                            .unwrap(),
                        engine.input_runtime_ownership.authority_revision(target),
                        engine
                            .prepared_source_epoch(target)
                            .unwrap()
                            .load(Ordering::Acquire),
                        engine.input_runtime_ownership.runtime_revision(target),
                    )
                });
                let captured = fixture.capture(target);
                let ticket = if matches!(point, SupersessionPoint::BeforePreparation) {
                    None
                } else {
                    Some(fixture.prepare(&captured))
                };
                if matches!(point, SupersessionPoint::BeforeCallbackClaim) {
                    fixture.with_engine(|engine| {
                        engine.publish_pad_copy(ticket.as_ref().unwrap()).unwrap()
                    });
                }
                let newer = publish_mapped_runtime(&fixture, START + 1);
                assert!(newer > revision, "{point:?}");
                match point {
                    SupersessionPoint::BeforePreparation => {
                        let error = Python::attach(|py| {
                            AudioEngine::prepare_pad_copy(
                                fixture.engine.clone_ref(py),
                                py,
                                &captured,
                            )
                        })
                        .err()
                        .expect("new MIDI runtime must reject stale preparation");
                        assert!(!error.to_string().is_empty());
                        assert_eq!(captured.publication_status(), "rejected");
                        assert!(fixture.callback.controls.is_empty());
                    }
                    SupersessionPoint::BeforePublication => {
                        let ticket = ticket.as_ref().unwrap();
                        fixture.with_engine(|engine| {
                            assert!(engine.publish_pad_copy(ticket).is_err());
                            assert!(!engine.reconcile_pad_copy(ticket).unwrap());
                        });
                        assert!(fixture.callback.controls.is_empty());
                    }
                    SupersessionPoint::BeforeCallbackClaim => {
                        let ticket = ticket.as_ref().unwrap();
                        match fixture.callback.controls.peek().unwrap() {
                            ControlMessage::PadCopy(work) => {
                                assert_eq!(work.guard.source_binding.runtime_revision, revision);
                                assert!(!work.guard.current());
                                assert_eq!(work.guard.phase.load(Ordering::Acquire), 0);
                            }
                            other => panic!("expected the original queued Copy, got {other:?}"),
                        }
                        assert_eq!(fixture.callback.drain(), 1);
                        assert_eq!(ticket.publication_status(), "rejected");
                        fixture.with_engine(|engine| {
                            assert!(!engine.reconcile_pad_copy(ticket).unwrap())
                        });
                    }
                }
                assert!(fixture.callback.mixer.bank_for_measurement()[target].is_none());
                assert!(Arc::ptr_eq(
                    &source.samples,
                    &fixture.callback.mixer.bank_for_measurement()[0]
                        .as_ref()
                        .unwrap()
                        .samples,
                ));
                assert_eq!(fixture.source_voice().generation, generation, "{point:?}");
                assert_eq!(
                    fixture.source_voice().source_playback.position(),
                    position,
                    "{point:?}"
                );
                fixture.with_engine(|engine| {
                    assert!(engine.sample_cache.lock().unwrap()[target].is_none());
                    let target_after = engine
                        .input_runtime_ownership
                        .slot_snapshot(target)
                        .unwrap();
                    assert!(target_after.same_projection(target_before));
                    assert!(target_after.layout_revision() >= target_before.layout_revision());
                    assert_eq!(
                        engine.input_runtime_ownership.authority_revision(target),
                        target_authority
                    );
                    assert_eq!(
                        engine
                            .prepared_source_epoch(target)
                            .unwrap()
                            .load(Ordering::Acquire),
                        target_epoch
                    );
                    assert_eq!(
                        engine.input_runtime_ownership.runtime_revision(target),
                        target_runtime
                    );
                    assert_eq!(
                        engine.input_runtime_ownership.authority_revision(0),
                        source_authority
                    );
                    assert_eq!(engine.input_runtime_ownership.runtime_revision(0), newer);
                    assert!(engine.cancel_pad_copy(&captured).unwrap());
                    assert_eq!(engine.input_runtime_ownership.copy_reservation(target), 0);
                });
                fixture.callback.render(31);
                assert_eq!(fixture.source_voice().generation, generation, "{point:?}");
                assert_ne!(
                    fixture.source_voice().source_playback.position(),
                    position,
                    "{point:?}"
                );
            }
        },
    );
}

#[test]
fn copy_accepts_same_source_binding_after_unrelated_publication_revision() {
    isolated(
        "midi_binding_tests::copy_accepts_same_source_binding_after_unrelated_publication_revision",
        || {
            let mut fixture = Fixture::new(false, false, "manual");
            let generation = fixture.source_voice().generation;
            let position = fixture.source_voice().source_playback.position();
            let captured = fixture.with_engine(|engine| {
                let binding = engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap();
                let ownership = &engine.input_runtime_ownership;
                let other = ownership.slot_snapshot(215).unwrap();
                let write = ownership
                    .prepare_slot_write_fixed(other, other.lifetime())
                    .unwrap();
                ownership
                    .commit_slot_write_fixed(&write, || true, |_| {})
                    .unwrap();
                let fresh = engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap();
                assert!(binding.is_current());
                assert_ne!(binding.binding, fresh.binding);
                assert!(binding.binding.same_projection(&fresh.binding));
                engine
                    .capture_pad_copy(&binding, 36, &fixture.envelope())
                    .unwrap()
            });
            let ticket = fixture.prepare(&captured);
            fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(ticket.publication_status(), "accepted");
            fixture.with_engine(|engine| {
                assert!(engine.reconcile_pad_copy(&ticket).unwrap());
                engine.finish_pad_copy(&ticket).unwrap();
                assert_eq!(engine.input_runtime_ownership.copy_reservation(36), 0);
            });
            assert_eq!(fixture.source_voice().generation, generation);
            assert_eq!(fixture.source_voice().source_playback.position(), position);
        },
    );
}
