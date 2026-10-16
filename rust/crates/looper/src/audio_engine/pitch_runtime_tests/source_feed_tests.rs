//! Real mixer capture/worker/adoption against an independent bounded-reset chronology.
//! This certifies A-reset replay only; the separate continuous-O negative probe remains open.

use super::*;
use crate::audio_engine::rubberband_backend::RubberBandLiveStateOpaque;
use std::collections::{BTreeMap, VecDeque};

#[path = "paused_source_feed_tests.rs"]
mod paused_source_feed_tests;

#[path = "selection_source_feed_tests.rs"]
pub(in crate::audio_engine) mod selection_source_feed_tests;

const START: usize = 64_000;
const END: usize = 160_000;
const EXTENT: usize = 240_000;
const F: usize = 67_904;
const E: usize = 72_000;
const BLOCK: usize = 512;
const EVENTS: [(usize, f64); 4] = [
    (1_237, 0.73),
    (17_999, 1.41),
    (47_711, 0.62),
    (64_894, 1.29),
];

// Independent C-ABI calls intentionally bypass the production Native wrapper, source
// reader, SourcePlayback, journal and preparation helpers for every reference sample.
unsafe extern "C" {
    #[link_name = "rubberband_live_new"]
    fn raw_new(rate: u32, channels: u32, options: i32) -> *mut RubberBandLiveStateOpaque;
    #[link_name = "rubberband_live_delete"]
    fn raw_delete(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_reset"]
    fn raw_reset(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_set_pitch_scale"]
    fn raw_pitch(state: *mut RubberBandLiveStateOpaque, pitch: f64);
    #[link_name = "rubberband_live_get_start_delay"]
    fn raw_delay(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_get_block_size"]
    fn raw_block(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_shift"]
    fn raw_shift(
        state: *mut RubberBandLiveStateOpaque,
        input: *const *const f32,
        output: *const *mut f32,
    );
}

struct RawRubberBand {
    state: *mut RubberBandLiveStateOpaque,
    pitch: f64,
    input: VecDeque<f32>,
    output: VecDeque<f32>,
}

impl RawRubberBand {
    fn new(pitch: f64) -> Self {
        // WINDOW_SHORT | CHANNELS_TOGETHER, the pinned mono production profile.
        let state = unsafe { raw_new(RATE, 1, 0x1000_0000) };
        assert!(!state.is_null());
        assert_eq!(unsafe { raw_block(state) } as usize, BLOCK);
        let mut raw = Self {
            state,
            pitch,
            input: VecDeque::new(),
            output: std::iter::repeat_n(0.0, BLOCK - 1).collect(),
        };
        // Native cold initialization is selected by the first COMPLETED real block.
        // The silent unity preprime never consumes source or adapter time.
        unsafe {
            raw_pitch(state, if pitch > 2.0 { 1.0 } else { pitch });
            raw_reset(state);
        }
        if pitch > 2.0 {
            let _ = raw.shift(&[0.0; BLOCK]);
            unsafe { raw_pitch(state, pitch) };
        }
        raw
    }

    fn shift(&mut self, input: &[f32; BLOCK]) -> [f32; BLOCK] {
        let mut output = [0.0; BLOCK];
        unsafe { raw_shift(self.state, &input.as_ptr(), &output.as_mut_ptr()) };
        output
    }

    fn history_bound(pitch: f64) -> usize {
        let native = Self::new(pitch);
        if (0.5..=2.0).contains(&pitch) {
            4_096
        } else {
            // Pinned 48 kHz Short window: 2048 source frames. Delay is measured only
            // AFTER this exact reset profile; no production bound helper is called.
            let delay = unsafe { raw_delay(native.state) } as usize;
            ((2048.0 * pitch.max(1.0 / pitch)).ceil() as usize + delay + 2 * BLOCK)
                .max(4_096)
                .div_ceil(BLOCK)
                * BLOCK
        }
    }

    fn sample(&mut self, sample: f32, pitch: f64) -> f32 {
        if pitch != self.pitch {
            unsafe { raw_pitch(self.state, pitch) };
            self.pitch = pitch;
        }
        self.input.push_back(sample);
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

impl Drop for RawRubberBand {
    fn drop(&mut self) {
        unsafe { raw_delete(self.state) };
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
    fn pitch(self) -> f64 {
        // Raw requested total +24, Lock ON. No production pitch conversion.
        2.0_f64.powf(24.0 / 12.0) / self.rate
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

/// Scalar arithmetic derived directly from the control events. Epoch rebasing retains
/// binary64 fractional phase, and a target change begins a fresh active 512-frame step.
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
        let (frame, mode) = if physical < boundary {
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
            passage: mode,
            rate,
        });
    }
    result
}

fn bounded_origin(timeline: &[ScalarFrame], end: usize) -> (usize, usize, usize) {
    let mut queried = BTreeMap::new();
    let mut bound = RawRubberBand::history_bound(timeline[end].pitch());
    loop {
        let origin = (end - bound) / BLOCK * BLOCK;
        let mut required = bound;
        for frame in &timeline[origin..end] {
            let pitch = frame.pitch();
            let history = *queried
                .entry(pitch.to_bits())
                .or_insert_with(|| RawRubberBand::history_bound(pitch));
            required = required.max(history);
        }
        if required == bound {
            return (origin, bound, queried.len());
        }
        bound = required;
    }
}

fn synthetic_source() -> SampleBuffer {
    synthetic_source_format(RATE, 1, EXTENT)
}

fn synthetic_source_format(sample_rate_hz: u32, channels: usize, extent: usize) -> SampleBuffer {
    SampleBuffer {
        channels,
        residency: None,
        samples: (0..extent)
            .flat_map(|frame| {
                (0..channels).map(move |channel| {
                    let time = frame as f64 / f64::from(sample_rate_hz);
                    (0.18
                        * (std::f64::consts::TAU
                            * ((149.0 + 37.0 * channel as f64) * time
                                + (7.0 + 2.0 * channel as f64) * time * time))
                            .sin()
                        + 0.05
                            * (std::f64::consts::TAU * (431.0 + 53.0 * channel as f64) * time)
                                .cos()) as f32
                })
            })
            .collect::<Vec<_>>()
            .into(),
    }
    .with_complete_source(sample_rate_hz)
}

fn setup(passage: Passage) -> (Fixture, SampleBuffer, u64) {
    setup_pitch(passage, true, 0)
}

fn setup_pitch(passage: Passage, lock: bool, extra: i32) -> (Fixture, SampleBuffer, u64) {
    let mut fixture = Fixture::new(lock);
    let source = synthetic_source();
    publish_tone(&fixture.control, 0, source.clone(), true);
    fixture.mixer.load_sample(0, source.clone());
    fixture.mixer.set_pad_loop_region(
        0,
        START as f64 / f64::from(RATE),
        Some(END as f64 / f64::from(RATE)),
    );
    fixture.mixer.set_pad_bpm(0, Some(120.0));
    // Refresh the real source publication's runtime binding; no ACK/permit is invented.
    let binding = input_runtime_binding::capture(&fixture.control.engine, 0)
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
    rows[0].key_lock = lock;
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
    let start = fixture.capture(0, &format!("pitch.extra:{extra}:set_retrigger"));
    fixture.publish(&start);
    fixture.wait_worker(&[&start]);
    fixture.render_to(start.execution_target());
    fixture.render(1);
    assert!(start.effective());
    let output_origin = fixture.transport.output_frame();
    let source_start = if passage == Passage::Intro {
        0
    } else {
        EXTENT - START
    };
    assert!(fixture.mixer.seek_sample_at_output_frame(
        0,
        source_start as f64 / f64::from(RATE),
        output_origin,
    ));
    (fixture, source, output_origin)
}

fn run_controls_to_capture(fixture: &mut Fixture, output_origin: u64) {
    for (event, rate) in EVENTS {
        fixture.render_to(output_origin + event as u64);
        if event == EVENTS[0].0 {
            fixture.mixer.set_speed(rate);
        } else {
            if event == EVENTS[1].0 {
                fixture.mixer.set_bpm_lock(true);
            }
            fixture.mixer.set_master_period(0.5 / rate);
        }
    }
    fixture.render_to(output_origin + F as u64);
}

fn assert_phase(fixture: &Fixture, expected: ScalarFrame) {
    let actual = fixture.voice(0).source_playback.position();
    assert_eq!(actual.frame, expected.frame);
    assert_eq!(actual.fraction.to_bits(), expected.fraction.to_bits());
    assert_eq!(actual.seek_mode, ExplicitSeekMode::Normal);
}

#[test]
fn expanded_moving_set_replays_actual_intro_and_tail_past_at_original_e() {
    for passage in [Passage::Intro, Passage::Tail] {
        let (mut fixture, source, output_origin) = setup(passage);
        run_controls_to_capture(&mut fixture, output_origin);
        let timeline = chronology(passage, E + 4_097);
        assert_phase(&fixture, timeline[F]);
        let generation = fixture.voice(0).generation;
        let address = fixture.voice(0).sample.as_ref().unwrap().source_address();
        let starts = fixture.starts(0);
        // Time the actual scalar-copy operation with destination storage already
        // allocated. Host observations are diagnostics, never device-deadline proof.
        let mut copied = crate::audio_engine::source_feed_history::SourceFeedSnapshot::new();
        let mut snapshot_ns = Vec::with_capacity(128);
        for _ in 0..128 {
            copied.clear_for_reuse();
            let began = Instant::now();
            assert!(
                fixture
                    .voice(0)
                    .source_feed_history
                    .capture_into(&mut copied)
            );
            snapshot_ns.push(began.elapsed().as_nanos());
        }
        snapshot_ns.sort_unstable();
        let captured_spans = copied.spans().len();
        assert!(
            captured_spans > 4,
            "real smoothing epochs must remain distinguishable"
        );
        assert_eq!(copied.active_frames() - copied.active_origin(), F as u64);
        let span_bytes =
            std::mem::size_of::<crate::audio_engine::source_feed_history::SourceFeedSpan>();
        // The real grid gives E-F=4096, keeping the original T instead of using the
        // nonquantized 8192-frame lead. This forces genuine pre-F expanded history.
        fixture.rows(PitchPadProjection {
            intent: PitchIntent::new(6, 0).unwrap(),
            key_lock: true,
            quantize_step_64ths: Some(16),
            ..PitchPadProjection::default()
        });
        // Fixture's first start/seek consumed one output frame. Place the real grid's
        // downbeat there, while retaining the actual capture timestamp and clock owner.
        fixture.refresh_clock();
        let mut clock = fixture.clock.read().unwrap();
        clock.downbeat_frame = output_origin;
        clock.master_beat = F as f64 / 24_000.0;
        fixture.clock.publish(clock);
        let action = fixture
            .control
            .engine
            .capture_pad_pitch_action(0, "pitch.extra:18:set", None)
            .unwrap();
        let original_t = action.original_due_frame();
        let original_content = action.content_key();
        let original_timestamp = action.received_timestamp();
        assert_eq!(original_t, output_origin + E as u64);
        fixture.publish(&action);
        assert_eq!(action.execution_target(), original_t);
        assert_eq!(
            action.execution_target() - fixture.transport.output_frame(),
            4_096
        );
        fixture.wait_worker(&[&action]);
        assert_eq!(fixture.transport.output_frame(), output_origin + F as u64);

        let started = Instant::now();
        let (origin, bound, profiles) = bounded_origin(&timeline, E);
        let prepass_elapsed = started.elapsed();
        assert!(bound > E - F, "pre-F history must be indispensable");
        assert!(origin < F);
        let crossing = timeline
            .iter()
            .position(|frame| frame.passage == Passage::Normal)
            .unwrap();
        assert!(
            origin < crossing && crossing < F,
            "actual {passage:?} crossing={crossing} must lie in required past {origin}..{F}"
        );
        let first_pitch = timeline[origin + BLOCK - 1].pitch();
        let mut reference = RawRubberBand::new(first_pitch);
        let mut wrong = RawRubberBand::new(first_pitch);
        let endpoint = timeline[E];
        for (active, frame) in timeline.iter().copied().enumerate().take(E).skip(origin) {
            reference.sample(frame.pcm(&source.samples), frame.pitch());
            // Deliberately wrong backwards-constant trajectory, with the SAME A,
            // cold pitch/reset profile and adapter phase as the genuine oracle.
            let coordinate = (endpoint.frame as f64 + endpoint.fraction
                - START as f64
                - (E - active) as f64 * endpoint.rate)
                .rem_euclid((END - START) as f64);
            let wrong_frame = ScalarFrame {
                frame: START + coordinate.floor() as usize,
                fraction: coordinate.fract(),
                passage: Passage::Normal,
                rate: endpoint.rate,
            };
            wrong.sample(wrong_frame.pcm(&source.samples), wrong_frame.pitch());
        }
        assert_eq!(
            reference.fifo(),
            ((E - origin) % BLOCK, BLOCK - 1 - (E - origin) % BLOCK)
        );
        assert_eq!(reference.fifo(), wrong.fifo());
        fixture.render_to(output_origin + E as u64);
        assert_phase(&fixture, endpoint);
        let mut maximum = 0.0_f32;
        let mut wrong_maximum = 0.0_f32;
        // Include the VERY FIRST adopted sample. The mono pad has unity trim,
        // neutral DSP, velocity and master gain, so no reference gain is fitted.
        for (partition, frames) in [1, 73, 511, 127, 512, 379, 512, 512, 512, 512, 446]
            .into_iter()
            .enumerate()
        {
            let active = fixture.transport.output_frame() as usize - output_origin as usize;
            let actual = fixture.render(frames);
            assert!(
                action.effective(),
                "{} {:?}",
                action.state(),
                action.reason()
            );
            for (offset, sample) in actual.iter().enumerate() {
                let frame = timeline[active + offset];
                let expected = reference.sample(frame.pcm(&source.samples), frame.pitch());
                let backwards_constant = wrong.sample(frame.pcm(&source.samples), frame.pitch());
                let difference = (*sample - expected).abs();
                maximum = maximum.max(difference);
                wrong_maximum = wrong_maximum.max((*sample - backwards_constant).abs());
                assert!(
                    difference < 2.0e-6,
                    "{passage:?} partition={partition} adopted_offset={} actual={sample} independent_A_reset={expected} error={difference}",
                    active + offset - E
                );
            }
            assert_phase(&fixture, timeline[active + frames]);
            assert_eq!(
                fixture.voice(0).stretch.pending_fifo_frames(),
                reference.fifo()
            );
        }
        assert!(
            wrong_maximum > 0.005,
            "backwards-constant oracle must expose missing history: {wrong_maximum}"
        );
        let voice = fixture.voice(0);
        assert_eq!(voice.generation, generation);
        assert_eq!(voice.sample.as_ref().unwrap().source_address(), address);
        assert_eq!(voice.stretch.adopted_request_id(), Some(action.sequence()));
        assert_eq!(fixture.starts(0), starts);
        assert_eq!(action.original_due_frame(), original_t);
        assert_eq!(action.execution_target(), original_t);
        assert_eq!(action.content_key(), original_content);
        assert_eq!(action.received_timestamp(), original_timestamp);
        let rendered = fixture.transport.output_frame() as usize - output_origin as usize - E;
        assert_eq!(
            voice
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            (E - origin + rendered) as u64
        );
        println!(
            "source_feed_A_reset passage={passage:?} output_O={output_origin} relative_A={origin} relative_F={F} relative_E={E} H={bound} profiles={profiles} independent_bound_probe_host_us={} span_bytes={span_bytes} captured_spans={captured_spans} copied_scalar_bytes={} snapshot_host_median_ns={} snapshot_host_max_ns={} every_sample_max_abs={maximum} backwards_constant_max_abs={wrong_maximum}; continuous_O_negative_is_separate_not_a_PASS",
            prepass_elapsed.as_micros(),
            captured_spans * span_bytes,
            snapshot_ns[64],
            snapshot_ns[127]
        );
    }
}

#[test]
fn public_worker_refuses_insufficient_actual_past_then_adopts_a_later_original_set() {
    fn identical_old_pcm_to(edited: &mut Fixture, reference: &mut Fixture, target: u64) {
        assert_eq!(
            edited.transport.output_frame(),
            reference.transport.output_frame()
        );
        while edited.transport.output_frame() < target {
            let frames = (target - edited.transport.output_frame()).min(379) as usize;
            assert_eq!(edited.render(frames), reference.render(frames));
            assert!(
                edited
                    .voice(0)
                    .source_playback
                    .matches_exact(&reference.voice(0).source_playback)
            );
            assert_eq!(
                edited.voice(0).stretch.pending_fifo_frames(),
                reference.voice(0).stretch.pending_fifo_frames()
            );
        }
    }

    let (mut fixture, _source, output_origin) = setup(Passage::Intro);
    let (mut reference, _reference_source, reference_origin) = setup(Passage::Intro);
    assert_eq!(output_origin, reference_origin);
    identical_old_pcm_to(&mut fixture, &mut reference, output_origin + 37);
    fixture.mixer.set_speed(0.6);
    reference.mixer.set_speed(0.6);
    identical_old_pcm_to(&mut fixture, &mut reference, output_origin + 1_000);
    let runtime_address = fixture.mixer.pitch_runtime.as_ref().unwrap() as *const _ as usize;
    let voice_generation = fixture.voice(0).generation;
    let source_address = fixture.voice(0).sample.as_ref().unwrap().source_address();
    let native_address = fixture.voice(0).stretch.native_state_address();
    let starts = fixture.starts(0);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::default())
    );
    fixture.rows(PitchPadProjection {
        intent: PitchIntent::new(6, 0).unwrap(),
        key_lock: true,
        ..PitchPadProjection::default()
    });
    let refused = fixture.capture(0, "pitch.extra:18:set");
    let original_t = refused.original_due_frame();
    let original_sequence = refused.sequence();
    let original_timestamp = refused.received_timestamp();
    let original_content = refused.content_key();
    assert_eq!(original_t, output_origin + 1_000);
    // Direct public enqueue/drain avoids an accepted-status race with this intentionally
    // fast worker refusal. The captured original action is never replaced or recaptured.
    assert!(
        fixture
            .control
            .engine
            .publish_pitch_action(&refused)
            .unwrap()
    );
    fixture.drain();
    let original_e = refused.execution_target();
    assert_eq!(original_e, original_t + 8_192);
    let fixed_f = fixture.transport.output_frame();
    let deadline = Instant::now() + WORKER_WAIT;
    while refused.state() != "rejected" {
        fixture.mixer.pitch_runtime.as_mut().unwrap().poll();
        assert!(
            Instant::now() < deadline,
            "request-local refusal host wait exceeded 2 s"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), fixed_f);
    assert_eq!(refused.reason(), Some("preparation_failed"));
    assert!(!refused.effective());
    assert_eq!(refused.original_due_frame(), original_t);
    assert_eq!(refused.execution_target(), original_e);
    assert_eq!(refused.sequence(), original_sequence);
    assert_eq!(refused.received_timestamp(), original_timestamp);
    assert_eq!(refused.content_key(), original_content);
    assert_eq!(fixture.voice(0).generation, voice_generation);
    assert_eq!(
        fixture.voice(0).stretch.native_state_address(),
        native_address
    );
    assert_eq!(
        fixture.voice(0).sample.as_ref().unwrap().source_address(),
        source_address
    );
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::default())
    );
    // Configured expanded H exceeds the entire actual age at E. No synthetic
    // backwards Intro/constant prefix may rescue this moving request.
    assert!(RawRubberBand::history_bound(4.0 / 0.6) > (original_e - output_origin) as usize);
    // Cover every old-voice sample across the rejected action's scheduled E.
    // The baseline is another independently owned actual mixer/source, with no SET
    // or source-preparation helper used to manufacture its expected PCM.
    identical_old_pcm_to(&mut fixture, &mut reference, original_e + 513);
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(fixture.voice(0).generation, voice_generation);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::default())
    );
    assert!(fixture.scheduler.is_empty());

    // The SAME public preparation worker now receives a new action with enough
    // genuine retained reads. The recent edit prevents the constant endpoint path.
    identical_old_pcm_to(&mut fixture, &mut reference, output_origin + 29_300);
    fixture.mixer.set_speed(0.91);
    reference.mixer.set_speed(0.91);
    identical_old_pcm_to(&mut fixture, &mut reference, output_origin + 30_000);
    fixture.rows(PitchPadProjection {
        intent: PitchIntent::new(6, 0).unwrap(),
        key_lock: true,
        ..PitchPadProjection::default()
    });
    let valid = fixture.capture(0, "pitch.extra:18:set");
    let valid_t = valid.original_due_frame();
    let valid_sequence = valid.sequence();
    let valid_timestamp = valid.received_timestamp();
    let valid_content = valid.content_key();
    assert!(valid_sequence > original_sequence);
    fixture.publish(&valid);
    let valid_e = valid.execution_target();
    assert_eq!(valid_t, output_origin + 30_000);
    assert_eq!(valid_e, valid_t + 8_192);
    fixture.wait_worker(&[&valid]);
    assert_eq!(fixture.transport.output_frame(), output_origin + 30_000);
    assert_eq!(
        fixture.mixer.pitch_runtime.as_ref().unwrap() as *const _ as usize,
        runtime_address
    );
    identical_old_pcm_to(&mut fixture, &mut reference, valid_e);
    let adopted_pcm = fixture.render(1);
    assert!(valid.effective(), "{} {:?}", valid.state(), valid.reason());
    assert!(adopted_pcm.iter().all(|sample| sample.is_finite()));
    let continuation = fixture.pcm(4_096);
    assert!(continuation.iter().all(|sample| sample.is_finite()));
    assert!(continuation.iter().any(|sample| sample.abs() > 0.01));
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(valid_sequence)
    );
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(6, 18).unwrap())
    );
    assert_eq!(fixture.voice(0).generation, voice_generation);
    assert_eq!(
        fixture.voice(0).sample.as_ref().unwrap().source_address(),
        source_address
    );
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(valid.original_due_frame(), valid_t);
    assert_eq!(valid.execution_target(), valid_e);
    assert_eq!(valid.sequence(), valid_sequence);
    assert_eq!(valid.received_timestamp(), valid_timestamp);
    assert_eq!(valid.content_key(), valid_content);
    assert_eq!(refused.state(), "rejected");
    assert_eq!(refused.original_due_frame(), original_t);
    assert_eq!(refused.execution_target(), original_e);
    assert!(!refused.effective());
    assert!(fixture.scheduler.is_empty());
}

#[test]
fn earlier_moving_set_survives_a_later_source_neutral_prospective_lock_request() {
    let (mut fixture, source, output_origin) = setup_pitch(Passage::Intro, false, 7);
    run_controls_to_capture(&mut fixture, output_origin);
    let generation = fixture.voice(0).generation;
    let source_address = fixture.voice(0).sample.as_ref().unwrap().source_address();
    let starts = fixture.starts(0);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(0, 7).unwrap())
    );
    assert!(!fixture.mixer.key_lock_for_measurement(0));
    fixture.rows(PitchPadProjection {
        intent: PitchIntent::new(6, 0).unwrap(),
        key_lock: false,
        ..PitchPadProjection::default()
    });
    let earlier = fixture.capture(0, "pitch.extra:18:set");
    let original_t = earlier.original_due_frame();
    let original_sequence = earlier.sequence();
    let original_timestamp = earlier.received_timestamp();
    let original_content = earlier.content_key();
    fixture.publish(&earlier);
    let original_e = earlier.execution_target();
    assert_eq!(original_t, output_origin + F as u64);
    assert_eq!(original_e, original_t + 8_192);
    assert_eq!(earlier.render().pitch, PitchIntent::new(6, 18).unwrap());
    assert!(!earlier.render().key_lock);
    fixture.wait_worker(&[&earlier]);

    let mode_f = original_t + 5_000;
    fixture.render_to(mode_f);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(0, 7).unwrap())
    );
    let later_mode = fixture.control.engine.set_pad_key_lock(0, true).unwrap();
    fixture.drain();
    let status = fixture
        .control
        .engine
        .input_runtime_ownership
        .key_lock_status(0)
        .unwrap();
    assert_eq!(status.request_id, later_mode);
    assert_eq!(status.state, "waiting");
    assert!(!status.effective);
    // This real render captures the prospective tuple's own forward-preparation
    // request. Its numeric pitch is still +7; it has no source/rate/seek effect.
    fixture.render(1);
    let fixed_f = fixture.transport.output_frame();
    let deadline = Instant::now() + WORKER_WAIT;
    while !fixture
        .mixer
        .voices
        .iter_mut()
        .find(|voice| voice.active && voice.sample_id == 0)
        .unwrap()
        .stretch
        .prospective_work_ready()
    {
        assert!(
            Instant::now() < deadline,
            "prospective Lock worker host wait exceeded 2 s"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), fixed_f);
    assert!(!fixture.mixer.key_lock_for_measurement(0));
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(0, 7).unwrap())
    );

    let end = (original_e - output_origin) as usize;
    let timeline = chronology(Passage::Intro, end + 4_097);
    // Lock OFF has prospective p=2^(24/12)=4 for every actual source-rate span.
    // H/reset origin and every PCM tap remain independent of production helpers.
    let bound = RawRubberBand::history_bound(4.0);
    let reset_origin = (end - bound) / BLOCK * BLOCK;
    assert!(bound > end - F);
    let mut raw = RawRubberBand::new(4.0);
    for frame in &timeline[reset_origin..end] {
        raw.sample(frame.pcm(&source.samples), 4.0);
    }
    fixture.render_to(original_e);
    assert!(!earlier.effective());
    assert_phase(&fixture, timeline[end]);
    let mut max_error = 0.0_f32;
    for frames in [1, 73, 511, 127, 512, 379, 512, 512, 512, 512, 446] {
        let active = (fixture.transport.output_frame() - output_origin) as usize;
        let actual = fixture.render(frames);
        assert!(
            earlier.effective(),
            "{} {:?}",
            earlier.state(),
            earlier.reason()
        );
        for (offset, sample) in actual.iter().enumerate() {
            let expected = raw.sample(timeline[active + offset].pcm(&source.samples), 4.0);
            let error = (*sample - expected).abs();
            max_error = max_error.max(error);
            assert!(
                error < 2.0e-6,
                "later prospective Lock changed earlier SET PCM at adopted_offset={}: actual={sample} independent_A_reset={expected} error={error}",
                active + offset - end
            );
        }
        assert_phase(&fixture, timeline[active + frames]);
        assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), raw.fifo());
    }
    // The earlier original tuple wins at E. The later request captured +7 and
    // therefore cannot adopt its prospective ON owner after +24 becomes actual.
    let status = fixture
        .control
        .engine
        .input_runtime_ownership
        .key_lock_status(0)
        .unwrap();
    assert_eq!(status.request_id, later_mode);
    assert_eq!(status.state, "error");
    assert!(!status.effective);
    assert!(!status.ready);
    assert!(!fixture.mixer.key_lock_for_measurement(0));
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(6, 18).unwrap())
    );
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(original_sequence)
    );
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(
        fixture.voice(0).sample.as_ref().unwrap().source_address(),
        source_address
    );
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(earlier.original_due_frame(), original_t);
    assert_eq!(earlier.execution_target(), original_e);
    assert_eq!(earlier.sequence(), original_sequence);
    assert_eq!(earlier.received_timestamp(), original_timestamp);
    assert_eq!(earlier.content_key(), original_content);
    assert!(fixture.scheduler.is_empty());
    println!(
        "source_neutral_prospective_Lock_after_moving_SET O={output_origin} A={reset_origin} F={F} E={end} H={bound} every_sample_max_abs={max_error}"
    );
}

#[test]
fn post_capture_rate_change_and_restore_refuses_only_the_old_set_and_retains_actual_past() {
    fn equal_old_to(edited: &mut Fixture, reference: &mut Fixture, target: u64) {
        assert_eq!(
            edited.transport.output_frame(),
            reference.transport.output_frame()
        );
        while edited.transport.output_frame() < target {
            let frame = edited.transport.output_frame();
            let mut stop = target.min(frame + 379);
            // A scheduled refusal still cuts the edited callback at its original E.
            // Give the independently owned baseline the exact same real render cuts.
            for next in [
                edited.scheduler.peek_next_target_frame(),
                reference.scheduler.peek_next_target_frame(),
            ]
            .into_iter()
            .flatten()
            {
                if next > frame {
                    stop = stop.min(next);
                }
            }
            let frames = (stop - frame) as usize;
            let before_adoption = (
                edited.voice(0).stretch.adopted_request_id(),
                reference.voice(0).stretch.adopted_request_id(),
            );
            let actual = edited.render(frames);
            let expected = reference.render(frames);
            if let Some((offset, (actual, expected))) = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .find(|(_, (actual, expected))| actual != expected)
            {
                panic!(
                    "old PCM mismatch: callback={frame} frames={frames} comparison_target={target} first_output={} offset={offset} actual={actual} expected={expected} adoption_before={before_adoption:?} adoption_after={:?} native_after={:?} phase_after={:?} fifo_after={:?}",
                    frame + offset as u64,
                    (
                        edited.voice(0).stretch.adopted_request_id(),
                        reference.voice(0).stretch.adopted_request_id()
                    ),
                    (
                        edited.voice(0).stretch.native_state_address(),
                        reference.voice(0).stretch.native_state_address()
                    ),
                    (
                        edited.voice(0).source_playback.position(),
                        reference.voice(0).source_playback.position()
                    ),
                    (
                        edited.voice(0).stretch.pending_fifo_frames(),
                        reference.voice(0).stretch.pending_fifo_frames()
                    )
                );
            }
            assert!(
                edited
                    .voice(0)
                    .source_playback
                    .matches_exact(&reference.voice(0).source_playback)
            );
            assert_eq!(
                edited.voice(0).stretch.pending_fifo_frames(),
                reference.voice(0).stretch.pending_fifo_frames()
            );
        }
    }

    for render_changed_interval in [true, false] {
        // Keep the old route dry so independently timed automatic Key Lock preparation
        // cannot replace either reference owner before the source-control mutation.
        let (mut fixture, _source, output_origin) = setup_pitch(Passage::Intro, false, 0);
        let (mut reference, _reference_source, reference_origin) =
            setup_pitch(Passage::Intro, false, 0);
        assert_eq!(output_origin, reference_origin);
        run_controls_to_capture(&mut fixture, output_origin);
        run_controls_to_capture(&mut reference, output_origin);
        let generation = fixture.voice(0).generation;
        let source_address = fixture.voice(0).sample.as_ref().unwrap().source_address();
        let native_address = fixture.voice(0).stretch.native_state_address();
        let starts = fixture.starts(0);
        let runtime_address = fixture.mixer.pitch_runtime.as_ref().unwrap() as *const _ as usize;
        fixture.rows(PitchPadProjection {
            intent: PitchIntent::new(6, 0).unwrap(),
            key_lock: false,
            ..PitchPadProjection::default()
        });
        let old = fixture.capture(0, "pitch.extra:18:set");
        let old_t = old.original_due_frame();
        let old_sequence = old.sequence();
        let old_timestamp = old.received_timestamp();
        let old_content = old.content_key();
        fixture.publish(&old);
        let old_e = old.execution_target();
        assert_eq!(old_e, old_t + 8_192);
        fixture.wait_worker(&[&old]);
        equal_old_to(&mut fixture, &mut reference, old_t + 1_000);
        // BPM_LOCK is active, so edit its actual master period rather than an ignored
        // global speed scalar. Restoring the target does not undo these real reads.
        fixture.mixer.set_master_period(0.5 / 0.91);
        reference.mixer.set_master_period(0.5 / 0.91);
        if render_changed_interval {
            equal_old_to(&mut fixture, &mut reference, old_t + 1_137);
        }
        fixture.mixer.set_master_period(0.5 / 1.29);
        reference.mixer.set_master_period(0.5 / 1.29);
        equal_old_to(&mut fixture, &mut reference, old_e + 513);
        assert_eq!(old.state(), "rejected");
        assert_eq!(old.reason(), Some("preparation_failed"));
        if !render_changed_interval {
            // Both period edits happened between reads: the physical endpoint still
            // equals the original scalar chronology. Refusal therefore proves the
            // source-control fence, rather than a later endpoint mismatch alone.
            let active = (fixture.transport.output_frame() - output_origin) as usize;
            let unchanged = chronology(Passage::Intro, active);
            assert_phase(&fixture, unchanged[active]);
        }
        assert!(!old.effective());
        assert_eq!(old.original_due_frame(), old_t);
        assert_eq!(old.execution_target(), old_e);
        assert_eq!(old.sequence(), old_sequence);
        assert_eq!(old.received_timestamp(), old_timestamp);
        assert_eq!(old.content_key(), old_content);
        assert_eq!(
            fixture.mixer.pad_pitch_intent(0),
            Some(PitchIntent::default())
        );
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(
            fixture.voice(0).sample.as_ref().unwrap().source_address(),
            source_address
        );
        assert_eq!(
            fixture.voice(0).stretch.native_state_address(),
            native_address
        );
        assert_eq!(fixture.starts(0), starts);
        assert!(fixture.scheduler.is_empty());

        // A new capture owns the actual changed-and-restored chronology. The journal
        // keeps these genuine spans; the old request's fence does not clear their data.
        fixture.rows(PitchPadProjection {
            intent: PitchIntent::new(6, 0).unwrap(),
            key_lock: false,
            ..PitchPadProjection::default()
        });
        let fresh = fixture.capture(0, "pitch.extra:18:set");
        let fresh_t = fresh.original_due_frame();
        let fresh_sequence = fresh.sequence();
        fixture.publish(&fresh);
        let fresh_e = fresh.execution_target();
        assert_eq!(fresh_e, fresh_t + 8_192);
        fixture.wait_worker(&[&fresh]);
        assert_eq!(
            fixture.mixer.pitch_runtime.as_ref().unwrap() as *const _ as usize,
            runtime_address
        );
        equal_old_to(&mut fixture, &mut reference, fresh_e);
        fixture.render(1);
        assert!(fresh.effective(), "{} {:?}", fresh.state(), fresh.reason());
        assert_eq!(fresh.original_due_frame(), fresh_t);
        assert_eq!(fresh.execution_target(), fresh_e);
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(fresh_sequence)
        );
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(
            fixture.voice(0).sample.as_ref().unwrap().source_address(),
            source_address
        );
        assert_eq!(fixture.starts(0), starts);
        assert_eq!(
            fixture.mixer.pad_pitch_intent(0),
            Some(PitchIntent::new(6, 18).unwrap())
        );
        assert_eq!(old.state(), "rejected");
        assert!(!old.effective());
        assert!(fixture.scheduler.is_empty());
        println!(
            "post_capture_rate_restore render_changed_interval={render_changed_interval} old_T={old_t} old_E={old_e} refusal=preparation_failed fresh_T={fresh_t} fresh_E={fresh_e} later_original_action_effective=true"
        );
    }
}
