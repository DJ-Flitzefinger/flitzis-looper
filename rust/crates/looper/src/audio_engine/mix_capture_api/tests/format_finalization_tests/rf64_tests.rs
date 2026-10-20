//! Explicit large-file software proof. Every payload byte traverses the original
//! Native writer and finalizer; no sparse extent, fabricated completion or device.
//!
//! Run this ignored test alone with the parent's configured interpreter. It keeps
//! about 12 GiB of owned output, so the runner must first resolve storage retention.
//! The two-hour deadline requests cancellation; actual hashing/I/O and thread/tree
//! retirement still have to return. A single Native hash has no cancellation hook,
//! although its existing binding releases the GIL. No hard abort/RAM/RT claim.

use super::*;
use crate::messages::MixCaptureTarget;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const RF64_FRAMES: u64 = 536_879_105;
const RF64_PAYLOAD_BYTES: u64 = 4_295_032_840;
const RF64_BLOCK_FRAMES: usize = 16_384;
const RF64_BLOCK_COUNT: usize = 8;
const RF64_POOL_BYTES: usize = 1_048_576;
const RF64_DEADLINE: Duration = Duration::from_secs(2 * 60 * 60);
const RF64_PATTERN: [f32; 16] = [
    -0.0,
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
// Hand-authored little-endian bytes, independent of the Native encoder and the
// f32::to_bits loop it uses. Each block's first frame receives a unique marker.
const RF64_GOLDEN: [u8; 64] = [
    0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3e, 0x00, 0x00, 0x80, 0xbe,
    0x00, 0x00, 0x00, 0x3f, 0x00, 0x00, 0x00, 0xbf, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x80, 0xbf,
    0x00, 0x00, 0xa0, 0x3f, 0x00, 0x00, 0xc0, 0xbf, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0xc0,
    0x00, 0x00, 0x80, 0x33, 0x00, 0x00, 0x80, 0xb3, 0x00, 0x00, 0x40, 0x34, 0x00, 0x00, 0x40, 0xb4,
];

const RF64_PYTHON: &str = r#"
import hashlib
import json
import os
import struct
import sys
import threading
import time
from threading import Event, Thread

from flitzis_looper.recording_finalize import finalize_recording_take
from flitzis_looper.recording_probe import validate_recording_streams
from flitzis_looper.recording_wav import inspect_recording_wav
from flitzis_looper_audio import inspect_mix_capture_spool
from tests.flitzis_looper.controller.stem_batch_neural_native_probe import _OwnedProcessObservation

FRAMES = 536879105
PAYLOAD_BYTES = 4295032840
BLOCK_FRAMES = 16384
BLOCK_BYTES = BLOCK_FRAMES * 8
PATTERN = bytes.fromhex(
    '00000080 00000000 0000803e 000080be 0000003f 000000bf '
    '0000803f 000080bf 0000a03f 0000c0bf 00000040 000000c0 '
    '00008033 000080b3 00004034 000040b4'
)


def file_identity(path):
    state = path.stat()
    assert state.st_ino != 0
    return state.st_dev, state.st_ino


def verification_checkpoint(cancel):
    if cancel.is_set():
        raise InterruptedError('RF64 test verification cancelled; existing output retained')


def verify_payload(source, cancel, *, require_eof):
    # Fixed 64 KiB reads and oracle storage; markers include the absolute block
    # ordinal, so same-length block duplication/reordering changes the oracle.
    remaining = PAYLOAD_BYTES
    position = 0
    digest = hashlib.sha256()
    while remaining:
        verification_checkpoint(cancel)
        count = min(65536, remaining)
        payload = source.read(count)
        assert len(payload) == count, (position, count, len(payload))
        expected = bytearray(PATTERN * (count // len(PATTERN)) + PATTERN[:count % len(PATTERN)])
        if position % BLOCK_BYTES == 0:
            marker = 0x3f000000 | (position // BLOCK_BYTES)
            expected[:8] = struct.pack('<II', marker, marker | 0x80000000)
        assert payload == expected, ('full uncropped payload mismatch', position)
        digest.update(payload)
        position += count
        remaining -= count
    if require_eof:
        assert source.read(1) == b''
    return digest.hexdigest()


def file_digest(path, cancel):
    digest = hashlib.sha256()
    with path.open('rb') as source:
        while payload := source.read(65536):
            verification_checkpoint(cancel)
            digest.update(payload)
    return digest.hexdigest()


def finish_streamed(take, engine, ticket, expected_hash, first, end):
    before = engine.mix_capture_status(ticket)
    assert before['session_id'] == ticket.session_id()
    assert before['spool_path'] == str(take.paths.spool_path)
    assert all(before[name] is True for name in (
        'started', 'rt_sealed', 'rt_released', 'worker_closed',
        'worker_joined', 'terminal_decided', 'successful_spool',
    )), before
    assert before['writer_ready'] is False
    assert before['failure'] is before['failure_code'] is before['error_detail'] is None
    assert before['first_frame'] == first
    assert before['captured_end'] == before['submitted_end'] == before['written_end'] == end
    assert before['end_frame'] == end
    assert before['submitted_frames'] == before['written_frames'] == FRAMES
    assert before['submitted_blocks'] == before['written_blocks'] == 32769
    assert before['buffered_frames'] == 0
    assert before['payload_sha256'] == expected_hash
    assert file_identity(take.paths.spool_path) == take.spool_identity
    assert take.paths.spool_path.stat().st_size == 256 + PAYLOAD_BYTES
    checkpoint_before = inspect_mix_capture_spool(str(take.paths.spool_path))
    assert checkpoint_before['state'] == 'sealed_candidate'
    assert checkpoint_before['declared_written_frames'] == FRAMES
    assert checkpoint_before['physical_whole_frames'] == FRAMES
    assert checkpoint_before['requires_verification'] is True
    assert checkpoint_before['publication_ready'] is False
    completion = finalize_recording_take(
        engine, ticket, take.paths, take.ready,
        spool_identity=take.spool_identity, cancel=take.cancel, retired=take.retired,
    )
    assert take.retired.is_set()
    assert not take.cancel.is_set()
    extent = completion.extent
    assert (extent.first_frame, extent.end_frame, extent.frames) == (first, end, FRAMES)
    assert (extent.rate, extent.channels, extent.session_id) == (
        take.ready.rate, 2, ticket.session_id(),
    )
    assert extent.payload_sha256 == expected_hash
    assert completion.path == take.paths.final_path
    assert completion.codec.decoded_frames == FRAMES
    assert completion.codec.output_identity == file_identity(take.paths.encoded_path)
    assert completion.codec.output_identity == file_identity(completion.path)
    assert completion.codec.output_bytes == completion.path.stat().st_size
    assert completion.codec.output_sha256 == file_digest(completion.path, take.cancel)
    assert completion.codec.decoded_payload_sha256 == expected_hash
    assert completion.pcm.frames == FRAMES
    assert completion.pcm.channels == 2
    assert completion.pcm.peak_abs == 2.0
    # The per-block marker replaces signed zero, not an overrange sample.
    assert completion.pcm.source_overrange_samples == (FRAMES // 8) * 4
    assert completion.pcm.source_overrange_frames == (FRAMES // 8) * 2
    assert completion.pcm.source_payload_sha256 == expected_hash
    assert completion.pcm.output_payload_sha256 == expected_hash
    assert completion.warning is not None
    assert not take.paths.decoded_path.exists(), 'WAV must not create a fourth payload copy'
    with take.paths.spool_path.open('rb') as source:
        state = os.fstat(source.fileno())
        assert (state.st_dev, state.st_ino) == take.spool_identity
        source.seek(256)
        assert verify_payload(source, take.cancel, require_eof=True) == expected_hash
    with take.paths.pcm_path.open('rb') as source:
        pcm_state = os.fstat(source.fileno())
        assert pcm_state.st_size == PAYLOAD_BYTES
        assert (pcm_state.st_dev, pcm_state.st_ino) != take.spool_identity
        assert verify_payload(source, take.cancel, require_eof=True) == expected_hash
    with completion.path.open('rb') as source:
        geometry = inspect_recording_wav(source)
        assert geometry.rf64 is True
        assert (geometry.encoding, geometry.rate, geometry.channels, geometry.frames) == (
            'float32', take.ready.rate, 2, FRAMES,
        )
        assert geometry.data_bytes == PAYLOAD_BYTES > 0xffffffff
        assert geometry.file_bytes == completion.codec.output_bytes
        # Independent exact ds64 fields supplement the shared container parser.
        header = source.read(48)
        marker, size32, form, chunk, length, riff64, data64, frames64, entries = struct.unpack(
            '<4sI4s4sIQQQI', header,
        )
        assert (marker, size32, form, chunk, length) == (
            b'RF64', 0xffffffff, b'WAVE', b'ds64', 28,
        )
        assert (riff64 + 8, data64, frames64, entries) == (
            geometry.file_bytes, PAYLOAD_BYTES, FRAMES, 0,
        )
        source.seek(geometry.data_offset - 8)
        assert struct.unpack('<4sI', source.read(8)) == (b'data', 0xffffffff)
        assert verify_payload(source, take.cancel, require_eof=False) == expected_hash
    streams_path = take.paths.encoded_path.with_name(take.paths.encoded_path.name + '.streams.json')
    with streams_path.open('rb') as source:
        metadata = source.read(65537)
    assert len(metadata) <= 65536
    validate_recording_streams(
        json.loads(metadata), take.paths.options, rate=take.ready.rate, channels=2, frames=FRAMES,
    )
    assert file_identity(take.paths.spool_path) == take.spool_identity
    assert inspect_mix_capture_spool(str(take.paths.spool_path)) == checkpoint_before
    assert engine.mix_capture_status(ticket) == before
    assert take.neighbor.read_bytes() == take.neighbor_bytes
    assert file_identity(take.neighbor) == take.neighbor_identity
    payload_file_extents = (
        take.paths.spool_path.stat().st_size + take.paths.pcm_path.stat().st_size
        + take.paths.encoded_path.stat().st_size
    )
    assert payload_file_extents < int(12.1 * (1 << 30))
    assert len({file_identity(path) for path in (
        take.paths.spool_path, take.paths.pcm_path, take.paths.encoded_path,
    )}) == 3
    return {
        'session_id': extent.session_id, 'first_frame': first, 'end_frame': end,
        'frames': FRAMES, 'rate': extent.rate, 'channels': 2,
        'payload_bytes': PAYLOAD_BYTES, 'writer_payload_sha256': expected_hash,
        'encoded_file_bytes': completion.codec.output_bytes,
        'encoded_sha256': completion.codec.output_sha256,
        'encoded_identity': completion.codec.output_identity,
        'spool_identity': take.spool_identity, 'rf64': True,
        'three_distinct_payload_file_extent_bytes': payload_file_extents,
        'final_is_encoded_hardlink': True, 'native_status_unchanged': True,
        'full_uncropped_payloads_verified': True, 'unknown_neighbor_preserved': True,
        'retired': take.retired.is_set(),
    }


class Rf64Work:
    def __init__(self, prepared_type, root, rate, channels, warm_probe=False):
        self.prepared_type = prepared_type
        self.root, self.rate, self.channels = root, rate, channels
        self.take = self.error = self.result = self.arguments = None
        self.ready = Event()
        self.continue_finalization = Event()
        self.aborted = Event()
        self.done = Event()
        self.joined = False
        self.closed = False
        self.readiness_scope_starts = 0
        self.warm_probe = warm_probe
        self.worker_hooks = None
        self.warm_trace_restored = self.warm_entry_trace_restored = False
        self.forwarded_calls = self.forwarded_returns = 0
        self.parent_profile, self.parent_trace = sys.getprofile(), sys.gettrace()
        self.future_profile_before = threading.getprofile()
        self.started = time.monotonic()
        self.meter = _OwnedProcessObservation()
        # Existing threading.setprofile hooks apply only to future threads.
        # The worker immediately replaces broad C-call profiling with selective
        # Python-call tracing of the SAME original supervisor method code objects.
        self.thread = Thread(target=self._entry, name='actual-rf64-readiness-finalizer')
        self.thread.start()

    @staticmethod
    def _warm_prior_trace(frame, event, argument):
        # A real non-None prior hook makes restoration observable in the warm
        # probe. It does not alter or replace any productive method or result.
        return None

    def _entry(self):
        if not self.warm_probe:
            self._run()
            return
        previous_trace = sys.gettrace()
        sentinel = self._warm_prior_trace
        sys.settrace(sentinel)
        try:
            self._run()
        finally:
            self.warm_trace_restored = sys.gettrace() is sentinel
            sys.settrace(previous_trace)
            self.warm_entry_trace_restored = sys.gettrace() is previous_trace

    def _trace_supervisor(self, frame, event, argument):
        if frame.f_code not in self.meter._codes:
            return None
        frame.f_trace_lines = False
        frame.f_trace_opcodes = False
        if event == 'call':
            self.forwarded_calls += 1
            self.meter._profile(frame, event, argument)
        elif event == 'return':
            self.forwarded_returns += 1
            self.meter._profile(frame, event, argument)
        return self._trace_supervisor

    def _run(self):
        previous_profile, previous_trace = sys.getprofile(), sys.gettrace()
        adapter = self._trace_supervisor
        profile_disabled = trace_installed = False
        try:
            # sys.settrace receives Python calls, not the converter's billions
            # of C calls. Unwatched frames get no local line/opcode trace.
            sys.setprofile(None)
            sys.settrace(adapter)
            profile_disabled = sys.getprofile() is None
            trace_installed = sys.gettrace() is adapter
            # Publish the object before its initializer so cancellation can reach
            # its original Event while actual codec preflight is still running.
            self.take = self.prepared_type.__new__(self.prepared_type)
            self.prepared_type.__init__(self.take, self.root, 'wav', self.rate, self.channels)
            self.readiness_scope_starts = self.meter.started
            self.ready.set()
            self.continue_finalization.wait()
            if self.aborted.is_set():
                return
            self.result = finish_streamed(self.take, *self.arguments)
        finally:
            self.error = sys.exception()
            try:
                try:
                    sys.settrace(previous_trace)
                finally:
                    sys.setprofile(previous_profile)
            finally:
                if self.error is None:
                    self.error = sys.exception()
                self.worker_hooks = {
                    'broad_profile_disabled': profile_disabled,
                    'selective_trace_installed': trace_installed,
                    'prior_profile_is_original_meter': (
                        getattr(previous_profile, '__self__', None) is self.meter
                        and getattr(previous_profile, '__func__', None) is type(self.meter)._profile
                    ),
                    'prior_trace_present': previous_trace is not None,
                    'profile_restored': sys.getprofile() is previous_profile,
                    'trace_restored': sys.gettrace() is previous_trace,
                }
                self.ready.set()
                self.done.set()

    def observer_hooks(self):
        assert self.worker_hooks is not None
        for key in (
            'broad_profile_disabled', 'selective_trace_installed',
            'prior_profile_is_original_meter', 'profile_restored', 'trace_restored',
        ):
            assert self.worker_hooks[key] is True, self.worker_hooks
        assert self.forwarded_calls == self.forwarded_returns > 0
        if self.warm_probe:
            assert self.worker_hooks['prior_trace_present'] is True
            assert self.warm_trace_restored and self.warm_entry_trace_restored
        return self.worker_hooks | {
            'forwarded_original_call_events': self.forwarded_calls,
            'forwarded_original_return_events': self.forwarded_returns,
            'watched_methods': sorted(self.meter._codes.values()),
            'warm_prior_trace_restored': self.warm_trace_restored if self.warm_probe else None,
            'warm_entry_trace_restored': self.warm_entry_trace_restored if self.warm_probe else None,
            'scope': 'worker adapter forwards only original meter._codes Python call/return, no C-call/line/opcode events; other original meter hooks unchanged',
        }

    def poll(self):
        if self.aborted.is_set():
            # A cancellation requested before PreparedTake created its original
            # Event must also reach that Event once initialization publishes it.
            self.cancel()
        self.meter.sample()
        return self.ready.is_set(), self.done.is_set(), self.error is not None

    def cancel(self):
        self.aborted.set()
        if self.take is not None and hasattr(self.take, 'cancel'):
            self.take.cancel.set()
        self.continue_finalization.set()

    def finalize(self, engine, ticket, expected_hash, first, end):
        assert self.ready.is_set() and not self.done.is_set()
        assert self.arguments is None and not self.aborted.is_set()
        self.arguments = engine, ticket, expected_hash, first, end
        self.continue_finalization.set()

    def finish(self):
        self.thread.join()
        self.joined = not self.thread.is_alive()
        assert self.joined
        hooks = self.observer_hooks()
        assert not self.meter.pending(), 'original owned process scopes still pending'
        report = self.meter.report()
        assert report['scope_starts'] > self.readiness_scope_starts > 0
        assert report['scope_starts'] == report['original_scope_closes']
        assert report['pending_scopes'] == report['inflight_supervisor_scopes'] == 0
        assert report['sampling_errors'] == ()
        assert report['peak_owned_members'] > 0
        assert report['samples'] > 0
        # Fixed before the first large run: a whole-payload allocation in the
        # parent or an observed owned process must fail this engineering gate.
        parent_peak = report['root_after']['peak_commit_charge_bytes']
        assert 0 < parent_peak <= (1 << 30), ('parent lifetime commit peak', parent_peak)
        rows = report['processes']
        assert rows
        for row in rows:
            assert row['samples'] > 0
            peak = row['maxima']['peak_commit_charge_bytes']
            assert 0 < peak <= (1 << 30), ('observed owned process lifetime commit peak', row)
        combined = report['sampled_concurrent_peaks']['combined_private_committed_bytes']
        assert 0 < combined <= (2 << 30), ('sampled combined private commit', combined)
        self.meter.close()
        self.closed = True
        if self.error is not None:
            raise RuntimeError('actual RF64 readiness/finalization worker failed') from self.error
        assert self.result is not None
        return json.dumps({
            'take': self.result, 'owned_process_observation': report,
            'worker_thread_joined': self.joined,
            'observer_hooks': hooks,
            'actual_readiness_scope_starts': self.readiness_scope_starts,
            'actual_finalization_scope_starts': report['scope_starts'] - self.readiness_scope_starts,
            'engineering_memory_gates': {
                'parent_and_each_observed_owned_lifetime_commit_ceiling_bytes': 1 << 30,
                'sampled_nonbusy_combined_private_commit_ceiling_bytes': 2 << 30,
                'parent_lifetime_commit_peak_bytes': parent_peak,
                'sampled_nonbusy_combined_private_commit_peak_bytes': combined,
            },
            'elapsed_seconds': time.monotonic() - self.started,
            'limits': (
                'manual off-RT producer, no CPAL/music/sustainable-rate/CONTROL; '
                'sampled simultaneous RAM differs from individual OS lifetime peaks; '
                'Native hash binding detaches the GIL but one hash cannot be cancelled internally; '
                'deadline requests cancellation and still waits for actual thread/tree retirement'
            ),
        }, sort_keys=True)

    def finish_cancelled_preflight(self):
        assert self.ready.is_set() and not self.done.is_set()
        assert self.arguments is self.result is None
        self.cancel()
        self.thread.join()
        self.joined = not self.thread.is_alive()
        assert self.joined and self.done.is_set()
        assert self.error is None, self.error
        assert self.aborted.is_set() and self.take.cancel.is_set()
        assert self.result is None
        assert self.take.ready.proof.decoded_frames == 10001
        assert not self.take.paths.spool_path.exists()
        assert not self.take.paths.pcm_path.exists()
        assert not self.take.paths.encoded_path.exists()
        assert not self.take.paths.final_path.exists()
        assert not self.meter.pending()
        hooks = self.observer_hooks()
        report = self.meter.report()
        assert report['scope_starts'] == self.readiness_scope_starts > 0
        assert report['scope_starts'] == report['original_scope_closes']
        assert report['pending_scopes'] == report['inflight_supervisor_scopes'] == 0
        assert report['sampling_errors'] == ()
        assert report['peak_owned_members'] > 0
        self.meter.close()
        self.closed = True
        assert sys.getprofile() is self.parent_profile
        assert sys.gettrace() is self.parent_trace
        assert threading.getprofile() is self.future_profile_before
        return json.dumps({
            'case': 'actual_preflight_then_cancel_before_native_prepare',
            'actual_preflight_frames': self.take.ready.proof.decoded_frames,
            'owned_process_observation': report, 'observer_hooks': hooks,
            'original_cancel_event_set': self.take.cancel.is_set(),
            'worker_thread_joined': self.joined,
            'parent_profile_trace_and_future_profile_restored': True,
            'spool_or_final_created': False,
            'limits': 'real codec preflight and wait-gate cancellation only; no Native capture, RF64, long-run memory or active-child cancellation acceptance',
        }, sort_keys=True)

    def abandon_and_join(self):
        self.cancel()
        self.thread.join()
        self.joined = not self.thread.is_alive()
        assert self.joined
        assert not self.meter.pending(), 'failed fixture still owns process scopes'
        if not self.closed:
            self.meter.close()
            self.closed = True
"#;

struct Rf64Worker {
    work: Py<PyAny>,
    completed: bool,
    deadline_requested: bool,
}

impl Rf64Worker {
    fn new(rig: &FormatRig) -> Self {
        Self::new_mode(rig, false)
    }

    fn new_mode(rig: &FormatRig, warm_probe: bool) -> Self {
        let work = Python::attach(|py| {
            let module = python_checked(
                py,
                "load RF64 streaming probe",
                PyModule::from_code(
                    py,
                    &CString::new(RF64_PYTHON).unwrap(),
                    c"native_capture_rf64_probe.py",
                    c"native_capture_rf64_probe",
                ),
            );
            let base = format_probe(py);
            python_checked(
                py,
                "start actual RF64 preparation worker",
                module.getattr("Rf64Work").unwrap().call1((
                    base.getattr("PreparedTake").unwrap(),
                    rig.root.to_string_lossy().as_ref(),
                    RATE,
                    CHANNELS,
                    warm_probe,
                )),
            )
            .unbind()
        });
        Self {
            work,
            completed: false,
            deadline_requested: false,
        }
    }

    fn poll(&mut self, deadline: Instant) -> (bool, bool, bool) {
        if Instant::now() >= deadline && !self.deadline_requested {
            self.deadline_requested = true;
            eprintln!(
                "RF64 engineering deadline: requesting original cancellation; waiting for actual ownership retirement"
            );
            Python::attach(|py| {
                python_checked(
                    py,
                    "request RF64 cancellation",
                    self.work.call_method0(py, "cancel"),
                );
            });
        }
        Python::attach(|py| {
            python_checked(
                py,
                "observe RF64 worker",
                self.work.call_method0(py, "poll"),
            )
            .extract::<(bool, bool, bool)>(py)
            .unwrap()
        })
    }

    fn finish(&mut self) -> Value {
        let text = Python::attach(|py| {
            python_checked(
                py,
                "join RF64 worker and original process scopes",
                self.work.call_method0(py, "finish"),
            )
            .extract::<String>(py)
            .unwrap()
        });
        self.completed = true;
        assert!(
            !self.deadline_requested,
            "RF64 engineering deadline exceeded; actual worker joined before rejection"
        );
        serde_json::from_str(&text).unwrap()
    }

    fn finish_cancelled_preflight(&mut self) -> Value {
        let text = Python::attach(|py| {
            python_checked(
                py,
                "cancel actual RF64 preflight wait and join original worker",
                self.work.call_method0(py, "finish_cancelled_preflight"),
            )
            .extract::<String>(py)
            .unwrap()
        });
        self.completed = true;
        assert!(
            !self.deadline_requested,
            "RF64 observer warm-probe deadline exceeded; actual worker joined before rejection"
        );
        serde_json::from_str(&text).unwrap()
    }
}

impl Drop for Rf64Worker {
    fn drop(&mut self) {
        if !self.completed {
            // Keep the real owner alive until the sole Python worker has actually
            // joined, including during Rust assertion unwinding. No fake retired
            // flag or process kill substitutes for the original scope's closure.
            Python::attach(|py| {
                if let Err(error) = self.work.call_method0(py, "abandon_and_join") {
                    error.print(py);
                    eprintln!(
                        "RF64 failed-fixture process retirement unproven; owned output retained"
                    );
                }
            });
        }
    }
}

impl FormatRig {
    fn prepare_rf64(&self, path: &str) -> Py<MixCaptureTicket> {
        let ticket = Python::attach(|py| {
            self.engine
                .borrow(py)
                .prepare_mix_capture_internal(
                    path,
                    RF64_BLOCK_FRAMES,
                    RF64_BLOCK_COUNT,
                    RF64_POOL_BYTES,
                )
                .unwrap()
        });
        let status = wait_for(&ticket.observer, "actual RF64 writer ready", |status| {
            status.writer_ready
        });
        assert_eq!(
            (status.config.sample_rate_hz, status.config.channels),
            (RATE, CHANNELS)
        );
        assert_eq!(status.allocated_blocks, RF64_BLOCK_COUNT);
        assert_eq!(status.pool_pcm_bytes, RF64_POOL_BYTES);
        assert_eq!(
            status.writer_scratch_bytes,
            RF64_BLOCK_FRAMES * CHANNELS * 4
        );
        Python::attach(|py| Py::new(py, ticket).unwrap())
    }

    fn claim_rf64(&mut self, ticket: &Py<MixCaptureTicket>) -> Box<CaptureRt> {
        let end = FORMAT_FIRST + RF64_FRAMES;
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
            panic!("original RF64 start publication expected")
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
            panic!("original RF64 exclusive stop expected")
        };
        assert_eq!((session_id, target_frame), (id, MixCaptureTarget::At(end)));
        assert!(self.consumer.is_empty());
        let mut rt = publication.claim().expect("own RF64 prepared capture");
        assert!(publication.claim().is_none());
        assert!(rt.start(id, FORMAT_FIRST, RATE, CHANNELS));
        rt
    }
}

fn rf64_capture(
    rig: &mut FormatRig,
    ticket: &Py<MixCaptureTicket>,
    worker: &mut Rf64Worker,
    deadline: Instant,
    phases: &mut Vec<Value>,
) -> (CaptureStatus, String, u64) {
    let observer = Python::attach(|py| ticket.borrow(py).observer.clone());
    let id = observer.status().session_id;
    let mut rt = rig.claim_rf64(ticket);
    let mut pcm: Vec<f32> = (0..RF64_BLOCK_FRAMES * CHANNELS)
        .map(|index| RF64_PATTERN[index % RF64_PATTERN.len()])
        .collect();
    let mut golden: Vec<u8> = (0..RF64_BLOCK_FRAMES * CHANNELS * 4)
        .map(|index| RF64_GOLDEN[index % RF64_GOLDEN.len()])
        .collect();
    let mut hash = Sha256::new();
    let mut frames = 0_u64;
    let mut block = 0_u32;
    let mut maximum_unwritten = 0_u64;
    let mut next_sample = Instant::now();
    let mut next_quarter = RF64_FRAMES / 4;
    let mut boundary_observed = false;
    while frames < RF64_FRAMES {
        loop {
            let status = observer.status();
            assert!(
                status.failure.is_none(),
                "actual RF64 writer failed: {status:?}"
            );
            let unwritten = status
                .submitted_blocks
                .checked_sub(status.written_blocks)
                .unwrap();
            maximum_unwritten = maximum_unwritten.max(unwritten);
            if Instant::now() >= next_sample {
                let (_, done, failed) = worker.poll(deadline);
                assert!(
                    !done && !failed,
                    "RF64 preparation worker exited during capture"
                );
                assert!(
                    !worker.deadline_requested,
                    "RF64 deadline requested cancellation during actual capture"
                );
                next_sample = Instant::now() + Duration::from_millis(20);
            }
            // Free blocks are returned immediately after written progress. With
            // eight actual blocks, at most two outstanding submissions leave
            // room for that handoff; a rejected append is never retried.
            if unwritten < 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let count = (RF64_FRAMES - frames).min(RF64_BLOCK_FRAMES as u64) as usize;
        let marker = 0x3f00_0000 | block;
        pcm[0] = f32::from_bits(marker);
        pcm[1] = f32::from_bits(marker | 0x8000_0000);
        golden[..4].copy_from_slice(&marker.to_le_bytes());
        golden[4..8].copy_from_slice(&(marker | 0x8000_0000).to_le_bytes());
        assert!(
            rt.append_segment(FORMAT_FIRST + frames, &pcm[..count * CHANNELS]),
            "actual RF64 append rejected at frame {frames}: {:?}",
            observer.status()
        );
        hash.update(&golden[..count * CHANNELS * 4]);
        frames += count as u64;
        block += 1;
        if frames == (1 << 29) {
            let status = observer.status();
            phases.push(json!({
                "phase":"actual_capture_riff32_payload_boundary", "captured_frames":frames,
                "captured_payload_bytes":frames * CHANNELS as u64 * 4,
                "submitted_frames":status.submitted_frames, "written_frames":status.written_frames,
                "parent":crate::audio_engine::c3_process_metrics::snapshot(),
            }));
            boundary_observed = true;
        }
        if frames >= next_quarter && phases.len() < 7 {
            let status = observer.status();
            phases.push(json!({
                "phase":"actual_capture_progress", "captured_frames":frames,
                "submitted_frames":status.submitted_frames, "written_frames":status.written_frames,
                "buffered_frames":status.buffered_frames,
                "parent":crate::audio_engine::c3_process_metrics::snapshot(),
            }));
            next_quarter += RF64_FRAMES / 4;
        }
    }
    assert_eq!(block, 32_769);
    assert!(boundary_observed);
    assert_eq!(frames * CHANNELS as u64 * 4, RF64_PAYLOAD_BYTES);
    assert!(maximum_unwritten <= 2);
    assert_eq!(observer.status().buffered_frames, 8193);
    assert!(rt.stop(id, FORMAT_FIRST + frames));
    assert!(rt.terminal());
    drop(rt); // Explicit manual off-RT producer: actual heavy owner release.
    let expected_digest = hash.finalize();
    let expected_hash = format!("{expected_digest:x}");
    let expected_digest: [u8; 32] = expected_digest.into();
    loop {
        let status = Python::attach(|py| {
            rig.engine
                .borrow(py)
                .mix_capture_status_internal(py, &ticket.borrow(py))
                .unwrap();
            observer.status()
        });
        assert!(
            status.failure.is_none(),
            "RF64 terminal writer failure: {status:?}"
        );
        let (_, done, failed) = worker.poll(deadline);
        assert!(
            !done && !failed,
            "RF64 preparation worker exited before writer settlement"
        );
        assert!(
            !worker.deadline_requested,
            "RF64 deadline requested cancellation before writer settlement"
        );
        if status.rt_released && status.worker_closed && status.worker_joined {
            assert!(
                status.terminal_decided && status.successful_spool(),
                "{status:?}"
            );
            assert_eq!(status.payload_sha256, Some(expected_digest));
            assert_eq!(
                (status.first_frame, status.end_frame),
                (Some(FORMAT_FIRST), Some(FORMAT_FIRST + frames))
            );
            assert_eq!(
                (status.submitted_frames, status.written_frames),
                (frames, frames)
            );
            assert_eq!(
                (status.submitted_blocks, status.written_blocks),
                (32_769, 32_769)
            );
            assert_eq!(status.buffered_frames, 0);
            return (status, expected_hash, maximum_unwritten);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "writes about 12 GiB actual payload; runner storage decision and isolated FLITZIS_RECORDING_FORMAT_PYTHON host required"]
fn actual_native_writer_finalizes_over_riff32_float_wav_with_bounded_memory() {
    let started = Instant::now();
    let deadline = started + RF64_DEADLINE;
    let process_before = crate::audio_engine::c3_process_metrics::snapshot();
    let _runtime = FormatPython::configure();
    assert_eq!((RATE, CHANNELS), (44_100, 2));
    let mut rig = FormatRig::new();
    let root = rig.root.clone();
    let mut worker = Rf64Worker::new(&rig);
    loop {
        let (ready, done, failed) = worker.poll(deadline);
        assert!(
            !done && !failed,
            "actual RF64 readiness worker exited before capture"
        );
        assert!(
            !worker.deadline_requested,
            "RF64 readiness exceeded engineering deadline"
        );
        if ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let take = Python::attach(|py| worker.work.getattr(py, "take").unwrap());
    let spool = Python::attach(|py| {
        take.call_method0(py, "spool_path")
            .unwrap()
            .extract::<String>(py)
            .unwrap()
    });
    let ticket = rig.prepare_rf64(&spool);
    Python::attach(|py| {
        python_checked(
            py,
            "capture original RF64 writer FileID",
            take.call_method0(py, "capture_writer_identity"),
        );
    });
    let mut phases = vec![
        json!({
            "phase":"before_imports_and_preparation", "parent":process_before,
        }),
        json!({
            "phase":"actual_ready_before_capture", "parent":crate::audio_engine::c3_process_metrics::snapshot(),
        }),
    ];
    let (status, expected_hash, maximum_unwritten) =
        rf64_capture(&mut rig, &ticket, &mut worker, deadline, &mut phases);
    phases.push(json!({
        "phase":"actual_writer_joined_before_finalizer", "parent":crate::audio_engine::c3_process_metrics::snapshot(),
    }));
    Python::attach(|py| {
        python_checked(
            py,
            "handoff original RF64 ticket to finalizer",
            worker.work.call_method1(
                py,
                "finalize",
                (
                    rig.engine.bind(py),
                    ticket.bind(py),
                    &expected_hash,
                    FORMAT_FIRST,
                    FORMAT_FIRST + RF64_FRAMES,
                ),
            ),
        );
    });
    loop {
        let (_, done, _) = worker.poll(deadline);
        if done {
            break;
        }
        // The actual future Python worker owns finalization. The driver only
        // samples; Native hash/verify already detach, without an internal cancel.
        std::thread::sleep(Duration::from_millis(20));
    }
    let result = worker.finish();
    phases.push(json!({
        "phase":"actual_finalizer_retired_and_thread_joined_before_owner_drop",
        "parent":crate::audio_engine::c3_process_metrics::snapshot(),
    }));
    drop(take);
    drop(ticket);
    drop(worker);
    drop(rig);
    Python::attach(|py| {
        py.import("gc").unwrap().call_method0("collect").unwrap();
    });
    let process_after = crate::audio_engine::c3_process_metrics::snapshot();
    phases.push(json!({"phase":"after_actual_owner_drop_and_gc", "parent":process_after}));
    let evidence = json!({
        "case":"actual_native_writer_to_original_ticket_finalizer_over_riff32_float_wav",
        "result":result, "expected_payload_sha256":expected_hash,
        "frames":RF64_FRAMES, "payload_bytes":RF64_PAYLOAD_BYTES,
        "first_frame":FORMAT_FIRST, "end_frame":FORMAT_FIRST + RF64_FRAMES,
        "rate":RATE, "channels":CHANNELS,
        "capture_pool_pcm_bytes":status.pool_pcm_bytes,
        "capture_allocated_blocks":status.allocated_blocks,
        "writer_scratch_bytes":status.writer_scratch_bytes,
        "producer_pcm_bytes":RF64_BLOCK_FRAMES * CHANNELS * 4,
        "producer_oracle_bytes":RF64_BLOCK_FRAMES * CHANNELS * 4,
        "maximum_observed_submitted_minus_written_blocks":maximum_unwritten,
        "engineering_deadline_seconds":RF64_DEADLINE.as_secs(),
        "elapsed_seconds":started.elapsed().as_secs_f64(),
        "parent_process_phases":phases,
        "parent_process_delta":crate::audio_engine::c3_process_metrics::delta(&process_before, &process_after),
        "runtime":crate::audio_engine::c3_process_metrics::runtime_identity().unwrap(),
        "limits":"actual large-file bounded-buffer software composition; one manual off-RT producer and one original finalizer worker; no sparse writes, whole-take arrays, CPAL, sustainable-rate, music, hearing, high-level CONTROL or hard process-RAM acceptance; imported modules/shared services remain loaded after owner drop",
    });
    let text = serde_json::to_string(&evidence).unwrap();
    fs::write(root.join("rf64-evidence.json"), &text).unwrap();
    eprintln!(
        "actual Native writer RF64 finalization: {text}; retained {}",
        root.display()
    );
}

#[test]
#[ignore = "actual codec preflight; isolated FLITZIS_RECORDING_FORMAT_PYTHON host required; no large capture"]
fn actual_rf64_selective_observer_preflight_cancel_restores_hooks() {
    let _runtime = FormatPython::configure();
    let rig = FormatRig::new();
    let root = rig.root.clone();
    let mut worker = Rf64Worker::new_mode(&rig, true);
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let (ready, done, failed) = worker.poll(deadline);
        assert!(
            !done && !failed,
            "actual RF64 observer warm-probe preparation worker exited early"
        );
        assert!(
            !worker.deadline_requested,
            "RF64 observer warm-probe preparation exceeded deadline"
        );
        if ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let evidence = worker.finish_cancelled_preflight();
    drop(worker);
    drop(rig);
    Python::attach(|py| {
        py.import("gc").unwrap().call_method0("collect").unwrap();
    });
    let text = serde_json::to_string(&evidence).unwrap();
    fs::write(root.join("rf64-observer-warm-evidence.json"), &text).unwrap();
    eprintln!(
        "actual RF64 selective observer warm probe: {text}; retained {}",
        root.display()
    );
}
