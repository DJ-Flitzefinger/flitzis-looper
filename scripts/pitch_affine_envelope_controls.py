"""Independent finite-PCM controls for the fixed-basis causal affine fit.

Known carrier amplitudes appear only in control-source generation and analytic
expectations. The public estimator receives original stereo PCM, frequencies and
complex stereo vectors. No target rendering, fixture metadata or private fitting
helper supplies an expected answer to the estimator.
"""

from typing import TYPE_CHECKING

import numpy as np
from pitch_mode_envelopes import fit_affine_causal_envelopes, fit_causal_envelopes

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]
type Indices = NDArray[np.int64]

# Freeze geometry and acceptance limits before measuring any control result.
_FS = 48_000
_WINDOW = 256
_HOP = 64
_FIRST_ACTIVE = 129
_EVENT = 1024
_LIMITS = {
    "physical_amplitude_max_abs_error": 2.0e-10,
    "left_hold_error_identity_max_abs_error": 3.0e-10,
    "slope_max_abs_error_per_frame": 2.0e-12,
    "analytic_condition_max_abs_error": 2.0e-10,
    "analytic_leverage_max_abs_error": 2.0e-10,
    "kernel_sum_max_abs_error": 2.0e-12,
    "kernel_first_moment_max_abs_error_frames": 2.0e-10,
    "kernel_negative_lobe_required_below": -1.0e-5,
    "unconstrained_vs_active_norm_minimum_gap": 1.0e-2,
    "v2_constant_fit_mean_abs_lag_minimum": 1.0e-3,
    "left_hold_truth_error_minimum": 1.0e-4,
    "post_event_envelope_difference_minimum": 1.0e-8,
    "post_event_source_modes_difference_minimum": 1.0e-8,
    "complementary_sum_max_abs_error": 2.0e-15,
    "condition_limit": 1.0e6,
    "maximum_passive_solves_per_tile": 128,
}


def _basis() -> tuple[Real, Complex]:
    frequencies = np.array([683.17, 1901.29])
    coefficients = np.array([
        [np.sqrt(0.7) * np.exp(0.31j), np.sqrt(0.3) * np.exp(-0.27j)],
        [np.sqrt(0.2) * np.exp(1.13j), np.sqrt(0.8) * np.exp(-0.54j)],
    ])
    return frequencies, coefficients / np.linalg.norm(coefficients, axis=1)[:, None]


def _features(frames: int, frequencies: Real, coefficients: Complex) -> Real:
    clock = np.arange(frames)
    oscillators = np.exp(2.0j * np.pi * np.outer(clock, frequencies) / _FS)
    return (oscillators[:, :, None] * coefficients[None, :, :]).real


def _source(amplitudes: Real, frequencies: Real, coefficients: Complex) -> Real:
    """Generate control PCM; amplitudes never enter either estimator API."""
    return np.sum(
        amplitudes[:, :, None] * _features(len(amplitudes), frequencies, coefficients), axis=1
    )


def _max_error(actual: Real, expected: Real) -> float:
    return float(np.max(np.abs(actual - expected)))


def _check_report(report: dict, frames: int, carriers: int, last_active: int) -> None:
    assert report["carrier_count"] == carriers
    assert report["design_column_count"] == 2 * carriers
    assert report["window_frames"] == _WINDOW
    assert report["window_offsets_inclusive"] == [-255, 0]
    assert report["hop_frames"] == _HOP
    assert report["condition_limit"] == _LIMITS["condition_limit"]
    assert report["maximum_passive_solves_per_tile"] == _LIMITS["maximum_passive_solves_per_tile"]
    assert report["maximum_active_set_solves"] <= _LIMITS["maximum_passive_solves_per_tile"]
    assert report["source_lookahead_frames"] == 0
    assert report["source_lookback_frames"] == 255
    assert report["source_activity_hull_inclusive"] == [_FIRST_ACTIVE, last_active]
    assert report["fixed_mode_birth_frames"] == [_FIRST_ACTIVE] * carriers
    assert report["causal_processing_span_inclusive"] == [_FIRST_ACTIVE, frames - 1]
    expected_knots = list(range(_FIRST_ACTIVE, frames, _HOP))
    assert report["tile_count"] == len(expected_knots)
    details = report["first_knot_details"]
    assert [detail["center_frame"] for detail in details] == expected_knots[:16]
    assert report["omitted_knot_detail_count"] == max(0, len(expected_knots) - 16)
    assert (
        report["unconstrained_current_endpoint_leverage_summary"][
            "nnls_active_set_sensitivity_claim"
        ]
        is False
    )


def _check_envelopes(envelopes: Real, frames: int, carriers: int) -> None:
    assert envelopes.shape == (frames, carriers)
    assert np.isfinite(envelopes).all()
    assert np.all(envelopes >= 0.0)
    assert np.all(envelopes[:_FIRST_ACTIVE] == 0.0)


def _constant_plateau_control() -> dict:
    frequencies, coefficients = _basis()
    truth = np.zeros((4096, 2))
    truth[_FIRST_ACTIVE:] = [0.24, 0.16]
    source = _source(truth, frequencies, coefficients)
    affine, report = fit_affine_causal_envelopes(source, frequencies, coefficients)
    constant, constant_report = fit_causal_envelopes(source, frequencies, coefficients)
    _check_envelopes(affine, len(source), 2)
    _check_report(report, len(source), 2, len(source) - 1)
    assert report["rejected_tile_count"] == 0
    assert constant_report["rejected_tile_count"] == 0
    # The first fully supported knot is 385: its window is [130, 385].
    interior = slice(_FIRST_ACTIVE + 4 * _HOP, len(source))
    affine_error = _max_error(affine[interior], truth[interior])
    constant_error = _max_error(constant[interior], truth[interior])
    assert affine_error < _LIMITS["physical_amplitude_max_abs_error"]
    assert constant_error < _LIMITS["physical_amplitude_max_abs_error"]
    return {
        "frequencies_hz": frequencies.tolist(),
        "different_complex_stereo_vectors": {
            "real": coefficients.real.tolist(),
            "imaginary": coefficients.imag.tolist(),
        },
        "fully_supported_plateau_span_inclusive": [interior.start, len(source) - 1],
        "affine_max_abs_amplitude_error": affine_error,
        "v2_constant_max_abs_amplitude_error": constant_error,
        "startup_or_source_edge_linearity_claim": False,
    }


def _reported_affine_endpoints(report: dict, truth: Real, slopes: Real) -> dict:
    errors = {"left": 0.0, "current_right": 0.0, "slope_per_frame": 0.0}
    centers = []
    for detail in report["first_knot_details"]:
        center = detail["center_frame"]
        if center - (_WINDOW - 1) < _FIRST_ACTIVE:
            continue
        assert detail["status"] == "converged"
        centers.append(center)
        errors["left"] = max(
            errors["left"], _max_error(np.asarray(detail["left_endpoints"]), truth[center - 255])
        )
        errors["current_right"] = max(
            errors["current_right"],
            _max_error(np.asarray(detail["current_right_endpoints"]), truth[center]),
        )
        errors["slope_per_frame"] = max(
            errors["slope_per_frame"], _max_error(np.asarray(detail["slopes_per_frame"]), slopes)
        )
    assert centers
    assert errors["left"] < _LIMITS["physical_amplitude_max_abs_error"]
    assert errors["current_right"] < _LIMITS["physical_amplitude_max_abs_error"]
    assert errors["slope_per_frame"] < _LIMITS["slope_max_abs_error_per_frame"]
    return {"checked_first_detail_full_window_centers": centers, "maximum_endpoint_errors": errors}


def _hold_error_report(fitted: Real, truth: Real, slopes: Real, first_knot: int) -> dict:
    positions = np.arange(first_knot, len(fitted))
    last_knots = _FIRST_ACTIVE + ((positions - _FIRST_ACTIVE) // _HOP) * _HOP
    assert np.array_equal(fitted[positions], fitted[last_knots])
    expected = -(positions - last_knots)[:, None] * slopes
    identity_error = _max_error(fitted[positions] - truth[positions], expected)
    measured = _max_error(fitted[positions], truth[positions])
    assert identity_error < _LIMITS["left_hold_error_identity_max_abs_error"]
    assert measured > _LIMITS["left_hold_truth_error_minimum"]
    assert positions[-1] != last_knots[-1]
    assert np.array_equal(fitted[-1], fitted[last_knots[-1]])
    return {
        "left_hold_error_identity": "a_hat[n]-a[n] = -slope*(n-last_knot[n])",
        "left_hold_error_identity_max_abs_error": identity_error,
        "left_hold_measured_max_abs_truth_error": measured,
        "left_hold_analytic_max_abs_truth_error": float(np.max(np.abs(slopes)) * (_HOP - 1)),
        "finite_last_frame": int(positions[-1]),
        "finite_last_regular_knot": int(last_knots[-1]),
        "extra_end_knot_absent": True,
    }


def _v2_lag_report(constant: Real, truth: Real, slopes: Real, knots: Indices) -> dict:
    errors = constant[knots] - truth[knots]
    mean_abs = float(np.mean(np.abs(errors)))
    mean_signed = np.mean(errors, axis=0)
    assert mean_abs > _LIMITS["v2_constant_fit_mean_abs_lag_minimum"]
    assert np.all(mean_signed * slopes < 0.0)
    return {
        "v2_constant_fit_same_knot_mean_abs_lag_error": mean_abs,
        "v2_constant_fit_mean_signed_error_per_carrier": mean_signed.tolist(),
        "v2_constant_fit_mean_effective_lag_frames_per_carrier": (-mean_signed / slopes).tolist(),
    }


def _affine_truth_control() -> dict:
    frequencies, coefficients = _basis()
    clock = np.arange(4096)
    slopes = np.array([3.0e-5, -2.0e-5])
    truth = np.array([0.12, 0.30]) + (clock - _FIRST_ACTIVE)[:, None] * slopes
    truth[:_FIRST_ACTIVE] = 0.0
    source = _source(truth, frequencies, coefficients)
    fitted, report = fit_affine_causal_envelopes(source, frequencies, coefficients)
    constant, constant_report = fit_causal_envelopes(source, frequencies, coefficients)
    _check_envelopes(fitted, len(source), 2)
    _check_report(report, len(source), 2, len(source) - 1)
    assert report["rejected_tile_count"] == 0
    assert constant_report["rejected_tile_count"] == 0
    knots = np.arange(_FIRST_ACTIVE + 4 * _HOP, len(source), _HOP)
    assert np.all(knots - 255 >= _FIRST_ACTIVE)
    knot_error = _max_error(fitted[knots], truth[knots])
    assert knot_error < _LIMITS["physical_amplitude_max_abs_error"]
    return {
        "independent_physical_slopes_per_frame": slopes.tolist(),
        "fully_supported_knot_span_inclusive": [int(knots[0]), int(knots[-1])],
        "fully_supported_knot_count": len(knots),
        "affine_knot_max_abs_amplitude_error": knot_error,
        **_reported_affine_endpoints(report, truth, slopes),
        **_hold_error_report(fitted, truth, slopes, int(knots[0])),
        **_v2_lag_report(constant, truth, slopes, knots),
        "startup_zeros_or_finite_source_edges_claimed_affine": False,
    }


def _pair_prefix_control(
    source: Real, changed: Real, frequencies: Real, coefficients: Complex
) -> dict:
    first, first_report = fit_affine_causal_envelopes(source, frequencies, coefficients)
    second, second_report = fit_affine_causal_envelopes(changed, frequencies, coefficients)
    features = _features(len(source), frequencies, coefficients)
    first_modes = np.sum(first[:, :, None] * features, axis=1)
    second_modes = np.sum(second[:, :, None] * features, axis=1)
    first_remainder = source - first_modes
    second_remainder = changed - second_modes
    _check_envelopes(first, len(source), len(frequencies))
    _check_envelopes(second, len(source), len(frequencies))
    assert np.array_equal(source[:_EVENT], changed[:_EVENT])
    assert np.array_equal(first[:_EVENT], second[:_EVENT])
    assert np.array_equal(first_modes[:_EVENT], second_modes[:_EVENT])
    assert np.array_equal(first_remainder[:_EVENT], second_remainder[:_EVENT])
    envelope_effect = _max_error(first[_EVENT:], second[_EVENT:])
    mode_effect = _max_error(first_modes[_EVENT:], second_modes[_EVENT:])
    assert envelope_effect > _LIMITS["post_event_envelope_difference_minimum"]
    assert mode_effect > _LIMITS["post_event_source_modes_difference_minimum"]
    closure_error = max(
        _max_error(first_modes + first_remainder, source),
        _max_error(second_modes + second_remainder, changed),
    )
    assert closure_error < _LIMITS["complementary_sum_max_abs_error"]
    return {
        "first": first,
        "first_report": first_report,
        "second_report": second_report,
        "evidence": {
            "prefix_envelopes_bitwise_equal": True,
            "prefix_source_modes_bitwise_equal": True,
            "prefix_exact_subtraction_complements_bitwise_equal": True,
            "complement_definition": "original_source_pcm - synthesized_source_modes",
            "post_event_envelope_max_abs_difference": envelope_effect,
            "post_event_source_modes_max_abs_difference": mode_effect,
            "complementary_sum_max_abs_error": closure_error,
        },
    }


def _future_controls() -> dict:
    frequencies, coefficients = _basis()
    truth = np.zeros((4096, 2))
    truth[_FIRST_ACTIVE:3072] = [0.24, 0.16]
    source = _source(truth, frequencies, coefficients)
    changed = source.copy()
    changed[_EVENT] += [0.47, -0.31]
    assert (_EVENT - _FIRST_ACTIVE) % _HOP != 0
    same = _pair_prefix_control(source, changed, frequencies, coefficients)
    _check_report(same["first_report"], len(source), 2, 3071)
    _check_report(same["second_report"], len(source), 2, 3071)
    truth[800:] = 0.0
    shorter = _source(truth, frequencies, coefficients)
    extended_hull = shorter.copy()
    extended_hull[_EVENT] = [0.47, -0.31]
    different = _pair_prefix_control(shorter, extended_hull, frequencies, coefficients)
    _check_report(different["first_report"], len(source), 2, 799)
    _check_report(different["second_report"], len(source), 2, _EVENT)
    retained_decay = float(np.max(different["first"][800:_EVENT]))
    assert retained_decay > _LIMITS["post_event_envelope_difference_minimum"]
    return {
        "fixed_basis_shared_between_each_pair": True,
        "future_impulse_frame": _EVENT,
        "impulse_off_first_active_hop_grid": True,
        "same_last_active_hull": same["evidence"],
        "different_last_active_hulls": {
            "source_hulls_inclusive": [[_FIRST_ACTIVE, 799], [_FIRST_ACTIVE, _EVENT]],
            **different["evidence"],
            "post_activity_retained_envelope_maximum_before_future_event": retained_decay,
        },
        "future_hull_does_not_mask_retained_decay": True,
        "global_carrier_discovery_causality_claim": False,
    }


def _hann_gram() -> tuple[Real, Real, Real, Real]:
    """Analytic two-endpoint Gram for constant-energy circular stereo features."""
    offsets = np.arange(-255, 1, dtype=np.float64)
    fraction = (offsets + 255.0) / 255.0
    hann = 0.5 - 0.5 * np.cos(2.0 * np.pi * np.arange(_WINDOW) / 255.0)
    zeroth = float(np.sum(hann))
    first = float(np.sum(hann * fraction))
    second = float(np.sum(hann * fraction**2))
    # Re(C*exp(i*phase)) has stereo energy exactly 1/2 at every frame.
    gram = 0.5 * np.array([
        [zeroth - 2.0 * first + second, first - second],
        [first - second, second],
    ])
    return gram, hann, fraction, offsets


def _detail_at(report: dict, center: int) -> dict:
    details = [
        detail for detail in report["first_knot_details"] if detail["center_frame"] == center
    ]
    assert len(details) == 1
    return details[0]


def _analytic_leverage_expectations() -> tuple[Real, float, float, dict]:
    gram, hann, fraction, offsets = _hann_gram()
    inverse = np.linalg.inv(gram)
    eigenvalues = np.linalg.eigvalsh(gram)
    expected_condition = float(np.sqrt(eigenvalues[-1] / eigenvalues[0]))
    expected_leverage = float(np.sqrt(inverse[1, 1]))
    kernel = 0.5 * hann * ((1.0 - fraction) * inverse[0, 1] + fraction * inverse[1, 1])
    kernel_sum_error = abs(float(np.sum(kernel)) - 1.0)
    kernel_moment_error = abs(float(np.dot(kernel, offsets)))
    kernel_minimum = float(np.min(kernel))
    assert kernel_sum_error < _LIMITS["kernel_sum_max_abs_error"]
    assert kernel_moment_error < _LIMITS["kernel_first_moment_max_abs_error_frames"]
    assert kernel_minimum < _LIMITS["kernel_negative_lobe_required_below"]
    return (
        gram,
        expected_condition,
        expected_leverage,
        {
            "independent_2_by_2_hann_gram": gram.tolist(),
            "circular_stereo_feature_energy": 0.5,
            "actual_two_column_condition": expected_condition,
            "unconstrained_weighted_target_pseudoinverse_right_row_norm": expected_leverage,
            "scalar_amplitude_kernel_sum_max_abs_error": kernel_sum_error,
            "scalar_amplitude_kernel_endpoint_first_moment_abs_error_frames": kernel_moment_error,
            "scalar_amplitude_kernel_minimum": kernel_minimum,
            "negative_lobes_required_for_linear_endpoint_identity": True,
        },
    )


def _boundary_active_set_control(
    frequencies: Real, coefficients: Complex, gram: Real, expected_leverage: float
) -> dict:
    # Signed affine source gives the independent unconstrained solution [-0.1, 0.3].
    # Its known NNLS optimum has L=0 and R=(G_RL*(-0.1)+G_RR*0.3)/G_RR.
    clock = np.arange(1024)
    boundary_truth = (-0.1 + 0.4 * (clock - 130) / 255.0)[:, None]
    boundary_truth[:_FIRST_ACTIVE] = 0.0
    constrained, constrained_report = fit_affine_causal_envelopes(
        _source(boundary_truth, frequencies, coefficients), frequencies, coefficients
    )
    constrained_detail = _detail_at(constrained_report, 385)
    expected_right = float(0.3 - 0.1 * gram[0, 1] / gram[1, 1])
    boundary_error = abs(float(constrained[385, 0]) - expected_right)
    assert constrained_detail["status"] == "converged"
    assert constrained_detail["left_endpoints"] == [0.0]
    assert boundary_error < _LIMITS["physical_amplitude_max_abs_error"]
    assert (
        abs(constrained_detail["current_right_endpoints"][0] - expected_right)
        < _LIMITS["physical_amplitude_max_abs_error"]
    )
    active_set_sensitivity = float(1.0 / np.sqrt(gram[1, 1]))
    norm_gap = expected_leverage - active_set_sensitivity
    assert norm_gap > _LIMITS["unconstrained_vs_active_norm_minimum_gap"]
    assert (
        abs(constrained_detail["unconstrained_current_endpoint_leverage"][0] - expected_leverage)
        < _LIMITS["analytic_leverage_max_abs_error"]
    )
    assert (
        constrained_report["unconstrained_current_endpoint_leverage_summary"][
            "nnls_active_set_sensitivity_claim"
        ]
        is False
    )
    return {
        "known_nnls_active_left_endpoint": 0.0,
        "known_nnls_right_endpoint": expected_right,
        "known_nnls_right_endpoint_abs_error": boundary_error,
        "fixed_active_set_weighted_target_right_row_norm": active_set_sensitivity,
        "unconstrained_minus_active_set_sensitivity_norm": norm_gap,
        "reported_unconstrained_leverage_is_not_nnls_active_set_sensitivity": True,
    }


def _leverage_and_active_set_control() -> dict:
    frequencies = np.array([683.17])
    coefficients = np.array([[1.0 / np.sqrt(2.0), -1.0j / np.sqrt(2.0)]])
    gram, condition, leverage, evidence = _analytic_leverage_expectations()
    truth = np.zeros((1024, 1))
    truth[_FIRST_ACTIVE:] = 0.24
    source = _source(truth, frequencies, coefficients)
    _, report = fit_affine_causal_envelopes(source, frequencies, coefficients)
    _check_report(report, len(source), 1, len(source) - 1)
    detail = _detail_at(report, 385)
    condition_error = abs(detail["condition_number"] - condition)
    leverage_error = abs(detail["unconstrained_current_endpoint_leverage"][0] - leverage)
    assert condition_error < _LIMITS["analytic_condition_max_abs_error"]
    assert leverage_error < _LIMITS["analytic_leverage_max_abs_error"]
    return {
        **evidence,
        "checked_full_window_center": 385,
        "reported_condition_max_abs_error": condition_error,
        "reported_leverage_max_abs_error": leverage_error,
        **_boundary_active_set_control(frequencies, coefficients, gram, leverage),
    }


def _conditioning_controls() -> dict:
    _, original_coefficients = _basis()
    coefficients = original_coefficients[[0, 0]]
    results = {}
    for label, frequencies in (
        ("duplicate", np.array([421.73, 421.73])),
        ("near_duplicate", np.array([421.73, 421.73000001])),
    ):
        truth = np.zeros((1024, 2))
        truth[_FIRST_ACTIVE:] = [0.24, 0.16]
        source = _source(truth, frequencies, coefficients)
        fitted, report = fit_affine_causal_envelopes(source, frequencies, coefficients)
        _check_report(report, len(source), 2, len(source) - 1)
        assert report["tile_count"] > 0
        assert report["rejected_tile_count"] == report["tile_count"]
        assert set(report["tile_status_counts"]) == {"ill_conditioned"}
        assert np.all(fitted == 0.0)
        center_condition = _detail_at(report, 385)["condition_number"]
        assert center_condition is None or center_condition > _LIMITS["condition_limit"]
        modes = np.sum(
            fitted[:, :, None] * _features(len(source), frequencies, coefficients), axis=1
        )
        assert np.array_equal(source - modes, source)
        results[label] = {
            "frequencies_hz": frequencies.tolist(),
            "requested_carrier_count_retained": report["carrier_count"],
            "actual_design_column_count": report["design_column_count"],
            "full_window_condition_at_frame_385": center_condition,
            "refused_tile_count": report["rejected_tile_count"],
            "zero_modes_leave_original_source_in_exact_complement": True,
        }
    return results


def integrity_controls() -> dict:
    """Return independent, bounded controls; assert each predeclared limit."""
    frequencies, coefficients = _basis()
    silence, report = fit_affine_causal_envelopes(np.zeros((512, 2)), frequencies, coefficients)
    assert np.all(silence == 0.0)
    assert report["tile_count"] == 0
    assert report["source_activity_hull_inclusive"] is None
    return {
        "stage": "affine_causal_mode_envelope_integrity_controls",
        "predeclared_numeric_acceptance_limits": dict(_LIMITS),
        "constant_plateau": _constant_plateau_control(),
        "independent_affine_amplitudes": _affine_truth_control(),
        "fixed_basis_local_causality": _future_controls(),
        "independent_analytic_leverage_and_nnls_boundary": _leverage_and_active_set_control(),
        "conditioning_refusals": _conditioning_controls(),
        "silence_exact_zero_without_fitting": True,
        "truth_amplitudes_only_in_control_source_generator_and_expectations": True,
        "private_estimator_helpers_or_fixture_output_targets_used": False,
        "finite_offline_pcm_controls_only": True,
        "global_carrier_discovery_or_complete_pipeline_causality_claim": False,
        "realtime_device_or_human_listening_acceptance_claim": False,
    }
