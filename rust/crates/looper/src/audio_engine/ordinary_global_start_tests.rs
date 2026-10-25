//! Public global starts retain one scheduler row and one atomic prepared group effect.

use super::global_playback_batch::GlobalPlaybackBatchTicket;
use super::input_mapping::launch_admission_tests::RATE;
use super::pitch_intent::PitchIntent;
use super::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START, physical_birth_tone_reference};
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

fn wait_reference(fixture: &mut Fixture, actions: [&super::pitch_actions::CapturedPitchAction; 2]) {
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if actions
            .into_iter()
            .all(|action| runtime.has_ready_action(action.sequence()))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "reference preparation exceeded its bounded host wait: {:?}",
            actions.map(|action| (action.state(), action.reason()))
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

#[test]
fn missing_map_global_start_keeps_two_ordinary_native_beginnings_and_pending_requests() {
    let mut pending = Fixture::new(true);
    let mut reference = Fixture::new(true);
    restore_nonzero_bank(&mut pending);
    restore_nonzero_bank(&mut reference);
    super::ordinary_start_tests::publish_missing_map_intent(&pending);
    let actual = public_start(&pending);
    let expected = public_start(&reference);
    pending.drain();
    reference.drain();
    wait_group(&mut pending, &actual);
    wait_group(&mut reference, &expected);
    let execution = pending.scheduler.peek_next_target_frame().unwrap();
    assert_eq!(
        reference.scheduler.peek_next_target_frame(),
        Some(execution)
    );
    let pcm = full_initial_attack(&mut pending, execution);
    assert_eq!(pcm, full_initial_attack(&mut reference, execution));
    assert!(pcm.iter().any(|value| value.abs() > 0.01));
    assert_eq!(actual.publication_status(), "accepted");
    for id in 0..2 {
        let voice = pending.voice(id);
        assert_eq!(pending.starts(id), 1);
        assert_eq!(
            voice.sync_request.mode,
            super::source_sync_policy::SourceSyncMode::Legacy
        );
        assert_eq!(voice.sync_request.intent_revision, 19);
        assert!(voice.mapped_start.is_none());
        assert!(
            voice
                .stretch
                .owns_pitch_contract(PitchIntent::new(0, SHIFTS[id]).unwrap(), true)
        );
        assert_ne!(voice.stretch.native_state_address(), 0);
    }
    assert_ne!(
        pending.voice(0).stretch.native_state_address(),
        pending.voice(1).stretch.native_state_address()
    );
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

    // Genuine future-grid piano births supply complete independently owned
    // Native beginnings. Piano's separate late-history contract must not crop
    // an ordinary-group reference before its first actual emitted sample.
    let mut rows = vec![super::input_mapping::PitchPadProjection::default(); super::NUM_SAMPLES];
    for row in &mut rows[..2] {
        row.key_lock = true;
        row.source_root = Some(0);
        row.source_mode = Some(super::pitch_actions::PitchKeyMode::Major);
        row.quantize_step_64ths = Some(16);
    }
    reference
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_pitch_runtime_state(rows)
        .unwrap();
    assert!(
        reference
            .render_to(17_000)
            .iter()
            .all(|value| *value == 0.0)
    );
    let first = reference.capture(0, "pitch.extra:7:set_retrigger");
    let second = reference.capture(1, "pitch.extra:-3:set_retrigger");
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
    wait_reference(&mut reference, [&first, &second]);
    assert_eq!(first.execution_target(), 24_000);
    assert_eq!(second.execution_target(), 24_000);
    assert_eq!(first.original_due_frame(), 24_000);
    assert_eq!(second.original_due_frame(), 24_000);
    let pcm = full_initial_attack(&mut fixture, 8192);
    let reference_pcm = full_initial_attack(&mut reference, 24_000);

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
        assert_eq!(
            taps.left_reads, 0,
            "negative physical-birth input must be silent"
        );

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
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    let raw_references = SOURCE_HZ
        .into_iter()
        .zip(SHIFTS)
        .map(|(source, shift)| {
            physical_birth_tone_reference(PCM_FRAMES, source, 0, shift, 1.0, true)
        })
        .collect::<Vec<_>>();
    let raw_error = pcm
        .iter()
        .enumerate()
        .map(|(frame, actual)| {
            (*actual - (raw_references[0][frame] + raw_references[1][frame])).abs()
        })
        .fold(0.0_f32, f32::max);
    assert!(
        raw_error < 4.0e-5,
        "complete independent raw mixed birth differs: {raw_error}"
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

pub(super) fn phase_group(lock: bool) -> (Fixture, GlobalPlaybackBatchTicket) {
    let mut fixture = Fixture::new(lock);
    for (id, beat) in [(0, 64.0), (1, 256.0)] {
        fixture.mixer.set_pad_bpm(id, Some(120.0));
        fixture.mixer.set_pad_timing_metadata(
            id,
            crate::messages::PadTimingMetadata {
                phase_anchor_s: LOOP_START as f64 / f64::from(RATE) - beat * 0.5,
            },
        );
    }
    if lock {
        restore_nonzero_bank(&mut fixture);
    }
    fixture.render_to(500);
    let ticket = public_start(&fixture);
    fixture.drain();
    if lock {
        wait_group(&mut fixture, &ticket);
        let execution = fixture.scheduler.peek_next_target_frame().unwrap();
        assert!(
            fixture
                .render_to(execution)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        // Execute the real due claim, without consuming the first output sample.
        assert!(fixture.render(0).is_empty());
    }
    assert_eq!(ticket.publication_status(), "accepted");
    for id in [0, 1] {
        assert!(fixture.voice(id).phase_launch_qualified);
        assert!(fixture.voice(id).phase_birth_unadvanced);
        assert!(
            fixture
                .voice(id)
                .source_playback
                .phase_reference_current(&fixture.transport)
        );
    }
    (fixture, ticket)
}

fn request_phase_bootstrap(fixture: &mut Fixture) {
    super::audio_stream::process_control_message(
        crate::messages::ControlMessage::BootstrapTransportFromPad { id: 0 },
        &mut fixture.scheduler,
        fixture.transport.output_frame(),
        &mut crate::messages::TriggerQuantization::Immediate,
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
}

#[test]
fn genuine_global_dry_births_rebind_same_boundary_bootstrap_without_moving_either_attack() {
    let (mut actual, _) = phase_group(false);
    let (mut reference, _) = phase_group(false);
    assert_eq!(actual.mixer.active_pad_beat_position(0), Some(64.0));
    assert_eq!(actual.mixer.active_pad_beat_position(1), Some(256.0));
    let old = actual.transport;
    request_phase_bootstrap(&mut actual);
    assert!(actual.render(0).is_empty());
    assert!(actual.voice(0).phase_birth_unadvanced);
    assert_eq!(actual.transport.phase_generation(), old.phase_generation());
    assert_eq!(actual.render(1), reference.render(1));
    assert_ne!(actual.transport.phase_generation(), old.phase_generation());
    for id in [0, 1] {
        let voice = actual.voice(id);
        assert!(voice.phase_launch_qualified);
        assert!(!voice.phase_birth_unadvanced);
        assert!(
            voice
                .source_playback
                .phase_reference_current(&actual.transport)
        );
        assert_eq!(
            voice.source_playback.position(),
            reference.voice(id).source_playback.position()
        );
    }
    assert_eq!(actual.pcm(1023), reference.pcm(1023));
}

#[test]
fn same_boundary_loop_aba_fences_only_changed_birth_and_noop_keeps_custody() {
    let (mut fixture, _) = phase_group(false);
    let original = fixture.voice(0).source_playback;
    let start = LOOP_START as f64 / f64::from(RATE);
    let end = Some(LOOP_END as f64 / f64::from(RATE));
    fixture.mixer.set_pad_loop_region(0, start, end);
    assert!(fixture.voice(0).phase_launch_qualified);
    assert!(fixture.voice(0).phase_birth_unadvanced);
    assert!(fixture.voice(0).source_playback.matches_exact(&original));
    for next in [LOOP_START + 1, LOOP_START] {
        let edit = fixture
            .mixer
            .prepare_loop_edit(0, next as f64 / f64::from(RATE), end)
            .unwrap();
        fixture.mixer.apply_loop_edit_claimed(edit);
    }
    assert!(!fixture.voice(0).phase_launch_qualified);
    assert!(!fixture.voice(0).phase_birth_unadvanced);
    assert!(fixture.voice(1).phase_launch_qualified);
    assert!(fixture.voice(1).phase_birth_unadvanced);
    request_phase_bootstrap(&mut fixture);
    fixture.render(1);
    assert!(
        !fixture
            .voice(0)
            .source_playback
            .phase_reference_current(&fixture.transport)
    );
    assert!(
        fixture
            .voice(1)
            .source_playback
            .phase_reference_current(&fixture.transport)
    );
}

#[test]
fn replacement_bank_loop_edit_preserves_foreign_retained_birth_custody() {
    let (mut fixture, _) = phase_group(false);
    let original = fixture.voice(0).source_playback;
    let address = fixture.voice(0).sample.as_ref().unwrap().source_address();
    let replacement = fixture.voice(1).sample.as_ref().unwrap().clone();
    assert_ne!(replacement.source_address(), address);
    assert!(
        fixture
            .mixer
            .load_sample_rt(0, replacement, &mut fixture.retirement)
    );
    fixture.mixer.set_pad_loop_region(
        0,
        (LOOP_START + 1) as f64 / f64::from(RATE),
        Some(LOOP_END as f64 / f64::from(RATE)),
    );
    let voice = fixture.voice(0);
    assert_eq!(voice.sample.as_ref().unwrap().source_address(), address);
    assert!(voice.source_playback.matches_exact(&original));
    assert!(voice.phase_launch_qualified && voice.phase_birth_unadvanced);
}

#[test]
fn resident_executor_claims_preserve_noop_and_foreign_birth_but_fence_changed_region_aba() {
    use super::input_runtime_binding::capture;
    use super::prepared_source::PreparedSourcePermit;
    use crate::messages::{
        ControlMessage, ResidentContext, ResidentControlIntent, ResidentTransaction,
    };
    use std::sync::atomic::Ordering;

    fn relocate(
        fixture: &mut Fixture,
        sample: &crate::messages::SampleBuffer,
        end: usize,
        revision: u64,
    ) {
        let binding = capture(&fixture.control.engine, 0)
            .unwrap()
            .unwrap()
            .binding;
        let publication = PreparedSourcePermit::new(
            fixture.control.engine.prepared_source_epochs[0].clone(),
            fixture.control.engine.prepared_source_epochs[0].load(Ordering::Acquire),
        );
        publication.mark_pending().unwrap();
        let next = sample
            .window(
                LOOP_START,
                end,
                revision + 1,
                ResidentContext::KeyLockFiniteLoop,
            )
            .unwrap();
        fixture
            .control
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::RelocateResident(Box::new(
                ResidentTransaction {
                    id: 0,
                    sample: next.clone(),
                    stems: None,
                    binding,
                    publication: publication.clone(),
                    expected_window_revision: revision,
                    intent: ResidentControlIntent {
                        loop_region: Some((LOOP_START, Some(end))),
                        ..Default::default()
                    },
                    seek_pin: None,
                },
            )))
            .unwrap();
        fixture.drain();
        assert_eq!(publication.status(), "accepted");
        // Executor-only fixture: consume this actual storage ACK into the control
        // cache before capturing another binding; no worker preparation is claimed.
        fixture.control.engine.sample_cache.lock().unwrap()[0] = Some(next);
    }

    let (mut fixture, _) = phase_group(false);
    let original = fixture.voice(0).source_playback;
    let sample = fixture.voice(0).sample.as_ref().unwrap().clone();
    let revision = sample.window_revision();
    relocate(&mut fixture, &sample, LOOP_END, revision);
    assert!(fixture.voice(0).phase_launch_qualified && fixture.voice(0).phase_birth_unadvanced);
    assert!(fixture.voice(0).source_playback.matches_exact(&original));
    for (offset, end) in [LOOP_END - 1, LOOP_END].into_iter().enumerate() {
        relocate(&mut fixture, &sample, end, revision + 1 + offset as u64);
        assert!(!fixture.voice(0).phase_launch_qualified);
        assert!(!fixture.voice(0).phase_birth_unadvanced);
        assert!(fixture.voice(0).source_playback.matches_exact(&original));
        assert!(fixture.voice(1).phase_launch_qualified && fixture.voice(1).phase_birth_unadvanced);
    }

    let (mut foreign, _) = phase_group(false);
    let original = foreign.voice(0).source_playback;
    let address = foreign.voice(0).sample.as_ref().unwrap().source_address();
    let replacement = foreign.voice(1).sample.as_ref().unwrap().clone();
    super::pitch_runtime_tests::publish_tone(&foreign.control, 0, replacement.clone(), true);
    assert!(
        foreign
            .mixer
            .load_sample_rt(0, replacement.clone(), &mut foreign.retirement)
    );
    relocate(
        &mut foreign,
        &replacement,
        LOOP_END - 1,
        replacement.window_revision(),
    );
    assert_eq!(
        foreign.voice(0).sample.as_ref().unwrap().source_address(),
        address
    );
    assert!(foreign.voice(0).source_playback.matches_exact(&original));
    assert!(foreign.voice(0).phase_launch_qualified && foreign.voice(0).phase_birth_unadvanced);
}

#[test]
fn native_birth_bootstrap_and_no_cut_loop_aba_preserve_complete_first_pcm_and_owners() {
    for edit in [false, true] {
        let (mut actual, _) = phase_group(true);
        let (mut reference, _) = phase_group(true);
        let previous = actual.transport;
        let snapshots = [0, 1].map(|id| {
            let voice = actual.voice(id);
            (
                voice.source_playback,
                voice.stretch.native_state_address(),
                voice.stretch.adopted_request_id(),
                voice.stretch.pending_fifo_frames(),
            )
        });
        if edit {
            for end in [LOOP_END - 1, LOOP_END] {
                let change = actual
                    .mixer
                    .prepare_loop_edit(
                        0,
                        LOOP_START as f64 / f64::from(RATE),
                        Some(end as f64 / f64::from(RATE)),
                    )
                    .unwrap();
                actual.mixer.apply_loop_edit_claimed(change);
            }
            assert!(!actual.voice(0).phase_launch_qualified);
            assert!(!actual.voice(0).phase_birth_unadvanced);
        }
        for (id, snapshot) in snapshots.iter().enumerate() {
            let voice = actual.voice(id);
            assert!(voice.source_playback.matches_exact(&snapshot.0));
            assert_eq!(voice.stretch.native_state_address(), snapshot.1);
            assert_eq!(voice.stretch.adopted_request_id(), snapshot.2);
            assert_eq!(voice.stretch.pending_fifo_frames(), snapshot.3);
        }
        request_phase_bootstrap(&mut actual);
        assert_eq!(actual.render(1), reference.render(1));
        assert_ne!(
            actual.transport.phase_generation(),
            previous.phase_generation()
        );
        for (id, snapshot) in snapshots.iter().enumerate() {
            let voice = actual.voice(id);
            assert!(
                !voice
                    .source_playback
                    .phase_reference_current(&actual.transport),
                "Native copies retain their old generation until owner-consistent qualification exists"
            );
            assert_eq!(voice.stretch.native_state_address(), snapshot.1);
            assert_eq!(voice.stretch.adopted_request_id(), snapshot.2);
        }
        assert_eq!(actual.pcm(4095), reference.pcm(4095));
        assert!(
            actual
                .voice(0)
                .source_playback
                .matches_exact(&reference.voice(0).source_playback)
        );
    }
}
