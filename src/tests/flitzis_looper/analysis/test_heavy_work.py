"""Actual common Native permission, borrowed scopes and cleanup-only retirement."""

from contextvars import copy_context
from threading import Event
from types import SimpleNamespace
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.analysis.contracts import WorkerLimits
from flitzis_looper.controller.stem_admission import StemHeavyAdmission
from flitzis_looper_audio import OfflineWorkTicket

if TYPE_CHECKING:
    from pathlib import Path


def test_actual_native_fifo_cancelled_ticket_and_last_borrow() -> None:
    first, cancelled, last = OfflineWorkTicket(), OfflineWorkTicket(), OfflineWorkTicket()
    borrowed = None
    try:
        assert first.try_acquire()
        assert not last.try_acquire()
        cancelled.close()
        borrowed = first.borrow()
        first.close()
        assert not last.try_acquire()
        borrowed.close()
        assert last.try_acquire()
    finally:
        first.close()
        cancelled.close()
        last.close()
        if borrowed is not None:
            borrowed.close()


def test_waiting_stem_withdrawal_keeps_metadata_without_stranding_common_fifo() -> None:
    analysis = OfflineWorkTicket()
    waiting = StemHeavyAdmission()
    successor = None
    try:
        assert analysis.try_acquire()
        assert not waiting.try_acquire()
        successor = OfflineWorkTicket()
        # The original PAD has become ineligible (playback/placement). Its
        # metadata may wait, but its never-started global FIFO entry must leave.
        waiting.withdraw_waiting()
        analysis.close()
        assert successor.try_acquire()
        assert not waiting.try_acquire()
        waiting.withdraw_waiting()
    finally:
        waiting.withdraw_waiting()
        analysis.close()
        if successor is not None:
            successor.close()


def test_native_callback_can_close_original_and_nested_python_scope_borrows() -> None:
    first, next_job = OfflineWorkTicket(), OfflineWorkTicket()
    try:
        assert first.try_acquire()

        def nested() -> None:
            # The Native callback's lexical borrow survives close(), and it
            # permits Python to borrow this exact job instead of deadlocking.
            first.close()
            with process.heavy_work_scope(Event()), process.offline_work_scope(Event()):
                assert not process._PROCESS_SLOT.acquire(blocking=False)
                assert not next_job.try_acquire()
            assert not next_job.try_acquire()

        first.run(nested)
        assert next_job.try_acquire()
        assert process._WORK_OWNER.get() is None
    finally:
        first.close()
        next_job.close()


def test_retained_preparation_metadata_owns_no_capacity_after_handoff() -> None:
    metadata = StemHeavyAdmission()
    work = successor = None
    try:
        assert metadata.try_acquire()
        work = metadata.transfer()
        successor = OfflineWorkTicket()
        metadata.close()
        assert not successor.try_acquire()
        work.close()
        # Keep metadata strongly reachable, as UI snapshots and callbacks do.
        assert metadata is not None
        assert successor.try_acquire()
    finally:
        metadata.close()
        if work is not None:
            work.close()
        if successor is not None:
            successor.close()


def test_cancelled_or_retired_physical_owner_never_registers_another_ticket() -> None:
    cancel = Event()
    cancel.set()
    owner = StemHeavyAdmission(cancel)
    successor = None
    try:
        assert not owner.try_acquire()
        assert not owner.registered
        cancel.clear()
        assert owner.try_acquire()
        successor = OfflineWorkTicket()
        assert not successor.try_acquire()
        owner.retire()
        assert successor.try_acquire()
        assert not owner.try_acquire()
        assert not owner.registered
    finally:
        owner.retire()
        if successor is not None:
            successor.close()


def test_closed_scope_context_cannot_reenter_another_native_job() -> None:
    with process.heavy_work_scope(Event()):
        saved = copy_context()
    occupied = OfflineWorkTicket()
    cancel = Event()
    try:
        assert occupied.try_acquire()
        cancel.set()
        with pytest.raises(process.OfflineWorkCancelledError, match="cancelled before resource"):
            saved.run(lambda: _scope(cancel))
    finally:
        occupied.close()


def _scope(cancel: Event) -> None:
    with process.heavy_work_scope(cancel):
        pytest.fail("a cancelled stale context acquired another job's capacity")


def test_failed_launch_cleanup_retains_native_job_until_actual_temp_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    release, retiring, retired = Event(), Event(), Event()

    class Temporary:
        name = str(tmp_path)

        def cleanup(self) -> None:
            if not release.is_set():
                retiring.set()
                message = "controlled still-owned temporary storage"
                raise OSError(message)

    def fail_start(*_args: object) -> None:
        message = "controlled unstarted child"
        raise OSError(message)

    monkeypatch.setattr(process.tempfile, "TemporaryDirectory", lambda **_kwargs: Temporary())
    monkeypatch.setattr(process, "_start", fail_start)
    command = process.WorkerCommand(tmp_path, tmp_path, tmp_path, tmp_path)
    next_job = None
    try:
        result = process.run_process(command, b"{}", Event(), WorkerLimits(), retired)
        assert result.reason == "worker_retiring"
        assert not result.resources_released
        assert retiring.wait(5)
        next_job = OfflineWorkTicket()
        assert not next_job.try_acquire()
        assert not retired.is_set()
        release.set()
        assert retired.wait(5)
        assert next_job.try_acquire()
    finally:
        release.set()
        assert retired.wait(5)
        if next_job is not None:
            next_job.close()
    assert process._PROCESS_SLOT.acquire(blocking=False)
    process._PROCESS_SLOT.release()
    assert process._ANALYSIS_ADMISSION.acquire(blocking=False)
    process._ANALYSIS_ADMISSION.release()


def test_cleanup_failure_does_not_release_the_other_jobs_slot() -> None:
    first, next_job = OfflineWorkTicket(), OfflineWorkTicket()
    fail = True
    owned = None

    class Temporary:
        def cleanup(self) -> None:
            if fail:
                message = "still in use"
                raise OSError(message)

    try:
        assert first.try_acquire()
        borrowed = first.borrow()
        assert process._PROCESS_SLOT.acquire(blocking=False)
        assert process._ANALYSIS_ADMISSION.acquire(blocking=False)
        owned = process._OwnedProcess(
            SimpleNamespace(stdout=None, stderr=None),
            None,
            Temporary(),
            Event(),
            whole_work=borrowed,
        )
        first.close()
        assert not owned.release()
        assert not next_job.try_acquire()
        fail = False
        assert owned.release()
        assert next_job.try_acquire()
    finally:
        fail = False
        if owned is not None and not owned.retired.is_set():
            assert owned.release()
        first.close()
        next_job.close()
