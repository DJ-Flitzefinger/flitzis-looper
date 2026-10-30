//! Nonneutral EQ processes the complete new birth after a late resident ACK.
#![cfg(windows)]

use super::*;

const CAPTURE_FRAME: u64 = 1_000;
const ORIGINAL_TARGET: u64 = 24_000;
const LATE_ACK_FRAME: u64 = 30_000;
const LATE_EXECUTION: u64 = 38_192;
const EMITTED_FRAMES: usize = 24_000;

fn capture_original_grid_target(fixture: &mut Fixture, original: &CapturedPadStart) {
    let message = fixture.control.consumer.pop().unwrap();
    assert!(matches!(&message, ControlMessage::CapturePadStartTarget(_)));
    process_control_message(
        message,
        &mut fixture.scheduler,
        fixture.transport.output_frame(),
        &mut TriggerQuantization::Grid { step_64ths: 16 },
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    assert_eq!(original.target_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(original.original_frame(), Some(ORIGINAL_TARGET));
}

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

#[test]
fn late_non_neutral_eq_keeps_complete_beginning_at_073_first_sound() {
    compare_late_start(0.73, "first_sound_at_grid");
}

#[test]
fn late_non_neutral_eq_keeps_complete_beginning_at_073_attack() {
    compare_late_start(0.73, "attack_at_grid");
}

#[test]
fn late_non_neutral_eq_keeps_complete_beginning_at_unity_first_sound() {
    compare_late_start(1.0, "first_sound_at_grid");
}

#[test]
fn late_non_neutral_eq_keeps_complete_beginning_at_unity_attack() {
    compare_late_start(1.0, "attack_at_grid");
}

#[test]
fn late_non_neutral_eq_keeps_complete_beginning_at_137_first_sound() {
    compare_late_start(1.37, "first_sound_at_grid");
}

#[test]
fn late_non_neutral_eq_keeps_complete_beginning_at_137_attack() {
    compare_late_start(1.37, "attack_at_grid");
}

#[test]
fn late_dry_non_neutral_eq_and_trim_keep_complete_beginning_first_sound() {
    compare_late_start_contract(1.0, "first_sound_at_grid", 0, false, -4.5);
}

#[test]
fn late_dry_unity_lock_on_eq_and_trim_keep_complete_beginning_attack() {
    compare_late_start_contract(1.0, "attack_at_grid", 0, true, -4.5);
}

fn compare_late_start(rate: f64, policy: &str) {
    compare_late_start_contract(rate, policy, 7, true, 0.0);
}

fn compare_late_start_contract(rate: f64, policy: &str, semitones: i32, lock: bool, trim_db: f32) {
    let late_execution = if semitones == 0 {
        LATE_ACK_FRAME
    } else {
        LATE_EXECUTION
    };
    let mut early = Fixture::new(lock);
    let mut late = Fixture::new(lock);
    let _early_files = install_synthetic_complete_lease(&mut early);
    let _late_files = install_synthetic_complete_lease(&mut late);
    let pitch = PitchIntent::new(0, semitones).unwrap();
    early.mixer.set_pad_pitch_intent(0, pitch).unwrap();
    late.mixer.set_pad_pitch_intent(0, pitch).unwrap();
    early.mixer.set_speed(rate);
    late.mixer.set_speed(rate);
    for fixture in [&mut early, &mut late] {
        fixture.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
        fixture.mixer.set_pad_gain(0, trim_db);
        fixture.drain();
        assert!(fixture.render(1).iter().all(|sample| *sample == 0.0));
        assert!(
            fixture
                .control
                .engine
                .set_key_lock_start_behavior(policy)
                .unwrap()
        );
    }
    assert!(
        early
            .render_to(CAPTURE_FRAME)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert!(
        late.render_to(CAPTURE_FRAME)
            .iter()
            .all(|sample| *sample == 0.0)
    );

    // Each fixture owns its own public capture, real finite source and resident ACK.
    let early_start = capture(&early, 1.0);
    let early_window = early
        .control
        .engine
        .prepare_captured_pad_start(&early_start)
        .unwrap();
    let late_start = capture(&late, 1.0);
    let late_window = late
        .control
        .engine
        .prepare_captured_pad_start(&late_start)
        .unwrap();
    assert_ne!(early_window.publication_status(), "accepted");
    assert_ne!(late_window.publication_status(), "accepted");
    capture_original_grid_target(&mut early, &early_start);
    capture_original_grid_target(&mut late, &late_start);

    drain_with_mode(&mut early, TriggerQuantization::Grid { step_64ths: 16 });
    assert_eq!(
        early_window.publication_status(),
        "accepted",
        "{:?}",
        early_window.error()
    );
    assert!(early_window.is_current());
    assert!(
        early
            .control
            .engine
            .publish_pad_start(&early_start, Some(&early_window))
            .unwrap()
    );
    drain_with_mode(&mut early, TriggerQuantization::Grid { step_64ths: 16 });
    assert!(early_start.accepted(), "{:?}", early_start.reason());
    assert_eq!(early_start.execution_frame(), Some(ORIGINAL_TARGET));
    wait_ready(&mut early, &[early_start.sequence()]);

    // The late source remains absent through T. Neither its source nor fresh EQ
    // consumes that unheard delay; both begin with the independent early birth.
    assert!(
        late.render_to(LATE_ACK_FRAME)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_ne!(late_window.publication_status(), "accepted");
    drain_with_mode(&mut late, TriggerQuantization::Grid { step_64ths: 16 });
    assert_eq!(
        late_window.publication_status(),
        "accepted",
        "{:?}",
        late_window.error()
    );
    assert!(late_window.is_current());
    assert!(
        late.control
            .engine
            .publish_pad_start(&late_start, Some(&late_window))
            .unwrap()
    );
    drain_with_mode(&mut late, TriggerQuantization::Grid { step_64ths: 16 });
    assert!(late_start.accepted(), "{:?}", late_start.reason());
    assert_eq!(late_start.execution_frame(), Some(late_execution));
    if semitones != 0 {
        wait_ready(&mut late, &[late_start.sequence()]);
    }
    let late_execution = if policy == "first_sound_at_grid" {
        LATE_ACK_FRAME
    } else {
        late_execution
    };
    for start in [&early_start, &late_start] {
        assert_eq!(start.original_frame(), Some(ORIGINAL_TARGET));
        assert_eq!(start.target_frame(), Some(ORIGINAL_TARGET));
    }

    assert!(
        early
            .render_to(ORIGINAL_TARGET)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert!(
        late.render_to(late_execution)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(early.starts(0), 0);
    assert_eq!(late.starts(0), usize::from(semitones == 0));
    let early_pcm = render_partitioned(&mut early, EMITTED_FRAMES, &[127, 1, 509, 31, 256]);
    // Keep every emitted sample from each independent source and fresh EQ birth.
    let late_pcm = render_partitioned(&mut late, EMITTED_FRAMES, &[257, 17, 1, 383, 96]);
    assert_eq!(late_pcm.len(), EMITTED_FRAMES);
    let expected_pcm = early_pcm.as_slice();
    assert_eq!(expected_pcm.len(), late_pcm.len());

    assert_eq!(early_start.state(), "applied", "{:?}", early_start.reason());
    assert_eq!(late_start.state(), "applied", "{:?}", late_start.reason());
    assert_eq!(early_start.execution_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(early_start.effective_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(late_start.execution_frame(), Some(late_execution));
    assert_eq!(late_start.effective_frame(), Some(late_execution));
    assert_eq!(early_start.original_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(late_start.original_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(early.starts(0), 1);
    assert_eq!(late.starts(0), 1);
    let early_voice = early.voice(0);
    let late_voice = late.voice(0);
    if semitones != 0 {
        assert!(early_voice.stretch.owns_pitch_contract(pitch, lock));
        assert!(late_voice.stretch.owns_pitch_contract(pitch, lock));
        assert_eq!(
            early_voice.stretch.adopted_request_id(),
            Some(early_start.sequence())
        );
        assert_eq!(
            late_voice.stretch.adopted_request_id(),
            Some(late_start.sequence())
        );
        let early_native = early_voice.stretch.native_state_address();
        let late_native = late_voice.stretch.native_state_address();
        assert_ne!(early_native, 0);
        assert_ne!(late_native, 0);
        assert_ne!(
            early_native, late_native,
            "the reference owns an independent Native"
        );
        assert_eq!(
            late_voice.stretch.native_primed_output_frames(),
            early_voice.stretch.native_primed_output_frames(),
            "unemitted delay must not add Native history to the new birth"
        );
    } else {
        assert_eq!(early_voice.stretch.adopted_request_id(), None);
        assert_eq!(late_voice.stretch.adopted_request_id(), None);
    }
    let expected_source = LOOP_START as f64 + EMITTED_FRAMES as f64 * rate;
    assert!(expected_source < LOOP_END as f64);
    for voice in [early_voice, late_voice] {
        let position = voice.source_playback.position();
        assert_eq!(position.frame, expected_source.floor() as usize);
        assert!((position.fraction - expected_source.fract()).abs() < 1.0e-8);
        assert_eq!(voice.source_playback.tempo_ratio(), rate);
    }
    assert!(expected_pcm.iter().any(|sample| sample.abs() > 0.01));
    let maximum_error = late_pcm
        .iter()
        .zip(expected_pcm)
        .map(|(actual, expected)| (*actual - *expected).abs())
        .fold(0.0_f32, f32::max);
    let first_difference = late_pcm
        .iter()
        .zip(expected_pcm)
        .position(|(actual, expected)| actual != expected);
    assert_eq!(
        first_difference, None,
        "nonneutral EQ complete late beginning differs; rate={rate} policy={policy} maximum error={maximum_error}"
    );
}
