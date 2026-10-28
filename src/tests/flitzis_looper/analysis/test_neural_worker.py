"""Native-worker bridge contracts; protocol doubles do not prove model quality."""

import base64
import json
import struct
import sys
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, replace
from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import neural_worker as bridge
from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatComponentResult,
    BeatModelIdentity,
    BeatPredictions,
)
from flitzis_looper.analysis.execution import (
    CudaBeatComponentResult,
    CudaBeatModelIdentity,
    ExecutionReceipt,
)
from flitzis_looper.analysis.publication import MAX_ENVELOPE_BYTES
from flitzis_looper.analysis.worker import WorkerConfiguration

if TYPE_CHECKING:
    from flitzis_looper.analysis.contracts import BeatWorkerRequest


class _Probe:
    def __init__(self) -> None:
        self.cancelled = False

    def is_cancelled(self) -> bool:
        return self.cancelled


class _Adapter:
    def __init__(self) -> None:
        self.retired = Event()
        self.retired.set()
        self.requests: list[BeatWorkerRequest] = []
        self.predictions = BeatPredictions(
            (0.125, 0.5000000000000001, 0.875),
            (0.125,),
            (-0.0, 0.12500000000000003, 0.8),
            (0.0, -2.5, 0.9),
        )

    def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult:
        self.requests.append(request)
        assert not cancel.is_set()
        return BeatComponentResult(
            request.identity, request.model, "ready", "ready", self.predictions
        )


@pytest.fixture
def request_fixture(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[tuple[object, ...], _Adapter, _Probe, WorkerConfiguration]:
    pcm = tmp_path / "native-owned-mono.f32le"
    pcm.write_bytes(bytes(48000 * 4))
    model = BeatModelIdentity(
        sha256="a" * 64, frontend_id="fixture-frontend", environment_id="fixture-environment"
    )
    configuration = WorkerConfiguration(
        Path(sys.executable), tmp_path / "worker.py", tmp_path / "final0.ckpt", tmp_path, model
    )
    adapter, probe = _Adapter(), _Probe()
    monkeypatch.setattr(bridge, "load_worker_configuration", lambda *_args: configuration)
    monkeypatch.setattr(bridge, "BeatWorkerAdapter", lambda *_args: adapter)
    identity = AnalysisIdentity(3, 42, "original-source-fixture", 9)
    arguments: tuple[object, ...] = (
        str(pcm),
        48000,
        48000,
        "b" * 64,
        json.dumps(asdict(identity)),
        str(tmp_path / "installed"),
        probe,
    )
    return arguments, adapter, probe, configuration


def _run(arguments: tuple[object, ...]) -> dict[str, object]:
    with ThreadPoolExecutor(max_workers=1) as executor:
        encoded = executor.submit(bridge.run_beat_this, *arguments).result(timeout=5)
    return json.loads(encoded)


@pytest.mark.parametrize("cancelled", [False, True])
def test_cuda_bridge_resolves_once_echoes_exact_request_and_never_publishes_cancelled_receipt(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, WorkerConfiguration],
    monkeypatch: pytest.MonkeyPatch,
    *,
    cancelled: bool,
) -> None:
    arguments, old_adapter, probe, configuration = request_fixture
    model = CudaBeatModelIdentity("a" * 64, "beat-this-1.1.0-soxr-hq-logmel-v1")
    configuration = replace(configuration, model=model)
    resolves = []

    def resolve(*_args):
        resolves.append(1)
        return configuration

    monkeypatch.setattr(bridge, "load_worker_configuration", resolve)
    monkeypatch.setattr(bridge, "require_cuda_native_capability", lambda: None)

    class Adapter:
        retired = Event()
        retired.set()

        def run_cuda(self, request, _cancel):
            probe.cancelled = cancelled
            execution = ExecutionReceipt(
                1, "auto", "cuda:0", "Fixture GPU", "float32", "2.8.0+cu128", "12.8", None
            )
            return CudaBeatComponentResult(
                request.identity,
                request.model,
                "ready",
                "ready",
                old_adapter.predictions,
                execution=execution,
            )

    monkeypatch.setattr(bridge, "BeatWorkerAdapter", lambda *_args: Adapter())
    result = _run(arguments)
    assert resolves == [1]
    assert result["schema_version"] == 2
    request = json.loads(result["request_json"])
    assert request["identity"] == json.loads(str(arguments[4]))
    assert request["pcm"]["path"] == arguments[0]
    assert request["pcm"]["sample_rate_hz"] == arguments[1]
    assert request["pcm"]["frame_count"] == arguments[2]
    assert request["model"] == asdict(model)
    assert request["execution_policy"] == "auto"
    if cancelled:
        assert result["status"] == "cancelled"
        assert result["predictions"] is result["execution"] is None
    else:
        assert result["status"] == "ready"
        assert result["execution"]["device"] == "cuda:0"


def test_bridge_retains_all_four_arrays_and_actual_input_without_frontend_fiction(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, WorkerConfiguration],
) -> None:
    arguments, adapter, _, configuration = request_fixture
    response = _run(arguments)
    assert response["status"] == "ready"
    assert response["resources_released"] is True
    assert response["model"] == asdict(configuration.model)
    assert response["identity"] == json.loads(str(arguments[4]))
    assert response["input"] == {
        "sample_rate_hz": 48000,
        "frame_count": 48000,
        "source_zero_frame": 0,
        "mono_sha256": "b" * 64,
        "dtype": "float32-le",
    }
    packed = response["predictions"]
    assert isinstance(packed, dict)
    assert set(packed) == {"encoding", *bridge._ARRAY_NAMES}
    assert packed["encoding"] == "float64-le/base64"
    for name in bridge._ARRAY_NAMES:
        original = getattr(adapter.predictions, name)
        assert base64.b64decode(packed[name], validate=True) == struct.pack(
            f"<{len(original)}d", *original
        )
    assert adapter.requests[0].pcm.path.read_bytes() == bytes(48000 * 4)


def test_main_thread_refuses_before_installation_or_adapter_activity(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, WorkerConfiguration],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    arguments, adapter, _, _ = request_fixture
    monkeypatch.setattr(
        bridge, "load_worker_configuration", lambda *_args: pytest.fail("main-thread I/O")
    )
    with pytest.raises(RuntimeError, match="background worker"):
        bridge.run_beat_this(*arguments)
    assert not adapter.requests


def test_missing_worker_is_unavailable_without_adapter_or_fallback(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, WorkerConfiguration],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    arguments, adapter, _, _ = request_fixture

    def missing(*_args: object) -> WorkerConfiguration:
        message = "explicit worker installation is absent"
        raise FileNotFoundError(message)

    monkeypatch.setattr(bridge, "load_worker_configuration", missing)
    response = _run(arguments)
    assert response["status"] == "unavailable"
    assert "installation is absent" in str(response["reason"])
    assert response["model"] is None
    assert response["predictions"] is None
    assert not adapter.requests


def test_native_cancellation_before_configuration_does_not_start_work(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, WorkerConfiguration],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    arguments, adapter, probe, _ = request_fixture
    probe.cancelled = True
    monkeypatch.setattr(
        bridge, "load_worker_configuration", lambda *_args: pytest.fail("cancelled I/O")
    )
    response = _run(arguments)
    assert response["status"] == "cancelled"
    assert response["predictions"] is None
    assert not adapter.requests


def test_existing_event_wait_observes_read_only_native_probe() -> None:
    probe = _Probe()
    event = bridge._NativeCancellationEvent(probe)
    assert not event.wait(0.001)
    probe.cancelled = True
    assert event.is_set()
    assert event.wait(0)


@pytest.mark.parametrize("malformed", ["retiring", "identity", "exception"])
def test_retiring_process_blocks_return_even_after_failure_or_invalid_result(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, WorkerConfiguration],
    monkeypatch: pytest.MonkeyPatch,
    malformed: str,
) -> None:
    arguments, adapter, probe, _ = request_fixture
    entered = Event()

    def retiring(request: BeatWorkerRequest, _cancel: Event) -> BeatComponentResult:
        adapter.retired.clear()
        entered.set()
        if malformed == "exception":
            message = "invalid response while a reader is still retiring"
            raise ValueError(message)
        if malformed == "identity":
            return BeatComponentResult(
                replace(request.identity, request_id=99),
                request.model,
                "failed",
                "invalid",
                resources_released=False,
            )
        return BeatComponentResult(
            request.identity, request.model, "failed", "worker_retiring", resources_released=False
        )

    monkeypatch.setattr(adapter, "run", retiring)
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(bridge.run_beat_this, *arguments)
        try:
            assert entered.wait(2)
            assert not future.done()
            assert Path(str(arguments[0])).is_file()
            probe.cancelled = True
        finally:
            adapter.retired.set()
        response = json.loads(future.result(timeout=5))
    assert response["status"] == "cancelled"
    assert response["resources_released"] is True
    assert response["predictions"] is None


def test_bridge_oversize_fails_without_cropping_or_changing_legacy_cap(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, WorkerConfiguration],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    arguments, adapter, _, _ = request_fixture
    monkeypatch.setattr(bridge, "MAX_NEURAL_WORKER_BYTES", 1024)
    adapter.predictions = BeatPredictions((0.125,), (), tuple([0.1] * 300), tuple([0.2] * 300))
    response = _run(arguments)
    assert response["status"] == "failed"
    assert response["reason"] == "neural_worker_response_limit"
    assert response["predictions"] is None
    assert MAX_ENVELOPE_BYTES == 1024 * 1024
