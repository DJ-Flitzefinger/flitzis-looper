"""Transient all-loaded operation views over the existing stem-interest owners.

This module admits only original immutable inventory rows. It owns no executor,
source lease, publication permit or durable state; the StemController remains the
authority for admission, worker lifetime, persistence and cancellation settlement.
"""

from collections import Counter
from dataclasses import dataclass
from types import MappingProxyType
from typing import TYPE_CHECKING, Literal

from flitzis_looper.controller.stem_job import StemGenerationInterest

if TYPE_CHECKING:
    from collections.abc import Mapping

    from flitzis_looper.controller.stem_batch_inventory import StemBatchInventory, StemBatchItem
    from flitzis_looper.controller.stem_job import StemGenerationInterests
    from flitzis_looper.controller.stems import StemController, StemGenerationCancellation

type StemBatchItemState = Literal[
    "queued", "running", "done", "skipped", "failed", "cancelling", "cancelled"
]

_FEED_PER_POLL = 8
_TERMINAL_STATES = frozenset({"done", "skipped", "failed", "cancelled"})


@dataclass(frozen=True, slots=True)
class StemBatchItemStatus:
    """Detached UI result retaining the original zero-based PAD identity."""

    pad_id: int
    state: StemBatchItemState
    detail: str | None = None


@dataclass(frozen=True, slots=True)
class StemBatchSummary:
    """Immutable progress over occupied original rows, including partial results."""

    items: tuple[StemBatchItemStatus, ...]
    total: int
    queued: int
    running: int
    done: int
    skipped: int
    failed: int
    cancelling: int
    cancelled: int
    active: bool


@dataclass(slots=True)
class _BatchItem:
    capture: StemBatchItem
    token: StemGenerationInterest
    state: StemBatchItemState = "queued"
    detail: str | None = None
    owner: StemGenerationInterests | None = None
    cancellation: StemGenerationCancellation | None = None

    def view(self) -> StemBatchItemStatus:
        return StemBatchItemStatus(self.capture.pad_id, self.state, self.detail)


class StemBatchController:
    """Feed one confirmed inventory through ordinary captured-interest admission.

    Poll this control owner once per application frame independently of Settings
    visibility. Each poll offers at most eight original rows; actual worker and
    queue admission remains with the existing shared StemController arbiter.

    Args:
        stems: The existing project StemController, never a second job owner.
    """

    def __init__(self, stems: StemController) -> None:
        self._stems = stems
        self._rows: list[_BatchItem] | None = None
        self._cancel_requested = False

    @property
    def active(self) -> bool:
        """Report unfinished original work, including actual cancellation readers."""
        return self._rows is not None and any(
            row.state not in _TERMINAL_STATES for row in self._rows
        )

    @property
    def items(self) -> tuple[StemBatchItemStatus, ...]:
        """Return detached original-item views without observing replacement slots."""
        return tuple(row.view() for row in self._rows) if self._rows is not None else ()

    @property
    def summary(self) -> StemBatchSummary | None:
        """Return retained progress, or None before start and after result dismissal."""
        if self._rows is None:
            return None
        items = self.items
        counts = Counter(item.state for item in items)
        return StemBatchSummary(
            items=items,
            total=len(items),
            queued=counts["queued"],
            running=counts["running"],
            done=counts["done"],
            skipped=counts["skipped"],
            failed=counts["failed"],
            cancelling=counts["cancelling"],
            cancelled=counts["cancelled"],
            active=self.active,
        )

    def start(self, inventory: StemBatchInventory) -> bool:
        """Retain one confirmed snapshot unless another operation is still active.

        Args:
            inventory: The exact immutable 216-slot warning-confirmed snapshot.

        Returns:
            Whether a new operation replaced the previous terminal result.
        """
        if self.active:
            return False
        self._rows = [
            _BatchItem(item, StemGenerationInterest(f"batch-pad-{item.pad_id + 1}"))
            for item in inventory.items
            if item is not None
        ]
        self._cancel_requested = False
        return True

    def capture_unadmitted_interests(self) -> Mapping[int, StemGenerationInterest]:
        """Detach exact queued caller tokens by original PAD without admitting work.

        The immutable snapshot retains the frozen token objects themselves. PAD
        numbers and diagnostic owner labels never substitute for caller identity;
        admitted rows belong to the existing StemController instead.

        Returns:
            Original PAD IDs mapped to their still-unadmitted queued tokens.
        """
        return MappingProxyType({
            row.capture.pad_id: row.token
            for row in self._rows or ()
            if row.state == "queued" and row.owner is None
        })

    def withdraw_unadmitted_interests(
        self, captured: frozenset[StemGenerationInterest]
    ) -> frozenset[StemGenerationInterest]:
        """Cancel only matching original queued tokens that never gained an owner.

        This metadata-only withdrawal cannot cancel admitted readers or a newer
        operation's tokens. Cancelled rows remain terminal, so later polling
        cannot regenerate stems after their captured deletion was accepted.

        Args:
            captured: Exact frozen caller tokens retained by the deletion warning.

        Returns:
            Only tokens actually withdrawn by this call, for truthful operation feedback.
        """
        withdrawn: set[StemGenerationInterest] = set()
        for row in self._rows or ():
            if row.state == "queued" and row.owner is None and row.token in captured:
                row.state = "cancelled"
                withdrawn.add(row.token)
        return frozenset(withdrawn)

    def poll(self) -> None:
        """Observe original holders and feed a bounded number of unadmitted rows."""
        if self._rows is None:
            return
        for row in self._rows:
            self._refresh(row)
        if self._cancel_requested:
            return
        offered = 0
        for row in self._rows:
            if (
                row.state != "queued"
                or row.owner is not None
                or self._stems.generation_item_is_deferred(row.capture)
            ):
                continue
            self._admit(row)
            offered += 1
            if offered == _FEED_PER_POLL:
                break

    def cancel(self) -> None:
        """Withdraw only this operation's tokens and retain exact reader receipts."""
        if not self.active or self._rows is None:
            return
        self._cancel_requested = True
        for row in self._rows:
            self._refresh(row)
            if row.owner is None:
                if row.state not in _TERMINAL_STATES:
                    row.state = "cancelled"
                continue
            if row.cancellation is None:
                row.cancellation = self._stems.cancel_generation_interest(row.owner, row.token)
            if row.state not in _TERMINAL_STATES:
                row.state = "cancelling"
                self._refresh(row)

    def dismiss_result(self) -> None:
        """Clear a retained terminal result; active work cannot be dismissed."""
        if not self.active:
            self._rows = None

    def _admit(self, row: _BatchItem) -> None:
        if row.capture.unavailable_reason is not None:
            row.state = "skipped"
            row.detail = row.capture.unavailable_reason
        elif not self._stems.generation_item_is_current(row.capture):
            row.state = "skipped"
            row.detail = "Original target changed"
        else:
            try:
                row.owner = self._stems.request_captured_generation_interest(row.capture, row.token)
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                row.state = "failed"
                row.detail = str(error)
                return
            if row.owner is None:
                row.state = "skipped"
                row.detail = "Original target unavailable"
            else:
                self._refresh(row)

    def _refresh(self, row: _BatchItem) -> None:
        if row.state in _TERMINAL_STATES or row.owner is None:
            return
        if row.cancellation is not None:
            if self._stems.generation_cancellation_is_pending(row.cancellation):
                return
            if not self._refresh_outcome(row):
                row.state = "cancelled"
            return
        if self._refresh_outcome(row):
            return
        if row.token not in row.owner.current or not self._stems.generation_interest_is_pending(
            row.owner
        ):
            row.state = "skipped"
            row.detail = "Original generation interest ended"
        else:
            row.state = (
                "running" if self._stems.generation_interest_is_running(row.owner) else "queued"
            )

    @staticmethod
    def _refresh_outcome(row: _BatchItem) -> bool:
        owner = row.owner
        if owner is None:
            return False
        if owner.committed:
            row.state = "done"
            row.detail = "Stems saved"
        elif owner.already_present:
            row.state = "skipped"
            row.detail = "Already present"
        elif owner.failure is not None:
            row.state = "failed"
            row.detail = owner.failure
        else:
            return False
        return True
