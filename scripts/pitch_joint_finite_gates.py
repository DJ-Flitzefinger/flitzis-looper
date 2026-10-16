"""Conditional offline independent gates, joint I/Q and one sparse stereo frame.

Every support and source-grid gate not excluded by a conservative lower bound
must be checked. Resource exhaustion refuses the model. The exact tonal remainder
still contains the sparse frame; this experiment never removes it from playback.
"""

from dataclasses import dataclass
from itertools import product, starmap
from typing import TYPE_CHECKING

import numpy as np
from pitch_finite_am_gate import FiniteSourceGate
from pitch_joint_gate_edges import enumerate_joint_edges
from pitch_joint_gate_plateau import (
    certify_joint_plateau,
    enclose_joint_coefficients,
    possible_sparse_frame_supports,
)
from pitch_mode_carriers import (
    FS,
    MAX_COEFFICIENT_DISTANCE,
    MAX_DESIGN_CONDITION,
    MAX_RELATIVE_FIT_RMS,
)
from pitch_mode_envelopes import MAX_MEMORY_BYTES

if TYPE_CHECKING:
    from numpy.typing import NDArray
    from pitch_joint_gate_plateau import JointPlateauCertificate

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]
type Edges = tuple[tuple[int, int], ...]

MAX_COMBINATIONS = 2_000_000
MAX_FLANK_SURVIVORS = 4096
MAX_DIRECT_CONFIRMATIONS = 4096
CORE_MEMORY_BYTES = 96 * 1024**2
INTERVAL_BLOCK_FRAMES = 256
_EPS = np.finfo(np.float64).eps


@dataclass(slots=True)
class JointFiniteGateFit:
    """A complete conditional source fit or a refusal without published modes."""

    gates: tuple[FiniteSourceGate, ...] | None
    multipliers: Complex
    report: dict


@dataclass(slots=True)
class JointGateProblem:
    """Numeric source intervals; source-family certification belongs to the caller."""

    source: Real
    basis: Real
    certificate: JointPlateauCertificate
    lower: Real
    upper: Real
    budget: float
    norm: float
    report: dict


@dataclass(slots=True)
class _Flank:
    edges: Edges
    total: float
    largest: float


@dataclass(slots=True)
class _ConstantFlank:
    start: int
    stop: int
    costs: Real
    prefixes: Real
    block_maxima: Real
    prefix_gamma: float

    def interval(self, state: int, start: int, stop: int) -> tuple[float, float]:
        start, stop = start - self.start, stop - self.start
        first = (start + INTERVAL_BLOCK_FRAMES - 1) // INTERVAL_BLOCK_FRAMES
        last = stop // INTERVAL_BLOCK_FRAMES
        if first >= last:
            values = self.costs[state, start:stop]
            return float(np.sum(values)), float(np.max(values, initial=0.0))
        left = self.costs[state, start : first * INTERVAL_BLOCK_FRAMES]
        right = self.costs[state, last * INTERVAL_BLOCK_FRAMES : stop]
        before, after = self.prefixes[state, first], self.prefixes[state, last]
        # Both nonnegative prefix magnitudes enter the cancellation bound.
        allowance = np.nextafter(self.prefix_gamma * (abs(before) + abs(after)), np.inf)
        interior = max(0.0, float(np.nextafter(after - before - allowance, -np.inf)))
        return (
            interior + float(np.sum(left)) + float(np.sum(right)),
            max(
                float(np.max(left, initial=0.0)),
                float(np.max(right, initial=0.0)),
                float(np.max(self.block_maxima[state, first:last], initial=0.0)),
            ),
        )


def _initial_report(frames: int) -> dict:
    memory = 512 * frames + CORE_MEMORY_BYTES
    return {
        "accepted": False,
        "status": "refused",
        "refusal_reason": None,
        "conditional_stationary_gates_and_one_sparse_frame_only": True,
        "offline_noncausal_fit": True,
        "physical_source_identification_claim": False,
        "universal_floating_or_libm_certificate_claim": False,
        "source_only_selection": True,
        "instantaneous_pcm_division_used": False,
        "full_exact_transformed_remainder_required": True,
        "nuisance_frame_is_retained_in_exact_residual": True,
        "maximum_nuisance_frames": 1,
        "relative_source_rms_limit": MAX_RELATIVE_FIT_RMS,
        "condition_limit": MAX_DESIGN_CONDITION,
        "landmark_diameter_limit_source_frames": 1,
        "coefficient_diameter_limit_normalized_source": MAX_COEFFICIENT_DISTANCE,
        "maximum_combinations": MAX_COMBINATIONS,
        "maximum_joint_flank_survivors": MAX_FLANK_SURVIVORS,
        "maximum_direct_confirmations": MAX_DIRECT_CONFIRMATIONS,
        "joint_flank_interval_method": "disjoint_constant_segments_and_merged_ramp_interiors",
        "joint_flank_constant_interval_block_frames": INTERVAL_BLOCK_FRAMES,
        "joint_flank_nuisance_subtracted_once_per_observation_union": True,
        "estimated_peak_working_bytes": memory,
        "memory_limit_bytes": MAX_MEMORY_BYTES,
        "search_complete": False,
        "incomplete_search": False,
        "direct_model_confirmations": 0,
        "direct_support_confirmations": 0,
        "paired_evaluations": 0,
        "feasible_models": 0,
        "landmark_ranges": [],
        "coefficient_ranges": [],
        "feasible_nuisance_supports": [],
        "boundary_unresolved": False,
        "all_feasible_joint_designs_passed": True,
        "all_feasible_joint_jacobians_passed": True,
    }


def _refused(report: dict, reason: str, *, incomplete: bool = False) -> JointFiniteGateFit:
    report.update(refusal_reason=reason, incomplete_search=incomplete)
    return JointFiniteGateFit(None, np.empty(0, dtype=np.complex128), report)


def _validate(source: Real, frequencies: Real, coefficients: Complex, reference: int) -> None:
    if (
        not isinstance(source, np.ndarray)
        or source.dtype != np.float64
        or source.ndim != 2
        or source.shape[1] != 2
        or np.iscomplexobj(source)
    ):
        message = "Joint gates require original real float64 stereo PCM"
        raise ValueError(message)
    if (
        frequencies.ndim != 1
        or not 1 <= len(frequencies) <= 2
        or not np.isfinite(frequencies).all()
        or np.any((frequencies <= 0.0) | (frequencies >= FS / 2.0))
        or len(np.unique(frequencies)) != len(frequencies)
    ):
        message = "Joint gates require one or two distinct finite in-band carriers"
        raise ValueError(message)
    if (
        coefficients.shape != (len(frequencies), 2)
        or not np.iscomplexobj(coefficients)
        or not np.isfinite(coefficients).all()
        or np.any(np.abs(np.linalg.norm(coefficients, axis=1) - 1.0) > 64 * _EPS)
    ):
        message = "Joint gates require unit complex stereo directions"
        raise ValueError(message)
    if isinstance(reference, (bool, np.bool_)) or not isinstance(reference, (int, np.integer)):
        message = "The source reference starts on an integer frame"
        raise TypeError(message)


def _basis_values(frames: int, frequencies: Real, coefficients: Complex) -> Real:
    basis = np.empty((frames, 2, 2 * len(frequencies)))
    for index, frequency in enumerate(frequencies):
        carrier = np.exp(2.0j * np.pi * frequency / FS * np.arange(frames))
        stereo = carrier[:, None] * coefficients[index]
        basis[:, :, 2 * index] = stereo.real
        basis[:, :, 2 * index + 1] = -stereo.imag
    return basis


def _coefficient_intervals(basis: Real, lower: Real, upper: Real) -> tuple[Real, Real]:
    left, right = basis * lower, basis * upper
    shape = (*basis.shape[:2], basis.shape[2] // 2, 2)
    return (
        np.minimum(left, right).reshape(shape).sum(axis=3),
        np.maximum(left, right).reshape(shape).sum(axis=3),
    )


def _interval_frame_errors(source: Real, lower: Real, upper: Real) -> Real:
    # Frozen basis samples define this conditional model. Pad subsequent floating
    # arithmetic outward; this is not a certificate for arbitrary libm error.
    pad = 4096 * _EPS * (1.0 + np.abs(source) + np.abs(lower) + np.abs(upper))
    distance = np.maximum(np.maximum(lower - pad - source, source - upper - pad), 0.0)
    return np.sum(distance * distance, axis=1)


def _interval_error(source: Real, lower: Real, upper: Real) -> tuple[float, float]:
    error = _interval_frame_errors(source, lower, upper)
    total = float(np.sum(error))
    allowance = 64 * _EPS * (len(source) + 16) * total
    return max(0.0, total - allowance), float(np.max(error, initial=0.0)) * (1.0 + 64 * _EPS)


def _constant_flank(problem: JointGateProblem, *, release: bool) -> _ConstantFlank:
    start = problem.certificate.end if release else 0
    stop = len(problem.source) if release else problem.certificate.start
    # At K <= 2 retained frame costs use at most 32*F bytes. The two block
    # tables add at most 64*ceil(F/256)+32 bytes, within the existing estimate.
    costs = np.empty((1 << (problem.basis.shape[2] // 2), stop - start))
    for state in range(len(costs)):
        weights = np.array([(state >> index) & 1 for index in range(problem.basis.shape[2] // 2)])
        costs[state] = _interval_frame_errors(
            problem.source[start:stop],
            np.sum(problem.lower[start:stop] * weights[None, None, :], axis=2),
            np.sum(problem.upper[start:stop] * weights[None, None, :], axis=2),
        )
    block_starts = np.arange(0, stop - start, INTERVAL_BLOCK_FRAMES)
    block_sums = np.add.reduceat(costs, block_starts, axis=1)
    prefixes = np.zeros((len(costs), len(block_starts) + 1))
    np.cumsum(block_sums, axis=1, out=prefixes[:, 1:])
    block_maxima = np.maximum.reduceat(costs, block_starts, axis=1)
    # A block reduction has at most B additions, followed by at most M
    # nonnegative block-prefix additions. This is much shorter than a source
    # prefix while still bounding both operations and prefix subtraction.
    gamma = 64 * _EPS * (INTERVAL_BLOCK_FRAMES + len(block_starts) + 16)
    if gamma >= 0.25 or not np.isfinite(costs).all() or not np.isfinite(prefixes).all():
        message = "Unresolved joint flank constant interval arithmetic"
        raise FloatingPointError(message)
    return _ConstantFlank(start, stop, costs, prefixes, block_maxima, gamma / (1.0 - gamma))


def _ramp_intervals(edges: Edges, cache: _ConstantFlank, *, release: bool) -> Edges:
    intervals = []
    for anchor, width in edges:
        start, stop = (anchor - width + 1, anchor) if release else (anchor + 1, anchor + width)
        start, stop = max(cache.start, start), min(cache.stop, stop)
        if start < stop:
            intervals.append((start, stop))
    merged = []
    for start, stop in sorted(intervals):
        if merged and start <= merged[-1][1]:
            merged[-1] = (merged[-1][0], max(merged[-1][1], stop))
        else:
            merged.append((start, stop))
    return tuple(merged)


def _constant_segments(cache: _ConstantFlank, ramps: Edges) -> Edges:
    segments, cursor = [], cache.start
    for start, stop in ramps:
        if cursor < start:
            segments.append((cursor, start))
        cursor = stop
    if cursor < cache.stop:
        segments.append((cursor, cache.stop))
    return tuple(segments)


def _constant_stats(
    cache: _ConstantFlank, edges: Edges, ramps: Edges, *, release: bool
) -> tuple[float, float]:
    total, largest = 0.0, 0.0
    for start, stop in _constant_segments(cache, ramps):
        state = sum(
            1 << index
            for index, (anchor, width) in enumerate(edges)
            if (start <= anchor - width if release else start >= anchor + width)
        )
        segment_total, segment_max = cache.interval(state, start, stop)
        total += segment_total
        largest = max(largest, segment_max)
    return total, largest


def _ramp_errors(problem: JointGateProblem, edges: Edges, ramps: Edges, *, release: bool) -> Real:
    if not ramps:
        return np.empty(0)
    positions = np.concatenate(list(starmap(np.arange, ramps)))
    gates = np.column_stack([
        np.clip((anchor - positions if release else positions - anchor) / width, 0.0, 1.0)
        for anchor, width in edges
    ])
    gates = 0.5 * (1.0 - np.cos(np.pi * gates))
    return _interval_frame_errors(
        problem.source[positions],
        np.sum(problem.lower[positions] * gates[:, None, :], axis=2),
        np.sum(problem.upper[positions] * gates[:, None, :], axis=2),
    )


def _flank_stats(
    problem: JointGateProblem, cache: _ConstantFlank, edges: Edges, *, release: bool
) -> _Flank:
    ramps = _ramp_intervals(edges, cache, release=release)
    total, largest = _constant_stats(cache, edges, ramps, release=release)
    errors = _ramp_errors(problem, edges, ramps, release=release)
    total += float(np.sum(errors))
    largest = max(largest, float(np.max(errors, initial=0.0)))
    largest = float(np.nextafter(largest * (1.0 + 64 * _EPS), np.inf))
    if not np.isfinite(total) or not np.isfinite(largest):
        message = "Nonfinite joint flank observation union"
        raise FloatingPointError(message)
    allowance = 64 * _EPS * (cache.stop - cache.start + 16) * total
    side = "release" if release else "onset"
    problem.report[f"joint_{side}_ramp_frame_evaluations"] += len(errors)
    return _Flank(edges, max(0.0, total - allowance), largest)


def enumerate_joint_flanks(
    problem: JointGateProblem, tables: tuple, *, release: bool
) -> list[_Flank] | None:
    """Enumerate interval-bound candidates without claiming source admission."""
    side = "release" if release else "onset"
    choices = [table[int(release)] for table in tables]
    requested = int(np.prod([len(choice) for choice in choices], dtype=np.int64))
    problem.report[f"requested_joint_{side}_combinations"] = requested
    if requested > MAX_COMBINATIONS:
        problem.report.update(
            refusal_reason=f"joint_{side}_combination_cap", incomplete_search=True
        )
        return None
    problem.report[f"joint_{side}_ramp_frame_evaluations"] = 0
    problem.report[f"joint_{side}_combinations_evaluated"] = 0
    if not requested:
        problem.report[f"joint_{side}_survivors"] = 0
        return []
    try:
        cache = _constant_flank(problem, release=release)
    except FloatingPointError:
        problem.report.update(
            refusal_reason="unresolved_joint_flank_intervals", incomplete_search=True
        )
        return None
    problem.report[f"joint_{side}_constant_state_count"] = len(cache.costs)
    problem.report[f"joint_{side}_constant_cost_frames_precomputed"] = cache.costs.size
    result = []
    for edges in product(*choices):
        try:
            candidate = _flank_stats(problem, cache, edges, release=release)
        except FloatingPointError:
            problem.report.update(
                refusal_reason="unresolved_joint_flank_intervals", incomplete_search=True
            )
            return None
        problem.report[f"joint_{side}_combinations_evaluated"] += 1
        if candidate.total - candidate.largest <= problem.budget**2:
            if len(result) >= MAX_FLANK_SURVIVORS:
                problem.report.update(
                    refusal_reason=f"joint_{side}_survivor_cap", incomplete_search=True
                )
                return None
            result.append(candidate)
    problem.report[f"joint_{side}_survivors"] = len(result)
    return result


def _rank_condition(design: Real) -> tuple[int, float]:
    norms = np.linalg.norm(design, axis=0)
    if np.any(norms == 0.0):
        return 0, float("inf")
    singular = np.linalg.svd(design / norms, compute_uv=False)
    rank = int(np.count_nonzero(singular > singular[0] * max(design.shape) * _EPS))
    return rank, float(singular[0] / singular[-1]) if singular[-1] else float("inf")


def _gate_record(gate: FiniteSourceGate) -> dict:
    return {
        "onset_frame": int(gate.onset_frame),
        "rise_end_frame": int(gate.onset_frame + gate.rise_frames),
        "release_start_frame": int(gate.end_frame - gate.release_frames),
        "end_frame": int(gate.end_frame),
        "rise_frames": int(gate.rise_frames),
        "release_frames": int(gate.release_frames),
    }


def _edge_errors(
    source: Real, error: Real, gates: tuple[FiniteSourceGate, ...]
) -> list[float] | None:
    result = []
    for gate in gates:
        for center in (gate.onset_frame, gate.end_frame):
            if center < 512 or center + 512 >= len(source):
                return None
            window = slice(center - 512, center + 513)
            norm = float(np.linalg.norm(source[window]))
            result.append(float(np.linalg.norm(error[window])) / norm if norm else float("inf"))
    return result


def _gate_derivatives(gate: FiniteSourceGate, positions: Real) -> Real:
    result = np.zeros((len(positions), 4))
    for side, (anchor, width) in enumerate((
        (gate.onset_frame, gate.rise_frames),
        (gate.end_frame, gate.release_frames),
    )):
        t = (anchor - positions if side else positions - anchor) / width
        active = (t > 0.0) & (t < 1.0)
        factor = np.pi / (2.0 * width) * np.sin(np.pi * t[active])
        result[active, 2 * side] = factor if side else -factor
        result[active, 2 * side + 1] = -t[active] * factor
    return result


def _edge_proves_infeasible(
    problem: JointGateProblem, design: Real, gates: tuple[FiniteSourceGate, ...], support: int
) -> bool:
    """Reject only when an unconstrained edge projection already exceeds its budget."""
    for gate in gates:
        for center in (gate.onset_frame, gate.end_frame):
            positions = np.arange(center - 512, center + 513)
            positions = positions[positions != support]
            features = design.reshape(len(problem.source), 2, -1)[positions].reshape(
                -1, design.shape[1]
            )
            features = features[:, np.any(features != 0.0, axis=0)]
            target = problem.source[positions].reshape(-1)
            u, singular, _ = np.linalg.svd(features, full_matrices=False)
            if not len(singular) or singular[-1] <= 0.0:
                continue
            condition = float(singular[0] / singular[-1])
            residual = target - u @ (u.T @ target)
            norm = float(np.linalg.norm(problem.source[center - 512 : center + 513]))
            allowance = (
                4096 * _EPS * max(features.shape) * condition * float(np.linalg.norm(target))
            )
            if float(np.linalg.norm(residual)) - allowance > MAX_RELATIVE_FIT_RMS * norm:
                return True
    return False


def _geometry_perturbation(
    features: Real, derivatives: Real, carrier: Real, beta: Real, radii: Real
) -> float:
    norm, delta = float(np.linalg.norm(beta)), float(np.linalg.norm(radii))
    if delta >= norm:
        return float("inf")
    columns = carrier[:, :, None] * derivatives[:, None, :]
    norms = np.linalg.norm(columns.reshape(-1, 4), axis=0)
    if np.any(norms == 0.0):
        return float("inf")
    return (2.0 * delta / (norm - delta)) ** 2 * float(
        np.sum(np.sum(features * features, axis=(1, 2))[:, None] * derivatives**2 / norms**2)
    )


def _jacobian_condition_enclosure(design: Real, perturbation_squared: float) -> tuple[float, float]:
    normalized = design.reshape(-1, design.shape[2])
    norms = np.linalg.norm(normalized, axis=0)
    if np.any(norms == 0.0):
        return float("inf"), float("inf")
    singular = np.linalg.svd(normalized / norms, compute_uv=False)
    perturbation = float(np.sqrt(perturbation_squared)) + 4096 * _EPS * max(normalized.shape)
    if perturbation >= min(1.0, singular[-1]):
        return perturbation, float("inf")
    condition = float(
        (singular[0] + perturbation)
        / (singular[-1] - perturbation)
        * (1.0 + perturbation)
        / (1.0 - perturbation)
    )
    return perturbation, condition


def _jacobian(
    problem: JointGateProblem,
    gates: tuple[FiniteSourceGate, ...],
    beta: Real,
    support: int,
    radii: Real,
) -> dict:
    spans = [np.arange(problem.certificate.start, problem.certificate.end)]
    for gate in gates:
        spans.extend(
            np.arange(center - 512, center + 513) for center in (gate.onset_frame, gate.end_frame)
        )
    positions = np.unique(np.concatenate(spans))
    positions = positions[positions != support]
    count = len(gates)
    design = np.empty((len(positions), 2, 6 * count))
    informative, perturbation_squared = [], 0.0
    for index, gate in enumerate(gates):
        features = problem.basis[positions, :, 2 * index : 2 * index + 2]
        multiplier = beta[2 * index : 2 * index + 2]
        norm = float(np.linalg.norm(multiplier))
        if norm == 0.0:
            return {"passed": False, "rank": 0, "condition": None}
        carrier = features @ (multiplier / norm)
        derivatives = _gate_derivatives(gate, positions)
        design[:, :, 6 * index : 6 * index + 2] = features * gate.values(positions)[:, None, None]
        design[:, :, 6 * index + 2 : 6 * index + 6] = carrier[:, :, None] * derivatives[:, None, :]
        perturbation_squared += _geometry_perturbation(
            features, derivatives, carrier, multiplier, radii[2 * index : 2 * index + 2]
        )
        informative.extend(
            int(np.count_nonzero((derivatives[:, side] != 0.0) & np.any(carrier != 0.0, axis=1)))
            for side in (0, 2)
        )
    rank, condition = _rank_condition(design.reshape(-1, 6 * count))
    perturbation, condition_upper = _jacobian_condition_enclosure(design, perturbation_squared)
    return {
        "rank": rank,
        "condition": condition if np.isfinite(condition) else None,
        "column_normalized": True,
        "per_carrier_beta_unit_norm": True,
        "nuisance_projected_by_removing_both_frame_rows": True,
        "all_global_feasible_coefficient_box_covered": True,
        "normalized_jacobian_perturbation_bound": perturbation,
        "condition_upper_over_coefficient_box": condition_upper
        if np.isfinite(condition_upper)
        else None,
        "informative_edge_frames": informative,
        "passed": rank == 6 * count
        and condition_upper <= MAX_DESIGN_CONDITION
        and min(informative) >= 2,
    }


def _record_feasible(
    problem: JointGateProblem,
    gates: tuple[FiniteSourceGate, ...],
    beta: Real,
    support: int,
    radii: Real,
) -> None:
    report = problem.report
    report["feasible_models"] += 1
    if support not in report["feasible_nuisance_supports"]:
        report["feasible_nuisance_supports"].append(support)
    for index, gate in enumerate(gates):
        if len(report["landmark_ranges"]) <= index:
            report["landmark_ranges"].append({})
            report["coefficient_ranges"].append({})
        for name, value in _gate_record(gate).items():
            bounds = report["landmark_ranges"][index].setdefault(name, [value, value])
            bounds[0], bounds[1] = min(bounds[0], value), max(bounds[1], value)
        for component, name in enumerate(("real", "imag")):
            value, radius = float(beta[2 * index + component]), float(radii[2 * index + component])
            bounds = report["coefficient_ranges"][index].setdefault(
                name, [float(value), float(value)]
            )
            bounds[0], bounds[1] = min(bounds[0], value - radius), max(bounds[1], value + radius)
        report["boundary_unresolved"] |= (
            gate.onset_frame == 0
            or gate.end_frame == len(problem.source) - 1
            or gate.rise_frames in {3, 512}
            or gate.release_frames in {3, 512}
        )


def _confirm_support(
    problem: JointGateProblem, gates: tuple[FiniteSourceGate, ...], design: Real, support: int
) -> dict | None:
    if problem.report["direct_support_confirmations"] >= MAX_DIRECT_CONFIRMATIONS:
        problem.report.update(
            refusal_reason="direct_support_confirmation_cap", incomplete_search=True
        )
        return None
    problem.report["direct_support_confirmations"] += 1
    keep = np.ones(len(problem.source) * 2, dtype=np.bool_)
    if support >= 0:
        keep[2 * support : 2 * support + 2] = False
    beta, _, _, _ = np.linalg.lstsq(design[keep], problem.source.reshape(-1)[keep], rcond=None)
    rank, condition = _rank_condition(design[keep])
    remainder = problem.source - (design @ beta).reshape(-1, 2)
    error = remainder.copy()
    if support >= 0:
        error[support] = 0.0
    global_error = float(np.linalg.norm(error)) / problem.norm
    if global_error > MAX_RELATIVE_FIT_RMS:
        allowance = 4096 * _EPS * max(design.shape) * condition
        if global_error - allowance <= MAX_RELATIVE_FIT_RMS:
            problem.report["refusal_reason"] = "unresolved_global_coefficient_feasibility"
        return None
    edges = _edge_errors(problem.source, error, gates)
    if edges is None:
        problem.report.update(
            refusal_reason="censored_joint_edge_observation", boundary_unresolved=True
        )
        return None
    if max(edges) > MAX_RELATIVE_FIT_RMS:
        if not _edge_proves_infeasible(problem, design, gates, support):
            problem.report["refusal_reason"] = "unresolved_joint_edge_coefficient_feasibility"
        return None
    frame_keep = keep[0::2]
    enclosure, enclosure_report = enclose_joint_coefficients(
        problem.source[frame_keep],
        design.reshape(len(problem.source), 2, -1)[frame_keep],
        problem.budget,
    )
    if enclosure is None:
        problem.report["refusal_reason"] = enclosure_report["refusal_reason"]
        return None
    radii = np.maximum(beta - enclosure[0], enclosure[1] - beta)
    _record_feasible(problem, gates, beta, support, radii)
    jacobian = _jacobian(problem, gates, beta, support, radii)
    problem.report["all_feasible_joint_designs_passed"] &= (
        rank == design.shape[1] and condition <= MAX_DESIGN_CONDITION
    )
    problem.report["all_feasible_joint_jacobians_passed"] &= jacobian["passed"]
    return {
        "gates": gates,
        "beta": beta,
        "support": support,
        "global_relative_rms": global_error,
        "edge_relative_rms": edges,
        "tonal_only_remainder_relative_rms": float(np.linalg.norm(remainder)) / problem.norm,
        "joint_design_rank": rank,
        "joint_design_condition": condition,
        "joint_geometry_jacobian": jacobian,
    }


def _confirm_model(problem: JointGateProblem, gates: tuple[FiniteSourceGate, ...]) -> dict | None:
    if problem.report["direct_model_confirmations"] >= MAX_DIRECT_CONFIRMATIONS:
        problem.report.update(
            refusal_reason="direct_model_confirmation_cap", incomplete_search=True
        )
        return None
    problem.report["direct_model_confirmations"] += 1
    design = problem.basis.copy()
    for index, gate in enumerate(gates):
        design[:, :, 2 * index : 2 * index + 2] *= gate.values(np.arange(len(design)))[
            :, None, None
        ]
    design = design.reshape(-1, 2 * len(gates))
    supports, record = possible_sparse_frame_supports(
        problem.source, design.reshape(len(problem.source), 2, 2 * len(gates)), problem.budget
    )
    problem.report["last_support_search"] = record
    if supports is None:
        problem.report["refusal_reason"] = record["refusal_reason"]
        return None
    if len(supports) + problem.report["direct_support_confirmations"] > MAX_DIRECT_CONFIRMATIONS:
        problem.report.update(
            refusal_reason="direct_support_confirmation_cap", incomplete_search=True
        )
        return None
    best = None
    for support in supports:
        candidate = _confirm_support(problem, gates, design, support)
        if problem.report["refusal_reason"]:
            return None
        if candidate is not None and (
            best is None or candidate["global_relative_rms"] < best["global_relative_rms"]
        ):
            best = candidate
    return best


def _joint_search(problem: JointGateProblem, tables: tuple) -> dict | None:
    onset, release = enumerate_joint_flanks(problem, tables, release=False), None
    if onset is not None:
        release = enumerate_joint_flanks(problem, tables, release=True)
    if onset is None or release is None:
        return None
    pairs = len(onset) * len(release)
    problem.report["requested_complete_pairs"] = pairs
    if pairs > MAX_COMBINATIONS:
        problem.report.update(refusal_reason="complete_pair_cap", incomplete_search=True)
        return None
    core = slice(problem.certificate.start, problem.certificate.end)
    middle, middle_max = _interval_error(
        problem.source[core], problem.lower[core].sum(axis=2), problem.upper[core].sum(axis=2)
    )
    best = None
    for left, right in product(onset, release):
        problem.report["paired_evaluations"] += 1
        lower = left.total + right.total + middle - max(left.largest, right.largest, middle_max)
        if lower > problem.budget**2:
            continue
        gates = tuple(
            FiniteSourceGate(a, w, e, r)
            for (a, w), (e, r) in zip(left.edges, right.edges, strict=True)
        )
        candidate = _confirm_model(problem, gates)
        if problem.report["refusal_reason"]:
            return None
        if candidate is not None and (
            best is None or candidate["global_relative_rms"] < best["global_relative_rms"]
        ):
            best = candidate
    problem.report["search_complete"] = True
    return best


def _admission_refusal(report: dict) -> str | None:
    names = ("onset_frame", "rise_end_frame", "release_start_frame", "end_frame")
    if any(
        bounds[name][1] - bounds[name][0] > 1
        for bounds in report["landmark_ranges"]
        for name in names
    ):
        return "ambiguous_independent_gate_landmarks"
    diameter = max(
        (
            float(
                np.hypot(
                    values["real"][1] - values["real"][0], values["imag"][1] - values["imag"][0]
                )
            )
            for values in report["coefficient_ranges"]
        ),
        default=float("inf"),
    )
    report["coefficient_diameter_upper_bound"] = diameter
    checks = (
        (report["boundary_unresolved"], "unresolved_source_or_ramp_boundary"),
        (diameter > MAX_COEFFICIENT_DISTANCE, "ambiguous_joint_complex_coefficients"),
        (len(report["feasible_nuisance_supports"]) > 1, "unresolved_sparse_support"),
        (not report["all_feasible_joint_designs_passed"], "unresolved_joint_iq_condition"),
        (not report["all_feasible_joint_jacobians_passed"], "unresolved_joint_geometry_jacobian"),
    )
    return next((reason for failed, reason in checks if failed), None)


def _source_search(pcm: Real, basis: Real, reference_start: int, report: dict) -> dict | None:
    norm = float(np.linalg.norm(pcm))
    budget = MAX_RELATIVE_FIT_RMS * norm
    certificate, plateau_report = certify_joint_plateau(pcm, basis, reference_start, budget)
    report["plateau_certificate"] = plateau_report
    if certificate is None:
        report["refusal_reason"] = "joint_plateau_not_certified"
        return None
    tables, edge_report = enumerate_joint_edges(pcm, basis, certificate, budget)
    report["edge_search"] = edge_report
    if tables is None:
        report.update(
            refusal_reason=edge_report["refusal_reason"],
            incomplete_search=edge_report.get("incomplete_search", False),
        )
        return None
    lower, upper = _coefficient_intervals(basis, certificate.beta_lower, certificate.beta_upper)
    problem = JointGateProblem(pcm, basis, certificate, lower, upper, budget, norm, report)
    return _joint_search(problem, tables)


def _publish_fit(best: dict, peak: float, report: dict) -> JointFiniteGateFit:
    beta, gates = best.pop("beta"), best.pop("gates")
    try:
        with np.errstate(over="raise", invalid="raise", divide="raise"):
            multipliers = (beta[0::2] + 1.0j * beta[1::2]) * peak
            amplitudes = np.abs(multipliers)
    except FloatingPointError:
        return _refused(report, "unresolved_output_coefficient_dynamic_range")
    if (
        not np.isfinite(multipliers).all()
        or not np.isfinite(amplitudes).all()
        or np.any(amplitudes == 0.0)
    ):
        return _refused(report, "unresolved_output_coefficient_dynamic_range")
    multipliers.setflags(write=False)
    report.update(best)
    report.update(
        accepted=True,
        status="admitted",
        selected_gates=[_gate_record(gate) for gate in gates],
        source_peak_normalization=peak,
    )
    return JointFiniteGateFit(gates, multipliers, report)


def fit_joint_finite_gates(
    source: Real, frequencies: Real, coefficients: Complex, reference_start: int
) -> JointFiniteGateFit:
    """Fit the declared finite source family, retaining every unresolved candidate."""
    frequencies = np.asarray(frequencies, dtype=np.float64)
    coefficients = np.asarray(coefficients, dtype=np.complex128)
    _validate(source, frequencies, coefficients, reference_start)
    report = _initial_report(len(source))
    if report["estimated_peak_working_bytes"] > MAX_MEMORY_BYTES:
        return _refused(report, "additional_working_memory_cap", incomplete=True)
    if not np.isfinite(source).all():
        message = "Joint source PCM must be finite"
        raise ValueError(message)
    peak = float(np.max(np.abs(source), initial=0.0))
    if peak == 0.0:
        return _refused(report, "silent_source")
    best = _source_search(
        source / peak,
        _basis_values(len(source), frequencies, coefficients),
        reference_start,
        report,
    )
    if best is None:
        return _refused(
            report,
            report["refusal_reason"] or "no_feasible_joint_source_model",
            incomplete=report["incomplete_search"],
        )
    refusal = _admission_refusal(report)
    if refusal:
        return _refused(report, refusal)
    return _publish_fit(best, peak, report)
