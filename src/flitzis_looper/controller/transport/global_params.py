from typing import TYPE_CHECKING

from flitzis_looper.constants import SPEED_MAX, SPEED_MIN, VOLUME_MAX, VOLUME_MIN
from flitzis_looper.controller.validation import ensure_finite, normalize_bpm

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController


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

    def reset_speed(self) -> None:
        """Reset global speed back to 1.0x."""
        self.set_speed(1.0)
