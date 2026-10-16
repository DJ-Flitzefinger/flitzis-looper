"""Independent physical controls for current-inclusive causal affine predictions.

Only generated stereo PCM and fixed carrier bases reach the public estimators.
The independently derived unconstrained norms describe linear maps before NNLS
or prediction projection changes active constraints, and make no listening claim.
"""

from typing import TYPE_CHECKING

import numpy as np
from pitch_mode_envelopes import (
    fit_current_weight_causal_envelopes,
    fit_predictive_causal_envelopes,
)

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]

# Frozen before execution: geometry, positive physical cue and numerical limits.
_FS, _WINDOW, _HOP, _FIRST_ACTIVE, _EVENT = 48_000, 256, 64, 129, 1024
_CENTER, _PLATEAU, _CUE = 385, 0.24, 0.001
_LIMITS = {
    "physical_amplitude_max_abs_error": 2.0e-10,
    "slope_max_abs_error_per_frame": 2.0e-12,
    "analytic_prediction_norm_max_abs_error": 2.0e-10,
    "kernel_moment_max_abs_error": 2.0e-10,
    "post_event_difference_minimum": 1.0e-8,
    "complementary_sum_max_abs_error": 2.0e-15,
    "condition_limit": 1.0e6,
    "maximum_passive_solves_per_tile": 128,
}


def _basis(*, alternative: bool = False) -> tuple[Real, Complex]:
    frequencies = np.array([937.61, 2713.47] if alternative else [683.17, 1901.29])
    coefficients = np.array([
        [np.sqrt(0.7) * np.exp(0.31j), np.sqrt(0.3) * np.exp(-0.27j)],
        [np.sqrt(0.2) * np.exp(1.13j), np.sqrt(0.8) * np.exp(-0.54j)],
    ])
    if alternative:
        coefficients = coefficients[:, ::-1].conjugate()
    return frequencies, coefficients / np.linalg.norm(coefficients, axis=1)[:, None]


def _features(frames: int, frequencies: Real, coefficients: Complex) -> Real:
    carriers = np.exp(2.0j * np.pi * np.outer(np.arange(frames), frequencies) / _FS)
    return (carriers[:, :, None] * coefficients[None, :, :]).real


def _source(truth: Real, frequencies: Real, coefficients: Complex) -> Real:
    return np.sum(truth[:, :, None] * _features(len(truth), frequencies, coefficients), axis=1)


def _error(actual: Real, expected: Real) -> float:
    return float(np.max(np.abs(actual - expected)))


def _detail(report: dict, center: int = _CENTER) -> dict:
    return next(item for item in report["first_knot_details"] if item["center_frame"] == center)


def _check(envelopes: Real, report: dict, frames: int, carriers: int, last: int) -> None:
    assert envelopes.shape == (frames, carriers)
    assert np.isfinite(envelopes).all()
    assert np.all(envelopes >= 0.0)
    assert np.all(envelopes[:_FIRST_ACTIVE] == 0.0)
    assert report["carrier_count"] == carriers
    assert report["design_column_count"] == 2 * carriers
    assert report["window_frames"] == _WINDOW
    assert report["hop_frames"] == _HOP
    assert report["window_offsets_inclusive"] == [-255, 0]
    assert report["window"] == ("trailing rising half-cosine; sqrt weights in source least squares")
    assert report["window_weight_formula"] == "0.5*(1-cos(pi*m/255)), m=0..255"
    assert report["current_source_weight"] == 1.0
    assert report["past_start_source_weight"] == 0.0
    assert report["source_lookahead_frames"] == 0
    assert report["source_lookback_frames"] == 255
    assert report["source_activity_hull_inclusive"] == [_FIRST_ACTIVE, last]
    assert report["fixed_mode_birth_frames"] == [_FIRST_ACTIVE] * carriers
    assert report["causal_processing_span_inclusive"] == [_FIRST_ACTIVE, frames - 1]
    assert report["last_active_source_frame_used_as_mask_or_knot"] is False
    assert report["fitted_decay_retained_through_finite_input"] is True
    assert report["prediction_age_offsets_inclusive"] == [0, 63]
    assert report["prediction_nonnegative_projection"] is True
    assert report["prediction_slope_divisor_frames"] == 255
    assert report["envelope_interpolation"] == (
        "nonnegative causal linear extrapolation from current R and fitted slope"
    )
    assert report["condition_limit"] == _LIMITS["condition_limit"]
    assert report["maximum_passive_solves_per_tile"] == _LIMITS["maximum_passive_solves_per_tile"]
    assert report["maximum_active_set_solves"] <= _LIMITS["maximum_passive_solves_per_tile"]
    knots = list(range(_FIRST_ACTIVE, frames, _HOP))
    assert report["tile_count"] == len(knots)
    assert [item["center_frame"] for item in report["first_knot_details"]] == knots[:16]
    assert report["omitted_knot_detail_count"] == max(0, len(knots) - 16)
    _check_norm_reports(report, len(knots) * carriers)


def _check_norm_reports(report: dict, count: int) -> None:
    for domain in ("weighted_target", "pcm"):
        summary = report[f"unconstrained_prediction_{domain}_row_norm_summary"]
        assert summary["nnls_active_set_sensitivity_claim"] is False
        for age in ("age_0", "age_63"):
            assert summary[age]["finite_count"] + summary[age]["unavailable_count"] == count
            for detail in report["first_knot_details"]:
                norms = detail[f"unconstrained_prediction_{domain}_row_norms"][age]
                if norms is not None:
                    assert len(norms) == report["carrier_count"]
                    assert np.isfinite(norms).all()
                    assert np.all(np.asarray(norms) >= 0.0)
    weighted = report["unconstrained_prediction_weighted_target_row_norm_summary"]
    pcm = report["unconstrained_prediction_pcm_row_norm_summary"]
    assert weighted["actual_pcm_noise_sensitivity_claim"] is False
    assert pcm["projection_active_set_sensitivity_claim"] is False
    assert pcm["unconstrained_linear_pcm_sensitivity"] is True


def _decomposition(source: Real, frequencies: Real, coefficients: Complex, last: int) -> dict:
    envelopes, report = fit_current_weight_causal_envelopes(source, frequencies, coefficients)
    _check(envelopes, report, len(source), len(frequencies), last)
    return {"envelopes": envelopes, **_complement(source, envelopes, frequencies, coefficients)}


def _complement(source: Real, envelopes: Real, frequencies: Real, coefficients: Complex) -> dict:
    modes = _source(envelopes, frequencies, coefficients)
    remainder = source - modes
    closure = _error(modes + remainder, source)
    assert closure < _LIMITS["complementary_sum_max_abs_error"]
    return {"modes": modes, "remainder": remainder, "closure": closure}


def _endpoint_errors(report: dict, truth: Real, slopes: Real) -> dict:
    endpoint_error, slope_error = 0.0, 0.0
    for detail in report["first_knot_details"][4:]:
        center = detail["center_frame"]
        endpoint_error = max(
            endpoint_error,
            _error(np.asarray(detail["left_endpoints"]), truth[center - 255]),
            _error(np.asarray(detail["current_right_endpoints"]), truth[center]),
        )
        slope_error = max(slope_error, _error(np.asarray(detail["slopes_per_frame"]), slopes))
    assert endpoint_error < _LIMITS["physical_amplitude_max_abs_error"]
    assert slope_error < _LIMITS["slope_max_abs_error_per_frame"]
    return {"endpoint_max_abs_error": endpoint_error, "slope_max_abs_error": slope_error}


def _affine_controls() -> list[dict]:
    results = []
    for alternative in (False, True):
        frequencies, coefficients = _basis(alternative=alternative)
        slopes = np.array([-4.0e-5, 2.5e-5] if alternative else [3.0e-5, -2.0e-5])
        initial = np.array([0.28, 0.10] if alternative else [0.12, 0.30])
        truth = initial + (np.arange(2049) - _FIRST_ACTIVE)[:, None] * slopes
        truth[:_FIRST_ACTIVE] = 0.0
        source = _source(truth, frequencies, coefficients)
        predicted, report = fit_current_weight_causal_envelopes(source, frequencies, coefficients)
        _check(predicted, report, len(source), 2, len(source) - 1)
        assert report["rejected_tile_count"] == 0
        interior = slice(_CENTER, len(source))
        assert (interior.stop - interior.start) % _HOP == 0
        measured = _error(predicted[interior], truth[interior])
        assert measured < _LIMITS["physical_amplitude_max_abs_error"]
        results.append({
            "frequencies_hz": frequencies.tolist(),
            "complex_stereo_vectors": {
                "real": coefficients.real.tolist(),
                "imaginary": coefficients.imag.tolist(),
            },
            "physical_slopes_per_frame": slopes.tolist(),
            "whole_supported_hops_inclusive": [interior.start, interior.stop - 1],
            "whole_hop_max_abs_amplitude_error": measured,
            **_endpoint_errors(report, truth, slopes),
            "finite_final_frame_is_age_63_without_extra_end_knot": True,
            "complementary_sum_max_abs_error": _complement(
                source, predicted, frequencies, coefficients
            )["closure"],
        })
    return results


def _projection_control() -> dict:
    frequencies, coefficients = _basis()
    frequencies, coefficients = frequencies[:1], coefficients[:1]
    crossing, magnitude = 408.5, 4.0e-4
    truth = np.maximum(0.0, magnitude * (crossing - np.arange(768)))[:, None]
    truth[:_FIRST_ACTIVE] = 0.0
    source = _source(truth, frequencies, coefficients)
    predicted, report = fit_current_weight_causal_envelopes(source, frequencies, coefficients)
    _check(predicted, report, len(source), 1, 408)
    expected = np.maximum(0.0, magnitude * (crossing - np.arange(_CENTER, _CENTER + _HOP)))
    measured = _error(predicted[_CENTER : _CENTER + _HOP, 0], expected)
    assert measured < _LIMITS["physical_amplitude_max_abs_error"]
    assert np.all(truth[_CENTER - 255 : _CENTER + 1] > 0.0)
    assert np.all(expected[:24] > 0.0)
    assert np.all(predicted[409 : _CENTER + _HOP] == 0.0)
    return {
        "checked_supported_hop_inclusive": [_CENTER, _CENTER + _HOP - 1],
        "analytic_zero_crossing_frame": crossing,
        "only_pre_next_fit_hop_compared_to_projected_affine_truth": True,
        "projected_hop_max_abs_amplitude_error": measured,
        "negative_predictions_projected_to_exact_zero": True,
        "complementary_sum_max_abs_error": _complement(
            source, predicted, frequencies, coefficients
        )["closure"],
    }


def _analytic_system() -> dict:
    fraction = np.arange(_WINDOW, dtype=np.float64) / 255.0
    weights = 0.5 * (1.0 - np.cos(np.pi * fraction))
    endpoints = np.column_stack((1.0 - fraction, fraction))
    moments = np.array([np.sum(weights * fraction**power) for power in range(3)])
    gram = 0.5 * np.array([
        [moments[0] - 2.0 * moments[1] + moments[2], moments[1] - moments[2]],
        [moments[1] - moments[2], moments[2]],
    ])
    gram_error = _error(gram, 0.5 * endpoints.T @ (weights[:, None] * endpoints))
    assert gram_error < _LIMITS["kernel_moment_max_abs_error"]
    squared_gram = 0.5 * endpoints.T @ (weights[:, None] ** 2 * endpoints)
    return {
        "weights": weights,
        "endpoints": endpoints,
        "gram": gram,
        "squared_gram": squared_gram,
        "inverse": np.linalg.inv(gram),
        "direct_weighted_fraction_moments_0_1_2": moments.tolist(),
        "direct_moment_gram_max_abs_error": gram_error,
        "offsets": np.arange(-255, 1, dtype=np.float64),
    }


def _norm_and_moment_controls(report: dict, system: dict) -> dict:
    results = {}
    detail = _detail(report)
    for age in (0, 63):
        row = np.array([-age / 255.0, 1.0 + age / 255.0])
        transform = row @ system["inverse"]
        expected = {
            "weighted_target": float(np.sqrt(transform @ row)),
            "pcm": float(np.sqrt(transform @ system["squared_gram"] @ transform)),
        }
        kernel = 0.5 * system["weights"] * (system["endpoints"] @ transform)
        errors = {
            "kernel_sum": abs(float(np.sum(kernel)) - 1.0),
            "kernel_first_past_moment": abs(float(kernel @ system["offsets"]) - age),
        }
        assert max(errors.values()) < _LIMITS["kernel_moment_max_abs_error"]
        for domain in ("weighted_target", "pcm"):
            measured = detail[f"unconstrained_prediction_{domain}_row_norms"][f"age_{age}"][0]
            errors[f"{domain}_norm"] = abs(measured - expected[domain])
            assert errors[f"{domain}_norm"] < _LIMITS["analytic_prediction_norm_max_abs_error"]
        assert expected["weighted_target"] > expected["pcm"]
        results[f"age_{age}"] = {"analytic_row_norms": expected, "absolute_errors": errors}
    return {
        "independent_gram": system["gram"].tolist(),
        "independent_squared_weight_gram": system["squared_gram"].tolist(),
        "direct_weighted_fraction_moments_0_1_2": system["direct_weighted_fraction_moments_0_1_2"],
        "direct_moment_gram_max_abs_error": system["direct_moment_gram_max_abs_error"],
        "weighted_target_norm_formula": "sqrt(h @ inverse(G) @ h.T)",
        "pcm_norm_formula": "sqrt(h @ inverse(G) @ G2 @ inverse(G) @ h.T)",
        "pcm_map_definition": "A^+ @ diag(sqrt(w)); fixed-basis unconstrained linear map",
        "distinct_domains_are_not_nnls_or_projection_active_set_gain": True,
        "kernel_sum_expected": 1.0,
        "kernel_first_past_moment_expected": "d at prediction age d",
        **results,
    }


def _signed_cue_evidence(
    source: Real,
    frequencies: Real,
    coefficients: Complex,
    baseline: dict,
    expected: dict,
    *,
    sign: float,
) -> dict:
    changed = source.copy()
    changed[_CENTER] += sign * _CUE * _features(len(source), frequencies, coefficients)[_CENTER, 0]
    fitted, report = fit_current_weight_causal_envelopes(changed, frequencies, coefficients)
    old, _ = fit_predictive_causal_envelopes(changed, frequencies, coefficients)
    detail = _detail(report)
    assert detail["status"] == "converged"
    endpoints = np.array([detail["left_endpoints"][0], detail["current_right_endpoints"][0]])
    assert np.all(endpoints > 0.0)
    assert np.all(_PLATEAU + sign * expected["prediction"] > 0.0)
    assert np.all(fitted[_CENTER : _CENTER + _HOP] > 0.0)
    errors = {
        "endpoint": _error(endpoints, _PLATEAU + sign * expected["endpoints"]),
        "whole_hop": _error(
            fitted[_CENTER : _CENTER + _HOP, 0] - baseline["new"][_CENTER : _CENTER + _HOP, 0],
            sign * expected["prediction"],
        ),
    }
    assert max(errors.values()) < _LIMITS["physical_amplitude_max_abs_error"]
    assert np.array_equal(old[_CENTER : _CENTER + _HOP], baseline["old"][_CENTER : _CENTER + _HOP])
    assert np.array_equal(changed[:_CENTER], source[:_CENTER])
    assert np.array_equal(changed[_CENTER + 1 :], source[_CENTER + 1 :])
    return {
        "sign": sign,
        "max_abs_errors": errors,
        "minimum_positive_endpoint_and_prediction": float(
            min(np.min(endpoints), np.min(fitted[_CENTER : _CENTER + _HOP]))
        ),
    }


def _current_cue_control(
    source: Real, frequencies: Real, coefficients: Complex, system: dict
) -> dict:
    baseline = {
        "new": fit_current_weight_causal_envelopes(source, frequencies, coefficients)[0],
        "old": fit_predictive_causal_envelopes(source, frequencies, coefficients)[0],
    }
    delta = 0.5 * _CUE * system["weights"][-1] * system["inverse"][:, 1]
    ages = np.arange(_HOP, dtype=np.float64) / 255.0
    expected = {"endpoints": delta, "prediction": np.column_stack((-ages, 1.0 + ages)) @ delta}
    assert np.all(expected["prediction"] > 0.0)
    return {
        "physical_current_frame_cue": "+/-0.001 * real(C * exp(i*phase(385))); only frame 385",
        "independent_endpoint_delta_formula": "0.5*epsilon*w_last*inverse(G) @ e_R",
        "independent_endpoint_delta": delta.tolist(),
        "whole_hop_inclusive": [_CENTER, _CENTER + _HOP - 1],
        "signed_cue_evidence": [
            _signed_cue_evidence(source, frequencies, coefficients, baseline, expected, sign=sign)
            for sign in (-1.0, 1.0)
        ],
        "nnls_and_projection_constraints_inactive_for_checked_hop": True,
        "v4_hann_current_cue_immediate_whole_hop_response_bitwise_zero": True,
        "later_v4_knots_may_respond": True,
    }


def _analytic_controls() -> dict:
    frequencies = np.array([823.19])
    coefficients = np.array([[1.0, -1.0j]]) / np.sqrt(2.0)
    truth = np.zeros((1024, 1))
    truth[_FIRST_ACTIVE:] = _PLATEAU
    source = _source(truth, frequencies, coefficients)
    fitted, report = fit_current_weight_causal_envelopes(source, frequencies, coefficients)
    _check(fitted, report, len(source), 1, len(source) - 1)
    assert report["rejected_tile_count"] == 0
    system = _analytic_system()
    return {
        "circular_stereo_feature_energy": 0.5,
        "checked_full_window_center": _CENTER,
        "norms_and_kernel_moments": _norm_and_moment_controls(report, system),
        "physical_current_frame_cue": _current_cue_control(
            source, frequencies, coefficients, system
        ),
    }


def _future_controls() -> dict:
    frequencies, coefficients = _basis()
    results = {}
    assert (_EVENT - _FIRST_ACTIVE) % _HOP != 0
    for label, end in (("same_last_active_hull", 1536), ("different_last_active_hulls", 800)):
        truth = np.zeros((2049, 2))
        truth[_FIRST_ACTIVE:end] = [0.24, 0.16]
        source = _source(truth, frequencies, coefficients)
        changed = source.copy()
        changed[_EVENT] += [0.47, -0.31]
        first = _decomposition(source, frequencies, coefficients, end - 1)
        second = _decomposition(changed, frequencies, coefficients, max(end - 1, _EVENT))
        assert np.array_equal(source[:_EVENT], changed[:_EVENT])
        for key in ("envelopes", "modes", "remainder"):
            assert np.array_equal(first[key][:_EVENT], second[key][:_EVENT])
        effect = {
            key: _error(first[key][_EVENT:], second[key][_EVENT:]) for key in ("envelopes", "modes")
        }
        assert min(effect.values()) > _LIMITS["post_event_difference_minimum"]
        retained = float(np.max(first["envelopes"][end:_EVENT])) if end < _EVENT else None
        if retained is not None:
            assert retained > _LIMITS["post_event_difference_minimum"]
        results[label] = {
            "source_hulls_inclusive": [
                [_FIRST_ACTIVE, end - 1],
                [_FIRST_ACTIVE, max(end - 1, _EVENT)],
            ],
            "prefix_envelopes_modes_and_exact_subtraction_complements_bitwise_equal": True,
            "post_event_max_abs_differences": effect,
            "retained_post_activity_envelope_maximum_before_future_event": retained,
            "complementary_sum_max_abs_error": max(first["closure"], second["closure"]),
        }
    return {"future_impulse_frame": _EVENT, "impulse_off_hop_grid": True, **results}


def _refusal_and_empty_controls() -> dict:
    _, basis = _basis()
    coefficients = basis[[0, 0]]
    results = {}
    for label, frequencies in (
        ("duplicate", np.array([421.73, 421.73])),
        ("near_duplicate", np.array([421.73, 421.73000001])),
    ):
        truth = np.zeros((1024, 2))
        truth[_FIRST_ACTIVE:] = [0.24, 0.16]
        source = _source(truth, frequencies, coefficients)
        fitted, report = fit_current_weight_causal_envelopes(source, frequencies, coefficients)
        _check(fitted, report, len(source), 2, len(source) - 1)
        assert report["rejected_tile_count"] == report["tile_count"] > 0
        assert set(report["tile_status_counts"]) == {"ill_conditioned"}
        assert (
            _detail(report)["condition_number"] is None
            or _detail(report)["condition_number"] > _LIMITS["condition_limit"]
        )
        assert np.all(fitted == 0.0)
        assert np.array_equal(source - _source(fitted, frequencies, coefficients), source)
        results[label] = {"refused_tile_count": report["rejected_tile_count"]}
    silence, report = fit_current_weight_causal_envelopes(
        np.zeros((512, 2)), frequencies, coefficients
    )
    assert np.all(silence == 0.0)
    assert report["tile_count"] == 0
    assert report["source_activity_hull_inclusive"] is None
    empty_frequencies, empty_coefficients = np.zeros(0), np.zeros((0, 2), dtype=np.complex128)
    empty, empty_report = fit_current_weight_causal_envelopes(
        source, empty_frequencies, empty_coefficients
    )
    assert empty.shape == (len(source), 0)
    assert empty_report["tile_count"] == 0
    assert np.array_equal(source - _source(empty, empty_frequencies, empty_coefficients), source)
    return {
        **results,
        "refused_modes_leave_original_source_in_exact_complement": True,
        "silence_exact_zero_without_fitting": True,
        "zero_carriers_n_by_zero_with_exact_source_complement": True,
    }


def integrity_controls() -> dict:
    """Assert frozen physical, current-cue, analytic-norm and causal controls."""
    return {
        "stage": "current_weight_causal_mode_envelope_integrity_controls",
        "predeclared_numeric_acceptance_limits": dict(_LIMITS),
        "independent_affine_amplitudes_and_bases": _affine_controls(),
        "within_hop_nonnegative_projection": _projection_control(),
        "independent_analytic_norms_and_physical_current_frame_cue": _analytic_controls(),
        "fixed_basis_local_causality_and_complete_source_decomposition": _future_controls(),
        "conditioning_refusals_silence_and_zero_carriers": _refusal_and_empty_controls(),
        "complement_definition": "original_source_pcm - synthesized_source_modes",
        "truth_only_in_control_source_generation_and_analytic_expectations": True,
        "private_estimator_helpers_or_output_targets_used": False,
        "global_discovery_complete_pipeline_realtime_or_listening_acceptance_claim": False,
    }
