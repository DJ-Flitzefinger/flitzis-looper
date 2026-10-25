//! A late real resident ACK retains the complete selected beginning at its actual effect.
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
fn late_original_gui_start_emits_the_independent_early_native_beginning_from_its_first_sample() {
    for rate in [0.73, 1.0, 1.37] {
        for policy in ["first_sound_at_grid", "attack_at_grid"] {
            compare_late_start(rate, policy, false);
        }
    }
}

#[test]
fn missing_map_pending_native_late_start_keeps_the_complete_original_beginning() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        compare_late_start(1.0, policy, true);
    }
}

fn compare_late_start(rate: f64, policy: &str, pending_map: bool) {
    let mut early = Fixture::new(true);
    let mut late = Fixture::new(true);
    let _early_files = install_synthetic_complete_lease(&mut early);
    let _late_files = install_synthetic_complete_lease(&mut late);
    let pitch = PitchIntent::new(0, 7).unwrap();
    early.mixer.set_pad_pitch_intent(0, pitch).unwrap();
    late.mixer.set_pad_pitch_intent(0, pitch).unwrap();
    early.mixer.set_speed(rate);
    late.mixer.set_speed(rate);
    for fixture in [&mut early, &mut late] {
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

    // Both are genuine public OrdinaryStart captures. CapturePadStartTarget is
    // consumed before either capture's own actual resident publication is ACKed.
    if pending_map {
        publish_missing_map_intent(&late);
    }
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

    // This fixture keeps its real resident transaction queued past T. The eventual
    // ACK determines E = 30000 + 8192; it must not replace or requantize T.
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
    assert_eq!(late_start.execution_frame(), Some(LATE_EXECUTION));
    wait_ready(&mut late, &[late_start.sequence()]);
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
        late.render_to(LATE_EXECUTION)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(early.starts(0), 0);
    assert_eq!(late.starts(0), 0);
    let early_pcm = render_partitioned(&mut early, EMITTED_FRAMES, &[127, 1, 509, 31, 256]);
    // Every sample actually emitted by the late fixture participates. In
    // particular, no initial Native/FIFO PCM is discarded to align the result.
    let late_pcm = render_partitioned(&mut late, EMITTED_FRAMES, &[257, 17, 1, 383, 96]);
    assert_eq!(late_pcm.len(), EMITTED_FRAMES);
    let expected_pcm = early_pcm.as_slice();
    assert_eq!(expected_pcm.len(), late_pcm.len());

    assert_eq!(early_start.state(), "applied", "{:?}", early_start.reason());
    assert_eq!(late_start.state(), "applied", "{:?}", late_start.reason());
    assert_eq!(early_start.execution_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(early_start.effective_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(late_start.execution_frame(), Some(LATE_EXECUTION));
    assert_eq!(late_start.effective_frame(), Some(LATE_EXECUTION));
    assert_eq!(early_start.original_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(late_start.original_frame(), Some(ORIGINAL_TARGET));
    assert_eq!(early.starts(0), 1);
    assert_eq!(late.starts(0), 1);
    let early_voice = early.voice(0);
    let late_voice = late.voice(0);
    assert!(early_voice.stretch.owns_pitch_contract(pitch, true));
    assert!(late_voice.stretch.owns_pitch_contract(pitch, true));
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
        "the reference must own an independent Native"
    );
    assert_eq!(
        late_voice.stretch.native_primed_output_frames(),
        early_voice.stretch.native_primed_output_frames(),
        "unemitted delay must not add Native history to the new birth"
    );
    let expected_source = LOOP_START as f64 + EMITTED_FRAMES as f64 * rate;
    assert!(expected_source < LOOP_END as f64);
    for voice in [early_voice, late_voice] {
        let position = voice.source_playback.position();
        assert_eq!(
            position.frame,
            expected_source.floor() as usize,
            "logical phase must include only actually emitted birth frames"
        );
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
        "late initial/full PCM differs from the complete independent Native beginning; rate={rate} policy={policy} maximum error={maximum_error}"
    );
}
