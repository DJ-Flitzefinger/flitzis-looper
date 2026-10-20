//! A real unquantized piano capture retains T when the next callback admits it after T.
#![cfg(windows)]

use super::*;
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::pitch_actions::CapturedPitchAction;
use crate::audio_engine::pitch_actions::PitchKeyMode;
use crate::messages::StemMixMode;

const EARLY_CAPTURE: u64 = 17_000;
const ORIGINAL_TARGET: u64 = 24_000;
const PREPARATION_LEAD: u64 = 8192;
const EMITTED_FRAMES: usize = 24_000;

fn row(quantize_step_64ths: Option<u32>) -> PitchPadProjection {
    PitchPadProjection {
        key_lock: true,
        source_root: Some(0),
        source_mode: Some(PitchKeyMode::Major),
        quantize_step_64ths,
        ..PitchPadProjection::default()
    }
}

fn active_fixture(rate: f64, policy: &str) -> Fixture {
    // The old voice remains dry while the new piano capture requests its own wet tuple.
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
    fixture.rows(row(None));
    assert!(fixture.mixer.play_sample(0, 1.0));
    fixture
}

fn render_partitioned(fixture: &mut Fixture, frames: usize, partitions: &[usize]) -> Vec<f32> {
    let mut pcm = Vec::with_capacity(frames);
    let mut rendered = 0;
    let mut partition = 0;
    while rendered < frames {
        let count = partitions[partition % partitions.len()].min(frames - rendered);
        pcm.extend(fixture.render(count));
        rendered += count;
        partition += 1;
    }
    pcm
}

fn compare_past_active_piano(rate: f64, policy: &str, delivery_delay: usize) {
    let mut early = active_fixture(rate, policy);
    let mut late = active_fixture(rate, policy);
    assert_eq!(
        early.render_to(EARLY_CAPTURE),
        late.render_to(EARLY_CAPTURE)
    );
    early.rows(row(Some(16)));
    let reference = early.capture(0, "pitch.note:7:set_retrigger");
    assert_eq!(reference.original_due_frame(), ORIGINAL_TARGET);
    early.publish(&reference);
    assert_eq!(reference.execution_target(), ORIGINAL_TARGET);
    early.wait_worker(&[&reference]);
    assert_eq!(
        early.render_to(ORIGINAL_TARGET),
        late.render_to(ORIGINAL_TARGET)
    );

    // Capture and publish the actual immediate public piano action at T. The old
    // voice renders before the next control drain, exactly as between GUI/device ticks.
    let old_generation = late.voice(0).generation;
    let old_native = late.voice(0).stretch.native_state_address();
    let action = late.capture(0, "pitch.note:7:set_retrigger");
    let timestamp = action.received_timestamp();
    assert_eq!(action.original_due_frame(), ORIGINAL_TARGET);
    assert!(late.control.engine.publish_pitch_action(&action).unwrap());
    assert!(!action.accepted());
    let before_admission = render_partitioned(&mut late, delivery_delay, &[31, 1, 127, 17]);
    assert_eq!(before_admission.len(), delivery_delay);
    let callback = ORIGINAL_TARGET + delivery_delay as u64;
    assert_eq!(late.transport.output_frame(), callback);
    assert!(action.original_due_frame() < callback);
    assert_eq!(late.voice(0).generation, old_generation);
    assert_eq!(late.voice(0).stretch.native_state_address(), old_native);
    late.drain();
    assert!(
        action.accepted(),
        "a genuine immediate piano action at T={} must remain usable at F={callback}; rate={rate} policy={policy} state={} reason={:?}",
        action.original_due_frame(),
        action.state(),
        action.reason()
    );
    let execution = callback + PREPARATION_LEAD;
    assert_eq!(action.execution_target(), execution);
    assert_eq!(action.original_due_frame(), ORIGINAL_TARGET);
    assert_eq!(action.received_timestamp(), timestamp);
    late.wait_worker(&[&action]);
    late.render_to(execution);
    assert!(!action.effective());
    assert_eq!(late.starts(0), 0);
    assert_eq!(late.voice(0).generation, old_generation);
    assert_eq!(late.voice(0).stretch.native_state_address(), old_native);

    let reference_pcm = render_partitioned(&mut early, EMITTED_FRAMES, &[127, 1, 509, 31, 256]);
    let actual_pcm = render_partitioned(&mut late, EMITTED_FRAMES, &[257, 17, 1, 383, 96]);
    let expected_pcm = &reference_pcm;
    assert_eq!(actual_pcm.len(), expected_pcm.len());
    assert!(expected_pcm.iter().any(|sample| sample.abs() > 0.01));
    let first_difference = actual_pcm
        .iter()
        .zip(expected_pcm)
        .position(|(actual, expected)| actual != expected);
    let maximum_error = actual_pcm
        .iter()
        .zip(expected_pcm)
        .map(|(actual, expected)| (*actual - *expected).abs())
        .fold(0.0_f32, f32::max);
    assert_eq!(
        first_difference, None,
        "complete first/continuing Native PCM differs from the independent complete on-time beginning; rate={rate} policy={policy} delay={delivery_delay} maximum error={maximum_error}"
    );
    assert_eq!(reference.state(), "applied", "{:?}", reference.reason());
    assert_eq!(action.state(), "applied", "{:?}", action.reason());
    assert!(reference.effective() && action.effective());
    assert_eq!(late.starts(0), 1);
    assert_eq!(late.voice(0).generation, old_generation + 1);
    assert_ne!(late.voice(0).stretch.native_state_address(), old_native);
    assert_eq!(
        late.voice(0).stretch.adopted_request_id(),
        Some(action.sequence())
    );
    assert_eq!(action.original_due_frame(), ORIGINAL_TARGET);
    assert_eq!(action.execution_target(), execution);
    assert_eq!(action.received_timestamp(), timestamp);
    assert_eq!(
        late.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(7, 0).unwrap())
    );
    assert!(
        early
            .voice(0)
            .source_playback
            .matches_exact(&late.voice(0).source_playback)
    );
    assert_eq!(
        late.voice(0).stretch.native_primed_output_frames(),
        early.voice(0).stretch.native_primed_output_frames()
    );
    assert_eq!(
        late.voice(0).stretch.pending_fifo_frames(),
        early.voice(0).stretch.pending_fifo_frames()
    );
    let expected_source = LOOP_START as f64 + EMITTED_FRAMES as f64 * rate;
    assert!(expected_source < LOOP_END as f64);
    let position = late.voice(0).source_playback.position();
    assert_eq!(position.frame, expected_source.floor() as usize);
    assert!((position.fraction - expected_source.fract()).abs() < 1.0e-8);
    assert_eq!(late.voice(0).source_playback.tempo_ratio(), rate);
    assert_eq!(late.voice(0).source_playback.rate_target(), rate);
}

#[test]
fn immediate_active_piano_retains_original_t_and_independent_native_beginning_after_next_callback()
{
    for rate in [0.73, 1.0, 1.37] {
        for policy in ["first_sound_at_grid", "attack_at_grid"] {
            compare_past_active_piano(rate, policy, 1);
        }
    }
}

#[test]
fn immediate_active_piano_past_capture_is_independent_of_unequal_callback_boundaries() {
    for delivery_delay in [31, 509] {
        compare_past_active_piano(0.73, "first_sound_at_grid", delivery_delay);
    }
}

#[test]
fn past_active_piano_refuses_rate_loop_and_pause_aba_inside_the_missed_interval() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        for change in ["rate", "loop", "pause"] {
            let mut fixture = active_fixture(1.0, policy);
            fixture.render_to(ORIGINAL_TARGET);
            let generation = fixture.voice(0).generation;
            let action = fixture.capture(0, "pitch.note:7:set_retrigger");
            let timestamp = action.received_timestamp();
            assert!(
                fixture
                    .control
                    .engine
                    .publish_pitch_action(&action)
                    .unwrap()
            );
            fixture.render(1);
            match change {
                "rate" => {
                    fixture.mixer.set_speed(1.37);
                    fixture.mixer.set_speed(1.0);
                }
                "loop" => {
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
                }
                "pause" => {
                    fixture
                        .mixer
                        .pause_sample_at_output_frame(0, ORIGINAL_TARGET + 1);
                    fixture
                        .mixer
                        .resume_sample_at_output_frame(0, ORIGINAL_TARGET + 1);
                }
                _ => unreachable!(),
            }
            fixture.render(1);
            fixture.drain();
            assert_eq!(
                action.state(),
                "rejected",
                "change={change} policy={policy}"
            );
            assert_eq!(action.reason(), Some("preparation_failed"));
            assert_eq!(action.original_due_frame(), ORIGINAL_TARGET);
            assert_eq!(action.received_timestamp(), timestamp);
            assert_eq!(fixture.voice(0).generation, generation);
            assert_eq!(fixture.starts(0), 0);
            assert_eq!(
                fixture.mixer.pad_pitch_intent(0),
                Some(PitchIntent::default())
            );
            assert!(fixture.scheduler.is_empty());
        }
    }
}

fn observed_fixture(policy: &str, paused: bool) -> Fixture {
    let mut fixture = active_fixture(1.0, policy);
    if paused {
        fixture.render_to(ORIGINAL_TARGET - 512);
        fixture
            .mixer
            .pause_sample_at_output_frame(0, ORIGINAL_TARGET - 512);
    }
    fixture.render_to(ORIGINAL_TARGET);
    fixture
}

fn publish_past(fixture: &mut Fixture) -> CapturedPitchAction {
    let original = fixture.transport.output_frame();
    let action = fixture.capture(0, "pitch.note:7:set_retrigger");
    assert_eq!(action.original_due_frame(), original);
    assert!(
        fixture
            .control
            .engine
            .publish_pitch_action(&action)
            .unwrap()
    );
    fixture.render(1);
    assert!(!action.accepted());
    action
}

fn admit_past(fixture: &mut Fixture) -> CapturedPitchAction {
    let action = publish_past(fixture);
    fixture.drain();
    assert!(action.accepted(), "{:?}", action.reason());
    assert_eq!(
        action.execution_target(),
        action.original_due_frame() + 1 + PREPARATION_LEAD
    );
    fixture.wait_worker(&[&action]);
    action
}

fn rate_or_loop_aba(fixture: &mut Fixture, change: &str) {
    match change {
        "rate" => {
            fixture.mixer.set_speed(1.37);
            fixture.mixer.set_speed(1.0);
        }
        "loop" => {
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
        }
        _ => unreachable!(),
    }
}

fn source_aba(fixture: &mut Fixture) {
    let original = fixture.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let replacement = fixture.control.engine.sample_cache.lock().unwrap()[1]
        .clone()
        .unwrap();
    // Both physical sources and fresh content lifetimes are actually published;
    // returning to the original Arc cannot resurrect the earlier action permit.
    publish_tone(&fixture.control, 0, replacement.clone(), true);
    fixture.mixer.load_sample(0, replacement);
    publish_tone(&fixture.control, 0, original.clone(), true);
    fixture.mixer.load_sample(0, original);
}

#[test]
fn past_active_and_paused_piano_reject_rate_loop_and_source_aba_after_admission() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        for paused in [false, true] {
            for change in ["rate", "loop", "source"] {
                let mut fixture = observed_fixture(policy, paused);
                let action = admit_past(&mut fixture);
                let timestamp = action.received_timestamp();
                if change == "source" {
                    source_aba(&mut fixture);
                } else {
                    rate_or_loop_aba(&mut fixture, change);
                }
                fixture.render_to(action.execution_target());
                fixture.render(1);
                assert_eq!(
                    action.state(),
                    "rejected",
                    "change={change} paused={paused} policy={policy} reason={:?}",
                    action.reason()
                );
                assert_eq!(
                    action.reason(),
                    Some(if change == "source" {
                        "stale_content"
                    } else {
                        "timing_changed"
                    })
                );
                assert!(!action.effective());
                assert_eq!(action.original_due_frame(), ORIGINAL_TARGET);
                assert_eq!(
                    action.execution_target(),
                    ORIGINAL_TARGET + 1 + PREPARATION_LEAD
                );
                assert_eq!(action.received_timestamp(), timestamp);
                assert_eq!(fixture.starts(0), 0);
                assert_eq!(
                    fixture.mixer.pad_pitch_intent(0),
                    Some(PitchIntent::default())
                );
                assert!(fixture.scheduler.is_empty());
            }
        }
    }
}

#[test]
fn past_paused_piano_rejects_rate_loop_and_source_aba_before_admission() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        for change in ["rate", "loop", "source"] {
            let mut fixture = observed_fixture(policy, true);
            let action = publish_past(&mut fixture);
            if change == "source" {
                source_aba(&mut fixture);
            } else {
                rate_or_loop_aba(&mut fixture, change);
            }
            fixture.render(1);
            fixture.drain();
            assert_eq!(
                action.state(),
                "rejected",
                "change={change} policy={policy} reason={:?}",
                action.reason()
            );
            assert_eq!(
                action.reason(),
                Some(if change == "source" {
                    "stale_content"
                } else {
                    "preparation_failed"
                })
            );
            assert!(!action.effective());
            assert_eq!(action.original_due_frame(), ORIGINAL_TARGET);
            assert_eq!(fixture.starts(0), 0);
            assert!(fixture.scheduler.is_empty());
        }
    }
}

fn stem_fixture(policy: &str, paused: bool) -> (Fixture, u64) {
    let (mut fixture, _source, stems, _) = crate::audio_engine::pitch_runtime_tests::source_feed_tests::selection_source_feed_tests::selection_setup(false, PitchIntent::default());
    fixture.control.engine.stop_sample(0).unwrap();
    fixture.drain();
    fixture.render(1);
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior(policy)
            .unwrap()
    );
    fixture.rows(row(None));
    assert!(
        fixture
            .mixer
            .set_stem_mix_mode(0, StemMixMode::AllStems, stems.source_version_hash)
    );
    assert!(fixture.mixer.play_sample(0, 1.0));
    if paused {
        fixture.render_to(ORIGINAL_TARGET - 512);
        fixture
            .mixer
            .pause_sample_at_output_frame(0, ORIGINAL_TARGET - 512);
    }
    fixture.render_to(ORIGINAL_TARGET);
    (fixture, stems.source_version_hash)
}

#[test]
fn past_active_and_paused_piano_reject_real_stem_selection_aba_before_and_after_admission() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        for paused in [false, true] {
            for after_admission in [false, true] {
                let (mut fixture, source_hash) = stem_fixture(policy, paused);
                let starts_before = fixture.starts(0);
                let action = if after_admission {
                    admit_past(&mut fixture)
                } else {
                    publish_past(&mut fixture)
                };
                assert!(
                    fixture
                        .mixer
                        .set_stem_mix_mode(0, StemMixMode::FullMix, source_hash)
                );
                assert!(
                    fixture
                        .mixer
                        .set_stem_mix_mode(0, StemMixMode::AllStems, source_hash)
                );
                if after_admission {
                    fixture.render_to(action.execution_target());
                    fixture.render(1);
                } else {
                    fixture.render(1);
                    fixture.drain();
                }
                assert_eq!(
                    action.state(),
                    "rejected",
                    "paused={paused} after_admission={after_admission} policy={policy} reason={:?}",
                    action.reason()
                );
                assert_eq!(
                    action.reason(),
                    Some(if after_admission {
                        "timing_changed"
                    } else {
                        "preparation_failed"
                    })
                );
                assert!(!action.effective());
                assert_eq!(action.original_due_frame(), ORIGINAL_TARGET);
                assert_eq!(fixture.starts(0), starts_before);
                assert!(fixture.scheduler.is_empty());
            }
        }
    }
}

#[test]
fn past_active_and_paused_new_launch_survives_old_voice_pause_or_independent_birth() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        for paused in [false, true] {
            for birth in [false, true] {
                let mut fixture = observed_fixture(policy, paused);
                let action = admit_past(&mut fixture);
                let prepared_native = fixture
                    .mixer
                    .pitch_runtime
                    .as_ref()
                    .unwrap()
                    .ready_pitch_native_address(action.sequence())
                    .unwrap();
                let frame = fixture.transport.output_frame();
                if birth {
                    assert!(fixture.mixer.play_sample(0, 0.7));
                } else if paused {
                    fixture.mixer.resume_sample_at_output_frame(0, frame);
                    fixture.mixer.pause_sample_at_output_frame(0, frame);
                } else {
                    fixture.mixer.pause_sample_at_output_frame(0, frame);
                    fixture.mixer.resume_sample_at_output_frame(0, frame);
                }
                fixture.render_to(action.execution_target());
                assert!(!action.effective());
                fixture.render(1);
                assert_eq!(
                    action.state(),
                    "applied",
                    "paused={paused} birth={birth} policy={policy} reason={:?}",
                    action.reason()
                );
                assert!(action.effective());
                assert_eq!(action.original_due_frame(), ORIGINAL_TARGET);
                assert_eq!(fixture.starts(0), 1);
                assert_eq!(
                    fixture.voice(0).stretch.native_state_address(),
                    prepared_native
                );
                assert_eq!(
                    fixture.voice(0).stretch.adopted_request_id(),
                    Some(action.sequence())
                );
                assert!(!fixture.voice(0).paused);
                assert_eq!(
                    fixture.voice(0).source_playback.position().frame,
                    LOOP_START + 1
                );
                assert!(fixture.scheduler.is_empty());
            }
        }
    }
}
