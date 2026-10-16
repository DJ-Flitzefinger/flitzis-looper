"""Independent declared-source controls for the bounded finite positive-AM gate.

Gate landmarks, carrier phase, stereo direction and modulation truth construct
original PCM and analytic expectations only. Low-level declared-basis controls
are separate from public PCM-only preparation and complete rendering. No output
target, fixture tag, private fitter helper or altered search policy is used.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING

import numpy as np
from pitch_amfm_model import render as render_residual
from pitch_finite_am_ambiguity_controls import landmark_ambiguity_control
from pitch_finite_am_gate import FiniteSourceGate, fit_finite_am_gate
from pitch_independent_modes import Prepared, prepare, render_prepared
from pitch_source_modulation import SourceModulator

if TYPE_CHECKING:
    from collections.abc import Callable

    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]

_FS, _FRAMES, _REFERENCE = 48_000, 32_768, 8192
_LIMITS = {
    "direct_integer_landmark_max_abs_error_frames": 0,
    "direct_normalized_complex_multiplier_max_abs_error": 2.0e-9,
    "independent_gate_values_max_abs_error": 2.0e-12,
    "direct_whole_analytic_max_abs_error": 2.0e-9,
    "independent_direct_PCM_factorization_max_abs_error": 2.0e-11,
    "public_source_frequency_max_abs_error_hz": 2.0e-5,
    "public_whole_analytic_max_abs_error": 2.0e-5,
    "public_integer_phase_max_abs_error": 4.0e-5,
    "complete_fractional_identity_max_abs_error": 1.0e-9,
    "complete_complement_max_abs_error": 2.0e-15,
}
_POLICY = {
    "certified_causal": True,
    "affine_endpoint": True,
    "predictive_envelope": True,
    "current_weight": True,
    "am_family": True,
    "modulated_basis": True,
    "linear_chirplet": True,
    "finite_am_gate": True,
}


@dataclass(frozen=True, slots=True)
class _Source:
    name: str
    source: Real
    frequency_hz: float
    coefficients: Complex
    modulator: SourceModulator
    multiplier: complex
    landmarks: tuple[int, int, int, int]
    gate_values: Real
    analytic: Complex
    exact_zero_samples: tuple[int, ...] = ()


def _error(actual: Real | Complex, expected: Real | Complex) -> float:
    return float(np.max(np.abs(actual - expected), initial=0.0))


def _unit(values: list[complex]) -> Complex:
    vector = np.asarray(values, dtype=np.complex128)
    return vector / np.linalg.norm(vector)


def _gate(positions: Real, landmarks: tuple[int, int, int, int]) -> Real:
    """Direct sine-squared edge equations, independent of the fitted model."""
    onset, rise, end, release = landmarks
    left = np.clip((positions - onset) / rise, 0.0, 1.0)
    right = np.clip((end - positions) / release, 0.0, 1.0)
    return np.sin(0.5 * np.pi * left) ** 2 * np.sin(0.5 * np.pi * right) ** 2


def _modulation(positions: Real, modulator: SourceModulator) -> Real:
    theta = 2.0 * np.pi * modulator.frequency_hz * positions / _FS
    ratio = modulator.ratio
    return 1.0 + 2.0 * (ratio.real * np.cos(theta) - ratio.imag * np.sin(theta))


def _make_source(
    name: str,
    frequency: float,
    coefficients: Complex,
    modulator: SourceModulator,
    multiplier: complex,
    landmarks: tuple[int, int, int, int],
    *,
    exact_zero_origin: int | None = None,
    exact_zero_step: int | None = None,
) -> _Source:
    clock = np.arange(_FRAMES, dtype=np.float64)
    gates, modulation = _gate(clock, landmarks), _modulation(clock, modulator)
    theta = 2.0 * np.pi * frequency * clock / _FS
    vector = multiplier * coefficients
    # Real sine/cosine synthesis is independent of the complex reconstruction.
    carrier = np.cos(theta)[:, None] * vector.real - np.sin(theta)[:, None] * vector.imag
    zero_samples: tuple[int, ...] = ()
    if exact_zero_origin is not None:
        assert exact_zero_step is not None
        zeros = np.flatnonzero((clock.astype(np.int64) - exact_zero_origin) % exact_zero_step == 0)
        # These declared mathematical carrier zeros otherwise carry sin(pi)'s
        # floating remainder. They are not activity-derived gate boundaries.
        carrier[zeros] = 0.0
        zero_samples = tuple(int(value) for value in zeros if gates[value] > 0.0)
    source = gates[:, None] * modulation[:, None] * carrier
    analytic = gates[:, None] * modulation[:, None] * np.exp(1.0j * theta)[:, None] * vector
    factorization = _error(source, analytic.real)
    assert factorization < _LIMITS["independent_direct_PCM_factorization_max_abs_error"]
    return _Source(
        name,
        source,
        frequency,
        coefficients,
        modulator,
        multiplier,
        landmarks,
        gates,
        analytic,
        zero_samples,
    )


def _positive_sources() -> tuple[_Source, _Source]:
    elliptical = _make_source(
        "unseen_elliptical_stereo",
        853.17,
        _unit([0.43 * np.exp(0.37j), 0.77 * np.exp(-1.09j)]),
        SourceModulator(31.41, 0.173 * np.exp(0.79j)),
        0.287 * np.exp(-0.61j),
        (2048, 73, 30_000, 335),
    )
    carrier_gauge = 0.37
    zero_origin = 2049
    collinear = _make_source(
        "unseen_collinear_stereo_carrier_zero_edge_samples",
        750.0,
        _unit([0.8 * np.exp(1.0j * carrier_gauge), -0.6 * np.exp(1.0j * carrier_gauge)]),
        SourceModulator(23.73, 0.219 * np.exp(-0.83j)),
        0.319 * np.exp(1.0j * (np.pi / 2.0 - np.pi / 32.0 - carrier_gauge)),
        (2048, 219, 30_018, 411),
        exact_zero_origin=zero_origin,
        exact_zero_step=32,
    )
    assert collinear.source[zero_origin].tolist() == [0.0, 0.0]
    assert collinear.source[collinear.landmarks[2] - 1].tolist() == [0.0, 0.0]
    activity = np.flatnonzero(np.any(collinear.source != 0.0, axis=1))
    assert int(activity[0]) == collinear.landmarks[0] + 2
    assert int(activity[-1]) == collinear.landmarks[2] - 2
    return elliptical, collinear


def _summary(report: dict) -> dict:
    """Retain bounded summaries, never candidate lists or source-window trees."""
    keys = (
        "accepted",
        "status",
        "refusal_reason",
        "search_complete",
        "incomplete_search",
        "feasible_model_pairs",
        "edge_candidate_counts",
        "edge_survivor_counts",
        "paired_evaluations",
        "landmark_ranges",
        "direct_relative_source_rms",
        "direct_relative_onset_rms",
        "direct_relative_release_rms",
        "all_feasible_designs_passed_condition",
        "all_feasible_jacobians_passed",
        "boundary_unresolved",
        "global_iq_rank",
        "global_iq_condition",
        "six_parameter_jacobian",
        "reference_fit",
        "estimated_peak_working_bytes",
        "minimum_direct_source_fit",
    )

    def finite(value):
        if isinstance(value, dict):
            return {key: finite(item) for key, item in value.items()}
        if isinstance(value, (tuple, list)):
            return [finite(item) for item in value]
        if isinstance(value, np.generic):
            value = value.item()
        if isinstance(value, float) and not np.isfinite(value):
            return "nonfinite"
        return value

    return {key: finite(report[key]) for key in keys if key in report}


def _landmarks(gate: FiniteSourceGate) -> tuple[int, int, int, int]:
    return (
        gate.onset_frame,
        gate.onset_frame + gate.rise_frames,
        gate.end_frame - gate.release_frames,
        gate.end_frame,
    )


def _truth_landmarks(source: _Source) -> tuple[int, int, int, int]:
    onset, rise, end, release = source.landmarks
    return onset, onset + rise, end - release, end


def _fit(source: _Source, pcm: Real | None = None, reference: int = _REFERENCE):
    return fit_finite_am_gate(
        source.source if pcm is None else pcm,
        source.frequency_hz,
        source.coefficients,
        source.modulator,
        reference,
    )


def _check_positive(source: _Source, scale: float = 1.0) -> dict:
    pcm = source.source * scale
    before = pcm.copy()
    result = _fit(source, pcm)
    assert result.gate is not None, (source.name, scale, _summary(result.report))
    assert result.report["accepted"]
    assert result.report["search_complete"]
    assert not result.report["incomplete_search"]
    actual_landmarks = _landmarks(result.gate)
    landmark_error = max(
        abs(left - right)
        for left, right in zip(actual_landmarks, _truth_landmarks(source), strict=True)
    )
    assert landmark_error <= _LIMITS["direct_integer_landmark_max_abs_error_frames"]
    expected_multiplier = source.multiplier * scale
    multiplier_error = abs(result.multiplier - expected_multiplier) / abs(expected_multiplier)
    assert multiplier_error < _LIMITS["direct_normalized_complex_multiplier_max_abs_error"]
    positions = np.arange(len(pcm), dtype=np.float64)
    fitted_gate = result.gate.values(positions)
    gate_error = _error(fitted_gate, source.gate_values)
    assert gate_error < _LIMITS["independent_gate_values_max_abs_error"]
    analytic = (
        fitted_gate[:, None]
        * _modulation(positions, source.modulator)[:, None]
        * np.exp(2.0j * np.pi * source.frequency_hz * positions / _FS)[:, None]
        * (source.coefficients * result.multiplier)
    )
    analytic_error = _error(analytic / scale, source.analytic)
    assert analytic_error < _LIMITS["direct_whole_analytic_max_abs_error"]
    residual = pcm - analytic.real
    closure = _error((analytic.real + residual) / scale, pcm / scale)
    assert closure < _LIMITS["complete_complement_max_abs_error"]
    assert np.array_equal(pcm, before)
    outside = np.array([-9.5, -1.0, source.landmarks[0], source.landmarks[2], _FRAMES + 1.25])
    assert np.array_equal(result.gate.values(outside), np.zeros(len(outside)))
    return {
        "source_kind": source.name,
        "source_scale": scale,
        "declared_frequency_hz": source.frequency_hz,
        "declared_modulation_hz_and_ratio": [
            source.modulator.frequency_hz,
            [source.modulator.ratio.real, source.modulator.ratio.imag],
        ],
        "landmarks_onset_rise_end_release_start_end": list(actual_landmarks),
        "all_landmark_max_abs_error_frames": landmark_error,
        "normalized_complex_multiplier_error": multiplier_error,
        "whole_gate_max_abs_error": gate_error,
        "normalized_whole_physical_analytic_max_abs_error": analytic_error,
        "independent_PCM_factorization_max_abs_error": _error(source.source, source.analytic.real),
        "source_scale_normalized_complement_max_abs_error": closure,
        "carrier_zero_samples_in_positive_gate": len(source.exact_zero_samples),
        "source_preserved_bitwise": True,
        "search": _summary(result.report),
    }


def _refused(source: _Source, label: str, pcm: Real, reference: int = _REFERENCE) -> dict:
    before = pcm.copy()
    result = _fit(source, pcm, reference)
    assert result.gate is None, (label, _summary(result.report))
    assert result.multiplier == 0.0j
    assert not result.report["accepted"]
    assert np.array_equal(pcm, before)
    return {
        "source_kind": label,
        "source_frames": len(pcm),
        "refused": True,
        **_summary(result.report),
    }


def _negative_sources(base: _Source) -> list[dict]:
    clock = np.arange(_FRAMES, dtype=np.float64)
    shorter_edge = _make_source(
        "edge_shorter_than_three_frames",
        base.frequency_hz,
        base.coefficients,
        base.modulator,
        base.multiplier,
        (2048, 2, 30_000, 335),
    )
    underresolved = _make_source(
        "collinear_three_frame_edge_one_informative_interior_sample",
        12_000.0,
        _unit([0.8 + 0.0j, -0.6 + 0.0j]),
        SourceModulator(31.41, 0.173 * np.exp(0.79j)),
        0.287 + 0.0j,
        (2048, 3, 30_000, 335),
        exact_zero_origin=2049,
        exact_zero_step=2,
    )
    no_plateau = _make_source(
        "overlapping_edges_without_plateau",
        base.frequency_hz,
        base.coefficients,
        base.modulator,
        base.multiplier,
        (2048, 17_000, 30_000, 17_000),
    )
    wide = _make_source(
        "edges_outside_frozen_512_frame_width_domain",
        base.frequency_hz,
        base.coefficients,
        base.modulator,
        base.multiplier,
        (2048, 733, 30_000, 941),
    )
    pulse = np.zeros_like(base.source)
    pulse[4096] = [0.21, -0.07]
    transient = base.source.copy()
    transient[10_047] += [0.11, -0.08]
    tail = base.source.copy()
    tail[31_001:31_801] = 0.023 * np.column_stack((
        np.sin(2.0 * np.pi * 1703.71 * clock[31_001:31_801] / _FS),
        np.cos(2.0 * np.pi * 1381.29 * clock[31_001:31_801] / _FS),
    ))
    mismatch = base.source.copy()
    mismatch += (
        base.gate_values[:, None]
        * 0.013
        * np.column_stack((
            np.cos(2.0 * np.pi * 1327.19 * clock / _FS + 0.27),
            np.sin(2.0 * np.pi * 1739.71 * clock / _FS - 0.61),
        ))
    )
    reports = [
        _refused(source, source.name, source.source)
        for source in (shorter_edge, underresolved, no_plateau, wide)
    ]
    reports.extend([
        _refused(base, "one_sample_collinear_pulse_without_plateau_IQ", pulse),
        _refused(base, "zero_global_multiplier", np.zeros_like(base.source)),
        _refused(
            base, "truncated_source_onset_neighborhood", base.source[2100:], _REFERENCE - 2100
        ),
        _refused(base, "independent_transient_inside_plateau", transient),
        _refused(base, "independent_nonzero_tail_outside_gate", tail),
        _refused(base, "whole_source_unmodelled_stereo_carriers", mismatch),
    ])
    return reports


def _ambiguity_control() -> dict:
    # One predeclared low-frequency construction. A carrier zero makes these
    # short edges weak in PCM, while a long plateau still identifies I/Q.
    # Do not retune it, the fit tolerance, or the enumerator to force a branch.
    onset, end, frequency = 2048, 28_448, 20.0
    phase = np.pi / 2.0 - 2.0 * np.pi * frequency * onset / _FS
    source = _make_source(
        "slow_collinear_carrier_zero_short_edges",
        frequency,
        _unit([0.8 + 0.0j, -0.6 + 0.0j]),
        SourceModulator(2.0, 0.173 * np.exp(0.59j)),
        0.287 * np.exp(1.0j * phase),
        (onset, 8, end, 8),
        exact_zero_origin=onset,
        exact_zero_step=1200,
    )
    result = _fit(source)
    compact = _summary(result.report)
    ranges = result.report.get("landmark_ranges", {})
    has_spread = any(
        isinstance(values, dict) and values["max"] - values["min"] > 1 for values in ranges.values()
    )
    demonstrated = result.report.get("feasible_model_pairs", 0) > 1 and has_spread
    if demonstrated:
        assert result.gate is None
        assert not result.report["accepted"]
    elif result.gate is not None:
        assert _landmarks(result.gate) == _truth_landmarks(source)
        relative_multiplier_error = abs(result.multiplier - source.multiplier) / abs(
            source.multiplier
        )
        assert (
            relative_multiplier_error
            < _LIMITS["direct_normalized_complex_multiplier_max_abs_error"]
        )
    return {
        "source_kind": source.name,
        "declared_onset_end_rise_release": [onset, end, 8, 8],
        "bounded_model_refused": result.gate is None,
        "multiple_feasible_landmark_spread_above_one_frame_demonstrated": demonstrated,
        "search": compact,
        "coverage_limit": (
            "No independent cap-exhaustion construction is claimed."
            if demonstrated
            else "The unchanged predeclared weak-edge source does not demonstrate the "
            "multiple-feasible landmark-diameter branch; cap-exhaustion coverage is not claimed."
        ),
        "policy_or_private_helpers_altered": False,
    }


def _linear_samples(source: Real, positions: Real) -> Real:
    output = np.zeros((len(positions), 2))
    lower = np.floor(positions).astype(np.int64)
    fraction = positions - lower
    for neighbor, weights in ((lower, 1.0 - fraction), (lower + 1, fraction)):
        inside = (neighbor >= 0) & (neighbor < len(source))
        output[inside] += weights[inside, None] * source[neighbor[inside]]
    return output


def _integer_mode(source: _Source, birth: int, positions: Real, rate: float, shift: float) -> Real:
    output = np.zeros((len(positions), 2))
    assert np.all(positions == np.round(positions))
    inside = (positions >= 0) & (positions < len(source.source))
    s = positions[inside]
    physical_amplitude = _gate(s, source.landmarks) * _modulation(s, source.modulator)
    theta = 2.0 * np.pi * source.frequency_hz / _FS * (birth + shift / rate * (s - birth))
    output[inside] = (
        physical_amplitude[:, None]
        * (np.exp(1.0j * theta)[:, None] * (source.coefficients * source.multiplier)).real
    )
    return output


def _public_integer_controls(source: _Source, prepared: Prepared) -> list[dict]:
    integer_reports = []
    for semitones in (-7, 0, 7):
        actual, report = render_prepared(prepared, 2.0, semitones)
        residual, residual_report = render_residual(prepared.residual, 2.0, semitones)
        assert report["capture_origin"] == residual_report["capture_origin"]
        positions = 2.0 * (report["capture_origin"] + np.arange(len(actual)))
        expected = _integer_mode(
            source,
            source.landmarks[0],
            positions,
            2.0,
            2.0 ** (semitones / 12.0),
        )
        phase_error = _error(actual, expected + residual)
        assert phase_error < _LIMITS["public_integer_phase_max_abs_error"]
        integer_reports.append({
            "rate": 2.0,
            "semitones": semitones,
            "complete_generated_output_frames": len(actual),
            "known_original_analytic_phase_plus_complete_residual_max_abs_error": phase_error,
        })
    return integer_reports


def _fractional_identities(mixed: Prepared) -> list[dict]:
    identities = []
    for semitones in (-7, 7):
        rate = 2.0 ** (semitones / 12.0)
        actual, report = render_prepared(mixed, rate, semitones)
        positions = rate * (report["capture_origin"] + np.arange(len(actual)))
        expected = _linear_samples(mixed.source, positions)
        error = _error(actual, expected)
        assert error < _LIMITS["complete_fractional_identity_max_abs_error"]
        identities.append({
            "semitones": semitones,
            "rate_equals_pitch_ratio": rate,
            "complete_generated_output_frames": len(actual),
            "complete_source_zero_extended_linear_identity_max_abs_error": error,
        })
    return identities


def _nonunity_tail_control(source: _Source, mixed: Prepared) -> dict:
    complete, details = render_prepared(mixed, 2.0, 7)
    residual, residual_details = render_residual(mixed.residual, 2.0, 7)
    assert details["capture_origin"] == residual_details["capture_origin"]
    positions = 2.0 * (details["capture_origin"] + np.arange(len(complete)))
    expected = (
        _integer_mode(
            source,
            source.landmarks[0],
            positions,
            2.0,
            2.0 ** (7 / 12.0),
        )
        + residual
    )
    tail_error = _error(complete, expected)
    assert tail_error < _LIMITS["public_integer_phase_max_abs_error"]
    nominal = (len(mixed.source) + 1) // 2
    tail_energy = float(np.sum(residual[nominal:] ** 2))
    assert len(complete) > nominal
    assert tail_energy > 1.0e-12
    return {
        "nonunity_complete_output_with_residual_tails_max_abs_error": tail_error,
        "nonunity_residual_tail_energy_beyond_nominal_duration": tail_energy,
    }


def _renderer_complement_controls(source: _Source, prepared: Prepared) -> dict:
    # A separately declared nonzero complement tests the unchanged renderer's
    # tails without claiming that this extra source is an isolated gate model.
    extra = np.zeros_like(prepared.source)
    extra[0], extra[-1] = [0.11, -0.07], [-0.19, 0.13]
    extra[173:797] = np.random.default_rng(771_893).normal(0.0, 0.013, (624, 2))
    mixed = Prepared(
        prepared.source + extra,
        prepared.modes,
        prepared.residual + extra,
        prepared.report,
    )
    return {
        "fractional_h_equals_r_with_declared_nonzero_complement": _fractional_identities(mixed),
        **_nonunity_tail_control(source, mixed),
        "extra_complement_is_renderer_control_not_public_gate_acceptance_claim": True,
    }


def _public_pipeline(source: _Source) -> dict:
    prepared = prepare(source.source, **_POLICY)
    assert len(prepared.modes) == 1, (source.name, prepared.report["frequencies_hz"])
    mode = prepared.modes[0]
    assert mode.birth == source.landmarks[0]
    frequency_error = abs(mode.frequency_hz - source.frequency_hz)
    analytic_error = _error(mode.analytic_pcm, source.analytic)
    assert frequency_error < _LIMITS["public_source_frequency_max_abs_error_hz"]
    assert analytic_error < _LIMITS["public_whole_analytic_max_abs_error"]
    closure = _error(mode.analytic_pcm.real + prepared.residual, source.source)
    assert closure < _LIMITS["complete_complement_max_abs_error"]
    return {
        "source_kind": source.name,
        "PCM_only_public_preparation": True,
        "source_frames": len(source.source),
        "frequency_max_abs_error_hz": frequency_error,
        "whole_physical_analytic_max_abs_error": analytic_error,
        "complete_source_complement_max_abs_error": closure,
        "integer_rate_whole_output_analytic_phase": _public_integer_controls(source, prepared),
        **_renderer_complement_controls(source, prepared),
    }


def _expect_rejection(callback: Callable, label: str) -> str:
    try:
        callback()
    except ValueError, TypeError:
        return label
    message = f"Malformed finite-gate contract accepted: {label}"
    raise AssertionError(message)


def _validation_controls(base: _Source) -> dict:
    source = np.zeros((17, 2), dtype=np.float64)
    arguments = [source, base.frequency_hz, base.coefficients, base.modulator, 0]
    refused = []
    cases = (
        ("complex_source", 0, source.astype(np.complex128)),
        ("float32_source", 0, source.astype(np.float32)),
        ("integer_source", 0, source.astype(np.int64)),
        ("wrong_source_shape", 0, np.zeros((17, 1))),
        ("nonfinite_source", 0, np.full((17, 2), np.nan)),
        ("zero_carrier_frequency", 1, 0.0),
        ("Nyquist_carrier_frequency", 1, _FS / 2),
        ("nonfinite_carrier_frequency", 1, float("nan")),
        ("wrong_coefficient_shape", 2, base.coefficients[None]),
        ("real_coefficient_dtype", 2, base.coefficients.real),
        ("nonunit_coefficients", 2, 2.0 * base.coefficients),
        ("nonfinite_coefficients", 2, np.array([complex("nan"), 0.0j])),
        ("wrong_modulator_type", 3, None),
        ("negative_modulator_frequency", 3, SourceModulator(-2.0, 0.1j)),
        ("nonpositive_modulator", 3, SourceModulator(2.0, 0.5 + 0.0j)),
        ("out_of_band_modulation_sideband", 3, SourceModulator(base.frequency_hz + 1.0, 0.1j)),
        ("fractional_reference", 4, 1.5),
        ("boolean_reference", 4, True),
    )
    for label, index, value in cases:
        invalid = arguments.copy()
        invalid[index] = value
        refused.append(
            _expect_rejection(lambda invalid=invalid: fit_finite_am_gate(*invalid), label)
        )
    for label, fields in (
        ("fractional_gate_landmark", (0.5, 3, 100, 3)),
        ("boolean_gate_landmark", (False, 3, 100, 3)),
        ("zero_gate_rise_width", (0, 0, 100, 3)),
        ("overlapping_gate_edges", (0, 70, 100, 70)),
    ):
        refused.append(
            _expect_rejection(
                lambda fields=fields: FiniteSourceGate(*fields),
                label,
            )
        )
    unsupported_references = [
        _refused(base, "negative_reference_geometry", base.source, -1),
        _refused(
            base,
            "incomplete_reference_geometry",
            base.source,
            len(base.source) - _REFERENCE + 1,
        ),
    ]
    huge = np.broadcast_to(np.zeros((1, 2), dtype=np.float64), (2_000_000, 2))
    memory = fit_finite_am_gate(
        huge,
        base.frequency_hz,
        base.coefficients,
        base.modulator,
        _REFERENCE,
    )
    assert memory.gate is None
    assert memory.multiplier == 0.0j
    assert memory.report["refusal_reason"] == "conservative_memory_limit"
    assert "first_active_frame" not in memory.report
    assert not memory.report["full_source_feature_array_allocated"]
    return {
        "invalid_contracts_rejected_before_short_or_silent_shortcut": refused,
        "unsupported_reference_geometry_refusals": unsupported_references,
        "large_memory_control_is_read_only_broadcast_zero_view": True,
        "large_memory_control_declared_additional_workspace_bytes": 128 * len(huge) + 64 * 1024**2,
        "large_memory_control_workspace_budget_bytes": 256 * 1024**2,
        "large_memory_refused_before_source_scan_or_fitted_output": _summary(memory.report),
    }


def integrity_controls() -> dict:
    """Check original PCM, conditional gate/gauge recovery and complete renders."""
    elliptical, collinear = _positive_sources()
    return {
        "stage": "bounded_source_finite_positive_AM_gate_integrity_controls",
        "passed": True,
        "predeclared_numeric_acceptance_limits": dict(_LIMITS),
        "independent_unseen_declared_basis_sources": [
            _check_positive(elliptical),
            _check_positive(collinear),
        ],
        "positive_finite_scale_invariance": [
            _check_positive(elliptical, 2.0**-400),
            _check_positive(elliptical, 2.0**400),
        ],
        "unsupported_underresolved_and_mismatched_sources": _negative_sources(elliptical),
        "predeclared_weak_edge_ambiguity_control": _ambiguity_control(),
        "independent_multiple_feasible_landmark_control": landmark_ambiguity_control(),
        "public_source_certificate_gate_and_complete_render": [
            _public_pipeline(elliptical),
            _public_pipeline(collinear),
        ],
        "malformed_contract_and_preallocation_refusals": _validation_controls(elliptical),
        "truth_only_in_declared_low_level_controls_generation_and_expectations": True,
        "output_targets_fixture_identifiers_private_helpers_or_policy_tuning_used": False,
        "latent_physics_causality_general_mixtures_native_or_hearing_acceptance_claim": False,
    }
