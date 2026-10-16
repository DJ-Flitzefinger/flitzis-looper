"""Independent PCM controls for joint finite gates and one stereo nuisance frame.

Frozen truth constructs source PCM and expectations only. The low-level fitter
receives declared unit carrier directions; public preparation receives PCM alone.
No fitted-output target, fixture selector, private model helper or policy override
is passed to either path. These offline checks make no product or hearing claim.
"""

from dataclasses import dataclass, replace
from itertools import starmap
from typing import TYPE_CHECKING

import numpy as np
from pitch_amfm_model import render as render_residual
from pitch_causal_model_probe import emit
from pitch_independent_modes import Prepared, prepare, render_prepared
from pitch_independent_modes_probe import cases as independent_cases
from pitch_joint_finite_gates import fit_joint_finite_gates
from pitch_joint_flank_bounds_controls import flank_partition_controls

if TYPE_CHECKING:
    from collections.abc import Callable

    from numpy.typing import NDArray
    from pitch_joint_finite_gates import JointFiniteGateFit

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]
type GateTruth = tuple[int, int, int, int]

_FS, _FRAMES, _REFERENCE = 48_000, 48_000, 20_000
_FREQUENCIES = (743.27, 1193.81)
_GATES = ((3000, 83, 42_000, 137), (5107, 109, 38_123, 177))
_MULTIPLIERS = (0.291 * np.exp(-0.61j), 0.183 * np.exp(0.87j))
_IMPULSE_FRAME, _IMPULSE = 12_019, (0.41, -0.23)
_LIMITS = {
    "integer_landmark_max_abs_error_source_frames": 1,
    "normalized_complex_multiplier_max_abs_error": 1.0e-3,
    "relative_complex_multiplier_max_abs_error": 1.0e-3,
    "complex_multiplier_phase_max_abs_error_radians": 1.0e-3,
    "global_and_each_1025_frame_edge_relative_rms": 1.0e-4,
    "joint_design_and_geometry_condition": 1.0e6,
    "normalized_independent_PCM_factorization_max_abs_error": 2.0e-11,
    "normalized_whole_complement_max_abs_error": 2.0e-15,
    "public_source_frequency_max_abs_error_hz": 2.0e-5,
    "public_normalized_whole_analytic_max_abs_error": 2.0e-4,
    "public_complete_integer_phase_max_abs_error": 5.0e-4,
    "complete_fractional_identity_max_abs_error": 1.0e-9,
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
    "joint_finite_gates": True,
}


@dataclass(frozen=True, slots=True)
class _Source:
    name: str
    source: Real
    frequencies: Real
    coefficients: Complex
    multipliers: Complex
    gates: tuple[GateTruth, ...]
    gate_values: tuple[Real, ...]
    analytic: tuple[Complex, ...]
    reference: int
    impulse_frame: int | None
    impulse: Real


def _unit(values: list[complex]) -> Complex:
    vector = np.asarray(values, dtype=np.complex128)
    return vector / np.linalg.norm(vector)


def _elliptical_directions() -> Complex:
    return np.vstack((
        _unit([0.47 * np.exp(0.31j), 0.83 * np.exp(-0.79j)]),
        _unit([0.72 * np.exp(-0.43j), -0.38 * np.exp(0.97j)]),
    ))


def _gate(positions: Real, truth: GateTruth, *, linear: bool = False) -> Real:
    """Evaluate independent clipped cosine equations in source coordinates."""
    onset, rise, end, release = truth
    left = np.clip((positions - onset) / rise, 0.0, 1.0)
    right = np.clip((end - positions) / release, 0.0, 1.0)
    if linear:
        return left * right
    return 0.25 * (1.0 - np.cos(np.pi * left)) * (1.0 - np.cos(np.pi * right))


def _error(actual: Real | Complex, expected: Real | Complex) -> float:
    return float(np.max(np.abs(actual - expected), initial=0.0))


def _same_bits(actual: Real, expected: Real) -> bool:
    return actual.shape == expected.shape and np.array_equal(
        actual.view(np.uint64), expected.view(np.uint64)
    )


def _make_source(
    name: str,
    *,
    coefficients: Complex | None = None,
    multipliers: tuple[complex, ...] = _MULTIPLIERS,
    gates: tuple[GateTruth, ...] = _GATES,
    impulse_frame: int | None = _IMPULSE_FRAME,
    linear_first_gate: bool = False,
) -> _Source:
    positions = np.arange(_FRAMES, dtype=np.float64)
    directions = _elliptical_directions() if coefficients is None else coefficients.copy()
    frequencies = np.array(_FREQUENCIES, dtype=np.float64)
    values, analytic = [], []
    pcm = np.zeros((_FRAMES, 2), dtype=np.float64)
    for index, (frequency, multiplier, truth) in enumerate(
        zip(frequencies, multipliers, gates, strict=True)
    ):
        envelope = _gate(positions, truth, linear=linear_first_gate and index == 0)
        phase = 2.0 * np.pi * frequency * positions / _FS
        vector = multiplier * directions[index]
        carrier = np.cos(phase)[:, None] * vector.real - np.sin(phase)[:, None] * vector.imag
        pcm += envelope[:, None] * carrier
        values.append(envelope)
        analytic.append(envelope[:, None] * np.exp(1.0j * phase)[:, None] * vector)
    expected = sum((component.real for component in analytic), np.zeros_like(pcm))
    factorization = _error(pcm, expected)
    assert factorization < _LIMITS["normalized_independent_PCM_factorization_max_abs_error"]
    impulse = np.asarray(_IMPULSE, dtype=np.float64)
    if impulse_frame is not None:
        pcm[impulse_frame] += impulse
    return _Source(
        name,
        pcm,
        frequencies,
        directions,
        np.asarray(multipliers, dtype=np.complex128),
        gates,
        tuple(values),
        tuple(analytic),
        _REFERENCE,
        impulse_frame,
        impulse,
    )


def _translated(source: _Source, offset: int) -> _Source:
    """Translate actual source samples and change the expected complex gauge."""
    return replace(
        source,
        name=f"{source.name}_source_translated_{offset}_frames",
        source=np.pad(source.source, ((offset, 0), (0, 0))),
        multipliers=source.multipliers * np.exp(-2.0j * np.pi * source.frequencies * offset / _FS),
        gates=tuple(
            (onset + offset, rise, end + offset, release)
            for onset, rise, end, release in source.gates
        ),
        gate_values=tuple(np.pad(values, (offset, 0)) for values in source.gate_values),
        analytic=tuple(np.pad(values, ((offset, 0), (0, 0))) for values in source.analytic),
        reference=source.reference + offset,
        impulse_frame=None if source.impulse_frame is None else source.impulse_frame + offset,
    )


def _finite_json(value):
    if isinstance(value, dict):
        return {key: _finite_json(item) for key, item in value.items()}
    if isinstance(value, (tuple, list)):
        return [_finite_json(item) for item in value]
    if isinstance(value, np.ndarray):
        return _finite_json(value.tolist())
    if isinstance(value, np.generic):
        value = value.item()
    if isinstance(value, float) and not np.isfinite(value):
        return "nonfinite"
    return value


def _summary(report: dict) -> dict:
    keys = (
        "accepted",
        "status",
        "refusal_reason",
        "search_complete",
        "incomplete_search",
        "estimated_peak_working_bytes",
        "memory_limit_bytes",
        "requested_joint_onset_combinations",
        "requested_joint_release_combinations",
        "joint_onset_survivors",
        "joint_release_survivors",
        "requested_complete_pairs",
        "paired_evaluations",
        "direct_model_confirmations",
        "direct_support_confirmations",
        "feasible_models",
        "landmark_ranges",
        "coefficient_ranges",
        "coefficient_diameter_upper_bound",
        "feasible_nuisance_supports",
        "boundary_unresolved",
        "all_feasible_joint_designs_passed",
        "all_feasible_joint_jacobians_passed",
        "global_relative_rms",
        "edge_relative_rms",
        "tonal_only_remainder_relative_rms",
        "joint_design_rank",
        "joint_design_condition",
        "joint_geometry_jacobian",
        "selected_gates",
    )
    result = {key: report[key] for key in keys if key in report}
    plateau_keys = (
        "accepted",
        "refusal_reason",
        "reference_start",
        "reference_frames",
        "plateau_start",
        "plateau_end_exclusive",
        "bin_proof_complete",
        "all_central_deletion_supports_considered",
        "plateau_factorization",
        "plateau_deletions",
    )
    if "plateau_certificate" in report:
        plateau = report["plateau_certificate"]
        result["plateau_certificate"] = {
            key: plateau[key] for key in plateau_keys if key in plateau
        }
    if "edge_search" in report:
        edges = report["edge_search"]
        result["edge_search"] = {
            key: edges[key]
            for key in (
                "accepted",
                "refusal_reason",
                "search_complete",
                "incomplete_search",
                "source_grid_search_complete",
                "full_source_grid_enumerated",
                "carrier_reports",
            )
            if key in edges
        }
    return _finite_json(result)


def _landmarks(truth: GateTruth) -> tuple[int, int, int, int]:
    onset, rise, end, release = truth
    return onset, onset + rise, end - release, end


def _positive_source_expectations(
    source: _Source, fitted: JointFiniteGateFit, scale: float
) -> tuple[int, list[float], list[Complex]]:
    assert fitted.gates is not None
    landmark_errors = []
    gate_errors, analytic = [], []
    positions = np.arange(len(source.source), dtype=np.float64)
    for index, (actual, truth) in enumerate(zip(fitted.gates, source.gates, strict=True)):
        measured = _landmarks((
            actual.onset_frame,
            actual.rise_frames,
            actual.end_frame,
            actual.release_frames,
        ))
        landmark_errors.append(
            max(abs(a - b) for a, b in zip(measured, _landmarks(truth), strict=True))
        )
        values = actual.values(positions)
        gate_errors.append(_error(values, source.gate_values[index]))
        analytic.append(
            values[:, None]
            * np.exp(2.0j * np.pi * source.frequencies[index] * positions / _FS)[:, None]
            * source.coefficients[index]
            * (fitted.multipliers[index] / scale)
        )
    assert max(landmark_errors) <= _LIMITS["integer_landmark_max_abs_error_source_frames"]
    return max(landmark_errors), gate_errors, analytic


def _positive_multiplier_errors(source: _Source, recovered: Complex) -> dict:
    peak = float(np.max(np.abs(source.source)))
    normalized_error = _error(recovered, source.multipliers) / peak
    relative_error = float(np.max(np.abs(recovered / source.multipliers - 1.0)))
    phase_error = float(np.max(np.abs(np.angle(recovered / source.multipliers))))
    assert normalized_error <= _LIMITS["normalized_complex_multiplier_max_abs_error"]
    assert relative_error <= _LIMITS["relative_complex_multiplier_max_abs_error"]
    assert phase_error <= _LIMITS["complex_multiplier_phase_max_abs_error_radians"]
    return {
        "normalized_complex_multiplier_max_abs_error": normalized_error,
        "relative_complex_multiplier_max_abs_error": relative_error,
        "complex_multiplier_phase_max_abs_error_radians": phase_error,
    }


def _check_positive_search(report: dict, carrier_count: int) -> None:
    assert report["accepted"]
    assert report["search_complete"]
    assert not report["incomplete_search"]
    assert not report["boundary_unresolved"]
    assert report["all_feasible_joint_designs_passed"]
    assert report["all_feasible_joint_jacobians_passed"]
    assert report["global_relative_rms"] <= _LIMITS["global_and_each_1025_frame_edge_relative_rms"]
    assert len(report["edge_relative_rms"]) == 2 * carrier_count
    assert (
        max(report["edge_relative_rms"]) <= _LIMITS["global_and_each_1025_frame_edge_relative_rms"]
    )
    assert report["joint_design_rank"] == 2 * carrier_count
    assert report["joint_design_condition"] <= _LIMITS["joint_design_and_geometry_condition"]
    jacobian = report["joint_geometry_jacobian"]
    assert jacobian["passed"]
    assert jacobian["rank"] == 6 * carrier_count
    assert jacobian["condition"] <= _LIMITS["joint_design_and_geometry_condition"]
    assert report["coefficient_diameter_upper_bound"] <= 1.0e-3
    for ranges in report["landmark_ranges"]:
        assert all(
            ranges[name][1] - ranges[name][0] <= 1
            for name in ("onset_frame", "rise_end_frame", "release_start_frame", "end_frame")
        )


def _positive_complement_errors(source: _Source, analytic: list[Complex], report: dict) -> dict:
    peak = float(np.max(np.abs(source.source)))
    tones = sum((component.real for component in analytic), np.zeros_like(source.source))
    residual = source.source - tones
    closure = _error(tones + residual, source.source) / peak
    assert closure <= _LIMITS["normalized_whole_complement_max_abs_error"]
    retained_impulse_error = None
    if source.impulse_frame is not None:
        retained_impulse_error = _error(residual[source.impulse_frame], source.impulse) / peak
        assert retained_impulse_error <= _LIMITS["normalized_complex_multiplier_max_abs_error"]
        assert report["feasible_nuisance_supports"] == [source.impulse_frame]
    return {
        "normalized_whole_source_complement_max_abs_error": closure,
        "normalized_nuisance_retention_max_abs_error": retained_impulse_error,
    }


def _check_positive(source: _Source, scale: float = 1.0) -> dict:
    pcm, before = source.source * scale, source.source * scale
    fitted = fit_joint_finite_gates(pcm, source.frequencies, source.coefficients, source.reference)
    assert _same_bits(pcm, before), source.name
    assert fitted.gates is not None, (source.name, scale, _summary(fitted.report))
    assert len(fitted.gates) == len(source.frequencies)
    _check_positive_search(fitted.report, len(source.frequencies))
    landmark_error, gate_errors, analytic = _positive_source_expectations(source, fitted, scale)
    return {
        "source_kind": source.name,
        "source_frames": len(pcm),
        "source_scale": scale,
        "declared_frequencies_hz": source.frequencies.tolist(),
        "declared_gates_onset_rise_end_release": source.gates,
        "arbitrary_stereo_nuisance_frame": source.impulse_frame,
        "all_landmark_max_abs_error_source_frames": landmark_error,
        "per_mode_whole_gate_max_abs_error": gate_errors,
        **_positive_multiplier_errors(source, fitted.multipliers / scale),
        **_positive_complement_errors(source, analytic, fitted.report),
        "source_preserved_bitwise": True,
        "entire_tonal_subtraction_residual_retained": True,
        "search": _summary(fitted.report),
    }


def _refused(source: _Source, label: str, pcm: Real, reference: int | None = None) -> dict:
    before = pcm.copy()
    fitted = fit_joint_finite_gates(
        pcm,
        source.frequencies,
        source.coefficients,
        source.reference if reference is None else reference,
    )
    assert _same_bits(pcm, before), label
    assert fitted.gates is None, (label, _summary(fitted.report))
    assert len(fitted.multipliers) == 0
    assert not fitted.report["accepted"]
    assert fitted.report["refusal_reason"], label
    return {
        "source_kind": label,
        "refused": True,
        "source_preserved_bitwise": True,
        "search": _summary(fitted.report),
    }


def _negative_controls(base: _Source) -> list[dict]:
    missing = _make_source("declared_second_mode_absent", multipliers=(_MULTIPLIERS[0], 0.0j))
    weak = _make_source(
        "declared_second_mode_below_source_error_budget",
        multipliers=(_MULTIPLIERS[0], _MULTIPLIERS[1] * 2.0**-20),
    )
    noncosine = _make_source("linear_first_gate_not_raised_cosine", linear_first_gate=True)
    disjoint = _make_source(
        "no_common_two_mode_plateau", gates=((3000, 83, 18_000, 137), (19_500, 109, 42_000, 177))
    )
    twice = base.source.copy()
    twice[15_131] += [-0.31, 0.37]
    noisy = base.source + np.random.default_rng(771_237).normal(0.0, 0.002, base.source.shape)
    results = [
        _refused(source, source.name, source.source)
        for source in (missing, weak, noncosine, disjoint)
    ]
    results.extend((
        _refused(
            base, "censored_first_onset_neighborhood", base.source[3051:], base.reference - 3051
        ),
        _refused(base, "two_independent_arbitrary_stereo_impulse_frames", twice),
        _refused(base, "unmodelled_deterministic_stereo_noise", noisy),
    ))
    return results


def _clean_source_observation() -> dict:
    """Record actual no-nuisance behavior separately from positive acceptance."""
    source = _make_source("exact_two_gate_source_without_nuisance", impulse_frame=None)
    before = source.source.copy()
    fitted = fit_joint_finite_gates(
        source.source, source.frequencies, source.coefficients, source.reference
    )
    assert _same_bits(source.source, before)
    admitted = fitted.gates is not None
    if admitted:
        assert fitted.report["accepted"]
        assert fitted.report["search_complete"]
        assert len(fitted.gates) == len(source.gates)
        for actual, truth in zip(fitted.gates, source.gates, strict=True):
            measured = _landmarks((
                actual.onset_frame,
                actual.rise_frames,
                actual.end_frame,
                actual.release_frames,
            ))
            assert max(abs(a - b) for a, b in zip(measured, _landmarks(truth), strict=True)) <= 1
        peak = float(np.max(np.abs(source.source)))
        assert (
            _error(fitted.multipliers, source.multipliers) / peak
            <= _LIMITS["normalized_complex_multiplier_max_abs_error"]
        )
    else:
        assert not fitted.report["accepted"]
        assert len(fitted.multipliers) == 0
        assert fitted.report["refusal_reason"]
    return {
        "source_kind": source.name,
        "bounded_model_admitted": admitted,
        "included_in_positive_acceptance_count": False,
        "source_preserved_bitwise": True,
        "coverage_limit": (
            "Observed no-nuisance behavior only; this is not evidence for all "
            "equivalent-support cases."
            if admitted
            else "This valid no-nuisance source was refused; its observed cap or support "
            "limitation remains unresolved."
        ),
        "search": _summary(fitted.report),
    }


def _expect_rejection(callback: Callable, label: str) -> str:
    try:
        callback()
    except TypeError, ValueError:
        return label
    message = f"Malformed joint finite-gate contract accepted: {label}"
    raise AssertionError(message)


def _contract_and_memory_controls(base: _Source) -> dict:
    short = np.zeros((17, 2), dtype=np.float64)
    arguments = [short, base.frequencies, base.coefficients, 0]
    cases = (
        ("complex_PCM", 0, short.astype(np.complex128)),
        ("float32_PCM", 0, short.astype(np.float32)),
        ("wrong_stereo_shape", 0, np.zeros((17, 1))),
        ("nonfinite_PCM", 0, np.full((17, 2), np.nan)),
        ("zero_frequency", 1, np.array([0.0, _FREQUENCIES[1]])),
        ("Nyquist_frequency", 1, np.array([_FS / 2, _FREQUENCIES[1]])),
        ("duplicate_frequencies", 1, np.full(2, _FREQUENCIES[0])),
        ("nonfinite_frequency", 1, np.array([np.nan, _FREQUENCIES[1]])),
        ("more_than_two_carriers", 1, np.array([743.27, 1193.81, 1801.13])),
        ("wrong_coefficient_shape", 2, base.coefficients[:, :1]),
        ("nonunit_coefficients", 2, 2.0 * base.coefficients),
        ("nonfinite_coefficients", 2, np.full((2, 2), complex("nan"))),
        ("fractional_reference", 3, 1.5),
        ("boolean_reference", 3, True),
    )
    invalid = []
    for label, index, value in cases:
        altered = arguments.copy()
        altered[index] = value
        invalid.append(
            _expect_rejection(lambda altered=altered: fit_joint_finite_gates(*altered), label)
        )
    backing = np.zeros((1, 2), dtype=np.float64)
    huge = np.broadcast_to(backing, (2_000_000, 2))
    guarded = fit_joint_finite_gates(huge, base.frequencies, base.coefficients, base.reference)
    assert guarded.gates is None
    assert len(guarded.multipliers) == 0
    assert not guarded.report["accepted"]
    assert guarded.report["incomplete_search"]
    assert guarded.report["estimated_peak_working_bytes"] > guarded.report["memory_limit_bytes"]
    assert "plateau_certificate" not in guarded.report
    assert _same_bits(backing, np.zeros((1, 2), dtype=np.float64))
    return {
        "malformed_contracts_rejected": invalid,
        "actual_large_input_is_readonly_zero_stride_broadcast": not huge.flags.writeable
        and huge.strides[0] == 0,
        "actual_large_input_backing_storage_bytes": backing.nbytes,
        "actual_large_input_logical_frames": len(huge),
        "large_input_refused_before_plateau_or_full_basis": _summary(guarded.report),
        "mocked_allocator_fitter_or_modified_resource_threshold": False,
    }


def _linear_samples(source: Real, positions: Real) -> Real:
    output = np.zeros((len(positions), 2))
    lower = np.floor(positions).astype(np.int64)
    fraction = positions - lower
    for neighbor, weights in ((lower, 1.0 - fraction), (lower + 1, fraction)):
        inside = (neighbor >= 0) & (neighbor < len(source))
        output[inside] += weights[inside, None] * source[neighbor[inside]]
    return output


def _integer_tones(source: _Source, positions: Real, rate: float, shift: float) -> Real:
    output = np.zeros((len(positions), 2))
    assert np.all(positions == np.round(positions))
    inside = (positions >= 0) & (positions < len(source.source))
    clock = positions[inside]
    for frequency, coefficients, multiplier, truth in zip(
        source.frequencies, source.coefficients, source.multipliers, source.gates, strict=True
    ):
        birth = truth[0]
        phase = 2.0 * np.pi * frequency / _FS * (birth + shift / rate * (clock - birth))
        vector = coefficients * multiplier
        carrier = np.cos(phase)[:, None] * vector.real - np.sin(phase)[:, None] * vector.imag
        output[inside] += _gate(clock, truth)[:, None] * carrier
    return output


def _integer_render_controls(source: _Source, prepared: Prepared) -> list[dict]:
    records = []
    for semitones in (-7, 0, 7):
        actual, details = render_prepared(prepared, 2.0, semitones)
        residual, residual_details = render_residual(prepared.residual, 2.0, semitones)
        assert details["capture_origin"] == residual_details["capture_origin"]
        positions = 2.0 * (details["capture_origin"] + np.arange(len(actual)))
        expected = _integer_tones(source, positions, 2.0, 2.0 ** (semitones / 12.0)) + residual
        error = _error(actual, expected)
        assert error <= _LIMITS["public_complete_integer_phase_max_abs_error"]
        assert details["fixed_mode_births_source_frames"] == [mode.birth for mode in prepared.modes]
        records.append({
            "rate": 2.0,
            "semitones": semitones,
            "complete_output_frames": len(actual),
            "own_birth_analytic_phase_plus_complete_residual_max_abs_error": error,
        })
    return records


def _renderer_complement_controls(source: _Source, prepared: Prepared) -> dict:
    extra = np.zeros_like(prepared.source)
    extra[0], extra[-1] = [0.11, -0.07], [-0.19, 0.13]
    extra[173:797] = np.random.default_rng(771_893).normal(0.0, 0.013, (624, 2))
    mixed = Prepared(
        prepared.source + extra, prepared.modes, prepared.residual + extra, prepared.report
    )
    identities = []
    for semitones in (-7, 7):
        rate = 2.0 ** (semitones / 12.0)
        actual, details = render_prepared(mixed, rate, semitones)
        positions = rate * (details["capture_origin"] + np.arange(len(actual)))
        error = _error(actual, _linear_samples(mixed.source, positions))
        assert error <= _LIMITS["complete_fractional_identity_max_abs_error"]
        identities.append({
            "semitones": semitones,
            "rate_equals_pitch_ratio": rate,
            "complete_source_linear_identity_max_abs_error": error,
        })
    complete, details = render_prepared(mixed, 2.0, 7)
    residual, residual_details = render_residual(mixed.residual, 2.0, 7)
    assert details["capture_origin"] == residual_details["capture_origin"]
    positions = 2.0 * (details["capture_origin"] + np.arange(len(complete)))
    error = _error(complete, _integer_tones(source, positions, 2.0, 2.0 ** (7 / 12.0)) + residual)
    assert error <= _LIMITS["public_complete_integer_phase_max_abs_error"]
    nominal = (len(mixed.source) + 1) // 2
    tail_energy = float(np.sum(residual[nominal:] ** 2))
    assert len(complete) > nominal
    assert tail_energy > 1.0e-12
    return {
        "fractional_complete_combined_source_identities": identities,
        "nonunity_complete_output_with_residual_tails_max_abs_error": error,
        "nonunity_residual_energy_beyond_nominal_duration": tail_energy,
        "extra_complement_is_renderer_control_not_gate_admission_claim": True,
    }


def _public_pipeline(source: _Source) -> dict:
    before = source.source.copy()
    prepared = prepare(source.source, **_POLICY)
    assert _same_bits(source.source, before)
    assert _same_bits(prepared.source, before)
    assert len(prepared.modes) == len(source.frequencies), prepared.report
    assert prepared.report["model_version"].endswith("v10")
    modes = sorted(prepared.modes, key=lambda mode: mode.frequency_hz)
    frequency_errors, birth_errors, analytic_errors = [], [], []
    peak = float(np.max(np.abs(source.source)))
    for index, mode in enumerate(modes):
        frequency_errors.append(abs(mode.frequency_hz - source.frequencies[index]))
        birth_errors.append(abs(mode.birth - source.gates[index][0]))
        analytic_errors.append(_error(mode.analytic_pcm, source.analytic[index]) / peak)
    assert max(frequency_errors) <= _LIMITS["public_source_frequency_max_abs_error_hz"]
    assert max(birth_errors) <= _LIMITS["integer_landmark_max_abs_error_source_frames"]
    assert max(analytic_errors) <= _LIMITS["public_normalized_whole_analytic_max_abs_error"]
    reconstruction = sum(
        (mode.analytic_pcm.real for mode in prepared.modes), np.zeros_like(source.source)
    )
    expected_residual = source.source - reconstruction
    assert len(prepared.residual) == len(source.source)
    assert _same_bits(prepared.residual, expected_residual)
    closure = _error(reconstruction + prepared.residual, source.source) / peak
    assert closure <= _LIMITS["normalized_whole_complement_max_abs_error"]
    assert source.impulse_frame is not None
    nuisance_error = _error(prepared.residual[source.impulse_frame], source.impulse) / peak
    assert nuisance_error <= _LIMITS["normalized_complex_multiplier_max_abs_error"]
    return {
        "source_kind": source.name,
        "public_preparation_receives_original_PCM_only": True,
        "model_version": prepared.report["model_version"],
        "per_mode_source_frequency_error_hz": frequency_errors,
        "per_mode_birth_error_source_frames": birth_errors,
        "normalized_per_mode_whole_analytic_max_abs_error": analytic_errors,
        "whole_residual_bitwise_equals_source_minus_all_published_tones": True,
        "normalized_nuisance_retention_max_abs_error": nuisance_error,
        "normalized_complete_source_complement_max_abs_error": closure,
        "source_preserved_bitwise": True,
        "integer_rate_whole_output_reference": _integer_render_controls(source, prepared),
        **_renderer_complement_controls(source, prepared),
    }


def publication_guard_controls() -> dict:
    """Exercise real weak-edge search pressure and finite-PCM amplitude overflow."""
    positions = np.arange(_FRAMES, dtype=np.float64)
    gate = _gate(positions, _GATES[0])
    maximum = np.finfo(np.float64).max
    left = (0.8 * maximum) * gate * np.sin(2.0 * np.pi * _FREQUENCIES[0] * positions / _FS)
    pcm = np.column_stack((left, left))
    pcm[_IMPULSE_FRAME] += 0.01 * maximum
    assert np.isfinite(pcm).all()
    before = pcm.copy()
    overflow = fit_joint_finite_gates(
        pcm,
        np.array([_FREQUENCIES[0]]),
        np.array([[2.0**-0.5, 2.0**-0.5]], dtype=np.complex128),
        _REFERENCE,
    )
    assert _same_bits(pcm, before)
    assert overflow.gates is None
    below_budget = _summary(overflow.report)
    # The first .01*MAX stereo impulse is below the whole-source error budget.
    # Retain its observed support-cap refusal and separately exercise a point
    # whose .1*MAX amplitude is analytically above that budget.
    pcm[_IMPULSE_FRAME] += 0.09 * maximum
    assert np.isfinite(pcm).all()
    before = pcm.copy()
    overflow = fit_joint_finite_gates(
        pcm,
        np.array([_FREQUENCIES[0]]),
        np.array([[2.0**-0.5, 2.0**-0.5]], dtype=np.complex128),
        _REFERENCE,
    )
    assert _same_bits(pcm, before)
    assert overflow.gates is None
    assert overflow.report["refusal_reason"] == "unresolved_output_coefficient_dynamic_range"
    frequencies = np.array([20.0, 31.07])
    weak = np.zeros((_FRAMES, 2))
    for index, frequency in enumerate(frequencies):
        weak[:, index] = (
            0.2
            * _gate(positions, (3000, 8, 42000, 8))
            * np.sin(2.0 * np.pi * frequency * (positions - 3004) / _FS)
        )
    weak[_IMPULSE_FRAME] += _IMPULSE
    pressure = fit_joint_finite_gates(weak, frequencies, np.eye(2, dtype=np.complex128), _REFERENCE)
    assert pressure.gates is None
    return {
        "initial_subbudget_impulse_did_not_isolate_scaling_guard": below_budget,
        "finite_original_pcm_with_unrepresentable_modal_amplitude_refused": _summary(
            overflow.report
        ),
        "source_preserved_bitwise": True,
        "weak_carrier_zero_edges_actual_observed_refusal": _summary(pressure.report),
        "joint_flank_cap_observed": pressure.report["refusal_reason"]
        in {"joint_onset_survivor_cap", "joint_release_survivor_cap"},
        "original_quality_sources_targets_and_numeric_limits_unchanged": True,
        "separate_scaling_probe_added_after_subbudget_observation": True,
    }


def _fallback_preparation_control(name: str, source: Real) -> dict:
    previous = prepare(source, **{**_POLICY, "joint_finite_gates": False})
    current = prepare(source, **_POLICY)
    attempt = current.report["carrier_estimation"].get("joint_finite_gates", {})
    assert not attempt.get("accepted", False)
    assert _same_bits(previous.source, current.source)
    assert _same_bits(previous.residual, current.residual)
    assert len(previous.modes) == len(current.modes)
    for old_mode, new_mode in zip(previous.modes, current.modes, strict=True):
        assert old_mode.frequency_hz == new_mode.frequency_hz
        assert old_mode.birth == new_mode.birth
        assert old_mode.phase_law == new_mode.phase_law
        assert np.array_equal(
            old_mode.analytic_pcm.view(np.uint64), new_mode.analytic_pcm.view(np.uint64)
        )
    if name == "two_source_transients_require_real_joint_refusal":
        assert len(current.modes) == 2
        assert attempt["conditional_stationary_gates_and_one_sparse_frame_only"]
    return {
        "source": name,
        "mode_count": len(current.modes),
        "source_modes_births_phase_laws_and_entire_residual_bitwise_unchanged": True,
        "joint_refusal_reason": attempt.get("refusal_reason"),
    }


def fallback_pipeline_controls() -> dict:
    """Compare complete prepared arrays for ineligible and truly refused sources."""
    selected = {"impulse", "mono_900.37", "unseen_chirp"}
    variants = [(case.name, case.source) for case in independent_cases() if case.name in selected]
    assert len(variants) == len(selected)
    source = _make_source("two_source_transients_require_real_joint_refusal").source.copy()
    source[25_013] += (0.19, 0.11)
    variants.append(("two_source_transients_require_real_joint_refusal", source))
    return {
        "stage": "v10_refusal_preserves_v9_complete_prepared_arrays",
        "passed": True,
        "cases": list(starmap(_fallback_preparation_control, variants)),
        "original_quality_sources_targets_and_numeric_limits_unchanged": True,
    }


def integrity_controls(*, include_public: bool = True) -> dict:
    """Check frozen source families, conservative refusals and complete rendering.

    Args:
        include_public: Also exercise PCM-only v10 preparation and the renderer.

    Returns:
        Finite JSON-compatible results with observed refusal reasons and limits.
    """
    base = _make_source("unseen_elliptical_staggered_gates_one_stereo_frame")
    collinear = np.vstack((
        _unit([0.8 * np.exp(0.37j), -0.6 * np.exp(0.37j)]),
        _unit([0.8 * np.exp(-0.21j), -0.6 * np.exp(-0.21j)]),
    ))
    variants = (
        base,
        _make_source("unseen_elliptical_shared_gates", gates=(_GATES[0], _GATES[0])),
        _make_source("unseen_collinear_stereo_staggered_gates", coefficients=collinear),
        _make_source(
            "unrelated_left_and_right_stereo_directions",
            coefficients=np.eye(2, dtype=np.complex128),
        ),
        _make_source("arbitrary_stereo_frame_inside_first_rising_edge", impulse_frame=3029),
        _make_source(
            "both_multipliers_global_phase_shift_0_731",
            multipliers=tuple(value * np.exp(0.731j) for value in _MULTIPLIERS),
        ),
        _translated(base, 137),
    )
    result = {
        "stage": "joint_independent_finite_gate_and_sparse_stereo_frame_integrity_controls",
        "passed": True,
        "predeclared_numeric_acceptance_limits": dict(_LIMITS),
        "independent_dense_flank_partition_bounds": flank_partition_controls(),
        "independently_synthesized_declared_basis_positive_sources": [
            _check_positive(source) for source in variants
        ],
        "positive_finite_source_scale_invariance": [
            _check_positive(base, 2.0**exponent) for exponent in (-400, 400)
        ],
        "unsupported_or_uncertified_source_refusals": _negative_controls(base),
        "separate_no_nuisance_source_observation": _clean_source_observation(),
        "malformed_contracts_and_actual_preallocation_guard": _contract_and_memory_controls(base),
        "bounded_survivor_storage_and_final_scaling_guards": publication_guard_controls(),
        "truth_used_only_for_source_generation_and_expectations": True,
        "fixture_tags_output_targets_private_helpers_or_threshold_tuning_used": False,
        "latent_physics_causality_native_device_product_or_hearing_acceptance_claim": False,
    }
    if include_public:
        result["v9_fallback_bitwise_preservation"] = fallback_pipeline_controls()
        result["PCM_only_preparation_own_births_full_complement_and_complete_render"] = (
            _public_pipeline(base)
        )
    return _finite_json(result)


if __name__ == "__main__":
    emit(integrity_controls())
