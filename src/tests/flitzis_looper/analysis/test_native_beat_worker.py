"""Native BeatThis resource admission and settled fallback with synthetic payloads."""

import hashlib
import json
import os
import struct
import sys
from concurrent.futures import ThreadPoolExecutor
from threading import Event
from types import SimpleNamespace
from typing import TYPE_CHECKING, Any
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis import native_beat_worker as native
from flitzis_looper.analysis import process
from flitzis_looper.analysis.artifacts import NATIVE_MODEL
from flitzis_looper.analysis.contracts import MonoPcmInput, WorkerLimits
from flitzis_looper.analysis.execution import NATIVE_CUDA_PACKAGE_SHA
from flitzis_looper.controller.stem_resources import StemResourceGuard

if TYPE_CHECKING:
    from pathlib import Path

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Native BeatThis uses Windows Jobs")

_PCM = bytes(4096)
_PCM_SHA = hashlib.sha256(_PCM).hexdigest()
_INPUT = {
    "schema_version": 1,
    "mono_sha256": _PCM_SHA,
    "frame_count": 1024,
    "sample_rate_hz": 22050,
    "graph_sha256": NATIVE_MODEL.sha256,
}
_CPU_RECEIPT = {
    **_INPUT,
    "ort_version": "1.24.4",
    "ort_api": 24,
    "logit_count": 3,
    "beat_seconds": [],
    "downbeat_seconds": [],
    "provider": "CPUExecutionProvider",
}
_CUDA_FAULT = {
    **_INPUT,
    "cuda": {
        "device_name": "Fixture GPU",
        "driver_api_version": 12080,
        "package_sha256": NATIVE_CUDA_PACKAGE_SHA,
        "provider_sha256": "110929d2552bc775828e7a5c18c5ef9ab90898fc57a57868623fe2fe8c82d494",
    },
    "cuda_error": 702,
}
_OUTPUT_BYTES = 2 * 3 * struct.calcsize("<f") + len(b"cancel")


@pytest.fixture
def native_attempt(tmp_path: Path) -> tuple[native._Attempt, Event, Mock]:
    pressure = Event()
    memory = Mock(
        side_effect=lambda: {
            "available_commit": 1,
            "system_available_commit_bytes": 1,
            "available_physical": 1,
            "low_memory": int(pressure.is_set()),
        }
    )
    pcm = MonoPcmInput(tmp_path / "staged.f32", 22050, 1024)
    pcm.path.write_bytes(_PCM)
    attempt = native._Attempt(
        tmp_path / "native-beat-this-worker.exe",
        tmp_path,
        pcm,
        _PCM_SHA,
        Event(),
        StemResourceGuard(memory),
        WorkerLimits(reap_timeout_seconds=3),
    )
    return attempt, pressure, memory


def _synthetic_command(activation_marker: Path, *, cuda: bool = False) -> list[str]:
    code = f"from pathlib import Path; Path({str(activation_marker)!r}).touch(); "
    if cuda:
        code += f"import sys; print({json.dumps(_CUDA_FAULT)!r}); sys.exit(70)"
    else:
        logits = struct.pack("<3f", 0.0, 0.5, 1.0)
        code += (
            f"Path('beat.f32').write_bytes({logits!r}); "
            f"Path('downbeat.f32').write_bytes({logits!r}); "
            f"print({json.dumps(_CPU_RECEIPT)!r})"
        )
    return [sys.executable, "-I", "-c", code]


@pytest.mark.parametrize(
    ("free", "late_pressure", "refusal"),
    [
        (_OUTPUT_BYTES - 1, False, "resource_disk_full"),
        (_OUTPUT_BYTES, True, "resource_pressure"),
        (_OUTPUT_BYTES, False, None),
    ],
)
def test_native_checks_concrete_output_space_and_rechecks_while_suspended(
    native_attempt: tuple[native._Attempt, Event, Mock],
    monkeypatch: pytest.MonkeyPatch,
    free: int,
    *,
    late_pressure: bool,
    refusal: str | None,
) -> None:
    attempt, pressure, memory = native_attempt
    marker = attempt.scratch / "activated"
    queried: list[Path] = []
    retirements: list[Event] = []

    def disk_usage(path: Path) -> SimpleNamespace:
        assert path.is_dir()
        assert path.parent == attempt.scratch
        queried.append(path)
        return SimpleNamespace(free=free)

    def launch(_arguments: list[str], **kwargs: Any) -> process.OwnedCommandResult:
        assert kwargs.get("commit_limit_bytes") is None
        assert kwargs["strict_completion"]
        retirements.append(kwargs["retired"])
        if late_pressure:
            pressure.set()
        return process.run_owned_command(_synthetic_command(marker), **kwargs)

    monkeypatch.setattr(native.shutil, "disk_usage", disk_usage)
    monkeypatch.setattr(native, "run_owned_command", launch)
    if refusal is None:
        outcome = attempt.run(cuda=False)
        assert not isinstance(outcome, native._NativeCudaFault)
        assert outcome[0].beat_logits == (0.0, 0.5, 1.0)
    else:
        with pytest.raises(RuntimeError, match=refusal):
            attempt.run(cuda=False)
    assert queried
    assert all(not output.exists() for output in queried)
    if free < _OUTPUT_BYTES:
        assert not retirements
    else:
        (retired,) = retirements
        assert retired.is_set()
        assert memory.call_count >= 2
    assert marker.exists() is (refusal is None)


@pytest.mark.parametrize("cpu_pressure", [False, True])
def test_native_auto_rechecks_cpu_only_after_cuda_reader_and_scratch_end(
    native_attempt: tuple[native._Attempt, Event, Mock],
    monkeypatch: pytest.MonkeyPatch,
    *,
    cpu_pressure: bool,
) -> None:
    attempt, pressure, memory = native_attempt
    stderr_drained, release_stderr = Event(), Event()
    attempts: list[tuple[Path, Event]] = []
    cuda_samples: list[int] = []
    read_real = process._CommandReader._read

    def read(reader: process._CommandReader) -> None:
        read_real(reader)
        if not reader.bounded and len(attempts) == 1:
            stderr_drained.set()
            assert release_stderr.wait(10)

    def launch(arguments: list[str], **kwargs: Any) -> process.OwnedCommandResult:
        output, retired = kwargs["cwd"], kwargs["retired"]
        assert kwargs.get("commit_limit_bytes") is None
        cuda = arguments[1] == "--cuda"
        assert arguments[2 if cuda else 1 :] == [
            str(attempt.pcm.path),
            "22050",
            "1024",
            _PCM_SHA,
            str(output),
        ]
        if not cuda:
            ((cuda_output, cuda_retired),) = attempts
            assert cuda_retired.is_set()
            assert not cuda_output.exists()
        attempts.append((output, retired))
        result = process.run_owned_command(
            _synthetic_command(attempt.scratch / "activated", cuda=cuda), **kwargs
        )
        if cuda:
            cuda_samples.append(memory.call_count)
            if cpu_pressure:
                pressure.set()
        return result

    monkeypatch.setattr(process._CommandReader, "_read", read)
    monkeypatch.setattr(native, "run_owned_command", launch)
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(native._run_selected, attempt, "auto")
        try:
            assert stderr_drained.wait(10)
            ((cuda_output, cuda_retired),) = attempts
            assert cuda_output.is_dir()
            assert not cuda_retired.is_set()
        finally:
            release_stderr.set()
        if cpu_pressure:
            with pytest.raises(RuntimeError, match="resource_pressure"):
                future.result(timeout=10)
        else:
            predictions, receipt = future.result(timeout=10)
            assert predictions.beat_logits == (0.0, 0.5, 1.0)
            assert receipt is not None
            assert (receipt.device, receipt.fallback_reason, receipt.cuda_error) == (
                "cpu",
                "cuda_device_error",
                702,
            )
    assert memory.call_count > cuda_samples[0]
    assert len(attempts) == (1 if cpu_pressure else 2)
    assert all(retired.is_set() and not output.exists() for output, retired in attempts)
