//! Independent physical-source reference: a unique short attack, never a DUT suffix.
#![cfg(windows)]

use super::*;
use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_sources;
use crate::messages::SampleBuffer;
use std::sync::Arc;

const ATTACK_FRAMES: usize = 192;
const OBSERVED_FRAMES: usize = 384;

fn source_value(frame: usize) -> f32 {
    if (LOOP_START..LOOP_START + ATTACK_FRAMES).contains(&frame) {
        // Deterministic, nonperiodic, nonzero first sample. No later attack exists.
        0.2 + (frame - LOOP_START) as f32 / 2048.0
    } else {
        0.0
    }
}

fn configured(rate: f64) -> Fixture {
    let mut fixture = Fixture::new(false);
    let sample = SampleBuffer {
        channels: 1,
        samples: Arc::from((0..3 * RATE as usize).map(source_value).collect::<Vec<_>>()),
        residency: None,
    }
    .with_complete_source(RATE);
    publish_tone(&fixture.control, 0, sample, true);
    adopt_current_sources(&mut fixture, false);
    fixture.mixer.set_speed(rate);
    fixture
}

fn expected_from(start: usize, rate: f64) -> Vec<f32> {
    (0..OBSERVED_FRAMES)
        .map(|frame| {
            let position = start as f64 + frame as f64 * rate;
            let base = position.floor() as usize;
            let fraction = position.fract() as f32;
            source_value(base) * (1.0 - fraction) + source_value(base + 1) * fraction
        })
        .collect()
}

fn compare_full_source(rate: f64, capture_delay: usize) {
    let mut fixture = configured(rate);
    let start = capture(&fixture, 1.0);
    fixture.drain();
    let original = start.target_frame().unwrap();
    assert_eq!(original, 0);
    assert!(
        fixture
            .render(capture_delay)
            .iter()
            .all(|value| *value == 0.0)
    );
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&start, None)
            .unwrap()
    );
    fixture.drain();
    assert!(start.accepted(), "{:?}", start.reason());
    if start.state() == "preparing" {
        wait_ready(&mut fixture, &[start.sequence()]);
    }
    let execution = start.execution_frame().unwrap();
    assert!(
        fixture
            .render_to(execution)
            .iter()
            .all(|value| *value == 0.0)
    );
    let actual = fixture.pcm(OBSERVED_FRAMES);
    assert_eq!(start.state(), "applied", "{:?}", start.reason());
    assert_eq!(start.original_frame(), Some(original));
    assert_eq!(start.effective_frame(), Some(execution));
    let reference = expected_from(LOOP_START, rate);
    let max_error = actual
        .iter()
        .zip(&reference)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        max_error < 2.0e-6,
        "rate={rate} capture_delay={capture_delay} T={original} E={execution} max_error={max_error} first={:?} expected={:?}",
        &actual[..8],
        &reference[..8]
    );
    assert_eq!(fixture.starts(0), 1);
}

#[test]
fn on_time_dry_start_keeps_the_complete_independent_source_attack() {
    for rate in [0.80, 1.0, 1.16] {
        compare_full_source(rate, 0);
    }
}

#[test]
fn late_dry_start_keeps_the_complete_independent_source_attack() {
    // Vary delay independently of the Human observation; these are not calibration offsets.
    for rate in [0.80, 1.0, 1.16] {
        for delay in [1, 19, 1274, 2048] {
            compare_full_source(rate, delay);
        }
    }
}

#[test]
fn automatic_marker_preserves_full_dry_onset_and_an_independent_peer_in_both_policies() {
    for rate in [0.80, 1.0, 1.16] {
        for policy in ["first_sound_at_grid", "attack_at_grid"] {
            for active in [false, true] {
                compare_automatic_marker_with_peer(rate, policy, active);
            }
        }
    }
}

fn compare_automatic_marker_with_peer(rate: f64, policy: &str, active: bool) {
    let mut actual = configured(rate);
    let mut peer = configured(rate);
    let selected = LOOP_START - 1;
    let marker = selected as f64 / f64::from(RATE);
    let sample = actual.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    assert_eq!(
        super::super::initial_loop_start::detect_initial_loop_start(&sample, RATE),
        Some(marker)
    );
    for fixture in [&mut actual, &mut peer] {
        fixture.render(1);
        assert!(
            fixture
                .control
                .engine
                .set_key_lock_start_behavior(policy)
                .unwrap()
        );
        assert!(fixture.mixer.play_sample(1, 0.25));
        if active {
            assert!(fixture.mixer.play_sample(0, 1.0));
        }
    }
    assert_eq!(actual.render(64), peer.render(64));
    let original = actual.transport.output_frame();
    let start = actual
        .control
        .engine
        .capture_pad_start(
            0,
            marker,
            Some(LOOP_END as f64 / f64::from(RATE)),
            1.0,
            false,
            None,
            None,
        )
        .unwrap();
    actual.drain();
    assert_eq!(start.target_frame(), Some(original));
    assert_eq!(actual.render(1274), peer.render(1274));
    // Only the reference peer stops its old PAD0. Actual PAD0 is replaced solely
    // by the original callback effect; this never alters peer PAD1's state.
    if active {
        peer.mixer.stop_sample(0);
    }
    assert!(
        actual
            .control
            .engine
            .publish_pad_start(&start, None)
            .unwrap()
    );
    actual.drain();
    assert_eq!(
        start.execution_frame(),
        Some(actual.transport.output_frame())
    );
    let actual_pcm = actual.pcm(OBSERVED_FRAMES);
    let peer_pcm = peer.pcm(OBSERVED_FRAMES);
    let reference = expected_from(selected, rate);
    let error = actual_pcm
        .iter()
        .zip(peer_pcm)
        .zip(reference)
        .map(|((actual, peer), reference)| (actual - peer - reference).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        error < 2.0e-6,
        "rate={rate} policy={policy} active={active} max_error={error}"
    );
    assert_eq!(start.original_frame(), Some(original));
    assert_eq!(start.effective_frame(), start.execution_frame());
    assert!(
        actual
            .voice(1)
            .source_playback
            .matches_exact(&peer.voice(1).source_playback)
    );
    assert_eq!(
        actual.mixer.loop_region_frames(0),
        (selected, Some(LOOP_END))
    );
}
