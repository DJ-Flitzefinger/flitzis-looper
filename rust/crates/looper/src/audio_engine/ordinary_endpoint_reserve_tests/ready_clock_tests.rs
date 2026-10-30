//! Physical readiness is independent of a future performer's launch coordinates.

use super::*;
use crate::audio_engine::captured_global_start::CapturedGlobalStart;
use pyo3::prelude::*;

fn rate_fixture(ratio: f64) -> Fixture {
    let mut fixture = fixture();
    fixture.mixer.set_bpm_lock(true);
    fixture.mixer.set_master_bpm(120.0);
    fixture.mixer.set_pad_bpm(0, Some(120.0 / ratio));
    fixture.mixer.set_pad_bpm(1, Some(120.0));
    fixture
}

#[test]
fn ready_active_master_clock_keeps_fresh_launch_metadata_and_complete_source_beginning() {
    for ratio in [0.8, 1.16] {
        for late in [false, true] {
            let mut hit = rate_fixture(ratio);
            let mut worker = rate_fixture(ratio);
            let native = select_and_wait(&mut hit);
            let original = capture(&hit, LOOP_START, LOOP_END, 1.0);
            hit.drain();
            if late {
                assert!(hit.render(512).iter().all(|value| *value == 0.0));
            }
            publish(&mut hit, &original);
            let execution = if late { 512 } else { 0 };
            assert_eq!(original.original_frame(), Some(0));
            assert_eq!(original.execution_frame(), Some(execution));
            let expected = capture(&worker, LOOP_START, LOOP_END, 1.0);
            worker.drain();
            publish(&mut worker, &expected);
            wait_action(&mut worker, &expected);
            let pcm = complete_pcm(&mut hit, execution);
            let reference = complete_pcm(&mut worker, expected.execution_frame().unwrap());
            assert_eq!(
                pcm, reference,
                "rate={ratio} late={late}: full physical prefix"
            );
            assert_eq!(hit.voice(0).stretch.native_state_address(), native);
            assert_eq!(
                hit.voice(0).stretch.adopted_request_id(),
                Some(original.sequence())
            );
            assert_eq!(original.effective_frame(), Some(execution));
            assert_eq!(
                hit.voice(0)
                    .source_playback
                    .diagnostic_launch_reference()
                    .unwrap()
                    .1,
                0
            );
            assert_eq!(hit.voice(0).launch_qualified, !late);
        }
    }
}

fn group(fixture: &mut Fixture) -> (CapturedGlobalStart, Vec<CapturedPadStart>) {
    group_with_mode(fixture, TriggerQuantization::Immediate)
}

fn group_with_mode(
    fixture: &mut Fixture,
    mut mode: TriggerQuantization,
) -> (CapturedGlobalStart, Vec<CapturedPadStart>) {
    let captured = Python::attach(|py| {
        let entries = [0, 1]
            .into_iter()
            .map(|id| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                (Py::new(py, binding).unwrap(), 0.1, Some(2.1), None)
            })
            .collect();
        fixture
            .control
            .engine
            .capture_global_start(py, entries, None)
            .unwrap()
    });
    drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        fixture.transport.output_frame(),
        &mut mode,
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    let starts = captured
        .resolve()
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|member| member.start)
        .collect();
    (captured, starts)
}

fn publish_group(fixture: &mut Fixture, captured: &CapturedGlobalStart) {
    assert!(
        fixture
            .control
            .engine
            .publish_captured_global_start(captured)
            .unwrap()
            .is_some()
    );
    fixture.drain();
}

#[test]
fn one_ready_wet_member_makes_the_whole_group_immediate_without_losing_either_beginning() {
    for late in [false, true] {
        let mut hit = rate_fixture(0.8);
        let mut worker = rate_fixture(0.8);
        let native = select_and_wait(&mut hit);
        let (captured, starts) = group(&mut hit);
        if late {
            assert!(hit.render(512).iter().all(|value| *value == 0.0));
        }
        publish_group(&mut hit, &captured);
        let execution = if late { 512 } else { 0 };
        assert!(
            starts.iter().all(|start| start.original_frame() == Some(0)
                && start.execution_frame() == Some(execution))
        );
        assert_eq!((hit.starts(0), hit.starts(1)), (1, 1));
        assert!(hit.voice(0).launch_unadvanced && hit.voice(1).launch_unadvanced);
        let (control, control_starts) = group(&mut worker);
        publish_group(&mut worker, &control);
        super::super::ordinary_start_tests::wait_ready(
            &mut worker,
            &control_starts
                .iter()
                .map(CapturedPadStart::sequence)
                .collect::<Vec<_>>(),
        );
        let pcm = complete_pcm(&mut hit, execution);
        let reference = complete_pcm(&mut worker, control_starts[0].execution_frame().unwrap());
        assert_eq!(
            pcm, reference,
            "late={late}: complete wet and dry mixed beginning"
        );
        assert!(
            starts
                .iter()
                .all(|start| start.effective_frame() == Some(execution))
        );
        assert_eq!((hit.starts(0), hit.starts(1)), (1, 1));
        assert_eq!(hit.voice(0).stretch.native_state_address(), native);
        assert_ne!(
            hit.voice(0).stretch.native_state_address(),
            hit.voice(1).stretch.native_state_address()
        );
        assert_eq!(hit.voice(1).source_playback.tempo_ratio(), 1.0);
    }
}

#[test]
fn second_wet_member_requires_own_worker_and_keeps_the_selected_reserve_unconsumed() {
    let mut fixture = rate_fixture(0.8);
    fixture.mixer.set_pad_bpm(1, Some(150.0));
    let native = select_and_wait(&mut fixture);
    let (captured, starts) = group(&mut fixture);
    publish_group(&mut fixture, &captured);
    assert!(
        starts
            .iter()
            .all(|start| start.execution_frame() == Some(8192))
    );
    assert_eq!(
        fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .selected_endpoint_native_address(),
        native
    );
    assert!(
        fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .selected_endpoint_ready()
    );
    assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));
    super::super::ordinary_start_tests::wait_ready(
        &mut fixture,
        &starts
            .iter()
            .map(CapturedPadStart::sequence)
            .collect::<Vec<_>>(),
    );
    complete_pcm(&mut fixture, 8192);
    assert!(
        starts
            .iter()
            .all(|start| start.effective_frame() == Some(8192))
    );
    assert_ne!(fixture.voice(0).stretch.native_state_address(), native);
    assert_ne!(
        fixture.voice(0).stretch.native_state_address(),
        fixture.voice(1).stretch.native_state_address()
    );
}

#[test]
fn ready_group_keeps_future_grid_and_rejects_all_members_after_source_or_stop_fence() {
    for fence in ["none", "source", "stop"] {
        let mut fixture = rate_fixture(0.8);
        let native = select_and_wait(&mut fixture);
        fixture.render_to(1_000);
        let (captured, starts) =
            group_with_mode(&mut fixture, TriggerQuantization::Grid { step_64ths: 16 });
        publish_group(&mut fixture, &captured);
        assert!(
            starts
                .iter()
                .all(|start| start.original_frame() == Some(24_000)
                    && start.execution_frame() == Some(24_000))
        );
        assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));
        match fence {
            "source" => {
                // Publish a replacement of the dry peer while the selected wet
                // candidate is already owned by this still-inaudible group.
                let sample = fixture.control.engine.sample_cache.lock().unwrap()[1]
                    .clone()
                    .unwrap();
                super::super::pitch_runtime_tests::publish_tone(&fixture.control, 1, sample, true);
            }
            "stop" => fixture.control.engine.stop_sample(1).unwrap(),
            _ => {}
        }
        fixture.drain();
        let before = fixture.render_to(24_000);
        assert!(before.iter().all(|value| *value == 0.0));
        fixture.render(1);
        if fence == "none" {
            assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
            assert!(
                starts
                    .iter()
                    .all(|start| start.effective_frame() == Some(24_000))
            );
            assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        } else {
            assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));
            assert!(starts.iter().all(|start| start.state() == "rejected"));
            assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
        }
    }
}

#[test]
fn quantization_off_keeps_distinct_wet_retriggers_faster_than_sixty_fourth_notes() {
    let mut fixture = rate_fixture(0.8);
    select_and_wait(&mut fixture);
    let mut starts = Vec::new();
    for original in [0, 16, 32, 48] {
        assert_eq!(fixture.transport.output_frame(), original);
        let start = capture(&fixture, LOOP_START, LOOP_END, 1.0);
        fixture.drain();
        publish(&mut fixture, &start);
        assert_eq!(start.original_frame(), Some(original));
        assert!(start.accepted());
        starts.push(start);
        fixture.render(16);
    }
    // At 120 BPM a 1/64 note spans 1500 output frames. Each original T here
    // remains immediate, rather than being rounded to that musical interval.
    assert!(
        starts
            .windows(2)
            .all(|pair| pair[0].sequence() != pair[1].sequence())
    );
    assert_eq!(starts[0].effective_frame(), Some(0));
    for start in starts.iter().skip(1) {
        if start.effective_frame().is_none() {
            wait_action(&mut fixture, start);
            fixture.render_to(start.execution_frame().unwrap());
            fixture.render(1);
        }
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        assert_eq!(start.effective_frame(), start.execution_frame());
    }
    assert_eq!(fixture.starts(0), 4);
}
