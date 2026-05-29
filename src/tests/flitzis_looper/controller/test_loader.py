from typing import TYPE_CHECKING

import pytest

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.stems import (
    cache_dir_for_sample_id,
    expected_stem_files,
    source_version_for_sample_path,
)
from flitzis_looper.models import (
    STEM_COMPONENT_MASK,
    STEM_KINDS,
    BeatGrid,
    ProjectState,
    SampleAnalysis,
    SessionState,
    StemCacheEntry,
)
from flitzis_looper_audio import AudioEngine
from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _running_audio_engine_or_skip() -> AudioEngine:
    audio = AudioEngine()
    try:
        audio.run()
    except RuntimeError as exc:
        audio.shut_down()
        pytest.skip(f"AudioEngine unavailable: {exc}")
    return audio


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
    controller.project.stem_cache[sample_id] = StemCacheEntry(
        source_version="old",
        cache_dir="samples/stems/old",
    )

    controller.loader.load_sample_async(sample_id, new_path)

    audio_engine_mock.unload_sample.assert_called_with(sample_id)
    audio_engine_mock.load_sample_async.assert_called_with(sample_id, new_path, run_analysis=True)
    assert controller.project.sample_paths[sample_id] is None
    assert controller.project.stem_cache[sample_id] is None
    assert controller.session.pending_sample_paths[sample_id] == new_path


def test_load_sample_async_resets_stale_empty_pad_settings(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Loading into an empty pad starts from defaults even if the config has stale values."""
    sample_id = 0
    defaults = ProjectState()
    controller.project.manual_bpm[sample_id] = 123.0
    controller.project.manual_key[sample_id] = "Gm"
    controller.project.pad_key_lock[sample_id] = True
    controller.project.pad_gain_db[sample_id] = -6.0
    controller.project.pad_eq_low_db[sample_id] = 1.0
    controller.project.pad_eq_mid_db[sample_id] = -2.0
    controller.project.pad_eq_high_db[sample_id] = 3.0
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_start_s[sample_id] = 5.0
    controller.project.pad_loop_end_s[sample_id] = 10.0
    controller.project.pad_loop_bars[sample_id] = 2.0
    controller.project.pad_grid_offset_samples[sample_id] = -240

    controller.loader.load_sample_async(sample_id, "/path/to/new.wav")

    assert controller.project.manual_bpm[sample_id] == defaults.manual_bpm[sample_id]
    assert controller.project.manual_key[sample_id] == defaults.manual_key[sample_id]
    assert controller.project.pad_key_lock[sample_id] == defaults.pad_key_lock[sample_id]
    assert controller.project.pad_gain_db[sample_id] == defaults.pad_gain_db[sample_id]
    assert controller.project.pad_eq_low_db[sample_id] == defaults.pad_eq_low_db[sample_id]
    assert controller.project.pad_eq_mid_db[sample_id] == defaults.pad_eq_mid_db[sample_id]
    assert controller.project.pad_eq_high_db[sample_id] == defaults.pad_eq_high_db[sample_id]
    assert controller.project.pad_loop_auto[sample_id] == defaults.pad_loop_auto[sample_id]
    assert controller.project.pad_loop_start_s[sample_id] == defaults.pad_loop_start_s[sample_id]
    assert controller.project.pad_loop_end_s[sample_id] == defaults.pad_loop_end_s[sample_id]
    assert controller.project.pad_loop_bars[sample_id] == defaults.pad_loop_bars[sample_id]
    assert (
        controller.project.pad_grid_offset_samples[sample_id]
        == defaults.pad_grid_offset_samples[sample_id]
    )
    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, None)
    audio_engine_mock.set_pad_gain.assert_called_with(sample_id, defaults.pad_gain_db[sample_id])
    audio_engine_mock.set_pad_eq.assert_called_with(sample_id, 0.0, 0.0, 0.0)
    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, 0.0, None)
    disabled = False
    audio_engine_mock.set_pad_key_lock.assert_called_with(sample_id, disabled)
    audio_engine_mock.load_sample_async.assert_called_with(
        sample_id, "/path/to/new.wav", run_analysis=True
    )


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


def test_stale_loader_success_with_old_request_id_is_ignored(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.load_sample_async.return_value = 2
    controller.loader.load_sample_async(0, "/path/to/current.wav")

    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 1,
            "duration_s": 1.0,
            "cached_path": "samples/old.wav",
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] is None
    assert controller.project.sample_durations[0] is None
    assert controller.session.pending_sample_paths[0] == "/path/to/current.wav"
    assert 0 in controller.session.loading_sample_ids


def test_stale_loader_progress_after_unload_is_ignored(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.load_sample_async.return_value = 7
    controller.loader.load_sample_async(0, "/path/to/current.wav")

    controller.loader.unload_sample(0)

    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "progress", "id": 0, "request_id": 7, "stage": "Decoding", "percent": 0.5},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.loading_sample_ids
    assert 0 not in controller.session.sample_load_progress
    assert 0 not in controller.session.sample_load_stage


def test_loader_success_initializes_new_sample_loop_defaults(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.session.pending_sample_paths[0] = "/path/to/original.wav"
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 5.0
    controller.project.pad_loop_end_s[0] = 10.0
    controller.project.pad_loop_bars[0] = 2.0

    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "duration_s": 32.0,
            "cached_path": "samples/foo.wav",
            "analysis": {
                "bpm": 120.0,
                "key": "C#m",
                "beat_grid": {"beats": [2.0, 2.5], "downbeats": [2.0], "bars": [2.0]},
            },
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_auto[0] is True
    assert controller.project.pad_loop_bars[0] == 8.0
    assert controller.project.pad_loop_start_s[0] == pytest.approx(0.0)
    assert controller.project.pad_loop_end_s[0] is None
    audio_engine_mock.set_pad_loop_region.assert_called_with(0, 0.0, 16.0)


def test_restored_sample_success_preserves_existing_loop_settings(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.session.pending_sample_paths[0] = "samples/foo.wav"
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 5.0
    controller.project.pad_loop_end_s[0] = 10.0
    controller.project.pad_loop_bars[0] = 2.0

    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "duration_s": 32.0,
            "cached_path": "samples/foo.wav",
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_auto[0] is False
    assert controller.project.pad_loop_bars[0] == 2.0
    assert controller.project.pad_loop_start_s[0] == pytest.approx(5.0)
    assert controller.project.pad_loop_end_s[0] == pytest.approx(10.0)


def test_unload_sample(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test unloading a sample stops playback and clears state."""
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.project.sample_durations[sample_id] = 1.0
    controller.project.stem_cache[sample_id] = StemCacheEntry(
        source_version="old",
        cache_dir="samples/stems/old",
    )
    controller.session.active_sample_ids.add(sample_id)
    controller.session.stem_generating_sample_ids.add(sample_id)

    controller.loader.unload_sample(sample_id)

    audio_engine_mock.unload_sample.assert_called_with(sample_id)
    assert controller.project.sample_paths[sample_id] is None
    assert controller.project.sample_durations[sample_id] is None
    assert controller.project.stem_cache[sample_id] is None
    assert sample_id not in controller.session.active_sample_ids
    assert sample_id not in controller.session.stem_generating_sample_ids


def test_unload_sample_closes_waveform_editor_for_unloaded_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Unloading the edited pad returns the center surface to the pad view."""
    sample_id = 0
    controller.project.sample_paths[sample_id] = "/path/to/sample.wav"
    controller.session.waveform_editor_open = True
    controller.session.waveform_editor_pad_id = sample_id

    controller.loader.unload_sample(sample_id)

    assert controller.session.waveform_editor_open is False
    assert controller.session.waveform_editor_pad_id is None
    audio_engine_mock.unload_sample.assert_called_once_with(sample_id)


def test_unload_sample_keeps_waveform_editor_for_different_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Unloading another pad does not close an unrelated editor."""
    sample_id = 0
    edited_pad_id = 1
    controller.project.sample_paths[sample_id] = "/path/to/sample.wav"
    controller.project.sample_paths[edited_pad_id] = "/path/to/other.wav"
    controller.session.waveform_editor_open = True
    controller.session.waveform_editor_pad_id = edited_pad_id

    controller.loader.unload_sample(sample_id)

    assert controller.session.waveform_editor_open is True
    assert controller.session.waveform_editor_pad_id == edited_pad_id
    audio_engine_mock.unload_sample.assert_called_once_with(sample_id)


def test_unload_sample_resets_track_bound_pad_settings(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Unloading clears the persisted settings that belong to the previous track."""
    sample_id = 0
    defaults = ProjectState()
    controller.project.sample_paths[sample_id] = "/path/to/sample.wav"
    controller.project.sample_durations[sample_id] = 42.0
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=123.0,
        key="C#m",
        beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.stem_cache[sample_id] = StemCacheEntry(
        source_version="old",
        cache_dir="samples/stems/old",
    )
    controller.project.pad_stem_mix_mode[sample_id] = "all_stems"
    controller.project.pad_key_lock[sample_id] = True
    controller.project.manual_bpm[sample_id] = 128.0
    controller.project.manual_key[sample_id] = "Gm"
    controller.project.pad_gain_db[sample_id] = -9.0
    controller.project.pad_eq_low_db[sample_id] = 1.5
    controller.project.pad_eq_mid_db[sample_id] = -3.0
    controller.project.pad_eq_high_db[sample_id] = 4.5
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_start_s[sample_id] = 6.0
    controller.project.pad_loop_end_s[sample_id] = 14.0
    controller.project.pad_loop_bars[sample_id] = 2.0
    controller.project.pad_grid_offset_samples[sample_id] = 512

    controller.loader.unload_sample(sample_id)

    assert controller.project.sample_paths[sample_id] == defaults.sample_paths[sample_id]
    assert controller.project.sample_durations[sample_id] == defaults.sample_durations[sample_id]
    assert controller.project.sample_analysis[sample_id] == defaults.sample_analysis[sample_id]
    assert controller.project.stem_cache[sample_id] == defaults.stem_cache[sample_id]
    assert controller.project.pad_stem_mix_mode[sample_id] == defaults.pad_stem_mix_mode[sample_id]
    assert controller.project.pad_key_lock[sample_id] == defaults.pad_key_lock[sample_id]
    assert controller.project.manual_bpm[sample_id] == defaults.manual_bpm[sample_id]
    assert controller.project.manual_key[sample_id] == defaults.manual_key[sample_id]
    assert controller.project.pad_gain_db[sample_id] == defaults.pad_gain_db[sample_id]
    assert controller.project.pad_eq_low_db[sample_id] == defaults.pad_eq_low_db[sample_id]
    assert controller.project.pad_eq_mid_db[sample_id] == defaults.pad_eq_mid_db[sample_id]
    assert controller.project.pad_eq_high_db[sample_id] == defaults.pad_eq_high_db[sample_id]
    assert controller.project.pad_loop_auto[sample_id] == defaults.pad_loop_auto[sample_id]
    assert controller.project.pad_loop_start_s[sample_id] == defaults.pad_loop_start_s[sample_id]
    assert controller.project.pad_loop_end_s[sample_id] == defaults.pad_loop_end_s[sample_id]
    assert controller.project.pad_loop_bars[sample_id] == defaults.pad_loop_bars[sample_id]
    assert (
        controller.project.pad_grid_offset_samples[sample_id]
        == defaults.pad_grid_offset_samples[sample_id]
    )
    audio_engine_mock.unload_sample.assert_called_once_with(sample_id)
    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, None)
    audio_engine_mock.set_pad_gain.assert_called_with(sample_id, defaults.pad_gain_db[sample_id])
    audio_engine_mock.set_pad_eq.assert_called_with(sample_id, 0.0, 0.0, 0.0)
    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, 0.0, None)
    disabled = False
    audio_engine_mock.set_pad_key_lock.assert_called_with(sample_id, disabled)


def test_unload_sample_with_negative_grid_offset_does_not_publish_invalid_timing_metadata(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Regression: unloaded pads must not publish stale negative grid anchors to Rust."""
    audio_engine_mock.output_sample_rate.return_value = 44_100
    audio_engine_mock.set_pad_timing_metadata.side_effect = ValueError(
        "phase_anchor_s out of range"
    )
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/loop.wav"
    controller.project.pad_grid_offset_samples[sample_id] = -1

    controller.loader.unload_sample(sample_id)

    audio_engine_mock.unload_sample.assert_called_once_with(sample_id)
    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, None)
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


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

    audio = _running_audio_engine_or_skip()

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
    audio = _running_audio_engine_or_skip()

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


def test_stale_analysis_success_after_unload_is_ignored(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    audio_engine_mock.analyze_sample_async.return_value = 5
    controller.loader.analyze_sample_async(0)

    controller.loader.unload_sample(0)

    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "task_success",
            "id": 0,
            "request_id": 5,
            "task": "analysis",
            "analysis": {
                "bpm": 120.0,
                "key": "C#m",
                "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
            },
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] is None
    assert controller.project.sample_analysis[0] is None
    assert 0 not in controller.session.analyzing_sample_ids


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


def test_stem_task_events_update_generation_state(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "stem_generation"},
        {
            "type": "task_progress",
            "id": 0,
            "task": "stem_generation",
            "percent": 0.25,
            "stage": "Generating stems",
        },
        {"type": "task_error", "id": 0, "task": "stem_generation", "msg": "not implemented"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.stem_generating_sample_ids
    assert 0 not in controller.session.stem_generation_progress
    assert 0 not in controller.session.stem_generation_stage
    assert controller.session.stem_generation_errors[0] == "not implemented"


def test_stem_task_success_marks_complete_current_cache_available(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_path = samples_dir / "loop.wav"
    write_mono_pcm16_wav(sample_path, 44_100)
    controller.project.sample_paths[0] = "samples/loop.wav"

    source_version = source_version_for_sample_path("samples/loop.wav")
    assert source_version is not None
    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")

    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=source_version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=False,
    )
    controller.session.stem_generating_sample_ids.add(0)
    controller.session.stem_generation_source_versions[0] = source_version
    controller.session.stem_generation_progress[0] = 0.5
    controller.session.stem_generation_stage[0] = "Writing stem cache"
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_success", "id": 0, "task": "stem_generation"},
        None,
    ]

    controller.loader.poll_loader_events()

    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is True
    audio_engine_mock.publish_prepared_stems.assert_called_once_with(0, source_version, cache_dir)
    audio_engine_mock.set_stem_mix_mode.assert_not_called()
    audio_engine_mock.set_stem_enabled_mask.assert_not_called()
    assert 0 not in controller.session.stem_generating_sample_ids
    assert 0 not in controller.session.stem_generation_source_versions
    assert 0 not in controller.session.stem_generation_progress
    assert 0 not in controller.session.stem_generation_stage


def test_stem_task_success_applies_all_stems_preference_after_publication(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_path = samples_dir / "loop.wav"
    write_mono_pcm16_wav(sample_path, 44_100)
    controller.project.sample_paths[0] = "samples/loop.wav"
    controller.project.pad_stem_mix_mode[0] = "all_stems"

    source_version = source_version_for_sample_path("samples/loop.wav")
    assert source_version is not None
    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")

    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=source_version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=False,
    )
    controller.session.stem_generating_sample_ids.add(0)
    controller.session.stem_generation_source_versions[0] = source_version
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_success", "id": 0, "task": "stem_generation"},
        None,
    ]

    controller.loader.poll_loader_events()

    audio_engine_mock.publish_prepared_stems.assert_called_once_with(0, source_version, cache_dir)
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "all_stems", source_version)
    audio_engine_mock.set_stem_enabled_mask.assert_called_once_with(
        0, STEM_COMPONENT_MASK, source_version
    )


def test_stem_task_success_keeps_cache_unavailable_when_pad_started(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_path = samples_dir / "loop.wav"
    write_mono_pcm16_wav(sample_path, 44_100)
    controller.project.sample_paths[0] = "samples/loop.wav"

    source_version = source_version_for_sample_path("samples/loop.wav")
    assert source_version is not None
    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")

    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=source_version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=False,
    )
    controller.session.stem_generating_sample_ids.add(0)
    controller.session.stem_generation_source_versions[0] = source_version
    controller.session.active_sample_ids.add(0)
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_success", "id": 0, "task": "stem_generation"},
        None,
    ]

    controller.loader.poll_loader_events()

    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    audio_engine_mock.publish_prepared_stems.assert_not_called()
    assert 0 not in controller.session.stem_generating_sample_ids


def test_stem_task_success_clears_stale_source_version(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    old_path = samples_dir / "old.wav"
    new_path = samples_dir / "new.wav"
    write_mono_pcm16_wav(old_path, 44_100)
    write_mono_pcm16_wav(new_path, 44_100)
    controller.project.sample_paths[0] = "samples/old.wav"

    old_version = source_version_for_sample_path("samples/old.wav")
    assert old_version is not None
    cache_dir = cache_dir_for_sample_id(0)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=old_version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=False,
    )
    controller.session.stem_generating_sample_ids.add(0)
    controller.session.stem_generation_source_versions[0] = old_version
    controller.project.sample_paths[0] = "samples/new.wav"
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_success", "id": 0, "task": "stem_generation"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.stem_cache[0] is None
    audio_engine_mock.publish_prepared_stems.assert_not_called()
    assert 0 not in controller.session.stem_generating_sample_ids


def test_stem_task_success_keeps_cache_unavailable_when_publication_fails(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_path = samples_dir / "loop.wav"
    write_mono_pcm16_wav(sample_path, 44_100)
    controller.project.sample_paths[0] = "samples/loop.wav"

    source_version = source_version_for_sample_path("samples/loop.wav")
    assert source_version is not None
    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")

    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=source_version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=False,
    )
    controller.session.stem_generating_sample_ids.add(0)
    controller.session.stem_generation_source_versions[0] = source_version
    audio_engine_mock.publish_prepared_stems.side_effect = RuntimeError("buffer may be full")
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_success", "id": 0, "task": "stem_generation"},
        None,
    ]

    controller.loader.poll_loader_events()

    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    audio_engine_mock.publish_prepared_stems.assert_called_once_with(0, source_version, cache_dir)
    assert "publication failed" in controller.session.stem_generation_errors[0]


def test_stale_stem_task_error_is_ignored(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_error", "id": 0, "task": "stem_generation", "msg": "late"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.stem_generation_errors


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


def test_unload_sample_deletes_stem_cache_dir(
    tmp_path: Path, controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test unloading a sample deletes the pad stem cache directory."""
    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")

    controller.project.sample_paths[0] = "samples/test.wav"
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="samples/test.wav|10|20",
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )

    controller.loader.unload_sample(0)

    assert not stems_dir.exists()
    assert controller.project.stem_cache[0] is None


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
