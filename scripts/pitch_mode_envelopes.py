"""Fit bounded nonnegative stationary-carrier envelopes from original stereo PCM.

The default frozen offline diagnostic uses centered source windows and global
carrier phase, with 128 source frames of lookahead. The opt-in trailing-window
fit is locally causal only for envelopes against an already fixed carrier basis;
it makes no global carrier-discovery causality claim. The caller must retain every
fitting error in an exact PCM remainder and fully transform that remainder.
Neither fit makes a fractional-frequency quality claim.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]
type Mask = NDArray[np.bool_]
type Indices = NDArray[np.int64]

FS = 48_000
WINDOW_FRAMES = 256
HOP_FRAMES = 64
MAX_CARRIERS = 6
MAX_CONDITION = 1.0e6
MAX_ACTIVE_SOLVES = 128
MAX_MEMORY_BYTES = 256 * 1024 * 1024
_OFFSETS = np.arange(WINDOW_FRAMES, dtype=np.int64) - WINDOW_FRAMES // 2
_CAUSAL_OFFSETS = np.arange(WINDOW_FRAMES, dtype=np.int64) - (WINDOW_FRAMES - 1)
_HANN = 0.5 - 0.5 * np.cos(2.0 * np.pi * np.arange(WINDOW_FRAMES) / (WINDOW_FRAMES - 1))


@dataclass(slots=True)
class _Solution:
    values: Real
    status: str
    solves: int
    condition: float | None


@dataclass(slots=True)
class _ActiveSet:
    values: Real
    passive: Mask
    solves: int = 0
    status: str = "converged"


def _require(message: str, *, condition: bool) -> None:
    if not condition:
        raise ValueError(message)


def _memory_estimate(frames: int, carriers: int) -> int:
    """Bound PCM, output, interpolation, knot arrays and small tile workspace."""
    knots = (frames + HOP_FRAMES - 1) // HOP_FRAMES + 1
    return frames * (48 + 8 * carriers) + knots * (8 * carriers + 16) + 1024 * 1024


def _validate_inputs(source: Real, frequencies_hz: Real, stereo_coefficients: Complex) -> None:
    _require(
        "Source must have shape (frames, 2).", condition=source.ndim == 2 and source.shape[1] == 2
    )
    _require("Source must be real float64 PCM.", condition=source.dtype == np.float64)
    _require("Carrier frequencies must be one-dimensional.", condition=frequencies_hz.ndim == 1)
    _require("At most six carriers are supported.", condition=len(frequencies_hz) <= MAX_CARRIERS)
    _require(
        "Stereo coefficients must have shape (carriers, 2).",
        condition=stereo_coefficients.shape == (len(frequencies_hz), 2),
    )
    _require("Source must contain only finite samples.", condition=np.isfinite(source).all())
    _require("Carrier frequencies must be finite.", condition=np.isfinite(frequencies_hz).all())
    _require(
        "Stereo coefficients must be finite.", condition=np.isfinite(stereo_coefficients).all()
    )
    _require(
        "Carrier frequencies must be between DC and Nyquist.",
        condition=np.all((frequencies_hz >= 0.0) & (frequencies_hz <= FS / 2.0)),
    )
    _require(
        "Each complex stereo coefficient must have unit Euclidean norm.",
        condition=np.allclose(
            np.sum(np.abs(stereo_coefficients) ** 2, axis=1), 1.0, atol=1e-10, rtol=0
        ),
    )
    _require(
        "The conservative fit memory estimate exceeds the 256 MiB limit.",
        condition=_memory_estimate(len(source), len(frequencies_hz)) < MAX_MEMORY_BYTES,
    )


def _activity_hull(source: Real) -> tuple[int, int] | None:
    active = np.flatnonzero(np.any(source != 0.0, axis=1))
    return (int(active[0]), int(active[-1])) if len(active) else None


def _knot_centers(hull: tuple[int, int]) -> Indices:
    centers = np.arange(hull[0], hull[1] + 1, HOP_FRAMES, dtype=np.int64)
    if centers[-1] != hull[1]:
        centers = np.append(centers, hull[1])
    return centers


def _tile_system(
    source: Real,
    frequencies_hz: Real,
    stereo_coefficients: Complex,
    center: int,
    *,
    offsets: Indices = _OFFSETS,
) -> tuple[Real, Real]:
    positions = center + offsets
    inside = (positions >= 0) & (positions < len(source))
    positions = positions[inside]
    weights = np.sqrt(_HANN[inside])
    carriers = np.exp(2.0j * np.pi * np.outer(positions, frequencies_hz) / FS)
    features = (carriers[:, :, None] * stereo_coefficients[None, :, :]).real
    design = (features * weights[:, None, None]).transpose(0, 2, 1)
    return design.reshape(-1, len(frequencies_hz)), (source[positions] * weights[:, None]).ravel()


def _passive_fit(design: Real, target: Real, state: _ActiveSet) -> bool:
    """Take bounded Lawson-Hanson inner steps using SVD least squares."""
    while np.any(state.passive):
        if state.solves >= MAX_ACTIVE_SOLVES:
            state.status = "iteration_limit"
            return False
        state.solves += 1
        trial = np.zeros_like(state.values)
        try:
            trial[state.passive] = np.linalg.lstsq(design[:, state.passive], target, rcond=None)[0]
        except np.linalg.LinAlgError:
            state.status = "svd_failure"
            return False
        if not np.isfinite(trial).all():
            state.status = "nonfinite_solution"
            return False
        if np.all(trial[state.passive] > 0.0):
            state.values = trial
            return True
        blocked = state.passive & (trial <= 0.0)
        denominator = state.values[blocked] - trial[blocked]
        ratios = np.full(len(trial), np.inf)
        ratios[blocked] = np.divide(
            state.values[blocked],
            denominator,
            out=np.zeros_like(denominator),
            where=denominator != 0,
        )
        alpha = float(np.min(ratios))
        state.values += alpha * (trial - state.values)
        leaving = blocked & (ratios <= alpha)
        state.values[leaving] = 0.0
        state.passive[leaving] = False
        np.maximum(state.values, 0.0, out=state.values)
    return True


def _lawson_hanson(design: Real, target: Real, tolerance: float, condition: float) -> _Solution:
    state = _ActiveSet(np.zeros(design.shape[1]), np.zeros(design.shape[1], dtype=np.bool_))
    while True:
        gradient = design.T @ (target - design @ state.values)
        candidates = np.where(state.passive, -np.inf, gradient)
        if float(np.max(candidates)) <= tolerance:
            return _Solution(state.values, "converged", state.solves, condition)
        state.passive[int(np.argmax(candidates))] = True
        if not _passive_fit(design, target, state):
            return _Solution(np.zeros_like(state.values), state.status, state.solves, condition)


def _fit_tile(design: Real, target: Real) -> _Solution:
    """Reject unresolved or ill-conditioned tiles explicitly, preserving no partial fit."""
    zero = np.zeros(design.shape[1])
    if design.shape[0] < design.shape[1]:
        return _Solution(zero, "ill_conditioned", 0, None)
    try:
        singular = np.linalg.svd(design, compute_uv=False)
    except np.linalg.LinAlgError:
        return _Solution(zero, "svd_failure", 0, None)
    if not np.isfinite(singular).all() or singular[-1] <= 0.0:
        return _Solution(zero, "ill_conditioned", 0, None)
    condition = float(singular[0] / singular[-1])
    if not np.isfinite(condition) or condition > MAX_CONDITION:
        return _Solution(zero, "ill_conditioned", 0, condition if np.isfinite(condition) else None)
    tolerance = 64.0 * np.finfo(np.float64).eps * float(singular[0]) * float(np.linalg.norm(target))
    return _lawson_hanson(design, target, tolerance, condition)


def _record_tile(summary: dict, center: int, result: _Solution) -> None:
    counts = summary["tile_status_counts"]
    counts[result.status] = counts.get(result.status, 0) + 1
    summary["maximum_active_set_solves"] = max(summary["maximum_active_set_solves"], result.solves)
    summary["total_active_set_solves"] += result.solves
    if result.condition is not None:
        summary["maximum_condition_number"] = max(
            summary["maximum_condition_number"] or 0.0, result.condition
        )
    if result.status != "converged":
        summary["rejected_tile_count"] += 1
        if len(summary["first_rejected_tile_centers"]) < 16:
            summary["first_rejected_tile_centers"].append(center)


def _fit_knots(
    source: Real,
    frequencies_hz: Real,
    stereo_coefficients: Complex,
    centers: Indices,
    *,
    offsets: Indices = _OFFSETS,
) -> tuple[Real, dict]:
    amplitudes = np.zeros((len(centers), len(frequencies_hz)))
    summary = {
        "tile_count": len(centers),
        "tile_status_counts": {},
        "maximum_condition_number": None,
        "maximum_active_set_solves": 0,
        "total_active_set_solves": 0,
        "rejected_tile_count": 0,
        "first_rejected_tile_centers": [],
    }
    for index, center in enumerate(centers):
        design, target = _tile_system(
            source, frequencies_hz, stereo_coefficients, int(center), offsets=offsets
        )
        result = _fit_tile(design, target)
        amplitudes[index] = result.values
        _record_tile(summary, int(center), result)
    summary["omitted_rejected_tile_detail_count"] = summary["rejected_tile_count"] - len(
        summary["first_rejected_tile_centers"]
    )
    return amplitudes, summary


def _base_report(source: Real, frequencies_hz: Real, hull: tuple[int, int] | None) -> dict:
    return {
        "sample_rate_hz": FS,
        "window_frames": WINDOW_FRAMES,
        "window_offsets_inclusive": [-128, 127],
        "window": "symmetric Hann; sqrt weights in source least squares",
        "hop_frames": HOP_FRAMES,
        "condition_limit": MAX_CONDITION,
        "maximum_passive_solves_per_tile": MAX_ACTIVE_SOLVES,
        "gradient_tolerance": "64*float64_epsilon*design_spectral_norm*target_euclidean_norm",
        "carrier_count": len(frequencies_hz),
        "source_activity_hull_inclusive": list(hull) if hull is not None else None,
        "fixed_mode_birth_frames": [hull[0]] * len(frequencies_hz) if hull is not None else [],
        "global_carrier_phase_origin_frame": 0,
        "source_lookahead_frames": WINDOW_FRAMES // 2,
        "offline_noncausal_fit": True,
        "exact_hull_mask_only": True,
        "independent_nonnegative_mode_envelopes": True,
        "output_fit_regularization_or_event_metadata": False,
        "fractional_frequency_quality_claim": False,
        "exact_pcm_remainder_required": True,
        "estimated_peak_working_bytes": _memory_estimate(len(source), len(frequencies_hz)),
        "tile_count": 0,
        "tile_status_counts": {},
        "maximum_condition_number": None,
        "maximum_active_set_solves": 0,
        "total_active_set_solves": 0,
        "rejected_tile_count": 0,
        "first_rejected_tile_centers": [],
        "omitted_rejected_tile_detail_count": 0,
    }


def fit_envelopes(
    source: Real, frequencies_hz: Real, stereo_coefficients: Complex
) -> tuple[Real, dict]:
    """Return independent real source envelopes and explicit bounded-fit evidence.

    Args:
        source: Original finite float64 PCM with shape (frames, 2), at 48 kHz.
        frequencies_hz: Source-only stationary carrier estimates, at most six.
        stereo_coefficients: Complex unit-norm stereo vectors with shape (K, 2),
            referenced to the original source's global sample-zero carrier phase.

    Returns:
        Nonnegative envelopes with shape (frames, K), linearly interpolated from
        Hann-weighted NNLS knots, and a compact report of every tile outcome.
        All samples outside the exact common source activity hull remain zero.
        Rejected tiles contribute an explicit zero knot; their source stays in
        the caller's mandatory exact PCM remainder.
    """
    _validate_inputs(source, frequencies_hz, stereo_coefficients)
    hull = _activity_hull(source)
    report = _base_report(source, frequencies_hz, hull)
    envelopes = np.zeros((len(source), len(frequencies_hz)))
    if hull is None or not len(frequencies_hz):
        return envelopes, report
    centers = _knot_centers(hull)
    amplitudes, summary = _fit_knots(source, frequencies_hz, stereo_coefficients, centers)
    positions = np.arange(hull[0], hull[1] + 1)
    for mode in range(len(frequencies_hz)):
        envelopes[hull[0] : hull[1] + 1, mode] = np.interp(positions, centers, amplitudes[:, mode])
    report.update(summary)
    return envelopes, report


def fit_causal_envelopes(
    source: Real, frequencies_hz: Real, stereo_coefficients: Complex
) -> tuple[Real, dict]:
    """Return trailing-window, left-held envelopes against a fixed global basis.

    Args:
        source: Original finite float64 PCM with shape (frames, 2), at 48 kHz.
        frequencies_hz: Already fixed source-only carrier estimates, at most six.
        stereo_coefficients: Fixed complex unit-norm stereo vectors with shape
            (K, 2), referenced to the original source's sample-zero carrier phase.

    Returns:
        Nonnegative envelopes with shape (frames, K) and bounded-fit evidence.
        Each knot fits the preceding 256 source frames, including its own frame,
        using a symmetric Hann window. Integer source positions hold their most
        recent knot; there is no interpolation from future knots. A fixed grid
        from the first active frame continues through the finite input, retaining
        fitted decay after its last active sample. All rejected-tile and other
        fit error must remain in the caller's exact PCM remainder.

        Local envelope fitting has zero source lookahead. This remains an offline
        fit with an already fixed global carrier basis;
        neither carrier discovery nor the complete pipeline is claimed causal.
    """
    _validate_inputs(source, frequencies_hz, stereo_coefficients)
    hull = _activity_hull(source)
    report = _base_report(source, frequencies_hz, hull)
    report.update({
        "window_offsets_inclusive": [-(WINDOW_FRAMES - 1), 0],
        "window": "trailing symmetric Hann; sqrt weights in source least squares",
        "source_lookahead_frames": 0,
        "source_lookback_frames": WINDOW_FRAMES - 1,
        "envelope_interpolation": "left hold; searchsorted(side=right)-1 at integer source frames",
        "knot_grid": "first active source frame + 64-frame grid through finite input; no end knot",
        "exact_hull_mask_only": False,
        "last_active_source_frame_used_as_mask_or_knot": False,
        "fitted_decay_retained_through_finite_input": True,
        "offline_fit": True,
        "offline_noncausal_fit": False,
        "envelope_fit_local_causal": True,
        "global_carrier_discovery_causality_claim": False,
        "complete_pipeline_causality_claim": False,
    })
    envelopes = np.zeros((len(source), len(frequencies_hz)))
    if hull is None or not len(frequencies_hz):
        return envelopes, report
    centers = np.arange(hull[0], len(source), HOP_FRAMES, dtype=np.int64)
    amplitudes, summary = _fit_knots(
        source, frequencies_hz, stereo_coefficients, centers, offsets=_CAUSAL_OFFSETS
    )
    positions = np.arange(hull[0], len(source))
    knot_indices = np.searchsorted(centers, positions, side="right") - 1
    for mode in range(len(frequencies_hz)):
        envelopes[hull[0] :, mode] = amplitudes[knot_indices, mode]
    report["causal_processing_span_inclusive"] = [hull[0], len(source) - 1]
    report.update(summary)
    return envelopes, report


def _nnls_controls() -> dict:
    design = np.array([[1.0, 1.0], [1.0, 0.0], [0.0, 1.0]])
    target = np.array([1.0, 1.0, -1.0])
    result = _fit_tile(design, target)
    error = float(np.max(np.abs(result.values - np.array([1.0, 0.0]))))
    gradient = design.T @ (target - design @ result.values)
    rejected = _fit_tile(np.ones((8, 2)), np.ones(8))
    assert result.status == "converged"
    assert error < 1e-12
    assert abs(float(gradient[0])) < 1e-12
    assert gradient[1] < 0.0
    assert rejected.status == "ill_conditioned"
    assert np.all(rejected.values == 0.0)
    return {
        "known_constrained_optimum_max_abs_error": error,
        "known_optimum_kkt_gradient": gradient.tolist(),
        "duplicate_feature_condition_refusal": True,
    }


def _control_coefficients() -> Complex:
    coefficients = np.array([
        [np.sqrt(0.7) * np.exp(0.31j), np.sqrt(0.3) * np.exp(-0.27j)],
        [np.sqrt(0.2) * np.exp(1.13j), np.sqrt(0.8) * np.exp(-0.54j)],
    ])
    return coefficients / np.linalg.norm(coefficients, axis=1)[:, None]


def _gated_control() -> dict:
    frequencies = np.array([683.17, 1901.29])
    coefficients = _control_coefficients()
    expected = np.zeros((2048, 2))
    expected[128:1472, 0] = 0.24
    expected[768:1920, 1] = 0.16
    carriers = np.exp(2.0j * np.pi * np.outer(np.arange(len(expected)), frequencies) / FS)
    source = np.sum(expected[:, :, None] * coefficients[None, :, :] * carriers[:, :, None], axis=1)
    fitted, report = fit_envelopes(source.real.copy(), frequencies, coefficients)
    regions = (slice(384, 512), slice(1024, 1152), slice(1664, 1728))
    error = max(float(np.max(np.abs(fitted[region] - expected[region]))) for region in regions)
    assert error < 1e-10
    assert np.all(fitted >= 0.0)
    assert np.all(fitted[:128] == 0.0)
    assert np.all(fitted[1920:] == 0.0)
    assert report["fixed_mode_birth_frames"] == [128, 128]
    assert report["rejected_tile_count"] == 0
    return {
        "unseen_carrier_frequencies_hz": frequencies.tolist(),
        "independently_gated_stereo_plateau_max_abs_error": error,
        "nonnegative_independent_envelopes_and_exact_hull": True,
        "separate_complex_stereo_vectors_preserved_in_joint_fit": True,
        "window_boundary_transition_accuracy_claim": False,
    }


def integrity_controls() -> dict:
    """Exercise a known NNLS optimum, refusal, silence and independent stereo gates."""
    silence, report = fit_envelopes(
        np.zeros((512, 2)), np.array([421.73]), _control_coefficients()[:1]
    )
    assert np.all(silence == 0.0)
    assert report["tile_count"] == 0
    return {
        "stage": "mode_envelope_integrity_controls",
        **_nnls_controls(),
        **_gated_control(),
        "silence_exact_zero_without_fitting": True,
        "source_oracle_gate_parameters_only_in_integrity_controls": True,
    }


def _causal_control_fit(source: Real, frequencies: Real, coefficients: Complex) -> Real:
    fitted, report = fit_causal_envelopes(source, frequencies, coefficients)
    assert np.all(fitted >= 0.0)
    assert np.all(fitted[:129] == 0.0)
    assert np.any(fitted[1920:] != 0.0)
    assert report["source_activity_hull_inclusive"] == [129, 1919]
    assert report["fixed_mode_birth_frames"] == [129, 129]
    assert report["rejected_tile_count"] == 0
    assert report["source_lookahead_frames"] == 0
    assert report["source_lookback_frames"] == 255
    assert report["window_offsets_inclusive"] == [-255, 0]
    assert report["causal_processing_span_inclusive"] == [129, 2047]
    return fitted


def _causal_future_impulse_control() -> dict:
    event_frame = 1024
    frequencies = np.array([683.17, 1901.29])
    coefficients = _control_coefficients()
    carriers = np.exp(2.0j * np.pi * np.outer(np.arange(2048), frequencies) / FS)
    features = (carriers[:, :, None] * coefficients[None, :, :]).real
    source = np.sum(np.array([0.24, 0.16])[None, :, None] * features, axis=1)
    # An off-grid impulse exposes future-knot interpolation as well as future windows.
    source[:129] = 0.0
    source[1920:] = 0.0
    changed = source.copy()
    changed[event_frame] += np.array([0.47, -0.31])
    assert np.array_equal(source[:event_frame], changed[:event_frame])
    fitted = _causal_control_fit(source, frequencies, coefficients)
    changed_fitted = _causal_control_fit(changed, frequencies, coefficients)
    modes = np.sum(fitted[:, :, None] * features, axis=1)
    changed_modes = np.sum(changed_fitted[:, :, None] * features, axis=1)
    mode_error = source - modes
    changed_mode_error = changed - changed_modes
    assert np.array_equal(fitted[:event_frame], changed_fitted[:event_frame])
    assert np.array_equal(modes[:event_frame], changed_modes[:event_frame])
    assert np.array_equal(mode_error[:event_frame], changed_mode_error[:event_frame])
    assert np.any(fitted[event_frame:] != changed_fitted[event_frame:])
    assert np.max(np.abs(fitted[449:960] - np.array([0.24, 0.16]))) < 1e-10
    return {
        "common_fixed_carrier_frequencies_hz": frequencies.tolist(),
        "identical_fixed_complex_stereo_basis_for_both_sources": True,
        "future_impulse_frame": event_frame,
        "future_impulse_between_envelope_knots": True,
        "common_source_activity_hull_inclusive": [129, 1919],
        "nonnegative_envelopes_and_fitted_post_activity_decay_retained": True,
        "pre_event_envelope_max_abs_difference": float(
            np.max(np.abs(fitted[:event_frame] - changed_fitted[:event_frame]))
        ),
        "pre_event_source_modes_max_abs_difference": float(
            np.max(np.abs(modes[:event_frame] - changed_modes[:event_frame]))
        ),
        "pre_event_complementary_mode_error_max_abs_difference": float(
            np.max(np.abs(mode_error[:event_frame] - changed_mode_error[:event_frame]))
        ),
        "future_impulse_changes_post_event_envelopes": True,
        "local_causality_only_against_fixed_global_carrier_basis": True,
        "target_renderer_or_output_fit_used": False,
    }


def _causal_refusal_control() -> dict:
    fitted, report = fit_causal_envelopes(
        np.ones((512, 2)),
        np.array([421.73, 421.73]),
        _control_coefficients()[[0, 0]],
    )
    assert report["tile_count"] > 0
    assert report["rejected_tile_count"] == report["tile_count"]
    assert set(report["tile_status_counts"]) == {"ill_conditioned"}
    assert np.all(fitted == 0.0)
    return {
        "causal_duplicate_carrier_all_knots_refused": True,
        "causal_duplicate_carrier_zero_envelopes_leave_source_in_exact_remainder": True,
        "causal_duplicate_carrier_refused_tile_count": report["rejected_tile_count"],
    }


def _causal_different_hull_control() -> dict:
    frequencies = np.array([683.17, 1901.29])
    coefficients = _control_coefficients()
    clock = np.arange(2048)
    carriers = np.exp(2.0j * np.pi * np.outer(clock, frequencies) / FS)
    features = (carriers[:, :, None] * coefficients[None, :, :]).real
    source = np.sum(np.array([0.24, 0.16])[None, :, None] * features, axis=1)
    source[:129] = 0.0
    source[800:] = 0.0
    changed = source.copy()
    changed[1024] = (0.47, -0.31)
    first, first_report = fit_causal_envelopes(source, frequencies, coefficients)
    second, second_report = fit_causal_envelopes(changed, frequencies, coefficients)
    assert first_report["source_activity_hull_inclusive"] == [129, 799]
    assert second_report["source_activity_hull_inclusive"] == [129, 1024]
    assert np.any(first[800:1024] != 0.0)
    assert np.array_equal(first[:1024], second[:1024])
    return {
        "different_last_active_frames_prefix_envelopes_exactly_equal": True,
        "post_activity_decay_not_removed_by_future_hull": True,
        "different_hull_prefix_max_abs_difference": float(
            np.max(np.abs(first[:1024] - second[:1024]))
        ),
    }


def causal_integrity_controls() -> dict:
    """Check local fixed-basis causality, retained decay, shared NNLS and refusal.

    Returns:
        Separate causal-path evidence without changing the default fit controls.
        Two sources with the same hull and fixed carrier basis differ only by a
        frame-1024 impulse. Their earlier envelopes and complementary source-mode
        errors must remain exactly identical, while later envelopes must differ.
        A separate changed-last-activity case checks retained decay without a
        future-dependent hull mask or extra final knot.
        This does not establish causality of global carrier discovery.
    """
    silence, report = fit_causal_envelopes(
        np.zeros((512, 2)), np.array([421.73]), _control_coefficients()[:1]
    )
    assert np.all(silence == 0.0)
    assert report["tile_count"] == 0
    return {
        "stage": "causal_mode_envelope_integrity_controls",
        **_nnls_controls(),
        **_causal_future_impulse_control(),
        **_causal_refusal_control(),
        **_causal_different_hull_control(),
        "silence_exact_zero_without_fitting": True,
        "source_oracle_gate_parameters_only_in_integrity_controls": True,
        "global_carrier_discovery_causality_claim": False,
    }
