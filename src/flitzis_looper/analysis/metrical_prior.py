"""One optional prior request under the existing Native source/process owner."""

import json
import os
import struct
from pathlib import Path
from threading import Event, current_thread, main_thread
from typing import TYPE_CHECKING, Annotated, Literal, Protocol
from uuid import uuid4

from pydantic import BaseModel, ConfigDict, Field, TypeAdapter

from flitzis_looper.analysis.app_tempo_prior import PriorEvidence
from flitzis_looper.analysis.contracts import AnalysisIdentity, WorkerLimits
from flitzis_looper.analysis.metrical_prior_memory import PriorMemoryObserver
from flitzis_looper.analysis.metrical_prior_setup import (
    E4_SHA256,
    FRONTEND_SHA256,
    INFERENCE_SHA256,
    NODE_SHA256,
    NUMERIC_PROFILE_SHA256,
    PROFILE_ID,
    PYTHON_DLL_SHA256,
    PYTHON_SHA256,
    REFERENCE_NODE_SHA256,
    PriorProfileToken,
    digest,
    metadata_array,
    metadata_integer,
    metadata_number,
    metadata_object,
    metadata_string,
    probe_metrical_prior_profile,
    read_json,
    verify_profile,
    write_json_new,
)
from flitzis_looper.analysis.neural_worker import _NativeCancellationEvent
from flitzis_looper.analysis.process import offline_work_scope, run_owned_command

if TYPE_CHECKING:
    from flitzis_looper.analysis.process import OwnedCommandResult

__all__ = ["probe_metrical_prior_profile", "run_metrical_prior"]

MAX_RESPONSE_BYTES = 2 * 1024 * 1024
KNOWN_FRONTEND_BASELINE_BYTES = 219414528
_IDENTITY = TypeAdapter(AnalysisIdentity)
_Digest = Annotated[str, Field(strict=True, pattern=r"^[0-9a-f]{64}$")]


class CancellationProbe(Protocol):
    def is_cancelled(self) -> bool: ...


class _Pcm(BaseModel):
    model_config = ConfigDict(extra="forbid")
    path: Annotated[str, Field(strict=True, min_length=1, max_length=4096)]
    source_sha256: _Digest
    mono_sha256: _Digest
    sample_rate_hz: Annotated[int, Field(strict=True, ge=8000, le=384000)]
    frame_count: Annotated[int, Field(strict=True, ge=1, le=2**63 - 1)]
    origin_seconds: Annotated[float, Field(strict=True, allow_inf_nan=False, ge=0, le=0)]
    dtype: Literal["float32-le"]
    channels: Annotated[int, Field(strict=True, ge=1, le=1)]
    downmix_revision: Literal["arithmetic-channel-mean-f64-v1"]


class _Request(BaseModel):
    model_config = ConfigDict(extra="forbid")
    schema_version: Annotated[int, Field(strict=True, ge=1, le=1)]
    identity: AnalysisIdentity
    installation_root: Annotated[str, Field(strict=True, min_length=1, max_length=4096)]
    profile_sha256: _Digest
    pcm: _Pcm
    request_directory: Annotated[str, Field(strict=True, min_length=1, max_length=4096)]
    native_live_bytes: Annotated[int, Field(strict=True, ge=0, le=536870912)]
    aggregate_limit_bytes: Annotated[int, Field(strict=True, ge=536870912, le=536870912)]


def _preflight(request: _Request) -> None:
    """Refuse known unfit geometry before imports/processes; actual guards still apply."""
    target = (
        request.pcm.frame_count * 11025 + request.pcm.sample_rate_hz - 1
    ) // request.pcm.sample_rate_hz
    columns = 1 + target // 512
    padded = max(256, columns)
    patches = 1 + (padded - 256) // 128
    if not 1 <= patches <= 1024:
        message = "prior_patch_limit"
        raise ValueError(message)
    elements, magnitude, mel = patches * 40 * 256, 513 * columns * 4, 40 * columns * 4
    output_chunk = (
        4096 * 11025 + request.pcm.sample_rate_hz - 1
    ) // request.pcm.sample_rate_hz + 65536
    phase = max(
        4096 * 4 + output_chunk * 4,
        target * 4 + magnitude + 2097152,
        2 * magnitude + 2 * mel + 40 * 513 * 4,
        mel + 40 * padded * 4 + elements * 4,
        elements * 42,
    )
    if (
        request.native_live_bytes + KNOWN_FRONTEND_BASELINE_BYTES + phase + 67108864 + 2097152
        > request.aggregate_limit_bytes
    ):
        message = "prior_known_frontend_memory_unavailable"
        raise ValueError(message)


def _environment() -> dict[str, str]:
    environment = dict(os.environ)
    for name in tuple(environment):
        if name.startswith((
            "NODE_",
            "NUMBA_",
            "LIBROSA_",
            "OPENBLAS_",
            "MKL_",
            "OMP_",
            "NUMEXPR_",
        )):
            environment.pop(name)
    for name in (
        "NODE_OPTIONS",
        "NODE_PATH",
        "NODE_COMPILE_CACHE",
        "NODE_V8_COVERAGE",
        "NODE_REPL_HISTORY",
        "PYTHONPATH",
        "PYTHONHOME",
        "PYTHONSTARTUP",
        "PYTHONUSERBASE",
        "PYTHONPYCACHEPREFIX",
    ):
        environment.pop(name, None)
    environment["NODE_DISABLE_COMPILE_CACHE"] = "1"
    for name in (
        "OMP_NUM_THREADS",
        "MKL_NUM_THREADS",
        "OPENBLAS_NUM_THREADS",
        "NUMEXPR_NUM_THREADS",
        "NUMBA_NUM_THREADS",
    ):
        environment[name] = "1"
    return environment


def _command(
    arguments: list[str], directory: Path, request: _Request, cancel: Event
) -> tuple[OwnedCommandResult, int]:
    with offline_work_scope(cancel):
        return _command_admitted(arguments, directory, request, cancel)


def _command_admitted(
    arguments: list[str], directory: Path, request: _Request, cancel: Event
) -> tuple[OwnedCommandResult, int]:
    observer = PriorMemoryObserver(request.native_live_bytes, request.aggregate_limit_bytes)
    retired = Event()
    try:
        outcome = run_owned_command(
            arguments,
            cwd=directory,
            env=_environment(),
            cancel=cancel,
            retired=retired,
            limits=WorkerLimits(
                timeout_seconds=21600.0, reap_timeout_seconds=0.5, max_response_bytes=65536
            ),
            on_process_started=observer.bind,
            stop_reason=observer.check,
        )
        while not retired.wait(0.02):
            pass
        if cancel.is_set():
            message = "cancelled"
            raise RuntimeError(message)
        if outcome.reason != "ok" or outcome.returncode != 0:
            message = f"prior_process_{outcome.reason}"
            raise RuntimeError(message)
        return outcome, observer.peak_bytes
    finally:
        while observer.handle is not None:
            try:
                observer.close()
            except OSError:
                Event().wait(0.02)


def _source_files(request: _Request, root: Path) -> Path:
    pcm = Path(request.pcm.path)
    if (
        not root.is_absolute()
        or not root.is_dir()
        or root.is_symlink()
        or root.is_junction()
        or not pcm.is_absolute()
    ):
        message = "Prior request needs original absolute source/temporary paths"
        raise ValueError(message)
    if request.identity.request_id == 0 or request.identity.source_generation == 0:
        message = "Prior source request identity is unavailable"
        raise ValueError(message)
    if pcm.stat().st_size != request.pcm.frame_count * 4:
        message = "Prior complete source PCM extent differs"
        raise ValueError(message)
    return pcm


def _write_frontend_input(request: _Request, owned: Path, cancel: Event) -> Path:
    custody = owned / "borrowed-source.json"
    write_json_new(
        custody,
        {
            "scope": "Borrowed original Native analysis mono; no new source authority",
            "identity": _IDENTITY.dump_python(request.identity),
            "input": request.pcm.model_dump(exclude={"path"}),
        },
    )
    input_path = owned / "frontend-input.json"
    write_json_new(
        input_path,
        {
            "schema_version": 1,
            "source_sha256": request.pcm.source_sha256,
            "source_sample_rate_hz": request.pcm.sample_rate_hz,
            "source_frame_count": request.pcm.frame_count,
            "source_channels": 1,
            "origin_seconds": 0,
            "mono_f32le_path": request.pcm.path,
            "mono_f32le_sha256": request.pcm.mono_sha256,
            "mono_downmix_revision": request.pcm.downmix_revision,
            "source_receipt_path": str(custody),
            "source_receipt_sha256": digest(custody, cancel),
        },
    )
    return input_path


def _frontend(
    request: _Request,
    token: PriorProfileToken,
    manifest: dict[str, object],
    owned: Path,
    cancel: Event,
) -> tuple[dict[str, object], Path, int]:
    input_path = _write_frontend_input(request, owned, cancel)
    output = owned / "frontend"
    root = token.installation_root
    python = metadata_object(manifest["python"])
    frontend = metadata_object(manifest["frontend"])
    _outcome, peak = _command(
        [
            str(root / metadata_string(python["path"])),
            "-I",
            "-S",
            "-B",
            str(root / metadata_string(frontend["path"])),
            "--manifest",
            str(input_path),
            "--packages-dir",
            str(root / "python-packages"),
            "--out",
            str(output),
            "--held-native-bytes",
            str(request.native_live_bytes),
        ],
        owned,
        request,
        cancel,
    )
    receipt, receipt_path = _check_frontend(request, output, cancel)
    return receipt, receipt_path, peak


def _check_frontend(
    request: _Request, output: Path, cancel: Event
) -> tuple[dict[str, object], Path]:
    if (output / "refusal.json").exists():
        message = "Prior frontend refused preparation"
        raise ValueError(message)
    receipt_path = output / "frontend-receipt.json"
    completion = read_json(output / "completion.json")
    if completion.get("status") != "complete" or completion.get("receipt_sha256") != digest(
        receipt_path, cancel
    ):
        message = "Prior frontend completion seal differs"
        raise ValueError(message)
    receipt = read_json(receipt_path)
    result = metadata_object(receipt["result"])
    features = metadata_object(result["features"])
    feature_file = output / "features.f32le"
    pcm_file = output / "complete11025.f32le"
    if (
        result["source_sha256"] != request.pcm.source_sha256
        or metadata_object(result["standardized_source_pcm"])["sha256"] != request.pcm.mono_sha256
    ):
        message = "Prior frontend source binding differs"
        raise ValueError(message)
    if (
        features["path"] != feature_file.name
        or digest(feature_file, cancel) != features["sha256"]
        or feature_file.stat().st_size != features["bytes"]
    ):
        message = "Prior frontend feature bytes differ"
        raise ValueError(message)
    if digest(pcm_file, cancel) != metadata_object(result["resampled_pcm"])["f32le_sha256"]:
        message = "Prior frontend complete resampled PCM differs"
        raise ValueError(message)
    if (
        metadata_object(result["memory_ledger"])["held_native_qp_bridge_bytes"]
        != request.native_live_bytes
    ):
        message = "Prior frontend held ownership charge differs"
        raise ValueError(message)
    return receipt, receipt_path


def _node_manifest(
    request: _Request,
    token: PriorProfileToken,
    receipt: dict[str, object],
    receipt_path: Path,
    owned: Path,
) -> tuple[Path, dict[str, object]]:
    result = metadata_object(receipt["result"])
    stats = metadata_object(result["global_normalization"])
    normalization = {
        "scope": "entire-overlapping-window-tensor",
        "precision": "float64",
        "roundtrip": "float16-to-float32",
        "mean_f64_le_hex": struct.pack("<d", metadata_number(stats["mean"])).hex(),
        "std_f64_le_hex": struct.pack("<d", metadata_number(stats["std"])).hex(),
    }
    features = metadata_object(result["features"])
    reference = {"path": str(receipt_path), "sha256": digest(receipt_path)}
    identity = _IDENTITY.dump_python(request.identity)
    node_identity = {
        key: value if key == "source_id" else str(value) for key, value in identity.items()
    }
    node: dict[str, object] = {
        "schema_version": 1,
        "identity": node_identity,
        "profile": {
            "profile_sha256": token.profile_sha256,
            "numeric_profile_sha256": token.numeric_profile_sha256,
        },
        "frontend_receipt": reference,
        "cases": [
            {
                "id": request.pcm.source_sha256,
                "source_sha256": request.pcm.source_sha256,
                "pcm_sha256": request.pcm.mono_sha256,
                "shape": features["shape"],
                "dtype": "float32-le",
                "features": {
                    "path": str(receipt_path.parent / metadata_string(features["path"])),
                    "sha256": features["sha256"],
                    "bytes": features["bytes"],
                },
                "normalization": normalization,
                "provenance_receipt": reference,
            }
        ],
    }
    path = owned / "node-input.json"
    write_json_new(path, node)
    return path, node


def _infer(
    request: _Request,
    token: PriorProfileToken,
    manifest: dict[str, object],
    receipt: dict[str, object],
    receipt_path: Path,
    owned: Path,
    cancel: Event,
) -> tuple[dict[str, object], int]:
    path, node_input = _node_manifest(request, token, receipt, receipt_path, owned)
    root, output = token.installation_root, owned / "node-result.json"
    node = metadata_object(manifest["node"])
    inference = metadata_object(manifest["inference"])
    _outcome, peak = _command(
        [
            str(root / metadata_string(node["path"])),
            str(root / metadata_string(inference["path"])),
            "--features",
            str(path),
            "--manifest-sha256",
            digest(path, cancel),
            "--batch-size",
            "2",
            "--held-native-bytes",
            str(request.native_live_bytes),
            "--request-root",
            str(owned),
            "--installation-root",
            str(root),
            "--output",
            str(output),
            "--tfjs-module",
            str(root / "node_modules" / "@tensorflow" / "tfjs"),
            "--model-dir",
            str(root / "upstream" / "deeptemp-k16-3-tfjs"),
        ],
        owned,
        request,
        cancel,
    )
    result = read_json(output, MAX_RESPONSE_BYTES)
    if (
        result.get("status") != "completed"
        or result.get("resources_released") is not True
        or result.get("identity") != node_input["identity"]
        or result.get("profile") != node_input["profile"]
    ):
        message = "Prior classifier response identity/completion differs"
        raise ValueError(message)
    if (
        result.get("input_manifest_sha256") != digest(path, cancel)
        or result.get("runner_sha256") != INFERENCE_SHA256
        or result.get("numeric_profile_sha256") != NUMERIC_PROFILE_SHA256
    ):
        message = "Prior classifier profile/input seal differs"
        raise ValueError(message)
    item = metadata_object(metadata_array(node_input["cases"])[0])
    if (
        any(
            result.get(key) != item[field]
            for key, field in (
                ("source_sha256", "source_sha256"),
                ("pcm_sha256", "pcm_sha256"),
                ("shape", "shape"),
                ("normalization", "normalization"),
            )
        )
        or result.get("feature_sha256") != metadata_object(item["features"])["sha256"]
    ):
        message = "Prior classifier frontend echo differs"
        raise ValueError(message)
    return result, peak


def _evidence(
    request: _Request,
    token: PriorProfileToken,
    manifest: dict[str, object],
    frontend: dict[str, object],
    classifier: dict[str, object],
    frontend_peak: int,
    classifier_peak: int,
) -> PriorEvidence:
    result = metadata_object(frontend["result"])
    model = metadata_object(classifier["model_pins"])
    profile = {
        "profile_id": PROFILE_ID,
        "profile_sha256": token.profile_sha256,
        "numeric_profile_sha256": token.numeric_profile_sha256,
        "python_version": "3.12.13",
        "python_executable_sha256": PYTHON_SHA256,
        "python_dll_sha256": PYTHON_DLL_SHA256,
        "node_version": "v25.5.0",
        "node_executable_sha256": NODE_SHA256,
        "tfjs_version": "4.22.0",
        "graph_sha256": model["graph"],
        "shard_sha256": model["shard"],
        "frontend_reference_sha256": E4_SHA256,
        "frontend_runner_sha256": FRONTEND_SHA256,
        "inference_reference_sha256": REFERENCE_NODE_SHA256,
        "inference_runner_sha256": metadata_object(manifest["inference"])["sha256"],
    }
    pcm = metadata_object(result["resampled_pcm"])
    features = metadata_object(result["features"])
    ledger = metadata_object(result["memory_ledger"])
    return PriorEvidence.model_validate(
        {
            "profile": profile,
            "input": request.pcm.model_dump(exclude={"path", "dtype", "channels"}),
            "frontend": {
                "frontend_id": "tempocnn-author-482c64ad-librosa011-global-std-f16-v1",
                "sample_rate_hz": 11025,
                "frame_count": pcm["frame_count"],
                "origin_seconds": 0.0,
                "mono_sha256": pcm["f32le_sha256"],
                "feature_shape": features["shape"],
                "feature_sha256": features["sha256"],
                "normalization": classifier["normalization"],
            },
            "probabilities": classifier["probabilities"],
            "global": classifier["global"],
            "execution": {"origin": "computed", "device": "cpu"},
            "resources": {
                "native_live_bytes": request.native_live_bytes,
                "aggregate_limit_bytes": request.aggregate_limit_bytes,
                "frontend_admitted_child_peak_bytes": metadata_integer(
                    ledger["admitted_child_peak_bytes"]
                ),
                "frontend_observed_child_peak_bytes": frontend_peak,
                "classifier_observed_child_peak_bytes": classifier_peak,
                "measurement": "windows-private-and-working-set-peaks",
                "aggregate_claim": True,
            },
        },
        strict=True,
    )


def _require_not_cancelled(cancel: Event) -> None:
    if cancel.is_set():
        message = "cancelled"
        raise RuntimeError(message)


def _request_root(request: _Request, token: PriorProfileToken) -> Path:
    root = Path(request.request_directory)
    _source_files(request, root)
    if token.installation_root.is_relative_to(root) or root.is_relative_to(token.installation_root):
        message = "Prior request and immutable profile must be disjoint"
        raise ValueError(message)
    return root


def _failure_reason(error: Exception, stage: str) -> str:
    reason = str(error)
    if not reason.startswith(("prior_process_", "prior_known_", "prior_patch_")):
        reason = f"prior_{stage}_unavailable"
    return reason[:1024]


def _execute_request(
    request: _Request, token: PriorProfileToken, cancel: Event, envelope: dict[str, object]
) -> None:
    stage = "preflight"
    try:
        _require_not_cancelled(cancel)
        _preflight(request)
        stage = "profile"
        manifest = verify_profile(token, cancel)
        owned = _request_root(request, token) / f"prior-{uuid4().hex}"
        owned.mkdir()
        stage = "frontend"
        frontend, receipt_path, frontend_peak = _frontend(request, token, manifest, owned, cancel)
        _require_not_cancelled(cancel)
        stage = "classifier"
        classifier, classifier_peak = _infer(
            request, token, manifest, frontend, receipt_path, owned, cancel
        )
        stage = "evidence"
        evidence = _evidence(
            request, token, manifest, frontend, classifier, frontend_peak, classifier_peak
        )
        envelope.update(
            status="completed",
            reason="completed",
            prior=evidence.model_dump(mode="json", by_alias=True),
        )
    except (OSError, RuntimeError, TypeError, ValueError, KeyError) as error:
        envelope.update(status="failed", reason=_failure_reason(error, stage))


def run_metrical_prior(request_json: str, cancel_probe: CancellationProbe) -> str:
    """Blocking primitive called only by the existing admitted Native analysis worker."""
    if current_thread() is main_thread():
        message = "Nominal prior must run on the existing background worker"
        raise RuntimeError(message)
    if len(request_json.encode("utf-8")) > 32768:
        message = "Prior request exceeds its bounded metadata size"
        raise ValueError(message)
    request = _Request.model_validate_json(request_json, strict=True)
    token = PriorProfileToken(
        Path(request.installation_root), request.profile_sha256, NUMERIC_PROFILE_SHA256
    )
    cancel = _NativeCancellationEvent(cancel_probe)
    envelope: dict[str, object] = {
        "schema_version": 1,
        "identity": _IDENTITY.dump_python(request.identity),
        "input": request.pcm.model_dump(exclude={"path"}),
        "profile": token.wire(),
        "status": "failed",
        "reason": "prior_unavailable",
        "resources_released": True,
    }
    _execute_request(request, token, cancel, envelope)
    if cancel.is_set():
        envelope.update(status="cancelled", reason="cancelled")
        envelope.pop("prior", None)
    encoded = json.dumps(envelope, allow_nan=False, separators=(",", ":"))
    if len(encoded.encode("utf-8")) > MAX_RESPONSE_BYTES:
        envelope.update(status="failed", reason="prior_response_limit")
        envelope.pop("prior", None)
        encoded = json.dumps(envelope, allow_nan=False, separators=(",", ":"))
    return encoded
