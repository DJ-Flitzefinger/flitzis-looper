"""Actual shared executor capacity; no inference or audio acceptance claims."""

import threading
import weakref
from contextlib import contextmanager
from functools import partial
from time import monotonic
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.stem_workers import STEM_QUEUED_JOBS, STEM_WORKERS, StemWorkerPool

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator


@contextmanager
def _occupied_pool() -> Iterator[tuple[StemWorkerPool, threading.Event]]:
    pool = StemWorkerPool()
    release = threading.Event()
    started = [threading.Event() for _ in range(STEM_WORKERS)]

    def hold(index: int) -> None:
        started[index].set()
        assert release.wait(10)

    try:
        for index in range(STEM_WORKERS):
            pool(partial(hold, index))
        assert all(event.wait(5) for event in started)
        yield pool, release
    finally:
        release.set()
        pool.shutdown()


@contextmanager
def _refuse_worker_start(
    monkeypatch: pytest.MonkeyPatch, before_refusal: Callable[[], None] | None = None
) -> Iterator[None]:
    start = threading.Thread.start

    def refuse(thread: threading.Thread) -> None:
        if thread.name.startswith("stems_"):
            if before_refusal is not None:
                before_refusal()
            message = "fixture worker start refused"
            raise RuntimeError(message)
        start(thread)

    with monkeypatch.context() as patch:
        patch.setattr(threading.Thread, "start", refuse)
        yield


@pytest.mark.parametrize("busy_worker", [False, True])
def test_refused_thread_start_never_runs_or_retains_rejected_target(
    monkeypatch: pytest.MonkeyPatch, *, busy_worker: bool
) -> None:
    pool = StemWorkerPool()
    started, release, rejected, drained = (threading.Event() for _ in range(4))

    def hold() -> None:
        started.set()
        assert release.wait(10)

    target = partial(rejected.set)
    reference = weakref.ref(target)
    try:
        if busy_worker:
            pool(hold)
            assert started.wait(5)
        with (
            _refuse_worker_start(monkeypatch),
            pytest.raises(RuntimeError, match="fixture worker start refused"),
        ):
            pool.try_submit(target)
        del target
        assert reference() is None, "rejected queued wrapper retained its caller's target"
        release.set()
        pool(drained.set)
        assert drained.wait(5)
    finally:
        release.set()
        pool._executor.shutdown(wait=True)
    assert not rejected.is_set(), "rejected work ran after its caller handled submission failure"
    held = []
    try:
        for _ in range(STEM_WORKERS + STEM_QUEUED_JOBS):
            reservation = pool.try_reserve()
            assert reservation is not None
            held.append(reservation)
        assert pool.try_reserve() is None
    finally:
        for reservation in held:
            reservation.close()
            reservation.close()


def test_existing_worker_cannot_enter_target_before_submission_is_accepted(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    pool = StemWorkerPool()
    started, release, rejected, drained = (threading.Event() for _ in range(4))

    def hold() -> None:
        started.set()
        assert release.wait(10)

    def dequeue_before_refusal() -> None:
        release.set()
        deadline = monotonic() + 5
        while pool._executor._work_queue.qsize():
            assert monotonic() < deadline, "existing worker did not dequeue rejected work"
            threading.Event().wait(0.001)

    try:
        pool(hold)
        assert started.wait(5)
        with (
            _refuse_worker_start(monkeypatch, dequeue_before_refusal),
            pytest.raises(RuntimeError, match="fixture worker start refused"),
        ):
            pool.try_submit(rejected.set)
        pool(drained.set)
        assert drained.wait(5)
    finally:
        release.set()
        pool._executor.shutdown(wait=True)
    assert not rejected.is_set()


def test_full_try_submit_is_nonterminal_and_original_callable_keeps_its_contract() -> None:
    with _occupied_pool() as (pool, release):
        completed = threading.Event()
        for _ in range(STEM_QUEUED_JOBS):
            pool(lambda: None)
        assert not pool.try_submit(completed.set)
        assert not completed.is_set()
        with pytest.raises(RuntimeError, match="Stem queue full"):
            pool(completed.set)
        release.set()
        # A real queued barrier proves all preceding tasks returned their permits.
        admitted = threading.Event()
        for _ in range(1000):
            if pool.try_submit(admitted.set):
                break
            threading.Event().wait(0.001)
        assert admitted.wait(5)
        assert pool.try_submit(completed.set)
        assert completed.wait(5)


def test_reservation_precedes_ownership_mutation_and_uses_the_same_queue() -> None:
    with _occupied_pool() as (pool, _release):
        reservation = pool.try_reserve()
        assert reservation is not None
        for _ in range(STEM_QUEUED_JOBS - 1):
            pool(lambda: None)
        assert pool.try_reserve() is None
        assert not pool.try_submit(lambda: None)
        reservation.close()
        reservation.close()
        replacement = pool.try_reserve()
        assert replacement is not None
        assert pool.try_reserve() is None
        replacement.close()


def test_unsubmitted_context_returns_capacity_on_caller_failure() -> None:
    with _occupied_pool() as (pool, _release):
        for _ in range(STEM_QUEUED_JOBS - 1):
            pool(lambda: None)
        reservation = pool.try_reserve()
        assert reservation is not None
        message = "capture failed"
        with pytest.raises(ValueError, match="capture failed"), reservation:
            raise ValueError(message)
        replacement = pool.try_reserve()
        assert replacement is not None
        replacement.close()


def test_submitted_context_keeps_capacity_until_the_actual_task_returns() -> None:
    with _occupied_pool() as (pool, release):
        entered = threading.Event()
        finish = threading.Event()
        returned = threading.Event()
        reservation = pool.try_reserve()
        assert reservation is not None

        def actual_read() -> None:
            reservation.close()
            entered.set()
            assert finish.wait(10)
            returned.set()

        try:
            with reservation:
                reservation.submit(actual_read)
            for _ in range(STEM_QUEUED_JOBS - 1):
                pool(lambda: None)
            assert pool.try_reserve() is None
            release.set()
            assert entered.wait(5)
            assert not returned.is_set()
            with pytest.raises(RuntimeError, match=r"already submitted|no longer available"):
                reservation.submit(lambda: None)
        finally:
            finish.set()
        assert returned.wait(5)


def test_closed_reservation_cannot_submit_and_never_runs_the_target() -> None:
    pool = StemWorkerPool()
    called = threading.Event()
    try:
        reservation = pool.try_reserve()
        assert reservation is not None
        reservation.close()
        with pytest.raises(RuntimeError, match="no longer available"):
            reservation.submit(called.set)
        assert not called.is_set()
    finally:
        pool.shutdown()


@pytest.mark.parametrize(
    "submit", [lambda pool, target: pool(target), lambda pool, target: pool.try_submit(target)]
)
def test_shutdown_submission_failure_returns_every_reserved_slot(
    submit: Callable[[StemWorkerPool, Callable[[], None]], object],
) -> None:
    pool = StemWorkerPool()
    pool.shutdown()
    for _ in range(STEM_WORKERS + STEM_QUEUED_JOBS + 1):
        with pytest.raises(RuntimeError, match="cannot schedule new futures after shutdown"):
            submit(pool, lambda: None)
