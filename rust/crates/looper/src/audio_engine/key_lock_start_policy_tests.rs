//! Device-free admission tests using the production control, scheduler and mixer paths.

use super::audio_stream::{drain_control_messages, render_scheduled_audio};
use super::buffer_retirement::ImmediateAudioBufferRetirement;
use super::hold_action::control::pause_for_producer;
use super::input_mapping::launch_admission_tests::LaunchFixture;
use super::input_runtime_binding::{self, InputRuntimeOwnership};
use super::key_lock_start_policy::KeyLockStartBehavior;
use super::mixer::RtMixer;
use super::pitch_actions::{
    CapturedPitchAction, FrozenPitchRender, PitchActionAdmission, PitchActionReject,
    PitchActionSpec,
};
use super::pitch_intent::PitchIntent;
use super::resident_relocation::control_publication_tests::PendingWriter;
use super::scheduler::FixedCapacityScheduler;
use super::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};
use std::sync::{Arc, Barrier};

struct Fixture {
    control: LaunchFixture,
    mixer: Box<RtMixer>,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    quantization: TriggerQuantization,
    feedback: Vec<AudioMessage>,
}

impl Fixture {
    fn new() -> Self {
        let control = LaunchFixture::new(16);
        let mixer = Box::new(control.mixer());
        let mut fixture = Self {
            control,
            mixer,
            scheduler: FixedCapacityScheduler::new(),
            transport: TransportTimeline::new(48_000),
            quantization: TriggerQuantization::Immediate,
            feedback: Vec::new(),
        };
        fixture.callback(1);
        assert!(fixture.control.engine.can_change_key_lock_start_behavior());
        fixture
    }

    fn callback(&mut self, frames: usize) -> Vec<f32> {
        let frame = self.transport.output_frame();
        drain_control_messages(
            &mut self.control.consumer,
            &mut self.scheduler,
            frame,
            &mut self.quantization,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut ImmediateAudioBufferRetirement,
        );
        let mut output = vec![0.0; frames];
        render_scheduled_audio(
            &mut self.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; super::constants::NUM_SAMPLES],
            frame,
            1,
            &mut self.transport,
            &mut self.feedback,
            &mut ImmediateAudioBufferRetirement,
        );
        self.transport.advance_by_rendered_frames(frames);
        output
    }

    fn start(&mut self) {
        assert!(self.control.trigger(|| {}));
        let output = self.callback(1);
        assert!(self.mixer.voices.iter().any(|voice| voice.active));
        assert!(output.iter().any(|sample| *sample != 0.0));
    }
}

#[test]
fn fully_stopped_native_choice_is_synchronous_and_invalid_values_change_nothing() {
    pyo3::Python::initialize();
    let engine = super::AudioEngine::new().unwrap();
    assert_eq!(engine.key_lock_start_behavior(), "first_sound_at_grid");
    assert!(engine.can_change_key_lock_start_behavior());
    assert!(
        engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    assert_eq!(engine.key_lock_start_behavior(), "attack_at_grid");
    for invalid in ["", "AttackAtGrid", "next_start", " first_sound_at_grid"] {
        assert!(engine.set_key_lock_start_behavior(invalid).is_err());
        assert_eq!(engine.key_lock_start_behavior(), "attack_at_grid");
    }
    assert!(
        engine
            .set_key_lock_start_behavior("first_sound_at_grid")
            .unwrap()
    );
    assert_eq!(engine.key_lock_start_behavior(), "first_sound_at_grid");
}

#[test]
fn unchanged_idle_callbacks_preserve_the_existing_publication_snapshot() {
    let mut fixture = Fixture::new();
    let ownership = fixture.control.engine.input_runtime_ownership.clone();
    let token = ownership.placement_read_token();
    assert!(token.is_some());
    for _ in 0..8 {
        fixture.callback(1);
        assert_eq!(ownership.placement_read_token(), token);
        assert!(fixture.control.engine.can_change_key_lock_start_behavior());
    }
}

#[test]
fn capture_wait_before_publication_blocks_policy_until_original_start_is_cancelled() {
    let mut fixture = Fixture::new();
    let start = fixture
        .control
        .engine
        .capture_pad_start(
            0,
            16.0 / 48_000.0,
            Some(96.0 / 48_000.0),
            1.0,
            false,
            None,
            None,
        )
        .unwrap();
    let original = start.clone();
    assert_eq!(
        start.start_behavior(),
        KeyLockStartBehavior::FirstSoundAtGrid
    );
    assert!(fixture.control.engine.admitted_launch_ids().is_empty());
    assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        !fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    fixture.callback(1);
    assert!(fixture.scheduler.is_empty());
    assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
    assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(start.cancel());
    assert!(!original.cancel());
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    assert_eq!(
        original.start_behavior(),
        KeyLockStartBehavior::FirstSoundAtGrid
    );
    assert!(!original.is_current());
    let next = fixture
        .control
        .engine
        .capture_pad_start(
            0,
            16.0 / 48_000.0,
            Some(96.0 / 48_000.0),
            1.0,
            false,
            None,
            None,
        )
        .unwrap();
    assert_eq!(next.start_behavior(), KeyLockStartBehavior::AttackAtGrid);
    let retained = next.clone();
    drop(next);
    assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
    drop(retained);
    // The original target capture is still admitted in the Native control queue.
    // Dropping the external handles must not open settings before its retirement.
    assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
    fixture.callback(1);
    assert!(fixture.scheduler.is_empty());
    assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
    assert!(fixture.control.engine.can_change_key_lock_start_behavior());
}

#[test]
fn public_global_capture_keeps_both_pending_members_owned_after_target_only_callback() {
    let mut fixture = Fixture::new();
    let group = pyo3::Python::attach(|py| {
        let entries = [0, 1]
            .into_iter()
            .map(|id| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                (
                    pyo3::Py::new(py, binding).unwrap(),
                    16.0 / 48_000.0,
                    Some(96.0 / 48_000.0),
                    None,
                )
            })
            .collect();
        fixture
            .control
            .engine
            .capture_global_start(py, entries, None)
            .unwrap()
    });
    let retained = group.clone();
    assert_eq!(group.diagnostic_slots(), vec![0, 1]);
    assert!(group.original_frame().is_none());
    assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        !fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    let target = fixture.transport.output_frame();
    fixture.callback(1);
    assert_eq!(group.original_frame(), Some(target));
    assert!(fixture.control.consumer.is_empty());
    assert!(fixture.scheduler.is_empty());
    assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
    assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(group.cancel());
    assert!(!retained.cancel());
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    assert!(
        fixture
            .control
            .engine
            .publish_captured_global_start(&retained)
            .is_err()
    );
    assert!(fixture.control.consumer.is_empty());
    fixture.callback(1);
    assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
}

#[test]
fn genuine_retrigger_capture_before_worker_or_scheduler_blocks_policy_until_rejection() {
    let fixture = Fixture::new();
    let binding = input_runtime_binding::capture(&fixture.control.engine, 0)
        .unwrap()
        .unwrap();
    let spec = PitchActionSpec::parse("pitch.extra:2:set_retrigger").unwrap();
    let action = CapturedPitchAction::capture(PitchActionAdmission {
        key_lock_request: fixture
            .control
            .engine
            .input_runtime_ownership
            .key_lock_request_for_cell(binding.binding.placement.unwrap().cell())
            .load(std::sync::atomic::Ordering::Acquire),

        key: binding.binding.placement.unwrap().content_key().unwrap(),
        binding: binding.binding,
        ownership: fixture.control.engine.input_runtime_ownership.clone(),
        acknowledgements: binding.acknowledgements.clone(),
        spec,
        render: FrozenPitchRender {
            pitch: PitchIntent::new(0, 2).unwrap(),
            key_lock: true,
        },
        received_at_ns: 1,
        due_frame: 100,
        quantize_step_64ths: None,
        sync: false,
        exclusive: false,
        loop_start_s: 0.0,
        loop_end_s: None,
        input_owners: None,
        previous_requested: PitchIntent::default(),
        previous_sequence: 0,
    })
    .unwrap();
    assert_eq!(
        action.start_behavior(),
        Some(KeyLockStartBehavior::FirstSoundAtGrid)
    );
    assert!(fixture.control.consumer.is_empty());
    assert!(fixture.scheduler.is_empty());
    assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        !fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    assert!(action.reject(PitchActionReject::LaunchCancelled));
    assert!(!action.reject(PitchActionReject::LaunchCancelled));
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    assert_eq!(
        action.start_behavior(),
        Some(KeyLockStartBehavior::FirstSoundAtGrid)
    );
}

#[test]
fn live_and_paused_voices_refuse_without_modifying_resumable_history_or_policy() {
    let mut fixture = Fixture::new();
    fixture.start();
    assert!(
        !fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    fixture.control.engine.pause_sample(0).unwrap();
    assert!(fixture.callback(8).iter().all(|sample| *sample == 0.0));
    let voice = fixture
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    assert!(voice.paused);
    let original = (
        voice.generation,
        voice.frame_pos,
        voice.sample.as_ref().unwrap().source_address(),
    );
    for behavior in ["attack_at_grid", "first_sound_at_grid"] {
        assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
        assert!(
            !fixture
                .control
                .engine
                .set_key_lock_start_behavior(behavior)
                .unwrap()
        );
        assert_eq!(
            fixture.control.engine.key_lock_start_behavior(),
            "first_sound_at_grid"
        );
    }
    let retained = fixture
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    assert_eq!(
        (
            retained.generation,
            retained.frame_pos,
            retained.sample.as_ref().unwrap().source_address()
        ),
        original
    );
}

#[test]
fn pending_and_acknowledged_native_hold_remain_busy_until_actual_stop() {
    let mut fixture = Fixture::new();
    fixture.start();
    let hold =
        pause_for_producer(&fixture.control.engine, &fixture.control.producer, 0, 91, 1).unwrap();
    assert_eq!(hold.publication_status(), "pause_pending");
    assert!(
        !fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    fixture.callback(1);
    assert_eq!(hold.publication_status(), "paused_owned");
    assert!(
        fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.paused)
    );
    assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        !fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    fixture.control.engine.stop_sample(0).unwrap();
    fixture.callback(1);
    fixture.callback(1);
    assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
    assert!(!hold.is_current());
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
}

#[test]
fn queued_stop_clearing_admitted_flag_is_not_actual_stop_and_rejection_is_not_queued() {
    let mut fixture = Fixture::new();
    fixture.start();
    fixture.control.engine.stop_sample(0).unwrap();
    assert!(fixture.control.engine.admitted_launch_ids().is_empty());
    assert!(fixture.mixer.voices.iter().any(|voice| voice.active));
    assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        !fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    fixture.callback(1);
    fixture.callback(1);
    assert!(fixture.control.engine.can_change_key_lock_start_behavior());
    assert_eq!(
        fixture.control.engine.key_lock_start_behavior(),
        "first_sound_at_grid"
    );
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
}

#[test]
fn actual_callback_stop_allows_choice_despite_historical_admitted_flag() {
    let mut fixture = Fixture::new();
    fixture.start();
    assert_eq!(fixture.control.engine.admitted_launch_ids(), vec![0]);
    fixture
        .mixer
        .stop_sample_rt(0, &mut ImmediateAudioBufferRetirement);
    fixture.callback(1);
    assert_eq!(fixture.control.engine.admitted_launch_ids(), vec![0]);
    assert!(fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
}

#[test]
fn stale_empty_callback_observation_cannot_acknowledge_a_later_resume_admission() {
    let mut fixture = Fixture::new();
    let ownership = fixture.control.engine.input_runtime_ownership.clone();
    let old_revision = ownership.key_lock_start_policy.begin_callback_drain();
    fixture.control.engine.resume_sample(0).unwrap();
    ownership.finish_key_lock_start_callback(old_revision, true);
    assert!(!fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        !fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
    fixture.callback(1);
    assert!(fixture.control.engine.can_change_key_lock_start_behavior());
}

#[test]
fn publication_marker_and_pending_owner_choose_one_coherent_race_order() {
    for _ in 0..16 {
        let ownership = Arc::new(InputRuntimeOwnership::default());
        let barrier = Arc::new(Barrier::new(2));
        let capture_owner = ownership.clone();
        let capture_barrier = barrier.clone();
        let capture = std::thread::spawn(move || {
            capture_barrier.wait();
            capture_owner.register_key_lock_start_owner()
        });
        barrier.wait();
        let changed = ownership.set_key_lock_start_behavior(KeyLockStartBehavior::AttackAtGrid);
        if let Ok(lease) = capture.join().unwrap() {
            assert_eq!(
                lease.behavior(),
                if changed {
                    KeyLockStartBehavior::AttackAtGrid
                } else {
                    KeyLockStartBehavior::FirstSoundAtGrid
                }
            );
            assert!(!ownership.can_change_key_lock_start_behavior());
            let retained = lease.clone();
            lease.finish();
            retained.finish();
        }
        assert!(ownership.can_change_key_lock_start_behavior());
        assert_eq!(
            ownership.key_lock_start_behavior(),
            if changed {
                KeyLockStartBehavior::AttackAtGrid
            } else {
                KeyLockStartBehavior::FirstSoundAtGrid
            }
        );
    }
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let held = PendingWriter::hold(ownership.clone());
    assert!(!ownership.set_key_lock_start_behavior(KeyLockStartBehavior::AttackAtGrid));
    assert!(!ownership.can_change_key_lock_start_behavior());
    held.complete();
    assert!(ownership.set_key_lock_start_behavior(KeyLockStartBehavior::AttackAtGrid));
}
