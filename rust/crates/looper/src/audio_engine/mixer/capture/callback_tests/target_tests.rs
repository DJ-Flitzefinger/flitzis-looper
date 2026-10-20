//! Native target intent on the actual FIFO/scheduler/final-output/retirement path.
//! This headless proof does not claim atomic global capture, CPAL or hearing acceptance.

use super::*;
use crate::audio_engine::constants::MAX_VOICES;
use crate::messages::MixCaptureTarget;

fn start_next(rig: &mut Rig, take: &Take) {
    rig.send(ControlMessage::StartMixCapture {
        publication: take.publication.clone(),
        target_frame: MixCaptureTarget::NextAdmissible,
    });
}

fn stop_next(rig: &mut Rig, take: &Take) {
    rig.send(ControlMessage::StopMixCapture {
        session_id: take.id(),
        target_frame: MixCaptureTarget::NextAdmissible,
    });
}

fn wait_for_retirement(rig: &mut Rig, slots: usize) {
    rig.worker_gate.store(true, Ordering::Release);
    let deadline = Instant::now() + WAIT;
    while rig.state().retirement.available_retirement_slots() < slots {
        assert!(
            Instant::now() < deadline,
            "actual retirement worker did not make progress"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn next_capture_uses_nonzero_first_unrendered_frame_and_exact_final_pcm_without_grid_or_plus_one() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    load_constant(&mut rig);
    assert!(rig.state().mixer.play_sample(0, 0.8));
    rig.render(&mut [0.0; 74]);
    assert_eq!(rig.state().transport.output_frame(), 37);
    let downbeat = rig.state().transport.downbeat_frame();
    let beat = rig.state().transport.master_period_seconds();
    rig.state().mode = TriggerQuantization::Grid { step_64ths: 16 };
    start_next(&mut rig, &take);
    assert_eq!(rig.drain(), 1);
    assert_eq!(rig.state().mixer.mix_capture_pending_start, Some(37));
    assert_eq!(rig.state().scheduler.peek_next_target_frame(), Some(37));
    assert_eq!(take.status().first_frame, None);
    let mut output = [0.0; 22];
    rig.render(&mut output[..6]);
    rig.render(&mut output[6..8]);
    rig.render(&mut output[8..]);
    assert_eq!(rig.state().transport.output_frame(), 48);
    stop_next(&mut rig, &take);
    assert_eq!(rig.drain(), 1);
    assert_eq!(
        rig.state().scheduler.capture_stop_target_frame(take.id()),
        Some(48)
    );
    rig.render(&mut []);
    assert_eq!(rig.state().transport.output_frame(), 48);
    assert_eq!(rig.state().transport.downbeat_frame(), downbeat);
    assert_eq!(rig.state().transport.master_period_seconds(), beat);
    assert!(
        output
            .iter()
            .all(|sample| (*sample - 0.15_f32).abs() < 1.0e-5)
    );
    rig.close();
    let status = take.joined();
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(37), Some(48)));
    assert_eq!((status.submitted_frames, status.written_frames), (11, 11));
    assert_eq!(take.bits(), output.map(f32::to_bits));
}

#[test]
fn next_start_left_in_actual_fifo_by_retirement_pressure_resolves_only_in_later_callback() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(true);
    rig.render(&mut [0.0; 26]);
    rig.fill_retirement();
    start_next(&mut rig, &take);
    assert_eq!(rig.drain(), 0);
    assert!(matches!(
        rig.state().consumer.peek().unwrap(),
        ControlMessage::StartMixCapture {
            target_frame: MixCaptureTarget::NextAdmissible,
            ..
        }
    ));
    rig.render(&mut [0.0; 14]);
    assert_eq!(rig.state().transport.output_frame(), 20);
    assert_eq!(take.status().first_frame, None);
    assert!(rig.state().mixer.mix_capture.is_none());
    wait_for_retirement(&mut rig, 2);
    assert_eq!(rig.drain(), 1);
    assert_eq!(rig.state().mixer.mix_capture_pending_start, Some(20));
    rig.render(&mut [0.0; 6]);
    stop_next(&mut rig, &take);
    rig.drain();
    rig.render(&mut []);
    rig.close();
    let status = take.joined();
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(20), Some(23)));
    assert_eq!(take.bits(), vec![0; 6]);
}

#[test]
fn capture_own_retirement_gate_defers_original_next_message_without_resolving_or_claiming() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(true);
    rig.render(&mut [0.0; 16]);
    rig.fill_retirement();
    start_next(&mut rig, &take);
    let message = rig.state().consumer.pop().unwrap();
    let state = rig.state();
    // Direct callback entry isolates the mixer's own last-owner capacity guard
    // below the outer FIFO preflight. It still claims no start/effect ACK.
    crate::audio_engine::audio_stream::process_control_message(
        message,
        &mut state.scheduler,
        state.transport.output_frame(),
        &mut state.mode,
        &mut state.transport,
        &mut state.mixer,
        &mut state.messages,
        &mut state.retirement,
    );
    assert!(matches!(
        state.mixer.deferred_control(),
        Some(ControlMessage::StartMixCapture {
            publication,
            target_frame: MixCaptureTarget::NextAdmissible,
        }) if Arc::ptr_eq(publication, &take.publication)
    ));
    assert!(state.mixer.mix_capture.is_none());
    assert!(state.scheduler.is_empty());
    assert_eq!(take.status().first_frame, None);
    assert!(!take.status().rt_released);
    rig.render(&mut [0.0; 12]);
    wait_for_retirement(&mut rig, 2);
    assert_eq!(rig.drain(), 1);
    assert_eq!(rig.state().mixer.mix_capture_pending_start, Some(14));
    rig.render(&mut [0.0; 4]);
    stop_next(&mut rig, &take);
    rig.drain();
    rig.render(&mut []);
    rig.close();
    let status = take.joined();
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(14), Some(16)));
    assert_eq!(take.bits(), vec![0; 4]);
}

#[test]
fn actual_deferred_control_keeps_next_intent_unresolved_until_placement_claim_releases() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    rig.render(&mut [0.0; 18]);
    let ownership = rig.state().mixer.input_ownership_for_effect();
    let placement = ownership.slot_snapshot(0).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(placement, placement.lifetime())
        .unwrap();
    let result = ownership.commit_slot_write_fixed(
        &write,
        || {
            let state = rig.state();
            crate::audio_engine::audio_stream::process_control_message(
                ControlMessage::StartMixCapture {
                    publication: take.publication.clone(),
                    target_frame: MixCaptureTarget::NextAdmissible,
                },
                &mut state.scheduler,
                state.transport.output_frame(),
                &mut state.mode,
                &mut state.transport,
                &mut state.mixer,
                &mut state.messages,
                &mut state.retirement,
            );
            assert!(matches!(
                state.mixer.deferred_control(),
                Some(ControlMessage::StartMixCapture {
                    target_frame: MixCaptureTarget::NextAdmissible,
                    ..
                })
            ));
            assert_eq!(state.mixer.mix_capture_pending_start, None);
            false
        },
        |_| panic!("refused fixture publication must not apply"),
    );
    assert_eq!(
        result,
        Err(crate::audio_engine::content_placement::PlacementError::ClaimRejected)
    );
    rig.render(&mut [0.0; 10]);
    assert_eq!(rig.state().transport.output_frame(), 14);
    assert_eq!(rig.drain(), 1);
    assert_eq!(rig.state().mixer.mix_capture_pending_start, Some(14));
    rig.render(&mut [0.0; 4]);
    stop_next(&mut rig, &take);
    rig.drain();
    rig.render(&mut []);
    rig.close();
    assert_eq!(
        (take.joined().first_frame, take.status().end_frame),
        (Some(14), Some(16))
    );
    assert_eq!(take.bits(), vec![0; 4]);
}

#[test]
fn resolved_start_f_is_not_retargeted_when_an_earlier_real_due_effect_defers_e() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(true);
    load_constant(&mut rig);
    assert!(rig.state().mixer.play_sample(0, 0.8));
    rig.render(&mut [0.0; 20]);
    let ownership = rig.state().mixer.input_ownership_for_effect();
    let placement = ownership.slot_snapshot(NUM_SAMPLES - 1).unwrap();
    rig.state()
        .scheduler
        .schedule(9, ScheduledCommand::StopSampleForPlacement { placement })
        .unwrap();
    start_next(&mut rig, &take);
    assert_eq!(rig.drain(), 1);
    assert_eq!(rig.state().mixer.mix_capture_pending_start, Some(10));
    rig.fill_retirement();
    rig.render(&mut [0.0; 20]);
    assert_eq!(rig.state().transport.output_frame(), 20);
    assert_eq!(
        rig.state().scheduler.deferred_due().unwrap().target_frame,
        9
    );
    assert_eq!(rig.state().mixer.mix_capture_pending_start, Some(10));
    assert_eq!(take.status().first_frame, None);
    wait_for_retirement(&mut rig, 3 * MAX_VOICES + 2);
    assert_eq!(rig.drain(), 0);
    assert_eq!(take.status().first_frame, Some(20));
    assert_eq!(rig.state().mixer.mix_capture_pending_start, None);
    let mut captured = [0.0; 4];
    rig.render(&mut captured);
    stop_next(&mut rig, &take);
    rig.drain();
    rig.render(&mut []);
    rig.close();
    let status = take.joined();
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(20), Some(22)));
    assert!(
        captured
            .iter()
            .all(|sample| (*sample - 0.15_f32).abs() < 1.0e-5)
    );
    assert_eq!(take.bits(), captured.map(f32::to_bits));
}

#[test]
fn next_stop_consumes_original_reserved_lane_when_full_and_foreign_or_duplicate_cannot_retarget() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    rig.render(&mut [0.0; 12]);
    start_next(&mut rig, &take);
    rig.drain();
    rig.render(&mut [0.0; 6]);
    for frame in 30..37 {
        rig.state()
            .scheduler
            .schedule(frame, ScheduledCommand::StopAll)
            .unwrap();
    }
    assert!(rig.state().scheduler.is_full());
    rig.send(ControlMessage::StopMixCapture {
        session_id: take.id() + 1,
        target_frame: MixCaptureTarget::NextAdmissible,
    });
    stop_next(&mut rig, &take);
    rig.stop(&take, 1); // A mixed-mode changed duplicate cannot cancel the original.
    stop_next(&mut rig, &take);
    assert_eq!(rig.drain(), 4);
    assert_eq!(
        rig.state().scheduler.capture_stop_target_frame(take.id()),
        Some(9)
    );
    rig.render(&mut []);
    assert_eq!(rig.state().scheduler.peek_next_target_frame(), Some(30));
    rig.close();
    let status = take.joined();
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(6), Some(9)));
    assert_eq!(take.bits(), vec![0; 6]);
}

#[test]
fn next_same_callback_empty_and_next_stop_before_explicit_future_start_keep_truthful_extents() {
    let mut empty = Take::new(2);
    let mut future = Take::new(2);
    let mut rig = Rig::new(false);
    rig.render(&mut [0.0; 62]);
    start_next(&mut rig, &empty);
    stop_next(&mut rig, &empty);
    assert_eq!(rig.drain(), 2);
    rig.render(&mut []);
    assert!(empty.status().empty());
    assert_eq!(
        (empty.status().first_frame, empty.status().end_frame),
        (Some(31), Some(31))
    );
    rig.start(&future, 40);
    stop_next(&mut rig, &future);
    rig.drain();
    rig.close();
    let status = empty.joined();
    assert!(status.empty() && !status.successful_spool());
    assert!(empty.bits().is_empty());
    let status = future.joined();
    assert_eq!(status.failure, Some(CaptureFailure::Cancelled));
    assert!(!status.started);
    assert_eq!((status.first_frame, status.end_frame), (None, None));
}

#[test]
fn explicit_past_start_still_rejects_and_explicit_past_stop_still_clamps_active_capture() {
    let mut stale = Take::new(2);
    let mut active = Take::new(2);
    let mut rig = Rig::new(false);
    rig.render(&mut [0.0; 34]);
    rig.start(&stale, 16);
    rig.drain();
    assert_eq!(stale.status().failure, Some(CaptureFailure::Rejected));
    rig.start(&active, 17);
    rig.drain();
    rig.render(&mut [0.0; 6]);
    rig.stop(&active, 0);
    rig.drain();
    assert_eq!(
        rig.state().scheduler.capture_stop_target_frame(active.id()),
        Some(20)
    );
    rig.render(&mut []);
    rig.close();
    assert!(!stale.joined().started);
    let status = active.joined();
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(17), Some(20)));
    assert_eq!(active.bits(), vec![0; 6]);
}

#[test]
fn next_target_keeps_geometry_cancel_and_stream_failures_without_start_or_positive_extent() {
    let mut geometry = Take::new(1);
    let mut cancelled = Take::new(2);
    let mut stream = Take::new(2);
    let mut rig = Rig::new(false);
    rig.render(&mut [0.0; 46]);
    start_next(&mut rig, &geometry);
    rig.drain();
    assert_eq!(geometry.status().failure, Some(CaptureFailure::Geometry));
    start_next(&mut rig, &cancelled);
    assert!(
        cancelled
            .control
            .request_failure(cancelled.id(), CaptureFailure::Cancelled)
    );
    rig.drain();
    assert_eq!(cancelled.status().failure, Some(CaptureFailure::Cancelled));
    start_next(&mut rig, &stream);
    rig.drain();
    assert_eq!(rig.state().mixer.mix_capture_pending_start, Some(23));
    rig.state().mixer.fail_mix_capture_stream();
    rig.render(&mut []);
    rig.close();
    for (take, failure) in [
        (&mut geometry, CaptureFailure::Geometry),
        (&mut cancelled, CaptureFailure::Cancelled),
        (&mut stream, CaptureFailure::Stream),
    ] {
        let status = take.joined();
        assert_eq!(status.failure, Some(failure));
        assert!(!status.started);
        assert_eq!((status.first_frame, status.end_frame), (None, None));
        assert_eq!(status.written_frames, 0);
    }
}
