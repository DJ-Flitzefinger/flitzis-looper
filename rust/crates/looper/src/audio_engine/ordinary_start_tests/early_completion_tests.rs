//! Deterministic completion publication, full physical PCM and real callback custody.

use super::preparation_deadline_tests::WorkerGate;
use super::*;
use crate::audio_engine::audio_stream::render_scheduled_audio;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::ordinary_endpoint_reserve_tests::{
    adopt_current_source_ids, adopt_current_sources, refresh_actual_runtime_bindings,
};
use crate::audio_engine::pitch_runtime_tests::physical_birth_tone_reference;
use crate::audio_engine::scheduler::ScheduledCommand;
use crate::test_memory::{ThreadAllocations, observe_current_thread};
use pyo3::prelude::*;
use rtrb::RingBuffer;

fn configured(ratio: f64) -> Fixture {
    let mut fixture = Fixture::new(true);
    adopt_current_sources(&mut fixture, true);
    refresh_actual_runtime_bindings(&fixture);
    fixture.mixer.set_bpm_lock(true);
    fixture.mixer.set_master_bpm(120.0);
    fixture.mixer.set_pad_bpm(0, Some(120.0 / ratio));
    fixture.mixer.set_pad_bpm(1, Some(120.0));
    fixture
}

fn publish(fixture: &mut Fixture, start: &CapturedPadStart) {
    fixture.drain();
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(start, None)
            .unwrap()
    );
    fixture.drain();
    assert!(start.accepted(), "{:?}", start.reason());
}

#[test]
fn early_completion_waits_for_real_worker_then_keeps_the_entire_wet_birth_without_rt_allocations() {
    for ratio in [0.8, 1.16] {
        let mut fixture = configured(ratio);
        let start = capture(&fixture, 1.0);
        let gate = WorkerGate::new(&fixture, &start);
        publish(&mut fixture, &start);
        assert_eq!(start.execution_frame(), Some(8192));
        gate.wait_for(false);
        fixture
            .scheduler
            .schedule(128, ScheduledCommand::StopSample { id: 1 })
            .unwrap();
        assert!(fixture.render(512).iter().all(|sample| *sample == 0.0));
        assert!(fixture.scheduler.deferred_due().is_none());
        assert_eq!(
            fixture.scheduler.len(),
            1,
            "unrelated scheduled STOP progressed"
        );
        assert_eq!(start.effective_frame(), None);
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        wait_ready(&mut fixture, &[start.sequence()]);
        assert_eq!(start.execution_frame(), Some(8192));

        let (mut feedback, mut messages) = RingBuffer::new(64);
        let ((), allocations) = observe_current_thread(|| {
            render_scheduled_audio(
                &mut fixture.mixer,
                &mut fixture.scheduler,
                &mut [],
                &mut [0.0; NUM_SAMPLES],
                512,
                1,
                &mut fixture.transport,
                &mut feedback,
                &mut fixture.retirement,
            )
        });
        assert_eq!(allocations, ThreadAllocations::default());
        assert_eq!(start.original_frame(), Some(0));
        assert_eq!(start.execution_frame(), Some(512));
        assert_eq!(start.effective_frame(), Some(512));
        assert!(messages.pop().is_ok());
        let pcm = fixture.pcm(24_000);
        assert_physical_birth_tone(&pcm, 0, 0, ratio, true, 1.0);
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(start.sequence())
        );
        assert_eq!(
            fixture
                .voice(0)
                .source_playback
                .diagnostic_launch_reference()
                .unwrap()
                .1,
            0
        );
    }
}

#[test]
fn early_completion_keeps_four_rapid_originals_distinct_and_in_order() {
    let mut fixture = configured(0.8);
    let first = capture(&fixture, 1.0);
    let gate = WorkerGate::new(&fixture, &first);
    publish(&mut fixture, &first);
    gate.wait_for(false);
    let mut starts = vec![first];
    for frame in [16, 32, 48] {
        fixture.render_to(frame);
        fixture.refresh_clock();
        let start = capture(&fixture, 1.0);
        publish(&mut fixture, &start);
        assert_eq!(start.original_frame(), Some(frame));
        starts.push(start);
    }
    fixture.render_to(64);
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_ready(
        &mut fixture,
        &starts
            .iter()
            .map(CapturedPadStart::sequence)
            .collect::<Vec<_>>(),
    );
    for (index, start) in starts.iter().enumerate() {
        fixture.render(16);
        let execution = 64 + index as u64 * 16;
        assert_eq!(
            start.effective_frame(),
            Some(execution),
            "{:?}",
            start.reason()
        );
        assert_eq!(fixture.starts(0), index + 1);
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(start.sequence())
        );
    }
    assert!(
        starts
            .windows(2)
            .all(|pair| pair[0].sequence() < pair[1].sequence())
    );
}

#[test]
fn cancelled_ready_predecessor_releases_only_ordering_and_keeps_its_retirement_custody() {
    for ratio in [0.8, 1.16] {
        for gap in [0, 16] {
            let mut fixture = configured(ratio);
            let first = capture(&fixture, 1.0);
            publish(&mut fixture, &first);
            wait_ready(&mut fixture, &[first.sequence()]);
            assert!(first.cancel());
            assert!(fixture.render(gap).iter().all(|sample| *sample == 0.0));
            fixture.refresh_clock();
            let next = capture(&fixture, 1.0);
            publish(&mut fixture, &next);
            wait_ready(&mut fixture, &[next.sequence()]);
            assert!(first.sequence() < next.sequence());
            assert_eq!(next.original_frame(), Some(gap as u64));
            let (mut feedback, mut messages) = RingBuffer::new(64);
            let ((), allocations) = observe_current_thread(|| {
                render_scheduled_audio(
                    &mut fixture.mixer,
                    &mut fixture.scheduler,
                    &mut [],
                    &mut [0.0; NUM_SAMPLES],
                    gap as u64,
                    1,
                    &mut fixture.transport,
                    &mut feedback,
                    &mut fixture.retirement,
                )
            });
            assert_eq!(allocations, ThreadAllocations::default());
            while let Ok(message) = messages.pop() {
                fixture.feedback.push(message);
            }
            assert_eq!(next.effective_frame(), Some(gap as u64));
            assert_eq!(first.execution_frame(), Some(8192));
            assert_eq!(first.effective_frame(), None);
            assert_eq!(first.reason(), Some("launch_cancelled"));
            assert_eq!(fixture.scheduler.len(), 1, "old event retains its capacity");
            assert_eq!(
                fixture
                    .mixer
                    .pitch_runtime
                    .as_ref()
                    .unwrap()
                    .unused_action_capacity()
                    .0,
                63
            );
            assert!(
                fixture
                    .mixer
                    .pitch_runtime
                    .as_ref()
                    .unwrap()
                    .has_ready_action(first.sequence())
            );
            assert_eq!(
                fixture.voice(0).stretch.adopted_request_id(),
                Some(next.sequence())
            );
            let native = fixture.voice(0).stretch.native_state_address();
            let pcm = fixture.pcm(24_000);
            assert_physical_birth_tone(&pcm, 0, 0, ratio, true, 1.0);
            assert_eq!(fixture.starts(0), 1, "old queued cleanup cannot retrigger");
            assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
            assert_eq!(
                fixture
                    .mixer
                    .pitch_runtime
                    .as_ref()
                    .unwrap()
                    .unused_action_capacity()
                    .0,
                64
            );
            assert_eq!(fixture.scheduler.len(), 0);
        }
    }
}

fn capture_playback_group(
    fixture: &Fixture,
    count: usize,
) -> crate::audio_engine::captured_global_start::CapturedGlobalStart {
    Python::attach(|py| {
        let entries = (0..count)
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
    })
}

fn playback_group(fixture: &mut Fixture, count: usize) -> Vec<CapturedPadStart> {
    let captured = capture_playback_group(fixture, count);
    fixture.drain();
    let starts = captured
        .resolve()
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|member| member.start)
        .collect::<Vec<_>>();
    assert!(
        fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
            .is_some()
    );
    fixture.drain();
    starts
}

fn group_fixture(count: usize, ratio: f64) -> Fixture {
    let mut fixture = configured(ratio);
    let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    for id in 1..count {
        publish_tone(&fixture.control, id, sample.clone(), true);
    }
    adopt_current_source_ids(&mut fixture, true, &(0..count).collect::<Vec<_>>());
    refresh_actual_runtime_bindings(&fixture);
    for id in 0..count {
        fixture.mixer.set_pad_loop_region(id, 0.1, Some(2.1));
        fixture.mixer.set_pad_bpm(id, Some(120.0 / ratio));
    }
    fixture
}

#[test]
fn rejected_group_peer_allows_other_peer_and_whole_group_without_releasing_old_custody() {
    for count in [2, 4, 6] {
        for ratio in [0.8, 1.16] {
            for whole_group in [false, true] {
                let mut fixture = group_fixture(count, ratio);
                let first = playback_group(&mut fixture, count);
                wait_ready(
                    &mut fixture,
                    &first
                        .iter()
                        .map(CapturedPadStart::sequence)
                        .collect::<Vec<_>>(),
                );
                assert!(first[count - 1].cancel());
                assert_ne!(first[0].state(), "rejected");
                let ((), allocations) =
                    observe_current_thread(|| fixture.mixer.pitch_runtime.as_mut().unwrap().poll());
                assert_eq!(allocations, ThreadAllocations::default());
                assert!(first.iter().all(|start| start.state() == "rejected"));
                assert_eq!(fixture.scheduler.len(), 1);
                let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap();
                assert_eq!(runtime.unused_action_capacity().0, 64 - count);
                assert!(
                    first
                        .iter()
                        .all(|start| runtime.has_ready_action(start.sequence()))
                );
                assert!(fixture.render(16).iter().all(|sample| *sample == 0.0));
                fixture.refresh_clock();
                let next = if whole_group {
                    playback_group(&mut fixture, count)
                } else {
                    let start = capture(&fixture, 1.0);
                    publish(&mut fixture, &start);
                    vec![start]
                };
                wait_ready(
                    &mut fixture,
                    &next
                        .iter()
                        .map(CapturedPadStart::sequence)
                        .collect::<Vec<_>>(),
                );
                let (mut feedback, mut messages) = RingBuffer::new(64);
                let ((), allocations) = observe_current_thread(|| {
                    render_scheduled_audio(
                        &mut fixture.mixer,
                        &mut fixture.scheduler,
                        &mut [],
                        &mut [0.0; NUM_SAMPLES],
                        16,
                        1,
                        &mut fixture.transport,
                        &mut feedback,
                        &mut fixture.retirement,
                    )
                });
                assert_eq!(allocations, ThreadAllocations::default());
                while let Ok(message) = messages.pop() {
                    fixture.feedback.push(message);
                }
                assert!(next.iter().all(|start| start.original_frame() == Some(16)
                    && start.effective_frame() == Some(16)));
                assert!(
                    first
                        .iter()
                        .all(|start| start.execution_frame() == Some(8192)
                            && start.effective_frame().is_none())
                );
                assert_eq!(fixture.scheduler.len(), 1);
                assert_eq!(
                    fixture
                        .mixer
                        .pitch_runtime
                        .as_ref()
                        .unwrap()
                        .unused_action_capacity()
                        .0,
                    64 - count
                );
                let natives = (0..next.len())
                    .map(|id| fixture.voice(id).stretch.native_state_address())
                    .collect::<Vec<_>>();
                assert!(natives.iter().all(|address| *address != 0));
                for (id, start) in next.iter().enumerate() {
                    assert_eq!(
                        fixture.voice(id).stretch.adopted_request_id(),
                        Some(start.sequence())
                    );
                    assert_eq!(
                        fixture.voice(id).sample.as_ref().unwrap().source_address(),
                        start.callback_guard().unwrap().binding.source_address
                    );
                    assert!(!natives[..id].contains(&natives[id]));
                }
                let pcm = fixture.pcm(24_000);
                let reference =
                    physical_birth_tone_reference(pcm.len(), SOURCE_HZ, 0, 0, ratio, true);
                assert!(
                    pcm.iter()
                        .zip(reference)
                        .all(|(a, b)| (*a - b * next.len() as f32).abs() < 1.0e-4)
                );
                assert!((0..count).all(|id| fixture.starts(id) == usize::from(id < next.len())));
                assert_eq!(
                    fixture
                        .mixer
                        .pitch_runtime
                        .as_ref()
                        .unwrap()
                        .unused_action_capacity()
                        .0,
                    64
                );
                assert_eq!(fixture.scheduler.len(), 0);
                assert_eq!(
                    (0..next.len())
                        .map(|id| fixture.voice(id).stretch.native_state_address())
                        .collect::<Vec<_>>(),
                    natives
                );
            }
        }
    }
}

#[test]
fn rejected_group_peer_keeps_held_worker_while_independent_piano_effects() {
    for count in [2, 4, 6] {
        let mut fixture = group_fixture(count, 1.0);
        fixture
            .mixer
            .set_pad_pitch_intent(0, PitchIntent::new(0, 3).unwrap())
            .unwrap();
        let captured = capture_playback_group(&fixture, count);
        fixture.drain();
        let first = captured
            .resolve()
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|member| member.start)
            .collect::<Vec<_>>();
        let gate = WorkerGate::for_ordinary(&first[0]);
        assert!(
            fixture
                .control
                .engine
                .publish_captured_global_start(&captured)
                .unwrap()
                .is_some()
        );
        fixture.drain();
        gate.wait_for(false);
        assert!(first[count - 1].cancel());
        let next = fixture.capture(0, "pitch.extra:0:set_retrigger");
        fixture.publish(&next);
        fixture.render(0);
        assert!(next.effective(), "{:?}", next.reason());
        assert!(
            first
                .iter()
                .all(|start| start.state() == "rejected" && start.effective_frame().is_none())
        );
        let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap();
        assert_eq!(runtime.unused_action_capacity().0, 64 - count);
        assert!(!runtime.has_ready_action(first[0].sequence()));
        assert_eq!(fixture.scheduler.len(), 1);
        gate.release();
        assert!(gate.wait_for(true).is_some());
        let sample = fixture.voice(0).sample.as_ref().unwrap();
        assert_eq!(sample.resident_start(), LOOP_START);
        let source = sample.samples.clone();
        let pcm = fixture.pcm(24_000);
        // Unity bypass is dry: compare its entire beginning directly to source,
        // not to an unnecessary RawRubberBand unity instance and its startup.
        assert_eq!(pcm, source[..24_000]);
        assert_eq!(fixture.starts(0), 1);
        assert!((1..count).all(|id| fixture.starts(id) == 0));
        assert_eq!(
            fixture
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .unused_action_capacity()
                .0,
            64
        );
    }
}

#[test]
fn cancelled_single_and_group_member_predecessors_keep_later_playback_groups_atomic() {
    for count in [2, 4, 6] {
        for older_group in [false, true] {
            let mut fixture = configured(0.8);
            let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
                .clone()
                .unwrap();
            for id in 1..count {
                publish_tone(&fixture.control, id, sample.clone(), true);
            }
            adopt_current_source_ids(&mut fixture, true, &(0..count).collect::<Vec<_>>());
            refresh_actual_runtime_bindings(&fixture);
            for id in 0..count {
                fixture.mixer.set_pad_loop_region(id, 0.1, Some(2.1));
                fixture.mixer.set_pad_bpm(id, Some(150.0));
            }
            let first = if older_group {
                playback_group(&mut fixture, count)
            } else {
                let start = capture(&fixture, 1.0);
                publish(&mut fixture, &start);
                vec![start]
            };
            wait_ready(
                &mut fixture,
                &first
                    .iter()
                    .map(CapturedPadStart::sequence)
                    .collect::<Vec<_>>(),
            );
            assert!(first[0].cancel());
            assert!(fixture.render(16).iter().all(|sample| *sample == 0.0));
            fixture.refresh_clock();
            let next = if older_group {
                let start = capture(&fixture, 1.0);
                publish(&mut fixture, &start);
                vec![start]
            } else {
                playback_group(&mut fixture, count)
            };
            wait_ready(
                &mut fixture,
                &next
                    .iter()
                    .map(CapturedPadStart::sequence)
                    .collect::<Vec<_>>(),
            );
            fixture.render(0);
            assert!(next.iter().all(|start| start.effective_frame() == Some(16)));
            assert!(first.iter().all(|start| start.effective_frame().is_none()));
            assert_eq!(fixture.scheduler.len(), 1, "older event remains queued");
            let pcm = fixture.pcm(24_000);
            let reference = physical_birth_tone_reference(pcm.len(), SOURCE_HZ, 0, 0, 0.8, true);
            assert!(
                pcm.iter()
                    .zip(reference)
                    .all(|(a, b)| (*a - b * next.len() as f32).abs() < 1.0e-4)
            );
            assert!(first.iter().all(|start| start.state() == "rejected"));
            assert!((0..count).all(|id| fixture.starts(id) == usize::from(id < next.len())));
            assert_eq!(
                fixture
                    .mixer
                    .pitch_runtime
                    .as_ref()
                    .unwrap()
                    .unused_action_capacity()
                    .0,
                64
            );
        }
    }
}

#[test]
fn early_completion_multiwet_groups_share_one_effect_and_preserve_all_physical_pcm() {
    for (count, wet) in [(2, 2), (4, 2), (6, 6)] {
        let mut fixture = configured(0.8);
        let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        for id in 1..count {
            publish_tone(&fixture.control, id, sample.clone(), true);
        }
        adopt_current_source_ids(&mut fixture, true, &(0..count).collect::<Vec<_>>());
        refresh_actual_runtime_bindings(&fixture);
        for id in 0..count {
            fixture.mixer.set_pad_loop_region(id, 0.1, Some(2.1));
            fixture
                .mixer
                .set_pad_bpm(id, Some(if id < wet { 150.0 } else { 120.0 }));
        }
        let captured = Python::attach(|py| {
            let entries = (0..count)
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
        fixture.drain();
        let starts = captured
            .resolve()
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|member| member.start)
            .collect::<Vec<_>>();
        assert!(
            fixture
                .control
                .engine
                .publish_captured_global_start(&captured)
                .unwrap()
                .is_some()
        );
        fixture.drain();
        assert!(
            starts
                .iter()
                .all(|start| start.execution_frame() == Some(8192))
        );
        wait_ready(
            &mut fixture,
            &starts
                .iter()
                .map(CapturedPadStart::sequence)
                .collect::<Vec<_>>(),
        );
        let (mut feedback, mut messages) = RingBuffer::new(64);
        let ((), allocations) = observe_current_thread(|| {
            render_scheduled_audio(
                &mut fixture.mixer,
                &mut fixture.scheduler,
                &mut [],
                &mut [0.0; NUM_SAMPLES],
                0,
                1,
                &mut fixture.transport,
                &mut feedback,
                &mut fixture.retirement,
            )
        });
        assert_eq!(
            allocations,
            ThreadAllocations::default(),
            "group {count}/{wet}"
        );
        while let Ok(message) = messages.pop() {
            fixture.feedback.push(message);
        }
        assert!(
            starts
                .iter()
                .all(|start| start.effective_frame() == Some(0))
        );
        let addresses = (0..wet)
            .map(|id| fixture.voice(id).stretch.native_state_address())
            .collect::<Vec<_>>();
        for (index, address) in addresses.iter().enumerate() {
            assert_ne!(*address, 0);
            assert!(!addresses[..index].contains(address));
        }
        let pcm = fixture.pcm(16_384);
        let reference = physical_birth_tone_reference(pcm.len(), SOURCE_HZ, 0, 0, 0.8, true);
        assert!(
            pcm.iter()
                .zip(reference)
                .enumerate()
                .all(|(frame, (actual, expected))| {
                    let dry = (0.25
                        * ((LOOP_START + frame % (LOOP_END - LOOP_START)) as f64
                            * SOURCE_HZ
                            * std::f64::consts::TAU
                            / f64::from(RATE))
                        .sin()) as f32;
                    (*actual - (expected * wet as f32 + dry * (count - wet) as f32)).abs() < 1.0e-4
                })
        );
        assert!((0..count).all(|id| fixture.starts(id) == 1));
    }
}

#[test]
fn early_completion_preserves_future_quantized_target_and_refuses_stale_control_envelopes() {
    let mut future = configured(0.8);
    future.render_to(1_000);
    future.refresh_clock();
    let start = capture(&future, 1.0);
    drain_with_mode(&mut future, TriggerQuantization::Grid { step_64ths: 16 });
    assert!(
        future
            .control
            .engine
            .publish_pad_start(&start, None)
            .unwrap()
    );
    drain_with_mode(&mut future, TriggerQuantization::Grid { step_64ths: 16 });
    wait_ready(&mut future, &[start.sequence()]);
    assert!(future.render_to(24_000).iter().all(|sample| *sample == 0.0));
    future.render(0);
    assert_eq!(start.original_frame(), Some(24_000));
    assert_eq!(start.effective_frame(), Some(24_000));

    for fence in ["stop", "source", "rate_aba", "gain"] {
        let mut fixture = configured(0.8);
        let start = capture(&fixture, 1.0);
        publish(&mut fixture, &start);
        wait_ready(&mut fixture, &[start.sequence()]);
        match fence {
            "stop" => fixture.control.engine.stop_sample(0).unwrap(),
            "source" => {
                let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
                    .clone()
                    .unwrap();
                publish_tone(&fixture.control, 0, sample, true);
            }
            "rate_aba" => {
                fixture.mixer.set_pad_bpm(0, Some(160.0));
                fixture.mixer.set_pad_bpm(0, Some(150.0));
            }
            _ => fixture.mixer.set_pad_gain(0, -6.0),
        }
        fixture.drain();
        fixture.render(1);
        assert_eq!(start.effective_frame(), None, "{fence}");
        fixture.render_to(8192);
        fixture.render(0);
        assert_eq!(start.state(), "rejected", "{fence} {:?}", start.reason());
        assert_eq!(fixture.starts(0), 0);
    }
}

#[test]
fn early_completion_feedback_pressure_rearms_only_after_real_legacy_reprepare() {
    let mut fixture = configured(0.8);
    let start = capture(&fixture, 1.0);
    publish(&mut fixture, &start);
    wait_ready(&mut fixture, &[start.sequence()]);
    for cycle in 1..=2 {
        let (mut feedback, mut messages) = RingBuffer::new(1);
        feedback
            .push(AudioMessage::SampleStarted { id: 1 })
            .unwrap();
        render_scheduled_audio(
            &mut fixture.mixer,
            &mut fixture.scheduler,
            &mut [0.0],
            &mut [0.0; NUM_SAMPLES],
            cycle - 1,
            1,
            &mut fixture.transport,
            &mut feedback,
            &mut fixture.retirement,
        );
        fixture.transport.advance_by_rendered_frames(1);
        assert_eq!(start.execution_frame(), Some(cycle - 1));
        assert_eq!(start.effective_frame(), None);
        assert!(fixture.scheduler.deferred_due().is_some());
        messages.pop().unwrap();
        fixture.render(0);
        assert_eq!(start.execution_frame(), Some(8192 + cycle));
        wait_ready(&mut fixture, &[start.sequence()]);
    }
    let pcm = complete_attack(&mut fixture, 2);
    assert_eq!(start.effective_frame(), Some(2));
    assert_physical_birth_tone(&pcm, 0, 0, 0.8, true, 1.0);
}

#[test]
fn returned_legacy_completion_refuses_cancelled_or_changed_ordinary_authority() {
    for fence in ["cancel", "stop", "source", "rate_aba", "gain"] {
        let mut fixture = configured(0.8);
        let start = capture(&fixture, 1.0);
        let gate = WorkerGate::new(&fixture, &start);
        publish(&mut fixture, &start);
        gate.wait_for(false);
        assert!(fixture.render_to(8193).iter().all(|value| *value == 0.0));
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        wait_ready(&mut fixture, &[start.sequence()]);
        fixture.render(0);
        assert_eq!(start.execution_frame(), Some(16385));
        wait_ready(&mut fixture, &[start.sequence()]);
        match fence {
            "cancel" => assert!(start.cancel()),
            "stop" => fixture.control.engine.stop_sample(0).unwrap(),
            "source" => {
                let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
                    .clone()
                    .unwrap();
                publish_tone(&fixture.control, 0, sample, true);
            }
            "rate_aba" => {
                fixture.mixer.set_pad_bpm(0, Some(160.0));
                fixture.mixer.set_pad_bpm(0, Some(150.0));
            }
            _ => fixture.mixer.set_pad_gain(0, -6.0),
        }
        fixture.drain();
        assert!(fixture.render_to(16386).iter().all(|value| *value == 0.0));
        assert_eq!(start.state(), "rejected", "{fence}: {:?}", start.reason());
        assert_eq!(start.effective_frame(), None);
        assert_eq!(fixture.starts(0), 0);
    }
}

#[test]
fn late_completion_requires_a_second_genuine_worker_and_keeps_full_pcm_and_native_custody() {
    for ratio in [0.8, 1.16] {
        let mut fixture = configured(ratio);
        let start = capture(&fixture, 1.0);
        let gate = WorkerGate::new(&fixture, &start);
        publish(&mut fixture, &start);
        gate.wait_for(false);
        assert!(fixture.render_to(8193).iter().all(|sample| *sample == 0.0));
        assert!(fixture.scheduler.deferred_due().is_some());
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        wait_ready(&mut fixture, &[start.sequence()]);
        let native = gate.prepared_native();
        gate.arm_next_attempt();
        fixture.render(0);
        assert_eq!(start.execution_frame(), Some(16385));
        gate.wait_for(false);
        fixture
            .scheduler
            .schedule(8300, ScheduledCommand::StopSample { id: 1 })
            .unwrap();
        assert!(fixture.render(512).iter().all(|sample| *sample == 0.0));
        assert!(fixture.scheduler.deferred_due().is_none());
        assert_eq!(fixture.scheduler.len(), 1);
        assert_eq!(start.execution_frame(), Some(16385));
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        assert_eq!(
            gate.prepared_native(),
            native,
            "the genuine worker reset reuses its Native"
        );
        wait_ready(&mut fixture, &[start.sequence()]);
        let (mut feedback, mut messages) = RingBuffer::new(64);
        let ((), allocations) = observe_current_thread(|| {
            render_scheduled_audio(
                &mut fixture.mixer,
                &mut fixture.scheduler,
                &mut [],
                &mut [0.0; NUM_SAMPLES],
                8705,
                1,
                &mut fixture.transport,
                &mut feedback,
                &mut fixture.retirement,
            );
        });
        assert_eq!(allocations, ThreadAllocations::default());
        while let Ok(message) = messages.pop() {
            fixture.feedback.push(message);
        }
        assert_eq!(start.original_frame(), Some(0));
        assert_eq!(start.effective_frame(), Some(8705));
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(start.sequence())
        );
        let pcm = fixture.pcm(24_000);
        assert_physical_birth_tone(&pcm, 0, 0, ratio, true, 1.0);
        assert_eq!(fixture.starts(0), 1);
    }
}
