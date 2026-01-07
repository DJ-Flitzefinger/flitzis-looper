import math
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.constants import SPEED_MAX, SPEED_MIN, VOLUME_MAX, VOLUME_MIN

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def test_set_volume_normal(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test setting volume within valid range."""
    volume = 0.7

    controller.transport.global_params.set_volume(volume)

    audio_engine_mock.return_value.set_volume.assert_called_with(volume)
    assert controller.project.volume == volume


def test_set_volume_clamps_max(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test volume is clamped to maximum."""
    volume = 2.0

    controller.transport.global_params.set_volume(volume)

    audio_engine_mock.return_value.set_volume.assert_called_with(VOLUME_MAX)
    assert controller.project.volume == VOLUME_MAX


def test_set_volume_clamps_min(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test volume is clamped to minimum."""
    volume = -1.0

    controller.transport.global_params.set_volume(volume)

    audio_engine_mock.return_value.set_volume.assert_called_with(VOLUME_MIN)
    assert controller.project.volume == VOLUME_MIN


def test_set_speed_normal(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test setting speed within valid range."""
    speed = 1.5

    controller.transport.global_params.set_speed(speed)

    audio_engine_mock.return_value.set_speed.assert_called_with(speed)
    assert controller.project.speed == speed


def test_set_speed_clamps_max(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test speed is clamped to maximum."""
    speed = 3.0

    controller.transport.global_params.set_speed(speed)

    audio_engine_mock.return_value.set_speed.assert_called_with(SPEED_MAX)
    assert controller.project.speed == SPEED_MAX


def test_set_speed_clamps_min(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test speed is clamped to minimum."""
    speed = 0.3

    controller.transport.global_params.set_speed(speed)

    audio_engine_mock.return_value.set_speed.assert_called_with(SPEED_MIN)
    assert controller.project.speed == SPEED_MIN


def test_reset_speed(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test resetting speed to 1.0."""
    controller.project.speed = 1.8

    controller.transport.global_params.reset_speed()

    audio_engine_mock.return_value.set_speed.assert_called_with(1.0)
    assert controller.project.speed == 1.0


def test_set_multi_loop_enable(controller: AppController) -> None:
    """Test enabling multi loop mode."""
    controller.project.multi_loop = False

    controller.transport.global_params.set_multi_loop(enabled=True)

    assert controller.project.multi_loop is True


def test_set_multi_loop_disable(controller: AppController) -> None:
    """Test disabling multi loop mode."""
    controller.project.multi_loop = True

    controller.transport.global_params.set_multi_loop(enabled=False)

    assert controller.project.multi_loop is False


def test_set_key_lock_enable(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test enabling key lock mode."""
    controller.project.key_lock = False

    controller.transport.global_params.set_key_lock(enabled=True)

    audio_engine_mock.return_value.set_key_lock.assert_called_with(enabled=True)
    assert controller.project.key_lock is True


def test_set_key_lock_disable(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test disabling key lock mode."""
    controller.project.key_lock = True

    controller.transport.global_params.set_key_lock(enabled=False)

    audio_engine_mock.return_value.set_key_lock.assert_called_with(enabled=False)
    assert controller.project.key_lock is False


def test_set_bpm_lock_enable(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test enabling BPM lock mode."""
    controller.project.bpm_lock = False

    controller.transport.global_params.set_bpm_lock(enabled=True)

    audio_engine_mock.return_value.set_bpm_lock.assert_called_with(enabled=True)
    assert controller.project.bpm_lock is True


def test_set_bpm_lock_disable(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test disabling BPM lock mode."""
    controller.project.bpm_lock = True

    controller.transport.global_params.set_bpm_lock(enabled=False)

    audio_engine_mock.return_value.set_bpm_lock.assert_called_with(enabled=False)
    assert controller.project.bpm_lock is False


def test_bpm_lock_anchors_master_bpm_to_selected_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.selected_pad = 1
    controller.transport.global_params.set_speed(1.5)
    controller.transport.bpm.set_manual_bpm(1, 120.0)

    audio_engine_mock.return_value.reset_mock()

    controller.transport.global_params.set_bpm_lock(enabled=True)

    assert controller.session.bpm_lock_anchor_pad_id == 1
    assert controller.session.master_bpm == pytest.approx(180.0)

    audio_engine_mock.return_value.set_bpm_lock.assert_called_with(enabled=True)
    assert audio_engine_mock.return_value.set_master_bpm.call_count == 1
    called_bpm = audio_engine_mock.return_value.set_master_bpm.call_args.args[0]
    assert called_bpm == pytest.approx(180.0)

    audio_engine_mock.return_value.reset_mock()

    controller.transport.global_params.set_speed(2.0)

    assert controller.session.master_bpm == pytest.approx(240.0)
    assert audio_engine_mock.return_value.set_master_bpm.call_count == 1
    called_bpm = audio_engine_mock.return_value.set_master_bpm.call_args.args[0]
    assert called_bpm == pytest.approx(240.0)


def test_non_finite_volume_nan(controller: AppController) -> None:
    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.global_params.set_volume(math.nan)


def test_non_finite_volume_inf(controller: AppController) -> None:
    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.global_params.set_volume(math.inf)


def test_non_finite_speed_nan(controller: AppController) -> None:
    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.global_params.set_speed(math.nan)


def test_non_finite_speed_inf(controller: AppController) -> None:
    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.global_params.set_speed(math.inf)
