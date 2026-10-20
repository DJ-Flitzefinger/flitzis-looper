//! Original File/ring writer byte witnesses; no checkpoint or final-file authority.

use super::*;
use sha2::{Digest, Sha256};

// Explicit f32le golden bytes, independent of the writer's scratch encoding:
// -0, +0, +1.25, -2, +0.375, -0.75, +0.125, -0.25.
const GOLDEN: [u8; 32] = [
    0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa0, 0x3f, 0x00, 0x00, 0x00, 0xc0,
    0x00, 0x00, 0xc0, 0x3e, 0x00, 0x00, 0x40, 0xbf, 0x00, 0x00, 0x00, 0x3e, 0x00, 0x00, 0x80, 0xbe,
];

fn golden_pcm() -> [f32; 8] {
    [-0.0, 0.0, 1.25, -2.0, 0.375, -0.75, 0.125, -0.25]
}

fn golden_hash() -> [u8; 32] {
    Sha256::digest(GOLDEN).into()
}

#[test]
fn original_writer_hashes_exact_signed_zero_and_overrange_golden_bytes() {
    let mut geometry = config();
    geometry.block_frames = 2;
    let mut rig = Rig::new(geometry, writer::Hooks::default());
    assert_eq!(rig.status().payload_sha256, None);
    let pcm = golden_pcm();
    rig.start(1000);
    assert!(rig.rt().append_segment(1000, &pcm[..2]));
    assert!(rig.rt().append_segment(1001, &pcm[2..6]));
    assert!(rig.rt().append_segment(1003, &pcm[6..]));
    assert_eq!(rig.status().payload_sha256, None);

    let status = rig.finish(1004);

    assert!(status.successful_spool());
    assert!(status.worker_closed && status.worker_joined);
    assert_eq!((status.submitted_blocks, status.written_blocks), (2, 2));
    assert_eq!(status.payload_sha256, Some(golden_hash()));
    let file = fs::read(&rig.path).unwrap();
    assert_eq!(&file[SPOOL_HEADER_BYTES as usize..], GOLDEN);
}

#[test]
fn original_writer_hash_covers_every_full_and_partial_block_in_native_order() {
    let mut geometry = config();
    geometry.block_frames = 3;
    let mut rig = Rig::new(
        geometry,
        writer::Hooks {
            gate: Some(Arc::new(AtomicBool::new(false))),
            ..writer::Hooks::default()
        },
    );
    let pcm = golden_pcm().repeat(4);
    rig.start(2000);
    assert!(rig.rt().append_segment(2000, &pcm[..2]));
    assert!(rig.rt().append_segment(2001, &pcm[2..12]));
    assert!(rig.rt().append_segment(2006, &pcm[12..]));
    assert_eq!(rig.status().payload_sha256, None);

    let status = rig.finish(2016);

    let expected: [u8; 32] = Sha256::digest(GOLDEN.repeat(4)).into();
    assert_eq!(status.payload_sha256, Some(expected));
    assert_ne!(expected, golden_hash());
    assert_eq!((status.submitted_blocks, status.written_blocks), (6, 6));
    assert_eq!((status.submitted_frames, status.written_frames), (16, 16));
    assert!(status.successful_spool());
    let file = fs::read(&rig.path).unwrap();
    assert_eq!(&file[SPOOL_HEADER_BYTES as usize..], GOLDEN.repeat(4));
}

#[test]
fn synced_payload_is_not_published_as_a_witness_before_terminal_decision() {
    let gate = Arc::new(writer::WorkerPhaseGate::default());
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            before_terminal_decision: Some(gate.clone()),
            ..writer::Hooks::default()
        },
    );
    let release = ReleaseWorkerPhase(gate.clone());
    rig.start(3000);
    assert!(rig.rt().append_segment(3000, &golden_pcm()));
    let session = rig.id();
    assert!(rig.rt().stop(session, 3004));
    let deadline = Instant::now() + WAIT;
    while !gate.reached.load(Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "writer did not reach synced payload gate"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let active = rig.status();
    assert_eq!(active.written_frames, 4);
    assert!(active.rt_sealed);
    assert!(!active.terminal_decided);
    assert!(!active.worker_closed);
    assert_eq!(active.payload_sha256, None);
    assert!(!active.successful_spool());

    drop(release);
    let closed = rig.release_and_join();
    assert_eq!(closed.payload_sha256, Some(golden_hash()));
    assert!(closed.successful_spool());
}

#[test]
fn interrupted_partial_write_cannot_publish_a_complete_payload_witness() {
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            gate: Some(Arc::new(AtomicBool::new(false))),
            fail_after_payload_bytes: Some(36),
            ..writer::Hooks::default()
        },
    );
    let pcm = golden_pcm().repeat(3);
    rig.start(4000);
    assert!(rig.rt().append_segment(4000, &pcm));

    let status = rig.finish(4012);

    assert_eq!(status.failure, Some(CaptureFailure::WriterIo));
    assert_eq!((status.submitted_frames, status.written_frames), (12, 4));
    assert_eq!(status.payload_sha256, None);
    assert!(!status.successful_spool());
    let file = fs::read(&rig.path).unwrap();
    assert_eq!(&file[SPOOL_HEADER_BYTES as usize..], GOLDEN);
    let checkpoint = inspect_spool(&rig.path).unwrap();
    assert_eq!(checkpoint.state, SpoolState::FailedIncomplete);
    assert_eq!(checkpoint.physical_whole_frames, 4);
}

#[test]
fn later_header_tail_io_failure_still_rejects_a_valid_original_payload_witness() {
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            final_checkpoint_tail_failure: true,
            ..writer::Hooks::default()
        },
    );
    rig.start(5000);
    assert!(rig.rt().append_segment(5000, &golden_pcm()));
    let status = rig.finish(5004);

    assert!(status.terminal_decided && status.worker_closed && status.worker_joined);
    assert_eq!(status.payload_sha256, Some(golden_hash()));
    assert_eq!(status.failure, Some(CaptureFailure::WriterIo));
    assert!(!status.successful_spool());
    let checkpoint = inspect_spool(&rig.path).unwrap();
    assert_eq!(checkpoint.state, SpoolState::SealedCandidate);
    assert_eq!(checkpoint.failure, None);
    assert!(checkpoint.requires_verification);
    assert!(!checkpoint.publication_ready);
}

#[test]
fn empty_or_unstarted_retired_capture_has_no_successful_payload_witness() {
    let mut empty = Rig::new(config(), writer::Hooks::default());
    empty.start(6000);
    let status = empty.finish(6000);
    assert!(status.empty());
    assert_eq!(status.payload_sha256, None);
    assert!(!status.successful_spool());

    let mut unstarted = Rig::new(config(), writer::Hooks::default());
    let status = unstarted.release_and_join();
    assert!(!status.started);
    assert_eq!(status.payload_sha256, None);
    assert!(!status.successful_spool());
}

#[test]
fn finite_in_place_postclose_edit_cannot_rewrite_original_live_writer_hash() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    rig.start(7000);
    assert!(rig.rt().append_segment(7000, &golden_pcm()));
    let status = rig.finish(7004);
    assert!(status.successful_spool());
    assert_eq!(status.payload_sha256, Some(golden_hash()));
    let original_identity =
        crate::audio_engine::project_assets::capture_identity(&rig.path).unwrap();
    assert!(original_identity.is_some());

    // Exact same opened file and size, ordinary finite replacement sample. The
    // checkpoint still agrees; only the original worker's byte witness differs.
    let mut file = fs::OpenOptions::new().write(true).open(&rig.path).unwrap();
    file.seek(SeekFrom::Start(SPOOL_HEADER_BYTES)).unwrap();
    file.write_all(&0.5_f32.to_le_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    assert_eq!(
        crate::audio_engine::project_assets::capture_identity(&rig.path).unwrap(),
        original_identity
    );
    let edited = fs::read(&rig.path).unwrap();
    let edited_hash: [u8; 32] = Sha256::digest(&edited[SPOOL_HEADER_BYTES as usize..]).into();
    assert_ne!(edited_hash, golden_hash());
    assert_eq!(rig.status().payload_sha256, Some(golden_hash()));
    let checkpoint = inspect_spool(&rig.path).unwrap();
    assert_eq!(checkpoint.state, SpoolState::SealedCandidate);
    assert_eq!(checkpoint.physical_whole_frames, 4);
    assert_eq!(checkpoint.partial_frame_bytes, 0);
    assert!(!checkpoint.publication_ready);
}
