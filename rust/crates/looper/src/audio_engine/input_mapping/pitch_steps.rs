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
        capture_event(runtime, event(runtime), key, base, extra)
    }

    fn capture_event(
        runtime: &InputRuntime,
        event: InputRuntimeEvent,
        key: &str,
        base: i32,
        extra: i32,
    ) -> CapturedPitchAction {
        assert_eq!(event.action_key.as_deref(), Some(key));
        assert!(event.direct && !event.dispatched);
        assert_eq!(event.pitch_rejection, None);
        let action = event.captured_pitch.unwrap();
        assert_eq!(
            action.render().pitch,
            PitchIntent::new(base, extra).unwrap()
        );
        assert_eq!(action.received_timestamp(), event.received_at_ns);
        assert_eq!(
            Some(action.content_key()),
            runtime.ownership.slot_snapshot(0).unwrap().content_key()
        );
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

    fn select_nrpn(runtime: &InputRuntime, channel: u8, parameter: u16) {
        let status = 0xB0 | (channel - 1);
        assert!(!runtime.inject_midi_message(&[status, 99, (parameter / 128) as u8]));
        assert!(!runtime.inject_midi_message(&[status, 98, (parameter % 128) as u8]));
    }

    fn data_entry(runtime: &InputRuntime, channel: u8, value: u8) {
        assert!(runtime.inject_midi_message(&[0xB0 | (channel - 1), 6, value]));
    }

    fn optional_low_byte(runtime: &InputRuntime, channel: u8, value: u8) {
        assert!(!runtime.inject_midi_message(&[0xB0 | (channel - 1), 38, value]));
    }

    fn capture_input(
        runtime: &InputRuntime,
        binding: &str,
        key: &str,
        value: u8,
        pitch: (i32, i32),
    ) -> CapturedPitchAction {
        let event = event(runtime);
        assert_eq!(event.binding_key, binding);
        assert_eq!(event.value, value);
        assert!(event.received_at_ns > 0);
        capture_event(runtime, event, key, pitch.0, pitch.1)
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
    fn routed_nrpn_data_entry_keeps_parameter_channel_and_button_owners_independent() {
        let key = "pitch.extra.step:1:set_retrigger";
        let runtime = runtime("midi:nrpn:1:130", key);
        let mut saved = mappings("midi:nrpn:1:130", key);
        saved.extend([
            ("midi:nrpn:1:131".into(), key.into()),
            ("midi:nrpn:2:130".into(), key.into()),
        ]);
        runtime.replace_mappings(saved.clone());

        let mut previous = None;
        for (channel, parameter, binding, extra) in [
            (1, 130, "midi:nrpn:1:130", 1),
            (1, 131, "midi:nrpn:1:131", 2),
            (2, 130, "midi:nrpn:2:130", 3),
        ] {
            select_nrpn(&runtime, channel, parameter);
            data_entry(&runtime, channel, 127);
            optional_low_byte(&runtime, channel, 0);
            data_entry(&runtime, channel, 127);
            let action = capture_input(&runtime, binding, key, 127, (0, extra));
            if let Some(sequence) = previous {
                assert!(sequence < action.sequence());
            }
            previous = Some(action.sequence());
            assert_eq!(action.variant(), PitchActionVariant::SetRetrigger);
        }
        barrier(&runtime, 3);

        // An unchanged saved mapping keeps its own held state. Releasing 131
        // must not release either 130 owner, including the other channel.
        runtime.replace_mappings(saved.clone());
        select_nrpn(&runtime, 1, 131);
        data_entry(&runtime, 1, 0);
        optional_low_byte(&runtime, 1, 127);
        select_nrpn(&runtime, 1, 130);
        data_entry(&runtime, 1, 127);
        data_entry(&runtime, 2, 127);
        barrier(&runtime, 3);
        select_nrpn(&runtime, 1, 131);
        data_entry(&runtime, 1, 127);
        capture_input(&runtime, "midi:nrpn:1:131", key, 127, (0, 4));
        barrier(&runtime, 4);

        runtime.replace_mappings(mappings("midi:nrpn:1:130", key));
        runtime.replace_mappings(saved);
        data_entry(&runtime, 1, 127);
        capture_input(&runtime, "midi:nrpn:1:131", key, 127, (0, 5));
        barrier(&runtime, 5);
    }

    #[test]
    fn routed_nrpn_data_entry_piano_and_original_keep_variants_and_only_high_edges() {
        for (variant, expected_variant) in [
            ("set", PitchActionVariant::Set),
            ("set_retrigger", PitchActionVariant::SetRetrigger),
        ] {
            for target in ["pitch.note:12", "pitch.original"] {
                let key = format!("{target}:{variant}");
                let runtime = runtime("midi:nrpn:1:130", &key);
                {
                    let mut state = runtime.runtime_state.lock().unwrap();
                    state.pads[0].pitch.intent = PitchIntent::new(12, 1).unwrap();
                    state.pads[0].pitch.correction_root = Some(7);
                    state.pads[0].pitch.correction_mode = Some(PitchKeyMode::Minor);
                }
                select_nrpn(&runtime, 1, 130);
                data_entry(&runtime, 1, 127);
                optional_low_byte(&runtime, 1, 0);
                data_entry(&runtime, 1, 127);
                data_entry(&runtime, 1, 0);
                optional_low_byte(&runtime, 1, 127);
                let original = target == "pitch.original";
                let pitch = if original { (0, 0) } else { (12, 1) };
                let first = capture_input(&runtime, "midi:nrpn:1:130", &key, 127, pitch);
                assert_eq!(first.variant(), expected_variant);
                assert_eq!(first.resets_key_correction(), original);
                barrier(&runtime, pitch.1);

                // Neither the optional low byte nor the Note barrier clears
                // selection or substitutes a second high edge.
                data_entry(&runtime, 1, 127);
                let second = capture_input(&runtime, "midi:nrpn:1:130", &key, 127, pitch);
                assert!(first.sequence() < second.sequence());
                assert_eq!(first.content_key(), second.content_key());
                assert_eq!(second.variant(), expected_variant);
                assert_eq!(second.resets_key_correction(), original);
                barrier(&runtime, pitch.1);
            }
        }
    }

    #[test]
    fn routed_nrpn_data_entry_uses_all_four_existing_modes_and_learned_step_signs() {
        type StepCase = (&'static str, &'static [u8], &'static [(u8, i32)]);
        let cases: [StepCase; 4] = [
            ("step", &[127, 127, 0, 0, 127], &[(127, 1), (127, 2)]),
            (
                "absolute",
                &[127, 127, 0, 127, 126, 126, 0],
                &[(0, 1), (127, 0), (126, -1), (0, 0)],
            ),
            ("twos_complement", &[1, 127, 0, 64], &[(1, 1), (127, 0)]),
            ("offset_binary", &[65, 63, 0, 64], &[(65, 1), (63, 0)]),
        ];
        for (mode, values, expected) in cases {
            for step in [-1, 1] {
                for (variant, expected_variant) in [
                    ("set", PitchActionVariant::Set),
                    ("set_retrigger", PitchActionVariant::SetRetrigger),
                ] {
                    let key = if mode == "step" {
                        format!("pitch.extra.step:{step}:{variant}")
                    } else {
                        format!("pitch.extra.encoder:{step}:{mode}:{variant}")
                    };
                    let runtime = runtime("midi:nrpn:1:130", &key);
                    select_nrpn(&runtime, 1, 130);
                    for &value in values {
                        data_entry(&runtime, 1, value);
                        optional_low_byte(&runtime, 1, 127 - value);
                    }
                    let mut previous = None;
                    for &(value, direction) in expected {
                        let action = capture_input(
                            &runtime,
                            "midi:nrpn:1:130",
                            &key,
                            value,
                            (0, direction * step),
                        );
                        if let Some(sequence) = previous {
                            assert!(sequence < action.sequence());
                        }
                        previous = Some(action.sequence());
                        assert_eq!(action.variant(), expected_variant);
                    }
                    barrier(&runtime, expected.last().unwrap().1 * step);
                }
            }
        }
    }

    #[test]
    fn routed_nrpn_data_entry_preserves_opposite_learned_octave_without_folding() {
        for step in [-12, 12] {
            let key = format!("pitch.note.encoder:{step}:offset_binary:set");
            let runtime = runtime("midi:nrpn:1:131", &key);
            select_nrpn(&runtime, 1, 131);
            for value in [65, 63, 0, 64] {
                data_entry(&runtime, 1, value);
                optional_low_byte(&runtime, 1, 127);
            }
            let first = capture_input(&runtime, "midi:nrpn:1:131", &key, 65, (step, 0));
            let second = capture_input(&runtime, "midi:nrpn:1:131", &key, 63, (0, 0));
            assert!(first.sequence() < second.sequence());
            assert_eq!(first.variant(), PitchActionVariant::Set);
            assert_eq!(second.variant(), PitchActionVariant::Set);
            barrier(&runtime, 0);
        }
    }

    #[test]
    fn routed_data_entry_without_own_selection_keeps_plain_cc_mapping_identity() {
        let key = "pitch.extra.step:1:set";
        let runtime = runtime("midi:cc:1:6", key);
        let mut saved = mappings("midi:cc:1:6", key);
        saved.extend([
            ("midi:cc:1:38".into(), key.into()),
            ("midi:cc:2:6".into(), key.into()),
            ("midi:cc:2:38".into(), key.into()),
        ]);
        runtime.replace_mappings(saved);
        data_entry(&runtime, 1, 127);
        capture_input(&runtime, "midi:cc:1:6", key, 127, (0, 1));
        assert!(runtime.inject_midi_message(&[0xB0, 38, 127]));
        capture_input(&runtime, "midi:cc:1:38", key, 127, (0, 2));
        select_nrpn(&runtime, 1, 130);
        data_entry(&runtime, 2, 127);
        capture_input(&runtime, "midi:cc:2:6", key, 127, (0, 3));
        assert!(runtime.inject_midi_message(&[0xB1, 38, 127]));
        capture_input(&runtime, "midi:cc:2:38", key, 127, (0, 4));
        barrier(&runtime, 4);
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
