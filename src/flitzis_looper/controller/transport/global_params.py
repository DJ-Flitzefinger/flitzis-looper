from typing import TYPE_CHECKING, cast

from flitzis_looper.constants import (
    PITCH_BPM_STEP,
    SPEED_MAX,
    SPEED_MIN,
    SPEED_STEP,
    VOLUME_MAX,
    VOLUME_MIN,
)
from flitzis_looper.controller.current_timing import (
    UNRESOLVED_TIMING,
    CurrentPadTiming,
    UnresolvedTiming,
)
from flitzis_looper.controller.validation import ensure_finite, normalize_bpm
from flitzis_looper.models import (
    LEGACY_TRIGGER_QUANTIZATION_TO_STEP,
    TRIGGER_QUANTIZATION_STEPS,
)

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper.models import (
        TriggerQuantizationMode,
        TriggerQuantizationStep,
    )


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
        changed = enabled != self._project.key_lock
        self._project.key_lock = enabled

        for sample_id in range(len(self._project.pad_key_lock)):
            if self._project.sample_paths[sample_id] is None:
                if self._project.pad_key_lock[sample_id]:
                    self._project.pad_key_lock[sample_id] = False
                    changed = True
                continue

            if self._project.pad_key_lock[sample_id] is enabled:
                continue

            self._project.pad_key_lock[sample_id] = enabled
            self._audio.set_pad_key_lock(sample_id, enabled)
            changed = True

        if not changed:
            return

        self._transport._mark_project_changed()

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
            self._transport.loop.apply_grid_anchor_to_audio(anchor_pad_id)
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

    def set_momentary_output_mute(self, *, enabled: bool) -> None:
        """Temporarily mute engine output without changing persisted volume."""
        if enabled:
            if self._session.global_stop_momentary_mute_active:
                return
            self._audio.set_volume(VOLUME_MIN)
            self._session.global_stop_momentary_mute_active = True
            return

        if not self._session.global_stop_momentary_mute_active:
            return
        self._audio.set_volume(self._project.volume)
        self._session.global_stop_momentary_mute_active = False

    def set_speed(
        self,
        speed: float,
        *,
        anchor_timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> None:
        """Set global playback speed multiplier."""
        ensure_finite(speed)
        anchor_id = self._session.bpm_lock_anchor_pad_id
        if self._project.bpm_lock and anchor_id is not None:
            if isinstance(anchor_timing, UnresolvedTiming):
                anchor_timing = self._bpm.current_timing(anchor_id)
            if anchor_timing is None and self._audio.pad_timing_intent(anchor_id) == "automatic":
                msg = "current automatic anchor timing is not acknowledged"
                raise RuntimeError(msg)
        clamped = min(max(speed, SPEED_MIN), SPEED_MAX)
        accepted_anchor = (
            self._project.bpm_lock
            and isinstance(anchor_timing, CurrentPadTiming)
            and anchor_timing.accepted_revision is not None
        )
        if accepted_anchor and isinstance(anchor_timing, CurrentPadTiming):
            self._audio.set_speed_and_master_period(clamped, anchor_timing.period_seconds / clamped)
        else:
            self._audio.set_speed(clamped)
        self._project.speed = clamped
        self._transport._mark_project_changed()
        self._bpm.recompute_master_bpm(timing=anchor_timing, publish_master=not accepted_anchor)

    def speed_reference_bpm(self) -> float | None:
        """Return the BPM value represented by 1.00x speed for the current context."""
        period = self.speed_reference_period_seconds()
        return 60.0 / period if period is not None else None

    def speed_reference_period_seconds(self) -> float | None:
        """Return the current source quarter period for the global speed control."""
        period, _timing = self._speed_reference()
        return period

    def _speed_reference(
        self,
    ) -> tuple[float | None, CurrentPadTiming | UnresolvedTiming | None]:
        speed = float(self._project.speed)
        if speed <= 0.0:
            return None, UNRESOLVED_TIMING

        if self._project.bpm_lock:
            anchor_pad_id = self._session.bpm_lock_anchor_pad_id
            if anchor_pad_id is not None:
                timing = self._bpm.current_timing(anchor_pad_id)
                return (timing.period_seconds if timing is not None else None), timing
            master_bpm = normalize_bpm(self._session.master_bpm)
            if master_bpm is not None:
                return 60.0 / (float(master_bpm) / speed), UNRESOLVED_TIMING

        timing = self._bpm.current_timing(self._project.selected_pad)
        return (timing.period_seconds if timing is not None else None), UNRESOLVED_TIMING

    def effective_display_bpm(self) -> float | None:
        """Return the BPM currently displayed above the global Pitch control."""
        period = self.speed_reference_period_seconds()
        if period is None:
            return None
        return 60.0 / (period / float(self._project.speed))

    def set_effective_display_bpm(self, bpm: float) -> bool:
        """Set global speed by targeting a displayed BPM value."""
        ensure_finite(bpm)
        if bpm <= 0.0:
            return False

        period, timing = self._speed_reference()
        if period is None:
            return False

        self.set_speed(period * (float(bpm) / 60.0), anchor_timing=timing)
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

        period, timing = self._speed_reference()
        if period is None:
            self.set_speed(float(self._project.speed) + SPEED_STEP * steps, anchor_timing=timing)
            return

        current_bpm = 60.0 / (period / float(self._project.speed))
        target_bpm = round(current_bpm + PITCH_BPM_STEP * steps, 2)
        if target_bpm > 0.0:
            self.set_speed(period * (target_bpm / 60.0), anchor_timing=timing)

    def reset_speed(self) -> None:
        """Reset global speed back to 1.0x."""
        self.set_speed(1.0)
