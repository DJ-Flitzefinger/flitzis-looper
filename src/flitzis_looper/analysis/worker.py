"""Lazy local Beat This adapter; installation remains an explicit separate operation."""

import hashlib
import re
from dataclasses import dataclass
from threading import Event, current_thread, main_thread
from typing import TYPE_CHECKING

from flitzis_looper.analysis.artifacts import verify_installation
from flitzis_looper.analysis.contracts import (
    BeatComponentResult,
    BeatModelIdentity,
    BeatWorkerRequest,
    ComponentStatus,
    WorkerLimits,
    decode_response,
    encode_request,
)
from flitzis_looper.analysis.process import WorkerCommand, run_process

if TYPE_CHECKING:
    from pathlib import Path

_HASH_CHUNK_BYTES = 1024 * 1024


@dataclass(frozen=True, slots=True)
class WorkerConfiguration:
    """Local artifacts supplied by explicit setup or a protocol-test configuration."""

    interpreter: Path
    script: Path
    checkpoint: Path
    scratch_dir: Path
    model: BeatModelIdentity
    manifest: Path | None = None


class BeatWorkerAdapter:
    """Validate a borrowed request and supervise its optional local worker off-thread.

    Construction has no filesystem, process, network or optional-library activity.
    ``run`` is a blocking primitive for the existing background job owner. An
    unconfigured adapter returns unavailable; configured reference workers require
    their setup receipt in addition to a matching checkpoint digest.
    """

    def __init__(
        self,
        configuration: WorkerConfiguration | None = None,
        limits: WorkerLimits | None = None,
    ) -> None:
        self._configuration = configuration
        self._limits = limits or WorkerLimits()
        self.retired = Event()
        self.retired.set()

    def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult:
        """Return an independent outcome after teardown, or explicit retiring ownership."""
        if current_thread() is main_thread():
            msg = "beat analysis must run on a background supervisor"
            raise RuntimeError(msg)
        if cancel.is_set():
            return self._result(request, "cancelled", "cancelled")
        try:
            encoded = encode_request(request, self._limits)
        except OSError, ValueError:
            return self._result(request, "failed", "invalid_pcm_request")
        configuration = self._configuration
        if configuration is None:
            return self._result(request, "unavailable", "missing_worker")
        return self._run_configured(configuration, request, encoded, cancel)

    def _run_configured(
        self,
        configuration: WorkerConfiguration,
        request: BeatWorkerRequest,
        encoded: bytes,
        cancel: Event,
    ) -> BeatComponentResult:
        reason = self._preflight(configuration, request, cancel)
        if reason is not None:
            status: ComponentStatus = "cancelled" if reason == "cancelled" else "unavailable"
            return self._result(request, status, reason)
        command = WorkerCommand(
            configuration.interpreter,
            configuration.script,
            configuration.checkpoint,
            configuration.scratch_dir,
        )
        outcome = run_process(command, encoded, cancel, self._limits, self.retired)
        if outcome.reason != "ok":
            status = "cancelled" if outcome.reason == "cancelled" else "failed"
            return self._result(
                request, status, outcome.reason, resources_released=outcome.resources_released
            )
        if cancel.is_set():
            return self._result(request, "cancelled", "cancelled")
        try:
            predictions = decode_response(outcome.response, request, self._limits)
        except ValueError:
            return self._result(request, "failed", "invalid_worker_response")
        return BeatComponentResult(request.identity, request.model, "ready", "ready", predictions)

    def _preflight(
        self, configuration: WorkerConfiguration, request: BeatWorkerRequest, cancel: Event
    ) -> str | None:
        if configuration.model != request.model:
            return "model_identity_mismatch"
        if re.fullmatch(r"[0-9a-f]{64}", configuration.model.sha256) is None:
            return "unverified_model_manifest"
        paths = (
            configuration.interpreter,
            configuration.script,
            configuration.checkpoint,
            configuration.scratch_dir,
        )
        if any(not path.is_absolute() for path in paths):
            return "worker_paths_must_be_absolute"
        if not configuration.interpreter.is_file() or not configuration.script.is_file():
            return "missing_worker"
        reason = self._verify_installation(configuration, cancel)
        if reason is not None:
            return reason
        return self._verify_checkpoint(configuration, cancel)

    @staticmethod
    def _verify_installation(configuration: WorkerConfiguration, cancel: Event) -> str | None:
        manifest = configuration.manifest
        if manifest is None:
            if configuration.model.environment_id.startswith("uv-lock-sha256:"):
                return "missing_installation_manifest"
            return None
        expected = (
            manifest.parent / ".venv" / "Scripts" / "python.exe",
            manifest.parent / "worker.py",
            manifest.parent / "final0.ckpt",
        )
        actual = (configuration.interpreter, configuration.script, configuration.checkpoint)
        if not manifest.is_absolute() or actual != expected:
            return "installation_paths_mismatch"
        try:
            verify_installation(manifest, configuration.model, cancel)
        except OSError, ValueError:
            return "cancelled" if cancel.is_set() else "installation_provenance_mismatch"
        return None

    def _verify_checkpoint(self, configuration: WorkerConfiguration, cancel: Event) -> str | None:
        try:
            size = configuration.checkpoint.stat().st_size
            if size == 0 or size > self._limits.max_checkpoint_bytes:
                return "invalid_checkpoint_size"
            digest = _checkpoint_digest(configuration.checkpoint, cancel, self._limits)
        except FileNotFoundError:
            return "missing_checkpoint"
        except OSError:
            return "unreadable_checkpoint"
        if digest in {"cancelled", "invalid_checkpoint_size"}:
            return digest
        if digest != configuration.model.sha256:
            return "checkpoint_hash_mismatch"
        return "cancelled" if cancel.is_set() else None

    @staticmethod
    def _result(
        request: BeatWorkerRequest,
        status: ComponentStatus,
        reason: str,
        *,
        resources_released: bool = True,
    ) -> BeatComponentResult:
        return BeatComponentResult(
            request.identity, request.model, status, reason, resources_released=resources_released
        )


def _checkpoint_digest(path: Path, cancel: Event, limits: WorkerLimits) -> str:
    digest = hashlib.sha256()
    read_bytes = 0
    with path.open("rb") as checkpoint:
        while chunk := checkpoint.read(_HASH_CHUNK_BYTES):
            if cancel.is_set():
                return "cancelled"
            read_bytes += len(chunk)
            if read_bytes > limits.max_checkpoint_bytes:
                return "invalid_checkpoint_size"
            digest.update(chunk)
    return digest.hexdigest()
