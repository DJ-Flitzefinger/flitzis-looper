import math
from typing import TYPE_CHECKING

from flitzis_looper.controller.current_timing import (
    UNRESOLVED_TIMING,
    CurrentPadTiming,
    UnresolvedTiming,
)

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import WaveFormRenderData


class WaveformController:
    """Manage waveform editor."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._audio = transport._audio
        self._readiness: dict[int, tuple[str, str | None]] = {}
        self._view_revision = 0

    @property
    def view_revision(self) -> int:
        """Invalidate UI projection caches when any shared editor path releases a view."""
        return self._view_revision

    def open_editor(self, pad_id: int) -> None:
        """Open or retarget the single editor, releasing the previous source view."""
        session = self._transport._session
        if session.waveform_editor_pad_id is not None and session.waveform_editor_pad_id != pad_id:
            self.release_view(session.waveform_editor_pad_id)
        session.waveform_editor_open = True
        session.waveform_editor_pad_id = pad_id

    def close_editor(self) -> None:
        """Close the single editor and release its view without transport mutations."""
        session = self._transport._session
        if session.waveform_editor_pad_id is not None:
            self.release_view(session.waveform_editor_pad_id)
        session.waveform_editor_open = False
        session.waveform_editor_pad_id = None

    def toggle_editor(self, pad_id: int) -> None:
        """Use the same open/close authority for toolbar, sidebar, and mapped actions."""
        if self._transport._project.sample_paths[pad_id] is None:
            return
        session = self._transport._session
        if session.waveform_editor_open and session.waveform_editor_pad_id == pad_id:
            self.close_editor()
        else:
            self.open_editor(pad_id)

    def readiness(self, pad_id: int) -> tuple[str, str | None]:
        """Return full-source projection readiness without changing playback."""
        return self._readiness.get(pad_id, ("idle", None))

    def source_identity(self, pad_id: int) -> tuple[int, str, int, int] | None:
        """Read the assignment identity used to fence same-path editor caches."""
        identity = self._audio.waveform_source_identity(pad_id)
        return identity if isinstance(identity, tuple) else None

    def retry(self, pad_id: int) -> None:
        """Retry a failed editor read without seeking or changing timing."""
        self._audio.retry_waveform(pad_id)
        self._readiness[pad_id] = ("idle", None)

    def release_view(self, pad_id: int) -> None:
        """Invalidate editor work; its worker owns the reader until read return."""
        self._audio.retry_waveform(pad_id)
        self._readiness.pop(pad_id, None)
        self._view_revision += 1

    def get_render_data(
        self,
        pad_id: int,
        width_px: int,
        start_s: float,
        end_s: float,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> WaveFormRenderData | None:
        if not math.isfinite(start_s) or not math.isfinite(end_s) or end_s <= start_s:
            return None
        source_start_s = max(0.0, start_s)
        if isinstance(timing, UnresolvedTiming):
            timing = self._transport.bpm.current_timing(pad_id)
        duration_s = (
            timing.source_duration_seconds if isinstance(timing, CurrentPadTiming) else None
        )
        if duration_s is None:
            duration_s = self._transport._project.sample_durations[pad_id]
        source_end_s = min(end_s, duration_s) if duration_s is not None else end_s
        if source_end_s <= source_start_s:
            return None
        source_width_px = max(
            1, round(width_px * (source_end_s - source_start_s) / (end_s - start_s))
        )
        try:
            data = self._audio.get_waveform_render_data(
                pad_id, source_width_px, source_start_s, source_end_s
            )
            status = ("ready", None) if data is not None else self._audio.waveform_readiness(pad_id)
        except (RuntimeError, ValueError) as error:
            self._readiness[pad_id] = ("error", str(error))
            return None
        self._readiness[pad_id] = status
        return data
