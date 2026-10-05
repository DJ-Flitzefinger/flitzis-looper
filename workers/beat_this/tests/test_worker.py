"""Deterministic reference-frontend and isolated worker boundary verification."""

import hashlib
import json
import math
import os
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace

import numpy as np
import pytest
import soxr
import torch
from beat_this.inference import Audio2Frames
from beat_this.model.postprocessor import Postprocessor
from beat_this.preprocessing import LogMelSpect

import worker


@pytest.fixture(scope="session", autouse=True)
def bounded_torch_threads() -> None:
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)


@pytest.fixture
def worker_request(tmp_path: Path) -> dict:
    signal = np.zeros(48000, dtype="<f4")
    signal[[0, 47999]] = 1
    path = tmp_path / "mono.f32le"
    signal.tofile(path)
    return {
        "schema_version": 1,
        "identity": {"pad_id": 1, "request_id": 2, "source_id": "fixture", "source_generation": 3},
        "model": {**worker.MODEL_CONFIGURATION, "environment_id": "test", "sha256": "a" * 64},
        "pcm": {
            "path": str(path),
            "sample_rate_hz": 48000,
            "frame_count": len(signal),
            "origin_seconds": 0.0,
            "dtype": "float32-le",
            "channels": 1,
        },
    }


@pytest.mark.parametrize("sample_rate", [22050, 44100, 48000])
@pytest.mark.parametrize("fixture", ["first", "last", "silence", "fractional_tone"])
def test_frontend_matches_upstream_complete_origin_and_tail(sample_rate: int, fixture: str) -> None:
    frames = sample_rate * 2 + 37
    signal = np.zeros(frames, dtype=np.float32)
    if fixture == "first":
        signal[0] = 1
    elif fixture == "last":
        signal[-1] = 1
    elif fixture == "fractional_tone":
        time = np.arange(frames) / sample_rate
        signal = np.asarray(np.sin(2 * np.pi * 437.3 * time) * 0.25, dtype=np.float32)
    expected_signal = (
        signal
        if sample_rate == 22050
        else soxr.resample(signal, in_rate=sample_rate, out_rate=22050)
    )
    actual_signal = worker.prepare_signal(signal, sample_rate)
    np.testing.assert_array_equal(actual_signal, expected_signal)
    assert len(actual_signal) == math.floor(frames * 22050 / sample_rate + 0.5)
    reference = SimpleNamespace(spect=LogMelSpect(device="cpu"), device="cpu")
    expected = Audio2Frames.signal2spect(reference, signal, sample_rate)
    actual = worker.signal_to_spectrogram(signal, sample_rate)
    torch.testing.assert_close(actual, expected, rtol=0, atol=0)
    assert actual.shape == (len(actual_signal) // 441 + 1, 128)
    if fixture == "first":
        assert float(actual[0].sum()) > 0
    elif fixture == "last":
        assert float(actual[-1].sum()) > 0
    elif fixture == "silence":
        assert not torch.count_nonzero(actual)


def test_pcm_is_full_untrimmed_shared_mono(worker_request: dict) -> None:
    worker.validate_request(worker_request, {"environment_id": "test"})
    pcm = worker.read_pcm(worker_request["pcm"])
    assert len(pcm) == 48000
    assert pcm[0] == pcm[-1] == 1
    assert not np.count_nonzero(pcm[1:-1])


@pytest.mark.parametrize("mutation", ["nan", "short", "long"])
def test_pcm_rejects_nonfinite_or_changed_extent(worker_request: dict, mutation: str) -> None:
    pcm = np.zeros(48000, dtype="<f4")
    if mutation == "nan":
        pcm[-1] = np.nan
    elif mutation == "short":
        pcm = pcm[:-1]
    else:
        pcm = np.append(pcm, np.float32(0))
    pcm.tofile(worker_request["pcm"]["path"])
    with pytest.raises(ValueError, match=r"nonfinite_pcm|pcm_size_mismatch"):
        worker.read_pcm(worker_request["pcm"])


@pytest.mark.parametrize("field", ["frontend_id", "environment_id", "device", "precision"])
def test_request_rejects_unaccepted_configuration(worker_request: dict, field: str) -> None:
    worker_request["model"][field] = "wrong"
    with pytest.raises(ValueError, match="model_configuration_mismatch"):
        worker.validate_request(worker_request, {"environment_id": "test"})


@pytest.mark.parametrize("field", ["sample_rate_hz", "frame_count", "channels", "origin_seconds"])
def test_pcm_metadata_rejects_bool_disguised_as_number(worker_request: dict, field: str) -> None:
    worker_request["pcm"][field] = True
    with pytest.raises(ValueError, match=r"pcm_shape_limit|pcm_format_mismatch"):
        worker.validate_request(worker_request, {"environment_id": "test"})


def test_oversize_prediction_work_rejected_before_frontend(worker_request: dict) -> None:
    worker_request["pcm"].update(sample_rate_hz=8000, frame_count=worker.MAX_PCM_BYTES // 4)
    with pytest.raises(ValueError, match="prediction_limit"):
        worker.validate_request(worker_request, {"environment_id": "test"})


def test_short_input_fails_without_alternative_padding() -> None:
    with pytest.raises(ValueError, match="audio_too_short_for_reference_reflect_padding"):
        worker.prepare_signal(np.zeros(512, dtype=np.float32), 22050)


def test_checkpoint_hash_failure_precedes_torch_load(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    checkpoint = tmp_path / "corrupt.ckpt"
    checkpoint.write_bytes(b"not a checkpoint")

    def unexpected_load(*args: object, **kwargs: object) -> None:
        pytest.fail("unverified checkpoint reached torch.load")

    monkeypatch.setattr(torch, "load", unexpected_load)
    with pytest.raises(ValueError, match="checkpoint_hash_mismatch"):
        worker.load_local_model(checkpoint, "0" * 64)


def test_missing_checkpoint_never_calls_upstream_download(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    def unexpected_download(*args: object, **kwargs: object) -> None:
        pytest.fail("missing artifact triggered a network fallback")

    monkeypatch.setattr(torch.hub, "load_state_dict_from_url", unexpected_download)
    with pytest.raises(FileNotFoundError):
        worker.load_local_model(tmp_path / "missing.ckpt", "0" * 64)


def test_all_worker_socket_access_is_denied() -> None:
    project = Path(worker.__file__).resolve().parent
    code = (
        "import socket, worker; worker.configure_runtime(); "
        "socket.socket().connect(('127.0.0.1', 1))"
    )
    result = subprocess.run(
        [sys.executable, "-c", code], cwd=project, capture_output=True, timeout=30, check=False
    )
    assert result.returncode != 0
    assert b"network_disabled_in_beat_worker" in result.stderr


def test_installed_environment_provenance_matches_lock() -> None:
    project = Path(worker.__file__).resolve().parent
    manifest = worker.environment_manifest(project)
    digest = hashlib.sha256((project / "uv.lock").read_bytes()).hexdigest()
    assert manifest["environment_id"] == f"uv-lock-sha256:{digest}"
    assert manifest["frontend_id"] == worker.FRONTEND_ID
    assert manifest["python_version"] == "3.12.13"
    assert manifest["packages"]["beat-this"] == "1.1.0"
    assert manifest["packages"]["torch"] == "2.8.0+cpu"
    assert "pytest" not in manifest["packages"]


def test_installed_environment_rejects_dependency_drift(monkeypatch: pytest.MonkeyPatch) -> None:
    project = Path(worker.__file__).resolve().parent
    monkeypatch.setattr(worker, "version", lambda name: "wrong-version")
    with pytest.raises(ValueError, match="environment_version_mismatch"):
        worker.environment_manifest(project)


def test_oversize_request_read_is_bounded(tmp_path: Path) -> None:
    path = tmp_path / "oversize.json"
    path.write_text(" " * (worker.MAX_REQUEST_BYTES + 1), encoding="utf-8")
    with pytest.raises(ValueError, match="request_limit"):
        worker._bounded_json(path)


def test_environment_probe_has_no_inference_dependency(tmp_path: Path) -> None:
    path = tmp_path / "environment.json"
    result = subprocess.run(
        [sys.executable, "-I", worker.__file__, "--environment-manifest", str(path)],
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr.decode()
    assert result.stdout == b""
    assert json.loads(path.read_text(encoding="utf-8"))["frontend_id"] == worker.FRONTEND_ID


@pytest.mark.skipif(
    not os.environ.get("BEAT_THIS_CHECKPOINT"), reason="explicit local model required"
)
@pytest.mark.parametrize("duration_seconds", [4, 32])
def test_real_final0_matches_upstream_logits_and_minimal_positions(
    worker_request: dict, monkeypatch: pytest.MonkeyPatch, duration_seconds: int
) -> None:
    checkpoint = Path(os.environ["BEAT_THIS_CHECKPOINT"])
    worker_request["model"]["sha256"] = hashlib.sha256(checkpoint.read_bytes()).hexdigest()
    sample_rate = worker_request["pcm"]["sample_rate_hz"]
    signal = np.zeros(sample_rate * duration_seconds + 73, dtype=np.float32)
    signal[:: sample_rate // 2] = 0.8
    signal[-1] = 0.3
    signal.tofile(worker_request["pcm"]["path"])
    worker_request["pcm"]["frame_count"] = len(signal)

    def unexpected_download(*args: object, **kwargs: object) -> None:
        pytest.fail("local reference model attempted to download weights")

    monkeypatch.setattr(torch.hub, "load_state_dict_from_url", unexpected_download)
    actual = worker.infer(worker_request, checkpoint)
    reference = Audio2Frames(str(checkpoint), device="cpu", float16=False)
    beat_logits, downbeat_logits = reference(signal, sample_rate)
    assert len(beat_logits) == duration_seconds * 50 + 1
    beats, downbeats = Postprocessor(type="minimal", fps=50)(beat_logits, downbeat_logits)
    duration = len(signal) / sample_rate
    assert actual["schema_version"] == 1
    assert actual["identity"] == worker_request["identity"]
    assert actual["model"] == worker_request["model"]
    predictions = actual["predictions"]
    np.testing.assert_array_equal(predictions["beat_logits"], beat_logits.numpy())
    np.testing.assert_array_equal(predictions["downbeat_logits"], downbeat_logits.numpy())
    np.testing.assert_array_equal(predictions["beat_seconds"], beats[beats < duration])
    np.testing.assert_array_equal(predictions["downbeat_seconds"], downbeats[downbeats < duration])
