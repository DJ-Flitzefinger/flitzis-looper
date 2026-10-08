"""Frozen complete-array engineering comparisons; neither backend supplies ground truth."""

import hashlib
import math
import struct
from fractions import Fraction
from itertools import pairwise
from typing import TYPE_CHECKING

from flitzis_looper.analysis.beat_matching import match_beats
from flitzis_looper.analysis.beat_scoring import TOLERANCES_MS, _distribution

if TYPE_CHECKING:
    from flitzis_looper.analysis.beat_candidate_models import NativeCandidate
    from flitzis_looper.analysis.corrected_legacy_models import CorrectedLegacyCandidate

POLICY_VERSION = "corrected-legacy-engineering-v1"


def array_identity(
    values: tuple[float, ...] | tuple[int, ...], code: str = "d"
) -> dict[str, object]:
    """Bind all original array values in their declared IEEE or integer representation."""
    return {
        "count": len(values),
        "encoding": {"d": "float64-le", "f": "float32-le", "Q": "uint64-le"}[code],
        "sha256": hashlib.sha256(struct.pack(f"<{len(values)}{code}", *values)).hexdigest(),
    }


def ordinal_fit(times: tuple[float, ...]) -> dict[str, object]:
    """Fit every original event with exact binary64-rational equal-weight OLS.

    Event ordinals are an unverified assumption. This fit provides no missing-count
    inference, robust gate, acoustic uncertainty or accepted constant-tempo decision.
    """
    if (
        len(times) > 250000
        or any(type(t) not in {int, float} or not math.isfinite(t) or t < 0 for t in times)
        or any(a >= b for a, b in pairwise(times))
    ):
        msg = "ordinal fit requires finite complete increasing nonnegative event times"
        raise ValueError(msg)
    if len(times) < 2:
        return {"status": "unsupported", "reason": "fewer_than_two_complete_events"}
    n = len(times)
    ys = tuple(Fraction(value) for value in times)
    mx = Fraction(n - 1, 2)
    my = sum(ys) / n
    period = sum((i - mx) * (y - my) for i, y in enumerate(ys)) / sum(
        (i - mx) ** 2 for i in range(n)
    )
    intercept = my - mx * period
    residual = tuple(float(y - intercept - i * period) for i, y in enumerate(ys))
    return {
        "status": "unverified",
        "count_interpretation": "original-event-ordinal; quarter units unverified",
        "observations": n,
        "period_seconds_per_ordinal": float(period),
        "ordinal_bpm": float(60 / period),
        "diagnostic_intercept_seconds": float(intercept),
        "residual_seconds": residual,
        "max_absolute_residual_seconds": max(map(abs, residual)),
        "acoustic_timing_bound": "not_established",
    }


def compare_event_arrays(
    left: tuple[float, ...], right: tuple[float, ...]
) -> list[dict[str, object]]:
    """Report symmetric disagreements at fixed B2 tolerances with every original index."""
    results = []
    for tolerance in TOLERANCES_MS:
        matched = match_beats(left, right, tolerance / 1000)
        li = {a for a, _ in matched.pairs}
        ri = {b for _, b in matched.pairs}
        errors = tuple((right[b] - left[a]) * 1000 for a, b in matched.pairs)
        distribution = _distribution(errors)
        results.append({
            "tolerance_ms": tolerance,
            "left_events": len(left),
            "right_events": len(right),
            "matched_original_index_pairs": matched.pairs,
            "left_unmatched_indices": tuple(i for i in range(len(left)) if i not in li),
            "right_unmatched_indices": tuple(i for i in range(len(right)) if i not in ri),
            "signed_right_minus_left_errors_ms": errors,
            "signed_bias_ms": distribution.signed_bias_ms,
            "absolute_p50_ms": distribution.absolute_p50_ms,
            "absolute_p95_ms": distribution.absolute_p95_ms,
            "absolute_max_ms": distribution.absolute_max_ms,
        })
    return results


def _region_series(
    left: tuple[float, ...], right: tuple[float, ...], start: float, end: float
) -> dict[str, object]:
    left_indices = tuple(i for i, t in enumerate(left) if start <= t < end)
    right_indices = tuple(i for i, t in enumerate(right) if start <= t < end)
    left_times = tuple(left[i] for i in left_indices)
    right_times = tuple(right[i] for i in right_indices)
    # Regional matching remains separate from complete-track matching. Retain
    # its index map explicitly so cropped-window endpoints never hide events.
    return {
        "legacy_original_indices": left_indices,
        "selected_original_indices": right_indices,
        "legacy_ordinal_fit": ordinal_fit(left_times),
        "selected_ordinal_fit": ordinal_fit(right_times),
        "legacy_local_interval_seconds": tuple(b - a for a, b in pairwise(left_times)),
        "selected_local_interval_seconds": tuple(b - a for a, b in pairwise(right_times)),
        "disagreement_with_regional_indices": compare_event_arrays(left_times, right_times),
        "cross_boundary_matches": "reported separately by complete-track comparison",
    }


def engineering_compare(
    selected: NativeCandidate, legacy: CorrectedLegacyCandidate
) -> dict[str, object]:
    """Compare verified complete same-source arrays without treating either as a reference."""
    loaded = legacy.result.envelope.loaded
    if (
        selected.track_id,
        selected.source_sha256,
        selected.source_bytes,
        selected.pcm_sha256,
        selected.request.pcm.sample_rate_hz,
        selected.request.pcm.frame_count,
    ) != (
        legacy.track_id,
        legacy.source_sha256,
        legacy.source_bytes,
        loaded.mono_sha256,
        loaded.sample_rate_hz,
        loaded.frame_count,
    ):
        msg = "engineering comparison requires the identical complete original and loaded PCM"
        raise ValueError(msg)
    left = legacy.result.beat_seconds
    right = selected.predictions.beat_seconds
    duration = loaded.frame_count / loaded.sample_rate_hz
    windows = (
        ("middle", 0.2, 0.8),
        ("early", 0, 1 / 3),
        ("central", 1 / 3, 2 / 3),
        ("late", 2 / 3, 1),
    )
    regions = [
        {
            "id": name,
            "start_seconds": first * duration,
            "end_seconds": end * duration,
            "legacy_original_indices": tuple(
                i for i, t in enumerate(left) if first * duration <= t < end * duration
            ),
            "selected_original_indices": tuple(
                i for i, t in enumerate(right) if first * duration <= t < end * duration
            ),
            "beats": _region_series(left, right, first * duration, end * duration),
            "downbeats": _region_series(
                legacy.result.downbeat_seconds,
                selected.predictions.downbeat_seconds,
                first * duration,
                end * duration,
            ),
        }
        for name, first, end in windows
    ]
    return {
        "policy_version": POLICY_VERSION,
        "status": "complete_engineering_comparison",
        "track_id": selected.track_id,
        "scope": "same-source backend disagreement; neither side is musical ground truth",
        "source_sha256": selected.source_sha256,
        "pcm_sha256": selected.pcm_sha256,
        "complete_loaded_duration_seconds": duration,
        "legacy": legacy.report(),
        "selected": selected.report(),
        "complete_array_identities": {
            "legacy": {
                name: array_identity(getattr(legacy.result, name), code)
                for name, code in (
                    ("beat_frames", "d"),
                    ("downbeat_raw_indices", "Q"),
                    ("beat_seconds", "d"),
                    ("downbeat_seconds", "d"),
                    ("compatibility_beats", "f"),
                    ("compatibility_downbeats", "f"),
                    ("compatibility_bars", "f"),
                )
            }
            | {"compatibility_bpm": array_identity((legacy.result.compatibility_bpm,), "f")},
            "selected": {
                name: array_identity(getattr(selected.predictions, name))
                for name in (
                    "beat_seconds",
                    "downbeat_seconds",
                    "beat_logits",
                    "downbeat_logits",
                )
            },
        },
        "legacy_complete_ordinal_fit": ordinal_fit(left),
        "selected_complete_ordinal_fit": ordinal_fit(right),
        "legacy_complete_downbeat_ordinal_fit": ordinal_fit(legacy.result.downbeat_seconds),
        "selected_complete_downbeat_ordinal_fit": ordinal_fit(
            selected.predictions.downbeat_seconds
        ),
        "legacy_local_interval_seconds": tuple(b - a for a, b in pairwise(left)),
        "selected_local_interval_seconds": tuple(b - a for a, b in pairwise(right)),
        "legacy_downbeat_local_interval_seconds": tuple(
            b - a for a, b in pairwise(legacy.result.downbeat_seconds)
        ),
        "selected_downbeat_local_interval_seconds": tuple(
            b - a for a, b in pairwise(selected.predictions.downbeat_seconds)
        ),
        "legacy_compatibility_bpm": legacy.result.compatibility_bpm,
        "legacy_detector_lattice_halfwidth_seconds": legacy.result.envelope.analyzer.odf_hop_samples
        / 88200,
        "selected_detector_lattice_halfwidth_seconds": 0.01,
        "detector_lattice_claim": "conditional only; no acoustic or musical bound",
        "beat_disagreement": compare_event_arrays(left, right),
        "downbeat_disagreement": compare_event_arrays(
            legacy.result.downbeat_seconds, selected.predictions.downbeat_seconds
        ),
        "predeclared_regions": regions,
        "independent_reference": "not_supplied",
        "musical_scores": "not_run",
        "paired_human_correction": "not_measured",
        "resource_acceptance": "not_established",
        "default_adoption": "blocked",
    }
