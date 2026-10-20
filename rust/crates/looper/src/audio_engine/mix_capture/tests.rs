//! Module-only File/ring/session proofs; productive scheduler/mixer acceptance is separate.

use super::*;

#[path = "tests/payload_hash_tests.rs"]
mod payload_hash_tests;
#[path = "tests/start_claim_tests.rs"]
mod start_claim_tests;
#[path = "tests/terminal_tests.rs"]
mod terminal_tests;

#[test]
#[ignore = "run once for actual writer-to-Python inspector evidence; retains one owned small spool"]
fn retain_real_closed_spool_for_independent_python_ffi_inspection() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    rig.start(1234);
    let pcm = [
        -0.0_f32, 0.0, 1.25, -2.0, 0.375, -0.75, 0.125, -0.25, 0.5, 0.25,
    ];
    assert!(rig.rt().append_segment(1234, &pcm[..4]));
    assert!(rig.rt().append_segment(1236, &pcm[4..]));
    let status = rig.finish(1239);
    assert!(status.successful_spool());
    assert_eq!(
        rig.payload_bits(),
        pcm.iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
    let retained = rig.directory.take().unwrap().keep();
    println!(
        "CAPTURE_FFI_SPOOL={}",
        retained.join("take.capture").display()
    );
}
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(5);

fn config() -> CaptureConfig {
    CaptureConfig {
        sample_rate_hz: 48_000,
        channels: 2,
        block_frames: 4,
        block_count: 8,
        max_pool_pcm_bytes: 4096,
    }
}

struct Rig {
    rt: Option<Box<CaptureRt>>,
    control: Option<CaptureControl>,
    directory: Option<tempfile::TempDir>,
    path: PathBuf,
}

impl Rig {
    fn new(config: CaptureConfig, hooks: writer::Hooks) -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let parent = workspace.join("test-artifacts/record-capture-20261020");
        fs::create_dir_all(&parent).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("core-")
            .tempdir_in(parent)
            .unwrap();
        fs::write(directory.path().join("OWNER.md"),
            b"Owned native mix-capture core test: synthetic float samples, real exclusive spool/rings/writer, no Human inputs or device. Drop only after actual RT release and worker join; failed unsettled fixtures remain retained. Module proof is not productive scheduler/mixer/format acceptance.\n").unwrap();
        let path = directory.path().join("take.capture");
        let (control, rt) = prepare_inner(&path, config, hooks).unwrap();
        let deadline = Instant::now() + WAIT;
        while !control.status().writer_ready {
            assert!(
                Instant::now() < deadline,
                "capture writer never became ready"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        Self {
            rt: Some(rt),
            control: Some(control),
            directory: Some(directory),
            path,
        }
    }

    fn id(&self) -> u64 {
        self.rt.as_ref().unwrap().session_id()
    }
    fn rt(&mut self) -> &mut CaptureRt {
        self.rt.as_mut().unwrap()
    }
    fn status(&self) -> CaptureStatus {
        self.control.as_ref().unwrap().status()
    }

    fn start(&mut self, frame: u64) {
        let id = self.id();
        let config = self.rt().config();
        assert!(
            self.rt()
                .start(id, frame, config.sample_rate_hz, config.channels)
        );
    }

    fn finish(&mut self, end: u64) -> CaptureStatus {
        let id = self.id();
        assert!(self.rt().stop(id, end));
        self.release_and_join()
    }

    fn release_and_join(&mut self) -> CaptureStatus {
        drop(self.rt.take()); // Simulated existing off-RT retirement consumer.
        let deadline = Instant::now() + WAIT;
        loop {
            if self.control.as_mut().unwrap().join_if_finished().unwrap() {
                break;
            }
            assert!(Instant::now() < deadline, "capture writer did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
        let status = self.status();
        assert!(
            status.rt_released && status.rt_sealed && status.worker_closed && status.worker_joined
        );
        status
    }

    fn payload_bits(&self) -> Vec<u32> {
        let bytes = fs::read(&self.path).unwrap();
        bytes[SPOOL_HEADER_BYTES as usize..]
            .chunks_exact(4)
            .map(|sample| u32::from_le_bytes(sample.try_into().unwrap()))
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(mut rt) = self.rt.take() {
            let id = rt.session_id();
            if !rt.terminal() {
                rt.fail(id, CaptureFailure::Abandoned);
            }
            drop(rt);
        }
        let deadline = Instant::now() + WAIT;
        let mut settled = self.control.is_none();
        while let Some(control) = &mut self.control {
            match control.join_if_finished() {
                Ok(true) => {
                    settled = true;
                    break;
                }
                Err(_) => break,
                Ok(false) if Instant::now() >= deadline => break,
                Ok(false) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        if !settled {
            if let Some(directory) = self.directory.take() {
                eprintln!(
                    "unsettled capture core fixture retained: {}",
                    directory.keep().display()
                );
            }
        }
    }
}

#[test]
fn irregular_segments_preserve_exact_float_bits_and_exclusive_native_extent() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    let input: Vec<f32> = (0..18).map(|index| (index as f32 - 7.0) * 0.375).collect();
    let mut input = input;
    input[0] = -0.0;
    let original: Vec<_> = input.iter().map(|sample| sample.to_bits()).collect();
    rig.start(100);
    assert!(rig.rt().append_segment(100, &input[..6]));
    assert!(rig.rt().append_segment(103, &input[6..16]));
    assert!(rig.rt().append_segment(108, &input[16..]));
    let status = rig.finish(109);
    assert!(status.successful_spool());
    assert_eq!(
        (status.first_frame, status.end_frame),
        (Some(100), Some(109))
    );
    assert_eq!(
        (status.submitted_end, status.written_end),
        (Some(109), Some(109))
    );
    assert_eq!((status.submitted_frames, status.written_frames), (9, 9));
    assert_eq!((status.submitted_blocks, status.written_blocks), (3, 3));
    assert_eq!(rig.payload_bits(), original);
    assert_eq!(
        input
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        original
    );
    let recovered = inspect_spool(&rig.path).unwrap();
    assert_eq!(recovered.state, SpoolState::SealedCandidate);
    assert_eq!(recovered.first_frame, Some(100));
    assert_eq!(recovered.declared_end_frame, Some(109));
    assert_eq!(
        (
            recovered.declared_written_frames,
            recovered.physical_whole_frames
        ),
        (9, 9)
    );
    assert_eq!(recovered.partial_frame_bytes, 0);
}

#[test]
fn thousands_of_one_frame_segments_share_partial_blocks_without_fragmentation_overflow() {
    use crate::test_memory::{ThreadAllocations, observe_current_thread};
    let mut compact = config();
    compact.block_frames = 2048;
    compact.block_count = 2;
    compact.max_pool_pcm_bytes = 65_536;
    let mut rig = Rig::new(
        compact,
        writer::Hooks {
            gate: Some(Arc::new(AtomicBool::new(false))),
            fail_after_payload_bytes: None,
            ..writer::Hooks::default()
        },
    );
    rig.start(10_000);
    let frame = [0.125_f32, -0.375];
    let (accepted, allocations) = observe_current_thread(|| {
        (0..2051).all(|offset| rig.rt().append_segment(10_000 + offset, &frame))
    });
    assert!(accepted);
    assert_eq!(allocations, ThreadAllocations::default());
    let buffered = rig.status();
    assert_eq!(buffered.failure, None);
    assert_eq!(buffered.captured_end, Some(12_051));
    assert_eq!(buffered.submitted_end, Some(12_048));
    assert_eq!(buffered.buffered_frames, 3);
    assert_eq!(buffered.submitted_blocks, 1);
    let status = rig.finish(12_051);
    assert!(status.successful_spool());
    assert_eq!((status.submitted_blocks, status.written_blocks), (2, 2));
    assert_eq!(status.buffered_frames, 0);
    assert_eq!(status.written_frames, 2051);
    let expected: Vec<_> = frame
        .into_iter()
        .map(f32::to_bits)
        .cycle()
        .take(4102)
        .collect();
    assert_eq!(rig.payload_bits(), expected);
}

#[test]
fn stalled_writer_overflow_is_sticky_independent_of_the_full_data_ring() {
    let mut limited = config();
    limited.block_count = 2;
    let mut rig = Rig::new(
        limited,
        writer::Hooks {
            gate: Some(Arc::new(AtomicBool::new(false))),
            fail_after_payload_bytes: None,
            ..writer::Hooks::default()
        },
    );
    rig.start(40);
    let block = [0.25_f32; 8];
    assert!(rig.rt().append_segment(40, &block));
    assert!(rig.rt().append_segment(44, &block));
    assert!(!rig.rt().append_segment(48, &block));
    let before_drop = rig.status();
    assert_eq!(before_drop.failure, Some(CaptureFailure::Overflow));
    assert!(before_drop.rt_sealed && !before_drop.rt_released);
    assert_eq!(before_drop.end_frame, Some(48));
    assert_eq!(before_drop.submitted_frames, 8);
    let status = rig.release_and_join();
    assert_eq!(status.failure, Some(CaptureFailure::Overflow));
    assert!(!status.successful_spool());
    assert_eq!((status.submitted_frames, status.written_frames), (8, 8));
    assert_eq!(rig.payload_bits(), vec![0.25_f32.to_bits(); 16]);
    let recovered = inspect_spool(&rig.path).unwrap();
    assert_eq!(recovered.state, SpoolState::FailedIncomplete);
    assert_eq!(recovered.failure, Some(CaptureFailure::Overflow));
    assert_eq!(recovered.physical_whole_frames, 8);
}

#[test]
fn nonfinite_or_discontinuous_input_cannot_publish_a_gapped_success() {
    for failure in [CaptureFailure::NonFinite, CaptureFailure::Discontinuous] {
        let mut rig = Rig::new(config(), writer::Hooks::default());
        rig.start(200);
        let prefix = [1.25_f32, -1.5, 0.0, -0.0, 0.25, 0.5, -0.75, 0.875];
        assert!(rig.rt().append_segment(200, &prefix));
        if failure == CaptureFailure::NonFinite {
            let bad = [f32::NAN; 2];
            assert!(!rig.rt().append_segment(204, &bad));
            assert!(bad[0].is_nan());
        } else {
            assert!(!rig.rt().append_segment(205, &[0.5; 2]));
        }
        let status = rig.release_and_join();
        assert_eq!(status.failure, Some(failure));
        assert_eq!((status.end_frame, status.written_frames), (Some(204), 4));
        assert!(!status.successful_spool());
        assert_eq!(rig.payload_bits(), prefix.map(f32::to_bits));
        assert_eq!(inspect_spool(&rig.path).unwrap().physical_whole_frames, 4);
    }
}

#[test]
fn partial_payload_write_retains_only_successful_full_block_prefix() {
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            gate: Some(Arc::new(AtomicBool::new(false))),
            fail_after_payload_bytes: Some(36),
            ..writer::Hooks::default()
        },
    );
    rig.start(300);
    assert!(rig.rt().append_segment(300, &[0.125; 24]));
    let status = rig.finish(312);
    assert_eq!(status.failure, Some(CaptureFailure::WriterIo));
    assert_eq!((status.submitted_frames, status.written_frames), (12, 4));
    assert_eq!(
        (status.end_frame, status.written_end),
        (Some(312), Some(304))
    );
    assert!(!status.successful_spool());
    assert!(
        rig.control
            .as_ref()
            .unwrap()
            .observer()
            .error_detail()
            .unwrap()
            .contains("partial")
    );
    assert_eq!(rig.payload_bits(), vec![0.125_f32.to_bits(); 8]);
    let recovered = inspect_spool(&rig.path).unwrap();
    assert_eq!(recovered.state, SpoolState::FailedIncomplete);
    assert_eq!(
        (
            recovered.declared_submitted_frames,
            recovered.declared_written_frames
        ),
        (12, 4)
    );
    assert_eq!(
        (
            recovered.physical_whole_frames,
            recovered.partial_frame_bytes
        ),
        (4, 0)
    );
}

#[test]
fn foreign_session_stop_failure_and_busy_start_cannot_change_the_original_take() {
    let mut first = Rig::new(config(), writer::Hooks::default());
    let mut next = Rig::new(config(), writer::Hooks::default());
    let first_id = first.id();
    let next_id = next.id();
    assert!(next_id > first_id);
    first.start(500);
    assert!(!first.rt().start(first_id, 900, 48_000, 2));
    assert!(!first.rt().stop(next_id, 500));
    assert!(!first.rt().fail(next_id, CaptureFailure::Cancelled));
    assert!(
        !first
            .control
            .as_ref()
            .unwrap()
            .request_failure(next_id, CaptureFailure::Stream)
    );
    assert_eq!(first.status().first_frame, Some(500));
    assert_eq!(first.status().failure, None);
    assert!(!first.status().rt_sealed);
    assert!(first.rt().append_segment(500, &[0.3; 2]));
    assert!(first.finish(501).successful_spool());
    next.start(900);
    assert!(!next.rt().stop(first_id, 900));
    assert!(next.rt().append_segment(900, &[0.7; 2]));
    assert!(next.finish(901).successful_spool());
}

#[test]
fn stream_signal_is_visible_without_another_render_but_cannot_fake_quiescence() {
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            gate: Some(Arc::new(AtomicBool::new(false))),
            fail_after_payload_bytes: None,
            ..writer::Hooks::default()
        },
    );
    rig.start(600);
    assert!(rig.rt().append_segment(600, &[0.4; 8]));
    assert!(
        rig.control
            .as_ref()
            .unwrap()
            .request_failure(rig.id(), CaptureFailure::Stream)
    );
    let pending = rig.status();
    assert_eq!(pending.failure, Some(CaptureFailure::Stream));
    assert!(!pending.rt_sealed && !pending.worker_closed && !pending.rt_released);
    assert!(!rig.control.as_mut().unwrap().join_if_finished().unwrap());
    // The callback-close/retirement owner can quiesce without another render.
    let status = rig.release_and_join();
    assert_eq!(status.failure, Some(CaptureFailure::Stream));
    assert_eq!((status.end_frame, status.written_frames), (Some(604), 4));
    assert!(!status.successful_spool());
}

#[test]
fn control_cancel_requires_rt_observation_then_drains_the_accepted_prefix() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    rig.start(700);
    assert!(rig.rt().append_segment(700, &[0.2; 6]));
    assert!(
        rig.control
            .as_ref()
            .unwrap()
            .request_failure(rig.id(), CaptureFailure::Cancelled)
    );
    assert!(!rig.status().rt_sealed);
    assert!(rig.rt().observe_failure());
    assert!(!rig.rt().append_segment(703, &[0.9; 2]));
    let status = rig.release_and_join();
    assert_eq!(status.failure, Some(CaptureFailure::Cancelled));
    assert_eq!(
        (status.end_frame, status.written_end),
        (Some(703), Some(703))
    );
    assert_eq!(rig.payload_bits(), vec![0.2_f32.to_bits(); 6]);
}

#[test]
fn retiring_an_unstopped_rt_owner_marks_abandonment_and_preserves_real_prefix() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    rig.start(750);
    assert!(rig.rt().append_segment(750, &[0.15; 6]));
    let observer = rig.control.as_ref().unwrap().observer();
    // No manufactured stop/ACK: the actual off-RT work destructor seals this prefix.
    let status = rig.release_and_join();
    assert_eq!(status.failure, Some(CaptureFailure::Abandoned));
    assert_eq!((status.end_frame, status.written_frames), (Some(753), 3));
    assert!(!status.successful_spool());
    assert!(observer.status().rt_released);
    let recovered = inspect_spool(&rig.path).unwrap();
    assert_eq!(recovered.state, SpoolState::FailedIncomplete);
    assert_eq!(recovered.failure, Some(CaptureFailure::Abandoned));
    assert_eq!(recovered.physical_whole_frames, 3);
}

#[test]
fn stop_ack_and_worker_join_do_not_claim_rt_pool_release() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    rig.start(800);
    let id = rig.id();
    assert!(rig.rt().stop(id, 800));
    let deadline = Instant::now() + WAIT;
    while !rig.control.as_mut().unwrap().join_if_finished().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let held = rig.status();
    assert!(held.worker_closed && held.worker_joined && held.rt_sealed);
    assert!(!held.rt_released);
    assert!(held.empty());
    assert!(!held.successful_spool());
    let released = rig.release_and_join();
    assert!(released.rt_released);
    assert_eq!(fs::metadata(&rig.path).unwrap().len(), SPOOL_HEADER_BYTES);
}

#[test]
fn geometry_and_absolute_frame_overflow_fail_before_any_pcm_is_submitted() {
    let mut wrong = Rig::new(config(), writer::Hooks::default());
    let wrong_id = wrong.id();
    assert!(!wrong.rt().start(wrong_id, 0, 44_100, 2));
    let status = wrong.release_and_join();
    assert_eq!(status.failure, Some(CaptureFailure::Geometry));
    assert_eq!(status.submitted_frames, 0);
    let mut overflow = Rig::new(config(), writer::Hooks::default());
    overflow.start(u64::MAX - 1);
    assert!(!overflow.rt().append_segment(u64::MAX - 1, &[0.5; 4]));
    let status = overflow.release_and_join();
    assert_eq!(status.failure, Some(CaptureFailure::FrameOverflow));
    assert_eq!(status.end_frame, Some(u64::MAX - 1));
    assert_eq!(status.written_frames, 0);
}

#[test]
fn pool_and_geometry_rejection_precede_file_creation_and_existing_spools_are_untouched() {
    let rig = Rig::new(config(), writer::Hooks::default());
    for invalid in [
        CaptureConfig {
            sample_rate_hz: 0,
            ..config()
        },
        CaptureConfig {
            channels: 0,
            ..config()
        },
        CaptureConfig {
            channels: usize::MAX,
            ..config()
        },
        CaptureConfig {
            block_frames: usize::MAX,
            ..config()
        },
        CaptureConfig {
            block_count: usize::MAX,
            ..config()
        },
        CaptureConfig {
            max_pool_pcm_bytes: 1,
            ..config()
        },
    ] {
        let invalid_path = rig.path.with_file_name("invalid.capture");
        assert!(prepare(&invalid_path, invalid).is_err());
        assert!(!invalid_path.exists());
    }
    assert!(prepare(Path::new("relative.capture"), config()).is_err());
    let sentinel = rig.path.with_file_name("existing.capture");
    fs::write(&sentinel, b"unknown original sentinel").unwrap();
    assert!(prepare(&sentinel, config()).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"unknown original sentinel");
}

#[test]
fn torn_latest_header_recovers_previous_metadata_without_loading_or_repairing_payload() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    rig.start(1000);
    assert!(rig.rt().append_segment(1000, &[0.6; 8]));
    rig.finish(1004);
    let final_header = inspect_spool(&rig.path).unwrap();
    assert_eq!(final_header.revision, 2);
    let mut file = fs::OpenOptions::new().write(true).open(&rig.path).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(&[0; 17]).unwrap();
    file.sync_all().unwrap();
    let recovered = inspect_spool(&rig.path).unwrap();
    assert_eq!(recovered.revision, 1);
    assert_eq!(recovered.state, SpoolState::RecordingIncomplete);
    assert_eq!(recovered.first_frame, Some(1000));
    assert_eq!(recovered.physical_whole_frames, 4);
    file.seek(SeekFrom::End(0)).unwrap();
    file.write_all(&[9, 8, 7]).unwrap();
    file.sync_all().unwrap();
    let interrupted = inspect_spool(&rig.path).unwrap();
    assert_eq!(
        (
            interrupted.physical_whole_frames,
            interrupted.partial_frame_bytes
        ),
        (4, 3)
    );
    file.seek(SeekFrom::Start(128)).unwrap();
    file.write_all(&[0; 17]).unwrap();
    file.sync_all().unwrap();
    let before = fs::read(&rig.path).unwrap();
    assert!(inspect_spool(&rig.path).is_err());
    assert_eq!(fs::read(&rig.path).unwrap(), before);
}

#[test]
fn failed_metadata_update_does_not_advance_into_the_previous_valid_slot() {
    let mut rig = Rig::new(config(), writer::Hooks::default());
    rig.start(1100);
    let path = rig.path.with_file_name("header-retry.capture");
    let mut writable = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .unwrap();
    let mut header = spool::Header::prepared(rig.id(), config());
    header.initialize(&mut writable).unwrap();
    let mut readonly = fs::File::open(&path).unwrap();
    let shared = &rig.control.as_ref().unwrap().shared;
    assert!(
        header
            .checkpoint(&mut readonly, shared, SpoolState::RecordingIncomplete)
            .is_err()
    );
    assert_eq!(header.revision, 0);
    assert_eq!(inspect_spool(&path).unwrap().revision, 0);
    header
        .checkpoint(&mut writable, shared, SpoolState::RecordingIncomplete)
        .unwrap();
    assert_eq!(header.revision, 1);
    let inspected = inspect_spool(&path).unwrap();
    assert_eq!(inspected.revision, 1);
    assert_eq!(inspected.first_frame, Some(1100));
    assert_eq!(inspected.physical_whole_frames, 0);
    assert_eq!(inspected.state, SpoolState::RecordingIncomplete);
}

#[test]
fn actual_rt_copy_stop_and_overflow_request_no_rust_allocator_operations() {
    use crate::test_memory::{ThreadAllocations, observe_current_thread};
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            gate: Some(Arc::new(AtomicBool::new(false))),
            fail_after_payload_bytes: None,
            ..writer::Hooks::default()
        },
    );
    let id = rig.id();
    let ((started, appended, stopped), counts) = observe_current_thread(|| {
        let rt = rig.rt();
        (
            rt.start(id, 1200, 48_000, 2),
            rt.append_segment(1200, &[0.8; 8]),
            rt.stop(id, 1204),
        )
    });
    assert!(started && appended && stopped);
    assert_eq!(counts, ThreadAllocations::default());
    let status = rig.release_and_join();
    assert_eq!(status.allocated_blocks, 8);
    assert_eq!(status.pool_pcm_bytes, 8 * 4 * 2 * size_of::<f32>());
    assert_eq!(status.writer_scratch_bytes, 4 * 2 * size_of::<f32>());
    let mut tiny = config();
    tiny.block_count = 1;
    let mut overflow = Rig::new(
        tiny,
        writer::Hooks {
            gate: Some(Arc::new(AtomicBool::new(false))),
            fail_after_payload_bytes: None,
            ..writer::Hooks::default()
        },
    );
    overflow.start(1300);
    let ((first, failed), counts) = observe_current_thread(|| {
        let rt = overflow.rt();
        (
            rt.append_segment(1300, &[0.4; 8]),
            rt.append_segment(1304, &[0.5; 8]),
        )
    });
    assert!(first && !failed);
    assert_eq!(counts, ThreadAllocations::default());
    assert_eq!(
        overflow.release_and_join().failure,
        Some(CaptureFailure::Overflow)
    );
}

/// Guarantees worker release before Rig cleanup even if a staging assertion fails.
struct ReleaseWorkerPhase(Arc<writer::WorkerPhaseGate>);

impl Drop for ReleaseWorkerPhase {
    fn drop(&mut self) {
        self.0.released.store(true, Ordering::Release);
    }
}

#[test]
fn submitted_sequence_published_before_acquired_seal_is_not_an_unpublished_block_error() {
    let gate = Arc::new(writer::WorkerPhaseGate::default());
    let mut rig = Rig::new(
        config(),
        writer::Hooks {
            unpublished_sequence_gate: Some(gate.clone()),
            ..writer::Hooks::default()
        },
    );
    // Declared after Rig so unwind releases the gate before writer settlement.
    let release = ReleaseWorkerPhase(gate.clone());
    let first_frame = 2000;
    let payload = [0.125_f32, -0.375, 1.25, -0.0];
    rig.start(first_frame);
    assert!(rig.rt().append_segment(first_frame, &payload));

    // Isolate the real flush_partial interleaving immediately after its SPSC
    // push and before its final submitted_blocks publication. The original
    // preallocated block and PCM came from actual append_segment; only this
    // private protocol boundary is staged, not a second producer or RT hook.
    {
        let rt = rig.rt();
        assert_eq!(rt.submitted_blocks, 0);
        let mut block = rt.partial.take().expect("actual retained partial block");
        assert_eq!(block.first_frame, first_frame);
        assert_eq!(block.frames, 2);
        block.sequence = 1;
        rt.data
            .push(block)
            .unwrap_or_else(|_| panic!("empty original capture data lane"));
    }
    let deadline = Instant::now() + WAIT;
    while !gate.reached.load(Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "worker did not observe unpublished sequence"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(rig.status().submitted_blocks, 0);
    assert!(!rig.status().rt_sealed);

    {
        let rt = rig.rt();
        // The unchanged scalar publication tail of that one original push.
        rt.submitted_end = rt.next_frame;
        rt.submitted_frames = 2;
        rt.submitted_blocks = 1;
        rt.shared
            .submitted_end
            .store(rt.submitted_end, Ordering::Relaxed);
        rt.shared
            .submitted_frames
            .store(rt.submitted_frames, Ordering::Relaxed);
        rt.shared.buffered_frames.store(0, Ordering::Relaxed);
        rt.shared
            .submitted_blocks
            .store(rt.submitted_blocks, Ordering::Release);
    }
    let session_id = rig.id();
    assert!(rig.rt().stop(session_id, first_frame + 2));
    assert_eq!(rig.status().submitted_blocks, 1);
    assert!(rig.status().rt_sealed);
    // Worker already observed the old sequence, then sees this later seal.
    // The original implementation incorrectly rejects without re-reading the
    // sequence after acquiring that seal. Keep the successful exact-data oracle.
    drop(release);
    let status = rig.release_and_join();
    assert_eq!(status.failure, None);
    assert!(status.successful_spool());
    assert_eq!(
        (status.first_frame, status.end_frame),
        (Some(first_frame), Some(first_frame + 2))
    );
    assert_eq!((status.submitted_frames, status.written_frames), (2, 2));
    assert_eq!((status.submitted_blocks, status.written_blocks), (1, 1));
    assert_eq!(
        rig.payload_bits(),
        payload
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
}
