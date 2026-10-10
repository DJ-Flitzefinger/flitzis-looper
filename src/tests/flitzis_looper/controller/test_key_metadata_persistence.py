"""Actual key-intent persistence and bounded recovery preserve performer settings."""

import json
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest
from pydantic import ValidationError

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.persistence import PROJECT_CONFIG_PATH, ProjectPersistence
from flitzis_looper.key_intent import MAX_KEY_EPOCH, PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import BeatGrid, PadContentIdentity, ProjectState, SampleAnalysis

if TYPE_CHECKING:
    from pathlib import Path


def _performer_project() -> ProjectState:
    project = ProjectState(
        selected_pad=215,
        selected_bank=5,
        stem_separator="bs-roformer:musdb18hq",
        demucs_shifts=4,
        volume=0.6,
    )
    material = "a" * 32
    source = f"samples/materials/M{material}/original/Shared.wav"
    for sample_id, content in [(0, "b" * 32), (215, "c" * 32)]:
        project.sample_paths[sample_id] = source
        project.pad_content[sample_id] = PadContentIdentity(
            instance_id=content, material_id=material
        )
        project.pad_gain_db[sample_id] = -3.0
        project.pad_eq_low_db[sample_id] = 2.0
        project.pad_loop_start_s[sample_id] = 3.5
        project.pad_loop_end_s[sample_id] = 9.0
        project.pad_stem_mix_mode[sample_id] = "all_stems"
    project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(version=7, raw_key="Em"),
        correction="Cm",
        analysis_epoch=11,
        correction_epoch=13,
        base_shift=-4,
        extra_shift=12,
        retrigger=True,
    )
    project.pad_key_intent[215] = PadKeyIntent(
        source=SourceKeyVersion(version=MAX_KEY_EPOCH, raw_key="Bb"),
        correction="unknown surviving text",
        analysis_epoch=MAX_KEY_EPOCH,
        correction_epoch=17,
        base_shift=6,
        extra_shift=-18,
        retrigger=False,
    )
    return project


def _write_config(tmp_path: Path, data: dict[str, object]) -> Path:
    config = tmp_path / PROJECT_CONFIG_PATH
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    return config


def _non_key_settings(project: ProjectState) -> dict[str, object]:
    return project.model_dump(mode="json", exclude={"pad_key_intent", "manual_key"})


def test_actual_json_roundtrip_retains_independent_endpoint_content_intent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    project = _performer_project()
    persistence = ProjectPersistence(project)
    persistence.mark_dirty()
    persistence.flush(now=0.0)

    raw = json.loads(PROJECT_CONFIG_PATH.read_text(encoding="utf-8"))
    assert raw["pad_key_intent"][0] == project.pad_key_intent[0].model_dump(mode="json")
    assert raw["pad_key_intent"][215] == project.pad_key_intent[215].model_dump(mode="json")
    assert raw["manual_key"][0] == "Cm"
    assert raw["manual_key"][215] == "unknown surviving text"
    reopened = ProjectPersistence.from_config_path().project
    assert reopened.pad_key_intent == project.pad_key_intent
    assert reopened.pad_content == project.pad_content
    assert reopened.pad_content[0] != reopened.pad_content[215]
    assert reopened.sample_paths[0] == reopened.sample_paths[215]
    assert reopened.pad_key_intent[0] is not project.pad_key_intent[0]
    assert reopened.pad_key_intent[0].source is not project.pad_key_intent[0].source
    with pytest.raises(ValidationError, match="frozen"):
        # Exercise the frozen DTO's runtime rejection in addition to static typing.
        reopened.pad_key_intent[0].extra_shift = 0  # type: ignore[misc]
    reopened.pad_key_intent[0] = reopened.pad_key_intent[0].corrected(None).changed(extra_shift=0)
    assert reopened.pad_key_intent[215] == project.pad_key_intent[215]
    assert project.pad_key_intent[0].correction == "Cm"
    assert project.pad_key_intent[0].extra_shift == 12


def test_saved_copy_survives_removing_origin_assignment(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    project = _performer_project()
    survivor_intent = project.pad_key_intent[215]
    survivor_identity = project.pad_content[215]
    project.sample_paths[0] = None
    project.pad_content[0] = None
    project.pad_key_intent[0] = PadKeyIntent()
    ProjectPersistence(project).flush(now=0.0)

    loaded = ProjectPersistence.from_config_path().project
    assert loaded.sample_paths[0] is None
    assert loaded.pad_content[0] is None
    assert loaded.pad_key_intent[0] == PadKeyIntent()
    assert loaded.sample_paths[215] == project.sample_paths[215]
    assert loaded.pad_content[215] == survivor_identity
    assert loaded.pad_key_intent[215] == survivor_intent
    assert loaded.pad_gain_db[215] == -3.0
    assert loaded.pad_loop_start_s[215] == 3.5
    assert loaded.pad_loop_end_s[215] == 9.0


@pytest.mark.parametrize("length", [215, 217])
def test_direct_fixed_array_rejection_retains_existing_live_assignment(length: int) -> None:
    project = _performer_project()
    previous = project.pad_key_intent
    bad = [PadKeyIntent()] * length
    with pytest.raises(ValidationError):
        ProjectState.model_validate({"pad_key_intent": bad})
    with pytest.raises(ValidationError):
        project.pad_key_intent = bad
    assert project.pad_key_intent is previous
    assert project.pad_key_intent[0].correction == "Cm"
    assert project.pad_content[0] is not None


@pytest.mark.parametrize(
    ("bad", "neutral"),
    [
        ({"source": {"version": True, "raw_key": "Em"}}, {"source": None}),
        ({"source": {"version": 8}}, {"source": None}),
        ({"analysis_epoch": -1}, {"analysis_epoch": 0}),
        ({"correction_epoch": MAX_KEY_EPOCH + 1}, {"correction_epoch": 0}),
        ({"base_shift": True}, {"base_shift": 0}),
        ({"extra_shift": 19}, {"extra_shift": 0}),
        ({"retrigger": "true"}, {"retrigger": False}),
        ({"correction": 12}, {"correction": None}),
        (
            {
                "source": {"version": -1, "raw_key": "Em"},
                "analysis_epoch": True,
                "extra_shift": False,
            },
            {"source": None, "analysis_epoch": 0, "extra_shift": 0},
        ),
    ],
)
def test_saved_bad_key_field_recovers_locally_without_erasing_other_performer_choices(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    bad: dict[str, object],
    neutral: dict[str, object],
) -> None:
    monkeypatch.chdir(tmp_path)
    original = _performer_project()
    data = original.model_dump(mode="json")
    table = cast("list[dict[str, object]]", data["pad_key_intent"])
    table[0].update(bad)
    config = _write_config(tmp_path, data)
    with pytest.raises(ValidationError):
        ProjectState.model_validate_json(config.read_text(encoding="utf-8"))

    loaded = ProjectPersistence.from_config_path(config).project

    assert loaded.pad_key_intent[0] == original.pad_key_intent[0].changed(**neutral)
    assert loaded.pad_key_intent[1:] == original.pad_key_intent[1:]
    assert _non_key_settings(loaded) == _non_key_settings(original)
    assert loaded.manual_key[0] == loaded.pad_key_intent[0].correction
    # A real subsequent write/reopen must persist the recovered authoritative value.
    ProjectPersistence(loaded).flush(now=0.0)
    reopened = ProjectPersistence.from_config_path(config).project
    assert reopened.pad_key_intent == loaded.pad_key_intent


@pytest.mark.parametrize("bad", [None, "unsupported", 42, [], {"base_shift": 2}])
def test_malformed_new_table_uses_bounded_defaults_without_legacy_resurrection(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, bad: object
) -> None:
    monkeypatch.chdir(tmp_path)
    original = _performer_project()
    data = original.model_dump(mode="json")
    data["pad_key_intent"] = bad
    data["manual_key"] = ["Am"] * NUM_SAMPLES
    config = _write_config(tmp_path, data)

    loaded = ProjectPersistence.from_config_path(config).project

    assert len(loaded.pad_key_intent) == NUM_SAMPLES
    assert loaded.pad_key_intent == [PadKeyIntent()] * NUM_SAMPLES
    assert list(loaded.manual_key) == [None] * NUM_SAMPLES
    assert _non_key_settings(loaded) == _non_key_settings(original)


def test_malformed_entries_and_oversized_table_preserve_every_valid_known_slot(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    original = _performer_project()
    data = original.model_dump(mode="json")
    table = cast("list[object]", data["pad_key_intent"])
    table[1] = "unsupported entry"
    table[2] = None
    table[3] = {"extra_shift": -7, "correction": None, "analysis_epoch": "bad"}
    table.append({"correction": "must not become a new pad", "retrigger": True})
    config = _write_config(tmp_path, data)

    loaded = ProjectPersistence.from_config_path(config).project

    assert len(loaded.pad_key_intent) == NUM_SAMPLES
    assert loaded.pad_key_intent[0] == original.pad_key_intent[0]
    assert loaded.pad_key_intent[1] == loaded.pad_key_intent[2] == PadKeyIntent()
    assert loaded.pad_key_intent[3] == PadKeyIntent(extra_shift=-7)
    assert loaded.pad_key_intent[4:] == original.pad_key_intent[4:]
    assert _non_key_settings(loaded) == _non_key_settings(original)


def test_short_saved_table_preserves_existing_entries_and_defaults_only_missing_pad(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    original = _performer_project()
    data = original.model_dump(mode="json")
    table = cast("list[object]", data["pad_key_intent"])
    table.pop()
    config = _write_config(tmp_path, data)
    loaded = ProjectPersistence.from_config_path(config).project
    assert loaded.pad_key_intent[:215] == original.pad_key_intent[:215]
    assert loaded.pad_key_intent[215] == PadKeyIntent()
    assert loaded.manual_key[215] is None
    assert _non_key_settings(loaded) == _non_key_settings(original)


def test_actual_legacy_load_migrates_raw_metadata_with_neutral_pitch(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    original = _performer_project()
    data = original.model_dump(mode="json", exclude={"pad_key_intent"})
    corrections: list[str | None] = [None] * NUM_SAMPLES
    corrections[0] = "arbitrary legacy ♭ metadata"
    corrections[215] = "Bbm"
    data["manual_key"] = corrections
    analysis = SampleAnalysis(
        bpm=123.0, key="Em", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[])
    )
    analyses = cast("list[object]", data["sample_analysis"])
    analyses[0] = analysis.model_dump(mode="json")
    config = _write_config(tmp_path, data)

    loaded = ProjectPersistence.from_config_path(config).project

    assert loaded.pad_key_intent[0] == PadKeyIntent(
        source=SourceKeyVersion(version=0, raw_key="Em"), correction=corrections[0]
    )
    assert loaded.pad_key_intent[215] == PadKeyIntent(correction="Bbm")
    assert all(intent.base_shift == intent.extra_shift == 0 for intent in loaded.pad_key_intent)
    assert all(not intent.retrigger for intent in loaded.pad_key_intent)
    assert loaded.sample_analysis[0] == analysis
    assert loaded.pad_content == original.pad_content
    assert loaded.pad_gain_db == original.pad_gain_db
    ProjectPersistence(loaded).flush(now=0.0)
    reopened = ProjectPersistence.from_config_path(config).project
    assert reopened.pad_key_intent == loaded.pad_key_intent


def test_removed_new_correction_stays_removed_through_actual_repeated_flush_and_load(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    project = _performer_project()
    project.pad_key_intent[0] = project.pad_key_intent[0].corrected(None)
    expected = project.pad_key_intent[0]
    data = project.model_dump(mode="json")
    data["manual_key"] = ["stale old correction"] * NUM_SAMPLES
    config = _write_config(tmp_path, data)
    for now in range(3):
        persistence = ProjectPersistence.from_config_path(config)
        assert persistence.project.pad_key_intent[0] == expected
        assert persistence.project.manual_key[0] is None
        persistence.flush(now=float(now))
        saved = json.loads(config.read_text(encoding="utf-8"))
        assert saved["pad_key_intent"][0]["correction"] is None
        assert saved["manual_key"][0] is None


def test_key_only_recovery_does_not_bypass_unrelated_invalid_project_settings(
    tmp_path: Path,
) -> None:
    data = _performer_project().model_dump(mode="json")
    table = cast("list[dict[str, object]]", data["pad_key_intent"])
    table[0]["base_shift"] = True
    data["volume"] = 99
    config = _write_config(tmp_path, data)
    assert ProjectPersistence.from_config_path(config).project == ProjectState()


def test_unknown_runtime_tokens_never_restore_permissions_or_start_native_work(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    native = Mock(side_effect=AssertionError("Metadata loading must not construct native audio"))
    monkeypatch.setattr("flitzis_looper.controller.app.AudioEngine", native)
    project = _performer_project()
    data = project.model_dump(mode="json")
    table = cast("list[dict[str, object]]", data["pad_key_intent"])
    table[0].update({
        "native_source_ticket": "historical",
        "hold_token": 7,
        "accepted": True,
        "analysis_request_id": 19,
    })
    config = _write_config(tmp_path, data)

    loaded = ProjectPersistence.from_config_path(config).project
    assert loaded.pad_key_intent == project.pad_key_intent
    ProjectPersistence(loaded).flush(now=0.0)
    saved = json.loads(config.read_text(encoding="utf-8"))
    assert set(saved["pad_key_intent"][0]) == set(PadKeyIntent.model_fields)
    assert set(saved["pad_key_intent"][0]["source"]) == {"version", "raw_key"}
    native.assert_not_called()
