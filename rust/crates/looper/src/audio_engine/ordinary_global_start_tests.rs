//! Public global starts retain one scheduler row and one atomic prepared group effect.

use super::global_playback_batch::GlobalPlaybackBatchTicket;
use super::input_mapping::launch_admission_tests::RATE;
use super::pitch_intent::PitchIntent;
use super::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START};
use pyo3::prelude::*;
use std::time::{Duration, Instant};

const PCM_FRAMES: usize = 48_000;
const SHIFTS: [i32; 2] = [7, -3];
const SOURCE_HZ: [f64; 2] = [160.0, 320.0];
const WORKER_WAIT: Duration = Duration::from_secs(2);

fn public_start(fixture: &Fixture) -> GlobalPlaybackBatchTicket {
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
                assert!(binding.is_current());
                (
                    Py::new(py, binding).unwrap(),
                    LOOP_START as f64 / f64::from(RATE),
                    Some(LOOP_END as f64 / f64::from(RATE)),
                )
            })
            .collect();
        fixture
            .control
            .engine
            .start_global_playback_batch(py, entries, None)
            .unwrap()
    })
}

fn restore_nonzero_bank(fixture: &mut Fixture) {
    for (id, shift) in SHIFTS.into_iter().enumerate() {
        fixture
            .mixer
            .set_pad_pitch_intent(id, PitchIntent::new(0, shift).unwrap())
            .unwrap();
        assert!(fixture.mixer.key_lock_for_measurement(id));
    }
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
}

fn wait_group(fixture: &mut Fixture, ticket: &GlobalPlaybackBatchTicket) {
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.ordinary_group_ready(2) {
            break;
        }
        assert_eq!(
            ticket.publication_status(),
            "pending",
            "group rejected while preparing"
        );
        assert!(
            Instant::now() < deadline,
            "global preparation exceeded its bounded host wait"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
}

fn wait_reference(fixture: &mut Fixture, sequences: [u64; 2]) {
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if sequences
            .into_iter()
            .all(|sequence| runtime.has_ready_action(sequence))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "reference preparation exceeded its bounded host wait"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn full_initial_attack(fixture: &mut Fixture, execution: u64) -> Vec<f32> {
    assert!(
        fixture
            .render_to(execution)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let mut pcm = fixture.render(1);
    pcm.extend(fixture.pcm(PCM_FRAMES - 1));
    pcm
}

fn spectral_amplitude(pcm: &[f32], frequency: f64) -> f64 {
    let (sine, cosine) =
        pcm.iter()
            .enumerate()
            .fold((0.0, 0.0), |(sine, cosine), (frame, sample)| {
                let phase = frame as f64 * frequency * std::f64::consts::TAU / f64::from(RATE);
                (
                    sine + f64::from(*sample) * phase.sin(),
                    cosine + f64::from(*sample) * phase.cos(),
                )
            });
    2.0 * sine.hypot(cosine) / pcm.len() as f64
}

#[test]
fn public_global_nonzero_first_attacks_share_one_e_and_adopt_two_own_native_fifos() {
    let mut fixture = Fixture::new(true);
    let mut reference = Fixture::new(true);
    restore_nonzero_bank(&mut fixture);
    let ticket = public_start(&fixture);
    assert_eq!(
        ticket.publication_status(),
        "pending",
        "producer queueing is not effect ACK"
    );
    fixture.drain();
    assert_eq!(
        fixture.scheduler.len(),
        1,
        "one group uses one existing scheduler row"
    );
    assert_eq!(fixture.scheduler.peek_next_target_frame(), Some(8192));
    assert_eq!(ticket.publication_status(), "pending");
    assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));

    // Separate existing pitch-input publications provide an independently owned
    // Native/worker reference. Neither reference candidate belongs to the group.
    let first = reference
        .control
        .engine
        .capture_pad_pitch_action(0, "pitch.extra:7:set_retrigger", None)
        .unwrap();
    let second = reference
        .control
        .engine
        .capture_pad_pitch_action(1, "pitch.extra:-3:set_retrigger", None)
        .unwrap();
    assert!(
        reference
            .control
            .engine
            .publish_pitch_action(&first)
            .unwrap()
    );
    assert!(
        reference
            .control
            .engine
            .publish_pitch_action(&second)
            .unwrap()
    );
    reference.drain();
    wait_group(&mut fixture, &ticket);
    wait_reference(&mut reference, [first.sequence(), second.sequence()]);
    assert_eq!(first.execution_target(), 8192);
    assert_eq!(second.execution_target(), 8192);
    let pcm = full_initial_attack(&mut fixture, 8192);
    let reference_pcm = full_initial_attack(&mut reference, 8192);

    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
    assert!(fixture.scheduler.is_empty());
    let mut request_ids = Vec::new();
    let mut native_handles = Vec::new();
    let mut source_addresses = Vec::new();
    for (id, shift) in SHIFTS.into_iter().enumerate() {
        let pitch = PitchIntent::new(0, shift).unwrap();
        assert_eq!(fixture.mixer.pad_pitch_intent(id), Some(pitch));
        assert!(fixture.mixer.key_lock_for_measurement(id));
        let voice = fixture.voice(id);
        request_ids.push(voice.stretch.adopted_request_id().unwrap());
        native_handles.push(voice.stretch.native_state_address());
        assert!(voice.stretch.owns_pitch_contract(pitch, true));
        let history = voice.stretch.productive_history().unwrap();
        assert!(history.fed_output_frames >= 4096);
        source_addresses.push(history.binding.source_address);
        assert_eq!(
            history.binding.source_address,
            voice.sample.as_ref().unwrap().source_address()
        );
        let taps = voice.stretch.native_tap_observation().unwrap();
        assert!(taps.left_reads > 0);
        assert_eq!(taps.missing_reads, 0);
        assert_eq!(
            voice.source_playback.position().frame,
            LOOP_START + PCM_FRAMES
        );
    }
    assert_ne!(request_ids[0], request_ids[1]);
    assert_ne!(native_handles[0], 0);
    assert_ne!(native_handles[1], 0);
    assert_ne!(native_handles[0], native_handles[1]);
    assert_ne!(source_addresses[0], source_addresses[1]);
    let initial_energy = pcm[..2048]
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / 2048.0;
    assert!(
        initial_energy > 0.001,
        "the group's complete initial Native/FIFO attack was silent"
    );
    let maximum_error = pcm
        .iter()
        .zip(&reference_pcm)
        .map(|(actual, expected)| (*actual - *expected).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        maximum_error < 4.0e-5,
        "complete initial mixed PCM differs: {maximum_error}"
    );
    for (source, shift) in SOURCE_HZ.into_iter().zip(SHIFTS) {
        // Independent integer-semitone oracle on all PCM, with no startup discard.
        let expected = source * 2.0_f64.powf(f64::from(shift) / 12.0);
        assert!(
            spectral_amplitude(&pcm, expected) > 0.02,
            "missing independent mixed component at {expected} Hz"
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum Invalidation {
    ActualBankTuple,
    SourceReplacement,
    PublicStop,
}

#[test]
fn prepared_global_group_rejects_every_participant_when_one_actual_owner_changes_before_e() {
    for invalidation in [
        Invalidation::ActualBankTuple,
        Invalidation::SourceReplacement,
        Invalidation::PublicStop,
    ] {
        let mut fixture = Fixture::new(true);
        restore_nonzero_bank(&mut fixture);
        let ticket = public_start(&fixture);
        fixture.drain();
        assert_eq!(fixture.scheduler.len(), 1);
        wait_group(&mut fixture, &ticket);
        assert_eq!(ticket.publication_status(), "pending");
        match invalidation {
            Invalidation::ActualBankTuple => fixture
                .mixer
                .set_pad_pitch_intent(1, PitchIntent::new(0, -4).unwrap())
                .unwrap(),
            Invalidation::SourceReplacement => fixture.control.publish(1, 0.2, 77),
            Invalidation::PublicStop => {
                fixture.control.engine.stop_sample(1).unwrap();
                fixture.drain();
            }
        }
        let pcm = fixture.render_to(8193);
        assert!(
            pcm.iter().all(|sample| *sample == 0.0),
            "partial audio after {invalidation:?}"
        );
        assert_eq!(ticket.publication_status(), "rejected", "{invalidation:?}");
        assert_eq!(
            (fixture.starts(0), fixture.starts(1)),
            (0, 0),
            "partial feedback after {invalidation:?}"
        );
        assert!(
            !fixture.mixer.voices.iter().any(|voice| voice.active),
            "partial voice after {invalidation:?}"
        );
        assert_eq!(
            fixture.mixer.pad_pitch_intent(0),
            Some(PitchIntent::new(0, 7).unwrap())
        );
        assert!(fixture.scheduler.is_empty());
    }
}

#[test]
fn scalar_global_group_keeps_immediate_callback_start_without_native_adoption() {
    let mut fixture = Fixture::new(false);
    fixture.render_to(500);
    let ticket = public_start(&fixture);
    assert_eq!(ticket.publication_status(), "pending");
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    fixture.drain();
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(fixture.transport.output_frame(), 500);
    assert!(fixture.scheduler.is_empty());
    assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
    for id in [0, 1] {
        assert_eq!(
            fixture.voice(id).source_playback.position().frame,
            LOOP_START
        );
        assert_eq!(fixture.voice(id).stretch.adopted_request_id(), None);
        assert_eq!(
            fixture.mixer.pad_pitch_intent(id),
            Some(PitchIntent::default())
        );
        assert!(!fixture.mixer.key_lock_for_measurement(id));
    }
    fixture.render(1);
    for id in [0, 1] {
        assert_eq!(
            fixture.voice(id).source_playback.position().frame,
            LOOP_START + 1
        );
    }
}
