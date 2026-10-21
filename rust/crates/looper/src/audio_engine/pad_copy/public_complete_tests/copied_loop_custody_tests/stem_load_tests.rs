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

#[test]
fn managed_stem_import_captures_independent_copy_and_empty_pad_originals() {
    isolated(
        "copied_loop_custody_tests::stem_load_tests::managed_stem_import_captures_independent_copy_and_empty_pad_originals",
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
                b"Owned synthetic real Copy/Config/assets/public Loader stem-WAV regressions. Managed and external valid/invalid inputs, original Native callback ACKs and last-reader retirement; no Human audio/settings/install/device. Root owns serial execution, reviewed evidence and cleanup.\n",
            )
            .unwrap();
            let mut fixture =
                Fixture::with_pair_registration_at(true, true, false, "tap", Some(parent));
            let previous = CurrentDirectory(std::env::current_dir().unwrap());
            std::env::set_current_dir(fixture.directory.path()).unwrap();
            let probe = stem_probe(&fixture);
            let source_before = state(&fixture, 0);
            let source_voice_generation = fixture.source_voice().generation;
            let generation = fixture
                .directory
                .path()
                .join(fixture.pair_json["wav_generation"].as_str().unwrap());
            let stem = generation.join("drums.wav");
            let pcm = fixture
                .directory
                .path()
                .join(fixture.pair_json["pcm_generation"].as_str().unwrap())
                .join("drums.f32le");

            Python::attach(|py| probe.call_method1(py, "begin_copy", (1,)).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            collect(&fixture);
            Python::attach(|py| probe.call_method1(py, "complete_copy", (1,)).unwrap());
            let copied_before = state(&fixture, 1);

            // The new input exception must leave every other managed-artifact
            // guard and the existing auto/restore meaning intact.
            let unknown = generation.parent().unwrap().join("unknown.wav");
            fs::write(&unknown, wav(&[3])).unwrap();
            let map = generation.parent().unwrap().parent().unwrap().join(
                ".pcm-cache/source-beatmaps/v1/.ready-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/map.json",
            );
            for (path, intent) in [
                (generation.join(".complete.json"), "import"),
                (pcm, "import"),
                (map, "import"),
                (unknown, "import"),
                (stem.clone(), "auto"),
                (stem.clone(), "restore"),
            ] {
                Python::attach(|py| {
                    probe
                        .call_method1(
                            py,
                            "assert_admission_rejected",
                            (1, path.to_string_lossy().as_ref(), intent),
                        )
                        .unwrap();
                });
                assert_eq!(state(&fixture, 1), copied_before);
                assert_eq!(state(&fixture, 0), source_before);
            }

            // A declared, but undecodable, project WAV still follows the genuine
            // background Error path while the complete copied row stays intact.
            let invalid_generation = generation
                .parent()
                .unwrap()
                .join(".ready-ffffffffffffffffffffffffffffffff");
            fs::create_dir(&invalid_generation).unwrap();
            let invalid = invalid_generation.join("bass.wav");
            fs::write(&invalid, b"not an audio container").unwrap();
            Python::attach(|py| {
                probe
                    .call_method1(
                        py,
                        "begin_managed_load",
                        (1, invalid.to_string_lossy().as_ref()),
                    )
                    .unwrap();
            });
            wait_for_loader(&mut fixture, &probe, "poll_failure");
            assert_eq!(state(&fixture, 1), copied_before);
            assert_eq!(state(&fixture, 0), source_before);

            let mut imported = Vec::new();
            for target in [1, 2] {
                if target == 2 {
                    assert!(fixture.callback.mixer.bank_for_measurement()[target].is_none());
                    assert!(fixture.with_engine(|engine| {
                        engine
                            .current_input_runtime_pad_binding(target)
                            .unwrap()
                            .is_none()
                    }));
                }
                let request: u64 = Python::attach(|py| {
                    probe
                        .call_method1(
                            py,
                            "begin_managed_load",
                            (target, stem.to_string_lossy().as_ref()),
                        )
                        .unwrap()
                        .extract(py)
                        .unwrap()
                });
                wait_for_loader(&mut fixture, &probe, "poll_success");
                let replacement = state(&fixture, target);
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
                        .any(|voice| voice.is_playing_sample(target))
                );
                for _ in 0..8 {
                    fixture.callback.drain();
                    fixture.callback.render(64);
                    collect(&fixture);
                    Python::attach(|py| probe.call_method0(py, "assert_retained").unwrap());
                    assert_eq!(state(&fixture, target), replacement);
                    assert_eq!(state(&fixture, 0), source_before);
                }
                imported.push(replacement);
            }
            assert_ne!(imported[0].assignment, imported[1].assignment);
            assert_ne!(imported[0].lifetime, imported[1].lifetime);

            // End the actual last old selection/voice/PreparedStemPair owners.
            // The imported originals remain independent even after the input WAV
            // and its complete generation physically disappear off-thread.
            Python::attach(|py| probe.call_method0(py, "retire_input_source").unwrap());
            fixture.callback.drain();
            fixture.pair = None;
            wait_for_with_diagnostics(
                || {
                    fixture.callback.drain();
                    fixture.callback.render(64);
                    collect(&fixture);
                    Python::attach(|py| {
                        probe.call_method0(py, "assert_imports_retained").unwrap();
                    });
                    !generation.exists()
                },
                "managed input generation did not retire after its last real reader",
                || diagnostics(&probe),
            );
            assert!(!stem.exists());
            assert_eq!(state(&fixture, 1), imported[0]);
            assert_eq!(state(&fixture, 2), imported[1]);
            eprintln!(
                "actual managed drums.wav import retained independent Copy and empty PAD originals after input generation retirement: assignments={:?}, lifetimes={:?}",
                imported
                    .iter()
                    .map(|state| state.assignment)
                    .collect::<Vec<_>>(),
                imported
                    .iter()
                    .map(|state| state.lifetime)
                    .collect::<Vec<_>>(),
            );
            Python::attach(|py| probe.call_method0(py, "close").unwrap());
            drop(probe);
            drop(previous);
        },
    );
}
