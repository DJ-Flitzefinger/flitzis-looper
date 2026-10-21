//! Actual Copy/Config/assets -> public incoming external stem WAV -> callback ACK.
//! The productive Loader observes its own success or error; no fabricated receipt.

use super::*;

fn stem_probe(fixture: &Fixture) -> Py<PyAny> {
    let copy = probe(&fixture.engine, "Probe", Some(fixture_input(fixture)));
    Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!("stem_load_probe.py")).unwrap(),
            c"copied_pad_stem_load_probe.py",
            c"copied_pad_stem_load_probe",
        )
        .unwrap();
        module
            .getattr("StemLoadProbe")
            .unwrap()
            .call1((copy,))
            .unwrap()
            .unbind()
    })
}

fn poll(probe: &Py<PyAny>, method: &str) -> bool {
    Python::attach(|py| probe.call_method0(py, method).unwrap().extract(py).unwrap())
}

fn diagnostics(probe: &Py<PyAny>) -> String {
    Python::attach(|py| {
        probe
            .call_method0(py, "diagnostics")
            .unwrap()
            .extract(py)
            .unwrap()
    })
}

fn wait_for_loader(fixture: &mut Fixture, probe: &Py<PyAny>, method: &str) {
    wait_for_with_diagnostics(
        || {
            fixture.callback.drain();
            fixture.callback.render(64);
            collect(fixture);
            poll(probe, method)
        },
        "copied PAD public Loader did not settle its original Native request",
        || diagnostics(probe),
    );
}

#[test]
fn copied_pad_external_stem_load_retains_success_and_decode_error_keeps_predecessor() {
    isolated(
        "copied_loop_custody_tests::stem_load_tests::copied_pad_external_stem_load_retains_success_and_decode_error_keeps_predecessor",
        || {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(4)
                .unwrap();
            let parent =
                workspace.join("test-artifacts/stem-load-custody-20261021/copy-loader-native");
            fs::create_dir_all(&parent).unwrap();
            fs::write(
                parent.join("OWNER.md"),
                b"Owned synthetic real Copy/Config/assets/public Loader stem-WAV regression. External valid and invalid WAV inputs, original Native callback ACKs; no Human audio/settings/install/device. Root owns serial execution, reviewed evidence and cleanup.\n",
            )
            .unwrap();
            let mut fixture =
                Fixture::with_pair_registration_at(true, true, false, "tap", Some(parent));
            let previous = CurrentDirectory(std::env::current_dir().unwrap());
            std::env::set_current_dir(fixture.directory.path()).unwrap();
            let probe = stem_probe(&fixture);
            let source_before = state(&fixture, 0);
            let source_voice_generation = fixture.source_voice().generation;

            Python::attach(|py| probe.call_method1(py, "begin_copy", (1,)).unwrap());
            assert!(fixture.callback.mixer.bank_for_measurement()[1].is_none());
            assert_eq!(fixture.callback.drain(), 1);
            collect(&fixture);
            Python::attach(|py| probe.call_method1(py, "complete_copy", (1,)).unwrap());
            collect(&fixture);
            let copied_before = state(&fixture, 1);
            assert_eq!(state(&fixture, 0), source_before);

            // Outside samples, a stem WAV is an ordinary incoming source. The
            // public loader must keep the full copied assignment on decode error.
            let external = fixture.directory.path().join("external-stems");
            fs::create_dir(&external).unwrap();
            let invalid = external.join("broken-drums.wav");
            fs::write(&invalid, b"not an audio container").unwrap();
            Python::attach(|py| {
                probe
                    .call_method1(py, "begin_load", (1, invalid.to_string_lossy().as_ref()))
                    .unwrap();
            });
            wait_for_loader(&mut fixture, &probe, "poll_failure");
            assert_eq!(state(&fixture, 1), copied_before);
            assert_eq!(state(&fixture, 0), source_before);
            assert_eq!(fixture.source_voice().generation, source_voice_generation);

            let valid = external.join("drums.wav");
            fs::write(&valid, wav(&[3])).unwrap();
            let request: u64 = Python::attach(|py| {
                probe
                    .call_method1(py, "begin_load", (1, valid.to_string_lossy().as_ref()))
                    .unwrap()
                    .extract(py)
                    .unwrap()
            });
            wait_for_loader(&mut fixture, &probe, "poll_success");
            let replacement = state(&fixture, 1);
            assert_eq!(replacement.generation, request);
            assert_ne!(replacement.assignment, copied_before.assignment);
            assert_ne!(replacement.lifetime, copied_before.lifetime);
            assert_eq!(state(&fixture, 0), source_before);
            assert_eq!(fixture.source_voice().generation, source_voice_generation);
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(1)),
                "an ordinary incoming load started its copied target"
            );

            for _ in 0..8 {
                fixture.callback.drain();
                fixture.callback.render(64);
                collect(&fixture);
                Python::attach(|py| probe.call_method0(py, "assert_retained").unwrap());
                assert_eq!(state(&fixture, 1), replacement);
                assert_eq!(state(&fixture, 0), source_before);
            }
            eprintln!(
                "actual Copy->Config/assets->public Loader external drums.wav retained: request={request}, old_lifetime={}, new_lifetime={}, source_sibling_unchanged=true; preceding real decode error preserved copied predecessor",
                copied_before.lifetime, replacement.lifetime,
            );
            Python::attach(|py| probe.call_method0(py, "close").unwrap());
            drop(probe);
            drop(previous);
        },
    );
}
