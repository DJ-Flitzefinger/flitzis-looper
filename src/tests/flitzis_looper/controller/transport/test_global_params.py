import math
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.constants import (
    MIN_KEY_LOCK_SMOOTHING_STEP,
    PITCH_BPM_COARSE_STEPS,
    SPEED_MAX,
    SPEED_MIN,
    VOLUME_MAX,
    VOLUME_MIN,
)

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def test_set_volume_normal(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test setting volume within valid range."""
    volume = 0.7

    controller.transport.global_params.set_volume(volume)

    audio_engine_mock.set_volume.assert_called_with(volume)
    assert controller.project.volume == volume


def test_set_volume_clamps_max(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test volume is clamped to maximum."""
    volume = 2.0

    controller.transport.global_params.set_volume(volume)

    audio_engine_mock.set_volume.assert_called_with(VOLUME_MAX)
    assert controller.project.volume == VOLUME_MAX


def test_set_volume_clamps_min(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test volume is clamped to minimum."""
    volume = -1.0

    controller.transport.global_params.set_volume(volume)

    audio_engine_mock.set_volume.assert_called_with(VOLUME_MIN)
    assert controller.project.volume == VOLUME_MIN


def test_momentary_output_mute_does_not_change_project_volume(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.volume = 0.7
    audio_engine_mock.reset_mock()

    controller.transport.global_params.set_momentary_output_mute(enabled=True)

    audio_engine_mock.set_volume.assert_called_once_with(VOLUME_MIN)
    assert controller.project.volume == pytest.approx(0.7)
    assert controller.session.global_stop_momentary_mute_active is True

    audio_engine_mock.reset_mock()
    controller.transport.global_params.set_momentary_output_mute(enabled=False)

    audio_engine_mock.set_volume.assert_called_once_with(0.7)
    assert controller.project.volume == pytest.approx(0.7)
    assert controller.session.global_stop_momentary_mute_active is False


def test_momentary_output_mute_repeated_calls_are_noops(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.volume = 0.7

    controller.transport.global_params.set_momentary_output_mute(enabled=True)
    controller.transport.global_params.set_momentary_output_mute(enabled=True)

    audio_engine_mock.set_volume.assert_called_once_with(VOLUME_MIN)

    audio_engine_mock.reset_mock()
    controller.transport.global_params.set_momentary_output_mute(enabled=False)
    controller.transport.global_params.set_momentary_output_mute(enabled=False)

    audio_engine_mock.set_volume.assert_called_once_with(0.7)


def test_set_speed_normal(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test setting speed within valid range."""
    speed = 1.5

    controller.transport.global_params.set_speed(speed)

    audio_engine_mock.set_speed.assert_called_with(speed)
    assert controller.project.speed == speed


def test_set_speed_clamps_max(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test speed is clamped to maximum."""
    speed = 3.0

    controller.transport.global_params.set_speed(speed)

    audio_engine_mock.set_speed.assert_called_with(SPEED_MAX)
    assert controller.project.speed == SPEED_MAX


def test_set_speed_clamps_min(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test speed is clamped to minimum."""
    speed = 0.3

    controller.transport.global_params.set_speed(speed)

    audio_engine_mock.set_speed.assert_called_with(SPEED_MIN)
    assert controller.project.speed == SPEED_MIN


def test_reset_speed(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test resetting speed to 1.0."""
    controller.project.speed = 1.8

    controller.transport.global_params.reset_speed()

    audio_engine_mock.set_speed.assert_called_with(1.0)
    assert controller.project.speed == 1.0


def test_effective_display_bpm_uses_selected_pad_reference(
    controller: AppController,
) -> None:
    controller.project.selected_pad = 1
    controller.transport.bpm.set_manual_bpm(1, 120.0)
    controller.project.speed = 1.25

    assert controller.transport.global_params.speed_reference_bpm() == pytest.approx(120.0)
    assert controller.transport.global_params.effective_display_bpm() == pytest.approx(150.0)


def test_set_effective_display_bpm_converts_to_speed(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.project.selected_pad = 1
    controller.transport.bpm.set_manual_bpm(1, 120.0)

    changed = controller.transport.global_params.set_effective_display_bpm(120.1)

    assert changed is True
    expected_speed = 120.1 / 120.0
    audio_engine_mock.set_speed.assert_called_with(pytest.approx(expected_speed))
    assert controller.project.speed == pytest.approx(expected_speed)


def test_set_effective_display_bpm_ignores_missing_reference(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    changed = controller.transport.global_params.set_effective_display_bpm(120.1)

    assert changed is False
    audio_engine_mock.set_speed.assert_not_called()
    assert controller.project.speed == 1.0


def test_nudge_speed_by_bpm_step_uses_one_tenth_bpm(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.project.selected_pad = 1
    controller.transport.bpm.set_manual_bpm(1, 120.0)

    controller.transport.global_params.nudge_speed_by_bpm_step(1)

    expected_speed = 120.1 / 120.0
    audio_engine_mock.set_speed.assert_called_with(pytest.approx(expected_speed))
    assert controller.project.speed == pytest.approx(expected_speed)

    controller.transport.global_params.nudge_speed_by_bpm_step(-1)

    assert controller.project.speed == pytest.approx(1.0)


def test_nudge_speed_by_multiple_bpm_steps(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.project.selected_pad = 1
    controller.transport.bpm.set_manual_bpm(1, 120.0)

    controller.transport.global_params.nudge_speed_by_bpm_steps(3)

    expected_speed = 120.3 / 120.0
    audio_engine_mock.set_speed.assert_called_with(pytest.approx(expected_speed))
    assert controller.project.speed == pytest.approx(expected_speed)


def test_nudge_speed_by_coarse_bpm_steps(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.project.selected_pad = 1
    controller.transport.bpm.set_manual_bpm(1, 120.0)

    controller.transport.global_params.nudge_speed_by_bpm_steps(PITCH_BPM_COARSE_STEPS)

    expected_speed = 121.0 / 120.0
    audio_engine_mock.set_speed.assert_called_with(pytest.approx(expected_speed))
    assert controller.project.speed == pytest.approx(expected_speed)


def test_set_effective_display_bpm_uses_locked_master_reference(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.project.bpm_lock = True
    controller.project.speed = 1.5
    controller.session.master_bpm = 180.0

    changed = controller.transport.global_params.set_effective_display_bpm(180.1)

    assert changed is True
    expected_speed = 180.1 / 120.0
    audio_engine_mock.set_speed.assert_called_with(pytest.approx(expected_speed))
    assert controller.project.speed == pytest.approx(expected_speed)


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

    audio_engine_mock.set_key_lock.assert_called_with(enabled=True)
    assert controller.project.key_lock is True


def test_set_key_lock_disable(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test disabling key lock mode."""
    controller.project.key_lock = True

    controller.transport.global_params.set_key_lock(enabled=False)

    audio_engine_mock.set_key_lock.assert_called_with(enabled=False)
    assert controller.project.key_lock is False


def test_set_key_lock_quality_updates_project_and_audio(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.transport.global_params.set_key_lock_quality("very_high")

    assert controller.project.key_lock_quality == "very_high"
    assert controller.project.key_lock_delay_min_samples == pytest.approx(96.0)
    assert controller.project.key_lock_delay_range_samples == pytest.approx(1792.0)
    assert controller.project.key_lock_head_count == 4
    audio_engine_mock.set_key_lock_parameters.assert_called_once_with(
        96.0,
        1792.0,
        4,
        "cubic",
        "hann",
        0.035,
        1.0,
    )


def test_set_key_lock_parameters_updates_project_and_audio(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.transport.global_params.set_key_lock_parameters(
        delay_min_samples=128.0,
        delay_range_samples=1024.0,
        head_count=1,
        interpolation="linear",
        window="triangle",
        smoothing_step=0.099,
        output_gain=1.2,
    )

    assert controller.project.key_lock_delay_min_samples == pytest.approx(128.0)
    assert controller.project.key_lock_delay_range_samples == pytest.approx(1024.0)
    assert controller.project.key_lock_head_count == 1
    assert controller.project.key_lock_interpolation == "linear"
    assert controller.project.key_lock_window == "triangle"
    assert controller.project.key_lock_smoothing_step == pytest.approx(0.099)
    assert controller.project.key_lock_output_gain == pytest.approx(1.2)
    audio_engine_mock.set_key_lock_parameters.assert_called_once_with(
        128.0,
        1024.0,
        1,
        "linear",
        "triangle",
        0.099,
        1.2,
    )


def test_set_key_lock_parameters_clamps_float_boundary_noise(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.transport.global_params.set_key_lock_parameters(
        delay_min_samples=128.0,
        delay_range_samples=1024.0,
        head_count=1,
        interpolation="linear",
        window="triangle",
        smoothing_step=MIN_KEY_LOCK_SMOOTHING_STEP - 1.0e-9,
        output_gain=1.2,
    )

    assert controller.project.key_lock_smoothing_step == pytest.approx(MIN_KEY_LOCK_SMOOTHING_STEP)
    audio_engine_mock.set_key_lock_parameters.assert_called_once_with(
        128.0,
        1024.0,
        1,
        "linear",
        "triangle",
        MIN_KEY_LOCK_SMOOTHING_STEP,
        1.2,
    )


def test_set_key_lock_quality_rejects_invalid_value(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    with pytest.raises(ValueError, match="key lock quality"):
        controller.transport.global_params.set_key_lock_quality("ultra")

    assert controller.project.key_lock_quality == "high"
    audio_engine_mock.set_key_lock_parameters.assert_not_called()


def test_set_key_lock_parameters_rejects_invalid_value(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    with pytest.raises(ValueError, match="delay_min_samples"):
        controller.transport.global_params.set_key_lock_parameters(
            delay_min_samples=8.0,
            delay_range_samples=1024.0,
            head_count=2,
            interpolation="cubic",
            window="hann",
            smoothing_step=0.05,
            output_gain=1.0,
        )

    with pytest.raises(ValueError, match="must be <="):
        controller.transport.global_params.set_key_lock_parameters(
            delay_min_samples=512.0,
            delay_range_samples=1984.0,
            head_count=2,
            interpolation="cubic",
            window="hann",
            smoothing_step=0.05,
            output_gain=1.0,
        )

    with pytest.raises(ValueError, match="head_count"):
        controller.transport.global_params.set_key_lock_parameters(
            delay_min_samples=64.0,
            delay_range_samples=1536.0,
            head_count=0,
            interpolation="cubic",
            window="hann",
            smoothing_step=0.05,
            output_gain=1.0,
        )

    with pytest.raises(ValueError, match="smoothing_step"):
        controller.transport.global_params.set_key_lock_parameters(
            delay_min_samples=64.0,
            delay_range_samples=1536.0,
            head_count=2,
            interpolation="cubic",
            window="hann",
            smoothing_step=0.1,
            output_gain=1.0,
        )

    audio_engine_mock.set_key_lock_parameters.assert_not_called()


def test_set_bpm_lock_enable(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test enabling BPM lock mode."""
    controller.project.bpm_lock = False

    controller.transport.global_params.set_bpm_lock(enabled=True)

    audio_engine_mock.set_bpm_lock.assert_called_with(enabled=True)
    assert controller.project.bpm_lock is True


def test_set_bpm_lock_disable(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test disabling BPM lock mode."""
    controller.project.bpm_lock = True

    controller.transport.global_params.set_bpm_lock(enabled=False)

    audio_engine_mock.set_bpm_lock.assert_called_with(enabled=False)
    assert controller.project.bpm_lock is False


def test_bpm_lock_anchors_master_bpm_to_selected_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.selected_pad = 1
    controller.transport.global_params.set_speed(1.5)
    controller.transport.bpm.set_manual_bpm(1, 120.0)

    audio_engine_mock.reset_mock()

    controller.transport.global_params.set_bpm_lock(enabled=True)

    assert controller.session.bpm_lock_anchor_pad_id == 1
    assert controller.session.master_bpm == pytest.approx(180.0)

    audio_engine_mock.set_bpm_lock.assert_called_with(enabled=True)
    assert audio_engine_mock.set_master_bpm.call_count == 1
    called_bpm = audio_engine_mock.set_master_bpm.call_args.args[0]
    assert called_bpm == pytest.approx(180.0)
    audio_engine_mock.anchor_transport_phase_from_pad.assert_not_called()

    audio_engine_mock.reset_mock()

    controller.transport.global_params.set_speed(2.0)

    assert controller.session.master_bpm == pytest.approx(240.0)
    assert audio_engine_mock.set_master_bpm.call_count == 1
    called_bpm = audio_engine_mock.set_master_bpm.call_args.args[0]
    assert called_bpm == pytest.approx(240.0)
    audio_engine_mock.anchor_transport_phase_from_pad.assert_not_called()


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


def test_set_key_lock_no_op(controller: AppController, audio_engine_mock: Mock) -> None:
    controller.project.key_lock = True
    controller.project.selected_pad = 1
    controller.transport.bpm.set_manual_bpm(1, 120.0)

    audio_engine_mock.reset_mock()

    controller.transport.global_params.set_key_lock(enabled=True)

    assert controller.project.key_lock is True
    audio_engine_mock.set_key_lock.assert_not_called()


def test_set_bpm_lock_no_op(controller: AppController, audio_engine_mock: Mock) -> None:
    controller.project.bpm_lock = True
    controller.project.selected_pad = 1
    controller.transport.bpm.set_manual_bpm(1, 120.0)

    audio_engine_mock.reset_mock()

    controller.transport.global_params.set_bpm_lock(enabled=True)

    assert controller.project.bpm_lock is True
    audio_engine_mock.set_bpm_lock.assert_not_called()


def test_set_bpm_lock_none_effective_bpm(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.selected_pad = 1
    controller.project.sample_paths[1] = "samples/foo.wav"

    controller.transport.global_params.set_bpm_lock(enabled=True)

    assert controller.session.bpm_lock_anchor_pad_id == 1
    assert controller.session.bpm_lock_anchor_bpm is None


def test_set_bpm_lock_non_finite_effective_bpm(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.selected_pad = 1
    controller.project.sample_paths[1] = "samples/foo.wav"

    controller.transport.global_params.set_bpm_lock(enabled=True)

    assert controller.session.bpm_lock_anchor_pad_id == 1
    assert controller.session.bpm_lock_anchor_bpm is None


def test_set_bpm_lock_disable_clears_anchor(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.bpm_lock = True
    controller.project.selected_pad = 1
    controller.session.bpm_lock_anchor_pad_id = 1
    controller.session.bpm_lock_anchor_bpm = 120.0

    controller.transport.global_params.set_bpm_lock(enabled=False)

    assert controller.session.bpm_lock_anchor_pad_id is None
    assert controller.session.bpm_lock_anchor_bpm is None


def test_enable_trigger_quantization_updates_project_and_audio(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.transport.global_params.set_trigger_quantization_enabled(enabled=True)

    assert controller.project.trigger_quantization_enabled is True
    assert controller.project.trigger_quantization_step == "1_64"
    audio_engine_mock.set_trigger_quantization.assert_called_once_with("1_64")


def test_set_trigger_quantization_step_updates_audio_only_when_enabled(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.transport.global_params.set_trigger_quantization_step("1_32")

    assert controller.project.trigger_quantization_step == "1_32"
    audio_engine_mock.set_trigger_quantization.assert_not_called()

    controller.transport.global_params.set_trigger_quantization_enabled(enabled=True)
    audio_engine_mock.set_trigger_quantization.assert_called_once_with("1_32")

    audio_engine_mock.reset_mock()
    controller.transport.global_params.set_trigger_quantization_step("1_64")
    audio_engine_mock.set_trigger_quantization.assert_called_once_with("1_64")


def test_legacy_set_trigger_quantization_maps_to_new_state(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.transport.global_params.set_trigger_quantization("next_bar")

    assert controller.project.trigger_quantization_enabled is True
    assert controller.project.trigger_quantization_step == "1_16"
    audio_engine_mock.set_trigger_quantization.assert_called_once_with("1_16")


def test_set_trigger_quantization_no_op_for_current_enabled_grid(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.trigger_quantization_enabled = True
    controller.project.trigger_quantization_step = "1_16"

    controller.transport.global_params.set_trigger_quantization("next_beat")

    audio_engine_mock.set_trigger_quantization.assert_not_called()


def test_set_trigger_quantization_rejects_invalid_mode(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    with pytest.raises(ValueError, match="unsupported"):
        controller.transport.global_params.set_trigger_quantization("half_note")

    assert controller.project.trigger_quantization_enabled is False
    assert controller.project.trigger_quantization_step == "1_64"
    audio_engine_mock.set_trigger_quantization.assert_not_called()
