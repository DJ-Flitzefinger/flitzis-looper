from typing import cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.models import ProjectState, SessionState, StemCacheEntry, StemSeparator


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
