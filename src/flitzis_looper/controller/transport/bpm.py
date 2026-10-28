import math
from itertools import pairwise
from statistics import median
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
from flitzis_looper.models import TimingIntent, validate_sample_id

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.transport import TransportController


class BpmController:
    """Manage BPM overrides, tap detection, and master BPM computation."""

    _TAP_BPM_RESET_AFTER_S = 3.0

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._audio = transport._audio
        self._on_accepted_refresh: Callable[[int], None] | None = None

    def set_accepted_refresh_callback(self, callback: Callable[[int], None]) -> None:
        """Connect productive accepted updates to acknowledged atomic derived refresh."""
        self._on_accepted_refresh = callback

    def project_legacy_timing_admission(self, sample_id: int) -> None:
        """Mirror an actually admitted Native scalar BPM or origin publication."""
        if self._project.pad_timing_intent[sample_id] != "legacy":
            self._project.pad_timing_intent[sample_id] = "legacy"
            self._transport._mark_project_changed()

    def set_manual_bpm(
        self,
        sample_id: int,
        bpm: float,
        *,
        expected_source_generation: int | None = None,
    ) -> None:
        """Set a pad's manual BPM override."""
        validate_sample_id(sample_id)
        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        if expected_source_generation is not None and generation != expected_source_generation:
            message = "BPM correction source changed before admission"
            raise ValueError(message)
        ensure_finite(bpm)
        if bpm <= 0:
            msg = f"bpm must be > 0, got {bpm!r}"
            raise ValueError(msg)
        # Failed admission must leave both the current accepted record and intent intact.
        self._audio.set_pad_bpm(
            sample_id, float(bpm), **self._transport.source_generation_options(generation)
        )
        self._project.manual_bpm[sample_id] = float(bpm)
        self._project.pad_timing_intent[sample_id] = "legacy"
        self._transport._mark_project_changed()
        self._transport.record_current_pad_edit(sample_id, generation, "timing")
        self.on_pad_bpm_changed(
            sample_id,
            publish_bpm=False,
            timing_intent="manual",
            expected_source_generation=generation,
        )

    def scale_bpm(self, sample_id: int, factor: float) -> None:
        """Correct half/double detection through the current-source manual path."""
        validate_sample_id(sample_id)
        if factor not in {0.5, 2.0}:
            message = "BPM correction factor must be 0.5 or 2"
            raise ValueError(message)
        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        if generation is None:
            message = "BPM correction source confirmation unavailable"
            raise ValueError(message)
        current = self.effective_bpm(sample_id)
        target = normalize_bpm(current * factor if current is not None else None)
        if target is None:
            message = "BPM is unavailable for half/double correction"
            raise ValueError(message)
        self.set_manual_bpm(sample_id, target, expected_source_generation=generation)

    def clear_manual_bpm(self, sample_id: int) -> None:
        """Clear a pad's manual BPM override."""
        validate_sample_id(sample_id)
        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        analysis = self._project.sample_analysis[sample_id]
        self._audio.set_pad_bpm(
            sample_id,
            normalize_bpm(analysis.bpm if analysis else None),
            **self._transport.source_generation_options(generation),
        )
        self._project.manual_bpm[sample_id] = None
        self._project.pad_timing_intent[sample_id] = "legacy"
        self._transport._mark_project_changed()
        self._transport.record_current_pad_edit(sample_id, generation, "timing")
        self.on_pad_bpm_changed(sample_id, publish_bpm=False, expected_source_generation=generation)

    def tap_bpm(self, sample_id: int) -> float | None:
        """Register a Tap BPM event and update manual BPM."""
        validate_sample_id(sample_id)
        generation = self._transport.capture_current_pad_edit_generation(sample_id)

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

        self._audio.set_pad_bpm(
            sample_id, bpm, **self._transport.source_generation_options(generation)
        )
        self._project.manual_bpm[sample_id] = bpm
        self._project.pad_timing_intent[sample_id] = "legacy"
        self._transport._mark_project_changed()
        self._transport.record_current_pad_edit(sample_id, generation, "timing")
        self.on_pad_bpm_changed(
            sample_id,
            publish_bpm=False,
            timing_intent="tap",
            expected_source_generation=generation,
        )
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
        if timing is None and self.awaiting_automatic_timing(sample_id):
            return None
        return self._legacy_bpm(sample_id)

    def _legacy_bpm(self, sample_id: int) -> float | None:
        manual = self._project.manual_bpm[sample_id]
        if manual is not None:
            return float(manual)
        analysis = self._project.sample_analysis[sample_id]
        return normalize_bpm(analysis.bpm) if analysis is not None else None

    def current_timing(self, sample_id: int) -> CurrentPadTiming | None:
        """Resolve one current period/origin/identity snapshot without caching history."""
        validate_sample_id(sample_id)
        manual = self._project.manual_bpm[sample_id]
        if manual is None:
            metadata = self._audio.current_constant_timing(sample_id)
            if metadata is not None:
                return current_accepted_timing(metadata, sample_id=sample_id)
            if self.awaiting_automatic_timing(sample_id):
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

    def awaiting_automatic_timing(self, sample_id: int) -> bool:
        """Keep Automatic intent unresolved until native timing is acknowledged."""
        validate_sample_id(sample_id)
        if self._project.manual_bpm[sample_id] is not None:
            return False
        return self._audio.pad_timing_intent(sample_id) == "automatic" or (
            self._project.pad_timing_intent[sample_id] == "automatic"
        )

    def recompute_master_bpm(
        self,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
        publish_master: bool = True,
    ) -> None:
        """Publish master timing using one resolved anchor snapshot."""
        if not publish_master or not self._project.resolve_bpm_lock():
            return
        anchor_pad_id = self._session.bpm_lock_anchor_pad_id
        if isinstance(timing, UnresolvedTiming):
            timing = self.current_timing(anchor_pad_id) if anchor_pad_id is not None else None
        if timing is None:
            return
        # Accepted automatic timing retains its existing qualified refresh owner.
        # Manual/Legacy reconciliation must never retry a refused speed request.
        self._transport.global_params._publish_manual_bpm_master(timing=timing)

    def on_pad_bpm_changed(
        self,
        sample_id: int,
        *,
        publish_bpm: bool = True,
        timing_intent: TimingIntent | None = None,
        expected_source_generation: int | None = None,
    ) -> None:
        """Refresh derived controls without replaying legacy values over accepted timing."""
        if (
            timing_intent is None
            and publish_bpm
            and (
                self._project.manual_bpm[sample_id] is not None
                or self._project.pad_timing_intent[sample_id] in {"manual", "tap"}
            )
        ):
            timing_intent = (
                "tap" if self._project.pad_timing_intent[sample_id] == "tap" else "manual"
            )
        timing = self.current_timing(sample_id)
        if (
            timing is not None
            and timing.accepted_revision is not None
            and self._on_accepted_refresh is not None
        ):
            self._on_accepted_refresh(sample_id)
            return
        if (
            timing is None
            and self._project.manual_bpm[sample_id] is None
            and self.awaiting_automatic_timing(sample_id)
        ):
            return
        bpm = normalize_bpm(
            timing.bpm
            if timing is not None and timing.accepted_revision is not None
            else self._legacy_bpm(sample_id)
        )
        if publish_bpm and (timing is None or timing.accepted_revision is None):
            self._audio.set_pad_bpm(
                sample_id,
                bpm,
                **self._transport.source_generation_options(expected_source_generation),
            )
            self.project_legacy_timing_admission(sample_id)

        # Scalar timing changes revoke older preparation. Publish the one coupled
        # resident request only after the edit's final Native authority is declared.
        self._transport.loop.reclamp_grid_offset_samples(
            sample_id,
            timing=timing,
            expected_source_generation=expected_source_generation,
            publish=False,
        )
        self._transport.loop.apply_grid_anchor_to_audio(
            sample_id, timing=timing, expected_source_generation=expected_source_generation
        )
        if timing_intent is not None:
            self._audio.set_pad_timing_intent(
                sample_id,
                timing_intent,
                **self._transport.source_generation_options(expected_source_generation),
            )
            if self._project.pad_timing_intent[sample_id] != timing_intent:
                self._project.pad_timing_intent[sample_id] = timing_intent
                self._transport._mark_project_changed()
        self._transport.loop._apply_effective_pad_loop_region_to_audio(
            sample_id,
            timing=timing,
            expected_source_generation=expected_source_generation,
        )

        if self._session.bpm_lock_anchor_pad_id != sample_id:
            return

        self.recompute_master_bpm(timing=timing)


def _estimate_tap_interval_s(timestamps: list[float]) -> float | None:
    """Refit the complete series without letting isolated taps move its beat count."""
    count = len(timestamps)
    if count < 2 or any(not math.isfinite(t) for t in timestamps):
        return None
    if any(a >= b for a, b in pairwise(timestamps)):
        return None
    # Avoid loss of precision from the absolute monotonic epoch. Original indices
    # remain intact even when their timestamps receive zero weight.
    times = [t - timestamps[0] for t in timestamps]
    if count < 4:
        fit = _fit_tap_line(times, [1.0] * count)
        return fit[1] if fit is not None else None
    return _robust_tap_period(times)


def _robust_tap_period(times: list[float]) -> float | None:
    count = len(times)
    seed = _tap_seed(times)
    if seed is None:
        return None
    origin, period = seed
    for _ in range(5):
        residual = [t - origin - i * period for i, t in enumerate(times)]
        center = median(residual)
        origin += center
        cutoff = _tap_residual_cutoff(residual, period)
        weights = [
            (1 - ((r - center) / cutoff) ** 2) ** 2 if abs(r - center) < cutoff else 0.0
            for r in residual
        ]
        if sum(w > 0 for w in weights) * 2 <= count:
            return None
        fit = _fit_tap_line(times, weights)
        if fit is None:
            return None
        origin, period = fit
    return _final_tap_period(times, origin, period)


def _tap_seed(times: list[float]) -> tuple[float, float] | None:
    count = len(times)
    # Long indexed spans suppress cyclic tap jitter; adjacent IOI medians can be
    # biased (e.g. a repeating +10ms,+10ms,-20ms pattern) over long histories.
    lag = count // 2
    long_period = median((times[i + lag] - times[i]) / lag for i in range(count - lag))
    short_period = median(b - a for a, b in pairwise(times))
    best: tuple[float, float] | None = None
    best_count = count // 2
    for period in (long_period, short_period):
        origin = median(t - i * period for i, t in enumerate(times))
        residual = [t - origin - i * period for i, t in enumerate(times)]
        center = median(residual)
        cutoff = _tap_residual_cutoff(residual, period)
        supported = sum(abs(r - center) < cutoff for r in residual)
        if supported > best_count:
            best_count = supported
            best = (origin + center, period)
    return best


def _final_tap_period(times: list[float], origin: float, period: float) -> float | None:
    # Reclassify against each new fit; previously excluded taps may re-enter.
    # Final OLS uses every inlier's ORIGINAL index across the whole measurement.
    previous: list[bool] | None = None
    count = len(times)
    for _ in range(5):
        residual = [t - origin - i * period for i, t in enumerate(times)]
        center = median(residual)
        cutoff = _tap_residual_cutoff(residual, period)
        keep = [abs(r - center) <= cutoff for r in residual]
        indices = [i for i, yes in enumerate(keep) if yes]
        if len(indices) * 2 <= count or indices[-1] - indices[0] < 0.5 * (count - 1):
            return None
        fit = _fit_tap_line(times, [float(yes) for yes in keep])
        if fit is None:
            return None
        origin, period = fit
        if keep == previous:
            return period
        previous = keep
    # An unstable inlier classification must not publish a new manual tempo.
    return None


def _tap_residual_cutoff(residual: list[float], period: float) -> float:
    center = median(residual)
    jitter = 1.4826 * median(abs(r - center) for r in residual)
    # Prediction jitter is distinct from uncertainty of the fitted period: its
    # gate must not collapse as 1/sqrt(N) and eventually reject normal human taps.
    return min(0.25 * period, max(0.003, 4.5 * jitter))


def _fit_tap_line(times: list[float], weights: list[float]) -> tuple[float, float] | None:
    total = math.fsum(weights)
    if total <= 0:
        return None
    mx = math.fsum(i * w for i, w in enumerate(weights)) / total
    my = math.fsum(t * w for t, w in zip(times, weights, strict=True)) / total
    xx = math.fsum(w * (i - mx) ** 2 for i, w in enumerate(weights))
    if xx <= 0:
        return None
    period = (
        math.fsum(
            w * (i - mx) * (t - my) for i, (t, w) in enumerate(zip(times, weights, strict=True))
        )
        / xx
    )
    origin = my - period * mx
    return (origin, period) if math.isfinite(period) and period > 0 else None
