//! Current-thread Rust allocation observations of real SET callbacks, with live workers.
//!
//! Native C++/FFT allocations and device deadlines are outside this allocator's scope.
//! This measures shared SET drain/render phases, not the outer CPAL callback's
//! clock publication, parameter drains or telemetry wrapper.
//! Reference PCM and diagnostics are collected outside each measured callback.

use super::*;
use crate::audio_engine::ordinary_endpoint_reserve_tests::{
    adopt_current_sources, refresh_actual_runtime_bindings,
};
use crate::test_memory::{ThreadAllocations, observe_current_thread};
use rtrb::{Consumer, Producer};

const FEEDBACK_CAPACITY: usize = 64;
const CALLBACK_FRAMES: usize = 512;
const IRREGULAR_FRAMES: [usize; 7] = [7, 509, 3, 127, 512, 31, 251];

fn callback_into(fixture: &mut Fixture, output: &mut [f32], feedback: &mut Producer<AudioMessage>) {
    let frame = fixture.transport.output_frame();
    drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        frame,
        &mut TriggerQuantization::Immediate,
        &mut fixture.transport,
        &mut fixture.mixer,
        feedback,
        &mut fixture.retirement,
    );
    render_scheduled_audio(
        &mut fixture.mixer,
        &mut fixture.scheduler,
        output,
        &mut [0.0; NUM_SAMPLES],
        frame,
        fixture.channels,
        &mut fixture.transport,
        feedback,
        &mut fixture.retirement,
    );
    fixture
        .transport
        .advance_by_rendered_frames(output.len() / fixture.channels);
}

fn measured_callback(
    fixture: &mut Fixture,
    output: &mut [f32],
    feedback: &mut Producer<AudioMessage>,
    phase: &str,
) {
    let ((), observed) = observe_current_thread(|| callback_into(fixture, output, feedback));
    assert_eq!(observed, ThreadAllocations::default(), "{phase}");
}

fn wait_custody_released(fixture: &Fixture) {
    let deadline = Instant::now() + WORKER_WAIT;
    while fixture.control.engine.pitch_work_capacity.used() != 0 {
        assert!(
            Instant::now() < deadline,
            "terminal SET work did not retire"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn started_fixture(
    lock: bool,
    extra: i32,
) -> (Fixture, Producer<AudioMessage>, Consumer<AudioMessage>) {
    let mut fixture = Fixture::new(lock);
    // Reuse the actual LoadSample claim/SourceACK setup; the shared Fixture's
    // direct bank installation alone cannot authorize real content feedback.
    adopt_current_sources(&mut fixture, lock);
    refresh_actual_runtime_bindings(&fixture);
    let (mut feedback, mut messages) = RingBuffer::new(FEEDBACK_CAPACITY);
    let start = fixture.capture(0, &format!("pitch.extra:{extra}:set_retrigger"));
    assert!(fixture.control.engine.publish_pitch_action(&start).unwrap());
    callback_into(&mut fixture, &mut [], &mut feedback);
    if !start.effective() {
        fixture.wait_worker(&[&start]);
        // Keep feedback on this measured callback's genuine producer while
        // observing completion-driven E before consuming any source history.
        callback_into(&mut fixture, &mut [], &mut feedback);
        measured_render_to(&mut fixture, start.execution_target(), &mut feedback);
    }
    callback_into(&mut fixture, &mut [0.0], &mut feedback);
    assert!(start.effective());
    assert!(matches!(
        messages.pop().unwrap(),
        AudioMessage::ContentFeedback { id: 0, kind: 1, .. }
    ));
    assert!(messages.is_empty());
    assert_eq!(feedback.slots(), FEEDBACK_CAPACITY);
    fixture.pcm(733); // A live source/FIFO checkpoint away from a Native block boundary.
    wait_custody_released(&fixture);
    (fixture, feedback, messages)
}

fn wait_ready_with_measured_callbacks(
    fixture: &mut Fixture,
    action: &CapturedPitchAction,
    feedback: &mut Producer<AudioMessage>,
) {
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        // Successful prepared-result polling stays inside the real callback measurement.
        measured_callback(fixture, &mut [], feedback, "prepared SET callback polling");
        if fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(action.sequence())
        {
            break;
        }
        assert_ne!(action.state(), "rejected", "reason={:?}", action.reason());
        assert!(
            Instant::now() < deadline,
            "SET preparation host wait exceeded 2 s"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
}

fn measured_render_to(fixture: &mut Fixture, target: u64, feedback: &mut Producer<AudioMessage>) {
    let mut output = [0.0; CALLBACK_FRAMES];
    assert!(target >= fixture.transport.output_frame());
    while fixture.transport.output_frame() < target {
        let frames =
            (target - fixture.transport.output_frame()).min(CALLBACK_FRAMES as u64) as usize;
        measured_callback(
            fixture,
            &mut output[..frames],
            feedback,
            "SET preparation lead rendering",
        );
    }
    assert_eq!(fixture.transport.output_frame(), target);
}

fn measured_pcm_into(
    fixture: &mut Fixture,
    pcm: &mut [f32],
    feedback: &mut Producer<AudioMessage>,
) {
    let mut output = [0.0; CALLBACK_FRAMES];
    let mut written = 0;
    let mut block = 0;
    while written < pcm.len() {
        let frames = IRREGULAR_FRAMES[block % IRREGULAR_FRAMES.len()].min(pcm.len() - written);
        measured_callback(
            fixture,
            &mut output[..frames],
            feedback,
            "SET irregular Native continuation",
        );
        pcm[written..written + frames].copy_from_slice(&output[..frames]);
        written += frames;
        block += 1;
    }
}

fn assert_matching_pcm(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    assert!(actual.iter().all(|sample| sample.is_finite()));
    assert!(expected.iter().all(|sample| sample.is_finite()));
    let maximum = actual
        .iter()
        .zip(expected)
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0_f32, f32::max);
    assert_eq!(
        maximum, 0.0,
        "independent reference PCM must match every sample"
    );
    let mean_square = actual
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / actual.len() as f64;
    assert!(
        mean_square > 0.001,
        "Native continuation must contain real audio"
    );
}

#[test]
fn hot_set_capture_adoption_and_native_continuation_allocate_no_rust_callback_storage() {
    for lock in [false, true] {
        let (mut fixture, mut feedback, messages) = started_fixture(lock, 0);
        let (mut reference, _reference_feedback, _reference_messages) = started_fixture(lock, 0);
        let generation = fixture.voice(0).generation;
        let position = fixture.voice(0).source_playback.position();
        let capture_frame = fixture.transport.output_frame();
        let action = fixture.capture(0, "pitch.extra:7:set");
        let expected_action = reference.capture(0, "pitch.extra:7:set");
        assert!(
            fixture
                .control
                .engine
                .publish_pitch_action(&action)
                .unwrap()
        );
        reference.publish(&expected_action);

        measured_callback(
            &mut fixture,
            &mut [],
            &mut feedback,
            "live SET source capture",
        );
        assert!(action.accepted());
        assert!(!action.effective());
        assert_eq!(action.original_due_frame(), capture_frame);
        assert_eq!(action.execution_target(), capture_frame + 8192);
        assert_eq!(
            action.execution_target(),
            expected_action.execution_target()
        );
        assert_eq!(fixture.voice(0).source_playback.position(), position);
        wait_ready_with_measured_callbacks(&mut fixture, &action, &mut feedback);
        reference.wait_worker(&[&expected_action]);

        let execution = action.execution_target();
        measured_render_to(&mut fixture, execution, &mut feedback);
        reference.render_to(execution);
        let mut expected = reference.render(1);
        expected.extend(reference.pcm(4095));
        let mut actual = vec![0.0; expected.len()];
        measured_callback(
            &mut fixture,
            &mut actual[..1],
            &mut feedback,
            "prepared SET adoption at E",
        );
        assert!(action.effective());
        assert!(fixture.scheduler.is_empty());
        let native = fixture.voice(0).stretch.native_state_address();
        let fed_at_adoption = fixture
            .voice(0)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames;
        measured_pcm_into(&mut fixture, &mut actual[1..], &mut feedback);

        assert_matching_pcm(&actual, &expected);
        assert_native_source(&fixture, 0, &action);
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(
            fixture.mixer.pad_pitch_intent(0),
            Some(PitchIntent::new(0, 7).unwrap())
        );
        assert!(fixture.voice(0).stretch.wet_render_ready());
        assert!(fixture.voice(0).stretch.wet_current_source_output_ready());
        assert_eq!(
            fixture
                .voice(0)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            fed_at_adoption + 4095
        );
        assert!(
            fixture
                .voice(0)
                .source_playback
                .matches_exact(&reference.voice(0).source_playback)
        );
        assert_eq!(
            fixture.voice(0).stretch.pending_fifo_frames(),
            reference.voice(0).stretch.pending_fifo_frames()
        );
        assert!(messages.is_empty(), "SET must not emit a second start");
        assert_eq!(feedback.slots(), FEEDBACK_CAPACITY);
        wait_custody_released(&fixture);
        assert!(
            action.effective(),
            "keeping the receipt must not retain heavy work custody"
        );
    }
}

#[test]
fn prepared_set_source_refusal_keeps_live_pcm_and_allocates_no_rust_callback_storage() {
    for lock in [false, true] {
        let (mut fixture, mut feedback, messages) = started_fixture(lock, 3);
        let (mut reference, _reference_feedback, _reference_messages) = started_fixture(lock, 3);
        let action = fixture.capture(0, "pitch.extra:7:set");
        let original_due = action.original_due_frame();
        assert!(
            fixture
                .control
                .engine
                .publish_pitch_action(&action)
                .unwrap()
        );
        measured_callback(
            &mut fixture,
            &mut [],
            &mut feedback,
            "refused SET source capture",
        );
        assert!(action.accepted());
        wait_ready_with_measured_callbacks(&mut fixture, &action, &mut feedback);
        let execution = action.execution_target();
        measured_render_to(&mut fixture, execution, &mut feedback);
        reference.render_to(execution);

        let before = fixture.voice(0);
        let generation = before.generation;
        let native = before.stretch.native_state_address();
        let request = before.stretch.adopted_request_id();
        let cursor = before.source_playback;
        let fifo = before.stretch.pending_fifo_frames();
        let fed = before
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames;
        assert_ne!(native, 0);
        // Change the real source publication at E, retaining the old playing voice. Equal
        // PCM cannot grant the already prepared action the replacement's fresh authority.
        publish_tone(&fixture.control, 0, tone(SOURCE_HZ), false);
        measured_callback(
            &mut fixture,
            &mut [],
            &mut feedback,
            "prepared SET source refusal at E",
        );
        assert_eq!(action.state(), "rejected");
        assert_eq!(action.reason(), Some("source_changed"));
        assert!(!action.effective());
        assert_eq!(action.original_due_frame(), original_due);
        assert_eq!(action.execution_target(), execution);
        assert!(fixture.scheduler.is_empty());
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(fixture.voice(0).stretch.adopted_request_id(), request);
        assert!(fixture.voice(0).source_playback.matches_exact(&cursor));
        assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), fifo);
        assert_eq!(
            fixture
                .voice(0)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            fed
        );
        assert_eq!(
            fixture.mixer.pad_pitch_intent(0),
            Some(PitchIntent::new(0, 3).unwrap())
        );

        // Stay before any separately requested retained-voice preparation's future endpoint;
        // this is a direct continuity oracle for the rejected action's unchanged old Native.
        let expected = reference.pcm(1024);
        let mut actual = vec![0.0; expected.len()];
        measured_pcm_into(&mut fixture, &mut actual, &mut feedback);
        assert_matching_pcm(&actual, &expected);
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(fixture.voice(0).stretch.adopted_request_id(), request);
        assert!(
            fixture
                .voice(0)
                .source_playback
                .matches_exact(&reference.voice(0).source_playback)
        );
        assert_eq!(
            fixture.voice(0).stretch.pending_fifo_frames(),
            reference.voice(0).stretch.pending_fifo_frames()
        );
        assert!(
            messages.is_empty(),
            "refused SET must not emit another start"
        );
        assert_eq!(feedback.slots(), FEEDBACK_CAPACITY);
        wait_custody_released(&fixture);
        assert_eq!(action.state(), "rejected");
    }
}
