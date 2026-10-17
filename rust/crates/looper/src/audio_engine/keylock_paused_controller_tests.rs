//! Public GLOBAL mode ownership with nonzero pitch and a genuinely paused source.
//! Every voice keeps its original PCM/source clock/EQ oracle from public birth.
//! Target Native references certify the exact constant endpoint-H reset profile only.

use super::*;
use crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::ControllerRawNative;
use crate::audio_engine::input_mapping::{InputRuntime, PitchPadProjection};
use crate::audio_engine::pitch_actions::CapturedPitchAction;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};

const SHIFTS: [(usize, i32); 2] = [(0, 7), (36, -3)];

struct TargetCandidate {
    execution: u64,
    endpoint: usize,
    enabled: bool,
    raw: ControllerRawNative,
    history: usize,
    native: usize,
}

struct TargetOracle {
    id: usize,
    shift: i32,
    enabled: bool,
    raw: ControllerRawNative,
    fed: usize,
    native: usize,
    candidate: Option<TargetCandidate>,
    samples: usize,
    raw_maximum: f32,
    eq_maximum: f32,
}

impl TargetOracle {
    fn prepared(
        own: &ExpectedVoice,
        shift: i32,
        enabled: bool,
        endpoint: usize,
    ) -> (ControllerRawNative, usize) {
        let pitch = 2.0_f64.powf(f64::from(shift) / 12.0) / if enabled { RATIO } else { 1.0 };
        let history = ControllerRawNative::constant_history_frames(pitch);
        assert!(
            endpoint >= history,
            "the genuine public birth predates this history"
        );
        if shift == 7 && enabled {
            assert!(pitch > 2.0 && history > HISTORY);
        } else {
            assert_eq!(history, HISTORY);
        }
        // This fixture's original source rate and selection have remained constant.
        // Production therefore retains its established exact endpoint-H profile;
        // rounding A to the O/512 lattice belongs to variable-chronology replay.
        let mut clock = AlgebraicClock::physical(own.start, own.end, RATIO);
        clock.advance(endpoint - history);
        let mono = clock.dry(&own.source, history);
        let feed = [mono.clone(), mono];
        // For p>2 the existing raw helper completes cold initialization with a
        // silent unity block, then sets exact p before the first REAL source block.
        // The silent block supplies neither source history nor adapter FIFO time.
        let mut raw = ControllerRawNative::new_with_pitch(pitch, true);
        raw.render(&feed);
        assert_eq!(raw.fifo_frames(), (0, 511));
        let mut endpoint_clock = AlgebraicClock::physical(own.start, own.end, RATIO);
        endpoint_clock.advance(endpoint);
        let actual_endpoint = clock.physical_position(0);
        let declared_endpoint = endpoint_clock.physical_position(0);
        assert_eq!(actual_endpoint.0, declared_endpoint.0);
        assert!((actual_endpoint.1 - declared_endpoint.1).abs() < 1e-8);
        assert_eq!(actual_endpoint.2, declared_endpoint.2);
        println!(
            "public_global_target_A_reset pad={} k={shift} lock={enabled} r={RATIO} p={pitch} active_endpoint={endpoint} A={} H={history}; constant_endpoint_profile_continuous_O_and_acoustic_gates_separate",
            own.id,
            endpoint - history,
        );
        (raw, history)
    }

    fn after_set(own: &ExpectedVoice, shift: i32, native: usize, execution: u64) -> Self {
        let (raw, history) = Self::prepared(own, shift, false, own.consumed);
        println!(
            "public_global_nonstarting_SET pad={} original_birth_output={} E={execution} original_active_frames={} retained_EQ_frames={}",
            own.id,
            execution - own.consumed as u64,
            own.consumed,
            own.consumed,
        );
        Self {
            id: own.id,
            shift,
            enabled: false,
            raw,
            fed: history,
            native,
            candidate: None,
            samples: 0,
            raw_maximum: 0.0,
            eq_maximum: 0.0,
        }
    }

    fn prepare_mode(
        &mut self,
        own: &ExpectedVoice,
        projection_frame: u64,
        execution: u64,
        enabled: bool,
        native: usize,
    ) {
        assert!(self.candidate.is_none());
        assert_ne!(enabled, self.enabled);
        let endpoint = own.consumed
            + if own.paused {
                0
            } else {
                (execution - projection_frame) as usize
            };
        let (raw, history) = Self::prepared(own, self.shift, enabled, endpoint);
        self.candidate = Some(TargetCandidate {
            execution,
            endpoint,
            enabled,
            raw,
            history,
            native,
        });
    }

    fn next(&mut self, own: &mut ExpectedVoice, frame: u64) -> Option<([f32; 2], [f32; 2])> {
        assert_eq!(self.id, own.id);
        if self
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate.execution == frame)
        {
            let candidate = self.candidate.take().unwrap();
            assert_eq!(own.consumed, candidate.endpoint);
            self.raw = candidate.raw;
            self.fed = candidate.history;
            self.native = candidate.native;
            self.enabled = candidate.enabled;
        }
        if own.paused {
            return None;
        }
        let mono = own.clock.dry(&own.source, 1);
        let dry = mono[0];
        let native = self.raw.render(&[mono.clone(), mono]);
        let raw = [native[0], native[1]];
        self.fed += 1;
        own.consumed += 1;
        own.filter.begin_frame();
        let filtered =
            std::array::from_fn(|channel| own.filter.process_sample(channel, raw[channel]));
        own.energy += filtered[0].abs().max(filtered[1].abs());
        own.wet_difference += (raw[0] - dry).abs();
        self.samples += 1;
        Some((raw, filtered))
    }

    fn assert_fifo(&self, harness: &Harness) {
        let live = voice(harness, self.id);
        assert_eq!(live.stretch.native_state_address(), self.native);
        assert_eq!(live.stretch.pending_fifo_frames(), self.raw.fifo_frames());
        let declared_pitch =
            2.0_f64.powf(f64::from(self.shift) / 12.0) / if self.enabled { RATIO } else { 1.0 };
        assert!(
            (live.stretch.native_pitch_scale_observation().unwrap() - declared_pitch).abs() < 1e-12
        );
        assert_eq!(
            live.stretch.productive_history().unwrap().fed_output_frames,
            self.fed as u64,
        );
    }
}

fn feed_history(harness: &Harness, id: usize) -> (u64, u64, String) {
    let mut snapshot = SourceFeedSnapshot::new();
    assert!(
        voice(harness, id)
            .source_feed_history
            .capture_into(&mut snapshot)
    );
    (
        snapshot.active_origin(),
        snapshot.active_frames(),
        format!("{:?}", snapshot.spans().collect::<Vec<_>>()),
    )
}

/// Install the ordinary device-free input/clock seam, then use real captured actions.
/// Neither the mixer bank nor its effective pitch arrays receive fixture assignments.
fn choose_nonzero_pitch(voices: &mut GlobalVoices) -> Vec<TargetOracle> {
    let clock = Arc::new(SharedOutputClock::new());
    Python::attach(|py| {
        let mut engine = voices.harness.engine.borrow_mut(py);
        engine.test_output_format = Some((RATE, 2));
        let runtime = InputRuntime::new_with_ownership(
            voices.harness.producer.clone(),
            engine.input_clock,
            engine.input_runtime_ownership.clone(),
        );
        runtime.attach_pitch_clock(clock.clone()).unwrap();
        runtime.set_render_format(RATE, 2).unwrap();
        let mut loaded = vec![false; NUM_SAMPLES];
        let mut starts = vec![0.0; NUM_SAMPLES];
        let mut ends = vec![None; NUM_SAMPLES];
        let mut bindings = vec![None; NUM_SAMPLES];
        let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
        for id in IDS {
            loaded[id] = true;
            starts[id] = START as f64 / f64::from(RATE);
            ends[id] = Some(END as f64 / f64::from(RATE));
            bindings[id] = engine.current_input_runtime_pad_binding(id).unwrap();
            rows[id].key_lock = id == 72;
        }
        runtime
            .set_runtime_state_with_pitch(
                true,
                loaded,
                starts,
                ends,
                bindings.iter().map(Option::as_ref).collect(),
                rows,
            )
            .unwrap();
        engine.input_runtime = Some(runtime);
        let now = engine.input_clock.capture_ns();
        clock.publish(OutputClockSnapshot {
            valid: true,
            observed_at_ns: now,
            audible_at_ns: now,
            output_frame: voices.harness.frame,
            sample_rate_hz: RATE,
            master_period_seconds: Some(0.5),
            downbeat_frame: 0,
            master_beat: voices.harness.frame as f64 / 24_000.0,
            freshness_ns: 2_000_000_000,
        });
    });
    let captures: Vec<CapturedPitchAction> = Python::attach(|py| {
        SHIFTS
            .map(|(id, shift)| {
                let capture = voices
                    .harness
                    .engine
                    .borrow(py)
                    .capture_pad_pitch_action(id, &format!("pitch.extra:{shift}:set"), None)
                    .unwrap();
                let owner = Py::new(py, capture.clone()).unwrap();
                assert!(
                    voices
                        .harness
                        .probe
                        .bind(py)
                        .getattr("transport")
                        .unwrap()
                        .getattr("pitch")
                        .unwrap()
                        .call_method1("accept_captured_event", (owner,))
                        .unwrap()
                        .extract::<bool>()
                        .unwrap()
                );
                capture
            })
            .into()
    });
    let f = voices.harness.frame;
    voices.harness.drain();
    wait_until(Duration::from_secs(10), || {
        let runtime = voices
            .harness
            .callback
            .mixer
            .pitch_runtime
            .as_mut()
            .unwrap();
        runtime.poll();
        captures.iter().all(|capture| {
            assert_ne!(capture.state(), "rejected", "{:?}", capture.reason());
            runtime.has_ready_action(capture.sequence())
        })
    });
    let prepared_natives: Vec<_> = captures
        .iter()
        .map(|capture| {
            voices
                .harness
                .callback
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .ready_pitch_native_address(capture.sequence())
                .expect("each original SET owns its prepared Native")
        })
        .collect();
    let e = captures[0].execution_target();
    assert!(e > f);
    for capture in &captures {
        assert_eq!(capture.original_due_frame(), f);
        assert_eq!(capture.execution_target(), e);
        assert!(!capture.effective());
    }
    // Both public SET actions keep their original E. Until E the existing independent
    // zero-pitch oracles still describe all four productive voices exactly.
    voices.render((e - f) as usize);
    voices.harness.render(0, false);
    for (((id, shift), capture), native) in SHIFTS.into_iter().zip(&captures).zip(&prepared_natives)
    {
        assert!(capture.effective(), "{:?}", capture.reason());
        assert_eq!(
            voices.harness.callback.mixer.pad_pitch_intent(id),
            Some(PitchIntent::new(0, shift).unwrap())
        );
        assert_eq!(
            voice(&voices.harness, id).generation,
            voices
                .expected
                .iter()
                .find(|own| own.id == id)
                .unwrap()
                .generation
        );
        assert_eq!(
            voice(&voices.harness, id).stretch.native_state_address(),
            *native
        );
        voices.own_mut(id).energy = 0.0;
    }
    SHIFTS
        .into_iter()
        .map(|(id, shift)| {
            let own = voices.expected.iter().find(|own| own.id == id).unwrap();
            let oracle = TargetOracle::after_set(
                own,
                shift,
                voice(&voices.harness, id).stretch.native_state_address(),
                e,
            );
            oracle.assert_fifo(&voices.harness);
            oracle
        })
        .collect()
}

/// Advance real audio against every voice's independent raw PCM, EQ and active clock.
fn render(voices: &mut GlobalVoices, targets: &mut [TargetOracle], frames: usize) {
    let mut sum_maximum = 0.0_f32;
    for _ in 0..frames {
        let frame = voices.harness.frame;
        let peers: Vec<_> = voices
            .expected
            .iter_mut()
            .filter(|own| !INHERITED.contains(&own.id))
            .map(|own| (own.id, own.next(frame).unwrap()))
            .collect();
        let expected_targets: Vec<_> = targets
            .iter_mut()
            .map(|target| {
                let own = voices
                    .expected
                    .iter_mut()
                    .find(|own| own.id == target.id)
                    .unwrap();
                (target.id, target.next(own, frame))
            })
            .collect();
        let mut actual = [0.0; 2];
        let mut peaks = [0.0; NUM_SAMPLES];
        let callback = voices.harness.callback.as_mut();
        render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut actual,
            &mut peaks,
            frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        assert!(actual.iter().all(|value| value.is_finite()));
        assert_eq!(
            voices
                .harness
                .callback
                .mixer
                .voices
                .iter()
                .filter(|own| own.active)
                .count(),
            IDS.len()
        );
        let mut active_ids: Vec<_> = voices
            .harness
            .callback
            .mixer
            .voices
            .iter()
            .filter(|own| own.active)
            .map(|own| own.sample_id)
            .collect();
        active_ids.sort_unstable();
        assert_eq!(active_ids, IDS, "live owners changed at {frame}");
        let mut sum = [0.0_f32; 2];
        for (id, (raw, filtered)) in peers {
            let own = voices.expected.iter().find(|own| own.id == id).unwrap();
            let buffers = voice(&voices.harness, id).stretch.output_buffers();
            for buffer in &buffers[..2] {
                assert!(
                    (buffer[0] - raw).abs() < 5e-5,
                    "explicit peer {id} raw at {frame}"
                );
            }
            assert!(
                (peaks[id] - filtered[0].abs().max(filtered[1].abs())).abs() < 5e-5,
                "explicit peer {id} EQ at {frame}"
            );
            own.assert_voice(
                &voices.harness,
                own.enabled && own.raw_fed > own.raw.block_size(),
            );
            for channel in 0..2 {
                sum[channel] += filtered[channel];
            }
        }
        for (id, expected) in expected_targets {
            let target = targets.iter_mut().find(|target| target.id == id).unwrap();
            target.assert_fifo(&voices.harness);
            if let Some((raw, filtered)) = expected {
                let buffers = voice(&voices.harness, id).stretch.output_buffers();
                for channel in 0..2 {
                    let error = (buffers[channel][0] - raw[channel]).abs();
                    target.raw_maximum = target.raw_maximum.max(error);
                    assert!(
                        error < 5e-5,
                        "target {id} raw channel {channel} at {frame}: {error}"
                    );
                    sum[channel] += filtered[channel];
                }
                let error = (peaks[id] - filtered[0].abs().max(filtered[1].abs())).abs();
                target.eq_maximum = target.eq_maximum.max(error);
                assert!(error < 5e-5, "target {id} EQ at {frame}: {error}");
            } else {
                assert_eq!(peaks[id], 0.0);
            }
        }
        for own in voices
            .expected
            .iter_mut()
            .filter(|own| INHERITED.contains(&own.id))
        {
            let live = voice(&voices.harness, own.id);
            assert_eq!(
                live.generation, own.generation,
                "pad {} retriggered",
                own.id
            );
            assert_eq!(live.paused, own.paused);
            let (source_frame, fraction, explicit) = own.clock.physical_position(0);
            let position = live.source_playback.position();
            assert_eq!(
                position.frame, source_frame,
                "pad {} source at {frame}",
                own.id
            );
            assert!((position.fraction - fraction).abs() < 1e-8);
            assert!(!explicit);
            assert_eq!(live.source_playback.tempo_ratio(), RATIO);
            assert_eq!(live.volume, 1.0);
            if own.paused {
                assert_eq!(peaks[own.id], 0.0);
            } else {
                let history = live.stretch.productive_history().unwrap();
                assert_eq!(history.next_position.frame, source_frame);
                assert!((history.next_position.fraction - fraction).abs() < 1e-8);
            }
            let dsp = voices
                .harness
                .callback
                .mixer
                .dsp_source_history_for_test(own.id)
                .unwrap();
            assert_eq!(dsp.fed_output_frames, own.consumed as u64);
            assert_eq!(dsp.next_position.frame, source_frame);
            assert!((dsp.next_position.fraction - fraction).abs() < 1e-8);
        }
        for channel in 0..2 {
            let error = (actual[channel] - sum[channel]).abs();
            sum_maximum = sum_maximum.max(error);
            assert!(
                error < 2e-4,
                "independent GLOBAL sum channel {channel} at {frame}: {error}"
            );
        }
        voices.harness.frame += 1;
    }
    voices.harness.call0("poll");
    println!("public_global_target_sample_sum frames={frames} max_abs={sum_maximum}");
}

/// Partition the genuine producer queue without changing its messages or fences.
fn drain_one(harness: &mut Harness) {
    let Ok(message) = harness.consumer.pop() else {
        return;
    };
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    producer.push(message).unwrap();
    let callback = harness.callback.as_mut();
    assert_eq!(
        drain_control_messages(
            &mut consumer,
            &mut callback.scheduler,
            harness.frame,
            &mut callback.quantization,
            &mut callback.transport,
            &mut callback.mixer,
            &mut callback.feedback,
            &mut callback.retirement,
        ),
        1
    );
}

fn status(ticket: &Py<ResidentWindowTicket>) -> String {
    Python::attach(|py| ticket.borrow(py).publication_status().to_owned())
}

fn global(voices: &mut GlobalVoices, targets: &mut [TargetOracle], enabled: bool) {
    let original = INHERITED.map(|id| identity(&voices.harness, id));
    let paused = FrozenVoice::capture(&voices.harness, 36);
    let paused_feed = feed_history(&voices.harness, 36);
    let peers = [72, 215].map(|id| FrozenVoice::capture(&voices.harness, id));
    for id in INHERITED {
        wait_until(Duration::from_secs(10), || {
            voice_mut(&mut voices.harness, id)
                .stretch
                .source_history_work_available()
        });
    }
    voices.harness.callback.retirement.capacity = 0;
    voices.harness.global(enabled);
    let tickets = INHERITED.map(|id| {
        pending(&voices.harness, id, enabled);
        voices.harness.ticket(id)
    });
    wait_until(Duration::from_secs(10), || {
        voices.harness.drain();
        tickets.iter().all(|ticket| status(ticket) == "pending")
    });
    paused.assert_exact(&voices.harness, 36);
    for (id, before) in [72, 215].into_iter().zip(&peers) {
        before.assert_exact(&voices.harness, id);
    }
    voices.harness.callback.retirement.capacity = usize::MAX;
    // Admit only one actual resident message first. Its F/E is deliberately one
    // output frame earlier than the other target's, exposing any ACK borrowing.
    wait_until(Duration::from_secs(10), || {
        drain_one(&mut voices.harness);
        tickets.iter().any(|ticket| status(ticket) == "adopting")
    });
    let first = tickets
        .iter()
        .position(|ticket| status(ticket) == "adopting")
        .unwrap();
    let second = 1 - first;
    assert_eq!(status(&tickets[second]), "pending");
    let f = voices.harness.frame;
    render(voices, targets, 1);
    let mut candidates = [0; 2];
    let mut executions = [0; 2];
    let mut freeze_candidate =
        |voices: &mut GlobalVoices, targets: &mut [TargetOracle], index: usize, expected: u64| {
            let id = INHERITED[index];
            let own = voice_mut(&mut voices.harness, id);
            wait_until(Duration::from_secs(10), || {
                own.stretch.prospective_work_ready()
            });
            executions[index] = own.stretch.prepared_target_output_frame().unwrap();
            assert_eq!(executions[index], expected);
            candidates[index] = own.stretch.prepared_native_address();
            assert_ne!(candidates[index], 0);
            assert_ne!(candidates[index], own.stretch.native_state_address());
            let fifo = own.stretch.prepared_fifo_frames();
            let declared = voices.expected.iter().find(|own| own.id == id).unwrap();
            let target = targets.iter_mut().find(|target| target.id == id).unwrap();
            target.prepare_mode(
                declared,
                voices.harness.frame,
                expected,
                enabled,
                candidates[index],
            );
            assert_eq!(
                fifo,
                Some(target.candidate.as_ref().unwrap().raw.fifo_frames())
            );
        };
    freeze_candidate(voices, targets, first, f + HISTORY as u64);
    wait_until(Duration::from_secs(10), || {
        drain_one(&mut voices.harness);
        status(&tickets[second]) == "adopting"
    });
    render(voices, targets, 1);
    freeze_candidate(voices, targets, second, f + 1 + HISTORY as u64);
    let old_native = INHERITED.map(|id| voice(&voices.harness, id).stretch.native_state_address());
    let remaining = (executions[first] - voices.harness.frame) as usize;
    render(voices, targets, remaining);
    for index in 0..2 {
        let id = INHERITED[index];
        assert_eq!(status(&tickets[index]), "adopting");
        pending(&voices.harness, id, enabled);
        assert_eq!(
            voice(&voices.harness, id).stretch.native_state_address(),
            old_native[index]
        );
        assert_eq!(
            voices.harness.callback.mixer.key_lock_for_measurement(id),
            !enabled
        );
    }
    render(voices, targets, 1);
    assert_eq!(status(&tickets[first]), "accepted");
    assert_eq!(
        status(&tickets[second]),
        "adopting",
        "a target borrowed the earlier target's MODEACK"
    );
    assert!(voices.harness.settled(INHERITED[first], enabled));
    pending(&voices.harness, INHERITED[second], enabled);
    assert_eq!(
        voice(&voices.harness, INHERITED[first])
            .stretch
            .native_state_address(),
        candidates[first]
    );
    render(voices, targets, 1);
    assert_eq!(status(&tickets[second]), "accepted");
    for (index, id) in INHERITED.into_iter().enumerate() {
        let own = voice(&voices.harness, id);
        assert_eq!(own.stretch.native_state_address(), candidates[index]);
        assert!(
            own.stretch
                .owns_pitch_contract(PitchIntent::new(0, SHIFTS[index].1).unwrap(), enabled)
        );
        let receipt = identity(&voices.harness, id);
        assert_eq!(
            receipt[..2],
            original[index][..2],
            "pad {id} changed its own source identity"
        );
        let window = Python::attach(|py| {
            tickets[index]
                .bind(py)
                .getattr("window_revision")
                .unwrap()
                .extract::<u64>()
                .unwrap()
        });
        assert_eq!(
            receipt[2..],
            [window, voices.harness.mode_request(&tickets[index])]
        );
        assert!(Python::attach(|py| tickets[index].borrow(py).is_current()));
        assert!(voices.harness.settled(id, enabled));
        voices
            .expected
            .iter_mut()
            .find(|own| own.id == id)
            .unwrap()
            .receipt = receipt;
    }
    paused.assert_source_eq(&voices.harness, 36);
    assert_eq!(feed_history(&voices.harness, 36), paused_feed);
    assert!(!voice(&voices.harness, 36).stretch.wet_output_ready());
    assert!(
        !voice(&voices.harness, 36)
            .stretch
            .wet_current_source_output_ready()
    );
    if enabled {
        Python::attach(|py| {
            let feedback = voices
                .harness
                .engine
                .borrow(py)
                .pad_key_lock_status(py, 36)
                .unwrap()
                .unwrap();
            assert_eq!(
                feedback
                    .bind(py)
                    .get_item("state")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "armed"
            );
        });
    }
    voices.assert_origins();
}

#[test]
fn actual_public_global_nonzero_pitch_paused_targets_keep_own_e_ack_and_resume_native_fifo() {
    let mut voices = GlobalVoices::new();
    voices.render(5000);
    let mut targets = choose_nonzero_pitch(&mut voices);
    render(&mut voices, &mut targets, 1025);
    voices.pause(36, true);
    for enabled in [true, false] {
        global(&mut voices, &mut targets, enabled);
        let paused = FrozenVoice::capture(&voices.harness, 36);
        let journal = feed_history(&voices.harness, 36);
        let paused_oracle = targets.iter().find(|target| target.id == 36).unwrap();
        let oracle_frozen = (
            paused_oracle.fed,
            paused_oracle.raw.fifo_frames(),
            paused_oracle.samples,
        );
        render(&mut voices, &mut targets, 513);
        paused.assert_exact(&voices.harness, 36);
        assert_eq!(feed_history(&voices.harness, 36), journal);
        let paused_oracle = targets.iter().find(|target| target.id == 36).unwrap();
        assert_eq!(
            (
                paused_oracle.fed,
                paused_oracle.raw.fifo_frames(),
                paused_oracle.samples
            ),
            oracle_frozen,
        );
        voices.pause(36, false);
        let before = FrozenVoice::capture(&voices.harness, 36);
        let fed = voice(&voices.harness, 36)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames;
        render(&mut voices, &mut targets, 1);
        let own = voice(&voices.harness, 36);
        assert_eq!(
            own.stretch.native_state_address(),
            before.native,
            "resume replaced own prepared Native"
        );
        assert_eq!(
            own.stretch.pending_fifo_frames(),
            ((before.fifo.0 + 1) % 512, 511 - (before.fifo.0 + 1) % 512)
        );
        assert_eq!(
            own.stretch.productive_history().unwrap().fed_output_frames,
            fed + 1
        );
        assert!(
            own.stretch.wet_current_source_output_ready(),
            "first resumed sample did not use its own Native"
        );
        if enabled {
            native_wet(&voices.harness, 36);
        }
        render(&mut voices, &mut targets, 1023);
        for id in INHERITED {
            assert!(
                voices
                    .expected
                    .iter()
                    .find(|own| own.id == id)
                    .unwrap()
                    .energy
                    > 1.0,
                "nonzero pad {id} lost its audible contribution"
            );
        }
        voices.assert_receipts();
        voices.pause(36, true);
    }
    voices.pause(36, false);
    render(&mut voices, &mut targets, 1024);
    voices.assert_receipts();
    voices.assert_origins();
    voices.assert_audible();
    for target in targets {
        assert!(target.samples >= 4097);
        assert!(target.candidate.is_none());
        println!(
            "public_global_nonzero_target pad={} k={} compared_samples={} raw_max_abs={} eq_peak_max_abs={}; own_staggered_E_and_first_adopted_resumed_samples_checked_continuous_O_acoustic_RT_gates_separate",
            target.id, target.shift, target.samples, target.raw_maximum, target.eq_maximum,
        );
    }
}
