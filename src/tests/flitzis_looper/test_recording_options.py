"""Recording option contracts only; encoder readiness and files need separate proof."""

import json
from typing import Protocol, cast

import pytest
from pydantic import ValidationError

from flitzis_looper.models import ProjectState
from flitzis_looper.recording_options import RecordingOptions


class _UncheckedOptionsConstructor(Protocol):
    """Retain model_construct's arbitrary inputs for intentional invalid fixtures."""

    def __call__(self, **values: object) -> RecordingOptions: ...


def test_recording_defaults_are_explicit_and_round_trip_in_project_json() -> None:
    expected = {
        "format": "flac",
        "wav_encoding": "float32",
        "flac_bit_depth": 24,
        "flac_compression": 8,
        "mp3_bitrate_kbps": 320,
    }
    project = ProjectState()
    assert project.recording_options.model_dump(mode="json") == expected
    assert ProjectState.model_validate_json("{}").recording_options == RecordingOptions()
    assert json.loads(project.model_dump_json())["recording_options"] == expected
    assert ProjectState.model_validate_json(project.model_dump_json()).recording_options == (
        project.recording_options
    )


@pytest.mark.parametrize("selected_format", ["wav", "flac", "mp3"])
def test_inactive_format_choices_survive_validation_and_project_round_trip(
    selected_format: str,
) -> None:
    payload = {
        "format": selected_format,
        "wav_encoding": "pcm24",
        "flac_bit_depth": 16,
        "flac_compression": 2,
        "mp3_bitrate_kbps": 192,
    }
    options = RecordingOptions.model_validate(payload)
    project = ProjectState(recording_options=options, speed=1.25, volume=0.4)
    reopened = ProjectState.model_validate_json(project.model_dump_json())
    assert reopened.recording_options.model_dump(mode="json") == payload
    assert RecordingOptions.model_validate_json(options.model_dump_json()) == options
    assert reopened.speed == 1.25
    assert reopened.volume == 0.4


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("wav_encoding", "float32"),
        ("wav_encoding", "pcm16"),
        ("wav_encoding", "pcm24"),
        ("flac_bit_depth", 16),
        ("flac_bit_depth", 24),
        *(("flac_compression", compression) for compression in range(9)),
        ("mp3_bitrate_kbps", 128),
        ("mp3_bitrate_kbps", 192),
        ("mp3_bitrate_kbps", 256),
        ("mp3_bitrate_kbps", 320),
    ],
)
def test_supported_quality_choices_are_retained(field: str, value: object) -> None:
    options = RecordingOptions.model_validate({field: value})
    assert getattr(options, field) == value


@pytest.mark.parametrize(
    ("field", "invalid"),
    [
        ("format", "WAV"),
        ("format", "aac"),
        ("format", True),
        ("format", None),
        ("wav_encoding", "float64"),
        ("wav_encoding", 32),
        ("wav_encoding", None),
        ("flac_bit_depth", True),
        ("flac_bit_depth", False),
        ("flac_bit_depth", 16.0),
        ("flac_bit_depth", 24.0),
        ("flac_bit_depth", "16"),
        ("flac_bit_depth", "24"),
        ("flac_bit_depth", None),
        ("flac_bit_depth", 8),
        ("flac_bit_depth", 32),
        ("flac_compression", True),
        ("flac_compression", False),
        ("flac_compression", 0.0),
        ("flac_compression", 8.0),
        ("flac_compression", "8"),
        ("flac_compression", None),
        ("flac_compression", -1),
        ("flac_compression", 9),
        ("mp3_bitrate_kbps", True),
        ("mp3_bitrate_kbps", False),
        ("mp3_bitrate_kbps", 128.0),
        ("mp3_bitrate_kbps", 320.0),
        ("mp3_bitrate_kbps", "128"),
        ("mp3_bitrate_kbps", "320"),
        ("mp3_bitrate_kbps", None),
        ("mp3_bitrate_kbps", 160),
        ("mp3_bitrate_kbps", 384),
    ],
)
def test_invalid_choices_and_numeric_coercions_are_rejected_in_python_and_json(
    field: str, invalid: object
) -> None:
    payload = {field: invalid}
    with pytest.raises(ValidationError):
        RecordingOptions.model_validate(payload)
    with pytest.raises(ValidationError):
        RecordingOptions.model_validate_json(json.dumps(payload))


@pytest.mark.parametrize("extra", ["schema_version", "normalize", "sample_rate_hz"])
def test_unknown_recording_option_fields_are_rejected(extra: str) -> None:
    with pytest.raises(ValidationError, match="Extra inputs are not permitted"):
        RecordingOptions.model_validate({extra: 1})


@pytest.mark.parametrize(
    ("field", "value"),
    [("format", "mp3"), ("wav_encoding", "pcm16"), ("flac_compression", 0)],
)
def test_options_are_frozen(field: str, value: object) -> None:
    options = RecordingOptions()
    before = options.model_dump()
    with pytest.raises(ValidationError, match="frozen"):
        setattr(options, field, value)
    assert options.model_dump() == before


def test_unvalidated_instance_is_revalidated_at_model_and_project_boundaries() -> None:
    construct = cast("_UncheckedOptionsConstructor", RecordingOptions.model_construct)
    invalid = construct(flac_bit_depth=16.0)
    with pytest.raises(ValidationError):
        RecordingOptions.model_validate(invalid)
    with pytest.raises(ValidationError):
        ProjectState(recording_options=invalid)
    project = ProjectState()
    original = project.recording_options
    with pytest.raises(ValidationError):
        project.recording_options = invalid
    assert project.recording_options is original
