//! Real retained QM evidence through public Automatic Copy, persistence and fresh restore.
//! The generated pulse fixture runs QM once before Copy; no CPAL/device or model inference.
#![cfg(windows)]

#[path = "qm_complete_tests/copied_loop_custody_tests.rs"]
mod copied_loop_custody_tests;

use crate::audio_engine::AudioEngine;
use crate::audio_engine::audio_stream::{
    drain_control_messages, drain_parameter_messages, render_scheduled_audio,
};
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirementWorker, RtAudioBufferRetirement, create_audio_buffer_retirement,
};
use crate::audio_engine::constant_timing::{self, ConstantTimingTicket};
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::material_migration::{
    PreparedMigrationMaterial, prepare_material_complete_for_test,
};
use crate::audio_engine::material_migration_control::{adopt_for_format, prepared_for_test};
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::native_owned_work::{PreparationObservationScope, WorkPoint};
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{
    AudioMessage, ControlMessage, ControlParameterMessage, LoaderEvent, TriggerQuantization,
};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use rtrb::{Consumer, Producer, RingBuffer};
use serde_json::{Value, json};
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

const CHILD: &str = "FLITZI_QM_COPY_COMPLETE_CHILD";
const TEST: &str = "actual_qm_automatic_copy_own_ack_persistence_and_fresh_engine_restore";
const START: usize = 128;
const END: usize = 2048;

fn isolated(body: impl FnOnce()) {
    if std::env::var_os(CHILD).is_some() {
        body();
        eprintln!("real QM Copy child PID={}", std::process::id());
        return;
    }
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
        "QM Copy child failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "QM Copy child did not run:\n{stdout}\n{stderr}"
    );
    assert!(
        stderr.contains("real QM Copy child PID="),
        "QM Copy fixture was skipped:\n{stdout}\n{stderr}"
    );
}

struct CurrentDirectory(PathBuf);

impl Drop for CurrentDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

struct Callback {
    mixer: Box<RtMixer>,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
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

    fn render(&mut self, frames: usize) {
        let mut output = vec![0.0; frames];
        render_scheduled_audio(
            &mut self.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            self.frame,
            1,
            &mut self.transport,
            &mut self.feedback,
            &mut self.retirement,
        );
        self.frame += frames as u64;
        assert!(output.iter().all(|value| value.is_finite()));
    }
}

struct Harness {
    engine: Py<AudioEngine>,
    callback: Callback,
    rate: u32,
}

impl Harness {
    fn new(rate: u32) -> Self {
        let mut engine = AudioEngine::new().unwrap();
        let (producer, controls) = RingBuffer::new(64);
        let producer = Arc::new(Mutex::new(producer));
        let (parameter_producer, parameters) = RingBuffer::new(64);
        let (feedback, messages) = RingBuffer::new(128);
        engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(messages))));
        engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameter_producer)));
        engine.test_output_format = Some((rate, 1));
        let mut mixer = Box::new(RtMixer::new(1, rate as f32));
        mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
        mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
        mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
        let (retirement, worker) = create_audio_buffer_retirement();
        Self {
            engine: Python::attach(|py| Py::new(py, engine).unwrap()),
            callback: Callback {
                mixer,
                producer,
                controls,
                parameters,
                feedback,
                scheduler: FixedCapacityScheduler::new(),
                transport: TransportTimeline::new(rate),
                quantization: TriggerQuantization::Immediate,
                retirement,
                _retirement_worker: worker,
                frame: 0,
            },
            rate,
        }
    }

    fn with_engine<T>(&self, operation: impl FnOnce(&mut AudioEngine) -> T) -> T {
        Python::attach(|py| operation(&mut self.engine.borrow_mut(py)))
    }

    fn load(&mut self, id: usize, material: &PreparedMigrationMaterial, root: &Path) {
        let preparation = self.with_engine(|engine| {
            prepared_for_test(
                engine,
                PreparedMigrationMaterial::from_current(
                    root,
                    material.sample.clone(),
                    material.lease.clone(),
                )
                .unwrap(),
                root,
            )
            .unwrap()
        });
        let admitted = self.with_engine(|engine| {
            adopt_for_format(
                engine,
                id,
                &preparation,
                self.callback.producer.clone(),
                (1, self.rate, root.to_path_buf()),
            )
            .unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.callback.drain();
            if self.with_engine(|engine| engine.cold_loading[id].load(Ordering::Acquire)) == 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "QM fixture source did not receive its own cold ACK"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let success = self.with_engine(|engine| {
            let mut success = false;
            while let Ok(event) = engine
                .loader_rx
                .lock()
                .unwrap()
                .try_recv()
                .map(LoaderEvent::into_test_payload)
            {
                match event {
                    LoaderEvent::Success { request_id, .. }
                        if request_id == admitted.request_id() =>
                    {
                        success = true
                    }
                    LoaderEvent::Error { error, .. } => panic!("QM source failed: {error:?}"),
                    _ => {}
                }
            }
            success
        });
        assert!(success && admitted.is_current().unwrap());
        preparation.release_preparation().unwrap();
    }

    fn current(&self, id: usize) -> Option<Value> {
        Python::attach(|py| {
            self.engine
                .borrow(py)
                .current_constant_timing(py, id)
                .unwrap()
                .map(|metadata| {
                    let encoded: String = py
                        .import("json")
                        .unwrap()
                        .call_method1("dumps", (metadata,))
                        .unwrap()
                        .extract()
                        .unwrap();
                    serde_json::from_str(&encoded).unwrap()
                })
        })
    }

    fn export(&self, id: usize, path: &str) -> Option<String> {
        Python::attach(|py| {
            AudioEngine::export_current_constant_timing(
                self.engine.bind(py),
                py,
                id,
                path.to_owned(),
            )
            .unwrap()
        })
    }

    fn prepare_saved(&mut self, id: usize, envelope: &str, path: &str) -> ConstantTimingTicket {
        self.with_engine(|engine| engine.set_pad_timing_intent(id, "automatic").unwrap());
        assert_eq!(self.callback.drain(), 1);
        let saved = self.with_engine(|engine| {
            engine
                .capture_saved_constant_timing(id, envelope, path.to_owned())
                .unwrap()
        });
        let ticket = Python::attach(|py| {
            AudioEngine::restore_constant_timing(self.engine.bind(py), py, &saved).unwrap()
        });
        assert_eq!(ticket.publication_status().unwrap(), "pending");
        assert!(self.current(id).is_none());
        assert!(self.export(id, path).is_none());
        self.with_engine(|engine| {
            assert_eq!(
                engine
                    .current_timing_acknowledgements
                    .current_for_slot(&engine.input_runtime_ownership, id),
                0
            );
            let binding = engine
                .current_input_runtime_pad_binding(id)
                .unwrap()
                .unwrap();
            assert!(binding.is_current());
            assert!(!binding.available());
        });
        ticket
    }

    fn acknowledge_saved(&mut self, id: usize, ticket: &ConstantTimingTicket) -> Value {
        assert_eq!(self.callback.drain(), 1);
        assert_eq!(ticket.publication_status().unwrap(), "accepted");
        let metadata = self.current(id).unwrap();
        self.with_engine(|engine| {
            assert_eq!(
                engine
                    .current_timing_acknowledgements
                    .current_for_slot(&engine.input_runtime_ownership, id),
                metadata["publication_epoch"].as_u64().unwrap()
            );
            assert!(
                engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap()
                    .available()
            );
        });
        metadata
    }
}

const PROBE: &str = r#"
import json
import struct
from pathlib import Path

from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.pad_arrangement import PadArrangementController
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.models import (
    BeatGrid, PadContentIdentity, PadStemIntent, ProjectState, SampleAnalysis, SessionState,
)


class Probe:
    def __init__(self, audio, fixture_json):
        fixture = json.loads(fixture_json)
        self.audio = audio
        self.accepted = fixture['accepted']
        period = struct.unpack('>d', bytes.fromhex(self.accepted['record']['period_bits']))[0]
        project = ProjectState(key_lock=False, selected_pad=0, volume=0.7)
        project.sample_paths[0] = fixture['source_reference']
        project.sample_durations[0] = fixture['frames'] / fixture['rate']
        project.pad_content[0] = PadContentIdentity(
            instance_id='8' * 32, material_id=fixture['material_id'],
            material_creation=fixture['material_creation'],
        )
        project.sample_analysis[0] = SampleAnalysis(
            bpm=60.0 / period, key='', beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]),
            accepted_timing=self.accepted,
        )
        project.pad_timing_intent[0] = 'automatic'
        project.pad_key_lock[0] = False
        project.pad_loop_auto[0] = False
        project.pad_loop_start_s[0] = 128 / fixture['rate']
        project.pad_loop_end_s[0] = 2048 / fixture['rate']
        project.pad_loop_bars[0] = 1.5
        project.pad_grid_anchor_s[0] = 0.003
        project.pad_grid_offset_samples[0] = 7
        project.pad_gain_db[0] = -7.0
        project.pad_eq_low_db[0] = 3.0
        project.pad_eq_mid_db[0] = -4.0
        project.pad_eq_high_db[0] = 1.5
        project.pad_stem_intent[0] = PadStemIntent(
            enabled_mask=0, last_custom_mask=5, display_mode='custom',
        )
        self.project = project
        self.session = SessionState()
        self.persistence = ProjectPersistence(project)
        self.persistence.config_path = Path('samples/qm-copy.json')
        self.persistence.bind_audio(audio)
        self.assets = ProjectAssetLifecycle(project, audio)
        self.assets.sync_assignments()
        self.persistence.flush()
        self.callbacks = []
        self.arrangement = PadArrangementController(
            self.persistence, self.session, audio, self.assets,
            stem_task_runner=lambda work: work(), on_copy_committed=self.committed,
        )

    def committed(self, target, ticket):
        receipt = json.loads(ticket.metadata_json())
        assert receipt['status'] == 'accepted' and receipt['control_mirrored']
        current = self.audio.current_constant_timing(target)
        assert current['pad_id'] == target
        assert current['source_generation'] == receipt['source_generation']
        assert current['accepted_request_id'] == receipt['source_generation']
        exported = self.audio.export_current_constant_timing(target, self.project.sample_paths[target])
        assert json.loads(exported) == self.accepted
        self.callbacks.append(target)

    def begin(self, target):
        self.target = target
        self.before = self.persistence.config_path.read_bytes()
        assert self.audio.current_constant_timing(0) is not None
        assert self.audio.current_constant_timing(target) is None
        assert self.arrangement.copy_to_empty(0, target)
        self.arrangement.poll()
        self.waiting()

    def waiting(self):
        for _ in range(3):
            self.arrangement.poll()
        assert self.arrangement.active
        assert self.project.sample_paths[self.target] is None
        assert self.project.pad_content[self.target] is None
        assert self.target not in self.callbacks
        assert self.persistence.config_path.read_bytes() == self.before
        try:
            self.persistence.flush()
        except PersistenceFenceError:
            pass
        else:
            raise AssertionError('source timing ACK bypassed complete Copy ACK')

    def complete(self, target):
        self.arrangement.poll()
        assert self.arrangement.status == 'completed', self.arrangement.error
        assert not self.arrangement.active
        assert self.callbacks[-1] == target
        saved = json.loads(self.persistence.config_path.read_text())
        for sample_id in (0, *self.callbacks):
            assert saved['sample_analysis'][sample_id]['accepted_timing'] == self.accepted
            assert saved['pad_timing_intent'][sample_id] == 'automatic'
            assert saved['manual_bpm'][sample_id] is None
        assert saved['pad_stem_intent'][target]['enabled_mask'] == 0
        for field in ('pad_gain_db', 'pad_eq_low_db', 'pad_eq_mid_db', 'pad_eq_high_db',
                      'pad_loop_start_s', 'pad_loop_end_s', 'pad_loop_auto', 'pad_loop_bars',
                      'pad_grid_anchor_s', 'pad_grid_offset_samples', 'pad_stem_intent'):
            assert saved[field][target] == saved[field][0], field
        assert saved['pad_content'][target]['copied_from'] == '8' * 32
        assert saved['pad_content'][target]['instance_id'] != '8' * 32
        assert self.project.selected_pad == 0 and self.project.volume == 0.7
        assert not self.session.active_sample_ids
        reopened = ProjectPersistence.from_config_path(self.persistence.config_path)
        assert reopened.load_error is None
        assert reopened.project.sample_analysis[target].accepted_timing.model_dump() == self.accepted

    def close(self):
        assert not self.arrangement.active
        self.arrangement = None
        self.assets.release_saved_assignments()
        self.assets = None
        self.persistence = None
        self.audio = None
"#;

fn probe(harness: &Harness, material: &PreparedMigrationMaterial, envelope: &Value) -> Py<PyAny> {
    let metadata = material.metadata();
    let input = json!({
        "source_reference": metadata["new_reference"], "material_id": metadata["material_id"],
        "material_creation": metadata["material_creation"], "rate": harness.rate,
        "frames": envelope["record"]["evidence"]["binding"]["frame_count"], "accepted": envelope,
    });
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
            &CString::new(PROBE).unwrap(),
            c"qm_copy_complete_probe.py",
            c"qm_copy_complete_probe",
        )
        .unwrap();
        module
            .getattr("Probe")
            .unwrap()
            .call1((harness.engine.clone_ref(py), input.to_string()))
            .unwrap()
            .unbind()
    })
}

fn copy_target(harness: &mut Harness, probe: &Py<PyAny>, target: usize, source_metadata: &Value) {
    let source =
        harness.with_engine(|engine| engine.sample_cache.lock().unwrap()[0].clone().unwrap());
    let base_epoch =
        harness.with_engine(|engine| engine.prepared_source_epochs[target].load(Ordering::Acquire));
    let timing_preparations = Arc::new(AtomicUsize::new(0));
    let observed = timing_preparations.clone();
    let _observation = PreparationObservationScope::install(Some(Box::new(move |point| {
        if point == WorkPoint::TimingPcm {
            observed.fetch_add(1, Ordering::Relaxed);
        }
    })));
    Python::attach(|py| probe.call_method1(py, "begin", (target,)).unwrap());
    assert_eq!(
        timing_preparations.load(Ordering::Relaxed),
        0,
        "Copy must reuse accepted QM evidence without preparing new analysis"
    );
    let expected_projection = match harness.callback.controls.peek().unwrap() {
        ControlMessage::PadCopy(work) => {
            let projection = work
                .projection
                .expect("Automatic Copy must carry accepted timing");
            assert_eq!(work.target, target);
            assert_eq!(work.guard.phase.load(Ordering::Acquire), 0);
            assert_eq!(work.publication.status(), "pending");
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
            assert_eq!(projection.publication_epoch, base_epoch + 1);
            assert_eq!(
                projection.period_seconds.to_bits(),
                source_metadata["period_seconds_per_quarter"]
                    .as_f64()
                    .unwrap()
                    .to_bits()
            );
            assert_eq!(
                projection.origin_seconds.to_bits(),
                source_metadata["origin_seconds"]
                    .as_f64()
                    .unwrap()
                    .to_bits()
            );
            assert!(!Arc::ptr_eq(
                &work.guard.source_epoch,
                &work.guard.target_epoch
            ));
            assert!(Arc::ptr_eq(&work.sample.samples, &source.samples));
            assert!(Arc::ptr_eq(
                &work.sample.residency.as_ref().unwrap().source,
                &source.residency.as_ref().unwrap().source
            ));
            assert!(!Arc::ptr_eq(
                work.sample.residency.as_ref().unwrap(),
                source.residency.as_ref().unwrap()
            ));
            projection
        }
        other => panic!("Automatic Copy did not enqueue its complete payload: {other:?}"),
    };
    assert_eq!(harness.current(0).unwrap(), *source_metadata);
    assert!(harness.current(target).is_none());
    harness.with_engine(|engine| {
        assert_eq!(
            engine.prepared_source_epochs[target].load(Ordering::Acquire),
            base_epoch
        );
        assert_eq!(
            engine
                .current_timing_acknowledgements
                .current_for_slot(&engine.input_runtime_ownership, target),
            0
        );
    });
    Python::attach(|py| probe.call_method0(py, "waiting").unwrap());
    assert_eq!(harness.callback.drain(), 1);
    assert!(harness.callback.mixer.bank_for_measurement()[target].is_some());
    assert_eq!(
        harness.callback.mixer.loop_region_frames(target),
        (START, Some(END))
    );
    assert!(
        !harness
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == target)
    );
    assert!(
        harness.current(target).is_none(),
        "RT ACK alone must await control reconciliation"
    );
    harness.with_engine(|engine| {
        assert_eq!(
            engine
                .current_timing_acknowledgements
                .current_for_slot(&engine.input_runtime_ownership, target),
            expected_projection.publication_epoch,
        )
    });
    Python::attach(|py| probe.call_method1(py, "complete", (target,)).unwrap());
    let current = harness.current(target).unwrap();
    for name in [
        "revision",
        "raw_revision",
        "period_seconds_per_quarter",
        "origin_seconds",
        "frame_count",
        "sample_rate_hz",
    ] {
        assert_eq!(
            current[name], source_metadata[name],
            "copied Automatic metadata {name}"
        );
    }
    assert_eq!(current["pad_id"], json!(target));
    assert_eq!(
        current["publication_epoch"],
        json!(expected_projection.publication_epoch)
    );
    harness.with_engine(|engine| {
        assert!(constant_timing::copy_records_share_accepted_for_test(
            engine, 0, target
        ));
        assert_eq!(engine.input_runtime_ownership.copy_reservation(target), 0);
        assert!(
            engine
                .current_input_runtime_pad_binding(target)
                .unwrap()
                .unwrap()
                .available()
        );
        let leases = engine.cold_leases.lock().unwrap();
        assert_ne!(
            leases[0].as_ref().unwrap().assignment_id(),
            leases[target].as_ref().unwrap().assignment_id()
        );
    });
    assert_eq!(harness.current(0).unwrap(), *source_metadata);
}

#[test]
fn actual_qm_automatic_copy_own_ack_persistence_and_fresh_engine_restore() {
    isolated(|| {
        Python::initialize();
        let (qm_original, historical_json) = constant_timing::migration_test_fixture();
        let historical: Value = serde_json::from_str(&historical_json).unwrap();
        let binding = &historical["record"]["evidence"]["binding"];
        let rate = u32::try_from(binding["sample_rate_hz"].as_u64().unwrap()).unwrap();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let directory = tempfile::Builder::new()
            .prefix("qm-copy-complete-")
            .tempdir_in(workspace.join("scratch"))
            .unwrap();
        let root = directory.path().join("samples");
        fs::create_dir(&root).unwrap();
        let original = root.join("qm-source.wav");
        fs::copy(qm_original, &original).unwrap();
        let _previous = CurrentDirectory(std::env::current_dir().unwrap());
        std::env::set_current_dir(directory.path()).unwrap();
        let (canonical, old_ownership) = {
            let material =
                prepare_material_complete_for_test(&root, &original, rate, 1, &|| false).unwrap();
            assert_eq!(
                material.sample.frame_count() as u64,
                binding["frame_count"].as_u64().unwrap()
            );
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
            let ticket = harness.prepare_saved(0, &historical_json, &canonical);
            let source_metadata = harness.acknowledge_saved(0, &ticket);
            assert_ne!(
                source_metadata["accepted_request_id"],
                binding["job"]["request_id"]
            );
            assert_ne!(
                source_metadata["source_generation"],
                binding["job"]["source_generation"]
            );
            assert_ne!(source_metadata["source_id"], binding["job"]["source_id"]);
            assert_eq!(harness.export(0, &canonical).unwrap(), historical_json);
            let probe = probe(&harness, &material, &historical);
            for target in [36, 215] {
                copy_target(&mut harness, &probe, target, &source_metadata);
                assert_eq!(harness.export(target, &canonical).unwrap(), historical_json);
            }
            let weak =
                harness.with_engine(|engine| Arc::downgrade(&engine.input_runtime_ownership));
            let Harness {
                engine, callback, ..
            } = harness;
            Python::attach(|py| {
                probe.call_method0(py, "close").unwrap();
                // Detached Py<T> drops defer decref until the next attachment.
                // Release these owned Python handles now so AudioEngine::drop
                // actually closes and joins its workers before the Weak oracle.
                probe.drop_ref(py);
                engine.drop_ref(py);
            });
            // The mixer and retirement worker retain the other actual Native
            // owners; dropping the callback joins retirement before proceeding.
            drop(callback);
            (canonical, weak)
        };
        assert!(
            old_ownership.upgrade().is_none(),
            "fresh restore must outlive every old Native authority owner"
        );
        let saved: Value =
            serde_json::from_slice(&fs::read(root.join("qm-copy.json")).unwrap()).unwrap();
        let target_json = saved["sample_analysis"][215]["accepted_timing"].to_string();
        assert_eq!(
            serde_json::from_str::<Value>(&target_json).unwrap(),
            historical
        );
        let material =
            prepare_material_complete_for_test(&root, Path::new(&canonical), rate, 1, &|| false)
                .unwrap();
        let mut fresh = Harness::new(rate);
        assert_eq!(
            fresh.with_engine(|engine| engine
                .current_timing_acknowledgements
                .current_for_slot(&engine.input_runtime_ownership, 215)),
            0
        );
        fresh.load(215, &material, &root);
        fresh.with_engine(|engine| {
            engine
                .set_pad_loop_region(
                    215,
                    saved["pad_loop_start_s"][215].as_f64().unwrap(),
                    saved["pad_loop_end_s"][215].as_f64(),
                )
                .unwrap();
            engine
                .set_pad_gain(215, saved["pad_gain_db"][215].as_f64().unwrap() as f32)
                .unwrap();
            engine.set_pad_eq(215, 3.0, -4.0, 1.5).unwrap();
        });
        fresh.callback.drain();
        let ticket = fresh.prepare_saved(215, &target_json, &canonical);
        let current = fresh.acknowledge_saved(215, &ticket);
        assert_eq!(current["pad_id"], json!(215));
        assert_eq!(
            current["revision"],
            historical["record"]["accepted_revision"]
        );
        assert_ne!(current["source_id"], binding["job"]["source_id"]);
        assert_eq!(
            fresh.callback.mixer.loop_region_frames(215),
            (START, Some(END))
        );
        assert_eq!(fresh.export(215, &canonical).unwrap(), historical_json);
        fresh.with_engine(|engine| engine.play_sample(215, 1.0, None).unwrap());
        let ControlMessage::CapturePadStartTarget(start) = fresh.callback.controls.peek().unwrap()
        else {
            panic!("fresh public launch must retain its original source owner");
        };
        let start = start.clone();
        let effect_frame = fresh.callback.frame;
        fresh.callback.drain();
        assert!(start.accepted(), "reason={:?}", start.reason());
        assert_eq!(start.target_frame(), Some(effect_frame));
        assert_eq!(start.execution_frame(), Some(effect_frame));
        fresh.callback.render(17);
        assert_eq!(start.state(), "applied", "reason={:?}", start.reason());
        assert_eq!(start.effective_frame(), Some(effect_frame));
        let voice = fresh
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(215))
            .unwrap();
        assert_eq!(
            voice.source_timing.accepted.unwrap().publication_epoch,
            current["publication_epoch"].as_u64().unwrap()
        );
    });
}
