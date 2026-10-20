//! Real Bank controller, Copy-seeded model/Native images, callback ACK and durable mirror.
//! No device, inferred hearing acceptance or Python-manufactured Native publication.
use super::*;
use crate::audio_engine::global_rate_status::RateState;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::productive_source_history::ProductiveSourceBinding;
use crate::audio_engine::{
    LoadedSourcePublication, SourcePitchInitialization, publish_loaded_sample_with_arm,
};
use pyo3::types::PyDict;

#[path = "controller_native_tests/bank_ui_tests.rs"]
mod bank_ui_tests;

#[path = "controller_native_tests/batch_216_tests.rs"]
mod batch_216_tests;

#[path = "controller_native_tests/fresh_recovery_tests.rs"]
mod fresh_recovery_tests;

#[path = "controller_native_tests/input_runtime_tests.rs"]
mod input_runtime_tests;

#[path = "controller_native_tests/last_reader_proof.rs"]
mod last_reader_proof;

#[path = "controller_native_tests/shared_job_tests.rs"]
mod shared_job_tests;

const WARMUP: usize = 16_384;

fn controller(fixture: &mut Fixture, wet: bool) -> Py<PyAny> {
    // Establish the real global tuple while stopped, then use the ordinary start
    // owner. Host preparation waits in Callback::start never advance virtual time.
    fixture.with_engine(|engine| engine.stop_sample(0).unwrap());
    fixture.callback.drain();
    let seed = probe(&fixture.engine, "Probe", Some(fixture_input(fixture)));
    // The public global setter requires a CPAL stream_handle. This headless
    // fixture uses its existing queue/dispatcher for that initial command only;
    // public source-bound Pad Key Lock and rate admission still run in the probe.
    // Global setter admission/resident-interest bookkeeping is outside this test.
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::SetKeyLock(wet))
        .unwrap();
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::SetStemMixMode {
            id: 0,
            mode: StemMixMode::FullMix,
            source_version_hash: 0,
        })
        .unwrap();
    let controller = Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/bank_native_probe.py"
            )))
            .unwrap(),
            c"bank_native_probe.py",
            c"bank_controller_native_probe",
        )
        .unwrap();
        let controller = module
            .getattr("BankControllerProbe")
            .unwrap()
            .call1((seed.bind(py),))
            .unwrap();
        let kwargs = PyDict::new(py);
        kwargs.set_item("key_lock", wet).unwrap();
        kwargs
            .set_item("speed", if wet { 0.8 } else { 1.0 })
            .unwrap();
        controller
            .call_method("prepare_full_mix", (), Some(&kwargs))
            .unwrap();
        controller.unbind()
    });
    fixture.callback.drain();
    fixture.with_engine(|engine| {
        let rate = engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        assert_eq!(rate.effect.state, RateState::Applied);
        assert_eq!(rate.requested.speed, if wet { 0.8 } else { 1.0 });
        assert_eq!(rate.effect.effective.speed, if wet { 0.8 } else { 1.0 });
    });
    assert_eq!(
        fixture.callback.mixer.pad_pitch_intent(0),
        Some(PitchIntent::default())
    );
    for id in 0..NUM_SAMPLES {
        assert_eq!(fixture.callback.mixer.key_lock_for_measurement(id), wet);
    }
    assert_eq!(
        fixture.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    Python::attach(|py| {
        let project = controller.bind(py).getattr("project").unwrap();
        assert_eq!(
            project
                .getattr("key_lock")
                .unwrap()
                .extract::<bool>()
                .unwrap(),
            wet
        );
        assert_eq!(
            project
                .getattr("pad_key_lock")
                .unwrap()
                .get_item(0)
                .unwrap()
                .extract::<bool>()
                .unwrap(),
            wet
        );
        assert_eq!(
            project.getattr("speed").unwrap().extract::<f64>().unwrap(),
            if wet { 0.8 } else { 1.0 }
        );
        let pitch = project
            .getattr("pad_key_intent")
            .unwrap()
            .get_item(0)
            .unwrap();
        assert_eq!(
            pitch
                .getattr("base_shift")
                .unwrap()
                .extract::<i32>()
                .unwrap(),
            0
        );
        assert_eq!(
            pitch
                .getattr("extra_shift")
                .unwrap()
                .extract::<i32>()
                .unwrap(),
            0
        );
    });
    fixture.start(0);
    let warm = render_fixed(fixture, WARMUP);
    assert!(warm.iter().all(|sample| sample.is_finite()));
    assert!(
        warm.iter().any(|sample| sample.abs() > 1e-4),
        "FullMix must actually sound"
    );
    if wet {
        assert_wet(fixture, 0);
    }
    controller
}

fn call(controller: &Py<PyAny>, method: &str) {
    Python::attach(|py| {
        controller.call_method0(py, method).unwrap();
    });
}

fn seed(fixture: &mut Fixture, controller: &Py<PyAny>, target: usize) {
    let ticket: Py<PadCopyTicket> = Python::attach(|py| {
        controller
            .call_method1(py, "seed_begin", (target,))
            .unwrap()
            .extract(py)
            .unwrap()
    });
    assert!(matches!(
        fixture.callback.controls.peek().unwrap(),
        ControlMessage::PadCopy(_)
    ));
    assert_eq!(fixture.callback.drain(), 1);
    Python::attach(|py| {
        assert_eq!(ticket.borrow(py).publication_status(), "accepted");
        controller
            .call_method1(py, "seed_complete", (target,))
            .unwrap();
    });
    fixture.with_engine(|engine| {
        assert_eq!(engine.input_runtime_ownership.copy_reservation(target), 0);
        assert!(
            engine
                .current_input_runtime_pad_binding(target)
                .unwrap()
                .unwrap()
                .is_current()
        );
    });
    assert_eq!(
        fixture.callback.mixer.pad_pitch_intent(target),
        Some(PitchIntent::default())
    );
    assert_eq!(
        fixture.callback.mixer.stem_demand_for_measurement(target).0,
        StemMixMode::FullMix
    );
}

fn render_fixed(fixture: &mut Fixture, mut frames: usize) -> Vec<f32> {
    let mut pcm = Vec::with_capacity(frames * 2);
    while frames != 0 {
        let next = frames.min(512);
        pcm.extend(fixture.callback.render(next));
        frames -= next;
    }
    pcm
}

fn actual_voice(fixture: &Fixture, id: usize) -> &crate::audio_engine::voice_slot::VoiceSlot {
    fixture
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(id))
        .unwrap()
}

fn assert_wet(fixture: &Fixture, id: usize) {
    let voice = actual_voice(fixture, id);
    assert!(fixture.callback.mixer.key_lock_for_measurement(id));
    let pitch = fixture
        .callback
        .mixer
        .pad_pitch_intent(id)
        .unwrap()
        .native_pitch_scale(voice.source_playback.tempo_ratio(), true)
        .unwrap();
    assert_ne!(
        pitch, 1.0,
        "this case must require actual Native processing"
    );
    assert_ne!(voice.stretch.native_state_address(), 0);
    assert!(voice.stretch.wet_render_ready());
    assert!(voice.stretch.wet_current_source_output_ready());
    assert!(
        voice
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames
            >= 4096
    );
    assert_productive_native_source(voice);
}

#[derive(Debug, PartialEq)]
struct VoiceProof {
    generation: u64,
    position: FractionalSourcePosition,
    paused: bool,
    native: usize,
    fifos: (usize, usize),
    feed_revision: u64,
    history: Option<(ProductiveSourceBinding, FractionalSourcePosition, u64)>,
    native_pitch: Option<u64>,
    period: Option<u64>,
    loop_region: (usize, Option<usize>),
}

fn proof(fixture: &Fixture, id: usize) -> VoiceProof {
    let voice = actual_voice(fixture, id);
    VoiceProof {
        generation: voice.generation,
        position: voice.source_playback.position(),
        paused: voice.paused,
        native: voice.stretch.native_state_address(),
        fifos: voice.stretch.pending_fifo_frames(),
        feed_revision: voice.source_feed_history.continuity_revision(),
        history: voice.stretch.productive_history().map(|history| {
            (
                history.binding,
                history.next_position,
                history.fed_output_frames,
            )
        }),
        native_pitch: voice
            .stretch
            .native_pitch_scale_observation()
            .map(f64::to_bits),
        period: voice.source_timing.period_seconds().map(f64::to_bits),
        loop_region: fixture.callback.mixer.loop_region_frames(id),
    }
}

fn session_effect(controller: &Py<PyAny>, id: usize, paused: bool) {
    // Setup projection follows the genuine callback start/Hold effect checked by Rust.
    // It does not create a Native acknowledgement or stand in for Controller.poll.
    Python::attach(|py| {
        let session = controller.bind(py).getattr("session").unwrap();
        session
            .getattr("active_sample_ids")
            .unwrap()
            .call_method1("add", (id,))
            .unwrap();
        session
            .getattr("paused_sample_ids")
            .unwrap()
            .call_method1(if paused { "add" } else { "discard" }, (id,))
            .unwrap();
    });
}

fn begin_bank(
    fixture: &mut Fixture,
    controller: &Py<PyAny>,
    source: Option<usize>,
    target: usize,
) -> Py<BankMutationTicket> {
    Python::attach(|py| {
        controller
            .call_method1(py, "capture", (source, target))
            .unwrap();
    });
    let frame = fixture.callback.frame;
    call(controller, "begin");
    assert!(fixture.callback.controls.is_empty());
    call(controller, "run_worker");
    let ticket: Py<BankMutationTicket> = Python::attach(|py| {
        controller
            .call_method0(py, "publish")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    assert_eq!(
        fixture.callback.frame, frame,
        "host work cannot advance the single audio clock"
    );
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
    ticket
}

fn settle(fixture: &Fixture, controller: &Py<PyAny>, ticket: &Py<BankMutationTicket>) -> Value {
    Python::attach(|py| {
        let ticket = ticket.borrow(py);
        assert_eq!(ticket.publication_status(), "accepted");
        let ack = ticket
            .acknowledgement()
            .expect("only the actual callback creates this parent ACK");
        fixture
            .with_engine(|engine| assert!(engine.validate_bank_mutation(&ticket, &ack).unwrap()));
    });
    call(controller, "settle");
    let receipt: Value = Python::attach(|py| {
        serde_json::from_str(
            &controller
                .call_method0(py, "settlement_json")
                .unwrap()
                .extract::<String>(py)
                .unwrap(),
        )
        .unwrap()
    });
    assert_eq!(receipt["commit_count"], json!(1));
    assert_eq!(
        receipt["revision"], receipt["before_revision"],
        "migration commits retain the established intent revision"
    );
    assert_eq!(receipt["revision"], receipt["current_revision"]);
    assert_eq!(receipt["sha256"], receipt["config_sha256"]);
    assert_eq!(
        receipt["sha256"],
        json!(format!("{:x}", Sha256::digest(config(controller))))
    );
    assert_eq!(receipt["owner"], receipt["receipt_owner"]);
    assert_eq!(receipt["phase"], json!("settled"));
    assert!(
        receipt["owner"]
            .as_str()
            .is_some_and(|owner| owner.len() == 32)
    );
    for name in [
        "mirror_applied",
        "publication_applied",
        "publication_settled",
        "native_finished",
        "writer_released",
        "completed",
    ] {
        assert_eq!(receipt[name], json!(true), "{name}");
    }
    fixture.with_engine(|engine| {
        for id in 0..NUM_SAMPLES {
            assert_eq!(engine.input_runtime_ownership.copy_reservation(id), 0);
            assert_eq!(engine.input_runtime_ownership.pair_reservation(id), 0);
        }
    });
    receipt
}

fn model_identity(controller: &Py<PyAny>, id: usize) -> Option<String> {
    Python::attach(|py| {
        let entry = controller
            .bind(py)
            .getattr("project")
            .unwrap()
            .getattr("pad_content")
            .unwrap()
            .get_item(id)
            .unwrap();
        if entry.is_none() {
            None
        } else {
            Some(entry.getattr("instance_id").unwrap().extract().unwrap())
        }
    })
}

fn model_parent(controller: &Py<PyAny>, id: usize) -> Option<String> {
    Python::attach(|py| {
        controller
            .bind(py)
            .getattr("project")
            .unwrap()
            .getattr("pad_content")
            .unwrap()
            .get_item(id)
            .unwrap()
            .getattr("copied_from")
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn config(controller: &Py<PyAny>) -> Vec<u8> {
    Python::attach(|py| {
        controller
            .bind(py)
            .getattr("persistence")
            .unwrap()
            .getattr("config_path")
            .unwrap()
            .call_method0("read_bytes")
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn close(controller: Py<PyAny>) {
    call(&controller, "forget_settled");
    call(&controller, "close");
    Python::attach(|_| drop(controller));
}

fn mixed_profile(mutate: bool) -> Vec<f32> {
    let mut fixture = bank_fixture_with_stems(true, true, "tap");
    let previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(fixture.directory.path()).unwrap();
    let controller = controller(&mut fixture, true);
    for target in [35, 36, 37, 71, 72] {
        seed(&mut fixture, &controller, target);
    }
    for target in [35, 37, 71, 72] {
        fixture.start(target);
        session_effect(&controller, target, false);
    }
    let warm = render_fixed(&mut fixture, WARMUP);
    assert!(warm.iter().all(|sample| sample.is_finite()));
    assert!(warm.iter().any(|sample| sample.abs() > 1e-4));
    for id in [0, 35, 37, 71, 72] {
        assert_wet(&fixture, id);
    }
    let source = hold(&fixture, 0, 501);
    let removed = hold(&fixture, 37, 502);
    let outside = hold(&fixture, 72, 503);
    fixture.callback.drain();
    for (id, action) in [(0, &source), (37, &removed), (72, &outside)] {
        assert_eq!(action.action.state(), HoldActionState::PausedOwned);
        session_effect(&controller, id, true);
    }
    let before = [proof(&fixture, 0), proof(&fixture, 35), proof(&fixture, 72)];
    let original_source = [
        model_identity(&controller, 0).unwrap(),
        model_identity(&controller, 35).unwrap(),
    ];
    let original_target = [
        model_identity(&controller, 36).unwrap(),
        model_identity(&controller, 71).unwrap(),
    ];
    let old_config = config(&controller);
    if mutate {
        let requests = fixture.with_engine(|engine| engine.pad_request_ids.lock().unwrap().clone());
        let ticket = begin_bank(&mut fixture, &controller, Some(0), 1);
        assert_eq!(
            [proof(&fixture, 0), proof(&fixture, 35), proof(&fixture, 72)],
            before
        );
        assert_eq!(config(&controller), old_config);
        fixture.with_engine(|engine| assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests));
        assert_eq!(fixture.callback.drain(), 1);
        assert_eq!(
            config(&controller),
            old_config,
            "callback ACK alone does not write Config"
        );
        assert_eq!(
            [proof(&fixture, 0), proof(&fixture, 35), proof(&fixture, 72)],
            before
        );
        assert_eq!(source.action.state(), HoldActionState::PausedOwned);
        assert_eq!(outside.action.state(), HoldActionState::PausedOwned);
        assert_eq!(removed.action.state(), HoldActionState::Retired);
        for index in 0..36 {
            assert_eq!(
                fixture.callback.mixer.bank_for_measurement()[36 + index].is_some(),
                index == 0 || index == 35
            );
        }
        assert!(
            !fixture
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| (36..72).contains(&voice.sample_id) && (voice.active || voice.paused))
        );
        settle(&fixture, &controller, &ticket);
        for (index, target) in [36, 71].into_iter().enumerate() {
            let identity = model_identity(&controller, target).unwrap();
            assert_ne!(identity, original_source[index]);
            assert_ne!(identity, original_target[index]);
            assert_eq!(
                model_parent(&controller, target),
                Some(original_source[index].clone())
            );
        }
        for target in 37..71 {
            assert!(model_identity(&controller, target).is_none());
        }
        fixture.with_engine(|engine| assert!(engine.release_waveform_hold(&removed).unwrap()));
        fixture.callback.drain();
        assert_eq!(removed.action.state(), HoldActionState::Retired);
        assert!(
            !fixture
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| (36..72).contains(&voice.sample_id) && (voice.active || voice.paused))
        );
        drop(ticket);
    } else {
        // Independently initialized reference removes only audible old target71.
        // Its paused old target37 remains inaudible throughout the compared prefix.
        fixture.with_engine(|engine| engine.stop_sample(71).unwrap());
        fixture.callback.drain();
        assert_eq!(
            [proof(&fixture, 0), proof(&fixture, 35), proof(&fixture, 72)],
            before
        );
    }
    for (id, action) in [(0, &source), (72, &outside)] {
        fixture.with_engine(|engine| assert!(engine.release_waveform_hold(action).unwrap()));
        fixture.callback.drain();
        assert_eq!(action.action.state(), HoldActionState::Resumed);
        session_effect(&controller, id, false);
    }
    let pcm = render_fixed(&mut fixture, WARMUP);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 1e-4));
    for id in [0, 35, 72] {
        assert_wet(&fixture, id);
    }
    close(controller);
    drop(previous);
    pcm
}

#[test]
fn real_controller_mixed_bank_preserves_productive_source_outside_pcm_and_hold_owners() {
    isolated(
        "bank_public_tests::controller_native_tests::real_controller_mixed_bank_preserves_productive_source_outside_pcm_and_hold_owners",
        || {
            let actual = mixed_profile(true);
            let reference = mixed_profile(false);
            assert_eq!(
                actual, reference,
                "complete fixed wet prefix, same inputs/partition/clock; no crop or alignment fit"
            );
        },
    );
}

#[test]
fn real_controller_all36_copy_and_clear_each_use_one_parent_ack_and_one_config_receipt() {
    isolated(
        "bank_public_tests::controller_native_tests::real_controller_all36_copy_and_clear_each_use_one_parent_ack_and_one_config_receipt",
        || {
            let mut fixture = bank_fixture_with_stems(true, false, "tap");
            let previous = CurrentDirectory(std::env::current_dir().unwrap());
            std::env::set_current_dir(fixture.directory.path()).unwrap();
            let controller = controller(&mut fixture, false);
            for target in (1..36).chain(std::iter::once(72)) {
                seed(&mut fixture, &controller, target);
            }
            let sources: Vec<_> = (0..36)
                .map(|id| model_identity(&controller, id).unwrap())
                .collect();
            let source_before = proof(&fixture, 0);
            let ticket = begin_bank(&mut fixture, &controller, Some(0), 1);
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(proof(&fixture, 0), source_before);
            assert!(
                fixture.callback.mixer.bank_for_measurement()[36..72]
                    .iter()
                    .all(Option::is_some)
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
            let copy_receipt = settle(&fixture, &controller, &ticket);
            let after: Vec<_> = (36..72)
                .map(|id| model_identity(&controller, id).unwrap())
                .collect();
            let unique: std::collections::HashSet<_> = after.iter().collect();
            assert_eq!(unique.len(), 36);
            for index in 0..36 {
                assert!(!sources.contains(&after[index]));
                assert_eq!(
                    model_parent(&controller, 36 + index),
                    Some(sources[index].clone())
                );
            }
            call(&controller, "forget_settled");
            drop(ticket);
            fixture.start(71);
            fixture.start(72);
            session_effect(&controller, 71, false);
            session_effect(&controller, 72, false);
            let removed = hold(&fixture, 71, 504);
            let outside = hold(&fixture, 72, 505);
            fixture.callback.drain();
            assert_eq!(removed.action.state(), HoldActionState::PausedOwned);
            assert_eq!(outside.action.state(), HoldActionState::PausedOwned);
            session_effect(&controller, 71, true);
            session_effect(&controller, 72, true);
            let source_before = proof(&fixture, 0);
            let outside_before = proof(&fixture, 72);
            let clear = begin_bank(&mut fixture, &controller, None, 1);
            assert_eq!(fixture.callback.drain(), 1);
            assert!(
                fixture.callback.mixer.bank_for_measurement()[36..72]
                    .iter()
                    .all(Option::is_none)
            );
            assert_eq!(proof(&fixture, 0), source_before);
            assert_eq!(proof(&fixture, 72), outside_before);
            assert_eq!(removed.action.state(), HoldActionState::Retired);
            assert_eq!(outside.action.state(), HoldActionState::PausedOwned);
            let clear_receipt = settle(&fixture, &controller, &clear);
            assert_ne!(copy_receipt["owner"], clear_receipt["owner"]);
            for id in 36..72 {
                assert!(model_identity(&controller, id).is_none());
            }
            fixture.with_engine(|engine| {
                assert!(engine.release_waveform_hold(&removed).unwrap());
                assert!(engine.release_waveform_hold(&outside).unwrap());
            });
            fixture.callback.drain();
            assert_eq!(outside.action.state(), HoldActionState::Resumed);
            let pcm = render_fixed(&mut fixture, 2048);
            assert!(pcm.iter().all(|sample| sample.is_finite()));
            assert!(pcm.iter().any(|sample| sample.abs() > 1e-4));
            drop(clear);
            close(controller);
            drop(previous);
        },
    );
}

#[test]
fn real_controller_rejects_same_pcm_native_target_aba_after_original_logical_capture() {
    isolated(
        "bank_public_tests::controller_native_tests::real_controller_rejects_same_pcm_native_target_aba_after_original_logical_capture",
        || {
            let mut fixture = bank_fixture_with_stems(true, false, "tap");
            let previous = CurrentDirectory(std::env::current_dir().unwrap());
            std::env::set_current_dir(fixture.directory.path()).unwrap();
            let controller = controller(&mut fixture, false);
            for target in [35, 36, 72] {
                seed(&mut fixture, &controller, target);
            }
            Python::attach(|py| {
                controller
                    .call_method1(py, "capture", (Some(0), 1))
                    .unwrap();
            });
            let target_identity = model_identity(&controller, 36).unwrap();
            let saved = config(&controller);
            let (sample, generation, before) = fixture.with_engine(|engine| {
                let sample = engine.sample_cache.lock().unwrap()[36]
                    .as_ref()
                    .unwrap()
                    .clone();
                let binding = engine
                    .current_input_runtime_pad_binding(36)
                    .unwrap()
                    .unwrap();
                (
                    sample,
                    binding.source_generation,
                    engine.input_runtime_ownership.slot_snapshot(36).unwrap(),
                )
            });
            fixture.with_engine(|engine| {
                let _requests = engine.pad_request_ids.lock().unwrap();
                let mut generations = engine.loaded_source_generations.lock().unwrap();
                let mut digests = engine.loaded_source_digests.lock().unwrap();
                let digest = digests[36].clone().unwrap();
                publish_loaded_sample_with_arm(
                    &fixture.producer,
                    &engine.sample_cache,
                    36,
                    sample.clone(),
                    LoadedSourcePublication {
                        initial_mix: None,
                        ownership: &engine.input_runtime_ownership,
                        placement: before,
                        generation,
                        rate: RATE,
                        generation_slot: &mut generations[36],
                        digest_slot: &mut digests[36],
                        digest,
                        cold: false,
                        cold_epoch: None,
                        cold_adoption: None,
                        replace_assignment: true,
                        loop_region: None,
                        resident_cancelled: None,
                        incoming_defaults: None,
                        initial_pitch: SourcePitchInitialization::FreshNeutral,
                        intent: None,
                    },
                    || Ok(()),
                )
                .unwrap();
            });
            assert_eq!(fixture.callback.drain(), 1);
            assert!(Arc::ptr_eq(
                &fixture.callback.mixer.bank_for_measurement()[36]
                    .as_ref()
                    .unwrap()
                    .samples,
                &sample.samples
            ));
            let requests = fixture.with_engine(|engine| {
                assert_eq!(
                    engine
                        .current_input_runtime_pad_binding(36)
                        .unwrap()
                        .unwrap()
                        .source_generation,
                    generation
                );
                assert_ne!(
                    engine
                        .input_runtime_ownership
                        .slot_snapshot(36)
                        .unwrap()
                        .lifetime(),
                    before.lifetime()
                );
                engine.pad_request_ids.lock().unwrap().clone()
            });
            let snapshots: Vec<_> = fixture.with_engine(|engine| {
                (0..NUM_SAMPLES)
                    .map(|id| engine.input_runtime_ownership.slot_snapshot(id).unwrap())
                    .collect()
            });
            let source_before = proof(&fixture, 0);
            call(&controller, "begin");
            let error: String = Python::attach(|py| {
                controller
                    .call_method0(py, "assert_rejected")
                    .unwrap()
                    .extract(py)
                    .unwrap()
            });
            assert!(
                error.contains("participants") && error.contains("stale"),
                "must reject the original Native witness: {error}"
            );
            assert_eq!(config(&controller), saved);
            assert_eq!(model_identity(&controller, 36), Some(target_identity));
            assert_eq!(proof(&fixture, 0), source_before);
            assert!(
                fixture.callback.controls.is_empty(),
                "rejected admission emitted no Bank command"
            );
            fixture.with_engine(|engine| {
                assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests);
                for (id, before) in snapshots.iter().enumerate() {
                    assert!(
                        engine
                            .input_runtime_ownership
                            .slot_snapshot(id)
                            .unwrap()
                            .same_projection(*before)
                    );
                    assert_eq!(engine.input_runtime_ownership.copy_reservation(id), 0);
                    assert_eq!(engine.input_runtime_ownership.pair_reservation(id), 0);
                }
            });
            close(controller);
            drop(previous);
        },
    );
}
