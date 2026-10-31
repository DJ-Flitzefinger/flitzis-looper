//! Public Generate -> source-leased pool -> fenced disk pair -> actual live Capture/Adopt.
//! Declared inference outputs, producer resolvers/capture and receipt flags form the fixture.
//! Native sealed-file verification and the actual generation/publication guards are unchanged.
//! The restored baseline pair is fixture setup; V1 is the first generated performative ALL.

use super::super::committed_stem_public_complete_tests as complete;
use super::*;
use crate::audio_engine::OfflineStemSourceLease;
use pyo3::types::PyBytes;

#[path = "public_generate_pitch_action_tests.rs"]
mod pitch_action_tests;

const CHILD: &str = "FLITZI_PUBLIC_GENERATE_ADOPTION_CHILD";
const TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::public_generate_adoption_tests::actual_public_generate_first_and_model_replacement_require_own_live_adoption";
const PIANO_CHILD: &str = "FLITZI_PUBLIC_GENERATE_PIANO_CHILD";
const PIANO_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::public_generate_adoption_tests::actual_public_generate_piano_pitch_waits_for_own_wet_output_with_either_lock";

#[test]
fn actual_public_generate_piano_pitch_waits_for_own_wet_output_with_either_lock() {
    if let Ok(lock) = std::env::var(PIANO_CHILD) {
        let (key_lock, followup): (bool, complete::PublicCompleteFollowup) = match lock.as_str() {
            "on" => (true, piano_history_on),
            "off" => (false, piano_history_off),
            _ => panic!("unknown declared piano lock profile"),
        };
        complete::history_with_piano_followup(key_lock, followup);
        eprintln!(
            "pitched public Generate child {lock} PID={}",
            std::process::id()
        );
        return;
    }
    for lock in ["off", "on"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", PIANO_TEST, "--nocapture", "--test-threads=1"])
            .env(PIANO_CHILD, lock)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "pitched public Generate {lock} child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains(&format!("pitched public Generate child {lock} PID="))
        );
    }
}

#[test]
fn actual_public_generate_first_and_model_replacement_require_own_live_adoption() {
    if std::env::var_os(CHILD).is_some() {
        complete::history_with_followup(true, Some((false, history)));
        eprintln!("public Generate adoption child PID={}", std::process::id());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "public Generate child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("public Generate adoption child PID=")
    );
}

/// Device-free shape comes from the actual acknowledged cache/generation. All
/// digest, offline lease, current-source and completion-ticket APIs are Native.
#[pyclass]
struct GenerateSourceAudio {
    engine: Arc<AudioEngine>,
}

#[pymethods]
impl GenerateSourceAudio {
    fn waveform_source_identity(&self, id: usize) -> PyResult<Option<(u64, String, usize, u32)>> {
        self.engine.waveform_source_identity(id)
    }

    fn loaded_source_digest(&self, id: usize) -> PyResult<Option<String>> {
        self.engine.loaded_source_digest(id)
    }

    fn loaded_sample_shape(&self, id: usize) -> PyResult<(u32, usize, usize)> {
        assert_eq!(id, 0);
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
        version: String,
    ) -> PyResult<OfflineStemSourceLease> {
        self.engine.capture_offline_stem_source(id, version)
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

struct ReleaseAdapters(Vec<Py<PyAny>>);

impl Drop for ReleaseAdapters {
    fn drop(&mut self) {
        Python::attach(|py| {
            for event in &self.0 {
                event.bind(py).call_method0("set").unwrap();
            }
        });
    }
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

fn selection_json(locals: &Py<PyDict>, name: &str) -> Value {
    Python::attach(|py| {
        let json: String = locals
            .bind(py)
            .get_item(name)
            .unwrap()
            .unwrap()
            .call_method0("model_dump_json")
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&json).unwrap()
    })
}

fn key_lock_identity(engine: &AudioEngine) -> [u64; 4] {
    Python::attach(|py| {
        let status = engine.pad_key_lock_status(py, 0).unwrap().unwrap();
        [
            "source_generation",
            "source_identity",
            "window_revision",
            "request_id",
        ]
        .map(|field| {
            status
                .bind(py)
                .get_item(field)
                .unwrap()
                .unwrap()
                .extract()
                .unwrap()
        })
    })
}

#[derive(Clone, Copy)]
struct ProcessingProfile {
    ratio: f64,
    key_lock: bool,
    piano_shift: i32,
    native_pitch: f64,
}

struct Peer {
    profile: ProcessingProfile,
    at: u64,
    consumed: u64,
    generation: u64,
    native: usize,
    fifo: (usize, usize),
    pcm: Arc<[f32]>,
    demand: (StemMixMode, u8, u64),
}

impl Peer {
    fn new(playback: &ControllerPlayback, profile: ProcessingProfile) -> Self {
        assert_eq!(playback.ratio, profile.ratio);
        assert_eq!(
            playback.callback.mixer.key_lock_for_measurement(0),
            profile.key_lock
        );
        assert_eq!(
            playback.callback.mixer.pad_pitch_intent(0).unwrap(),
            crate::audio_engine::pitch_intent::PitchIntent::new(profile.piano_shift, 0).unwrap()
        );
        let peer = complete::other(&playback.callback.mixer);
        assert_eq!(
            peer.volume, 0.0,
            "this peer proves phase/owners, not audibility"
        );
        Self {
            profile,
            at: playback.frame,
            // Public peer T=E=0 precedes the main voice's prepared Native E.
            consumed: playback.frame,
            generation: peer.generation,
            native: peer.stretch.native_state_address(),
            fifo: peer.stretch.pending_fifo_frames(),
            pcm: peer.sample.as_ref().unwrap().samples.clone(),
            demand: playback.callback.mixer.stem_demand_for_measurement(215),
        }
    }

    fn assert_current(&self, playback: &ControllerPlayback) {
        let peer = complete::other(&playback.callback.mixer);
        let phase = ((self.consumed + playback.frame - self.at) as f64 * self.profile.ratio)
            .rem_euclid(4096.0);
        assert_eq!(
            peer.source_playback.position().frame,
            phase.floor() as usize
        );
        assert_eq!(
            peer.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(peer.generation, self.generation);
        assert_eq!(peer.source_playback.tempo_ratio(), self.profile.ratio);
        assert_eq!(peer.volume, 0.0);
        assert_eq!(peer.stretch.native_state_address(), self.native);
        assert_eq!(peer.stretch.pending_fifo_frames(), self.fifo);
        assert!(Arc::ptr_eq(
            &peer.sample.as_ref().unwrap().samples,
            &self.pcm
        ));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            self.demand
        );
    }
}

fn render(playback: &mut ControllerPlayback, peer: &Peer, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let outputs = playback.render(frames, true);
    let raw = playback.raw.as_ref().unwrap();
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .pending_fifo_frames(),
        (raw.input[0].len(), raw.output[0].len())
    );
    assert_eq!(
        playback.callback.mixer.key_lock_for_measurement(0),
        peer.profile.key_lock
    );
    peer.assert_current(playback);
    outputs
}

struct DeclaredOutputs {
    first: [Vec<u8>; 5],
    second: [Vec<u8>; 5],
    components: [SampleBuffer; 4],
}

fn declared_wavs() -> DeclaredOutputs {
    let first = std::array::from_fn(|index| {
        if index < 4 {
            signal_wav(4096, index)
        } else {
            signal_sum_wav(4096, &[1, 2, 3])
        }
    });
    let second = [
        first[1].clone(),
        first[0].clone(),
        first[2].clone(),
        first[3].clone(),
        signal_sum_wav(4096, &[0, 2, 3]),
    ];
    let components = std::array::from_fn(|index| SampleBuffer {
        channels: 2,
        residency: None,
        samples: first[index][44..]
            .chunks_exact(2)
            .map(|bytes| f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32767.0)
            .collect::<Vec<_>>()
            .into(),
    });
    assert_ne!(first[0], first[1]);
    DeclaredOutputs {
        first,
        second,
        components,
    }
}

struct GenerationFixture {
    root: PathBuf,
    parameters: rtrb::Consumer<ControlParameterMessage>,
    release: ReleaseAdapters,
}

fn setup(
    playback: &mut ControllerPlayback,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    first: &[Vec<u8>; 5],
    second: &[Vec<u8>; 5],
    profile: ProcessingProfile,
) -> GenerationFixture {
    let (root, producer) = Python::attach(|py| {
        let worker = locals
            .bind(py)
            .get_item("audio")
            .unwrap()
            .unwrap()
            .extract::<PyRef<PairWorkerAudio>>()
            .unwrap();
        (worker.root.clone(), worker.producer.clone())
    });
    let (parameters, consumer) = rtrb::RingBuffer::new(16);
    let parameters = Arc::new(Mutex::new(parameters));
    Python::attach(|py| {
        let bound = locals.bind(py);
        bound
            .set_item(
                "generate_source_audio",
                Py::new(
                    py,
                    GenerateSourceAudio {
                        engine: engine.clone(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        bound
            .set_item(
                "load_audio",
                Py::new(
                    py,
                    PublicLoadAudio {
                        engine: engine.clone(),
                        producer: producer.clone(),
                        parameters,
                    },
                )
                .unwrap(),
            )
            .unwrap();
        for (name, wavs) in [("generated_wavs1", first), ("generated_wavs2", second)] {
            let values = wavs
                .iter()
                .map(|bytes| PyBytes::new(py, bytes).unbind())
                .collect::<Vec<_>>();
            bound.set_item(name, values).unwrap();
        }
    });
    controller_python(
        locals,
        r#"
import hashlib
import threading
from flitzis_looper.controller import stems as generate_module
from flitzis_looper.controller.stem_generation import StemGenerationResult
from flitzis_looper.controller.stem_separators import SelectedStemGenerationBackend
from flitzis_looper.controller.bs_roformer_assets import MODEL_NAME as declared_bs_model
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper_audio import SealedProcessingFiles
from pathlib import Path
persistence = ProjectPersistence(project)
persistence.mark_dirty()
persistence.flush()
controller.bind_disk_selection_commit(persistence.commit_stem_selection, persistence.flush)
settings = SettingsController(project, session, audio, persistence.mark_dirty)
settings.set_stem_separator('demucs:htdemucs')
persistence.flush()
assert project.stem_separator == 'demucs:htdemucs'
assert project.pad_stem_mix_mode[0] == 'full_mix' and project.pad_key_lock[0] is key_lock
assert session.active_sample_ids == {0, 215}
generate_path, generate_content = project.sample_paths[0], project.pad_content[0]
generate_source_version = fixture['source_version']
baseline_disk = project.stem_cache[0].pair
baseline_live = controller._committed_residency._live_selections.get(0)
baseline_control = (baseline_live.control if baseline_live is not None
                    else controller._committed_residency._selections.get(0))
baseline_attempt = baseline_live.attempt if baseline_live is not None else None
baseline_effective_identity = controller.stem_residency_status(0).effective_stem_set_identity
baseline_receipt = baseline_control.receipt if baseline_control is not None else None
assert baseline_attempt is None and baseline_effective_identity is None, (
    'public FullMix baseline', baseline_attempt, baseline_effective_identity)
assert 0 not in controller._verified_sets

class GenerateAudio:
    def __getattr__(self, name):
        for delegate in (generate_source_audio, audio):
            if hasattr(delegate, name):
                return getattr(delegate, name)
        raise AttributeError(name)
controller._audio = GenerateAudio()
generate_original = Path(generate_path).absolute()
generate_original_bytes = generate_original.read_bytes()
generate_model_root = Path.cwd() / 'declared-generate-adapters'
generate_model_root.mkdir()
generate_models, generate_role_bytes = {}, {}
for key, role in (('demucs:htdemucs', 'demucs'), ('bs-roformer:musdb18hq', 'bs')):
    directory = generate_model_root / role
    directory.mkdir()
    paths = tuple(directory / name for name in ('checkpoint.bin', 'inference-definition.txt', 'encoder-input-contract.bin'))
    for index, path in enumerate(paths):
        path.write_bytes(f'Declared non-neural inference-boundary {role} role {index}; WAV and receipt fixture'.encode())
    generate_models[key] = (directory, paths)
    generate_role_bytes[key] = tuple(path.read_bytes() for path in paths)
generate_saved_functions = (generate_module.separator_model_cache_dir, generate_module.resolved_demucs_encoder,
    generate_module.encoder_contract, generate_module.bs_input_contract,
    generate_module.capture_processing_files, generate_module.processing_source_roots)
generate_module.separator_model_cache_dir = lambda separator: generate_models[separator][0]
generate_module.resolved_demucs_encoder = lambda directory: directory / 'encoder-input-contract.bin'
generate_module.encoder_contract = lambda encoder: 'declared-inference-output-receipt'
generate_module.bs_input_contract = lambda encoder: 'declared-inference-input-receipt'
def capture_declared_generate_files(request):
    guard = SealedProcessingFiles([str(path) for path in generate_models[request.separator][1]])
    witnesses = [(index, digest, size) for index, (_name, digest, size) in enumerate(guard.witnesses())]
    return guard, hashlib.sha256(json.dumps(witnesses).encode()).hexdigest()
generate_module.capture_processing_files = capture_declared_generate_files
generate_module.processing_source_roots = lambda request, guard: (generate_models[request.separator][0],)

class DeclaredAdapter:
    def __init__(self, separator, wavs):
        self.separator, self.wavs = separator, tuple(wavs)
        self.calls = []
        self.entered, self.finish = threading.Event(), threading.Event()
    def generate(self, request, progress):
        assert threading.current_thread().name.startswith('stems_')
        assert request.separator == self.separator
        directory, paths = generate_models[self.separator]
        assert request.encoder_executable.resolve() == paths[-1].resolve()
        assert request.encoder_contract == ('declared-inference-input-receipt'
            if self.separator == 'bs-roformer:musdb18hq' else 'declared-inference-output-receipt')
        assert request.python_source_roots == (directory,)
        assert tuple(path.read_bytes() for path in paths) == generate_role_bytes[self.separator]
        assert request.source_path == generate_original
        assert request.target_shape.sample_rate_hz == fixture['rate'] and request.target_shape.channels == 2
        assert request.target_shape.frame_count == fixture['frames']
        assert request.source_path.read_bytes() == generate_original_bytes
        assert request.source_version.endswith('|sha256-v1:' + hashlib.sha256(generate_original_bytes).hexdigest())
        self.calls.append(request)
        self.entered.set()
        progress(0.5, 'Declared inference output held while real playback continues')
        assert self.finish.wait(20), 'declared inference adapter was not released'
        assert request.source_path.read_bytes() == generate_original_bytes
        request.cache_dir.mkdir(parents=True)
        names = ('vocals', 'melody', 'bass', 'drums', 'instrumental')
        for name, encoded in zip(names, self.wavs, strict=True):
            path = request.cache_dir / (name + '.wav')
            path.write_bytes(encoded)
            assert path.read_bytes() == encoded
        # The common promotion owns the exclusive, fsynced SHA/provenance marker
        # and atomic .generation -> .ready publication after this adapter returns.
        assert not (request.cache_dir / '.complete.json').exists()
        # These are declared inference/encoder-receipt fixture values. The production
        # verifier is unchanged; no actual neural model or encoder execution is claimed.
        if self.separator == 'demucs:htdemucs':
            return StemGenerationResult('demucs', 'htdemucs', 'cpu', False, 5, encoder_verified=True)
        return StemGenerationResult('bs-roformer', declared_bs_model, 'cpu', False, 5, decoder_verified=True)
generate_demucs = DeclaredAdapter('demucs:htdemucs', generated_wavs1)
generate_bs = DeclaredAdapter('bs-roformer:musdb18hq', generated_wavs2)
controller._stem_backend = SelectedStemGenerationBackend({
    'demucs:htdemucs': generate_demucs, 'bs-roformer:musdb18hq': generate_bs})
assert isinstance(controller._stem_backend, SelectedStemGenerationBackend)
assert controller._stem_worker_pool._executor._max_workers == 2
assert controller.stem_generation_block_reason(0) is None

def admitted_generate_job(separator):
    # Public Generate captures the original logical caller before the ordinary
    # fair-admission poll acquires its own Native source and physical pool job.
    requested = controller._requested_generations[0]
    assert requested.request.sample_id == requested.capture.pad_id == 0
    assert requested.request.separator == separator
    assert requested.request.source_path == generate_original
    assert requested.request.source_version == generate_source_version
    assert requested.capture.path == generate_path
    assert requested.capture.content == generate_content
    assert requested.capture.source_identity == generate_source_audio.waveform_source_identity(0)
    assert requested.capture.processing_revision == project.stem_processing_revision
    assert requested.capture.processing_signature[0] == separator
    assert requested.intent.settings_revision == project.stem_processing_revision
    assert requested.owner.current == frozenset({generate_module.DIRECT_STEM_INTEREST})
    assert controller._requested_generation_current(requested)
    assert not controller._jobs
    controller.on_frame_render()
    assert 0 not in controller._requested_generations
    assert not session.stem_generation_errors, session.stem_generation_errors
    job = controller._jobs[requested.request.cache_dir]
    assert job.request is requested.request and job.intent is requested.intent
    subscribers = job.subscribers()
    assert len(subscribers) == 1
    subscriber = subscribers[0]
    assert subscriber.sample_id == 0 and subscriber.content_id == generate_content.instance_id
    assert subscriber.request is requested.request
    assert subscriber.interest_owner is requested.owner
    assert subscriber.source is job.source
    assert job.source.source_generation == requested.capture.source_identity[0]
    assert controller._audio.offline_stem_source_is_current(0, job.source)
    return job
"#,
    );
    let release = ReleaseAdapters(Python::attach(|py| {
        ["generate_demucs", "generate_bs"]
            .map(|name| {
                locals
                    .bind(py)
                    .get_item(name)
                    .unwrap()
                    .unwrap()
                    .getattr("finish")
                    .unwrap()
                    .unbind()
            })
            .into()
    }));
    assert_eq!(playback.ratio, profile.ratio);
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    GenerationFixture {
        root,
        parameters: consumer,
        release,
    }
}

fn generate(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    peer: &Peer,
    replacement: bool,
) {
    controller_python(
        locals,
        if replacement {
            r#"
old_processing_revision = project.stem_processing_revision
settings.set_stem_separator('bs-roformer:musdb18hq')
assert project.stem_processing_revision != old_processing_revision
assert project.pad_stem_mix_mode[0] == 'all_stems'
assert controller.stem_generation_block_reason(0) is None
assert controller.generate_stems_async(0), session.stem_generation_errors
generate_job2 = admitted_generate_job('bs-roformer:musdb18hq')
assert generate_job2.request.separator == 'bs-roformer:musdb18hq'
"#
        } else {
            r#"
assert controller.generate_stems_async(0), session.stem_generation_errors
generate_job1 = admitted_generate_job('demucs:htdemucs')
assert generate_job1.request.separator == 'demucs:htdemucs'
"#
        },
    );
    wait_until(|| {
        complete::feedback(locals, &mut playback.callback);
        python_bool(
            locals,
            if replacement {
                "generate_bs.entered.is_set()"
            } else {
                "generate_demucs.entered.is_set()"
            },
        )
    });
    controller_python(
        locals,
        if replacement {
            r#"
assert len(generate_bs.calls) == len(generate_demucs.calls) == 1
assert generate_job2.running and len(generate_job2.subscribers()) == 1
assert generate_job2.source is not None and not generate_job2._source_lease.released
assert controller._audio.offline_stem_source_is_current(0, generate_job2.source)
assert generate_job2.processing_files is not None
assert generate_job2.generation_fingerprint is not None and generate_job2.producer_identity is not None
assert generate_job2.subscribers()[0].intent.settings_revision == project.stem_processing_revision
assert generated1.effective_mode_status() == 'effective'
"#
        } else {
            r#"
assert len(generate_demucs.calls) == 1 and not generate_bs.calls
assert generate_job1.running and len(generate_job1.subscribers()) == 1
assert generate_job1.source is not None and not generate_job1._source_lease.released
assert controller._audio.offline_stem_source_is_current(0, generate_job1.source)
assert generate_job1.processing_files is not None
assert generate_job1.generation_fingerprint is not None and generate_job1.producer_identity is not None
assert generate_job1.subscribers()[0].intent.settings_revision == project.stem_processing_revision
assert project.pad_stem_mix_mode[0] == 'full_mix'
"#
        },
    );
    for frames in [1, 31, 137, 511] {
        render(playback, peer, frames);
        assert!(
            consumer.is_empty(),
            "inference output cannot manufacture an early Capture/Adopt"
        );
    }
    controller_python(
        locals,
        if replacement {
            "generate_bs.finish.set()"
        } else {
            "generate_demucs.finish.set()"
        },
    );
    wait_until(|| {
        complete::feedback(locals, &mut playback.callback);
        controller_python(
            locals,
            "assert not session.stem_generation_errors, session.stem_generation_errors",
        );
        python_bool(
            locals,
            if replacement {
                "0 not in controller._pair_preparations.pending and 0 not in controller._disk_selection_settlements and project.stem_cache[0].pair is not None and project.stem_cache[0].pair != generated_disk1"
            } else {
                "0 not in controller._pair_preparations.pending and 0 not in controller._disk_selection_settlements and project.stem_cache[0].pair is not None and project.stem_cache[0].pair != baseline_disk"
            },
        )
    });
    controller_python(
        locals,
        if replacement {
            r#"
generated_disk2 = project.stem_cache[0].pair
generated_live2 = controller._committed_residency._live_selections[0]
generated2 = generated_live2.attempt.receipt
assert generated2 is not generated1 and not generated2.claimed()
assert generated2.is_current() and generated2.effective_mode_status() == 'waiting'
assert generated1.effective_mode_status() == 'effective'
assert generated_disk2 != generated_disk1 and generated_disk2.stem_set_identity != generated_disk1.stem_set_identity
assert project.stem_cache[0].generation_fingerprint != generated_fingerprint1
assert project.stem_cache[0].producer_identity != generated_producer1
assert not generated_live2.disk.prepared.has_components()
assert project.pad_stem_mix_mode[0] == 'all_stems'
assert len(generate_demucs.calls) == len(generate_bs.calls) == 1
generated2_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert generated2_reopened.project.stem_cache[0].pair == generated_disk2
"#
        } else {
            r#"
generated_disk1 = project.stem_cache[0].pair
generated_fingerprint1 = project.stem_cache[0].generation_fingerprint
generated_producer1 = project.stem_cache[0].producer_identity
assert generated_fingerprint1 is not None and generated_producer1 is not None
generated_live1 = controller._committed_residency._live_selections[0]
generated1_before_all = {
    'retained_control': generated_live1.control is baseline_control,
    'retained_attempt': generated_live1.attempt is baseline_attempt,
    'effective_identity': generated_live1.effective_identity,
    'baseline_effective_identity': baseline_effective_identity,
    'disk_identity': generated_disk1.stem_set_identity,
    'baseline_identity': baseline_disk.stem_set_identity,
    'has_components': generated_live1.disk.prepared.has_components(),
    'requested_mode': project.pad_stem_mix_mode[0],
}
assert generated_live1.control is baseline_control, generated1_before_all
assert generated_live1.attempt is baseline_attempt, generated1_before_all
assert generated_live1.effective_identity == baseline_effective_identity, generated1_before_all
assert not generated_live1.disk.prepared.has_components(), generated1_before_all
assert project.pad_stem_mix_mode[0] == 'full_mix'
generated1_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert generated1_reopened.project.stem_cache[0].pair == generated_disk1
assert controller.set_stem_enabled_mask(0, 0b1010)
assert controller.set_stem_mix_mode(0, 'all_stems')
generated1 = generated_live1.attempt.receipt
assert generated1 is not None and not generated1.claimed() and generated1.effective_mode_status() == 'waiting'
assert generated_live1.attempt is not baseline_control and generated1 is not baseline_receipt, generated1_before_all
assert generated_live1.attempt.entry.pair == generated_disk1, generated1_before_all
"#
        },
    );
    controller_python(
        locals,
        "assert project.sample_paths[0] == generate_path and project.pad_content[0] == generate_content\nassert project.stem_cache[0].source_version == generate_source_version",
    );
}

fn adopt(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    peer: &Peer,
    name: &str,
) -> u64 {
    assert_eq!(
        playback.drain(consumer),
        1,
        "own generated component Capture"
    );
    wait_until(|| consumer.peek().is_ok());
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "pending"
    );
    render(playback, peer, 31);
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history().unwrap();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let ack = playback.frame;
    assert_eq!(playback.drain(consumer), 1, "own generated component Adopt");
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "accepted"
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "transitioning"
    );
    let after = voice(&playback.callback.mixer);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.generation, generation);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    let after_history = after.stretch.productive_history().unwrap();
    assert_eq!(after_history.binding, history.binding);
    assert_eq!(after_history.next_position, history.next_position);
    assert_eq!(after_history.fed_output_frames, history.fed_output_frames);
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    peer.assert_current(playback);
    complete::feedback(locals, &mut playback.callback);
    ack
}

fn settle(
    playback: &mut ControllerPlayback,
    locals: &Py<PyDict>,
    peer: &Peer,
    name: &str,
    ack: u64,
    retiring: Option<(&AudioEngine, &FinalReaderProbe)>,
) {
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let mut remaining = (128.0_f64 / peer.profile.ratio).ceil() as usize;
    for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
        let count = frames.min(remaining);
        render(playback, peer, count);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        remaining -= count;
        if remaining == 0 {
            break;
        }
        assert_eq!(
            controller_ticket_status(locals, name, "effective_mode_status"),
            "transitioning"
        );
        assert!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
    }
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert!(
        !voice(&playback.callback.mixer)
            .stretch
            .wet_current_source_output_ready(),
        "completed source ramp still has no own current-selection wet output"
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "transitioning"
    );
    // Own ramp end is established before the first timestamped settled feed.
    // The expected Native horizon and PCM derive solely from this counted anchor.
    let anchor = playback.frame;
    let source_anchor = playback.source_frame;
    assert!(anchor > ack);
    let expected_target = anchor + 4096;
    let mut raw = ControllerRawNative::new_with_pitch(peer.profile.native_pitch, true);
    raw.render(&playback.feed(source_anchor, 4096));
    if let Some((_, old)) = retiring {
        old.assert_retained();
        let outgoing = playback
            .callback
            .mixer
            .outgoing_stems_for_measurement(0)
            .unwrap();
        assert!(std::sync::Weak::ptr_eq(
            &old.identity,
            &Arc::downgrade(&outgoing.complete_set_identity)
        ));
        playback.callback.retirement.slots = usize::MAX;
    }
    render(playback, peer, 1);
    if let Some((engine, old)) = retiring {
        fifo(
            playback,
            old,
            "V1 real outgoing retirement FIFO before V2 Native adoption",
        );
        Python::attach(|py| {
            engine.poll_loader_events(py).unwrap();
        });
        old.assert_retained();
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
    }
    wait_until(|| {
        voice_mut(&mut playback.callback.mixer)
            .stretch
            .source_preparation_ready()
            && voice_mut(&mut playback.reference)
                .stretch
                .source_preparation_ready()
    });
    assert_eq!(
        voice_mut(&mut playback.callback.mixer)
            .stretch
            .prepared_target_output_frame(),
        Some(expected_target)
    );
    assert_eq!(
        voice_mut(&mut playback.reference)
            .stretch
            .prepared_target_output_frame(),
        Some(expected_target)
    );
    let prepared = voice_mut(&mut playback.callback.mixer)
        .stretch
        .prepared_native_address();
    assert_ne!(prepared, native);
    assert!(
        voice_mut(&mut playback.callback.mixer)
            .stretch
            .prepared_tap_observation()
            .is_none()
    );
    playback.candidate = Some((expected_target, raw));
    let mut counts = [1_usize, 127, 777, 96, 257, 512, 31].into_iter().cycle();
    while playback.frame < expected_target {
        render(
            playback,
            peer,
            counts
                .next()
                .unwrap()
                .min((expected_target - playback.frame) as usize),
        );
    }
    if let Some((_, old)) = retiring {
        old.assert_retained();
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
    }
    render(playback, peer, 137);
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        prepared
    );
    assert!(
        voice(&playback.callback.mixer)
            .stretch
            .wet_current_source_output_ready()
    );
    for frames in [1, 31, 137, 511, 777] {
        render(playback, peer, frames);
    }
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "effective"
    );
    complete::feedback(locals, &mut playback.callback);
}

fn probe(
    root: &Path,
    playback: &ControllerPlayback,
    locals: &Py<PyDict>,
    name: &str,
    components: &[SampleBuffer; 4],
    mapping: [usize; 4],
) -> FinalReaderProbe {
    let actual = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    for (index, resident) in actual.stems.iter().enumerate() {
        assert_eq!(
            (
                resident.resident_start(),
                resident.resident_end(),
                resident.frame_count()
            ),
            (0, 4096, 4096)
        );
        assert_eq!(
            resident.samples.as_ref(),
            components[mapping[index]].samples.as_ref()
        );
        assert!(!Arc::ptr_eq(
            &resident.samples,
            &components[mapping[index]].samples
        ));
    }
    let selection = selection_json(locals, name);
    assert_complete_selection(root, &selection);
    FinalReaderProbe::new(root, &selection, actual)
}

fn fifo(playback: &mut ControllerPlayback, probe: &FinalReaderProbe, stage: &str) {
    let marker = Arc::new(AtomicU8::new(0));
    let done = Arc::downgrade(&marker);
    playback.callback.retirement.retire_cold_adoption(marker);
    probe.wait_for(stage, || done.strong_count() == 0);
    assert!(playback.callback.retirement.retired.is_empty());
}

fn cleanup(engine: &AudioEngine, locals: &Py<PyDict>, probe: &FinalReaderProbe) {
    probe.wait_for("generated actual Native/component final readers", || {
        Python::attach(|py| {
            engine.poll_loader_events(py).unwrap();
        });
        controller_python(locals, "controller.on_frame_render()");
        probe.identity.strong_count() == 0 && probe.pcm.iter().all(|pcm| pcm.strong_count() == 0)
    });
    probe.wait_for("generated WAV/PCM/descriptor asset cleanup", || {
        Python::attach(|py| {
            engine.poll_loader_events(py).unwrap();
        });
        probe.areas.iter().all(|path| !path.exists())
    });
}

fn history(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    owners: &[Arc<InputRuntimeOwnership>; 2],
) {
    history_profile(
        playback,
        consumer,
        locals,
        engine,
        owners,
        ProcessingProfile {
            ratio: 0.75,
            key_lock: true,
            piano_shift: 0,
            native_pitch: 1.0 / 0.75,
        },
    );
}

fn piano_history_on(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    owners: &[Arc<InputRuntimeOwnership>; 2],
) {
    history_profile(
        playback,
        consumer,
        locals,
        engine,
        owners,
        ProcessingProfile {
            ratio: 1.0,
            key_lock: true,
            piano_shift: 12,
            native_pitch: 2.0,
        },
    );
}

fn piano_history_off(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    owners: &[Arc<InputRuntimeOwnership>; 2],
) {
    history_profile(
        playback,
        consumer,
        locals,
        engine,
        owners,
        ProcessingProfile {
            ratio: 1.0,
            key_lock: false,
            piano_shift: 12,
            native_pitch: 2.0,
        },
    );
}

fn history_profile(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    _: &[Arc<InputRuntimeOwnership>; 2],
    profile: ProcessingProfile,
) {
    let peer = Peer::new(playback, profile);
    let DeclaredOutputs {
        first: first_wavs,
        second: second_wavs,
        components,
    } = declared_wavs();
    for (expected, baseline) in components.iter().zip(&playback.components) {
        assert_eq!(
            expected.samples.as_ref(),
            baseline.samples.as_ref(),
            "independent declared V1 WAV bytes must equal the established static reference"
        );
        assert!(!Arc::ptr_eq(&expected.samples, &baseline.samples));
    }
    let (actual, worker) = crate::audio_engine::buffer_retirement::create_audio_buffer_retirement();
    playback.callback.retirement.retired.clear();
    playback.callback.retirement.actual = Some(actual);
    let GenerationFixture {
        root,
        mut parameters,
        release,
    } = setup(playback, locals, engine, &first_wavs, &second_wavs, profile);
    let generation = voice(&playback.callback.mixer).generation;
    let source_window = key_lock_identity(engine);
    let hash = crate::audio_engine::stem_cache::source_version_hash(&Python::attach(|py| {
        locals
            .bind(py)
            .get_item("generate_source_version")
            .unwrap()
            .unwrap()
            .extract::<String>()
            .unwrap()
    }));
    generate(playback, consumer, locals, &peer, false);
    assert_eq!(key_lock_identity(engine), source_window);
    let ack1 = adopt(playback, consumer, locals, &peer, "generated1");
    assert!(playback.reference.set_stem_enabled_mask(0, 0b1010, hash));
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::AllStems, hash)
    );
    playback.select(Some(0b1010));
    let old = probe(
        &root,
        playback,
        locals,
        "generated_disk1",
        &components,
        [0, 1, 2, 3],
    );
    settle(playback, locals, &peer, "generated1", ack1, None);
    assert_eq!(key_lock_identity(engine), source_window);
    old.assert_retained();
    controller_python(
        locals,
        r#"
assert controller._verified_set_current(0)
repeat1_before = len(generate_demucs.calls)
assert controller.generate_stems_async(0)
assert len(generate_demucs.calls) == repeat1_before and not controller._jobs
assert not controller._requested_generations
assert controller.stem_generation_diagnostic(0) == 'Stems for the current model are already present'
"#,
    );
    let old_native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let old_adoption = voice(&playback.callback.mixer).stretch.adopted_request_id();
    generate(playback, consumer, locals, &peer, true);
    assert_eq!(key_lock_identity(engine), source_window);
    old.assert_retained();
    let ack2 = adopt(playback, consumer, locals, &peer, "generated2");
    // V2 swaps serialized vocals/melody. Actual1010 equals independently held
    // V1's1001; this changes audible feed while preserving complete alignment.
    assert!(playback.reference.set_stem_enabled_mask(0, 0b1001, hash));
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::AllStems, hash)
    );
    playback.select(Some(0b1001));
    let current = probe(
        &root,
        playback,
        locals,
        "generated_disk2",
        &components,
        [1, 0, 2, 3],
    );
    old.assert_retained();
    controller_python(
        locals,
        "del generated1, generated_live1, generate_job1, generated1_reopened\ncontroller.on_frame_render()",
    );
    playback.callback.retirement.slots = 0;
    settle(
        playback,
        locals,
        &peer,
        "generated2",
        ack2,
        Some((engine, &old)),
    );
    assert_eq!(key_lock_identity(engine), source_window);
    fifo(
        playback,
        &old,
        "V1 post-adoption production retirement FIFO",
    );
    assert_ne!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        old_native
    );
    assert_ne!(
        voice(&playback.callback.mixer).stretch.adopted_request_id(),
        old_adoption
    );
    cleanup(engine, locals, &old);
    current.assert_retained();
    for frames in [1, 31, 137, 511] {
        render(playback, &peer, frames);
        current.assert_retained();
    }
    controller_python(
        locals,
        r#"
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == generated_disk2.stem_set_identity
assert status.control_stem_set_identity == generated_disk2.stem_set_identity
assert status.effective_stem_set_identity == generated_disk2.stem_set_identity and status.effective == 'effective'
assert generated2.is_current() and generated2.claimed() and generated2.error() is None
assert generated2.effective_failure_reason() is None
assert controller._verified_set_current(0)
repeat2_before = len(generate_bs.calls)
assert controller.generate_stems_async(0)
assert len(generate_bs.calls) == repeat2_before and not controller._jobs
assert not controller._requested_generations
assert len(generate_demucs.calls) == len(generate_bs.calls) == 1
assert project.sample_paths[0] == generate_path and project.pad_content[0] == generate_content
assert session.active_sample_ids == {0, 215} and not session.paused_sample_ids
assert transport_events == [('started', 215), ('started', 0)]
"#,
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    prepare_public_unload_loader(locals);
    playback.callback.retirement.slots = 0;
    controller_python(
        locals,
        r#"
loader.unload_sample(0)
assert project.sample_paths[0] is None and project.stem_cache[0] is None
assert project.pad_stem_mix_mode[0] == 'full_mix'
del generated2, generated_live2, generate_job2, generated2_reopened
"#,
    );
    current.assert_retained();
    assert_eq!(
        playback.drain(consumer),
        0,
        "own public Unload still awaits retirement admission"
    );
    assert!(voice(&playback.callback.mixer).is_playing_sample(0));
    playback.callback.retirement.slots = usize::MAX;
    let before = playback.callback.feedback.events.len();
    let stopped_feedback =
        ExpectedStoppedFeedback::source_command(&playback.callback.mixer, consumer, 0);
    crate::audio_engine::audio_stream::drain_parameter_messages(
        &mut parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(parameters.is_empty());
    assert_eq!(
        playback.drain(consumer),
        4,
        "own public Unload/defaults ACK"
    );
    assert!(consumer.is_empty());
    assert!(playback.callback.mixer.bank_for_measurement()[0].is_none());
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(playback.callback.feedback.events.len(), before + 1);
    stopped_feedback.assert_message(&playback.callback.feedback.events[before]);
    complete::feedback(locals, &mut playback.callback);
    for frames in [1, 31, 137] {
        let mut silence = vec![0.0; frames * 2];
        playback.callback.mixer.render_rt_at_output_frame(
            &mut silence,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            &mut playback.callback.retirement,
        );
        assert!(silence.iter().all(|sample| *sample == 0.0));
        playback.frame += frames as u64;
        peer.assert_current(playback);
    }
    fifo(playback, &current, "V2 own public Unload production FIFO");
    cleanup(engine, locals, &current);
    controller_python(
        locals,
        r#"
assert session.active_sample_ids == {215}
assert not session.stem_generation_errors and not session.sample_load_errors
assert len(generate_demucs.calls) == len(generate_bs.calls) == 1
(generate_module.separator_model_cache_dir, generate_module.resolved_demucs_encoder,
 generate_module.encoder_contract, generate_module.bs_input_contract,
 generate_module.capture_processing_files, generate_module.processing_source_roots) = generate_saved_functions
"#,
    );
    playback.callback.retirement.actual.take();
    drop(worker);
    drop(release);
}
