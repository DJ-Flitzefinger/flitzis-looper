from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, call

import pytest

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.models import ProjectState, SessionState, StemCacheEntry, StemSeparator

if TYPE_CHECKING:
    from pathlib import Path


def test_separator_switch_and_rollback_preserve_stems_and_running_intent() -> None:
    project = ProjectState(demucs_shifts=4, demucs_overlap=0.25)
    entry = StemCacheEntry(source_version="saved-source", cache_dir="saved-set", available=True)
    project.stem_cache[0] = entry
    project.pad_stem_mix_mode[0] = "all_stems"
    session = SessionState()
    session.stem_generating_sample_ids.add(1)
    session.stem_generation_source_versions[1] = "running-source"
    audio = Mock()
    changed = Mock()
    settings = SettingsController(project, session, audio, on_project_changed=changed)

    settings.set_stem_separator("bs-roformer:musdb18hq")

    assert project.stem_processing_revision == 1
    assert project.stem_separator == "bs-roformer:musdb18hq"
    assert project.stem_cache[0] is entry
    assert project.pad_stem_mix_mode[0] == "all_stems"
    assert session.stem_generating_sample_ids == {1}
    assert session.stem_generation_source_versions == {1: "running-source"}
    assert audio.mock_calls == []

    settings.set_stem_separator("demucs:htdemucs")

    assert project.stem_processing_revision == 2
    assert project.model_dump()["stem_separator"] == "demucs:htdemucs"
    assert project.demucs_shifts == 4
    assert project.demucs_overlap == 0.25
    assert project.stem_cache[0] is entry
    assert audio.mock_calls == []
    assert changed.call_count == 2


def test_unchanged_separator_does_not_mark_project_dirty() -> None:
    changed = Mock()
    project = ProjectState()
    settings = SettingsController(project, SessionState(), Mock(), on_project_changed=changed)

    settings.set_stem_separator("demucs:htdemucs")

    assert project.stem_processing_revision == 0
    changed.assert_not_called()


def test_unknown_separator_rejects_without_mutating_project() -> None:
    project = ProjectState()
    changed = Mock()
    settings = SettingsController(project, SessionState(), Mock(), on_project_changed=changed)
    previous = project.model_dump()

    with pytest.raises(ValueError, match="supported separator/model identity"):
        settings.set_stem_separator(cast("StemSeparator", "unknown:model"))

    assert project.model_dump() == previous
    assert project.stem_processing_revision == 0
    changed.assert_not_called()


def test_settings_quality_aba_advances_only_the_models_effective_revision() -> None:
    project = ProjectState(demucs_shifts=4, demucs_overlap=0.25)
    changed = Mock()
    settings = SettingsController(project, SessionState(), Mock(), on_project_changed=changed)

    settings.set_demucs_quality(shifts=6, overlap=0.25)
    settings.set_demucs_quality(shifts=4, overlap=0.25)
    assert project.stem_processing_revision == 2
    assert changed.call_count == 2

    settings.set_demucs_quality(shifts=4, overlap=0.25)
    assert project.stem_processing_revision == 2
    assert changed.call_count == 2

    with pytest.raises(ValueError, match="demucs shifts"):
        settings.set_demucs_quality(shifts=0, overlap=0.25)
    assert project.stem_processing_revision == 2
    assert changed.call_count == 2


def test_settings_stores_roformer_inactive_quality_without_invalidating_effective_intent() -> None:
    project = ProjectState(stem_separator="bs-roformer:musdb18hq")
    changed = Mock()
    settings = SettingsController(project, SessionState(), Mock(), on_project_changed=changed)

    settings.set_demucs_quality(shifts=4, overlap=0.25)

    assert project.demucs_shifts == 4
    assert project.demucs_overlap == 0.25
    assert project.stem_processing_revision == 0
    changed.assert_called_once_with()
    settings.set_stem_separator("demucs:htdemucs")
    assert project.stem_processing_revision == 1


@pytest.mark.parametrize("value", [True, 128.0, "512", None])
def test_resident_budget_rejects_noninteger_before_native_or_persistence(value: object) -> None:
    project = ProjectState()
    audio, changed = Mock(), Mock()
    settings = SettingsController(project, SessionState(), audio, on_project_changed=changed)
    with pytest.raises(TypeError, match="integer"):
        settings.set_resident_pcm_budget_mib(cast("int", value))
    assert project.resident_pcm_budget_mib == 512
    audio.set_resident_pcm_budget_mib.assert_not_called()
    changed.assert_not_called()


@pytest.mark.parametrize("value", [127, 16385])
def test_resident_budget_rejects_out_of_range_without_native_mutation(value: int) -> None:
    audio = Mock()
    project = ProjectState()
    with pytest.raises(ValueError, match="128 and 16384"):
        SettingsController(project, SessionState(), audio).set_resident_pcm_budget_mib(value)
    assert project.resident_pcm_budget_mib == 512
    audio.set_resident_pcm_budget_mib.assert_not_called()


def test_native_budget_failure_preserves_persisted_allowance_and_policy() -> None:
    project = ProjectState(preload_stem_loops_on_startup=True)
    audio = Mock()
    audio.set_resident_pcm_budget_mib.side_effect = RuntimeError("Native stopped")
    changed, policy = Mock(), Mock()
    settings = SettingsController(project, SessionState(), audio, on_project_changed=changed)
    settings.bind_residency_policy(policy)
    with pytest.raises(RuntimeError, match="Native stopped"):
        settings.set_resident_pcm_budget_mib(128)
    assert project.resident_pcm_budget_mib == 512
    assert project.preload_stem_loops_on_startup
    changed.assert_not_called()
    policy.assert_not_called()


def test_budget_lowering_applies_native_before_dirtying_without_stopping_audio() -> None:
    project = ProjectState(resident_pcm_budget_mib=2048)
    project.pad_stem_mix_mode[0] = "all_stems"
    session = SessionState(active_sample_ids={0})
    audio, changed = Mock(), Mock()

    def apply_native(value: int) -> None:
        assert project.resident_pcm_budget_mib == 2048
        assert value == 128

    audio.set_resident_pcm_budget_mib.side_effect = apply_native
    settings = SettingsController(project, session, audio, on_project_changed=changed)
    settings.set_resident_pcm_budget_mib(128)
    assert project.resident_pcm_budget_mib == 128
    assert project.pad_stem_mix_mode[0] == "all_stems"
    assert session.active_sample_ids == {0}
    assert audio.mock_calls == [call.set_resident_pcm_budget_mib(128)]
    changed.assert_called_once_with()


def test_residency_settings_round_trip_and_old_config_defaults(tmp_path: Path) -> None:
    project = ProjectState()
    assert project.preload_stem_loops_on_startup is False
    assert project.resident_pcm_budget_mib == 512
    project.pad_stem_mix_mode[0] = "all_stems"
    settings = SettingsController(project, SessionState(), Mock())
    settings.set_preload_stem_loops_on_startup(enabled=True)
    settings.set_resident_pcm_budget_mib(16384)
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "project.json"
    persistence.mark_dirty()
    persistence.flush()
    reopened = ProjectPersistence.from_config_path(persistence.config_path).project
    assert reopened.preload_stem_loops_on_startup
    assert reopened.resident_pcm_budget_mib == 16384
    assert reopened.pad_stem_mix_mode[0] == "all_stems"
