import math
from typing import TYPE_CHECKING

from flitzis_looper.bpm_range_options import BpmRangeOptions
from flitzis_looper.constants import (
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
)
from flitzis_looper.controller.base import BaseController
from flitzis_looper.models import KEY_LOCK_START_BEHAVIORS, STEM_SEPARATORS
from flitzis_looper.recording_options import RecordingOptions

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.models import (
        KeyLockStartBehavior,
        ProjectState,
        SessionState,
        StemSeparator,
    )
    from flitzis_looper_audio import AudioEngine


class SettingsController(BaseController):
    """Manage persistent global preferences through their authoritative control paths."""

    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_project_changed: Callable[[], None] | None = None,
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)
        self._on_residency_policy_changed: Callable[[], None] | None = None

    def can_change_key_lock_start_behavior(self) -> bool:
        """Read Native's admission condition, including retained and pending playback."""
        return self._audio.can_change_key_lock_start_behavior()

    def set_key_lock_start_behavior(self, behavior: KeyLockStartBehavior) -> bool:
        """Persist a launch policy only after Native atomically accepts it while stopped."""
        if behavior not in KEY_LOCK_START_BEHAVIORS:
            message = "Key Lock start behavior must be a supported launch policy"
            raise ValueError(message)
        # This call is the authoritative admission, even after a permissive UI read.
        # A refusal retains neither persistent intent nor a future-start request.
        if not self._audio.set_key_lock_start_behavior(behavior):
            return False
        if behavior != self._project.key_lock_start_behavior:
            self._project.key_lock_start_behavior = behavior
            self._mark_project_changed()
        return True

    def bind_residency_policy(self, callback: Callable[[], None]) -> None:
        """Refresh background interests after a validated persistent policy edit."""
        self._on_residency_policy_changed = callback

    def set_recording_options(self, options: RecordingOptions) -> None:
        """Replace validated future-take defaults without changing an existing take."""
        validated = RecordingOptions.model_validate(options)
        if validated == self._project.recording_options:
            return
        self._project.recording_options = validated
        self._mark_project_changed()

    def set_automatic_bpm_range(self, options: BpmRangeOptions) -> None:
        """Configure future Native admissions before persisting their immutable intent."""
        validated = BpmRangeOptions.model_validate(options)
        if validated == self._project.automatic_bpm_range:
            return
        self._audio.configure_automatic_bpm_range(
            validated.enabled, validated.minimum_bpm, validated.maximum_bpm
        )
        self._project.automatic_bpm_range = validated
        self._mark_project_changed()

    def set_preload_stem_loops_on_startup(self, *, enabled: bool) -> None:
        """Set deliberate component preload without changing any pad's mix preference."""
        if type(enabled) is not bool:
            message = "startup stem preload must be a boolean"
            raise TypeError(message)
        if enabled == self._project.preload_stem_loops_on_startup:
            return
        self._project.preload_stem_loops_on_startup = enabled
        self._mark_project_changed()
        if self._on_residency_policy_changed is not None:
            self._on_residency_policy_changed()

    def set_resident_pcm_budget_mib(self, value: int) -> None:
        """Retain the manual limit, applying it first only when manual mode is active."""
        if type(value) is not int:
            message = "resident PCM budget must be an integer"
            raise TypeError(message)
        if not 128 <= value <= 16384:
            message = "resident PCM budget must be between 128 and 16384 MiB"
            raise ValueError(message)
        if value == self._project.resident_pcm_budget_mib:
            return
        if not self._project.resident_pcm_budget_automatic:
            self._audio.set_resident_pcm_budget_mib(value)
        self._project.resident_pcm_budget_mib = value
        self._mark_project_changed()
        if (
            not self._project.resident_pcm_budget_automatic
            and self._on_residency_policy_changed is not None
        ):
            self._on_residency_policy_changed()

    def set_resident_pcm_budget_automatic(self, *, enabled: bool) -> None:
        """Apply admission mode before persisting it, retaining the optional manual limit."""
        if type(enabled) is not bool:
            message = "automatic resident PCM admission must be a boolean"
            raise TypeError(message)
        if enabled == self._project.resident_pcm_budget_automatic:
            return
        self._audio.set_resident_pcm_budget_mib(
            0 if enabled else self._project.resident_pcm_budget_mib
        )
        self._project.resident_pcm_budget_automatic = enabled
        self._mark_project_changed()
        if self._on_residency_policy_changed is not None:
            self._on_residency_policy_changed()

    def set_stem_separator(self, separator: StemSeparator) -> None:
        """Choose the separator/model for future jobs without changing existing stems."""
        if separator not in STEM_SEPARATORS:
            msg = "stem separator must be a supported separator/model identity"
            raise ValueError(msg)
        if separator == self._project.stem_separator:
            return
        self._project.stem_separator = separator
        self._mark_project_changed()

    def set_demucs_quality(self, *, shifts: int, overlap: float) -> None:
        """Set persistent Demucs stem-generation quality controls."""
        self._validate_demucs_quality(shifts=shifts, overlap=overlap)
        if shifts == self._project.demucs_shifts and overlap == self._project.demucs_overlap:
            return
        self._project.demucs_shifts = shifts
        self._project.demucs_overlap = overlap
        self._mark_project_changed()

    @staticmethod
    def _validate_demucs_quality(*, shifts: int, overlap: float) -> None:
        if not isinstance(shifts, int):
            msg = "demucs shifts must be an integer"
            raise TypeError(msg)
        if not MIN_DEMUCS_SHIFTS <= shifts <= MAX_DEMUCS_SHIFTS:
            msg = f"demucs shifts must be between {MIN_DEMUCS_SHIFTS} and {MAX_DEMUCS_SHIFTS}"
            raise ValueError(msg)
        if not (math.isfinite(overlap) and MIN_DEMUCS_OVERLAP <= overlap <= MAX_DEMUCS_OVERLAP):
            msg = (
                f"demucs overlap must be between {MIN_DEMUCS_OVERLAP:g} and {MAX_DEMUCS_OVERLAP:g}"
            )
            raise ValueError(msg)
