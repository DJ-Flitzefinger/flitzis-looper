import hashlib
import json
import time
from contextlib import closing
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.models import STEM_KINDS, ProjectState, StemCacheEntry
from flitzis_looper_audio import AudioEngine

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path

    from flitzis_looper.controller.asset_lifecycle import AssignmentImage, _AssignmentRetirement
    from flitzis_looper_audio import ProjectAssetLease


@pytest.fixture
def native_assets(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> Iterator[ProjectAssetLifecycle]:
    """Use actual Native file owners without starting audio or opening a device."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / "samples").mkdir()
    audio = AudioEngine()
    assets = ProjectAssetLifecycle(ProjectState(), audio)
    try:
        yield assets
    finally:
        try:
            assets.release_saved_assignments()
        finally:
            audio.shut_down()


def _native_original(
    assets: ProjectAssetLifecycle, sample_id: int, name: str
) -> tuple[Path, ProjectAssetLease]:
    source = assets._root / "samples" / name
    source.write_bytes(name.encode())
    assets._project.sample_paths[sample_id] = f"samples/{name}"
    prepared = (source, assets.acquire(source))
    assets._assignments["original", sample_id] = prepared
    return prepared


def _native_replacement(assets: ProjectAssetLifecycle, sample_id: int) -> AssignmentImage:
    candidate = assets._project.model_copy(deep=True)
    name = f"new-{sample_id}.wav"
    (assets._root / "samples" / name).write_bytes(b"new Native assignment source")
    candidate.sample_paths[sample_id] = f"samples/{name}"
    return assets.prepare_assignment_image(candidate)


def _wait_for_deletion(path: Path, audio: AudioEngine) -> None:
    deadline = time.monotonic() + 5
    while path.exists() and time.monotonic() < deadline:
        time.sleep(0.01)
    assert not path.exists(), audio.project_asset_cleanup_status()


def _saved_direct_stems(assets: ProjectAssetLifecycle, layout: str) -> tuple[Path, Path, str]:
    samples = assets._root / "samples"
    payload = b"owned synthetic original"
    digest = hashlib.sha256(payload).hexdigest()
    if layout == "legacy":
        original = samples / "source.wav"
        stems = samples / "stems" / "#1"
    elif layout == "readable":
        material = samples / "source.wav"
        material.mkdir()
        original = material / "source.wav"
        (material / ".material.json").write_text(
            json.dumps({
                "schema_version": 1,
                "sha256": digest,
                "bytes": len(payload),
                "original_leaf": original.name,
            })
        )
        stems = material / "stems"
    else:
        material = samples / "materials" / f"M{digest[:32]}"
        original = material / "original" / "source.wav"
        stems = material / "stems"
    original.parent.mkdir(parents=True, exist_ok=True)
    original.write_bytes(payload)
    stems.mkdir(parents=True)
    leaves = [*(f"{kind}.wav" for kind in STEM_KINDS), ".complete.json"]
    for name in leaves:
        (stems / name).write_bytes(f"declared {name}".encode())
    unknown = stems / "performer-notes.txt"
    unknown.write_bytes(b"preserved unknown content")
    newer = stems / f".ready-{'a' * 32}"
    newer.mkdir()
    (newer / "drums.wav").write_bytes(b"newer generation")
    source = original.relative_to(assets._root).as_posix()
    cache = stems.relative_to(assets._root).as_posix()
    entry = StemCacheEntry(
        source_version=f"{source}|sha256-v1:{digest}",
        cache_dir=cache,
        stems=expected_stem_files(cache),
        available=True,
    )
    for slot in [0, 1]:
        assets._project.sample_paths[slot] = source
        assets._project.stem_cache[slot] = entry.model_copy(deep=True)
    assets.sync_assignments()
    return original, stems, source


@pytest.mark.parametrize("layout", ["legacy", "readable", "material_id"])
def test_saved_direct_stems_retire_after_last_copy_without_clearing_sources(
    native_assets: ProjectAssetLifecycle, layout: str
) -> None:
    """Use real Native path admission, alias owners and physical off-thread cleanup."""
    assets = native_assets
    original, stems, source = _saved_direct_stems(assets, layout)
    leaves = [*(f"{kind}.wav" for kind in STEM_KINDS), ".complete.json"]
    assert ("stems", 0) in assets._assignments
    assert ("stems", 1) in assets._assignments
    with pytest.raises((RuntimeError, ValueError)):
        assets._audio.retire_project_asset(str(stems), recursive=True)
    with pytest.raises((RuntimeError, ValueError)):
        assets._audio.retire_project_asset(str(stems / "performer-notes.txt"))
    assets._project.stem_cache[0] = None
    with assets.admission():
        assets.sync_assignments()
    assert not assets.retirement_errors
    assert all((stems / name).exists() for name in leaves)
    reader = assets.acquire(stems / "drums.wav")
    assets._project.stem_cache[1] = None
    try:
        with assets.admission():
            assets.sync_assignments()
        assert not assets.retirement_errors
        for name in leaves:
            if name != "drums.wav":
                _wait_for_deletion(stems / name, assets._audio)
        assert (stems / "drums.wav").read_bytes() == b"declared drums.wav"
    finally:
        reader.release()
    for name in leaves:
        _wait_for_deletion(stems / name, assets._audio)
    assert assets._project.sample_paths[:2] == [source, source]
    assert original.read_bytes() == b"owned synthetic original"
    assert (stems / "performer-notes.txt").read_bytes() == b"preserved unknown content"
    assert (stems / f".ready-{'a' * 32}" / "drums.wav").read_bytes() == b"newer generation"


def test_uninitialized_native_registry_waits_for_last_assignment_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "samples" / "owned.wav"
    source.parent.mkdir()
    source.write_bytes(b"owned complete original")
    audio = AudioEngine()
    first = audio.acquire_project_asset_lease(str(source))
    second = audio.acquire_project_asset_lease(str(source))
    audio.retire_project_asset(str(source))

    first.release()
    assert first.released
    assert not second.released
    assert source.exists()
    second.release()
    _wait_for_deletion(source, audio)
    assert not any(str(source) in error for error in audio.project_asset_cleanup_status()[3])
    audio.shut_down()


def test_native_shutdown_keeps_late_python_reader_and_cleans_after_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "samples" / "job-original.wav"
    source.parent.mkdir()
    source.write_bytes(b"backend still reading")
    audio = AudioEngine()
    job = audio.acquire_project_asset_lease(str(source))
    audio.retire_project_asset(str(source))

    audio.shut_down()
    assert source.read_bytes() == b"backend still reading"
    job.release()
    _wait_for_deletion(source, audio)


def test_native_retirement_cannot_delete_external_original_or_unknown_pad_container(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    pad = samples / "stems" / "#1"
    pad.mkdir(parents=True)
    private = pad / "private.txt"
    private.write_bytes(b"private")
    external = tmp_path / "external.wav"
    external.write_bytes(b"external source")
    audio = AudioEngine()

    with pytest.raises((RuntimeError, ValueError)):
        audio.retire_project_asset(str(external))
    with pytest.raises((RuntimeError, ValueError)):
        audio.retire_project_asset(str(pad), recursive=True)
    assert external.read_bytes() == b"external source"
    assert private.read_bytes() == b"private"
    audio.shut_down()


@pytest.mark.parametrize("released_owner", ["acquired", "reused"])
def test_native_publication_rejects_released_required_owner_before_store(
    native_assets: ProjectAssetLifecycle, released_owner: str
) -> None:
    original = _native_original(native_assets, 0, "old.wav")
    reused = _native_original(native_assets, 1, "reused.wav")
    image = _native_replacement(native_assets, 0)
    try:
        plan = native_assets.prepare_assignment_publication(image)
        before = native_assets._assignments
        required = image[1]["original", 0][1] if released_owner == "acquired" else reused[1]
        required.release()
        assert required.released is True

        with pytest.raises(RuntimeError, match="ownership changed before"):
            plan.apply()

        assert native_assets._assignments is before
        assert native_assets._assignments["original", 0] is original
        assert native_assets._assignments["original", 1] is reused
        assert not plan._store_attempted
        assert not plan.applied
        assert not plan.settled
        assert plan.owns_unpublished_image
        assert plan._retirement_index == 0
        assert original[1].released is False
        assert original[0].read_bytes() == b"old.wav"
        assert not native_assets._pending
        assert not native_assets._pending_groups
    finally:
        native_assets.release_assignment_image(image)


def test_native_publication_retry_skips_released_old_owner_and_preserves_later_table(
    native_assets: ProjectAssetLifecycle, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Inject only a Python return interruption; Native custody and cleanup stay real."""
    original = _native_original(native_assets, 0, "old.wav")
    image = _native_replacement(native_assets, 0)
    witness = native_assets.acquire(original[0])
    try:
        plan = native_assets.prepare_assignment_publication(image)
        retire = native_assets._retire_assignment
        retirements: list[ProjectAssetLease] = []

        def interrupt_after_release(
            kind: str,
            path: Path,
            lease: ProjectAssetLease,
            *,
            progress: _AssignmentRetirement | None = None,
        ) -> None:
            retirements.append(lease)
            retire(kind, path, lease, progress=progress)
            if len(retirements) == 1:
                assert lease.released is True
                message = "interrupted after actual Native release"
                raise RuntimeError(message)

        monkeypatch.setattr(native_assets, "_retire_assignment", interrupt_after_release)
        with closing(native_assets.reserve()) as reservation:
            with (
                reservation.activate(),
                pytest.raises(RuntimeError, match="after actual Native release"),
            ):
                plan.apply()
            assert original[1].released is True
            assert plan.applied
            assert not plan.settled
            assert not plan.owns_unpublished_image
            assert plan._retirement_index == 0
            assert witness.released is False
            assert original[0].read_bytes() == b"old.wav"

            later = _native_original(native_assets, 0, "later-occupant.wav")
            latest = native_assets._assignments.copy()
            native_assets._assignments = latest
            with reservation.activate():
                plan.apply()

            assert plan.settled
            assert retirements == [original[1]]
            assert native_assets._assignments is latest
            assert native_assets._assignments["original", 0] is later
            assert later[1].released is False
            assert image[1]["original", 0][1].released is False
            assert later[0].read_bytes() == b"later-occupant.wav"
            assert image[1]["original", 0][0].read_bytes() == b"new Native assignment source"
            plan.apply()
            assert retirements == [original[1]]
            assert native_assets._assignments is latest
            witness.release()
            _wait_for_deletion(original[0], native_assets._audio)
    finally:
        witness.release()
        native_assets.release_assignment_image(image)
