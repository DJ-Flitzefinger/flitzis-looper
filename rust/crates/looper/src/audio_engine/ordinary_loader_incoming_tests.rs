//! Actual incoming Engine loads, callback ACKs and productive first voice histories.
//! Only the single-test child changes cwd; no facade supplies audio or source ACKs.

use super::*;
use crate::audio_engine::audio_stream::{
    drain_control_messages, drain_parameter_messages, execute_scheduled_command,
    render_scheduled_audio,
};
use crate::audio_engine::buffer_retirement::{AudioBufferRetirement, RetiredAudioBuffer};
use crate::audio_engine::dsp::{DspNodeSlot, DspParameterId, DspParameterSlot, PerPadDspChain};
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::native_history_permit::NativeHistoryContext;
use crate::audio_engine::pad_start::CapturedPadStart;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::prepared_native_history::NativeAdapterState;
use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::source_reader::{
    reset_tap_observation_for_test, tap_observation_for_test,
};
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, IncomingLoadDefaults, ResidentContext, TriggerQuantization};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const RATIO: f64 = 0.75;
const CHILD: &str = "FLITZI_ORDINARY_INCOMING_NATIVE_CHILD";
const TEST: &str = "audio_engine::ordinary_loader_incoming_tests::ordinary_loader_incoming_public_cold_warm_first_replacement_and_native_history";

#[test]
fn ordinary_loader_incoming_public_cold_warm_first_replacement_and_native_history() {
    if std::env::var_os(CHILD).is_some() {
        for warm in [false, true] {
            for replacement in [false, true] {
                for key_lock in [false, true] {
                    defaults_history(warm, replacement, key_lock);
                }
            }
        }
        for key_lock in [false, true] {
            latest_timing_history(key_lock);
            no_bpm_history(key_lock);
        }
        short_source_history();
        claimed_revision_history();
        updating_revision_rejects_queued_history();
        stale_queued_history();
        predecessor_edit_admission_history();
        predecessor_bpm_coalescing_history();
        for exclusive in [false, true] {
            predecessor_scheduled_start_history(exclusive);
        }
        retirement_pressure_history();
        queue_pressure_history();
        eprintln!("ordinary incoming native child PID={}", std::process::id());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "actual ordinary incoming child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("ordinary incoming native child PID=")
    );
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "ordinary incoming worker/Native did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wav(words: &[[i16; 2]]) -> Vec<u8> {
    let size = u32::try_from(words.len() * 4).unwrap();
    let mut encoded = Vec::with_capacity(size as usize + 44);
    encoded.extend_from_slice(b"RIFF");
    encoded.extend_from_slice(&(size + 36).to_le_bytes());
    encoded.extend_from_slice(b"WAVEfmt ");
    encoded.extend_from_slice(&16_u32.to_le_bytes());
    encoded.extend_from_slice(&1_u16.to_le_bytes());
    encoded.extend_from_slice(&2_u16.to_le_bytes());
    encoded.extend_from_slice(&RATE.to_le_bytes());
    encoded.extend_from_slice(&(RATE * 4).to_le_bytes());
    encoded.extend_from_slice(&4_u16.to_le_bytes());
    encoded.extend_from_slice(&16_u16.to_le_bytes());
    encoded.extend_from_slice(b"data");
    encoded.extend_from_slice(&size.to_le_bytes());
    for frame in words {
        for word in frame {
            encoded.extend_from_slice(&word.to_le_bytes());
        }
    }
    encoded
}

fn pulse_track() -> Vec<u8> {
    // Eighteen seconds of independently encoded 120-BPM attacks and quiet
    // stereo content. The first attack follows a fixed 1024-frame lead-in.
    // BPM estimation itself is not retroactively asserted to be accepted timing.
    let words = (0..18 * RATE as usize)
        .map(|frame| {
            if frame < 1024 {
                return [0, 0];
            }
            let beat_frame = (frame - 1024) % (RATE as usize / 2);
            if beat_frame < 256 {
                let decay = 256 - beat_frame;
                let value = ((beat_frame * 37 % 31) as i32 - 15) * decay as i32;
                [value as i16, (-value / 2) as i16]
            } else {
                [
                    ((frame * 13 % 511) as i16) - 255,
                    ((frame * 19 % 503) as i16) - 251,
                ]
            }
        })
        .collect::<Vec<_>>();
    wav(&words)
}

fn independent_pcm(encoded: &[u8]) -> SampleBuffer {
    SampleBuffer {
        channels: 2,
        samples: encoded[44..]
            .chunks_exact(2)
            .map(|bytes| f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32768.0)
            .collect::<Vec<_>>()
            .into(),
        residency: None,
    }
}

fn activity_frame(source: &SampleBuffer) -> usize {
    source
        .samples
        .chunks_exact(2)
        .position(|frame| {
            frame
                .iter()
                .any(|value| value.is_finite() && value.abs() > 0.01)
        })
        .unwrap_or(0)
        .saturating_sub(1)
}

struct Cwd(PathBuf);
impl Drop for Cwd {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

struct Retirement {
    slots: usize,
    owners: Vec<RetiredAudioBuffer>,
}

macro_rules! retire {
    ($name:ident, $ty:ty, $variant:ident) => {
        fn $name(&mut self, value: $ty) {
            self.owners.push(RetiredAudioBuffer::$variant(value));
        }
    };
}

impl AudioBufferRetirement for Retirement {
    retire!(retire_sample, SampleBuffer, Sample);
    retire!(
        retire_prepared_stems,
        crate::messages::PreparedStemSet,
        PreparedStems
    );
    retire!(
        retire_constant_timing,
        constant_timing::PreparedConstantTiming,
        ConstantTiming
    );
    retire!(
        retire_cold_adoption,
        Arc<std::sync::atomic::AtomicU8>,
        ColdAdoption
    );
    retire!(
        retire_resident_cancellation,
        Arc<AtomicBool>,
        ResidentCancellation
    );
    retire!(
        retire_resident_transaction,
        Box<crate::messages::ResidentTransaction>,
        ResidentTransaction
    );
    retire!(
        retire_resident_capture,
        Arc<resident_seek::ResidentSeekCapture>,
        ResidentSeekCapture
    );
    retire!(
        retire_global_playback_batch,
        Arc<global_playback_batch::GlobalPlaybackBatch>,
        GlobalPlaybackBatch
    );
    retire!(
        retire_accepted_timing_refresh,
        Arc<accepted_timing_refresh::AcceptedTimingRefresh>,
        AcceptedTimingRefresh
    );
    fn available_retirement_slots(&mut self) -> usize {
        self.slots
    }
}

struct Callback {
    mixer: Box<RtMixer>,
    consumer: rtrb::Consumer<ControlMessage>,
    parameters: rtrb::Consumer<ControlParameterMessage>,
    feedback: rtrb::Producer<AudioMessage>,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    quantization: TriggerQuantization,
    retirement: Retirement,
    frame: u64,
    other_consumed: u64,
}

impl Callback {
    fn queued_start(&self) -> CapturedPadStart {
        let ControlMessage::CapturePadStartTarget(original) = self.consumer.peek().unwrap() else {
            panic!("ordinary public Play must retain its original target before PreparedStart");
        };
        original.clone()
    }

    fn wait_start_ready(&mut self, original: &CapturedPadStart) {
        let frame = self.frame;
        wait_until(|| {
            assert_ne!(
                original.state(),
                "rejected",
                "reason={:?}",
                original.reason()
            );
            if original.state() == "applied" {
                return true;
            }
            let runtime = self.mixer.pitch_runtime.as_mut().unwrap();
            runtime.poll();
            runtime.has_ready_action(original.sequence())
        });
        assert_eq!(
            self.frame, frame,
            "worker waits cannot advance virtual output"
        );
        assert!(original.accepted());
    }

    fn render_scheduled(&mut self, frames: usize) -> Vec<f32> {
        assert_eq!(self.transport.output_frame(), self.frame);
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
        self.transport.advance_by_rendered_frames(frames);
        self.frame = self.transport.output_frame();
        self.other_consumed += frames as u64;
        output
    }

    fn complete_start(&mut self, original: &CapturedPadStart) {
        self.wait_start_ready(original);
        let execution = original.execution_frame().unwrap();
        for count in [31, 137, 512, 777].into_iter().cycle() {
            if self.frame == execution {
                break;
            }
            assert!(self.frame < execution);
            let output = self.render_scheduled(count.min((execution - self.frame) as usize));
            assert!(output.iter().all(|sample| *sample == 0.0));
            assert_eq!(original.effective_frame(), None);
        }
        // Claim the real due event without omitting its first output sample.
        assert!(self.render_scheduled(0).is_empty());
        assert_eq!(
            original.state(),
            "applied",
            "reason={:?}",
            original.reason()
        );
        assert_eq!(original.effective_frame(), Some(execution));
    }

    fn drain(&mut self) -> usize {
        drain_parameter_messages(&mut self.parameters, &mut self.mixer, &mut self.transport);
        drain_control_messages(
            &mut self.consumer,
            &mut self.scheduler,
            self.frame,
            &mut self.quantization,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut self.retirement,
        )
    }

    fn render(&mut self, frames: usize, timestamp: bool, paused: bool) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        if timestamp {
            self.mixer.render_rt_at_output_frame(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                self.frame,
                &mut self.retirement,
            );
        } else {
            self.mixer
                .render_rt(&mut output, &mut [0.0; NUM_SAMPLES], &mut self.retirement);
        }
        self.frame += frames as u64;
        self.transport.advance_by_rendered_frames(frames);
        if !paused {
            self.other_consumed += frames as u64;
        }
        output
    }
}

struct Captured {
    sample: SampleBuffer,
    defaults: IncomingLoadDefaults,
    adoption: Arc<std::sync::atomic::AtomicU8>,
    cancelled: Arc<AtomicBool>,
}

struct Fixture {
    _cwd: Cwd,
    engine: Py<AudioEngine>,
    callback: Callback,
    parameters: Arc<Mutex<rtrb::Producer<ControlParameterMessage>>>,
    path: PathBuf,
    encoded: Vec<u8>,
    complete: SampleBuffer,
    other: SampleBuffer,
    old: Option<SampleBuffer>,
    file_identities: Vec<(PathBuf, Option<project_assets::FileIdentity>)>,
    _warm: Option<cold_store::CommittedColdLease>,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn new(encoded: Vec<u8>, warm: bool, replacement: bool) -> Self {
        Python::initialize();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let directory = tempfile::tempdir_in(workspace.join("scratch")).unwrap();
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(material_paths::ordinary_windows_path(directory.path())).unwrap();
        let root = directory.path().join("samples");
        fs::create_dir(&root).unwrap();
        let path = directory.path().join("incoming.wav");
        fs::write(&path, &encoded).unwrap();
        let complete = independent_pcm(&encoded);
        let warm_material = warm.then(|| {
            // Seed an actual copy-first external import. Material migration only
            // accepts an already owned Original and cannot admit this external path.
            let mut transaction =
                cold_store::ColdTransaction::capture(&root, &path, true, &|| false).unwrap();
            let _gate = transaction.preparation_gate(RATE, 2, &|| false).unwrap();
            material_migration::prepare_disk_pcm(
                &mut transaction,
                &path,
                RATE,
                2,
                cold_jobs::PCM_LIMIT_BYTES,
                &|| false,
                &mut |_| {},
            )
            .unwrap();
            transaction.commit(&|| false).unwrap();
            transaction.into_lease()
        });
        let other_path = directory.path().join("other.wav");
        let other_words = (0..8192)
            .map(|frame| [frame as i16 - 4096, 2048 - frame as i16 / 2])
            .collect::<Vec<_>>();
        fs::write(&other_path, wav(&other_words)).unwrap();
        let (producer, consumer) = rtrb::RingBuffer::new(64);
        let producer = Arc::new(Mutex::new(producer));
        let (parameters, parameter_consumer) = rtrb::RingBuffer::new(128);
        let parameters = Arc::new(Mutex::new(parameters));
        let (feedback, feedback_consumer) = rtrb::RingBuffer::new(128);
        let mut engine = AudioEngine::new().unwrap();
        engine.test_output_format = Some((RATE, 2));
        engine.test_control_channels = Some((producer, Arc::new(Mutex::new(feedback_consumer))));
        engine.test_parameter_producer = Some(parameters.clone());
        let mut mixer = Box::new(RtMixer::new(2, RATE as f32));
        mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
        mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
        mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
        let engine = Python::attach(|py| Py::new(py, engine).unwrap());
        let mut fixture = Self {
            _cwd: Cwd(previous),
            engine,
            callback: Callback {
                mixer,
                consumer,
                parameters: parameter_consumer,
                feedback,
                scheduler: FixedCapacityScheduler::new(),
                transport: TransportTimeline::new(RATE),
                quantization: TriggerQuantization::Immediate,
                retirement: Retirement {
                    slots: usize::MAX,
                    owners: Vec::new(),
                },
                frame: 0,
                other_consumed: 0,
            },
            parameters,
            path,
            encoded,
            complete,
            other: independent_pcm(&wav(&other_words)),
            old: None,
            file_identities: Vec::new(),
            _warm: warm_material,
            _directory: directory,
        };
        let request = fixture.legacy_load(215, &other_path);
        fixture.accept_legacy(215, request);
        fixture.other = Python::attach(|py| {
            fixture.engine.borrow(py).sample_cache.lock().unwrap()[215]
                .clone()
                .unwrap()
        });
        fixture.call("set_speed", (RATIO,));
        fixture.play_legacy(215, 0.0);
        assert_eq!(
            fixture.transport_events(),
            vec![("started".to_owned(), 215)]
        );
        if replacement {
            let old_path = fixture.path.with_file_name("old.wav");
            fs::write(&old_path, wav(&vec![[4096, -2048]; 16384])).unwrap();
            let request = fixture.legacy_load(0, &old_path);
            fixture.accept_legacy(0, request);
            fixture.call("set_pad_bpm", (0, Some(240.0_f64)));
            fixture
                .parameters
                .lock()
                .unwrap()
                .push(ControlParameterMessage::SetPadGain {
                    id: 0,
                    gain_db: -12.0,
                })
                .unwrap();
            fixture.play_legacy(0, 1.0);
            assert_eq!(fixture.transport_events(), vec![("started".to_owned(), 0)]);
            fixture.old = Python::attach(|py| {
                fixture.engine.borrow(py).sample_cache.lock().unwrap()[0].clone()
            });
        }
        fixture
    }

    fn call<A>(&self, name: &str, args: A)
    where
        A: for<'py> pyo3::call::PyCallArgs<'py>,
    {
        Python::attach(|py| {
            self.engine.bind(py).call_method1(name, args).unwrap();
        });
    }

    fn play_legacy(&mut self, id: usize, volume: f32) -> CapturedPadStart {
        let capture = self.callback.frame;
        self.call("play_sample", (id, volume));
        let original = self.callback.queued_start();
        assert_eq!(original.original_frame(), None);
        assert_eq!(
            self.callback.drain(),
            2,
            "original target plus prepared publication"
        );
        assert_eq!(original.original_frame(), Some(capture));
        assert!(original.execution_frame().unwrap() >= capture);
        self.callback.complete_start(&original);
        original
    }

    fn legacy_load(&self, id: usize, path: &Path) -> u64 {
        Python::attach(|py| {
            self.engine
                .borrow(py)
                .load_sample_async(
                    id,
                    path.to_string_lossy().into_owned(),
                    Some(false),
                    false,
                    false,
                    None,
                    None,
                    false,
                    "import",
                )
                .unwrap()
        })
    }

    fn start(&self, key_lock: bool) -> u64 {
        Python::attach(|py| {
            self.engine
                .bind(py)
                .call_method1(
                    "load_incoming_sample_async",
                    (0, self.path.to_string_lossy().as_ref(), key_lock),
                )
                .unwrap()
                .extract()
                .unwrap()
        })
    }

    fn update(
        &self,
        request: u64,
        revision: u64,
        key_lock: bool,
        region: (usize, Option<usize>),
        auto: bool,
        period: Option<f64>,
    ) -> PyResult<bool> {
        Python::attach(|py| {
            self.engine.borrow(py).update_incoming_load_intent(
                0,
                request,
                revision,
                key_lock,
                Some(region.0 as f64 / f64::from(RATE)),
                region.1.map(|end| end as f64 / f64::from(RATE)),
                auto,
                8.0,
                true,
                period,
                Some(64.0 / f64::from(RATE)),
            )
        })
    }

    fn event(&self) -> Option<Py<PyAny>> {
        Python::attach(|py| {
            self.engine
                .bind(py)
                .call_method0("poll_loader_events")
                .unwrap()
                .extract()
                .unwrap()
        })
    }

    fn assert_no_success(&self) {
        while let Some(event) = self.event() {
            Python::attach(|py| {
                let kind = event
                    .bind(py)
                    .get_item("type")
                    .unwrap()
                    .extract::<String>()
                    .unwrap();
                assert!(
                    kind != "success" && kind != "error",
                    "premature terminal event: {event:?}"
                );
            });
        }
    }

    fn terminal(&self, request: u64, success: bool) -> Py<PyAny> {
        let mut terminal = None;
        wait_until(|| {
            while let Some(event) = self.event() {
                let done = Python::attach(|py| {
                    let event = event.bind(py);
                    let kind = event.get_item("type").unwrap().extract::<String>().unwrap();
                    if kind != "success" && kind != "error" {
                        return false;
                    }
                    assert_eq!(
                        event
                            .get_item("request_id")
                            .unwrap()
                            .extract::<u64>()
                            .unwrap(),
                        request
                    );
                    assert_eq!(kind == "success", success, "unexpected terminal: {event:?}");
                    true
                });
                if done {
                    terminal = Some(event);
                }
            }
            terminal.is_some()
                && Python::attach(|py| {
                    self.engine.borrow(py).cold_loading[0].load(Ordering::Acquire) == 0
                })
        });
        terminal.unwrap()
    }

    fn accept_legacy(&mut self, id: usize, request: u64) {
        wait_until(|| self.callback.consumer.peek().is_ok());
        assert_eq!(self.callback.drain(), 1);
        let mut done = false;
        wait_until(|| {
            while let Some(event) = self.event() {
                Python::attach(|py| {
                    let value = event.bind(py);
                    let kind = value.get_item("type").unwrap().extract::<String>().unwrap();
                    assert_ne!(kind, "error", "legacy fixture load: {value:?}");
                    if kind == "success" {
                        assert_eq!(
                            value
                                .get_item("request_id")
                                .unwrap()
                                .extract::<u64>()
                                .unwrap(),
                            request
                        );
                        done = true;
                    }
                });
            }
            done && Python::attach(|py| {
                self.engine.borrow(py).cold_loading[id].load(Ordering::Acquire) == 0
            })
        });
    }

    fn pending(&mut self, request: u64) -> Captured {
        wait_until(|| self.callback.consumer.peek().is_ok());
        self.assert_no_success();
        let ControlMessage::LoadColdSample {
            id,
            sample,
            source_generation,
            incoming_defaults,
            adoption,
            resident_cancelled,
            ..
        } = self.callback.consumer.peek().unwrap()
        else {
            panic!("expected actual incoming LoadColdSample");
        };
        assert_eq!((*id, *source_generation), (0, request));
        assert_eq!(adoption.load(Ordering::Acquire), 0);
        let captured = Captured {
            sample: sample.clone(),
            defaults: incoming_defaults.expect("incoming defaults"),
            adoption: adoption.clone(),
            cancelled: resident_cancelled
                .clone()
                .expect("own incoming revision fence"),
        };
        if let Some(old) = &self.old {
            assert!(Arc::ptr_eq(
                &voice(&self.callback.mixer, 0)
                    .sample
                    .as_ref()
                    .unwrap()
                    .samples,
                &old.samples
            ));
        } else {
            assert!(self.callback.mixer.bank_for_measurement()[0].is_none());
        }
        assert_eq!(
            Python::attach(|py| self
                .engine
                .borrow(py)
                .input_runtime_ownership
                .cold_status(0, request)),
            Some(0)
        );
        captured
    }

    fn accept(&mut self, request: u64, captured: &Captured) -> Py<PyAny> {
        assert!(!captured.cancelled.load(Ordering::Acquire));
        assert_eq!(self.callback.drain(), 1);
        assert_eq!(captured.adoption.load(Ordering::Acquire), 2);
        assert_eq!(self.transport_events(), vec![("stopped".to_owned(), 0)]);
        let event = self.terminal(request, true);
        assert_eq!(
            self.callback.mixer.loop_region_frames(0),
            (captured.defaults.start_frame, captured.defaults.end_frame)
        );
        assert_eq!(
            self.callback.mixer.key_lock_for_measurement(0),
            captured.defaults.key_lock
        );
        assert!(
            self.callback
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.is_playing_sample(0))
        );
        assert_eq!(
            Python::attach(|py| self
                .engine
                .borrow(py)
                .input_runtime_ownership
                .cold_status(0, request)),
            Some(2)
        );
        Python::attach(|py| {
            let receipt = event.bind(py).get_item("incoming_defaults").unwrap();
            assert_eq!(
                receipt
                    .get_item("revision")
                    .unwrap()
                    .extract::<u64>()
                    .unwrap(),
                captured.defaults.revision
            );
            assert_eq!(
                receipt
                    .get_item("key_lock")
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
                captured.defaults.key_lock
            );
            assert_eq!(
                receipt
                    .get_item("start_s")
                    .unwrap()
                    .extract::<f64>()
                    .unwrap(),
                captured.defaults.start_frame as f64 / f64::from(RATE)
            );
            assert_eq!(
                receipt
                    .get_item("end_s")
                    .unwrap()
                    .extract::<Option<f64>>()
                    .unwrap(),
                captured
                    .defaults
                    .end_frame
                    .map(|end| end as f64 / f64::from(RATE))
            );
            assert_eq!(
                receipt
                    .get_item("period_seconds")
                    .unwrap()
                    .extract::<Option<f64>>()
                    .unwrap(),
                captured.defaults.period_seconds
            );
            assert_eq!(
                receipt
                    .get_item("grid_origin_s")
                    .unwrap()
                    .extract::<f64>()
                    .unwrap(),
                captured.defaults.origin_frame / f64::from(RATE)
            );
            assert_eq!(
                receipt
                    .get_item("loop_auto")
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
                captured.defaults.loop_auto
            );
            assert_eq!(
                receipt
                    .get_item("loop_bars")
                    .unwrap()
                    .extract::<f64>()
                    .unwrap(),
                captured.defaults.loop_bars
            );
        });
        self.file_identities = Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let leases = engine.cold_leases.lock().unwrap();
            let lease = leases[0].as_ref().unwrap();
            let mut paths = vec![self.path.clone(), lease.original_path.clone()];
            paths.extend(
                ["decoder.f32le", "playback.f32le", "manifest.json"]
                    .map(|leaf| lease.cache_path.join(leaf)),
            );
            paths
                .into_iter()
                .map(|path| {
                    let identity = project_assets::capture_identity(&path).unwrap();
                    assert!(
                        identity.is_some(),
                        "actual source/artifact identity: {path:?}"
                    );
                    (path, identity)
                })
                .collect()
        });
        event
    }

    fn transport_events(&self) -> Vec<(String, usize)> {
        let mut events = Vec::new();
        Python::attach(|py| {
            loop {
                let message = self.engine.borrow_mut(py).receive_msg().unwrap();
                let Some(message) = message else { break };
                match message {
                    AudioMessage::SampleStarted { id } => events.push(("started".to_owned(), id)),
                    AudioMessage::SampleStopped { id } => events.push(("stopped".to_owned(), id)),
                    AudioMessage::ContentFeedback {
                        id, kind: 1 | 2, ..
                    } => {
                        let engine = self.engine.borrow(py);
                        assert_eq!(engine._resolve_pad_feedback(&message).unwrap(), Some(id));
                        let AudioMessage::ContentFeedback { kind, .. } = message else {
                            unreachable!()
                        };
                        if kind == 1 {
                            let bank = self.callback.mixer.bank_for_measurement()[id]
                                .as_ref()
                                .unwrap();
                            let current = voice(&self.callback.mixer, id);
                            assert!(current.sample.as_ref().unwrap().same_source(bank));
                            assert_eq!(
                                current.sample.as_ref().unwrap().resident_binding(),
                                bank.resident_binding()
                            );
                            assert_eq!(
                                current.source_admission.as_ref().unwrap().content_key(),
                                engine.input_runtime_ownership.callback_feedback_key(id)
                            );
                        }
                        events.push((if kind == 1 { "started" } else { "stopped" }.to_owned(), id));
                    }
                    _ => panic!("unexpected actual callback feedback: {message:?}"),
                }
            }
        });
        events
    }

    fn assert_disk(&self, warm: bool) {
        assert_eq!(fs::read(&self.path).unwrap(), self.encoded);
        for (path, identity) in &self.file_identities {
            assert_eq!(
                &project_assets::capture_identity(path).unwrap(),
                identity,
                "source/artifact FileID changed: {path:?}"
            );
        }
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let leases = engine.cold_leases.lock().unwrap();
            let lease = leases[0].as_ref().unwrap();
            assert_eq!(fs::read(&lease.original_path).unwrap(), self.encoded);
            assert_eq!(lease.integrity.warm, warm);
            let bytes = (self.complete.samples.len() * 4) as u64;
            assert!(lease.integrity.decoder_verify_bytes >= bytes);
            assert!(lease.integrity.playback_verify_bytes >= bytes);
            assert!(lease.integrity.manifest_verify_bytes > 0);
            for leaf in ["decoder.f32le", "playback.f32le"] {
                let held = fs::read(lease.cache_path.join(leaf)).unwrap();
                assert_eq!(held.len(), bytes as usize);
                for (actual, expected) in held.chunks_exact(4).zip(self.complete.samples.iter()) {
                    assert_eq!(actual, expected.to_le_bytes());
                }
            }
        });
    }

    fn assert_other(&self) {
        let other = voice(&self.callback.mixer, 215);
        assert!(Arc::ptr_eq(
            &other.sample.as_ref().unwrap().samples,
            &self.other.samples
        ));
        let phase = (self.callback.other_consumed as f64 * RATIO)
            .rem_euclid(self.other.frame_count() as f64);
        assert_eq!(
            other.source_playback.position().frame,
            phase.floor() as usize
        );
        assert_eq!(
            other.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        Python::attach(|py| {
            assert!(
                self.engine
                    .borrow(py)
                    .input_runtime_ownership
                    .source_current(215, &self.other, RATE)
            )
        });
    }

    fn render_predecessor(&mut self) {
        let old = self.old.as_ref().expect("actual predecessor");
        let before = voice(&self.callback.mixer, 0).source_playback.position();
        assert_eq!((before.frame, before.fraction), (0, 0.0));
        let independent = independent_pcm(&wav(&vec![[4096, -2048]; 16384]));
        let mut reference = Box::new(RtMixer::new(2, RATE as f32));
        reference.load_sample(0, independent);
        reference.set_speed(RATIO);
        reference.set_pad_gain(0, -12.0);
        assert!(reference.play_sample_at_output_frame(0, 1.0, self.callback.frame));
        let mut expected = vec![0.0; 63 * 2];
        reference.render(&mut expected, &mut [0.0; NUM_SAMPLES]);
        let actual = self.callback.render(63, false, false);
        assert_eq!(
            actual, expected,
            "old confirmed source remains productive before incoming ACK"
        );
        let current = voice(&self.callback.mixer, 0);
        assert!(Arc::ptr_eq(
            &current.sample.as_ref().unwrap().samples,
            &old.samples
        ));
        assert_eq!(
            (
                current.source_playback.position().frame,
                current.source_playback.position().fraction
            ),
            (47, 0.25)
        );
        assert!(
            current
                .source_playback
                .matches_exact(&voice(&reference, 0).source_playback)
        );
        self.assert_other();
    }
}

fn voice(mixer: &RtMixer, id: usize) -> &crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(id))
        .unwrap()
}

fn voice_mut(mixer: &mut RtMixer, id: usize) -> &mut crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter_mut()
        .find(|voice| voice.is_playing_sample(id))
        .unwrap()
}

struct WorkerHold(Vec<mpsc::Sender<()>>);
impl WorkerHold {
    fn new(fixture: &Fixture) -> Self {
        let (entered, waiting) = mpsc::channel();
        let mut releases = Vec::new();
        Python::attach(|py| {
            let engine = fixture.engine.borrow(py);
            for _ in 0..cold_jobs::WORKERS {
                let (release, hold) = mpsc::channel();
                releases.push(release);
                let entered = entered.clone();
                let reservation = engine.cold_jobs.reserve().unwrap();
                engine
                    .cold_jobs
                    .submit(reservation, move || {
                        entered.send(()).unwrap();
                        hold.recv().unwrap();
                    })
                    .unwrap();
            }
        });
        for _ in 0..cold_jobs::WORKERS {
            waiting.recv_timeout(Duration::from_secs(30)).unwrap();
        }
        Self(releases)
    }
    fn release(&mut self) {
        for sender in self.0.drain(..) {
            let _ = sender.send(());
        }
    }
}
impl Drop for WorkerHold {
    fn drop(&mut self) {
        self.release();
    }
}

struct RawNative {
    native: RubberBandLiveShifter,
    input: [VecDeque<f32>; 2],
    output: [VecDeque<f32>; 2],
    used: bool,
}
impl RawNative {
    fn prepared_start() -> Self {
        let mut raw = Self::new();
        let history = raw.native.source_history_frames(1.0 / RATIO).unwrap();
        assert_eq!(history, prepared_native_history::PREPARED_HISTORY_FRAMES);
        // A fresh physical source supplies no PCM before its selected origin.
        // Keep H and the exact Native/FIFO phase without borrowing its loop tail.
        let feed = std::array::from_fn(|_| vec![0.0; history]);
        let _ = raw.render(&feed);
        assert!(
            raw.used,
            "the first output owns its independently prepared silent birth context"
        );
        raw
    }

    fn new() -> Self {
        let mut native = RubberBandLiveShifter::new(RATE, 2).unwrap();
        native.prepare_exact_pitch(1.0 / RATIO).unwrap();
        let block = native.block_size();
        Self {
            native,
            input: std::array::from_fn(|_| VecDeque::new()),
            output: std::array::from_fn(|_| VecDeque::from(vec![0.0; block - 1])),
            used: false,
        }
    }
    fn render(&mut self, feed: &[Vec<f32>; 2]) -> Vec<f32> {
        let mut result = Vec::new();
        let mut offset = 0;
        while offset < feed[0].len() {
            let block = self.native.block_size();
            let count = feed[0].len() - offset;
            for (channel, source) in self.input.iter_mut().zip(feed) {
                channel.extend(&source[offset..offset + count]);
            }
            while self.input[0].len() >= block {
                let input = self
                    .input
                    .iter_mut()
                    .map(|channel| channel.drain(..block).collect())
                    .collect::<Vec<Vec<f32>>>();
                let mut output = vec![vec![0.0; block]; 2];
                self.native.shift(&input, &mut output).unwrap();
                self.used = true;
                for (fifo, values) in self.output.iter_mut().zip(output) {
                    fifo.extend(values);
                }
            }
            assert!(self.output.iter().all(|channel| channel.len() >= count));
            for _ in 0..count {
                for channel in 0..2 {
                    let wet = self.output[channel].pop_front().unwrap();
                    result.push(wet);
                }
            }
            offset += count;
        }
        result
    }
}

fn filter() -> PerPadDspChain {
    let mut filter = PerPadDspChain::new(0, RATE as f32, 2);
    for (slot, db) in [
        (DspParameterSlot::Slot0, -12.0_f32),
        (DspParameterSlot::Slot1, -3.0),
        (DspParameterSlot::Slot2, 4.0),
    ] {
        let target = if db <= 0.0 {
            0.5 * 10.0_f32.powf(db / 20.0)
        } else {
            0.5 + 0.5 * db / 6.0
        };
        assert!(filter.set_parameter(
            DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
            target
        ));
    }
    filter.reset();
    filter
}

fn prepare_reference_start(reference: &mut RtMixer, execution: u64) {
    let ownership = reference.input_ownership_for_effect();
    let acknowledgements = Arc::new(constant_timing::CurrentTimingAcknowledgements::default());
    reference.set_current_timing_acknowledgements(acknowledgements.clone());
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
    let pitch = PitchIntent::default();
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
}

struct Playback {
    reference: Box<RtMixer>,
    raw: Option<RawNative>,
    filter: PerPadDspChain,
    start: usize,
    end: usize,
    consumed: u64,
    seams: HashSet<u64>,
    original: CapturedPadStart,
}

impl Playback {
    fn new(fixture: &mut Fixture, defaults: IncomingLoadDefaults, bpm_lock: bool) -> Self {
        fixture.call("set_speed", (RATIO,));
        if bpm_lock {
            assert_eq!(defaults.period_seconds, Some(0.5));
            fixture.call("set_bpm_lock", (true,));
            fixture.call("set_master_bpm", (90.0_f64,));
        }
        fixture
            .parameters
            .lock()
            .unwrap()
            .push(ControlParameterMessage::SetPadEq {
                id: 0,
                low_db: -12.0,
                mid_db: -3.0,
                high_db: 4.0,
            })
            .unwrap();
        fixture.callback.drain();
        let original = fixture.play_legacy(0, 1.0);
        assert_eq!(fixture.transport_events(), vec![("started".to_owned(), 0)]);
        let start = defaults.start_frame;
        let end = defaults.end_frame.unwrap_or(fixture.complete.frame_count());
        let mut reference = Box::new(RtMixer::new(2, RATE as f32));
        let ownership = Arc::new(input_runtime_binding::InputRuntimeOwnership::tracked());
        ownership.publish_test_source(0, &fixture.complete, RATE, 1);
        reference.set_input_runtime_ownership(ownership);
        reference.load_sample(0, fixture.complete.clone());
        reference.set_loop_region_frames(0, start, defaults.end_frame);
        reference.set_pad_bpm(0, defaults.period_seconds.map(|period| 60.0 / period));
        reference.set_speed(RATIO);
        reference.set_pad_key_lock(0, defaults.key_lock);
        reference.set_pad_eq(0, -12.0, -3.0, 4.0);
        assert!(reference.play_sample_at_output_frame(0, 1.0, fixture.callback.frame));
        if defaults.key_lock {
            prepare_reference_start(&mut reference, fixture.callback.frame);
            let current = voice(&fixture.callback.mixer, 0);
            assert_eq!(
                current.stretch.adopted_request_id(),
                Some(original.sequence())
            );
            assert_ne!(current.stretch.native_state_address(), 0);
            let history = current.stretch.productive_history().unwrap();
            assert_eq!(
                history.fed_output_frames,
                prepared_native_history::PREPARED_HISTORY_FRAMES as u64
            );
            assert_eq!(history.next_position.frame, start);
            assert_eq!(history.next_position.fraction, 0.0);
            if start != 0 || end != fixture.complete.frame_count() {
                let taps = current
                    .stretch
                    .native_tap_observation()
                    .expect("the actual finite source must retain its prepared coverage proof");
                assert_eq!(taps.missing_reads, 0);
                // The admitted finite window remains held, but warming the fresh
                // owner reads only explicit silence before its physical origin.
                assert_eq!(taps.min_frame, None);
                assert_eq!(taps.max_frame, None);
            } else {
                assert!(current.stretch.native_tap_observation().is_none());
            }
        }
        assert_eq!(
            voice(&fixture.callback.mixer, 0)
                .source_playback
                .position()
                .frame,
            start
        );
        Self {
            reference,
            raw: defaults.key_lock.then(RawNative::prepared_start),
            filter: filter(),
            start,
            end,
            consumed: 0,
            seams: HashSet::new(),
            original,
        }
    }

    fn feed(&mut self, fixture: &Fixture, start: u64, frames: usize) -> [Vec<f32>; 2] {
        let length = self.end - self.start;
        std::array::from_fn(|channel| {
            (0..frames)
                .map(|offset| {
                    let distance = (start + offset as u64) as f64 * RATIO;
                    let phase = distance.rem_euclid(length as f64);
                    let left = self.start + phase.floor() as usize;
                    let right = if left + 1 == self.end {
                        self.start
                    } else {
                        left + 1
                    };
                    if right == self.start && phase.fract() > 0.0 {
                        self.seams.insert((distance / length as f64).floor() as u64);
                    }
                    let a = fixture.complete.samples[left * 2 + channel];
                    let b = fixture.complete.samples[right * 2 + channel];
                    a + (b - a) * phase.fract() as f32
                })
                .collect()
        })
    }

    fn render(&mut self, fixture: &mut Fixture, frames: usize, timestamp: bool) {
        let feed = self.feed(fixture, self.consumed, frames);
        let raw = self.raw.as_mut().map_or_else(
            || {
                (0..frames)
                    .flat_map(|frame| [feed[0][frame], feed[1][frame]])
                    .collect()
            },
            // Incoming ON is effective at its own source ACK, before voice birth.
            // This history has no pending mode change with a dry bootstrap.
            |native| native.render(&feed),
        );
        let mut expected = vec![0.0; frames * 2];
        if timestamp {
            self.reference.render_at_output_frame(
                fixture.callback.frame,
                &mut expected,
                &mut [0.0; NUM_SAMPLES],
            );
        } else {
            self.reference
                .render(&mut expected, &mut [0.0; NUM_SAMPLES]);
        }
        reset_tap_observation_for_test();
        let actual = fixture.callback.render(frames, timestamp, false);
        assert_eq!(tap_observation_for_test().missing_reads, 0);
        assert_eq!(
            actual, expected,
            "complete PCM callback oracle at {}",
            fixture.callback.frame
        );
        for (actual, raw) in actual.chunks_exact(2).zip(raw.chunks_exact(2)) {
            self.filter.begin_frame();
            for channel in 0..2 {
                assert_eq!(
                    actual[channel],
                    self.filter.process_sample(channel, raw[channel]),
                    "independent raw Native + continuous EQ"
                );
            }
        }
        self.consumed += frames as u64;
        let phase = (self.consumed as f64 * RATIO).rem_euclid((self.end - self.start) as f64);
        let current = voice(&fixture.callback.mixer, 0);
        assert_eq!(
            current.source_playback.position().frame,
            self.start + phase.floor() as usize
        );
        assert_eq!(
            current.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert!(
            current
                .source_playback
                .matches_exact(&voice(&self.reference, 0).source_playback)
        );
        if let Some(raw) = &self.raw {
            assert_eq!(
                current.stretch.adopted_request_id(),
                Some(self.original.sequence())
            );
            assert_eq!(
                current.stretch.pending_fifo_frames(),
                (raw.input[0].len(), raw.output[0].len())
            );
        }
        fixture.assert_other();
    }

    fn pause_resume(&mut self, fixture: &mut Fixture) {
        let current = voice(&fixture.callback.mixer, 0);
        let generation = current.generation;
        let native = current.stretch.native_state_address();
        let fifo = current.stretch.pending_fifo_frames();
        let history = current.stretch.productive_history();
        let position = current.source_playback.position();
        fixture.call("pause_sample", (0,));
        fixture.call("pause_sample", (215,));
        assert_eq!(fixture.callback.drain(), 2);
        self.reference
            .pause_sample_at_output_frame(0, fixture.callback.frame);
        assert!(
            fixture
                .callback
                .render(257, false, true)
                .iter()
                .all(|value| *value == 0.0)
        );
        let paused = voice(&fixture.callback.mixer, 0);
        assert_eq!(paused.source_playback.position(), position);
        assert_eq!(paused.stretch.native_state_address(), native);
        assert_eq!(paused.stretch.pending_fifo_frames(), fifo);
        if let Some(before) = history {
            let after = paused.stretch.productive_history().unwrap();
            assert_eq!(after.next_position, before.next_position);
            assert_eq!(after.fed_output_frames, before.fed_output_frames);
        }
        fixture.assert_other();
        fixture.call("resume_sample", (0,));
        fixture.call("resume_sample", (215,));
        assert_eq!(fixture.callback.drain(), 2);
        self.reference
            .resume_sample_at_output_frame(0, fixture.callback.frame);
        assert_eq!(voice(&fixture.callback.mixer, 0).generation, generation);
    }

    fn adopt_native(&mut self, fixture: &mut Fixture) {
        let old = voice(&fixture.callback.mixer, 0)
            .stretch
            .native_state_address();
        assert_ne!(old, 0);
        let generation = voice(&fixture.callback.mixer, 0).generation;
        for count in [1, 31, 137, 512, 777, 96, 257] {
            self.render(fixture, count, true);
            let current = voice(&fixture.callback.mixer, 0);
            assert_eq!(current.stretch.native_state_address(), old);
            assert_eq!(
                current.stretch.adopted_request_id(),
                Some(self.original.sequence())
            );
            assert_eq!(current.generation, generation);
            assert!(
                !voice_mut(&mut fixture.callback.mixer, 0)
                    .stretch
                    .source_preparation_ready(),
                "the first ordinary candidate remains productive without a second warm-up"
            );
        }
    }
}

fn defaults_history(warm: bool, replacement: bool, key_lock: bool) {
    eprintln!("incoming defaults: warm={warm}, replacement={replacement}, ON={key_lock}");
    let mut fixture = Fixture::new(pulse_track(), warm, replacement);
    let input_identity = project_assets::capture_identity(&fixture.path).unwrap();
    let request = fixture.start(key_lock);
    let captured = fixture.pending(request);
    if replacement {
        fixture.render_predecessor();
    }
    let start = activity_frame(&fixture.complete);
    assert!(start > 0);
    assert_eq!(captured.defaults.start_frame, start);
    assert_eq!(captured.defaults.origin_frame, start as f64);
    assert!(captured.defaults.loop_auto);
    assert_eq!(captured.defaults.loop_bars, 8.0);
    assert_eq!(captured.defaults.revision, 0);
    assert_eq!(captured.defaults.key_lock, key_lock);
    let end = captured
        .defaults
        .end_frame
        .expect("real incoming automatic BPM must supply Auto8bars");
    assert!(start < end && end <= fixture.complete.frame_count());
    assert_eq!(
        (
            captured.sample.resident_start(),
            captured.sample.resident_end()
        ),
        (start, end)
    );
    assert_eq!(
        captured.sample.samples.as_ref(),
        &fixture.complete.samples[start * 2..end * 2]
    );
    assert_eq!(
        captured.sample.residency.as_ref().unwrap().context,
        if key_lock {
            ResidentContext::KeyLockFiniteLoop
        } else {
            ResidentContext::FiniteLoop
        }
    );
    let event = fixture.accept(request, &captured);
    Python::attach(|py| {
        let analysis = event.bind(py).get_item("analysis").unwrap();
        let bpm = analysis.get_item("bpm").unwrap().extract::<f64>().unwrap();
        assert!(bpm.is_finite() && bpm > 0.0);
        let period = 60.0 / bpm;
        assert_eq!(captured.defaults.period_seconds, Some(period));
        let independent_end = ((start as f64 / f64::from(RATE) + 32.0 * period) * f64::from(RATE))
            .round_ties_even()
            .min(fixture.complete.frame_count() as f64) as usize;
        assert_eq!(end, independent_end);
    });
    fixture.assert_disk(warm);
    let mut playback = Playback::new(&mut fixture, captured.defaults, false);
    for count in [1, 31, 137, 512, 777, 96, 257] {
        playback.render(&mut fixture, count, false);
    }
    playback.pause_resume(&mut fixture);
    if key_lock {
        playback.adopt_native(&mut fixture);
    }
    assert_eq!(
        project_assets::capture_identity(&fixture.path).unwrap(),
        input_identity
    );
    fixture.assert_disk(warm);
}

fn latest_timing_history(key_lock: bool) {
    eprintln!("incoming latest explicit physical/timing + actual BPM lock: ON={key_lock}");
    let mut fixture = Fixture::new(pulse_track(), false, true);
    let mut hold = WorkerHold::new(&fixture);
    let request = fixture.start(!key_lock);
    assert!(
        fixture
            .update(request, 1, !key_lock, (32, Some(2048)), false, Some(0.4))
            .unwrap()
    );
    assert!(
        fixture
            .update(request, 2, key_lock, (96, Some(4192)), false, Some(0.5))
            .unwrap()
    );
    assert!(
        fixture
            .update(request, 2, key_lock, (96, Some(4192)), false, Some(0.5))
            .is_err()
    );
    assert!(
        fixture
            .update(request + 1, 3, key_lock, (96, Some(4192)), false, Some(0.5))
            .is_err()
    );
    assert!(
        fixture
            .update(
                request,
                3,
                !key_lock,
                (128, Some(8192)),
                false,
                Some(f64::MAX)
            )
            .is_err()
    );
    assert!(
        fixture
            .update(
                request,
                3,
                !key_lock,
                (128, Some(8192)),
                false,
                Some(f64::from_bits(1))
            )
            .is_err()
    );
    Python::attach(|py| {
        assert!(
            fixture
                .engine
                .bind(py)
                .call_method1(
                    "update_incoming_load_intent",
                    (
                        0,
                        request,
                        3_u64,
                        !key_lock,
                        Some(128.0 / f64::from(RATE)),
                        Some(8192.0 / f64::from(RATE)),
                        false,
                        8.0_f64,
                        true,
                        Some(0.5_f64),
                        Some(f64::MAX),
                    )
                )
                .is_err()
        );
    });
    assert!(fixture.callback.consumer.is_empty());
    fixture.assert_no_success();
    assert!(Arc::ptr_eq(
        &voice(&fixture.callback.mixer, 0)
            .sample
            .as_ref()
            .unwrap()
            .samples,
        &fixture.old.as_ref().unwrap().samples
    ));
    fixture.render_predecessor();
    hold.release();
    let captured = fixture.pending(request);
    assert_eq!(captured.defaults.revision, 2);
    assert_eq!(captured.defaults.key_lock, key_lock);
    assert_eq!(
        (captured.defaults.start_frame, captured.defaults.end_frame),
        (96, Some(4192))
    );
    assert_eq!(captured.defaults.period_seconds, Some(0.5));
    assert_eq!(captured.defaults.origin_frame, 64.0);
    assert!(!captured.defaults.loop_auto);
    assert_eq!(
        captured.sample.samples.as_ref(),
        &fixture.complete.samples[96 * 2..4192 * 2]
    );
    fixture.accept(request, &captured);
    let mut playback = Playback::new(&mut fixture, captured.defaults, true);
    for count in [1, 31, 137, 512, 777, 96, 257] {
        playback.render(&mut fixture, count, false);
    }
    playback.pause_resume(&mut fixture);
    if key_lock {
        playback.adopt_native(&mut fixture);
    }
    for _ in 0..24 {
        playback.render(&mut fixture, 777, false);
    }
    assert!(
        playback.seams.len() >= 3,
        "independently sampled physical wrap taps: {:?}",
        playback.seams
    );
    assert!(
        !fixture
            .update(request, 3, !key_lock, (128, Some(8192)), false, Some(0.25))
            .unwrap()
    );
    assert_eq!(
        fixture.callback.mixer.loop_region_frames(0),
        (96, Some(4192))
    );
    assert_eq!(fixture.callback.mixer.key_lock_for_measurement(0), key_lock);
    fixture.assert_disk(false);
}

fn no_bpm_history(key_lock: bool) {
    eprintln!("incoming silent unknown BPM: ON={key_lock}");
    let mut fixture = Fixture::new(wav(&vec![[0, 0]; 256]), false, false);
    let request = fixture.start(key_lock);
    // Actual 48k→44.1k analysis has 236 frames, below the QM 1024-frame
    // window and the key frontend's 8820-frame minimum. Its ordinary
    // unchanged defaults therefore have BPM0 and no invented physical end.
    let captured = fixture.pending(request);
    assert_eq!(captured.defaults.revision, 0);
    assert_eq!(captured.defaults.start_frame, 0);
    assert_eq!(captured.defaults.origin_frame, 0.0);
    assert_eq!(captured.defaults.key_lock, key_lock);
    assert_eq!(captured.defaults.period_seconds, None);
    assert_eq!(captured.defaults.end_frame, None);
    assert!(captured.defaults.loop_auto);
    assert_eq!(captured.defaults.loop_bars, 8.0);
    assert_eq!(
        (
            captured.sample.resident_start(),
            captured.sample.resident_end()
        ),
        (0, 256)
    );
    assert_eq!(
        captured.sample.residency.as_ref().unwrap().context,
        if key_lock {
            ResidentContext::KeyLockFullTrack
        } else {
            ResidentContext::FullTrack
        }
    );
    let event = fixture.accept(request, &captured);
    Python::attach(|py| {
        let analysis = event.bind(py).get_item("analysis").unwrap();
        assert_eq!(
            analysis.get_item("bpm").unwrap().extract::<f64>().unwrap(),
            0.0
        );
        assert_eq!(
            analysis
                .get_item("key")
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "unknown"
        );
        let grid = analysis.get_item("beat_grid").unwrap();
        for field in ["beats", "downbeats", "bars"] {
            assert!(
                grid.get_item(field)
                    .unwrap()
                    .extract::<Vec<f32>>()
                    .unwrap()
                    .is_empty(),
                "actual short-source grid {field}"
            );
        }
    });
    fixture.assert_disk(false);
    let mut playback = Playback::new(&mut fixture, captured.defaults, false);
    for count in [1, 31, 137, 512, 777] {
        playback.render(&mut fixture, count, false);
    }
}

fn short_source_history() {
    let mut words = vec![[0, 0]; 64];
    words[8..].fill([8192, -4096]);
    let mut fixture = Fixture::new(wav(&words), false, false);
    let mut hold = WorkerHold::new(&fixture);
    let request = fixture.start(true);
    assert!(
        fixture
            .update(request, 1, true, (7, None), true, Some(0.5))
            .unwrap()
    );
    hold.release();
    let captured = fixture.pending(request);
    assert_eq!(
        (captured.defaults.start_frame, captured.defaults.end_frame),
        (7, Some(64))
    );
    assert_eq!(
        captured.sample.samples.as_ref(),
        &fixture.complete.samples[14..]
    );
    fixture.accept(request, &captured);
    fixture.assert_disk(false);
}

fn stale_queued_history() {
    let mut fixture = Fixture::new(pulse_track(), false, true);
    let request = fixture.start(true);
    let captured = fixture.pending(request);
    let old_position = voice(&fixture.callback.mixer, 0).source_playback.position();
    assert!(
        fixture
            .update(request, 1, false, (96, Some(4192)), false, Some(0.5))
            .unwrap()
    );
    assert!(captured.cancelled.load(Ordering::Acquire));
    assert_eq!(fixture.callback.drain(), 1);
    assert_eq!(captured.adoption.load(Ordering::Acquire), 3);
    assert!(fixture.transport_events().is_empty());
    fixture.terminal(request, false);
    let old = fixture.old.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &voice(&fixture.callback.mixer, 0)
            .sample
            .as_ref()
            .unwrap()
            .samples,
        &old.samples
    ));
    assert_eq!(
        voice(&fixture.callback.mixer, 0).source_playback.position(),
        old_position
    );
    Python::attach(|py| {
        assert!(
            fixture
                .engine
                .borrow(py)
                .input_runtime_ownership
                .source_current(0, old, RATE)
        )
    });
    assert_eq!(fixture.callback.mixer.loop_region_frames(0), (0, None));
    assert!(!fixture.callback.mixer.key_lock_for_measurement(0));
    fixture.render_predecessor();
    assert_eq!(fs::read(&fixture.path).unwrap(), fixture.encoded);
    fixture.assert_other();
}

fn claimed_revision_history() {
    eprintln!("incoming actual callback claim fences a later revision");
    let mut fixture = Fixture::new(pulse_track(), false, true);
    let mut hold = WorkerHold::new(&fixture);
    let request = fixture.start(true);
    assert!(
        fixture
            .update(request, 1, true, (96, Some(4192)), false, Some(0.5))
            .unwrap()
    );
    hold.release();
    let captured = fixture.pending(request);
    fixture.render_predecessor();
    let engine = Python::attach(|py| fixture.engine.clone_ref(py));
    let adoption = captured.adoption.clone();
    let cancelled = captured.cancelled.clone();
    let observed = Arc::new(AtomicBool::new(false));
    let observed_in_callback = observed.clone();
    incoming_load::set_claim_observer(move || {
        assert_eq!(adoption.load(Ordering::Acquire), 1, "actual own CAS claim");
        assert!(!cancelled.load(Ordering::Acquire));
        Python::attach(|py| {
            let result = engine
                .bind(py)
                .call_method1(
                    "update_incoming_load_intent",
                    (
                        0,
                        request,
                        2_u64,
                        false,
                        Some(128.0 / f64::from(RATE)),
                        Some(8192.0 / f64::from(RATE)),
                        false,
                        8.0_f64,
                        true,
                        Some(0.25_f64),
                        Some(64.0 / f64::from(RATE)),
                    ),
                )
                .unwrap()
                .extract::<bool>()
                .unwrap();
            assert!(
                !result,
                "claimed incoming revision must not revoke its own source ACK"
            );
        });
        assert_eq!(adoption.load(Ordering::Acquire), 1);
        assert!(!cancelled.load(Ordering::Acquire));
        observed_in_callback.store(true, Ordering::Release);
    });
    fixture.accept(request, &captured);
    assert!(observed.load(Ordering::Acquire));
    assert_eq!(captured.defaults.revision, 1);
    assert_eq!(
        (captured.defaults.start_frame, captured.defaults.end_frame),
        (96, Some(4192))
    );
    assert_eq!(captured.defaults.period_seconds, Some(0.5));
    assert!(fixture.callback.mixer.key_lock_for_measurement(0));
    let mut playback = Playback::new(&mut fixture, captured.defaults, true);
    for count in [1, 31, 137, 512, 777] {
        playback.render(&mut fixture, count, false);
    }
    fixture.assert_disk(false);
}

fn retirement_pressure_history() {
    let mut fixture = Fixture::new(pulse_track(), false, true);
    let request = fixture.start(false);
    let captured = fixture.pending(request);
    let position = voice(&fixture.callback.mixer, 0).source_playback.position();
    fixture.callback.retirement.slots = 0;
    assert_eq!(fixture.callback.drain(), 0);
    assert_eq!(captured.adoption.load(Ordering::Acquire), 0);
    assert_eq!(
        voice(&fixture.callback.mixer, 0).source_playback.position(),
        position
    );
    assert!(Arc::ptr_eq(
        &voice(&fixture.callback.mixer, 0)
            .sample
            .as_ref()
            .unwrap()
            .samples,
        &fixture.old.as_ref().unwrap().samples
    ));
    fixture.assert_no_success();
    fixture.render_predecessor();
    fixture.callback.retirement.slots = usize::MAX;
    fixture.accept(request, &captured);
    fixture.assert_disk(false);
}

fn updating_revision_rejects_queued_history() {
    eprintln!("incoming real state4 update rejects an already queued candidate");
    let mut fixture = Fixture::new(pulse_track(), false, true);
    let request = fixture.start(true);
    let captured = fixture.pending(request);
    let old = fixture.old.as_ref().unwrap().clone();
    let prior = voice(&fixture.callback.mixer, 0);
    let position = prior.source_playback.position();
    let generation = prior.generation;
    let native = prior.stretch.native_state_address();
    let fifo = prior.stretch.pending_fifo_frames();
    let engine = Python::attach(|py| fixture.engine.clone_ref(py));
    let callback = std::rc::Rc::new(std::cell::RefCell::new(Some(fixture.callback)));
    let callback_in_update = callback.clone();
    let adoption = captured.adoption.clone();
    let cancelled = captured.cancelled.clone();
    let observed = Arc::new(AtomicBool::new(false));
    let observed_in_update = observed.clone();
    incoming_load::set_update_release_observer(move || {
        assert_eq!(
            adoption.load(Ordering::Acquire),
            incoming_load::INTENT_UPDATING
        );
        assert!(cancelled.load(Ordering::Acquire));
        let mut callback = callback_in_update.borrow_mut();
        let callback = callback.as_mut().unwrap();
        assert_eq!(
            callback.drain(),
            1,
            "actual queued callback within the own update gate"
        );
        assert_eq!(adoption.load(Ordering::Acquire), 3);
        let current = voice(&callback.mixer, 0);
        assert!(Arc::ptr_eq(
            &current.sample.as_ref().unwrap().samples,
            &old.samples
        ));
        assert_eq!(current.source_playback.position(), position);
        assert_eq!(current.generation, generation);
        assert_eq!(current.stretch.native_state_address(), native);
        assert_eq!(current.stretch.pending_fifo_frames(), fifo);
        observed_in_update.store(true, Ordering::Release);
    });
    let result = Python::attach(|py| {
        engine
            .bind(py)
            .call_method1(
                "update_incoming_load_intent",
                (
                    0,
                    request,
                    1_u64,
                    false,
                    Some(96.0 / f64::from(RATE)),
                    Some(4192.0 / f64::from(RATE)),
                    false,
                    8.0_f64,
                    true,
                    Some(0.5_f64),
                    Some(64.0 / f64::from(RATE)),
                ),
            )
            .map(|value| value.unbind())
    });
    fixture.callback = callback.borrow_mut().take().unwrap();
    assert!(
        result.is_err(),
        "own rejection must prevent updater release back to pending"
    );
    assert!(observed.load(Ordering::Acquire));
    assert_eq!(
        captured.adoption.load(Ordering::Acquire),
        3,
        "rejected candidate cannot return to state0"
    );
    assert!(fixture.transport_events().is_empty());
    fixture.terminal(request, false);
    let old = fixture.old.as_ref().unwrap();
    Python::attach(|py| {
        assert!(
            fixture
                .engine
                .borrow(py)
                .input_runtime_ownership
                .source_current(0, old, RATE)
        )
    });
    assert_eq!(fixture.callback.mixer.loop_region_frames(0), (0, None));
    assert!(!fixture.callback.mixer.key_lock_for_measurement(0));
    fixture.render_predecessor();
    assert_eq!(fs::read(&fixture.path).unwrap(), fixture.encoded);
    fixture.assert_other();
}

fn queue_pressure_history() {
    let fixture = Fixture::new(pulse_track(), false, true);
    Python::attach(|py| {
        let engine = fixture.engine.borrow(py);
        let mut producer = engine.control_producer().unwrap().lock().unwrap();
        while !producer.is_full() {
            producer.push(ControlMessage::Ping()).unwrap();
        }
    });
    let request = fixture.start(true);
    fixture.terminal(request, false);
    let old = fixture.old.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &voice(&fixture.callback.mixer, 0)
            .sample
            .as_ref()
            .unwrap()
            .samples,
        &old.samples
    ));
    Python::attach(|py| {
        assert!(
            fixture
                .engine
                .borrow(py)
                .input_runtime_ownership
                .source_current(0, old, RATE)
        )
    });
    assert_eq!(fs::read(&fixture.path).unwrap(), fixture.encoded);
    fixture.assert_other();
}

fn current_generation(fixture: &Fixture) -> u64 {
    Python::attach(|py| {
        let binding = fixture
            .engine
            .bind(py)
            .call_method1("current_input_runtime_pad_binding", (0,))
            .unwrap();
        assert!(
            binding
                .call_method0("is_current")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        binding
            .call_method0("metadata")
            .unwrap()
            .get_item("source_generation")
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn bound_call<A>(fixture: &Fixture, name: &str, args: A, generation: u64) -> PyResult<()>
where
    A: for<'py> pyo3::call::PyCallArgs<'py>,
{
    Python::attach(|py| {
        let kwargs = PyDict::new(py);
        kwargs.set_item("expected_source_generation", generation)?;
        fixture
            .engine
            .bind(py)
            .call_method(name, args, Some(&kwargs))?;
        Ok(())
    })
}

fn reject_predecessor_edits(fixture: &Fixture, generation: u64) {
    let before = Python::attach(|py| {
        let engine = fixture.engine.borrow(py);
        (
            engine.prepared_source_epochs[0].load(Ordering::Acquire),
            engine.input_runtime_ownership.authority_revision(0),
        )
    });
    for result in [
        bound_call(
            fixture,
            "set_pad_loop_region",
            (0, 0.0, Some(0.1)),
            generation,
        ),
        bound_call(fixture, "set_pad_bpm", (0, Some(300.0)), generation),
        bound_call(fixture, "set_pad_timing_metadata", (0, 0.0), generation),
        bound_call(fixture, "set_pad_timing_intent", (0, "manual"), generation),
        bound_call(fixture, "set_pad_key_lock", (0, true), generation),
        bound_call(fixture, "prepare_resident_control", (0,), generation),
        bound_call(fixture, "play_sample", (0, 1.0_f32), generation),
        bound_call(fixture, "play_sample_exclusive", (0, 1.0_f32), generation),
        bound_call(fixture, "seek_sample", (0, 0.0), generation),
    ] {
        let error = result.expect_err("predecessor UI edit must be rejected");
        assert!(
            error
                .to_string()
                .contains("current complete source changed"),
            "{error}"
        );
    }
    Python::attach(|py| {
        let engine = fixture.engine.borrow(py);
        assert_eq!(
            before,
            (
                engine.prepared_source_epochs[0].load(Ordering::Acquire),
                engine.input_runtime_ownership.authority_revision(0)
            )
        );
    });
}

fn predecessor_edit_admission_history() {
    eprintln!("incoming actual source generation rejects predecessor live edit admission");
    let mut fixture = Fixture::new(pulse_track(), false, true);
    let predecessor = current_generation(&fixture);
    let request = fixture.start(false);
    let captured = fixture.pending(request);
    reject_predecessor_edits(&fixture, predecessor);
    assert!(!captured.cancelled.load(Ordering::Acquire));
    assert_eq!(captured.adoption.load(Ordering::Acquire), 0);
    assert!(fixture.callback.parameters.is_empty());
    fixture.render_predecessor();
    fixture.accept(request, &captured);
    reject_predecessor_edits(&fixture, predecessor);
    assert!(fixture.callback.consumer.is_empty());
    assert!(fixture.callback.parameters.is_empty());
    let own = current_generation(&fixture);
    assert_ne!(own, predecessor);
    assert_eq!(own, request);
    bound_call(
        &fixture,
        "set_pad_loop_region",
        (
            0,
            captured.defaults.start_frame as f64 / f64::from(RATE),
            captured
                .defaults
                .end_frame
                .map(|end| end as f64 / f64::from(RATE)),
        ),
        own,
    )
    .unwrap();
    fixture.callback.drain();
    let mut playback = Playback::new(&mut fixture, captured.defaults, false);
    for frames in [31, 137, 512, 777] {
        playback.render(&mut fixture, frames, false);
    }
    fixture.assert_other();
    fixture.assert_disk(false);
}

fn predecessor_bpm_coalescing_history() {
    eprintln!("incoming actual parameter channel rejects stale BPM before coalescing");
    let mut fixture = Fixture::new(pulse_track(), false, true);
    let predecessor = current_generation(&fixture);
    bound_call(&fixture, "set_pad_bpm", (0, Some(300.0)), predecessor).unwrap();
    // Hold the actual admitted parameter while its independent control channel
    // adopts the replacement. The original message and source guard are intact.
    let delayed = fixture.callback.parameters.pop().unwrap();
    assert!(
        matches!(&delayed, ControlParameterMessage::SetLegacyPadBpm {
        expected_source_generation: Some(generation), ..
    } if *generation == predecessor)
    );
    let request = fixture.start(false);
    let captured = fixture.pending(request);
    fixture.accept(request, &captured);
    let own = current_generation(&fixture);
    bound_call(&fixture, "set_pad_bpm", (0, Some(180.0)), own).unwrap();
    fixture.parameters.lock().unwrap().push(delayed).unwrap();
    fixture.callback.drain();
    assert_eq!(
        fixture.callback.mixer.output_bpm_for_sample_id(0),
        Some(180.0 * RATIO)
    );
    assert_eq!(
        fixture.callback.mixer.loop_region_frames(0),
        (captured.defaults.start_frame, captured.defaults.end_frame)
    );
    assert_eq!(
        fixture.callback.mixer.key_lock_for_measurement(0),
        captured.defaults.key_lock
    );
    assert_eq!(captured.adoption.load(Ordering::Acquire), 2);
    assert!(!captured.cancelled.load(Ordering::Acquire));
    assert!(fixture.callback.consumer.is_empty());
    assert!(fixture.callback.parameters.is_empty());
    fixture.assert_other();
    fixture.assert_disk(false);
}

fn predecessor_scheduled_start_history(exclusive: bool) {
    eprintln!(
        "incoming actual source generation rejects due predecessor launch: exclusive={exclusive}"
    );
    let mut fixture = Fixture::new(pulse_track(), false, true);
    let predecessor = current_generation(&fixture);
    fixture.callback.render(1, false, false);
    assert!(fixture.callback.transport.set_master_bpm(120.0));
    fixture.callback.quantization = TriggerQuantization::Grid { step_64ths: 4 };
    bound_call(
        &fixture,
        if exclusive {
            "play_sample_exclusive"
        } else {
            "play_sample"
        },
        (0, 1.0_f32),
        predecessor,
    )
    .unwrap();
    let original = fixture.callback.queued_start();
    assert_eq!(fixture.callback.drain(), 2);
    fixture.callback.wait_start_ready(&original);
    assert_eq!(fixture.callback.scheduler.len(), 1);
    assert!(fixture.transport_events().is_empty());
    let due_frame = fixture.callback.scheduler.peek_next_target_frame().unwrap();
    assert_eq!(original.original_frame(), Some(due_frame));
    assert_eq!(original.execution_frame(), Some(due_frame));
    assert!(due_frame > fixture.callback.frame);
    let request = fixture.start(false);
    let captured = fixture.pending(request);
    fixture.accept(request, &captured);
    let other_before = voice(&fixture.callback.mixer, 215)
        .source_playback
        .position();
    let region_before = fixture.callback.mixer.loop_region_frames(0);
    let event = fixture
        .callback
        .scheduler
        .pop_due_through(fixture.callback.frame, due_frame)
        .unwrap();
    assert_eq!(event.target_frame, due_frame);
    execute_scheduled_command(
        &mut fixture.callback.mixer,
        &mut fixture.callback.transport,
        event.execution_frame,
        event.command,
        &mut fixture.callback.feedback,
        &mut fixture.callback.retirement,
    );
    assert_eq!(original.state(), "rejected");
    assert_eq!(original.effective_frame(), None);
    assert!(fixture.transport_events().is_empty());
    assert_eq!(
        voice(&fixture.callback.mixer, 215)
            .source_playback
            .position(),
        other_before
    );
    assert_eq!(fixture.callback.mixer.loop_region_frames(0), region_before);
    assert!(
        fixture
            .callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.active || voice.sample_id != 0)
    );
    fixture.callback.quantization = TriggerQuantization::Immediate;
    let mut playback = Playback::new(&mut fixture, captured.defaults, false);
    for frames in [31, 137, 512, 777] {
        playback.render(&mut fixture, frames, false);
    }
    fixture.assert_other();
    fixture.assert_disk(false);
}

#[path = "ordinary_loader_public_all_tests.rs"]
mod ordinary_loader_public_all_tests;
#[path = "ordinary_loader_public_schedule_tests.rs"]
mod ordinary_loader_public_schedule_tests;
