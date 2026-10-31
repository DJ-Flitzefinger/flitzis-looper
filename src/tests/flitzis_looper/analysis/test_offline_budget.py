"""Shared heavy admission, effective child pools and real retirement boundaries."""

import json
import os
import sys
from concurrent.futures import ThreadPoolExecutor
from contextvars import copy_context
from threading import Event
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.controller import stem_process
from flitzis_looper.controller.stem_resources import StemResourceGuard
from flitzis_looper.offline_runtime import child_environment
from flitzis_looper_audio import OfflineChildMemoryReservation

if TYPE_CHECKING:
    from pathlib import Path


def test_scope_wait_cancellation_nesting_and_copied_context_do_not_release_owner() -> None:
    cancel, entered = Event(), Event()

    def waiter() -> None:
        entered.set()
        with process.offline_work_scope(cancel):
            pytest.fail("another thread bypassed occupied admission")

    with ThreadPoolExecutor(max_workers=1) as executor, process.offline_work_scope(Event()):
        context = copy_context()
        with process.offline_work_scope(Event()):
            assert not process._PROCESS_SLOT.acquire(blocking=False)
        future = executor.submit(context.run, waiter)
        assert entered.wait(5)
        cancel.set()
        with pytest.raises(RuntimeError, match="cancelled before resource admission"):
            future.result(5)
        assert not process._PROCESS_SLOT.acquire(blocking=False)
    assert process._PROCESS_SLOT.acquire(blocking=False)
    process._PROCESS_SLOT.release()
    # A context retained after its original scope ended grants no later admission.
    with process.offline_work_scope(Event()):
        cancel.set()
        with pytest.raises(RuntimeError, match="cancelled before resource admission"):
            context.run(lambda: _enter(cancel))


def _enter(cancel: Event) -> None:
    with process.offline_work_scope(cancel):
        pytest.fail("expired context bypassed occupied admission")


def test_real_child_effective_torch_pools_and_parent_environment(tmp_path: Path) -> None:
    original = dict(os.environ)
    hostile = original | {
        "OMP_NUM_THREADS": "16",
        "MKL_NUM_THREADS": "16",
        "OPENBLAS_NUM_THREADS": "16",
    }
    body = (
        "import json, os, torch\n"
        "from flitzis_looper.offline_runtime import configure_child\n"
        "threads = configure_child(torch.set_num_threads, torch.set_num_interop_threads, "
        "torch.get_num_threads, torch.get_num_interop_threads)\n"
        "x = torch.arange(64, dtype=torch.float64).reshape(8, 8)\n"
        "print(json.dumps({'threads':threads, 'omp':os.environ['OMP_NUM_THREADS'], "
        "'matrix':(x @ x.T).tolist()}))\n"
    )
    result = process.run_owned_command(
        [sys.executable, "-c", body],
        cwd=tmp_path,
        env=child_environment(hostile),
        cancel=Event(),
        retired=Event(),
    )
    assert result.reason == "ok", result.stderr
    image = json.loads(result.stdout)
    assert image["threads"] == [1, 1]
    assert image["omp"] == "1"
    assert image["matrix"] == [
        [sum((row * 8 + k) * (column * 8 + k) for k in range(8)) for column in range(8)]
        for row in range(8)
    ]
    assert hostile["OMP_NUM_THREADS"] == "16"
    assert dict(os.environ) == original


@pytest.mark.skipif(os.name != "nt", reason="Windows kernel job commit enforcement")
def test_job_commit_limit_blocks_allocation_and_is_preserved_during_termination(
    tmp_path: Path,
) -> None:
    body = (
        "try:\n x = bytearray(512 * 1024 * 1024)\n"
        "except MemoryError:\n print('allocation refused')\n"
        "else:\n raise SystemExit('unbounded allocation')\n"
    )
    result = process.run_owned_command(
        [sys.executable, "-I", "-c", body],
        cwd=tmp_path,
        env=dict(os.environ),
        cancel=Event(),
        retired=Event(),
        commit_limit_bytes=128 * 1024 * 1024,
    )
    assert result.reason == "ok", result.stderr
    assert result.stdout.strip() == b"allocation refused"


@pytest.mark.skipif(os.name != "nt", reason="Windows Native ledger and owned tree")
def test_child_commit_promise_and_heavy_permit_survive_finished_launcher_and_held_readers(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    held, release, closed = Event(), Event(), Event()
    actual_read = process._CommandReader._read

    class Budget:
        def __init__(self) -> None:
            self.actual = OfflineChildMemoryReservation()

        @property
        def limit_bytes(self) -> int:
            return self.actual.limit_bytes

        def close(self) -> None:
            self.actual.close()
            closed.set()

    def read(reader: process._CommandReader) -> None:
        actual_read(reader)
        held.set()
        assert release.wait(10)

    def run() -> process.OwnedCommandResult:
        with stem_process.stem_command_scope(Event(), resources=StemResourceGuard()):
            return stem_process.run_stem_command(
                [sys.executable, "-I", "-c", "print('finished')"],
                cwd=tmp_path,
                env=dict(os.environ),
            )

    monkeypatch.setattr(stem_process, "OfflineChildMemoryReservation", Budget)
    monkeypatch.setattr(process._CommandReader, "_read", read)
    with ThreadPoolExecutor(max_workers=1) as executor:
        pending = executor.submit(run)
        try:
            assert held.wait(5)
            assert not pending.done()
            assert not closed.is_set()
            assert not process._PROCESS_SLOT.acquire(blocking=False)
        finally:
            release.set()
        assert pending.result(10).reason == "ok"
    assert closed.is_set()
    assert process._PROCESS_SLOT.acquire(blocking=False)
    process._PROCESS_SLOT.release()


def test_invalid_commit_limit_starts_no_child_and_marks_actual_retirement(tmp_path: Path) -> None:
    retired = Event()
    with pytest.raises(ValueError, match="commit limit"):
        process.run_owned_command(
            ["must-not-launch"],
            cwd=tmp_path,
            env={},
            cancel=Event(),
            retired=retired,
            commit_limit_bytes=-1,
        )
    assert retired.is_set()
