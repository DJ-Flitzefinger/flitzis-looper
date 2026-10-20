"""Real Config recovery isolates recording preferences from performer intent and fences."""

import json
import sys
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.controller.recording_settings_persistence import recover_recording_settings_json
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity, ProjectState, StemCacheEntry
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.stem_intent import PadStemIntent

if TYPE_CHECKING:
    from pathlib import Path


def _preferences() -> RecordingOptions:
    return RecordingOptions(
        format="mp3",
        wav_encoding="pcm24",
        flac_bit_depth=16,
        flac_compression=2,
        mp3_bitrate_kbps=192,
    )


def _project() -> ProjectState:
    project = ProjectState(
        selected_pad=215,
        selected_bank=5,
        volume=0.63,
        stem_separator="bs-roformer:musdb18hq",
        demucs_shifts=4,
        recording_options=_preferences(),
    )
    material = "a" * 32
    for sample_id, instance in [(0, "b" * 32), (215, "c" * 32)]:
        project.sample_paths[sample_id] = f"samples/materials/M{material}/original/Shared.wav"
        project.pad_content[sample_id] = PadContentIdentity(
            instance_id=instance, material_id=material
        )
        project.sample_durations[sample_id] = 12.0
        project.pad_gain_db[sample_id] = -7.0
        project.pad_eq_low_db[sample_id] = 2.0
        project.pad_eq_high_db[sample_id] = -3.0
        project.pad_loop_start_s[sample_id] = 3.5
        project.pad_loop_end_s[sample_id] = 9.0
        project.pad_loop_auto[sample_id] = False
        project.pad_key_lock[sample_id] = sample_id == 0
    project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(version=7, raw_key="Em"),
        correction="Cm",
        base_shift=-4,
        extra_shift=12,
        retrigger=True,
    )
    project.pad_key_intent[215] = PadKeyIntent(
        source=SourceKeyVersion(version=11, raw_key="Bb"), extra_shift=-7
    )
    project.pad_stem_intent[215] = PadStemIntent(
        enabled_mask=14, last_custom_mask=3, display_mode="instrumental"
    )
    project.stem_cache[215] = StemCacheEntry(
        source_version="surviving-source", cache_dir="surviving-set", available=True
    )
    return project


def _write_config(tmp_path: Path, data: dict[str, object]) -> Path:
    config = tmp_path / "samples" / "flitzis_looper.config.json"
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(json.dumps(data, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    return config


def _unrelated(project: ProjectState) -> dict[str, object]:
    return project.model_dump(mode="json", exclude={"recording_options"})


def test_new_missing_config_uses_recording_defaults_without_a_recovery_warning(
    tmp_path: Path,
) -> None:
    config = tmp_path / "new-project.json"
    loaded = ProjectPersistence.from_config_path(config)

    assert loaded.project.recording_options == RecordingOptions()
    assert loaded.recording_settings_warning is None
    assert loaded.load_error is None
    assert not config.exists()


@pytest.mark.parametrize("format_name", ["wav", "flac", "mp3"])
def test_real_config_save_and_reopen_preserves_all_format_preferences_and_performer_intent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, format_name: str
) -> None:
    monkeypatch.chdir(tmp_path)
    project = _project()
    options = _preferences().model_dump(mode="json") | {"format": format_name}
    project.recording_options = RecordingOptions.model_validate(options, strict=True)
    persistence = ProjectPersistence(project)
    persistence.mark_dirty()
    persistence.flush(now=0.0)

    loaded = ProjectPersistence.from_config_path()
    assert loaded.project.recording_options == project.recording_options
    assert loaded.project.recording_options is not project.recording_options
    assert _unrelated(loaded.project) == _unrelated(project)
    assert loaded.recording_settings_warning is None
    assert loaded.load_error is None
    assert loaded.save_error is None


def test_existing_legacy_config_missing_recording_options_warns_without_erasing_pads(
    tmp_path: Path,
) -> None:
    original = _project()
    data = original.model_dump(mode="json")
    del data["recording_options"]
    config = _write_config(tmp_path, data)
    encoded = config.read_bytes()

    loaded = ProjectPersistence.from_config_path(config)

    assert loaded.project.recording_options == RecordingOptions()
    assert loaded.recording_settings_warning is not None
    assert _unrelated(loaded.project) == _unrelated(original)
    assert loaded.load_error is None
    assert config.read_bytes() == encoded


@pytest.mark.parametrize("malformed", [None, True, 42, [], "wav"])
def test_malformed_recording_payload_defaults_only_recording_settings(
    tmp_path: Path, malformed: object
) -> None:
    original = _project()
    data = original.model_dump(mode="json")
    data["recording_options"] = malformed
    config = _write_config(tmp_path, data)
    encoded = config.read_bytes()

    loaded = ProjectPersistence.from_config_path(config)

    assert loaded.project.recording_options == RecordingOptions()
    assert loaded.recording_settings_warning is not None
    assert loaded.load_error is None
    assert _unrelated(loaded.project) == _unrelated(original)
    assert config.read_bytes() == encoded


@pytest.mark.parametrize(
    "field",
    ["format", "wav_encoding", "flac_bit_depth", "flac_compression", "mp3_bitrate_kbps"],
)
def test_missing_nested_field_defaults_only_that_preference(tmp_path: Path, field: str) -> None:
    original = _project()
    data = original.model_dump(mode="json")
    values = cast("dict[str, object]", data["recording_options"])
    del values[field]
    config = _write_config(tmp_path, data)

    loaded = ProjectPersistence.from_config_path(config)

    expected = _preferences().model_dump(mode="json")
    expected[field] = RecordingOptions().model_dump(mode="json")[field]
    assert loaded.project.recording_options.model_dump(mode="json") == expected
    assert loaded.recording_settings_warning is not None
    assert {
        "format": "recording format",
        "wav_encoding": "WAV precision",
        "flac_bit_depth": "FLAC precision",
        "flac_compression": "FLAC compression",
        "mp3_bitrate_kbps": "MP3 bitrate",
    }[field] in loaded.recording_settings_warning
    assert loaded.load_error is None
    assert _unrelated(loaded.project) == _unrelated(original)


@pytest.mark.parametrize(
    ("field", "bad"),
    [
        ("format", "aac"),
        ("format", True),
        ("wav_encoding", "pcm32"),
        ("wav_encoding", False),
        ("flac_bit_depth", True),
        ("flac_bit_depth", 16.0),
        ("flac_bit_depth", "16"),
        ("flac_bit_depth", 32),
        ("flac_compression", False),
        ("flac_compression", 8.0),
        ("flac_compression", "8"),
        ("flac_compression", -1),
        ("flac_compression", 9),
        ("mp3_bitrate_kbps", True),
        ("mp3_bitrate_kbps", 320.0),
        ("mp3_bitrate_kbps", "320"),
        ("mp3_bitrate_kbps", 64),
    ],
)
def test_invalid_or_coercible_nested_preference_never_rewrites_valid_siblings(
    tmp_path: Path, field: str, bad: object
) -> None:
    original = _project()
    data = original.model_dump(mode="json")
    values = cast("dict[str, object]", data["recording_options"])
    values[field] = bad
    config = _write_config(tmp_path, data)

    loaded = ProjectPersistence.from_config_path(config)

    expected = _preferences().model_dump(mode="json")
    expected[field] = RecordingOptions().model_dump(mode="json")[field]
    assert loaded.project.recording_options.model_dump(mode="json") == expected
    assert loaded.recording_settings_warning is not None
    assert {
        "format": "recording format",
        "wav_encoding": "WAV precision",
        "flac_bit_depth": "FLAC precision",
        "flac_compression": "FLAC compression",
        "mp3_bitrate_kbps": "MP3 bitrate",
    }[field] in loaded.recording_settings_warning
    assert loaded.load_error is None
    assert _unrelated(loaded.project) == _unrelated(original)


def test_unknown_nested_fields_warn_and_preserve_every_known_preference(tmp_path: Path) -> None:
    original = _project()
    data = original.model_dump(mode="json")
    values = cast("dict[str, object]", data["recording_options"])
    values["future_encoder"] = {"format": "unsupported", "gain": 0.0}
    config = _write_config(tmp_path, data)

    loaded = ProjectPersistence.from_config_path(config)

    assert loaded.project.recording_options == original.recording_options
    assert loaded.recording_settings_warning is not None
    assert "unknown recording fields" in loaded.recording_settings_warning
    assert _unrelated(loaded.project) == _unrelated(original)
    assert loaded.load_error is None


def test_recording_warning_allows_actual_save_and_reopens_as_valid_settings(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    original = _project()
    data = original.model_dump(mode="json")
    values = cast("dict[str, object]", data["recording_options"])
    values["flac_compression"] = True
    config = _write_config(tmp_path, data)
    loaded = ProjectPersistence.from_config_path(config)
    loaded.config_path = config
    assert loaded.recording_settings_warning is not None
    assert loaded.load_error is None
    assert _unrelated(loaded.project) == _unrelated(original)

    loaded.project.volume = 0.42
    loaded.mark_dirty()
    assert loaded.maybe_flush(now=0.0)

    reopened = ProjectPersistence.from_config_path(config)
    assert reopened.recording_settings_warning is None
    assert reopened.load_error is None
    assert reopened.project.recording_options == loaded.project.recording_options
    assert reopened.project.recording_options.flac_compression == 8
    assert reopened.project.recording_options.mp3_bitrate_kbps == 192
    assert reopened.project.volume == 0.42
    assert reopened.project.pad_content == original.pad_content
    assert reopened.project.pad_key_intent == original.pad_key_intent
    assert reopened.project.pad_gain_db == original.pad_gain_db
    assert reopened.project.pad_loop_start_s == original.pad_loop_start_s
    assert reopened.project.pad_loop_end_s == original.pad_loop_end_s
    assert loaded.save_error is None


@pytest.mark.parametrize(
    "unsafe", ["migration", "revision", "stem_pair", "stem_selection", "migration_and_stem_pair"]
)
def test_recording_recovery_preserves_existing_unsafe_metadata_fences(
    tmp_path: Path, unsafe: str
) -> None:
    original = _project()
    data = original.model_dump(mode="json")
    values = cast("dict[str, object]", data["recording_options"])
    values["wav_encoding"] = "unsupported"
    if unsafe in {"migration", "migration_and_stem_pair"}:
        data["material_migrations"] = {"1" * 32: {"ack": "historic fake permission"}}
    if unsafe == "revision":
        data["config_revision"] = True
    if unsafe in {"stem_pair", "migration_and_stem_pair"}:
        entries = cast("list[object]", data["stem_cache"])
        entries[0] = {
            "source_version": "invalid-pair-source",
            "cache_dir": "invalid-pair-set",
            "available": True,
            "pair": {"schema": "unsupported"},
        }
    if unsafe == "stem_selection":
        intents = cast("list[object]", data["pad_stem_intent"])
        intents[0] = {"schema_version": 2}
    config = _write_config(tmp_path, data)
    encoded = config.read_bytes()

    loaded = ProjectPersistence.from_config_path(config)
    loaded.config_path = config

    assert loaded.recording_settings_warning is not None
    assert loaded.project.recording_options.wav_encoding == "float32"
    assert loaded.project.recording_options.format == "mp3"
    assert loaded.project.recording_options.flac_bit_depth == 16
    assert loaded.project.recording_options.mp3_bitrate_kbps == 192
    assert loaded.load_error is not None
    assert loaded.project.model_dump(mode="json", exclude={"recording_options", "stem_cache"}) == (
        original.model_dump(mode="json", exclude={"recording_options", "stem_cache"})
    )
    assert loaded.project.stem_cache[215] == original.stem_cache[215]
    if unsafe in {"stem_pair", "migration_and_stem_pair"}:
        assert loaded.invalid_stem_pair_ids == {0}
        entry = loaded.project.stem_cache[0]
        assert entry is not None
        assert entry.pair is None
        assert entry.available is False
    else:
        assert not loaded.invalid_stem_pair_ids
    loaded.mark_dirty()
    with pytest.raises(PersistenceFenceError):
        loaded.flush(now=0.0)
    assert config.read_bytes() == encoded


def test_recording_warning_survives_other_selective_recovery_without_creating_a_fence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    original = _project()
    data = original.model_dump(mode="json")
    values = cast("dict[str, object]", data["recording_options"])
    values["mp3_bitrate_kbps"] = "320"
    keys = cast("list[dict[str, object]]", data["pad_key_intent"])
    keys[0]["extra_shift"] = True
    config = _write_config(tmp_path, data)

    loaded = ProjectPersistence.from_config_path(config)
    loaded.config_path = config

    assert loaded.recording_settings_warning is not None
    assert loaded.project.recording_options.mp3_bitrate_kbps == 320
    assert loaded.project.recording_options.flac_compression == 2
    assert loaded.project.pad_key_intent[0].extra_shift == 0
    assert loaded.project.pad_key_intent[0].source == original.pad_key_intent[0].source
    assert loaded.project.pad_key_intent[215] == original.pad_key_intent[215]
    assert loaded.project.pad_content == original.pad_content
    assert loaded.project.pad_gain_db == original.pad_gain_db
    assert loaded.load_error is None
    loaded.mark_dirty()
    loaded.flush(now=0.0)
    reopened = ProjectPersistence.from_config_path(config)
    assert reopened.recording_settings_warning is None
    assert reopened.project.recording_options == loaded.project.recording_options
    assert reopened.project.pad_key_intent == loaded.project.pad_key_intent


def test_real_saved_overlimit_integer_recovers_only_its_recording_preference(
    tmp_path: Path,
) -> None:
    original = _project()
    data = original.model_dump(mode="json")
    values = cast("dict[str, object]", data["recording_options"])
    marker = "RECORDING_INTEGER_LIMIT_TOKEN"
    values["flac_compression"] = marker
    # Exercise CPython's real numeric-token limit, without mocking a decoder or
    # trying to construct the disallowed integer in the test itself. Restore the
    # process setting even when the original regression raises during load.
    previous_limit = sys.get_int_max_str_digits()
    limit = sys.int_info.str_digits_check_threshold
    sys.set_int_max_str_digits(limit)
    try:
        raw = json.dumps(data).replace(json.dumps(marker), "7" * (limit + 1))
        config = tmp_path / "overlimit-recording.config.json"
        config.write_text(raw, encoding="utf-8", newline="\n")
        with pytest.raises(ValueError, match="integer string conversion"):
            json.loads(config.read_text(encoding="utf-8"))

        loaded = ProjectPersistence.from_config_path(config)

        expected = _preferences().model_dump(mode="json") | {"flac_compression": 8}
        assert loaded.project.recording_options.model_dump(mode="json") == expected
        assert loaded.recording_settings_warning is not None
        assert loaded.load_error is None
        assert _unrelated(loaded.project) == _unrelated(original)
        assert config.read_text(encoding="utf-8") == raw
    finally:
        sys.set_int_max_str_digits(previous_limit)


@pytest.mark.parametrize("depth", [300, 10_000])
def test_actual_deep_non_project_json_keeps_invalid_shape_default_behavior(
    tmp_path: Path, depth: int
) -> None:
    # A whole-root array is not a recoverable recording settings object. The
    # established invalid-shape result is a default project, not selective repair.
    # The current CPython scanner accepts these actual depths independently of
    # sys.getrecursionlimit(); this proves supported deep fallback, not an
    # observed RecursionError or the defensive exception-containment branch.
    raw = "[" * depth + "0" + "]" * depth
    config = tmp_path / "deep-non-project.config.json"
    config.write_text(raw, encoding="utf-8", newline="\n")
    assert isinstance(json.loads(config.read_text(encoding="utf-8")), list)

    loaded = ProjectPersistence.from_config_path(config)

    assert loaded.project == ProjectState()
    assert loaded.recording_settings_warning is None
    assert loaded.load_error is None
    assert loaded.save_error is None
    assert config.read_text(encoding="utf-8") == raw


def test_overlimit_numeric_source_path_keeps_original_validation_instead_of_becoming_text(
    tmp_path: Path,
) -> None:
    data = _project().model_dump(mode="json")
    del data["recording_options"]
    marker = "UNRELATED_INTEGER_LIMIT_TOKEN"
    paths = cast("list[object]", data["sample_paths"])
    paths[0] = marker
    previous_limit = sys.get_int_max_str_digits()
    limit = sys.int_info.str_digits_check_threshold
    sys.set_int_max_str_digits(limit)
    try:
        raw = json.dumps(data).replace(json.dumps(marker), "7" * (limit + 1))
        config = tmp_path / "outside-recording-integer.config.json"
        config.write_text(raw, encoding="utf-8", newline="\n")
        with pytest.raises(ValueError, match="integer string conversion"):
            json.loads(config.read_text(encoding="utf-8"))

        # Missing recording settings would normally request a JSON rewrite. An
        # unsupported numeric token outside them must instead retain the exact
        # raw input and its original normal project validation result.
        assert recover_recording_settings_json(raw) == (raw, None)
        loaded = ProjectPersistence.from_config_path(config)

        assert loaded.project == ProjectState()
        assert loaded.project.sample_paths[0] is None
        assert loaded.recording_settings_warning is None
        assert loaded.load_error is None
        assert config.read_text(encoding="utf-8") == raw
    finally:
        sys.set_int_max_str_digits(previous_limit)


def test_overlimit_recording_integer_retains_actual_unsafe_alias_save_fence(tmp_path: Path) -> None:
    original = _project()
    data = original.model_dump(mode="json")
    values = cast("dict[str, object]", data["recording_options"])
    marker = "FENCED_RECORDING_INTEGER_TOKEN"
    values["flac_compression"] = marker
    data["material_migrations"] = {"1" * 32: {"ack": "historic fake permission"}}
    previous_limit = sys.get_int_max_str_digits()
    limit = sys.int_info.str_digits_check_threshold
    sys.set_int_max_str_digits(limit)
    try:
        raw = json.dumps(data).replace(json.dumps(marker), "7" * (limit + 1))
        config = tmp_path / "overlimit-recording-unsafe-alias.config.json"
        config.write_text(raw, encoding="utf-8", newline="\n")
        with pytest.raises(ValueError, match="integer string conversion"):
            json.loads(config.read_text(encoding="utf-8"))

        loaded = ProjectPersistence.from_config_path(config)
        loaded.config_path = config

        expected = _preferences().model_dump(mode="json") | {"flac_compression": 8}
        assert loaded.project.recording_options.model_dump(mode="json") == expected
        assert loaded.recording_settings_warning is not None
        assert _unrelated(loaded.project) == _unrelated(original)
        assert loaded.load_error == "Unsupported migration metadata retained on disk"
        loaded.mark_dirty()
        with pytest.raises(PersistenceFenceError):
            loaded.flush(now=0.0)
        assert config.read_text(encoding="utf-8") == raw
    finally:
        sys.set_int_max_str_digits(previous_limit)


def test_deep_unknown_legacy_payload_preserves_known_intent_through_recording_rewrite(
    tmp_path: Path,
) -> None:
    original = _project()
    data = original.model_dump(mode="json")
    del data["recording_options"]
    marker = "DEEP_UNKNOWN_LEGACY_PAYLOAD"
    data["unknown_legacy_payload"] = marker
    nested = "[" * 10_000 + "0" + "]" * 10_000
    raw = json.dumps(data).replace(json.dumps(marker), nested)
    config = tmp_path / "deep-legacy-recording-defaults.config.json"
    config.write_text(raw, encoding="utf-8", newline="\n")
    # Current runtime supports both real operations at this bounded depth. No
    # injected exception or recursion-limit claim stands in for that boundary.
    decoded = json.loads(config.read_text(encoding="utf-8"))
    assert isinstance(decoded, dict)
    assert "unknown_legacy_payload" in json.dumps(decoded)

    repaired, warning = recover_recording_settings_json(raw)
    assert repaired != raw
    assert warning is not None
    assert isinstance(json.loads(repaired), dict)
    loaded = ProjectPersistence.from_config_path(config)

    assert loaded.project.recording_options == RecordingOptions()
    assert _unrelated(loaded.project) == _unrelated(original)
    assert loaded.recording_settings_warning is not None
    assert loaded.load_error is None
    assert loaded.save_error is None
    assert config.read_text(encoding="utf-8") == raw
