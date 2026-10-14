//! Repeated real Pair settlement, real fresh-source birth and accepted transport intent.
//! These bounded FullMix callback proofs leave the musical/resource/Human gates open.

use super::*;
use crate::audio_engine::cold_load;
use crate::audio_engine::pad_pair::PairAcknowledgementSnapshot;
use crate::messages::LoaderEvent;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

const NEXT_EMPTY: usize = 4;

fn assert_cached(fixture: &CallbackFixture, slot: usize, expected: Option<&SampleBuffer>) {
    let cache = fixture.control.engine.sample_cache.lock().unwrap();
    match (&cache[slot], expected) {
        (Some(actual), Some(expected)) => {
            assert!(actual.same_window(expected));
            assert!(Arc::ptr_eq(&actual.samples, &expected.samples));
        }
        (None, None) => {}
        _ => panic!("control custody changed before its own mirror"),
    }
}

/// Every invocation captures and prepares new actual participants. Earlier ACK owners
/// remain alive, and their delayed observations must not reapply an earlier projection.
fn complete_pair(
    fixture: &mut CallbackFixture,
    hold: &HoldActionTicket,
    slots: (usize, usize),
    held: &LaneEvidence,
    other_after: &[(usize, &LaneEvidence)],
    earlier: &[(PadPairTicket, PairAcknowledgementSnapshot)],
) -> (PadPairTicket, PairAcknowledgementSnapshot) {
    let (source, target) = slots;
    held.assert_retained(&fixture.mixer, source, 0);
    let before_hold = hold.action.projection();
    assert_eq!(hold.current_control_slot(), Some(source));
    assert_eq!(before_hold.state, HoldActionState::PausedOwned);
    assert!(before_hold.effect_current);
    let before_cache = {
        let cache = fixture.control.engine.sample_cache.lock().unwrap();
        [cache[source].clone(), cache[target].clone()]
    };
    let captured = fixture
        .control
        .engine
        ._capture_pad_pair(source, target)
        .unwrap();
    // The same productive preparation route as the public device-backed method,
    // with this existing callback fixture's actual mono rate/channel format.
    let ticket = control::prepare(&fixture.control.engine, &captured, RATE, 1).unwrap();
    assert!(ticket.own_acknowledgement().is_none());
    assert!(!fixture.control.engine._reconcile_pad_pair(&ticket).unwrap());
    fixture.control.engine._publish_pad_pair(&ticket).unwrap();
    assert_eq!(ticket.publication_status(), "pending");
    assert!(ticket.own_acknowledgement().is_none());
    assert_eq!(fixture.drain(), 1);
    assert!(fixture.control.consumer.is_empty());
    assert!(fixture.scheduler.is_empty());

    let receipt = ticket.native_acknowledgement().unwrap();
    let ack = ticket.own_acknowledgement().unwrap();
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(receipt.target_after.lifetime, held.key.lifetime());
    assert_eq!(
        receipt.kind,
        if before_cache[1].is_some() {
            PairPlacementKind::Swap
        } else {
            PairPlacementKind::Move
        }
    );
    let carried = receipt.holds[0].unwrap();
    assert_eq!(carried, before_hold);
    assert!(receipt.holds[1].is_none());
    assert_eq!(hold.action.projection(), before_hold);
    assert_eq!(hold.current_control_slot(), None);
    assert_cached(fixture, source, before_cache[0].as_ref());
    assert_cached(fixture, target, before_cache[1].as_ref());
    held.assert_retained(&fixture.mixer, target, 0);
    assert!(fixture.mixer.voices[held.lane].paused);
    for &(slot, other) in other_after {
        other.assert_retained(&fixture.mixer, slot, 0);
        assert!(fixture.mixer.voices[other.lane].paused);
    }
    // Native own ACK and its atomic image are deliberately observed before control
    // reconciliation. They cannot independently authorize a complete finish.
    assert!(
        !fixture
            .control
            .engine
            ._validate_pad_pair_acknowledgement(&ticket, &ack)
            .unwrap()
    );
    assert!(
        fixture
            .control
            .engine
            ._finish_pad_pair(&ticket, &ack)
            .is_err()
    );
    let reservation = fixture
        .control
        .engine
        .input_runtime_ownership
        .pair_reservation(source);
    assert_ne!(reservation, 0);
    assert_eq!(
        fixture
            .control
            .engine
            .input_runtime_ownership
            .pair_reservation(target),
        reservation
    );

    for (old_ticket, old_receipt) in earlier {
        let old_ack = old_ticket.own_acknowledgement().unwrap();
        assert_eq!(old_ticket.native_acknowledgement(), Some(*old_receipt));
        assert!(
            !fixture
                .control
                .engine
                ._validate_pad_pair_acknowledgement(old_ticket, &old_ack)
                .unwrap()
        );
        assert!(
            fixture
                .control
                .engine
                ._reconcile_pad_pair(old_ticket)
                .unwrap()
        );
        fixture
            .control
            .engine
            ._finish_pad_pair(old_ticket, &old_ack)
            .unwrap();
        assert_eq!(
            fixture
                .control
                .engine
                .input_runtime_ownership
                .pair_reservation(source),
            reservation
        );
        assert_eq!(hold.action.projection(), before_hold);
        held.assert_retained(&fixture.mixer, target, 0);
        for &(slot, other) in other_after {
            other.assert_retained(&fixture.mixer, slot, 0);
        }
        assert_cached(fixture, source, before_cache[0].as_ref());
        assert_cached(fixture, target, before_cache[1].as_ref());
    }

    fixture.finish(&ticket);
    assert_eq!(hold.current_control_slot(), Some(target));
    assert_eq!(hold.action.projection(), before_hold);
    assert_cached(fixture, source, before_cache[1].as_ref());
    assert_cached(fixture, target, before_cache[0].as_ref());
    held.assert_retained(&fixture.mixer, target, 0);
    for &(slot, other) in other_after {
        other.assert_retained(&fixture.mixer, slot, 0);
    }
    assert_eq!(ticket.native_acknowledgement(), Some(receipt));
    (ticket, receipt)
}

fn owned_scratch() -> tempfile::TempDir {
    let root = std::env::temp_dir().join("codex");
    fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("goal-r2-native-hold-reuse-")
        .tempdir_in(root)
        .unwrap()
}

fn write_equal_mono_wav(path: &Path) {
    // Exactly the held fixture's 128 mono frames, so material equality cannot
    // accidentally stand in for the fresh content/lifetime boundary.
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
    fs::write(path, bytes).unwrap();
}

fn birth_fresh_equal_source(fixture: &mut CallbackFixture, root: &Path) -> u64 {
    fixture.control.engine.unload_sample(SOURCE).unwrap();
    assert_eq!(fixture.drain(), 1);
    assert_cached(fixture, SOURCE, None);
    let source = root.join("fresh-equal.wav");
    write_equal_mono_wav(&source);
    // Real cold admission/worker/guarded source publication/Native own ACK, using
    // only the explicit mono fixture format instead of the public stereo default.
    let request = cold_load::admit_for_format(
        &fixture.control.engine,
        SOURCE,
        source.to_string_lossy().into_owned(),
        false,
        false,
        true,
        fixture.control.producer.clone(),
        1,
        RATE,
        root.join("samples"),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut success = false;
    let mut adopted_commands = 0;
    loop {
        adopted_commands += fixture.drain();
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
                    assert_eq!(id, SOURCE);
                    assert_eq!(request_id, request);
                    success = true;
                }
                LoaderEvent::Error { error, .. } => panic!("fresh source birth failed: {error}"),
                _ => {}
            }
        }
        if success
            && fixture.control.engine.cold_loading[SOURCE].load(Ordering::Acquire) == 0
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
            "fresh source birth did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(adopted_commands, 1);
    assert!(fixture.control.consumer.is_empty());
    assert!(fixture.scheduler.is_empty());
    let loaded = fixture.control.engine.sample_cache.lock().unwrap()[SOURCE]
        .clone()
        .unwrap();
    assert_eq!(loaded.channels, 1);
    assert_eq!(loaded.samples.as_ref(), &[0.25; 128]);
    let ownership = &fixture.control.engine.input_runtime_ownership;
    assert_ne!(ownership.slot_snapshot(SOURCE).unwrap().lifetime(), 0);
    assert!(
        !fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(SOURCE))
    );
    request
}

#[test]
fn actual_repeated_move_swap_one_hold_survives_delayed_mirrors_and_fresh_equal_old_slot() {
    // Declaration order keeps real cold PCM/lease owners alive until the fixture
    // has dropped; owned scratch is the only filesystem used by this proof.
    let directory = owned_scratch();
    let mut fixture = CallbackFixture::new(8);
    let hold = fixture.hold();
    assert_eq!(fixture.drain(), 1);
    let original = hold.action.projection();
    let held = LaneEvidence::capture(&fixture.mixer, SOURCE);
    let other = LaneEvidence::capture(&fixture.mixer, OTHER);
    let effect = original.effect.unwrap();
    assert_eq!(effect.key, held.key);
    assert_eq!(effect.action_id, original.action_id);
    assert_eq!(effect.cohort_mask, 1 << held.lane);
    assert_eq!(effect.changed_mask, 1 << held.lane);
    assert_eq!(effect.generations[held.lane], held.generation);
    let mut earlier = Vec::new();
    let first = complete_pair(
        &mut fixture,
        &hold,
        (SOURCE, EMPTY),
        &held,
        &[(OTHER, &other)],
        &earlier,
    );
    earlier.push(first);

    birth_fresh_equal_source(&mut fixture, directory.path());
    held.assert_retained(&fixture.mixer, EMPTY, 0);
    other.assert_retained(&fixture.mixer, OTHER, 0);
    assert_eq!(hold.action.projection(), original);
    fixture
        .control
        .engine
        .play_sample(SOURCE, 1.0, None)
        .unwrap();
    assert_eq!(fixture.drain(), 1);
    fixture.render(19);
    fixture.control.engine.pause_sample(SOURCE).unwrap();
    assert_eq!(fixture.drain(), 1);
    let fresh = LaneEvidence::capture(&fixture.mixer, SOURCE);
    assert_ne!(fresh.key, held.key);
    assert_ne!(fresh.key.lifetime(), held.key.lifetime());
    assert_eq!(fresh.sample.samples.as_ref(), held.sample.samples.as_ref());
    assert!(!Arc::ptr_eq(&fresh.sample.samples, &held.sample.samples));
    assert!(fixture.mixer.voices[fresh.lane].paused);
    held.assert_retained(&fixture.mixer, EMPTY, 0);
    other.assert_retained(&fixture.mixer, OTHER, 0);

    let second = complete_pair(
        &mut fixture,
        &hold,
        (EMPTY, OTHER),
        &held,
        &[(EMPTY, &other), (SOURCE, &fresh)],
        &earlier,
    );
    earlier.push(second);
    let third = complete_pair(
        &mut fixture,
        &hold,
        (OTHER, NEXT_EMPTY),
        &held,
        &[(EMPTY, &other), (SOURCE, &fresh)],
        &earlier,
    );
    earlier.push(third);
    let fourth = complete_pair(
        &mut fixture,
        &hold,
        (NEXT_EMPTY, EMPTY),
        &held,
        &[(NEXT_EMPTY, &other), (SOURCE, &fresh)],
        &earlier,
    );
    earlier.push(fourth);
    assert_eq!(hold.action.projection(), original);
    assert_eq!(hold.current_control_slot(), Some(EMPTY));
    assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
    assert_eq!(fixture.drain(), 1);
    assert_eq!(hold.action.state(), HoldActionState::Resumed);
    assert_eq!(hold.action.effect(), Some(effect));
    held.assert_retained(&fixture.mixer, EMPTY, 1);
    other.assert_retained(&fixture.mixer, NEXT_EMPTY, 0);
    fresh.assert_retained(&fixture.mixer, SOURCE, 0);
    assert!(!fixture.mixer.voices[held.lane].paused);
    assert!(fixture.mixer.voices[other.lane].paused);
    assert!(fixture.mixer.voices[fresh.lane].paused);
    assert_eq!(hold.current_control_slot(), None);
    let terminal = hold.action.projection();
    for _ in 0..3 {
        assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
        assert_eq!(fixture.drain(), 0);
        assert_eq!(hold.action.projection(), terminal);
        held.assert_retained(&fixture.mixer, EMPTY, 1);
        other.assert_retained(&fixture.mixer, NEXT_EMPTY, 0);
        fresh.assert_retained(&fixture.mixer, SOURCE, 0);
        for (ticket, receipt) in &earlier {
            assert_eq!(ticket.native_acknowledgement(), Some(*receipt));
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum LaterIntent {
    Stop,
    Retrigger,
    ManualPause,
    Resume,
}

impl LaterIntent {
    fn admit(self, fixture: &mut CallbackFixture, slot: usize) {
        match self {
            Self::Stop => fixture.control.engine.stop_sample(slot).unwrap(),
            Self::Retrigger => fixture.control.engine.play_sample(slot, 1.0, None).unwrap(),
            Self::ManualPause => fixture.control.engine.pause_sample(slot).unwrap(),
            Self::Resume => fixture.control.engine.resume_sample(slot).unwrap(),
        }
    }

    fn assert_applied(self, fixture: &CallbackFixture, slot: usize, held: &LaneEvidence) {
        let voice = &fixture.mixer.voices[held.lane];
        assert!(Arc::ptr_eq(&voice.history_lifetime, &held.lifetime));
        assert!(Arc::ptr_eq(voice.stretch.source_epoch_owner(), &held.epoch));
        assert_eq!(
            voice.stretch.source_epoch_owner().load(Ordering::Acquire),
            held.epoch_value + 1
        );
        assert_eq!(voice.stretch.native_state_address(), held.native);
        match self {
            Self::Stop => {
                assert!(!voice.active);
                assert!(!voice.paused);
                assert!(voice.sample.is_none());
                assert!(voice.source_admission.is_none());
                assert_eq!(voice.generation, held.generation + 1);
                assert_eq!(voice.history_lifetime.load(Ordering::Acquire), 0);
                assert_eq!(voice.stretch.pending_fifo_frames(), (0, 0));
                assert!(voice.stretch.productive_history().is_none());
                assert_eq!(voice.source_playback.position().frame, 0);
                assert_eq!(voice.source_playback.position().fraction, 0.0);
                assert!(fixture.mixer.dsp_source_history_for_test(slot).is_none());
            }
            Self::Retrigger => {
                assert!(voice.active);
                assert!(!voice.paused);
                assert_eq!(voice.sample_id, slot);
                assert_eq!(voice.generation, held.generation + 1);
                assert_eq!(
                    voice.history_lifetime.load(Ordering::Acquire),
                    voice.generation
                );
                assert_eq!(
                    voice.source_admission.as_ref().unwrap().content_key(),
                    Some(held.key)
                );
                assert!(Arc::ptr_eq(
                    &voice.sample.as_ref().unwrap().samples,
                    &held.sample.samples
                ));
                assert_eq!(voice.frame_pos, 16);
                assert_eq!(voice.source_playback.position().frame, 16);
                assert_eq!(voice.source_playback.position().fraction, 0.0);
                assert_eq!(voice.stretch.pending_fifo_frames(), (0, 0));
                assert!(voice.stretch.productive_history().is_none());
                assert_eq!(voice.source_timing.accepted, held.accepted);
                assert_eq!(
                    voice.source_timing.legacy_period_seconds,
                    held.legacy_period
                );
                assert_eq!(voice.source_timing.legacy_origin_frame, held.legacy_origin);
                assert!(fixture.mixer.dsp_source_history_for_test(slot).is_none());
            }
            Self::ManualPause | Self::Resume => {
                held.assert_retained(&fixture.mixer, slot, 1);
                assert_eq!(voice.paused, matches!(self, Self::ManualPause));
            }
        }
        assert_eq!(
            fixture
                .control
                .engine
                .input_runtime_ownership
                .slot_snapshot(slot)
                .unwrap()
                .content_key(),
            Some(held.key)
        );
        assert_cached(fixture, slot, Some(&held.sample));
    }
}

#[test]
fn actual_accepted_stop_retrigger_manual_pause_resume_after_pair_supersede_old_hold_release() {
    for target in [EMPTY, OTHER] {
        for intent in [
            LaterIntent::Stop,
            LaterIntent::Retrigger,
            LaterIntent::ManualPause,
            LaterIntent::Resume,
        ] {
            for release_before_transport_callback in [false, true] {
                let mut fixture = CallbackFixture::new(8);
                let hold = fixture.hold();
                assert_eq!(fixture.drain(), 1);
                let held = LaneEvidence::capture(&fixture.mixer, SOURCE);
                let other = LaneEvidence::capture(&fixture.mixer, OTHER);
                let effect = hold.action.effect().unwrap();
                let other_slot = if target == EMPTY { OTHER } else { SOURCE };
                let (ticket, receipt) = complete_pair(
                    &mut fixture,
                    &hold,
                    (SOURCE, target),
                    &held,
                    &[(other_slot, &other)],
                    &[],
                );
                intent.admit(&mut fixture, target);
                if release_before_transport_callback {
                    // The later real gesture is admitted first. The original release
                    // queues behind it before either callback effect/ACK is observed.
                    assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
                    assert!(hold.release_requested());
                    assert!(hold.release_admitted());
                    assert_eq!(fixture.drain(), 2);
                } else {
                    assert_eq!(fixture.drain(), 1);
                    assert_eq!(hold.action.state(), HoldActionState::Superseded);
                    intent.assert_applied(&fixture, target, &held);
                    assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
                    assert_eq!(fixture.drain(), 0);
                }
                assert_eq!(hold.action.state(), HoldActionState::Superseded);
                assert_eq!(hold.action.effect(), Some(effect));
                assert_eq!(hold.current_control_slot(), None);
                intent.assert_applied(&fixture, target, &held);
                other.assert_retained(&fixture.mixer, other_slot, 0);
                assert!(fixture.mixer.voices[other.lane].paused);
                assert_eq!(ticket.native_acknowledgement(), Some(receipt));
                let terminal = hold.action.projection();
                for _ in 0..3 {
                    assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
                    assert_eq!(fixture.drain(), 0);
                    assert_eq!(hold.action.projection(), terminal);
                    intent.assert_applied(&fixture, target, &held);
                    other.assert_retained(&fixture.mixer, other_slot, 0);
                    assert_eq!(ticket.native_acknowledgement(), Some(receipt));
                }
                assert!(fixture.control.consumer.is_empty());
                assert!(fixture.scheduler.is_empty());
            }
        }
    }
}
