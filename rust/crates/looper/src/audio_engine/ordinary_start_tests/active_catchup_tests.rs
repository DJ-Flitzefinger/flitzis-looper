//! An active constant-rate piano retrigger owns the same original attack as an early peer.
#![cfg(windows)]

use super::*;
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::pitch_actions::CapturedPitchAction;
use std::sync::atomic::Ordering;

const EARLY_CAPTURE: u64 = 17_000;
const ORIGINAL_TARGET: u64 = 24_000;
const LATE_EXECUTION: u64 = 32_192;
const EMITTED_FRAMES: usize = 24_000;

fn render_partitioned(fixture: &mut Fixture, frames: usize, partitions: &[usize]) -> Vec<f32> {
    let mut pcm = Vec::with_capacity(frames);
    let mut rendered = 0;
    let mut callback = 0;
    while rendered < frames {
        let count = partitions[callback % partitions.len()].min(frames - rendered);
        assert!(count > 0);
        pcm.extend(fixture.render(count));
        rendered += count;
        callback += 1;
    }
    pcm
}

fn active_fixture(rate: f64, policy: &str) -> Fixture {
    // Keep the predecessor on its deterministic dry source path. The explicit
    // captured future tuple requests Key Lock ON; it must not retune this old voice.
    let mut fixture = Fixture::new(false);
    fixture.mixer.set_speed(rate);
    fixture.drain();
    assert_eq!(fixture.render(1), vec![0.0]);
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior(policy)
            .unwrap()
    );
    fixture.rows(PitchPadProjection {
        key_lock: true,
        ..PitchPadProjection::default()
    });
    assert!(fixture.mixer.play_sample(0, 1.0));
    assert!(fixture.voice(0).active);
    assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), rate);
    assert_eq!(fixture.voice(0).source_playback.rate_target(), rate);
    fixture
}

fn assert_applied(action: &CapturedPitchAction, original: u64, execution: u64) {
    assert_eq!(action.original_due_frame(), original);
    assert_eq!(action.execution_target(), execution);
    assert!(action.accepted(), "{:?}", action.reason());
    assert!(
        action.effective(),
        "{} {:?}",
        action.state(),
        action.reason()
    );
    assert_eq!(action.state(), "applied");
}

#[test]
fn active_constant_retrigger_emits_the_independent_early_native_beginning_from_its_first_sample() {
    for rate in [0.73, 1.0, 1.37] {
        for policy in ["first_sound_at_grid", "attack_at_grid"] {
            compare_active_retrigger(rate, policy);
        }
    }
}

fn compare_active_retrigger(rate: f64, policy: &str) {
    let mut early = active_fixture(rate, policy);
    let mut late = active_fixture(rate, policy);
    let early_past = early.render_to(EARLY_CAPTURE);
    let late_past = late.render_to(EARLY_CAPTURE);
    assert!(
        early_past == late_past,
        "independent old dry PCM must match"
    );
    assert!(early_past.iter().any(|sample| sample.abs() > 0.01));
    for fixture in [&early, &late] {
        assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), rate);
        assert_eq!(fixture.voice(0).source_playback.rate_target(), rate);
    }

    // The early public capture uses the real input projection/grid, leaving enough
    // actual worker lead to execute at T. Both fixtures already have audible old voices.
    early.rows(PitchPadProjection {
        key_lock: true,
        quantize_step_64ths: Some(16),
        ..PitchPadProjection::default()
    });
    let early_action = early.capture(0, "pitch.extra:7:set_retrigger");
    assert_eq!(early_action.original_due_frame(), ORIGINAL_TARGET);
    early.publish(&early_action);
    assert_eq!(early_action.execution_target(), ORIGINAL_TARGET);
    early.wait_worker(&[&early_action]);
    let old_to_t = early.render_to(ORIGINAL_TARGET);
    assert!(
        old_to_t == late.render_to(ORIGINAL_TARGET),
        "old PCM before T must match"
    );
    assert!(!early_action.effective());
    assert_eq!(early.starts(0), 0);
    assert_eq!(late.starts(0), 0);

    // This independent actual capture occurs at the same original T. Admission at
    // F=T needs its own worker and freezes E=T+8192; no action metadata is relabelled.
    let old_generation = late.voice(0).generation;
    let old_native = late.voice(0).stretch.native_state_address();
    let old_lifetime = late.voice(0).history_lifetime.clone();
    let late_action = late.capture(0, "pitch.extra:7:set_retrigger");
    assert_eq!(late_action.original_due_frame(), ORIGINAL_TARGET);
    late.publish(&late_action);
    assert_eq!(late_action.execution_target(), LATE_EXECUTION);
    late.wait_worker(&[&late_action]);
    let old_before_e = late.render_to(LATE_EXECUTION);
    assert!(old_before_e.iter().any(|sample| sample.abs() > 0.01));
    assert!(!late_action.effective());
    assert_eq!(late.starts(0), 0);
    assert_eq!(late.voice(0).generation, old_generation);
    assert_eq!(late.voice(0).stretch.native_state_address(), old_native);
    assert_eq!(old_lifetime.load(Ordering::Acquire), old_generation);

    // Only the declared absolute T/E boundaries exclude the old voices. Every
    // sample actually emitted at/after late E participates, including first FIFO PCM.
    let early_pcm = render_partitioned(&mut early, EMITTED_FRAMES, &[127, 1, 509, 31, 256]);
    let late_pcm = render_partitioned(&mut late, EMITTED_FRAMES, &[257, 17, 1, 383, 96]);
    let expected_pcm = early_pcm.as_slice();
    assert_eq!(late_pcm.len(), EMITTED_FRAMES);
    assert_eq!(late_pcm.len(), expected_pcm.len());
    assert_applied(&early_action, ORIGINAL_TARGET, ORIGINAL_TARGET);
    assert_applied(&late_action, ORIGINAL_TARGET, LATE_EXECUTION);
    assert_eq!(early.starts(0), 1);
    assert_eq!(late.starts(0), 1);
    assert_eq!(late.voice(0).generation, old_generation + 1);
    assert_eq!(old_lifetime.load(Ordering::Acquire), old_generation + 1);
    let pitch = PitchIntent::new(0, 7).unwrap();
    let early_voice = early.voice(0);
    let late_voice = late.voice(0);
    for (voice, action) in [(early_voice, &early_action), (late_voice, &late_action)] {
        assert!(voice.stretch.owns_pitch_contract(pitch, true));
        assert_eq!(voice.stretch.adopted_request_id(), Some(action.sequence()));
        assert_ne!(voice.stretch.native_state_address(), 0);
        assert_eq!(voice.source_playback.tempo_ratio(), rate);
        assert_eq!(voice.source_playback.rate_target(), rate);
    }
    assert_ne!(
        early_voice.stretch.native_state_address(),
        late_voice.stretch.native_state_address(),
        "the early reference must own an independent Native"
    );
    assert_ne!(late_voice.stretch.native_state_address(), old_native);
    assert!(
        early_voice
            .source_playback
            .matches_exact(&late_voice.source_playback)
    );
    assert_eq!(
        late_voice.stretch.native_primed_output_frames(),
        early_voice.stretch.native_primed_output_frames()
    );
    assert_eq!(
        late_voice.stretch.pending_fifo_frames(),
        early_voice.stretch.pending_fifo_frames()
    );
    let expected_source = LOOP_START as f64 + EMITTED_FRAMES as f64 * rate;
    assert!(expected_source < LOOP_END as f64);
    for voice in [early_voice, late_voice] {
        let position = voice.source_playback.position();
        assert_eq!(position.frame, expected_source.floor() as usize);
        assert!((position.fraction - expected_source.fract()).abs() < 1.0e-8);
    }
    assert!(expected_pcm.iter().any(|sample| sample.abs() > 0.01));
    let first_difference = late_pcm
        .iter()
        .zip(expected_pcm)
        .position(|(actual, expected)| actual != expected);
    let maximum_error = late_pcm
        .iter()
        .zip(expected_pcm)
        .map(|(actual, expected)| (*actual - *expected).abs())
        .fold(0.0_f32, f32::max);
    assert_eq!(
        first_difference, None,
        "active late initial/full Native PCM differs; rate={rate} policy={policy} maximum error={maximum_error}"
    );
}

#[test]
fn active_equal_execution_distinct_pitch_retriggers_keep_both_original_attacks_and_capture_order() {
    let mut fixture = active_fixture(1.0, "first_sound_at_grid");
    assert!(
        fixture
            .render_to(EARLY_CAPTURE)
            .iter()
            .any(|sample| sample.abs() > 0.01)
    );
    let old_generation = fixture.voice(0).generation;
    let old_lifetime = fixture.voice(0).history_lifetime.clone();
    let first = fixture.capture(0, "pitch.extra:7:set_retrigger");
    let second = fixture.capture(0, "pitch.extra:3:set_retrigger");
    assert!(first.sequence() < second.sequence());
    assert_eq!(first.content_key(), second.content_key());
    assert_eq!(first.original_due_frame(), EARLY_CAPTURE);
    assert_eq!(second.original_due_frame(), EARLY_CAPTURE);
    assert_eq!(first.render().pitch, PitchIntent::new(0, 7).unwrap());
    assert_eq!(second.render().pitch, PitchIntent::new(0, 3).unwrap());
    // Reverse publication cannot turn two separately accepted originals into the
    // first request only, or let its old-voice lifetime cancel the second request.
    fixture.publish(&second);
    fixture.publish(&first);
    let execution = EARLY_CAPTURE + 8192;
    assert_eq!(first.execution_target(), execution);
    assert_eq!(second.execution_target(), execution);
    assert_eq!(fixture.scheduler.len(), 2);
    fixture.wait_worker(&[&first, &second]);
    fixture.render_to(execution);
    assert!(!first.effective() && !second.effective());
    assert_eq!(fixture.starts(0), 0);
    assert_eq!(fixture.voice(0).generation, old_generation);
    assert_eq!(old_lifetime.load(Ordering::Acquire), old_generation);
    fixture.render(1);
    assert_applied(&first, EARLY_CAPTURE, execution);
    assert_applied(&second, EARLY_CAPTURE, execution);
    assert_eq!(fixture.starts(0), 2);
    assert_eq!(fixture.voice(0).generation, old_generation + 2);
    assert_eq!(old_lifetime.load(Ordering::Acquire), old_generation + 2);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(second.render().pitch)
    );
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(second.sequence())
    );
    assert!(
        fixture
            .voice(0)
            .stretch
            .owns_pitch_contract(second.render().pitch, true)
    );
    let position = fixture.voice(0).source_playback.position();
    assert_eq!(position.frame, LOOP_START + 1);
    assert!(position.fraction.abs() < 1.0e-9);
    assert!(fixture.scheduler.is_empty());
    assert!(fixture.control.engine.publish_pitch_action(&first).unwrap());
    fixture.drain();
    assert_eq!(fixture.starts(0), 2, "an original owner remains one-shot");
    assert!(fixture.scheduler.is_empty());
}

#[test]
fn active_prepared_new_launch_survives_the_old_voices_pause_resume_aba() {
    let mut fixture = active_fixture(1.0, "first_sound_at_grid");
    assert!(
        fixture
            .render_to(EARLY_CAPTURE)
            .iter()
            .any(|sample| sample.abs() > 0.01)
    );
    let old_generation = fixture.voice(0).generation;
    let old_native = fixture.voice(0).stretch.native_state_address();
    let old_lifetime = fixture.voice(0).history_lifetime.clone();
    let old_position = fixture.voice(0).source_playback.position();
    let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
    fixture.publish(&action);
    fixture.wait_worker(&[&action]);
    let execution = EARLY_CAPTURE + 8192;
    assert_eq!(action.original_due_frame(), EARLY_CAPTURE);
    assert_eq!(action.execution_target(), execution);

    // Real effective controls return all observable pause/phase/generation fields
    // to their earlier values. This does not change the independent new launch's
    // constant rate, source or clock. Old-voice replay has separate strict fences.
    fixture.mixer.pause_sample_at_output_frame(0, EARLY_CAPTURE);
    assert!(fixture.voice(0).paused);
    fixture
        .mixer
        .resume_sample_at_output_frame(0, EARLY_CAPTURE);
    assert!(!fixture.voice(0).paused);
    assert_eq!(fixture.voice(0).source_playback.position(), old_position);
    assert_eq!(fixture.voice(0).generation, old_generation);
    assert_eq!(old_lifetime.load(Ordering::Acquire), old_generation);
    let old_pcm = fixture.render_to(execution);
    assert!(old_pcm.iter().any(|sample| sample.abs() > 0.01));
    assert!(!action.effective());
    assert_eq!(fixture.voice(0).generation, old_generation);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), old_native);
    let mut beginning = fixture.render(1);
    assert_applied(&action, EARLY_CAPTURE, execution);
    assert_eq!(fixture.starts(0), 1);
    assert_eq!(fixture.voice(0).generation, old_generation + 1);
    assert_eq!(old_lifetime.load(Ordering::Acquire), old_generation + 1);
    assert_ne!(fixture.voice(0).stretch.native_state_address(), old_native);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(action.render().pitch)
    );
    assert!(
        fixture
            .voice(0)
            .stretch
            .owns_pitch_contract(action.render().pitch, true)
    );
    assert_eq!(
        fixture.voice(0).source_playback.position().frame,
        LOOP_START + 1
    );
    assert!(!fixture.voice(0).paused);
    assert!(fixture.scheduler.is_empty());
    // The first due-frame sample was consumed above. Preserve that complete
    // physical prefix, including genuine Native startup, in an independent oracle.
    beginning.extend(fixture.pcm(16_383));
    crate::audio_engine::pitch_runtime_tests::assert_physical_birth_tone(
        &beginning, 0, 7, 1.0, true, 1.0,
    );
}

#[test]
fn active_prepared_new_launch_rejects_rate_and_loop_control_aba_without_overwriting_old_voice() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        for loop_edit in [false, true] {
            let mut fixture = active_fixture(1.0, policy);
            fixture.render_to(EARLY_CAPTURE);
            let old_generation = fixture.voice(0).generation;
            let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
            fixture.publish(&action);
            fixture.wait_worker(&[&action]);
            let execution = EARLY_CAPTURE + 8192;
            if loop_edit {
                fixture.mixer.set_pad_loop_region(
                    0,
                    (LOOP_START + 2400) as f64 / f64::from(RATE),
                    Some((LOOP_END - 2400) as f64 / f64::from(RATE)),
                );
                fixture.mixer.set_pad_loop_region(
                    0,
                    LOOP_START as f64 / f64::from(RATE),
                    Some(LOOP_END as f64 / f64::from(RATE)),
                );
            } else {
                fixture.mixer.set_speed(1.37);
                fixture.mixer.set_speed(1.0);
            }
            fixture.render_to(execution);
            assert!(!action.effective());
            fixture.render(1);
            assert_eq!(action.state(), "rejected");
            assert_eq!(action.reason(), Some("timing_changed"));
            assert_eq!(action.original_due_frame(), EARLY_CAPTURE);
            assert_eq!(action.execution_target(), execution);
            assert_eq!(fixture.starts(0), 0);
            assert_eq!(fixture.voice(0).generation, old_generation);
            assert_eq!(
                fixture.mixer.pad_pitch_intent(0),
                Some(PitchIntent::default())
            );
            assert!(fixture.scheduler.is_empty());
            assert!(fixture.pcm(1024).iter().any(|sample| sample.abs() > 0.01));
        }
    }
}
