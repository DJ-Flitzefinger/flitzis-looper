"""Independent physical controls for projected causal affine predictions.

Only generated stereo PCM and a fixed carrier basis reach the public estimator.
Known amplitudes supply independent expectations, never estimator-side targets.
These finite offline controls make no device, listening or complete-pipeline claim.
"""

from typing import TYPE_CHECKING

import numpy as np
from pitch_mode_envelopes import fit_affine_causal_envelopes, fit_predictive_causal_envelopes

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]

# Geometry and precision limits are frozen before any measurements are collected.
_FS, _WINDOW, _HOP, _FIRST_ACTIVE, _EVENT = 48_000, 256, 64, 129, 1024
_LIMITS = {
    "physical_amplitude_max_abs_error": 2.0e-10,
    "slope_max_abs_error_per_frame": 2.0e-12,
    "analytic_prediction_norm_max_abs_error": 2.0e-10,
    "post_event_envelope_difference_minimum": 1.0e-8,
    "post_event_source_modes_difference_minimum": 1.0e-8,
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
    oscillators = np.exp(2.0j * np.pi * np.outer(np.arange(frames), frequencies) / _FS)
    return (oscillators[:, :, None] * coefficients[None, :, :]).real


def _source(amplitudes: Real, frequencies: Real, coefficients: Complex) -> Real:
    return np.sum(
        amplitudes[:, :, None] * _features(len(amplitudes), frequencies, coefficients), axis=1
    )


def _error(actual: Real, expected: Real) -> float:
    return float(np.max(np.abs(actual - expected)))


def _decompose(
    source: Real, envelopes: Real, frequencies: Real, coefficients: Complex
) -> tuple[Real, Real, float]:
    modes = _source(envelopes, frequencies, coefficients)
    remainder = source - modes
    closure = _error(modes + remainder, source)
    assert closure < _LIMITS["complementary_sum_max_abs_error"]
    return modes, remainder, closure


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
    summary = report["unconstrained_prediction_weighted_target_row_norm_summary"]
    assert summary["nnls_active_set_sensitivity_claim"] is False
    assert summary["actual_pcm_noise_sensitivity_claim"] is False
    for age in ("age_0", "age_63"):
        counts = summary[age]
        assert counts["finite_count"] + counts["unavailable_count"] == len(knots) * carriers
        for detail in report["first_knot_details"]:
            norms = detail["unconstrained_prediction_weighted_target_row_norms"][age]
            if norms is not None:
                assert len(norms) == carriers
                assert np.isfinite(norms).all()
                assert np.all(np.asarray(norms) >= 0.0)


def _endpoint_errors(report: dict, truth: Real, slopes: Real) -> dict:
    slope_error, endpoint_error = 0.0, 0.0
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
    return {
        "first_full_window_detail_max_abs_endpoint_error": endpoint_error,
        "first_full_window_detail_max_abs_slope_error_per_frame": slope_error,
    }


def _affine_controls() -> list[dict]:
    results = []
    for alternative in (False, True):
        frequencies, coefficients = _basis(alternative=alternative)
        slopes = np.array([-4.0e-5, 2.5e-5] if alternative else [3.0e-5, -2.0e-5])
        initial = np.array([0.28, 0.10] if alternative else [0.12, 0.30])
        truth = initial + (np.arange(2049) - _FIRST_ACTIVE)[:, None] * slopes
        truth[:_FIRST_ACTIVE] = 0.0
        source = _source(truth, frequencies, coefficients)
        predicted, report = fit_predictive_causal_envelopes(source, frequencies, coefficients)
        held, _ = fit_affine_causal_envelopes(source, frequencies, coefficients)
        _check(predicted, report, len(source), 2, len(source) - 1)
        assert report["rejected_tile_count"] == 0
        knots = np.arange(_FIRST_ACTIVE, len(source), _HOP)
        assert np.array_equal(predicted[knots], held[knots])
        interior = slice(_FIRST_ACTIVE + 4 * _HOP, len(source))
        assert (interior.stop - interior.start) % _HOP == 0
        amplitude_error = _error(predicted[interior], truth[interior])
        assert amplitude_error < _LIMITS["physical_amplitude_max_abs_error"]
        _, _, closure = _decompose(source, predicted, frequencies, coefficients)
        results.append({
            "frequencies_hz": frequencies.tolist(),
            "complex_stereo_vectors": {
                "real": coefficients.real.tolist(),
                "imaginary": coefficients.imag.tolist(),
            },
            "physical_slopes_per_frame": slopes.tolist(),
            "whole_fully_supported_hop_span_inclusive": [interior.start, interior.stop - 1],
            "whole_hop_max_abs_amplitude_error": amplitude_error,
            **_endpoint_errors(report, truth, slopes),
            "all_regular_knots_bitwise_equal_to_public_affine_left_hold": True,
            "finite_final_frame_is_age_63_without_extra_end_knot": True,
            "complementary_sum_max_abs_error": closure,
        })
    return results


def _projection_control() -> dict:
    frequencies, coefficients = _basis()
    frequencies, coefficients = frequencies[:1], coefficients[:1]
    center, crossing, magnitude = 385, 408.5, 4.0e-4
    truth = np.maximum(0.0, magnitude * (crossing - np.arange(768)))[:, None]
    truth[:_FIRST_ACTIVE] = 0.0
    source = _source(truth, frequencies, coefficients)
    predicted, report = fit_predictive_causal_envelopes(source, frequencies, coefficients)
    _check(predicted, report, len(source), 1, 408)
    hop = slice(center, center + _HOP)
    expected = np.maximum(0.0, magnitude * (crossing - np.arange(hop.start, hop.stop)))
    measured = _error(predicted[hop, 0], expected)
    assert measured < _LIMITS["physical_amplitude_max_abs_error"]
    assert np.all(expected[:24] > 0.0)
    assert np.all(predicted[409 : hop.stop] == 0.0)
    assert np.all(truth[center - 255 : center + 1] > 0.0)
    _, _, closure = _decompose(source, predicted, frequencies, coefficients)
    return {
        "checked_supported_hop_inclusive": [center, hop.stop - 1],
        "analytic_zero_crossing_frame": crossing,
        "positive_fit_window_precedes_source_clipping": True,
        "only_pre_next_fit_hop_compared_to_projected_affine_truth": True,
        "projected_hop_max_abs_amplitude_error": measured,
        "negative_predictions_projected_to_exact_zero": True,
        "complementary_sum_max_abs_error": closure,
    }


def _analytic_norm_control() -> dict:
    frequencies = np.array([823.19])
    coefficients = np.array([[1.0 / np.sqrt(2.0), -1.0j / np.sqrt(2.0)]])
    truth = np.zeros((1024, 1))
    truth[_FIRST_ACTIVE:] = 0.24
    predicted, report = fit_predictive_causal_envelopes(
        _source(truth, frequencies, coefficients), frequencies, coefficients
    )
    _check(predicted, report, len(truth), 1, len(truth) - 1)
    fraction = np.arange(_WINDOW, dtype=np.float64) / 255.0
    hann = 0.5 - 0.5 * np.cos(2.0 * np.pi * fraction)
    endpoints = np.column_stack((1.0 - fraction, fraction))
    gram = 0.5 * endpoints.T @ (hann[:, None] * endpoints)
    inverse = np.linalg.inv(gram)
    detail = next(item for item in report["first_knot_details"] if item["center_frame"] == 385)
    expected, errors = {}, {}
    for age in (0, 63):
        row = np.array([-age / 255.0, 1.0 + age / 255.0])
        expected[f"age_{age}"] = float(np.sqrt(row @ inverse @ row))
        measured = detail["unconstrained_prediction_weighted_target_row_norms"][f"age_{age}"]
        errors[f"age_{age}"] = abs(measured[0] - expected[f"age_{age}"])
        assert errors[f"age_{age}"] < _LIMITS["analytic_prediction_norm_max_abs_error"]
    assert expected["age_63"] > expected["age_0"]
    return {
        "circular_stereo_feature_energy": 0.5,
        "independent_two_by_two_hann_gram": gram.tolist(),
        "checked_full_window_center": 385,
        "analytic_norm_definition": "sqrt(h @ inverse(G) @ h.T), h=(-d/255,1+d/255)",
        "analytic_weighted_target_prediction_row_norms": expected,
        "reported_prediction_row_norm_abs_errors": errors,
        "actual_pcm_noise_or_nnls_active_set_sensitivity_claim": False,
    }


def _fit_decomposition(source: Real, frequencies: Real, coefficients: Complex, last: int) -> dict:
    envelopes, report = fit_predictive_causal_envelopes(source, frequencies, coefficients)
    _check(envelopes, report, len(source), len(frequencies), last)
    modes, remainder, closure = _decompose(source, envelopes, frequencies, coefficients)
    return {"envelopes": envelopes, "modes": modes, "remainder": remainder, "closure": closure}


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
        first = _fit_decomposition(source, frequencies, coefficients, end - 1)
        second = _fit_decomposition(changed, frequencies, coefficients, max(end - 1, _EVENT))
        assert np.array_equal(source[:_EVENT], changed[:_EVENT])
        for key in ("envelopes", "modes", "remainder"):
            assert np.array_equal(first[key][:_EVENT], second[key][:_EVENT])
        envelope_effect = _error(first["envelopes"][_EVENT:], second["envelopes"][_EVENT:])
        mode_effect = _error(first["modes"][_EVENT:], second["modes"][_EVENT:])
        assert envelope_effect > _LIMITS["post_event_envelope_difference_minimum"]
        assert mode_effect > _LIMITS["post_event_source_modes_difference_minimum"]
        retained = float(np.max(first["envelopes"][end:_EVENT])) if end < _EVENT else None
        if retained is not None:
            assert retained > _LIMITS["post_event_envelope_difference_minimum"]
        results[label] = {
            "source_hulls_inclusive": [
                [_FIRST_ACTIVE, end - 1],
                [_FIRST_ACTIVE, max(end - 1, _EVENT)],
            ],
            "prefix_envelopes_modes_and_subtraction_complements_bitwise_equal": True,
            "post_event_envelope_max_abs_difference": envelope_effect,
            "post_event_source_modes_max_abs_difference": mode_effect,
            "retained_post_activity_envelope_maximum_before_future_event": retained,
            "complementary_sum_max_abs_error": max(first["closure"], second["closure"]),
        }
    return {"future_impulse_frame": _EVENT, "impulse_off_hop_grid": True, **results}


def _refusal_and_silence_controls() -> dict:
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
        predicted, report = fit_predictive_causal_envelopes(source, frequencies, coefficients)
        _check(predicted, report, len(source), 2, len(source) - 1)
        assert report["rejected_tile_count"] == report["tile_count"] > 0
        assert set(report["tile_status_counts"]) == {"ill_conditioned"}
        assert np.all(predicted == 0.0)
        modes, remainder, _ = _decompose(source, predicted, frequencies, coefficients)
        assert np.all(modes == 0.0)
        assert np.array_equal(remainder, source)
        results[label] = {
            "refused_tile_count": report["rejected_tile_count"],
            "zero_modes_leave_original_source_in_exact_complement": True,
        }
    silence, report = fit_predictive_causal_envelopes(np.zeros((512, 2)), frequencies, coefficients)
    assert np.all(silence == 0.0)
    assert report["tile_count"] == 0
    assert report["source_activity_hull_inclusive"] is None
    return {**results, "silence_exact_zero_without_fitting": True}


def integrity_controls() -> dict:
    """Assert predeclared physical, projection, prediction-norm and causal controls."""
    return {
        "stage": "predictive_causal_mode_envelope_integrity_controls",
        "predeclared_numeric_acceptance_limits": dict(_LIMITS),
        "independent_affine_amplitudes_and_bases": _affine_controls(),
        "within_hop_nonnegative_projection": _projection_control(),
        "independent_analytic_prediction_row_norm": _analytic_norm_control(),
        "fixed_basis_local_causality_and_complete_source_decomposition": _future_controls(),
        "conditioning_refusals_and_silence": _refusal_and_silence_controls(),
        "complement_definition": "original_source_pcm - synthesized_source_modes",
        "truth_only_in_control_source_generation_and_analytic_expectations": True,
        "private_estimator_helpers_or_output_targets_used": False,
        "global_discovery_complete_pipeline_realtime_or_listening_acceptance_claim": False,
    }
