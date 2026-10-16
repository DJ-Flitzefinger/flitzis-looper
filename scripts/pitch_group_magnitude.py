"""Bounded offline replacement of a complex stereo group's analytic magnitude.

The caller supplies already complex-linearly sampled source contributions and
their independently phase-pitched counterparts. This deterministic, time-varying
amplitude replacement can create additional sidebands. It does not identify a
physical source family, fit output gains, or establish transport quality.
"""

import math
from typing import TYPE_CHECKING

import numpy as np
from pitch_mode_envelopes import MAX_MEMORY_BYTES

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]

_EPS = np.finfo(np.float64).eps
_TINY = np.finfo(np.float64).tiny
_MAX_FLOAT = np.finfo(np.float64).max
_PHASE_ROUNDOFF = 64.0 * _EPS
_MEMORY_RESERVE_BYTES = 1024 * 1024


def _validate_shapes(source: tuple[Complex, ...], transported: tuple[Complex, ...]) -> int:
    if (
        not isinstance(source, tuple)
        or not isinstance(transported, tuple)
        or not 1 <= len(source) <= 2
        or len(source) != len(transported)
    ):
        message = "Group magnitude requires paired tuples of one or two components"
        raise ValueError(message)
    shape = None
    for component in (*source, *transported):
        if (
            not isinstance(component, np.ndarray)
            or component.dtype != np.complex128
            or component.ndim != 2
            or component.shape[1] != 2
        ):
            message = "Every group component must be a complex128 array with shape (frames, 2)"
            raise ValueError(message)
        if shape is not None and component.shape != shape:
            message = "Every source and transported component must have the same shape"
            raise ValueError(message)
        shape = component.shape
    return source[0].shape[0]


def _initial_report(frames: int, components: int) -> dict:
    # Logical input storage is 64*N*K bytes. The 384*N allowance covers both
    # sums, retained norms, normalized component scratch, masks, phase, output,
    # and overlapping real/imag norm and range-validation temporaries.
    return {
        "accepted": False,
        "status": "refused",
        "refusal_reason": None,
        "frames": frames,
        "component_count": components,
        "offline_numeric_operator_only": True,
        "operator": "G=norm_stereo_complex(sum(z_k))*sum(v_k)/norm_stereo_complex(sum(v_k))",
        "stereo_complex_norm": "hypot(hypot(real(left),imag(left)),hypot(real(right),imag(right)))",
        "amplitude_replacement": True,
        "deterministic_time_varying_analytic_amplitude_replacement": True,
        "fitted_wet_parameters": False,
        "additional_sidebands_expected": True,
        "instantaneous_real_pcm_division_used": False,
        "epsilon_regularization_used": False,
        "phase_offsets_or_mode_weights_used": False,
        "physical_source_admission_performed": False,
        "caller_stationary_same_spatial_group_admission_required": True,
        "caller_baseline_nyquist_check_required": True,
        "full_exact_transformed_remainder_required": True,
        "source_components_complex_sampled_before_norm_required": True,
        "full_field_support_preserved": True,
        "identity_special_case_used": False,
        "estimated_peak_working_bytes": frames * (64 * components + 384) + _MEMORY_RESERVE_BYTES,
        "memory_estimate_formula": "N*(64*K+384)+1048576; logical inputs, output and scratch",
        "memory_limit_bytes": MAX_MEMORY_BYTES,
        "normalization_common_peak": None,
        "normalization_peak_definition": "common max(abs(real),abs(imag)) over every input value",
        "normalization_power_of_two_scale": None,
        "normalization_scale_definition": "highest power of two at or below the common input peak",
        "normalized_subnormal_inputs_refused": True,
        "exact_binary_normalization_required": True,
        "normalization_lost_nonzero_components": 0,
        "normalization_subnormal_components": 0,
        "normalization_inexact_components": 0,
        "phase_guard_ratio": _PHASE_ROUNDOFF,
        "phase_guard_definition": "norm(V) <= 64*eps*sum(norm(v_k)) at source-active frames",
        "phase_guard_is_relative_floating_resolution_only": True,
        "source_active_frames": 0,
        "transported_active_frames": 0,
        "source_zero_and_transport_zero_frames": frames,
        "undefined_phase_frames": 0,
        "uncertain_phase_frames": 0,
        "minimum_phase_margin_ratio": None,
        "minimum_phase_guard_multiple": None,
        "maximum_source_norm_normalized": 0.0,
        "maximum_transported_norm_normalized": 0.0,
        "maximum_transported_component_norm_sum_normalized": 0.0,
        "unrepresentable_amplitude_frames": 0,
        "unrepresentable_output_components": 0,
    }


def _input_peak(source: tuple[Complex, ...], transported: tuple[Complex, ...]) -> float:
    peak = 0.0
    for component in (*source, *transported):
        if not np.isfinite(component).all():
            message = "Group components must contain only finite complex128 values"
            raise ValueError(message)
        peak = max(
            peak,
            float(np.max(np.abs(component.real), initial=0.0)),
            float(np.max(np.abs(component.imag), initial=0.0)),
        )
    return peak


def _stereo_norm(values: Complex) -> Real:
    # A cancellation sum can be subnormal even when every nonzero normalized
    # summand is normal. Hypot retains its nonzero magnitude without squaring.
    with np.errstate(under="ignore"):
        return np.hypot(
            np.hypot(values.real[:, 0], values.imag[:, 0]),
            np.hypot(values.real[:, 1], values.imag[:, 1]),
        )


def _normalized_sum(
    components: tuple[Complex, ...], scale: float, report: dict, *, role: str
) -> tuple[Complex, Real] | None:
    total = np.zeros(components[0].shape, dtype=np.complex128)
    norm_sum = np.zeros(len(total))
    for index, component in enumerate(components):
        normalized = np.empty(component.shape, dtype=np.complex128)
        # Real division avoids complex division's unnecessary reciprocal and
        # cross-products. Binary scaling is exact for accepted input components;
        # each normalized real/imag value has absolute magnitude below two.
        with np.errstate(under="ignore"):
            np.divide(component.real, scale, out=normalized.real)
            np.divide(component.imag, scale, out=normalized.imag)
        lost = 0
        subnormal = 0
        inexact = 0
        for original, scaled in (
            (component.real, normalized.real),
            (component.imag, normalized.imag),
        ):
            lost += int(np.count_nonzero((original != 0.0) & (scaled == 0.0)))
            subnormal += int(np.count_nonzero((scaled != 0.0) & (np.abs(scaled) < _TINY)))
            # A rounded subnormal can become TINY, so the range test alone is
            # insufficient. Exact round-trip scaling detects that boundary too.
            with np.errstate(under="ignore", over="ignore"):
                inexact += int(np.count_nonzero(scaled * scale != original))
        if lost or subnormal or inexact:
            report.update(
                refusal_reason="normalization_range_loss",
                normalization_failure_role=role,
                normalization_failure_component_index=index,
                normalization_lost_nonzero_components=lost,
                normalization_subnormal_components=subnormal,
                normalization_inexact_components=inexact,
            )
            return None
        total += normalized
        norm_sum += _stereo_norm(normalized)
    return total, norm_sum


def _phase_resolved(amplitude: Real, norm: Real, component_norm_sum: Real, report: dict) -> bool:
    active = amplitude > 0.0
    ratios = np.zeros(len(norm))
    with np.errstate(under="ignore"):
        np.divide(norm, component_norm_sum, out=ratios, where=component_norm_sum > 0.0)
        phase_bound = _PHASE_ROUNDOFF * component_norm_sum
    undefined = int(np.count_nonzero(active & (norm == 0.0)))
    uncertain = int(np.count_nonzero(active & (norm > 0.0) & (norm <= phase_bound)))
    minimum = float(np.min(ratios[active])) if np.any(active) else None
    report.update(
        source_active_frames=int(np.count_nonzero(active)),
        transported_active_frames=int(np.count_nonzero(norm > 0.0)),
        source_zero_and_transport_zero_frames=int(np.count_nonzero(~active & (norm == 0.0))),
        undefined_phase_frames=undefined,
        uncertain_phase_frames=uncertain,
        minimum_phase_margin_ratio=minimum,
        minimum_phase_guard_multiple=minimum / _PHASE_ROUNDOFF if minimum is not None else None,
        maximum_source_norm_normalized=float(np.max(amplitude, initial=0.0)),
        maximum_transported_norm_normalized=float(np.max(norm, initial=0.0)),
        maximum_transported_component_norm_sum_normalized=float(
            np.max(component_norm_sum, initial=0.0)
        ),
    )
    if undefined or uncertain:
        report["refusal_reason"] = (
            "undefined_transported_phase" if undefined else "uncertain_transported_phase"
        )
        return False
    return True


def _publish(
    amplitude: Real, transported: Complex, norm: Real, scale: float, report: dict
) -> Complex | None:
    if scale > 1.0:
        report["unrepresentable_amplitude_frames"] = int(
            np.count_nonzero(amplitude > _MAX_FLOAT / scale)
        )
        if report["unrepresentable_amplitude_frames"]:
            report["refusal_reason"] = "unrepresentable_output_amplitude"
            return None
    with np.errstate(under="ignore"):
        physical_amplitude = amplitude * scale
    report["unrepresentable_amplitude_frames"] = int(
        np.count_nonzero((amplitude > 0.0) & (physical_amplitude == 0.0))
    )
    if report["unrepresentable_amplitude_frames"]:
        report["refusal_reason"] = "unrepresentable_output_amplitude"
        return None
    # Normalize before multiplying, so a large A/norm(V) ratio never overflows.
    nonzero = norm[:, None] > 0.0
    output = np.zeros_like(transported)
    with np.errstate(under="ignore"):
        np.divide(transported.real, norm[:, None], out=output.real, where=nonzero)
        np.divide(transported.imag, norm[:, None], out=output.imag, where=nonzero)
    for original, result in ((transported.real, output.real), (transported.imag, output.imag)):
        with np.errstate(under="ignore"):
            np.multiply(result, physical_amplitude[:, None], out=result)
        report["unrepresentable_output_components"] += int(
            np.count_nonzero(
                (original != 0.0) & (physical_amplitude[:, None] > 0.0) & (result == 0.0)
            )
        )
    if report["unrepresentable_output_components"] or not np.isfinite(output).all():
        report["refusal_reason"] = "unrepresentable_output_component"
        return None
    report.update(accepted=True, status="accepted")
    return output


def replace_group_magnitude(
    source_components: tuple[Complex, ...], transported_components: tuple[Complex, ...]
) -> tuple[Complex | None, dict]:
    """Replace the transported group's analytic norm over the complete input field.

    Args:
        source_components: One or two finite complex128 (frames, 2) source arrays,
            already complex-linearly sampled at the caller's source coordinates.
        transported_components: Corresponding independently phase-pitched arrays
            with the same shape. Inputs are never mutated.

    Returns:
        A fresh complex128 stereo array and a numeric-operator report, or None
        with an explicit resource, numeric range, or transported-phase refusal.
        A zero source norm produces zero, including when the transported norm
        is also zero. A positive source norm requires resolved transported phase.
        Malformed or nonfinite inputs raise ValueError. Source-family admission,
        baseline Nyquist checks and full residual transformation belong to the
        caller; this function does not certify those model assumptions.
    """
    frames = _validate_shapes(source_components, transported_components)
    report = _initial_report(frames, len(source_components))
    if report["estimated_peak_working_bytes"] > MAX_MEMORY_BYTES:
        report["refusal_reason"] = "memory_limit"
        return None, report
    peak = _input_peak(source_components, transported_components)
    report["normalization_common_peak"] = peak
    if peak == 0.0:
        report.update(accepted=True, status="accepted")
        return np.zeros((frames, 2), dtype=np.complex128), report
    scale = math.ldexp(1.0, math.frexp(peak)[1] - 1)
    report["normalization_power_of_two_scale"] = scale
    source = _normalized_sum(source_components, scale, report, role="source")
    if source is None:
        return None, report
    transported = _normalized_sum(transported_components, scale, report, role="transported")
    if transported is None:
        return None, report
    amplitude, norm = _stereo_norm(source[0]), _stereo_norm(transported[0])
    if not _phase_resolved(amplitude, norm, transported[1], report):
        return None, report
    return _publish(amplitude, transported[0], norm, scale, report), report
