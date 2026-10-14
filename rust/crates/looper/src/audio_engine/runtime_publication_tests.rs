//! The complete MIDI mirror and all stable-cell revisions publish at one Native boundary.

use super::*;
use crate::audio_engine::AudioEngine;
use crate::audio_engine::content_placement::SlotSnapshot;
use crate::messages::{ResidentContext, SampleBuffer};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use pyo3::Python;
use rtrb::{Consumer, RingBuffer};
use std::cell::Cell;
use std::sync::Barrier;

const ID: usize = 0;
const LAST: usize = NUM_SAMPLES - 1;
const RATE: u32 = 48_000;
const OLD_START: f64 = 2.0 / RATE as f64;
const NEW_START: f64 = 4.0 / RATE as f64;
const LAST_START: f64 = 3.0 / RATE as f64;
const END: f64 = 24.0 / RATE as f64;

struct Inputs {
    loaded: Vec<bool>,
    starts: Vec<f64>,
    ends: Vec<Option<f64>>,
}

impl Inputs {
    fn new(changed: bool, change_last: bool) -> Self {
        let mut loaded = vec![false; NUM_SAMPLES];
        loaded[ID] = true;
        loaded[LAST] = true;
        let mut starts = vec![0.0; NUM_SAMPLES];
        starts[ID] = if changed { NEW_START } else { OLD_START };
        starts[LAST] = if change_last { NEW_START } else { LAST_START };
        let mut ends = vec![None; NUM_SAMPLES];
        ends[ID] = Some(END);
        ends[LAST] = Some(END);
        Self {
            loaded,
            starts,
            ends,
        }
    }
}

struct Observation {
    slots: [SlotSnapshot; NUM_SAMPLES],
    revisions: [u64; NUM_SAMPLES],
    state: RuntimeState,
}

struct Fixture {
    engine: AudioEngine,
    runtime: InputRuntime,
    controls: Consumer<ControlMessage>,
    bindings: Vec<Option<InputRuntimePadBinding>>,
}

impl Fixture {
    fn new() -> Self {
        Python::initialize();
        let engine = AudioEngine::new().unwrap();
        Self::publish_source(&engine, ID, Self::sample(0.25), 7);
        Self::publish_source(&engine, LAST, Self::sample(-0.5), 8);
        let bindings: Vec<_> = (0..NUM_SAMPLES)
            .map(|id| {
                if id == ID || id == LAST {
                    let binding = engine
                        .current_input_runtime_pad_binding(id)
                        .unwrap()
                        .unwrap();
                    assert!(binding.is_current());
                    assert!(binding.available());
                    assert_ne!(binding.binding.placement.unwrap().lifetime(), 0);
                    Some(binding)
                } else {
                    None
                }
            })
            .collect();
        let (producer, controls) = RingBuffer::new(8);
        let runtime = InputRuntime::new_with_ownership(
            Arc::new(Mutex::new(producer)),
            engine.input_clock,
            engine.input_runtime_ownership.clone(),
        );
        assert!(Arc::ptr_eq(
            &runtime.ownership,
            &engine.input_runtime_ownership
        ));
        let inputs = Inputs::new(false, false);
        // Changing multi_loop from the real default also initializes all unloaded rows. No
        // synthetic zero-runtime binding or untracked owner grants admission in these tests.
        runtime
            .set_runtime_state(
                true,
                inputs.loaded,
                inputs.starts,
                inputs.ends,
                bindings.iter().map(Option::as_ref).collect(),
            )
            .unwrap();
        runtime.replace_mappings(vec![
            ("midi:note:1:60".into(), "pad.trigger:0".into()),
            ("midi:note:1:61".into(), format!("pad.trigger:{LAST}")),
        ]);
        runtime.set_enabled(true);
        let fixture = Self {
            engine,
            runtime,
            controls,
            bindings,
        };
        assert!(
            fixture
                .observe()
                .revisions
                .iter()
                .all(|revision| *revision > 0)
        );
        fixture
    }

    fn sample(value: f32) -> SampleBuffer {
        SampleBuffer {
            samples: Arc::from([value; 64]),
            channels: 1,
            residency: None,
        }
        .with_complete_source(RATE)
    }

    fn publish_source(engine: &AudioEngine, id: usize, sample: SampleBuffer, generation: u64) {
        let mut requests = engine.pad_request_ids.lock().unwrap();
        let ownership = &engine.input_runtime_ownership;
        let captured = ownership.slot_snapshot(id).unwrap();
        let lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
        let prepared = ownership.prepare_slot_write(captured, lifetime).unwrap();
        let authority = ownership.next_authority(id).unwrap();
        let publication = ownership
            .publish_source_write(
                &prepared,
                &sample,
                RATE,
                generation,
                authority,
                Some(TimingIntent::Manual),
                false,
                || true,
            )
            .unwrap();
        assert!(publication.current(&sample, RATE));
        assert_ne!(publication.key().unwrap().lifetime(), 0);
        requests[id] = generation;
        engine.sample_cache.lock().unwrap()[id] = Some(sample);
        engine.loaded_source_generations.lock().unwrap()[id] = (generation, RATE);
        engine.loaded_source_digests.lock().unwrap()[id] = Some(format!("{generation:064x}"));
        engine.timing_intents.lock().unwrap()[id] = TimingIntent::Manual;
        engine
            .prepared_source_epoch(id)
            .unwrap()
            .store(generation, Ordering::Release);
    }

    fn binding_refs(&self) -> Vec<Option<&InputRuntimePadBinding>> {
        self.bindings.iter().map(Option::as_ref).collect()
    }

    fn candidate(&self, change_last: bool, before_commit: impl FnOnce()) -> Result<(), String> {
        let inputs = Inputs::new(true, change_last);
        self.runtime.set_runtime_state_before_commit(
            true,
            inputs.loaded,
            inputs.starts,
            inputs.ends,
            self.binding_refs(),
            before_commit,
        )
    }

    fn revisions_at(&self, slots: &[SlotSnapshot; NUM_SAMPLES]) -> [u64; NUM_SAMPLES] {
        // This remains an exact observation while the global layout is odd and after a Move.
        // A slot resolver returning zero during Pending would invalidate the test oracle.
        std::array::from_fn(|id| {
            self.runtime
                .ownership
                .runtime_for_cell(slots[id].cell())
                .load(Ordering::Acquire)
        })
    }

    fn observe(&self) -> Observation {
        let slots = std::array::from_fn(|id| self.runtime.ownership.slot_snapshot(id).unwrap());
        let revisions = self.revisions_at(&slots);
        let state = self.runtime.runtime_state.lock().unwrap().clone();
        Observation {
            slots,
            revisions,
            state,
        }
    }

    fn assert_runtime_unchanged(&self, before: &Observation) {
        assert_eq!(self.revisions_at(&before.slots), before.revisions);
        let state = self.runtime.runtime_state.lock().unwrap();
        assert_eq!(state.multi_loop, before.state.multi_loop);
        assert_eq!(state.pads, before.state.pads);
    }

    fn mapped_trigger(&mut self, id: usize, start: f64, revision: u64) -> InputPadBinding {
        let key = if id == ID {
            "midi:note:1:60"
        } else {
            "midi:note:1:61"
        };
        assert!(self
            .runtime
            .capture_mapping_dispatch_for_test(key, 42)
            .unwrap()());
        let ControlMessage::TriggerInputPad {
            id: actual_id,
            start_s,
            end_s,
            exclusive,
            binding,
            received_at_ns,
            ..
        } = self.controls.pop().unwrap()
        else {
            panic!("actual MIDI mapping must admit a guarded Native trigger");
        };
        assert_eq!(actual_id, id);
        assert_eq!(start_s, start);
        assert_eq!(end_s, Some(END));
        assert!(!exclusive);
        assert_eq!(received_at_ns, 42);
        assert_eq!(binding.runtime_revision, revision);
        assert_ne!(revision, 0);
        assert!(self.runtime.ownership.admission_current(id, binding));
        assert_eq!(
            binding.placement.unwrap().content_key(),
            self.bindings[id]
                .as_ref()
                .unwrap()
                .binding
                .placement
                .unwrap()
                .content_key()
        );
        assert!(self.controls.is_empty());
        binding
    }

    fn retry_and_assert_exact_publication(&mut self, before: &Observation) {
        let old = self.mapped_trigger(ID, OLD_START, before.revisions[ID]);
        let unaffected = self.mapped_trigger(LAST, LAST_START, before.revisions[LAST]);
        self.candidate(false, || {}).unwrap();
        let after = self.observe();
        for id in 0..NUM_SAMPLES {
            assert_eq!(
                after.revisions[id],
                before.revisions[id] + u64::from(id == ID),
                "runtime row {id}"
            );
        }
        assert_eq!(after.state.multi_loop, before.state.multi_loop);
        assert_eq!(after.state.pads[ID].loop_start_s, NEW_START);
        assert_eq!(after.state.pads[LAST], before.state.pads[LAST]);
        assert!(!self.runtime.ownership.admission_current(ID, old));
        assert!(self.runtime.ownership.admission_current(LAST, unaffected));
        self.mapped_trigger(ID, NEW_START, before.revisions[ID] + 1);
        self.mapped_trigger(LAST, LAST_START, before.revisions[LAST]);
        // Republishing the identical full state is a no-op for every actual revision.
        self.candidate(false, || {}).unwrap();
        assert_eq!(self.revisions_at(&before.slots), after.revisions);
    }
}

#[test]
fn early_pending_runtime_replacement_keeps_all_216_rows_and_retries_exactly_once() {
    let mut fixture = Fixture::new();
    let before = fixture.observe();
    let ownership = fixture.runtime.ownership.clone();
    let other = ownership.slot_snapshot(LAST).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(other, other.lifetime())
        .unwrap();
    let mut result = None;
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                result = Some(fixture.candidate(false, || {
                    panic!("early Pending must reject before complete preparation");
                }));
            },
        )
        .unwrap();
    assert!(result.unwrap().is_err());
    fixture.assert_runtime_unchanged(&before);
    fixture.retry_and_assert_exact_publication(&before);
}

#[test]
fn pending_after_all_216_rows_are_prepared_keeps_native_revisions_and_midi_mirror_atomic() {
    let mut fixture = Fixture::new();
    let before = fixture.observe();
    let ownership = fixture.runtime.ownership.clone();
    let other = ownership.slot_snapshot(LAST).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(other, other.lifetime())
        .unwrap();
    let after_preparation = Barrier::new(2);
    let writer_is_odd = Barrier::new(2);
    let release_writer = Barrier::new(2);
    let (result, reached, observed_while_odd) = thread::scope(|scope| {
        let writer = scope.spawn(|| {
            after_preparation.wait();
            ownership.commit_slot_write_fixed(
                &write,
                || true,
                |_| {
                    writer_is_odd.wait();
                    release_writer.wait();
                },
            )
        });
        let reached = Cell::new(false);
        let observed_while_odd = Cell::new(None);
        let result = fixture.candidate(false, || {
            reached.set(true);
            after_preparation.wait();
            writer_is_odd.wait();
            observed_while_odd.set(Some(fixture.revisions_at(&before.slots)));
        });
        // Release/join before assertions, so an assertion cannot strand the real writer.
        release_writer.wait();
        let completed = writer.join().unwrap().unwrap();
        assert_eq!(completed.content_key(), other.content_key());
        assert_eq!(completed.binding_epoch(), other.binding_epoch());
        (result, reached.get(), observed_while_odd.get())
    });
    assert!(reached);
    assert_eq!(observed_while_odd, Some(before.revisions));
    assert!(result.is_err());
    fixture.assert_runtime_unchanged(&before);
    fixture.retry_and_assert_exact_publication(&before);
}

#[derive(Debug, Clone, Copy)]
enum Invalidation {
    Source,
    Window,
    UnloadedMapping,
}

#[test]
fn source_window_and_unloaded_slot_move_after_preparation_reject_the_whole_runtime_state() {
    for invalidation in [
        Invalidation::Source,
        Invalidation::Window,
        Invalidation::UnloadedMapping,
    ] {
        let fixture = Fixture::new();
        if matches!(invalidation, Invalidation::UnloadedMapping) {
            // This source has no RuntimePadState binding: the full 216-row placement guard,
            // rather than only the two loaded MIDI guards, must detect its later Move.
            Fixture::publish_source(&fixture.engine, 36, Fixture::sample(0.75), 9);
        }
        let before = fixture.observe();
        let reached = Cell::new(false);
        let result = fixture.candidate(false, || {
            reached.set(true);
            let ownership = &fixture.runtime.ownership;
            match invalidation {
                Invalidation::Source => {
                    Fixture::publish_source(&fixture.engine, ID, Fixture::sample(-0.75), 10);
                }
                Invalidation::Window => {
                    let source = fixture.engine.sample_cache.lock().unwrap()[ID]
                        .as_ref()
                        .unwrap()
                        .clone();
                    let window = source
                        .window(
                            0,
                            source.frame_count(),
                            source.window_revision() + 1,
                            ResidentContext::FullTrack,
                        )
                        .unwrap();
                    assert!(source.same_source(&window));
                    assert!(!source.same_window(&window));
                    let captured = ownership.slot_snapshot(ID).unwrap();
                    let write = ownership
                        .prepare_slot_write_fixed(captured, captured.lifetime())
                        .unwrap();
                    ownership
                        .commit_slot_write_fixed(
                            &write,
                            || true,
                            |cell| {
                                ownership.publish_window_for_cell(cell, &window);
                            },
                        )
                        .unwrap();
                }
                Invalidation::UnloadedMapping => {
                    let source = ownership.slot_snapshot(36).unwrap();
                    let target = ownership.slot_snapshot(37).unwrap();
                    let pair = ownership.prepare_placement_pair(source, target).unwrap();
                    // Authority-only fixture interleaving; this does not enable a live Pair API.
                    ownership
                        .commit_placement_pair(&pair, true, || true, || {})
                        .unwrap();
                    assert_eq!(
                        ownership.slot_snapshot(37).unwrap().content_key(),
                        source.content_key()
                    );
                }
            }
        });
        assert!(reached.get(), "{invalidation:?}");
        assert!(result.is_err(), "{invalidation:?}");
        fixture.assert_runtime_unchanged(&before);
        match invalidation {
            Invalidation::Source => assert!(
                !fixture
                    .runtime
                    .ownership
                    .content_key_current(before.slots[ID].content_key().unwrap())
            ),
            Invalidation::Window => {
                assert!(
                    fixture
                        .runtime
                        .ownership
                        .content_key_current(before.slots[ID].content_key().unwrap())
                );
                assert!(!fixture.bindings[ID].as_ref().unwrap().is_current());
            }
            Invalidation::UnloadedMapping => {
                assert!(fixture.bindings[ID].as_ref().unwrap().is_current());
                assert!(fixture.bindings[LAST].as_ref().unwrap().is_current());
                assert!(
                    !fixture
                        .runtime
                        .ownership
                        .slot_snapshot_current(before.slots[36])
                );
            }
        }
    }
}

#[test]
fn late_runtime_overflow_preserves_every_earlier_prepared_revision_and_midi_row() {
    let fixture = Fixture::new();
    let last = fixture.runtime.ownership.slot_snapshot(LAST).unwrap();
    fixture
        .runtime
        .ownership
        .runtime_for_cell(last.cell())
        .store(u64::MAX, Ordering::Release);
    let before = fixture.observe();
    assert!(
        fixture
            .candidate(true, || {
                panic!("overflow in the last changed row must fail before the claim seam");
            })
            .is_err()
    );
    fixture.assert_runtime_unchanged(&before);
}

#[test]
fn late_runtime_supersession_after_preparation_cannot_restore_old_values_or_publish_other_rows() {
    let fixture = Fixture::new();
    let mut before = fixture.observe();
    let reached = Cell::new(false);
    let result = fixture.candidate(false, || {
        reached.set(true);
        fixture
            .runtime
            .ownership
            .runtime_for_cell(before.slots[LAST].cell())
            .store(u64::MAX, Ordering::Release);
    });
    assert!(reached.get());
    assert!(result.is_err());
    // The independent supersession belongs to this source; failed publication must neither
    // overwrite it with its captured value nor leak the proposed row-zero increment.
    before.revisions[LAST] = u64::MAX;
    fixture.assert_runtime_unchanged(&before);
}

#[test]
fn real_foreign_engine_binding_rejects_without_revoking_either_runtime() {
    let fixture = Fixture::new();
    let foreign = Fixture::new();
    let before = fixture.observe();
    let foreign_before = foreign.observe();
    let foreign_binding = foreign.bindings[ID].as_ref().unwrap();
    assert!(foreign_binding.is_current());
    assert!(!Arc::ptr_eq(
        &fixture.runtime.ownership,
        &foreign_binding.ownership
    ));
    let mut bindings = fixture.binding_refs();
    bindings[ID] = Some(foreign_binding);
    let inputs = Inputs::new(true, false);
    assert!(
        fixture
            .runtime
            .set_runtime_state_before_commit(
                true,
                inputs.loaded,
                inputs.starts,
                inputs.ends,
                bindings,
                || panic!("an actual foreign Engine binding must not reach complete preparation"),
            )
            .is_err()
    );
    fixture.assert_runtime_unchanged(&before);
    foreign.assert_runtime_unchanged(&foreign_before);
}

#[test]
fn fresh_native_bindings_after_runtime_and_unrelated_source_markers_keep_all_revisions_current() {
    let mut fixture = Fixture::new();
    let before = fixture.observe();
    let admitted = fixture.mapped_trigger(ID, OLD_START, before.revisions[ID]);
    let unaffected = fixture.mapped_trigger(LAST, LAST_START, before.revisions[LAST]);
    let mut last_captured_revision = fixture.bindings[ID]
        .as_ref()
        .unwrap()
        .binding
        .placement
        .unwrap()
        .layout_revision();
    for unrelated_source_write in [false, true] {
        if unrelated_source_write {
            let ownership = &fixture.runtime.ownership;
            let captured = ownership.slot_snapshot(LAST).unwrap();
            let write = ownership
                .prepare_slot_write_fixed(captured, captured.lifetime())
                .unwrap();
            ownership
                .commit_slot_write_fixed(&write, || true, |_| {})
                .unwrap();
        }
        // Real control capture sees the latest complete global revision, even though both
        // sources and every musical runtime field still have precisely the previous values.
        let fresh: Vec<_> = (0..NUM_SAMPLES)
            .map(|id| {
                if id == ID || id == LAST {
                    Some(
                        fixture
                            .engine
                            .current_input_runtime_pad_binding(id)
                            .unwrap()
                            .unwrap(),
                    )
                } else {
                    None
                }
            })
            .collect();
        let placement = fresh[ID].as_ref().unwrap().binding.placement.unwrap();
        assert_ne!(placement.layout_revision(), last_captured_revision);
        assert_eq!(placement.cell(), before.slots[ID].cell());
        assert_eq!(placement.lifetime(), before.slots[ID].lifetime());
        assert_eq!(placement.binding_epoch(), before.slots[ID].binding_epoch());
        last_captured_revision = placement.layout_revision();
        let inputs = Inputs::new(false, false);
        fixture
            .runtime
            .set_runtime_state(
                true,
                inputs.loaded,
                inputs.starts,
                inputs.ends,
                fresh.iter().map(Option::as_ref).collect(),
            )
            .unwrap();
        assert_eq!(fixture.revisions_at(&before.slots), before.revisions);
        assert!(fixture.runtime.ownership.admission_current(ID, admitted));
        assert!(
            fixture
                .runtime
                .ownership
                .admission_current(LAST, unaffected)
        );
        let state = fixture.runtime.runtime_state.lock().unwrap();
        assert!(state.multi_loop);
        assert_eq!(state.pads[ID].loop_start_s, OLD_START);
        assert_eq!(state.pads[LAST].loop_start_s, LAST_START);
        drop(state);
        fixture.mapped_trigger(ID, OLD_START, before.revisions[ID]);
        fixture.mapped_trigger(LAST, LAST_START, before.revisions[LAST]);
    }
}
