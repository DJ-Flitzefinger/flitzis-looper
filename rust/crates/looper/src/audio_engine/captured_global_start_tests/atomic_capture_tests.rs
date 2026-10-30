//! Actual captured GLOBAL publication, final-mix spool and original writer settlement.
//! These headless proofs do not establish device, GUI or hearing acceptance.
use super::*;
#[path = "atomic_capture_tests/controller_tests.rs"]
mod controller_tests;
#[cfg(windows)]
#[path = "atomic_capture_tests/stem_group_tests.rs"]
mod stem_group_tests;
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirement, CallbackCloseOwner, RtAudioBufferRetirement,
    create_audio_buffer_retirement_with_callback_paused,
};
use crate::audio_engine::mix_capture::SPOOL_HEADER_BYTES;
use crate::audio_engine::mix_capture_api::MixCaptureTicket;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::ordinary_endpoint_reserve_tests::{
    adopt_current_sources, refresh_actual_runtime_bindings,
};
use crate::audio_engine::scheduler::ScheduledCommand;
use crate::messages::MixCaptureTarget;
use pyo3::types::PyDictMethods;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

const CAPTURE_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq, Eq)]
struct Status {
    writer_ready: bool,
    start_queued: bool,
    started: bool,
    rt_sealed: bool,
    rt_released: bool,
    worker_closed: bool,
    worker_joined: bool,
    first_frame: Option<u64>,
    end_frame: Option<u64>,
    submitted_frames: u64,
    written_frames: u64,
    failure: Option<String>,
    empty: bool,
    successful_spool: bool,
}

fn status(engine: &AudioEngine, ticket: &MixCaptureTicket) -> Status {
    Python::attach(|py| {
        let snapshot = engine.mix_capture_status_internal(py, ticket).unwrap();
        let snapshot = snapshot.bind(py);
        let item = |name: &str| snapshot.get_item(name).unwrap().unwrap();
        Status {
            writer_ready: item("writer_ready").extract().unwrap(),
            start_queued: item("start_queued").extract().unwrap(),
            started: item("started").extract().unwrap(),
            rt_sealed: item("rt_sealed").extract().unwrap(),
            rt_released: item("rt_released").extract().unwrap(),
            worker_closed: item("worker_closed").extract().unwrap(),
            worker_joined: item("worker_joined").extract().unwrap(),
            first_frame: item("first_frame").extract().unwrap(),
            end_frame: item("end_frame").extract().unwrap(),
            submitted_frames: item("submitted_frames").extract().unwrap(),
            written_frames: item("written_frames").extract().unwrap(),
            failure: item("failure").extract().unwrap(),
            empty: item("empty").extract().unwrap(),
            successful_spool: item("successful_spool").extract().unwrap(),
        }
    })
}

struct CaptureRig {
    fixture: Fixture,
    ticket: Arc<MixCaptureTicket>,
    path: PathBuf,
    directory: Option<tempfile::TempDir>,
}

impl CaptureRig {
    fn new(lock: bool) -> Self {
        Self::with_block_count(lock, 8)
    }

    fn with_block_count(lock: bool, block_count: usize) -> Self {
        Self::with_fixture(Fixture::new(lock), block_count)
    }

    fn with_fixture(fixture: Fixture, block_count: usize) -> Self {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let parent = workspace.join("test-artifacts/record-capture-20261020");
        fs::create_dir_all(&parent).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("atomic-global-")
            .tempdir_in(parent)
            .unwrap();
        fs::write(
            directory.path().join("OWNER.md"),
            b"Owned atomic GLOBAL/capture regression: synthetic source PCM, original engine/control FIFO, real scheduler/mixer/retirement and exclusive float spool writer. No device or Human input. Delete only after actual original RT release and writer join; unsettled output is retained.\n",
        )
        .unwrap();
        let path = directory.path().join("take.capture");
        let ticket = fixture
            .control
            .engine
            .prepare_mix_capture_internal(path.to_str().unwrap(), 4096, block_count, 256 * 1024)
            .unwrap();
        let rig = Self {
            fixture,
            ticket: Arc::new(ticket),
            path,
            directory: Some(directory),
        };
        let deadline = Instant::now() + CAPTURE_WAIT;
        while !rig.status().writer_ready {
            assert!(
                Instant::now() < deadline,
                "actual writer readiness timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        rig
    }

    fn status(&self) -> Status {
        status(&self.fixture.control.engine, &self.ticket)
    }

    fn publish(&self, captured: &CapturedGlobalStart) -> GlobalPlaybackBatchTicket {
        self.fixture
            .control
            .engine
            .publish_captured_global_start_py(captured, Some(&self.ticket))
            .unwrap()
            .expect("ready original capture/group must publish")
    }

    fn stop(&mut self, frame: u64) {
        assert!(
            self.fixture
                .control
                .engine
                .stop_mix_capture_internal(&self.ticket, frame)
                .unwrap()
        );
        self.fixture.drain();
    }

    fn joined(&mut self) -> Status {
        let deadline = Instant::now() + CAPTURE_WAIT;
        loop {
            self.fixture.drain();
            self.fixture.render(0);
            let snapshot = self.status();
            if snapshot.worker_joined && snapshot.rt_released {
                assert!(snapshot.worker_closed && snapshot.rt_sealed);
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "actual original RT/writer settlement timed out: {snapshot:?}"
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

impl Drop for CaptureRig {
    fn drop(&mut self) {
        self.fixture.control.engine.close_mix_capture_admission();
        let deadline = Instant::now() + CAPTURE_WAIT;
        let settled = loop {
            self.fixture.drain();
            self.fixture.render(0);
            let snapshot = self.status();
            if snapshot.worker_joined && snapshot.rt_released {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        if !settled && let Some(directory) = self.directory.take() {
            eprintln!(
                "unsettled atomic GLOBAL capture retained: {}",
                directory.keep().display()
            );
        }
    }
}

struct RetirementPause {
    open: Arc<AtomicBool>,
    original: Option<CallbackCloseOwner<RtAudioBufferRetirement>>,
}

impl RetirementPause {
    fn install(fixture: &mut Fixture) -> Self {
        let open = Arc::new(AtomicBool::new(false));
        let gate = open.clone();
        let (mut original, worker) =
            create_audio_buffer_retirement_with_callback_paused(Box::new, move || {
                while !gate.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
            .unwrap();
        std::mem::swap(&mut fixture.retirement, original.state_mut());
        let previous_worker = std::mem::replace(&mut fixture._retirement_worker, worker);
        drop(previous_worker);
        Self {
            open,
            original: Some(original),
        }
    }

    fn fill(&self, fixture: &mut Fixture) {
        let marker = Arc::new(AtomicU8::new(0));
        while fixture.retirement.available_retirement_slots() != 0 {
            fixture.retirement.retire_cold_adoption(marker.clone());
        }
    }

    fn release(&mut self) {
        self.open.store(true, Ordering::Release);
        drop(self.original.take());
    }
}

impl Drop for RetirementPause {
    fn drop(&mut self) {
        self.release();
    }
}

/// The productive drain/render seam with caller-owned fixed output and a
/// preallocated feedback sink. No Fixture::render Vec or oracle enters it.
fn fixed_capture_callback(fixture: &mut Fixture, output: &mut [f32]) -> usize {
    let frame = fixture.transport.output_frame();
    let processed = drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        frame,
        &mut TriggerQuantization::Immediate,
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    crate::audio_engine::audio_stream::render_scheduled_audio(
        &mut fixture.mixer,
        &mut fixture.scheduler,
        output,
        &mut [0.0; crate::audio_engine::constants::NUM_SAMPLES],
        frame,
        fixture.channels,
        &mut fixture.transport,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    fixture
        .transport
        .advance_by_rendered_frames(output.len() / fixture.channels);
    processed
}

fn measured_capture_callback(fixture: &mut Fixture, output: &mut [f32], phase: &str) -> usize {
    let (processed, allocations) =
        crate::test_memory::observe_current_thread(|| fixed_capture_callback(fixture, output));
    assert_eq!(
        allocations,
        crate::test_memory::ThreadAllocations::default(),
        "{phase}"
    );
    processed
}

#[test]
fn one_ready_wet_group_starts_its_original_recording_at_the_same_immediate_effect() {
    for late in [false, true] {
        let mut fixture = Fixture::new(true);
        adopt_current_sources(&mut fixture, true);
        refresh_actual_runtime_bindings(&fixture);
        fixture.mixer.set_bpm_lock(true);
        fixture.mixer.set_master_bpm(120.0);
        fixture.mixer.set_pad_bpm(0, Some(150.0));
        fixture.mixer.set_pad_bpm(1, Some(120.0));
        fixture.control.engine.select_input_pad(0).unwrap();
        let deadline = Instant::now() + CAPTURE_WAIT;
        loop {
            fixture.render(0);
            if fixture
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .selected_endpoint_ready()
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut rig = CaptureRig::with_fixture(fixture, 16);
        let captured = public_capture(&rig.fixture);
        rig.fixture.drain();
        if late {
            assert!(rig.fixture.render(512).iter().all(|sample| *sample == 0.0));
        }
        let execution = if late { 512 } else { 0 };
        rig.fixture.feedback.reserve(64);
        let batch = rig.publish(&captured);
        assert_eq!(
            measured_capture_callback(
                &mut rig.fixture,
                &mut [],
                "ready group and recording immediate commit",
            ),
            1
        );
        assert_eq!(batch.publication_status(), "accepted");
        assert_eq!(rig.status().first_frame, Some(execution));
        assert!(
            captured
                .owner
                .members
                .iter()
                .all(|member| member.start.effective_frame() == Some(execution))
        );
        let pcm = rig.fixture.pcm(1024);
        rig.stop(execution + 1024);
        let settled = rig.joined();
        assert_eq!(settled.first_frame, Some(execution));
        assert_eq!(settled.end_frame, Some(execution + 1024));
        assert_eq!(settled.written_frames, 1024);
        assert_eq!(
            rig.bits(),
            pcm.into_iter().map(f32::to_bits).collect::<Vec<_>>()
        );
    }
}

#[test]
fn attached_group_admission_prepared_poll_claim_commit_and_render_make_no_rust_allocator_requests()
{
    // Both the dry birth and the actual prepared non-neutral Native birth cross
    // the new final RefCell/borrowed-claim/extract-restore path under observation.
    // Worker allocation, Native/C++ allocation and outer CPAL work are separate.
    for lock in [false, true] {
        // This complete-PCM fixture fits its declared 256 KiB pool even if
        // the actual writer gets no host time during the measured rendering.
        let mut rig = CaptureRig::with_block_count(lock, 16);
        if lock {
            restore_nonzero(&mut rig.fixture);
        }
        rig.fixture.render_to(1000);
        let captured = public_capture(&rig.fixture);
        drain_grid(&mut rig.fixture);
        let execution = 24_000;
        assert_eq!(captured.original_frame(), Some(execution));
        rig.fixture.feedback.reserve(64);
        assert!(rig.fixture.feedback.capacity() >= 64);
        fixed_capture_callback(&mut rig.fixture, &mut []);
        let feedback_capacity = rig.fixture.feedback.capacity();
        let feedback_address = rig.fixture.feedback.as_ptr();
        let batch = rig.publish(&captured);
        assert_eq!(batch.publication_status(), "pending");
        assert!(!rig.status().started);

        assert_eq!(
            measured_capture_callback(&mut rig.fixture, &mut [], "attached group FIFO admission"),
            1
        );
        assert_eq!(rig.fixture.scheduler.len(), 1);
        assert_eq!(
            rig.fixture.scheduler.peek_next_target_frame(),
            Some(execution)
        );
        assert!(!rig.status().started);
        let deadline = Instant::now() + CAPTURE_WAIT;
        loop {
            measured_capture_callback(
                &mut rig.fixture,
                &mut [],
                "attached group prepared-result polling",
            );
            if rig
                .fixture
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .ordinary_group_ready(2)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "actual attached group worker readiness timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut before = [0.0; 512];
        while rig.fixture.transport.output_frame() < execution - 7 {
            let frames = (execution - 7 - rig.fixture.transport.output_frame()).min(512) as usize;
            measured_capture_callback(
                &mut rig.fixture,
                &mut before[..frames],
                "attached group lead render",
            );
            assert!(before[..frames].iter().all(|sample| *sample == 0.0));
        }
        assert!(
            rig.fixture
                .control
                .engine
                .stop_mix_capture_internal(&rig.ticket, execution + GLOBAL_PCM_FRAMES as u64)
                .unwrap()
        );
        let mut around = vec![0.0; GLOBAL_PCM_FRAMES + 67];
        let callback_frames = [29, 511, 1024, 557];
        let mut cursor = 0;
        let mut callback = 0;
        let mut processed = 0;
        while cursor < around.len() {
            let frames =
                callback_frames[callback % callback_frames.len()].min(around.len() - cursor);
            processed += measured_capture_callback(
                &mut rig.fixture,
                &mut around[cursor..cursor + frames],
                "attached group final claim/voice commit/capture start-stop/render",
            );
            cursor += frames;
            callback += 1;
        }
        assert_eq!(
            processed, 1,
            "the original reserved-stop request was admitted in the measured seam"
        );
        assert_eq!(rig.fixture.feedback.capacity(), feedback_capacity);
        assert_eq!(rig.fixture.feedback.as_ptr(), feedback_address);
        assert!(around[..7].iter().all(|sample| *sample == 0.0));
        let recorded = &around[7..7 + GLOBAL_PCM_FRAMES];
        assert_eq!(
            batch.publication_status(),
            "accepted",
            "lock={lock}, capture={:?}, original={} reason={:?}",
            rig.status(),
            captured.state(),
            captured.reason()
        );
        assert_eq!(captured.state(), "applied");
        assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
        for member in &captured.owner.members {
            assert_eq!(member.start.effective_frame(), Some(execution));
        }
        assert!(
            recorded.iter().any(|sample| sample.abs() > 0.01),
            "lock={lock}, capture={:?}",
            rig.status()
        );
        if lock {
            // The independent Native reference retains EVERY startup frame;
            // its declared spectral window begins at 8192, not 2077.
            assert_complete_global_birth_pcm(
                recorded,
                [(LOOP_START, LOOP_END), (LOOP_START, LOOP_END)],
            );
        } else {
            for (frame, actual) in recorded.iter().enumerate() {
                let source_frame = LOOP_START + frame % (LOOP_END - LOOP_START);
                let source = [160.0, 320.0].map(|hz| {
                    (0.25
                        * (source_frame as f64 * hz * std::f64::consts::TAU / f64::from(RATE))
                            .sin()) as f32
                });
                assert!(
                    (*actual - (source[0] + source[1])).abs() < 2.0e-6,
                    "dry complete-prefix PCM frame={frame}: {actual}"
                );
            }
        }
        let snapshot = rig.joined();
        assert!(snapshot.successful_spool);
        assert_eq!(snapshot.failure, None);
        assert_eq!(
            (snapshot.first_frame, snapshot.end_frame),
            (Some(execution), Some(execution + GLOBAL_PCM_FRAMES as u64))
        );
        assert_eq!(
            (snapshot.submitted_frames, snapshot.written_frames),
            (GLOBAL_PCM_FRAMES as u64, GLOBAL_PCM_FRAMES as u64)
        );
        assert_eq!(
            rig.bits(),
            recorded
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn real_python_binding_accepts_original_capture_keyword_and_omitted_or_none_routes() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/record-capture-20261020");
    fs::create_dir_all(&parent).unwrap();
    // Retain the tiny owned fixture even on unwind; no TempDir destructor can
    // remove a spool while the real callback/worker still owns it.
    let root = tempfile::Builder::new()
        .prefix("atomic-python-binding-")
        .tempdir_in(parent)
        .unwrap()
        .keep();
    fs::write(root.join("OWNER.md"), b"Owned actual PyO3 atomic GLOBAL capture binding regression: synthetic PCM, original live AudioEngine/ticket, actual callback and spool writer. No device or Human files. Retained on success/failure; remove only after actual RT release and writer join.\n").unwrap();
    for mode in ["omitted", "none", "capture"] {
        let mut fixture = Fixture::new(false);
        fixture.render_to(321);
        let captured = public_capture(&fixture);
        fixture.drain();
        assert_eq!(captured.original_frame(), Some(321));
        // Move the real original engine into its actual opaque PyO3 class. The
        // empty field placeholder is never used to publish, authorize or ACK;
        // the fixture's retained consumer/mixer keep the original authority.
        let engine = Python::attach(|py| {
            Py::new(
                py,
                std::mem::replace(&mut fixture.control.engine, AudioEngine::new().unwrap()),
            )
            .unwrap()
        });
        let path = root.join(format!("{mode}.capture"));
        let ticket: Option<Py<MixCaptureTicket>> = if mode == "capture" {
            Some(Python::attach(|py| {
                engine
                    .bind(py)
                    .call_method1(
                        "prepare_mix_capture",
                        (path.to_str().unwrap(), 4096, 8, 256 * 1024),
                    )
                    .unwrap()
                    .extract()
                    .unwrap()
            }))
        } else {
            None
        };
        if let Some(ticket) = &ticket {
            let deadline = Instant::now() + CAPTURE_WAIT;
            while !Python::attach(|py| status(&engine.borrow(py), &ticket.borrow(py))).writer_ready
            {
                assert!(
                    Instant::now() < deadline,
                    "actual Python-prepared writer did not become ready"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let batch: Py<GlobalPlaybackBatchTicket> = Python::attach(|py| {
            let original_group = Py::new(py, captured.clone()).unwrap();
            let kwargs = pyo3::types::PyDict::new(py);
            if let Some(ticket) = &ticket {
                kwargs.set_item("capture", ticket.bind(py)).unwrap();
            } else if mode == "none" {
                kwargs.set_item("capture", py.None()).unwrap();
            }
            engine
                .bind(py)
                .call_method(
                    "publish_captured_global_start",
                    (original_group,),
                    if mode == "omitted" {
                        None
                    } else {
                        Some(&kwargs)
                    },
                )
                .unwrap()
                .extract()
                .unwrap()
        });
        assert_eq!(
            Python::attach(|py| batch.borrow(py).publication_status()),
            "pending"
        );
        if let Some(ticket) = &ticket {
            let before = Python::attach(|py| status(&engine.borrow(py), &ticket.borrow(py)));
            assert!(before.start_queued && !before.started);
            assert_eq!(before.first_frame, None);
        }
        fixture.drain();
        let output = fixture.render(31);
        assert_eq!(
            Python::attach(|py| batch.borrow(py).publication_status()),
            "accepted"
        );
        assert_eq!(captured.state(), "applied");
        assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
        for member in &captured.owner.members {
            assert_eq!(member.start.effective_frame(), Some(321));
        }
        if let Some(ticket) = &ticket {
            assert!(Python::attach(|py| engine
                .bind(py)
                .call_method1("stop_mix_capture", (ticket.bind(py),),)
                .unwrap()
                .extract::<bool>()
                .unwrap()));
            fixture.drain();
            let deadline = Instant::now() + CAPTURE_WAIT;
            let settled = loop {
                fixture.drain();
                fixture.render(0);
                let snapshot = Python::attach(|py| status(&engine.borrow(py), &ticket.borrow(py)));
                if snapshot.rt_released && snapshot.worker_joined {
                    break snapshot;
                }
                assert!(
                    Instant::now() < deadline,
                    "actual Python-owned capture settlement timed out: {snapshot:?}"
                );
                std::thread::sleep(Duration::from_millis(1));
            };
            assert!(settled.successful_spool);
            assert_eq!(settled.failure, None);
            assert_eq!(
                (settled.first_frame, settled.end_frame),
                (Some(321), Some(352))
            );
            assert_eq!((settled.submitted_frames, settled.written_frames), (31, 31));
            let actual = fs::read(&path).unwrap()[SPOOL_HEADER_BYTES as usize..]
                .chunks_exact(4)
                .map(|sample| u32::from_le_bytes(sample.try_into().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(
                actual,
                output
                    .iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>()
            );
        } else {
            assert!(Python::attach(|py| engine
                .borrow(py)
                .mix_capture_owner
                .lock()
                .unwrap()
                .is_none()));
            assert!(
                !path.exists(),
                "omitted/None must not prepare or attach a capture"
            );
        }
    }
}

#[test]
fn real_group_and_spool_share_nonzero_effective_frame_and_exact_complete_mix_extent() {
    let mut rig = CaptureRig::with_block_count(true, 16);
    restore_nonzero(&mut rig.fixture);
    rig.fixture.render_to(1000);
    let captured = public_capture(&rig.fixture);
    drain_grid(&mut rig.fixture);
    assert_eq!(captured.original_frame(), Some(24_000));
    assert!(
        rig.fixture
            .render_to(30_000)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let batch = rig.publish(&captured);
    drain_grid(&mut rig.fixture);
    let execution = rig.fixture.scheduler.peek_next_target_frame().unwrap();
    assert_eq!(execution, 38_192);
    assert_eq!(rig.fixture.scheduler.len(), 1, "one existing group action");
    assert!(rig.status().start_queued);
    assert!(!rig.status().started);
    wait_group(&mut rig.fixture);
    assert!(
        rig.fixture
            .render_to(execution - 7)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    rig.stop(execution + GLOBAL_PCM_FRAMES as u64);
    let mut around = Vec::with_capacity(GLOBAL_PCM_FRAMES + 67);
    let callback_frames = [29, 511, 1024, 557];
    let mut callback = 0;
    while around.len() < GLOBAL_PCM_FRAMES + 67 {
        let frames = callback_frames[callback % callback_frames.len()]
            .min(GLOBAL_PCM_FRAMES + 67 - around.len());
        around.extend(rig.fixture.render(frames));
        callback += 1;
    }
    assert!(around[..7].iter().all(|sample| *sample == 0.0));
    let expected = &around[7..7 + GLOBAL_PCM_FRAMES];
    assert_eq!(
        batch.publication_status(),
        "accepted",
        "capture={:?}, original={} reason={:?}",
        rig.status(),
        captured.state(),
        captured.reason()
    );
    assert_eq!(captured.state(), "applied");
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
    for member in &captured.owner.members {
        assert_eq!(member.start.original_frame(), Some(24_000));
        assert_eq!(member.start.effective_frame(), Some(execution));
    }
    // Capture correctness includes the entire independent Native startup and
    // first attack. The former 2077-frame audibility assertion ended before
    // this existing reference's explicitly declared later spectral window.
    assert_complete_global_birth_pcm(expected, [(LOOP_START, LOOP_END), (LOOP_START, LOOP_END)]);
    assert!(expected.iter().any(|sample| sample.abs() > 0.01));
    assert!(
        around[7 + GLOBAL_PCM_FRAMES..]
            .iter()
            .any(|sample| sample.abs() > 0.001)
    );
    let snapshot = rig.joined();
    assert!(snapshot.successful_spool);
    assert_eq!(snapshot.failure, None);
    assert_eq!(snapshot.first_frame, Some(execution));
    assert_eq!(
        snapshot.end_frame,
        Some(execution + GLOBAL_PCM_FRAMES as u64)
    );
    assert_eq!(
        (snapshot.submitted_frames, snapshot.written_frames),
        (GLOBAL_PCM_FRAMES as u64, GLOBAL_PCM_FRAMES as u64)
    );
    assert_eq!(
        rig.bits(),
        expected
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn own_stop_at_common_effective_frame_starts_both_pads_then_seals_empty_capture() {
    let mut rig = CaptureRig::new(false);
    rig.fixture.render_to(1000);
    let captured = public_capture(&rig.fixture);
    drain_grid(&mut rig.fixture);
    let batch = rig.publish(&captured);
    drain_grid(&mut rig.fixture);
    assert_eq!(rig.fixture.scheduler.peek_next_target_frame(), Some(24_000));
    rig.stop(24_000);
    assert!(
        rig.fixture
            .render_to(23_999)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    rig.fixture.render(2);
    assert_eq!(batch.publication_status(), "accepted");
    assert_eq!(captured.state(), "applied");
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
    let snapshot = rig.joined();
    assert!(snapshot.started && snapshot.empty);
    assert!(!snapshot.successful_spool);
    assert_eq!(snapshot.failure, None);
    assert_eq!(
        (snapshot.first_frame, snapshot.end_frame),
        (Some(24_000), Some(24_000))
    );
    assert_eq!((snapshot.submitted_frames, snapshot.written_frames), (0, 0));
    assert!(rig.bits().is_empty());
}

#[test]
fn completed_foreign_event_at_common_frame_keeps_original_group_before_its_own_stop() {
    let mut rig = CaptureRig::new(false);
    let unrelated_source = rig.fixture.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    crate::audio_engine::pitch_runtime_tests::publish_tone(
        &rig.fixture.control,
        2,
        unrelated_source.clone(),
        true,
    );
    rig.fixture.mixer.load_sample(2, unrelated_source);
    rig.fixture.render_to(1000);
    // Reserve this unrelated scalar input order before the original group input.
    // Stopping the unrelated loaded pad completes without invalidating members 0 and 1.
    rig.fixture
        .scheduler
        .schedule(24_000, ScheduledCommand::StopSample { id: 2 })
        .unwrap();
    let captured = public_capture(&rig.fixture);
    drain_grid(&mut rig.fixture);
    let batch = rig.publish(&captured);
    drain_grid(&mut rig.fixture);
    rig.stop(24_000);
    assert!(
        rig.fixture
            .render_to(23_999)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    rig.fixture.render(2);
    assert!(rig.fixture.feedback.iter().any(|message| matches!(
        message,
        crate::messages::AudioMessage::SampleStopped { id: 2 }
            | crate::messages::AudioMessage::ContentFeedback { id: 2, kind: 2, .. }
    )));
    assert_eq!(batch.publication_status(), "accepted");
    assert_eq!(captured.state(), "applied");
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
    let snapshot = rig.joined();
    assert!(snapshot.started && snapshot.empty);
    assert_eq!(snapshot.failure, None);
    assert_eq!(
        (snapshot.first_frame, snapshot.end_frame),
        (Some(24_000), Some(24_000))
    );
    assert_eq!((snapshot.submitted_frames, snapshot.written_frames), (0, 0));
    assert!(rig.bits().is_empty());
}

#[test]
fn late_replan_crossing_original_stop_rejects_whole_group_without_recording_a_prefix() {
    let mut rig = CaptureRig::new(true);
    restore_nonzero(&mut rig.fixture);
    rig.fixture.render_to(1000);
    let captured = public_capture(&rig.fixture);
    drain_grid(&mut rig.fixture);
    let batch = rig.publish(&captured);
    drain_grid(&mut rig.fixture);
    wait_group(&mut rig.fixture);
    let execution = rig.fixture.scheduler.peek_next_target_frame().unwrap();
    let original_stop = execution + 64;
    rig.stop(original_stop);
    rig.fixture
        .scheduler
        .schedule(100_000, ScheduledCommand::StopAll)
        .unwrap();
    // Emulate the next actual callback arriving after E; the native output clock
    // advances, while no callback or source sample was rendered in that gap.
    rig.fixture.transport.advance_by_rendered_frames(
        (execution + 1 - rig.fixture.transport.output_frame()) as usize,
    );
    assert!(rig.fixture.render(1).iter().all(|sample| *sample == 0.0));
    assert_eq!(batch.publication_status(), "rejected");
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
    assert_eq!(
        rig.fixture.scheduler.peek_next_target_frame(),
        Some(100_000)
    );
    let snapshot = rig.joined();
    assert_eq!(snapshot.failure.as_deref(), Some("rejected"));
    assert!(!snapshot.started && !snapshot.successful_spool);
    assert_eq!((snapshot.first_frame, snapshot.end_frame), (None, None));
    assert_eq!(snapshot.written_frames, 0);
    assert!(rig.bits().is_empty());
}

#[test]
fn transferred_duplicate_batch_is_inert_and_preserves_original_active_capture() {
    let mut rig = CaptureRig::new(false);
    rig.fixture.render_to(257);
    let captured = public_capture(&rig.fixture);
    rig.fixture.drain();
    let batch = rig.publish(&captured);
    let ControlMessage::GlobalPlaybackBatch(message) = rig.fixture.control.consumer.pop().unwrap()
    else {
        panic!("original combined batch must be queued once");
    };
    {
        let mut producer = rig.fixture.control.producer.lock().unwrap();
        producer
            .push(ControlMessage::GlobalPlaybackBatch(message.clone()))
            .unwrap();
        producer
            .push(ControlMessage::GlobalPlaybackBatch(message.clone()))
            .unwrap();
    }
    rig.fixture.drain();
    let mut expected = rig.fixture.render(31);
    rig.fixture
        .control
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::GlobalPlaybackBatch(message))
        .unwrap();
    rig.fixture
        .control
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::StopMixCapture {
            session_id: rig.ticket.session_id() + 1,
            target_frame: MixCaptureTarget::At(1),
        })
        .unwrap();
    rig.fixture.drain();
    assert_eq!(batch.publication_status(), "accepted");
    assert_eq!(captured.state(), "applied");
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
    assert_eq!(rig.status().failure, None);
    expected.extend(rig.fixture.render(106));
    rig.stop(394);
    rig.fixture.render(0);
    let snapshot = rig.joined();
    assert!(snapshot.successful_spool);
    assert_eq!(
        (snapshot.first_frame, snapshot.end_frame),
        (Some(257), Some(394))
    );
    assert_eq!(
        rig.bits(),
        expected
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn full_scheduler_rejects_both_originals_and_preserves_every_foreign_event() {
    let mut rig = CaptureRig::new(false);
    let captured = public_capture(&rig.fixture);
    rig.fixture.drain();
    for frame in 100_000..100_016 {
        rig.fixture
            .scheduler
            .schedule(frame, ScheduledCommand::StopAll)
            .unwrap();
    }
    let batch = rig.publish(&captured);
    rig.fixture.drain();
    assert_eq!(batch.publication_status(), "rejected");
    assert_eq!(rig.fixture.scheduler.len(), 16);
    assert_eq!(
        rig.fixture.scheduler.peek_next_target_frame(),
        Some(100_000)
    );
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
    let snapshot = rig.joined();
    assert_eq!(snapshot.failure.as_deref(), Some("rejected"));
    assert!(!snapshot.started && !snapshot.successful_spool);
    assert_eq!(snapshot.first_frame, None);
    assert!(rig.bits().is_empty());
}

#[test]
fn foreign_combined_batch_cannot_cancel_or_replace_an_older_active_capture() {
    let mut original = CaptureRig::new(false);
    let captured = public_capture(&original.fixture);
    original.fixture.drain();
    let original_batch = original.publish(&captured);
    original.fixture.drain();
    let mut expected = original.fixture.render(11);

    let mut foreign = CaptureRig::new(false);
    let foreign_captured = public_capture(&foreign.fixture);
    foreign.fixture.drain();
    let foreign_batch = foreign.publish(&foreign_captured);
    let message = foreign.fixture.control.consumer.pop().unwrap();
    original
        .fixture
        .control
        .producer
        .lock()
        .unwrap()
        .push(message)
        .unwrap();
    original.fixture.drain();
    assert_eq!(foreign_batch.publication_status(), "rejected");
    assert_eq!(original_batch.publication_status(), "accepted");
    assert_eq!(original.status().failure, None);
    assert!(original.status().started);
    assert_eq!(
        (original.fixture.starts(0), original.fixture.starts(1)),
        (1, 1)
    );
    let foreign_status = foreign.joined();
    assert!(!foreign_status.started && !foreign_status.successful_spool);
    assert_eq!(foreign_status.failure.as_deref(), Some("rejected"));
    assert!(foreign.bits().is_empty());
    expected.extend(original.fixture.render(23));
    original.stop(34);
    original.fixture.render(0);
    assert!(original.joined().successful_spool);
    assert_eq!(
        original.bits(),
        expected
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn retirement_pressure_retains_both_untransferred_owners_then_admits_one_actual_frame() {
    let mut rig = CaptureRig::new(false);
    rig.fixture.render_to(300);
    let captured = public_capture(&rig.fixture);
    rig.fixture.drain();
    let mut pause = RetirementPause::install(&mut rig.fixture);
    pause.fill(&mut rig.fixture);
    let batch = rig.publish(&captured);
    rig.fixture.drain();
    assert_eq!(batch.publication_status(), "pending");
    assert!(rig.fixture.scheduler.is_empty());
    assert!(!rig.status().started);
    assert!(rig.fixture.render(11).iter().all(|sample| *sample == 0.0));
    pause.release();
    let deadline = Instant::now() + CAPTURE_WAIT;
    while batch.publication_status() == "pending" {
        rig.fixture.drain();
        rig.fixture.render(0);
        assert!(
            Instant::now() < deadline,
            "retirement admission did not resume"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(batch.publication_status(), "accepted");
    assert_eq!(captured.original_frame(), Some(300));
    assert_eq!(rig.status().first_frame, Some(311));
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
    let expected = rig.fixture.render(17);
    rig.stop(328);
    rig.fixture.render(0);
    assert!(rig.joined().successful_spool);
    assert_eq!(
        rig.bits(),
        expected
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn due_own_stop_settles_pending_group_despite_full_retirement_without_false_start() {
    let mut rig = CaptureRig::new(true);
    restore_nonzero(&mut rig.fixture);
    rig.fixture.render_to(1000);
    let captured = public_capture(&rig.fixture);
    drain_grid(&mut rig.fixture);
    let batch = rig.publish(&captured);
    drain_grid(&mut rig.fixture);
    wait_group(&mut rig.fixture);
    let execution = rig.fixture.scheduler.peek_next_target_frame().unwrap();
    rig.stop(execution);
    let mut pause = RetirementPause::install(&mut rig.fixture);
    pause.fill(&mut rig.fixture);
    rig.fixture.render_to(execution);
    assert!(rig.fixture.render(1).iter().all(|sample| *sample == 0.0));
    let snapshot = rig.status();
    assert!(snapshot.rt_sealed && !snapshot.rt_released);
    assert!(!snapshot.started);
    assert_eq!(snapshot.first_frame, None);
    assert_eq!(snapshot.failure.as_deref(), Some("cancelled"));
    assert_eq!(batch.publication_status(), "rejected");
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
    pause.release();
    let snapshot = rig.joined();
    assert!(!snapshot.started && !snapshot.successful_spool);
    assert!(rig.bits().is_empty());
}

#[test]
fn real_control_cancellation_before_or_after_capture_claim_has_one_combined_winner() {
    for after_claim in [false, true] {
        let mut rig = CaptureRig::new(false);
        rig.fixture.render_to(700);
        let captured = public_capture(&rig.fixture);
        rig.fixture.drain();
        let batch = rig.publish(&captured);
        // Keep the original engine/control authority alive while the callback
        // runs; the empty placeholder is never used to capture or acknowledge.
        let original_engine = Arc::new(std::mem::replace(
            &mut rig.fixture.control.engine,
            AudioEngine::new().unwrap(),
        ));
        let hook_engine = original_engine.clone();
        let hook_ticket = rig.ticket.clone();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            RtMixer::with_group_capture_claim_hook_for_test(
                after_claim,
                move || {
                    assert!(!status(&hook_engine, &hook_ticket).started);
                    assert!(
                        hook_engine
                            .cancel_mix_capture_internal(&hook_ticket)
                            .unwrap()
                    );
                    let snapshot = status(&hook_engine, &hook_ticket);
                    assert!(!snapshot.started);
                    assert_eq!(snapshot.first_frame, None);
                    assert_eq!(snapshot.failure.as_deref(), Some("cancelled"));
                },
                || rig.fixture.drain(),
            );
        }));
        rig.fixture.control.engine = Arc::try_unwrap(original_engine)
            .ok()
            .expect("consumed claim hook must release the original engine");
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
        rig.fixture.render(1);
        assert!(
            captured.effect_claimed(),
            "even a subsequent capture-permission failure closes the group gate permanently"
        );
        let snapshot = rig.joined();
        assert_eq!(snapshot.failure.as_deref(), Some("cancelled"));
        assert!(!snapshot.successful_spool);
        if after_claim {
            assert_eq!(batch.publication_status(), "accepted");
            assert_eq!(captured.state(), "applied");
            assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
            assert!(snapshot.started);
            assert_eq!(snapshot.first_frame, Some(700));
        } else {
            assert_eq!(batch.publication_status(), "rejected");
            assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
            assert!(!snapshot.started);
            assert_eq!(snapshot.first_frame, None);
        }
    }
}

#[test]
fn original_group_cancel_immediately_before_or_after_the_effect_gate_has_one_winner() {
    for after_claim in [false, true] {
        let mut rig = CaptureRig::new(false);
        rig.fixture.render_to(700);
        let captured = public_capture(&rig.fixture);
        rig.fixture.drain();
        let batch = rig.publish(&captured);
        let cancellation = captured.clone();
        RtMixer::with_group_capture_claim_hook_for_test(
            after_claim,
            move || assert_eq!(cancellation.cancel(), !after_claim),
            || rig.fixture.drain(),
        );
        rig.fixture.render(1);
        assert_eq!(
            (rig.fixture.starts(0), rig.fixture.starts(1)),
            if after_claim { (1, 1) } else { (0, 0) }
        );
        assert_eq!(
            batch.publication_status(),
            if after_claim { "accepted" } else { "rejected" }
        );
        if after_claim {
            rig.stop(701);
        }
        let snapshot = rig.joined();
        assert_eq!(snapshot.started, after_claim);
        assert!(
            !captured.cancel(),
            "a completed cancellation or effect gate cannot reopen"
        );
    }
}
