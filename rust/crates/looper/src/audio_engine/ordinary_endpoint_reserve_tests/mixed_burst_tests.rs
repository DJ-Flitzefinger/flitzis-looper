//! Complementary renderer and per-event executor observations of one mixed-origin
//! opposing-tuple burst. The virtual clock waits for preparation; no device deadline,
//! acoustic acceptance, Loader cold-source path or wider timing matrix is claimed.

use super::*;
use crate::audio_engine::audio_stream::{ScheduledExecution, execute_scheduled_command};
use crate::audio_engine::input_runtime_binding::InputPadBinding;
use crate::audio_engine::scheduler::ScheduledCommand;
use std::sync::Arc;

const EXTRA_MINUS_SEVEN: &str = "pitch.extra:-7:set_retrigger";
const TARGET: u64 = 24_000;

fn capture_opposing_midi(fixture: &Fixture) -> CapturedPitchAction {
    fixture.refresh_clock();
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    assert!(runtime.inject_midi_message(&[0x91, 60, 127]));
    let event = routed_event(fixture);
    assert_eq!(event.binding_key, "midi:note:2:60");
    assert_eq!(event.action_key.as_deref(), Some(EXTRA_MINUS_SEVEN));
    assert!(event.direct && !event.dispatched && event.pitch_rejection.is_none());
    let action = event.captured_pitch.unwrap();
    assert_eq!(action.received_timestamp(), event.received_at_ns);
    assert!(!runtime.inject_midi_message(&[0x81, 60, 0]));
    assert!(!runtime.inject_midi_message(&[0x91, 60, 0]));
    assert!(runtime.poll_event().is_none());
    action
}

fn assert_original_source(
    fixture: &Fixture,
    action: &CapturedPitchAction,
    source: InputPadBinding,
) {
    let voice = fixture.voice(0);
    let sample = voice.sample.as_ref().unwrap();
    assert_eq!(sample.source_address(), source.source_address);
    assert_eq!(sample.source_sample_count(), source.sample_count);
    assert_eq!(sample.channels, source.channels);
    assert_eq!(sample.resident_binding(), source.resident);
    assert_eq!(voice.source_timing.accepted, source.accepted);
    assert_eq!(voice.stretch.adopted_request_id(), Some(action.sequence()));
    assert!(
        voice
            .stretch
            .owns_pitch_contract(action.render().pitch, action.render().key_lock)
    );
    let history = voice.stretch.productive_history().unwrap();
    assert_eq!(history.binding.source_address, source.source_address);
    assert_eq!(history.binding.sample_count, source.sample_count);
    assert_eq!(history.binding.channels, source.channels);
    assert_eq!(history.binding.sample_rate_hz, source.sample_rate_hz);
    assert_eq!(history.binding.resident, source.resident);
    assert_eq!(history.binding.accepted, source.accepted);
    assert_eq!(
        voice
            .stretch
            .native_tap_observation()
            .unwrap()
            .missing_reads,
        0
    );
}

fn mixed_burst_profile(observe_each_executor: bool, oracle: &[f32], oracle_fifo: (usize, usize)) {
    let mut hit = fixture();
    let mut background = fixture();
    set_pitch(&mut hit, 7);
    set_pitch(&mut background, 7);
    let previous = start_ordinary_and_progress(&mut hit);
    start_ordinary_and_progress(&mut background);
    runtime_rows(&hit, Some(16), LOOP_START, LOOP_END);
    hit.control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .replace_mappings(vec![("midi:note:2:60".into(), EXTRA_MINUS_SEVEN.into())]);
    let cached = select_ready(&mut hit, Origin::PublicGui);
    let old_native = hit.voice(0).stretch.native_state_address();
    let old_generation = hit.voice(0).generation;
    let old_playback = hit.voice(0).source_playback;
    let old_fifo = hit.voice(0).stretch.pending_fifo_frames();
    assert_ne!(cached, old_native);

    // All three original captures precede action publication and worker ACK. Only
    // requested intent changes here: the bank and live voice still own actual +7.
    let actions = [
        capture_pitch(&hit, Origin::PublicGui),
        capture_opposing_midi(&hit),
        capture_pitch(&hit, Origin::PublicGui),
    ];
    let source = actions[0].control_guard(None).unwrap().binding;
    let identities = actions
        .iter()
        .map(|action| {
            (
                action.content_key(),
                action.sequence(),
                action.received_timestamp(),
                action.render(),
                action.original_due_frame(),
                action.loop_region(),
                action.epoch(),
            )
        })
        .collect::<Vec<_>>();
    for (action, extra) in actions.iter().zip([7, -7, 7]) {
        assert_eq!(action.render().pitch, PitchIntent::new(0, extra).unwrap());
        assert!(action.render().key_lock);
        assert!(!action.sync() && !action.exclusive());
        assert_eq!(action.variant(), PitchActionVariant::SetRetrigger);
        assert_eq!(action.original_due_frame(), TARGET);
        assert_eq!(action.content_key(), actions[0].content_key());
        assert_eq!(action.control_guard(None).unwrap().binding, source);
        assert!(!action.accepted() && !action.effective());
    }
    for pair in actions.windows(2) {
        assert!(pair[0].sequence() < pair[1].sequence());
        assert!(pair[0].received_timestamp() <= pair[1].received_timestamp());
        assert!(!Arc::ptr_eq(
            pair[0].preparation_epoch(),
            pair[1].preparation_epoch()
        ));
    }
    assert!(!Arc::ptr_eq(
        actions[0].preparation_epoch(),
        actions[2].preparation_epoch()
    ));
    assert_eq!(
        hit.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(0, 7).unwrap())
    );
    assert_eq!(hit.voice(0).stretch.native_state_address(), old_native);
    assert_eq!(hit.voice(0).stretch.pending_fifo_frames(), old_fifo);
    assert!(hit.voice(0).source_playback.matches_exact(&old_playback));
    assert!(
        hit.mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .selected_endpoint_ready()
    );

    // First consumes the one cache bundle. Reverse publication of captures 2/3
    // makes original sequence ordering observable despite their different tuples.
    for index in [0, 2, 1] {
        publish_pitch(&mut hit, &actions[index]);
        assert_eq!(actions[index].execution_target(), TARGET);
        assert!(
            !hit.mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .selected_endpoint_ready()
        );
    }
    assert_eq!(hit.scheduler.len(), 3);
    assert_eq!(
        hit.mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .unused_action_capacity()
            .0,
        61
    );
    hit.wait_worker(&actions.iter().collect::<Vec<_>>());
    assert_eq!(hit.transport.output_frame(), 17_000);
    assert_eq!(hit.render_to(TARGET), background.render_to(TARGET));
    assert!(
        actions
            .iter()
            .all(|action| action.accepted() && !action.effective())
    );
    assert_eq!(hit.starts(0), 1);
    assert_eq!(hit.voice(0).generation, old_generation);
    assert_eq!(hit.voice(0).stretch.native_state_address(), old_native);
    assert_eq!(
        hit.voice(0).stretch.adopted_request_id(),
        Some(previous.sequence())
    );
    assert!(
        hit.voice(0)
            .source_playback
            .matches_exact(&background.voice(0).source_playback)
    );
    assert_eq!(
        hit.voice(0).stretch.pending_fifo_frames(),
        background.voice(0).stretch.pending_fifo_frames()
    );

    if observe_each_executor {
        // Complementary observation only: pop the real scheduler and use the same
        // production executor at frozen T, without rendering between equal-T events.
        // The other profile below leaves all three to the actual render(1) drain.
        let mut natives = Vec::new();
        for (index, action) in actions.iter().enumerate() {
            let event = hit.scheduler.pop_due_at_callback_start(TARGET).unwrap();
            assert_eq!(event.target_frame, TARGET);
            assert_eq!(event.execution_frame, TARGET);
            assert!(!event.was_late);
            assert!(matches!(
                &event.command,
                ScheduledCommand::PitchAction { action_sequence, .. }
                    if *action_sequence == action.sequence()
            ));
            assert!(matches!(
                execute_scheduled_command(
                    &mut hit.mixer,
                    &mut hit.transport,
                    TARGET,
                    event.command,
                    &mut hit.feedback,
                    &mut hit.retirement,
                ),
                ScheduledExecution::Complete
            ));
            assert!(action.effective(), "reason={:?}", action.reason());
            assert!(actions[..=index].iter().all(CapturedPitchAction::effective));
            assert!(actions[index + 1..].iter().all(|later| !later.effective()));
            assert_eq!(hit.starts(0), index + 2);
            assert_eq!(hit.voice(0).generation, old_generation + index as u64 + 1);
            assert_eq!(hit.mixer.pad_pitch_intent(0), Some(action.render().pitch));
            assert_original_source(&hit, action, source);
            let native = hit.voice(0).stretch.native_state_address();
            assert_ne!(native, 0);
            assert_ne!(native, old_native);
            assert!(!natives.contains(&native));
            if index == 0 {
                assert_eq!(native, cached);
            } else {
                assert_ne!(native, cached);
            }
            natives.push(native);
            assert_eq!(hit.voice(0).source_playback.position().frame, LOOP_START);
            assert_eq!(hit.voice(0).source_playback.position().fraction, 0.0);
            assert_eq!(hit.transport.output_frame(), TARGET);
        }
        assert!(hit.scheduler.is_empty());
    }

    // In the main profile this production render drains the entire mixed burst.
    // Both profiles compare every initial sample with the independently raw-bound
    // on-time ordinary reference, including its explicit-silence cold beginning.
    let mut pcm = hit.render(1);
    assert!(actions.iter().all(CapturedPitchAction::effective));
    assert!(hit.scheduler.is_empty());
    assert_eq!(hit.starts(0), 4);
    assert_eq!(hit.voice(0).generation, old_generation + 3);
    let final_native = hit.voice(0).stretch.native_state_address();
    assert_ne!(final_native, cached);
    assert_ne!(final_native, old_native);
    assert_native_attack(&hit, &actions[2], final_native, 1);
    assert_original_source(&hit, &actions[2], source);
    pcm.extend(hit.pcm(47_999));
    assert_native_attack(&hit, &actions[2], final_native, pcm.len());
    assert_eq!(hit.voice(0).stretch.pending_fifo_frames(), oracle_fifo);
    assert_initial_pcm(&pcm, oracle);
    for (action, identity) in actions.iter().zip(identities) {
        assert_eq!(
            (
                action.content_key(),
                action.sequence(),
                action.received_timestamp(),
                action.render(),
                action.original_due_frame(),
                action.loop_region(),
                action.epoch(),
            ),
            identity
        );
        assert_eq!(action.execution_target(), TARGET);
    }
}

#[test]
fn accepted_mixed_gui_midi_opposing_burst_keeps_originals_after_reserve_consumption() {
    let (oracle, oracle_fifo) = {
        let mut reference = fixture();
        set_pitch(&mut reference, 7);
        assert!(
            reference
                .render_to(TARGET)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        let expected = ordinary_ready(&mut reference);
        let pcm = complete_pcm(&mut reference, TARGET);
        assert_eq!(expected.state(), "applied");
        let raw = physical_birth_tone_reference(48_000, SOURCE_HZ, 0, 7, 1.0, true);
        assert_initial_pcm(&pcm, &raw);
        // H=4096 is block aligned and 48000=93*512+384. The existing
        // adapter therefore retains 384 input frames and 511-384 output frames.
        assert_eq!(reference.voice(0).stretch.pending_fifo_frames(), (384, 127));
        (pcm, reference.voice(0).stretch.pending_fifo_frames())
    };
    for observe_each_executor in [false, true] {
        mixed_burst_profile(observe_each_executor, &oracle, oracle_fifo);
    }
}
