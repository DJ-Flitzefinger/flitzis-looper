//! Genuine worker completion through public Piano capture and the one scheduler.

use super::*;
use crate::audio_engine::ordinary_endpoint_reserve_tests::{
    adopt_current_sources, refresh_actual_runtime_bindings,
};
use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::WorkerGate;
use crate::audio_engine::scheduler::ScheduledCommand;
use crate::test_memory::{ThreadAllocations, observe_current_thread};

fn configured(rate: f64, lock: bool) -> Fixture {
    let mut fixture = Fixture::new(lock);
    adopt_current_sources(&mut fixture, lock);
    refresh_actual_runtime_bindings(&fixture);
    fixture.mixer.set_bpm_lock(true);
    fixture.mixer.set_master_bpm(120.0);
    fixture.mixer.set_pad_bpm(0, Some(120.0 / rate));
    fixture.mixer.set_pad_bpm(1, Some(120.0));
    fixture
}

#[test]
fn piano_completion_waits_for_worker_and_preserves_complete_physical_pcm_without_rt_allocations() {
    for (rate, lock, extra) in [(0.8, true, 0), (1.16, true, 0), (1.0, false, 7)] {
        let mut fixture = configured(rate, lock);
        let action = fixture.capture(0, &format!("pitch.extra:{extra}:set_retrigger"));
        let gate = WorkerGate::for_pitch(&action);
        fixture.publish(&action);
        assert_eq!(action.execution_target(), 8192);
        gate.wait_for(false);
        fixture
            .scheduler
            .schedule(128, ScheduledCommand::StopSample { id: 1 })
            .unwrap();
        assert!(fixture.render(512).iter().all(|value| *value == 0.0));
        assert_eq!(fixture.scheduler.len(), 1);
        assert!(fixture.scheduler.deferred_due().is_none());
        assert!(!action.effective());
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        fixture.wait_worker(&[&action]);
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
            );
        });
        assert_eq!(allocations, ThreadAllocations::default());
        assert!(messages.pop().is_ok());
        assert!(action.effective(), "{:?}", action.reason());
        assert_eq!(action.original_due_frame(), 0);
        assert_eq!(action.execution_target(), 512);
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(action.sequence())
        );
        let pcm = fixture.pcm(24_000);
        assert_physical_birth_tone(&pcm, 0, extra, rate, lock, 1.0);
        let deadline = Instant::now() + WORKER_WAIT;
        while fixture.control.engine.pitch_work_capacity.used() != 0 {
            assert!(
                Instant::now() < deadline,
                "completed work did not retire off RT"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn piano_completion_keeps_equal_and_sixteen_frame_originals_distinct_and_ordered() {
    for gap in [0, 16] {
        let mut fixture = configured(0.8, true);
        let first = fixture.capture(0, "pitch.extra:0:set_retrigger");
        let gate = WorkerGate::for_pitch(&first);
        fixture.publish(&first);
        gate.wait_for(false);
        let mut actions = vec![first];
        for ordinal in 1..4 {
            fixture.render_to(gap * ordinal);
            let next = fixture.capture(0, "pitch.extra:0:set_retrigger");
            fixture.publish(&next);
            assert_eq!(next.original_due_frame(), gap * ordinal);
            actions.push(next);
        }
        fixture.render_to(64);
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        fixture.wait_worker(&actions.iter().collect::<Vec<_>>());
        let mut prefix = Vec::new();
        if gap == 0 {
            prefix = fixture.render(16);
            assert!(
                actions
                    .iter()
                    .all(|action| action.effective() && action.execution_target() == 64)
            );
            assert_eq!(fixture.starts(0), actions.len());
            assert_eq!(
                fixture.voice(0).stretch.adopted_request_id(),
                Some(actions.last().unwrap().sequence())
            );
        } else {
            for (ordinal, action) in actions.iter().enumerate() {
                prefix = fixture.render(16);
                assert!(action.effective(), "{:?}", action.reason());
                assert_eq!(action.execution_target(), 64 + ordinal as u64 * 16);
                assert_eq!(fixture.starts(0), ordinal + 1);
                assert_eq!(
                    fixture.voice(0).stretch.adopted_request_id(),
                    Some(action.sequence())
                );
            }
        }
        assert!(
            actions
                .windows(2)
                .all(|pair| pair[0].sequence() < pair[1].sequence())
        );
        let mut pcm = prefix;
        pcm.extend(fixture.pcm(24_000));
        assert_physical_birth_tone(&pcm, 0, 0, 0.8, true, 1.0);
    }
}

#[test]
fn live_original_chain_piano_keeps_full_physical_pcm_and_independent_native_owners() {
    for (ratio, lock, extra) in [(0.8, true, 0), (1.16, true, 0), (1.0, false, 7)] {
        let mut fixture = configured(ratio, lock);
        let first = fixture.capture(0, &format!("pitch.extra:{extra}:set_retrigger"));
        let gate = WorkerGate::for_pitch(&first);
        fixture.publish(&first);
        gate.wait_for(false);
        let mut actions = vec![first];
        for frame in [16, 32, 48] {
            fixture.render_to(frame);
            let next = fixture.capture(0, &format!("pitch.extra:{extra}:set_retrigger"));
            fixture.publish(&next);
            actions.push(next);
        }
        fixture.render_to(64);
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        fixture.wait_worker(&actions.iter().collect::<Vec<_>>());
        let natives = actions
            .iter()
            .map(|action| {
                fixture
                    .mixer
                    .pitch_runtime
                    .as_ref()
                    .unwrap()
                    .ready_pitch_native_address(action.sequence())
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(
            natives
                .iter()
                .enumerate()
                .all(|(i, native)| *native != 0 && !natives[..i].contains(native))
        );
        let mut pcm = fixture.render(512);
        assert_eq!(fixture.starts(0), 4);
        for (ordinal, action) in actions.iter().enumerate() {
            assert_eq!(action.original_due_frame(), ordinal as u64 * 16);
            assert!(action.effective(), "{:?}", action.reason());
            assert_eq!(action.execution_target(), 64 + ordinal as u64 * 16);
        }
        assert_eq!(fixture.voice(0).stretch.native_state_address(), natives[3]);
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(actions[3].sequence())
        );
        pcm.extend(fixture.pcm(24_048 - pcm.len()));
        let reference = physical_birth_tone_reference(24_000, SOURCE_HZ, 0, extra, ratio, lock);
        for (frame, actual) in pcm.iter().enumerate() {
            let age = if frame < 48 { frame % 16 } else { frame - 48 };
            assert!(
                (*actual - reference[age]).abs() < 1.0e-4,
                "{ratio}/{lock}/{extra}/{frame}"
            );
        }
    }
}

#[test]
fn piano_completion_preserves_future_t_and_refuses_cancelled_or_changed_authority() {
    let mut future = configured(0.8, true);
    future.render_to(17_000);
    future.rows(PitchPadProjection {
        key_lock: true,
        source_root: Some(0),
        source_mode: Some(PitchKeyMode::Major),
        quantize_step_64ths: Some(16),
        ..PitchPadProjection::default()
    });
    let action = future.capture(0, "pitch.extra:0:set_retrigger");
    assert_eq!(action.original_due_frame(), 24_000);
    future.publish(&action);
    future.wait_worker(&[&action]);
    assert!(future.render_to(24_000).iter().all(|value| *value == 0.0));
    future.render(0);
    assert!(action.effective());
    assert_eq!(action.execution_target(), 24_000);

    for fence in ["cancel", "stop", "source", "rate_aba", "gain"] {
        let mut fixture = configured(0.8, true);
        let action = fixture.capture(0, "pitch.extra:0:set_retrigger");
        fixture.publish(&action);
        fixture.wait_worker(&[&action]);
        match fence {
            "cancel" => assert!(fixture.control.engine.cancel_pitch_action(&action).unwrap()),
            "stop" => fixture.control.engine.stop_sample(0).unwrap(),
            "source" => {
                let source = fixture.control.engine.sample_cache.lock().unwrap()[0]
                    .clone()
                    .unwrap();
                publish_tone(&fixture.control, 0, source, true);
            }
            "rate_aba" => {
                fixture.mixer.set_pad_bpm(0, Some(160.0));
                fixture.mixer.set_pad_bpm(0, Some(150.0));
            }
            _ => fixture.mixer.set_pad_gain(0, -6.0),
        }
        fixture.drain();
        fixture.render(1);
        assert!(!action.effective(), "{fence}");
        fixture.render_to(8192);
        fixture.render(0);
        assert_eq!(action.state(), "rejected", "{fence}: {:?}", action.reason());
        assert_eq!(fixture.starts(0), 0);
    }
}

#[test]
fn piano_completion_feedback_pressure_rearms_only_after_real_legacy_reprepare() {
    let mut fixture = configured(0.8, true);
    let action = fixture.capture(0, "pitch.extra:0:set_retrigger");
    fixture.publish(&action);
    fixture.wait_worker(&[&action]);
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
        assert_eq!(action.execution_target(), cycle - 1);
        assert!(!action.effective());
        assert!(fixture.scheduler.deferred_due().is_some());
        messages.pop().unwrap();
        fixture.render(0);
        assert_eq!(action.execution_target(), 8192 + cycle);
        fixture.wait_worker(&[&action]);
    }
    fixture.render(0);
    assert!(action.effective(), "{:?}", action.reason());
    assert_eq!(action.execution_target(), 2);
    let pcm = fixture.pcm(24_000);
    assert_physical_birth_tone(&pcm, 0, 0, 0.8, true, 1.0);
}

#[test]
fn returned_legacy_completion_refuses_cancelled_or_changed_piano_authority() {
    for fence in ["cancel", "stop", "source", "rate_aba", "gain"] {
        let mut fixture = configured(0.8, true);
        let action = fixture.capture(0, "pitch.extra:0:set_retrigger");
        let gate = WorkerGate::for_pitch(&action);
        fixture.publish(&action);
        gate.wait_for(false);
        assert!(fixture.render_to(8193).iter().all(|value| *value == 0.0));
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        fixture.wait_worker(&[&action]);
        fixture.render(0);
        assert_eq!(action.execution_target(), 16385);
        fixture.wait_worker(&[&action]);
        match fence {
            "cancel" => assert!(fixture.control.engine.cancel_pitch_action(&action).unwrap()),
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
        assert_eq!(action.state(), "rejected", "{fence}: {:?}", action.reason());
        assert!(!action.effective());
        assert_eq!(fixture.starts(0), 0);
    }
}

#[test]
fn piano_late_completion_requires_real_reprepare_return_and_preserves_native_and_full_pcm() {
    for (rate, lock, extra) in [(0.8, true, 0), (1.16, true, 0), (1.0, false, 7)] {
        let mut fixture = configured(rate, lock);
        let action = fixture.capture(0, &format!("pitch.extra:{extra}:set_retrigger"));
        let gate = WorkerGate::for_pitch(&action);
        fixture.publish(&action);
        gate.wait_for(false);
        assert!(fixture.render_to(8193).iter().all(|value| *value == 0.0));
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        fixture.wait_worker(&[&action]);
        let native = gate.prepared_native();
        gate.arm_next_attempt();
        fixture.render(0);
        assert_eq!(action.execution_target(), 16385);
        gate.wait_for(false);
        assert!(fixture.render(512).iter().all(|value| *value == 0.0));
        assert!(!action.effective());
        assert_eq!(action.execution_target(), 16385);
        assert!(fixture.scheduler.deferred_due().is_none());
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        assert_eq!(gate.prepared_native(), native);
        fixture.wait_worker(&[&action]);
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
        assert_eq!(action.original_due_frame(), 0);
        assert!(action.effective(), "{:?}", action.reason());
        assert_eq!(action.execution_target(), 8705);
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(action.sequence())
        );
        let pcm = fixture.pcm(24_000);
        assert_physical_birth_tone(&pcm, 0, extra, rate, lock, 1.0);
        assert_eq!(fixture.starts(0), 1);
    }
}
