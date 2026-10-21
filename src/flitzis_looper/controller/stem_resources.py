"""Conservative host headroom policy for existing control/offline stem owners."""

import time
from dataclasses import dataclass
from threading import Lock
from typing import TYPE_CHECKING, Literal

from flitzis_looper.system_resources import host_memory_snapshot

if TYPE_CHECKING:
    from collections.abc import Callable

_MIB = 1024**2
_ADMISSION_COMMIT = 2048 * _MIB
_RECOVERY_COMMIT = 3072 * _MIB
_ADMISSION_PHYSICAL = 512 * _MIB
_RECOVERY_PHYSICAL = 768 * _MIB
_CRITICAL_COMMIT = 512 * _MIB
_CRITICAL_PHYSICAL = 128 * _MIB
_SAMPLE_SECONDS = 0.25
_UNAVAILABLE_SECONDS = 1.0
type ResourceState = Literal["unobserved", "clear", "pressure", "unavailable", "unsupported"]


@dataclass(frozen=True, slots=True)
class StemResourceDecision:
    """Last observation and stable performer-facing admission status."""

    state: ResourceState
    detail: str = ""

    @property
    def allowed(self) -> bool:
        """Allow measured headroom or explicitly unsupported platform behavior."""
        return self.state in {"clear", "unsupported"}

    @property
    def message(self) -> str | None:
        """Describe only current admission, without a prediction or progress claim."""
        if self.state == "pressure":
            return "Waiting for available memory"
        if self.state == "unavailable":
            return "Waiting for memory monitoring"
        if self.state == "unsupported":
            return "Host memory monitoring unsupported; bounded stem pool remains active"
        return None


class StemResourceError(RuntimeError):
    """One original stem attempt failed resource policy, independently of cancel."""

    def __init__(self, reason: str, detail: str) -> None:
        self.reason = reason
        super().__init__(f"Stem command failed: {reason}" + (f" ({detail})" if detail else ""))


class StemResourceGuard:
    """Share bounded observations, recovery latch and fixed query-failure deadline.

    The lock only protects short control/offline OS queries and cached state. No
    background sampler, process handle, job registry or realtime work is created.
    """

    def __init__(
        self,
        sampler: Callable[[], dict[str, int] | None] = host_memory_snapshot,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        self._sampler = sampler
        self._clock = clock
        self._lock = Lock()
        self._observed_at: float | None = None
        self._unknown_since: float | None = None
        self._blocked = False
        self._critical = False
        self._current = StemResourceDecision("unobserved")

    @property
    def current(self) -> StemResourceDecision:
        """Read immutable status without performing an OS query on UI access."""
        with self._lock:
            return self._current

    def admission(self) -> StemResourceDecision:
        """Check cached headroom before touching original arbiter capacity/owners."""
        with self._lock:
            return self._observe(fresh=False)

    def entry(self) -> None:
        """Recheck fresh headroom inside the worker's existing settlement boundary."""
        with self._lock:
            decision = self._observe(fresh=True)
            if not decision.allowed:
                reason = (
                    "resource_unavailable"
                    if decision.state == "unavailable"
                    else "resource_pressure"
                )
                raise StemResourceError(reason, decision.detail)

    def command_stop_reason(self) -> str | None:
        """Stop critical active work or a fixed consecutive query-failure interval."""
        with self._lock:
            decision = self._observe(fresh=False)
            if decision.state == "unavailable":
                if (
                    self._unknown_since is not None
                    and self._clock() - self._unknown_since >= _UNAVAILABLE_SECONDS
                ):
                    return "resource_unavailable"
                return None
            return "resource_pressure" if self._critical else None

    def failure(self, reason: str) -> StemResourceError:
        """Build a typed failure only after the original command actually settles."""
        with self._lock:
            return StemResourceError(reason, self._current.detail)

    def _observe(self, *, fresh: bool) -> StemResourceDecision:
        now = self._clock()
        if (
            not fresh
            and self._observed_at is not None
            and now - self._observed_at < _SAMPLE_SECONDS
        ):
            return self._current
        self._observed_at = now
        try:
            image = self._sampler()
        except (OSError, ValueError) as error:
            self._blocked = True
            self._critical = False
            if self._unknown_since is None:
                self._unknown_since = now
            self._current = StemResourceDecision("unavailable", str(error))
            return self._current
        self._unknown_since = None
        self._critical = False
        if image is None:
            self._current = StemResourceDecision("unsupported")
            return self._current
        caller = image["available_commit"]
        system = image["system_available_commit_bytes"]
        physical = image["available_physical"]
        commit_floor = _RECOVERY_COMMIT if self._blocked else _ADMISSION_COMMIT
        physical_floor = _RECOVERY_PHYSICAL if self._blocked else _ADMISSION_PHYSICAL
        self._blocked = min(caller, system) < commit_floor or physical < physical_floor
        self._critical = min(caller, system) <= _CRITICAL_COMMIT or physical <= _CRITICAL_PHYSICAL
        detail = (
            f"available bytes: caller commit={caller}, system commit={system}, physical={physical}"
        )
        self._current = StemResourceDecision("pressure" if self._blocked else "clear", detail)
        return self._current
