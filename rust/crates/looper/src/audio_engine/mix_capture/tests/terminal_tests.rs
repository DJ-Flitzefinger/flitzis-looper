//! Actual File/ring terminal-decision races; candidate metadata is not publication authority.

use super::*;

fn wait_at_worker_phase(gate: &writer::WorkerPhaseGate) {
    let deadline = Instant::now() + WAIT;
    while !gate.reached.load(Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "writer did not reach terminal phase gate"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn seal_original_prefix(rig: &mut Rig, first_frame: u64) -> [f32; 8] {
    let payload = [0.125_f32, -0.375, 1.25, -0.0, -1.5, 0.625, 0.0, 0.875];
    rig.start(first_frame);
    assert!(rig.rt().append_segment(first_frame, &payload));
    let session_id = rig.id();
    assert!(rig.rt().stop(session_id, first_frame + 4));
    payload
}

#[test]
fn cancellation_winning_before_terminal_decision_is_persisted_as_failed_incomplete() {
    let gate = Arc::new(writer::WorkerPhaseGate::default());
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            before_terminal_decision: Some(gate.clone()),
            ..writer::Hooks::default()
        },
    );
    // Parent RAII release is declared after Rig so unwind cannot strand its worker.
    let release = ReleaseWorkerPhase(gate.clone());
    let first_frame = 3000;
    let payload = seal_original_prefix(&mut rig, first_frame);
    wait_at_worker_phase(&gate);
    let before = rig.status();
    assert!(before.rt_sealed);
    assert!(!before.terminal_decided);
    assert!(!before.worker_closed);
    assert_eq!((before.submitted_frames, before.written_frames), (4, 4));
    assert!(
        rig.control
            .as_ref()
            .unwrap()
            .request_failure(rig.id(), CaptureFailure::Cancelled)
    );
    assert_eq!(rig.status().failure, Some(CaptureFailure::Cancelled));
    assert!(!rig.status().terminal_decided);
    drop(release);
    let status = rig.release_and_join();
    assert!(status.terminal_decided);
    assert_eq!(status.failure, Some(CaptureFailure::Cancelled));
    assert!(!status.successful_spool());
    assert_eq!(
        (status.first_frame, status.end_frame),
        (Some(first_frame), Some(first_frame + 4))
    );
    assert_eq!(
        rig.payload_bits(),
        payload
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
    let inspected = inspect_spool(&rig.path).unwrap();
    assert_eq!(inspected.state, SpoolState::FailedIncomplete);
    assert_eq!(inspected.failure, Some(CaptureFailure::Cancelled));
    assert_eq!(
        (
            inspected.declared_written_frames,
            inspected.physical_whole_frames
        ),
        (4, 4)
    );
    assert!(!inspected.publication_ready);
}

#[test]
fn terminal_decision_winning_before_cancel_rejects_cancel_without_rewriting_closed_result() {
    let gate = Arc::new(writer::WorkerPhaseGate::default());
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            after_terminal_decision: Some(gate.clone()),
            ..writer::Hooks::default()
        },
    );
    let release = ReleaseWorkerPhase(gate.clone());
    let first_frame = 4000;
    let payload = seal_original_prefix(&mut rig, first_frame);
    wait_at_worker_phase(&gate);
    let before = rig.status();
    assert!(before.rt_sealed);
    assert!(before.terminal_decided);
    assert!(!before.worker_closed);
    assert!(!before.rt_released);
    assert_eq!(before.failure, None);
    assert!(
        !rig.control
            .as_ref()
            .unwrap()
            .request_failure(rig.id(), CaptureFailure::Cancelled)
    );
    assert_eq!(rig.status().failure, None);
    assert!(!rig.status().worker_closed);
    drop(release);
    let status = rig.release_and_join();
    assert!(status.terminal_decided);
    assert_eq!(status.failure, None);
    assert!(status.successful_spool());
    assert_eq!(
        (status.first_frame, status.end_frame),
        (Some(first_frame), Some(first_frame + 4))
    );
    assert_eq!((status.submitted_frames, status.written_frames), (4, 4));
    assert_eq!(
        rig.payload_bits(),
        payload
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
    let inspected = inspect_spool(&rig.path).unwrap();
    assert_eq!(inspected.state, SpoolState::SealedCandidate);
    assert_eq!(inspected.failure, None);
    assert!(inspected.requires_verification);
    assert!(!inspected.publication_ready);
    assert!(
        !rig.control
            .as_ref()
            .unwrap()
            .request_failure(status.session_id, CaptureFailure::Stream)
    );
    assert_eq!(rig.status().failure, None);
}

#[test]
fn valid_candidate_header_does_not_override_runtime_error_after_final_checkpoint_tail() {
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            final_checkpoint_tail_failure: true,
            ..writer::Hooks::default()
        },
    );
    let first_frame = 5000;
    let payload = seal_original_prefix(&mut rig, first_frame);
    let status = rig.release_and_join();
    assert!(status.terminal_decided);
    assert_eq!(status.failure, Some(CaptureFailure::WriterIo));
    assert!(!status.successful_spool());
    assert_eq!((status.submitted_frames, status.written_frames), (4, 4));
    assert_eq!(
        rig.payload_bits(),
        payload
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );

    // The explicit fault is an error returned AFTER the real final header's
    // write/sync/seek succeeded. It models uncertain last-I/O outcome; it is not
    // evidence that the operating system actually failed sync or lost bytes.
    let inspected = inspect_spool(&rig.path).unwrap();
    assert_eq!(inspected.state, SpoolState::SealedCandidate);
    assert_eq!(inspected.failure, None);
    assert_eq!(
        (
            inspected.declared_written_frames,
            inspected.physical_whole_frames
        ),
        (4, 4)
    );
    assert_eq!(inspected.declared_end_frame, Some(first_frame + 4));
    assert_eq!(inspected.partial_frame_bytes, 0);
    assert!(inspected.requires_verification);
    assert!(!inspected.publication_ready);
    assert!(
        !rig.control
            .as_ref()
            .unwrap()
            .request_failure(status.session_id, CaptureFailure::Cancelled)
    );
    assert_eq!(rig.status().failure, Some(CaptureFailure::WriterIo));
}
