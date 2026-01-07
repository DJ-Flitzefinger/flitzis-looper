from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.persistence import PROJECT_CONFIG_PATH, ProjectPersistence
from flitzis_looper.models import BeatGrid, ProjectState, SampleAnalysis, SessionState
from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from pathlib import Path


def test_load_project_state_missing_returns_defaults(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    assert ProjectPersistence.from_config_path().project == ProjectState()


def test_persistence_roundtrip_writes_atomic_json(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)

    samples_dir = tmp_path / "samples"
    samples_dir.mkdir(parents=True)

    wav_path = samples_dir / "foo.wav"
    write_mono_pcm16_wav(wav_path, 48_000)

    project = ProjectState(volume=0.5)
    project.sample_paths[0] = str(wav_path)

    persistence = ProjectPersistence(project)
    persistence.mark_dirty()
    persistence.flush(now=0.0)

    loaded = ProjectPersistence.from_config_path().project
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
    loaded = ProjectPersistence.from_config_path().project

    assert loaded == ProjectState()


def test_restore_loads_valid_audio_files_without_reanalysis(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)

    (tmp_path / "samples").mkdir(parents=True)

    wav_path = tmp_path / "samples" / "sample.wav"
    write_mono_pcm16_wav(wav_path, 44_100)

    project = ProjectState()
    project.sample_paths[0] = "samples/sample.wav"
    project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0, 1.0], downbeats=[0.0]),
    )

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

    assert project.sample_paths[0] == "samples/sample.wav"
    assert project.sample_analysis[0] is not None
    audio.load_sample_async.assert_called_with(0, "samples/sample.wav", run_analysis=False)
    on_project_changed.assert_not_called()

    project.sample_paths[1] = "samples/missing.wav"
    loader.restore_samples_from_project_state()
    assert project.sample_paths[1] is None
