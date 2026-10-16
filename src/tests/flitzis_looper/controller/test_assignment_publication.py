"""Owning Python assignment publication; fake leases do not certify Native file custody."""

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller import asset_lifecycle
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.asset_lifecycle import (
        AssignmentImage,
        AssignmentPublicationPlan,
        _AssignmentRetirement,
    )
    from flitzis_looper_audio import AudioEngine, ProjectAssetLease


@dataclass
class _Lease:
    path: str
    release_count: int = 0

    def release(self) -> None:
        if not self.release_count:
            self.release_count = 1

    @property
    def released(self) -> bool:
        return self.release_count != 0


@dataclass
class _Audio:
    leases: list[_Lease] = field(default_factory=list)
    admitted: list[tuple[str, bool]] = field(default_factory=list)
    pressure: bool = False

    def acquire_project_asset_lease(self, path: str) -> ProjectAssetLease:
        lease = _Lease(path)
        self.leases.append(lease)
        return cast("ProjectAssetLease", lease)

    def retire_project_asset(self, path: str, *, recursive: bool = False) -> None:
        if self.pressure:
            message = "project asset retirement queue full"
            raise RuntimeError(message)
        self.admitted.append((path, recursive))


def _assets(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[ProjectAssetLifecycle, _Audio]:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "samples").mkdir()
    audio = _Audio()
    assets = ProjectAssetLifecycle(ProjectState(), cast("AudioEngine", audio))
    monkeypatch.setattr(assets, "_start_retry_worker_locked", lambda: None)
    return assets, audio


def _original(
    assets: ProjectAssetLifecycle, sample_id: int, name: str
) -> tuple[Path, ProjectAssetLease]:
    path = assets._root / "samples" / name
    path.write_bytes(b"fixture source")
    assets._project.sample_paths[sample_id] = f"samples/{name}"
    prepared = (path, assets.acquire(path))
    assets._assignments["original", sample_id] = prepared
    return prepared


def _replacement_image(assets: ProjectAssetLifecycle, *sample_ids: int) -> AssignmentImage:
    candidate = assets._project.model_copy(deep=True)
    for sample_id in sample_ids:
        name = f"new-{sample_id}.wav"
        (assets._root / "samples" / name).write_bytes(b"new fixture source")
        candidate.sample_paths[sample_id] = f"samples/{name}"
    return assets.prepare_assignment_image(candidate)


def test_complete_store_interrupt_is_recognized_and_newest_unrelated_table_survives(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    assets, audio = _assets(tmp_path, monkeypatch)
    original = _original(assets, 0, "old.wav")
    image = _replacement_image(assets, 0)
    plan = assets.prepare_assignment_publication(image)
    store = asset_lifecycle._store_assignment_table

    def interrupt_after_store(
        owner: ProjectAssetLifecycle,
        table: dict[tuple[str, int], tuple[Path, ProjectAssetLease]],
        *,
        publication: AssignmentPublicationPlan,
    ) -> None:
        store(owner, table, publication=publication)
        message = "interrupted after complete own store"
        raise RuntimeError(message)

    monkeypatch.setattr(asset_lifecycle, "_store_assignment_table", interrupt_after_store)
    with pytest.raises(RuntimeError, match="complete own store"):
        plan.apply()
    assert plan.applied
    assert not plan.settled
    assert not plan.owns_unpublished_image
    assert assets._assignments["original", 0] is image[1]["original", 0]
    unrelated = _original(assets, 36, "unrelated.wav")
    latest = assets._assignments.copy()
    assets._assignments = latest
    reservation = assets.reserve()
    with reservation.activate():
        plan.apply()
    reservation.close()
    assert plan.settled
    assert assets._assignments is latest
    assert assets._assignments["original", 36] is unrelated
    assert cast("_Lease", unrelated[1]).release_count == 0
    assert cast("_Lease", original[1]).release_count == 1
    assert audio.admitted == [(str(original[0]), False)]
    plan.apply()
    assert assets._assignments is latest
    assert audio.admitted == [(str(original[0]), False)]


@pytest.mark.parametrize("after_transfer", [False, True])
def test_retirement_failure_keeps_original_tuple_cursor_and_later_occupant(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, after_transfer: bool
) -> None:
    assets, audio = _assets(tmp_path, monkeypatch)
    old_zero = _original(assets, 0, "old-zero.wav")
    old_one = _original(assets, 1, "old-one.wav")
    image = _replacement_image(assets, 0, 1)
    plan = assets.prepare_assignment_publication(image)
    retire = assets._retire_assignment
    interrupted = False

    def fail_once(
        kind: str,
        path: Path,
        lease: ProjectAssetLease,
        *,
        progress: _AssignmentRetirement | None = None,
    ) -> None:
        nonlocal interrupted
        if lease is old_one[1] and not interrupted:
            interrupted = True
            if after_transfer:
                retire(kind, path, lease, progress=progress)
            message = "retirement interrupted"
            raise RuntimeError(message)
        retire(kind, path, lease, progress=progress)

    monkeypatch.setattr(assets, "_retire_assignment", fail_once)
    reservation = assets.reserve()
    with reservation.activate(), pytest.raises(RuntimeError, match="retirement interrupted"):
        plan.apply()
    assert plan.applied
    assert not plan.settled
    assert not plan.owns_unpublished_image
    assert plan._retirement_index == 1
    assert plan._retired[1].lease is old_one[1]
    later = _original(assets, 1, "later-occupant.wav")
    latest = assets._assignments.copy()
    assets._assignments = latest
    with reservation.activate():
        plan.apply()
    reservation.close()
    assert plan.settled
    assert assets._assignments is latest
    assert assets._assignments["original", 1] is later
    assert cast("_Lease", later[1]).release_count == 0
    assert cast("_Lease", old_zero[1]).release_count == 1
    assert cast("_Lease", old_one[1]).release_count == 1
    assert audio.admitted.count((str(old_zero[0]), False)) == 1
    assert audio.admitted.count((str(old_one[0]), False)) == 1


def test_unknown_before_store_replacement_rejects_without_releasing_any_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    assets, audio = _assets(tmp_path, monkeypatch)
    original = _original(assets, 0, "old.wav")
    image = _replacement_image(assets, 0)
    plan = assets.prepare_assignment_publication(image)
    unknown = (original[0], assets.acquire(original[0]))
    latest = {("original", 0): unknown}
    assets._assignments = latest
    with pytest.raises(RuntimeError, match="ownership changed before"):
        plan.apply()
    assert not plan.applied
    assert not plan.settled
    assert plan.owns_unpublished_image
    assert assets._assignments is latest
    assert all(lease.release_count == 0 for lease in audio.leases)
    assert audio.admitted == []
    assets.release_assignment_image(image)
    assert cast("_Lease", image[1]["original", 0][1]).release_count == 1
    assert cast("_Lease", unknown[1]).release_count == 0
    assert cast("_Lease", original[1]).release_count == 0


def test_latest_shared_path_releases_only_old_lease_without_retiring_saved_bytes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    assets, audio = _assets(tmp_path, monkeypatch)
    original = _original(assets, 0, "shared.wav")
    image = _replacement_image(assets, 0)
    plan = assets.prepare_assignment_publication(image)
    store = asset_lifecycle._store_assignment_table

    def interrupt_after_store(
        owner: ProjectAssetLifecycle,
        table: dict[tuple[str, int], tuple[Path, ProjectAssetLease]],
        *,
        publication: AssignmentPublicationPlan,
    ) -> None:
        store(owner, table, publication=publication)
        message = "complete store interrupted"
        raise RuntimeError(message)

    monkeypatch.setattr(asset_lifecycle, "_store_assignment_table", interrupt_after_store)
    with pytest.raises(RuntimeError, match="store interrupted"):
        plan.apply()
    later_shared = (original[0], assets.acquire(original[0]))
    assets._assignments["original", 36] = later_shared
    plan.apply()
    assert plan.settled
    assert cast("_Lease", original[1]).release_count == 1
    assert cast("_Lease", later_shared[1]).release_count == 0
    assert audio.admitted == []


def test_native_queue_pressure_transfers_exact_retired_lease_to_existing_retry_lane(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    assets, audio = _assets(tmp_path, monkeypatch)
    original = _original(assets, 0, "old.wav")
    image = _replacement_image(assets, 0)
    plan = assets.prepare_assignment_publication(image)
    audio.pressure = True
    reservation = assets.reserve()
    with reservation.activate():
        plan.apply()
    reservation.close()
    assert plan.settled
    assert assets._pending[original[0], False][0] is original[1]
    assert cast("_Lease", original[1]).release_count == 0
    plan.apply()
    assert len(assets._pending[original[0], False]) == 1
    audio.pressure = False
    assets.retry_retirements()
    assert cast("_Lease", original[1]).release_count == 1
    assert audio.admitted == [(str(original[0]), False)]


def test_partial_legacy_capacity_failure_retries_only_remaining_declared_files(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    assets, audio = _assets(tmp_path, monkeypatch)
    legacy = tmp_path / "samples" / "stems" / "#1"
    legacy.mkdir(parents=True)
    for name in assets._LEGACY_STEM_FILES:
        (legacy / name).write_bytes(b"legacy fixture")
    original = (legacy, assets.acquire(legacy))
    assets._assignments["stems", 0] = original
    plan = assets.prepare_assignment_publication(({}, {}))
    monkeypatch.setattr(assets, "_MAX_PENDING_RETIREMENTS", 3)
    reservation = assets.reserve(3)
    audio.pressure = True
    with reservation.activate(), pytest.raises(RuntimeError, match="no reserved retirement slot"):
        plan.apply()
    reservation.close()
    assert plan.applied
    assert not plan.settled
    assert plan._retired[0].legacy_index == 3
    assert cast("_Lease", original[1]).release_count == 0
    audio.pressure = False
    assets.retry_retirements()
    monkeypatch.setattr(assets, "_MAX_PENDING_RETIREMENTS", 16)
    reservation = assets.reserve(8)
    with reservation.activate():
        plan.apply()
    reservation.close()
    assert plan.settled
    assert plan._retired[0].legacy_index == len(assets._LEGACY_STEM_FILES)
    assert cast("_Lease", original[1]).release_count == 1
    assert len(audio.admitted) == len(assets._LEGACY_STEM_FILES)
    assert len(set(audio.admitted)) == len(audio.admitted)
