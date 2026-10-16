"""Frozen offline centered phase-gradient candidate with complete signed capture.

This NumPy-only experiment adapts Prusa/Holighaus phase-gradient integration.
Its shared stereo correction, finite signed wrapper and causal SRC are custom.
It provides no Native, device, live-crossing or listening acceptance evidence.
"""

import heapq
import math
from dataclasses import asdict, dataclass
from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Pcm = NDArray[np.float64]
type Spectrum = NDArray[np.complex128]
type Indices = NDArray[np.int64]
type Mask = NDArray[np.bool_]

WINDOW_FRAMES = 4096
HALF_WINDOW = WINDOW_FRAMES // 2
ANALYSIS_STEP = 256
PRIORITY_TOLERANCE = 1.0e-6
SRC_RADIUS = 32
SRC_BETA = 8.6
SRC_BLOCK_FRAMES = 2048
_BIN_STEP = 2.0 * np.pi / WINDOW_FRAMES
_OMEGA = _BIN_STEP * np.arange(HALF_WINDOW + 1)
_WINDOW = 0.5 - 0.5 * np.cos(2.0 * np.pi * np.arange(WINDOW_FRAMES) / WINDOW_FRAMES)


@dataclass(slots=True)
class _Statistics:
    pooled_connection_fallbacks: int = 0
    invalid_time_edges: int = 0
    invalid_frequency_edges: int = 0
    temporal_assignments: int = 0
    vertical_assignments: int = 0
    component_seeds: int = 0
    dc_component_seeds: int = 0
    below_priority_coefficients: int = 0
    identity_phase_frames: int = 0
    unsupported_alpha_count: int = 0


@dataclass(slots=True)
class _Gradients:
    time: Pcm
    frequency: Pcm
    time_connections: Pcm
    valid_time: Mask
    frequency_connections: Pcm
    valid_frequency: Mask


@dataclass(slots=True)
class _FrameState:
    correction: Pcm
    unknown: Mask
    remaining: int
    heap: list[tuple[float, int, int]]
    priority: Pcm
    frequency: Pcm
    connections: Pcm
    valid_connections: Mask


@dataclass(slots=True)
class _Grid:
    analysis: Indices
    synthesis: Indices
    alpha: float
    rounding: Pcm


def _principal(angles: Pcm) -> Pcm:
    """Return principal angles with a fixed negative-pi tie convention."""
    return (angles + np.pi) % (2.0 * np.pi) - np.pi


def _read_zero_extended(source: Pcm, indices: Indices, origin: int = 0) -> Pcm:
    local = indices - origin
    values = np.zeros((*indices.shape, 2), dtype=np.float64)
    inside = (local >= 0) & (local < len(source))
    values[inside] = source[local[inside]]
    return values


def _analyze(source: Pcm, centers: Indices, window: Pcm) -> Spectrum:
    length = len(window)
    half = length // 2
    offsets = np.arange(length, dtype=np.int64) - half
    omega = 2.0 * np.pi * np.arange(half + 1) / length
    gauge = np.exp(1j * omega * half)
    coefficients = np.empty((len(centers), half + 1, 2), dtype=np.complex128)
    for index, center in enumerate(centers):
        packet = _read_zero_extended(source, center + offsets) * window[:, None]
        coefficients[index] = np.fft.rfft(packet, axis=0) * gauge[:, None]
    return coefficients


def _connections(left: Spectrum, right: Spectrum) -> tuple[Pcm, Mask, int]:
    """Use pooled angular connections, falling back only on exact cancellation."""
    products = np.conj(left) * right
    pooled = np.sum(products, axis=-1)
    selected_channel = np.argmax(np.abs(products), axis=-1)
    selected = np.take_along_axis(products, selected_channel[..., None], axis=-1)[..., 0]
    fallback = (pooled == 0.0) & (selected != 0.0)
    pooled[fallback] = selected[fallback]
    return _principal(np.angle(pooled)), pooled != 0.0, int(np.count_nonzero(fallback))


def _gradients(coefficients: Spectrum, centers: Indices, statistics: _Statistics) -> _Gradients:
    qt, valid_time, time_fallbacks = _connections(coefficients[:-1], coefficients[1:])
    qf, valid_frequency, frequency_fallbacks = _connections(
        coefficients[:, :-1], coefficients[:, 1:]
    )
    statistics.pooled_connection_fallbacks += time_fallbacks + frequency_fallbacks
    statistics.invalid_time_edges += int(np.count_nonzero(~valid_time))
    statistics.invalid_frequency_edges += int(np.count_nonzero(~valid_frequency))
    steps = np.diff(centers)[:, None]
    edge_time = _OMEGA + _principal(qt - steps * _OMEGA) / steps
    edge_time[~valid_time] = np.broadcast_to(_OMEGA, edge_time.shape)[~valid_time]
    time = np.empty(coefficients.shape[:2], dtype=np.float64)
    time[0], time[-1] = edge_time[0], edge_time[-1]
    time[1:-1] = 0.5 * (edge_time[:-1] + edge_time[1:])
    frequency = np.empty_like(time)
    frequency[:, 0], frequency[:, -1] = qf[:, 0], qf[:, -1]
    frequency[:, 1:-1] = 0.5 * (qf[:, :-1] + qf[:, 1:])
    return _Gradients(time, frequency, qt, valid_time, qf, valid_frequency)


def _desired_time(previous: Pcm, current: Pcm, delta_a: int, delta_d: float, alpha: float) -> Pcm:
    return alpha * delta_a * 0.5 * (previous + current) + _OMEGA * delta_d


def _assign(state: _FrameState, index: int, correction: float) -> None:
    if index == 0 or index + 1 == len(state.correction):
        correction = 0.0 if np.cos(correction) >= 0.0 else math.pi
    state.correction[index] = correction
    state.unknown[index] = False
    state.remaining -= 1
    heapq.heappush(state.heap, (-float(state.priority[index]), 1, index))


def _seed_component(
    state: _FrameState, time: Pcm, center_difference: int, statistics: _Statistics
) -> None:
    candidates = np.flatnonzero(state.unknown)
    index = int(candidates[np.argmax(state.priority[candidates])])
    if state.unknown[0]:
        index = 0
        correction = 0.0
        statistics.dc_component_seeds += 1
    else:
        correction = float(time[index] * center_difference)
    statistics.component_seeds += 1
    _assign(state, index, correction)


def _spread_frequency(state: _FrameState, index: int, statistics: _Statistics) -> None:
    for direction in (-1, 1):
        neighbor = index + direction
        if not 0 <= neighbor < len(state.unknown) or not state.unknown[neighbor]:
            continue
        edge = min(index, neighbor)
        if not state.valid_connections[edge]:
            continue
        desired = 0.5 * (state.frequency[index] + state.frequency[neighbor])
        correction = state.correction[index] + direction * (desired - state.connections[edge])
        _assign(state, neighbor, float(correction))
        statistics.vertical_assignments += 1


def _new_frame(
    index: int,
    priority: Pcm,
    gradients: _Gradients,
    grid: _Grid,
    statistics: _Statistics,
) -> _FrameState:
    current_priority = priority[index]
    previous_priority = priority[index - 1] if index else np.zeros_like(current_priority)
    threshold = PRIORITY_TOLERANCE * max(np.max(current_priority), np.max(previous_priority))
    active = current_priority > threshold
    statistics.below_priority_coefficients += int(np.count_nonzero(~active))
    state = _FrameState(
        np.zeros_like(current_priority),
        active.copy(),
        int(np.count_nonzero(active)),
        [],
        current_priority,
        grid.alpha * gradients.frequency[index] + _BIN_STEP * grid.rounding[index],
        gradients.frequency_connections[index],
        gradients.valid_frequency[index],
    )
    if index:
        eligible = active & (previous_priority > threshold) & gradients.valid_time[index - 1]
        state.heap = [(-float(previous_priority[k]), 0, int(k)) for k in np.flatnonzero(eligible)]
        heapq.heapify(state.heap)
    return state


def _frame_time(index: int, gradients: _Gradients, grid: _Grid) -> Pcm:
    if not index:
        return np.zeros_like(gradients.time[0])
    return _desired_time(
        gradients.time[index - 1],
        gradients.time[index],
        int(grid.analysis[index] - grid.analysis[index - 1]),
        float(grid.rounding[index] - grid.rounding[index - 1]),
        grid.alpha,
    )


def _integrate_frame(
    index: int,
    previous: Pcm,
    priority: Pcm,
    gradients: _Gradients,
    grid: _Grid,
    statistics: _Statistics,
) -> Pcm:
    state = _new_frame(index, priority, gradients, grid, statistics)
    desired_time = _frame_time(index, gradients, grid)
    while state.remaining:
        if not state.heap:
            _seed_component(
                state,
                gradients.time[index],
                int(grid.synthesis[index] - grid.analysis[index]),
                statistics,
            )
        _, frame_kind, bin_index = heapq.heappop(state.heap)
        if frame_kind:
            _spread_frequency(state, bin_index, statistics)
        elif state.unknown[bin_index]:
            correction = (
                previous[bin_index]
                + desired_time[bin_index]
                - gradients.time_connections[index - 1, bin_index]
            )
            _assign(state, bin_index, float(correction))
            statistics.temporal_assignments += 1
    return state.correction


def _correct_phases(coefficients: Spectrum, grid: _Grid) -> tuple[Spectrum, _Statistics]:
    statistics = _Statistics()
    if grid.alpha == 1.0:
        statistics.identity_phase_frames = len(grid.analysis)
        return coefficients, statistics
    gradients = _gradients(coefficients, grid.analysis, statistics)
    priority = np.sqrt(np.sum(np.abs(coefficients) ** 2, axis=-1))
    output = np.empty_like(coefficients)
    previous = np.zeros(coefficients.shape[1], dtype=np.float64)
    for index in range(len(grid.analysis)):
        correction = _integrate_frame(index, previous, priority, gradients, grid, statistics)
        multiplier = np.exp(1j * correction)
        multiplier[[0, -1]] = np.where(np.cos(correction[[0, -1]]) >= 0.0, 1.0, -1.0)
        output[index] = coefficients[index] * multiplier[:, None]
        previous = correction
    return output, statistics


def _inverse_packet(coefficient: Spectrum, inverse_gauge: Spectrum, length: int) -> Pcm:
    packet_coefficients = coefficient * inverse_gauge[:, None]
    packet_coefficients[[0, -1]] = packet_coefficients[[0, -1]].real
    return np.fft.irfft(packet_coefficients, n=length, axis=0)


def _overlap_add(coefficients: Spectrum, centers: Indices, window: Pcm) -> tuple[Pcm, int]:
    """Use all packet windows in the static dual, retaining the full signed support."""
    starts = centers - len(window) // 2
    origin = int(starts[0])
    numerator = np.zeros((int(starts[-1]) + len(window) - origin, 2), dtype=np.float64)
    denominator = np.zeros(len(numerator), dtype=np.float64)
    inverse_gauge = np.exp(
        -1j * 2.0 * np.pi * np.arange(len(window) // 2 + 1) / len(window) * (len(window) // 2)
    )
    for coefficient, start in zip(coefficients, starts, strict=True):
        local = int(start) - origin
        packet = _inverse_packet(coefficient, inverse_gauge, len(window))
        numerator[local : local + len(window)] += packet * window[:, None]
        denominator[local : local + len(window)] += window * window
    zero = denominator == 0.0
    assert np.all(numerator[zero] == 0.0), "Nonzero numerator at a zero static denominator"
    result = np.zeros_like(numerator)
    np.divide(numerator, denominator[:, None], out=result, where=~zero[:, None])
    return result, origin


def _src_kernel(positions: Pcm, shift: float) -> tuple[Indices, Pcm]:
    taps = np.floor(positions).astype(np.int64)[:, None] + np.arange(
        -2 * SRC_RADIUS, 1, dtype=np.int64
    )
    distance = positions[:, None] - SRC_RADIUS - taps
    inside = np.abs(distance) <= SRC_RADIUS
    kaiser = np.i0(SRC_BETA * np.sqrt(np.maximum(0.0, 1.0 - (distance / SRC_RADIUS) ** 2)))
    kaiser /= np.i0(SRC_BETA)
    cutoff = min(1.0, 1.0 / shift)
    kernel = cutoff * np.sinc(cutoff * distance) * kaiser * inside
    total = np.sum(kernel, axis=1, keepdims=True)
    assert np.all(total != 0.0), "Zero static SRC coefficient sum"
    return taps, kernel / total


def _resample(stretched: Pcm, origin: int, shift: float) -> tuple[Pcm, int]:
    if shift == 1.0:
        return stretched.copy(), origin
    first = math.ceil(origin / shift)
    last = math.floor((origin + len(stretched) - 1 + 2 * SRC_RADIUS) / shift)
    result = np.empty((last - first + 1, 2), dtype=np.float64)
    for start in range(0, len(result), SRC_BLOCK_FRAMES):
        stop = min(start + SRC_BLOCK_FRAMES, len(result))
        positions = shift * np.arange(first + start, first + stop, dtype=np.float64)
        taps, kernel = _src_kernel(positions, shift)
        values = _read_zero_extended(stretched, taps, origin)
        result[start:stop] = np.sum(values * kernel[..., None], axis=1)
    return result, first


def _validate_inputs(source: Pcm, rate: float, semitones: int) -> tuple[Pcm, float, float]:
    source = np.asarray(source, dtype=np.float64)
    if source.ndim != 2 or source.shape[1] != 2 or not np.all(np.isfinite(source)):
        message = "Source must be finite Nx2 PCM"
        raise ValueError(message)
    if not math.isfinite(rate) or rate <= 0.0 or not isinstance(semitones, int):
        message = "Rate must be finite and positive; semitones must be an integer"
        raise ValueError(message)
    shift = 2.0 ** (semitones / 12.0)
    alpha = shift / rate
    if not math.isfinite(alpha) or not 0.0 < alpha <= 1.0:
        message = f"Unsupported stretch alpha={alpha}: frozen candidate requires 0 < alpha <= 1"
        raise ValueError(message)
    return source, shift, alpha


def _stretch(source: Pcm, alpha: float) -> tuple[Pcm, int, _Grid, _Statistics]:
    final_center = math.ceil((len(source) + HALF_WINDOW) / ANALYSIS_STEP) * ANALYSIS_STEP
    centers = np.arange(-HALF_WINDOW, final_center + 1, ANALYSIS_STEP, dtype=np.int64)
    synthesis_centers = np.floor(alpha * centers).astype(np.int64)
    grid = _Grid(centers, synthesis_centers, alpha, synthesis_centers - alpha * centers)
    coefficients = _analyze(source, centers, _WINDOW)
    corrected, statistics = _correct_phases(coefficients, grid)
    stretched, stretch_origin = _overlap_add(corrected, synthesis_centers, _WINDOW)
    return stretched, stretch_origin, grid, statistics


def _details(
    source_frames: int,
    rate: float,
    semitones: int,
    shift: float,
    grid: _Grid,
    statistics: _Statistics,
    stretch_support: tuple[int, int],
    capture_support: tuple[int, int],
) -> dict[str, object]:
    return {
        "model": "centered-phase-gradient-signed-offline-v1",
        "capture_origin": capture_support[0],
        "capture_end_exclusive": capture_support[1],
        "nominal_duration_frames": source_frames / rate,
        "source_frames": source_frames,
        "rate": rate,
        "semitones": semitones,
        "pitch_factor": shift,
        "alpha": grid.alpha,
        "supported_alpha": "0 < alpha <= 1; alpha > 1 explicitly unsupported",
        "window": "periodic Hann",
        "window_frames": WINDOW_FRAMES,
        "analysis_step_frames": ANALYSIS_STEP,
        "analysis_center_min": int(grid.analysis[0]),
        "analysis_center_max": int(grid.analysis[-1]),
        "analysis_support_min": int(grid.analysis[0]) - HALF_WINDOW,
        "analysis_support_max_exclusive": int(grid.analysis[-1]) + HALF_WINDOW,
        "packet_count": len(grid.analysis),
        "synthesis_center_min": int(grid.synthesis[0]),
        "synthesis_center_max": int(grid.synthesis[-1]),
        "synthesis_center_rounding": "floor(alpha * analysis_center)",
        "rounding_residual_min": float(np.min(grid.rounding)),
        "rounding_residual_max": float(np.max(grid.rounding)),
        "stretch_capture_origin": stretch_support[0],
        "stretch_capture_end_exclusive": stretch_support[1],
        "priority_tolerance": PRIORITY_TOLERANCE,
        "heap_exact_tie_order": "previous frame, current frame, lower bin",
        "stereo": "shared scalar correction; source per-bin IPD preserved",
        "src": "identity bypass" if shift == 1.0 else "causal normalized Kaiser-windowed sinc",
        "src_radius_input_frames": SRC_RADIUS,
        "src_kaiser_beta": SRC_BETA,
        "src_cutoff": min(1.0, 1.0 / shift),
        "src_delay_output_frames": 0.0 if shift == 1.0 else SRC_RADIUS / shift,
        "src_delay_compensated": False,
        "static_dual": "sum of Hann squared over every admitted synthesis packet",
        "complete_signed_packets_and_src_tails": True,
        "native_device_live_or_listening_acceptance": False,
        "counts": asdict(statistics),
    }


def render(source: Pcm, rate: float, semitones: int) -> tuple[Pcm, dict[str, object]]:
    """Render complete signed offline PCM and its predetermined capture coordinates.

    Args:
        source: Finite original stereo PCM with shape Nx2.
        rate: Positive original-source frames per nominal output frame.
        semitones: Integer pitch shift; h = 2 ** (semitones / 12).

    Returns:
        Full signed capture and metadata. ``capture_origin`` is its absolute output
        frame coordinate; the nominal duration never crops this capture.
    """
    source, shift, alpha = _validate_inputs(source, rate, semitones)
    stretched, stretch_origin, grid, statistics = _stretch(source, alpha)
    output, capture_origin = _resample(stretched, stretch_origin, shift)
    details = _details(
        len(source),
        rate,
        semitones,
        shift,
        grid,
        statistics,
        (stretch_origin, stretch_origin + len(stretched)),
        (capture_origin, capture_origin + len(output)),
    )
    return output, details


def _gradient_control_case(kind: str) -> tuple[_Gradients, Pcm, Indices]:
    centers = np.array([-ANALYSIS_STEP, 0, ANALYSIS_STEP], dtype=np.int64)
    coefficients = np.zeros((3, HALF_WINDOW + 1, 2), dtype=np.complex128)
    if kind == "tone":
        frequency = _OMEGA[37]
        phase = frequency * centers + 0.37
        components = ((-1, WINDOW_FRAMES / 8), (0, WINDOW_FRAMES / 4), (1, WINDOW_FRAMES / 8))
        for offset, magnitude in components:
            coefficients[:, 37 + offset, 0] = magnitude * np.exp(1j * phase)
    else:
        local = 100.0 - centers
        phase = -local[:, None] * _OMEGA
        magnitude = 0.5 + 0.5 * np.cos(2.0 * np.pi * local / WINDOW_FRAMES)
        coefficients[..., 0] = magnitude[:, None] * np.exp(1j * phase)
    coefficients[..., 1] = 0.5 * coefficients[..., 0]
    return _gradients(coefficients, centers, _Statistics()), phase, centers


def _closure_control(kind: str, gradients: _Gradients, phase: Pcm, centers: Indices) -> float:
    alpha = 0.67
    synthesis_centers = np.floor(alpha * centers).astype(np.int64)
    rounding = synthesis_centers - alpha * centers
    if kind == "tone":
        target = alpha * phase[:, None] + rounding[:, None] * _OMEGA
        bins = slice(36, 39)
        frequency_bins = slice(36, 38)
    else:
        target = alpha * phase + rounding[:, None] * _OMEGA
        bins = slice(None)
        frequency_bins = slice(None)
    desired_time = np.stack([
        _desired_time(
            gradients.time[i - 1],
            gradients.time[i],
            ANALYSIS_STEP,
            float(rounding[i] - rounding[i - 1]),
            alpha,
        )
        for i in (1, 2)
    ])
    vertical = alpha * gradients.frequency + _BIN_STEP * rounding[:, None]
    desired_frequency = 0.5 * (vertical[:, :-1] + vertical[:, 1:])
    return _maximum_closure_error(target, desired_time, desired_frequency, bins, frequency_bins)


def _maximum_closure_error(
    target: Pcm, desired_time: Pcm, desired_frequency: Pcm, bins: slice, frequency_bins: slice
) -> float:
    time_error = _principal((np.diff(target, axis=0) - desired_time)[:, bins])
    frequency_error = _principal((np.diff(target, axis=1) - desired_frequency)[:, frequency_bins])
    return max(float(np.max(np.abs(time_error))), float(np.max(np.abs(frequency_error))))


def _dual_control() -> dict[str, float]:
    length = 32
    source = np.zeros((19, 2), dtype=np.float64)
    source[0] = (1.0, 0.5)
    source[-1] = (-0.25, -0.125)
    source[8] = (0.125, 0.0625)
    window = 0.5 - 0.5 * np.cos(2.0 * np.pi * np.arange(length) / length)
    centers = np.arange(-length // 2, 41, 8, dtype=np.int64)
    reconstructed, origin = _overlap_add(_analyze(source, centers, window), centers, window)
    expected = _read_zero_extended(
        source, np.arange(origin, origin + len(reconstructed), dtype=np.int64)
    )
    error = float(np.max(np.abs(reconstructed - expected)))
    first_error = float(np.max(np.abs(reconstructed[-origin] - source[0])))
    last_error = float(np.max(np.abs(reconstructed[len(source) - 1 - origin] - source[-1])))
    assert max(error, first_error, last_error) < 1.0e-12, "Static dual identity control failed"
    return {"max_error": error, "first_sample_error": first_error, "last_sample_error": last_error}


def _sign_change_endpoint_control() -> float:
    """Drive a real DC sign change through current and subsequent heap frames."""
    centers = np.array([0, ANALYSIS_STEP, 2 * ANALYSIS_STEP], dtype=np.int64)
    coefficients = np.zeros((3, HALF_WINDOW + 1, 2), dtype=np.complex128)
    coefficients[:, 0, 0] = [2.0, -2.0, -2.0]
    coefficients[:, 1, 0] = 1.0
    coefficients[..., 1] = 0.5 * coefficients[..., 0]
    statistics = _Statistics()
    gradients = _gradients(coefficients, centers, statistics)
    grid = _Grid(centers, centers // 2, 0.5, np.zeros(3))
    priority = np.sqrt(np.sum(np.abs(coefficients) ** 2, axis=-1))
    previous = _integrate_frame(0, np.zeros(HALF_WINDOW + 1), priority, gradients, grid, statistics)
    current = _integrate_frame(1, previous, priority, gradients, grid, statistics)
    following = _integrate_frame(2, current, priority, gradients, grid, statistics)
    desired = 0.25 * (gradients.frequency[1, 0] + gradients.frequency[1, 1])
    emitted_endpoint_phase = np.pi
    edge_error = (
        current[1] - emitted_endpoint_phase - desired + gradients.frequency_connections[1, 0]
    )
    return float(
        np.max(
            np.abs(
                _principal(
                    np.array([
                        current[0] - emitted_endpoint_phase,
                        following[0] - emitted_endpoint_phase,
                        edge_error,
                    ])
                )
            )
        )
    )


def _endpoint_control() -> dict[str, float]:
    """Check emitted real endpoint phase against propagated neighbor state."""
    errors = []
    for index in (0, 2):
        state = _FrameState(
            np.zeros(3),
            np.ones(3, dtype=np.bool_),
            3,
            [],
            np.ones(3),
            np.zeros(3),
            np.zeros(2),
            np.ones(2, dtype=np.bool_),
        )
        _assign(state, index, 0.6 * np.pi)
        _spread_frequency(state, index, _Statistics())
        emitted_real_phase = np.pi  # cos(0.6*pi)<0; emitted common multiplier is -1.
        error = float(np.max(np.abs(_principal(state.correction[[index, 1]] - emitted_real_phase))))
        errors.append(error)
    maximum = max(errors)
    sign_change = _sign_change_endpoint_control()
    assert max(maximum, sign_change) < 1.0e-12, (
        "Endpoint emitted phase differs from propagated heap state"
    )
    return {
        "endpoint_and_neighbor_phase_error": maximum,
        "sign_change_current_next_and_neighbor_phase_error": sign_change,
    }


def gradient_controls() -> dict[str, object]:
    """Assert independent source-defined algebra controls, without quality acceptance.

    Returns:
        Tone/impulse derivative and rounded edge closure errors, plus a small
        unchanged-coefficient static-dual first/last-sample reconstruction control.
    """
    tone, tone_phase, centers = _gradient_control_case("tone")
    impulse, impulse_phase, _ = _gradient_control_case("impulse")
    tone_time = float(np.max(np.abs(tone.time[:, 36:39] - _OMEGA[37])))
    tone_frequency = float(np.max(np.abs(tone.frequency[:, 36:39])))
    impulse_time = float(np.max(np.abs(impulse.time - _OMEGA)))
    expected_frequency = -_BIN_STEP * (100.0 - centers[:, None])
    impulse_frequency = float(np.max(np.abs(impulse.frequency - expected_frequency)))
    tone_closure = _closure_control("tone", tone, tone_phase, centers)
    impulse_closure = _closure_control("impulse", impulse, impulse_phase, centers)
    errors = {
        "tone_time_error": tone_time,
        "tone_frequency_error": tone_frequency,
        "impulse_time_error": impulse_time,
        "impulse_frequency_error": impulse_frequency,
        "tone_rounded_edge_closure_error": tone_closure,
        "impulse_rounded_edge_closure_error": impulse_closure,
    }
    assert max(errors.values()) < 1.0e-11, "Source-defined phase-gradient algebra control failed"
    return {
        "scope": "algebra controls only; no sound-quality acceptance",
        **errors,
        "static_dual": _dual_control(),
        "endpoint_projection": _endpoint_control(),
    }
