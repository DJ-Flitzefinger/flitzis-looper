import math
from itertools import pairwise
from time import monotonic
from typing import TYPE_CHECKING

from flitzis_looper.constants import (
    PAD_EQ_DB_MAX,
    PAD_EQ_DB_MIN,
    PAD_GAIN_MAX,
    PAD_GAIN_MIN,
    SPEED_MAX,
    SPEED_MIN,
    VOLUME_MAX,
    VOLUME_MIN,
)
from flitzis_looper.controller.validation import ensure_finite, normalize_bpm
from flitzis_looper.models import ProjectState, SessionState, validate_sample_id

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper_audio import AudioEngine


class TransportController:
    _TAP_BPM_WINDOW_SIZE = 5

    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        *,
        on_project_changed: Callable[[], None] | None = None,
    ) -> None:
        self._project = project
        self._session = session
        self._audio = audio
        self._on_project_changed = on_project_changed

        self.loop = PadLoopController(self)
        self.playback = PadPlaybackController(self)

    def _mark_project_changed(self) -> None:
        if self._on_project_changed is not None:
            self._on_project_changed()

    def _output_sample_rate_hz(self) -> int | None:
        fn = getattr(self._audio, "output_sample_rate", None)
        if fn is None:
            return None
        try:
            return int(fn())
        except RuntimeError:
            return None
        except TypeError:
            return None
        except ValueError:
            return None

    def _quantize_time_to_output_samples(self, time_s: float) -> float:
        """Quantize a time to an integer output-sample boundary."""
        sample_rate_hz = self._output_sample_rate_hz()
        if sample_rate_hz is None or sample_rate_hz <= 0:
            return time_s

        frames = round(time_s * sample_rate_hz)
        if not isinstance(frames, int):
            return time_s
        frames = max(frames, 0)
        return frames / sample_rate_hz

    def _apply_effective_pad_loop_region_to_audio(self, sample_id: int) -> None:
        if self._project.sample_paths[sample_id] is None:
            return
        start_s, end_s = self._effective_pad_loop_region(sample_id)
        self._audio.set_pad_loop_region(sample_id, start_s, end_s)

    def _apply_project_state_to_audio(self) -> None:
        defaults = ProjectState()

        self._apply_global_audio_settings(defaults)
        self._apply_per_pad_mixing(defaults)
        self._apply_pad_loop_regions(defaults)
        self._apply_pad_bpm_settings()
        self._apply_bpm_lock_settings()

    def _apply_global_audio_settings(self, defaults: ProjectState) -> None:
        if self._project.volume != defaults.volume:
            self._audio.set_volume(self._project.volume)

        if self._project.speed != defaults.speed:
            self._audio.set_speed(self._project.speed)

        if self._project.key_lock != defaults.key_lock:
            self._audio.set_key_lock(enabled=self._project.key_lock)

        if self._project.bpm_lock != defaults.bpm_lock:
            self._audio.set_bpm_lock(enabled=self._project.bpm_lock)

    def _apply_per_pad_mixing(self, defaults: ProjectState) -> None:
        for sample_id, gain in enumerate(self._project.pad_gain):
            if gain != defaults.pad_gain[sample_id]:
                self._audio.set_pad_gain(sample_id, gain)

        for sample_id, low_db in enumerate(self._project.pad_eq_low_db):
            mid_db = self._project.pad_eq_mid_db[sample_id]
            high_db = self._project.pad_eq_high_db[sample_id]

            if (
                low_db == defaults.pad_eq_low_db[sample_id]
                and mid_db == defaults.pad_eq_mid_db[sample_id]
                and high_db == defaults.pad_eq_high_db[sample_id]
            ):
                continue

            self._audio.set_pad_eq(sample_id, low_db, mid_db, high_db)

    def _apply_pad_loop_regions(self, defaults: ProjectState) -> None:
        for sample_id in range(len(self._project.sample_paths)):
            if self._project.sample_paths[sample_id] is None:
                continue

            start_s = self._project.pad_loop_start_s[sample_id]
            end_s = self._project.pad_loop_end_s[sample_id]
            if (
                start_s == defaults.pad_loop_start_s[sample_id]
                and end_s == defaults.pad_loop_end_s[sample_id]
                and not self._project.pad_loop_auto[sample_id]
            ):
                continue

            self._apply_effective_pad_loop_region_to_audio(sample_id)

    def _apply_pad_bpm_settings(self) -> None:
        for sample_id in range(len(self._project.sample_paths)):
            if (
                self._project.manual_bpm[sample_id] is None
                and self._project.sample_analysis[sample_id] is None
            ):
                continue
            self._on_pad_bpm_changed(sample_id)

    def _apply_bpm_lock_settings(self) -> None:
        if self._project.bpm_lock:
            anchor_pad_id = self._project.selected_pad
            anchor_bpm = normalize_bpm(self.effective_bpm(anchor_pad_id))
            self._session.bpm_lock_anchor_pad_id = anchor_pad_id
            self._session.bpm_lock_anchor_bpm = anchor_bpm
        else:
            self._session.bpm_lock_anchor_pad_id = None
            self._session.bpm_lock_anchor_bpm = None

        self._recompute_master_bpm()

    def trigger_pad(self, sample_id: int) -> None:
        """Trigger or retrigger a pad's loop.

        When Multi Loop is disabled, all other active pads are stopped first.

        Args:
            sample_id: Sample slot identifier.
        """
        validate_sample_id(sample_id)

        if self._project.sample_paths[sample_id] is None:
            return

        if self._project.multi_loop:
            self.stop_pad(sample_id)
        else:
            self.stop_all_pads()

        start_s, end_s = self._effective_pad_loop_region(sample_id)
        self._audio.set_pad_loop_region(sample_id, start_s, end_s)
        self._audio.play_sample(sample_id, 1.0)
        self._session.active_sample_ids.add(sample_id)

    def stop_pad(self, sample_id: int) -> None:
        """Stop a pad if it is currently active."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.active_sample_ids:
            return

        self._audio.stop_sample(sample_id)
        self._session.active_sample_ids.discard(sample_id)

    def stop_all_pads(self) -> None:
        """Stop all currently active pads."""
        self._audio.stop_all()
        self._session.active_sample_ids.clear()

    def _reset_pad_loop_region(self, sample_id: int) -> None:
        """Reset a pad's loop region to a computed default."""
        validate_sample_id(sample_id)

        start_s, end_s, auto = self._default_pad_loop_region(sample_id)
        self._project.pad_loop_start_s[sample_id] = start_s
        self._project.pad_loop_end_s[sample_id] = end_s
        self._project.pad_loop_auto[sample_id] = auto
        self._project.pad_loop_bars[sample_id] = 4
        self._mark_project_changed()

        self._apply_effective_pad_loop_region_to_audio(sample_id)

    def _set_pad_loop_region(
        self,
        sample_id: int,
        *,
        start_s: float,
        end_s: float | None,
    ) -> None:
        """Set the loop region for a pad.

        This updates persisted project state and applies the new region to the audio engine.
        """
        validate_sample_id(sample_id)
        ensure_finite(start_s)
        if end_s is not None:
            ensure_finite(end_s)

        start_s = max(0.0, float(start_s))
        start_s = self._quantize_time_to_output_samples(start_s)

        if end_s is not None:
            end_s = max(0.0, float(end_s))
            end_s = self._quantize_time_to_output_samples(end_s)
            if end_s <= start_s:
                end_s = None

        self._project.pad_loop_start_s[sample_id] = start_s
        self._project.pad_loop_end_s[sample_id] = end_s
        self._mark_project_changed()

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

        bpm = normalize_bpm(self.effective_bpm(sample_id))
        if bpm is None:
            return (start_s, None, True)

        duration_s = (4 * 4) * 60.0 / bpm
        end_s = self._snap_to_nearest_beat(start_s + duration_s, beats)
        end_s = self._quantize_time_to_output_samples(end_s)
        return (start_s, end_s, True)

    @staticmethod
    def _snap_to_nearest_beat(target_s: float, beat_times: list[float]) -> float:
        if not beat_times:
            return target_s
        return min(beat_times, key=lambda beat_s: abs(beat_s - target_s))

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

        bpm = normalize_bpm(self.effective_bpm(sample_id))
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

    def is_sample_loaded(self, sample_id: int) -> bool:
        """Return whether a sample slot has audio loaded."""
        validate_sample_id(sample_id)
        return self._project.sample_paths[sample_id] is not None

    def set_volume(self, volume: float) -> None:
        """Set global volume."""
        ensure_finite(volume)
        clamped = min(max(volume, VOLUME_MIN), VOLUME_MAX)
        self._audio.set_volume(clamped)
        self._project.volume = clamped
        self._mark_project_changed()

    def set_speed(self, speed: float) -> None:
        """Set global playback speed multiplier."""
        ensure_finite(speed)
        clamped = min(max(speed, SPEED_MIN), SPEED_MAX)
        self._audio.set_speed(clamped)
        self._project.speed = clamped
        self._recompute_master_bpm()
        self._mark_project_changed()

    def reset_speed(self) -> None:
        """Reset global speed back to 1.0x."""
        self.set_speed(1.0)

    def set_pad_gain(self, sample_id: int, gain: float) -> None:
        validate_sample_id(sample_id)
        ensure_finite(gain)
        clamped = min(max(gain, PAD_GAIN_MIN), PAD_GAIN_MAX)
        self._audio.set_pad_gain(sample_id, clamped)
        self._project.pad_gain[sample_id] = clamped
        self._mark_project_changed()

    def set_pad_eq(self, sample_id: int, low_db: float, mid_db: float, high_db: float) -> None:
        validate_sample_id(sample_id)
        for value in (low_db, mid_db, high_db):
            ensure_finite(value)

        low_db = min(max(low_db, PAD_EQ_DB_MIN), PAD_EQ_DB_MAX)
        mid_db = min(max(mid_db, PAD_EQ_DB_MIN), PAD_EQ_DB_MAX)
        high_db = min(max(high_db, PAD_EQ_DB_MIN), PAD_EQ_DB_MAX)

        self._audio.set_pad_eq(sample_id, low_db, mid_db, high_db)
        self._project.pad_eq_low_db[sample_id] = low_db
        self._project.pad_eq_mid_db[sample_id] = mid_db
        self._project.pad_eq_high_db[sample_id] = high_db
        self._mark_project_changed()

    def set_multi_loop(self, *, enabled: bool) -> None:
        """Enable or disable Multi Loop mode."""
        self._project.multi_loop = enabled
        self._mark_project_changed()

    def set_key_lock(self, *, enabled: bool) -> None:
        """Enable or disable Key Lock mode."""
        if enabled == self._project.key_lock:
            return
        self._project.key_lock = enabled
        self._audio.set_key_lock(enabled=enabled)
        self._mark_project_changed()

    def set_bpm_lock(self, *, enabled: bool) -> None:
        """Enable or disable BPM Lock mode."""
        if enabled == self._project.bpm_lock:
            return

        self._project.bpm_lock = enabled
        self._mark_project_changed()

        if enabled:
            anchor_pad_id = self._project.selected_pad
            anchor_bpm = normalize_bpm(self.effective_bpm(anchor_pad_id))
            self._session.bpm_lock_anchor_pad_id = anchor_pad_id
            self._session.bpm_lock_anchor_bpm = anchor_bpm
        else:
            self._session.bpm_lock_anchor_pad_id = None
            self._session.bpm_lock_anchor_bpm = None

        self._audio.set_bpm_lock(enabled=enabled)
        self._recompute_master_bpm()

    def set_manual_bpm(self, sample_id: int, bpm: float) -> None:
        """Set a pad's manual BPM override."""
        validate_sample_id(sample_id)
        ensure_finite(bpm)
        if bpm <= 0:
            msg = f"bpm must be > 0, got {bpm!r}"
            raise ValueError(msg)
        self._project.manual_bpm[sample_id] = float(bpm)
        self._on_pad_bpm_changed(sample_id)
        self._mark_project_changed()

    def clear_manual_bpm(self, sample_id: int) -> None:
        """Clear a pad's manual BPM override."""
        validate_sample_id(sample_id)
        self._project.manual_bpm[sample_id] = None
        self._on_pad_bpm_changed(sample_id)
        self._mark_project_changed()

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
        self._mark_project_changed()
        return bpm

    def effective_bpm(self, sample_id: int) -> float | None:
        """Return the effective BPM for a pad (manual overrides detected)."""
        validate_sample_id(sample_id)

        manual = self._project.manual_bpm[sample_id]
        if manual is not None:
            return float(manual)

        analysis = self._project.sample_analysis[sample_id]
        return analysis.bpm if analysis is not None else None

    def set_manual_key(self, sample_id: int, key: str) -> None:
        """Set a pad's manual key override."""
        validate_sample_id(sample_id)
        if not key:
            msg = "key must be a non-empty string"
            raise ValueError(msg)
        self._project.manual_key[sample_id] = key
        self._mark_project_changed()

    def clear_manual_key(self, sample_id: int) -> None:
        """Clear a pad's manual key override."""
        validate_sample_id(sample_id)
        self._project.manual_key[sample_id] = None
        self._mark_project_changed()

    def effective_key(self, sample_id: int) -> str | None:
        """Return the effective key for a pad (manual overrides detected)."""
        validate_sample_id(sample_id)

        manual = self._project.manual_key[sample_id]
        if manual is not None:
            return manual

        analysis = self._project.sample_analysis[sample_id]
        return analysis.key if analysis is not None else None

    def _recompute_master_bpm(self) -> None:
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

    def _on_pad_bpm_changed(self, sample_id: int) -> None:
        bpm = normalize_bpm(self.effective_bpm(sample_id))
        self._audio.set_pad_bpm(sample_id, bpm)

        if self._session.bpm_lock_anchor_pad_id != sample_id:
            return

        self._session.bpm_lock_anchor_bpm = bpm
        self._recompute_master_bpm()


class PadPlaybackController:
    """Pad playback helpers that bypass multi-loop trigger semantics."""

    def __init__(self, transport: TransportController) -> None:
        self._t = transport

    def play(self, sample_id: int) -> None:
        validate_sample_id(sample_id)
        if self._t._project.sample_paths[sample_id] is None:
            return

        self._t._apply_effective_pad_loop_region_to_audio(sample_id)
        self._t._audio.play_sample(sample_id, 1.0)
        self._t._session.active_sample_ids.add(sample_id)

    def toggle(self, sample_id: int) -> None:
        validate_sample_id(sample_id)
        if sample_id in self._t._session.active_sample_ids:
            self._t.stop_pad(sample_id)
        else:
            self.play(sample_id)


class PadLoopController:
    """Per-pad loop region manipulation."""

    def __init__(self, transport: TransportController) -> None:
        self._t = transport

    def reset(self, sample_id: int) -> None:
        self._t._reset_pad_loop_region(sample_id)

    def effective_region(self, sample_id: int) -> tuple[float, float | None]:
        validate_sample_id(sample_id)
        return self._t._effective_pad_loop_region(sample_id)

    def set_auto(self, sample_id: int, *, enabled: bool) -> None:
        validate_sample_id(sample_id)
        if enabled == self._t._project.pad_loop_auto[sample_id]:
            return

        self._t._project.pad_loop_auto[sample_id] = enabled
        if enabled:
            analysis = self._t._project.sample_analysis[sample_id]
            beats = [] if analysis is None else [float(t) for t in analysis.beat_grid.beats]
            start_s = float(self._t._project.pad_loop_start_s[sample_id])
            start_s = self._t._snap_to_nearest_beat(start_s, beats)
            start_s = self._t._quantize_time_to_output_samples(start_s)
            self._t._project.pad_loop_start_s[sample_id] = start_s

        self._t._mark_project_changed()
        self._t._apply_effective_pad_loop_region_to_audio(sample_id)

    def set_bars(self, sample_id: int, *, bars: int) -> None:
        validate_sample_id(sample_id)
        bars = max(1, int(bars))
        if bars == self._t._project.pad_loop_bars[sample_id]:
            return

        self._t._project.pad_loop_bars[sample_id] = bars
        self._t._mark_project_changed()
        self._t._apply_effective_pad_loop_region_to_audio(sample_id)

    def set_start(self, sample_id: int, start_s: float) -> None:
        validate_sample_id(sample_id)
        ensure_finite(start_s)

        start_s = max(0.0, float(start_s))
        analysis = self._t._project.sample_analysis[sample_id]
        beats = [] if analysis is None else [float(t) for t in analysis.beat_grid.beats]
        if self._t._project.pad_loop_auto[sample_id]:
            start_s = self._t._snap_to_nearest_beat(start_s, beats)

        start_s = self._t._quantize_time_to_output_samples(start_s)
        self._t._project.pad_loop_start_s[sample_id] = start_s
        self._t._mark_project_changed()
        self._t._apply_effective_pad_loop_region_to_audio(sample_id)

    def set_end(self, sample_id: int, end_s: float | None) -> None:
        validate_sample_id(sample_id)

        if end_s is not None:
            ensure_finite(end_s)
            end_s = max(0.0, float(end_s))
            end_s = self._t._quantize_time_to_output_samples(end_s)

            start_s = self._t._quantize_time_to_output_samples(
                float(self._t._project.pad_loop_start_s[sample_id])
            )
            if end_s <= start_s:
                end_s = None

        self._t._project.pad_loop_end_s[sample_id] = end_s
        self._t._mark_project_changed()
        self._t._apply_effective_pad_loop_region_to_audio(sample_id)
