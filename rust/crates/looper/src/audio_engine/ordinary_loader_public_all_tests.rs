//! First complete-source Play through the actual Python Loader and Transport.
//! The kernel is a cfg(test) Release binary, not the installed wheel's CPAL API.

use super::*;
use std::ffi::CString;

const PUBLIC_CHILD: &str = "FLITZI_ORDINARY_PUBLIC_COMPLETE_CHILD";
const PUBLIC_OFF_TEST: &str = "audio_engine::ordinary_loader_incoming_tests::ordinary_loader_public_all_tests::public_first_complete_source_play_off";
const PUBLIC_ON_TEST: &str = "audio_engine::ordinary_loader_incoming_tests::ordinary_loader_public_all_tests::public_first_complete_source_play_on";
const COMPLETE_FRAMES: usize = 997;

#[test]
fn public_first_complete_source_play_off() {
    isolated_history(false, PUBLIC_OFF_TEST);
}

#[test]
fn public_first_complete_source_play_on() {
    isolated_history(true, PUBLIC_ON_TEST);
}

fn isolated_history(key_lock: bool, test: &str) {
    if std::env::var_os(PUBLIC_CHILD).is_some() {
        for warm in [false, true] {
            for replacement in [false, true] {
                complete_history(warm, replacement, key_lock);
            }
        }
        eprintln!("public complete incoming child PID={}", std::process::id());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(PUBLIC_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "public complete incoming child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("public complete incoming child PID=")
    );
}

/// Forward source admission, ownership, tickets, launch and feedback unchanged.
/// The two stream-only scalar APIs use the same checked parameter kernels as
/// SavedRestoreAudio; they cannot supply a source, a ticket ACK or a voice.
#[pyclass]
struct PublicIncomingAudio {
    engine: Py<AudioEngine>,
    parameters: Arc<Mutex<rtrb::Producer<ControlParameterMessage>>>,
    accesses: Mutex<Vec<String>>,
    successes: Mutex<Vec<Py<PyDict>>>,
}

#[pymethods]
impl PublicIncomingAudio {
    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        let value = self.engine.bind(py).getattr(name)?.unbind();
        self.accesses.lock().unwrap().push(name.to_owned());
        Ok(value)
    }

    fn take_accesses(&self) -> Vec<String> {
        std::mem::take(&mut *self.accesses.lock().unwrap())
    }

    #[getter]
    fn received_successes(&self, py: Python<'_>) -> Vec<Py<PyDict>> {
        self.successes
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.clone_ref(py))
            .collect()
    }

    fn poll_loader_events(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let event = self.engine.borrow(py).poll_loader_events(py)?;
        if let Some(event) = &event {
            let value = event.bind(py);
            if value.get_item("type")?.extract::<String>()? == "success"
                && value.get_item("id")?.extract::<usize>()? == 0
            {
                // Retain detached scalar evidence only. The original event,
                // including its Original lease, belongs to the real Loader.
                let evidence = PyDict::new(py);
                evidence.set_item("id", value.get_item("id")?.extract::<usize>()?)?;
                evidence.set_item(
                    "request_id",
                    value.get_item("request_id")?.extract::<u64>()?,
                )?;
                evidence.set_item(
                    "cached_path",
                    value.get_item("cached_path")?.extract::<Option<String>>()?,
                )?;
                evidence.set_item(
                    "duration_s",
                    value.get_item("duration_s")?.extract::<f64>()?,
                )?;
                let native_defaults = value.get_item("incoming_defaults")?;
                let defaults = PyDict::new(py);
                defaults.set_item(
                    "revision",
                    native_defaults.get_item("revision")?.extract::<u64>()?,
                )?;
                for field in ["key_lock", "loop_auto"] {
                    defaults
                        .set_item(field, native_defaults.get_item(field)?.extract::<bool>()?)?;
                }
                for field in ["start_s", "grid_origin_s", "loop_bars"] {
                    defaults.set_item(field, native_defaults.get_item(field)?.extract::<f64>()?)?;
                }
                for field in ["end_s", "period_seconds"] {
                    defaults.set_item(
                        field,
                        native_defaults.get_item(field)?.extract::<Option<f64>>()?,
                    )?;
                }
                evidence.set_item("incoming_defaults", defaults)?;
                let native_analysis = value.get_item("analysis")?;
                let analysis = PyDict::new(py);
                analysis.set_item("bpm", native_analysis.get_item("bpm")?.extract::<f64>()?)?;
                analysis.set_item("key", native_analysis.get_item("key")?.extract::<String>()?)?;
                evidence.set_item("analysis", analysis)?;
                self.successes.lock().unwrap().push(evidence.unbind());
            }
        }
        Ok(event)
    }

    fn set_pad_gain(&self, id: usize, gain_db: f32) -> PyResult<()> {
        use crate::audio_engine::constants::{PAD_GAIN_DB_MAX, PAD_GAIN_DB_MIN};
        if id >= NUM_SAMPLES
            || !gain_db.is_finite()
            || !(PAD_GAIN_DB_MIN..=PAD_GAIN_DB_MAX).contains(&gain_db)
        {
            return Err(pyo3::exceptions::PyValueError::new_err("gain out of range"));
        }
        crate::audio_engine::push_parameter_message(
            &mut self.parameters.lock().unwrap(),
            ControlParameterMessage::SetPadGain { id, gain_db },
            "SetPadGain",
        )
    }

    fn set_pad_eq(&self, id: usize, low_db: f32, mid_db: f32, high_db: f32) -> PyResult<()> {
        use crate::audio_engine::constants::{PAD_EQ_DB_MAX, PAD_EQ_DB_MIN};
        if id >= NUM_SAMPLES
            || [low_db, mid_db, high_db].into_iter().any(|value| {
                !value.is_finite() || !(PAD_EQ_DB_MIN..=PAD_EQ_DB_MAX).contains(&value)
            })
        {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "eq gain out of range",
            ));
        }
        crate::audio_engine::push_parameter_message(
            &mut self.parameters.lock().unwrap(),
            ControlParameterMessage::SetPadEq {
                id,
                low_db,
                mid_db,
                high_db,
            },
            "SetPadEq",
        )
    }
}

fn public_python(locals: &Py<PyDict>, code: &str) {
    Python::attach(|py| {
        py.run(&CString::new(code).unwrap(), Some(locals.bind(py)), None)
            .unwrap_or_else(|error| {
                panic!("public ordinary controller script failed: {error:?}\n{code}")
            });
    });
}

fn public_bool(locals: &Py<PyDict>, code: &str) -> bool {
    Python::attach(|py| {
        py.eval(&CString::new(code).unwrap(), Some(locals.bind(py)), None)
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn public_request(locals: &Py<PyDict>) -> u64 {
    Python::attach(|py| {
        locals
            .bind(py)
            .get_item("request")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn public_setup(fixture: &Fixture, replacement: bool, key_lock: bool) -> Py<PyDict> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let project_cwd = material_paths::ordinary_windows_path(&std::env::current_dir().unwrap());
    let references = Python::attach(|py| {
        let engine = fixture.engine.borrow(py);
        let leases = engine.cold_leases.lock().unwrap();
        [0, 215].map(|id| {
            leases[id].as_ref().map(|lease| {
                let original_path = material_paths::ordinary_windows_path(&lease.original_path);
                Path::new(&original_path)
                    .strip_prefix(Path::new(&project_cwd))
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
        })
    });
    assert_eq!(references[0].is_some(), replacement);
    assert!(references[1].is_some());
    Python::attach(|py| {
        py.import("sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1(
                "insert",
                (0, repository.join("src").to_string_lossy().as_ref()),
            )
            .unwrap();
        let audio = Py::new(
            py,
            PublicIncomingAudio {
                engine: fixture.engine.clone_ref(py),
                parameters: fixture.parameters.clone(),
                accesses: Mutex::new(Vec::new()),
                successes: Mutex::new(Vec::new()),
            },
        )
        .unwrap();
        let locals = PyDict::new(py);
        locals.set_item("audio", audio).unwrap();
        locals
            .set_item("messages", py.get_type::<AudioMessage>())
            .unwrap();
        locals.set_item("references", references).unwrap();
        locals.set_item("replacement", replacement).unwrap();
        locals.set_item("key_lock", key_lock).unwrap();
        locals.set_item("rate", RATE).unwrap();
        locals.set_item("frames", COMPLETE_FRAMES).unwrap();
        locals
            .set_item("input_path", fixture.path.to_string_lossy().as_ref())
            .unwrap();
        locals.unbind()
    })
}

fn complete_history(warm: bool, replacement: bool, key_lock: bool) {
    eprintln!("public complete: warm={warm}, replacement={replacement}, ON={key_lock}");
    // The resampled analysis input has fewer than the QM 1024-frame minimum
    // and the key frontend's 8820-frame minimum, while every physical frame
    // has meaningful independent stereo PCM. BPM0/unknown is real analysis.
    let words = (0..COMPLETE_FRAMES)
        .map(|frame| {
            [
                4096 + (frame * 37 % 8191) as i16,
                -2048 - (frame * 53 % 4093) as i16,
            ]
        })
        .collect::<Vec<_>>();
    let mut fixture = Fixture::new(wav(&words), warm, replacement);
    let mut hold = WorkerHold::new(&fixture);
    let locals = public_setup(&fixture, replacement, key_lock);
    public_python(
        &locals,
        r#"
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import ProjectState, SessionState
from pathlib import Path
project, session = ProjectState(), SessionState()
project.key_lock = key_lock
project.multi_loop = True
project.speed = .75
project.sample_paths[0], project.sample_paths[215] = references
project.sample_durations[215] = 8192 / rate
project.pad_key_lock[215] = False
if replacement:
    project.sample_durations[0] = 16384 / rate
    project.manual_bpm[0] = 240.
    project.pad_gain_db[0] = -12.
transport = TransportController(project, session, audio)
loader = LoaderController(project, session, audio, transport.bpm.on_pad_bpm_changed)
transport.bind_incoming_global_key_lock(loader._refresh_incoming_global_intent)
transport.bind_current_pad_edit(
    loader.record_current_pad_edit, incoming_request=loader.incoming_load_request_id)
def incoming_loaded(sample_id):
    transport.residency.cancel(sample_id)
    transport.residency.remember(sample_id)
loader.set_new_sample_loaded_callback(
    lambda *_: None,
    key_lock_callback=lambda i: transport.residency.apply_key_lock(i, force=True),
    incoming_callback=incoming_loaded)
loader._assets.sync_assignments()
received_transport_events = []
def poll_transport():
    while (message := audio.receive_msg()) is not None:
        kind = message.feedback_kind()
        if kind in {'started', 'stopped'}:
            sample_id = audio._resolve_pad_feedback(message)
            assert sample_id == message.sample_id()
            received_transport_events.append((kind, sample_id))
            if kind == 'started':
                transport.playback.handle_sample_started_message(message, sample_id=sample_id)
            else:
                transport.playback.handle_sample_stopped_message(message, sample_id=sample_id)
        elif isinstance(message, messages.SampleStarted):
            received_transport_events.append(('started', message.sample_id()))
            transport.playback.handle_sample_started_message(message)
        elif isinstance(message, messages.SampleStopped):
            received_transport_events.append(('stopped', message.sample_id()))
            transport.playback.handle_sample_stopped_message(message)
    transport.on_frame_render()
guard_armed = False
source_reads, source_mutations = [], []
class ObservedValues(list):
    def __init__(self, name, values):
        super().__init__(values)
        self.name = name
    def __getitem__(self, index):
        if guard_armed and index == 0:
            source_reads.append(self.name)
        return super().__getitem__(index)
    def __setitem__(self, index, value):
        if guard_armed and index == 0:
            source_mutations.append(self.name)
        return super().__setitem__(index, value)
observed_fields = (
    'sample_durations', 'sample_analysis', 'manual_bpm', 'pad_loop_start_s',
    'pad_loop_end_s', 'pad_loop_auto', 'pad_loop_bars', 'pad_grid_anchor_s',
    'pad_grid_offset_samples', 'pad_key_lock', 'pad_timing_intent')
for field in observed_fields:
    # Pydantic assignment validation normalizes list subclasses. Retain this
    # read/mutation observer around the identical already validated values.
    object.__setattr__(project, field, ObservedValues(field, getattr(project, field)))
    assert isinstance(getattr(project, field), ObservedValues)
def assert_pending_public_rejection():
    global guard_armed
    before = {field: list.copy(getattr(project, field)) for field in observed_fields}
    active_before = set(session.active_sample_ids)
    source_reads.clear()
    source_mutations.clear()
    audio.take_accesses()
    guard_armed = True
    rejected = []
    try:
        for operation in (
            lambda: transport.loop.set_full_track_region(0),
            lambda: transport.playback.trigger_pad_keep_others(0),
        ):
            try:
                operation()
            except ValueError as error:
                assert 'Ordinary incoming assignment receipt is pending' in str(error), error
                rejected.append(True)
            else:
                rejected.append(False)
    finally:
        guard_armed = False
    # A genuinely unassigned first Play is a public no-op. A confirmed
    # predecessor assignment reaches the pending-receipt exception guard.
    assert rejected == [True, replacement], rejected
    assert not source_reads and not source_mutations, (source_reads, source_mutations)
    assert not audio.take_accesses(), 'pending public gesture reached a Native API'
    assert before == {field: list.copy(getattr(project, field)) for field in observed_fields}
    assert session.active_sample_ids == active_before
    assert loader.incoming_load_request_id(0) == request
loader.load_sample_async(0, input_path)
request = loader.incoming_load_request_id(0)
assert isinstance(request, int) and not isinstance(request, bool)
assert session.loading_sample_ids == {0} and not session.sample_load_errors
assert project.sample_paths[0] == references[0]
assert_pending_public_rejection()
# Request-addressed incoming APIs retain their own admission while public
# current-source gestures are fenced. Inheritance preserves the true mode.
loader.set_incoming_key_lock_override(0, request, override=None)
assert loader._incoming_loads[0].intent.revision == 1
assert project.pad_key_lock[0] is None
"#,
    );
    // Synchronize the auxiliary Session only with a genuine callback event.
    // It remains inaudible and is never relaunched by any Pad0 operation below.
    fixture.play_legacy(215, 0.0);
    public_python(
        &locals,
        "poll_transport()\nassert session.active_sample_ids == {215}",
    );
    let other_generation = voice(&fixture.callback.mixer, 215).generation;
    let other_native = voice(&fixture.callback.mixer, 215)
        .stretch
        .native_state_address();
    let other_fifo = voice(&fixture.callback.mixer, 215)
        .stretch
        .pending_fifo_frames();
    let old_position = fixture
        .old
        .as_ref()
        .map(|_| voice(&fixture.callback.mixer, 0).source_playback.position());
    hold.release();
    wait_until(|| fixture.callback.consumer.peek().is_ok());
    let request = public_request(&locals);
    let captured = match fixture.callback.consumer.peek().unwrap() {
        ControlMessage::LoadColdSample {
            id,
            sample,
            source_generation,
            incoming_defaults,
            adoption,
            resident_cancelled,
            ..
        } => {
            assert_eq!((*id, *source_generation), (0, request));
            Captured {
                sample: sample.clone(),
                defaults: incoming_defaults.unwrap(),
                adoption: adoption.clone(),
                cancelled: resident_cancelled.clone().unwrap(),
            }
        }
        _ => panic!("public Loader did not queue its actual incoming source"),
    };
    assert_eq!(captured.defaults.revision, 1);
    assert_eq!(captured.defaults.period_seconds, None);
    assert_eq!(
        (captured.defaults.start_frame, captured.defaults.end_frame),
        (0, None)
    );
    assert_eq!(captured.defaults.key_lock, key_lock);
    assert_complete_window(&captured.sample, &fixture, key_lock);
    public_python(
        &locals,
        "loader.poll_loader_events()\nassert not audio.received_successes\nassert_pending_public_rejection()",
    );
    assert_eq!(captured.adoption.load(Ordering::Acquire), 0);
    if let Some(old_position) = old_position {
        assert_eq!(
            voice(&fixture.callback.mixer, 0).source_playback.position(),
            old_position
        );
    } else {
        assert!(fixture.callback.mixer.bank_for_measurement()[0].is_none());
    }
    assert_eq!(fixture.callback.drain(), 1, "own actual Source ACK");
    assert_eq!(captured.adoption.load(Ordering::Acquire), 2);
    assert!(!captured.cancelled.load(Ordering::Acquire));
    assert!(
        fixture
            .callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.is_playing_sample(0))
    );
    wait_until(|| {
        Python::attach(|py| fixture.engine.borrow(py).cold_loading[0].load(Ordering::Acquire) == 0)
    });
    public_python(
        &locals,
        "assert not audio.received_successes\nassert session.loading_sample_ids == {0}\nassert project.sample_paths[0] == references[0]\nassert_pending_public_rejection()",
    );
    // Only the real Loader consumes and projects matching NativeSourceSuccess.
    wait_until(|| {
        public_python(&locals, "loader.poll_loader_events()");
        public_bool(&locals, "not session.loading_sample_ids")
    });
    capture_file_identities(&mut fixture);
    fixture.assert_disk(warm);
    public_python(
        &locals,
        r#"
assert not session.sample_load_errors, session.sample_load_errors
assert loader.incoming_load_request_id(0) is None
assert len(audio.received_successes) == 1
source_receipt = audio.received_successes[0]
assert source_receipt['request_id'] == request
assert source_receipt['incoming_defaults']['revision'] == 1
assert source_receipt['analysis']['bpm'] == 0.
assert source_receipt['analysis']['key'] == 'unknown'
assert project.sample_durations[0] == frames / rate
assert project.pad_loop_start_s[0] == 0. and project.pad_loop_end_s[0] is None
assert project.pad_loop_auto[0] and project.pad_key_lock[0] is None
assert project.manual_bpm[0] is None and project.pad_timing_intent[0] == 'legacy'
assert project.resolve_pad_key_lock(0) is key_lock
assert Path(project.sample_paths[0]).read_bytes() == Path(input_path).read_bytes()
assert ('original', 0) in loader._assets._assignments
assert audio.current_input_runtime_pad_binding(0).metadata()['source_generation'] == request
poll_transport()
assert session.active_sample_ids == {215}
assert received_transport_events == [('started', 215), ('stopped', 0)]
audio.set_pad_eq(0, -12., -3., 4.)
transport.loop.set_full_track_region(0)
assert project.pad_loop_start_s[0] == 0. and project.pad_loop_end_s[0] == frames / rate
assert project.pad_loop_auto[0] is False
mode_ticket = transport.residency._pending[0].ticket
assert mode_ticket is not None
transport.playback.trigger_pad_keep_others(0)
captured_pending, = transport.residency._captured_starts.values()
original_start = captured_pending.start
launch_ticket = captured_pending.ticket
assert launch_ticket is not None
assert (launch_ticket.start_frame, launch_ticket.end_frame, launch_ticket.full_frame_count) == (0, frames, frames)
assert 0 not in session.active_sample_ids
"#,
    );
    let original = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("original_start")
            .unwrap()
            .unwrap()
            .extract::<Py<CapturedPadStart>>()
            .unwrap()
            .borrow(py)
            .clone()
    });
    let capture_frame = fixture.callback.frame;
    let bank_before = fixture.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert_complete_window(&bank_before, &fixture, key_lock);
    assert!(
        fixture
            .callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.is_playing_sample(0)),
        "no Pad0 voice precedes its own public Resident ACK"
    );
    wait_until(|| {
        fixture.callback.drain();
        assert_ne!(
            original.state(),
            "rejected",
            "reason={:?}",
            original.reason()
        );
        if original.accepted() {
            return true;
        }
        public_python(
            &locals,
            "poll_transport()\nassert 0 not in session.active_sample_ids",
        );
        false
    });
    fixture.callback.wait_start_ready(&original);
    assert_eq!(original.original_frame(), Some(capture_frame));
    let execution = original.execution_frame().unwrap();
    assert!(execution >= capture_frame);
    if key_lock {
        assert!(execution > capture_frame);
        assert_eq!(original.effective_frame(), None);
    }
    public_python(
        &locals,
        r#"
assert launch_ticket.publication_status() == 'accepted'
assert launch_ticket.is_current()
descriptor = audio.loaded_residency(0)
assert descriptor['window_revision'] == launch_ticket.window_revision
assert (descriptor['resident_start_frame'], descriptor['resident_end_frame']) == (0, frames)
assert descriptor['resident_pcm_bytes'] == frames * 2 * 4
assert descriptor['context'] == (
    'full-track-key-lock-continuation-unproved-v1' if key_lock else 'full-track-v1'), descriptor
assert launch_ticket.key_lock_request_id == mode_ticket.key_lock_request_id
assert mode_ticket.publication_status() == 'accepted'
assert isinstance(mode_ticket.key_lock_request_id, int) and mode_ticket.key_lock_request_id > 0
assert mode_ticket.window_revision == launch_ticket.window_revision
def assert_own_mode(state, effective, ready):
    feedback = audio.pad_key_lock_status(0)
    assert feedback is not None
    assert feedback['source_generation'] == request, feedback
    assert feedback['source_identity'] == descriptor['source_identity'], feedback
    assert feedback['window_revision'] == launch_ticket.window_revision, feedback
    assert feedback['request_id'] == mode_ticket.key_lock_request_id, feedback
    assert feedback['state'] == state, feedback
    assert feedback['effective'] is effective and feedback['ready'] is ready, feedback
    assert feedback['error'] is None, feedback
assert_own_mode('armed' if key_lock else 'dry', key_lock, True)
assert 0 not in session.active_sample_ids
assert captured_pending.start is original_start
assert captured_pending.ticket is launch_ticket
"#,
    );
    let adopted = fixture.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert_complete_window(&adopted, &fixture, key_lock);
    assert!(adopted.same_source(&captured.sample));
    fixture.callback.complete_start(&original);
    assert_eq!(fixture.callback.frame, execution);
    assert_eq!(
        fixture.callback.mixer.loop_region_frames(0),
        (0, Some(COMPLETE_FRAMES))
    );
    public_python(
        &locals,
        "assert 0 not in session.active_sample_ids\npoll_transport()\nassert session.active_sample_ids == {0, 215}\nassert received_transport_events == [('started', 215), ('stopped', 0), ('started', 0)]\nassert original_start.state() == 'applied'\nassert original_start.effective_frame() == original_start.execution_frame()",
    );
    assert_eq!(fixture.callback.mixer.key_lock_for_measurement(0), key_lock);
    assert!(Arc::ptr_eq(
        &voice(&fixture.callback.mixer, 0)
            .sample
            .as_ref()
            .unwrap()
            .samples,
        &adopted.samples
    ));
    assert_eq!(
        voice(&fixture.callback.mixer, 0)
            .source_playback
            .position()
            .frame,
        0
    );
    let mut playback = oracle_after_public_birth(&fixture, key_lock, original);
    if key_lock {
        let current = voice(&fixture.callback.mixer, 0);
        assert_eq!(
            current.stretch.adopted_request_id(),
            Some(playback.original.sequence())
        );
        assert_eq!(
            current
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            prepared_native_history::PREPARED_HISTORY_FRAMES as u64
        );
        assert!(
            current.stretch.native_tap_observation().is_none(),
            "the real complete source needs no finite-window proof"
        );
        // Compare the entire first block, including the causal quiet lead from
        // independently prepared silent birth context at the first FIFO sample.
        let block = playback.raw.as_ref().unwrap().native.block_size();
        assert!(block > 0);
        for frames in [1, 31, 137, 512, 777].into_iter().cycle() {
            if playback.consumed == block as u64 {
                break;
            }
            let frames = frames.min(block - playback.consumed as usize);
            playback.render(&mut fixture, frames, false);
            public_python(&locals, "assert_own_mode('wet', True, True)");
        }
        assert!(playback.raw.as_ref().unwrap().used);
        playback.render(&mut fixture, 1, false);
        public_python(&locals, "assert_own_mode('wet', True, True)");
    }
    for frames in [1, 31, 137, 512, 777] {
        playback.render(&mut fixture, frames, false);
    }
    if key_lock {
        adopt_complete_native(&mut playback, &mut fixture);
    }
    for _ in 0..24 {
        playback.render(&mut fixture, 777, false);
    }
    assert!(
        playback.seams.len() >= 3,
        "independent complete-track wrap taps"
    );
    assert_eq!(
        voice(&fixture.callback.mixer, 215).generation,
        other_generation
    );
    assert_eq!(
        voice(&fixture.callback.mixer, 215)
            .stretch
            .native_state_address(),
        other_native
    );
    assert_eq!(
        voice(&fixture.callback.mixer, 215)
            .stretch
            .pending_fifo_frames(),
        other_fifo
    );
    fixture.assert_other();
    fixture.assert_disk(warm);
}

fn assert_complete_window(sample: &SampleBuffer, fixture: &Fixture, key_lock: bool) {
    assert_eq!(sample.frame_count(), COMPLETE_FRAMES);
    assert_eq!(
        (sample.resident_start(), sample.resident_end()),
        (0, COMPLETE_FRAMES)
    );
    assert_eq!(sample.samples.as_ref(), fixture.complete.samples.as_ref());
    assert_eq!(
        sample.samples.len() * size_of::<f32>(),
        COMPLETE_FRAMES * 2 * 4
    );
    assert_eq!(
        sample.residency.as_ref().unwrap().context,
        if key_lock {
            ResidentContext::KeyLockFullTrack
        } else {
            ResidentContext::FullTrack
        },
        "public complete extent must retain its real full-track context"
    );
}

/// Construct only independent oracles after the actual public voice exists.
/// Playback::new would itself call Engine.play_sample and steal voice birth.
fn oracle_after_public_birth(
    fixture: &Fixture,
    key_lock: bool,
    original: CapturedPadStart,
) -> Playback {
    let mut reference = Box::new(RtMixer::new(2, RATE as f32));
    let ownership = Arc::new(input_runtime_binding::InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &fixture.complete, RATE, 1);
    reference.set_input_runtime_ownership(ownership.clone());
    reference.load_sample(0, fixture.complete.clone());
    reference.set_loop_region_frames(0, 0, Some(COMPLETE_FRAMES));
    reference.set_speed(RATIO);
    if key_lock {
        // Independent complete PCM stages an independently owned ON receipt,
        // matching public fresh-voice startup rather than legacy untracked ON.
        let own_request = ownership.next_key_lock_request(0).unwrap();
        reference.set_pad_key_lock_request(
            0,
            true,
            own_request,
            input_runtime_binding::InputPadBinding {
                placement: ownership.binding_placement(0),
                resident: fixture.complete.resident_binding(),
                source_address: fixture.complete.source_address(),
                sample_count: fixture.complete.source_sample_count(),
                channels: fixture.complete.channels,
                sample_rate_hz: RATE,
                authority_revision: ownership.authority_revision(0),
                runtime_revision: 0,
                accepted: None,
            },
            1,
        );
    } else {
        reference.set_pad_key_lock(0, false);
    }
    reference.set_pad_eq(0, -12.0, -3.0, 4.0);
    assert!(reference.play_sample_at_output_frame(0, 1.0, fixture.callback.frame));
    if key_lock {
        prepare_reference_start(&mut reference, fixture.callback.frame);
    }
    Playback {
        reference,
        raw: key_lock.then(RawNative::prepared_start),
        filter: filter(),
        start: 0,
        end: COMPLETE_FRAMES,
        consumed: 0,
        seams: HashSet::new(),
        original,
    }
}

/// Timestamped continuation retains the first ordinary Native/FIFO owner.
fn adopt_complete_native(playback: &mut Playback, fixture: &mut Fixture) {
    assert_complete_window(
        fixture.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap(),
        fixture,
        true,
    );
    assert!(
        voice(&fixture.callback.mixer, 0)
            .stretch
            .native_tap_observation()
            .is_none()
    );
    playback.adopt_native(fixture);
    assert!(
        voice(&fixture.callback.mixer, 0)
            .stretch
            .native_tap_observation()
            .is_none()
    );
}
fn capture_file_identities(fixture: &mut Fixture) {
    fixture.file_identities = Python::attach(|py| {
        let engine = fixture.engine.borrow(py);
        let leases = engine.cold_leases.lock().unwrap();
        let lease = leases[0].as_ref().unwrap();
        let mut paths = vec![fixture.path.clone(), lease.original_path.clone()];
        paths.extend(
            ["decoder.f32le", "playback.f32le", "manifest.json"]
                .map(|leaf| lease.cache_path.join(leaf)),
        );
        paths
            .into_iter()
            .map(|path| {
                let identity = project_assets::capture_identity(&path).unwrap();
                assert!(identity.is_some());
                (path, identity)
            })
            .collect()
    });
}
