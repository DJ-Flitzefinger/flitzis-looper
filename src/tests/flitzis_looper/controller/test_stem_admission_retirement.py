"""Actual pool admission includes failed Native discard custody and fresh fences."""

from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.stem_job import StemGenerationInterests
from flitzis_looper.controller.stem_pair_preparation import StemPairIntentBinding
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.conftest import FakePreparedSourceTicket
from tests.flitzis_looper.controller.test_stem_admission import _held_pool
from tests.flitzis_looper.controller.test_stem_pending_admission import _production_pool

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_workers import StemWorkerReservation
    from flitzis_looper_audio import PreparedSourceTicket, PreparedStemPair


def _request_pair(controller: AppController, audio: Mock) -> StemGenerationInterests:
    audio.prepare_stem_pair = Mock()
    stems = controller.stems
    version = stems._loaded_source_version_for_pad(0)
    assert version is not None
    entry = stems._eligible_generation_entry(0, version)
    assert entry is not None
    owner = StemGenerationInterests()
    assert stems._prepare_pair(
        0,
        entry,
        stems._capture_source_ticket(0, version),
        binding=StemPairIntentBinding(own_source_version=version, interest_owner=owner),
    )
    assert stems.generation_interest_is_pending(owner)
    assert not stems._pair_preparations.pending
    return owner


def test_failed_discards_bound_new_readers_even_after_executor_capacity_returns(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        queue = stems._pair_preparations
        retained = [Mock() for _ in range(34)]
        for prepared in retained:
            prepared.discard.side_effect = RuntimeError("Native retirement queue full")
            assert queue.discard(1, cast("PreparedStemPair", prepared)) is not None
        assert queue.retained_preparation_count() == stems._stem_preparation_count() == 34
        owner = _request_pair(controller, audio_engine_mock)
        original = stems._requested_preparations[0]
        reserved = stems._assets._reserved
        audio_engine_mock.acquire_project_asset_lease.reset_mock()
        try:
            for _ in range(3):
                stems.on_frame_render()
                assert stems._requested_preparations[0] is original
                assert stems.generation_interest_is_pending(owner)
                assert not queue.pending
                assert stems._stem_preparation_count() == 34
                assert stems._assets._reserved == reserved
                assert owner.failure is None
                audio_engine_mock.acquire_project_asset_lease.assert_not_called()
                audio_engine_mock.prepare_stem_pair.assert_not_called()
            # Workers remain blocked but queue capacity is genuinely available.
            reservation = held.pool.try_reserve()
            assert reservation is not None
            reservation.close()
            retained[0].discard.side_effect = None
            assert queue.discard(1, cast("PreparedStemPair", retained[0])) is None
            assert stems._stem_preparation_count() == 33
            stems.on_frame_render()
            assert 0 not in stems._requested_preparations
            assert queue.pending[0].interest_owner is owner
            assert len(queue._discard_retries) == 33
            assert stems._stem_preparation_count() == 34
            audio_engine_mock.prepare_stem_pair.assert_not_called()
        finally:
            stems.shut_down()


@pytest.mark.parametrize("change", ["content", "source_request"])
def test_preparation_revalidates_after_reservation_and_settles_original_outcome(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    change: str,
) -> None:
    with _held_pool() as held:
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        owner = _request_pair(controller, audio_engine_mock)
        reserve = held.pool.try_reserve
        original = stems._requested_preparations[0]
        reserved = stems._assets._reserved
        changed = False

        def reserve_then_change_original() -> StemWorkerReservation | None:
            nonlocal changed
            reservation = reserve()
            if reservation is not None and not changed:
                changed = True
                if change == "content":
                    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
                else:
                    ticket = FakePreparedSourceTicket()
                    ticket.source_request = 2
                    audio_engine_mock.capture_prepared_source.side_effect = None
                    audio_engine_mock.capture_prepared_source.return_value = cast(
                        "PreparedSourceTicket", ticket
                    )
            return reservation

        monkeypatch.setattr(held.pool, "try_reserve", reserve_then_change_original)
        audio_engine_mock.acquire_project_asset_lease.reset_mock()
        try:
            stems.on_frame_render()
            assert changed
            assert owner.failure is not None
            assert not owner.committed
            assert not stems.generation_interest_is_pending(owner)
            assert 0 not in stems._requested_preparations
            assert original.spec.binding is not None
            assert original.spec.binding.interest_owner is owner
            assert not stems._pair_preparations.pending
            assert stems._assets._reserved == reserved
            assert stems.stem_generation_error(0) is None
            audio_engine_mock.acquire_project_asset_lease.assert_not_called()
            audio_engine_mock.prepare_stem_pair.assert_not_called()
            reservation = reserve()
            assert reservation is not None
            reservation.close()
        finally:
            stems.shut_down()
