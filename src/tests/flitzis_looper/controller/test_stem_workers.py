import threading

import pytest

from flitzis_looper.controller.stem_workers import STEM_QUEUED_JOBS, STEM_WORKERS, StemWorkerPool


def test_pool_runs_two_workers_and_limits_queue_to_32_jobs() -> None:
    pool = StemWorkerPool()
    release = threading.Event()
    started = [threading.Event(), threading.Event()]
    queued_started = threading.Event()

    def blocked(index: int) -> None:
        started[index].set()
        assert release.wait(10)

    def queued() -> None:
        queued_started.set()

    try:
        pool(lambda: blocked(0))
        pool(lambda: blocked(1))
        assert all(event.wait(5) for event in started)
        for _ in range(32):
            pool(queued)
        assert not queued_started.is_set()
        with pytest.raises(RuntimeError, match="2 workers, 32 queued jobs"):
            pool(queued)
        assert STEM_WORKERS == 2
        assert STEM_QUEUED_JOBS == 32
    finally:
        release.set()
        pool.shutdown()
    assert queued_started.wait(5)


def test_failed_worker_releases_admission_before_next_queued_job() -> None:
    pool = StemWorkerPool()
    fail = threading.Event()
    release = threading.Event()
    failing_started = threading.Event()
    other_started = threading.Event()
    queued_started = threading.Event()
    replacement_completed = threading.Event()

    def failing() -> None:
        failing_started.set()
        assert fail.wait(10)
        msg = "intentional separator failure"
        raise RuntimeError(msg)

    def other() -> None:
        other_started.set()
        assert release.wait(10)

    def queued() -> None:
        queued_started.set()
        assert release.wait(10)

    try:
        pool(failing)
        pool(other)
        assert failing_started.wait(5)
        assert other_started.wait(5)
        for _ in range(32):
            pool(queued)
        with pytest.raises(RuntimeError, match="queue full"):
            pool(queued)
        fail.set()
        # This starts only after the failing target's finally block returned its permit.
        assert queued_started.wait(5)
        pool(replacement_completed.set)
    finally:
        fail.set()
        release.set()
        pool.shutdown()
    assert replacement_completed.wait(5)


def test_shutdown_returns_while_workers_are_blocked_and_rejects_later_admission() -> None:
    pool = StemWorkerPool()
    release = threading.Event()
    started = threading.Event()
    shutdown_returned = threading.Event()

    def blocked() -> None:
        started.set()
        assert release.wait(10)

    def close() -> None:
        pool.shutdown()
        shutdown_returned.set()

    pool(blocked)
    closing = threading.Thread(target=close)
    try:
        assert started.wait(5)
        closing.start()
        assert shutdown_returned.wait(5)
        assert not release.is_set()
        # Repeated rejected submits must return their acquired permits as well.
        for _ in range(STEM_WORKERS + STEM_QUEUED_JOBS + 1):
            with pytest.raises(RuntimeError, match="cannot schedule new futures after shutdown"):
                pool(lambda: None)
    finally:
        release.set()
        if closing.ident is not None:
            closing.join(timeout=5)
        pool.shutdown()
    assert shutdown_returned.is_set()
