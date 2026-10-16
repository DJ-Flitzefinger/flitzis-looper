"""Conditional offline finite gates for one already certified positive-AM family.

Frequency, complex stereo direction, modulator and a clean plateau restriction
are supplied by the caller. This module discovers none of them and makes no
claim that an unknown physical source has this isolated model. Its sole output
is a source-coordinate gate and one complex multiplier. The caller must keep
every subtraction error in the fully transformed exact PCM remainder.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING

import numpy as np
from pitch_mode_carriers import (
    FS,
    MAX_COEFFICIENT_DISTANCE,
    MAX_DESIGN_CONDITION,
    MAX_RELATIVE_FIT_RMS,
    WINDOW_FRAMES,
)
from pitch_mode_envelopes import MAX_MEMORY_BYTES
from pitch_source_modulation import SourceModulator, modulation_values, validate_modulators

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Integer = NDArray[np.int64]

EDGE_MARGIN = 512
MIN_RAMP_FRAMES = 3
MAX_RAMP_FRAMES = 512
MAX_EDGE_SURVIVORS = 4096
MAX_PAIRED_EVALUATIONS = 2_000_000
SEARCH_BATCH = 512
BLOCK_FRAMES = 8192
CORE_MEMORY_BYTES = 64 * 1024 * 1024
_EPS = np.finfo(np.float64).eps


@dataclass(frozen=True, slots=True)
class FiniteSourceGate:
    """An integer source-grid raised-cosine rise, plateau and release.

    The onset and end samples are exactly zero. ``rise_frames`` and
    ``release_frames`` count source-frame intervals, not sample counts.
    """

    onset_frame: int
    rise_frames: int
    end_frame: int
    release_frames: int

    def __post_init__(self) -> None:
        values = (self.onset_frame, self.rise_frames, self.end_frame, self.release_frames)
        if any(
            isinstance(value, (bool, np.bool_)) or not isinstance(value, (int, np.integer))
            for value in values
        ):
            message = "Finite gate landmarks and widths must be integers"
            raise ValueError(message)
        if (
            self.rise_frames <= 0
            or self.release_frames <= 0
            or self.onset_frame + self.rise_frames > self.end_frame - self.release_frames
        ):
            message = "Finite gate needs positive widths and a nonoverlapping plateau"
            raise ValueError(message)

    def values(self, positions: Real | Integer) -> Real:
        """Evaluate the gate at caller-supplied source positions without extrapolation."""
        positions = np.asarray(positions)
        rise = np.clip((positions - self.onset_frame) / self.rise_frames, 0.0, 1.0)
        release = np.clip((self.end_frame - positions) / self.release_frames, 0.0, 1.0)
        return 0.5 * (1.0 - np.cos(np.pi * np.minimum(rise, release)))


@dataclass(slots=True)
class FiniteGateFit:
    """A complete conditional fit, or a refusal with no admitted component."""

    gate: FiniteSourceGate | None
    multiplier: complex
    report: dict


@dataclass(slots=True)
class _Edge:
    start: int
    stop: int
    positions: Integer
    design: Real
    target: Real
    q: Real
    h: Real
    energy: float


@dataclass(slots=True)
class _Table:
    landmarks: Integer
    grams: Real
    targets: Real


@dataclass(slots=True)
class _Basis:
    frequency_hz: float
    coefficients: NDArray[np.complex128]
    modulator: SourceModulator

    def values(self, positions: Integer) -> Real:
        carrier = np.exp(2.0j * np.pi * self.frequency_hz / FS * positions)
        stereo = carrier[:, None] * self.coefficients[None, :]
        features = np.stack((stereo.real, -stereo.imag), axis=2)
        features *= modulation_values(positions, (self.modulator,))[:, :, None]
        return features


@dataclass(slots=True)
class _Problem:
    source: Real
    basis: _Basis
    peak: float
    left: _Edge
    right: _Edge
    reference_start: int
    middle: tuple[Real, Real, float]


@dataclass(slots=True)
class _SearchResult:
    best: dict | None = None
    multipliers: list[complex] | None = None
    design_failed: bool = False
    jacobian_failed: bool = False


def _report(frames: int) -> dict:
    estimate = 128 * frames + CORE_MEMORY_BYTES
    return {
        "accepted": False,
        "status": "refused",
        "refusal_reason": None,
        "conditional_isolated_model_only": True,
        "offline_only": True,
        "physical_model_identification_claim": False,
        "unknown_model_mismatch_bound_claim": False,
        "complete_pipeline_causality_claim": False,
        "source_certification_required_from_caller": True,
        "fixed_global_phase_origin_frame": 0,
        "one_complex_multiplier_for_entire_source": True,
        "instantaneous_pcm_division_used": False,
        "source_only_selection": True,
        "full_exact_transformed_remainder_required": True,
        "relative_source_rms_limit": MAX_RELATIVE_FIT_RMS,
        "design_condition_limit": MAX_DESIGN_CONDITION,
        "complex_multiplier_distance_limit": MAX_COEFFICIENT_DISTANCE,
        "landmark_diameter_limit_source_frames": 1,
        "reference_frames": WINDOW_FRAMES,
        "edge_margin_frames": EDGE_MARGIN,
        "ramp_width_bounds": [MIN_RAMP_FRAMES, MAX_RAMP_FRAMES],
        "maximum_edge_survivors": MAX_EDGE_SURVIVORS,
        "maximum_paired_evaluations": MAX_PAIRED_EVALUATIONS,
        "maximum_search_batch": SEARCH_BATCH,
        "memory_estimate_bytes": estimate,
        "estimated_peak_working_bytes": estimate,
        "memory_limit_bytes": MAX_MEMORY_BYTES,
        "core_memory_budget_bytes": CORE_MEMORY_BYTES,
        "full_source_feature_array_allocated": False,
        "search_complete": False,
        "incomplete_search": False,
        "edge_candidate_counts": {"onset": 0, "release": 0},
        "edge_survivor_counts": {"onset": 0, "release": 0},
        "paired_evaluations": 0,
        "requested_paired_evaluations": 0,
        "direct_global_confirmations": 0,
        "feasible_model_pairs": 0,
        "landmark_ranges": {},
        "multiplier_ranges_normalized_source": {},
        "maximum_normalized_complex_multiplier_distance": None,
        "all_feasible_models_retained_in_counts_and_ranges": True,
        "boundary_unresolved": False,
        "selected_gate": None,
        "multiplier": None,
    }


def _refused(report: dict, reason: str, *, incomplete: bool = False) -> FiniteGateFit:
    report["refusal_reason"] = reason
    report["incomplete_search"] = incomplete
    return FiniteGateFit(None, 0.0j, report)


def _validate(source: Real, basis: _Basis, reference_start: int) -> None:
    if (
        not isinstance(source, np.ndarray)
        or source.dtype != np.float64
        or source.ndim != 2
        or source.shape[1] != 2
        or np.iscomplexobj(source)
    ):
        message = "Finite gate fitting requires original real float64 stereo PCM"
        raise ValueError(message)
    if not np.isfinite(basis.frequency_hz) or not 0.0 < basis.frequency_hz < FS / 2.0:
        message = "Finite gate fitting requires one finite in-band carrier frequency"
        raise ValueError(message)
    coefficients = basis.coefficients
    if (
        not isinstance(coefficients, np.ndarray)
        or coefficients.shape != (2,)
        or not np.iscomplexobj(coefficients)
        or not np.isfinite(coefficients).all()
        or abs(float(np.linalg.norm(coefficients)) - 1.0) > 64.0 * _EPS
    ):
        message = "Finite gate fitting requires one unit-norm finite complex stereo direction"
        raise ValueError(message)
    if isinstance(reference_start, (bool, np.bool_)) or not isinstance(
        reference_start, (int, np.integer)
    ):
        message = "The clean plateau reference must start on an integer source frame"
        raise TypeError(message)
    validate_modulators(np.array([basis.frequency_hz]), (basis.modulator,))


def _source_extent(source: Real) -> tuple[float, int, int] | None:
    peak, first, last = 0.0, len(source), -1
    for start in range(0, len(source), BLOCK_FRAMES):
        block = source[start : start + BLOCK_FRAMES]
        if not np.isfinite(block).all():
            message = "Finite gate source PCM must contain only finite samples"
            raise ValueError(message)
        peak = max(peak, float(np.max(np.abs(block), initial=0.0)))
        active = np.flatnonzero(np.any(block != 0.0, axis=1))
        if len(active):
            first = min(first, start + int(active[0]))
            last = start + int(active[-1])
    return (peak, first, last) if last >= 0 else None


def _statistics(features: Real, target: Real) -> tuple[Real, Real, float]:
    a, b = features[:, :, 0], features[:, :, 1]
    q = np.column_stack((np.sum(a * a, axis=1), np.sum(a * b, axis=1), np.sum(b * b, axis=1)))
    h = np.column_stack((np.sum(a * target, axis=1), np.sum(b * target, axis=1)))
    return q, h, float(np.sum(target * target))


def _edge(source: Real, basis: _Basis, peak: float, center: int) -> _Edge:
    start, stop = center - EDGE_MARGIN, center + EDGE_MARGIN + 1
    positions = np.arange(start, stop, dtype=np.int64)
    features = basis.values(positions)
    target = source[start:stop] / peak
    q, h, energy = _statistics(features, target)
    return _Edge(start, stop, positions, features, target, q, h, energy)


def _rank_condition(design: Real) -> tuple[int, float]:
    singular = np.linalg.svd(design, compute_uv=False)
    if not len(singular) or singular[0] == 0.0:
        return 0, float("inf")
    rank = int(np.count_nonzero(singular > _EPS * max(design.shape) * singular[0]))
    condition = float(singular[0] / singular[-1]) if singular[-1] else float("inf")
    return rank, condition


def _reference(source: Real, basis: _Basis, peak: float, start: int) -> dict:
    positions = np.arange(start, start + WINDOW_FRAMES, dtype=np.int64)
    design = basis.values(positions).reshape(-1, 2)
    target = (source[start : start + WINDOW_FRAMES] / peak).ravel()
    beta, _, rank, singular = np.linalg.lstsq(design, target, rcond=None)
    energy = float(np.linalg.norm(target))
    condition = float(singular[0] / singular[-1]) if singular[-1] else float("inf")
    relative = float(np.linalg.norm(target - design @ beta) / energy) if energy else None
    return {
        "start_frame": int(start),
        "end_frame_inclusive": int(start + WINDOW_FRAMES - 1),
        "declared_clean_plateau_restriction": True,
        "constant_iq_rank": int(rank),
        "constant_iq_condition": condition if np.isfinite(condition) else None,
        "constant_iq_relative_source_rms": relative,
        "constant_iq_multiplier_normalized_source": beta.tolist(),
        "passed": bool(
            rank == 2
            and condition <= MAX_DESIGN_CONDITION
            and relative is not None
            and relative <= MAX_RELATIVE_FIT_RMS
        ),
    }


def _middle_statistics(
    source: Real, basis: _Basis, peak: float, left: _Edge, right: _Edge
) -> tuple[Real, Real, float]:
    gram, target, energy = np.zeros(3), np.zeros(2), 0.0
    for start in range(0, len(source), BLOCK_FRAMES):
        stop = min(start + BLOCK_FRAMES, len(source))
        normalized = source[start:stop] / peak
        energy += float(np.sum(normalized * normalized))
        lo, hi = max(start, left.stop), min(stop, right.start)
        if lo < hi:
            features = basis.values(np.arange(lo, hi, dtype=np.int64))
            q, h, _ = _statistics(features, normalized[lo - start : hi - start])
            gram += np.sum(q, axis=0)
            target += np.sum(h, axis=0)
    return gram, target, energy


def _normal_lower_bound(grams: Real, targets: Real, energy: float, terms: int) -> tuple[Real, Real]:
    """Return provisional minima and a deliberately conservative rounding allowance.

    E-H'G^-1H is never a final quality test. Perturbation allowances grow with
    the inverse Gram eigenvalue, and unresolved Gram systems cannot prune at
    all. Every survivor is recomputed from actual sample residuals using SVD.
    """
    matrices = np.empty((len(grams), 2, 2))
    matrices[:, 0, 0], matrices[:, 0, 1] = grams[:, 0], grams[:, 1]
    matrices[:, 1, 0], matrices[:, 1, 1] = grams[:, 1], grams[:, 2]
    eigenvalues = np.linalg.eigvalsh(matrices)
    trace = np.maximum(grams[:, 0] + grams[:, 2], 0.0)
    gamma = 64.0 * _EPS * (terms + 32)
    gram_error = gamma * trace
    stable = eigenvalues[:, 0] > 4.0 * gram_error
    beta = np.zeros_like(targets)
    if np.any(stable):
        beta[stable] = np.linalg.solve(matrices[stable], targets[stable, :, None])[:, :, 0]
    explained = np.sum(targets * beta, axis=1)
    bound = np.full(len(grams), np.inf)
    minimum = np.maximum(eigenvalues[stable, 0], np.finfo(np.float64).tiny)
    h_norm = np.linalg.norm(targets[stable], axis=1)
    h_error = gamma * np.sqrt(energy * trace[stable])
    bound[stable] = gamma * (energy + np.abs(explained[stable])) + 8.0 * (
        h_norm**2 * gram_error[stable] / minimum**2
        + h_norm * h_error / minimum
        + h_error**2 / minimum
    )
    return energy - explained, bound


def _weights(edge: _Edge, anchors: Integer, widths: Integer, *, release: bool) -> Real:
    positions = edge.positions[None, :]
    progress = (
        (anchors[:, None] - positions) if release else (positions - anchors[:, None])
    ) / widths[:, None]
    np.clip(progress, 0.0, 1.0, out=progress)
    progress *= np.pi
    np.cos(progress, out=progress)
    progress *= -0.5
    progress += 0.5
    return progress


def _empty_table() -> _Table:
    return _Table(np.empty((0, 2), dtype=np.int64), np.empty((0, 3)), np.empty((0, 2)))


def _anchor_range(edge: _Edge, *, release: bool) -> range:
    center = (edge.start + edge.stop - 1) // 2
    return (
        range(center, center + EDGE_MARGIN)
        if release
        else range(center - EDGE_MARGIN + 1, center + 1)
    )


def _provisional(grams: Real, targets: Real, energy: float, terms: int) -> Integer:
    minima, allowance = _normal_lower_bound(grams, targets, energy, terms)
    return np.flatnonzero(minima <= MAX_RELATIVE_FIT_RMS**2 * energy + allowance)


def _edge_supports(edge: _Edge, weights: Real) -> bool:
    design = (weights[:, None, None] * edge.design).reshape(-1, 2)
    beta = np.linalg.lstsq(design, edge.target.ravel(), rcond=None)[0]
    error = float(np.linalg.norm(edge.target.ravel() - design @ beta))
    return error <= MAX_RELATIVE_FIT_RMS * np.sqrt(edge.energy)


def _edge_table(edge: _Edge, reference_start: int, report: dict, *, release: bool) -> _Table | None:
    name = "release" if release else "onset"
    records, grams, targets = [], [], []
    widths = np.arange(MIN_RAMP_FRAMES, MAX_RAMP_FRAMES + 1, dtype=np.int64)
    for anchor in _anchor_range(edge, release=release):
        allowed = (
            anchor - widths >= reference_start + WINDOW_FRAMES - 1
            if release
            else anchor + widths <= reference_start
        )
        trial_widths = widths[allowed]
        if not len(trial_widths):
            continue
        weights = _weights(
            edge, np.full(len(trial_widths), anchor, dtype=np.int64), trial_widths, release=release
        )
        trial_grams = (weights * weights) @ edge.q
        trial_targets = weights @ edge.h
        report["edge_candidate_counts"][name] += len(trial_widths)
        for index in _provisional(trial_grams, trial_targets, edge.energy, len(edge.positions)):
            if not _edge_supports(edge, weights[index]):
                continue
            records.append((anchor, int(trial_widths[index])))
            grams.append(trial_grams[index].copy())
            targets.append(trial_targets[index].copy())
            report["edge_survivor_counts"][name] += 1
            if len(records) > MAX_EDGE_SURVIVORS:
                return None
    if not records:
        return _empty_table()
    return _Table(np.asarray(records, dtype=np.int64), np.asarray(grams), np.asarray(targets))


def _global_iq(
    source: Real, basis: _Basis, peak: float, gate: FiniteSourceGate
) -> tuple[Real, int, float]:
    """Compress the actual full design with blocked QR, then solve using SVD."""
    reduced = np.empty((0, 3))
    for start in range(0, len(source), BLOCK_FRAMES):
        stop = min(start + BLOCK_FRAMES, len(source))
        positions = np.arange(start, stop, dtype=np.int64)
        design = basis.values(positions)
        design *= gate.values(positions)[:, None, None]
        augmented = np.column_stack((design.reshape(-1, 2), (source[start:stop] / peak).ravel()))
        local = np.linalg.qr(augmented, mode="r")
        reduced = np.linalg.qr(np.vstack((reduced, local)), mode="r")
    beta, _, _, _ = np.linalg.lstsq(reduced[:, :2], reduced[:, 2], rcond=None)
    rank, condition = _rank_condition(reduced[:, :2])
    return beta, rank, condition


def _direct_errors(
    source: Real,
    basis: _Basis,
    peak: float,
    gate: FiniteSourceGate,
    beta: Real,
    left: _Edge,
    right: _Edge,
    energy: float,
) -> dict:
    errors = {"global": 0.0, "onset_edge": 0.0, "release_edge": 0.0}
    for start in range(0, len(source), BLOCK_FRAMES):
        stop = min(start + BLOCK_FRAMES, len(source))
        positions = np.arange(start, stop, dtype=np.int64)
        model = (basis.values(positions) @ beta) * gate.values(positions)[:, None]
        residual = source[start:stop] / peak - model
        squared = np.sum(residual * residual, axis=1)
        errors["global"] += float(np.sum(squared))
        for name, edge in (("onset_edge", left), ("release_edge", right)):
            lo, hi = max(start, edge.start), min(stop, edge.stop)
            if lo < hi:
                errors[name] += float(np.sum(squared[lo - start : hi - start]))
    norms = {"global": energy, "onset_edge": left.energy, "release_edge": right.energy}
    return {name: float(np.sqrt(value / norms[name])) for name, value in errors.items()}


def _gate_derivatives(gate: FiniteSourceGate, positions: Integer) -> Real:
    derivatives = np.zeros((len(positions), 4))
    rise_t = (positions - gate.onset_frame) / gate.rise_frames
    release_t = (gate.end_frame - positions) / gate.release_frames
    rising, falling = (rise_t > 0.0) & (rise_t < 1.0), (release_t > 0.0) & (release_t < 1.0)
    rise_factor = np.pi / (2.0 * gate.rise_frames) * np.sin(np.pi * rise_t[rising])
    release_factor = np.pi / (2.0 * gate.release_frames) * np.sin(np.pi * release_t[falling])
    derivatives[rising, 0], derivatives[rising, 1] = -rise_factor, -rise_t[rising] * rise_factor
    derivatives[falling, 2] = release_factor
    derivatives[falling, 3] = -release_t[falling] * release_factor
    return derivatives


def _jacobian(
    basis: _Basis,
    gate: FiniteSourceGate,
    beta: Real,
    left: _Edge,
    right: _Edge,
    reference_start: int,
) -> dict:
    reference = np.arange(reference_start, reference_start + WINDOW_FRAMES, dtype=np.int64)
    positions = np.unique(np.concatenate((left.positions, reference, right.positions)))
    features = basis.values(positions)
    carrier = features @ (beta / np.linalg.norm(beta))
    derivatives = _gate_derivatives(gate, positions)
    design = np.empty((len(positions), 2, 6))
    design[:, :, :2] = features * gate.values(positions)[:, None, None]
    design[:, :, 2:] = carrier[:, :, None] * derivatives[:, None, :]
    rank, condition = _rank_condition(design.reshape(-1, 6))
    informative = np.any(carrier != 0.0, axis=1)
    counts = {
        "onset": int(np.count_nonzero((derivatives[:, 0] != 0.0) & informative)),
        "release": int(np.count_nonzero((derivatives[:, 2] != 0.0) & informative)),
    }
    return {
        "rank": rank,
        "condition": condition if np.isfinite(condition) else None,
        "source_beta_unit_norm_for_amplitude_invariance": True,
        "deduplicated_source_frames": len(positions),
        "informative_interior_source_frames": counts,
        "passed": bool(
            rank == 6 and condition <= MAX_DESIGN_CONDITION and min(counts.values()) >= 2
        ),
    }


def _gate_record(gate: FiniteSourceGate) -> dict:
    return {
        "onset_frame": int(gate.onset_frame),
        "rise_end_frame": int(gate.onset_frame + gate.rise_frames),
        "release_start_frame": int(gate.end_frame - gate.release_frames),
        "end_frame": int(gate.end_frame),
        "rise_frames": int(gate.rise_frames),
        "release_frames": int(gate.release_frames),
    }


def _record_feasible(report: dict, gate: FiniteSourceGate, beta: Real) -> None:
    report["feasible_model_pairs"] += 1
    for name, value in _gate_record(gate).items():
        bounds = report["landmark_ranges"].setdefault(name, {"min": value, "max": value})
        bounds["min"], bounds["max"] = min(bounds["min"], value), max(bounds["max"], value)
    values = {
        "real": float(beta[0]),
        "imag": float(beta[1]),
        "magnitude": float(np.linalg.norm(beta)),
    }
    for name, value in values.items():
        bounds = report["multiplier_ranges_normalized_source"].setdefault(
            name, {"min": value, "max": value}
        )
        bounds["min"], bounds["max"] = min(bounds["min"], value), max(bounds["max"], value)


def _landmarks_equivalent(report: dict) -> bool:
    names = ("onset_frame", "rise_end_frame", "release_start_frame", "end_frame")
    return all(
        report["landmark_ranges"][name]["max"] - report["landmark_ranges"][name]["min"] <= 1
        for name in names
    )


def _boundary_unresolved(report: dict, first: int, last: int) -> bool:
    ranges = report["landmark_ranges"]
    limits = {
        "onset_frame": (first - EDGE_MARGIN + 1, first),
        "end_frame": (last, last + EDGE_MARGIN - 1),
        "rise_frames": (MIN_RAMP_FRAMES, MAX_RAMP_FRAMES),
        "release_frames": (MIN_RAMP_FRAMES, MAX_RAMP_FRAMES),
    }
    return any(
        ranges[name]["min"] == lo or ranges[name]["max"] == hi for name, (lo, hi) in limits.items()
    )


def _pair_candidate(problem: _Problem, gate: FiniteSourceGate, report: dict) -> dict | None:
    beta, rank, condition = _global_iq(problem.source, problem.basis, problem.peak, gate)
    report["direct_global_confirmations"] += 1
    errors = _direct_errors(
        problem.source,
        problem.basis,
        problem.peak,
        gate,
        beta,
        problem.left,
        problem.right,
        problem.middle[2],
    )
    if max(errors.values()) > MAX_RELATIVE_FIT_RMS:
        return None
    return {
        "gate": gate,
        "beta": beta,
        "errors": errors,
        "design_rank": rank,
        "design_condition": condition,
        "jacobian": _jacobian(
            problem.basis, gate, beta, problem.left, problem.right, problem.reference_start
        ),
    }


def _record_candidate(result: _SearchResult, candidate: dict, report: dict) -> None:
    _record_feasible(report, candidate["gate"], candidate["beta"])
    result.design_failed |= (
        candidate["design_rank"] != 2 or candidate["design_condition"] > MAX_DESIGN_CONDITION
    )
    result.jacobian_failed |= not candidate["jacobian"]["passed"]
    if _landmarks_equivalent(report):
        result.multipliers.append(complex(*candidate["beta"]))
    else:
        result.multipliers.clear()
    if result.best is None or candidate["errors"]["global"] < result.best["errors"]["global"]:
        result.best = candidate


def _multiplier_diameter(multipliers: list[complex]) -> float:
    # Four integer landmarks with diameter <= 1 permit at most 2**4 models.
    # Thus the exact all-pairs multiplier check needs no unbounded storage.
    maximum = 0.0
    for index, value in enumerate(multipliers):
        for other in multipliers[index + 1 :]:
            scale = max(abs(value), abs(other))
            distance = abs(value / scale - other / scale) if scale else 0.0
            maximum = max(maximum, distance)
    return maximum


def _paired_search(problem: _Problem, tables: tuple[_Table, _Table], report: dict) -> dict | None:
    onset, release = tables
    result = _SearchResult(multipliers=[])
    for left_index, (anchor, width) in enumerate(onset.landmarks):
        for start in range(0, len(release.landmarks), SEARCH_BATCH):
            stop = min(start + SEARCH_BATCH, len(release.landmarks))
            grams = onset.grams[left_index] + problem.middle[0] + release.grams[start:stop]
            targets = onset.targets[left_index] + problem.middle[1] + release.targets[start:stop]
            report["paired_evaluations"] += stop - start
            for offset in _provisional(grams, targets, problem.middle[2], len(problem.source)):
                end, release_width = release.landmarks[start + offset]
                gate = FiniteSourceGate(int(anchor), int(width), int(end), int(release_width))
                candidate = _pair_candidate(problem, gate, report)
                if candidate is not None:
                    _record_candidate(result, candidate, report)
    report["search_complete"] = True
    report["all_feasible_designs_passed_condition"] = not result.design_failed
    report["all_feasible_jacobians_passed"] = not result.jacobian_failed
    if result.best is not None and _landmarks_equivalent(report):
        report["maximum_normalized_complex_multiplier_distance"] = _multiplier_diameter(
            result.multipliers
        )
    return result.best


def _geometry_refusal(frames: int, reference_start: int, first: int, last: int) -> str | None:
    checks = (
        (
            reference_start < 0 or reference_start + WINDOW_FRAMES > frames,
            "plateau_reference_outside_source",
        ),
        (
            first - EDGE_MARGIN < 0 or last + EDGE_MARGIN >= frames,
            "truncated_or_censored_edge_neighborhood",
        ),
        (first + EDGE_MARGIN >= last - EDGE_MARGIN, "overlapping_edge_neighborhoods"),
    )
    return next((reason for failed, reason in checks if failed), None)


def _prepare_problem(
    source: Real, basis: _Basis, reference_start: int, report: dict
) -> _Problem | None:
    extent = _source_extent(source)
    if extent is None:
        report["refusal_reason"] = "silent_source"
        return None
    peak, first, last = extent
    report["source_peak_normalization"] = peak
    report["first_active_frame"], report["last_active_frame"] = first, last
    report["refusal_reason"] = _geometry_refusal(len(source), reference_start, first, last)
    if report["refusal_reason"]:
        return None
    left, right = _edge(source, basis, peak, first), _edge(source, basis, peak, last)
    report["edge_observation_ranges_inclusive"] = {
        "onset": [left.start, left.stop - 1],
        "release": [right.start, right.stop - 1],
    }
    report["reference_fit"] = _reference(source, basis, peak, reference_start)
    if not report["reference_fit"]["passed"]:
        report["refusal_reason"] = "declared_plateau_constant_iq_fit_failed"
        return None
    middle = _middle_statistics(source, basis, peak, left, right)
    return _Problem(source, basis, peak, left, right, reference_start, middle)


def _run_problem(problem: _Problem, report: dict) -> dict | None:
    onset = _edge_table(problem.left, problem.reference_start, report, release=False)
    if onset is None:
        report.update(refusal_reason="onset_edge_survivor_cap", incomplete_search=True)
        return None
    release = _edge_table(problem.right, problem.reference_start, report, release=True)
    if release is None:
        report.update(refusal_reason="release_edge_survivor_cap", incomplete_search=True)
        return None
    pairs = len(onset.landmarks) * len(release.landmarks)
    report["requested_paired_evaluations"] = pairs
    if not pairs:
        report.update(refusal_reason="no_feasible_edge_candidates", search_complete=True)
        return None
    if pairs > MAX_PAIRED_EVALUATIONS:
        report.update(refusal_reason="paired_evaluation_cap", incomplete_search=True)
        return None
    best = _paired_search(problem, (onset, release), report)
    if best is None:
        report["refusal_reason"] = "no_feasible_complete_source_model"
    return best


def _admission_refusal(report: dict, multiplier: complex) -> str | None:
    checks = (
        (report["boundary_unresolved"], "feasible_set_touches_search_boundary"),
        (not _landmarks_equivalent(report), "ambiguous_source_grid_landmarks"),
        (
            (report["maximum_normalized_complex_multiplier_distance"] or 0.0)
            > MAX_COEFFICIENT_DISTANCE,
            "ambiguous_complex_multiplier",
        ),
        (not report["all_feasible_designs_passed_condition"], "unresolved_global_iq_design"),
        (not report["all_feasible_jacobians_passed"], "unresolved_six_parameter_jacobian"),
        (not np.isfinite(multiplier), "nonfinite_original_scale_multiplier"),
    )
    return next((reason for failed, reason in checks if failed), None)


def fit_finite_am_gate(
    source: Real,
    frequency_hz: float,
    stereo_coefficients: NDArray[np.complex128],
    modulator: SourceModulator,
    reference_start: int,
) -> FiniteGateFit:
    """Fit a complete finite gate conditional on one fixed certified source family.

    Args:
        source: Original float64 stereo PCM, never an output or cropped residual.
        frequency_hz: Source-certified carrier frequency with global phase origin zero.
        stereo_coefficients: Source-certified complex stereo direction of unit norm.
        modulator: Immutable, strictly positive source-certified AM law.
        reference_start: Start of the declared clean 8192-frame plateau restriction.

    Returns:
        A source-only complete fit or a refusal carrying no component. Invalid
        input types/values raise ValueError or TypeError.
        Finite unsupported, censored, ambiguous and incomplete searches refuse.
    """
    basis = _Basis(float(frequency_hz), stereo_coefficients, modulator)
    _validate(source, basis, reference_start)
    report = _report(len(source))
    report["model"] = {
        "frequency_hz": basis.frequency_hz,
        "stereo_coefficients": [[float(c.real), float(c.imag)] for c in stereo_coefficients],
        "modulator_frequency_hz": modulator.frequency_hz,
        "modulator_ratio": [modulator.ratio.real, modulator.ratio.imag],
    }
    report["reference_start"] = int(reference_start)
    if report["memory_estimate_bytes"] >= MAX_MEMORY_BYTES:
        return _refused(report, "conservative_memory_limit")
    try:
        problem = _prepare_problem(source, basis, reference_start, report)
        best = _run_problem(problem, report) if problem is not None else None
    except np.linalg.LinAlgError:
        return _refused(report, "numerical_svd_failure", incomplete=True)
    if best is None:
        return _refused(report, report["refusal_reason"], incomplete=report["incomplete_search"])
    report["boundary_unresolved"] = _boundary_unresolved(
        report, report["first_active_frame"], report["last_active_frame"]
    )
    report["minimum_direct_source_fit"] = {
        "gate": _gate_record(best["gate"]),
        "relative_source_rms": best["errors"],
        "global_iq_rank": best["design_rank"],
        "global_iq_condition": best["design_condition"],
        "six_parameter_jacobian": best["jacobian"],
    }
    multiplier = complex(*best["beta"]) * problem.peak
    refusal = _admission_refusal(report, multiplier)
    if refusal:
        return _refused(report, refusal)
    report.update({
        "accepted": True,
        "status": "accepted",
        "selected_gate": _gate_record(best["gate"]),
        "multiplier": [multiplier.real, multiplier.imag],
        "direct_relative_source_rms": best["errors"],
        "global_iq_rank": best["design_rank"],
        "global_iq_condition": best["design_condition"],
        "six_parameter_jacobian": best["jacobian"],
    })
    return FiniteGateFit(best["gate"], multiplier, report)
