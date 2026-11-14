"""Native-worker bridge contracts; protocol doubles do not prove model quality."""

import base64
import json
import struct
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis import neural_worker as bridge
from flitzis_looper.analysis.artifacts import NATIVE_MODEL
from flitzis_looper.analysis.contracts import AnalysisIdentity, BeatPredictions
from flitzis_looper.analysis.native_beat_worker import NativeWorkerConfiguration
from flitzis_looper.analysis.publication import MAX_ENVELOPE_BYTES

if TYPE_CHECKING:
    from threading import Event

    from flitzis_looper.analysis.contracts import MonoPcmInput


class _Probe:
    def __init__(self) -> None:
        self.cancelled = False

    def is_cancelled(self) -> bool:
        return self.cancelled


class _Adapter:
    def __init__(self) -> None:
        self.requests: list[MonoPcmInput] = []
        self.predictions = BeatPredictions(
            (0.125, 0.5000000000000001, 0.875),
            (0.125,),
            (-0.0, 0.12500000000000003, 0.8),
            (0.0, -2.5, 0.9),
        )

    def run(
        self,
        configuration: NativeWorkerConfiguration,
        pcm: MonoPcmInput,
        mono_sha: str,
        cancel: Event,
    ) -> tuple[BeatPredictions, None]:
        assert configuration.execution_policy == "cpu"
        assert mono_sha == "b" * 64
        assert not cancel.is_set()
        self.requests.append(pcm)
        return self.predictions, None


@pytest.fixture
def request_fixture(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[tuple[object, ...], _Adapter, _Probe, NativeWorkerConfiguration]:
    pcm = tmp_path / "native-owned-mono.f32le"
    pcm.write_bytes(bytes(48000 * 4))
    configuration = NativeWorkerConfiguration(
        tmp_path / "installed/manifest.json", tmp_path, "a" * 64
    )
    adapter, probe = _Adapter(), _Probe()
    load = Mock(return_value=configuration)
    monkeypatch.setattr(bridge, "load_worker_configuration", load)
    monkeypatch.setattr(bridge, "run_native_worker", adapter.run)
    monkeypatch.setattr(bridge, "require_native_worker_capability", lambda _configuration: None)
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


def test_bridge_retains_all_four_arrays_and_actual_input_without_frontend_fiction(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, NativeWorkerConfiguration],
) -> None:
    arguments, adapter, _, _ = request_fixture
    response = _run(arguments)
    assert response["status"] == "ready"
    assert response["resources_released"] is True
    assert response["schema_version"] == 3
    assert response["model"] == asdict(NATIVE_MODEL)
    bridge.load_worker_configuration.assert_called_once_with(
        Path(str(arguments[5])), Path(str(arguments[0])).parent, native_only=True
    )
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
    assert adapter.requests[0].path.read_bytes() == bytes(48000 * 4)


def test_main_thread_refuses_before_installation_or_adapter_activity(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, NativeWorkerConfiguration],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    arguments, adapter, _, _ = request_fixture
    monkeypatch.setattr(
        bridge,
        "load_worker_configuration",
        lambda *_args, **_kwargs: pytest.fail("main-thread I/O"),
    )
    with pytest.raises(RuntimeError, match="background worker"):
        bridge.run_beat_this(*arguments)
    assert not adapter.requests


def test_missing_worker_is_unavailable_without_adapter_or_fallback(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, NativeWorkerConfiguration],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    arguments, adapter, _, _ = request_fixture

    def missing(*_args: object, **_kwargs: object) -> NativeWorkerConfiguration:
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
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, NativeWorkerConfiguration],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    arguments, adapter, probe, _ = request_fixture
    probe.cancelled = True
    monkeypatch.setattr(
        bridge, "load_worker_configuration", lambda *_args, **_kwargs: pytest.fail("cancelled I/O")
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


def test_bridge_oversize_fails_without_cropping_or_changing_legacy_cap(
    request_fixture: tuple[tuple[object, ...], _Adapter, _Probe, NativeWorkerConfiguration],
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
