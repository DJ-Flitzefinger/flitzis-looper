import threading
from concurrent.futures import ThreadPoolExecutor
from typing import TYPE_CHECKING, Self

if TYPE_CHECKING:
    from collections.abc import Callable
    from types import TracebackType

STEM_WORKERS = 2
STEM_QUEUED_JOBS = 32


class StemWorkerReservation:
    """One slot in the existing pool, acquired before new task ownership mutates."""

    def __init__(self, pool: StemWorkerPool) -> None:
        self._pool = pool
        self._lock = threading.Lock()
        self._submitted = False
        self._released = False

    def __enter__(self) -> Self:
        """Keep unsubmitted capacity scoped to the caller's preparation attempt."""
        return self

    def __exit__(
        self,
        _exc_type: type[BaseException] | None,
        _exc_value: BaseException | None,
        _traceback: TracebackType | None,
    ) -> None:
        self.close()

    def submit(self, target: Callable[[], None]) -> None:
        """Submit once; a running or queued task owns its slot until actual return."""
        with self._lock:
            if self._released or self._submitted:
                message = "Stem worker reservation is no longer available or already submitted"
                raise RuntimeError(message)
            self._submitted = True
        try:
            self._pool._executor.submit(self._run, target)
        except RuntimeError:
            self._release()
            raise

    def close(self) -> None:
        """Return unused capacity once without releasing a submitted task's slot."""
        with self._lock:
            if self._submitted or self._released:
                return
            self._released = True
        self._pool._slots.release()

    def _run(self, target: Callable[[], None]) -> None:
        try:
            target()
        finally:
            self._release()

    def _release(self) -> None:
        with self._lock:
            if self._released:
                return
            self._released = True
        self._pool._slots.release()


class StemWorkerPool:
    """Bound offline separator admissions without retaining rejected jobs."""

    def __init__(self) -> None:
        self._slots = threading.BoundedSemaphore(STEM_WORKERS + STEM_QUEUED_JOBS)
        self._executor = ThreadPoolExecutor(max_workers=STEM_WORKERS, thread_name_prefix="stems")

    def __call__(self, target: Callable[[], None]) -> None:
        """Admit one job or fail immediately with existing playback intact."""
        if not self.try_submit(target):
            msg = "Stem queue full (2 workers, 32 queued jobs)"
            raise RuntimeError(msg)

    def try_reserve(self) -> StemWorkerReservation | None:
        """Reserve existing capacity nonblockingly, returning None under backpressure."""
        if not self._slots.acquire(blocking=False):
            return None
        return StemWorkerReservation(self)

    def try_submit(self, target: Callable[[], None]) -> bool:
        """Keep full capacity nonterminal for incremental feeders of this same pool."""
        reservation = self.try_reserve()
        if reservation is None:
            return False
        reservation.submit(target)
        return True

    def shutdown(self) -> None:
        """Drain cancelled jobs without blocking the control/UI thread."""
        self._executor.shutdown(wait=False)
