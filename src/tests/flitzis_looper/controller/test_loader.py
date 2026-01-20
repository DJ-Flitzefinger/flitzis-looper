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

    audio_engine_mock.load_sample_async.assert_called_with(sample_id, path, run_analysis=True)
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

    audio_engine_mock.unload_sample.assert_called_with(sample_id)
    audio_engine_mock.load_sample_async.assert_called_with(sample_id, new_path, run_analysis=True)
    assert controller.project.sample_paths[sample_id] is None
    assert controller.session.pending_sample_paths[sample_id] == new_path


def test_loader_success_updates_project_sample_path(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.session.pending_sample_paths[0] = "/path/to/original.wav"

    analysis = {
        "bpm": 120.0,
        "key": "C#m",
        "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
    }

    audio_engine_mock.poll_loader_events.side_effect = [
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

    audio_engine_mock.unload_sample.assert_called_with(sample_id)
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
    audio_engine_mock.poll_loader_events.side_effect = [
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
    audio_engine_mock.poll_loader_events.side_effect = [
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
        "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
    }

    audio_engine_mock.poll_loader_events.side_effect = [
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
    audio_engine_mock.poll_loader_events.side_effect = [
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


def test_load_sample_async_already_loading(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test scheduling a load for a sample already loading clears existing state."""
    sample_id = 0
    path1 = "/path/to/first.wav"
    path2 = "/path/to/second.wav"

    controller.session.loading_sample_ids.add(sample_id)
    controller.session.pending_sample_paths[sample_id] = path1
    controller.session.sample_load_progress[sample_id] = 0.5
    controller.session.sample_load_stage[sample_id] = "Loading"

    controller.loader.load_sample_async(sample_id, path2)

    assert controller.session.pending_sample_paths[sample_id] == path2
    assert sample_id in controller.session.loading_sample_ids
    assert controller.session.sample_load_progress.get(sample_id) is None
    assert controller.session.sample_load_stage.get(sample_id) is None


def test_loader_started_event_handling(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test loader started event clears previous state and marks loading."""
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "started", "id": 0},
        None,
    ]

    controller.session.sample_load_errors[0] = "previous error"
    controller.session.sample_load_progress[0] = 0.5
    controller.session.sample_load_stage[0] = "Loading"
    controller.project.sample_paths[0] = None

    controller.loader.poll_loader_events()

    assert 0 in controller.session.loading_sample_ids
    assert controller.session.sample_load_errors.get(0) is None
    assert controller.session.sample_load_progress.get(0) is None
    assert controller.session.sample_load_stage.get(0) is None
    assert controller.project.sample_analysis[0] is None


def test_loader_progress_event_handling(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test loader progress event updates stage and percent."""
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "progress", "id": 0, "stage": "Decoding", "percent": 0.75},
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.session.sample_load_stage[0] == "Decoding"
    assert controller.session.sample_load_progress[0] == 0.75


def test_loader_error_event_handling(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test loader error event clears state and records error message."""
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "error",
            "id": 0,
            "msg": "File not found",
        },
        None,
    ]

    controller.session.loading_sample_ids.add(0)
    controller.session.pending_sample_paths[0] = "/path/to/sample.wav"
    controller.session.sample_load_progress[0] = 0.5
    controller.session.sample_load_stage[0] = "Loading"
    controller.project.sample_paths[0] = "/path/to/sample.wav"

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.loading_sample_ids
    assert 0 not in controller.session.pending_sample_paths
    assert controller.session.sample_load_progress.get(0) is None
    assert controller.session.sample_load_stage.get(0) is None
    assert controller.project.sample_paths[0] is None
    assert controller.project.sample_durations[0] is None
    assert controller.project.sample_analysis[0] is None
    assert controller.session.sample_load_errors[0] == "File not found"


def test_analyze_sample_async_success(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test successful analysis schedules async analysis."""
    controller.project.sample_paths[0] = "/path/to/sample.wav"

    controller.loader.analyze_sample_async(0)

    audio_engine_mock.analyze_sample_async.assert_called_with(0)
    assert 0 in controller.session.analyzing_sample_ids


def test_analyze_sample_async_already_loading(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test analysis returns early if sample is already loading."""
    controller.session.loading_sample_ids.add(0)

    controller.loader.analyze_sample_async(0)

    audio_engine_mock.analyze_sample_async.assert_not_called()
    assert 0 not in controller.session.analyzing_sample_ids


def test_analyze_sample_async_runtime_error(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test analysis runtime error is handled gracefully."""
    audio_engine_mock.analyze_sample_async.side_effect = RuntimeError("Audio engine busy")

    controller.loader.analyze_sample_async(0)

    assert 0 not in controller.session.analyzing_sample_ids
    assert controller.session.sample_analysis_errors[0] == "Audio engine busy"


def test_invalid_analysis_data_ignored(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test invalid analysis data (not a dict) is ignored."""
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        {"type": "task_success", "id": 0, "task": "analysis", "analysis": "invalid"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_analysis[0] is None


def test_analysis_validation_error_ignored(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test analysis data failing validation is ignored."""
    invalid_analysis = {"bpm": 120.0, "key": "C#m", "beat_grid": "invalid"}
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        {"type": "task_success", "id": 0, "task": "analysis", "analysis": invalid_analysis},
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_analysis[0] is None


def test_pending_sample_path(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test pending_sample_path returns pending path for loading sample."""
    path = "/path/to/sample.wav"
    controller.session.pending_sample_paths[0] = path

    assert controller.loader.pending_sample_path(0) == path


def test_sample_load_error(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test sample_load_error returns last error message."""
    controller.session.sample_load_errors[0] = "File not found"

    assert controller.loader.sample_load_error(0) == "File not found"


def test_sample_load_progress(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test sample_load_progress returns progress percentage."""
    controller.session.sample_load_progress[0] = 0.75

    assert controller.loader.sample_load_progress(0) == 0.75


def test_sample_load_stage(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test sample_load_stage returns load stage description."""
    controller.session.sample_load_stage[0] = "Decoding"

    assert controller.loader.sample_load_stage(0) == "Decoding"


def test_unload_sample_windows_path(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test unloading a sample with Windows path skips file deletion."""
    sample_id = 0
    path = "C:\\Users\\test\\sample.wav"
    controller.project.sample_paths[sample_id] = path

    controller.loader.unload_sample(sample_id)

    assert controller.project.sample_paths[sample_id] is None


def test_unload_sample_deletes_cached_file(
    tmp_path: Path, controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test unloading a sample deletes cached file."""
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    cached_file = samples_dir / "test.wav"
    cached_file.write_bytes(b"test data")

    controller.project.sample_paths[0] = "samples/test.wav"

    controller.loader.unload_sample(0)

    assert not cached_file.exists()


def test_unload_sample_outside_samples_dir(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test unloading a sample outside samples dir skips file deletion."""
    sample_id = 0
    path = "/other/path/sample.wav"
    controller.project.sample_paths[sample_id] = path

    controller.loader.unload_sample(sample_id)

    audio_engine_mock.unload_sample.assert_called_with(sample_id)
    assert controller.project.sample_paths[sample_id] is None


def test_poll_loader_events_with_malformed_events(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test poll_loader_events ignores malformed events."""
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "success", "id": 0, "duration_s": 1.0},
        {"type": 123, "id": 0},
        {"type": "success", "id": "not_a_number"},
        {"type": "unknown_type", "id": 0},
        {"type": "started", "id": 1},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.loading_sample_ids
    assert 1 in controller.session.loading_sample_ids
    assert controller.project.sample_durations[0] == 1.0
