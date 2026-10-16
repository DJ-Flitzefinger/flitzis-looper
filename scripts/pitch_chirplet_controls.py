"""Independent source, fixed-phase envelope and complete chirplet render controls.

Truth enters generated control PCM and analytic expectations only. The estimator
receives original PCM; no fixture identifier, target, pitch or rate reaches it.
These are numeric integrity controls, without an acoustic or production claim.
"""

import math
from typing import TYPE_CHECKING

import numpy as np
from pitch_amfm_model import render as render_residual
from pitch_chirplet_carriers import estimate_chirplet
from pitch_independent_modes import (
    Mode,
    Prepared,
    UnsupportedPitchDomainError,
    prepare,
    render_prepared,
)
from pitch_mode_envelopes import (
    fit_current_weight_causal_envelopes,
    fit_phase_current_weight_envelopes,
)
from pitch_source_phase import LinearSourcePhase

if TYPE_CHECKING:
    from collections.abc import Callable

    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]

_FS, _WINDOW, _HOP, _BIRTH, _CENTER, _EVENT = 48_000, 256, 64, 129, 385, 1024
_LIMITS = {
    "estimated_frequency_hz_max_abs_error": 1.0e-5,
    "estimated_slope_hz_per_second_max_abs_error": 1.0e-4,
    "global_gauge_coefficient_max_abs_error": 1.0e-5,
    "full_source_factorization_relative_rms": 1.0e-5,
    "integrated_phase_radians_max_abs_error": 2.0e-10,
    "physical_envelope_max_abs_error": 2.0e-10,
    "independent_gram_prediction_max_abs_error": 2.0e-10,
    "exact_complement_max_abs_error": 2.0e-15,
    "render_integer_phase_max_abs_error": 1.0e-9,
    "complete_fractional_identity_max_abs_error": 1.0e-9,
    "post_event_difference_minimum": 1.0e-8,
}


def _error(actual: Real | Complex, expected: Real | Complex) -> float:
    return float(np.max(np.abs(actual - expected), initial=0.0))


def _unit(values: list[complex]) -> Complex:
    coefficients = np.asarray(values, dtype=np.complex128)
    return coefficients / np.linalg.norm(coefficients)


def _cycles(positions: Real, epoch: float, frequency: float, slope: float) -> Real:
    """Independently integrate the declared affine law in physical seconds."""
    seconds = (np.asarray(positions, dtype=np.float64) - epoch) / _FS
    return frequency * seconds + 0.5 * slope * seconds**2


def _independent_features(frames: int, laws: tuple, coefficients: Complex) -> Real:
    clock = np.arange(frames, dtype=np.float64)
    phase = np.column_stack([
        _cycles(clock, law.epoch_frame, law.frequency_hz, law.slope_hz_per_second) for law in laws
    ])
    return (np.exp(2.0j * np.pi * phase)[:, :, None] * coefficients[None]).real


def _source(gates: Real, laws: tuple, coefficients: Complex) -> Real:
    return np.sum(gates[:, :, None] * _independent_features(len(gates), laws, coefficients), axis=1)


def _complete_complement(source: Real, gates: Real, laws: tuple, coefficients: Complex) -> dict:
    modes = _source(gates, laws, coefficients)
    remainder = source - modes
    closure = _error(modes + remainder, source)
    assert closure < _LIMITS["exact_complement_max_abs_error"]
    assert len(modes) == len(remainder) == len(source)
    return {"modes": modes, "remainder": remainder, "closure": closure}


def _chirplet_source(
    frames: int, frequency: float, slope: float, coefficients: Complex, amplitude: float = 0.37
) -> Real:
    theta = 2.0 * np.pi * _cycles(np.arange(frames), 0.0, frequency, slope)
    return amplitude * (np.exp(1.0j * theta)[:, None] * coefficients).real


def _unseen_source_controls() -> list[dict]:
    controls = (
        (733.19, 1173.43, _unit([0.61 * np.exp(0.83j), 0.79 * np.exp(-1.17j)])),
        (2147.73, -889.61, _unit([0.4 * np.exp(-0.47j), 0.916 * np.exp(1.11j)])),
    )
    results = []
    for frequency, slope, coefficients in controls:
        source = _chirplet_source(24_576, frequency, slope, coefficients)
        estimate = estimate_chirplet(source)
        assert estimate.phase is not None
        phase = estimate.phase
        epoch = phase.epoch_frame
        truth_at_epoch = frequency + slope * epoch / _FS
        gauge = np.exp(2.0j * np.pi * _cycles(np.array([epoch]), 0.0, frequency, slope)[0])
        expected_coefficients = gauge * coefficients
        assert estimate.stereo_coefficients.shape == (1, 2)
        errors = {
            "frequency_at_declared_epoch_hz": abs(phase.frequency_hz - truth_at_epoch),
            "slope_hz_per_second": abs(phase.slope_hz_per_second - slope),
            "fixed_global_gauge_stereo_coefficients": _error(
                estimate.stereo_coefficients[0], expected_coefficients
            ),
        }
        assert (
            errors["frequency_at_declared_epoch_hz"]
            < _LIMITS["estimated_frequency_hz_max_abs_error"]
        )
        assert (
            errors["slope_hz_per_second"] < _LIMITS["estimated_slope_hz_per_second_max_abs_error"]
        )
        assert (
            errors["fixed_global_gauge_stereo_coefficients"]
            < _LIMITS["global_gauge_coefficient_max_abs_error"]
        )
        assert abs(np.linalg.norm(estimate.stereo_coefficients[0]) - 1.0) < 1.0e-12
        # One declared epoch and one gauge reconstruct EVERY original sample.
        clock = np.arange(len(source), dtype=np.float64)
        modeled = (
            0.37
            * (
                np.exp(2.0j * np.pi * phase.cycles(clock))[:, None]
                * estimate.stereo_coefficients[0]
            ).real
        )
        relative = float(np.linalg.norm(source - modeled) / np.linalg.norm(source))
        assert relative < _LIMITS["full_source_factorization_relative_rms"]
        expected_frequency = frequency + slope * clock / _FS
        law_frequency_error = _error(phase.frequency_at(clock), expected_frequency)
        assert law_frequency_error < _LIMITS["estimated_frequency_hz_max_abs_error"]
        results.append({
            "independently_generated_source_frequency_at_frame_zero_hz": frequency,
            "independently_generated_slope_hz_per_second": slope,
            "source_frames": len(source),
            "complete_8192_frame_windows_at_4096_frame_hop": 5,
            "declared_estimated_source_epoch_frame": epoch,
            "errors": errors,
            "frequency_law_max_abs_error_hz": law_frequency_error,
            "all_original_pcm_global_gauge_factorization_relative_rms": relative,
            "per_window_phase_alignment_or_reset": False,
        })
    return results


def _refused(source: Real, label: str) -> dict:
    estimate = estimate_chirplet(source)
    assert estimate.phase is None, f"Unsupported source model admitted: {label}"
    assert estimate.stereo_coefficients.shape == (0, 2)
    return {
        "refused": True,
        "source_frames": len(source),
        "rejection": estimate.report["rejection"],
        "accepted_window_count": estimate.report.get("accepted_window_count", 0),
        "candidates": estimate.report.get("candidates", []),
    }


def _negative_source_controls() -> dict:
    frames = 24_576
    coefficients = _unit([0.53 * np.exp(0.27j), 0.85 * np.exp(-0.91j)])
    base = _chirplet_source(frames, 1121.39, 717.83, coefficients)
    clock = np.arange(frames, dtype=np.float64)
    seconds = clock / _FS
    nonlinear_phase = (
        2.0 * np.pi * (1121.39 * seconds + 0.5 * 717.83 * seconds**2 + 270.0 * seconds**3)
    )
    nonlinear = 0.37 * (np.exp(1.0j * nonlinear_phase)[:, None] * coefficients).real
    jumped = base.copy()
    jumped[12_000:] = _chirplet_source(frames, 1121.39, 717.83, coefficients * 1.0j)[12_000:]
    impulse = np.zeros_like(base)
    impulse[123] = [0.7, -0.4]
    pair = _chirplet_source(frames, 831.17, 0.0, coefficients)
    pair += _chirplet_source(frames, 833.53, 0.0, coefficients.conjugate(), 0.29)
    unrelated = np.column_stack((
        _chirplet_source(frames, 681.73, 971.29, coefficients)[:, 0],
        _chirplet_source(frames, 1701.31, -527.13, coefficients)[:, 1],
    ))
    competing = base + _chirplet_source(frames, 1937.71, -381.47, coefficients[::-1], 0.31)
    # Both different laws independently have three complete globally scanned
    # 8192-frame supports. Neither merely wins by having more clean windows.
    ambiguous = np.zeros((36_864, 2))
    ambiguous[:16_384] = _chirplet_source(36_864, 751.13, 603.19, coefficients)[:16_384]
    ambiguous[20_480:] = _chirplet_source(36_864, 1821.37, -431.29, coefficients)[20_480:]
    results = {
        label: _refused(source, label)
        for label, source in (
            ("noise", np.random.default_rng(617_331).normal(0.0, 0.17, base.shape)),
            ("isolated_impulse", impulse),
            ("too_short_for_three_complete_windows", base[:16_383]),
            ("nonlinear_instantaneous_frequency", nonlinear),
            ("phase_jump_with_no_three_clean_same_gauge_supports", jumped),
            ("close_stationary_pair", pair),
            ("unrelated_stereo_chirps", unrelated),
            ("simultaneous_competing_chirps", competing),
            ("different_laws_each_with_three_clean_supports", ambiguous),
        )
    }
    ambiguous_result = results["different_laws_each_with_three_clean_supports"]
    assert ambiguous_result["rejection"] == "multiple_incompatible_supported_chirplets"
    assert len(ambiguous_result["candidates"]) == 2
    assert all(candidate["eligible"] for candidate in ambiguous_result["candidates"])
    assert [
        candidate["distinct_complete_support_windows"]
        for candidate in ambiguous_result["candidates"]
    ] == [3, 3]
    ambiguous_result.update({
        "first_law_clean_complete_window_starts": [0, 4096, 8192],
        "second_law_clean_complete_window_starts": [20_480, 24_576, 28_672],
        "neither_law_selected_by_longer_or_stronger_support": True,
    })
    return results


def _phase_algebra_control() -> dict:
    law = LinearSourcePhase(177.25, 1431.79, -773.41)
    positions = np.array([-0.75, 0.0, 177.25, 1327.5, 8191.0, 24_575.75])
    anchor = 391.75
    expected = (
        2.0
        * np.pi
        * (
            _cycles(positions, 177.25, 1431.79, -773.41)
            - _cycles(np.array([anchor]), 177.25, 1431.79, -773.41)[0]
        )
    )
    error = _error(law.phase_difference(positions, anchor), expected)
    cycles_error = _error(law.cycles(positions), _cycles(positions, 177.25, 1431.79, -773.41))
    assert max(error, cycles_error) < _LIMITS["integrated_phase_radians_max_abs_error"]
    return {
        "nonzero_fractional_epoch_frame": 177.25,
        "fixed_fractional_anchor_frame": anchor,
        "phase_difference_radians_max_abs_error": error,
        "integrated_cycles_max_abs_error": cycles_error,
        "phase_integrated_once_in_seconds": True,
    }


def _output_quadrature(actual: Real, coefficients: Complex, amplitude: float) -> Complex:
    stereo_map = np.column_stack((coefficients.real, -coefficients.imag))
    assert np.linalg.cond(stereo_map) < 10.0
    quadratures = np.linalg.solve(stereo_map, actual.T).T / amplitude
    return quadratures[:, 0] + 1.0j * quadratures[:, 1]


def _quadrature_frequency_measurement(
    measured_complex: Complex, seconds: Real, frequency: float, slope: float, h: float
) -> dict:
    center_seconds = float(np.mean(seconds))
    centered = seconds - center_seconds
    design = np.column_stack((np.ones(len(centered)), centered, 0.5 * centered**2))
    fitted = np.linalg.lstsq(
        design, np.unwrap(np.angle(measured_complex)) / (2.0 * np.pi), rcond=None
    )[0]
    expected_center_frequency = h * (frequency + slope * 2.0 * center_seconds)
    expected_output_slope = h * 2.0 * slope
    frequency_error = abs(float(fitted[1]) - expected_center_frequency)
    slope_error = abs(float(fitted[2]) - expected_output_slope)
    assert frequency_error < _LIMITS["estimated_frequency_hz_max_abs_error"]
    assert slope_error < _LIMITS["estimated_slope_hz_per_second_max_abs_error"]
    return {
        "measured_output_frequency_at_center_hz": float(fitted[1]),
        "expected_output_frequency_at_center_hz": expected_center_frequency,
        "measured_output_frequency_slope_hz_per_second": float(fitted[2]),
        "expected_output_frequency_slope_hz_per_second": expected_output_slope,
        "measured_frequency_and_slope_max_abs_errors": [frequency_error, slope_error],
    }


def _public_render_measurement(
    actual: Real, report: dict, coefficients: Complex, profile: tuple
) -> dict:
    frequency, slope, amplitude, source_frames = profile
    output_frames = report["capture_origin"] + np.arange(len(actual))
    interior = (2.0 * output_frames >= 2048) & (2.0 * output_frames < source_frames - 1024)
    seconds = output_frames[interior] / _FS
    h = 2.0 ** (7 / 12.0)
    expected_theta = 2.0 * np.pi * h * (frequency * seconds + 0.5 * slope * 2.0 * seconds**2)
    # Only this measurement projects actual output onto independently known
    # stereo quadratures. No such target reaches prepare or render.
    measured_complex = _output_quadrature(actual[interior], coefficients, amplitude)
    errors = {
        "actual_mixed_output_pcm_max_abs_error": _error(
            actual[interior],
            amplitude * (np.exp(1.0j * expected_theta)[:, None] * coefficients).real,
        ),
        "actual_mixed_output_independent_quadrature_vector_max_abs_error": _error(
            measured_complex, np.exp(1.0j * expected_theta)
        ),
    }
    assert max(errors.values()) < _LIMITS["global_gauge_coefficient_max_abs_error"]
    return {
        **errors,
        **_quadrature_frequency_measurement(measured_complex, seconds, frequency, slope, h),
        "actual_mixed_output_complete_supported_interior_frames": int(np.count_nonzero(interior)),
    }


def _public_prepare_render_control() -> dict:
    frequency, slope, amplitude = 1237.41, -1131.73, 0.37
    coefficients = _unit([0.57 * np.exp(-0.37j), 0.81 * np.exp(1.07j)])
    source = _chirplet_source(24_576, frequency, slope, coefficients, amplitude)
    prepared = prepare(
        source,
        certified_causal=True,
        affine_endpoint=True,
        predictive_envelope=True,
        current_weight=True,
        am_family=True,
        modulated_basis=True,
        linear_chirplet=True,
    )
    assert len(prepared.modes) == 1
    assert prepared.modes[0].phase_law is not None
    assert prepared.report["model_version"].endswith("v8")
    source_closure = _error(prepared.modes[0].analytic_pcm.real + prepared.residual, source)
    assert source_closure < _LIMITS["exact_complement_max_abs_error"]
    actual, report = render_prepared(prepared, 2.0, 7)
    return {
        "public_source_only_prepare_then_full_mixed_render": True,
        "independently_generated_frequency_and_slope": [frequency, slope],
        "rate_and_semitones": [2.0, 7],
        "prepared_source_exact_complement_max_abs_error": source_closure,
        **_public_render_measurement(
            actual, report, coefficients, (frequency, slope, amplitude, len(source))
        ),
        "target_used_only_for_independent_generated_control_and_post_render_measurement": True,
        "general_residual_or_full_matrix_quality_acceptance_claim": False,
    }


def _fit_basis() -> tuple[tuple, Complex]:
    laws = (
        LinearSourcePhase(73.25, 997.31, 1631.47),
        LinearSourcePhase(219.5, 2701.79, -917.13),
    )
    coefficients = np.stack((
        _unit([0.71 * np.exp(0.31j), 0.49 * np.exp(-1.09j)]),
        _unit([0.37 * np.exp(-0.73j), 0.83 * np.exp(0.81j)]),
    ))
    return laws, coefficients


def _fit(source: Real, laws: tuple, coefficients: Complex) -> tuple[Real, dict]:
    return fit_phase_current_weight_envelopes(
        source, np.array([law.frequency_hz for law in laws]), coefficients, laws
    )


def _fit_contract(envelopes: Real, report: dict, frames: int, carriers: int) -> None:
    assert envelopes.shape == (frames, carriers)
    assert np.isfinite(envelopes).all()
    assert np.all(envelopes >= 0.0)
    assert np.all(envelopes[:_BIRTH] == 0.0)
    assert report["window_frames"] == _WINDOW
    assert report["hop_frames"] == _HOP
    assert report["window_offsets_inclusive"] == [-255, 0]
    assert report["window_weight_formula"] == "0.5*(1-cos(pi*m/255)), m=0..255"
    assert report["current_source_weight"] == 1.0
    assert report["source_lookahead_frames"] == 0
    assert report["prediction_age_offsets_inclusive"] == [0, 63]
    assert report["last_active_source_frame_used_as_mask_or_knot"] is False
    assert report["fitted_decay_retained_through_finite_input"] is True
    assert report["complete_pipeline_causality_claim"] is False
    assert report["design_column_count"] == 2 * carriers


def _affine_envelope_controls() -> dict:
    laws, coefficients = _fit_basis()
    slopes = np.array([3.7e-5, -2.3e-5])
    gates = np.array([0.17, 0.29]) + (np.arange(2049) - _BIRTH)[:, None] * slopes
    gates[:_BIRTH] = 0.0
    source = _source(gates, laws, coefficients)
    fitted, report = _fit(source, laws, coefficients)
    _fit_contract(fitted, report, len(source), 2)
    assert report["rejected_tile_count"] == 0
    assert (len(source) - _CENTER) % _HOP == 0
    error = _error(fitted[_CENTER:], gates[_CENTER:])
    assert error < _LIMITS["physical_envelope_max_abs_error"]
    endpoint_error = 0.0
    for detail in report["first_knot_details"][4:]:
        center = detail["center_frame"]
        endpoint_error = max(
            endpoint_error,
            _error(np.asarray(detail["left_endpoints"]), gates[center - 255]),
            _error(np.asarray(detail["current_right_endpoints"]), gates[center]),
        )
    assert endpoint_error < _LIMITS["physical_envelope_max_abs_error"]
    return {
        "two_independent_known_chirplet_phase_laws": True,
        "source_certificate_required_in_this_declared_fixed_basis_control": False,
        "complete_supported_prediction_hops_inclusive": [_CENTER, len(source) - 1],
        "physical_positive_affine_gate_max_abs_error": error,
        "independent_physical_endpoint_max_abs_error": endpoint_error,
        "complementary_sum_max_abs_error": _complete_complement(source, fitted, laws, coefficients)[
            "closure"
        ],
    }


def _independent_gram_system(frames: int, laws: tuple, coefficients: Complex) -> dict:
    fraction = np.arange(_WINDOW, dtype=np.float64) / 255.0
    endpoints = np.column_stack((1.0 - fraction, fraction))
    weights = 0.5 * (1.0 - np.cos(np.pi * fraction))
    features = _independent_features(frames, laws, coefficients)[_CENTER - 255 : _CENTER + 1]
    # Build the two-endpoint/two-mode design independently; rows are original
    # sample then stereo channel, columns are each mode's left/right endpoint.
    design = (features[:, :, :, None] * endpoints[:, None, None, :]).transpose(0, 2, 1, 3)
    design = design.reshape(512, 4)
    row_weights = np.repeat(weights, 2)
    gram = design.T @ (row_weights[:, None] * design)
    assert np.linalg.cond(gram) < 1.0e6
    return {"design": design, "weights": row_weights, "gram": gram}


def _signed_gram_response(
    source: Real,
    laws: tuple,
    coefficients: Complex,
    baseline: Real,
    perturbation: Real,
    delta: Real,
    predicted_delta: Real,
    sign: float,
) -> dict:
    fitted, changed_report = _fit(source + sign * perturbation, laws, coefficients)
    detail = next(
        item for item in changed_report["first_knot_details"] if item["center_frame"] == _CENTER
    )
    actual_endpoints = np.column_stack((
        detail["left_endpoints"],
        detail["current_right_endpoints"],
    ))
    expected_endpoints = np.array([0.24, 0.19])[:, None] + sign * delta
    assert np.all(expected_endpoints > 0.0)
    assert np.all(actual_endpoints > 0.0)
    expected_prediction = np.array([0.24, 0.19])[None] + sign * predicted_delta
    assert np.all(expected_prediction > 0.0)
    errors = {
        "four_endpoints": _error(actual_endpoints, expected_endpoints),
        "whole_64_frame_two_mode_prediction": _error(
            fitted[_CENTER : _CENTER + _HOP], expected_prediction
        ),
        "signed_baseline_response": _error(
            fitted[_CENTER : _CENTER + _HOP] - baseline[_CENTER : _CENTER + _HOP],
            sign * predicted_delta,
        ),
    }
    assert max(errors.values()) < _LIMITS["independent_gram_prediction_max_abs_error"]
    return {"sign": sign, "maximum_absolute_errors": errors}


def _independent_gram_control() -> dict:
    laws, coefficients = _fit_basis()
    gates = np.zeros((1024, 2))
    gates[_BIRTH:] = [0.24, 0.19]
    source = _source(gates, laws, coefficients)
    baseline, report = _fit(source, laws, coefficients)
    _fit_contract(baseline, report, len(source), 2)
    system = _independent_gram_system(len(source), laws, coefficients)
    perturbation = np.zeros_like(source)
    perturbation[_CENTER] = [0.0007, -0.0009]
    delta = np.linalg.solve(
        system["gram"],
        system["design"].T
        @ (system["weights"] * perturbation[_CENTER - 255 : _CENTER + 1].reshape(-1)),
    ).reshape(2, 2)
    ages = np.arange(_HOP, dtype=np.float64) / 255.0
    predicted_delta = np.column_stack((-ages, 1.0 + ages)) @ delta.T
    return {
        "independent_full_stereo_four_column_weighted_gram": True,
        "gram_condition": float(np.linalg.cond(system["gram"])),
        "current_frame_only_original_pcm_cue": [0.0007, -0.0009],
        "signed_numeric_evidence": [
            _signed_gram_response(
                source, laws, coefficients, baseline, perturbation, delta, predicted_delta, sign
            )
            for sign in (-1.0, 1.0)
        ],
        "all_endpoints_and_whole_hop_predictions_strictly_positive": True,
        "nonlinear_active_set_or_general_mixture_sensitivity_claim": False,
    }


def _future_pair(end: int) -> dict:
    laws, coefficients = _fit_basis()
    gates = np.zeros((2049, 2))
    gates[_BIRTH:end] = [0.24, 0.16]
    source = _source(gates, laws, coefficients)
    changed = source.copy()
    changed[_EVENT] += [0.47, -0.31]
    first_envelopes, first_report = _fit(source, laws, coefficients)
    second_envelopes, second_report = _fit(changed, laws, coefficients)
    _fit_contract(first_envelopes, first_report, len(source), 2)
    _fit_contract(second_envelopes, second_report, len(source), 2)
    first = {
        "envelopes": first_envelopes,
        **_complete_complement(source, first_envelopes, laws, coefficients),
    }
    second = {
        "envelopes": second_envelopes,
        **_complete_complement(changed, second_envelopes, laws, coefficients),
    }
    assert np.array_equal(source[:_EVENT], changed[:_EVENT])
    for key in ("envelopes", "modes", "remainder"):
        assert np.array_equal(first[key][:_EVENT], second[key][:_EVENT])
    effect = _error(first_envelopes[_EVENT:], second_envelopes[_EVENT:])
    assert effect > _LIMITS["post_event_difference_minimum"]
    retained_decay = float(np.max(first_envelopes[end:]))
    assert retained_decay > _LIMITS["post_event_difference_minimum"]
    return {
        "original_activity_hulls_inclusive": [
            first_report["source_activity_hull_inclusive"],
            second_report["source_activity_hull_inclusive"],
        ],
        "same_fixed_phase_laws_and_stereo_gauge_for_both_sources": True,
        "prefix_envelopes_modes_and_exact_complements_bitwise_equal": True,
        "post_future_cue_envelope_max_abs_difference": effect,
        "nonzero_post_activity_prediction_retained": retained_decay,
        "all_source_and_complement_samples_retained": len(source),
        "complementary_sum_max_abs_error": max(first["closure"], second["closure"]),
    }


def _known_mode(
    *, frames: int = 1025, frequency: float = 1731.47, slope: float = 2371.13
) -> tuple[Mode, Real, Complex, Real]:
    epoch, birth = 83.25, 41
    clock = np.arange(frames, dtype=np.float64)
    gates = np.zeros(frames)
    gates[birth:] = 0.21 + 0.00003 * (clock[birth:] - birth)
    coefficients = _unit([0.63 * np.exp(0.71j), 0.82 * np.exp(-0.43j)])
    theta = 2.0 * np.pi * _cycles(clock, epoch, frequency, slope)
    analytic = gates[:, None] * np.exp(1.0j * theta)[:, None] * coefficients
    law = LinearSourcePhase(epoch, frequency, slope)
    return Mode(frequency, birth, analytic, law), analytic.real.copy(), coefficients, gates


def _integer_mode_expectation(
    mode: Mode,
    gates: Real,
    coefficients: Complex,
    details: dict,
    frames: int,
    rate: float,
    semitones: int,
) -> Real:
    law, birth = mode.phase_law, mode.birth
    output_frames = details["capture_origin"] + np.arange(frames)
    source_positions = rate * output_frames
    assert np.all(source_positions == np.round(source_positions))
    indices = source_positions.astype(np.int64)
    envelope = np.zeros(frames)
    inside = (indices >= 0) & (indices < len(gates))
    envelope[inside] = gates[indices[inside]]
    theta_birth = (
        2.0
        * np.pi
        * _cycles(np.array([birth]), law.epoch_frame, law.frequency_hz, law.slope_hz_per_second)[0]
    )
    frequency_birth = law.frequency_hz + law.slope_hz_per_second * (birth - law.epoch_frame) / _FS
    seconds = (output_frames - birth / rate) / _FS
    h = 2.0 ** (semitones / 12.0)
    theta = theta_birth + 2.0 * np.pi * h * (
        frequency_birth * seconds + 0.5 * law.slope_hz_per_second * rate * seconds**2
    )
    return envelope[:, None] * (coefficients * np.exp(1.0j * theta)[:, None]).real


def _independent_linear(source: Real, positions: Real) -> Real:
    output = np.zeros((len(positions), 2))
    lower = np.floor(positions).astype(np.int64)
    fraction = positions - lower
    for neighbor, weights in ((lower, 1.0 - fraction), (lower + 1, fraction)):
        inside = (neighbor >= 0) & (neighbor < len(source))
        output[inside] += weights[inside, None] * source[neighbor[inside]]
    return output


def _integer_renderer_controls(
    mode: Mode, source: Real, coefficients: Complex, gates: Real
) -> list:
    prepared = Prepared(source, (mode,), np.zeros_like(source), {})
    integer = []
    for semitones in (-7, 0, 7):
        actual, report = render_prepared(prepared, 2.0, semitones)
        expected = _integer_mode_expectation(
            mode, gates, coefficients, report, len(actual), 2.0, semitones
        )
        error = _error(actual, expected)
        assert error < _LIMITS["render_integer_phase_max_abs_error"]
        integer.append({
            "rate": 2.0,
            "semitones": semitones,
            "maximum_absolute_error": error,
            "expected_output_frequency_slope_hz_per_second": 2.0 ** (semitones / 12.0)
            * 2.0
            * mode.phase_law.slope_hz_per_second,
        })
    return integer


def _nonunity_residual_tail_control(
    mixed: Prepared, mode: Mode, coefficients: Complex, gates: Real
) -> dict:
    actual, report = render_prepared(mixed, 2.0, 7)
    transformed_residual, residual_report = render_residual(mixed.residual, 2.0, 7)
    assert len(actual) == len(transformed_residual)
    assert report["capture_origin"] == residual_report["capture_origin"]
    expected_mode = _integer_mode_expectation(
        mode, gates, coefficients, report, len(actual), 2.0, 7
    )
    all_tail_error = _error(actual, expected_mode + transformed_residual)
    assert all_tail_error < _LIMITS["render_integer_phase_max_abs_error"]
    nominal = math.ceil(len(mixed.source) / 2.0)
    tail_energy = float(np.sum(transformed_residual[nominal:] ** 2))
    assert len(actual) > nominal
    assert tail_energy > 1.0e-12
    return {
        "complete_nonzero_residual_transform_max_abs_error": all_tail_error,
        "nonunity_generated_residual_tail_energy_beyond_nominal_duration": tail_energy,
    }


def _fractional_identity_controls(mixed: Prepared) -> list:
    fractional = []
    for semitones in (-7, 7):
        rate = 2.0 ** (semitones / 12.0)
        actual, details = render_prepared(mixed, rate, semitones)
        positions = rate * (details["capture_origin"] + np.arange(len(actual)))
        expected = _independent_linear(mixed.source, positions)
        error = _error(actual, expected)
        assert error < _LIMITS["complete_fractional_identity_max_abs_error"]
        assert np.all(expected[(positions < -1.0) | (positions > len(mixed.source))] == 0.0)
        fractional.append({
            "rate_equals_pitch_ratio": rate,
            "semitones": semitones,
            "full_generated_output_frames": len(actual),
            "capture_origin": details["capture_origin"],
            "all_samples_and_zero_extensions_max_abs_error": error,
        })
    return fractional


def _fractional_interpolation_error(
    mode: Mode, source: Real, coefficients: Complex, gates: Real
) -> float:
    # Fractional sampling of the analytic carrier is an interpolation approximation.
    # Report it separately; it has no continuous-phase exactness assertion.
    actual, details = render_prepared(Prepared(source, (mode,), np.zeros_like(source), {}), 0.75, 7)
    positions = 0.75 * (details["capture_origin"] + np.arange(len(actual)))
    analytic_ideal = np.zeros_like(actual)
    inside = (positions >= mode.birth) & (positions <= len(gates) - 1)
    s = positions[inside]
    law = mode.phase_law
    source_cycles = _cycles(s, law.epoch_frame, law.frequency_hz, law.slope_hz_per_second)
    birth_cycles = _cycles(
        np.array([mode.birth]), law.epoch_frame, law.frequency_hz, law.slope_hz_per_second
    )[0]
    total = source_cycles + (2.0 ** (7 / 12.0) / 0.75 - 1.0) * (source_cycles - birth_cycles)
    ideal_gate = 0.21 + 0.00003 * (s - mode.birth)
    analytic_ideal[inside] = (
        ideal_gate[:, None] * (coefficients * np.exp(2.0j * np.pi * total)[:, None]).real
    )
    return _error(actual, analytic_ideal)


def _renderer_controls() -> dict:
    mode, source, coefficients, gates = _known_mode()
    residual = np.zeros_like(source)
    residual[0], residual[-1] = [0.11, -0.07], [-0.19, 0.13]
    residual[173:797] = np.random.default_rng(818_733).normal(0.0, 0.013, (624, 2))
    mixed = Prepared(source + residual, (mode,), residual, {})
    return {
        "known_mode_integer_positions_integrated_phase": _integer_renderer_controls(
            mode, source, coefficients, gates
        ),
        "output_frequency_formula": "h*f(r*n); output frequency slope h*r*alpha",
        "fixed_birth_including_nonzero_epoch_and_stereo_coefficient_gauge": True,
        **_nonunity_residual_tail_control(mixed, mode, coefficients, gates),
        "fractional_h_equals_r_complete_source_identities": _fractional_identity_controls(mixed),
        "fractional_interpolation_vs_ideal_max_abs_error": _fractional_interpolation_error(
            mode, source, coefficients, gates
        ),
        "fractional_interpolation_measured_rate_and_semitones": [0.75, 7],
        "fractional_continuous_ideal_exactness_or_acoustic_acceptance_claim": False,
    }


def _expect_value_error(callback: Callable, label: str) -> str:
    try:
        callback()
    except ValueError:
        return label
    message = f"Invalid chirplet contract accepted: {label}"
    raise AssertionError(message)


def _renderer_refusals() -> list[str]:
    refused = []
    # The final nonzero sample is deliberately tiny: no threshold or crop may
    # discard a known crossing. A separate case crosses only on interpolation support.
    for label, frequency, slope, semitones in (
        ("upper_band_crossing_in_nonzero_fitted_support", 15_500.0, 100_000.0, 7),
        ("falling_frequency_crosses_zero", 17.0, -1500.0, 0),
        ("upper_band_crossing_only_on_interpolation_support", 23_499.75, 24_000.0, 0),
    ):
        frames = 1001
        law = LinearSourcePhase(0.0, frequency, slope)
        analytic = np.zeros((frames, 2), dtype=np.complex128)
        analytic[41] = [0.2, 0.1j]
        analytic[-1] = [1.0e-30, 0.0]
        mode = Mode(frequency, 41, analytic, law)
        prepared = Prepared(analytic.real, (mode,), np.zeros((frames, 2)), {})
        try:
            render_prepared(prepared, 0.5, semitones)
        except UnsupportedPitchDomainError:
            refused.append(label)
        else:
            message = f"Known full-support frequency crossing accepted: {label}"
            raise AssertionError(message)
    return refused


def _validation_controls() -> dict:
    laws, coefficients = _fit_basis()
    frequencies = np.array([law.frequency_hz for law in laws])
    source = np.zeros((512, 2))
    refused = []
    for label, values in (
        ("nonfinite_epoch", (float("nan"), 997.31, 1.0)),
        ("nonfinite_epoch_frequency", (0.0, float("inf"), 1.0)),
        ("nonfinite_frequency_slope", (0.0, 997.31, float("nan"))),
    ):
        refused.append(_expect_value_error(lambda values=values: LinearSourcePhase(*values), label))
    for label, invalid in (
        ("phase_law_count_mismatch", laws[:1]),
        ("phase_law_type_mismatch", (None, laws[1])),
        ("phase_epoch_frequency_mismatch", (LinearSourcePhase(0.0, 998.31, 1.0), laws[1])),
    ):
        refused.append(
            _expect_value_error(
                lambda invalid=invalid: fit_phase_current_weight_envelopes(
                    source, frequencies, coefficients, invalid
                ),
                label,
            )
        )
    _, ordinary = fit_current_weight_causal_envelopes(source, frequencies, coefficients)
    _, integrated = _fit(source, laws, coefficients)
    extra = integrated["estimated_peak_working_bytes"] - ordinary["estimated_peak_working_bytes"]
    independently_expected_extra = len(source) * 16 + _WINDOW * 32 * len(laws) + 4096
    assert extra == independently_expected_extra
    # A read-only broadcast input costs no giant source allocation. Its declared
    # conservative phase workspace must refuse before creating fitted output.
    huge_zero_view = np.broadcast_to(np.zeros((1, 2)), (2_400_000, 2))
    old_zero_envelopes, old_large_report = fit_current_weight_causal_envelopes(
        huge_zero_view, frequencies, coefficients
    )
    assert old_zero_envelopes.shape == (len(huge_zero_view), 2)
    assert np.all(old_zero_envelopes == 0.0)
    old_large_bytes = old_large_report["estimated_peak_working_bytes"]
    assert old_large_bytes < 256 * 1024 * 1024
    del old_zero_envelopes
    refused.append(
        _expect_value_error(
            lambda: _fit(huge_zero_view, laws, coefficients), "phase_fit_workspace_limit"
        )
    )
    return {
        "invalid_contracts_refused_before_silence_shortcut": refused,
        "additional_phase_workspace_bytes_on_512_frames_two_modes": extra,
        "same_large_input_accepted_by_unchanged_stationary_basis_estimated_bytes": old_large_bytes,
        "preallocation_large_input_is_broadcast_zero_view": True,
    }


def integrity_controls() -> dict:
    """Assert independent source law, envelope, domain and full-response algebra."""
    return {
        "stage": "source_linear_chirplet_integrity_controls",
        "predeclared_numeric_acceptance_limits": dict(_LIMITS),
        "independent_unseen_source_estimation": _unseen_source_controls(),
        "unsupported_and_ambiguous_source_refusals": _negative_source_controls(),
        "declared_epoch_integrated_phase_algebra": _phase_algebra_control(),
        "public_unseen_source_preparation_and_output_phase": _public_prepare_render_control(),
        "known_phase_positive_affine_whole_hops": _affine_envelope_controls(),
        "independent_two_mode_weighted_gram_and_signed_pcm_cue": _independent_gram_control(),
        "fixed_phase_future_cue_local_causality": {
            "future_event_frame": _EVENT,
            "same_source_activity_hull": _future_pair(1536),
            "different_source_activity_hulls": _future_pair(800),
            "global_source_discovery_or_full_preparation_causality_claim": False,
        },
        "known_phase_renderer_and_full_complement": _renderer_controls(),
        "complete_fitted_and_interpolation_support_domain_refusals": _renderer_refusals(),
        "malformed_contract_and_preallocation_refusals": _validation_controls(),
        "source_truth_in_generation_and_independent_expectations_only": True,
        "private_estimator_helpers_fixture_tags_or_output_targets_used": False,
        "general_mixture_variable_rate_native_realtime_or_listening_acceptance_claim": False,
    }
