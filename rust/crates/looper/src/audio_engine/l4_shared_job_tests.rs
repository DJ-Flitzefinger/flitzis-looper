//! Genuine shared Python backend reads retain actual Native file custody.
//! Only the declared nonneural producer/read is substituted; public admission,
//! worker pool, subscriber cancellation, Unload ACKs and asset cleanup are real.

use super::*;
use crate::audio_engine::OfflineStemSourceLease;

const CHILD: &str = "FLITZI_L4_SHARED_RUNNING_JOB_CHILD";
const TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::l4_shared_job_tests::actual_shared_running_job_keeps_custody_after_final_public_unload";

/// Device-free routing reads the actual acknowledged source shape. Offline and
/// digest admission delegate unchanged to Native; no source permit is invented.
#[pyclass]
struct OfflineJobAudio {
    engine: Arc<AudioEngine>,
}

#[pymethods]
impl OfflineJobAudio {
    fn loaded_source_digest(&self, id: usize) -> PyResult<Option<String>> {
        self.engine.loaded_source_digest(id)
    }

    fn loaded_sample_shape(&self, id: usize) -> PyResult<(u32, usize, usize)> {
        assert!([37, 38].contains(&id));
        assert!(self.engine.loaded_source_digest(id)?.is_some());
        let cache = self.engine.sample_cache.lock().unwrap();
        let sample = cache[id].as_ref().unwrap();
        let generations = self.engine.loaded_source_generations.lock().unwrap();
        let (generation, rate) = generations[id];
        assert!(generation > 0 && rate > 0);
        Ok((rate, sample.channels, sample.frame_count()))
    }

    fn capture_offline_stem_source(
        &self,
        id: usize,
        source_version: String,
    ) -> PyResult<OfflineStemSourceLease> {
        self.engine.capture_offline_stem_source(id, source_version)
    }

    fn offline_stem_source_is_current(
        &self,
        id: usize,
        source: &OfflineStemSourceLease,
    ) -> PyResult<bool> {
        self.engine.offline_stem_source_is_current(id, source)
    }

    fn capture_prepared_stem_source(
        &self,
        id: usize,
        source: &OfflineStemSourceLease,
    ) -> PyResult<PreparedSourceTicket> {
        self.engine.capture_prepared_stem_source(id, source)
    }
}

struct BackendRelease(Py<PyAny>);

impl Drop for BackendRelease {
    fn drop(&mut self) {
        Python::attach(|py| {
            self.0.bind(py).call_method0("set").unwrap();
        });
    }
}

fn drain_defaults(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
) {
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(parameters.is_empty());
    playback.drain(consumer);
    assert!(consumer.is_empty());
}

fn python_bool(locals: &Py<PyDict>, expression: &str) -> bool {
    Python::attach(|py| {
        py.eval(
            &std::ffi::CString::new(expression).unwrap(),
            Some(locals.bind(py)),
            None,
        )
        .unwrap()
        .extract()
        .unwrap()
    })
}

pub(super) fn shared_job_after_origin_readers(
    engine: &Arc<AudioEngine>,
    root: &Path,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    locals: &Py<PyDict>,
    current: &FinalReaderProbe,
) {
    assert!(playback.callback.retirement.actual.is_some());
    assert!(playback.callback.retirement.retired.is_empty());
    let input = root.parent().unwrap().join("shared-job-input.wav");
    fs::write(&input, signal_wav(4096, 7)).unwrap();
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("job_input", input.to_string_lossy().as_ref())
            .unwrap();
        locals
            .bind(py)
            .set_item(
                "offline_job_audio",
                Py::new(
                    py,
                    OfflineJobAudio {
                        engine: engine.clone(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
    });
    prepare_public_unload_loader(locals);
    // Each public import has its own ColdLoad/Success/default transaction.
    // Verified equal B bytes share B's canonical original, distinct from A/215.
    for id in [37, 38] {
        Python::attach(|py| {
            locals.bind(py).set_item("job_load_id", id).unwrap();
        });
        controller_python(
            locals,
            "loader.load_sample_async(job_load_id, job_input)\nassert not session.sample_load_errors, session.sample_load_errors",
        );
        wait_until(|| consumer.peek().is_ok());
        assert!(
            matches!(consumer.peek(), Ok(ControlMessage::LoadColdSample { id: own, .. }) if *own == id)
        );
        assert_eq!(playback.drain(consumer), 1, "own B ColdLoad ACK");
        wait_until(|| {
            controller_python(locals, "loader.poll_loader_events()");
            python_bool(locals, "job_load_id not in session.loading_sample_ids")
        });
        drain_defaults(playback, consumer, parameters);
        wait_until(|| {
            controller_python(locals, "loader.poll_loader_events()");
            !crate::audio_engine::has_active_task_for_id(&engine.active_tasks.lock().unwrap(), id)
                && python_bool(locals, "job_load_id not in session.analyzing_sample_ids")
        });
        drain_defaults(playback, consumer, parameters);
        assert!(playback.callback.mixer.bank_for_measurement()[id].is_some());
        playback.render(137, false);
        current.assert_retained();
    }
    let pcm = [37, 38].map(|id| {
        Arc::downgrade(
            &playback.callback.mixer.bank_for_measurement()[id]
                .as_ref()
                .unwrap()
                .samples,
        )
    });
    let readers = [37, 38].map(|id| {
        engine.cold_leases.lock().unwrap()[id]
            .as_ref()
            .unwrap()
            .reader_lifetime_probe_for_test()
    });
    controller_python(
        locals,
        r#"
import hashlib
import threading
from flitzis_looper.controller import stems as job_stems_module
from flitzis_looper.controller.stem_generation import StemGenerationResult
from flitzis_looper_audio import SealedProcessingFiles
assert project.sample_paths[37] == project.sample_paths[38]
assert project.sample_paths[37] != project.sample_paths[0]
assert project.pad_content[37] != project.pad_content[38]
job_original = Path(project.sample_paths[37]).absolute()
job_original_bytes = job_original.read_bytes()
job_model = Path.cwd() / 'declared-shared-job-producer'
job_model.mkdir()
job_files = tuple(job_model / name for name in ('checkpoint.bin', 'read-definition.txt', 'encoder.bin'))
for index, path in enumerate(job_files):
    path.write_bytes(f'declared nonneural held-read input {index}'.encode())
job_saved_functions = (job_stems_module.separator_model_cache_dir,
    job_stems_module.resolved_demucs_encoder, job_stems_module.encoder_contract,
    job_stems_module.capture_processing_files, job_stems_module.processing_source_roots)
def capture_declared_job_files(request):
    guard = SealedProcessingFiles([str(path) for path in job_files])
    producer = hashlib.sha256(json.dumps(guard.witnesses()).encode()).hexdigest()
    return guard, producer
job_stems_module.separator_model_cache_dir = lambda separator: job_model
job_stems_module.resolved_demucs_encoder = lambda model: job_files[-1]
job_stems_module.encoder_contract = lambda encoder: 'declared-nonneural-held-read'
job_stems_module.capture_processing_files = capture_declared_job_files
job_stems_module.processing_source_roots = lambda request, guard: (job_model,)
class JobAudio:
    def __getattr__(self, name):
        for delegate in (offline_job_audio, unload_audio):
            if hasattr(delegate, name):
                return getattr(delegate, name)
        raise AttributeError(name)
controller._audio = JobAudio()
job_entered, job_finish, job_returned = threading.Event(), threading.Event(), threading.Event()
class DeclaredHeldRead:
    calls = 0
    def generate(self, request, progress):
        self.calls += 1
        assert threading.current_thread().name.startswith('stems_')
        assert request.source_path == job_original
        request.cache_dir.mkdir(parents=True)
        (request.cache_dir / 'vocals.wav').write_bytes(job_original_bytes)
        job_entered.set()
        assert job_finish.wait(20), 'backend read was never released'
        assert request.source_path.read_bytes() == job_original_bytes
        assert (request.cache_dir / 'vocals.wav').read_bytes() == job_original_bytes
        job_returned.set()
        return StemGenerationResult('declared-nonneural-read', 'custody-fixture', 'cpu', False, 1)
job_backend = DeclaredHeldRead()
controller._stem_backend = job_backend
assert controller.generate_stems_async(37), session.stem_generation_errors
assert controller.generate_stems_async(38), session.stem_generation_errors
"#,
    );
    let release = BackendRelease(Python::attach(|py| {
        locals
            .bind(py)
            .get_item("job_finish")
            .unwrap()
            .unwrap()
            .unbind()
    }));
    wait_until(|| {
        controller_python(
            locals,
            "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
        );
        python_bool(
            locals,
            "job_entered.is_set() and len(controller._jobs) == 1 and len(next(iter(controller._jobs.values())).subscribers()) == 2",
        )
    });
    controller_python(
        locals,
        r#"
job = next(iter(controller._jobs.values()))
assert job.running and job.phase == 'running'
assert job.generation_fingerprint is not None and job_backend.calls == 1
job_origin = job.request.sample_id
job_survivor = 75 - job_origin
job_generation = job.request.cache_dir
job_reserved = job.retirement.remaining
assert job_reserved > 0
assert not job._source_lease.released and not job._generation_lease.released
job_worker_calls = audio.worker_call_count()
loader.unload_sample(job_origin)
assert tuple(subscriber.sample_id for subscriber in job.subscribers()) == (job_survivor,)
assert not job.cancel_event.is_set() and job.running
assert controller.is_stem_generation_running(job_survivor)
"#,
    );
    let origin: usize = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("job_origin")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap()
    });
    for id in [origin, 75 - origin] {
        if id != origin {
            controller_python(
                locals,
                "loader.unload_sample(job_survivor)\nassert not job.subscribers() and job.cancel_event.is_set() and job.running\nassert job.phase == 'running'",
            );
        }
        let before = playback.callback.feedback.events.len();
        let stopped_feedback =
            ExpectedStoppedFeedback::source_command(&playback.callback.mixer, consumer, id);
        drain_defaults(playback, consumer, parameters);
        assert_eq!(playback.callback.feedback.events.len(), before + 1);
        stopped_feedback.assert_message(&playback.callback.feedback.events[before]);
        assert!(playback.callback.mixer.bank_for_measurement()[id].is_none());
        assert!(playback.callback.mixer.stems_for_measurement()[id].is_none());
        playback.render(137, false);
        current.assert_retained();
        controller_python(
            locals,
            "loader.poll_loader_events()\ncontroller.on_frame_render()",
        );
    }
    let marker = Arc::new(AtomicU8::new(0));
    let completion = Arc::downgrade(&marker);
    playback.callback.retirement.retire_cold_adoption(marker);
    current.wait_for("B actual Unload audio worker completion", || {
        completion.strong_count() == 0
    });
    current.wait_for("B actual final PCM readers", || {
        controller_python(
            locals,
            "loader.poll_loader_events()\ncontroller.on_frame_render()",
        );
        pcm.iter().all(|probe| probe.strong_count() == 0)
    });
    assert!(
        !readers[origin - 37](),
        "running job retains genuine original/cache reader"
    );
    let original: PathBuf = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("job_original")
            .unwrap()
            .unwrap()
            .call_method0("__str__")
            .unwrap()
            .extract::<String>()
            .unwrap()
            .into()
    });
    let generation: PathBuf = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("job_generation")
            .unwrap()
            .unwrap()
            .call_method0("__str__")
            .unwrap()
            .extract::<String>()
            .unwrap()
            .into()
    });
    let original_bytes = fs::read(&original).unwrap();
    assert!(fs::write(&original, b"job-leased overwrite").is_err());
    let sentinel = root.join("materials/M77777777777777777777777777777777/original/sentinel.wav");
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, stereo_wav(64)).unwrap();
    let mut pin = engine.project_assets.acquire_pin(root, &sentinel).unwrap();
    engine
        .project_assets
        .retire(root, &sentinel, false)
        .unwrap();
    pin.release();
    drop(pin);
    current.wait_for(
        "asset worker progresses while running job alone retains B",
        || {
            assert_eq!(fs::read(&original).unwrap(), original_bytes);
            assert_eq!(
                fs::read(generation.join("vocals.wav")).unwrap(),
                original_bytes
            );
            !sentinel.exists()
        },
    );
    controller_python(
        locals,
        r#"
assert job.running and job.phase == 'running' and not job.subscribers()
assert not job._source_lease.released and not job._generation_lease.released
assert job.retirement.remaining == job_reserved
assert job.processing_files is not None
assert all(project.sample_paths[id] is None and project.stem_cache[id] is None for id in (37, 38))
assert not job_finish.is_set() and not job_returned.is_set()
assert audio.worker_call_count() == job_worker_calls
"#,
    );
    for frames in [1, 137, 511, 257] {
        playback.render(frames, false);
        current.assert_retained();
    }
    drop(release);
    current.wait_for("actual backend return and control stale-event settlement", || {
        controller_python(locals, "loader.poll_loader_events()\ncontroller.on_frame_render()");
        python_bool(locals, "job_returned.is_set() and not job.running and job.phase == 'disposed' and not controller._jobs")
    });
    current.wait_for(
        "shared job final actual source readers and physical cleanup",
        || {
            controller_python(
                locals,
                "loader.poll_loader_events()\ncontroller.on_frame_render()",
            );
            readers.iter().all(|ended| ended()) && !original.exists() && !generation.exists()
        },
    );
    assert!(pcm.iter().all(|probe| probe.strong_count() == 0));
    assert!(engine.project_assets.status().unwrap().3.is_empty());
    controller_python(
        locals,
        r#"
assert job._source_lease.released and job._generation_lease.released
assert job.source is None and job.processing_files is None
assert job.retirement.remaining == 0 and job_backend.calls == 1
assert controller._assets._reserved == 0
assert not controller._assets.retirement_errors and not controller._assets._pending
assert not session.stem_generating_sample_ids and not session.stem_generation_errors
assert not session.sample_load_errors
assert audio.worker_call_count() == job_worker_calls
assert not controller._committed_residency.contains(37) and not controller._committed_residency.contains(38)
persistence.flush()
job_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert job_reopened.load_error is None
assert all(job_reopened.project.sample_paths[id] is None and job_reopened.project.stem_cache[id] is None for id in (37, 38))
assert job_reopened.project.stem_cache[0].pair == disk_v2
(job_stems_module.separator_model_cache_dir, job_stems_module.resolved_demucs_encoder,
 job_stems_module.encoder_contract, job_stems_module.capture_processing_files,
 job_stems_module.processing_source_roots) = job_saved_functions
"#,
    );
    assert!(consumer.is_empty() && parameters.is_empty());
    current.assert_retained();
}

#[test]
fn actual_shared_running_job_keeps_custody_after_final_public_unload() {
    if std::env::var_os(CHILD).is_some() {
        round_trip(
            true,
            None,
            true,
            None,
            false,
            CandidateTrace::SharedJobFinalReader,
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L4 shared-running-job child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}
