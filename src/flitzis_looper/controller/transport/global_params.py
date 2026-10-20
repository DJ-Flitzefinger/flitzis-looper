import math
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, Literal, cast

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
from flitzis_looper.controller.transport.key_lock_status import KeyLockStatus
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


@dataclass(frozen=True, slots=True)
class GlobalRateStatus:
    """Separate requested settings from one coherent callback-owned rate observation."""

    requested_speed: float
    request_revision: int | None = None
    effect_revision: int | None = None
    state: Literal["queued", "preparing", "applied", "rejected"] | None = None
    requested_master_period: float | None = None
    effective_speed: float | None = None
    effective_master_period: float | None = None
    execution_frame: int | None = None
    error: str | None = None


def _rate_number(value: object, *, optional: bool = False) -> float | None:
    if value is None and optional:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        message = "Native rate confirmation has an invalid numeric setting."
        raise TypeError(message)
    try:
        number = float(value)
    except OverflowError as error:
        message = "Native rate confirmation has an invalid numeric setting."
        raise ValueError(message) from error
    if not math.isfinite(number) or number <= 0.0:
        message = "Native rate confirmation has an invalid numeric setting."
        raise ValueError(message)
    return number


def _rate_revision(value: object) -> int:
    if type(value) is not int:
        message = "Native rate confirmation has an invalid receipt revision."
        raise TypeError(message)
    if value < 0:
        message = "Native rate confirmation has an invalid receipt revision."
        raise ValueError(message)
    return value


def _rate_state(
    value: object, *, request: int, effect: int
) -> Literal["queued", "preparing", "applied", "rejected"]:
    if not isinstance(value, str):
        message = "Native rate confirmation has an invalid receipt state."
        raise TypeError(message)
    if value not in {"queued", "preparing", "applied", "rejected"}:
        message = "Native rate confirmation has an invalid receipt state."
        raise ValueError(message)
    if (value == "queued") != (effect != request):
        message = "Native rate confirmation has inconsistent receipt revisions."
        raise ValueError(message)
    return cast('Literal["queued", "preparing", "applied", "rejected"]', value)


def _rate_error(value: object, *, state: str) -> str | None:
    if value is not None and not isinstance(value, str):
        message = "Native rate confirmation has an invalid receipt error."
        raise TypeError(message)
    if (state == "rejected") != (value is not None) or (value is not None and not value):
        message = "Native rate confirmation has an inconsistent refusal reason."
        raise ValueError(message)
    return value


def _decode_rate_status(feedback: object, *, minimum_revision: int) -> GlobalRateStatus:
    if not isinstance(feedback, dict):
        message = "Native rate confirmation is unavailable."
        raise TypeError(message)
    values = cast("dict[str, object]", feedback)
    required = {
        "request_revision",
        "effect_revision",
        "state",
        "requested_speed",
        "requested_master_period",
        "effective_speed",
        "effective_master_period",
        "execution_frame",
        "error",
    }
    if not required.issubset(values):
        message = "Native rate confirmation has an incomplete receipt."
        raise ValueError(message)
    request = _rate_revision(values["request_revision"])
    effect = _rate_revision(values["effect_revision"])
    if request < minimum_revision:
        message = "Native rate confirmation returned an older request receipt."
        raise ValueError(message)
    if effect > request:
        message = "Native rate confirmation has an invalid receipt."
        raise ValueError(message)
    state = _rate_state(values["state"], request=request, effect=effect)
    frame_value = values["execution_frame"]
    frame = None if frame_value is None else _rate_revision(frame_value)
    if state == "queued" and frame is not None:
        message = "Native rate confirmation has an unconfirmed execution frame."
        raise ValueError(message)
    requested = _rate_number(values["requested_speed"])
    assert requested is not None
    return GlobalRateStatus(
        requested_speed=requested,
        request_revision=request,
        effect_revision=effect,
        state=state,
        requested_master_period=_rate_number(values["requested_master_period"], optional=True),
        effective_speed=_rate_number(values["effective_speed"]),
        effective_master_period=_rate_number(values["effective_master_period"], optional=True),
        execution_frame=frame,
        error=_rate_error(values["error"], state=state),
    )


class GlobalParametersController:
    """Manage global playback modes/states (multi-loop, key lock, BPM lock, etc.)."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._audio = transport._audio
        self._bpm = transport.bpm
        self._rate_status: GlobalRateStatus | None = None
        self._rate_status_error: str | None = None
        self._rate_request_unobserved = False
        self._rate_observed_revision = 0
        self._rate_request_after_revision = 0

    def _poll_global_rate_status(self) -> None:
        """Observe Native once on the normal runtime tick without publishing control intent."""
        observe = getattr(self._audio, "global_rate_status", None)
        self._rate_status = None
        self._rate_status_error = None
        if not callable(observe):
            self._rate_status_error = "Rate confirmation requires the current audio runtime."
            return
        try:
            feedback = observe()
            if feedback is None:
                return
            status = _decode_rate_status(feedback, minimum_revision=self._rate_observed_revision)
            revision = status.request_revision
            assert revision is not None
            self._rate_status = status
            self._rate_observed_revision = revision
            if revision > self._rate_request_after_revision:
                self._rate_request_unobserved = False
        except (RuntimeError, ValueError, TypeError) as error:
            self._rate_status_error = str(error)

    def global_rate_status(self) -> GlobalRateStatus:
        """Return cached audio confirmation independently of the persisted speed control."""
        status = self._rate_status
        if status is None:
            return GlobalRateStatus(
                requested_speed=float(self._project.speed), error=self._rate_status_error
            )
        if self._rate_request_unobserved:
            return replace(
                status,
                requested_speed=float(self._project.speed),
                request_revision=None,
                state="queued",
                requested_master_period=None,
                execution_frame=None,
                error=None,
            )
        return status

    def set_multi_loop(self, *, enabled: bool) -> None:
        """Enable or disable Multi Loop mode."""
        self._project.multi_loop = enabled
        self._transport._mark_project_changed()

    def set_key_lock(self, *, enabled: bool) -> None:
        """Change the default and publish only pads that inherit it."""
        self._transport.ensure_global_key_lock_edit_allowed()
        generations = self._capture_inherited_generations()
        changed = enabled != self._project.key_lock
        self._project.key_lock = enabled
        if self._transport._on_global_key_lock_changed is not None:
            self._transport._on_global_key_lock_changed()

        for sample_id in range(len(self._project.pad_key_lock)):
            if self._project.sample_paths[sample_id] is None:
                if self._project.pad_key_lock[sample_id] is not None:
                    self._project.pad_key_lock[sample_id] = None
                    changed = True
                continue
            if self._project.pad_key_lock[sample_id] is not None or sample_id not in generations:
                continue

            try:
                self._transport.residency.apply_key_lock(
                    sample_id, expected_source_generation=generations[sample_id]
                )
            except (RuntimeError, ValueError) as error:
                self._transport.residency.report_key_lock_error(sample_id, str(error))
            changed = True

        if not changed:
            return

        self._transport._mark_project_changed()

    def _capture_inherited_generations(self) -> dict[int, int | None]:
        generations: dict[int, int | None] = {}
        for sample_id, path in enumerate(self._project.sample_paths):
            if path is None or self._project.pad_key_lock[sample_id] is not None:
                continue
            provider = self._transport._incoming_load_request
            if provider is not None and provider(sample_id) is not None:
                # Only the request-bound resolver owns incoming Global inheritance.
                continue
            try:
                generations[sample_id] = self._transport.capture_current_pad_edit_generation(
                    sample_id
                )
            except (RuntimeError, ValueError) as error:
                self._transport.residency.report_key_lock_error(sample_id, str(error))
                continue
            self._transport.residency.remember(sample_id)
        return generations

    def key_lock_status(self) -> KeyLockStatus:
        """Summarize current per-target confirmations across all banks without rolling them back."""
        states = [
            self._transport.residency.key_lock_status(sample_id)
            for sample_id, path in enumerate(self._project.sample_paths)
            if path is not None
        ]
        modes = {state.effective for state in states}
        effective = next(iter(modes)) if len(modes) == 1 else None
        errors = [state.error for state in states if state.error]
        return KeyLockStatus(
            self._project.key_lock,
            effective,
            pending=any(state.pending for state in states),
            unconfirmed=any(state.unconfirmed for state in states),
            error=f"{len(errors)} pad(s): {errors[0]}" if errors else None,
            mixed=len(modes) > 1,
        )

    def set_bpm_lock(self, *, enabled: bool) -> None:
        """Enable or disable BPM Lock mode."""
        if enabled == self._project.bpm_lock:
            return
        if enabled:
            self._transport.ensure_pad_edit_allowed(self._project.selected_pad)

        anchor_pad_id = None
        anchor_bpm = None
        if enabled:
            anchor_pad_id = self._project.selected_pad
            anchor_bpm = normalize_bpm(self._bpm.effective_bpm(anchor_pad_id))
            self._transport.loop.apply_grid_anchor_to_audio(anchor_pad_id)

        self._audio.set_bpm_lock(enabled=enabled)
        self._project.bpm_lock = enabled
        self._session.bpm_lock_anchor_pad_id = anchor_pad_id
        self._session.bpm_lock_anchor_bpm = anchor_bpm
        self._transport._mark_project_changed()
        self._bpm.recompute_master_bpm()

    def _publish_trigger_quantization(
        self, *, enabled: bool, step: TriggerQuantizationStep
    ) -> None:
        self._audio.set_trigger_quantization(step if enabled else "immediate")

    def set_trigger_quantization_enabled(self, *, enabled: bool) -> None:
        """Enable or disable global trigger quantization."""
        if enabled == self._project.trigger_quantization_enabled:
            return

        self._publish_trigger_quantization(
            enabled=enabled, step=self._project.trigger_quantization_step
        )
        self._project.trigger_quantization_enabled = enabled
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

        if self._project.trigger_quantization_enabled:
            self._publish_trigger_quantization(enabled=True, step=step)
        self._project.trigger_quantization_step = step
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

        self._publish_trigger_quantization(enabled=True, step=step)
        self._project.trigger_quantization_step = step
        self._project.trigger_quantization_enabled = True
        self._transport._mark_project_changed()

    def set_volume(self, volume: float) -> None:
        """Set global volume."""
        ensure_finite(volume)
        clamped = min(max(volume, VOLUME_MIN), VOLUME_MAX)
        if not self._session.global_stop_momentary_mute_active:
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
            self._transport.ensure_pad_edit_allowed(anchor_id)
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
        self._rate_request_unobserved = True
        self._rate_request_after_revision = self._rate_observed_revision
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
        """Return requested BPM for the existing global Pitch target control."""
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
