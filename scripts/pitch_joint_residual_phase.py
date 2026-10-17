"""Frozen offline joint phase experiment on the complete finite complex bank.

The complete bank and every source segment are summed before phase extraction.
Its exact real sum is the original source; its imaginary sum is one fixed finite
quadrature FIR. This separate experiment does not replace production rendering
or establish acoustic, Native, device, listening, or realtime acceptance.
"""

import math
from dataclasses import asdict, dataclass
from types import MappingProxyType
from typing import TYPE_CHECKING

import numpy as np
import pitch_amfm_model as bank
from pitch_mode_envelopes import MAX_MEMORY_BYTES

if TYPE_CHECKING:
    from collections.abc import Mapping
    from typing import Never

    from numpy.typing import NDArray

type Pcm = NDArray[np.float64]
type ComplexPcm = NDArray[np.complex128]

BANK_FRAMES = bank.BANK_FRAMES
_MEMORY_RESERVE_BYTES = 1024 * 1024
_VALIDATION_CHUNK_FRAMES = 4096
# A separate length guard bounds direct-convolution work even when preparation
# alone would fit. It is derived from the declared byte limit, not source content.
MAX_SOURCE_FRAMES = (MAX_MEMORY_BYTES - _MEMORY_RESERVE_BYTES) // 272
_ODD_OFFSETS = np.arange(1, BANK_FRAMES - 1, 2, dtype=np.float64)
_ODD_KERNEL = 2.0 * bank._WINDOW[1:-1:2] / BANK_FRAMES / np.tan(np.pi * _ODD_OFFSETS / BANK_FRAMES)
_ODD_KERNEL.setflags(write=False)


class JointResidualPhaseError(ValueError):
    """Refuse an unsupported resource or numerical range with explicit details."""

    def __init__(self, reason: str, report: dict) -> None:
        self.report = {**report, "status": "refused", "refusal_reason": reason}
        super().__init__(f"Joint residual phase experiment refused: {reason}")


@dataclass(frozen=True, slots=True)
class Prepared:
    """Retain immutable source PCM and its complete source-clock complex field.

    Both arrays use immutable bytes backing, so changing the caller's array or
    requesting writable flags cannot change prepared samples. The report is a
    read-only mapping; render reports are fresh JSON-compatible dictionaries.
    """

    source: Pcm
    analytic_pcm: ComplexPcm
    report: Mapping[str, object]


def _refuse(reason: str, report: dict) -> Never:
    raise JointResidualPhaseError(reason, report)


def _all_finite(values: Pcm | ComplexPcm) -> bool:
    return all(
        bool(np.isfinite(values[start : start + _VALIDATION_CHUNK_FRAMES]).all())
        for start in range(0, len(values), _VALIDATION_CHUNK_FRAMES)
    )


def _immutable_copy(values: Pcm | ComplexPcm) -> Pcm | ComplexPcm:
    """Copy into immutable bytes rather than a reversible writable array flag."""
    return np.frombuffer(values.tobytes(order="C"), dtype=values.dtype).reshape(values.shape)


def _source_shape(source: Pcm) -> tuple[int, int]:
    message = "Source must be a finite real numeric stereo array with shape (frames, 2)"
    if not isinstance(source, np.ndarray):
        raise TypeError(message)
    if (
        np.ma.isMaskedArray(source)
        or source.ndim != 2
        or source.shape[1] != 2
        or not np.issubdtype(source.dtype, np.number)
        or np.issubdtype(source.dtype, np.complexfloating)
    ):
        raise ValueError(message)
    return len(source), int(source.nbytes)


def _preparation_report(frames: int, input_bytes: int) -> dict:
    field_frames = frames + BANK_FRAMES - 1 if frames else 0
    estimate = input_bytes + 64 * frames + 96 * field_frames + _MEMORY_RESERVE_BYTES
    return {
        "model": "complete-bank-joint-residual-phase-offline-v1",
        "status": "preparing",
        "refusal_reason": None,
        "source_frames": frames,
        "complete_complex_field_frames": field_frames,
        "sample_rate_hz": bank.FS,
        "filter_frames": BANK_FRAMES,
        "band_count": bank.BAND_COUNT,
        "maximum_filter_tail_source_frames": BANK_FRAMES - 1,
        "source_input_logical_bytes": input_bytes,
        "prepared_source_logical_bytes": 16 * frames,
        "prepared_complex_field_logical_bytes": 32 * field_frames,
        "preparation_estimated_peak_working_bytes": estimate,
        "estimated_peak_working_bytes": estimate,
        "preparation_memory_estimate_formula": ("input_bytes+64*N+96*(N+1023)+1048576; empty M=0"),
        "memory_limit_bytes": MAX_MEMORY_BYTES,
        "memory_accounting": (
            "conservative logical live arrays, input, immutable tobytes overlap, "
            "output and scratch; "
            "not measured RSS, allocator overhead or a process-wide memory guarantee"
        ),
        "memory_refusal_policy": "refuse before large allocation; no partial output or fallback",
        "direct_convolution_max_source_frames": MAX_SOURCE_FRAMES,
        "direct_convolution_work_bound": "four real convolutions, each at most ceil(N/2)*511 taps",
        "quadrature_kernel": "q[l]=2*w[l]/1024*cot(pi*l/1024) for odd l; even l=0; q[1023]=0",
        "window": "w[l]=.5+.5*cos(pi*l/1023), l=0..1023; w[1023]=0 exactly",
        "complex_field": "Q_c=x_c+i*(x_c*q); full causal linear convolution",
        "real_reconstruction": "Re(Q_c)=original source followed by exact zero extension",
        "bank_sum_order": "sum every band and every segment before extracting any phase",
        "source_cue_phase_resets": False,
        "source_segmentation_used": False,
        "quadrature_implementation": (
            "direct parity-split np.convolve with q[1:1023:2]; no FFT or bank-by-frame matrix"
        ),
        "structural_zero_policy": (
            "exact even taps and final tap remain zero; direct convolution retains structural "
            "impulse parity and unsupported zero regions without an amplitude threshold"
        ),
        "source_input_mutated": False,
        "prepared_arrays_immutable_bytes_backed": True,
        "new_separate_offline_experiment": True,
        "offline_full_source_lookahead": True,
        "native_used": False,
        "production_activation": False,
        "realtime_causality_claim": False,
        "overall_acoustic_acceptance": False,
        "normalization": "none",
        "fitted_parameters": False,
        "fixture_labels_or_target_outputs_used": False,
        "magnitude_replacement_used": False,
        "independent_band_phase_actions_used": False,
        "risks": (
            "the finite causal quadrature field is not an ideal analytic signal",
            "joint nonlinear phase action does not establish real PCM energy preservation",
            "direct convolution cancellation roundoff can perturb exact-zero phase decisions",
            "pooled products can underflow or cancel; the unchanged bank fallback remains",
            "unrelated stereo mixtures do not have a uniquely identified common physical phase",
            "fractional complex and elapsed-phase interpolation approximates continuous phase",
            "no anti-alias filter, shifted-frequency clamp, cutoff or window adjustment is used",
            "complete retained tails can alter the background after nonlinear phase action",
            "realtime safety, Native adoption, devices and listening remain unestablished",
        ),
    }


def _source_activity(source: Pcm) -> tuple[int | None, int]:
    last = None
    active_frames = 0
    for start in range(0, len(source), _VALIDATION_CHUNK_FRAMES):
        active = np.flatnonzero(
            np.any(source[start : start + _VALIDATION_CHUNK_FRAMES] != 0, axis=1)
        )
        active_frames += len(active)
        if len(active):
            last = start + int(active[-1])
    return last, active_frames


def _complete_field(source: Pcm, report: dict) -> ComplexPcm:
    frames = len(source)
    field = np.zeros((report["complete_complex_field_frames"], 2), dtype=np.complex128)
    field.real[:frames] = source
    for channel in range(2):
        for source_parity, first_target in ((0, 1), (1, 2)):
            samples = source[source_parity::2, channel]
            if not len(samples):
                continue
            with np.errstate(over="raise", invalid="raise"):
                try:
                    convolved = np.convolve(samples, _ODD_KERNEL)
                except FloatingPointError:
                    _refuse("quadrature_numeric_range", report)
            stop = first_target + 2 * len(convolved)
            field.imag[first_target:stop:2, channel] = convolved
    if not _all_finite(field):
        _refuse("nonfinite_complete_complex_field", report)
    return _immutable_copy(field)


def prepare(source: Pcm) -> Prepared:
    """Prepare the complete immutable finite-bank sum without rate or pitch intent.

    Args:
        source: Finite real numeric NumPy PCM with shape (frames, 2), including
            all original silence and tails, at the diagnostic 48000-Hz rate.

    Returns:
        Immutable float64 source and complex128 Q, with complete preparation
        details. Invalid shape or samples raise ValueError. Resource or numeric
        range refusal raises JointResidualPhaseError carrying a report.
    """
    frames, input_bytes = _source_shape(source)
    report = _preparation_report(frames, input_bytes)
    if report["estimated_peak_working_bytes"] > MAX_MEMORY_BYTES:
        _refuse("memory_limit", report)
    if frames > MAX_SOURCE_FRAMES:
        _refuse("direct_convolution_source_length_limit", report)
    if not _all_finite(source):
        message = "Source must contain only finite stereo PCM samples"
        raise ValueError(message)
    with np.errstate(over="raise", invalid="raise"):
        try:
            converted = np.array(source, dtype=np.float64, order="C", copy=True)
        except FloatingPointError:
            _refuse("source_float64_numeric_range", report)
    if not _all_finite(converted):
        _refuse("source_float64_numeric_range", report)
    pcm = _immutable_copy(converted)
    del converted
    last, active_frames = _source_activity(pcm)
    analytic_pcm = _complete_field(pcm, report)
    report.update(
        status="prepared",
        last_nonzero_source_frame=last,
        nonzero_source_frames=active_frames,
        source_exactly_silent=last is None,
        empty_source=frames == 0,
    )
    return Prepared(pcm, analytic_pcm, MappingProxyType(report))


def _finite_scalar(value: float, name: str) -> float:
    message = f"{name} must be a finite real scalar"
    if isinstance(value, (bool, np.bool_)) or not isinstance(
        value, (int, float, np.integer, np.floating)
    ):
        raise TypeError(message)
    try:
        result = float(value)
    except OverflowError as error:
        raise ValueError(message) from error
    if not math.isfinite(result):
        raise ValueError(message)
    return result


def _controls(rate: float, semitones: float) -> tuple[float, float, float]:
    rate = _finite_scalar(rate, "Rate")
    semitones = _finite_scalar(semitones, "Semitones")
    if rate <= 0.0:
        message = "Rate must be finite and strictly positive"
        raise ValueError(message)
    try:
        shift = 2.0 ** (semitones / 12.0)
    except OverflowError as error:
        message = "Pitch ratio h=2**(semitones/12) must be finite and strictly positive"
        raise ValueError(message) from error
    if not math.isfinite(shift) or shift <= 0.0:
        message = "Pitch ratio h=2**(semitones/12) must be finite and strictly positive"
        raise ValueError(message)
    return rate, semitones, shift


def _capture_support(prepared: Prepared, rate: float) -> tuple[int, int, int]:
    """Compute the old bank's signed capture bounds without float overflow."""
    frames = len(prepared.source)
    if not frames:
        return 0, 0, 0
    numerator, denominator = rate.as_integer_ratio()
    origin = min(0, (-denominator) // numerator + 1)
    nominal = (frames * denominator + numerator - 1) // numerator
    last = prepared.report["last_nonzero_source_frame"]
    retired = nominal
    if last is not None:
        retired = max(nominal, ((last + BANK_FRAMES - 1) * denominator) // numerator + 1)
    return origin, nominal, retired


def _validate_prepared(prepared: Prepared) -> None:
    if not isinstance(prepared, Prepared):
        message = "Prepared input must be returned by joint residual phase prepare"
        raise TypeError(message)
    source, field = prepared.source, prepared.analytic_pcm
    expected = len(source) + BANK_FRAMES - 1 if len(source) else 0
    message = "Prepared input must retain its immutable complete source and complex field"
    if (
        source.dtype != np.float64
        or source.ndim != 2
        or source.shape[1] != 2
        or source.flags.writeable
    ):
        raise ValueError(message)
    if (
        field.dtype != np.complex128
        or field.shape != (expected, 2)
        or field.flags.writeable
        or "last_nonzero_source_frame" not in prepared.report
    ):
        raise ValueError(message)


def render_prepared(prepared: Prepared, rate: float, semitones: float) -> tuple[Pcm, dict]:
    """Render one pooled phase action after complex sampling of complete Q.

    Args:
        prepared: Complete source-only preparation returned by prepare.
        rate: Finite strictly positive source frames per output frame.
        semitones: Finite pitch intent with finite positive h=2**(semitones/12).

    Returns:
        Fresh float64 stereo PCM over the old bank's complete signed capture,
        plus an explicit experimental report. Resource and numeric range limits
        raise JointResidualPhaseError before publishing partial output.
    """
    _validate_prepared(prepared)
    rate, semitones, shift = _controls(rate, semitones)
    origin, nominal, retired = _capture_support(prepared, rate)
    output_frames = retired - origin
    field_frames = len(prepared.analytic_pcm)
    estimate = (
        16 * len(prepared.source) + 256 * field_frames + 256 * output_frames + _MEMORY_RESERVE_BYTES
    )
    phase_multiplier = shift / rate
    report = {
        **prepared.report,
        "status": "rendering",
        "rate": rate,
        "semitones": semitones,
        "h": shift,
        "phase_multiplier": phase_multiplier if math.isfinite(phase_multiplier) else None,
        "estimated_peak_working_bytes": estimate,
        "render_memory_estimate_formula": "16*N+256*M+256*K+1048576; prepared source/Q included",
        "capture_origin": origin,
        "capture_origin_output_frame": origin,
        "nominal_output_frames": nominal,
        "retired_output_frame_exclusive": retired,
        "full_generated_output_frames": output_frames,
        "capture_allocation_limit_frames": output_frames,
        "global_coordinate": "s=r*(capture_origin+n)",
        "phase_rule": "Re{L(Q_c,s)*exp(i*(h/r-1)*L(Psi,s))}",
        "complex_sampling_before_phase_action": True,
        "phase_reference_omega": 0.0,
        "phase_elapsed_implementation": "unchanged pitch_amfm_model._phase_elapsed, omega=0",
        "render_implementation": "one unchanged pitch_amfm_model._render_band, omega=0",
        "global_phase_semantics": (
            "one pooled stereo phase on the complete globally summed Q; no band, cue or "
            "source-segment phase reset"
        ),
        "unwrap": "principal(arg(sum_c conj(Q_c[j-1])*Q_c[j])); principal in [-pi,pi)",
        "negative_pi_tie_convention": True,
        "birth_rule": "restart only after exact common zeros; no amplitude threshold",
        "birth_gauge": "dominant birth channel argument, left tie; cancels from elapsed Psi",
        "pooled_zero_fallback": "largest-magnitude channel product, left tie; then omega=0",
        "sampler": "complex linear Q interpolation and separate linear elapsed-Psi interpolation",
        "tail_policy": "full original nominal duration and every complete generated causal tail",
        "support_rule": (
            "origin=min(0,floor(-1/r)+1); nominal=ceil(N/r); "
            "retired=max(nominal,floor((last_nonzero+1023)/r)+1); silence retires at nominal"
        ),
        "integer_phase_rule_exact": True,
        "fractional_phase_rule_exact": False,
        "fractional_rate_only_identity": "h=r gives linear interpolation of original source",
        "identity_special_case_used": False,
    }
    if estimate > MAX_MEMORY_BYTES:
        _refuse("memory_limit", report)
    if not math.isfinite(phase_multiplier):
        _refuse("phase_multiplier_numeric_range", report)
    statistics = bank._Statistics()
    if field_frames:
        positions = rate * (origin + np.arange(output_frames, dtype=np.float64))
        with np.errstate(over="raise", invalid="raise", under="ignore"):
            try:
                output = np.array(
                    bank._render_band(
                        prepared.analytic_pcm, positions, 0.0, rate, shift, statistics
                    ),
                    dtype=np.float64,
                    order="C",
                    copy=True,
                )
            except FloatingPointError:
                _refuse("phase_or_output_numeric_range", {**report, **asdict(statistics)})
        if not _all_finite(output):
            _refuse("nonfinite_generated_output", {**report, **asdict(statistics)})
    else:
        output = np.zeros((0, 2), dtype=np.float64)
    report.update(
        status="rendered",
        numerical_render_completed=True,
        **asdict(statistics),
    )
    return output, report


def render(source: Pcm, rate: float, semitones: float) -> tuple[Pcm, dict]:
    """Prepare finite stereo source and render the separate offline experiment."""
    _controls(rate, semitones)
    return render_prepared(prepare(source), rate, semitones)
