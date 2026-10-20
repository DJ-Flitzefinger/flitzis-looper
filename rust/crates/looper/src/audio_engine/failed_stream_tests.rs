//! Actual headless callback close/join with the original engine capture owner.
//! No CPAL handle, device error delivery, effective ACK or replacement ticket is fabricated.
use super::*;
use crate::audio_engine::buffer_retirement::{
    RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY, close_callback_and_join,
};
use crate::audio_engine::mix_capture::SPOOL_HEADER_BYTES;
use crate::audio_engine::{AudioEngine, MixCaptureTicket, StreamLifecycle, StreamQuiescence};
use pyo3::prelude::*;
use pyo3::types::PyDictMethods;
use std::fs;
use std::path::PathBuf;
use std::sync::Weak;
use std::sync::atomic::AtomicU8;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const WAIT: Duration = Duration::from_secs(5);
const FULL_RETIREMENT_SLOTS: usize = RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY + 128;

#[derive(Debug, PartialEq, Eq)]
struct Status {
    session_id: u64,
    writer_ready: bool,
    start_queued: bool,
    started: bool,
    rt_sealed: bool,
    rt_released: bool,
    worker_closed: bool,
    worker_joined: bool,
    terminal_decided: bool,
    first_frame: Option<u64>,
    captured_end: Option<u64>,
    end_frame: Option<u64>,
    submitted_frames: u64,
    written_frames: u64,
    failure: Option<String>,
    successful_spool: bool,
}

fn status(engine: &AudioEngine, ticket: &MixCaptureTicket) -> Status {
    Python::attach(|py| {
        let snapshot = engine.mix_capture_status_internal(py, ticket).unwrap();
        let snapshot = snapshot.bind(py);
        let item = |name: &str| snapshot.get_item(name).unwrap().unwrap();
        assert_eq!(item("sample_rate_hz").extract::<u32>().unwrap(), RATE);
        assert_eq!(item("channels").extract::<usize>().unwrap(), CHANNELS);
        Status {
            session_id: item("session_id").extract().unwrap(),
            writer_ready: item("writer_ready").extract().unwrap(),
            start_queued: item("start_queued").extract().unwrap(),
            started: item("started").extract().unwrap(),
            rt_sealed: item("rt_sealed").extract().unwrap(),
            rt_released: item("rt_released").extract().unwrap(),
            worker_closed: item("worker_closed").extract().unwrap(),
            worker_joined: item("worker_joined").extract().unwrap(),
            terminal_decided: item("terminal_decided").extract().unwrap(),
            first_frame: item("first_frame").extract().unwrap(),
            captured_end: item("captured_end").extract().unwrap(),
            end_frame: item("end_frame").extract().unwrap(),
            submitted_frames: item("submitted_frames").extract().unwrap(),
            written_frames: item("written_frames").extract().unwrap(),
            failure: item("failure").extract().unwrap(),
            successful_spool: item("successful_spool").extract().unwrap(),
        }
    })
}

/// The gate opens only after the actual unique callback owner has handed off
/// its complete image. The productive generic close/join primitive drops this
/// tiny wrapper and then performs its original checked retirement-worker join.
struct CloseThenRelease {
    callback: Option<CallbackCloseOwner<AudioCallbackState>>,
    release: Option<mpsc::Sender<()>>,
}

impl Drop for CloseThenRelease {
    fn drop(&mut self) {
        drop(self.callback.take());
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}

struct Rig {
    engine: AudioEngine,
    ticket: MixCaptureTicket,
    callback: Option<CallbackCloseOwner<AudioCallbackState>>,
    worker: Option<AudioBufferRetirementWorker>,
    release_worker: Option<mpsc::Sender<()>>,
    failure: Arc<AtomicBool>,
    callback_clock: Weak<SharedOutputClock>,
    retired_marker: Option<Weak<AtomicU8>>,
    close_calls: usize,
    path: PathBuf,
    directory: Option<tempfile::TempDir>,
}

impl Rig {
    fn new(panic_worker: bool) -> Self {
        Python::initialize();
        let mut engine = AudioEngine::new().unwrap();
        let (producer, consumer) = RingBuffer::new(16);
        let (_parameters, parameter_consumer) = RingBuffer::new(8);
        let (feedback, feedback_consumer) = RingBuffer::new(64);
        engine.test_control_channels = Some((
            Arc::new(Mutex::new(producer)),
            Arc::new(Mutex::new(feedback_consumer)),
        ));
        engine.test_output_format = Some((RATE, CHANNELS));
        let failure = Arc::new(AtomicBool::new(false));
        engine.stream_lifecycle = Some(StreamLifecycle {
            failed: failure.clone(),
            quiescence: StreamQuiescence::Open,
        });
        let mixer = create_stream_mixer(
            CHANNELS,
            RATE,
            engine.current_timing_acknowledgements.clone(),
            engine.input_runtime_ownership.clone(),
            engine.prepared_source_epochs.clone(),
        )
        .unwrap();
        let clock = Arc::new(SharedOutputClock::new());
        let callback_clock = Arc::downgrade(&clock);
        let (release_worker, worker_start) = mpsc::channel();
        let (callback, worker) = create_stream_callback_owner_with_start(
            (consumer, parameter_consumer, feedback),
            mixer,
            RATE,
            clock,
            engine.loop_acceptance.clone(),
            move || {
                worker_start.recv().unwrap();
                assert!(!panic_worker, "owned failed-stream retirement join probe");
            },
        )
        .unwrap();
        // Also close/release in this order if artifact preparation below fails
        // before the complete fixture has acquired its Drop implementation.
        let mut close_guard = CloseThenRelease {
            callback: Some(callback),
            release: Some(release_worker),
        };
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let parent = workspace.join("test-artifacts/record-capture-20261020");
        fs::create_dir_all(&parent).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("failed-stream-")
            .tempdir_in(parent)
            .unwrap();
        fs::write(
            directory.path().join("OWNER.md"),
            b"Owned failed-stream lifecycle regression: actual headless productive callback image, original engine/ticket/control FIFO, real retirement close/join and exclusive float spool writer. No CPAL/device/Human input. Remove only after independently observed image release and original writer join; unsettled outputs are retained.\n",
        )
        .unwrap();
        let path = directory.path().join("take.capture");
        let ticket = engine
            .prepare_mix_capture_internal(path.to_str().unwrap(), 4, 8, 4096)
            .unwrap();
        let rig = Self {
            engine,
            ticket,
            callback: close_guard.callback.take(),
            worker: Some(worker),
            release_worker: close_guard.release.take(),
            failure,
            callback_clock,
            retired_marker: None,
            close_calls: 0,
            path,
            directory: Some(directory),
        };
        let deadline = Instant::now() + WAIT;
        while !rig.status().writer_ready {
            assert!(
                Instant::now() < deadline,
                "original writer readiness timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        rig
    }

    fn state(&mut self) -> &mut AudioCallbackState {
        self.callback.as_mut().unwrap().state_mut()
    }

    fn status(&self) -> Status {
        status(&self.engine, &self.ticket)
    }

    fn start(&self, frame: u64) {
        assert!(
            self.engine
                .start_mix_capture_internal(&self.ticket, frame)
                .unwrap()
        );
    }

    fn drain(&mut self) -> usize {
        let state = self.state();
        drain_control_messages(
            &mut state.consumer_in,
            &mut state.scheduler,
            state.transport.output_frame(),
            &mut state.trigger_quantization,
            &mut state.transport,
            &mut state.mixer,
            &mut state.producer_out,
            &mut state.retired_buffers,
        )
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * CHANNELS];
        let state = self.state();
        render_scheduled_audio(
            &mut state.mixer,
            &mut state.scheduler,
            &mut output,
            &mut state.pad_peaks,
            state.transport.output_frame(),
            CHANNELS,
            &mut state.transport,
            &mut state.producer_out,
            &mut state.retired_buffers,
        );
        state.transport.advance_by_rendered_frames(frames);
        output
    }

    fn fill_retirement(&mut self) {
        let marker = Arc::new(AtomicU8::new(0));
        let witness = Arc::downgrade(&marker);
        let state = self.state();
        let available = state.retired_buffers.available_retirement_slots();
        assert!(available > 0 && available <= FULL_RETIREMENT_SLOTS);
        let mut filled = 0;
        while state.retired_buffers.available_retirement_slots() != 0 {
            assert!(filled < FULL_RETIREMENT_SLOTS);
            state.retired_buffers.retire_cold_adoption(marker.clone());
            filled += 1;
        }
        assert_eq!(filled, available);
        drop(marker);
        assert_eq!(witness.strong_count(), filled);
        self.retired_marker = Some(witness);
    }

    fn signal_failure(&self) {
        self.failure.store(true, Ordering::Release);
        assert!(self.engine.audio_stream_failed());
    }

    fn quiesce(&mut self) -> PyResult<bool> {
        let Self {
            engine,
            ticket,
            callback,
            worker,
            release_worker,
            close_calls,
            ..
        } = self;
        engine.quiesce_failed_stream_with(|engine| {
            *close_calls += 1;
            assert_eq!(
                engine.stream_lifecycle.as_ref().unwrap().quiescence,
                StreamQuiescence::Uncertain
            );
            // The actual failure precedes close, but signaling cannot invent
            // callback sealing, original-pool release or writer completion.
            let snapshot = status(engine, ticket);
            assert!(snapshot.failure.is_some());
            assert!(!snapshot.rt_sealed && !snapshot.rt_released);
            assert!(!snapshot.worker_closed && !snapshot.worker_joined);
            close_callback_and_join(
                CloseThenRelease {
                    callback: callback.take(),
                    release: release_worker.take(),
                },
                worker.take().expect("original retirement worker"),
            )
        })
    }

    fn image_released(&self) -> bool {
        // callback_clock is declared after the real consumer/mixer/scheduler.
        // The marker spans the real normal retirement queue and final backlog.
        self.callback_clock.strong_count() == 0
            && self
                .retired_marker
                .as_ref()
                .is_none_or(|marker| marker.strong_count() == 0)
    }

    fn joined(&self) -> Status {
        let deadline = Instant::now() + WAIT;
        loop {
            let snapshot = self.status();
            if snapshot.worker_joined && snapshot.rt_released {
                assert!(snapshot.worker_closed && snapshot.rt_sealed);
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "original writer did not join after actual image release: {snapshot:?}"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn bits(&self) -> Vec<u32> {
        fs::read(&self.path).unwrap()[SPOOL_HEADER_BYTES as usize..]
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.engine.close_mix_capture_admission();
        if let Some(worker) = self.worker.take() {
            let _ = close_callback_and_join(
                CloseThenRelease {
                    callback: self.callback.take(),
                    release: self.release_worker.take(),
                },
                worker,
            );
        }
        // Even a checked-join failure does not grant production orphan claim.
        // Fixture-only cleanup requires these independent actual lifetime
        // witnesses, not a fabricated lifecycle Verified value.
        let mut settled = false;
        if self.callback.is_none() && self.image_released() {
            self.engine.poll_mix_capture_shutdown();
            let deadline = Instant::now() + WAIT;
            loop {
                let snapshot = self.status();
                if snapshot.rt_released && snapshot.worker_joined {
                    settled = true;
                    break;
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if !settled && let Some(directory) = self.directory.take() {
            eprintln!(
                "unsettled failed-stream fixture retained: {}",
                directory.keep().display()
            );
        }
    }
}

#[test]
fn healthy_or_unstarted_failure_quiescence_is_noop_and_run_guard_precedes_resets() {
    Python::initialize();
    let mut unstarted = AudioEngine::new().unwrap();
    assert!(!unstarted.audio_stream_failed());
    assert!(!unstarted.quiesce_failed_audio_stream().unwrap());

    let mut rig = Rig::new(false);
    rig.start(100);
    let before = rig.status();
    assert!(!rig.engine.audio_stream_failed());
    assert!(!rig.quiesce().unwrap());
    assert!(!rig.engine.quiesce_failed_audio_stream().unwrap());
    assert_eq!(rig.close_calls, 0);
    assert!(rig.callback.is_some() && rig.worker.is_some());
    assert_eq!(rig.callback_clock.strong_count(), 1);
    assert_eq!(rig.status(), before);
    // This call must stop at the productive lifecycle preguard, without
    // attempting device construction or resetting the existing cold/source state.
    rig.engine.cold_cancelled.store(true, Ordering::Release);
    assert!(
        rig.engine
            .run()
            .unwrap_err()
            .to_string()
            .contains("quiescence")
    );
    assert!(rig.engine.cold_cancelled.load(Ordering::Acquire));
    assert_eq!(rig.status(), before);
    assert_eq!(
        rig.engine.stream_lifecycle.as_ref().unwrap().quiescence,
        StreamQuiescence::Open
    );
}

#[derive(Clone, Copy, Debug)]
enum Placement {
    Unread,
    Pending,
    Active,
}

#[test]
fn actual_failed_close_preserves_original_unread_pending_active_and_first_cancelled_failure() {
    for placement in [Placement::Unread, Placement::Pending, Placement::Active] {
        for prior_cancelled in [false, true] {
            let mut rig = Rig::new(false);
            let expected = match placement {
                Placement::Unread => {
                    rig.start(314_159);
                    assert!(!rig.state().consumer_in.is_empty());
                    Vec::new()
                }
                Placement::Pending => {
                    rig.start(100);
                    assert_eq!(rig.drain(), 1);
                    assert_eq!(rig.state().scheduler.peek_next_target_frame(), Some(100));
                    Vec::new()
                }
                Placement::Active => {
                    rig.start(3);
                    assert_eq!(rig.drain(), 1);
                    let output = rig.render(8);
                    let snapshot = rig.status();
                    assert!(snapshot.started);
                    assert_eq!(snapshot.first_frame, Some(3));
                    assert_eq!(snapshot.captured_end, Some(8));
                    output[3 * CHANNELS..].to_vec()
                }
            };
            let session = rig.ticket.session_id();
            assert!(rig.status().start_queued);
            assert!(
                rig.engine
                    .ensure_mix_capture_settled_before_run()
                    .unwrap_err()
                    .to_string()
                    .contains("not released and joined"),
                "the original live pool/writer must fence explicit restart"
            );
            if prior_cancelled {
                assert!(rig.engine.cancel_mix_capture_internal(&rig.ticket).unwrap());
                assert_eq!(rig.status().failure.as_deref(), Some("cancelled"));
            }
            rig.fill_retirement();
            rig.signal_failure();
            assert!(rig.quiesce().unwrap());
            assert_eq!(rig.close_calls, 1);
            assert!(rig.callback.is_none() && rig.worker.is_none());
            assert!(rig.image_released());
            assert_eq!(
                rig.engine.stream_lifecycle.as_ref().unwrap().quiescence,
                StreamQuiescence::Verified
            );
            let snapshot = rig.joined();
            assert!(rig.engine.ensure_mix_capture_settled_before_run().is_ok());
            assert_eq!(snapshot.session_id, session);
            assert_eq!(
                snapshot.failure.as_deref(),
                Some(if prior_cancelled {
                    "cancelled"
                } else {
                    "stream"
                })
            );
            assert!(snapshot.terminal_decided);
            assert!(!snapshot.successful_spool);
            if matches!(placement, Placement::Active) {
                assert!(snapshot.started);
                assert_eq!(
                    (snapshot.first_frame, snapshot.end_frame),
                    (Some(3), Some(8))
                );
                assert_eq!((snapshot.submitted_frames, snapshot.written_frames), (5, 5));
            } else {
                assert!(!snapshot.started);
                assert_eq!((snapshot.first_frame, snapshot.end_frame), (None, None));
                assert_eq!((snapshot.submitted_frames, snapshot.written_frames), (0, 0));
            }
            assert_eq!(
                rig.bits(),
                expected
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>()
            );
            assert!(rig.engine.quiesce_failed_audio_stream().unwrap());
            assert!(rig.quiesce().unwrap());
            assert_eq!(rig.close_calls, 1);
            assert!(rig.engine.audio_stream_failed());
            assert_eq!(rig.status(), snapshot);
        }
    }
}

#[test]
fn actual_retirement_worker_panic_keeps_uncertainty_and_cannot_claim_unread_original() {
    let mut rig = Rig::new(true);
    rig.start(314_159);
    rig.fill_retirement();
    rig.signal_failure();
    let error = rig.quiesce().unwrap_err();
    assert!(error.to_string().contains("retirement worker panicked"));
    assert_eq!(rig.close_calls, 1);
    assert!(rig.callback.is_none() && rig.worker.is_none());
    // Join returned the actual worker panic. Its unwind destroyed the complete
    // queued callback image and normal/backlog markers, independently observed.
    assert!(rig.image_released());
    assert_eq!(
        rig.engine.stream_lifecycle.as_ref().unwrap().quiescence,
        StreamQuiescence::Uncertain
    );
    let before_cleanup = rig.status();
    assert_eq!(before_cleanup.failure.as_deref(), Some("stream"));
    assert!(before_cleanup.start_queued);
    assert!(!before_cleanup.started);
    assert!(!before_cleanup.rt_sealed && !before_cleanup.rt_released);
    assert!(!before_cleanup.worker_closed && !before_cleanup.worker_joined);
    assert!(!before_cleanup.terminal_decided && !before_cleanup.successful_spool);
    assert_eq!(
        (before_cleanup.first_frame, before_cleanup.end_frame),
        (None, None)
    );
    assert!(rig.engine.quiesce_failed_audio_stream().is_err());
    assert!(rig.quiesce().is_err());
    assert_eq!(rig.close_calls, 1);
    assert_eq!(rig.status(), before_cleanup);
    rig.engine.cold_cancelled.store(true, Ordering::Release);
    assert!(
        rig.engine
            .run()
            .unwrap_err()
            .to_string()
            .contains("quiescence")
    );
    assert!(rig.engine.cold_cancelled.load(Ordering::Acquire));
    assert_eq!(rig.status(), before_cleanup);

    // Only fixture cleanup follows the independently observed real image
    // destruction. Production uncertainty remains sticky and is not promoted.
    rig.engine.poll_mix_capture_shutdown();
    let terminal = rig.joined();
    assert_eq!(terminal.session_id, rig.ticket.session_id());
    assert_eq!(terminal.failure.as_deref(), Some("stream"));
    assert!(!terminal.started && !terminal.successful_spool);
    assert_eq!((terminal.first_frame, terminal.end_frame), (None, None));
    assert_eq!((terminal.submitted_frames, terminal.written_frames), (0, 0));
    assert!(rig.bits().is_empty());
    assert_eq!(
        rig.engine.stream_lifecycle.as_ref().unwrap().quiescence,
        StreamQuiescence::Uncertain
    );
    assert!(rig.engine.quiesce_failed_audio_stream().is_err());
    assert_eq!(rig.close_calls, 1);
}

#[test]
fn failed_builder_with_verified_actual_callback_join_returns_its_original_error_only() {
    let mut rig = Rig::new(false);
    rig.start(314_159);
    rig.fill_retirement();
    let before = rig.status();
    let startup_failure = Arc::new(AtomicBool::new(false));
    let build_error: Box<dyn std::error::Error> =
        Box::new(std::io::Error::other("owned failed-builder original cause"));
    let original_address = build_error.as_ref() as *const dyn std::error::Error as *const ();

    // A failed device builder has already disposed of its supplied callback
    // closure. Perform that same real handoff before the productive build seam.
    drop(CloseThenRelease {
        callback: rig.callback.take(),
        release: rig.release_worker.take(),
    });
    let error = finish_stream_build::<()>(
        Err(build_error),
        rig.worker.take().unwrap(),
        startup_failure.clone(),
    )
    .err()
    .expect("forced original builder error must be returned");
    assert!(rig.image_released());
    assert_eq!(
        error.as_ref() as *const dyn std::error::Error as *const (),
        original_address
    );
    assert!(error.downcast_ref::<std::io::Error>().is_some());
    assert!(error.downcast_ref::<UncertainStreamStartup>().is_none());
    rig.engine.note_stream_startup_error(error.as_ref());
    assert!(!startup_failure.load(Ordering::Acquire));
    assert!(!rig.engine.audio_stream_failed());
    let lifecycle = rig.engine.stream_lifecycle.as_ref().unwrap();
    assert_eq!(lifecycle.quiescence, StreamQuiescence::Open);
    assert!(Arc::ptr_eq(&lifecycle.failed, &rig.failure));
    assert_eq!(rig.status(), before);
    assert!(!before.rt_sealed && !before.rt_released && !before.worker_closed);

    // The original unread capture was never transferred or acknowledged by
    // the failed builder. Only independently proven fixture cleanup settles it.
    rig.engine.close_mix_capture_admission();
    rig.engine.poll_mix_capture_shutdown();
    let terminal = rig.joined();
    assert!(!terminal.started && !terminal.successful_spool);
    assert_eq!((terminal.first_frame, terminal.end_frame), (None, None));
    assert_eq!((terminal.submitted_frames, terminal.written_frames), (0, 0));
    assert!(rig.bits().is_empty());
}

#[test]
fn failed_builder_worker_panic_fences_first_or_prior_lifecycle_without_faking_stream_failure() {
    for preserve_prior_signal in [false, true] {
        let mut rig = Rig::new(true);
        // The exact headless constructor has not installed any CPAL handle.
        // Cover both a first startup and retention of an earlier signal owner;
        // neither branch fabricates a successful quiescence receipt.
        if !preserve_prior_signal {
            rig.engine.stream_lifecycle = None;
        }
        rig.start(314_159);
        rig.fill_retirement();
        let before = rig.status();
        let startup_failure = Arc::new(AtomicBool::new(false));
        let build_error: Box<dyn std::error::Error> =
            Box::new(std::io::Error::other("owned builder failure before handle"));
        drop(CloseThenRelease {
            callback: rig.callback.take(),
            release: rig.release_worker.take(),
        });
        let error = finish_stream_build::<()>(
            Err(build_error),
            rig.worker.take().unwrap(),
            startup_failure.clone(),
        )
        .err()
        .expect("actual checked retirement panic must remain a startup error");
        let uncertain = error.downcast_ref::<UncertainStreamStartup>().unwrap();
        assert!(Arc::ptr_eq(&uncertain.failure_signal(), &startup_failure));
        assert!(
            error
                .to_string()
                .contains("owned builder failure before handle")
        );
        assert!(error.to_string().contains("retirement worker panicked"));
        assert!(rig.image_released());
        rig.engine.note_stream_startup_error(error.as_ref());
        assert!(!startup_failure.load(Ordering::Acquire));
        assert!(!rig.engine.audio_stream_failed());
        let lifecycle = rig.engine.stream_lifecycle.as_ref().unwrap();
        assert_eq!(lifecycle.quiescence, StreamQuiescence::Uncertain);
        assert!(Arc::ptr_eq(
            &lifecycle.failed,
            if preserve_prior_signal {
                &rig.failure
            } else {
                &startup_failure
            }
        ));
        // Neither the builder error nor an unsuccessful join grants the
        // original engine publication's sole RT claim or any terminal ACK.
        assert_eq!(rig.status(), before);
        assert!(before.start_queued && before.writer_ready);
        assert_eq!(before.failure, None);
        assert!(!before.started && !before.rt_sealed && !before.rt_released);
        assert!(!before.worker_closed && !before.worker_joined);
        assert!(!before.terminal_decided && !before.successful_spool);
        assert_eq!((before.first_frame, before.end_frame), (None, None));
        assert!(rig.engine.quiesce_failed_audio_stream().is_err());
        assert!(rig.quiesce().is_err());
        assert_eq!(rig.close_calls, 0);
        rig.engine.cold_cancelled.store(true, Ordering::Release);
        assert!(
            rig.engine
                .run()
                .unwrap_err()
                .to_string()
                .contains("quiescence")
        );
        assert!(rig.engine.cold_cancelled.load(Ordering::Acquire));
        assert_eq!(rig.status(), before);

        // The real worker unwind and both weak witnesses have independently
        // proved image destruction. This is explicit test cleanup only; the
        // productive lifecycle remains Uncertain even once its writer joins.
        rig.engine.close_mix_capture_admission();
        rig.engine.poll_mix_capture_shutdown();
        let terminal = rig.joined();
        assert_eq!(terminal.session_id, rig.ticket.session_id());
        assert!(!terminal.started && !terminal.successful_spool);
        assert_eq!((terminal.first_frame, terminal.end_frame), (None, None));
        assert_eq!((terminal.submitted_frames, terminal.written_frames), (0, 0));
        assert!(rig.bits().is_empty());
        assert!(!rig.engine.audio_stream_failed());
        assert_eq!(
            rig.engine.stream_lifecycle.as_ref().unwrap().quiescence,
            StreamQuiescence::Uncertain
        );
        assert!(rig.engine.quiesce_failed_audio_stream().is_err());
        assert_eq!(rig.close_calls, 0);
    }
}
