//! Independent source beginning through captured public SET_RETRIGGER actions.
//! This proves dry source content and clock ownership, not musical Native alignment.

use super::*;
use crate::audio_engine::key_lock_start_policy::KeyLockStartBehavior;
use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_sources;
use crate::audio_engine::pitch_actions::PitchActionVariant;
use pyo3::prelude::*;
use pyo3::types::PyInt;

const ATTACK_FRAMES: usize = 192;
const OBSERVE_FRAMES: usize = 384;

#[derive(Clone, Copy, Debug)]
enum Previous {
    Stopped,
    Running,
    Paused,
}

// One unique, nonperiodic attack followed by silence. Neither a DUT playhead nor
// a Native/history/delay measurement defines the expected source beginning.
fn source_value(frame: usize) -> f32 {
    if (LOOP_START..LOOP_START + ATTACK_FRAMES).contains(&frame) {
        let index = frame - LOOP_START;
        0.16 + ((index * index * 17 + index * 43 + 11) % 251) as f32 / 1_024.0
    } else {
        0.0
    }
}

fn expected_sample(output_frame: usize, rate: f64) -> f32 {
    let position = LOOP_START as f64 + output_frame as f64 * rate;
    let left = position.floor() as usize;
    let fraction = (position - left as f64) as f32;
    source_value(left) * (1.0 - fraction) + source_value(left + 1) * fraction
}

fn configured(rate: f64, policy: KeyLockStartBehavior, previous: Previous) -> Fixture {
    let mut fixture = Fixture::new(false);
    let source = SampleBuffer {
        channels: 1,
        samples: Arc::from((0..3 * RATE as usize).map(source_value).collect::<Vec<_>>()),
        residency: None,
    }
    .with_complete_source(RATE);
    publish_tone(&fixture.control, 0, source, true);
    adopt_current_sources(&mut fixture, false);
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    for id in [0, 1] {
        loaded[id] = true;
        starts[id] = LOOP_START as f64 / f64::from(RATE);
        ends[id] = Some(LOOP_END as f64 / f64::from(RATE));
        bindings[id] = fixture
            .control
            .engine
            .current_input_runtime_pad_binding(id)
            .unwrap();
        rows[id].source_root = Some(0);
        rows[id].source_mode = Some(PitchKeyMode::Major);
        rows[id].key_lock = false;
        fixture.mixer.set_pad_bpm(id, Some(120.0));
    }
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
    // Publish the real stopped callback observation required by the policy gate.
    fixture.pcm(1);
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior(policy.as_str())
            .unwrap()
    );
    fixture.mixer.set_speed(rate);
    fixture.mixer.play_sample(1, 0.25);
    if !matches!(previous, Previous::Stopped) {
        fixture.mixer.play_sample(0, 1.0);
    }
    fixture.pcm(512);
    if matches!(previous, Previous::Paused) {
        fixture
            .mixer
            .pause_sample_at_output_frame(0, fixture.transport.output_frame());
    }
    fixture.pcm(512);
    fixture
}

fn capture_at_current_frame(fixture: &Fixture, key: &str) -> CapturedPitchAction {
    Python::initialize();
    Python::attach(|py| {
        fixture.refresh_clock();
        let timestamp = PyInt::new(py, fixture.clock.read().unwrap().audible_at_ns);
        let action = fixture
            .control
            .engine
            .capture_pad_pitch_action(0, key, Some(timestamp.as_any()))
            .unwrap();
        assert_eq!(
            action.original_due_frame(),
            fixture.transport.output_frame()
        );
        action
    })
}

fn publish_ready(fixture: &mut Fixture, action: &CapturedPitchAction) {
    assert!(fixture.control.engine.publish_pitch_action(action).unwrap());
    fixture.drain();
    assert!(action.accepted(), "{:?}", action.reason());
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WORKER_WAIT;
    while !action.effective()
        && !fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(action.sequence())
    {
        fixture.mixer.pitch_runtime.as_mut().unwrap().poll();
        assert_ne!(action.state(), "rejected", "{:?}", action.reason());
        assert!(Instant::now() < deadline, "bounded pitch preparation wait");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
}

fn complete_attack(
    rate: f64,
    policy: KeyLockStartBehavior,
    previous: Previous,
    delay: usize,
    key: &str,
) {
    let mut actual = configured(rate, policy, previous);
    // An independent unchanged peer/control passage, never a cropped primary launch.
    let mut peer_reference = configured(rate, policy, previous);
    let source = actual.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let transport = actual.transport;
    let bpm = actual.mixer.output_bpm_for_sample_id(0);
    assert!((bpm.unwrap() - 120.0 * rate).abs() < 1.0e-10);
    let loop_region = actual.mixer.loop_region_frames(0);
    let native_allocations = actual.control.engine.pitch_work_capacity.allocations().1;
    let action = capture_at_current_frame(&actual, key);
    assert_eq!(action.variant(), PitchActionVariant::SetRetrigger);
    assert_eq!(action.render().pitch, PitchIntent::default());
    assert!(!action.render().key_lock);
    assert_eq!(action.start_behavior(), Some(policy));
    assert!(!action.exclusive());
    let identity = (
        action.sequence(),
        action.content_key(),
        action.received_timestamp(),
        action.original_due_frame(),
    );
    assert_eq!(actual.pcm(delay), peer_reference.pcm(delay));
    let publication_frame = actual.transport.output_frame();
    publish_ready(&mut actual, &action);
    let execution = action.execution_target();
    assert_eq!(
        execution, publication_frame,
        "ready dry birth needs no unheard DSP lead"
    );
    assert_eq!(execution, identity.3 + delay as u64);
    assert_eq!(actual.transport.output_frame(), execution);

    // The first callback begins at actual E. Include every emitted sample from E;
    // do not select a suffix using lateness, DSP history or a detected peak.
    let mut pcm = actual.render(1);
    assert!(action.effective(), "{:?}", action.reason());
    assert_eq!(actual.starts(0), 1);
    pcm.extend(actual.pcm(OBSERVE_FRAMES - 1));
    let peer_pcm = peer_reference.pcm(OBSERVE_FRAMES);
    let maximum = pcm
        .iter()
        .zip(&peer_pcm)
        .enumerate()
        .map(|(frame, (mixed, peer))| {
            assert!(mixed.is_finite() && peer.is_finite());
            (mixed - peer - expected_sample(frame, rate)).abs()
        })
        .fold(0.0_f32, f32::max);
    assert!(
        maximum < 2.0e-6,
        "complete attack error={maximum}, rate={rate}, policy={policy:?}, previous={previous:?}, delay={delay}"
    );
    let voice = actual.voice(0);
    assert!(!voice.paused);
    assert!(voice.sample.as_ref().unwrap().same_source(&source));
    assert_eq!(voice.source_playback.tempo_ratio(), rate);
    assert_eq!(voice.source_playback.rate_target(), rate);
    let position = voice.source_playback.position();
    let expected = LOOP_START as f64 + OBSERVE_FRAMES as f64 * rate;
    assert!((position.frame as f64 + position.fraction - expected).abs() < 1.0e-8);
    assert_eq!(voice.stretch.adopted_request_id(), None);
    assert_eq!(voice.stretch.native_primed_output_frames(), 0);
    assert!(voice.stretch.productive_history().is_none());
    assert_eq!(
        actual.control.engine.pitch_work_capacity.allocations().1,
        native_allocations
    );
    let peer = actual.voice(1);
    let reference_peer = peer_reference.voice(1);
    assert_eq!(peer.generation, reference_peer.generation);
    assert_eq!(peer.paused, reference_peer.paused);
    assert_eq!(peer.volume, reference_peer.volume);
    assert!(
        peer.source_playback
            .matches_exact(&reference_peer.source_playback)
    );
    assert_eq!(
        peer.stretch.adopted_request_id(),
        reference_peer.stretch.adopted_request_id()
    );
    assert_eq!(
        peer.stretch.pending_fifo_frames(),
        reference_peer.stretch.pending_fifo_frames()
    );
    assert_eq!(
        peer.stretch.loop_acceptance_state(),
        reference_peer.stretch.loop_acceptance_state()
    );
    assert_eq!(actual.starts(1), 0);
    assert_eq!(actual.mixer.loop_region_frames(0), loop_region);
    assert_eq!(actual.mixer.output_bpm_for_sample_id(0), bpm);
    assert_eq!(
        actual.transport.master_period_seconds(),
        transport.master_period_seconds()
    );
    assert_eq!(
        actual.transport.downbeat_frame(),
        transport.downbeat_frame()
    );
    assert_eq!(
        actual
            .transport
            .beat_position_at_frame(execution + OBSERVE_FRAMES as u64),
        transport.beat_position_at_frame(execution + OBSERVE_FRAMES as u64)
    );
    assert_eq!(
        (
            action.sequence(),
            action.content_key(),
            action.received_timestamp(),
            action.original_due_frame(),
        ),
        identity
    );
    assert_eq!(action.execution_target(), execution);
    assert_eq!(
        actual.transport.output_frame(),
        execution + OBSERVE_FRAMES as u64
    );
}

#[test]
fn on_time_original_key_retrigger_keeps_complete_independent_source_attack() {
    for rate in [0.80, 1.0, 1.16] {
        for policy in [
            KeyLockStartBehavior::FirstSoundAtGrid,
            KeyLockStartBehavior::AttackAtGrid,
        ] {
            complete_attack(
                rate,
                policy,
                Previous::Stopped,
                0,
                "pitch.original:set_retrigger",
            );
        }
    }
}

#[test]
fn late_original_key_retrigger_keeps_complete_independent_source_attack() {
    for rate in [0.80, 1.0, 1.16] {
        for policy in [
            KeyLockStartBehavior::FirstSoundAtGrid,
            KeyLockStartBehavior::AttackAtGrid,
        ] {
            for previous in [Previous::Stopped, Previous::Running, Previous::Paused] {
                complete_attack(rate, policy, previous, 1, "pitch.original:set_retrigger");
            }
        }
    }
}

#[test]
fn late_nominal_piano_note_keeps_complete_independent_source_attack_after_publication_delay() {
    for rate in [0.80, 1.0, 1.16] {
        for policy in [
            KeyLockStartBehavior::FirstSoundAtGrid,
            KeyLockStartBehavior::AttackAtGrid,
        ] {
            for delay in [19, 1_274] {
                complete_attack(
                    rate,
                    policy,
                    Previous::Stopped,
                    delay,
                    "pitch.note:0:set_retrigger",
                );
            }
        }
    }
}
