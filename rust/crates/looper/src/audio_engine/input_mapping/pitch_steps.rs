//! Mapping-owned MIDI step decoding on the input dispatcher, never on realtime.

use super::{InputMapping, MidiBindingKind, NormalizedMidiEvent};
use crate::audio_engine::pitch_actions::{
    PitchActionReject, PitchActionSpec, PitchActionTarget, PitchEncoderMode,
};

#[derive(Debug, Default)]
pub(super) struct PitchStepState {
    button_high: bool,
    previous_absolute: Option<u8>,
}

impl PitchStepState {
    fn button_attack(&mut self, value: u8) -> bool {
        let high = value >= 64;
        let attack = high && !self.button_high;
        self.button_high = high;
        attack
    }

    fn encoder_direction(&mut self, mode: PitchEncoderMode, value: u8) -> i32 {
        match mode {
            PitchEncoderMode::Absolute => {
                let previous = self.previous_absolute.replace(value);
                previous.map_or(0, |previous| {
                    let delta = (i32::from(value) - i32::from(previous) + 64).rem_euclid(128) - 64;
                    delta.signum()
                })
            }
            PitchEncoderMode::TwosComplement => match value {
                1..=63 => 1,
                65..=127 => -1,
                _ => 0,
            },
            PitchEncoderMode::OffsetBinary => match value {
                65..=127 => 1,
                1..=63 => -1,
                _ => 0,
            },
        }
    }
}

pub(super) fn resolve(
    mapping: &InputMapping,
    event: &NormalizedMidiEvent,
    mut spec: PitchActionSpec,
) -> Result<Option<PitchActionSpec>, PitchActionReject> {
    let (piano, step, mode) = match spec.target {
        PitchActionTarget::Note(_) | PitchActionTarget::Original => {
            if event.binding.kind == MidiBindingKind::Note {
                return Ok(Some(spec));
            }
            if event.value > 127 {
                return Err(PitchActionReject::InvalidAction);
            }
            let attack = mapping
                .pitch_step_state
                .lock()
                .map_err(|_| PitchActionReject::InvalidAction)?
                .button_attack(event.value);
            return Ok(attack.then_some(spec));
        }
        PitchActionTarget::NoteStep(step) => (true, step, None),
        PitchActionTarget::ExtraStep(step) => (false, step, None),
        PitchActionTarget::NoteEncoder(step, mode) => (true, step, Some(mode)),
        PitchActionTarget::ExtraEncoder(step, mode) => (false, step, Some(mode)),
        _ => return Ok(Some(spec)),
    };
    if event.binding.kind == MidiBindingKind::Note {
        return if mode.is_none() {
            Ok(Some(spec))
        } else {
            Err(PitchActionReject::InvalidAction)
        };
    }
    if event.value > 127 {
        return Err(PitchActionReject::InvalidAction);
    }
    let mut state = mapping
        .pitch_step_state
        .lock()
        .map_err(|_| PitchActionReject::InvalidAction)?;
    let direction = if let Some(mode) = mode {
        state.encoder_direction(mode, event.value)
    } else if state.button_attack(event.value) {
        1
    } else {
        0
    };
    if direction == 0 {
        return Ok(None);
    }
    let step = step * direction;
    spec.target = if piano {
        PitchActionTarget::NoteStep(step)
    } else {
        PitchActionTarget::ExtraStep(step)
    };
    Ok(Some(spec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::input_mapping::{
        InputRuntime, InputRuntimeEvent, PitchPadProjection, RuntimePadState,
    };
    use crate::audio_engine::input_runtime_binding::{
        InputRuntimeOwnership, InputRuntimePadBinding,
    };
    use crate::audio_engine::pitch_actions::{
        CapturedPitchAction, PitchActionVariant, PitchKeyMode,
    };
    use crate::audio_engine::pitch_intent::PitchIntent;
    use crate::audio_engine::timing::{InputClock, OutputClockSnapshot, SharedOutputClock};
    use crate::messages::ControlMessage;
    use rtrb::RingBuffer;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const BARRIER_KEY: &str = "pitch.note:0:set";

    // Real normalization, mapping lookup, dispatcher and content-bound capture.
    // This fixture opens neither a MIDI port nor an audio device and makes no
    // productive DSP, preparation or Human listening acceptance claim.
    fn runtime(binding: &str, action: &str) -> InputRuntime {
        let (producer, _consumer) = RingBuffer::<ControlMessage>::new(8);
        let ownership = Arc::new(InputRuntimeOwnership::default());
        let input_clock = InputClock::new();
        let runtime = InputRuntime::new_with_ownership(
            Arc::new(Mutex::new(producer)),
            input_clock,
            ownership.clone(),
        );
        let clock = Arc::new(SharedOutputClock::new());
        let now = input_clock.capture_ns();
        clock.publish(OutputClockSnapshot {
            valid: true,
            observed_at_ns: now,
            audible_at_ns: now,
            output_frame: 24_000,
            sample_rate_hz: 48_000,
            master_period_seconds: Some(0.5),
            downbeat_frame: 0,
            master_beat: 1.0,
            freshness_ns: 2_000_000_000,
        });
        let native = InputRuntimePadBinding::for_test(0, ownership);
        {
            let mut state = runtime.runtime_state.lock().unwrap();
            state.pitch_clock = Some(clock);
            state.pads[0] = RuntimePadState {
                loaded: true,
                binding: Some(native.binding),
                acknowledgements: Some(native.acknowledgements.clone()),
                loop_start_s: 0.25,
                loop_end_s: Some(1.0),
                pitch: PitchPadProjection {
                    source_root: Some(0),
                    source_mode: Some(PitchKeyMode::Major),
                    quantize_step_64ths: Some(4),
                    ..PitchPadProjection::default()
                },
                ..RuntimePadState::default()
            };
        }
        runtime.set_selected_pad(0).unwrap();
        runtime.replace_mappings(mappings(binding, action));
        runtime.set_enabled(true);
        runtime
    }

    fn mappings(binding: &str, action: &str) -> Vec<(String, String)> {
        vec![
            (binding.into(), action.into()),
            ("midi:note:1:62".into(), BARRIER_KEY.into()),
        ]
    }

    fn event(runtime: &InputRuntime) -> InputRuntimeEvent {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(event) = runtime.poll_event() {
                return event;
            }
            assert!(
                Instant::now() < deadline,
                "routed pitch-step event did not arrive"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn capture(runtime: &InputRuntime, key: &str, base: i32, extra: i32) -> CapturedPitchAction {
        let event = event(runtime);
        assert_eq!(event.action_key.as_deref(), Some(key));
        assert!(event.direct && !event.dispatched);
        assert_eq!(event.pitch_rejection, None);
        let action = event.captured_pitch.unwrap();
        assert_eq!(
            action.render().pitch,
            PitchIntent::new(base, extra).unwrap()
        );
        assert_eq!(action.received_timestamp(), event.received_at_ns);
        assert_eq!(action.control_guard(None).unwrap().id, 0);
        assert_eq!(action.loop_region(), (0.25, Some(1.0)));
        action
    }

    fn cc(runtime: &InputRuntime, value: u8) {
        assert!(runtime.inject_midi_message(&[0xB0, 71, value]));
    }

    fn barrier(runtime: &InputRuntime, extra: i32) {
        assert!(runtime.inject_midi_message(&[0x90, 62, 100]));
        capture(runtime, BARRIER_KEY, 0, extra);
        assert!(runtime.poll_event().is_none());
    }

    #[test]
    fn routed_step_button_captures_high_edges_without_release_or_held_repeats() {
        let key = "pitch.extra.step:1:set_retrigger";
        let runtime = runtime("midi:cc:1:71", key);
        for value in [127, 127, 0, 0, 127] {
            cc(&runtime, value);
        }
        let first = capture(&runtime, key, 0, 1);
        let second = capture(&runtime, key, 0, 2);
        assert!(first.sequence() < second.sequence());
        assert_eq!(first.variant(), PitchActionVariant::SetRetrigger);
        assert_eq!(second.variant(), PitchActionVariant::SetRetrigger);
        barrier(&runtime, 2);
    }

    #[test]
    fn routed_new_piano_and_original_cc_buttons_capture_only_fresh_high_edges() {
        for key in [
            "pitch.note:12:set_retrigger",
            "pitch.original:set_retrigger",
        ] {
            let runtime = runtime("midi:cc:1:71", key);
            runtime.runtime_state.lock().unwrap().pads[0].pitch.intent =
                PitchIntent::new(12, 1).unwrap();
            for value in [127, 127, 0] {
                cc(&runtime, value);
            }
            let original = key.starts_with("pitch.original:");
            let (base, extra) = if original { (0, 0) } else { (12, 1) };
            let first = capture(&runtime, key, base, extra);
            assert_eq!(first.resets_key_correction(), original);
            assert_eq!(first.variant(), PitchActionVariant::SetRetrigger);
            // The following Note barrier proves the preceding held value and
            // release were processed without producing another pitch event.
            barrier(&runtime, extra);
            cc(&runtime, 127);
            let second = capture(&runtime, key, base, extra);
            assert!(first.sequence() < second.sequence());
            assert_eq!(second.resets_key_correction(), original);
            assert_eq!(second.variant(), PitchActionVariant::SetRetrigger);
        }
    }

    #[test]
    fn routed_fixed_piano_note_on_messages_remain_repeated_genuine_attacks() {
        let key = "pitch.note:12:set_retrigger";
        let runtime = runtime("midi:note:1:60", key);
        assert!(runtime.inject_midi_message(&[0x90, 60, 127]));
        assert!(runtime.inject_midi_message(&[0x90, 60, 127]));
        let first = capture(&runtime, key, 12, 0);
        let second = capture(&runtime, key, 12, 0);
        assert!(first.sequence() < second.sequence());
        assert_eq!(first.variant(), PitchActionVariant::SetRetrigger);
        assert_eq!(second.variant(), PitchActionVariant::SetRetrigger);
        barrier(&runtime, 0);
    }

    #[test]
    fn routed_relative_encoder_modes_reverse_once_and_ignore_neutral_messages() {
        for (mode, values) in [
            ("twos_complement", [1, 127, 0, 64]),
            ("offset_binary", [65, 63, 0, 64]),
        ] {
            for step in [-1, 1] {
                let key = format!("pitch.extra.encoder:{step}:{mode}:set_retrigger");
                let runtime = runtime("midi:cc:1:71", &key);
                for value in values {
                    cc(&runtime, value);
                }
                let first = capture(&runtime, &key, 0, step);
                let second = capture(&runtime, &key, 0, 0);
                assert!(first.sequence() < second.sequence());
                assert_eq!(first.variant(), PitchActionVariant::SetRetrigger);
                assert_eq!(second.variant(), PitchActionVariant::SetRetrigger);
                barrier(&runtime, 0);
            }
        }
    }

    #[test]
    fn routed_absolute_encoder_initializes_ignores_equal_and_wraps_in_both_directions() {
        let key = "pitch.extra.encoder:1:absolute:set";
        let runtime = runtime("midi:cc:1:71", key);
        for value in [127, 127, 0, 127, 126, 126, 0] {
            cc(&runtime, value);
        }
        for extra in [1, 0, -1, 0] {
            capture(&runtime, key, 0, extra);
        }
        barrier(&runtime, 0);
    }

    #[test]
    fn routed_negative_learned_octave_inverts_encoder_direction_without_folding() {
        let key = "pitch.note.encoder:-12:offset_binary:set";
        let runtime = runtime("midi:cc:1:71", key);
        cc(&runtime, 65);
        cc(&runtime, 63);
        capture(&runtime, key, -12, 0);
        capture(&runtime, key, 0, 0);
        barrier(&runtime, 0);
    }

    #[test]
    fn routed_note_steps_remain_repeated_attacks_and_legacy_fixed_cc_keeps_release_semantics() {
        let note_key = "pitch.extra.step:1:set";
        let note_runtime = runtime("midi:note:1:60", note_key);
        assert!(note_runtime.inject_midi_message(&[0x90, 60, 1]));
        assert!(note_runtime.inject_midi_message(&[0x90, 60, 127]));
        assert!(!note_runtime.inject_midi_message(&[0x80, 60, 0]));
        assert!(!note_runtime.inject_midi_message(&[0x90, 60, 0]));
        capture(&note_runtime, note_key, 0, 1);
        capture(&note_runtime, note_key, 0, 2);
        barrier(&note_runtime, 2);

        let fixed_key = "pitch.extra:7:set";
        let fixed_runtime = runtime("midi:cc:1:71", fixed_key);
        for value in [127, 0, 0] {
            cc(&fixed_runtime, value);
        }
        for _ in 0..3 {
            capture(&fixed_runtime, fixed_key, 0, 7);
        }
        barrier(&fixed_runtime, 7);
    }

    #[test]
    fn routed_encoder_bound_refusal_preserves_request_and_opposite_direction_remains_usable() {
        let key = "pitch.extra.encoder:1:twos_complement:set";
        let runtime = runtime("midi:cc:1:71", key);
        runtime.runtime_state.lock().unwrap().pads[0].pitch.intent =
            PitchIntent::new(23, 1).unwrap();
        cc(&runtime, 1);
        let refusal = event(&runtime);
        assert_eq!(refusal.action_key.as_deref(), Some(key));
        assert_eq!(
            refusal.pitch_rejection,
            Some(PitchActionReject::UnsupportedPitch)
        );
        assert!(refusal.captured_pitch.is_none());
        assert_eq!(runtime.pitch_request_sequence(0).unwrap(), 0);
        assert_eq!(
            runtime.runtime_state.lock().unwrap().pads[0].pitch.intent,
            PitchIntent::new(23, 1).unwrap()
        );
        cc(&runtime, 127);
        capture(&runtime, key, 23, 0);
    }

    #[test]
    fn routed_button_state_follows_only_an_unchanged_mapping_owner() {
        let key = "pitch.extra.step:1:set";
        let runtime = runtime("midi:cc:1:71", key);
        cc(&runtime, 127);
        capture(&runtime, key, 0, 1);
        runtime.replace_mappings(mappings("midi:cc:1:71", key));
        cc(&runtime, 127);
        barrier(&runtime, 1);
        runtime.replace_mappings(Vec::new());
        runtime.replace_mappings(mappings("midi:cc:1:71", key));
        cc(&runtime, 127);
        capture(&runtime, key, 0, 2);
    }

    #[test]
    fn routed_absolute_history_is_preserved_for_same_mapping_and_reset_for_relearning() {
        let key = "pitch.extra.encoder:1:absolute:set";
        let runtime = runtime("midi:cc:1:71", key);
        cc(&runtime, 100);
        barrier(&runtime, 0);
        runtime.replace_mappings(mappings("midi:cc:1:71", key));
        cc(&runtime, 101);
        capture(&runtime, key, 0, 1);
        let inverted = "pitch.extra.encoder:-1:absolute:set";
        runtime.replace_mappings(mappings("midi:cc:1:71", inverted));
        cc(&runtime, 50);
        cc(&runtime, 51);
        capture(&runtime, inverted, 0, 0);
        barrier(&runtime, 0);
    }

    #[test]
    fn routed_nrpn_steps_use_the_same_declared_relative_decoder() {
        let key = "pitch.extra.encoder:1:offset_binary:set";
        let runtime = runtime("midi:nrpn:1:130", key);
        assert!(!runtime.inject_midi_message(&[0xB0, 99, 1]));
        assert!(!runtime.inject_midi_message(&[0xB0, 98, 2]));
        assert!(runtime.inject_midi_message(&[0xB0, 96, 0]));
        assert!(runtime.inject_midi_message(&[0xB0, 97, 0]));
        capture(&runtime, key, 0, 1);
        capture(&runtime, key, 0, 0);
    }

    #[test]
    fn routed_step_state_reinitializes_after_disabled_or_learn_input_gaps() {
        for learn in [false, true] {
            let key = "pitch.extra.step:1:set";
            let runtime = runtime("midi:cc:1:71", key);
            cc(&runtime, 127);
            capture(&runtime, key, 0, 1);
            if learn {
                runtime.set_learn_capture_active(true);
            } else {
                runtime.set_enabled(false);
            }
            cc(&runtime, 0);
            let ignored = event(&runtime);
            assert!(ignored.action_key.is_none());
            assert!(ignored.captured_pitch.is_none());
            if learn {
                runtime.set_learn_capture_active(false);
            } else {
                runtime.set_enabled(true);
            }
            cc(&runtime, 127);
            capture(&runtime, key, 0, 2);
        }
    }

    #[test]
    fn encoder_actions_never_fall_back_to_note_velocity_or_a_direct_capture() {
        let key = "pitch.extra.encoder:1:twos_complement:set";
        let runtime = runtime("midi:note:1:60", key);
        assert!(matches!(
            runtime.capture_selected_pitch(key, runtime.clock.capture_ns()),
            Err(PitchActionReject::InvalidAction)
        ));
        assert!(runtime.inject_midi_message(&[0x90, 60, 1]));
        let refused = event(&runtime);
        assert_eq!(refused.action_key.as_deref(), Some(key));
        assert_eq!(
            refused.pitch_rejection,
            Some(PitchActionReject::InvalidAction)
        );
        assert!(refused.captured_pitch.is_none());
        assert_eq!(runtime.pitch_request_sequence(0).unwrap(), 0);
    }
}
