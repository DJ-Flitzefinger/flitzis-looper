//! Real source/ACK, original policy queue and actual dry-render static correction.
//! Generated PCM and declared grids do not establish device or musical acceptance.

#[path = "runtime_policy_static_master_tests.rs"]
mod master_tests;

use super::*;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::loop_phase_reference::LoopPhaseReference;
use crate::audio_engine::prepared_native_history::SourceReadCheckpoint;
use crate::audio_engine::source_control_epoch::SourceControlEpoch;
use crate::audio_engine::source_map_admission::SourceMapRow;
use crate::audio_engine::source_reader::{SourceReadPlan, StemReadSets};
use crate::test_memory::{ThreadAllocations, observe_current_thread};
use std::sync::Mutex;

const STATIC_START: usize = 13;
const STATIC_SOURCE_FRAMES: usize = 32_768;
const STATIC_F: f64 = 64.0;
const STATIC_PERIOD: f64 = STATIC_F / RATE as f64;
const STATIC_ORIGIN: f64 = -12.0;

struct StaticPath {
    path: Path,
    lanes: usize,
    lengths: Vec<usize>,
    parameters: rtrb::Consumer<crate::messages::ControlParameterMessage>,
}

impl StaticPath {
    fn new(lengths: &[usize]) -> Self {
        Self::new_with_pause(lengths, false)
    }

    fn with_pause(lengths: &[usize]) -> Self {
        Self::new_with_pause(lengths, true)
    }

    fn new_with_pause(lengths: &[usize], pause_before_progress: bool) -> Self {
        assert!(!lengths.is_empty() && lengths.len() <= 32);
        let pcm = (0..STATIC_SOURCE_FRAMES)
            .map(|frame| ((frame * 73 % 28_001) as i32 - 14_000) as i16)
            .collect::<Vec<_>>();
        let mut source = ColdCustody::new_selected_with_signal(None, &pcm);
        for id in 2..lengths.len() {
            source.load_and_ack_selected_with_frames(id, None, STATIC_SOURCE_FRAMES);
        }
        source.engine.test_output_format = Some((RATE, 2));
        source.engine.input_runtime = Some(InputRuntime::new_with_ownership(
            source.producer.clone(),
            source.engine.input_clock,
            source.engine.input_runtime_ownership.clone(),
        ));
        source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_render_format(RATE, 2)
            .unwrap();
        let (producer, parameters) = rtrb::RingBuffer::new(128);
        source.engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
        source.mixer.set_bpm_lock(true);
        source.mixer.set_master_period(STATIC_PERIOD);
        source.mixer.set_speed(1.0);
        source.mixer.set_volume(1.0 / lengths.len() as f32);
        let mut timeline = TransportTimeline::new(RATE);
        assert!(timeline.set_master_period(STATIC_PERIOD));
        timeline.complete_bootstrap();
        assert_eq!(timeline.frames_per_beat(), Some(STATIC_F));
        let path = Path {
            source,
            timeline,
            scheduler: FixedCapacityScheduler::new(),
            events: Vec::with_capacity(8192),
        };
        let mut fixture = Self {
            path,
            lanes: lengths.len(),
            lengths: lengths.to_vec(),
            parameters,
        };
        for id in 0..fixture.lanes {
            fixture
                .path
                .source
                .engine
                .set_pad_bpm(id, Some(45_000.0))
                .unwrap();
            fixture
                .path
                .source
                .engine
                .set_pad_timing_metadata(id, STATIC_ORIGIN / f64::from(RATE))
                .unwrap();
            fixture.path.source.mixer.set_pad_key_lock(id, false);
            fixture.drain();
        }
        fixture.publish(false, 1, 7, None);
        for id in 0..fixture.lanes {
            let birth = fixture.start(id);
            assert_eq!(birth.state(), "applied", "{:?}", birth.reason());
            assert!(!fixture.path.voice(id).source_playback.is_static_phase());
        }
        if pause_before_progress {
            assert_eq!(fixture.path.timeline.output_frame(), 0);
            for id in 0..fixture.lanes {
                let pause = fixture.path.source.engine.pause_sample(id).unwrap();
                fixture.drain();
                assert!(
                    pause
                        .current_observation()
                        .unwrap()
                        .unwrap()
                        .ordinary_paused()
                );
                assert_eq!(
                    fixture.path.voice(id).source_playback.position().frame,
                    STATIC_START
                );
            }
            let silent = fixture.path.render(25);
            assert!(silent.iter().all(|sample| *sample == 0.0));
            assert_eq!(fixture.path.timeline.output_frame(), 25);
            for id in 0..fixture.lanes {
                let raw = fixture.path.voice(id).source_playback.position();
                assert_eq!(raw.frame, STATIC_START);
                assert_eq!(raw.fraction, 0.0);
                fixture.path.source.engine.resume_sample(id).unwrap();
                fixture.drain();
                assert!(!fixture.path.voice(id).paused);
                assert_eq!(fixture.path.voice(id).source_playback.position(), raw);
            }
        }
        fixture.path.render(23);
        for id in 0..fixture.lanes {
            assert_eq!(
                fixture.path.voice(id).source_playback.position().frame,
                STATIC_START + 23
            );
            assert_eq!(fixture.path.voice(id).source_playback.tempo_ratio(), 1.0);
        }
        fixture
    }

    fn publish(&self, sync: bool, revision: u64, intent: u64, key_lock: Option<usize>) {
        self.publish_parent(true, sync, revision, intent, key_lock);
    }

    fn publish_parent(
        &self,
        multi_loop: bool,
        sync: bool,
        revision: u64,
        intent: u64,
        key_lock: Option<usize>,
    ) {
        self.publish_parent_with_optout(multi_loop, sync, revision, intent, key_lock, None);
    }

    fn publish_parent_with_optout(
        &self,
        multi_loop: bool,
        sync: bool,
        revision: u64,
        intent: u64,
        key_lock: Option<usize>,
        optout: Option<(usize, u64)>,
    ) {
        let bindings = (0..NUM_SAMPLES)
            .map(|id| {
                (id < self.lanes).then(|| {
                    self.path
                        .source
                        .engine
                        .current_input_runtime_pad_binding(id)
                        .unwrap()
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let rows = (0..NUM_SAMPLES)
            .map(|id| SourceMapRow {
                sync_requested: id < self.lanes
                    && optout.is_none_or(|(excluded, _)| id != excluded),
                intent_revision: optout
                    .filter(|(excluded, _)| id == *excluded)
                    .map_or(intent, |(_, revision)| revision),
                ..Default::default()
            })
            .collect();
        let mut starts = vec![0.0; NUM_SAMPLES];
        let mut ends = vec![None; NUM_SAMPLES];
        for id in 0..self.lanes {
            starts[id] = STATIC_START as f64 / f64::from(RATE);
            ends[id] = Some((STATIC_START + self.lengths[id]) as f64 / f64::from(RATE));
        }
        self.path
            .source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_policy(
                multi_loop,
                (0..NUM_SAMPLES).map(|id| id < self.lanes).collect(),
                starts,
                ends,
                bindings.iter().map(Option::as_ref).collect(),
                Some(
                    (0..NUM_SAMPLES)
                        .map(|id| PitchPadProjection {
                            sync: true,
                            key_lock: key_lock == Some(id),
                            ..Default::default()
                        })
                        .collect(),
                ),
                Some(rows),
                GlobalTimingPolicy::parse(Some(&wire(sync, false, revision).to_string())).unwrap(),
            )
            .unwrap();
    }

    fn start(&mut self, id: usize) -> CapturedPadStart {
        let birth = self
            .path
            .source
            .engine
            .capture_pad_start(
                id,
                STATIC_START as f64 / f64::from(RATE),
                Some((STATIC_START + self.lengths[id]) as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        self.drain();
        assert!(
            self.path
                .source
                .engine
                .publish_pad_start(&birth, None)
                .unwrap()
        );
        self.drain();
        birth
    }

    fn drain(&mut self) {
        self.path.drain();
        drain_parameter_messages(
            &mut self.parameters,
            &mut self.path.source.mixer,
            &mut self.path.timeline,
        );
    }

    fn effect(&mut self) -> Arc<RuntimePolicySnapshot> {
        let snapshot = request(&self.path);
        self.drain();
        self.path.render(0);
        snapshot
    }

    fn install(&mut self) -> Arc<RuntimePolicySnapshot> {
        self.publish(true, 2, 7, None);
        let first = self.effect();
        assert_eq!(
            first.observation().unwrap().state,
            "applied",
            "{:?}",
            first.observation()
        );
        assert!(
            (0..self.lanes).all(|id| !self.path.voice(id).source_playback.is_static_phase()),
            "new Fixed request must first retain ordinary scalar adoption"
        );
        let second = self.effect();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(
            second.observation().unwrap().state,
            "applied",
            "{:?}",
            second.observation()
        );
        assert!((0..self.lanes).all(|id| self.path.voice(id).source_playback.is_static_phase()));
        for id in 0..self.lanes {
            assert!(self.path.voice(id).source_playback.static_grid_matches(
                STATIC_F,
                STATIC_ORIGIN,
                self.lengths[id] as f64 / STATIC_F
            ));
        }
        second
    }

    fn capture_pitch(
        &self,
        action: &str,
    ) -> crate::audio_engine::pitch_actions::CapturedPitchAction {
        use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};
        let clock = Arc::new(SharedOutputClock::new());
        let now = self.path.source.engine.input_clock.capture_ns();
        clock.publish(OutputClockSnapshot {
            valid: true,
            observed_at_ns: now,
            audible_at_ns: now,
            output_frame: self.path.timeline.output_frame(),
            sample_rate_hz: RATE,
            master_period_seconds: self.path.timeline.master_period_seconds(),
            downbeat_frame: self.path.timeline.downbeat_frame(),
            master_beat: self.path.timeline.beat_position().unwrap(),
            freshness_ns: 2_000_000_000,
        });
        self.path
            .source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .attach_pitch_clock(clock)
            .unwrap();
        self.path
            .source
            .engine
            .capture_pad_pitch_action(0, action, None)
            .unwrap()
    }

    fn render_into(&mut self, output: &mut [f32], retirement: &mut impl AudioBufferRetirement) {
        let frames = output.len() / 2;
        render_scheduled_audio(
            &mut self.path.source.mixer,
            &mut self.path.scheduler,
            output,
            &mut [0.0; NUM_SAMPLES],
            self.path.timeline.output_frame(),
            2,
            &mut self.path.timeline,
            &mut self.path.events,
            retirement,
        );
        self.path.timeline.advance_by_rendered_frames(frames);
    }
}

/// Exact integer reference for the binary case, independently checked by the
/// retained launch-relative Fraction oracle. Birth T=0, real pause25, E=48,
/// raw=36, A=13, F=64: positive debt25, slope65/64 until n=1648, then1.
/// O=-12 remains source-grid provenance. No production query
/// participates in this calculation.
fn dense_position(frame: u64, length: usize) -> (usize, f64) {
    assert!(frame >= 48);
    let elapsed = u128::from(frame - 48);
    let unfolded = 36_u128 * 64 + elapsed * 64 + elapsed.min(1600);
    let relative = (unfolded - STATIC_START as u128 * 64) % (length as u128 * 64);
    (
        STATIC_START + (relative / 64) as usize,
        (relative % 64) as f64 / 64.0,
    )
}

fn verify_dense_actual_reads(fixture: &StaticPath, id: usize) -> usize {
    let voice = fixture.path.voice(id);
    let sample = voice.sample.as_ref().unwrap();
    let mut history = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut history));
    let mut count = 0;
    for span in history
        .spans()
        .filter(|span| span.playback.is_static_phase())
    {
        let first = span.playback.static_output_frame().unwrap();
        let mut expected_pcm = [vec![0.0; span.frames], vec![0.0; span.frames]];
        for offset in 0..=span.frames {
            let (whole, fraction) = dense_position(first + offset as u64, fixture.lengths[id]);
            let actual = span.playback.position_at(offset).unwrap();
            assert_eq!(
                (actual.frame, actual.fraction.to_bits()),
                (whole, fraction.to_bits())
            );
            let plan = SourceReadPlan {
                frame_pos: whole,
                ..span.plan
            };
            let taps = plan.try_fractional_taps(fraction).unwrap();
            let right = if fraction == 0.0 {
                None
            } else {
                Some(if whole + 1 == STATIC_START + fixture.lengths[id] {
                    STATIC_START
                } else {
                    whole + 1
                })
            };
            assert_eq!(
                (taps.left, taps.right, taps.right_gain.to_bits()),
                (whole, right, (fraction as f32).to_bits())
            );
            if offset < span.frames {
                for channel in 0..2 {
                    let a = sample.samples[(whole - sample.resident_start()) * 2 + channel];
                    expected_pcm[channel][offset] = right.map_or(a, |right| {
                        let b = sample.samples[(right - sample.resident_start()) * 2 + channel];
                        a + (b - a) * (fraction as f32)
                    });
                }
            }
        }
        for checked in [false, true] {
            let mut actual = [vec![17.0; span.frames], vec![19.0; span.frames]];
            let result = if checked {
                span.plan.fill_fractional_buffers_checked_with_readers(
                    sample,
                    StemReadSets::default(),
                    &span.playback,
                    &mut actual,
                    span.frames,
                )
            } else {
                span.plan.fill_fractional_buffers_with_readers(
                    sample,
                    StemReadSets::default(),
                    &span.playback,
                    &mut actual,
                    span.frames,
                )
            };
            result.unwrap();
            assert_eq!(actual, expected_pcm);
        }
        if span.frames > 2 {
            let nested = span.slice(1, span.frames - 1).unwrap().slice(1, 1).unwrap();
            let direct = span.slice(2, 1).unwrap();
            assert!(nested.playback.matches_exact(&direct.playback));
            assert!(nested.plan.matches_exact(direct.plan));
        }
        count += span.frames;
    }
    count
}

fn verify_first_resumed_read(fixture: &StaticPath, raw: FractionalSourcePosition) {
    let voice = fixture.path.voice(0);
    let sample = voice.sample.as_ref().unwrap();
    let mut history = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut history));
    let span = history.spans().last().unwrap();
    assert_eq!(span.frames, 1);
    assert!(!span.playback.is_static_phase());
    assert_eq!(
        (
            span.playback.position().frame,
            span.playback.position().fraction.to_bits()
        ),
        (raw.frame, raw.fraction.to_bits())
    );
    let right = if raw.fraction == 0.0 {
        None
    } else {
        Some(if raw.frame + 1 == STATIC_START + fixture.lengths[0] {
            STATIC_START
        } else {
            raw.frame + 1
        })
    };
    let taps = span.plan.try_fractional_taps(raw.fraction).unwrap();
    assert_eq!(
        (taps.left, taps.right, taps.right_gain.to_bits()),
        (raw.frame, right, (raw.fraction as f32).to_bits())
    );
    let expected = (0..2)
        .map(|channel| {
            let a = sample.samples[(raw.frame - sample.resident_start()) * 2 + channel];
            right.map_or(a, |frame| {
                let b = sample.samples[(frame - sample.resident_start()) * 2 + channel];
                a + (b - a) * (raw.fraction as f32)
            })
        })
        .collect::<Vec<_>>();
    for checked in [false, true] {
        let mut actual = [vec![17.0; 1], vec![19.0; 1]];
        let result = if checked {
            span.plan.fill_fractional_buffers_checked_with_readers(
                sample,
                StemReadSets::default(),
                &span.playback,
                &mut actual,
                1,
            )
        } else {
            span.plan.fill_fractional_buffers_with_readers(
                sample,
                StemReadSets::default(),
                &span.playback,
                &mut actual,
                1,
            )
        };
        result.unwrap();
        assert_eq!(
            (actual[0][0].to_bits(), actual[1][0].to_bits()),
            (expected[0].to_bits(), expected[1].to_bits())
        );
    }
}

#[test]
fn static_aligned_original_launch_ignores_off_grid_origin_without_artificial_debt() {
    let mut fixture = StaticPath::new(&[2048, 16384]);
    let mut ordinary = StaticPath::new(&[2048, 16384]);
    assert_eq!(fixture.path.timeline.output_frame(), 23);
    let snapshot = fixture.install();
    assert_eq!(snapshot.observation().unwrap().execution_frame, Some(23));
    for id in 0..2 {
        let playback = fixture.path.voice(id).source_playback;
        assert_eq!(playback.position().frame, 36);
        assert_eq!(playback.position().fraction.to_bits(), 0_f64.to_bits());
        assert!(playback.static_phase_settled());
        assert_eq!(playback.tempo_ratio(), 1.0);
    }
    for frames in [1, 31, 1993, 4096, 16384] {
        assert_eq!(fixture.path.render(frames), ordinary.path.render(frames));
        let output = fixture.path.timeline.output_frame() as usize;
        for id in 0..2 {
            let playback = fixture.path.voice(id).source_playback;
            assert!(playback.static_phase_settled());
            assert_eq!(
                playback.position().frame,
                STATIC_START + output % fixture.lengths[id]
            );
            assert_eq!(playback.position().fraction.to_bits(), 0_f64.to_bits());
        }
    }
}

#[test]
fn static_actual_e_preserves_first_pair_taps_pcm_and_requires_a_second_original_claim() {
    let mut fixture = StaticPath::with_pause(&[2048, 16384]);
    let mut preceding = StaticPath::with_pause(&[2048, 16384]);
    let master = fixture.path.timeline;
    let raw = [
        fixture.path.voice(0).source_playback,
        fixture.path.voice(1).source_playback,
    ];
    let snapshot = fixture.install();
    assert_eq!(snapshot.observation().unwrap().execution_frame, Some(48));
    for (id, old) in raw.iter().enumerate() {
        let actual = fixture.path.voice(id).source_playback;
        assert_eq!(
            (
                actual.position().frame,
                actual.position().fraction.to_bits()
            ),
            (old.position().frame, old.position().fraction.to_bits())
        );
        assert_eq!(actual.effective_domain(), old.effective_domain());
        assert_eq!(actual.static_first_frame(), Some(48));
        assert!(!actual.static_phase_settled());
    }
    assert!(fixture.path.timeline.matches_clock_epoch(&master));
    assert_eq!(fixture.path.render(1), preceding.path.render(1));
    assert_eq!(verify_dense_actual_reads(&fixture, 0), 1);
    assert_eq!(verify_dense_actual_reads(&fixture, 1), 1);
}

#[test]
fn static_actual_eight_and_sixty_four_bar_loops_keep_independent_modulo_and_exact_pcm() {
    let mut results = Vec::new();
    for partitions in [&[512][..], &[31, 257, 1, 96, 777][..]] {
        let mut fixture = StaticPath::with_pause(&[2048, 16384]);
        fixture.install();
        let master = fixture.path.timeline;
        let mut output = Vec::with_capacity(65536 * 2);
        let mut remaining = 65536;
        let mut part = 0;
        while remaining > 0 {
            let frames = partitions[part % partitions.len()].min(remaining);
            output.extend(fixture.path.render(frames));
            remaining -= frames;
            part += 1;
        }
        assert!(fixture.path.timeline.matches_clock_epoch(&master));
        assert_eq!(fixture.path.timeline.output_frame(), 65584);
        for id in 0..2 {
            assert_eq!(verify_dense_actual_reads(&fixture, id), 65536);
            let voice = fixture.path.voice(id);
            assert!(voice.source_playback.static_phase_settled());
            let (whole, fraction) = dense_position(65584, fixture.lengths[id]);
            assert_eq!(
                (
                    voice.source_playback.position().frame,
                    voice.source_playback.position().fraction.to_bits()
                ),
                (whole, fraction.to_bits())
            );
        }
        results.push((
            output,
            fixture.path.voice(0).source_playback,
            fixture.path.voice(1).source_playback,
        ));
    }
    assert_eq!(results[0].0, results[1].0);
    assert!(results[0].1.matches_exact(&results[1].1));
    assert!(results[0].2.matches_exact(&results[1].2));
}

#[test]
fn static_same_snapshot_service_waits_for_actual_glide_then_keeps_one_program() {
    let mut fixture = StaticPath::new(&[2048]);
    fixture.path.source.mixer.set_bpm_lock(false);
    fixture.path.source.mixer.set_speed(1.5);
    fixture.path.render(6000);
    fixture.path.source.mixer.set_bpm_lock(true);
    // The legacy test setter clears the actual matching master on OFF. Restore
    // that original fixture master explicitly before requesting Fixed matching.
    fixture.path.source.mixer.set_master_period(STATIC_PERIOD);
    fixture.publish(true, 2, 7, None);
    let snapshot = fixture.effect();
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    assert_eq!(fixture.path.voice(0).source_playback.rate_target(), 1.0);
    fixture.path.render(6000);
    assert_eq!(fixture.path.voice(0).source_playback.tempo_ratio(), 1.0);
    assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
    assert!(fixture.path.voice(0).source_playback.is_static_phase());
    let raw = fixture.path.voice(0).source_playback;
    let attempt = snapshot.observation().unwrap().attempt;
    for _ in 0..32 {
        assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
        assert!(fixture.path.voice(0).source_playback.matches_exact(&raw));
        assert_eq!(snapshot.observation().unwrap().attempt, attempt);
    }
}

#[test]
fn static_nonclosure_refuses_the_second_claim_for_all_thirty_two_lanes_without_partial_programs() {
    let mut lengths = vec![2048; 32];
    lengths[31] = 2047;
    let mut fixture = StaticPath::new(&lengths);
    fixture.publish(true, 2, 7, None);
    let snapshot = fixture.effect();
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert!((0..32).all(|id| !fixture.path.voice(id).source_playback.is_static_phase()));
    let before = scalar_state(&fixture.path);
    assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
    assert_eq!(snapshot.observation().unwrap().state, "refused");
    unchanged(&fixture.path, &before);
    fixture.path.render(1);
    assert!((0..32).all(|id| !fixture.path.voice(id).source_playback.is_static_phase()));
    let attempt = snapshot.observation().unwrap().attempt;
    assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
    assert!(
        snapshot.observation().unwrap().attempt > attempt,
        "advancing/numeric phase refusal must not receive immutable suppression"
    );
}

#[test]
fn static_pause_and_real_hold_resume_preserve_cursor_and_requalify_at_the_live_master() {
    let mut fixture = StaticPath::with_pause(&[2048]);
    fixture.install();
    assert!(!fixture.path.voice(0).source_playback.static_phase_settled());
    assert_ne!(
        fixture.path.voice(0).source_playback.tempo_ratio(),
        fixture.path.voice(0).source_playback.rate_target()
    );
    fixture.path.render(73);
    let before_failed_hold = fixture.path.voice(0).source_playback;
    assert!(fixture.path.source.consumer.is_empty());
    let mut fillers = 0;
    while fixture
        .path
        .source
        .producer
        .lock()
        .unwrap()
        .push(crate::messages::ControlMessage::Ping())
        .is_ok()
    {
        fillers += 1;
    }
    assert!(fillers > 0);
    assert!(Python::attach(|py| fixture
        .path
        .source
        .engine
        .pause_waveform_hold(0, 5510_u64.into_pyobject(py).unwrap().as_any(), None)
        .is_err()));
    for _ in 0..fillers {
        assert!(matches!(
            fixture.path.source.consumer.pop().unwrap(),
            crate::messages::ControlMessage::Ping()
        ));
    }
    assert!(fixture.path.source.consumer.is_empty());
    fixture.drain();
    assert!(!fixture.path.voice(0).paused);
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .matches_exact(&before_failed_hold)
    );
    let raw = fixture.path.voice(0).source_playback.position();
    let pause = fixture.path.source.engine.pause_sample(0).unwrap();
    fixture.drain();
    assert!(
        pause
            .current_observation()
            .unwrap()
            .unwrap()
            .ordinary_paused()
    );
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    fixture.path.render(97);
    assert_eq!(fixture.path.voice(0).source_playback.position(), raw);
    fixture.path.source.engine.resume_sample(0).unwrap();
    fixture.drain();
    assert_eq!(fixture.path.voice(0).source_playback.position(), raw);
    let resumed = fixture.path.timeline.output_frame();
    let snapshot = fixture.effect();
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Busy)
    );
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    assert_eq!(fixture.path.voice(0).source_playback.position(), raw);
    fixture.path.render(1);
    verify_first_resumed_read(&fixture, raw);
    let after_first_read = fixture.path.voice(0).source_playback.position();
    assert_eq!(fixture.path.voice(0).source_playback.tempo_ratio(), 1.0);
    assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
    assert_eq!(
        fixture.path.voice(0).source_playback.static_first_frame(),
        Some(resumed + 1)
    );
    assert_eq!(
        fixture.path.voice(0).source_playback.position(),
        after_first_read
    );
    fixture.path.render(19);
    let raw = fixture.path.voice(0).source_playback.position();
    let hold = Python::attach(|py| {
        fixture
            .path
            .source
            .engine
            .pause_waveform_hold(0, 5511_u64.into_pyobject(py).unwrap().as_any(), None)
            .unwrap()
    });
    fixture.drain();
    assert!(hold.is_current());
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    fixture.path.render(41);
    assert_eq!(fixture.path.voice(0).source_playback.position(), raw);
    assert!(
        fixture
            .path
            .source
            .engine
            .release_waveform_hold(&hold)
            .unwrap()
    );
    fixture.drain();
    assert_eq!(hold.publication_status(), "resumed");
    assert_eq!(fixture.path.voice(0).source_playback.position(), raw);
    let resumed = fixture.path.timeline.output_frame();
    let snapshot = fixture.effect();
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Busy)
    );
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    assert_eq!(fixture.path.voice(0).source_playback.position(), raw);
    fixture.path.render(1);
    verify_first_resumed_read(&fixture, raw);
    let after_first_read = fixture.path.voice(0).source_playback.position();
    assert_eq!(fixture.path.voice(0).source_playback.tempo_ratio(), 1.0);
    assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
    assert_eq!(
        fixture.path.voice(0).source_playback.static_first_frame(),
        Some(resumed + 1)
    );
    assert_eq!(
        fixture.path.voice(0).source_playback.position(),
        after_first_read
    );
}

#[test]
fn static_raw_parent_away_back_before_callback_requires_a_new_claim_without_numeric_restart() {
    let mut fixture = StaticPath::new(&[2048, 16384]);
    let original = fixture.install();
    let clocks = [
        fixture.path.voice(0).source_playback,
        fixture.path.voice(1).source_playback,
    ];
    let requests = [
        fixture.path.voice(0).sync_request,
        fixture.path.voice(1).sync_request,
    ];
    assert!((0..2).all(|id| {
        fixture
            .path
            .voice(id)
            .phase_policy_revision
            .map(|(revision, _)| revision)
            == Some(original.revision())
    }));
    // Both parent publications precede any callback or phase effect. The final
    // SourceSyncRequest is bitwise equal to the original, while the actual raw
    // snapshot custody changed through OFF and ON and must be claimed anew.
    fixture.publish_parent(false, true, 2, 7, None);
    fixture.publish_parent(true, true, 2, 7, None);
    for id in 0..2 {
        assert!(
            fixture
                .path
                .voice(id)
                .source_playback
                .matches_exact(&clocks[id])
        );
        assert_eq!(fixture.path.voice(id).sync_request, requests[id]);
    }
    let current = request(&fixture.path);
    assert!(!Arc::ptr_eq(&original, &current));
    assert_ne!(current.revision(), original.revision());
    for (id, request) in requests.iter().enumerate() {
        assert_eq!(current.row(id).unwrap().request, *request);
    }
    assert_eq!(current.observation().unwrap().state, "pending");
    fixture.drain();
    fixture.path.render(0);
    assert_eq!(current.observation().unwrap().state, "applied");
    assert_eq!(current.observation().unwrap().execution_frame, Some(23));
    for id in 0..2 {
        assert_eq!(
            fixture
                .path
                .voice(id)
                .phase_policy_revision
                .map(|(revision, _)| revision),
            Some(current.revision())
        );
        assert!(
            fixture
                .path
                .voice(id)
                .source_playback
                .matches_exact(&clocks[id])
        );
    }
    let attempt = current.observation().unwrap().attempt;
    assert!(Arc::ptr_eq(&current, &fixture.effect()));
    assert_eq!(current.observation().unwrap().attempt, attempt);
}

#[test]
fn static_same_period_origin_refresh_invalidates_only_a_real_grid_change() {
    let mut fixture = StaticPath::with_pause(&[2048]);
    fixture.install();
    assert!(!fixture.path.voice(0).source_playback.static_phase_settled());
    assert_ne!(
        fixture.path.voice(0).source_playback.tempo_ratio(),
        fixture.path.voice(0).source_playback.rate_target()
    );
    let before = fixture.path.voice(0).source_playback;
    fixture
        .path
        .source
        .engine
        .set_pad_timing_metadata(0, STATIC_ORIGIN / f64::from(RATE))
        .unwrap();
    fixture.drain();
    assert!(fixture.path.voice(0).source_playback.matches_exact(&before));
    fixture
        .path
        .source
        .engine
        .set_pad_bpm(0, Some(45_000.0))
        .unwrap();
    fixture.drain();
    assert!(fixture.path.voice(0).source_playback.matches_exact(&before));
    fixture
        .path
        .source
        .engine
        .set_pad_timing_metadata(0, -13.0 / f64::from(RATE))
        .unwrap();
    fixture.drain();
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    assert_eq!(
        fixture.path.voice(0).source_playback.position(),
        before.position()
    );
    fixture.publish(true, 3, 7, None);
    let snapshot = fixture.effect();
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Busy)
    );
    assert_eq!(
        fixture.path.voice(0).source_playback.position(),
        before.position()
    );
    fixture.path.render(1);
    verify_first_resumed_read(&fixture, before.position());
    let actual_first = fixture.path.timeline.output_frame();
    let after_first_read = fixture.path.voice(0).source_playback.position();
    assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
    assert_eq!(
        fixture.path.voice(0).source_playback.static_first_frame(),
        Some(actual_first)
    );
    assert_eq!(
        fixture.path.voice(0).source_playback.position(),
        after_first_read
    );
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .static_grid_matches(STATIC_F, -13.0, 32.0)
    );
}

#[test]
fn static_parent_intent_aba_cannot_reuse_a_preceding_program_or_pending_capture() {
    let mut fixture = StaticPath::with_pause(&[2048]);
    fixture.install();
    assert!(!fixture.path.voice(0).source_playback.static_phase_settled());
    assert_ne!(
        fixture.path.voice(0).source_playback.tempo_ratio(),
        fixture.path.voice(0).source_playback.rate_target()
    );
    let old = fixture.path.voice(0).source_playback;
    let future = fixture
        .path
        .source
        .engine
        .capture_pad_start(
            0,
            STATIC_START as f64 / f64::from(RATE),
            Some((STATIC_START + 2048) as f64 / f64::from(RATE)),
            1.0,
            false,
            None,
            None,
        )
        .unwrap();
    fixture.drain();
    fixture.publish(false, 3, 8, None);
    fixture.effect();
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    assert_eq!(
        fixture.path.voice(0).source_playback.position(),
        old.position()
    );
    fixture.publish(true, 4, 9, None);
    assert!(
        fixture
            .path
            .source
            .engine
            .publish_pad_start(&future, None)
            .is_err()
    );
    let snapshot = fixture.effect();
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Busy)
    );
    assert_eq!(
        fixture.path.voice(0).source_playback.position(),
        old.position()
    );
    fixture.path.render(1);
    verify_first_resumed_read(&fixture, old.position());
    let actual_first = fixture.path.timeline.output_frame();
    let after_first_read = fixture.path.voice(0).source_playback.position();
    assert!(Arc::ptr_eq(&snapshot, &fixture.effect()));
    let new = fixture.path.voice(0).source_playback;
    assert!(new.is_static_phase());
    assert_eq!(new.static_first_frame(), Some(actual_first));
    assert_eq!(new.position(), after_first_read);
    assert!(!new.matches_exact(&old));
    assert_eq!(fixture.path.voice(0).sync_request.global_revision, Some(4));
    assert_eq!(fixture.path.voice(0).sync_request.intent_revision, 9);
}

#[test]
fn static_actual_read_history_keeps_its_clock_and_cannot_certify_constant_unity_after_capture() {
    let mut fixture = StaticPath::with_pause(&[2048]);
    fixture.install();
    fixture.path.render(2048);
    let voice = fixture.path.voice(0);
    assert!(voice.source_playback.static_phase_settled());
    assert_eq!(voice.source_playback.tempo_ratio(), 1.0);
    assert!(!voice.source_playback.has_constant_scalar_trajectory());
    assert!(voice.source_playback.history_before_constant(17).is_none());
    assert!(voice.source_playback.shifted_normal_history(0.0).is_none());
    let mut history = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut history));
    let span = history
        .spans()
        .find(|span| span.playback.is_static_phase())
        .unwrap();
    let checkpoint = SourceReadCheckpoint {
        playback: span.playback,
        plan: span.plan,
    };
    assert!(checkpoint.is_consistent());
    let mut epoch = SourceControlEpoch::default();
    epoch.record(Some(48), span.frames, checkpoint, span.context, 1);
    assert!(!epoch.covers(49, 48 + span.frames as u64, checkpoint, span.context, 1));
    let mut advanced = checkpoint;
    assert!(
        !advanced.project_output_frames(span.frames as u64),
        "Native constant-history projection must refuse the static clock"
    );
    assert!(!epoch.covers(49, 48 + span.frames as u64, advanced, span.context, 1));
}

#[test]
fn static_full_and_checked_readers_reject_wrong_scalar_domain_or_layout_before_output() {
    let mut fixture = StaticPath::new(&[2048]);
    fixture.install();
    fixture.path.render(17);
    let voice = fixture.path.voice(0);
    let sample = voice.sample.as_ref().unwrap();
    let mut history = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut history));
    let span = history
        .spans()
        .find(|span| span.playback.is_static_phase())
        .unwrap();
    for bad in [
        SourceReadPlan {
            channels: 1,
            ..span.plan
        },
        SourceReadPlan {
            sample_frames: span.plan.sample_frames - 1,
            ..span.plan
        },
        SourceReadPlan {
            loop_region: FrameRange {
                start: STATIC_START,
                end: STATIC_START + 2047,
            },
            ..span.plan
        },
        SourceReadPlan {
            loop_period: crate::audio_engine::source_reader::SourceReaderPeriod::ScalarMusical(
                2048.0_f64.to_bits(),
            ),
            ..span.plan
        },
    ] {
        for checked in [false, true] {
            let mut buffers = [vec![17.0; span.frames], vec![19.0; span.frames]];
            let before = buffers.clone();
            let result = if checked {
                bad.fill_fractional_buffers_checked_with_readers(
                    sample,
                    StemReadSets::default(),
                    &span.playback,
                    &mut buffers,
                    span.frames,
                )
            } else {
                bad.fill_fractional_buffers_with_readers(
                    sample,
                    StemReadSets::default(),
                    &span.playback,
                    &mut buffers,
                    span.frames,
                )
            };
            assert!(result.is_err());
            assert_eq!(buffers, before);
        }
    }
    // Coordinate-only late copy: its original resident PCM/reader are genuine,
    // but a two-frame query would overflow the static intended output address.
    // Neither full nor checked rendering may write the first sample first.
    let mut late_master = TransportTimeline::new(RATE);
    assert!(late_master.set_master_period(STATIC_PERIOD));
    let mut raw = SourcePlayback::scalar_continuation(
        FractionalSourcePosition {
            frame: STATIC_START,
            fraction: 0.0,
            seek_mode: ExplicitSeekMode::Normal,
        },
        span.plan.domain(),
        1.0,
        1.0,
    )
    .unwrap();
    raw.install_launch_reference(LoopPhaseReference::capture(Some(late_master), u64::MAX - 1));
    let late = SourcePlayback::static_continuation(
        &raw,
        late_master,
        u64::MAX - 1,
        77,
        STATIC_F,
        STATIC_START as f64,
        32.0,
    )
    .unwrap();
    assert!(late.position_at(2).is_none());
    let late_plan = SourceReadPlan {
        frame_pos: STATIC_START,
        ..span.plan
    };
    for checked in [false, true] {
        let mut buffers = [vec![17.0; 2], vec![19.0; 2]];
        let before = buffers.clone();
        let result = if checked {
            late_plan.fill_fractional_buffers_checked_with_readers(
                sample,
                StemReadSets::default(),
                &late,
                &mut buffers,
                2,
            )
        } else {
            late_plan.fill_fractional_buffers_with_readers(
                sample,
                StemReadSets::default(),
                &late,
                &mut buffers,
                2,
            )
        };
        assert!(result.is_err());
        assert_eq!(buffers, before);
    }
}

#[test]
fn static_competing_original_global_lease_keeps_phase_work_pending_without_clock_mutation() {
    let (mut fixture, snapshot) = pending_phase(2, false);
    // Both original control/parameter drains complete before the competing
    // publisher takes its lease; otherwise the earlier Busy guard, rather than
    // final lease contention, is the actual refusal predicate.
    fixture.drain();
    let before = scalar_state(&fixture.path);
    let original = fixture.path.timeline;
    let ownership = fixture.path.source.engine.input_runtime_ownership.clone();
    let lease =
        crate::audio_engine::GlobalTimingLease::acquire(&ownership.global_timing_publishing)
            .unwrap();
    fixture.path.render(0);
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    let ScheduledCommand::AdoptRuntimePolicy(work) =
        &fixture.path.scheduler.deferred_due().unwrap().command
    else {
        panic!("the exact original static policy work must remain pending");
    };
    assert!(std::ptr::eq(&**work, snapshot.as_ref()));
    unchanged(&fixture.path, &before);
    assert!(fixture.path.timeline.matches_clock_epoch(&original));
    let attempt = snapshot.observation().unwrap().attempt;
    drop(lease);
    // Deferred Work retries only at a later callback frame. Read that real
    // preceding scalar frame before the same original attempt can be claimed.
    let mut advanced = before.clone();
    for (_, _, clock, _) in &mut advanced {
        assert!(clock.advance(1));
    }
    let mut advanced_master = original;
    advanced_master.advance_by_rendered_frames(1);
    fixture.path.render(1);
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    assert_eq!(snapshot.observation().unwrap().attempt, attempt);
    unchanged(&fixture.path, &advanced);
    assert!(fixture.path.timeline.matches_clock_epoch(&advanced_master));
    let ScheduledCommand::AdoptRuntimePolicy(work) =
        &fixture.path.scheduler.deferred_due().unwrap().command
    else {
        panic!("the exact original static policy work must remain deferred");
    };
    assert!(std::ptr::eq(&**work, snapshot.as_ref()));
    // Complete the original callback drains before retrying at E+1; submit no
    // replacement request and preserve the actual live first source pair.
    fixture.drain();
    fixture.path.render(0);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert_eq!(snapshot.observation().unwrap().attempt, attempt);
    for (id, _, preceding, _) in &advanced {
        let clock = fixture.path.voice(*id).source_playback;
        assert!(clock.is_static_phase());
        assert_eq!(
            clock.static_first_frame(),
            Some(advanced_master.output_frame())
        );
        assert_eq!(clock.position().frame, preceding.position().frame);
        assert_eq!(
            clock.position().fraction.to_bits(),
            preceding.position().fraction.to_bits()
        );
        assert_eq!(clock.position().seek_mode, preceding.position().seek_mode);
    }
    assert!(fixture.path.timeline.matches_clock_epoch(&advanced_master));
}

#[test]
fn static_nonclosing_neighbor_cannot_block_a_new_pad_optout_ordinary_mode_effect() {
    let mut fixture = StaticPath::new(&[2047, 3072]);
    fixture
        .path
        .source
        .engine
        .set_pad_bpm(1, Some(30_000.0))
        .unwrap();
    fixture.drain();
    let master = fixture.path.timeline;
    fixture.publish(true, 2, 7, None);
    let fixed = fixture.effect();
    assert_eq!(
        fixed.observation().unwrap().state,
        "applied",
        "{:?}",
        fixed.observation()
    );
    assert_eq!(fixed.row(0).unwrap().request.mode, SourceSyncMode::Fixed);
    assert_eq!(fixed.row(1).unwrap().request.mode, SourceSyncMode::Fixed);
    assert!((0..2).all(|id| !fixture.path.voice(id).source_playback.is_static_phase()));
    assert_eq!(fixture.path.voice(1).source_playback.rate_target(), 1.5);
    fixture.path.render(6000);
    assert_eq!(fixture.path.voice(1).source_playback.tempo_ratio(), 1.5);
    assert!(Arc::ptr_eq(&fixed, &fixture.effect()));
    assert_eq!(
        fixed.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    assert!((0..2).all(|id| !fixture.path.voice(id).source_playback.is_static_phase()));
    let before = scalar_state(&fixture.path);
    let request0 = fixture.path.voice(0).sync_request;
    let epoch = fixture.path.timeline;
    let frame = fixture.path.timeline.output_frame();
    // Realtime is globally static. Only PAD1 opts out; PAD0 retains exactly its
    // preceding Fixed raw request, including global and per-content revisions.
    fixture.publish_parent_with_optout(true, true, 2, 7, None, Some((1, 8)));
    let optout = fixture.effect();
    assert_eq!(
        optout.observation().unwrap().state,
        "applied",
        "{:?}",
        optout.observation()
    );
    assert_eq!(optout.observation().unwrap().execution_frame, Some(frame));
    assert_eq!(optout.row(0).unwrap().request, request0);
    assert_eq!(optout.row(1).unwrap().request.mode, SourceSyncMode::Legacy);
    assert_eq!(fixture.path.voice(0).sync_request, request0);
    assert_eq!(
        fixture.path.voice(1).sync_request,
        optout.row(1).unwrap().request
    );
    assert_eq!(fixture.path.voice(1).sync_request.intent_revision, 8);
    for (id, generation, clock, _) in &before {
        assert_eq!(fixture.path.voice(*id).generation, *generation);
        assert!(fixture.path.voice(*id).source_playback.matches_exact(clock));
        assert!(!fixture.path.voice(*id).source_playback.is_static_phase());
    }
    assert_eq!(fixture.path.voice(1).source_playback.rate_target(), 1.5);
    assert!(fixture.path.timeline.matches_clock_epoch(&epoch));
    assert!(fixture.path.timeline.matches_clock_epoch(&master));
    assert_eq!(fixture.path.timeline.output_frame(), frame);
    let adopted = scalar_state(&fixture.path);
    assert!(Arc::ptr_eq(&optout, &fixture.effect()));
    assert_eq!(
        optout.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&fixture.path, &adopted);
    assert_eq!(
        fixture.path.voice(1).sync_request.mode,
        SourceSyncMode::Legacy
    );
    assert!((0..2).all(|id| !fixture.path.voice(id).source_playback.is_static_phase()));
}

#[test]
fn static_failed_global_master_admission_preserves_the_complete_original_clock() {
    let mut fixture = StaticPath::new(&[2048]);
    fixture.install();
    fixture.path.render(19);
    let before = fixture.path.voice(0).source_playback;
    let original = fixture.path.timeline;
    let ownership = fixture.path.source.engine.input_runtime_ownership.clone();
    let lease =
        crate::audio_engine::GlobalTimingLease::acquire(&ownership.global_timing_publishing)
            .unwrap();
    assert!(
        fixture
            .path
            .source
            .engine
            .set_speed_and_master_period(1.25, STATIC_PERIOD * 2.0)
            .is_err()
    );
    drop(lease);
    fixture.drain();
    assert!(fixture.path.voice(0).source_playback.matches_exact(&before));
    assert!(fixture.path.timeline.matches_clock_epoch(&original));
}

#[test]
fn static_terminal_key_lock_and_pitch_refusals_leave_a_working_sync_off_escape() {
    let mut fixture = StaticPath::new(&[2048]);
    fixture.install();
    fixture.path.render(2048);
    let before = fixture.path.voice(0).source_playback;
    let revision = fixture
        .path
        .source
        .engine
        .set_pad_key_lock(0, true)
        .unwrap();
    fixture.drain();
    let status = Python::attach(|py| {
        let status = fixture
            .path
            .source
            .engine
            .pad_key_lock_status(py, 0)
            .unwrap()
            .unwrap();
        let status = status.bind(py);
        (
            status
                .get_item("request_id")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            status
                .get_item("state")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            status
                .get_item("effective")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap(),
        )
    });
    assert_eq!(status, (revision, "error".into(), false));
    assert!(fixture.path.voice(0).source_playback.matches_exact(&before));
    for action in ["pitch.extra:1:set", "pitch.extra:0:set_retrigger"] {
        let captured = fixture.capture_pitch(action);
        assert!(
            fixture
                .path
                .source
                .engine
                .publish_pitch_action(&captured)
                .unwrap()
        );
        fixture.drain();
        fixture.path.render(0);
        assert_eq!(captured.state(), "rejected", "{:?}", captured.reason());
        assert_eq!(captured.reason(), Some("mapped_timing_unavailable"));
        assert!(fixture.path.voice(0).source_playback.matches_exact(&before));
    }
    // The requested Key Lock preference remains on, but its actual unsupported
    // operation has already terminally refused through the existing owner.
    fixture.publish(false, 3, 8, Some(0));
    let off = fixture.effect();
    assert_eq!(
        off.observation().unwrap().state,
        "applied",
        "{:?}",
        off.observation()
    );
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    assert_eq!(
        fixture.path.voice(0).source_playback.position(),
        before.position()
    );
    fixture.publish(false, 4, 9, None);
    fixture.effect();
    fixture
        .path
        .source
        .engine
        .set_pad_key_lock(0, false)
        .unwrap();
    fixture.drain();
    let birth = fixture.start(0);
    assert_eq!(birth.state(), "applied", "{:?}", birth.reason());
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
}

#[test]
fn static_original_source_write_keeps_the_frozen_voice_when_new_bank_metadata_changes() {
    use std::sync::atomic::Ordering;
    let mut fixture = StaticPath::with_pause(&[2048]);
    fixture.install();
    fixture.path.render(17);
    let old_clock = fixture.path.voice(0).source_playback;
    let old_source = fixture.path.voice(0).sample.as_ref().unwrap().clone();
    let old_generation = fixture
        .path
        .source
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap()
        .source_generation;
    // Only this fixture's generated external WAV changes. Decode and acknowledge
    // its new PCM in the inactive second slot, then exercise the existing original
    // source-write publication primitive for slot zero without IncomingDefaults.
    // A direct Cold replacement of slot zero deliberately restarts it and is the
    // wrong fixture for retained-voice bank replacement (initial failure retained).
    let external = fixture.path.source.directory.path().join("external.wav");
    let mut bytes = std::fs::read(&external).unwrap();
    for (index, sample) in bytes[44..].chunks_exact_mut(2).enumerate() {
        let value = ((index * 41 % 24_001) as i32 - 12_000) as i16;
        sample.copy_from_slice(&value.to_le_bytes());
    }
    std::fs::write(&external, bytes).unwrap();
    let request_id = crate::audio_engine::cold_load::admit_for_format_selected(
        &fixture.path.source.engine,
        1,
        external.to_string_lossy().into(),
        (false, false, true, None),
        fixture.path.source.producer.clone(),
        (
            2,
            RATE,
            fixture.path.source.directory.path().join("samples"),
        ),
    )
    .unwrap();
    wait_for(|| {
        fixture
            .path
            .source
            .engine
            .input_runtime_ownership
            .cold_status(1, request_id)
            == Some(0)
            && !fixture.path.source.consumer.is_empty()
    });
    fixture.drain();
    assert_eq!(
        fixture
            .path
            .source
            .engine
            .input_runtime_ownership
            .cold_status(1, request_id),
        Some(2)
    );
    wait_for(|| fixture.path.source.engine.cold_loading[1].load(Ordering::Acquire) == 0);
    let bank = fixture.path.source.engine.sample_cache.lock().unwrap()[1]
        .as_ref()
        .unwrap()
        .clone();
    assert!(!bank.same_source(&old_source));
    let ownership = fixture.path.source.engine.input_runtime_ownership.clone();
    let before = ownership.slot_snapshot(0).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(
            before,
            crate::audio_engine::content_lifetime::reserve().unwrap(),
        )
        .unwrap();
    let generation = request_id;
    let publication = ownership
        .publish_source_write(
            &write,
            &bank,
            RATE,
            generation,
            ownership.next_authority(0).unwrap(),
            Some(flitzis_looper_analysis::tempo_acceptance::TimingIntent::Manual),
            false,
            || true,
        )
        .unwrap();
    let digest = fixture
        .path
        .source
        .engine
        .loaded_source_digests
        .lock()
        .unwrap()[1]
        .clone();
    fixture.path.source.engine.sample_cache.lock().unwrap()[0] = Some(bank.clone());
    fixture.path.source.engine.pad_request_ids.lock().unwrap()[0] = generation;
    fixture
        .path
        .source
        .engine
        .loaded_source_generations
        .lock()
        .unwrap()[0] = (generation, RATE);
    fixture
        .path
        .source
        .engine
        .loaded_source_digests
        .lock()
        .unwrap()[0] = digest;
    fixture.path.source.engine.timing_intents.lock().unwrap()[0] =
        flitzis_looper_analysis::tempo_acceptance::TimingIntent::Manual;
    fixture
        .path
        .source
        .producer
        .lock()
        .unwrap()
        .push(crate::messages::ControlMessage::LoadSample {
            id: 0,
            sample: bank.clone(),
            publication: Some(publication),
        })
        .unwrap();
    fixture.drain();
    assert_eq!(
        ownership.callback_source_generation(0, &bank, RATE),
        Some(generation)
    );
    assert_eq!(
        ownership.callback_feedback_key(0),
        ownership.slot_snapshot(0).unwrap().content_key()
    );
    assert!(ownership.source_current(0, &bank, RATE));
    assert!(
        fixture
            .path
            .voice(0)
            .sample
            .as_ref()
            .unwrap()
            .same_window(&old_source)
    );
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .matches_exact(&old_clock)
    );
    let new_binding = fixture
        .path
        .source
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap();
    assert_ne!(new_binding.source_generation, old_generation);
    fixture
        .path
        .source
        .engine
        .set_pad_bpm(0, Some(45_000.0))
        .unwrap();
    fixture
        .path
        .source
        .engine
        .set_pad_timing_metadata(0, -37.0 / f64::from(RATE))
        .unwrap();
    fixture.drain();
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .matches_exact(&old_clock)
    );
    assert!(fixture.path.voice(0).source_playback.static_grid_matches(
        STATIC_F,
        STATIC_ORIGIN,
        32.0
    ));
    fixture.publish(true, 3, 9, None);
    let attempt = fixture.effect();
    assert!(matches!(
        attempt.observation().unwrap().reason,
        Some(RuntimePolicyError::SourceChanged | RuntimePolicyError::Stale)
    ));
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .matches_exact(&old_clock)
    );
    fixture.path.render(73);
    let mut expected = old_clock;
    assert!(expected.advance(73));
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .matches_exact(&expected)
    );
    let actual = fixture.path.feedback(0);
    assert_ne!(actual["phase_program_current"], json!(true));
    assert_eq!(actual["effective"], json!(false));
    assert_eq!(verify_dense_actual_reads(&fixture, 0), 90);
}

#[test]
fn static_blocked_pitch_set_retrigger_can_prepare_and_apply_after_real_sync_off() {
    let mut fixture = StaticPath::new(&[2048]);
    fixture.install();
    fixture.path.render(2048);
    let before = fixture.path.voice(0).source_playback;
    let blocked = fixture.capture_pitch("pitch.extra:1:set_retrigger");
    assert!(
        fixture
            .path
            .source
            .engine
            .publish_pitch_action(&blocked)
            .unwrap()
    );
    fixture.drain();
    fixture.path.render(0);
    assert_eq!(blocked.reason(), Some("mapped_timing_unavailable"));
    assert!(fixture.path.voice(0).source_playback.matches_exact(&before));
    fixture.publish(false, 3, 8, None);
    assert_eq!(fixture.effect().observation().unwrap().state, "applied");
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    // Observe genuine new scalar history before requesting Native work again;
    // no past static trajectory is relabelled as constant history.
    fixture.path.render(16_384);
    let action = fixture.capture_pitch("pitch.extra:1:set_retrigger");
    assert!(
        fixture
            .path
            .source
            .engine
            .publish_pitch_action(&action)
            .unwrap()
    );
    fixture.drain();
    let frame = fixture.path.timeline.output_frame();
    wait_for(|| {
        fixture
            .path
            .source
            .mixer
            .pitch_runtime
            .as_mut()
            .unwrap()
            .poll();
        assert_ne!(action.state(), "rejected", "{:?}", action.reason());
        fixture
            .path
            .source
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(action.sequence())
    });
    assert_eq!(fixture.path.timeline.output_frame(), frame);
    while fixture.path.timeline.output_frame() < action.execution_target() {
        fixture.path.render(
            (action.execution_target() - fixture.path.timeline.output_frame()).min(512) as usize,
        );
    }
    fixture.path.render(1);
    assert_eq!(action.state(), "applied", "{:?}", action.reason());
    assert!(action.effective());
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    assert_ne!(fixture.path.voice(0).stretch.native_state_address(), 0);
    let output = fixture.path.render(4096);
    assert!(output.iter().all(|value| value.is_finite()));
    assert!(output.iter().any(|value| *value != 0.0));
}

fn accepted_musical_static() -> (StaticPath, f64, f64, f64, f64) {
    use crate::audio_engine::constant_timing::l3_constant_timing_fixture;
    let mut fixture = StaticPath::new(&[1500]);
    fixture.path.source.engine.stop_sample(0).unwrap();
    fixture.drain();
    fixture
        .path
        .source
        .engine
        .set_pad_timing_intent(0, "automatic")
        .unwrap();
    let timing = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
        &fixture.path.source.engine,
        &fixture.path.source.producer,
        0,
        24_004.0 / f64::from(RATE),
    );
    fixture.drain();
    assert_eq!(timing.publication_status().unwrap(), "accepted");
    let (period, origin) = Python::attach(|py| {
        let metadata = fixture
            .path
            .source
            .engine
            .current_constant_timing(py, 0)
            .unwrap()
            .unwrap();
        let metadata = metadata.bind(py).cast::<PyDict>().unwrap();
        (
            metadata
                .get_item("period_seconds_per_quarter")
                .unwrap()
                .unwrap()
                .extract::<f64>()
                .unwrap(),
            metadata
                .get_item("origin_seconds")
                .unwrap()
                .unwrap()
                .extract::<f64>()
                .unwrap(),
        )
    });
    fixture.path.source.mixer.set_master_period(period);
    assert!(fixture.path.timeline.set_master_period(period));
    fixture.publish(false, 1, 7, None);
    assert_eq!(fixture.path.timeline.output_frame(), 23);
    let launch_master = fixture.path.timeline.beat_position_at_frame(23).unwrap();
    let birth = fixture.start(0);
    assert_eq!(birth.state(), "applied", "{:?}", birth.reason());
    let actual = fixture.path.voice(0).source_timing.accepted.unwrap();
    assert_eq!(actual.period_seconds.to_bits(), period.to_bits());
    assert_eq!(actual.origin_seconds.to_bits(), origin.to_bits());
    let f = period * f64::from(RATE);
    let o = origin * f64::from(RATE);
    let p = fixture
        .path
        .voice(0)
        .source_playback
        .reader_period()
        .scalar_period()
        .unwrap();
    assert_ne!(p, fixture.lengths[0] as f64);
    assert!((p - 1500.0).abs() <= 1.0);
    assert_eq!((f / 16.0).to_bits(), p.to_bits());
    fixture.path.render(23);
    fixture.publish(true, 2, 7, None);
    let first = fixture.effect();
    assert_eq!(first.observation().unwrap().state, "applied");
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    let second = fixture.effect();
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(
        second.observation().unwrap().state,
        "applied",
        "{:?}",
        second.observation()
    );
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .static_grid_matches(f, o, 1.0 / 16.0)
    );
    (fixture, f, o, p, launch_master)
}

#[allow(clippy::too_many_arguments)] // Keep the independent original launch and reader inputs explicit.
fn verify_musical_static_reads(
    fixture: &StaticPath,
    f: f64,
    o: f64,
    p: f64,
    launch_master: f64,
    initial: FractionalSourcePosition,
    first: u64,
    master: TransportTimeline,
) -> (usize, usize) {
    use pyo3::types::PyModule;
    use std::ffi::CString;
    let voice = fixture.path.voice(0);
    let sample = voice.sample.as_ref().unwrap();
    let mut history = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut history));
    Python::attach(|py| {
        let code = CString::new(MUSICAL_FRACTION_ORACLE).unwrap();
        let module = PyModule::from_code(
            py,
            &code,
            c"static_phase_fraction_oracle.py",
            c"static_phase_fraction_oracle",
        )
        .unwrap();
        let m0 = master.beat_position_at_frame(first).unwrap();
        let mut read_frames = 0;
        let mut seam_frames = 0;
        let mut rounded_knot_seen = false;
        let mut rounded_period_seen = false;
        for span in history
            .spans()
            .filter(|span| span.playback.is_static_phase())
        {
            let output = span.playback.static_output_frame().unwrap();
            let masters = (0..=span.frames)
                .map(|offset| {
                    master
                        .beat_position_at_frame(output + offset as u64)
                        .unwrap()
                })
                .collect::<Vec<_>>();
            let expected = module
                .getattr("musical_trace")
                .unwrap()
                .call1((
                    f,
                    o,
                    p,
                    STATIC_START,
                    fixture.lengths[0],
                    initial.frame,
                    initial.fraction,
                    launch_master,
                    m0,
                    1.0,
                    masters,
                ))
                .unwrap()
                .extract::<Vec<(usize, u64, usize, Option<usize>, u32, bool)>>()
                .unwrap();
            let mut expected_pcm = [vec![0.0; span.frames], vec![0.0; span.frames]];
            for (offset, (whole, fraction_bits, left, right, gain_bits, captured)) in
                expected.into_iter().enumerate()
            {
                let position = span.playback.position_at(offset).unwrap();
                assert_eq!(
                    (position.frame, position.fraction.to_bits()),
                    (whole, fraction_bits)
                );
                let plan = SourceReadPlan {
                    frame_pos: position.frame,
                    ..span.plan
                };
                let taps = plan.try_fractional_taps(position.fraction).unwrap();
                assert_eq!(
                    (taps.left, taps.right, taps.right_gain.to_bits()),
                    (left, right, gain_bits),
                    "n={}",
                    output + offset as u64
                );
                if output + offset as u64 == 1522 {
                    // The exact split remains below the last knot. The existing
                    // ScalarMusical reader rounds its binary64 phase onto it;
                    // equal PCM at weights one/zero cannot prove tap topology.
                    assert_eq!(position.frame, 1511);
                    assert!(position.fraction > 0.0 && position.fraction < 1.0);
                    assert_eq!(
                        (position.frame - STATIC_START) as f64 + position.fraction,
                        1499.0
                    );
                    assert_eq!(
                        (left, right, gain_bits, captured),
                        (1512, Some(13), 0, true)
                    );
                    rounded_knot_seen = true;
                }
                if output + offset as u64 == 6024 {
                    // Rounding onto virtual P skips the seam branch; the
                    // ordinary reader then wraps both physical integer taps.
                    assert_eq!(position.frame, 1513);
                    assert!(position.fraction > 0.0 && position.fraction < 0.25);
                    let phase = (position.frame - STATIC_START) as f64 + position.fraction;
                    assert_eq!(phase, p);
                    assert_eq!(phase.rem_euclid(p), 0.0);
                    assert_eq!(
                        (left, right, gain_bits, captured),
                        (13, Some(14), 0x3e80_0000, true)
                    );
                    rounded_period_seen = true;
                }
                if offset == span.frames {
                    let mut end = span.playback;
                    assert!(end.advance(offset));
                    assert_eq!(end.static_phase_settled(), captured);
                } else {
                    if left == STATIC_START + fixture.lengths[0] - 1 && right == Some(STATIC_START)
                    {
                        seam_frames += 1;
                    }
                    for channel in 0..2 {
                        let a = sample.samples[(left - sample.resident_start()) * 2 + channel];
                        expected_pcm[channel][offset] = right.map_or(a, |frame| {
                            let b = sample.samples[(frame - sample.resident_start()) * 2 + channel];
                            a + (b - a) * f32::from_bits(gain_bits)
                        });
                    }
                }
            }
            for checked in [false, true] {
                let mut actual = [vec![17.0; span.frames], vec![19.0; span.frames]];
                let result = if checked {
                    span.plan.fill_fractional_buffers_checked_with_readers(
                        sample,
                        StemReadSets::default(),
                        &span.playback,
                        &mut actual,
                        span.frames,
                    )
                } else {
                    span.plan.fill_fractional_buffers_with_readers(
                        sample,
                        StemReadSets::default(),
                        &span.playback,
                        &mut actual,
                        span.frames,
                    )
                };
                result.unwrap();
                assert_eq!(actual, expected_pcm);
            }
            read_frames += span.frames;
        }
        assert!(rounded_knot_seen);
        assert!(rounded_period_seen);
        (read_frames, seam_frames)
    })
}

#[test]
fn static_actual_accepted_scalar_musical_period_keeps_virtual_seam_pcm_capture_and_dissolution() {
    let mut results = Vec::new();
    for partitions in [&[512][..], &[31, 257, 1, 96, 777][..]] {
        let (mut fixture, f, o, p, launch_master) = accepted_musical_static();
        let first = fixture.path.timeline.output_frame();
        let master = fixture.path.timeline;
        let initial = fixture.path.voice(0).source_playback.position();
        let domain = fixture
            .path
            .voice(0)
            .source_playback
            .effective_domain()
            .unwrap();
        assert_eq!(domain.musical_period.scalar_period(), Some(p));
        let mut remaining = 65_536;
        let mut part = 0;
        let mut output = Vec::with_capacity(remaining * 2);
        while remaining > 0 {
            let frames = partitions[part % partitions.len()].min(remaining);
            output.extend(fixture.path.render(frames));
            part += 1;
            remaining -= frames;
        }
        let (reads, seam_reads) =
            verify_musical_static_reads(&fixture, f, o, p, launch_master, initial, first, master);
        assert_eq!(reads, 65_536);
        assert!(seam_reads > 0);
        assert!(fixture.path.voice(0).source_playback.static_phase_settled());
        assert!(fixture.path.timeline.matches_clock_epoch(&master));
        let raw = fixture.path.voice(0).source_playback.position();
        fixture.publish(false, 3, 8, None);
        let off = fixture.effect();
        assert_eq!(
            off.observation().unwrap().state,
            "applied",
            "{:?}",
            off.observation()
        );
        assert!(!fixture.path.voice(0).source_playback.is_static_phase());
        assert_eq!(
            (
                fixture.path.voice(0).source_playback.position().frame,
                fixture
                    .path
                    .voice(0)
                    .source_playback
                    .position()
                    .fraction
                    .to_bits()
            ),
            (raw.frame, raw.fraction.to_bits())
        );
        assert_eq!(
            fixture.path.voice(0).source_playback.effective_domain(),
            Some(domain)
        );
        results.push((output, raw.frame, raw.fraction.to_bits(), p.to_bits()));
    }
    assert_eq!(results[0], results[1]);
}

struct StaticRetirement {
    work: Vec<crate::audio_engine::input_mapping::RuntimePolicyWork>,
}
impl StaticRetirement {
    fn new() -> Self {
        Self {
            work: Vec::with_capacity(64),
        }
    }
}
impl AudioBufferRetirement for StaticRetirement {
    fn retire_runtime_policy(
        &mut self,
        work: crate::audio_engine::input_mapping::RuntimePolicyWork,
    ) {
        assert!(self.work.len() < self.work.capacity());
        self.work.push(work);
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.work.capacity() - self.work.len()
    }
    fn retire_resident_capture(
        &mut self,
        _: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
        panic!("unexpected resident retirement")
    }
    fn retire_resident_cancellation(&mut self, _: Arc<std::sync::atomic::AtomicBool>) {
        panic!("unexpected resident cancellation")
    }
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {
        panic!("unexpected resident transaction")
    }
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {
        panic!("unexpected cold adoption")
    }
    fn retire_sample(&mut self, _: crate::messages::SampleBuffer) {
        panic!("unexpected sample retirement")
    }
    fn retire_prepared_stems(&mut self, _: crate::messages::PreparedStemSet) {
        panic!("unexpected stem retirement")
    }
    fn retire_constant_timing(
        &mut self,
        _: crate::audio_engine::constant_timing::PreparedConstantTiming,
    ) {
        panic!("unexpected timing retirement")
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        panic!("unexpected global retirement")
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        panic!("unexpected refresh retirement")
    }
}

fn pending_phase(lanes: usize, correcting: bool) -> (StaticPath, Arc<RuntimePolicySnapshot>) {
    let mut fixture = if correcting {
        StaticPath::with_pause(&vec![2048; lanes])
    } else {
        StaticPath::new(&vec![2048; lanes])
    };
    fixture.publish(true, 2, 7, None);
    let snapshot = fixture.effect();
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    assert!(Arc::ptr_eq(&snapshot, &request(&fixture.path)));
    (fixture, snapshot)
}

fn drain_and_render_static(
    fixture: &mut StaticPath,
    output: &mut [f32],
    retired: &mut StaticRetirement,
) {
    drain_control_messages(
        &mut fixture.path.source.consumer,
        &mut fixture.path.scheduler,
        fixture.path.timeline.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut fixture.path.timeline,
        &mut fixture.path.source.mixer,
        &mut fixture.path.events,
        retired,
    );
    drain_parameter_messages(
        &mut fixture.parameters,
        &mut fixture.path.source.mixer,
        &mut fixture.path.timeline,
    );
    fixture.render_into(output, retired);
}

#[test]
fn static_productive_constructor_claim_and_renderer_fit_two_mib_without_rust_requests() {
    fn copy<T: Copy>() {}
    copy::<SourcePlayback>();
    assert!(!std::mem::needs_drop::<SourcePlayback>());
    assert!(std::mem::size_of::<[Option<SourcePlayback>; 32]>() <= 16 * 1024);
    assert!(
        std::mem::size_of::<crate::audio_engine::mapped_source_clock::MappedClockEpoch>() <= 152
    );
    assert!(
        std::mem::size_of::<crate::audio_engine::mapped_source_clock::MappedSourceSpan>() <= 264
    );
    let child = std::thread::Builder::new()
        .name("static-productive-two-mib".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let (mut fixture, snapshot) = pending_phase(6, true);
            let mut output = [0.0; 1024];
            let mut retired = StaticRetirement::new();
            let (_, allocations) = observe_current_thread(|| {
                drain_and_render_static(&mut fixture, &mut output, &mut retired)
            });
            assert_eq!(
                snapshot.observation().unwrap().state,
                "applied",
                "{:?}",
                snapshot.observation()
            );
            assert!((0..6).all(|id| fixture.path.voice(id).source_playback.is_static_phase()));
            assert!(output.iter().all(|sample| sample.is_finite()));
            assert!(output.iter().any(|sample| *sample != 0.0));
            allocations
        })
        .unwrap();
    assert_eq!(child.join().unwrap(), ThreadAllocations::default());
}

#[test]
#[ignore = "descriptive productive profile; Root runs serially after the frozen systems review"]
fn descriptive_static_actual_constructor_renderer_cohorts_report_cost_and_limits() {
    for lanes in [2, 4, 6, 10] {
        let mut elapsed = Vec::new();
        for repeat in 0..4 {
            let (mut fixture, snapshot) = pending_phase(lanes, true);
            let mut output = [0.0; 1024];
            let mut retired = StaticRetirement::new();
            let start = Instant::now();
            let (_, allocations) = observe_current_thread(|| {
                drain_and_render_static(&mut fixture, &mut output, &mut retired)
            });
            let seconds = start.elapsed().as_secs_f64();
            assert_eq!(allocations, ThreadAllocations::default());
            assert_eq!(snapshot.observation().unwrap().state, "applied");
            if repeat > 0 {
                elapsed.push(seconds * 1000.0);
            }
        }
        elapsed.sort_by(f64::total_cmp);
        let nominal = 512.0 / f64::from(RATE) * 1000.0;
        println!(
            "static_productive_profile active={lanes} fixed_capacity=32 frames_per_lane=512 warmups=1 repeats=3 median_ms={:.4} max_ms={:.4} nominal_ms={nominal:.7} within_nominal={}; includes original queue/drains, second actual-E claim, exact static construction, full dry renderer, history and copied advance; setup/output/retirement allocation precedes measurement; Rust requests observed zero; Native allocation/deallocation and device/music/worstcase acceptance unobserved; previous mapped performance negatives remain open",
            elapsed[1],
            elapsed[2],
            elapsed[2] <= nominal
        );
    }
}
// Launch-relative standard-library Fraction reference, version2. Historical
// absolute-origin oracle and results remain unchanged in their original block.
const MUSICAL_FRACTION_ORACLE: &str = r####""""Independent rational expectations for the declared static-phase contract.

No project module or Native library is imported. Source coordinates use Fraction,
not the production fixed-grid core. Physical reader topology and gains separately
follow the existing ScalarMusical binary64 operations after the exact source split.
Generated tuples preserve the defined binary64 Transport operation order; the Rust
test separately verifies those master bits against the actual TransportTimeline.
"""

import argparse
import json
import math
import struct
from fractions import Fraction as F
from pathlib import Path


def bits(value: float) -> int:
    return int(struct.unpack("<Q", struct.pack("<d", value))[0])


def master(frame: int, fpb: float, anchor_frame: int, anchor: float) -> float:
    return anchor + (float(frame) - float(anchor_frame)) / fpb


def choose(initial: F, start: int, period: F, nominal: F, mt: F, m0: F, fpb: F):
    residue = (F(start) + (m0 - mt) * fpb - initial) % period
    candidates = [residue] if residue == 0 else [residue, residue - period]
    candidates.sort(key=lambda value: (abs(value), value < 0))
    for debt in candidates:
        for shift in range(6, 65):
            step = nominal / (1 << shift)
            corrected = nominal + (step if debt > 0 else -step if debt < 0 else 0)
            if F(1, 2) <= corrected <= 2 and step <= F(1, 32):
                return debt, shift
    return None


def split(value: F, start: int, period: F, raw_fraction: float):
    value = start + (value - start) % period
    whole = value.numerator // value.denominator
    remainder = value - whole
    fraction = raw_fraction if remainder == F(raw_fraction) else float(remainder)
    if fraction == 1:
        whole += 1
        fraction = 0.0
    if F(whole) + F(fraction) >= start + period:
        whole, fraction = start, 0.0
    return whole, bits(fraction)


def rn32(value: F) -> int:
    """Round a rational interpolation weight directly, without double rounding."""
    assert 0 <= value <= 1
    if not value:
        return 0
    n, d = value.numerator, value.denominator
    exponent = n.bit_length() - d.bit_length()
    if exponent >= 0:
        if n < d << exponent:
            exponent -= 1
    elif n << -exponent < d:
        exponent -= 1
    scale = max(-149, exponent - 23)
    quotient, remainder = divmod(n << max(0, -scale), d << max(0, scale))
    denominator = d << max(0, scale)
    if 2 * remainder > denominator or (2 * remainder == denominator and quotient & 1):
        quotient += 1
    if scale == -149 and quotient < 1 << 23:
        return quotient
    if quotient >= 1 << 24:
        quotient >>= 1
        scale += 1
    return ((scale + 23 + 127) << 23) | (quotient - (1 << 23))


def musical_trace(source_fpb, origin, period, start, physical_length, raw_whole,
                  raw_fraction, launch_master, m0, nominal, masters):
    """Exact source circle, then the existing rounded physical last/first reader."""
    initial = F(raw_whole) + F(raw_fraction)
    # Origin is source-grid provenance; the target uses original selected A/T.
    debt, shift = choose(initial, start, F(period), F(nominal), F(launch_master), F(m0), F(source_fpb))
    last = min(-(-F(period).numerator // F(period).denominator) - 1, physical_length - 1)
    result = []
    for value in masters:
        progress = (F(value) - F(m0)) * F(source_fpb)
        assert progress >= 0
        correction = min(progress / (1 << shift), abs(debt))
        exact = initial + progress + (correction if debt > 0 else -correction)
        whole, fraction_bits = split(exact, start, F(period), raw_fraction)
        fraction = struct.unpack("<d", struct.pack("<Q", fraction_bits))[0]
        # Positive normal phase: fmod matches Rust f64::rem_euclid. Preserve
        # the reader's binary64 add/rem/sub/div before its final RN32 gain.
        phase = math.fmod(float(max(whole - start, 0)) + fraction, period)
        if phase >= float(last):
            left, right = start + last, start
            gain = rn32(F((phase - float(last)) / (period - float(last))))
        else:
            base = max(whole - start, 0)
            left = start + base % physical_length
            right = start + (base + 1) % physical_length if fraction else None
            gain = rn32(F(fraction))
        captured = progress >= abs(debt) * (1 << shift)
        result.append((whole, fraction_bits, left, right, gain, captured))
    return result


"####;
