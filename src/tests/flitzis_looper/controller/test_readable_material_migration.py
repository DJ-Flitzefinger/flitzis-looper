"""Real sealed original/group/config IO with explicitly substituted source ACK tickets.

These tests certify all-bank aliases and independent current intent. They make no
216-native-voice, callback, RAM, audio device, neural inference or hearing claim.
"""

import hashlib
import json
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller import material_migration as migration_module
from flitzis_looper.controller.material_migration_cleanup import MigrationArtifactReconciler
from flitzis_looper.controller.material_migration_grouping import (
    current_candidates,
    original_requests,
    retain_content_group,
)
from flitzis_looper.controller.stem_cache import cache_dir_for_sample_id, expected_stem_files
from flitzis_looper.models import (
    STEM_KINDS,
    PadContentIdentity,
    ProjectState,
    SessionState,
    StemCacheEntry,
)
from flitzis_looper.project_materials import original_asset, resolve_asset
from flitzis_looper.stem_intent import PadStemIntent
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import write_test_stem_marker
from tests.flitzis_looper.controller.test_material_migration import (
    _OLD,
    _DeliveredLease,
    _Harness,
    _Preparation,
)

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _readable(source: Path) -> tuple[str, str]:
    root = source.parent / "Readable take.wav"
    root.mkdir()
    target = root / "Readable take.wav"
    target.write_bytes(source.read_bytes())
    digest = hashlib.sha256(target.read_bytes()).hexdigest()
    (root / ".material.json").write_text(
        json.dumps({
            "schema_version": 1,
            "sha256": digest,
            "bytes": target.stat().st_size,
            "original_leaf": target.name,
        }),
        encoding="utf-8",
    )
    return target.as_posix(), digest[:32]


def test_native_legacy_details_keep_slot_cache_and_owned_stem_assignment(
    controller: AppController,
) -> None:
    source = Path("samples/Legacy original.wav")
    source.parent.mkdir(exist_ok=True)
    write_mono_pcm16_wav(source, 44_100)
    cache = Path("samples/stems/#7")
    cache.mkdir(parents=True)
    for kind in STEM_KINDS:
        write_mono_pcm16_wav(cache / f"{kind}.wav", 44_100)
    version = f"{source.as_posix()}|sha256-v1:{hashlib.sha256(source.read_bytes()).hexdigest()}"
    write_test_stem_marker(cache, version)
    controller.project.sample_paths[6] = source.as_posix()
    entry = StemCacheEntry(
        source_version=version,
        cache_dir=cache.as_posix(),
        stems=expected_stem_files(cache.as_posix()),
        available=True,
    )
    controller.project.stem_cache[6] = entry
    assert original_asset(source).layout == "legacy"
    assert original_asset(source).material_root is None
    assert resolve_asset(cache).material_root is None
    assert cache_dir_for_sample_id(6, source.as_posix()) == cache.as_posix()
    assert controller._assets._stem_assignment_path(6, entry) == cache.resolve()
    assignments = controller._assets._current_assignments()
    assert assignments["stems", 6] == cache.resolve()


class _ReadablePreparation(_Preparation):
    def __init__(self, target: str, material: str) -> None:
        super().__init__()
        self.target = target
        self.material = material

    def metadata_json(self) -> str:
        result = json.loads(super().metadata_json())
        result |= {
            "new_reference": self.target,
            "material_id": self.material,
            "cache_path": str(Path(self.target).parent / ".pcm-cache/v1" / f".ready-{'d' * 32}"),
        }
        return json.dumps(result)


def test_both_old_layouts_group_by_real_full_content_and_hold_each_original(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(migration_module, "ProjectAssetLease", _DeliveredLease)
    rig = _Harness(controller, audio_engine_mock)
    second = Path(f"samples/materials/M{'7' * 32}/original/Different name.wav")
    second.parent.mkdir(parents=True)
    second.write_bytes(Path(_OLD).read_bytes())
    original_content = controller.project.pad_content[215]
    assert original_content is not None
    controller.project.sample_paths[215] = second.as_posix()
    controller.project.pad_content[215] = PadContentIdentity(
        instance_id=original_content.instance_id, material_id="7" * 32
    )
    target, material = _readable(Path(_OLD))
    rig.preparation = _ReadablePreparation(target, material)
    audio_engine_mock.prepare_material_migration.return_value = rig.preparation
    controller.project.pad_key_lock[0] = False
    controller.project.pad_key_lock[215] = True
    controller.project.pad_stem_intent[0] = PadStemIntent(
        enabled_mask=1, last_custom_mask=1, display_mode="custom"
    )
    controller.project.pad_stem_intent[215] = PadStemIntent(
        enabled_mask=8, last_custom_mask=8, display_mode="custom"
    )
    expected = controller.project.model_copy(deep=True)
    rig.begin()
    assignments = rig.records()[-1].assignments
    assert [(item.sample_id, item.old_reference) for item in assignments] == [
        (0, _OLD),
        (215, second.as_posix()),
    ]
    assert len(rig.migration._artifact_captures[0].leases) == 2
    with pytest.raises(PermissionError):
        second.write_bytes(b"replacement under held sealed source")
    rig.migration.poll()
    assert set(rig.sources) == {0, 215}
    rig.sources[215].state = "acknowledged"
    rig.migration.poll()
    assert controller.project.sample_paths == expected.sample_paths
    rig.sources[0].state = "acknowledged"
    rig.migration.poll()
    assert rig.migration.status == "config_committed"
    saved = ProjectState.model_validate_json(controller.persistence.config_path.read_bytes())
    assert saved.sample_paths[0] == saved.sample_paths[215] == target
    assert saved.pad_key_intent == expected.pad_key_intent
    assert saved.pad_key_lock == expected.pad_key_lock
    assert saved.pad_loop_start_s == expected.pad_loop_start_s
    assert saved.pad_loop_end_s == expected.pad_loop_end_s
    assert saved.pad_gain_db == expected.pad_gain_db
    assert saved.pad_eq_low_db == expected.pad_eq_low_db
    assert saved.pad_stem_intent == expected.pad_stem_intent
    assert controller.project.pad_stem_intent[0].enabled_mask == 1
    assert controller.project.pad_stem_intent[215].enabled_mask == 8
    assert second.read_bytes() == Path(_OLD).read_bytes()
    assert original_asset(target).layout == "readable"


def test_real_grouping_distinguishes_same_size_bytes_and_never_adopts_unknown_external(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    original = samples / "first.wav"
    write_mono_pcm16_wav(original, 48_000)
    equal = samples / "renamed.wav"
    equal.write_bytes(original.read_bytes())
    different = samples / "same-size.wav"
    changed = bytearray(original.read_bytes())
    changed[-1] ^= 1
    different.write_bytes(changed)
    foreign = tmp_path / "private.wav"
    foreign.write_bytes(original.read_bytes())
    project = ProjectState()
    for index, source in enumerate((original, equal, different, foreign)):
        project.sample_paths[index] = source.relative_to(tmp_path).as_posix()
        project.pad_content[index] = PadContentIdentity(instance_id=f"{index + 1:032x}")
    candidates = current_candidates(project, SessionState())
    assert [item.sample_id for item in candidates] == [0, 1, 2]
    reconciler = MigrationArtifactReconciler(samples, samples / "group.config.json")
    capture = reconciler.capture(
        original_requests("samples/first.wav", candidates), include_legacy_pcm=False
    ).result(timeout=10)
    try:
        grouped, witness = retain_content_group("samples/first.wav", capture, candidates)
        assert [item.sample_id for item in grouped] == [0, 1]
        assert witness == (
            hashlib.sha256(original.read_bytes()).hexdigest(),
            original.stat().st_size,
        )
        assert len(capture.leases) == len(capture.records) == 2
        with pytest.raises(PermissionError):
            equal.write_bytes(b"forbidden write")
        different.write_bytes(b"unrelated source remains independently writable")
        assert foreign.read_bytes() == original.read_bytes()
    finally:
        capture.release()
        reconciler.shut_down()


def test_all_216_old_aliases_group_without_shared_performer_state_or_native_voice_claim(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    sources = [samples / "first.wav", samples / "last.wav"]
    write_mono_pcm16_wav(sources[0], 48_000)
    sources[1].write_bytes(sources[0].read_bytes())
    project = ProjectState()
    for index in range(216):
        project.sample_paths[index] = sources[index % 2].relative_to(tmp_path).as_posix()
        project.pad_content[index] = PadContentIdentity(instance_id=f"{index + 1:032x}")
        project.pad_gain_db[index] = float(index % 13 - 6)
        project.pad_loop_start_s[index] = index / 48_000
        project.pad_key_lock[index] = bool(index % 2)
    before = project.model_copy(deep=True)
    candidates = current_candidates(project, SessionState())
    reconciler = MigrationArtifactReconciler(samples, samples / "all.config.json")
    capture = reconciler.capture(
        original_requests("samples/first.wav", candidates), include_legacy_pcm=False
    ).result(timeout=10)
    try:
        assignments, _witness = retain_content_group("samples/first.wav", capture, candidates)
        assert len(assignments) == 216
        assert len({item.instance_id for item in assignments}) == 216
        assert len(capture.leases) == 2
        assert project == before
        project.pad_gain_db[0] = -9.0
        assert project.pad_gain_db[215] == before.pad_gain_db[215]
        content = before.pad_content[215]
        assert content is not None
        assert assignments[215].instance_id == content.instance_id
    finally:
        capture.release()
        reconciler.shut_down()
