"""Post-Copy control retirement; Native CopyACK and disk integrity are separate suites."""

from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import committed_stem_residency, stems
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.stem_pair_preparation import PairPreparation
from flitzis_looper.models import PadContentIdentity, StemCacheEntry
from tests.flitzis_looper.conftest import FakePreparedSourceTicket
from tests.flitzis_looper.controller.test_pair_stem_observers import _job, _rig, _subscribe
from tests.flitzis_looper.controller.test_stem_pair_preparation import _PreparedPair

if TYPE_CHECKING:
    from flitzis_looper.controller.asset_lifecycle import AssetRetirementReservation
    from flitzis_looper.controller.stems import StemController
    from flitzis_looper_audio import (
        CommittedStemSetTicket,
        PreparedSourceTicket,
        PreparedStemPair,
        ProjectAssetLease,
        ResidentStemTicket,
    )
    from tests.flitzis_looper.controller.test_stem_job import _Lease, _Reservation


def _entry() -> StemCacheEntry:
    root = f"samples/stems/#1/.ready-{'a' * 32}"
    return StemCacheEntry(
        source_version=f"samples/shared.wav|sha256-v1:{'1' * 64}",
        cache_dir=root,
        stems=expected_stem_files(root),
        available=True,
    )


def _retire(controller: StemController) -> None:
    controller.retire_overwritten_native_state(0, original_content_id="a" * 32)


def test_overwrite_detaches_only_removed_subscriber_and_cancels_exact_native_tickets(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    controller = rig.controller
    job = _job(rig)
    _subscribe(rig, job, 36)
    removed, survivor = job.subscribers()
    source = job.begin()
    assert source is not None
    entry = _entry()
    controller._project.stem_cache[0] = entry
    controller._project.pad_stem_mix_mode[0] = "all_stems"
    registration = Mock(spec=["cancel"])
    receipt = Mock(spec=["cancel"])
    preload = Mock(spec=["cancel"])
    selection = committed_stem_residency._CommittedSelection(
        entry,
        "a" * 32,
        "samples/shared.wav",
        cast("CommittedStemSetTicket", registration),
        receipt=cast("ResidentStemTicket", receipt),
        preload_receipt=cast("ResidentStemTicket", preload),
    )
    independent = Mock(name="unrelated committed owner")
    controller._committed_residency._selections[0] = selection
    controller._committed_residency._selections[36] = independent
    controller._demands[0] = stems._StemDemand(entry.source_version, "a" * 32, None, None)
    controller._demands[36] = stems._StemDemand(entry.source_version, "c" * 32, None, None)
    controller._resident_pairs.update((0, 36))
    controller._pending_full_mix_releases.add(0)
    controller._residency_errors[0] = "old target error"
    controller._pair_deferred_sample_ids = frozenset((0,))
    rig.audio.reset_mock()

    _retire(controller)
    _retire(controller)

    assert job.subscribers() == (survivor,)
    assert job.subscribers()[0] is survivor
    assert job.source is source
    assert job.running
    assert not job.cancel_event.is_set()
    assert cast("_Lease", removed.source_lease).released
    assert cast("_Reservation", removed.retirement).closed
    assert not cast("_Lease", survivor.source_lease).released
    assert not cast("_Reservation", survivor.retirement).closed
    assert controller._generation_sources[36] is survivor.source
    assert controller._session.stem_generating_sample_ids == {36}
    assert controller._committed_residency._selections == {36: independent}
    assert set(controller._demands) == {36}
    assert controller._resident_pairs == {36}
    assert not controller._pending_full_mix_releases
    assert not controller._residency_errors
    assert controller._project.stem_cache[0] is entry
    assert controller._project.pad_stem_mix_mode[0] == "all_stems"
    registration.cancel.assert_called_once_with()
    receipt.cancel.assert_called_once_with()
    preload.cancel.assert_called_once_with()
    assert rig.audio.method_calls == []
    job.finish_read()
    job.cancel()


def test_overwrite_keeps_cancelled_preparation_reader_until_actual_return(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    controller = rig.controller
    entry = _entry()
    controller._project.stem_cache[0] = entry
    prepared = _PreparedPair()
    lease = rig.assets.acquire(Path(entry.cache_dir))
    reservation = rig.assets.reserve(16)
    request = PairPreparation(
        0,
        entry,
        cast("PreparedSourceTicket", FakePreparedSourceTicket()),
        "a" * 32,
        entry,
        cast("AssetRetirementReservation", reservation),
        cast("ProjectAssetLease", lease),
        Event(),
        prepared=cast("PreparedStemPair", prepared),
        source_path="samples/shared.wav",
    )
    controller._pair_preparations.pending[0] = request
    controller._pair_deferred_sample_ids = frozenset((0,))
    _retire(controller)
    assert request.cancelled.is_set()
    assert controller._pair_preparations.pending[0] is request
    assert not lease.released
    assert not reservation.closed
    controller._pair_preparations._results.put(request)
    controller._poll_pair_preparations()
    assert controller._pair_preparations.pending[0] is request
    assert not lease.released
    assert not prepared.discarded
    controller._project.pad_content[0] = PadContentIdentity(instance_id="e" * 32)
    controller._pair_deferred_sample_ids = frozenset()
    rig.audio.reset_mock()
    controller._poll_pair_preparations()
    assert not controller._pair_preparations.pending
    assert lease.released
    assert reservation.closed
    assert prepared.discarded
    assert controller._project.stem_cache[0] is entry
    assert rig.audio.method_calls == []


def test_overwrite_releases_pending_publication_and_keeps_disk_settlement_until_superseded_write(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    controller = rig.controller
    entry = _entry()
    controller._project.stem_cache[0] = entry
    pending_pair = _PreparedPair()
    pending_lease = rig.assets.acquire(Path("previous-stems"))
    pending_reservation = rig.assets.reserve(16)
    pending = stems._PendingStemPublication(
        0,
        cast("PreparedSourceTicket", FakePreparedSourceTicket()),
        entry,
        None,
        cast("ProjectAssetLease", pending_lease),
        cast("AssetRetirementReservation", pending_reservation),
        prepared_pair=cast("PreparedStemPair", pending_pair),
        content_id="a" * 32,
    )
    controller._pending_stem_publications[0] = pending
    controller._publication_retries.retain(pending, "a" * 32, "samples/shared.wav")
    selected_pair = _PreparedPair(components=False)
    request = PairPreparation(
        0,
        entry,
        pending.source_ticket,
        "a" * 32,
        None,
        cast("AssetRetirementReservation", rig.assets.reserve(16)),
        cast("ProjectAssetLease", rig.assets.acquire(Path(entry.cache_dir))),
        Event(),
        prepared=cast("PreparedStemPair", selected_pair),
        source_path="samples/shared.wav",
    )
    selected_lease = rig.assets.acquire(Path(entry.cache_dir))
    selected_reservation = rig.assets.reserve(16)
    settlement = stems._DiskSelectionSettlement(
        request,
        entry,
        pending.source_ticket,
        cast("ProjectAssetLease", selected_lease),
        None,
        cast("AssetRetirementReservation", selected_reservation),
    )
    controller._disk_selection_settlements[0] = settlement
    flush = Mock()
    controller._flush_superseded_disk_selection = flush
    controller._pair_deferred_sample_ids = frozenset((0,))
    _retire(controller)
    assert controller._project.stem_cache[0] is entry
    assert not controller._pending_stem_publications
    assert not controller._publication_retries.pending
    assert pending_pair.discarded
    assert pending_lease.released
    assert pending_reservation.closed
    assert settlement.content_retired
    assert request.cancelled.is_set()
    assert controller._disk_selection_settlements[0] is settlement
    assert not selected_lease.released
    assert not selected_reservation.closed
    controller._poll_disk_selection_settlements()
    flush.assert_not_called()
    successor = entry.model_copy(deep=True)
    controller._project.stem_cache[0] = successor
    controller._project.pad_content[0] = PadContentIdentity(instance_id="e" * 32)
    controller._pair_deferred_sample_ids = frozenset()
    rig.audio.reset_mock()
    controller._poll_disk_selection_settlements()
    flush.assert_called_once_with()
    assert selected_pair.selected
    assert not selected_pair.discarded
    assert selected_lease.released
    assert selected_reservation.closed
    assert not controller._disk_selection_settlements
    assert controller._project.stem_cache[0] is successor
    assert rig.audio.method_calls == []
    request.close()


@pytest.mark.parametrize("guard", ["unfenced", "replacement"])
def test_overwrite_retirement_rejects_without_original_fenced_project_owner(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    guard: str,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    job = _job(rig)
    original = job.subscribers()[0]
    rig.controller._pair_deferred_sample_ids = (
        frozenset() if guard == "unfenced" else frozenset((0,))
    )
    if guard == "replacement":
        rig.controller._project.pad_content[0] = PadContentIdentity(instance_id="e" * 32)
    with pytest.raises(RuntimeError, match="fenced original content"):
        _retire(rig.controller)
    assert job.subscribers() == (original,)
    assert not cast("_Lease", original.source_lease).released
    assert rig.controller._generation_sources[0] is original.source
    job.cancel()
