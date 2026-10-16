"""Frozen source-group amplitude replacement, scored with unchanged whole PCM."""

import math
import time
from functools import lru_cache
from typing import TYPE_CHECKING

import numpy as np
from pitch_amfm_model import render as render_residual
from pitch_causal_model_probe import FS, SOURCE_TARGET, emit
from pitch_group_magnitude import replace_group_magnitude
from pitch_group_magnitude_controls import integrity_controls
from pitch_independent_modes import prepare, render_prepared, sample_complex
from pitch_independent_modes_probe import cases, dense_frequency_report
from pitch_overlap_probe import RUN_PROFILES, metric_controls, profile

if TYPE_CHECKING:
    from numpy.typing import NDArray
    from pitch_independent_modes import Prepared

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]

_EPS = np.finfo(np.float64).eps
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


class GroupMagnitudeError(ValueError):
    """Reject a numerically undefined experimental group contribution."""


def _spatial_admission(prepared: Prepared) -> dict:
    report = {"accepted": False, "source_only": True, "projection_roundoff_factor": 64}
    joint = prepared.report.get("carrier_estimation", {}).get("joint_finite_gates", {})
    if not joint.get("accepted") or len(prepared.modes) != 2:
        return {**report, "refusal_reason": "requires_complete_two_carrier_joint_source_fit"}
    if any(mode.phase_law is not None for mode in prepared.modes):
        return {**report, "refusal_reason": "requires_stationary_source_group"}
    direction, bounds = None, []
    for mode in prepared.modes:
        values = mode.analytic_pcm
        peak = max(float(np.max(np.abs(values.real))), float(np.max(np.abs(values.imag))))
        if peak == 0.0:
            return {**report, "refusal_reason": "empty_source_mode"}
        scaled = values / peak
        norms = np.linalg.norm(scaled, axis=1)
        if direction is None:
            pivot = int(np.argmax(norms))
            direction = scaled[pivot] / norms[pivot]
        projected = (scaled @ direction.conj())[:, None] * direction
        error = float(np.max(np.linalg.norm(scaled - projected, axis=1)))
        allowance = 64 * _EPS * float(np.max(norms))
        bounds.append({"maximum_projection_error": error, "roundoff_allowance": allowance})
        if error > allowance:
            return {
                **report,
                "refusal_reason": "source_spatial_directions_not_shared",
                "modes": bounds,
            }
    return {**report, "accepted": True, "modes": bounds, "physical_source_uniqueness_claim": False}


def _group_fields(
    prepared: Prepared, positions: Real, rate: float, shift: float
) -> tuple[tuple[Complex, ...], tuple[Complex, ...]]:
    sources, transported = [], []
    for mode in prepared.modes:
        sampled = sample_complex(mode.analytic_pcm, positions)
        correction = np.exp(
            2.0j * np.pi * mode.frequency_hz / FS * (shift / rate - 1.0) * (positions - mode.birth)
        )
        sources.append(sampled)
        transported.append(sampled * correction[:, None])
    return tuple(sources), tuple(transported)


def _substitution_roundoff_bound(
    baseline: Real, components: tuple[Complex, ...], group: Real
) -> float:
    peaks = [float(np.max(np.abs(baseline))), float(np.max(np.abs(group)))]
    peaks.extend(float(np.max(np.abs(values.real))) for values in components)
    scale = max(peaks)
    if scale == 0.0:
        return 0.0
    unit = _EPS / 2
    gamma2, gamma6 = 2 * unit / (1 - 2 * unit), 6 * unit / (1 - 6 * unit)
    modes = sum(value / scale for value in peaks[2:])
    residual = (peaks[0] / scale + (1 + gamma2) * modes) / (1 - gamma2)
    bound = gamma6 * (residual + modes + peaks[1] / scale) * (1 + 64 * _EPS) * scale
    return float(np.nextafter(bound + 16 * np.nextafter(0.0, 1.0), np.inf))


def render_group_prepared(prepared: Prepared, rate: float, semitones: int) -> tuple[Real, dict]:
    """Apply one declared group operator after the original guards and capture."""
    admission = _spatial_admission(prepared)
    baseline, report = render_prepared(prepared, rate, semitones)
    if not admission["accepted"]:
        return baseline, {**report, "group_magnitude_source_admission": admission}
    shift = 2.0 ** (semitones / 12.0)
    positions = rate * (report["capture_origin"] + np.arange(len(baseline)))
    sources, transported = _group_fields(prepared, positions, rate, shift)
    grouped, replacement = replace_group_magnitude(sources, transported)
    if grouped is None:
        raise GroupMagnitudeError(replacement["refusal_reason"])
    # The baseline already contains every transformed residual sample and tail.
    # Replacing only its complete analytic tonal sum leaves that component in place.
    old_tonal = sum((values.real for values in transported), np.zeros_like(baseline))
    output = baseline + (grouped.real - old_tonal)
    assert np.isfinite(output).all()
    return output, {
        **report,
        "model": "conditional-source-group-magnitude-replacement-v11-experiment",
        "group_magnitude_source_admission": admission,
        "group_magnitude_replacement": replacement,
        "deterministic_time_varying_analytic_amplitude_replacement": True,
        "individual_mode_energy_report_is_baseline_before_replacement": True,
        "grouped_tonal_output_energy": float(np.sum(grouped.real * grouped.real)),
        "whole_response_addition_roundoff_bound": _substitution_roundoff_bound(
            baseline, transported, grouped.real
        ),
        "roundoff_bound_scope": (
            "stored residual plus stored group; additions, not interpolation/libm/kernel"
        ),
        "source_group_amplitude_preservation_implies_real_energy_timing": False,
        "additional_sidebands_or_alias_safety_certified": False,
    }


def addition_controls(prepared: Prepared, rate: float, semitones: int) -> dict:
    """Compare the complete substitution with a freshly transformed exact residual."""
    actual, details = render_group_prepared(prepared, rate, semitones)
    residual, residual_details = render_residual(prepared.residual, rate, semitones)
    assert details["capture_origin"] == residual_details["capture_origin"]
    positions = rate * (details["capture_origin"] + np.arange(len(actual)))
    sources, transported = _group_fields(prepared, positions, rate, 2.0 ** (semitones / 12))
    group, _ = replace_group_magnitude(sources, transported)
    assert group is not None
    expected = residual + group.real
    error = float(np.max(np.abs(actual - expected)))
    bound = details["whole_response_addition_roundoff_bound"]
    assert error <= bound
    tail = np.all(group == 0.0, axis=1)
    for component in transported:
        tail &= np.all(component == 0.0, axis=1)
    assert np.array_equal(actual[tail], residual[tail])
    return {
        "stage": "whole_group_substitution_addition_controls",
        "rate": rate,
        "semitones": semitones,
        "direct_complete_residual_plus_group_error": error,
        "addition_roundoff_bound": bound,
        "zero_tonal_tail_frames": int(np.count_nonzero(tail)),
        "whole_tail_values_equal_to_complete_residual": True,
        "bound_covers_stored_component_additions_only": True,
    }


def _source_controls(fixture, prepared: Prepared, elapsed: float) -> dict:
    admission = _spatial_admission(prepared)
    emit({
        "stage": "group_source_preparation",
        "fixture": fixture.name,
        "elapsed_seconds": elapsed,
        "source_admission": admission,
        "joint_fit": prepared.report["carrier_estimation"].get("joint_finite_gates"),
    })
    if admission["accepted"]:
        emit({"fixture": fixture.name, **addition_controls(prepared, 2.0, -7)})
    return admission


def _run_source(fixture, negatives: dict) -> None:
    started = time.perf_counter()
    prepared = prepare(fixture.source, **_POLICY)
    admission = _source_controls(fixture, prepared, time.perf_counter() - started)
    if not admission["accepted"]:
        negatives["source"].append(fixture.name)

    # Cache by intent; numpy arrays cannot be lru_cache keys.
    @lru_cache(maxsize=1)
    def render_profile(rate: float, semitones: int) -> tuple[Real, dict]:
        return render_group_prepared(prepared, rate, semitones)

    def whole_renderer(source: Real, rate: float, semitones: int) -> tuple[Real, dict]:
        assert np.array_equal(source, prepared.source)
        return render_profile(rate, semitones)

    controls = {}
    for rate, semitones in RUN_PROFILES:
        try:
            record = profile(fixture, rate, semitones, controls, renderer=whole_renderer)
        except GroupMagnitudeError as error:
            emit({
                "stage": "group_phase_refusal",
                "fixture": fixture.name,
                "rate": rate,
                "semitones": semitones,
                "reason": str(error),
            })
            negatives["phase"].append([fixture.name, rate, semitones])
            continue
        output, details = render_profile(rate, semitones)
        if fixture.name == "unseen_close_carriers":
            record["frequency"] = dense_frequency_report(
                output,
                SOURCE_TARGET // rate - details["capture_origin"],
                [frequency * 2.0 ** (semitones / 12.0) for frequency in (521.17, 533.43)],
            )
        record["group_magnitude_source_admission"] = details["group_magnitude_source_admission"]
        record["group_magnitude_replacement"] = details.get("group_magnitude_replacement")
        emit(record)
        if record["isolated_timing_retention_tail_pass"] is False:
            negatives["timing"].append([fixture.name, rate, semitones])
        if "frequency" in record and not record["frequency"]["tonal_15_cent_pass"]:
            negatives["tonal"].append([fixture.name, rate, semitones])
    for rate in (0.75, 1.25, 2.0):
        semitones = 12.0 * math.log2(rate)
        actual, details = render_group_prepared(prepared, rate, semitones)
        positions = rate * (details["capture_origin"] + np.arange(len(actual)))
        expected = sample_complex(prepared.source.astype(np.complex128), positions).real
        error = float(np.max(np.abs(actual - expected)))
        assert error <= 1e-9
        emit({
            "stage": "group_fractional_combined_identity",
            "fixture": fixture.name,
            "rate": rate,
            "maximum_absolute_error": error,
        })


def main() -> None:
    """Measure the frozen first comparison before any broader source admission."""
    emit({
        "stage": "contract",
        "frozen_equation": "norm(Z)*V/norm(V)",
        "amplitude_replacement_is_explicit": True,
        "fixture_inputs_never_enter_renderer": True,
        "original_quality_sources_targets_and_limits_unchanged": True,
        "full_product_gate_closed": False,
    })
    emit(integrity_controls())
    emit(metric_controls())
    selected = {"unseen_close_carriers", "independent_gated_900_1100_plus_impulse"}
    negatives = {"timing": [], "tonal": [], "phase": [], "source": []}
    for fixture in cases():
        if fixture.name in selected:
            emit({"stage": "source_started", "fixture": fixture.name})
            _run_source(fixture, negatives)
    emit({
        "stage": "summary",
        "negatives": negatives,
        "first_comparison_passed": not any(negatives.values()),
        "full_product_gate_closed": False,
    })


if __name__ == "__main__":
    main()
