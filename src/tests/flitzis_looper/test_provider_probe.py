"""Device observation tests use declared metadata, never initialize CUDA or infer audio."""

from types import SimpleNamespace
from unittest.mock import Mock

import pytest
import torch

from flitzis_looper import provider_probe


@pytest.mark.parametrize("device", ["cuda:1", "cuda"])
def test_completed_observation_uses_exact_executed_ordinal(
    monkeypatch: pytest.MonkeyPatch, device: str
) -> None:
    current = Mock(return_value=7)
    identity = Mock(return_value={"identity": "3" * 64, "free_bytes": 123})
    peak = Mock(return_value=456)
    monkeypatch.setattr(torch.cuda, "current_device", current)
    monkeypatch.setattr(provider_probe, "_device_identity", identity)
    monkeypatch.setattr(torch.cuda, "max_memory_reserved", peak)
    assert provider_probe.completed_cuda_observation(device) == ("3" * 64, 456)
    index = 1 if device == "cuda:1" else 7
    identity.assert_called_once_with(index)
    peak.assert_called_once_with(index)
    if device == "cuda:1":
        current.assert_not_called()


def test_cpu_completion_never_queries_cuda(monkeypatch: pytest.MonkeyPatch) -> None:
    forbidden = Mock(side_effect=AssertionError("CPU queried CUDA"))
    monkeypatch.setattr(torch.cuda, "current_device", forbidden)
    monkeypatch.setattr(provider_probe, "_device_identity", forbidden)
    monkeypatch.setattr(torch.cuda, "max_memory_reserved", forbidden)
    assert provider_probe.completed_cuda_observation("cpu") == (None, None)
    forbidden.assert_not_called()


@pytest.mark.parametrize("error", [OSError, ValueError, RuntimeError, AttributeError])
def test_unavailable_device_metadata_preserves_unknown_completion(
    monkeypatch: pytest.MonkeyPatch, error: type[Exception]
) -> None:
    monkeypatch.setattr(provider_probe, "_device_identity", Mock(side_effect=error))
    assert provider_probe.completed_cuda_observation("cuda:1") == (None, None)


@pytest.mark.parametrize(
    ("devices", "buffers"),
    [
        ([], []),
        (["cpu"], []),
        (["cuda:1"], []),
        (["cuda:0", "cuda:1"], []),
        (["cuda:1"], ["cpu"]),
        (["cuda:1"], ["cuda:0"]),
        ([], ["cuda:1"]),
    ],
)
def test_model_observation_requires_one_actual_placement(
    monkeypatch: pytest.MonkeyPatch, devices: list[str], buffers: list[str]
) -> None:
    observe = Mock(return_value=("3" * 64, 456))
    monkeypatch.setattr(provider_probe, "completed_cuda_observation", observe)
    model = SimpleNamespace(
        parameters=lambda: iter(SimpleNamespace(device=torch.device(d)) for d in devices),
        buffers=lambda: iter(SimpleNamespace(device=torch.device(d)) for d in buffers),
    )
    result = provider_probe.completed_model_cuda_observation(model)
    placements = set(devices + buffers)
    if len(placements) == 1:
        assert result == ("3" * 64, 456)
        observe.assert_called_once_with(next(iter(placements)))
    else:
        assert result == (None, None)
        observe.assert_not_called()


def test_device_fingerprint_queries_properties_and_memory_on_same_ordinal(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def driver_version(pointer: object) -> int:
        pointer._obj.value = 13000
        return 0

    driver = SimpleNamespace(cuDriverGetVersion=Mock(side_effect=driver_version))
    properties = Mock(
        return_value=SimpleNamespace(uuid="actual-device-1", name="GPU", major=8, minor=9)
    )
    memory = Mock(return_value=(123, 456))
    monkeypatch.setattr(provider_probe.ctypes, "CDLL", Mock(return_value=driver))
    monkeypatch.setattr(provider_probe, "_driver_release", lambda: "580.1")
    monkeypatch.setattr(torch.cuda, "get_device_properties", properties)
    monkeypatch.setattr(torch.cuda, "mem_get_info", memory)
    result = provider_probe._device_identity(1)
    assert len(result["identity"]) == 64
    assert result["free_bytes"] == 123
    properties.assert_called_once_with(1)
    memory.assert_called_once_with(1)
