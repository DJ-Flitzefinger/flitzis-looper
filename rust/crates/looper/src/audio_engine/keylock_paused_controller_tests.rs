//! Public GLOBAL mode ownership with nonzero pitch and a genuinely paused source.
//! The existing independent PCM/Native/EQ oracles continue to own both explicit peers.
//! Nonzero target PCM history accuracy has its separate raw-Native regression suite.

use super::*;
use crate::audio_engine::input_mapping::{InputRuntime, PitchPadProjection};
use crate::audio_engine::pitch_actions::CapturedPitchAction;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};

const SHIFTS: [(usize, i32); 2] = [(0, 7), (36, -3)];

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
fn choose_nonzero_pitch(voices: &mut GlobalVoices) {
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
    for ((id, shift), capture) in SHIFTS.into_iter().zip(&captures) {
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
        voices.own_mut(id).energy = 0.0;
    }
}

/// Advance real audio while independently checking explicit peers and active source clocks.
/// Target peaks prove each nonzero owner remains audible, without using mixed master PCM
/// or a different pad's native output as its own acoustic reference.
fn render(voices: &mut GlobalVoices, frames: usize) {
    for _ in 0..frames {
        let frame = voices.harness.frame;
        let peers: Vec<_> = voices
            .expected
            .iter_mut()
            .filter(|own| !INHERITED.contains(&own.id))
            .map(|own| (own.id, own.next(frame).unwrap()))
            .collect();
        for own in voices
            .expected
            .iter_mut()
            .filter(|own| INHERITED.contains(&own.id))
        {
            if !own.paused {
                own.clock.advance(1);
                own.consumed += 1;
            }
        }
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
                own.energy += peaks[own.id];
                let history = live.stretch.productive_history().unwrap();
                assert_eq!(history.next_position.frame, source_frame);
                assert!((history.next_position.fraction - fraction).abs() < 1e-8);
            }
        }
        voices.harness.frame += 1;
    }
    voices.harness.call0("poll");
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

fn global(voices: &mut GlobalVoices, enabled: bool) {
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
    render(voices, 1);
    let mut candidates = [0; 2];
    let mut executions = [0; 2];
    let mut freeze_candidate = |voices: &mut GlobalVoices, index: usize, expected: u64| {
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
    };
    freeze_candidate(voices, first, f + HISTORY as u64);
    wait_until(Duration::from_secs(10), || {
        drain_one(&mut voices.harness);
        status(&tickets[second]) == "adopting"
    });
    render(voices, 1);
    freeze_candidate(voices, second, f + 1 + HISTORY as u64);
    let old_native = INHERITED.map(|id| voice(&voices.harness, id).stretch.native_state_address());
    let remaining = (executions[first] - voices.harness.frame) as usize;
    render(voices, remaining);
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
    render(voices, 1);
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
    render(voices, 1);
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
    choose_nonzero_pitch(&mut voices);
    render(&mut voices, 1025);
    voices.pause(36, true);
    for enabled in [true, false] {
        global(&mut voices, enabled);
        let paused = FrozenVoice::capture(&voices.harness, 36);
        let journal = feed_history(&voices.harness, 36);
        render(&mut voices, 513);
        paused.assert_exact(&voices.harness, 36);
        assert_eq!(feed_history(&voices.harness, 36), journal);
        voices.pause(36, false);
        let before = FrozenVoice::capture(&voices.harness, 36);
        let fed = voice(&voices.harness, 36)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames;
        render(&mut voices, 1);
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
        render(&mut voices, 1023);
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
    render(&mut voices, 1024);
    voices.assert_receipts();
    voices.assert_origins();
    voices.assert_audible();
}
