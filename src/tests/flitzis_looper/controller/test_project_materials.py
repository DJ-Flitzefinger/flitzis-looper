import subprocess
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.stem_cache import cache_dir_for_sample_id
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper.project_materials import original_asset, resolve_asset

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


MATERIAL = "0123456789abcdef0123456789abcdef"
REFERENCE = f"samples/materials/M{MATERIAL}/original/Mélodie 東京.wav"


@pytest.mark.parametrize("existing_root", [False, True])
def test_native_resolver_keeps_relative_plain_and_extended_absolute_binding(
    tmp_path: Path, *, existing_root: bool
) -> None:
    if existing_root:
        (tmp_path / "samples").mkdir()
    absolute = tmp_path / REFERENCE
    relative = original_asset(REFERENCE, project_root=tmp_path)
    plain = original_asset(absolute, project_root=tmp_path)
    extended = original_asset("\\\\?\\" + str(absolute), project_root=tmp_path)
    assert relative == plain == extended
    assert relative.path == absolute
    assert relative.material_id == MATERIAL
    assert not absolute.exists(), "resolution must not create a filesystem artifact"


@pytest.mark.parametrize(
    "reference",
    [
        "samples/../outside.wav",
        "samples/./Take.wav",
        "samples/#0/membership.json",
        "samples/#217/membership.json",
        "samples/#01/membership.json",
        "samples/materials/Mbad/original/Take.wav",
        f"samples/materials/M{MATERIAL}/original/Take.wav:stream",
        f"samples/materials/M{MATERIAL}/original/Take.wav.",
        f"samples/materials/M{MATERIAL}/original/NUL.wav",
        f"samples/materials/M{MATERIAL}/manifest.json",
    ],
)
def test_native_resolver_rejects_unsafe_or_undeclared_references_before_normalization(
    tmp_path: Path, reference: str
) -> None:
    with pytest.raises(ValueError, match=r"."):
        resolve_asset(reference, project_root=tmp_path)
    assert list(tmp_path.iterdir()) == []


@pytest.mark.parametrize("sample_id", [0, 215])
def test_material_stem_root_is_independent_of_slot_and_wrong_artifact_is_not_original(
    tmp_path: Path, sample_id: int
) -> None:
    assert (
        cache_dir_for_sample_id(sample_id, sample_path=REFERENCE)
        == f"samples/materials/M{MATERIAL}/stems"
    )
    membership = resolve_asset(f"samples/#{sample_id + 1}/membership.json", project_root=tmp_path)
    assert membership.kind == "slot_membership"
    with pytest.raises(ValueError, match="typed original"):
        original_asset(f"samples/#{sample_id + 1}/membership.json", project_root=tmp_path)


def test_native_resolver_rejects_real_junction_without_touching_foreign_original(
    tmp_path: Path,
) -> None:
    foreign = tmp_path / "foreign"
    foreign.mkdir()
    original = foreign / "Take.wav"
    original.write_bytes(b"private bytes")
    samples = tmp_path / "samples"
    result = subprocess.run(
        ["cmd", "/c", "mklink", "/J", str(samples), str(foreign)],
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    try:
        with pytest.raises(ValueError, match=r"."):
            original_asset("samples/Take.wav", project_root=tmp_path)
        assert original.read_bytes() == b"private bytes"
        assert list(foreign.iterdir()) == [original]
    finally:
        samples.rmdir()


def test_durable_restore_preserves_content_identity_and_does_not_dirty_project(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    original = tmp_path / REFERENCE
    original.parent.mkdir(parents=True)
    original.write_bytes(b"original")
    identity = PadContentIdentity(instance_id="a" * 32, material_id=MATERIAL)
    controller.project.sample_paths[215] = REFERENCE
    controller.project.pad_content[215] = identity
    changed = Mock()
    monkeypatch.setattr(controller.loader, "_mark_project_changed", changed)
    controller.loader._schedule_restored_load(215, Path(REFERENCE), run_analysis=False)
    controller.loader._handle_loader_success(215, {"cached_path": REFERENCE})
    reopened = ProjectState.model_validate_json(controller.project.model_dump_json())
    assert reopened.pad_content[215] == identity
    assert reopened.sample_paths[215] == REFERENCE
    changed.assert_not_called()
    assert 215 not in controller.session.loading_sample_ids
    audio_engine_mock.load_sample_async.assert_called_once_with(
        215, REFERENCE, run_analysis=False, replace_assignment=True, source_intent="restore"
    )


def test_invalid_saved_material_is_rejected_before_native_load_admission(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    identity = PadContentIdentity(instance_id="a" * 32, material_id="b" * 32)
    controller.project.sample_paths[0] = REFERENCE
    controller.project.pad_content[0] = identity
    assert not controller.loader._schedule_restored_load(0, Path(REFERENCE), run_analysis=False)
    audio_engine_mock.load_sample_async.assert_not_called()
    assert controller.project.pad_content[0] == identity
    assert "material identity" in controller.session.sample_load_errors[0]
    assert controller.loader._assets._reserved == 0


@pytest.mark.parametrize("failure", ["invalid_path", "owner_capacity"])
def test_terminal_assignment_failure_preserves_old_tuple_and_settles_loading(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path, failure: str
) -> None:
    original = tmp_path / "samples" / "old.wav"
    original.parent.mkdir()
    original.write_bytes(b"old original")
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="c" * 32)
    controller.project.manual_bpm[0] = 123.0
    controller.loader._assets.sync_assignments()
    previous = controller.project.model_dump()
    owner = controller.loader._assets._assignments["original", 0][1]
    controller.loader.load_sample_async(0, "external.wav")
    controller.session.sample_load_progress[0] = 0.5
    controller.session.sample_load_stage[0] = "loading"
    if failure == "owner_capacity":
        audio_engine_mock.acquire_project_asset_lease.side_effect = RuntimeError("owner full")
        cached_path = REFERENCE
    else:
        cached_path = "samples/../outside.wav"
    controller.loader._handle_loader_success(0, {"cached_path": cached_path})
    assert controller.project.model_dump() == previous
    assert not owner.released
    assert original.read_bytes() == b"old original"
    assert 0 not in controller.session.loading_sample_ids
    assert 0 not in controller.session.pending_sample_paths
    assert 0 not in controller.session.sample_load_progress
    assert 0 not in controller.session.sample_load_stage
    assert not controller.loader._load_request_ids
    assert controller.loader._assets._reserved == 0
    assert "assignment failed" in controller.session.sample_load_errors[0]
