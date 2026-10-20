//! Actual original-RT/File/ring claim and ACK lifecycle; no group/device acceptance.

use super::*;
use sha2::{Digest, Sha256};

// Hand-authored f32le bytes for +0, -0, +1.25, -2.0. Expected payload bytes do
// not use the productive writer scratch conversion or the start publication.
const GOLDEN_BYTES: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0xa0, 0x3f, 0x00, 0x00, 0x00, 0xc0,
];
const GOLDEN_PCM: [f32; 4] = [0.0, -0.0, 1.25, -2.0];

fn wait_for_phase(gate: &writer::WorkerPhaseGate) {
    let deadline = Instant::now() + WAIT;
    while !gate.reached.load(Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "real writer terminal gate timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn assert_no_effect_ack(status: &CaptureStatus) {
    assert!(!status.started);
    assert_eq!((status.first_frame, status.captured_end), (None, None));
    assert_eq!((status.submitted_end, status.written_end), (None, None));
    assert_eq!(status.end_frame, None);
    assert_eq!((status.submitted_frames, status.written_frames), (0, 0));
}

fn assert_header_failure_codes(path: &Path, expected_latest: u32) {
    let bytes = fs::read(path).unwrap();
    let mut latest = (0, 0);
    for header in bytes[..SPOOL_HEADER_BYTES as usize].chunks_exact(128) {
        let revision = u64::from_le_bytes(header[16..24].try_into().unwrap());
        let failure = u32::from_le_bytes(header[44..48].try_into().unwrap());
        assert_eq!(
            failure & 0xc0,
            0,
            "private permission/final bits reached disk"
        );
        assert!(failure <= CaptureFailure::WorkerPanicked.code().into());
        if revision >= latest.0 {
            latest = (revision, failure);
        }
    }
    assert_eq!(latest.1, expected_latest);
}

#[test]
fn real_preclaim_control_failure_refuses_permission_and_leaves_no_effect_ack_or_payload() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    let session = rig.id();
    assert!(
        rig.control
            .as_ref()
            .unwrap()
            .request_failure(session, CaptureFailure::Cancelled)
    );
    assert!(rig.rt().try_claim_start(session, 71, 48_000, 2).is_none());
    assert_eq!(
        rig.rt().shared.failure.load(Ordering::Acquire) & !FINAL_DECISION,
        CaptureFailure::Cancelled.code()
    );
    assert_no_effect_ack(&rig.status());
    let status = rig.release_and_join();
    assert_no_effect_ack(&status);
    assert!(status.rt_sealed && status.rt_released && status.worker_closed && status.worker_joined);
    assert_eq!(status.failure, Some(CaptureFailure::Cancelled));
    assert!(!status.successful_spool());
    assert!(rig.payload_bits().is_empty());
    let info = inspect_spool(&rig.path).unwrap();
    assert_eq!(info.state, SpoolState::FailedIncomplete);
    assert_eq!(info.failure, Some(CaptureFailure::Cancelled));
    assert_eq!((info.first_frame, info.declared_end_frame), (None, None));
    assert_header_failure_codes(&rig.path, CaptureFailure::Cancelled.code().into());
}

#[test]
fn claim_is_not_an_ack_and_infallible_publication_preserves_actual_golden_writer_payload() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    let session = rig.id();
    let observer = rig.control.as_ref().unwrap().observer();
    let shared = rig.rt().shared.clone();
    let claim = rig.rt().try_claim_start(session, 73, 48_000, 2).unwrap();
    assert_eq!(shared.failure.load(Ordering::Acquire), START_CLAIMED);
    let permitted = observer.status();
    assert_no_effect_ack(&permitted);
    assert!(permitted.writer_ready && !permitted.rt_sealed && !permitted.worker_closed);
    assert_eq!(permitted.failure, None);
    // The live token exclusively borrows the actual RT owner: append, stop,
    // another claim or publication on a foreign owner are unrepresentable.
    assert_no_effect_ack(&observer.status());

    claim.publish();

    assert_eq!(rig.status().first_frame, Some(73));
    assert!(rig.status().started);
    assert!(!rig.rt().start(session, 99, 48_000, 2));
    assert!(rig.rt().append_segment(73, &GOLDEN_PCM[..2]));
    assert!(rig.rt().append_segment(74, &GOLDEN_PCM[2..]));
    let status = rig.finish(75);
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(73), Some(75)));
    assert_eq!((status.submitted_frames, status.written_frames), (2, 2));
    assert_eq!(
        status.payload_sha256,
        Some(Sha256::digest(GOLDEN_BYTES).into())
    );
    assert_eq!(
        shared.failure.load(Ordering::Acquire),
        START_CLAIMED | FINAL_DECISION
    );
    let file = fs::read(&rig.path).unwrap();
    assert_eq!(&file[SPOOL_HEADER_BYTES as usize..], GOLDEN_BYTES);
    let info = inspect_spool(&rig.path).unwrap();
    assert_eq!(info.state, SpoolState::SealedCandidate);
    assert_eq!(info.failure, None);
    assert_eq!(
        (info.first_frame, info.declared_end_frame),
        (Some(73), Some(75))
    );
    assert!(info.requires_verification && !info.publication_ready);
    assert_header_failure_codes(&rig.path, 0);
}

#[test]
fn public_control_failure_after_claim_uses_preserving_retry_and_cannot_revoke_publication() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    let session = rig.id();
    let observer = rig.control.as_ref().unwrap().observer();
    let shared = rig.rt().shared.clone();
    // Borrow disjoint real control/RT fields. The control's atomic failure
    // request remains available while the token retains the exclusive RT borrow.
    let control = rig.control.as_ref().unwrap();
    let claim = rig
        .rt
        .as_mut()
        .unwrap()
        .try_claim_start(session, 79, 48_000, 2)
        .unwrap();
    assert_eq!(shared.failure.load(Ordering::Acquire), START_CLAIMED);
    // The real public control primitive first attempts CAS(0, Cancelled). The
    // actual permission forces that CAS to lose and its bounded retry to run.
    assert!(control.request_failure(session, CaptureFailure::Cancelled));
    assert_eq!(
        shared.failure.load(Ordering::Acquire),
        START_CLAIMED | CaptureFailure::Cancelled.code()
    );
    assert_eq!(observer.status().failure, Some(CaptureFailure::Cancelled));
    assert_no_effect_ack(&observer.status());
    assert!(!observer.status().rt_sealed);
    assert!(control.request_failure(session, CaptureFailure::Stream));
    assert_eq!(observer.status().failure, Some(CaptureFailure::Cancelled));

    claim.publish();

    let effective = rig.status();
    assert!(effective.started);
    assert_eq!(effective.first_frame, Some(79));
    assert_eq!(effective.failure, Some(CaptureFailure::Cancelled));
    assert!(!rig.rt().append_segment(79, &GOLDEN_PCM));
    let status = rig.release_and_join();
    assert_eq!((status.first_frame, status.end_frame), (Some(79), Some(79)));
    assert_eq!(status.failure, Some(CaptureFailure::Cancelled));
    assert_eq!(status.payload_sha256, None);
    assert!(status.empty() && !status.successful_spool());
    assert_eq!(
        shared.failure.load(Ordering::Acquire),
        START_CLAIMED | FINAL_DECISION | CaptureFailure::Cancelled.code()
    );
    assert!(
        !rig.control
            .as_ref()
            .unwrap()
            .request_failure(session, CaptureFailure::Stream)
    );
    assert!(rig.payload_bits().is_empty());
    let info = inspect_spool(&rig.path).unwrap();
    assert_eq!(info.state, SpoolState::FailedIncomplete);
    assert_eq!(info.failure, Some(CaptureFailure::Cancelled));
    assert_header_failure_codes(&rig.path, CaptureFailure::Cancelled.code().into());
}

#[test]
fn actual_late_writer_io_preserves_claim_and_terminal_bits_without_reopening_control_failure() {
    let gate = Arc::new(writer::WorkerPhaseGate::default());
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            after_terminal_decision: Some(gate.clone()),
            final_checkpoint_tail_failure: true,
            ..writer::Hooks::default()
        },
    );
    let release = ReleaseWorkerPhase(gate.clone());
    rig.start(83);
    let shared = rig.rt().shared.clone();
    assert!(rig.rt().append_segment(83, &GOLDEN_PCM));
    let session = rig.id();
    assert!(rig.rt().stop(session, 85));
    wait_for_phase(&gate);
    assert_eq!(
        shared.failure.load(Ordering::Acquire),
        START_CLAIMED | FINAL_DECISION
    );
    assert!(!rig.status().worker_closed);
    assert_eq!(rig.status().failure, None);
    assert!(
        !rig.control
            .as_ref()
            .unwrap()
            .request_failure(session, CaptureFailure::Cancelled)
    );
    assert_eq!(rig.status().failure, None);
    drop(release);
    let status = rig.release_and_join();
    assert_eq!(status.failure, Some(CaptureFailure::WriterIo));
    assert!(status.terminal_decided && status.worker_closed && status.worker_joined);
    assert_eq!(
        shared.failure.load(Ordering::Acquire),
        START_CLAIMED | FINAL_DECISION | CaptureFailure::WriterIo.code()
    );
    assert_eq!(
        status.payload_sha256,
        Some(Sha256::digest(GOLDEN_BYTES).into())
    );
    assert!(!status.successful_spool());
    // Actual final header I/O completed before the injected tail returned an
    // error; the unchanged candidate is explicitly weaker than live status.
    let info = inspect_spool(&rig.path).unwrap();
    assert_eq!(info.state, SpoolState::SealedCandidate);
    assert_eq!(info.failure, None);
    assert_header_failure_codes(&rig.path, 0);
}

#[test]
fn original_control_failure_survives_actual_later_writer_error_and_final_decision() {
    let gate = Arc::new(writer::WorkerPhaseGate::default());
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            before_terminal_decision: Some(gate.clone()),
            final_checkpoint_tail_failure: true,
            ..writer::Hooks::default()
        },
    );
    let release = ReleaseWorkerPhase(gate.clone());
    rig.start(89);
    let shared = rig.rt().shared.clone();
    assert!(rig.rt().append_segment(89, &GOLDEN_PCM));
    let session = rig.id();
    assert!(rig.rt().stop(session, 91));
    wait_for_phase(&gate);
    assert!(
        rig.control
            .as_ref()
            .unwrap()
            .request_failure(session, CaptureFailure::Cancelled)
    );
    drop(release);
    let status = rig.release_and_join();
    assert_eq!(status.failure, Some(CaptureFailure::Cancelled));
    assert_eq!(
        shared.failure.load(Ordering::Acquire),
        START_CLAIMED | FINAL_DECISION | CaptureFailure::Cancelled.code()
    );
    assert_eq!(status.payload_sha256, None);
    assert!(
        rig.control
            .as_ref()
            .unwrap()
            .observer()
            .error_detail()
            .unwrap()
            .contains("injected final checkpoint tail I/O failure")
    );
    let info = inspect_spool(&rig.path).unwrap();
    assert_eq!(info.state, SpoolState::FailedIncomplete);
    assert_eq!(info.failure, Some(CaptureFailure::Cancelled));
    assert_header_failure_codes(&rig.path, CaptureFailure::Cancelled.code().into());
}

#[test]
fn foreign_session_geometry_and_sealed_owner_guards_precede_any_permission() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    let session = rig.id();
    assert!(
        rig.rt()
            .try_claim_start(session + 1, 97, 48_000, 2)
            .is_none()
    );
    assert_eq!(rig.rt().shared.failure.load(Ordering::Acquire), 0);
    assert_no_effect_ack(&rig.status());
    assert!(!rig.status().rt_sealed);
    assert!(rig.rt().try_claim_start(session, 97, 44_100, 2).is_none());
    assert_eq!(
        rig.rt().shared.failure.load(Ordering::Acquire) & !FINAL_DECISION,
        CaptureFailure::Geometry.code()
    );
    assert!(rig.rt().try_claim_start(session, 97, 48_000, 2).is_none());
    assert!(!rig.rt().start(session, 97, 48_000, 2));
    let status = rig.release_and_join();
    assert_eq!(status.failure, Some(CaptureFailure::Geometry));
    assert_no_effect_ack(&status);
    assert!(rig.payload_bits().is_empty());
    assert_header_failure_codes(&rig.path, CaptureFailure::Geometry.code().into());
}

#[test]
fn abandoned_unpublished_permission_does_not_fabricate_ack_or_allow_a_new_claim() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    let session = rig.id();
    let observer = rig.control.as_ref().unwrap().observer();
    {
        let _claim = rig.rt().try_claim_start(session, 101, 48_000, 2).unwrap();
        assert_no_effect_ack(&observer.status());
    }
    assert!(rig.rt().try_claim_start(session, 103, 48_000, 2).is_none());
    assert_no_effect_ack(&rig.status());
    let status = rig.release_and_join();
    assert_eq!(status.failure, Some(CaptureFailure::Abandoned));
    assert_no_effect_ack(&status);
    assert!(rig.payload_bits().is_empty());
    assert_header_failure_codes(&rig.path, CaptureFailure::Abandoned.code().into());
}

#[test]
fn claim_publication_and_real_ring_submission_have_no_current_thread_allocator_work() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    let session = rig.id();
    let ((claimed, appended, stopped), observed) =
        crate::test_memory::observe_current_thread(|| {
            let rt = rig.rt();
            let Some(claim) = rt.try_claim_start(session, 107, 48_000, 2) else {
                return (false, false, false);
            };
            claim.publish();
            (
                true,
                rt.append_segment(107, &GOLDEN_PCM),
                rt.stop(session, 109),
            )
        });
    assert!(claimed && appended && stopped);
    assert_eq!(observed, crate::test_memory::ThreadAllocations::default());
    assert!(rig.release_and_join().successful_spool());
    let bytes = fs::read(&rig.path).unwrap();
    assert_eq!(&bytes[SPOOL_HEADER_BYTES as usize..], GOLDEN_BYTES);
}
