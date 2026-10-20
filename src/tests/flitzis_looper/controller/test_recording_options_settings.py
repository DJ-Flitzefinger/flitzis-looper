"""Recording defaults replace intent without driving Native or current takes."""

from typing import Protocol, cast
from unittest.mock import Mock

import pytest
from pydantic import ValidationError

from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.recording_options import RecordingOptions


class _UncheckedOptionsConstructor(Protocol):
    """Retain model_construct's arbitrary inputs for intentional invalid fixtures."""

    def __call__(self, **values: object) -> RecordingOptions: ...


def test_recording_settings_preserve_musical_and_running_state_without_audio_calls() -> None:
    project = ProjectState(
        multi_loop=True,
        key_lock=True,
        bpm_lock=True,
        speed=1.25,
        volume=0.4,
        stem_separator="bs-roformer:musdb18hq",
    )
    project.sample_paths[0] = "samples/retained.wav"
    project.pad_loop_start_s[0] = 3.0
    project.pad_loop_end_s[0] = 9.0
    project.pad_stem_mix_mode[0] = "all_stems"
    project.pad_gain_db[0] = -4.0
    session = SessionState(active_sample_ids={0, 36}, paused_sample_ids={36})
    session.stem_generating_sample_ids.add(215)
    audio, changed, policy = Mock(spec=[]), Mock(), Mock()
    settings = SettingsController(project, session, audio, on_project_changed=changed)
    settings.bind_residency_policy(policy)
    before = project.model_dump(exclude={"recording_options"})
    session_before = session.model_dump()
    producer_revision = project.stem_processing_revision
    options = RecordingOptions(
        format="flac",
        wav_encoding="pcm24",
        flac_bit_depth=16,
        flac_compression=3,
        mp3_bitrate_kbps=192,
    )

    settings.set_recording_options(options)

    assert project.recording_options == options
    assert project.model_dump(exclude={"recording_options"}) == before
    assert project.stem_processing_revision == producer_revision
    assert session.model_dump() == session_before
    changed.assert_called_once_with()
    policy.assert_not_called()
    assert audio.mock_calls == []


def test_unchanged_options_keep_identity_and_do_not_mark_dirty() -> None:
    project = ProjectState()
    audio, changed = Mock(spec=[]), Mock()
    settings = SettingsController(project, SessionState(), audio, on_project_changed=changed)
    original = project.recording_options
    equal_options = RecordingOptions.model_validate_json(original.model_dump_json())

    settings.set_recording_options(equal_options)
    assert project.recording_options is original
    changed.assert_not_called()

    settings.set_recording_options(RecordingOptions(format="mp3"))
    changed.assert_called_once_with()
    updated = project.recording_options
    settings.set_recording_options(RecordingOptions(format="mp3"))
    assert project.recording_options is updated
    changed.assert_called_once_with()
    assert audio.mock_calls == []


def test_take_snapshot_retains_original_immutable_options_after_default_replacement() -> None:
    project = ProjectState(
        recording_options=RecordingOptions(
            format="flac", wav_encoding="pcm16", flac_compression=2, mp3_bitrate_kbps=128
        )
    )
    audio, changed = Mock(spec=[]), Mock()
    settings = SettingsController(project, SessionState(), audio, on_project_changed=changed)
    take_options = project.recording_options
    captured = take_options.model_dump()
    future_options = RecordingOptions(
        format="mp3", wav_encoding="pcm24", flac_bit_depth=16, flac_compression=7
    )

    settings.set_recording_options(future_options)

    assert project.recording_options == future_options
    assert project.recording_options is not take_options
    assert take_options.model_dump() == captured
    assert take_options.format == "flac"
    assert take_options.wav_encoding == "pcm16"
    assert take_options.flac_bit_depth == 24
    assert take_options.flac_compression == 2
    assert take_options.mp3_bitrate_kbps == 128
    changed.assert_called_once_with()
    assert audio.mock_calls == []


@pytest.mark.parametrize(
    ("field", "invalid"),
    [
        ("format", "aac"),
        ("flac_bit_depth", 16.0),
        ("flac_compression", True),
        ("mp3_bitrate_kbps", 320.0),
        ("mp3_bitrate_kbps", "320"),
    ],
)
def test_settings_revalidate_unchecked_instances_before_mutation_or_dirtying(
    field: str, invalid: object
) -> None:
    project = ProjectState()
    session = SessionState(active_sample_ids={0})
    audio, changed = Mock(spec=[]), Mock()
    settings = SettingsController(project, session, audio, on_project_changed=changed)
    original = project.recording_options
    before = project.model_dump()
    session_before = session.model_dump()
    construct = cast("_UncheckedOptionsConstructor", RecordingOptions.model_construct)
    unchecked = construct(**{field: invalid})

    with pytest.raises(ValidationError):
        settings.set_recording_options(unchecked)

    assert project.recording_options is original
    assert project.model_dump() == before
    assert session.model_dump() == session_before
    changed.assert_not_called()
    assert audio.mock_calls == []
