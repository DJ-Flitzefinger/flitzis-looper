//! Original nonneutral Pitch work through actual Pair Move/Swap and scheduled adoption.
//! Strong source/Native ownership remains independent of the delayed control mirror.
//! These hardware-free profiles leave acoustic and performer-deadline acceptance open.

use super::*;
use crate::audio_engine::cold_load;
use crate::audio_engine::content_placement::{ContentKey, PairPlacementKind};
use crate::audio_engine::pad_pair::{PadPairTicket, control as pair_control};
use crate::audio_engine::pitch_actions::PitchActionReject;
use crate::messages::LoaderEvent;
use std::path::Path;

const A: usize = 0;
const B: usize = 1;
const EMPTY: usize = 2;
const CAPTURE_FRAME: u64 = 17_000;
const EFFECT_FRAME: u64 = 24_000;

fn diagnostic_started_slots(fixture: &Fixture, since: usize) -> Vec<usize> {
    // This fixture's Vec sink emits diagnostic SampleStarted IDs and discards
    // captured ContentFeedback. Phase cutoffs prevent historical slot IDs from
    // being attributed to a later occupant. Native receipts/source guards below
    // establish content ownership; these diagnostics count attacks only.
    fixture.feedback[since..]
        .iter()
        .filter_map(|message| {
            if let AudioMessage::SampleStarted { id } = message {
                Some(*id)
            } else {
                None
            }
        })
        .collect()
}

fn start_independent_b(fixture: &mut Fixture) {
    // B already owns genuine nonneutral Native state before A is captured.
    let b = fixture.capture(B, "pitch.extra:3:set_retrigger");
    assert!(!b.exclusive());
    fixture.publish(&b);
    fixture.apply(&b);
    assert_native_source(fixture, B, &b);
    assert_eq!(diagnostic_started_slots(fixture, 0), vec![B]);
}

fn original_is_unchanged(action: &CapturedPitchAction, original: (u64, ContentKey, u64)) {
    assert_eq!(action.sequence(), original.0);
    assert_eq!(action.content_key(), original.1);
    assert_eq!(action.received_timestamp(), original.2);
    assert_eq!(action.original_due_frame(), EFFECT_FRAME);
    assert_eq!(action.execution_target(), EFFECT_FRAME);
    assert_eq!(action.render().pitch, PitchIntent::new(0, 7).unwrap());
    assert!(!action.render().key_lock);
    assert!(!action.sync());
    assert!(!action.exclusive());
}

fn finish_original_pair(fixture: &Fixture, ticket: &PadPairTicket) {
    let ack = ticket.own_acknowledgement().unwrap();
    assert!(fixture.control.engine._reconcile_pad_pair(ticket).unwrap());
    assert!(
        fixture
            .control
            .engine
            ._validate_pad_pair_acknowledgement(ticket, &ack)
            .unwrap()
    );
    fixture
        .control
        .engine
        ._finish_pad_pair(ticket, &ack)
        .unwrap();
}

fn refill_former_a(fixture: &mut Fixture, original_a: ContentKey) -> (ContentKey, SampleBuffer) {
    // Mirror the existing Pair/Hold cold-load proof's real admission/worker/
    // callback/ACK path. This explicit mono format and owned output root are the
    // established hardware-free seam, not the literal stereo public wrapper.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let owned = workspace.join("test-artifacts/pitch-remap-worker-20261016");
    // A fresh checkout can run the regression without manual workspace setup.
    // Never overwrite the block's existing ownership or retention instructions.
    std::fs::create_dir_all(&owned).unwrap();
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(owned.join("OWNER.md"))
    {
        Ok(mut owner) => {
            use std::io::Write;
            owner.write_all(b"Owner: pitch remap regression. Reproducible cold-load test output; retain until all jobs are terminal and reviewed delivery is complete.\n").unwrap();
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => panic!("pitch remap ownership: {error}"),
    }
    // Keep outputs for terminal-job/storage review and native cleanup.
    let directory = tempfile::Builder::new()
        .prefix("move-refill-")
        .tempdir_in(owned)
        .unwrap()
        .keep();
    std::fs::write(directory.join("OWNER.md"),
        "Owner: pitch remap worker regression. Own reproducible cold-load input/output; Clean after terminal jobs.\n").unwrap();
    let path = directory.join("fresh-c.wav");
    let data_bytes = 128_u32 * 2;
    let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for _ in 0..128 {
        bytes.extend_from_slice(&8192_i16.to_le_bytes());
    }
    std::fs::write(&path, bytes).unwrap();

    fixture.control.engine.unload_sample(A).unwrap();
    fixture.drain();
    assert!(fixture.control.engine.sample_cache.lock().unwrap()[A].is_none());
    let request = cold_load::admit_for_format(
        &fixture.control.engine,
        A,
        path.to_string_lossy().into_owned(),
        false,
        false,
        true,
        fixture.control.producer.clone(),
        1,
        RATE,
        directory.join("samples"),
    )
    .unwrap();
    // Same bounded cold-worker host wait as the existing Pair/Hold proof. This
    // does not advance virtual audio time or alter the original action's T/E.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut success = false;
    loop {
        fixture.drain();
        while let Ok(event) = fixture
            .control
            .engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
            .map(LoaderEvent::into_test_payload)
        {
            match event {
                LoaderEvent::Success { id, request_id, .. } => {
                    assert_eq!(id, A);
                    assert_eq!(request_id, request);
                    success = true;
                }
                LoaderEvent::Error { error, .. } => panic!("fresh C failed: {error}"),
                _ => {}
            }
        }
        if success
            && fixture.control.engine.cold_loading[A].load(Ordering::Acquire) == 0
            && fixture
                .control
                .engine
                .active_tasks
                .lock()
                .unwrap()
                .is_empty()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fresh C cold source did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), CAPTURE_FRAME);
    assert_eq!(fixture.scheduler.len(), 1); // A's original future event survives.
    assert!(fixture.control.consumer.is_empty());
    let sample = fixture.control.engine.sample_cache.lock().unwrap()[A]
        .clone()
        .unwrap();
    assert_eq!(sample.channels, 1);
    assert_eq!(sample.samples.as_ref(), &[0.25; 128]);
    assert!(
        fixture.mixer.bank_for_measurement()[A]
            .as_ref()
            .unwrap()
            .same_source(&sample)
    );
    let ownership = &fixture.control.engine.input_runtime_ownership;
    let binding = input_runtime_binding::capture(&fixture.control.engine, A)
        .unwrap()
        .unwrap();
    assert!(binding.current());
    assert!(ownership.binding_source_adoption_accepted(A, binding.binding));
    let key = ownership.slot_snapshot(A).unwrap().content_key().unwrap();
    assert_ne!(key, original_a);
    assert_ne!(key.lifetime(), original_a.lifetime());
    assert!(
        fixture
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.active || voice.sample_id != A)
    );
    (key, sample)
}

fn remap_case(target: usize, prepared_before_pair: bool) {
    let mut fixture = Fixture::new(false);
    let mut background = Fixture::new(false);
    start_independent_b(&mut fixture);
    start_independent_b(&mut background);
    let after_b_start = fixture.feedback.len();
    assert_eq!(
        fixture.render_to(CAPTURE_FRAME),
        background.render_to(CAPTURE_FRAME)
    );

    let b_key = fixture
        .control
        .engine
        .input_runtime_ownership
        .slot_snapshot(B)
        .unwrap()
        .content_key()
        .unwrap();
    let b_generation = fixture.voice(B).generation;
    let b_native = fixture.voice(B).stretch.native_state_address();
    let b_sample = fixture.voice(B).sample.as_ref().unwrap().clone();
    let b_after = if target == B { A } else { B };
    let allocations = fixture.control.engine.pitch_work_capacity.allocations();

    fixture.rows(PitchPadProjection {
        source_root: Some(0),
        source_mode: Some(PitchKeyMode::Major),
        key_lock: false,
        sync: false,
        quantize_step_64ths: Some(16),
        ..PitchPadProjection::default()
    });
    // Original capture precedes Pair capture. Its requested intent therefore
    // belongs to the exact rows captured by Pair; it is never recaptured.
    let action = fixture.capture(A, "pitch.extra:7:set_retrigger");
    let original = (
        action.sequence(),
        action.content_key(),
        action.received_timestamp(),
    );
    assert_eq!(action.original_due_frame(), EFFECT_FRAME);
    if prepared_before_pair {
        fixture.publish(&action);
        fixture.wait_worker(&[&action]);
        assert_eq!(action.state(), "prepared");
    }

    let captured = fixture.control.engine._capture_pad_pair(A, target).unwrap();
    // Same productive Pair preparation as public _prepare_pad_pair, with
    // this existing device-free callback's real mono format. Its public
    // no-device default is stereo, so calling that wrapper is unsuitable.
    let ticket = pair_control::prepare(&fixture.control.engine, &captured, RATE, 1).unwrap();
    fixture.control.engine._publish_pad_pair(&ticket).unwrap();
    if !prepared_before_pair {
        // Deliberately queue Pitch after Pair, while control placement is
        // still original. Fixture::publish would drain prematurely here.
        assert!(
            fixture
                .control
                .engine
                .publish_pitch_action(&action)
                .unwrap()
        );
    }
    fixture.drain();
    assert!(fixture.control.consumer.is_empty());
    let receipt = ticket.native_acknowledgement().unwrap();
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(
        receipt.kind,
        if target == EMPTY {
            PairPlacementKind::Move
        } else {
            PairPlacementKind::Swap
        }
    );
    assert_eq!(receipt.target_after.lifetime, original.1.lifetime());
    assert!(matches!(
        action.control_guard(None),
        Err(PitchActionReject::PlacementPending)
    ));
    assert_eq!(
        action
            .callback_guard(None)
            .unwrap_or_else(|error| {
                panic!(
                    "original Native guard {error:?}; state={} reason={:?}",
                    action.state(),
                    action.reason()
                )
            })
            .id,
        target
    );
    assert!(!action.effective());
    assert_eq!(fixture.transport.output_frame(), CAPTURE_FRAME);

    // No reconcile/finish has happened. In the during-Pair profile, reaching
    // PREPARED proves the original worker progressed during the held mirror.
    // Keep the existing host wait bound and virtual clock unchanged.
    fixture.wait_worker(&[&action]);
    assert_eq!(action.state(), "prepared");
    assert!(matches!(
        action.control_guard(None),
        Err(PitchActionReject::PlacementPending)
    ));
    original_is_unchanged(&action, original);
    assert_eq!(
        fixture.control.engine.pitch_work_capacity.allocations(),
        (allocations.0 + 1, allocations.1 + 1)
    );
    // Neither Pair nor the held mirror has introduced A's attack or restarted B.
    assert!(diagnostic_started_slots(&fixture, after_b_start).is_empty());
    assert_eq!(fixture.voice(b_after).generation, b_generation);
    assert_eq!(
        fixture.voice(b_after).stretch.native_state_address(),
        b_native
    );

    finish_original_pair(&fixture, &ticket);
    assert_eq!(action.control_guard(None).unwrap().id, target);
    let fresh_c = if target == EMPTY {
        assert!(fixture.mixer.bank_for_measurement()[A].is_none());
        assert!(fixture.control.engine.sample_cache.lock().unwrap()[A].is_none());
        Some(refill_former_a(&mut fixture, original.1))
    } else {
        // Actual Swap already reuses former A's slot for independent B.
        assert_eq!(
            fixture
                .control
                .engine
                .input_runtime_ownership
                .slot_snapshot(A)
                .unwrap()
                .content_key(),
            Some(b_key)
        );
        None
    };
    original_is_unchanged(&action, original);
    assert!(diagnostic_started_slots(&fixture, after_b_start).is_empty());
    let before_a_effect = fixture.feedback.len();

    let before = fixture.apply(&action);
    assert_eq!(before, background.render_to(EFFECT_FRAME));
    background.render(1);
    assert_eq!(fixture.transport.output_frame(), EFFECT_FRAME + 1);
    original_is_unchanged(&action, original);
    assert!(action.effective());
    assert_native_source(&fixture, target, &action);
    assert_eq!(
        fixture
            .voice(target)
            .source_admission
            .as_ref()
            .unwrap()
            .content_key(),
        Some(original.1)
    );
    // Swap's old B attack was also diagnosed at slot 1. Only this new feedback
    // slice belongs to A's actual effect at its remapped target.
    assert_eq!(
        diagnostic_started_slots(&fixture, before_a_effect),
        vec![target]
    );
    assert_source_phase(
        fixture.voice(target).source_playback.position(),
        LOOP_START as f64,
        1,
        1.0,
    );

    let mixed = fixture.pcm(48_000);
    let b_only = background.pcm(48_000);
    // Same independent-reference subtraction as routed_midi_tests. The
    // crossing oracle sees A alone, not the mixed 160/320-Hz sources.
    // B's own Native/generation/phase/PCM pins are checked separately.
    let a_only: Vec<_> = mixed.iter().zip(b_only).map(|(a, b)| a - b).collect();
    assert_tone(&a_only[12_000..], 0, 7, 1.0, false);
    assert_source_phase(
        fixture.voice(target).source_playback.position(),
        LOOP_START as f64,
        48_001,
        1.0,
    );
    let b = fixture.voice(b_after);
    assert_eq!(
        b.source_admission.as_ref().unwrap().content_key(),
        Some(b_key)
    );
    assert_eq!(b.generation, b_generation);
    assert_eq!(b.stretch.native_state_address(), b_native);
    assert!(Arc::ptr_eq(
        &b.sample.as_ref().unwrap().samples,
        &b_sample.samples
    ));
    assert!(
        b.source_playback
            .matches_exact(&background.voice(B).source_playback)
    );
    assert_eq!(
        fixture.mixer.pad_pitch_intent(b_after),
        Some(PitchIntent::new(0, 3).unwrap())
    );
    // Exactly A's one new attack; no B restart at its new slot or C attack.
    assert_eq!(
        diagnostic_started_slots(&fixture, before_a_effect),
        vec![target]
    );
    if let Some((c_key, c_sample)) = fresh_c {
        assert_eq!(
            fixture
                .control
                .engine
                .input_runtime_ownership
                .slot_snapshot(A)
                .unwrap()
                .content_key(),
            Some(c_key)
        );
        assert!(
            fixture.mixer.bank_for_measurement()[A]
                .as_ref()
                .unwrap()
                .same_source(&c_sample)
        );
        assert!(Arc::ptr_eq(
            &fixture.mixer.bank_for_measurement()[A]
                .as_ref()
                .unwrap()
                .samples,
            &c_sample.samples
        ));
        assert_eq!(
            fixture.mixer.pad_pitch_intent(A),
            Some(PitchIntent::default())
        );
        assert!(
            diagnostic_started_slots(&fixture, after_b_start)
                .iter()
                .all(|id| *id != A)
        );
        assert!(
            fixture
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != A)
        );
    }
    assert_eq!(ticket.native_acknowledgement(), Some(receipt));
    assert!(fixture.scheduler.is_empty());
}

#[test]
fn original_pitch_worker_prepares_during_move_control_mirror_lag() {
    remap_case(EMPTY, false);
}

#[test]
fn original_pitch_worker_prepares_during_swap_control_mirror_lag() {
    remap_case(B, false);
}

#[test]
fn prepared_original_pitch_survives_move_and_fresh_former_slot() {
    remap_case(EMPTY, true);
}

#[test]
fn prepared_original_pitch_survives_swap_with_independent_native_b() {
    remap_case(B, true);
}
