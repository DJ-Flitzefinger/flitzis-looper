"""Observe Windows memory pressure for existing control/offline stem owners."""

import time
from threading import Lock
from typing import TYPE_CHECKING

from flitzis_looper.system_resources import (
    OfflineMemoryMonitor,
    stem_memory_snapshot,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.system_resources import MemoryObservation


class StemResourceError(RuntimeError):
    """One original stem attempt failed resource policy, independently of cancel."""

    def __init__(self, reason: str, detail: str) -> None:
        self.reason = reason
        super().__init__(f"Stem command failed: {reason}" + (f" ({detail})" if detail else ""))


class StemResourceGuard:
    """Share current observations and the bounded query-failure deadline.

    The lock only protects short control/offline OS queries and cached state. No
    background sampler, process handle, job registry or realtime work is created.
    """

    def __init__(
        self,
        sampler: Callable[[], dict[str, int] | None] = stem_memory_snapshot,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        self._lock = Lock()
        self._monitor = OfflineMemoryMonitor(
            sampler,
            clock,
            unsupported_message=(
                "Host memory monitoring unsupported; bounded stem pool remains active"
            ),
        )

    @property
    def current(self) -> MemoryObservation:
        """Read immutable status without performing an OS query on UI access."""
        with self._lock:
            return self._monitor.current

    def admission(self) -> MemoryObservation:
        """Check cached headroom before touching original arbiter capacity/owners."""
        with self._lock:
            return self._monitor.observe()

    def entry(self) -> None:
        """Recheck fresh headroom inside the worker's existing settlement boundary."""
        with self._lock:
            decision = self._monitor.observe(fresh=True)
            if (reason := decision.refusal_reason) is not None:
                raise StemResourceError(reason, decision.detail)

    def command_stop_reason(self) -> str | None:
        """Stop on observed OS pressure or sustained monitoring failure."""
        with self._lock:
            return self._monitor.command_stop_reason()

    def failure(self, reason: str) -> StemResourceError:
        """Build a typed failure only after the original command actually settles."""
        with self._lock:
            return StemResourceError(reason, self._monitor.current.detail)
