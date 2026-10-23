//! Public OFF of an already audible public-born complete KEY LOCK source.
//! Four bounded representatives share the established controller birth and PCM oracle.

use super::committed_stem_public_complete_tests as complete;
use super::*;

const CHILD: &str = "FLITZI_PUBLIC_KEY_LOCK_OFF_CHILD";
const PREFIX: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::public_key_lock_off_tests::";

#[test]
fn public_complete_full_mix_key_lock_off_running() {
    isolated(
        false,
        false,
        "public_complete_full_mix_key_lock_off_running",
    );
}

#[test]
fn public_complete_full_mix_key_lock_off_paused() {
    isolated(false, true, "public_complete_full_mix_key_lock_off_paused");
}

#[test]
fn public_complete_all_key_lock_off_running() {
    isolated(true, false, "public_complete_all_key_lock_off_running");
}

#[test]
fn public_complete_all_key_lock_off_paused() {
    isolated(true, true, "public_complete_all_key_lock_off_paused");
}

fn isolated(all: bool, paused: bool, name: &str) {
    if std::env::var_os(CHILD).is_some() {
        let followup: complete::PublicCompleteFollowup = if paused {
            paused_followup
        } else {
            running_followup
        };
        complete::history_with_followup(true, Some((all, followup)));
        eprintln!("public KEY LOCK OFF child PID={}", std::process::id());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("{PREFIX}{name}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "public KEY LOCK OFF child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("public KEY LOCK OFF child PID="));
}

fn running_followup(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    reference_owners: &[Arc<InputRuntimeOwnership>; 2],
) {
    public_off(playback, consumer, locals, engine, reference_owners, false);
}

fn paused_followup(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    reference_owners: &[Arc<InputRuntimeOwnership>; 2],
) {
    public_off(playback, consumer, locals, engine, reference_owners, true);
}

struct Peer {
    sample: SampleBuffer,
    generation: u64,
    native: usize,
    fifo: (usize, usize),
    demand: (StemMixMode, u8, u64),
    callback_start: u64,
    consumed_start: u64,
}

impl Peer {
    fn new(playback: &ControllerPlayback, initial_prepare_frames: u64) -> Self {
        let consumed_start = initial_prepare_frames + playback.source_frame;
        assert!(
            playback.frame >= consumed_start,
            "the peer's preparation lead plus emitted frames cannot exceed callback time"
        );
        let peer = complete::other(&playback.callback.mixer);
        Self {
            sample: peer.sample.clone().unwrap(),
            generation: peer.generation,
            native: peer.stretch.native_state_address(),
            fifo: peer.stretch.pending_fifo_frames(),
            demand: playback.callback.mixer.stem_demand_for_measurement(215),
            callback_start: playback.frame,
            // The peer started at T=E=0 and consumed the primary's preparation
            // lead before both voices emitted common active output. Later
            // common pauses consume neither source counter.
            consumed_start,
        }
    }

    fn assert_current(&self, playback: &ControllerPlayback) {
        let peer = complete::other(&playback.callback.mixer);
        assert!(!peer.paused && !playback.callback.mixer.key_lock_for_measurement(215));
        assert_eq!(peer.generation, self.generation);
        assert_eq!(peer.stretch.native_state_address(), self.native);
        assert_eq!(peer.stretch.pending_fifo_frames(), self.fifo);
        assert!(peer.sample.as_ref().unwrap().same_window(&self.sample));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            self.demand
        );
        let consumed = self.consumed_start + playback.frame - self.callback_start;
        let phase = (consumed as f64 * playback.ratio).rem_euclid(playback.loop_period);
        assert_eq!(
            peer.source_playback.position().frame,
            phase.floor() as usize
        );
        assert_eq!(
            peer.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(
            peer.source_playback.tempo_ratio().to_bits(),
            playback.ratio.to_bits()
        );
    }
}

/// Pad 215 continues independently while only the tested pad is paused.
/// Neither the complete PCM feed nor its EQ/Native oracle consumes this gap.
fn paused_render(playback: &mut ControllerPlayback, frames: usize) {
    let current = voice(&playback.callback.mixer);
    assert!(current.paused);
    let clock = current.source_playback;
    let generation = current.generation;
    let native = current.stretch.native_state_address();
    let fifo = current.stretch.pending_fifo_frames();
    let history = current.stretch.productive_history();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let consumed = playback.source_frame;
    let mut actual = vec![0.0; frames * 2];
    playback.callback.mixer.render_rt_at_output_frame(
        &mut actual,
        &mut [0.0; NUM_SAMPLES],
        playback.frame,
        &mut playback.callback.retirement,
    );
    assert!(actual.iter().all(|sample| *sample == 0.0));
    for reference in [&mut playback.reference, &mut playback.full_mix] {
        let mut expected = vec![0.0; frames * 2];
        reference.render_at_output_frame(playback.frame, &mut expected, &mut [0.0; NUM_SAMPLES]);
        assert_eq!(actual, expected);
        assert!(voice(reference).source_playback.matches_exact(&clock));
    }
    let current = voice(&playback.callback.mixer);
    assert!(current.paused && current.source_playback.matches_exact(&clock));
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    match (history, current.stretch.productive_history()) {
        (Some(before), Some(after)) => {
            assert_eq!(after.binding, before.binding);
            assert_eq!(after.next_position, before.next_position);
            assert_eq!(after.fed_output_frames, before.fed_output_frames);
        }
        (None, None) => {}
        _ => panic!("Pause cannot change productive source history"),
    }
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    assert_eq!(playback.source_frame, consumed);
    playback.frame += frames as u64;
}

fn remember_own_pause_admission(locals: &Py<PyDict>, paused: bool) {
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("own_requested_paused", paused)
            .unwrap();
    });
    controller_python(
        locals,
        r#"
assert project.pad_content[0] == off_content
assert session.active_sample_ids == {0, 215}
assert session.paused_sample_ids == (set() if own_requested_paused else {0})
assert not feedback_route._ordinary_observation_fenced(0)
own_pause_owner = transport.playback._pause_intent._owners[off_content.instance_id]
own_pause_admission = own_pause_owner.admission
assert own_pause_owner.pending and own_pause_admission is not None
assert own_pause_admission.requested_paused is own_requested_paused
assert own_pause_admission.current_control_slot() == 0
own_pause_binding = transport._audio.current_input_runtime_pad_binding(0)
assert own_pause_binding is not None and own_pause_binding.is_current()
own_pause_source_keys = (
    'content_lifetime', 'source_generation', 'source_sha256', 'sample_rate_hz',
    'channels', 'frame_count', 'window_revision', 'resident_start_frame',
    'resident_end_frame', 'resident_pcm_identity', 'resident_context')
own_pause_metadata = own_pause_binding.metadata()
own_pause_source = tuple(own_pause_metadata[key] for key in own_pause_source_keys)
assert own_pause_admission.content_lifetime == own_pause_metadata['content_lifetime']
own_pause_before = own_pause_admission.current_observation()
assert own_pause_before is not None and own_pause_before.current_control_slot() == 0
assert own_pause_before.content_lifetime == own_pause_admission.content_lifetime
assert own_pause_before.active and own_pause_before.ordinary_paused is not own_requested_paused
assert not own_pause_before.settles_admission(own_pause_admission)
own_pause_before_sequence = own_pause_before.observation_sequence
assert transport.playback._pause_intent.requested_paused(0) is own_requested_paused
assert not transport.playback._pause_intent.requested_paused(215)
assert own_pause_owner.admission is own_pause_admission and own_pause_owner.pending
assert session.paused_sample_ids == (set() if own_requested_paused else {0})
"#,
    );
}

fn assert_own_pause_effect(locals: &Py<PyDict>) {
    controller_python(
        locals,
        r#"
assert project.pad_content[0] == off_content
assert own_pause_binding.is_current()
own_pause_current_binding = transport._audio.current_input_runtime_pad_binding(0)
assert own_pause_current_binding is not None and own_pause_current_binding.is_current()
own_pause_current_metadata = own_pause_current_binding.metadata()
assert tuple(own_pause_current_metadata[key] for key in own_pause_source_keys) == own_pause_source
assert own_pause_admission.current_control_slot() == 0
own_pause_observation = own_pause_admission.current_observation()
assert own_pause_observation is not None and own_pause_observation.current_control_slot() == 0
assert own_pause_observation.content_lifetime == own_pause_admission.content_lifetime
assert own_pause_observation.content_lifetime == own_pause_current_metadata['content_lifetime']
assert own_pause_observation.observation_sequence > own_pause_before_sequence
assert own_pause_observation.matches_admission(own_pause_admission)
assert own_pause_observation.settles_admission(own_pause_admission)
assert own_pause_observation.outcome == 'applied' and own_pause_observation.active
assert own_pause_observation.ordinary_paused is own_requested_paused
transport.playback.poll_pause_observations()
assert session.active_sample_ids == {0, 215}
assert session.paused_sample_ids == ({0} if own_requested_paused else set())
assert not transport.playback._pause_intent.requested_paused(215)
assert not own_pause_owner.pending and own_pause_owner.admission is None
del own_pause_owner, own_pause_admission, own_pause_binding, own_pause_metadata
del own_pause_source_keys, own_pause_source, own_pause_before, own_pause_before_sequence
del own_pause_current_binding, own_pause_current_metadata, own_pause_observation
"#,
    );
}

fn assert_off_status(locals: &Py<PyDict>) {
    controller_python(
        locals,
        r#"
transport.on_frame_render()
controller.on_frame_render()
assert off_window.publication_status() == 'accepted'
status = transport.residency.key_lock_status(0)
assert not status.requested and status.effective is False and not status.pending
assert not status.unconfirmed and status.error is None
feedback = transport.residency._mode_feedback(0)
assert feedback['source_id'] == off_source['source_id']
assert feedback['source_generation'] == off_source['source_generation']
assert feedback['source_identity'] == off_source['source_identity']
assert feedback['window_revision'] == off_window.window_revision
assert feedback['request_id'] == off_window.key_lock_request_id
assert feedback['effective'] is False and feedback['ready'] is True and feedback['state'] == 'dry'
assert project.pad_key_lock[0] is False and not project.resolve_pad_key_lock(215)
assert project.sample_paths[0] == off_path and project.pad_content[0] == off_content
assert project.stem_cache[0].pair == disk_selection and registration.is_current()
assert controller.stems_available(0) and audio.worker_call_count() == off_worker_calls
assert transport_events == [('started', 215), ('started', 0)]
assert not session.stem_generation_errors and transport.residency.status(0)[1] is None
"#,
    );
}

fn public_off(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    reference_owners: &[Arc<InputRuntimeOwnership>; 2],
    paused: bool,
) {
    assert!(consumer.is_empty() && playback.candidate.is_none());
    assert_eq!(playback.ratio.to_bits(), 0.75_f64.to_bits());
    assert!(playback.complete_resident && playback.loop_start == 0 && playback.loop_end == 4096);
    let all = playback.selection == Some(0b1010);
    let initial_prepare_frames = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("initial_prepare_frames")
            .unwrap()
            .unwrap()
            .extract::<u64>()
            .unwrap()
    });
    let peer = Peer::new(playback, initial_prepare_frames);
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let historical_adoption = voice(&playback.callback.mixer).stretch.adopted_request_id();
    assert!(
        historical_adoption.is_some(),
        "own real preparation worker was adopted"
    );
    let history = voice(&playback.callback.mixer)
        .stretch
        .productive_history()
        .unwrap();
    assert_eq!(
        history.binding,
        crate::audio_engine::productive_source_history::ProductiveSourceBinding::new(
            voice(&playback.callback.mixer).sample.as_ref().unwrap(),
            RATE,
            None,
        )
    );
    assert_eq!(
        history.next_position,
        voice(&playback.callback.mixer).source_playback.position()
    );
    assert!(history.fed_output_frames >= 4096);
    assert!(
        voice(&playback.callback.mixer)
            .stretch
            .wet_current_source_output_ready()
    );
    let (old_source, old_source_view) = {
        let source = playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert_eq!((source.resident_start(), source.resident_end()), (0, 4096));
        assert_eq!(
            source.residency.as_ref().unwrap().context,
            ResidentContext::KeyLockFullTrack
        );
        (
            Arc::downgrade(&source.samples),
            Arc::downgrade(source.residency.as_ref().unwrap()),
        )
    };
    let old_stems = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .map(|set| {
            set.stems
                .each_ref()
                .map(|stem| Arc::downgrade(&stem.samples))
        });
    assert_eq!(old_stems.is_some(), all);
    let set_identity = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .map(|set| Arc::downgrade(&set.complete_set_identity));
    complete::feedback(locals, &mut playback.callback);
    controller_python(
        locals,
        r#"
off_source = transport.residency._mode_feedback(0).copy()
assert off_source['effective'] is True and off_source['ready'] is True
assert transport.residency.key_lock_status(0).effective is True
off_path, off_content = project.sample_paths[0], project.pad_content[0]
off_worker_calls = audio.worker_call_count()
assert project.pad_key_lock[0] is True and registration.is_current()
assert session.active_sample_ids == {0, 215} and not session.paused_sample_ids
"#,
    );
    if paused {
        controller_python(locals, "transport.playback.pause_pad(0)");
        remember_own_pause_admission(locals, true);
        assert_eq!(playback.drain(consumer), 1, "public own Pause");
        assert_own_pause_effect(locals);
        for reference in [&mut playback.reference, &mut playback.full_mix] {
            reference.pause_sample_at_output_frame(0, playback.frame);
        }
        paused_render(playback, 137);
        peer.assert_current(playback);
    }
    controller_python(
        locals,
        r#"
transport.pad.set_pad_key_lock(0, enabled=False)
off_window = transport.residency._pending[0].ticket
assert off_window is not None and off_window.key_lock_request_id is not None
assert off_window.key_lock_request_id != off_source['request_id']
assert project.pad_key_lock[0] is False
"#,
    );
    wait_until(|| consumer.peek().is_ok());
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::RelocateResident(_))
    ));
    // The own payload precedes completion of the worker's publication marker.
    // Its request guard is retained through that commit and predecessor settlement.
    drop(engine.pad_request_ids.lock().unwrap());
    controller_python(
        locals,
        r#"
status = transport.residency.key_lock_status(0)
assert not status.requested and status.pending and status.effective is True and status.error is None, repr(status)
assert project.stem_cache[0].pair == disk_selection and registration.is_current()
"#,
    );
    playback.callback.retirement.slots = 0;
    assert_eq!(
        playback.drain(consumer),
        0,
        "own OFF waits for callback retirement capacity"
    );
    for frames in [1, 31, 137, 257] {
        if paused {
            paused_render(playback, frames);
        } else {
            playback.render(frames, true);
        }
        assert!(playback.callback.mixer.key_lock_for_measurement(0));
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert_eq!(
            voice(&playback.callback.mixer).stretch.adopted_request_id(),
            historical_adoption
        );
        assert!(
            voice(&playback.callback.mixer)
                .stretch
                .productive_history()
                .is_some()
        );
        assert!(
            voice(&playback.callback.mixer)
                .stretch
                .wet_current_source_output_ready()
        );
        assert_eq!(playback.drain(consumer), 0);
        controller_python(
            locals,
            r#"
transport.on_frame_render()
controller.on_frame_render()
assert off_window.publication_status() == 'pending'
status = transport.residency.key_lock_status(0)
assert not status.requested and status.pending and status.effective is True and status.error is None
assert transport.residency._mode_feedback(0)['request_id'] == off_source['request_id']
assert transport_events == [('started', 215), ('started', 0)]
assert registration.is_current() and project.stem_cache[0].pair == disk_selection
"#,
        );
        peer.assert_current(playback);
    }
    let current = voice(&playback.callback.mixer);
    let clock = current.source_playback;
    let fifo = current.stretch.pending_fifo_frames();
    assert_ne!(
        fifo,
        (0, 0),
        "the actual wet adapter owns queued history before OFF"
    );
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let loop_region = playback.callback.mixer.loop_region_frames(0);
    let feedback_count = playback.callback.feedback.events.len();
    playback.callback.retirement.slots = usize::MAX;
    assert_eq!(
        playback.drain(consumer),
        1,
        "own public OFF mode/window ACK"
    );
    assert!(consumer.is_empty());
    let current = voice(&playback.callback.mixer);
    assert_eq!(current.generation, generation);
    assert_eq!(current.paused, paused);
    assert!(current.source_playback.matches_exact(&clock));
    assert_eq!(playback.callback.mixer.loop_region_frames(0), loop_region);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(
        current.stretch.pending_fifo_frames(),
        (0, 0),
        "OFF resets Native FIFOs"
    );
    assert!(
        current.stretch.productive_history().is_none(),
        "OFF retires productive Wet history"
    );
    assert!(!current.stretch.wet_render_ready());
    assert!(!current.stretch.wet_output_ready());
    assert!(!current.stretch.wet_current_source_output_ready());
    assert_eq!(current.stretch.adopted_request_id(), historical_adoption);
    assert!(!playback.callback.mixer.key_lock_for_measurement(0));
    assert_eq!(
        playback.callback.feedback.events.len(),
        feedback_count,
        "OFF creates no START/STOP"
    );
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    let source = playback.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_eq!(
        source.residency.as_ref().unwrap().context,
        ResidentContext::FullTrack
    );
    assert_eq!((source.resident_start(), source.resident_end()), (0, 4096));
    assert_eq!(source.samples.as_ref(), playback.source.samples.as_ref());
    assert_eq!(source.samples.len() * size_of::<f32>(), 4096 * 2 * 4);
    assert!(source.same_source(&playback.source));
    assert!(
        old_source.ptr_eq(&Arc::downgrade(&source.samples)),
        "same-range OFF shares the admitted immutable PCM"
    );
    assert!(
        !old_source_view.ptr_eq(&Arc::downgrade(source.residency.as_ref().unwrap())),
        "own OFF revision/context replaces the old ON view"
    );
    for (reference, owner) in [playback.reference.as_mut(), playback.full_mix.as_mut()]
        .into_iter()
        .zip(reference_owners)
    {
        let source = &playback.source;
        let request = owner.next_key_lock_request(0).unwrap();
        reference.set_pad_key_lock_request(
            0,
            false,
            request,
            InputPadBinding {
                placement: owner.binding_placement(0),
                resident: source.resident_binding(),
                source_address: source.source_address(),
                sample_count: source.source_sample_count(),
                channels: source.channels,
                sample_rate_hz: RATE,
                authority_revision: owner.authority_revision(0),
                runtime_revision: 0,
                accepted: None,
            },
            1,
        );
        assert!(!reference.key_lock_for_measurement(0));
        assert!(voice(reference).source_playback.matches_exact(&clock));
        assert_eq!(voice(reference).stretch.pending_fifo_frames(), (0, 0));
    }
    playback.raw = None;
    playback.candidate = None;
    complete::feedback(locals, &mut playback.callback);
    controller_python(locals, "assert off_window.is_current()");
    assert_off_status(locals);
    peer.assert_current(playback);
    let off_stems = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .map(|set| {
            assert!(
                set_identity
                    .as_ref()
                    .unwrap()
                    .ptr_eq(&Arc::downgrade(&set.complete_set_identity))
            );
            set.stems.each_ref().map(|stem| {
                assert_eq!((stem.resident_start(), stem.resident_end()), (0, 4096));
                Arc::downgrade(&stem.samples)
            })
        });
    assert_eq!(
        off_stems.is_some(),
        all,
        "OFF retains the selected ALL bank"
    );
    if let Some(stems) = &off_stems {
        for (old, current) in old_stems.as_ref().unwrap().iter().zip(stems) {
            assert!(
                old.ptr_eq(current),
                "same-range ALL context shares component PCM"
            );
        }
    }
    if paused {
        for frames in [1, 31, 137, 513] {
            paused_render(playback, frames);
            peer.assert_current(playback);
            assert_off_status(locals);
        }
        controller_python(
            locals,
            "assert session.paused_sample_ids == {0}\ntransport.playback.resume_pad(0)",
        );
        remember_own_pause_admission(locals, false);
        assert_eq!(playback.drain(consumer), 1, "public own Resume after OFF");
        assert_own_pause_effect(locals);
        for reference in [&mut playback.reference, &mut playback.full_mix] {
            reference.resume_sample_at_output_frame(0, playback.frame);
        }
        playback.resumed_at = Some(playback.frame);
    }
    // The real adopted Native owner pins the old ON view. OFF replaces its
    // revision/context while keeping same-range immutable source/component PCM.
    // This old view can disappear only after every actual off-thread reader is
    // retired; the underlying current PCM is deliberately still alive.
    playback.callback.retirement.retired.clear();
    reconcile(engine).unwrap();
    wait_until(|| old_source_view.strong_count() == 0);
    assert!(old_source.strong_count() > 0);
    if let Some(stems) = &off_stems {
        assert!(
            stems.iter().all(|stem| stem.strong_count() > 0),
            "actual OFF ALL bank still owns its components"
        );
    }
    let begin = playback.source_frame;
    let mut remaining = playback.source.frame_count() * 3 + 4096;
    let mut audible = false;
    let mut differs_from_full_mix = false;
    for partition in [1_usize, 31, 137, 512, 777].into_iter().cycle() {
        let frames = partition.min(remaining);
        let (actual, full_mix) =
            complete::render_off_with_historical_adoption(playback, frames, historical_adoption);
        audible |= actual.iter().any(|sample| sample.abs() > 0.001);
        differs_from_full_mix |= actual
            .iter()
            .zip(full_mix)
            .any(|(actual, full)| (actual - full).abs() > 0.005);
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .pending_fifo_frames(),
            (0, 0)
        );
        peer.assert_current(playback);
        assert_off_status(locals);
        remaining -= frames;
        if remaining == 0 {
            break;
        }
    }
    assert!(audible);
    assert_eq!(differs_from_full_mix, all);
    let fractional_wraps = (begin..playback.source_frame)
        .filter(|frame| {
            let phase = (*frame as f64 * playback.ratio).rem_euclid(playback.loop_period);
            phase.floor() as usize == 4095 && phase.fract() > 0.0
        })
        .count();
    assert!(
        fractional_wraps >= 3,
        "complete dry end-to-start interpolation after OFF"
    );
    if let Some(stems) = off_stems {
        release_all_after_off(playback, consumer, locals, historical_adoption);
        assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
        playback.callback.retirement.retired.clear();
        reconcile(engine).unwrap();
        wait_until(|| stems.iter().all(|stem| stem.strong_count() == 0));
        assert!(
            old_stems
                .unwrap()
                .iter()
                .all(|stem| stem.strong_count() == 0)
        );
        assert_off_status(locals);
        peer.assert_current(playback);
    }
    complete::feedback(locals, &mut playback.callback);
    controller_python(
        locals,
        "assert session.active_sample_ids == {0, 215} and not session.paused_sample_ids\nassert transport_events == [('started', 215), ('started', 0)]",
    );
}

fn release_all_after_off(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    historical_adoption: Option<u64>,
) {
    controller_python(
        locals,
        r#"
assert controller.set_stem_mix_mode(0, 'full_mix')
off_release = controller._committed_residency._selections[0].receipt
assert off_release is not first and off_release.effective_mode_status() == 'waiting'
assert project.pad_stem_mix_mode[0] == 'full_mix'
assert project.stem_cache[0].pair == disk_selection and registration.is_current()
"#,
    );
    assert_eq!(
        playback.drain(consumer),
        1,
        "own public OFF ALL release Capture"
    );
    wait_until(|| consumer.peek().is_ok());
    complete::render_off_with_historical_adoption(playback, 31, historical_adoption);
    let clock = voice(&playback.callback.mixer).source_playback;
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    assert_eq!(
        playback.drain(consumer),
        1,
        "own public OFF ALL release Adopt"
    );
    assert_eq!(
        controller_ticket_status(locals, "off_release", "residency_status"),
        "accepted"
    );
    assert_eq!(
        controller_ticket_status(locals, "off_release", "effective_mode_status"),
        "transitioning"
    );
    let current = voice(&playback.callback.mixer);
    assert!(current.source_playback.matches_exact(&clock));
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), (0, 0));
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::FullMix, 0)
    );
    playback.select(None);
    let mut remaining = (128.0 / playback.ratio).ceil() as usize;
    for partition in [1_usize, 31, 17, 7].into_iter().cycle() {
        let frames = partition.min(remaining);
        complete::render_off_with_historical_adoption(playback, frames, historical_adoption);
        remaining -= frames;
        if remaining == 0 {
            break;
        }
        assert_eq!(
            controller_ticket_status(locals, "off_release", "effective_mode_status"),
            "transitioning"
        );
    }
    complete::render_off_with_historical_adoption(playback, 137, historical_adoption);
    assert_eq!(
        controller_ticket_status(locals, "off_release", "effective_mode_status"),
        "effective"
    );
    complete::feedback(locals, &mut playback.callback);
}
