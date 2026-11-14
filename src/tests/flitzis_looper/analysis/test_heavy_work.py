"""Actual common Native permission, borrowed scopes and cleanup-only retirement."""

from contextvars import copy_context
from threading import Event
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import process
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


def _refuse_admission(
    monkeypatch: pytest.MonkeyPatch, failure: str
) -> tuple[list[OfflineWorkTicket], list[None]]:
    tickets: list[OfflineWorkTicket] = []
    closes: list[None] = []

    class RefusedTicket:
        def __init__(self) -> None:
            if failure == "constructor":
                message = "heavy work queue allocation failed"
                raise RuntimeError(message)
            self.ticket = OfflineWorkTicket()
            tickets.append(self.ticket)

        def try_acquire(self) -> bool:
            message = "heavy work gate poisoned"
            raise RuntimeError(message)

        def close(self) -> None:
            closes.append(None)
            self.ticket.close()

    monkeypatch.setattr(process, "OfflineWorkTicket", RefusedTicket)
    return tickets, closes


@pytest.mark.parametrize("failure", ["constructor", "acquire"])
def test_refused_heavy_admission_retires_without_launching_or_stranding_fifo(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    tickets, closes = _refuse_admission(monkeypatch, failure)

    def no_launch(*_args: object) -> None:
        pytest.fail("refused heavy admission launched a child")

    monkeypatch.setattr(process, "_create_owned_command", no_launch)
    successor = None
    try:
        with pytest.raises(process.OfflineWorkAdmissionError), process.offline_work_scope(Event()):
            no_launch()
        assert len(closes) == (1 if failure == "acquire" else 0)
        assert list(tmp_path.iterdir()) == []
        successor = OfflineWorkTicket()
        assert successor.try_acquire()
    finally:
        for ticket in tickets:
            ticket.close()
        if successor is not None:
            successor.close()
    assert process._PROCESS_SLOT.acquire(blocking=False)
    process._PROCESS_SLOT.release()


@pytest.mark.parametrize("phase", ["wait_callback", "payload"])
def test_caller_runtime_error_keeps_its_identity_outside_native_admission(phase: str) -> None:
    occupied = OfflineWorkTicket()
    error = RuntimeError("controlled caller failure")

    def fail() -> None:
        raise error

    try:
        if phase == "wait_callback":
            assert occupied.try_acquire()
        else:
            occupied.close()
        with (
            pytest.raises(RuntimeError) as caught,
            process.heavy_work_scope(Event(), fail if phase == "wait_callback" else None),
        ):
            fail()
        assert caught.value is error
        assert not isinstance(caught.value, process.OfflineWorkAdmissionError)
    finally:
        occupied.close()
    successor = OfflineWorkTicket()
    try:
        assert successor.try_acquire()
    finally:
        successor.close()
