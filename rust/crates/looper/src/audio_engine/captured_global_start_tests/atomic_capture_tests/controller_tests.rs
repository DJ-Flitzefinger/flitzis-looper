//! Original PyO3 engine -> real recording controller/Thread -> callback -> WAV.
//!
//! Run these ignored cases in an isolated Windows libtest process with an actual
//! FLITZIS_RECORDING_FORMAT_PYTHON. The independent dry PCM oracle includes every
//! first/exclusive-end sample. This is neither App/UI nor CPAL/hearing acceptance.
//! Additive production cases traverse the real UiContext recording facade and
//! Transport/GlobalPlayback owners without constructing App or rendering ImGui.
#![cfg(windows)]

use super::*;
use pyo3::types::{PyBytes, PyModule};
use std::ffi::CString;
use std::path::Path;

const CONTROLLER_WAIT: Duration = Duration::from_secs(120);
const CONTROLLER_PYTHON: &str = "FLITZIS_RECORDING_FORMAT_PYTHON";

const CONTROLLER_PROBE: &str = r#"
import hashlib
import json
import os
import time
from contextlib import contextmanager
from pathlib import Path
from threading import Event

from flitzis_looper.controller.recording import RecordingController
from flitzis_looper.models import ProjectState
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_paths import hold_live_recording_spool
from flitzis_looper.recording_wav import inspect_recording_wav
from flitzis_looper_audio import inspect_mix_capture_spool


def identity(path):
    state = path.stat(follow_symlinks=False)
    return state.st_dev, state.st_ino


class OuterReleaseGate:
    # This test-only pause retains the ACTUAL live guard. It stores no engine,
    # worker, controller, Native ticket, codec result, exception or traceback.
    def __init__(self, pause):
        self.pause = pause
        self.blocked = Event()
        self.release = Event()
        self.identity = None
        if not pause:
            self.release.set()

    @contextmanager
    def hold(self, path):
        with hold_live_recording_spool(path) as original_identity:
            self.identity = original_identity
            try:
                yield original_identity
            finally:
                if self.pause:
                    self.blocked.set()
                    if not self.release.wait(120):
                        raise TimeoutError('actual outer live-spool release gate timed out')


class OriginalGroupGate:
    # A fixture adapter for the new external gate seam, not a second transport
    # controller. The real original capture and its exact native bindings own
    # validation and the same existing atomic FIFO batch owns publication/ACK.
    def __init__(self, engine):
        self.engine = engine
        self.entries = []
        for pad in (0, 1):
            binding = engine.current_input_runtime_pad_binding(pad)
            assert binding is not None and binding.is_current()
            self.entries.append((binding, 0.1, 2.1, None))
        self.original = engine.capture_global_start(self.entries)
        self.batch = None
        self.accepted_publications = 0

    def is_current(self):
        return self.original.state() != 'rejected' and all(
            entry[0].is_current() for entry in self.entries
        )

    def publish(self, ticket):
        assert self.batch is None
        batch = self.engine.publish_captured_global_start(self.original, capture=ticket)
        if batch is None:
            return False
        self.batch = batch
        self.accepted_publications += 1
        return True

    def cancel(self):
        self.original.cancel()


class ControllerCase:
    def __init__(self, engine, root, route):
        self.engine = engine
        self.root = Path(root)
        self.route = route
        self.record_root = self.root / 'record'
        self.record_root.mkdir()
        self.neighbor = self.record_root / 'unknown-neighbor.bin'
        self.neighbor_bytes = b'unknown recording neighbor must remain unchanged\x00\xff'
        self.neighbor.write_bytes(self.neighbor_bytes)
        self.neighbor_identity = identity(self.neighbor)
        self.outer = OuterReleaseGate(route == 'right')
        self.controller = self.make_recording_controller(engine)
        self.worker = None
        self.ticket = None
        self.paths = None
        self.group = None
        self.capture_count = 0
        self.shutdown_requested = False

    def make_recording_controller(self, engine):
        return RecordingController(
            ProjectState(recording_options=RecordingOptions()), engine, self.record_root,
            live_spool_witness=self.outer.hold,
        )

    def capture_original(self):
        self.capture_count += 1
        assert self.capture_count == 1
        assert self.worker is None, 'original left press must precede worker readiness'
        self.group = OriginalGroupGate(self.engine)
        return self.group

    def forbidden_recapture(self):
        raise AssertionError('busy input recaptured a group or admitted another take')

    def begin(self):
        if self.route == 'left':
            self.controller.record_group(self.capture_original)
        else:
            self.controller.record_mix()
        owner = self.controller._owner
        assert owner is not None
        self.worker = owner.worker
        assert self.controller.status().phase == 'Preparing'
        assert self.controller.status().busy and not self.controller.status().retired
        self.controller.record_mix()
        self.controller.record_group(self.forbidden_recapture)
        assert self.controller._owner.worker is self.worker
        assert not self.controller.acknowledge_failure()
        assert self.capture_count == (1 if self.route == 'left' else 0)

    def observe(self):
        self.controller.poll()
        work = self.worker.snapshot()
        if work.paths is not None:
            self.paths = work.paths
        if work.ready is not None:
            if self.ticket is None:
                self.ticket = work.ready.ticket
            assert self.ticket is work.ready.ticket
        status = self.controller.status()
        assert status.phase != 'Failed', status
        return status

    def poll_recording(self):
        status = self.observe()
        if not status.effective_recording:
            return False
        assert status.phase == 'Recording'
        assert self.ticket is not None and self.paths is not None
        native = self.engine.mix_capture_status(self.ticket)
        assert native['started'] is True and native['rt_sealed'] is False
        assert native['session_id'] == self.ticket.session_id()
        assert native['spool_path'] == str(self.paths.spool_path)
        assert identity(self.paths.spool_path) == self.outer.identity
        assert self.worker._preflight_retired.is_set()
        assert native['start_target_mode'] == (
            'explicit' if self.route == 'left' else 'next_admissible'
        )
        if self.group is not None:
            assert self.group.accepted_publications == 1
            assert self.group.batch.publication_status() == 'accepted'
            assert self.group.original.state() == 'applied'
        return True

    def stop(self):
        assert self.controller.status().effective_recording
        self.controller.record_mix()
        assert self.controller._owner.worker is self.worker
        native = self.engine.mix_capture_status(self.ticket)
        assert native['stop_queued'] is True
        assert native['stop_target_mode'] == 'next_admissible'
        assert native['stop_target'] is None

    def request_shutdown(self):
        assert self.controller.status().effective_recording
        self.shutdown_requested = True
        assert not self.controller.prepare_shutdown(deadline=time.monotonic() + 90)
        native = self.engine.mix_capture_status(self.ticket)
        assert native['stop_queued'] is True
        assert native['stop_target_mode'] == 'next_admissible'
        assert self.controller.status().busy

    def poll_outer_blocked(self):
        self.observe()
        return self.outer.blocked.is_set()

    def check_outer_busy_and_release(self):
        status = self.observe()
        assert self.outer.blocked.is_set()
        assert status.phase == 'Finalizing'
        assert status.busy and not status.retired
        assert status.last_completion is not None
        assert status.last_completion.path.is_file()
        assert self.worker._finalizer_retired.is_set(), 'private finalizer really returned'
        assert not self.worker._done.is_set()
        assert self.worker._thread.is_alive()
        assert not self.worker.snapshot().cleanup_complete
        assert self.worker._resources is not None
        self.controller.record_mix()
        self.controller.record_group(self.forbidden_recapture)
        self.controller.discover_recovery()
        assert self.controller._owner.worker is self.worker
        assert not self.controller.acknowledge_failure()
        assert len(tuple(self.record_root.glob('.incomplete-*'))) == 1
        self.outer.release.set()

    def poll_idle(self):
        status = self.observe()
        return status.phase == 'Idle' and status.retired and not status.busy

    def finish(self, expected_payload, first, end):
        expected_payload = bytes(expected_payload)
        frames = end - first
        status = self.observe()
        assert status.phase == 'Idle' and status.retired and not status.busy
        assert self.controller._owner is None
        completion = status.last_completion
        assert completion is not None
        assert completion.path == self.paths.final_path
        assert completion.extent.first_frame == first
        assert completion.extent.end_frame == end
        assert completion.extent.frames == frames > 0
        assert completion.extent.rate == 48000 and completion.extent.channels == 1
        assert completion.extent.session_id == self.ticket.session_id()
        digest = hashlib.sha256(expected_payload).hexdigest()
        assert completion.extent.payload_sha256 == digest
        assert completion.pcm.source_payload_sha256 == digest
        assert completion.pcm.output_payload_sha256 == digest
        assert completion.codec.decoded_payload_sha256 == digest
        assert completion.codec.decoded_frames == frames
        assert completion.warning is None
        assert self.worker._done.is_set() and not self.worker._thread.is_alive()
        assert self.worker.join_if_done()
        assert self.worker.snapshot().cleanup_complete
        assert self.worker.snapshot().ready is None
        assert self.worker._resources is None and self.worker._audio is None
        assert self.worker._preflight_retired.is_set()
        assert self.worker._finalizer_retired.is_set()
        native = self.engine.mix_capture_status(self.ticket)
        assert all(native[key] is True for key in (
            'started', 'terminal_decided', 'rt_sealed', 'rt_released',
            'worker_closed', 'worker_joined', 'successful_spool',
        )), native
        assert native['writer_ready'] is False and native['empty'] is False
        assert native['failure'] is native['failure_code'] is native['error_detail'] is None
        assert (native['first_frame'], native['end_frame']) == (first, end)
        assert native['submitted_frames'] == native['written_frames'] == frames
        assert native['buffered_frames'] == 0
        assert native['payload_sha256'] == digest
        assert self.outer.identity == identity(self.paths.spool_path)
        spool = self.paths.spool_path.read_bytes()
        assert len(spool) == 256 + len(expected_payload)
        assert spool[256:] == expected_payload
        checkpoint = inspect_mix_capture_spool(str(self.paths.spool_path))
        assert checkpoint['state'] == 'sealed_candidate'
        assert checkpoint['requires_verification'] is True
        assert checkpoint['publication_ready'] is False
        with completion.path.open('rb') as source:
            wav = inspect_recording_wav(source)
            assert (wav.encoding, wav.rate, wav.channels, wav.frames) == (
                'float32', 48000, 1, frames,
            )
            source.seek(wav.data_offset)
            assert source.read(wav.data_bytes) == expected_payload
        assert self.paths.pcm_path.read_bytes() == expected_payload
        # Float WAV is verified directly from its complete container payload;
        # the production verifier intentionally creates no fourth decoded file.
        assert not self.paths.decoded_path.exists()
        assert identity(completion.path) == identity(self.paths.encoded_path)
        assert identity(completion.path) == completion.codec.output_identity
        encoded = completion.path.read_bytes()
        assert len(encoded) == completion.codec.output_bytes
        assert hashlib.sha256(encoded).hexdigest() == completion.codec.output_sha256
        assert self.neighbor.read_bytes() == self.neighbor_bytes
        assert identity(self.neighbor) == self.neighbor_identity
        # Real unchanged-byte writes and an owned directory move exercise actual
        # release; a metadata flag alone cannot pass Windows deny-write/delete.
        for path in (self.paths.spool_path, self.paths.pcm_path,
                     self.paths.encoded_path, completion.path):
            with path.open('r+b') as output:
                byte = output.read(1)
                assert len(byte) == 1
                output.seek(0)
                assert output.write(byte) == 1
                output.flush()
                os.fsync(output.fileno())
        shifted = self.paths.directory.with_name(self.paths.directory.name + '.released')
        self.paths.directory.rename(shifted)
        shifted.rename(self.paths.directory)
        assert self.paths.spool_path.read_bytes()[256:] == expected_payload
        assert hashlib.sha256(completion.path.read_bytes()).hexdigest() == completion.codec.output_sha256
        assert self.capture_count == (1 if self.route == 'left' else 0)
        if self.shutdown_requested:
            assert self.controller.prepare_shutdown(deadline=time.monotonic() + 1)
            self.controller.shut_down()
        return json.dumps({
            'route': self.route, 'frames': frames, 'first_frame': first,
            'exclusive_end_frame': end, 'session_id': self.ticket.session_id(),
            'original_capture_count': self.capture_count,
            'writer_payload_sha256': digest,
            'output_identity': completion.codec.output_identity,
            'output_sha256': completion.codec.output_sha256,
            'private_preflight_retired': self.worker._preflight_retired.is_set(),
            'private_finalizer_retired': self.worker._finalizer_retired.is_set(),
            'actual_take_thread_joined': self.worker.join_if_done(),
            'public_retired': status.retired, 'outer_release_exercised': True,
            'shutdown_before_audio_release': self.shutdown_requested,
            'unknown_neighbor_preserved': True,
        }, sort_keys=True)

    def abort_and_poll(self):
        # Unwind cleanup never invents retirement or restores the interpreter
        # while an actual owned codec child could still need it.
        self.outer.release.set()
        settled = self.controller.prepare_shutdown(deadline=time.monotonic() - 1)
        if settled:
            self.controller.shut_down()
        return settled
"#;

// Additive composition over the existing full-file/original-worker oracles above.
// The tiny envelope supplies no audio, source, gate, ACK or retirement implementation.
const PRODUCTION_GLOBAL_PROBE: &str = r#"
from types import SimpleNamespace

from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import SessionState
from flitzis_looper.ui.context import UiContext


class ProductionGlobalCase(ControllerCase):
    def __init__(self, engine, root, route):
        self.prepare_blocked = Event()
        self.prepare_release = Event()
        super().__init__(engine, root, route)
        self.session = SessionState(
            global_stop_engaged=True, global_stop_restore_sample_ids={0, 1},
        )
        self.transport = TransportController(self.project, self.session, engine)
        self.global_owner = self.transport.playback._global_playback
        self.envelope = SimpleNamespace(
            project=self.project, session=self.session, transport=self.transport,
            recording=self.controller, persistence=None,
        )
        self.ui = UiContext(self.envelope)
        self.original = None
        self.production_gate = None
        self.batch = None
        self.press_timestamp = None
        original_factory = self.transport.playback.capture_recording_start

        def observed_original_factory(*, received_at_ns=None):
            # Observation only: delegate to the real production facade and gate.
            self.capture_count += 1
            assert self.capture_count == 1
            assert self.worker is None and self.controller._owner is None
            assert received_at_ns == self.press_timestamp
            gate = original_factory(received_at_ns=received_at_ns)
            assert gate is not None
            assert type(gate).__module__ == 'flitzis_looper.controller.transport.global_playback'
            self.production_gate = gate
            self.original = gate._capture
            return gate

        self.transport.playback.capture_recording_start = observed_original_factory

    def make_recording_controller(self, engine):
        self.project = ProjectState(recording_options=RecordingOptions(), multi_loop=True)
        for pad in (0, 1):
            binding = engine.current_input_runtime_pad_binding(pad)
            assert binding is not None and binding.is_current()
            metadata = binding.metadata()
            self.project.sample_paths[pad] = str(self.root / f'fixture-source-{pad}.wav')
            self.project.sample_durations[pad] = metadata['frame_count'] / metadata['sample_rate_hz']
            self.project.pad_loop_start_s[pad] = 0.1
            self.project.pad_loop_end_s[pad] = 2.1
            self.project.pad_loop_auto[pad] = False
        # Source PCM/Native ownership comes from the real existing Rust fixture;
        # these paths are display intent, not a disk-load or restore assertion.
        return RecordingController(
            self.project, engine, self.record_root,
            live_spool_witness=self.hold_before_preflight,
            stream_failed=engine.audio_stream_failed,
            quiesce_failed_stream=engine.quiesce_failed_audio_stream,
        )

    @contextmanager
    def hold_before_preflight(self, path):
        # Retain the actual Windows witness and writer while the main controller
        # proves that normal GlobalPlayback polling cannot launch its held gate.
        with self.outer.hold(path) as original_identity:
            self.prepare_blocked.set()
            if not self.prepare_release.wait(120):
                raise TimeoutError('actual preflight handoff hold timed out')
            yield original_identity

    def begin(self):
        self.press_timestamp = self.engine.capture_input_timestamp_ns()
        assert type(self.press_timestamp) is int and self.press_timestamp >= 0
        self.ui.recording.record_group(received_at_ns=self.press_timestamp)
        owner = self.controller._owner
        assert owner is not None and owner.gate is self.production_gate
        self.worker = owner.worker
        assert self.capture_count == 1 and self.original is not None
        assert self.group is None, 'the fixture OriginalGroupGate is not used'
        assert self.global_owner._deferred.capture is self.original
        assert self.global_owner._deferred.recording_gate is self.production_gate
        assert self.global_owner._pending is None
        self.ui.recording.record_mix()
        self.ui.recording.record_group(received_at_ns=self.press_timestamp)
        assert self.controller._owner.worker is self.worker
        assert self.capture_count == 1

    def poll_preflight_held(self):
        self.transport.on_frame_render()
        self.observe()
        if not self.prepare_blocked.is_set():
            return False
        resources = self.worker._resources
        assert resources is not None and resources.ticket is not None
        self.ticket = resources.ticket
        assert self.paths is not None
        assert self.worker.snapshot().ready is None
        assert not self.worker._preflight_retired.is_set()
        native = self.engine.mix_capture_status(self.ticket)
        assert native['writer_ready'] is True
        assert native['start_queued'] is native['started'] is False
        assert native['first_frame'] is native['end_frame'] is None
        assert self.global_owner._pending is None
        assert self.global_owner._deferred.capture is self.original
        assert self.global_owner._deferred.recording_gate is self.production_gate
        assert self.session.global_stop_restore_sample_ids == {0, 1}
        assert self.session.global_stop_engaged
        assert self.capture_count == 1
        assert identity(self.paths.spool_path) == self.outer.identity
        return True

    def release_preflight(self):
        self.prepare_release.set()

    def poll_group_queued(self):
        self.observe()
        pending = self.global_owner._pending
        if pending is None:
            return False
        assert pending.capture is self.original
        assert pending.recording_gate is self.production_gate
        assert pending.ticket.publication_status() == 'pending'
        self.batch = pending.ticket
        assert self.global_owner._deferred is None
        assert self.production_gate._capture_ticket is self.ticket
        assert self.session.global_stop_restore_sample_ids == {0, 1}
        assert self.session.global_stop_engaged
        assert not self.controller.status().effective_recording
        assert self.capture_count == 1
        return True

    def observe_production_ack(self):
        assert self.batch is not None and self.batch.publication_status() == 'accepted'
        assert self.original.state() == 'applied'
        assert self.global_owner._pending.ticket is self.batch
        assert self.session.global_stop_restore_sample_ids == {0, 1}
        assert self.session.global_stop_engaged
        # Only the production GlobalPlayback observer consumes the actual ACK.
        self.transport.on_frame_render()
        assert self.global_owner._pending is None and self.global_owner._deferred is None
        assert self.session.global_stop_restore_sample_ids == set()
        assert self.session.global_stop_engaged is False
        assert self.capture_count == 1

    def stop(self):
        assert self.ui.recording.status.effective_recording
        self.ui.recording.record_mix()
        assert self.controller._owner.worker is self.worker
        native = self.engine.mix_capture_status(self.ticket)
        assert native['stop_queued'] is True
        assert native['stop_target_mode'] == 'next_admissible'
        assert native['stop_target'] is None

    def finish(self, expected_payload, first, end):
        assert self.global_owner._pending is None and self.global_owner._deferred is None
        assert self.batch.publication_status() == 'accepted'
        assert self.original.state() == 'applied'
        assert self.production_gate._capture_ticket is self.ticket
        return super().finish(expected_payload, first, end)

    def poll_failed_retired(self):
        self.controller.poll()
        self.transport.on_frame_render()
        status = self.ui.recording.status
        return status.phase == 'Failed' and status.retired and not status.busy

    def finish_rejected_original(self):
        status = self.ui.recording.status
        assert status.phase == 'Failed' and status.retired and not status.busy
        assert status.error is not None and status.last_completion is None
        assert self.capture_count == 1
        assert self.original.state() == 'rejected'
        assert self.production_gate._cancelled
        assert self.global_owner._deferred is None and self.global_owner._pending is None
        assert self.session.global_stop_restore_sample_ids == {0, 1}
        assert self.session.global_stop_engaged
        assert self.worker._done.is_set() and not self.worker._thread.is_alive()
        assert self.worker.join_if_done() and self.worker.snapshot().cleanup_complete
        assert self.worker._resources is None and self.worker._audio is None
        native = self.engine.mix_capture_status(self.ticket)
        assert native['session_id'] == self.ticket.session_id()
        assert native['failure'] is not None
        assert all(native[key] is True for key in (
            'terminal_decided', 'rt_sealed', 'rt_released', 'worker_closed', 'worker_joined',
        )), native
        assert native['started'] is native['successful_spool'] is False
        assert native['first_frame'] is native['end_frame'] is None
        assert native['submitted_frames'] == native['written_frames'] == 0
        assert self.paths.spool_path.stat().st_size == 256
        assert identity(self.paths.spool_path) == self.outer.identity
        assert not self.paths.pcm_path.exists()
        assert not self.paths.encoded_path.exists() and not self.paths.final_path.exists()
        assert self.neighbor.read_bytes() == self.neighbor_bytes
        assert identity(self.neighbor) == self.neighbor_identity
        with self.paths.spool_path.open('r+b') as output:
            first_byte = output.read(1)
            assert len(first_byte) == 1
            output.seek(0)
            assert output.write(first_byte) == 1
            output.flush()
            os.fsync(output.fileno())
        shifted = self.paths.directory.with_name(self.paths.directory.name + '.released')
        self.paths.directory.rename(shifted)
        shifted.rename(self.paths.directory)
        # A visible failure cannot admit a replacement until actual retirement
        # and explicit acknowledgement, through the real public facade.
        self.ui.recording.record_group(received_at_ns=self.press_timestamp)
        assert self.capture_count == 1
        assert self.ui.recording.acknowledge_failure()
        assert self.ui.recording.status.phase == 'Idle'
        assert self.controller._owner is None
        assert self.ui.recording.status.retired and not self.ui.recording.status.busy

    def abort_and_poll(self):
        self.prepare_release.set()
        return super().abort_and_poll()
"#;

struct ControllerPython {
    executable: String,
    path: Py<PyAny>,
}

impl ControllerPython {
    fn configure() -> Self {
        Python::initialize();
        let interpreter = PathBuf::from(
            std::env::var_os(CONTROLLER_PYTHON)
                .expect("controller integration needs an explicit uv-discovered python.exe"),
        );
        assert!(interpreter.is_absolute() && interpreter.is_file());
        let interpreter = interpreter.canonicalize().unwrap();
        assert_eq!(interpreter.file_name().unwrap(), "python.exe");
        assert_ne!(
            interpreter,
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        Python::attach(|py| {
            let sys = py.import("sys").unwrap();
            let executable = sys.getattr("executable").unwrap().extract().unwrap();
            let path = sys
                .getattr("path")
                .unwrap()
                .call_method0("copy")
                .unwrap()
                .unbind();
            sys.setattr("executable", interpreter.to_string_lossy().as_ref())
                .unwrap();
            let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .unwrap();
            sys.getattr("path")
                .unwrap()
                .call_method1(
                    "insert",
                    (0, repository.join("src").to_string_lossy().as_ref()),
                )
                .unwrap();
            Self { executable, path }
        })
    }
}

impl Drop for ControllerPython {
    fn drop(&mut self) {
        Python::attach(|py| {
            let result = py.import("sys").and_then(|sys| {
                sys.setattr("path", self.path.bind(py))?;
                sys.setattr("executable", &self.executable)
            });
            if let Err(error) = result {
                eprintln!("recording controller interpreter restoration failed: {error}");
            }
        });
    }
}

struct ControllerRig {
    fixture: Fixture,
    engine: Py<AudioEngine>,
    case: Py<PyAny>,
    root: PathBuf,
    python: Option<ControllerPython>,
}

fn controller_checked<T>(py: Python<'_>, stage: &str, result: PyResult<T>) -> T {
    result.unwrap_or_else(|error| {
        error.print(py);
        panic!("actual recording controller {stage}: {error}")
    })
}

impl ControllerRig {
    fn new(route: &str) -> Self {
        Self::with_probe(route, CONTROLLER_PROBE, "ControllerCase")
    }

    fn production_global() -> Self {
        Self::with_probe(
            "left",
            &format!("{CONTROLLER_PROBE}\n{PRODUCTION_GLOBAL_PROBE}"),
            "ProductionGlobalCase",
        )
    }

    fn with_probe(route: &str, source: &str, class_name: &str) -> Self {
        let python = ControllerPython::configure();
        let mut fixture = Fixture::new(false);
        fixture.render_to(321);
        if route != "left" {
            let captured = public_capture(&fixture);
            fixture.drain();
            let batch = fixture
                .control
                .engine
                .publish_captured_global_start(&captured)
                .unwrap()
                .unwrap();
            fixture.drain();
            assert_eq!(batch.publication_status(), "accepted");
            assert_eq!(captured.state(), "applied");
            assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
            let prefix = fixture.render(17);
            assert_eq!(prefix, independent_dry_sum(0, 17));
        }
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let parent = workspace.join("test-artifacts/recording-controller-20261020");
        fs::create_dir_all(&parent).unwrap();
        let root = tempfile::Builder::new()
            .prefix(&format!("native-{route}-"))
            .tempdir_in(parent)
            .unwrap()
            .keep();
        fs::write(root.join("OWNER.md"), b"Owned actual RecordingController/per-take Thread/selected WAV preflight/finalizer integration. Original PyO3 engine and Native tickets, synthetic two-pad PCM, real FIFO/scheduler/mixer/retirement/spool and owned codec children. No App/device/music/Human inputs. All files retained on success/failure; remove only after actual Native, child, outer-pin and take-thread retirement.\n").unwrap();
        let original_ownership = fixture.control.engine.input_runtime_ownership.clone();
        let engine = Python::attach(|py| {
            // Move the ACTUAL producer/ownership engine. The empty field left
            // in Fixture is never used for status, prepare, publish or ACK.
            let original =
                std::mem::replace(&mut fixture.control.engine, AudioEngine::new().unwrap());
            Py::new(py, original).unwrap()
        });
        let case = Python::attach(|py| {
            assert!(Arc::ptr_eq(
                &engine.borrow(py).input_runtime_ownership,
                &original_ownership
            ));
            let probe = controller_checked(
                py,
                "load integration probe",
                PyModule::from_code(
                    py,
                    &CString::new(source).unwrap(),
                    c"actual_recording_controller_probe.py",
                    c"actual_recording_controller_probe",
                ),
            );
            controller_checked(
                py,
                "construct controller",
                probe.getattr(class_name).unwrap().call1((
                    engine.bind(py),
                    root.to_str().unwrap(),
                    route,
                )),
            )
            .unbind()
        });
        Self {
            fixture,
            engine,
            case,
            root,
            python: Some(python),
        }
    }

    fn invoke(&self, method: &str) {
        Python::attach(|py| {
            controller_checked(py, method, self.case.bind(py).call_method0(method));
        });
    }

    fn callback(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * self.fixture.channels];
        fixed_capture_callback(&mut self.fixture, &mut output);
        output
    }

    fn wait(&mut self, method: &str) {
        let deadline = Instant::now() + CONTROLLER_WAIT;
        loop {
            self.callback(0);
            let ready = Python::attach(|py| {
                controller_checked(py, method, self.case.bind(py).call_method0(method))
                    .extract::<bool>()
                    .unwrap()
            });
            if ready {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{method} timed out; retained {}",
                self.root.display()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn ticket(&self) -> Py<MixCaptureTicket> {
        Python::attach(|py| {
            self.case
                .bind(py)
                .getattr("ticket")
                .unwrap()
                .extract()
                .unwrap()
        })
    }

    fn original_group(&self) -> Py<CapturedGlobalStart> {
        Python::attach(|py| {
            self.case
                .bind(py)
                .getattr("group")
                .unwrap()
                .getattr("original")
                .unwrap()
                .extract()
                .unwrap()
        })
    }

    fn finish(&self, expected: &[f32], first: u64, end: u64) {
        let bytes = expected
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        let report = Python::attach(|py| {
            controller_checked(
                py,
                "verify full controller WAV",
                self.case
                    .bind(py)
                    .call_method1("finish", (PyBytes::new(py, &bytes), first, end)),
            )
            .extract::<String>()
            .unwrap()
        });
        println!("RECORDING_CONTROLLER_NATIVE {report}");
    }
}

impl Drop for ControllerRig {
    fn drop(&mut self) {
        let deadline = Instant::now() + CONTROLLER_WAIT;
        loop {
            self.callback(0);
            let settled = Python::attach(|py| {
                self.case
                    .bind(py)
                    .call_method0("abort_and_poll")
                    .and_then(|value| value.extract::<bool>())
            });
            if matches!(settled, Ok(true)) {
                break;
            }
            if Instant::now() >= deadline {
                eprintln!(
                    "unsettled recording controller retained: {}; result={settled:?}",
                    self.root.display()
                );
                // Do not change the command interpreter under a surviving real
                // worker/owned child. This isolated failed process retains its
                // configuration and evidence; no completed retirement is claimed.
                if let Some(python) = self.python.take() {
                    std::mem::forget(python);
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// Literal source formula, independent of DUT buffers, voices, phase and spool.
/// Neutral isolator gains are sample-exact; the complete dry prefix is compared.
fn independent_dry_sum(offset: usize, frames: usize) -> Vec<f32> {
    (offset..offset + frames)
        .map(|frame| {
            let source_frame = LOOP_START + frame % (LOOP_END - LOOP_START);
            [160.0_f64, 320.0]
                .into_iter()
                .map(|hz| {
                    (0.25
                        * (source_frame as f64 * hz * std::f64::consts::TAU / f64::from(RATE))
                            .sin()) as f32
                })
                .sum::<f32>()
        })
        .collect()
}

fn run_controller_route(route: &str, frames: usize) {
    let mut rig = ControllerRig::new(route);
    rig.invoke("begin");
    let original = if route == "left" {
        let original = rig.original_group();
        assert_eq!(
            Python::attach(|py| original.borrow(py).original_frame()),
            None
        );
        rig.callback(0); // original press target, before codec/worker readiness
        assert_eq!(
            Python::attach(|py| original.borrow(py).original_frame()),
            Some(321)
        );
        assert!(rig.callback(31).iter().all(|sample| *sample == 0.0));
        Some(original)
    } else {
        None
    };
    let first = rig.fixture.transport.output_frame();
    assert_eq!(first, if route == "left" { 352 } else { 338 });
    rig.wait("poll_recording");
    let ticket = rig.ticket();
    let started = Python::attach(|py| status(&rig.engine.borrow(py), &ticket.borrow(py)));
    assert!(started.started && !started.rt_sealed);
    assert_eq!(started.first_frame, Some(first));
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
    if let Some(original) = &original {
        Python::attach(|py| {
            let captured = original.borrow(py);
            assert_eq!(captured.original_frame(), Some(321));
            assert_eq!(captured.state(), "applied");
            for member in &captured.owner.members {
                assert_eq!(member.start.original_frame(), Some(321));
                assert_eq!(member.start.effective_frame(), Some(first));
            }
        });
    }
    let offset = if route == "left" { 0 } else { 17 };
    let expected = independent_dry_sum(offset, frames);
    let mut actual = Vec::with_capacity(frames);
    let mut remaining = frames;
    for requested in [1, 29, 511, 1024, 512] {
        let count = remaining.min(requested);
        actual.extend(rig.callback(count));
        remaining -= count;
        if remaining == 0 {
            break;
        }
    }
    assert_eq!(remaining, 0);
    assert_eq!(
        actual, expected,
        "complete independent two-pad dry PCM, no crop"
    );
    assert!(actual.iter().any(|sample| sample.abs() > 0.01));
    let end = first + frames as u64;
    assert_eq!(rig.fixture.transport.output_frame(), end);
    rig.invoke(if route == "shutdown" {
        "request_shutdown"
    } else {
        "stop"
    });
    rig.callback(0); // original NextAdmissible stop resolves at this callback F
    if route == "right" {
        rig.wait("poll_outer_blocked");
        rig.invoke("check_outer_busy_and_release");
    }
    rig.wait("poll_idle");
    let closed = Python::attach(|py| status(&rig.engine.borrow(py), &ticket.borrow(py)));
    assert!(closed.successful_spool && closed.worker_joined && closed.rt_released);
    assert_eq!(closed.failure, None);
    assert_eq!(
        (closed.first_frame, closed.end_frame),
        (Some(first), Some(end))
    );
    assert_eq!(
        (closed.submitted_frames, closed.written_frames),
        (frames as u64, frames as u64)
    );
    assert_eq!(
        (rig.fixture.starts(0), rig.fixture.starts(1)),
        (1, 1),
        "recording never retriggers playback"
    );
    rig.finish(&expected, first, end);
}

#[test]
#[ignore = "actual controller/codec integration; isolated FLITZIS_RECORDING_FORMAT_PYTHON host"]
fn real_recording_controller_right_next_start_stop_waits_outer_release_and_thread_join() {
    run_controller_route("right", 2077);
}

#[test]
#[ignore = "actual controller/atomic group/codec integration; isolated FLITZIS_RECORDING_FORMAT_PYTHON host"]
fn real_recording_controller_left_original_press_and_two_pad_common_e_publish_exact_wav() {
    run_controller_route("left", 2077);
}

#[test]
#[ignore = "actual controller shutdown/codec integration; isolated FLITZIS_RECORDING_FORMAT_PYTHON host"]
fn real_recording_controller_shutdown_finishes_original_take_before_audio_owner_release() {
    run_controller_route("shutdown", 31);
}

fn production_original(rig: &ControllerRig) -> Py<CapturedGlobalStart> {
    Python::attach(|py| {
        rig.case
            .bind(py)
            .getattr("original")
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn original_press_timestamp(rig: &ControllerRig) -> u64 {
    Python::attach(|py| {
        rig.case
            .bind(py)
            .getattr("press_timestamp")
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn hold_production_original(rig: &mut ControllerRig) -> Py<CapturedGlobalStart> {
    rig.invoke("begin");
    let original = production_original(rig);
    let timestamp = original_press_timestamp(rig);
    Python::attach(|py| {
        let captured = original.borrow(py);
        assert_eq!(captured.received_timestamp(), timestamp);
        assert_eq!(captured.original_frame(), None);
        assert_eq!(captured.owner.members.len(), 2);
    });
    assert_eq!(rig.callback(0).len(), 0);
    Python::attach(|py| {
        assert_eq!(original.borrow(py).original_frame(), Some(321));
    });
    rig.wait("poll_preflight_held");
    assert!(rig.callback(31).iter().all(|sample| *sample == 0.0));
    rig.invoke("poll_preflight_held");
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
    assert_eq!(rig.fixture.transport.output_frame(), 352);
    original
}

#[test]
#[ignore = "production UiContext/Transport/GlobalPlayback and codec integration; isolated FLITZIS_RECORDING_FORMAT_PYTHON host"]
fn real_production_global_recording_gate_retains_press_and_observes_actual_ack_before_restore_clear()
 {
    let mut rig = ControllerRig::production_global();
    let original = hold_production_original(&mut rig);
    let timestamp = original_press_timestamp(&rig);
    rig.invoke("release_preflight");
    rig.wait("poll_group_queued");
    // Publication is still requested here. The next actual callback owns the
    // same final group/capture claim; normal controller polling cannot fake it.
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
    let ticket = rig.ticket();
    Python::attach(|py| {
        let snapshot = status(&rig.engine.borrow(py), &ticket.borrow(py));
        assert!(snapshot.start_queued && !snapshot.started);
        assert_eq!(original.borrow(py).original_frame(), Some(321));
    });
    rig.wait("poll_recording");
    let first = rig.fixture.transport.output_frame();
    assert_eq!(first, 352);
    Python::attach(|py| {
        let captured = original.borrow(py);
        assert_eq!(captured.received_timestamp(), timestamp);
        assert_eq!(captured.original_frame(), Some(321));
        assert_eq!(captured.state(), "applied");
        for member in &captured.owner.members {
            assert_eq!(member.start.original_frame(), Some(321));
            assert_eq!(member.start.effective_frame(), Some(first));
        }
        assert_eq!(
            status(&rig.engine.borrow(py), &ticket.borrow(py)).first_frame,
            Some(first)
        );
    });
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
    rig.invoke("observe_production_ack");

    let frames = 2077;
    let expected = independent_dry_sum(0, frames);
    let mut actual = Vec::with_capacity(frames);
    for count in [1, 29, 511, 1024, 512] {
        actual.extend(rig.callback(count));
    }
    assert_eq!(actual.len(), frames);
    assert_eq!(
        actual, expected,
        "entire independently specified production-gate dry prefix"
    );
    assert!(actual.iter().any(|sample| sample.abs() > 0.01));
    let end = first + frames as u64;
    rig.invoke("stop");
    rig.callback(0);
    rig.wait("poll_idle");
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
    rig.finish(&expected, first, end);
}

#[test]
#[ignore = "production original group staleness and real worker retirement; isolated FLITZIS_RECORDING_FORMAT_PYTHON host"]
fn real_production_global_recording_gate_rejects_source_change_during_preflight_without_recapture()
{
    let mut rig = ControllerRig::production_global();
    let original = hold_production_original(&mut rig);
    let timestamp = original_press_timestamp(&rig);
    let ticket = rig.ticket();
    // Actual public source removal invalidates one original member. The group
    // must not recapture current sources or start its other member as a prefix.
    Python::attach(|py| rig.engine.borrow_mut(py).unload_sample(1).unwrap());
    rig.callback(0);
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
    assert!(rig.callback(17).iter().all(|sample| *sample == 0.0));
    rig.invoke("release_preflight");
    rig.wait("poll_failed_retired");
    Python::attach(|py| {
        let captured = original.borrow(py);
        assert_eq!(captured.received_timestamp(), timestamp);
        assert_eq!(captured.original_frame(), Some(321));
        assert_eq!(captured.state(), "rejected");
        for member in &captured.owner.members {
            assert_eq!(member.start.effective_frame(), None);
        }
        let snapshot = status(&rig.engine.borrow(py), &ticket.borrow(py));
        assert!(!snapshot.started && !snapshot.successful_spool);
        assert!(snapshot.rt_released && snapshot.worker_closed && snapshot.worker_joined);
        assert_eq!((snapshot.first_frame, snapshot.end_frame), (None, None));
        assert_eq!((snapshot.submitted_frames, snapshot.written_frames), (0, 0));
    });
    assert!(rig.callback(19).iter().all(|sample| *sample == 0.0));
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
    rig.invoke("finish_rejected_original");
}
