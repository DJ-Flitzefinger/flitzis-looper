//! Ordinary control commands must retire an earlier committed operation's effect claim.
//! These use the public setters' exact messages on the real producer/callback path.
//! The device-free harness cannot exercise their CPAL-handle admission wrappers.

use super::*;

#[derive(Clone, Copy, Debug)]
enum Edit {
    Mode,
    Mask,
}

fn effective_all(key_lock: bool) -> (Productive, ResidentStemTicket) {
    let mut playback = Productive::new(key_lock, 0.73);
    let receipt = playback.request("all_stems", 0b1111);
    playback.capture_then_wait(&receipt);
    assert_eq!(playback.h.callback.drain(&mut playback.h.consumer), 1);
    assert_eq!(receipt.residency_status(), "accepted");
    playback.select_reference("all_stems", 0b1111);
    playback.render(257, false);
    if key_lock {
        playback.render(13, true);
        playback.adopt_native();
    }
    assert_eq!(receipt.effective_mode_status(), "effective");
    assert!(receipt.is_current());
    (playback, receipt)
}

fn ordinary_command(playback: &mut Productive, edit: Edit, restore: bool) {
    let hash = crate::audio_engine::stem_cache::source_version_hash(&playback.h.version);
    let message = match edit {
        Edit::Mode => {
            let mode = if restore {
                StemMixMode::AllStems
            } else {
                StemMixMode::FullMix
            };
            assert!(playback.reference.set_stem_mix_mode(0, mode, hash));
            ControlMessage::SetStemMixMode {
                id: 0,
                mode,
                source_version_hash: hash,
            }
        }
        Edit::Mask => {
            let enabled_stem_mask = if restore { 0b1111 } else { 0b0011 };
            assert!(
                playback
                    .reference
                    .set_stem_enabled_mask(0, enabled_stem_mask, hash)
            );
            ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask,
                source_version_hash: hash,
            }
        }
    };
    playback.h.producer.lock().unwrap().push(message).unwrap();
}

fn ordinary_edit_withdraws_effect_claim(edit: Edit, aba: bool, key_lock: bool) {
    let (mut playback, receipt) = effective_all(key_lock);
    let selection_json = playback.pair.selection_json().to_owned();
    let source = playback.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let generation = voice(&playback.h.callback.mixer).generation;
    let native = voice(&playback.h.callback.mixer)
        .stretch
        .native_state_address();
    let fifo = voice(&playback.h.callback.mixer)
        .stretch
        .pending_fifo_frames();
    let prior_playback = voice(&playback.h.callback.mixer).source_playback;
    ordinary_command(&mut playback, edit, false);
    if aba {
        // Both effective controls occur in one drain, before any source render.
        // Returning to identical fields cannot recreate the earlier operation's intent.
        ordinary_command(&mut playback, edit, true);
    }
    assert_eq!(
        receipt.effective_mode_status(),
        "effective",
        "queueing alone is not a callback effect"
    );
    assert_eq!(
        playback.h.callback.drain(&mut playback.h.consumer),
        if aba { 2 } else { 1 }
    );
    assert_eq!(voice(&playback.h.callback.mixer).generation, generation);
    assert_eq!(
        voice(&playback.h.callback.mixer)
            .stretch
            .native_state_address(),
        native
    );
    assert_eq!(
        voice(&playback.h.callback.mixer)
            .stretch
            .pending_fifo_frames(),
        fifo
    );
    assert!(
        voice(&playback.h.callback.mixer)
            .source_playback
            .matches_exact(&prior_playback)
    );
    let expected = match (edit, aba) {
        (Edit::Mode, false) => (StemMixMode::FullMix, 0b1111),
        (Edit::Mask, false) => (StemMixMode::AllStems, 0b0011),
        (_, true) => (StemMixMode::AllStems, 0b1111),
    };
    let demand = playback.h.callback.mixer.stem_demand_for_measurement(0);
    assert_eq!((demand.0, demand.1), expected);
    let mut pcm = Vec::new();
    for count in [1, 31, 127, 257] {
        pcm.extend(playback.render(count, false));
        assert_eq!(
            voice(&playback.h.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert_eq!(voice(&playback.h.callback.mixer).generation, generation);
    }
    assert!(pcm.iter().any(|sample| sample.abs() > 0.001));
    assert!(playback.selected.is_current());
    assert_eq!(playback.pair.selection_json(), selection_json);
    let current = playback.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&source.samples, &current.samples));
    assert_eq!(source.resident_binding(), current.resident_binding());
    assert_eq!(
        receipt.residency_status(),
        "accepted",
        "historical component admission remains true"
    );
    assert_eq!(
        receipt.effective_failure_reason(),
        None,
        "an ordinary intent is not a Native failure"
    );
    assert_eq!(
        receipt.effective_mode_status(),
        "rejected",
        "old receipt must not acknowledge a later ordinary intent: edit={edit:?} aba={aba} key_lock={key_lock}"
    );
    // Further unchanged polling/rendering cannot implicitly resubmit or resurrect it.
    playback.render(31, false);
    assert_eq!(receipt.effective_mode_status(), "rejected");
    assert!(playback.h.consumer.pop().is_err());
}

#[test]
fn ordinary_full_mix_withdraws_prior_effective_committed_receipt() {
    for key_lock in [false, true] {
        ordinary_edit_withdraws_effect_claim(Edit::Mode, false, key_lock);
    }
}

#[test]
fn ordinary_mask_withdraws_prior_effective_committed_receipt() {
    for key_lock in [false, true] {
        ordinary_edit_withdraws_effect_claim(Edit::Mask, false, key_lock);
    }
}

#[test]
fn ordered_ordinary_mode_aba_cannot_resurrect_prior_committed_effect_claim() {
    for key_lock in [false, true] {
        ordinary_edit_withdraws_effect_claim(Edit::Mode, true, key_lock);
    }
}

#[test]
fn ordered_ordinary_mask_aba_cannot_resurrect_prior_committed_effect_claim() {
    for key_lock in [false, true] {
        ordinary_edit_withdraws_effect_claim(Edit::Mask, true, key_lock);
    }
}

#[test]
fn rejected_and_unchanged_ordinary_controls_preserve_current_committed_effect_claim() {
    for key_lock in [false, true] {
        let (mut playback, receipt) = effective_all(key_lock);
        let hash = crate::audio_engine::stem_cache::source_version_hash(&playback.h.version);
        let wrong_hash = hash ^ 1;
        let native = voice(&playback.h.callback.mixer)
            .stretch
            .native_state_address();
        let fifo = voice(&playback.h.callback.mixer)
            .stretch
            .pending_fifo_frames();
        let source_playback = voice(&playback.h.callback.mixer).source_playback;
        let generation = voice(&playback.h.callback.mixer).generation;
        let commands = [
            ControlMessage::SetStemMixMode {
                id: 0,
                mode: StemMixMode::AllStems,
                source_version_hash: hash,
            },
            ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: 0b1111,
                source_version_hash: hash,
            },
            ControlMessage::SetStemMixMode {
                id: 0,
                mode: StemMixMode::AllStems,
                source_version_hash: wrong_hash,
            },
            ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: 0b0011,
                source_version_hash: wrong_hash,
            },
            ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: 0x80,
                source_version_hash: hash,
            },
            ControlMessage::SetStemMixMode {
                id: NUM_SAMPLES,
                mode: StemMixMode::FullMix,
                source_version_hash: 0,
            },
            ControlMessage::SetStemEnabledMask {
                id: NUM_SAMPLES,
                enabled_stem_mask: 0,
                source_version_hash: hash,
            },
        ];
        let count = commands.len();
        for command in commands {
            playback.h.producer.lock().unwrap().push(command).unwrap();
        }
        assert_eq!(playback.h.callback.drain(&mut playback.h.consumer), count);
        assert_eq!(receipt.effective_mode_status(), "effective");
        assert_eq!(receipt.residency_status(), "accepted");
        assert!(receipt.is_current());
        assert_eq!(receipt.effective_failure_reason(), None);
        assert_eq!(
            playback.h.callback.mixer.stem_demand_for_measurement(0),
            (StemMixMode::AllStems, 0b1111, hash)
        );
        assert_eq!(
            voice(&playback.h.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert_eq!(
            voice(&playback.h.callback.mixer)
                .stretch
                .pending_fifo_frames(),
            fifo
        );
        assert!(
            voice(&playback.h.callback.mixer)
                .source_playback
                .matches_exact(&source_playback)
        );
        assert_eq!(voice(&playback.h.callback.mixer).generation, generation);
        let pcm = playback.render(257, false);
        assert!(pcm.iter().any(|sample| sample.abs() > 0.001));
        assert_eq!(receipt.effective_mode_status(), "effective");
        assert!(playback.selected.is_current());
    }
}

fn effective_full_with_preloaded_components(key_lock: bool) -> (Productive, ResidentStemTicket) {
    let (mut playback, _previous) = effective_all(key_lock);
    // Admit the real independent preload interest before the explicit FullMix
    // operation. Its callback ACK, rather than a fabricated field, retains PCM.
    let preload = playback
        .h
        .engine
        .request_committed_stem_residency_with_interest_and_producer(
            &playback.selected,
            "full_mix",
            0b1111,
            "preload",
            &playback.h.producer,
        )
        .unwrap();
    assert_eq!(preload.effective_mode_status(), "not_requested");
    assert_eq!(playback.h.callback.drain(&mut playback.h.consumer), 1);
    wait_until(|| {
        (playback.h.consumer.peek().is_ok() || preload.error().is_some())
            && playback.h.producer.try_lock().is_ok()
    });
    assert_eq!(
        preload.residency_status(),
        "pending",
        "{:?}",
        preload.error()
    );
    assert_eq!(playback.h.callback.drain(&mut playback.h.consumer), 1);
    assert_eq!(
        preload.residency_status(),
        "accepted",
        "{:?}",
        preload.error()
    );
    reconcile(&playback.h.engine).unwrap();
    let full = playback.request("full_mix", 0b1111);
    playback.capture_then_wait(&full);
    assert_eq!(playback.h.callback.drain(&mut playback.h.consumer), 1);
    playback.select_reference("full_mix", 0b1111);
    playback.render(257, false);
    if key_lock {
        playback.render(13, true);
        playback.adopt_native();
    }
    assert_eq!(full.residency_status(), "accepted");
    assert_eq!(
        full.effective_mode_status(),
        "effective",
        "{:?}",
        full.error()
    );
    assert!(full.is_current());
    assert!(playback.h.callback.mixer.stems_for_measurement()[0].is_some());
    assert_eq!(
        playback.h.callback.mixer.stem_demand_for_measurement(0),
        (StemMixMode::FullMix, 0b1111, 0)
    );
    (playback, full)
}

fn full_mix_mask_control_changes(aba: bool, key_lock: bool, rejected_and_unchanged: bool) {
    let (mut playback, receipt) = effective_full_with_preloaded_components(key_lock);
    let hash = crate::audio_engine::stem_cache::source_version_hash(&playback.h.version);
    let source = playback.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let selection_json = playback.pair.selection_json().to_owned();
    let generation = voice(&playback.h.callback.mixer).generation;
    let native = voice(&playback.h.callback.mixer)
        .stretch
        .native_state_address();
    let fifo = voice(&playback.h.callback.mixer)
        .stretch
        .pending_fifo_frames();
    let prior_playback = voice(&playback.h.callback.mixer).source_playback;
    let mut count = 0;
    if rejected_and_unchanged {
        let commands = [
            ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: 0b1111,
                source_version_hash: hash,
            },
            ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: 0b0011,
                source_version_hash: hash ^ 1,
            },
            ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: 0x80,
                source_version_hash: hash,
            },
            ControlMessage::SetStemMixMode {
                id: 0,
                mode: StemMixMode::FullMix,
                source_version_hash: 0,
            },
        ];
        count = commands.len();
        for command in commands {
            playback.h.producer.lock().unwrap().push(command).unwrap();
        }
    } else {
        ordinary_command(&mut playback, Edit::Mask, false);
        count += 1;
        if aba {
            ordinary_command(&mut playback, Edit::Mask, true);
            count += 1;
        }
    }
    assert_eq!(receipt.effective_mode_status(), "effective");
    assert_eq!(playback.h.callback.drain(&mut playback.h.consumer), count);
    let mask = if aba || rejected_and_unchanged {
        0b1111
    } else {
        0b0011
    };
    assert_eq!(
        playback.h.callback.mixer.stem_demand_for_measurement(0),
        (StemMixMode::FullMix, mask, 0)
    );
    assert_eq!(voice(&playback.h.callback.mixer).generation, generation);
    assert_eq!(
        voice(&playback.h.callback.mixer)
            .stretch
            .native_state_address(),
        native
    );
    assert_eq!(
        voice(&playback.h.callback.mixer)
            .stretch
            .pending_fifo_frames(),
        fifo
    );
    assert!(
        voice(&playback.h.callback.mixer)
            .source_playback
            .matches_exact(&prior_playback)
    );
    let mut pcm = Vec::new();
    for frames in [1, 31, 127, 257] {
        pcm.extend(playback.render(frames, false));
        assert_eq!(
            voice(&playback.h.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert_eq!(voice(&playback.h.callback.mixer).generation, generation);
    }
    assert!(pcm.iter().any(|sample| sample.abs() > 0.001));
    assert!(playback.selected.is_current());
    assert_eq!(playback.pair.selection_json(), selection_json);
    let current = playback.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&source.samples, &current.samples));
    assert_eq!(source.resident_binding(), current.resident_binding());
    assert!(playback.h.callback.mixer.stems_for_measurement()[0].is_some());
    assert_eq!(receipt.residency_status(), "accepted");
    assert_eq!(receipt.effective_failure_reason(), None);
    let expected = if rejected_and_unchanged {
        "effective"
    } else {
        "rejected"
    };
    assert_eq!(
        receipt.effective_mode_status(),
        expected,
        "FullMix mask intent still belongs to the committed receipt: aba={aba} key_lock={key_lock}"
    );
    playback.render(31, false);
    assert_eq!(receipt.effective_mode_status(), expected);
    assert!(playback.h.consumer.pop().is_err());
}

#[test]
fn ordinary_mask_withdraws_effective_full_mix_receipt_with_real_preload_retention() {
    for key_lock in [false, true] {
        full_mix_mask_control_changes(false, key_lock, false);
    }
}

#[test]
fn ordered_ordinary_mask_aba_cannot_resurrect_preloaded_full_mix_effect_claim() {
    for key_lock in [false, true] {
        full_mix_mask_control_changes(true, key_lock, false);
    }
}

#[test]
fn rejected_and_unchanged_mask_controls_preserve_preloaded_full_mix_effect_claim() {
    for key_lock in [false, true] {
        full_mix_mask_control_changes(false, key_lock, true);
    }
}
