//! Canonical source -> actual public Native Copy -> callback -> productive config commit.
//! No CPAL/device, model inference, invented acceptance, or duplicate test-module includes.
#![cfg(windows)]

#[path = "midi_binding_tests.rs"]
mod midi_binding_tests;

#[path = "overwrite_public_tests.rs"]
mod overwrite_public_tests;

use super::{CapturedPadCopy, PadCopyTicket};
use crate::audio_engine::AudioEngine;
use crate::audio_engine::audio_stream::{
    drain_control_messages, drain_parameter_messages, render_scheduled_audio,
};
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirementWorker, RtAudioBufferRetirement, create_audio_buffer_retirement,
};
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::material_migration::{
    PreparedMigrationMaterial, prepare_material_complete_for_test,
};
use crate::audio_engine::material_migration_control::{adopt_for_format, prepared_for_test};
use crate::audio_engine::material_paths;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::resident_relocation;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::stem_cache::{STEM_FILE_NAMES, source_version_hash};
use crate::audio_engine::stem_pair_preparation::PreparedStemPair;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{
    AudioMessage, ControlMessage, ControlParameterMessage, LoaderEvent, StemMixMode,
    TriggerQuantization,
};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use rtrb::{Consumer, Producer, RingBuffer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, atomic::Ordering};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const FRAMES: usize = 4096;
const START: usize = 128;
const END: usize = 2048;
const BPM: f64 = 137.25;
const ANCHOR: f64 = 0.003;
const OFFSET: i64 = 7;
const CHILD: &str = "FLITZI_PAD_COPY_PUBLIC_CHILD";
const PRODUCER_PARENT: &str = "FLITZI_PAD_COPY_PRODUCER_PARENT";
const PRODUCER_HANDOFF: &str = "copy-producer-handoff.json";
const RESTORE_ROOT: &str = "FLITZI_PAD_COPY_RESTORE_ROOT";
const RESTORE_PARENT: &str = "FLITZI_PAD_COPY_RESTORE_PARENT";
const RESTORE_READY: &str = "COPY_RESTORE_OWN_ACK_READY";

fn test_name(name: &str) -> String {
    let (_, modules) = module_path!().split_once("::").unwrap();
    format!("{modules}::{name}")
}

fn isolated(name: &str, body: impl FnOnce()) {
    if std::env::var_os(CHILD).is_some() {
        body();
        eprintln!("actual Native Copy child PID={}", std::process::id());
        return;
    }
    // libtest lists paths relative to the crate; module_path! includes its name.
    let test = test_name(name);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Native Copy child {test} failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "Native Copy child {test} did not run exactly one passing test:\n{stdout}\n{stderr}"
    );
    assert!(
        stderr.contains("actual Native Copy child PID="),
        "Native Copy child {test} did not execute its actual fixture:\n{stdout}\n{stderr}"
    );
}

// A real PCM16 five-artifact source/set, with instrumental = melody+bass+drums.
// Matching aligned components avoid an unrelated synthetic correlation oracle.
fn wav(layers: &[usize]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let size = (FRAMES * 4) as u32;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for frame in 0..FRAMES {
        for channel in 0..2 {
            let value: i16 = layers
                .iter()
                .map(|layer| {
                    let phase =
                        frame as f64 * (0.031 + *layer as f64 * 0.017) + channel as f64 * 0.41;
                    (phase.sin() * (1700.0 + *layer as f64 * 500.0)).round() as i16
                })
                .sum();
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

struct Callback {
    mixer: Box<RtMixer>,
    controls: Consumer<ControlMessage>,
    parameters: Consumer<ControlParameterMessage>,
    feedback: Producer<AudioMessage>,
    scheduler: FixedCapacityScheduler<32>,
    transport: TransportTimeline,
    quantization: TriggerQuantization,
    retirement: RtAudioBufferRetirement,
    _retirement_worker: AudioBufferRetirementWorker,
    frame: u64,
}

impl Callback {
    fn drain(&mut self) -> usize {
        drain_parameter_messages(&mut self.parameters, &mut self.mixer, &mut self.transport);
        drain_control_messages(
            &mut self.controls,
            &mut self.scheduler,
            self.frame,
            &mut self.quantization,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut self.retirement,
        )
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        render_scheduled_audio(
            &mut self.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            self.frame,
            2,
            &mut self.transport,
            &mut self.feedback,
            &mut self.retirement,
        );
        self.frame += frames as u64;
        output
    }
}

fn empty_engine() -> (AudioEngine, Callback, Arc<Mutex<Producer<ControlMessage>>>) {
    let mut engine = AudioEngine::new().unwrap();
    let (producer, controls) = RingBuffer::new(128);
    let producer = Arc::new(Mutex::new(producer));
    let (parameter_producer, parameters) = RingBuffer::new(128);
    let (feedback, messages) = RingBuffer::new(512);
    engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(messages))));
    engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameter_producer)));
    let mut mixer = Box::new(RtMixer::new(2, RATE as f32));
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    let (retirement, worker) = create_audio_buffer_retirement();
    let callback = Callback {
        mixer,
        controls,
        parameters,
        feedback,
        scheduler: FixedCapacityScheduler::new(),
        transport: TransportTimeline::new(RATE),
        quantization: TriggerQuantization::Immediate,
        retirement,
        _retirement_worker: worker,
        frame: 0,
    };
    (engine, callback, producer)
}

struct Fixture {
    engine: Py<AudioEngine>,
    callback: Callback,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    material: PreparedMigrationMaterial,
    version: String,
    pair: Option<PreparedStemPair>,
    pair_json: Value,
    key_lock: bool,
    timing: &'static str,
    // Drop generated fixture files only after all sealed source/reader owners.
    directory: tempfile::TempDir,
}

impl Fixture {
    fn new(stems: bool, key_lock: bool, timing: &'static str) -> Self {
        Self::with_pair_registration(stems, true, key_lock, timing)
    }

    fn with_pair_registration(
        stems: bool,
        registered: bool,
        key_lock: bool,
        timing: &'static str,
    ) -> Self {
        Python::initialize();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let fixture_parent = std::env::var_os(PRODUCER_PARENT)
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace.join("scratch"));
        let directory = tempfile::Builder::new()
            .prefix("native-copy-public-")
            .tempdir_in(fixture_parent)
            .unwrap();
        let root = directory.path().join("samples");
        fs::create_dir(&root).unwrap();
        let original = root.join("copy-source.wav");
        fs::write(&original, wav(&[0, 1, 2, 3])).unwrap();
        let material =
            prepare_material_complete_for_test(&root, &original, RATE, 2, &|| false).unwrap();
        let metadata = material.metadata();
        let version = format!(
            "{}|sha256-v1:{}",
            metadata["new_reference"].as_str().unwrap(),
            metadata["original"]["sha256"].as_str().unwrap()
        );
        let (mut engine, mut callback, producer) = empty_engine();
        let preparation = prepared_for_test(
            &engine,
            PreparedMigrationMaterial::from_current(
                &root,
                material.sample.clone(),
                material.lease.clone(),
            )
            .unwrap(),
            &root,
        )
        .unwrap();
        let loaded = adopt_for_format(
            &engine,
            0,
            &preparation,
            producer.clone(),
            (2, RATE, root.clone()),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            callback.drain();
            if engine.cold_loading[0].load(Ordering::Acquire) == 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "canonical source did not receive its real cold ACK"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut success = false;
        while let Ok(event) = engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
            .map(LoaderEvent::into_test_payload)
        {
            match event {
                LoaderEvent::Success { request_id, .. } if request_id == loaded.request_id() => {
                    success = true
                }
                LoaderEvent::Error { error, .. } => panic!("canonical source failed: {error:?}"),
                _ => {}
            }
        }
        assert!(success && loaded.is_current().unwrap());
        preparation.release_preparation().unwrap();
        engine.set_pad_bpm(0, Some(BPM)).unwrap();
        engine
            .set_pad_timing_metadata(0, ANCHOR + OFFSET as f64 / f64::from(RATE))
            .unwrap();
        engine
            .set_pad_loop_region(
                0,
                START as f64 / f64::from(RATE),
                Some(END as f64 / f64::from(RATE)),
            )
            .unwrap();
        engine.set_pad_gain(0, -7.0).unwrap();
        engine.set_pad_eq(0, 3.0, -4.0, 1.5).unwrap();
        // Scalar BPM and origin publication deliberately select Legacy. Restore
        // the saved TAP/Manual intent after both real scalar setters have run.
        engine.set_pad_timing_intent(0, timing).unwrap();
        callback.drain();
        assert_eq!(engine.pad_timing_intent(0).unwrap(), timing);
        let mode_request = engine.set_pad_key_lock(0, key_lock).unwrap();
        callback.drain();
        let mode = engine.input_runtime_ownership.key_lock_status(0).unwrap();
        assert_eq!(mode.request_id, mode_request);
        assert_eq!(mode.effective, key_lock);
        let (pair, pair_json) = if stems {
            let material_root = material_paths::canonical_original_root_reference(
                Path::new(metadata["new_reference"].as_str().unwrap()),
                metadata["material_id"].as_str().unwrap(),
            )
            .unwrap();
            let wav_reference = format!(
                "{}/stems/.ready-0123456789abcdef0123456789abcdef",
                material_root.to_string_lossy().replace('\\', "/")
            );
            let wav_path = directory.path().join(&wav_reference);
            fs::create_dir_all(&wav_path).unwrap();
            let mut hashes = serde_json::Map::new();
            for (index, name) in STEM_FILE_NAMES.iter().enumerate() {
                let layers = if index == 4 {
                    vec![1, 2, 3]
                } else {
                    vec![index]
                };
                let encoded = wav(&layers);
                fs::write(wav_path.join(format!("{name}.wav")), &encoded).unwrap();
                hashes.insert(
                    (*name).to_owned(),
                    json!(format!("{:x}", Sha256::digest(&encoded))),
                );
            }
            fs::write(
                wav_path.join(".complete.json"),
                serde_json::to_vec(&json!({
                    "schema":"stem-set-sha256-v1", "source_version":version, "stems":hashes,
                }))
                .unwrap(),
            )
            .unwrap();
            let source_ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
            let pair = engine
                .prepare_stem_pair_at_root(
                    &root,
                    0,
                    &version,
                    &wav_reference,
                    &source_ticket,
                    true,
                    None,
                )
                .unwrap();
            if registered {
                let selection = engine
                    .register_committed_stem_pair_with_producer(&pair, &source_ticket, &producer)
                    .unwrap();
                assert_eq!(selection.publication_status(), "pending");
                callback.drain();
                assert_eq!(selection.publication_status(), "accepted");
                // Stopped residency acknowledges readers, not audible effect.
                // Establish the real public source voice before requesting ALL.
                engine.play_sample(0, 1.0, None).unwrap();
                callback.drain();
                callback.render(37);
                assert!(
                    callback
                        .mixer
                        .voices
                        .iter()
                        .any(|voice| voice.is_playing_sample(0))
                );
                let residency = engine
                    .request_committed_stem_residency_with_producer(
                        &selection,
                        "all_stems",
                        0,
                        &producer,
                    )
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while residency.effective_mode_status() != "effective" {
                    callback.drain();
                    callback.render(17);
                    assert!(
                        residency.residency_status() != "rejected",
                        "{:?}",
                        residency.error()
                    );
                    assert!(
                        Instant::now() < deadline,
                        "source components did not receive own effective ACK: residency={} effect={} error={:?}",
                        residency.residency_status(),
                        residency.effective_mode_status(),
                        residency.error()
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                resident_relocation::reconcile(&engine).unwrap();
            }
            let selected = serde_json::from_str(pair.selection_json()).unwrap();
            pair.select();
            (Some(pair), selected)
        } else {
            (None, Value::Null)
        };
        let engine = Python::attach(|py| Py::new(py, engine).unwrap());
        let mut fixture = Self {
            engine,
            callback,
            producer,
            material,
            version,
            pair,
            pair_json,
            key_lock,
            timing,
            directory,
        };
        if !fixture
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
        {
            fixture.with_engine(|engine| engine.play_sample(0, 1.0, None).unwrap());
            fixture.callback.drain();
            fixture.callback.render(37);
        }
        assert!(
            fixture
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
        );
        fixture
    }

    fn with_engine<T>(&self, operation: impl FnOnce(&mut AudioEngine) -> T) -> T {
        Python::attach(|py| operation(&mut self.engine.borrow_mut(py)))
    }

    fn envelope(&self) -> String {
        let metadata = self.material.metadata();
        json!({"schema_version":1, "resolved_key_lock":self.key_lock, "snapshot": {
            "source_path":metadata["new_reference"],
            "source_identity":{"instance_id":"88888888888888888888888888888888", "material_id":metadata["material_id"],
                "material_creation":metadata["material_creation"], "copied_from":null},
            "duration":FRAMES as f64 / f64::from(RATE), "analysis":null,
            "stem_cache":if self.pair.is_some() { json!({"available":false, "pair":self.pair_json}) } else { Value::Null },
            "stem_mix_mode":if self.pair.is_some() { "all_stems" } else { "full_mix" },
            "stem_intent":{"schema_version":1,"enabled_mask":0,"last_custom_mask":5,"display_mode":"custom"},
            "key_lock":self.key_lock,"manual_bpm":BPM,"timing_intent":self.timing,"key_intent":null,
            "gain_db":-7.0,"eq_low_db":3.0,"eq_mid_db":-4.0,"eq_high_db":1.5,
            "loop_start_s":START as f64 / f64::from(RATE),"loop_end_s":END as f64 / f64::from(RATE),
            "loop_auto":false,"loop_bars":1.5,"grid_offset_samples":OFFSET,"grid_anchor_s":ANCHOR,
        }}).to_string()
    }

    fn capture(&self, target: usize) -> CapturedPadCopy {
        self.with_engine(|engine| {
            let binding = engine
                .current_input_runtime_pad_binding(0)
                .unwrap()
                .unwrap();
            assert!(binding.is_current());
            engine
                .capture_pad_copy(&binding, target, &self.envelope(), None)
                .unwrap()
        })
    }

    fn prepare(&self, captured: &CapturedPadCopy) -> PadCopyTicket {
        Python::attach(|py| {
            AudioEngine::prepare_pad_copy(self.engine.clone_ref(py), py, captured).unwrap()
        })
    }

    fn source_voice(&self) -> &crate::audio_engine::voice_slot::VoiceSlot {
        self.callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap()
    }

    fn keep_directory_for_process_exit(self) -> PathBuf {
        let Self {
            engine,
            callback,
            producer,
            material,
            pair,
            directory,
            ..
        } = self;
        Python::attach(|_| {
            drop(engine);
        });
        drop(callback);
        drop(producer);
        drop(pair);
        drop(material);
        directory.keep()
    }
}

#[test]
fn actual_public_copy_to_36_and_215_keeps_source_voice_and_independent_shared_custody() {
    isolated(
        "actual_public_copy_to_36_and_215_keeps_source_voice_and_independent_shared_custody",
        || {
            for (key_lock, timing, registered) in [
                (false, "tap", true),
                (true, "manual", true),
                (false, "tap", false),
            ] {
                let mut fixture =
                    Fixture::with_pair_registration(true, registered, key_lock, timing);
                let source = fixture.callback.mixer.bank_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .clone();
                let stems = fixture.callback.mixer.stems_for_measurement()[0]
                    .as_ref()
                    .map(|set| set.stems.clone())
                    .or_else(|| {
                        fixture
                            .pair
                            .as_ref()
                            .unwrap()
                            .component_views_for_test()
                            .cloned()
                    })
                    .unwrap();
                let original_demand = fixture.callback.mixer.stem_demand_for_measurement(0);
                let generation = fixture.source_voice().generation;
                let position = fixture.source_voice().source_playback.position();
                let authority = fixture
                    .with_engine(|engine| engine.input_runtime_ownership.authority_revision(0));
                let mut lifetimes = Vec::new();
                let mut copied_selections = Vec::new();
                for target in [36, 215] {
                    let epoch = fixture.with_engine(|engine| {
                        engine.prepared_source_epochs[target].load(Ordering::Acquire)
                    });
                    let captured = fixture.capture(target);
                    let ticket = fixture.prepare(&captured);
                    assert_ne!(ticket.publication_status(), "accepted");
                    fixture.with_engine(|engine| {
                        assert!(engine.sample_cache.lock().unwrap()[target].is_none());
                        assert_eq!(
                            engine.prepared_source_epochs[target].load(Ordering::Acquire),
                            epoch
                        );
                        assert!(!engine.reconcile_pad_copy(&ticket).unwrap());
                        assert!(engine.finish_pad_copy(&ticket).is_err());
                        engine.publish_pad_copy(&ticket).unwrap();
                    });
                    let (prepared, prepared_stems) = match fixture.callback.controls.peek().unwrap()
                    {
                        ControlMessage::PadCopy(work) => {
                            assert_eq!(work.publication.status(), "pending");
                            assert_eq!(work.guard.phase.load(Ordering::Acquire), 0);
                            assert_eq!(work.intent.enabled_mask, 0);
                            assert_eq!(work.intent.gain_db, -7.0);
                            assert_eq!(
                                (
                                    work.intent.eq_low_db,
                                    work.intent.eq_mid_db,
                                    work.intent.eq_high_db
                                ),
                                (3.0, -4.0, 1.5)
                            );
                            assert_eq!(work.intent.period_seconds, Some(60.0 / BPM));
                            assert_eq!(
                                work.intent.origin_frame,
                                (ANCHOR * f64::from(RATE)).round_ties_even() + OFFSET as f64
                            );
                            assert_eq!(work.intent.key_lock, key_lock);
                            (work.sample.clone(), work.stems.as_ref().unwrap().clone())
                        }
                        other => panic!("expected the actual single Copy payload, got {other:?}"),
                    };
                    assert!(Arc::ptr_eq(&source.samples, &prepared.samples));
                    assert!(Arc::ptr_eq(
                        &source.residency.as_ref().unwrap().source,
                        &prepared.residency.as_ref().unwrap().source
                    ));
                    assert!(!Arc::ptr_eq(
                        source.residency.as_ref().unwrap(),
                        prepared.residency.as_ref().unwrap()
                    ));
                    for (original, copy) in stems.iter().zip(&prepared_stems.stems) {
                        assert!(Arc::ptr_eq(&original.samples, &copy.samples));
                        assert!(!Arc::ptr_eq(
                            original.residency.as_ref().unwrap(),
                            copy.residency.as_ref().unwrap()
                        ));
                    }
                    assert_eq!(fixture.callback.drain(), 1);
                    assert_eq!(ticket.publication_status(), "accepted");
                    assert!(fixture.callback.mixer.bank_for_measurement()[target].is_some());
                    assert!(
                        !fixture
                            .callback
                            .mixer
                            .voices
                            .iter()
                            .any(|voice| voice.active && voice.sample_id == target)
                    );
                    assert_eq!(
                        fixture.callback.mixer.loop_region_frames(target),
                        (START, Some(END))
                    );
                    assert_eq!(
                        fixture.callback.mixer.key_lock_for_measurement(target),
                        key_lock
                    );
                    assert_eq!(
                        fixture.callback.mixer.stem_demand_for_measurement(target),
                        (
                            StemMixMode::AllStems,
                            0,
                            source_version_hash(&fixture.version)
                        )
                    );
                    fixture.with_engine(|engine| {
                        assert!(engine.reconcile_pad_copy(&ticket).unwrap());
                        assert!(
                            engine
                                .set_pad_gain(target, -2.0)
                                .unwrap_err()
                                .to_string()
                                .contains("copy")
                        );
                        let leases = engine.cold_leases.lock().unwrap();
                        assert_ne!(
                            leases[0].as_ref().unwrap().assignment_id(),
                            leases[target].as_ref().unwrap().assignment_id()
                        );
                        drop(leases);
                        engine.finish_pad_copy(&ticket).unwrap();
                        copied_selections.push(ticket.committed_stem_ticket().unwrap().unwrap());
                        assert_eq!(engine.input_runtime_ownership.copy_reservation(target), 0);
                        assert!(
                            engine
                                .current_input_runtime_pad_binding(target)
                                .unwrap()
                                .unwrap()
                                .is_current()
                        );
                    });
                    let metadata: Value = serde_json::from_str(&ticket.metadata_json()).unwrap();
                    lifetimes.push(metadata["content_lifetime"].as_u64().unwrap());
                }
                assert!(lifetimes.iter().all(|lifetime| *lifetime != 0));
                assert_ne!(lifetimes[0], lifetimes[1]);
                assert_eq!(fixture.source_voice().generation, generation);
                assert_eq!(fixture.source_voice().source_playback.position(), position);
                assert_eq!(
                    fixture.with_engine(|engine| engine
                        .input_runtime_ownership
                        .authority_for_cell(
                            engine
                                .input_runtime_ownership
                                .slot_snapshot(0)
                                .unwrap()
                                .cell()
                        )
                        .load(Ordering::Acquire)),
                    authority
                );
                fixture.callback.render(29);
                assert_eq!(fixture.source_voice().generation, generation);
                assert_ne!(fixture.source_voice().source_playback.position(), position);
                fixture.with_engine(|engine| engine.play_sample(36, 1.0, None).unwrap());
                fixture.callback.drain();
                fixture.callback.render(37);
                let own_mask = fixture.with_engine(|engine| {
                    engine
                        .request_committed_stem_residency(
                            &copied_selections[0],
                            "all_stems",
                            15,
                            "explicit",
                        )
                        .unwrap()
                });
                let deadline = Instant::now() + Duration::from_secs(10);
                while own_mask.effective_mode_status() != "effective" {
                    fixture.callback.drain();
                    fixture.callback.render(37);
                    assert!(
                        own_mask.residency_status() != "rejected",
                        "{:?}",
                        own_mask.error()
                    );
                    assert!(
                        Instant::now() < deadline,
                        "copied target mask lacks its own effect ACK: {:?}",
                        own_mask.error()
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(fixture.callback.mixer.stem_demand_for_measurement(36).1, 15);
                assert_eq!(
                    fixture.callback.mixer.stem_demand_for_measurement(0),
                    original_demand
                );
                assert_eq!(fixture.callback.mixer.stem_demand_for_measurement(215).1, 0);
                assert_eq!(fixture.source_voice().generation, generation);
                fixture.with_engine(|engine| {
                    engine
                        .set_pad_loop_region(
                            36,
                            (START + 16) as f64 / f64::from(RATE),
                            Some((END - 32) as f64 / f64::from(RATE)),
                        )
                        .unwrap();
                    engine.set_pad_gain(36, -2.0).unwrap();
                    engine.set_pad_eq(36, -2.0, 1.0, -1.0).unwrap();
                });
                fixture.callback.drain();
                fixture.with_engine(|engine| engine.set_pad_key_lock(36, false).unwrap());
                fixture.callback.drain();
                assert_eq!(
                    fixture.callback.mixer.stem_demand_for_measurement(0),
                    original_demand
                );
                assert_eq!(
                    fixture.callback.mixer.loop_region_frames(0),
                    (START, Some(END))
                );
                assert_eq!(
                    fixture.callback.mixer.loop_region_frames(36),
                    (START + 16, Some(END - 32))
                );
                assert_eq!(
                    fixture.callback.mixer.loop_region_frames(215),
                    (START, Some(END))
                );
                assert_eq!(fixture.callback.mixer.key_lock_for_measurement(0), key_lock);
                assert!(!fixture.callback.mixer.key_lock_for_measurement(36));
                assert_eq!(fixture.callback.mixer.stem_demand_for_measurement(215).1, 0);
                fixture.with_engine(|engine| engine.unload_sample(0).unwrap());
                fixture.callback.drain();
                fixture.with_engine(|engine| {
                    assert!(engine.sample_cache.lock().unwrap()[0].is_none());
                    for target in [36, 215] {
                        let lease = engine.cold_leases.lock().unwrap()[target].clone().unwrap();
                        assert!(!lease.assignment_retired());
                        assert!(
                            lease.original_path.is_file()
                                && lease.cache_path.join("playback.f32le").is_file()
                        );
                        assert!(
                            engine
                                .current_input_runtime_pad_binding(target)
                                .unwrap()
                                .unwrap()
                                .is_current()
                        );
                    }
                });
                let audible_copy = fixture.callback.render(1025);
                assert!(
                    audible_copy.iter().any(|sample| sample.abs() > 1e-8),
                    "mask-15 copy did not render real audio after origin unload"
                );
                let copied_voice = fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .find(|voice| voice.is_playing_sample(36))
                    .unwrap();
                assert_eq!(
                    copied_voice.source_timing.legacy_period_seconds,
                    Some(60.0 / BPM)
                );
                assert_eq!(
                    copied_voice.source_timing.legacy_origin_frame,
                    (ANCHOR * f64::from(RATE)).round_ties_even() + OFFSET as f64
                );
                assert!(Arc::ptr_eq(
                    &copied_voice.sample.as_ref().unwrap().samples,
                    &source.samples
                ));
            }
        },
    );
}

#[test]
fn actual_public_copy_cancellation_staleness_queue_and_uncertain_claim_keep_fences() {
    isolated(
        "actual_public_copy_cancellation_staleness_queue_and_uncertain_claim_keep_fences",
        || {
            let mut fixture = Fixture::new(false, false, "tap");
            let source_binding = fixture.with_engine(|engine| {
                engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap()
            });
            let foreign = AudioEngine::new().unwrap();
            assert!(
                foreign
                    .capture_pad_copy(&source_binding, 36, &fixture.envelope(), None)
                    .is_err()
            );
            let captured = fixture.capture(36);
            let ticket = fixture.prepare(&captured);
            let epoch = fixture
                .with_engine(|engine| engine.prepared_source_epochs[36].load(Ordering::Acquire));
            fixture.with_engine(|engine| {
                assert!(engine.unload_sample(36).is_err());
                assert!(engine.set_pad_gain(36, -3.0).is_err());
            });
            while !fixture.producer.lock().unwrap().is_full() {
                fixture
                    .producer
                    .lock()
                    .unwrap()
                    .push(ControlMessage::Ping())
                    .unwrap();
            }
            fixture.with_engine(|engine| assert!(engine.publish_pad_copy(&ticket).is_err()));
            assert_ne!(ticket.publication_status(), "accepted");
            assert!(fixture.callback.mixer.bank_for_measurement()[36].is_none());
            assert_eq!(fixture.with_engine(|engine| engine.prepared_source_epochs[36].load(Ordering::Acquire)), epoch);
            while fixture.callback.drain() != 0 {}
            fixture.with_engine(|engine| {
                engine.publish_pad_copy(&ticket).unwrap();
                assert!(!engine.reconcile_pad_copy(&ticket).unwrap());
                assert!(engine.cancel_pad_copy(&captured).unwrap());
            });
            fixture.callback.drain();
            assert_eq!(ticket.publication_status(), "rejected");
            assert!(fixture.callback.mixer.bank_for_measurement()[36].is_none());
            assert_eq!(
                fixture.with_engine(|engine| engine.input_runtime_ownership.copy_reservation(36)),
                0
            );
            let stale = fixture.capture(215);
            let stale_ticket = fixture.prepare(&stale);
            fixture.with_engine(|engine| engine.set_pad_bpm(0, Some(141.0)).unwrap());
            fixture.with_engine(|engine| {
                assert!(engine.publish_pad_copy(&stale_ticket).is_err());
                assert!(engine.cancel_pad_copy(&stale).unwrap());
            });
            fixture.callback.drain();
            fixture.with_engine(|engine| {
                engine.set_pad_bpm(0, Some(BPM)).unwrap();
                engine.set_pad_timing_intent(0, fixture.timing).unwrap();
            });
            fixture.callback.drain();
            let uncertain = fixture.capture(36);
            let uncertain_ticket = fixture.prepare(&uncertain);
            fixture.with_engine(|engine| engine.publish_pad_copy(&uncertain_ticket).unwrap());
            let held_claim = match fixture.callback.controls.pop().unwrap() {
                ControlMessage::PadCopy(work) => {
                    // Exercise the real eligible claim method while deliberately withholding
                    // installation/ACK. No fixture assigns a phase or manufactures acceptance.
                    assert!(work.guard.claim());
                    work
                }
                _ => panic!("expected actual prepared Native Copy"),
            };
            assert_eq!(uncertain_ticket.publication_status(), "adopting");
            fixture.with_engine(|engine| {
                assert!(!engine.cancel_pad_copy(&uncertain).unwrap());
                assert!(!engine.reconcile_pad_copy(&uncertain_ticket).unwrap());
                assert!(engine.finish_pad_copy(&uncertain_ticket).is_err());
                assert!(engine.unload_sample(36).is_err());
                assert_ne!(engine.input_runtime_ownership.copy_reservation(36), 0);
                assert!(engine.sample_cache.lock().unwrap()[36].is_none());
            });
            assert!(fixture.callback.mixer.bank_for_measurement()[36].is_none());
            assert_eq!(held_claim.publication.status(), "pending");
            drop(held_claim);
        },
    );
}

struct CurrentDirectory(PathBuf);
impl Drop for CurrentDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

fn probe(engine: &Py<AudioEngine>, class: &str, input: Option<String>) -> Py<PyAny> {
    Python::attach(|py| {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        py.import("sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1(
                "insert",
                (0, repository.join("src").to_string_lossy().as_ref()),
            )
            .unwrap();
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!("public_complete_probe.py")).unwrap(),
            c"pad_copy_public_complete_probe.py",
            c"pad_copy_public_complete_probe",
        )
        .unwrap();
        let kind = module.getattr(class).unwrap();
        match input {
            Some(input) => kind.call1((engine.clone_ref(py), input)).unwrap().unbind(),
            None => kind.call1((engine.clone_ref(py),)).unwrap().unbind(),
        }
    })
}

fn fixture_input(fixture: &Fixture) -> String {
    let metadata = fixture.material.metadata();
    json!({"original_reference":metadata["new_reference"], "material_id":metadata["material_id"],
        "material_creation":metadata["material_creation"], "frames":FRAMES, "rate":RATE,
        "source_version":fixture.version, "pair":fixture.pair_json})
    .to_string()
}

fn wait_for(condition: impl FnMut() -> bool, description: &str) {
    wait_for_with_diagnostics(condition, description, String::new);
}

fn wait_for_with_diagnostics(
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

type ArtifactSnapshot =
    BTreeMap<PathBuf, (crate::audio_engine::project_assets::FileIdentity, String)>;

fn artifact_snapshot(root: &Path) -> ArtifactSnapshot {
    fn visit(root: &Path, path: &Path, result: &mut ArtifactSnapshot) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            assert!(!kind.is_symlink());
            if kind.is_dir() {
                visit(root, &entry.path(), result);
            } else {
                assert!(kind.is_file());
                assert!(
                    result.len() < 64,
                    "generated Copy fixture exceeded its artifact bound"
                );
                let path = entry.path();
                result.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    (
                        crate::audio_engine::project_assets::capture_identity(&path)
                            .unwrap()
                            .unwrap(),
                        format!("{:x}", Sha256::digest(fs::read(&path).unwrap())),
                    ),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

fn producer_process_has_exited(pid: u32) -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn GetExitCodeProcess(process: *mut std::ffi::c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    // SAFETY: query-only process handle; no application process is changed.
    let process = unsafe { OpenProcess(0x1000, 0, pid) };
    if process.is_null() {
        return std::io::Error::last_os_error().raw_os_error() == Some(87);
    }
    let mut code = 259_u32;
    // SAFETY: the opened handle is live and code is initialized output storage.
    let queried = unsafe { GetExitCodeProcess(process, &mut code) } != 0;
    // SAFETY: close this independently opened query handle exactly once.
    unsafe {
        CloseHandle(process);
    }
    queried && code != 259
}

fn restore_saved_in_new_process(root: &Path) {
    let parent: u32 = std::env::var(RESTORE_PARENT).unwrap().parse().unwrap();
    assert_ne!(parent, std::process::id());
    let handoff: Value =
        serde_json::from_slice(&fs::read(root.parent().unwrap().join(PRODUCER_HANDOFF)).unwrap())
            .unwrap();
    assert_eq!(handoff["producer_pid"].as_u64(), Some(u64::from(parent)));
    assert_eq!(handoff["producer_exited"], true);
    assert_eq!(
        fs::canonicalize(handoff["root"].as_str().unwrap()).unwrap(),
        fs::canonicalize(root).unwrap()
    );
    assert!(
        producer_process_has_exited(parent),
        "saved Copy producer is still alive"
    );
    let _previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(root).unwrap();
    Python::initialize();
    let before = artifact_snapshot(&root.join("samples"));
    let (engine, mut callback, _producer) = empty_engine();
    let engine = Python::attach(|py| Py::new(py, engine).unwrap());
    let probe = probe(&engine, "RestoreProbe", None);
    Python::attach(|py| probe.call_method0(py, "start").unwrap());
    assert!(callback.mixer.bank_for_measurement()[36].is_none());
    assert!(callback.mixer.bank_for_measurement()[215].is_none());
    wait_for(
        || {
            callback.drain();
            Python::attach(|py| {
                probe
                    .call_method0(py, "poll_sources")
                    .unwrap()
                    .extract(py)
                    .unwrap()
            })
        },
        "fresh-process public source restores did not receive their own cold ACKs",
    );
    callback.drain();
    Python::attach(|py| {
        let native = engine.borrow(py);
        assert!(native.sample_cache.lock().unwrap()[0].is_none());
        let leases = native.cold_leases.lock().unwrap();
        assert_ne!(
            leases[36].as_ref().unwrap().assignment_id(),
            leases[215].as_ref().unwrap().assignment_id()
        );
        assert!(!leases[36].as_ref().unwrap().assignment_retired());
        assert!(!leases[215].as_ref().unwrap().assignment_retired());
        drop(leases);
        for target in [36, 215] {
            let binding = native
                .current_input_runtime_pad_binding(target)
                .unwrap()
                .unwrap();
            assert!(binding.is_current());
            assert_eq!(native.pad_timing_intent(target).unwrap(), "tap");
        }
    });
    {
        let first = callback.mixer.bank_for_measurement()[36].as_ref().unwrap();
        let last = callback.mixer.bank_for_measurement()[215].as_ref().unwrap();
        assert!(Arc::ptr_eq(&first.samples, &last.samples));
        assert!(Arc::ptr_eq(
            &first.residency.as_ref().unwrap().source,
            &last.residency.as_ref().unwrap().source
        ));
        assert!(!Arc::ptr_eq(
            first.residency.as_ref().unwrap(),
            last.residency.as_ref().unwrap()
        ));
        assert_eq!((first.resident_start(), first.resident_end()), (START, END));
    }
    for target in [36, 215] {
        Python::attach(|py| probe.call_method1(py, "register_pair", (target,)).unwrap());
        callback.drain();
        Python::attach(|py| {
            probe
                .call_method1(py, "request_components", (target,))
                .unwrap()
        });
        wait_for(
            || {
                callback.drain();
                Python::attach(|py| {
                    probe
                        .call_method1(py, "components_ready", (target,))
                        .unwrap()
                        .extract(py)
                        .unwrap()
                })
            },
            "fresh-process selected pair did not receive its own registration/residency ACK",
        );
        Python::attach(|py| {
            let pairs = probe.bind(py).getattr("pairs").unwrap();
            let pair: Py<PreparedStemPair> = pairs.get_item(target).unwrap().extract().unwrap();
            let pair = pair.borrow(py);
            assert!(!pair.pair_reader.created_pcm && !pair.pair_reader.created_descriptor);
        });
        assert_eq!(
            callback.mixer.stem_demand_for_measurement(target).0,
            StemMixMode::AllStems
        );
        assert_eq!(callback.mixer.stem_demand_for_measurement(target).1, 0);
    }
    {
        let first = callback.mixer.stems_for_measurement()[36].as_ref().unwrap();
        let last = callback.mixer.stems_for_measurement()[215]
            .as_ref()
            .unwrap();
        for (a, b) in first.stems.iter().zip(&last.stems) {
            assert!(Arc::ptr_eq(&a.samples, &b.samples));
            assert!(!Arc::ptr_eq(
                a.residency.as_ref().unwrap(),
                b.residency.as_ref().unwrap()
            ));
        }
    }
    Python::attach(|py| probe.call_method0(py, "assert_restored").unwrap());
    assert_eq!(
        artifact_snapshot(&root.join("samples")),
        before,
        "restore changed a verified FileID/hash or duplicated a generated source/pair artifact"
    );
    Python::attach(|py| engine.borrow_mut(py).play_sample(36, 1.0, None).unwrap());
    callback.drain();
    callback.render(37);
    Python::attach(|py| probe.call_method0(py, "request_audible_mask").unwrap());
    wait_for(
        || {
            callback.drain();
            callback.render(37);
            Python::attach(|py| {
                probe
                    .call_method0(py, "audible_mask_ready")
                    .unwrap()
                    .extract(py)
                    .unwrap()
            })
        },
        "restored Copy mask lacks its own effective Native ACK",
    );
    assert_eq!(callback.mixer.stem_demand_for_measurement(36).1, 15);
    assert_eq!(callback.mixer.stem_demand_for_measurement(215).1, 0);
    let audio = callback.render(1025);
    assert!(
        audio.iter().any(|sample| sample.abs() > 1e-8),
        "fresh-process restored Copy rendered no audio after its own mask ACK"
    );
    let voice = callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(36))
        .unwrap();
    assert_eq!(voice.source_timing.legacy_period_seconds, Some(60.0 / BPM));
    assert_eq!(
        voice.source_timing.legacy_origin_frame,
        (ANCHOR * f64::from(RATE)).round_ties_even() + OFFSET as f64
    );
    let (original, cache_path, cold_gone) = Python::attach(|py| {
        let native = engine.borrow(py);
        let lease = native.cold_leases.lock().unwrap()[215].clone().unwrap();
        (
            lease.original_path.clone(),
            lease.cache_path.clone(),
            lease.reader_lifetime_probe_for_test(),
        )
    });
    let (pair_path, wav_path, pcm_path, pair_gone) = Python::attach(|py| {
        let pairs = probe.bind(py).getattr("pairs").unwrap();
        let pair: Py<PreparedStemPair> = pairs.get_item(215).unwrap().extract().unwrap();
        let pair = pair.borrow(py);
        (
            root.join(&pair.descriptor_reference),
            pair.wav_path.clone(),
            root.join(&pair.pair_reader.descriptor.pcm_generation),
            pair.reader_lifetime_for_test(),
        )
    });
    Python::attach(|py| probe.call_method0(py, "hold_last_reader").unwrap());
    println!("{RESTORE_READY}");
    Python::attach(|py| probe.call_method1(py, "unload", (36,)).unwrap());
    callback.drain();
    assert!(callback.mixer.bank_for_measurement()[36].is_none());
    assert!(original.is_file() && wav_path.is_dir() && pcm_path.is_dir() && pair_path.is_file());
    Python::attach(|py| {
        assert!(
            engine
                .borrow(py)
                .current_input_runtime_pad_binding(215)
                .unwrap()
                .unwrap()
                .is_current()
        )
    });
    Python::attach(|py| probe.call_method1(py, "unload", (215,)).unwrap());
    callback.drain();
    assert!(callback.mixer.bank_for_measurement()[215].is_none());
    Python::attach(|py| probe.call_method0(py, "assert_held_reader").unwrap());
    assert!(original.is_file() && wav_path.is_dir() && pcm_path.is_dir() && pair_path.is_file());
    Python::attach(|py| probe.call_method0(py, "release_last_reader").unwrap());
    wait_for_with_diagnostics(
        || {
            Python::attach(|py| {
                probe.call_method0(py, "poll_retirements").unwrap();
            });
            pair_gone.strong_count() == 0
                && cold_gone()
                && !original.exists()
                && !cache_path.exists()
                && !wav_path.exists()
                && !pcm_path.exists()
                && !pair_path.exists()
        },
        "last genuine restored-C reader did not release its verified original/PCM/pair through the asset worker",
        || {
            let native = Python::attach(|py| {
                let native = engine.borrow(py);
                let source_slots = {
                    let samples = native.sample_cache.lock().unwrap();
                    [36, 215].map(|id| (id, samples[id].is_some()))
                };
                let lease_slots = {
                    let leases = native.cold_leases.lock().unwrap();
                    [36, 215].map(|id| (id, leases[id].is_some()))
                };
                let accepted_components = {
                    let owners = native.resident_stem_cache.lock().unwrap();
                    [36, 215].map(|id| (id, owners[id].accepted.is_some()))
                };
                let histories = [36, 215]
                    .map(|id| (id, resident_relocation::history_count_for_test(&native, id)));
                json!({"source_slots":source_slots,"lease_slots":lease_slots,
                    "accepted_components":accepted_components,"histories":histories,
                    "cleanup":native.project_asset_cleanup_status().unwrap(),
                    "cleanup_admission":native.project_asset_cleanup_admission_status()})
            });
            let python = Python::attach(|py| {
                probe
                    .call_method0(py, "retirement_diagnostics")
                    .unwrap()
                    .extract::<String>(py)
                    .unwrap()
            });
            json!({"pair_strong_count":pair_gone.strong_count(),"cold_readers_gone":cold_gone(),
                "paths_exist":{"original":original.exists(),"cache":cache_path.exists(),
                    "wav":wav_path.exists(),"pcm":pcm_path.exists(),"descriptor":pair_path.exists()},
                "native":native,"python":python}).to_string()
        },
    );
    Python::attach(|py| probe.call_method0(py, "close").unwrap());
}

fn coordinate_saved_copy_processes() {
    // This coordinator owns paths and scalar evidence only. All Native owners,
    // including the producer's process-global saved cold claims, end at exit.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let directory = tempfile::Builder::new()
        .prefix("native-copy-processes-")
        .tempdir_in(workspace.join("scratch"))
        .unwrap();
    let test = test_name(
        "productive_arrangement_real_native_ack_precedes_config_and_shared_asset_publication",
    );
    let producer = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env(PRODUCER_PARENT, directory.path())
        .env_remove(RESTORE_ROOT)
        .env_remove(RESTORE_PARENT)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let producer_pid = producer.id();
    let output = producer.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Copy producer failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "Copy producer did not execute:\n{stdout}\n{stderr}"
    );
    assert!(stderr.contains(&format!("actual Native Copy child PID={producer_pid}")));
    assert!(producer_process_has_exited(producer_pid));

    let handoff_path = directory.path().join(PRODUCER_HANDOFF);
    let mut handoff: Value = serde_json::from_slice(&fs::read(&handoff_path).unwrap()).unwrap();
    assert_eq!(
        handoff["producer_pid"].as_u64(),
        Some(u64::from(producer_pid))
    );
    let root = PathBuf::from(handoff["root"].as_str().unwrap());
    let contained_root = fs::canonicalize(&root).unwrap();
    let contained_parent = fs::canonicalize(directory.path()).unwrap();
    assert_ne!(contained_root, contained_parent);
    assert!(contained_root.starts_with(&contained_parent));
    assert_eq!(contained_root.parent(), Some(contained_parent.as_path()));
    // Only this actual successful wait authorizes the restore process precondition.
    handoff["producer_exited"] = json!(true);
    handoff["coordinator_pid"] = json!(std::process::id());
    fs::write(&handoff_path, serde_json::to_vec(&handoff).unwrap()).unwrap();

    let restore = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env(RESTORE_ROOT, &root)
        .env(RESTORE_PARENT, producer_pid.to_string())
        .env_remove(PRODUCER_PARENT)
        .current_dir(&root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let restore_pid = restore.id();
    assert_ne!(producer_pid, restore_pid);
    assert_ne!(restore_pid, std::process::id());
    let output = restore.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Copy fresh-process restore/final-reader failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "Copy restore did not execute:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains(RESTORE_READY),
        "Copy restore lacked its actual own-ACK checkpoint:\n{stdout}\n{stderr}"
    );
    assert!(stderr.contains(&format!("actual Native Copy child PID={restore_pid}")));
}

#[test]
fn productive_arrangement_real_native_ack_precedes_config_and_shared_asset_publication() {
    if std::env::var_os(RESTORE_ROOT).is_none() && std::env::var_os(PRODUCER_PARENT).is_none() {
        coordinate_saved_copy_processes();
        return;
    }
    isolated(
        "productive_arrangement_real_native_ack_precedes_config_and_shared_asset_publication",
        || {
            if let Some(root) = std::env::var_os(RESTORE_ROOT) {
                restore_saved_in_new_process(Path::new(&root));
                return;
            }
            let mut fixture = Fixture::new(true, false, "tap");
            let previous = CurrentDirectory(std::env::current_dir().unwrap());
            std::env::set_current_dir(fixture.directory.path()).unwrap();
            let probe = probe(&fixture.engine, "Probe", Some(fixture_input(&fixture)));
            let source_generation = fixture.source_voice().generation;
            for target in [36, 215] {
                Python::attach(|py| probe.call_method1(py, "begin", (target,)).unwrap());
                assert!(matches!(
                    fixture.callback.controls.peek().unwrap(),
                    ControlMessage::PadCopy(_)
                ));
                assert!(fixture.callback.mixer.bank_for_measurement()[target].is_none());
                Python::attach(|py| probe.call_method0(py, "assert_waiting").unwrap());
                assert_eq!(fixture.source_voice().generation, source_generation);
                assert_eq!(fixture.callback.drain(), 1);
                assert!(fixture.callback.mixer.bank_for_measurement()[target].is_some());
                Python::attach(|py| probe.call_method1(py, "complete", (target,)).unwrap());
                assert_eq!(
                    fixture.with_engine(|engine| engine
                        .input_runtime_ownership
                        .copy_reservation(target)),
                    0
                );
                assert_eq!(
                    fixture.callback.mixer.stem_demand_for_measurement(target).1,
                    0
                );
            }
            Python::attach(|py| probe.call_method0(py, "origin_unloaded").unwrap());
            fixture.callback.drain();
            assert!(fixture.callback.mixer.bank_for_measurement()[0].is_none());
            for target in [36, 215] {
                let source = fixture.callback.mixer.bank_for_measurement()[target]
                    .as_ref()
                    .unwrap();
                assert!(Arc::ptr_eq(
                    &source.samples,
                    &fixture.callback.mixer.bank_for_measurement()[36]
                        .as_ref()
                        .unwrap()
                        .samples
                ));
                fixture.with_engine(|engine| {
                    assert!(
                        engine
                            .current_input_runtime_pad_binding(target)
                            .unwrap()
                            .unwrap()
                            .is_current()
                    )
                });
            }
            let before = artifact_snapshot(&fixture.directory.path().join("samples"));
            Python::attach(|py| probe.call_method0(py, "close").unwrap());
            drop(probe);
            drop(previous);
            let root = fixture.keep_directory_for_process_exit();
            assert_eq!(artifact_snapshot(&root.join("samples")), before);
            let parent = PathBuf::from(std::env::var_os(PRODUCER_PARENT).unwrap());
            assert_eq!(
                fs::canonicalize(&root).unwrap().parent(),
                Some(fs::canonicalize(&parent).unwrap().as_path())
            );
            fs::write(
                parent.join(PRODUCER_HANDOFF),
                serde_json::to_vec(&json!({
                    "root":root,"producer_pid":std::process::id()
                }))
                .unwrap(),
            )
            .unwrap();
        },
    );
}

#[test]
fn productive_shutdown_keeps_actual_worker_custody_and_settles_acknowledged_copy() {
    isolated(
        "productive_shutdown_keeps_actual_worker_custody_and_settles_acknowledged_copy",
        || {
            let mut fixture = Fixture::new(true, false, "tap");
            let _previous = CurrentDirectory(std::env::current_dir().unwrap());
            std::env::set_current_dir(fixture.directory.path()).unwrap();
            let probe = probe(&fixture.engine, "Probe", Some(fixture_input(&fixture)));
            let generation = fixture.source_voice().generation;
            Python::attach(|py| probe.call_method0(py, "shutdown_before_worker").unwrap());
            assert_eq!(
                fixture.with_engine(|engine| engine.input_runtime_ownership.copy_reservation(36)),
                0
            );
            assert!(fixture.callback.mixer.bank_for_measurement()[36].is_none());
            assert_eq!(fixture.source_voice().generation, generation);
            Python::attach(|py| probe.call_method1(py, "begin", (36,)).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            assert!(fixture.callback.mixer.bank_for_measurement()[36].is_some());
            Python::attach(|py| probe.call_method0(py, "shutdown_after_native_ack").unwrap());
            assert_eq!(
                fixture.with_engine(|engine| engine.input_runtime_ownership.copy_reservation(36)),
                0
            );
            assert_eq!(fixture.source_voice().generation, generation);
            Python::attach(|py| probe.call_method0(py, "close").unwrap());
        },
    );
}
