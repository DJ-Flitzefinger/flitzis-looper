import wave
from pathlib import Path
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.persistence import PROJECT_CONFIG_PATH, ProjectPersistence, load_project_state


def test_load_project_state_missing_returns_defaults(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    assert tmp_path == Path(tmp_path)
    assert load_project_state() == ProjectState()


def test_persistence_roundtrip_writes_atomic_json(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)

    samples_dir = tmp_path / "samples"
    samples_dir.mkdir(parents=True)

    wav_path = samples_dir / "foo.wav"
    with wave.open(str(wav_path), "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(48_000)
        f.writeframes(b"\x00\x00" * 48)

    project = ProjectState(volume=0.5)
    project.sample_paths[0] = str(wav_path)

    persistence = ProjectPersistence(project)
    persistence.mark_dirty()
    persistence.flush(now=0.0)

    loaded = load_project_state(PROJECT_CONFIG_PATH)
    assert loaded.volume == pytest.approx(0.5)
    assert loaded.sample_paths[0] == "samples/foo.wav"


def test_debounced_flush_limits_writes(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(tmp_path)

    project = ProjectState(volume=0.1)
    persistence = ProjectPersistence(project)

    persistence.mark_dirty()
    assert persistence.maybe_flush(now=0.0) is True

    first_text = PROJECT_CONFIG_PATH.read_text(encoding="utf-8")

    project.volume = 0.2
    persistence.mark_dirty()
    assert persistence.maybe_flush(now=5.0) is False

    assert PROJECT_CONFIG_PATH.read_text(encoding="utf-8") == first_text

    assert persistence.maybe_flush(now=11.0) is True
    assert PROJECT_CONFIG_PATH.read_text(encoding="utf-8") != first_text


def test_load_project_state_invalid_json_returns_defaults(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)

    config_path = tmp_path / PROJECT_CONFIG_PATH
    config_path.parent.mkdir(parents=True)
    config_path.write_text("{not json}", encoding="utf-8")

    assert load_project_state(PROJECT_CONFIG_PATH) == ProjectState()


def test_restore_ignores_missing_or_mismatched_cached_wav(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)

    (tmp_path / "samples").mkdir(parents=True)

    wav_path = tmp_path / "samples" / "bad_rate.wav"
    with wave.open(str(wav_path), "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(44_100)
        f.writeframes(b"\x00\x00" * 48)

    project = ProjectState()
    project.sample_paths[0] = "samples/bad_rate.wav"

    session = SessionState()

    audio = Mock()
    audio.output_sample_rate.return_value = 48_000

    on_project_changed = Mock()
    on_pad_bpm_changed = Mock()

    loader = LoaderController(
        project,
        session,
        audio,
        on_pad_bpm_changed,
        on_project_changed=on_project_changed,
    )

    loader.restore_samples_from_project_state()

    assert project.sample_paths[0] is None
    assert project.sample_analysis[0] is None
    audio.load_sample_async.assert_not_called()
    on_pad_bpm_changed.assert_called_with(0)
    on_project_changed.assert_called()

    project.sample_paths[1] = "samples/missing.wav"
    loader.restore_samples_from_project_state()
    assert project.sample_paths[1] is None
