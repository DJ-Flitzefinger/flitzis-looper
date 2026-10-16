"""Independent multiple-feasible finite-AM source controls, without output targets."""

import math
import time

import numpy as np
from pitch_causal_model_probe import emit
from pitch_finite_am_gate import fit_finite_am_gate
from pitch_source_modulation import SourceModulator

_FRAMES, _FS, _REFERENCE = 32768, 48000, 8192
_TRUE_GATE = (2048, 8, 28456, 8)
_ALTERNATIVE_GATE = (2046, 12, 28458, 12)
_BOUNDARY_GATE = (2049, 3, 28456, 8)
_RMS_LIMIT = 1.0e-4


def _gate(positions: np.ndarray, fields: tuple[int, int, int, int]) -> np.ndarray:
    """Evaluate independent sine-squared equations, without the fitter's gate type."""
    onset, rise, end, release = fields
    left = np.clip((positions - onset) / rise, 0.0, 1.0)
    right = np.clip((end - positions) / release, 0.0, 1.0)
    return np.sin(0.5 * np.pi * left) ** 2 * np.sin(0.5 * np.pi * right) ** 2


def _source() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, complex]:
    positions = np.arange(_FRAMES, dtype=np.float64)
    modulation = 1.0 - 0.999998 * np.cos(2.0 * np.pi * (positions - 2052) / 480)
    envelope = _gate(positions, _TRUE_GATE) * modulation
    coefficients = np.array([1.0, 1.0j]) / np.sqrt(2.0)
    multiplier = 0.287 * np.exp(-0.61j)
    phase = 2.0 * np.pi * 1000 * positions / _FS
    direction = coefficients * multiplier
    carrier = np.cos(phase)[:, None] * direction.real - np.sin(phase)[:, None] * direction.imag
    return envelope[:, None] * carrier, positions, modulation, coefficients, multiplier


def _independent_model(positions: np.ndarray, modulation: np.ndarray, fields: tuple) -> dict:
    """Use the pointwise isotropic I/Q Gram to solve the global scalar fit exactly."""
    truth, alternative = _gate(positions, _TRUE_GATE), _gate(positions, fields)
    weights = modulation * modulation
    scale = float(np.sum(truth * alternative * weights) / np.sum(alternative**2 * weights))
    error = (alternative * scale - truth) * modulation
    target = truth * modulation
    observations = {
        "global": np.ones(_FRAMES, dtype=bool),
        "onset_edge": np.abs(positions - 2049) <= 512,
        "release_edge": np.abs(positions - 28455) <= 512,
    }
    residuals = {
        name: float(np.sqrt(np.sum(error[mask] ** 2) / np.sum(target[mask] ** 2)))
        for name, mask in observations.items()
    }
    assert max(residuals.values()) <= _RMS_LIMIT
    return {
        "gate_onset_rise_end_release": list(fields),
        "global_least_squares_multiplier_scale": scale,
        "independent_global_and_each_edge_relative_RMS": residuals,
    }


def independent_feasibility_control() -> dict:
    """Prove source-only alternatives independently before calling the search."""
    source, positions, modulation, _, _ = _source()
    active = np.flatnonzero(np.any(source != 0.0, axis=1))
    assert active[[0, -1]].tolist() == [2049, 28455]
    # Every possible source-grid model is zero outside U and identically one on P.
    # P contains 52 complete AM periods; U fits within 58 periods. Nonnegative,
    # 480-periodic m**2 and pointwise F'F=(m**2/2)I bound their Gram ratio by58/52.
    union_length = (28455 + 511) - (2049 - 511) + 1
    common_plateau_length = (28455 - 512) - (2049 + 512) + 1
    upper_periods = math.ceil(union_length / 480)
    lower_periods = common_plateau_length // 480
    multiplier_bound = (
        2.0 * _RMS_LIMIT / (1.0 - _RMS_LIMIT) * math.sqrt(upper_periods / lower_periods)
    )
    assert (upper_periods, lower_periods) == (58, 52)
    assert multiplier_bound < 1.0e-3
    return {
        "source_frames": _FRAMES,
        "carrier_hz": 1000.0,
        "modulation_hz": 100.0,
        "modulation_formula": "1-.999998*cos(2*pi*(n-2052)/480)",
        "stereo_coefficients": "(1,i)/sqrt(2)",
        "global_multiplier": ".287*exp(-.61i)",
        "declared_true_gate_onset_rise_end_release": list(_TRUE_GATE),
        "observed_first_last_nonzero_frames": active[[0, -1]].tolist(),
        "alternative_global_LS_source_fits": [
            _independent_model(positions, modulation, fields)
            for fields in (_ALTERNATIVE_GATE, _BOUNDARY_GATE)
        ],
        "two_declared_models_corresponding_landmark_distance_frames": 2,
        "unchanged_global_and_each_edge_RMS_limit": _RMS_LIMIT,
        "all_feasible_complex_multiplier_distance_upper_bound": multiplier_bound,
        "bound_formula": "2*epsilon/(1-epsilon)*sqrt(58/52)",
        "bound_requires_this_quadrature_stereo_and_480_periodic_AM": True,
        "complex_multiplier_threshold_crossing_demonstrated": False,
        "fitter_private_helpers_or_output_targets_used": False,
    }


def verify_landmark_search(report: dict) -> dict:
    """Check complete candidate accounting, retaining primary-refusal limitations."""
    assert not report["accepted"]
    assert report["search_complete"]
    assert not report["incomplete_search"]
    assert report["relative_source_rms_limit"] == _RMS_LIMIT
    survivor_pairs = math.prod(report["edge_survivor_counts"].values())
    assert report["paired_evaluations"] == survivor_pairs
    assert report["requested_paired_evaluations"] == survivor_pairs
    assert report["feasible_model_pairs"] >= 2
    assert report["boundary_unresolved"]
    assert report["refusal_reason"] == "feasible_set_touches_search_boundary"
    ranges = report["landmark_ranges"]
    landmarks = {
        "onset_frame": (2046, 2048, 2049),
        "rise_end_frame": (2058, 2056, 2052),
        "release_start_frame": (28446, 28448, 28448),
        "end_frame": (28458, 28456, 28456),
    }
    for name, values in landmarks.items():
        assert ranges[name]["min"] <= min(values)
        assert ranges[name]["max"] >= max(values)
    assert any(ranges[name]["max"] - ranges[name]["min"] > 1 for name in landmarks)
    assert report["maximum_normalized_complex_multiplier_distance"] is None
    assert report["all_feasible_designs_passed_condition"]
    assert not report["all_feasible_jacobians_passed"]
    return {
        "refusal_reason": report["refusal_reason"],
        "search_complete": report["search_complete"],
        "edge_candidate_counts": report["edge_candidate_counts"],
        "edge_survivor_counts": report["edge_survivor_counts"],
        "paired_evaluations": report["paired_evaluations"],
        "feasible_model_pairs": report["feasible_model_pairs"],
        "landmark_ranges": ranges,
        "all_feasible_designs_passed_condition": True,
        "all_feasible_jacobians_passed": False,
        "maximum_normalized_complex_multiplier_distance": None,
        "boundary_refusal_priority_masks_landmark_refusal_reason": True,
        "isolated_landmark_or_Jacobian_refusal_branch_coverage_claimed": False,
        "complex_multiplier_ambiguity_refusal_coverage_claimed": False,
        "cap_exhaustion_coverage_claimed": False,
    }


def landmark_ambiguity_control() -> dict:
    """Run the unchanged exhaustive fitter after independent alternative fits."""
    independent = independent_feasibility_control()
    source, _, _, coefficients, _ = _source()
    original = source.copy()
    modulator = SourceModulator(100.0, -0.499999 * np.exp(-2j * np.pi * 100 * 2052 / _FS))
    started = time.perf_counter()
    fitted = fit_finite_am_gate(source, 1000.0, coefficients, modulator, _REFERENCE)
    assert fitted.gate is None
    assert fitted.multiplier == 0.0j
    assert np.array_equal(source, original)
    return {
        "stage": "independent_multiple_feasible_finite_AM_landmarks",
        "passed": True,
        "independent_source_models": independent,
        "complete_unchanged_search": verify_landmark_search(fitted.report),
        "elapsed_seconds": time.perf_counter() - started,
        "source_preserved_bitwise": True,
        "production_or_remaining_finite_gate_acceptance": False,
    }


if __name__ == "__main__":
    emit(landmark_ambiguity_control())
