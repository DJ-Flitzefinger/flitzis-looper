//! Genuine acknowledged QM -> productive Automatic Copies -> real Python loop edits.
//! FullMix, neutral pitch, Key Lock OFF; no device or Human acceptance.

use super::*;
use crate::audio_engine::native_owned_work::CompleteSampleWork;
use crate::audio_engine::resident_relocation::{self, ResidentWindowTicket};

const SLOTS: [usize; 3] = [0, 1, 2];
const TEST: &str = "automatic_copies_keep_exact_reader_custody_through_productive_loop_edits";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SlotState {
    assignment: u64,
    generation: u64,
    lifetime: u64,
    frames: usize,
    channels: usize,
    revision: u64,
    window: (usize, usize),
    loop_region: (usize, Option<usize>),
}

struct Baseline {
    timing: [Value; 3],
    identities: Value,
    accepted: Value,
    canonical: String,
}

fn isolated(body: impl FnOnce()) {
    if std::env::var_os(CHILD).is_some() {
        body();
        eprintln!(
            "actual Automatic copied-loop child PID={}",
            std::process::id()
        );
        return;
    }
    // The ancestor's launcher deliberately names its original restore test.
    // Launch this distinct composition by its own exact child name instead.
    let (_, modules) = module_path!().split_once("::").unwrap();
    let name = format!("{modules}::{TEST}");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &name, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Automatic copied-loop child failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "child did not execute:\n{stdout}\n{stderr}"
    );
    assert!(
        stderr.contains("actual Automatic copied-loop child PID="),
        "profile skipped:\n{stdout}\n{stderr}"
    );
}

fn wait_for(
    mut condition: impl FnMut() -> bool,
    description: &str,
    diagnostics: impl Fn() -> String,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "{description}: {}",
            diagnostics()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn collect(harness: &Harness) {
    harness
        .with_engine(|engine| engine.project_assets.clone())
        .collect_for_test();
}

fn state(harness: &Harness, slot: usize) -> SlotState {
    harness.with_engine(|engine| {
        let binding = engine
            .current_input_runtime_pad_binding(slot)
            .unwrap()
            .unwrap();
        assert!(
            binding.is_current() && binding.available(),
            "slot {slot} lost genuine Automatic authority"
        );
        let lifetime = binding
            .binding
            .placement
            .unwrap()
            .content_key()
            .unwrap()
            .lifetime();
        let leases = engine.cold_leases.lock().unwrap();
        let lease = leases[slot].as_ref().unwrap();
        assert!(
            !lease.assignment_retired(),
            "slot {slot} actual assignment retired"
        );
        let assignment = lease.assignment_id();
        drop(leases);
        assert_eq!(
            engine.cold_lease_generations[slot].load(Ordering::Acquire),
            binding.source_generation
        );
        let cache = engine.sample_cache.lock().unwrap();
        let sample = cache[slot].as_ref().unwrap();
        SlotState {
            assignment,
            generation: binding.source_generation,
            lifetime,
            frames: sample.frame_count(),
            channels: sample.channels,
            revision: sample.window_revision(),
            window: (sample.resident_start(), sample.resident_end()),
            loop_region: harness.callback.mixer.loop_region_frames(slot),
        }
    })
}

fn states(harness: &Harness) -> [SlotState; 3] {
    SLOTS.map(|slot| state(harness, slot))
}

fn initial_finite_window(harness: &mut Harness) {
    let generation = harness.with_engine(|engine| {
        let binding = engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .unwrap();
        assert!(binding.is_current() && binding.available());
        binding.source_generation
    });
    let ticket: ResidentWindowTicket = harness.with_engine(|engine| {
        engine
            .prepare_resident_control_with_source(
                0,
                Some(START as f64 / f64::from(harness.rate)),
                Some(END as f64 / f64::from(harness.rate)),
                None,
                Some(false),
                Some(generation),
            )
            .unwrap()
    });
    collect(harness);
    wait_for(
        || {
            collect(harness);
            harness.callback.drain();
            collect(harness);
            harness.callback.render(17);
            harness.with_engine(|engine| resident_relocation::reconcile(engine).unwrap());
            collect(harness);
            match ticket.publication_status() {
                "accepted" => true,
                "preparing" | "pending" | "adopting" => false,
                status => panic!(
                    "genuine Automatic source finite window failed: {status}: {:?}",
                    ticket.error()
                ),
            }
        },
        "genuine Automatic finite source did not obtain its own window ACK",
        || format!("{}: {:?}", ticket.publication_status(), ticket.error()),
    );
    assert!(ticket.is_current());
    assert!(ticket.error().unwrap().is_none());
    let actual = state(harness, 0);
    assert_eq!(actual.generation, generation);
    assert_eq!(actual.window, (START, END));
    assert!(actual.frames > END);
    assert_eq!(actual.loop_region, (START, Some(END)));
    drop(ticket);
    collect(harness);
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

fn saved(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join("qm-copy.json")).unwrap()).unwrap()
}

fn assert_timing(harness: &Harness, baseline: &Baseline) {
    for slot in SLOTS {
        assert_eq!(
            harness.current(slot).unwrap(),
            baseline.timing[slot],
            "slot {slot} timing authority changed during independent loop edits"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&harness.export(slot, &baseline.canonical).unwrap())
                .unwrap(),
            baseline.accepted
        );
        assert!(!harness.callback.mixer.key_lock_for_measurement(slot));
        assert_eq!(
            harness
                .callback
                .mixer
                .pad_pitch_intent(slot)
                .unwrap()
                .total_semitones(),
            0
        );
    }
    assert!(
        harness
            .callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.active && !voice.paused),
        "a loop edit launched a stopped source/copy"
    );
}

fn materialize_current(harness: &Harness, expected: &[SlotState; 3]) {
    for slot in SLOTS {
        {
            let work = harness.with_engine(|engine| {
                let cache = engine.sample_cache.lock().unwrap();
                CompleteSampleWork::capture(engine, slot, cache[slot].as_ref().unwrap()).unwrap()
            });
            let complete = work
                .materialize(crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES)
                .unwrap_or_else(|error| {
                    panic!("Automatic slot {slot} exact-assignment complete reader: {error}")
                });
            assert_eq!(
                (complete.resident_start(), complete.resident_end()),
                (0, expected[slot].frames)
            );
            assert_eq!(
                complete.samples.len(),
                expected[slot].frames * expected[slot].channels
            );
            // Real work/full PCM end before collection; no additional helper pin.
        }
        collect(harness);
        assert_eq!(states(harness), *expected);
    }
}

fn edit(
    harness: &mut Harness,
    probe: &Py<PyAny>,
    transport: &Py<PyAny>,
    expected: &mut [SlotState; 3],
    slot: usize,
    start: usize,
    baseline: &Baseline,
) {
    assert_eq!(states(harness), *expected);
    assert_timing(harness, baseline);
    let regions = SLOTS.map(|id| harness.callback.mixer.loop_region_frames(id));
    collect(harness);
    let revision: u64 = Python::attach(|py| {
        transport
            .bind(py)
            .getattr("loop")
            .unwrap()
            .call_method1("set_start", (slot, start as f64 / f64::from(harness.rate)))
            .unwrap();
        let residency = transport.bind(py).getattr("residency").unwrap();
        let (pending_status, error): (Option<String>, Option<String>) = residency
            .call_method1("status", (slot,))
            .unwrap()
            .extract()
            .unwrap();
        assert!(
            error.is_none(),
            "Automatic slot {slot} actual loop admission: {error:?}"
        );
        assert!(
            pending_status.is_some(),
            "Automatic loop did not retain its own actual request"
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
    assert_eq!(
        SLOTS.map(|id| harness.callback.mixer.loop_region_frames(id)),
        regions,
        "requested loop became effective before its actual callback ACK"
    );
    collect(harness);
    wait_for(
        || {
            collect(harness);
            harness.callback.drain();
            collect(harness);
            harness.callback.render(17);
            harness.with_engine(|engine| resident_relocation::reconcile(engine).unwrap());
            collect(harness);
            Python::attach(|py| {
                transport
                    .bind(py)
                    .getattr("residency")
                    .unwrap()
                    .call_method0("poll")
                    .unwrap();
            });
            collect(harness);
            let (pending, error) = status(transport, slot);
            assert!(
                error.is_none(),
                "Automatic slot {slot} actual loop preparation: {error:?}"
            );
            pending.is_none()
        },
        "productive Automatic loop did not observe its exact Native ACK",
        || format!("slot={slot} status={:?}", status(transport, slot)),
    );
    expected[slot].revision = revision;
    expected[slot].window.0 = start;
    expected[slot].loop_region.0 = start;
    assert_eq!(states(harness), *expected);
    assert_timing(harness, baseline);
    Python::attach(|py| {
        probe
            .bind(py)
            .getattr("persistence")
            .unwrap()
            .call_method0("flush")
            .unwrap();
    });
    let root = PathBuf::from("samples");
    let committed = saved(&root);
    assert_eq!(committed["pad_content"], baseline.identities);
    for id in SLOTS {
        assert_eq!(committed["pad_timing_intent"][id], "automatic");
        assert!(committed["manual_bpm"][id].is_null());
        assert_eq!(
            committed["sample_analysis"][id]["accepted_timing"],
            baseline.accepted
        );
        assert_eq!(
            committed["pad_loop_start_s"][id].as_f64(),
            Some(expected[id].loop_region.0 as f64 / f64::from(harness.rate))
        );
        assert_eq!(
            committed["pad_loop_end_s"][id].as_f64(),
            Some(END as f64 / f64::from(harness.rate))
        );
        assert_eq!(committed["pad_loop_auto"][id], false);
    }
    collect(harness);
}

#[test]
fn automatic_copies_keep_exact_reader_custody_through_productive_loop_edits() {
    isolated(|| {
        Python::initialize();
        // Existing fixture runs the actual QM producer and retains its real
        // evidence; neither the declared flag nor this test invents acceptance.
        let (qm_original, historical_json) = constant_timing::migration_test_fixture();
        let historical: Value = serde_json::from_str(&historical_json).unwrap();
        let binding = &historical["record"]["evidence"]["binding"];
        let rate = u32::try_from(binding["sample_rate_hz"].as_u64().unwrap()).unwrap();
        let frames = usize::try_from(binding["frame_count"].as_u64().unwrap()).unwrap();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let parent =
            workspace.join("test-artifacts/rearrange-human-fixes-20261021/automatic-loop-native");
        fs::create_dir_all(&parent).unwrap();
        fs::write(parent.join("OWNER.md"), b"Owned generated genuine QM/Automatic productive Copy and loop-custody regression. FullMix, neutral pitch, Key Lock OFF, stopped pads. Established synthetic migration fixture supplies genuine historical evidence. No Human audio/settings/installation/device. Root owns execution/review/cleanup.\n").unwrap();
        let directory = tempfile::Builder::new()
            .prefix("automatic-loop-")
            .tempdir_in(parent)
            .unwrap();
        let root = directory.path().join("samples");
        fs::create_dir(&root).unwrap();
        let original = root.join("automatic-source.wav");
        fs::copy(qm_original, &original).unwrap();
        let previous = CurrentDirectory(std::env::current_dir().unwrap());
        std::env::set_current_dir(directory.path()).unwrap();
        let material =
            prepare_material_complete_for_test(&root, &original, rate, 1, &|| false).unwrap();
        assert_eq!(material.sample.frame_count(), frames);
        let canonical = material.lease.original_path.to_string_lossy().into_owned();
        let mut harness = Harness::new(rate);
        harness.load(0, &material, &root);
        harness.with_engine(|engine| {
            engine
                .set_pad_timing_metadata(0, 0.003 + 7.0 / f64::from(rate))
                .unwrap();
            engine
                .set_pad_loop_region(
                    0,
                    START as f64 / f64::from(rate),
                    Some(END as f64 / f64::from(rate)),
                )
                .unwrap();
            engine.set_pad_gain(0, -7.0).unwrap();
            engine.set_pad_eq(0, 3.0, -4.0, 1.5).unwrap();
            engine.set_pad_key_lock(0, false).unwrap();
        });
        harness.callback.drain();
        let timing = harness.prepare_saved(0, &historical_json, &canonical);
        let acknowledged = harness.acknowledge_saved(0, &timing);
        let probe = probe(&harness, &material, &historical);
        // The actual completed preparation's pins end before finite Copy.
        drop(timing);
        drop(material);
        collect(&harness);
        initial_finite_window(&mut harness);
        assert_eq!(harness.current(0).unwrap(), acknowledged);
        let source_metadata = harness.current(0).unwrap();
        let source = state(&harness, 0);
        for target in [1, 2] {
            collect(&harness);
            copy_target(&mut harness, &probe, target, &source_metadata);
            collect(&harness);
            assert_eq!(state(&harness, 0), source);
        }
        let mut expected = states(&harness);
        for first in 0..SLOTS.len() {
            for second in first + 1..SLOTS.len() {
                assert_ne!(expected[first].assignment, expected[second].assignment);
                assert_ne!(expected[first].lifetime, expected[second].lifetime);
            }
        }
        let old_finite = harness.with_engine(|engine| {
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
        let baseline = Baseline {
            timing: SLOTS.map(|slot| harness.current(slot).unwrap()),
            identities: saved(&root)["pad_content"].clone(),
            accepted: historical,
            canonical,
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
                    harness.engine.clone_ref(py),
                    owned
                        .getattr("persistence")
                        .unwrap()
                        .getattr("mark_dirty")
                        .unwrap(),
                ))
                .unwrap()
                .unbind()
        });
        // First copied loop edits precede every extra complete-reader check.
        // Preserve each real saved assignment; do not apply/reset audio state.
        for (slot, start) in [(1, START + 64), (2, START + 128), (0, START + 192)] {
            edit(
                &mut harness,
                &probe,
                &transport,
                &mut expected,
                slot,
                start,
                &baseline,
            );
        }
        wait_for(
            || {
                harness.callback.drain();
                harness.callback.render(17);
                collect(&harness);
                old_finite.strong_count() == 0
            },
            "actual replaced Automatic finite PCM remained owned",
            || format!("old finite strong readers={}", old_finite.strong_count()),
        );
        collect(&harness);
        materialize_current(&harness, &expected);
        for (slot, start) in [(1, START + 256), (2, START + 320), (0, START + 384)] {
            edit(
                &mut harness,
                &probe,
                &transport,
                &mut expected,
                slot,
                start,
                &baseline,
            );
            materialize_current(&harness, &expected);
        }
        assert_timing(&harness, &baseline);
        Python::attach(|py| {
            for component in ["pitch", "residency"] {
                transport
                    .bind(py)
                    .getattr(component)
                    .unwrap()
                    .call_method0("shut_down")
                    .unwrap();
            }
            transport.drop_ref(py);
            py.import("gc").unwrap().call_method0("collect").unwrap();
            probe.call_method0(py, "close").unwrap();
            probe.drop_ref(py);
        });
        let Harness {
            engine, callback, ..
        } = harness;
        Python::attach(|py| engine.drop_ref(py));
        drop(callback);
        drop(previous);
    });
}
