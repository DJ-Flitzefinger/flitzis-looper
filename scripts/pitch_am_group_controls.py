"""Independent generated-source controls for source-only positive-AM grouping.

Oracle coefficients construct controls and expectations only. The public
certificate receives original PCM and an explicit stationary CarrierEstimate,
without targets, requested pitch/rate, gate locations, or scorer information.
No source estimator, renderer, Native module, device, or file output is run here.
"""

from typing import TYPE_CHECKING

import numpy as np
from pitch_am_groups import group_am_carriers
from pitch_mode_carriers import (
    FS,
    MAX_COEFFICIENT_DISTANCE,
    MIN_SUPPORT_WINDOWS,
    WINDOW_FRAMES,
    CarrierEstimate,
)

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]

_FRAMES = 3 * WINDOW_FRAMES
_CLOCK = np.arange(_FRAMES, dtype=np.float64) / FS


def _estimate(frequencies: Real, coefficients: Complex) -> CarrierEstimate:
    norms = np.linalg.norm(coefficients, axis=1)
    return CarrierEstimate(
        tuple(float(value) for value in frequencies),
        coefficients / norms[:, None],
        {
            "method": "explicit_stationary_control_coefficients",
            "admitted_carrier_count": len(norms),
        },
    )


def _source(frequencies: Real, coefficients: Complex) -> Real:
    # Direct real sine/cosine sums do not use the certificate's complex fit.
    source = np.zeros((_FRAMES, 2))
    for frequency, vector in zip(frequencies, coefficients, strict=True):
        angle = 2.0 * np.pi * frequency * _CLOCK
        source += np.cos(angle)[:, None] * vector.real
        source -= np.sin(angle)[:, None] * vector.imag
    return source


def _family(
    frequency: float, spacing: float, depth: float, phase: float, vector: Complex
) -> tuple[Real, Complex, Real, float]:
    frequencies = np.array([frequency - spacing, frequency, frequency + spacing])
    u = depth * np.exp(1j * phase)
    coefficients = np.array([np.conj(u) * vector, vector, u * vector])
    carrier = np.column_stack([
        abs(value) * np.cos(2.0 * np.pi * frequency * _CLOCK + np.angle(value)) for value in vector
    ])
    modulator = 1.0 + 2.0 * depth * np.cos(2.0 * np.pi * spacing * _CLOCK + phase)
    direct_am = modulator[:, None] * carrier
    factorization_error = float(np.max(np.abs(direct_am - _source(frequencies, coefficients))))
    return frequencies, coefficients, direct_am, factorization_error


def _unchanged(result: CarrierEstimate, original: CarrierEstimate) -> bool:
    return result.frequencies_hz == original.frequencies_hz and np.array_equal(
        result.stereo_coefficients, original.stereo_coefficients
    )


def _positive_case(
    frequency: float, spacing: float, depth: float, phase: float, vector: Complex
) -> tuple[dict, float]:
    frequencies, coefficients, source, error = _family(frequency, spacing, depth, phase, vector)
    before = source.copy()
    estimate = _estimate(frequencies, coefficients)
    original_coefficients = estimate.stereo_coefficients.copy()
    result = group_am_carriers(source, estimate)
    groups = result.report["am_grouping"]
    direction_error = float(
        np.linalg.norm(result.stereo_coefficients[0] - vector / np.linalg.norm(vector))
    )
    assert result.frequencies_hz == (frequency,)
    assert groups["group_count"] == 1
    assert groups["complete_source_windows"] >= MIN_SUPPORT_WINDOWS
    assert groups["candidates"][0]["support_windows"] == groups["complete_source_windows"]
    assert direction_error < 1.0e-10
    assert error < 1.0e-10
    assert np.array_equal(source, before)
    assert np.array_equal(estimate.stereo_coefficients, original_coefficients)
    assert "am_grouping" not in estimate.report
    return {
        "center_hz": frequency,
        "source_spacing_hz": spacing,
        "declared_positive_margin": 1.0 - 2.0 * depth,
        "independent_full_PCM_factorization_max_error": error,
        "center_direction_error": direction_error,
        "support_windows": groups["complete_source_windows"],
        "source_and_original_estimate_unchanged": True,
    }, max(error, direction_error)


def _positive_controls() -> tuple[list[dict], float]:
    reports, maximum_error = [], 0.0
    for parameters in (
        (643.29, 17.83, 0.17, 0.63, np.array([0.19 * np.exp(0.19j), 0.11 * np.exp(-0.77j)])),
        (1037.51, 31.27, 0.31, -0.41, np.array([0.13 * np.exp(-1.11j), 0.21 * np.exp(0.87j)])),
        (1709.23, 57.11, 0.09, 1.73, np.array([0.23 * np.exp(0.43j), -0.07j])),
    ):
        report, error = _positive_case(*parameters)
        reports.append(report)
        maximum_error = max(maximum_error, error)
    return reports, maximum_error


def _refusal_cases() -> list[tuple[str, Real, Complex]]:
    vector = np.array([0.17 * np.exp(0.33j), 0.09 * np.exp(-0.91j)])
    frequencies, coefficients, _, _ = _family(879.37, 29.83, 0.22, -0.67, vector)
    cases = []
    asymmetric = frequencies.copy()
    asymmetric[0] -= 0.41
    cases.append(("asymmetric_spacing", asymmetric, coefficients.copy()))
    unmatched = coefficients.copy()
    unmatched[2] *= np.exp(0.29j)
    cases.append(("unmatched_sideband_phase", frequencies.copy(), unmatched))
    unequal = coefficients.copy()
    unequal[2, 1] *= 1.3
    cases.append(("unequal_channel_ratios", frequencies.copy(), unequal))
    beyond_tolerance = coefficients.copy()
    beyond_tolerance[2] *= np.exp(2j * MAX_COEFFICIENT_DISTANCE)
    cases.append((
        "conjugacy_beyond_existing_coherence_limit",
        frequencies.copy(),
        beyond_tolerance,
    ))
    negative_frequencies, negative_coefficients, _, _ = _family(1231.71, 43.79, 0.63, 0.37, vector)
    cases.append(("nonpositive_envelope", negative_frequencies, negative_coefficients))
    return cases


def _refusal_controls() -> list[dict]:
    reports = []
    for name, original_frequencies, original_coefficients in _refusal_cases():
        source = _source(original_frequencies, original_coefficients)
        estimate = _estimate(original_frequencies, original_coefficients)
        result = group_am_carriers(source, estimate)
        report = result.report["am_grouping"]
        assert _unchanged(result, estimate)
        assert report["group_count"] == 0
        assert report["complete_source_windows"] >= MIN_SUPPORT_WINDOWS
        assert report["candidates"][0]["refusal_counts"]
        reports.append({
            "source_kind": name,
            "original_carriers_preserved_bitwise": True,
            "source_complete_windows": report["complete_source_windows"],
            "refusal_counts": report["candidates"][0]["refusal_counts"],
        })
    return reports


def _continuing_family_source(
    frequency: float, spacing: float, depth: float, phase: float, vector: Complex
) -> Real:
    clock = _CLOCK + _FRAMES / FS
    carrier = np.column_stack([
        abs(value) * np.cos(2.0 * np.pi * frequency * clock + np.angle(value)) for value in vector
    ])
    modulator = 1.0 + 2.0 * depth * np.cos(2.0 * np.pi * spacing * clock + phase)
    return modulator[:, None] * carrier


def _changing_family_case(
    name: str,
    parameters: tuple[float, float, Complex],
    first: tuple[Real, Complex, Real, float],
) -> dict:
    later = _family(827.39, 34.71, *parameters)
    assert np.array_equal(first[0], later[0])
    # _source's local clock starts at zero; advance each line by the actual
    # segment start to preserve global carrier and modulator phase coordinates.
    continuing_coefficients = later[1] * np.exp(2j * np.pi * later[0] * _FRAMES / FS)[:, None]
    continuation = _source(later[0], continuing_coefficients)
    error = float(
        np.max(np.abs(continuation - _continuing_family_source(827.39, 34.71, *parameters)))
    )
    assert error < 1.0e-10
    assert 1.0 - 2.0 * parameters[0] > 0.0
    estimate = _estimate(first[0], first[1])
    plateaus = (
        group_am_carriers(first[2], estimate),
        group_am_carriers(continuation, _estimate(later[0], continuing_coefficients)),
    )
    assert all(value.report["am_grouping"]["group_count"] == 1 for value in plateaus)
    result = group_am_carriers(np.vstack((first[2], continuation)), estimate)
    report = result.report["am_grouping"]
    candidate = report["candidates"][0]
    assert candidate["support_windows"] >= MIN_SUPPORT_WINDOWS
    assert candidate["refusal_counts"]
    assert report["complete_source_windows"] > candidate["support_windows"]
    assert report["group_count"] == 0
    assert _unchanged(result, estimate)
    return {
        "source_kind": name,
        "frames_per_stationary_plateau": _FRAMES,
        "each_plateau_individually_positive_AM_certified": True,
        "independent_global_clock_continuation_PCM_max_error": error,
        "initial_supported_windows_before_later_refusal": candidate["support_windows"],
        "complete_source_windows": report["complete_source_windows"],
        "refusal_counts": candidate["refusal_counts"],
        "original_carriers_preserved_bitwise": True,
        "source_only_no_rendered_output": True,
    }


def _changing_family_controls() -> list[dict]:
    vector = np.array([0.17 * np.exp(0.33j), 0.09 * np.exp(-0.91j)])
    first = _family(827.39, 34.71, 0.18, 0.47, vector)
    assert 1.0 - 2.0 * 0.18 > 0.0
    return [
        _changing_family_case(name, parameters, first)
        for name, parameters in (
            ("later_global_modulator_phase_change", (0.18, 1.1, vector)),
            ("later_modulation_depth_change", (0.31, 0.47, vector)),
            ("later_common_center_stereo_phase_change", (0.18, 0.47, vector * np.exp(0.59j))),
        )
    ]


def _ambiguous_control() -> dict:
    frequencies = 1217.39 + 37.13 * np.arange(-2, 3)
    vector = np.array([0.17 * np.exp(0.41j), 0.12 * np.exp(-0.93j)])
    # The two positive-AM triples share their middle spectral line as a sideband.
    coefficients = np.array([0.15, 1.0, 0.15, 1.0, 0.15])[:, None] * vector
    estimate = _estimate(frequencies, coefficients)
    result = group_am_carriers(_source(frequencies, coefficients), estimate)
    report = result.report["am_grouping"]
    assert _unchanged(result, estimate)
    assert report["certified_candidate_count"] >= 2
    assert report["ambiguous_candidate_count"] == report["certified_candidate_count"]
    assert report["group_count"] == 0
    return {
        "certified_overlapping_candidates": report["certified_candidate_count"],
        "sideband_shared_by_two_positive_AM_families": True,
        "all_ambiguous_carriers_preserved_bitwise": True,
        "no_greedy_or_output_fitted_group_choice": True,
    }


def _disjoint_control() -> dict:
    first = _family(
        611.29, 18.63, 0.14, 0.31, np.array([0.13 * np.exp(0.71j), 0.08 * np.exp(-0.43j)])
    )
    second = _family(
        1537.87, 41.13, 0.21, -0.57, np.array([0.09 * np.exp(-0.29j), 0.14 * np.exp(1.13j)])
    )
    frequencies = np.concatenate((first[0], second[0]))
    coefficients = np.concatenate((first[1], second[1]))
    estimate = _estimate(frequencies, coefficients)
    result = group_am_carriers(first[2] + second[2], estimate)
    assert result.frequencies_hz == (611.29, 1537.87)
    assert result.report["am_grouping"]["group_count"] == 2
    error = max(
        float(
            np.linalg.norm(
                result.stereo_coefficients[row]
                - coefficients[index] / np.linalg.norm(coefficients[index])
            )
        )
        for row, index in enumerate((1, 4))
    )
    assert error < 1.0e-10
    return {"two_disjoint_groups_admitted": True, "maximum_center_direction_error": error}


def _small_controls() -> dict:
    source = np.zeros((WINDOW_FRAMES, 2))
    empty = CarrierEstimate((), np.zeros((0, 2), dtype=np.complex128), {})
    assert _unchanged(group_am_carriers(source, empty), empty)
    frequencies, coefficients, _, _ = _family(
        717.23, 27.41, 0.17, 0.39, np.array([0.11 + 0.07j, 0.08 - 0.03j])
    )
    estimate = _estimate(frequencies, coefficients)
    silence = group_am_carriers(source, estimate)
    short = group_am_carriers(_source(frequencies, coefficients)[:WINDOW_FRAMES], estimate)
    assert _unchanged(silence, estimate)
    assert _unchanged(short, estimate)
    assert short.report["am_grouping"]["complete_source_windows"] == 1
    return {"empty_silent_and_insufficient_support_preserved": True}


def _invalid_controls() -> dict:
    source = np.zeros((17, 2))
    unit = np.array([[1.0 + 0.0j, 0.0j]])
    cases = [
        CarrierEstimate((FS / 2,), unit, {}),
        CarrierEstimate((float("nan"),), unit, {}),
        CarrierEstimate((-17.0,), unit, {}),
        CarrierEstimate(np.array([[713.29]]), unit, {}),
        CarrierEstimate((713.29,), 2.0 * unit, {}),
        CarrierEstimate((713.29,), unit.real, {}),
        CarrierEstimate((713.29,), np.array([[complex("nan"), 0.0j]]), {}),
    ]
    for estimate in cases:
        try:
            group_am_carriers(source, estimate)
        except ValueError:
            pass
        else:
            message = "Malformed carrier contract was accepted"
            raise AssertionError(message)
    return {"invalid_frequency_and_vector_contracts_rejected": len(cases)}


def integrity_controls() -> dict:
    """Check actual public certificates against independent original equations."""
    positive, maximum_error = _positive_controls()
    return {
        "stage": "positive_AM_group_source_certificate_controls",
        "passed": True,
        "positive_sources": positive,
        "maximum_independent_factorization_or_direction_error": maximum_error,
        "negative_sources": _refusal_controls(),
        "changing_stationary_families": _changing_family_controls(),
        "overlap_ambiguity": _ambiguous_control(),
        "disjoint_groups": _disjoint_control(),
        **_small_controls(),
        **_invalid_controls(),
        "oracle_parameters_only_in_control_construction_and_expectations": True,
        "renderer_or_estimator_or_native_used": False,
        "overall_acoustic_acceptance": False,
    }
