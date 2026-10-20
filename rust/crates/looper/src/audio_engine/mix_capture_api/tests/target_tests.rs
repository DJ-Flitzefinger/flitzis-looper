//! Original request identity and retry/status proof, without a fabricated callback ACK.
//! Manual RT extents below retain the parent fixture's control/writer-only scope.

use super::*;

fn assert_request_status(
    rig: &Rig,
    ticket: &MixCaptureTicket,
    start: Option<MixCaptureTarget>,
    stop: Option<MixCaptureTarget>,
) {
    {
        let slot = rig.engine.mix_capture_owner.lock().unwrap();
        let capture = slot.as_ref().unwrap();
        assert_eq!(capture.start_request, start);
        assert_eq!(capture.stop_request, stop);
    }
    Python::attach(|py| {
        let status = rig.engine.mix_capture_status_internal(py, ticket).unwrap();
        let status = status.bind(py);
        for (prefix, request) in [("start", start), ("stop", stop)] {
            assert_eq!(
                status
                    .get_item(format!("{prefix}_queued"))
                    .unwrap()
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
                request.is_some(),
            );
            assert_eq!(
                status
                    .get_item(format!("{prefix}_target"))
                    .unwrap()
                    .unwrap()
                    .extract::<Option<u64>>()
                    .unwrap(),
                request.and_then(MixCaptureTarget::explicit_frame),
            );
            assert_eq!(
                status
                    .get_item(format!("{prefix}_target_mode"))
                    .unwrap()
                    .unwrap()
                    .extract::<Option<String>>()
                    .unwrap()
                    .as_deref(),
                request.map(MixCaptureTarget::mode),
            );
        }
    });
}

fn claim_target_start(
    rig: &mut Rig,
    ticket: &MixCaptureTicket,
    target: MixCaptureTarget,
) -> Box<CaptureRt> {
    let ControlMessage::StartMixCapture {
        publication,
        target_frame,
    } = rig.consumer.pop().unwrap()
    else {
        panic!("the actual original start must be queued")
    };
    assert_eq!(target_frame, target);
    assert_eq!(publication.session_id(), ticket.session_id());
    let slot = rig.engine.mix_capture_owner.lock().unwrap();
    assert!(Arc::ptr_eq(
        &publication,
        &slot.as_ref().unwrap().publication,
    ));
    let rt = publication.claim().unwrap();
    assert!(publication.claim().is_none());
    rt
}

#[test]
fn next_request_full_fifo_retry_preserves_immutable_modes_and_only_actual_extent_is_ack() {
    let mut rig = Rig::new(1);
    let ticket = rig.prepare("next-retry.capture");
    assert_request_status(&rig, &ticket, None, None);
    assert!(!rig.engine.stop_mix_capture(&ticket, None).unwrap());
    assert!(rig.consumer.is_empty());

    rig.push_ping();
    assert!(!rig.engine.start_mix_capture(&ticket, None).unwrap());
    assert_request_status(&rig, &ticket, None, None);
    rig.pop_ping();
    assert!(rig.engine.start_mix_capture(&ticket, None).unwrap());
    assert!(!rig.engine.start_mix_capture(&ticket, None).unwrap());
    assert!(!rig.engine.start_mix_capture(&ticket, Some(73)).unwrap());
    assert_request_status(&rig, &ticket, Some(MixCaptureTarget::NextAdmissible), None);
    let mut rt = claim_target_start(&mut rig, &ticket, MixCaptureTarget::NextAdmissible);
    assert!(rig.consumer.is_empty());
    assert_eq!(ticket.observer.status().first_frame, None);

    rig.push_ping();
    assert!(!rig.engine.stop_mix_capture(&ticket, None).unwrap());
    assert_request_status(&rig, &ticket, Some(MixCaptureTarget::NextAdmissible), None);
    rig.pop_ping();
    assert!(rig.engine.stop_mix_capture(&ticket, None).unwrap());
    assert!(rig.engine.stop_mix_capture(&ticket, None).unwrap());
    assert!(!rig.engine.stop_mix_capture(&ticket, Some(77)).unwrap());
    assert_request_status(
        &rig,
        &ticket,
        Some(MixCaptureTarget::NextAdmissible),
        Some(MixCaptureTarget::NextAdmissible),
    );
    let ControlMessage::StopMixCapture {
        session_id,
        target_frame,
    } = rig.consumer.pop().unwrap()
    else {
        panic!("one actual original stop must remain")
    };
    assert_eq!(session_id, ticket.session_id());
    assert_eq!(target_frame, MixCaptureTarget::NextAdmissible);
    assert!(
        rig.consumer.is_empty(),
        "duplicate stop cannot enqueue again"
    );
    assert_eq!(ticket.observer.status().end_frame, None);

    let expected = manual_extent(&mut rt, 73);
    drop(rt);
    let status = rig.poll_until_settled();
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(73), Some(77)));
    let stored = fs::read(rig.path("next-retry.capture")).unwrap();
    let bits = stored[mix_capture::SPOOL_HEADER_BYTES as usize..]
        .chunks_exact(4)
        .map(|sample| u32::from_le_bytes(sample.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(bits, expected);
    assert!(rig.engine.stop_mix_capture(&ticket, None).unwrap());
    assert!(!rig.engine.stop_mix_capture(&ticket, Some(77)).unwrap());
    assert_request_status(
        &rig,
        &ticket,
        Some(MixCaptureTarget::NextAdmissible),
        Some(MixCaptureTarget::NextAdmissible),
    );
    assert!(rig.consumer.is_empty());
}

#[test]
fn explicit_request_and_next_mode_never_alias_even_when_actual_frames_coincide() {
    let mut rig = Rig::new(2);
    let ticket = rig.prepare("explicit-mode.capture");
    assert!(rig.engine.start_mix_capture(&ticket, Some(73)).unwrap());
    let mut rt = claim_target_start(&mut rig, &ticket, MixCaptureTarget::At(73));
    assert!(rig.engine.stop_mix_capture(&ticket, Some(77)).unwrap());
    assert!(!rig.engine.stop_mix_capture(&ticket, None).unwrap());
    assert!(rig.engine.stop_mix_capture_internal(&ticket, 77).unwrap());
    assert!(!rig.engine.stop_mix_capture_internal(&ticket, 78).unwrap());
    assert_request_status(
        &rig,
        &ticket,
        Some(MixCaptureTarget::At(73)),
        Some(MixCaptureTarget::At(77)),
    );
    let ControlMessage::StopMixCapture { target_frame, .. } = rig.consumer.pop().unwrap() else {
        panic!("one explicit stop expected")
    };
    assert_eq!(target_frame, MixCaptureTarget::At(77));
    assert!(rig.consumer.is_empty());
    drop(manual_extent(&mut rt, 73));
    drop(rt);
    assert!(rig.poll_until_settled().successful_spool());
    assert!(!rig.engine.stop_mix_capture(&ticket, None).unwrap());
}

#[test]
fn next_mode_keeps_foreign_retired_cancel_and_stream_failure_guards() {
    let mut first = Rig::new(2);
    let second = Rig::new(2);
    let old = first.prepare("next-cancel.capture");
    let foreign = second.prepare("next-foreign.capture");
    assert!(first.engine.start_mix_capture(&foreign, None).is_err());
    assert!(first.engine.stop_mix_capture(&foreign, None).is_err());
    assert!(second.engine.start_mix_capture(&old, None).is_err());
    assert!(second.engine.stop_mix_capture(&old, None).is_err());
    assert!(first.engine.cancel_mix_capture_internal(&old).unwrap());
    assert!(!first.engine.start_mix_capture(&old, None).unwrap());
    assert!(!first.engine.stop_mix_capture(&old, None).unwrap());
    let cancelled = first.poll_until_settled();
    assert_eq!(cancelled.failure, Some(CaptureFailure::Cancelled));
    assert_eq!((cancelled.first_frame, cancelled.end_frame), (None, None));
    let ticket = first.prepare("next-stream.capture");
    assert!(first.engine.start_mix_capture(&old, None).is_err());
    assert!(first.engine.stop_mix_capture(&old, None).is_err());
    assert!(first.engine.start_mix_capture(&ticket, None).unwrap());
    {
        let mut slot = first.engine.mix_capture_owner.lock().unwrap();
        slot.as_mut().unwrap().poll(true);
    }
    assert_eq!(
        ticket.observer.status().failure,
        Some(CaptureFailure::Stream)
    );
    assert!(!ticket.observer.status().rt_sealed);
    assert!(!ticket.observer.status().rt_released);
    assert!(!first.engine.stop_mix_capture(&ticket, None).unwrap());
    let ControlMessage::StartMixCapture { target_frame, .. } = first.consumer.pop().unwrap() else {
        panic!("the original queued start remains")
    };
    assert_eq!(target_frame, MixCaptureTarget::NextAdmissible);
    assert!(first.consumer.is_empty());
    first.engine.poll_mix_capture_shutdown();
    let terminal = first.poll_until_settled();
    assert_eq!(terminal.failure, Some(CaptureFailure::Stream));
    assert!(!terminal.started);
    assert_eq!((terminal.first_frame, terminal.end_frame), (None, None));
    assert!(second.consumer.is_empty());
    assert_eq!(foreign.observer.status().failure, None);
}
