"""Bounded source-only certification of one isolated linear chirplet.

Sign-changing zero crossings provide a blind initializer, never an admission
certificate. Complete original stereo windows must fit the integrated affine
frequency law after projected variable-projection refinement. Final coefficients
share the declared source epoch; no window phase alignment is permitted.

The caller explicitly selects this experiment after stationary admission returns
K=0. No FFT peaks, scorer targets, event metadata, SciPy, files or devices are
used. Every refusal leaves the caller's entire exact remainder authoritative.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING

import numpy as np
from pitch_mode_carriers import (
    CLUSTER_WIDTH_HZ,
    FS,
    HOP_FRAMES,
    MAX_COEFFICIENT_DISTANCE,
    MAX_CREST_FACTOR,
    MAX_DESIGN_CONDITION,
    MAX_FREQUENCY_HZ,
    MAX_REFINEMENT_NORMAL_CONDITION,
    MAX_RELATIVE_FIT_RMS,
    MIN_BLOCK_ENERGY_RATIO,
    MIN_FREQUENCY_HZ,
    MIN_SUPPORT_WINDOWS,
    REFINEMENT_ITERATIONS,
    WINDOW_FRAMES,
)
from pitch_source_phase import LinearSourcePhase

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Pcm = NDArray[np.float64]
type ComplexPcm = NDArray[np.complex128]

MAX_SCAN_WINDOWS = 4096
MAX_DIMENSIONLESS_STEP = 0.5
REFINEMENT_STOP_CYCLES = 1.0e-11
LINE_SEARCH = (1.0, 0.5, 0.25, 0.125)
_WINDOW_SCALE_SECONDS = WINDOW_FRAMES / FS
_WINDOW_OFFSETS = np.arange(WINDOW_FRAMES, dtype=np.float64)


@dataclass(slots=True)
class ChirpletEstimate:
    """One admitted phase and its unit pooled-norm complex stereo direction.

    The gauge means ``real(exp(2j*pi*phase.cycles(s))*coefficient)``. Empty
    estimates have no phase and coefficients with shape (0, 2).
    """

    phase: LinearSourcePhase | None
    stereo_coefficients: ComplexPcm
    report: dict


@dataclass(slots=True)
class _Window:
    start: int
    phase: LinearSourcePhase


@dataclass(slots=True)
class _Fit:
    design: Pcm
    coefficients: Pcm
    residual: Pcm
    relative_rms: float
    condition: float
    source_norm: float
    source_peak: float


def _finite(value: float) -> float | None:
    return float(value) if np.isfinite(value) else None


def _phase_report(phase: LinearSourcePhase) -> dict:
    return {
        "epoch_frame": phase.epoch_frame,
        "frequency_hz": phase.frequency_hz,
        "slope_hz_per_second": phase.slope_hz_per_second,
    }


def _parameters() -> dict:
    return {
        "sample_rate_hz": FS,
        "window_frames": WINDOW_FRAMES,
        "hop_frames": HOP_FRAMES,
        "minimum_distinct_support_windows": MIN_SUPPORT_WINDOWS,
        "minimum_eight_block_energy_ratio": MIN_BLOCK_ENERGY_RATIO,
        "maximum_crest_factor": MAX_CREST_FACTOR,
        "frequency_limits_hz": [MIN_FREQUENCY_HZ, MAX_FREQUENCY_HZ],
        "maximum_relative_fit_rms": MAX_RELATIVE_FIT_RMS,
        "maximum_design_condition": MAX_DESIGN_CONDITION,
        "maximum_normalized_coefficient_distance": MAX_COEFFICIENT_DISTANCE,
        "maximum_refinement_normal_condition": MAX_REFINEMENT_NORMAL_CONDITION,
        "affine_law_full_width_hz_on_active_hull": CLUSTER_WIDTH_HZ,
        "refinement_iterations": REFINEMENT_ITERATIONS,
        "refinement_line_search": list(LINE_SEARCH),
        "maximum_dimensionless_refinement_step_cycles": MAX_DIMENSIONLESS_STEP,
        "refinement_stop_cycles": REFINEMENT_STOP_CYCLES,
        "global_refinement_windows": "earliest/middle/latest complete associated support; <=3",
        "maximum_complete_scan_windows": MAX_SCAN_WINDOWS,
        "estimated_extra_working_memory_mib": 32,
        "working_memory_budget_mib": 64,
        "memory_accounting": (
            "window-only PCM/design/Jacobian/QR/lstsq arrays <4 MiB; "
            "bounded <=4096 compact window/refinement reports and associations <28 MiB; "
            "caller source storage excluded; no input-length feature or finite mask"
        ),
    }


def _source_hull(source: Pcm) -> tuple[int, int] | None:
    first, last = len(source), -1
    for start in range(0, len(source), WINDOW_FRAMES):
        block = source[start : start + WINDOW_FRAMES]
        if not np.isfinite(block).all():
            message = "Chirplet estimation requires finite real stereo PCM"
            raise ValueError(message)
        active = np.flatnonzero(np.any(block != 0, axis=1))
        if len(active):
            first = min(first, start + int(active[0]))
            last = start + int(active[-1])
    return (first, last + 1) if last >= first else None


def _normalized_window(source: Pcm, start: int) -> tuple[Pcm, float]:
    block = np.asarray(source[start : start + WINDOW_FRAMES], dtype=np.float64)
    peak = float(np.max(np.abs(block)))
    return (block / peak if peak else np.zeros_like(block)), peak


def _guarded_window(source: Pcm, start: int, record: dict) -> Pcm | None:
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


def _zero_crossing_seed(block: Pcm, start: int, record: dict) -> LinearSourcePhase | None:
    channel = int(np.argmax(np.sum(block * block, axis=0)))
    wave = block[:, channel]
    crossings = np.flatnonzero(np.signbit(wave[:-1]) != np.signbit(wave[1:]))
    record.update(seed_channel=channel, sign_changing_zero_crossings=len(crossings))
    if len(crossings) < 4:
        record["rejection"] = "insufficient_sign_changing_zero_crossings"
        return None
    left = np.abs(wave[crossings])
    right = np.abs(wave[crossings + 1])
    positions = crossings + left / (left + right)
    center = start + (WINDOW_FRAMES - 1) / 2
    # j=c+2*f_center*tau+alpha*tau**2; dimensionless coordinates keep the
    # regression independent of the Hz/seconds unit scales.
    coordinate = (positions - (WINDOW_FRAMES - 1) / 2) / WINDOW_FRAMES
    design = np.column_stack((np.ones_like(coordinate), coordinate, coordinate**2))
    try:
        coefficients, _, rank, singular = np.linalg.lstsq(
            design, np.arange(len(positions), dtype=np.float64), rcond=None
        )
    except np.linalg.LinAlgError:
        record["rejection"] = "zero_crossing_regression_failed"
        return None
    condition = float(singular[0] / singular[-1]) if singular[-1] else float("inf")
    record["zero_crossing_regression_condition"] = _finite(condition)
    if rank != 3 or condition > MAX_DESIGN_CONDITION or not np.isfinite(coefficients).all():
        record["rejection"] = "ill_conditioned_zero_crossing_regression"
        return None
    phase = LinearSourcePhase(
        center,
        float(coefficients[1] / (2 * _WINDOW_SCALE_SECONDS)),
        float(coefficients[2] / _WINDOW_SCALE_SECONDS**2),
    )
    record["zero_crossing_seed"] = _phase_report(phase)
    return phase


def _fit(source: Pcm, start: int, phase: LinearSourcePhase) -> _Fit:
    block, peak = _normalized_window(source, start)
    angle = 2 * np.pi * phase.cycles(start + _WINDOW_OFFSETS)
    design = np.column_stack((np.cos(angle), np.sin(angle)))
    coefficients, _, rank, singular = np.linalg.lstsq(design, block, rcond=None)
    condition = float(singular[0] / singular[-1]) if singular[-1] else float("inf")
    if rank != 2:
        condition = float("inf")
    residual = block - design @ coefficients
    source_norm = float(np.linalg.norm(block))
    relative = float(np.linalg.norm(residual)) / source_norm if source_norm else 0.0
    return _Fit(design, coefficients, residual, relative, condition, source_norm, peak)


def _phase_parameters(phase: LinearSourcePhase, scale_seconds: float) -> Pcm:
    return np.array([
        phase.frequency_hz * scale_seconds,
        0.5 * phase.slope_hz_per_second * scale_seconds**2,
    ])


def _parameter_phase(parameters: Pcm, epoch: float, scale_seconds: float) -> LinearSourcePhase:
    return LinearSourcePhase(
        epoch,
        float(parameters[0] / scale_seconds),
        float(2 * parameters[1] / scale_seconds**2),
    )


def _cost(source: Pcm, starts: list[int], phase: LinearSourcePhase) -> float:
    return sum(_fit(source, start, phase).relative_rms ** 2 for start in starts)


def _normal_step(
    source: Pcm, starts: list[int], phase: LinearSourcePhase, scale_seconds: float
) -> tuple[Pcm, float]:
    normal = np.zeros((2, 2))
    gradient = np.zeros(2)
    for start in starts:
        fit = _fit(source, start, phase)
        if fit.condition > MAX_DESIGN_CONDITION or not fit.source_norm:
            return np.zeros(2), float("inf")
        coordinate = (start + _WINDOW_OFFSETS - phase.epoch_frame) / (FS * scale_seconds)
        features = np.column_stack((coordinate, coordinate**2))
        carrier_derivative = (
            -fit.design[:, 1, None] * fit.coefficients[0]
            + fit.design[:, 0, None] * fit.coefficients[1]
        )
        derivative = 2 * np.pi * carrier_derivative[:, :, None] * features[:, None, :]
        orthogonal, _ = np.linalg.qr(fit.design, mode="reduced")
        flat = derivative.reshape(WINDOW_FRAMES, 4)
        flat -= orthogonal @ (orthogonal.T @ flat)
        jacobian = flat.reshape(WINDOW_FRAMES * 2, 2) / fit.source_norm
        residual = fit.residual.reshape(WINDOW_FRAMES * 2) / fit.source_norm
        normal += jacobian.T @ jacobian
        gradient += jacobian.T @ residual
    condition = float(np.linalg.cond(normal))
    if not np.isfinite(condition) or condition > MAX_REFINEMENT_NORMAL_CONDITION:
        return np.zeros(2), condition
    return np.linalg.solve(normal, gradient), condition


def _refine(
    source: Pcm, starts: list[int], initial: LinearSourcePhase, scale_seconds: float
) -> tuple[LinearSourcePhase, dict, bool]:
    phase = initial
    parameters = _phase_parameters(phase, scale_seconds)
    report: dict = {
        "starts": starts,
        "iterations": 0,
        "normal_conditions": [],
        "dimensionless_time_scale_seconds": scale_seconds,
        "initial_phase": _phase_report(initial),
    }
    try:
        cost = _cost(source, starts, phase)
        report["initial_sum_relative_error_squared"] = cost
        for iteration in range(REFINEMENT_ITERATIONS):
            step, condition = _normal_step(source, starts, phase, scale_seconds)
            report["normal_conditions"].append(_finite(condition))
            if not np.isfinite(condition) or condition > MAX_REFINEMENT_NORMAL_CONDITION:
                report["rejection"] = "ill_conditioned_joint_phase_refinement"
                return phase, report, False
            if float(np.max(np.abs(step))) < REFINEMENT_STOP_CYCLES:
                break
            step = np.clip(step, -MAX_DIMENSIONLESS_STEP, MAX_DIMENSIONLESS_STEP)
            for scale in LINE_SEARCH:
                trial_parameters = parameters + scale * step
                trial = _parameter_phase(trial_parameters, initial.epoch_frame, scale_seconds)
                trial_cost = _cost(source, starts, trial)
                if trial_cost < cost:
                    phase, parameters, cost = trial, trial_parameters, trial_cost
                    break
            else:
                report["stopped_without_decreasing_step"] = True
                break
            report["iterations"] = iteration + 1
    except np.linalg.LinAlgError:
        report["rejection"] = "joint_phase_refinement_factorization_failed"
        return phase, report, False
    report.update(final_sum_relative_error_squared=cost, phase=_phase_report(phase))
    return phase, report, True


def _within_band(phase: LinearSourcePhase, first: float, last: float) -> bool:
    frequencies = phase.frequency_at(np.array([first, last]))
    return bool(
        np.isfinite(frequencies).all()
        and np.all(frequencies >= MIN_FREQUENCY_HZ)
        and np.all(frequencies <= MAX_FREQUENCY_HZ)
    )


def _certify_window(
    source: Pcm, start: int, phase: LinearSourcePhase, record: dict
) -> tuple[_Window | None, dict]:
    try:
        fit = _fit(source, start, phase)
    except np.linalg.LinAlgError:
        record["rejection"] = "source_fit_factorization_failed"
        return None, record
    record.update(
        phase=_phase_report(phase),
        source_fit_relative_rms=fit.relative_rms,
        source_fit_condition=_finite(fit.condition),
    )
    if not _within_band(phase, start, start + WINDOW_FRAMES - 1):
        record["rejection"] = "refined_source_window_frequency_outside_band"
        return None, record
    if fit.condition > MAX_DESIGN_CONDITION or fit.relative_rms > MAX_RELATIVE_FIT_RMS:
        record["rejection"] = "poor_linear_chirplet_source_fit"
        return None, record
    record["accepted"] = True
    return _Window(start, phase), record


def _analyze_window(source: Pcm, start: int) -> tuple[_Window | None, dict]:
    record: dict = {"start": start, "accepted": False}
    block = _guarded_window(source, start, record)
    if block is None:
        return None, record
    seed = _zero_crossing_seed(block, start, record)
    if seed is None:
        return None, record
    if not _within_band(seed, start, start + WINDOW_FRAMES - 1):
        record["rejection"] = "zero_crossing_seed_outside_frequency_band"
        return None, record
    phase, refinement, valid = _refine(source, [start], seed, _WINDOW_SCALE_SECONDS)
    record["refinement"] = refinement
    if not valid:
        record["rejection"] = "uncertain_local_linear_phase_refinement"
        return None, record
    return _certify_window(source, start, phase, record)


def _same_law(left: _Window, right: _Window, hull: tuple[int, int]) -> bool:
    positions = np.array([hull[0], hull[1] - 1], dtype=np.float64)
    distance = np.max(
        np.abs(left.phase.frequency_at(positions) - right.phase.frequency_at(positions))
    )
    return bool(distance <= CLUSTER_WIDTH_HZ)


def _associate(
    windows: list[_Window], hull: tuple[int, int], epoch: float, report: dict
) -> list[tuple[list[_Window], LinearSourcePhase, dict]]:
    groups: list[list[_Window]] = []
    for window in windows:
        compatible = [
            index
            for index, group in enumerate(groups)
            if all(_same_law(window, member, hull) for member in group)
        ]
        if len(compatible) == 1:
            groups[compatible[0]].append(window)
        elif not compatible:
            groups.append([window])
        else:
            report["ambiguous_window_association"] = window.start
            return []
    candidates = []
    for group in groups:
        frequencies = [float(window.phase.frequency_at(epoch)) for window in group]
        slopes = [window.phase.slope_hz_per_second for window in group]
        phase = LinearSourcePhase(epoch, float(np.median(frequencies)), float(np.median(slopes)))
        record = {
            "phase": _phase_report(phase),
            "distinct_complete_support_windows": len(group),
            "starts": [window.start for window in group],
            "eligible": len(group) >= MIN_SUPPORT_WINDOWS,
        }
        if not record["eligible"]:
            record["rejection"] = "insufficient_associated_support_windows"
        candidates.append((group, phase, record))
    report["candidates"] = [candidate[2] for candidate in candidates]
    return [candidate for candidate in candidates if candidate[2]["eligible"]]


def _admit(
    source: Pcm, windows: list[_Window], phase: LinearSourcePhase
) -> tuple[ComplexPcm, dict]:
    observations = []
    support_reports = []
    for window in windows:
        record: dict = {"start": window.start, "accepted": False}
        try:
            fit = _fit(source, window.start, phase)
        except np.linalg.LinAlgError:
            record["rejection"] = "shared_source_fit_factorization_failed"
            support_reports.append(record)
            continue
        record.update(
            source_fit_relative_rms=fit.relative_rms,
            source_fit_condition=_finite(fit.condition),
        )
        coefficient = fit.coefficients[0] - 1j * fit.coefficients[1]
        amplitude = float(np.linalg.norm(coefficient))
        if (
            fit.condition > MAX_DESIGN_CONDITION
            or fit.relative_rms > MAX_RELATIVE_FIT_RMS
            or not amplitude
        ):
            record["rejection"] = "poor_shared_linear_source_fit"
        else:
            record["accepted"] = True
            observations.append((
                amplitude * fit.source_peak,
                window.start,
                coefficient / amplitude,
            ))
        support_reports.append(record)
    report: dict = {
        "admitted": False,
        "clean_support_windows": len(observations),
        "support_windows": support_reports,
    }
    empty = np.empty((0, 2), dtype=np.complex128)
    if len(observations) < MIN_SUPPORT_WINDOWS:
        report["rejection"] = "insufficient_refined_clean_support"
        return empty, report
    if len(observations) != len(windows):
        report["rejection"] = "inconsistent_shared_source_phase_fit"
        return empty, report
    reference = max(observations, key=lambda value: (value[0], -value[1]))
    distance = max(float(np.linalg.norm(value[2] - reference[2])) for value in observations)
    report.update(
        coefficient_reference_start=reference[1],
        maximum_normalized_coefficient_distance=distance,
        maximum_support_source_fit_relative_rms=max(
            record["source_fit_relative_rms"] for record in support_reports
        ),
        comparison_gauge="declared common source epoch; no per-window phase alignment",
    )
    if distance > MAX_COEFFICIENT_DISTANCE:
        report["rejection"] = "incoherent_global_source_stereo_phase"
        return empty, report
    report["admitted"] = True
    return np.asarray([reference[2]], dtype=np.complex128), report


def _refined_estimate(
    source: Pcm,
    windows: list[_Window],
    initial: LinearSourcePhase,
    hull: tuple[int, int],
    report: dict,
) -> ChirpletEstimate:
    empty = ChirpletEstimate(None, np.empty((0, 2), dtype=np.complex128), report)
    starts = sorted({windows[0].start, windows[len(windows) // 2].start, windows[-1].start})
    scale = max(
        _WINDOW_SCALE_SECONDS,
        max(abs(hull[0] - initial.epoch_frame), abs(hull[1] - 1 - initial.epoch_frame)) / FS,
    )
    phase, refinement, valid = _refine(source, starts, initial, scale)
    report["refinement"] = refinement
    if not valid:
        report["rejection"] = "uncertain_shared_linear_phase_refinement"
        return empty
    endpoints = phase.frequency_at(np.array([hull[0], hull[1] - 1]))
    report["active_hull_endpoint_frequencies_hz"] = endpoints.tolist()
    if not _within_band(phase, hull[0], hull[1] - 1):
        report["rejection"] = "source_active_hull_frequency_outside_band"
        return empty
    coefficients, admission = _admit(source, windows, phase)
    report["admissions"] = [admission]
    if not admission["admitted"]:
        report["rejection"] = admission["rejection"]
        return empty
    report.update(admitted_carrier_count=1, phase=_phase_report(phase))
    return ChirpletEstimate(phase, coefficients, report)


def estimate_chirplet(source: Pcm) -> ChirpletEstimate:
    """Certify one source-only linear chirplet, or retain an explicit refusal.

    Args:
        source: Finite real stereo PCM with shape (frames, 2) at 48000 Hz.

    Returns:
        A common-epoch integrated phase, unit complex stereo direction and
        bounded source scan, association, refinement and admission evidence.
        The caller remains responsible for exact subtraction, envelopes and
        guarding the full fitted mode plus interpolation support at render time.

    Raises:
        ValueError: If the PCM is complex, nonfinite or not a real stereo array.
    """
    pcm = np.asarray(source)
    if (
        pcm.ndim != 2
        or pcm.shape[1] != 2
        or np.iscomplexobj(pcm)
        or not np.issubdtype(pcm.dtype, np.number)
    ):
        message = "Chirplet estimation requires finite real PCM with shape (frames, 2)"
        raise ValueError(message)
    hull = _source_hull(pcm)
    report: dict = {
        "method": "source_zero_crossing_seed_with_stereo_projected_linear_phase_certification",
        "source_frames": len(pcm),
        "parameters": _parameters(),
        "phase_coordinates": "fixed epoch near active source support center; no event phase reset",
        "source_active_hull_frames": list(hull) if hull else None,
        "unadmitted_samples": "caller exact remainder; this estimator discards no PCM",
        "admitted_carrier_count": 0,
    }
    empty = ChirpletEstimate(None, np.empty((0, 2), dtype=np.complex128), report)
    window_count = max(0, (len(pcm) - WINDOW_FRAMES) // HOP_FRAMES + 1)
    report["raw_window_count"] = window_count
    if hull is None or window_count < MIN_SUPPORT_WINDOWS:
        report["rejection"] = (
            "zero_source" if hull is None else "insufficient_complete_source_windows"
        )
        return empty
    if window_count > MAX_SCAN_WINDOWS:
        report["rejection"] = "source_scan_window_budget_exceeded"
        return empty
    windows, records = [], []
    for start in range(0, len(pcm) - WINDOW_FRAMES + 1, HOP_FRAMES):
        window, record = _analyze_window(pcm, start)
        records.append(record)
        if window is not None:
            windows.append(window)
    rejections: dict[str, int] = {}
    for record in records:
        if "rejection" in record:
            reason = record["rejection"]
            rejections[reason] = rejections.get(reason, 0) + 1
    report.update(
        accepted_window_count=len(windows), windows=records, window_rejection_counts=rejections
    )
    epoch = (hull[0] + hull[1] - 1) / 2
    eligible = _associate(windows, hull, epoch, report)
    if "ambiguous_window_association" in report:
        report["rejection"] = "ambiguous_affine_law_association"
        return empty
    if len(eligible) != 1:
        report["rejection"] = (
            "no_recurrent_linear_chirplet"
            if not eligible
            else "multiple_incompatible_supported_chirplets"
        )
        return empty
    candidate = eligible[0]
    return _refined_estimate(pcm, candidate[0], candidate[1], hull, report)
