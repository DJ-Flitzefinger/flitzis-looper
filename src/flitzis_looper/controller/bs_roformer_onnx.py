"""Run Windows BS-RoFormer ONNX under the existing offline Stem Job."""

import hashlib
import json
import logging
import os
import shutil
import sys
import time
from dataclasses import dataclass, replace
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import TYPE_CHECKING, Annotated, Literal

from pydantic import ConfigDict, Field, TypeAdapter, ValidationError

from flitzis_looper import bs_decode
from flitzis_looper.analysis.beat_cuda_cache import CudaDeviceIdentity, cuda_device_identity
from flitzis_looper.analysis.execution import (
    NATIVE_CPU_PACKAGE_SHA,
    NATIVE_CUDA_PACKAGE_SHA,
    DeviceName,
)
from flitzis_looper.bs_decode import FRAME_BYTES, SAMPLE_RATE
from flitzis_looper.bs_input_provenance import (
    BS_INPUT_CONTRACT_ENV,
    bs_input_contract,
    verified_input_output,
)
from flitzis_looper.bs_onnx_cuda_cache import (
    BsCudaCacheStatus,
    bs_cuda_cache_status,
    ensure_bs_cuda_cache,
)
from flitzis_looper.bs_onnx_installation import (
    BS_ONNX_CUDA_PROVIDER_SHA256,
    BS_ONNX_MANIFEST_ENV,
    BS_ONNX_MODELS_DIRECTORY,
    BS_ONNX_WEIGHTS_SHA256,
    BsOnnxInstallation,
    bs_onnx_worker_cwd,
    load_bs_onnx_installation,
)
from flitzis_looper.bs_onnx_vram import (
    CudaMemoryReceipt,
    nvml_free_bytes,
)
from flitzis_looper.controller.bs_roformer_assets import MODEL_IDENTITY, MODEL_NAME
from flitzis_looper.controller.processing_code import (
    fresh_python_source_environment,
    loaded_code_identity,
)
from flitzis_looper.controller.stem_artifacts import write_project_cache_artifacts
from flitzis_looper.controller.stem_generation import (
    StemDevice,
    StemGenerationError,
    StemGenerationRequest,
    StemGenerationResult,
    StemProgressCallback,
    _validate_request,
)
from flitzis_looper.controller.stem_identity import request_fingerprint
from flitzis_looper.controller.stem_process import (
    run_stem_command,
    stem_cache_io_path,
    stem_command_cancel_event,
    stem_command_checkpoint,
)
from flitzis_looper.controller.stem_provenance import (
    _host_processing_paths,
    native_conversion_identity,
)
from flitzis_looper.controller.stem_provider import (
    CudaCapability,
    ProviderCalibration,
    ProviderPlan,
    StemProviderSelector,
)
from flitzis_looper.encoder_provenance import supported_encoder_images
from flitzis_looper.inference_provider import is_cuda_failure
from flitzis_looper.pcm16_wav import PcmWavReader
from flitzis_looper.stem_execution_protocol import ExecutionReceipt, report_execution
from flitzis_looper_audio import SealedProcessingFiles

if TYPE_CHECKING:
    from flitzis_looper.analysis.process import OwnedCommandResult

_SOURCES = ("drums", "bass", "other", "vocals")
_FFT_PROFILES = {
    "cpu": "onemkl-2026.0-20260401-intel-lp64-stft",
    "cuda": "rustfft-bs-cuda-amp-endpoints-v1",
}
_ONNX_HOST_SOURCES = (
    "controller/bs_roformer_onnx.py",
    "controller/stem_provenance.py",
    "bs_onnx_installation.py",
    "bs_onnx_cuda_cache.py",
    "bs_onnx_vram.py",
    "bs_decode.py",
    "analysis/beat_cuda_cache.py",
)
_DECODER_SOURCES = (
    "__init__.py",
    "bs_decode.py",
    "bs_input_provenance.py",
    "encoder_provenance.py",
    "offline_runtime.py",
    "stem_cancellation.py",
)
_logger = logging.getLogger(__name__)

type _Digest = Annotated[str, Field(pattern=r"^[0-9a-f]{64}$")]
type _Positive = Annotated[int, Field(strict=True, gt=0)]
type _Nonnegative = Annotated[int, Field(strict=True, ge=0)]
type _CudaUuid = Annotated[str, Field(pattern=r"^GPU-[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$")]


@dataclass(frozen=True, slots=True)
class _CudaExecution:
    __pydantic_config__ = ConfigDict(extra="forbid", strict=True)

    device_uuid: _CudaUuid
    device_name: DeviceName
    driver_api_version: _Positive
    package_sha256: _Digest
    provider_sha256: _Digest


@dataclass(frozen=True, slots=True)
class _CudaArena:
    __pydantic_config__ = ConfigDict(extra="forbid", strict=True)

    kind: Literal["ort-cuda-device-arena"]
    max_in_use_bytes: Annotated[int, Field(strict=True, gt=0, le=2**63 - 1)]
    peak_reserved_bytes: Annotated[int, Field(strict=True, gt=0, le=2**63 - 1)] | None


@dataclass(frozen=True, slots=True)
class _TrackReceipt:
    __pydantic_config__ = ConfigDict(extra="forbid", strict=True)

    schema_version: Literal[1]
    mode: Literal["track"]
    device: StemDevice
    provider: Literal["CPUExecutionProvider", "CUDAExecutionProvider"]
    frames: _Positive
    sample_rate_hz: Literal[44100]
    channels: Literal[2]
    pcm_sha256: _Digest
    graph_sha256: _Digest
    weights_sha256: _Digest
    outputs: dict[str, _Digest]
    ort_version: Literal["1.24.4"]
    ort_api: Literal[24]
    ort_package_sha256: _Digest
    cuda_kernel_events: _Nonnegative
    cuda: _CudaExecution | None
    fft_profile: str
    cuda_memory: CudaMemoryReceipt = None
    cuda_arena: _CudaArena | None = None


_RECEIPT = TypeAdapter(_TrackReceipt)


@dataclass(slots=True)
class _SourcePcm:
    path: Path
    frames: int
    digest: str
    guard: SealedProcessingFiles | None
    decoder_verified: bool
    decoder_witnesses: tuple[tuple[str, int], ...] = ()

    def verify(self) -> None:
        assert self.guard is not None
        self.guard.verify()

    def release(self) -> None:
        # Drop the verified file and ancestor readers even if an exception has
        # retained this attempt's stack frames for reporting.
        self.guard = None


def _digests(guard: SealedProcessingFiles) -> tuple[tuple[str, int], ...]:
    return tuple((digest, size) for _, digest, size in guard.witnesses())


def _run_failure(result: OwnedCommandResult, tool: str) -> str:
    detail = " ".join(result.stderr.decode("utf-8", "replace").split())[-700:]
    message = f"{tool} failed ({result.reason}, exit {result.returncode})"
    return f"{message}: {detail}" if detail else message


def _decode(request: StemGenerationRequest, scratch: Path, encoder: Path) -> _SourcePcm:
    pcm = scratch / "source.f32"
    env = dict(os.environ)
    env["PATH"] = str(encoder.parent) + os.pathsep + env.get("PATH", "")
    env.pop(BS_INPUT_CONTRACT_ENV, None)
    contract = bs_input_contract(encoder)
    if contract is not None:
        env[BS_INPUT_CONTRACT_ENV] = contract
    package = Path(bs_decode.__file__).resolve(strict=True).parent
    decoder_guard = SealedProcessingFiles([str(package / name) for name in _DECODER_SOURCES])
    if (package / "__init__.py").read_bytes() != b"":
        msg = "BS-RoFormer decoder source requires the measured application package"
        raise StemGenerationError(msg)
    with fresh_python_source_environment(env, scratch, source_roots=(package.parent,)) as child:
        result = run_stem_command(
            [sys.executable, "-m", "flitzis_looper.bs_decode", str(request.source_path), str(pcm)],
            cwd=Path.cwd(),
            env=child,
        )
    decoder_guard.verify()
    if result.reason != "ok" or result.returncode != 0:
        raise StemGenerationError(_run_failure(result, "BS-RoFormer decoder"))
    stem_command_checkpoint()
    guard = SealedProcessingFiles([str(pcm)])
    try:
        _, digest, size = guard.witnesses()[0]
        frames, remainder = divmod(size, FRAME_BYTES)
        if not 0 < frames <= sys.maxsize // FRAME_BYTES or remainder:
            msg = "BS-RoFormer decoder returned incomplete stereo float32 PCM"
            raise StemGenerationError(msg)
        verified = verified_input_output(
            scratch, contract, result.stdout.decode("utf-8", "replace")
        )
        return _SourcePcm(pcm, frames, digest, guard, verified, _digests(decoder_guard))
    finally:
        del guard


def _producer_family(
    source: _SourcePcm,
    guards: tuple[SealedProcessingFiles, ...],
    host_execution: str | None,
) -> str | None:
    if not source.decoder_verified or not source.decoder_witnesses or host_execution is None:
        return None
    conversion = native_conversion_identity()
    if conversion is None:
        return None
    # The manifest includes both offered graphs. Runtime and weights pins
    # complete this provider-independent cost family; saved assets use a
    # separate identity attesting the precise profile which produced them.
    identity = (
        "bs-roformer-onnx-family-v2",
        host_execution,
        tuple(_digests(guard) for guard in guards),
        source.decoder_witnesses,
        sys.version,
        conversion,
        tuple(_FFT_PROFILES.items()),
        BS_ONNX_WEIGHTS_SHA256,
        NATIVE_CPU_PACKAGE_SHA,
        NATIVE_CUDA_PACKAGE_SHA,
    )
    return hashlib.sha256(json.dumps(identity, separators=(",", ":")).encode("utf-8")).hexdigest()


def _producer_identity(
    execution: ExecutionReceipt,
    receipt: _TrackReceipt,
    source: _SourcePcm,
    guards: tuple[SealedProcessingFiles, ...],
    host_execution: str | None,
) -> str | None:
    if (
        host_execution is None
        or not source.decoder_verified
        or receipt.fft_profile != _FFT_PROFILES[execution.device]
    ):
        return None
    conversion = native_conversion_identity()
    if conversion is None:
        return None
    identity = (
        "bs-roformer-onnx-v2",
        host_execution,
        tuple(_digests(guard) for guard in guards),
        source.decoder_witnesses,
        sys.version,
        conversion,
        execution.device,
        execution.cuda_device_identity,
        receipt.fft_profile,
        receipt.graph_sha256,
        receipt.weights_sha256,
        receipt.ort_version,
        receipt.ort_api,
        receipt.ort_package_sha256,
    )
    return hashlib.sha256(json.dumps(identity, separators=(",", ":")).encode("utf-8")).hexdigest()


def _onnx_host_paths(request: StemGenerationRequest) -> tuple[Path, ...]:
    package = Path(bs_decode.__file__).resolve(strict=True).parent
    return (*_host_processing_paths(request), *(package / name for name in _ONNX_HOST_SOURCES))


def _seal_launch(installation: BsOnnxInstallation, encoder: Path) -> SealedProcessingFiles:
    images = supported_encoder_images(encoder)
    probe = encoder.with_name("ffprobe.exe")
    if not images or not probe.is_file() or probe.with_suffix(".exe.local").exists():
        msg = "BS-RoFormer ONNX requires the supported shared FFmpeg and FFprobe"
        raise StemGenerationError(msg)
    crt_files = [installation.root / "crt" / name for name, _, _ in installation.crt_files]
    guard = SealedProcessingFiles([
        str(path) for path in (installation.executable, *crt_files, *images, probe)
    ])
    witnesses = guard.witnesses()
    if witnesses[0][1] != installation.executable_sha256 or [
        witness[1:] for witness in witnesses[1 : len(crt_files) + 1]
    ] != [(digest, size) for _, size, digest in installation.crt_files]:
        msg = "BS-RoFormer ONNX worker or CRT differs from its installation manifest"
        raise StemGenerationError(msg)
    return guard


def resolve_bs_onnx_manifest(project_root: Path) -> Path | None:
    """Capture the setup-selected manifest or the plain Source installation pointer."""
    if manifest := os.environ.get(BS_ONNX_MANIFEST_ENV):
        return Path(manifest)
    if manifest is not None:
        msg = "The configured BS-RoFormer ONNX manifest path is empty"
        raise StemGenerationError(msg)
    root = project_root / BS_ONNX_MODELS_DIRECTORY
    try:
        (root / "current.json").lstat()
    except FileNotFoundError:
        return None
    from flitzis_looper.bs_onnx_provisioning import (  # noqa: PLC0415
        selected_bs_onnx_installation,
    )

    return selected_bs_onnx_installation(root).manifest


def _environment(
    installation: BsOnnxInstallation,
    output: Path,
    device: CudaDeviceIdentity | None,
    cache_path: Path | None,
) -> dict[str, str]:
    package, digest = installation.package(cuda=device is not None)
    env = {
        "SystemRoot": os.environ["SYSTEMROOT"],
        "TEMP": str(output),
        "TMP": str(output),
        "FLITZIS_TRIAL_ORT_PACKAGE": str(package),
        "FLITZIS_TRIAL_ORT_PACKAGE_SHA256": digest,
        "FLITZIS_TRIAL_ORT_LOAD_POLICY": "package-system32",
        "FLITZIS_BEAT_THIS_CRT": str(installation.root / "crt"),
    }
    if device is not None:
        if cache_path is None:
            cache_path = output / "cuda-cache"
            cache_path.mkdir()
        env["CUDA_VISIBLE_DEVICES"] = device.uuid
        env["CUDA_CACHE_PATH"] = str(cache_path)
        env["CUDA_CACHE_DISABLE"] = "0"
    return env


def _cuda_execution_matches(cuda: _CudaExecution | None, device: CudaDeviceIdentity) -> bool:
    return (
        cuda is not None
        and cuda.device_uuid.lower() == device.uuid.lower()
        and cuda.package_sha256 == NATIVE_CUDA_PACKAGE_SHA
        and cuda.provider_sha256 == BS_ONNX_CUDA_PROVIDER_SHA256
    )


def _cuda_identity(device: CudaDeviceIdentity) -> str:
    return hashlib.sha256(
        f"ort-1.24.4:{device.driver_release}:{device.uuid.lower()}".encode("ascii")
    ).hexdigest()


def _reserved_cuda_peak(receipt: _TrackReceipt) -> int | None:
    arena = receipt.cuda_arena
    if (
        receipt.device == "cuda"
        and receipt.fft_profile == _FFT_PROFILES["cuda"]
        and arena is not None
        and arena.peak_reserved_bytes is not None
        and arena.peak_reserved_bytes >= arena.max_in_use_bytes
    ):
        return arena.peak_reserved_bytes
    return None


def _execution(
    receipt: _TrackReceipt,
    device: CudaDeviceIdentity | None,
    *,
    cpu_fallback: bool,
) -> ExecutionReceipt:
    if receipt.fft_profile != _FFT_PROFILES[receipt.device]:
        msg = "BS-RoFormer ONNX FFT profile differs from its completed provider"
        raise StemGenerationError(msg)
    identity = None
    if device is None:
        valid = (
            receipt.device == "cpu"
            and receipt.provider == "CPUExecutionProvider"
            and receipt.cuda is None
            and receipt.cuda_kernel_events == 0
            and receipt.ort_package_sha256 == NATIVE_CPU_PACKAGE_SHA
        )
    else:
        cuda = receipt.cuda
        valid = (
            receipt.device == "cuda"
            and receipt.provider == "CUDAExecutionProvider"
            and receipt.ort_package_sha256 == NATIVE_CUDA_PACKAGE_SHA
            and receipt.cuda_kernel_events > 0
            and _cuda_execution_matches(cuda, device)
        )
        identity = _cuda_identity(device)
    if not valid:
        msg = "BS-RoFormer ONNX provider receipt differs from its admitted attempt"
        raise StemGenerationError(msg)
    return ExecutionReceipt(
        backend="bs-roformer",
        model_name=MODEL_NAME,
        device=receipt.device,
        cpu_fallback=cpu_fallback,
        intra_op_threads=1,
        inter_op_threads=1,
        cuda_peak_bytes=_reserved_cuda_peak(receipt),
        cuda_device_identity=identity,
    )


def _verify_outputs(
    receipt: _TrackReceipt, graph_sha256: str, source: _SourcePcm, output: Path
) -> SealedProcessingFiles:
    if (
        receipt.frames != source.frames
        or receipt.pcm_sha256 != source.digest
        or receipt.graph_sha256 != graph_sha256
        or receipt.weights_sha256 != BS_ONNX_WEIGHTS_SHA256
        or set(receipt.outputs) != set(_SOURCES)
    ):
        msg = "BS-RoFormer ONNX output receipt differs from its admitted model or PCM"
        raise StemGenerationError(msg)
    paths = [output / f"{stem}.wav" for stem in _SOURCES]
    guard = SealedProcessingFiles([str(path) for path in paths])
    try:
        for stem, path, (_, digest, _) in zip(_SOURCES, paths, guard.witnesses(), strict=True):
            stem_command_checkpoint()
            if receipt.outputs[stem] != digest:
                msg = f"BS-RoFormer ONNX {stem} output digest differs from its receipt"
                raise StemGenerationError(msg)
            with PcmWavReader(path) as wav:
                shape = (
                    wav.getnframes(),
                    wav.getframerate(),
                    wav.getnchannels(),
                    wav.getsampwidth(),
                )
                if shape != (source.frames, SAMPLE_RATE, 2, 2):
                    msg = f"BS-RoFormer ONNX {stem} output has the wrong PCM16 shape"
                    raise StemGenerationError(msg)
        return guard
    finally:
        del guard


def _run_worker(
    installation: BsOnnxInstallation,
    source: _SourcePcm,
    output: Path,
    device: CudaDeviceIdentity | None,
    cache_path: Path | None,
) -> OwnedCommandResult:
    stem_command_checkpoint()
    env = _environment(installation, output, device, cache_path)
    return run_stem_command(
        [
            str(installation.executable),
            *(["--cuda"] if device is not None else []),
            "track",
            str(source.path),
            str(source.frames),
            source.digest,
            str(output),
            installation.graph_digest(cuda=device is not None),
        ],
        cwd=bs_onnx_worker_cwd(env["SystemRoot"]),
        env=env,
        native_bs_onnx=True,
    )


def _run_cache_warmup(args: list[str], _cwd: Path, env: dict[str, str]) -> OwnedCommandResult:
    # The installation cache owns the recipe and publication. This Stem Job
    # owns the child, cancellation, playback/resource checks and read settlement.
    return run_stem_command(
        args, cwd=bs_onnx_worker_cwd(env["SystemRoot"]), env=env, native_bs_onnx=True
    )


def _cuda_cache(
    installation: BsOnnxInstallation,
    device: CudaDeviceIdentity,
    progress: StemProgressCallback,
) -> BsCudaCacheStatus:
    state = bs_cuda_cache_status(installation, device)
    cancel = stem_command_cancel_event()
    if state.status == "pending" and cancel is not None:
        progress(0.08, "Preparing the BS-RoFormer CUDA cache")
        state = ensure_bs_cuda_cache(
            installation,
            device,
            cancel,
            runner=_run_cache_warmup,
            checkpoint=stem_command_checkpoint,
        )
        stem_command_checkpoint()
    if state.cache_path is None:
        _logger.warning("%s; the current BS-RoFormer attempt uses a temporary cache", state.detail)
    return state


@dataclass(frozen=True, slots=True)
class _FinishedAttempt:
    output: Path
    stdout: bytes
    device: CudaDeviceIdentity | None
    cuda_error: str | None
    cache_diagnostic: str | None
    cache: BsCudaCacheStatus | None


def _confirmed_cuda_device(device: CudaDeviceIdentity) -> bool:
    try:
        return cuda_device_identity() == device
    except OSError, RuntimeError, ValueError:
        return False


def _retained_cuda_cache(installation: BsOnnxInstallation, attempt: _FinishedAttempt) -> bool:
    if attempt.device is None:
        return True
    if attempt.cache is None or attempt.cache.cache_path is None:
        return False
    observed = bs_cuda_cache_status(installation, attempt.device)
    return (
        observed.status == "ready"
        and observed.key == attempt.cache.key
        and observed.cache_path == attempt.cache.cache_path
    )


@dataclass(frozen=True, slots=True)
class _CostContext:
    plan: ProviderPlan
    family: str | None
    started: float
    warm: bool
    host_execution: str | None


class BsOnnxStemGenerationBackend:
    """Separate stems in an admitted Windows process and use shared artifact custody."""

    def __init__(self) -> None:
        self._provider_selector = StemProviderSelector()

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        """Decode once and align one validated four-source output after reader settlement."""
        started = time.monotonic()
        _validate_request(request)
        manifest = request.bs_onnx_manifest
        encoder = request.encoder_executable
        if request.separator != "bs-roformer:musdb18hq":
            msg = "BS-RoFormer ONNX requires a BS-RoFormer MUSDB18HQ request"
            raise StemGenerationError(msg)
        if os.name != "nt" or manifest is None or encoder is None:
            msg = "BS-RoFormer ONNX requires a prepared Windows installation and FFmpeg"
            raise StemGenerationError(msg)
        manifest_guard = SealedProcessingFiles([str(manifest)])
        installation = load_bs_onnx_installation(manifest)
        launch_guard = _seal_launch(installation, encoder)
        host_paths = _onnx_host_paths(request)
        host_guard = SealedProcessingFiles([str(path) for path in host_paths])
        guards = (manifest_guard, launch_guard, host_guard)
        host_execution = loaded_code_identity(host_paths)
        cache = stem_cache_io_path(request.cache_dir)
        cache.mkdir(parents=True, exist_ok=True)
        with TemporaryDirectory(prefix=".bs-onnx-", dir=cache) as name:
            scratch = Path(name)
            progress(0.05, "Decoding BS-RoFormer source")
            source = _decode(request, scratch, encoder)
            try:
                if shutil.disk_usage(scratch).free < source.frames * 16:
                    msg = "Insufficient disk space for the four BS-RoFormer PCM16 outputs"
                    raise StemGenerationError(msg)
                return self._generate(
                    request,
                    installation,
                    source,
                    progress,
                    guards=guards,
                    started=started,
                    host_execution=host_execution,
                )
            finally:
                source.release()

    def _generate(
        self,
        request: StemGenerationRequest,
        installation: BsOnnxInstallation,
        source: _SourcePcm,
        progress: StemProgressCallback,
        *,
        guards: tuple[SealedProcessingFiles, ...] = (),
        started: float | None = None,
        host_execution: str | None = None,
    ) -> StemGenerationResult:
        family = _producer_family(source, guards, host_execution)
        plan, observed = self._plan(replace(request, producer_identity=family), installation)
        cache = (
            _cuda_cache(installation, observed, progress)
            if observed is not None and "cuda" in plan.devices
            else None
        )
        cost = _CostContext(
            plan,
            family,
            time.monotonic() if started is None else started,
            cache is None or (cache.cache_path is not None and not cache.warmup_executed),
            host_execution,
        )
        cache_path = cache.cache_path if cache is not None else None
        cache_diagnostic = cache.detail if cache is not None and cache_path is None else None
        cuda_error = None
        for device in plan.devices:
            progress(0.1, f"Separating {MODEL_NAME} with ONNX on {device.upper()}")
            selected = observed if device == "cuda" else None
            try:
                with TemporaryDirectory(prefix=f".{device}-", dir=source.path.parent) as name:
                    output = Path(name)
                    result = _run_worker(installation, source, output, selected, cache_path)
                    if result.reason != "ok" or result.returncode != 0:
                        failure = _run_failure(result, f"BS-RoFormer ONNX {device.upper()}")
                        stdout = result.stdout.decode("utf-8", "replace")
                        if (
                            result.reason == "worker_crashed"
                            and device == "cuda"
                            and "cpu" in plan.devices
                            and is_cuda_failure(result.returncode, stdout)
                        ):
                            cuda_error = failure
                            continue
                        raise StemGenerationError(failure)
                    return self._finish(
                        request,
                        installation,
                        source,
                        progress,
                        _FinishedAttempt(
                            output, result.stdout, selected, cuda_error, cache_diagnostic, cache
                        ),
                        guards=guards,
                        cost=cost,
                    )
            except (OSError, RuntimeError, ValueError) as error:
                if cuda_error is not None:
                    _logger.warning("BS-RoFormer ONNX CPU retry failed after %s", cuda_error)
                    if isinstance(error, StemGenerationError):
                        msg = f"{error} / first CUDA attempt: {cuda_error}"
                        raise StemGenerationError(msg) from error
                    error.add_note(f"First attempt: {cuda_error}")
                raise
        msg = "BS-RoFormer ONNX did not run"
        raise StemGenerationError(msg)

    def _plan(
        self, request: StemGenerationRequest, installation: BsOnnxInstallation
    ) -> tuple[ProviderPlan, CudaDeviceIdentity | None]:
        if request.device_policy == "cpu":
            return self._provider_selector.cpu_plan(request, MODEL_NAME), None
        device = cuda_device_identity() if installation.cuda else None
        capability = (
            CudaCapability(
                available=True,
                identity=_cuda_identity(device),
                free_bytes=nvml_free_bytes(device.uuid),
            )
            if device is not None
            else CudaCapability(available=False)
        )
        return self._provider_selector.plan(request, MODEL_NAME, capability), device

    @staticmethod
    def _finish(
        request: StemGenerationRequest,
        installation: BsOnnxInstallation,
        source: _SourcePcm,
        progress: StemProgressCallback,
        attempt: _FinishedAttempt,
        *,
        guards: tuple[SealedProcessingFiles, ...],
        cost: _CostContext,
    ) -> StemGenerationResult:
        try:
            receipt = _RECEIPT.validate_json(attempt.stdout)
        except ValidationError as error:
            msg = "BS-RoFormer ONNX worker returned an invalid track receipt"
            raise StemGenerationError(msg) from error
        execution = _execution(receipt, attempt.device, cpu_fallback=attempt.cuda_error is not None)
        confirmed = attempt.device is None or _confirmed_cuda_device(attempt.device)
        if not confirmed:
            execution = replace(execution, cuda_device_identity=None)
        graph_sha256 = installation.graph_digest(cuda=attempt.device is not None)
        outputs = _verify_outputs(receipt, graph_sha256, source, attempt.output)
        try:
            source.verify()
            report_execution(progress, execution)
            progress(0.85, "Aligning stem cache")
            write_project_cache_artifacts(
                output_root=attempt.output,
                cache_dir=request.cache_dir,
                target_shape=request.target_shape,
                progress=progress,
            )
            outputs.verify()
        finally:
            del outputs
        diagnostic = f"{MODEL_IDENTITY} / ONNX graph {graph_sha256}"
        memory = receipt.cuda_memory
        if execution.device == "cuda" and receipt.fft_profile == _FFT_PROFILES["cuda"] and memory:
            diagnostic += (
                f" / DXGI WDDM sampled process usage {memory.observed_peak_bytes} bytes"
                f" ({memory.samples} samples, minimum budget {memory.minimum_budget_bytes} bytes)"
            )
        if attempt.cuda_error is not None:
            diagnostic += f" / CUDA attempt: {attempt.cuda_error} / generated on CPU"
        if attempt.cache_diagnostic is not None:
            diagnostic += f" / temporary CUDA cache: {attempt.cache_diagnostic}"
        for guard in guards:
            guard.verify()
        producer = None
        if confirmed:
            producer = _producer_identity(execution, receipt, source, guards, cost.host_execution)
        else:
            diagnostic += " / CUDA device or driver identity could not be reconfirmed"
        observed_at = time.monotonic()
        calibration = None
        if (
            producer is not None
            and cost.plan.key is not None
            and cost.warm
            and not execution.cpu_fallback
            and _retained_cuda_cache(installation, attempt)
        ):
            calibration = ProviderCalibration(
                cost.plan,
                observed_at - cost.started,
                verified=True,
                observed_at=observed_at,
                cuda_process_sampled_usage_bytes=(
                    memory.observed_peak_bytes
                    if (
                        execution.device == "cuda"
                        and memory is not None
                        and memory.observed_peak_bytes > memory.first_usage_bytes
                    )
                    else None
                ),
            )
        return StemGenerationResult(
            backend_name="bs-roformer",
            model_name=MODEL_NAME,
            device=execution.device,
            cpu_fallback=execution.cpu_fallback,
            artifact_count=len(_SOURCES) + 1,
            diagnostic=diagnostic,
            decoder_verified=source.decoder_verified,
            execution=execution,
            provider_calibration=calibration,
            executed_producer_identity=producer,
            calibration_family=cost.family if calibration is not None else None,
        )

    def accept_calibration(
        self, request: StemGenerationRequest, result: StemGenerationResult
    ) -> None:
        """Promote only a complete observation for this guarded ONNX family."""
        calibration = result.provider_calibration
        family = result.calibration_family
        if calibration is None or calibration.plan.key is None or family is None:
            return
        actual_key = request_fingerprint(replace(request, device_policy="auto"), family)
        if actual_key == calibration.plan.key[0]:
            self._provider_selector.accept(calibration, result.execution)
