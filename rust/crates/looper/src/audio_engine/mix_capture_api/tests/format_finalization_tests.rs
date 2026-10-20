//! Real Native control/writer -> Python selected-codec finalization.
//!
//! Manual CaptureRt extents exercise the original publication and exclusive
//! writer, not CPAL, callback scheduling, realtime allocation or hearing. Run
//! this ignored case alone: embedded Python's command interpreter is configured
//! explicitly and restored before exit. All generated evidence is retained.

use super::*;
use pyo3::types::{PyBytes, PyModule};
use std::ffi::CString;

#[path = "format_finalization_tests/rf64_tests.rs"]
mod rf64_tests;

const FORMAT_FRAMES: usize = 10_001;
const FORMAT_FIRST: u64 = 731;
const FORMAT_PYTHON: &str = "FLITZIS_RECORDING_FORMAT_PYTHON";

const PYTHON_PROBE: &str = r#"
import hashlib
import json
import math
import os
import struct
from contextlib import contextmanager
from pathlib import Path
from threading import Event
from unittest.mock import patch

from flitzis_looper import recording_paths
from flitzis_looper.recording_codec import discover_recording_codecs, preflight_recording_codec
from flitzis_looper.recording_finalize import RecordingFinalizationCommittedError, finalize_recording_take
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_paths import RecordingPublicationCommittedError, create_recording_namespace
from flitzis_looper.recording_probe import validate_recording_streams, verify_mp3_packets
from flitzis_looper.recording_wav import inspect_recording_wav
from flitzis_looper_audio import inspect_mix_capture_spool


def identity(path):
    state = path.stat()
    assert state.st_ino != 0
    return state.st_dev, state.st_ino


class PreparedTake:
    def __init__(self, root, selected, rate, channels):
        root = Path(root)
        options = RecordingOptions(format=selected)
        self.paths = create_recording_namespace(root / 'recordings', options)
        self.neighbor = self.paths.root / 'unknown-neighbor.keep'
        with self.neighbor.open('xb') as output:
            output.write(b'unknown neighbor: preserve exact bytes and FileID\n')
        self.neighbor_bytes = self.neighbor.read_bytes()
        self.neighbor_identity = identity(self.neighbor)
        self.cancel = Event()
        self.retired = Event()
        ready_retired = Event()
        ready_directory = self.paths.directory / 'actual-codec-readiness'
        ready_directory.mkdir()
        self.ready = preflight_recording_codec(
            discover_recording_codecs(), options, rate=rate, channels=channels,
            directory=ready_directory, cancel=self.cancel, retired=ready_retired,
        )
        assert ready_retired.is_set()
        assert self.ready.options == self.paths.options == options
        assert (self.ready.rate, self.ready.channels) == (rate, channels)
        assert self.ready.proof.decoded_frames == 10001
        assert not self.paths.final_path.exists()
        assert not self.paths.spool_path.exists()

    def spool_path(self):
        return str(self.paths.spool_path)

    def capture_writer_identity(self):
        # Called only after actual writer_ready and before manual RT start.
        with self.paths.spool_path.open('rb') as source:
            state = os.fstat(source.fileno())
            self.spool_identity = state.st_dev, state.st_ino
        assert self.spool_identity[1] != 0

    def finish(self, engine, ticket, source_bytes, first, end, *, expect_committed_error=False):
        # Independent hand-authored f32 and signed PCM24 values. Eight frames
        # repeat 1250 times and the final frame retains both signed zero bits.
        source_pattern = bytes.fromhex(
            '00000080 00000000 0000803e 000080be 0000003f 000000bf '
            '0000803f 000080bf 0000a03f 0000c0bf 00000040 000000c0 '
            '00008033 000080b3 00004034 000040b4'
        )
        expected_source = source_pattern * 1250 + source_pattern[:8]
        assert source_bytes == expected_source
        expected_source_sha256 = hashlib.sha256(expected_source).hexdigest()
        integer_pattern = bytes.fromhex(
            '000000 000000 000020 0000e0 000040 0000c0 '
            'ffff7f 000080 ffff7f 000080 ffff7f 000080 '
            '000000 000000 020000 feffff'
        )
        expected_integer = integer_pattern * 1250 + integer_pattern[:6]
        before = engine.mix_capture_status(ticket)
        assert before['session_id'] == ticket.session_id()
        assert before['spool_path'] == str(self.paths.spool_path)
        assert all(before[name] is True for name in (
            'started', 'rt_sealed', 'rt_released', 'worker_closed',
            'worker_joined', 'terminal_decided', 'successful_spool',
        )), before
        # Actual Native readiness is active preparation, not a sticky terminal ACK.
        assert before['writer_ready'] is False
        assert before['failure'] is before['failure_code'] is before['error_detail'] is None
        assert before['first_frame'] == first
        assert before['end_frame'] == end
        assert before['submitted_frames'] == before['written_frames'] == 10001
        assert before['buffered_frames'] == 0
        assert before['payload_sha256'] == expected_source_sha256
        assert identity(self.paths.spool_path) == self.spool_identity
        assert self.paths.spool_path.read_bytes()[256:] == expected_source

        try:
            completion = finalize_recording_take(
                engine, ticket, self.paths, self.ready, spool_identity=self.spool_identity,
                cancel=self.cancel, retired=self.retired,
            )
        except RecordingFinalizationCommittedError as error:
            if not expect_committed_error:
                raise
            assert error.retirement_uncertain is True
            # Keep the actual typed exception and its traceback while testing
            # that real Native/file guards have still released below.
            self.committed_exception = error
            completion = error.completion
        else:
            assert not expect_committed_error, 'post-link custody failure was not reported'
        assert self.retired.is_set() is (not expect_committed_error)
        assert not self.cancel.is_set()
        extent = completion.extent
        assert (extent.first_frame, extent.end_frame, extent.frames) == (first, end, 10001)
        assert (extent.rate, extent.channels, extent.session_id) == (
            self.ready.rate, self.ready.channels, ticket.session_id(),
        )
        assert extent.payload_sha256 == expected_source_sha256
        assert completion.path == self.paths.final_path
        assert completion.codec.decoded_frames == 10001
        assert identity(completion.path) == identity(self.paths.encoded_path)
        assert identity(completion.path) == completion.codec.output_identity
        encoded = completion.path.read_bytes()
        assert len(encoded) == completion.codec.output_bytes
        assert hashlib.sha256(encoded).hexdigest() == completion.codec.output_sha256
        assert identity(self.paths.spool_path) == self.spool_identity
        assert self.paths.spool_path.read_bytes()[256:] == expected_source
        assert self.neighbor.read_bytes() == self.neighbor_bytes
        assert identity(self.neighbor) == self.neighbor_identity
        assert completion.pcm.frames == 10001
        assert completion.pcm.channels == 2
        assert completion.pcm.peak_abs == 2.0
        assert completion.pcm.source_overrange_samples == 5000
        assert completion.pcm.source_overrange_frames == 2500
        assert completion.pcm.source_payload_sha256 == expected_source_sha256

        selected = self.paths.options.format
        if selected == 'wav':
            with completion.path.open('rb') as source:
                geometry = inspect_recording_wav(source)
                assert (geometry.encoding, geometry.rate, geometry.channels, geometry.frames) == (
                    'float32', self.ready.rate, 2, 10001,
                )
                source.seek(geometry.data_offset)
                assert source.read(geometry.data_bytes) == expected_source
            assert completion.pcm.output_payload_sha256 == hashlib.sha256(expected_source).hexdigest()
            assert completion.codec.decoded_payload_sha256 == completion.pcm.output_payload_sha256
        elif selected == 'flac':
            assert self.paths.decoded_path.read_bytes() == expected_integer
            assert completion.pcm.output_payload_sha256 == hashlib.sha256(expected_integer).hexdigest()
            assert completion.codec.decoded_payload_sha256 == completion.pcm.output_payload_sha256
            assert completion.warning is not None
        else:
            decoded = self.paths.decoded_path.read_bytes()
            assert len(decoded) == 10001 * 2 * 4
            assert all(math.isfinite(value) for (value,) in struct.iter_unpack('<f', decoded))
            assert completion.codec.decoded_payload_sha256 == hashlib.sha256(decoded).hexdigest()
            assert completion.pcm.output_payload_sha256 == hashlib.sha256(expected_integer).hexdigest()
            assert completion.warning is not None
            metadata = json.loads(self.paths.encoded_path.with_name(
                self.paths.encoded_path.name + '.streams.json').read_bytes())
            info = validate_recording_streams(
                metadata, self.paths.options, rate=self.ready.rate, channels=2, frames=10001,
            )
            packets_path = self.paths.encoded_path.with_name(self.paths.encoded_path.name + '.packets')
            with packets_path.open('rb') as packets:
                trim = verify_mp3_packets(packets, info, frames=10001, bitrate_kbps=320)
            assert trim.skip_samples == completion.codec.mp3_skip_samples > 0
            assert trim.discard_padding == completion.codec.mp3_discard_padding >= 0
            assert trim.encoded_frames - trim.skip_samples - trim.discard_padding == 10001
        after = engine.mix_capture_status(ticket)
        assert after == before, (before, after)
        return json.dumps({
            'format': selected, 'session_id': extent.session_id,
            'first_frame': extent.first_frame, 'exclusive_end_frame': extent.end_frame,
            'frames': extent.frames, 'rate': extent.rate, 'channels': extent.channels,
            'source_overrange_samples': completion.pcm.source_overrange_samples,
            'source_overrange_frames': completion.pcm.source_overrange_frames,
            'writer_payload_sha256': extent.payload_sha256,
            'output_identity': completion.codec.output_identity,
            'output_sha256': completion.codec.output_sha256,
            'decoded_frames': completion.codec.decoded_frames,
            'mp3_skip_samples': completion.codec.mp3_skip_samples,
            'mp3_discard_padding': completion.codec.mp3_discard_padding,
            'unknown_neighbor_preserved': True,
            'retired': self.retired.is_set(),
            'retirement_uncertain': expect_committed_error,
        }, sort_keys=True)

    def finish_without_postlink_reads(self, engine, ticket, source_bytes, first, end):
        original = recording_paths._plain_metadata
        postlink_reads = []

        def fail_only_after_publication(path, *, directory):
            if path == self.paths.final_path and path.exists():
                postlink_reads.append(str(path))
                raise OSError('injected final metadata read after successful real link')
            return original(path, directory=directory)

        # The ordinary actual finalizer must return, with every normal payload,
        # identity, source-witness and retirement assertion in finish preserved.
        with patch.object(recording_paths, '_plain_metadata', fail_only_after_publication):
            result = self.finish(engine, ticket, source_bytes, first, end)
        assert postlink_reads == [], postlink_reads
        evidence = json.loads(result)
        evidence['publication_boundary_case'] = 'no_fallible_postlink_metadata_read'
        evidence['postlink_metadata_hook_calls'] = 0
        return json.dumps(evidence, sort_keys=True)

    def finish_with_committed_release_error(self, engine, ticket, source_bytes, first, end):
        original = recording_paths._windows_pin
        actual_releases = []

        @contextmanager
        def fail_after_actual_source_release(path, *, directory):
            # Release the genuine original Windows handle before injecting the
            # report. The exception represents conservative custody uncertainty.
            with original(path, directory=directory) as descriptor:
                yield descriptor
            if path == self.paths.encoded_path and self.paths.final_path.exists():
                actual_releases.append(str(path))
                raise OSError('injected report after real encoded pin release and final link')

        with patch.object(recording_paths, '_windows_pin', fail_after_actual_source_release):
            result = self.finish(
                engine, ticket, source_bytes, first, end, expect_committed_error=True,
            )
        assert actual_releases == [str(self.paths.encoded_path)], actual_releases
        retained = self.committed_exception
        assert isinstance(retained, RecordingFinalizationCommittedError)
        assert retained.retirement_uncertain is True
        assert retained.completion.path == self.paths.final_path
        assert retained.completion.extent.frames == 10001
        assert identity(self.paths.final_path) == retained.completion.codec.output_identity
        publication = retained.__cause__
        assert isinstance(publication, RecordingPublicationCommittedError)
        assert publication.published_path == self.paths.final_path
        assert publication.output_identity == retained.completion.codec.output_identity
        assert publication.retirement_uncertain is True
        assert not self.retired.is_set()

        # Read-only reopening alone could coexist with the Native deny-write
        # pins. An unchanged-byte write proves actual release even while the
        # typed exception/tracebacks stay alive. Every leaf is this owned take.
        for path in (
            self.paths.spool_path, self.paths.pcm_path,
            self.paths.encoded_path, self.paths.final_path,
        ):
            with path.open('r+b') as source:
                first_byte = source.read(1)
                assert len(first_byte) == 1
                source.seek(0)
                assert source.write(first_byte) == 1
                source.flush()
                os.fsync(source.fileno())
        assert hashlib.sha256(self.paths.final_path.read_bytes()).hexdigest() == retained.completion.codec.output_sha256
        assert identity(self.paths.final_path) == retained.completion.codec.output_identity
        assert identity(self.paths.spool_path) == self.spool_identity
        assert self.paths.spool_path.read_bytes()[256:] == source_bytes
        assert engine.mix_capture_status(ticket)['payload_sha256'] == hashlib.sha256(source_bytes).hexdigest()
        assert self.neighbor.read_bytes() == self.neighbor_bytes
        assert identity(self.neighbor) == self.neighbor_identity
        assert not self.retired.is_set()
        evidence = json.loads(result)
        evidence['publication_boundary_case'] = 'typed_committed_custody_release_error'
        evidence['actual_encoded_pin_releases_before_error'] = len(actual_releases)
        evidence['retained_exception_has_verified_completion'] = True
        evidence['original_handles_released_despite_retained_traceback'] = True
        return json.dumps(evidence, sort_keys=True)

    def reject_finite_payload_mutation(self, engine, ticket, source_bytes, first, end):
        before = engine.mix_capture_status(ticket)
        assert before['session_id'] == ticket.session_id()
        assert before['spool_path'] == str(self.paths.spool_path)
        assert all(before[name] is True for name in (
            'started', 'rt_sealed', 'rt_released', 'worker_closed',
            'worker_joined', 'terminal_decided', 'successful_spool',
        )), before
        assert before['writer_ready'] is False
        assert before['failure'] is before['failure_code'] is before['error_detail'] is None
        assert (before['first_frame'], before['end_frame']) == (first, end)
        assert before['submitted_frames'] == before['written_frames'] == 10001
        assert before['buffered_frames'] == 0
        assert before['payload_sha256'] == hashlib.sha256(source_bytes).hexdigest()
        original = self.paths.spool_path.read_bytes()
        assert original[256:] == source_bytes
        assert identity(self.paths.spool_path) == self.spool_identity
        checkpoint_before = inspect_mix_capture_spool(str(self.paths.spool_path))
        replacement = struct.pack('<f', 0.75)
        assert original[256:260] != replacement
        # The original writer has really closed and joined. This is a finite,
        # same-file in-place change, without touching its valid checkpoint.
        with self.paths.spool_path.open('r+b') as source:
            state = os.fstat(source.fileno())
            assert (state.st_dev, state.st_ino) == self.spool_identity
            source.seek(256)
            assert source.write(replacement) == 4
            source.flush()
            os.fsync(source.fileno())
        mutated = self.paths.spool_path.read_bytes()
        assert mutated == original[:256] + replacement + original[260:]
        assert identity(self.paths.spool_path) == self.spool_identity
        assert inspect_mix_capture_spool(str(self.paths.spool_path)) == checkpoint_before
        assert engine.mix_capture_status(ticket) == before
        mutated_digest = hashlib.sha256(mutated[256:]).hexdigest()
        assert mutated_digest != before['payload_sha256']
        assert all(math.isfinite(value) for (value,) in struct.iter_unpack('<f', mutated[256:]))
        assert not self.retired.is_set()
        try:
            finalize_recording_take(
                engine, ticket, self.paths, self.ready, spool_identity=self.spool_identity,
                cancel=self.cancel, retired=self.retired,
            )
        except ValueError as error:
            detail = str(error).lower()
            assert 'writer' in detail and ('payload' in detail or 'digest' in detail), detail
        else:
            raise AssertionError('same-FileID finite payload mutation was published')
        assert self.retired.is_set()
        assert not self.cancel.is_set()
        assert not self.paths.encoded_path.exists()
        assert not self.paths.decoded_path.exists()
        assert not self.paths.final_path.exists()
        assert not (self.paths.directory / 'verified.json').exists()
        assert engine.mix_capture_status(ticket) == before
        assert before['payload_sha256'] == hashlib.sha256(source_bytes).hexdigest()
        assert inspect_mix_capture_spool(str(self.paths.spool_path)) == checkpoint_before
        assert identity(self.paths.spool_path) == self.spool_identity
        assert self.paths.spool_path.read_bytes() == mutated
        assert self.neighbor.read_bytes() == self.neighbor_bytes
        assert identity(self.neighbor) == self.neighbor_identity
        return json.dumps({
            'format': 'wav', 'rejection': 'finite_same_file_payload_digest_mismatch',
            'session_id': ticket.session_id(), 'first_frame': first,
            'exclusive_end_frame': end, 'frames': 10001,
            'original_writer_payload_sha256': before['payload_sha256'],
            'mutated_payload_sha256': mutated_digest,
            'spool_identity_unchanged': True, 'checkpoint_unchanged': True,
            'native_status_unchanged': True, 'unknown_neighbor_preserved': True,
            'retired': True, 'encoded_or_final_created': False,
        }, sort_keys=True)
"#;

struct FormatPython {
    executable: String,
}

impl FormatPython {
    fn configure() -> Self {
        Python::initialize();
        let interpreter = PathBuf::from(
            std::env::var_os(FORMAT_PYTHON)
                .expect("isolated format test requires an explicit uv-discovered python.exe"),
        );
        assert!(interpreter.is_absolute() && interpreter.is_file());
        let interpreter = interpreter.canonicalize().unwrap();
        assert_eq!(interpreter.file_name().unwrap(), "python.exe");
        assert_ne!(
            interpreter,
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        let executable = Python::attach(|py| {
            let sys = py.import("sys").unwrap();
            let original = sys
                .getattr("executable")
                .unwrap()
                .extract::<String>()
                .unwrap();
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
            original
        });
        Self { executable }
    }
}

impl Drop for FormatPython {
    fn drop(&mut self) {
        Python::attach(|py| {
            if let Err(error) = py
                .import("sys")
                .and_then(|sys| sys.setattr("executable", &self.executable))
            {
                eprintln!("format fixture interpreter restoration failed: {error}");
            }
        });
    }
}

struct FormatRig {
    engine: Py<AudioEngine>,
    consumer: Consumer<ControlMessage>,
    root: PathBuf,
}

impl FormatRig {
    fn new() -> Self {
        let mut engine = AudioEngine::new().unwrap();
        let (producer, consumer) = RingBuffer::new(2);
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
        let parent = workspace.join("test-artifacts/recording-format-20261020");
        fs::create_dir_all(&parent).unwrap();
        let root = tempfile::Builder::new()
            .prefix("native-finalizer-")
            .tempdir_in(parent)
            .unwrap()
            .keep();
        fs::write(root.join("OWNER.md"), b"Owned synthetic Native control/writer -> actual Python codec finalizer test. No device, callback scheduling, models or Human inputs. All generated files stay retained, including after panic; never remove while RT or writer/process retirement is incomplete.\n").unwrap();
        let engine = Python::attach(|py| Py::new(py, engine).unwrap());
        Self {
            engine,
            consumer,
            root,
        }
    }

    fn prepare(&self, path: &str) -> Py<MixCaptureTicket> {
        let ticket = Python::attach(|py| {
            self.engine
                .borrow(py)
                .prepare_mix_capture_internal(path, 1024, 16, 131_072)
                .unwrap()
        });
        let ready = wait_for(
            &ticket.observer,
            "actual finalizer spool writer ready",
            |status| status.writer_ready,
        );
        assert_eq!(
            (ready.config.sample_rate_hz, ready.config.channels),
            (RATE, CHANNELS)
        );
        Python::attach(|py| Py::new(py, ticket).unwrap())
    }

    fn manual_capture(&mut self, ticket: &Py<MixCaptureTicket>, pcm: &[f32]) -> CaptureStatus {
        let end = FORMAT_FIRST + FORMAT_FRAMES as u64;
        let id = Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let ticket = ticket.borrow(py);
            assert!(
                engine
                    .start_mix_capture_internal(&ticket, FORMAT_FIRST)
                    .unwrap()
            );
            ticket.session_id()
        });
        let ControlMessage::StartMixCapture {
            publication,
            target_frame,
        } = self.consumer.pop().unwrap()
        else {
            panic!("original Native capture start expected")
        };
        assert_eq!(
            (publication.session_id(), target_frame),
            (id, MixCaptureTarget::At(FORMAT_FIRST))
        );
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let owner = engine.mix_capture_owner.lock().unwrap();
            assert!(Arc::ptr_eq(
                &publication,
                &owner.as_ref().unwrap().publication
            ));
            drop(owner);
            assert!(
                engine
                    .stop_mix_capture_internal(&ticket.borrow(py), end)
                    .unwrap()
            );
        });
        let ControlMessage::StopMixCapture {
            session_id,
            target_frame,
        } = self.consumer.pop().unwrap()
        else {
            panic!("original Native capture exclusive stop expected")
        };
        assert_eq!((session_id, target_frame), (id, MixCaptureTarget::At(end)));
        assert!(self.consumer.is_empty());
        let mut rt = publication
            .claim()
            .expect("exact original prepared CaptureRt");
        assert!(publication.claim().is_none());
        assert!(rt.start(id, FORMAT_FIRST, RATE, CHANNELS));
        let mut first = 0;
        for frames in [1, 997, 2049, 13, 4096, 2845] {
            assert!(rt.append_segment(
                FORMAT_FIRST + first as u64,
                &pcm[first * CHANNELS..(first + frames) * CHANNELS]
            ));
            first += frames;
        }
        assert_eq!(first, FORMAT_FRAMES);
        assert!(rt.stop(id, end));
        assert!(rt.terminal());
        drop(rt); // Control-only fixture: actual heavy release occurs off realtime.
        let deadline = Instant::now() + WAIT;
        loop {
            let status = Python::attach(|py| {
                let engine = self.engine.borrow(py);
                let ticket = ticket.borrow(py);
                engine.mix_capture_status_internal(py, &ticket).unwrap();
                ticket.observer.status()
            });
            if status.rt_released && status.worker_closed && status.worker_joined {
                assert!(
                    status.terminal_decided && status.successful_spool(),
                    "{status:?}"
                );
                assert_eq!(
                    (status.first_frame, status.end_frame),
                    (Some(FORMAT_FIRST), Some(end))
                );
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "actual finalizer writer settlement timed out: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for FormatRig {
    fn drop(&mut self) {
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            engine.close_mix_capture_admission();
            while let Ok(message) = self.consumer.pop() {
                drop(message);
            }
            engine.poll_mix_capture_shutdown();
            let deadline = Instant::now() + WAIT;
            loop {
                let mut slot = engine
                    .mix_capture_owner
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let Some(capture) = slot.as_mut() else { break };
                let id = capture.control.status().session_id;
                capture
                    .control
                    .request_failure(id, CaptureFailure::Cancelled);
                drop(capture.publication.claim());
                capture.poll(false);
                if capture.settled() {
                    break;
                }
                if Instant::now() >= deadline {
                    eprintln!(
                        "unsettled Native format fixture retained: {}",
                        self.root.display()
                    );
                    break;
                }
                drop(slot);
                std::thread::sleep(Duration::from_millis(1));
            }
        });
    }
}

fn format_probe<'py>(py: Python<'py>) -> Bound<'py, PyModule> {
    python_checked(
        py,
        "load finalizer probe",
        PyModule::from_code(
            py,
            &CString::new(PYTHON_PROBE).unwrap(),
            c"native_capture_format_probe.py",
            c"native_capture_format_probe",
        ),
    )
}

fn python_checked<T>(py: Python<'_>, stage: &str, result: PyResult<T>) -> T {
    result.unwrap_or_else(|error| {
        error.print(py);
        panic!("Native format fixture {stage}: {error}")
    })
}

#[test]
fn actual_python_binding_omitted_target_keeps_next_request_separate_from_ack() {
    Python::initialize();
    let mut rig = FormatRig::new();
    let ticket = rig.prepare(rig.root.join("python-default.capture").to_str().unwrap());
    Python::attach(|py| {
        assert!(
            rig.engine
                .bind(py)
                .call_method1("start_mix_capture", (ticket.bind(py),))
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            rig.engine
                .bind(py)
                .call_method1("stop_mix_capture", (ticket.bind(py),))
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            rig.engine
                .bind(py)
                .call_method1("stop_mix_capture", (ticket.bind(py), py.None()))
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            !rig.engine
                .bind(py)
                .call_method1("stop_mix_capture", (ticket.bind(py), 0_u64))
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        let status = rig
            .engine
            .bind(py)
            .call_method1("mix_capture_status", (ticket.bind(py),))
            .unwrap();
        for prefix in ["start", "stop"] {
            assert_eq!(
                status
                    .get_item(format!("{prefix}_target_mode"))
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "next_admissible"
            );
            assert!(
                status
                    .get_item(format!("{prefix}_target"))
                    .unwrap()
                    .is_none()
            );
        }
        for field in ["first_frame", "end_frame"] {
            assert!(status.get_item(field).unwrap().is_none());
        }
    });
    let ControlMessage::StartMixCapture {
        publication,
        target_frame,
    } = rig.consumer.pop().unwrap()
    else {
        panic!("actual Python default must queue the original start")
    };
    assert_eq!(target_frame, MixCaptureTarget::NextAdmissible);
    let work = publication.claim().unwrap();
    let ControlMessage::StopMixCapture {
        session_id,
        target_frame,
    } = rig.consumer.pop().unwrap()
    else {
        panic!("actual Python default must queue one original stop")
    };
    assert_eq!(
        session_id,
        Python::attach(|py| ticket.borrow(py).session_id())
    );
    assert_eq!(target_frame, MixCaptureTarget::NextAdmissible);
    assert!(rig.consumer.is_empty());
    Python::attach(|py| {
        assert!(
            rig.engine
                .bind(py)
                .call_method1("cancel_mix_capture", (ticket.bind(py),))
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
    });
    drop(work); // Actual manual offRT release; no callback ACK is fabricated.
    let deadline = Instant::now() + WAIT;
    loop {
        let status = Python::attach(|py| {
            rig.engine
                .borrow(py)
                .mix_capture_status_internal(py, &ticket.borrow(py))
                .unwrap();
            ticket.borrow(py).observer.status()
        });
        if status.rt_released && status.worker_closed && status.worker_joined {
            assert_eq!(status.failure, Some(CaptureFailure::Cancelled));
            assert_eq!((status.first_frame, status.end_frame), (None, None));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "original Python default capture did not retire: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "actual codec child integration; isolated one-filter process with FLITZIS_RECORDING_FORMAT_PYTHON"]
fn actual_native_capture_writer_finalizes_wav_flac_and_mp3_with_original_ticket() {
    let _runtime = FormatPython::configure();
    let pattern = [
        -0.0_f32,
        0.0,
        0.25,
        -0.25,
        0.5,
        -0.5,
        1.0,
        -1.0,
        1.25,
        -1.5,
        2.0,
        -2.0,
        1.0 / 16_777_216.0,
        -1.0 / 16_777_216.0,
        3.0 / 16_777_216.0,
        -3.0 / 16_777_216.0,
    ];
    let pcm: Vec<f32> = (0..FORMAT_FRAMES * CHANNELS)
        .map(|index| pattern[index % pattern.len()])
        .collect();
    let source_bytes: Vec<u8> = pcm
        .iter()
        .flat_map(|sample| sample.to_bits().to_le_bytes())
        .collect();
    for (selected, operation) in [
        ("wav", "finish"),
        ("flac", "finish"),
        ("mp3", "finish"),
        ("wav", "reject_finite_payload_mutation"),
        ("wav", "finish_without_postlink_reads"),
        ("wav", "finish_with_committed_release_error"),
    ] {
        let mut rig = FormatRig::new();
        let take = Python::attach(|py| {
            let result = format_probe(py).getattr("PreparedTake").unwrap().call1((
                rig.root.to_string_lossy().as_ref(),
                selected,
                RATE,
                CHANNELS,
            ));
            python_checked(py, "actual selected-codec preparation", result).unbind()
        });
        let spool = Python::attach(|py| {
            take.call_method0(py, "spool_path")
                .unwrap()
                .extract::<String>(py)
                .unwrap()
        });
        let ticket = rig.prepare(&spool);
        Python::attach(|py| {
            python_checked(
                py,
                "original writer-ready FileID",
                take.call_method0(py, "capture_writer_identity"),
            );
        });
        let status = rig.manual_capture(&ticket, &pcm);
        assert_eq!(
            (status.submitted_frames, status.written_frames),
            (FORMAT_FRAMES as u64, FORMAT_FRAMES as u64)
        );
        let evidence = Python::attach(|py| {
            let result = take.call_method1(
                py,
                operation,
                (
                    rig.engine.bind(py),
                    ticket.bind(py),
                    PyBytes::new(py, &source_bytes),
                    FORMAT_FIRST,
                    FORMAT_FIRST + FORMAT_FRAMES as u64,
                ),
            );
            python_checked(py, "actual Native writer-to-format finalization", result)
                .extract::<String>(py)
                .unwrap()
        });
        fs::write(rig.root.join("evidence.json"), &evidence).unwrap();
        eprintln!(
            "actual Native capture finalizer {selected}/{operation}: {evidence}; retained {}",
            rig.root.display()
        );
    }
}
