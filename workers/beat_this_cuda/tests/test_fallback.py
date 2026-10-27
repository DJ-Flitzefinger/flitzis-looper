"""Bounded failure injection; these tests require no CUDA device or checkpoint."""

import weakref
from pathlib import Path

import numpy as np
import pytest
import torch

import worker


def _fail(message):
    raise RuntimeError(message)


class _Input:
    def __init__(self, harness, device="cpu"):
        self.harness = harness
        self.device = device

    def to(self, *, device, dtype):
        placed = _Input(self.harness, device)
        if device != "cpu":
            self.harness.references.append(weakref.ref(placed))
        return placed


class _Model:
    def __init__(self, harness):
        self.harness = harness
        self.device = "cpu"

    def to(self, *, device, dtype):
        self.device = device
        if device != "cpu" and self.harness.failure == "placement":
            _fail("injected CUDA placement failure")
        return self

    def eval(self):
        return self


class _Harness:
    def __init__(self, monkeypatch, failure):
        self.failure = failure
        self.references = []
        self.loads = []
        self.progress = []
        monkeypatch.setattr(worker, "load_local_model", self.load)
        monkeypatch.setattr(worker, "read_pcm", lambda _pcm: np.zeros(100))
        monkeypatch.setattr(worker, "signal_to_spectrogram", self.frontend)
        monkeypatch.setattr(worker, "_selected_device", lambda _policy: ("cuda:0", None))
        monkeypatch.setattr(worker, "_placed", lambda *_args: True)
        monkeypatch.setattr(
            worker,
            "_execution",
            lambda _request, device, fallback: {
                "device": device,
                "fallback_reason": fallback,
            },
        )
        monkeypatch.setattr(worker, "_emit_execution", self.emit)
        monkeypatch.setattr(worker, "split_predict_aggregate", self.predict)
        monkeypatch.setattr(worker, "Postprocessor", lambda **_kwargs: self.postprocess)

    def load(self, *_args):
        if self.failure == "checkpoint":
            _fail("injected checkpoint failure")
        if self.loads:
            assert all(reference() is None for reference in self.references)
        model = _Model(self)
        self.loads.append(len(self.loads))
        if len(self.loads) == 1:
            self.references.append(weakref.ref(model))
        return model

    def frontend(self, *_args):
        if self.failure == "frontend":
            _fail("injected frontend failure")
        return _Input(self)

    def emit(self, _request, execution):
        if self.failure == "protocol_output":
            _fail("injected protocol output failure")
        self.progress.append(execution)

    def predict(self, *, model, **_kwargs):
        if model.device != "cpu" and self.failure == "inference":
            _fail("injected CUDA inference failure")
        return {"beat": torch.zeros(5), "downbeat": torch.zeros(5)}

    def postprocess(self, *_args):
        if self.failure == "postprocess":
            _fail("injected postprocess failure")
        return np.array([0.1]), np.array([0.1])

    def run(self):
        return worker.infer(
            {
                "execution_policy": "auto",
                "model": {"sha256": "a" * 64},
                "identity": {},
                "pcm": {"sample_rate_hz": 100, "frame_count": 100},
            },
            Path("unused"),
        )


@pytest.mark.parametrize("failure", ["placement", "inference"])
def test_failed_cuda_objects_retire_before_one_fresh_cpu_model(
    monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    harness = _Harness(monkeypatch, failure)
    result = harness.run()
    assert harness.loads == [0, 1]
    assert result["execution"] == {"device": "cpu", "fallback_reason": f"cuda_{failure}_error"}
    assert [item["device"] for item in harness.progress] == (
        ["cpu"] if failure == "placement" else ["cuda:0", "cpu"]
    )


@pytest.mark.parametrize("failure", ["frontend", "checkpoint", "postprocess", "protocol_output"])
def test_non_cuda_failure_never_triggers_cpu_retry(
    monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    harness = _Harness(monkeypatch, failure)
    monkeypatch.setattr(
        worker,
        "_release_failed_cuda",
        lambda: pytest.fail("independent error triggered CUDA retry"),
    )
    with pytest.raises(RuntimeError, match="injected"):
        harness.run()
    assert len(harness.loads) == (0 if failure in {"frontend", "checkpoint"} else 1)


@pytest.mark.parametrize(
    ("available", "probe_error", "reason"),
    [(False, False, "cuda_unavailable"), (False, True, "cuda_probe_error")],
)
def test_unavailable_and_probe_errors_select_only_auto_cpu(
    monkeypatch: pytest.MonkeyPatch, available, probe_error, reason
) -> None:
    def probe():
        if probe_error:
            _fail("injected probe failure")
        return available

    monkeypatch.setattr(torch.cuda, "is_available", probe)
    assert worker._selected_device("auto") == ("cpu", reason)
    assert worker._selected_device("cpu") == ("cpu", None)
    with pytest.raises(RuntimeError, match=reason):
        worker._selected_device("cuda")
