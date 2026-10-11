"""Sticky job cancellation is isolated to commands on that admitted stem worker."""

import os
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis.process import OwnedCommandResult
from flitzis_looper.controller import stem_process
from flitzis_looper.controller.stem_generation import CommandResult, run_command
from flitzis_looper.controller.stem_process import (
    StemCommandCancelledError,
    stem_command_checkpoint,
    stem_command_scope,
)
from flitzis_looper.stem_cancellation import CANCEL_PATH_ENV

if TYPE_CHECKING:
    from collections.abc import Callable


def _wait_file(path: Path) -> None:
    deadline = time.monotonic() + 10
    while not path.is_file() or not path.stat().st_size:
        assert time.monotonic() < deadline, "stem command fixture failed to start"
        Event().wait(0.01)


def test_cancelled_scope_starts_no_command_and_resets_context_after_failure(tmp_path: Path) -> None:
    cancel = Event()
    cancel.set()
    marker = tmp_path / "must-not-run"
    with stem_command_scope(cancel), pytest.raises(StemCommandCancelledError):
        run_command(
            [
                sys.executable,
                "-I",
                "-c",
                f"from pathlib import Path; Path({str(marker)!r}).touch()",
            ],
            cwd=tmp_path,
            env=dict(os.environ),
        )
    assert not marker.exists()
    result = run_command(
        [sys.executable, "-I", "-c", "print('independent')"], cwd=tmp_path, env=dict(os.environ)
    )
    assert result == CommandResult(0, "independent", "")


def test_two_worker_scopes_cancel_only_their_own_running_command(tmp_path: Path) -> None:
    first_ready, second_ready, release = (
        tmp_path / name for name in ("first", "second", "release")
    )
    cancel_first = Event()
    cancel_second = Event()

    def run(cancel: Event, ready: Path) -> CommandResult:
        body = (
            "import time\nfrom pathlib import Path\n"
            f"Path({str(ready)!r}).write_text('ready')\n"
            f"while not Path({str(release)!r}).exists(): time.sleep(0.005)\n"
            "print('survived')\n"
        )
        with stem_command_scope(cancel):
            return run_command(
                [sys.executable, "-I", "-c", body], cwd=tmp_path, env=dict(os.environ)
            )

    with ThreadPoolExecutor(max_workers=2) as executor:
        first = executor.submit(run, cancel_first, first_ready)
        second = executor.submit(run, cancel_second, second_ready)
        try:
            _wait_file(first_ready)
            _wait_file(second_ready)
            cancel_first.set()
            with pytest.raises(StemCommandCancelledError):
                first.result(10)
            assert not second.done()
        finally:
            release.touch()
            cancel_first.set()
        assert second.result(10) == CommandResult(0, "survived", "")


def test_parent_checkpoint_observes_sticky_event_and_resets_nested_scope() -> None:
    outer, inner = Event(), Event()
    outer.set()
    with stem_command_scope(outer):
        with pytest.raises(StemCommandCancelledError):
            stem_command_checkpoint()
        with stem_command_scope(inner):
            stem_command_checkpoint()
            inner.set()
            with pytest.raises(StemCommandCancelledError):
                stem_command_checkpoint()
        with pytest.raises(StemCommandCancelledError):
            stem_command_checkpoint()
    stem_command_checkpoint()


@pytest.mark.parametrize(
    ("arguments", "own_channel"),
    [
        ([sys.executable, "-m", "flitzis_looper.demucs_streaming"], True),
        ([sys.executable, "-m", "flitzis_bs_roformer"], True),
        ([sys.executable, "-m", "foreign_module"], False),
        ([sys.executable, "-I", "-m", "flitzis_looper.demucs_streaming"], False),
        (["foreign-python.exe", "-m", "flitzis_bs_roformer"], False),
        (["ffprobe", "-v", "error"], False),
    ],
)
def test_exact_own_modules_receive_fresh_channel_and_foreign_commands_strip_inheritance(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    arguments: list[str],
    *,
    own_channel: bool,
) -> None:
    observed: list[str | None] = []
    environment = {**os.environ, CANCEL_PATH_ENV: "must-never-be-inherited"}
    private = tmp_path / "own-private"

    def run(
        args: list[str],
        cwd: Path,
        env: dict[str, str],
        cancel: Event,
        request_abort: Callable[[], None] | None = None,
    ) -> OwnedCommandResult:
        assert args == arguments
        assert cwd == tmp_path
        assert not cancel.is_set()
        observed.append(env.get(CANCEL_PATH_ENV))
        if own_channel:
            assert request_abort is not None
            channel = private / Path(env[CANCEL_PATH_ENV]).parent.name / "cancel"
            assert channel.parent.is_dir()
            assert not channel.exists()
            request_abort()
            assert channel.read_bytes() == b"cancel"
        else:
            assert request_abort is None
        return OwnedCommandResult("ok", 0, b"", b"")

    monkeypatch.setattr(stem_process, "_run_owned", run)
    with stem_command_scope(Event(), directory=private):
        stem_process.run_stem_command(arguments, cwd=tmp_path, env=environment)
        stem_process.run_stem_command(arguments, cwd=tmp_path, env=environment)
    assert environment[CANCEL_PATH_ENV] == "must-never-be-inherited"
    if own_channel:
        assert observed[0] != observed[1]
        assert not list(private.glob(".stem-command-*"))
    else:
        assert observed == [None, None]
        assert not private.exists()
