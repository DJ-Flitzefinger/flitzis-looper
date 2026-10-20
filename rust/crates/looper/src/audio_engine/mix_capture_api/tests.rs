//! Headless control ownership proof with the real spool worker and publication.
//! Manual CaptureRt extents below exercise control settlement, not callback/device acceptance.

use super::*;

#[cfg(windows)]
#[path = "tests/format_finalization_tests.rs"]
mod format_finalization_tests;
#[path = "tests/stream_failure_tests.rs"]
mod stream_failure_tests;
#[path = "tests/target_tests.rs"]
mod target_tests;
use crate::audio_engine::mix_capture::{CaptureRt, CaptureStatus};
use crate::messages::AudioMessage;
use rtrb::{Consumer, RingBuffer};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const RATE: u32 = 44_100;
const CHANNELS: usize = 2;
const WAIT: Duration = Duration::from_secs(5);

struct Rig {
    engine: AudioEngine,
    consumer: Consumer<ControlMessage>,
    directory: Option<tempfile::TempDir>,
}

impl Rig {
    fn new(capacity: usize) -> Self {
        Python::initialize();
        let mut engine = AudioEngine::new().unwrap();
        let (producer, consumer) = RingBuffer::new(capacity);
        let (_feedback, feedback) = RingBuffer::<AudioMessage>::new(1);
        engine.test_control_channels = Some((
            Arc::new(Mutex::new(producer)),
            Arc::new(Mutex::new(feedback)),
        ));
        engine.test_output_format = Some((RATE, CHANNELS));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let parent = workspace.join("test-artifacts/record-capture-20261020");
        fs::create_dir_all(&parent).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("control-api-")
            .tempdir_in(parent)
            .unwrap();
        fs::write(
            directory.path().join("OWNER.md"),
            b"Owned headless mix-capture control API test: synthetic PCM, real exclusive spool, publication and writer. No device, models or Human inputs. Manual RT extents prove control ownership only. Remove only after actual RT release and writer join; unsettled failed fixtures remain retained.\n",
        )
        .unwrap();
        Self {
            engine,
            consumer,
            directory: Some(directory),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.as_ref().unwrap().path().join(name)
    }

    fn prepare_result(&self, name: &str) -> PyResult<MixCaptureTicket> {
        self.engine
            .prepare_mix_capture_internal(self.path(name).to_str().unwrap(), 4, 8, 4096)
    }

    fn prepare(&self, name: &str) -> MixCaptureTicket {
        let ticket = self.prepare_result(name).unwrap();
        self.wait_ready(&ticket);
        ticket
    }

    fn wait_ready(&self, ticket: &MixCaptureTicket) {
        wait_for(&ticket.observer, "writer ready", |status| {
            status.writer_ready
        });
        assert_eq!(ticket.observer.status().config.sample_rate_hz, RATE);
        assert_eq!(ticket.observer.status().config.channels, CHANNELS);
    }

    fn prepare_when_worker_finishes(&self, name: &str) -> MixCaptureTicket {
        let deadline = Instant::now() + WAIT;
        loop {
            match self.prepare_result(name) {
                Ok(ticket) => {
                    self.wait_ready(&ticket);
                    return ticket;
                }
                Err(error) => {
                    assert!(
                        error.to_string().contains("previous mix capture"),
                        "unexpected preparation failure: {error}",
                    );
                    assert!(!self.path(name).exists());
                    assert!(Instant::now() < deadline, "actual writer join timed out");
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn push_ping(&self) {
        self.engine
            .control_producer()
            .unwrap()
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
    }

    fn pop_ping(&mut self) {
        assert!(matches!(
            self.consumer.pop().unwrap(),
            ControlMessage::Ping()
        ));
    }

    fn claim_start(&mut self, ticket: &MixCaptureTicket, frame: u64) -> Box<CaptureRt> {
        let ControlMessage::StartMixCapture {
            publication,
            target_frame,
        } = self.consumer.pop().unwrap()
        else {
            panic!("original capture start must be the sole queued request")
        };
        assert_eq!(target_frame, MixCaptureTarget::At(frame));
        assert_eq!(publication.session_id(), ticket.session_id());
        assert!(Arc::ptr_eq(
            &publication,
            &self
                .engine
                .mix_capture_owner
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .publication,
        ));
        let rt = publication.claim().expect("original prepared owner");
        assert!(
            publication.claim().is_none(),
            "publication has exactly one claim"
        );
        assert_eq!(rt.session_id(), ticket.session_id());
        rt
    }

    fn poll_until_settled(&self) -> CaptureStatus {
        let deadline = Instant::now() + WAIT;
        loop {
            let mut slot = self.engine.mix_capture_owner.lock().unwrap();
            let capture = slot.as_mut().unwrap();
            capture.poll(false);
            let status = capture.control.status();
            if capture.settled() {
                assert!(status.rt_sealed && status.worker_closed);
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "control settlement timed out: {status:?}"
            );
            drop(slot);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        // This fixture has no stream/callback. Drain its actual headless FIFO on
        // control before invoking the post-quiescence orphan-release hook.
        self.engine.close_mix_capture_admission();
        while let Ok(message) = self.consumer.pop() {
            drop(message);
        }
        self.engine.poll_mix_capture_shutdown();
        let deadline = Instant::now() + WAIT;
        let settled = loop {
            let mut slot = self
                .engine
                .mix_capture_owner
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let Some(capture) = slot.as_mut() else {
                break true;
            };
            // Also recover a fixture assertion that poisoned this test-only
            // owner lock; never leave its real writer waiting on a hidden Box.
            let id = capture.control.status().session_id;
            capture
                .control
                .request_failure(id, CaptureFailure::Cancelled);
            drop(capture.publication.claim());
            capture.poll(false);
            if capture.settled() {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            drop(slot);
            std::thread::sleep(Duration::from_millis(1));
        };
        if !settled && let Some(directory) = self.directory.take() {
            eprintln!(
                "unsettled capture control fixture retained: {}",
                directory.keep().display()
            );
        }
    }
}

fn wait_for(
    observer: &CaptureObserver,
    label: &str,
    predicate: impl Fn(&CaptureStatus) -> bool,
) -> CaptureStatus {
    let deadline = Instant::now() + WAIT;
    loop {
        let status = observer.status();
        if predicate(&status) {
            return status;
        }
        assert!(Instant::now() < deadline, "{label} timed out: {status:?}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn assert_ticket_rejected(engine: &AudioEngine, ticket: &MixCaptureTicket) {
    assert!(engine.start_mix_capture_internal(ticket, 100).is_err());
    assert!(engine.stop_mix_capture_internal(ticket, 104).is_err());
    assert!(engine.cancel_mix_capture_internal(ticket).is_err());
    Python::attach(|py| {
        assert!(engine.mix_capture_status_internal(py, ticket).is_err());
    });
}

fn manual_extent(rt: &mut CaptureRt, first: u64) -> Vec<u32> {
    let pcm = [-0.0_f32, 0.25, -0.5, 0.75, 1.0, -1.25, 1.5, -1.75];
    let id = rt.session_id();
    assert!(rt.start(id, first, RATE, CHANNELS));
    assert!(rt.append_segment(first, &pcm));
    assert!(rt.stop(id, first + 4));
    pcm.iter().map(|sample| sample.to_bits()).collect()
}

#[test]
fn unavailable_engine_and_invalid_output_geometry_create_no_capture_or_spool() {
    let mut rig = Rig::new(1);
    let path = rig.path("unavailable.capture");
    let channels = rig.engine.test_control_channels.take();
    assert!(rig.prepare_result("unavailable.capture").is_err());
    assert!(!path.exists());
    assert!(rig.engine.mix_capture_owner.lock().unwrap().is_none());
    rig.engine.test_control_channels = channels;
    for format in [(0, CHANNELS), (RATE, 0)] {
        rig.engine.test_output_format = Some(format);
        assert!(rig.prepare_result("invalid-format.capture").is_err());
        assert!(!rig.path("invalid-format.capture").exists());
        assert!(rig.engine.mix_capture_owner.lock().unwrap().is_none());
    }
    rig.engine.test_output_format = Some((RATE, CHANNELS));
    let ticket = rig.prepare("valid.capture");
    assert!(!ticket.observer.status().started);
    assert!(rig.consumer.is_empty());
}

#[test]
fn stop_without_start_is_not_admitted_and_full_start_can_retry_exactly_once() {
    let mut rig = Rig::new(1);
    let ticket = rig.prepare("retry.capture");
    assert!(!rig.engine.stop_mix_capture_internal(&ticket, 104).unwrap());
    assert!(rig.consumer.is_empty());
    assert!(
        rig.engine
            .mix_capture_owner
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stop_request
            .is_none()
    );
    rig.push_ping();
    assert!(!rig.engine.start_mix_capture_internal(&ticket, 100).unwrap());
    let before = ticket.observer.status();
    assert!(!before.started && !before.rt_sealed && !before.rt_released);
    assert_eq!(before.failure, None);
    assert!(
        !rig.engine
            .mix_capture_owner
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .start_request
            .is_some()
    );
    rig.pop_ping();
    assert!(rig.engine.start_mix_capture_internal(&ticket, 100).unwrap());
    assert!(!rig.engine.start_mix_capture_internal(&ticket, 900).unwrap());
    let rt = rig.claim_start(&ticket, 100);
    assert!(rig.consumer.is_empty());
    assert!(
        !ticket.observer.status().started,
        "FIFO admission is not an effect ACK"
    );
    drop(rt);
    assert_eq!(
        rig.poll_until_settled().failure,
        Some(CaptureFailure::Abandoned)
    );
}

#[test]
fn full_stop_retry_preserves_first_endpoint_and_request_status_precedes_real_extent() {
    let mut rig = Rig::new(1);
    let ticket = rig.prepare("stop.capture");
    assert!(rig.engine.start_mix_capture_internal(&ticket, 100).unwrap());
    let mut rt = rig.claim_start(&ticket, 100);
    rig.push_ping();
    assert!(!rig.engine.stop_mix_capture_internal(&ticket, 104).unwrap());
    assert!(
        rig.engine
            .mix_capture_owner
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stop_request
            .is_none()
    );
    rig.pop_ping();
    assert!(rig.engine.stop_mix_capture_internal(&ticket, 104).unwrap());
    assert!(rig.engine.stop_mix_capture_internal(&ticket, 104).unwrap());
    assert!(!rig.engine.stop_mix_capture_internal(&ticket, 105).unwrap());
    Python::attach(|py| {
        let status = rig.engine.mix_capture_status_internal(py, &ticket).unwrap();
        let status = status.bind(py);
        assert!(
            status
                .get_item("start_queued")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert_eq!(
            status
                .get_item("stop_target")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            104
        );
        assert!(
            !status
                .get_item("started")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            !status
                .get_item("rt_sealed")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(status.get_item("first_frame").unwrap().unwrap().is_none());
        assert!(
            !status
                .get_item("successful_spool")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
    });
    let ControlMessage::StopMixCapture {
        session_id,
        target_frame,
    } = rig.consumer.pop().unwrap()
    else {
        panic!("one original stop request expected")
    };
    assert_eq!(
        (session_id, target_frame),
        (ticket.session_id(), MixCaptureTarget::At(104))
    );
    assert!(
        rig.consumer.is_empty(),
        "identical stop must not enqueue a duplicate"
    );
    let original_bits = manual_extent(&mut rt, 100);
    drop(rt); // Actual original owner leaves this control-only test on control.
    let status = rig.poll_until_settled();
    assert!(status.successful_spool());
    assert_eq!(
        (status.first_frame, status.end_frame),
        (Some(100), Some(104))
    );
    assert_eq!((status.submitted_frames, status.written_frames), (4, 4));
    Python::attach(|py| {
        let status = rig.engine.mix_capture_status_internal(py, &ticket).unwrap();
        let status = status.bind(py);
        assert_eq!(
            status
                .get_item("first_frame")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            100
        );
        assert_eq!(
            status
                .get_item("end_frame")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            104
        );
        assert!(
            status
                .get_item("rt_released")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            status
                .get_item("worker_joined")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            status
                .get_item("successful_spool")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(status.get_item("failure").unwrap().unwrap().is_none());
    });
    let bytes = fs::read(rig.path("stop.capture")).unwrap();
    let stored_bits: Vec<_> = bytes[mix_capture::SPOOL_HEADER_BYTES as usize..]
        .chunks_exact(4)
        .map(|sample| u32::from_le_bytes(sample.try_into().unwrap()))
        .collect();
    assert_eq!(stored_bits, original_bits);
}

#[test]
fn unqueued_cancel_after_full_fifo_releases_original_pool_and_allows_next_prepare() {
    let mut rig = Rig::new(1);
    let old = rig.prepare("unqueued.capture");
    rig.push_ping();
    assert!(!rig.engine.start_mix_capture_internal(&old, 100).unwrap());
    assert_eq!(old.observer.status().failure, None);
    rig.engine.cancel_mix_capture_internal(&old).unwrap();
    let cancelled = old.observer.status();
    assert_eq!(cancelled.failure, Some(CaptureFailure::Cancelled));
    assert!(cancelled.rt_released && cancelled.rt_sealed);
    assert!(!cancelled.started);
    assert!(
        rig.engine
            .mix_capture_owner
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .publication
            .claim()
            .is_none()
    );
    let settled = rig.poll_until_settled();
    assert!(settled.worker_closed && settled.worker_joined);
    assert_eq!(settled.written_frames, 0);
    assert!(!settled.successful_spool());
    rig.pop_ping();
    let next = rig.prepare("after-cancel.capture");
    assert_ne!(next.session_id(), old.session_id());
    assert_ticket_rejected(&rig.engine, &old);
    assert_eq!(next.observer.status().failure, None);
    assert!(rig.consumer.is_empty());
}

#[test]
fn foreign_engine_and_independently_wrong_owner_or_session_cannot_change_original() {
    let first = Rig::new(2);
    let second = Rig::new(2);
    let first_ticket = first.prepare("first.capture");
    let second_ticket = second.prepare("second.capture");
    assert_ticket_rejected(&first.engine, &second_ticket);
    assert_ticket_rejected(&second.engine, &first_ticket);
    let wrong_owner = MixCaptureTicket {
        observer: first_ticket.observer.clone(),
        owner: second_ticket.owner.clone(),
    };
    let wrong_session = MixCaptureTicket {
        observer: second_ticket.observer.clone(),
        owner: first_ticket.owner.clone(),
    };
    assert_ticket_rejected(&first.engine, &wrong_owner);
    assert_ticket_rejected(&first.engine, &wrong_session);
    for (rig, ticket) in [(&first, &first_ticket), (&second, &second_ticket)] {
        let status = ticket.observer.status();
        assert_eq!(status.failure, None);
        assert!(!status.started && !status.rt_sealed && !status.rt_released);
        let slot = rig.engine.mix_capture_owner.lock().unwrap();
        let capture = slot.as_ref().unwrap();
        assert!(capture.start_request.is_none() && capture.stop_request.is_none());
        assert!(capture.matches(ticket));
        assert!(rig.consumer.is_empty());
    }
}

#[test]
fn joined_writer_does_not_allow_second_pool_while_actual_rt_owner_is_retained() {
    let mut rig = Rig::new(1);
    let old = rig.prepare("held-rt.capture");
    assert!(rig.engine.start_mix_capture_internal(&old, 20).unwrap());
    let mut rt = rig.claim_start(&old, 20);
    manual_extent(&mut rt, 20);
    let closed = wait_for(&old.observer, "writer close with retained RT", |s| {
        s.worker_closed
    });
    assert!(!closed.rt_released && !closed.worker_joined);
    {
        let mut slot = rig.engine.mix_capture_owner.lock().unwrap();
        let capture = slot.as_mut().unwrap();
        assert!(!capture.settled());
        let deadline = Instant::now() + WAIT;
        while !capture.control.join_if_finished().unwrap() {
            assert!(Instant::now() < deadline, "closed writer did not join");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(capture.control.status().worker_joined);
        assert!(
            !capture.settled(),
            "joined writer cannot release retained PCM/rings"
        );
    }
    assert!(rig.prepare_result("too-early.capture").is_err());
    assert!(!rig.path("too-early.capture").exists());
    assert_eq!(old.observer.status().failure, None);
    drop(rt);
    assert!(old.observer.status().rt_released);
    let next = rig.prepare("after-release.capture");
    assert_ne!(next.session_id(), old.session_id());
    assert_ticket_rejected(&rig.engine, &old);
    assert_eq!(next.observer.status().failure, None);
}

#[test]
fn actual_rt_release_and_worker_close_still_require_real_join_before_settlement() {
    let mut rig = Rig::new(1);
    let old = rig.prepare("unjoined.capture");
    assert!(rig.engine.start_mix_capture_internal(&old, 30).unwrap());
    let mut rt = rig.claim_start(&old, 30);
    manual_extent(&mut rt, 30);
    drop(rt);
    let closed = wait_for(&old.observer, "writer close before control poll", |s| {
        s.worker_closed
    });
    assert!(closed.rt_released);
    assert!(
        !closed.worker_joined,
        "observation cannot manufacture worker join"
    );
    {
        let slot = rig.engine.mix_capture_owner.lock().unwrap();
        assert!(!slot.as_ref().unwrap().settled());
    }
    // Preparation is allowed to perform the actual nonblocking finished-worker
    // join itself. A merely closed status was not sufficient before this call.
    let next = rig.prepare_when_worker_finishes("after-join.capture");
    assert!(old.observer.status().worker_joined);
    assert_ne!(next.session_id(), old.session_id());
}

#[test]
fn dropping_ticket_does_not_cancel_engine_owned_queued_capture() {
    let mut rig = Rig::new(1);
    let ticket = rig.prepare("ticket-drop.capture");
    let observer = ticket.observer.clone();
    let id = ticket.session_id();
    assert!(rig.engine.start_mix_capture_internal(&ticket, 60).unwrap());
    drop(ticket);
    let pending = observer.status();
    assert_eq!(pending.failure, None);
    assert!(!pending.started && !pending.rt_sealed && !pending.rt_released);
    let ControlMessage::StartMixCapture {
        publication,
        target_frame,
    } = rig.consumer.pop().unwrap()
    else {
        panic!("ticket drop must leave its original start queued")
    };
    assert_eq!(
        (publication.session_id(), target_frame),
        (id, MixCaptureTarget::At(60))
    );
    let mut rt = publication
        .claim()
        .expect("engine retains original pool after UI ticket drop");
    manual_extent(&mut rt, 60);
    drop(rt);
    let status = rig.poll_until_settled();
    assert!(status.successful_spool());
    assert_eq!(status.failure, None);
    assert_eq!(status.end_frame, Some(64));
}

#[test]
fn queued_cancel_waits_for_actual_orphan_release_after_headless_fifo_quiescence() {
    let mut rig = Rig::new(1);
    let ticket = rig.prepare("unread.capture");
    assert!(rig.engine.start_mix_capture_internal(&ticket, 70).unwrap());
    rig.engine.cancel_mix_capture_internal(&ticket).unwrap();
    let signalled = ticket.observer.status();
    assert_eq!(signalled.failure, Some(CaptureFailure::Cancelled));
    assert!(!signalled.rt_sealed && !signalled.rt_released && !signalled.worker_closed);
    assert!(rig.prepare_result("while-unread.capture").is_err());
    assert!(!rig.path("while-unread.capture").exists());
    let request = rig.consumer.pop().unwrap();
    assert!(matches!(request, ControlMessage::StartMixCapture { .. }));
    drop(request); // No callback exists; the actual test FIFO is now quiescent.
    assert!(
        !ticket.observer.status().rt_released,
        "engine still owns original publication"
    );
    rig.engine.poll_mix_capture_shutdown();
    let status = rig.poll_until_settled();
    assert!(status.rt_released && status.worker_closed && status.worker_joined);
    assert!(!status.started && !status.successful_spool());
    assert_eq!(status.failure, Some(CaptureFailure::Cancelled));
    assert_eq!(status.written_frames, 0);
}
