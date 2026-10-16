"""Exact-model AM gauge bounds and separate computed-basis checks, never admission."""

import math

import numpy as np
from pitch_causal_model_probe import emit
from pitch_mode_carriers import FS, MAX_FREQUENCY_HZ, MIN_FREQUENCY_HZ, WINDOW_FRAMES
from pitch_source_modulation import SourceModulator, modulation_values

_EPSILON = 1.0e-4
_EXTRA_SUPPORT = 1024


def theorem_limits() -> dict:
    """Evaluate outward constants for the separately documented exact-model proof."""
    assert (FS, MIN_FREQUENCY_HZ, MAX_FREQUENCY_HZ, WINDOW_FRAMES) == (48000, 20, 23520, 8192)
    assert 1.0 / math.sin(math.pi * 40 / FS) < 383
    assert 1.0 / math.sin(math.pi * 20 / FS) < 765
    assert 1.0 / math.sin(math.pi * 9 / FS) < 1700
    assert 1.0 / math.sin(math.pi * 18 / FS) < 850
    error = 1.5 * 383 + 2 * 765 + 2 * 1700 + 0.5 * 850
    assert error < 5930
    fast_k_squared = (2 * (8192 + _EXTRA_SUPPORT) + 5930) / (8192 - 5930)
    fast_bound = 2 * _EPSILON / (1 - _EPSILON) * math.sqrt(fast_k_squared)
    denominator = 2 * math.sin(2 * math.pi * 20 / FS)
    short_rho = 32 / ((8192 - 17) * denominator)
    long_rho = 6 / ((3 * (FS / 9) / 8 - 4) * denominator)
    extension = (1 + _EXTRA_SUPPORT / (8192 - 24)) ** 8
    assert max(short_rho, long_rho) < 0.748
    assert extension < 2.573
    slow_k_squared = 2 * 2.573 / (1 - 0.748)
    slow_bound = 2 * _EPSILON / (1 - _EPSILON) * math.sqrt(slow_k_squared)
    constant_bound = 2 * _EPSILON / (1 - _EPSILON) * math.sqrt((8192 + 1024 + 383) / (8192 - 383))
    assert fast_bound < 0.000657
    assert slow_bound < 0.000905
    assert constant_bound < 0.000222
    return {
        "source_only_residual_epsilon": _EPSILON,
        "minimum_common_plateau_frames": 8192,
        "maximum_contiguous_extra_support_frames": _EXTRA_SUPPORT,
        "central_carrier_limits_hz": [20, 23520],
        "modulation_limits_hz": [0, 11750],
        "actual_symmetric_sidebands_must_be_inside_hz": [0, 24000],
        "symmetric_sidebands_not_assumed_inside_original_carrier_limits": True,
        "fast_modulation_at_least_hz": 9,
        "fast_rounded_Dirichlet_error": 5930,
        "fast_normalized_complex_distance_bound": fast_bound,
        "slow_short_weighted_oscillation_ratio": short_rho,
        "slow_long_weighted_oscillation_ratio": long_rho,
        "slow_extension_ratio": extension,
        "slow_normalized_complex_distance_bound": slow_bound,
        "constant_normalized_complex_distance_bound": constant_bound,
        "all_exact_model_bounds_below_existing_0_001": True,
        "requires_landmark_diameter_at_most_one_frame": True,
        "universal_floating_point_libm_or_latent_physics_certification": False,
    }


def _enclosure(frequency: float, modulation: float, radius: float) -> None:
    if not (
        20 <= frequency <= 23520
        and 0 <= modulation <= 11750
        and 0 <= radius < 0.5
        and (radius == 0 or (frequency - modulation > 0 and frequency + modulation < 24000))
    ):
        message = "Gauge theorem input lies outside its declared public-frequency enclosure"
        raise ValueError(message)


def _actual_features(positions: np.ndarray, frequency: float, ratio: complex, modulation: float):
    coefficients = np.array([0.8, -0.6], dtype=np.complex128) * np.exp(0.37j)
    phase = 2 * np.pi * frequency / FS * positions
    carrier = np.exp(1j * phase)[:, None] * coefficients
    modulator = SourceModulator(modulation, ratio)
    values = modulation_values(positions, (modulator,))[:, 0]
    features = np.stack((carrier.real, -carrier.imag), axis=2) * values[:, None, None]
    scale = float(np.max(np.abs(features)))
    assert scale > 0
    return features / scale, values / scale, coefficients, phase


def _gram(features: np.ndarray) -> np.ndarray:
    flattened = features.reshape(-1, 2)
    return flattened.T @ flattened


def _computed_gram_bounds(
    features: np.ndarray, values: np.ndarray, coefficients: np.ndarray, phase: np.ndarray
) -> dict:
    union = _gram(features)
    plateau = _gram(features[512:-512])
    gram_energy = float(np.sum(values * values))
    moment = np.sum(values * values * np.exp(2j * phase)) * np.sum(coefficients**2)
    independent_union = 0.5 * np.array([
        [gram_energy + moment.real, -moment.imag],
        [-moment.imag, gram_energy - moment.real],
    ])
    identity_error = float(np.max(np.abs(union - independent_union)) / np.trace(union))
    assert identity_error < 1.0e-10
    # This is a conservative standard-dot-product allowance for this actual
    # finite matrix calculation, not a global libm or cancellation theorem.
    allowance = 64 * np.finfo(float).eps * (2 * len(features) + 64) * np.trace(union)
    lower = float(np.linalg.eigvalsh(plateau)[0] - allowance)
    upper = float(np.linalg.eigvalsh(union)[-1] + allowance)
    assert lower > 0
    bound = 2 * _EPSILON / (1 - _EPSILON) * math.sqrt(upper / lower)
    assert bound < 0.001
    return {
        "independent_complex_moment_Gram_relative_error": identity_error,
        "standard_dot_product_roundoff_allowance": float(allowance),
        "computed_basis_normalized_complex_distance_bound": bound,
        "floating_enclosure_is_conditional_on_standard_rounding_assumptions": True,
    }


def _computed_case(case: tuple) -> dict:
    name, frequency, modulation, radius, phase_offset, start, frames = case
    _enclosure(frequency, modulation, radius)
    positions = np.arange(start - 512, start + frames + 512, dtype=np.float64)
    trough = start + frames * 0.377
    ratio = radius * np.exp(1j * (math.pi - 2 * math.pi * modulation * trough / FS + phase_offset))
    features, values, coefficients, phase = _actual_features(
        positions, frequency, ratio, modulation
    )
    return {
        "case": name,
        "carrier_hz": frequency,
        "modulation_hz": modulation,
        "modulation_radius": radius,
        "modulation_phase_offset": phase_offset,
        "source_plateau_start_frames": start,
        "source_plateau_frames": frames,
        "actual_symmetric_sidebands_hz": [frequency - modulation, frequency + modulation],
        "computed_modulation_min_max_scaled": [float(np.min(values)), float(np.max(values))],
        **_computed_gram_bounds(features, values, coefficients, phase),
        "search_or_standalone_complex_refusal_branch_executed": False,
    }


def integrity_controls() -> dict:
    """Check exact proof constants and predeclared actual program-basis examples."""
    theorem = theorem_limits()
    radius = float(np.nextafter(0.5, 0.0))
    cases = (
        ("constant_low_carrier", 20.0, 0.0, 0.0, 0.0, 4096, 8192),
        ("constant_near_cancellation", 23520.0, 0.0, radius, 0.0, 4096, 8192),
        ("strong_slow_boundary", 20.01, 8.9999, 0.499999, 0.0, 4096, 8192),
        ("strong_slow_trough", 20.01, 0.001, 0.499999999999, 0.0, 4096, 8192),
        ("tiny_frequency_cancellation", 20.01, 2.0**-35, radius, 0.0, 4096, 8192),
        ("adjusted_lower_sideband", 20.01, 20.0, 0.499999, 0.0, 4096, 8192),
        ("adjusted_upper_sideband", 23519.0, 480.99, 0.499999, 0.0, 4096, 8192),
        ("max_spacing", 12000.0, 11750.0, 0.499999, 0.0, 4096, 8192),
        ("fast_split", 500.0, 9.0, 0.499999, 0.0, 4096, 8192),
        ("slow_phase_shift", 20.01, 3.7, 0.499999, 1.37, 4096, 8192),
        ("long_plateau", 20.01, 0.5, 0.499999, 0.0, 4096, 65536),
        ("late_source_trough", 20.01, 0.001, radius, 0.0, 1555000, 8192),
    )
    actual = [_computed_case(case) for case in cases]
    unsupported = ((19.9, 2.0, 0.2), (23520.1, 2.0, 0.2), (12000, 11750.1, 0.2))
    for fields in unsupported:
        try:
            _enclosure(*fields)
        except ValueError:
            continue
        message = "Out-of-proof-domain input received a public gauge theorem claim"
        raise AssertionError(message)
    return {
        "stage": "public_finite_AM_complex_gauge_bound_controls",
        "passed": True,
        "exact_model_theorem": theorem,
        "separate_actual_computed_basis_examples": actual,
        "out_of_proof_domain_inputs_refused": len(unsupported),
        "fitter_search_admission_or_existing_phase_guard_changed": False,
        "generic_lowlevel_negative_or_standalone_refusal_branch_coverage_claimed": False,
        "physical_interpretation_native_realtime_or_product_acceptance": False,
    }


if __name__ == "__main__":
    emit(integrity_controls())
