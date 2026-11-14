import hashlib
from concurrent.futures import ThreadPoolExecutor
from contextlib import nullcontext
from dataclasses import replace
from pathlib import Path
from threading import Event
from types import SimpleNamespace
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import beat_cuda_cache as cache
from flitzis_looper.analysis import native_beat_worker
from flitzis_looper.analysis.native_beat_worker import NativeWorkerConfiguration

if TYPE_CHECKING:
    from flitzis_looper.analysis.contracts import MonoPcmInput

_UUID = "GPU-01234567-890a-bcde-f012-3456789abcde"
_RECIPE = [
    (1_452_000, "4cf5ac6939311668adac199b7b57cb9b609ff891c7399d41c74d8410f9235105"),
    (7_776_000, "33bb22811743779d7b3e27cc7b05ceb6213cb41bc084115c82640a98ab48cc0b"),
    (384_000, "1cd89ae39c1e7019297da422c2ac09ce7e531be57f0e0f628a2e5e3b83369512"),
]


@pytest.fixture
def worker(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> NativeWorkerConfiguration:
    scratch = tmp_path / "scratch"
    installed = tmp_path / "beat-this" / "installation-fixture"
    scratch.mkdir()
    installed.mkdir(parents=True)
    monkeypatch.setattr(cache, "heavy_work_scope", lambda _cancel: nullcontext())
    monkeypatch.setattr(
        cache, "cuda_device_identity", lambda: cache.CudaDeviceIdentity(_UUID, "621.24")
    )
    return NativeWorkerConfiguration(installed / "manifest.json", scratch, "a" * 64, "auto")


def test_warmup_reuses_cache_across_owners_and_relocation(
    worker: NativeWorkerConfiguration, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls = []
    started = Event()
    let_complete = Event()

    def run(
        configuration: NativeWorkerConfiguration,
        pcm: MonoPcmInput,
        digest: str,
        _cancel: Event,
        *,
        cuda_cache_path: Path,
        cuda_device_uuid: str,
    ) -> tuple[None, SimpleNamespace]:
        assert configuration.execution_policy == "cuda"
        assert cuda_device_uuid == _UUID
        assert pcm.sample_rate_hz == 48_000
        assert pcm.path.is_relative_to(worker.scratch_dir)
        assert not pcm.path.is_relative_to(cuda_cache_path)
        assert not (cuda_cache_path.parent / "preparation.json").exists()
        with pcm.path.open("rb") as stream:
            assert hashlib.file_digest(stream, "sha256").hexdigest() == digest
        assert pcm.path.stat().st_size == pcm.frame_count * 4
        calls.append((pcm.frame_count, digest))
        started.set()
        assert let_complete.wait(5)
        shard = cuda_cache_path / "a0"
        shard.mkdir(exist_ok=True)
        (shard / "fixture.bin").write_bytes(b"kernel")
        (shard / "second.bin").write_bytes(b"second kernel")
        return None, SimpleNamespace(device="cuda:0", cuda_kernel_events=10)

    monkeypatch.setattr(native_beat_worker, "run_native_worker", run)
    assert cache.cuda_cache_status(worker)["status"] == "pending"
    with ThreadPoolExecutor(max_workers=2) as pool:
        owner = pool.submit(cache.ensure_cuda_cache, worker)
        assert started.wait(5)
        follower = pool.submit(cache.ensure_cuda_cache, worker)
        assert cache.cuda_cache_status(worker)["status"] == "pending"
        let_complete.set()
        first = owner.result(timeout=5)
        second = follower.result(timeout=5)
    assert first["status"] == second["status"] == "ready"
    assert first["warmup_executed"]
    assert not second["warmup_executed"]
    assert first["cache_path"] == second["cache_path"]
    assert calls == _RECIPE
    assert not list(worker.scratch_dir.iterdir())

    original = worker.manifest.parent.parent
    relocated = original.rename(original.with_name("relocated-worker"))
    moved = replace(worker, manifest=relocated / worker.manifest.relative_to(original))
    moved_result = cache.cuda_cache_status(moved)
    assert moved_result["status"] == "ready"
    assert moved_result["key"] == first["key"]
    assert moved_result["cache_path"] != first["cache_path"]
    assert_partial_loss_detected(moved, moved_result)

    monkeypatch.setattr(
        cache, "cuda_device_identity", lambda: cache.CudaDeviceIdentity(_UUID, "622.01")
    )
    changed = cache.cuda_cache_status(moved)
    assert changed["status"] == "pending"
    assert changed["key"] != first["key"]


def assert_partial_loss_detected(
    moved: NativeWorkerConfiguration, result: cache.CudaCacheStatus
) -> None:
    assert result["cache_path"] is not None
    shard = Path(result["cache_path"]) / "a0"
    (shard / "later-analysis.bin").write_bytes(b"additional kernel")
    assert cache.cuda_cache_status(moved)["status"] == "ready"
    (shard / "second.bin").unlink()
    assert cache.cuda_cache_status(moved)["status"] == "pending"


def test_failed_warmup_is_visible_and_only_repair_retries_same_key(
    worker: NativeWorkerConfiguration, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls = 0

    def unavailable(*_args: object, **_kwargs: object) -> tuple[None, None]:
        nonlocal calls
        calls += 1
        return None, None

    monkeypatch.setattr(native_beat_worker, "run_native_worker", unavailable)
    first = cache.ensure_cuda_cache(worker)
    assert first["status"] == "failed"
    assert first["warmup_executed"]
    assert first["cache_path"] is None
    assert "completed CUDA execution receipt" in first["detail"]
    assert not cache.ensure_cuda_cache(worker)["warmup_executed"]
    assert calls == 1
    assert cache.ensure_cuda_cache(worker, retry_failed=True)["warmup_executed"]
    assert calls == 2


def test_cancelled_warmup_cannot_publish_ready(
    worker: NativeWorkerConfiguration, monkeypatch: pytest.MonkeyPatch
) -> None:
    cancel = Event()

    def cancelled(*_args: object, **_kwargs: object) -> tuple[None, None]:
        cancel.set()
        return None, None

    monkeypatch.setattr(native_beat_worker, "run_native_worker", cancelled)
    result = cache.ensure_cuda_cache(worker, cancel)
    assert result["status"] == "pending"
    assert result["warmup_executed"]
    assert result["cache_path"] is None
    assert cache.cuda_cache_status(worker)["status"] == "pending"
    assert not list(worker.scratch_dir.iterdir())


def test_cpu_absence_and_probe_failure_have_different_status(
    worker: NativeWorkerConfiguration, monkeypatch: pytest.MonkeyPatch
) -> None:
    cpu = replace(worker, execution_policy="cpu")
    assert cache.ensure_cuda_cache(cpu)["status"] == "pending"
    monkeypatch.setattr(cache, "cuda_device_identity", lambda: None)
    assert cache.ensure_cuda_cache(cpu)["status"] == "unavailable"

    def probe_error() -> None:
        message = "NVML release query failed"
        raise RuntimeError(message)

    monkeypatch.setattr(cache, "cuda_device_identity", probe_error)
    result = cache.ensure_cuda_cache(cpu)
    assert result["status"] == "failed"
    assert "NVML release query failed" in result["detail"]
