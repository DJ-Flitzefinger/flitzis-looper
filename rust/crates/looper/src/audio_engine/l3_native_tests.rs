//! New complete-set adoption reads genuine distinct files through own Capture/Adopt.
//! Reordering two components leaves the full original unchanged. The complete V1
//! oracle therefore supplies an independent exact PCM expectation by permuting its
//! selected mask; it never installs V2 or borrows the transaction under test.
use super::*;

fn reordered_mask(mask: u8) -> u8 {
    (mask & 0b0110) | ((mask & 0b0001) << 3) | ((mask & 0b1000) >> 3)
}

fn candidate(p: &mut Productive, serial: usize) -> (PreparedStemPair, CommittedStemSetTicket) {
    let generation = format!("{serial:032x}");
    let reference = p.h.wav_reference.replace(GENERATION, &generation);
    let path = p.h.root.parent().unwrap().join(&reference);
    write_signal_set(&path, p.h.material.sample.frame_count(), &p.h.version, 0);
    let vocals = fs::read(path.join("vocals.wav")).unwrap();
    let drums = fs::read(path.join("drums.wav")).unwrap();
    fs::write(path.join("vocals.wav"), drums).unwrap();
    fs::write(path.join("drums.wav"), vocals).unwrap();
    fs::write(
        path.join("instrumental.wav"),
        signal_sum_wav(p.h.material.sample.frame_count(), &[0, 1, 2]),
    )
    .unwrap();
    let hashes: serde_json::Map<_, _> = STEM_FILE_NAMES
        .iter()
        .map(|name| {
            let content = fs::read(path.join(format!("{name}.wav"))).unwrap();
            (
                (*name).to_owned(),
                json!(format!("{:x}", Sha256::digest(content))),
            )
        })
        .collect();
    fs::write(
        path.join(".complete.json"),
        serde_json::to_vec(&json!({
            "schema": "stem-set-sha256-v1", "source_version": p.h.version, "stems": hashes,
        }))
        .unwrap(),
    )
    .unwrap();
    let source = p.h.ticket(0);
    let pair =
        p.h.engine
            .prepare_stem_pair_at_root(&p.h.root, 0, &p.h.version, &reference, &source, false, None)
            .unwrap();
    assert_ne!(selection(&pair), selection(&p.pair));
    assert_eq!(
        pair.pair_reader.descriptor.content.alignment.offset_frames,
        0
    );
    assert!(!pair.has_components());
    pair.select();
    let ticket =
        p.h.engine
            .register_pending_committed_stem_pair_with_producer(&pair, &source, &p.h.producer)
            .unwrap();
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(ticket.is_current());
    (pair, ticket)
}

fn request_candidate(
    p: &mut Productive,
    ticket: &CommittedStemSetTicket,
    mask: u8,
) -> ResidentStemTicket {
    p.h.engine
        .request_committed_stem_residency_with_producer(ticket, "all_stems", mask, &p.h.producer)
        .unwrap()
}

fn settle(p: &mut Productive, receipt: &ResidentStemTicket) {
    let native = voice(&p.h.callback.mixer).stretch.native_state_address();
    let mut remaining =
        (128.0 / voice(&p.h.callback.mixer).source_playback.tempo_ratio()).ceil() as usize;
    for count in [1_usize, 31, 17, 7].into_iter().cycle() {
        let count = count.min(remaining);
        p.render(count, false);
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.native_state_address(),
            native
        );
        remaining -= count;
        if remaining == 0 {
            break;
        }
    }
    assert!(
        !p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    let output = p.render(777, false);
    assert!(output.iter().any(|value| value.abs() > 0.001));
    if p.key_lock {
        p.render(13, true);
        p.adopt_native();
    }
    assert_eq!(
        receipt.effective_mode_status(),
        "effective",
        "{:?}",
        receipt.error()
    );
}

fn make_v1_effective(p: &mut Productive) {
    let receipt = p.request("all_stems", 0b1010);
    p.capture_then_wait(&receipt);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    p.select_reference("all_stems", 0b1010);
    settle(p, &receipt);
}

#[test]
fn live_candidate_first_and_replacement_keep_actual_native_fifo_and_two_exact_readers() {
    for key_lock in [false, true] {
        for ratio in [0.73, 1.371_234_567_890_123] {
            for selected_v1 in [false, true] {
                let mut p = Productive::new(key_lock, ratio);
                if selected_v1 {
                    make_v1_effective(&mut p);
                }
                let before = voice(&p.h.callback.mixer).source_playback;
                let generation = voice(&p.h.callback.mixer).generation;
                let native = voice(&p.h.callback.mixer).stretch.native_state_address();
                let old_identity = p.h.callback.mixer.stems_for_measurement()[0]
                    .as_ref()
                    .map(|set| set.complete_set_identity.clone());
                let (_pair, selected) = candidate(&mut p, 101);
                assert!(
                    p.selected.is_current(),
                    "registration preserves V1 authority"
                );
                assert!(
                    voice(&p.h.callback.mixer)
                        .source_playback
                        .matches_exact(&before)
                );
                assert_eq!(
                    voice(&p.h.callback.mixer).stretch.native_state_address(),
                    native
                );
                let receipt = request_candidate(&mut p, &selected, 0b1010);
                p.capture_then_wait(&receipt);
                assert!(
                    p.selected.is_current(),
                    "preparation preserves V1 authority"
                );
                p.render(31, false);
                let at_adopt = voice(&p.h.callback.mixer).source_playback;
                let at_adopt_fifos = voice(&p.h.callback.mixer).stretch.pending_fifo_frames();
                assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
                assert_eq!(
                    receipt.residency_status(),
                    "accepted",
                    "{:?}",
                    receipt.error()
                );
                assert!(receipt.claimed());
                assert!(!receipt.withdraw());
                assert!(selected.is_current());
                assert!(!p.selected.is_current());
                assert!(
                    voice(&p.h.callback.mixer)
                        .source_playback
                        .matches_exact(&at_adopt)
                );
                assert_eq!(voice(&p.h.callback.mixer).generation, generation);
                assert_eq!(
                    voice(&p.h.callback.mixer).stretch.native_state_address(),
                    native
                );
                assert_eq!(
                    voice(&p.h.callback.mixer).stretch.pending_fifo_frames(),
                    at_adopt_fifos
                );
                let new_identity = p.h.callback.mixer.stems_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .complete_set_identity
                    .clone();
                if let Some(old) = old_identity {
                    assert!(!Arc::ptr_eq(&old, &new_identity));
                }
                // V2 files have permuted components; the independent complete V1
                // oracle changes only its mask, retaining all real Native history.
                p.select_reference("all_stems", reordered_mask(0b1010));
                p.selected = selected;
                settle(&mut p, &receipt);
                // Later ordinary FullMix -> ALL uses the accepted V2 ticket and
                // exercises a fresh own preparation rather than reusing its ACK.
                let full = p.request("full_mix", 0b1010);
                assert!(
                    !receipt.is_current(),
                    "later ordinary intent retires only V2's old receipt"
                );
                assert!(
                    p.selected.is_current(),
                    "accepted V2 selection remains reusable"
                );
                p.capture_then_wait(&full);
                assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
                p.select_reference("full_mix", 0b1010);
                settle(&mut p, &full);
                let again = p.request("all_stems", 0b1010);
                p.capture_then_wait(&again);
                assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
                p.select_reference("all_stems", reordered_mask(0b1010));
                settle(&mut p, &again);
            }
        }
    }
}

#[test]
fn paused_candidate_claim_waits_for_consumption_and_defers_a_third_owner() {
    for key_lock in [false, true] {
        let mut p = Productive::new(key_lock, 0.73);
        make_v1_effective(&mut p);
        p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
        p.reference.pause_sample_at_output_frame(0, p.frame);
        let (_pair, selected) = candidate(&mut p, 102);
        let receipt = request_candidate(&mut p, &selected, 0b1010);
        p.capture_then_wait(&receipt);
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        p.select_reference("all_stems", reordered_mask(0b1010));
        p.selected = selected;
        assert_eq!(receipt.effective_mode_status(), "transitioning");
        assert!(receipt.claimed());
        assert!(!receipt.withdraw());
        for frames in [1, 127, 384] {
            p.render_paused(frames);
            assert_eq!(receipt.effective_mode_status(), "transitioning");
        }
        let source = p.h.ticket(0);
        assert!(p.h.engine.register_pending_committed_stem_pair_with_producer(
            &p.pair, &source, &p.h.producer,
        ).is_err(), "a later candidate cannot occupy V1's outgoing slot");
        assert!(p.selected.is_current() && receipt.is_current());
        p.h.callback.mixer.resume_sample_at_output_frame(0, p.frame);
        p.reference.resume_sample_at_output_frame(0, p.frame);
        settle(&mut p, &receipt);
    }
}

#[test]
fn stopped_candidate_is_control_ready_and_ordinary_v2_modes_need_their_own_ack() {
    let mut p = Productive::new(false, 0.73);
    p.h.callback.mixer.stop_sample(0);
    p.reference.stop_sample(0);
    let (_pair, selected) = candidate(&mut p, 103);
    let receipt = request_candidate(&mut p, &selected, 0b1010);
    p.capture_then_wait(&receipt);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(receipt.residency_status(), "accepted");
    assert_eq!(receipt.effective_mode_status(), "waiting");
    assert!(receipt.claimed() && !receipt.withdraw());
    assert!(selected.is_current());
    assert!(!p.selected.is_current());
    p.selected = selected;
    for mode in ["full_mix", "all_stems"] {
        let mode_receipt = p.request(mode, 0b1010);
        assert_eq!(mode_receipt.residency_status(), "captured");
        p.capture_then_wait(&mode_receipt);
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        assert_eq!(mode_receipt.residency_status(), "accepted");
        assert_eq!(mode_receipt.effective_mode_status(), "waiting");
    }
}

#[test]
fn preclaim_withdrawal_and_peak_rejection_preserve_selected_v1_control_and_audio() {
    for reject_peak in [false, true] {
        let mut p = Productive::new(false, 0.73);
        make_v1_effective(&mut p);
        let old = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity
            .clone();
        let (_pair, selected) = candidate(&mut p, 104);
        if reject_peak {
            p.h.engine.resident_stem_cache.lock().unwrap()[0]
                .committed
                .pcm_limit_for_test = Some(1);
        }
        let receipt = request_candidate(&mut p, &selected, 0b1010);
        if reject_peak {
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            wait_until(|| receipt.error().is_some());
            assert_eq!(receipt.residency_status(), "rejected");
        } else {
            p.capture_then_wait(&receipt);
            assert!(receipt.withdraw());
            assert!(!receipt.claimed());
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            assert_eq!(receipt.residency_status(), "rejected");
        }
        assert!(p.selected.is_current());
        assert!(Arc::ptr_eq(
            &old,
            &p.h.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap()
                .complete_set_identity
        ));
        p.render(257, false);
        p.h.engine.resident_stem_cache.lock().unwrap()[0]
            .committed
            .pcm_limit_for_test = None;
        let full = p.request("full_mix", 0b1010);
        p.capture_then_wait(&full);
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        p.select_reference("full_mix", 0b1010);
        settle(&mut p, &full);
        assert!(
            !selected.is_current(),
            "newer FullMix intent withdraws future candidate ALL"
        );
    }
}

#[test]
fn empty_candidate_mask_keeps_exact_reader_authority_before_a_changed_mask() {
    let mut p = Productive::new(false, 0.73);
    let (_pair, selected) = candidate(&mut p, 105);
    let empty = request_candidate(&mut p, &selected, 0);
    p.capture_then_wait(&empty);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(empty.residency_status(), "accepted");
    p.select_reference("all_stems", 0);
    p.selected = selected;
    p.render(257, false);
    assert_eq!(empty.effective_mode_status(), "effective");
    // The source feed is zero, while the real isolator is allowed to drain its
    // retained filter history. Exact output equality above and below proves we
    // did not reset that audible history merely to obtain instant silence.
    p.render(777, false);
    p.render(127, false);
    assert!(
        voice(&p.h.callback.mixer)
            .stretch
            .varispeed_buffers()
            .iter()
            .all(|channel| channel[..127].iter().all(|sample| *sample == 0.0))
    );
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_some());
    let changed = p.request("all_stems", 0b1010);
    assert!(!empty.is_current());
    p.capture_then_wait(&changed);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    p.select_reference("all_stems", reordered_mask(0b1010));
    settle(&mut p, &changed);
}

#[test]
fn actual_predecessor_change_after_capture_rejects_candidate_and_keeps_v1_usable() {
    let mut p = Productive::new(false, 0.73);
    make_v1_effective(&mut p);
    let old = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .complete_set_identity
        .clone();
    let (_pair, selected) = candidate(&mut p, 106);
    let candidate_receipt = request_candidate(&mut p, &selected, 0b1010);
    p.capture_then_wait(&candidate_receipt);
    let hash = crate::audio_engine::stem_cache::source_version_hash(&p.h.version);
    assert!(p.h.callback.mixer.set_stem_enabled_mask(0, 0b0101, hash));
    p.select_reference("all_stems", 0b0101);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(candidate_receipt.residency_status(), "rejected");
    assert!(!candidate_receipt.claimed());
    assert!(p.selected.is_current());
    assert!(Arc::ptr_eq(
        &old,
        &p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity
    ));
    p.render(257, false);
    let full = p.request("full_mix", 0b0101);
    p.capture_then_wait(&full);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    p.select_reference("full_mix", 0b0101);
    settle(&mut p, &full);
}

#[test]
fn candidate_queue_pressure_preserves_v1_intent_and_releases_every_job_reservation() {
    let mut p = Productive::new(false, 0.73);
    make_v1_effective(&mut p);
    let old_identity = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .complete_set_identity
        .clone();
    let before = voice(&p.h.callback.mixer).source_playback;
    let intent = p.h.engine.resident_stem_cache.lock().unwrap()[0]
        .intent_epoch
        .load(Ordering::Acquire);
    let (_pair, selected) = candidate(&mut p, 107);
    let (mut full_producer, mut full_consumer) = rtrb::RingBuffer::new(1);
    full_producer.push(ControlMessage::Ping()).unwrap();
    let full_producer = Arc::new(Mutex::new(full_producer));
    // More than the 32-job cap proves every failed queue admission returns its
    // cold-lane reservation without mutating the predecessor's control intent.
    for _ in 0..64 {
        assert!(
            p.h.engine
                .request_committed_stem_residency_with_producer(
                    &selected,
                    "all_stems",
                    0b1010,
                    &full_producer,
                )
                .is_err()
        );
    }
    assert!(matches!(
        full_consumer.pop().unwrap(),
        ControlMessage::Ping()
    ));
    assert!(full_consumer.pop().is_err());
    assert_eq!(
        intent,
        p.h.engine.resident_stem_cache.lock().unwrap()[0]
            .intent_epoch
            .load(Ordering::Acquire)
    );
    assert!(
        !p.h.engine
            .input_runtime_ownership
            .resident_control_pending(0)
    );
    assert!(p.selected.is_current() && selected.is_current());
    assert!(before.matches_exact(&voice(&p.h.callback.mixer).source_playback));
    assert!(Arc::ptr_eq(
        &old_identity,
        &p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity
    ));
    let actual_retry = request_candidate(&mut p, &selected, 0b1010);
    p.capture_then_wait(&actual_retry);
    assert!(actual_retry.withdraw());
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(actual_retry.residency_status(), "rejected");
    assert!(p.selected.is_current());
    p.render(257, false);
}

#[test]
fn candidate_retirement_pressure_defers_adopt_and_preclaim_withdrawal_preserves_v1() {
    let mut p = Productive::new(false, 0.73);
    make_v1_effective(&mut p);
    let old = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .complete_set_identity
        .clone();
    let (_pair, selected) = candidate(&mut p, 108);
    let receipt = request_candidate(&mut p, &selected, 0b1010);
    p.capture_then_wait(&receipt);
    let source = p.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let playback = voice(&p.h.callback.mixer).source_playback;
    let native = voice(&p.h.callback.mixer).stretch.native_state_address();
    let fifos = voice(&p.h.callback.mixer).stretch.pending_fifo_frames();
    p.h.callback.retirement.slots = 2;
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 0);
    assert_eq!(receipt.residency_status(), "pending");
    assert!(!receipt.claimed());
    assert!(p.h.consumer.peek().is_ok());
    assert!(p.selected.is_current());
    assert!(Arc::ptr_eq(
        &old,
        &p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity
    ));
    assert!(receipt.withdraw());
    p.h.callback.retirement.slots = usize::MAX;
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(receipt.residency_status(), "rejected");
    assert!(Arc::ptr_eq(
        &source.samples,
        &p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    assert!(playback.matches_exact(&voice(&p.h.callback.mixer).source_playback));
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.pending_fifo_frames(),
        fifos
    );
    assert!(p.selected.is_current());
    p.render(257, false);
}

#[test]
fn candidate_captured_voice_or_history_change_rejects_without_replacing_v1_readers() {
    for history_change in [false, true] {
        let mut p = Productive::new(false, 0.73);
        make_v1_effective(&mut p);
        let old = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity
            .clone();
        let (_pair, selected) = candidate(&mut p, 109);
        let launches = if history_change {
            None
        } else {
            let callback = p.h.callback.as_mut();
            let actual = callback
                .mixer
                .prepare_play_sample_rt(0, 1.0, p.frame, &mut callback.retirement)
                .unwrap();
            let expected = p
                .reference
                .prepare_play_sample_rt(
                    0,
                    1.0,
                    p.frame,
                    &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
                )
                .unwrap();
            Some((actual, expected))
        };
        let receipt = request_candidate(&mut p, &selected, 0b1010);
        p.capture_then_wait(&receipt);
        if history_change {
            voice_mut(&mut p.h.callback.mixer)
                .stretch
                .invalidate_prepared();
            voice_mut(&mut p.reference).stretch.invalidate_prepared();
        } else {
            let (actual, expected) = launches.unwrap();
            let callback = p.h.callback.as_mut();
            assert!(
                callback
                    .mixer
                    .play_prepared_sample_rt(actual, &mut callback.retirement)
            );
            assert!(p.reference.play_prepared_sample_rt(
                expected,
                &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
            ));
        }
        // The explicit race owns the changed voice/history. Candidate rejection
        // must preserve that actual post-race state, rather than restore the pin.
        let generation = voice(&p.h.callback.mixer).generation;
        let playback = voice(&p.h.callback.mixer).source_playback;
        let native = voice(&p.h.callback.mixer).stretch.native_state_address();
        let fifos = voice(&p.h.callback.mixer).stretch.pending_fifo_frames();
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        assert_eq!(receipt.residency_status(), "rejected");
        assert!(!receipt.claimed());
        assert!(Arc::ptr_eq(
            &old,
            &p.h.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap()
                .complete_set_identity
        ));
        assert_eq!(voice(&p.h.callback.mixer).generation, generation);
        assert!(playback.matches_exact(&voice(&p.h.callback.mixer).source_playback));
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.native_state_address(),
            native
        );
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.pending_fifo_frames(),
            fifos
        );
        assert!(p.selected.is_current());
        p.render(257, false);
    }
}

#[test]
fn candidate_source_or_timing_authority_change_rejects_without_mutating_v1_pcm() {
    for source_change in [false, true] {
        let mut p = Productive::new(false, 0.73);
        make_v1_effective(&mut p);
        let old = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .clone();
        let sample = p.h.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        let (_pair, selected) = candidate(&mut p, 110);
        let receipt = request_candidate(&mut p, &selected, 0b1010);
        p.capture_then_wait(&receipt);
        if source_change {
            // Real current source authority is revoked. Held V1 PCM remains an
            // immutable reader; neither its ticket nor old-source output is claimed current.
            p.h.engine.input_runtime_ownership.revoke_source(0);
        } else {
            let (mut parameters, mut parameter_consumer) = rtrb::RingBuffer::new(1);
            push_preparation_epoch_message(
                &mut parameters,
                ControlParameterMessage::SetPadBpm {
                    id: 0,
                    bpm: Some(127.0),
                },
                &p.h.engine.prepared_source_epochs[0],
                "SetPadBpm",
            )
            .unwrap();
            let callback = p.h.callback.as_mut();
            crate::audio_engine::audio_stream::drain_parameter_messages(
                &mut parameter_consumer,
                &mut callback.mixer,
                &mut callback.transport,
            );
            assert!(parameter_consumer.is_empty());
        }
        assert!(p.h.callback.drain(&mut p.h.consumer) >= 1);
        assert_eq!(receipt.residency_status(), "rejected");
        assert!(!receipt.is_current() && !receipt.claimed());
        let current = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(
            &old.complete_set_identity,
            &current.complete_set_identity
        ));
        for (before, after) in old.stems.iter().zip(&current.stems) {
            assert!(Arc::ptr_eq(&before.samples, &after.samples));
        }
        assert!(Arc::ptr_eq(
            &sample.samples,
            &p.h.callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap()
                .samples
        ));
        if source_change {
            assert!(!p.selected.is_current());
        }
        // No old timing/source output oracle is applied after this authority race.
    }
}

#[test]
fn candidate_new_window_ack_wins_and_keeps_the_actual_v1_owner() {
    let mut p = Productive::new(false, 0.73);
    make_v1_effective(&mut p);
    let old = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .complete_set_identity
        .clone();
    let old_window = p.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let (_pair, selected) = candidate(&mut p, 111);
    let receipt = request_candidate(&mut p, &selected, 0b1010);
    p.capture_then_wait(&receipt);
    let refresh = prepare_window_with_producer(
        &p.h.engine,
        0,
        WindowRequest {
            storage_range: Some((
                (LOOP_START - 1) as f64 / f64::from(RATE),
                (LOOP_END + 1) as f64 / f64::from(RATE),
            )),
            ..WindowRequest::default()
        },
        p.h.producer.clone(),
    )
    .unwrap();
    wait_until(|| refresh.publication_status() == "pending" || refresh.error().unwrap().is_some());
    assert_eq!(
        refresh.publication_status(),
        "pending",
        "{:?}",
        refresh.error()
    );
    assert!(p.h.callback.drain(&mut p.h.consumer) >= 2);
    assert_eq!(refresh.publication_status(), "accepted");
    assert_eq!(receipt.residency_status(), "rejected");
    assert!(!receipt.claimed());
    assert!(p.selected.is_current());
    assert!(Arc::ptr_eq(
        &old,
        &p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity
    ));
    let current_window = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_ne!(
        old_window.resident_binding(),
        current_window.resident_binding()
    );
    assert_eq!(current_window.resident_start(), LOOP_START - 1);
    assert_eq!(current_window.resident_end(), LOOP_END + 1);
    // Storage changed, but the same original, V1 selection and finite loop knots
    // remain the complete oracle's actual authority.
    p.render(257, false);
}

#[test]
fn candidate_full_track_callback_bank_rejects_first_and_replacement_without_claim() {
    for stopped in [false, true] {
        for selected_v1 in [false, true] {
            let mut p = Productive::new(false, 0.73);
            if selected_v1 {
                make_v1_effective(&mut p);
            }
            if stopped {
                p.h.callback.mixer.stop_sample(0);
                p.reference.stop_sample(0);
            }
            let expanded = prepare_window_with_producer(
                &p.h.engine,
                0,
                WindowRequest {
                    storage_range: Some((
                        0.0,
                        p.h.material.sample.frame_count() as f64 / f64::from(RATE),
                    )),
                    ..WindowRequest::default()
                },
                p.h.producer.clone(),
            )
            .unwrap();
            wait_until(|| {
                expanded.publication_status() == "pending" || expanded.error().unwrap().is_some()
            });
            assert_eq!(
                expanded.publication_status(),
                "pending",
                "{:?}",
                expanded.error()
            );
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            assert_eq!(expanded.publication_status(), "accepted");
            assert_eq!(
                p.h.callback.mixer.bank_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .residency
                    .as_ref()
                    .unwrap()
                    .context,
                crate::messages::ResidentContext::FullTrack
            );
            let old = p.h.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .map(|set| set.complete_set_identity.clone());
            let source = p.h.callback.mixer.bank_for_measurement()[0]
                .clone()
                .unwrap();
            let before = (!stopped).then(|| voice(&p.h.callback.mixer).source_playback);
            let (_pair, selected) = candidate(&mut p, 112);
            let receipt = request_candidate(&mut p, &selected, 0b1010);
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            assert_eq!(receipt.residency_status(), "rejected");
            assert!(!receipt.claimed());
            assert!(p.selected.is_current());
            if let Some(before) = before {
                assert!(before.matches_exact(&voice(&p.h.callback.mixer).source_playback));
            } else {
                // No active old window/voice can explain this rejection: it proves
                // the explicit candidate FullTrack guard on the actual current bank.
                assert!(
                    p.h.callback
                        .mixer
                        .voices
                        .iter()
                        .all(|voice| !voice.is_playing_sample(0))
                );
            }
            assert!(Arc::ptr_eq(
                &source.samples,
                &p.h.callback.mixer.bank_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .samples
            ));
            match (
                old.as_ref(),
                p.h.callback.mixer.stems_for_measurement()[0].as_ref(),
            ) {
                (Some(old), Some(current)) => {
                    assert!(Arc::ptr_eq(old, &current.complete_set_identity))
                }
                (None, None) => {}
                _ => panic!("rejected full-track candidate changed the predecessor reader"),
            }
            if !stopped {
                p.render(257, false);
            }
        }
    }
}
