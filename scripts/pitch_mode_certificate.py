"""Bounded source-window shadow certificates; no carrier admission or rendering.

Both geometries use the same one-sample pencil shift and therefore the original
sample-rate eigenphase. The original energy guard is reported without preventing
the independent stationary-exponential certificate. Every refusal retains PCM.
"""

from typing import TYPE_CHECKING

import numpy as np
from pitch_mode_carriers import (
    CLUSTER_WIDTH_HZ,
    CONJUGATE_TOLERANCE,
    FS,
    MAX_CREST_FACTOR,
    MAX_DESIGN_CONDITION,
    MAX_FREQUENCY_HZ,
    MAX_POLE_CONDITION,
    MAX_RANK,
    MAX_RELATIVE_FIT_RMS,
    MIN_BLOCK_ENERGY_RATIO,
    MIN_FREQUENCY_HZ,
    PENCIL_LAGS,
    POLE_RADIUS_TOLERANCE,
    RANK_RATIO,
    WINDOW_FRAMES,
)

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Pcm = NDArray[np.float64]
type ComplexPcm = NDArray[np.complex128]
type Indices = NDArray[np.int64]

ROWS_PER_CHANNEL = WINDOW_FRAMES // 2
_LOCAL_TIME = (np.arange(WINDOW_FRAMES) - (WINDOW_FRAMES - 1) / 2) / FS


def _offsets(geometry: str) -> Indices:
    columns = np.arange(PENCIL_LAGS, dtype=np.int64)
    if geometry == "compact":
        return columns
    if geometry == "spread":
        return columns * (ROWS_PER_CHANNEL - 1) // (PENCIL_LAGS - 1)
    message = "Geometry must be 'compact' or 'spread'."
    raise ValueError(message)


def _validate(source: Pcm, start: int) -> None:
    if not isinstance(source, np.ndarray) or source.ndim != 2 or source.shape[1] != 2:
        message = "Source must have shape (frames, 2)."
        raise ValueError(message)
    if np.iscomplexobj(source) or source.dtype != np.float64:
        message = "Source must be real float64 PCM."
        raise ValueError(message)
    if isinstance(start, (bool, np.bool_)) or not isinstance(start, (int, np.integer)):
        message = "Window start must be an integer source frame."
        raise TypeError(message)
    if start < 0 or start + WINDOW_FRAMES > len(source):
        message = "A complete 8192-frame source window is required."
        raise ValueError(message)
    # Validate the entire input without allocating an input-length boolean array.
    for begin in range(0, len(source), WINDOW_FRAMES):
        if not np.isfinite(source[begin : begin + WINDOW_FRAMES]).all():
            message = "Source must contain only finite samples."
            raise ValueError(message)


def _finite(value: float) -> float | None:
    return float(value) if np.isfinite(value) else None


def _energy_guard(block: Pcm, report: dict) -> None:
    energies = np.mean(block.reshape(8, WINDOW_FRAMES // 8, 2) ** 2, axis=(1, 2))
    ratio = float(np.min(energies) / np.max(energies))
    crest = float(np.max(np.linalg.norm(block, axis=1))) / float(
        np.sqrt(np.mean(np.sum(block * block, axis=1)))
    )
    passed = ratio >= MIN_BLOCK_ENERGY_RATIO and crest <= MAX_CREST_FACTOR
    report.update(
        block_energy_ratio=ratio,
        crest_factor=crest,
        energy_guard_passed=passed,
        original_energy_guard_pass=passed,
        energy_guard_rejection=None if passed else "nonstationary_energy_or_crest",
    )


def _pencil(block: Pcm, offsets: Indices, report: dict) -> Pcm | None:
    positions = np.arange(ROWS_PER_CHANNEL)[:, None] + offsets[None, :]
    hankel = np.concatenate([block[positions, channel] for channel in range(2)], axis=0)
    shifted = np.concatenate([block[positions + 1, channel] for channel in range(2)], axis=0)
    try:
        left, singular, right = np.linalg.svd(hankel, full_matrices=False)
        if singular[0] == 0.0:
            report.update(rank=0, rejection="zero_pencil_rows")
            return None
        rank = int(np.count_nonzero(singular > singular[0] * RANK_RATIO))
        report.update(rank=rank, relative_singular_values=(singular / singular[0]).tolist())
        if rank < 2 or rank > MAX_RANK or rank % 2:
            report["rejection"] = "not_low_even_rank"
            return None
        report["retained_singular_condition"] = float(singular[0] / singular[rank - 1])
        pencil = (left[:, :rank].T @ shifted @ right[:rank].T) / singular[:rank, None]
        poles, eigenvectors = np.linalg.eig(pencil)
        condition = float(np.linalg.cond(eigenvectors))
    except np.linalg.LinAlgError:
        report["rejection"] = "pencil_factorization_failed"
        return None
    return _frequencies(poles, condition, rank, report)


def _frequencies(poles: ComplexPcm, condition: float, rank: int, report: dict) -> Pcm | None:
    positive, negative = poles[np.angle(poles) > 0], poles[np.angle(poles) < 0]
    frequencies = np.sort(np.angle(positive) * FS / (2 * np.pi))
    equal_counts = len(positive) == len(negative) == rank // 2
    positive_paired = equal_counts and all(
        np.min(np.abs(negative - np.conj(pole))) <= CONJUGATE_TOLERANCE for pole in positive
    )
    negative_paired = equal_counts and all(
        np.min(np.abs(positive - np.conj(pole))) <= CONJUGATE_TOLERANCE for pole in negative
    )
    paired = positive_paired and negative_paired
    stable = bool(
        np.isfinite(poles).all() and np.all(np.abs(np.abs(poles) - 1) <= POLE_RADIUS_TOLERANCE)
    )
    in_band = bool(
        len(frequencies) == rank // 2
        and np.isfinite(frequencies).all()
        and np.all((frequencies >= MIN_FREQUENCY_HZ) & (frequencies <= MAX_FREQUENCY_HZ))
    )
    conditioned = bool(np.isfinite(condition) and condition <= MAX_POLE_CONDITION)
    report.update(
        pole_eigenvector_condition=_finite(condition),
        pole_eigenvector_condition_within_limit=conditioned,
        conjugate_poles_paired=bool(paired),
        conjugate_positive_to_negative_paired=bool(positive_paired),
        conjugate_negative_to_positive_paired=bool(negative_paired),
        pole_radii_within_tolerance=stable,
        pole_frequencies_in_band=in_band,
        poles=[
            {
                "real": _finite(pole.real),
                "imaginary": _finite(pole.imag),
                "radius": _finite(abs(pole)),
            }
            for pole in poles
        ],
    )
    if not (conditioned and paired and stable and in_band):
        report["rejection"] = "unstable_out_of_band_or_unpaired_poles"
        return None
    report["frequencies_hz"] = frequencies.tolist()
    return frequencies


def _source_fit(block: Pcm, frequencies: Pcm, report: dict) -> None:
    phase = 2 * np.pi * _LOCAL_TIME[:, None] * frequencies[None, :]
    design = np.concatenate((np.cos(phase), np.sin(phase)), axis=1)
    try:
        coefficients, _, rank, singular = np.linalg.lstsq(design, block, rcond=None)
    except np.linalg.LinAlgError:
        report["rejection"] = "source_fit_factorization_failed"
        return
    condition = float(singular[0] / singular[-1]) if singular[-1] else float("inf")
    if rank != design.shape[1]:
        condition = float("inf")
    relative = float(np.linalg.norm(block - design @ coefficients) / np.linalg.norm(block))
    report.update(
        source_fit_condition=_finite(condition), source_fit_relative_rms=_finite(relative)
    )
    if (
        not np.isfinite(relative)
        or condition > MAX_DESIGN_CONDITION
        or relative > MAX_RELATIVE_FIT_RMS
    ):
        report["rejection"] = "poor_stationary_source_fit"
        return
    report["accepted_certificate"] = True


def inspect_window(source: Pcm, start: int, geometry: str) -> dict:
    """Inspect one complete source window without admitting modes or altering PCM.

    All work arrays have fixed window dimensions: each pencil is 8192 by 32,
    and the source refit is at most 8192 by 12. Input validation is chunked; the
    caller-owned source is neither copied in full nor retained by this report.
    """
    offsets = _offsets(geometry)
    _validate(source, start)
    block = source[start : start + WINDOW_FRAMES]
    peak = float(np.max(np.abs(block)))
    report = {
        "start": int(start),
        "geometry": geometry,
        "accepted_certificate": False,
        "window_frames": WINDOW_FRAMES,
        "rows_per_channel": ROWS_PER_CHANNEL,
        "column_offsets_frames": offsets.tolist(),
        "pencil_shift_frames": 1,
        "eigenphase_sample_rate_hz": FS,
        "source_peak": peak,
        "carrier_admission_or_renderer_changed": False,
    }
    if not peak:
        report.update(
            energy_guard_passed=False,
            original_energy_guard_pass=False,
            energy_guard_rejection="zero_source",
            rejection="zero_source",
        )
        return report
    normalized = block / peak
    _energy_guard(normalized, report)
    frequencies = _pencil(normalized, offsets, report)
    if frequencies is not None:
        _source_fit(normalized, frequencies, report)
    return report


def integrity_controls() -> dict:
    """Check unseen stationary sources; expected frequencies exist only here."""
    results, maximum_error = [], 0.0
    clock = np.arange(WINDOW_FRAMES) / FS
    for label, expected in (("separated", (487.31, 1291.47)), ("close", (639.47, 652.13))):
        mono = 0.19 * np.cos(2 * np.pi * expected[0] * clock + 0.31)
        mono += 0.13 * np.sin(2 * np.pi * expected[1] * clock - 0.27)
        source = np.column_stack((mono, 0.5 * mono))
        for geometry in ("compact", "spread"):
            report = inspect_window(source, 0, geometry)
            error = None
            if report["accepted_certificate"] and len(report["frequencies_hz"]) == len(expected):
                error = float(np.max(np.abs(np.asarray(report["frequencies_hz"]) - expected)))
                maximum_error = max(maximum_error, error)
            if label == "separated" or geometry == "spread":
                assert report["accepted_certificate"]
                assert error is not None
                assert error < CLUSTER_WIDTH_HZ * 1e-3
            results.append({
                "source_kind": label,
                "geometry": geometry,
                "accepted_certificate": report["accepted_certificate"],
                "energy_guard_passed": report["energy_guard_passed"],
                "maximum_frequency_error_hz": error,
            })
    refusal_controls = _refusal_controls()
    return {
        "stage": "mode_certificate_integrity_controls",
        "passed": True,
        "same_one_sample_shift_at_original_sample_rate": True,
        "expected_frequencies_only_in_controls": True,
        "maximum_accepted_frequency_error_hz": maximum_error,
        "cases": results,
        **refusal_controls,
    }


def _refusal_controls() -> dict:
    source = np.zeros((WINDOW_FRAMES, 2))
    zero = inspect_window(source, 0, "compact")
    source[-1] = (0.7, 0.35)
    late = inspect_window(source, 0, "compact")
    assert zero["rejection"] == "zero_source"
    assert late["rejection"] == "zero_pencil_rows"
    return {"exact_zero_refused": True, "unobserved_late_impulse_refused_without_nan": True}
