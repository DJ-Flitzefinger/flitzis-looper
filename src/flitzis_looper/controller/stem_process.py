"""Per-worker command cancellation and actual read settlement for stem backends."""

import os
import sys
from contextlib import contextmanager
from contextvars import ContextVar
from dataclasses import dataclass
from functools import partial
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Event
from typing import TYPE_CHECKING

from flitzis_looper.analysis.process import (
    IndependentReceipts,
    OfflineWorkCancelledError,
    OwnedCommandResult,
    offline_work_scope,
    run_owned_command,
)
from flitzis_looper.bs_input_provenance import BS_INPUT_RESULT
from flitzis_looper.encoder_provenance import ENCODER_RESULT
from flitzis_looper.inference_provider import CUDA_FAILURE_RECEIPT
from flitzis_looper.offline_runtime import child_environment
from flitzis_looper.stem_cancellation import CANCEL_PATH_ENV
from flitzis_looper.stem_execution import STEM_EXECUTION_RESULT
from flitzis_looper_audio import OfflineChildMemoryReservation

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

    from flitzis_looper.controller.stem_resources import StemResourceGuard


@dataclass(frozen=True, slots=True)
class _StemCommandScope:
    cancel: Event
    directory: Path | None
    resources: StemResourceGuard | None
    playback: Event | None


_COMMAND_SCOPE: ContextVar[_StemCommandScope | None] = ContextVar(
    "stem-command-scope", default=None
)
_RECEIPT_PREFIXES = (
    CUDA_FAILURE_RECEIPT.encode("ascii"),
    ENCODER_RESULT.encode("ascii"),
    BS_INPUT_RESULT.encode("ascii"),
    STEM_EXECUTION_RESULT.encode("ascii"),
)
_OWN_INFERENCE_MODULES = ("flitzis_looper.demucs_streaming", "flitzis_bs_roformer")


class StemCommandCancelledError(RuntimeError):
    """A stem command was cancelled after all its actual process reads settled."""


@contextmanager
def stem_command_scope(
    cancel: Event,
    *,
    directory: Path | None = None,
    resources: StemResourceGuard | None = None,
    playback: Event | None = None,
) -> Iterator[None]:
    """Bind one job's cancellation and resource guard on its existing worker.

    Context tokens are reset even on backend failure. Separately admitted workers
    and injected fake command runners keep their independent state/signatures.
    """
    token = _COMMAND_SCOPE.set(_StemCommandScope(cancel, directory, resources, playback))
    try:
        yield
    finally:
        _COMMAND_SCOPE.reset(token)


def stem_command_checkpoint() -> None:
    """Observe only the current worker's sticky Event, with no file/source access."""
    scope = _COMMAND_SCOPE.get()
    if scope is not None and scope.cancel.is_set():
        msg = "Stem command cancelled"
        raise StemCommandCancelledError(msg)
    if scope is not None and scope.playback is not None and scope.playback.is_set():
        message = "Stem generation stopped for playback headroom; retry after playback stops"
        raise RuntimeError(message)


def stem_conversion_checkpoint() -> None:
    """Recheck the existing job's cancellation and entry/recovery memory policy."""
    stem_command_checkpoint()
    scope = _COMMAND_SCOPE.get()
    if scope is not None and scope.resources is not None:
        scope.resources.entry()


def _own_inference(args: list[str]) -> bool:
    return (
        len(args) >= 3
        and os.path.normcase(os.path.abspath(args[0]))
        == os.path.normcase(os.path.abspath(sys.executable))
        and args[1] == "-m"
        and args[2] in _OWN_INFERENCE_MODULES
    )


def _request_abort(marker: Path) -> None:
    with marker.open("xb") as handle:
        handle.write(b"cancel")


def _run_owned(
    args: list[str],
    cwd: Path,
    env: dict[str, str],
    cancel: Event,
    request_abort: Callable[[], None] | None = None,
    stop_reason: Callable[[], str | None] | None = None,
) -> OwnedCommandResult:
    try:
        with offline_work_scope(cancel):
            return _run_admitted(args, cwd, env, cancel, request_abort, stop_reason)
    except OfflineWorkCancelledError as error:
        raise StemCommandCancelledError(str(error)) from None


def _run_admitted(
    args: list[str],
    cwd: Path,
    env: dict[str, str],
    cancel: Event,
    request_abort: Callable[[], None] | None,
    stop_reason: Callable[[], str | None] | None,
) -> OwnedCommandResult:
    scope = _COMMAND_SCOPE.get()
    budget = None
    with offline_work_scope(cancel):
        try:
            stem_command_checkpoint()
            # No unsupported platform is labelled as enforced. On Windows the
            # promise includes existing Native deltas BEFORE suspended launch.
            if os.name == "nt" and scope is not None and scope.resources is not None:
                scope.resources.entry()
                budget = OfflineChildMemoryReservation()
            return run_owned_command(
                args,
                cwd=cwd,
                env=env,
                cancel=cancel,
                retired=Event(),
                receipt_prefixes=IndependentReceipts(_RECEIPT_PREFIXES),
                request_abort=request_abort,
                abort_grace_seconds=0.4,
                stop_reason=stop_reason,
                commit_limit_bytes=budget.limit_bytes if budget is not None else None,
            )
        finally:
            # run_owned_command cannot return before tree AND pipe readers end.
            if budget is not None:
                budget.close()


def run_stem_command(args: list[str], *, cwd: Path, env: dict[str, str]) -> OwnedCommandResult:
    """Drain bounded diagnostics and hold caller custody until tree/read retirement."""
    stem_command_checkpoint()
    scope = _COMMAND_SCOPE.get()
    resources = scope.resources if scope is not None else None
    if resources is not None:
        resources.entry()
    cancel = scope.cancel if scope is not None else Event()

    def stop() -> str | None:
        if scope is not None and scope.playback is not None and scope.playback.is_set():
            return "audio_headroom"
        return resources.command_stop_reason() if resources is not None else None

    stop_reason = (
        stop
        if resources is not None or (scope is not None and scope.playback is not None)
        else None
    )
    child_env = child_environment(env)
    child_env.pop(CANCEL_PATH_ENV, None)
    if scope is not None and scope.directory is not None and _own_inference(args):
        scope.directory.mkdir(parents=True, exist_ok=True)
        with TemporaryDirectory(prefix=".stem-command-", dir=scope.directory) as name:
            marker = Path(name).absolute() / "cancel"
            child_env[CANCEL_PATH_ENV] = str(marker)
            result = _run_owned(
                args, cwd, child_env, cancel, partial(_request_abort, marker), stop_reason
            )
    else:
        result = _run_owned(args, cwd, child_env, cancel, stop_reason=stop_reason)
    if result.reason == "cancelled":
        msg = "Stem command cancelled"
        raise StemCommandCancelledError(msg)
    if result.reason == "audio_headroom":
        message = "Stem generation stopped for playback headroom; retry after playback stops"
        raise RuntimeError(message)
    if resources is not None and result.reason in {"resource_pressure", "resource_unavailable"}:
        raise resources.failure(result.reason)
    if result.reason not in {"ok", "worker_crashed"}:
        msg = f"Stem command failed: {result.reason}"
        raise RuntimeError(msg)
    return result
