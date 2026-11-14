"""Owned route, same-PCM retry and executed receipt boundaries for BS-RoFormer ONNX."""

import json
import os
from dataclasses import replace
from pathlib import Path
from threading import Event
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis.beat_cuda_cache import CudaDeviceIdentity
from flitzis_looper.analysis.execution import NATIVE_CPU_PACKAGE_SHA, NATIVE_CUDA_PACKAGE_SHA
from flitzis_looper.analysis.process import OwnedCommandResult
from flitzis_looper.bs_onnx_cuda_cache import BsCudaCacheStatus
from flitzis_looper.bs_onnx_installation import (
    BS_ONNX_CUDA_PROVIDER_SHA256,
    BS_ONNX_EXECUTABLE,
    BS_ONNX_MANIFEST_ENV,
    BS_ONNX_MODELS_DIRECTORY,
    BS_ONNX_WEIGHTS_SHA256,
    BsOnnxInstallation,
)
from flitzis_looper.controller import bs_roformer_onnx as adapter
from flitzis_looper.controller import stem_process
from flitzis_looper.controller.stem_cache import (
    expected_stem_files,
    promote_generation_artifacts,
    verified_stem_cache_available,
)
from flitzis_looper.controller.stem_generation import (
    AudioShape,
    StemGenerationError,
    StemGenerationRequest,
    StemGenerationResult,
)
from flitzis_looper.controller.stem_provenance import capture_processing_files
from flitzis_looper.controller.stem_provider import ProviderCalibration
from flitzis_looper.controller.stem_separators import SelectedStemGenerationBackend
from flitzis_looper.inference_provider import CUDA_FAILURE_EXIT, CUDA_FAILURE_RECEIPT
from flitzis_looper.models import STEM_KINDS, StemCacheEntry
from flitzis_looper.stem_cancellation import CANCEL_PATH_ENV

_UUID = "GPU-12345678-1234-1234-1234-123456789abc"
_DEVICE = CudaDeviceIdentity(_UUID, "621.24")
_CPU_GRAPH = "1" * 64
_CUDA_GRAPH = "2" * 64
_PCM = "3" * 64


def _installation(tmp_path: Path) -> BsOnnxInstallation:
    return BsOnnxInstallation(
        tmp_path / "manifest.json",
        executable_sha256="4" * 64,
        graph_sha256=_CPU_GRAPH,
        cuda_graph_sha256=_CUDA_GRAPH,
        crt_files=(),
    )


def _request(tmp_path: Path, *, cpu: bool = False) -> StemGenerationRequest:
    return StemGenerationRequest(
        0,
        tmp_path / "input.wav",
        "test-source",
        tmp_path / "artifacts",
        AudioShape(48_000, 2, 900),
        tmp_path / "models",
        device_policy="cpu" if cpu else "auto",
        separator="bs-roformer:musdb18hq",
        bs_onnx_manifest=tmp_path / "manifest.json",
    )


def _receipt(device: str, *, kernels: int = 2) -> bytes:
    cuda = device == "cuda"
    return json.dumps({
        "schema_version": 1,
        "mode": "track",
        "device": device,
        "provider": "CUDAExecutionProvider" if cuda else "CPUExecutionProvider",
        "frames": 900,
        "sample_rate_hz": 44_100,
        "channels": 2,
        "pcm_sha256": _PCM,
        "graph_sha256": _CUDA_GRAPH if cuda else _CPU_GRAPH,
        "weights_sha256": BS_ONNX_WEIGHTS_SHA256,
        "ort_version": "1.24.4",
        "ort_api": 24,
        "fft_profile": adapter._FFT_PROFILES[device],
        "ort_package_sha256": NATIVE_CUDA_PACKAGE_SHA if cuda else NATIVE_CPU_PACKAGE_SHA,
        "outputs": dict.fromkeys(("drums", "bass", "other", "vocals"), "0" * 64),
        "cuda_kernel_events": kernels if cuda else 0,
        "cuda": {
            "device_uuid": _UUID,
            "device_name": "fixture GPU",
            "driver_api_version": 13000,
            "package_sha256": NATIVE_CUDA_PACKAGE_SHA,
            "provider_sha256": BS_ONNX_CUDA_PROVIDER_SHA256,
        }
        if cuda
        else None,
    }).encode()


def test_selected_onnx_request_cannot_acquire_torch_lineage(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    request = _request(tmp_path)
    assert capture_processing_files(request) is None
    generate = Mock()
    monkeypatch.setattr(adapter.BsOnnxStemGenerationBackend, "generate", generate)
    progress = Mock()
    assert SelectedStemGenerationBackend().generate(request, progress) is generate.return_value
    generate.assert_called_once_with(request, progress)


def test_cpu_selection_skips_cuda_probe(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    probe = Mock(side_effect=AssertionError("CPU selected CUDA"))
    monkeypatch.setattr(adapter, "cuda_device_identity", probe)
    run = Mock(return_value=OwnedCommandResult("ok", 0, b"fixture receipt", b""))
    monkeypatch.setattr(adapter, "_run_worker", run)
    finish = Mock()
    backend = adapter.BsOnnxStemGenerationBackend()
    monkeypatch.setattr(backend, "_finish", finish)
    source = adapter._SourcePcm(
        tmp_path / "source.f32", 900, _PCM, guard=None, decoder_verified=False
    )
    assert backend._generate(_request(tmp_path, cpu=True), _installation(tmp_path), source, Mock())
    assert run.call_args.args[-2:] == (None, None)
    probe.assert_not_called()


@pytest.mark.parametrize(
    ("reason", "marker", "qualified"),
    [
        ("worker_crashed", b"unclassified crash", False),
        ("resource_pressure", CUDA_FAILURE_RECEIPT.encode(), False),
        ("worker_crashed", CUDA_FAILURE_RECEIPT.encode(), True),
    ],
)
def test_cuda_retry_waits_for_attempt_cleanup_and_uses_same_sealed_pcm(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    reason: str,
    marker: bytes,
    *,
    qualified: bool,
) -> None:
    monkeypatch.setattr(adapter, "cuda_device_identity", lambda: _DEVICE)
    backend = adapter.BsOnnxStemGenerationBackend()
    sealed = adapter._SourcePcm(
        tmp_path / "input.f32", 900, _PCM, guard=None, decoder_verified=False
    )
    paths: list[Path] = []

    def run(
        _installation: BsOnnxInstallation,
        source: adapter._SourcePcm,
        output: Path,
        device: CudaDeviceIdentity | None,
        cache_path: Path | None,
    ) -> OwnedCommandResult:
        assert source is sealed
        assert cache_path is None
        if not paths:
            assert device is _DEVICE
            (output / "leftover").touch()
            paths.append(output)
            return OwnedCommandResult(reason, CUDA_FAILURE_EXIT, marker, b"fixture error")
        assert device is None
        assert not paths[0].exists()
        paths.append(output)
        return OwnedCommandResult("ok", 0, b"fixture receipt", b"")

    finish = Mock()
    monkeypatch.setattr(adapter, "_run_worker", run)
    monkeypatch.setattr(backend, "_finish", finish)
    request, installation = _request(tmp_path), _installation(tmp_path)
    if qualified:
        assert backend._generate(request, installation, sealed, Mock()) is finish.return_value
        assert "fixture error" in finish.call_args.args[-1].cuda_error
        assert len(paths) == 2
        assert not paths[1].exists()
    else:
        with pytest.raises(StemGenerationError, match="fixture error"):
            backend._generate(request, installation, sealed, Mock())
        assert len(paths) == 1
        assert not paths[0].exists()
        finish.assert_not_called()


def test_source_pointer_is_optional_but_bad_selection_cannot_fall_back(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.delenv(BS_ONNX_MANIFEST_ENV, raising=False)
    assert adapter.resolve_bs_onnx_manifest(tmp_path) is None
    package = tmp_path / BS_ONNX_MODELS_DIRECTORY
    package.mkdir(parents=True)
    (package / "current.json").write_text("{}", encoding="ascii")
    with pytest.raises(ValueError, match="Invalid BS-RoFormer worker pointer"):
        adapter.resolve_bs_onnx_manifest(tmp_path)
    explicit = tmp_path / "other" / "manifest.json"
    monkeypatch.setenv(BS_ONNX_MANIFEST_ENV, str(explicit))
    assert adapter.resolve_bs_onnx_manifest(tmp_path) == explicit


def test_ready_and_pending_cache_use_original_stem_job(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    installation = _installation(tmp_path)
    kernels = tmp_path / "cache"
    status = BsCudaCacheStatus("ready", "ready", kernels)
    lookup = Mock(return_value=status)
    ensure = Mock(return_value=status)
    monkeypatch.setattr(adapter, "bs_cuda_cache_status", lookup)
    monkeypatch.setattr(adapter, "ensure_bs_cuda_cache", ensure)
    progress = Mock()
    cancel = Event()

    with stem_process.stem_command_scope(cancel):
        assert adapter._cuda_cache(installation, _DEVICE, progress) is status
        ensure.assert_not_called()
        lookup.return_value = BsCudaCacheStatus("pending", "needs work")
        assert adapter._cuda_cache(installation, _DEVICE, progress) is status
    ensure.assert_called_once_with(
        installation,
        _DEVICE,
        cancel,
        runner=adapter._run_cache_warmup,
        checkpoint=stem_process.stem_command_checkpoint,
    )


def test_deep_stem_and_cache_jobs_use_short_system_cwd(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    system_root = tmp_path
    scratch = tmp_path / ("x" * 120) / ("y" * 120) / "output"
    assert len(str(scratch)) > 260
    installation = _installation(scratch / "installation")
    source = adapter._SourcePcm(
        scratch / "source.f32", 900, _PCM, guard=None, decoder_verified=False
    )
    run = Mock()
    monkeypatch.setattr(adapter, "run_stem_command", run)
    monkeypatch.setenv("SYSTEMROOT", str(system_root))

    adapter._run_worker(installation, source, scratch, None, None)
    command = run.call_args
    assert command.kwargs["cwd"] == system_root
    assert command.kwargs["env"]["TEMP"] == str(scratch)
    assert command.kwargs["native_bs_onnx"]
    assert command.args[0][0] == str(installation.executable)
    assert str(source.path) in command.args[0]
    assert str(scratch) in command.args[0]

    args = [str(installation.executable), "--cuda", "track", str(source.path)]
    env = {"SystemRoot": str(system_root), "TEMP": str(scratch)}
    adapter._run_cache_warmup(args, scratch, env)
    run.assert_called_with(args, cwd=system_root, env=env, native_bs_onnx=True)


@pytest.mark.skipif(os.name != "nt", reason="Windows extended stem-cache paths")
def test_deep_generation_promotes_and_restores_relative_cache(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = tmp_path / ("x" * 55) / ("y" * 55)
    root.mkdir(parents=True)
    monkeypatch.chdir(root)
    original_mkdir = Path.mkdir
    ordinary_stat = Path.stat

    def reject_long_dos_path(path: Path, *, follow_symlinks: bool = True) -> os.stat_result:
        if len(str(path)) > 260 and not str(path).startswith("\\\\?\\"):
            raise FileNotFoundError(str(path))
        return ordinary_stat(path, follow_symlinks=follow_symlinks)

    cache_dir = "samples/stems/#1"
    generation = root / cache_dir / (".generation-" + "a" * 32)
    generation_io = stem_process.stem_cache_io_path(generation)
    original_mkdir(generation_io, parents=True)
    for kind in STEM_KINDS:
        (generation_io / f"{kind}.wav").write_bytes(kind.encode("ascii"))
    assert len(str(generation / "instrumental.wav")) > 260
    monkeypatch.setattr(Path, "stat", reject_long_dos_path)
    entry = StemCacheEntry(
        source_version="fixture", cache_dir=cache_dir, stems=expected_stem_files(cache_dir)
    )
    promoted = promote_generation_artifacts(entry, generation)
    assert promoted.cache_dir == f"{cache_dir}/.ready-{'a' * 32}"
    assert verified_stem_cache_available(promoted)
    assert not generation_io.exists()


def test_cuda_success_requires_matching_actual_device_and_compute() -> None:
    cuda = adapter._RECEIPT.validate_json(_receipt("cuda"))
    cpu = adapter._RECEIPT.validate_json(_receipt("cpu"))
    assert adapter._execution(cuda, _DEVICE, cpu_fallback=False).cuda_device_identity
    assert adapter._execution(cpu, None, cpu_fallback=True).cpu_fallback
    with pytest.raises(StemGenerationError, match="provider receipt"):
        adapter._execution(cuda, None, cpu_fallback=False)
    with pytest.raises(StemGenerationError, match="provider receipt"):
        adapter._execution(
            adapter._RECEIPT.validate_json(_receipt("cuda", kernels=0)),
            _DEVICE,
            cpu_fallback=False,
        )


def test_onnx_host_identity_rejects_replaced_alignment(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = adapter._onnx_host_paths(_request(tmp_path))
    assert adapter.loaded_code_identity(paths) is not None
    monkeypatch.setattr(adapter, "write_project_cache_artifacts", lambda **_kwargs: None)
    assert adapter.loaded_code_identity(paths) is None


def test_track_receipt_requires_exact_fft_profile() -> None:
    raw = json.loads(_receipt("cpu"))
    raw.pop("fft_profile")
    with pytest.raises(ValueError, match="fft_profile"):
        adapter._RECEIPT.validate_json(json.dumps(raw))
    raw["fft_profile"] = adapter._FFT_PROFILES["cuda"]
    receipt = adapter._RECEIPT.validate_json(json.dumps(raw))
    with pytest.raises(StemGenerationError, match="FFT profile"):
        adapter._execution(receipt, None, cpu_fallback=False)


def test_post_execution_producer_requires_decoder_and_exact_profile(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(adapter, "native_conversion_identity", lambda: ("conversion", "c" * 64))
    source = adapter._SourcePcm(
        tmp_path / "input.f32",
        900,
        _PCM,
        guard=None,
        decoder_verified=True,
        decoder_witnesses=(("a" * 64, 123),),
    )
    guard = Mock()
    guard.witnesses.return_value = [("fixture", "b" * 64, 100)]
    cpu = adapter._RECEIPT.validate_json(_receipt("cpu"))
    execution = adapter._execution(cpu, None, cpu_fallback=False)
    assert adapter._producer_identity(execution, cpu, source, (guard,), None) is None
    cpu = replace(cpu, fft_profile=adapter._FFT_PROFILES["cpu"])
    producer = adapter._producer_identity(execution, cpu, source, (guard,), "d" * 64)
    assert producer is not None
    cuda = replace(
        adapter._RECEIPT.validate_json(_receipt("cuda")), fft_profile=adapter._FFT_PROFILES["cuda"]
    )
    gpu = adapter._execution(cuda, _DEVICE, cpu_fallback=False)
    assert adapter._producer_identity(gpu, cuda, source, (guard,), "d" * 64) != producer
    source.decoder_verified = False
    assert adapter._producer_identity(execution, cpu, source, (guard,), "d" * 64) is None


def test_cuda_arena_peak_excludes_diagnostics_and_unproven_reservation() -> None:
    payload = json.loads(_receipt("cuda"))
    payload["fft_profile"] = adapter._FFT_PROFILES["cuda"]
    payload["cuda_memory"] = {
        "kind": "dxgi_wddm_local_sampled",
        "observed_peak_bytes": 20,
        "minimum_budget_bytes": 30,
        "first_usage_bytes": 0,
        "last_usage_bytes": 4,
        "samples": 2,
    }
    receipt = adapter._RECEIPT.validate_json(json.dumps(payload))
    assert receipt.cuda_memory is not None
    assert adapter._execution(receipt, _DEVICE, cpu_fallback=False).cuda_peak_bytes is None
    arena = {"kind": "ort-cuda-device-arena", "max_in_use_bytes": 15, "peak_reserved_bytes": 14}
    payload["cuda_arena"] = arena
    receipt = adapter._RECEIPT.validate_json(json.dumps(payload))
    assert adapter._execution(receipt, _DEVICE, cpu_fallback=False).cuda_peak_bytes is None
    arena["peak_reserved_bytes"] = 16
    receipt = adapter._RECEIPT.validate_json(json.dumps(payload))
    assert adapter._execution(receipt, _DEVICE, cpu_fallback=False).cuda_peak_bytes == 16


def test_verified_cost_family_shares_cpu_cuda_policy_without_asset_identity(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    backend = adapter.BsOnnxStemGenerationBackend()
    request = replace(
        _request(tmp_path), source_version="source|sha256-v1:" + "f" * 64, original_bytes=400
    )
    family = "7" * 64
    plan_request = replace(request, producer_identity=family)
    free = Mock(return_value=2 << 30)
    monkeypatch.setattr(adapter, "nvml_free_bytes", free)
    monkeypatch.setattr(adapter, "cuda_device_identity", lambda: _DEVICE)
    installation = _installation(tmp_path)
    cuda_plan, _ = backend._plan(plan_request, installation)
    assert cuda_plan.key is not None
    free.assert_called_once_with(_UUID)
    payload = json.loads(_receipt("cuda"))
    payload["fft_profile"] = adapter._FFT_PROFILES["cuda"]
    payload["cuda_arena"] = {
        "kind": "ort-cuda-device-arena",
        "max_in_use_bytes": 1 << 30,
        "peak_reserved_bytes": 1 << 30,
    }
    gpu = adapter._execution(
        adapter._RECEIPT.validate_json(json.dumps(payload)), _DEVICE, cpu_fallback=False
    )
    accepted = StemGenerationResult(
        backend_name="bs-roformer",
        model_name=adapter.MODEL_NAME,
        device="cuda",
        cpu_fallback=False,
        artifact_count=5,
        decoder_verified=True,
        execution=gpu,
        executed_producer_identity="8" * 64,
        calibration_family=family,
        provider_calibration=ProviderCalibration(
            cuda_plan, 10.0, verified=True, observed_at=adapter.time.monotonic()
        ),
    )
    backend.accept_calibration(request, replace(accepted, calibration_family="9" * 64))
    free.return_value = 1 << 29
    assert backend._plan(plan_request, installation)[0].devices == ("cuda", "cpu")
    backend.accept_calibration(request, accepted)
    assert backend._plan(plan_request, installation)[0].devices == ("cpu",)
    # The observed whole-process floor can be above the exact ORT arena reservation.
    free.return_value = (1 << 30) + (300 << 20)
    assert backend._plan(plan_request, installation)[0].devices == ("cuda", "cpu")
    assert accepted.provider_calibration is not None
    larger_process = replace(
        accepted.provider_calibration,
        observed_at=adapter.time.monotonic(),
        cuda_process_sampled_usage_bytes=(1 << 30) + (80 << 20),
    )
    backend.accept_calibration(request, replace(accepted, provider_calibration=larger_process))
    assert backend._plan(plan_request, installation)[0].devices == ("cpu",)
    free.reset_mock()
    cpu_plan, cuda = backend._plan(replace(plan_request, device_policy="cpu"), installation)
    assert cuda is None
    assert cpu_plan.key == cuda_plan.key
    free.assert_not_called()


@pytest.mark.skipif(os.name != "nt", reason="Windows owned BS worker route")
def test_native_bs_worker_requires_scope_and_uses_strict_retirement(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    executable = tmp_path / BS_ONNX_EXECUTABLE
    args = [str(executable), "track"]
    result = OwnedCommandResult("ok", 0, b"", b"")
    with pytest.raises(RuntimeError, match="requires an admitted Windows stem scope"):
        stem_process.run_stem_command(args, cwd=tmp_path, env={}, native_bs_onnx=True)

    def run(
        _args: list[str],
        _cwd: Path,
        env: dict[str, str],
        _cancel: Event,
        request_abort: object,
        _stop_reason: object,
        *,
        strict_completion: bool,
    ) -> OwnedCommandResult:
        assert strict_completion
        assert callable(request_abort)
        marker = Path(env[CANCEL_PATH_ENV])
        assert marker.parent.parent.samefile(tmp_path)
        assert not marker.exists()
        request_abort()
        assert marker.read_bytes() == b"cancel"
        return result

    monkeypatch.setattr(stem_process, "_run_owned", run)
    with stem_process.stem_command_scope(Event(), directory=tmp_path, resources=Mock()):
        assert (
            stem_process.run_stem_command(args, cwd=tmp_path, env={}, native_bs_onnx=True) is result
        )
    assert not list(tmp_path.glob(".stem-command-*"))
