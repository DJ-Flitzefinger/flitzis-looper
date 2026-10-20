//! Deterministic production lookup/admission split plus actual dispatcher receipts.
//!
//! The existing split calls the real StopPad dispatch branch after mapping lookup;
//! it does not run the dispatcher loop or fabricate its event receipt. The full-run
//! tests below separately exercise the actual bounded input/event channels.

use super::*;
use crate::audio_engine::audio_stream::process_control_message;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_sources;
use crate::audio_engine::pitch_runtime_tests::{Fixture, publish_tone};
use crate::messages::TriggerQuantization;

const BINDING: &str = "midi:note:1:60";
const STOP_A: &str = "pad.stop:0";
const STOP_B: &str = "pad.stop:1";

#[derive(Clone, Copy, Debug)]
enum Revocation {
    MappingRetired,
    Disabled,
    Learn,
}

fn fixture() -> (LaunchFixture, InputRuntime) {
    let control = LaunchFixture::new(8);
    let runtime = InputRuntime::new_with_ownership(
        control.producer.clone(),
        control.engine.input_clock,
        control.engine.input_runtime_ownership.clone(),
    );
    runtime.replace_mappings(vec![(BINDING.into(), STOP_A.into())]);
    runtime.set_enabled(true);
    (control, runtime)
}

fn revoke(runtime: &InputRuntime, change: Revocation) {
    match change {
        Revocation::MappingRetired => {
            runtime.replace_mappings(vec![(BINDING.into(), STOP_B.into())])
        }
        Revocation::Disabled => runtime.set_enabled(false),
        Revocation::Learn => runtime.set_learn_capture_active(true),
    }
}

fn restore(runtime: &InputRuntime, change: Revocation) {
    match change {
        // A replacement mapping is a distinct owner/action and must stay that way.
        Revocation::MappingRetired => {}
        Revocation::Disabled => runtime.set_enabled(true),
        Revocation::Learn => runtime.set_learn_capture_active(false),
    }
}

fn event(runtime: &InputRuntime) -> InputRuntimeEvent {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(event) = runtime.poll_event() {
            return event;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "real MIDI STOP receipt timed out"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn revoked_before_admission(change: Revocation) {
    let (mut control, runtime) = fixture();
    assert!(control.trigger(|| {}));
    let ownership = control.engine.input_runtime_ownership.clone();
    let original = ownership.slot_snapshot(0).unwrap();
    let revision = ownership.launch_revision(0);
    assert_eq!(ownership.admitted_launch_ids(), vec![0]);
    let mapping_owner = runtime.mappings.lock().unwrap()[0].owner.clone();
    let received_at_ns = control.engine.capture_input_timestamp_ns();
    let dispatch = runtime
        .capture_mapping_dispatch_for_test(BINDING, received_at_ns)
        .unwrap();
    revoke(&runtime, change);
    assert_eq!(
        mapping_owner.load(Ordering::Acquire),
        !matches!(change, Revocation::MappingRetired)
    );

    assert!(
        !dispatch(),
        "STOP captured before {change:?} must be refused at Native admission"
    );
    assert_eq!(ownership.launch_revision(0), revision);
    assert_eq!(ownership.admitted_launch_ids(), vec![0]);
    let ControlMessage::TriggerInputPad {
        binding,
        launch_revision,
        received_at_ns: attack_time,
        ..
    } = control.consumer.pop().unwrap()
    else {
        panic!("revoked STOP replaced the original pending attack");
    };
    assert_eq!(
        binding.placement.unwrap().content_key(),
        original.content_key()
    );
    assert_eq!(launch_revision, revision);
    assert_eq!(attack_time, 42);
    assert!(ownership.binding_launch_current(0, binding, launch_revision));
    assert!(control.consumer.is_empty());
    assert!(
        runtime.poll_event().is_none(),
        "the split does not emit a fabricated receipt"
    );

    restore(&runtime, change);
    // A genuine subsequent dispatcher turn proves that restore did not replay STOP.
    assert!(runtime.inject_midi_message(&[0x90, 61, 99]));
    let barrier = event(&runtime);
    assert_eq!(barrier.binding_key, "midi:note:1:61");
    assert!(barrier.action_key.is_none() && !barrier.dispatched);
    assert!(control.consumer.is_empty());
    assert_eq!(ownership.launch_revision(0), revision);
    assert_eq!(ownership.admitted_launch_ids(), vec![0]);

    // Only a fresh gesture uses the surviving current mapping. A retired A mapping
    // must neither replay A nor substitute B into the original captured operation.
    assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
    let receipt = event(&runtime);
    let id = usize::from(matches!(change, Revocation::MappingRetired));
    assert_eq!(receipt.binding_key, BINDING);
    assert_eq!(
        receipt.action_key.as_deref(),
        Some(if id == 0 { STOP_A } else { STOP_B })
    );
    assert!(receipt.dispatched && receipt.direct);
    assert!(receipt.received_at_ns >= received_at_ns);
    let ControlMessage::StopSampleForPlacement { placement } = control.consumer.pop().unwrap()
    else {
        panic!("fresh MIDI STOP must retain its actual content placement");
    };
    assert_eq!(
        placement.content_key(),
        ownership.slot_snapshot(id).unwrap().content_key()
    );
    assert!(control.consumer.is_empty());
    assert_eq!(ownership.launch_revision(0), revision + u64::from(id == 0));
    assert_eq!(
        ownership.admitted_launch_ids(),
        if id == 0 { vec![] } else { vec![0] }
    );
}

#[test]
fn retired_mapping_cannot_admit_its_captured_stop_or_cancel_pending_start() {
    revoked_before_admission(Revocation::MappingRetired);
}

#[test]
fn disabled_mapping_cannot_admit_its_captured_stop_or_cancel_pending_start() {
    revoked_before_admission(Revocation::Disabled);
}

#[test]
fn learn_transition_cannot_admit_its_captured_stop_or_cancel_pending_start() {
    revoked_before_admission(Revocation::Learn);
}

fn revoked_and_restored_before_admission(learn: bool) {
    let (mut control, runtime) = fixture();
    assert!(control.trigger(|| {}));
    let ownership = control.engine.input_runtime_ownership.clone();
    let revision = ownership.launch_revision(0);
    let original = ownership.slot_snapshot(0).unwrap();
    let received_at_ns = control.engine.capture_input_timestamp_ns();
    let dispatch = runtime
        .capture_mapping_dispatch_for_test(BINDING, received_at_ns)
        .unwrap();
    if learn {
        runtime.set_learn_capture_active(true);
        runtime.set_learn_capture_active(false);
    } else {
        runtime.set_enabled(false);
        runtime.set_enabled(true);
    }
    assert!(runtime.enabled.load(Ordering::Acquire));
    assert!(!runtime.learn_capture_active.load(Ordering::Acquire));
    assert!(
        runtime.mappings.lock().unwrap()[0]
            .owner
            .load(Ordering::Acquire)
    );
    assert!(
        !dispatch(),
        "restored flags must not revive the STOP looked up before the input gap"
    );
    assert_eq!(ownership.launch_revision(0), revision);
    assert_eq!(ownership.admitted_launch_ids(), vec![0]);
    let ControlMessage::TriggerInputPad {
        binding,
        launch_revision,
        ..
    } = control.consumer.pop().unwrap()
    else {
        panic!("input ABA must retain the previously admitted original attack");
    };
    assert_eq!(
        binding.placement.unwrap().content_key(),
        original.content_key()
    );
    assert!(ownership.binding_launch_current(0, binding, launch_revision));
    assert!(control.consumer.is_empty());
    assert!(runtime.poll_event().is_none());
}

#[test]
fn disable_then_enable_cannot_revive_a_stop_from_the_previous_lookup_incarnation() {
    revoked_and_restored_before_admission(false);
}

#[test]
fn learn_then_off_cannot_revive_a_stop_from_the_previous_lookup_incarnation() {
    revoked_and_restored_before_admission(true);
}

#[test]
fn unchanged_input_switches_preserve_the_original_stop_lookup() {
    let (mut control, runtime) = fixture();
    assert!(control.trigger(|| {}));
    let ownership = control.engine.input_runtime_ownership.clone();
    let revision = ownership.launch_revision(0);
    let admission_revision = runtime.stop_admission_revision.load(Ordering::SeqCst);
    let dispatch = runtime
        .capture_mapping_dispatch_for_test(BINDING, 42)
        .unwrap();
    runtime.set_enabled(true);
    runtime.set_learn_capture_active(false);
    assert_eq!(
        runtime.stop_admission_revision.load(Ordering::SeqCst),
        admission_revision
    );
    assert!(dispatch());
    assert_eq!(ownership.launch_revision(0), revision + 1);
    assert!(ownership.admitted_launch_ids().is_empty());
    assert!(matches!(
        control.consumer.pop().unwrap(),
        ControlMessage::TriggerInputPad { .. }
    ));
    assert!(matches!(
        control.consumer.pop().unwrap(),
        ControlMessage::StopSampleForPlacement { .. }
    ));
    assert!(control.consumer.is_empty());
}

#[test]
fn partial_input_switch_publication_refuses_stop_without_holding_the_producer() {
    for change in [Revocation::Disabled, Revocation::Learn] {
        let (mut control, runtime) = fixture();
        let runtime = Arc::new(runtime);
        assert!(control.trigger(|| {}));
        let ownership = control.engine.input_runtime_ownership.clone();
        let revision = ownership.launch_revision(0);
        let original = ownership.slot_snapshot(0).unwrap();
        let admission_revision = runtime.stop_admission_revision.load(Ordering::SeqCst);
        let dispatch = runtime
            .capture_mapping_dispatch_for_test(BINDING, 42)
            .unwrap();
        let step_state = runtime.mappings.lock().unwrap()[0].pitch_step_state.clone();
        // The real setter publishes its odd revision and changed flag before
        // resetting this existing step state. No test-only dispatch hook is used.
        let held_step = step_state.lock().unwrap();
        let writer_runtime = runtime.clone();
        let writer = thread::spawn(move || revoke(&writer_runtime, change));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut during_revision = runtime.stop_admission_revision.load(Ordering::SeqCst);
        while during_revision & 1 == 0 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
            during_revision = runtime.stop_admission_revision.load(Ordering::SeqCst);
        }
        let producer_available = runtime.audio_producer.try_lock().is_ok();
        let dispatched = dispatch();
        let final_partial_revision = runtime.stop_admission_revision.load(Ordering::SeqCst);
        // Always release the control barrier before assertions or joining.
        drop(held_step);
        let writer_result = writer.join();

        writer_result.unwrap();
        assert_eq!(during_revision, admission_revision + 1, "{change:?}");
        assert_eq!(final_partial_revision, during_revision);
        assert!(
            producer_available,
            "input setters must not hold the audio producer"
        );
        assert!(!dispatched);
        assert_eq!(
            runtime.stop_admission_revision.load(Ordering::SeqCst),
            admission_revision + 2
        );
        assert_eq!(ownership.launch_revision(0), revision);
        assert_eq!(ownership.admitted_launch_ids(), vec![0]);
        let ControlMessage::TriggerInputPad {
            binding,
            launch_revision,
            received_at_ns,
            ..
        } = control.consumer.pop().unwrap()
        else {
            panic!("a partial input switch must retain the admitted original attack");
        };
        assert_eq!(received_at_ns, 42);
        assert_eq!(
            binding.placement.unwrap().content_key(),
            original.content_key()
        );
        assert!(ownership.binding_launch_current(0, binding, launch_revision));
        assert!(control.consumer.is_empty());
    }
}

#[test]
fn exhausted_input_incarnation_never_wraps_or_revives_a_stop() {
    let (mut control, runtime) = fixture();
    assert!(control.trigger(|| {}));
    let ownership = control.engine.input_runtime_ownership.clone();
    let revision = ownership.launch_revision(0);
    let original = ownership.slot_snapshot(0).unwrap();
    // Place only the private counter near its limit; real setters perform both
    // publication transitions, including the last representable stable revision.
    runtime
        .stop_admission_revision
        .store(u64::MAX - 3, Ordering::SeqCst);
    let dispatch = runtime
        .capture_mapping_dispatch_for_test(BINDING, 42)
        .unwrap();
    runtime.set_enabled(false);
    assert_eq!(
        runtime.stop_admission_revision.load(Ordering::SeqCst),
        u64::MAX - 1
    );
    runtime.set_enabled(true);
    assert_eq!(
        runtime.stop_admission_revision.load(Ordering::SeqCst),
        u64::MAX
    );
    assert!(!dispatch());
    runtime.set_learn_capture_active(true);
    runtime.set_learn_capture_active(false);
    runtime.replace_mappings(vec![(BINDING.into(), STOP_A.into())]);
    assert_eq!(
        runtime.stop_admission_revision.load(Ordering::SeqCst),
        u64::MAX
    );
    assert!(runtime.enabled.load(Ordering::Acquire));
    assert!(!runtime.learn_capture_active.load(Ordering::Acquire));
    assert!(!runtime
        .capture_mapping_dispatch_for_test(BINDING, 43)
        .unwrap()());
    assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
    let receipt = event(&runtime);
    assert_eq!(receipt.binding_key, BINDING);
    assert_eq!(receipt.action_key.as_deref(), Some(STOP_A));
    assert!(receipt.direct && !receipt.dispatched);
    assert_eq!(ownership.launch_revision(0), revision);
    assert_eq!(ownership.admitted_launch_ids(), vec![0]);
    let ControlMessage::TriggerInputPad {
        binding,
        launch_revision,
        ..
    } = control.consumer.pop().unwrap()
    else {
        panic!("exhausted input authority must retain the admitted original attack");
    };
    assert_eq!(
        binding.placement.unwrap().content_key(),
        original.content_key()
    );
    assert!(ownership.binding_launch_current(0, binding, launch_revision));
    assert!(control.consumer.is_empty());
}

#[test]
fn stop_admitted_before_mapping_control_change_keeps_its_original_cancellation() {
    for change in [
        Revocation::MappingRetired,
        Revocation::Disabled,
        Revocation::Learn,
    ] {
        let (mut control, runtime) = fixture();
        assert!(control.trigger(|| {}));
        let ownership = control.engine.input_runtime_ownership.clone();
        let original = ownership.slot_snapshot(0).unwrap();
        let revision = ownership.launch_revision(0);
        let received_at_ns = control.engine.capture_input_timestamp_ns();
        let dispatch = runtime
            .capture_mapping_dispatch_for_test(BINDING, received_at_ns)
            .unwrap();
        assert!(dispatch());
        revoke(&runtime, change);

        assert_eq!(ownership.launch_revision(0), revision + 1);
        assert!(ownership.admitted_launch_ids().is_empty());
        let ControlMessage::TriggerInputPad {
            binding,
            launch_revision,
            ..
        } = control.consumer.pop().unwrap()
        else {
            panic!("the original pending attack must keep queue order");
        };
        assert!(!ownership.binding_launch_current(0, binding, launch_revision));
        let ControlMessage::StopSampleForPlacement { placement } = control.consumer.pop().unwrap()
        else {
            panic!("an already admitted STOP cannot be erased by a later switch");
        };
        assert_eq!(placement.content_key(), original.content_key());
        assert!(control.consumer.is_empty());
    }
}

#[test]
fn actual_dispatcher_stop_receipt_keeps_input_binding_time_and_cancel_effect() {
    let (mut control, runtime) = fixture();
    assert!(control.trigger(|| {}));
    let ownership = control.engine.input_runtime_ownership.clone();
    let original = ownership.slot_snapshot(0).unwrap();
    let revision = ownership.launch_revision(0);
    let before = control.engine.capture_input_timestamp_ns();
    assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
    let receipt = event(&runtime);
    let after = control.engine.capture_input_timestamp_ns();
    assert_eq!(receipt.binding_key, BINDING);
    assert_eq!(receipt.action_key.as_deref(), Some(STOP_A));
    assert_eq!(receipt.value, 100);
    assert!(before <= receipt.received_at_ns && receipt.received_at_ns <= after);
    assert!(receipt.dispatched && receipt.direct);
    assert!(receipt.captured_trigger.is_none() && receipt.captured_pitch.is_none());
    assert!(receipt.selection_revision.is_none());
    assert_eq!(ownership.launch_revision(0), revision + 1);
    assert!(ownership.admitted_launch_ids().is_empty());
    assert!(matches!(
        control.consumer.pop().unwrap(),
        ControlMessage::TriggerInputPad { .. }
    ));
    let ControlMessage::StopSampleForPlacement { placement } = control.consumer.pop().unwrap()
    else {
        panic!("actual dispatcher must enqueue the original content-bound STOP");
    };
    assert_eq!(placement.content_key(), original.content_key());
    assert!(control.consumer.is_empty());
}

#[test]
fn admitted_stop_never_substitutes_a_later_source_at_the_same_physical_pad() {
    let mut fixture = Fixture::new(false);
    adopt_current_sources(&mut fixture, false);
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    runtime.replace_mappings(vec![(BINDING.into(), STOP_A.into())]);
    runtime.set_enabled(true);
    let original = fixture
        .control
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap();
    assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
    let receipt = event(runtime);
    assert!(receipt.dispatched && receipt.direct);
    let received_at_ns = receipt.received_at_ns;
    let stop = fixture.control.consumer.pop().unwrap();
    let ControlMessage::StopSampleForPlacement { placement } = &stop else {
        panic!("actual dispatcher did not retain the captured STOP source");
    };
    assert_eq!(placement.content_key(), original.content_key());
    let same_pcm = fixture.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    publish_tone(&fixture.control, 0, same_pcm, true);
    adopt_current_sources(&mut fixture, false);
    let replacement = fixture
        .control
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap();
    assert_ne!(replacement.content_key(), original.content_key());
    assert!(fixture.mixer.play_sample(0, 0.5));
    let generation = fixture.voice(0).generation;
    fixture.feedback.clear();
    process_control_message(
        stop,
        &mut fixture.scheduler,
        fixture.transport.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    assert_eq!(fixture.voice(0).generation, generation);
    assert!(
        fixture.feedback.is_empty(),
        "the old STOP cannot report a replacement-source effect"
    );
    assert_eq!(receipt.received_at_ns, received_at_ns);
    assert_eq!(receipt.binding_key, BINDING);
    assert_eq!(receipt.action_key.as_deref(), Some(STOP_A));
}
