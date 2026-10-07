import math
from dataclasses import replace
from typing import TYPE_CHECKING

from flitzis_looper.constants import (
    PAD_LOOP_BARS_DEFAULT,
    PAD_LOOP_BARS_GRANULARITY,
    PAD_LOOP_BARS_MIN,
)
from flitzis_looper.controller.current_timing import (
    UNRESOLVED_TIMING,
    CurrentPadTiming,
    UnresolvedTiming,
)
from flitzis_looper.controller.scalar_grid import (
    nearest_grid_source_s,
    physical_source_marker_s,
    scalar_source_grid,
)
from flitzis_looper.controller.timing_metadata import timing_anchor_sec_from_analysis
from flitzis_looper.controller.validation import ensure_finite
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController


class PadLoopController:
    """Per-pad loop region manipulation."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._bpm = transport.bpm
        self._project = transport._project
        self._audio = transport._audio

    def reset(self, sample_id: int) -> None:
        """Set a pad's loop region to the full loaded track.

        Kept as the current UI/backward-compatible entry point until the
        waveform editor labels this action as ALL.
        """
        self.set_full_track_region(sample_id)

    def initialize_loaded_pad_defaults(
        self, sample_id: int, detected_loop_start_s: float | None = None
    ) -> None:
        """Initialize a new track's loop and grid at the same source activity boundary."""
        validate_sample_id(sample_id)
        timing = self._bpm.current_timing(sample_id)
        anchor_s = self._loaded_activity_anchor_s(sample_id, detected_loop_start_s, timing=timing)
        start_s = anchor_s if anchor_s is not None else 0.0

        changed = (
            self._project.pad_loop_start_s[sample_id] != start_s
            or self._project.pad_loop_end_s[sample_id] is not None
            or not self._project.pad_loop_auto[sample_id]
            or self._project.pad_loop_bars[sample_id] != PAD_LOOP_BARS_DEFAULT
            or self._project.pad_grid_anchor_s[sample_id] != anchor_s
            or self._project.pad_grid_offset_samples[sample_id] != 0
        )

        self._project.pad_loop_start_s[sample_id] = start_s
        self._project.pad_loop_end_s[sample_id] = None
        self._project.pad_loop_auto[sample_id] = True
        self._project.pad_loop_bars[sample_id] = PAD_LOOP_BARS_DEFAULT
        self._project.pad_grid_anchor_s[sample_id] = anchor_s
        self._project.pad_grid_offset_samples[sample_id] = 0
        if changed:
            self._transport._mark_project_changed()

        # The new durable legacy origin is now the activity anchor, not the old snapshot's base.
        if timing is not None and timing.accepted_revision is None:
            timing = replace(
                timing,
                origin_seconds=self._legacy_grid_anchor_sec(
                    sample_id, sample_rate_hz=self._timing_sample_rate_hz(timing)
                ),
            )
        self.apply_grid_anchor_to_audio(sample_id, timing=timing)
        self._apply_effective_pad_loop_region_to_audio(sample_id, timing=timing)

    def _loaded_activity_anchor_s(
        self,
        sample_id: int,
        detected_loop_start_s: float | None,
        *,
        timing: CurrentPadTiming | None,
    ) -> float | None:
        if (
            detected_loop_start_s is None
            or not math.isfinite(detected_loop_start_s)
            or detected_loop_start_s < 0.0
        ):
            return None

        duration_s = self._project.sample_durations[sample_id]
        if (
            duration_s is None
            or not math.isfinite(duration_s)
            or duration_s <= 0.0
            or detected_loop_start_s >= duration_s
        ):
            return None

        # This candidate protects the source attack; musical snapping could trim it.
        start_s = physical_source_marker_s(
            detected_loop_start_s, sample_rate_hz=self._timing_sample_rate_hz(timing)
        )
        return start_s if start_s < duration_s else None

    def set_full_track_region(self, sample_id: int) -> None:
        """Store and publish an explicit full-track loop region for a loaded pad."""
        validate_sample_id(sample_id)

        if self._project.sample_paths[sample_id] is None:
            return

        timing = self._bpm.current_timing(sample_id)
        duration_s = self._source_duration_s(sample_id, timing)
        if duration_s is None or not math.isfinite(duration_s) or duration_s <= 0.0:
            return

        start_s = 0.0
        end_s = physical_source_marker_s(
            float(duration_s), sample_rate_hz=self._timing_sample_rate_hz(timing)
        )
        if end_s <= start_s:
            return

        changed = (
            self._project.pad_loop_start_s[sample_id] != start_s
            or self._project.pad_loop_end_s[sample_id] != end_s
            or self._project.pad_loop_auto[sample_id]
        )

        self._project.pad_loop_start_s[sample_id] = start_s
        self._project.pad_loop_end_s[sample_id] = end_s
        self._project.pad_loop_auto[sample_id] = False
        if changed:
            self._transport._mark_project_changed()

        self._apply_effective_pad_loop_region_to_audio(sample_id, timing=timing)

    def _resolve_timing(
        self, sample_id: int, timing: CurrentPadTiming | UnresolvedTiming | None
    ) -> CurrentPadTiming | None:
        return (
            timing
            if isinstance(timing, CurrentPadTiming) or timing is None
            else self._bpm.current_timing(sample_id)
        )

    def _apply_effective_pad_loop_region_to_audio(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> None:
        if self._project.sample_paths[sample_id] is None:
            return
        start_s, end_s = self._effective_pad_loop_region(sample_id, timing=timing)
        self._audio.set_pad_loop_region(sample_id, start_s, end_s)

    def apply_grid_anchor_to_audio(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> None:
        """Publish legacy origins without revoking current accepted timing."""
        validate_sample_id(sample_id)
        if self._project.sample_paths[sample_id] is None:
            return

        if (
            self._project.manual_bpm[sample_id] is None
            and self._audio.pad_timing_intent(sample_id) == "automatic"
        ):
            return
        timing = self._resolve_timing(sample_id, timing)
        if timing is not None and timing.accepted_revision is not None:
            return
        self._audio.set_pad_timing_metadata(sample_id, self._legacy_grid_anchor_sec(sample_id))

    def _grid_offset_samples(self, sample_id: int) -> int:
        return int(self._project.pad_grid_offset_samples[sample_id])

    def _base_grid_anchor_sec(self, sample_id: int) -> float:
        anchor_s = self._project.pad_grid_anchor_s[sample_id]
        if anchor_s is not None and math.isfinite(anchor_s) and anchor_s >= 0.0:
            return anchor_s
        return timing_anchor_sec_from_analysis(self._project.sample_analysis[sample_id])

    def _base_grid_anchor_sample(self, sample_id: int, *, sample_rate_hz: int) -> int:
        anchor_s = self._base_grid_anchor_sec(sample_id)
        frames = round(anchor_s * sample_rate_hz)
        if not isinstance(frames, int):
            return 0
        return max(frames, 0)

    def _bar_samples_for_grid_offset_clamp(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> int | None:
        timing = self._resolve_timing(sample_id, timing)
        if timing is None:
            return None

        sample_rate_hz = self._timing_sample_rate_hz(timing)
        if sample_rate_hz is None or sample_rate_hz <= 0:
            return None

        bar_sec = self._duration_s_for_bars(bars=1.0, period_seconds=timing.period_seconds)
        return max(0, round(bar_sec * sample_rate_hz))

    def _clamp_grid_offset_samples(
        self, sample_id: int, value: int, *, timing: CurrentPadTiming | None
    ) -> int:
        bar_samples = self._bar_samples_for_grid_offset_clamp(sample_id, timing=timing)
        if bar_samples is None:
            return int(value)

        return max(-bar_samples, min(bar_samples, int(value)))

    def reclamp_grid_offset_samples(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> bool:
        """Re-clamp the stored grid offset when effective BPM changes."""
        validate_sample_id(sample_id)

        timing = self._resolve_timing(sample_id, timing)
        current = int(self._project.pad_grid_offset_samples[sample_id])
        clamped = self._clamp_grid_offset_samples(sample_id, current, timing=timing)
        if clamped == current:
            return False

        self._project.pad_grid_offset_samples[sample_id] = clamped
        self._transport._mark_project_changed()
        self.apply_grid_anchor_to_audio(sample_id, timing=timing)
        self._apply_effective_pad_loop_region_to_audio(sample_id, timing=timing)
        return True

    def set_grid_offset_samples(self, sample_id: int, grid_offset_samples: int) -> None:
        validate_sample_id(sample_id)

        timing = self._bpm.current_timing(sample_id)
        grid_offset_samples = self._clamp_grid_offset_samples(
            sample_id, int(grid_offset_samples), timing=timing
        )

        if grid_offset_samples == self._project.pad_grid_offset_samples[sample_id]:
            return

        if self._project.sample_paths[sample_id] is not None:
            # An explicit offset edit is legacy intent. Successful native publication
            # revokes accepted authority; failed publication leaves saved intent intact.
            self._audio.set_pad_timing_metadata(
                sample_id,
                self._legacy_grid_anchor_sec(
                    sample_id,
                    sample_rate_hz=self._timing_sample_rate_hz(timing),
                    grid_offset_samples=grid_offset_samples,
                ),
            )
        self._project.pad_grid_offset_samples[sample_id] = grid_offset_samples
        self._project.pad_timing_intent[sample_id] = "legacy"
        self._transport._mark_project_changed()
        self._bpm.on_pad_bpm_changed(sample_id)

    def grid_anchor_sec(self, sample_id: int) -> float:
        """Return the current accepted signed origin or the durable legacy origin."""
        validate_sample_id(sample_id)
        return self._grid_anchor_sec(sample_id)

    def _grid_anchor_sec(self, sample_id: int) -> float:
        timing = self._bpm.current_timing(sample_id)
        return (
            timing.origin_seconds if timing is not None else self._legacy_grid_anchor_sec(sample_id)
        )

    def _legacy_grid_anchor_sec(
        self,
        sample_id: int,
        *,
        sample_rate_hz: int | None = None,
        grid_offset_samples: int | None = None,
    ) -> float:
        """Resolve saved legacy intent without consulting current accepted timing."""
        if sample_rate_hz is None:
            sample_rate_hz = self._transport._output_sample_rate_hz()
        if sample_rate_hz is None or sample_rate_hz <= 0:
            # Without a sample rate, we can't express a sample offset in seconds.
            return self._base_grid_anchor_sec(sample_id)

        base_sample = self._base_grid_anchor_sample(sample_id, sample_rate_hz=sample_rate_hz)
        offset = (
            self._grid_offset_samples(sample_id)
            if grid_offset_samples is None
            else grid_offset_samples
        )
        anchor_sample = base_sample + offset
        return anchor_sample / sample_rate_hz

    def _snap_to_nearest_64th_grid(
        self, target_s: float, *, timing: CurrentPadTiming | None
    ) -> float:
        if timing is None:
            return target_s

        grid = scalar_source_grid(
            origin_s=timing.origin_seconds, period_seconds=timing.period_seconds
        )
        if grid is None:
            return target_s
        return nearest_grid_source_s(target_s, grid=grid, step_beats=1.0 / 16.0)

    def _timing_sample_rate_hz(self, timing: CurrentPadTiming | None) -> int | None:
        if timing is not None and timing.sample_rate_hz is not None:
            return timing.sample_rate_hz
        return self._transport._output_sample_rate_hz()

    def _source_duration_s(self, sample_id: int, timing: CurrentPadTiming | None) -> float | None:
        if timing is not None and timing.source_duration_seconds is not None:
            return timing.source_duration_seconds
        return self._project.sample_durations[sample_id]

    @staticmethod
    def _duration_s_for_bars(*, bars: float, period_seconds: float) -> float:
        grid = scalar_source_grid(origin_s=0.0, period_seconds=period_seconds)
        duration_s = grid.source_at_beat(bars * 4.0) if grid is not None else None
        if duration_s is None:
            msg = "bar duration must have a finite scalar projection"
            raise ValueError(msg)
        return duration_s

    @staticmethod
    def _normalize_requested_bars(bars: float) -> float:
        ensure_finite(bars)
        bars = float(bars)
        if bars < PAD_LOOP_BARS_MIN:
            msg = f"bars must be >= {PAD_LOOP_BARS_MIN}, got {bars!r}"
            raise ValueError(msg)

        steps = bars / PAD_LOOP_BARS_GRANULARITY
        rounded_steps = round(steps)
        if not math.isclose(steps, rounded_steps, abs_tol=1e-9):
            msg = f"bars must use {PAD_LOOP_BARS_GRANULARITY}-bar granularity, got {bars!r}"
            raise ValueError(msg)
        return float(rounded_steps * PAD_LOOP_BARS_GRANULARITY)

    def _stored_bars(self, sample_id: int) -> float:
        bars = float(self._project.pad_loop_bars[sample_id])
        if not math.isfinite(bars) or bars < PAD_LOOP_BARS_MIN:
            return PAD_LOOP_BARS_DEFAULT
        return bars

    def max_auto_loop_bars(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> float | None:
        """Return the largest auto-loop bar count that fits, or None when unknown."""
        validate_sample_id(sample_id)

        timing = self._resolve_timing(sample_id, timing)
        if timing is None:
            return None

        duration_s = self._source_duration_s(sample_id, timing)
        if duration_s is None or not math.isfinite(duration_s) or duration_s <= 0.0:
            return None

        start_s = physical_source_marker_s(
            float(self._project.pad_loop_start_s[sample_id]),
            sample_rate_hz=self._timing_sample_rate_hz(timing),
        )
        remaining_s = float(duration_s) - start_s
        if remaining_s <= 0.0:
            return 0.0

        bar_s = self._duration_s_for_bars(bars=1.0, period_seconds=timing.period_seconds)
        if bar_s <= 0.0:
            return None
        return remaining_s / bar_s

    def _effective_pad_loop_region(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> tuple[float, float | None]:
        timing = self._resolve_timing(sample_id, timing)
        start_s = float(self._project.pad_loop_start_s[sample_id])
        end_s = self._project.pad_loop_end_s[sample_id]

        sample_rate_hz = self._timing_sample_rate_hz(timing)
        one_sample_s = (
            1.0 / sample_rate_hz if sample_rate_hz is not None and sample_rate_hz > 0 else 0.0001
        )

        if not self._project.pad_loop_auto[sample_id]:
            start_s = physical_source_marker_s(start_s, sample_rate_hz=sample_rate_hz)
            if end_s is not None:
                end_s = physical_source_marker_s(float(end_s), sample_rate_hz=sample_rate_hz)
                if end_s <= start_s:
                    end_s = start_s + one_sample_s
            return (start_s, end_s)

        if timing is None:
            return (physical_source_marker_s(start_s, sample_rate_hz=sample_rate_hz), None)

        bars = self._stored_bars(sample_id)
        grid = scalar_source_grid(
            origin_s=timing.origin_seconds, period_seconds=timing.period_seconds
        )
        end_s_effective = grid.source_after_beats(start_s, bars * 4.0) if grid is not None else None
        start_s = physical_source_marker_s(start_s, sample_rate_hz=sample_rate_hz)
        if end_s_effective is None:
            return (start_s, None)
        end_s_effective = physical_source_marker_s(end_s_effective, sample_rate_hz=sample_rate_hz)
        if end_s_effective <= start_s:
            end_s_effective = start_s + one_sample_s
        return (start_s, end_s_effective)

    def effective_region(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> tuple[float, float | None]:
        validate_sample_id(sample_id)
        return self._effective_pad_loop_region(sample_id, timing=timing)

    def set_auto(self, sample_id: int, *, enabled: bool) -> None:
        validate_sample_id(sample_id)
        if enabled == self._transport._project.pad_loop_auto[sample_id]:
            return

        timing = self._bpm.current_timing(sample_id)
        self._transport._project.pad_loop_auto[sample_id] = enabled
        if enabled:
            start_s = float(self._transport._project.pad_loop_start_s[sample_id])
            start_s = self._snap_to_nearest_64th_grid(start_s, timing=timing)
            start_s = physical_source_marker_s(
                start_s, sample_rate_hz=self._timing_sample_rate_hz(timing)
            )
            self._transport._project.pad_loop_start_s[sample_id] = start_s

        self._transport._mark_project_changed()
        self._apply_effective_pad_loop_region_to_audio(sample_id, timing=timing)

    def set_bars(self, sample_id: int, *, bars: float) -> None:
        validate_sample_id(sample_id)

        bars = self._normalize_requested_bars(bars)
        timing = self._bpm.current_timing(sample_id)
        max_bars = self.max_auto_loop_bars(sample_id, timing=timing)
        if max_bars is not None and bars > max_bars + 1e-9:
            return

        if bars == self._transport._project.pad_loop_bars[sample_id]:
            return

        self._transport._project.pad_loop_bars[sample_id] = bars
        self._transport._mark_project_changed()
        self._apply_effective_pad_loop_region_to_audio(sample_id, timing=timing)

    def set_start(self, sample_id: int, start_s: float) -> None:
        validate_sample_id(sample_id)
        ensure_finite(start_s)

        timing = self._bpm.current_timing(sample_id)
        sample_rate_hz = self._timing_sample_rate_hz(timing)
        start_s = max(0.0, start_s)
        if self._transport._project.pad_loop_auto[sample_id]:
            start_s = self._snap_to_nearest_64th_grid(start_s, timing=timing)

        start_s = physical_source_marker_s(start_s, sample_rate_hz=sample_rate_hz)
        self._transport._project.pad_loop_start_s[sample_id] = start_s

        end_s = self._transport._project.pad_loop_end_s[sample_id]
        one_sample_s = (
            1.0 / sample_rate_hz if sample_rate_hz is not None and sample_rate_hz > 0 else 0.0001
        )

        if end_s is not None and end_s <= start_s:
            self._transport._project.pad_loop_end_s[sample_id] = start_s + one_sample_s

        self._transport._mark_project_changed()
        self._apply_effective_pad_loop_region_to_audio(sample_id, timing=timing)

    def set_end(self, sample_id: int, end_s: float | None) -> None:
        validate_sample_id(sample_id)

        timing = self._bpm.current_timing(sample_id)
        if end_s is not None:
            ensure_finite(end_s)
            sample_rate_hz = self._timing_sample_rate_hz(timing)
            end_s = max(0.0, end_s)
            if self._transport._project.pad_loop_auto[sample_id]:
                end_s = self._snap_to_nearest_64th_grid(end_s, timing=timing)
            end_s = physical_source_marker_s(end_s, sample_rate_hz=sample_rate_hz)

            start_s = physical_source_marker_s(
                float(self._transport._project.pad_loop_start_s[sample_id]),
                sample_rate_hz=sample_rate_hz,
            )
            one_sample_s = (
                1.0 / sample_rate_hz
                if sample_rate_hz is not None and sample_rate_hz > 0
                else 0.0001
            )

            if end_s <= start_s:
                end_s = start_s + one_sample_s

        self._transport._project.pad_loop_end_s[sample_id] = end_s
        self._transport._mark_project_changed()
        self._apply_effective_pad_loop_region_to_audio(sample_id, timing=timing)
