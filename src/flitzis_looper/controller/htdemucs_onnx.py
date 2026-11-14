"""App stem worker adapter for the pinned Windows HTDemucs release."""

import ctypes
import hashlib
import json
import os
import re
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path
from random import SystemRandom
from tempfile import TemporaryDirectory

from flitzis_looper.analysis.beat_cuda_cache import (
    CudaDeviceIdentity,
    _system_library,
    cuda_device_identity,
)
from flitzis_looper.analysis.execution import NATIVE_CPU_PACKAGE_SHA, NATIVE_CUDA_PACKAGE_SHA
from flitzis_looper.bs_input_provenance import (
    BS_INPUT_CONTRACT_ENV,
    bs_input_contract,
    verified_input_output,
)
from flitzis_looper.bs_onnx_vram import nvml_free_bytes
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
    _format_demucs_float,
    _validate_request,
)
from flitzis_looper.controller.stem_process import (
    NATIVE_DEMUCS_EXECUTABLE,
    run_stem_command,
    stem_cache_io_path,
    stem_command_checkpoint,
)
from flitzis_looper.controller.stem_provider import CudaCapability, StemProviderSelector
from flitzis_looper.encoder_provenance import (
    ENCODER_CONTRACT_ENV,
    encoder_contract,
    supported_encoder_images,
    verified_encoder_output,
)
from flitzis_looper.htdemucs_onnx_provisioning import (
    HTDEMUCS_RUNTIME_EXECUTABLE,
    HTDEMUCS_SOURCE_CRT_ENV,
    HTDEMUCS_SOURCE_WORKER_ENV,
    htdemucs_crt_witnesses,
    htdemucs_source_crt,
    htdemucs_source_worker,
    selected_htdemucs_installation,
)
from flitzis_looper.inference_provider import is_cuda_failure
from flitzis_looper.pcm16_wav import PcmWavReader
from flitzis_looper.stem_execution_protocol import ExecutionReceipt, report_execution
from flitzis_looper_audio import SealedProcessingFiles

_RELEASE = Path(__file__).resolve().parents[3] / "workers/htdemucs/native_cpu/release.json"
_WORKER_PIN = _RELEASE.with_name("worker.json")
_SOURCES = ("drums", "bass", "other", "vocals")
_RATE = 44100
_CODEC_OUTPUT = "FLITZIS_HTDEMUCS_CODEC_OUTPUT_V1:"
_MAX_DECODE_CHANNELS = 8192  # One frame must fit the worker's 32 KiB read block.
_CUDA_PROVIDER_SHA = "110929d2552bc775828e7a5c18c5ef9ab90898fc57a57868623fe2fe8c82d494"
_CUDA_MANIFEST_SHA = "7ebc26a630b9fb721539e7821b918cf9f4a9aa94b856dcc153b81a7b140b240d"
_MANIFEST_SHA = "4929eb4f6a3aebe2c117850a2e1354c48bc004bfa5ff4d805c2ce0ecbef2915d"
_DIGEST = re.compile(r"[0-9a-f]{64}")


def _require(condition: object, message: str) -> None:
    if not condition:
        raise StemGenerationError(message)


def _identity(parts: object) -> str:
    return hashlib.sha256(json.dumps(parts, separators=(",", ":")).encode()).hexdigest()


@dataclass(frozen=True, slots=True)
class _ReleaseIdentity:
    graph_sha256: str
    trace_sha256: str
    approval_sha256: str
    worker_sha256: str
    worker_bytes: int


def _approved_release(*, cuda: bool = False) -> _ReleaseIdentity:
    _require(os.name == "nt", "Native HTDemucs CPU requires Windows")
    approval = _RELEASE.parents[1] / "native_cuda/release.json" if cuda else _RELEASE
    raw, value = _read_approval(approval)
    if not isinstance(value, dict):
        msg = "The pinned HTDemucs learned Scan64 graph is unavailable"
        raise StemGenerationError(msg)
    if (
        set(value)
        != {
            "graph_bytes",
            "graph_sha256",
            "trace_sha256",
            "attention_sha256",
        }
        or type(value["graph_bytes"]) is not int
        or not 0 < value["graph_bytes"] <= sys.maxsize
    ):
        msg = "Native HTDemucs release extent is invalid"
        raise StemGenerationError(msg)
    graph, trace, attention = (
        value[key]
        for key in (
            "graph_sha256",
            "trace_sha256",
            "attention_sha256",
        )
    )
    if not all(
        isinstance(digest, str) and _DIGEST.fullmatch(digest)
        for digest in (
            graph,
            trace,
            attention,
        )
    ):
        msg = "Native HTDemucs release digests are invalid"
        raise StemGenerationError(msg)
    worker_sha, worker_size = _approved_worker()
    return _ReleaseIdentity(graph, trace, hashlib.sha256(raw).hexdigest(), worker_sha, worker_size)


def _read_approval(path: Path) -> tuple[bytes, object]:
    _require(path.is_file(), f"Native HTDemucs approval is unavailable for {path.name}")
    with path.open("rb") as stream:
        raw = stream.read(4097)
    _require(len(raw) <= 4096, "Native HTDemucs release approval exceeds its size limit")
    return raw, json.loads(raw)


def _approved_worker() -> tuple[str, int]:
    _, value = _read_approval(_WORKER_PIN)
    if not isinstance(value, dict) or set(value) != {"sha256", "bytes"}:
        msg = "Native HTDemucs worker build approval is unavailable"
        raise StemGenerationError(msg)
    digest, size = value["sha256"], value["bytes"]
    if (
        not isinstance(digest, str)
        or not _DIGEST.fullmatch(digest)
        or type(size) is not int
        or size <= 0
    ):
        msg = "Native HTDemucs worker build approval is invalid"
        raise StemGenerationError(msg)
    return digest, size


def _run_codec(
    mode: str, first: Path, second: Path, scratch: Path, encoder: Path
) -> tuple[str, bool]:
    is_decode = mode == "decode"
    contract = bs_input_contract(encoder) if is_decode else encoder_contract(encoder)
    key = BS_INPUT_CONTRACT_ENV if is_decode else ENCODER_CONTRACT_ENV
    env = dict(os.environ)
    env.pop(key, None)
    env["PATH"] = str(encoder.parent) + os.pathsep + env.get("PATH", "")
    if contract is not None:
        env[key] = contract
    with fresh_python_source_environment(
        env, scratch, source_roots=(Path(__file__).resolve().parents[2],)
    ) as guarded:
        result = run_stem_command(
            [sys.executable, "-m", "flitzis_looper.htdemucs_codec", mode, str(first), str(second)],
            cwd=Path.cwd(),
            env=guarded,
        )
    if result.reason != "ok" or result.returncode != 0:
        detail = result.stderr.decode(errors="replace").strip()[-1500:]
        msg = f"HTDemucs {mode} failed ({result.returncode}): {detail or result.reason}"
        raise StemGenerationError(msg)
    stem_command_checkpoint()
    stdout = result.stdout.decode("utf-8", "replace")
    verified = (
        verified_input_output(scratch, contract, stdout)
        if is_decode
        else verified_encoder_output(scratch, contract, stdout)
    )
    return stdout, verified


def _verified_codec_files(stdout: str, guard: SealedProcessingFiles) -> bool:
    records = [
        line.removeprefix(_CODEC_OUTPUT)
        for line in stdout.splitlines()
        if line.startswith(_CODEC_OUTPUT)
    ]
    if len(records) != 1:
        return False
    try:
        receipt: object = json.loads(records[0])
    except ValueError:
        return False
    witnesses = {}
    for name, digest, extent in guard.witnesses():
        path = Path(name)
        info = path.stat(follow_symlinks=False)
        witnesses[path.name] = [info.st_dev, info.st_ino, extent, digest]
    return witnesses == receipt


def _outputs(
    stdout: bytes, expected: dict[str, object], device: CudaDeviceIdentity | None
) -> tuple[int, dict[str, str]]:
    receipt = json.loads(stdout)
    if not isinstance(receipt, dict):
        msg = "Native HTDemucs worker returned an invalid receipt"
        raise StemGenerationError(msg)
    runtime = receipt.get("runtime")
    outputs = receipt.get("outputs")
    frames = receipt.get("frames")
    cuda = receipt.get("cuda")
    gpu_events = receipt.get("cuda_kernel_events")
    cpu_capability = receipt.get("cpu_capability")
    if device is None:
        _require(
            cuda is None and type(gpu_events) is int and gpu_events == 0,
            "Native HTDemucs CPU reported CUDA execution",
        )
    else:
        _require(
            isinstance(cuda, dict)
            and str(cuda.get("device_uuid")).casefold() == device.uuid.casefold()
            and cuda.get("package_sha256") == NATIVE_CUDA_PACKAGE_SHA
            and cuda.get("provider_sha256") == _CUDA_PROVIDER_SHA
            and type(gpu_events) is int
            and gpu_events > 0,
            "Native HTDemucs output lacks confirmed CUDA compute",
        )
    manifest_sha = _MANIFEST_SHA if device is None else _CUDA_MANIFEST_SHA
    package_sha = NATIVE_CPU_PACKAGE_SHA if device is None else NATIVE_CUDA_PACKAGE_SHA
    loaded = runtime.get("loaded_package") if isinstance(runtime, dict) else None
    package = runtime.get("declared_package") if isinstance(runtime, dict) else None
    _require(
        set(receipt)
        == {
            *expected,
            "frames",
            "job",
            "outputs",
            "runtime",
            "cuda",
            "cuda_kernel_events",
            "cuda_memory",
            "cpu_capability",
        }
        and isinstance(cpu_capability, str)
        and cpu_capability in {"AVX512", "AVX2", "DEFAULT"}
        and type(frames) is int
        and 0 < frames <= sys.maxsize // (8 * len(_SOURCES))
        and all(receipt.get(key) == value for key, value in expected.items())
        and receipt["job"] == "windows-owned"
        and isinstance(runtime, dict)
        and runtime.get("runtime_version") == "1.24.4"
        and runtime.get("ort_api") == 24
        and runtime.get("manifest_sha256") == manifest_sha
        and isinstance(loaded, dict)
        and loaded.get("policy") == "package-system32"
        and isinstance(package, dict)
        and package.get("manifest_sha256") == package_sha,
        "Native HTDemucs worker receipt differs from the admitted request or runtime",
    )
    if (
        not isinstance(outputs, dict)
        or set(outputs) != set(_SOURCES)
        or not all(
            isinstance(value, str) and _DIGEST.fullmatch(value) for value in outputs.values()
        )
    ):
        msg = "Native HTDemucs worker did not attest the four PCM outputs"
        raise StemGenerationError(msg)
    # Use the verified worker extent for every PCM and WAV payload.
    assert isinstance(frames, int)
    return frames, outputs


def _encode(
    encoder: Path, scratch: Path, frames: int, outputs: dict[str, str]
) -> tuple[bool, SealedProcessingFiles]:
    paths = [scratch / f"{name}.f32" for name in _SOURCES]
    guard = SealedProcessingFiles([str(path) for path in paths])
    _require(
        [(digest, size) for _, digest, size in guard.witnesses()]
        == [(outputs[name], frames * 8) for name in _SOURCES],
        "Native HTDemucs output PCM changed after the worker exited",
    )
    _require(
        shutil.disk_usage(scratch).free >= frames * 4 * len(paths),
        "Insufficient disk space for HTDemucs WAV output",
    )
    stdout, verified = _run_codec("encode", encoder, scratch, scratch, encoder)
    waves = tuple(scratch / f"{name}.wav" for name in _SOURCES)
    wave_guard = SealedProcessingFiles([str(path) for path in waves])
    for wav in waves:
        with PcmWavReader(wav) as reader:
            _require(
                (
                    reader.getnframes(),
                    reader.getframerate(),
                    reader.getnchannels(),
                    reader.getsampwidth(),
                )
                == (frames, _RATE, 2, 2),
                f"Invalid native HTDemucs PCM16 output shape for {wav.name}",
            )
            reader.verify(checkpoint=stem_command_checkpoint)
    guard.verify()
    wave_guard.verify()
    _require(
        _verified_codec_files(stdout, wave_guard),
        "HTDemucs encoder output differs from its receipt",
    )
    return verified, wave_guard


class _StagedPcm:
    def __init__(
        self,
        path: Path,
        channels: int,
        frames: int,
        guard: SealedProcessingFiles,
        *,
        verified: bool,
    ) -> None:
        self.path = path
        self.channels = channels
        self.frames = frames
        self.guard: SealedProcessingFiles | None = guard
        self.digest = guard.witnesses()[0][1]
        self.decoder_verified = verified

    def verify(self) -> None:
        assert self.guard is not None
        self.guard.verify()

    def release(self) -> None:
        self.guard = None


def _stage(encoder: Path, request: StemGenerationRequest, pcm: Path) -> _StagedPcm:
    stdout, verified = _run_codec("decode", request.source_path, pcm, pcm.parent, encoder)
    try:
        channels = json.loads(stdout.splitlines()[0])["channels"]
    except (IndexError, KeyError, TypeError, ValueError) as error:
        msg = "Invalid HTDemucs decoder receipt"
        raise StemGenerationError(msg) from error
    _require(
        type(channels) is int and 0 < channels <= _MAX_DECODE_CHANNELS,
        "Invalid HTDemucs staged source channel count",
    )
    guard = SealedProcessingFiles([str(pcm)])
    size = guard.witnesses()[0][2]
    frames, remainder = divmod(size, channels * 4)
    _require(
        frames > 0 and remainder == 0 and size <= sys.maxsize,
        "HTDemucs staged source PCM extent is invalid",
    )
    _require(
        _verified_codec_files(stdout, guard), "HTDemucs decoder output differs from its receipt"
    )
    return _StagedPcm(pcm, channels, frames, guard, verified=verified)


def _native_cuda_profile_supported() -> bool:
    # This GroupNorm program and learned graph were exercised on the single
    # measured RTX 5090 Laptop profile. Other CUDA arithmetic uses qualified CPU.
    with _system_library("nvcuda.dll") as driver:
        device = ctypes.c_int()
        name = ctypes.create_string_buffer(128)
        major, minor = ctypes.c_int(), ctypes.c_int()
        if driver.cuDeviceGet(ctypes.byref(device), 0) != 0:
            return False
        return (
            driver.cuDeviceGetName(name, len(name), device) == 0
            and name.value == b"NVIDIA GeForce RTX 5090 Laptop GPU"
            and driver.cuDeviceGetAttribute(ctypes.byref(major), 75, device) == 0
            and driver.cuDeviceGetAttribute(ctypes.byref(minor), 76, device) == 0
            and (major.value, minor.value) == (12, 0)
        )


def _observed_cuda() -> CudaDeviceIdentity | None:
    try:
        identity = cuda_device_identity()
        if identity is not None and _native_cuda_profile_supported():
            return identity
    except OSError, RuntimeError, ValueError:
        # An unavailable driver observation proves neither placement nor identity.
        return None
    return None


def _runtime_build() -> tuple[Path | None, Path | None]:
    package = Path(__file__).resolve().parents[2]
    if portable := os.environ.get("FLITZIS_PORTABLE_RUNTIME"):
        return Path(portable) / HTDEMUCS_RUNTIME_EXECUTABLE, package / "flitzis_looper_audio"
    source = os.environ.get(HTDEMUCS_SOURCE_WORKER_ENV)
    crt = os.environ.get(HTDEMUCS_SOURCE_CRT_ENV)
    if source is not None and crt is not None:
        return Path(source), Path(crt)
    if source is not None or crt is not None:
        msg = "The HTDemucs source build requires both worker and CRT bindings"
        raise StemGenerationError(msg)
    worker = htdemucs_source_worker(package.parent / ".cache/runtime-setup")
    if worker.is_file():
        return worker, htdemucs_source_crt()
    return None, None


class _CudaAttemptError(StemGenerationError):
    """An eligible CUDA failure from the owned worker after retirement."""


def _separate(
    source: _StagedPcm,
    executable: Path,
    scratch: Path,
    release: _ReleaseIdentity,
    overlap: str,
    offsets: list[int],
    device: CudaDeviceIdentity | None,
) -> tuple[int, dict[str, str]]:
    token, provider, directory, package_sha = (
        ("cpu", "CPUExecutionProvider", "ort", NATIVE_CPU_PACKAGE_SHA)
        if device is None
        else ("cuda", "CUDAExecutionProvider", "cuda", NATIVE_CUDA_PACKAGE_SHA)
    )
    env = {
        "SystemRoot": os.environ["SYSTEMROOT"],
        "TEMP": str(scratch),
        "TMP": str(scratch),
        "FLITZIS_TRIAL_ORT_LOAD_POLICY": "package-system32",
        "FLITZIS_TRIAL_ORT_PACKAGE_SHA256": package_sha,
        "FLITZIS_TRIAL_ORT_PACKAGE": str(executable.parent / directory / "step23-package.json"),
        "FLITZIS_BEAT_THIS_CRT": str(executable.parent / "crt"),
    }
    args = [str(executable)]
    if device is not None:
        cache = scratch / "cuda-cache"
        cache.mkdir()
        env["CUDA_VISIBLE_DEVICES"] = device.uuid
        env["CUDA_CACHE_PATH"] = str(cache)
        args += ["--cuda"]
    result = run_stem_command(
        [
            *args,
            str(source.path),
            str(_RATE),
            str(source.channels),
            str(source.frames),
            source.digest,
            overlap,
            json.dumps(offsets),
            str(scratch),
        ],
        cwd=Path(env["SystemRoot"]),
        env=env,
        native_demucs=True,
    )
    source.verify()
    if result.reason != "ok" or result.returncode != 0:
        detail = result.stderr.decode(errors="replace").strip()[-1500:]
        msg = (
            f"Native HTDemucs {token.upper()} failed ({result.returncode}): "
            f"{detail or result.reason}"
        )
        if (
            device is not None
            and result.reason == "worker_crashed"
            and is_cuda_failure(result.returncode, result.stdout.decode(errors="replace"))
        ):
            raise _CudaAttemptError(msg)
        raise StemGenerationError(msg)
    stem_command_checkpoint()
    frames, outputs = _outputs(
        result.stdout,
        {
            "schema_version": 3,
            "device": token,
            "provider": provider,
            "graph_sha256": release.graph_sha256,
            "trace_sha256": release.trace_sha256,
            "approval_sha256": release.approval_sha256,
            "pcm_sha256": source.digest,
            "source": {"rate": _RATE, "channels": source.channels, "frames": source.frames},
            "overlap": float(overlap),
            "offsets": offsets,
        },
        device,
    )
    _require(frames == source.frames, "HTDemucs worker changed the decoded frame count")
    return frames, outputs


class HtdemucsOnnxBackend:
    """Run the approved native worker within the original App generation owner."""

    def __init__(self) -> None:
        self._provider_selector = StemProviderSelector()

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        """Decode once, run the selected provider and align the complete WAV set."""
        _validate_request(request)
        _require(
            request.separator == "demucs:htdemucs",
            "Native HTDemucs requires an HTDemucs request",
        )
        return self._generate(request, progress, _approved_release())

    def _generate(  # noqa: PLR0914, PLR0915
        self,
        request: StemGenerationRequest,
        progress: StemProgressCallback,
        release: _ReleaseIdentity,
    ) -> StemGenerationResult:
        encoder = request.encoder_executable
        if (
            encoder is None
            or not (images := supported_encoder_images(encoder))
            or bs_input_contract(encoder) is None
        ):
            msg = "Native HTDemucs requires the supported shared FFmpeg codec"
            raise StemGenerationError(msg)
        probe = encoder.with_name("ffprobe.exe")
        cuda_release = _approved_release(cuda=True)
        runtime_worker, runtime_crt = _runtime_build()
        installation = selected_htdemucs_installation(
            request.model_cache_dir.resolve(),
            release.graph_sha256,
            release.worker_sha256,
            release.worker_bytes,
            cuda_graph_sha256=cuda_release.graph_sha256,
            worker_executable=runtime_worker,
            crt_dir=runtime_crt,
        )
        root = installation.directory
        executable = root / NATIVE_DEMUCS_EXECUTABLE
        _require(
            root.is_dir()
            and {entry.name for entry in root.iterdir()}
            == {
                NATIVE_DEMUCS_EXECUTABLE,
                "htdemucs.onnx",
                "htdemucs-cuda.onnx",
                "ort",
                "cuda",
                "crt",
            },
            "Native HTDemucs worker directory differs from its required layout",
        )
        inventory = root / "crt" / "installed.json"
        installed_guard = SealedProcessingFiles([str(inventory)])
        crt = htdemucs_crt_witnesses(root)
        _require(
            selected_htdemucs_installation(
                request.model_cache_dir.resolve(),
                release.graph_sha256,
                release.worker_sha256,
                release.worker_bytes,
                cuda_graph_sha256=cuda_release.graph_sha256,
                worker_executable=runtime_worker,
                crt_dir=runtime_crt,
            )
            == installation,
            "HTDemucs installation selection changed before admission",
        )
        guard = SealedProcessingFiles([
            str(path)
            for path in (
                executable,
                root / "htdemucs.onnx",
                root / "htdemucs-cuda.onnx",
                *(root / "crt" / name for name, _, _ in crt),
                *images,
                probe,
            )
        ])
        witnesses = guard.witnesses()
        _require(
            witnesses[0][1:] == (installation.worker_sha256, installation.worker_bytes)
            and witnesses[1][1] == release.graph_sha256
            and witnesses[2][1] == cuda_release.graph_sha256
            and [w[1:] for w in witnesses[3 : 3 + len(crt)]]
            == [(digest, extent) for _, extent, digest in crt],
            "Native HTDemucs worker, graphs or CRT differ from their approval",
        )
        from flitzis_looper.controller import stem_provenance  # noqa: PLC0415

        package = Path(__file__).resolve().parents[1]
        host_paths = (
            *stem_provenance._host_processing_paths(request),
            Path(__file__).resolve(),
            package / "htdemucs_onnx_provisioning.py",
            package / "controller" / "stem_provenance.py",
            package / "analysis" / "beat_cuda_cache.py",
            package / "bs_onnx_vram.py",
        )
        codec = Path(__file__).resolve().parents[1] / "htdemucs_codec.py"
        host_guard = SealedProcessingFiles([str(path) for path in (*host_paths, codec)])
        host_execution = loaded_code_identity(host_paths)
        cache = stem_cache_io_path(request.cache_dir)
        cache.mkdir(parents=True, exist_ok=True)
        device = None if request.device_policy == "cpu" else _observed_cuda()
        cuda_identity = (
            _identity(("ort-1.24.4", device.driver_release, device.uuid.casefold()))
            if device is not None
            else None
        )
        availability = (
            CudaCapability(
                available=True, identity=cuda_identity, free_bytes=nvml_free_bytes(device.uuid)
            )
            if device is not None
            else CudaCapability(available=False)
        )
        plan = self._provider_selector.plan(request, "htdemucs", availability)
        overlap = _format_demucs_float(request.demucs_overlap)
        rng = SystemRandom()
        offsets = [rng.randint(0, _RATE // 2) for _ in range(request.demucs_shifts)]
        diagnostic = None
        with TemporaryDirectory(prefix=".htdemucs-", dir=cache) as name:
            scratch = Path(name)
            progress(0.05, "Decoding HTDemucs source")
            source = _stage(encoder, request, scratch / "input.f32")
            try:
                providers = tuple(device if item == "cuda" else None for item in plan.devices)
                for selected in providers:
                    token: StemDevice = "cuda" if selected is not None else "cpu"
                    with TemporaryDirectory(prefix=f".{token}-", dir=scratch) as attempt:
                        output = Path(attempt)
                        progress(0.1, f"Separating HTDemucs with ONNX on {token.upper()}")
                        try:
                            frames, outputs = _separate(
                                source,
                                executable,
                                output,
                                release if selected is None else cuda_release,
                                overlap,
                                offsets,
                                selected,
                            )
                        except _CudaAttemptError as failure:
                            diagnostic = str(failure)
                            continue
                        except (OSError, RuntimeError, ValueError) as failure:
                            if diagnostic is not None:
                                failure.add_note(f"First CUDA attempt: {diagnostic}")
                            raise
                        codec_verified, waves = _encode(encoder, output, frames, outputs)
                        try:
                            producer = self._producer(
                                release if selected is None else cuda_release,
                                (guard, host_guard),
                                host_execution,
                                selected,
                                source,
                                encoder_verified=codec_verified,
                            )
                            guard.verify()
                            installed_guard.verify()
                            execution = ExecutionReceipt(
                                backend="demucs",
                                model_name="htdemucs",
                                device=token,
                                cpu_fallback=diagnostic is not None,
                                intra_op_threads=1,
                                inter_op_threads=1,
                                cuda_device_identity=(
                                    cuda_identity if selected is not None and producer else None
                                ),
                            )
                            report_execution(progress, execution)
                            progress(0.85, "Aligning stem cache")
                            write_project_cache_artifacts(
                                output_root=output,
                                cache_dir=request.cache_dir,
                                target_shape=request.target_shape,
                                progress=progress,
                            )
                            waves.verify()
                        finally:
                            del waves
                        return StemGenerationResult(
                            backend_name="demucs",
                            model_name="htdemucs",
                            device=token,
                            cpu_fallback=diagnostic is not None,
                            diagnostic=diagnostic,
                            artifact_count=len(_SOURCES) + 1,
                            execution=execution,
                            encoder_verified=codec_verified,
                            decoder_verified=source.decoder_verified,
                            executed_producer_identity=producer,
                        )
            finally:
                source.release()
        msg = "Native HTDemucs did not complete an eligible attempt"
        raise StemGenerationError(msg)

    @staticmethod
    def _producer(
        release: _ReleaseIdentity,
        guards: tuple[SealedProcessingFiles, SealedProcessingFiles],
        host_execution: str | None,
        device: CudaDeviceIdentity | None,
        source: _StagedPcm,
        *,
        encoder_verified: bool,
    ) -> str | None:
        if not encoder_verified or not source.decoder_verified:
            return None
        from flitzis_looper.controller.stem_provenance import (  # noqa: PLC0415
            native_conversion_identity,
        )

        conversion = native_conversion_identity()
        if host_execution is None or conversion is None:
            return None
        if device is not None and _observed_cuda() != device:
            return None
        for guard in guards:
            guard.verify()
        return _identity((
            "htdemucs-onnx-v1",
            host_execution,
            conversion,
            tuple(
                tuple((digest, extent) for _, digest, extent in guard.witnesses())
                for guard in guards
            ),
            release.graph_sha256,
            release.approval_sha256,
            ("cpu", NATIVE_CPU_PACKAGE_SHA)
            if device is None
            else ("cuda", device.uuid, device.driver_release, NATIVE_CUDA_PACKAGE_SHA),
        ))
