"""Audio preference recovery must not discard unrelated performer intent."""

import json
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

import pytest
from pydantic import ValidationError

from flitzis_looper.audio_output_options import AudioOutputOptions
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import ProjectState


@pytest.mark.parametrize("field", ["sample_rate_hz", "buffer_frames", "left_channel"])
def test_output_integer_preferences_refuse_boolean_coercion(field: str) -> None:
    with pytest.raises(ValidationError):
        AudioOutputOptions.model_validate({field: True})


def test_corrupt_output_preserves_project_and_original_request_until_apply(tmp_path: Path) -> None:
    path = tmp_path / "project.json"
    project = ProjectState(selected_pad=17, key_lock=True)
    project.manual_bpm[17] = 123.0
    data = project.model_dump(mode="json")
    invalid = {"backend": "asio", "device_id": "driver-id", "buffer_frames": "96"}
    data["audio_output"] = invalid
    path.write_text(json.dumps(data), encoding="utf-8")
    persistence = ProjectPersistence.from_config_path(path)
    persistence.config_path = path
    assert persistence.project.selected_pad == 17
    assert persistence.project.manual_bpm[17] == 123.0
    assert persistence.project.key_lock
    assert persistence.audio_output_recovery is not None
    assert persistence.audio_output_recovery.original == invalid
    persistence.project.selected_pad = 18
    persistence.mark_dirty()
    persistence.flush()
    assert json.loads(path.read_text(encoding="utf-8"))["audio_output"] == invalid
    options = AudioOutputOptions(
        backend="wasapi",
        sample_rate_hz=96_000,
        buffer_frames=96,
    )
    persistence.commit_audio_output(options)
    restored = ProjectPersistence.from_config_path(path)
    assert restored.audio_output_recovery is None
    assert restored.project.audio_output == options
    assert restored.project.selected_pad == 18
    assert restored.project.manual_bpm[17] == 123.0


def test_output_commit_failure_retains_previous_request_and_disk(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    persistence = ProjectPersistence()
    persistence.config_path = tmp_path / "project.json"
    persistence.flush()
    original = persistence.config_path.read_bytes()
    previous = persistence.project.audio_output

    def fail(_text: str, **_options: object) -> None:
        message = "injected config write failure"
        raise OSError(message)

    monkeypatch.setattr(persistence, "_atomic_write_text", fail)
    with pytest.raises(OSError, match="injected config write failure"):
        persistence.commit_audio_output(AudioOutputOptions(backend="asio", device_id="driver-id"))
    assert persistence.project.audio_output == previous
    assert persistence.config_path.read_bytes() == original


def test_saved_output_intent_is_not_replaced_by_observed_geometry(tmp_path: Path) -> None:
    options = AudioOutputOptions(
        backend="asio",
        device_id="driver-id",
        sample_rate_hz=None,
        buffer_frames=None,
        left_channel=4,
        right_channel=5,
    )
    persistence = ProjectPersistence(ProjectState(audio_output=options))
    persistence.config_path = tmp_path / "project.json"
    persistence.flush()
    assert (
        ProjectPersistence.from_config_path(persistence.config_path).project.audio_output == options
    )
