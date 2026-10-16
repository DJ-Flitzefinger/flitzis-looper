"""Independent finite-PCM controls for a fixed, positive source-clock AM basis.

The public fitter receives only source PCM and an already fixed carrier/modulator
basis. Common-gate truth and independent moment expectations remain confined to
these controls. Published physical envelopes multiply the common gate by AM once.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING

import numpy as np
from pitch_am_groups import group_am_carriers
from pitch_mode_carriers import CarrierEstimate
from pitch_mode_envelopes import fit_modulated_current_weight_envelopes
from pitch_source_modulation import SourceModulator, modulators_from_estimate

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]
type Modulators = tuple[SourceModulator, ...]

# Freeze source geometry, physical perturbation sizes and limits before execution.
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


@dataclass(frozen=True, slots=True)
class _AnalyticControl:
    source: Real
    frequencies: Real
    coefficients: Complex
    modulators: Modulators
    baseline: Real
    system: dict


def _basis(*, alternative: bool = False) -> tuple[Real, Complex, Modulators]:
    frequencies = np.array([1249.37, 3181.29] if alternative else [911.43, 2367.81])
    coefficients = np.array([
        [np.sqrt(0.7) * np.exp(0.31j), np.sqrt(0.3) * np.exp(-0.27j)],
        [np.sqrt(0.2) * np.exp(1.13j), np.sqrt(0.8) * np.exp(-0.54j)],
    ])
    parameters = (
        ((19.61, -0.11 + 0.09j), (43.73, 0.09 - 0.12j))
        if alternative
        else (
            (31.43, 0.13 + 0.07j),
            (47.17, -0.08 + 0.11j),
        )
    )
    if alternative:
        coefficients = coefficients[:, ::-1].conjugate()
    modulators = tuple(
        SourceModulator(frequency_hz=frequency, ratio=ratio) for frequency, ratio in parameters
    )
    return frequencies, coefficients / np.linalg.norm(coefficients, axis=1)[:, None], modulators


def _features(frames: int, frequencies: Real, coefficients: Complex) -> Real:
    phase = 2.0 * np.pi * np.outer(np.arange(frames), frequencies) / _FS
    return (
        np.cos(phase)[:, :, None] * coefficients.real[None, :, :]
        - np.sin(phase)[:, :, None] * coefficients.imag[None, :, :]
    )


def _modulations(frames: int, modulators: Modulators) -> Real:
    """Synthesize AM directly without using a production modulator helper."""
    values = np.ones((frames, len(modulators)))
    for index, modulator in enumerate(modulators):
        phase = 2.0 * np.pi * modulator.frequency_hz * np.arange(frames) / _FS
        values[:, index] += 2.0 * (
            modulator.ratio.real * np.cos(phase) - modulator.ratio.imag * np.sin(phase)
        )
    return values


def _modes(envelopes: Real, frequencies: Real, coefficients: Complex) -> Real:
    """Physical envelopes multiply unmodulated carrier features exactly once."""
    return np.sum(
        envelopes[:, :, None] * _features(len(envelopes), frequencies, coefficients), axis=1
    )


def _source(gates: Real, frequencies: Real, coefficients: Complex, modulators: Modulators) -> Real:
    return _modes(gates * _modulations(len(gates), modulators), frequencies, coefficients)


def _error(actual: Real, expected: Real) -> float:
    return float(np.max(np.abs(actual - expected)))


def _detail(report: dict, center: int = _CENTER) -> dict:
    return next(item for item in report["first_knot_details"] if item["center_frame"] == center)


def _check_norm_reports(report: dict, count: int) -> None:
    for prefix in ("", "physical_envelope_"):
        for domain in ("weighted_target", "pcm"):
            summary = report[f"{prefix}unconstrained_prediction_{domain}_row_norm_summary"]
            for age in ("age_0", "age_63"):
                assert summary[age]["finite_count"] + summary[age]["unavailable_count"] == count
                for detail in report["first_knot_details"]:
                    values = detail[f"{prefix}unconstrained_prediction_{domain}_row_norms"][age]
                    if values is not None:
                        assert len(values) == report["carrier_count"]
                        assert np.isfinite(values).all()
                        assert np.all(np.asarray(values) >= 0.0)
    weighted = report["unconstrained_prediction_weighted_target_row_norm_summary"]
    pcm = report["unconstrained_prediction_pcm_row_norm_summary"]
    assert weighted["actual_pcm_noise_sensitivity_claim"] is False
    assert pcm["nnls_active_set_sensitivity_claim"] is False
    assert pcm["projection_active_set_sensitivity_claim"] is False
    assert pcm["unconstrained_linear_pcm_sensitivity"] is True


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
    assert report["condition_limit"] == _LIMITS["condition_limit"]
    assert report["maximum_passive_solves_per_tile"] == _LIMITS["maximum_passive_solves_per_tile"]
    assert report["maximum_active_set_solves"] <= _LIMITS["maximum_passive_solves_per_tile"]
    knots = list(range(_FIRST_ACTIVE, frames, _HOP))
    assert report["tile_count"] == len(knots)
    assert [item["center_frame"] for item in report["first_knot_details"]] == knots[:16]
    assert report["omitted_knot_detail_count"] == max(0, len(knots) - 16)
    _check_norm_reports(report, len(knots) * carriers)


def _complement(source: Real, envelopes: Real, frequencies: Real, coefficients: Complex) -> dict:
    modes = _modes(envelopes, frequencies, coefficients)
    remainder = source - modes
    closure = _error(modes + remainder, source)
    assert closure < _LIMITS["complementary_sum_max_abs_error"]
    return {"envelopes": envelopes, "modes": modes, "remainder": remainder, "closure": closure}


def _decomposition(
    source: Real, frequencies: Real, coefficients: Complex, modulators: Modulators, last: int
) -> dict:
    envelopes, report = fit_modulated_current_weight_envelopes(
        source, frequencies, coefficients, modulators
    )
    _check(envelopes, report, len(source), len(frequencies), last)
    return _complement(source, envelopes, frequencies, coefficients)


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
    return {
        "common_gate_endpoint_max_abs_error": endpoint_error,
        "gate_slope_max_abs_error": slope_error,
    }


def _affine_controls() -> list[dict]:
    results = []
    for alternative in (False, True):
        frequencies, coefficients, modulators = _basis(alternative=alternative)
        slopes = np.array([-4.0e-5, 2.5e-5] if alternative else [3.0e-5, -2.0e-5])
        initial = np.array([0.28, 0.10] if alternative else [0.12, 0.30])
        gates = initial + (np.arange(2049) - _FIRST_ACTIVE)[:, None] * slopes
        gates[:_FIRST_ACTIVE] = 0.0
        modulations = _modulations(len(gates), modulators)
        truth = gates * modulations
        source = _source(gates, frequencies, coefficients, modulators)
        fitted, report = fit_modulated_current_weight_envelopes(
            source, frequencies, coefficients, modulators
        )
        _check(fitted, report, len(source), 2, len(source) - 1)
        assert report["rejected_tile_count"] == 0
        interior = slice(_CENTER, len(source))
        assert (interior.stop - interior.start) % _HOP == 0
        measured = _error(fitted[interior], truth[interior])
        assert measured < _LIMITS["physical_amplitude_max_abs_error"]
        assert np.all(gates[interior] > 0.0)
        assert np.all(modulations > 0.0)
        results.append({
            "control_only_carrier_frequencies_hz": frequencies.tolist(),
            "control_only_modulation_frequencies_hz": [item.frequency_hz for item in modulators],
            "common_gate_slopes_per_frame": slopes.tolist(),
            "whole_supported_hops_inclusive": [interior.start, interior.stop - 1],
            "physical_envelope_whole_hop_max_abs_error": measured,
            **_endpoint_errors(report, gates, slopes),
            "finite_final_frame_is_age_63_without_extra_end_knot": True,
            "positive_nonconstant_modulation_and_positive_affine_common_gates": True,
            "complementary_sum_max_abs_error": _complement(
                source, fitted, frequencies, coefficients
            )["closure"],
        })
    return results


def _projection_control() -> dict:
    frequencies, coefficients, modulators = _basis()
    frequencies, coefficients, modulators = frequencies[:1], coefficients[:1], modulators[:1]
    crossing, magnitude = 408.5, 4.0e-4
    gates = np.maximum(0.0, magnitude * (crossing - np.arange(768)))[:, None]
    gates[:_FIRST_ACTIVE] = 0.0
    source = _source(gates, frequencies, coefficients, modulators)
    fitted, report = fit_modulated_current_weight_envelopes(
        source, frequencies, coefficients, modulators
    )
    _check(fitted, report, len(source), 1, 408)
    expected = gates * _modulations(len(gates), modulators)
    measured = _error(fitted[_CENTER : _CENTER + _HOP], expected[_CENTER : _CENTER + _HOP])
    assert measured < _LIMITS["physical_amplitude_max_abs_error"]
    assert np.all(gates[_CENTER - 255 : _CENTER + 1] > 0.0)
    assert np.all(fitted[409 : _CENTER + _HOP] == 0.0)
    return {
        "checked_supported_hop_inclusive": [_CENTER, _CENTER + _HOP - 1],
        "analytic_common_gate_zero_crossing_frame": crossing,
        "projected_physical_envelope_hop_max_abs_error": measured,
        "common_gate_prediction_projected_before_positive_am_multiplication": True,
        "complementary_sum_max_abs_error": _complement(source, fitted, frequencies, coefficients)[
            "closure"
        ],
    }


def _analytic_system(modulators: Modulators) -> dict:
    fraction = np.arange(_WINDOW, dtype=np.float64) / 255.0
    weights = 0.5 * (1.0 - np.cos(np.pi * fraction))
    endpoints = np.column_stack((1.0 - fraction, fraction))
    modulation = _modulations(_CENTER + _HOP, modulators)[:, 0]
    tile_modulation = modulation[_CENTER - 255 : _CENTER + 1]
    assert np.all(modulation > 0.0)
    assert not np.all(tile_modulation == tile_modulation[0])
    effective_weights = weights * tile_modulation**2
    moments = np.array([np.sum(effective_weights * fraction**power) for power in range(3)])
    gram = 0.5 * np.array([
        [moments[0] - 2.0 * moments[1] + moments[2], moments[1] - moments[2]],
        [moments[1] - moments[2], moments[2]],
    ])
    direct = 0.5 * endpoints.T @ (effective_weights[:, None] * endpoints)
    gram_error = _error(gram, direct)
    assert gram_error < _LIMITS["kernel_moment_max_abs_error"]
    squared_gram = 0.5 * endpoints.T @ ((weights**2 * tile_modulation**2)[:, None] * endpoints)
    eigenvalues = np.linalg.eigvalsh(gram)
    return {
        "weights": weights,
        "endpoints": endpoints,
        "gram": gram,
        "squared_gram": squared_gram,
        "inverse": np.linalg.inv(gram),
        "modulation": modulation,
        "tile_modulation": tile_modulation,
        "moments": moments,
        "gram_error": gram_error,
        "condition": float(np.sqrt(eigenvalues[-1] / eigenvalues[0])),
        "offsets": np.arange(-255, 1, dtype=np.float64),
    }


def _moment_at_age(detail: dict, system: dict, age: int) -> dict:
    row = np.array([-age / 255.0, 1.0 + age / 255.0])
    transform = row @ system["inverse"]
    modulation = system["modulation"][_CENTER + age]
    expected = {
        "weighted_target": float(np.sqrt(transform @ row)),
        "pcm": float(np.sqrt(transform @ system["squared_gram"] @ transform)),
    }
    gate_kernel = (
        0.5 * system["weights"] * system["tile_modulation"] ** 2 * (system["endpoints"] @ transform)
    )
    physical_kernel = modulation * gate_kernel / system["tile_modulation"]
    errors = {
        "gate_kernel_sum": abs(float(np.sum(gate_kernel)) - 1.0),
        "gate_kernel_first_past_moment": abs(float(gate_kernel @ system["offsets"]) - age),
        "physical_kernel_modulated_constant": abs(
            float(physical_kernel @ system["tile_modulation"]) - modulation
        ),
        "physical_kernel_modulated_affine": abs(
            float(physical_kernel @ (system["tile_modulation"] * system["offsets"]))
            - modulation * age
        ),
    }
    assert max(errors.values()) < _LIMITS["kernel_moment_max_abs_error"]
    for domain in ("weighted_target", "pcm"):
        for prefix, multiplier in (("", 1.0), ("physical_envelope_", modulation)):
            measured = detail[f"{prefix}unconstrained_prediction_{domain}_row_norms"][f"age_{age}"][
                0
            ]
            errors[f"{prefix}{domain}_norm"] = abs(measured - multiplier * expected[domain])
            assert (
                errors[f"{prefix}{domain}_norm"] < _LIMITS["analytic_prediction_norm_max_abs_error"]
            )
    return {
        "common_gate_analytic_norms": expected,
        "physical_envelope_multiplier": float(modulation),
        "absolute_errors": errors,
    }


def _norm_and_moment_controls(report: dict, system: dict) -> dict:
    detail = _detail(report)
    condition_error = abs(detail["condition_number"] - system["condition"])
    assert condition_error < _LIMITS["analytic_prediction_norm_max_abs_error"]
    return {
        "independent_am_weighted_fraction_moments_0_1_2": system["moments"].tolist(),
        "independent_gram": system["gram"].tolist(),
        "independent_squared_weight_gram": system["squared_gram"].tolist(),
        "direct_moment_gram_max_abs_error": system["gram_error"],
        "actual_modulated_design_condition_abs_error": condition_error,
        "common_gate_pcm_norm_formula": "sqrt(h @ inverse(G) @ G2 @ inverse(G) @ h.T)",
        "physical_envelope_norm_formula": "m(center+d) * common_gate_norm",
        "gate_kernel_moments": "sum(k)=1; sum(k*past_offset)=d",
        "physical_kernel_moments": "sum(K*m)=m_future; sum(K*m*past_offset)=m_future*d",
        "norms_conditional_on_fixed_basis_and_inactive_constraints": True,
        **{f"age_{age}": _moment_at_age(detail, system, age) for age in (0, 63)},
    }


def _prediction_rows() -> Real:
    ages = np.arange(_HOP, dtype=np.float64) / 255.0
    return np.column_stack((-ages, 1.0 + ages))


def _signed_response(
    control: _AnalyticControl,
    perturbation: Real,
    endpoint_delta: Real,
) -> list[dict]:
    gate_delta = _prediction_rows() @ endpoint_delta
    expected = control.system["modulation"][_CENTER : _CENTER + _HOP] * gate_delta
    results = []
    for sign in (-1.0, 1.0):
        fitted, report = fit_modulated_current_weight_envelopes(
            control.source + sign * perturbation,
            control.frequencies,
            control.coefficients,
            control.modulators,
        )
        detail = _detail(report)
        endpoints = np.array([detail["left_endpoints"][0], detail["current_right_endpoints"][0]])
        assert detail["status"] == "converged"
        assert np.all(endpoints > 0.0)
        assert np.all(_PLATEAU + sign * gate_delta > 0.0)
        assert np.all(fitted[_CENTER : _CENTER + _HOP] > 0.0)
        errors = {
            "common_gate_endpoints": _error(endpoints, _PLATEAU + sign * endpoint_delta),
            "whole_hop_physical_envelope": _error(
                fitted[_CENTER : _CENTER + _HOP, 0] - control.baseline[_CENTER : _CENTER + _HOP, 0],
                sign * expected,
            ),
        }
        assert max(errors.values()) < _LIMITS["physical_amplitude_max_abs_error"]
        results.append({
            "sign": sign,
            "maximum_absolute_errors": errors,
            "minimum_positive_fitted_endpoint": float(np.min(endpoints)),
            "minimum_positive_gate_prediction": float(np.min(_PLATEAU + sign * gate_delta)),
        })
    return results


def _current_cue_control(control: _AnalyticControl) -> dict:
    features = _features(len(control.source), control.frequencies, control.coefficients)[:, 0]
    perturbation = np.zeros_like(control.source)
    perturbation[_CENTER] = _CUE * features[_CENTER]
    delta = (
        0.5
        * _CUE
        * control.system["weights"][-1]
        * control.system["tile_modulation"][-1]
        * control.system["inverse"][:, 1]
    )
    assert not np.any(perturbation[:_CENTER])
    assert not np.any(perturbation[_CENTER + 1 :])
    return {
        "physical_current_pcm_cue": (
            "+/-0.001 * UNMODULATED circular carrier vector; frame 385 only"
        ),
        "endpoint_delta_formula": "0.5*epsilon*w_current*m_current*inverse(G) @ e_R",
        "independent_common_gate_endpoint_delta": delta.tolist(),
        "checked_whole_hop_inclusive": [_CENTER, _CENTER + _HOP - 1],
        "signed_cue_evidence": _signed_response(control, perturbation, delta),
        "nnls_and_projection_constraints_inactive_for_both_signs_and_all_64_predictions": True,
    }


def _pcm_noise_control(control: _AnalyticControl) -> dict:
    row = np.array([-63.0 / 255.0, 1.0 + 63.0 / 255.0])
    transform = row @ control.system["inverse"]
    features = _features(len(control.source), control.frequencies, control.coefficients)[
        _CENTER - 255 : _CENTER + 1, 0
    ]
    pcm_row = (
        control.system["weights"]
        * control.system["tile_modulation"]
        * (control.system["endpoints"] @ transform)
    )[:, None] * features
    row_norm = float(np.linalg.norm(pcm_row))
    physical_norm = float(control.system["modulation"][_CENTER + 63] * row_norm)
    perturbation = np.zeros_like(control.source)
    perturbation[_CENTER - 255 : _CENTER + 1] = _CUE * pcm_row / row_norm
    tile_noise = perturbation[_CENTER - 255 : _CENTER + 1]
    rhs = control.system["endpoints"].T @ (
        control.system["weights"]
        * control.system["tile_modulation"]
        * np.sum(features * tile_noise, axis=1)
    )
    delta = control.system["inverse"] @ rhs
    norm_error = abs(float(np.linalg.norm(perturbation)) - _CUE)
    gain = float(control.system["modulation"][_CENTER + 63] * (row @ delta) / _CUE)
    gain_error = abs(gain - physical_norm)
    assert norm_error < _LIMITS["kernel_moment_max_abs_error"]
    assert gain_error < _LIMITS["analytic_prediction_norm_max_abs_error"]
    return {
        "physical_pcm_noise_euclidean_norm": _CUE,
        "noise_direction": "independent age-63 original-PCM prediction row normalized to unit norm",
        "noise_norm_abs_error": norm_error,
        "analytic_physical_envelope_age_63_gain": physical_norm,
        "independent_noise_direction_gain_abs_error": gain_error,
        "signed_noise_evidence": _signed_response(control, perturbation, delta),
        "fixed_basis_unconstrained_gain_only_with_checked_inactive_constraints": True,
    }


def _analytic_controls() -> dict:
    frequencies = np.array([1093.73])
    coefficients = np.array([[1.0, -1.0j]]) / np.sqrt(2.0)
    modulators = (SourceModulator(frequency_hz=37.19, ratio=0.17 + 0.09j),)
    gates = np.zeros((1024, 1))
    gates[_FIRST_ACTIVE:] = _PLATEAU
    source = _source(gates, frequencies, coefficients, modulators)
    fitted, report = fit_modulated_current_weight_envelopes(
        source, frequencies, coefficients, modulators
    )
    _check(fitted, report, len(source), 1, len(source) - 1)
    assert report["rejected_tile_count"] == 0
    system = _analytic_system(modulators)
    control = _AnalyticControl(source, frequencies, coefficients, modulators, fitted, system)
    return {
        "unmodulated_circular_stereo_feature_energy": 0.5,
        "checked_full_window_center": _CENTER,
        "positive_nonconstant_known_am": True,
        "norms_and_kernel_moments": _norm_and_moment_controls(report, system),
        "signed_physical_current_frame_cue": _current_cue_control(control),
        "signed_actual_pcm_noise": _pcm_noise_control(control),
    }


def _future_pair(
    frequencies: Real, coefficients: Complex, modulators: Modulators, end: int
) -> dict:
    gates = np.zeros((2049, 2))
    gates[_FIRST_ACTIVE:end] = [0.24, 0.16]
    source = _source(gates, frequencies, coefficients, modulators)
    changed = source.copy()
    changed[_EVENT] += [0.47, -0.31]
    first = _decomposition(source, frequencies, coefficients, modulators, end - 1)
    second = _decomposition(changed, frequencies, coefficients, modulators, max(end - 1, _EVENT))
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
    return {
        "source_hulls_inclusive": [[_FIRST_ACTIVE, end - 1], [_FIRST_ACTIVE, max(end - 1, _EVENT)]],
        "prefix_physical_envelopes_modes_and_exact_subtraction_complements_bitwise_equal": True,
        "post_event_max_abs_differences": effect,
        "retained_post_activity_physical_envelope_maximum_before_future_event": retained,
        "complementary_sum_max_abs_error": max(first["closure"], second["closure"]),
    }


def _future_controls() -> dict:
    frequencies, coefficients, modulators = _basis()
    assert (_EVENT - _FIRST_ACTIVE) % _HOP != 0
    return {
        "identical_fixed_carriers_stereo_vectors_and_modulators_for_each_pair": True,
        "future_impulse_frame": _EVENT,
        "impulse_off_hop_grid": True,
        "same_last_active_hull": _future_pair(frequencies, coefficients, modulators, 1536),
        "different_last_active_hulls": _future_pair(frequencies, coefficients, modulators, 800),
        "global_discovery_or_modulator_estimation_causality_claim": False,
    }


def _conditioning_controls() -> dict:
    _, basis, modulation_basis = _basis()
    coefficients, modulators = basis[[0, 0]], (modulation_basis[0], modulation_basis[0])
    results = {}
    for label, frequencies in (
        ("duplicate", np.array([421.73, 421.73])),
        ("near_duplicate", np.array([421.73, 421.73000001])),
    ):
        gates = np.zeros((1024, 2))
        gates[_FIRST_ACTIVE:] = [0.24, 0.16]
        source = _source(gates, frequencies, coefficients, modulators)
        fitted, report = fit_modulated_current_weight_envelopes(
            source, frequencies, coefficients, modulators
        )
        _check(fitted, report, len(source), 2, len(source) - 1)
        assert report["rejected_tile_count"] == report["tile_count"] > 0
        assert set(report["tile_status_counts"]) == {"ill_conditioned"}
        assert _detail(report)["condition_number"] is None or (
            _detail(report)["condition_number"] > _LIMITS["condition_limit"]
        )
        assert np.all(fitted == 0.0)
        assert np.array_equal(source - _modes(fitted, frequencies, coefficients), source)
        results[label] = {"refused_tile_count": report["rejected_tile_count"]}
    return results


def _silence_and_empty_controls() -> dict:
    frequencies, coefficients, modulators = _basis()
    silence, report = fit_modulated_current_weight_envelopes(
        np.zeros((512, 2)), frequencies, coefficients, modulators
    )
    assert np.all(silence == 0.0)
    assert report["tile_count"] == 0
    assert report["source_activity_hull_inclusive"] is None
    source = np.ones((512, 2))
    empty_frequencies, empty_coefficients = np.zeros(0), np.zeros((0, 2), dtype=np.complex128)
    empty, empty_report = fit_modulated_current_weight_envelopes(
        source, empty_frequencies, empty_coefficients, ()
    )
    assert empty.shape == (len(source), 0)
    assert empty_report["tile_count"] == 0
    assert np.array_equal(source - _modes(empty, empty_frequencies, empty_coefficients), source)
    return {
        "silence_exact_zero_without_fitting": True,
        "zero_carriers_n_by_zero_with_exact_original_source_complement": True,
    }


def _mixed_certificate_source() -> dict:
    frequencies = np.array([611.83, 1437.61])
    coefficients = _basis()[1][::-1]
    modulators = (
        SourceModulator(frequency_hz=0.0, ratio=0.0j),
        SourceModulator(frequency_hz=29.37, ratio=-0.11 + 0.09j),
    )
    gates = np.zeros((24_576, 2))
    gates[_FIRST_ACTIVE:] = [0.16, 0.24]
    original_frequencies = np.array([
        frequencies[1] - modulators[1].frequency_hz,
        frequencies[1],
        frequencies[1] + modulators[1].frequency_hz,
        frequencies[0],
    ])
    vectors = np.stack((
        np.conj(modulators[1].ratio) * coefficients[1],
        coefficients[1],
        modulators[1].ratio * coefficients[1],
        coefficients[0],
    ))
    norms = np.linalg.norm(vectors, axis=1)
    order = np.argsort(original_frequencies)
    amplitudes = np.zeros((len(gates), 4))
    amplitudes[_FIRST_ACTIVE:] = (np.array([0.24, 0.24, 0.24, 0.16]) * norms)[order]
    source = _source(gates, frequencies, coefficients, modulators)
    estimate = CarrierEstimate(
        tuple(float(value) for value in original_frequencies[order]),
        (vectors / norms[:, None])[order],
        {"stage": "independent_control_fixed_four_carrier_estimate"},
    )
    expansion_error = _error(
        _modes(amplitudes, np.asarray(estimate.frequencies_hz), estimate.stereo_coefficients),
        source,
    )
    assert expansion_error < _LIMITS["physical_amplitude_max_abs_error"]
    return {
        "source": source,
        "estimate": estimate,
        "frequencies": frequencies,
        "coefficients": coefficients,
        "modulators": modulators,
        "gates": gates,
        "stationary_expansion_max_abs_error": expansion_error,
    }


def _mixed_certificate_control() -> dict:
    control = _mixed_certificate_source()
    grouped = group_am_carriers(control["source"], control["estimate"])
    grouping = grouped.report["am_grouping"]
    assert len(grouped.frequencies_hz) == 2
    assert np.array_equal(np.asarray(grouped.frequencies_hz), control["frequencies"])
    assert grouping["group_count"] == 1
    assert grouping["accepted_groups"][0]["original_indices"] == [1, 2, 3]
    certificate = next(value for value in grouping["candidates"] if value["grouped"])
    assert certificate["support_windows"] >= 3
    assert grouping["complete_source_windows"] >= 3
    assert not certificate["refusal_counts"]
    modulators = modulators_from_estimate(grouped)
    assert modulators is not None
    assert len(modulators) == 2
    assert modulators[0] == SourceModulator(frequency_hz=0.0, ratio=0.0j)
    errors = {
        "global_sideband_ratio": abs(modulators[1].ratio - control["modulators"][1].ratio),
        "modulation_frequency_hz": abs(
            modulators[1].frequency_hz - control["modulators"][1].frequency_hz
        ),
        "retained_complex_stereo_vectors": _error(
            grouped.stereo_coefficients, control["coefficients"]
        ),
    }
    assert max(errors.values()) < _LIMITS["physical_amplitude_max_abs_error"]
    fitted, report = fit_modulated_current_weight_envelopes(
        control["source"],
        np.asarray(grouped.frequencies_hz),
        grouped.stereo_coefficients,
        modulators,
    )
    _check(fitted, report, len(control["source"]), 2, len(control["source"]) - 1)
    assert report["rejected_tile_count"] == 0
    truth = control["gates"] * _modulations(len(fitted), control["modulators"])
    stop = _FIRST_ACTIVE + ((len(fitted) - _FIRST_ACTIVE) // _HOP) * _HOP
    assert (stop - _CENTER) % _HOP == 0
    errors["complete_supported_hops_physical_envelope"] = _error(
        fitted[_CENTER:stop], truth[_CENTER:stop]
    )
    errors["supported_finite_tail_physical_envelope"] = _error(fitted[stop:], truth[stop:])
    assert max(errors.values()) < _LIMITS["physical_amplitude_max_abs_error"]
    return {
        "original_and_retained_carrier_counts": [4, 2],
        "certified_family_original_indices": certificate["original_indices"],
        "clean_complete_certificate_support_windows": certificate["support_windows"],
        "certificate_window_and_hop_frames": [grouping["window_frames"], grouping["hop_frames"]],
        "source_frames_and_first_active_frame": [len(fitted), _FIRST_ACTIVE],
        "flat_modulator_retained_index": 0,
        "globally_phased_family_modulator_retained_index": 1,
        "stationary_expansion_max_abs_error": control["stationary_expansion_max_abs_error"],
        "complete_supported_hops_inclusive": [_CENTER, stop - 1],
        "absolute_errors": errors,
        "complementary_sum_max_abs_error": _complement(
            control["source"],
            fitted,
            np.asarray(grouped.frequencies_hz),
            grouped.stereo_coefficients,
        )["closure"],
        "public_certificate_to_modulator_to_mixed_basis_fit_exercised": True,
        "source_clock_am_published_once_for_family_and_flat_carrier_unchanged": True,
        "unique_physical_am_interpretation_claim": False,
    }


def _validation_controls() -> dict:
    frequencies, coefficients, valid = _basis()
    source = np.zeros((512, 2))
    parameters = {
        "negative_modulation_frequency": (-1.0, 0.1 + 0.0j),
        "above_nyquist_modulation_frequency": (_FS / 2.0 + 1.0, 0.1 + 0.0j),
        "nonfinite_modulation_frequency": (float("nan"), 0.1 + 0.0j),
        "nonfinite_modulation_ratio_real": (31.43, complex(float("inf"), 0.0)),
        "nonfinite_modulation_ratio_imaginary": (31.43, complex(0.0, float("nan"))),
        "nonpositive_am_boundary": (31.43, 0.5 + 0.0j),
        "negative_am_possible": (31.43, 0.75 + 0.0j),
    }
    refused = []
    for label, (frequency, ratio) in parameters.items():
        try:
            invalid = SourceModulator(frequency_hz=frequency, ratio=ratio)
            fit_modulated_current_weight_envelopes(
                source, frequencies, coefficients, (invalid,) * 2
            )
        except ValueError:
            refused.append(label)
        else:
            message = f"Invalid AM basis accepted: {label}"
            raise AssertionError(message)
    try:
        fit_modulated_current_weight_envelopes(source, frequencies, coefficients, valid[:1])
    except ValueError:
        refused.append("modulator_count_mismatch")
    else:
        message = "Modulator count mismatch accepted"
        raise AssertionError(message)
    return {
        "invalid_modulators_refused_even_for_silence_before_fit_shortcut": refused,
        "strict_positive_am_bound_required": "2*abs(ratio)<1",
    }


def integrity_controls() -> dict:
    """Assert independent physical-envelope, gate, sensitivity and refusal controls."""
    return {
        "stage": "source_am_basis_causal_envelope_integrity_controls",
        "predeclared_numeric_acceptance_limits": dict(_LIMITS),
        "independent_affine_common_gates_and_unseen_bases": _affine_controls(),
        "within_hop_gate_projection_then_positive_am": _projection_control(),
        "independent_analytic_norms_moments_and_physical_pcm_cues": _analytic_controls(),
        "fixed_basis_local_causality_and_complete_source_decomposition": _future_controls(),
        "conditioning_refusals": _conditioning_controls(),
        "silence_and_zero_carriers": _silence_and_empty_controls(),
        "public_certificate_to_mixed_am_and_stationary_basis": _mixed_certificate_control(),
        "invalid_modulator_refusals": _validation_controls(),
        "complement_definition": "original_source_pcm - physical_envelope * unmodulated_carrier",
        "physical_envelope_contains_source_clock_am_exactly_once": True,
        "common_gate_and_published_physical_envelope_norms_separately_verified": True,
        "truth_only_in_control_source_generation_and_independent_analytic_expectations": True,
        "private_estimator_helpers_or_fixture_output_targets_used": False,
        "global_discovery_complete_pipeline_realtime_or_listening_acceptance_claim": False,
    }
