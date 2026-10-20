"""Ordinary-to-candidate ownership handoff; declared receipts are not Native proof."""

from typing import TYPE_CHECKING, cast

import pytest

from tests.flitzis_looper.controller.test_new_stem_adoption import _OwnReceipt
from tests.flitzis_looper.controller.test_new_stem_adoption import adoption as adoption_fixture

if TYPE_CHECKING:
    from flitzis_looper.models import StemMixMode
    from tests.flitzis_looper.controller.test_new_stem_adoption import _Harness, _Registration


adoption = adoption_fixture


def _ordinary_request(rig: _Harness, *, claimed: bool) -> tuple[_Registration, _OwnReceipt]:
    rig.bind("b")
    receipt = _OwnReceipt(owned_claim=claimed)
    rig.audio.receipts.append(receipt)
    assert rig.helper.request(0, "all_stems", 5)
    return rig.audio.registrations[-1], receipt


@pytest.mark.parametrize("claimed", [False, True])
def test_first_live_registration_retains_the_exact_ordinary_request_and_latest_desire(
    adoption: _Harness, *, claimed: bool
) -> None:
    rig = adoption
    predecessor, receipt = _ordinary_request(rig, claimed=claimed)
    original = rig.helper._selections[0]
    assert cast("object", original.receipt) is receipt
    assert original.desired == original.receipt_intent == ("all_stems", 5)

    rig.bind("c", active=False)

    live = rig.helper._live_selections[0]
    assert 0 not in rig.helper._selections
    assert live.control is original
    assert live.attempt is original
    assert cast("object", live.attempt.receipt) is receipt
    assert live.desired == ("all_stems", 5)
    assert len(rig.audio.admissions) == 1
    assert predecessor.current
    assert predecessor.cancellations == receipt.cancellations == receipt.withdrawals == 0
    assert receipt.reader_active


@pytest.mark.parametrize("mode", ["all_stems", "full_mix"])
def test_unclaimed_ordinary_request_is_withdrawn_before_the_latest_mode_is_admitted(
    adoption: _Harness, mode: StemMixMode
) -> None:
    rig = adoption
    predecessor, old = _ordinary_request(rig, claimed=False)
    latest = _OwnReceipt()
    rig.audio.receipts.append(latest)
    rig.bind("c", active=False)

    assert rig.helper.request(0, mode, 9)
    rig.helper.poll()

    assert old.withdrawals == 1
    assert not old.current
    assert old.cancellations == 0
    assert old.reader_active
    assert predecessor.current
    assert predecessor.cancellations == 0
    target = rig.audio.candidates[-1] if mode == "all_stems" else predecessor
    assert rig.audio.admissions == [(predecessor, "all_stems", 5), (target, mode, 9)]
    live = rig.helper._live_selections[0]
    assert live.desired == (mode, 9)
    assert live.attempt is not None
    assert cast("object", live.attempt.receipt) is latest
    assert live.attempt.receipt_intent == (mode, 9)
    assert not rig.errors


@pytest.mark.parametrize("mode", ["all_stems", "full_mix"])
@pytest.mark.parametrize("terminal_effect", ["waiting", "effective"])
def test_claimed_ordinary_request_waits_for_its_actual_terminal_state_before_latest_intent(
    adoption: _Harness, mode: StemMixMode, terminal_effect: str
) -> None:
    rig = adoption
    predecessor, old = _ordinary_request(rig, claimed=True)
    original = rig.helper._selections[0]
    latest = _OwnReceipt()
    rig.audio.receipts.append(latest)
    rig.bind("c", active=False)
    assert rig.helper.request(0, "all_stems", 2)
    assert rig.helper.request(0, mode, 9)

    for _ in range(3):
        rig.helper.poll()
        live = rig.helper._live_selections[0]
        assert live.attempt is original
        assert cast("object", live.attempt.receipt) is old
        assert live.desired == (mode, 9)
        assert len(rig.audio.admissions) == 1
        assert not rig.audio.candidates
        assert old.current
        assert old.cancellations == predecessor.cancellations == 0
    old.residency = "accepted"
    old.effective = "transitioning"
    rig.helper.poll()
    assert len(rig.audio.admissions) == 1
    assert not rig.audio.candidates
    assert rig.helper._live_selections[0].attempt is original

    old.effective = terminal_effect
    rig.helper.poll()

    target = rig.audio.candidates[-1] if mode == "all_stems" else predecessor
    assert rig.audio.admissions == [(predecessor, "all_stems", 5), (target, mode, 9)]
    live = rig.helper._live_selections[0]
    assert live.control is original
    assert live.attempt is not None
    assert cast("object", live.attempt.receipt) is latest
    assert live.desired == (mode, 9)
    assert old.reader_active
    assert old.cancellations == predecessor.cancellations == 0
    assert all(error is None for _, error in rig.errors)
    if terminal_effect == "effective":
        assert rig.errors == [(0, None)]
    else:
        assert not rig.errors


def test_repeated_disk_completion_keeps_the_claimed_ordinary_owner_and_coalesces_latest_set(
    adoption: _Harness,
) -> None:
    rig = adoption
    predecessor, old = _ordinary_request(rig, claimed=True)
    original = rig.helper._selections[0]
    latest = _OwnReceipt()
    rig.audio.receipts.append(latest)
    for generation, mask in (("c", 2), ("d", 9), ("e", 1)):
        rig.bind(generation, active=False)
        assert rig.helper.request(0, "all_stems", mask)
        rig.helper.poll()
        assert rig.helper._live_selections[0].attempt is original
        assert cast("object", original.receipt) is old
        assert len(rig.audio.admissions) == 1
        assert not rig.audio.candidates
    old.residency = "accepted"
    old.effective = "waiting"

    rig.helper.poll()

    assert [selection.identity for selection in rig.audio.candidates] == ["e" * 64]
    assert rig.audio.admissions[-1] == (rig.audio.candidates[-1], "all_stems", 1)
    assert len(rig.audio.admissions) == 2
    assert predecessor.cancellations == old.cancellations == 0
    assert old.reader_active
    assert not rig.errors


@pytest.mark.parametrize("claimed", [False, True])
@pytest.mark.parametrize("change", ["source", "content", "unload"])
def test_handed_off_ordinary_owner_is_cancelled_only_when_its_content_source_ends(
    adoption: _Harness, change: str, *, claimed: bool
) -> None:
    rig = adoption
    predecessor, old = _ordinary_request(rig, claimed=claimed)
    rig.bind("c", active=False)
    prior_candidates = tuple(rig.audio.candidates)
    if change == "unload":
        rig.image.pop(0)
        rig.helper.poll()
        assert not rig.helper.contains(0)
    else:
        if change == "source":
            rig.bind("d", active=False, source="2", path="samples/new-source.wav")
        else:
            rig.bind("d", active=False, content="replacement-content")
        live = rig.helper._live_selections[0]
        assert live.control is None
        assert live.attempt is None
        assert live.desired is None
        assert live.disk.selection is not None
        assert cast("object", live.disk.selection.ticket) is not predecessor
    assert len(rig.audio.admissions) == 1
    assert predecessor.cancellations == old.cancellations == 1
    assert old.reader_active
    assert predecessor.reader_active
    assert all(candidate.cancellations == 1 for candidate in prior_candidates)
    assert not rig.errors


@pytest.mark.parametrize("launch_current", [False, True])
def test_settled_ordinary_control_does_not_replay_historical_desire_on_first_live_registration(
    adoption: _Harness, *, launch_current: bool
) -> None:
    rig = adoption
    predecessor = rig.effective_v1()
    original = rig.helper._selections[0]
    receipt = cast("_OwnReceipt", original.receipt)
    assert receipt.residency == "accepted"
    assert receipt.owned_claim
    assert original.desired == ("all_stems", 15)
    receipt.current = launch_current

    rig.bind("c", active=False)

    live = rig.helper._live_selections[0]
    assert live.control is original
    assert live.attempt is None
    assert live.desired is None
    for _ in range(3):
        rig.helper.poll()
        assert len(rig.audio.admissions) == 1
        assert live.attempt is None
        assert live.desired is None
    assert predecessor.cancellations == receipt.cancellations == receipt.withdrawals == 0
    assert predecessor.reader_active
    assert receipt.reader_active
    assert all(error is None for _, error in rig.errors)

    fresh = _OwnReceipt()
    rig.audio.receipts.append(fresh)
    assert rig.helper.request(0, "all_stems", 9)

    assert rig.audio.admissions[-1] == (rig.audio.candidates[-1], "all_stems", 9)
    assert len(rig.audio.admissions) == 2
    assert live.desired == ("all_stems", 9)
    assert live.attempt is not None
    assert cast("object", live.attempt.receipt) is fresh
    assert all(error is None for _, error in rig.errors)
