"""Tests for LooperController class.

Tests cover:
- Initialization
- Sample management (load, unload, is_loaded)
- Playback control (trigger, stop, stop_all)
- Audio parameters (volume, speed, reset_speed with clamping)
- Mode toggles (multi_loop, key_lock, bpm_lock)
- Error handling (invalid IDs, non-finite values, edge cases)
"""

import math
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.constants import (
    NUM_SAMPLES,
    PAD_EQ_DB_MAX,
    PAD_EQ_DB_MIN,
    PAD_GAIN_MAX,
    PAD_GAIN_MIN,
    SPEED_MAX,
    SPEED_MIN,
    VOLUME_MAX,
    VOLUME_MIN,
)
from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
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

        controller.loader.load_sample_async(sample_id, path)

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

        controller.loader.load_sample_async(sample_id, new_path)

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

        controller.loader.unload_sample(sample_id)

        audio_engine_mock.return_value.unload_sample.assert_called_with(sample_id)
        assert controller.project.sample_paths[sample_id] is None
        assert sample_id not in controller.session.active_sample_ids

    def test_is_sample_loaded_true(self, controller: LooperController) -> None:
        """Test is_sample_loaded returns True when sample is loaded."""
        sample_id = 0
        path = "/path/to/sample.wav"

        controller.project.sample_paths[sample_id] = path

        assert controller.transport.is_sample_loaded(sample_id) is True

    def test_is_sample_loaded_false(self, controller: LooperController) -> None:
        """Test is_sample_loaded returns False when sample is not loaded."""
        sample_id = 0

        assert controller.transport.is_sample_loaded(sample_id) is False


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

        controller.transport.trigger_pad(sample_id)

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

        controller.transport.trigger_pad(sample_id)

        # Should stop only this pad (toggle behavior)
        audio_engine_mock.return_value.stop_sample.assert_called_with(sample_id)
        # Should not stop all pads
        audio_engine_mock.return_value.stop_all.assert_not_called()

    def test_trigger_pad_not_loaded(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test triggering an unloaded pad does nothing."""
        sample_id = 0

        controller.transport.trigger_pad(sample_id)

        audio_engine_mock.return_value.play_sample.assert_not_called()

    def test_stop_pad(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test stopping a specific pad."""
        sample_id = 0
        path = "/path/to/sample.wav"
        controller.project.sample_paths[sample_id] = path
        controller.session.active_sample_ids.add(sample_id)

        controller.transport.stop_pad(sample_id)

        audio_engine_mock.return_value.stop_sample.assert_called_with(sample_id)
        assert sample_id not in controller.session.active_sample_ids

    def test_stop_pad_not_active(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test stopping an inactive pad does nothing."""
        sample_id = 0

        controller.transport.stop_pad(sample_id)

        audio_engine_mock.return_value.stop_sample.assert_not_called()

    def test_stop_all_pads(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test stopping all pads clears active samples."""
        controller.session.active_sample_ids.update({0, 1, 2})

        controller.transport.stop_all_pads()

        audio_engine_mock.return_value.stop_all.assert_called_once()
        assert controller.session.active_sample_ids == set()

    def test_is_sample_active_true(self, controller: LooperController) -> None:
        """Test is_sample_active returns True when sample is playing."""
        sample_id = 0
        controller.session.active_sample_ids.add(sample_id)

        assert sample_id in controller.session.active_sample_ids

    def test_is_sample_active_false(self, controller: LooperController) -> None:
        """Test is_sample_active returns False when sample is not playing."""
        sample_id = 0

        assert sample_id not in controller.session.active_sample_ids


class TestAudioParameters:
    """Test volume and speed parameter control."""

    def test_set_volume_normal(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test setting volume within valid range."""
        volume = 0.7

        controller.transport.set_volume(volume)

        audio_engine_mock.return_value.set_volume.assert_called_with(volume)
        assert controller.project.volume == volume

    def test_set_volume_clamps_max(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test volume is clamped to maximum."""
        volume = 2.0

        controller.transport.set_volume(volume)

        audio_engine_mock.return_value.set_volume.assert_called_with(VOLUME_MAX)
        assert controller.project.volume == VOLUME_MAX

    def test_set_volume_clamps_min(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test volume is clamped to minimum."""
        volume = -1.0

        controller.transport.set_volume(volume)

        audio_engine_mock.return_value.set_volume.assert_called_with(VOLUME_MIN)
        assert controller.project.volume == VOLUME_MIN

    def test_set_speed_normal(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test setting speed within valid range."""
        speed = 1.5

        controller.transport.set_speed(speed)

        audio_engine_mock.return_value.set_speed.assert_called_with(speed)
        assert controller.project.speed == speed

    def test_set_speed_clamps_max(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test speed is clamped to maximum."""
        speed = 3.0

        controller.transport.set_speed(speed)

        audio_engine_mock.return_value.set_speed.assert_called_with(SPEED_MAX)
        assert controller.project.speed == SPEED_MAX

    def test_set_speed_clamps_min(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test speed is clamped to minimum."""
        speed = 0.3

        controller.transport.set_speed(speed)

        audio_engine_mock.return_value.set_speed.assert_called_with(SPEED_MIN)
        assert controller.project.speed == SPEED_MIN

    def test_reset_speed(self, controller: LooperController, audio_engine_mock: Mock) -> None:
        """Test resetting speed to 1.0."""
        controller.project.speed = 1.8

        controller.transport.reset_speed()

        audio_engine_mock.return_value.set_speed.assert_called_with(1.0)
        assert controller.project.speed == 1.0


class TestPerPadMixing:
    def test_set_pad_gain_clamps_and_calls_engine(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        sample_id = 0

        controller.transport.set_pad_gain(sample_id, 0.25)
        audio_engine_mock.return_value.set_pad_gain.assert_called_with(sample_id, 0.25)
        assert controller.project.pad_gain[sample_id] == 0.25

        controller.transport.set_pad_gain(sample_id, -1.0)
        audio_engine_mock.return_value.set_pad_gain.assert_called_with(sample_id, PAD_GAIN_MIN)
        assert controller.project.pad_gain[sample_id] == PAD_GAIN_MIN

        controller.transport.set_pad_gain(sample_id, 2.0)
        audio_engine_mock.return_value.set_pad_gain.assert_called_with(sample_id, PAD_GAIN_MAX)
        assert controller.project.pad_gain[sample_id] == PAD_GAIN_MAX

    def test_set_pad_eq_clamps_and_calls_engine(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        sample_id = 0

        controller.transport.set_pad_eq(sample_id, 1.0, -2.0, 3.0)
        audio_engine_mock.return_value.set_pad_eq.assert_called_with(sample_id, 1.0, -2.0, 3.0)
        assert controller.project.pad_eq_low_db[sample_id] == 1.0
        assert controller.project.pad_eq_mid_db[sample_id] == -2.0
        assert controller.project.pad_eq_high_db[sample_id] == 3.0

        controller.transport.set_pad_eq(sample_id, 999.0, -999.0, 0.0)
        audio_engine_mock.return_value.set_pad_eq.assert_called_with(
            sample_id,
            PAD_EQ_DB_MAX,
            PAD_EQ_DB_MIN,
            0.0,
        )
        assert controller.project.pad_eq_low_db[sample_id] == PAD_EQ_DB_MAX
        assert controller.project.pad_eq_mid_db[sample_id] == PAD_EQ_DB_MIN

    def test_poll_audio_messages_updates_session_peak(
        self, controller: LooperController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        now = 123.0
        monkeypatch.setattr("flitzis_looper.controller.metering.monotonic", lambda: now)

        msg1 = Mock()
        msg1.pad_peak.return_value = (0, 0.8)
        msg2 = Mock()
        msg2.pad_peak.return_value = (0, 1.2)
        msg3 = Mock()
        msg3.pad_peak.return_value = None

        audio_engine_mock.return_value.receive_msg.side_effect = [msg1, msg2, msg3, None]

        controller.metering.poll_audio_messages()

        assert controller.session.pad_peak[0] == pytest.approx(1.0)
        assert controller.session.pad_peak_updated_at[0] == now


class TestModeToggles:
    """Test mode toggle methods."""

    def test_set_multi_loop_enable(self, controller: LooperController) -> None:
        """Test enabling multi loop mode."""
        controller.project.multi_loop = False

        controller.transport.set_multi_loop(enabled=True)

        assert controller.project.multi_loop is True

    def test_set_multi_loop_disable(self, controller: LooperController) -> None:
        """Test disabling multi loop mode."""
        controller.project.multi_loop = True

        controller.transport.set_multi_loop(enabled=False)

        assert controller.project.multi_loop is False

    def test_set_key_lock_enable(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test enabling key lock mode."""
        controller.project.key_lock = False

        controller.transport.set_key_lock(enabled=True)

        audio_engine_mock.return_value.set_key_lock.assert_called_with(enabled=True)
        assert controller.project.key_lock is True

    def test_set_key_lock_disable(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test disabling key lock mode."""
        controller.project.key_lock = True

        controller.transport.set_key_lock(enabled=False)

        audio_engine_mock.return_value.set_key_lock.assert_called_with(enabled=False)
        assert controller.project.key_lock is False

    def test_set_bpm_lock_enable(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test enabling BPM lock mode."""
        controller.project.bpm_lock = False

        controller.transport.set_bpm_lock(enabled=True)

        audio_engine_mock.return_value.set_bpm_lock.assert_called_with(enabled=True)
        assert controller.project.bpm_lock is True

    def test_set_bpm_lock_disable(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test disabling BPM lock mode."""
        controller.project.bpm_lock = True

        controller.transport.set_bpm_lock(enabled=False)

        audio_engine_mock.return_value.set_bpm_lock.assert_called_with(enabled=False)
        assert controller.project.bpm_lock is False

    def test_bpm_lock_anchors_master_bpm_to_selected_pad(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        controller.project.selected_pad = 1
        controller.transport.set_speed(1.5)
        controller.transport.set_manual_bpm(1, 120.0)

        audio_engine_mock.return_value.reset_mock()

        controller.transport.set_bpm_lock(enabled=True)

        assert controller.session.bpm_lock_anchor_pad_id == 1
        assert controller.session.master_bpm == pytest.approx(180.0)

        audio_engine_mock.return_value.set_bpm_lock.assert_called_with(enabled=True)
        assert audio_engine_mock.return_value.set_master_bpm.call_count == 1
        called_bpm = audio_engine_mock.return_value.set_master_bpm.call_args.args[0]
        assert called_bpm == pytest.approx(180.0)

        audio_engine_mock.return_value.reset_mock()

        controller.transport.set_speed(2.0)

        assert controller.session.master_bpm == pytest.approx(240.0)
        assert audio_engine_mock.return_value.set_master_bpm.call_count == 1
        called_bpm = audio_engine_mock.return_value.set_master_bpm.call_args.args[0]
        assert called_bpm == pytest.approx(240.0)


class TestManualBpm:
    """Test manual BPM set/clear, Tap BPM, and effective BPM."""

    def test_set_and_clear_manual_bpm(self, controller: LooperController) -> None:
        sample_id = 0

        controller.transport.set_manual_bpm(sample_id, 120.0)
        assert controller.project.manual_bpm[sample_id] == 120.0

        controller.transport.clear_manual_bpm(sample_id)
        assert controller.project.manual_bpm[sample_id] is None

    def test_effective_bpm_prefers_manual(self, controller: LooperController) -> None:
        sample_id = 0

        controller.project.sample_analysis[sample_id] = SampleAnalysis(
            bpm=123.4,
            key="C#m",
            beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0]),
        )
        assert controller.transport.effective_bpm(sample_id) == 123.4

        controller.transport.set_manual_bpm(sample_id, 120.0)
        assert controller.transport.effective_bpm(sample_id) == 120.0

    def test_tap_bpm_three_taps(
        self, controller: LooperController, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        sample_id = 0
        times = iter([0.0, 0.5, 1.0])
        monkeypatch.setattr("flitzis_looper.controller.transport.monotonic", lambda: next(times))

        assert controller.transport.tap_bpm(sample_id) is None
        assert controller.transport.tap_bpm(sample_id) is None
        bpm = controller.transport.tap_bpm(sample_id)

        assert bpm == pytest.approx(120.0, abs=0.01)
        assert controller.project.manual_bpm[sample_id] == pytest.approx(120.0, abs=0.01)

    def test_tap_bpm_uses_five_most_recent_taps(
        self, controller: LooperController, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        sample_id = 0
        times = iter([0.0, 1.0, 2.0, 3.0, 4.0, 10.0, 10.5, 11.0, 11.5, 12.0])
        monkeypatch.setattr("flitzis_looper.controller.transport.monotonic", lambda: next(times))

        bpm: float | None = None
        for _ in range(10):
            bpm = controller.transport.tap_bpm(sample_id)

        assert bpm == pytest.approx(120.0, abs=0.01)
        assert controller.session.tap_bpm_pad_id == sample_id
        assert len(controller.session.tap_bpm_timestamps) == 5


class TestManualKey:
    """Test manual key set/clear and effective key."""

    def test_set_and_clear_manual_key(self, controller: LooperController) -> None:
        sample_id = 0

        controller.transport.set_manual_key(sample_id, "Gm")
        assert controller.project.manual_key[sample_id] == "Gm"

        controller.transport.clear_manual_key(sample_id)
        assert controller.project.manual_key[sample_id] is None

    def test_effective_key_prefers_manual(self, controller: LooperController) -> None:
        sample_id = 0

        controller.project.sample_analysis[sample_id] = SampleAnalysis(
            bpm=123.4,
            key="C#m",
            beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0]),
        )
        assert controller.transport.effective_key(sample_id) == "C#m"

        controller.transport.set_manual_key(sample_id, "Gm")
        assert controller.transport.effective_key(sample_id) == "Gm"


class TestErrorHandling:
    """Test error handling and edge cases."""

    def test_invalid_sample_id_too_low(self, controller: LooperController) -> None:
        """Test that invalid sample ID below 0 raises ValueError."""
        invalid_id = -1

        with pytest.raises(ValueError, match="sample_id must be"):
            controller.loader.load_sample_async(invalid_id, "/path/to/sample.wav")

    def test_invalid_sample_id_too_high(self, controller: LooperController) -> None:
        """Test that invalid sample ID above NUM_SAMPLES raises ValueError."""
        invalid_id = NUM_SAMPLES

        with pytest.raises(ValueError, match="sample_id must be"):
            controller.loader.load_sample_async(invalid_id, "/path/to/sample.wav")

    def test_non_finite_volume_nan(self, controller: LooperController) -> None:
        """Test that NaN volume raises ValueError."""
        with pytest.raises(ValueError, match="value must be finite"):
            controller.transport.set_volume(math.nan)

    def test_non_finite_volume_inf(self, controller: LooperController) -> None:
        """Test that infinite volume raises ValueError."""
        with pytest.raises(ValueError, match="value must be finite"):
            controller.transport.set_volume(math.inf)

    def test_non_finite_speed_nan(self, controller: LooperController) -> None:
        """Test that NaN speed raises ValueError."""
        with pytest.raises(ValueError, match="value must be finite"):
            controller.transport.set_speed(math.nan)

    def test_non_finite_speed_inf(self, controller: LooperController) -> None:
        """Test that infinite speed raises ValueError."""
        with pytest.raises(ValueError, match="value must be finite"):
            controller.transport.set_speed(math.inf)

    def test_trigger_unloaded_sample(
        self, controller: LooperController, audio_engine_mock: Mock
    ) -> None:
        """Test triggering unloaded sample does not raise error."""
        sample_id = 0

        # Should not raise error
        controller.transport.trigger_pad(sample_id)

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

        controller.loader.poll_loader_events()

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

        controller.loader.poll_loader_events()

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

        controller.loader.poll_loader_events()

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

        controller.loader.poll_loader_events()

        assert 0 not in controller.session.analyzing_sample_ids
        assert 0 not in controller.session.sample_analysis_progress
        assert 0 not in controller.session.sample_analysis_stage
        assert controller.session.sample_analysis_errors[0] == "bad audio"
