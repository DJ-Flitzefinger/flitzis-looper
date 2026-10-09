import threading
from concurrent.futures import ThreadPoolExecutor
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable

STEM_WORKERS = 2
STEM_QUEUED_JOBS = 32


class StemWorkerPool:
    """Bound offline separator admissions without retaining rejected jobs."""

    def __init__(self) -> None:
        self._slots = threading.BoundedSemaphore(STEM_WORKERS + STEM_QUEUED_JOBS)
        self._executor = ThreadPoolExecutor(max_workers=STEM_WORKERS, thread_name_prefix="stems")

    def __call__(self, target: Callable[[], None]) -> None:
        """Admit one job or fail immediately with existing playback intact."""
        if not self._slots.acquire(blocking=False):
            msg = "Stem queue full (2 workers, 32 queued jobs)"
            raise RuntimeError(msg)
        try:
            self._executor.submit(self._run, target)
        except RuntimeError:
            self._slots.release()
            raise

    def _run(self, target: Callable[[], None]) -> None:
        try:
            target()
        finally:
            self._slots.release()

    def shutdown(self) -> None:
        """Drain cancelled jobs without blocking the control/UI thread."""
        self._executor.shutdown(wait=False)
