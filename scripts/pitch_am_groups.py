"""Source-only positive-AM phase families for the offline mode experiment.

A certified triplet is a stationary-exponential representation of one positive
real modulation of a central carrier. This is an algebraic source interpretation,
not a claim that the physical interpretation of arbitrary PCM is identifiable.
Uncertified and ambiguous carriers remain exactly as originally estimated.
"""

from dataclasses import dataclass, field
from itertools import combinations
from typing import TYPE_CHECKING

import numpy as np
from pitch_mode_carriers import (
    FS,
    HOP_FRAMES,
    MAX_CARRIERS,
    MAX_COEFFICIENT_DISTANCE,
    MAX_DESIGN_CONDITION,
    MAX_RELATIVE_FIT_RMS,
    MIN_SUPPORT_WINDOWS,
    WINDOW_FRAMES,
    CarrierEstimate,
)

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]

_LOCAL_TIME = (np.arange(WINDOW_FRAMES) - (WINDOW_FRAMES - 1) / 2) / FS


@dataclass(slots=True)
class _Fit:
    design: Real
    block: Real
    coefficients: Complex
    source_norm: float
    source_peak: float
    center_seconds: float
    condition: float
    relative_rms: float


@dataclass(slots=True)
class _Candidate:
    indices: tuple[int, int, int]
    frequencies: Real
    support: int = 0
    refusals: dict[str, int] = field(default_factory=dict)
    first_ratio: complex | None = None
    first_direction: Complex | None = None
    reference_direction: Complex | None = None
    reference_amplitude: float = 0.0
    reference_start: int | None = None
    maximum_vector_defect: float = 0.0
    maximum_conjugacy_defect: float = 0.0
    maximum_ratio_distance: float = 0.0
    maximum_direction_distance: float = 0.0
    maximum_source_rms: float = 0.0
    maximum_condition: float = 0.0
    minimum_positivity_margin: float | None = None


@dataclass(slots=True)
class _Factorization:
    ratio: complex
    direction: Complex
    vector_defect: float
    conjugacy_defect: float
    positivity_margin: float
    source_rms: float
    center_amplitude: float


def _validate(source: Real, estimate: CarrierEstimate) -> Real:
    if (
        not isinstance(source, np.ndarray)
        or source.ndim != 2
        or source.shape[1] != 2
        or source.dtype != np.float64
        or np.iscomplexobj(source)
    ):
        message = "AM certificates require original real float64 stereo PCM"
        raise ValueError(message)
    for start in range(0, len(source), WINDOW_FRAMES):
        if not np.isfinite(source[start : start + WINDOW_FRAMES]).all():
            message = "AM certificate source must be finite"
            raise ValueError(message)
    frequencies = np.asarray(estimate.frequencies_hz)
    if frequencies.ndim != 1 or np.iscomplexobj(frequencies):
        message = "AM carrier frequencies must be a real one-dimensional sequence"
        raise ValueError(message)
    frequencies = frequencies.astype(np.float64)
    count = len(frequencies)
    if (
        count > MAX_CARRIERS
        or not np.isfinite(frequencies).all()
        or np.any(frequencies <= 0.0)
        or np.any(frequencies >= FS / 2)
        or np.any(np.diff(frequencies) <= 0.0)
    ):
        message = "AM certificates require bounded sorted finite in-band frequencies"
        raise ValueError(message)
    if (
        not isinstance(estimate.stereo_coefficients, np.ndarray)
        or estimate.stereo_coefficients.shape != (count, 2)
        or not np.iscomplexobj(estimate.stereo_coefficients)
        or not np.isfinite(estimate.stereo_coefficients).all()
    ):
        message = "AM certificates require finite complex stereo carrier vectors"
        raise ValueError(message)
    norms = np.linalg.norm(estimate.stereo_coefficients, axis=1)
    if np.max(np.abs(norms - 1.0), initial=0.0) > 64.0 * np.finfo(np.float64).eps:
        message = "AM carrier stereo vectors must have unit pooled complex norm"
        raise ValueError(message)
    return frequencies


def _design(frequencies: Real) -> Real:
    phase = 2.0 * np.pi * _LOCAL_TIME[:, None] * frequencies[None, :]
    return np.concatenate((np.cos(phase), np.sin(phase)), axis=1)


def _solve(design: Real, normalized: Real, start: int, frequencies: Real, peak: float) -> _Fit:
    coefficients, _, rank, singular = np.linalg.lstsq(design, normalized, rcond=None)
    condition = float(singular[0] / singular[-1]) if singular[-1] else float("inf")
    if rank != design.shape[1]:
        condition = float("inf")
    source_norm = float(np.linalg.norm(normalized))
    relative = float(np.linalg.norm(normalized - design @ coefficients)) / source_norm
    count = len(frequencies)
    center = (start + (WINDOW_FRAMES - 1) / 2) / FS
    complex_coefficients = (coefficients[:count] - 1j * coefficients[count:]) * np.exp(
        -2j * np.pi * frequencies * center
    )[:, None]
    return _Fit(
        design, normalized, complex_coefficients, source_norm, peak, center, condition, relative
    )


def _fit(source: Real, start: int, frequencies: Real) -> _Fit | None:
    block = source[start : start + WINDOW_FRAMES]
    peak = float(np.max(np.abs(block)))
    if peak == 0.0:
        return None
    return _solve(_design(frequencies), block / peak, start, frequencies, peak)


def _fit_supported(fit: _Fit | None) -> bool:
    if fit is None:
        return False
    return bool(
        np.isfinite(fit.condition)
        and np.isfinite(fit.relative_rms)
        and fit.condition <= MAX_DESIGN_CONDITION
        and fit.relative_rms <= MAX_RELATIVE_FIT_RMS
        and np.isfinite(fit.coefficients).all()
    )


def _refuse(candidate: _Candidate, reason: str) -> None:
    candidate.refusals[reason] = candidate.refusals.get(reason, 0) + 1


def _constrained_rms(fit: _Fit, frequencies: Real, candidate: _Candidate, ratio: complex) -> float:
    coefficients = fit.coefficients.copy()
    low, center, high = candidate.indices
    coefficients[low] = np.conj(ratio) * coefficients[center]
    coefficients[high] = ratio * coefficients[center]
    local = coefficients * np.exp(2j * np.pi * frequencies * fit.center_seconds)[:, None]
    real_coefficients = np.concatenate((local.real, -local.imag), axis=0)
    reconstruction = fit.design @ real_coefficients
    return float(np.linalg.norm(fit.block - reconstruction)) / fit.source_norm


def _measure(candidate: _Candidate, fit: _Fit) -> _Factorization | None:
    coefficients = fit.coefficients[list(candidate.indices)]
    norms = np.linalg.norm(coefficients, axis=1)
    if np.min(norms) == 0.0:
        return None
    ratios = np.array([
        np.vdot(coefficients[1], coefficients[index]) / (norms[1] * norms[1]) for index in (0, 2)
    ])
    scale = float(np.max(np.abs(ratios)))
    if scale == 0.0:
        return None
    ratio = complex(0.5 * (ratios[1] + np.conj(ratios[0])))
    vector_defect = float(
        np.max(
            np.linalg.norm(coefficients[[0, 2]] - ratios[:, None] * coefficients[1], axis=1)
            / norms[[0, 2]]
        )
    )
    return _Factorization(
        ratio=ratio,
        direction=coefficients[1] / norms[1],
        vector_defect=vector_defect,
        conjugacy_defect=float(abs(ratios[1] - np.conj(ratios[0])) / scale),
        positivity_margin=1.0 - 2.0 * abs(ratio),
        source_rms=_constrained_rms(fit, candidate.frequencies, candidate, ratio),
        center_amplitude=float(norms[1]) * fit.source_peak,
    )


def _inspect_coefficients(candidate: _Candidate, fit: _Fit, start: int) -> None:
    measured = _measure(candidate, fit)
    if measured is None:
        _refuse(candidate, "zero_or_orthogonal_source_coefficients")
        return
    if not all(
        np.isfinite(value)
        for value in (
            measured.ratio.real,
            measured.ratio.imag,
            measured.vector_defect,
            measured.conjugacy_defect,
            measured.positivity_margin,
            measured.source_rms,
            measured.center_amplitude,
            *measured.direction.real,
            *measured.direction.imag,
        )
    ):
        _refuse(candidate, "nonfinite_factorization")
        return
    if candidate.first_ratio is None:
        candidate.first_ratio = measured.ratio
        candidate.first_direction = measured.direction.copy()
    ratio_scale = max(abs(candidate.first_ratio), abs(measured.ratio))
    ratio_distance = (
        abs(measured.ratio / ratio_scale - candidate.first_ratio / ratio_scale)
        if ratio_scale
        else 0.0
    )
    direction_distance = float(np.linalg.norm(measured.direction - candidate.first_direction))
    candidate.maximum_vector_defect = max(candidate.maximum_vector_defect, measured.vector_defect)
    candidate.maximum_conjugacy_defect = max(
        candidate.maximum_conjugacy_defect, measured.conjugacy_defect
    )
    candidate.maximum_ratio_distance = max(candidate.maximum_ratio_distance, ratio_distance)
    candidate.maximum_direction_distance = max(
        candidate.maximum_direction_distance, direction_distance
    )
    candidate.maximum_source_rms = max(candidate.maximum_source_rms, measured.source_rms)
    candidate.minimum_positivity_margin = (
        measured.positivity_margin
        if candidate.minimum_positivity_margin is None
        else min(candidate.minimum_positivity_margin, measured.positivity_margin)
    )
    defects = (
        measured.vector_defect,
        measured.conjugacy_defect,
        ratio_distance,
        direction_distance,
    )
    if not all(np.isfinite(value) for value in defects):
        _refuse(candidate, "nonfinite_factorization")
    elif max(defects) > MAX_COEFFICIENT_DISTANCE:
        _refuse(candidate, "incoherent_positive_AM_factorization")
    elif measured.positivity_margin <= 0.0:
        _refuse(candidate, "nonpositive_AM_envelope")
    elif measured.source_rms > MAX_RELATIVE_FIT_RMS:
        _refuse(candidate, "poor_coefficient_constrained_source_fit")
    else:
        candidate.support += 1
        if (
            candidate.reference_direction is None
            or measured.center_amplitude > candidate.reference_amplitude
        ):
            candidate.reference_direction = measured.direction.copy()
            candidate.reference_amplitude = measured.center_amplitude
            candidate.reference_start = start


def _scan(source: Real, frequencies: Real, candidates: list[_Candidate]) -> dict:
    summary = {"raster_windows": 0, "complete_source_windows": 0, "source_fit_failures": 0}
    for start in range(0, len(source) - WINDOW_FRAMES + 1, HOP_FRAMES):
        summary["raster_windows"] += 1
        try:
            original = _fit(source, start, frequencies)
        except np.linalg.LinAlgError:
            summary["source_fit_failures"] += 1
            continue
        if not _fit_supported(original):
            continue
        summary["complete_source_windows"] += 1
        for candidate in candidates:
            try:
                fitted = _fit(source, start, candidate.frequencies)
            except np.linalg.LinAlgError:
                _refuse(candidate, "symmetric_source_factorization_failed")
                continue
            if not _fit_supported(fitted):
                _refuse(candidate, "poor_symmetric_frequency_source_fit")
                continue
            candidate.maximum_condition = max(candidate.maximum_condition, fitted.condition)
            _inspect_coefficients(candidate, fitted, start)
    return summary


def _candidate_report(candidate: _Candidate, *, ambiguous: bool) -> dict:
    return {
        "original_indices": list(candidate.indices),
        "symmetric_frequencies_hz": candidate.frequencies[list(candidate.indices)].tolist(),
        "support_windows": candidate.support,
        "refusal_counts": candidate.refusals,
        "maximum_stereo_vector_relative_defect": candidate.maximum_vector_defect,
        "maximum_conjugacy_relative_defect": candidate.maximum_conjugacy_defect,
        "maximum_global_modulator_relative_distance": candidate.maximum_ratio_distance,
        "maximum_center_direction_distance": candidate.maximum_direction_distance,
        "maximum_coefficient_constrained_source_rms": candidate.maximum_source_rms,
        "maximum_symmetric_source_design_condition": candidate.maximum_condition,
        "minimum_positive_envelope_margin": candidate.minimum_positivity_margin,
        "reference_start": candidate.reference_start,
        "global_positive_sideband_ratio": (
            [candidate.first_ratio.real, candidate.first_ratio.imag]
            if candidate.first_ratio is not None
            else None
        ),
        "algebraically_certified": not candidate.refusals
        and candidate.support >= MIN_SUPPORT_WINDOWS,
        "overlap_ambiguous": ambiguous,
        "grouped": not candidate.refusals
        and candidate.support >= MIN_SUPPORT_WINDOWS
        and not ambiguous,
    }


def _make_candidates(frequencies: Real) -> list[_Candidate]:
    candidates = []
    for indices in combinations(range(len(frequencies)), 3):
        low, center, high = indices
        constrained = frequencies.copy()
        spacing = (frequencies[high] - frequencies[low]) / 2.0
        constrained[low], constrained[high] = (
            frequencies[center] - spacing,
            frequencies[center] + spacing,
        )
        if constrained[low] > 0.0:
            candidates.append(_Candidate(indices, constrained))
    return candidates


def _select_groups(candidates: list[_Candidate]) -> tuple[list[_Candidate], set[int], int]:
    certified = [
        candidate
        for candidate in candidates
        if not candidate.refusals and candidate.support >= MIN_SUPPORT_WINDOWS
    ]
    ambiguous_indices = {
        index
        for left, right in combinations(range(len(certified)), 2)
        if set(certified[left].indices) & set(certified[right].indices)
        for index in (left, right)
    }
    selected = [
        candidate for index, candidate in enumerate(certified) if index not in ambiguous_indices
    ]
    ambiguous_ids = {id(certified[index]) for index in ambiguous_indices}
    return selected, ambiguous_ids, len(certified)


def _retained_carriers(
    frequencies: Real, estimate: CarrierEstimate, selected: list[_Candidate]
) -> tuple[list[int], Complex]:
    sidebands = {index for candidate in selected for index in candidate.indices[::2]}
    centers = {candidate.indices[1]: candidate for candidate in selected}
    retained = [index for index in range(len(frequencies)) if index not in sidebands]
    coefficients = estimate.stereo_coefficients[retained].copy()
    for row, original_index in enumerate(retained):
        if original_index in centers:
            coefficients[row] = centers[original_index].reference_direction
    return retained, coefficients


def group_am_carriers(source: Real, estimate: CarrierEstimate) -> CarrierEstimate:
    """Replace only unique source-certified positive-AM triplets by their center.

    Every original complete source-fit window must support the constrained
    factorization, and at least the existing minimum support count is required.
    Frequencies are not tested with a new hertz tolerance: all carriers are refit
    jointly with the triplet constrained to fc-nu,fc,fc+nu, holding fc unchanged.
    Existing source RMS/condition and coefficient-coherence limits are reused.

    The output center's fixed stereo direction comes from the largest actual
    source center coefficient, with the earliest-window tie. Its real envelope
    must still be estimated from original PCM by the caller. This function never
    renders, reads output intent, changes a sample, or drops the exact remainder.
    """
    frequencies = _validate(source, estimate)
    candidates = _make_candidates(frequencies)
    summary = (
        _scan(source, frequencies, candidates)
        if candidates
        else {"raster_windows": 0, "complete_source_windows": 0, "source_fit_failures": 0}
    )
    selected, ambiguous_ids, certified_count = _select_groups(candidates)
    retained, coefficients = _retained_carriers(frequencies, estimate, selected)
    report = dict(estimate.report)
    report["admitted_carrier_count_before_AM_grouping"] = len(frequencies)
    report["admitted_carrier_count"] = len(retained)
    report["am_grouping"] = {
        "method": "source_only_unique_positive_AM_triplet_factorization_v1",
        "original_frequencies_hz": frequencies.tolist(),
        "result_frequencies_hz": frequencies[retained].tolist(),
        "candidate_count": len(candidates),
        "certified_candidate_count": certified_count,
        "ambiguous_candidate_count": len(ambiguous_ids),
        "group_count": len(selected),
        "accepted_groups": [
            {
                "original_indices": list(value.indices),
                "center_frequency_hz": float(frequencies[value.indices[1]]),
                "modulation_frequency_hz": float(
                    (frequencies[value.indices[2]] - frequencies[value.indices[0]]) / 2.0
                ),
                "positive_modulator_margin": value.minimum_positivity_margin,
            }
            for value in selected
        ],
        **summary,
        "window_frames": WINDOW_FRAMES,
        "hop_frames": HOP_FRAMES,
        "minimum_support_windows": MIN_SUPPORT_WINDOWS,
        "maximum_source_relative_rms": MAX_RELATIVE_FIT_RMS,
        "maximum_source_design_condition": MAX_DESIGN_CONDITION,
        "maximum_relative_coefficient_defect": MAX_COEFFICIENT_DISTANCE,
        "strict_positive_modulator_margin_required": True,
        "all_original_clean_complete_windows_checked": True,
        "overlapping_certificates_all_preserved_as_independent_carriers": True,
        "source_phase_coordinate": "global source sample zero",
        "unique_physical_AM_interpretation_claimed": False,
        "fixture_metadata_or_render_intent_used": False,
        "candidates": [
            _candidate_report(value, ambiguous=id(value) in ambiguous_ids) for value in candidates
        ],
        "working_memory": "fixed8192x12 source fits; <=20 triplet summaries; no full PCM copy",
    }
    return CarrierEstimate(
        tuple(float(frequencies[index]) for index in retained), coefficients, report
    )
