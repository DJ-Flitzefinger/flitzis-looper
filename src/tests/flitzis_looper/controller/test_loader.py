from typing import TYPE_CHECKING

from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper_audio import AudioEngine
from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from pathlib import Path

    import pytest


def test_restore_sample_does_not_copy_file(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(tmp_path)

    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_filepath = samples_dir / "test.wav"
    write_mono_pcm16_wav(sample_filepath, 44_100)

    project = ProjectState()
    session = SessionState()
    project.sample_paths[0] = "samples/test.wav"

    audio = AudioEngine()
    audio.run()

    try:
        loader = LoaderController(
            project=project,
            session=session,
            audio=audio,
            on_pad_bpm_changed=lambda _: None,
        )
        loader.restore_samples_from_project_state()
        while loader.is_sample_loading(0):
            loader.poll_loader_events()

        assert loader.is_sample_loaded(0)
        sample_files = list(samples_dir.glob("*"))
        assert len(sample_files) == 1
        assert sample_files[0].name == "test.wav"
        assert project.sample_paths[0] == "samples/test.wav"

    finally:
        audio.shut_down()


def test_load_new_sample_copies_file(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(tmp_path)

    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    source_dir = tmp_path / "source"
    source_dir.mkdir()

    source_filepath = source_dir / "test.wav"
    write_mono_pcm16_wav(source_filepath, 44_100)

    # Create project and session state
    project = ProjectState()
    session = SessionState()

    # Create audio engine and loader
    audio = AudioEngine()
    audio.run()

    try:
        loader = LoaderController(
            project=project,
            session=session,
            audio=audio,
            on_pad_bpm_changed=lambda _: None,
        )
        loader.restore_samples_from_project_state()

        loader.load_sample_async(0, source_filepath.as_posix())
        while loader.is_sample_loading(0):
            loader.poll_loader_events()

        assert loader.is_sample_loaded(0)
        sample_files = list(samples_dir.glob("*"))
        assert len(sample_files) == 1
        assert sample_files[0].name == "test.wav"
        assert project.sample_paths[0] == "samples/test.wav"

    finally:
        audio.shut_down()
