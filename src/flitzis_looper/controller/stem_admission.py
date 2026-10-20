"""Transient fair admission into the existing bounded offline stem worker pool."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.stem_workers import StemWorkerReservation

type StemAdmissionClass = Literal["direct", "batch"]
type StemAdmissionPhase = Literal["resolve", "infer", "prepare"]
type StemAdmissionLane = tuple[StemAdmissionClass, StemAdmissionPhase]
type StemAdmissionResult = Literal["submitted", "deferred", "settled"]

STEM_ADMISSION_LANES: tuple[StemAdmissionLane, ...] = (
    ("direct", "resolve"),
    ("direct", "infer"),
    ("direct", "prepare"),
    ("batch", "resolve"),
    ("batch", "infer"),
    ("batch", "prepare"),
)
STEM_ADMISSION_GRANTS_PER_POLL = len(STEM_ADMISSION_LANES)


@dataclass(frozen=True, slots=True)
class StemAdmissionCandidate[T]:
    """An immutable view of an existing owner's current readiness, without a task.

    The provider owns the referenced record and its readiness sequence. The view
    conveys no source, publication or Native authority; admission must revalidate
    that original owner after existing pool capacity has been reserved.
    """

    owner: T
    ready_sequence: int
    request_class: StemAdmissionClass
    phase: StemAdmissionPhase

    def __post_init__(self) -> None:
        if type(self.ready_sequence) is not int:
            message = "Stem readiness sequence must be an integer"
            raise TypeError(message)
        if self.ready_sequence < 0:
            message = "Stem readiness sequence must be nonnegative"
            raise ValueError(message)
        if (
            type(self.request_class) is not str
            or type(self.phase) is not str
            or self.lane not in STEM_ADMISSION_LANES
        ):
            message = "Unknown stem admission class or phase"
            raise ValueError(message)

    @property
    def lane(self) -> StemAdmissionLane:
        """Return the request class and offline phase selected by this view."""
        return self.request_class, self.phase


class StemAdmissionArbiter:
    """Rotate six lanes using only the existing pool's scoped reservations.

    Only successful submissions move the cursor. Each run owns transient views
    and exclusions, never task closures, worker permits or a second job registry.
    The control owner must serialize calls and provide a finite candidate image.
    """

    __slots__ = ("_cursor",)

    def __init__(self) -> None:
        self._cursor = 0

    def run[T](
        self,
        provider: Callable[[], tuple[StemAdmissionCandidate[T], ...]],
        reserve: Callable[[], StemWorkerReservation | None],
        admit: Callable[[StemAdmissionCandidate[T], StemWorkerReservation], StemAdmissionResult],
    ) -> int:
        """Submit at most six current candidates without retaining refused work.

        Args:
            provider: Read finite immutable views of existing authoritative owners.
                Refreshed after each submission so its exact owner can be consumed.
            reserve: The existing shared StemWorkerPool.try_reserve entry point.
            admit: Revalidate the exact owner under a reservation, then submit or
                report deferred/settled. Return submitted only after submit succeeds.

        Returns:
            The number of actual successful submissions reported by admission.
            A full pool ends the poll before admission can mutate any owner.

        Raises:
            ValueError: Admission returned an unsupported result.
            Exception: Caller errors propagate after unused capacity is returned.
        """
        submitted = 0
        excluded: set[tuple[StemAdmissionClass, StemAdmissionPhase, int, int]] = set()
        candidates = provider()
        while submitted < STEM_ADMISSION_GRANTS_PER_POLL:
            candidate = self._next(candidates, excluded)
            if candidate is None:
                break
            reservation = reserve()
            if reservation is None:
                break
            excluded.add(self._key(candidate))
            with reservation:
                result = admit(candidate, reservation)
                if result not in {"submitted", "deferred", "settled"}:
                    message = "Unknown stem admission result"
                    raise ValueError(message)
            if result == "submitted":
                self._cursor = (STEM_ADMISSION_LANES.index(candidate.lane) + 1) % len(
                    STEM_ADMISSION_LANES
                )
                submitted += 1
                if submitted < STEM_ADMISSION_GRANTS_PER_POLL:
                    candidates = provider()
        return submitted

    def _next[T](
        self,
        candidates: tuple[StemAdmissionCandidate[T], ...],
        excluded: set[tuple[StemAdmissionClass, StemAdmissionPhase, int, int]],
    ) -> StemAdmissionCandidate[T] | None:
        for offset in range(len(STEM_ADMISSION_LANES)):
            lane = STEM_ADMISSION_LANES[(self._cursor + offset) % len(STEM_ADMISSION_LANES)]
            ready = (
                candidate
                for candidate in candidates
                if candidate.lane == lane and self._key(candidate) not in excluded
            )
            candidate = min(ready, key=lambda current: current.ready_sequence, default=None)
            if candidate is not None:
                return candidate
        return None

    @staticmethod
    def _key[T](
        candidate: StemAdmissionCandidate[T],
    ) -> tuple[StemAdmissionClass, StemAdmissionPhase, int, int]:
        return (
            candidate.request_class,
            candidate.phase,
            id(candidate.owner),
            candidate.ready_sequence,
        )
