use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, OTHER, dispatch};
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};
use rtrb::RingBuffer;
use std::sync::{Arc, Mutex};

fn deliver(fixture: &mut LaunchFixture, mixer: &mut RtMixer) {
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut transport = TransportTimeline::new(48_000);
    let mut quantization = TriggerQuantization::Immediate;
    let mut feedback = Vec::new();
    while let Ok(message) = fixture.consumer.pop() {
        dispatch(
            mixer,
            message,
            &mut scheduler,
            &mut transport,
            &mut quantization,
            &mut feedback,
        );
    }
}

fn fixture() -> (LaunchFixture, RtMixer) {
    let mut fixture = LaunchFixture::new(8);
    let (_, consumer) = RingBuffer::<AudioMessage>::new(8);
    fixture.engine.test_control_channels =
        Some((fixture.producer.clone(), Arc::new(Mutex::new(consumer))));
    let mut mixer = fixture.mixer();
    assert!(fixture.trigger(|| {}));
    deliver(&mut fixture, &mut mixer);
    assert!(mixer.transport_state_for_test(0).is_some());
    (fixture, mixer)
}

#[test]
fn accepted_pause_then_seek_before_callback_preserves_both_ordered_effects() {
    let (mut fixture, mut mixer) = fixture();
    fixture.engine.pause_sample(0).unwrap();
    fixture
        .engine
        .seek_sample_with_source(0, 70.0 / 48_000.0, Some(7))
        .unwrap();
    deliver(&mut fixture, &mut mixer);
    let (paused, frame, _) = mixer.transport_state_for_test(0).unwrap();
    assert!(paused);
    assert_eq!(frame, 70);
}

#[test]
fn accepted_seek_then_resume_before_callback_resumes_at_the_new_position() {
    let (mut fixture, mut mixer) = fixture();
    fixture.engine.pause_sample(0).unwrap();
    deliver(&mut fixture, &mut mixer);
    assert!(mixer.transport_state_for_test(0).unwrap().0);
    fixture
        .engine
        .seek_sample_with_source(0, 70.0 / 48_000.0, Some(7))
        .unwrap();
    fixture.engine.resume_sample(0).unwrap();
    deliver(&mut fixture, &mut mixer);
    let (paused, frame, _) = mixer.transport_state_for_test(0).unwrap();
    assert!(!paused);
    assert_eq!(frame, 70);
}

#[test]
fn accepted_transport_follows_original_move_and_never_the_equal_byte_new_occupant() {
    let (mut fixture, mut mixer) = fixture();
    fixture.engine.pause_sample(0).unwrap();
    fixture.swap(&mut mixer);
    fixture.publish(0, 0.25, 7);
    mixer.load_sample(
        0,
        fixture.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap(),
    );
    mixer.play_sample(0, 1.0);
    assert!(!mixer.transport_state_for_test(0).unwrap().0);
    deliver(&mut fixture, &mut mixer);
    assert!(mixer.transport_state_for_test(OTHER).unwrap().0);
    assert!(!mixer.transport_state_for_test(0).unwrap().0);
}
