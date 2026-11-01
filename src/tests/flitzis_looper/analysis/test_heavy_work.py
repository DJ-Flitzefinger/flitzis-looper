"""Actual common Native permission, borrowed scopes and cleanup-only retirement."""

import hashlib
import sys
from concurrent.futures import ThreadPoolExecutor
from contextvars import copy_context
from pathlib import Path
from threading import Event
from types import SimpleNamespace

import pytest

from flitzis_looper.analysis import neural_worker, process
from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatModelIdentity,
    BeatWorkerRequest,
    MonoPcmInput,
    WorkerLimits,
)
from flitzis_looper.analysis.execution import CudaBeatModelIdentity, CudaBeatWorkerRequest
from flitzis_looper.analysis.worker import BeatWorkerAdapter, WorkerConfiguration
from flitzis_looper.controller.stem_admission import StemHeavyAdmission
from flitzis_looper_audio import OfflineWorkTicket


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

    monkeypatch.setattr(process, "_start", no_launch)
    retired = Event()
    retired.set()
    command = process.WorkerCommand(tmp_path, tmp_path, tmp_path, tmp_path)
    successor = None
    try:
        outcome = process.run_process(command, b"{}", Event(), WorkerLimits(), retired)
        assert outcome.reason == "worker_supervision_failed"
        assert outcome.resources_released
        assert retired.is_set()
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
    assert process._ANALYSIS_ADMISSION.acquire(blocking=False)
    process._ANALYSIS_ADMISSION.release()


@pytest.mark.parametrize("failure", ["constructor", "acquire"])
@pytest.mark.parametrize("cuda", [False, True])
def test_refused_admission_returns_from_both_neural_bridges(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure: str, *, cuda: bool
) -> None:
    tickets, closes = _refuse_admission(monkeypatch, failure)
    adapters: list[BeatWorkerAdapter] = []
    checkpoint = tmp_path / "fixture.ckpt"
    checkpoint.write_bytes(b"protocol fixture; not neural weights")
    digest = hashlib.sha256(checkpoint.read_bytes()).hexdigest()
    pcm = tmp_path / "mono.f32le"
    pcm.write_bytes(bytes(400))
    (tmp_path / "worker.py").write_text(
        "raise AssertionError('refused admission launched a child')\n"
    )
    model = (
        CudaBeatModelIdentity(digest, "fixture")
        if cuda
        else BeatModelIdentity(digest, "fixture", "fixture")
    )
    configuration = WorkerConfiguration(
        Path(sys.executable), tmp_path / "worker.py", checkpoint, tmp_path, model
    )
    identity = AnalysisIdentity(0, 1, "source-fixture", 1)
    source = MonoPcmInput(pcm, 48000, 100)

    def adapter(
        configuration: WorkerConfiguration, limits: WorkerLimits | None = None
    ) -> BeatWorkerAdapter:
        instance = BeatWorkerAdapter(configuration, limits)
        # Protocol-two installation is deliberately absent. Exercise the real
        # adapter and shared process path without any model/device invocation.
        monkeypatch.setattr(instance, "_preflight", lambda *_args: None)
        adapters.append(instance)
        return instance

    monkeypatch.setattr(neural_worker, "BeatWorkerAdapter", adapter)
    cancel = neural_worker._NativeCancellationEvent(SimpleNamespace(is_cancelled=lambda: False))
    with ThreadPoolExecutor(max_workers=1) as executor:
        if cuda:
            request = CudaBeatWorkerRequest(identity, source, model)
            future = executor.submit(
                neural_worker._run_cuda_retired, request, configuration, cancel
            )
        else:
            request = BeatWorkerRequest(identity, source, model)
            future = executor.submit(neural_worker._run_retired, request, configuration, cancel)
        try:
            result = future.result(timeout=2)
            assert result.status == "failed"
            assert result.reason == "worker_supervision_failed"
            assert result.predictions is None
            assert result.resources_released
            assert adapters[0].retired.is_set()
            assert len(closes) == (1 if failure == "acquire" else 0)
            successor = OfflineWorkTicket()
            try:
                assert successor.try_acquire()
            finally:
                successor.close()
        finally:
            # The unchanged-source RED case blocks on an event with no child.
            # Unblock and join only this test's adapter before returning.
            for instance in adapters:
                instance.retired.set()
            for ticket in tickets:
                ticket.close()
    assert pcm.read_bytes() == bytes(400)
    assert not any(path.name.startswith("beat-request-") for path in tmp_path.iterdir())


def test_failed_nested_beat_admission_retires_only_its_own_borrow(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    owner, successor = OfflineWorkTicket(), OfflineWorkTicket()
    tickets, closes = _refuse_admission(monkeypatch, "acquire")
    retired = Event()
    command = process.WorkerCommand(tmp_path, tmp_path, tmp_path, tmp_path)

    def nested() -> None:
        outcome = process.run_process(command, b"{}", Event(), WorkerLimits(), retired)
        assert outcome.reason == "worker_supervision_failed"
        assert retired.is_set()
        assert closes == [None]
        assert not successor.try_acquire()

    try:
        assert owner.try_acquire()
        owner.run(nested)
        # A completed Beat branch cannot release the enclosing Native/Key job.
        assert not successor.try_acquire()
        owner.close()
        assert successor.try_acquire()
    finally:
        for ticket in tickets:
            ticket.close()
        owner.close()
        successor.close()


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
