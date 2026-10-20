//! Productive Python Copy/Config/assets followed by the real Python loop controller.
//! Scalar/Weak-only observations do not add a Native assignment or PCM owner.

use super::*;
use pyo3::types::PyDict;

struct ControllerBaseline {
    original_voice_generation: u64,
    identities: Value,
}

fn status(transport: &Py<PyAny>, slot: usize) -> (Option<String>, Option<String>) {
    Python::attach(|py| {
        transport
            .bind(py)
            .getattr("residency")
            .unwrap()
            .call_method1("status", (slot,))
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn saved(fixture: &Fixture) -> Value {
    serde_json::from_slice(
        &fs::read(fixture.directory.path().join("samples/copy-public.json")).unwrap(),
    )
    .unwrap()
}

fn assert_voice_independence(fixture: &Fixture, original_generation: u64) {
    assert_eq!(fixture.source_voice().generation, original_generation);
    for slot in [1, 2] {
        assert!(
            !fixture
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(slot)),
            "a loop edit started copied slot {slot}"
        );
    }
}

fn edit_from_controller(
    fixture: &mut Fixture,
    probe: &Py<PyAny>,
    transport: &Py<PyAny>,
    expected: &mut [SlotState; 3],
    slot: usize,
    start: usize,
    baseline: &ControllerBaseline,
) {
    assert_eq!(states(fixture), *expected);
    let before_regions = SLOTS.map(|id| fixture.callback.mixer.loop_region_frames(id));
    collect(fixture);
    let revision: u64 = Python::attach(|py| {
        let controller = transport.bind(py);
        controller
            .getattr("loop")
            .unwrap()
            .call_method1("set_start", (slot, start as f64 / f64::from(RATE)))
            .unwrap();
        // Read this real controller's own admitted ticket without retaining an
        // extra ticket/PCM owner after the controller observes its Native ACK.
        let residency = controller.getattr("residency").unwrap();
        let (pending_status, error): (Option<String>, Option<String>) = residency
            .call_method1("status", (slot,))
            .unwrap()
            .extract()
            .unwrap();
        assert!(
            error.is_none(),
            "slot {slot} real loop admission: {error:?}"
        );
        assert!(
            pending_status.is_some(),
            "slot {slot} did not retain its own request"
        );
        let pending = residency
            .getattr("_pending")
            .unwrap()
            .get_item(slot)
            .unwrap();
        assert_eq!(
            pending
                .getattr("expected_source_generation")
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            expected[slot].generation
        );
        pending
            .getattr("ticket")
            .unwrap()
            .getattr("window_revision")
            .unwrap()
            .extract()
            .unwrap()
    });
    let (pending, error) = status(transport, slot);
    assert!(
        pending.is_some(),
        "loop edit did not retain its own real request"
    );
    assert!(
        error.is_none(),
        "slot {slot} initial loop admission: {error:?}"
    );
    assert_eq!(
        SLOTS.map(|id| fixture.callback.mixer.loop_region_frames(id)),
        before_regions,
        "unACKed requested fields were presented as effective Native loops"
    );
    collect(fixture);
    wait_for_with_diagnostics(
        || {
            collect(fixture);
            fixture.callback.drain();
            collect(fixture);
            fixture.callback.render(64);
            fixture.with_engine(|engine| resident_relocation::reconcile(engine).unwrap());
            collect(fixture);
            Python::attach(|py| {
                transport
                    .bind(py)
                    .getattr("residency")
                    .unwrap()
                    .call_method0("poll")
                    .unwrap();
            });
            collect(fixture);
            let (pending, error) = status(transport, slot);
            assert!(
                error.is_none(),
                "slot {slot} real loop preparation: {error:?}"
            );
            pending.is_none()
        },
        "productive Python loop edit did not observe its own Native ACK",
        || format!("slot={slot} status={:?}", status(transport, slot)),
    );
    expected[slot].window.0 = start;
    expected[slot].loop_region.0 = start;
    assert_eq!(states(fixture), *expected);
    fixture.with_engine(|engine| {
        let cache = engine.sample_cache.lock().unwrap();
        assert_eq!(
            cache[slot].as_ref().unwrap().window_revision(),
            revision,
            "controller completed against a different Native window"
        );
    });
    assert_voice_independence(fixture, baseline.original_voice_generation);
    Python::attach(|py| {
        probe
            .bind(py)
            .getattr("persistence")
            .unwrap()
            .call_method0("flush")
            .unwrap();
    });
    let committed = saved(fixture);
    assert_eq!(&committed["pad_content"], &baseline.identities);
    for id in SLOTS {
        assert_eq!(
            committed["pad_loop_start_s"][id].as_f64(),
            Some(expected[id].loop_region.0 as f64 / f64::from(RATE)),
            "slot {slot} loop edit did not durably preserve slot {id} intent"
        );
        assert_eq!(
            committed["pad_loop_end_s"][id].as_f64(),
            Some(END as f64 / f64::from(RATE))
        );
        assert_eq!(committed["pad_timing_intent"][id], "tap");
        assert_eq!(committed["pad_loop_auto"][id], false);
    }
    collect(fixture);
}

#[test]
fn productive_python_copy_commit_preserves_independent_loop_reader_custody_after_collection() {
    isolated(
        "copied_loop_custody_tests::controller_tests::productive_python_copy_commit_preserves_independent_loop_reader_custody_after_collection",
        || {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(4)
                .unwrap();
            let parent = workspace.join(
                "test-artifacts/rearrange-human-fixes-20261021/copied-loop-controller-native",
            );
            fs::create_dir_all(&parent).unwrap();
            fs::write(
                parent.join("OWNER.md"),
                b"Owned synthetic productive Python Copy/Config/assets/loop-controller regression. TAP profile, original Native ACKs, no Human audio/settings/install/device. Root owns execution, review and cleanup.\n",
            )
            .unwrap();
            let mut fixture =
                Fixture::with_pair_registration_at(true, true, false, "tap", Some(parent));
            let previous = CurrentDirectory(std::env::current_dir().unwrap());
            std::env::set_current_dir(fixture.directory.path()).unwrap();
            settle_window(&mut fixture, 0, START, END);
            let probe = probe(&fixture.engine, "Probe", Some(fixture_input(&fixture)));
            // The shared Probe's +8 saved pitch requires Native geometry that
            // this deliberately neutral OFF/FiniteLoop source has never prepared.
            // Its original rejected run is retained in composed-tests.log.
            // Align only this test instance's numeric metadata with the real
            // source; preserve source key/correction/epochs/retrigger verbatim.
            assert_eq!(
                fixture
                    .callback
                    .mixer
                    .pad_pitch_intent(0)
                    .unwrap()
                    .total_semitones(),
                0
            );
            fixture.with_engine(|engine| {
                let cache = engine.sample_cache.lock().unwrap();
                let sample = cache[0].as_ref().unwrap();
                assert_eq!(
                    sample.residency.as_ref().unwrap().context,
                    crate::messages::ResidentContext::FiniteLoop
                );
                assert!(!RtMixer::initial_key_lock_geometry_available(sample));
            });
            Python::attach(|py| {
                let owned = probe.bind(py);
                let keys = owned
                    .getattr("project")
                    .unwrap()
                    .getattr("pad_key_intent")
                    .unwrap();
                let before = keys.get_item(0).unwrap();
                let before_json: String = before
                    .call_method0("model_dump_json")
                    .unwrap()
                    .extract()
                    .unwrap();
                let mut before_fields: Value = serde_json::from_str(&before_json).unwrap();
                assert_eq!(before_fields["base_shift"], -4);
                assert_eq!(before_fields["extra_shift"], 12);
                eprintln!(
                    "controller fixture baseline correction: saved pitch -4+12=8 differs from actual neutral Native source; existing FiniteLoop has no pitch geometry. Preserve metadata/retrigger, change only numeric shifts to 0+0; original rejection retained in composed-tests.log"
                );
                let changes = PyDict::new(py);
                changes.set_item("base_shift", 0).unwrap();
                changes.set_item("extra_shift", 0).unwrap();
                let neutral = before.call_method("changed", (), Some(&changes)).unwrap();
                let after_json: String = neutral
                    .call_method0("model_dump_json")
                    .unwrap()
                    .extract()
                    .unwrap();
                let after_fields: Value = serde_json::from_str(&after_json).unwrap();
                before_fields["base_shift"] = json!(0);
                before_fields["extra_shift"] = json!(0);
                assert_eq!(after_fields, before_fields);
                keys.set_item(0, neutral).unwrap();
                let persistence = owned.getattr("persistence").unwrap();
                persistence.call_method0("mark_dirty").unwrap();
                persistence.call_method0("flush").unwrap();
            });
            assert_eq!(saved(&fixture)["pad_key_intent"][0]["base_shift"], 0);
            assert_eq!(saved(&fixture)["pad_key_intent"][0]["extra_shift"], 0);
            let original = state(&fixture, 0);
            let original_voice_generation = fixture.source_voice().generation;
            for target in [1, 2] {
                Python::attach(|py| probe.call_method1(py, "begin", (target,)).unwrap());
                collect(&fixture);
                assert_eq!(state(&fixture, 0), original);
                assert!(fixture.callback.mixer.bank_for_measurement()[target].is_none());
                let ControlMessage::PadCopy(work) = fixture.callback.controls.peek().unwrap()
                else {
                    panic!("productive Copy did not enqueue its own Native payload");
                };
                let native_geometry = RtMixer::initial_key_lock_geometry_available(&work.sample);
                eprintln!(
                    "productive finite Copy before callback: target={} source_pitch={} payload_pitch={} key_lock={} extent={}..{} context={:?} native_geometry={} geometry_required={} guard_current={} source_generation={} target_generation={} source_epoch={}/{} target_epoch={}/{}",
                    target,
                    fixture
                        .callback
                        .mixer
                        .pad_pitch_intent(0)
                        .unwrap()
                        .total_semitones(),
                    work.intent.pitch_intent.total_semitones(),
                    work.intent.key_lock,
                    work.sample.resident_start(),
                    work.sample.resident_end(),
                    work.sample.residency.as_ref().unwrap().context,
                    native_geometry,
                    work.intent.key_lock || work.intent.pitch_intent.total_semitones() != 0,
                    work.guard.current(),
                    work.guard.source_generation,
                    work.source_generation,
                    work.guard.source_epoch.load(Ordering::Acquire),
                    work.guard.captured_source_epoch,
                    work.guard.target_epoch.load(Ordering::Acquire),
                    work.guard.captured_target_epoch,
                );
                assert_eq!(work.intent.pitch_intent.total_semitones(), 0);
                assert!(work.guard.current());
                assert_eq!(fixture.callback.drain(), 1);
                collect(&fixture);
                Python::attach(|py| {
                    let receipt: String = probe
                        .bind(py)
                        .getattr("arrangement")
                        .unwrap()
                        .getattr("_copy")
                        .unwrap()
                        .getattr("ticket")
                        .unwrap()
                        .call_method0("metadata_json")
                        .unwrap()
                        .extract()
                        .unwrap();
                    eprintln!("productive finite Copy after callback: {receipt}");
                });
                // Existing productive Probe requires the actual accepted,
                // control-mirrored receipt, durable Config and released fences.
                Python::attach(|py| probe.call_method1(py, "complete", (target,)).unwrap());
                collect(&fixture);
                assert_eq!(state(&fixture, 0), original);
                assert_eq!(
                    fixture.with_engine(|engine| engine
                        .input_runtime_ownership
                        .copy_reservation(target)),
                    0
                );
                assert_voice_independence(&fixture, original_voice_generation);
            }
            let mut expected = states(&fixture);
            for first in 0..SLOTS.len() {
                for second in first + 1..SLOTS.len() {
                    assert_ne!(expected[first].assignment, expected[second].assignment);
                    assert_ne!(expected[first].lifetime, expected[second].lifetime);
                }
            }
            let original_finite = fixture.with_engine(|engine| {
                let cache = engine.sample_cache.lock().unwrap();
                let source = cache[0].as_ref().unwrap();
                for target in [1, 2] {
                    let copied = cache[target].as_ref().unwrap();
                    assert!(Arc::ptr_eq(&source.samples, &copied.samples));
                    assert!(!Arc::ptr_eq(
                        source.residency.as_ref().unwrap(),
                        copied.residency.as_ref().unwrap()
                    ));
                }
                Arc::downgrade(&source.samples)
            });
            let baseline = ControllerBaseline {
                original_voice_generation,
                identities: saved(&fixture)["pad_content"].clone(),
            };
            let transport = Python::attach(|py| {
                let owned = probe.bind(py);
                py.import("flitzis_looper.controller.transport")
                    .unwrap()
                    .getattr("TransportController")
                    .unwrap()
                    .call1((
                        owned.getattr("project").unwrap(),
                        owned.getattr("session").unwrap(),
                        fixture.engine.clone_ref(py),
                        owned
                            .getattr("persistence")
                            .unwrap()
                            .getattr("mark_dirty")
                            .unwrap(),
                    ))
                    .unwrap()
                    .unbind()
            });
            // No apply_project_state_to_audio or helper materialization can
            // repair the real copied assignment before Human's first edit.
            for (slot, start) in [(1, START + 64), (2, START + 128), (0, START + 192)] {
                edit_from_controller(
                    &mut fixture,
                    &probe,
                    &transport,
                    &mut expected,
                    slot,
                    start,
                    &baseline,
                );
            }
            wait_for_with_diagnostics(
                || {
                    fixture.callback.drain();
                    fixture.callback.render(64);
                    collect(&fixture);
                    original_finite.strong_count() == 0
                },
                "productive copies retained replaced finite PCM",
                || {
                    format!(
                        "old finite strong readers={}",
                        original_finite.strong_count()
                    )
                },
            );
            collect(&fixture);
            materialize_current_readers(&fixture, &expected);
            for (slot, start) in [(1, START + 256), (2, START + 320), (0, START + 384)] {
                edit_from_controller(
                    &mut fixture,
                    &probe,
                    &transport,
                    &mut expected,
                    slot,
                    start,
                    &baseline,
                );
                materialize_current_readers(&fixture, &expected);
            }
            Python::attach(|py| {
                for component in ["pitch", "residency"] {
                    transport
                        .bind(py)
                        .getattr(component)
                        .unwrap()
                        .call_method0("shut_down")
                        .unwrap();
                }
            });
            drop(transport);
            Python::attach(|py| {
                py.import("gc").unwrap().call_method0("collect").unwrap();
                probe.call_method0(py, "close").unwrap();
            });
            drop(probe);
            drop(previous);
        },
    );
}
