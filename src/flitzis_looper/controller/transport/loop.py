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
        analysis = self._project.sample_analysis[sample_id]
        start_s = 0.0
        beats: list[float] = []

        if analysis is not None:
            grid = analysis.beat_grid
            if grid.downbeats:
                start_s = float(grid.downbeats[0])
            elif grid.beats:
                start_s = float(grid.beats[0])
            beats = [float(t) for t in grid.beats]

        # Auto-loop is the default, even when BPM is unavailable.
        start_s = self._snap_to_nearest_beat(start_s, beats)
        start_s = self._quantize_time_to_output_samples(start_s)

        effective_bpm = self._transport.bpm.effective_bpm(sample_id)
        bpm = normalize_bpm(effective_bpm)
        if bpm is None:
            return (start_s, None, True)

        duration_s = (4 * 4) * 60.0 / bpm
        end_s = self._snap_to_nearest_beat(start_s + duration_s, beats)
        end_s = self._quantize_time_to_output_samples(end_s)
        return (start_s, end_s, True)

    def _apply_effective_pad_loop_region_to_audio(self, sample_id: int) -> None:
        if self._project.sample_paths[sample_id] is None:
            return
        start_s, end_s = self._effective_pad_loop_region(sample_id)
        self._audio.set_pad_loop_region(sample_id, start_s, end_s)

    def _quantize_time_to_output_samples(self, time_s: float) -> float:
        """Quantize a time to an integer output-sample boundary."""
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

        analysis = self._project.sample_analysis[sample_id]
        beats = [] if analysis is None else [float(t) for t in analysis.beat_grid.beats]

        if not self._project.pad_loop_auto[sample_id]:
            start_s = self._quantize_time_to_output_samples(start_s)
            if end_s is not None:
                end_s = self._quantize_time_to_output_samples(float(end_s))
                if end_s <= start_s:
                    end_s = None
            return (start_s, end_s)

        start_s = self._snap_to_nearest_beat(start_s, beats)
        start_s = self._quantize_time_to_output_samples(start_s)

        effective_bpm = self._bpm.effective_bpm(sample_id)
        bpm = normalize_bpm(effective_bpm)
        if bpm is None:
            if end_s is not None:
                end_s = self._quantize_time_to_output_samples(float(end_s))
                if end_s <= start_s:
                    end_s = None
            return (start_s, end_s)

        bars = max(1, int(self._project.pad_loop_bars[sample_id]))
        duration_s = (bars * 4) * 60.0 / bpm
        end_s_effective = self._snap_to_nearest_beat(start_s + duration_s, beats)
        end_s_effective = self._quantize_time_to_output_samples(end_s_effective)
        if end_s_effective <= start_s:
            return (start_s, None)
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
            analysis = self._transport._project.sample_analysis[sample_id]
            beats = [] if analysis is None else [float(t) for t in analysis.beat_grid.beats]
            start_s = float(self._transport._project.pad_loop_start_s[sample_id])
            start_s = self._snap_to_nearest_beat(start_s, beats)
            start_s = self._quantize_time_to_output_samples(start_s)
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

        start_s = max(0.0, float(start_s))
        analysis = self._transport._project.sample_analysis[sample_id]
        beats = [] if analysis is None else [float(t) for t in analysis.beat_grid.beats]
        if self._transport._project.pad_loop_auto[sample_id]:
            start_s = self._snap_to_nearest_beat(start_s, beats)

        start_s = self._quantize_time_to_output_samples(start_s)
        self._transport._project.pad_loop_start_s[sample_id] = start_s
        self._transport._mark_project_changed()
        self._apply_effective_pad_loop_region_to_audio(sample_id)

    def set_end(self, sample_id: int, end_s: float | None) -> None:
        validate_sample_id(sample_id)

        if end_s is not None:
            ensure_finite(end_s)
            end_s = max(0.0, float(end_s))
            end_s = self._quantize_time_to_output_samples(end_s)

            start_s = self._quantize_time_to_output_samples(
                float(self._transport._project.pad_loop_start_s[sample_id])
            )
            if end_s <= start_s:
                end_s = None

        self._transport._project.pad_loop_end_s[sample_id] = end_s
        self._transport._mark_project_changed()
        self._apply_effective_pad_loop_region_to_audio(sample_id)

    @staticmethod
    def _snap_to_nearest_beat(target_s: float, beat_times: list[float]) -> float:
        if not beat_times:
            return target_s
        return min(beat_times, key=lambda beat_s: abs(beat_s - target_s))
