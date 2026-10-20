//! Fresh physical-birth independence through the actual ordinary start path.
//!
//! The paired sources share their complete early positive content but have different
//! remote loop tails. An independent explicit-silence/raw-source oracle checks the
//! complete emitted beginning. This bounded software proof neither chooses C nor
//! establishes musical/Human acceptance.
#![cfg(windows)]

use super::*;
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::ordinary_endpoint_reserve_tests::{
    adopt_current_sources, refresh_actual_runtime_bindings,
};
use crate::audio_engine::pitch_actions::{PitchActionVariant, PitchKeyMode};
use crate::audio_engine::pitch_runtime_tests::source_feed_tests::RawRubberBand;
use crate::audio_engine::source_reader::{
    reset_tap_observation_for_test, tap_observation_for_test,
};
use crate::messages::SampleBuffer;
use pyo3::prelude::*;
use sha2::{Digest, Sha256};
use std::sync::Arc;

const ATTACK_FRAMES: usize = 192;
const IDENTICAL_POSITIVE_SOURCE_FRAMES: usize = 20_000;
const REMOTE_TAIL_START: usize = LOOP_END - 8_000;
const DECLARED_HISTORY_FRAMES: usize = 4_096;
const EMITTED_PREFIX_FRAMES: usize = 4_096;
const NATIVE_BLOCK_FRAMES: usize = 512;
const ADAPTER_LEAD_FRAMES: usize = NATIVE_BLOCK_FRAMES - 1;
const ORACLE_PCM_TOLERANCE: f32 = 2.0e-6;
const CAPTURE_FRAME: u64 = 1_000;
const ORIGINAL_TARGET: u64 = 24_000;
const EQUAL_PARTITIONS: &[usize] = &[512];
const UNEQUAL_A: &[usize] = &[127, 1, 509, 31, 256];
const UNEQUAL_B: &[usize] = &[257, 17, 1, 383, 96];

fn source(tail_b: bool) -> SampleBuffer {
    let samples = (0..3 * RATE as usize)
        .map(|frame| {
            if (LOOP_START..LOOP_START + ATTACK_FRAMES).contains(&frame) {
                // Unique independently declared event at the selected physical beginning.
                return 0.2 + (frame - LOOP_START) as f32 / 2048.0;
            }
            if (REMOTE_TAIL_START..LOOP_END).contains(&frame) {
                let time = (frame - REMOTE_TAIL_START) as f64 / f64::from(RATE);
                let tau = std::f64::consts::TAU;
                // Both tails are nonzero; neither enters the shared positive guard.
                return if tail_b {
                    (-0.11 + 0.055 * (tau * 431.0 * time + 0.4).sin()
                        - 0.02 * (tau * 1171.0 * time).cos()) as f32
                } else {
                    (0.06 + 0.025 * (tau * 223.0 * time).cos() + 0.015 * (tau * 947.0 * time).sin())
                        as f32
                };
            }
            0.0
        })
        .collect::<Vec<_>>();
    SampleBuffer {
        channels: 1,
        samples: Arc::from(samples),
        residency: None,
    }
    .with_complete_source(RATE)
}

fn source_sha256(source: &SampleBuffer) -> String {
    let mut hash = Sha256::new();
    for sample in source.samples.iter() {
        hash.update(sample.to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}

struct SilentBirthOracle {
    pcm: Vec<f32>,
    fifo: (usize, usize),
}

fn independent_silent_birth_oracle(source: &SampleBuffer, rate: f64) -> SilentBirthOracle {
    // Pinned raw helper: mono 48k, Short | ChannelsTogether, exact p then reset.
    // No DUT phase, history/delay getter, source reader or preparation helper is used.
    assert_eq!(RATE, 48_000);
    assert_eq!(source.channels, 1);
    let pitch = 1.0 / rate;
    let mut raw = RawRubberBand::new(pitch);
    assert_eq!(raw.fifo(), (0, ADAPTER_LEAD_FRAMES));
    for _ in 0..DECLARED_HISTORY_FRAMES {
        assert_eq!(raw.sample(0.0, pitch), 0.0);
    }
    assert_eq!(raw.fifo(), (0, ADAPTER_LEAD_FRAMES));
    let mut maximum_source_tap = LOOP_START;
    let pcm = (0..EMITTED_PREFIX_FRAMES)
        .map(|output_frame| {
            let coordinate = LOOP_START as f64 + output_frame as f64 * rate;
            let left = coordinate.floor() as usize;
            let right = left + 1;
            assert!(right < LOOP_START + IDENTICAL_POSITIVE_SOURCE_FRAMES);
            assert!(right < REMOTE_TAIL_START);
            maximum_source_tap = maximum_source_tap.max(right);
            let fraction = coordinate.fract() as f32;
            let a = source.samples[left];
            let b = source.samples[right];
            // Raw.sample already accounts for the initial 511 adapter frames. Feed
            // this first physical sample directly; add no second FIFO/silence prefix.
            raw.sample(a + (b - a) * fraction, pitch)
        })
        .collect::<Vec<_>>();
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
    let fifo = raw.fifo();
    assert_eq!(fifo, (0, ADAPTER_LEAD_FRAMES));
    println!(
        "physical_birth independent_oracle native_profile=mono_48000_short_channels_together \
         rate={rate:.17} pitch={pitch:.17} explicit_silent_history={DECLARED_HISTORY_FRAMES} \
         declared_adapter_lead={ADAPTER_LEAD_FRAMES} extra_adapter_prefix=0 \
         physical_source_first_tap={LOOP_START} physical_source_max_tap={maximum_source_tap} \
         complete_emitted_prefix_frames={EMITTED_PREFIX_FRAMES} fifo={fifo:?} \
         scalar_pcm_tolerance={ORACLE_PCM_TOLERANCE:.12e} accepted_musical_c=none"
    );
    SilentBirthOracle { pcm, fifo }
}

fn configured(source: &SampleBuffer, rate: f64, policy: &str) -> Fixture {
    let mut fixture = Fixture::new(true);
    // Reuse real source/lifetime publication, complete LoadSample callback ACK and
    // the corresponding current runtime binding; no stale fixture binding is reused.
    publish_tone(&fixture.control, 0, source.clone(), true);
    adopt_current_sources(&mut fixture, true);
    refresh_actual_runtime_bindings(&fixture);
    fixture.mixer.set_speed(rate);
    fixture
        .mixer
        .set_pad_pitch_intent(0, PitchIntent::new(0, 0).unwrap())
        .unwrap();
    fixture.drain();
    assert!(fixture.render(1).iter().all(|sample| *sample == 0.0));
    assert!(fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior(policy)
            .unwrap()
    );
    fixture
}

fn render_prefix(fixture: &mut Fixture, partitions: &[usize]) -> Vec<f32> {
    assert!(!partitions.is_empty() && partitions.iter().all(|count| *count > 0));
    let mut pcm = Vec::with_capacity(EMITTED_PREFIX_FRAMES);
    let mut emitted = 0;
    let mut callback = 0;
    while emitted < EMITTED_PREFIX_FRAMES {
        let count = partitions[callback % partitions.len()].min(EMITTED_PREFIX_FRAMES - emitted);
        // The first due-frame output sample is included; there is no E-T/D/FIFO crop.
        pcm.extend(fixture.render(count));
        emitted += count;
        callback += 1;
    }
    assert_eq!(pcm.len(), EMITTED_PREFIX_FRAMES);
    pcm
}

fn emitted(
    source: &SampleBuffer,
    identity: &str,
    tail: &str,
    rate: f64,
    policy: &str,
    partitions: &[usize],
    oracle_fifo: (usize, usize),
) -> Vec<f32> {
    let mut fixture = configured(source, rate, policy);
    assert!(
        fixture
            .render_to(CAPTURE_FRAME)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    fixture.refresh_clock();
    let start = capture(&fixture, 1.0);
    let key = start.content_key();
    let ownership = fixture.control.engine.input_runtime_ownership.clone();
    assert_eq!(ownership.slot_snapshot(0).unwrap().content_key(), Some(key));
    assert_eq!(ownership.callback_feedback_key(0), Some(key));
    assert_eq!(start.start_behavior().as_str(), policy);
    drain_with_mode(&mut fixture, TriggerQuantization::Grid { step_64ths: 16 });
    assert_eq!(start.original_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(start.target_frame(), Some(ORIGINAL_TARGET));
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&start, None)
            .unwrap()
    );
    drain_with_mode(&mut fixture, TriggerQuantization::Grid { step_64ths: 16 });
    assert!(
        start.accepted(),
        "state={} reason={:?}",
        start.state(),
        start.reason()
    );
    assert_eq!(start.execution_frame(), Some(ORIGINAL_TARGET));
    // Genuine future-T worker readiness, with virtual playback fixed at frame 1000.
    wait_ready(&mut fixture, &[start.sequence()]);
    assert_eq!(fixture.transport.output_frame(), CAPTURE_FRAME);
    assert!(
        fixture
            .render_to(ORIGINAL_TARGET)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(fixture.starts(0), 0);

    println!(
        "physical_birth source tail={tail} pcm_sha256={identity} source_format=mono_f32le_48000 \
         source_frames={} selected_loop=[{LOOP_START},{LOOP_END}) \
         declared_attack=[{LOOP_START},{}) identical_positive_source_frames={IDENTICAL_POSITIVE_SOURCE_FRAMES} \
         differing_remote_tail=[{REMOTE_TAIL_START},{LOOP_END}) \
         declared_history_output_frames={DECLARED_HISTORY_FRAMES} emitted_prefix_frames={EMITTED_PREFIX_FRAMES} \
         rate={rate:.17} native_pitch={:.17} policy={policy} partitions={partitions:?} \
         original_target={ORIGINAL_TARGET} actual_execution={:?} accepted_musical_c=none",
        source.frame_count(),
        LOOP_START + ATTACK_FRAMES,
        1.0 / rate,
        start.execution_frame(),
    );
    // This observer is callback-thread local. Worker preparation reads are retained
    // separately by the adopted Native's existing preparation coverage receipt.
    reset_tap_observation_for_test();
    let pcm = render_prefix(&mut fixture, partitions);
    let live_taps = tap_observation_for_test();
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert_eq!(start.state(), "applied", "reason={:?}", start.reason());
    assert_eq!(start.original_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(start.target_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(start.execution_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(start.effective_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(fixture.starts(0), 1);
    assert_eq!(
        fixture.transport.output_frame(),
        ORIGINAL_TARGET + EMITTED_PREFIX_FRAMES as u64
    );
    assert_eq!(ownership.slot_snapshot(0).unwrap().content_key(), Some(key));
    assert_eq!(ownership.callback_feedback_key(0), Some(key));
    assert!(fixture.scheduler.is_empty());
    let voice = fixture.voice(0);
    let pitch = PitchIntent::new(0, 0).unwrap();
    assert!(voice.stretch.owns_pitch_contract(pitch, true));
    assert_eq!(voice.stretch.adopted_request_id(), Some(start.sequence()));
    assert_ne!(voice.stretch.native_state_address(), 0);
    assert_eq!(voice.stretch.pending_fifo_frames(), oracle_fifo);
    assert_eq!(
        voice.stretch.native_primed_output_frames(),
        DECLARED_HISTORY_FRAMES
    );
    // A complete-source owner need not carry a finite-window tap receipt.
    // Keep that absence explicit; callback taps below still prove the live horizon.
    let worker_taps = voice.stretch.native_tap_observation();
    assert!(worker_taps.is_none_or(|taps| taps.missing_reads == 0));
    let history = voice.stretch.productive_history().unwrap();
    let position = voice.source_playback.position();
    let expected = LOOP_START as f64 + EMITTED_PREFIX_FRAMES as f64 * rate;
    assert!(expected + 1.0 < (LOOP_START + IDENTICAL_POSITIVE_SOURCE_FRAMES) as f64);
    assert_eq!(position.frame, expected.floor() as usize);
    assert!((position.fraction - expected.fract()).abs() < 1.0e-8);
    assert_eq!(voice.source_playback.tempo_ratio(), rate);
    // Independently chosen 20000-source-frame guard, not a measured Native delay.
    // Every actual live interpolation/lookahead tap must remain inside this shared
    // early content. The backend cannot inspect source PCM never supplied to it.
    assert!(live_taps.left_reads > 0);
    assert_eq!(live_taps.missing_reads, 0);
    assert!(live_taps.min_frame.unwrap() >= LOOP_START);
    assert!(live_taps.max_frame.unwrap() < LOOP_START + IDENTICAL_POSITIVE_SOURCE_FRAMES);
    println!(
        "physical_birth capture tail={tail} rate={rate:.17} policy={policy} \
         original_target={:?} execution={:?} effective={:?} source_position={position:?} \
         native_owner={} adopted_request={:?} native_primed={} productive_fed={} \
         native_fifo={:?} independent_oracle_fifo={oracle_fifo:?} \
         worker_prepared_taps={worker_taps:?} callback_positive_taps={live_taps:?} \
         positive_guard_verified=true accepted_musical_c=none",
        start.original_frame(),
        start.execution_frame(),
        start.effective_frame(),
        voice.stretch.native_state_address(),
        voice.stretch.adopted_request_id(),
        voice.stretch.native_primed_output_frames(),
        history.fed_output_frames,
        voice.stretch.pending_fifo_frames(),
    );
    pcm
}

#[derive(Debug)]
struct Difference {
    first: Option<(usize, f32, f32)>,
    maximum: f32,
}

fn difference(left: &[f32], right: &[f32]) -> Difference {
    assert_eq!(left.len(), EMITTED_PREFIX_FRAMES);
    assert_eq!(right.len(), EMITTED_PREFIX_FRAMES);
    Difference {
        first: left
            .iter()
            .zip(right)
            .enumerate()
            .find_map(|(frame, (a, b))| (a != b).then_some((frame, *a, *b))),
        maximum: left
            .iter()
            .zip(right)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max),
    }
}

#[test]
fn public_fresh_native_birth_is_tail_independent_and_matches_explicit_silence_source_oracle() {
    let a = source(false);
    let b = source(true);
    assert!(LOOP_START + IDENTICAL_POSITIVE_SOURCE_FRAMES < REMOTE_TAIL_START);
    let shared = LOOP_START..LOOP_START + IDENTICAL_POSITIVE_SOURCE_FRAMES;
    assert!(a.samples[shared.clone()] == b.samples[shared]);
    assert!(
        a.samples[REMOTE_TAIL_START..LOOP_END]
            .iter()
            .all(|sample| *sample != 0.0)
    );
    assert!(
        b.samples[REMOTE_TAIL_START..LOOP_END]
            .iter()
            .all(|sample| *sample != 0.0)
    );
    assert!(a.samples[REMOTE_TAIL_START..LOOP_END] != b.samples[REMOTE_TAIL_START..LOOP_END]);
    let identity_a = source_sha256(&a);
    let identity_b = source_sha256(&b);
    assert_ne!(identity_a, identity_b);
    let mut partition_violations = Vec::new();
    let mut independence_violations = Vec::new();
    let mut oracle_violations = Vec::new();
    for rate in [4.0_f64 / 5.0, 29.0 / 25.0] {
        // A and B have exactly the same addressed positive source. One independent
        // reference per rate therefore serves all policies/tails/partitions.
        let oracle = independent_silent_birth_oracle(&a, rate);
        for policy in ["first_sound_at_grid", "attack_at_grid"] {
            let a_equal = emitted(
                &a,
                &identity_a,
                "A",
                rate,
                policy,
                EQUAL_PARTITIONS,
                oracle.fifo,
            );
            let b_equal = emitted(
                &b,
                &identity_b,
                "B",
                rate,
                policy,
                EQUAL_PARTITIONS,
                oracle.fifo,
            );
            let a_unequal = emitted(&a, &identity_a, "A", rate, policy, UNEQUAL_A, oracle.fifo);
            let b_unequal = emitted(&b, &identity_b, "B", rate, policy, UNEQUAL_B, oracle.fifo);
            for (label, pcm) in [
                ("A_equal", &a_equal),
                ("B_equal", &b_equal),
                ("A_unequal", &a_unequal),
                ("B_unequal", &b_unequal),
            ] {
                let diff = difference(pcm, &oracle.pcm);
                let first_above_tolerance = pcm.iter().zip(&oracle.pcm).enumerate().find_map(
                    |(frame, (actual, expected))| {
                        ((actual - expected).abs() >= ORACLE_PCM_TOLERANCE)
                            .then_some((frame, *actual, *expected))
                    },
                );
                println!(
                    "physical_birth oracle_difference capture={label} rate={rate:.17} policy={policy} \
                     complete_emitted_prefix_frames={EMITTED_PREFIX_FRAMES} first_difference={:?} \
                     first_above_tolerance={first_above_tolerance:?} maximum_difference={:.12e} \
                     pcm_tolerance={ORACLE_PCM_TOLERANCE:.12e} accepted_musical_c=none",
                    diff.first, diff.maximum,
                );
                if diff.maximum >= ORACLE_PCM_TOLERANCE {
                    oracle_violations.push(format!(
                        "{label} rate={rate:.17} policy={policy} first_above_tolerance={first_above_tolerance:?} {diff:?}"
                    ));
                }
            }
            for (label, left, right, tail_independence) in [
                ("same_A_partition_control", &a_equal, &a_unequal, false),
                ("same_B_partition_control", &b_equal, &b_unequal, false),
                ("A_B_equal_partitions", &a_equal, &b_equal, true),
                ("A_B_unequal_partitions", &a_unequal, &b_unequal, true),
            ] {
                let diff = difference(left, right);
                println!(
                    "physical_birth difference comparison={label} rate={rate:.17} policy={policy} \
                     full_emitted_prefix_frames={EMITTED_PREFIX_FRAMES} \
                     first_difference={:?} maximum_difference={:.12e} \
                     physical_birth_independence={tail_independence} accepted_musical_c=none",
                    diff.first, diff.maximum,
                );
                if diff.first.is_some() {
                    let failure = format!("{label} rate={rate:.17} policy={policy} {diff:?}");
                    if tail_independence {
                        independence_violations.push(failure);
                    } else {
                        partition_violations.push(failure);
                    }
                }
            }
        }
    }
    // Retain the original 4096-frame profile and print all sixteen real captures
    // before checking the normative birth rule. Exact pair/partition checks remain
    // separate from the established independent-scalar 2e-6 PCM allowance.
    assert!(
        partition_violations.is_empty(),
        "partition controls: {partition_violations:?}"
    );
    assert!(
        independence_violations.is_empty(),
        "fresh physical birth depends on unread remote loop tail: {independence_violations:?}"
    );
    assert!(
        oracle_violations.is_empty(),
        "complete fresh physical birth differs from independent explicit-silence/source Native: {oracle_violations:?}"
    );
}

struct RouteCapture {
    pcm: Vec<f32>,
    execution: u64,
}

fn assert_paired_positive_source(a: &SampleBuffer, b: &SampleBuffer) {
    let shared = LOOP_START..LOOP_START + IDENTICAL_POSITIVE_SOURCE_FRAMES;
    assert!(a.samples[shared.clone()] == b.samples[shared]);
    assert!(LOOP_START + IDENTICAL_POSITIVE_SOURCE_FRAMES < REMOTE_TAIL_START);
    assert!(a.samples[REMOTE_TAIL_START..LOOP_END] != b.samples[REMOTE_TAIL_START..LOOP_END]);
    assert_ne!(a.source_address(), b.source_address());
}

fn assert_fresh_route_voice(
    fixture: &Fixture,
    id: usize,
    sequence: u64,
    rate: f64,
    oracle_fifo: (usize, usize),
) {
    let ownership = &fixture.control.engine.input_runtime_ownership;
    let key = ownership.slot_snapshot(id).unwrap().content_key().unwrap();
    assert_eq!(ownership.callback_feedback_key(id), Some(key));
    let sample = fixture.control.engine.sample_cache.lock().unwrap()[id]
        .clone()
        .unwrap();
    let voice = fixture.voice(id);
    assert_eq!(
        voice.source_admission.as_ref().unwrap().content_key(),
        Some(key)
    );
    assert_eq!(voice.stretch.adopted_request_id(), Some(sequence));
    assert_ne!(voice.stretch.native_state_address(), 0);
    assert!(
        voice
            .stretch
            .owns_pitch_contract(PitchIntent::default(), true)
    );
    assert_eq!(
        voice.stretch.native_pitch_scale_observation(),
        Some(1.0 / rate)
    );
    assert_eq!(voice.stretch.pending_fifo_frames(), oracle_fifo);
    assert_eq!(
        voice.stretch.native_primed_output_frames(),
        DECLARED_HISTORY_FRAMES
    );
    assert_eq!(
        fixture.mixer.pad_pitch_intent(id),
        Some(PitchIntent::default())
    );
    assert!(fixture.mixer.key_lock_for_measurement(id));
    assert_eq!(
        fixture.mixer.loop_region_frames(id),
        (LOOP_START, Some(LOOP_END))
    );
    assert_eq!(
        voice.sample.as_ref().unwrap().source_address(),
        sample.source_address()
    );
    let history = voice.stretch.productive_history().unwrap();
    assert_eq!(history.binding.source_address, sample.source_address());
    assert_eq!(history.binding.sample_count, sample.source_sample_count());
    assert_eq!(history.binding.channels, 1);
    assert_eq!(history.binding.sample_rate_hz, RATE);
    assert_eq!(history.binding.resident, sample.resident_binding());
    assert_eq!(history.binding.accepted, voice.source_timing.accepted);
    assert_eq!(
        history.fed_output_frames,
        (DECLARED_HISTORY_FRAMES + EMITTED_PREFIX_FRAMES) as u64
    );
    let position = voice.source_playback.position();
    let expected = LOOP_START as f64 + EMITTED_PREFIX_FRAMES as f64 * rate;
    assert!(expected + 1.0 < (LOOP_START + IDENTICAL_POSITIVE_SOURCE_FRAMES) as f64);
    assert_eq!(position.frame, expected.floor() as usize);
    assert!((position.fraction - expected.fract()).abs() < 1.0e-8);
    assert_eq!(voice.source_playback.tempo_ratio(), rate);
    let worker_taps = voice.stretch.native_tap_observation();
    assert!(worker_taps.is_none_or(|taps| taps.missing_reads == 0));
    println!(
        "physical_birth route_owner pad={id} key={key:?} request={sequence} rate={rate:.17} \
         native_owner={} fifo={oracle_fifo:?} source_position={position:?} \
         worker_prepared_taps={worker_taps:?} accepted_musical_c=none",
        voice.stretch.native_state_address(),
    );
}

fn assert_live_route_guard(route: &str) {
    let taps = tap_observation_for_test();
    assert!(taps.left_reads > 0);
    assert_eq!(taps.missing_reads, 0);
    assert!(taps.min_frame.unwrap() >= LOOP_START);
    assert!(taps.max_frame.unwrap() < LOOP_START + IDENTICAL_POSITIVE_SOURCE_FRAMES);
    println!(
        "physical_birth route={route} callback_positive_taps={taps:?} positive_guard_verified=true"
    );
}

fn piano_emitted(
    source: &SampleBuffer,
    rate: f64,
    policy: &str,
    partitions: &[usize],
    oracle_fifo: (usize, usize),
) -> RouteCapture {
    let mut fixture = configured(source, rate, policy);
    fixture.rows(PitchPadProjection {
        intent: PitchIntent::default(),
        key_lock: true,
        source_root: Some(0),
        source_mode: Some(PitchKeyMode::Major),
        quantize_step_64ths: Some(16),
        ..PitchPadProjection::default()
    });
    // Piano uses the established nearest input grid: capture past the midpoint
    // so its genuine immutable original is the future 24000 boundary.
    assert!(
        fixture
            .render_to(17_000)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    fixture.refresh_clock();
    let before = fixture.transport;
    let bpm = fixture.mixer.output_bpm_for_sample_id(0);
    assert!(fixture.mixer.key_lock_for_measurement(0));
    let action = fixture
        .control
        .engine
        .capture_pad_pitch_action(0, "pitch.note:0:set_retrigger", None)
        .unwrap();
    let identity = (
        action.content_key(),
        action.sequence(),
        action.received_timestamp(),
        action.render(),
    );
    assert_eq!(action.variant(), PitchActionVariant::SetRetrigger);
    assert_eq!(action.render().pitch, PitchIntent::default());
    assert!(action.render().key_lock);
    assert_eq!(action.start_behavior().unwrap().as_str(), policy);
    assert_eq!(action.original_due_frame(), ORIGINAL_TARGET);
    assert_eq!(
        fixture
            .control
            .engine
            .input_runtime_ownership
            .callback_feedback_key(0),
        Some(identity.0)
    );
    assert!(
        fixture
            .control
            .engine
            .publish_pitch_action(&action)
            .unwrap()
    );
    fixture.drain();
    assert!(action.accepted(), "{:?}", action.reason());
    assert_eq!(fixture.scheduler.len(), 1);
    fixture.wait_worker(&[&action]);
    let execution = action.execution_target();
    assert!(execution >= ORIGINAL_TARGET);
    assert_eq!(fixture.scheduler.peek_next_target_frame(), Some(execution));
    assert!(
        fixture
            .render_to(execution)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(fixture.starts(0), 0);
    println!(
        "physical_birth route=piano source_pcm_sha256={} rate={rate:.17} policy={policy} \
         T={ORIGINAL_TARGET} E={execution} first_emitted_frame={execution} \
         partitions={partitions:?} source_event={LOOP_START} shared_positive_guard={IDENTICAL_POSITIVE_SOURCE_FRAMES} \
         remote_tail={REMOTE_TAIL_START} accepted_musical_c=none",
        source_sha256(source),
    );
    reset_tap_observation_for_test();
    let pcm = render_prefix(&mut fixture, partitions);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert_live_route_guard("piano");
    assert!(action.effective(), "{:?}", action.reason());
    assert_eq!(action.state(), "applied");
    assert_eq!(action.original_due_frame(), ORIGINAL_TARGET);
    assert_eq!(action.execution_target(), execution);
    assert_eq!(
        (
            action.content_key(),
            action.sequence(),
            action.received_timestamp(),
            action.render()
        ),
        identity
    );
    assert_eq!(fixture.starts(0), 1);
    assert_eq!(fixture.starts(1), 0);
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        1
    );
    assert!(fixture.scheduler.is_empty());
    assert_fresh_route_voice(&fixture, 0, action.sequence(), rate, oracle_fifo);
    assert_eq!(
        fixture.transport.output_frame(),
        execution + EMITTED_PREFIX_FRAMES as u64
    );
    assert_eq!(
        fixture.transport.master_period_seconds(),
        before.master_period_seconds()
    );
    assert_eq!(fixture.transport.downbeat_frame(), before.downbeat_frame());
    assert_eq!(fixture.mixer.output_bpm_for_sample_id(0), bpm);
    RouteCapture { pcm, execution }
}

fn group_emitted(
    sources: &[SampleBuffer; 2],
    rate: f64,
    policy: &str,
    partitions: &[usize],
    oracle_fifo: (usize, usize),
) -> RouteCapture {
    let mut fixture = configured(&sources[0], rate, policy);
    publish_tone(&fixture.control, 1, sources[1].clone(), true);
    adopt_current_sources(&mut fixture, true);
    refresh_actual_runtime_bindings(&fixture);
    fixture
        .mixer
        .set_pad_pitch_intent(1, PitchIntent::default())
        .unwrap();
    assert!(fixture.render(1).iter().all(|sample| *sample == 0.0));
    assert!(
        fixture
            .render_to(CAPTURE_FRAME)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    fixture.refresh_clock();
    let before = fixture.transport;
    let bpms = std::array::from_fn::<_, 2, _>(|id| fixture.mixer.output_bpm_for_sample_id(id));
    Python::initialize();
    let group = Python::attach(|py| {
        let entries = [0, 1]
            .into_iter()
            .map(|id| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                assert!(binding.is_current());
                (
                    Py::new(py, binding).unwrap(),
                    LOOP_START as f64 / f64::from(RATE),
                    Some(LOOP_END as f64 / f64::from(RATE)),
                    None,
                )
            })
            .collect();
        fixture
            .control
            .engine
            .capture_global_start(py, entries, None)
            .unwrap()
    });
    assert_eq!(group.diagnostic_slots(), vec![0, 1]);
    drain_with_mode(&mut fixture, TriggerQuantization::Grid { step_64ths: 16 });
    assert_eq!(group.original_frame(), Some(ORIGINAL_TARGET));
    let starts = group
        .resolve()
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|member| member.start)
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 2);
    let keys = [starts[0].content_key(), starts[1].content_key()];
    assert_ne!(keys[0], keys[1]);
    assert_ne!(keys[0].lifetime(), keys[1].lifetime());
    assert_ne!(starts[0].sequence(), starts[1].sequence());
    for (id, start) in starts.iter().enumerate() {
        assert_eq!(start.original_frame(), Some(ORIGINAL_TARGET));
        assert_eq!(start.start_behavior().as_str(), policy);
        assert_eq!(
            fixture
                .control
                .engine
                .input_runtime_ownership
                .callback_feedback_key(id),
            Some(keys[id])
        );
    }
    assert!(fixture.scheduler.is_empty());
    let ticket = fixture
        .control
        .engine
        .publish_captured_global_start(&group)
        .unwrap()
        .unwrap();
    drain_with_mode(&mut fixture, TriggerQuantization::Grid { step_64ths: 16 });
    assert_eq!(
        fixture.scheduler.len(),
        1,
        "one two-PAD group occupies one scheduler row"
    );
    let execution = fixture.scheduler.peek_next_target_frame().unwrap();
    assert!(execution >= ORIGINAL_TARGET);
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.ordinary_group_ready(2) {
            break;
        }
        assert_eq!(ticket.publication_status(), "pending");
        assert!(
            Instant::now() < deadline,
            "bounded two-PAD group preparation wait"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), CAPTURE_FRAME);
    assert_eq!(fixture.scheduler.len(), 1);
    assert_eq!(ticket.publication_status(), "pending");
    for start in &starts {
        assert!(start.accepted(), "{:?}", start.reason());
        assert_eq!(start.execution_frame(), Some(execution));
        assert_eq!(start.effective_frame(), None);
    }
    assert!(
        fixture
            .render_to(execution)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    println!(
        "physical_birth route=two_pad_group source_pcm_sha256=[{},{}] rate={rate:.17} policy={policy} \
         T={ORIGINAL_TARGET} common_E={execution} first_emitted_frame={execution} scheduler_rows=1 \
         participants=2 partitions={partitions:?} source_event={LOOP_START} \
         shared_positive_guard={IDENTICAL_POSITIVE_SOURCE_FRAMES} remote_tail={REMOTE_TAIL_START} \
         accepted_musical_c=none",
        source_sha256(&sources[0]),
        source_sha256(&sources[1]),
    );
    reset_tap_observation_for_test();
    let pcm = render_prefix(&mut fixture, partitions);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert_live_route_guard("two_pad_group");
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(group.state(), "applied", "{:?}", group.reason());
    assert_eq!(group.original_frame(), Some(ORIGINAL_TARGET));
    assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        2
    );
    assert!(fixture.scheduler.is_empty());
    for (id, start) in starts.iter().enumerate() {
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        assert_eq!(start.original_frame(), Some(ORIGINAL_TARGET));
        assert_eq!(start.target_frame(), Some(ORIGINAL_TARGET));
        assert_eq!(start.execution_frame(), Some(execution));
        assert_eq!(start.effective_frame(), Some(execution));
        assert_eq!(start.content_key(), keys[id]);
        assert_eq!(
            fixture
                .control
                .engine
                .input_runtime_ownership
                .slot_snapshot(id)
                .unwrap()
                .content_key(),
            Some(keys[id])
        );
        assert_fresh_route_voice(&fixture, id, start.sequence(), rate, oracle_fifo);
        assert_eq!(fixture.mixer.output_bpm_for_sample_id(id), bpms[id]);
    }
    assert_ne!(
        fixture.voice(0).stretch.native_state_address(),
        fixture.voice(1).stretch.native_state_address()
    );
    assert_ne!(
        fixture.voice(0).sample.as_ref().unwrap().source_address(),
        fixture.voice(1).sample.as_ref().unwrap().source_address()
    );
    assert_eq!(
        fixture.transport.output_frame(),
        execution + EMITTED_PREFIX_FRAMES as u64
    );
    assert_eq!(
        fixture.transport.master_period_seconds(),
        before.master_period_seconds()
    );
    assert_eq!(fixture.transport.downbeat_frame(), before.downbeat_frame());
    RouteCapture { pcm, execution }
}

fn assert_route_captures(
    route: &str,
    rate: f64,
    policy: &str,
    captures: [RouteCapture; 4],
    oracle: &[f32],
) {
    for (label, capture) in ["A_equal", "B_equal", "A_unequal", "B_unequal"]
        .into_iter()
        .zip(&captures)
    {
        assert_eq!(capture.execution, captures[0].execution);
        let diff = difference(&capture.pcm, oracle);
        println!(
            "physical_birth route={route} capture={label} rate={rate:.17} policy={policy} \
             full_emitted_prefix_frames={EMITTED_PREFIX_FRAMES} first_oracle_difference={:?} \
             maximum_oracle_difference={:.12e} pcm_tolerance={ORACLE_PCM_TOLERANCE:.12e} \
             accepted_musical_c=none",
            diff.first, diff.maximum,
        );
        assert!(
            diff.maximum < ORACLE_PCM_TOLERANCE,
            "{route} {label} {diff:?}"
        );
    }
    for (label, left, right) in [
        ("A_partition_control", 0, 2),
        ("B_partition_control", 1, 3),
        ("A_B_equal_partitions", 0, 1),
        ("A_B_unequal_partitions", 2, 3),
    ] {
        let diff = difference(&captures[left].pcm, &captures[right].pcm);
        println!(
            "physical_birth route={route} comparison={label} rate={rate:.17} policy={policy} \
             first_difference={:?} maximum_difference={:.12e} accepted_musical_c=none",
            diff.first, diff.maximum,
        );
        assert!(diff.first.is_none(), "{route} {label} {diff:?}");
    }
}

#[test]
fn public_piano_physical_birth_is_tail_independent_and_matches_zero_source_oracle() {
    let a = source(false);
    let b = source(true);
    assert_paired_positive_source(&a, &b);
    for rate in [4.0_f64 / 5.0, 29.0 / 25.0] {
        let oracle = independent_silent_birth_oracle(&a, rate);
        for policy in ["first_sound_at_grid", "attack_at_grid"] {
            let captures = [
                piano_emitted(&a, rate, policy, EQUAL_PARTITIONS, oracle.fifo),
                piano_emitted(&b, rate, policy, EQUAL_PARTITIONS, oracle.fifo),
                piano_emitted(&a, rate, policy, UNEQUAL_A, oracle.fifo),
                piano_emitted(&b, rate, policy, UNEQUAL_B, oracle.fifo),
            ];
            assert_route_captures("piano", rate, policy, captures, &oracle.pcm);
        }
    }
}

#[test]
fn public_two_pad_group_physical_birth_is_atomic_tail_independent_and_matches_two_zero_oracles() {
    // Separate immutable source owners as well as separate mutable Native/voice owners.
    let a = [source(false), source(false)];
    let b = [source(true), source(true)];
    for id in [0, 1] {
        assert_paired_positive_source(&a[id], &b[id]);
    }
    assert_ne!(a[0].source_address(), a[1].source_address());
    assert_ne!(b[0].source_address(), b[1].source_address());
    for rate in [4.0_f64 / 5.0, 29.0 / 25.0] {
        let first = independent_silent_birth_oracle(&a[0], rate);
        let second = independent_silent_birth_oracle(&a[1], rate);
        assert_eq!(first.fifo, second.fifo);
        let sum = first
            .pcm
            .iter()
            .zip(&second.pcm)
            .map(|(a, b)| a + b)
            .collect::<Vec<_>>();
        for policy in ["first_sound_at_grid", "attack_at_grid"] {
            let captures = [
                group_emitted(&a, rate, policy, EQUAL_PARTITIONS, first.fifo),
                group_emitted(&b, rate, policy, EQUAL_PARTITIONS, first.fifo),
                group_emitted(&a, rate, policy, UNEQUAL_A, first.fifo),
                group_emitted(&b, rate, policy, UNEQUAL_B, first.fifo),
            ];
            assert_route_captures("two_pad_group", rate, policy, captures, &sum);
        }
    }
}
