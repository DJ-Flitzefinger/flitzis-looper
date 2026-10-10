"""Controller ownership/retry policy; real native rejection/ACK is tested in Rust."""

from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.models import PadContentIdentity, ProjectState
from tests.flitzis_looper.conftest import FakePreparedSourceTicket
from tests.flitzis_looper.controller.test_stem_pair_preparation import _PreparedPair, _queued

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stems import _PendingStemPublication
    from flitzis_looper_audio import PreparedSourceTicket


def _rejected(
    controller: AppController,
    audio: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> tuple[list[Callable[[], None]], _PendingStemPublication, _PreparedPair]:
    jobs, prepared, ticket, _ = _queued(controller, audio, monkeypatch, tmp_path)
    jobs[0]()
    controller.stems._poll_pair_preparations()
    pending = controller.stems._pending_stem_publications[0]
    ticket.reason = "timing-changed"
    ticket.status = "rejected"
    controller.stems._poll_stem_publications()
    assert controller.project.stem_cache[0] is pending.entry
    assert not pending.entry.available
    assert prepared.selected
    assert not prepared.discarded
    assert pending.previous_lease is not None
    assert not pending.previous_lease.released
    assert pending.retirement.remaining > 0
    return jobs, pending, prepared


def _submit_fresh(
    controller: AppController, audio: Mock
) -> tuple[_PreparedPair, FakePreparedSourceTicket]:
    prepared = _PreparedPair()
    ticket = FakePreparedSourceTicket("captured")
    audio.capture_prepared_source.return_value = ticket
    audio.prepare_stem_pair.return_value = prepared
    audio.publish_stem_pair.side_effect = lambda *_: setattr(ticket, "status", "pending")
    controller.stems._poll_publication_retries()
    return prepared, ticket


def test_reject_reprepare_transfers_original_owner_and_waits_own_ack_without_extra_worker(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, original, first = _rejected(controller, audio_engine_mock, monkeypatch, tmp_path)
    previous_lease = original.previous_lease
    controller.session.active_sample_ids.add(0)
    controller.stems._poll_publication_retries()
    assert len(jobs) == 1
    controller.session.active_sample_ids.remove(0)
    prepared, ticket = _submit_fresh(controller, audio_engine_mock)
    assert len(jobs) == 2
    assert original.retirement.remaining > 0
    jobs[-1]()
    controller.stems._poll_pair_preparations()
    replacement = controller.stems._pending_stem_publications[0]
    assert replacement is not original
    assert replacement.previous_entry is original.previous_entry
    assert replacement.previous_lease is previous_lease
    assert previous_lease is not None
    assert not previous_lease.released
    assert original.retirement.remaining == 0
    assert first.selected
    assert not first.discarded
    retry = controller.stems._publication_retries.pending[0]
    for _ in range(20):
        controller.stems._poll_stem_publications()
        controller.stems._poll_publication_retries()
    assert retry.attempts == 1
    assert len(jobs) == 2
    assert not prepared.selected
    assert not replacement.entry.available
    ticket.status = "accepted"
    controller.stems._poll_stem_publications()
    assert prepared.selected
    assert controller.project.stem_cache[0].available
    assert not controller.stems._publication_retries.pending
    assert not controller.stems._pending_stem_publications
    assert previous_lease.released
    assert controller._assets._reserved == 0
    assert 0 not in controller.session.stem_generation_errors


@pytest.mark.parametrize("change", ["uuid", "source_path", "selection", "native_request"])
def test_new_current_intent_wins_without_old_retry_publication(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    change: str,
) -> None:
    jobs, pending, prepared = _rejected(controller, audio_engine_mock, monkeypatch, tmp_path)
    if change == "uuid":
        controller.project.pad_content[0] = PadContentIdentity(instance_id="2" * 32)
    elif change == "source_path":
        controller.project.sample_paths[0] = "samples/new.wav"
    elif change == "selection":
        controller.project.stem_cache[0] = pending.entry.model_copy()
    else:
        ticket = FakePreparedSourceTicket("captured")
        ticket.source_request = 2
        audio_engine_mock.capture_prepared_source.return_value = ticket
    current = controller.project.stem_cache[0]
    controller.stems._poll_publication_retries()
    assert len(jobs) == 1
    assert controller.project.stem_cache[0] is current
    assert not controller.stems._pending_stem_publications
    assert not controller.stems._publication_retries.pending
    assert prepared.selected
    assert not prepared.discarded
    assert pending.retirement.remaining == 0
    assert controller._assets._reserved == 0


def test_native_queue_failure_keeps_original_rollback_until_later_admission(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, pending, prepared = _rejected(controller, audio_engine_mock, monkeypatch, tmp_path)
    _submit_fresh(controller, audio_engine_mock)
    audio_engine_mock.publish_stem_pair.side_effect = RuntimeError("native queue full")
    jobs[-1]()
    controller.stems._poll_pair_preparations()
    assert controller.stems._pending_stem_publications[0] is pending
    assert pending.previous_lease is not None
    assert not pending.previous_lease.released
    assert pending.retirement.remaining > 0
    assert not prepared.discarded
    assert controller.stems._publication_retries.pending[0].attempts == 1
    assert "native queue full" in controller.session.stem_generation_errors[0]


def test_changed_returned_pair_selection_cannot_replace_retry_owner(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, pending, _ = _rejected(controller, audio_engine_mock, monkeypatch, tmp_path)
    prepared, _ = _submit_fresh(controller, audio_engine_mock)
    original_json = prepared.selection_json()
    monkeypatch.setattr(
        prepared,
        "selection_json",
        lambda: original_json.replace("b" * 32 + ".json", "3" * 32 + ".json"),
    )
    audio_engine_mock.publish_stem_pair.reset_mock()
    jobs[-1]()
    controller.stems._poll_pair_preparations()
    assert controller.project.stem_cache[0] is pending.entry
    assert controller.stems._pending_stem_publications[0] is pending
    assert pending.previous_lease is not None
    assert not pending.previous_lease.released
    assert pending.retirement.remaining > 0
    audio_engine_mock.publish_stem_pair.assert_not_called()
    assert "selection changed" in controller.session.stem_generation_errors[0]


def test_new_selection_cannot_transfer_an_old_rejected_rollback_owner(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    _, pending, _ = _rejected(controller, audio_engine_mock, monkeypatch, tmp_path)
    newer = pending.entry.model_copy()
    controller.project.stem_cache[0] = newer
    audio_engine_mock.publish_stem_pair.reset_mock()
    with pytest.raises(RuntimeError, match="Another stem publication"):
        controller.stems._queue_stem_publication(
            0, newer, pending.source_ticket, pending.prepared_pair
        )
    audio_engine_mock.publish_stem_pair.assert_not_called()
    assert controller.project.stem_cache[0] is newer
    assert pending.previous_lease is not None
    assert not pending.previous_lease.released
    assert pending.retirement.remaining > 0


def test_attempt_limit_spans_eight_actual_worker_and_rejected_ack_rounds(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, _, _ = _rejected(controller, audio_engine_mock, monkeypatch, tmp_path)
    for attempt in range(1, 9):
        prepared, ticket = _submit_fresh(controller, audio_engine_mock)
        jobs[-1]()
        controller.stems._poll_pair_preparations()
        assert controller.stems._publication_retries.pending[0].attempts == attempt
        assert not prepared.selected
        ticket.status = "rejected"
        ticket.reason = "timing-changed"
        controller.stems._poll_stem_publications()
    controller.stems._poll_publication_retries()
    assert len(jobs) == 9
    assert not controller.stems._publication_retries.pending
    assert not controller.stems._pending_stem_publications
    assert not controller.stems._pair_preparations.pending
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.pair is not None
    assert not entry.available
    assert controller._assets._reserved == 0
    assert "retry limit reached" in controller.session.stem_generation_errors[0]


def test_current_full_mix_selects_disk_only_after_retry_worker_return(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, original, first = _rejected(controller, audio_engine_mock, monkeypatch, tmp_path)
    assert controller.stems.set_stem_mix_mode(0, "full_mix")
    prepared = _PreparedPair(components=False)
    audio_engine_mock.prepare_stem_pair.return_value = prepared
    audio_engine_mock.capture_prepared_source.return_value = FakePreparedSourceTicket("captured")
    audio_engine_mock.publish_stem_pair.reset_mock()
    controller.stems._poll_publication_retries()
    assert not prepared.selected
    jobs[-1]()
    controller.stems._poll_pair_preparations()
    assert prepared.selected
    assert first.selected
    assert not first.discarded
    assert original.retirement.remaining == 0
    assert controller.project.stem_cache[0].available
    assert not controller.stems._publication_retries.pending
    assert not controller.stems._pending_stem_publications
    assert 0 not in controller.stems._resident_pairs
    audio_engine_mock.publish_stem_pair.assert_not_called()


def test_retry_cancellation_does_not_cancel_newer_pair_worker(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    _, pending, _ = _rejected(controller, audio_engine_mock, monkeypatch, tmp_path)
    entry = pending.entry.model_copy()
    controller.project.stem_cache[0] = entry
    content = PadContentIdentity(instance_id="2" * 32)
    controller.project.pad_content[0] = content
    ticket = FakePreparedSourceTicket("captured")
    controller.stems._prepare_pair(0, entry, cast("PreparedSourceTicket", ticket))
    newer = controller.stems._pair_preparations.pending[0]
    controller.stems._poll_publication_retries()
    assert not newer.cancelled.is_set()
    assert not newer.lease.released
    assert newer.retirement.remaining > 0
    assert controller.stems._pair_preparations.pending[0] is newer


@pytest.mark.parametrize("worker_pending", [False, True])
def test_shutdown_retains_first_rejected_verified_disk_selection_for_json_reopen(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    *,
    worker_pending: bool,
) -> None:
    jobs, prepared, ticket, _ = _queued(controller, audio_engine_mock, monkeypatch, tmp_path)
    request = controller.stems._pair_preparations.pending[0]
    controller.project.stem_cache[0] = None
    request.previous_entry = None
    jobs[0]()
    controller.stems._poll_pair_preparations()
    pending = controller.stems._pending_stem_publications[0]
    assert pending.previous_entry is None
    ticket.status = "rejected"
    ticket.reason = "pad-playing"
    controller.stems._poll_stem_publications()
    selected = pending.entry.pair
    if worker_pending:
        _submit_fresh(controller, audio_engine_mock)
    controller.stems.shut_down()
    restored = ProjectState.model_validate_json(controller.project.model_dump_json())
    entry = restored.stem_cache[0]
    assert entry is not None
    assert entry.pair == selected
    assert not entry.available
    assert prepared.selected
    assert not prepared.discarded
    assert pending.retirement.remaining == 0
    assert not controller.stems._publication_retries.pending
    if worker_pending:
        retry_work = request = controller.stems._pair_preparations.pending[0]
        assert retry_work.cancelled.is_set()
        assert not retry_work.lease.released
        jobs[-1]()
        controller.stems._poll_pair_preparations()
        assert retry_work.lease.released
    assert controller._assets._reserved == 0
