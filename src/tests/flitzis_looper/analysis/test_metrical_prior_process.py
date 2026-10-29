"""The opt-in prior observer cannot miss fast completion or release an owned reader."""

import os
import subprocess
import sys
from threading import Event
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.analysis.contracts import WorkerLimits
from flitzis_looper.analysis.metrical_prior_memory import PriorMemoryObserver

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("reason", ["resource_pressure", "resource_unavailable"])
def test_opt_in_observer_checks_before_fast_exit_acceptance(reason: str) -> None:
    child = Mock(spec=subprocess.Popen)
    child.poll.return_value = 0
    observer = Mock(return_value=reason)
    owner = process._OwnedProcess(child, None, None, Event())
    assert (
        process._monitor(owner, Event(), WorkerLimits(), observer, observe_before_exit=True)
        == reason
    )
    child.poll.assert_not_called()
    observer.assert_called_once_with()


def test_new_observer_failure_is_not_hidden_by_fast_zero_exit() -> None:
    child = Mock(spec=subprocess.Popen)
    child.poll.return_value = 0
    observer = Mock(side_effect=OSError("query failed"))
    owner = process._OwnedProcess(child, None, None, Event())
    assert (
        process._monitor(owner, Event(), WorkerLimits(), observer, observe_before_exit=True)
        == "resource_unavailable"
    )
    child.poll.assert_not_called()


@pytest.mark.parametrize("reason", ["resource_pressure", "resource_unavailable"])
def test_final_query_observes_a_peak_first_visible_at_exit_and_retires(reason: str) -> None:
    child = Mock(spec=subprocess.Popen)
    child.stdout = None
    child.stderr = None
    child.wait.return_value = 0
    exited = False

    def poll() -> int:
        nonlocal exited
        exited = True
        return 0

    def observe() -> str | None:
        if not exited:
            return None
        if reason == "resource_unavailable":
            message = "final peak query failed"
            raise OSError(message)
        return "resource_pressure"

    child.poll.side_effect = poll
    observer = Mock(side_effect=observe)
    retired = Event()
    owner = process._OwnedProcess(child, None, None, retired)
    limits = WorkerLimits()
    outcome = process._monitor(owner, Event(), limits, observer, observe_before_exit=True)
    assert outcome == reason
    assert observer.call_count == 2
    child.poll.assert_called_once_with()
    finished = process._finish(owner, outcome, limits)
    assert finished.reason == reason
    assert finished.resources_released
    assert retired.is_set()


@pytest.mark.parametrize("cancel_at", ["exit", "final_query"])
def test_cancel_wins_before_and_after_final_completed_process_query(cancel_at: str) -> None:
    child = Mock(spec=subprocess.Popen)
    cancel = Event()
    exited = False

    def poll() -> int:
        nonlocal exited
        exited = True
        if cancel_at == "exit":
            cancel.set()
        return 0

    def observe() -> str | None:
        if not exited:
            return None
        cancel.set()
        return "resource_pressure"

    child.poll.side_effect = poll
    observer = Mock(side_effect=observe)
    owner = process._OwnedProcess(child, None, None, Event())
    assert (
        process._monitor(owner, cancel, WorkerLimits(), observer, observe_before_exit=True)
        == "cancelled"
    )
    assert observer.call_count == (1 if cancel_at == "exit" else 2)
    child.poll.assert_called_once_with()


def test_cancel_precedes_the_new_observer_and_zero_exit() -> None:
    child = Mock(spec=subprocess.Popen)
    child.poll.return_value = 0
    observer = Mock(return_value="resource_pressure")
    cancel = Event()
    cancel.set()
    owner = process._OwnedProcess(child, None, None, Event())
    assert (
        process._monitor(owner, cancel, WorkerLimits(), observer, observe_before_exit=True)
        == "cancelled"
    )
    observer.assert_not_called()
    child.poll.assert_not_called()


def test_existing_default_monitor_keeps_completed_command_precedence() -> None:
    child = Mock(spec=subprocess.Popen)
    child.poll.return_value = 0
    observer = Mock(return_value="resource_pressure")
    owner = process._OwnedProcess(child, None, None, Event())
    assert process._monitor(owner, Event(), WorkerLimits(), observer) == "ok"
    observer.assert_not_called()


@pytest.mark.skipif(os.name != "nt", reason="suspended Windows owned process")
def test_observer_binding_failure_reaps_before_source_can_run(tmp_path: Path) -> None:
    marker = tmp_path / "must-not-run"
    retired = Event()
    binder = Mock(side_effect=OSError("query setup failed"))
    result = process.run_owned_command(
        [
            sys._base_executable,
            "-I",
            "-c",
            f"from pathlib import Path; Path({str(marker)!r}).touch()",
        ],
        cwd=tmp_path,
        env=dict(os.environ),
        cancel=Event(),
        retired=retired,
        on_process_started=binder,
        stop_reason=lambda: None,
    )
    assert result.reason == "resource_unavailable"
    assert retired.is_set()
    assert not marker.exists()
    binder.assert_called_once()


@pytest.mark.skipif(os.name != "nt", reason="actual Windows private commit/high-water counters")
def test_owned_direct_base_fast_exit_retains_private_peak_and_query_handle_closes(
    tmp_path: Path,
) -> None:
    observer = PriorMemoryObserver(0)
    retired = Event()
    try:
        result = process.run_owned_command(
            [
                sys._base_executable,
                "-I",
                "-c",
                "x=bytearray(8*1024*1024); x[::4096]=b'X'*(len(x)//4096)",
            ],
            cwd=tmp_path,
            env=dict(os.environ),
            cancel=Event(),
            retired=retired,
            on_process_started=observer.bind,
            stop_reason=observer.check,
        )
        assert retired.is_set()
        assert result.reason == "ok"
        assert result.returncode == 0
        assert observer.checks >= 1
        assert observer.max_peak_pagefile_bytes >= 8 * 1024 * 1024
        assert observer.native_live_bytes + observer.peak_bytes <= 536870912
    finally:
        observer.close()
    assert observer.handle is None
