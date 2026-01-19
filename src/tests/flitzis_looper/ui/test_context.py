"""Tests for UI context components.

Tests cover:
- UiState computed properties (pad_label, is_pad_loaded, is_pad_active, etc)
- AudioActions delegation to controller
- UiActions state mutations
- UiContext initialization and access
"""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.constants import SPEED_STEP
from flitzis_looper.models import BeatGrid, ProjectState, SampleAnalysis
from flitzis_looper.ui.context import (
    AudioActions,
    ReadOnlyStateProxy,
    UiActions,
    UiContext,
    UiState,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


class TestReadOnlyStateProxy:
    """Test the read-only state proxy behavior."""

    def test_read_only_proxy_read_access(self) -> None:
        """Test that read-only proxy allows reading attributes."""
        project = ProjectState()
        proxy = ReadOnlyStateProxy(project)

        # Should be able to read attributes
        assert proxy.multi_loop is False
        assert proxy.speed == 1.0
        assert proxy.volume == 1.0

    def test_read_only_proxy_write_protection(self) -> None:
        """Test that read-only proxy prevents writing attributes."""
        project = ProjectState()
        proxy = ReadOnlyStateProxy(project)

        # Should NOT be able to write attributes
        with pytest.raises(AttributeError, match="State is read-only"):
            proxy.multi_loop = True

        with pytest.raises(AttributeError, match="State is read-only"):
            proxy.speed = 2.0

        # Original model should be unchanged
        assert project.multi_loop is False
        assert project.speed == 1.0

    def test_read_only_proxy_nested_attributes(self) -> None:
        """Test that read-only proxy works with nested attributes."""
        project = ProjectState()
        proxy = ReadOnlyStateProxy(project)

        # Should be able to read nested attributes
        assert len(proxy.sample_paths) == 216
        assert proxy.sample_paths[0] is None

        # Note: ReadOnlyStateProxy only prevents direct attribute assignment
        # It doesn't prevent modification of nested mutable objects
        # This is by design - the proxy protects the model attributes, not nested data


class TestUiStateComputedProperties:
    """Test UiState computed properties."""

    def test_pad_label_no_file(self, controller: AppController) -> None:
        """Test pad_label returns empty string when no file is loaded."""
        ui_state = UiState(controller)

        assert not ui_state.pads.label(0)
        assert not ui_state.pads.label(100)

    def test_pad_label_with_unix_path(self, controller: AppController) -> None:
        """Test pad_label returns basename for Unix paths."""
        ui_state = UiState(controller)
        controller.project.sample_paths[0] = "/home/user/samples/loop.wav"

        assert ui_state.pads.label(0) == "loop.wav"

    def test_pad_label_with_windows_path(self, controller: AppController) -> None:
        """Test pad_label returns basename for Windows paths."""
        ui_state = UiState(controller)
        controller.project.sample_paths[0] = r"C:\Users\user\samples\loop.wav"

        assert ui_state.pads.label(0) == "loop.wav"

    def test_is_pad_loaded_true(self, controller: AppController) -> None:
        """Test is_pad_loaded returns True when pad has sample."""
        ui_state = UiState(controller)
        controller.project.sample_paths[0] = "/path/to/sample.wav"

        assert ui_state.pads.is_loaded(0) is True

    def test_is_pad_loaded_false(self, controller: AppController) -> None:
        """Test is_pad_loaded returns False when pad has no sample."""
        ui_state = UiState(controller)

        assert ui_state.pads.is_loaded(0) is False

    def test_is_pad_active_true(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test is_pad_active returns True when pad is playing."""
        ui_state = UiState(controller)
        controller.project.sample_paths[0] = "/path/to/sample.wav"
        controller.transport.playback.trigger_pad(0)
        # Simulate the audio message that would update state
        msg = Mock()
        msg.sample_id.return_value = 0
        controller.transport.playback.handle_sample_started_message(msg)

        assert ui_state.pads.is_active(0) is True

    def test_is_pad_active_false(self, controller: AppController) -> None:
        """Test is_pad_active returns False when pad is not playing."""
        ui_state = UiState(controller)

        assert ui_state.pads.is_active(0) is False

    def test_is_pad_pressed_true(self, controller: AppController) -> None:
        """Test is_pad_pressed returns True when pad is pressed."""
        ui_state = UiState(controller)
        controller.session.pressed_pads[0] = True

        assert ui_state.pads.is_pressed(0) is True

    def test_is_pad_pressed_false(self, controller: AppController) -> None:
        """Test is_pad_pressed returns False when pad is not pressed."""
        ui_state = UiState(controller)

        assert ui_state.pads.is_pressed(0) is False

    def test_is_bank_selected_true(self, controller: AppController) -> None:
        """Test is_bank_selected returns True when bank is selected."""
        ui_state = UiState(controller)
        controller.project.selected_bank = 2

        assert ui_state.banks.is_selected(2) is True

    def test_is_bank_selected_false(self, controller: AppController) -> None:
        """Test is_bank_selected returns False when bank is not selected."""
        ui_state = UiState(controller)
        controller.project.selected_bank = 0

        assert ui_state.banks.is_selected(1) is False

    def test_pad_effective_bpm_prefers_manual(self, controller: AppController) -> None:
        ui_state = UiState(controller)
        controller.project.sample_analysis[0] = SampleAnalysis(
            bpm=123.4,
            key="C#m",
            beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0], bars=[0.0]),
        )

        assert ui_state.pads.effective_bpm(0) == 123.4

        controller.transport.bpm.set_manual_bpm(0, 120.0)
        assert ui_state.pads.effective_bpm(0) == 120.0

    def test_pad_effective_key_prefers_manual(self, controller: AppController) -> None:
        ui_state = UiState(controller)
        controller.project.sample_analysis[0] = SampleAnalysis(
            bpm=123.4,
            key="C#m",
            beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0], bars=[0.0]),
        )

        assert ui_state.pads.effective_key(0) == "C#m"

        controller.transport.pad.set_manual_key(0, "Gm")
        assert ui_state.pads.effective_key(0) == "Gm"


class TestAudioActions:
    """Test AudioActions delegation to controller."""

    def test_trigger_pad(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test trigger_pad delegates to controller."""
        audio_actions = AudioActions(controller)
        controller.project.sample_paths[0] = "/path/to/sample.wav"

        audio_actions.pads.trigger_pad(0)

        audio_engine_mock.return_value.play_sample.assert_called_once_with(0, 1.0)
        # Simulate the audio message that would update state
        msg = Mock()
        msg.sample_id.return_value = 0
        controller.transport.playback.handle_sample_started_message(msg)
        assert 0 in controller.session.active_sample_ids

    def test_stop_pad(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test stop_pad delegates to controller."""
        audio_actions = AudioActions(controller)
        controller.project.sample_paths[0] = "/path/to/sample.wav"
        controller.transport.playback.trigger_pad(0)
        # Simulate the audio message that would update state
        msg = Mock()
        msg.sample_id.return_value = 0
        controller.transport.playback.handle_sample_started_message(msg)

        audio_actions.pads.stop_pad(0)

        audio_engine_mock.return_value.stop_sample.assert_called_once_with(0)
        # Simulate the audio message that would update state
        msg2 = Mock()
        msg2.sample_id.return_value = 0
        controller.transport.playback.handle_sample_stopped_message(msg2)
        assert 0 not in controller.session.active_sample_ids

    def test_load_sample_async(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test load_sample_async delegates to controller."""
        audio_actions = AudioActions(controller)

        audio_actions.pads.load_sample_async(0, "/path/to/sample.wav")

        audio_engine_mock.return_value.load_sample_async.assert_called_once_with(
            0, "/path/to/sample.wav", run_analysis=True
        )
        assert controller.session.pending_sample_paths[0] == "/path/to/sample.wav"
        assert controller.project.sample_paths[0] is None

    def test_analyze_sample_async(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test analyze_sample_async delegates to controller."""
        audio_actions = AudioActions(controller)
        controller.project.sample_paths[0] = "/path/to/sample.wav"

        audio_actions.pads.analyze_sample_async(0)

        audio_engine_mock.return_value.analyze_sample_async.assert_called_once_with(0)
        assert 0 in controller.session.analyzing_sample_ids

    def test_set_and_clear_manual_bpm(self, controller: AppController) -> None:
        audio_actions = AudioActions(controller)

        audio_actions.pads.set_manual_bpm(0, 120.0)
        assert controller.project.manual_bpm[0] == 120.0

        audio_actions.pads.clear_manual_bpm(0)
        assert controller.project.manual_bpm[0] is None

    def test_set_and_clear_manual_key(self, controller: AppController) -> None:
        audio_actions = AudioActions(controller)

        audio_actions.pads.set_manual_key(0, "Gm")
        assert controller.project.manual_key[0] == "Gm"

        audio_actions.pads.clear_manual_key(0)
        assert controller.project.manual_key[0] is None

    def test_tap_bpm_delegates(
        self, controller: AppController, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        audio_actions = AudioActions(controller)
        times = iter([0.0, 0.5, 1.0])
        mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
        monkeypatch.setattr(mp_target, lambda: next(times))

        assert audio_actions.pads.tap_bpm(0) is None
        assert audio_actions.pads.tap_bpm(0) is None
        assert audio_actions.pads.tap_bpm(0) == pytest.approx(120.0, abs=0.01)

    def test_unload_sample(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test unload_sample delegates to controller."""
        audio_actions = AudioActions(controller)
        controller.project.sample_paths[0] = "/path/to/sample.wav"

        audio_actions.pads.unload_sample(0)

        audio_engine_mock.return_value.unload_sample.assert_called_once_with(0)
        assert controller.project.sample_paths[0] is None

    def test_set_volume(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test set_volume delegates to controller."""
        audio_actions = AudioActions(controller)

        audio_actions.global_.set_volume(0.8)

        audio_engine_mock.return_value.set_volume.assert_called_once()
        assert controller.project.volume == 0.8

    def test_set_speed(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test set_speed delegates to controller."""
        audio_actions = AudioActions(controller)

        audio_actions.global_.set_speed(1.5)

        audio_engine_mock.return_value.set_speed.assert_called_once()
        assert controller.project.speed == 1.5

    def test_reset_speed(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test reset_speed delegates to controller."""
        audio_actions = AudioActions(controller)
        controller.project.speed = 1.8

        audio_actions.global_.reset_speed()

        # reset_speed should call set_speed with 1.0
        audio_engine_mock.return_value.set_speed.assert_called()
        assert controller.project.speed == 1.0

    def test_increase_speed(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test increase_speed increases speed by SPEED_STEP."""
        audio_actions = AudioActions(controller)
        initial_speed = controller.project.speed

        audio_actions.global_.increase_speed()

        expected_speed = initial_speed + SPEED_STEP
        audio_engine_mock.return_value.set_speed.assert_called_once()
        assert controller.project.speed == expected_speed

    def test_decrease_speed(self, controller: AppController, audio_engine_mock: Mock) -> None:
        """Test decrease_speed decreases speed by SPEED_STEP."""
        audio_actions = AudioActions(controller)
        controller.project.speed = 1.5
        initial_speed = controller.project.speed

        audio_actions.global_.decrease_speed()

        expected_speed = initial_speed - SPEED_STEP
        audio_engine_mock.return_value.set_speed.assert_called_once()
        assert controller.project.speed == expected_speed

    def test_toggle_multi_loop(self, controller: AppController) -> None:
        """Test toggle_multi_loop toggles multi_loop mode."""
        audio_actions = AudioActions(controller)
        initial_state = controller.project.multi_loop

        audio_actions.global_.toggle_multi_loop()

        assert controller.project.multi_loop is not initial_state

    def test_toggle_key_lock(self, controller: AppController) -> None:
        """Test toggle_key_lock toggles key_lock mode."""
        audio_actions = AudioActions(controller)
        initial_state = controller.project.key_lock

        audio_actions.global_.toggle_key_lock()

        assert controller.project.key_lock is not initial_state

    def test_toggle_bpm_lock(self, controller: AppController) -> None:
        """Test toggle_bpm_lock toggles bpm_lock mode."""
        audio_actions = AudioActions(controller)
        initial_state = controller.project.bpm_lock

        audio_actions.global_.toggle_bpm_lock()

        assert controller.project.bpm_lock is not initial_state


class TestUiActions:
    """Test UiActions state mutations."""

    def test_toggle_left_sidebar(self, controller: AppController) -> None:
        """Test toggle_left_sidebar toggles sidebar state."""
        ui_actions = UiActions(controller)
        initial_state = controller.project.sidebar_left_expanded

        ui_actions.toggle_left_sidebar()

        assert controller.project.sidebar_left_expanded is not initial_state

    def test_toggle_right_sidebar(self, controller: AppController) -> None:
        """Test toggle_right_sidebar toggles sidebar state."""
        ui_actions = UiActions(controller)
        initial_state = controller.project.sidebar_right_expanded

        ui_actions.toggle_right_sidebar()

        assert controller.project.sidebar_right_expanded is not initial_state

    def test_open_file_dialog(self, controller: AppController) -> None:
        """Test open_file_dialog sets file_dialog_pad_id."""
        ui_actions = UiActions(controller)

        ui_actions.open_file_dialog(5)

        assert controller.session.file_dialog_pad_id == 5

    def test_close_file_dialog(self, controller: AppController) -> None:
        """Test close_file_dialog clears file_dialog_pad_id."""
        ui_actions = UiActions(controller)
        controller.session.file_dialog_pad_id = 5

        ui_actions.close_file_dialog()

        assert controller.session.file_dialog_pad_id is None

    def test_select_pad(self, controller: AppController) -> None:
        """Test select_pad sets selected_pad."""
        ui_actions = UiActions(controller)

        ui_actions.select_pad(10)

        assert controller.project.selected_pad == 10

    def test_select_bank(self, controller: AppController) -> None:
        """Test select_bank sets selected_bank."""
        ui_actions = UiActions(controller)

        ui_actions.select_bank(3)

        assert controller.project.selected_bank == 3

    def test_store_pressed_pad_state_true(self, controller: AppController) -> None:
        """Test store_pressed_pad_state sets pad pressed state to True."""
        ui_actions = UiActions(controller)

        ui_actions.store_pressed_pad_state(5, pressed=True)

        assert controller.session.pressed_pads[5] is True

    def test_store_pressed_pad_state_false(self, controller: AppController) -> None:
        """Test store_pressed_pad_state sets pad pressed state to False."""
        ui_actions = UiActions(controller)
        controller.session.pressed_pads[5] = True

        ui_actions.store_pressed_pad_state(5, pressed=False)

        assert controller.session.pressed_pads[5] is False


class TestUiContext:
    """Test UiContext initialization and access."""

    def test_ui_context_initialization(self, controller: AppController) -> None:
        """Test UiContext initializes with controller."""
        ui_context = UiContext(controller)

        assert ui_context._controller is controller
        assert isinstance(ui_context.state, UiState)
        assert isinstance(ui_context.audio, AudioActions)
        assert isinstance(ui_context.ui, UiActions)

    def test_ui_context_provides_access_to_components(self, controller: AppController) -> None:
        """Test UiContext provides access to all UI components."""
        ui_context = UiContext(controller)

        # Should be able to access state
        assert ui_context.state is not None

        # Should be able to access audio actions
        assert ui_context.audio is not None

        # Should be able to access UI actions
        assert ui_context.ui is not None
