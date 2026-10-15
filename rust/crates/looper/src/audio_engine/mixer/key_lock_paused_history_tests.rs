// Included inside source_feed_history_tests to reuse only its independent scalar
// arithmetic/direct C ABI oracle and its public-action fixture setup.

struct PausedOracle {
    reference: DirectNative,
    backwards: DirectNative,
    reset_origin: usize,
    bound: usize,
    endpoint: usize,
    total: i32,
    lock: bool,
    rendered: usize,
    maximum: f32,
    wrong_maximum: f32,
}

impl PausedOracle {
    fn new(
        timeline: &[ScalarFrame],
        source: &SampleBuffer,
        endpoint: usize,
        total: i32,
        lock: bool,
    ) -> Self {
        let (reset_origin, bound) = bounded_origin(timeline, endpoint, total, lock);
        assert!(bound > 4_096 && reset_origin < endpoint);
        let first_pitch = timeline[reset_origin + BLOCK - 1].pitch(total, lock);
        let mut reference = DirectNative::new(first_pitch);
        let mut backwards = DirectNative::new(first_pitch);
        let end = timeline[endpoint];
        for (active, frame) in timeline
            .iter()
            .copied()
            .enumerate()
            .take(endpoint)
            .skip(reset_origin)
        {
            reference.sample(frame.pcm(&source.samples), frame.pitch(total, lock));
            let coordinate = (end.frame as f64 + end.fraction
                - START as f64
                - (endpoint - active) as f64 * end.rate)
                .rem_euclid((END - START) as f64);
            let wrong = ScalarFrame {
                frame: START + coordinate.floor() as usize,
                fraction: coordinate.fract(),
                passage: Passage::Normal,
                rate: end.rate,
            };
            backwards.sample(wrong.pcm(&source.samples), wrong.pitch(total, lock));
        }
        assert_eq!(
            reference.fifo(),
            (
                (endpoint - reset_origin) % BLOCK,
                BLOCK - 1 - (endpoint - reset_origin) % BLOCK
            )
        );
        assert_eq!(reference.fifo(), backwards.fifo());
        Self {
            reference,
            backwards,
            reset_origin,
            bound,
            endpoint,
            total,
            lock,
            rendered: 0,
            maximum: 0.0,
            wrong_maximum: 0.0,
        }
    }

    fn compare(
        &mut self,
        fixture: &mut Fixture,
        source: &SampleBuffer,
        timeline: &[ScalarFrame],
        frames: usize,
        label: &str,
    ) {
        let pcm = fixture.render(frames);
        for (offset, actual) in pcm.iter().enumerate() {
            let active = self.endpoint + self.rendered + offset;
            let frame = timeline[active];
            let expected = self.reference.sample(
                frame.pcm(&source.samples),
                frame.pitch(self.total, self.lock),
            );
            let wrong = self.backwards.sample(
                frame.pcm(&source.samples),
                frame.pitch(self.total, self.lock),
            );
            let error = (*actual - expected).abs();
            self.maximum = self.maximum.max(error);
            self.wrong_maximum = self.wrong_maximum.max((*actual - wrong).abs());
            assert!(
                error < 2.0e-6,
                "{label}: resumed_offset={} actual={actual} independent_A_reset={expected} error={error}",
                self.rendered + offset
            );
        }
        self.rendered += frames;
        assert_phase(fixture, timeline[self.endpoint + self.rendered]);
        assert_eq!(
            fixture.voice(0).stretch.pending_fifo_frames(),
            self.reference.fifo(),
            "{label}"
        );
    }
}

fn pause_public(fixture: &mut Fixture) {
    let output = fixture.transport.output_frame();
    let phase = fixture.voice(0).source_playback;
    fixture.control.engine.pause_sample(0).unwrap();
    fixture.drain();
    assert_eq!(fixture.transport.output_frame(), output);
    assert!(fixture.voice(0).paused);
    assert!(fixture.voice(0).source_playback.matches_exact(&phase));
}

fn resume_public(fixture: &mut Fixture) {
    let output = fixture.transport.output_frame();
    let phase = fixture.voice(0).source_playback;
    fixture.control.engine.resume_sample(0).unwrap();
    fixture.drain();
    assert_eq!(fixture.transport.output_frame(), output);
    assert!(!fixture.voice(0).paused);
    assert!(fixture.voice(0).source_playback.matches_exact(&phase));
}

fn active_history_clock(fixture: &Fixture, id: usize) -> (u64, u64) {
    let mut snapshot = crate::audio_engine::source_feed_history::SourceFeedSnapshot::new();
    assert!(
        fixture
            .voice(id)
            .source_feed_history
            .capture_into(&mut snapshot)
    );
    (snapshot.active_origin(), snapshot.active_frames())
}

/// Probe a clone with unequal gains so retained internal filter history is
/// observable even though the actual PCM oracle uses the neutral pad chain.
fn filter_history_probe(chain: &PerPadDspChain) -> Vec<f32> {
    let mut probe = chain.clone();
    let low = DspParameterId::per_pad(0, DspNodeSlot::Slot0, DspParameterSlot::Slot0).unwrap();
    assert!(probe.set_parameter(low, 0.0));
    (0..73)
        .map(|frame| {
            probe.begin_frame();
            probe.process_sample(0, if frame == 0 { 0.37 } else { 0.0 })
        })
        .collect()
}

/// A separate actual voice keeps rendering while pad 0 is paused. Its zero attack
/// velocity leaves the independently calculated mono PCM oracle unchanged.
fn start_silent_peer(fixture: &mut Fixture) {
    let captured_at = fixture.transport.output_frame();
    assert_eq!(fixture.mixer.pad_pitch_intent[1], PitchIntent::default());
    let start = fixture
        .control
        .engine
        .capture_pad_start(
            1,
            LOOP_START as f64 / f64::from(RATE),
            Some(LOOP_END as f64 / f64::from(RATE)),
            0.0,
            false,
            None,
            None,
        )
        .unwrap();
    fixture
        .control
        .engine
        .publish_pad_start(&start, None)
        .unwrap();
    fixture.drain();
    // Its actual tuple is untransposed at r=1, so this public ordinary start
    // must retain the current dueframe rather than skip the first rate event.
    assert_eq!(start.execution_frame(), Some(captured_at));
    assert_eq!(start.original_frame(), Some(captured_at));
    // PreparedStart drains its due unity action during control-message handling.
    // Its runtime slot is already retired after success, so has_ready_action()
    // can never become true for that completed original receipt.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert_eq!(fixture.transport.output_frame(), captured_at);
        assert_ne!(
            start.state(),
            "rejected",
            "silent peer start rejected at fixed output {captured_at}: sequence={} reason={:?} original={:?} E={:?}",
            start.sequence(),
            start.reason(),
            start.original_frame(),
            start.execution_frame()
        );
        if let Some(effective) = start.effective_frame() {
            assert_eq!(effective, captured_at);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "silent peer original start did not settle at fixed output {captured_at}: sequence={} state={} reason={:?} original={:?} E={:?} scheduler_target={:?}",
            start.sequence(),
            start.state(),
            start.reason(),
            start.original_frame(),
            start.execution_frame(),
            fixture.scheduler.peek_next_target_frame()
        );
        fixture.mixer.pitch_runtime.as_mut().unwrap().poll();
        fixture.drain();
        std::thread::sleep(Duration::from_millis(1));
    }
    fixture.render(1);
    assert_eq!(start.effective_frame(), Some(captured_at));
    assert!(!fixture.voice(1).paused);
    assert_eq!(fixture.voice(1).volume, 0.0);
    assert_eq!(fixture.transport.output_frame(), captured_at + 1);
}

fn render_frozen(fixture: &mut Fixture, frames: usize) {
    let phase = fixture.voice(0).source_playback;
    let fifo = fixture.voice(0).stretch.pending_fifo_frames();
    let history = active_history_clock(fixture, 0);
    let filter = filter_history_probe(&fixture.mixer.pad_dsp_chains[0]);
    let pcm = fixture.render(frames);
    assert!(
        pcm.iter().all(|sample| *sample == 0.0),
        "paused pad must emit silence"
    );
    assert!(fixture.voice(0).paused);
    assert!(fixture.voice(0).source_playback.matches_exact(&phase));
    assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), fifo);
    assert_eq!(active_history_clock(fixture, 0), history);
    assert_eq!(
        filter_history_probe(&fixture.mixer.pad_dsp_chains[0]),
        filter,
        "paused callbacks must not consume filter history"
    );
}

#[test]
fn public_paused_modes_replay_only_genuine_past_and_resume_exact_native_pcm_without_owner_replacement()
 {
    for passage in [Passage::Intro, Passage::Tail] {
        for pitch in [
            PitchIntent::new(6, 18).unwrap(),
            PitchIntent::new(-5, -18).unwrap(),
        ] {
            for old in [false, true] {
                let (mut fixture, source, output_origin) = setup(passage, old, pitch);
                start_silent_peer(&mut fixture);
                controls_to_capture(&mut fixture, output_origin);
                let timeline = chronology(passage, F + 4_097 + 8_193);
                assert_phase(&fixture, timeline[F]);
                let mut oracle =
                    PausedOracle::new(&timeline, &source, F, pitch.total_semitones(), !old);
                let crossing = timeline
                    .iter()
                    .position(|frame| frame.passage == Passage::Normal)
                    .unwrap();
                assert!(
                    oracle.reset_origin < crossing && crossing < F,
                    "{passage:?} crossing={crossing} must lie in genuine paused replay {}..{F}",
                    oracle.reset_origin
                );
                let generation = fixture.voice(0).generation;
                let address = fixture.voice(0).sample.as_ref().unwrap().source_address();
                let starts = fixture.starts(0);
                let old_native = fixture.voice(0).stretch.native_state_address();
                let peer_generation = fixture.voice(1).generation;
                let peer_starts = fixture.starts(1);
                let filter = filter_history_probe(&fixture.mixer.pad_dsp_chains[0]);
                let mut cold_filter = fixture.mixer.pad_dsp_chains[0].clone();
                cold_filter.reset();
                let cold_probe = filter_history_probe(&cold_filter);
                assert!(
                    filter
                        .iter()
                        .zip(cold_probe)
                        .any(|(warm, cold)| (*warm - cold).abs() > 1.0e-7),
                    "filter-history preservation must have a real signal state to protect"
                );
                pause_public(&mut fixture);
                let frozen = fixture.voice(0).source_playback;
                let frozen_history = active_history_clock(&fixture, 0);
                assert_eq!(frozen_history.1 - frozen_history.0, F as u64);
                let peer_clock = active_history_clock(&fixture, 1).1;
                let capture = fixture.transport.output_frame();
                wait_history_capacity(&mut fixture, "paused independent PCM capture");
                let request = submit_scalar(&mut fixture, !old);
                let execution = fixture.mixer.pending_key_lock_tuples[0]
                    .as_ref()
                    .unwrap()
                    .execution
                    .unwrap();
                assert_eq!(execution, capture + 4_096);
                assert_eq!(execution, output_origin + E as u64);
                wait_own_candidate(&mut fixture, request, "paused independent PCM preparation");
                let pending = fixture.mixer.pending_key_lock_tuples[0].as_ref().unwrap();
                assert_eq!(pending.request.pitch, pitch);
                let binding = pending.request.binding;
                let source_generation = pending.request.source_generation;
                while fixture.transport.output_frame() < execution {
                    let frames = (execution - fixture.transport.output_frame()).min(127) as usize;
                    render_frozen(&mut fixture, frames);
                    assert_eq!(fixture.mixer.pad_key_lock_enabled[0], old);
                    assert_eq!(fixture.voice(0).stretch.native_state_address(), old_native);
                    assert!(
                        fixture
                            .mixer
                            .voices
                            .iter_mut()
                            .find(|voice| voice.active && voice.sample_id == 0)
                            .unwrap()
                            .stretch
                            .prospective_work_ready(),
                        "paused callbacks must retain their already prepared candidate"
                    );
                    let pending = fixture.mixer.pending_key_lock_tuples[0].as_ref().unwrap();
                    assert_eq!(pending.execution, Some(execution));
                    assert_eq!(pending.request.request_id, request);
                    assert_eq!(pending.request.binding, binding);
                    assert_eq!(pending.request.source_generation, source_generation);
                }
                let silence = fixture.render(1);
                assert_eq!(silence, vec![0.0]);
                assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
                assert_eq!(fixture.mixer.pad_key_lock_enabled[0], !old);
                let adopted_native = fixture.voice(0).stretch.native_state_address();
                assert_ne!(adopted_native, old_native);
                assert!(fixture.voice(0).stretch.owns_pitch_contract(pitch, !old));
                assert!(fixture.voice(0).source_playback.matches_exact(&frozen));
                assert_eq!(active_history_clock(&fixture, 0), frozen_history);
                assert_eq!(
                    fixture.voice(0).stretch.pending_fifo_frames(),
                    oracle.reference.fifo()
                );
                assert!(!fixture.voice(0).stretch.wet_render_ready());
                assert!(!fixture.voice(0).stretch.wet_current_source_output_ready());
                assert_eq!(
                    filter_history_probe(&fixture.mixer.pad_dsp_chains[0]),
                    filter,
                    "paused tuple adoption must retain the actual filter history"
                );
                let status = fixture.mixer.pad_key_lock_status[0].unwrap();
                assert_eq!(status.request_id, request);
                assert_eq!(status.effective, !old);
                assert_eq!(status.state, if !old { "armed" } else { "dry" });
                assert!(status.ready);
                for frames in [73, 511, 127, 512, 379] {
                    render_frozen(&mut fixture, frames);
                    assert_eq!(
                        fixture.voice(0).stretch.native_state_address(),
                        adopted_native
                    );
                    assert_eq!(
                        fixture.voice(0).stretch.pending_fifo_frames(),
                        oracle.reference.fifo()
                    );
                    assert!(!fixture.voice(0).stretch.wet_current_source_output_ready());
                }
                let pause_frames = fixture.transport.output_frame() - capture;
                assert_eq!(
                    active_history_clock(&fixture, 1).1 - peer_clock,
                    pause_frames
                );
                assert_eq!(fixture.voice(1).generation, peer_generation);
                assert_eq!(fixture.starts(1), peer_starts);
                assert!(!fixture.voice(1).paused);
                resume_public(&mut fixture);
                assert_eq!(
                    filter_history_probe(&fixture.mixer.pad_dsp_chains[0]),
                    filter,
                    "public Resume must retain the actual filter history"
                );
                assert_eq!(
                    fixture.voice(0).stretch.native_state_address(),
                    adopted_native
                );
                assert_eq!(
                    fixture.voice(0).stretch.pending_fifo_frames(),
                    oracle.reference.fifo()
                );
                let label = format!(
                    "paused {passage:?} k={} old_lock={old}",
                    pitch.total_semitones()
                );
                for frames in PARTITIONS {
                    oracle.compare(&mut fixture, &source, &timeline, frames, &label);
                    assert_eq!(
                        fixture.voice(0).stretch.native_state_address(),
                        adopted_native,
                        "Resume must keep the claimed Native/FIFO owner"
                    );
                    assert_eq!(fixture.voice(0).generation, generation);
                    assert_eq!(
                        fixture.voice(0).sample.as_ref().unwrap().source_address(),
                        address
                    );
                    assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
                    assert!(
                        !fixture.voice(0).stretch.source_preparation_pending(),
                        "Resume must not queue an automatic replacement for the claimed paused tuple"
                    );
                    if oracle.rendered == 1 {
                        assert!(fixture.voice(0).stretch.wet_current_source_output_ready());
                        let status = fixture.mixer.pad_key_lock_status[0].unwrap();
                        assert_eq!(status.request_id, request);
                        assert_eq!(status.state, if !old { "wet" } else { "dry" });
                        assert!(status.ready);
                    }
                }
                assert_eq!(oracle.rendered, 4_097);
                for frames in std::iter::repeat_n(512, 16).chain(std::iter::once(1)) {
                    oracle.compare(&mut fixture, &source, &timeline, frames, &label);
                    assert_eq!(
                        fixture.voice(0).stretch.native_state_address(),
                        adopted_native,
                        "a later automatic request replaced the adopted paused tuple"
                    );
                    assert!(!fixture.voice(0).stretch.source_preparation_pending());
                }
                assert_eq!(oracle.rendered, 4_097 + 8_193);
                assert!(
                    oracle.wrong_maximum > 0.005,
                    "paused backwards-constant history must disagree: {label} error={}",
                    oracle.wrong_maximum
                );
                assert_eq!(fixture.starts(0), starts);
                assert_eq!(fixture.voice(0).generation, generation);
                assert_eq!(
                    fixture.voice(0).sample.as_ref().unwrap().source_address(),
                    address
                );
                assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
                assert_eq!(
                    fixture
                        .voice(0)
                        .stretch
                        .productive_history()
                        .unwrap()
                        .fed_output_frames,
                    (F - oracle.reset_origin + oracle.rendered) as u64
                );
                println!(
                    "paused_mode_A_reset passage={passage:?} k={} old_lock={old} output_O={output_origin} relative_A={} active_F={F} original_global_E={execution} H={} resumed_samples={} every_sample_max_abs={} backwards_constant_max_abs={}; continuous_O_and_acoustic_acceptance_are_separate",
                    pitch.total_semitones(),
                    oracle.reset_origin,
                    oracle.bound,
                    oracle.rendered,
                    oracle.maximum,
                    oracle.wrong_maximum
                );
            }
        }
    }
}

#[test]
fn public_paused_mode_resume_restore_supersession_and_stop_fence_original_request_then_allow_fresh_work()
 {
    for race in ["resume_restore", "supersession", "stop"] {
        let pitch = PitchIntent::new(6, 18).unwrap();
        let (mut fixture, source, output_origin) = setup(Passage::Intro, false, pitch);
        controls_to_capture(&mut fixture, output_origin);
        let timeline = chronology(Passage::Intro, F + 4_097);
        let old_native = fixture.voice(0).stretch.native_state_address();
        let generation = fixture.voice(0).generation;
        let starts = fixture.starts(0);
        pause_public(&mut fixture);
        let frozen = fixture.voice(0).source_playback;
        let history = active_history_clock(&fixture, 0);
        wait_history_capacity(&mut fixture, "paused race capture");
        let original = submit_scalar(&mut fixture, true);
        let execution = fixture.mixer.pending_key_lock_tuples[0]
            .as_ref()
            .unwrap()
            .execution
            .unwrap();
        assert_eq!(execution, output_origin + E as u64);
        wait_own_candidate(&mut fixture, original, "paused race original preparation");
        let superseding = match race {
            "resume_restore" => {
                resume_public(&mut fixture);
                pause_public(&mut fixture);
                None
            }
            "supersession" => {
                let newer = fixture.control.engine.set_pad_key_lock(0, false).unwrap();
                assert!(newer > original);
                fixture.drain();
                Some(newer)
            }
            "stop" => {
                fixture.control.engine.stop_sample(0).unwrap();
                fixture.drain();
                None
            }
            _ => unreachable!(),
        };
        fixture.render_to(execution + 1);
        assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
        assert!(!fixture.mixer.pad_key_lock_enabled[0]);
        assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
        let status = fixture.mixer.pad_key_lock_status[0].unwrap();
        assert_eq!(status.request_id, superseding.unwrap_or(original));
        assert!(!status.effective);
        if superseding.is_none() {
            assert_eq!(
                status.state, "error",
                "{race}: original paused request must refuse"
            );
            assert!(!status.ready);
        }
        assert_eq!(fixture.starts(0), starts);
        if race == "stop" {
            assert!(
                !fixture
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.active && voice.sample_id == 0)
            );
            continue;
        }
        assert!(fixture.voice(0).paused);
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.voice(0).stretch.native_state_address(), old_native);
        assert!(fixture.voice(0).source_playback.matches_exact(&frozen));
        assert_eq!(active_history_clock(&fixture, 0), history);
        wait_history_capacity(&mut fixture, "fresh paused request after race");
        let fresh_f = fixture.transport.output_frame();
        let fresh = submit_scalar(&mut fixture, true);
        assert!(fresh > superseding.unwrap_or(original));
        let fresh_e = fixture.mixer.pending_key_lock_tuples[0]
            .as_ref()
            .unwrap()
            .execution
            .unwrap();
        assert_eq!(fresh_e, fresh_f + 4_096);
        assert!(fresh_e > execution);
        wait_own_candidate(&mut fixture, fresh, "fresh paused preparation after race");
        fixture.render_to(fresh_e);
        fixture.render(1);
        assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
        assert!(fixture.mixer.pad_key_lock_enabled[0]);
        let adopted = fixture.voice(0).stretch.native_state_address();
        assert_ne!(adopted, old_native);
        assert!(fixture.voice(0).source_playback.matches_exact(&frozen));
        assert_eq!(active_history_clock(&fixture, 0), history);
        let status = fixture.mixer.pad_key_lock_status[0].unwrap();
        assert_eq!(status.request_id, fresh);
        assert_eq!(status.state, "armed");
        assert!(status.ready && status.effective);
        let mut oracle = PausedOracle::new(&timeline, &source, F, pitch.total_semitones(), true);
        assert_eq!(
            fixture.voice(0).stretch.pending_fifo_frames(),
            oracle.reference.fifo()
        );
        resume_public(&mut fixture);
        for frames in PARTITIONS {
            oracle.compare(&mut fixture, &source, &timeline, frames, race);
            assert_eq!(fixture.voice(0).stretch.native_state_address(), adopted);
        }
        assert_eq!(oracle.rendered, 4_097);
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.starts(0), starts);
        assert_eq!(
            fixture.mixer.pad_key_lock_status[0].unwrap().request_id,
            fresh
        );
    }
}
