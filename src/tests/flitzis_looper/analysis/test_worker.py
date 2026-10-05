import hashlib
import json
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import replace
from pathlib import Path
from threading import Event, Thread
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatComponentResult,
    BeatModelIdentity,
    BeatWorkerRequest,
    MonoPcmInput,
    WorkerLimits,
    decode_response,
    encode_request,
)
from flitzis_looper.analysis.worker import BeatWorkerAdapter, WorkerConfiguration

if TYPE_CHECKING:
    from collections.abc import Callable
    from tempfile import TemporaryDirectory


_WORKER_HEADER = """
import json
import os
import pathlib
import sys
import time
request_path = pathlib.Path(sys.argv[sys.argv.index('--request') + 1])
request = json.loads(request_path.read_text())
pcm_path = pathlib.Path(request['pcm']['path'])
pcm_path.with_suffix('.started').write_text(str(os.getpid()))
assert request['pcm']['dtype'] == 'float32-le'
assert request['pcm']['channels'] == 1
assert request['pcm']['origin_seconds'] == 0.0
assert pcm_path.stat().st_size == request['pcm']['frame_count'] * 4
assert os.environ['OMP_NUM_THREADS'] == '1'
assert os.environ['HF_HUB_OFFLINE'] == '1'
assert os.environ['TRANSFORMERS_OFFLINE'] == '1'
response = {
    'schema_version': 1,
    'identity': request['identity'],
    'model': request['model'],
    'predictions': {
        'beat_seconds': [0.125, 0.5, 0.875],
        'downbeat_seconds': [0.125],
        'beat_logits': [0.1, 0.8],
        'downbeat_logits': [0.2, 0.9],
    },
}
"""


@pytest.fixture
def prepared(tmp_path: Path) -> tuple[WorkerConfiguration, BeatWorkerRequest]:
    checkpoint = tmp_path / "fixture.checkpoint"
    checkpoint.write_bytes(b"deterministic test double; never neural weights")
    model = BeatModelIdentity(
        sha256=hashlib.sha256(checkpoint.read_bytes()).hexdigest(),
        frontend_id="fixture-front-end-v1",
        environment_id="fixture-environment-v1",
    )
    script = tmp_path / "worker.py"
    script.write_text(_WORKER_HEADER + "print(json.dumps(response))\n", encoding="utf-8")
    pcm = tmp_path / "mono.f32"
    pcm.write_bytes(bytes(48000 * 4))
    request = BeatWorkerRequest(
        AnalysisIdentity(3, 42, "source-fixture", 9), MonoPcmInput(pcm, 48000, 48000), model
    )
    return WorkerConfiguration(Path(sys.executable), script, checkpoint, tmp_path, model), request


def _run(
    adapter: BeatWorkerAdapter, request: BeatWorkerRequest, cancel: Event | None = None
) -> BeatComponentResult:
    with ThreadPoolExecutor(max_workers=1) as executor:
        return executor.submit(adapter.run, request, cancel or Event()).result(timeout=10)


def _wait_started(request: BeatWorkerRequest) -> None:
    deadline = time.monotonic() + 5
    while not request.pcm.path.with_suffix(".started").exists():
        assert time.monotonic() < deadline, "worker failed to start"
        Event().wait(0.01)


def _record_processes(monkeypatch: pytest.MonkeyPatch) -> list[subprocess.Popen[bytes]]:
    processes: list[subprocess.Popen[bytes]] = []
    original = process._start

    def start(command: process.WorkerCommand, request_path: Path) -> subprocess.Popen[bytes]:
        child = original(command, request_path)
        processes.append(child)
        return child

    monkeypatch.setattr(process, "_start", start)
    return processes


def test_unconfigured_worker_is_lazy_and_does_not_start_process(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest], monkeypatch: pytest.MonkeyPatch
) -> None:
    _, request = prepared
    processes = _record_processes(monkeypatch)
    adapter = BeatWorkerAdapter()
    assert adapter.retired.is_set()
    result = _run(adapter, request)
    assert result.status == "unavailable"
    assert result.reason == "missing_worker"
    assert result.predictions is None
    assert processes == []
    assert request.pcm.path.exists()


@pytest.mark.parametrize(
    ("change", "reason"),
    [
        ("missing_worker", "missing_worker"),
        ("missing_checkpoint", "missing_checkpoint"),
        ("empty_checkpoint", "invalid_checkpoint_size"),
        ("corrupt_checkpoint", "checkpoint_hash_mismatch"),
        ("wrong_digest", "model_identity_mismatch"),
        ("unverified_manifest", "unverified_model_manifest"),
    ],
)
def test_local_preflight_never_invokes_worker_or_acquires_model(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest],
    monkeypatch: pytest.MonkeyPatch,
    change: str,
    reason: str,
) -> None:
    configuration, request = prepared
    processes = _record_processes(monkeypatch)
    if change == "missing_worker":
        configuration.script.unlink()
    elif change == "missing_checkpoint":
        configuration.checkpoint.unlink()
    elif change == "empty_checkpoint":
        configuration.checkpoint.write_bytes(b"")
    elif change == "corrupt_checkpoint":
        configuration.checkpoint.write_bytes(b"wrong test content")
    elif change == "wrong_digest":
        request = replace(request, model=replace(request.model, sha256="0" * 64))
    else:
        request = replace(request, model=replace(request.model, sha256=""))
        configuration = replace(configuration, model=request.model)

    result = _run(BeatWorkerAdapter(configuration), request)

    assert result.status == "unavailable"
    assert result.reason == reason
    assert processes == []
    assert not request.pcm.path.with_suffix(".started").exists()


def test_worker_roundtrip_retains_source_domain_and_raw_fractional_times(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest],
) -> None:
    configuration, request = prepared
    adapter = BeatWorkerAdapter(configuration)
    before = request.pcm.path.read_bytes()

    result = _run(adapter, request)

    assert result.status == "ready"
    assert result.identity == request.identity
    assert result.model == request.model
    assert result.predictions is not None
    assert result.predictions.beat_seconds == (0.125, 0.5, 0.875)
    assert result.predictions.beat_logits == (0.1, 0.8)
    assert result.resources_released
    assert adapter.retired.is_set()
    assert list(configuration.scratch_dir.glob("beat-request-*")) == []
    assert request.pcm.path.read_bytes() == before


@pytest.mark.parametrize("failure", ["extent", "origin", "oversize"])
def test_invalid_full_track_input_is_rejected_before_worker_start(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest], failure: str
) -> None:
    configuration, request = prepared
    limits = WorkerLimits()
    if failure == "extent":
        request.pcm.path.write_bytes(b"short")
    elif failure == "origin":
        request = replace(request, pcm=replace(request.pcm, origin_seconds=0.25))
    else:
        limits = replace(limits, max_pcm_bytes=4)

    result = _run(BeatWorkerAdapter(configuration, limits), request)

    assert result.status == "failed"
    assert result.reason == "invalid_pcm_request"
    assert not request.pcm.path.with_suffix(".started").exists()


@pytest.mark.parametrize(
    "mutation",
    [
        "response['identity']['request_id'] += 1",
        "response['identity']['source_generation'] += 1",
        "response['identity']['source_id'] = 'other-source'",
        "response['identity']['pad_id'] += 1",
        "response['model']['sha256'] = '0' * 64",
        "response['schema_version'] = 2",
        "response['schema_version'] = True",
        "del response['schema_version']",
        "response['unexpected'] = True",
        "response['predictions']['beat_seconds'] = [0.5, 0.125]",
        "response['predictions']['beat_seconds'] = [0.5, 0.5]",
        "response['predictions']['beat_seconds'] = [-0.1]",
        "response['predictions']['beat_seconds'] = [1.0]",
        "response['predictions']['beat_seconds'] = [float('nan')]",
        "response['predictions']['beat_logits'] = [float('inf'), 0.2]",
        "response['predictions']['downbeat_logits'] = []",
    ],
)
def test_untrusted_worker_response_cannot_publish(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest], mutation: str
) -> None:
    configuration, request = prepared
    configuration.script.write_text(
        _WORKER_HEADER + mutation + "\nprint(json.dumps(response))\n", encoding="utf-8"
    )

    result = _run(BeatWorkerAdapter(configuration), request)

    assert result.status == "failed"
    assert result.reason == "invalid_worker_response"
    assert result.predictions is None


@pytest.mark.parametrize(
    ("body", "reason"),
    [
        ("time.sleep(60)", "worker_timeout"),
        ("raise SystemExit(7)", "worker_crashed"),
        ("sys.stdout.write('x' * 1000000); sys.stdout.flush(); time.sleep(60)", "response_limit"),
    ],
)
def test_timeout_crash_and_output_flood_reap_before_releasing_resources(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest],
    monkeypatch: pytest.MonkeyPatch,
    body: str,
    reason: str,
) -> None:
    configuration, request = prepared
    configuration.script.write_text(_WORKER_HEADER + body, encoding="utf-8")
    processes = _record_processes(monkeypatch)
    adapter = BeatWorkerAdapter(
        configuration, WorkerLimits(timeout_seconds=0.4, max_response_bytes=4096)
    )

    result = _run(adapter, request)

    assert result.status == "failed"
    assert result.reason == reason
    assert result.resources_released
    assert adapter.retired.is_set()
    assert len(processes) == 1
    assert processes[0].poll() is not None
    assert list(configuration.scratch_dir.glob("beat-request-*")) == []


def test_cancellation_reaps_worker_and_busy_request_cannot_start_a_second_process(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest], monkeypatch: pytest.MonkeyPatch
) -> None:
    configuration, request = prepared
    configuration.script.write_text(_WORKER_HEADER + "time.sleep(60)", encoding="utf-8")
    processes = _record_processes(monkeypatch)
    adapter = BeatWorkerAdapter(configuration)
    cancel = Event()
    with ThreadPoolExecutor(max_workers=2) as executor:
        running = executor.submit(adapter.run, request, cancel)
        try:
            _wait_started(request)
            assert not adapter.retired.is_set()
            busy = executor.submit(BeatWorkerAdapter(configuration).run, request, Event()).result(5)
            assert busy.reason == "worker_busy"
            assert len(processes) == 1
        finally:
            cancel.set()
        result = running.result(5)

    assert result.status == "cancelled"
    assert result.resources_released
    assert adapter.retired.is_set()
    assert processes[0].poll() is not None


def test_cancelled_request_does_not_hash_or_start_worker(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest], monkeypatch: pytest.MonkeyPatch
) -> None:
    configuration, request = prepared
    processes = _record_processes(monkeypatch)
    cancel = Event()
    cancel.set()
    result = _run(BeatWorkerAdapter(configuration), request, cancel)
    assert result.status == "cancelled"
    assert processes == []


def test_invalid_scratch_releases_global_slot(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest],
) -> None:
    configuration, request = prepared
    missing = replace(configuration, scratch_dir=configuration.scratch_dir / "missing")
    assert _run(BeatWorkerAdapter(missing), request).reason == "worker_scratch_unavailable"
    assert _run(BeatWorkerAdapter(configuration), request).status == "ready"


def test_ui_thread_cannot_perform_blocking_analysis(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest],
) -> None:
    configuration, request = prepared
    with pytest.raises(RuntimeError, match="background supervisor"):
        BeatWorkerAdapter(configuration).run(request, Event())


def test_metadata_request_has_no_pcm_payload_and_checks_counts(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest],
) -> None:
    _, request = prepared
    encoded = encode_request(request, WorkerLimits())
    dto = json.loads(encoded)
    assert dto["schema_version"] == 1
    assert dto["pcm"]["frame_count"] == 48000
    assert dto["pcm"]["sample_rate_hz"] == 48000
    assert len(encoded) < 2048
    response = {
        "schema_version": 1,
        "identity": dto["identity"],
        "model": dto["model"],
        "predictions": {
            "beat_seconds": [0.1, 0.2],
            "downbeat_seconds": [],
            "beat_logits": [],
            "downbeat_logits": [],
        },
    }
    with pytest.raises(ValueError, match="prediction_limit"):
        decode_response(
            json.dumps(response).encode(), request, WorkerLimits(max_prediction_count=1)
        )


def test_failed_finite_reap_stays_retiring_until_process_is_reaped(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest], monkeypatch: pytest.MonkeyPatch
) -> None:
    configuration, request = prepared
    configuration.script.write_text(_WORKER_HEADER + "time.sleep(60)", encoding="utf-8")
    original = process._start
    release_reaper = Event()
    reaper_started = Event()

    def start(command: process.WorkerCommand, request_path: Path) -> subprocess.Popen[bytes]:
        child = original(command, request_path)
        real_wait: Callable[[float | None], int] = child.wait

        def wait(timeout: float | None = None) -> int:
            if timeout is not None:
                command_name = "fixture worker"
                raise subprocess.TimeoutExpired(command_name, timeout)
            reaper_started.set()
            assert release_reaper.wait(5)
            return real_wait(None)

        monkeypatch.setattr(child, "wait", wait)
        return child

    monkeypatch.setattr(process, "_start", start)
    adapter = BeatWorkerAdapter(configuration, WorkerLimits(timeout_seconds=0.4))
    try:
        result = _run(adapter, request)
        assert reaper_started.wait(2)
        assert result.reason == "worker_retiring"
        assert not result.resources_released
        assert not adapter.retired.is_set()
        assert list(configuration.scratch_dir.glob("beat-request-*"))
        assert _run(BeatWorkerAdapter(configuration), request).reason == "worker_busy"
    finally:
        release_reaper.set()
        assert adapter.retired.wait(5)
    assert list(configuration.scratch_dir.glob("beat-request-*")) == []


def test_output_thread_start_failure_still_kills_and_reaps_process(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest], monkeypatch: pytest.MonkeyPatch
) -> None:
    configuration, request = prepared
    configuration.script.write_text(_WORKER_HEADER + "time.sleep(60)", encoding="utf-8")
    children = _record_processes(monkeypatch)
    real_start = Thread.start

    def start(thread: Thread) -> None:
        if thread.name == "beat-worker-output":
            msg = "fixture thread allocation failure"
            raise RuntimeError(msg)
        real_start(thread)

    monkeypatch.setattr(Thread, "start", start)
    adapter = BeatWorkerAdapter(configuration)

    result = _run(adapter, request)

    assert result.reason == "worker_supervision_failed"
    assert result.resources_released
    assert adapter.retired.is_set()
    assert children[0].poll() is not None
    assert list(configuration.scratch_dir.glob("beat-request-*")) == []


def test_retirement_thread_failure_keeps_supervisor_until_resources_are_released(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest], monkeypatch: pytest.MonkeyPatch
) -> None:
    configuration, request = prepared
    configuration.script.write_text(_WORKER_HEADER + "time.sleep(60)", encoding="utf-8")
    original = process._start
    real_start = Thread.start
    children: list[subprocess.Popen[bytes]] = []

    def start_process(
        command: process.WorkerCommand, request_path: Path
    ) -> subprocess.Popen[bytes]:
        child = original(command, request_path)
        children.append(child)
        real_wait: Callable[[float | None], int] = child.wait

        def wait(timeout: float | None = None) -> int:
            if timeout is not None:
                command_name = "fixture worker"
                raise subprocess.TimeoutExpired(command_name, timeout)
            return real_wait(None)

        monkeypatch.setattr(child, "wait", wait)
        return child

    def start_thread(thread: Thread) -> None:
        if thread.name == "beat-worker-retiring":
            msg = "fixture thread allocation failure"
            raise RuntimeError(msg)
        real_start(thread)

    monkeypatch.setattr(process, "_start", start_process)
    monkeypatch.setattr(Thread, "start", start_thread)
    adapter = BeatWorkerAdapter(configuration, WorkerLimits(timeout_seconds=0.2))

    result = _run(adapter, request)

    assert result.reason == "worker_supervision_failed"
    assert result.resources_released
    assert adapter.retired.is_set()
    assert children[0].poll() is not None
    assert list(configuration.scratch_dir.glob("beat-request-*")) == []


def test_failed_process_start_keeps_temporary_resource_slot_during_cleanup(
    prepared: tuple[WorkerConfiguration, BeatWorkerRequest], monkeypatch: pytest.MonkeyPatch
) -> None:
    configuration, request = prepared
    real_release = process._release_temporary
    release_cleanup = Event()

    def fail_start(command: process.WorkerCommand, request_path: Path) -> subprocess.Popen[bytes]:
        msg = "fixture process allocation failure"
        raise OSError(msg)

    def release(temporary: TemporaryDirectory[str], retired: Event) -> bool:
        if not release_cleanup.is_set():
            return False
        return real_release(temporary, retired)

    monkeypatch.setattr(process, "_start", fail_start)
    monkeypatch.setattr(process, "_release_temporary", release)
    adapter = BeatWorkerAdapter(configuration)
    try:
        result = _run(adapter, request)
        assert result.reason == "worker_retiring"
        assert not result.resources_released
        assert not adapter.retired.is_set()
        assert _run(BeatWorkerAdapter(configuration), request).reason == "worker_busy"
    finally:
        release_cleanup.set()
        assert adapter.retired.wait(5)
    assert list(configuration.scratch_dir.glob("beat-request-*")) == []
