//! Real public GLOBAL capture -> target callback -> existing atomic group execution.

#[path = "captured_global_start_first_wet_tests.rs"]
mod first_wet_tests;

use super::*;
use crate::audio_engine::audio_stream::drain_control_messages;
use crate::audio_engine::input_mapping::launch_admission_tests::RATE;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START};
use crate::messages::{ControlMessage, TriggerQuantization};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(3);

fn public_capture(fixture: &Fixture) -> CapturedGlobalStart {
    Python::attach(|py| {
        let entries = [0, 1]
            .into_iter()
            .map(|id| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                (
                    Py::new(py, binding).unwrap(),
                    LOOP_START as f64 / f64::from(RATE),
                    Some(LOOP_END as f64 / f64::from(RATE)),
                    None,
                )
            })
            .collect();
        fixture
            .control
            .engine
            .capture_global_start(py, entries, None)
            .unwrap()
    })
}

fn drain_grid(fixture: &mut Fixture) {
    drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        fixture.transport.output_frame(),
        &mut TriggerQuantization::Grid { step_64ths: 16 },
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
}

fn restore_nonzero(fixture: &mut Fixture) {
    for (id, shift) in [7, -3].into_iter().enumerate() {
        fixture
            .mixer
            .set_pad_pitch_intent(id, PitchIntent::new(0, shift).unwrap())
            .unwrap();
    }
}

fn wait_group(fixture: &mut Fixture) {
    let deadline = Instant::now() + WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.ordinary_group_ready(2) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "captured GLOBAL worker readiness timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn spectral_amplitude(pcm: &[f32], hz: f64) -> f64 {
    let (sin, cos) = pcm
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(sin, cos), (frame, sample)| {
            let phase = frame as f64 * hz * std::f64::consts::TAU / f64::from(RATE);
            (
                sin + f64::from(*sample) * phase.sin(),
                cos + f64::from(*sample) * phase.cos(),
            )
        });
    2.0 * sin.hypot(cos) / pcm.len() as f64
}

#[test]
fn deferred_public_global_retains_one_original_grid_target_and_native_first_attacks() {
    let mut fixture = Fixture::new(true);
    restore_nonzero(&mut fixture);
    fixture.render_to(1000);
    let captured = public_capture(&fixture);
    assert_eq!(captured.original_frame(), None);
    assert_eq!(captured.state(), "captured");
    drain_grid(&mut fixture);
    assert_eq!(captured.original_frame(), Some(24_000));
    assert!(fixture.scheduler.is_empty());
    assert!(
        fixture
            .render_to(30_000)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let ticket = fixture
        .control
        .engine
        .publish_captured_global_start(&captured)
        .unwrap()
        .unwrap();
    assert_eq!(ticket.publication_status(), "pending");
    drain_grid(&mut fixture);
    assert_eq!(fixture.scheduler.len(), 1);
    assert_eq!(fixture.scheduler.peek_next_target_frame(), Some(38_192));
    assert_eq!(
        captured.original_frame(),
        Some(24_000),
        "later grid is 48000, never a new T"
    );
    wait_group(&mut fixture);
    assert!(
        fixture
            .render_to(38_192)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let mut pcm = fixture.render(1);
    pcm.extend(fixture.pcm(47_999));
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(captured.state(), "applied");
    assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
    for ((id, source), shift) in [(0, 160.0), (1, 320.0)].into_iter().zip([7, -3]) {
        let pitch = PitchIntent::new(0, shift).unwrap();
        assert_eq!(fixture.mixer.pad_pitch_intent(id), Some(pitch));
        let voice = fixture.voice(id);
        assert!(voice.stretch.owns_pitch_contract(pitch, true));
        assert_ne!(voice.stretch.native_state_address(), 0);
        let history = voice.stretch.productive_history().unwrap();
        assert!(history.fed_output_frames >= 4096);
        assert_eq!(
            history.binding.source_address,
            voice.sample.as_ref().unwrap().source_address()
        );
        assert_eq!(
            voice
                .stretch
                .native_tap_observation()
                .unwrap()
                .missing_reads,
            0
        );
        let expected = source * 2.0_f64.powf(f64::from(shift) / 12.0);
        assert!(spectral_amplitude(&pcm, expected) > 0.02);
    }
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    let initial_energy = pcm[..2048]
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / 2048.0;
    assert!(
        initial_energy > 0.001,
        "complete initial FIFO prefix must be audible"
    );
}

#[test]
fn publication_pressure_preserves_exact_work_and_original_group_until_one_immediate_effect() {
    let mut fixture = Fixture::new(false);
    let captured = public_capture(&fixture);
    let member = &captured.owner.members[0].start;
    let work = member.take_work(None).unwrap().unwrap();
    let address = (&*work) as *const _;
    let sequence = work.sequence();
    assert!(member.restore_unpublished_work(work).is_ok());
    {
        let mut producer = fixture.control.producer.lock().unwrap();
        while !producer.is_full() {
            producer.push(ControlMessage::Ping()).unwrap();
        }
    }
    assert!(
        fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .is_err()
    );
    assert_eq!(captured.state(), "captured");
    assert!(!member.is_published());
    let work = member.take_work(None).unwrap().unwrap();
    assert_eq!((&*work) as *const _, address);
    assert_eq!(work.sequence(), sequence);
    assert!(member.restore_unpublished_work(work).is_ok());
    fixture.drain();
    assert_eq!(captured.original_frame(), Some(0));
    let ticket = fixture
        .control
        .engine
        .publish_captured_global_start(&captured)
        .unwrap()
        .unwrap();
    fixture.drain();
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
    assert!(fixture.scheduler.is_empty());
    assert!(
        fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .is_err()
    );
}

#[test]
fn capture_and_publish_before_callback_drain_use_fifo_target_before_group_admission() {
    let mut fixture = Fixture::new(false);
    let captured = public_capture(&fixture);
    assert_eq!(captured.original_frame(), None);
    let ticket = fixture
        .control
        .engine
        .publish_captured_global_start(&captured)
        .unwrap()
        .unwrap();
    assert_eq!(
        captured.original_frame(),
        None,
        "producer publication never calculates T"
    );
    assert_eq!(ticket.publication_status(), "pending");
    fixture.drain();
    assert_eq!(captured.original_frame(), Some(0));
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
}

#[test]
fn changed_participant_before_publication_or_stop_after_preparation_never_starts_a_prefix() {
    for before_publication in [true, false] {
        let mut fixture = Fixture::new(true);
        restore_nonzero(&mut fixture);
        let captured = public_capture(&fixture);
        fixture.drain();
        if before_publication {
            fixture.control.publish(1, 0.2, 77);
            assert!(
                fixture
                    .control
                    .engine
                    .publish_captured_global_start(&captured)
                    .is_err()
            );
            assert!(
                captured
                    .owner
                    .members
                    .iter()
                    .all(|member| !member.start.is_published())
            );
            captured.cancel();
        } else {
            let ticket = fixture
                .control
                .engine
                .publish_captured_global_start(&captured)
                .unwrap()
                .unwrap();
            fixture.drain();
            wait_group(&mut fixture);
            fixture.control.engine.stop_sample(1).unwrap();
            fixture.drain();
            assert!(fixture.render_to(8193).iter().all(|sample| *sample == 0.0));
            assert_eq!(ticket.publication_status(), "rejected");
        }
        assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));
        assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    }
}

#[test]
fn original_global_members_follow_real_move_and_swap_without_new_capture() {
    for target in [1, 4] {
        let mut fixture = Fixture::new(false);
        let captured = public_capture(&fixture);
        fixture.drain();
        let pair = fixture.control.engine._capture_pad_pair(0, target).unwrap();
        let ticket = crate::audio_engine::pad_pair::control::prepare(
            &fixture.control.engine,
            &pair,
            RATE,
            1,
        )
        .unwrap();
        fixture.control.engine._publish_pad_pair(&ticket).unwrap();
        fixture.drain();
        assert!(ticket.own_acknowledgement().is_some());
        assert!(fixture.control.engine._reconcile_pad_pair(&ticket).unwrap());
        let ack = ticket.own_acknowledgement().unwrap();
        fixture
            .control
            .engine
            ._finish_pad_pair(&ticket, &ack)
            .unwrap();
        assert_eq!(
            captured.owner.members[0].start.diagnostic_slot(),
            Some(target)
        );
        assert_eq!(
            captured.owner.members[1].start.diagnostic_slot(),
            Some(if target == 1 { 0 } else { 1 })
        );
        let batch = fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
            .unwrap();
        fixture.drain();
        assert_eq!(batch.publication_status(), "accepted");
        assert_eq!(fixture.starts(target), 1);
        assert_eq!(fixture.starts(if target == 1 { 0 } else { 1 }), 1);
    }
}

#[cfg(windows)]
struct ColdGate(std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);

#[cfg(windows)]
impl ColdGate {
    fn occupy_workers(fixture: &Fixture) -> Self {
        let gate = Self(std::sync::Arc::new((
            std::sync::Mutex::new(false),
            std::sync::Condvar::new(),
        )));
        let (sender, receiver) = std::sync::mpsc::channel();
        for _ in 0..crate::audio_engine::cold_jobs::WORKERS {
            let gate = gate.0.clone();
            let sender = sender.clone();
            fixture
                .control
                .engine
                .cold_jobs
                .submit(
                    fixture.control.engine.cold_jobs.reserve().unwrap(),
                    move || {
                        sender.send(()).unwrap();
                        let mut released = gate.0.lock().unwrap();
                        while !*released {
                            released = gate.1.wait(released).unwrap();
                        }
                    },
                )
                .unwrap();
        }
        for _ in 0..crate::audio_engine::cold_jobs::WORKERS {
            receiver.recv_timeout(WAIT).unwrap();
        }
        gate
    }
}

#[cfg(windows)]
impl Drop for ColdGate {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

#[cfg(windows)]
#[test]
fn original_global_target_and_requested_loop_survive_exact_existing_resident_ack() {
    use crate::audio_engine::ordinary_start_tests::install_synthetic_complete_lease;
    let mut fixture = Fixture::new(true);
    let _files = install_synthetic_complete_lease(&mut fixture);
    restore_nonzero(&mut fixture);
    let gate = ColdGate::occupy_workers(&fixture);
    fixture.render_to(1000);
    let window = fixture
        .control
        .engine
        .prepare_resident_control(0, Some(1.5), Some(2.75), None, Some(true))
        .unwrap();
    let (window, captured) = Python::attach(|py| {
        let window = Py::new(py, window).unwrap();
        let entries = [0, 1]
            .into_iter()
            .map(|id| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                if id == 0 {
                    (
                        Py::new(py, binding).unwrap(),
                        1.5,
                        Some(2.75),
                        Some(window.clone_ref(py)),
                    )
                } else {
                    (
                        Py::new(py, binding).unwrap(),
                        LOOP_START as f64 / f64::from(RATE),
                        Some(LOOP_END as f64 / f64::from(RATE)),
                        None,
                    )
                }
            })
            .collect();
        let captured = fixture
            .control
            .engine
            .capture_global_start(py, entries, None)
            .unwrap();
        (window, captured)
    });
    drain_grid(&mut fixture);
    assert_eq!(captured.original_frame(), Some(24_000));
    assert!(
        fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
            .is_none()
    );
    assert!(
        fixture
            .render_to(30_000)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    drop(gate);
    let deadline = Instant::now() + WAIT;
    let ticket = loop {
        drain_grid(&mut fixture);
        Python::attach(|py| {
            let window = window.borrow(py);
            assert!(
                !matches!(
                    window.publication_status(),
                    "failed" | "cancelled" | "rejected"
                ),
                "original real WindowACK failed: {:?}",
                window.error()
            );
        });
        if let Some(ticket) = fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
        {
            break ticket;
        }
        assert!(
            Instant::now() < deadline,
            "genuine original WindowACK timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    drain_grid(&mut fixture);
    assert_eq!(captured.original_frame(), Some(24_000));
    assert_eq!(fixture.scheduler.peek_next_target_frame(), Some(38_192));
    wait_group(&mut fixture);
    assert!(
        fixture
            .render_to(38_192)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let pcm = fixture.render(2048);
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
    assert_eq!(
        fixture.voice(0).source_playback.position().frame,
        72_000 + 2048
    );
    assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
    assert_eq!(
        captured.owner.members[0].start.loop_region(),
        (1.5, Some(2.75))
    );
}

#[cfg(windows)]
#[test]
fn a_later_matching_real_ticket_cannot_replace_the_original_global_dependency() {
    use crate::audio_engine::ordinary_start_tests::install_synthetic_complete_lease;
    let mut fixture = Fixture::new(true);
    let _files = install_synthetic_complete_lease(&mut fixture);
    let gate = ColdGate::occupy_workers(&fixture);
    let original = fixture
        .control
        .engine
        .prepare_resident_control(0, Some(1.5), Some(2.75), None, Some(true))
        .unwrap();
    let captured = Python::attach(|py| {
        let original = Py::new(py, original).unwrap();
        let entries = [0, 1]
            .into_iter()
            .map(|id| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                if id == 0 {
                    (
                        Py::new(py, binding).unwrap(),
                        1.5,
                        Some(2.75),
                        Some(original.clone_ref(py)),
                    )
                } else {
                    (
                        Py::new(py, binding).unwrap(),
                        LOOP_START as f64 / f64::from(RATE),
                        Some(LOOP_END as f64 / f64::from(RATE)),
                        None,
                    )
                }
            })
            .collect();
        fixture
            .control
            .engine
            .capture_global_start(py, entries, None)
            .unwrap()
    });
    fixture.drain();
    let replacement = fixture
        .control
        .engine
        .prepare_resident_control(0, Some(1.5), Some(2.75), None, Some(true))
        .unwrap();
    drop(gate);
    let deadline = Instant::now() + WAIT;
    while replacement.publication_status() != "accepted" {
        fixture.drain();
        assert!(
            !matches!(
                replacement.publication_status(),
                "failed" | "cancelled" | "rejected"
            ),
            "replacement real WindowACK failed: {:?}",
            replacement.error()
        );
        assert!(
            Instant::now() < deadline,
            "replacement real WindowACK timed out: {:?}",
            replacement.error()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let error = fixture
        .control
        .engine
        .publish_captured_global_start(&captured)
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .contains("original global resident dependency")
    );
    assert!(
        captured
            .owner
            .members
            .iter()
            .all(|member| !member.start.is_published())
    );
    assert!(fixture.scheduler.is_empty());
    assert!(
        fixture
            .render_to(50_000)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));
}
