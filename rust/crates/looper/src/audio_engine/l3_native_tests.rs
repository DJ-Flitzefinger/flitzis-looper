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
    let binding = crate::audio_engine::input_runtime_binding::capture(&p.h.engine, 0)
        .unwrap()
        .unwrap();
    assert!(binding.current());
    assert_eq!(
        source.publication.accepted_projection(),
        binding.binding.accepted
    );
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

// The usual 4096-frame fixture cannot contain the 24 whole-source positions
// required for supported 120-BPM evidence, even with explicit 1/64-quarter
// labels. This larger source changes only this test's immutable PCM geometry;
// its admitted callback bank still owns the same proper finite NormalLoop.
pub(super) fn bpm_harness() -> Harness {
    let mut h = Harness::new();
    let original = h.root.join("l3-bpm-residency.wav");
    fs::write(&original, signal_sum_wav(16_384, &[0, 1, 2, 3])).unwrap();
    let material = prepare_material(&h.root, &original, RATE, 2, &|| false).unwrap();
    let version = canonical_version(&material);
    let wav_reference = canonical_wav_reference(&material);
    let wav_path = h.root.parent().unwrap().join(&wav_reference);
    write_signal_set(&wav_path, material.sample.frame_count(), &version, 0);
    let preparation = control::prepared_for_test(
        &h.engine,
        PreparedMigrationMaterial::from_current(
            &h.root,
            material.sample.clone(),
            material.lease.clone(),
        )
        .unwrap(),
        &h.root,
    )
    .unwrap();
    let requests: [_; 2] = [0, 215].map(|id| {
        control::adopt_for_format(
            &h.engine,
            id,
            &preparation,
            h.producer.clone(),
            (2, RATE, h.root.clone()),
        )
        .unwrap()
    });
    wait_until(|| h.consumer.slots() >= 2);
    assert_eq!(h.callback.drain(&mut h.consumer), 2);
    source_successes(
        &h.engine,
        &[requests[0].request_id(), requests[1].request_id()],
    );
    preparation.release_preparation().unwrap();
    h._original_owner = h
        .engine
        .project_assets
        .acquire(&h.root, &material.lease.original_path)
        .unwrap();
    h.material = material;
    h.version = version;
    h.wav_reference = wav_reference;
    h.wav_path = wav_path;
    h
}

fn master_bpm_through_callback(h: &mut Harness, bpm: f64) {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    crate::audio_engine::push_global_timing_message(
        &h.engine.global_timing_revision,
        &mut producer,
        ControlParameterMessage::SetMasterBpm(bpm),
        "SetMasterBpm",
    )
    .unwrap();
    let callback = h.callback.as_mut();
    crate::audio_engine::audio_stream::drain_parameter_messages(
        &mut consumer,
        &mut callback.mixer,
        &mut callback.transport,
    );
    assert!(consumer.is_empty());
}

pub(super) fn accepted_bpm_timing(
    h: &mut Harness,
) -> crate::audio_engine::constant_timing::AcceptedTimingProjection {
    use crate::audio_engine::constant_timing;
    use crate::audio_engine::input_runtime_binding;
    use flitzis_looper_analysis::tempo_acceptance::TimingIntent;

    constant_timing::set_intent(&h.engine, &h.producer, 0, TimingIntent::Automatic).unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    let timing = constant_timing::l3_constant_timing_fixture::publish_synthetic_loaded_timing(
        &h.engine,
        &h.producer,
        0,
        60.0 / 119.999,
    );
    assert_eq!(timing.publication_status().unwrap(), "pending");
    let before_ack = input_runtime_binding::capture(&h.engine, 0)
        .unwrap()
        .unwrap();
    assert_eq!(before_ack.binding.accepted, None);
    assert!(before_ack.current());
    assert!(!before_ack.available());
    assert!(
        h.engine
            .capture_prepared_source(0, h.version.clone())
            .is_err()
    );
    Python::attach(|py| {
        assert!(
            constant_timing::current_metadata(&h.engine, py, 0)
                .unwrap()
                .is_none()
        );
    });
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(timing.publication_status().unwrap(), "accepted");
    let current = input_runtime_binding::capture(&h.engine, 0)
        .unwrap()
        .unwrap();
    assert!(current.current());
    let projection = current.binding.accepted.unwrap();
    assert!((60.0 / projection.period_seconds - 119.999).abs() < 1e-9);
    assert_eq!(projection.sample_rate_hz, RATE);
    assert_eq!(
        h.engine.current_timing_acknowledgements.current_epoch(0),
        projection.publication_epoch
    );
    Python::attach(|py| {
        let metadata = constant_timing::current_metadata(&h.engine, py, 0)
            .unwrap()
            .unwrap();
        let metadata = metadata.bind(py).cast::<pyo3::types::PyDict>().unwrap();
        assert_eq!(
            metadata
                .get_item("period_seconds_per_quarter")
                .unwrap()
                .unwrap()
                .extract::<f64>()
                .unwrap()
                .to_bits(),
            projection.period_seconds.to_bits()
        );
        assert_eq!(
            metadata
                .get_item("source_sha256")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            h.engine.loaded_source_digests.lock().unwrap()[0]
                .clone()
                .unwrap()
        );
    });

    projection
}

fn bpm_productive(
    key_lock: bool,
) -> (
    Productive,
    crate::audio_engine::constant_timing::AcceptedTimingProjection,
) {
    use crate::audio_engine::constant_timing;
    let mut h = bpm_harness();
    let projection = accepted_bpm_timing(&mut h);
    let source = h.ticket(0);
    assert_eq!(source.publication.accepted_projection(), Some(projection));
    let oracle_pair = h.prepare(0, &source, true, None);
    assert_eq!(
        oracle_pair
            .pair_reader
            .descriptor
            .content
            .alignment
            .offset_frames,
        0
    );
    let selection = selection(&oracle_pair);
    let saved = h.save(&oracle_pair);
    let full = h.material.sample.clone();
    let full_stems = PreparedStemSet {
        complete_set_identity: Arc::new([71; 32]),
        reference_samples: full.samples.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        accepted_timing: None,
        source_version_hash: crate::audio_engine::stem_cache::source_version_hash(&h.version),
        sample_rate_hz: RATE,
        channels: 2,
        frame_count: full.frame_count(),
        available_mask: 0b1111,
        stems: oracle_pair.component_views_for_test().unwrap().clone(),
    };
    drop(oracle_pair);
    let window = prepare_window_with_producer(
        &h.engine,
        0,
        WindowRequest {
            key_lock: Some(key_lock),
            loop_region: Some((
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
            )),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    wait_until(|| h.consumer.peek().is_ok() || window.error().unwrap().is_some());
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(
        window.publication_status(),
        "accepted",
        "{:?}",
        window.error()
    );
    reconcile(&h.engine).unwrap();
    let bank = h.callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
    assert_eq!(
        (bank.resident_start(), bank.resident_end()),
        (LOOP_START, LOOP_END)
    );
    assert_eq!(bank.samples.len(), (LOOP_END - LOOP_START) * 2);
    assert_eq!(h.callback.mixer.key_lock_for_measurement(0), key_lock);
    let source = h.ticket(0);
    assert_eq!(source.publication.accepted_projection(), Some(projection));
    let pair = h.prepare(0, &source, false, Some(&selection));
    let selected = register(&mut h, &pair);

    let mut reference = Box::new(RtMixer::new(2, RATE as f32));
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_source(0, &full, RATE, 1);
    reference.set_input_runtime_ownership(ownership.clone());
    reference.load_sample(0, full.clone());
    assert!(reference.publish_prepared_stems(0, full_stems));
    let publication = PreparedSourcePermit::unrestricted();
    publication.mark_pending().unwrap();
    assert!(reference.publish_constant_timing_rt(
        0,
        constant_timing::PreparedConstantTiming {
            reference: full,
            publication,
            // Only the complete PCM oracle receives this copied actual ACK.
            // Engine/source authority above came from productive publication.
            projection,
        },
        &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
    ));
    reference.set_loop_region_frames(0, LOOP_START, Some(LOOP_END));
    {
        let mut producer = h.producer.lock().unwrap();
        crate::audio_engine::push_global_timing_message(
            &h.engine.global_timing_revision,
            &mut producer,
            ControlMessage::SetBpmLock(true),
            "SetBpmLock",
        )
        .unwrap();
    }
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    master_bpm_through_callback(&mut h, 150.0);
    reference.set_bpm_lock(true);
    reference.set_master_bpm(150.0);
    for mixer in [&mut h.callback.mixer, reference.as_mut()] {
        mixer.set_pad_key_lock(0, key_lock);
        mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
        assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
        assert!((mixer.output_bpm_for_sample_id(0).unwrap() - 150.0).abs() < 1e-10);
    }
    let mut p = Productive {
        h,
        _saved: saved,
        pair,
        selected,
        reference,
        reference_ownership: ownership,
        frame: 0,
        key_lock,
        complete_resident: false,
        loop_region: crate::audio_engine::source_reader::FrameRange {
            start: LOOP_START,
            end: LOOP_END,
        },
    };
    if key_lock {
        p.render(13, true);
        p.adopt_native();
    } else {
        p.render(137, false);
    }
    (p, projection)
}

fn cross_bpm_virtual_seam(
    p: &mut Productive,
    projection: crate::audio_engine::constant_timing::AcceptedTimingProjection,
) {
    let period = projection.period_seconds * f64::from(RATE) / 16.0;
    assert_ne!(period.to_bits(), ((LOOP_END - LOOP_START) as f64).to_bits());
    let playback = voice(&p.h.callback.mixer).source_playback;
    assert_eq!(playback.loop_period(), Some(period));
    let position = playback.position();
    let phase = (position.frame - LOOP_START) as f64 + position.fraction;
    let mut remaining = ((period - phase) / playback.tempo_ratio()).ceil() as usize + 1;
    let mut wrapped = false;
    for frames in [1_usize, 31, 17, 127].into_iter().cycle() {
        let frames = frames.min(remaining);
        let before = voice(&p.h.callback.mixer).source_playback.position();
        let before_phase = (before.frame - LOOP_START) as f64 + before.fraction;
        p.render(frames, false);
        let after = voice(&p.h.callback.mixer).source_playback;
        let after_position = after.position();
        let after_phase = (after_position.frame - LOOP_START) as f64 + after_position.fraction;
        assert_eq!(after.loop_period(), Some(period));
        let expected = (before_phase + frames as f64 * playback.tempo_ratio()).rem_euclid(period);
        assert!((after_phase - expected).abs() < 1e-8);
        wrapped |= after_phase < before_phase;
        remaining -= frames;
        if remaining == 0 {
            break;
        }
    }
    assert!(
        wrapped,
        "normal render must actually cross the accepted virtual seam"
    );
    if p.key_lock {
        let history = voice(&p.h.callback.mixer)
            .stretch
            .productive_history()
            .unwrap();
        assert_eq!(history.binding.accepted, Some(projection));
        assert_eq!(
            history.next_position,
            voice(&p.h.callback.mixer).source_playback.position()
        );
    }
}

#[test]
fn accepted_bpm_first_and_replacement_candidates_keep_real_period_native_fifo_and_eq() {
    use crate::audio_engine::input_runtime_binding;

    for key_lock in [false, true] {
        for selected_v1 in [false, true] {
            let (mut p, projection) = bpm_productive(key_lock);
            assert_eq!(
                voice(&p.h.callback.mixer).source_playback.tempo_ratio(),
                projection.period_seconds / (60.0 / 150.0)
            );
            if selected_v1 {
                make_v1_effective(&mut p);
            }
            cross_bpm_virtual_seam(&mut p, projection);
            let before = voice(&p.h.callback.mixer);
            let generation = before.generation;
            let native = before.stretch.native_state_address();
            let (_pair, selected) = candidate(&mut p, 201);
            let current = input_runtime_binding::capture(&p.h.engine, 0)
                .unwrap()
                .unwrap();
            assert!(current.current());
            assert_eq!(current.binding.accepted, Some(projection));
            assert!(p.selected.is_current());
            let receipt = request_candidate(&mut p, &selected, 0b1010);
            p.capture_then_wait(&receipt);
            p.render(31, false);
            let at_adopt = voice(&p.h.callback.mixer).source_playback;
            let fifos = voice(&p.h.callback.mixer).stretch.pending_fifo_frames();
            let history = voice(&p.h.callback.mixer).stretch.productive_history();
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            assert_eq!(
                receipt.residency_status(),
                "accepted",
                "{:?}",
                receipt.error()
            );
            assert!(receipt.claimed() && !receipt.withdraw());
            let actual = voice(&p.h.callback.mixer);
            assert_eq!(actual.generation, generation);
            assert!(actual.source_playback.matches_exact(&at_adopt));
            assert_eq!(actual.stretch.native_state_address(), native);
            assert_eq!(actual.stretch.pending_fifo_frames(), fifos);
            if let Some(before) = history {
                let after = actual.stretch.productive_history().unwrap();
                assert_eq!(after.binding, before.binding);
                assert_eq!(after.binding.accepted, Some(projection));
                assert_eq!(after.next_position, before.next_position);
                assert_eq!(after.fed_output_frames, before.fed_output_frames);
            }
            assert_eq!(
                p.h.callback.mixer.stems_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .accepted_timing,
                Some(projection)
            );
            p.select_reference("all_stems", reordered_mask(0b1010));
            p.selected = selected;
            settle(&mut p, &receipt);
            cross_bpm_virtual_seam(&mut p, projection);

            // The accepted V2 selection also needs a fresh own callback/worker
            // preparation when it is later released and requested again.
            let full = p.request("full_mix", 0b1010);
            p.capture_then_wait(&full);
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            p.select_reference("full_mix", 0b1010);
            settle(&mut p, &full);
            let again = p.request("all_stems", 0b1010);
            p.capture_then_wait(&again);
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            p.select_reference("all_stems", reordered_mask(0b1010));
            settle(&mut p, &again);
            cross_bpm_virtual_seam(&mut p, projection);
        }
    }
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

// Independent full-source phase arithmetic supplements the complete PCM mixer.
// The oracle does not read the actual SourceReadPlan or borrow candidate readers.
fn render_full_track(p: &mut Productive, frames: usize) -> bool {
    let before = voice(&p.h.callback.mixer).source_playback;
    let position = before.position();
    let period = p.h.material.sample.frame_count() as f64;
    assert_eq!(p.loop_region.start, 0);
    assert_eq!(p.loop_region.end as f64, period);
    let phase = position.frame as f64 + position.fraction;
    let distance = frames as f64 * before.tempo_ratio();
    let output = p.render(frames, false);
    assert!(output.iter().all(|sample| sample.is_finite()));
    let actual = voice(&p.h.callback.mixer).source_playback.position();
    let expected = (phase + distance).rem_euclid(period);
    let error = (actual.frame as f64 + actual.fraction - expected).abs();
    assert!(error.min((period - error).abs()) < 1e-8);
    assert_eq!(
        actual.seek_mode,
        crate::audio_engine::source_reader::ExplicitSeekMode::Normal
    );
    phase + distance >= period
}

#[test]
fn full_track_first_and_replacement_keep_own_native_fifo_eq_and_actual_seam() {
    for key_lock in [false, true] {
        for ratio in [0.73, 1.371_234_567_890_123] {
            for selected_v1 in [false, true] {
                let mut p = Productive::full_track(key_lock, ratio);
                if selected_v1 {
                    make_v1_effective(&mut p);
                }
                // Ordinary productive callbacks place the handover at the actual
                // end-to-start seam; no Seek, restart or phase reset is involved.
                let position = voice(&p.h.callback.mixer).source_playback.position();
                let period = p.loop_region.end as f64;
                let distance =
                    (period - 64.0 - position.frame as f64 - position.fraction).rem_euclid(period);
                let frames = (distance / ratio).floor() as usize;
                if frames != 0 {
                    render_full_track(&mut p, frames);
                }
                let generation = voice(&p.h.callback.mixer).generation;
                let native = voice(&p.h.callback.mixer).stretch.native_state_address();
                let original = p.h.callback.mixer.bank_for_measurement()[0]
                    .clone()
                    .unwrap();
                let old = p.h.callback.mixer.stems_for_measurement()[0]
                    .as_ref()
                    .map(|set| set.complete_set_identity.clone());
                let (_pair, selected) = candidate(&mut p, 112);
                assert!(p.selected.is_current());
                let receipt = request_candidate(&mut p, &selected, 0b1010);
                p.capture_then_wait(&receipt);
                render_full_track(&mut p, 1);
                let before = voice(&p.h.callback.mixer);
                let playback = before.source_playback;
                let fifos = before.stretch.pending_fifo_frames();
                let history = before.stretch.productive_history();
                assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
                assert_eq!(
                    receipt.residency_status(),
                    "accepted",
                    "{:?}",
                    receipt.error()
                );
                assert!(receipt.claimed() && selected.is_current() && !p.selected.is_current());
                assert!(!receipt.withdraw());
                let actual = voice(&p.h.callback.mixer);
                assert_eq!(actual.generation, generation);
                assert!(actual.source_playback.matches_exact(&playback));
                assert_eq!(actual.stretch.native_state_address(), native);
                assert_eq!(actual.stretch.pending_fifo_frames(), fifos);
                if let Some(history) = history {
                    let after = actual.stretch.productive_history().unwrap();
                    assert_eq!(after.binding, history.binding);
                    assert_eq!(after.next_position, history.next_position);
                    assert_eq!(after.fed_output_frames, history.fed_output_frames);
                }
                assert!(
                    original.same_window(
                        p.h.callback.mixer.bank_for_measurement()[0]
                            .as_ref()
                            .unwrap()
                    )
                );
                let current = &p.h.callback.mixer.stems_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .complete_set_identity;
                if let Some(old) = old {
                    assert!(!Arc::ptr_eq(&old, current));
                }
                p.select_reference("all_stems", reordered_mask(0b1010));
                p.selected = selected;
                let mut remaining = (128.0 / ratio).ceil() as usize;
                let mut wrapped = false;
                for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
                    let frames = frames.min(remaining);
                    wrapped |= render_full_track(&mut p, frames);
                    assert_eq!(
                        voice(&p.h.callback.mixer).stretch.native_state_address(),
                        native
                    );
                    remaining -= frames;
                    if remaining == 0 {
                        break;
                    }
                }
                assert!(wrapped, "the consumed-source handover must cross N-1 -> 0");
                assert!(
                    !p.h.callback
                        .mixer
                        .stem_transition_for_measurement(0)
                        .is_active()
                );
                render_full_track(&mut p, 777);
                if key_lock {
                    p.render(13, true);
                    p.adopt_native();
                }
                assert_eq!(
                    receipt.effective_mode_status(),
                    "effective",
                    "{:?}",
                    receipt.error()
                );
                assert!(render_full_track(&mut p, (period / ratio).ceil() as usize));
                assert_eq!(voice(&p.h.callback.mixer).generation, generation);
            }
        }
    }
}

#[test]
fn full_track_paused_and_stopped_adoption_waits_for_own_consumption() {
    for key_lock in [false, true] {
        for selected_v1 in [false, true] {
            for stopped in [false, true] {
                let mut p = Productive::full_track(key_lock, 0.73);
                if selected_v1 {
                    make_v1_effective(&mut p);
                }
                if stopped {
                    p.h.callback.mixer.stop_sample(0);
                    p.reference.stop_sample(0);
                } else {
                    p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
                    p.reference.pause_sample_at_output_frame(0, p.frame);
                }
                let (_pair, selected) = candidate(&mut p, 113);
                let receipt = request_candidate(&mut p, &selected, 0b1010);
                p.capture_then_wait(&receipt);
                assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
                assert_eq!(
                    receipt.residency_status(),
                    "accepted",
                    "{:?}",
                    receipt.error()
                );
                assert!(receipt.claimed() && selected.is_current());
                p.select_reference("all_stems", reordered_mask(0b1010));
                p.selected = selected;
                if stopped {
                    assert_eq!(receipt.effective_mode_status(), "waiting");
                    assert!(
                        p.h.callback
                            .mixer
                            .voices
                            .iter()
                            .all(|voice| !voice.is_playing_sample(0))
                    );
                    assert!(
                        p.h.callback
                            .mixer
                            .play_sample_at_output_frame(0, 1.0, p.frame)
                    );
                    assert!(p.reference.play_sample_at_output_frame(0, 1.0, p.frame));
                } else {
                    for frames in [1, 127, 384] {
                        p.render_paused(frames);
                        assert_eq!(receipt.effective_mode_status(), "transitioning");
                    }
                    p.h.callback.mixer.resume_sample_at_output_frame(0, p.frame);
                    p.reference.resume_sample_at_output_frame(0, p.frame);
                }
                settle(&mut p, &receipt);
                assert!(render_full_track(
                    &mut p,
                    (4096.0_f64 / 0.73).ceil() as usize
                ));
            }
        }
    }
}

fn acknowledge_candidate_seek(
    p: &mut Productive,
    target: usize,
) -> crate::audio_engine::resident_relocation::ResidentWindowTicket {
    let old_window = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap()
        .clone();
    let previous_frame = p.h.callback.transport.output_frame();
    p.h.callback
        .transport
        .advance_by_rendered_frames((p.frame - previous_frame) as usize);
    let seek = prepare_window_with_producer(
        &p.h.engine,
        0,
        WindowRequest {
            seek_position_s: Some(target as f64 / f64::from(RATE)),
            ..Default::default()
        },
        p.h.producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        if matches!(
            p.h.consumer.peek(),
            Ok(ControlMessage::CaptureResidentSeek(_))
        ) {
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        }
        seek.publication_status() == "accepted"
            || (seek.publication_status() == "pending" && !p.h.consumer.is_empty())
    });
    if seek.publication_status() == "accepted" {
        let current = p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert_eq!(current.resident_binding(), old_window.resident_binding());
        assert!(Arc::ptr_eq(&current.samples, &old_window.samples));
        assert!(p.h.consumer.is_empty(), "covered Seek requires no Relocate");
    } else {
        assert_eq!(
            p.h.callback.drain(&mut p.h.consumer),
            1,
            "actual own Seek Relocate"
        );
    }
    assert_eq!(seek.publication_status(), "accepted", "{:?}", seek.error());
    reconcile(&p.h.engine).unwrap();
    assert!(seek.is_current());
    assert_eq!(
        seek.effective_seek_seconds(),
        Some(target as f64 / f64::from(RATE))
    );
    seek
}

struct CandidateSeekPrepared {
    target: u64,
    address: usize,
    raw: ControllerRawNative,
    baseline: ControllerRawNative,
}

struct CandidateSeekOracle {
    original: SampleBuffer,
    components: [SampleBuffer; 4],
    seek_target: usize,
    selection: Option<u8>,
    previous: Option<u8>,
    baseline_selection: Option<u8>,
    output_frames: u64,
    ramp_start: u64,
    clock: crate::audio_engine::mixer::prepared_native_mixer_tests::finite_lifecycle_tests::AlgebraicClock,
    raw: ControllerRawNative,
    filter: PerPadDspChain,
    baseline_raw: ControllerRawNative,
    baseline_filter: PerPadDspChain,
    new_wet: bool,
    distinct_wet: bool,
    candidate: Option<CandidateSeekPrepared>,
}

impl CandidateSeekOracle {
    fn after_ack(p: &mut Productive, target: usize, selected_v1: bool) -> Self {
        use crate::audio_engine::mixer::prepared_native_mixer_tests::finite_lifecycle_tests::AlgebraicClock;
        assert!(p.reference.seek_sample_at_output_frame(
            0,
            target as f64 / f64::from(RATE),
            p.frame
        ));
        let full = p.reference.bank_for_measurement()[0].as_ref().unwrap();
        let stems = p.reference.stems_for_measurement()[0].as_ref().unwrap();
        let original = full.clone();
        let components = stems.stems.clone();
        let selection = selected_v1.then_some(0b1010);
        let mut clock = AlgebraicClock::physical(LOOP_START, LOOP_END, 0.73);
        clock.seek_physical(target, full.frame_count());
        // Seek cuts the old DSP state. Apply its persistent targets to a fresh
        // stereo chain without reset(), retaining ordinary parameter smoothing.
        let mut filter = PerPadDspChain::new(0, RATE as f32, 2);
        let mut baseline_filter = PerPadDspChain::new(0, RATE as f32, 2);
        for (slot, db) in [
            (DspParameterSlot::Slot0, -12.0_f32),
            (DspParameterSlot::Slot1, -3.0),
            (DspParameterSlot::Slot2, 4.0),
        ] {
            let normalized = if db <= 0.0 {
                0.5 * 10.0_f32.powf(db / 20.0)
            } else {
                0.5 + 0.5 * (db / 6.0)
            };
            for chain in [&mut filter, &mut baseline_filter] {
                assert!(chain.set_parameter(
                    DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
                    normalized
                ));
            }
        }
        Self {
            original,
            components,
            seek_target: target,
            selection,
            previous: selection,
            baseline_selection: selection,
            output_frames: 0,
            ramp_start: 0,
            clock,
            raw: ControllerRawNative::new(0.73, false),
            filter,
            baseline_raw: ControllerRawNative::new(0.73, false),
            baseline_filter,
            new_wet: false,
            distinct_wet: false,
            candidate: None,
        }
    }

    fn select_candidate(&mut self, p: &mut Productive) {
        // Only the independent complete V1 components supply expected V2 PCM.
        // The declared permutation changes its mask, never its reader identity.
        self.previous = self.selection;
        self.selection = Some(reordered_mask(0b1010));
        self.ramp_start = self.output_frames;
        p.select_reference("all_stems", self.selection.unwrap());
    }

    fn feed(&self, frames: usize) -> [Vec<f32>; 2] {
        self.feed_selection(frames, false)
    }

    fn feed_selection(&self, frames: usize, baseline: bool) -> [Vec<f32>; 2] {
        // Independent physical one-shot knots and 128 consumed-source gains;
        // mix each knot before interpolation, preserving the declared f32 order.
        std::array::from_fn(|channel| {
            (0..frames)
                .map(|offset| {
                    let (left, fraction, explicit) = self.clock.physical_position(offset);
                    let boundary = if explicit {
                        if self.seek_target < LOOP_START {
                            LOOP_START
                        } else {
                            self.original.frame_count()
                        }
                    } else {
                        LOOP_END
                    };
                    let right = if left + 1 == boundary {
                        LOOP_START
                    } else {
                        left + 1
                    };
                    let knot = |frame: usize, mask: Option<u8>| {
                        mask.map_or(self.original.samples[frame * 2 + channel], |mask| {
                            self.components
                                .iter()
                                .enumerate()
                                .filter(|(index, _)| mask & (1 << index) != 0)
                                .fold(0.0, |sum, (_, component)| {
                                    sum + component.samples[frame * 2 + channel]
                                })
                        })
                    };
                    let sample = |frame| {
                        if baseline {
                            return knot(frame, self.baseline_selection);
                        }
                        let gain = if self.previous == self.selection {
                            1.0
                        } else {
                            (((self.output_frames + offset as u64 - self.ramp_start) as f64 * 0.73)
                                .min(128.0) as f32)
                                / 128.0
                        };
                        if gain == 1.0 {
                            knot(frame, self.selection)
                        } else {
                            knot(frame, self.previous) * (1.0 - gain)
                                + knot(frame, self.selection) * gain
                        }
                    };
                    let value = sample(left);
                    value + (sample(right) - value) * fraction as f32
                })
                .collect()
        })
    }

    fn render(&mut self, p: &mut Productive, frames: usize, timestamp: bool) -> Vec<f32> {
        if self
            .candidate
            .as_ref()
            .is_some_and(|prepared| p.frame == prepared.target)
        {
            let prepared = self.candidate.take().unwrap();
            self.raw = prepared.raw;
            self.baseline_raw = prepared.baseline;
            self.new_wet = self.selection != self.baseline_selection;
            assert_eq!(
                voice_mut(&mut p.h.callback.mixer)
                    .stretch
                    .prepared_native_address(),
                prepared.address
            );
        }
        let feed = self.feed(frames);
        let raw = self.raw.render(&feed);
        let baseline = self.baseline_raw.render(&self.feed_selection(frames, true));
        let mut output = vec![0.0; frames * 2];
        let mut complete = vec![0.0; frames * 2];
        reset_tap_observation_for_test();
        if timestamp {
            p.h.callback.mixer.render_rt_at_output_frame(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                p.frame,
                &mut p.h.callback.retirement,
            );
            p.reference
                .render_at_output_frame(p.frame, &mut complete, &mut [0.0; NUM_SAMPLES]);
        } else {
            p.h.callback.mixer.render_rt(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                &mut p.h.callback.retirement,
            );
            p.reference.render(&mut complete, &mut [0.0; NUM_SAMPLES]);
        }
        let taps = tap_observation_for_test();
        let sample = voice(&p.h.callback.mixer).sample.as_ref().unwrap();
        assert_eq!(taps.missing_reads, 0);
        assert!(
            taps.min_frame
                .is_some_and(|frame| frame >= sample.resident_start())
        );
        assert!(
            taps.max_frame
                .is_some_and(|frame| frame < sample.resident_end())
        );
        assert_eq!(
            output, complete,
            "complete post-Seek candidate guard at {}",
            p.frame
        );
        for ((actual, raw), baseline) in output
            .chunks_exact(2)
            .zip(raw.chunks_exact(2))
            .zip(baseline.chunks_exact(2))
        {
            self.filter.begin_frame();
            self.baseline_filter.begin_frame();
            for channel in 0..2 {
                assert_eq!(
                    actual[channel],
                    self.filter.process_sample(channel, raw[channel]),
                    "independent post-Seek Native/EQ at {}",
                    p.frame
                );
                let unchanged = self
                    .baseline_filter
                    .process_sample(channel, baseline[channel]);
                if self.new_wet {
                    self.distinct_wet |= (actual[channel] - unchanged).abs() > 0.001;
                } else if self.selection == self.baseline_selection {
                    assert_eq!(actual[channel], unchanged);
                }
            }
        }
        self.clock.advance(frames);
        self.output_frames += frames as u64;
        let (frame, fraction, explicit) = self.clock.physical_position(0);
        let actual = voice(&p.h.callback.mixer);
        assert_eq!(actual.source_playback.position().frame, frame);
        assert!((actual.source_playback.position().fraction - fraction).abs() < 1e-9);
        assert_eq!(
            actual.source_playback.position().seek_mode
                != crate::audio_engine::source_reader::ExplicitSeekMode::Normal,
            explicit
        );
        assert!(
            actual
                .source_playback
                .matches_exact(&voice(&p.reference).source_playback)
        );
        assert_eq!(
            actual.stretch.pending_fifo_frames(),
            (self.raw.input[0].len(), self.raw.output[0].len())
        );
        assert_eq!(
            actual.stretch.pending_fifo_frames(),
            voice(&p.reference).stretch.pending_fifo_frames()
        );
        p.frame += frames as u64;
        output
    }

    fn later_native(&mut self, p: &mut Productive) {
        let capture = p.frame;
        let mut raw = ControllerRawNative::new(0.73, true);
        raw.render(&self.feed(4096));
        let mut baseline = ControllerRawNative::new(0.73, true);
        baseline.render(&self.feed_selection(4096, true));
        self.render(p, 1, true);
        wait_until(|| {
            voice_mut(&mut p.h.callback.mixer)
                .stretch
                .source_preparation_ready()
                && voice_mut(&mut p.reference)
                    .stretch
                    .source_preparation_ready()
        });
        let target = voice_mut(&mut p.h.callback.mixer)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        assert_eq!(target, capture + 4096);
        assert_eq!(
            voice_mut(&mut p.reference)
                .stretch
                .prepared_target_output_frame(),
            Some(target)
        );
        let address = voice_mut(&mut p.h.callback.mixer)
            .stretch
            .prepared_native_address();
        assert_ne!(
            address,
            voice(&p.h.callback.mixer).stretch.native_state_address()
        );
        let taps = voice_mut(&mut p.h.callback.mixer)
            .stretch
            .prepared_tap_observation()
            .unwrap();
        let sample = voice(&p.h.callback.mixer).sample.as_ref().unwrap();
        assert_eq!(taps.missing_reads, 0);
        assert_eq!(taps.left_reads, 4096 * 2);
        assert!(
            taps.min_frame
                .is_some_and(|frame| frame >= sample.resident_start())
        );
        assert!(
            taps.max_frame
                .is_some_and(|frame| frame < sample.resident_end())
        );
        self.candidate = Some(CandidateSeekPrepared {
            target,
            address,
            raw,
            baseline,
        });
        for frames in [1_usize, 127, 777, 96, 257, 512, 31].into_iter().cycle() {
            if p.frame == target {
                break;
            }
            self.render(p, frames.min((target - p.frame) as usize), true);
        }
        let mut audible = false;
        for frames in [137, 511, 257, 31, 512, 777, 512] {
            audible |= self
                .render(p, frames, true)
                .iter()
                .any(|sample| sample.abs() > 0.001);
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                address
            );
        }
        assert!(audible);
    }
}

#[test]
fn actual_tail_first_and_replacement_keep_own_wet_history_through_normal_return() {
    use crate::audio_engine::source_reader::ExplicitSeekMode;
    for selected_v1 in [false, true] {
        let mut p = Productive::with_harness(bpm_harness(), true, 0.73, false);
        assert!(
            crate::audio_engine::input_runtime_binding::capture(&p.h.engine, 0)
                .unwrap()
                .unwrap()
                .binding
                .accepted
                .is_none()
        );
        if selected_v1 {
            make_v1_effective(&mut p);
        }
        let _seek = acknowledge_candidate_seek(&mut p, LOOP_END);
        let source = p.h.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        assert_eq!(source.frame_count(), 16_384);
        assert_eq!(
            (source.resident_start(), source.resident_end()),
            (LOOP_START, 16_384)
        );
        assert_eq!(
            source.residency.as_ref().unwrap().context,
            crate::messages::ResidentContext::KeyLockFiniteLoop
        );
        assert_eq!(
            voice(&p.h.callback.mixer).explicit_seek_mode,
            ExplicitSeekMode::AfterLoop
        );
        let generation = voice(&p.h.callback.mixer).generation;
        let mut oracle = CandidateSeekOracle::after_ack(&mut p, LOOP_END, selected_v1);
        for frames in [1, 31, 137, 257, 511] {
            oracle.render(&mut p, frames, false);
        }
        oracle.later_native(&mut p);
        assert_eq!(
            voice(&p.h.callback.mixer).explicit_seek_mode,
            ExplicitSeekMode::AfterLoop
        );
        assert!(
            voice(&p.h.callback.mixer)
                .stretch
                .wet_current_source_output_ready()
        );
        let (_pair, registration) = candidate(&mut p, 170 + usize::from(selected_v1));
        let predecessor = p.h.callback.mixer.stems_for_measurement()[0].clone();
        let receipt = request_candidate(&mut p, &registration, 0b1010);
        p.capture_then_wait(&receipt);
        oracle.render(&mut p, 31, false);
        assert_eq!(receipt.effective_mode_status(), "waiting");
        let before = voice(&p.h.callback.mixer);
        let playback = before.source_playback;
        let native = before.stretch.native_state_address();
        let fifo = before.stretch.pending_fifo_frames();
        let history = before.stretch.productive_history().unwrap();
        let dsp = p.h.callback.mixer.dsp_source_history_for_test(0).unwrap();
        assert_eq!(before.explicit_seek_mode, ExplicitSeekMode::AfterLoop);
        assert_eq!(
            p.h.callback.drain(&mut p.h.consumer),
            1,
            "own Tail V2 Adopt"
        );
        assert_eq!(
            receipt.residency_status(),
            "accepted",
            "{:?}",
            receipt.error()
        );
        assert!(receipt.claimed() && registration.is_current());
        assert_eq!(receipt.effective_mode_status(), "transitioning");
        let after = voice(&p.h.callback.mixer);
        assert!(after.source_playback.matches_exact(&playback));
        assert_eq!(after.generation, generation);
        assert_eq!(after.stretch.native_state_address(), native);
        assert_eq!(after.stretch.pending_fifo_frames(), fifo);
        let after_history = after.stretch.productive_history().unwrap();
        assert_eq!(after_history.binding, history.binding);
        assert_eq!(after_history.next_position, history.next_position);
        assert_eq!(after_history.domain, history.domain);
        assert_eq!(after_history.fed_output_frames, history.fed_output_frames);
        let after_dsp = p.h.callback.mixer.dsp_source_history_for_test(0).unwrap();
        assert_eq!(after_dsp.binding, dsp.binding);
        assert_eq!(after_dsp.next_position, dsp.next_position);
        assert_eq!(after_dsp.domain, dsp.domain);
        assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
        let resident = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert_eq!(*resident.complete_set_identity, *_pair._identity);
        for component in &resident.stems {
            assert_eq!(
                (component.resident_start(), component.resident_end()),
                (LOOP_START, 16_384)
            );
        }
        if let Some(previous) = predecessor.as_ref() {
            let outgoing =
                p.h.callback
                    .mixer
                    .outgoing_stems_for_measurement(0)
                    .unwrap();
            assert!(Arc::ptr_eq(
                &previous.complete_set_identity,
                &outgoing.complete_set_identity
            ));
            assert_ne!(
                *previous.complete_set_identity,
                *resident.complete_set_identity
            );
        }
        oracle.select_candidate(&mut p);
        p.selected = registration;
        let mut remaining = (128.0_f64 / 0.73).ceil() as usize;
        for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
            let frames = frames.min(remaining);
            oracle.render(&mut p, frames, false);
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                native
            );
            remaining -= frames;
            assert_eq!(
                p.h.callback
                    .mixer
                    .stem_transition_for_measurement(0)
                    .is_active(),
                remaining > 0
            );
            assert_eq!(receipt.effective_mode_status(), "transitioning");
            if remaining == 0 {
                break;
            }
        }
        oracle.later_native(&mut p);
        assert_eq!(
            voice(&p.h.callback.mixer).explicit_seek_mode,
            ExplicitSeekMode::AfterLoop
        );
        assert!(
            oracle.new_wet && oracle.distinct_wet,
            "V2 own Wet must differ from independently retained unchanged FullMix/V1"
        );
        assert_eq!(
            receipt.effective_mode_status(),
            "effective",
            "{:?}",
            receipt.error()
        );
        let tail_native = voice(&p.h.callback.mixer).stretch.native_state_address();
        let (frame, fraction, explicit) = oracle.clock.physical_position(0);
        assert!(explicit);
        let mut remaining = ((16_384.0 - frame as f64 - fraction) / 0.73).ceil() as usize;
        for frames in [1_usize, 127, 511, 31, 777].into_iter().cycle() {
            let frames = frames.min(remaining);
            oracle.render(&mut p, frames, true);
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                tail_native
            );
            remaining -= frames;
            if remaining == 0 {
                break;
            }
        }
        assert_eq!(
            voice(&p.h.callback.mixer).explicit_seek_mode,
            ExplicitSeekMode::Normal
        );
        for frames in [1, 31, 137, 511, 257, 777, 512, 777, 1093] {
            oracle.render(&mut p, frames, true);
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                tail_native
            );
        }
        assert_eq!(voice(&p.h.callback.mixer).generation, generation);
        assert!(
            voice(&p.h.callback.mixer)
                .sample
                .as_ref()
                .unwrap()
                .same_window(&source)
        );
        assert!(receipt.claimed() && receipt.is_current() && p.selected.is_current());
        assert!(oracle.distinct_wet && p.h.consumer.is_empty());
    }
}

#[test]
fn actual_tail_same_set_requests_preserve_their_own_voice() {
    unsupported_seek_modes(
        LOOP_END,
        &[
            (true, false, false),
            (false, false, false),
            (true, false, true),
            (false, false, true),
            (true, true, false),
            (false, true, false),
            (true, true, true),
            (false, true, true),
        ],
    );
}

#[test]
fn actual_intro_same_set_requests_preserve_their_own_voice() {
    unsupported_seek_modes(
        0,
        &[
            (true, false, false),
            (false, false, false),
            (true, false, true),
            (false, false, true),
            (true, true, false),
            (false, true, false),
            (true, true, true),
            (false, true, true),
        ],
    );
}

fn unsupported_seek_modes(target: usize, contexts: &[(bool, bool, bool)]) {
    use crate::audio_engine::source_reader::ExplicitSeekMode;
    for &(key_lock, musical, paused) in contexts {
        let mut p = if musical {
            bpm_productive(key_lock).0
        } else {
            Productive::new(key_lock, 0.73)
        };
        let _seek = acknowledge_candidate_seek(&mut p, target);
        if paused {
            p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
            assert!(voice(&p.h.callback.mixer).paused);
        }
        assert_eq!(
            voice(&p.h.callback.mixer).explicit_seek_mode,
            if target == 0 {
                ExplicitSeekMode::BeforeLoop
            } else {
                ExplicitSeekMode::AfterLoop
            }
        );
        assert!(
            crate::audio_engine::input_runtime_binding::capture(&p.h.engine, 0)
                .unwrap()
                .unwrap()
                .binding
                .accepted
                .is_some()
                == musical
        );
        // Fresh own registration for the already selected complete set does not
        // acquire the separate new-generation candidate authority.
        let source = p.h.ticket(0);
        let pair = p.h.prepare(0, &source, false, Some(&selection(&p.pair)));
        let registration =
            p.h.engine
                .register_committed_stem_pair_with_producer(&pair, &source, &p.h.producer)
                .unwrap();
        if registration.publication_status() == "pending" {
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        }
        assert_eq!(registration.publication_status(), "accepted");
        assert!(registration.is_current());
        let receipt = request_candidate(&mut p, &registration, 0b1010);
        let before = voice(&p.h.callback.mixer);
        let playback = before.source_playback;
        let generation = before.generation;
        let native = before.stretch.native_state_address();
        let fifo = before.stretch.pending_fifo_frames();
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        assert_eq!(receipt.residency_status(), "rejected");
        assert!(!receipt.claimed() && registration.is_current());
        wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
        assert!(p.h.consumer.is_empty());
        let after = voice(&p.h.callback.mixer);
        assert!(after.source_playback.matches_exact(&playback));
        assert_eq!(after.generation, generation);
        assert_eq!(after.paused, paused);
        assert_eq!(after.stretch.native_state_address(), native);
        assert_eq!(after.stretch.pending_fifo_frames(), fifo);
        assert_eq!(receipt.effective_mode_status(), "rejected");
    }
}

#[test]
fn complete_key_lock_intro_and_tail_same_set_requests_preserve_own_voice() {
    for target in [0, LOOP_END] {
        for paused in [false, true] {
            let mut p = Productive::complete_partial(true, 0.73);
            make_v1_effective(&mut p);
            let _seek = acknowledge_candidate_seek(&mut p, target);
            let sample = p.h.callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap();
            assert_eq!(
                sample.residency.as_ref().unwrap().context,
                crate::messages::ResidentContext::KeyLockFullTrack
            );
            assert_eq!(sample.resident_start(), 0);
            assert_eq!(sample.resident_end(), sample.frame_count());
            assert_eq!(
                voice(&p.h.callback.mixer).explicit_seek_mode,
                if target == 0 {
                    crate::audio_engine::source_reader::ExplicitSeekMode::BeforeLoop
                } else {
                    crate::audio_engine::source_reader::ExplicitSeekMode::AfterLoop
                }
            );
            if paused {
                p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
            }
            let source = p.h.ticket(0);
            let pair = p.h.prepare(0, &source, false, Some(&selection(&p.pair)));
            let registration =
                p.h.engine
                    .register_committed_stem_pair_with_producer(&pair, &source, &p.h.producer)
                    .unwrap();
            if registration.publication_status() == "pending" {
                assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            }
            assert_eq!(registration.publication_status(), "accepted");
            let receipt = request_candidate(&mut p, &registration, 0b1010);
            let before = voice(&p.h.callback.mixer);
            let clock = before.source_playback;
            let generation = before.generation;
            let native = before.stretch.native_state_address();
            let fifo = before.stretch.pending_fifo_frames();
            let source = before.sample.clone().unwrap();
            let readers = p.h.callback.mixer.stems_for_measurement()[0]
                .clone()
                .unwrap();
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            assert_eq!(receipt.residency_status(), "rejected");
            assert_eq!(receipt.effective_mode_status(), "rejected");
            assert!(!receipt.claimed() && p.h.consumer.is_empty());
            // The rejected Capture releases its waiting worker off callback;
            // rejection is not a synchronous worker-termination acknowledgement.
            wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
            let after = voice(&p.h.callback.mixer);
            assert!(after.source_playback.matches_exact(&clock));
            assert_eq!(after.generation, generation);
            assert_eq!(after.paused, paused);
            assert_eq!(after.stretch.native_state_address(), native);
            assert_eq!(after.stretch.pending_fifo_frames(), fifo);
            assert!(after.sample.as_ref().unwrap().same_window(&source));
            let current = p.h.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap();
            assert!(Arc::ptr_eq(
                &readers.complete_set_identity,
                &current.complete_set_identity
            ));
            for (old, current) in readers.stems.iter().zip(&current.stems) {
                assert!(Arc::ptr_eq(&old.samples, &current.samples));
            }
            assert!(registration.is_current());
        }
    }
}

#[test]
fn own_in_loop_and_equal_phase_seek_rejects_older_capture_and_adopt() {
    for key_lock in [false, true] {
        for musical in [false, true] {
            for equal_phase in [false, true] {
                for after_capture in [false, true] {
                    let mut p = if musical {
                        bpm_productive(key_lock).0
                    } else {
                        Productive::with_harness(bpm_harness(), key_lock, 0.73, false)
                    };
                    make_v1_effective(&mut p);
                    let target = LOOP_START + 64;
                    if equal_phase {
                        let _first_seek = acknowledge_candidate_seek(&mut p, target);
                        assert_eq!(
                            voice(&p.h.callback.mixer).source_playback.position().frame,
                            target
                        );
                        assert_eq!(
                            voice(&p.h.callback.mixer)
                                .source_playback
                                .position()
                                .fraction,
                            0.0
                        );
                    }
                    let (_pair, registration) = candidate(&mut p, 182);
                    let receipt = request_candidate(&mut p, &registration, 0b1010);
                    if after_capture {
                        p.capture_then_wait(&receipt);
                    }
                    let stale = p.h.consumer.pop().unwrap();
                    assert!(p.h.consumer.is_empty());
                    let before_seek = voice(&p.h.callback.mixer);
                    let previous_generation = before_seek.generation;
                    let previous_history = before_seek
                        .stretch
                        .source_epoch_owner()
                        .load(Ordering::Acquire);
                    let previous_timing = before_seek.source_timing.accepted;
                    if equal_phase {
                        assert_eq!(before_seek.source_playback.position().frame, target);
                        assert_eq!(before_seek.source_playback.position().fraction, 0.0);
                    }
                    let _seek = acknowledge_candidate_seek(&mut p, target);
                    let before = voice(&p.h.callback.mixer);
                    assert_ne!(before.generation, previous_generation);
                    assert_ne!(
                        before.stretch.source_epoch_owner().load(Ordering::Acquire),
                        previous_history
                    );
                    assert_eq!(before.source_timing.accepted, previous_timing);
                    assert_eq!(before.source_playback.position().frame, target);
                    assert_eq!(before.source_playback.position().fraction, 0.0);
                    assert_eq!(
                        before.explicit_seek_mode,
                        crate::audio_engine::source_reader::ExplicitSeekMode::Normal
                    );
                    let clock = before.source_playback;
                    let generation = before.generation;
                    let native = before.stretch.native_state_address();
                    let fifo = before.stretch.pending_fifo_frames();
                    let source = before.sample.clone().unwrap();
                    let old_readers = p.h.callback.mixer.stems_for_measurement()[0]
                        .clone()
                        .unwrap();
                    p.h.producer.lock().unwrap().push(stale).unwrap();
                    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
                    wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
                    assert_eq!(receipt.residency_status(), "rejected");
                    assert_eq!(receipt.effective_mode_status(), "rejected");
                    assert!(!receipt.claimed() && p.h.consumer.is_empty());
                    let after = voice(&p.h.callback.mixer);
                    assert!(after.source_playback.matches_exact(&clock));
                    assert_eq!(after.generation, generation);
                    assert_eq!(after.stretch.native_state_address(), native);
                    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
                    assert_eq!(after.source_timing.accepted, previous_timing);
                    assert!(after.sample.as_ref().unwrap().same_window(&source));
                    let current = p.h.callback.mixer.stems_for_measurement()[0]
                        .as_ref()
                        .unwrap();
                    assert!(Arc::ptr_eq(
                        &old_readers.complete_set_identity,
                        &current.complete_set_identity
                    ));
                    for (old, current) in old_readers.stems.iter().zip(&current.stems) {
                        assert!(Arc::ptr_eq(&old.samples, &current.samples));
                    }
                    assert!(p.reference.seek_sample_at_output_frame(
                        0,
                        target as f64 / f64::from(RATE),
                        p.frame
                    ));
                    if key_lock {
                        p.render(13, true);
                        p.adopt_native();
                    }
                    let output = p.render(257, true);
                    assert!(output.iter().any(|sample| sample.abs() > 0.001));
                    assert_eq!(receipt.effective_mode_status(), "rejected");
                }
            }
        }
    }
}

#[test]
fn musical_intro_and_tail_stale_timing_ack_rejects_capture_and_adopt() {
    use crate::audio_engine::constant_timing::l3_constant_timing_fixture;
    for target in [0, LOOP_END] {
        for key_lock in [false, true] {
            for paused in [false, true] {
                for after_capture in [false, true] {
                    let (mut p, accepted) = bpm_productive(key_lock);
                    make_v1_effective(&mut p);
                    let _seek = acknowledge_candidate_seek(&mut p, target);
                    if paused {
                        p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
                    }
                    let (_pair, registration) = candidate(&mut p, 181);
                    let receipt = request_candidate(&mut p, &registration, 0b1010);
                    if after_capture {
                        p.capture_then_wait(&receipt);
                    }
                    // Hold the actual queued Capture or Adopt off realtime while
                    // another genuine timing publication obtains its own ACK.
                    let stale = p.h.consumer.pop().unwrap();
                    assert!(p.h.consumer.is_empty());
                    let newer = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
                        &p.h.engine,
                        &p.h.producer,
                        0,
                        60.0 / 121.001,
                    );
                    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
                    assert_eq!(newer.publication_status().unwrap(), "accepted");
                    let binding =
                        crate::audio_engine::input_runtime_binding::capture(&p.h.engine, 0)
                            .unwrap()
                            .unwrap();
                    let new_accepted = binding.binding.accepted.unwrap();
                    assert_ne!(new_accepted.publication_epoch, accepted.publication_epoch);
                    let before = voice(&p.h.callback.mixer);
                    let clock = before.source_playback;
                    let generation = before.generation;
                    let native = before.stretch.native_state_address();
                    let fifo = before.stretch.pending_fifo_frames();
                    let source = before.sample.clone().unwrap();
                    let old_readers = p.h.callback.mixer.stems_for_measurement()[0].clone();
                    p.h.producer.lock().unwrap().push(stale).unwrap();
                    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
                    wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
                    assert_eq!(receipt.residency_status(), "rejected");
                    assert_eq!(receipt.effective_mode_status(), "rejected");
                    assert!(!receipt.claimed() && p.h.consumer.is_empty());
                    let after = voice(&p.h.callback.mixer);
                    assert!(after.source_playback.matches_exact(&clock));
                    assert_eq!(after.generation, generation);
                    assert_eq!(after.paused, paused);
                    assert_eq!(after.stretch.native_state_address(), native);
                    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
                    assert!(after.sample.as_ref().unwrap().same_window(&source));
                    match (
                        old_readers.as_ref(),
                        p.h.callback.mixer.stems_for_measurement()[0].as_ref(),
                    ) {
                        (Some(old), Some(current)) => {
                            assert!(Arc::ptr_eq(
                                &old.complete_set_identity,
                                &current.complete_set_identity
                            ));
                            for (old, current) in old.stems.iter().zip(&current.stems) {
                                assert!(Arc::ptr_eq(&old.samples, &current.samples));
                            }
                        }
                        (None, None) => {}
                        _ => panic!("stale musical candidate changed current readers"),
                    }
                    assert_eq!(
                        crate::audio_engine::input_runtime_binding::capture(&p.h.engine, 0)
                            .unwrap()
                            .unwrap()
                            .binding
                            .accepted,
                        Some(new_accepted)
                    );
                }
            }
        }
    }
}

#[test]
fn actual_paused_intro_seek_withdrawal_preserves_post_seek_audio() {
    use crate::audio_engine::source_reader::ExplicitSeekMode;
    let target = 0;
    for selected_v1 in [false, true] {
        let mut p = Productive::new(true, 0.73);
        if selected_v1 {
            make_v1_effective(&mut p);
        }
        let _seek = acknowledge_candidate_seek(&mut p, target);
        let source = p.h.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        let extent = if target == 0 {
            (0, LOOP_END)
        } else {
            (LOOP_START, p.h.material.sample.frame_count())
        };
        assert_eq!((source.resident_start(), source.resident_end()), extent);
        assert_eq!(
            source.residency.as_ref().unwrap().context,
            crate::messages::ResidentContext::KeyLockFiniteLoop
        );
        let before = voice(&p.h.callback.mixer);
        assert_eq!(
            before.source_playback.position().seek_mode,
            if target == 0 {
                ExplicitSeekMode::BeforeLoop
            } else {
                ExplicitSeekMode::AfterLoop
            }
        );
        assert!(before.sample.as_ref().unwrap().same_window(&source));
        let clock = before.source_playback;
        let generation = before.generation;
        let native = before.stretch.native_state_address();
        assert_eq!(before.stretch.pending_fifo_frames(), (0, 0));
        assert!(before.stretch.productive_history().is_none());
        assert!(p.h.callback.mixer.dsp_source_history_for_test(0).is_none());
        let old = p.h.callback.mixer.stems_for_measurement()[0].clone();
        let mut oracle = CandidateSeekOracle::after_ack(&mut p, target, selected_v1);
        p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
        assert!(voice(&p.h.callback.mixer).paused);
        let (_pair, registration) = candidate(&mut p, 120 + target + usize::from(selected_v1));
        let receipt = request_candidate(&mut p, &registration, 0b1010);
        assert!(
            receipt.withdraw(),
            "own cancellation before candidate Capture"
        );
        assert_eq!(
            p.h.callback.drain(&mut p.h.consumer),
            1,
            "actual fresh candidate Capture"
        );
        assert_eq!(receipt.residency_status(), "rejected");
        assert!(!receipt.claimed());
        assert!(registration.is_current());
        assert_eq!(registration.publication_status(), "accepted");
        wait_until(|| p.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
        assert!(
            p.h.consumer.is_empty(),
            "rejected candidate enqueues no Adopt"
        );
        let after = voice(&p.h.callback.mixer);
        assert!(after.source_playback.matches_exact(&clock));
        assert_eq!(after.generation, generation);
        assert_eq!(after.stretch.native_state_address(), native);
        assert_eq!(after.stretch.pending_fifo_frames(), (0, 0));
        assert!(after.stretch.productive_history().is_none());
        assert!(p.h.callback.mixer.dsp_source_history_for_test(0).is_none());
        assert!(after.paused);
        p.h.callback.mixer.resume_sample_at_output_frame(0, p.frame);
        for frames in [1, 31, 137, 257, 511] {
            oracle.render(&mut p, frames, false);
        }
        oracle.later_native(&mut p);
        assert_eq!(voice(&p.h.callback.mixer).generation, generation);
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
                assert!(Arc::ptr_eq(
                    &old.complete_set_identity,
                    &current.complete_set_identity
                ));
                for (old, current) in old.stems.iter().zip(&current.stems) {
                    assert!(Arc::ptr_eq(&old.samples, &current.samples));
                }
            }
            (None, None) => {}
            _ => panic!("rejected explicit-Seek candidate changed old reader ownership"),
        }
        assert_eq!(receipt.residency_status(), "rejected");
        assert_eq!(receipt.effective_mode_status(), "rejected");
        assert!(receipt.effective_failure_reason().is_none());
        assert!(registration.is_current() && p.h.consumer.is_empty());
    }
}

// Independent transport expectation: the declared .5 -> 2.0 control trajectory
// applies .05 per 512 consumed output frames. No copied SourcePlayback cursor,
// chunk/advance call or transition helper supplies the expected distance.
struct GlidingSourceOracle {
    control_frame: u64,
    phase: f64,
    binding: crate::audio_engine::productive_source_history::ProductiveSourceBinding,
    dsp_frames: u64,
    wet_frames: Option<u64>,
}

impl GlidingSourceOracle {
    fn new(p: &Productive) -> Self {
        let actual = voice(&p.h.callback.mixer);
        assert_eq!(actual.source_playback.tempo_ratio(), 0.5);
        assert_eq!(actual.source_playback.rate_target(), 0.5);
        let position = actual.source_playback.position();
        let dsp = p.h.callback.mixer.dsp_source_history_for_test(0).unwrap();
        assert_eq!(dsp.next_position, position);
        let history = actual.stretch.productive_history();
        assert_eq!(history.is_some(), p.key_lock);
        Self {
            control_frame: p.frame,
            phase: (position.frame - LOOP_START) as f64 + position.fraction,
            binding: dsp.binding,
            dsp_frames: dsp.fed_output_frames,
            wet_frames: history.map(|history| history.fed_output_frames),
        }
    }

    fn rate_at(output_frame: usize) -> f64 {
        (0.5 + 0.05 * (1 + output_frame / 512) as f64).min(2.0)
    }

    fn distance_at(output_frames: usize) -> f64 {
        let whole_steps = output_frames / 512;
        let whole_distance: f64 = (0..whole_steps)
            .map(|step| 512.0 * Self::rate_at(step * 512))
            .sum();
        whole_distance + (output_frames % 512) as f64 * Self::rate_at(whole_steps * 512)
    }

    fn elapsed(&self, p: &Productive) -> usize {
        (p.frame - self.control_frame) as usize
    }

    fn verify(&self, p: &Productive) {
        let elapsed = self.elapsed(p);
        assert!(elapsed > 0);
        let actual = voice(&p.h.callback.mixer);
        let playback = actual.source_playback;
        // At exactly 512 consumed frames the old step remains observable until
        // the next real frame starts. Target intent itself stays unchanged.
        assert!((playback.tempo_ratio() - Self::rate_at(elapsed - 1)).abs() < 1e-12);
        assert_eq!(playback.rate_target(), 2.0);
        let position = playback.position();
        assert_eq!(
            position.seek_mode,
            crate::audio_engine::source_reader::ExplicitSeekMode::Normal
        );
        let period = (LOOP_END - LOOP_START) as f64;
        let expected = (self.phase + Self::distance_at(elapsed)).rem_euclid(period);
        let observed = (position.frame - LOOP_START) as f64 + position.fraction;
        let difference = (observed - expected).abs();
        assert!(
            difference.min((period - difference).abs()) < 1e-8,
            "independent gliding phase at {elapsed}: actual={observed}, expected={expected}"
        );
        let dsp = p.h.callback.mixer.dsp_source_history_for_test(0).unwrap();
        assert_eq!(dsp.binding, self.binding);
        assert_eq!(dsp.next_position, position);
        assert_eq!(dsp.fed_output_frames, self.dsp_frames + elapsed as u64);
        if p.key_lock && elapsed <= 1536 {
            // Before the deliberate later preparation/unity bypass, the real
            // already-used Native/FIFOs must keep their chronological ledger.
            let history = actual.stretch.productive_history().unwrap();
            assert_eq!(history.binding, self.binding);
            assert_eq!(history.next_position, position);
            assert_eq!(
                history.fed_output_frames,
                self.wet_frames.unwrap() + elapsed as u64
            );
            assert!(actual.stretch.wet_render_ready());
            assert!(actual.stretch.wet_output_ready());
        } else if !p.key_lock {
            assert!(actual.stretch.productive_history().is_none());
            assert!(!actual.stretch.wet_render_ready());
            assert!(!actual.stretch.wet_output_ready());
        }
    }

    fn render(&self, p: &mut Productive, frames: usize, timestamp: bool) -> Vec<f32> {
        // Productive compares every output sample, the exact complete source
        // clock and real adapter FIFO lengths with its independent full PCM mixer.
        let output = p.render(frames, timestamp);
        self.verify(p);
        output
    }
}

fn complete_pcm_selection_at(
    source: &SampleBuffer,
    components: &PreparedStemSet,
    mask: Option<u8>,
    position: crate::audio_engine::source_playback::FractionalSourcePosition,
    channel: usize,
) -> f32 {
    assert_eq!(source.resident_start(), 0);
    let right = if position.frame + 1 == LOOP_END {
        LOOP_START
    } else {
        position.frame + 1
    };
    let knot = |frame: usize| match mask {
        None => source.samples[frame * 2 + channel],
        Some(mask) => components
            .stems
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) != 0)
            .map(|(_, component)| {
                assert_eq!(component.resident_start(), 0);
                component.samples[frame * 2 + channel]
            })
            .sum::<f32>(),
    };
    let left = knot(position.frame);
    left + (knot(right) - left) * position.fraction as f32
}

#[test]
fn gliding_first_and_replacement_candidates_keep_source_distance_native_fifo_and_eq() {
    for key_lock in [false, true] {
        for selected_v1 in [false, true] {
            let mut p = Productive::new(key_lock, 0.5);
            if selected_v1 {
                make_v1_effective(&mut p);
            }
            let oracle = GlidingSourceOracle::new(&p);
            let revision = p.h.engine.global_timing_revision.load(Ordering::Acquire);
            let (mut parameters, mut parameter_consumer) = rtrb::RingBuffer::new(1);
            crate::audio_engine::push_global_timing_message(
                &p.h.engine.global_timing_revision,
                &mut parameters,
                ControlParameterMessage::SetSpeed(2.0),
                "SetSpeed",
            )
            .unwrap();
            assert_eq!(
                p.h.engine.global_timing_revision.load(Ordering::Acquire),
                revision + 1
            );
            let callback = p.h.callback.as_mut();
            crate::audio_engine::audio_stream::drain_parameter_messages(
                &mut parameter_consumer,
                &mut callback.mixer,
                &mut callback.transport,
            );
            assert!(parameter_consumer.is_empty());
            p.reference.set_speed(2.0);

            oracle.render(&mut p, 13, false);
            assert_eq!(oracle.elapsed(&p), 13);
            let (_pair, selected) = candidate(&mut p, 202);
            let old_identity = p.h.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .map(|set| set.complete_set_identity.clone());
            let receipt = request_candidate(&mut p, &selected, 0b1010);
            p.capture_then_wait(&receipt);
            assert!(p.selected.is_current());
            for frames in [31, 257, 1, 198] {
                oracle.render(&mut p, frames, false);
            }
            assert_eq!(oracle.elapsed(&p), 500);
            let before = voice(&p.h.callback.mixer);
            let clock = before.source_playback;
            let generation = before.generation;
            let native = before.stretch.native_state_address();
            let fifo = before.stretch.pending_fifo_frames();
            let history = before.stretch.productive_history();
            let wet = before.stretch.wet_output_ready();
            let dsp = p.h.callback.mixer.dsp_source_history_for_test(0).unwrap();
            assert_eq!(history.is_some(), key_lock);
            assert_eq!(wet, key_lock);
            if key_lock {
                assert_ne!(fifo, (0, 0), "productive adapter state before Adopt");
            } else {
                assert_eq!(fifo, (0, 0));
                assert!(!before.stretch.wet_render_ready());
            }
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            assert_eq!(
                receipt.residency_status(),
                "accepted",
                "{:?}",
                receipt.error()
            );
            assert!(receipt.claimed() && !receipt.withdraw());
            assert!(selected.is_current());
            assert!(!p.selected.is_current());
            let after = voice(&p.h.callback.mixer);
            assert_eq!(after.generation, generation);
            assert!(after.source_playback.matches_exact(&clock));
            assert_eq!(after.stretch.native_state_address(), native);
            assert_eq!(after.stretch.pending_fifo_frames(), fifo);
            assert_eq!(after.stretch.wet_output_ready(), wet);
            if let Some(history) = history {
                let next = after.stretch.productive_history().unwrap();
                assert_eq!(next.binding, history.binding);
                assert_eq!(next.next_position, history.next_position);
                assert_eq!(next.domain, history.domain);
                assert_eq!(next.fed_output_frames, history.fed_output_frames);
            } else {
                assert!(after.stretch.productive_history().is_none());
            }
            let next_dsp = p.h.callback.mixer.dsp_source_history_for_test(0).unwrap();
            assert_eq!(next_dsp.binding, dsp.binding);
            assert_eq!(next_dsp.next_position, dsp.next_position);
            assert_eq!(next_dsp.fed_output_frames, dsp.fed_output_frames);
            if let Some(old_identity) = old_identity {
                assert!(!Arc::ptr_eq(
                    &old_identity,
                    &p.h.callback.mixer.stems_for_measurement()[0]
                        .as_ref()
                        .unwrap()
                        .complete_set_identity
                ));
            }
            p.select_reference("all_stems", reordered_mask(0b1010));
            p.selected = selected;
            oracle.verify(&p);

            let handover_distance = |frames| {
                GlidingSourceOracle::distance_at(500 + frames)
                    - GlidingSourceOracle::distance_at(500)
            };
            let completion = (1..=256)
                .find(|frames| handover_distance(*frames) >= 128.0)
                .unwrap();
            assert_eq!(completion, 215);
            assert!(handover_distance(completion - 1) < 128.0);
            assert!(handover_distance(completion) >= 128.0);
            let mut rendered = 0;
            let mut crossed_step_inside_callback = false;
            for frames in [1_usize, 7, 31, 17].into_iter().cycle() {
                let frames = frames.min(completion - 1 - rendered);
                let before_frame = oracle.elapsed(&p);
                oracle.render(&mut p, frames, false);
                crossed_step_inside_callback |= before_frame < 512 && oracle.elapsed(&p) > 512;
                rendered += frames;
                assert!(
                    p.h.callback
                        .mixer
                        .stem_transition_for_measurement(0)
                        .is_active()
                );
                assert_eq!(voice(&p.h.callback.mixer).generation, generation);
                assert_eq!(
                    voice(&p.h.callback.mixer).stretch.native_state_address(),
                    native
                );
                if rendered == completion - 1 {
                    break;
                }
            }
            // The real 512 boundary fell inside this ramp, rather than at a
            // callback endpoint. 214 output frames consumed only 127.8 source frames.
            assert!(crossed_step_inside_callback);
            assert_eq!(oracle.elapsed(&p), 714);
            assert!((voice(&p.h.callback.mixer).source_playback.tempo_ratio() - 0.6).abs() < 1e-12);
            oracle.render(&mut p, 1, false);
            assert!(
                !p.h.callback
                    .mixer
                    .stem_transition_for_measurement(0)
                    .is_active()
            );
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                native
            );
            assert_eq!(oracle.elapsed(&p), 715);

            // Observe actual selected feed after the ramp. Independently sum
            // and interpolate immutable complete V1 PCM; neither a SourceReadPlan
            // nor the candidate's own V2 buffers supplies these expectations.
            // Single-frame callbacks make buffer[0] the executed current feed,
            // avoiding inspection of stale prefixes from a multi-chunk callback.
            let mut difference_from_full = 0.0_f32;
            let mut difference_from_v1 = 0.0_f32;
            let mut output = Vec::new();
            for _ in 0..31 {
                let position = voice(&p.h.callback.mixer).source_playback.position();
                let full = &p.h.material.sample;
                let components = p.reference.stems_for_measurement()[0].as_ref().unwrap();
                let expected: [f32; 2] = std::array::from_fn(|channel| {
                    complete_pcm_selection_at(
                        full,
                        components,
                        Some(reordered_mask(0b1010)),
                        position,
                        channel,
                    )
                });
                let full_mix: [f32; 2] = std::array::from_fn(|channel| {
                    complete_pcm_selection_at(full, components, None, position, channel)
                });
                let v1: [f32; 2] = std::array::from_fn(|channel| {
                    complete_pcm_selection_at(full, components, Some(0b1010), position, channel)
                });
                output.extend(oracle.render(&mut p, 1, false));
                for channel in 0..2 {
                    let actual = voice(&p.h.callback.mixer).stretch.varispeed_buffers()[channel][0];
                    assert_eq!(actual, expected[channel]);
                    difference_from_full =
                        difference_from_full.max((actual - full_mix[channel]).abs());
                    difference_from_v1 = difference_from_v1.max((actual - v1[channel]).abs());
                }
            }
            assert!(difference_from_full > 0.001);
            assert!(difference_from_v1 > 0.001);
            output.extend(oracle.render(&mut p, 777, false));
            assert!(output.iter().any(|value| value.abs() > 0.001));
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                native
            );

            if key_lock {
                // Crossing ratio 1.0 legitimately uses the existing dry bypass
                // and cancels wet work. Start a genuine later source preparation
                // after that interval, while smoothing toward 2.0 still continues.
                let mut remaining = 5121 - oracle.elapsed(&p);
                for frames in [31_usize, 257, 1, 127].into_iter().cycle() {
                    let frames = frames.min(remaining);
                    oracle.render(&mut p, frames, false);
                    remaining -= frames;
                    if remaining == 0 {
                        break;
                    }
                }
                assert!(
                    (voice(&p.h.callback.mixer).source_playback.tempo_ratio() - 1.05).abs() < 1e-12
                );
                let before_preparation = voice(&p.h.callback.mixer).stretch.native_state_address();
                oracle.render(&mut p, 13, true);
                // This helper waits for both actual workers, checks their target
                // and finite taps, adopts the new actual Native, then compares
                // eight productive Native blocks with uninterrupted complete PCM.
                p.adopt_native();
                oracle.verify(&p);
                let current = voice(&p.h.callback.mixer);
                assert_ne!(current.stretch.native_state_address(), before_preparation);
                assert!(current.source_playback.tempo_ratio() < 2.0);
                assert_eq!(current.source_playback.rate_target(), 2.0);
                assert!(current.stretch.wet_render_ready());
                assert!(current.stretch.wet_current_source_output_ready());
                let history = current.stretch.productive_history().unwrap();
                assert_eq!(history.binding, oracle.binding);
                assert_eq!(history.next_position, current.source_playback.position());
            }
            assert_eq!(voice(&p.h.callback.mixer).generation, generation);
            assert_eq!(
                receipt.effective_mode_status(),
                "effective",
                "{:?}",
                receipt.error()
            );
        }
    }
}
