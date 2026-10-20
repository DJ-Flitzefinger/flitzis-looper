//! Real automatic InputMapping refresh must preserve original Bank participant revisions.
//! Outside pitch rows and a newly published MIDI mapping remain usable under the same fence.
use super::*;
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};

fn input_probe(controller: &Py<PyAny>) -> Py<PyAny> {
    Python::attach(|py| {
        PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/bank_input_runtime_probe.py"
            )))
            .unwrap(),
            c"bank_input_runtime_probe.py",
            c"bank_input_runtime_probe",
        )
        .unwrap()
        .getattr("BankInputRuntimeProbe")
        .unwrap()
        .call1((controller.bind(py),))
        .unwrap()
        .unbind()
    })
}

fn clock(fixture: &Fixture, clock: &SharedOutputClock) {
    fixture.with_engine(|engine| {
        let now = engine.input_clock.capture_ns();
        clock.publish(OutputClockSnapshot {
            valid: true,
            observed_at_ns: now,
            audible_at_ns: now,
            output_frame: fixture.callback.frame,
            sample_rate_hz: RATE,
            master_period_seconds: Some(0.5),
            downbeat_frame: 0,
            master_beat: fixture.callback.frame as f64 / 24_000.0,
            freshness_ns: 2_000_000_000,
        });
    });
}

fn revisions(fixture: &Fixture) -> Vec<u64> {
    fixture.with_engine(|engine| {
        (0..72)
            .map(|slot| engine.input_runtime_ownership.runtime_revision(slot))
            .collect()
    })
}

fn install_input_runtime(fixture: &Fixture) -> Arc<SharedOutputClock> {
    let output_clock = Arc::new(SharedOutputClock::new());
    fixture.with_engine(|engine| {
        let runtime = InputRuntime::new_with_ownership(
            fixture.producer.clone(),
            engine.input_clock,
            engine.input_runtime_ownership.clone(),
        );
        runtime.attach_pitch_clock(output_clock.clone()).unwrap();
        runtime.set_render_format(RATE, 2).unwrap();
        runtime.set_enabled(true);
        engine.input_runtime = Some(runtime);
    });
    clock(fixture, &output_clock);
    output_clock
}

fn publish(controller: &Py<PyAny>) -> Py<BankMutationTicket> {
    call(controller, "run_worker");
    Python::attach(|py| {
        controller
            .call_method0(py, "publish")
            .unwrap()
            .extract::<Py<BankMutationTicket>>(py)
            .unwrap()
    })
}

fn dispatch_outside_mapping(fixture: &Fixture) {
    fixture.with_engine(|engine| {
        let runtime = engine.input_runtime.as_ref().unwrap();
        let dispatch = runtime
            .capture_mapping_dispatch_for_test("midi:note:1:60", engine.input_clock.capture_ns())
            .expect("actual saved outside mapping must remain available");
        assert!(
            dispatch(),
            "outside Stop must dispatch while Bank remains reserved"
        );
    });
}

#[test]
fn bank_input_runtime_refresh_keeps_original_participants_and_outside_controls() {
    isolated(
        "bank_public_tests::controller_native_tests::input_runtime_tests::bank_input_runtime_refresh_keeps_original_participants_and_outside_controls",
        || {
            let mut fixture = bank_fixture_with_stems(true, false, "tap");
            let previous = CurrentDirectory(std::env::current_dir().unwrap());
            std::env::set_current_dir(fixture.directory.path()).unwrap();
            let controller = controller(&mut fixture, false);
            seed(&mut fixture, &controller, 72);
            let output_clock = install_input_runtime(&fixture);
            let input = input_probe(&controller);
            assert!(revisions(&fixture)[0] > 0);
            let source = proof(&fixture, 0);
            call(&input, "begin");
            let original = revisions(&fixture);
            call(&input, "refresh");
            assert_eq!(
                revisions(&fixture),
                original,
                "an automatic frame must not turn reserved Bank participants into a new unloaded runtime image"
            );
            clock(&fixture, &output_clock);
            call(&input, "outside_controls");
            dispatch_outside_mapping(&fixture);
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(revisions(&fixture), original);
            assert_eq!(proof(&fixture, 0), source);
            let ticket = publish(&controller);
            call(&input, "refresh");
            assert_eq!(revisions(&fixture), original);
            call(&controller, "assert_waiting");
            let before = config(&controller);
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(config(&controller), before);
            assert_eq!(proof(&fixture, 0), source);
            settle(&fixture, &controller, &ticket);
            call(&input, "assert_restored_runtime");
            fixture.with_engine(|engine| {
                let runtime = engine.input_runtime.as_ref().unwrap();
                assert!(runtime.pair_binding_for_test(36).is_some());
                assert!(runtime.pair_binding_for_test(72).is_some());
            });
            drop(ticket);
            call(&input, "close");
            drop(input);
            close(controller);
            drop(previous);
        },
    );
}

#[test]
fn bank_input_runtime_refresh_explicit_multi_loop_supersedes_then_fresh_clear_drops_cache() {
    isolated(
        "bank_public_tests::controller_native_tests::input_runtime_tests::bank_input_runtime_refresh_explicit_multi_loop_supersedes_then_fresh_clear_drops_cache",
        || {
            let mut fixture = bank_fixture_with_stems(true, false, "tap");
            let previous = CurrentDirectory(std::env::current_dir().unwrap());
            std::env::set_current_dir(fixture.directory.path()).unwrap();
            let controller = controller(&mut fixture, false);
            for target in [36, 72] {
                seed(&mut fixture, &controller, target);
            }
            let output_clock = install_input_runtime(&fixture);
            let input = input_probe(&controller);
            let source = proof(&fixture, 0);
            let original_target = model_identity(&controller, 36);
            call(&input, "begin");
            let ticket = publish(&controller);
            let original = revisions(&fixture);
            let before = config(&controller);
            call(&input, "supersede_multi_loop");
            assert_ne!(
                revisions(&fixture),
                original,
                "a deliberate global MultiLoop change must still supersede captured runtime authority"
            );
            assert_eq!(fixture.callback.drain(), 1);
            Python::attach(|py| {
                assert_eq!(ticket.borrow(py).publication_status(), "rejected");
                assert!(ticket.borrow(py).acknowledgement().is_none());
            });
            assert_eq!(config(&controller), before);
            assert_eq!(proof(&fixture, 0), source);
            assert_eq!(model_identity(&controller, 36), original_target);
            call(&input, "settle_superseded");
            assert_eq!(model_identity(&controller, 36), original_target);
            drop(ticket);

            call(&input, "begin_clear");
            let original = revisions(&fixture);
            call(&input, "refresh");
            assert_eq!(revisions(&fixture), original);
            let clear = publish(&controller);
            call(&input, "refresh");
            assert_eq!(revisions(&fixture), original);
            call(&controller, "assert_waiting");
            let before = config(&controller);
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(config(&controller), before);
            assert_eq!(proof(&fixture, 0), source);
            settle(&fixture, &controller, &clear);
            call(&input, "assert_cleared_runtime");
            fixture.with_engine(|engine| {
                let runtime = engine.input_runtime.as_ref().unwrap();
                assert!(runtime.pair_binding_for_test(36).is_none());
                assert!(runtime.pair_binding_for_test(72).is_some());
            });
            clock(&fixture, &output_clock);
            call(&input, "outside_controls");
            dispatch_outside_mapping(&fixture);
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(proof(&fixture, 0), source);
            drop(clear);
            call(&input, "close");
            drop(input);
            close(controller);
            drop(previous);
        },
    );
}
