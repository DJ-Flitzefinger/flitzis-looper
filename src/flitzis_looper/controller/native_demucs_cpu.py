"""App Stem Worker adapter for the explicitly selected Windows HTDemucs CPU release."""

import hashlib
import json
import os
import re
from pathlib import Path
from random import SystemRandom
from tempfile import TemporaryDirectory
from typing import NamedTuple

from flitzis_looper.controller.stem_artifacts import write_project_cache_artifacts
from flitzis_looper.controller.stem_generation import (
    StemGenerationError,
    StemGenerationRequest,
    StemGenerationResult,
    StemProgressCallback,
    _validate_request,
)
from flitzis_looper.controller.stem_process import (
    NATIVE_DEMUCS_EXECUTABLE,
    NATIVE_DEMUCS_LIMIT,
    run_stem_command,
    stem_command_checkpoint,
)
from flitzis_looper.encoder_provenance import supported_encoder_images
from flitzis_looper.offline_runtime import ffmpeg_command
from flitzis_looper.pcm16_wav import PcmWavReader
from flitzis_looper.stem_execution_protocol import ExecutionReceipt, report_execution
from flitzis_looper_audio import SealedProcessingFiles

_RELEASE = Path(__file__).resolve().parents[3] / "workers/htdemucs/native_cpu/release.json"
_WORKER_PIN = _RELEASE.with_name("worker.json")
_SOURCES = ("drums", "bass", "other", "vocals")
_RATE = 44100
_MAX_RATE = 768000
_MAX_DECODE_CHANNELS = 8192  # One frame must fit the worker's 32 KiB read block.
_PACKAGE_SHA = "7a5e216d9cc35418e8c38afa293ece04e86abc63153553fe56e81aa901cf2319"
_MANIFEST_SHA = "4929eb4f6a3aebe2c117850a2e1354c48bc004bfa5ff4d805c2ce0ecbef2915d"
_DIGEST = re.compile(r"[0-9a-f]{64}")


def _require(condition: object, message: str) -> None:
    if not condition:
        raise StemGenerationError(message)


class _ReleaseIdentity(NamedTuple):
    graph_sha256: str
    trace_sha256: str
    approval_sha256: str
    worker_sha256: str
    worker_bytes: int


def _approved_release() -> _ReleaseIdentity:
    _require(os.name == "nt", "Native HTDemucs CPU requires Windows")
    raw, value = _read_approval(_RELEASE)
    if not isinstance(value, dict):
        msg = "The pinned HTDemucs learned Scan64 graph and phase admission are unavailable"
        raise StemGenerationError(msg)
    if set(value) != {
        "graph_bytes",
        "graph_sha256",
        "trace_sha256",
        "attention_sha256",
        "load_extra_bytes",
        "run_extra_bytes",
    } or not all(
        type(value[key]) is int and value[key] > 0
        for key in (
            "graph_bytes",
            "load_extra_bytes",
            "run_extra_bytes",
        )
    ):
        msg = "Native HTDemucs release phase admission is invalid"
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


def _run(args: list[str], scratch: Path, *, env: dict[str, str], native: bool = False) -> bytes:
    result = run_stem_command(args, cwd=scratch, env=env, native_demucs=native)
    if result.reason != "ok" or result.returncode != 0:
        detail = result.stderr.decode(errors="replace").strip()[-1500:]
        msg = f"{Path(args[0]).name} failed ({result.returncode}): {detail or result.reason}"
        raise StemGenerationError(msg)
    stem_command_checkpoint()
    return result.stdout


def _decode(encoder: Path, source: Path, pcm: Path) -> tuple[int, int, int]:
    env = dict(os.environ)
    probe = encoder.with_name("ffprobe.exe")
    stdout = _run(
        [
            str(probe),
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=sample_rate,channels",
            "-of",
            "json",
            str(source),
        ],
        pcm.parent,
        env=env,
    )
    try:
        [stream] = json.loads(stdout)["streams"]
        rate, channels = int(stream["sample_rate"]), stream["channels"]
    except (KeyError, TypeError, ValueError) as err:
        msg = "Cannot establish the HTDemucs source audio shape"
        raise StemGenerationError(msg) from err
    if (
        type(channels) is not int
        or not 0 < channels <= _MAX_DECODE_CHANNELS
        or not 0 < rate <= _MAX_RATE
    ):
        msg = "Native HTDemucs CPU source channel count or rate is outside its supported range"
        raise StemGenerationError(msg)
    _run(
        ffmpeg_command([
            str(encoder),
            "-nostdin",
            "-n",
            "-v",
            "error",
            "-xerror",
            "-i",
            str(source),
            "-map",
            "0:a:0",
            "-ar",
            str(rate),
            "-ac",
            str(channels),
            "-c:a",
            "pcm_f32le",
            "-f",
            "f32le",
            "-fs",
            str(NATIVE_DEMUCS_LIMIT),
            str(pcm),
        ]),
        pcm.parent,
        env=env,
    )
    extent = pcm.stat().st_size
    frames, remainder = divmod(extent, channels * 4)
    _require(
        frames > 0 and remainder == 0 and extent < NATIVE_DEMUCS_LIMIT,
        "HTDemucs source PCM is incomplete or exceeds the staging bound",
    )
    return rate, channels, frames


def _outputs(stdout: bytes, expected: dict[str, object]) -> tuple[int, dict[str, str]]:
    receipt = json.loads(stdout)
    if not isinstance(receipt, dict):
        msg = "Native HTDemucs worker returned an invalid receipt"
        raise StemGenerationError(msg)
    runtime = receipt.get("runtime")
    outputs = receipt.get("outputs")
    frames = receipt.get("frames")
    loaded = runtime.get("loaded_package") if isinstance(runtime, dict) else None
    package = runtime.get("declared_package") if isinstance(runtime, dict) else None
    _require(
        set(receipt) == {*expected, "frames", "job_bytes", "outputs", "runtime"}
        and type(frames) is int
        and 0 < frames <= NATIVE_DEMUCS_LIMIT // (8 * len(_SOURCES))
        and all(receipt.get(key) == value for key, value in expected.items())
        and type(receipt["job_bytes"]) is int
        and 0 < receipt["job_bytes"] <= NATIVE_DEMUCS_LIMIT
        and isinstance(runtime, dict)
        and runtime.get("runtime_version") == "1.24.4"
        and runtime.get("ort_api") == 24
        and runtime.get("manifest_sha256") == _MANIFEST_SHA
        and isinstance(loaded, dict)
        and loaded.get("policy") == "package-system32"
        and isinstance(package, dict)
        and package.get("manifest_sha256") == _PACKAGE_SHA,
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
    # Julius uses float32 rounding and a convolution extent clamp. Retain
    # the exact Native geometry and verify all PCM/WAV extents against it.
    return frames, outputs


def _encode(encoder: Path, scratch: Path, frames: int, outputs: dict[str, str]) -> None:
    paths = [scratch / f"{name}.f32" for name in _SOURCES]
    guard = SealedProcessingFiles([str(path) for path in paths])
    _require(
        [(digest, size) for _, digest, size in guard.witnesses()]
        == [(outputs[name], frames * 8) for name in _SOURCES],
        "Native HTDemucs output PCM changed after the worker exited",
    )
    for name, raw in zip(_SOURCES, paths, strict=True):
        wav = scratch / f"{name}.wav"
        _run(
            ffmpeg_command([
                str(encoder),
                "-nostdin",
                "-n",
                "-v",
                "error",
                "-f",
                "f32le",
                "-ar",
                str(_RATE),
                "-ac",
                "2",
                "-i",
                str(raw),
                "-c:a",
                "pcm_s16le",
                "-rf64",
                "auto",
                str(wav),
            ]),
            scratch,
            env=dict(os.environ),
        )
        with PcmWavReader(wav) as reader:
            _require(
                (
                    reader.getnframes(),
                    reader.getframerate(),
                    reader.getnchannels(),
                    reader.getsampwidth(),
                )
                == (frames, _RATE, 2, 2),
                f"Invalid native HTDemucs PCM16 output shape for {name}",
            )
    guard.verify()


def _separate(
    request: StemGenerationRequest,
    encoder: Path,
    executable: Path,
    scratch: Path,
    release: _ReleaseIdentity,
) -> tuple[int, dict[str, str]]:
    pcm = scratch / "input.f32"
    rate, channels, frames = _decode(encoder, request.source_path, pcm)
    pcm_guard = SealedProcessingFiles([str(pcm)])
    digest, size = pcm_guard.witnesses()[0][1:]
    _require(size == frames * channels * 4, "HTDemucs staged source PCM extent changed")
    random = SystemRandom()
    offsets = [random.randint(0, _RATE // 2) for _ in range(request.demucs_shifts)]
    env = dict(os.environ)
    env.update(
        FLITZIS_TRIAL_ORT_LOAD_POLICY="package-system32",
        FLITZIS_TRIAL_ORT_PACKAGE_SHA256=_PACKAGE_SHA,
        FLITZIS_TRIAL_ORT_PACKAGE=str(executable.parent / "ort/step23-package.json"),
    )
    stdout = _run(
        [
            str(executable),
            str(pcm),
            str(rate),
            str(channels),
            str(frames),
            digest,
            str(request.demucs_overlap),
            json.dumps(offsets),
            str(scratch),
        ],
        scratch,
        env=env,
        native=True,
    )
    pcm_guard.verify()
    return _outputs(
        stdout,
        {
            "schema_version": 1,
            "device": "cpu",
            "graph_sha256": release.graph_sha256,
            "trace_sha256": release.trace_sha256,
            "approval_sha256": release.approval_sha256,
            "pcm_sha256": digest,
            "source": {"rate": rate, "channels": channels, "frames": frames},
            "overlap": request.demucs_overlap,
            "offsets": offsets,
        },
    )


class NativeDemucsCpuBackend:
    """Run the approved native worker within the original App generation owner."""

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        """Decode once, separate in an admitted CPU Job and align private WAVs."""
        _validate_request(request)
        _require(
            request.separator == "demucs:htdemucs",
            "Native HTDemucs CPU requires an HTDemucs request",
        )
        return self._generate(request, progress, _approved_release())

    def _generate(
        self,
        request: StemGenerationRequest,
        progress: StemProgressCallback,
        release: _ReleaseIdentity,
    ) -> StemGenerationResult:
        encoder = request.encoder_executable
        if encoder is None or not (images := supported_encoder_images(encoder)):
            msg = "Native HTDemucs CPU requires the supported shared FFmpeg codec"
            raise StemGenerationError(msg)
        probe = encoder.with_name("ffprobe.exe")
        root = request.model_cache_dir.resolve() / "native-cpu"
        executable = root / NATIVE_DEMUCS_EXECUTABLE
        _require(
            root.is_dir()
            and {entry.name for entry in root.iterdir()}
            == {
                NATIVE_DEMUCS_EXECUTABLE,
                "htdemucs.onnx",
                "ort",
            },
            "Native HTDemucs worker directory differs from its required layout",
        )
        guard = SealedProcessingFiles([str(path) for path in (executable, *images, probe)])
        _require(
            guard.witnesses()[0][1:] == (release.worker_sha256, release.worker_bytes),
            "Native HTDemucs worker differs from its approved executable",
        )
        request.cache_dir.mkdir(parents=True, exist_ok=True)
        progress(0.05, "Separating stems with Native HTDemucs CPU")
        with TemporaryDirectory(prefix=".htdemucs-cpu-", dir=request.cache_dir) as name:
            scratch = Path(name).absolute()
            converted_frames, outputs = _separate(request, encoder, executable, scratch, release)
            _encode(encoder, scratch, converted_frames, outputs)
            guard.verify()
            execution = ExecutionReceipt(
                backend="demucs",
                model_name="htdemucs",
                device="cpu",
                intra_op_threads=1,
                inter_op_threads=1,
            )
            report_execution(progress, execution)
            progress(0.85, "Aligning stem cache")
            write_project_cache_artifacts(
                output_root=scratch,
                cache_dir=request.cache_dir,
                target_shape=request.target_shape,
                progress=progress,
            )
        return StemGenerationResult(
            backend_name="demucs",
            model_name="htdemucs",
            device="cpu",
            cpu_fallback=False,
            artifact_count=len(_SOURCES) + 1,
            execution=execution,
        )
