"""Batch operation metadata only; no Native, inference or device acceptance claims.

The narrow facade uses real inventory values, stable interest holders and real
cancellation receipts. Its explicit pending/running/read-settlement controls stand
in for the separately tested StemController authorities, never for audio output.
"""

from dataclasses import FrozenInstanceError
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.stem_batch import StemBatchController
from flitzis_looper.controller.stem_batch_inventory import (
    StemBatchInventory,
    StemBatchItem,
    capture_stem_batch_inventory,
    stem_batch_item_is_current,
)
from flitzis_looper.controller.stem_generation import AudioShape
from flitzis_looper.controller.stem_job import (
    DIRECT_STEM_INTEREST,
    StemGenerationInterest,
    StemGenerationInterests,
)
from flitzis_looper.controller.stems import StemGenerationCancellation
from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_batch import StemBatchSummary
    from flitzis_looper.controller.stems import StemController

_SOURCE = (7, "a" * 64, 128, 44_100)
_SHAPE = AudioShape(44_100, 2, 128)


class _ControlledStems:
    """Expose only captured-item admission and original-owner metadata authorities."""

    def __init__(self, pad_ids: tuple[int, ...]) -> None:
        self.project = ProjectState()
        for pad_id in pad_ids:
            self.project.sample_paths[pad_id] = f"samples/material-{pad_id}/original.wav"
        self.unavailable: set[int] = set()
        self.deferred: set[int] = set()
        self.already_present: set[int] = set()
        self.refused: set[int] = set()
        self.owners: dict[int, StemGenerationInterests] = {}
        self.pending: set[StemGenerationInterests] = set()
        self.running: set[StemGenerationInterests] = set()
        self.original_reads: set[StemGenerationInterests] = set()
        self.requests: list[tuple[StemBatchItem, StemGenerationInterest]] = []
        self.checked_items: list[StemBatchItem] = []
        self.cancellations: list[StemGenerationCancellation] = []
        self.observed_receipts: list[StemGenerationCancellation] = []
        self.batch = StemBatchController(cast("StemController", self))

    def inventory(self) -> StemBatchInventory:
        return capture_stem_batch_inventory(
            self.project,
            source_identity=lambda pad_id: None if pad_id in self.unavailable else _SOURCE,
            target_shape=lambda _pad_id: _SHAPE,
        )

    def generation_item_is_current(self, item: StemBatchItem) -> bool:
        self.checked_items.append(item)
        return stem_batch_item_is_current(
            self.project,
            item,
            source_identity=lambda pad_id: None if pad_id in self.unavailable else _SOURCE,
            target_shape=lambda _pad_id: _SHAPE,
        )

    def generation_item_is_deferred(self, item: StemBatchItem) -> bool:
        return item.pad_id in self.deferred

    def request_captured_generation_interest(
        self, item: StemBatchItem, token: StemGenerationInterest
    ) -> StemGenerationInterests | None:
        self.requests.append((item, token))
        if item.pad_id in self.refused or not self.generation_item_is_current(item):
            return None
        owner = self.owners.setdefault(item.pad_id, StemGenerationInterests(frozenset()))
        owner.attach(token)
        if item.pad_id in self.already_present:
            owner.mark_already_present()
        else:
            self.pending.add(owner)
        return owner

    def generation_interest_is_pending(self, owner: StemGenerationInterests) -> bool:
        return owner in self.pending

    def generation_interest_is_running(self, owner: StemGenerationInterests) -> bool:
        return owner in self.running

    def cancel_generation_interest(
        self, owner: StemGenerationInterests, token: StemGenerationInterest
    ) -> StemGenerationCancellation:
        receipt = StemGenerationCancellation(owner.detach(token), owner)
        self.cancellations.append(receipt)
        return receipt

    def generation_cancellation_is_pending(self, receipt: StemGenerationCancellation) -> bool:
        self.observed_receipts.append(receipt)
        return (
            receipt.disposition == "detached"
            and not receipt.owner.committed
            and receipt.owner in self.original_reads
        )


def _summary(case: _ControlledStems) -> StemBatchSummary:
    summary = case.batch.summary
    assert summary is not None
    assert summary.total == sum((
        summary.queued,
        summary.running,
        summary.done,
        summary.skipped,
        summary.failed,
        summary.cancelling,
        summary.cancelled,
    ))
    return summary


def test_start_and_views_are_metadata_only_immutable_and_guard_active_operation() -> None:
    case = _ControlledStems((0, 215))
    inventory = case.inventory()
    before = case.project.model_dump()
    assert case.batch.summary is None
    assert case.batch.items == ()
    assert not case.batch.active

    assert case.batch.start(inventory)
    assert not case.requests
    assert not case.batch.start(inventory)
    case.batch.dismiss_result()
    summary = _summary(case)
    assert summary.queued == summary.total == 2
    assert summary.active
    assert [item.pad_id for item in summary.items] == [0, 215]
    assert case.batch.items == summary.items
    with pytest.raises(FrozenInstanceError):
        summary.total = 99  # type: ignore[misc]
    with pytest.raises(FrozenInstanceError):
        summary.items[0].state = "done"  # type: ignore[misc]
    assert case.project.model_dump() == before


def test_one_admission_exception_keeps_other_original_items_progressing(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    case = _ControlledStems((0, 215))
    request = case.request_captured_generation_interest

    def fail_one(
        item: StemBatchItem, token: StemGenerationInterest
    ) -> StemGenerationInterests | None:
        if item.pad_id == 0:
            message = "Original item admission failed"
            raise RuntimeError(message)
        return request(item, token)

    monkeypatch.setattr(case, "request_captured_generation_interest", fail_one)
    assert case.batch.start(case.inventory())
    case.batch.poll()
    summary = _summary(case)
    assert (summary.failed, summary.queued, summary.active) == (1, 1, True)
    assert summary.items[0].detail == "Original item admission failed"
    assert 215 in case.owners


def test_216_occupied_rows_feed_eight_per_poll_and_finish_without_settings_visibility() -> None:
    case = _ControlledStems(tuple(range(NUM_SAMPLES)))
    inventory = case.inventory()
    before = case.project.model_dump()
    assert case.batch.start(inventory)
    previous_summary = _summary(case)

    for poll_index in range(27):
        case.batch.poll()
        assert len(case.requests) == (poll_index + 1) * 8
        for owner in case.owners.values():
            owner.mark_committed()
    case.batch.poll()

    summary = _summary(case)
    assert previous_summary.done == 0
    assert summary.total == summary.done == NUM_SAMPLES == 216
    assert not summary.active
    assert [item.pad_id for item in summary.items] == list(range(NUM_SAMPLES))
    assert all(item.detail == "Stems saved" for item in summary.items)
    assert [item.pad_id for item, _token in case.requests] == list(range(NUM_SAMPLES))
    assert len({token for _item, token in case.requests}) == NUM_SAMPLES
    assert all(item is inventory.items[item.pad_id] for item, _token in case.requests)
    assert case.project.model_dump() == before


def test_arrangement_fence_defers_original_rows_without_blocking_outside_pad_216() -> None:
    case = _ControlledStems((*range(36), 215))
    inventory = case.inventory()
    case.deferred.update(range(36))
    assert case.batch.start(inventory)
    case.batch.poll()

    assert [item.pad_id for item, _token in case.requests] == [215]
    assert all(item.pad_id == 215 for item in case.checked_items)
    assert _summary(case).queued == 37
    case.project.sample_paths[0] = "samples/replacement/original.wav"
    case.batch.poll()
    assert [item.pad_id for item, _token in case.requests] == [215]
    case.deferred.clear()
    case.batch.poll()

    summary = _summary(case)
    assert summary.skipped == 1
    assert summary.items[0].detail == "Original target changed"
    assert [item.pad_id for item, _token in case.requests] == [215, *range(1, 8)]
    assert all(item is inventory.items[item.pad_id] for item, _token in case.requests)


def test_metadata_waiting_stays_queued_until_authoritative_work_actually_runs() -> None:
    case = _ControlledStems((0,))
    assert case.batch.start(case.inventory())
    case.batch.poll()
    owner = case.owners[0]
    assert _summary(case).queued == 1

    case.running.add(owner)
    case.batch.poll()
    assert _summary(case).running == 1
    case.running.remove(owner)
    case.batch.poll()
    assert _summary(case).queued == 1


def test_stale_unavailable_and_refused_original_rows_skip_without_replacement_admission() -> None:
    case = _ControlledStems((0, 1, 215))
    case.unavailable.add(1)
    inventory = case.inventory()
    original = inventory.items[0]
    assert original is not None
    case.project.sample_paths[0] = "samples/replacement/original.wav"
    case.refused.add(215)
    assert case.batch.start(inventory)
    case.batch.poll()

    summary = _summary(case)
    assert summary.total == summary.skipped == 3
    assert not summary.active
    assert [item.detail for item in summary.items] == [
        "Original target changed",
        "Loaded source identity unavailable",
        "Original target unavailable",
    ]
    assert [item.pad_id for item, _token in case.requests] == [215]
    assert not case.owners
    assert original.path == "samples/material-0/original.wav"


def test_partial_results_and_failure_survive_slot_change_until_dismiss_or_restart() -> None:
    case = _ControlledStems((0, 1, 2, 215))
    case.already_present.add(2)
    assert case.batch.start(case.inventory())
    case.batch.poll()
    case.owners[0].mark_committed()
    case.owners[1].mark_failed("Original producer failed: disk full")
    case.project.sample_paths[1] = "samples/replacement/original.wav"
    case.pending.remove(case.owners[215])
    case.batch.poll()

    summary = _summary(case)
    assert [item.state for item in summary.items] == ["done", "failed", "skipped", "skipped"]
    assert [item.detail for item in summary.items] == [
        "Stems saved",
        "Original producer failed: disk full",
        "Already present",
        "Original generation interest ended",
    ]
    assert not summary.active
    case.batch.poll()
    assert case.batch.summary == summary
    case.batch.dismiss_result()
    assert case.batch.summary is None
    assert case.batch.items == ()
    assert case.batch.start(case.inventory())
    assert _summary(case).queued == 4


def test_all_already_present_results_are_visible_skips_and_empty_start_is_retained() -> None:
    case = _ControlledStems((0, 215))
    case.already_present.update((0, 215))
    assert case.batch.start(case.inventory())
    case.batch.poll()
    summary = _summary(case)
    assert summary.skipped == summary.total == 2
    assert all(item.detail == "Already present" for item in summary.items)
    assert not summary.active

    empty = _ControlledStems(())
    assert empty.batch.start(empty.inventory())
    assert _summary(empty).total == 0
    assert not empty.batch.active
    empty.batch.dismiss_result()
    assert empty.batch.summary is None


def test_cancel_before_feed_has_no_requests_and_never_mutates_persisted_selection() -> None:
    case = _ControlledStems(tuple(range(NUM_SAMPLES)))
    before = case.project.model_dump()
    assert case.batch.start(case.inventory())
    case.batch.cancel()
    case.batch.poll()
    case.batch.cancel()

    summary = _summary(case)
    assert summary.cancelled == summary.total == NUM_SAMPLES
    assert not summary.active
    assert not case.requests
    assert not case.cancellations
    assert case.project.model_dump() == before


def test_unadmitted_capture_is_detached_immutable_metadata_and_excludes_admitted_rows() -> None:
    case = _ControlledStems((0, 215))
    case.unavailable.add(215)
    before = case.project.model_dump()
    assert not case.batch.capture_unadmitted_interests()
    assert case.batch.start(case.inventory())

    captured = case.batch.capture_unadmitted_interests()
    assert set(captured) == {0, 215}
    assert captured[0] is case.batch.capture_unadmitted_interests()[0]
    with pytest.raises(TypeError):
        captured[0] = StemGenerationInterest("replacement")  # type: ignore[index]
    with pytest.raises(FrozenInstanceError):
        captured[0].owner = "replacement"  # type: ignore[misc]
    assert not case.requests
    assert not case.checked_items
    assert not case.cancellations
    assert case.project.model_dump() == before

    case.batch.poll()
    assert _summary(case).items[1].state == "skipped"
    assert not case.batch.capture_unadmitted_interests()
    assert set(captured) == {0, 215}
    assert case.requests[0][1] is captured[0]
    case.owners[0].mark_committed()
    case.batch.poll()
    assert not case.batch.active
    assert not case.batch.capture_unadmitted_interests()
    case.batch.dismiss_result()
    assert not case.batch.capture_unadmitted_interests()
    assert set(captured) == {0, 215}


def test_withdraw_exact_unadmitted_tokens_prevents_later_feed_without_cancelling_others() -> None:
    case = _ControlledStems((0, 1, 215))
    before = case.project.model_dump()
    assert case.batch.start(case.inventory())
    captured = case.batch.capture_unadmitted_interests()
    same_label = StemGenerationInterest(captured[0].owner)
    assert same_label != captured[0]

    withdrawn = frozenset({captured[1], same_label, StemGenerationInterest("foreign")})
    assert case.batch.withdraw_unadmitted_interests(withdrawn) == frozenset({captured[1]})
    summary = _summary(case)
    assert [item.state for item in summary.items] == ["queued", "cancelled", "queued"]
    assert set(case.batch.capture_unadmitted_interests()) == {0, 215}
    assert set(captured) == {0, 1, 215}
    assert case.batch.withdraw_unadmitted_interests(withdrawn) == frozenset()
    assert case.batch.summary == summary
    assert not case.requests
    assert not case.checked_items
    assert not case.cancellations
    assert case.project.model_dump() == before

    case.batch.poll()
    assert [item.pad_id for item, _token in case.requests] == [0, 215]
    assert 1 not in case.owners
    for owner in case.owners.values():
        owner.mark_committed()
    case.batch.poll()
    summary = _summary(case)
    assert (summary.done, summary.cancelled, summary.active) == (2, 1, False)
    case.batch.poll()
    assert case.batch.summary == summary
    assert not case.cancellations
    assert case.project.model_dump() == before


def test_unadmitted_withdrawal_does_not_detach_tokens_admitted_since_warning() -> None:
    case = _ControlledStems(tuple(range(10)))
    direct = StemGenerationInterests()
    foreign = StemGenerationInterest("foreign")
    direct.attach(foreign)
    case.owners[0] = direct
    assert case.batch.start(case.inventory())
    captured = case.batch.capture_unadmitted_interests()
    case.batch.poll()
    case.running.add(direct)
    case.original_reads.add(direct)
    assert set(case.batch.capture_unadmitted_interests()) == {8, 9}
    interests = {pad_id: owner.current for pad_id, owner in case.owners.items()}
    checked = tuple(case.checked_items)
    requests = tuple(case.requests)

    case.batch.withdraw_unadmitted_interests(frozenset({captured[0], captured[8]}))
    assert not case.cancellations
    assert tuple(case.checked_items) == checked
    assert tuple(case.requests) == requests
    assert {pad_id: owner.current for pad_id, owner in case.owners.items()} == interests
    assert direct.current == frozenset({DIRECT_STEM_INTEREST, foreign, captured[0]})
    assert direct in case.original_reads
    assert set(case.batch.capture_unadmitted_interests()) == {9}
    assert _summary(case).items[8].state == "cancelled"

    case.batch.poll()
    assert [item.pad_id for item, _token in case.requests] == [*range(8), 9]
    assert _summary(case).items[0].state == "running"
    assert _summary(case).items[8].state == "cancelled"
    assert not case.cancellations


def test_old_unadmitted_snapshot_cannot_cancel_same_pad_tokens_in_new_operation() -> None:
    case = _ControlledStems((0, 215))
    inventory = case.inventory()
    assert case.batch.start(inventory)
    original = case.batch.capture_unadmitted_interests()
    old_tokens = frozenset(original.values())
    case.batch.withdraw_unadmitted_interests(old_tokens)
    terminal = _summary(case)
    assert terminal.cancelled == terminal.total == 2
    assert not terminal.active
    assert not case.batch.capture_unadmitted_interests()
    case.batch.withdraw_unadmitted_interests(old_tokens)
    assert case.batch.summary == terminal
    case.batch.dismiss_result()
    case.batch.withdraw_unadmitted_interests(old_tokens)
    assert case.batch.summary is None

    assert case.batch.start(inventory)
    replacement = case.batch.capture_unadmitted_interests()
    for pad_id, token in original.items():
        assert replacement[pad_id].owner == token.owner
        assert replacement[pad_id] is not token
    case.batch.withdraw_unadmitted_interests(old_tokens)
    assert _summary(case).queued == 2
    assert case.batch.capture_unadmitted_interests() == replacement
    case.batch.poll()
    assert [item.pad_id for item, _token in case.requests] == [0, 215]
    assert all(token is replacement[item.pad_id] for item, token in case.requests)
    assert not case.cancellations


def test_cancel_detaches_only_batch_token_and_shared_direct_owner_continues() -> None:
    case = _ControlledStems((0, 215))
    direct = StemGenerationInterests()
    foreign = StemGenerationInterest("another-operation")
    direct.attach(foreign)
    case.owners[0] = direct
    assert case.batch.start(case.inventory())
    case.batch.poll()
    case.running.add(direct)
    case.original_reads.add(direct)
    case.batch.poll()
    assert _summary(case).running == 1
    case.batch.cancel()

    summary = _summary(case)
    assert summary.cancelled == 2
    assert not summary.active
    assert direct.current == frozenset({DIRECT_STEM_INTEREST, foreign})
    assert direct in case.pending
    assert direct in case.original_reads
    assert [receipt.disposition for receipt in case.cancellations] == ["remaining", "detached"]
    direct.mark_committed()
    case.batch.poll()
    assert case.batch.summary == summary


def test_queued_cancel_stops_unfed_inventory_and_is_idempotent_for_each_original_token() -> None:
    case = _ControlledStems(tuple(range(20)))
    assert case.batch.start(case.inventory())
    case.batch.poll()
    assert len(case.requests) == 8
    assert _summary(case).queued == 20
    case.batch.cancel()
    case.batch.cancel()
    case.batch.poll()

    summary = _summary(case)
    assert summary.cancelled == summary.total == 20
    assert len(case.requests) == len(case.cancellations) == 8
    assert all(not owner.current for owner in case.owners.values())
    assert not summary.active


def test_owned_original_holder_survives_slot_replacement_without_recapture_or_retarget() -> None:
    case = _ControlledStems((0,))
    inventory = case.inventory()
    assert case.batch.start(inventory)
    case.batch.poll()
    original = case.owners[0]
    case.running.add(original)
    checked = tuple(case.checked_items)
    case.project.sample_paths[0] = "samples/replacement/original.wav"
    replacement = StemGenerationInterests()
    case.pending.add(replacement)
    case.original_reads.add(replacement)
    case.batch.poll()

    summary = _summary(case)
    assert summary.running == 1
    assert summary.items[0].pad_id == 0
    assert tuple(case.checked_items) == checked
    case.batch.cancel()
    assert case.cancellations[0].owner is original
    assert replacement.current == frozenset({DIRECT_STEM_INTEREST})
    assert replacement in case.pending
    assert replacement in case.original_reads
    assert len(case.requests) == 1
    assert case.requests[0][0] is inventory.items[0]


def test_cancelling_waits_for_exact_original_reader_receipt_and_ignores_foreign_reads() -> None:
    case = _ControlledStems((0, 215))
    assert case.batch.start(case.inventory())
    case.batch.poll()
    original = case.owners[0]
    unrelated = StemGenerationInterests()
    case.running.add(original)
    case.original_reads.update((original, unrelated))
    case.batch.cancel()
    receipt, queued_receipt = case.cancellations
    assert receipt.owner is original
    assert queued_receipt.owner is case.owners[215]
    summary = _summary(case)
    assert summary.cancelling == summary.cancelled == 1
    assert summary.active
    case.batch.dismiss_result()
    assert case.batch.summary == summary
    assert not case.batch.start(case.inventory())
    case.batch.poll()
    assert _summary(case).cancelling == 1

    case.original_reads.remove(original)
    case.batch.poll()
    summary = _summary(case)
    assert summary.cancelled == 2
    assert not summary.active
    assert unrelated in case.original_reads
    assert case.observed_receipts[-1] is receipt
    assert len(case.cancellations) == 2


def test_failure_after_cancel_does_not_hide_unsettled_original_reader() -> None:
    case = _ControlledStems((0,))
    assert case.batch.start(case.inventory())
    case.batch.poll()
    owner = case.owners[0]
    case.original_reads.add(owner)
    case.batch.cancel()
    owner.mark_failed("Original worker failed during teardown")
    case.batch.poll()
    assert _summary(case).cancelling == 1
    assert case.batch.active

    case.original_reads.remove(owner)
    case.batch.poll()
    summary = _summary(case)
    assert summary.failed == 1
    assert summary.items[0].detail == "Original worker failed during teardown"
    assert not summary.active


def test_cancel_preserves_committed_failed_and_present_results_and_late_commit_winner() -> None:
    case = _ControlledStems((0, 1, 2, 3, 215))
    case.already_present.add(2)
    assert case.batch.start(case.inventory())
    case.batch.poll()
    case.owners[0].mark_committed()
    case.owners[1].mark_failed("Original inference failure")
    racing = case.owners[3]
    case.original_reads.add(racing)
    case.batch.cancel()
    summary = _summary(case)
    assert [item.state for item in summary.items] == [
        "done",
        "failed",
        "skipped",
        "cancelling",
        "cancelled",
    ]
    assert len(case.cancellations) == 5
    racing.mark_committed()
    case.batch.poll()

    summary = _summary(case)
    assert summary.done == 2
    assert summary.failed == summary.skipped == summary.cancelled == 1
    assert not summary.active
    assert [item.detail for item in summary.items[:3]] == [
        "Stems saved",
        "Original inference failure",
        "Already present",
    ]
