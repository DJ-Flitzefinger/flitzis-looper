//! Public unity births followed by a real global-rate edit, without a device clock.
//! Direct Native observations describe this pinned software profile, not hearing acceptance.

use super::*;
use crate::audio_engine::audio_stream::drain_parameter_messages;
use crate::audio_engine::rubberband_backend::RubberBandLiveStateOpaque;
use crate::messages::ControlParameterMessage;
use pyo3::prelude::*;
use std::collections::VecDeque;

const NATIVE_BLOCK: usize = 512;
const DRY_FRAMES: usize = 4_829;
const FIRST_CHUNK: usize = 73;
const HISTORY_FRAMES: usize = 4_096;

#[derive(Debug)]
struct RateStatus {
    request_revision: u64,
    effect_revision: u64,
    state: String,
    requested_speed: f64,
    requested_master_period: Option<f64>,
    effective_speed: f64,
    effective_master_period: Option<f64>,
    execution_frame: Option<u64>,
    error: Option<String>,
}

fn rate_status(fixture: &Fixture) -> RateStatus {
    Python::attach(|py| {
        let status = fixture
            .control
            .engine
            .global_rate_status(py)
            .unwrap()
            .unwrap();
        let status = status.bind(py);
        let value = |key: &str| status.get_item(key).unwrap().unwrap();
        RateStatus {
            request_revision: value("request_revision").extract().unwrap(),
            effect_revision: value("effect_revision").extract().unwrap(),
            state: value("state").extract().unwrap(),
            requested_speed: value("requested_speed").extract().unwrap(),
            requested_master_period: value("requested_master_period").extract().unwrap(),
            effective_speed: value("effective_speed").extract().unwrap(),
            effective_master_period: value("effective_master_period").extract().unwrap(),
            execution_frame: value("execution_frame").extract().unwrap(),
            error: value("error").extract().unwrap(),
        }
    })
}

fn parameter_fixture() -> (Fixture, rtrb::Consumer<ControlParameterMessage>) {
    let mut fixture = Fixture::new(true);
    fixture.control.engine.test_output_format = Some((RATE, 1));
    let (parameters, consumer) = RingBuffer::<ControlParameterMessage>::new(8);
    fixture.control.engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameters)));
    (fixture, consumer)
}

fn drain_rate(fixture: &mut Fixture, consumer: &mut rtrb::Consumer<ControlParameterMessage>) {
    drain_parameter_messages(consumer, &mut fixture.mixer, &mut fixture.transport);
    fixture.drain();
    fixture.render(0);
}

fn public_unity_birth(fixture: &mut Fixture) -> CapturedPitchAction {
    let action = fixture.capture(0, "pitch.extra:12:set_retrigger");
    let due = action.original_due_frame();
    fixture.publish(&action);
    assert_eq!(action.execution_target(), due);
    fixture.render(0);
    assert!(
        action.effective(),
        "{} {:?}",
        action.state(),
        action.reason()
    );
    assert_eq!(fixture.transport.output_frame(), due);
    assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), 2.0);
    assert_eq!(
        fixture.voice(0).source_playback.position().frame,
        LOOP_START
    );
    action
}

fn wait_source_target(fixture: &mut Fixture, id: usize, execution: u64) {
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        fixture.render(0);
        let voice = fixture
            .mixer
            .voices
            .iter_mut()
            .find(|voice| voice.active && voice.sample_id == id)
            .unwrap();
        if voice.stretch.source_preparation_ready()
            && voice.stretch.prepared_target_output_frame() == Some(execution)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "rate history host wait exceeded 2 s"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
}

// Independent C ABI: neither the production adapter, preparation, source reader nor
// journal supplies reference samples, source chronology or FIFO accounting.
unsafe extern "C" {
    #[link_name = "rubberband_live_new"]
    fn raw_new(rate: u32, channels: u32, options: i32) -> *mut RubberBandLiveStateOpaque;
    #[link_name = "rubberband_live_delete"]
    fn raw_delete(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_reset"]
    fn raw_reset(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_set_pitch_scale"]
    fn raw_pitch(state: *mut RubberBandLiveStateOpaque, pitch: f64);
    #[link_name = "rubberband_live_get_block_size"]
    fn raw_block(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_shift"]
    fn raw_shift(
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
    fn new(pitch: f64) -> Self {
        // Pinned mono Short-window/channels-together options, as in existing direct probes.
        let state = unsafe { raw_new(RATE, 1, 0x1000_0000) };
        assert!(!state.is_null());
        assert_eq!(unsafe { raw_block(state) } as usize, NATIVE_BLOCK);
        unsafe {
            raw_pitch(state, pitch);
            raw_reset(state);
        }
        Self {
            state,
            pitch,
            input: VecDeque::new(),
            output: std::iter::repeat_n(0.0, NATIVE_BLOCK - 1).collect(),
        }
    }

    fn sample(&mut self, sample: f32, pitch: f64) -> f32 {
        if pitch != self.pitch {
            unsafe { raw_pitch(self.state, pitch) };
            self.pitch = pitch;
        }
        self.input.push_back(sample);
        if self.input.len() == NATIVE_BLOCK {
            let input: [f32; NATIVE_BLOCK] =
                std::array::from_fn(|_| self.input.pop_front().unwrap());
            let mut output = [0.0; NATIVE_BLOCK];
            unsafe { raw_shift(self.state, &input.as_ptr(), &output.as_mut_ptr()) };
            self.output.extend(output);
        }
        self.output.pop_front().unwrap()
    }
}

impl Drop for DirectNative {
    fn drop(&mut self) {
        unsafe { raw_delete(self.state) };
    }
}

/// Analytic fixture knots and physical-loop interpolation, independent of SourceReadPlan.
fn scalar_source(phase: f64) -> f32 {
    scalar_source_hz(phase, SOURCE_HZ)
}

fn scalar_source_hz(phase: f64, hz: f64) -> f32 {
    let phase =
        LOOP_START as f64 + (phase - LOOP_START as f64).rem_euclid((LOOP_END - LOOP_START) as f64);
    let frame = phase.floor() as usize;
    let fraction = (phase - frame as f64) as f32;
    let right = if frame + 1 == LOOP_END {
        LOOP_START
    } else {
        frame + 1
    };
    let knot = |frame: usize| {
        (0.25 * (frame as f64 * hz * std::f64::consts::TAU / f64::from(RATE)).sin()) as f32
    };
    let left = knot(frame);
    left + (knot(right) - left) * fraction
}

fn pcm_difference(actual: &[f32], expected: &[f32]) -> (f32, f64) {
    assert_eq!(actual.len(), expected.len());
    let maximum = actual
        .iter()
        .zip(expected)
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0_f32, f32::max);
    let rms = (actual
        .iter()
        .zip(expected)
        .map(|(actual, expected)| f64::from(actual - expected).powi(2))
        .sum::<f64>()
        / actual.len() as f64)
        .sqrt();
    (maximum, rms)
}

#[test]
fn public_native_unity_birth_then_rate_edit_keeps_first_irregular_wet_history_and_pcm() {
    let (mut fixture, mut parameter_consumer) = parameter_fixture();
    fixture.control.engine.set_speed(2.0).unwrap();
    drain_rate(&mut fixture, &mut parameter_consumer);

    // This is a stopped public SET_RETRIGGER, born at actual h/r=2/2=1.
    // It has no prepared source candidate and no artificially supplied wet history.
    let birth = public_unity_birth(&mut fixture);
    let sequence = birth.sequence();
    let content = birth.content_key();
    let received = birth.received_timestamp();
    let due = birth.original_due_frame();
    assert_eq!(birth.render().pitch, PitchIntent::new(0, 12).unwrap());
    assert!(birth.render().key_lock);
    assert_eq!(due, 0);
    assert_eq!(birth.execution_target(), due);
    assert_eq!(fixture.transport.output_frame(), due);
    assert!(fixture.scheduler.is_empty());
    assert_eq!(fixture.starts(0), 1);
    let generation = fixture.voice(0).generation;
    let source_address = fixture.voice(0).sample.as_ref().unwrap().source_address();
    assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), 2.0);
    assert_eq!(
        fixture.voice(0).source_playback.position().frame,
        LOOP_START
    );

    let dry = fixture.pcm(DRY_FRAMES);
    let dry_expected: Vec<_> = (0..DRY_FRAMES)
        .map(|frame| scalar_source(LOOP_START as f64 + frame as f64 * 2.0))
        .collect();
    let dry_difference = pcm_difference(&dry, &dry_expected);
    assert!(dry_difference.0 < 2.0e-5, "dry PCM={dry_difference:?}");
    let voice = fixture.voice(0);
    assert!(!voice.stretch.loop_acceptance_state().0);
    assert_eq!(voice.stretch.native_primed_output_frames(), 0);
    assert_eq!(voice.stretch.adopted_request_id(), None);
    assert!(voice.stretch.productive_history().is_none());
    assert!(
        !voice
            .stretch
            .owns_pitch_contract(birth.render().pitch, true)
    );
    let before = voice.source_playback;
    let before_native = voice.stretch.native_state_address();
    let before_prepared = fixture
        .mixer
        .voices
        .iter_mut()
        .find(|voice| voice.active && voice.sample_id == 0)
        .unwrap()
        .stretch
        .source_preparation_ready();
    assert!(!before_prepared);

    // Observe the original public parameter mailbox; no direct cursor/rate mutation.
    fixture.control.engine.set_speed(1.8).unwrap();
    assert_eq!(rate_status(&fixture).state, "queued");
    drain_rate(&mut fixture, &mut parameter_consumer);
    assert!(fixture.voice(0).source_playback.matches_exact(&before));
    let preparing = rate_status(&fixture);
    let rate_execution = fixture.transport.output_frame() + HISTORY_FRAMES as u64;
    assert_eq!(preparing.state, "preparing", "{preparing:?}");
    assert_eq!(preparing.request_revision, preparing.effect_revision);
    assert_eq!(preparing.execution_frame, Some(rate_execution));
    assert_eq!(preparing.requested_speed, 1.8);
    assert_eq!(preparing.effective_speed, 2.0);
    assert_eq!(preparing.requested_master_period, None);
    assert_eq!(preparing.effective_master_period, None);
    assert!(preparing.error.is_none());
    wait_source_target(&mut fixture, 0, rate_execution);

    // Readiness is not effect: every real frame before E remains the healthy old
    // dry unity trajectory, with no early target change or startup silence.
    let pending = fixture.render_to(rate_execution);
    let pending_expected: Vec<_> = (DRY_FRAMES..DRY_FRAMES + HISTORY_FRAMES)
        .map(|frame| scalar_source(LOOP_START as f64 + frame as f64 * 2.0))
        .collect();
    assert!(pcm_difference(&pending, &pending_expected).0 < 2.0e-5);
    let mut expected_clock = before;
    expected_clock.advance(HISTORY_FRAMES);
    assert!(
        fixture
            .voice(0)
            .source_playback
            .matches_exact(&expected_clock)
    );
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), 1);
    assert_eq!(
        fixture.voice(0).stretch.native_state_address(),
        before_native
    );
    assert_eq!(rate_status(&fixture).state, "preparing");
    fixture.render(0);
    let applied = rate_status(&fixture);
    assert_eq!(applied.state, "applied", "{applied:?}");
    assert_eq!(applied.request_revision, preparing.request_revision);
    assert_eq!(applied.effect_revision, preparing.request_revision);
    assert_eq!(applied.execution_frame, Some(rate_execution));
    assert_eq!(applied.effective_speed, 1.8);
    expected_clock.set_target(1.8);
    let (frames, actual_rate) = expected_clock.chunk(FIRST_CHUNK);
    assert_eq!(frames, FIRST_CHUNK);
    assert_eq!(actual_rate, 1.95);
    expected_clock.advance(frames);

    // The declared A-reset reference starts at A=E-4096=F, feeds only the actual
    // old unity history, and continues at E. Continuous-O remains diagnostic;
    // its different unbounded past is not the finite-history acceptance oracle.
    let mut bounded = DirectNative::new(1.0);
    for &sample in &pending_expected {
        bounded.sample(sample, 1.0);
    }
    let mut continuous = DirectNative::new(1.0);
    for &sample in dry_expected.iter().chain(&pending_expected) {
        continuous.sample(sample, 1.0);
    }
    let pitch = 2.0 / actual_rate;
    let mut cold = DirectNative::new(pitch);
    let transition_phase = LOOP_START as f64 + (DRY_FRAMES + HISTORY_FRAMES) as f64 * 2.0;
    let mut expected = Vec::with_capacity(FIRST_CHUNK);
    let mut continuous_expected = Vec::with_capacity(FIRST_CHUNK);
    let mut cold_expected = Vec::with_capacity(FIRST_CHUNK);
    for frame in 0..FIRST_CHUNK {
        let source = scalar_source(transition_phase + frame as f64 * actual_rate);
        expected.push(bounded.sample(source, pitch));
        continuous_expected.push(continuous.sample(source, pitch));
        cold_expected.push(cold.sample(source, pitch));
    }

    let first = fixture.render(FIRST_CHUNK);
    let voice = fixture.voice(0);
    let owner = voice
        .stretch
        .owns_pitch_contract(birth.render().pitch, true);
    let source_wet = voice.stretch.wet_current_source_output_ready();
    let state = voice.stretch.loop_acceptance_state();
    let history = voice.stretch.productive_history();
    let difference = pcm_difference(&first, &expected);
    let continuous_difference = pcm_difference(&first, &continuous_expected);
    let cold_difference = pcm_difference(&first, &cold_expected);
    let leading_zeros = first.iter().take_while(|sample| **sample == 0.0).count();
    let reference_energy = expected
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / expected.len() as f64;
    println!(
        "native_unity_BIRTH public_sequence={sequence} original_T={due} birth_E={} rate_E={rate_execution} rate_request={} declared_A={} dry_frames={DRY_FRAMES} dry_max={} requested_rate=1.8 actual_first_rate={actual_rate} first_frames={FIRST_CHUNK} same_native={} source_owner={owner} current_source_wet={source_wet} native_state={state:?} adopted_request={:?} primed_frames={} productive_frames={:?} first_leading_zeros={leading_zeros} matched_A_reset_max={} matched_A_reset_rms={} continuous_O_diagnostic={continuous_difference:?} fresh_cold_max={} fresh_cold_rms={} reference_energy={reference_energy} first_pcm={:?}; software_only_no_acoustic_acceptance",
        birth.execution_target(),
        preparing.request_revision,
        DRY_FRAMES,
        dry_difference.0,
        voice.stretch.native_state_address() == before_native,
        voice.stretch.adopted_request_id(),
        voice.stretch.native_primed_output_frames(),
        history.map(|history| history.fed_output_frames),
        difference.0,
        difference.1,
        cold_difference.0,
        cold_difference.1,
        &first[..8],
    );

    // Check public identity and canonical source continuity even when the final wet
    // contract fails, so the baseline distinguishes dropout from a retrigger/seek.
    assert!(birth.effective());
    assert_eq!(birth.sequence(), sequence);
    assert_eq!(birth.content_key(), content);
    assert_eq!(birth.received_timestamp(), received);
    assert_eq!(birth.original_due_frame(), due);
    assert_eq!(birth.execution_target(), due);
    assert_eq!(
        fixture
            .control
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .pitch_request_sequence(0)
            .unwrap(),
        sequence,
    );
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(birth.render().pitch)
    );
    assert_eq!(fixture.starts(0), 1);
    assert_eq!(voice.generation, generation);
    assert_eq!(
        voice.sample.as_ref().unwrap().source_address(),
        source_address
    );
    assert!(voice.source_playback.matches_exact(&expected_clock));
    assert_eq!(voice.source_playback.rate_target(), 1.8);
    assert!(first.iter().all(|sample| sample.is_finite()));
    assert!(reference_energy > 0.001);
    assert_eq!(
        voice.stretch.pending_fifo_frames(),
        (bounded.input.len(), bounded.output.len())
    );
    let history = history.expect("the adopted tuple retains actual source history");
    assert_eq!(history.binding.source_address, source_address);
    assert_eq!(history.next_position, expected_clock.position());
    assert!(history.fed_output_frames >= (HISTORY_FRAMES + FIRST_CHUNK) as u64);
    assert!(
        owner && source_wet && difference.0 < 2.0e-5,
        "unity birth lost source-specific wet continuation: owner={owner} source_wet={source_wet} first_PCM_max={} fresh_cold_max={} leading_zeros={leading_zeros}",
        difference.0,
        cold_difference.0,
    );
}

#[test]
fn coupled_rate_tuple_waits_for_unity_owner_and_preserves_an_independent_productive_voice() {
    let (mut fixture, mut parameters) = parameter_fixture();
    let (mut reference, mut reference_parameters) = parameter_fixture();
    for (current, consumer) in [
        (&mut fixture, &mut parameters),
        (&mut reference, &mut reference_parameters),
    ] {
        current.control.engine.set_speed(2.0).unwrap();
        drain_rate(current, consumer);
        let other = current.capture(1, "pitch.extra:7:set_retrigger");
        current.publish(&other);
        current.apply(&other);
    }
    let birth = public_unity_birth(&mut fixture);
    let birth_frame = birth.execution_target();
    assert_eq!(reference.transport.output_frame(), birth_frame);
    fixture.pcm(DRY_FRAMES);
    reference.pcm(DRY_FRAMES);
    let other = fixture.voice(1);
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_request = other.stretch.adopted_request_id();
    assert!(
        other
            .stretch
            .owns_pitch_contract(PitchIntent::new(0, 7).unwrap(), true)
    );
    assert!(other_request.is_some());
    let old_transport_period = fixture.transport.master_period_seconds();
    let capture = fixture.transport.output_frame();

    fixture
        .control
        .engine
        .set_speed_and_master_period(1.8, 0.625)
        .unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let preparing = rate_status(&fixture);
    let execution = capture + HISTORY_FRAMES as u64;
    assert_eq!(preparing.state, "preparing", "{preparing:?}");
    assert_eq!(preparing.execution_frame, Some(execution));
    assert_eq!(preparing.requested_speed, 1.8);
    assert_eq!(preparing.requested_master_period, Some(0.625));
    assert_eq!(preparing.effective_speed, 2.0);
    assert_eq!(preparing.effective_master_period, None);
    assert_eq!(
        fixture.transport.master_period_seconds(),
        old_transport_period
    );
    wait_source_target(&mut fixture, 0, execution);

    // Compare B with its independent uninterrupted mixer, subtracting only the
    // independently calculated dry-A signal. Both settings remain old before E.
    let actual = fixture.render_to(execution);
    let unchanged_other = reference.render_to(execution);
    let isolated_other: Vec<_> = actual
        .iter()
        .enumerate()
        .map(|(offset, sample)| {
            *sample - scalar_source(LOOP_START as f64 + (DRY_FRAMES + offset) as f64 * 2.0)
        })
        .collect();
    assert!(pcm_difference(&isolated_other, &unchanged_other).0 < 2.0e-5);
    assert_eq!(
        fixture.transport.master_period_seconds(),
        old_transport_period
    );
    assert_eq!(
        fixture.voice(1).stretch.native_state_address(),
        other_native
    );
    assert_eq!(fixture.voice(1).stretch.adopted_request_id(), other_request);

    fixture.render(0);
    let applied = rate_status(&fixture);
    assert_eq!(applied.state, "applied", "{applied:?}");
    assert_eq!(applied.effect_revision, preparing.request_revision);
    assert_eq!(applied.effective_speed, 1.8);
    assert_eq!(applied.effective_master_period, Some(0.625));
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.625));
    assert_eq!(
        fixture.voice(1).stretch.native_state_address(),
        other_native
    );
    assert_eq!(fixture.voice(1).stretch.adopted_request_id(), other_request);
    assert_eq!(fixture.voice(1).generation, other_generation);
    assert_eq!(fixture.starts(1), 1);

    // B is already productive, so the same public tuple applies immediately to
    // its independent reference at this exact E. Pausing A isolates B's bytes.
    fixture.mixer.pause_sample_at_output_frame(0, execution);
    reference
        .control
        .engine
        .set_speed_and_master_period(1.8, 0.625)
        .unwrap();
    drain_rate(&mut reference, &mut reference_parameters);
    assert_eq!(rate_status(&reference).state, "applied");
    let actual_other = fixture.render(FIRST_CHUNK);
    let expected_other = reference.render(FIRST_CHUNK);
    assert_eq!(actual_other, expected_other);
    assert!(
        fixture
            .voice(1)
            .source_playback
            .matches_exact(&reference.voice(1).source_playback)
    );
    assert_eq!(
        fixture.voice(1).stretch.pending_fifo_frames(),
        reference.voice(1).stretch.pending_fifo_frames()
    );
    assert_eq!(
        fixture.voice(1).stretch.native_state_address(),
        other_native
    );
    assert_eq!(fixture.voice(1).stretch.adopted_request_id(), other_request);
    assert_eq!(fixture.starts(0), 1);
    assert_eq!(fixture.starts(1), 1);
}

#[test]
fn public_stop_rejects_pending_rate_without_late_effect_and_same_worker_accepts_a_fresh_birth() {
    let (mut fixture, mut parameters) = parameter_fixture();
    fixture.control.engine.set_speed(2.0).unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let original = public_unity_birth(&mut fixture);
    fixture.pcm(DRY_FRAMES);
    fixture.control.engine.set_speed(1.8).unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let pending = rate_status(&fixture);
    let old_execution = pending.execution_frame.unwrap();
    assert_eq!(pending.state, "preparing");
    wait_source_target(&mut fixture, 0, old_execution);
    fixture.control.engine.stop_sample(0).unwrap();
    fixture.drain();
    fixture.render(0);
    let rejected = rate_status(&fixture);
    assert_eq!(rejected.state, "rejected", "{rejected:?}");
    assert_eq!(rejected.request_revision, pending.request_revision);
    assert_eq!(rejected.effect_revision, pending.request_revision);
    assert_eq!(rejected.effective_speed, 2.0);
    assert!(rejected.error.is_some());
    assert!(
        fixture
            .render_to(old_execution + FIRST_CHUNK as u64)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(rate_status(&fixture).state, "rejected");
    assert!(original.effective());

    let fresh = public_unity_birth(&mut fixture);
    assert!(fresh.sequence() > original.sequence());
    fixture.pcm(733);
    let generation = fixture.voice(0).generation;
    fixture.control.engine.set_speed(1.6).unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let retry = rate_status(&fixture);
    assert_eq!(retry.state, "preparing", "{retry:?}");
    assert!(retry.request_revision > pending.request_revision);
    let execution = retry.execution_frame.unwrap();
    wait_source_target(&mut fixture, 0, execution);
    fixture.render_to(execution);
    fixture.render(0);
    let applied = rate_status(&fixture);
    assert_eq!(applied.state, "applied", "{applied:?}");
    assert_eq!(applied.request_revision, retry.request_revision);
    assert_eq!(applied.effect_revision, retry.request_revision);
    assert_eq!(applied.effective_speed, 1.6);
    assert!(applied.error.is_none());
    let first = fixture.render(FIRST_CHUNK);
    assert!(
        fixture
            .voice(0)
            .stretch
            .owns_pitch_contract(fresh.render().pitch, true)
    );
    assert!(fixture.voice(0).stretch.wet_current_source_output_ready());
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), 2);
    assert!(first.iter().all(|sample| sample.is_finite()));
}

#[test]
fn superseded_rate_identity_cannot_apply_at_its_old_deadline_or_replace_the_latest_tuple() {
    let (mut fixture, mut parameters) = parameter_fixture();
    fixture.control.engine.set_speed(2.0).unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let birth = public_unity_birth(&mut fixture);
    fixture.pcm(DRY_FRAMES);
    let generation = fixture.voice(0).generation;
    let before = fixture.voice(0).source_playback;
    let original_capture = fixture.transport.output_frame();
    fixture.control.engine.set_speed(1.8).unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let first = rate_status(&fixture);
    let old_execution = first.execution_frame.unwrap();
    assert_eq!(first.state, "preparing");
    wait_source_target(&mut fixture, 0, old_execution);
    let mut all_dry = fixture.render(127);
    fixture.control.engine.set_speed(1.6).unwrap();
    assert_eq!(rate_status(&fixture).state, "queued");
    drain_rate(&mut fixture, &mut parameters);
    let latest = rate_status(&fixture);
    assert_eq!(latest.state, "preparing", "{latest:?}");
    assert!(latest.request_revision > first.request_revision);
    assert_eq!(latest.effect_revision, latest.request_revision);
    assert_eq!(latest.requested_speed, 1.6);
    assert_eq!(latest.effective_speed, 2.0);
    let execution = latest.execution_frame.unwrap();
    assert_eq!(execution, old_execution + 127);
    wait_source_target(&mut fixture, 0, execution);
    all_dry.extend(fixture.render_to(old_execution + 1));
    let after_old_deadline = rate_status(&fixture);
    assert_eq!(
        after_old_deadline.state, "preparing",
        "{after_old_deadline:?}"
    );
    assert_eq!(after_old_deadline.request_revision, latest.request_revision);
    assert_eq!(after_old_deadline.effect_revision, latest.request_revision);
    assert_eq!(after_old_deadline.effective_speed, 2.0);
    assert!(!fixture.voice(0).stretch.loop_acceptance_state().0);
    assert_eq!(fixture.voice(0).stretch.adopted_request_id(), None);
    all_dry.extend(fixture.render_to(execution));
    let expected: Vec<_> = (0..(execution - original_capture) as usize)
        .map(|frame| scalar_source(LOOP_START as f64 + (DRY_FRAMES + frame) as f64 * 2.0))
        .collect();
    assert!(pcm_difference(&all_dry, &expected).0 < 2.0e-5);
    let mut expected_clock = before;
    expected_clock.advance(all_dry.len());
    assert!(
        fixture
            .voice(0)
            .source_playback
            .matches_exact(&expected_clock)
    );
    fixture.render(0);
    let applied = rate_status(&fixture);
    assert_eq!(applied.state, "applied", "{applied:?}");
    assert_eq!(applied.request_revision, latest.request_revision);
    assert_eq!(applied.effect_revision, latest.request_revision);
    assert_eq!(applied.effective_speed, 1.6);
    assert_eq!(applied.execution_frame, Some(execution));
    assert!(applied.error.is_none());
    fixture.render(FIRST_CHUNK);
    assert_eq!(fixture.voice(0).source_playback.rate_target(), 1.6);
    assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), 1.95);
    assert!(
        fixture
            .voice(0)
            .stretch
            .owns_pitch_contract(birth.render().pitch, true)
    );
    assert!(fixture.voice(0).stretch.wet_current_source_output_ready());
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), 1);
    assert!(birth.effective());
}

/// Complete a newly captured dry-unity rate request against the declared finite
/// actual-history oracle, including its first irregular output and adapter state.
fn complete_rate_from_dry_unity(fixture: &mut Fixture, request: &RateStatus) -> RateStatus {
    let execution = request.execution_frame.unwrap();
    assert_eq!(request.state, "preparing");
    assert_eq!(
        execution,
        fixture.transport.output_frame() + HISTORY_FRAMES as u64
    );
    let before = fixture.voice(0).source_playback;
    assert_eq!(before.tempo_ratio(), 2.0);
    assert_eq!(before.rate_target(), 2.0);
    let origin = before.position();
    let phase = origin.frame as f64 + origin.fraction;
    let generation = fixture.voice(0).generation;
    wait_source_target(fixture, 0, execution);
    let old_expected: Vec<_> = (0..HISTORY_FRAMES)
        .map(|frame| scalar_source(phase + frame as f64 * 2.0))
        .collect();
    let old_actual = fixture.render_to(execution);
    assert!(pcm_difference(&old_actual, &old_expected).0 < 2.0e-5);
    fixture.render(0);
    let applied = rate_status(fixture);
    assert_eq!(applied.state, "applied", "{applied:?}");
    assert_eq!(applied.effect_revision, request.request_revision);
    assert_eq!(applied.request_revision, request.request_revision);
    assert_eq!(applied.effective_speed, request.requested_speed);
    assert_eq!(
        applied.effective_master_period,
        request.requested_master_period
    );
    assert!(applied.error.is_none());

    let mut raw = DirectNative::new(1.0);
    for sample in old_expected {
        raw.sample(sample, 1.0);
    }
    let mut expected_clock = before;
    expected_clock.advance(HISTORY_FRAMES);
    expected_clock.set_target(request.requested_speed);
    let (frames, actual_rate) = expected_clock.chunk(FIRST_CHUNK);
    assert_eq!(frames, FIRST_CHUNK);
    let expected: Vec<_> = (0..FIRST_CHUNK)
        .map(|frame| {
            raw.sample(
                scalar_source(phase + HISTORY_FRAMES as f64 * 2.0 + frame as f64 * actual_rate),
                2.0 / actual_rate,
            )
        })
        .collect();
    expected_clock.advance(FIRST_CHUNK);
    let actual = fixture.render(FIRST_CHUNK);
    let difference = pcm_difference(&actual, &expected);
    assert!(difference.0 < 2.0e-5, "recovered first PCM={difference:?}");
    let voice = fixture.voice(0);
    assert!(
        voice
            .stretch
            .owns_pitch_contract(PitchIntent::new(0, 12).unwrap(), true)
    );
    assert!(voice.stretch.wet_current_source_output_ready());
    assert_eq!(
        voice.stretch.pending_fifo_frames(),
        (raw.input.len(), raw.output.len())
    );
    assert_eq!(voice.generation, generation);
    assert!(voice.source_playback.matches_exact(&expected_clock));
    applied
}

#[test]
fn actual_parameter_only_drain_cannot_borrow_a_previous_command_drain_revision() {
    let (mut fixture, mut parameters) = parameter_fixture();
    fixture.control.engine.set_speed(2.0).unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let birth = public_unity_birth(&mut fixture);
    fixture.pcm(DRY_FRAMES);
    let before = fixture.voice(0).source_playback;
    let epoch = fixture.voice(0).stretch.preparation_epoch();
    let generation = fixture.voice(0).generation;

    fixture.control.engine.set_speed(1.8).unwrap();
    // This intentionally exercises only one physical callback drain. The empty
    // command drain from an older global revision cannot certify this request.
    drain_parameter_messages(&mut parameters, &mut fixture.mixer, &mut fixture.transport);
    fixture.render(0);
    let incomplete = rate_status(&fixture);
    assert_eq!(incomplete.state, "preparing", "{incomplete:?}");
    assert_eq!(incomplete.execution_frame, None);
    assert_eq!(incomplete.requested_speed, 1.8);
    assert_eq!(incomplete.effective_speed, 2.0);
    assert_eq!(fixture.voice(0).stretch.preparation_epoch(), epoch);
    assert_eq!(fixture.voice(0).stretch.adopted_request_id(), None);
    assert!(fixture.voice(0).source_playback.matches_exact(&before));
    let old_actual = fixture.render(FIRST_CHUNK);
    let old_expected: Vec<_> = (0..FIRST_CHUNK)
        .map(|frame| scalar_source(LOOP_START as f64 + (DRY_FRAMES + frame) as f64 * 2.0))
        .collect();
    assert!(pcm_difference(&old_actual, &old_expected).0 < 2.0e-5);
    assert_eq!(rate_status(&fixture).execution_frame, None);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.voice(0).stretch.preparation_epoch(), epoch);

    let capture_frame = fixture.transport.output_frame();
    fixture.drain();
    fixture.render(0);
    let captured = rate_status(&fixture);
    assert_eq!(captured.request_revision, incomplete.request_revision);
    assert_eq!(
        captured.execution_frame,
        Some(capture_frame + HISTORY_FRAMES as u64)
    );
    complete_rate_from_dry_unity(&mut fixture, &captured);
    assert!(birth.effective());
    assert_eq!(fixture.starts(0), 1);
}

#[test]
fn completed_publisher_cannot_lend_its_unread_control_payload_to_a_late_drain_finish() {
    use crate::messages::ControlMessage;

    let (mut fixture, mut parameters) = parameter_fixture();
    fixture.control.engine.set_speed(1.8).unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let original_rate = rate_status(&fixture);
    assert_eq!(original_rate.state, "applied", "{original_rate:?}");
    assert_eq!(
        original_rate.effect_revision,
        original_rate.request_revision
    );
    assert!(fixture.mixer.global_rate_drains_settled());
    assert!(fixture.control.consumer.is_empty() && parameters.is_empty());
    assert_eq!(fixture.transport.bootstrap_reference(), None);

    let ownership = fixture.control.engine.input_runtime_ownership.clone();
    let published_revision = fixture
        .mixer
        .global_rate_timing_revision()
        .checked_add(1)
        .unwrap();
    assert!(published_revision > original_rate.request_revision);
    // The real control drain clears its completion flag and captures its own
    // beginning token before its first queue observation.
    fixture.mixer.set_global_commands_drained(false);
    let drain_token = fixture.mixer.begin_global_timing_drain();
    assert_eq!(
        drain_token,
        Some(fixture.mixer.global_rate_timing_revision())
    );
    // These sequentially consistent stages are one legal ordering of the real
    // push_global_timing_message publisher: flag, revision, queue push, flag.
    // Suspend it before the push, after the consumer's final unsuccessful pop.
    ownership
        .global_timing_publishing
        .store(true, Ordering::SeqCst);
    ownership
        .global_timing_revision
        .store(published_revision, Ordering::SeqCst);
    assert!(fixture.control.consumer.pop().is_err());
    // Retain the old post-loop revision only as a diagnostic: it must never
    // replace this drain's beginning token. Neither it nor the saved empty
    // result consumed the publisher's payload.
    let drained_revision = fixture.mixer.global_rate_timing_revision();
    let observed_empty = fixture.control.consumer.is_empty();
    assert_eq!(drained_revision, published_revision);
    assert!(observed_empty);
    assert!(ownership.global_timing_publishing.load(Ordering::SeqCst));
    fixture
        .control
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::BootstrapTransportFromPad { id: 0 })
        .unwrap();
    ownership
        .global_timing_publishing
        .store(false, Ordering::SeqCst);

    // The other real drain can now observe the completed publication's revision.
    // Its empty parameter queue cannot make the unread control payload consumed.
    drain_parameter_messages(&mut parameters, &mut fixture.mixer, &mut fixture.transport);
    assert!(matches!(
        fixture.control.consumer.peek(),
        Ok(ControlMessage::BootstrapTransportFromPad { id: 0 })
    ));
    fixture
        .mixer
        .finish_global_control_drain(drain_token, observed_empty);
    let unread_rate = rate_status(&fixture);
    assert_eq!(unread_rate.request_revision, original_rate.request_revision);
    assert_eq!(unread_rate.effect_revision, original_rate.effect_revision);
    assert_eq!(fixture.transport.bootstrap_reference(), None);
    assert!(
        !fixture.mixer.global_rate_drains_settled(),
        "the late finish borrowed a completed publisher's revision while its original control payload remained unread"
    );

    // After genuine subsequent drains, the exact queued payload takes effect and
    // both sides may settle. A timing-only publication is never a new rate ID.
    fixture.drain();
    drain_parameter_messages(&mut parameters, &mut fixture.mixer, &mut fixture.transport);
    assert!(fixture.control.consumer.is_empty() && parameters.is_empty());
    assert_eq!(fixture.transport.bootstrap_reference(), Some(0));
    assert!(fixture.mixer.global_rate_drains_settled());
    let consumed_rate = rate_status(&fixture);
    assert_eq!(consumed_rate.state, "applied", "{consumed_rate:?}");
    assert_eq!(
        consumed_rate.request_revision,
        original_rate.request_revision
    );
    assert_eq!(consumed_rate.effect_revision, original_rate.effect_revision);
    assert_eq!(consumed_rate.effective_speed, original_rate.effective_speed);
    assert_eq!(
        fixture.mixer.global_rate_timing_revision(),
        published_revision
    );
}

#[test]
fn bounded_partial_parameter_drain_cannot_borrow_later_full_tuple_or_capture_its_old_identity() {
    use crate::audio_engine::audio_stream::MAX_PARAMETER_MESSAGES_PER_CALLBACK;

    let (mut fixture, mut initial_parameters) = parameter_fixture();
    fixture.control.engine.set_speed(2.0).unwrap();
    drain_rate(&mut fixture, &mut initial_parameters);
    let birth = public_unity_birth(&mut fixture);
    fixture.pcm(DRY_FRAMES);
    let before = fixture.voice(0).source_playback;
    let epoch = fixture.voice(0).stretch.preparation_epoch();
    let (producer, mut parameters) =
        RingBuffer::<ControlParameterMessage>::new(MAX_PARAMETER_MESSAGES_PER_CALLBACK + 2);
    let producer = Arc::new(Mutex::new(producer));
    fixture.control.engine.test_parameter_producer = Some(producer.clone());

    fixture.control.engine.set_speed(1.8).unwrap();
    let first_revision = rate_status(&fixture).request_revision;
    // Neutral entries occupy actual bounded mailbox slots; all meaningful rate
    // payloads and identities still originate in the real public methods.
    {
        let mut producer = producer.lock().unwrap();
        for _ in 1..MAX_PARAMETER_MESSAGES_PER_CALLBACK {
            producer
                .push(ControlParameterMessage::SetVolume(1.0))
                .unwrap();
        }
    }
    fixture.control.engine.set_master_period(0.625).unwrap();
    let latest = rate_status(&fixture);
    assert!(latest.request_revision > first_revision);
    assert_eq!(latest.requested_speed, 1.8);
    assert_eq!(latest.requested_master_period, Some(0.625));

    drain_parameter_messages(&mut parameters, &mut fixture.mixer, &mut fixture.transport);
    assert!(
        !parameters.is_empty(),
        "the later public period is genuinely undrained"
    );
    fixture.drain();
    fixture.render(0);
    let partial = rate_status(&fixture);
    assert_eq!(partial.state, "queued", "{partial:?}");
    assert_eq!(partial.request_revision, latest.request_revision);
    assert_eq!(partial.effect_revision, first_revision);
    assert_eq!(partial.execution_frame, None);
    assert_eq!(partial.effective_speed, 2.0);
    assert_eq!(partial.effective_master_period, None);
    assert_eq!(fixture.voice(0).stretch.preparation_epoch(), epoch);
    assert_eq!(fixture.voice(0).stretch.adopted_request_id(), None);
    assert!(fixture.voice(0).source_playback.matches_exact(&before));

    drain_rate(&mut fixture, &mut parameters);
    assert!(parameters.is_empty());
    let captured = rate_status(&fixture);
    assert_eq!(captured.state, "preparing", "{captured:?}");
    assert_eq!(captured.request_revision, latest.request_revision);
    assert_eq!(captured.effect_revision, latest.request_revision);
    assert_eq!(captured.requested_speed, 1.8);
    assert_eq!(captured.requested_master_period, Some(0.625));
    let applied = complete_rate_from_dry_unity(&mut fixture, &captured);
    assert_eq!(applied.effective_speed, 1.8);
    assert_eq!(applied.effective_master_period, Some(0.625));
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.625));
    assert!(birth.effective());
    assert_eq!(fixture.starts(0), 1);
}

#[test]
fn public_pause_refuses_the_whole_pending_tuple_and_resume_recovers_exact_history_on_same_lane() {
    let (mut fixture, mut parameters) = parameter_fixture();
    fixture.control.engine.set_speed(2.0).unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let birth = public_unity_birth(&mut fixture);
    fixture.pcm(DRY_FRAMES);
    let before = fixture.voice(0).source_playback;
    let generation = fixture.voice(0).generation;
    let old_period = fixture.transport.master_period_seconds();
    fixture
        .control
        .engine
        .set_speed_and_master_period(1.8, 0.625)
        .unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let original = rate_status(&fixture);
    let old_execution = original.execution_frame.unwrap();
    wait_source_target(&mut fixture, 0, old_execution);

    fixture.control.engine.pause_sample(0).unwrap();
    fixture.drain();
    fixture.render(0);
    let rejected = rate_status(&fixture);
    assert_eq!(rejected.state, "rejected", "{rejected:?}");
    assert_eq!(rejected.effect_revision, original.request_revision);
    assert_eq!(rejected.requested_speed, 1.8);
    assert_eq!(rejected.requested_master_period, Some(0.625));
    assert_eq!(rejected.effective_speed, 2.0);
    assert_eq!(rejected.effective_master_period, None);
    assert_eq!(fixture.transport.master_period_seconds(), old_period);
    assert!(rejected.error.is_some());
    assert!(fixture.voice(0).paused);
    assert!(
        fixture
            .render_to(old_execution + FIRST_CHUNK as u64)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(rate_status(&fixture).state, "rejected");
    assert!(fixture.voice(0).source_playback.matches_exact(&before));
    assert_eq!(fixture.voice(0).generation, generation);

    fixture.control.engine.resume_sample(0).unwrap();
    fixture.drain();
    fixture.render(0);
    assert!(!fixture.voice(0).paused);
    assert!(fixture.voice(0).source_playback.matches_exact(&before));
    fixture
        .control
        .engine
        .set_speed_and_master_period(1.6, 0.75)
        .unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let retry = rate_status(&fixture);
    assert!(retry.request_revision > original.request_revision);
    let applied = complete_rate_from_dry_unity(&mut fixture, &retry);
    assert_eq!(applied.effective_speed, 1.6);
    assert_eq!(applied.effective_master_period, Some(0.75));
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.75));
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), 1);
    assert!(birth.effective());
}

#[test]
fn public_bpm_lock_off_updates_effective_period_without_relabelling_the_applied_rate_request() {
    let (mut fixture, mut parameters) = parameter_fixture();
    fixture
        .control
        .engine
        .set_speed_and_master_period(1.8, 0.625)
        .unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let applied = rate_status(&fixture);
    assert_eq!(applied.state, "applied", "{applied:?}");
    assert_eq!(applied.request_revision, applied.effect_revision);
    assert_eq!(applied.requested_speed, 1.8);
    assert_eq!(applied.requested_master_period, Some(0.625));
    assert_eq!(applied.effective_master_period, Some(0.625));
    assert_eq!(applied.effective_speed, 1.8);
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));

    fixture.control.engine.set_bpm_lock(false).unwrap();
    fixture.drain();
    let cleared = rate_status(&fixture);
    assert_eq!(cleared.state, "applied", "{cleared:?}");
    assert_eq!(cleared.request_revision, applied.request_revision);
    assert_eq!(cleared.effect_revision, applied.effect_revision);
    assert_eq!(cleared.requested_speed, 1.8);
    assert_eq!(cleared.requested_master_period, Some(0.625));
    assert_eq!(cleared.effective_speed, 1.8);
    assert_eq!(cleared.effective_master_period, None);
    assert!(cleared.error.is_none());
    assert_eq!(fixture.transport.output_frame(), 0);

    let transport_period_after_clear = fixture.transport.master_period_seconds();
    fixture.control.engine.set_speed(1.6).unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let next = rate_status(&fixture);
    assert_eq!(next.state, "applied", "{next:?}");
    assert!(next.request_revision > cleared.request_revision);
    assert_eq!(next.effect_revision, next.request_revision);
    assert_eq!(next.requested_speed, 1.6);
    assert_eq!(next.requested_master_period, None);
    assert_eq!(next.effective_speed, 1.6);
    assert_eq!(next.effective_master_period, None);
    assert_eq!(
        fixture.transport.master_period_seconds(),
        transport_period_after_clear
    );
    assert_eq!(fixture.transport.output_frame(), 0);
}

#[test]
fn foreign_second_history_lane_refuses_both_unity_voices_before_work_and_both_recover_together() {
    let (mut fixture, mut parameters) = parameter_fixture();
    fixture.control.engine.set_speed(2.0).unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let first_birth = public_unity_birth(&mut fixture);
    let second_birth = fixture.capture(1, "pitch.extra:12:set_retrigger");
    fixture.publish(&second_birth);
    fixture.render(0);
    assert!(second_birth.effective());
    assert_eq!(
        second_birth.execution_target(),
        second_birth.original_due_frame()
    );
    fixture.pcm(DRY_FRAMES);
    let generations = [0, 1].map(|id| fixture.voice(id).generation);
    let native = [0, 1].map(|id| fixture.voice(id).stretch.native_state_address());
    let first_epoch = fixture.voice(0).stretch.preparation_epoch();
    for id in [0, 1] {
        assert_eq!(fixture.voice(id).source_playback.tempo_ratio(), 2.0);
        assert_eq!(
            fixture.mixer.pad_pitch_intent(id),
            Some(PitchIntent::new(0, 12).unwrap())
        );
        assert!(
            !fixture
                .voice(id)
                .stretch
                .owns_pitch_contract(PitchIntent::new(0, 12).unwrap(), true)
        );
        assert!(!fixture.voice(id).stretch.loop_acceptance_state().0);
    }

    // A real public, not-yet-effective OFF request occupies B's actual source
    // worker lane. Its genuine ready candidate must retain that foreign owner.
    let capture_frame = fixture.transport.output_frame();
    let foreign = fixture.control.engine.set_pad_key_lock(1, false).unwrap();
    fixture.drain();
    // Existing mode capture runs inside a real mixer segment; a zero-frame
    // outer render services global requests but never enters mode polling.
    let capture_pcm = fixture.render(1);
    let capture_phase = LOOP_START as f64 + DRY_FRAMES as f64 * 2.0;
    let capture_expected =
        scalar_source_hz(capture_phase, SOURCE_HZ) + scalar_source_hz(capture_phase, 320.0);
    assert!((capture_pcm[0] - capture_expected).abs() < 2.0e-5);
    let foreign_execution = capture_frame + HISTORY_FRAMES as u64;
    wait_source_target(&mut fixture, 1, foreign_execution);
    assert!(fixture.voice(1).stretch.source_lane_busy());
    assert_eq!(fixture.voice(1).stretch.adopted_request_id(), None);
    assert_eq!(fixture.voice(1).stretch.native_state_address(), native[1]);
    let old_period = fixture.transport.master_period_seconds();

    fixture
        .control
        .engine
        .set_speed_and_master_period(1.8, 0.625)
        .unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let rejected = rate_status(&fixture);
    assert_eq!(rejected.state, "rejected", "{rejected:?}");
    assert!(
        rejected
            .error
            .as_deref()
            .is_some_and(|error| error.contains("capacity"))
    );
    assert_eq!(rejected.requested_speed, 1.8);
    assert_eq!(rejected.requested_master_period, Some(0.625));
    assert_eq!(rejected.effective_speed, 2.0);
    assert_eq!(rejected.effective_master_period, None);
    assert_eq!(rejected.execution_frame, None);
    assert_eq!(fixture.transport.master_period_seconds(), old_period);
    // The unavailable second lane is found before A's epoch or work is changed.
    assert_eq!(fixture.voice(0).stretch.preparation_epoch(), first_epoch);
    for id in [0, 1] {
        assert_eq!(fixture.voice(id).stretch.native_state_address(), native[id]);
        assert_eq!(fixture.voice(id).stretch.adopted_request_id(), None);
        assert_eq!(fixture.voice(id).generation, generations[id]);
        assert!(!fixture.voice(id).stretch.loop_acceptance_state().0);
    }

    // Supersede only B's foreign mode using its real public request path. Both
    // original unity voices then emit the same old PCM past the abandoned E.
    let restore = fixture.control.engine.set_pad_key_lock(1, true).unwrap();
    assert!(restore > foreign);
    fixture.drain();
    fixture.render(0);
    let old_actual = fixture.render_to(foreign_execution + FIRST_CHUNK as u64);
    let old_expected: Vec<_> = (0..old_actual.len())
        .map(|offset| {
            let phase = LOOP_START as f64 + (DRY_FRAMES + 1 + offset) as f64 * 2.0;
            scalar_source_hz(phase, SOURCE_HZ) + scalar_source_hz(phase, 320.0)
        })
        .collect();
    assert!(pcm_difference(&old_actual, &old_expected).0 < 2.0e-5);
    assert_eq!(rate_status(&fixture).state, "rejected");
    assert_eq!(rate_status(&fixture).effective_speed, 2.0);
    for id in [0, 1] {
        assert_eq!(fixture.voice(id).stretch.native_state_address(), native[id]);
        assert_eq!(fixture.voice(id).stretch.adopted_request_id(), None);
        assert_eq!(fixture.voice(id).generation, generations[id]);
        assert_eq!(fixture.starts(id), 1);
    }

    let before = [0, 1].map(|id| fixture.voice(id).source_playback);
    fixture
        .control
        .engine
        .set_speed_and_master_period(1.6, 0.75)
        .unwrap();
    drain_rate(&mut fixture, &mut parameters);
    let retry = rate_status(&fixture);
    assert_eq!(retry.state, "preparing", "{retry:?}");
    assert!(retry.request_revision > rejected.request_revision);
    let execution = retry.execution_frame.unwrap();
    wait_source_target(&mut fixture, 0, execution);
    wait_source_target(&mut fixture, 1, execution);
    let phases = before.map(|clock| {
        let position = clock.position();
        position.frame as f64 + position.fraction
    });
    let mut direct = [DirectNative::new(1.0), DirectNative::new(1.0)];
    let mut pending_expected = Vec::with_capacity(HISTORY_FRAMES);
    for frame in 0..HISTORY_FRAMES {
        let samples =
            [SOURCE_HZ, 320.0].map(|hz| scalar_source_hz(phases[0] + frame as f64 * 2.0, hz));
        pending_expected.push(samples[0] + samples[1]);
        for id in [0, 1] {
            direct[id].sample(samples[id], 1.0);
        }
    }
    let pending_actual = fixture.render_to(execution);
    assert!(pcm_difference(&pending_actual, &pending_expected).0 < 2.0e-5);
    for id in [0, 1] {
        assert_eq!(fixture.voice(id).stretch.native_state_address(), native[id]);
        assert_eq!(fixture.voice(id).stretch.adopted_request_id(), None);
    }
    fixture.render(0);
    let applied = rate_status(&fixture);
    assert_eq!(applied.state, "applied", "{applied:?}");
    assert_eq!(applied.effect_revision, retry.request_revision);
    assert_eq!(applied.effective_speed, 1.6);
    assert_eq!(applied.effective_master_period, Some(0.75));
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.75));
    let mut expected_clocks = before;
    for clock in &mut expected_clocks {
        clock.advance(HISTORY_FRAMES);
        clock.set_target(1.6);
        assert_eq!(clock.chunk(FIRST_CHUNK), (FIRST_CHUNK, 1.95));
        clock.advance(FIRST_CHUNK);
    }
    let mut expected = [Vec::new(), Vec::new()];
    for frame in 0..FIRST_CHUNK {
        for id in [0, 1] {
            let hz = [SOURCE_HZ, 320.0][id];
            expected[id].push(direct[id].sample(
                scalar_source_hz(
                    phases[id] + HISTORY_FRAMES as f64 * 2.0 + frame as f64 * 1.95,
                    hz,
                ),
                2.0 / 1.95,
            ));
        }
    }
    let first_actual = fixture.render(FIRST_CHUNK);
    let first_expected: Vec<_> = expected[0]
        .iter()
        .zip(&expected[1])
        .map(|(a, b)| a + b)
        .collect();
    assert!(pcm_difference(&first_actual, &first_expected).0 < 2.0e-5);
    for id in [0, 1] {
        let voice = fixture.voice(id);
        assert!(
            voice
                .stretch
                .owns_pitch_contract(PitchIntent::new(0, 12).unwrap(), true)
        );
        assert!(voice.stretch.wet_current_source_output_ready());
        assert!(
            pcm_difference(
                &voice.stretch.output_buffers()[0][..FIRST_CHUNK],
                &expected[id]
            )
            .0 < 2.0e-5
        );
        assert_eq!(
            voice.stretch.pending_fifo_frames(),
            (direct[id].input.len(), direct[id].output.len())
        );
        assert!(voice.source_playback.matches_exact(&expected_clocks[id]));
        assert_eq!(voice.generation, generations[id]);
        assert_eq!(fixture.starts(id), 1);
    }
    assert!(first_birth.effective() && second_birth.effective());
}
