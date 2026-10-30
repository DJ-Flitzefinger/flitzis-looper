//! Initial recording completion shares the original group event and capture owner.

use super::*;
use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::WorkerGate;
use crate::audio_engine::pitch_runtime_tests::publish_tone;
use crate::messages::SampleBuffer;

fn wait_members(fixture: &mut Fixture, count: usize) {
    let deadline = Instant::now() + CAPTURE_WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.ordinary_group_ready(count) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "whole recording group did not return"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn capture_members(fixture: &Fixture, count: usize) -> CapturedGlobalStart {
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

fn rate_rig(ratio: f64, burst: bool) -> CaptureRig {
    let mut fixture = Fixture::new(true);
    if burst {
        let samples = (0..3 * RATE as usize)
            .map(|frame| {
                let Some(age) = frame.checked_sub(LOOP_START) else {
                    return 0.0;
                };
                let seconds = age as f64 / f64::from(RATE);
                (0.4 * (-seconds / 0.004).exp() * (seconds * 1000.0 * std::f64::consts::TAU).sin())
                    as f32
            })
            .collect::<Vec<_>>();
        let sample = SampleBuffer {
            channels: 1,
            samples: Arc::from(samples),
            residency: None,
        }
        .with_complete_source(RATE);
        for id in [0, 1] {
            publish_tone(&fixture.control, id, sample.clone(), true);
        }
    }
    adopt_current_sources(&mut fixture, true);
    refresh_actual_runtime_bindings(&fixture);
    fixture.mixer.set_bpm_lock(true);
    fixture.mixer.set_master_bpm(120.0);
    for id in [0, 1] {
        fixture.mixer.set_pad_bpm(id, Some(120.0 / ratio));
    }
    fixture.feedback.reserve(128);
    CaptureRig::with_fixture(fixture, 16)
}

fn complete_at_512(rig: &mut CaptureRig, captured: &CapturedGlobalStart) -> u128 {
    let gate = WorkerGate::for_ordinary(&captured.owner.members[0].start);
    rig.publish(captured);
    rig.fixture.drain();
    assert!(
        captured
            .owner
            .members
            .iter()
            .all(|member| member.start.execution_frame() == Some(8192))
    );
    gate.wait_for(false);
    assert!(rig.fixture.render(512).iter().all(|sample| *sample == 0.0));
    assert!(!rig.status().started);
    assert!(rig.fixture.scheduler.deferred_due().is_none());
    let released = Instant::now();
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_group(&mut rig.fixture);
    released.elapsed().as_micros()
}

fn complete_after_real_reprepare(
    rig: &mut CaptureRig,
    captured: &CapturedGlobalStart,
    wet: usize,
) -> u128 {
    let gate = WorkerGate::for_ordinary(&captured.owner.members[wet - 1].start);
    rig.publish(captured);
    rig.fixture.drain();
    gate.wait_for(false);
    assert!(
        rig.fixture
            .render_to(8193)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert!(!rig.status().started);
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_members(&mut rig.fixture, captured.owner.members.len());
    gate.arm_next_attempt();
    rig.fixture.render(0);
    assert!(
        captured
            .owner
            .members
            .iter()
            .all(|member| member.start.execution_frame() == Some(16385))
    );
    gate.wait_for(false);
    rig.fixture
        .scheduler
        .schedule(8300, ScheduledCommand::StopSample { id: 10 })
        .unwrap();
    assert!(rig.fixture.render(512).iter().all(|sample| *sample == 0.0));
    assert_eq!(
        rig.fixture.scheduler.len(),
        1,
        "unrelated STOP progresses while late worker owns the final member"
    );
    assert!(rig.fixture.scheduler.deferred_due().is_none());
    assert!(!rig.status().started);
    assert!(
        captured
            .owner
            .members
            .iter()
            .all(|member| member.start.effective_frame().is_none()
                && member.start.execution_frame() == Some(16385))
    );
    let released = Instant::now();
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_members(&mut rig.fixture, captured.owner.members.len());
    released.elapsed().as_micros()
}

fn burst_landmarks(pcm: &[f32]) -> (usize, usize, usize) {
    let peak = pcm
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);
    assert!(peak > 1.0e-5);
    let first = pcm.iter().position(|sample| sample.abs() > 1.0e-7).unwrap();
    let significant = pcm
        .iter()
        .position(|sample| sample.abs() >= peak * 0.1)
        .unwrap();
    let width = RATE as usize / 1000;
    let principal = pcm
        .windows(width)
        .enumerate()
        .map(|(frame, window)| {
            (
                frame,
                window
                    .iter()
                    .map(|sample| f64::from(*sample).powi(2))
                    .sum::<f64>(),
            )
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap()
        .0
        + width / 2;
    (first, significant, principal)
}

#[test]
#[ignore = "paired frozen-clock recording scheduling/attack diagnostic, not device latency"]
fn completed_recording_response() {
    for ratio in [0.8, 1.16] {
        let mut rig = rate_rig(ratio, true);
        let captured = public_capture(&rig.fixture);
        rig.fixture.drain();
        let residual_ready_us = complete_at_512(&mut rig, &captured);
        rig.fixture.render(0);
        let execution = captured.owner.members[0].start.execution_frame().unwrap();
        let mut pcm = rig.fixture.render_to(execution);
        pcm.extend(rig.fixture.pcm(24_000));
        assert_eq!(captured.state(), "applied");
        assert_eq!(rig.status().first_frame, Some(execution));
        let (first, significant, principal) = burst_landmarks(&pcm);
        rig.stop(execution + 24_000);
        let settled = rig.joined();
        assert!(settled.successful_spool);
        assert_eq!(settled.written_frames, 24_000);
        assert_eq!(
            rig.bits(),
            pcm[(execution - 512) as usize..]
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        );
        println!(
            "RECORDING_COMPLETION ratio={ratio} T=0 admitted_E=8192 ready_observed_frame=512 residual_release_to_ready_us={residual_ready_us} actual_E={execution} first_from_T={} significant_from_T={} principal_from_T={} principal_after_E={} n=1 source_resident=true clock_frozen_during_worker=true",
            first + 512,
            significant + 512,
            principal + 512,
            principal + 512 - execution as usize
        );
    }
}

#[test]
fn recording_completion_waits_for_every_worker_and_preserves_independent_full_pcm_and_spool_bits() {
    use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_source_ids;
    use crate::audio_engine::pitch_runtime_tests::{SOURCE_HZ, physical_birth_tone_reference};
    for ratio in [0.8, 1.16] {
        for (count, wet) in [(2, 2), (4, 2), (6, 6)] {
            let mut rig = rate_rig(ratio, false);
            let sample = rig.fixture.control.engine.sample_cache.lock().unwrap()[0]
                .clone()
                .unwrap();
            for id in 0..count {
                publish_tone(&rig.fixture.control, id, sample.clone(), true);
            }
            adopt_current_source_ids(&mut rig.fixture, true, &(0..count).collect::<Vec<_>>());
            refresh_actual_runtime_bindings(&rig.fixture);
            for id in 0..count {
                rig.fixture.mixer.set_pad_loop_region(id, 0.1, Some(2.1));
                rig.fixture
                    .mixer
                    .set_pad_bpm(id, Some(if id < wet { 120.0 / ratio } else { 120.0 }));
            }
            let captured = capture_members(&rig.fixture, count);
            rig.fixture.drain();
            let gate = WorkerGate::for_ordinary(&captured.owner.members[wet - 1].start);
            let batch = rig.publish(&captured);
            rig.fixture.drain();
            gate.wait_for(false);
            rig.fixture.mixer.pitch_runtime.as_mut().unwrap().poll();
            assert!(
                rig.fixture
                    .mixer
                    .pitch_runtime
                    .as_ref()
                    .unwrap()
                    .has_ready_action(captured.owner.members[0].start.sequence())
            );
            rig.fixture
                .scheduler
                .schedule(128, ScheduledCommand::StopSample { id: 10 })
                .unwrap();
            assert!(rig.fixture.render(512).iter().all(|sample| *sample == 0.0));
            assert_eq!(rig.fixture.scheduler.len(), 1, "unrelated STOP progressed");
            assert!(rig.fixture.scheduler.deferred_due().is_none());
            assert!(!rig.status().started);
            assert!(
                captured
                    .owner
                    .members
                    .iter()
                    .all(|member| member.start.effective_frame().is_none())
            );
            gate.release();
            assert_eq!(gate.wait_for(true), None);
            wait_members(&mut rig.fixture, count);
            measured_capture_callback(
                &mut rig.fixture,
                &mut [],
                "complete recording group retarget and capture claim",
            );
            assert_eq!(batch.publication_status(), "accepted");
            assert_eq!(rig.status().first_frame, Some(512));
            assert!(
                captured
                    .owner
                    .members
                    .iter()
                    .all(|member| member.start.original_frame() == Some(0)
                        && member.start.effective_frame() == Some(512))
            );
            let addresses = (0..wet)
                .map(|id| rig.fixture.voice(id).stretch.native_state_address())
                .collect::<Vec<_>>();
            for (id, address) in addresses.iter().enumerate() {
                assert_ne!(*address, 0);
                assert!(!addresses[..id].contains(address));
                assert_eq!(
                    rig.fixture.voice(id).stretch.adopted_request_id(),
                    Some(captured.owner.members[id].start.sequence())
                );
            }
            let pcm = rig.fixture.pcm(24_000);
            let reference = physical_birth_tone_reference(pcm.len(), SOURCE_HZ, 0, 0, ratio, true);
            for (frame, (actual, expected)) in pcm.iter().zip(reference).enumerate() {
                let dry = (0.25
                    * ((LOOP_START + frame % (LOOP_END - LOOP_START)) as f64
                        * SOURCE_HZ
                        * std::f64::consts::TAU
                        / f64::from(RATE))
                    .sin()) as f32;
                assert!(
                    actual.is_finite()
                        && (*actual - expected * wet as f32 - dry * (count - wet) as f32).abs()
                            < 1.0e-4,
                    "{count}/{wet}, ratio={ratio}, frame={frame}"
                );
            }
            assert!((0..count).all(|id| rig.fixture.starts(id) == 1));
            rig.stop(24_512);
            let settled = rig.joined();
            assert!(settled.successful_spool);
            assert_eq!(
                (settled.first_frame, settled.end_frame),
                (Some(512), Some(24_512))
            );
            assert_eq!(
                (settled.submitted_frames, settled.written_frames),
                (24_000, 24_000)
            );
            assert_eq!(
                rig.bits(),
                pcm.iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn late_recording_completion_requires_all_real_reprepares_and_keeps_whole_pcm_spool_and_stop() {
    use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_source_ids;
    use crate::audio_engine::pitch_runtime_tests::{SOURCE_HZ, physical_birth_tone_reference};
    for ratio in [0.8, 1.16] {
        for (count, wet) in [(2, 2), (4, 2), (6, 6)] {
            let mut rig = rate_rig(ratio, false);
            let sample = rig.fixture.control.engine.sample_cache.lock().unwrap()[0]
                .clone()
                .unwrap();
            for id in 0..count {
                publish_tone(&rig.fixture.control, id, sample.clone(), true);
            }
            adopt_current_source_ids(&mut rig.fixture, true, &(0..count).collect::<Vec<_>>());
            refresh_actual_runtime_bindings(&rig.fixture);
            for id in 0..count {
                rig.fixture.mixer.set_pad_loop_region(id, 0.1, Some(2.1));
                rig.fixture
                    .mixer
                    .set_pad_bpm(id, Some(if id < wet { 120.0 / ratio } else { 120.0 }));
            }
            let captured = capture_members(&rig.fixture, count);
            rig.fixture.drain();
            complete_after_real_reprepare(&mut rig, &captured, wet);
            // Admission of STOP still uses pending S; moving S earlier preserves
            // this original reservation, including its exact recorded endpoint.
            rig.stop(32_705);
            measured_capture_callback(
                &mut rig.fixture,
                &mut [],
                "returned legacy recording completion",
            );
            assert_eq!(rig.status().first_frame, Some(8705));
            assert!(
                captured
                    .owner
                    .members
                    .iter()
                    .all(|member| member.start.original_frame() == Some(0)
                        && member.start.effective_frame() == Some(8705))
            );
            let addresses = (0..wet)
                .map(|id| rig.fixture.voice(id).stretch.native_state_address())
                .collect::<Vec<_>>();
            for (id, address) in addresses.iter().enumerate() {
                assert_ne!(*address, 0);
                assert!(!addresses[..id].contains(address));
                assert_eq!(
                    rig.fixture.voice(id).stretch.adopted_request_id(),
                    Some(captured.owner.members[id].start.sequence())
                );
            }
            let pcm = rig.fixture.pcm(24_000);
            let reference = physical_birth_tone_reference(pcm.len(), SOURCE_HZ, 0, 0, ratio, true);
            for (frame, (actual, expected)) in pcm.iter().zip(reference).enumerate() {
                let dry = (0.25
                    * ((LOOP_START + frame % (LOOP_END - LOOP_START)) as f64
                        * SOURCE_HZ
                        * std::f64::consts::TAU
                        / f64::from(RATE))
                    .sin()) as f32;
                assert!(
                    actual.is_finite()
                        && (*actual - expected * wet as f32 - dry * (count - wet) as f32).abs()
                            < 1.0e-4,
                    "{count}/{wet}, r={ratio}, frame={frame}"
                );
            }
            rig.fixture.render(0);
            let settled = rig.joined();
            assert!(settled.successful_spool);
            assert_eq!(
                (
                    settled.first_frame,
                    settled.end_frame,
                    settled.written_frames
                ),
                (Some(8705), Some(32_705), 24_000)
            );
            assert_eq!(
                rig.bits(),
                pcm.iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>()
            );
            assert!((0..count).all(|id| rig.fixture.starts(id) == 1));
        }
    }
}

#[test]
#[ignore = "frozen-clock late recording scheduling/attack diagnostic, not device latency"]
fn late_reprepared_recording_response() {
    for ratio in [0.8, 1.16] {
        let mut rig = rate_rig(ratio, true);
        let captured = public_capture(&rig.fixture);
        rig.fixture.drain();
        let residual_ready_us = complete_after_real_reprepare(&mut rig, &captured, 2);
        rig.fixture.render(0);
        let execution = captured.owner.members[0].start.execution_frame().unwrap();
        let pcm = rig.fixture.pcm(24_000);
        let (first, significant, principal) = burst_landmarks(&pcm);
        rig.stop(execution + 24_000);
        let settled = rig.joined();
        assert!(settled.successful_spool);
        assert_eq!(rig.status().first_frame, Some(execution));
        assert_eq!(
            rig.bits(),
            pcm.iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        );
        println!(
            "LATE_REPREPARED_RECORDING ratio={ratio} T=0 admitted_E=8192 legacy_E=16385 ready_observed_frame=8705 residual_release_to_ready_us={residual_ready_us} actual_E={execution} first_from_T={} significant_from_T={} principal_from_T={} principal_after_E={principal} n=1 source_resident=true clock_frozen_during_worker=true",
            first as u64 + execution,
            significant as u64 + execution,
            principal as u64 + execution
        );
    }
}

#[test]
fn returned_legacy_recording_completion_refuses_cancelled_or_changed_whole_group() {
    for fence in ["cancel", "source", "rate_aba", "gain"] {
        let mut rig = rate_rig(0.8, false);
        let captured = public_capture(&rig.fixture);
        rig.fixture.drain();
        complete_after_real_reprepare(&mut rig, &captured, 2);
        match fence {
            "cancel" => {
                assert!(captured.cancel());
            }
            "source" => {
                let sample = rig.fixture.control.engine.sample_cache.lock().unwrap()[1]
                    .clone()
                    .unwrap();
                publish_tone(&rig.fixture.control, 1, sample, true);
            }
            "rate_aba" => {
                rig.fixture.mixer.set_pad_bpm(1, Some(160.0));
                rig.fixture.mixer.set_pad_bpm(1, Some(150.0));
            }
            _ => rig.fixture.mixer.set_pad_gain(1, -6.0),
        }
        rig.fixture.drain();
        assert!(
            rig.fixture
                .render_to(16386)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert_eq!(captured.state(), "rejected", "{fence}");
        assert!(
            captured
                .owner
                .members
                .iter()
                .all(|member| member.start.effective_frame().is_none()
                    && member.start.execution_frame() == Some(16385))
        );
        assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
        let settled = rig.joined();
        assert!(!settled.started && !settled.successful_spool);
        assert_eq!(settled.written_frames, 0);
        assert!(rig.bits().is_empty());
    }
}

#[test]
fn recording_completion_updates_pending_start_and_preserves_the_original_reserved_stop() {
    for end in [512, 1536, 9216] {
        let mut rig = rate_rig(0.8, false);
        let captured = public_capture(&rig.fixture);
        rig.fixture.drain();
        complete_at_512(&mut rig, &captured);
        if end == 9216 {
            while !rig.fixture.scheduler.is_full() {
                rig.fixture
                    .scheduler
                    .schedule(100_000, ScheduledCommand::StopSample { id: 10 })
                    .unwrap();
            }
            rig.stop(end);
            assert!(
                rig.fixture.scheduler.is_full(),
                "only the reserved STOP lane was spent"
            );
        }
        measured_capture_callback(
            &mut rig.fixture,
            &mut [],
            "recording completion before own STOP admission",
        );
        assert_eq!(rig.status().first_frame, Some(512));
        if end != 9216 {
            rig.stop(end);
        }
        assert_eq!(
            rig.fixture
                .control
                .engine
                .stop_mix_capture_internal(&rig.ticket, 512)
                .unwrap(),
            end == 512,
            "the same STOP is idempotent; a different endpoint cannot replace it"
        );
        rig.fixture
            .control
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::StopMixCapture {
                session_id: rig.ticket.session_id(),
                target_frame: MixCaptureTarget::At(512),
            })
            .unwrap();
        rig.fixture.drain();
        assert_eq!(
            rig.fixture
                .scheduler
                .capture_stop_target_frame(rig.ticket.session_id()),
            Some(end),
            "later duplicate cannot replace the original admitted STOP"
        );
        let pcm = rig.fixture.render((end - 512) as usize + 1);
        let settled = rig.joined();
        assert_eq!(
            (settled.first_frame, settled.end_frame),
            (Some(512), Some(end))
        );
        assert_eq!(settled.failure, None);
        assert_eq!(settled.written_frames, end - 512);
        assert_eq!(settled.empty, end == 512);
        assert_eq!(
            rig.bits(),
            pcm[..(end - 512) as usize]
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
    }
}

#[test]
fn recording_completion_cannot_revive_a_stop_cancelled_or_stale_original_group() {
    for fence in ["own_stop", "cancel", "source", "rate_aba", "gain"] {
        let mut rig = rate_rig(0.8, false);
        let captured = public_capture(&rig.fixture);
        rig.fixture.drain();
        let gate = WorkerGate::for_ordinary(&captured.owner.members[1].start);
        let batch = rig.publish(&captured);
        rig.fixture.drain();
        gate.wait_for(false);
        assert!(rig.fixture.render(512).iter().all(|sample| *sample == 0.0));
        let held_cancellation = matches!(fence, "own_stop" | "cancel");
        if !held_cancellation {
            gate.release();
            assert_eq!(gate.wait_for(true), None);
            wait_members(&mut rig.fixture, 2);
        }
        match fence {
            "own_stop" => rig.stop(512),
            "cancel" => {
                assert!(captured.cancel());
            }
            "source" => {
                let sample = rig.fixture.control.engine.sample_cache.lock().unwrap()[1]
                    .clone()
                    .unwrap();
                publish_tone(&rig.fixture.control, 1, sample, true);
            }
            "rate_aba" => {
                rig.fixture.mixer.set_pad_bpm(1, Some(160.0));
                rig.fixture.mixer.set_pad_bpm(1, Some(150.0));
            }
            _ => rig.fixture.mixer.set_pad_gain(1, -6.0),
        }
        if held_cancellation {
            gate.release();
            gate.wait_for(true);
        }
        assert!(
            rig.fixture
                .render_to(8192)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        rig.fixture.render(0);
        assert_eq!(batch.publication_status(), "rejected", "{fence}");
        assert_eq!(
            (rig.fixture.starts(0), rig.fixture.starts(1)),
            (0, 0),
            "{fence}"
        );
        let settled = rig.joined();
        assert!(!settled.started && !settled.successful_spool, "{fence}");
        assert_eq!((settled.first_frame, settled.end_frame), (None, None));
        assert_eq!(settled.written_frames, 0);
        assert!(rig.bits().is_empty());
    }
}

#[test]
fn recording_completion_feedback_pressure_rejects_the_whole_capture_without_a_prefix() {
    use crate::audio_engine::audio_stream::render_scheduled_audio;
    use crate::audio_engine::constants::NUM_SAMPLES;
    use rtrb::RingBuffer;
    let mut rig = rate_rig(0.8, false);
    let captured = public_capture(&rig.fixture);
    rig.fixture.drain();
    let gate = WorkerGate::for_ordinary(&captured.owner.members[1].start);
    let batch = rig.publish(&captured);
    rig.fixture.drain();
    gate.wait_for(false);
    assert!(rig.fixture.render(512).iter().all(|sample| *sample == 0.0));
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_members(&mut rig.fixture, 2);
    let (mut feedback, mut messages) = RingBuffer::new(1);
    feedback
        .push(crate::messages::AudioMessage::SampleStarted { id: 10 })
        .unwrap();
    render_scheduled_audio(
        &mut rig.fixture.mixer,
        &mut rig.fixture.scheduler,
        &mut [0.0],
        &mut [0.0; NUM_SAMPLES],
        512,
        1,
        &mut rig.fixture.transport,
        &mut feedback,
        &mut rig.fixture.retirement,
    );
    rig.fixture.transport.advance_by_rendered_frames(1);
    assert!(rig.fixture.scheduler.deferred_due().is_none());
    assert!(!rig.status().started);
    assert!((0..2).all(|id| rig.fixture.starts(id) == 0));
    assert!(
        captured
            .owner
            .members
            .iter()
            .all(|member| member.start.execution_frame() == Some(512)
                && member.start.effective_frame().is_none())
    );
    messages.pop().unwrap();
    assert_eq!(batch.publication_status(), "rejected");
    assert_eq!(captured.state(), "rejected");
    let settled = rig.joined();
    assert_eq!((settled.first_frame, settled.end_frame), (None, None));
    assert_eq!(settled.written_frames, 0);
    assert_eq!(settled.failure.as_deref(), Some("rejected"));
    assert!(rig.bits().is_empty());
}
