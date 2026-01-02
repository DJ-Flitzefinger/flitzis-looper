"""Tests for LooperController class.

Tests cover:
- Initialization
- Sample management (load, unload, is_loaded)
- Playback control (trigger, stop, stop_all)
- Audio parameters (volume, speed, reset_speed with clamping)
- Mode toggles (multi_loop, key_lock, bpm_lock)
- Error handling (invalid IDs, non-finite values, edge cases)
"""

from __future__ import annotations

import math
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.constants import NUM_SAMPLES, SPEED_MAX, SPEED_MIN, VOLUME_MAX, VOLUME_MIN

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import LooperController


class TestControllerInitialization:
    """Test controller initialization and setup."""

    def test_controller_initializes_states(self, controller: LooperController) -> None:
        """Test that controller initializes project and session states."""
        assert controller.project is not None
        assert controller.session is not None
        assert len(controller.project.sample_paths) == NUM_SAMPLES

    def test_controller_initializes_audio_engine(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test that controller initializes and starts audio engine."""
        audio_engine_mock.assert_called_once()
        audio_engine_mock.return_value.run.assert_called_once()


class TestSampleManagement:
    """Test sample loading, unloading, and status checks."""

    def test_load_sample_async(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test scheduling a sample load updates state and calls audio engine."""
        sample_id = 0
        path = "/path/to/sample.wav"

        controller.load_sample_async(sample_id, path)

        audio_engine_mock.return_value.load_sample_async.assert_called_with(sample_id, path)
        assert controller.session.pending_sample_paths[sample_id] == path
        assert sample_id in controller.session.loading_sample_ids
        assert controller.project.sample_paths[sample_id] is None

    def test_load_sample_async_unloads_existing(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test scheduling a load for an already-loaded slot unloads first."""
        sample_id = 0
        old_path = "/path/to/old.wav"
        new_path = "/path/to/new.wav"

        controller.project.sample_paths[sample_id] = old_path

        controller.load_sample_async(sample_id, new_path)

        audio_engine_mock.return_value.unload_sample.assert_called_with(sample_id)
        audio_engine_mock.return_value.load_sample_async.assert_called_with(sample_id, new_path)
        assert controller.project.sample_paths[sample_id] is None
        assert controller.session.pending_sample_paths[sample_id] == new_path

    def test_unload_sample(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test unloading a sample stops playback and clears state."""
        sample_id = 0
        path = "/path/to/sample.wav"
        controller.project.sample_paths[sample_id] = path
        controller.session.active_sample_ids.add(sample_id)

        controller.unload_sample(sample_id)

        audio_engine_mock.return_value.unload_sample.assert_called_with(sample_id)
        assert controller.project.sample_paths[sample_id] is None
        assert sample_id not in controller.session.active_sample_ids

    def test_is_sample_loaded_true(self, controller: LooperController) -> None:
        """Test is_sample_loaded returns True when sample is loaded."""
        sample_id = 0
        path = "/path/to/sample.wav"

        controller.project.sample_paths[sample_id] = path

        assert controller.is_sample_loaded(sample_id) is True

    def test_is_sample_loaded_false(self, controller: LooperController) -> None:
        """Test is_sample_loaded returns False when sample is not loaded."""
        sample_id = 0

        assert controller.is_sample_loaded(sample_id) is False


class TestPlaybackControl:
    """Test playback control methods."""

    def test_trigger_pad_single_loop(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test triggering a pad in single loop mode stops other pads."""
        sample_id = 0
        path = "/path/to/sample.wav"
        controller.project.sample_paths[sample_id] = path
        controller.session.active_sample_ids.add(5)  # Another active sample
        controller.project.multi_loop = False

        controller.trigger_pad(sample_id)

        # Should stop all other pads first
        audio_engine_mock.return_value.stop_all.assert_called_once()
        # Then play the triggered pad
        audio_engine_mock.return_value.play_sample.assert_called_with(sample_id, 1.0)
        # Only the triggered pad should be active
        assert controller.session.active_sample_ids == {sample_id}

    def test_trigger_pad_multi_loop(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test triggering a pad in multi loop mode toggles playback."""
        sample_id = 0
        path = "/path/to/sample.wav"
        controller.project.sample_paths[sample_id] = path
        controller.session.active_sample_ids.add(sample_id)  # Already active
        controller.project.multi_loop = True

        controller.trigger_pad(sample_id)

        # Should stop only this pad (toggle behavior)
        audio_engine_mock.return_value.stop_sample.assert_called_with(sample_id)
        # Should not stop all pads
        audio_engine_mock.return_value.stop_all.assert_not_called()

    def test_trigger_pad_not_loaded(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test triggering an unloaded pad does nothing."""
        sample_id = 0

        controller.trigger_pad(sample_id)

        audio_engine_mock.return_value.play_sample.assert_not_called()

    def test_stop_pad(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test stopping a specific pad."""
        sample_id = 0
        path = "/path/to/sample.wav"
        controller.project.sample_paths[sample_id] = path
        controller.session.active_sample_ids.add(sample_id)

        controller.stop_pad(sample_id)

        audio_engine_mock.return_value.stop_sample.assert_called_with(sample_id)
        assert sample_id not in controller.session.active_sample_ids

    def test_stop_pad_not_active(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test stopping an inactive pad does nothing."""
        sample_id = 0

        controller.stop_pad(sample_id)

        audio_engine_mock.return_value.stop_sample.assert_not_called()

    def test_stop_all_pads(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test stopping all pads clears active samples."""
        controller.session.active_sample_ids.update({0, 1, 2})

        controller.stop_all_pads()

        audio_engine_mock.return_value.stop_all.assert_called_once()
        assert controller.session.active_sample_ids == set()

    def test_is_sample_active_true(self, controller: LooperController) -> None:
        """Test is_sample_active returns True when sample is playing."""
        sample_id = 0
        controller.session.active_sample_ids.add(sample_id)

        assert controller.is_sample_active(sample_id) is True

    def test_is_sample_active_false(self, controller: LooperController) -> None:
        """Test is_sample_active returns False when sample is not playing."""
        sample_id = 0

        assert controller.is_sample_active(sample_id) is False


class TestAudioParameters:
    """Test volume and speed parameter control."""

    def test_set_volume_normal(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test setting volume within valid range."""
        volume = 0.7

        controller.set_volume(volume)

        audio_engine_mock.return_value.set_volume.assert_called_with(volume)
        assert controller.project.volume == volume

    def test_set_volume_clamps_max(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test volume is clamped to maximum."""
        volume = 2.0

        controller.set_volume(volume)

        audio_engine_mock.return_value.set_volume.assert_called_with(VOLUME_MAX)
        assert controller.project.volume == VOLUME_MAX

    def test_set_volume_clamps_min(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test volume is clamped to minimum."""
        volume = -1.0

        controller.set_volume(volume)

        audio_engine_mock.return_value.set_volume.assert_called_with(VOLUME_MIN)
        assert controller.project.volume == VOLUME_MIN

    def test_set_speed_normal(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test setting speed within valid range."""
        speed = 1.5

        controller.set_speed(speed)

        audio_engine_mock.return_value.set_speed.assert_called_with(speed)
        assert controller.project.speed == speed

    def test_set_speed_clamps_max(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test speed is clamped to maximum."""
        speed = 3.0

        controller.set_speed(speed)

        audio_engine_mock.return_value.set_speed.assert_called_with(SPEED_MAX)
        assert controller.project.speed == SPEED_MAX

    def test_set_speed_clamps_min(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test speed is clamped to minimum."""
        speed = 0.3

        controller.set_speed(speed)

        audio_engine_mock.return_value.set_speed.assert_called_with(SPEED_MIN)
        assert controller.project.speed == SPEED_MIN

    def test_reset_speed(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test resetting speed to 1.0."""
        controller.project.speed = 1.8

        controller.reset_speed()

        audio_engine_mock.return_value.set_speed.assert_called_with(1.0)
        assert controller.project.speed == 1.0


class TestModeToggles:
    """Test mode toggle methods."""

    def test_set_multi_loop_enable(self, controller: LooperController) -> None:
        """Test enabling multi loop mode."""
        controller.project.multi_loop = False

        controller.set_multi_loop(enabled=True)

        assert controller.project.multi_loop is True

    def test_set_multi_loop_disable(self, controller: LooperController) -> None:
        """Test disabling multi loop mode."""
        controller.project.multi_loop = True

        controller.set_multi_loop(enabled=False)

        assert controller.project.multi_loop is False

    def test_set_key_lock_enable(self, controller: LooperController) -> None:
        """Test enabling key lock mode."""
        controller.project.key_lock = False

        controller.set_key_lock(enabled=True)

        assert controller.project.key_lock is True

    def test_set_key_lock_disable(self, controller: LooperController) -> None:
        """Test disabling key lock mode."""
        controller.project.key_lock = True

        controller.set_key_lock(enabled=False)

        assert controller.project.key_lock is False

    def test_set_bpm_lock_enable(self, controller: LooperController) -> None:
        """Test enabling BPM lock mode."""
        controller.project.bpm_lock = False

        controller.set_bpm_lock(enabled=True)

        assert controller.project.bpm_lock is True

    def test_set_bpm_lock_disable(self, controller: LooperController) -> None:
        """Test disabling BPM lock mode."""
        controller.project.bpm_lock = True

        controller.set_bpm_lock(enabled=False)

        assert controller.project.bpm_lock is False


class TestErrorHandling:
    """Test error handling and edge cases."""

    def test_invalid_sample_id_too_low(self, controller: LooperController) -> None:
        """Test that invalid sample ID below 0 raises ValueError."""
        invalid_id = -1

        with pytest.raises(ValueError, match="sample_id must be"):
            controller.load_sample_async(invalid_id, "/path/to/sample.wav")

    def test_invalid_sample_id_too_high(self, controller: LooperController) -> None:
        """Test that invalid sample ID above NUM_SAMPLES raises ValueError."""
        invalid_id = NUM_SAMPLES

        with pytest.raises(ValueError, match="sample_id must be"):
            controller.load_sample_async(invalid_id, "/path/to/sample.wav")

    def test_non_finite_volume_nan(self, controller: LooperController) -> None:
        """Test that NaN volume raises ValueError."""
        with pytest.raises(ValueError, match="value must be finite"):
            controller.set_volume(math.nan)

    def test_non_finite_volume_inf(self, controller: LooperController) -> None:
        """Test that infinite volume raises ValueError."""
        with pytest.raises(ValueError, match="value must be finite"):
            controller.set_volume(math.inf)

    def test_non_finite_speed_nan(self, controller: LooperController) -> None:
        """Test that NaN speed raises ValueError."""
        with pytest.raises(ValueError, match="value must be finite"):
            controller.set_speed(math.nan)

    def test_non_finite_speed_inf(self, controller: LooperController) -> None:
        """Test that infinite speed raises ValueError."""
        with pytest.raises(ValueError, match="value must be finite"):
            controller.set_speed(math.inf)

    def test_trigger_unloaded_sample(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test triggering unloaded sample does not raise error."""
        sample_id = 0

        # Should not raise error
        controller.trigger_pad(sample_id)

        # Should not attempt to play
        audio_engine_mock.return_value.play_sample.assert_not_called()


class TestAnalysisEvents:
    """Test controller handling of analysis task loader events."""

    def test_task_started_sets_analyzing_state(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        audio_engine_mock.return_value.poll_loader_events.side_effect = [
            {"type": "task_started", "id": 0, "task": "analysis"},
            None,
        ]

        controller.poll_loader_events()

        assert 0 in controller.session.analyzing_sample_ids
        assert 0 not in controller.session.sample_analysis_progress
        assert 0 not in controller.session.sample_analysis_stage
        assert 0 not in controller.session.sample_analysis_errors

    def test_task_progress_updates_stage_and_percent(
        self, controller: LooperController, audio_engine_mock: Mock
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

        controller.poll_loader_events()

        assert controller.session.sample_analysis_progress[0] == 0.25
        assert controller.session.sample_analysis_stage[0] == "Analyzing"

    def test_task_success_stores_analysis_and_clears_task_state(
        self, controller: LooperController, audio_engine_mock: Mock
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

        controller.poll_loader_events()

        assert controller.project.sample_analysis[0] is not None
        assert controller.project.sample_analysis[0].bpm == 120.0
        assert controller.project.sample_analysis[0].key == "C#m"
        assert 0 not in controller.session.analyzing_sample_ids
        assert 0 not in controller.session.sample_analysis_progress
        assert 0 not in controller.session.sample_analysis_stage
        assert 0 not in controller.session.sample_analysis_errors

    def test_task_error_records_error_and_clears_progress(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        audio_engine_mock.return_value.poll_loader_events.side_effect = [
            {"type": "task_started", "id": 0, "task": "analysis"},
            {"type": "task_progress", "id": 0, "task": "analysis", "percent": 0.5},
            {"type": "task_error", "id": 0, "task": "analysis", "msg": "bad audio"},
            None,
        ]

        controller.poll_loader_events()

        assert 0 not in controller.session.analyzing_sample_ids
        assert 0 not in controller.session.sample_analysis_progress
        assert 0 not in controller.session.sample_analysis_stage
        assert controller.session.sample_analysis_errors[0] == "bad audio"
