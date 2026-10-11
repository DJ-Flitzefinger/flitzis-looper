//! Controller -> PyO3 -> WindowWork -> callback ACK -> genuine wet/dry render.
//! Device startup is replaced only by the engine's existing test producer seam.

use super::*;
use crate::audio_engine::audio_stream::{drain_parameter_messages, render_scheduled_audio};
use crate::audio_engine::resident_relocation::ResidentWindowTicket;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::ffi::CString;

const RATE: u32 = 48_000;
const START: usize = 32;
const END: usize = 224;
const RATIO: f64 = 0.73;

struct Harness {
    directory: tempfile::TempDir,
    engine: Py<AudioEngine>,
    probe: Py<PyAny>,
    // The debug Windows test thread retains one callback owner on the heap;
    // its fixed mixer arrays must not be copied through Harness return frames.
    callback: Box<Callback>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    parameters: rtrb::Consumer<crate::messages::ControlParameterMessage>,
    mono: Vec<f32>,
    frame: u64,
    active_frames: u64,
}

impl Harness {
    fn new(initial: bool, ids: &[usize]) -> Self {
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let directory = tempfile::Builder::new()
            .prefix("live-keylock-controller-")
            .tempdir_in(repository.parent().unwrap().join("scratch"))
            .unwrap();
        let source = directory.path().join("control.wav");
        let mono = wav(&source, RATE);
        let mut engine = AudioEngine::new().unwrap();
        let mut callback = Box::new(Callback::new(&engine, RATE));
        let (producer, mut consumer) = rtrb::RingBuffer::new(64);
        let producer = Arc::new(Mutex::new(producer));
        let (feedback, feedback_rx) = rtrb::RingBuffer::new(64);
        callback.feedback = feedback;
        let (parameter_producer, parameters) = rtrb::RingBuffer::new(64);
        engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback_rx))));
        engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameter_producer)));
        let mut paths = Vec::new();
        for &id in ids {
            let request = admit_for_format_selected(
                &engine,
                id,
                source.to_string_lossy().into_owned(),
                (
                    false,
                    false,
                    true,
                    Some(ResidentLoadHint {
                        start_s: START as f64 / f64::from(RATE),
                        end_s: END as f64 / f64::from(RATE),
                        key_lock: false,
                    }),
                ),
                producer.clone(),
                (2, RATE, directory.path().join("samples")),
            )
            .unwrap();
            wait_until(Duration::from_secs(10), || {
                callback.drain(&mut consumer);
                engine.cold_loading[id].load(Ordering::Acquire) == 0
            });
            let event = terminal(&engine, request);
            let LoaderEvent::Success { cached_path, .. } = event else {
                panic!("actual source load failed: {event:?}");
            };
            paths.push((id, cached_path));
        }
        Python::initialize();
        let (engine, probe) = Python::attach(|py| {
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
            let module = PyModule::from_code(
                py,
                &CString::new(include_str!("keylock_controller_probe.py")).unwrap(),
                c"keylock_controller_probe.py",
                c"keylock_controller_probe",
            )
            .unwrap();
            let probe = module
                .getattr("Probe")
                .unwrap()
                .call1((
                    engine.clone_ref(py),
                    paths,
                    initial,
                    RATE,
                    START,
                    END,
                    mono.len(),
                    py.get_type::<AudioMessage>(),
                ))
                .unwrap()
                .unbind();
            (engine, probe)
        });
        let mut harness = Self {
            directory,
            engine,
            probe,
            callback,
            producer,
            consumer,
            parameters,
            mono,
            frame: 0,
            active_frames: 0,
        };
        harness.call0("initialize");
        harness.settle(ids, initial, false);
        harness
    }

    fn call0(&self, method: &str) {
        Python::attach(|py| self.probe.call_method0(py, method).unwrap());
    }

    fn local(&self, id: usize, enabled: bool) {
        Python::attach(|py| {
            self.probe
                .call_method1(py, "local_mode", (id, enabled))
                .unwrap()
        });
    }

    fn global(&self, enabled: bool) {
        Python::attach(|py| {
            self.probe
                .call_method1(py, "global_mode", (enabled,))
                .unwrap()
        });
    }

    fn target(&self, method: &str, id: usize) {
        Python::attach(|py| self.probe.call_method1(py, method, (id,)).unwrap());
    }

    fn ticket(&self, id: usize) -> Py<ResidentWindowTicket> {
        Python::attach(|py| {
            self.probe
                .call_method1(py, "ticket", (id,))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn settled(&self, id: usize, enabled: bool) -> bool {
        Python::attach(|py| {
            self.probe
                .call_method1(py, "settled", (id, enabled))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn mode_request(&self, ticket: &Py<ResidentWindowTicket>) -> u64 {
        Python::attach(|py| {
            ticket
                .bind(py)
                .getattr("key_lock_request_id")
                .unwrap()
                .extract::<Option<u64>>()
                .unwrap()
                .unwrap()
        })
    }

    fn failure_settled(&self, enabled: bool) -> bool {
        Python::attach(|py| {
            self.probe
                .call_method1(py, "failure_settled", (0, enabled))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn assert_failure(&self, request: u64, enabled: bool) {
        Python::attach(|py| {
            self.probe
                .call_method1(py, "assert_native_failure", (0, request, enabled))
                .unwrap();
        });
    }

    fn assert_dry(&self, output: &[f32], before: u64) {
        assert!(
            output
                .iter()
                .zip(self.dry_oracle(before, output.len() / 2))
                .all(|(actual, expected)| (actual - expected).abs() < 1e-6),
            "failed ON replaced the actual dry output"
        );
    }

    fn drain_native(&mut self) {
        let callback = self.callback.as_mut();
        drain_parameter_messages(
            &mut self.parameters,
            &mut callback.mixer,
            &mut callback.transport,
        );
        drain_control_messages(
            &mut self.consumer,
            &mut callback.scheduler,
            self.frame,
            &mut callback.quantization,
            &mut callback.transport,
            &mut callback.mixer,
            &mut callback.feedback,
            &mut callback.retirement,
        );
    }

    fn drain(&mut self) {
        self.drain_native();
        self.call0("poll");
    }

    fn render(&mut self, frames: usize, active: bool) -> Vec<f32> {
        self.render_observed(frames, active, true)
    }

    fn render_observed(&mut self, frames: usize, active: bool, observe: bool) -> Vec<f32> {
        self.drain_native();
        if observe {
            self.call0("poll");
        }
        let mut output = vec![0.0; frames * 2];
        let callback = self.callback.as_mut();
        render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            self.frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        self.frame += frames as u64;
        if active {
            self.active_frames += frames as u64;
        }
        if observe {
            self.call0("poll");
        }
        output
    }

    fn native_ready(&self, enabled: bool) -> bool {
        Python::attach(|py| {
            let feedback = self
                .engine
                .borrow(py)
                .pad_key_lock_status(py, 0)
                .unwrap()
                .unwrap();
            let feedback = feedback.bind(py);
            feedback
                .get_item("effective")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
                == enabled
                && feedback
                    .get_item("ready")
                    .unwrap()
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
        })
    }

    fn feedback_identity(&self) -> [u64; 4] {
        Python::attach(|py| {
            let feedback = self
                .engine
                .borrow(py)
                .pad_key_lock_status(py, 0)
                .unwrap()
                .unwrap();
            [
                "source_generation",
                "source_identity",
                "window_revision",
                "request_id",
            ]
            .map(|field| {
                feedback
                    .bind(py)
                    .get_item(field)
                    .unwrap()
                    .unwrap()
                    .extract::<u64>()
                    .unwrap()
            })
        })
    }

    fn settle(&mut self, ids: &[usize], enabled: bool, playing: bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ids.iter().all(|&id| self.settled(id, enabled)) {
            self.drain();
            // A device callback continues while stopped or paused. Its silent
            // render observes returned warm handles without advancing a voice.
            self.render(128, playing);
            assert!(Instant::now() < deadline, "controller mode did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn dry_oracle(&self, active_frame: u64, frames: usize) -> Vec<f32> {
        // Independent full original PCM, algebraic source progression and
        // interpolation: no resident reader, SourcePlayback or Native helper.
        (0..frames)
            .flat_map(|offset| {
                let distance = (active_frame + offset as u64) as f64 * RATIO;
                let whole = distance.floor() as usize;
                let fraction = (distance - whole as f64) as f32;
                let first = START + whole % (END - START);
                let second = START + (whole + 1) % (END - START);
                let value = self.mono[first] * (1.0 - fraction) + self.mono[second] * fraction;
                [value; 2]
            })
            .collect()
    }

    fn assert_phase(&self, generation: u64) {
        let voice = self
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        assert_eq!(
            voice.generation, generation,
            "mode operation retriggered voice"
        );
        let distance = self.active_frames as f64 * RATIO;
        let position = voice.source_playback.position();
        assert_eq!(
            position.frame,
            START + distance.floor() as usize % (END - START)
        );
        assert!(
            (position.fraction - distance.fract()).abs() < 1e-7,
            "source fraction changed"
        );
        assert_eq!(
            voice.explicit_seek_mode,
            super::super::source_reader::ExplicitSeekMode::Normal
        );
    }

    fn start(&mut self) -> u64 {
        Python::attach(|py| self.engine.borrow_mut(py).set_speed(RATIO).unwrap());
        self.drain();
        self.target("start", 0);
        wait_until(Duration::from_secs(10), || {
            self.drain();
            self.callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
        });
        self.callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap()
            .generation
    }
}

#[test]
fn actual_python_controller_native_failure_and_exhausted_reserve_keep_previous_dry_output() {
    for failure in 0..3 {
        let mut harness = Harness::new(false, &[0]);
        let generation = harness.start();
        let first = harness.render(777, true);
        harness.assert_dry(&first, 0);
        if failure != 1 {
            let voice = harness
                .callback
                .mixer
                .voices
                .iter_mut()
                .find(|voice| voice.is_playing_sample(0))
                .unwrap();
            if failure == 0 {
                voice.stretch.fail_preparation_worker();
            } else {
                voice.stretch.exhaust_warmed_reserve();
            }
        }
        harness.local(0, true);
        let ticket = harness.ticket(0);
        let request = harness.mode_request(&ticket);
        if failure == 1 {
            // The Window was really claimed, but no wet chunk reached output.
            wait_until(Duration::from_secs(10), || {
                harness.drain();
                Python::attach(|py| ticket.borrow(py).publication_status() == "accepted")
            });
            harness
                .callback
                .mixer
                .voices
                .iter()
                .find(|voice| voice.is_playing_sample(0))
                .unwrap()
                .stretch
                .fail_preparation_worker();
        }
        wait_until(Duration::from_secs(10), || {
            let before = harness.active_frames;
            let output = harness.render(257, true);
            harness.assert_dry(&output, before);
            harness.assert_phase(generation);
            harness.failure_settled(false)
        });
        harness.assert_failure(request, false);
        for frames in [31, 1024, 1, 777] {
            let before = harness.active_frames;
            let output = harness.render(frames, true);
            harness.assert_dry(&output, before);
            harness.assert_phase(generation);
        }
    }
}

#[test]
fn actual_python_controller_stopped_armed_on_start_requires_first_wet_and_rolls_back_failure() {
    for failure in [Some(false), Some(true), None] {
        let mut harness = Harness::new(true, &[0]);
        harness.target("assert_stopped_armed", 0);
        assert!(
            !harness
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
        );
        let armed_identity = harness.feedback_identity();
        let source_backing = harness.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
            .clone();
        let generation = harness.start();
        let started_identity = harness.feedback_identity();
        assert_eq!(&started_identity[..3], &armed_identity[..3]);
        assert!(
            started_identity[3] > armed_identity[3],
            "public start did not receive its own real source-bound Window ACK"
        );
        harness.assert_phase(generation);
        let first = harness.render(128, true);
        harness.assert_dry(&first, 0);
        harness.assert_phase(generation);
        Python::attach(|py| {
            harness
                .probe
                .call_method1(py, "assert_live_waiting", (0, started_identity[3]))
                .unwrap();
        });
        if let Some(exhaust) = failure {
            // The actual stopped arming succeeded. The newly started voice has
            // not rendered any wet frames and must retain its first-wet guard.
            let voice = harness
                .callback
                .mixer
                .voices
                .iter_mut()
                .find(|voice| voice.is_playing_sample(0))
                .unwrap();
            if exhaust {
                voice.stretch.exhaust_warmed_reserve();
            } else {
                voice.stretch.fail_preparation_worker();
            }
            wait_until(Duration::from_secs(10), || {
                let before = harness.active_frames;
                let output = harness.render(257, true);
                harness.assert_dry(&output, before);
                harness.assert_phase(generation);
                Python::attach(|py| {
                    harness
                        .probe
                        .call_method1(py, "failure_settled", (0, false, true))
                        .unwrap()
                        .extract::<bool>(py)
                        .unwrap()
                })
            });
            Python::attach(|py| {
                harness
                    .probe
                    .call_method1(
                        py,
                        "assert_native_failure",
                        (0, started_identity[3], false, true),
                    )
                    .unwrap();
            });
            assert!(!harness.callback.mixer.key_lock_for_measurement(0));
        } else {
            wait_until(Duration::from_secs(10), || {
                harness.render(128, true);
                harness.assert_phase(generation);
                harness.native_ready(true)
            });
            let before = harness.active_frames;
            let wet = harness.render(4096, true);
            let difference: f32 = wet
                .iter()
                .zip(harness.dry_oracle(before, 4096))
                .map(|(actual, dry)| (actual - dry).abs())
                .sum();
            assert!(
                difference > 10.0 && wet.iter().map(|value| value.abs()).sum::<f32>() > 1.0,
                "stopped arming was mistaken for actual live wet output"
            );
            harness.assert_phase(generation);
            Python::attach(|py| {
                harness
                    .probe
                    .call_method1(py, "assert_native_wet", (0, started_identity[3]))
                    .unwrap();
            });
        }
        assert_eq!(
            harness.feedback_identity(),
            started_identity,
            "first-wet settlement changed source/window/request ownership"
        );
        assert!(Arc::ptr_eq(
            &source_backing,
            &harness.callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap()
                .samples
        ));
    }
}

#[test]
fn actual_python_controller_late_worker_error_preserves_confirmed_native_on_mode() {
    for observe_ready in [true, false] {
        let mut harness = Harness::new(false, &[0]);
        let generation = harness.start();
        harness.render(777, true);
        harness.local(0, true);
        let ticket = harness.ticket(0);
        let request = harness.mode_request(&ticket);
        if observe_ready {
            harness.settle(&[0], true, true);
        } else {
            // Controller sees the accepted Window, then is deliberately not polled
            // until after actual wet output and its subsequent own-request error.
            wait_until(Duration::from_secs(10), || {
                harness.drain();
                Python::attach(|py| ticket.borrow(py).publication_status() == "accepted")
            });
            wait_until(Duration::from_secs(10), || {
                harness.render_observed(1024, true, false);
                harness.native_ready(true)
            });
        }
        let before = harness.active_frames;
        let wet = harness.render_observed(4096, true, observe_ready);
        let difference: f32 = wet
            .iter()
            .zip(harness.dry_oracle(before, 4096))
            .map(|(actual, dry)| (actual - dry).abs())
            .sum();
        assert!(difference > 10.0 && wet.iter().map(|value| value.abs()).sum::<f32>() > 1.0);
        harness
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap()
            .stretch
            .fail_preparation_worker();
        wait_until(Duration::from_secs(10), || {
            harness.render_observed(257, true, observe_ready);
            harness.assert_phase(generation);
            if !observe_ready {
                harness.call0("poll");
            }
            harness.failure_settled(true)
        });
        harness.assert_failure(request, true);
        assert!(harness.callback.mixer.key_lock_for_measurement(0));
    }
}

#[test]
fn actual_python_controller_paused_armed_on_failure_before_resume_keeps_dry_source() {
    for exhaust in [false, true] {
        let mut harness = Harness::new(false, &[0]);
        let generation = harness.start();
        harness.render(777, true);
        harness.target("pause", 0);
        harness.drain();
        assert!(
            harness
                .callback
                .mixer
                .voices
                .iter()
                .find(|voice| voice.is_playing_sample(0))
                .unwrap()
                .paused,
            "real public pause did not reach the callback"
        );
        let before = harness
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap()
            .source_playback;
        harness.local(0, true);
        let request = harness.mode_request(&harness.ticket(0));
        harness.settle(&[0], true, false);
        assert!(harness.render(257, false).iter().all(|value| *value == 0.0));
        let after = harness
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap()
            .source_playback;
        assert!(
            before.matches_exact(&after),
            "paused source changed before={before:?} after={after:?}"
        );
        let voice = harness
            .callback
            .mixer
            .voices
            .iter_mut()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        if exhaust {
            voice.stretch.exhaust_warmed_reserve();
        } else {
            voice.stretch.fail_preparation_worker();
        }
        harness.target("resume", 0);
        wait_until(Duration::from_secs(10), || {
            let before = harness.active_frames;
            let output = harness.render(257, true);
            harness.assert_dry(&output, before);
            harness.assert_phase(generation);
            Python::attach(|py| {
                harness
                    .probe
                    .call_method1(py, "failure_settled", (0, false, true))
                    .unwrap()
                    .extract::<bool>(py)
                    .unwrap()
            })
        });
        Python::attach(|py| {
            harness
                .probe
                .call_method1(py, "assert_native_failure", (0, request, false, true))
                .unwrap();
        });
    }
}

fn productive_both_directions(initial: bool) {
    let mut harness = Harness::new(initial, &[0]);
    let generation = harness.start();
    let first = harness.render(1536, true);
    if !initial {
        let expected = harness.dry_oracle(0, 1536);
        assert!(
            first
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
    }
    harness.assert_phase(generation);
    let before = harness
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
        .source_playback;
    let source_backing = harness.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap()
        .samples
        .clone();
    harness.local(0, !initial);
    let ticket = harness.ticket(0);
    Python::attach(|py| {
        harness
            .probe
            .call_method1(py, "assert_pending", (0, !initial))
            .unwrap();
        harness
            .probe
            .call_method1(py, "assert_identical_pending", (0, !initial))
            .unwrap();
    });
    wait_until(Duration::from_secs(10), || {
        harness.drain();
        Python::attach(|py| ticket.borrow(py).publication_status() == "accepted")
    });
    let after = &harness
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
        .source_playback;
    assert!(before.matches_exact(after), "mode ACK moved source clock");
    assert!(
        Arc::ptr_eq(
            &source_backing,
            &harness.callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap()
                .samples
        ),
        "same-extent mode change copied the existing source backing"
    );
    Python::attach(|py| {
        let mode_request: Option<u64> = ticket
            .bind(py)
            .getattr("key_lock_request_id")
            .unwrap()
            .extract()
            .unwrap();
        assert!(mode_request.is_some());
        let ticket = ticket.borrow(py);
        let observation = ticket.read_observation_for_test().unwrap();
        assert_eq!(
            (observation.source.start_frame, observation.source.end_frame),
            (START, END)
        );
        // The genuine worker verifies the held source and physical range, then
        // shares its registered PCM instead of doing fresh range I/O/allocation.
        assert_eq!(observation.source.read_bytes, 0);
        assert_eq!(observation.source.allocated_bytes, 0);
    });
    let start = harness.active_frames;
    let actual = harness.render(13, true);
    if !initial {
        let voice = harness
            .callback
            .mixer
            .voices
            .iter_mut()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        wait_until(Duration::from_secs(5), || {
            voice.stretch.source_preparation_ready()
        });
    }
    let mut wet_difference = actual
        .iter()
        .zip(harness.dry_oracle(start, 13))
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>();
    let mut wet_energy = actual.iter().map(|value| value.abs()).sum::<f32>();
    for frames in [
        31, 777, 1024, 1, 2247, 37, 1, 127, 384, 96, 257, 512, 31, 2048,
    ] {
        let start = harness.active_frames;
        let actual = harness.render(frames, true);
        let dry = harness.dry_oracle(start, frames);
        if initial {
            assert!(
                actual.iter().zip(&dry).all(|(a, b)| (a - b).abs() < 1e-6),
                "OFF retained wet FIFO"
            );
        }
        wet_difference += actual
            .iter()
            .zip(dry)
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>();
        wet_energy += actual.iter().map(|value| value.abs()).sum::<f32>();
        harness.assert_phase(generation);
    }
    if !initial {
        assert!(
            wet_difference > 10.0 && wet_energy > 1.0,
            "ON had no nontrivial wet processing"
        );
        let voice = harness
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        assert!(
            voice.stretch.adopted_request_id().is_some(),
            "later source candidate did not adopt"
        );
    }
    harness.settle(&[0], !initial, true);
    harness.target("pause", 0);
    harness.drain();
    assert!(
        harness
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap()
            .paused,
        "real public pause did not reach the callback"
    );
    let paused = harness
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
        .source_playback;
    harness.global(initial);
    harness.settle(&[0], initial, false);
    assert!(harness.render(791, false).iter().all(|value| *value == 0.0));
    assert!(
        paused.matches_exact(
            &harness
                .callback
                .mixer
                .voices
                .iter()
                .find(|voice| voice.is_playing_sample(0))
                .unwrap()
                .source_playback
        )
    );
    harness.target("resume", 0);
    harness.render(1536, true);
    harness.assert_phase(generation);
    harness.settle(&[0], initial, true);
    // All three gestures are real public requests while the voice keeps running.
    // The callback has not claimed the first request before it is superseded.
    harness.local(0, !initial);
    let rapid_old = harness.ticket(0);
    harness.local(0, initial);
    harness.local(0, !initial);
    harness.settle(&[0], !initial, true);
    harness.assert_phase(generation);
    Python::attach(|py| assert!(!rapid_old.borrow(py).is_current()));
}

#[test]
fn actual_python_controller_off_started_live_on_native_output_and_pause_global_off() {
    productive_both_directions(false);
}

#[test]
fn actual_python_controller_on_started_live_off_dry_output_and_pause_global_on() {
    productive_both_directions(true);
}

#[test]
fn actual_python_global_all_banks_local_override_supersession_and_one_target_failure() {
    let ids = [0, 36, 215];
    let mut harness = Harness::new(false, &ids);
    harness.global(true);
    let old = harness.ticket(0);
    harness.local(0, false);
    harness.local(0, true);
    harness.settle(&ids, true, false);
    assert!(Python::attach(|py| old.borrow(py).publication_status()
        != "accepted"
        || !old.borrow(py).is_current()));
    harness.local(36, false);
    harness.settle(&[36], false, false);
    harness.call0("assert_mixed");
    Python::attach(|py| {
        harness
            .probe
            .call_method1(
                py,
                "assert_modes",
                (vec![(0, true), (36, false), (215, true)],),
            )
            .unwrap()
    });
    harness.global(true);
    harness.settle(&ids, true, false);
    harness.global(false);
    harness.settle(&ids, false, false);
    harness.target("unavailable_timing", 36);
    harness.drain();
    harness.global(true);
    harness.settle(&[0, 215], true, false);
    // Eight failed real admissions settle as a bounded individual error; the
    // subsequent target has already succeeded through its own Window worker.
    for _ in 0..9 {
        harness.call0("poll");
    }
    harness.target("assert_failed_target", 36);
    Python::attach(|py| {
        harness
            .probe
            .call_method1(py, "assert_modes", (vec![(0, true), (215, true)],))
            .unwrap()
    });
}

#[test]
fn actual_python_pending_unload_reload_rejects_previous_source_feedback() {
    let mut harness = Harness::new(false, &[0]);
    let generation = Python::attach(|py| {
        harness
            .engine
            .borrow(py)
            .loaded_source_generations
            .lock()
            .unwrap()[0]
            .0
    });
    harness.local(0, true);
    let old = harness.ticket(0);
    harness.target("unload", 0);
    harness.drain();
    Python::attach(|py| {
        assert!(
            harness
                .engine
                .borrow(py)
                .pad_key_lock_status(py, 0)
                .unwrap()
                .is_none()
        );
        assert!(!old.borrow(py).is_current());
    });
    let source = harness.directory.path().join("control.wav");
    let request = Python::attach(|py| {
        admit_for_format_selected(
            &harness.engine.borrow(py),
            0,
            source.to_string_lossy().into_owned(),
            (
                false,
                false,
                true,
                Some(ResidentLoadHint {
                    start_s: START as f64 / f64::from(RATE),
                    end_s: END as f64 / f64::from(RATE),
                    key_lock: false,
                }),
            ),
            harness.producer.clone(),
            (2, RATE, harness.directory.path().join("samples")),
        )
        .unwrap()
    });
    wait_until(Duration::from_secs(10), || {
        harness.drain();
        Python::attach(|py| harness.engine.borrow(py).cold_loading[0].load(Ordering::Acquire) == 0)
    });
    let cached_path = Python::attach(|py| {
        let event = terminal(&harness.engine.borrow(py), request);
        let LoaderEvent::Success { cached_path, .. } = event else {
            panic!("{event:?}");
        };
        cached_path
    });
    Python::attach(|py| {
        harness
            .probe
            .call_method1(py, "assign", (0, cached_path, false))
            .unwrap();
        harness
            .probe
            .call_method1(py, "assert_new_source_dry", (0, generation))
            .unwrap();
        assert!(!old.borrow(py).is_current());
    });
}

impl Harness {
    fn select_four_stems(&mut self, mask: u8) -> (u64, Arc<[u8; 32]>) {
        use super::super::stem_cache::{STEM_FILE_NAMES, source_version_hash};
        use crate::messages::StemMixMode;
        let root = self.directory.path().join("samples");
        let (material_id, version) = Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let leases = engine.cold_leases.lock().unwrap();
            let lease = leases[0].as_ref().unwrap();
            let reference = lease
                .original_path
                .strip_prefix(self.directory.path())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (
                lease.material_id.clone().unwrap(),
                format!("{reference}|sha256-v1:{}", full_hash(&lease.original_path)),
            )
        });
        let reference = format!(
            "samples/materials/M{material_id}/stems/.ready-00112233445566778899aabbccddeeff"
        );
        let path = self.directory.path().join(&reference);
        fs::create_dir_all(&path).unwrap();
        let source_words: Vec<i16> = self
            .mono
            .iter()
            .map(|sample| (*sample * 32768.0) as i16)
            .collect();
        let components: [Vec<i16>; 4] = std::array::from_fn(|component| {
            source_words
                .iter()
                .map(|word| (*word / 16) * (component as i16 + 1))
                .collect()
        });
        for (index, name) in STEM_FILE_NAMES.iter().enumerate() {
            let words: Vec<i16> = if index < 4 {
                components[index].clone()
            } else {
                (0..source_words.len())
                    .map(|frame| components[1][frame] + components[2][frame] + components[3][frame])
                    .collect()
            };
            let stereo: Vec<i16> = words.into_iter().flat_map(|word| [word; 2]).collect();
            write_pcm16(&path.join(format!("{name}.wav")), RATE, 2, &stereo);
        }
        let hashes: serde_json::Map<String, serde_json::Value> = STEM_FILE_NAMES
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    serde_json::json!(full_hash(&path.join(format!("{name}.wav")))),
                )
            })
            .collect();
        fs::write(
            path.join(".complete.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema": "stem-set-sha256-v1", "source_version": version, "stems": hashes,
            }))
            .unwrap(),
        )
        .unwrap();
        // The complete committed pair, shared alignment and finite component
        // view use the ordinary preparation/publication kernels. Device startup
        // is the sole substituted seam; KEYLOCK remains actual Python/PyO3.
        Python::attach(|py| {
            let engine_handle = self.engine.clone_ref(py);
            let engine = engine_handle.borrow(py);
            let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
            let pair = engine
                .prepare_stem_pair_at_root(&root, 0, &version, &reference, &ticket, true, None)
                .unwrap();
            assert!(pair.has_components());
            pair.select();
            engine
                .publish_stem_pair_with_producer(&pair, &ticket, &self.producer)
                .unwrap();
            self.drain_native();
            assert_eq!(ticket.publication_status(), "accepted");
            super::super::resident_relocation::reconcile(&engine).unwrap();
        });
        let hash = source_version_hash(&version);
        {
            let mut producer = self.producer.lock().unwrap();
            producer
                .push(ControlMessage::SetStemMixMode {
                    id: 0,
                    mode: StemMixMode::AllStems,
                    source_version_hash: hash,
                })
                .unwrap();
            producer
                .push(ControlMessage::SetStemEnabledMask {
                    id: 0,
                    enabled_stem_mask: mask,
                    source_version_hash: hash,
                })
                .unwrap();
        }
        self.drain();
        let identity = self.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity
            .clone();
        // Independent serialized PCM16 component sum. Do not derive expected
        // samples from the committed native buffers or resident source reader.
        self.mono = (0..source_words.len())
            .map(|frame| {
                (0..4)
                    .filter(|component| mask & (1 << component) != 0)
                    .map(|component| f32::from(components[component][frame]) / 32767.0)
                    .sum()
            })
            .collect();
        (hash, identity)
    }

    fn publish_legacy(&mut self, sample: SampleBuffer) -> u64 {
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let mut requests = engine.pad_request_ids.lock().unwrap();
            requests[0] += 1;
            let generation = requests[0];
            let mut generations = engine.loaded_source_generations.lock().unwrap();
            let mut digests = engine.loaded_source_digests.lock().unwrap();
            super::super::publish_loaded_sample(
                &self.producer,
                &engine.sample_cache,
                0,
                sample,
                super::super::LoadedSourcePublication {
                    ownership: &engine.input_runtime_ownership,
                    generation,
                    rate: RATE,
                    generation_slot: &mut generations[0],
                    digest_slot: &mut digests[0],
                    digest: full_hash(&self.directory.path().join("control.wav")),
                    cold: false,
                    cold_epoch: None,
                    cold_adoption: None,
                    replace_assignment: false,
                    loop_region: None,
                    resident_cancelled: None,
                    intent: None,
                },
            )
            .unwrap();
            generation
        })
    }
}

#[test]
fn actual_python_controller_selected_four_stem_window_native_output_and_dry_continuation() {
    use crate::messages::StemMixMode;
    let mut harness = Harness::new(false, &[0]);
    let mask = 0b1010;
    let (hash, identity) = harness.select_four_stems(mask);
    let generation = harness.start();
    // Let the actual component selection ramp settle before this independent
    // selected-source oracle is compared with subsequent mode-only output.
    harness.render(1536, true);
    let before = harness.active_frames;
    let output = harness.render(777, true);
    harness.assert_dry(&output, before);
    let source_backing = harness.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap()
        .samples
        .clone();
    let components: [Arc<[f32]>; 4] = std::array::from_fn(|index| {
        let set = harness.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert_eq!(set.stems[index].samples.len(), (END - START) * 2);
        set.stems[index].samples.clone()
    });
    harness.local(0, true);
    let ticket = harness.ticket(0);
    wait_until(Duration::from_secs(10), || {
        harness.drain();
        Python::attach(|py| ticket.borrow(py).publication_status() == "accepted")
    });
    Python::attach(|py| {
        let ticket = ticket.borrow(py);
        let observation = ticket.read_observation_for_test().unwrap();
        assert_eq!(
            (observation.source.start_frame, observation.source.end_frame),
            (START, END)
        );
        // The selected four-component view already owns this exact physical
        // extent. Mode-only work verifies/relabels those registered backings.
        assert_eq!(observation.source.read_bytes, 0);
        assert_eq!(observation.source.allocated_bytes, 0);
        assert_eq!(observation.stems.read_bytes, [0; 5]);
        assert_eq!(observation.stems.allocated_bytes, 0);
    });
    assert!(Arc::ptr_eq(
        &source_backing,
        &harness.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    for (index, backing) in components.iter().enumerate() {
        assert!(Arc::ptr_eq(
            backing,
            &harness.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap()
                .stems[index]
                .samples
        ));
    }
    harness.assert_phase(generation);
    let mut difference = 0.0_f32;
    let mut energy = 0.0_f32;
    for frames in [13, 777, 31, 2048, 1, 1024, 37, 2247, 512, 1536] {
        let before = harness.active_frames;
        let output = harness.render(frames, true);
        difference += output
            .iter()
            .zip(harness.dry_oracle(before, frames))
            .map(|(actual, dry)| (actual - dry).abs())
            .sum::<f32>();
        energy += output.iter().map(|value| value.abs()).sum::<f32>();
        harness.assert_phase(generation);
        assert_eq!(
            harness.callback.mixer.stem_demand_for_measurement(0),
            (StemMixMode::AllStems, mask, hash)
        );
        assert!(Arc::ptr_eq(
            &identity,
            &harness.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap()
                .complete_set_identity
        ));
    }
    assert!(
        difference > 10.0 && energy > 1.0,
        "selected components produced no genuine wet output"
    );
    harness.settle(&[0], true, true);
    harness.local(0, false);
    harness.settle(&[0], false, true);
    for frames in [1, 31, 777, 1024, 37] {
        let before = harness.active_frames;
        let output = harness.render(frames, true);
        harness.assert_dry(&output, before);
        harness.assert_phase(generation);
        assert_eq!(
            harness.callback.mixer.stem_demand_for_measurement(0),
            (StemMixMode::AllStems, mask, hash)
        );
    }
}

#[test]
fn actual_python_legacy_scalar_requires_own_callback_ack_and_wet_output() {
    let mut harness = Harness::new(false, &[0]);
    harness.target("unload", 0);
    harness.drain();
    // Legacy-load setup uses the genuine decoder and production queue/cache/
    // generation/source-fence publisher. It is not a cold-cache loader proof.
    let source = harness.directory.path().join("control.wav");
    let sample =
        super::super::sample_loader::decode_audio_file_to_sample_buffer(&source, 2, RATE, |_| {})
            .unwrap();
    assert!(sample.residency.is_none());
    harness.publish_legacy(sample.clone());
    harness.drain();
    Python::attach(|py| {
        harness
            .probe
            .call_method1(py, "assign", (0, source.to_string_lossy().as_ref(), false))
            .unwrap()
    });
    Python::attach(|py| {
        let descriptor = harness.engine.borrow(py).loaded_residency(py, 0).unwrap();
        assert!(
            !descriptor
                .bind(py)
                .get_item("cache_backed")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        harness.engine.borrow_mut(py).set_speed(RATIO).unwrap();
    });
    {
        let mut producer = harness.producer.lock().unwrap();
        producer
            .push(ControlMessage::SetPadLoopRegion {
                id: 0,
                start_s: START as f64 / f64::from(RATE),
                end_s: Some(END as f64 / f64::from(RATE)),
            })
            .unwrap();
        producer
            .push(ControlMessage::PlaySample {
                id: 0,
                volume: 1.0,
                received_at_ns: None,
            })
            .unwrap();
    }
    harness.drain();
    let generation = harness
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
        .generation;
    let first = harness.render(777, true);
    assert!(
        first
            .iter()
            .zip(harness.dry_oracle(0, 777))
            .all(|(a, b)| (a - b).abs() < 1e-6)
    );
    harness.local(0, true);
    harness.target("assert_scalar_enqueue_is_pending", 0);
    let request: u64 = Python::attach(|py| {
        harness
            .probe
            .call_method1(py, "scalar_request", (0,))
            .unwrap()
            .extract(py)
            .unwrap()
    });
    harness.drain();
    let before = harness.active_frames;
    let wet = harness.render(8192, true);
    let energy: f32 = wet.iter().map(|value| value.abs()).sum();
    let difference: f32 = wet
        .iter()
        .zip(harness.dry_oracle(before, 8192))
        .map(|(a, b)| (a - b).abs())
        .sum();
    assert!(
        energy > 1.0 && difference > 10.0,
        "scalar ON produced no genuine wet processing"
    );
    harness.settle(&[0], true, true);
    harness.assert_phase(generation);
    Python::attach(|py| {
        let feedback = harness
            .engine
            .borrow(py)
            .pad_key_lock_status(py, 0)
            .unwrap()
            .unwrap();
        assert_eq!(
            feedback
                .bind(py)
                .get_item("request_id")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            request
        );
    });
    harness.local(0, false);
    harness.drain();
    let before = harness.active_frames;
    let dry = harness.render(1024, true);
    assert!(
        dry.iter()
            .zip(harness.dry_oracle(before, 1024))
            .all(|(a, b)| (a - b).abs() < 1e-6)
    );
    harness.settle(&[0], false, true);
    harness.assert_phase(generation);
    harness.local(0, true);
    let old_request: u64 = Python::attach(|py| {
        harness
            .probe
            .call_method1(py, "scalar_request", (0,))
            .unwrap()
            .extract(py)
            .unwrap()
    });
    // Same immutable PCM address, distinct actual load generation. The queued
    // predecessor must not authorize this newly published source.
    let next_generation = harness.publish_legacy(sample);
    harness.drain();
    Python::attach(|py| {
        let feedback = harness
            .engine
            .borrow(py)
            .pad_key_lock_status(py, 0)
            .unwrap()
            .unwrap();
        assert_eq!(
            feedback
                .bind(py)
                .get_item("source_generation")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            next_generation
        );
        assert_ne!(
            feedback
                .bind(py)
                .get_item("request_id")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            old_request
        );
        assert!(
            !feedback
                .bind(py)
                .get_item("effective")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
    });
}
