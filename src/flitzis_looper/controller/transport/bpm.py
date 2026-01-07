import math
from itertools import pairwise
from time import monotonic
from typing import TYPE_CHECKING

from flitzis_looper.controller.validation import ensure_finite, normalize_bpm
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController


class BpmController:
    """Manage BPM overrides, tap detection, and master BPM computation."""

    _TAP_BPM_WINDOW_SIZE = 5

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._audio = transport._audio

    def set_manual_bpm(self, sample_id: int, bpm: float) -> None:
        """Set a pad's manual BPM override."""
        validate_sample_id(sample_id)
        ensure_finite(bpm)
        if bpm <= 0:
            msg = f"bpm must be > 0, got {bpm!r}"
            raise ValueError(msg)
        self._project.manual_bpm[sample_id] = float(bpm)
        self.on_pad_bpm_changed(sample_id)
        self._transport._mark_project_changed()

    def clear_manual_bpm(self, sample_id: int) -> None:
        """Clear a pad's manual BPM override."""
        validate_sample_id(sample_id)
        self._project.manual_bpm[sample_id] = None
        self.on_pad_bpm_changed(sample_id)
        self._transport._mark_project_changed()

    def tap_bpm(self, sample_id: int) -> float | None:
        """Register a Tap BPM event and update manual BPM."""
        validate_sample_id(sample_id)

        now = monotonic()
        if self._session.tap_bpm_pad_id != sample_id:
            self._session.tap_bpm_pad_id = sample_id
            self._session.tap_bpm_timestamps.clear()

        timestamps = self._session.tap_bpm_timestamps
        if timestamps and now <= timestamps[-1]:
            return None

        timestamps.append(now)
        if len(timestamps) > self._TAP_BPM_WINDOW_SIZE:
            del timestamps[: -self._TAP_BPM_WINDOW_SIZE]

        if len(timestamps) < 3:
            return None

        intervals = [b - a for a, b in pairwise(timestamps)]
        avg_interval = sum(intervals) / len(intervals)
        if avg_interval <= 0:
            return None

        bpm = 60.0 / avg_interval
        if not math.isfinite(bpm):
            return None

        self._project.manual_bpm[sample_id] = bpm
        self._transport._mark_project_changed()
        return bpm

    def effective_bpm(self, sample_id: int) -> float | None:
        """Return the effective BPM for a pad (manual overrides detected)."""
        validate_sample_id(sample_id)

        manual = self._project.manual_bpm[sample_id]
        if manual is not None:
            return float(manual)

        analysis = self._project.sample_analysis[sample_id]
        return analysis.bpm if analysis is not None else None

    def recompute_master_bpm(self) -> None:
        if not self._project.bpm_lock:
            self._session.master_bpm = None
            return

        anchor_bpm = normalize_bpm(self._session.bpm_lock_anchor_bpm)
        if anchor_bpm is None:
            self._session.master_bpm = None
            return

        master_bpm = anchor_bpm * self._project.speed
        self._session.master_bpm = master_bpm
        self._audio.set_master_bpm(master_bpm)

    def on_pad_bpm_changed(self, sample_id: int) -> None:
        bpm = normalize_bpm(self.effective_bpm(sample_id))
        self._audio.set_pad_bpm(sample_id, bpm)

        if self._session.bpm_lock_anchor_pad_id != sample_id:
            return

        self._session.bpm_lock_anchor_bpm = bpm
        self.recompute_master_bpm()
