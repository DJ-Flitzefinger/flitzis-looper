import math
from time import monotonic
from typing import TYPE_CHECKING

from flitzis_looper.controller.validation import ensure_finite, normalize_bpm
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController


class BpmController:
    """Manage BPM overrides, tap detection, and master BPM computation."""

    _TAP_BPM_RESET_AFTER_S = 3.0

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
        if timestamps:
            elapsed_since_last_tap = now - timestamps[-1]
            if elapsed_since_last_tap <= 0:
                return None
            if elapsed_since_last_tap > self._TAP_BPM_RESET_AFTER_S:
                timestamps.clear()

        timestamps.append(now)

        if len(timestamps) < 2:
            return None

        avg_interval = _estimate_tap_interval_s(timestamps)
        if avg_interval is None:
            return None

        bpm = 60.0 / avg_interval
        if not math.isfinite(bpm):
            return None

        self._project.manual_bpm[sample_id] = bpm
        self.on_pad_bpm_changed(sample_id)
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

        # Grid offset clamp depends on effective BPM, so re-clamp on changes.
        self._transport.loop.reclamp_grid_offset_samples(sample_id)
        self._transport.loop.apply_grid_anchor_to_audio(sample_id)
        self._transport.loop._apply_effective_pad_loop_region_to_audio(sample_id)

        if self._session.bpm_lock_anchor_pad_id != sample_id:
            return

        self._session.bpm_lock_anchor_bpm = bpm
        self.recompute_master_bpm()


def _estimate_tap_interval_s(timestamps: list[float]) -> float | None:
    """Estimate the constant tap interval from all accepted tap timestamps."""
    count = len(timestamps)
    if count < 2:
        return None

    mean_index = (count - 1) / 2.0
    mean_time = sum(timestamps) / count
    denominator = 0.0
    numerator = 0.0
    for index, timestamp in enumerate(timestamps):
        index_offset = index - mean_index
        denominator += index_offset * index_offset
        numerator += index_offset * (float(timestamp) - mean_time)

    if denominator <= 0.0:
        return None

    interval_s = numerator / denominator
    if not math.isfinite(interval_s) or interval_s <= 0.0:
        return None

    return interval_s
