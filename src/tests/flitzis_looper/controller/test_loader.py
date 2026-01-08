from typing import TYPE_CHECKING

import pytest

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper_audio import AudioEngine
from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def test_load_sample_async(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test scheduling a sample load updates state and calls audio engine."""
    sample_id = 0
    path = "/path/to/sample.wav"

    controller.loader.load_sample_async(sample_id, path)

    audio_engine_mock.return_value.load_sample_async.assert_called_with(
        sample_id, path, run_analysis=True
    )
    assert controller.session.pending_sample_paths[sample_id] == path
    assert sample_id in controller.session.loading_sample_ids
    assert controller.project.sample_paths[sample_id] is None


def test_load_sample_async_unloads_existing(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test scheduling a load for an already-loaded slot unloads first."""
    sample_id = 0
    old_path = "/path/to/old.wav"
    new_path = "/path/to/new.wav"

    controller.project.sample_paths[sample_id] = old_path

    controller.loader.load_sample_async(sample_id, new_path)

    audio_engine_mock.return_value.unload_sample.assert_called_with(sample_id)
    audio_engine_mock.return_value.load_sample_async.assert_called_with(
        sample_id, new_path, run_analysis=True
    )
    assert controller.project.sample_paths[sample_id] is None
    assert controller.session.pending_sample_paths[sample_id] == new_path


def test_loader_success_updates_project_sample_path(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.session.pending_sample_paths[0] = "/path/to/original.wav"

    analysis = {
        "bpm": 120.0,
        "key": "C#m",
        "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0]},
    }

    audio_engine_mock.return_value.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "duration_s": 1.0,
            "cached_path": "samples/foo.wav",
            "analysis": analysis,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] == "samples/foo.wav"
    assert controller.project.sample_durations[0] == 1.0
    assert controller.project.sample_analysis[0] is not None
    assert 0 not in controller.session.pending_sample_paths


def test_unload_sample(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test unloading a sample stops playback and clears state."""
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.project.sample_durations[sample_id] = 1.0
    controller.session.active_sample_ids.add(sample_id)

    controller.loader.unload_sample(sample_id)

    audio_engine_mock.return_value.unload_sample.assert_called_with(sample_id)
    assert controller.project.sample_paths[sample_id] is None
    assert controller.project.sample_durations[sample_id] is None
    assert sample_id not in controller.session.active_sample_ids


def test_is_sample_loaded_true(controller: AppController) -> None:
    """Test is_sample_loaded returns True when sample is loaded."""
    sample_id = 0
    path = "/path/to/sample.wav"

    controller.project.sample_paths[sample_id] = path

    assert controller.loader.is_sample_loaded(sample_id) is True


def test_is_sample_loaded_false(controller: AppController) -> None:
    """Test is_sample_loaded returns False when sample is not loaded."""
    sample_id = 0

    assert controller.loader.is_sample_loaded(sample_id) is False


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
        assert project.sample_durations[0] == pytest.approx(0.0029, rel=1e-3)

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
        assert project.sample_durations[0] == pytest.approx(0.0029, rel=1e-3)

    finally:
        audio.shut_down()


def test_invalid_sample_id_too_low(controller: AppController) -> None:
    """Test that invalid sample ID below 0 raises ValueError."""
    invalid_id = -1

    with pytest.raises(ValueError, match="sample_id must be"):
        controller.loader.load_sample_async(invalid_id, "/path/to/sample.wav")


def test_invalid_sample_id_too_high(controller: AppController) -> None:
    """Test that invalid sample ID above NUM_SAMPLES raises ValueError."""
    invalid_id = NUM_SAMPLES

    with pytest.raises(ValueError, match="sample_id must be"):
        controller.loader.load_sample_async(invalid_id, "/path/to/sample.wav")


def test_task_started_sets_analyzing_state(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.return_value.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 in controller.session.analyzing_sample_ids
    assert 0 not in controller.session.sample_analysis_progress
    assert 0 not in controller.session.sample_analysis_stage
    assert 0 not in controller.session.sample_analysis_errors


def test_task_progress_updates_stage_and_percent(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.return_value.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        {
            "type": "task_progress",
            "id": 0,
            "task": "analysis",
            "percent": 0.25,
            "stage": "Analyzing",
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.session.sample_analysis_progress[0] == 0.25
    assert controller.session.sample_analysis_stage[0] == "Analyzing"


def test_task_success_stores_analysis_and_clears_task_state(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    analysis = {
        "bpm": 120.0,
        "key": "C#m",
        "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0]},
    }

    audio_engine_mock.return_value.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        {"type": "task_success", "id": 0, "task": "analysis", "analysis": analysis},
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_analysis[0] is not None
    assert controller.project.sample_analysis[0].bpm == 120.0
    assert controller.project.sample_analysis[0].key == "C#m"
    assert 0 not in controller.session.analyzing_sample_ids
    assert 0 not in controller.session.sample_analysis_progress
    assert 0 not in controller.session.sample_analysis_stage
    assert 0 not in controller.session.sample_analysis_errors


def test_task_error_records_error_and_clears_progress(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.return_value.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        {"type": "task_progress", "id": 0, "task": "analysis", "percent": 0.5},
        {"type": "task_error", "id": 0, "task": "analysis", "msg": "bad audio"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.analyzing_sample_ids
    assert 0 not in controller.session.sample_analysis_progress
    assert 0 not in controller.session.sample_analysis_stage
    assert controller.session.sample_analysis_errors[0] == "bad audio"
