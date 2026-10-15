//! Actual MIDI dispatcher captures the original attack before the one callback path.

use super::input_mapping::CapturedInputPadTrigger;
use super::input_mapping::launch_admission_tests::RATE;
use super::pad_start::CapturedPadStart;
use super::pitch_intent::PitchIntent;
use super::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START, assert_tone};
use crate::messages::ControlMessage;
use std::time::{Duration, Instant};

const PCM_FRAMES: usize = 48_000;
const HOST_WAIT: Duration = Duration::from_secs(2);

fn injected_original(fixture: &Fixture, dispatched: bool) -> CapturedInputPadTrigger {
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    runtime.set_enabled(true);
    runtime.replace_mappings(vec![("midi:note:1:60".into(), "pad.trigger:0".into())]);
    assert!(runtime.inject_midi_message(&[0x90, 60, 37]));
    let deadline = Instant::now() + HOST_WAIT;
    loop {
        if let Some(event) = runtime.poll_event() {
            assert_eq!(event.binding_key, "midi:note:1:60");
            assert_eq!(event.action_key.as_deref(), Some("pad.trigger:0"));
            assert_eq!(event.value, 37);
            assert!(event.direct);
            assert_eq!(event.dispatched, dispatched);
            assert!(event.pitch_rejection.is_none());
            let trigger = event
                .captured_trigger
                .expect("actual original MIDI capture");
            assert_eq!(trigger.received_at_ns(), event.received_at_ns);
            assert_eq!(
                trigger.pad_start().received_timestamp(),
                event.received_at_ns
            );
            assert!(event.received_at_ns > 0);
            return trigger;
        }
        assert!(
            Instant::now() < deadline,
            "actual MIDI dispatcher exceeded its host wait"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_ready(fixture: &mut Fixture, owner: &CapturedPadStart) {
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + HOST_WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.has_ready_action(owner.sequence()) {
            break;
        }
        assert_ne!(owner.state(), "rejected", "reason={:?}", owner.reason());
        assert!(
            Instant::now() < deadline,
            "MIDI Native worker exceeded its host wait"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
}

fn initial_pcm(fixture: &mut Fixture, owner: &CapturedPadStart) -> Vec<f32> {
    let execution = owner.execution_frame().unwrap();
    assert!(
        fixture
            .render_to(execution)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let mut pcm = fixture.render(1);
    pcm.extend(fixture.pcm(PCM_FRAMES - 1));
    assert_eq!(owner.state(), "applied", "reason={:?}", owner.reason());
    assert_eq!(owner.effective_frame(), Some(execution));
    pcm
}

fn assert_native_first_attack(fixture: &Fixture, owner: &CapturedPadStart, pitch: PitchIntent) {
    assert_eq!(fixture.starts(0), 1);
    assert_eq!(fixture.mixer.pad_pitch_intent(0), Some(pitch));
    assert!(fixture.mixer.key_lock_for_measurement(0));
    let voice = fixture.voice(0);
    // The established pad-trigger action has unity volume; NoteOn velocity is the
    // gesture's value and must not rewrite the captured launch volume on a retry.
    assert_eq!(voice.volume, 1.0);
    assert_eq!(voice.stretch.adopted_request_id(), Some(owner.sequence()));
    assert_ne!(voice.stretch.native_state_address(), 0);
    assert!(voice.stretch.owns_pitch_contract(pitch, true));
    let history = voice.stretch.productive_history().unwrap();
    assert!(history.fed_output_frames >= 4096);
    assert_eq!(
        history.binding.source_address,
        voice.sample.as_ref().unwrap().source_address()
    );
    assert_eq!(
        history.binding.resident,
        voice.sample.as_ref().unwrap().resident_binding()
    );
    let taps = voice.stretch.native_tap_observation().unwrap();
    assert!(taps.left_reads > 0);
    assert_eq!(taps.missing_reads, 0);
    assert_eq!(
        voice.source_playback.position().frame,
        LOOP_START + PCM_FRAMES
    );
    assert!(voice.source_playback.position().fraction.abs() < 1.0e-9);
}

fn assert_releases_create_no_attack(fixture: &mut Fixture) {
    let starts = fixture.starts(0);
    let generation = fixture.voice(0).generation;
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    assert!(!runtime.inject_midi_message(&[0x80, 60, 0]));
    assert!(!runtime.inject_midi_message(&[0x90, 60, 0]));
    assert!(runtime.poll_event().is_none());
    assert!(fixture.control.consumer.is_empty());
    fixture.drain();
    fixture.render(1);
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(fixture.voice(0).generation, generation);
}

#[test]
fn actual_midi_direct_first_native_attack_retains_original_owner_and_full_initial_pcm() {
    for (base, extra) in [(0, 7), (6, 18)] {
        let mut midi = Fixture::new(true);
        let mut reference = Fixture::new(true);
        let pitch = PitchIntent::new(base, extra).unwrap();
        midi.mixer.set_pad_pitch_intent(0, pitch).unwrap();
        reference.mixer.set_pad_pitch_intent(0, pitch).unwrap();
        let trigger = injected_original(&midi, true);
        let original = trigger.pad_start().clone();
        let timestamp = original.received_timestamp();
        assert!(original.is_published());
        assert!(
            !original.accepted(),
            "dispatcher queueing is not callback admission"
        );
        assert!(matches!(
            midi.control.consumer.peek().unwrap(),
            ControlMessage::CapturePadStartTarget(_)
        ));
        midi.drain();
        assert!(midi.control.consumer.is_empty());
        assert_eq!(midi.scheduler.len(), 1);
        assert!(original.accepted(), "reason={:?}", original.reason());
        assert_eq!(original.target_frame(), Some(0));
        assert_eq!(original.execution_frame(), Some(8192));
        assert_eq!(midi.starts(0), 0);

        // A fresh GUI origin owns a separate candidate and request. The MIDI source
        // must render the same whole first attack without recapturing after ACK.
        let expected = reference
            .control
            .engine
            .capture_pad_start(
                0,
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        assert!(
            reference
                .control
                .engine
                .publish_pad_start(&expected, None)
                .unwrap()
        );
        reference.drain();
        wait_ready(&mut midi, &original);
        wait_ready(&mut reference, &expected);
        let pcm = initial_pcm(&mut midi, &original);
        let reference_pcm = initial_pcm(&mut reference, &expected);
        assert_eq!(original.received_timestamp(), timestamp);
        assert_eq!(trigger.original_owner_status(), "admitted");
        assert_native_first_attack(&midi, &original, pitch);
        let initial_energy = pcm[..2048]
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / 2048.0;
        assert!(
            initial_energy > 0.001,
            "MIDI first Native/FIFO attack was silent"
        );
        let maximum_error = pcm
            .iter()
            .zip(&reference_pcm)
            .map(|(actual, expected)| (*actual - *expected).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            maximum_error < 2.0e-5,
            "complete MIDI initial PCM differs: {maximum_error}"
        );
        // Independent raw source frequency and integer shifts; include sample 0 and
        // every startup/FIFO sample, rather than dropping an initial analysis slice.
        assert_tone(&pcm, base, extra, 1.0, true);
        assert_releases_create_no_attack(&mut midi);
    }
}

#[cfg(windows)]
fn refresh_actual_runtime_bindings(fixture: &Fixture) {
    use super::constants::NUM_SAMPLES;

    let mut bindings = vec![None; NUM_SAMPLES];
    for id in [0, 1] {
        bindings[id] = fixture
            .control
            .engine
            .current_input_runtime_pad_binding(id)
            .unwrap();
    }
    fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state(
            true,
            bindings.iter().map(Option::is_some).collect(),
            vec![LOOP_START as f64 / f64::from(RATE); NUM_SAMPLES],
            vec![Some(LOOP_END as f64 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
        )
        .unwrap();
}

#[cfg(windows)]
#[test]
fn actual_midi_pressure_fallback_keeps_original_target_through_own_ack_and_first_native_attack() {
    use super::audio_stream::drain_control_messages;
    use super::ordinary_start_tests::install_synthetic_complete_lease;
    use crate::messages::TriggerQuantization;

    let mut fixture = Fixture::new(true);
    let _synthetic_files = install_synthetic_complete_lease(&mut fixture);
    refresh_actual_runtime_bindings(&fixture);
    let pitch = PitchIntent::new(0, 7).unwrap();
    fixture.mixer.set_pad_pitch_intent(0, pitch).unwrap();
    fixture.render_to(1000);
    {
        let mut producer = fixture.control.producer.lock().unwrap();
        while producer.slots() > 1 {
            producer.push(ControlMessage::Ping()).unwrap();
        }
    }
    // The real scalar target publication takes the final queue slot. The same
    // original captured trigger returns undispatched for Controller readiness.
    let trigger = injected_original(&fixture, false);
    let original = trigger.pad_start().clone();
    let timestamp = original.received_timestamp();
    assert!(!original.is_published());
    drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        fixture.transport.output_frame(),
        &mut TriggerQuantization::Grid { step_64ths: 16 },
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    assert_eq!(original.target_frame(), Some(24_000));
    assert_eq!(fixture.starts(0), 0);
    let window = fixture
        .control
        .engine
        .prepare_captured_input_trigger(&trigger)
        .unwrap();
    assert_ne!(window.publication_status(), "accepted");
    assert!(
        fixture
            .render_to(30_000)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_ne!(window.publication_status(), "accepted");
    fixture.drain();
    assert_eq!(
        window.publication_status(),
        "accepted",
        "error={:?}",
        window.error()
    );
    assert!(
        fixture
            .control
            .engine
            .trigger_prepared_input_pad(&trigger, &window)
            .unwrap()
    );
    fixture.drain();
    assert!(original.accepted());
    assert_eq!(original.target_frame(), Some(24_000));
    assert_eq!(original.original_frame(), Some(24_000));
    assert_eq!(original.execution_frame(), Some(38_192));
    wait_ready(&mut fixture, &original);
    let pcm = initial_pcm(&mut fixture, &original);
    assert_eq!(original.received_timestamp(), timestamp);
    assert_native_first_attack(&fixture, &original, pitch);
    assert_tone(&pcm, 0, 7, 1.0, true);
    assert_releases_create_no_attack(&mut fixture);
}
