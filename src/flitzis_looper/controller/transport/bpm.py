import math
from time import monotonic
from typing import TYPE_CHECKING

from flitzis_looper.controller.current_timing import (
    UNRESOLVED_TIMING,
    CurrentPadTiming,
    UnresolvedTiming,
    current_accepted_timing,
)
from flitzis_looper.controller.scalar_grid import beat_duration_s
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
        # Failed admission must leave both the current accepted record and intent intact.
        self._audio.set_pad_bpm(sample_id, float(bpm))
        self._project.manual_bpm[sample_id] = float(bpm)
        self.on_pad_bpm_changed(sample_id, publish_bpm=False)
        self._audio.set_pad_timing_intent(sample_id, "manual")
        self._transport._mark_project_changed()

    def clear_manual_bpm(self, sample_id: int) -> None:
        """Clear a pad's manual BPM override."""
        validate_sample_id(sample_id)
        analysis = self._project.sample_analysis[sample_id]
        self._audio.set_pad_bpm(sample_id, normalize_bpm(analysis.bpm if analysis else None))
        self._project.manual_bpm[sample_id] = None
        self.on_pad_bpm_changed(sample_id, publish_bpm=False)
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

        self._audio.set_pad_bpm(sample_id, bpm)
        self._project.manual_bpm[sample_id] = bpm
        self.on_pad_bpm_changed(sample_id, publish_bpm=False)
        self._audio.set_pad_timing_intent(sample_id, "tap")
        self._transport._mark_project_changed()
        return bpm

    def effective_bpm(self, sample_id: int) -> float | None:
        """Return display BPM, preferring manual then current accepted timing."""
        validate_sample_id(sample_id)

        manual = self._project.manual_bpm[sample_id]
        if manual is not None:
            return float(manual)

        timing = self.current_timing(sample_id)
        if timing is not None and timing.accepted_revision is not None:
            return timing.bpm
        if timing is None and self._audio.pad_timing_intent(sample_id) == "automatic":
            return None
        return self._legacy_bpm(sample_id)

    def _legacy_bpm(self, sample_id: int) -> float | None:
        manual = self._project.manual_bpm[sample_id]
        if manual is not None:
            return float(manual)
        analysis = self._project.sample_analysis[sample_id]
        return analysis.bpm if analysis is not None else None

    def current_timing(self, sample_id: int) -> CurrentPadTiming | None:
        """Resolve one current period/origin/identity snapshot without caching history."""
        validate_sample_id(sample_id)
        manual = self._project.manual_bpm[sample_id]
        if manual is None:
            metadata = self._audio.current_constant_timing(sample_id)
            if metadata is not None:
                return current_accepted_timing(metadata, sample_id=sample_id)
            if self._audio.pad_timing_intent(sample_id) == "automatic":
                return None
        analysis = self._project.sample_analysis[sample_id]
        period = beat_duration_s(
            manual if manual is not None else analysis.bpm if analysis else None
        )
        if period is None:
            return None
        return CurrentPadTiming(
            period_seconds=period,
            origin_seconds=self._transport.loop._legacy_grid_anchor_sec(sample_id),
            sample_rate_hz=self._transport._output_sample_rate_hz(),
        )

    def recompute_master_bpm(
        self,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
        publish_master: bool = True,
    ) -> None:
        """Publish master timing using one resolved anchor snapshot."""
        if not self._project.bpm_lock:
            self._session.master_bpm = None
            self._session.master_period_seconds = None
            self._session.bpm_lock_anchor_revision = None
            return

        anchor_pad_id = self._session.bpm_lock_anchor_pad_id
        if isinstance(timing, UnresolvedTiming):
            timing = self.current_timing(anchor_pad_id) if anchor_pad_id is not None else None
        if (
            anchor_pad_id is not None
            and timing is None
            and self._audio.pad_timing_intent(anchor_pad_id) == "automatic"
        ):
            # No acknowledgement is not permission to replace accepted master timing.
            return
        anchor_bpm = normalize_bpm(
            self._legacy_bpm(anchor_pad_id)
            if anchor_pad_id is not None and (timing is None or timing.accepted_revision is None)
            else timing.bpm
            if timing is not None
            else self._session.bpm_lock_anchor_bpm
        )
        if anchor_bpm is None:
            self._session.master_bpm = None
            self._session.master_period_seconds = None
            self._session.bpm_lock_anchor_revision = None
            return

        period = timing.period_seconds if timing is not None else 60.0 / anchor_bpm
        master_period = period / self._project.speed
        master_bpm = (
            60.0 / master_period
            if timing is not None and timing.accepted_revision is not None
            else anchor_bpm * self._project.speed
        )
        if timing is not None and timing.accepted_revision is not None:
            if publish_master:
                self._audio.set_master_period(master_period)
        else:
            if publish_master:
                self._audio.set_master_bpm(master_bpm)
            master_period = 60.0 / master_bpm
        self._session.master_bpm = master_bpm
        self._session.master_period_seconds = master_period
        self._session.bpm_lock_anchor_bpm = anchor_bpm
        self._session.bpm_lock_anchor_revision = timing.accepted_revision if timing else None
        if anchor_pad_id is not None:
            self._audio.bootstrap_transport_from_pad(anchor_pad_id)

    def on_pad_bpm_changed(self, sample_id: int, *, publish_bpm: bool = True) -> None:
        """Refresh derived controls without replaying legacy values over accepted timing."""
        timing = self.current_timing(sample_id)
        if (
            timing is None
            and self._project.manual_bpm[sample_id] is None
            and self._audio.pad_timing_intent(sample_id) == "automatic"
        ):
            return
        bpm = normalize_bpm(
            timing.bpm
            if timing is not None and timing.accepted_revision is not None
            else self._legacy_bpm(sample_id)
        )
        if publish_bpm and (timing is None or timing.accepted_revision is None):
            self._audio.set_pad_bpm(sample_id, bpm)

        # Grid offset clamp depends on effective BPM, so re-clamp on changes.
        self._transport.loop.reclamp_grid_offset_samples(sample_id, timing=timing)
        self._transport.loop.apply_grid_anchor_to_audio(sample_id, timing=timing)
        self._transport.loop._apply_effective_pad_loop_region_to_audio(sample_id, timing=timing)

        if publish_bpm and self._project.manual_bpm[sample_id] is not None:
            self._audio.set_pad_timing_intent(sample_id, "manual")

        if self._session.bpm_lock_anchor_pad_id != sample_id:
            return

        self._session.bpm_lock_anchor_bpm = bpm
        self.recompute_master_bpm(timing=timing)


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
