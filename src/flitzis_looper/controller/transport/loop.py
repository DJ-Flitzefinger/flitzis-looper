from typing import TYPE_CHECKING

from flitzis_looper.controller.validation import ensure_finite, normalize_bpm
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
        """Reset a pad's loop region to a computed default."""
        validate_sample_id(sample_id)

        start_s, end_s, auto = self._default_pad_loop_region(sample_id)
        self._project.pad_loop_start_s[sample_id] = start_s
        self._project.pad_loop_end_s[sample_id] = end_s
        self._project.pad_loop_auto[sample_id] = auto
        self._project.pad_loop_bars[sample_id] = 4
        self._transport._mark_project_changed()

        self._apply_effective_pad_loop_region_to_audio(sample_id)

    def _default_pad_loop_region(self, sample_id: int) -> tuple[float, float | None, bool]:
        # Anchor the musical grid to the default onset.
        start_s = self._grid_anchor_sec(sample_id)

        # Auto-loop is the default, even when BPM is unavailable.
        start_s = self._snap_to_nearest_64th_grid(sample_id, start_s)
        start_s = self._quantize_time_to_cached_samples(start_s)

        bpm = normalize_bpm(self._transport.bpm.effective_bpm(sample_id))
        if bpm is None:
            return (start_s, None, True)

        duration_s = (4 * 4) * 60.0 / bpm
        end_s = self._quantize_time_to_cached_samples(start_s + duration_s)
        return (start_s, end_s, True)

    def _apply_effective_pad_loop_region_to_audio(self, sample_id: int) -> None:
        if self._project.sample_paths[sample_id] is None:
            return
        start_s, end_s = self._effective_pad_loop_region(sample_id)
        self._audio.set_pad_loop_region(sample_id, start_s, end_s)

    def _grid_offset_samples(self, sample_id: int) -> int:
        return int(self._project.pad_grid_offset_samples[sample_id])

    def _default_onset_sec(self, sample_id: int) -> float:
        analysis = self._project.sample_analysis[sample_id]
        if analysis is None:
            return 0.0

        grid = analysis.beat_grid
        if grid.downbeats:
            return float(grid.downbeats[0])
        if grid.beats:
            return float(grid.beats[0])
        return 0.0

    def _default_onset_sample(self, sample_id: int, *, sample_rate_hz: int) -> int:
        onset_sec = self._default_onset_sec(sample_id)
        frames = round(onset_sec * sample_rate_hz)
        if not isinstance(frames, int):
            return 0
        return max(frames, 0)

    def _bar_samples_for_grid_offset_clamp(self, sample_id: int) -> int | None:
        bpm = normalize_bpm(self._transport.bpm.effective_bpm(sample_id))
        if bpm is None:
            return None

        sample_rate_hz = self._transport._output_sample_rate_hz()
        if sample_rate_hz is None or sample_rate_hz <= 0:
            return None

        beat_sec = 60.0 / bpm
        bar_sec = beat_sec * 4.0
        return max(0, round(bar_sec * sample_rate_hz))

    def _clamp_grid_offset_samples(self, sample_id: int, value: int) -> int:
        bar_samples = self._bar_samples_for_grid_offset_clamp(sample_id)
        if bar_samples is None:
            return int(value)

        return max(-bar_samples, min(bar_samples, int(value)))

    def reclamp_grid_offset_samples(self, sample_id: int) -> bool:
        """Re-clamp the stored grid offset when effective BPM changes."""
        validate_sample_id(sample_id)

        current = int(self._project.pad_grid_offset_samples[sample_id])
        clamped = self._clamp_grid_offset_samples(sample_id, current)
        if clamped == current:
            return False

        self._project.pad_grid_offset_samples[sample_id] = clamped
        self._transport._mark_project_changed()
        return True

    def set_grid_offset_samples(self, sample_id: int, grid_offset_samples: int) -> None:
        validate_sample_id(sample_id)

        grid_offset_samples = int(grid_offset_samples)
        grid_offset_samples = self._clamp_grid_offset_samples(sample_id, grid_offset_samples)

        if grid_offset_samples == self._project.pad_grid_offset_samples[sample_id]:
            return

        self._project.pad_grid_offset_samples[sample_id] = grid_offset_samples
        self._transport._mark_project_changed()

    def grid_anchor_sec(self, sample_id: int) -> float:
        """Grid anchor time in seconds (default onset + per-pad sample offset)."""
        validate_sample_id(sample_id)
        return self._grid_anchor_sec(sample_id)

    def _grid_anchor_sec(self, sample_id: int) -> float:
        sample_rate_hz = self._transport._output_sample_rate_hz()
        if sample_rate_hz is None or sample_rate_hz <= 0:
            # Without a sample rate, we can't express a sample offset in seconds.
            return self._default_onset_sec(sample_id)

        onset_sample = self._default_onset_sample(sample_id, sample_rate_hz=sample_rate_hz)
        anchor_sample = onset_sample + self._grid_offset_samples(sample_id)
        return anchor_sample / sample_rate_hz

    @staticmethod
    def _grid_step_sec(bpm: float) -> float:
        beat_sec = 60.0 / bpm
        return beat_sec / 16.0

    @staticmethod
    def _snap_to_nearest_grid_point(target_s: float, *, anchor_s: float, step_s: float) -> float:
        if step_s <= 0.0:
            return target_s

        steps = round((target_s - anchor_s) / step_s)
        if not isinstance(steps, int):
            return target_s
        return anchor_s + steps * step_s

    def _snap_to_nearest_64th_grid(self, sample_id: int, target_s: float) -> float:
        bpm = normalize_bpm(self._bpm.effective_bpm(sample_id))
        if bpm is None:
            return target_s

        step_s = self._grid_step_sec(bpm)
        anchor_s = self._grid_anchor_sec(sample_id)
        return self._snap_to_nearest_grid_point(target_s, anchor_s=anchor_s, step_s=step_s)

    def _quantize_time_to_cached_samples(self, time_s: float) -> float:
        """Quantize a time to an integer sample index at the cached WAV sample rate."""
        sample_rate_hz = self._transport._output_sample_rate_hz()
        if sample_rate_hz is None or sample_rate_hz <= 0:
            return time_s

        frames = round(time_s * sample_rate_hz)
        if not isinstance(frames, int):
            return time_s
        frames = max(frames, 0)
        return frames / sample_rate_hz

    def _effective_pad_loop_region(self, sample_id: int) -> tuple[float, float | None]:
        start_s = float(self._project.pad_loop_start_s[sample_id])
        end_s = self._project.pad_loop_end_s[sample_id]

        sample_rate_hz = self._transport._output_sample_rate_hz()
        one_sample_s = (
            1.0 / sample_rate_hz if sample_rate_hz is not None and sample_rate_hz > 0 else 0.0001
        )

        if not self._project.pad_loop_auto[sample_id]:
            start_s = self._quantize_time_to_cached_samples(start_s)
            if end_s is not None:
                end_s = self._quantize_time_to_cached_samples(float(end_s))
                if end_s <= start_s:
                    end_s = start_s + one_sample_s
            return (start_s, end_s)
        start_s = self._snap_to_nearest_64th_grid(sample_id, start_s)

        start_s = self._quantize_time_to_cached_samples(start_s)

        effective_bpm = self._bpm.effective_bpm(sample_id)
        bpm = normalize_bpm(effective_bpm)
        if bpm is None:
            if end_s is not None:
                end_s = self._quantize_time_to_cached_samples(float(end_s))
                if end_s <= start_s:
                    end_s = start_s + one_sample_s
            return (start_s, end_s)

        bars = max(1, int(self._project.pad_loop_bars[sample_id]))
        duration_s = (bars * 4) * 60.0 / bpm
        end_s_effective = start_s + duration_s
        end_s_effective = self._quantize_time_to_cached_samples(end_s_effective)
        if end_s_effective <= start_s:
            end_s_effective = start_s + one_sample_s
        return (start_s, end_s_effective)

    def effective_region(self, sample_id: int) -> tuple[float, float | None]:
        validate_sample_id(sample_id)
        return self._effective_pad_loop_region(sample_id)

    def set_auto(self, sample_id: int, *, enabled: bool) -> None:
        validate_sample_id(sample_id)
        if enabled == self._transport._project.pad_loop_auto[sample_id]:
            return

        self._transport._project.pad_loop_auto[sample_id] = enabled
        if enabled:
            start_s = float(self._transport._project.pad_loop_start_s[sample_id])
            start_s = self._snap_to_nearest_64th_grid(sample_id, start_s)
            start_s = self._quantize_time_to_cached_samples(start_s)
            self._transport._project.pad_loop_start_s[sample_id] = start_s

        self._transport._mark_project_changed()
        self._apply_effective_pad_loop_region_to_audio(sample_id)

    def set_bars(self, sample_id: int, *, bars: int) -> None:
        validate_sample_id(sample_id)
        bars = max(1, int(bars))
        if bars == self._transport._project.pad_loop_bars[sample_id]:
            return

        self._transport._project.pad_loop_bars[sample_id] = bars
        self._transport._mark_project_changed()
        self._apply_effective_pad_loop_region_to_audio(sample_id)

    def set_start(self, sample_id: int, start_s: float) -> None:
        validate_sample_id(sample_id)
        ensure_finite(start_s)

        start_s = max(0.0, start_s)
        if self._transport._project.pad_loop_auto[sample_id]:
            start_s = self._snap_to_nearest_64th_grid(sample_id, start_s)

        start_s = self._quantize_time_to_cached_samples(start_s)
        self._transport._project.pad_loop_start_s[sample_id] = start_s

        end_s = self._transport._project.pad_loop_end_s[sample_id]
        sample_rate_hz = self._transport._output_sample_rate_hz()
        one_sample_s = (
            1.0 / sample_rate_hz if sample_rate_hz is not None and sample_rate_hz > 0 else 0.0001
        )

        if end_s is not None and end_s <= start_s:
            self._transport._project.pad_loop_end_s[sample_id] = start_s + one_sample_s

        self._transport._mark_project_changed()
        self._apply_effective_pad_loop_region_to_audio(sample_id)

    def set_end(self, sample_id: int, end_s: float | None) -> None:
        validate_sample_id(sample_id)

        if end_s is not None:
            ensure_finite(end_s)
            end_s = max(0.0, end_s)
            if self._transport._project.pad_loop_auto[sample_id]:
                end_s = self._snap_to_nearest_64th_grid(sample_id, end_s)
            end_s = self._quantize_time_to_cached_samples(end_s)

            start_s = self._quantize_time_to_cached_samples(
                float(self._transport._project.pad_loop_start_s[sample_id])
            )
            sample_rate_hz = self._transport._output_sample_rate_hz()
            one_sample_s = (
                1.0 / sample_rate_hz
                if sample_rate_hz is not None and sample_rate_hz > 0
                else 0.0001
            )

            if end_s <= start_s:
                end_s = start_s + one_sample_s

        self._transport._project.pad_loop_end_s[sample_id] = end_s
        self._transport._mark_project_changed()
        self._apply_effective_pad_loop_region_to_audio(sample_id)
