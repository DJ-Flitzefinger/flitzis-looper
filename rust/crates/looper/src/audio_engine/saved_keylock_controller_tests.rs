//! Fresh saved ON admission through the public Loader and ordinary pad Play.
//! No OFF preload supplies the initial source or its Native continuation.

use super::*;
use crate::audio_engine::native_history_permit::NativeHistoryContext;
use crate::audio_engine::pad_start::CapturedPadStart;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::prepared_native_history::NativeAdapterState;

/// The real Engine owns public loading, assets, source ACK and resident Play.
/// Only stream-bound scalar kernels are forwarded to the headless queues.
#[pyclass]
struct SavedRestoreAudio {
    engine: Py<AudioEngine>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    parameters: Arc<Mutex<rtrb::Producer<ControlParameterMessage>>>,
}

#[pymethods]
impl SavedRestoreAudio {
    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        Ok(self.engine.bind(py).getattr(name)?.unbind())
    }

    fn output_sample_rate(&self) -> u32 {
        RATE
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

    #[pyo3(signature=(id, start_s, end_s=None, *, expected_source_generation=None))]
    fn set_pad_loop_region(
        &self,
        py: Python<'_>,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        if id >= NUM_SAMPLES
            || !start_s.is_finite()
            || start_s < 0.0
            || end_s.is_some_and(|end| !end.is_finite() || end < 0.0)
        {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "loop bounds out of range",
            ));
        }
        let engine = self.engine.borrow(py);
        if expected_source_generation.is_some() {
            return engine.publish_pad_loop_region_with_source(
                id,
                start_s,
                end_s,
                expected_source_generation,
                Some((RATE, &self.producer)),
            );
        }
        reconcile(&engine)?;
        let _requests = engine.pad_request_ids.lock().unwrap();
        if let Some(sample) = engine.sample_cache.lock().unwrap()[id].as_ref() {
            let start = (start_s * f64::from(RATE)).round() as usize;
            let end = end_s.map(|end| (end * f64::from(RATE)).round() as usize);
            if let Some(region) = crate::audio_engine::source_reader::effective_loop_region(
                start,
                end,
                sample.frame_count(),
            ) && (region.start < sample.resident_start() || region.end > sample.resident_end())
            {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "loop requires a matching prepared resident window",
                ));
            }
        }
        let mut producer = self.producer.lock().unwrap();
        if producer.is_full() {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "SetPadLoopRegion queue full",
            ));
        }
        let authority = engine.input_runtime_ownership.next_authority(id)?;
        engine.note_resident_loop_intent(id, start_s, end_s)?;
        engine.input_runtime_ownership.revoke(id, authority);
        crate::audio_engine::push_control_message(
            &mut producer,
            ControlMessage::SetPadLoopRegion {
                id,
                start_s,
                end_s,
                expected_source_generation: None,
            },
            "SetPadLoopRegion",
        )
    }

    #[pyo3(signature=(id, bpm=None, *, expected_source_generation=None))]
    fn set_pad_bpm(
        &self,
        py: Python<'_>,
        id: usize,
        bpm: Option<f64>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        crate::audio_engine::constant_timing::publish_legacy_bpm_with_source(
            &self.engine.borrow(py),
            &self.parameters,
            id,
            bpm,
            expected_source_generation,
        )
    }

    #[pyo3(signature=(id, phase_anchor_s, *, expected_source_generation=None))]
    fn set_pad_timing_metadata(
        &self,
        py: Python<'_>,
        id: usize,
        phase_anchor_s: f64,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        crate::audio_engine::constant_timing::publish_legacy_origin_with_source(
            &self.engine.borrow(py),
            &self.producer,
            id,
            phase_anchor_s,
            expected_source_generation,
        )
    }
}

pub(super) fn run() {
    for explicit in [false, true] {
        for bpm_target in [false, true] {
            for extra in [0, 3] {
                history(explicit, bpm_target, extra);
            }
        }
    }
}

fn transport_event(callback: &Callback, event: &AudioMessage) -> Option<(&'static str, usize)> {
    match event {
        AudioMessage::SampleStarted { id } => Some(("started", *id)),
        AudioMessage::SampleStopped { id } => Some(("stopped", *id)),
        AudioMessage::ContentFeedback {
            id,
            owner,
            cell,
            lifetime,
            transport_revision,
            kind,
            ..
        } if *kind == 1 || *kind == 2 => {
            let ownership = callback.mixer.input_ownership_for_effect();
            let key = ownership
                .callback_feedback_key(*id)
                .expect("transport feedback retains current original content");
            assert_eq!(*owner, ownership.owner_identity().diagnostic());
            assert_eq!(*cell, key.cell().index());
            assert_eq!(*lifetime, key.lifetime());
            assert_eq!(
                *transport_revision,
                ownership
                    .transport_intent_for_cell(key.cell())
                    .load(std::sync::atomic::Ordering::Acquire)
            );
            let bank = callback.mixer.bank_for_measurement()[*id]
                .as_ref()
                .expect("transport feedback source remains loaded");
            assert!(ownership.source_current(*id, bank, RATE));
            if *kind == 1 {
                let voice = callback
                    .mixer
                    .voices
                    .iter()
                    .find(|voice| voice.is_playing_sample(*id))
                    .expect("accepted start feedback belongs to the actual active voice");
                let source = voice.sample.as_ref().unwrap();
                assert!(source.same_source(bank));
                assert_eq!(source.resident_binding(), bank.resident_binding());
                assert!(Arc::ptr_eq(&source.samples, &bank.samples));
                assert_eq!(
                    voice.source_admission.as_ref().unwrap().content_key(),
                    Some(key)
                );
            }
            Some((if *kind == 1 { "started" } else { "stopped" }, *id))
        }
        _ => None,
    }
}

fn poll(locals: &Py<PyDict>, callback: &mut Callback, feedback: &mut rtrb::Producer<AudioMessage>) {
    // Deliver the actual callback messages through the application's public
    // receive_msg and original-content resolver, then the real transport handlers.
    // No fabricated SampleStarted or source ACK settles the intent.
    let mut expected = Vec::new();
    for event in std::mem::take(&mut callback.feedback.events) {
        if let Some(event) = transport_event(callback, &event) {
            expected.push(event);
        }
        feedback.push(event).unwrap();
    }
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("expected_transport_events", expected)
            .unwrap();
    });
    controller_python(
        locals,
        r#"
received_messages, delivered_transport_events = [], []
while (message := audio.receive_msg()) is not None:
    received_messages.append((type(message).__name__, message.sample_id()))
    kind = message.feedback_kind()
    if kind in {'started', 'stopped'}:
        sample_id = audio._resolve_pad_feedback(message)
        assert sample_id == message.sample_id(), (sample_id, received_messages)
        delivered_transport_events.append((kind, sample_id))
        if kind == 'started':
            transport.playback.handle_sample_started_message(message, sample_id=sample_id)
        else:
            transport.playback.handle_sample_stopped_message(message, sample_id=sample_id)
    elif isinstance(message, messages.SampleStarted):
        delivered_transport_events.append(('started', message.sample_id()))
        transport.playback.handle_sample_started_message(message)
    elif isinstance(message, messages.SampleStopped):
        delivered_transport_events.append(('stopped', message.sample_id()))
        transport.playback.handle_sample_stopped_message(message)
assert delivered_transport_events == expected_transport_events, (
    expected_transport_events, delivered_transport_events, received_messages)
loader.poll_loader_events()
transport.on_frame_render()
"#,
    );
}

fn modes_settled(locals: &Py<PyDict>) -> bool {
    Python::attach(|py| {
        let residency = locals
            .bind(py)
            .get_item("transport")
            .unwrap()
            .unwrap()
            .getattr("residency")
            .unwrap();
        [0, 215].into_iter().all(|id| {
            !residency
                .call_method1("key_lock_status", (id,))
                .unwrap()
                .getattr("pending")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        })
    })
}

fn drain_at(
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    frame: u64,
) -> usize {
    drain_control_messages(
        consumer,
        &mut callback.scheduler,
        frame,
        &mut callback.quantization,
        &mut callback.transport,
        &mut callback.mixer,
        &mut callback.feedback,
        &mut callback.retirement,
    )
}

fn reference(
    source: &SampleBuffer,
    ratio: f64,
    execution: u64,
    pitch: PitchIntent,
    launch: LoopPhaseReference,
    timeline: &crate::audio_engine::transport::TransportTimeline,
) -> Box<RtMixer> {
    let mut reference = Box::new(RtMixer::new(2, RATE as f32));
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let acknowledgements =
        Arc::new(crate::audio_engine::constant_timing::CurrentTimingAcknowledgements::default());
    ownership.publish_test_source(0, source, RATE, 1);
    reference.set_input_runtime_ownership(ownership.clone());
    reference.set_current_timing_acknowledgements(acknowledgements.clone());
    reference.load_sample(0, source.clone());
    reference.set_pad_pitch_intent(0, pitch).unwrap();
    reference.set_loop_region_frames(0, LOOP_START, Some(LOOP_END));
    reference.set_speed(ratio);
    let request = ownership.next_key_lock_request(0).unwrap();
    reference.set_pad_key_lock_request(
        0,
        true,
        request,
        InputPadBinding {
            placement: ownership.binding_placement(0),
            resident: source.resident_binding(),
            source_address: source.source_address(),
            sample_count: source.source_sample_count(),
            channels: 2,
            sample_rate_hz: RATE,
            authority_revision: ownership.authority_revision(0),
            runtime_revision: 0,
            accepted: None,
        },
        1,
    );
    reference.set_pad_eq(0, -12.0, -3.0, 4.0);
    assert!(reference.play_sample_at_output_frame(0, 1.0, execution));
    install_public_start_reference(&mut reference, launch, timeline);
    // This complete-PCM comparison mixer has independent source authority. It
    // must begin at the same fresh physical origin with silent pre-origin history.
    let index = reference
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(0))
        .unwrap();
    let snapshot = reference.capture_pitch_voice_snapshot(index).unwrap();
    let epoch = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let permit = NativeHistoryContext {
        id: 0,
        ownership: &ownership,
        acknowledgements: &acknowledgements,
        preparation_epoch: &epoch,
    }
    .capture(&snapshot.sample, RATE, None)
    .unwrap();
    let request = snapshot
        .into_request(permit, pitch, true, execution, 1, 1)
        .unwrap();
    let mut candidate = Box::new(NativeAdapterState::new(
        Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
        2,
    ));
    candidate.prepare_source_at_physical_birth(request).unwrap();
    assert!(reference.adopt_prepared_pitch_voice(
        index,
        &mut candidate,
        execution,
        pitch,
        true,
        1,
        1,
    ));
    reference
}

fn prepared_raw_start(ratio: f64, extra: i32) -> ControllerRawNative {
    // Numeric semitones and the declared physical source rate are this oracle's
    // only pitch inputs; no live bank value or production pitch helper is read.
    let scale = 2.0_f64.powf(f64::from(extra) / 12.0) / ratio;
    let history_frames = ControllerRawNative::constant_history_frames(scale);
    let mut native = RubberBandLiveShifter::new(RATE, 2).unwrap();
    native.prepare_exact_pitch(scale).unwrap();
    let block = native.block_size();
    let mut raw = ControllerRawNative {
        native,
        input: std::array::from_fn(|_| VecDeque::new()),
        output: std::array::from_fn(|_| VecDeque::from(vec![0.0; block - 1])),
        used: false,
        pitch: scale,
    };
    // Independently declare silence before this new physical source's birth.
    // Keep the raw adapter's single initial FIFO lead; no remote loop tail or
    // second silence prefix may stand in for the complete first source content.
    let feed = std::array::from_fn(|_| vec![0.0; history_frames]);
    assert!(raw.render(&feed).iter().all(|sample| *sample == 0.0));
    assert!(raw.used);
    raw
}

fn native_status_diagnostic(callback: &Callback) -> String {
    let voice = voice(&callback.mixer);
    let ownership = callback.mixer.input_ownership_for_effect();
    let bank = callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
    let placement = ownership.slot_snapshot(0).unwrap();
    let request = ownership
        .key_lock_request_for_cell(placement.cell())
        .load(std::sync::atomic::Ordering::Acquire);
    let status = ownership.key_lock_status(0).map(|status| {
        (
            status.request_id,
            status.source_generation,
            status.source_address,
            status.window_revision,
            status.state,
            status.effective,
            status.ready,
        )
    });
    format!(
        "frame={} ratio={:?} target={:?} lock={} pitch={:?} native={} \
         wet_render={} wet_output={} current_output={} failed={} adapter={:?} \
         voice_timing={:?} request={} source={:?}/{} window={} status={status:?}",
        callback.transport.output_frame(),
        voice.source_playback.tempo_ratio(),
        voice.source_playback.rate_target(),
        callback.mixer.key_lock_for_measurement(0),
        callback.mixer.pad_pitch_intent(0),
        voice.stretch.native_state_address(),
        voice.stretch.wet_render_ready(),
        voice.stretch.wet_output_ready(),
        voice.stretch.wet_current_source_output_ready(),
        voice.stretch.wet_mode_failed(),
        voice.stretch.loop_acceptance_state(),
        voice.source_timing,
        request,
        ownership.callback_source_generation(0, bank, RATE),
        bank.source_address(),
        bank.window_revision(),
    )
}

fn history(explicit: bool, bpm_target: bool, extra: i32) {
    eprintln!(
        "saved initial ON: explicit={explicit}, public_bpm_target={bpm_target}, saved_extra={extra}"
    );
    let pitch = PitchIntent::new(0, extra).unwrap();
    assert_eq!(RATE, 48_000, "the real headless cold loader uses 48 kHz");
    Python::initialize();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let directory = tempfile::Builder::new()
        .prefix("saved-keylock-public-")
        .tempdir_in(repository.parent().unwrap().join("scratch"))
        .unwrap();
    let root = directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    let encoded = signal_sum_wav(4096, &[0, 1, 2, 3]);
    let input = root.join("saved-on.wav");
    fs::write(&input, &encoded).unwrap();
    // Material preparation creates genuine canonical storage but contributes
    // no complete PCM to the Engine. The independent WAV integers own the oracle.
    let material =
        crate::audio_engine::material_migration::prepare_material(&root, &input, RATE, 2, &|| {
            false
        })
        .unwrap();
    let saved_path = material.metadata()["new_reference"]
        .as_str()
        .unwrap()
        .to_owned();
    let original_before = fs::read(&material.lease.original_path).unwrap();
    let complete = SampleBuffer {
        channels: 2,
        samples: encoded[44..]
            .chunks_exact(2)
            .map(|bytes| f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32768.0)
            .collect::<Vec<_>>()
            .into(),
        residency: None,
    };
    let original = material.lease.original_path.clone();
    let preparation_pcm = Arc::downgrade(&material.sample.samples);
    let previous_cwd = std::env::current_dir().unwrap();
    struct RestoreCwd(PathBuf);
    impl Drop for RestoreCwd {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }
    let _restore_cwd = RestoreCwd(previous_cwd);
    // This TempDir inherits canonicalize's extended Windows prefix. Ordinary
    // Python startup uses the same Win32 spelling as the public asset resolver;
    // pathlib's lexical relative_to must compare those equal spellings.
    let project_cwd = crate::audio_engine::material_paths::ordinary_windows_path(directory.path());
    std::env::set_current_dir(&project_cwd).unwrap();
    let mut engine = AudioEngine::new().unwrap();
    let mut callback = Box::new(Callback {
        mixer: crate::audio_engine::audio_stream::create_stream_mixer(
            2,
            RATE,
            engine.current_timing_acknowledgements.clone(),
            engine.input_runtime_ownership.clone(),
            engine.prepared_source_epochs.clone(),
        )
        .unwrap(),
        scheduler: FixedCapacityScheduler::new(),
        transport: TransportTimeline::new(RATE),
        quantization: TriggerQuantization::Immediate,
        feedback: Feedback {
            slots: usize::MAX,
            events: Vec::new(),
        },
        retirement: Retirement {
            slots: usize::MAX,
            retired: Vec::new(),
            actual: None,
        },
    });
    callback
        .mixer
        .set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    callback
        .mixer
        .set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    callback
        .mixer
        .set_prepared_source_epochs(engine.prepared_source_epochs.clone());
    // The real synchronous restore may publish defaults for every missing pad.
    // Each bounded queue can hold all scalar defaults plus the two source jobs.
    let (producer, mut consumer) = rtrb::RingBuffer::new(NUM_SAMPLES * 4 + 8);
    let producer = Arc::new(Mutex::new(producer));
    let (mut feedback, feedback_rx) = rtrb::RingBuffer::new(64);
    let (parameters, mut parameter_consumer) = rtrb::RingBuffer::new(NUM_SAMPLES * 4 + 8);
    let parameters = Arc::new(Mutex::new(parameters));
    engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback_rx))));
    engine.test_output_format = Some((RATE, 2));
    engine.test_parameter_producer = Some(parameters.clone());
    let (engine, locals) = Python::attach(|py| {
        py.import("sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1(
                "insert",
                (0, repository.join("src").to_string_lossy().as_ref()),
            )
            .unwrap();
        let engine = Py::new(py, engine).unwrap();
        let audio = Py::new(
            py,
            SavedRestoreAudio {
                engine: engine.clone_ref(py),
                producer: producer.clone(),
                parameters: parameters.clone(),
            },
        )
        .unwrap();
        let locals = PyDict::new(py);
        locals.set_item("audio", audio).unwrap();
        locals
            .set_item("messages", py.get_type::<AudioMessage>())
            .unwrap();
        locals.set_item("saved_path", &saved_path).unwrap();
        locals.set_item("project_cwd", &project_cwd).unwrap();
        locals.set_item("explicit", explicit).unwrap();
        locals.set_item("bpm_target", bpm_target).unwrap();
        locals.set_item("saved_extra", extra).unwrap();
        locals.set_item("rate", RATE).unwrap();
        locals.set_item("start", LOOP_START).unwrap();
        locals.set_item("end", LOOP_END).unwrap();
        worker_python(
            py,
            &locals,
            r#"
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.project_materials import original_asset
from pathlib import Path
assert Path.cwd() == Path(project_cwd), (Path.cwd(), project_cwd)
assert original_asset(saved_path).path.relative_to(Path.cwd()).as_posix() == saved_path
project, session = ProjectState(), SessionState()
project.multi_loop = True
project.key_lock = not explicit
for sample_id in (0, 215):
    project.sample_paths[sample_id] = saved_path
    project.sample_durations[sample_id] = 4096 / rate
    project.pad_loop_auto[sample_id] = False
    project.pad_loop_start_s[sample_id] = start / rate
    project.pad_loop_end_s[sample_id] = end / rate
    project.pad_timing_intent[sample_id] = 'legacy'
project.pad_key_lock[0] = True if explicit else None
project.pad_key_lock[215] = False
project.pad_key_intent[0] = PadKeyIntent(
    source=SourceKeyVersion(raw_key='unrecognized saved source'),
    correction='saved arbitrary correction', extra_shift=saved_extra, retrigger=True)
transport = TransportController(project, session, audio)
loader = LoaderController(project, session, audio, transport.bpm.on_pad_bpm_changed)
def playback_state():
    return {
        'active': sorted(session.active_sample_ids),
        'paused': sorted(session.paused_sample_ids),
        'loading': sorted(session.loading_sample_ids),
        'load_errors': dict(session.sample_load_errors),
        'mode_status': {i: repr(transport.residency.key_lock_status(i)) for i in (0, 215)},
        'native_status': {i: audio.pad_key_lock_status(i) for i in (0, 215)},
        'pending': {i: repr(pending) for i, pending in transport.residency._pending.items()},
        'residency_errors': {i: repr(error) for i, error in transport.residency._errors.items()},
        'events': globals().get('received_messages', []),
    }
loader._assets.sync_assignments()
assert len(loader._assets._assignments) == 2
def refresh_key_lock(sample_id):
    transport.residency.cancel(sample_id)
    transport.residency.apply_key_lock(sample_id, force=True)
loader.set_new_sample_loaded_callback(lambda *_: None, key_lock_callback=refresh_key_lock)
if explicit:
    transport.pad.set_pad_key_lock(0, enabled=True)
else:
    transport.global_params.set_key_lock(enabled=True)
assert project.resolve_pad_key_lock(0) is True
assert project.pad_key_lock[0] is (True if explicit else None)
assert project.pad_key_lock[215] is False
assert audio.pad_key_lock_status(0) is None
"#,
        );
        (engine, locals.unbind())
    });
    // Real saved assignment owners now retain both originals. Only the
    // preparation alias goes away before actual initial source admission.
    drop(material);
    assert_eq!(preparation_pcm.strong_count(), 0);
    assert!(original.is_file());
    assert!(directory.path().join(&saved_path).is_file());
    let managed = crate::audio_engine::material_paths::resolve(&root, Path::new(&saved_path))
        .expect("the public restore reference must resolve in this actual samples root");
    assert_eq!(
        managed.path.canonicalize().unwrap(),
        original.canonicalize().unwrap()
    );
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::SetPadKeyLockSavedIntent {
            placement,
            enabled: true
        }) if placement.slot() == 0 && placement.lifetime() == 0
    ));
    assert_eq!(
        callback.drain(&mut consumer),
        1,
        "saved ON intent precedes actual Load"
    );
    assert!(callback.mixer.key_lock_for_measurement(0));
    assert!(callback.mixer.bank_for_measurement()[0].is_none());
    controller_python(
        &locals,
        r#"
from pathlib import Path
loader.restore_samples_from_project_state()
restore_state = {
    'loading': sorted(session.loading_sample_ids),
    'errors': dict(session.sample_load_errors),
    'deferred': {i: str(path) for i, path in loader._deferred_restores.items()},
    'pending_paths': dict(session.pending_sample_paths),
    'request_ids': dict(loader._load_request_ids),
    'stages': dict(session.sample_load_stage),
    'project_paths': {i: project.sample_paths[i] for i in (0, 215)},
    'saved_path': saved_path,
    'saved_exists': Path(saved_path).is_file(),
    'cwd': str(Path.cwd()),
    'output_rate': audio.output_sample_rate(),
    'key_lock': {i: project.resolve_pad_key_lock(i) for i in (0, 215)},
    'saved_loops': {i: (project.pad_loop_start_s[i], project.pad_loop_end_s[i]) for i in (0, 215)},
}
assert session.loading_sample_ids == {0, 215}, restore_state
assert not session.sample_load_errors, restore_state
assert not loader._deferred_restores, restore_state
assert set(loader._load_request_ids) == {0, 215}, restore_state
"#,
    );
    // Wait for both source writers to close their markers before one-shot ACKs.
    wait_until(|| consumer.slots() >= 2 && producer.try_lock().is_ok());
    let mut captured = [false; 2];
    wait_until(|| {
        if let Ok(ControlMessage::LoadColdSample { id, sample, .. }) = consumer.peek() {
            let index = usize::from(*id == 215);
            let pad_id = *id;
            assert!(*id == 0 || *id == 215);
            assert!(!captured[index]);
            assert_eq!(
                (
                    sample.resident_start(),
                    sample.resident_end(),
                    sample.frame_count()
                ),
                (LOOP_START, LOOP_END, 4096)
            );
            assert_eq!(sample.samples.len(), (LOOP_END - LOOP_START) * 2);
            assert_eq!(
                sample.residency.as_ref().unwrap().context,
                if *id == 0 {
                    crate::messages::ResidentContext::KeyLockFiniteLoop
                } else {
                    crate::messages::ResidentContext::FiniteLoop
                }
            );
            assert_eq!(
                sample.samples.as_ref(),
                &complete.samples[LOOP_START * 2..LOOP_END * 2]
            );
            callback.feedback.slots = 1;
            assert_eq!(callback.drain(&mut consumer), 1, "own initial source ACK");
            if pad_id == 0 {
                assert_eq!(callback.mixer.pad_pitch_intent(0), Some(pitch));
                assert!(
                    callback
                        .mixer
                        .voices
                        .iter()
                        .all(|voice| !voice.is_playing_sample(0))
                );
            }
            captured[index] = true;
        }
        captured.into_iter().all(|captured| captured)
    });
    callback.feedback.slots = usize::MAX;
    let finite = callback.mixer.bank_for_measurement()[0].clone().unwrap();
    let other = callback.mixer.bank_for_measurement()[215].clone().unwrap();
    assert_eq!(
        finite.residency.as_ref().unwrap().source.original_sha256,
        <[u8; 32]>::from(Sha256::digest(&encoded))
    );
    wait_until(|| {
        poll(&locals, &mut callback, &mut feedback);
        crate::audio_engine::audio_stream::drain_parameter_messages(
            &mut parameter_consumer,
            &mut callback.mixer,
            &mut callback.transport,
        );
        callback.drain(&mut consumer);
        Python::attach(|py| {
            locals
                .bind(py)
                .get_item("session")
                .unwrap()
                .unwrap()
                .getattr("loading_sample_ids")
                .unwrap()
                .len()
                .unwrap()
                == 0
        }) && consumer.is_empty()
            && modes_settled(&locals)
            && producer.try_lock().is_ok()
    });
    controller_python(
        &locals,
        r#"
transport.on_frame_render()
assert not session.sample_load_errors, playback_state()
assert not session.loading_sample_ids and not session.active_sample_ids, playback_state()
assert project.sample_paths[0] == project.sample_paths[215] == saved_path
assert project.pad_key_lock[0] is (True if explicit else None)
assert project.pad_key_lock[215] is False
assert audio.loaded_residency(0)['context'] == 'finite-keylock-source-coverage-v1'
feedback = audio.pad_key_lock_status(0)
assert feedback['request_id'] > 0 and feedback['state'] == 'armed', playback_state()
assert feedback['effective'] and feedback['ready'] and feedback['error'] is None, playback_state()
assert transport.residency.key_lock_status(0).effective is True, playback_state()
other_feedback = audio.pad_key_lock_status(215)
assert other_feedback['request_id'] > 0, playback_state()
assert other_feedback['effective'] is False and other_feedback['ready'], playback_state()
assert other_feedback['error'] is None, playback_state()
assert transport.residency.key_lock_status(215).effective is False, playback_state()
assert not transport.residency.key_lock_status(215).pending, playback_state()
if bpm_target:
    transport.bpm.set_manual_bpm(0, 120.0)
    assert transport.global_params.set_effective_display_bpm(87.6)
    assert project.manual_bpm[0] == 120.0
    assert abs(transport.global_params.effective_display_bpm() - 87.6) < 1e-10
else:
    transport.global_params.set_speed(.73)
assert abs(project.speed - .73) < 1e-15
"#,
    );
    wait_until(|| {
        crate::audio_engine::audio_stream::drain_parameter_messages(
            &mut parameter_consumer,
            &mut callback.mixer,
            &mut callback.transport,
        );
        callback.drain(&mut consumer);
        poll(&locals, &mut callback, &mut feedback);
        consumer.is_empty()
            && parameter_consumer.is_empty()
            && modes_settled(&locals)
            && producer.try_lock().is_ok()
    });
    callback.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    let launch_timeline = callback.transport;
    controller_python(
        &locals,
        r#"
transport.playback.trigger_pad_keep_others(0)
assert len(transport.residency._captured_starts) == 1, playback_state()
ordinary_start = next(iter(transport.residency._captured_starts.values())).start
assert ordinary_start.diagnostic_slot == 0
assert ordinary_start.effective_frame() is None
"#,
    );
    let ordinary_start = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("ordinary_start")
            .unwrap()
            .unwrap()
            .extract::<PyRef<'_, CapturedPadStart>>()
            .unwrap()
            .clone()
    });
    let capture_frame = callback.transport.output_frame();
    assert_eq!(capture_frame, 0);
    // Host preparation gets its bounded wait while the actual output clock is
    // fixed. Polling a window/worker never advances T, E or source playback.
    wait_until(|| {
        callback.drain(&mut consumer);
        poll(&locals, &mut callback, &mut feedback);
        assert_eq!(callback.transport.output_frame(), capture_frame);
        assert!(callback.mixer.voices.iter().all(|voice| !voice.active));
        assert_ne!(
            ordinary_start.state(),
            "rejected",
            "ordinary start rejected: {:?}",
            ordinary_start.reason()
        );
        callback.mixer.pitch_runtime.as_mut().unwrap().poll();
        callback
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(ordinary_start.sequence())
    });
    assert_eq!(ordinary_start.original_frame(), Some(capture_frame));
    let launch = LoopPhaseReference::capture(
        Some(launch_timeline),
        ordinary_start.original_frame().unwrap(),
    );
    let execution = ordinary_start.execution_frame().unwrap();
    assert!(
        execution > capture_frame,
        "non-unity Native start owns a later E"
    );
    assert_eq!(callback.scheduler.peek_next_target_frame(), Some(execution));
    assert!(ordinary_start.accepted());
    assert_eq!(ordinary_start.effective_frame(), None);
    let mut partitions = [1_usize, 127, 777, 96, 257, 512, 31].into_iter().cycle();
    while callback.transport.output_frame() < execution {
        let frame = callback.transport.output_frame();
        let frames = partitions.next().unwrap().min((execution - frame) as usize);
        let mut output = vec![0.0; frames * 2];
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        callback.transport.advance_by_rendered_frames(frames);
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert!(callback.mixer.voices.iter().all(|voice| !voice.active));
        assert_eq!(ordinary_start.effective_frame(), None);
    }
    assert_eq!(callback.transport.output_frame(), execution);
    // The production scheduler claims the due event at E without consuming its
    // first sample. This exposes the adopted endpoint before first-block checks.
    crate::audio_engine::audio_stream::render_scheduled_audio(
        &mut callback.mixer,
        &mut callback.scheduler,
        &mut [],
        &mut [0.0; NUM_SAMPLES],
        execution,
        2,
        &mut callback.transport,
        &mut callback.feedback,
        &mut callback.retirement,
    );
    assert_eq!(ordinary_start.state(), "applied");
    assert_eq!(ordinary_start.original_frame(), Some(capture_frame));
    assert_eq!(ordinary_start.execution_frame(), Some(execution));
    assert_eq!(ordinary_start.effective_frame(), Some(execution));
    assert!(callback.scheduler.is_empty());
    let initial_native = voice(&callback.mixer).stretch.native_state_address();
    assert_ne!(initial_native, 0);
    assert_eq!(
        voice(&callback.mixer).stretch.adopted_request_id(),
        Some(ordinary_start.sequence())
    );
    // Numeric source/display BPM values and saved semitones independently
    // define the Native preparation history and the unconsumed source birth at E.
    let ratio: f64 = (60.0 / 120.0) * (87.6 / 60.0);
    assert_eq!(ratio, 0.73);
    let history_frames =
        ControllerRawNative::constant_history_frames(2.0_f64.powf(f64::from(extra) / 12.0) / ratio);
    assert!(execution >= capture_frame);
    let initial_history = voice(&callback.mixer).stretch.productive_history().unwrap();
    assert_eq!(initial_history.fed_output_frames, history_frames as u64);
    assert_eq!(initial_history.next_position.frame, LOOP_START);
    assert_eq!(
        initial_history.next_position.fraction.to_bits(),
        0.0_f64.to_bits()
    );
    let prepared_taps = voice(&callback.mixer)
        .stretch
        .native_tap_observation()
        .expect("actual finite physical-birth preparation observation");
    assert_eq!(prepared_taps.missing_reads, 0);
    assert_eq!(prepared_taps.min_frame, None);
    assert_eq!(prepared_taps.max_frame, None);
    assert!(Arc::ptr_eq(
        &finite.samples,
        &voice(&callback.mixer).sample.as_ref().unwrap().samples
    ));
    assert!(callback.mixer.key_lock_for_measurement(0));
    assert_eq!(
        voice(&callback.mixer).source_playback.position().frame,
        LOOP_START
    );
    let (other_pending, other_current) = Python::attach(|py| {
        let engine = engine.borrow(py);
        (
            engine.input_runtime_ownership.resident_control_pending(215),
            engine
                .input_runtime_ownership
                .source_current(215, &other, RATE),
        )
    });
    assert!(
        callback.mixer.can_play_sample(215, 0.0),
        "real auxiliary launch unavailable: pending={other_pending}, current={other_current}, \
         loop={:?}, resident={:?}, key_lock={}, events={:?}",
        callback.mixer.loop_region_frames(215),
        (
            other.resident_start(),
            other.resident_end(),
            other.frame_count()
        ),
        callback.mixer.key_lock_for_measurement(215),
        callback.feedback.events,
    );
    assert!(!other_pending && other_current);
    assert_eq!(
        callback.mixer.loop_region_frames(215),
        (LOOP_START, Some(LOOP_END))
    );
    controller_python(
        &locals,
        "auxiliary_start = audio.capture_pad_start(215, start / rate, end / rate, 0.0)",
    );
    assert_eq!(
        drain_at(&mut callback, &mut consumer, execution),
        1,
        "the auxiliary original target is captured by the same callback clock"
    );
    controller_python(&locals, "assert audio.publish_pad_start(auxiliary_start)");
    assert_eq!(drain_at(&mut callback, &mut consumer, execution), 1);
    assert!(
        callback.feedback.events.iter().any(|event| {
            matches!(
                event,
                AudioMessage::ContentFeedback {
                    id: 215,
                    kind: 1,
                    ..
                }
            ) && transport_event(&callback, event) == Some(("started", 215))
        }),
        "own auxiliary start feedback missing: {:?}",
        callback.feedback.events,
    );
    assert!(
        callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(215))
    );
    poll(&locals, &mut callback, &mut feedback);
    controller_python(
        &locals,
        "assert session.active_sample_ids == {0, 215}, playback_state()",
    );
    // Both public histories request the independently declared physical SPEED.
    let published_intent = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("project")
            .unwrap()
            .unwrap()
            .getattr("speed")
            .unwrap()
            .extract::<f64>()
            .unwrap()
    });
    assert_eq!(published_intent.to_bits(), ratio.to_bits());
    assert!(
        voice(&callback.mixer)
            .source_playback
            .phase_reference_current(&launch_timeline)
    );
    let raw = prepared_raw_start(ratio, extra);
    let mut playback = ControllerPlayback {
        callback,
        reference: reference(&complete, ratio, execution, pitch, launch, &launch_timeline),
        full_mix: reference(&complete, ratio, execution, pitch, launch, &launch_timeline),
        frame: execution,
        source_frame: 0,
        resumed_at: None,
        seek_epoch: None,
        complete_resident: false,
        source: complete.clone(),
        components: std::array::from_fn(|_| complete.clone()),
        ratio,
        target_bpm: 87.6,
        loop_period: (LOOP_END - LOOP_START) as f64,
        loop_start: LOOP_START,
        loop_end: LOOP_END,
        rate_ledger: None,
        scheduled: true,
        selection: None,
        previous: None,
        ramp_start: 0,
        raw: Some(raw),
        candidate: None,
        filter: controller_filter(),
    };
    // Validate independent fresh references at E without consuming their first
    // source/Native/EQ output during the publication and preparation lead.
    playback.advance_expected_birth(capture_frame, execution);
    let raw = playback.raw.as_ref().unwrap();
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .pending_fifo_frames(),
        (raw.input[0].len(), raw.output[0].len()),
        "ordinary endpoint retains its fully prepared Native/FIFO owner"
    );
    let generation = voice(&playback.callback.mixer).generation;
    let block = voice(&playback.callback.mixer)
        .stretch
        .rubberband_block_size();
    playback.render(block, true);
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames,
        history_frames as u64 + block as u64
    );
    let mut initial_prefix = Vec::new();
    for count in [1, 31, 137, 512, 777, 96, 257, 1021, 1008, 256] {
        initial_prefix.extend(playback.render(count, false).0);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            initial_native
        );
    }
    assert!(
        initial_prefix.iter().any(|sample| sample.abs() > 1.0e-6),
        "the complete independently compared physical prefix must become audible"
    );
    let before_poll = native_status_diagnostic(&playback.callback);
    let current = voice(&playback.callback.mixer);
    assert_eq!(
        current.source_playback.tempo_ratio().to_bits(),
        ratio.to_bits()
    );
    assert!(
        current.stretch.wet_render_ready()
            && current.stretch.wet_output_ready()
            && current.stretch.wet_current_source_output_ready()
            && !current.stretch.wet_mode_failed(),
        "prepared ordinary source did not actually render wet: {before_poll}"
    );
    poll(&locals, &mut playback.callback, &mut feedback);
    let after_poll = native_status_diagnostic(&playback.callback);
    Python::attach(|py| {
        let engine = engine.borrow(py);
        let ownership = &engine.input_runtime_ownership;
        let placement = ownership.slot_snapshot(0).unwrap();
        let status = ownership.key_lock_status(0).unwrap();
        let request = ownership
            .key_lock_request_for_cell(placement.cell())
            .load(std::sync::atomic::Ordering::Acquire);
        let ack = engine
            .current_timing_acknowledgements
            .current_for_slot(ownership, 0);
        let diagnostic = format!("before: {before_poll}; after: {after_poll}; timing_ack={ack}");
        locals
            .bind(py)
            .set_item("native_start_diagnostic", &diagnostic)
            .unwrap();
        assert_eq!(status.request_id, request, "{diagnostic}");
        let bank = playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert_eq!(
            Some(status.source_generation),
            ownership.callback_source_generation(0, bank, RATE),
            "{diagnostic}"
        );
        assert_eq!(status.source_address, bank.source_address(), "{diagnostic}");
        assert_eq!(
            status.window_revision,
            bank.window_revision(),
            "{diagnostic}"
        );
    });
    controller_python(
        &locals,
        r#"
feedback = audio.pad_key_lock_status(0)
assert feedback['state'] == 'wet' and feedback['effective'] and feedback['ready'], (
    native_start_diagnostic, playback_state())
assert feedback['error'] is None and feedback['request_id'] > 0, playback_state()
assert transport.residency.key_lock_status(0).effective is True, playback_state()
assert project.pad_key_lock[0] is (True if explicit else None)
assert project.pad_key_intent[0].extra_shift == saved_extra
assert project.pad_key_intent[0].correction == 'saved arbitrary correction'
assert project.pad_key_intent[0].retrigger is True
"#,
    );
    let request = voice(&playback.callback.mixer).stretch.adopted_request_id();
    assert!(
        voice(&playback.callback.mixer)
            .stretch
            .productive_history()
            .is_some()
    );
    public_explicit_pause(&mut playback, &mut consumer, &locals);
    controller_python(
        &locals,
        "transport.playback.resume_pad(0); transport.playback.resume_pad(215)",
    );
    assert_eq!(playback.drain(&mut consumer), 2);
    for reference in [&mut playback.reference, &mut playback.full_mix] {
        reference.resume_sample_at_output_frame(0, playback.frame);
    }
    playback.resumed_at = Some(playback.frame);
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert_eq!(
        voice(&playback.callback.mixer).stretch.adopted_request_id(),
        request
    );
    // The ordinary attack already owns its prepared Native history. Resume
    // retains that accepted owner; it requires no later cold-to-warm exchange.
    for frames in [13, 1, 31, 137, 512] {
        playback.render(frames, true);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            initial_native
        );
    }
    assert_eq!(
        voice(&playback.callback.mixer).stretch.adopted_request_id(),
        request
    );
    let raw = playback.raw.as_ref().unwrap();
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .pending_fifo_frames(),
        (raw.input[0].len(), raw.output[0].len())
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert!(Arc::ptr_eq(
        &finite.samples,
        &voice(&playback.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .samples
    ));
    assert!(Arc::ptr_eq(
        &other.samples,
        &other_voice(&playback.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .samples
    ));
    let phase = (playback.source_frame as f64 * ratio).rem_euclid((LOOP_END - LOOP_START) as f64);
    let actual = voice(&playback.callback.mixer).source_playback.position();
    assert_eq!(actual.frame, LOOP_START + phase.floor() as usize);
    assert_eq!(actual.fraction.to_bits(), phase.fract().to_bits());
    let other_actual = other_voice(&playback.callback.mixer)
        .source_playback
        .position();
    // Both public voices actually start at E and pause together later.
    let other_frames = playback.source_frame;
    let other_phase = (other_frames as f64 * ratio).rem_euclid((LOOP_END - LOOP_START) as f64);
    assert_eq!(
        other_actual.frame,
        LOOP_START + other_phase.floor() as usize
    );
    assert_eq!(
        other_actual.fraction.to_bits(),
        other_phase.fract().to_bits()
    );
    Python::attach(|py| {
        let engine = engine.borrow(py);
        for id in [0, 215] {
            let cached = engine.sample_cache.lock().unwrap()[id].clone().unwrap();
            assert_eq!(cached.samples.len(), (LOOP_END - LOOP_START) * 2);
            assert_eq!(cached.frame_count(), 4096);
            assert!(engine.cold_leases.lock().unwrap()[id].is_some());
        }
    });
    assert_eq!(fs::read(original).unwrap(), original_before);
    controller_python(
        &locals,
        r#"
transport.on_frame_render()
assert not session.sample_load_errors, playback_state()
assert project.pad_key_lock[0] is (True if explicit else None)
assert project.pad_key_lock[215] is False
assert project.pad_loop_start_s[0] == start / rate and project.pad_loop_end_s[0] == end / rate
assert transport.residency.key_lock_status(0).effective is True, playback_state()
if bpm_target:
    assert project.manual_bpm[0] == 120.0 and project.pad_timing_intent[0] == 'manual'
    assert abs(transport.global_params.effective_display_bpm() - 87.6) < 1e-10
loader.shut_down()
"#,
    );
}
