"""Real config replacement and control custody; Native pair verification is separate."""

import os
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.models import STEM_KINDS, PadContentIdentity, StemCacheEntry
from flitzis_looper.stem_pair_selection import StemPairSelection
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.controller.test_asset_lifecycle import _install_eager_registry
from tests.flitzis_looper.controller.test_stem_pair_preparation import _queued, _selection

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_pair_preparation import PairPreparation
    from flitzis_looper.models import ProjectState
    from flitzis_looper_audio import ProjectAssetLease
    from tests.flitzis_looper.conftest import FakeProjectAssetLease
    from tests.flitzis_looper.controller.test_stem_pair_preparation import _PreparedPair


@dataclass(slots=True)
class _DiskCommit:
    controller: AppController
    audio: Mock
    jobs: list[Callable[[], None]]
    prepared: _PreparedPair
    request: PairPreparation
    previous: StemCacheEntry
    independent: StemCacheEntry
    owners: list[FakeProjectAssetLease]
    encoded: bytes
    revision: int


def _write_pair_fixture(pair: StemPairSelection) -> None:
    """Give the eager control registry real files, without claiming Native integrity."""
    wav = Path(pair.wav_generation)
    wav.mkdir(parents=True, exist_ok=True)
    for name in STEM_KINDS:
        write_mono_pcm16_wav(wav / f"{name}.wav", 44_100)
    (wav / ".complete.json").write_text("{}", encoding="utf-8")
    descriptor = Path(pair.descriptor_reference)
    descriptor.parent.mkdir(parents=True, exist_ok=True)
    descriptor.write_text("{}", encoding="utf-8")
    Path(pair.pcm_generation).mkdir(parents=True, exist_ok=True)


def _record_eager_owners(audio: Mock) -> list[FakeProjectAssetLease]:
    _install_eager_registry(audio)
    acquire = cast(
        "Callable[[str], FakeProjectAssetLease]", audio.acquire_project_asset_lease.side_effect
    )
    owners: list[FakeProjectAssetLease] = []

    def recorded(path: str) -> FakeProjectAssetLease:
        owner = acquire(path)
        owners.append(owner)
        return owner

    audio.acquire_project_asset_lease.side_effect = recorded
    return owners


def _previous_pair(prepared: _PreparedPair, source_version: str) -> StemCacheEntry:
    selection = prepared.selection.copy()
    root = str(selection["wav_generation"]).rsplit("/stems/", 1)[0]
    selection.update(
        descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{'1' * 32}.json",
        stem_set_identity="2" * 64,
        wav_generation=f"{root}/stems/.ready-{'3' * 32}",
        pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{'4' * 32}",
    )
    pair = StemPairSelection.model_validate(selection)
    _write_pair_fixture(pair)
    return StemCacheEntry(
        source_version=source_version,
        cache_dir=pair.wav_generation,
        stems=expected_stem_files(pair.wav_generation),
        available=True,
        pair=pair,
    )


@pytest.fixture
def disk_commit(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> _DiskCommit:
    material = str(_selection()["wav_generation"]).rsplit("/stems/", 1)[0]
    source_relative = f"{material}/original/source.wav"
    source = tmp_path / source_relative
    source.parent.mkdir(parents=True, exist_ok=True)
    write_mono_pcm16_wav(source, 44_100)
    controller.project.sample_paths[0] = source_relative
    controller.project.sample_paths[1] = source_relative
    audio_engine_mock.load_sample_async(0, source_relative)
    audio_engine_mock.load_sample_async(1, source_relative)
    digest = audio_engine_mock.loaded_source_digest(0)
    source_version = f"{source_relative}|sha256-v1:{digest}"
    owners = _record_eager_owners(audio_engine_mock)
    jobs, prepared, _, candidate = _queued(
        controller, audio_engine_mock, monkeypatch, tmp_path, components=False
    )
    candidate.source_version = source_version
    request = controller.stems._pair_preparations.pending[0]
    request.own_source_version = source_version
    request.artifact_source_version = source_version
    previous = _previous_pair(prepared, source_version)
    independent = previous.model_copy(deep=True)
    controller.project.stem_cache[0] = previous
    controller.project.stem_cache[1] = independent
    request.previous_entry = previous
    _write_pair_fixture(StemPairSelection.model_validate(prepared.selection))
    controller._assets.sync_assignments()
    controller._persistence.flush(now=0.0)
    return _DiskCommit(
        controller,
        audio_engine_mock,
        jobs,
        prepared,
        request,
        previous,
        independent,
        owners,
        controller._persistence.config_path.read_bytes(),
        controller._persistence.revision,
    )


def _assert_independent_copy(rig: _DiskCommit) -> None:
    assert rig.controller.project.stem_cache[1] is rig.independent
    owner = cast("FakeProjectAssetLease", rig.controller._assets._assignments["stems", 1][1])
    assert not owner.released
    assert Path(rig.independent.cache_dir).is_dir()
    assert all(Path(value).is_file() for value in rig.independent.stems.model_dump().values())


def _assert_reopened(rig: _DiskCommit, expected: StemCacheEntry) -> None:
    reopened = ProjectPersistence.from_config_path(rig.controller._persistence.config_path)
    assert reopened.load_error is None
    assert not reopened.invalid_stem_pair_ids
    assert reopened.project.stem_cache[0] == expected
    assert reopened.project.stem_cache[1] == rig.independent
    assert expected.pair is not None
    assert expected.available


def _assert_precommit_retained(rig: _DiskCommit) -> None:
    controller = rig.controller
    assert controller.project.stem_cache[0] is rig.previous
    assert controller._persistence.config_path.read_bytes() == rig.encoded
    assert controller._persistence.revision == rig.revision
    assert not controller.stems._disk_selection_settlements
    assert not controller.stems._pair_preparations.pending
    assert controller._assets._reserved == 0
    assert rig.prepared.discarded
    assert not rig.prepared.selected
    assigned = {id(value[1]) for value in controller._assets._assignments.values()}
    assert all(owner.released for owner in rig.owners if id(owner) not in assigned)
    assert all(not owner.released for owner in rig.owners if id(owner) in assigned)
    assert not tuple(controller._persistence.config_path.parent.glob(".*.tmp"))
    _assert_independent_copy(rig)
    _assert_reopened(rig, rig.previous)


@pytest.mark.parametrize("boundary", ["new", "previous"])
def test_disk_owner_admission_failure_preserves_old_config_and_independent_copy(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch, boundary: str
) -> None:
    rig = disk_commit
    acquire = rig.controller._assets.acquire
    rejected = (
        str(rig.prepared.selection["wav_generation"])
        if boundary == "new"
        else rig.previous.cache_dir
    )

    def fail_owner(path: Path) -> ProjectAssetLease:
        if path == Path(rejected):
            message = f"injected {boundary} owner admission failure"
            raise RuntimeError(message)
        return acquire(path)

    monkeypatch.setattr(rig.controller._assets, "acquire", fail_owner)
    rig.jobs[0]()
    rig.controller.stems._poll_pair_preparations()
    assert f"{boundary} owner admission failure" in rig.controller.session.stem_generation_errors[0]
    rig.audio.retire_project_asset.assert_not_called()
    _assert_precommit_retained(rig)


@pytest.mark.parametrize("boundary", ["fsync", "replace"])
def test_actual_candidate_config_io_failure_preserves_old_live_and_durable_pair(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch, boundary: str
) -> None:
    rig = disk_commit
    fault = Mock(side_effect=OSError(f"injected candidate {boundary} failure"))
    monkeypatch.setattr(os, boundary, fault)
    rig.jobs[0]()
    rig.controller.stems._poll_pair_preparations()
    fault.assert_called_once()
    assert f"candidate {boundary} failure" in rig.controller.session.stem_generation_errors[0]
    _assert_precommit_retained(rig)


def test_cancel_after_actual_fsync_rejects_pair_before_actual_config_replace(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch
) -> None:
    rig = disk_commit
    fsync = os.fsync
    replace = Mock(side_effect=os.replace)

    def cancel_after_fsync(descriptor: int) -> None:
        fsync(descriptor)
        rig.request.cancelled.set()

    monkeypatch.setattr(os, "fsync", cancel_after_fsync)
    monkeypatch.setattr(os, "replace", replace)
    rig.jobs[0]()
    rig.controller.stems._poll_pair_preparations()
    replace.assert_not_called()
    assert rig.request.cancelled.is_set()
    _assert_precommit_retained(rig)


def test_cancel_after_actual_replace_commits_once_without_false_rollback(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch
) -> None:
    rig = disk_commit
    replace = os.replace

    def cancel_after_replace(source: str | Path, destination: str | Path) -> None:
        replace(source, destination)
        rig.request.cancelled.set()

    replacement = Mock(side_effect=cancel_after_replace)
    monkeypatch.setattr(os, "replace", replacement)
    rig.jobs[0]()
    rig.controller.stems._poll_pair_preparations()
    selected = rig.controller.project.stem_cache[0]
    assert selected is not None
    assert selected.pair == StemPairSelection.model_validate(rig.prepared.selection)
    assert selected.available
    assert rig.prepared.selected
    assert not rig.prepared.discarded
    assert not rig.controller.stems._disk_selection_settlements
    assert rig.controller._assets._reserved == 0
    assert rig.controller._persistence.revision == rig.revision + 1
    assert 0 not in rig.controller.session.stem_generation_errors
    rig.controller.stems._poll_disk_selection_settlements()
    rig.controller.stems._poll_pair_preparations()
    replacement.assert_called_once()
    _assert_independent_copy(rig)
    _assert_reopened(rig, selected)


def _defer_owner_settlement(
    rig: _DiskCommit, monkeypatch: pytest.MonkeyPatch, boundary: str
) -> Callable[[], None]:
    target: object
    if boundary == "select":
        target = rig.prepared
        name = "select"
    else:
        target = rig.controller._assets
        name = "adopt_stems" if boundary == "adopt" else "_retire_assignment"
    original = getattr(target, name)
    monkeypatch.setattr(
        target, name, Mock(side_effect=RuntimeError(f"injected postcommit {boundary} failure"))
    )

    def restore() -> None:
        monkeypatch.setattr(target, name, original)

    return restore


@pytest.mark.parametrize("boundary", ["select", "adopt", "retire"])
def test_postcommit_fault_retains_exact_owners_and_reservation_until_bounded_retry(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch, boundary: str
) -> None:
    rig = disk_commit
    restore = _defer_owner_settlement(rig, monkeypatch, boundary)
    replacement = Mock(side_effect=os.replace)
    monkeypatch.setattr(os, "replace", replacement)
    rig.jobs[0]()
    rig.controller.stems._poll_pair_preparations()
    settlement = rig.controller.stems._disk_selection_settlements[0]
    assert settlement.request is rig.request
    assert rig.controller.project.stem_cache[0] is settlement.entry
    assert not cast("FakeProjectAssetLease", settlement.lease).released
    assert settlement.previous_lease is not None
    assert not cast("FakeProjectAssetLease", settlement.previous_lease).released
    assert settlement.retirement.remaining > 0
    assert rig.controller._assets._reserved == settlement.retirement.remaining
    assert not rig.prepared.discarded
    assert "selection committed; owner settlement deferred" in (
        rig.controller.session.stem_generation_errors[0].lower()
    )
    committed = rig.controller._persistence.config_path.read_bytes()
    _assert_reopened(rig, settlement.entry)
    assert not rig.controller.stems.generate_stems_async(0)
    assert "another pad task" in rig.controller.session.stem_generation_errors[0]
    assert not rig.controller.stems._jobs
    assert not cast("FakeProjectAssetLease", settlement.lease).released
    assert not cast("FakeProjectAssetLease", settlement.previous_lease).released
    _assert_independent_copy(rig)

    restore()
    rig.controller.stems._poll_disk_selection_settlements()
    assert not rig.controller.stems._disk_selection_settlements
    assert rig.prepared.selected
    assert not rig.prepared.discarded
    assert rig.controller._assets._reserved == 0
    assert settlement.previous_lease is None
    assignment = rig.controller._assets._assignments["stems", 0]
    assert assignment[0] == Path(settlement.entry.cache_dir).absolute()
    assert not cast("FakeProjectAssetLease", assignment[1]).released
    assert rig.controller._persistence.config_path.read_bytes() == committed
    replacement.assert_called_once()
    _assert_independent_copy(rig)
    _assert_reopened(rig, settlement.entry)


@pytest.mark.parametrize("change", ["content", "unload"])
def test_committed_retry_retires_stale_pair_without_reviving_changed_pad(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    rig = disk_commit
    restore = _defer_owner_settlement(rig, monkeypatch, "select")
    rig.jobs[0]()
    rig.controller.stems._poll_pair_preparations()
    settlement = rig.controller.stems._disk_selection_settlements[0]
    committed_pair = Path(settlement.entry.cache_dir)
    if change == "content":
        rig.controller.stems.invalidate_stem_cache(0)
        rig.controller.project.pad_content[0] = PadContentIdentity(instance_id="8" * 32)
        replacement = rig.previous.model_copy(deep=True)
        rig.controller.project.stem_cache[0] = replacement
        rig.controller._assets.sync_assignments()
    else:
        rig.controller.loader.unload_sample(0)
        replacement = None
        assert rig.controller.project.sample_paths[0] is None
    assert rig.controller.project.stem_cache[0] is replacement
    assert committed_pair.is_dir()
    assert not cast("FakeProjectAssetLease", settlement.lease).released
    restore()
    rig.controller.stems._poll_disk_selection_settlements()
    assert rig.controller.project.stem_cache[0] is replacement
    assert not rig.controller.stems._disk_selection_settlements
    assert not rig.controller.stems._committed_residency.contains(0)
    assert cast("FakeProjectAssetLease", settlement.lease).released
    assert settlement.previous_lease is None
    assert not committed_pair.exists()
    assert rig.controller._assets._reserved == 0
    assert rig.prepared.selected
    assert not rig.prepared.discarded
    rig.controller._persistence.flush(now=1.0)
    reopened = ProjectPersistence.from_config_path(rig.controller._persistence.config_path)
    assert reopened.project.stem_cache[0] == replacement
    assert reopened.project.stem_cache[1] == rig.independent
    _assert_independent_copy(rig)


def _change_after_disk_replace(rig: _DiskCommit, change: str) -> StemCacheEntry | None:
    if change == "unload":
        rig.controller.loader.unload_sample(0)
        assert rig.controller.project.sample_paths[0] is None
        return None
    rig.controller.stems.invalidate_stem_cache(0)
    rig.controller.project.pad_content[0] = PadContentIdentity(instance_id="8" * 32)
    replacement = rig.previous.model_copy(deep=True)
    rig.controller.project.stem_cache[0] = replacement
    rig.controller._assets.sync_assignments()
    rig.controller._persistence.mark_dirty()
    return replacement


def _assert_stale_pair_custody(rig: _DiskCommit) -> None:
    settlement = rig.controller.stems._disk_selection_settlements[0]
    assert settlement.request is rig.request
    assert not cast("FakeProjectAssetLease", settlement.lease).released
    assert settlement.previous_lease is not None
    assert not cast("FakeProjectAssetLease", settlement.previous_lease).released
    assert settlement.retirement.remaining > 0
    assert rig.controller._assets._reserved == settlement.retirement.remaining
    assert Path(settlement.entry.cache_dir).is_dir()
    assert not rig.prepared.discarded
    assert not rig.controller.stems._committed_residency.contains(0)
    _assert_reopened(rig, settlement.entry)
    _assert_independent_copy(rig)


def _assert_committed_change_pending(
    rig: _DiskCommit,
    written: list[ProjectState],
    replacement: StemCacheEntry | None,
) -> None:
    settlement = rig.controller.stems._disk_selection_settlements[0]
    assert rig.controller.project.stem_cache[0] is replacement
    assert len(written) == 1
    assert written[0].stem_cache[0] == settlement.entry
    assert written[0].pad_content[0] is not None
    assert written[0].pad_content[0].instance_id == rig.request.content_id
    assert "newer config flush failure" in rig.controller.session.stem_generation_errors[0]
    _assert_stale_pair_custody(rig)


def _assert_newer_snapshot(
    rig: _DiskCommit, snapshot: ProjectState, replacement: StemCacheEntry | None
) -> None:
    assert snapshot.stem_cache[0] == replacement
    assert snapshot.pad_content[0] == rig.controller.project.pad_content[0]
    assert snapshot.sample_paths[0] == rig.controller.project.sample_paths[0]


@pytest.mark.parametrize("change", ["content", "unload"])
def test_after_replace_change_retains_committed_pair_until_newer_config_is_durable(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    rig = disk_commit
    replace = os.replace
    fsync = os.fsync
    written: list[ProjectState] = []
    replacement: StemCacheEntry | None = None

    def change_after_actual_replace(source: str | Path, destination: str | Path) -> None:
        nonlocal replacement
        replace(source, destination)
        written.append(
            ProjectPersistence.from_config_path(rig.controller._persistence.config_path).project
        )
        if len(written) == 1:
            replacement = _change_after_disk_replace(rig, change)

    def reject_newer_config_fsync(descriptor: int) -> None:
        if written:
            message = "injected newer config flush failure"
            raise OSError(message)
        fsync(descriptor)

    replacements = Mock(side_effect=change_after_actual_replace)
    monkeypatch.setattr(os, "replace", replacements)
    monkeypatch.setattr(os, "fsync", reject_newer_config_fsync)
    rig.jobs[0]()
    rig.controller.stems._poll_pair_preparations()
    settlement = rig.controller.stems._disk_selection_settlements[0]
    _assert_committed_change_pending(rig, written, replacement)
    released_after_newer_config: list[bool] = []
    release = settlement.lease.release

    def release_only_after_newer_config() -> None:
        reopened = ProjectPersistence.from_config_path(rig.controller._persistence.config_path)
        assert reopened.load_error is None
        _assert_newer_snapshot(rig, reopened.project, replacement)
        assert len(written) == 2
        released_after_newer_config.append(True)
        release()

    monkeypatch.setattr(settlement.lease, "release", release_only_after_newer_config)
    monkeypatch.setattr(os, "fsync", fsync)
    rig.controller.stems._poll_disk_selection_settlements()
    assert rig.controller.project.stem_cache[0] is replacement
    assert not rig.controller.stems._disk_selection_settlements
    assert released_after_newer_config == [True]
    assert settlement.previous_lease is None
    assert rig.controller._assets._reserved == 0
    assert not Path(settlement.entry.cache_dir).exists()
    assert rig.prepared.selected
    assert not rig.prepared.discarded
    assert not rig.controller.stems._committed_residency.contains(0)
    assert not rig.controller._persistence._dirty
    _assert_newer_snapshot(rig, written[1], replacement)
    assert replacements.call_count == 2
    _assert_independent_copy(rig)
