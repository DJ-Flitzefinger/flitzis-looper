//! Genuine StemController -> Native candidate Capture/Adopt -> continuous output.
//! V2 swaps real vocals/drums files; independent complete V1 PCM supplies the
//! algebraic feed, raw Native/FIFO or dry Varispeed, and uninterrupted EQ oracle.

use super::*;

const CHILD: &str = "FLITZI_L3_CONTROLLER_CANDIDATE_CHILD";
const TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_first_and_replacement_candidates_keep_own_wet_chronology";
const VARISPEED_CHILD: &str = "FLITZI_L3_CONTROLLER_VARISPEED_CHILD";
const VARISPEED_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_keylock_off_first_and_replacement_keep_varispeed_feed_and_eq";

fn v2_files(
    engine: &AudioEngine,
    root: &Path,
    version: &str,
    original_reference: &str,
    frames: usize,
) -> (PreparedStemPair, PreparedSourceTicket) {
    let generation = format!("{:032x}", 0x301_u32);
    let reference = original_reference.replace(GENERATION, &generation);
    let path = root.parent().unwrap().join(&reference);
    write_signal_set(&path, frames, version, 0);
    let vocals = fs::read(path.join("vocals.wav")).unwrap();
    let drums = fs::read(path.join("drums.wav")).unwrap();
    fs::write(path.join("vocals.wav"), drums).unwrap();
    fs::write(path.join("drums.wav"), vocals).unwrap();
    fs::write(
        path.join("instrumental.wav"),
        signal_sum_wav(frames, &[0, 1, 2]),
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
    assert_eq!(playback.callback.drain(consumer), 1, "own callback Capture");
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, name, "residency_status") == "rejected"
    });
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "pending"
    );
    if key_lock {
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
    assert_eq!(playback.callback.drain(consumer), 1, "own callback Adopt");
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
    let feed = playback.feed(playback.frame, frames);
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
    let phase = ((playback.frame + frames as u64) as f64 * playback.ratio)
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
    (output, full_mix)
}

fn settle_varispeed(playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str) {
    assert_eq!(playback.ratio, 1.25);
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let fifo = voice(&playback.callback.mixer)
        .stretch
        .pending_fifo_frames();
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

fn round_trip(selected_v1: bool, delete_pause: Option<bool>, key_lock: bool) {
    assert!(key_lock || delete_pause.is_none());
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
        playback.adopt_native(true);
    } else {
        render_varispeed(&mut playback, 13);
    }
    controller_python(&locals, "session.active_sample_ids.update({0, 215})");
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
        0b1010
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
    // Actual V2 mask 1010 means V1 melody+vocals after the physical file permutation.
    assert!(
        playback
            .reference
            .set_stem_enabled_mask(0, 0b0011, full_stems.source_version_hash)
    );
    assert!(playback.reference.set_stem_mix_mode(
        0,
        StemMixMode::AllStems,
        full_stems.source_version_hash
    ));
    playback.select(Some(0b0011));
    controller_python(
        &locals,
        r#"
controller.on_frame_render()
status = controller.stem_residency_status(0)
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == (disk_v1.stem_set_identity if selected_v1 else None)
assert status.effective == 'transitioning'
assert candidate.claimed() and not candidate.withdraw()
assert not previous_control.ticket.is_current()
"#,
    );
    if let Some(paused) = delete_pause {
        delete_during_claimed_handover(
            &mut playback,
            &mut h.consumer,
            &locals,
            old_owner.expect("delete case starts with selected V1"),
            new_owner,
            paused,
        );
    } else {
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
            round_trip(selected_v1, None, true);
        }
        for paused in [false, true] {
            round_trip(true, Some(paused), true);
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
            round_trip(selected_v1, None, false);
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
