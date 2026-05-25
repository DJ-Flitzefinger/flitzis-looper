from typing import TYPE_CHECKING, cast

from flitzis_looper.constants import (
    DEFAULT_KEY_LOCK_DELAY_MIN_SAMPLES,
    DEFAULT_KEY_LOCK_DELAY_RANGE_SAMPLES,
    DEFAULT_KEY_LOCK_HEAD_COUNT,
    DEFAULT_KEY_LOCK_INTERPOLATION,
    DEFAULT_KEY_LOCK_OUTPUT_GAIN,
    DEFAULT_KEY_LOCK_SMOOTHING_STEP,
    DEFAULT_KEY_LOCK_WINDOW,
    MAX_KEY_LOCK_DELAY_MIN_SAMPLES,
    MAX_KEY_LOCK_DELAY_RANGE_SAMPLES,
    MAX_KEY_LOCK_DELAY_TOTAL_SAMPLES,
    MAX_KEY_LOCK_HEAD_COUNT,
    MAX_KEY_LOCK_OUTPUT_GAIN,
    MAX_KEY_LOCK_SMOOTHING_STEP,
    MIN_KEY_LOCK_DELAY_MIN_SAMPLES,
    MIN_KEY_LOCK_DELAY_RANGE_SAMPLES,
    MIN_KEY_LOCK_HEAD_COUNT,
    MIN_KEY_LOCK_OUTPUT_GAIN,
    MIN_KEY_LOCK_SMOOTHING_STEP,
    PITCH_BPM_STEP,
    SPEED_MAX,
    SPEED_MIN,
    SPEED_STEP,
    VOLUME_MAX,
    VOLUME_MIN,
)
from flitzis_looper.controller.validation import ensure_finite, normalize_bpm
from flitzis_looper.models import (
    KEY_LOCK_INTERPOLATIONS,
    KEY_LOCK_QUALITIES,
    KEY_LOCK_WINDOWS,
    LEGACY_TRIGGER_QUANTIZATION_TO_STEP,
    TRIGGER_QUANTIZATION_STEPS,
)

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper.models import (
        KeyLockInterpolation,
        KeyLockQuality,
        KeyLockWindow,
        TriggerQuantizationMode,
        TriggerQuantizationStep,
    )


def _bounded_float(name: str, value: float, minimum: float, maximum: float) -> float:
    ensure_finite(value)
    value = float(value)
    if not minimum <= value <= maximum:
        msg = f"{name} must be between {minimum} and {maximum}"
        raise ValueError(msg)
    return value


def _key_lock_parameters_from_quality(quality: KeyLockQuality) -> dict[str, object]:
    match quality:
        case "performance":
            return {
                "delay_min_samples": 48.0,
                "delay_range_samples": 1024.0,
                "head_count": 2,
                "interpolation": "linear",
                "window": "triangle",
                "smoothing_step": 0.08,
                "output_gain": 1.0,
            }
        case "balanced":
            return {
                "delay_min_samples": 64.0,
                "delay_range_samples": 1280.0,
                "head_count": 2,
                "interpolation": "linear",
                "window": "hann",
                "smoothing_step": 0.06,
                "output_gain": 1.0,
            }
        case "very_high":
            return {
                "delay_min_samples": 96.0,
                "delay_range_samples": 1792.0,
                "head_count": 4,
                "interpolation": "cubic",
                "window": "hann",
                "smoothing_step": 0.035,
                "output_gain": 1.0,
            }
        case _:
            return {
                "delay_min_samples": DEFAULT_KEY_LOCK_DELAY_MIN_SAMPLES,
                "delay_range_samples": DEFAULT_KEY_LOCK_DELAY_RANGE_SAMPLES,
                "head_count": DEFAULT_KEY_LOCK_HEAD_COUNT,
                "interpolation": DEFAULT_KEY_LOCK_INTERPOLATION,
                "window": DEFAULT_KEY_LOCK_WINDOW,
                "smoothing_step": DEFAULT_KEY_LOCK_SMOOTHING_STEP,
                "output_gain": DEFAULT_KEY_LOCK_OUTPUT_GAIN,
            }


class GlobalParametersController:
    """Manage global playback modes/states (multi-loop, key lock, BPM lock, etc.)."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._audio = transport._audio
        self._bpm = transport.bpm

    def set_multi_loop(self, *, enabled: bool) -> None:
        """Enable or disable Multi Loop mode."""
        self._project.multi_loop = enabled
        self._transport._mark_project_changed()

    def set_key_lock(self, *, enabled: bool) -> None:
        """Enable or disable Key Lock mode."""
        if enabled == self._project.key_lock:
            return
        self._project.key_lock = enabled
        self._audio.set_key_lock(enabled=enabled)
        self._transport._mark_project_changed()

    def set_key_lock_quality(self, quality: KeyLockQuality | str) -> None:
        """Set legacy Key Lock quality by applying its concrete DSP parameters."""
        if quality not in KEY_LOCK_QUALITIES:
            msg = "key lock quality is unsupported"
            raise ValueError(msg)

        validated = cast("KeyLockQuality", quality)
        parameters = _key_lock_parameters_from_quality(validated)
        quality_changed = validated != self._project.key_lock_quality

        self._project.key_lock_quality = validated
        parameters_changed = self.set_key_lock_parameters(
            delay_min_samples=cast("float", parameters["delay_min_samples"]),
            delay_range_samples=cast("float", parameters["delay_range_samples"]),
            head_count=cast("int", parameters["head_count"]),
            interpolation=cast("KeyLockInterpolation", parameters["interpolation"]),
            window=cast("KeyLockWindow", parameters["window"]),
            smoothing_step=cast("float", parameters["smoothing_step"]),
            output_gain=cast("float", parameters["output_gain"]),
        )
        if quality_changed and not parameters_changed:
            self._transport._mark_project_changed()

    def set_key_lock_parameters(
        self,
        *,
        delay_min_samples: float,
        delay_range_samples: float,
        head_count: int,
        interpolation: KeyLockInterpolation | str,
        window: KeyLockWindow | str,
        smoothing_step: float,
        output_gain: float,
    ) -> bool:
        """Set the global manual Key Lock DSP parameters."""
        delay_min_samples = _bounded_float(
            "delay_min_samples",
            delay_min_samples,
            MIN_KEY_LOCK_DELAY_MIN_SAMPLES,
            MAX_KEY_LOCK_DELAY_MIN_SAMPLES,
        )
        delay_range_samples = _bounded_float(
            "delay_range_samples",
            delay_range_samples,
            MIN_KEY_LOCK_DELAY_RANGE_SAMPLES,
            MAX_KEY_LOCK_DELAY_RANGE_SAMPLES,
        )
        if delay_min_samples + delay_range_samples > MAX_KEY_LOCK_DELAY_TOTAL_SAMPLES:
            msg = (
                "delay_min_samples + delay_range_samples must be <= "
                f"{MAX_KEY_LOCK_DELAY_TOTAL_SAMPLES}"
            )
            raise ValueError(msg)
        if not MIN_KEY_LOCK_HEAD_COUNT <= head_count <= MAX_KEY_LOCK_HEAD_COUNT:
            msg = (
                f"head_count must be between {MIN_KEY_LOCK_HEAD_COUNT} "
                f"and {MAX_KEY_LOCK_HEAD_COUNT}"
            )
            raise ValueError(msg)
        if interpolation not in KEY_LOCK_INTERPOLATIONS:
            msg = "interpolation must be linear or cubic"
            raise ValueError(msg)
        if window not in KEY_LOCK_WINDOWS:
            msg = "window must be triangle or hann"
            raise ValueError(msg)
        smoothing_step = _bounded_float(
            "smoothing_step",
            smoothing_step,
            MIN_KEY_LOCK_SMOOTHING_STEP,
            MAX_KEY_LOCK_SMOOTHING_STEP,
        )
        output_gain = _bounded_float(
            "output_gain",
            output_gain,
            MIN_KEY_LOCK_OUTPUT_GAIN,
            MAX_KEY_LOCK_OUTPUT_GAIN,
        )

        validated_interpolation = cast("KeyLockInterpolation", interpolation)
        validated_window = cast("KeyLockWindow", window)
        changed = (
            delay_min_samples != self._project.key_lock_delay_min_samples
            or delay_range_samples != self._project.key_lock_delay_range_samples
            or int(head_count) != self._project.key_lock_head_count
            or validated_interpolation != self._project.key_lock_interpolation
            or validated_window != self._project.key_lock_window
            or smoothing_step != self._project.key_lock_smoothing_step
            or output_gain != self._project.key_lock_output_gain
        )
        if not changed:
            return False

        if delay_range_samples < self._project.key_lock_delay_range_samples:
            self._project.key_lock_delay_range_samples = delay_range_samples
            self._project.key_lock_delay_min_samples = delay_min_samples
        else:
            self._project.key_lock_delay_min_samples = delay_min_samples
            self._project.key_lock_delay_range_samples = delay_range_samples
        self._project.key_lock_head_count = int(head_count)
        self._project.key_lock_interpolation = validated_interpolation
        self._project.key_lock_window = validated_window
        self._project.key_lock_smoothing_step = smoothing_step
        self._project.key_lock_output_gain = output_gain
        self._audio.set_key_lock_parameters(
            delay_min_samples,
            delay_range_samples,
            int(head_count),
            validated_interpolation,
            validated_window,
            smoothing_step,
            output_gain,
        )
        self._transport._mark_project_changed()
        return True

    def set_bpm_lock(self, *, enabled: bool) -> None:
        """Enable or disable BPM Lock mode."""
        if enabled == self._project.bpm_lock:
            return

        self._project.bpm_lock = enabled
        self._transport._mark_project_changed()

        if enabled:
            anchor_pad_id = self._project.selected_pad
            anchor_bpm = normalize_bpm(self._bpm.effective_bpm(anchor_pad_id))
            self._session.bpm_lock_anchor_pad_id = anchor_pad_id
            self._session.bpm_lock_anchor_bpm = anchor_bpm
        else:
            self._session.bpm_lock_anchor_pad_id = None
            self._session.bpm_lock_anchor_bpm = None

        self._audio.set_bpm_lock(enabled=enabled)
        self._bpm.recompute_master_bpm()

    def _audio_trigger_quantization_mode(self) -> str:
        if not self._project.trigger_quantization_enabled:
            return "immediate"
        return self._project.trigger_quantization_step

    def _publish_trigger_quantization(self) -> None:
        self._audio.set_trigger_quantization(self._audio_trigger_quantization_mode())

    def set_trigger_quantization_enabled(self, *, enabled: bool) -> None:
        """Enable or disable global trigger quantization."""
        if enabled == self._project.trigger_quantization_enabled:
            return

        self._project.trigger_quantization_enabled = enabled
        self._publish_trigger_quantization()
        self._transport._mark_project_changed()

    def toggle_trigger_quantization(self) -> None:
        """Toggle global trigger quantization on or off."""
        self.set_trigger_quantization_enabled(
            enabled=not self._project.trigger_quantization_enabled
        )

    def set_trigger_quantization_step(self, step: TriggerQuantizationStep) -> None:
        """Set the global trigger quantization grid step."""
        if step == self._project.trigger_quantization_step:
            return

        self._project.trigger_quantization_step = step
        if self._project.trigger_quantization_enabled:
            self._publish_trigger_quantization()
        self._transport._mark_project_changed()

    def set_trigger_quantization(self, mode: TriggerQuantizationMode | str) -> None:
        """Set global trigger quantization from legacy mode strings."""
        if mode in {"immediate", "disabled", "off"}:
            self.set_trigger_quantization_enabled(enabled=False)
            return

        if mode == "enabled":
            self.set_trigger_quantization_enabled(enabled=True)
            return

        step = LEGACY_TRIGGER_QUANTIZATION_TO_STEP.get(str(mode))
        if step is None:
            if mode not in TRIGGER_QUANTIZATION_STEPS:
                msg = "trigger quantization mode is unsupported"
                raise ValueError(msg)
            step = cast("TriggerQuantizationStep", mode)

        changed = (
            not self._project.trigger_quantization_enabled
            or step != self._project.trigger_quantization_step
        )
        if not changed:
            return

        self._project.trigger_quantization_step = step
        self._project.trigger_quantization_enabled = True
        self._publish_trigger_quantization()
        self._transport._mark_project_changed()

    def set_volume(self, volume: float) -> None:
        """Set global volume."""
        ensure_finite(volume)
        clamped = min(max(volume, VOLUME_MIN), VOLUME_MAX)
        self._audio.set_volume(clamped)
        self._project.volume = clamped
        self._transport._mark_project_changed()

    def set_speed(self, speed: float) -> None:
        """Set global playback speed multiplier."""
        ensure_finite(speed)
        clamped = min(max(speed, SPEED_MIN), SPEED_MAX)
        self._audio.set_speed(clamped)
        self._project.speed = clamped
        self._bpm.recompute_master_bpm()
        self._transport._mark_project_changed()

    def speed_reference_bpm(self) -> float | None:
        """Return the BPM value represented by 1.00x speed for the current context."""
        speed = float(self._project.speed)
        if speed <= 0.0:
            return None

        if self._project.bpm_lock:
            master_bpm = normalize_bpm(self._session.master_bpm)
            if master_bpm is not None:
                return float(master_bpm) / speed

        return normalize_bpm(self._bpm.effective_bpm(self._project.selected_pad))

    def effective_display_bpm(self) -> float | None:
        """Return the BPM currently displayed above the global Pitch control."""
        reference_bpm = self.speed_reference_bpm()
        if reference_bpm is None:
            return None
        return float(reference_bpm) * float(self._project.speed)

    def set_effective_display_bpm(self, bpm: float) -> bool:
        """Set global speed by targeting a displayed BPM value."""
        ensure_finite(bpm)
        if bpm <= 0.0:
            return False

        reference_bpm = self.speed_reference_bpm()
        if reference_bpm is None:
            return False

        self.set_speed(float(bpm) / float(reference_bpm))
        return True

    def nudge_speed_by_bpm_step(self, direction: int) -> None:
        """Move Pitch by one BPM-grid step, falling back to multiplier steps without BPM."""
        if direction > 0:
            self.nudge_speed_by_bpm_steps(1)
        elif direction < 0:
            self.nudge_speed_by_bpm_steps(-1)

    def nudge_speed_by_bpm_steps(self, steps: int) -> None:
        """Move Pitch by signed BPM-grid steps, or multiplier steps without BPM."""
        if steps == 0:
            return

        current_bpm = self.effective_display_bpm()
        if current_bpm is None:
            self.set_speed(float(self._project.speed) + SPEED_STEP * steps)
            return

        self.set_effective_display_bpm(round(float(current_bpm) + PITCH_BPM_STEP * steps, 2))

    def reset_speed(self) -> None:
        """Reset global speed back to 1.0x."""
        self.set_speed(1.0)
