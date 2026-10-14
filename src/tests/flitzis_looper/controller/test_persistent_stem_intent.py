"""Durable musical stem choices remain independent of prepared-set availability."""

import json
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from pydantic import ValidationError

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import STEM_COMPONENT_MASK, STEM_INSTRUMENTAL_PRESET_MASK, StemCacheEntry
from flitzis_looper.stem_intent import PadStemIntent

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController


def test_public_mask_choices_roundtrip_independently_with_all_components_muted(
    controller: AppController, tmp_path: Path
) -> None:
    for sample_id in (0, 36, 215):
        controller.project.sample_paths[sample_id] = "samples/same-source.wav"
    assert controller.stems.set_stem_enabled_mask(0, 5, "custom")
    assert controller.stems.set_stem_enabled_mask(0, STEM_INSTRUMENTAL_PRESET_MASK, "instrumental")
    assert controller.stems.set_stem_enabled_mask(36, 0, "custom")
    assert controller.stems.set_stem_enabled_mask(215, 3, "custom")
    assert controller.stems.set_stem_enabled_mask(215, STEM_COMPONENT_MASK, "all")
    intended = tuple(controller.project.pad_stem_intent)

    persistence = ProjectPersistence(controller.project)
    persistence.config_path = tmp_path / "samples" / "intent.config.json"
    persistence.flush(now=0.0)
    raw = json.loads(persistence.config_path.read_text(encoding="utf-8"))
    assert raw["pad_stem_intent"][36]["enabled_mask"] == 0
    assert not {
        "pad_stem_enabled_mask",
        "pad_stem_last_custom_mask",
        "pad_stem_mask_display_mode",
    }.intersection(raw)

    reopened = ProjectPersistence.from_config_path(persistence.config_path).project
    assert tuple(reopened.pad_stem_intent) == intended
    assert reopened.pad_stem_intent[0].last_custom_mask == 5
    assert reopened.pad_stem_intent[0].display_mode == "instrumental"
    assert reopened.pad_stem_intent[36].enabled_mask == 0
    assert reopened.pad_stem_intent[215].last_custom_mask == 3
    assert reopened.pad_stem_intent[215].display_mode == "all"
    reopened.pad_stem_intent[36] = PadStemIntent()
    assert tuple(controller.project.pad_stem_intent) == intended
    assert reopened.pad_stem_intent[0] == intended[0]
    assert reopened.pad_stem_intent[215] == intended[215]


def test_identical_public_mask_gesture_keeps_revision_and_publication_unchanged(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    changed = Mock()
    publish = Mock(return_value=False)
    monkeypatch.setattr(controller.stems, "_on_project_changed", changed)
    monkeypatch.setattr(controller.stems, "publish_stem_enabled_mask_if_available", publish)

    assert not controller.stems.set_stem_enabled_mask(0, 5, "custom")
    requested = controller.project.pad_stem_intent[0]
    changed.assert_called_once_with()
    publish.assert_called_once_with(0)
    assert controller.stems.set_stem_enabled_mask(0, 5, "custom")
    assert controller.project.pad_stem_intent[0] is requested
    changed.assert_called_once_with()
    publish.assert_called_once_with(0)


def test_full_mix_and_missing_restored_set_keep_current_musical_choice(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    assert controller.stems.set_stem_enabled_mask(0, 5, "custom")
    assert controller.stems.set_stem_enabled_mask(0, STEM_INSTRUMENTAL_PRESET_MASK, "instrumental")
    intended = controller.project.pad_stem_intent[0]
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    assert controller.stems.set_stem_mix_mode(0, "full_mix")
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "full_mix")
    assert controller.project.pad_stem_intent[0] is intended

    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="samples/missing.wav|sha256-v1:" + "a" * 64,
        cache_dir="samples/stems/#1",
        available=True,
    )
    controller.stems.restore_stem_cache_from_project_state()
    assert controller.project.stem_cache[0] is None
    assert controller.project.pad_stem_intent[0] is intended
    assert controller.stems.stem_enabled_mask(0) == STEM_INSTRUMENTAL_PRESET_MASK
    assert controller.stems.remembered_custom_mask(0) == 5
    assert controller.stems.stem_mask_display_mode(0) == "instrumental"


@pytest.mark.parametrize("mask", [True, -1, 16])
def test_invalid_public_mask_never_changes_intent_or_requests_publication(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, mask: int
) -> None:
    changed = Mock()
    publish = Mock()
    monkeypatch.setattr(controller.stems, "_on_project_changed", changed)
    monkeypatch.setattr(controller.stems, "publish_stem_enabled_mask_if_available", publish)
    intended = controller.project.pad_stem_intent[0]

    with pytest.raises(ValidationError):
        controller.stems.set_stem_enabled_mask(0, mask)
    assert controller.project.pad_stem_intent[0] is intended
    changed.assert_not_called()
    publish.assert_not_called()
