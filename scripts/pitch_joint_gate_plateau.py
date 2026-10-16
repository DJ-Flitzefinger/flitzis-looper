"""Source-only plateau certificate for a bounded joint finite-gate family.

The caller supplies ungated real IQ columns in a fixed source-coordinate gauge.
The conditional family has one or two independent integer raised-cosine gates,
with rise/release widths in [3, 512], and at most one arbitrary stereo nuisance
frame. This module neither discovers carriers nor identifies a physical model.
All residuals and coefficient bounds use the original source alone.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING

import numpy as np
from pitch_mode_carriers import MAX_DESIGN_CONDITION, WINDOW_FRAMES
from pitch_mode_envelopes import MAX_MEMORY_BYTES

if TYPE_CHECKING:
    from typing import Never

    from numpy.typing import NDArray

type Real = NDArray[np.float64]

BIN_FRAMES = 512
BIN_COUNT = 16
PLATEAU_OFFSET = 3072
PLATEAU_FRAMES = 2048
MIN_RAMP_FRAMES = 3
MAX_RAMP_FRAMES = 512
MAX_CARRIERS = 2
WORKING_MEMORY_BYTES = 32 * 1024 * 1024
MIN_FRAME_MARGIN = 1.0e-8
MAX_PROJECTOR_UNCERTAINTY = 0.01
_EPS = np.finfo(np.float64).eps


@dataclass(frozen=True, slots=True)
class JointPlateauCertificate:
    """An inclusive/exclusive common plateau and all-support IQ enclosure.

    The read-only bounds have length ``2 * K`` and use the caller's interleaved
    real/imaginary multiplier coordinates. They enclose the union of every
    feasible no-deletion and one-whole-stereo-frame deletion ellipsoid.
    """

    start: int
    end: int
    beta_lower: Real
    beta_upper: Real
    report: dict


@dataclass(slots=True)
class _Fit:
    u: Real
    v: Real
    singular: Real
    beta: Real
    inverse_gram: Real
    residual: Real
    sse: float
    target_norm: float
    source_scale: float
    basis_scale: float
    gamma: float
    design_error: float
    sigma_lower: float
    condition: float
    projector_error: float
    residual_error: float
    report: dict


@dataclass(slots=True)
class _Supports:
    sse: Real
    allowance: Real
    centers: Real
    inverse_diagonal_upper: Real
    center_error: Real
    report: dict


@dataclass(slots=True)
class _Geometry:
    u: Real
    v: Real
    singular: Real
    design_error: float
    sigma_lower: float
    condition: float
    projector_error: float
    report: dict


@dataclass(slots=True)
class _FrameDowndates:
    leverage: Real
    eigen_min: Real
    margin: Real
    conditions: Real
    inverse_m: Real
    solved: Real
    correction: Real


class _UnresolvedError(ArithmeticError):
    """A numerical or family boundary that cannot authorize a certificate."""


def _unresolved(reason: str) -> Never:
    raise _UnresolvedError(reason)


def _report(source: Real, basis: Real, reference_start: int, error_budget: float) -> dict:
    return {
        "accepted": False,
        "status": "refused",
        "refusal_reason": None,
        "method": "all_proper_subsets_and_all_whole_stereo_frame_rank_two_deletions",
        "conditional_family_only": True,
        "conditional_family": {
            "carrier_count_bounds": [1, MAX_CARRIERS],
            "independent_integer_raised_cosine_gates": True,
            "ramp_width_bounds_source_frames": [MIN_RAMP_FRAMES, MAX_RAMP_FRAMES],
            "nonoverlapping_rise_and_release_per_gate": True,
            "one_constant_complex_multiplier_per_carrier": True,
            "maximum_arbitrary_nuisance_stereo_frames": 1,
            "global_l2_source_error_budget": error_budget,
        },
        "source_only": True,
        "output_or_scoring_oracle_used": False,
        "physical_model_identification_claim": False,
        "source_mutated": False,
        "missing_or_censored_samples_inferred": False,
        "coefficient_gauge": "supplied ungated Re(C exp), -Im(C exp), interleaved per carrier",
        "phase_origin": "unchanged caller source coordinates",
        "reference_start": reference_start,
        "reference_frames": WINDOW_FRAMES,
        "bin_frames": BIN_FRAMES,
        "bin_count": BIN_COUNT,
        "plateau_start": None,
        "plateau_end_exclusive": None,
        "design_condition_limit": MAX_DESIGN_CONDITION,
        "minimum_frame_downdate_margin": MIN_FRAME_MARGIN,
        "maximum_projector_uncertainty": MAX_PROJECTOR_UNCERTAINTY,
        "roundoff_policy": "outward SSE/center/inverse-Gram allowances; unresolved values refuse",
        "negative_press_sse_policy": "recompute kept residual norm; never clip a negative SSE",
        "proper_subsets_include_empty": True,
        "bin_proof_complete": False,
        "bins": [],
        "all_central_deletion_supports_considered": False,
        "best_support_selection_used": False,
        "proof": {
            "nonplateau_central_carrier_implies_whole_zero_bins": 5,
            "other_carrier_ramps_intersect_at_most_bins": 4,
            "contradiction": "one zero bin is a constant proper subset despite any nuisance frame",
            "conclusion": "every feasible family member is all-plateau on the central 2048 frames",
        },
        "memory_limit_bytes": MAX_MEMORY_BYTES,
        "working_memory_budget_bytes": WORKING_MEMORY_BYTES,
        "estimated_peak_working_bytes": int(
            getattr(source, "nbytes", 0) + getattr(basis, "nbytes", 0) + WORKING_MEMORY_BYTES
        ),
    }


def _validate_arrays(source: Real, basis: Real) -> int:
    arrays = (source, basis)
    if any(not isinstance(value, np.ndarray) or np.ma.isMaskedArray(value) for value in arrays):
        _unresolved("missing_censored_or_nonarray_input")
    if any(value.dtype != np.float64 for value in arrays):
        _unresolved("original_float64_source_and_basis_required")
    if source.ndim != 2 or source.shape[1] != 2:
        _unresolved("stereo_source_shape_required")
    if basis.ndim != 3 or basis.shape[:2] != source.shape:
        _unresolved("complete_source_aligned_stereo_iq_basis_required")
    columns = basis.shape[2]
    if columns not in {2, 4}:
        _unresolved("one_or_two_interleaved_iq_carriers_required")
    for start in range(0, len(source), WINDOW_FRAMES):
        stop = start + WINDOW_FRAMES
        if not np.isfinite(source[start:stop]).all() or not np.isfinite(basis[start:stop]).all():
            _unresolved("missing_or_nonfinite_source_or_basis")
    return columns // 2


def _validate_problem(
    source: Real, basis: Real, reference_start: int, error_budget: float, report: dict
) -> int:
    if report["estimated_peak_working_bytes"] > MAX_MEMORY_BYTES:
        _unresolved("memory_cap_exceeded")
    carriers = _validate_arrays(source, basis)
    if isinstance(reference_start, (bool, np.bool_)) or not isinstance(
        reference_start, (int, np.integer)
    ):
        _unresolved("integer_reference_start_required")
    if reference_start < 0 or reference_start + WINDOW_FRAMES > len(source):
        _unresolved("missing_or_censored_8192_frame_reference")
    if isinstance(error_budget, (bool, np.bool_)) or not isinstance(
        error_budget, (int, float, np.integer, np.floating)
    ):
        _unresolved("finite_nonnegative_scalar_error_budget_required")
    if not 0.0 <= error_budget <= np.finfo(np.float64).max:
        _unresolved("finite_nonnegative_scalar_error_budget_required")
    report["carrier_count"] = carriers
    return carriers


def _empty_fit(target: Real, source_scale: float, gamma: float) -> _Fit:
    rows = len(target)
    empty = np.empty(0, dtype=np.float64)
    norm = float(np.linalg.norm(target))
    return _Fit(
        np.empty((rows, 0), dtype=np.float64),
        np.empty((0, 0), dtype=np.float64),
        empty,
        empty,
        np.empty((0, 0), dtype=np.float64),
        target.reshape(-1, 2),
        float(target @ target),
        norm,
        source_scale,
        1.0,
        gamma,
        0.0,
        float("inf"),
        1.0,
        0.0,
        gamma * norm,
        {"rank": 0, "columns": 0, "condition_upper": 1.0, "empty_subset": True},
    )


def _factor_geometry(design: Real, gamma: float) -> _Geometry:
    u, singular, vh = np.linalg.svd(design, full_matrices=False)
    identity = np.eye(design.shape[1])
    u_error = float(np.linalg.norm(u.T @ u - identity)) + gamma
    v_error = float(np.linalg.norm(vh @ vh.T - identity)) + gamma
    reconstruction = float(np.linalg.norm(design - (u * singular) @ vh))
    design_error = reconstruction + gamma * float(np.linalg.norm(design))
    design_error += 4.0 * (u_error + v_error) * float(singular[0])
    sigma_lower = float(singular[-1]) - design_error
    if sigma_lower <= 0.0:
        _unresolved("rank_unresolved_with_roundoff")
    condition = (float(singular[0]) + design_error) / sigma_lower
    if not np.isfinite(condition) or condition > MAX_DESIGN_CONDITION:
        _unresolved("constant_iq_design_condition_limit")
    projector_error = 16.0 * (gamma + design_error / sigma_lower + u_error + v_error)
    if projector_error >= MAX_PROJECTOR_UNCERTAINTY:
        _unresolved("projector_roundoff_unresolved")
    record = {
        "rank": design.shape[1],
        "columns": design.shape[1],
        "condition_upper": condition,
        "normalized_singular_minimum_lower": sigma_lower,
        "normalized_design_backward_error_upper": design_error,
        "left_orthogonality_error_upper": u_error,
        "right_orthogonality_error_upper": v_error,
        "projector_error_upper": projector_error,
        "normalized_residual_norm_error_upper": None,
        "floating_operation_allowance": gamma,
    }
    return _Geometry(
        u, vh.T, singular, design_error, sigma_lower, condition, projector_error, record
    )


def _solve_factor(
    geometry: _Geometry, target: Real, source_scale: float, basis_scale: float, gamma: float
) -> _Fit:
    projection = geometry.u.T @ target
    beta = geometry.v @ (projection / geometry.singular)
    residual = target - geometry.u @ projection
    target_norm = float(np.linalg.norm(target))
    residual_error = 8.0 * (gamma + geometry.projector_error) * target_norm
    inverse = (geometry.v / geometry.singular**2) @ geometry.v.T
    geometry.report["normalized_residual_norm_error_upper"] = residual_error
    return _Fit(
        geometry.u,
        geometry.v,
        geometry.singular,
        beta,
        inverse,
        residual.reshape(-1, 2),
        float(residual @ residual),
        target_norm,
        source_scale,
        basis_scale,
        gamma,
        geometry.design_error,
        geometry.sigma_lower,
        geometry.condition,
        geometry.projector_error,
        residual_error,
        geometry.report,
    )


def _factor(source: Real, basis: Real) -> _Fit:
    rows, columns = source.size, basis.shape[2]
    gamma = 128.0 * _EPS * (rows + columns + 1)
    source_scale = float(np.max(np.abs(source), initial=0.0)) or 1.0
    target = (source / source_scale).reshape(-1)
    if not columns:
        return _empty_fit(target, source_scale, gamma)
    basis_scale = float(np.max(np.abs(basis), initial=0.0))
    if basis_scale == 0.0:
        _unresolved("rank_deficient_constant_iq_design")
    design = (basis / basis_scale).reshape(rows, columns)
    geometry = _factor_geometry(design, gamma)
    return _solve_factor(geometry, target, source_scale, basis_scale, gamma)


def _stable_deleted_sse(fit: _Fit, correction: Real, solved: Real) -> tuple[Real, int, int]:
    deleted = fit.sse - correction
    unstable = deleted <= 1024.0 * _EPS * (fit.sse + correction)
    negative_count = int(np.count_nonzero(deleted < 0.0))
    frame_u = fit.u.reshape(len(fit.residual), 2, -1)
    for frame in np.flatnonzero(unstable):
        adjustment = frame_u[frame].T @ solved[frame]
        residual = fit.residual + (fit.u @ adjustment).reshape(-1, 2)
        residual[frame] = 0.0
        deleted[frame] = float(np.sum(residual * residual))
    if not np.isfinite(deleted).all() or np.any(deleted < 0.0):
        _unresolved("negative_or_nonfinite_deleted_sse_unresolved")
    return deleted, int(np.count_nonzero(unstable)), negative_count


def _support_roundoff(fit: _Fit, correction: Real, eigen_min: Real, margin: Real) -> Real:
    residual_norm = float(np.linalg.norm(fit.residual))
    frame_norm = np.linalg.norm(fit.residual, axis=1)
    delta = fit.residual_error
    full_error = 2.0 * residual_norm * delta + delta**2 + fit.gamma * fit.sse
    residual_error = (2.0 * frame_norm * delta + delta**2) / margin
    inverse_error = frame_norm**2 * fit.projector_error / (eigen_min * margin)
    arithmetic_error = 16.0 * fit.gamma * (fit.sse + correction) / margin
    allowance = np.concatenate((
        [full_error],
        full_error + residual_error + inverse_error + arithmetic_error,
    ))
    return np.nextafter(allowance, np.inf)


def _coefficient_downdates(
    fit: _Fit, inverse_m: Real, solved: Real, eigen_min: Real, margin: Real
) -> tuple[Real, Real, Real]:
    frames, columns = len(fit.residual), len(fit.beta)
    if not columns:
        empty = np.empty((frames + 1, 0), dtype=np.float64)
        return empty, empty, np.zeros(frames + 1, dtype=np.float64)
    frame_u = fit.u.reshape(frames, 2, columns)
    influence = np.einsum("ij,nkj->nik", fit.v / fit.singular, frame_u)
    centers = fit.beta - np.einsum("npi,ni->np", influence, solved)
    inverse_diagonal = np.diag(fit.inverse_gram) + np.einsum(
        "npi,nij,npj->np", influence, inverse_m, influence
    )
    centers = np.vstack((fit.beta, centers))
    inverse_diagonal = np.vstack((np.diag(fit.inverse_gram), inverse_diagonal))
    margins = np.concatenate(([1.0], margin))
    computed_margins = np.concatenate(([1.0], eigen_min))
    sigma = float(fit.singular[-1]) * np.sqrt(computed_margins)
    perturbation = fit.design_error / sigma
    if np.any(perturbation >= 1.0):
        _unresolved("deleted_inverse_gram_perturbation_unresolved")
    inverse_error = 64.0 * (fit.gamma + fit.projector_error) / (sigma**2 * margins)
    inverse_upper = (inverse_diagonal + inverse_error[:, None]) / (1.0 - perturbation[:, None]) ** 2
    center_error = (
        32.0
        * (fit.gamma + fit.projector_error + perturbation)
        * fit.target_norm
        / (fit.sigma_lower * np.sqrt(margins) * margins)
    )
    if np.any(inverse_upper <= 0.0):
        _unresolved("deleted_inverse_gram_diagonal_unresolved")
    return centers, np.nextafter(inverse_upper, np.inf), np.nextafter(center_error, np.inf)


def _frame_downdates(fit: _Fit) -> _FrameDowndates:
    frames, columns = len(fit.residual), len(fit.beta)
    frame_u = fit.u.reshape(frames, 2, columns)
    leverage = frame_u @ np.swapaxes(frame_u, 1, 2)
    m = np.eye(2)[None, :, :] - leverage
    eigenvalues, vectors = np.linalg.eigh(m)
    eigen_min = eigenvalues[:, 0]
    margin = eigen_min - fit.projector_error - 16.0 * fit.gamma
    if np.any(margin <= MIN_FRAME_MARGIN):
        _unresolved("whole_frame_leverage_margin_unresolved")
    conditions = fit.condition / np.sqrt(margin)
    if np.any(conditions > MAX_DESIGN_CONDITION):
        _unresolved("deleted_constant_iq_design_condition_limit")
    inverse_m = (vectors / eigenvalues[:, None, :]) @ np.swapaxes(vectors, 1, 2)
    solved = np.einsum("nij,nj->ni", inverse_m, fit.residual)
    correction = np.einsum("ni,ni->n", fit.residual, solved)
    if np.any(correction < 0.0):
        _unresolved("negative_press_correction_unresolved")
    return _FrameDowndates(leverage, eigen_min, margin, conditions, inverse_m, solved, correction)


def _supports(fit: _Fit) -> _Supports:
    downdates = _frame_downdates(fit)
    deleted, recomputed, negative = _stable_deleted_sse(fit, downdates.correction, downdates.solved)
    allowance = _support_roundoff(fit, downdates.correction, downdates.eigen_min, downdates.margin)
    centers, inverse_upper, center_error = _coefficient_downdates(
        fit, downdates.inverse_m, downdates.solved, downdates.eigen_min, downdates.margin
    )
    sse = np.concatenate(([fit.sse], deleted))
    arrays = (sse, allowance, centers, inverse_upper, center_error)
    if any(not np.isfinite(value).all() for value in arrays):
        _unresolved("nonfinite_all_support_roundoff_bounds")
    record = {
        "support_count": len(fit.residual) + 1,
        "whole_stereo_frame_deletion_count": len(fit.residual),
        "no_deletion_included": True,
        "minimum_frame_downdate_margin_lower": float(np.min(downdates.margin)),
        "maximum_frame_leverage_trace": float(
            np.max(np.trace(downdates.leverage, axis1=1, axis2=2))
        ),
        "maximum_deleted_design_condition_upper": float(np.max(downdates.conditions)),
        "maximum_normalized_sse_roundoff_allowance": float(np.max(allowance)),
        "maximum_normalized_center_roundoff_allowance": float(np.max(center_error)),
        "cancellation_recomputed_support_count": recomputed,
        "raw_negative_press_support_count": negative,
        "negative_sse_clipping_used": False,
    }
    return _Supports(sse, allowance, centers, inverse_upper, center_error, record)


def _budget_squared(error_budget: float, source_scale: float) -> float:
    normalized = error_budget / source_scale
    if not np.isfinite(normalized) or normalized > np.sqrt(np.finfo(np.float64).max):
        _unresolved("normalized_error_budget_dynamic_range_unresolved")
    if normalized == 0.0:
        if error_budget != 0.0:
            _unresolved("normalized_error_budget_underflow")
        return 0.0
    upper = float(np.nextafter(normalized, np.inf))
    squared = upper * upper
    if squared == 0.0:
        _unresolved("squared_error_budget_underflow")
    return float(np.nextafter(squared, np.inf))


def _bin_subset(source: Real, basis: Real, subset: tuple[int, ...], error_budget: float) -> dict:
    columns = [column for carrier in subset for column in (2 * carrier, 2 * carrier + 1)]
    fit = _factor(source, basis[:, :, columns])
    supports = _supports(fit)
    lower = np.nextafter(supports.sse - supports.allowance, -np.inf)
    budget_squared = _budget_squared(error_budget, fit.source_scale)
    minimum = float(np.min(lower))
    return {
        "carriers": list(subset),
        "source_scale": fit.source_scale,
        "basis_scale": fit.basis_scale,
        "minimum_constant_iq_sse_normalized": float(np.min(supports.sse)),
        "robust_sse_lower_normalized": minimum,
        "error_budget_squared_upper_normalized": budget_squared,
        "strictly_excluded": minimum > budget_squared,
        "factorization": fit.report,
        "deletions": supports.report,
    }


def _certify_bins(
    source: Real, basis: Real, reference_start: int, error_budget: float, report: dict
) -> None:
    carriers = basis.shape[2] // 2
    subsets = [
        tuple(index for index in range(carriers) if mask & (1 << index))
        for mask in range((1 << carriers) - 1)
    ]
    for index in range(BIN_COUNT):
        start = reference_start + index * BIN_FRAMES
        end = start + BIN_FRAMES
        record = {"index": index, "start": start, "end_exclusive": end, "proper_subsets": []}
        report["bins"].append(record)
        for subset in subsets:
            result = _bin_subset(source[start:end], basis[start:end], subset, error_budget)
            record["proper_subsets"].append(result)
            if not result["strictly_excluded"]:
                _unresolved("proper_subset_compatible_with_budget_or_roundoff_in_reference_bin")
    report["bin_proof_complete"] = True


def _physical_bounds(fit: _Fit, lower: Real, upper: Real) -> tuple[Real, Real]:
    ratio = fit.source_scale / fit.basis_scale
    if not np.isfinite(ratio) or ratio <= 0.0:
        _unresolved("coefficient_coordinate_scaling_unresolved")
    conversion_error = 8.0 * _EPS * ratio * np.maximum(np.abs(lower), np.abs(upper))
    lower = np.nextafter(lower * ratio - conversion_error, -np.inf)
    upper = np.nextafter(upper * ratio + conversion_error, np.inf)
    if not np.isfinite(lower).all() or not np.isfinite(upper).all():
        _unresolved("nonfinite_coefficient_enclosure")
    lower.setflags(write=False)
    upper.setflags(write=False)
    return lower, upper


def _enclose_plateau(
    source: Real, basis: Real, error_budget: float, report: dict
) -> tuple[Real, Real]:
    fit = _factor(source, basis)
    supports = _supports(fit)
    sse_lower = np.nextafter(supports.sse - supports.allowance, -np.inf)
    budget_squared = _budget_squared(error_budget, fit.source_scale)
    feasible = sse_lower <= budget_squared
    count = int(np.count_nonzero(feasible))
    report["all_central_deletion_supports_considered"] = True
    report["plateau_factorization"] = fit.report
    report["plateau_deletions"] = supports.report
    report["plateau_deletions"].update(
        feasible_or_numerically_possible_support_count=count,
        provably_infeasible_support_count=len(supports.sse) - count,
        minimum_normalized_sse=float(np.min(supports.sse)),
        minimum_normalized_sse_lower=float(np.min(sse_lower)),
        error_budget_squared_upper_normalized=budget_squared,
        source_scale=fit.source_scale,
        basis_scale=fit.basis_scale,
    )
    if not count:
        _unresolved("no_feasible_central_plateau_support")
    radii = np.sqrt(
        (budget_squared - sse_lower[feasible, None]) * supports.inverse_diagonal_upper[feasible]
    )
    lower = np.min(
        supports.centers[feasible] - supports.center_error[feasible, None] - radii, axis=0
    )
    upper = np.max(
        supports.centers[feasible] + supports.center_error[feasible, None] + radii, axis=0
    )
    return _physical_bounds(fit, lower, upper)


def enclose_joint_coefficients(
    source: Real, basis: Real, error_budget: float
) -> tuple[tuple[Real, Real] | None, dict]:
    """Enclose all fixed-support IQ vectors within the complete source budget.

    Only rows retained after the caller's explicit nuisance projection are given;
    this certificate removes no further nuisance frame.
    """
    report = {"refusal_reason": None}
    try:
        fit = _factor(source, basis)
        residual_lower = max(0.0, float(np.sqrt(fit.sse)) - fit.residual_error)
        radius_squared = _budget_squared(error_budget, fit.source_scale) - residual_lower**2
        if radius_squared < 0.0:
            _unresolved("fixed_support_coefficient_budget_infeasible")
        perturbation = fit.design_error / float(fit.singular[-1])
        inverse_error = 64.0 * (fit.gamma + fit.projector_error) / fit.sigma_lower**2
        inverse = (np.diag(fit.inverse_gram) + inverse_error) / (1.0 - perturbation) ** 2
        center_error = (
            32.0
            * (fit.gamma + fit.projector_error + perturbation)
            * fit.target_norm
            / fit.sigma_lower
        )
        radii = np.sqrt(radius_squared * inverse) + center_error
        lower, upper = _physical_bounds(fit, fit.beta - radii, fit.beta + radii)
    except (_UnresolvedError, np.linalg.LinAlgError, FloatingPointError, MemoryError) as error:
        report["refusal_reason"] = str(error) or type(error).__name__
        return None, report
    report.update(
        factorization=fit.report,
        all_global_feasible_coefficients_enclosed=True,
        no_additional_nuisance_frame_removed=True,
    )
    return (lower, upper), report


def possible_sparse_frame_supports(
    source: Real, basis: Real, error_budget: float
) -> tuple[list[int] | None, dict]:
    """Retain all one-frame supports using the same conservative SVD arithmetic.

    A returned support of -1 denotes no nuisance. This is a necessary source-fit
    test; the caller must recompute surviving complete original-PCM fits directly.
    """
    report = {"refusal_reason": None, "all_source_frame_supports_examined": len(source)}
    try:
        if 512 * len(source) + 96 * 1024**2 > MAX_MEMORY_BYTES:
            _unresolved("sparse_support_working_memory_cap")
        _validate_arrays(source, basis)
        if not np.isfinite(error_budget) or error_budget < 0.0:
            _unresolved("finite_nonnegative_scalar_error_budget_required")
        with np.errstate(over="raise", invalid="raise", divide="raise"):
            fit = _factor(source, basis)
            supports = _supports(fit)
            lower = np.nextafter(supports.sse - supports.allowance, -np.inf)
            possible = np.flatnonzero(lower <= _budget_squared(error_budget, fit.source_scale))
            result = (possible - 1).tolist()
    except (_UnresolvedError, np.linalg.LinAlgError, FloatingPointError, MemoryError) as error:
        report["refusal_reason"] = str(error) or type(error).__name__
        return None, report
    report.update(
        possible_supports=len(result),
        factorization=fit.report,
        deletions=supports.report,
        source_scale=fit.source_scale,
        basis_scale=fit.basis_scale,
        direct_original_pcm_confirmation_required=True,
    )
    return result, report


def certify_joint_plateau(
    source: Real, basis: Real, reference_start: int, error_budget: float
) -> tuple[JointPlateauCertificate | None, dict]:
    """Certify a common plateau and enclose every one-frame IQ fit.

    Args:
        source: Original finite float64 PCM with shape (frames, 2), never modified.
        basis: Finite float64 ungated IQ columns with shape (frames, 2, 2 * K).
            Columns are Re(C exp), -Im(C exp), interleaved for one or two carriers.
        reference_start: Integer start of an available, uncensored 8192-frame span.
        error_budget: Nonnegative global source L2 error, including all ordinary
            model mismatch but excluding at most one arbitrary stereo frame.

    Returns:
        A conditional certificate with read-only coordinate bounds, or None and
        an explicit refusal report. All bin subsets and central deletion supports
        must be resolved before acceptance; no source/output truth is consulted.
    """
    report = _report(source, basis, reference_start, error_budget)
    try:
        _validate_problem(source, basis, reference_start, error_budget, report)
        with np.errstate(over="raise", invalid="raise", divide="raise"):
            _certify_bins(source, basis, reference_start, error_budget, report)
            start = int(reference_start) + PLATEAU_OFFSET
            end = start + PLATEAU_FRAMES
            report.update(plateau_start=start, plateau_end_exclusive=end)
            lower, upper = _enclose_plateau(
                source[start:end], basis[start:end], error_budget, report
            )
    except (_UnresolvedError, np.linalg.LinAlgError, FloatingPointError, MemoryError) as error:
        report["refusal_reason"] = str(error) or type(error).__name__
        return None, report
    report.update(
        accepted=True,
        status="certified_conditional_joint_plateau",
        beta_lower=lower.tolist(),
        beta_upper=upper.tolist(),
        coefficient_bounds_read_only=True,
    )
    return JointPlateauCertificate(start, end, lower, upper, report), report
