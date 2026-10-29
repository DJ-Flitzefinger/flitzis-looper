use super::*;
use crate::audio_engine::audio_stream::{
    AudioMessageSink, drain_control_messages, process_control_message,
};
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, dispatch};
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{ControlMessage, TriggerQuantization};

#[test]
fn ordinary_transport_fixed_metadata_and_terminal_scratch_layout_are_bounded() {
    use crate::audio_engine::constants::{MAX_VOICES, NUM_SAMPLES};
    use std::mem::size_of;
    let slot = size_of::<OrdinaryObservationSlot>();
    let metadata = size_of::<[OrdinaryObservationSlot; NUM_SAMPLES]>()
        + 2 * size_of::<[AtomicU64; NUM_SAMPLES]>();
    let terminal_scratch = size_of::<[Option<ContentKey>; MAX_VOICES]>();
    let target = size_of::<crate::audio_engine::explicit_playback_target::ExplicitTargetSlot>();
    let stop_sources =
        NUM_SAMPLES * size_of::<crate::audio_engine::global_playback_batch::ExplicitStopSource>();
    println!(
        "explicit target={target}B; maximum control-owned source stop bindings={stop_sources}B; callback stop flags={NUM_SAMPLES}B"
    );
    assert!(target <= 128 && stop_sources <= 8192);
    println!(
        "ordinary fixed layout: slot={slot}B; {NUM_SAMPLES} slots+semantic epochs+pending origins={metadata}B; terminal {MAX_VOICES}-lane scratch={terminal_scratch}B; ControlMessage={}B; AudioMessage={}B",
        size_of::<ControlMessage>(),
        size_of::<AudioMessage>()
    );
    assert!(
        metadata <= 32 * 1024,
        "fixed ordinary metadata exceeded 32KiB"
    );
    assert!(
        terminal_scratch <= 1024,
        "fixed terminal scratch exceeded 1KiB"
    );
}

struct Path {
    fixture: LaunchFixture,
    mixer: RtMixer,
    timeline: TransportTimeline,
    scheduler: FixedCapacityScheduler<8>,
    feedback: Vec<AudioMessage>,
}
impl Path {
    fn new() -> Self {
        let fixture = LaunchFixture::new(16);
        let mixer = fixture.mixer();
        let mut path = Self {
            fixture,
            mixer,
            timeline: TransportTimeline::new(48_000),
            scheduler: FixedCapacityScheduler::new(),
            feedback: Vec::new(),
        };
        assert!(path.fixture.trigger(|| {}));
        path.drain();
        path.feedback.clear();
        path
    }
    fn drain(&mut self) {
        while let Ok(message) = self.fixture.consumer.pop() {
            dispatch(
                &mut self.mixer,
                message,
                &mut self.scheduler,
                &mut self.timeline,
                &mut TriggerQuantization::Immediate,
                &mut self.feedback,
            );
        }
    }
}

#[test]
fn a_successful_native_midi_start_promotes_the_same_source_bound_explicit_target() {
    use std::time::{Duration, Instant};
    // Use the production dispatcher and prepared ordinary-start lane. Path::new
    // deliberately keeps the older raw-trigger guard fixture used by its neighbors.
    let mut fixture = crate::audio_engine::pitch_runtime_tests::Fixture::new(false);
    fixture.refresh_clock();
    let original = {
        let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
        runtime.replace_mappings(vec![("midi:note:1:60".into(), "pad.trigger:0".into())]);
        runtime.set_enabled(true);
        assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
        let deadline = Instant::now() + Duration::from_secs(2);
        let event = loop {
            if let Some(event) = runtime.poll_event() {
                break event;
            }
            assert!(
                Instant::now() < deadline,
                "actual mapped MIDI capture did not arrive"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(event.action_key.as_deref(), Some("pad.trigger:0"));
        assert!(event.direct && event.dispatched);
        let trigger = event
            .captured_trigger
            .expect("production MIDI retains the original source owner");
        let start = trigger.captured_start();
        assert_eq!(start.received_timestamp(), event.received_at_ns);
        assert!(Arc::ptr_eq(
            start.ownership(),
            &fixture.control.engine.input_runtime_ownership
        ));
        start
    };
    assert!(matches!(fixture.control.consumer.peek().unwrap(),
        ControlMessage::CapturePadStartTarget(start) if start.same_owner(&original)));
    assert!(
        fixture
            .control
            .engine
            .current_explicit_playback_target()
            .unwrap()
            .is_none(),
        "capturing or admitting MIDI intent is not an effective playable choice"
    );
    fixture.drain();
    assert_eq!(original.state(), "applied", "{:?}", original.reason());
    assert_eq!(fixture.starts(0), 1);
    assert!(
        fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    let target = fixture
        .control
        .engine
        .current_explicit_playback_target()
        .unwrap()
        .unwrap();
    assert_eq!(target.current_control_slot().unwrap(), Some(0));
    assert_eq!(target.original_sequence(), original.sequence());
    assert_eq!(Some(target.effective_frame()), original.effective_frame());
    assert_eq!(target.content_lifetime(), original.content_key().lifetime());
    assert_eq!(
        fixture
            .control
            .engine
            .input_runtime_ownership
            .explicit_target_snapshot()
            .unwrap()
            .unwrap()
            .key,
        original.content_key()
    );
}

#[test]
fn ordinary_transport_seek_carries_exact_completed_intent_without_retagging_origin() {
    let mut path = Path::new();
    let pause = path.fixture.engine.pause_sample(0).unwrap();
    path.fixture
        .engine
        .seek_sample_with_source(0, 70.0 / 48_000.0, Some(7))
        .unwrap();
    path.drain();
    let current = pause.current_observation().unwrap().unwrap();
    assert!(current.ordinary_paused());
    assert!(current.settles_admission(&pause));
    assert!(!current.matches_admission(&pause));
    assert_eq!(current.requested_effect(), Some("seek"));
    assert_eq!(current.transport_revision(), pause.transport_revision + 1);
    let receipts: Vec<_> = path
        .feedback
        .iter()
        .filter(|message| matches!(message, AudioMessage::OrdinaryTransportFeedback { .. }))
        .collect();
    assert_eq!(receipts.len(), 2);
    assert!(
        resolve(&path.fixture.engine.input_runtime_ownership, receipts[0])
            .unwrap()
            .is_none()
    );
    let observed = resolve(&path.fixture.engine.input_runtime_ownership, receipts[1])
        .unwrap()
        .unwrap();
    assert_eq!(
        observed.observation_sequence(),
        current.observation_sequence()
    );
    let resume = path.fixture.engine.resume_sample(0).unwrap();
    assert!(!current.settles_admission(&resume));
    path.drain();
    let current = resume.current_observation().unwrap().unwrap();
    assert!(!current.ordinary_paused());
    assert!(current.matches_admission(&resume));
    assert!(current.settles_admission(&resume));
}

struct Gate {
    slots: usize,
    messages: Vec<AudioMessage>,
}
impl AudioMessageSink for Gate {
    fn push_audio_message(&mut self, message: AudioMessage) {
        assert!(
            self.slots > 0,
            "ordinary effect lost its reserved completion"
        );
        self.slots -= 1;
        self.messages.push(message);
    }
    fn available_audio_message_slots(&mut self) -> usize {
        self.slots
    }
}

#[test]
fn ordinary_transport_feedback_pressure_preserves_head_and_direct_deferred_command() {
    for direct in [false, true] {
        let mut path = Path::new();
        let pause = path.fixture.engine.pause_sample(0).unwrap();
        let before = path.mixer.transport_state_for_test(0).unwrap();
        let mut gate = Gate {
            slots: 0,
            messages: Vec::new(),
        };
        if direct {
            let message = path.fixture.consumer.pop().unwrap();
            process_control_message(
                message,
                &mut path.scheduler,
                0,
                &mut TriggerQuantization::Immediate,
                &mut path.timeline,
                &mut path.mixer,
                &mut gate,
                &mut ImmediateAudioBufferRetirement,
            );
            assert!(
                matches!(path.mixer.deferred_control(), Some(ControlMessage::TransportForPlacement { transport_revision, .. }) if *transport_revision == pause.transport_revision)
            );
        } else {
            assert_eq!(
                drain_control_messages(
                    &mut path.fixture.consumer,
                    &mut path.scheduler,
                    0,
                    &mut TriggerQuantization::Immediate,
                    &mut path.timeline,
                    &mut path.mixer,
                    &mut gate,
                    &mut ImmediateAudioBufferRetirement
                ),
                0
            );
            assert!(!path.fixture.consumer.is_empty());
        }
        assert_eq!(path.mixer.transport_state_for_test(0).unwrap(), before);
        gate.slots = 1;
        drain_control_messages(
            &mut path.fixture.consumer,
            &mut path.scheduler,
            0,
            &mut TriggerQuantization::Immediate,
            &mut path.timeline,
            &mut path.mixer,
            &mut gate,
            &mut ImmediateAudioBufferRetirement,
        );
        assert_eq!(gate.messages.len(), 1);
        let current = pause.current_observation().unwrap().unwrap();
        assert!(current.matches_admission(&pause));
        assert!(current.ordinary_paused());
    }
}

#[test]
fn ordinary_transport_lifecycle_same_origin_cannot_resurrect_old_receipt() {
    let mut path = Path::new();
    let pause = path.fixture.engine.pause_sample(0).unwrap();
    path.drain();
    let receipt = path
        .feedback
        .iter()
        .find(|message| matches!(message, AudioMessage::OrdinaryTransportFeedback { .. }))
        .unwrap()
        .clone();
    let first = pause
        .current_observation()
        .unwrap()
        .unwrap()
        .observation_sequence();
    path.mixer
        .stop_sample_rt(0, &mut ImmediateAudioBufferRetirement);
    assert!(
        resolve(&path.fixture.engine.input_runtime_ownership, &receipt)
            .unwrap()
            .is_none()
    );
    let current = pause.current_observation().unwrap().unwrap();
    assert_eq!(current.status_kind(), "stopped");
    assert!(!current.active());
    assert!(!current.ordinary_paused());
    assert!(current.settles_admission(&pause));
    assert!(current.observation_sequence() > first);
    assert!(path.mixer.play_sample(0, 1.0));
    let current = pause.current_observation().unwrap().unwrap();
    assert_eq!(current.status_kind(), "started");
    assert!(current.active());
    assert!(!current.ordinary_paused());
    assert!(
        resolve(&path.fixture.engine.input_runtime_ownership, &receipt)
            .unwrap()
            .is_none()
    );
}

#[test]
fn ordinary_transport_revoke_identical_republication_never_resurrects_admission() {
    let mut path = Path::new();
    let pause = path.fixture.engine.pause_sample(0).unwrap();
    path.drain();
    let old_source = pause.source;
    let sample = path.fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let ownership = &path.fixture.engine.input_runtime_ownership;
    ownership.revoke_all_sources();
    ownership.publish_source(0, &sample, 48_000, 7);
    assert_ne!(
        ownership.ordinary_source_epoch(pause.key.unwrap().cell()),
        Some(old_source)
    );
    assert!(pause.current_control_slot().unwrap().is_none());
    assert!(pause.current_observation().unwrap().is_none());
}

#[test]
fn ordinary_transport_compatible_resident_refresh_preserves_source_and_completed_custody() {
    let mut path = Path::new();
    let pause = path.fixture.engine.pause_sample(0).unwrap();
    path.drain();
    let before = pause.current_observation().unwrap().unwrap();
    let source = path.fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let original_view = source.residency.as_ref().unwrap();
    let refreshed = crate::messages::SampleBuffer {
        channels: 1,
        samples: Arc::from([0.25; 64]),
        residency: Some(Arc::new(crate::messages::ResidentSourceView {
            source: original_view.source.clone(),
            start_frame: 32,
            window_revision: original_view.window_revision + 1,
            context: crate::messages::ResidentContext::FiniteLoop,
        })),
    };
    assert!(source.same_source(&refreshed));
    assert!(!source.same_window(&refreshed));
    let ownership = &path.fixture.engine.input_runtime_ownership;
    ownership.publish_source(0, &refreshed, 48_000, 7);
    path.mixer.load_sample(0, refreshed);
    assert_eq!(pause.current_control_slot().unwrap(), Some(0));
    let current = pause.current_observation().unwrap().unwrap();
    assert_eq!(
        current.observation_sequence(),
        before.observation_sequence()
    );
    assert!(current.matches_admission(&pause));
    assert!(current.ordinary_paused());
}

#[test]
fn ordinary_transport_source_changed_after_admission_refuses_old_scalar_effect() {
    let mut path = Path::new();
    let pause = path.fixture.engine.pause_sample(0).unwrap();
    let source = path.fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    path.fixture
        .engine
        .input_runtime_ownership
        .publish_source(0, &source, 48_000, 9);
    path.drain();
    assert!(!path.mixer.transport_state_for_test(0).unwrap().0);
    assert!(pause.current_control_slot().unwrap().is_none());
    assert!(
        path.feedback
            .iter()
            .all(|message| !matches!(message, AudioMessage::OrdinaryTransportFeedback { .. }))
    );
}

fn hold(path: &Path, press: u64) -> crate::audio_engine::hold_action::HoldActionTicket {
    Python::attach(|py| {
        let values = pyo3::types::PyDict::new(py);
        values.set_item("press", press).unwrap();
        path.fixture
            .engine
            .pause_waveform_hold(0, &values.get_item("press").unwrap().unwrap(), None)
            .unwrap()
    })
}

fn drain_gate(path: &mut Path, gate: &mut Gate) -> usize {
    drain_control_messages(
        &mut path.fixture.consumer,
        &mut path.scheduler,
        path.timeline.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut path.timeline,
        &mut path.mixer,
        gate,
        &mut ImmediateAudioBufferRetirement,
    )
}

fn render_held(path: &mut Path, gate: &mut Gate) {
    let mut audio = [0.0; 32];
    let start = path.timeline.output_frame();
    crate::audio_engine::audio_stream::render_scheduled_audio(
        &mut path.mixer,
        &mut path.scheduler,
        &mut audio,
        &mut [0.0; crate::audio_engine::constants::NUM_SAMPLES],
        start,
        1,
        &mut path.timeline,
        gate,
        &mut ImmediateAudioBufferRetirement,
    );
    path.timeline.advance_by_rendered_frames(32);
    assert!(audio.iter().all(|value| *value == 0.0));
}

#[test]
fn ordinary_transport_deferred_head_preserves_exact_hold_cause_through_render_and_retry() {
    let mut path = Path::new();
    let held = hold(&path, 1);
    path.drain();
    let effect = held.action.effect().unwrap();
    let before = path.mixer.transport_state_for_test(0).unwrap();
    let pause = path.fixture.engine.pause_sample(0).unwrap();
    let mut gate = Gate {
        slots: 0,
        messages: Vec::new(),
    };
    assert_eq!(drain_gate(&mut path, &mut gate), 0);
    render_held(&mut path, &mut gate);
    assert_eq!(
        held.action.state(),
        crate::audio_engine::hold_action::HoldActionState::PausedOwned
    );
    assert_eq!(path.mixer.transport_state_for_test(0).unwrap(), before);
    assert_eq!(
        path.mixer
            .capture_hold_projection_rt(effect.key)
            .unwrap()
            .effect
            .unwrap()
            .effect_id,
        effect.effect_id
    );
    assert!(
        !held.action.intent_current(),
        "old release authority is fenced at admission"
    );
    gate.slots = 1;
    assert_eq!(drain_gate(&mut path, &mut gate), 1);
    let current = pause.current_observation().unwrap().unwrap();
    assert_eq!(current.outcome(), Some("no_effect"));
    assert!(current.ordinary_paused());
    assert_eq!(
        held.action.state(),
        crate::audio_engine::hold_action::HoldActionState::Superseded
    );
    assert!(path.mixer.capture_hold_projection_rt(effect.key).is_none());
    assert_eq!(
        path.fixture
            .engine
            .input_runtime_ownership
            .pending_ordinary_origin(effect.key.cell()),
        0
    );
}

#[test]
fn ordinary_transport_public_admission_during_hold_cleanup_preserves_cause_until_exact_retry() {
    use crate::audio_engine::hold_action::HoldActionState;
    use std::cell::RefCell;
    use std::rc::Rc;

    for admitted_before_cleanup in [false, true] {
        let mut path = Path::new();
        let held = hold(&path, 1);
        path.drain();
        let effect = held.action.effect().unwrap();
        let before = path.mixer.transport_state_for_test(0).unwrap();
        let ownership = path.fixture.engine.input_runtime_ownership.clone();
        assert_eq!(ownership.pending_ordinary_origin(effect.key.cell()), 0);
        let engine = Python::attach(|py| Py::new(py, path.fixture.engine).unwrap());
        let admission = Rc::new(RefCell::new(None::<OrdinaryTransportAdmission>));
        if admitted_before_cleanup {
            Python::attach(|py| {
                *admission.borrow_mut() = Some(engine.borrow_mut(py).pause_sample(0).unwrap());
            });
        }
        let hook_engine = Python::attach(|py| engine.clone_ref(py));
        let hook_admission = admission.clone();
        let hook_ownership = ownership.clone();
        let mut gate = Gate {
            slots: 0,
            messages: Vec::new(),
        };
        RtMixer::with_hold_cleanup_intent_interleaving_for_test(
            move || {
                // This hook is immediately BEFORE the real intent load. Keeping
                // it here with the former pending-first ordering reproduces the
                // old-zero-pending/new-global race, rather than a fake algorithm.
                if admitted_before_cleanup {
                    assert_eq!(
                        hook_ownership.pending_ordinary_origin(effect.key.cell()),
                        hook_admission
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .transport_revision()
                    );
                } else {
                    assert_eq!(hook_ownership.pending_ordinary_origin(effect.key.cell()), 0);
                    Python::attach(|py| {
                        *hook_admission.borrow_mut() =
                            Some(hook_engine.borrow_mut(py).pause_sample(0).unwrap());
                    });
                }
            },
            || {
                let mut audio = [0.0; 32];
                let start = path.timeline.output_frame();
                crate::audio_engine::audio_stream::render_scheduled_audio(
                    &mut path.mixer,
                    &mut path.scheduler,
                    &mut audio,
                    &mut [0.0; crate::audio_engine::constants::NUM_SAMPLES],
                    start,
                    1,
                    &mut path.timeline,
                    &mut gate,
                    &mut ImmediateAudioBufferRetirement,
                );
                path.timeline.advance_by_rendered_frames(32);
                assert!(audio.iter().all(|value| *value == 0.0));
            },
        );
        let admission = admission.borrow_mut().take().unwrap();
        assert!(
            !held.action.intent_current(),
            "the public admission already fences release"
        );
        assert_eq!(held.action.state(), HoldActionState::PausedOwned);
        assert_eq!(
            path.mixer
                .capture_hold_projection_rt(effect.key)
                .unwrap()
                .effect,
            Some(effect)
        );
        assert_eq!(path.mixer.transport_state_for_test(0).unwrap(), before);
        assert!(
            !admission
                .current_observation()
                .unwrap()
                .unwrap()
                .ordinary_paused()
        );
        assert_eq!(
            drain_control_messages(
                &mut path.fixture.consumer,
                &mut path.scheduler,
                path.timeline.output_frame(),
                &mut TriggerQuantization::Immediate,
                &mut path.timeline,
                &mut path.mixer,
                &mut gate,
                &mut ImmediateAudioBufferRetirement,
            ),
            0
        );
        assert_eq!(
            path.mixer
                .capture_hold_projection_rt(effect.key)
                .unwrap()
                .effect,
            Some(effect)
        );
        assert!(gate.messages.is_empty());
        gate.slots = 1;
        assert_eq!(
            drain_control_messages(
                &mut path.fixture.consumer,
                &mut path.scheduler,
                path.timeline.output_frame(),
                &mut TriggerQuantization::Immediate,
                &mut path.timeline,
                &mut path.mixer,
                &mut gate,
                &mut ImmediateAudioBufferRetirement,
            ),
            1
        );
        let observed = admission.current_observation().unwrap().unwrap();
        assert!(observed.matches_admission(&admission));
        assert!(observed.settles_admission(&admission));
        assert!(observed.ordinary_paused());
        assert_eq!(observed.outcome(), Some("no_effect"));
        assert_eq!(held.action.state(), HoldActionState::Superseded);
        assert!(path.mixer.capture_hold_projection_rt(effect.key).is_none());
        assert_eq!(path.mixer.transport_state_for_test(0).unwrap(), before);
        assert_eq!(ownership.pending_ordinary_origin(effect.key.cell()), 0);
        assert_eq!(gate.messages.len(), 1);
        Python::attach(|_| drop(engine));
    }
}

#[test]
fn ordinary_transport_earlier_release_retains_cause_and_older_completion_preserves_newer_marker() {
    let mut path = Path::new();
    let held = hold(&path, 1);
    path.drain();
    let key = held.action.key();
    assert!(path.fixture.engine.release_waveform_hold(&held).unwrap());
    let pause = path.fixture.engine.pause_sample(0).unwrap();
    let resume = path.fixture.engine.resume_sample(0).unwrap();
    let release = path.fixture.consumer.pop().unwrap();
    let mut gate = Gate {
        slots: 1,
        messages: Vec::new(),
    };
    process_control_message(
        release,
        &mut path.scheduler,
        0,
        &mut TriggerQuantization::Immediate,
        &mut path.timeline,
        &mut path.mixer,
        &mut gate,
        &mut ImmediateAudioBufferRetirement,
    );
    assert_eq!(
        held.action.state(),
        crate::audio_engine::hold_action::HoldActionState::Superseded
    );
    assert!(path.mixer.capture_hold_projection_rt(key).is_some());
    gate.slots = 0;
    render_held(&mut path, &mut gate);
    let first = path.fixture.consumer.pop().unwrap();
    gate.slots = 1;
    process_control_message(
        first,
        &mut path.scheduler,
        path.timeline.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut path.timeline,
        &mut path.mixer,
        &mut gate,
        &mut ImmediateAudioBufferRetirement,
    );
    let first = pause.current_observation().unwrap().unwrap();
    assert!(first.ordinary_paused());
    assert_eq!(first.outcome(), Some("no_effect"));
    assert_eq!(
        path.fixture
            .engine
            .input_runtime_ownership
            .pending_ordinary_origin(key.cell()),
        resume.transport_revision()
    );
    gate.slots = 1;
    assert_eq!(drain_gate(&mut path, &mut gate), 1);
    assert!(
        !resume
            .current_observation()
            .unwrap()
            .unwrap()
            .ordinary_paused()
    );
    assert_eq!(
        path.fixture
            .engine
            .input_runtime_ownership
            .pending_ordinary_origin(key.cell()),
        0
    );
}

#[test]
fn ordinary_transport_pause_then_hold_no_effect_keeps_ordinary_frontier() {
    let mut path = Path::new();
    let pause = path.fixture.engine.pause_sample(0).unwrap();
    let ticket = hold(&path, 1);
    path.drain();
    assert_eq!(ticket.publication_status(), "no_effect");
    let current = pause.current_observation().unwrap().unwrap();
    assert!(current.ordinary_paused());
    assert!(current.matches_admission(&pause));
    assert!(current.settles_admission(&pause));
}

#[test]
fn ordinary_transport_scalar_seek_takes_over_only_its_consumed_valid_hold() {
    let mut path = Path::new();
    let monitor = path.fixture.engine.resume_sample(0).unwrap();
    path.drain();
    let before = monitor.current_observation().unwrap().unwrap();
    let ticket = hold(&path, 1);
    path.drain();
    assert!(path.mixer.transport_state_for_test(0).unwrap().0);
    let held = monitor.current_observation().unwrap().unwrap();
    assert_eq!(held.observation_sequence(), before.observation_sequence());
    assert!(!held.ordinary_paused());
    path.fixture
        .engine
        .seek_sample_with_source(0, 70.0 / 48_000.0, Some(7))
        .unwrap();
    path.drain();
    let current = monitor.current_observation().unwrap().unwrap();
    assert_eq!(current.requested_effect(), Some("seek"));
    assert_eq!(current.outcome(), Some("applied"));
    assert!(current.ordinary_paused());
    assert!(current.settles_admission(&monitor));
    assert!(path.fixture.engine.release_waveform_hold(&ticket).unwrap());
    path.drain();
    assert!(path.mixer.transport_state_for_test(0).unwrap().0);
}

#[test]
fn ordinary_transport_first_seek_observation_itself_retains_durable_restart_monitor() {
    let mut path = Path::new();
    let _hold = hold(&path, 1);
    path.drain();
    path.fixture
        .engine
        .seek_sample_with_source(0, 70.0 / 48_000.0, Some(7))
        .unwrap();
    path.drain();
    let bootstrap = path
        .fixture
        .engine
        ._current_ordinary_transport_observation(0)
        .unwrap()
        .unwrap();
    assert!(bootstrap.ordinary_paused());
    assert_eq!(bootstrap.requested_effect(), Some("seek"));
    let receipt = path
        .feedback
        .iter()
        .find(|message| matches!(message, AudioMessage::OrdinaryTransportFeedback { .. }))
        .unwrap();
    let monitor = path
        .fixture
        .engine
        ._resolve_ordinary_transport_feedback(receipt)
        .unwrap()
        .unwrap();
    assert!(monitor.ordinary_paused());
    assert_eq!(monitor.current_control_slot().unwrap(), Some(0));
    assert_eq!(
        monitor
            .current_observation()
            .unwrap()
            .unwrap()
            .observation_sequence(),
        monitor.observation_sequence()
    );
    // Deliberately do not consume a best-effort started event. Actual Native birth is
    // discoverable through the retained opaque observation even with no prior admission.
    assert!(path.mixer.play_sample(0, 1.0));
    let current = monitor.current_observation().unwrap().unwrap();
    assert_eq!(current.status_kind(), "started");
    assert!(current.active());
    assert!(!current.ordinary_paused());
    assert!(current.observation_sequence() > monitor.observation_sequence());
    assert!(monitor.ordinary_paused(), "original DTO remains immutable");
}

#[test]
fn ordinary_transport_bootstrap_query_transient_writer_returns_none_and_validates_bounds() {
    let path = Path::new();
    let ownership = &path.fixture.engine.input_runtime_ownership;
    let cell = ownership.slot_snapshot(0).unwrap().cell();
    let slot = &ownership.ordinary_observations[cell.index()];
    let sequence = slot.sequence.load(Ordering::SeqCst);
    slot.sequence.store(sequence + 1, Ordering::SeqCst);
    assert!(
        path.fixture
            .engine
            ._current_ordinary_transport_observation(0)
            .unwrap()
            .is_none()
    );
    slot.sequence.store(sequence, Ordering::SeqCst);
    assert!(
        path.fixture
            .engine
            ._current_ordinary_transport_observation(0)
            .unwrap()
            .is_some()
    );
    assert!(
        path.fixture
            .engine
            ._current_ordinary_transport_observation(crate::audio_engine::constants::NUM_SAMPLES)
            .is_err()
    );
}

#[test]
fn ordinary_transport_retired_equal_origin_command_never_clears_new_lifetime_pending_custody() {
    let mut path = Path::new();
    let old = path.fixture.engine.pause_sample(0).unwrap();
    let old_command = path.fixture.consumer.pop().unwrap();
    path.mixer
        .stop_sample_rt(0, &mut ImmediateAudioBufferRetirement);
    path.fixture.publish(0, 0.25, 7);
    let replacement = path.fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    path.mixer.load_sample(0, replacement);
    assert!(path.mixer.play_sample(0, 1.0));
    // The initial lifetime already admitted a genuine start. Reach the same
    // numeric ordinary origin on this fresh lifetime through a real NoEffect.
    let no_effect = path.fixture.engine.resume_sample(0).unwrap();
    path.drain();
    assert_eq!(
        no_effect.current_observation().unwrap().unwrap().outcome(),
        Some("no_effect")
    );
    let current = path.fixture.engine.pause_sample(0).unwrap();
    assert_eq!(current.transport_revision(), old.transport_revision());
    assert_ne!(current.content_lifetime(), old.content_lifetime());
    let key = current.key.unwrap();
    dispatch(
        &mut path.mixer,
        old_command,
        &mut path.scheduler,
        &mut path.timeline,
        &mut TriggerQuantization::Immediate,
        &mut path.feedback,
    );
    assert_eq!(
        path.fixture
            .engine
            .input_runtime_ownership
            .pending_ordinary_origin(key.cell()),
        current.transport_revision()
    );
    assert!(!path.mixer.transport_state_for_test(0).unwrap().0);
    path.drain();
    assert!(
        current
            .current_observation()
            .unwrap()
            .unwrap()
            .ordinary_paused()
    );
    assert_eq!(
        path.fixture
            .engine
            .input_runtime_ownership
            .pending_ordinary_origin(key.cell()),
        0
    );
    assert!(old.current_observation().unwrap().is_none());
}

#[test]
fn ordinary_transport_sequence_last_success_remains_readable_then_lifecycle_closes_authority() {
    let mut path = Path::new();
    let cell = path
        .fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap()
        .cell();
    path.fixture
        .engine
        .input_runtime_ownership
        .ordinary_observations[cell.index()]
    .sequence
    .store(u64::MAX - 3, Ordering::SeqCst);
    let pause = path.fixture.engine.pause_sample(0).unwrap();
    path.drain();
    let current = pause.current_observation().unwrap().unwrap();
    assert_eq!(current.observation_sequence(), u64::MAX - 1);
    assert_eq!(
        path.fixture
            .engine
            ._current_ordinary_transport_observation(0)
            .unwrap()
            .unwrap()
            .observation_sequence(),
        u64::MAX - 1
    );
    assert!(current.ordinary_paused());
    assert!(path.fixture.engine.resume_sample(0).is_err());
    assert!(path.mixer.transport_state_for_test(0).unwrap().0);
    path.mixer
        .stop_sample_rt(0, &mut ImmediateAudioBufferRetirement);
    assert!(pause.current_observation().is_err());
    assert!(pause.current_control_slot().is_err());
    assert!(
        path.fixture
            .engine
            ._current_ordinary_transport_observation(0)
            .is_err()
    );
    assert!(path.mixer.transport_state_for_test(0).is_none());
}

#[test]
fn ordinary_transport_already_queued_at_terminal_sequence_never_mutates_or_claims_success() {
    let mut path = Path::new();
    let pause = path.fixture.engine.pause_sample(0).unwrap();
    let cell = pause.key.unwrap().cell();
    path.fixture
        .engine
        .input_runtime_ownership
        .ordinary_observations[cell.index()]
    .sequence
    .store(u64::MAX, Ordering::SeqCst);
    let before = path.mixer.transport_state_for_test(0).unwrap();
    path.drain();
    assert_eq!(path.mixer.transport_state_for_test(0).unwrap(), before);
    assert!(path.feedback.is_empty());
    assert!(pause.current_observation().is_err());
    path.mixer
        .stop_sample_rt(0, &mut ImmediateAudioBufferRetirement);
    assert!(path.mixer.transport_state_for_test(0).is_none());
}

#[test]
fn ordinary_transport_full_control_queue_does_not_publish_a_requested_intent() {
    let mut path = Path::new();
    let ownership = path.fixture.engine.input_runtime_ownership.clone();
    let cell = ownership.slot_snapshot(0).unwrap().cell();
    let revision = ownership
        .transport_intent_for_cell(cell)
        .load(Ordering::Acquire);
    {
        let mut producer = path.fixture.producer.lock().unwrap();
        for _ in 0..16 {
            producer.push(ControlMessage::Ping()).unwrap();
        }
    }
    assert!(path.fixture.engine.pause_sample(0).is_err());
    assert_eq!(
        ownership
            .transport_intent_for_cell(cell)
            .load(Ordering::Acquire),
        revision
    );
    assert!(!path.mixer.transport_state_for_test(0).unwrap().0);
}
