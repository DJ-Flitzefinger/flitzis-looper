//! Actual engine-owned stream-failure projection without a fabricated device handle.
//! The headless FIFO has no callback; failure notification and real quiescence stay separate.

use super::*;

#[test]
fn queued_engine_stream_failure_is_visible_before_callback_quiescence_then_releases_original() {
    let mut rig = Rig::new(2);
    let ticket = rig.prepare("stream-before-callback.capture");
    let session = ticket.session_id();
    let target = 314_159;
    assert!(
        rig.engine
            .start_mix_capture_internal(&ticket, target)
            .unwrap()
    );
    let publication_address;
    {
        let mut slot = rig.engine.mix_capture_owner.lock().unwrap();
        let capture = slot.as_mut().unwrap();
        assert!(capture.matches(&ticket));
        assert!(capture.start_request.is_some() && capture.stop_request.is_none());
        publication_address = Arc::as_ptr(&capture.publication);
        assert_eq!(capture.control.status().failure, None);
        // This is the productive EngineCapture projection used by engine-side
        // status/service when the stream's sticky error bit is observed. No
        // StreamHandle, CPAL invocation, callback or terminal ACK is fabricated.
        capture.poll(true);
    }
    let signalled = ticket.observer.status();
    assert_eq!(signalled.session_id, session);
    assert_eq!(
        (signalled.config.sample_rate_hz, signalled.config.channels),
        (RATE, CHANNELS)
    );
    assert_eq!(signalled.failure, Some(CaptureFailure::Stream));
    assert!(!signalled.writer_ready);
    assert!(!signalled.started && !signalled.rt_sealed && !signalled.rt_released);
    assert!(!signalled.worker_closed && !signalled.worker_joined && !signalled.terminal_decided);
    assert_eq!((signalled.first_frame, signalled.end_frame), (None, None));
    assert_eq!(
        (signalled.submitted_frames, signalled.written_frames),
        (0, 0)
    );
    Python::attach(|py| {
        let status = rig.engine.mix_capture_status_internal(py, &ticket).unwrap();
        let status = status.bind(py);
        assert_eq!(
            status
                .get_item("session_id")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            session
        );
        assert_eq!(
            status
                .get_item("failure")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "stream"
        );
        assert!(
            status
                .get_item("start_queued")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        for field in [
            "started",
            "rt_sealed",
            "rt_released",
            "worker_closed",
            "worker_joined",
            "successful_spool",
        ] {
            assert!(
                !status
                    .get_item(field)
                    .unwrap()
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
                "{field} cannot precede actual quiescence"
            );
        }
    });
    assert!(
        !rig.engine
            .start_mix_capture_internal(&ticket, target)
            .unwrap()
    );
    assert!(
        !rig.engine
            .stop_mix_capture_internal(&ticket, target + 4)
            .unwrap()
    );
    assert!(
        rig.prepare_result("while-stream-owner-pending.capture")
            .is_err()
    );
    assert!(!rig.path("while-stream-owner-pending.capture").exists());
    let ControlMessage::StartMixCapture {
        publication,
        target_frame,
    } = rig.consumer.pop().unwrap()
    else {
        panic!("original start remains the only queued request")
    };
    assert_eq!(target_frame, MixCaptureTarget::At(target));
    assert_eq!(publication.session_id(), session);
    assert_eq!(Arc::as_ptr(&publication), publication_address);
    drop(publication);
    assert!(
        rig.consumer.is_empty(),
        "failure/status/retries cannot manufacture another request"
    );
    let before_close = ticket.observer.status();
    assert!(!before_close.rt_sealed && !before_close.rt_released && !before_close.worker_closed);

    // No callback was created, and the actual headless FIFO is now empty. The
    // existing post-quiescence hook must claim/drop the original engine-owned
    // publication off RT before its real writer can close and be joined.
    rig.engine.poll_mix_capture_shutdown();
    let terminal = rig.poll_until_settled();
    assert_eq!(terminal.session_id, session);
    assert_eq!(terminal.failure, Some(CaptureFailure::Stream));
    assert!(terminal.rt_sealed && terminal.rt_released);
    assert!(terminal.worker_closed && terminal.worker_joined && terminal.terminal_decided);
    assert!(!terminal.started && !terminal.successful_spool());
    assert_eq!((terminal.first_frame, terminal.end_frame), (None, None));
    assert_eq!((terminal.submitted_frames, terminal.written_frames), (0, 0));
    assert!(
        rig.engine
            .mix_capture_owner
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .matches(&ticket)
    );
    assert_eq!(
        fs::metadata(rig.path("stream-before-callback.capture"))
            .unwrap()
            .len(),
        mix_capture::SPOOL_HEADER_BYTES
    );
}
