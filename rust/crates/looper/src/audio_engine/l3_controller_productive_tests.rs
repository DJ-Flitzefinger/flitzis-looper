//! Genuine StemController -> Native candidate Capture/Adopt -> continuous output.
//! V2/V3 permute real files; independent complete V1 PCM supplies the
//! algebraic feed, raw Native/FIFO or dry Varispeed, and uninterrupted EQ oracle.

use super::*;
use crate::audio_engine::buffer_retirement::AudioBufferRetirement;

const CHILD: &str = "FLITZI_L3_CONTROLLER_CANDIDATE_CHILD";
const TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_first_and_replacement_candidates_keep_own_wet_chronology";
const VARISPEED_CHILD: &str = "FLITZI_L3_CONTROLLER_VARISPEED_CHILD";
const VARISPEED_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_keylock_off_first_and_replacement_keep_varispeed_feed_and_eq";
const STOPPED_NEW_SET_CHILD: &str = "FLITZI_L3_CONTROLLER_STOPPED_NEW_SET_CHILD";
const STOPPED_NEW_SET_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_stopped_replacement_play_then_fresh_all_uses_own_wet_receipt";
const PAUSED_FIRST_CHILD: &str = "FLITZI_L3_CONTROLLER_PAUSED_FIRST_CHILD";
const PAUSED_FIRST_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_wet_full_mix_paused_before_first_candidate_keeps_consumed_clock";
const REPEATED_SETS_CHILD: &str = "FLITZI_L3_CONTROLLER_REPEATED_SETS_CHILD";
const REPEATED_SETS_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_empty_handover_defers_v3_and_uses_latest_mask_with_own_ack";
const WITHDRAW_PRECLAIM_CHILD: &str = "FLITZI_L3_CONTROLLER_WITHDRAW_PRECLAIM_CHILD";
const WITHDRAW_PRECLAIM_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_full_mix_withdraws_queued_v2_adopt_then_all_uses_fresh_authority";
const ADMISSION_PRESSURE_CHILD: &str = "FLITZI_L3_CONTROLLER_ADMISSION_PRESSURE_CHILD";
const ADMISSION_PRESSURE_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_queue_full_keeps_v1_then_explicit_all_admits_own_v2";

#[derive(Clone, Copy)]
enum CandidateTrace {
    Ordinary,
    RepeatedSets,
    WithdrawPreclaim,
    AdmissionPressure,
}

enum FixtureSet {
    Second,
    Third,
}

fn v2_files(
    engine: &AudioEngine,
    root: &Path,
    version: &str,
    original_reference: &str,
    frames: usize,
) -> (PreparedStemPair, PreparedSourceTicket) {
    version_files(
        engine,
        root,
        version,
        original_reference,
        frames,
        FixtureSet::Second,
    )
}

fn version_files(
    engine: &AudioEngine,
    root: &Path,
    version: &str,
    original_reference: &str,
    frames: usize,
    set: FixtureSet,
) -> (PreparedStemPair, PreparedSourceTicket) {
    let (generation, first, second, instrumental) = match set {
        FixtureSet::Second => (0x301_u32, "vocals", "drums", [0, 1, 2]),
        FixtureSet::Third => (0x302_u32, "melody", "bass", [2, 1, 3]),
    };
    let generation = format!("{generation:032x}");
    let reference = original_reference.replace(GENERATION, &generation);
    let path = root.parent().unwrap().join(&reference);
    write_signal_set(&path, frames, version, 0);
    let first_path = path.join(format!("{first}.wav"));
    let second_path = path.join(format!("{second}.wav"));
    let first_contents = fs::read(&first_path).unwrap();
    let second_contents = fs::read(&second_path).unwrap();
    fs::write(first_path, second_contents).unwrap();
    fs::write(second_path, first_contents).unwrap();
    fs::write(
        path.join("instrumental.wav"),
        signal_sum_wav(frames, &instrumental),
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
            "schema": "stem-set-sha256-v1", "source_version": version, "stems": hashes,
        }))
        .unwrap(),
    )
    .unwrap();
    let source = engine
        .capture_prepared_source(0, version.to_owned())
        .unwrap();
    let pair = engine
        .prepare_stem_pair_at_root(root, 0, version, &reference, &source, false, None)
        .unwrap();
    assert!(!pair.has_components());
    assert_eq!(
        pair.pair_reader.descriptor.content.alignment.offset_frames,
        0
    );
    assert_complete_selection(root, &selection(&pair));
    (pair, source)
}

fn commit_candidate_v2(
    engine: &AudioEngine,
    root: &Path,
    version: &str,
    original_reference: &str,
    frames: usize,
    locals: &Py<PyDict>,
) {
    let (v2, source_ticket) = v2_files(engine, root, version, original_reference, frames);
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("candidate_json", v2.selection_json().to_owned())
            .unwrap();
        locals
            .bind(py)
            .set_item("prepared_v2", Py::new(py, v2).unwrap())
            .unwrap();
        locals
            .bind(py)
            .set_item("source_v2", Py::new(py, source_ticket).unwrap())
            .unwrap();
    });
    controller_python(
        locals,
        r#"
disk_v2 = StemPairSelection.model_validate_json(candidate_json)
assert disk_v2 != disk_v1
entry_v1 = project.stem_cache[0]
entry_v2 = StemCacheEntry(source_version=fixture['source_version'], cache_dir=disk_v2.wav_generation,
    stems=expected_stem_files(disk_v2.wav_generation), pair=disk_v2, available=True)
# Both real complete assignment owners survive the actual fenced config replacement.
owner_v1 = audio.acquire_project_asset_lease(str(Path(entry_v1.cache_dir).absolute()))
owner_v2 = audio.acquire_project_asset_lease(str(Path(entry_v2.cache_dir).absolute()))
persistence.commit_stem_selection(0, entry_v1, entry_v2,
    lambda: project.stem_cache[0] is entry_v1 and source_v2.same_source_request(
        audio.capture_prepared_source(0, fixture['source_version'])))
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None
assert reopened.project.stem_cache[0].pair == disk_v2
project.stem_cache[0] = entry_v2
prepared_v2.select()
controller._register_committed_pair(0, prepared_v2, source_v2)
live = controller._committed_residency._live_selections[0]
assert live.control is previous_control and live.attempt is None
assert live.disk.selection.ticket.publication_status() == 'accepted'
assert previous_control.ticket.is_current()
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v1.stem_set_identity
"#,
    );
}

fn complete_reference(
    full: &SampleBuffer,
    stems: &PreparedStemSet,
    ratio: f64,
    key_lock_enabled: bool,
) -> Box<RtMixer> {
    let mut mixer = Box::new(RtMixer::new(2, RATE as f32));
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_source(0, full, RATE, 1);
    mixer.set_input_runtime_ownership(ownership.clone());
    mixer.load_sample(0, full.clone());
    assert!(mixer.publish_prepared_stems(0, stems.clone()));
    mixer.set_loop_region_frames(0, LOOP_START, Some(LOOP_END));
    mixer.set_speed(ratio);
    let key_lock = ownership.next_key_lock_request(0).unwrap();
    mixer.set_pad_key_lock_request(
        0,
        key_lock_enabled,
        key_lock,
        InputPadBinding {
            resident: full.resident_binding(),
            source_address: full.source_address(),
            sample_count: full.source_sample_count(),
            channels: full.channels,
            sample_rate_hz: RATE,
            authority_revision: ownership.authority[0].load(Ordering::Acquire),
            runtime_revision: 0,
            accepted: None,
        },
        1,
    );
    mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

fn own_adoption(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    name: &str,
    key_lock: bool,
) {
    assert_eq!(playback.drain(consumer), 1, "own callback Capture");
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, name, "residency_status") == "rejected"
    });
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "pending"
    );
    if voice(&playback.callback.mixer).paused {
        playback.render_paused(31);
    } else if key_lock {
        playback.render(31, false);
    } else {
        render_varispeed(playback, 31);
    }
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    assert_eq!(history.is_some(), key_lock);
    assert_eq!(playback.drain(consumer), 1, "own callback Adopt");
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "accepted"
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "transitioning"
    );
    let after = voice(&playback.callback.mixer);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.generation, generation);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let next_history = after.stretch.productive_history().unwrap();
        assert_eq!(next_history.binding, history.binding);
        assert_eq!(next_history.next_position, history.next_position);
        assert_eq!(next_history.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(after.stretch.productive_history().is_none());
    }
}

fn render_varispeed(playback: &mut ControllerPlayback, frames: usize) -> (Vec<f32>, Vec<f32>) {
    assert!(!playback.callback.mixer.key_lock_for_measurement(0));
    assert!(playback.raw.is_none() && playback.candidate.is_none());
    assert!(frames <= playback.callback.mixer.max_realtime_render_frames());
    // The clock, interpolation, selected sum and ramp gains come from complete
    // independent PCM, not from the actual SourcePlayback or SourceReadPlan.
    let feed = playback.feed(playback.source_frame, frames);
    let before = voice(&playback.callback.mixer);
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let mut output = vec![0.0; frames * 2];
    let mut expected = vec![0.0; frames * 2];
    let mut full_mix = vec![0.0; frames * 2];
    reset_tap_observation_for_test();
    playback.callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        playback.frame,
        &mut playback.callback.retirement,
    );
    let taps = tap_observation_for_test();
    assert_eq!(taps.missing_reads, 0);
    assert!(taps.min_frame.is_some_and(|frame| frame >= LOOP_START));
    assert!(taps.max_frame.is_some_and(|frame| frame < LOOP_END));
    for (mixer, target) in [
        (playback.reference.as_mut(), &mut expected),
        (playback.full_mix.as_mut(), &mut full_mix),
    ] {
        mixer.render_at_output_frame(playback.frame, target, &mut [0.0; NUM_SAMPLES]);
    }
    assert_eq!(
        output, expected,
        "controller Varispeed chronology at {}",
        playback.frame
    );
    let current = voice(&playback.callback.mixer);
    for (channel, expected) in feed.iter().enumerate() {
        assert_eq!(
            &current.stretch.varispeed_buffers()[channel][..frames],
            expected.as_slice()
        );
        assert_eq!(
            &current.stretch.output_buffers()[channel][..frames],
            expected.as_slice()
        );
    }
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    assert!(current.stretch.productive_history().is_none());
    assert!(!current.stretch.wet_render_ready());
    assert!(current.stretch.adopted_request_id().is_none());
    assert!(
        current
            .source_playback
            .matches_exact(&voice(&playback.reference).source_playback)
    );
    assert!(
        current
            .source_playback
            .matches_exact(&voice(&playback.full_mix).source_playback)
    );
    assert_eq!(
        current.stretch.pending_fifo_frames(),
        voice(&playback.reference).stretch.pending_fifo_frames()
    );
    let phase = ((playback.source_frame + frames as u64) as f64 * playback.ratio)
        .rem_euclid((LOOP_END - LOOP_START) as f64);
    assert_eq!(
        current.source_playback.position().frame,
        LOOP_START + phase.floor() as usize
    );
    assert_eq!(current.source_playback.position().fraction, phase.fract());
    for (frame, actual) in output.chunks_exact(2).enumerate() {
        playback.filter.begin_frame();
        for channel in 0..2 {
            assert_eq!(
                actual[channel],
                playback
                    .filter
                    .process_sample(channel, feed[channel][frame]),
                "complete PCM Varispeed + uninterrupted EQ at {}",
                playback.frame
            );
        }
    }
    playback.frame += frames as u64;
    playback.source_frame += frames as u64;
    (output, full_mix)
}

fn ramp_varispeed(playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str) {
    assert_eq!(playback.ratio, 1.25);
    let mut remaining = (128.0 / playback.ratio).ceil() as usize;
    for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
        let count = frames.min(remaining);
        render_varispeed(playback, count);
        remaining -= count;
        if remaining == 0 {
            break;
        }
        assert_eq!(
            controller_ticket_status(locals, name, "effective_mode_status"),
            "transitioning"
        );
        assert!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
    }
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
}

fn settle_varispeed(playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str) {
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let fifo = voice(&playback.callback.mixer)
        .stretch
        .pending_fifo_frames();
    ramp_varispeed(playback, locals, name);
    // OFF effectiveness follows actual dry source consumption. No Wet or 4096
    // Native preparation is invoked or used as evidence for this branch.
    let mut output = Vec::new();
    let mut counterfactual = Vec::new();
    for frames in [1, 127, 257, 511, 31, 137] {
        let (actual, full_mix) = render_varispeed(playback, frames);
        output.extend(actual);
        counterfactual.extend(full_mix);
    }
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    assert!(
        output
            .iter()
            .zip(counterfactual)
            .any(|(actual, full_mix)| (actual - full_mix).abs() > 0.005)
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        native
    );
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .pending_fifo_frames(),
        fifo
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "effective"
    );
    controller_python(
        locals,
        "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
    );
}

fn settle_wet(playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str) {
    playback.ramp(locals, name, playback.ratio);
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    // This real timestamped feed starts a fresh ordinary Native preparation.
    // adopt_native independently reconstructs its complete 4096-frame raw feed.
    playback.render(13, true);
    let (actual, full_mix) = playback.adopt_native(false);
    assert!(actual.iter().any(|value| value.abs() > 0.001));
    assert!(
        actual
            .iter()
            .zip(full_mix)
            .any(|(actual, full)| (actual - full).abs() > 0.005)
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "effective"
    );
    controller_python(
        locals,
        "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
    );
}

fn settle_empty(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) {
    assert_eq!(playback.selection, Some(0));
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    if key_lock {
        playback.ramp(locals, "candidate", playback.ratio);
        let empty_source_start = playback.source_frame;
        // Old Wet/FIFO samples and filter tails need not be silent. All samples
        // still match the uninterrupted independent raw-Native/EQ oracle.
        playback.render(13, true);
        wait_until(|| {
            voice_mut(&mut playback.callback.mixer)
                .stretch
                .source_preparation_ready()
        });
        let target = voice_mut(&mut playback.callback.mixer)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        let history_frames = crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES;
        assert_eq!(history_frames, 4096);
        let warm_source_start =
            target - history_frames as u64 - (playback.frame - playback.source_frame);
        assert!(
            warm_source_start >= empty_source_start,
            "own complete preparation follows the empty ramp"
        );
        assert!(
            playback
                .feed(warm_source_start, history_frames)
                .iter()
                .flatten()
                .all(|sample| *sample == 0.0)
        );
        controller_python(
            locals,
            r#"
assert candidate.effective_mode_status() == 'transitioning'
controller.on_frame_render()
assert live.disk.selection is None and live.disk.registration == 'deferred'
assert live.control is v2_control and live.attempt is v2_control
assert v2_control.receipt is candidate and v2_control.receipt_intent == ('all_stems', 0)
assert live.desired == ('all_stems', 0b1010)
assert candidate.claimed() and candidate.is_current()
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v3.stem_set_identity
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == disk_v1.stem_set_identity
assert status.registration == 'deferred' and status.effective == 'transitioning'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
        );
        assert!(
            consumer.is_empty(),
            "V3 stays deferred after the ramp while own Native is prepared"
        );
        playback.adopt_native(false);
        assert_ne!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert!(
            voice(&playback.callback.mixer)
                .stretch
                .wet_current_source_output_ready()
        );
        playback.render(127, true);
    } else {
        ramp_varispeed(playback, locals, "candidate");
        render_varispeed(playback, 127);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
    }
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert!(
        playback
            .feed(playback.source_frame, 127)
            .iter()
            .flatten()
            .all(|sample| *sample == 0.0)
    );
    let actual = voice(&playback.callback.mixer);
    for channel in 0..2 {
        // Only the freshly rendered prefix is output, not unused scratch space.
        assert!(
            actual.stretch.varispeed_buffers()[channel][..127]
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert!(
            actual.stretch.output_buffers()[channel][..127]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }
    assert_eq!(
        controller_ticket_status(locals, "candidate", "effective_mode_status"),
        "effective"
    );
    // Do not poll the effective receipt yet: V2's own effectiveness precedes
    // any V3 registration, independently of the earlier pre-Native poll.
}

fn settle_full_mix(playback: &mut ControllerPlayback, locals: &Py<PyDict>, key_lock: bool) {
    assert_eq!(playback.selection, None);
    let generation = voice(&playback.callback.mixer).generation;
    let mut output = Vec::new();
    if key_lock {
        playback.ramp(locals, "full", playback.ratio);
        playback.render(13, true);
        output.extend(playback.adopt_native(false).0);
        assert!(
            voice(&playback.callback.mixer)
                .stretch
                .wet_current_source_output_ready()
        );
    } else {
        ramp_varispeed(playback, locals, "full");
        for frames in [1, 127, 257, 511, 31, 137] {
            output.extend(render_varispeed(playback, frames).0);
        }
    }
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(
        controller_ticket_status(locals, "full", "effective_mode_status"),
        "effective"
    );
    controller_python(
        locals,
        "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
    );
}

fn withdraw_queued_candidate_for_full_mix(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) {
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    assert_eq!(
        playback.drain(consumer),
        1,
        "actual V2 Capture before withdrawal"
    );
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, "candidate", "residency_status") == "rejected"
    });
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if matches!(work.as_ref(), CommittedStemRtWork::Adopt(transaction)
            if transaction.id == 0 && transaction.target == StemMixMode::AllStems
            && transaction.mask == 0b1010 && transaction.candidate.is_some()))
    );
    controller_python(
        locals,
        r#"
withdrawn = candidate
withdrawn_registration = live.disk.selection.ticket
assert withdrawn.residency_status() == 'pending' and withdrawn.effective_mode_status() == 'waiting'
assert withdrawn.is_current() and not withdrawn.claimed()
assert controller.set_stem_mix_mode(0, 'full_mix')
full = live.control.receipt
assert full is not withdrawn and full.is_current()
assert full.residency_status() in {'captured', 'pending'} and full.effective_mode_status() == 'waiting'
assert live.control is previous_control and live.attempt is previous_control
assert live.control.receipt_intent == ('full_mix', 0b1010)
assert live.disk.selection is None and live.disk.registration == 'withdrawn'
assert live.desired == ('full_mix', 0b1010)
assert withdrawn.residency_status() == 'rejected' and withdrawn.effective_mode_status() == 'rejected'
assert not withdrawn.claimed() and not withdrawn.is_current()
assert withdrawn.effective_failure_reason() is None
assert withdrawn_registration.publication_status() == 'accepted' and not withdrawn_registration.is_current()
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v1.stem_set_identity
assert status.effective_stem_set_identity == disk_v1.stem_set_identity
assert project.pad_stem_mix_mode[0] == 'full_mix' and project.stem_cache[0].pair == disk_v2
persistence.flush()
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None and reopened.project.stem_cache[0].pair == disk_v2
assert reopened.project.pad_stem_mix_mode[0] == 'full_mix'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    let set = playback.callback.mixer.stems_for_measurement()[0]
        .clone()
        .unwrap();
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let message = consumer.pop().unwrap();
    assert!(matches!(&message, ControlMessage::CommittedStemWork(work)
        if matches!(work.as_ref(), CommittedStemRtWork::Adopt(transaction)
            if !transaction.selection.current() && transaction.stems.is_some())));
    let callback = playback.callback.as_mut();
    assert!(callback.retirement.available_retirement_slots() >= 3);
    // The unchanged actual queued V2 message reaches the same Native handler
    // as drain_control_messages, without also draining the later FullMix Capture.
    crate::audio_engine::audio_stream::process_control_message(
        message,
        &mut callback.scheduler,
        playback.frame,
        &mut callback.quantization,
        &mut callback.transport,
        &mut callback.mixer,
        &mut callback.feedback,
        &mut callback.retirement,
    );
    let current = voice(&callback.mixer);
    assert!(current.source_playback.matches_exact(&clock));
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let after = current.stretch.productive_history().unwrap();
        assert_eq!(after.binding, history.binding);
        assert_eq!(after.next_position, history.next_position);
        assert_eq!(after.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(current.stretch.productive_history().is_none());
    }
    let after_dsp = callback.mixer.dsp_source_history_for_test(0).unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    assert_eq!(
        callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::AllStems
    );
    assert_eq!(callback.mixer.stem_demand_for_measurement(0).1, 0b1010);
    let current_set = callback.mixer.stems_for_measurement()[0].as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &current_set.complete_set_identity
    ));
    for (before, after) in set.stems.iter().zip(&current_set.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if matches!(work.as_ref(), CommittedStemRtWork::Capture(capture)
            if capture.id == 0 && capture.target == StemMixMode::FullMix))
    );
    controller_python(
        locals,
        r#"
controller.on_frame_render()
assert not withdrawn.claimed() and withdrawn.residency_status() == 'rejected'
assert live.control is previous_control and live.attempt is previous_control
assert full.effective_mode_status() == 'waiting'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    if key_lock {
        playback.render(31, false);
    } else {
        render_varispeed(playback, 31);
    }
    own_adoption(playback, consumer, locals, "full", key_lock);
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::FullMix, 0)
    );
    playback.select(None);
    controller_python(
        locals,
        r#"
controller.on_frame_render()
assert live.control is previous_control and live.control.receipt is full
assert full.claimed() and full.effective_mode_status() == 'transitioning'
assert controller.stem_residency_status(0).effective_stem_set_identity == disk_v1.stem_set_identity
"#,
    );
    settle_full_mix(playback, locals, key_lock);
    controller_python(
        locals,
        r#"
for _ in range(16):
    controller.on_frame_render()
    assert live.control is previous_control and live.attempt is None
    assert live.disk.selection is None and live.disk.registration == 'withdrawn'
    status = controller.stem_residency_status(0)
    assert status.disk_stem_set_identity == disk_v2.stem_set_identity
    assert status.control_stem_set_identity == disk_v1.stem_set_identity
    assert status.effective_stem_set_identity is None
    assert status.requested_mode == 'full_mix' and status.effective == 'effective'
assert full.is_current() and full.residency_status() == 'accepted'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert!(consumer.is_empty(), "poll cannot retry withdrawn V2 ALL");
    controller_python(
        locals,
        r#"
assert controller.set_stem_mix_mode(0, 'all_stems')
fresh_registration = live.disk.selection.ticket
candidate = live.attempt.receipt
assert fresh_registration is not withdrawn_registration and fresh_registration.is_current()
assert fresh_registration.publication_status() == 'accepted'
assert candidate is not withdrawn and candidate is not full
assert candidate.is_current() and not candidate.claimed()
assert candidate.residency_status() in {'captured', 'pending'} and candidate.effective_mode_status() == 'waiting'
assert live.control is previous_control
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v1.stem_set_identity
assert status.effective_stem_set_identity is None
assert project.pad_stem_mix_mode[0] == 'all_stems'
persistence.flush()
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None and reopened.project.stem_cache[0].pair == disk_v2
assert reopened.project.pad_stem_mix_mode[0] == 'all_stems'
"#,
    );
}

fn full_queue_then_explicit_retry(
    engine: &AudioEngine,
    producer: &Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) {
    assert!(consumer.is_empty());
    let set = playback.callback.mixer.stems_for_measurement()[0]
        .clone()
        .unwrap();
    let source = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let demand = playback.callback.mixer.stem_demand_for_measurement(0);
    assert_eq!(demand.0, StemMixMode::AllStems);
    assert_eq!(demand.1, 0b1010);
    let intent = engine.resident_stem_cache.lock().unwrap()[0]
        .intent_epoch
        .load(Ordering::Acquire);
    {
        let mut queue = producer.lock().unwrap();
        // Ping uses this actual control queue; Volume uses a different queue.
        for _ in 0..8 {
            queue.push(ControlMessage::Ping()).unwrap();
        }
        assert!(queue.is_full());
    }
    controller_python(
        locals,
        r#"
pressure_selection = live.disk.selection
pressure_registration = pressure_selection.ticket
admission_error = 'Stem residency admission failed: committed residency queue is full'
assert first.is_current() and first.effective_mode_status() == 'effective'
assert pressure_registration.is_current() and pressure_registration.publication_status() == 'accepted'
assert pressure_selection.receipt is None and pressure_selection.receipt_intent is None
assert not controller.set_stem_mix_mode(0, 'all_stems')
for _ in range(16):
    controller.on_frame_render()
    assert live.blocked and live.desired == ('all_stems', 0b1010)
    assert live.disk.selection is pressure_selection and live.attempt is None
    assert pressure_selection.receipt is None and pressure_selection.receipt_intent is None
    assert pressure_registration.is_current() and pressure_registration.publication_status() == 'accepted'
    assert live.control is previous_control and previous_control.receipt is first
    assert previous_control.ticket.is_current() and first.is_current()
    assert first.residency_status() == 'accepted' and first.effective_mode_status() == 'effective'
    status = controller.stem_residency_status(0)
    assert status.disk_stem_set_identity == disk_v2.stem_set_identity
    assert status.control_stem_set_identity == disk_v1.stem_set_identity
    assert status.effective_stem_set_identity == disk_v1.stem_set_identity
    assert status.attempt_stem_set_identity == disk_v1.stem_set_identity
    assert status.effective == 'effective' and status.desired_mode == 'all_stems'
    assert session.stem_generation_errors == {0: admission_error}
assert project.stem_cache[0].pair == disk_v2 and project.pad_stem_mix_mode[0] == 'all_stems'
"#,
    );
    assert!(matches!(consumer.peek(), Ok(ControlMessage::Ping())));
    assert_eq!(playback.drain(consumer), 8, "only the eight actual Pings");
    assert!(consumer.is_empty(), "failed admission queued no Capture");
    assert_eq!(
        engine.resident_stem_cache.lock().unwrap()[0]
            .intent_epoch
            .load(Ordering::Acquire),
        intent
    );
    assert!(!engine.input_runtime_ownership.resident_control_pending(0));
    let current = voice(&playback.callback.mixer);
    assert!(current.source_playback.matches_exact(&clock));
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let after = current.stretch.productive_history().unwrap();
        assert_eq!(after.binding, history.binding);
        assert_eq!(after.next_position, history.next_position);
        assert_eq!(after.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(current.stretch.productive_history().is_none());
    }
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        demand
    );
    assert!(Arc::ptr_eq(
        &source.samples,
        &playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    let after = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &after.complete_set_identity
    ));
    for (before, after) in set.stems.iter().zip(&after.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    let mut output = Vec::new();
    for frames in [1, 31, 137, 257, 511] {
        output.extend(if key_lock {
            playback.render(frames, false).0
        } else {
            render_varispeed(playback, frames).0
        });
        controller_python(
            locals,
            r#"
for _ in range(16):
    controller.on_frame_render()
    assert live.blocked and live.attempt is None and live.disk.selection is pressure_selection
    assert pressure_selection.receipt is None and pressure_selection.receipt_intent is None
    assert first.is_current() and first.effective_mode_status() == 'effective'
    assert controller.stem_residency_status(0).effective_stem_set_identity == disk_v1.stem_set_identity
    assert session.stem_generation_errors == {0: admission_error}
"#,
        );
        assert!(
            consumer.is_empty(),
            "drained queue does not authorize polling retry"
        );
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
    }
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    controller_python(
        locals,
        r#"
assert controller.set_stem_mix_mode(0, 'all_stems')
assert not live.blocked and live.attempt is pressure_selection
assert live.disk.selection is pressure_selection and pressure_selection.ticket is pressure_registration
candidate = pressure_selection.receipt
assert candidate is not first and candidate.is_current() and not candidate.claimed()
assert candidate.residency_status() in {'captured', 'pending'} and candidate.effective_mode_status() == 'waiting'
assert pressure_selection.receipt_intent == ('all_stems', 0b1010)
assert live.control is previous_control and first.is_current()
status = controller.stem_residency_status(0)
assert status.effective == 'waiting' and status.attempt_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v1.stem_set_identity
assert status.effective_stem_set_identity == disk_v1.stem_set_identity
assert session.stem_generation_errors == {0: admission_error}
"#,
    );
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if matches!(work.as_ref(), crate::audio_engine::committed_stem_residency::CommittedStemRtWork::Capture(capture)
            if capture.id == 0 && capture.target == StemMixMode::AllStems))
    );
}

fn repeated_sets_after_empty_claim(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    full_stems: &PreparedStemSet,
    predecessor: &PreparedStemSet,
    key_lock: bool,
) {
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    assert_eq!(
        transition.required_from_selection(),
        Some(
            crate::audio_engine::source_reader::StemRenderSelection::for_stems(predecessor, 0b1010)
        )
    );
    let v2 = playback.callback.mixer.stems_for_measurement()[0]
        .clone()
        .unwrap();
    assert!(!Arc::ptr_eq(
        &predecessor.complete_set_identity,
        &v2.complete_set_identity
    ));
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    controller_python(
        locals,
        r#"
v2_control = live.control
assert live.attempt is v2_control and v2_control.receipt is candidate
assert v2_control.receipt_intent == ('all_stems', 0)
for mask in (0b0101, 0, 0b1010):
    assert controller.set_stem_enabled_mask(0, mask)
    assert session.pad_stem_enabled_mask[0] == mask
    assert live.desired == ('all_stems', mask)
    assert live.control is v2_control and live.attempt is v2_control
    assert v2_control.receipt is candidate and v2_control.receipt_intent == ('all_stems', 0)
    assert candidate.claimed() and candidate.is_current()
    assert candidate.effective_mode_status() == 'transitioning'

disk_v3 = StemPairSelection.model_validate_json(third_json)
assert len({disk_v1.stem_set_identity, disk_v2.stem_set_identity, disk_v3.stem_set_identity}) == 3
assert len({disk_v1.wav_generation, disk_v2.wav_generation, disk_v3.wav_generation}) == 3
entry_v3 = StemCacheEntry(source_version=fixture['source_version'], cache_dir=disk_v3.wav_generation,
    stems=expected_stem_files(disk_v3.wav_generation), pair=disk_v3, available=True)
owner_v3 = audio.acquire_project_asset_lease(str(Path(entry_v3.cache_dir).absolute()))
persistence.commit_stem_selection(0, entry_v2, entry_v3,
    lambda: project.stem_cache[0] is entry_v2 and source_v3.same_source_request(
        audio.capture_prepared_source(0, fixture['source_version'])))
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None and reopened.project.stem_cache[0].pair == disk_v3
project.stem_cache[0] = entry_v3
prepared_v3.select()
controller._register_committed_pair(0, prepared_v3, source_v3)
assert controller.set_stem_mix_mode(0, 'all_stems')
for _ in range(16):
    controller.on_frame_render()
    assert live.disk.selection is None and live.disk.registration == 'deferred'
    assert live.control is v2_control and live.attempt is v2_control
    assert live.desired == ('all_stems', 0b1010)
    assert candidate.claimed() and candidate.is_current() and not candidate.withdraw()
    status = controller.stem_residency_status(0)
    assert status.disk_stem_set_identity == disk_v3.stem_set_identity
    assert status.control_stem_set_identity == disk_v2.stem_set_identity
    assert status.attempt_stem_set_identity == disk_v2.stem_set_identity
    assert status.effective_stem_set_identity == disk_v1.stem_set_identity
    assert status.residency == 'accepted' and status.effective == 'transitioning'
    assert status.registration == 'deferred'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert!(
        consumer.is_empty(),
        "no V3 callback command while V2 is claimed"
    );
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    let current = voice(&playback.callback.mixer);
    assert!(current.source_playback.matches_exact(&clock));
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    let held = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(
        &held.complete_set_identity,
        &v2.complete_set_identity
    ));
    for (before, after) in v2.stems.iter().zip(&held.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    assert_eq!(playback.callback.mixer.stem_demand_for_measurement(0).1, 0);
    settle_empty(playback, consumer, locals, key_lock);
    assert!(consumer.is_empty());
    controller_python(
        locals,
        r#"
assert candidate.residency_status() == 'accepted' and candidate.effective_mode_status() == 'effective'
assert live.disk.selection is None and live.control is v2_control
controller.on_frame_render()
third_control = live.disk.selection
third = live.attempt.receipt
assert third_control is not v2_control and third is not candidate
assert third_control.receipt_intent == ('all_stems', 0b1010)
assert third_control.ticket.publication_status() == 'accepted' and third.is_current()
assert third.residency_status() in {'captured', 'pending'} and third.effective_mode_status() == 'waiting'
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v3.stem_set_identity
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.attempt_stem_set_identity == disk_v3.stem_set_identity
assert status.effective_stem_set_identity == disk_v2.stem_set_identity
assert status.effective == 'waiting'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert!(
        Arc::ptr_eq(
            &playback.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap()
                .complete_set_identity,
            &v2.complete_set_identity
        ),
        "V2 ACK cannot install V3"
    );
    own_adoption(playback, consumer, locals, "third", key_lock);
    let v3 = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(!Arc::ptr_eq(
        &v2.complete_set_identity,
        &v3.complete_set_identity
    ));
    assert!(!Arc::ptr_eq(
        &predecessor.complete_set_identity,
        &v3.complete_set_identity
    ));
    for (before, after) in v2.stems.iter().zip(&v3.stems) {
        assert!(!Arc::ptr_eq(&before.samples, &after.samples));
    }
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).1,
        0b1010
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    // Real V3 swaps melody/bass: actual1010 means independent V1 bass+drums.
    assert!(
        playback
            .reference
            .set_stem_enabled_mask(0, 0b1100, full_stems.source_version_hash)
    );
    playback.select(Some(0b1100));
    controller_python(
        locals,
        r#"
controller.on_frame_render()
status = controller.stem_residency_status(0)
assert live.control is third_control and live.attempt is third_control
assert status.control_stem_set_identity == disk_v3.stem_set_identity
assert status.effective_stem_set_identity == disk_v2.stem_set_identity
assert status.effective == 'transitioning'
assert third.claimed() and not third.withdraw()
"#,
    );
    if key_lock {
        settle_wet(playback, locals, "third");
    } else {
        settle_varispeed(playback, locals, "third");
    }
    controller_python(
        locals,
        r#"
status = controller.stem_residency_status(0)
assert status.registration == 'accepted' and status.residency == 'accepted'
assert status.effective == 'effective' and live.attempt is None
assert status.disk_stem_set_identity == disk_v3.stem_set_identity
assert status.control_stem_set_identity == disk_v3.stem_set_identity
assert status.effective_stem_set_identity == disk_v3.stem_set_identity
assert project.stem_cache[0].pair == disk_v3
assert session.pad_stem_enabled_mask[0] == 0b1010
assert not session.stem_generation_errors, session.stem_generation_errors
owner_v3.release()
"#,
    );
}

fn other_voice(mixer: &RtMixer) -> &crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(215))
        .unwrap()
}

fn delete_during_claimed_handover(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    old_owner: std::sync::Weak<[u8; 32]>,
    new_owner: std::sync::Weak<[u8; 32]>,
    paused: bool,
) {
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    assert!(transition.is_active());
    if paused {
        for id in [0, 215] {
            playback
                .callback
                .mixer
                .pause_sample_at_output_frame(id, playback.frame);
        }
        playback
            .reference
            .pause_sample_at_output_frame(0, playback.frame);
        playback
            .full_mix
            .pause_sample_at_output_frame(0, playback.frame);
    }
    controller_python(
        locals,
        r#"
assert candidate.claimed() and candidate.effective_mode_status() == 'transitioning'
assert controller.delete_stems(0)
assert project.stem_cache[0] is None and project.pad_stem_mix_mode[0] == 'full_mix'
assert not controller._committed_residency.contains(0)
assert controller.stem_residency_status(0) is None
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert_eq!(
        playback.callback.drain(consumer),
        1,
        "real ordered Python delete"
    );
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::AllStems
    );
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    let after = voice(&playback.callback.mixer);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.generation, generation);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    playback.callback.retirement.slots = 0;
    let held_before = playback.callback.retirement.retired.len();
    if paused {
        // These callbacks consume no source frames. Keep the independent source-feed
        // clock fixed, then resume the same voice and its unmodified Native/FIFOs.
        for frames in [1, 127, 384] {
            let mut output = vec![0.0; frames * 2];
            playback.callback.mixer.render_rt(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                &mut playback.callback.retirement,
            );
            assert!(output.iter().all(|sample| *sample == 0.0));
            let current = voice(&playback.callback.mixer);
            assert!(current.source_playback.matches_exact(&clock));
            assert_eq!(current.generation, generation);
            assert_eq!(current.stretch.native_state_address(), native);
            assert_eq!(current.stretch.pending_fifo_frames(), fifo);
            assert!(
                playback
                    .callback
                    .mixer
                    .stem_transition_for_measurement(0)
                    .matches_exact(transition)
            );
            assert_eq!(
                playback.callback.mixer.stem_demand_for_measurement(0).0,
                StemMixMode::AllStems
            );
            assert!(old_owner.upgrade().is_some() && new_owner.upgrade().is_some());
        }
        for id in [0, 215] {
            playback
                .callback
                .mixer
                .resume_sample_at_output_frame(id, playback.frame);
        }
        playback
            .reference
            .resume_sample_at_output_frame(0, playback.frame);
        playback
            .full_mix
            .resume_sample_at_output_frame(0, playback.frame);
    }
    let ramp_frames = (128.0 / playback.ratio).ceil() as usize;
    let mut elapsed = 0;
    for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
        let count = frames.min(ramp_frames - elapsed);
        playback.render(count, false);
        elapsed += count;
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        if elapsed == ramp_frames {
            break;
        }
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(0).0,
            StemMixMode::AllStems
        );
    }
    // Finishing V1 -> V2 arms the retained delete intent's own V2 -> FullMix
    // ramp. Neither ownership pressure nor Python cancellation may erase it.
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert_eq!(playback.callback.retirement.retired.len(), held_before);
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::FullMix, 0)
    );
    playback.select(None);
    let mut elapsed = 0;
    for frames in [7_usize, 1, 43, 17].into_iter().cycle() {
        let count = frames.min(ramp_frames - elapsed);
        playback.render(count, false);
        elapsed += count;
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        if elapsed == ramp_frames {
            break;
        }
        assert!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
    }
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_some());
    assert_eq!(playback.callback.retirement.retired.len(), held_before);
    assert!(old_owner.upgrade().is_some() && new_owner.upgrade().is_some());
    playback.callback.retirement.slots = 2;
    playback.render(1, false);
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(playback.callback.retirement.retired.len(), held_before + 2);
    for owner in [&old_owner, &new_owner] {
        assert!(
            playback.callback.retirement.retired[held_before..]
                .iter()
                .any(|retired| {
                    matches!(retired, RetiredAudioBuffer::PreparedStems(stems)
                if Arc::ptr_eq(&stems.complete_set_identity, &owner.upgrade().unwrap()))
                })
        );
    }
    playback.callback.retirement.slots = usize::MAX;
    playback.render(13, true);
    let (actual, _) = playback.adopt_native(false);
    assert!(actual.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    controller_python(
        locals,
        "controller.on_frame_render()\nassert project.stem_cache[0] is None\nassert controller.stem_residency_status(0) is None\nassert not session.stem_generation_errors, session.stem_generation_errors",
    );
    // Transfer both exact component owners through the existing production
    // retirement worker. Its join drains the queue outside rendering.
    let held_counts = (old_owner.strong_count(), new_owner.strong_count());
    let (mut off_rt, worker) =
        crate::audio_engine::buffer_retirement::create_audio_buffer_retirement();
    for retired in std::mem::take(&mut playback.callback.retirement.retired) {
        match retired {
            RetiredAudioBuffer::PreparedStems(stems) => off_rt.retire_prepared_stems(stems),
            other => playback.callback.retirement.retired.push(other),
        }
    }
    drop(off_rt);
    drop(worker);
    assert!(old_owner.strong_count() < held_counts.0);
    assert!(new_owner.strong_count() < held_counts.1);
}

fn stopped_adopt(
    engine: &AudioEngine,
    locals: &Py<PyDict>,
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    name: &str,
) {
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
    assert_eq!(callback.drain(consumer), 1, "stopped own Capture");
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, name, "residency_status") == "rejected"
    });
    assert_eq!(callback.drain(consumer), 1, "stopped own Adopt");
    callback.mixer.finish_committed_stem_mode(0);
    reconcile(engine).unwrap();
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "accepted"
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "waiting"
    );
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
}

fn stopped_candidate_play(
    locals: &Py<PyDict>,
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    before_poll: bool,
) {
    let source = callback.mixer.bank_for_measurement()[0].clone().unwrap();
    let set = callback.mixer.stems_for_measurement()[0].clone().unwrap();
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("before_poll", before_poll)
            .unwrap()
    });
    controller_python(
        locals,
        r#"
assert stopped.residency_status() == 'accepted' and stopped.is_current()
assert stopped.effective_mode_status() == 'waiting' and stopped.claimed()
assert stopped.effective_failure_reason() is None and stopped.error() is None
if not before_poll:
    controller.on_frame_render()
    assert live.control is live.disk.selection and live.attempt is None
else:
    assert live.control is previous_control and live.attempt is live.disk.selection
from flitzis_looper.controller.transport import TransportController
class ReadyTransportAudio:
    def __getattr__(self, name):
        return getattr(playback_audio, name)
    def play_sample(self, *args, **kwargs):
        raise AssertionError('ordinary Play must use its own resident window ACK')
    play_sample_exclusive = play_sample
project.pad_loop_auto[0] = False
project.pad_loop_start_s[0] = 17 / fixture['rate']
project.pad_loop_end_s[0] = 1517 / fixture['rate']
project.pad_key_lock[0] = True
transport = TransportController(project, session, ReadyTransportAudio())
transport.playback.trigger_pad_keep_others(0)
ready_start = transport.residency._pending[0].ticket
assert ready_start is not None and ready_start.publication_status() == 'pending'
assert not stopped.is_current() and stopped.effective_mode_status() == 'rejected'
controller.on_frame_render()
assert live.control is live.disk.selection and live.attempt is None
assert not live.control.candidate and live.control.ticket.is_current()
assert live.control.receipt is None and live.control.receipt_intent is None
assert live.control.desired is None and live.desired is None
status = controller.stem_residency_status(0)
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective is None and status.effective_stem_set_identity is None
assert project.pad_stem_mix_mode[0] == 'all_stems'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::RelocateResident(_))
    ));
    assert_eq!(callback.drain(consumer), 1, "ordinary Play window ACK");
    assert!(Arc::ptr_eq(
        &source.samples,
        &callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    let actual = callback.mixer.stems_for_measurement()[0].as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &actual.complete_set_identity
    ));
    for (before, after) in set.stems.iter().zip(&actual.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    Python::attach(|py| {
        let ready = locals.bind(py).get_item("ready_start").unwrap().unwrap();
        let ready = ready
            .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
            .unwrap();
        assert!(
            ready.read_observation_for_test().is_none(),
            "Play reused exact ready window"
        );
    });
    controller_python(
        locals,
        "transport.on_frame_render()\nassert transport.residency.status(0)[1] is None",
    );
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::TriggerInputPad {
            resident_control: Some(_),
            ..
        })
    ));
    assert_eq!(callback.drain(consumer), 1, "ordinary Play launch");
    controller_python(
        locals,
        r#"
session.active_sample_ids.add(0)
for _ in range(16):
    controller.on_frame_render()
assert live.control.receipt is None
assert controller.stem_residency_status(0).effective is None
assert not session.stem_generation_errors, session.stem_generation_errors
assert audio.worker_call_count() == 1
"#,
    );
    assert!(
        consumer.is_empty(),
        "poll cannot implicitly retry obsolete ALL"
    );
    assert!(
        callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    let actual = callback.mixer.stems_for_measurement()[0].as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &actual.complete_set_identity
    ));
    for (before, after) in set.stems.iter().zip(&actual.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
}

fn round_trip(
    selected_v1: bool,
    delete_pause: Option<bool>,
    key_lock: bool,
    stopped_before_poll: Option<bool>,
    paused_first: bool,
    trace: CandidateTrace,
) {
    let repeated_sets = matches!(trace, CandidateTrace::RepeatedSets);
    let withdraw_preclaim = matches!(trace, CandidateTrace::WithdrawPreclaim);
    let admission_pressure = matches!(trace, CandidateTrace::AdmissionPressure);
    assert!(key_lock || delete_pause.is_none());
    assert!(
        !(repeated_sets || withdraw_preclaim || admission_pressure)
            || (selected_v1
                && delete_pause.is_none()
                && stopped_before_poll.is_none()
                && !paused_first)
    );
    let previous_cwd = std::env::current_dir().unwrap();
    let mut h = long_harness();
    // Only the exact child owns this cwd and its project config/artifact paths.
    std::env::set_current_dir(h.root.parent().unwrap()).unwrap();
    let source = h.ticket(0);
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
    let saved_selection = selection(&oracle_pair);
    let _saved = h.save(&oracle_pair);
    let full = h.material.sample.clone();
    let full_stems = PreparedStemSet {
        complete_set_identity: Arc::new([0x74; 32]),
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
    assert!(full_stems.stems.iter().all(|component| {
        component.samples[LOOP_START * 2..LOOP_END * 2]
            .iter()
            .any(|value| value.abs() > 0.04)
    }));
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
    assert_eq!(window.publication_status(), "accepted");
    reconcile(&h.engine).unwrap();
    let finite = h.callback.mixer.bank_for_measurement()[0].clone().unwrap();
    let other_source = h.callback.mixer.bank_for_measurement()[215]
        .clone()
        .unwrap();
    let metadata = h.material.metadata();
    let fixture = json!({
        "original_reference": metadata["new_reference"], "material_id": metadata["material_id"],
        "source_version": h.version, "pair": saved_selection, "frames": full.frame_count(), "rate": RATE,
    });
    let engine = Arc::new(h.engine);
    let locals = Python::attach(|py| {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        py.import("sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1(
                "insert",
                (0, repository.join("src").to_string_lossy().as_ref()),
            )
            .unwrap();
        let locals = PyDict::new(py);
        locals
            .set_item("input_json", serde_json::to_string(&fixture).unwrap())
            .unwrap();
        locals.set_item("selected_v1", selected_v1).unwrap();
        locals.set_item("repeated_sets", repeated_sets).unwrap();
        locals
            .set_item("admission_pressure", admission_pressure)
            .unwrap();
        locals
            .set_item("withdraw_preclaim", withdraw_preclaim)
            .unwrap();
        locals
            .set_item(
                "playback_audio",
                Py::new(
                    py,
                    ReadyPlaybackAudio {
                        engine: engine.clone(),
                        producer: h.producer.clone(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        locals
            .set_item(
                "audio",
                Py::new(
                    py,
                    PairWorkerAudio {
                        engine: engine.clone(),
                        root: h.root.clone(),
                        producer: h.producer.clone(),
                        control_thread: std::thread::current().id(),
                        worker_calls: AtomicUsize::new(0),
                        only_pool_threads: AtomicBool::new(true),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        worker_python(
            py,
            &locals,
            r#"
import json
from pathlib import Path
from flitzis_looper.models import ProjectState, SessionState, StemCacheEntry, PadContentIdentity
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.stem_workers import StemWorkerPool
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.stem_pair_selection import StemPairSelection
fixture = json.loads(input_json)
project = ProjectState()
project.sample_paths[0] = fixture['original_reference']
project.sample_durations[0] = fixture['frames'] / fixture['rate']
project.pad_content[0] = PadContentIdentity(instance_id='8' * 32, material_id=fixture['material_id'])
project.pad_timing_intent[0] = 'legacy'
project.pad_stem_mix_mode[0] = 'full_mix'
disk_v1 = StemPairSelection.model_validate(fixture['pair'])
project.stem_cache[0] = StemCacheEntry(source_version=fixture['source_version'],
    cache_dir=disk_v1.wav_generation, stems=expected_stem_files(disk_v1.wav_generation), pair=disk_v1, available=True)
session = SessionState()
session.pad_stem_enabled_mask[0] = 0b1010
persistence = ProjectPersistence(project)
controller = StemController(project, session, audio, persistence.mark_dirty)
assert isinstance(controller._stem_worker_pool, StemWorkerPool)
assert controller._stem_worker_pool._executor._max_workers == 2
assert controller._committed_residency.pending_supported
controller.restore_stem_cache_from_project_state()
assert controller.publish_restored_stem_cache_if_available(0)
"#,
        );
        locals.unbind()
    });
    wait_until(|| {
        Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
            );
            locals
                .bind(py)
                .get_item("controller")
                .unwrap()
                .unwrap()
                .call_method1("stem_residency_status", (0,))
                .unwrap()
                .getattr("registration")
                .map(|status| status.extract::<String>().unwrap() == "pending")
                .unwrap_or(false)
        })
    });
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    controller_python(
        &locals,
        r#"
controller.on_frame_render()
previous_control = controller._committed_residency._selections[0]
assert previous_control.ticket.publication_status() == 'accepted'
assert previous_control.ticket.is_current()
assert controller.stem_residency_status(0).effective is None
assert audio.worker_call_count() == 1 and audio.prepared_only_on_pool_threads()
persistence.mark_dirty()
persistence.flush()
"#,
    );
    let ratio = if key_lock { 0.75 } else { 1.25 };
    h.callback.mixer.set_speed(ratio);
    assert_eq!(h.callback.mixer.key_lock_for_measurement(0), key_lock);
    h.callback.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    if let Some(before_poll) = stopped_before_poll {
        assert!(!paused_first);
        assert!(key_lock && !selected_v1 && delete_pause.is_none());
        controller_python(
            &locals,
            "assert controller.set_stem_mix_mode(0, 'all_stems')\nv1_stopped = previous_control.receipt",
        );
        stopped_adopt(
            &engine,
            &locals,
            &mut h.callback,
            &mut h.consumer,
            "v1_stopped",
        );
        controller_python(
            &locals,
            "controller.on_frame_render()\nassert controller.stem_residency_status(0).effective_stem_set_identity is None",
        );
        let v1 = h.callback.mixer.stems_for_measurement()[0].clone().unwrap();
        commit_candidate_v2(
            &engine,
            &h.root,
            &h.version,
            &h.wav_reference,
            full.frame_count(),
            &locals,
        );
        controller_python(
            &locals,
            "assert controller.set_stem_mix_mode(0, 'all_stems')\nstopped = live.attempt.receipt\nassert stopped is not v1_stopped",
        );
        stopped_adopt(
            &engine,
            &locals,
            &mut h.callback,
            &mut h.consumer,
            "stopped",
        );
        let v2 = h.callback.mixer.stems_for_measurement()[0].clone().unwrap();
        assert!(!Arc::ptr_eq(
            &v1.complete_set_identity,
            &v2.complete_set_identity
        ));
        for (before, after) in v1.stems.iter().zip(&v2.stems) {
            assert!(!Arc::ptr_eq(&before.samples, &after.samples));
        }
        stopped_candidate_play(&locals, &mut h.callback, &mut h.consumer, before_poll);
        let mut reference = complete_reference(&full, &full_stems, ratio, true);
        // Stopped V2 is already selected before this new ordinary voice starts.
        // Physical V2 mask1010 equals complete independent V1 mask0011.
        reference.stop_sample(0);
        assert!(reference.set_stem_enabled_mask(0, 0b0011, full_stems.source_version_hash));
        assert!(reference.set_stem_mix_mode(
            0,
            StemMixMode::AllStems,
            full_stems.source_version_hash
        ));
        assert!(reference.play_sample_at_output_frame(0, 1.0, 0));
        let mut playback = ControllerPlayback {
            callback: h.callback,
            reference,
            full_mix: complete_reference(&full, &full_stems, ratio, true),
            frame: 0,
            source_frame: 0,
            resumed_at: None,
            source: full.clone(),
            components: full_stems.stems.clone(),
            ratio,
            selection: Some(0b0011),
            previous: Some(0b0011),
            ramp_start: 0,
            raw: Some(ControllerRawNative::new(ratio, false)),
            candidate: None,
            filter: controller_filter(),
        };
        playback.render(13, true);
        let (actual, counterfactual) = playback.adopt_native(true);
        assert!(actual.iter().any(|sample| sample.abs() > 0.001));
        assert!(
            actual
                .iter()
                .zip(counterfactual)
                .any(|(actual, full)| (actual - full).abs() > 0.005)
        );
        controller_python(
            &locals,
            r#"
controller.on_frame_render()
assert controller.stem_residency_status(0).effective is None
assert controller.set_stem_mix_mode(0, 'all_stems')
fresh = live.control.receipt
assert fresh is not stopped and fresh.is_current()
assert fresh.residency_status() in {'captured', 'pending'}
assert fresh.effective_mode_status() == 'waiting'
"#,
        );
        own_adoption(&mut playback, &mut h.consumer, &locals, "fresh", true);
        let after = playback.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(
            &v2.complete_set_identity,
            &after.complete_set_identity
        ));
        for (before, after) in v2.stems.iter().zip(&after.stems) {
            assert!(Arc::ptr_eq(&before.samples, &after.samples));
        }
        let mut output = Vec::new();
        for frames in [1, 31, 137, 257, 777] {
            output.extend(playback.render(frames, true).0);
        }
        assert!(output.iter().any(|sample| sample.abs() > 0.001));
        controller_python(
            &locals,
            r#"
controller.on_frame_render()
assert fresh.is_current() and fresh.effective_mode_status() == 'effective'
assert fresh.effective_failure_reason() is None and fresh.error() is None
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == disk_v2.stem_set_identity
assert status.effective == 'effective' and not stopped.is_current()
assert not session.stem_generation_errors, session.stem_generation_errors
assert audio.worker_call_count() == 1
controller.shut_down()
controller._assets.release_saved_assignments()
owner_v1.release()
owner_v2.release()
"#,
        );
        std::env::set_current_dir(previous_cwd).unwrap();
        return;
    }
    assert!(h.callback.mixer.play_sample_at_output_frame(0, 1.0, 0));
    // A genuinely active other pad contributes zero gain, so the independent pad-0
    // raw oracle can still assert every output sample without hiding other-pad state.
    h.callback
        .mixer
        .set_loop_region_frames(215, LOOP_START, Some(LOOP_END));
    assert!(h.callback.mixer.play_sample_at_output_frame(215, 0.0, 0));
    let other_generation = other_voice(&h.callback.mixer).generation;
    let other_native = other_voice(&h.callback.mixer)
        .stretch
        .native_state_address();
    let other_fifo = other_voice(&h.callback.mixer).stretch.pending_fifo_frames();
    let other_demand = h.callback.mixer.stem_demand_for_measurement(215);
    let mut playback = ControllerPlayback {
        callback: h.callback,
        reference: complete_reference(&full, &full_stems, ratio, key_lock),
        full_mix: complete_reference(&full, &full_stems, ratio, key_lock),
        frame: 0,
        source_frame: 0,
        resumed_at: None,
        source: full.clone(),
        components: full_stems.stems.clone(),
        ratio,
        selection: None,
        previous: None,
        ramp_start: 0,
        raw: key_lock.then(|| ControllerRawNative::new(ratio, false)),
        candidate: None,
        filter: controller_filter(),
    };
    if key_lock {
        playback.render(13, true);
        let (actual, _) = playback.adopt_native(true);
        if paused_first {
            assert!(actual.iter().any(|sample| sample.abs() > 0.001));
        }
    } else {
        render_varispeed(&mut playback, 13);
    }
    controller_python(&locals, "session.active_sample_ids.update({0, 215})");
    if paused_first {
        assert!(key_lock && !selected_v1 && delete_pause.is_none());
        assert!(voice(&playback.callback.mixer).stretch.wet_render_ready());
        assert!(
            voice(&playback.callback.mixer)
                .stretch
                .wet_current_source_output_ready()
        );
        controller_python(
            &locals,
            r#"
from flitzis_looper.controller.transport import TransportController
transport = TransportController(project, session, playback_audio)
transport.playback.pause_pad(0)
transport.playback.pause_pad(215)
assert session.paused_sample_ids == {0, 215}
"#,
        );
        assert_eq!(
            playback.drain(&mut h.consumer),
            2,
            "actual public Pause commands"
        );
        playback
            .reference
            .pause_sample_at_output_frame(0, playback.frame);
        playback
            .full_mix
            .pause_sample_at_output_frame(0, playback.frame);
        playback.render_paused(513);
    }
    if selected_v1 {
        controller_python(
            &locals,
            "assert controller.set_stem_mix_mode(0, 'all_stems')\nfirst = controller._committed_residency._selections[0].receipt",
        );
        own_adoption(&mut playback, &mut h.consumer, &locals, "first", key_lock);
        assert!(playback.reference.set_stem_enabled_mask(
            0,
            0b1010,
            full_stems.source_version_hash
        ));
        assert!(playback.reference.set_stem_mix_mode(
            0,
            StemMixMode::AllStems,
            full_stems.source_version_hash
        ));
        playback.select(Some(0b1010));
        if key_lock {
            settle_wet(&mut playback, &locals, "first");
        } else {
            settle_varispeed(&mut playback, &locals, "first");
        }
    }
    let before = voice(&playback.callback.mixer);
    let initial_clock = before.source_playback;
    let initial_generation = before.generation;
    let initial_native = before.stretch.native_state_address();
    let initial_fifo = before.stretch.pending_fifo_frames();
    let initial_history = before.stretch.productive_history();
    assert_eq!(initial_history.is_some(), key_lock);
    let predecessor = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .map(|stems| stems.complete_set_identity.clone());
    let predecessor_set = repeated_sets.then(|| {
        playback.callback.mixer.stems_for_measurement()[0]
            .clone()
            .unwrap()
    });
    let (v2, source_ticket) = v2_files(
        &engine,
        &h.root,
        &h.version,
        &h.wav_reference,
        full.frame_count(),
    );
    let candidate_json = v2.selection_json().to_owned();
    assert_ne!(
        serde_json::from_str::<Value>(&candidate_json).unwrap(),
        fixture["pair"]
    );
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("candidate_json", candidate_json)
            .unwrap();
        locals
            .bind(py)
            .set_item("prepared_v2", Py::new(py, v2).unwrap())
            .unwrap();
        locals
            .bind(py)
            .set_item("source_v2", Py::new(py, source_ticket).unwrap())
            .unwrap();
    });
    controller_python(
        &locals,
        r#"
disk_v2 = StemPairSelection.model_validate_json(candidate_json)
entry_v1 = project.stem_cache[0]
entry_v2 = StemCacheEntry(source_version=fixture['source_version'], cache_dir=disk_v2.wav_generation,
    stems=expected_stem_files(disk_v2.wav_generation), pair=disk_v2, available=True)
# Both real complete assignment owners survive the actual fenced config replacement.
owner_v1 = audio.acquire_project_asset_lease(str(Path(entry_v1.cache_dir).absolute()))
owner_v2 = audio.acquire_project_asset_lease(str(Path(entry_v2.cache_dir).absolute()))
persistence.commit_stem_selection(0, entry_v1, entry_v2,
    lambda: project.stem_cache[0] is entry_v1 and source_v2.same_source_request(
        audio.capture_prepared_source(0, fixture['source_version'])))
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None
assert reopened.project.stem_cache[0].pair == disk_v2
project.stem_cache[0] = entry_v2
prepared_v2.select()
controller._register_committed_pair(0, prepared_v2, source_v2)
live = controller._committed_residency._live_selections[0]
assert live.control is previous_control and live.attempt is None
assert live.disk.selection.ticket.publication_status() == 'accepted'
assert previous_control.ticket.is_current()
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v1.stem_set_identity
assert status.effective_stem_set_identity == (disk_v1.stem_set_identity if selected_v1 else None)
"#,
    );
    if admission_pressure {
        full_queue_then_explicit_retry(
            &engine,
            &h.producer,
            &mut playback,
            &mut h.consumer,
            &locals,
            key_lock,
        );
    }
    controller_python(
        &locals,
        r#"
if repeated_sets:
    assert controller.set_stem_enabled_mask(0, 0)
if not admission_pressure:
    assert controller.set_stem_mix_mode(0, 'all_stems')
    assert controller._publish_all_stems_mode_if_preferred(0, fixture['source_version'])
candidate = live.attempt.receipt
assert candidate is not None and candidate.effective_mode_status() == 'waiting'
for _ in range(16):
    controller.on_frame_render()
assert candidate.effective_mode_status() == 'waiting'
assert previous_control.ticket.is_current()
"#,
    );
    if !admission_pressure {
        let current = voice(&playback.callback.mixer);
        assert!(current.source_playback.matches_exact(&initial_clock));
        assert_eq!(current.generation, initial_generation);
        assert_eq!(current.stretch.native_state_address(), initial_native);
        assert_eq!(current.stretch.pending_fifo_frames(), initial_fifo);
        if let Some(initial_history) = initial_history {
            let history = current.stretch.productive_history().unwrap();
            assert_eq!(history.binding, initial_history.binding);
            assert_eq!(history.next_position, initial_history.next_position);
            assert_eq!(history.fed_output_frames, initial_history.fed_output_frames);
        } else {
            assert!(current.stretch.productive_history().is_none());
        }
    }
    if withdraw_preclaim {
        withdraw_queued_candidate_for_full_mix(&mut playback, &mut h.consumer, &locals, key_lock);
    }
    own_adoption(
        &mut playback,
        &mut h.consumer,
        &locals,
        "candidate",
        key_lock,
    );
    assert_eq!(
        playback.callback.mixer.key_lock_for_measurement(0),
        key_lock
    );
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).1,
        if repeated_sets { 0 } else { 0b1010 }
    );
    let current_source = playback.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&current_source.samples, &finite.samples));
    assert_eq!(current_source.resident_binding(), finite.resident_binding());
    let old_owner = predecessor.as_ref().map(Arc::downgrade);
    let adopted_owner = &playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .complete_set_identity;
    let new_owner = Arc::downgrade(adopted_owner);
    if let Some(old) = predecessor {
        assert!(!Arc::ptr_eq(&old, adopted_owner));
    }
    assert_eq!(
        voice(&playback.callback.mixer).generation,
        initial_generation
    );
    // Actual V2 mask1010 maps to V1 vocals+melody; mask0 stays Some(0), not FullMix.
    let oracle_mask = if repeated_sets { 0 } else { 0b0011 };
    assert!(playback.reference.set_stem_enabled_mask(
        0,
        oracle_mask,
        full_stems.source_version_hash
    ));
    assert!(playback.reference.set_stem_mix_mode(
        0,
        StemMixMode::AllStems,
        full_stems.source_version_hash
    ));
    playback.select(Some(oracle_mask));
    controller_python(
        &locals,
        r#"
controller.on_frame_render()
status = controller.stem_residency_status(0)
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == (disk_v1.stem_set_identity if selected_v1 and not withdraw_preclaim else None)
assert status.effective == 'transitioning'
assert candidate.claimed() and not candidate.withdraw()
assert not previous_control.ticket.is_current()
if admission_pressure:
    assert session.stem_generation_errors == {0: admission_error}
"#,
    );
    if repeated_sets {
        let (v3, source_ticket) = version_files(
            &engine,
            &h.root,
            &h.version,
            &h.wav_reference,
            full.frame_count(),
            FixtureSet::Third,
        );
        Python::attach(|py| {
            locals
                .bind(py)
                .set_item("third_json", v3.selection_json().to_owned())
                .unwrap();
            locals
                .bind(py)
                .set_item("prepared_v3", Py::new(py, v3).unwrap())
                .unwrap();
            locals
                .bind(py)
                .set_item("source_v3", Py::new(py, source_ticket).unwrap())
                .unwrap();
        });
        repeated_sets_after_empty_claim(
            &mut playback,
            &mut h.consumer,
            &locals,
            &full_stems,
            &predecessor_set.unwrap(),
            key_lock,
        );
    } else if let Some(paused) = delete_pause {
        delete_during_claimed_handover(
            &mut playback,
            &mut h.consumer,
            &locals,
            old_owner.expect("delete case starts with selected V1"),
            new_owner,
            paused,
        );
    } else {
        if paused_first {
            let source_frame = playback.source_frame;
            let fifo = voice(&playback.callback.mixer)
                .stretch
                .pending_fifo_frames();
            for frames in [1, 127, 384, 512] {
                playback.render_paused(frames);
                assert_eq!(playback.source_frame, source_frame);
                assert_eq!(
                    controller_ticket_status(&locals, "candidate", "effective_mode_status"),
                    "transitioning"
                );
                controller_python(
                    &locals,
                    "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
                );
            }
            assert!(h.consumer.is_empty());
            controller_python(
                &locals,
                r#"
assert candidate.claimed() and candidate.is_current()
assert controller.stem_residency_status(0).effective == 'transitioning'
transport.playback.resume_pad(0)
transport.playback.resume_pad(215)
assert not session.paused_sample_ids
"#,
            );
            assert_eq!(
                playback.drain(&mut h.consumer),
                2,
                "actual public Resume commands"
            );
            playback
                .reference
                .resume_sample_at_output_frame(0, playback.frame);
            playback
                .full_mix
                .resume_sample_at_output_frame(0, playback.frame);
            playback.resumed_at = Some(playback.frame);
            let resumed = voice(&playback.callback.mixer);
            assert!(!resumed.paused);
            assert!(resumed.source_playback.matches_exact(&initial_clock));
            assert_eq!(resumed.generation, initial_generation);
            assert_eq!(resumed.stretch.native_state_address(), initial_native);
            assert_eq!(resumed.stretch.pending_fifo_frames(), fifo);
        }
        if key_lock {
            settle_wet(&mut playback, &locals, "candidate");
        } else {
            settle_varispeed(&mut playback, &locals, "candidate");
        }
        controller_python(
            &locals,
            r#"
status = controller.stem_residency_status(0)
assert status.registration == 'accepted' and status.residency == 'accepted'
assert status.effective == 'effective'
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == disk_v2.stem_set_identity
assert project.stem_cache[0].pair == disk_v2
assert controller._stem_worker_pool._executor._max_workers == 2
if admission_pressure:
    assert live.control is pressure_selection and live.disk.selection is pressure_selection
    assert pressure_selection.ticket is pressure_registration and pressure_selection.receipt is candidate
    assert not session.stem_generation_errors, session.stem_generation_errors
if withdraw_preclaim:
    assert not withdrawn.claimed() and not withdrawn.is_current()
    assert withdrawn.residency_status() == 'rejected' and withdrawn.effective_mode_status() == 'rejected'
    assert withdrawn.effective_failure_reason() is None
"#,
        );
    }
    let other = other_voice(&playback.callback.mixer);
    assert_eq!(other.generation, other_generation);
    assert_eq!(other.stretch.native_state_address(), other_native);
    assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
    assert!(
        other
            .source_playback
            .matches_exact(&voice(&playback.callback.mixer).source_playback)
    );
    assert!(Arc::ptr_eq(
        &other.sample.as_ref().unwrap().samples,
        &other_source.samples
    ));
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(215),
        other_demand
    );
    assert!(playback.callback.mixer.stems_for_measurement()[215].is_none());
    controller_python(
        &locals,
        "controller.shut_down()\ncontroller._assets.release_saved_assignments()\nowner_v1.release()\nowner_v2.release()",
    );
    std::env::set_current_dir(previous_cwd).unwrap();
}

#[test]
fn actual_python_controller_first_and_replacement_candidates_keep_own_wet_chronology() {
    if std::env::var_os(CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                true,
                None,
                false,
                CandidateTrace::Ordinary,
            );
        }
        for paused in [false, true] {
            round_trip(
                true,
                Some(paused),
                true,
                None,
                false,
                CandidateTrace::Ordinary,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 productive controller child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_keylock_off_first_and_replacement_keep_varispeed_feed_and_eq() {
    if std::env::var_os(VARISPEED_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::Ordinary,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", VARISPEED_TEST, "--nocapture", "--test-threads=1"])
        .env(VARISPEED_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 Varispeed controller child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_stopped_replacement_play_then_fresh_all_uses_own_wet_receipt() {
    if std::env::var_os(STOPPED_NEW_SET_CHILD).is_some() {
        for before_poll in [false, true] {
            round_trip(
                false,
                None,
                true,
                Some(before_poll),
                false,
                CandidateTrace::Ordinary,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            STOPPED_NEW_SET_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(STOPPED_NEW_SET_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 stopped replacement child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_wet_full_mix_paused_before_first_candidate_keeps_consumed_clock() {
    if std::env::var_os(PAUSED_FIRST_CHILD).is_some() {
        round_trip(false, None, true, None, true, CandidateTrace::Ordinary);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            PAUSED_FIRST_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PAUSED_FIRST_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 paused-first child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_empty_handover_defers_v3_and_uses_latest_mask_with_own_ack() {
    if std::env::var_os(REPEATED_SETS_CHILD).is_some() {
        for key_lock in [true, false] {
            round_trip(
                true,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::RepeatedSets,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            REPEATED_SETS_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(REPEATED_SETS_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 repeated sets child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_full_mix_withdraws_queued_v2_adopt_then_all_uses_fresh_authority() {
    if std::env::var_os(WITHDRAW_PRECLAIM_CHILD).is_some() {
        for key_lock in [true, false] {
            round_trip(
                true,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::WithdrawPreclaim,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            WITHDRAW_PRECLAIM_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(WITHDRAW_PRECLAIM_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 preclaim withdrawal child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_queue_full_keeps_v1_then_explicit_all_admits_own_v2() {
    if std::env::var_os(ADMISSION_PRESSURE_CHILD).is_some() {
        for key_lock in [true, false] {
            round_trip(
                true,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::AdmissionPressure,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            ADMISSION_PRESSURE_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ADMISSION_PRESSURE_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 admission pressure child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}
