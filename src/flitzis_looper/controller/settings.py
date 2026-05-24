import math
from typing import TYPE_CHECKING

from flitzis_looper.constants import (
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
)
from flitzis_looper.controller.base import BaseController

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine


class SettingsController(BaseController):
    """Manage persistent non-audio settings."""

    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_project_changed: Callable[[], None] | None = None,
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)

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
            msg = (
                f"demucs shifts must be between {MIN_DEMUCS_SHIFTS} "
                f"and {MAX_DEMUCS_SHIFTS}"
            )
            raise ValueError(msg)
        if not (
            math.isfinite(overlap)
            and MIN_DEMUCS_OVERLAP <= overlap <= MAX_DEMUCS_OVERLAP
        ):
            msg = (
                f"demucs overlap must be between {MIN_DEMUCS_OVERLAP:g} "
                f"and {MAX_DEMUCS_OVERLAP:g}"
            )
            raise ValueError(msg)
