"""Frozen offline complete finite-bank AM/FM falsification model.

The complete source decomposition and phase rule below are a custom derivation,
not a claim that a published method solves arbitrary mixtures. Source-only cues
split PCM; every sample, every band and every causal convolution tail participates.
There is no dry residual, attack bridge, gain fit, delay fit or nominal-end crop.
"""

import math
from dataclasses import asdict, dataclass
from itertools import pairwise
from typing import TYPE_CHECKING

import numpy as np
from pitch_overlap_model import source_cues

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Pcm = NDArray[np.float64]
type ComplexPcm = NDArray[np.complex128]

FS = 48_000
BANK_FRAMES = 1024
BAND_COUNT = BANK_FRAMES // 2 + 1
_OFFSETS = np.arange(BANK_FRAMES, dtype=np.float64)
_WINDOW = 0.5 + 0.5 * np.cos(np.pi * _OFFSETS / (BANK_FRAMES - 1))
_WINDOW[-1] = 0.0
_OMEGA = 2.0 * np.pi * np.arange(BAND_COUNT) / BANK_FRAMES
_BAND_WEIGHTS = np.full(BAND_COUNT, 2.0)
_BAND_WEIGHTS[[0, -1]] = 1.0


@dataclass(slots=True)
class _Statistics:
    processed_segments: int = 0
    processed_bands: int = 0
    phase_runs: int = 0
    exact_common_zero_samples: int = 0
    pooled_cancellation_fallbacks: int = 0
    invalid_nonzero_phase_edges: int = 0
    carrier_unwrap_edges: int = 0
    shifted_phase_edges_beyond_nyquist: int = 0
    maximum_working_fft_frames: int = 0
    maximum_complete_bank_error: float = 0.0
    complete_bank_error_energy: float = 0.0
    complete_bank_source_energy: float = 0.0
    minimum_source_phase_advance: float | None = None
    maximum_source_phase_advance: float | None = None


@dataclass(slots=True)
class _Capture:
    output: Pcm
    origin: int
    nominal: int
    retired: int
    segments: list[dict]
    statistics: _Statistics


def _principal(angles: Pcm) -> Pcm:
    """Use [-pi, pi), including a fixed negative-pi tie convention."""
    return (angles + np.pi) % (2.0 * np.pi) - np.pi


def _kernel(band: int) -> NDArray[np.complex128]:
    """Return one explicitly modulated finite FIR, including real endpoints."""
    kernel = _BAND_WEIGHTS[band] * _WINDOW * np.exp(1j * _OMEGA[band] * _OFFSETS)
    if band in {0, BAND_COUNT - 1}:
        kernel = kernel.real.astype(np.complex128)
    return kernel / BANK_FRAMES


def _prepare_fft(source: Pcm) -> tuple[ComplexPcm, NDArray[np.complex128], NDArray[np.bool_]]:
    """Prepare one shared source FFT and one prototype FFT for all bands."""
    count = len(source) + BANK_FRAMES - 1
    fft_frames = 1 << (max(BANK_FRAMES, count) - 1).bit_length()
    positions = np.arange(count)
    prefix = np.concatenate((np.zeros(1, dtype=np.int64), np.cumsum(np.any(source != 0, axis=1))))
    upper = np.minimum(positions + 1, len(source))
    lower = np.clip(positions - (BANK_FRAMES - 2), 0, len(source))
    supported = prefix[upper] != prefix[lower]
    return (
        np.fft.fft(source, n=fft_frames, axis=0),
        np.fft.fft(_WINDOW / BANK_FRAMES, n=fft_frames),
        supported,
    )


def _band_signal(
    source: Pcm,
    source_fft: ComplexPcm,
    prototype_fft: NDArray[np.complex128],
    supported: NDArray[np.bool_],
    band: int,
) -> ComplexPcm:
    """Convolve completely; impose only algebraically known support/endpoints."""
    stride = len(prototype_fft) // BANK_FRAMES
    response = _BAND_WEIGHTS[band] * np.roll(prototype_fft, band * stride)
    values = np.fft.ifft(source_fft * response[:, None], axis=0)[: len(supported)]
    if band in {0, BAND_COUNT - 1}:
        values.imag = 0.0
    # The first tap is exactly d_k/N and the last FIR tap is exactly zero.
    # These are structural identities, not amplitude-dependent gates.
    values[0] = _BAND_WEIGHTS[band] * source[0] / BANK_FRAMES
    values[~supported] = 0.0
    return values


def _phase_edges(
    values: ComplexPcm, omega: float, statistics: _Statistics
) -> tuple[Pcm, NDArray[np.bool_], NDArray[np.bool_]]:
    """Resolve pooled adjacent phase edges and the frozen exact-zero fallback."""
    active = np.any(values != 0.0, axis=1)
    products = np.conj(values[:-1]) * values[1:]
    pooled = np.sum(products, axis=1)
    selected_channel = np.argmax(np.abs(products), axis=1)
    selected = np.take_along_axis(products, selected_channel[:, None], axis=1)[:, 0]
    fallback = (pooled == 0.0) & (selected != 0.0)
    pooled[fallback] = selected[fallback]
    connected = active[:-1] & active[1:]
    valid = connected & (pooled != 0.0)
    edges = np.full(len(products), omega)
    edges[valid] = omega + _principal(np.angle(pooled[valid]) - omega)
    statistics.pooled_cancellation_fallbacks += int(np.count_nonzero(fallback))
    statistics.invalid_nonzero_phase_edges += int(np.count_nonzero(connected & ~valid))
    statistics.carrier_unwrap_edges += int(np.count_nonzero(connected))
    return edges, connected, active


def _record_phase_advances(used: Pcm, shift: float, statistics: _Statistics) -> None:
    """Record unmodified source advances and shifted Nyquist crossings."""
    if len(used):
        low, high = float(np.min(used)), float(np.max(used))
        statistics.minimum_source_phase_advance = (
            low
            if statistics.minimum_source_phase_advance is None
            else min(statistics.minimum_source_phase_advance, low)
        )
        statistics.maximum_source_phase_advance = (
            high
            if statistics.maximum_source_phase_advance is None
            else max(statistics.maximum_source_phase_advance, high)
        )
        statistics.shifted_phase_edges_beyond_nyquist += int(
            np.count_nonzero(np.abs(shift * used) > np.pi)
        )


def _phase_elapsed(values: ComplexPcm, omega: float, shift: float, statistics: _Statistics) -> Pcm:
    """Integrate pooled stereo phase, restarting only at exact common zeros.

    A run's absolute birth phase is the dominant birth channel's argument, with
    left-channel ties. It cancels in phi-phi_birth, which is returned here. Each
    stereo channel keeps its original complex coefficient and receives the same
    scalar correction. Therefore proportional, antiphase and silent-channel
    relations are preserved without multiplying an interchannel phase offset.
    """
    edges, connected, active = _phase_edges(values, omega, statistics)
    starts = active & ~np.concatenate((np.zeros(1, dtype=np.bool_), active[:-1]))
    advances = np.zeros(len(values), dtype=np.float64)
    advances[1:] = np.where(connected, edges, 0.0)
    cumulative = np.cumsum(advances)
    birth_indices = np.maximum.accumulate(np.where(starts, np.arange(len(values)), 0))
    elapsed = cumulative - cumulative[birth_indices]
    elapsed[~active] = 0.0
    statistics.phase_runs += int(np.count_nonzero(starts))
    statistics.exact_common_zero_samples += int(np.count_nonzero(~active))
    _record_phase_advances(edges[connected], shift, statistics)
    return elapsed


def _linear_sample(values: Pcm | ComplexPcm, positions: Pcm) -> Pcm | ComplexPcm:
    """Interpolate declared sample coordinates, with exact support zero extension."""
    result = np.zeros((len(positions), *values.shape[1:]), dtype=values.dtype)
    inside = (positions > -1.0) & (positions < len(values))
    coordinates = positions[inside]
    lower = np.floor(coordinates).astype(np.int64)
    upper = lower + 1
    fraction = coordinates - lower
    lower_valid = lower >= 0
    upper_valid = upper < len(values)
    if values.ndim == 2:
        fraction = fraction[:, None]
        lower_valid = lower_valid[:, None]
        upper_valid = upper_valid[:, None]
    result[inside] = (1.0 - fraction) * values[
        np.clip(lower, 0, len(values) - 1)
    ] * lower_valid + fraction * values[np.clip(upper, 0, len(values) - 1)] * upper_valid
    return result


def _render_band(
    values: ComplexPcm,
    positions: Pcm,
    omega: float,
    rate: float,
    shift: float,
    statistics: _Statistics,
) -> Pcm:
    """Transform full instantaneous phase at integer source coordinates."""
    elapsed = _phase_elapsed(values, omega, shift, statistics)
    sampled = _linear_sample(values, positions)
    sampled_elapsed = _linear_sample(elapsed, positions)
    correction = np.exp(1j * (shift / rate - 1.0) * sampled_elapsed)
    return np.real(sampled * correction[:, None])


def _render_segment(
    source: Pcm, birth: int, rate: float, shift: float, statistics: _Statistics
) -> tuple[Pcm, int, dict]:
    """Render every complete band sequentially, retaining the global source clock."""
    source_fft, prototype_fft, supported = _prepare_fft(source)
    count = len(supported)
    start = math.floor((birth - 1) / rate) + 1
    stop = math.floor((birth + count - 1) / rate) + 1
    positions = rate * np.arange(start, stop, dtype=np.float64) - birth
    output = np.zeros((len(positions), 2), dtype=np.float64)
    reconstruction = np.zeros((count, 2), dtype=np.float64)
    statistics.maximum_working_fft_frames = max(
        statistics.maximum_working_fft_frames, len(prototype_fft)
    )
    for band, omega in enumerate(_OMEGA):
        values = _band_signal(source, source_fft, prototype_fft, supported, band)
        reconstruction += values.real
        output += _render_band(values, positions, float(omega), rate, shift, statistics)
        statistics.processed_bands += 1
    reconstruction[: len(source)] -= source
    error = float(np.max(np.abs(reconstruction)))
    statistics.maximum_complete_bank_error = max(statistics.maximum_complete_bank_error, error)
    statistics.complete_bank_error_energy += float(np.sum(reconstruction * reconstruction))
    statistics.complete_bank_source_energy += float(np.sum(source * source))
    statistics.processed_segments += 1
    return (
        output,
        start,
        {
            "birth_source_frame": birth,
            "nonzero_input_span_frames": len(source),
            "complete_convolution_frames": count,
            "output_begin": start,
            "output_end_exclusive": stop,
            "first_sample_source_coordinate": float(positions[0]) if len(positions) else None,
            "fft_frames": len(prototype_fft),
            "bands_processed": BAND_COUNT,
            "complete_bank_max_abs_error_before_warp": error,
        },
    )


def _validate(source: Pcm, rate: float, shift: float) -> Pcm:
    pcm = np.asarray(source, dtype=np.float64)
    if pcm.ndim != 2 or pcm.shape[1] != 2 or not np.all(np.isfinite(pcm)):
        message = "Source must be finite stereo PCM with shape (frames, 2)"
        raise ValueError(message)
    if not math.isfinite(rate) or rate <= 0.0 or not math.isfinite(shift) or shift <= 0.0:
        message = "Rate and pitch ratio must be finite and strictly positive"
        raise ValueError(message)
    return pcm


def _new_capture(pcm: Pcm, rate: float) -> _Capture:
    """Allocate the complete declared signed support before any segment is summed."""
    nominal = math.ceil(len(pcm) / rate)
    capture_origin = min(0, math.floor(-1.0 / rate) + 1) if len(pcm) else 0
    allocation = math.floor((len(pcm) + BANK_FRAMES - 2) / rate) + 1 if len(pcm) else 0
    output = np.zeros((max(nominal, allocation) - capture_origin, 2), dtype=np.float64)
    return _Capture(output, capture_origin, nominal, nominal, [], _Statistics())


def _source_segment(
    pcm: Pcm, begin: int, end: int, rate: float, shift: float, statistics: _Statistics
) -> tuple[Pcm | None, int, dict]:
    """Trim only exact input zeros while retaining original source coordinates."""
    segment = pcm[begin:end]
    nonzero = np.flatnonzero(np.any(segment != 0.0, axis=1))
    record = {"source_begin": begin, "source_end": end}
    if not len(nonzero):
        return None, 0, {**record, "exactly_silent": True, "bands_processed": 0}
    first, last = int(nonzero[0]), int(nonzero[-1])
    stream, start, active = _render_segment(
        segment[first : last + 1], begin + first, rate, shift, statistics
    )
    return (
        stream,
        start,
        {
            **record,
            "exactly_silent": False,
            "trimmed_leading_input_zeros": first,
            "trimmed_trailing_input_zeros": len(segment) - last - 1,
            **active,
        },
    )


def _capture_segments(pcm: Pcm, rate: float, shift: float, cues: list[int]) -> _Capture:
    """Sum every complete segment stream into its fixed global capture."""
    capture = _new_capture(pcm, rate)
    for begin, end in pairwise(cues):
        stream, start, record = _source_segment(pcm, begin, end, rate, shift, capture.statistics)
        capture.segments.append(record)
        if stream is None:
            continue
        stop = start + len(stream)
        assert stop - capture.origin <= len(capture.output), "A full tail exceeds allocation"
        assert start >= capture.origin, "A fractional interpolation preview exceeds capture"
        capture.output[start - capture.origin : stop - capture.origin] += stream
        capture.retired = max(capture.retired, stop)
    assert np.all(np.isfinite(capture.output)), "Nonfinite complete generated output"
    return capture


def _render_details(
    pcm: Pcm, rate: float, semitones: int, shift: float, cues: list[int], capture: _Capture
) -> dict:
    """Describe the frozen mechanism and aggregate its existing observations."""
    return {
        "model": "source-segmented-complete-causal-amfm-v1",
        "sample_rate_hz": FS,
        "source_frames": len(pcm),
        "rate": rate,
        "semitones": semitones,
        "h": shift,
        "phase_multiplier": shift / rate,
        "filter_frames": BANK_FRAMES,
        "filter_last_tap_exact_zero": True,
        "maximum_filter_tail_source_frames": BANK_FRAMES - 1,
        "band_count": BAND_COUNT,
        "bin_spacing_hz": FS / BANK_FRAMES,
        "filter": "d_k/N*(.5+.5*cos(pi*l/(N-1)))*exp(i*2*pi*k*l/N); l=0..N-1",
        "endpoint_weights": "DC/Nyquist 1; every interior positive-frequency band 2",
        "reconstruction_identity": "Re(sum_k g_k[l])=delta[l]; w[0]=1, support 0..N-1",
        "source_decomposition": "disjoint source cue segments; every segment has every FIR band",
        "cue_detector": "unchanged source_cues: 8*past512 first-difference norm; 1e-12; gap240",
        "cue_indices": cues,
        "segments": capture.segments,
        "segment_count": len(capture.segments),
        "nominal_output_frames": capture.nominal,
        "full_generated_output_frames": capture.retired - capture.origin,
        "capture_origin": capture.origin,
        "capture_origin_output_frame": capture.origin,
        "capture_allocation_limit_frames": len(capture.output),
        "global_coordinate": "s=r*n-birth; full -1<s<L linear interpolation support is captured",
        "phase_rule": "z_c(s)*exp(i*(h/r-1)*(phi_ref(s)-phi_ref_birth))",
        "unwrap": "omega_k+principal(arg(sum_c conj(z_c[j-1])*z_c[j])-omega_k)",
        "pooled_zero_fallback": "largest-magnitude channel product, left tie; then carrier omega_k",
        "birth_rule": "first exact common-nonzero sample after exact common zero; no threshold",
        "birth_gauge": "argument of dominant birth channel, left tie; cancels in phase difference",
        "sampler": "complex linear coefficient plus linear unwrapped-reference-phase interpolation",
        "integer_phase_rule_exact": True,
        "fractional_phase_rule_exact": False,
        "fractional_rate_only_identity": "h=r gives linear interpolation of full original source",
        "tail_policy": (
            "all causal convolution tails; full nominal duration even after input zero trim"
        ),
        "numerical_support": (
            "exact source-activity prefix mask for FIR l=0..N-2, including internal zero gaps"
        ),
        "normalization": "none; no dry residual, fitted gain/delay or output crop",
        "offline_full_source_lookahead": True,
        "native_used": False,
        "production_activation": False,
        "realtime_causality_claim": False,
        "overall_acoustic_acceptance": False,
        "risks": [
            "causal prototype has negative-image leakage and broad asymmetric responses",
            "nonlinear band phase warp can destroy exact cross-band transient cancellation",
            "source cue heuristics can miss events or chop sustained components",
            "a full bank is not a decomposition into uniquely identifiable physical modes",
            "DC/Nyquist real-band phase and mixed-band phase can be ambiguous",
            "interior FFT roundoff may perturb exact-zero and near-zero phase decisions",
            "fractional complex/phase interpolation approximates the full AM/FM formula",
            "fractional interpolation has at most one source frame of preview; signed capture kept",
            "no anti-alias filter, band rejection or shifted-frequency clamp is applied",
            "all retained segment tails may overlap and alter the background",
        ],
        **asdict(capture.statistics),
    }


def render(source: Pcm, rate: float, semitones: int) -> tuple[Pcm, dict]:
    """Render source PCM using frozen source-only segmentation and the full bank.

    Args:
        source: Original finite stereo PCM at the fixed 48000-Hz diagnostic rate.
        rate: Original source frames per output frame, independent of pitch.
        semitones: Pitch ratio intent h=2**(semitones/12).

    Returns:
        Complete finite generated PCM and JSON-compatible mechanism details.
    """
    shift = 2.0 ** (semitones / 12.0)
    pcm = _validate(source, rate, shift)
    cues = source_cues(pcm)
    capture = _capture_segments(pcm, rate, shift, cues)
    return (
        capture.output[: capture.retired - capture.origin],
        _render_details(pcm, rate, semitones, shift, cues, capture),
    )


def _bank_controls() -> dict:
    """Compare explicit FIR sums and direct convolution with the FFT bank."""
    source = np.array([[0.3, -0.2], [-0.1, 0.4], [0.0, 0.0], [0.7, -0.5]])
    source_fft, prototype_fft, supported = _prepare_fft(source)
    explicit_kernel_sum = np.zeros(BANK_FRAMES)
    direct_sum = np.zeros((len(supported), 2))
    fft_sum = np.zeros_like(direct_sum)
    maximum_fft_direct_error = 0.0
    for band in range(BAND_COUNT):
        # Explicit FIR/direct convolution does not use FFT modulation shifts.
        kernel = _kernel(band)
        explicit_kernel_sum += kernel.real
        direct = np.column_stack([np.convolve(source[:, channel], kernel) for channel in range(2)])
        transformed = _band_signal(source, source_fft, prototype_fft, supported, band)
        direct_sum += direct.real
        fft_sum += transformed.real
        maximum_fft_direct_error = max(
            maximum_fft_direct_error, float(np.max(np.abs(transformed - direct)))
        )
    expected_kernel = np.zeros(BANK_FRAMES)
    expected_kernel[0] = 1.0
    expected_source = np.zeros_like(direct_sum)
    expected_source[: len(source)] = source
    return {
        "explicit_sum_delta_max_abs_error": float(
            np.max(np.abs(explicit_kernel_sum - expected_kernel))
        ),
        "independent_direct_complete_bank_max_abs_error": float(
            np.max(np.abs(direct_sum - expected_source))
        ),
        "fft_complete_bank_max_abs_error": float(np.max(np.abs(fft_sum - expected_source))),
        "fft_vs_direct_each_band_max_abs_error": maximum_fft_direct_error,
    }


def _boundary_controls() -> dict:
    """Retain both first and last source pulses and the complete generated tail."""
    pulse_source = np.zeros((19, 2))
    pulse_source[0] = (0.5, -0.25)
    pulse_source[-1] = (-0.7, 0.35)
    boundary, boundary_details = render(pulse_source, 1.0, 0)
    boundary_expected = np.zeros_like(boundary)
    boundary_expected[: len(pulse_source)] = pulse_source
    return {
        "first_and_last_source_boundary_pulse_max_abs_error": float(
            np.max(np.abs(boundary - boundary_expected))
        ),
        "boundary_full_generated_frames": len(boundary),
        "boundary_nominal_frames": boundary_details["nominal_output_frames"],
    }


def _stereo_control_errors(mono: NDArray[np.complex128], positions: Pcm, omega: float) -> list:
    """Measure positive, negative and silent channel relations after correction."""
    errors = []
    for proportion in (0.6, -0.4, 0.0):
        relation = np.column_stack((mono, proportion * mono))
        rendered = _render_band(relation, positions, omega, 2.0, 1.5, _Statistics())
        errors.append(float(np.max(np.abs(rendered[:, 1] - proportion * rendered[:, 0]))))
    return errors


def _phase_controls() -> dict:
    """Check a known complex phase slope with a nonunity AM/FM multiplier."""
    omega = 0.37
    true_advance = 0.43
    positions = np.arange(23, dtype=np.float64)
    initial = 0.61
    mono = np.exp(1j * (initial + true_advance * positions))
    analytic = np.column_stack((mono, -0.4 * mono))
    corrected = _render_band(analytic, positions, omega, 2.0, 1.5, _Statistics())
    known = np.column_stack((
        np.cos(initial + 0.75 * true_advance * positions),
        -0.4 * np.cos(initial + 0.75 * true_advance * positions),
    ))
    return {
        "known_complex_nonunity_phase_rule_max_abs_error": float(np.max(np.abs(corrected - known))),
        "positive_negative_silent_stereo_max_abs_errors": _stereo_control_errors(
            mono, positions, omega
        ),
    }


def _fractional_profile_control(source: Pcm, semitones: int) -> tuple[float, int]:
    """Compare rate-only output to independent ordinary source interpolation."""
    rate = 2.0 ** (semitones / 12.0)
    fractional, details = render(source, rate, semitones)
    origin = details["capture_origin_output_frame"]
    clock = rate * (origin + np.arange(len(fractional), dtype=np.float64))
    knots = np.arange(-1, len(source) + 1)
    padded = np.vstack((np.zeros((1, 2)), source, np.zeros((1, 2))))
    expected = np.column_stack([
        np.interp(clock, knots, padded[:, channel], left=0.0, right=0.0) for channel in range(2)
    ])
    return float(np.max(np.abs(fractional - expected))), origin


def _fractional_controls() -> dict:
    """Check slower/faster fractional source clocks, including signed preview."""
    fractional_source = np.array([[0.2, -0.1], [-0.3, 0.15], [0.7, -0.35], [0.0, 0.0], [0.4, -0.2]])
    fractional_errors = []
    fractional_origins = []
    for semitones in (-12, 7):
        error, origin = _fractional_profile_control(fractional_source, semitones)
        fractional_errors.append(error)
        fractional_origins.append(origin)
    return {
        "fractional_rate_only_linear_source_max_abs_errors": fractional_errors,
        "fractional_rate_only_capture_origins": fractional_origins,
    }


def _gap_controls() -> dict:
    """Check structural zeros and separate births across a long internal gap."""
    gap_source = np.zeros((2 * BANK_FRAMES + 9, 2))
    gap_source[0] = (0.3, -0.15)
    gap_source[-1] = (-0.4, 0.2)
    gap_fft, gap_prototype, gap_supported = _prepare_fft(gap_source)
    gap_values = _band_signal(gap_source, gap_fft, gap_prototype, gap_supported, 117)
    gap_statistics = _Statistics()
    _phase_elapsed(gap_values, float(_OMEGA[117]), 1.5, gap_statistics)
    return {
        "long_gap_structural_zero_frames": int(np.count_nonzero(~gap_supported)),
        "long_gap_false_nonzero_values": int(np.count_nonzero(gap_values[~gap_supported])),
        "long_gap_phase_runs": gap_statistics.phase_runs,
    }


def integrity_controls() -> dict:
    """Check independent algebra, boundaries, stereo and nontrivial phase rules.

    These synthetic controls establish implementation identities only. They do
    not supply renderer parameters, fixture targets or acoustic acceptance.
    """
    measurements = {
        **_bank_controls(),
        **_boundary_controls(),
        **_phase_controls(),
        **_fractional_controls(),
        **_gap_controls(),
    }
    errors = [
        measurements[key]
        for key in (
            "explicit_sum_delta_max_abs_error",
            "independent_direct_complete_bank_max_abs_error",
            "fft_complete_bank_max_abs_error",
            "fft_vs_direct_each_band_max_abs_error",
            "first_and_last_source_boundary_pulse_max_abs_error",
            "known_complex_nonunity_phase_rule_max_abs_error",
        )
    ]
    errors.extend(measurements["fractional_rate_only_linear_source_max_abs_errors"])
    errors.extend(measurements["positive_negative_silent_stereo_max_abs_errors"])
    tolerance = 2.0e-11
    passed = (
        all(error < tolerance for error in errors)
        and measurements["long_gap_structural_zero_frames"] > 0
        and measurements["long_gap_false_nonzero_values"] == 0
        and measurements["long_gap_phase_runs"] == 2
    )
    assert passed, "Complete-bank implementation integrity control failed"
    return {
        "stage": "amfm_integrity_controls",
        "passed": passed,
        "absolute_tolerance": tolerance,
        **measurements,
        "acoustic_acceptance": False,
    }
