import math
from dataclasses import dataclass, replace
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
    from flitzis_looper.controller.source_beatmap_selection import SourceBeatmapSelection
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper.models import PadContentIdentity
    from flitzis_looper.source_beatmap_intent import PadSourceBeatmapIntent
    from flitzis_looper_audio import LoaderEventDelivery, SourceBeatmap, SourceBeatmapEvaluation


@dataclass(frozen=True, slots=True)
class LoopLaunchRegion:
    region: tuple[float, float | None]
    delivery: LoaderEventDelivery | None
    ordinary_pending: bool


@dataclass(frozen=True, slots=True)
class _SourceMapLoopEdit:
    geometry: SourceBeatmap
    rate: int
    duration: float
    generation: int
    timing: CurrentPadTiming | None
    content: PadContentIdentity | None
    intent: PadSourceBeatmapIntent | None


class PadLoopController:
    """Per-pad loop region manipulation."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._bpm = transport.bpm
        self._project = transport._project
        self._audio = transport._audio
        self._source_maps: SourceBeatmapSelection | None = None

    def bind_source_maps(self, source_maps: SourceBeatmapSelection) -> None:
        """Use the Loader's existing source-qualified editor selection owner."""
        self._source_maps = source_maps

    def _source_map_selected(self, sample_id: int) -> bool:
        return self._source_maps is not None and self._source_maps.is_selected(sample_id)

    def _variable_map_selected(self, sample_id: int) -> bool:
        return self._project.resolve_pad_realtime_bpm(sample_id) and self._source_map_selected(
            sample_id
        )

    def _requires_variable_map(self, sample_id: int) -> bool:
        return self._project.resolve_pad_realtime_bpm(sample_id)

    def _ensure_retained_map_edit_safe(self, sample_id: int) -> None:
        """An inactive request cannot give scalar edits custody of an old mapped voice."""
        if self._project.resolve_pad_realtime_bpm(sample_id):
            return
        if not callable(getattr(self._audio, "pad_source_sync_status", None)):
            return
        feedback = self._transport.source_sync._observation(sample_id)
        if feedback is not None and feedback.get("policy_mode") == "mapped" and feedback["active"]:
            message = "Loop editing is waiting for the actual mapped voice to convert or stop."
            raise ValueError(message)

    def _source_map_snapshot(self, sample_id: int) -> tuple[SourceBeatmap, int, float]:
        geometry = None if self._source_maps is None else self._source_maps.effective(sample_id)
        if geometry is None:
            message = "The selected source map is unavailable or still preparing"
            raise ValueError(message)
        info = geometry.editor_info()
        rate = info["sample_rate_hz"]
        duration = info["source_duration_seconds"]
        if (
            not isinstance(rate, int)
            or isinstance(rate, bool)
            or rate <= 0
            or not math.isfinite(duration)
            or duration <= 0.0
        ):
            message = "The selected source map has no valid physical source extent"
            raise ValueError(message)
        return geometry, rate, duration

    def _source_map_start(self, sample_id: int, sample_rate_hz: int) -> float:
        """Read the physical marker without resolving another map's automatic end."""
        return physical_source_marker_s(
            float(self._project.pad_loop_start_s[sample_id]), sample_rate_hz=sample_rate_hz
        )

    @staticmethod
    def _source_map_marker(
        geometry: SourceBeatmap, result: SourceBeatmapEvaluation, sample_rate_hz: int
    ) -> float:
        value = result["source_seconds"]
        if (
            result["map_revision"] != geometry.revision()
            or result["coordinate_status"] not in {"supported", "anchor"}
            or result["physical_domain"] not in {"within_source", "exclusive_source_end"}
            or value is None
            or not math.isfinite(value)
        ):
            message = "Source map operation is outside supported physical coverage"
            raise ValueError(message)
        return physical_source_marker_s(value, sample_rate_hz=sample_rate_hz)

    def _capture_source_map_edit(self, sample_id: int) -> _SourceMapLoopEdit:
        if not self._project.resolve_pad_realtime_bpm(sample_id):
            message = "Variable source timing is unavailable; source-map loop actions are disabled."
            raise ValueError(message)
        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        if generation is None:
            message = "Source map loop editing requires the current source generation"
            raise ValueError(message)
        content = self._project.pad_content[sample_id]
        intent = self._project.pad_source_beatmap_intent[sample_id]
        self._transport.residency.remember(sample_id)
        geometry, rate, duration = self._source_map_snapshot(sample_id)
        return _SourceMapLoopEdit(
            geometry,
            rate,
            duration,
            generation,
            self._bpm.current_timing(sample_id),
            content,
            intent,
        )

    def _publish_source_map_region(
        self,
        sample_id: int,
        edit: _SourceMapLoopEdit,
        start: float,
        end: float,
        *,
        bars: float | None = None,
        auto: bool = False,
    ) -> None:
        if (
            self._project.pad_content[sample_id] is not edit.content
            or self._project.pad_source_beatmap_intent[sample_id] is not edit.intent
            or not self._source_map_selected(sample_id)
        ):
            message = "Source map loop edit was superseded by newer content or map intent"
            raise ValueError(message)
        if not (
            math.isfinite(start) and math.isfinite(end) and 0.0 <= start < end <= edit.duration
        ):
            message = "Source map loop markers must remain inside the physical source"
            raise ValueError(message)
        previous = self._loop_intent(sample_id)
        self._project.pad_loop_start_s[sample_id] = start
        self._project.pad_loop_end_s[sample_id] = end
        self._project.pad_loop_auto[sample_id] = auto
        if bars is not None:
            self._project.pad_loop_bars[sample_id] = bars
        self._publish_loop_edit(
            sample_id,
            previous,
            timing=edit.timing,
            expected_source_generation=edit.generation,
            changed=self._loop_intent(sample_id) != previous,
            physical_region=(start, end),
        )

    def _source_map_bar_end(self, edit: _SourceMapLoopEdit, start: float, bars: float) -> float:
        result = edit.geometry.source_after_beats(start, bars * 4.0, require_four_four=True)
        return self._source_map_marker(edit.geometry, result, edit.rate)

    def _source_map_interval(
        self,
        edit: _SourceMapLoopEdit,
        start: SourceBeatmapEvaluation,
        end: SourceBeatmapEvaluation,
    ) -> tuple[float, float]:
        markers = (
            self._source_map_marker(edit.geometry, start, edit.rate),
            self._source_map_marker(edit.geometry, end, edit.rate),
        )
        start_s, start_beat, end_beat = start["source_seconds"], start["beat"], end["beat"]
        if start_s is None or start_beat is None or end_beat is None:
            message = "Source map loop interval has no supported musical coordinates"
            raise ValueError(message)
        interval = edit.geometry.source_after_beats(
            start_s, end_beat - start_beat, require_four_four=False
        )
        self._source_map_marker(edit.geometry, interval, edit.rate)
        return markers

    def apply_source_map_bars(self, sample_id: int, bars: float | None = None) -> None:
        """Materialize an explicit supported map interval without enabling mapped audio."""
        validate_sample_id(sample_id)
        bars = self._normalize_requested_bars(
            self._stored_bars(sample_id) if bars is None else bars
        )
        edit = self._capture_source_map_edit(sample_id)
        start = self._source_map_start(sample_id, edit.rate)
        end = self._source_map_bar_end(edit, start, bars)
        self._publish_source_map_region(
            sample_id,
            edit,
            start,
            end,
            bars=bars,
            auto=self._project.resolve_pad_source_sync(sample_id),
        )

    def snap_source_map_markers(self, sample_id: int) -> None:
        """Snap both requested physical markers through one current Native map snapshot."""
        validate_sample_id(sample_id)
        edit = self._capture_source_map_edit(sample_id)
        start = self._source_map_start(sample_id, edit.rate)
        end = (
            self._source_map_bar_end(edit, start, self._stored_bars(sample_id))
            if self._project.pad_loop_auto[sample_id]
            else self._project.pad_loop_end_s[sample_id]
        )
        start, end = self._source_map_interval(
            edit,
            edit.geometry.nearest_grid_source(start, 1.0 / 16.0),
            edit.geometry.nearest_grid_source(edit.duration if end is None else end, 1.0 / 16.0),
        )
        self._publish_source_map_region(sample_id, edit, start, end)

    def _set_source_map_start(self, sample_id: int, start: float) -> None:
        edit = self._capture_source_map_edit(sample_id)
        start = self._source_map_marker(
            edit.geometry, edit.geometry.nearest_grid_source(start, 1.0 / 16.0), edit.rate
        )
        end = self._source_map_bar_end(edit, start, self._stored_bars(sample_id))
        self._publish_source_map_region(
            sample_id,
            edit,
            start,
            end,
            auto=self._project.resolve_pad_source_sync(sample_id),
        )

    def _set_source_map_end(self, sample_id: int, end: float | None) -> None:
        edit = self._capture_source_map_edit(sample_id)
        start = self._source_map_start(sample_id, edit.rate)
        start, end = self._source_map_interval(
            edit,
            edit.geometry.evaluate_source(start),
            edit.geometry.evaluate_source(edit.duration)
            if end is None
            else edit.geometry.nearest_grid_source(end, 1.0 / 16.0),
        )
        self._publish_source_map_region(sample_id, edit, start, end)

    def _max_source_map_bars(self, sample_id: int) -> float | None:
        try:
            geometry, rate, duration = self._source_map_snapshot(sample_id)
            start = self._source_map_start(sample_id, rate)
            first, last = geometry.evaluate_source(start), geometry.evaluate_source(duration)
            self._source_map_marker(geometry, first, rate)
            self._source_map_marker(geometry, last, rate)
            if first["beat"] is None or last["beat"] is None:
                return None
            beats = last["beat"] - first["beat"]
            result = geometry.source_after_beats(start, beats, require_four_four=True)
            self._source_map_marker(geometry, result, rate)
        except RuntimeError, ValueError:
            return None
        return beats / 4.0 if math.isfinite(beats) and beats >= 0.0 else None

    def _loop_intent(self, sample_id: int) -> tuple[float, float | None, bool, float]:
        return (
            self._project.pad_loop_start_s[sample_id],
            self._project.pad_loop_end_s[sample_id],
            self._project.pad_loop_auto[sample_id],
            self._project.pad_loop_bars[sample_id],
        )

    def _publish_loop_edit(
        self,
        sample_id: int,
        previous: tuple[float, float | None, bool, float],
        *,
        timing: CurrentPadTiming | None,
        expected_source_generation: int | None,
        changed: bool = True,
        physical_region: tuple[float, float] | None = None,
    ) -> None:
        """Commit saved markers after admission; resident ACKs retain their own owner."""
        try:
            self._ensure_retained_map_edit_safe(sample_id)
            if physical_region is None:
                self._apply_effective_pad_loop_region_to_audio(
                    sample_id, timing=timing, expected_source_generation=expected_source_generation
                )
            else:
                self._transport.residency.publish_loop(
                    sample_id,
                    *physical_region,
                    expected_source_generation=expected_source_generation,
                )
        except RuntimeError, ValueError:
            (
                self._project.pad_loop_start_s[sample_id],
                self._project.pad_loop_end_s[sample_id],
                self._project.pad_loop_auto[sample_id],
                self._project.pad_loop_bars[sample_id],
            ) = previous
            raise
        if changed:
            self._transport._mark_project_changed()
        self._transport.record_current_pad_edit(sample_id, expected_source_generation, "loop")

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
        self._transport.residency.remember(sample_id)
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
        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        self._transport.residency.remember(sample_id)

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

        previous = self._loop_intent(sample_id)
        self._project.pad_loop_start_s[sample_id] = start_s
        self._project.pad_loop_end_s[sample_id] = end_s
        self._project.pad_loop_auto[sample_id] = False
        self._publish_loop_edit(
            sample_id,
            previous,
            timing=timing,
            expected_source_generation=generation,
            changed=changed,
        )

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
        expected_source_generation: int | None = None,
    ) -> None:
        if self._project.sample_paths[sample_id] is None:
            return
        start_s, end_s = self._effective_pad_loop_region(sample_id, timing=timing)
        self._transport.residency.publish_loop(
            sample_id, start_s, end_s, expected_source_generation=expected_source_generation
        )

    def apply_grid_anchor_to_audio(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
        expected_source_generation: int | None = None,
    ) -> None:
        """Publish legacy origins without revoking current accepted timing."""
        validate_sample_id(sample_id)
        if self._project.sample_paths[sample_id] is None:
            return

        if self._project.manual_bpm[sample_id] is None and self._bpm.awaiting_automatic_timing(
            sample_id
        ):
            return
        timing = self._resolve_timing(sample_id, timing)
        if timing is not None and timing.accepted_revision is not None:
            return
        self._audio.set_pad_timing_metadata(
            sample_id,
            self._legacy_grid_anchor_sec(sample_id),
            **self._transport.source_generation_options(expected_source_generation),
        )
        self._bpm.project_legacy_timing_admission(sample_id)

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
        expected_source_generation: int | None = None,
        publish: bool = True,
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
        if publish:
            self.apply_grid_anchor_to_audio(
                sample_id, timing=timing, expected_source_generation=expected_source_generation
            )
            self._apply_effective_pad_loop_region_to_audio(
                sample_id, timing=timing, expected_source_generation=expected_source_generation
            )
        return True

    def set_grid_offset_samples(self, sample_id: int, grid_offset_samples: int) -> None:
        validate_sample_id(sample_id)
        generation = self._transport.capture_current_pad_edit_generation(sample_id)

        timing = self._bpm.current_timing(sample_id)
        grid_offset_samples = self._clamp_grid_offset_samples(
            sample_id, int(grid_offset_samples), timing=timing
        )

        if grid_offset_samples == self._project.pad_grid_offset_samples[sample_id]:
            return

        self._ensure_retained_map_edit_safe(sample_id)

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
                **self._transport.source_generation_options(generation),
            )
        self._project.pad_grid_offset_samples[sample_id] = grid_offset_samples
        self._project.pad_timing_intent[sample_id] = "legacy"
        self._transport.record_current_pad_edit(sample_id, generation, "timing")
        self._transport._mark_project_changed()
        self._bpm.on_pad_bpm_changed(sample_id, expected_source_generation=generation)

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
        if self._variable_map_selected(sample_id) or self._requires_variable_map(sample_id):
            return self._max_source_map_bars(sample_id)
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
        return self.region_for_intent(
            sample_id,
            start=float(self._project.pad_loop_start_s[sample_id]),
            end=self._project.pad_loop_end_s[sample_id],
            auto=self._project.pad_loop_auto[sample_id],
            bars=self._stored_bars(sample_id),
            timing=timing,
        )

    def region_for_intent(
        self,
        sample_id: int,
        *,
        start: float,
        end: float | None,
        auto: bool,
        bars: float,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> tuple[float, float | None]:
        """Project a saved intent without substituting pending project fields."""
        timing = self._resolve_timing(sample_id, timing)
        mapped = (
            self._auto_policy_region(sample_id, start=start, end=end, bars=bars) if auto else None
        )
        if mapped is not None:
            return mapped

        return self._scalar_region_for_intent(
            sample_id, start=start, end=end, auto=auto, bars=bars, timing=timing
        )

    def _scalar_region_for_intent(
        self,
        sample_id: int,
        *,
        start: float,
        end: float | None,
        auto: bool,
        bars: float,
        timing: CurrentPadTiming | None,
    ) -> tuple[float, float | None]:
        start_s, end_s = start, end

        sample_rate_hz = self._timing_sample_rate_hz(timing)
        one_sample_s = (
            1.0 / sample_rate_hz if sample_rate_hz is not None and sample_rate_hz > 0 else 0.0001
        )

        if not auto:
            start_s = physical_source_marker_s(start_s, sample_rate_hz=sample_rate_hz)
            if end_s is not None:
                end_s = physical_source_marker_s(float(end_s), sample_rate_hz=sample_rate_hz)
                if end_s <= start_s:
                    end_s = start_s + one_sample_s
            return (start_s, end_s)

        if timing is None:
            return (physical_source_marker_s(start_s, sample_rate_hz=sample_rate_hz), None)

        grid = scalar_source_grid(
            origin_s=timing.origin_seconds, period_seconds=timing.period_seconds
        )
        end_s_effective = grid.source_after_beats(start_s, bars * 4.0) if grid is not None else None
        start_s = physical_source_marker_s(start_s, sample_rate_hz=sample_rate_hz)
        if end_s_effective is None:
            return (start_s, None)
        end_s_effective = physical_source_marker_s(end_s_effective, sample_rate_hz=sample_rate_hz)
        duration_s = self._source_duration_s(sample_id, timing)
        physical_end = (
            physical_source_marker_s(duration_s, sample_rate_hz=sample_rate_hz)
            if duration_s is not None and math.isfinite(duration_s) and duration_s > 0.0
            else None
        )
        if physical_end is not None:
            # Keep even an out-of-range generic marker within the last physical sample.
            start_s = min(start_s, max(0.0, physical_end - one_sample_s))
            end_s_effective = min(end_s_effective, physical_end)
        if end_s_effective <= start_s:
            end_s_effective = start_s + one_sample_s
        if physical_end is not None:
            end_s_effective = min(end_s_effective, physical_end)
        return (start_s, end_s_effective)

    def _auto_policy_region(
        self, sample_id: int, *, start: float, end: float | None, bars: float
    ) -> tuple[float, float | None] | None:
        # Requested inactive geometry is scalar. Existing mapped audio keeps its
        # Native owner; _ensure_retained_map_edit_safe fences scalar loop edits.
        return self._mapped_auto_region(sample_id, start=start, bars=bars)

    def _mapped_auto_region(
        self, sample_id: int, *, start: float, bars: float
    ) -> tuple[float, float] | None:
        if not self._project.resolve_pad_realtime_bpm(sample_id):
            return None
        geometry = None if self._source_maps is None else self._source_maps.effective(sample_id)
        if geometry is None:
            if self._project.resolve_pad_source_sync(sample_id):
                message = "Variable SYNC requires a qualified source map."
                raise ValueError(message)
            return None
        return self._mapped_region(geometry, start=start, bars=bars)

    def _mapped_region(
        self, geometry: SourceBeatmap, *, start: float, bars: float
    ) -> tuple[float, float]:
        rate = geometry.editor_info()["sample_rate_hz"]
        start = physical_source_marker_s(start, sample_rate_hz=rate)
        result = geometry.source_after_beats(start, bars * 4.0, require_four_four=True)
        end = self._source_map_marker(geometry, result, rate)
        if end <= start:
            message = "Mapped automatic loop has no physical source interval"
            raise ValueError(message)
        return start, end

    def _source_sync_requested_region(
        self, sample_id: int, geometry: SourceBeatmap
    ) -> tuple[float, float | None]:
        """Validate status against its captured map without publishing loop or start intent."""
        if self._project.pad_loop_auto[sample_id] and self._requires_variable_map(sample_id):
            return self._mapped_region(
                geometry,
                start=float(self._project.pad_loop_start_s[sample_id]),
                bars=self._stored_bars(sample_id),
            )
        return self.requested_region(sample_id)

    def effective_region(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> tuple[float, float | None]:
        validate_sample_id(sample_id)
        acknowledged = self._transport.residency.acknowledged_region(sample_id)
        if acknowledged is not None:
            return acknowledged
        return self._effective_pad_loop_region(sample_id, timing=timing)

    def requested_region(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> tuple[float, float | None]:
        """Return performer intent for preparation, independently of effective audio."""
        validate_sample_id(sample_id)
        return self._effective_pad_loop_region(sample_id, timing=timing)

    def _capture_launch_region(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> LoopLaunchRegion:
        """Allow ordinary birth only when this observation has no effective map."""
        validate_sample_id(sample_id)
        timing = self._resolve_timing(sample_id, timing)
        variable = self._project.resolve_pad_realtime_bpm(sample_id)
        requested = self._project.resolve_pad_source_sync(sample_id)
        geometry, delivery = (
            self._source_maps._playback_projection(sample_id)
            if variable and self._source_maps is not None
            else (None, None)
        )
        start = float(self._project.pad_loop_start_s[sample_id])
        bars = self._stored_bars(sample_id)
        auto = self._project.pad_loop_auto[sample_id]
        region = (
            self._mapped_region(geometry, start=start, bars=bars)
            if auto and geometry is not None
            else self._scalar_region_for_intent(
                sample_id,
                start=start,
                end=self._project.pad_loop_end_s[sample_id],
                auto=auto,
                bars=bars,
                timing=timing,
            )
        )
        return LoopLaunchRegion(
            region, delivery if requested else None, requested and variable and geometry is None
        )

    def launch_region(
        self,
        sample_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> tuple[float, float | None]:
        """Resolve a new Play region without changing strict edit/seek geometry."""
        return self._capture_launch_region(sample_id, timing=timing).region

    def set_auto(self, sample_id: int, *, enabled: bool) -> None:
        validate_sample_id(sample_id)
        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        self._transport.residency.remember(sample_id)
        if enabled == self._transport._project.pad_loop_auto[sample_id]:
            return

        if enabled and self._requires_variable_map(sample_id):
            # An explicit mapped bar cycle preserves the physical chosen start.
            # Scalar grid snapping would move it under a different timing owner.
            self.apply_source_map_bars(sample_id)
            return

        timing = self._bpm.current_timing(sample_id)
        previous = self._loop_intent(sample_id)
        self._transport._project.pad_loop_auto[sample_id] = enabled
        if enabled:
            start_s = float(self._transport._project.pad_loop_start_s[sample_id])
            start_s = self._snap_to_nearest_64th_grid(start_s, timing=timing)
            start_s = physical_source_marker_s(
                start_s, sample_rate_hz=self._timing_sample_rate_hz(timing)
            )
            self._transport._project.pad_loop_start_s[sample_id] = start_s

        self._publish_loop_edit(
            sample_id, previous, timing=timing, expected_source_generation=generation
        )

    def set_bars(self, sample_id: int, *, bars: float) -> None:
        validate_sample_id(sample_id)
        if self._variable_map_selected(sample_id):
            self.apply_source_map_bars(sample_id, bars)
            return
        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        self._transport.residency.remember(sample_id)

        bars = self._normalize_requested_bars(bars)
        timing = self._bpm.current_timing(sample_id)
        max_bars = self.max_auto_loop_bars(sample_id, timing=timing)
        if max_bars is not None and bars > max_bars + 1e-9:
            return

        if bars == self._transport._project.pad_loop_bars[sample_id]:
            return

        previous = self._loop_intent(sample_id)
        self._transport._project.pad_loop_bars[sample_id] = bars
        self._publish_loop_edit(
            sample_id, previous, timing=timing, expected_source_generation=generation
        )

    def set_start(self, sample_id: int, start_s: float) -> None:
        validate_sample_id(sample_id)
        ensure_finite(start_s)
        if self._variable_map_selected(sample_id) and self._project.pad_loop_auto[sample_id]:
            self._set_source_map_start(sample_id, start_s)
            return
        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        self._transport.residency.remember(sample_id)

        timing = self._bpm.current_timing(sample_id)
        sample_rate_hz = self._timing_sample_rate_hz(timing)
        start_s = max(0.0, start_s)
        if self._transport._project.pad_loop_auto[sample_id]:
            start_s = self._snap_to_nearest_64th_grid(start_s, timing=timing)

        previous = self._loop_intent(sample_id)
        start_s = physical_source_marker_s(start_s, sample_rate_hz=sample_rate_hz)
        self._transport._project.pad_loop_start_s[sample_id] = start_s

        end_s = self._transport._project.pad_loop_end_s[sample_id]
        one_sample_s = (
            1.0 / sample_rate_hz if sample_rate_hz is not None and sample_rate_hz > 0 else 0.0001
        )

        if end_s is not None and end_s <= start_s:
            self._transport._project.pad_loop_end_s[sample_id] = start_s + one_sample_s

        self._publish_loop_edit(
            sample_id, previous, timing=timing, expected_source_generation=generation
        )

    def set_end(self, sample_id: int, end_s: float | None) -> None:
        validate_sample_id(sample_id)
        if end_s is not None:
            ensure_finite(end_s)
        if self._variable_map_selected(sample_id) and self._project.pad_loop_auto[sample_id]:
            self._set_source_map_end(sample_id, end_s)
            return
        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        self._transport.residency.remember(sample_id)

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

        previous = self._loop_intent(sample_id)
        self._transport._project.pad_loop_end_s[sample_id] = end_s
        self._publish_loop_edit(
            sample_id, previous, timing=timing, expected_source_generation=generation
        )
