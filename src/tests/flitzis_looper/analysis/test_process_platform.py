"""Unsupported hosts refuse offline execution before creating an unowned child."""

import os
import subprocess
from threading import Event
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.analysis.contracts import WorkerLimits
from flitzis_looper_audio import OfflineWorkTicket

if TYPE_CHECKING:
    from pathlib import Path


@pytest.fixture
def unsupported_platform(monkeypatch: pytest.MonkeyPatch) -> tuple[Mock, ...]:
    constructors = tuple(
        Mock(side_effect=AssertionError("Unsupported host allocated a child resource"))
        for _ in range(4)
    )
    job, linux, budget, launch = constructors
    monkeypatch.setattr(process, "os", SimpleNamespace(name="posix", environ=os.environ))
    monkeypatch.setattr(process, "sys", SimpleNamespace(platform="darwin"))
    monkeypatch.setattr(process, "WindowsJob", job)
    monkeypatch.setattr(process, "LinuxProcess", linux)
    monkeypatch.setattr(process, "OfflineChildMemoryReservation", budget)
    monkeypatch.setattr(
        process,
        "subprocess",
        SimpleNamespace(Popen=launch, DEVNULL=subprocess.DEVNULL, PIPE=subprocess.PIPE),
    )
    return constructors


def test_owned_command_refuses_unsupported_host_before_spawn_and_settles(
    tmp_path: Path, unsupported_platform: tuple[Mock, ...]
) -> None:
    retired = Event()
    with pytest.raises(OSError, match="process-tree supervision is unavailable on darwin"):
        process.run_owned_command(
            ["unused-worker"], cwd=tmp_path, env={}, cancel=Event(), retired=retired
        )
    assert retired.is_set()
    for constructor in unsupported_platform:
        constructor.assert_not_called()


def test_beat_refusal_releases_temporary_borrow_and_admission_for_next_request(
    tmp_path: Path, unsupported_platform: tuple[Mock, ...]
) -> None:
    command = process.WorkerCommand(
        tmp_path / "python", tmp_path / "worker.py", tmp_path / "model.ckpt", tmp_path
    )
    for _ in range(2):
        retired = Event()
        result = process.run_process(command, b"{}", Event(), WorkerLimits(), retired)
        assert result.reason == "worker_start_failed"
        assert result.resources_released
        assert retired.is_set()
        assert not list(tmp_path.glob("beat-request-*"))
        successor = OfflineWorkTicket()
        try:
            assert successor.try_acquire()
        finally:
            successor.close()
    for constructor in unsupported_platform:
        constructor.assert_not_called()
