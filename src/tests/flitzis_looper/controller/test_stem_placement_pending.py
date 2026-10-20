"""Actual controller demand handling with explicit mock-Native placement/ACK boundaries.

Cached original metadata comes from the existing loaded-source fixture. Selected
disk-pair registration and residency receipts are substitutes, not Native, DSP,
music, device or complete batch/resource acceptance.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, call

import pytest

from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.models import PadContentIdentity, StemCacheEntry
from flitzis_looper.stem_pair_selection import StemPairSelection
from tests.flitzis_looper.conftest import FakePreparedSourceTicket
from tests.flitzis_looper.controller.test_committed_stem_residency import (
    _RegistrationTicket,
    _ResidencyReceipt,
)
from tests.flitzis_looper.controller.test_new_stem_adoption import _OwnReceipt
from tests.flitzis_looper.controller.test_stem_pair_preparation import _PreparedPair, _selection
from tests.flitzis_looper.controller.test_stems import _load_project_sample

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import PreparedSourceTicket, PreparedStemPair, ResidentStemTicket


PLACEMENT_PENDING = "[resident_stem_placement_pending] content placement publication is in progress"


@pytest.mark.parametrize(
    ("message", "state"),
    [
        ("[resident_pcm_budget_exceeded] requested=4096 budget=1024", "deferred"),
        ("content placement is pending or unavailable", "error"),
    ],
)
def test_mask_pressure_keeps_budget_deferral_and_terminal_policy(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    message: str,
    state: str,
) -> None:
    rig = _registered_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    old = _OwnReceipt(residency="accepted", effective="effective", owned_claim=True)
    rig.request.return_value = old
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    controller.stems.on_frame_render()
    rig.request.side_effect = RuntimeError(message)
    assert controller.stems.set_stem_enabled_mask(0, 5) is (state == "deferred")
    attempts = rig.request.call_count
    controller.stems._committed_residency.poll()
    assert rig.request.call_count == attempts
    for _ in range(3):
        controller.stems.on_frame_render()
    assert rig.request.call_count == attempts + (3 if state == "deferred" else 0)
    assert controller.stems._demands[0].state == state
    assert controller.stems.residency_summary().acked == 0
    assert old.reader_active
    assert old.cancellations == old.withdrawals == 0


@pytest.mark.parametrize("replacement", ["first_registration", "new_mask", "new_candidate"])
def test_pending_after_registration_preserves_automatic_exact_intent_retry(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    replacement: str,
) -> None:
    rig = _registered_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    registration = rig.registration
    mask = 15
    if replacement == "first_registration":
        registration.state = "pending"
    else:
        rig.request.return_value = _OwnReceipt(
            residency="accepted", effective="effective", owned_claim=True
        )
        assert controller.stems.set_stem_mix_mode(0, "all_stems")
        controller.stems.on_frame_render()
        assert controller.stems.residency_summary().acked == 1
    if replacement == "new_candidate":
        registration = _RegistrationTicket(state="pending")
        monkeypatch.setattr(
            audio_engine_mock,
            "register_pending_committed_stem_pair",
            Mock(return_value=registration),
            raising=False,
        )
        assert rig.entry.pair is not None
        pair = rig.entry.pair.model_copy(update={"stem_set_identity": "d" * 64})
        controller.project.stem_cache[0] = rig.entry.model_copy(update={"pair": pair})
        prepared = _PreparedPair(components=False, selection=pair.model_dump(mode="json"))
        prepared.select()
        source = audio_engine_mock.capture_offline_stem_source(0, rig.entry.source_version)
        ticket = FakePreparedSourceTicket()
        ticket.source_request = source.source_request
        controller.stems._register_committed_pair(
            0, cast("PreparedStemPair", prepared), cast("PreparedSourceTicket", ticket)
        )
    fresh = _OwnReceipt()
    rig.request.return_value = fresh
    rig.request.side_effect = RuntimeError(PLACEMENT_PENDING)
    if replacement == "new_mask":
        mask = 5
        assert controller.stems.set_stem_enabled_mask(0, mask)
    else:
        assert controller.stems.set_stem_mix_mode(0, "all_stems")
    registration.state = "accepted"
    for _ in range(3):
        controller.stems.on_frame_render()
        assert controller.stems.residency_summary().acked == 0
    attempts = rig.request.call_count
    rig.request.side_effect = None
    for _ in range(3):
        controller.stems.on_frame_render()
    assert rig.request.call_count > attempts, "the saved exact desire stopped retrying"
    assert rig.request.call_args == call(registration, mode="all_stems", enabled_mask=mask)
    assert controller.stems.residency_summary().acked == 0
    fresh.residency = "accepted"
    fresh.effective = "effective"
    controller.stems.on_frame_render()
    assert controller.stems.residency_summary().acked == 1


@dataclass(slots=True)
class _PlacementControl:
    entry: StemCacheEntry
    registration: _RegistrationTicket
    receipt: _ResidencyReceipt
    request: Mock


def _registered_control(
    controller: AppController,
    audio: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> _PlacementControl:
    """Start at an own registered selection using coherent cached source metadata."""
    source_path = _load_project_sample(controller, tmp_path)
    source_version = controller.stems._loaded_source_version_for_pad(0)
    assert source_version is not None
    assert source_version.startswith(f"{source_path}|sha256-v1:")
    source = audio.capture_offline_stem_source(0, source_version)
    audio.waveform_source_identity.return_value = (
        source.source_generation,
        source.digest,
        source.target_shape[2],
        source.target_shape[0],
    )
    pair = StemPairSelection.model_validate(_selection())
    entry = StemCacheEntry(
        source_version=source_version,
        cache_dir=pair.wav_generation,
        stems=expected_stem_files(pair.wav_generation),
        pair=pair,
        available=True,
    )
    controller.project.stem_cache[0] = entry
    controller.project.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
    controller.project.preload_stem_loops_on_startup = False
    controller.project.pad_stem_mix_mode[0] = "full_mix"
    registration = _RegistrationTicket(state="accepted")
    receipt = _ResidencyReceipt()
    monkeypatch.setattr(
        audio, "register_committed_stem_pair", Mock(return_value=registration), raising=False
    )
    request = Mock(return_value=receipt)
    monkeypatch.setattr(audio, "request_committed_stem_residency", request, raising=False)
    prepared = _PreparedPair(components=False, selection=pair.model_dump(mode="json"))
    prepared.select()
    ticket = FakePreparedSourceTicket()
    ticket.source_request = source.source_request
    controller.stems._register_committed_pair(
        0, cast("PreparedStemPair", prepared), cast("PreparedSourceTicket", ticket)
    )
    assert controller.stems._committed_residency.contains(0)
    assert controller.stems._loaded_source_version_for_pad(0) == entry.source_version
    audio.capture_offline_stem_source.reset_mock()
    audio.capture_prepared_source.reset_mock()
    return _PlacementControl(entry, registration, receipt, request)


def test_placement_pending_retains_original_demand_then_requires_its_own_ack(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    rig = _registered_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    rig.request.side_effect = RuntimeError(PLACEMENT_PENDING)

    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    demand = controller.stems._demands[0]
    original_binding = (
        demand.source_version,
        demand.content_id,
        demand.source_path,
        demand.selection_identity,
    )
    for _ in range(3):
        controller.stems.on_frame_render()
        assert controller.stems._demands[0] is demand
        assert demand.state == "queued"
        assert demand.explicit
        assert controller.project.stem_cache[0] is rig.entry
        assert controller.project.pad_stem_mix_mode[0] == "all_stems"
        summary = controller.stems.residency_summary()
        assert (summary.requested, summary.queued, summary.acked, summary.errors) == (1, 1, 0, 0)
        status = controller.stems.stem_residency_status(0)
        assert status is not None
        assert status.registration == "accepted"
        assert status.residency is status.effective is status.requested_mode is None
    # Only the existing bounded demand owner retries after an admission refusal.
    assert rig.request.call_count == 4
    assert (
        demand.source_version,
        demand.content_id,
        demand.source_path,
        demand.selection_identity,
    ) == original_binding
    assert all(
        observed == call(rig.registration, mode="all_stems", enabled_mask=15)
        for observed in rig.request.call_args_list
    )
    rig.request.side_effect = None

    controller.stems.on_frame_render()

    assert rig.request.call_count == 5
    assert demand.state == "preparing"
    assert controller.stems.residency_summary().acked == 0
    for _ in range(3):
        controller.stems.on_frame_render()
        assert controller.stems.residency_summary().acked == 0
    assert rig.request.call_count == 5
    rig.receipt.residency = "accepted"
    rig.receipt.effective = "effective"

    controller.stems.on_frame_render()

    assert demand.state == "acked"
    assert controller.stems.residency_summary().acked == 1
    assert controller.stems.stem_generation_error(0) is None
    assert rig.registration.cancellations == rig.receipt.cancellations == 0
    audio_engine_mock.capture_offline_stem_source.assert_not_called()
    audio_engine_mock.capture_prepared_source.assert_not_called()


def _retried_placement_control(
    controller: AppController,
    audio: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> _PlacementControl:
    """Retain real demand/error ownership across refusal and one pending own retry."""
    rig = _registered_control(controller, audio, monkeypatch, tmp_path)
    rig.request.side_effect = RuntimeError(PLACEMENT_PENDING)
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    rig.request.side_effect = None
    controller.stems._poll_residency_demands()
    assert controller.stems._demands[0].state == "preparing"
    assert controller.stems._demands[0].error == PLACEMENT_PENDING
    assert controller.stems._residency_errors[0] == PLACEMENT_PENDING
    assert controller.stems.stem_generation_error(0) == PLACEMENT_PENDING
    assert rig.receipt.residency == "pending"
    return rig


def _assert_no_new_residency_work(rig: _PlacementControl, audio: Mock) -> None:
    assert rig.request.call_args_list == [
        call(rig.registration, mode="all_stems", enabled_mask=15),
        call(rig.registration, mode="all_stems", enabled_mask=15),
    ]
    assert rig.registration.cancellations == rig.receipt.cancellations == 0
    audio.capture_offline_stem_source.assert_not_called()
    audio.capture_prepared_source.assert_not_called()


def test_own_stopped_ack_clears_placement_feedback_before_fair_demand_visit(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    rig = _retried_placement_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    demand = controller.stems._demands[0]
    binding = (
        demand.source_version,
        demand.content_id,
        demand.source_path,
        demand.selection_identity,
    )
    before = controller.project.model_dump()
    rig.receipt.residency = "accepted"
    assert rig.receipt.effective == "waiting"

    # This is the all-selection receipt observation, before the eight-owner fair
    # admission poll can revisit this particular stopped PAD's demand.
    controller.stems._poll_committed_residency()

    assert controller.stems._demands[0] is demand
    assert demand.state == "preparing"
    assert (
        demand.source_version,
        demand.content_id,
        demand.source_path,
        demand.selection_identity,
    ) == binding
    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert (status.residency, status.effective, status.requested_mode) == (
        "accepted",
        "waiting",
        "all_stems",
    )
    selection = controller.stems._committed_residency._selections[0]
    assert selection.receipt is cast("ResidentStemTicket", rig.receipt)
    assert selection.receipt_intent == ("all_stems", 15)
    assert controller.project.stem_cache[0] is rig.entry
    assert controller.project.model_dump() == before
    assert demand.error is None
    assert status.demand_error is None
    assert 0 not in controller.stems._residency_errors
    assert controller.stems.stem_generation_error(0) is None
    _assert_no_new_residency_work(rig, audio_engine_mock)


def _preloaded_stopped_control(
    controller: AppController,
    audio: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> tuple[_PlacementControl, _ResidencyReceipt]:
    rig = _registered_control(controller, audio, monkeypatch, tmp_path)
    rig.receipt.residency = "accepted"
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    preload = _ResidencyReceipt(residency="accepted", effective="not_requested")
    rig.request.return_value = preload
    controller.settings.set_preload_stem_loops_on_startup(enabled=True)
    controller.stems._poll_residency_demands()
    return rig, preload


def test_own_all_ack_cannot_clear_unsettled_policy_withdrawal_pressure(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    rig, preload = _preloaded_stopped_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    demand = controller.stems._demands[0]
    selection = controller.stems._committed_residency._selections[0]
    assert selection.preload_receipt is cast("ResidentStemTicket", preload)
    assert (demand.explicit, demand.policy, demand.withdraw_policy, demand.state) == (
        True,
        True,
        False,
        "acked",
    )

    controller.settings.set_preload_stem_loops_on_startup(enabled=False)
    rig.request.side_effect = RuntimeError(PLACEMENT_PENDING)
    controller.stems._poll_residency_demands()
    assert (demand.explicit, demand.policy, demand.withdraw_policy) == (True, False, True)
    assert controller.stems.stem_generation_error(0) == PLACEMENT_PENDING
    before = controller.project.model_dump()
    attempts = rig.request.call_count

    controller.stems._poll_committed_residency()

    assert controller.stems.stem_generation_error(0) == PLACEMENT_PENDING
    assert controller.stems._residency_errors[0] == demand.error == PLACEMENT_PENDING
    assert demand.withdraw_policy
    assert selection.receipt is cast("ResidentStemTicket", rig.receipt)
    assert selection.preload_receipt is cast("ResidentStemTicket", preload)
    assert rig.request.call_count == attempts

    withdrawal = _ResidencyReceipt(effective="not_requested")
    rig.request.side_effect = None
    rig.request.return_value = withdrawal
    controller.stems._poll_residency_demands()
    controller.stems._poll_committed_residency()
    assert selection.preload_receipt is cast("ResidentStemTicket", withdrawal)
    assert demand.withdraw_policy
    assert controller.stems.stem_generation_error(0) == PLACEMENT_PENDING

    withdrawal.residency = "accepted"
    controller.stems._poll_residency_demands()

    observed: tuple[bool, bool, str, str | None] = (
        demand.policy,
        demand.withdraw_policy,
        demand.state,
        demand.error,
    )
    assert observed == (
        False,
        False,
        "acked",
        None,
    )
    assert controller.stems.stem_generation_error(0) is None
    assert 0 not in controller.stems._residency_errors
    assert selection.receipt is cast("ResidentStemTicket", rig.receipt)
    assert selection.receipt_intent == ("all_stems", 15)
    assert (rig.receipt.current, rig.receipt.residency, rig.receipt.effective) == (
        True,
        "accepted",
        "waiting",
    )
    assert controller.project.stem_cache[0] is rig.entry
    assert controller.project.model_dump() == before
    assert rig.request.call_args_list == [
        call(rig.registration, mode="all_stems", enabled_mask=15),
        call(rig.registration, mode="full_mix", interest="preload"),
        call(rig.registration, mode="full_mix", interest="withdraw_preload"),
        call(rig.registration, mode="full_mix", interest="withdraw_preload"),
    ]
    assert rig.registration.cancellations == rig.receipt.cancellations == 0
    assert preload.cancellations == withdrawal.cancellations == 0
    audio_engine_mock.capture_offline_stem_source.assert_not_called()
    audio_engine_mock.capture_prepared_source.assert_not_called()


def test_old_withdrawal_ack_cannot_clear_new_policy_preload_pressure(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    rig, preload = _preloaded_stopped_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    withdrawal = _ResidencyReceipt(residency="accepted", effective="not_requested")
    rig.request.return_value = withdrawal
    controller.settings.set_preload_stem_loops_on_startup(enabled=False)
    controller.stems._poll_residency_demands()
    demand = controller.stems._demands[0]
    selection = controller.stems._committed_residency._selections[0]
    assert selection.preload_receipt is cast("ResidentStemTicket", withdrawal)
    assert selection.preload_enabled is False
    assert (demand.policy, demand.withdraw_policy, demand.state) == (False, False, "acked")

    controller.settings.set_preload_stem_loops_on_startup(enabled=True)
    rig.request.side_effect = RuntimeError(PLACEMENT_PENDING)
    controller.stems._poll_residency_demands()
    assert (demand.explicit, demand.policy, demand.withdraw_policy) == (True, True, False)
    assert controller.stems.stem_generation_error(0) == PLACEMENT_PENDING
    before = controller.project.model_dump()
    attempts = rig.request.call_count

    controller.stems._poll_committed_residency()

    assert controller.stems.stem_generation_error(0) == PLACEMENT_PENDING
    assert controller.stems._residency_errors[0] == demand.error == PLACEMENT_PENDING
    assert selection.preload_receipt is cast("ResidentStemTicket", withdrawal)
    assert selection.preload_enabled is False
    assert selection.receipt is cast("ResidentStemTicket", rig.receipt)
    assert rig.request.call_count == attempts

    fresh = _ResidencyReceipt(effective="not_requested")
    rig.request.side_effect = None
    rig.request.return_value = fresh
    controller.stems._poll_residency_demands()
    controller.stems._poll_committed_residency()
    assert selection.preload_receipt is cast("ResidentStemTicket", fresh)
    assert selection.preload_enabled is True
    assert controller.stems.stem_generation_error(0) == PLACEMENT_PENDING

    fresh.residency = "accepted"
    controller.stems._poll_residency_demands()

    assert (demand.policy, demand.withdraw_policy, demand.state, demand.error) == (
        True,
        False,
        "acked",
        None,
    )
    assert controller.stems.stem_generation_error(0) is None
    assert 0 not in controller.stems._residency_errors
    assert selection.receipt is cast("ResidentStemTicket", rig.receipt)
    assert selection.receipt_intent == ("all_stems", 15)
    assert (rig.receipt.current, rig.receipt.residency, rig.receipt.effective) == (
        True,
        "accepted",
        "waiting",
    )
    assert controller.project.stem_cache[0] is rig.entry
    assert controller.project.model_dump() == before
    assert rig.request.call_args_list == [
        call(rig.registration, mode="all_stems", enabled_mask=15),
        call(rig.registration, mode="full_mix", interest="preload"),
        call(rig.registration, mode="full_mix", interest="withdraw_preload"),
        call(rig.registration, mode="full_mix", interest="preload"),
        call(rig.registration, mode="full_mix", interest="preload"),
    ]
    assert rig.registration.cancellations == rig.receipt.cancellations == 0
    assert preload.cancellations == withdrawal.cancellations == fresh.cancellations == 0
    audio_engine_mock.capture_offline_stem_source.assert_not_called()
    audio_engine_mock.capture_prepared_source.assert_not_called()


@pytest.mark.parametrize("feedback", ["newer_session", "terminal_residency"])
def test_stopped_ack_preserves_newer_or_terminal_feedback(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    feedback: str,
) -> None:
    rig = _retried_placement_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    message = (
        "Newer source-owner feedback" if feedback == "newer_session" else "Native data invalid"
    )
    if feedback == "newer_session":
        controller.session.stem_generation_errors[0] = message
    else:
        controller.stems._report_residency_error(0, message)
    before = controller.project.model_dump()
    rig.receipt.residency = "accepted"

    controller.stems._poll_committed_residency()

    assert controller.stems.stem_generation_error(0) == message
    if feedback == "terminal_residency":
        assert controller.stems._residency_errors[0] == message
    else:
        assert 0 not in controller.stems._residency_errors
        assert controller.stems._demands[0].error is None
    assert rig.receipt.effective == "waiting"
    assert controller.project.model_dump() == before
    _assert_no_new_residency_work(rig, audio_engine_mock)


@pytest.mark.parametrize("ack", ["pending", "stale", "wrong_mask", "rejected_effective"])
def test_missing_or_wrong_own_ack_cannot_clear_placement_feedback(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    ack: str,
) -> None:
    rig = _retried_placement_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    rig.receipt.residency = "accepted"
    expected_error = PLACEMENT_PENDING
    if ack == "pending":
        rig.receipt.residency = "pending"
    elif ack == "stale":
        rig.receipt.current = False
    elif ack == "wrong_mask":
        # Project the newer desired mask while the old accepted operation still
        # names mask15; its own receipt cannot confirm this newer desire.
        controller.project.pad_stem_intent[0] = controller.project.pad_stem_intent[0].model_copy(
            update={"enabled_mask": 5}
        )
    else:
        rig.receipt.effective = "rejected"
        rig.receipt.message = expected_error = "Own mode adoption rejected"
    before = controller.project.model_dump()

    controller.stems._poll_committed_residency()

    assert controller.stems.stem_generation_error(0) == expected_error
    assert controller.stems._residency_errors[0] == expected_error
    assert controller.project.model_dump() == before
    _assert_no_new_residency_work(rig, audio_engine_mock)


@pytest.mark.parametrize("change", ["source", "content", "selection"])
def test_placement_pending_rechecks_original_binding_before_retry(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    change: str,
) -> None:
    rig = _registered_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    rig.request.side_effect = RuntimeError(PLACEMENT_PENDING)
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    original = controller.stems._demands[0]
    if change == "source":
        _load_project_sample(controller, tmp_path, "replacement.wav")
        version = controller.stems._loaded_source_version_for_pad(0)
        assert version is not None
        assert version != rig.entry.source_version
        controller.project.stem_cache[0] = rig.entry.model_copy(update={"source_version": version})
    elif change == "content":
        controller.project.pad_content[0] = PadContentIdentity(instance_id="8" * 32)
    else:
        assert rig.entry.pair is not None
        pair = rig.entry.pair.model_copy(update={"stem_set_identity": "c" * 64})
        controller.project.stem_cache[0] = rig.entry.model_copy(update={"pair": pair})
    replacement = controller.project.stem_cache[0]
    replacement_content = controller.project.pad_content[0]
    controller.session.stem_generation_errors[0] = "newer original-owner feedback"
    rig.request.side_effect = None

    for _ in range(3):
        controller.stems.on_frame_render()

    assert 0 not in controller.stems._demands
    assert not controller.stems._committed_residency.contains(0)
    assert original.state == "queued"
    assert rig.request.call_count == 1
    assert rig.registration.cancellations == 1
    assert rig.receipt.cancellations == 0
    assert controller.project.stem_cache[0] is replacement
    assert controller.project.pad_content[0] is replacement_content
    assert controller.stems.residency_summary().acked == 0
    assert controller.session.stem_generation_errors[0] == "newer original-owner feedback"
    audio_engine_mock.capture_prepared_source.assert_not_called()


def test_full_mix_supersedes_placement_pending_without_replaying_old_all_intent(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    rig = _registered_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    rig.request.side_effect = RuntimeError(PLACEMENT_PENDING)
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    demand = controller.stems._demands[0]
    rig.request.side_effect = None

    assert controller.stems.set_stem_mix_mode(0, "full_mix")
    for _ in range(3):
        controller.stems.on_frame_render()

    assert rig.request.call_args_list == [
        call(rig.registration, mode="all_stems", enabled_mask=15),
        call(rig.registration, mode="full_mix", enabled_mask=15),
    ]
    assert not demand.explicit
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert controller.project.stem_cache[0] is rig.entry
    assert controller.stems.residency_summary().requested == 0
    assert rig.registration.cancellations == rig.receipt.cancellations == 0


def test_invalidating_placement_pending_cancels_only_original_registration(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    rig = _registered_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    rig.request.side_effect = RuntimeError(PLACEMENT_PENDING)
    assert controller.stems.set_stem_mix_mode(0, "all_stems")

    controller.stems.invalidate_stem_cache(0)
    rig.request.side_effect = None
    for _ in range(3):
        controller.stems.on_frame_render()

    assert rig.request.call_count == 1
    assert rig.registration.cancellations == 1
    assert rig.registration.reader_active
    assert rig.receipt.cancellations == 0
    assert 0 not in controller.stems._demands
    assert not controller.stems._committed_residency.contains(0)
    assert controller.project.stem_cache[0] is None
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert controller.stems.residency_summary().requested == 0


@pytest.mark.parametrize(
    "message",
    [
        "content placement is pending or unavailable",
        "committed selection source is stale",
        "committed selection source is unavailable",
        "committed selection belongs to another engine",
        "committed selection is retired",
        "content placement observation generation exhausted",
        f"corrupt source contains {PLACEMENT_PENDING}",
    ],
)
def test_uncoded_or_embedded_placement_errors_remain_terminal(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    message: str,
) -> None:
    rig = _registered_control(controller, audio_engine_mock, monkeypatch, tmp_path)
    rig.request.side_effect = RuntimeError(message)

    assert not controller.stems.set_stem_mix_mode(0, "all_stems")
    demand = controller.stems._demands[0]
    assert demand.state == "error"
    assert demand.error is not None
    assert message in demand.error
    rig.request.side_effect = None
    for _ in range(3):
        controller.stems.on_frame_render()

    assert rig.request.call_count == 1
    assert controller.project.stem_cache[0] is rig.entry
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert controller.stems.residency_summary().acked == 0
    assert controller.stems.residency_summary().errors == 1
    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert status.residency is status.effective is status.requested_mode is None
    assert rig.registration.cancellations == rig.receipt.cancellations == 0
