"""Conservative, source-only stationary carriers for an offline falsification model.

The low-rank pencil follows Hua and Sarkar, IEEE TASSP 38(5), 1990,
https://www.math.ucdavis.edu/~saito/data/sonar/HuaSarkarPencilMethod.pdf.
The fixed-window stationarity, recurrence, refinement and admission rules are a
custom conservative policy, not a claim about arbitrary musical mixtures. No
target frequency, event position, render profile or fixture metadata is read.
Unadmitted PCM must remain in the caller's exact remainder.

All matrices are window sized. This module does no file/device/native work and
does not use an FFT peak, a manually seeded frequency, SciPy or a fixed model K.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Pcm = NDArray[np.float64]
type ComplexPcm = NDArray[np.complex128]

FS = 48_000
WINDOW_FRAMES = 8192
HOP_FRAMES = 4096
PENCIL_LAGS = 32
RANK_RATIO = 1.0e-7
MAX_RANK = 12
MAX_CARRIERS = MAX_RANK // 2
MIN_FREQUENCY_HZ = 20.0
MAX_FREQUENCY_HZ = 0.49 * FS
POLE_RADIUS_TOLERANCE = 0.005
CONJUGATE_TOLERANCE = 1.0e-6
MAX_POLE_CONDITION = 1.0e7
MAX_DESIGN_CONDITION = 1.0e6
MIN_BLOCK_ENERGY_RATIO = 0.25
MAX_CREST_FACTOR = 8.0
MAX_RELATIVE_FIT_RMS = 1.0e-4
CLUSTER_WIDTH_HZ = 1.0
MIN_SUPPORT_WINDOWS = 3
MAX_COEFFICIENT_DISTANCE = 1.0e-3
REFINEMENT_ITERATIONS = 6
MAX_REFINEMENT_STEP_HZ = 0.5
REFINEMENT_STOP_HZ = 1.0e-10
MAX_REFINEMENT_NORMAL_CONDITION = 1.0e12
_LOCAL_TIME = (np.arange(WINDOW_FRAMES) - (WINDOW_FRAMES - 1) / 2) / FS


@dataclass(slots=True)
class CarrierEstimate:
    """Admitted frequencies and fixed stereo directions in global source phase.

    ``stereo_coefficients[k]`` has pooled complex Euclidean norm one. Its gauge
    means ``real(exp(2j*pi*f[k]*global_sample/FS) * coefficient[k])``; it is not
    reset at an event start. The caller estimates time-varying amplitudes and
    retains the exact subtraction remainder. Empty estimates have shape (0, 2).
    """

    frequencies_hz: tuple[float, ...]
    stereo_coefficients: ComplexPcm
    report: dict


@dataclass(slots=True)
class _Window:
    start: int
    frequencies: tuple[float, ...]


@dataclass(slots=True)
class _Cluster:
    frequency: float
    starts: tuple[int, ...]
    spread: float
    duplicate_window: bool


@dataclass(slots=True)
class _Fit:
    design: Pcm
    coefficients: Pcm
    residual: Pcm
    relative_rms: float
    condition: float
    source_norm: float
    source_peak: float


def _parameters() -> dict:
    return {
        "sample_rate_hz": FS,
        "window_frames": WINDOW_FRAMES,
        "hop_frames": HOP_FRAMES,
        "pencil_lags": PENCIL_LAGS,
        "singular_rank_ratio": RANK_RATIO,
        "accepted_real_rank": [2, MAX_RANK, "even"],
        "pole_radius_tolerance": POLE_RADIUS_TOLERANCE,
        "conjugate_tolerance": CONJUGATE_TOLERANCE,
        "frequency_limits_hz": [MIN_FREQUENCY_HZ, MAX_FREQUENCY_HZ],
        "maximum_pole_eigenvector_condition": MAX_POLE_CONDITION,
        "maximum_design_condition": MAX_DESIGN_CONDITION,
        "minimum_eight_block_energy_ratio": MIN_BLOCK_ENERGY_RATIO,
        "maximum_crest_factor": MAX_CREST_FACTOR,
        "maximum_relative_fit_rms": MAX_RELATIVE_FIT_RMS,
        "cluster_full_width_hz": CLUSTER_WIDTH_HZ,
        "minimum_distinct_support_windows": MIN_SUPPORT_WINDOWS,
        "maximum_carriers": MAX_CARRIERS,
        "maximum_normalized_coefficient_distance": MAX_COEFFICIENT_DISTANCE,
        "refinement_iterations": REFINEMENT_ITERATIONS,
        "maximum_refinement_step_hz": MAX_REFINEMENT_STEP_HZ,
        "refinement_stop_hz": REFINEMENT_STOP_HZ,
        "maximum_refinement_normal_condition": MAX_REFINEMENT_NORMAL_CONDITION,
        "refinement_line_search": [1.0, 0.5, 0.25, 0.125],
        "refinement_windows": "earliest/middle/latest complete support per carrier; <=18",
        "estimated_extra_working_memory_mib": 64,
        "working_memory_budget_mib": 256,
    }


def _normalized_window(source: Pcm, start: int) -> tuple[Pcm, float]:
    block = source[start : start + WINDOW_FRAMES]
    peak = float(np.max(np.abs(block)))
    return (block / peak if peak else np.zeros_like(block)), peak


def _fit(source: Pcm, start: int, frequencies: Pcm) -> _Fit:
    block, peak = _normalized_window(source, start)
    phase = 2 * np.pi * _LOCAL_TIME[:, None] * frequencies[None, :]
    design = np.concatenate((np.cos(phase), np.sin(phase)), axis=1)
    coefficients, _, rank, singular = np.linalg.lstsq(design, block, rcond=None)
    condition = float(singular[0] / singular[-1]) if singular[-1] else float("inf")
    if rank != design.shape[1]:
        condition = float("inf")
    residual = block - design @ coefficients
    source_norm = float(np.linalg.norm(block))
    relative = float(np.linalg.norm(residual)) / source_norm if source_norm else 0.0
    return _Fit(design, coefficients, residual, relative, condition, source_norm, peak)


def _global_coefficients(fit: _Fit, start: int, frequencies: Pcm) -> ComplexPcm:
    count = len(frequencies)
    local = fit.coefficients[:count] - 1j * fit.coefficients[count:]
    center = (start + (WINDOW_FRAMES - 1) / 2) / FS
    return local * np.exp(-2j * np.pi * frequencies * center)[:, None]


def _stationary_window(source: Pcm, start: int, record: dict) -> Pcm | None:
    block, peak = _normalized_window(source, start)
    if not peak:
        record["rejection"] = "zero_source"
        return None
    energies = np.mean(block.reshape(8, WINDOW_FRAMES // 8, 2) ** 2, axis=(1, 2))
    ratio = float(np.min(energies) / np.max(energies))
    crest = float(np.max(np.linalg.norm(block, axis=1))) / float(
        np.sqrt(np.mean(np.sum(block * block, axis=1)))
    )
    record.update(block_energy_ratio=ratio, crest_factor=crest)
    if ratio < MIN_BLOCK_ENERGY_RATIO or crest > MAX_CREST_FACTOR:
        record["rejection"] = "nonstationary_energy_or_crest"
        return None
    return block


def _pencil_poles(block: Pcm, record: dict) -> tuple[ComplexPcm, float] | None:
    views = [
        np.lib.stride_tricks.sliding_window_view(block[:, channel], PENCIL_LAGS + 1)
        for channel in range(2)
    ]
    hankel = np.concatenate([view[:, :-1] for view in views], axis=0)
    shifted = np.concatenate([view[:, 1:] for view in views], axis=0)
    try:
        left, singular, right = np.linalg.svd(hankel, full_matrices=False)
        rank = int(np.count_nonzero(singular > singular[0] * RANK_RATIO))
        record["rank"] = rank
        if rank < 2 or rank > MAX_RANK or rank % 2:
            record["rejection"] = "not_low_even_rank"
            return None
        record["retained_singular_condition"] = float(singular[0] / singular[rank - 1])
        # X0=U S V.T. S^-1 U.T X1 V is similar to the source pole diagonal.
        pencil = (left[:, :rank].T @ shifted @ right[:rank].T) / singular[:rank, None]
        poles, eigenvectors = np.linalg.eig(pencil)
        condition = float(np.linalg.cond(eigenvectors))
    except np.linalg.LinAlgError:
        record["rejection"] = "pencil_factorization_failed"
        return None
    return poles, condition


def _pole_frequencies(poles: ComplexPcm, condition: float, record: dict) -> Pcm | None:
    record["pole_eigenvector_condition"] = condition if np.isfinite(condition) else None
    record["poles"] = [
        {"real": float(pole.real), "imaginary": float(pole.imag), "radius": float(abs(pole))}
        for pole in poles
    ]
    positive = poles[np.angle(poles) > 0]
    negative = poles[np.angle(poles) < 0]
    frequencies = np.sort(np.angle(positive) * FS / (2 * np.pi))
    conjugate = len(positive) == len(negative) == record["rank"] // 2 and all(
        np.min(np.abs(negative - np.conj(pole))) <= CONJUGATE_TOLERANCE for pole in positive
    )
    invalid_condition = not np.isfinite(condition) or condition > MAX_POLE_CONDITION
    invalid_frequency = np.any(frequencies < MIN_FREQUENCY_HZ) or np.any(
        frequencies > MAX_FREQUENCY_HZ
    )
    if (
        invalid_condition
        or np.any(np.abs(np.abs(poles) - 1) > POLE_RADIUS_TOLERANCE)
        or not conjugate
        or invalid_frequency
    ):
        record["rejection"] = "unstable_out_of_band_or_unpaired_poles"
        return None
    return frequencies


def _analyze_window(source: Pcm, start: int) -> tuple[_Window | None, dict]:
    record: dict = {"start": start, "accepted": False}
    block = _stationary_window(source, start, record)
    if block is None:
        return None, record
    pencil = _pencil_poles(block, record)
    if pencil is None:
        return None, record
    frequencies = _pole_frequencies(*pencil, record)
    if frequencies is None:
        return None, record
    try:
        fit = _fit(source, start, frequencies)
    except np.linalg.LinAlgError:
        record["rejection"] = "source_fit_factorization_failed"
        return None, record
    record.update(
        frequencies_hz=frequencies.tolist(),
        source_fit_relative_rms=fit.relative_rms,
        source_fit_condition=fit.condition if np.isfinite(fit.condition) else None,
    )
    if fit.condition > MAX_DESIGN_CONDITION or fit.relative_rms > MAX_RELATIVE_FIT_RMS:
        record["rejection"] = "poor_stationary_source_fit"
        return None, record
    record["accepted"] = True
    return _Window(start, tuple(float(value) for value in frequencies)), record


def _clusters(windows: list[_Window]) -> list[_Cluster]:
    observations = sorted(
        (frequency, window.start) for window in windows for frequency in window.frequencies
    )
    grouped: list[list[tuple[float, int]]] = []
    for observation in observations:
        if not grouped or observation[0] - grouped[-1][0][0] > CLUSTER_WIDTH_HZ:
            grouped.append([])
        grouped[-1].append(observation)
    return [
        _Cluster(
            float(np.median([value[0] for value in group])),
            tuple(sorted({value[1] for value in group})),
            group[-1][0] - group[0][0],
            len({value[1] for value in group}) != len(group),
        )
        for group in grouped
    ]


def _complete_windows(windows: list[_Window], clusters: list[_Cluster]) -> list[_Window]:
    return [
        window
        for window in windows
        if all(
            any(
                window.start in cluster.starts
                and abs(frequency - cluster.frequency) <= CLUSTER_WIDTH_HZ
                for cluster in clusters
            )
            for frequency in window.frequencies
        )
    ]


def _refinement_starts(windows: list[_Window], clusters: list[_Cluster]) -> list[int]:
    available = {window.start for window in windows}
    selected: set[int] = set()
    for cluster in clusters:
        starts = [start for start in cluster.starts if start in available]
        if starts:
            selected.update((starts[0], starts[len(starts) // 2], starts[-1]))
    return sorted(selected)


def _cost(source: Pcm, starts: list[int], frequencies: Pcm) -> float:
    return sum(_fit(source, start, frequencies).relative_rms ** 2 for start in starts)


def _normal_step(source: Pcm, starts: list[int], frequencies: Pcm) -> tuple[Pcm, float]:
    count = len(frequencies)
    normal = np.zeros((count, count))
    gradient = np.zeros(count)
    for start in starts:
        fit = _fit(source, start, frequencies)
        if fit.condition > MAX_DESIGN_CONDITION:
            return np.zeros(count), float("inf")
        phase = 2 * np.pi * _LOCAL_TIME[:, None] * frequencies[None, :]
        derivative = (
            2
            * np.pi
            * _LOCAL_TIME[:, None, None]
            * (
                -np.sin(phase)[:, None, :] * fit.coefficients[:count].T[None, :, :]
                + np.cos(phase)[:, None, :] * fit.coefficients[count:].T[None, :, :]
            )
        )
        orthogonal, _ = np.linalg.qr(fit.design, mode="reduced")
        flat = derivative.reshape(WINDOW_FRAMES, 2 * count)
        flat -= orthogonal @ (orthogonal.T @ flat)
        jacobian = flat.reshape(WINDOW_FRAMES * 2, count) / fit.source_norm
        residual = fit.residual.reshape(WINDOW_FRAMES * 2) / fit.source_norm
        normal += jacobian.T @ jacobian
        gradient += jacobian.T @ residual
    condition = float(np.linalg.cond(normal))
    if not np.isfinite(condition) or condition > MAX_REFINEMENT_NORMAL_CONDITION:
        return np.zeros(count), condition
    return np.linalg.solve(normal, gradient), condition


def _refine(source: Pcm, starts: list[int], initial: Pcm) -> tuple[Pcm, dict, bool]:
    frequencies = initial.copy()
    report: dict = {"starts": starts, "iterations": 0, "normal_conditions": []}
    try:
        cost = _cost(source, starts, frequencies)
        report["initial_sum_relative_error_squared"] = cost
        for iteration in range(REFINEMENT_ITERATIONS):
            step, condition = _normal_step(source, starts, frequencies)
            report["normal_conditions"].append(condition if np.isfinite(condition) else None)
            if not np.isfinite(condition) or condition > MAX_REFINEMENT_NORMAL_CONDITION:
                report["rejection"] = "ill_conditioned_frequency_refinement"
                return frequencies, report, False
            if float(np.max(np.abs(step))) < REFINEMENT_STOP_HZ:
                break
            step = np.clip(step, -MAX_REFINEMENT_STEP_HZ, MAX_REFINEMENT_STEP_HZ)
            for scale in (1.0, 0.5, 0.25, 0.125):
                trial = np.clip(
                    frequencies + scale * step,
                    np.maximum(initial - CLUSTER_WIDTH_HZ, MIN_FREQUENCY_HZ),
                    np.minimum(initial + CLUSTER_WIDTH_HZ, MAX_FREQUENCY_HZ),
                )
                trial_cost = _cost(source, starts, trial)
                if trial_cost < cost:
                    frequencies, cost = trial, trial_cost
                    break
            else:
                report["stopped_without_decreasing_step"] = True
                break
            report["iterations"] = iteration + 1
    except np.linalg.LinAlgError:
        report["rejection"] = "frequency_refinement_factorization_failed"
        return frequencies, report, False
    report["final_sum_relative_error_squared"] = cost
    report["frequencies_hz"] = frequencies.tolist()
    return frequencies, report, True


def _admit(
    source: Pcm, windows: list[_Window], clusters: list[_Cluster], frequencies: Pcm
) -> tuple[tuple[float, ...], ComplexPcm, list[dict]]:
    observations: list[list[tuple[float, int, ComplexPcm, float]]] = [[] for _ in frequencies]
    for window in windows:
        try:
            fit = _fit(source, window.start, frequencies)
        except np.linalg.LinAlgError:
            continue
        if fit.condition > MAX_DESIGN_CONDITION or fit.relative_rms > MAX_RELATIVE_FIT_RMS:
            continue
        coefficients = _global_coefficients(fit, window.start, frequencies)
        for index, cluster in enumerate(clusters):
            amplitude = float(np.linalg.norm(coefficients[index]))
            if window.start in cluster.starts and amplitude > 0:
                observations[index].append((
                    amplitude * fit.source_peak,
                    window.start,
                    coefficients[index] / amplitude,
                    fit.relative_rms,
                ))
    admitted_frequencies, admitted_coefficients, reports = [], [], []
    for index, values in enumerate(observations):
        record: dict = {
            "frequency_hz": float(frequencies[index]),
            "clean_support_windows": len(values),
            "admitted": False,
        }
        if len(values) < MIN_SUPPORT_WINDOWS:
            record["rejection"] = "insufficient_refined_clean_support"
        else:
            reference = max(values, key=lambda value: (value[0], -value[1]))
            distance = max(float(np.linalg.norm(value[2] - reference[2])) for value in values)
            record.update(
                coefficient_reference_start=reference[1],
                maximum_normalized_coefficient_distance=distance,
                maximum_support_source_fit_relative_rms=max(value[3] for value in values),
            )
            if distance > MAX_COEFFICIENT_DISTANCE:
                record["rejection"] = "incoherent_global_source_stereo_phase"
            else:
                record["admitted"] = True
                admitted_frequencies.append(float(frequencies[index]))
                admitted_coefficients.append(reference[2])
        reports.append(record)
    coefficients = np.asarray(admitted_coefficients, dtype=np.complex128).reshape(-1, 2)
    return tuple(admitted_frequencies), coefficients, reports


def _scan_windows(source: Pcm, report: dict) -> list[_Window]:
    windows, window_reports = [], []
    for start in range(0, len(source) - WINDOW_FRAMES + 1, HOP_FRAMES):
        window, window_report = _analyze_window(source, start)
        window_reports.append(window_report)
        if window is not None:
            windows.append(window)
    rejections: dict[str, int] = {}
    for value in window_reports:
        if "rejection" in value:
            reason = value["rejection"]
            rejections[reason] = rejections.get(reason, 0) + 1
    report.update(
        raw_window_count=len(window_reports),
        accepted_window_count=len(windows),
        window_rejection_counts=rejections,
        windows=window_reports,
    )
    return windows


def _eligible_clusters(windows: list[_Window], report: dict) -> list[_Cluster]:
    clusters = _clusters(windows)
    report["clusters"] = [
        {
            "median_frequency_hz": cluster.frequency,
            "full_spread_hz": cluster.spread,
            "distinct_windows": len(cluster.starts),
            "duplicate_component_in_window": cluster.duplicate_window,
            "eligible": len(cluster.starts) >= MIN_SUPPORT_WINDOWS and not cluster.duplicate_window,
        }
        for cluster in clusters
    ]
    return [
        cluster
        for cluster in clusters
        if len(cluster.starts) >= MIN_SUPPORT_WINDOWS and not cluster.duplicate_window
    ]


def _refined_estimate(
    source: Pcm, windows: list[_Window], eligible: list[_Cluster], report: dict
) -> CarrierEstimate:
    empty = CarrierEstimate((), np.empty((0, 2), dtype=np.complex128), report)
    complete = _complete_windows(windows, eligible)
    starts = _refinement_starts(complete, eligible)
    report["complete_clean_window_count"] = len(complete)
    if not starts:
        report.update(rejection="no_complete_clean_refinement_windows", admitted_carrier_count=0)
        return empty
    frequencies, refinement_report, valid = _refine(
        source, starts, np.array([cluster.frequency for cluster in eligible])
    )
    report["refinement"] = refinement_report
    if not valid:
        report.update(rejection="uncertain_shared_frequency_refinement", admitted_carrier_count=0)
        return empty
    admitted, coefficients, admissions = _admit(source, complete, eligible, frequencies)
    report.update(admissions=admissions, admitted_carrier_count=len(admitted))
    return CarrierEstimate(admitted, coefficients, report)


def estimate_carriers(source: Pcm) -> CarrierEstimate:
    """Discover recurring stationary carriers using only finite stereo source PCM.

    Args:
        source: Finite real PCM with shape (frames, 2), sampled at fixed 48000 Hz.

    Returns:
        Frequencies, unit pooled-norm complex stereo coefficients in global
        source coordinates, and source-window/refinement/admission diagnostics.
        Rejection is conservative; it never authorizes dropping source samples.

    Raises:
        ValueError: If the PCM is complex, nonfinite or not stereo.
    """
    if np.iscomplexobj(source):
        message = "Carrier estimation requires real stereo PCM"
        raise ValueError(message)
    pcm = np.asarray(source, dtype=np.float64)
    if pcm.ndim != 2 or pcm.shape[1] != 2 or not np.all(np.isfinite(pcm)):
        message = "Carrier estimation requires finite PCM with shape (frames, 2)"
        raise ValueError(message)
    report: dict = {
        "method": "pooled_stereo_thin_svd_matrix_pencil_with_conservative_stationary_admission",
        "source_frames": len(pcm),
        "parameters": _parameters(),
        "phase_coordinates": "global source sample zero; no event phase reset",
        "unadmitted_samples": "caller exact remainder; this estimator discards no PCM",
    }
    windows = _scan_windows(pcm, report)
    eligible = _eligible_clusters(windows, report)
    if not eligible or len(eligible) > MAX_CARRIERS:
        report["rejection"] = "no_recurrent_carriers" if not eligible else "carrier_cap_exceeded"
        report["admitted_carrier_count"] = 0
        return CarrierEstimate((), np.empty((0, 2), dtype=np.complex128), report)
    return _refined_estimate(pcm, windows, eligible, report)


def _control_errors(source: Pcm, expected: Pcm, coefficients: ComplexPcm) -> tuple[float, float]:
    estimate = estimate_carriers(source)
    frequencies = np.array(estimate.frequencies_hz)
    frequency_error = (
        float(np.max(np.abs(frequencies - expected))) if len(frequencies) == 2 else float("inf")
    )
    normalized = coefficients / np.linalg.norm(coefficients, axis=1)[:, None]
    coefficient_error = (
        float(np.max(np.linalg.norm(estimate.stereo_coefficients - normalized, axis=1)))
        if len(frequencies) == 2
        else float("inf")
    )
    return frequency_error, coefficient_error


def _unseen_controls() -> tuple[float, float]:
    count = 32768
    clock = np.arange(count) / FS
    expected = np.array([537.125, 1729.75])
    coincident_parts = np.array([[0.17 + 0.04j, -0.02 + 0.12j], [0.03 - 0.07j, 0.08j]])
    coefficients = np.array([np.sum(coincident_parts, axis=0), [0.03j, 0.11 - 0.02j]])
    source = np.real(np.exp(2j * np.pi * clock[:, None] * expected) @ coefficients)
    return _control_errors(source, expected, coefficients)


def _gated_controls() -> tuple[float, float]:
    gated = np.zeros((45056, 2))
    gated_expected = np.array([683.375, 1441.625])
    gated_coefficients = np.array([[0.19 - 0.06j, -0.07j], [0.04 + 0.03j, 0.13 + 0.02j]])
    for index, (start, stop) in enumerate(((8192, 40960), (4096, 36864))):
        global_clock = np.arange(start, stop) / FS
        gated[start:stop] += np.real(
            np.exp(2j * np.pi * gated_expected[index] * global_clock)[:, None]
            * gated_coefficients[index]
        )
    return _control_errors(gated, gated_expected, gated_coefficients)


def _rejection_controls() -> dict:
    short = np.zeros((24576, 2))
    short[3000:3384, 0] = np.sin(2 * np.pi * 1234.5 * np.arange(384) / FS)
    impulse = np.zeros_like(short)
    impulse[9000] = (0.7, -0.4)
    noise = np.random.default_rng(94027).standard_normal((WINDOW_FRAMES, 2))
    return {
        "short_384_frame_tone": len(estimate_carriers(short).frequencies_hz),
        "impulse": len(estimate_carriers(impulse).frequencies_hz),
        "deterministic_noise": len(estimate_carriers(noise).frequencies_hz),
        "silence_and_exact_coincident_cancellation": len(
            estimate_carriers(np.zeros_like(short)).frequencies_hz
        ),
    }


def integrity_controls() -> dict:
    """Exercise estimation with unseen tones, global phase and conservative rejects.

    Analytic frequencies in private control helpers are scoring truth only.
    They are never passed to estimate_carriers, and do not establish acoustic
    or product acceptance. Controls execute only when the caller invokes them.
    """
    frequency_error, coefficient_error = _unseen_controls()
    gated_frequency_error, gated_coefficient_error = _gated_controls()
    rejected = _rejection_controls()
    passed = (
        max(frequency_error, gated_frequency_error) < 1.0e-4
        and max(coefficient_error, gated_coefficient_error) < MAX_COEFFICIENT_DISTANCE
        and all(value == 0 for value in rejected.values())
    )
    assert passed, "Source-only stationary carrier integrity controls failed"
    return {
        "stage": "mode_carrier_integrity_controls",
        "passed": passed,
        "unseen_and_coincident_frequency_max_abs_error_hz": frequency_error,
        "unseen_and_coincident_global_coefficient_max_distance": coefficient_error,
        "independently_gated_frequency_max_abs_error_hz": gated_frequency_error,
        "independently_gated_global_coefficient_max_distance": gated_coefficient_error,
        "rejected_carrier_counts": rejected,
        "acoustic_acceptance": False,
    }
