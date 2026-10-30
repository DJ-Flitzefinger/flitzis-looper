//! Genuine public complete-source restore shares immutable PCM across three audible owners.
//! Distinct cold subscribers, public GLOBAL and ordered public unload retain their own ACKs.

use super::*;
use crate::audio_engine::resident_relocation::reconcile;

const RESTORED: [usize; 3] = [0, 36, 215];
const FRAMES: usize = 4096;

/// Same stream-bound scalar bridge as the saved public restore fixture. Loading,
/// source intent, cold admission, source ACK and transport remain actual Engine calls.
#[pyclass]
struct SharedRestoreAudio {
    engine: Py<AudioEngine>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    parameters: Arc<Mutex<rtrb::Producer<ControlParameterMessage>>>,
}

#[pymethods]
impl SharedRestoreAudio {
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
            return Err(pyo3::exceptions::PyValueError::new_err("EQ out of range"));
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
            return Err(pyo3::exceptions::PyValueError::new_err("loop out of range"));
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
                    "unprepared loop range",
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

const PROBE: &str = r#"
from keylock_controller_probe import Probe
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.project_materials import original_asset
from pathlib import Path
from collections import deque

class SharedRestoreProbe(Probe):
    def setup(self, saved_path, project_cwd):
        assert Path.cwd() == Path(project_cwd)
        assert original_asset(saved_path).path.relative_to(Path.cwd()).as_posix() == saved_path
        self.saved_path = saved_path
        self.original_starts = {}
        self.original_start_timestamps = {}
        self.pending_content_feedback = deque()
        for sample_id, override in ((0, None), (36, True), (215, False)):
            self.assign(sample_id, saved_path, False)
            content = self.project.pad_content[sample_id]
            self.project.pad_content[sample_id] = content.model_copy(
                update={"material_id": original_asset(saved_path).material_id})
            self.project.pad_key_lock[sample_id] = override
        self.loader = LoaderController(self.project, self.session, self.audio,
                                       self.transport.bpm.on_pad_bpm_changed)
        self.loader._assets.sync_assignments()
        assert len(self.loader._assets._assignments) == 3
        self.loader.set_new_sample_loaded_callback(lambda *_: None, key_lock_callback=self.refresh_mode)
        self.loader.set_sample_unloaded_callback(self.transport.residency.cancel)

    def refresh_mode(self, sample_id):
        self.transport.residency.cancel(sample_id)
        self.transport.residency.apply_key_lock(sample_id, force=True)

    def restore(self):
        self.loader.restore_samples_from_project_state()
        assert self.session.loading_sample_ids == {0, 36, 215}, (
            self.session.loading_sample_ids, self.session.sample_load_errors)
        assert not self.session.sample_load_errors
        assert not self.loader._deferred_restores
        assert set(self.loader._load_request_ids) == {0, 36, 215}
        self.restore_requests = dict(self.loader._load_request_ids)

    def poll(self):
        incoming = list(self.pending_content_feedback)
        self.pending_content_feedback.clear()
        while (message := self.audio.receive_msg()) is not None:
            incoming.append(message)
        for message in incoming:
            kind = message.feedback_kind()
            if kind is not None:
                try:
                    sample_id = self.audio._resolve_pad_feedback(message)
                except RuntimeError as error:
                    if 'content feedback placement pending' not in str(error):
                        raise
                    assert len(self.pending_content_feedback) < 1024
                    self.pending_content_feedback.append(message)
                    continue
                if sample_id is None:
                    continue
                if kind == 'started':
                    self.transport.playback.handle_sample_started_message(message, sample_id=sample_id)
                elif kind == 'stopped':
                    self.transport.playback.handle_sample_stopped_message(message, sample_id=sample_id)
            elif isinstance(message, self._messages.SampleStarted):
                self.transport.playback.handle_sample_started_message(message)
            elif isinstance(message, self._messages.SampleStopped):
                self.transport.playback.handle_sample_stopped_message(message)
        self.transport.on_frame_render()
        if hasattr(self, 'loader'):
            self.loader.poll_loader_events()
            self.transport.on_frame_render()

    def start(self, sample_id):
        timestamp = self.audio.capture_input_timestamp_ns()
        self.transport.playback.trigger_pad_keep_others(sample_id, received_at_ns=timestamp)
        originals = [pending for pending in self.transport.residency._captured_starts.values()
                     if pending.start.diagnostic_slot == sample_id]
        assert len(originals) == 1
        self.original_starts[sample_id] = originals[0]
        self.original_start_timestamps[sample_id] = timestamp
        assert originals[0].start.effective_frame() is None

    def original_start(self, sample_id):
        return self.original_starts[sample_id].start

    def original_start_window(self, sample_id):
        return self.original_starts[sample_id].ticket

    def original_start_timestamp(self, sample_id):
        return self.original_start_timestamps[sample_id]

    def finish_start(self, sample_id):
        original = self.original_starts.pop(sample_id)
        assert original.start.state() == 'applied'
        assert original.published and original.ticket is not None
        assert original.ticket.publication_status() == 'accepted' and original.ticket.is_current()
        assert sample_id in self.session.active_sample_ids
        self.original_start_timestamps.pop(sample_id)

    def restored(self):
        if self.session.loading_sample_ids:
            return False
        assert not self.session.sample_load_errors, self.session.sample_load_errors
        assert not self.loader._load_request_ids
        for sample_id, override in ((0, None), (36, True), (215, False)):
            assert self.project.sample_paths[sample_id] == self.saved_path
            assert self.project.pad_key_lock[sample_id] is override
            assert self.project.pad_loop_start_s[sample_id] == self.start_s
            assert self.project.pad_loop_end_s[sample_id] == self.end_s
            assert self.project.pad_timing_intent[sample_id] == 'legacy'
            if not self.settled(sample_id, sample_id == 36):
                return False
        return True

    def unload(self, sample_id):
        self.loader.unload_sample(sample_id)

    def assert_unloaded(self, sample_id):
        assert self.project.sample_paths[sample_id] is None
        assert sample_id not in self.session.active_sample_ids
        assert sample_id not in self.session.paused_sample_ids
        assert sample_id not in self.session.loading_sample_ids
        assert sample_id not in self.transport.residency._pending
"#;

struct RestoreCwd(PathBuf);

impl Drop for RestoreCwd {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

fn new_restore() -> (GlobalVoices, RestoreCwd, Vec<f32>, String) {
    let mut harness = Harness::new(false, &[]);
    let root = harness.directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    let input = root.join("shared.wav");
    let integers: Vec<i16> = (0..FRAMES)
        .map(|frame| (((frame * 211 + 37) % 16_384) as i32 - 8192) as i16)
        .collect();
    let interleaved: Vec<_> = integers.iter().flat_map(|integer| [*integer; 2]).collect();
    write_pcm16(&input, RATE, 2, &interleaved);
    let mono: Vec<_> = integers
        .into_iter()
        .map(|integer| f32::from(integer) / 32768.0)
        .collect();
    let stereo: Vec<_> = mono.iter().flat_map(|value| [*value; 2]).collect();
    let material =
        crate::audio_engine::material_migration::prepare_material(&root, &input, RATE, 2, &|| {
            false
        })
        .unwrap();
    let saved_path = material.metadata()["new_reference"]
        .as_str()
        .unwrap()
        .to_owned();
    let original = material.lease.original_path.clone();
    let preparation_pcm = Arc::downgrade(&material.sample.samples);
    let restore_cwd = RestoreCwd(std::env::current_dir().unwrap());
    let project_cwd =
        crate::audio_engine::material_paths::ordinary_windows_path(harness.directory.path());
    std::env::set_current_dir(&project_cwd).unwrap();
    // Synchronous restore can publish scalar defaults for all pads. Replace only
    // the empty headless producer queues; preserve Engine's genuine feedback owner.
    let (producer, consumer) = rtrb::RingBuffer::new(NUM_SAMPLES * 4 + 8);
    let producer = Arc::new(Mutex::new(producer));
    let (parameters, parameter_consumer) = rtrb::RingBuffer::new(NUM_SAMPLES * 4 + 8);
    let parameters = Arc::new(Mutex::new(parameters));
    Python::attach(|py| {
        let mut engine = harness.engine.borrow_mut(py);
        engine.test_control_channels.as_mut().unwrap().0 = producer.clone();
        engine.test_parameter_producer = Some(parameters.clone());
        engine.test_output_format = Some((RATE, 2));
    });
    harness.producer = producer.clone();
    harness.consumer = consumer;
    harness.parameters = parameter_consumer;
    harness.mono = mono;
    harness.probe = Python::attach(|py| {
        let audio = Py::new(
            py,
            SharedRestoreAudio {
                engine: harness.engine.clone_ref(py),
                producer,
                parameters,
            },
        )
        .unwrap();
        let module = PyModule::from_code(
            py,
            &CString::new(PROBE).unwrap(),
            c"public_shared_restore_probe.py",
            c"public_shared_restore_probe",
        )
        .unwrap();
        let probe = module
            .getattr("SharedRestoreProbe")
            .unwrap()
            .call1((
                audio,
                Vec::<(usize, String)>::new(),
                false,
                RATE,
                0,
                FRAMES,
                FRAMES,
                py.get_type::<AudioMessage>(),
            ))
            .unwrap();
        probe
            .call_method1("setup", (&saved_path, project_cwd))
            .unwrap();
        probe.unbind()
    });
    // ProjectAssets retains the canonical original. Its material preparation PCM
    // is gone before the first real restore subscriber can be admitted.
    drop(material);
    assert_eq!(preparation_pcm.strong_count(), 0);
    assert!(original.is_file());
    for id in RESTORED {
        assert!(harness.callback.mixer.bank_for_measurement()[id].is_none());
        Python::attach(|py| {
            assert!(harness.engine.borrow(py).sample_cache.lock().unwrap()[id].is_none())
        });
    }
    (
        GlobalVoices {
            harness,
            expected: Vec::new(),
            shared_pcm_pairs: Vec::new(),
        },
        restore_cwd,
        stereo,
        saved_path,
    )
}

fn drain_one_actual(harness: &mut Harness) {
    let Ok(message) = harness.consumer.pop() else {
        return;
    };
    // Partition the actual producer's ordered messages, without fabricating or
    // modifying their source/adoption fences. The same production drain owns ACK.
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    producer.push(message).unwrap();
    let callback = harness.callback.as_mut();
    assert_eq!(
        drain_control_messages(
            &mut consumer,
            &mut callback.scheduler,
            harness.frame,
            &mut callback.quantization,
            &mut callback.transport,
            &mut callback.mixer,
            &mut callback.feedback,
            &mut callback.retirement,
        ),
        1
    );
}

fn restore_sources(voices: &mut GlobalVoices, stereo: &[f32]) -> Vec<(usize, u64)> {
    voices.harness.call0("restore");
    let mut generations = Vec::new();
    let mut acknowledgements = Vec::new();
    // Observe each real source transaction before this sole callback claims it.
    // The source digest/device preparation gate binds complete PCM before returning
    // from prepare, so distinct queued subscribers can share it before any own ACK.
    wait_until(Duration::from_secs(10), || {
        if let Ok(ControlMessage::LoadColdSample {
            id,
            sample,
            source_generation,
            adoption,
            ..
        }) = voices.harness.consumer.peek()
        {
            assert!(RESTORED.contains(id));
            assert!(!generations.iter().any(|(previous, _)| previous == id));
            assert_eq!(adoption.load(Ordering::Acquire), 0);
            assert_eq!(
                (
                    sample.resident_start(),
                    sample.resident_end(),
                    sample.frame_count()
                ),
                (0, FRAMES, FRAMES)
            );
            assert_eq!(sample.samples.as_ref(), stereo);
            let own = (*id, *source_generation);
            let ack = adoption.clone();
            voices.harness.callback.retirement.capacity = 0;
            voices.harness.drain_native();
            assert_eq!(ack.load(Ordering::Acquire), 0);
            assert!(voices.harness.callback.mixer.bank_for_measurement()[own.0].is_none());
            voices.harness.callback.retirement.capacity = usize::MAX;
            drain_one_actual(&mut voices.harness);
            assert_eq!(
                ack.load(Ordering::Acquire),
                2,
                "pad {} own cold source ACK",
                own.0
            );
            generations.push(own);
            acknowledgements.push(ack);
            voices.harness.call0("poll");
        } else {
            drain_one_actual(&mut voices.harness);
            voices.harness.call0("poll");
        }
        generations.len() == RESTORED.len()
    });
    wait_until(Duration::from_secs(10), || {
        voices.harness.drain();
        Python::attach(|py| {
            voices
                .harness
                .probe
                .call_method0(py, "restored")
                .unwrap()
                .extract(py)
                .unwrap()
        })
    });
    for (id, generation) in &generations {
        Python::attach(|py| {
            assert_eq!(
                voices
                    .harness
                    .engine
                    .borrow(py)
                    .loaded_source_generations
                    .lock()
                    .unwrap()[*id],
                (*generation, RATE)
            )
        });
        assert_eq!(identity(&voices.harness, *id)[0], *generation);
    }
    assert!(
        acknowledgements
            .iter()
            .all(|ack| ack.load(Ordering::Acquire) == 2)
    );
    drop(acknowledgements);
    generations
}

fn shared_pcm(voices: &GlobalVoices) -> Weak<[f32]> {
    let banks = voices.harness.callback.mixer.bank_for_measurement();
    let first = banks[0].as_ref().unwrap();
    assert_eq!(first.samples.len(), FRAMES * 2);
    for id in [36, 215] {
        assert!(
            Arc::ptr_eq(&first.samples, &banks[id].as_ref().unwrap().samples),
            "complete public restore did not share actual PCM with pad {id}"
        );
    }
    println!("public complete restore actual PCM: 0=36=215 (one shared allocation)");
    Arc::downgrade(&first.samples)
}

fn assert_shared_active(voices: &GlobalVoices, pcm: &Weak<[f32]>) {
    for own in &voices.expected {
        let bank = voices.harness.callback.mixer.bank_for_measurement()[own.id]
            .as_ref()
            .unwrap();
        assert!(Weak::ptr_eq(pcm, &Arc::downgrade(&bank.samples)));
        assert!(Arc::ptr_eq(
            &bank.samples,
            &voice(&voices.harness, own.id)
                .sample
                .as_ref()
                .unwrap()
                .samples
        ));
        assert_eq!(
            (
                bank.resident_start(),
                bank.resident_end(),
                bank.frame_count()
            ),
            (0, FRAMES, FRAMES)
        );
    }
    assert!(pcm.strong_count() > 0);
}

fn start(voices: &mut GlobalVoices, id: usize, enabled: bool, eq: [f32; 3], gap: usize) {
    let original_frame = voices.harness.frame;
    voices.harness.target("start", id);
    let original = Python::attach(|py| {
        voices
            .harness
            .probe
            .call_method1(py, "original_start", (id,))
            .unwrap()
            .extract::<PyRef<crate::audio_engine::pad_start::CapturedPadStart>>(py)
            .unwrap()
            .clone()
    });
    let original_timestamp = Python::attach(|py| {
        voices
            .harness
            .probe
            .call_method1(py, "original_start_timestamp", (id,))
            .unwrap()
            .extract::<u64>(py)
            .unwrap()
    });
    assert_eq!(original.received_timestamp(), original_timestamp);
    wait_until(Duration::from_secs(10), || {
        voices.harness.drain();
        assert_ne!(
            original.state(),
            "rejected",
            "pad {id} original START: {:?}, T={:?}, E={:?}",
            original.reason(),
            original.original_frame(),
            original.execution_frame()
        );
        if original.state() == "applied" {
            return true;
        }
        let runtime = voices
            .harness
            .callback
            .mixer
            .pitch_runtime
            .as_mut()
            .unwrap();
        runtime.poll();
        runtime.has_ready_action(original.sequence())
    });
    assert_eq!(original.original_frame(), Some(original_frame));
    let execution = original.execution_frame().unwrap();
    assert_eq!(execution, original_frame + if enabled { 8192 } else { 0 });
    Python::attach(|py| {
        let ticket = voices
            .harness
            .probe
            .call_method1(py, "original_start_window", (id,))
            .unwrap();
        let ticket = ticket.extract::<PyRef<ResidentWindowTicket>>(py).unwrap();
        assert!(
            ticket
                .capture_pad_start_launch_guard(&original)
                .unwrap()
                .is_some(),
            "pad {id} must publish from its original member's own WindowACK"
        );
    });
    let callback = voices.harness.callback.as_mut();
    render_scheduled_audio(
        &mut callback.mixer,
        &mut callback.scheduler,
        &mut [],
        &mut [0.0; NUM_SAMPLES],
        original_frame,
        2,
        &mut callback.transport,
        &mut callback.feedback,
        &mut callback.retirement,
    );
    let execution = original.execution_frame().unwrap();
    // Settle the new birth before existing peers render, preserving every prefix.
    let lead = (execution - voices.harness.frame) as usize;
    if lead != 0 {
        voices.render(lead);
    }
    let callback = voices.harness.callback.as_mut();
    render_scheduled_audio(
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
    voices.harness.call0("poll");
    assert_eq!(original.effective_frame(), Some(execution));
    assert_eq!(original.received_timestamp(), original_timestamp);
    let mut expected = ExpectedVoice::new_at_range(&voices.harness, id, enabled, 0, FRAMES);
    expected.prepare_ordinary_start();
    let (frame, fraction, explicit) = expected.clock.physical_position(0);
    let actual = voice(&voices.harness, id);
    assert_eq!(actual.source_playback.position().frame, frame);
    assert!((actual.source_playback.position().fraction - fraction).abs() < 1e-8);
    assert!(!explicit);
    voices.harness.target("finish_start", id);
    if enabled {
        let actual = voice(&voices.harness, id);
        assert_eq!(
            actual.stretch.adopted_request_id(),
            Some(original.sequence())
        );
        assert_eq!(
            actual
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            HISTORY as u64
        );
    }
    Python::attach(|py| {
        voices
            .harness
            .engine
            .borrow(py)
            .test_parameter_producer
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .push(ControlParameterMessage::SetPadEq {
                id,
                low_db: eq[0],
                mid_db: eq[1],
                high_db: eq[2],
            })
            .unwrap()
    });
    for (slot, db) in [
        DspParameterSlot::Slot0,
        DspParameterSlot::Slot1,
        DspParameterSlot::Slot2,
    ]
    .into_iter()
    .zip(eq)
    {
        assert!(expected.filter.set_parameter(
            DspParameterId::per_pad(id, DspNodeSlot::Slot0, slot).unwrap(),
            fixture_eq_normalized(db)
        ));
    }
    voices.expected.push(expected);
    voices.harness.drain();
    voices.render(gap);
}

fn global(voices: &mut GlobalVoices, enabled: bool) -> Py<ResidentWindowTicket> {
    let frozen: Vec<_> = RESTORED
        .map(|id| FrozenVoice::capture(&voices.harness, id))
        .into();
    voices.harness.callback.retirement.capacity = 0;
    voices.harness.global(enabled);
    pending(&voices.harness, 0, enabled);
    let ticket = voices.harness.ticket(0);
    wait_until(Duration::from_secs(10), || {
        voices.harness.drain();
        Python::attach(|py| ticket.borrow(py).publication_status() == "pending")
    });
    for (id, before) in RESTORED.into_iter().zip(&frozen) {
        before.assert_exact(&voices.harness, id);
    }
    assert!(!accepted(&ticket));
    voices.harness.callback.retirement.capacity = usize::MAX;
    wait_until(Duration::from_secs(10), || {
        voices.harness.drain();
        accepted(&ticket)
    });
    frozen[0].assert_source_eq(&voices.harness, 0);
    frozen[1].assert_exact(&voices.harness, 36);
    frozen[2].assert_exact(&voices.harness, 215);
    let window: u64 = Python::attach(|py| {
        ticket
            .bind(py)
            .getattr("window_revision")
            .unwrap()
            .extract()
            .unwrap()
    });
    assert_eq!(
        identity(&voices.harness, 0),
        [
            frozen[0].receipt[0],
            frozen[0].receipt[1],
            window,
            voices.harness.mode_request(&ticket)
        ]
    );
    assert!(Python::attach(|py| ticket.borrow(py).is_current()));
    assert_eq!(
        voices.harness.callback.mixer.key_lock_for_measurement(0),
        enabled
    );
    voices
        .expected
        .iter_mut()
        .find(|own| own.id == 0)
        .unwrap()
        .set_mode(&voices.harness, enabled);
    voices.harness.assert_origin(0, None);
    voices.harness.assert_origin(36, Some(true));
    voices.harness.assert_origin(215, Some(false));
    ticket
}

fn worker_marker(voices: &mut GlobalVoices) -> Weak<AtomicU8> {
    let marker = Arc::new(AtomicU8::new(0));
    let done = Arc::downgrade(&marker);
    voices
        .harness
        .callback
        .retirement
        .retire_cold_adoption(marker);
    done
}

fn drain_readers(voices: &mut GlobalVoices, done: &Weak<AtomicU8>, condition: impl Fn() -> bool) {
    wait_until(Duration::from_secs(10), || {
        voices.harness.drain();
        Python::attach(|py| reconcile(&voices.harness.engine.borrow(py)).unwrap());
        done.strong_count() == 0 && condition()
    });
}

fn unload(voices: &mut GlobalVoices, id: usize, pcm: &Weak<[f32]>, final_owner: bool) {
    let frozen: Vec<_> = voices
        .expected
        .iter()
        .filter(|own| own.id != id)
        .map(|own| (own.id, FrozenVoice::capture(&voices.harness, own.id)))
        .collect();
    voices.harness.callback.retirement.capacity = 0;
    voices.harness.target("unload", id);
    assert!(voice(&voices.harness, id).active);
    voices.harness.drain_native();
    assert!(
        voice(&voices.harness, id).active,
        "request is not the ordered Unload/STOP ACK"
    );
    for (other, before) in &frozen {
        before.assert_exact(&voices.harness, *other);
    }
    voices.harness.callback.retirement.capacity = usize::MAX;
    voices.harness.drain();
    assert!(voices.harness.callback.mixer.bank_for_measurement()[id].is_none());
    assert!(
        !voices
            .harness
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(id))
    );
    voices.harness.target("assert_unloaded", id);
    for (other, before) in &frozen {
        before.assert_exact(&voices.harness, *other);
    }
    voices.expected.retain(|own| own.id != id);
    voices.render(if final_owner { 1 } else { 257 });
    let done = worker_marker(voices);
    drain_readers(voices, &done, || !final_owner || pcm.strong_count() == 0);
    if final_owner {
        assert_eq!(
            pcm.strong_count(),
            0,
            "last shared PCM reader outlived public unload and real workers"
        );
        assert!(voices.expected.is_empty());
    } else {
        assert_shared_active(voices, pcm);
        voices.assert_receipts();
    }
}

#[test]
fn actual_public_complete_restore_shared_pcm_global_modes_and_last_unload_readers() {
    let (mut voices, restore_cwd, stereo, saved_path) = new_restore();
    let generations = restore_sources(&mut voices, &stereo);
    let pcm = shared_pcm(&voices);
    Python::attach(|py| {
        voices
            .harness
            .engine
            .borrow_mut(py)
            .set_speed(RATIO)
            .unwrap()
    });
    voices.harness.drain();
    for (id, enabled, eq, gap) in [
        (0, false, [-12.0, -3.0, 4.0], 53),
        (36, true, [-6.0, 2.0, -4.0], 94),
        (215, false, [-3.0, -5.0, 1.0], 193),
    ] {
        start(&mut voices, id, enabled, eq, gap);
    }
    // Each ordinary public START owns a coupled complete 0..N loop publication.
    // Initial saved OFF hints may still use their existing FiniteLoop classifier.
    for (id, context) in [
        (0, ResidentContext::FullTrack),
        (36, ResidentContext::KeyLockFullTrack),
        (215, ResidentContext::FullTrack),
    ] {
        assert_eq!(
            voices.harness.callback.mixer.bank_for_measurement()[id]
                .as_ref()
                .unwrap()
                .residency
                .as_ref()
                .unwrap()
                .context,
            context
        );
    }
    assert_shared_active(&voices, &pcm);
    for (id, generation) in generations {
        assert_eq!(identity(&voices.harness, id)[0], generation);
    }
    voices.render(5000);
    native_wet(&voices.harness, 36);
    voices.assert_receipts();
    let on_ticket = global(&mut voices, true);
    voices.render(6000);
    for id in [0, 36] {
        native_wet(&voices.harness, id);
    }
    assert!(voices.harness.settled(215, false));
    assert_shared_active(&voices, &pcm);
    let old_on = Arc::downgrade(
        voices.harness.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .residency
            .as_ref()
            .unwrap(),
    );
    assert_eq!(
        voices.harness.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .residency
            .as_ref()
            .unwrap()
            .context,
        ResidentContext::KeyLockFullTrack
    );
    let off_ticket = global(&mut voices, false);
    assert_eq!(
        voices.harness.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .residency
            .as_ref()
            .unwrap()
            .context,
        ResidentContext::FullTrack
    );
    voices.render(257);
    drop(on_ticket);
    drop(off_ticket);
    let done = worker_marker(&mut voices);
    drain_readers(&mut voices, &done, || old_on.strong_count() == 0);
    assert_eq!(old_on.strong_count(), 0);
    assert_shared_active(&voices, &pcm);
    assert!(voices.harness.settled(0, false));
    native_wet(&voices.harness, 36);
    voices.assert_receipts();
    for own in &voices.expected {
        assert!(
            own.energy > 1.0,
            "pad {} was not independently audible",
            own.id
        );
        if [0, 36].contains(&own.id) {
            assert!(own.wet_difference > 10.0);
        }
    }
    // Independent serialized reference bytes are not an alias of the actual shared
    // allocation; release all other fixture owners before proving its final reader.
    drop(stereo);
    drop(saved_path);
    unload(&mut voices, 0, &pcm, false);
    native_wet(&voices.harness, 36);
    unload(&mut voices, 36, &pcm, false);
    assert!(voices.harness.settled(215, false));
    unload(&mut voices, 215, &pcm, true);
    // Engine, cold workers, preparation worker and retirement worker are all still
    // alive at Weak-zero; no stream/device or disk-deletion claim is made here.
    assert_eq!(pcm.strong_count(), 0);
    drop(voices);
    drop(restore_cwd);
}
