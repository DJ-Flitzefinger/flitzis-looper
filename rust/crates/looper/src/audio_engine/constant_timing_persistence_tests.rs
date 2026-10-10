use super::*;
use crate::audio_engine::constant_timing::tests::{
    PERIOD, RATE, accept_message, acknowledged_mixer, decision, origin, queue, source, test_engine,
};
use crate::audio_engine::sample_loader::decode_audio_file_to_sample_buffer;
use std::sync::OnceLock;

// A test-only control facade substitutes in-memory callback construction for
// device startup. Its save exports execute the actual native verifier/owner.
#[pyclass]
struct NativeSaveOwner {
    engine: Arc<AudioEngine>,
}

#[pymethods]
impl NativeSaveOwner {
    fn export_current_constant_timing(
        &self,
        py: Python<'_>,
        id: usize,
        source_path: String,
    ) -> PyResult<Option<String>> {
        py.detach(|| export_current(&self.engine, id, source_path))
            .map_err(PyValueError::new_err)
    }
    fn pad_timing_intent(&self, id: usize) -> PyResult<&'static str> {
        self.engine.pad_timing_intent(id)
    }
}

fn actual_project_file_roundtrip(engine: Arc<AudioEngine>, path: &str, config: &Path) -> String {
    Python::attach(|py| {
        let locals = PyDict::new(py);
        locals
            .set_item(
                "native_owner",
                Py::new(py, NativeSaveOwner { engine }).unwrap(),
            )
            .unwrap();
        locals.set_item("source_path", path).unwrap();
        locals
            .set_item("config_path", config.to_string_lossy().as_ref())
            .unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        locals
            .set_item(
                "source_modules",
                repo.join("src").to_string_lossy().as_ref(),
            )
            .unwrap();
        let code=std::ffi::CString::new(r#"
import sys
from pathlib import Path
sys.path.insert(0, source_modules)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import ProjectState, SampleAnalysis, BeatGrid
project = ProjectState()
project.sample_paths[0] = source_path
project.pad_timing_intent[0] = 'automatic'
project.sample_analysis[0] = SampleAnalysis(bpm=119.999, key='8A', beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]))
persistence = ProjectPersistence(project)
persistence.config_path = Path(config_path)
persistence.bind_audio(native_owner)
persistence.flush()
loaded = ProjectPersistence.from_config_path(Path(config_path)).project
assert loaded.pad_timing_intent[0] == 'automatic'
assert loaded.sample_analysis[0].key == '8A'
assert loaded.sample_analysis[0].accepted_timing is not None
roundtripped = loaded.sample_analysis[0].accepted_timing.model_dump_json()
assert not tuple(Path(config_path).parent.glob('.flitzis_looper.config.json.*.tmp'))
"#).unwrap();
        py.run(&code, Some(&locals), None).unwrap();
        locals
            .get_item("roundtripped")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap()
    })
}

struct Fixture {
    directory: tempfile::TempDir,
    path: String,
    envelope: String,
    sample: SampleBuffer,
    digest: String,
    revision: String,
    period: f64,
}

fn temp_directory() -> tempfile::TempDir {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    tempfile::tempdir_in(workspace.join("scratch")).unwrap()
}

fn write_wave(path: &Path) {
    let pcm: Vec<u8> = source()
        .samples
        .iter()
        .flat_map(|v| ((*v * 16384.0) as i16).to_le_bytes())
        .collect();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36_u32 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    std::fs::write(path, wav).unwrap();
}

fn install(
    engine: &AudioEngine,
    sample: SampleBuffer,
    digest: String,
    generation: u64,
    request: u64,
) {
    engine.sample_cache.lock().unwrap()[0] = Some(sample.clone());
    engine.loaded_source_digests.lock().unwrap()[0] = Some(digest);
    engine.loaded_source_generations.lock().unwrap()[0] = (generation, RATE);
    engine.pad_request_ids.lock().unwrap()[0] = request;
    engine
        .input_runtime_ownership
        .publish_source(0, &sample, RATE, generation);
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let directory=temp_directory();
        let path=directory.path().join("original.wav"); write_wave(&path);
        let digest=file_sha256(&path).unwrap();
        let sample=decode_audio_file_to_sample_buffer(&path,1,RATE,|_|{}).unwrap();
        let engine=test_engine(); install(&engine,sample.clone(),digest.clone(),7,7);
        let ticket=prepare(&engine,0,TimingBound { halfwidth_seconds:0.05,provenance:"independent fixture matching tolerance".into() }).unwrap();
        let counts:Vec<Option<i64>>=ticket.evidence.beat_seconds().iter().map(|t|Some((t/PERIOD).round() as i64)).collect();
        let hypotheses=json!([{"id":"independent-pulse-quarters","provenance":"generated test quarter times; association by known period","verification":"verified","quarter_note_denominator":1,"quarter_counts":counts}]).to_string();
        let (producer,mut consumer)=queue(1); let mut mixer=acknowledged_mixer(&engine);
        publish(&engine,&producer,&ticket,&hypotheses,origin(),decision()).unwrap();
        assert!(export_current(&engine,0,path.to_string_lossy().into()).unwrap().is_none());
        assert!(accept_message(&mut mixer,consumer.pop().unwrap()));
        let (revision,period)={ let guard=ticket.guard.lock().unwrap();let a=guard.accepted().unwrap();(a.revision().to_owned(),a.period_seconds_per_quarter()) };
        drop(ticket); // Current native registry, not ticket history, owns complete evidence.
        let envelope=export_current(&engine,0,path.to_string_lossy().into()).unwrap().unwrap();
        Fixture { directory,path:path.to_string_lossy().into(),envelope,sample,digest,revision,period }
    })
}

pub(super) fn migration_fixture() -> (String, String) {
    let fixture = fixture();
    (fixture.path.clone(), fixture.envelope.clone())
}

fn fresh_engine() -> AudioEngine {
    let f = fixture();
    let engine = test_engine();
    let sample = decode_audio_file_to_sample_buffer(Path::new(&f.path), 1, RATE, |_| {}).unwrap();
    assert_eq!(sample.samples.as_ref(), f.sample.samples.as_ref());
    install(&engine, sample, f.digest.clone(), 71, 91);
    engine
}

#[test]
fn saved_timing_productive_native_qm_file_decode_export_fresh_adoption_roundtrip() {
    let f = fixture();
    let engine = fresh_engine();
    let (producer, mut consumer) = queue(2);
    let mut mixer = acknowledged_mixer(&engine);
    let saved = capture_saved(&engine, 0, &f.envelope, f.path.clone()).unwrap();
    assert_eq!(saved.pcm_budget, PcmBudget::default());
    assert_eq!(saved.request_id, 92);
    assert_eq!(saved.source_generation, 71);
    let ticket = restore_saved(&engine, &producer, &saved).unwrap();
    assert_eq!(ticket.pcm_budget, PcmBudget::default());
    assert_eq!(ticket.publication_status().unwrap(), "pending");
    Python::attach(|py| assert!(current_metadata(&engine, py, 0).unwrap().is_none()));
    assert!(
        export_current(&engine, 0, f.path.clone())
            .unwrap()
            .is_none()
    );
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    let current = engine.current_constant_timing.lock().unwrap();
    let record = current[0].last().unwrap();
    assert_eq!(record.revision, f.revision);
    assert_eq!(record.period_seconds.to_bits(), f.period.to_bits());
    assert_eq!(record.origin.seconds.to_bits(), origin().seconds.to_bits());
    assert_eq!(record.binding.job.request_id, 92);
    assert_eq!(record.binding.job.source_generation, 71);
    assert_eq!(record.accepted.evidence().binding().job.request_id, 8);
    assert_eq!(
        record.accepted.evidence().binding().job.source_generation,
        7
    );
    drop(current);
    assert_eq!(
        export_current(&engine, 0, f.path.clone()).unwrap().unwrap(),
        f.envelope
    );
}

#[test]
#[cfg(windows)]
fn saved_finite_resident_timing_uses_complete_evidence_without_complete_ticket_pin() {
    use crate::audio_engine::LoaderEvent;
    use crate::audio_engine::audio_stream::drain_control_messages;
    use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
    use crate::audio_engine::cold_load::admit_for_format_selected;
    use crate::audio_engine::cold_residency::ResidentLoadHint;
    use crate::audio_engine::mixer::RtMixer;
    use crate::audio_engine::scheduler::FixedCapacityScheduler;
    use crate::audio_engine::transport::TransportTimeline;
    use crate::messages::{AudioMessage, TriggerQuantization};
    use std::time::{Duration, Instant};

    let f = fixture();
    let directory = temp_directory();
    let mut engine = AudioEngine::new().unwrap();
    engine.timing_intents.lock().unwrap()[0] = TimingIntent::Automatic;
    engine
        .input_runtime_ownership
        .set_timing_intent(0, TimingIntent::Automatic);
    let (producer, mut consumer) = queue(8);
    let request = admit_for_format_selected(
        &engine,
        0,
        f.path.clone(),
        (
            false,
            true,
            true,
            Some(ResidentLoadHint {
                start_s: 2.25,
                end_s: 3.25,
                key_lock: false,
            }),
        ),
        producer.clone(),
        (1, RATE, directory.path().join("samples")),
    )
    .unwrap();
    let mut mixer = RtMixer::new(1, RATE as f32);
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut transport = TransportTimeline::new(RATE);
    let mut quantization = TriggerQuantization::Immediate;
    let (mut feedback, _feedback_reader) = rtrb::RingBuffer::<AudioMessage>::new(16);
    let mut retirement = ImmediateAudioBufferRetirement;
    let deadline = Instant::now() + Duration::from_secs(10);
    while consumer.peek().is_err() {
        assert!(
            Instant::now() < deadline,
            "finite cold publication timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        engine.input_runtime_ownership.cold_status(0, request),
        Some(0)
    );
    assert_eq!(
        drain_control_messages(
            &mut consumer,
            &mut scheduler,
            0,
            &mut quantization,
            &mut transport,
            &mut mixer,
            &mut feedback,
            &mut retirement,
        ),
        1
    );
    let loaded = loop {
        if let Ok(event) = engine.loader_rx.lock().unwrap().try_recv() {
            match event {
                LoaderEvent::Success { request_id, .. } if request_id == request => break true,
                LoaderEvent::Error { error, .. } => panic!("finite cold load failed: {error}"),
                _ => {}
            }
        }
        assert!(Instant::now() < deadline, "finite cold success timed out");
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(loaded);
    while engine.cold_loading[0].load(Ordering::Acquire) != 0 {
        assert!(
            Instant::now() < deadline,
            "finite cold completion timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let resident = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    assert_eq!(resident.frame_count(), f.sample.frame_count());
    assert_eq!(resident.resident_start(), 18_000);
    assert_eq!(resident.resident_end(), 26_000);
    assert_eq!(resident.samples.len(), 8_000);
    assert_eq!(mixer.loop_region_frames(0), (18_000, Some(26_000)));
    let actual_original = engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .original_path
        .to_string_lossy()
        .into_owned();
    let materialization_peak =
        resident.samples.len() * size_of::<f32>() + f.sample.samples.len() * 2 * size_of::<f32>();
    assert!(
        engine
            .complete_sample(0, &resident, materialization_peak - 1)
            .is_err()
    );
    let complete = engine.complete_sample(0, &resident, MAX_PCM_BYTES).unwrap();
    assert_eq!(complete.samples.as_ref(), f.sample.samples.as_ref());
    assert!(complete.same_source(&resident));
    let temporary_full = Arc::downgrade(&complete.samples);
    drop(complete);
    assert!(temporary_full.upgrade().is_none());

    let captured = capture_saved(&engine, 0, &f.envelope, actual_original.clone()).unwrap();
    assert!(captured.sample.same_window(&resident));
    let ticket = restore_saved(&engine, &producer, &captured).unwrap();
    assert!(ticket.sample.same_window(&resident));
    assert_eq!(ticket.sample.samples.len(), 8_000);
    assert_eq!(ticket.binding.frame_count, f.sample.frame_count() as u64);
    assert!(Arc::ptr_eq(&ticket.sample.samples, &resident.samples));
    assert_eq!(ticket.publication_status().unwrap(), "pending");
    assert_eq!(
        drain_control_messages(
            &mut consumer,
            &mut scheduler,
            0,
            &mut quantization,
            &mut transport,
            &mut mixer,
            &mut feedback,
            &mut retirement,
        ),
        1
    );
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    assert_eq!(
        export_current(&engine, 0, actual_original)
            .unwrap()
            .unwrap(),
        f.envelope
    );
    Python::attach(|py| {
        let current = current_metadata(&engine, py, 0).unwrap().unwrap();
        let metadata = current.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            metadata
                .get_item("frame_count")
                .unwrap()
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            f.sample.frame_count()
        );
        assert_eq!(
            metadata
                .get_item("source_zero_seconds")
                .unwrap()
                .unwrap()
                .extract::<f64>()
                .unwrap(),
            0.0
        );
        let binding = crate::audio_engine::input_runtime_binding::capture(&engine, 0)
            .unwrap()
            .unwrap();
        assert!(binding.current());
        assert_eq!(
            binding
                .metadata(py)
                .unwrap()
                .bind(py)
                .cast::<PyDict>()
                .unwrap()
                .get_item("frame_count")
                .unwrap()
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            f.sample.frame_count()
        );
    });
    assert!(mixer.play_sample(0, 1.0));
    let physical = 8_000;
    let period = f.period * f64::from(RATE) * 2.0;
    let last = ((period.ceil() as usize) - 1).min(physical - 1);
    let mut rendered = 0;
    let mut peaks = [0.0; NUM_SAMPLES];
    while rendered < 16_731 {
        let frames = 512.min(16_731 - rendered);
        let mut output = vec![0.0; frames];
        mixer.render(&mut output, &mut peaks);
        for (offset, actual) in output.iter().enumerate() {
            // Independent raw complete-source knot oracle: no production reader,
            // SourcePlayback, relative window address or advancement helper.
            let phase = ((rendered + offset) as f64).rem_euclid(period);
            let (left, right, alpha) = if phase >= last as f64 {
                (last, 0, (phase - last as f64) / (period - last as f64))
            } else {
                let left = phase.floor() as usize;
                (left, left + 1, phase - left as f64)
            };
            let a = f.sample.samples[18_000 + left];
            let b = f.sample.samples[18_000 + right];
            let expected = a + (b - a) * alpha as f32;
            assert!((*actual - expected).abs() < 1.0e-6);
        }
        rendered += frames;
    }
    assert!(temporary_full.upgrade().is_none());
    assert_eq!(
        engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples
            .len(),
        8_000
    );
    engine.shut_down().unwrap();
}

#[test]
fn finite_timing_known_materialization_budget_failure_preserves_request() {
    let rate = 384_000;
    let complete = SampleBuffer {
        residency: None,
        channels: 32,
        samples: Arc::from(vec![0.125_f32; 400 * 32]),
    }
    .with_complete_source(rate);
    let finite = complete
        .window(71, 91, 2, crate::messages::ResidentContext::FiniteLoop)
        .unwrap();
    let limit =
        complete.samples.len() * 2 * size_of::<f32>() + finite.samples.len() * size_of::<f32>() - 1;
    // The old complete-geometry estimate passes: the rejected peak is the
    // separately retained finite view plus the complete materializer Vec/Arc.
    assert!(
        PcmBudget::new(limit)
            .unwrap()
            .check_loaded_geometry(complete.source_sample_count(), complete.channels, rate,)
            .is_ok()
    );
    let engine = test_engine();
    engine.sample_cache.lock().unwrap()[0] = Some(finite);
    engine.loaded_source_generations.lock().unwrap()[0] = (7, rate);
    let before_request = engine.pad_request_ids.lock().unwrap()[0];
    let before_epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
    let rejected = capture_preparation_with_limit(
        &engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.0,
            provenance: "independent finite materialization budget boundary".into(),
        },
        None,
        limit,
    );
    assert!(matches!(rejected, Err(ref error) if error.contains("PCM byte limit")));
    assert_eq!(engine.pad_request_ids.lock().unwrap()[0], before_request);
    assert_eq!(
        engine.prepared_source_epochs[0].load(Ordering::Acquire),
        before_epoch
    );
}

#[test]
fn current_export_keeps_admitted_runtime_budget_without_encoding_or_restoring_it() {
    let f = fixture();
    let engine = fresh_engine();
    let binding = crate::audio_engine::input_runtime_binding::capture(&engine, 0)
        .unwrap()
        .unwrap();
    let captured = capture_preparation_with_limit(
        &engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.05,
            provenance: "independent bounded fixture matching tolerance".into(),
        },
        Some(&binding),
        32 * 1024 * 1024,
    )
    .unwrap();
    let ticket = prepare_captured(&engine, &captured).unwrap();
    let counts: Vec<Option<i64>> = ticket
        .evidence
        .beat_seconds()
        .iter()
        .map(|t| Some((t / PERIOD).round() as i64))
        .collect();
    let hypotheses = json!([{
        "id":"independent-pulse-quarters",
        "provenance":"generated test quarter times; association by known period",
        "verification":"verified",
        "quarter_note_denominator":1,
        "quarter_counts":counts
    }])
    .to_string();
    let (producer, mut consumer) = queue(1);
    let mut mixer = acknowledged_mixer(&engine);
    publish(
        &engine,
        &producer,
        &ticket,
        &hypotheses,
        origin(),
        decision(),
    )
    .unwrap();
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    let admitted = ticket.pcm_budget;
    drop(ticket);
    assert_eq!(
        engine.current_constant_timing.lock().unwrap()[0]
            .last()
            .unwrap()
            .pcm_budget,
        admitted
    );
    let exported = export_current(&engine, 0, f.path.clone()).unwrap().unwrap();
    assert!(!exported.contains("pcm_limit_bytes"));
    // A test-only policy reduction proves content verification actually uses
    // the retained current budget, rather than a new caller/default budget.
    engine.current_constant_timing.lock().unwrap()[0]
        .last_mut()
        .unwrap()
        .pcm_budget = PcmBudget::new(1).unwrap();
    assert!(export_current(&engine, 0, f.path.clone()).is_err());
    engine.current_constant_timing.lock().unwrap()[0]
        .last_mut()
        .unwrap()
        .pcm_budget = admitted;
    assert_eq!(
        export_current(&engine, 0, f.path.clone()).unwrap().unwrap(),
        exported
    );
    let saved = capture_saved(&engine, 0, &exported, f.path.clone()).unwrap();
    assert_eq!(saved.pcm_budget, PcmBudget::default());
}

#[test]
fn saved_timing_actual_project_atomic_save_load_to_fresh_native_callback_adoption() {
    let f = fixture();
    let engine = Arc::new(fresh_engine());
    let (producer, mut consumer) = queue(1);
    let mut mixer = acknowledged_mixer(&engine);
    let saved = capture_saved(&engine, 0, &f.envelope, f.path.clone()).unwrap();
    let first = restore_saved(&engine, &producer, &saved).unwrap();
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_eq!(first.publication_status().unwrap(), "accepted");
    let directory = temp_directory();
    let config = directory.path().join("flitzis_looper.config.json");
    let from_disk = actual_project_file_roundtrip(engine.clone(), &f.path, &config);
    assert_eq!(
        serde_json::from_str::<Value>(&from_disk).unwrap(),
        serde_json::from_str::<Value>(&f.envelope).unwrap()
    );
    let fresh = fresh_engine();
    let decoded = decode_audio_file_to_sample_buffer(Path::new(&f.path), 1, RATE, |_| {}).unwrap();
    install(&fresh, decoded, f.digest.clone(), 101, 122);
    let saved = capture_saved(&fresh, 0, &from_disk, f.path.clone()).unwrap();
    let ticket = restore_saved(&fresh, &producer, &saved).unwrap();
    Python::attach(|py| assert!(current_metadata(&fresh, py, 0).unwrap().is_none()));
    assert_eq!(ticket.publication_status().unwrap(), "pending");
    let mut fresh_callback = acknowledged_mixer(&fresh);
    assert!(accept_message(&mut fresh_callback, consumer.pop().unwrap()));
    let records = fresh.current_constant_timing.lock().unwrap();
    let current = &records[0][0];
    assert_eq!(current.revision, f.revision);
    assert_eq!(current.binding.job.source_generation, 101);
    assert_eq!(current.binding.job.request_id, 123);
    assert_eq!(current.accepted.evidence().binding().job.request_id, 8);
}

/// Real native content verification and Python atomic persistence, without CPAL.
/// This small fixture measures costs; it is not C3 startup or device acceptance.
#[test]
#[ignore = "explicit C1b save/export accounting writes private evidence"]
fn c1b_actual_native_export_and_project_save_integrity_cost() {
    let output = std::env::var("FLITZI_C1B_SAVE_EVIDENCE")
        .expect("FLITZI_C1B_SAVE_EVIDENCE must name a workspace evidence file");
    let f = fixture();
    let engine = Arc::new(fresh_engine());
    let (producer, mut consumer) = queue(1);
    let mut mixer = acknowledged_mixer(&engine);
    let saved = capture_saved(&engine, 0, &f.envelope, f.path.clone()).unwrap();
    let ticket = restore_saved(&engine, &producer, &saved).unwrap();
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    let directory = temp_directory();
    let config = directory.path().join("flitzis_looper.config.json");
    Python::attach(|py| {
        let locals = PyDict::new(py);
        locals
            .set_item(
                "native_owner",
                Py::new(py, NativeSaveOwner { engine }).unwrap(),
            )
            .unwrap();
        for (key, value) in [
            ("source_path", f.path.as_str()),
            ("config_path", config.to_str().unwrap()),
            ("output_path", output.as_str()),
            ("expected_revision", f.revision.as_str()),
        ] {
            locals.set_item(key, value).unwrap();
        }
        locals
            .set_item("loaded_frames", f.sample.samples.len())
            .unwrap();
        locals.set_item("loaded_rate", RATE).unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        locals
            .set_item(
                "source_modules",
                repo.join("src").to_string_lossy().as_ref(),
            )
            .unwrap();
        let code = std::ffi::CString::new(r#"
import hashlib
import json
import sys
import time
from pathlib import Path
sys.path.insert(0, source_modules)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import ProjectState

source = Path(source_path)
source_bytes = source.stat().st_size
source_digest = hashlib.sha256(source.read_bytes()).hexdigest()
def measured(operation):
    wall = time.perf_counter()
    cpu = time.process_time()
    result = operation()
    return result, {'wall_seconds': time.perf_counter() - wall,
                    'process_cpu_seconds': time.process_time() - cpu}

exports = []
for _ in range(3):
    record, cost = measured(lambda: native_owner.export_current_constant_timing(0, source_path))
    parsed = json.loads(record)
    assert parsed['record']['accepted_revision'] == expected_revision
    cost['json_bytes'] = len(record.encode())
    exports.append(cost)

project = ProjectState()
project.sample_paths[0] = source_path
project.pad_timing_intent[0] = 'automatic'
persistence = ProjectPersistence(project)
persistence.config_path = Path(config_path)
persistence.bind_audio(native_owner)
saves = []
for _ in range(3):
    persistence.mark_dirty()
    _, cost = measured(persistence.flush)
    assert not persistence._dirty
    loaded = ProjectPersistence.from_config_path(Path(config_path)).project
    assert loaded.sample_analysis[0].accepted_timing.record['accepted_revision'] == expected_revision
    cost['config_bytes'] = Path(config_path).stat().st_size
    saves.append(cost)

previous_config = Path(config_path).read_bytes()
original = source.read_bytes()
corrupted = bytearray(original)
corrupted[-1] ^= 1
source.write_bytes(corrupted)
try:
    try:
        native_owner.export_current_constant_timing(0, source_path)
    except ValueError:
        pass
    else:
        raise AssertionError('same-size source corruption must reject native export')
    persistence.mark_dirty()
    assert not persistence.flush_if_dirty()
    assert persistence._dirty
    assert Path(config_path).read_bytes() == previous_config
finally:
    source.write_bytes(original)
assert hashlib.sha256(source.read_bytes()).hexdigest() == source_digest

Path(output_path).write_text(json.dumps({
    'scope': 'real native accepted-QM export plus actual Python atomic project save; 32-second 8kHz fixture',
    'source_sha256': source_digest, 'source_bytes': source_bytes,
    'loaded_frames': loaded_frames, 'loaded_rate_hz': loaded_rate,
    'source_hash_reads_per_verification': 2,
    'source_hash_bytes_per_verification': 2 * source_bytes,
    'byte_accounting_basis': 'unchanged production verify() invokes two complete file_sha256 reads; no PCM disk reads',
    'pcm_cpu_work': 'full loaded mono conversion, 44100Hz resampling, actual complete PCM/backend digest verification',
    'exports': exports, 'saves': saves,
    'same_size_corruption_rejected': True, 'failed_save_retains_previous_config_and_dirty_state': True,
    'manual_device_hearing_acceptance': 'OPEN; no stream or device',
    'limits': 'preliminary warm-file-cache measurements; no C3 200-pad or startup/RAM benefit claim'
}, indent=2) + '\n', encoding='utf-8')
"#).unwrap();
        py.run(&code, Some(&locals), None).unwrap();
    });
}

#[test]
fn saved_timing_pending_failed_and_rejected_replacement_preserve_previous_current_record() {
    let f = fixture();
    let engine = fresh_engine();
    let (producer, mut consumer) = queue(1);
    let mut mixer = acknowledged_mixer(&engine);
    let saved = capture_saved(&engine, 0, &f.envelope, f.path.clone()).unwrap();
    let first = restore_saved(&engine, &producer, &saved).unwrap();
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    let previous = engine.current_timing_acknowledgements.current_epoch(0);
    let saved = capture_saved(&engine, 0, &f.envelope, f.path.clone()).unwrap();
    engine.constant_timing_busy.store(true, Ordering::Release);
    assert!(restore_saved(&engine, &producer, &saved).is_err());
    assert!(export_current(&engine, 0, f.path.clone()).is_err());
    assert_eq!(
        engine.current_timing_acknowledgements.current_epoch(0),
        previous
    );
    assert!(engine.constant_timing_busy.load(Ordering::Acquire));
    engine.constant_timing_busy.store(false, Ordering::Release);
    producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert!(restore_saved(&engine, &producer, &saved).is_err());
    consumer.pop().unwrap();
    assert_eq!(
        engine.current_timing_acknowledgements.current_epoch(0),
        previous
    );
    assert_eq!(
        export_current(&engine, 0, f.path.clone()).unwrap().unwrap(),
        f.envelope
    );
    let pending = restore_saved(&engine, &producer, &saved).unwrap();
    assert_eq!(pending.publication_status().unwrap(), "pending");
    assert_eq!(
        engine.current_timing_acknowledgements.current_epoch(0),
        previous
    );
    assert_eq!(
        export_current(&engine, 0, f.path.clone()).unwrap().unwrap(),
        f.envelope
    );
    let successor = capture_saved(&engine, 0, &f.envelope, f.path.clone()).unwrap();
    assert!(!accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_eq!(pending.publication_status().unwrap(), "rejected");
    assert_eq!(
        engine.current_timing_acknowledgements.current_epoch(0),
        previous
    );
    assert_eq!(
        export_current(&engine, 0, f.path.clone()).unwrap().unwrap(),
        f.envelope
    );
    let final_ticket = restore_saved(&engine, &producer, &successor).unwrap();
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_eq!(final_ticket.publication_status().unwrap(), "accepted");
    assert_ne!(
        engine.current_timing_acknowledgements.current_epoch(0),
        previous
    );
    assert_eq!(first.publication_status().unwrap(), "accepted");
}

#[test]
fn saved_timing_rejects_changed_content_rate_extent_zero_schema_and_incomplete_evidence() {
    let f = fixture();
    for mutation in 0..13 {
        let engine = fresh_engine();
        let mut envelope: Value = serde_json::from_str(&f.envelope).unwrap();
        match mutation {
            0 => envelope["schema_version"] = json!(2),
            1 => envelope["encoding"] = json!("legacy-bpm"),
            2 => envelope["record"]["evidence"]["binding"]["source_sha256"] = json!("c".repeat(64)),
            3 => envelope["record"]["evidence"]["binding"]["pcm_sha256"] = json!("c".repeat(64)),
            4 => envelope["record"]["evidence"]["binding"]["sample_rate_hz"] = json!(48000),
            5 => envelope["record"]["evidence"]["binding"]["frame_count"] = json!(10),
            6 => envelope["record"]["evidence"]["binding"]["source_zero_bits"] = json!(bits(-0.0)),
            7 => envelope["record"]["evidence"]["binding"]["mono_revision"] = json!("unsupported"),
            8 => envelope["record"]["period_bits"] = json!(bits(f.period.next_up())),
            9 => envelope["record"]["evidence"]["qm"]
                .as_object_mut()
                .unwrap()
                .remove("beat_frame_bits")
                .map(|_| ())
                .unwrap(),
            10 => envelope["record"]["origin"]["seconds_bits"] = json!(bits(-0.0)),
            11 => envelope["record"]["decision"]["provenance"] = json!("different assertion"),
            _ => {
                envelope["record"]["evidence"]["qm"]["input"]["input_sha256"] =
                    json!("c".repeat(64))
            }
        }
        let (producer, mut consumer) = queue(1);
        if let Ok(saved) = capture_saved(&engine, 0, &envelope.to_string(), f.path.clone()) {
            assert!(
                restore_saved(&engine, &producer, &saved).is_err(),
                "mutation {mutation}"
            );
        }
        assert!(consumer.pop().is_err());
        Python::attach(|py| assert!(current_metadata(&engine, py, 0).unwrap().is_none()));
    }
    // Same pathname changed after native load must fail both fresh adoption and save.
    let directory = temp_directory();
    let path = directory.path().join("mutable.wav");
    std::fs::copy(&f.path, &path).unwrap();
    let engine = fresh_engine();
    let saved = capture_saved(&engine, 0, &f.envelope, path.to_string_lossy().into()).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[48] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    let (producer, mut consumer) = queue(1);
    assert!(restore_saved(&engine, &producer, &saved).is_err());
    assert!(consumer.pop().is_err());
    std::fs::copy(&f.path, &path).unwrap();
    let saved = capture_saved(&engine, 0, &f.envelope, path.to_string_lossy().into()).unwrap();
    let ticket = restore_saved(&engine, &producer, &saved).unwrap();
    let mut mixer = acknowledged_mixer(&engine);
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[48] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    assert!(export_current(&engine, 0, path.to_string_lossy().into()).is_err());
    assert!(f.directory.path().exists());
}

#[test]
fn saved_timing_fresh_capture_stale_authority_source_failure_and_callback_rejection() {
    let f = fixture();
    for mutation in 0..9 {
        let engine = fresh_engine();
        let saved = capture_saved(&engine, 0, &f.envelope, f.path.clone()).unwrap();
        let (producer, mut consumer) = queue(2);
        match mutation {
            0 => engine.pad_request_ids.lock().unwrap()[0] += 1,
            1 => {
                engine.prepared_source_epochs[0].fetch_add(1, Ordering::AcqRel);
            }
            2 => engine.loaded_source_generations.lock().unwrap()[0].0 += 1,
            3 => engine.loaded_source_generations.lock().unwrap()[0].1 = 48000,
            4 => engine.sample_cache.lock().unwrap()[0] = Some(source()),
            5 => engine.loaded_source_digests.lock().unwrap()[0] = Some("c".repeat(64)),
            6 => {
                set_intent(&engine, &producer, 0, TimingIntent::Manual).unwrap();
                set_intent(&engine, &producer, 0, TimingIntent::Automatic).unwrap();
                consumer.pop().unwrap();
                consumer.pop().unwrap();
            }
            7 => engine.timing_intents.lock().unwrap()[0] = TimingIntent::Tap,
            _ => engine.timing_intents.lock().unwrap()[0] = TimingIntent::Legacy,
        }
        assert!(
            restore_saved(&engine, &producer, &saved).is_err(),
            "mutation {mutation}"
        );
        assert!(consumer.pop().is_err());
    }
    let engine = fresh_engine();
    let saved = capture_saved(&engine, 0, &f.envelope, f.path.clone()).unwrap();
    let (producer, mut consumer) = queue(1);
    producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert!(restore_saved(&engine, &producer, &saved).is_err());
    consumer.pop().unwrap();
    let ticket = restore_saved(&engine, &producer, &saved).unwrap();
    assert_eq!(ticket.publication_status().unwrap(), "pending");
    engine.prepared_source_epochs[0].fetch_add(1, Ordering::AcqRel);
    let mut mixer = acknowledged_mixer(&engine);
    assert!(!accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_eq!(ticket.publication_status().unwrap(), "rejected");
    Python::attach(|py| assert!(current_metadata(&engine, py, 0).unwrap().is_none()));
    assert!(
        export_current(&engine, 0, f.path.clone())
            .unwrap()
            .is_none()
    );
}

#[test]
fn saved_timing_signed_zero_roundtrip_and_nonaccepted_unsupported_export() {
    let engine = test_engine();
    let ticket = crate::audio_engine::constant_timing::tests::synthetic_ticket(&engine);
    let (producer, mut consumer) = queue(1);
    let mut mixer = acknowledged_mixer(&engine);
    publish(
        &engine,
        &producer,
        &ticket,
        &crate::audio_engine::constant_timing::tests::hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert!(
        export_current(&engine, 0, "unused".into())
            .unwrap()
            .is_none()
    ); // Unsupported complete BeatThis fixture, never partial persistence.
    let f = fixture();
    let engine = fresh_engine();
    let saved = capture_saved(&engine, 0, &f.envelope, f.path.clone()).unwrap();
    let (accepted, _, _) = verify(&engine, &saved).unwrap();
    let hypotheses = parse_hypotheses(&saved.record["hypotheses"].to_string()).unwrap();
    let borrowed: Vec<_> = hypotheses
        .iter()
        .map(|h| QuarterNoteHypothesis {
            id: &h.id,
            provenance: &h.provenance,
            verification: h.verification,
            quarter_note_denominator: h.denominator,
            quarter_counts: &h.counts,
        })
        .collect();
    let minus_zero = AcceptedConstantTiming::from_raw(
        accepted.evidence().clone(),
        &borrowed,
        IndependentTimingOrigin {
            seconds: -0.0,
            provenance: "explicit negative zero fixture".into(),
        },
        decision(),
    )
    .unwrap();
    let json = encode(&minus_zero).unwrap();
    let saved = capture_saved(&engine, 0, &json.to_string(), f.path.clone()).unwrap();
    let (a, _, _) = verify(&engine, &saved).unwrap();
    assert_eq!(a.origin().seconds.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(a.revision(), minus_zero.revision());
}
