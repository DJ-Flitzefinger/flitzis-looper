//! Complete physical-source birth and committed ALL through the public controllers.
//! The source fixture is adopted without a device; no fixture creates the tested voice.

use super::*;

const CHILD: &str = "FLITZI_COMMITTED_PUBLIC_COMPLETE_CHILD";
const OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::committed_stem_public_complete_tests::public_complete_all_full_mix_fresh_all_off";
const ON_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::committed_stem_public_complete_tests::public_complete_all_full_mix_fresh_all_on";

/// Forward the actual engine's tagged feedback, original input clock and two-argument
/// captured-start dependency API. This facade grants no replacement receipt.
#[pyclass]
struct PublicFeedbackAudio {
    engine: Arc<AudioEngine>,
}

#[pymethods]
impl PublicFeedbackAudio {
    fn _resolve_pad_feedback(&self, message: &AudioMessage) -> PyResult<Option<usize>> {
        self.engine._resolve_pad_feedback(message)
    }

    fn _resolve_ordinary_transport_feedback(
        &self,
        message: &AudioMessage,
    ) -> PyResult<Option<crate::audio_engine::ordinary_transport::OrdinaryTransportObservation>>
    {
        self.engine._resolve_ordinary_transport_feedback(message)
    }

    fn capture_input_timestamp_ns(&self) -> u64 {
        self.engine.capture_input_timestamp_ns()
    }

    #[pyo3(signature = (captured, window=None))]
    fn prepare_captured_pad_start(
        &self,
        captured: &crate::audio_engine::pad_start::CapturedPadStart,
        window: Option<&crate::audio_engine::resident_relocation::ResidentWindowTicket>,
    ) -> PyResult<crate::audio_engine::resident_relocation::ResidentWindowTicket> {
        self.engine
            .python_prepare_captured_pad_start(captured, window)
    }
}

#[test]
fn public_complete_all_full_mix_fresh_all_off() {
    isolated(false, OFF_TEST);
}

#[test]
fn public_complete_all_full_mix_fresh_all_on() {
    isolated(true, ON_TEST);
}

fn isolated(key_lock: bool, test: &str) {
    if std::env::var_os(CHILD).is_some() {
        history(key_lock);
        eprintln!("public complete committed child PID={}", std::process::id());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "public complete committed child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("public complete committed child PID=")
    );
}

/// Deliver the actual callback objects to the same handlers used by the App.
/// The collecting sink only replaces the device feedback FIFO in this child.
pub(super) fn feedback(locals: &Py<PyDict>, callback: &mut Callback) {
    let events = std::mem::take(&mut callback.feedback.events);
    Python::attach(|py| {
        let events = events
            .into_iter()
            .map(|event| event.into_pyobject(py).unwrap().unbind())
            .collect::<Vec<_>>();
        locals.bind(py).set_item("native_messages", events).unwrap();
    });
    controller_python(
        locals,
        r#"
transport.playback.set_pause_observation_fence(feedback_route._ordinary_observation_fenced)
if feedback_route._ordinary_feedback_audio is not feedback_route._audio:
    feedback_route._pending_ordinary_feedback.clear()
    feedback_route._ordinary_feedback_owner = None
    feedback_route._ordinary_feedback_audio = feedback_route._audio
ordinary_pending = tuple(feedback_route._pending_ordinary_feedback.values())
feedback_route._pending_ordinary_feedback.clear()
for message in ordinary_pending:
    assert feedback_route._handle_content_feedback(message)
for message in native_messages:
    assert isinstance(message, messages) and type(message) is not messages, type(message)
    if feedback_route._handle_content_feedback(message):
        continue
    if isinstance(message, messages.SampleStarted):
        transport_events.append(('started', message.sample_id()))
        transport.playback.handle_sample_started_message(message)
    elif isinstance(message, messages.SampleStopped):
        transport_events.append(('stopped', message.sample_id()))
        transport.playback.handle_sample_stopped_message(message)
native_messages.clear()
transport.on_frame_render()
controller.on_frame_render()
"#,
    );
    // Production retains exact Pending events across frames. This device-free
    // carrier observes only completion of that same control publication; it
    // neither recaptures an owner nor advances the callback/source clock.
    // Ordinary feedback instead remains retained until a later feedback pass:
    // a real Loader/Pair/Bank fence may still be held by this caller.
    wait_until(|| {
        controller_python(
            locals,
            r#"
for _ in range(len(feedback_route._pending_content_feedback)):
    feedback_route._handle_content_feedback(feedback_route._pending_content_feedback.popleft())
"#,
        );
        Python::attach(|py| {
            locals
                .bind(py)
                .get_item("feedback_route")
                .unwrap()
                .unwrap()
                .getattr("_pending_content_feedback")
                .unwrap()
                .len()
                .unwrap()
                == 0
        })
    });
}

pub(super) fn other(mixer: &RtMixer) -> &crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(215))
        .unwrap()
}

fn assert_extent(
    sample: &SampleBuffer,
    full: &SampleBuffer,
    key_lock: bool,
    start: usize,
    end: usize,
) {
    assert!(sample.same_source(full));
    assert_eq!(sample.frame_count(), full.frame_count());
    assert_eq!(
        (sample.resident_start(), sample.resident_end()),
        (start, end)
    );
    assert_eq!(
        sample.samples.len() * size_of::<f32>(),
        (end - start) * 2 * 4
    );
    assert_eq!(sample.samples.as_ref(), &full.samples[start * 2..end * 2]);
    assert_eq!(
        sample.residency.as_ref().unwrap().context,
        match (key_lock, start == 0 && end == full.frame_count()) {
            (false, true) => ResidentContext::FullTrack,
            (true, true) => ResidentContext::KeyLockFullTrack,
            (false, false) => ResidentContext::FiniteLoop,
            (true, false) => ResidentContext::KeyLockFiniteLoop,
        },
    );
}

fn reference_with_ownership(
    full: &SampleBuffer,
    stems: &PreparedStemSet,
    ratio: f64,
    key_lock: bool,
    execution: u64,
    pitch: crate::audio_engine::pitch_intent::PitchIntent,
    launch: (
        LoopPhaseReference,
        &crate::audio_engine::transport::TransportTimeline,
    ),
) -> (Box<RtMixer>, Arc<InputRuntimeOwnership>) {
    let mut mixer = Box::new(RtMixer::new(2, RATE as f32));
    // Immutable standalone PCM oracle: its unrestricted permit has no Engine source owner.
    let ownership = Arc::new(InputRuntimeOwnership::default());
    ownership.publish_test_source(0, full, RATE, 1);
    mixer.set_input_runtime_ownership(ownership.clone());
    mixer.load_sample(0, full.clone());
    assert!(mixer.publish_prepared_stems(0, stems.clone()));
    mixer.set_loop_region_frames(0, 0, Some(full.frame_count()));
    mixer.set_speed(ratio);
    let request = ownership.next_key_lock_request(0).unwrap();
    mixer.set_pad_key_lock_request(
        0,
        key_lock,
        request,
        InputPadBinding {
            placement: ownership.binding_placement(0),
            resident: full.resident_binding(),
            source_address: full.source_address(),
            sample_count: full.source_sample_count(),
            channels: full.channels,
            sample_rate_hz: RATE,
            authority_revision: ownership.authority_revision(0),
            runtime_revision: 0,
            accepted: None,
        },
        1,
    );
    // Declared intent belongs only to this independent PCM reference. The tested
    // mixer receives its tuple through the production PitchController below.
    mixer.set_pad_pitch_intent(0, pitch).unwrap();
    mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, execution));
    install_public_start_reference(&mut mixer, launch.0, launch.1);
    if pitch.native_pitch_scale(ratio, key_lock).unwrap() != 1.0 {
        // The public ordinary attack owns prepared physical-birth context before
        // its first sample.
        // Give this independent full-file reference its own source/Native owner at that
        // same physical launch endpoint; the raw oracle below independently checks PCM.
        let acknowledgements: Arc<
            crate::audio_engine::constant_timing::CurrentTimingAcknowledgements,
        > = Arc::default();
        mixer.set_current_timing_acknowledgements(acknowledgements.clone());
        let index = mixer
            .voices
            .iter()
            .position(|voice| voice.is_playing_sample(0))
            .unwrap();
        let mut snapshot = mixer.capture_pitch_voice_snapshot(index).unwrap();
        snapshot.stems = None;
        let epoch = mixer.voices[index].stretch.preparation_epoch();
        let permit = crate::audio_engine::native_history_permit::NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: mixer.voices[index].stretch.source_epoch_owner(),
        }
        .capture(&snapshot.sample, RATE, None)
        .unwrap();
        let request = snapshot
            .into_request(permit, pitch, key_lock, execution, 1, epoch)
            .unwrap();
        let mut candidate = Box::new(
            crate::audio_engine::prepared_native_history::NativeAdapterState::new(
                Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
                2,
            ),
        );
        candidate.prepare_source_at_physical_birth(request).unwrap();
        let mut snapshot = mixer.capture_pitch_voice_snapshot(index).unwrap();
        snapshot.stems = None;
        let feed = crate::audio_engine::stretch_processor::ProductiveSourceFeed {
            pitch_intent: pitch,
            key_lock,
            sample: &snapshot.sample,
            stems: None,
            outgoing_stems: None,
            sample_rate_hz: RATE,
            accepted: None,
            plan: snapshot.plan,
            playback: &snapshot.playback,
            permit: None,
            output_frame: Some(execution),
        };
        assert!(
            mixer.voices[index]
                .stretch
                .adopt_prepared_pitch(&mut candidate, &feed, epoch, 1)
        );
    }
    (mixer, ownership)
}

fn initial_raw_native(
    ratio: f64,
    pitch: crate::audio_engine::pitch_intent::PitchIntent,
    key_lock: bool,
) -> ControllerRawNative {
    let scale = pitch.native_pitch_scale(ratio, key_lock).unwrap();
    let mut raw = ControllerRawNative::new_with_pitch(scale, true);
    let history_frames = ControllerRawNative::constant_history_frames(scale);
    // The fresh birth has zero input before its physical origin. Independently
    // retain the exact H and Native/FIFO chronology without wrapping a future tail.
    // No production playback clock, reader or preparation helper supplies this feed.
    let history = std::array::from_fn(|_| vec![0.0; history_frames]);
    raw.render(&history);
    raw
}

fn render(
    playback: &mut ControllerPlayback,
    key_lock: bool,
    frames: usize,
) -> (Vec<f32>, Vec<f32>) {
    if key_lock || playback.raw.is_some() {
        return playback.render(frames, true);
    }
    render_off_with_historical_adoption(playback, frames, None)
}

pub(super) fn render_off_with_historical_adoption(
    playback: &mut ControllerPlayback,
    frames: usize,
    historical_adoption: Option<u64>,
) -> (Vec<f32>, Vec<f32>) {
    assert!(!playback.callback.mixer.key_lock_for_measurement(0));
    // Inspect the actual OFF scratch for each bounded mixer chunk, while
    // retaining the caller's full partition and uninterrupted PCM/EQ oracle.
    let capacity = playback.callback.mixer.max_realtime_render_frames();
    if frames > capacity {
        let mut actual = Vec::new();
        let mut full_mix = Vec::new();
        let mut remaining = frames;
        while remaining > 0 {
            let count = remaining.min(capacity);
            let (output, counterfactual) =
                render_off_with_historical_adoption(playback, count, historical_adoption);
            actual.extend(output);
            full_mix.extend(counterfactual);
            remaining -= count;
        }
        return (actual, full_mix);
    }
    assert!(playback.raw.is_none() && playback.candidate.is_none());
    assert!(frames <= playback.callback.mixer.max_realtime_render_frames());
    let feed = playback.feed(playback.source_frame, frames);
    let before = voice(&playback.callback.mixer);
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let mut actual = vec![0.0; frames * 2];
    let mut expected = vec![0.0; frames * 2];
    let mut full_mix = vec![0.0; frames * 2];
    reset_tap_observation_for_test();
    playback.callback.mixer.render_rt_at_output_frame(
        &mut actual,
        &mut [0.0; NUM_SAMPLES],
        playback.frame,
        &mut playback.callback.retirement,
    );
    let taps = tap_observation_for_test();
    assert_eq!(taps.missing_reads, 0);
    assert!(
        taps.min_frame
            .is_some_and(|frame| frame >= playback.resident_start())
    );
    assert!(
        taps.max_frame
            .is_some_and(|frame| frame < playback.resident_end())
    );
    for (mixer, output) in [
        (playback.reference.as_mut(), &mut expected),
        (playback.full_mix.as_mut(), &mut full_mix),
    ] {
        mixer.render_at_output_frame(playback.frame, output, &mut [0.0; NUM_SAMPLES]);
    }
    assert_eq!(actual, expected, "public complete OFF mixer chronology");
    let current = voice(&playback.callback.mixer);
    for (channel, samples) in feed.iter().enumerate() {
        assert_eq!(
            &current.stretch.varispeed_buffers()[channel][..frames],
            samples
        );
        assert_eq!(
            &current.stretch.output_buffers()[channel][..frames],
            samples
        );
    }
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    assert!(current.stretch.productive_history().is_none());
    assert!(!current.stretch.wet_render_ready());
    assert!(!current.stretch.wet_output_ready());
    assert!(!current.stretch.wet_current_source_output_ready());
    assert_eq!(current.stretch.adopted_request_id(), historical_adoption);
    for mixer in [&playback.reference, &playback.full_mix] {
        assert!(
            current
                .source_playback
                .matches_exact(&voice(mixer).source_playback)
        );
    }
    let phase = ((playback.source_frame + frames as u64) as f64 * playback.ratio)
        .rem_euclid(playback.loop_period);
    assert_eq!(
        current.source_playback.position().frame,
        phase.floor() as usize
    );
    assert_eq!(
        current.source_playback.position().fraction.to_bits(),
        phase.fract().to_bits()
    );
    assert_eq!(
        current.source_playback.tempo_ratio().to_bits(),
        playback.ratio.to_bits()
    );
    for (frame, output) in actual.chunks_exact(2).enumerate() {
        playback.filter.begin_frame();
        for channel in 0..2 {
            assert_eq!(
                output[channel],
                playback
                    .filter
                    .process_sample(channel, feed[channel][frame]),
                "independent complete OFF PCM and uninterrupted EQ",
            );
        }
    }
    for mixer in [
        &mut playback.callback.mixer,
        &mut playback.reference,
        &mut playback.full_mix,
    ] {
        assert!(!voice_mut(mixer).stretch.source_preparation_ready());
    }
    playback.frame += frames as u64;
    playback.source_frame += frames as u64;
    (actual, full_mix)
}

fn adopt(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    name: &str,
    key_lock: bool,
) {
    assert_eq!(
        playback.drain(consumer),
        1,
        "own committed component Capture"
    );
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, name, "residency_status") == "rejected"
    });
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "pending"
    );
    render(playback, key_lock, 31);
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(playback.drain(consumer), 1, "own committed component Adopt");
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
        let after = after.stretch.productive_history().unwrap();
        assert_eq!(after.binding, history.binding);
        assert_eq!(after.next_position, history.next_position);
        assert_eq!(after.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(after.stretch.productive_history().is_none());
    }
    let after = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after.binding, dsp.binding);
    assert_eq!(after.next_position, dsp.next_position);
    assert_eq!(after.fed_output_frames, dsp.fed_output_frames);
    feedback(locals, &mut playback.callback);
}

fn finish_ramp(playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str, key_lock: bool) {
    let total = (128.0 / playback.ratio).ceil() as usize;
    let mut remaining = total - (playback.source_frame - playback.ramp_start) as usize;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    for partition in [1_usize, 31, 17, 7].into_iter().cycle() {
        let frames = partition.min(remaining);
        render(playback, key_lock, frames);
        remaining -= frames;
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
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
    if key_lock {
        assert_eq!(
            controller_ticket_status(locals, name, "effective_mode_status"),
            "transitioning"
        );
    }
}

fn settle(playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str, key_lock: bool) {
    finish_ramp(playback, locals, name, key_lock);
    let (actual, full_mix) = if key_lock {
        render(playback, true, 13);
        playback.adopt_native(false)
    } else {
        let mut actual = Vec::new();
        let mut full_mix = Vec::new();
        for frames in [1, 127, 257, 511, 31, 137] {
            let (output, counterfactual) = render(playback, false, frames);
            actual.extend(output);
            full_mix.extend(counterfactual);
        }
        (actual, full_mix)
    };
    assert!(actual.iter().any(|sample| sample.abs() > 0.001));
    if playback.selection.is_some() {
        assert!(
            actual
                .iter()
                .zip(full_mix)
                .any(|(actual, full)| (actual - full).abs() > 0.005),
            "own audible complete ALL differs from predecessor FullMix",
        );
    }
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "effective"
    );
    feedback(locals, &mut playback.callback);
}

fn pause_ramp(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) {
    render(playback, key_lock, 31);
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    controller_python(
        locals,
        "transport.playback.pause_pad(0)\ntransport.playback.pause_pad(215)\nassert not session.paused_sample_ids",
    );
    l3_controller_productive_tests::remember_public_pause_admissions(locals, true);
    assert_eq!(
        playback.drain(consumer),
        2,
        "actual public Pause during component ramp"
    );
    l3_controller_productive_tests::assert_public_pause_effect(locals);
    for mixer in [&mut playback.reference, &mut playback.full_mix] {
        mixer.pause_sample_at_output_frame(0, playback.frame);
    }
    let other_clock = other(&playback.callback.mixer).source_playback;
    let consumed = playback.source_frame;
    let ramp_start = playback.ramp_start;
    playback.render_paused(513);
    assert_eq!(playback.source_frame, consumed);
    assert_eq!(playback.ramp_start, ramp_start);
    assert!(
        other(&playback.callback.mixer)
            .source_playback
            .matches_exact(&other_clock)
    );
    assert_eq!(
        controller_ticket_status(locals, "first", "effective_mode_status"),
        "transitioning"
    );
    feedback(locals, &mut playback.callback);
    controller_python(
        locals,
        "transport.playback.resume_pad(0)\ntransport.playback.resume_pad(215)\nassert session.paused_sample_ids == {0, 215}",
    );
    l3_controller_productive_tests::remember_public_pause_admissions(locals, false);
    assert_eq!(
        playback.drain(consumer),
        2,
        "actual public Resume retains consumed ramp clock"
    );
    l3_controller_productive_tests::assert_public_pause_effect(locals);
    for mixer in [&mut playback.reference, &mut playback.full_mix] {
        mixer.resume_sample_at_output_frame(0, playback.frame);
    }
    playback.resumed_at = Some(playback.frame);
    feedback(locals, &mut playback.callback);
}

fn public_window_ack(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
) {
    controller_python(
        locals,
        "window = transport.residency._pending[0].ticket\nassert window is not None",
    );
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, "window", "publication_status") == "rejected"
    });
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    assert_eq!(playback.drain(consumer), 1, "own public loop window ACK");
    assert_eq!(
        controller_ticket_status(locals, "window", "publication_status"),
        "accepted"
    );
    let after = voice(&playback.callback.mixer);
    assert_eq!(after.generation, generation);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    // A changed loop rebases the clock's internal epoch. Its actual physical
    // phase and applied/target rate remain unchanged at this non-rendering ACK.
    assert_eq!(after.source_playback.position(), clock.position());
    assert_eq!(
        after.source_playback.tempo_ratio().to_bits(),
        clock.tempo_ratio().to_bits()
    );
    assert_eq!(
        after.source_playback.rate_target().to_bits(),
        clock.rate_target().to_bits()
    );
    feedback(locals, &mut playback.callback);
    controller_python(
        locals,
        "assert window.is_current()\nassert transport.residency.status(0)[1] is None",
    );
}

pub(super) type PublicCompleteFollowup = fn(
    &mut ControllerPlayback,
    &mut rtrb::Consumer<ControlMessage>,
    &Py<PyDict>,
    &Arc<AudioEngine>,
    &[Arc<InputRuntimeOwnership>; 2],
);

fn history(key_lock: bool) {
    history_with_followup(key_lock, None);
}

/// Reuse the actual public birth, optionally ending after one bounded follow-up.
/// Existing complete histories pass None and retain their original chronology.
pub(super) fn history_with_followup(
    key_lock: bool,
    followup: Option<(bool, PublicCompleteFollowup)>,
) {
    history_with_profile(key_lock, followup, if key_lock { 0.75 } else { 1.25 }, None);
}

/// Apply a genuine stopped piano SET before the ordinary public complete birth.
/// Both lock profiles retain r=1 and independently compare their h=2 Native PCM.
pub(super) fn history_with_piano_followup(key_lock: bool, followup: PublicCompleteFollowup) {
    history_with_profile(key_lock, Some((false, followup)), 1.0, Some(12));
}

fn history_with_profile(
    key_lock: bool,
    followup: Option<(bool, PublicCompleteFollowup)>,
    ratio: f64,
    piano_note: Option<i32>,
) {
    let pitch =
        crate::audio_engine::pitch_intent::PitchIntent::new(piano_note.unwrap_or(0), 0).unwrap();
    let needs_native = pitch.native_pitch_scale(ratio, key_lock).unwrap() != 1.0;
    let previous_cwd = std::env::current_dir().unwrap();
    let mut h = long_harness();
    h.callback
        .mixer
        .set_prepared_source_epochs(h.engine.prepared_source_epochs.clone());
    std::env::set_current_dir(h.root.parent().unwrap()).unwrap();
    let source_ticket = h.ticket(0);
    let pair = h.prepare(0, &source_ticket, true, None);
    assert_eq!(
        pair.pair_reader.descriptor.content.alignment.offset_frames,
        0
    );
    let disk_selection = selection(&pair);
    let _saved = h.save(&pair);
    let full = h.material.sample.clone();
    let full_stems = PreparedStemSet {
        complete_set_identity: Arc::new([0x76; 32]),
        reference_samples: full.samples.clone(),
        reference_residency: full.residency.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        accepted_timing: None,
        source_version_hash: crate::audio_engine::stem_cache::source_version_hash(&h.version),
        sample_rate_hz: RATE,
        channels: 2,
        frame_count: full.frame_count(),
        available_mask: 0b1111,
        stems: pair
            .component_views_for_test()
            .unwrap()
            .each_ref()
            .map(|component| {
                let mut independent = component.clone();
                independent.samples = Arc::from(component.samples.as_ref().to_vec());
                independent
            }),
    };
    drop(pair);
    // Supply the actual public command producer for the independent zero-volume pad.
    // The child delivers only actual Callback feedback objects through feedback().
    let (_feedback_producer, feedback_consumer) = rtrb::RingBuffer::new(32);
    h.engine.test_control_channels =
        Some((h.producer.clone(), Arc::new(Mutex::new(feedback_consumer))));
    h.engine.test_output_format = Some((RATE, 2));
    let pitch_clock = piano_note.map(|_| {
        let runtime = crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            h.producer.clone(),
            h.engine.input_clock,
            h.engine.input_runtime_ownership.clone(),
        );
        runtime.set_render_format(RATE, 2).unwrap();
        let clock = Arc::new(crate::audio_engine::timing::SharedOutputClock::new());
        runtime.attach_pitch_clock(clock.clone()).unwrap();
        h.engine.input_runtime = Some(runtime);
        clock
    });
    h.callback.feedback.events.clear();
    h.callback.mixer.set_speed(ratio);
    h.callback.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    h.engine.play_sample(215, 0.0, None).unwrap();
    let ControlMessage::CapturePadStartTarget(peer_start) = h.consumer.peek().unwrap() else {
        panic!("public peer must retain its original target before PreparedStart");
    };
    let peer_start = peer_start.clone();
    assert_eq!(peer_start.control_guard().unwrap().id, 215);
    assert!(!peer_start.exclusive() && peer_start.effective_frame().is_none());
    assert_eq!(
        h.callback.drain(&mut h.consumer),
        2,
        "original target then one public zero-volume PreparedStart"
    );
    assert!(h.consumer.is_empty() && h.callback.scheduler.is_empty());
    assert_eq!(peer_start.original_frame(), Some(0));
    assert_eq!(
        peer_start.execution_frame(),
        Some(0),
        "peer admission state={} reason={:?}",
        peer_start.state(),
        peer_start.reason(),
    );
    assert_eq!(
        peer_start.effective_frame(),
        Some(0),
        "peer start state={} reason={:?} key_lock={} guard={:?}",
        peer_start.state(),
        peer_start.reason(),
        h.callback.mixer.key_lock_for_measurement(215),
        peer_start.control_guard(),
    );
    assert_eq!(
        h.callback
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.is_playing_sample(215))
            .count(),
        1
    );
    assert_eq!(other(&h.callback.mixer).volume, 0.0);
    assert_eq!(other(&h.callback.mixer).source_playback.position().frame, 0);
    assert_eq!(
        other(&h.callback.mixer)
            .source_playback
            .tempo_ratio()
            .to_bits(),
        ratio.to_bits()
    );
    assert_eq!(
        other(&h.callback.mixer).stretch.pending_fifo_frames(),
        (0, 0)
    );
    assert!(
        other(&h.callback.mixer)
            .stretch
            .productive_history()
            .is_none()
    );
    assert_eq!(
        h.callback
            .feedback
            .events
            .iter()
            .filter(|event| matches!(
                event,
                AudioMessage::SampleStarted { id: 215 }
                    | AudioMessage::ContentFeedback {
                        id: 215,
                        kind: 1,
                        ..
                    }
            ))
            .count(),
        1,
        "target capture must not duplicate the peer START"
    );
    let other_generation = other(&h.callback.mixer).generation;
    let other_native = other(&h.callback.mixer).stretch.native_state_address();
    let other_fifo = other(&h.callback.mixer).stretch.pending_fifo_frames();
    let other_sample = other(&h.callback.mixer).sample.as_ref().unwrap().clone();
    let other_demand = h.callback.mixer.stem_demand_for_measurement(215);
    let metadata = h.material.metadata();
    let fixture = json!({
        "original_reference": metadata["new_reference"], "material_id": metadata["material_id"],
        "source_version": h.version, "pair": disk_selection, "frames": full.frame_count(), "rate": RATE,
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
        locals.set_item("key_lock", key_lock).unwrap();
        locals.set_item("piano_note", piano_note).unwrap();
        locals
            .set_item("messages", py.get_type::<AudioMessage>())
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
                "feedback_audio",
                Py::new(
                    py,
                    PublicFeedbackAudio {
                        engine: engine.clone(),
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
from types import SimpleNamespace
from flitzis_looper.controller.app import AppController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.models import ProjectState, SessionState, StemCacheEntry, PadContentIdentity, PadStemIntent
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.stem_workers import StemWorkerPool
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.stem_pair_selection import StemPairSelection
fixture = json.loads(input_json)
project, session = ProjectState(), SessionState()
project.sample_paths[0] = project.sample_paths[215] = fixture['original_reference']
project.sample_durations[0] = project.sample_durations[215] = fixture['frames'] / fixture['rate']
project.pad_content[0] = PadContentIdentity(instance_id='9' * 32, material_id=fixture['material_id'])
project.pad_content[215] = PadContentIdentity(instance_id='a' * 32, material_id=fixture['material_id'])
project.pad_timing_intent[0] = 'legacy'
project.pad_key_lock[0] = key_lock
project.pad_key_lock[215] = False
project.pad_stem_mix_mode[0] = 'full_mix'
if piano_note is not None:
    from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
    project.pad_key_intent[0] = PadKeyIntent(source=SourceKeyVersion(raw_key='C'))
    project.multi_loop = True
    project.bpm_lock = False
    project.trigger_quantization_enabled = False
disk_selection = StemPairSelection.model_validate(fixture['pair'])
project.stem_cache[0] = StemCacheEntry(source_version=fixture['source_version'],
    cache_dir=disk_selection.wav_generation, stems=expected_stem_files(disk_selection.wav_generation),
    pair=disk_selection, available=True)
project.pad_stem_intent[0] = PadStemIntent(enabled_mask=0b1010, display_mode='custom')
controller = StemController(project, session, audio)
assert isinstance(controller._stem_worker_pool, StemWorkerPool)
assert controller._stem_worker_pool._executor._max_workers == 2
captured_start_calls, captured_dependency_calls = [], []
class PublicTransportAudio:
    def __getattr__(self, name):
        return getattr(playback_audio, name)
    def capture_pad_start(self, *args, **kwargs):
        original = playback_audio.capture_pad_start(*args, **kwargs)
        captured_start_calls.append((original, kwargs.get('received_at_ns')))
        return original
    def prepare_captured_pad_start(self, captured, window=None):
        captured_dependency_calls.append((captured, window))
        return feedback_audio.prepare_captured_pad_start(captured, window)
    def play_sample(self, *args, **kwargs):
        raise AssertionError('complete cache Play must await its own public resident ACK')
    play_sample_exclusive = play_sample
transport = TransportController(project, session, PublicTransportAudio())
transport_events = []
class ObservedFeedbackPlayback:
    def handle_sample_started_message(self, message, *, sample_id):
        transport_events.append(('started', sample_id))
        transport.playback.handle_sample_started_message(message, sample_id=sample_id)
    def handle_sample_stopped_message(self, message, *, sample_id):
        transport_events.append(('stopped', sample_id))
        transport.playback.handle_sample_stopped_message(message, sample_id=sample_id)
    def handle_ordinary_transport_observation(self, observation):
        transport.playback.handle_ordinary_transport_observation(observation)
class CurrentFixtureLoader:
    def __init__(self):
        self._observed_loader = False
    def incoming_load_request_id(self, sample_id):
        current_loader = globals().get('loader')
        if current_loader is None:
            # The initial Native source is already adopted; LoaderB has not begun.
            assert not self._observed_loader
            assert not session.loading_sample_ids and not session.pending_sample_paths
            return None
        self._observed_loader = True
        return current_loader.incoming_load_request_id(sample_id)
class PublicFeedbackRoute:
    # Exercise the production dispatch/retention methods without constructing
    # AppController, opening a stream, or substituting a Native receipt.
    _init_audio_feedback = AppController._init_audio_feedback
    _handle_content_feedback = AppController._handle_content_feedback
    _handle_ordinary_transport_feedback = AppController._handle_ordinary_transport_feedback
    _ordinary_observation_fenced = AppController._ordinary_observation_fenced
    _retain_ordinary_transport_feedback = AppController._retain_ordinary_transport_feedback
    _retain_content_feedback = AppController._retain_content_feedback
    def __init__(self):
        self._audio = feedback_audio
        self._session = session
        self.transport = SimpleNamespace(playback=ObservedFeedbackPlayback())
        self.loader = CurrentFixtureLoader()
        self.metering = MeteringController(project, session, feedback_audio)
        self.pairs = SimpleNamespace(observer_fenced_sample_ids=set())
        self.banks = SimpleNamespace(observer_fenced_sample_ids=set())
        self.arrangement = SimpleNamespace(fenced_sample_ids=set())
        self._init_audio_feedback()
feedback_route = PublicFeedbackRoute()
transport.playback.set_pause_observation_fence(feedback_route._ordinary_observation_fenced)
assert not session.active_sample_ids
controller.restore_stem_cache_from_project_state()
assert controller.publish_restored_stem_cache_if_available(0)
"#,
        );
        locals.unbind()
    });
    feedback(&locals, &mut h.callback);
    controller_python(
        &locals,
        "assert session.active_sample_ids == {215}\nassert transport_events == [('started', 215)]",
    );
    wait_until(|| {
        controller_python(
            &locals,
            "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
        );
        let native_register_queued = matches!(
            h.consumer.peek(),
            Ok(ControlMessage::CommittedStemWork(work))
                if matches!(work.as_ref(), crate::audio_engine::committed_stem_residency::CommittedStemRtWork::Register { id: 0, .. })
        );
        native_register_queued
            && Python::attach(|py| {
                locals
                    .bind(py)
                    .get_item("controller")
                    .unwrap()
                    .unwrap()
                    .call_method1("stem_residency_status", (0,))
                    .unwrap()
                    .getattr("registration")
                    .map(|value| value.extract::<String>().unwrap() == "pending")
                    .unwrap_or(false)
            })
    });
    assert_eq!(
        h.callback.drain(&mut h.consumer),
        1,
        "own committed set registration ACK"
    );
    feedback(&locals, &mut h.callback);
    if let Some(clock) = pitch_clock {
        let now = engine.input_clock.capture_ns();
        clock.publish(crate::audio_engine::timing::OutputClockSnapshot {
            valid: true,
            observed_at_ns: now,
            audible_at_ns: now,
            output_frame: 0,
            sample_rate_hz: RATE,
            master_period_seconds: Some(0.5),
            downbeat_frame: 0,
            master_beat: 0.0,
            freshness_ns: 2_000_000_000,
        });
        controller_python(
            &locals,
            r#"
runtime_loaded = [False] * len(project.sample_paths)
runtime_starts = [0.] * len(project.sample_paths)
runtime_ends = [None] * len(project.sample_paths)
runtime_bindings = [None] * len(project.sample_paths)
runtime_pitch_rows = [dict(source_root=None, source_mode=None,
    correction_root=None, correction_mode=None, base_shift=0, extra_shift=0,
    key_lock=False, quantize_step_64ths=None, sync=False, request_sequence=0)
    for _ in project.sample_paths]
for slot in (0, 215):
    runtime_loaded[slot] = True
    runtime_ends[slot] = fixture['frames'] / fixture['rate']
    runtime_bindings[slot] = playback_audio.current_input_runtime_pad_binding(slot)
    assert runtime_bindings[slot] is not None
runtime_pitch_rows[0].update(source_root=0, source_mode='major', key_lock=key_lock)
playback_audio.publish_piano_runtime(runtime_loaded, runtime_starts,
    runtime_ends, runtime_bindings, pitch_rows=runtime_pitch_rows)
piano_timestamp = feedback_audio.capture_input_timestamp_ns()
transport.pitch.choose_piano_note(0, piano_note, retrigger=False,
    received_at_ns=piano_timestamp)
assert session.input_mapping_error is None, session.input_mapping_error
assert len(transport.pitch._owners) == 1, transport.pitch.status(0)
piano_owner = next(iter(transport.pitch._owners.values()))
piano_capture = piano_owner.capture
assert piano_owner.published and piano_owner.content_id == '9' * 32
assert piano_capture.base_shift == 12 and piano_capture.extra_shift == 0
assert not piano_capture.retrigger and piano_capture.received_at_ns == piano_timestamp
assert project.pad_key_intent[0].base_shift == 12
assert project.pad_key_lock[0] is key_lock
assert session.active_sample_ids == {215}
"#,
        );
        assert_eq!(
            h.callback.drain(&mut h.consumer),
            1,
            "original stopped piano SET"
        );
        {
            let callback = h.callback.as_mut();
            crate::audio_engine::audio_stream::render_scheduled_audio(
                &mut callback.mixer,
                &mut callback.scheduler,
                &mut [],
                &mut [0.0; NUM_SAMPLES],
                0,
                2,
                &mut callback.transport,
                &mut callback.feedback,
                &mut callback.retirement,
            );
        }
        assert_eq!(h.callback.transport.output_frame(), 0);
        assert!(h.callback.scheduler.is_empty());
        assert_eq!(h.callback.mixer.pad_pitch_intent(0), Some(pitch));
        assert_eq!(h.callback.mixer.key_lock_for_measurement(0), key_lock);
        assert!(
            h.callback
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.is_playing_sample(0))
        );
        controller_python(
            &locals,
            r#"
assert piano_capture.state == 'applied' and piano_capture.effective
assert piano_capture.due_frame == piano_capture.effective_output_frame == 0
transport.pitch.poll()
assert transport.pitch.status(0) == ('applied', None) and not transport.pitch._owners
assert session.active_sample_ids == {215}
assert transport_events == [('started', 215)]
"#,
        );
    }
    let launch_timeline = h.callback.transport;
    controller_python(
        &locals,
        r#"
registration = controller._committed_residency._selections[0].ticket
assert registration.publication_status() == 'accepted' and registration.is_current()
assert controller.stem_residency_status(0).effective is None
assert audio.worker_call_count() == 1 and audio.prepared_only_on_pool_threads()
transport.loop.set_full_track_region(0)
assert project.pad_loop_start_s[0] == 0. and project.pad_loop_end_s[0] == fixture['frames'] / fixture['rate']
assert project.pad_loop_auto[0] is False
birth = transport.residency._pending[0].ticket
assert birth is not None and 0 not in session.active_sample_ids
assert birth.publication_status() in {'preparing', 'pending'}
original_timestamp = feedback_audio.capture_input_timestamp_ns()
transport.playback.trigger_pad_keep_others(0, received_at_ns=original_timestamp)
ordinary_launch = next(iter(transport.residency._captured_starts.values()))
ordinary_birth = ordinary_launch.start
assert ordinary_launch.dependency is birth
assert ordinary_launch.published is False
assert ordinary_birth.original_frame() is None and ordinary_birth.effective_frame() is None
assert captured_start_calls == [(ordinary_birth, original_timestamp)]
"#,
    );
    let ordinary_birth = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("ordinary_birth")
            .unwrap()
            .unwrap()
            .extract::<PyRef<crate::audio_engine::pad_start::CapturedPadStart>>()
            .unwrap()
            .clone()
    });
    let original_timestamp = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("original_timestamp")
            .unwrap()
            .unwrap()
            .extract::<u64>()
            .unwrap()
    });
    assert_eq!(ordinary_birth.received_timestamp(), original_timestamp);
    eprintln!(
        "public seed before dependency ACK: raw_loop={:?} current_revision={:?} captured_revision={:?} guard={:?}",
        h.callback.mixer.loop_region_frames(0),
        h.callback
            .mixer
            .source_control_revision(ordinary_birth.content_key()),
        ordinary_birth.source_control_revision(),
        ordinary_birth.control_guard().map(|_| ()),
    );
    // CaptureTarget is already queued, so peek alone cannot prove the original
    // edit has published. Retain that exact ticket through its full transaction.
    wait_until(|| {
        Python::attach(|py| {
            locals
                .bind(py)
                .get_item("birth")
                .unwrap()
                .unwrap()
                .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
                .unwrap()
                .control_published_for_test()
        }) && h.producer.try_lock().is_ok()
    });
    assert!(
        h.callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.is_playing_sample(0))
    );
    assert_eq!(
        h.callback.drain(&mut h.consumer),
        2,
        "original public target and exact pending complete-window ACK"
    );
    assert_eq!(ordinary_birth.original_frame(), Some(0));
    assert_eq!(ordinary_birth.received_timestamp(), original_timestamp);
    eprintln!(
        "public seed after dependency ACK: raw_loop={:?} current_revision={:?} captured_revision={:?} guard={:?}",
        h.callback.mixer.loop_region_frames(0),
        h.callback
            .mixer
            .source_control_revision(ordinary_birth.content_key()),
        ordinary_birth.source_control_revision(),
        ordinary_birth.control_guard().map(|_| ()),
    );
    assert!(
        h.callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.is_playing_sample(0))
    );
    feedback(&locals, &mut h.callback);
    // An unrelated off-RT marker may still be closing after the native ACK.
    // Observe the exact ticket's current guards after that bounded settlement.
    wait_until(|| {
        Python::attach(|py| {
            let birth = locals.bind(py).get_item("birth").unwrap().unwrap();
            birth
                .call_method0("publication_status")
                .unwrap()
                .extract::<String>()
                .unwrap()
                == "accepted"
                && birth
                    .call_method0("is_current")
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
        })
    });
    controller_python(
        &locals,
        "assert birth.publication_status() == 'accepted' and birth.is_current()\nassert 0 not in session.active_sample_ids\nassert ordinary_launch.dependency is birth\nassert ordinary_launch.start is ordinary_birth\nassert captured_start_calls == [(ordinary_birth, original_timestamp)]\nassert all(start is ordinary_birth and dependency is birth for start, dependency in captured_dependency_calls)\nassert ordinary_birth.effective_frame() is None",
    );
    assert_extent(
        h.callback.mixer.bank_for_measurement()[0].as_ref().unwrap(),
        &full,
        key_lock,
        0,
        full.frame_count(),
    );
    wait_until(|| {
        h.callback.drain(&mut h.consumer);
        feedback(&locals, &mut h.callback);
        assert_ne!(
            ordinary_birth.state(),
            "rejected",
            "{:?}",
            ordinary_birth.reason()
        );
        assert_eq!(h.callback.transport.output_frame(), 0);
        if ordinary_birth.state() == "applied" {
            return true;
        }
        let runtime = h.callback.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        runtime.has_ready_action(ordinary_birth.sequence())
    });
    let execution = ordinary_birth.execution_frame().unwrap();
    assert_eq!(ordinary_birth.original_frame(), Some(0));
    assert_eq!(
        execution > 0,
        needs_native,
        "only the actual non-unity Native tuple needs preparation"
    );
    controller_python(
        &locals,
        "assert birth.publication_status() == 'accepted' and birth.is_current()\nassert ordinary_launch.dependency is birth\nassert ordinary_launch.start is ordinary_birth\nassert captured_start_calls == [(ordinary_birth, original_timestamp)]\nassert captured_dependency_calls and all(start is ordinary_birth and dependency is birth for start, dependency in captured_dependency_calls)\nassert ordinary_launch.ticket is not None\nassert ordinary_launch.ticket.publication_status() == 'accepted' and ordinary_launch.ticket.is_current()",
    );
    Python::attach(|py| {
        let member = locals
            .bind(py)
            .get_item("ordinary_launch")
            .unwrap()
            .unwrap()
            .getattr("ticket")
            .unwrap();
        let member = member
            .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
            .unwrap();
        assert!(
            member
                .capture_pad_start_launch_guard(&ordinary_birth)
                .unwrap()
                .is_some()
        );
    });
    assert_eq!(ordinary_birth.received_timestamp(), original_timestamp);
    while h.callback.transport.output_frame() < execution {
        let frame = h.callback.transport.output_frame();
        let frames = (execution - frame).min(257) as usize;
        let mut output = vec![0.0; frames * 2];
        let callback = h.callback.as_mut();
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        h.callback.transport.advance_by_rendered_frames(frames);
        assert!(
            output.iter().all(|sample| *sample == 0.0),
            "only the zero-volume peer may play before E"
        );
        assert!(
            h.callback
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.is_playing_sample(0))
        );
        assert_eq!(other(&h.callback.mixer).generation, other_generation);
        assert_eq!(
            other(&h.callback.mixer).stretch.native_state_address(),
            other_native
        );
        assert_eq!(
            other(&h.callback.mixer).stretch.pending_fifo_frames(),
            other_fifo
        );
    }
    {
        let callback = h.callback.as_mut();
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut [],
            &mut [0.0; NUM_SAMPLES],
            execution,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
    }
    assert_eq!(ordinary_birth.effective_frame(), Some(execution));
    assert!(h.callback.scheduler.is_empty());
    let original = ordinary_birth.original_frame().unwrap();
    assert!(execution >= original);
    if needs_native {
        assert_eq!(
            voice(&h.callback.mixer).stretch.adopted_request_id(),
            Some(ordinary_birth.sequence())
        );
        assert_eq!(
            voice(&h.callback.mixer)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            ControllerRawNative::constant_history_frames(
                pitch.native_pitch_scale(ratio, key_lock).unwrap()
            ) as u64
        );
    }
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("initial_prepare_frames", execution)
            .unwrap()
    });
    assert_eq!(voice(&h.callback.mixer).source_playback.position().frame, 0);
    assert_eq!(
        voice(&h.callback.mixer)
            .source_playback
            .position()
            .fraction
            .to_bits(),
        0.0_f64.to_bits()
    );
    assert_eq!(
        h.callback.mixer.loop_region_frames(0),
        (0, Some(full.frame_count()))
    );
    assert_eq!(h.callback.mixer.key_lock_for_measurement(0), key_lock);
    feedback(&locals, &mut h.callback);
    controller_python(
        &locals,
        "assert session.active_sample_ids == {0, 215}\nassert transport_events == [('started', 215), ('started', 0)]",
    );
    let generation = voice(&h.callback.mixer).generation;
    // A follow-up measuring reader retirement must not keep a test-owned pin.
    let source = followup
        .is_none()
        .then(|| h.callback.mixer.bank_for_measurement()[0].clone().unwrap());
    let launch = LoopPhaseReference::capture(
        Some(launch_timeline),
        ordinary_birth.original_frame().unwrap(),
    );
    assert!(
        voice(&h.callback.mixer)
            .source_playback
            .phase_reference_current(&launch_timeline)
    );
    let (reference, reference_owner) = reference_with_ownership(
        &full,
        &full_stems,
        ratio,
        key_lock,
        execution,
        pitch,
        (launch, &launch_timeline),
    );
    let (full_mix, full_mix_owner) = reference_with_ownership(
        &full,
        &full_stems,
        ratio,
        key_lock,
        execution,
        pitch,
        (launch, &launch_timeline),
    );
    let reference_owners = [reference_owner, full_mix_owner];
    let mut playback = ControllerPlayback {
        callback: h.callback,
        reference,
        full_mix,
        frame: execution,
        source_frame: 0,
        resumed_at: None,
        seek_epoch: None,
        complete_resident: true,
        source: full.clone(),
        components: full_stems.stems.clone(),
        ratio,
        target_bpm: 150.0,
        loop_period: full.frame_count() as f64,
        loop_start: 0,
        loop_end: full.frame_count(),
        rate_ledger: None,
        scheduled: false,
        selection: None,
        previous: None,
        ramp_start: 0,
        raw: needs_native.then(|| initial_raw_native(ratio, pitch, key_lock)),
        candidate: None,
        filter: controller_filter(),
    };
    let initial_native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    render(&mut playback, key_lock, 13);
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        initial_native
    );
    if needs_native {
        assert_eq!(
            voice(&playback.callback.mixer).stretch.adopted_request_id(),
            Some(ordinary_birth.sequence())
        );
    }
    if let Some((false, run)) = followup {
        run(
            &mut playback,
            &mut h.consumer,
            &locals,
            &engine,
            &reference_owners,
        );
        finish_history(&mut playback, &locals);
        drop(locals);
        std::env::set_current_dir(previous_cwd).unwrap();
        return;
    }
    controller_python(
        &locals,
        r#"
assert controller.set_stem_mix_mode(0, 'all_stems')
first = controller._committed_residency._selections[0].receipt
assert first is not None and first.effective_mode_status() == 'waiting'
assert project.pad_stem_mix_mode[0] == 'all_stems'
"#,
    );
    adopt(&mut playback, &mut h.consumer, &locals, "first", key_lock);
    let identity = Arc::downgrade(
        &playback.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity,
    );
    assert!(
        playback
            .reference
            .set_stem_enabled_mask(0, 0b1010, full_stems.source_version_hash)
    );
    assert!(playback.reference.set_stem_mix_mode(
        0,
        StemMixMode::AllStems,
        full_stems.source_version_hash
    ));
    playback.select(Some(0b1010));
    pause_ramp(&mut playback, &mut h.consumer, &locals, key_lock);
    settle(&mut playback, &locals, "first", key_lock);
    if let Some((true, run)) = followup {
        run(
            &mut playback,
            &mut h.consumer,
            &locals,
            &engine,
            &reference_owners,
        );
        finish_history(&mut playback, &locals);
        drop(locals);
        std::env::set_current_dir(previous_cwd).unwrap();
        return;
    }
    let old = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| Arc::downgrade(&stem.samples));
    controller_python(
        &locals,
        r#"
assert controller.stem_residency_status(0).effective == 'effective'
assert controller.set_stem_mix_mode(0, 'full_mix')
release = controller._committed_residency._selections[0].receipt
assert release is not first and release.effective_mode_status() == 'waiting'
assert project.pad_stem_mix_mode[0] == 'full_mix'
assert project.stem_cache[0].pair == disk_selection and registration.is_current()
"#,
    );
    adopt(&mut playback, &mut h.consumer, &locals, "release", key_lock);
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::FullMix, 0)
    );
    playback.select(None);
    finish_ramp(&mut playback, &locals, "release", key_lock);
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    if key_lock {
        assert!(
            old.iter().any(|reader| reader.strong_count() > 0),
            "actual ALL Native/history reader survives bank withdrawal"
        );
        render(&mut playback, true, 13);
        playback.adopt_native(false);
    } else {
        render(&mut playback, false, 137);
    }
    assert_eq!(
        controller_ticket_status(&locals, "release", "effective_mode_status"),
        "effective"
    );
    playback.callback.retirement.retired.clear();
    reconcile(&engine).unwrap();
    wait_until(|| old.iter().all(|reader| reader.strong_count() == 0));
    feedback(&locals, &mut playback.callback);
    controller_python(
        &locals,
        r#"
assert controller.stems_available(0) and project.stem_cache[0].pair == disk_selection
assert registration.is_current()
assert controller.set_stem_mix_mode(0, 'all_stems')
fresh = controller._committed_residency._selections[0].receipt
assert fresh is not first and fresh is not release
assert fresh.effective_mode_status() == 'waiting'
assert audio.worker_call_count() == 1 and audio.prepared_only_on_pool_threads()
"#,
    );
    adopt(&mut playback, &mut h.consumer, &locals, "fresh", key_lock);
    let current = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(identity.ptr_eq(&Arc::downgrade(&current.complete_set_identity)));
    for (old, stem) in old.iter().zip(&current.stems) {
        assert!(!old.ptr_eq(&Arc::downgrade(&stem.samples)));
        assert_eq!(
            (stem.resident_start(), stem.resident_end()),
            (0, full.frame_count())
        );
    }
    assert!(playback.reference.set_stem_mix_mode(
        0,
        StemMixMode::AllStems,
        full_stems.source_version_hash
    ));
    playback.select(Some(0b1010));
    settle(&mut playback, &locals, "fresh", key_lock);
    // Later blocks include both repeated complete-track wraps and the full 4096
    // warm-history length, without deriving expected position from the callback.
    let begin = playback.source_frame;
    let mut remaining = full.frame_count() * 3 + 4096;
    for partition in [1_usize, 31, 137, 512, 777].into_iter().cycle() {
        let frames = partition.min(remaining);
        render(&mut playback, key_lock, frames);
        remaining -= frames;
        if remaining == 0 {
            break;
        }
    }
    assert!((playback.source_frame - begin) as f64 * ratio >= full.frame_count() as f64 * 3.0);
    let fractional_wraps = (begin..playback.source_frame)
        .filter(|frame| {
            let phase = (*frame as f64 * ratio).rem_euclid(full.frame_count() as f64);
            phase.floor() as usize == full.frame_count() - 1 && phase.fract() > 0.0
        })
        .count();
    assert!(
        fractional_wraps >= 3,
        "independent complete end-to-start interpolation knots"
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    let source = source.unwrap();
    assert!(Arc::ptr_eq(
        &source.samples,
        &playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    assert_eq!(
        source.resident_binding(),
        playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .resident_binding()
    );
    assert_eq!(other(&playback.callback.mixer).generation, other_generation);
    assert_eq!(
        other(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        other_native
    );
    assert_eq!(
        other(&playback.callback.mixer)
            .stretch
            .pending_fifo_frames(),
        other_fifo
    );
    assert!(Arc::ptr_eq(
        &other(&playback.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .samples,
        &other_sample.samples
    ));
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(215),
        other_demand
    );
    // The peer starts at output zero and actually renders the preparation lead.
    // The primary source counter contains only its post-E active frames; the
    // shared pause advances neither source counter.
    let other_rendered_frames = execution + playback.source_frame;
    let phase = (other_rendered_frames as f64 * ratio).rem_euclid(full.frame_count() as f64);
    assert_eq!(
        other(&playback.callback.mixer)
            .source_playback
            .position()
            .frame,
        phase.floor() as usize
    );
    assert_eq!(
        other(&playback.callback.mixer)
            .source_playback
            .position()
            .fraction
            .to_bits(),
        phase.fract().to_bits()
    );
    feedback(&locals, &mut playback.callback);
    controller_python(
        &locals,
        r#"
assert session.active_sample_ids == {0, 215} and not session.paused_sample_ids
assert transport_events == [('started', 215), ('started', 0)]
assert controller.stem_residency_status(0).effective == 'effective'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    // A bounded public proper-finite excursion preserves the actual voice while
    // changing physical storage/context through each own ACK, then returns 0..N.
    while !(LOOP_START..LOOP_END).contains(
        &((playback.source_frame as f64 * ratio)
            .rem_euclid(full.frame_count() as f64)
            .floor() as usize),
    ) {
        render(&mut playback, key_lock, 1);
    }
    controller_python(&locals, "transport.loop.set_start(0, 17 / fixture['rate'])");
    public_window_ack(&mut playback, &mut h.consumer, &locals);
    assert_extent(
        playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap(),
        &full,
        key_lock,
        LOOP_START,
        full.frame_count(),
    );
    controller_python(&locals, "transport.loop.set_end(0, 1517 / fixture['rate'])");
    public_window_ack(&mut playback, &mut h.consumer, &locals);
    assert_extent(
        playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap(),
        &full,
        key_lock,
        LOOP_START,
        LOOP_END,
    );
    assert_eq!(
        playback.callback.mixer.loop_region_frames(0),
        (LOOP_START, Some(LOOP_END))
    );
    controller_python(&locals, "transport.loop.set_full_track_region(0)");
    public_window_ack(&mut playback, &mut h.consumer, &locals);
    assert_extent(
        playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap(),
        &full,
        key_lock,
        0,
        full.frame_count(),
    );
    assert_eq!(
        playback.callback.mixer.loop_region_frames(0),
        (0, Some(full.frame_count()))
    );
    controller_python(
        &locals,
        r#"
assert project.pad_loop_start_s[0] == 0. and project.pad_loop_end_s[0] == fixture['frames'] / fixture['rate']
assert session.active_sample_ids == {0, 215}
assert transport_events == [('started', 215), ('started', 0)]
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    finish_history(&mut playback, &locals);
    drop(locals);
    std::env::set_current_dir(previous_cwd).unwrap();
}

fn finish_history(playback: &mut ControllerPlayback, locals: &Py<PyDict>) {
    controller_python(
        locals,
        "controller.shut_down()\ncontroller._assets.release_saved_assignments()",
    );
    playback
        .callback
        .mixer
        .stop_sample_rt(0, &mut playback.callback.retirement);
    playback
        .callback
        .mixer
        .stop_sample_rt(215, &mut playback.callback.retirement);
    playback.callback.retirement.retired.clear();
}
