// These public mode tests certify the declared finite A-reset replay contract only.
// Continuous-from-O Native and device/acoustic acceptance remain separate gates.
mod source_feed_history_tests {
    use super::*;
    use crate::audio_engine::input_mapping::PitchPadProjection;
    use crate::audio_engine::rubberband_backend::RubberBandLiveStateOpaque;
    use crate::audio_engine::source_reader::ExplicitSeekMode;
    use std::collections::BTreeMap;

    const START: usize = 64_000;
    const END: usize = 160_000;
    const EXTENT: usize = 240_000;
    const F: usize = 68_904;
    const E: usize = F + 4_096;
    const BLOCK: usize = 512;
    const PARTITIONS: [usize; 11] = [1, 73, 511, 127, 512, 379, 512, 512, 512, 512, 446];
    const EVENTS: [(usize, f64); 4] = [
        (1_237, 0.73),
        (17_999, 1.41),
        (47_711, 0.62),
        (68_411, 1.29),
    ];

    // Direct C ABI intentionally bypasses production source readers, playback clocks,
    // Native wrappers, preparation helpers and source-history snapshots in the oracle.
    unsafe extern "C" {
        #[link_name = "rubberband_live_new"]
        fn mode_raw_new(rate: u32, channels: u32, options: i32) -> *mut RubberBandLiveStateOpaque;
        #[link_name = "rubberband_live_delete"]
        fn mode_raw_delete(state: *mut RubberBandLiveStateOpaque);
        #[link_name = "rubberband_live_reset"]
        fn mode_raw_reset(state: *mut RubberBandLiveStateOpaque);
        #[link_name = "rubberband_live_set_pitch_scale"]
        fn mode_raw_pitch(state: *mut RubberBandLiveStateOpaque, pitch: f64);
        #[link_name = "rubberband_live_get_start_delay"]
        fn mode_raw_delay(state: *mut RubberBandLiveStateOpaque) -> u32;
        #[link_name = "rubberband_live_get_block_size"]
        fn mode_raw_block(state: *mut RubberBandLiveStateOpaque) -> u32;
        #[link_name = "rubberband_live_shift"]
        fn mode_raw_shift(
            state: *mut RubberBandLiveStateOpaque,
            input: *const *const f32,
            output: *const *mut f32,
        );
    }

    struct DirectNative {
        state: *mut RubberBandLiveStateOpaque,
        pitch: f64,
        input: VecDeque<f32>,
        output: VecDeque<f32>,
    }

    impl DirectNative {
        fn new(first_completed_block_pitch: f64) -> Self {
            // The pinned production profile is Short window, channels together, mono.
            let state = unsafe { mode_raw_new(RATE, 1, 0x1000_0000) };
            assert!(!state.is_null());
            assert_eq!(unsafe { mode_raw_block(state) } as usize, BLOCK);
            let mut native = Self {
                state,
                pitch: first_completed_block_pitch,
                input: VecDeque::new(),
                output: std::iter::repeat_n(0.0, BLOCK - 1).collect(),
            };
            unsafe {
                mode_raw_pitch(
                    state,
                    if first_completed_block_pitch > 2.0 {
                        1.0
                    } else {
                        first_completed_block_pitch
                    },
                );
                mode_raw_reset(state);
            }
            if first_completed_block_pitch > 2.0 {
                let _ = native.shift(&[0.0; BLOCK]);
                unsafe { mode_raw_pitch(state, first_completed_block_pitch) };
            }
            native
        }

        fn shift(&mut self, input: &[f32; BLOCK]) -> [f32; BLOCK] {
            let mut output = [0.0; BLOCK];
            unsafe { mode_raw_shift(self.state, &input.as_ptr(), &output.as_mut_ptr()) };
            output
        }

        fn history_bound(pitch: f64) -> usize {
            let native = Self::new(pitch);
            if (0.5..=2.0).contains(&pitch) {
                4_096
            } else {
                // Query the configured reset profile itself, independently of H helpers.
                let delay = unsafe { mode_raw_delay(native.state) } as usize;
                ((2048.0 * pitch.max(1.0 / pitch)).ceil() as usize + delay + 2 * BLOCK)
                    .max(4_096)
                    .div_ceil(BLOCK)
                    * BLOCK
            }
        }

        fn sample(&mut self, pcm: f32, pitch: f64) -> f32 {
            if pitch != self.pitch {
                unsafe { mode_raw_pitch(self.state, pitch) };
                self.pitch = pitch;
            }
            self.input.push_back(pcm);
            if self.input.len() == BLOCK {
                let input = std::array::from_fn(|_| self.input.pop_front().unwrap());
                let output = self.shift(&input);
                self.output.extend(output);
            }
            self.output.pop_front().unwrap()
        }

        fn fifo(&self) -> (usize, usize) {
            (self.input.len(), self.output.len())
        }
    }

    impl Drop for DirectNative {
        fn drop(&mut self) {
            unsafe { mode_raw_delete(self.state) };
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Passage {
        Intro,
        Tail,
        Normal,
    }

    #[derive(Clone, Copy)]
    struct ScalarFrame {
        frame: usize,
        fraction: f64,
        passage: Passage,
        rate: f64,
    }

    impl ScalarFrame {
        fn pitch(self, total: i32, lock: bool) -> f64 {
            2.0_f64.powf(f64::from(total) / 12.0) / if lock { self.rate } else { 1.0 }
        }

        fn pcm(self, source: &[f32]) -> f32 {
            let right = match self.passage {
                Passage::Normal if self.frame + 1 == END => START,
                Passage::Tail if self.frame + 1 == EXTENT => START,
                _ => self.frame + 1,
            };
            source[self.frame] + (source[right] - source[self.frame]) * self.fraction as f32
        }
    }

    /// Independent binary64 arithmetic from the controls. Rebase only at actual
    /// smoothing steps; an off-grid target edit starts a new 512-active-frame step.
    fn chronology(passage: Passage, frames: usize) -> Vec<ScalarFrame> {
        let mut origin_frame = if passage == Passage::Intro {
            0
        } else {
            EXTENT - START
        };
        let mut origin_fraction = 0.0;
        let mut origin_mode = passage;
        let mut epoch = 0;
        let mut rate: f64 = 1.0;
        let mut target: f64 = 1.0;
        let mut next_step = usize::MAX;
        let mut result = Vec::with_capacity(frames + 1);
        for active in 0..=frames {
            if let Some((_, value)) = EVENTS.iter().find(|(at, _)| *at == active) {
                target = *value;
                next_step = active;
            }
            let position = |rate: f64| {
                let distance = origin_fraction + (active - epoch) as f64 * rate;
                let whole = distance.floor() as usize;
                let physical = origin_frame + whole;
                let boundary = match origin_mode {
                    Passage::Intro => START,
                    Passage::Tail => EXTENT,
                    Passage::Normal => END,
                };
                if physical < boundary {
                    (physical, distance - whole as f64, origin_mode)
                } else {
                    (
                        START + (physical - boundary) % (END - START),
                        distance - whole as f64,
                        Passage::Normal,
                    )
                }
            };
            if active == next_step && rate != target {
                (origin_frame, origin_fraction, origin_mode) = position(rate);
                epoch = active;
                rate = if (target - rate).abs() <= 0.05 {
                    target
                } else {
                    rate + (target - rate).clamp(-0.05, 0.05)
                };
                next_step = active + BLOCK;
            }
            let distance = origin_fraction + (active - epoch) as f64 * rate;
            let whole = distance.floor() as usize;
            let physical = origin_frame + whole;
            let boundary = match origin_mode {
                Passage::Intro => START,
                Passage::Tail => EXTENT,
                Passage::Normal => END,
            };
            let (frame, passage) = if physical < boundary {
                (physical, origin_mode)
            } else {
                (
                    START + (physical - boundary) % (END - START),
                    Passage::Normal,
                )
            };
            result.push(ScalarFrame {
                frame,
                fraction: distance - whole as f64,
                passage,
                rate,
            });
        }
        result
    }

    fn bounded_origin(timeline: &[ScalarFrame], end: usize, total: i32, lock: bool) -> (usize, usize) {
        let mut queried = BTreeMap::new();
        let mut bound = DirectNative::history_bound(timeline[end].pitch(total, lock));
        loop {
            let origin = (end - bound) / BLOCK * BLOCK;
            let mut required = bound;
            for frame in &timeline[origin..end] {
                let pitch = frame.pitch(total, lock);
                let history = *queried
                    .entry(pitch.to_bits())
                    .or_insert_with(|| DirectNative::history_bound(pitch));
                required = required.max(history);
            }
            if required == bound {
                return (origin, bound);
            }
            bound = required;
        }
    }

    fn setup(passage: Passage, old: bool, pitch: PitchIntent) -> (Fixture, SampleBuffer, u64) {
        let mut fixture = Fixture::new(old);
        let source = SampleBuffer {
            channels: 1,
            residency: None,
            samples: (0..EXTENT)
                .map(|frame| {
                    let time = frame as f64 / f64::from(RATE);
                    (0.18 * (std::f64::consts::TAU * (149.0 * time + 7.0 * time * time)).sin()
                        + 0.05 * (std::f64::consts::TAU * 431.0 * time).cos()) as f32
                })
                .collect::<Vec<_>>()
                .into(),
        }
        .with_complete_source(RATE);
        publish_tone(&fixture.control, 0, source.clone(), true);
        fixture.mixer.load_sample(0, source.clone());
        fixture.mixer.set_pad_loop_region(
            0,
            START as f64 / f64::from(RATE),
            Some(END as f64 / f64::from(RATE)),
        );
        fixture.mixer.set_pad_bpm(0, Some(120.0));
        // Publish the actual custom loop and current source binding before the
        // public action captures its requested full numeric tuple.
        let binding = crate::audio_engine::input_runtime_binding::capture(&fixture.control.engine, 0)
            .unwrap()
            .unwrap();
        let mut loaded = vec![false; NUM_SAMPLES];
        let mut starts = vec![0.0; NUM_SAMPLES];
        let mut ends = vec![None; NUM_SAMPLES];
        let mut bindings = vec![None; NUM_SAMPLES];
        let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
        loaded[0] = true;
        starts[0] = START as f64 / f64::from(RATE);
        ends[0] = Some(END as f64 / f64::from(RATE));
        bindings[0] = Some(binding);
        rows[0].intent = pitch;
        rows[0].key_lock = old;
        fixture
            .control
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_pitch(
                true,
                loaded,
                starts,
                ends,
                bindings.iter().map(Option::as_ref).collect(),
                rows,
            )
            .unwrap();
        let start = fixture.capture(0, &format!("pitch.extra:{}:set_retrigger", pitch.extra_shift()));
        fixture.publish(&start);
        fixture.wait_worker(&[&start]);
        fixture.render_to(start.execution_target());
        fixture.render(1);
        assert!(start.effective(), "{} {:?}", start.state(), start.reason());
        assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
        assert_eq!(fixture.mixer.pad_key_lock_enabled[0], old);
        assert!(fixture.voice(0).stretch.owns_pitch_contract(pitch, old));
        let output_origin = fixture.transport.output_frame();
        let source_start = if passage == Passage::Intro {
            0
        } else {
            EXTENT - START
        };
        wait(|| {
            fixture.mixer.seek_sample_at_output_frame(
                0,
                source_start as f64 / f64::from(RATE),
                output_origin,
            )
        });
        (fixture, source, output_origin)
    }

    fn controls_to_capture(fixture: &mut Fixture, origin: u64) {
        for (event, rate) in EVENTS.into_iter().take(3) {
            fixture.render_to(origin + event as u64);
            if event == EVENTS[0].0 {
                fixture.mixer.set_speed(rate);
            } else {
                if event == EVENTS[1].0 {
                    fixture.mixer.set_bpm_lock(true);
                }
                fixture.mixer.set_master_period(0.5 / rate);
            }
        }
        // Seek deliberately reset Native ownership without erasing actual future
        // source reads. A real nonstarting same-numeric SET resynchronizes that
        // productive owner after the genuine Intro/Tail crossing. It neither
        // retriggers nor rebases the source journal's O.
        fixture.render_to(origin + 60_000);
        let pitch = fixture.mixer.pad_pitch_intent[0];
        let old = fixture.mixer.pad_key_lock_enabled[0];
        let already_owned = fixture.voice(0).stretch.owns_pitch_contract(pitch, old);
        let generation = fixture.voice(0).generation;
        let starts = fixture.starts(0);
        fixture.rows(PitchPadProjection {
            intent: pitch,
            key_lock: old,
            ..PitchPadProjection::default()
        });
        let resync = fixture.capture(0, &format!("pitch.extra:{}:set", pitch.extra_shift()));
        fixture.publish(&resync);
        assert_eq!(resync.original_due_frame(), origin + 60_000);
        assert_eq!(
            resync.execution_target(),
            origin + if already_owned { 60_000 } else { 68_192 }
        );
        fixture.wait_worker(&[&resync]);
        fixture.render_to(resync.execution_target());
        fixture.render(1);
        assert!(resync.effective(), "{} {:?}", resync.state(), resync.reason());
        assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
        assert_eq!(fixture.mixer.pad_key_lock_enabled[0], old);
        assert!(fixture.voice(0).stretch.owns_pitch_contract(pitch, old));
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.starts(0), starts);
        fixture.render_to(origin + EVENTS[3].0 as u64);
        fixture.mixer.set_master_period(0.5 / EVENTS[3].1);
        fixture.render_to(origin + F as u64);
        assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
        assert_eq!(fixture.mixer.pad_key_lock_enabled[0], old);
        assert!(fixture.voice(0).stretch.owns_pitch_contract(pitch, old));
        assert!(fixture.voice(0).stretch.productive_history().is_some());
    }

    fn assert_phase(fixture: &Fixture, expected: ScalarFrame) {
        let actual = fixture.voice(0).source_playback.position();
        assert_eq!(actual.frame, expected.frame);
        assert_eq!(actual.fraction.to_bits(), expected.fraction.to_bits());
        assert_eq!(actual.seek_mode, ExplicitSeekMode::Normal);
    }

    fn advance_old_to(
        fixture: &mut Fixture,
        target: u64,
        origin: u64,
        timeline: &[ScalarFrame],
        old: bool,
        native: usize,
    ) {
        while fixture.transport.output_frame() < target {
            let frames = (target - fixture.transport.output_frame()).min(379) as usize;
            let pcm = fixture.render(frames);
            assert!(pcm.iter().all(|sample| sample.is_finite()));
            assert_eq!(fixture.mixer.pad_key_lock_enabled[0], old);
            assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
            let active = (fixture.transport.output_frame() - origin) as usize;
            assert_phase(fixture, timeline[active]);
        }
    }

    /// Give an earlier automatic request bounded host time to vacate the existing
    /// lane before creating this test's original mode receipt. Audio time stays fixed.
    fn wait_history_capacity(fixture: &mut Fixture, stage: &str) {
        let fixed_frame = fixture.transport.output_frame();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let voice = fixture.mixer.voices.iter_mut()
                .find(|voice| voice.active && voice.sample_id == 0).unwrap();
            let available = voice.stretch.source_history_work_available();
            let finished = voice.stretch.prospective_work_finished();
            let failed = voice.stretch.wet_mode_failed();
            let epoch = voice.stretch.preparation_epoch();
            assert_eq!(fixture.transport.output_frame(), fixed_frame);
            assert!(!failed, "{stage}: preparation worker failed at fixed output {fixed_frame}");
            if available {
                break;
            }
            assert!(Instant::now() < deadline,
                "{stage}: no source-history capacity at fixed output {fixed_frame}; status={:?} pending={:?} work_finished={finished} preparation_epoch={epoch}",
                fixture.mixer.pad_key_lock_status[0].map(|status| (status.request_id, status.state, status.effective, status.ready, status.error)),
                fixture.mixer.pending_key_lock_tuples[0].as_ref().map(|pending| (pending.request.request_id, pending.execution)));
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn wait_own_candidate(fixture: &mut Fixture, request: u64, stage: &str) {
        let fixed_frame = fixture.transport.output_frame();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let pending = fixture.mixer.pending_key_lock_tuples[0].as_ref();
            assert_eq!(pending.map(|pending| pending.request.request_id), Some(request),
                "{stage}: original mode request lost before preparation at fixed output {fixed_frame}; status={:?}",
                fixture.mixer.pad_key_lock_status[0].map(|status| (status.request_id, status.state, status.effective, status.ready, status.error)));
            let execution = pending.unwrap().execution;
            let voice = fixture.mixer.voices.iter_mut()
                .find(|voice| voice.active && voice.sample_id == 0).unwrap();
            let ready = voice.stretch.prospective_work_ready();
            let finished = voice.stretch.prospective_work_finished();
            let capacity = voice.stretch.source_history_work_available();
            let epoch = voice.stretch.preparation_epoch();
            let failed = voice.stretch.wet_mode_failed();
            assert_eq!(fixture.transport.output_frame(), fixed_frame);
            assert!(!failed, "{stage}: worker failed for original request {request}");
            if ready {
                break;
            }
            assert!(Instant::now() < deadline,
                "{stage}: original request {request} has no candidate at fixed output {fixed_frame}; E={execution:?} status={:?} work_finished={finished} source_history_work_available={capacity} preparation_epoch={epoch}",
                fixture.mixer.pad_key_lock_status[0].map(|status| (status.request_id, status.state, status.effective, status.ready, status.error)));
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn public_mode_requests_replay_genuine_variable_intro_and_tail_history_at_original_e() {
        for passage in [Passage::Intro, Passage::Tail] {
            for pitch in [PitchIntent::new(6, 18).unwrap(), PitchIntent::new(-5, -18).unwrap()] {
                for old in [false, true] {
                    let (mut fixture, source, output_origin) = setup(passage, old, pitch);
                    controls_to_capture(&mut fixture, output_origin);
                    let timeline = chronology(passage, E + 4_097);
                    assert_phase(&fixture, timeline[F]);
                    let total = pitch.total_semitones();
                    let generation = fixture.voice(0).generation;
                    let address = fixture.voice(0).sample.as_ref().unwrap().source_address();
                    let native = fixture.voice(0).stretch.native_state_address();
                    let starts = fixture.starts(0);
                    assert!(fixture.voice(0).stretch.owns_pitch_contract(pitch, old));
                    let (reset_origin, bound) = bounded_origin(&timeline, E, total, !old);
                    assert!(bound > E - F && reset_origin < F);
                    let crossing = timeline.iter().position(|frame| frame.passage == Passage::Normal).unwrap();
                    assert!(reset_origin < crossing && crossing < F,
                        "{passage:?} crossing={crossing} must be in required pre-F history {reset_origin}..{F}, H={bound}");
                    let first_pitch = timeline[reset_origin + BLOCK - 1].pitch(total, !old);
                    let mut reference = DirectNative::new(first_pitch);
                    let mut backwards = DirectNative::new(first_pitch);
                    let endpoint = timeline[E];
                    for (active, frame) in timeline.iter().copied().enumerate().take(E).skip(reset_origin) {
                        reference.sample(frame.pcm(&source.samples), frame.pitch(total, !old));
                        let coordinate = (endpoint.frame as f64 + endpoint.fraction
                            - START as f64 - (E - active) as f64 * endpoint.rate)
                            .rem_euclid((END - START) as f64);
                        let wrong = ScalarFrame {
                            frame: START + coordinate.floor() as usize,
                            fraction: coordinate.fract(),
                            passage: Passage::Normal,
                            rate: endpoint.rate,
                        };
                        backwards.sample(wrong.pcm(&source.samples), wrong.pitch(total, !old));
                    }
                    assert_eq!(reference.fifo(), ((E - reset_origin) % BLOCK, BLOCK - 1 - (E - reset_origin) % BLOCK));
                    assert_eq!(reference.fifo(), backwards.fifo());
                    wait_history_capacity(&mut fixture, "independent PCM mode capture");
                    let request = submit_scalar(&mut fixture, !old);
                    assert_eq!(fixture.mixer.pad_key_lock_enabled[0], old);
                    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
                    assert_phase(&fixture, timeline[F + 1]);
                    let pending = fixture.mixer.pending_key_lock_tuples[0].as_ref().unwrap();
                    assert_eq!(pending.request.request_id, request);
                    assert_eq!(pending.request.pitch, pitch);
                    assert_eq!(pending.execution, Some(output_origin + E as u64));
                    let content = pending.request.binding;
                    let source_generation = pending.request.source_generation;
                    wait_own_candidate(&mut fixture, request, "independent PCM mode preparation");
                    advance_old_to(&mut fixture, output_origin + E as u64, output_origin, &timeline, old, native);
                    let pending = fixture.mixer.pending_key_lock_tuples[0].as_ref().unwrap();
                    assert_eq!(pending.request.binding, content);
                    assert_eq!(pending.request.source_generation, source_generation);
                    assert_eq!(pending.execution, Some(output_origin + E as u64));
                    let mut maximum = 0.0_f32;
                    let mut wrong_maximum = 0.0_f32;
                    for frames in PARTITIONS {
                        let active = (fixture.transport.output_frame() - output_origin) as usize;
                        let pcm = fixture.render(frames);
                        assert_eq!(fixture.mixer.pad_key_lock_enabled[0], !old);
                        for (offset, actual) in pcm.iter().enumerate() {
                            let frame = timeline[active + offset];
                            let expected = reference.sample(frame.pcm(&source.samples), frame.pitch(total, !old));
                            let wrong = backwards.sample(frame.pcm(&source.samples), frame.pitch(total, !old));
                            let error = (*actual - expected).abs();
                            maximum = maximum.max(error);
                            wrong_maximum = wrong_maximum.max((*actual - wrong).abs());
                            assert!(error < 2.0e-6,
                                "{passage:?} k={total} old_lock={old} adopted_offset={} actual={actual} independent_A_reset={expected} error={error}", active + offset - E);
                        }
                        assert_phase(&fixture, timeline[active + frames]);
                        assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), reference.fifo());
                        assert_eq!(fixture.voice(0).generation, generation);
                        assert_eq!(fixture.voice(0).sample.as_ref().unwrap().source_address(), address);
                        assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
                    }
                    assert_eq!(PARTITIONS.into_iter().sum::<usize>(), 4_097);
                    assert!(wrong_maximum > 0.005,
                        "backwards-constant history must disagree: {passage:?} k={total} old_lock={old} error={wrong_maximum}");
                    assert_ne!(fixture.voice(0).stretch.native_state_address(), native);
                    assert_eq!(fixture.starts(0), starts);
                    assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
                    let status = fixture.mixer.pad_key_lock_status[0].unwrap();
                    assert_eq!(status.request_id, request);
                    assert_eq!(status.effective, !old);
                    assert!(status.ready);
                    assert_eq!(fixture.voice(0).stretch.productive_history().unwrap().fed_output_frames,
                        (E - reset_origin + 4_097) as u64);
                    println!("prospective_mode_A_reset passage={passage:?} k={total} old_lock={old} output_O={output_origin} relative_A={reset_origin} relative_F={F} relative_E={E} H={bound} every_sample_max_abs={maximum} backwards_constant_max_abs={wrong_maximum}; continuous_O_and_acoustic_acceptance_are_separate");
                }
            }
        }
    }

    #[test]
    fn restored_source_control_refuses_only_original_mode_then_same_lane_adopts_fresh_request() {
        let pitch = PitchIntent::new(6, 18).unwrap();
        let (mut fixture, _source, output_origin) = setup(Passage::Intro, false, pitch);
        controls_to_capture(&mut fixture, output_origin);
        let timeline = chronology(Passage::Intro, E + 16_384);
        let native = fixture.voice(0).stretch.native_state_address();
        let lane = &fixture.voice(0).stretch as *const _ as usize;
        let generation = fixture.voice(0).generation;
        let address = fixture.voice(0).sample.as_ref().unwrap().source_address();
        let starts = fixture.starts(0);
        wait_history_capacity(&mut fixture, "initial mode capture");
        let refused = submit_scalar(&mut fixture, true);
        wait_own_candidate(&mut fixture, refused, "initial mode preparation");
        let original_e = fixture.mixer.pending_key_lock_tuples[0].as_ref().unwrap().execution.unwrap();
        assert_eq!(original_e, output_origin + E as u64);
        // Both real source-control edits occur between reads. The endpoint and all
        // scalar source samples still match; only the retained trajectory guard changes.
        fixture.mixer.set_master_period(0.5 / 0.91);
        fixture.mixer.set_master_period(0.5 / 1.29);
        advance_old_to(&mut fixture, original_e + 1, output_origin, &timeline, false, native);
        assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
        let status = fixture.mixer.pad_key_lock_status[0].unwrap();
        assert_eq!(status.request_id, refused);
        assert_eq!(status.state, "error");
        assert!(!status.effective && !status.ready);
        assert!(!fixture.voice(0).stretch.wet_mode_failed());
        assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
        // A new request on the same existing per-voice lane must recapture the
        // retained genuine history. No stop/restart or worker replacement rescues it.
        let fresh_f = fixture.transport.output_frame();
        wait_history_capacity(&mut fixture, "fresh mode capture after refusal");
        assert_eq!(fixture.transport.output_frame(), fresh_f);
        let fresh = submit_scalar(&mut fixture, true);
        assert!(fresh > refused);
        wait_own_candidate(&mut fixture, fresh, "fresh mode preparation after refusal");
        let fresh_e = fixture.mixer.pending_key_lock_tuples[0].as_ref().unwrap().execution.unwrap();
        assert_eq!(fresh_e, fresh_f + 4_096);
        advance_old_to(&mut fixture, fresh_e, output_origin, &timeline, false, native);
        let first = fixture.render(1);
        assert!(first.iter().all(|sample| sample.is_finite()));
        assert!(fixture.mixer.pad_key_lock_enabled[0]);
        assert_ne!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.voice(0).sample.as_ref().unwrap().source_address(), address);
        assert_eq!(&fixture.voice(0).stretch as *const _ as usize, lane);
        assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
        assert_eq!(fixture.starts(0), starts);
        let status = fixture.mixer.pad_key_lock_status[0].unwrap();
        assert_eq!(status.request_id, fresh);
        assert!(status.ready && status.effective);
    }

    include!("key_lock_paused_history_tests.rs");
}
