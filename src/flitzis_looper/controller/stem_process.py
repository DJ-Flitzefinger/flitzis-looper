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

from flitzis_looper.analysis.process import OwnedCommandResult, run_owned_command
from flitzis_looper.bs_input_provenance import BS_INPUT_RESULT
from flitzis_looper.encoder_provenance import ENCODER_RESULT
from flitzis_looper.stem_cancellation import CANCEL_PATH_ENV

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

    from flitzis_looper.controller.stem_resources import StemResourceGuard


@dataclass(frozen=True, slots=True)
class _StemCommandScope:
    cancel: Event
    directory: Path | None
    resources: StemResourceGuard | None


_COMMAND_SCOPE: ContextVar[_StemCommandScope | None] = ContextVar(
    "stem-command-scope", default=None
)
_RECEIPT_PREFIXES = (ENCODER_RESULT.encode("ascii"), BS_INPUT_RESULT.encode("ascii"))
_OWN_INFERENCE_MODULES = ("flitzis_looper.demucs_streaming", "flitzis_bs_roformer")


class StemCommandCancelledError(RuntimeError):
    """A stem command was cancelled after all its actual process reads settled."""


@contextmanager
def stem_command_scope(
    cancel: Event, *, directory: Path | None = None, resources: StemResourceGuard | None = None
) -> Iterator[None]:
    """Bind one job's cancellation and resource guard on its existing worker.

    Context tokens are reset even on backend failure. Separately admitted workers
    and injected fake command runners keep their independent state/signatures.
    """
    token = _COMMAND_SCOPE.set(_StemCommandScope(cancel, directory, resources))
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
    return run_owned_command(
        args,
        cwd=cwd,
        env=env,
        cancel=cancel,
        retired=Event(),
        receipt_prefixes=_RECEIPT_PREFIXES,
        request_abort=request_abort,
        abort_grace_seconds=0.4,
        stop_reason=stop_reason,
    )


def run_stem_command(args: list[str], *, cwd: Path, env: dict[str, str]) -> OwnedCommandResult:
    """Drain bounded diagnostics and hold caller custody until tree/read retirement."""
    stem_command_checkpoint()
    scope = _COMMAND_SCOPE.get()
    resources = scope.resources if scope is not None else None
    if resources is not None:
        resources.entry()
    cancel = scope.cancel if scope is not None else Event()
    stop_reason = resources.command_stop_reason if resources is not None else None
    child_env = dict(env)
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
    if resources is not None and result.reason in {"resource_pressure", "resource_unavailable"}:
        raise resources.failure(result.reason)
    if result.reason not in {"ok", "worker_crashed"}:
        msg = f"Stem command failed: {result.reason}"
        raise RuntimeError(msg)
    return result
