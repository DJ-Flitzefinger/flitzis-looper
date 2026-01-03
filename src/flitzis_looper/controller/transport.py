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
    from flitzis_looper_audio import AudioEngine


class TransportController:
    _TAP_BPM_WINDOW_SIZE = 5

    def __init__(self, project: ProjectState, session: SessionState, audio: AudioEngine) -> None:
        self._project = project
        self._session = session
        self._audio = audio

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

    def set_speed(self, speed: float) -> None:
        """Set global playback speed multiplier."""
        ensure_finite(speed)
        clamped = min(max(speed, SPEED_MIN), SPEED_MAX)
        self._audio.set_speed(clamped)
        self._project.speed = clamped
        self._recompute_master_bpm()

    def reset_speed(self) -> None:
        """Reset global speed back to 1.0x."""
        self.set_speed(1.0)

    def set_pad_gain(self, sample_id: int, gain: float) -> None:
        validate_sample_id(sample_id)
        ensure_finite(gain)
        clamped = min(max(gain, PAD_GAIN_MIN), PAD_GAIN_MAX)
        self._audio.set_pad_gain(sample_id, clamped)
        self._project.pad_gain[sample_id] = clamped

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

    def set_multi_loop(self, *, enabled: bool) -> None:
        """Enable or disable Multi Loop mode."""
        self._project.multi_loop = enabled

    def set_key_lock(self, *, enabled: bool) -> None:
        """Enable or disable Key Lock mode."""
        if enabled == self._project.key_lock:
            return
        self._project.key_lock = enabled
        self._audio.set_key_lock(enabled=enabled)

    def set_bpm_lock(self, *, enabled: bool) -> None:
        """Enable or disable BPM Lock mode."""
        if enabled == self._project.bpm_lock:
            return

        self._project.bpm_lock = enabled

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

    def clear_manual_bpm(self, sample_id: int) -> None:
        """Clear a pad's manual BPM override."""
        validate_sample_id(sample_id)
        self._project.manual_bpm[sample_id] = None
        self._on_pad_bpm_changed(sample_id)

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

    def clear_manual_key(self, sample_id: int) -> None:
        """Clear a pad's manual key override."""
        validate_sample_id(sample_id)
        self._project.manual_key[sample_id] = None

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
