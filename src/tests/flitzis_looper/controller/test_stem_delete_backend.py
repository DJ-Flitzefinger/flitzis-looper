"""Real deletion control/retirement joins; mocked readers grant no device acceptance."""

from dataclasses import replace
from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller import committed_stem_residency as residency_module
from flitzis_looper.controller import stems as stems_module
from flitzis_looper.controller.stem_delete_inventory import (
    StemDeleteItem,
    stem_delete_request_identity,
)
from flitzis_looper.controller.stem_job import StemGenerationInterest, StemSubscriber
from flitzis_looper.controller.stem_pair_preparation import PairPreparation
from flitzis_looper.controller.stem_publication_retry import StemPublicationRetry
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.controller import test_stem_disk_commit as disk_fixtures
from tests.flitzis_looper.controller.test_stem_admission import _held_pool
from tests.flitzis_looper.controller.test_stem_batch_integration import _loaded_metadata
from tests.flitzis_looper.controller.test_stem_delete_inventory import _entry, _intent
from tests.flitzis_looper.controller.test_stem_pending_admission import _production_pool

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.asset_lifecycle import AssetRetirementReservation
    from flitzis_looper.controller.stem_intent import StemGenerationIntent
    from flitzis_looper.controller.stem_job import StemGenerationJob
    from flitzis_looper.controller.stems import StemController, _StemBackendEvent
    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper_audio import (
        CommittedStemSetTicket,
        OfflineStemSourceLease,
        PreparedSourceTicket,
        PreparedStemPair,
        ProjectAssetLease,
    )
    from tests.flitzis_looper.controller.test_stem_disk_commit import _DiskCommit

disk_commit = disk_fixtures.disk_commit
_SOURCE = (7, "a" * 64, 128, 44_100)


def _loaded(controller: AppController, audio: Mock, pads: tuple[int, ...] = (0, 215)) -> None:
    for pad_id in pads:
        controller.project.sample_paths[pad_id] = f"samples/shared-{pad_id}/original.wav"
        controller.project.pad_content[pad_id] = PadContentIdentity(
            instance_id=f"{pad_id + 1:032x}"
        )
    audio.waveform_source_identity.side_effect = lambda pad_id: _SOURCE if pad_id in pads else None


def _first(controller: AppController) -> StemDeleteItem:
    item = controller.stems.capture_deletion_inventory().items[0]
    assert item is not None
    return item


def _publication(
    entry: StemCacheEntry, intent: StemGenerationIntent
) -> stems_module._PendingStemPublication:
    return stems_module._PendingStemPublication(
        sample_id=0,
        source_ticket=cast("PreparedSourceTicket", Mock()),
        entry=entry,
        previous_entry=None,
        previous_lease=None,
        retirement=cast("AssetRetirementReservation", Mock()),
        generation_intent=intent,
    )


def _install_retained_controls(
    stems: StemController,
) -> tuple[residency_module._CommittedSelection, ...]:
    selection = residency_module._CommittedSelection(
        _entry(3), None, None, cast("CommittedStemSetTicket", Mock())
    )
    disk = residency_module._DiskCandidate(
        _entry(4),
        None,
        None,
        cast("PreparedStemPair", Mock()),
        cast("PreparedSourceTicket", Mock()),
    )
    disk.selection = residency_module._CommittedSelection(
        _entry(5), None, None, cast("CommittedStemSetTicket", Mock())
    )
    control = residency_module._CommittedSelection(
        _entry(6), None, None, cast("CommittedStemSetTicket", Mock())
    )
    attempt = residency_module._CommittedSelection(
        _entry(7), None, None, cast("CommittedStemSetTicket", Mock())
    )
    stems._committed_residency._selections[0] = selection
    stems._committed_residency._live_selections[0] = residency_module._LiveSelection(
        disk, control=control, attempt=attempt
    )
    return selection, disk.selection, control, attempt


def test_warning_captures_all_slots_without_file_work_source_leases_or_native_mutation(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _loaded(controller, audio_engine_mock)
    controller.project.stem_cache[0] = _entry()
    token = StemGenerationInterest("original queued caller")
    pending = {215: token}
    controller.stems.bind_delete_logical_interests(lambda: pending)
    before = controller.project.model_dump()
    audio_engine_mock.reset_mock()
    forbidden = Mock(side_effect=AssertionError("warning borrowed file or mutation authority"))
    with monkeypatch.context() as boundary:
        for name in ("open", "stat", "is_file", "is_dir", "iterdir", "resolve"):
            boundary.setattr(Path, name, forbidden)
        for name in ("acquire", "reserve", "sync_assignments"):
            boundary.setattr(controller.stems._assets, name, forbidden)
        inventory = controller.stems.capture_deletion_inventory()

    assert len(inventory.items) == NUM_SAMPLES == 216
    assert [item.pad_id for item in inventory.items if item is not None] == [0, 215]
    assert inventory.items[215] is not None
    assert inventory.items[215].logical_pending == frozenset({token})
    assert len(inventory.set_identities) == 1
    assert controller.project.model_dump() == before
    assert not forbidden.called
    assert [call[0] for call in audio_engine_mock.mock_calls] == [
        "waveform_source_identity",
        "waveform_source_identity",
    ]
    pending.clear()
    assert inventory.items[215].logical_pending == frozenset({token})


def test_inventory_includes_surviving_committed_pair_settlement_publication_and_promotion_rows(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    stems = rig.controller.stems
    _loaded_metadata(rig.controller, rig.audio, (0, 1))
    controls = _install_retained_controls(stems)
    publication = _publication(_entry(8), _intent(8))
    retry = _publication(_entry(9), _intent(9))
    stems._pending_stem_publications[0] = publication
    stems._publication_retries.pending[0] = StemPublicationRetry(retry, None, None)
    request = PairPreparation(
        sample_id=0,
        entry=_entry(10),
        ticket=cast("PreparedSourceTicket", Mock()),
        content_id=None,
        previous_entry=_entry(11),
        retirement=cast("AssetRetirementReservation", Mock()),
        lease=cast("ProjectAssetLease", Mock()),
        cancelled=Event(),
        generation_intent=_intent(10),
    )
    stems._disk_selection_settlements[0] = stems_module._DiskSelectionSettlement(
        request,
        request.entry,
        cast("PreparedSourceTicket", Mock()),
        cast("ProjectAssetLease", Mock()),
        None,
        cast("AssetRetirementReservation", Mock()),
    )
    subscriber = StemSubscriber(
        0,
        cast("OfflineStemSourceLease", Mock()),
        None,
        cast("AssetRetirementReservation", Mock()),
        intent=_intent(12),
    )
    job = Mock()
    job.subscribers.return_value = (subscriber,)
    promotion = stems_module._PromotedStemGeneration(
        _entry(12), cast("ProjectAssetLease", Mock(released=False))
    )
    delivery = stems_module._StemJobEventDelivery(
        cast("_StemBackendEvent", Mock()),
        cast("StemGenerationJob", job),
        (subscriber,),
        promotion=promotion,
    )
    stems._pair_held_generation_events[cast("StemGenerationJob", job), "terminal"] = delivery
    stems._jobs[Path("metadata-only-original-job")] = cast("StemGenerationJob", job)
    reserved = stems._assets._reserved
    rig.audio.reset_mock()

    item = _first(rig.controller)
    known = {identity.cache_dir for identity in item.set_identities}
    assert {
        entry.cache_dir
        for entry in (
            rig.previous,
            rig.request.entry,
            *(_entry(index) for index in range(3, 13)),
        )
    } <= known
    assert item.requests == frozenset(
        stem_delete_request_identity(_intent(sequence)) for sequence in (8, 9, 10, 12)
    )
    assert stems._assets._reserved == reserved
    assert not request.cancelled.is_set()
    assert not rig.request.cancelled.is_set()
    assert not promotion.released
    for row in controls:
        assert not cast("Mock", row.ticket).mock_calls
    assert [call[0] for call in rig.audio.mock_calls] == [
        "waveform_source_identity",
        "waveform_source_identity",
    ]


def test_pending_only_real_admission_is_removed_while_legacy_bool_remains_unchanged(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        owner = stems.request_generation_interest(0, StemGenerationInterest("pending"))
        assert owner is not None
        item = _first(controller)
        request = stems._requested_generations[0]
        assert item.requests == frozenset({stem_delete_request_identity(request.intent)})
        assert controller.project.stem_cache[0] is None

        result = stems.delete_captured_stems(item)
        assert result.state == "removed"
        assert not owner.current
        assert not stems.generation_interest_is_pending(owner)
        assert not stems._jobs
        legacy = stems.request_generation_interest(0, StemGenerationInterest("legacy pending"))
        assert legacy is not None
        assert not stems.delete_stems(0)
        assert not legacy.current
        assert not stems.generation_interest_is_pending(legacy)
        audio_engine_mock.capture_offline_stem_source.assert_not_called()
        audio_engine_mock.set_stem_mix_mode.assert_not_called()


def test_new_same_settings_admission_stales_warning_but_bound_revision_does_not(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        token = StemGenerationInterest("same diagnostic label")
        owner = stems.request_generation_interest(0, token)
        assert owner is not None
        item = _first(controller)
        requested = stems._requested_generations[0]
        bound = stems._intents.bind(requested.intent, "d" * 64)
        assert bound is not None
        stems._requested_generations[0] = replace(requested, intent=bound)
        assert _first(controller).requests == item.requests
        assert stems.cancel_generation_interest(owner, token).disposition == "detached"
        replacement = stems.request_generation_interest(0, StemGenerationInterest(token.owner))
        assert replacement is not None
        assert stems.delete_captured_stems(item).state == "skipped"
        assert replacement.current
        assert stems._requested_generations[0].owner is replacement
        audio_engine_mock.capture_offline_stem_source.assert_not_called()


def test_disappeared_original_request_and_unrelated_settings_change_allow_truthful_noop(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        token = StemGenerationInterest("already ended")
        owner = stems.request_generation_interest(0, token)
        assert owner is not None
        item = _first(controller)
        stems.cancel_generation_interest(owner, token)
        before_revision = controller.project.stem_processing_revision
        controller.project.demucs_shifts += 1
        assert controller.project.stem_processing_revision > before_revision
        assert stems.delete_captured_stems(item).state == "noop"
        assert not stems._jobs
        audio_engine_mock.capture_offline_stem_source.assert_not_called()


@pytest.mark.parametrize("changed", ["source", "content", "set", "logical_token"])
def test_new_original_identity_is_skipped_before_retirement_or_native_mutation(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    changed: str,
) -> None:
    _loaded(controller, audio_engine_mock)
    controller.project.stem_cache[0] = _entry()
    original = StemGenerationInterest("batch-pad-1")
    logical = {0: original}
    controller.stems.bind_delete_logical_interests(lambda: logical)
    item = _first(controller)
    if changed == "source":
        audio_engine_mock.waveform_source_identity.side_effect = None
        audio_engine_mock.waveform_source_identity.return_value = (8, *_SOURCE[1:])
    elif changed == "content":
        controller.project.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
    elif changed == "set":
        controller.project.stem_cache[0] = _entry(2)
    else:
        logical[0] = StemGenerationInterest(original.owner)
    before = controller.project.model_dump()
    forbidden = Mock(side_effect=AssertionError("stale warning reached mutation admission"))
    monkeypatch.setattr(controller.stems._assets, "admission", forbidden)

    assert controller.stems.delete_captured_stems(item).state == "skipped"
    assert controller.project.model_dump() == before
    forbidden.assert_not_called()
    audio_engine_mock.set_stem_mix_mode.assert_not_called()


def test_arrangement_fence_defers_before_source_provider_or_owner_observation(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _loaded(controller, audio_engine_mock)
    item = _first(controller)
    forbidden = Mock(side_effect=AssertionError("fenced intermediate image was read"))
    controller.stems.bind_delete_logical_interests(forbidden)
    audio_engine_mock.waveform_source_identity.side_effect = forbidden
    controller.stems._pair_deferred_sample_ids = frozenset({0})
    assert controller.stems.delete_captured_stems(item).state == "deferred"
    with pytest.raises(RuntimeError, match="PAD arrangement"):
        controller.stems.capture_deletion_inventory()
    forbidden.assert_not_called()


@pytest.mark.parametrize("boundary", ["native", "retirement"])
def test_full_mix_or_retirement_admission_failure_preserves_original_selection_and_pending_pair(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch, boundary: str
) -> None:
    rig = disk_commit
    _loaded_metadata(rig.controller, rig.audio, (0, 1))
    item = _first(rig.controller)
    before = rig.controller.project.model_dump()
    if boundary == "native":
        rig.audio.set_stem_pair_full_mix.side_effect = RuntimeError("native admission rejected")
    else:
        monkeypatch.setattr(
            rig.controller.stems._assets,
            "reserve",
            Mock(side_effect=RuntimeError("retirement backlog full")),
        )
    result = rig.controller.stems.delete_captured_stems(item)
    assert result.state == "failed"
    assert result.detail is not None
    assert rig.controller.stems.stem_generation_error(0) == result.detail
    assert rig.controller.project.model_dump() == before
    assert not rig.request.cancelled.is_set()
    assert not rig.request.lease.released
    assert Path(rig.previous.cache_dir).is_dir()


def test_success_reuses_shared_retirement_and_retains_actual_cancelled_reader(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    _loaded_metadata(rig.controller, rig.audio, (0, 1))
    item = _first(rig.controller)
    independent = rig.controller.project.stem_cache[1]
    selected = rig.controller.project.selected_pad, rig.controller.project.selected_bank
    result = rig.controller.stems.delete_captured_stems(item)
    assert result.state == "removed"
    assert rig.controller.project.stem_cache[0] is None
    assert rig.controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert rig.controller.project.stem_cache[1] is independent
    assert rig.request.cancelled.is_set()
    assert not rig.request.lease.released
    assert Path(rig.previous.cache_dir).is_dir()
    assert Path(rig.controller.project.sample_paths[0] or "").is_file()
    assert (rig.controller.project.selected_pad, rig.controller.project.selected_bank) == selected
    rig.audio.set_stem_pair_full_mix.assert_called_once_with(0)
    fresh = _first(rig.controller)
    assert rig.controller.stems.delete_captured_stems(fresh).state == "noop"
    rig.audio.set_stem_pair_full_mix.assert_called_once_with(0)


def test_pending_full_mix_release_is_removed_then_fresh_repeat_is_noop(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _loaded(controller, audio_engine_mock)
    controller.stems._pending_full_mix_releases.add(0)
    assert controller.stems.delete_captured_stems(_first(controller)).state == "removed"
    assert 0 not in controller.stems._pending_full_mix_releases
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "full_mix")
    assert controller.stems.delete_captured_stems(_first(controller)).state == "noop"
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "full_mix")


def test_logical_only_noop_checks_original_and_edit_guard_without_retirement_scan(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _loaded(controller, audio_engine_mock)
    item = _first(controller)
    guard = Mock()
    controller.stems.bind_pad_edit_guard(guard)
    forbidden = Mock(side_effect=AssertionError("empty original entered retirement admission"))
    monkeypatch.setattr(controller.stems._assets, "reserve", forbidden)
    monkeypatch.setattr(controller.stems._assets, "sync_assignments", forbidden)

    assert controller.stems.delete_captured_stems(item).state == "noop"

    guard.assert_called_once_with(0)
    forbidden.assert_not_called()
    audio_engine_mock.set_stem_mix_mode.assert_not_called()
    audio_engine_mock.retire_project_asset.assert_not_called()


def test_saved_assignment_after_model_clear_still_reaches_shared_retirement(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _loaded(controller, audio_engine_mock)
    assets = controller.stems._assets
    path = assets._root / "samples/stems/#1" / (".ready-" + "1" * 32)
    path.mkdir(parents=True)
    owner = assets.acquire(path)
    reader = assets.acquire(path)
    assets.adopt_stems(0, (path, owner))
    assert controller.project.stem_cache[0] is None
    synchronize = Mock(wraps=assets.sync_assignments)
    monkeypatch.setattr(assets, "sync_assignments", synchronize)

    assert controller.stems.delete_captured_stems(_first(controller)).state == "noop"

    assert synchronize.call_count == 2
    assert ("stems", 0) not in assets._assignments
    assert owner.released
    assert not reader.released
    audio_engine_mock.retire_project_asset.assert_called_once_with(str(path), recursive=True)
    assert controller.stems.delete_captured_stems(_first(controller)).state == "noop"
    assert synchronize.call_count == 2
    reader.release()


@pytest.mark.parametrize("retained", ["resident_pair", "admission_metadata"])
def test_retained_control_work_never_uses_logical_only_noop(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    retained: str,
) -> None:
    _loaded(controller, audio_engine_mock)
    if retained == "resident_pair":
        controller.stems._resident_pairs.add(0)
    else:
        controller.stems._generation_intents[0] = _intent(1)
    item = _first(controller)
    synchronize = Mock(wraps=controller.stems._assets.sync_assignments)
    monkeypatch.setattr(controller.stems._assets, "sync_assignments", synchronize)

    controller.stems.delete_captured_stems(item)

    assert synchronize.call_count == 2
    assert 0 not in controller.stems._resident_pairs
    assert 0 not in controller.stems._generation_intents
    assert controller.stems.delete_captured_stems(_first(controller)).state == "noop"
    assert synchronize.call_count == 2
