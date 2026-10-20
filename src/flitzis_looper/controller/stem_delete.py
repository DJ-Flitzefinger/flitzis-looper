"""Transient confirmed deletion progress over the existing guarded PAD authority.

This owner retains metadata and results only. It owns no executor, reader lease,
Native selection or physical cleanup permission. Removed means the original stem
selection or pending requests were withdrawn; global final-reader cleanup remains
an independent observation of the existing retirement authority.
"""

from collections import Counter, deque
from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal, Protocol

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.stem_delete_inventory import StemDeleteInventory, StemDeleteItem
    from flitzis_looper.controller.stem_job import StemGenerationInterest

type StemDeleteOutcomeState = Literal["removed", "noop", "skipped", "failed", "deferred"]
type StemDeleteItemState = Literal["queued", "removed", "noop", "skipped", "failed"]

_FEED_PER_POLL = 8


@dataclass(frozen=True, slots=True)
class StemDeleteOutcome:
    """An original target's guarded logical outcome, never physical-file completion."""

    state: StemDeleteOutcomeState
    detail: str | None = None


class StemDeleteBackend(Protocol):
    """Existing per-PAD deletion owner retaining all currentness and lifetime guards."""

    def delete_captured_stems(self, item: StemDeleteItem) -> StemDeleteOutcome:
        """Revalidate and withdraw only the exact captured original target."""
        ...


@dataclass(frozen=True, slots=True)
class StemDeleteItemStatus:
    """Detached result preserving an original zero-based PAD identity."""

    pad_id: int
    state: StemDeleteItemState
    detail: str | None = None


@dataclass(frozen=True, slots=True)
class StemDeleteSummary:
    """Immutable logical progress with retained partial results and no cleanup claims."""

    items: tuple[StemDeleteItemStatus, ...]
    total: int
    queued: int
    removed: int
    noop: int
    skipped: int
    failed: int
    active: bool


@dataclass(slots=True)
class _DeleteItem:
    capture: StemDeleteItem
    state: StemDeleteItemState = "queued"
    detail: str | None = None
    logical_withdrawn: bool = False

    def view(self) -> StemDeleteItemStatus:
        return StemDeleteItemStatus(self.capture.pad_id, self.state, self.detail)


class StemDeleteController:
    """Offer confirmed original rows to one existing guarded deletion backend.

    Accept withdraws exactly the captured unadmitted logical caller tokens through
    its existing operation owner. This callback performs no physical cancellation
    and grants no authority over replacement tokens or admitted readers. Polling
    offers at most eight original queued rows per application frame. Deferred
    targets rotate fairly without this owner reading an intermediate project image.

    Args:
        backend: The existing stem deletion/currentness authority.
        withdraw_unadmitted: Metadata-only withdrawal of exact captured logical
            tokens, implemented by their existing unadmitted caller owner.
    """

    def __init__(
        self,
        backend: StemDeleteBackend,
        *,
        withdraw_unadmitted: Callable[
            [frozenset[StemGenerationInterest]], frozenset[StemGenerationInterest]
        ],
    ) -> None:
        self._backend = backend
        self._withdraw_unadmitted = withdraw_unadmitted
        self._rows: tuple[_DeleteItem, ...] | None = None
        self._queued: deque[_DeleteItem] = deque()

    @property
    def active(self) -> bool:
        """Report original rows awaiting guarded logical withdrawal."""
        return bool(self._queued)

    @property
    def items(self) -> tuple[StemDeleteItemStatus, ...]:
        """Return detached original-item views without observing current PAD slots."""
        return tuple(row.view() for row in self._rows) if self._rows is not None else ()

    @property
    def summary(self) -> StemDeleteSummary | None:
        """Retain exact logical results until deliberately dismissed or replaced."""
        if self._rows is None:
            return None
        items = self.items
        counts = Counter(item.state for item in items)
        return StemDeleteSummary(
            items=items,
            total=len(items),
            queued=counts["queued"],
            removed=counts["removed"],
            noop=counts["noop"],
            skipped=counts["skipped"],
            failed=counts["failed"],
            active=self.active,
        )

    def start(self, inventory: StemDeleteInventory) -> bool:
        """Accept exact warning scope unless an earlier operation is still active."""
        if self.active:
            return False
        rows = tuple(_DeleteItem(item) for item in inventory.items if item is not None)
        logical_pending = frozenset(token for row in rows for token in row.capture.logical_pending)
        withdrawn = self._withdraw_unadmitted(logical_pending)
        for row in rows:
            row.logical_withdrawn = bool(row.capture.logical_pending & withdrawn)
        self._rows = rows
        self._queued = deque(rows)
        return True

    def poll(self) -> None:
        """Offer at most eight original rows once each, keeping deferred work fair."""
        for _ in range(min(_FEED_PER_POLL, len(self._queued))):
            row = self._queued.popleft()
            try:
                outcome = self._backend.delete_captured_stems(row.capture)
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                row.state = "failed"
                row.detail = str(error)
                continue
            row.detail = outcome.detail
            if outcome.state == "deferred":
                self._queued.append(row)
            elif outcome.state == "noop" and row.logical_withdrawn:
                row.state = "removed"
                row.detail = "Pending generation withdrawn"
            else:
                row.state = outcome.state

    def dismiss_result(self) -> None:
        """Clear terminal logical results; unfinished original work remains visible."""
        if not self.active:
            self._rows = None
