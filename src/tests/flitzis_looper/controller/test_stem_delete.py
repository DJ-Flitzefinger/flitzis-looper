"""Confirmed deletion operation metadata, with an explicit non-Native backend.

The stub uses real inventory currentness to represent the existing per-PAD
authority. Logical results prove neither physical cleanup nor audio, reader,
resource, musical or device acceptance.
"""

from dataclasses import FrozenInstanceError, replace
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.stem_delete import (
    StemDeleteController,
    StemDeleteOutcome,
)
from flitzis_looper.controller.stem_delete_inventory import (
    StemDeleteRequestIdentity,
    capture_stem_delete_inventory,
    stem_delete_item_currentness,
)
from flitzis_looper.controller.stem_job import StemGenerationInterest
from tests.flitzis_looper.controller.test_stem_delete_inventory import _content, _entry, _metadata

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_delete import StemDeleteSummary
    from flitzis_looper.controller.stem_delete_inventory import (
        StemDeleteInventory,
        StemDeleteItem,
        StemDeletePadMetadata,
    )


class _ControlledDelete:
    """Expose only logical withdrawal; no physical-cancel, Native or UI API exists."""

    def __init__(self, rows: tuple[StemDeletePadMetadata, ...]) -> None:
        self.metadata = {row.pad_id: row for row in rows}
        self.deferred: set[int] = set()
        self.failures: dict[int, OSError | RuntimeError | TypeError | ValueError] = {}
        self.offered: list[StemDeleteItem] = []
        self.reads: list[int] = []
        self.removed: list[StemDeleteItem] = []
        self.withdrawals: list[frozenset[StemGenerationInterest]] = []
        self.logical_pending = {row.pad_id: token for row in rows for token in row.logical_pending}
        self.selection = (74, 5)
        self.operation = StemDeleteController(self, withdraw_unadmitted=self.withdraw_unadmitted)

    def inventory(self) -> StemDeleteInventory:
        rows = tuple(self.metadata.get(pad_id) for pad_id in range(NUM_SAMPLES))
        return capture_stem_delete_inventory(rows)

    def withdraw_unadmitted(
        self, captured: frozenset[StemGenerationInterest]
    ) -> frozenset[StemGenerationInterest]:
        """Withdraw exact unadmitted caller identity, preserving same-label replacements."""
        self.withdrawals.append(captured)
        withdrawn: set[StemGenerationInterest] = set()
        for pad_id, token in tuple(self.logical_pending.items()):
            if token in captured:
                withdrawn.add(token)
                self.logical_pending.pop(pad_id)
                row = self.metadata[pad_id]
                self.metadata[pad_id] = replace(
                    row,
                    logical_pending=tuple(held for held in row.logical_pending if held != token),
                )
        return frozenset(withdrawn)

    def _read_metadata(self, pad_id: int) -> StemDeletePadMetadata | None:
        self.reads.append(pad_id)
        return self.metadata.get(pad_id)

    def delete_captured_stems(self, item: StemDeleteItem) -> StemDeleteOutcome:
        self.offered.append(item)
        current = stem_delete_item_currentness(
            item, read_metadata=self._read_metadata, deferred=item.pad_id in self.deferred
        )
        if current == "deferred":
            return StemDeleteOutcome("deferred", "Original arrangement publication pending")
        if current == "stale":
            return StemDeleteOutcome("skipped", "Original target changed or unavailable")
        if item.pad_id in self.failures:
            raise self.failures[item.pad_id]
        if not (item.set_identities or item.requests or item.logical_pending):
            return StemDeleteOutcome("noop", "No captured stem selection or pending request")
        self.metadata[item.pad_id] = replace(
            self.metadata[item.pad_id],
            current_entry=None,
            retained_entries=(),
            requests=(),
            logical_pending=(),
        )
        self.removed.append(item)
        return StemDeleteOutcome(
            "removed", "Original stem selection and pending requests withdrawn"
        )


def _summary(case: _ControlledDelete) -> StemDeleteSummary:
    summary = case.operation.summary
    assert summary is not None
    assert summary.total == sum((
        summary.queued,
        summary.removed,
        summary.noop,
        summary.skipped,
        summary.failed,
    ))
    return summary


def test_start_is_metadata_only_and_active_results_cannot_be_replaced_or_dismissed() -> None:
    case = _ControlledDelete((_metadata(0), _metadata(215)))
    inventory = case.inventory()
    original = dict(case.metadata)
    assert case.operation.summary is None
    assert case.operation.items == ()
    assert not case.operation.active

    assert case.operation.start(inventory)
    summary = _summary(case)
    assert summary.total == summary.queued == 2
    assert summary.active
    assert [item.pad_id for item in summary.items] == [0, 215]
    assert case.operation.items == summary.items
    assert case.offered == []
    assert case.reads == []
    assert case.removed == []
    assert case.withdrawals == [frozenset()]
    assert case.metadata == original
    assert not case.operation.start(inventory)
    assert case.withdrawals == [frozenset()]
    case.operation.dismiss_result()
    assert case.operation.summary == summary
    with pytest.raises(FrozenInstanceError):
        summary.total = 99  # type: ignore[misc]
    with pytest.raises(FrozenInstanceError):
        summary.items[0].state = "removed"  # type: ignore[misc]
    assert case.selection == (74, 5)


@pytest.mark.parametrize("shared", [False, True])
def test_all216_original_rows_offer_at_most_eight_per_poll_and_keep_exact_results(
    *, shared: bool
) -> None:
    rows = tuple(
        replace(_metadata(pad_id), current_entry=_entry(1 if shared else pad_id + 1))
        for pad_id in range(NUM_SAMPLES)
    )
    case = _ControlledDelete(rows)
    inventory = case.inventory()
    assert len(inventory.set_identities) == (1 if shared else 216)
    assert case.operation.start(inventory)
    before = _summary(case)

    for index in range(27):
        case.operation.poll()
        summary = _summary(case)
        assert len(case.offered) == summary.removed == (index + 1) * 8
        assert summary.queued == NUM_SAMPLES - summary.removed
        assert len(case.removed) == summary.removed

    summary = _summary(case)
    assert before.queued == 216
    assert before.removed == 0
    assert summary.total == summary.removed == 216
    assert not summary.active
    assert [item.pad_id for item in summary.items] == list(range(216))
    assert [item.pad_id for item in case.offered] == list(range(216))
    assert all(item is inventory.items[item.pad_id] for item in case.offered)
    assert case.selection == (74, 5)
    offers = tuple(case.offered)
    case.operation.poll()
    assert tuple(case.offered) == offers
    assert case.operation.summary == summary


def test_pending_only_withdrawal_is_removed_while_empty_original_is_truthful_noop() -> None:
    token = StemGenerationInterest("old-unadmitted-batch")
    case = _ControlledDelete((
        replace(_metadata(0), current_entry=None, logical_pending=(token,)),
        replace(_metadata(1), current_entry=None),
        replace(_metadata(215), current_entry=None, retained_entries=(_entry(),)),
    ))
    inventory = case.inventory()

    assert case.operation.start(inventory)

    assert case.withdrawals == [frozenset({token})]
    assert case.logical_pending == {}
    assert case.offered == []
    case.operation.poll()
    summary = _summary(case)
    assert (summary.removed, summary.noop, summary.skipped, summary.failed) == (2, 1, 0, 0)
    assert [item.state for item in summary.items] == ["removed", "noop", "removed"]
    assert all(item.detail is not None for item in summary.items)
    assert not summary.active
    assert case.selection == (74, 5)


def test_deferred_original_rotates_without_observing_intermediate_or_retargeting() -> None:
    case = _ControlledDelete(tuple(_metadata(pad_id) for pad_id in (*range(36), 215)))
    inventory = case.inventory()
    case.deferred.update(range(36))
    assert case.operation.start(inventory)

    for _ in range(5):
        offered = len(case.offered)
        case.operation.poll()
        assert len(case.offered) - offered == 8

    assert case.reads == [215]
    summary = _summary(case)
    assert (summary.queued, summary.removed) == (36, 1)
    deferred_view = summary.items[0]
    assert deferred_view.state == "queued"
    assert deferred_view.detail == "Original arrangement publication pending"
    replacement = replace(case.metadata[0], path="samples/new/original.wav", content=_content(80))
    case.metadata[0] = replacement
    case.operation.poll()
    assert case.reads == [215]
    case.deferred.clear()

    for _ in range(5):
        case.operation.poll()

    summary = _summary(case)
    assert (summary.removed, summary.skipped, summary.queued) == (36, 1, 0)
    assert summary.items[0].state == "skipped"
    assert case.metadata[0] is replacement
    assert all(item is inventory.items[item.pad_id] for item in case.offered)
    assert deferred_view.state == "queued"
    assert not summary.active
    assert case.selection == (74, 5)


@pytest.mark.parametrize("change", ["source", "content", "set", "request", "unload"])
def test_new_or_stale_original_is_skipped_by_backend_and_never_recaptured(change: str) -> None:
    case = _ControlledDelete((_metadata(0), _metadata(215)))
    inventory = case.inventory()
    assert case.operation.start(inventory)
    original = case.metadata[0]
    if change == "source":
        replacement = replace(original, source_identity=(9, "new-digest", 128, 44_100))
    elif change == "content":
        replacement = replace(original, content=_content(80))
    elif change == "set":
        replacement = replace(original, current_entry=_entry(2))
    elif change == "request":
        replacement = replace(original, requests=(StemDeleteRequestIdentity("material", 2),))
    else:
        replacement = None
    if replacement is None:
        case.metadata.pop(0)
    else:
        case.metadata[0] = replacement

    case.operation.poll()

    summary = _summary(case)
    assert (summary.skipped, summary.removed, summary.failed) == (1, 1, 0)
    assert summary.items[0].state == "skipped"
    assert case.metadata.get(0) is replacement
    assert case.offered[0] is inventory.items[0]
    assert [item.pad_id for item in case.removed] == [215]
    assert case.selection == (74, 5)


def test_accept_withdraws_exact_union_and_preserves_same_label_token_replacement() -> None:
    original = StemGenerationInterest("same-label")
    replacement = StemGenerationInterest("same-label")
    second = StemGenerationInterest("other-captured-batch")
    foreign = StemGenerationInterest("foreign-new-batch")
    case = _ControlledDelete((
        replace(_metadata(0), current_entry=None, logical_pending=(original,)),
        replace(_metadata(215), current_entry=None, logical_pending=(second,)),
    ))
    inventory = case.inventory()
    newer_metadata = replace(case.metadata[0], logical_pending=(replacement,))
    case.metadata[0] = newer_metadata
    case.logical_pending[0] = replacement
    case.logical_pending[42] = foreign
    assert original.owner == replacement.owner
    assert original != replacement

    assert case.operation.start(inventory)

    assert case.withdrawals == [frozenset({original, second})]
    assert case.logical_pending == {0: replacement, 42: foreign}
    case.operation.poll()
    summary = _summary(case)
    assert (summary.skipped, summary.removed, summary.noop) == (1, 1, 0)
    assert case.metadata[0] is newer_metadata
    assert case.logical_pending == {0: replacement, 42: foreign}
    assert all(item is inventory.items[item.pad_id] for item in case.offered)


class _LogicalOnlyDelete(_ControlledDelete):
    def delete_captured_stems(self, item: StemDeleteItem) -> StemDeleteOutcome:
        self.offered.append(item)
        return StemDeleteOutcome("noop", "No admitted stem selection or pending request")


@pytest.mark.parametrize("already_withdrawn", [False, True])
def test_logical_only_result_uses_actual_accept_time_withdrawal_receipt(
    *, already_withdrawn: bool
) -> None:
    token = StemGenerationInterest("original logical batch")
    case = _LogicalOnlyDelete((
        replace(_metadata(0), current_entry=None, logical_pending=(token,)),
    ))
    inventory = case.inventory()
    if already_withdrawn:
        assert case.withdraw_unadmitted(frozenset({token})) == frozenset({token})

    assert case.operation.start(inventory)
    case.operation.poll()

    summary = _summary(case)
    assert summary.removed == int(not already_withdrawn)
    assert summary.noop == int(already_withdrawn)
    assert not case.logical_pending
    assert case.offered == [inventory.items[0]]
    if not already_withdrawn:
        assert summary.items[0].detail == "Pending generation withdrawn"


@pytest.mark.parametrize("failure_type", [OSError, RuntimeError, TypeError, ValueError])
def test_item_failure_keeps_partial_results_and_can_only_be_dismissed_when_terminal(
    failure_type: type[OSError | RuntimeError | TypeError | ValueError],
) -> None:
    case = _ControlledDelete((
        _metadata(0),
        _metadata(1),
        replace(_metadata(215), current_entry=None),
    ))
    case.failures[0] = failure_type("Original logical withdrawal refused")
    assert case.operation.start(case.inventory())

    case.operation.poll()

    summary = _summary(case)
    assert (summary.failed, summary.removed, summary.noop, summary.queued) == (1, 1, 1, 0)
    assert summary.items[0].detail == "Original logical withdrawal refused"
    assert not summary.active
    offers = tuple(case.offered)
    case.operation.poll()
    assert tuple(case.offered) == offers
    assert case.operation.summary == summary
    case.operation.dismiss_result()
    assert case.operation.summary is None
    assert case.operation.items == ()
    assert summary.failed == summary.removed == summary.noop == 1
    assert case.selection == (74, 5)


def test_terminal_result_replacement_repeats_deletion_as_visible_noop() -> None:
    case = _ControlledDelete((_metadata(0), _metadata(215)))
    assert case.operation.start(case.inventory())
    case.operation.poll()
    original = _summary(case)
    assert original.removed == 2

    assert case.operation.start(case.inventory())
    assert _summary(case).queued == 2
    case.operation.poll()

    summary = _summary(case)
    assert summary.noop == 2
    assert summary.removed == summary.failed == summary.skipped == 0
    assert not summary.active
    assert original.removed == 2
    assert original.noop == 0
    assert case.withdrawals == [frozenset(), frozenset()]


def test_empty_confirmed_inventory_retains_terminal_zero_result_until_dismissal() -> None:
    case = _ControlledDelete(())

    assert case.operation.start(case.inventory())
    case.operation.poll()

    summary = _summary(case)
    assert summary.total == summary.queued == summary.removed == 0
    assert not summary.active
    assert case.withdrawals == [frozenset()]
    assert case.offered == []
    case.operation.dismiss_result()
    assert case.operation.summary is None
