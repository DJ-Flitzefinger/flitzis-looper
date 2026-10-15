//! Device-free original GUI capture through the actual callback, worker and scheduler.

use super::audio_stream::drain_control_messages;
#[cfg(windows)]
use super::audio_stream::process_control_message;
use super::input_mapping::launch_admission_tests::RATE;
use super::pad_start::CapturedPadStart;
use super::pitch_intent::PitchIntent;
use super::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START, assert_tone};
#[cfg(windows)]
use super::pitch_runtime_tests::{SOURCE_HZ, publish_tone};
#[cfg(windows)]
use crate::messages::ControlMessage;
use crate::messages::{AudioMessage, TriggerQuantization};
use std::time::{Duration, Instant};

const PCM_FRAMES: usize = 48_000;
const WORKER_WAIT: Duration = Duration::from_secs(2);

fn capture(fixture: &Fixture, volume: f32) -> CapturedPadStart {
    fixture
        .control
        .engine
        .capture_pad_start(
            0,
            LOOP_START as f64 / f64::from(RATE),
            Some(LOOP_END as f64 / f64::from(RATE)),
            volume,
            false,
            None,
            None,
        )
        .unwrap()
}

fn drain_with_mode(fixture: &mut Fixture, mut mode: TriggerQuantization) {
    drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        fixture.transport.output_frame(),
        &mut mode,
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
}

/// Bound the host wait while virtual playback remains fixed. This proves readiness,
/// not a CPAL/device deadline or performer listening acceptance.
fn wait_ready(fixture: &mut Fixture, sequences: &[u64]) {
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if sequences
            .iter()
            .all(|sequence| runtime.has_ready_action(*sequence))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "ordinary preparation exceeded its host wait"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
}

fn complete_attack(fixture: &mut Fixture, execution: u64) -> Vec<f32> {
    let before = fixture.render_to(execution);
    assert!(before.iter().all(|sample| *sample == 0.0));
    // Include the actual first output sample. No startup or FIFO prefix is discarded.
    let mut pcm = fixture.render(1);
    pcm.extend(fixture.pcm(PCM_FRAMES - 1));
    pcm
}

#[test]
fn first_ordinary_gui_attack_uses_actual_nonzero_bank_and_complete_native_initial_pcm() {
    let mut ordinary = Fixture::new(true);
    let mut reference = Fixture::new(true);
    let pitch = PitchIntent::new(0, 7).unwrap();
    // Saved/source initialization has already installed the effective tuple. The
    // input RuntimeState still has its default requested pitch and is not authority.
    ordinary.mixer.set_pad_pitch_intent(0, pitch).unwrap();
    assert!(!ordinary.mixer.voices.iter().any(|voice| voice.active));
    let original = capture(&ordinary, 1.0);
    assert_eq!(original.target_frame(), None);
    ordinary.drain();
    assert_eq!(original.target_frame(), Some(0));
    assert!(!original.accepted());
    assert!(
        ordinary
            .control
            .engine
            .publish_pad_start(&original, None)
            .unwrap()
    );
    assert!(
        !original.accepted(),
        "producer publication is not Native admission"
    );
    ordinary.drain();
    assert!(original.accepted(), "reason={:?}", original.reason());
    assert_eq!(ordinary.starts(0), 0);

    // Independent existing input origin prepares the same physical attack. It owns
    // a different Native candidate, publication and worker request.
    let expected = reference
        .control
        .engine
        .capture_pad_pitch_action(0, "pitch.extra:7:set_retrigger", None)
        .unwrap();
    assert!(
        reference
            .control
            .engine
            .publish_pitch_action(&expected)
            .unwrap()
    );
    reference.drain();
    wait_ready(&mut ordinary, &[original.sequence()]);
    wait_ready(&mut reference, &[expected.sequence()]);
    let execution = original.execution_frame().unwrap();
    assert_eq!(original.original_frame(), Some(0));
    assert_eq!(execution, expected.execution_target());
    let pcm = complete_attack(&mut ordinary, execution);
    let expected_pcm = complete_attack(&mut reference, expected.execution_target());

    assert_eq!(
        original.state(),
        "applied",
        "reason={:?}",
        original.reason()
    );
    assert_eq!(original.effective_frame(), Some(execution));
    assert_eq!(ordinary.starts(0), 1);
    assert_eq!(ordinary.mixer.pad_pitch_intent(0), Some(pitch));
    let voice = ordinary.voice(0);
    assert_eq!(
        voice.stretch.adopted_request_id(),
        Some(original.sequence())
    );
    assert_ne!(voice.stretch.native_state_address(), 0);
    assert!(voice.stretch.owns_pitch_contract(pitch, true));
    let history = voice.stretch.productive_history().unwrap();
    assert!(history.fed_output_frames >= 4096);
    let taps = voice.stretch.native_tap_observation().unwrap();
    assert!(taps.left_reads > 0);
    assert_eq!(taps.missing_reads, 0);
    let position = voice.source_playback.position();
    assert_eq!(position.frame, LOOP_START + PCM_FRAMES);
    assert!(position.fraction.abs() < 1.0e-9);

    let initial_energy = pcm[..2048]
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / 2048.0;
    assert!(
        initial_energy > 0.001,
        "first Native/FIFO attack was silent"
    );
    let maximum_error = pcm
        .iter()
        .zip(&expected_pcm)
        .map(|(actual, expected)| (*actual - *expected).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        maximum_error < 2.0e-5,
        "complete initial PCM differs: {maximum_error}"
    );
    // Integer +7 and 160 Hz are an external frequency oracle; every initial sample
    // participates, including Native's first FIFO output and startup history.
    assert_tone(&pcm, 0, 7, 1.0, true);
}

#[test]
fn equal_target_scalar_gui_starts_keep_separate_lifecycles_volume_and_capture_order() {
    let mut fixture = Fixture::new(false);
    fixture.render_to(1000);
    assert!(fixture.mixer.play_sample(1, 0.3));
    let first = capture(&fixture, 0.4);
    let second = fixture
        .control
        .engine
        .capture_pad_start(
            0,
            LOOP_START as f64 / f64::from(RATE),
            Some(LOOP_END as f64 / f64::from(RATE)),
            0.7,
            true,
            None,
            None,
        )
        .unwrap();
    assert!(first.sequence() < second.sequence());
    drain_with_mode(&mut fixture, TriggerQuantization::Grid { step_64ths: 16 });
    assert_eq!(first.target_frame(), Some(24_000));
    assert_eq!(second.target_frame(), first.target_frame());
    // Publication order cannot coalesce the original accepted attacks or change the
    // scheduler's existing total input order at an equal target.
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&second, None)
            .unwrap()
    );
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&first, None)
            .unwrap()
    );
    fixture.drain();
    assert!(first.accepted() && second.accepted());
    assert_eq!(first.execution_frame(), Some(24_000));
    assert_eq!(second.execution_frame(), Some(24_000));
    assert_eq!(fixture.starts(0), 0);
    assert!(
        fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 1)
    );
    fixture.render_to(24_000);
    fixture.render(1);
    assert_eq!(first.state(), "applied");
    assert_eq!(second.state(), "applied");
    assert_eq!(first.effective_frame(), Some(24_000));
    assert_eq!(second.effective_frame(), Some(24_000));
    assert_eq!(fixture.starts(0), 2);
    assert_eq!(fixture.voice(0).volume, 0.7);
    assert!(
        !fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 1)
    );
    assert!(fixture.feedback.iter().any(|message| matches!(
        message,
        AudioMessage::ContentFeedback { id: 1, kind: 2, .. }
            | AudioMessage::SampleStopped { id: 1 }
    )));
    assert_eq!(fixture.voice(0).stretch.adopted_request_id(), None);
    assert!(fixture.scheduler.is_empty());
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&first, None)
            .unwrap()
    );
    fixture.drain();
    assert_eq!(fixture.starts(0), 2, "same owner must remain one-shot");
}

#[test]
fn public_stop_revokes_gui_start_before_publication_and_after_native_preparation() {
    for after_preparation in [false, true] {
        let mut fixture = Fixture::new(true);
        fixture
            .mixer
            .set_pad_pitch_intent(0, PitchIntent::new(0, 7).unwrap())
            .unwrap();
        let original = capture(&fixture, 1.0);
        fixture.drain();
        if after_preparation {
            assert!(
                fixture
                    .control
                    .engine
                    .publish_pad_start(&original, None)
                    .unwrap()
            );
            fixture.drain();
            wait_ready(&mut fixture, &[original.sequence()]);
        }
        fixture.control.engine.stop_sample(0).unwrap();
        fixture.drain();
        if !after_preparation {
            assert!(
                fixture
                    .control
                    .engine
                    .publish_pad_start(&original, None)
                    .is_err()
            );
        }
        let through = original.execution_frame().unwrap_or(8192) + 1;
        assert!(
            fixture
                .render_to(through)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert_eq!(fixture.starts(0), 0);
        assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
        assert_eq!(original.effective_frame(), None);
        if after_preparation {
            assert_eq!(original.state(), "rejected");
            assert_eq!(original.reason(), Some("launch_cancelled"));
        }
    }
}

#[cfg(windows)]
pub(super) fn install_synthetic_complete_lease(fixture: &mut Fixture) -> tempfile::TempDir {
    use super::cold_store::{ColdTransaction, PcmArtifactInput};
    use crate::messages::{ResidentContext, SampleBuffer};
    use serde_json::json;
    use std::sync::Arc;

    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("synthetic-ordinary.wav");
    std::fs::write(&source, b"synthetic ordinary-start complete-source owner").unwrap();
    let samples = (0..144_000)
        .map(|frame| {
            (0.25 * (frame as f64 * SOURCE_HZ * std::f64::consts::TAU / f64::from(RATE)).sin())
                as f32
        })
        .collect::<Vec<_>>();
    let mut transaction =
        ColdTransaction::capture(&temp.path().join("samples"), &source, true, &|| false).unwrap();
    let manifest = transaction
        .write_pcm_artifacts(
            PcmArtifactInput {
                samples: &samples,
                rate_hz: RATE,
                channels: 1,
                provenance: json!({"decoder":"synthetic-test-v1"}),
            },
            PcmArtifactInput {
                samples: &samples,
                rate_hz: RATE,
                channels: 1,
                provenance: json!({"processing":"identity-test-v1"}),
            },
            json!({"kind":"synthetic-test-identity","from_rate":RATE,"to_rate":RATE}),
            &|| false,
        )
        .unwrap();
    transaction.commit(&|| false).unwrap();
    let lease = transaction.into_lease();
    let mut complete = SampleBuffer {
        channels: 1,
        samples: Arc::from(samples),
        residency: None,
    };
    super::cold_residency::attach(&mut complete, &manifest).unwrap();
    let sample = complete
        .window(LOOP_START, LOOP_END, 2, ResidentContext::KeyLockFiniteLoop)
        .unwrap();
    lease.verify_reference(&sample).unwrap();
    lease.bind_sample(&sample).unwrap();
    // A later real range read extends this exact Native assignment. Installing
    // only the descriptor/lease would omit the Loader's actual reader custody.
    fixture
        .control
        .engine
        .project_assets
        .retain_cold(
            lease.clone(),
            &sample,
            Arc::downgrade(&fixture.control.engine.project_asset_engine_owner),
        )
        .unwrap();
    publish_tone(&fixture.control, 0, sample.clone(), true);
    fixture.control.engine.cold_leases.lock().unwrap()[0] = Some(lease);
    fixture.mixer.load_sample(0, sample);
    fixture.mixer.set_pad_loop_region(
        0,
        LOOP_START as f64 / f64::from(RATE),
        Some(LOOP_END as f64 / f64::from(RATE)),
    );
    fixture.mixer.set_pad_key_lock(0, true);
    temp
}

#[cfg(windows)]
#[test]
fn original_gui_grid_target_precedes_own_real_window_ack_and_is_never_requantized() {
    let mut fixture = Fixture::new(true);
    let _synthetic_files = install_synthetic_complete_lease(&mut fixture);
    fixture
        .mixer
        .set_pad_pitch_intent(0, PitchIntent::new(0, 7).unwrap())
        .unwrap();
    fixture.render_to(1000);
    let original = capture(&fixture, 1.0);
    let window = fixture
        .control
        .engine
        .prepare_captured_pad_start(&original)
        .unwrap();
    assert_ne!(window.publication_status(), "accepted");
    // Process only the actual first capture message. Keep its own real resident
    // transaction queued while the virtual output passes the original grid target.
    let target_message = fixture.control.consumer.pop().unwrap();
    assert!(matches!(
        &target_message,
        ControlMessage::CapturePadStartTarget(_)
    ));
    process_control_message(
        target_message,
        &mut fixture.scheduler,
        fixture.transport.output_frame(),
        &mut TriggerQuantization::Grid { step_64ths: 16 },
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    assert_eq!(original.target_frame(), Some(24_000));
    assert_eq!(original.original_frame(), Some(24_000));
    assert!(
        fixture
            .render_to(30_000)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_ne!(window.publication_status(), "accepted");
    drain_with_mode(&mut fixture, TriggerQuantization::Grid { step_64ths: 16 });
    assert_eq!(
        window.publication_status(),
        "accepted",
        "error={:?}",
        window.error()
    );
    assert!(window.is_current());
    assert_eq!(fixture.starts(0), 0);
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&original, Some(&window))
            .unwrap()
    );
    drain_with_mode(&mut fixture, TriggerQuantization::Grid { step_64ths: 16 });
    assert_eq!(original.original_frame(), Some(24_000));
    assert_eq!(original.target_frame(), Some(24_000));
    assert_eq!(original.execution_frame(), Some(38_192));
    wait_ready(&mut fixture, &[original.sequence()]);
    let pcm = complete_attack(&mut fixture, 38_192);
    assert_eq!(original.effective_frame(), Some(38_192));
    assert_eq!(fixture.starts(0), 1);
    assert_tone(&pcm, 0, 7, 1.0, true);
}
