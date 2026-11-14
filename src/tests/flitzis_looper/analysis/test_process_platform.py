"""Unsupported hosts refuse offline execution before creating an unowned child."""

import os
import subprocess
from threading import Event
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis import process

if TYPE_CHECKING:
    from pathlib import Path


@pytest.fixture
def unsupported_platform(monkeypatch: pytest.MonkeyPatch) -> tuple[Mock, ...]:
    constructors = tuple(
        Mock(side_effect=AssertionError("Unsupported host allocated a child resource"))
        for _ in range(3)
    )
    job, linux, launch = constructors
    monkeypatch.setattr(process, "os", SimpleNamespace(name="posix", environ=os.environ))
    monkeypatch.setattr(process, "sys", SimpleNamespace(platform="darwin"))
    monkeypatch.setattr(process, "WindowsJob", job)
    monkeypatch.setattr(process, "LinuxProcess", linux)
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
