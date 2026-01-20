from typing import TYPE_CHECKING
from unittest.mock import Mock, patch

import pytest

from flitzis_looper.controller.transport.state import ApplyProjectState
from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController


@pytest.fixture
def apply_project_state(
    transport_controller: TransportController,
) -> ApplyProjectState:
    return ApplyProjectState(transport_controller)


def test_apply_project_state_initialization(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
) -> None:
    """Test ApplyProjectState stores transport reference."""
    assert apply_project_state._transport is transport_controller
    assert apply_project_state._project is transport_controller._project
    assert apply_project_state._session is transport_controller._session
    assert apply_project_state._bpm is transport_controller.bpm
    assert apply_project_state._global_modes is transport_controller.global_params
    assert apply_project_state._audio is transport_controller._audio


def test_apply_global_audio_settings_only_when_changed(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
    audio_engine_mock: Mock,
) -> None:
    """Test _apply_global_audio_settings only calls methods when values differ from defaults."""
    transport_controller._project.volume = 0.8
    transport_controller._project.speed = 1.2
    transport_controller._project.key_lock = True
    transport_controller._project.bpm_lock = False

    defaults = ProjectState()
    apply_project_state._apply_global_audio_settings(defaults)

    audio_engine_mock.set_volume.assert_called_once()
    audio_engine_mock.set_speed.assert_called_once()
    audio_engine_mock.set_key_lock.assert_called_once()
    audio_engine_mock.set_bpm_lock.assert_not_called()


def test_apply_global_audio_settings_calls_all_methods(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
    audio_engine_mock: Mock,
) -> None:
    """Test _apply_global_audio_settings calls all audio methods when values differ."""
    transport_controller._project.volume = 0.9
    transport_controller._project.speed = 0.5
    transport_controller._project.key_lock = True
    transport_controller._project.bpm_lock = True

    defaults = ProjectState()
    apply_project_state._apply_global_audio_settings(defaults)

    audio_engine_mock.set_volume.assert_called_once()
    audio_engine_mock.set_speed.assert_called_once()
    audio_engine_mock.set_key_lock.assert_called_once()
    audio_engine_mock.set_bpm_lock.assert_called_once()


def test_apply_per_pad_mixing_only_when_changed(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
    audio_engine_mock: Mock,
) -> None:
    """Test _apply_per_pad_mixing only updates pads with changed values."""
    transport_controller._project.pad_gain[0] = 0.9
    transport_controller._project.pad_gain[1] = 1.0

    defaults = ProjectState()
    apply_project_state._apply_per_pad_mixing(defaults)

    audio_engine_mock.set_pad_gain.assert_called_once()


def test_apply_per_pad_mixing_calls_gain_for_each_pad(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
    audio_engine_mock: Mock,
) -> None:
    """Test _apply_per_pad_mixing iterates over all pads for gain."""
    transport_controller._project.pad_gain[0] = 0.9
    transport_controller._project.pad_gain[1] = 0.8
    transport_controller._project.pad_gain[2] = 0.7

    defaults = ProjectState()
    apply_project_state._apply_per_pad_mixing(defaults)

    assert audio_engine_mock.set_pad_gain.call_count >= 3


def test_apply_per_pad_mixing_calls_eq_for_each_pad(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
    audio_engine_mock: Mock,
) -> None:
    """Test _apply_per_pad_mixing calls EQ for pads with changed EQ values."""
    transport_controller._project.pad_eq_low_db[0] = -3.0
    transport_controller._project.pad_eq_mid_db[0] = 0.0
    transport_controller._project.pad_eq_high_db[0] = 3.0

    defaults = ProjectState()
    apply_project_state._apply_per_pad_mixing(defaults)

    audio_engine_mock.set_pad_eq.assert_called_once()


def test_apply_pad_loop_regions_skips_unloaded_pads(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
) -> None:
    """Test _apply_pad_loop_regions skips pads with no sample loaded."""
    transport_controller._project.sample_paths[0] = None
    transport_controller._project.sample_paths[1] = "/path/to/sample.wav"
    transport_controller._project.pad_loop_start_s[1] = 1.0

    with patch.object(
        transport_controller.loop, "_apply_effective_pad_loop_region_to_audio", autospec=True
    ) as mock_method:
        mock_method.return_value = None

        defaults = ProjectState()
        apply_project_state._apply_pad_loop_regions(defaults)
        assert mock_method.called
        mock_method.assert_called_with(1)


def test_apply_pad_loop_regions_only_when_changed(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
) -> None:
    """Test _apply_pad_loop_regions only updates when loop settings differ from defaults."""
    transport_controller._project.sample_paths[0] = "/path/to/sample.wav"
    transport_controller._project.pad_loop_start_s[0] = 1.0
    transport_controller._project.pad_loop_end_s[0] = 3.0
    transport_controller._project.pad_loop_auto[0] = True

    with patch.object(
        transport_controller.loop, "_apply_effective_pad_loop_region_to_audio", autospec=True
    ) as mock_method:
        mock_method.return_value = None

        defaults = ProjectState()
        apply_project_state._apply_pad_loop_regions(defaults)
        assert mock_method.called


def test_apply_pad_loop_regions_applies_effective_region(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
) -> None:
    """Test _apply_pad_loop_regions delegates effective region application to loop controller."""
    transport_controller._project.sample_paths[0] = "/path/to/sample.wav"
    transport_controller._project.pad_loop_start_s[0] = 1.0

    with patch.object(
        transport_controller.loop, "_apply_effective_pad_loop_region_to_audio", autospec=True
    ) as mock_method:
        mock_method.return_value = None

        defaults = ProjectState()
        apply_project_state._apply_pad_loop_regions(defaults)
        mock_method.assert_called_once_with(0)


def test_apply_pad_bpm_settings_only_when_available(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
) -> None:
    """Test _apply_pad_bpm_settings only triggers updates when BPM data is available."""
    transport_controller._project.sample_paths[0] = "/path/to/sample.wav"
    transport_controller._project.manual_bpm[0] = 120.0

    with patch.object(transport_controller.bpm, "on_pad_bpm_changed", autospec=True) as mock_method:
        mock_method.return_value = None
        apply_project_state._apply_pad_bpm_settings()
        assert mock_method.called


def test_apply_pad_bpm_settings_triggers_bpm_update(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
) -> None:
    """Test _apply_pad_bpm_settings triggers on_pad_bpm_changed for pads with BPM data."""
    transport_controller._project.sample_paths[0] = "/path/to/sample.wav"
    transport_controller._project.sample_paths[1] = "/path/to/sample2.wav"
    transport_controller._project.manual_bpm[0] = 120.0
    transport_controller._project.manual_bpm[1] = None

    with patch.object(transport_controller.bpm, "on_pad_bpm_changed", autospec=True) as mock_method:
        mock_method.return_value = None
        apply_project_state._apply_pad_bpm_settings()
        assert mock_method.call_count >= 1


def test_apply_bpm_lock_settings_enabled(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
) -> None:
    """Test _apply_bpm_lock_settings sets anchor when BPM lock is enabled."""
    transport_controller._project.bpm_lock = True
    transport_controller._project.selected_pad = 0

    with patch.object(transport_controller.bpm, "effective_bpm", autospec=True) as mock_effective:
        mock_effective.return_value = 120.0

        apply_project_state._apply_bpm_lock_settings()

        assert transport_controller._session.bpm_lock_anchor_pad_id == 0
        assert transport_controller._session.bpm_lock_anchor_bpm == 120.0


def test_apply_bpm_lock_settings_disabled(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
) -> None:
    """Test _apply_bpm_lock_settings clears anchor when BPM lock is disabled."""
    transport_controller._project.bpm_lock = False

    apply_project_state._apply_bpm_lock_settings()

    assert transport_controller._session.bpm_lock_anchor_pad_id is None
    assert transport_controller._session.bpm_lock_anchor_bpm is None


def test_apply_bpm_lock_settings_triggers_recompute(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
) -> None:
    """Test _apply_bpm_lock_settings triggers master BPM recompute."""
    with patch.object(
        transport_controller.bpm, "recompute_master_bpm", autospec=True
    ) as mock_recompute:
        mock_recompute.return_value = None

        apply_project_state._apply_bpm_lock_settings()

        mock_recompute.assert_called_once()


def test_apply_project_state_with_defaults(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
    audio_engine_mock: Mock,
) -> None:
    """Test apply_project_state_to_audio with default project state."""
    apply_project_state.apply_project_state_to_audio()

    audio_engine_mock.set_volume.assert_not_called()
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_key_lock.assert_not_called()
    audio_engine_mock.set_bpm_lock.assert_not_called()


def test_apply_project_state_with_modified_state(
    transport_controller: TransportController,
    apply_project_state: ApplyProjectState,
    audio_engine_mock: Mock,
) -> None:
    """Test apply_project_state_to_audio with modified project state."""
    transport_controller._project.volume = 0.8
    transport_controller._project.speed = 0.5
    transport_controller._project.key_lock = True
    transport_controller._project.bpm_lock = True
    transport_controller._project.sample_paths[0] = "/path/to/sample.wav"
    transport_controller._project.pad_gain[0] = 0.9
    transport_controller._project.pad_loop_start_s[0] = 1.0
    transport_controller._project.pad_loop_auto[0] = True

    with patch.object(
        transport_controller.loop, "_apply_effective_pad_loop_region_to_audio", autospec=True
    ) as mock_loop:
        mock_loop.return_value = None
    with patch.object(transport_controller.bpm, "on_pad_bpm_changed", autospec=True) as mock_bpm:
        mock_bpm.return_value = None
    with patch.object(
        transport_controller.bpm, "recompute_master_bpm", autospec=True
    ) as mock_recompute:
        mock_recompute.return_value = None

        apply_project_state.apply_project_state_to_audio()

        audio_engine_mock.set_volume.assert_called_once()
        audio_engine_mock.set_speed.assert_called_once()
        audio_engine_mock.set_key_lock.assert_called_once()
        audio_engine_mock.set_bpm_lock.assert_called_once()
