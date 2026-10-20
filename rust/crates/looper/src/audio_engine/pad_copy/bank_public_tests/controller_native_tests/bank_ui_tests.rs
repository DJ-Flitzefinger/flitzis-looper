//! Rendered Bank-row/modal IO through the same original controller/Native/Config owners.
//! The Python delegate narrows App polling only; this fixture alone advances audio/ACK.
use super::*;

fn rendered_probe(controller: &Py<PyAny>) -> Py<PyAny> {
    Python::attach(|py| {
        PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/bank_ui_native_probe.py"
            )))
            .unwrap(),
            c"bank_ui_native_probe.py",
            c"bank_ui_native_probe",
        )
        .unwrap()
        .getattr("RenderedBankProbe")
        .unwrap()
        .call1((controller.bind(py),))
        .unwrap()
        .unbind()
    })
}

fn publish_rendered(
    fixture: &mut Fixture,
    controller: &Py<PyAny>,
    rendered: &Py<PyAny>,
    gesture: &str,
) -> Py<BankMutationTicket> {
    let before = config(controller);
    let frame = fixture.callback.frame;
    call(rendered, gesture);
    assert_eq!(fixture.callback.frame, frame);
    assert!(fixture.callback.controls.is_empty());
    assert_eq!(config(controller), before);
    call(controller, "run_worker");
    let ticket = Python::attach(|py| {
        controller
            .call_method0(py, "publish")
            .unwrap()
            .extract::<Py<BankMutationTicket>>(py)
            .unwrap()
    });
    assert_eq!(fixture.callback.frame, frame);
    assert_eq!(fixture.callback.controls.slots(), 1);
    assert!(matches!(
        fixture.callback.controls.peek().unwrap(),
        ControlMessage::BankMutation(_)
    ));
    call(controller, "assert_waiting");
    Python::attach(|py| {
        assert_eq!(ticket.borrow(py).publication_status(), "pending");
        assert!(ticket.borrow(py).acknowledgement().is_none());
    });
    assert_eq!(config(controller), before);
    ticket
}

fn commit_rendered(
    fixture: &mut Fixture,
    controller: &Py<PyAny>,
    ticket: &Py<BankMutationTicket>,
) -> Value {
    let before = config(controller);
    assert_eq!(fixture.callback.drain(), 1);
    assert_eq!(
        config(controller),
        before,
        "only controller settlement after its actual ACK may write Config"
    );
    settle(fixture, controller, ticket)
}

#[test]
fn rendered_bank_gestures_join_original_request_native_ack_and_durable_config() {
    isolated(
        "bank_public_tests::controller_native_tests::bank_ui_tests::rendered_bank_gestures_join_original_request_native_ack_and_durable_config",
        || {
            let mut fixture = bank_fixture_with_stems(true, false, "tap");
            let previous = CurrentDirectory(std::env::current_dir().unwrap());
            std::env::set_current_dir(fixture.directory.path()).unwrap();
            let controller = controller(&mut fixture, false);
            seed(&mut fixture, &controller, 72);
            fixture.start(72);
            session_effect(&controller, 0, false);
            session_effect(&controller, 72, false);
            let rendered = rendered_probe(&controller);
            let source = proof(&fixture, 0);
            let outside = proof(&fixture, 72);
            let original = model_identity(&controller, 0).unwrap();
            let mut owners = std::collections::HashSet::new();

            for gesture in ["empty_copy", "occupied_copy"] {
                let ticket = publish_rendered(&mut fixture, &controller, &rendered, gesture);
                assert_eq!(proof(&fixture, 0), source);
                assert_eq!(proof(&fixture, 72), outside);
                let receipt = commit_rendered(&mut fixture, &controller, &ticket);
                assert_eq!(proof(&fixture, 0), source);
                assert_eq!(proof(&fixture, 72), outside);
                assert!(owners.insert(receipt["owner"].as_str().unwrap().to_owned()));
                assert_eq!(model_parent(&controller, 36), Some(original.clone()));
                assert_ne!(model_identity(&controller, 36), Some(original.clone()));
                assert!(fixture.callback.mixer.bank_for_measurement()[36].is_some());
                assert!(
                    fixture.callback.mixer.bank_for_measurement()[37..72]
                        .iter()
                        .all(Option::is_none)
                );
                assert!(
                    !fixture
                        .callback
                        .mixer
                        .voices
                        .iter()
                        .any(|voice| (36..72).contains(&voice.sample_id)
                            && (voice.active || voice.paused))
                );
                drop(ticket);
            }

            let clear = publish_rendered(&mut fixture, &controller, &rendered, "clear_other");
            let receipt = commit_rendered(&mut fixture, &controller, &clear);
            assert!(owners.insert(receipt["owner"].as_str().unwrap().to_owned()));
            assert!(
                fixture.callback.mixer.bank_for_measurement()[36..72]
                    .iter()
                    .all(Option::is_none)
            );
            assert_eq!(proof(&fixture, 0), source);
            assert_eq!(proof(&fixture, 72), outside);
            drop(clear);

            let clear = publish_rendered(&mut fixture, &controller, &rendered, "clear_current");
            let receipt = commit_rendered(&mut fixture, &controller, &clear);
            assert!(owners.insert(receipt["owner"].as_str().unwrap().to_owned()));
            assert!(
                fixture.callback.mixer.bank_for_measurement()[0..36]
                    .iter()
                    .all(Option::is_none)
            );
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.sample_id < 36 && (voice.active || voice.paused))
            );
            assert_eq!(proof(&fixture, 72), outside);
            assert_eq!(owners.len(), 4);
            drop(clear);
            call(&rendered, "close");
            drop(rendered);
            close(controller);
            drop(previous);
        },
    );
}
