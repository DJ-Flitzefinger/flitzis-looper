//! Real retained DeferredEvents, complete physical PCM and strict refusal bounds.

use super::*;
use crate::audio_engine::captured_global_start::CapturedGlobalStart;
use crate::audio_engine::ordinary_endpoint_reserve_tests::{
    adopt_current_source_ids, refresh_actual_runtime_bindings,
};
use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::{
    WorkerGate, with_unrelated_marker,
};
use crate::audio_engine::pad_start::CapturedPadStart;
use crate::audio_engine::pitch_runtime_tests::{
    Fixture, SOURCE_HZ, physical_birth_tone_reference, publish_tone,
};
use crate::test_memory::{ThreadAllocations, observe_current_thread};
use pyo3::prelude::*;
use std::cell::RefCell;
use std::time::{Duration, Instant};

thread_local! {
    static BIND_HOOK: RefCell<Option<(usize, Box<dyn FnOnce()>)>> = RefCell::new(None);
}

pub(super) fn before_bind(member: usize) {
    let hook = BIND_HOOK.with(|hook| {
        let mut hook = hook.borrow_mut();
        if hook.as_ref().is_some_and(|(index, _)| *index == member) {
            hook.take().map(|(_, hook)| hook)
        } else {
            None
        }
    });
    if let Some(hook) = hook {
        hook();
    }
}

fn deferred(
    count: usize,
    wet: usize,
    ratio: f64,
) -> (
    Fixture,
    CapturedGlobalStart,
    Vec<CapturedPadStart>,
    Vec<usize>,
) {
    let mut fixture = Fixture::new(true);
    let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    for id in 0..count {
        publish_tone(&fixture.control, id, sample.clone(), true);
    }
    adopt_current_source_ids(&mut fixture, true, &(0..count).collect::<Vec<_>>());
    refresh_actual_runtime_bindings(&fixture);
    fixture.mixer.set_bpm_lock(true);
    fixture.mixer.set_master_bpm(120.0);
    for id in 0..count {
        fixture.mixer.set_pad_loop_region(id, 0.1, Some(2.1));
        fixture
            .mixer
            .set_pad_bpm(id, Some(if id < wet { 120.0 / ratio } else { 120.0 }));
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
    let gate = WorkerGate::for_ordinary(&starts[0]);
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
    assert!(fixture.render_to(8192).iter().all(|sample| *sample == 0.0));
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.ordinary_group_ready(count) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    with_unrelated_marker(&mut fixture, |fixture| {
        assert_eq!(fixture.render(1), vec![0.0]);
        assert!(fixture.scheduler.deferred_due().is_some());
    });
    let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap();
    let group = runtime
        .slots
        .iter()
        .find_map(|slot| slot.group.as_ref())
        .unwrap();
    assert_eq!(group.work.iter().flatten().count(), count);
    let addresses = group
        .work
        .iter()
        .flatten()
        .take(wet)
        .map(|work| {
            work.candidate
                .as_ref()
                .unwrap()
                .rubberband
                .as_ref()
                .unwrap()
                .state_address()
        })
        .collect();
    assert!(starts.iter().all(|start| start.effective_frame().is_none() && start.execution_frame() == Some(8192)));
    (fixture, captured, starts, addresses)
}

#[test]
fn deferred_group_reuses_every_independent_physical_birth_without_worker_roundtrip() {
    for ratio in [0.8, 1.16] {
        for (count, wet) in [(2, 2), (4, 2), (6, 6)] {
            let (mut fixture, captured, starts, addresses) = deferred(count, wet, ratio);
            let gate = WorkerGate::for_ordinary(&starts[0]); // Any extra worker would block.
            let (mut feedback, mut messages) = rtrb::RingBuffer::new(128);
            let ((), allocations) = observe_current_thread(|| {
                crate::audio_engine::audio_stream::render_scheduled_audio(
                    &mut fixture.mixer,
                    &mut fixture.scheduler,
                    &mut [],
                    &mut [0.0; super::super::super::constants::NUM_SAMPLES],
                    8193,
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
            assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
            assert!(
                starts.iter().all(|start| start.original_frame() == Some(0)
                    && start.effective_frame() == Some(8193))
            );
            assert!(fixture.scheduler.is_empty());
            assert_eq!(
                fixture
                    .mixer
                    .pitch_runtime
                    .as_ref()
                    .unwrap()
                    .unused_action_capacity()
                    .0,
                MAX_PITCH_ACTIONS
            );
            for (id, address) in addresses.iter().enumerate() {
                assert_ne!(*address, 0);
                assert!(!addresses[..id].contains(address));
                assert_eq!(fixture.voice(id).stretch.native_state_address(), *address);
                assert_eq!(
                    fixture.voice(id).stretch.adopted_request_id(),
                    Some(starts[id].sequence())
                );
            }
            drop(gate);
            let pcm = fixture.pcm(24_000);
            let reference = physical_birth_tone_reference(pcm.len(), SOURCE_HZ, 0, 0, ratio, true);
            let source = &sample_source(&fixture);
            for (frame, (actual, expected)) in pcm.iter().zip(reference).enumerate() {
                assert!(
                    (*actual - expected * wet as f32 - source[frame] * (count - wet) as f32).abs()
                        < 1.0e-4,
                    "{count}/{wet}/{ratio}/{frame}"
                );
            }
            assert!((0..count).all(|id| fixture.starts(id) == 1));
        }
    }
}

fn sample_source(fixture: &Fixture) -> Vec<f32> {
    let sample = fixture.voice(0).sample.as_ref().unwrap();
    assert_eq!(sample.resident_start(), 4800);
    sample.samples[..24_000].to_vec()
}

#[test]
fn deferred_group_requires_complete_exact_metadata_and_unchanged_late_bounds() {
    let (mut fixture, _, _, _) = deferred(2, 2, 0.8);
    let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
    let header = runtime
        .slots
        .iter()
        .position(|slot| slot.group.is_some())
        .unwrap();
    let mut group = runtime.slots[header].group.take().unwrap();
    let peer = group.slots[1];
    assert!(runtime.deferred_group_metadata_current(header, &group, 8192, 8193));
    let owner = runtime.slots[peer].owner.take();
    assert!(!runtime.deferred_group_metadata_current(header, &group, 8192, 8193));
    runtime.slots[peer].owner = owner;
    for field in 1..7 {
        let slot = &mut runtime.slots[peer];
        let (sequence, planned, original, content, replans) = (
            slot.sequence,
            slot.planned,
            slot.original,
            slot.content,
            slot.replans,
        );
        match field {
            1 => slot.sequence += 1,
            2 => slot.planned += 1,
            3 => slot.original += 1,
            4 => slot.content = None,
            5 => slot.replans = MAX_LATE_REPLANS,
            _ => {}
        }
        let absent = if field == 6 {
            group.work[1].take()
        } else {
            None
        };
        assert!(!runtime.deferred_group_metadata_current(header, &group, 8192, 8193));
        let slot = &mut runtime.slots[peer];
        (
            slot.sequence,
            slot.planned,
            slot.original,
            slot.content,
            slot.replans,
        ) = (sequence, planned, original, content, replans);
        if field == 6 {
            group.work[1] = absent;
        }
    }
    assert!(!runtime.deferred_group_metadata_current(header, &group, 8192, MAX_LAUNCH_DELAY + 1));
    runtime.slots[header].group = Some(group);
}

#[test]
fn deferred_group_lost_later_member_bind_rejects_all_before_any_audio() {
    let (mut fixture, captured, starts, _) = deferred(2, 2, 0.8);
    let cancelled = starts[1].clone();
    BIND_HOOK.with(|hook| {
        *hook.borrow_mut() = Some((
            1,
            Box::new(move || {
                assert!(cancelled.cancel());
            }),
        ))
    });
    assert_eq!(fixture.render(1), vec![0.0]);
    assert!(BIND_HOOK.with(|hook| hook.borrow().is_none()));
    assert_eq!(captured.state(), "rejected");
    assert!(
        starts
            .iter()
            .all(|start| start.state() == "rejected" && start.effective_frame().is_none())
    );
    assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));
    assert!(fixture.scheduler.is_empty());
}

#[test]
fn deferred_group_stale_source_control_cancel_and_epoch_cannot_publish_a_prefix() {
    for failure in 0..4 {
        let (mut fixture, _, starts, _) = deferred(2, 2, 0.8);
        match failure {
            0 => {
                assert!(starts[1].cancel());
            }
            1 => {
                fixture
                    .mixer
                    .set_pad_pitch_intent(
                        1,
                        crate::audio_engine::pitch_intent::PitchIntent::new(0, 3).unwrap(),
                    )
                    .unwrap();
            }
            2 => {
                let sample = fixture.control.engine.sample_cache.lock().unwrap()[1]
                    .clone()
                    .unwrap();
                publish_tone(&fixture.control, 1, sample, true);
            }
            _ => {
                let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap();
                let group = runtime
                    .slots
                    .iter()
                    .find_map(|slot| slot.group.as_ref())
                    .unwrap();
                group.work[1]
                    .as_ref()
                    .unwrap()
                    .epoch
                    .store(0, Ordering::Release);
            }
        }
        assert_eq!(fixture.render(1), vec![0.0]);
        assert!(starts.iter().all(|start| start.effective_frame().is_none()));
        assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));
    }
}

#[test]
fn deferred_group_occupied_displaced_pins_require_a_real_worker_again() {
    let (mut fixture, _, starts, _) = deferred(2, 2, 0.8);
    let mut runtime = fixture.mixer.pitch_runtime.take().unwrap();
    let header = runtime
        .slots
        .iter()
        .position(|slot| slot.group.is_some())
        .unwrap();
    let mut group = runtime.slots[header].group.take().unwrap();
    // Use the real whole fresh earlier bind to occupy the retained pin slot.
    for (member, index) in group.slots[..group.len].iter().copied().enumerate() {
        let work = group.work[member].as_mut().unwrap();
        let plan = fixture
            .mixer
            .plan_ordinary_early_completion(work, 8191, 8193)
            .unwrap();
        work.bind_early_completion(plan).unwrap();
        runtime.slots[index].planned = 8191;
    }
    assert!(
        !runtime
            .bind_deferred_group(&mut fixture.mixer, header, &mut group, 8191, 8193)
            .unwrap()
    );
    runtime.slots[header].group = Some(group);
    fixture.mixer.pitch_runtime = Some(runtime);
    let gate = WorkerGate::for_ordinary(&starts[0]);
    assert_eq!(fixture.render(1), vec![0.0]);
    gate.wait_for(false);
    assert!(
        starts
            .iter()
            .all(|start| start.effective_frame().is_none()
                && start.execution_frame() == Some(16_385))
    );
    assert!(fixture.scheduler.deferred_due().is_none());
    gate.release();
    assert_eq!(gate.wait_for(true), None);
}
