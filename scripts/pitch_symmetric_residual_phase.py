"""Frozen offline phase experiment with a finite antisymmetric quadrature FIR.

The fixed one-sided quadrature kernel is mirrored with a negative sign, retaining
all negative source coordinates and both complete tails. This field is a new
noncausal experiment, not the original 513-band sum. It establishes no acoustic,
finite-order, production, Native, device, listening, or realtime acceptance.
"""

import math
from dataclasses import asdict, dataclass
from types import MappingProxyType
from typing import TYPE_CHECKING

import numpy as np
import pitch_amfm_model as bank
import pitch_joint_residual_phase as base

if TYPE_CHECKING:
    from collections.abc import Mapping
    from typing import Never

    from numpy.typing import NDArray

type Pcm = NDArray[np.float64]
type ComplexPcm = NDArray[np.complex128]

BANK_FRAMES = base.BANK_FRAMES
FILTER_RADIUS_SOURCE_FRAMES = BANK_FRAMES - 1
FILTER_FRAMES = 2 * FILTER_RADIUS_SOURCE_FRAMES + 1
MAX_MEMORY_BYTES = base.MAX_MEMORY_BYTES
# Two unchanged preparations double the original direct-convolution work. Keep
# their combined maximum source length within the original fixed work guard.
MAX_SOURCE_FRAMES = base.MAX_SOURCE_FRAMES // 2


class SymmetricResidualPhaseError(base.JointResidualPhaseError):
    """Refuse a resource or numerical range with the base error's report contract."""

    def __init__(self, reason: str, report: dict) -> None:
        self.report = {**report, "status": "refused", "refusal_reason": reason}
        ValueError.__init__(self, f"Symmetric residual phase experiment refused: {reason}")


@dataclass(frozen=True, slots=True)
class Prepared:
    """Retain immutable PCM and its complete field on a signed source clock.

    The field has origin -1023 and N+2046 stereo frames for nonempty input.
    Empty input has an empty field and origin zero. Both arrays have immutable
    bytes backing; the preparation report is a read-only mapping.
    """

    source: Pcm
    analytic_pcm: ComplexPcm
    field_origin: int
    report: Mapping[str, object]


def _refuse(reason: str, report: dict) -> Never:
    raise SymmetricResidualPhaseError(reason, report)


def _preparation_report(frames: int, input_bytes: int) -> dict:
    radius = FILTER_RADIUS_SOURCE_FRAMES
    field_frames = frames + 2 * radius if frames else 0
    field_origin = -radius if frames else 0
    field_overhead = 256 * (field_frames - frames)
    estimate = input_bytes + 512 * frames + field_overhead + base._MEMORY_RESERVE_BYTES
    return {
        "model": "symmetric-residual-phase-offline-v1",
        "status": "preparing",
        "refusal_reason": None,
        "source_frames": frames,
        "complete_complex_field_frames": field_frames,
        "field_origin": field_origin,
        "field_origin_source_frame": field_origin,
        "sample_rate_hz": bank.FS,
        "filter_frames": FILTER_FRAMES,
        "fixed_one_sided_kernel_frames": BANK_FRAMES,
        "source_quadrature_kernel_bank_count": bank.BAND_COUNT,
        "original_complete_bank_sum": False,
        "maximum_filter_advance_source_frames": radius,
        "maximum_filter_tail_source_frames": radius,
        "source_input_logical_bytes": input_bytes,
        "prepared_source_logical_bytes": 16 * frames,
        "prepared_complex_field_logical_bytes": 32 * field_frames,
        "preparation_estimated_peak_working_bytes": estimate,
        "estimated_peak_working_bytes": estimate,
        "preparation_memory_estimate_formula": (
            "input_bytes+512*N+256*(M-N)+1048576; M=N+2046, or M=0 when N=0"
        ),
        "memory_limit_bytes": MAX_MEMORY_BYTES,
        "memory_accounting": (
            "conservative logical live arrays including input, both one-sided preparations, "
            "their immutable tobytes temporaries, symmetric field and its immutable copy, "
            "output and scratch; not measured RSS, allocator overhead or a process-wide guarantee"
        ),
        "memory_refusal_policy": "refuse before large allocation; no partial output or fallback",
        "direct_convolution_max_source_frames": MAX_SOURCE_FRAMES,
        "direct_convolution_work_bound": (
            "eight real convolutions, each at most ceil(N/2)*511 taps; "
            "2*N bounded by the original one-sided maximum source length"
        ),
        "quadrature_kernel": (
            "q_sym[l]=q[l] for l>0; q_sym[-l]=-q[l]; q_sym[0]=0; "
            "q[l]=2*w[l]/1024*cot(pi*l/1024) for odd l; even l=0; "
            "q_sym[-1023]=q_sym[1023]=0 exactly"
        ),
        "window": "w[l]=.5+.5*cos(pi*l/1023), l=0..1023; w[1023]=0 exactly",
        "complex_field": (
            "Q_sym,c=x_c+i*(x_c*q_sym); complete signed linear convolution; "
            "field index j represents source coordinate j-1023 for nonempty input"
        ),
        "real_reconstruction": (
            "Re(Q_sym,c)[1023:1023+N]=original source; exact zero extension on both sides"
        ),
        "source_cue_phase_resets": False,
        "source_segmentation_used": False,
        "quadrature_implementation": (
            "two unchanged pitch_joint_residual_phase.prepare calls, forward and reversed; "
            "combine their parity-split direct convolutions with opposite signs; "
            "no FFT or bank-by-frame matrix"
        ),
        "structural_zero_policy": (
            "exact even, center and +/-1023 kernel taps remain zero; "
            "retain complete negative coordinates and both tails without an amplitude threshold"
        ),
        "source_input_mutated": False,
        "prepared_arrays_immutable_bytes_backed": True,
        "new_separate_offline_experiment": True,
        "offline_full_source_lookahead": True,
        "noncausal_source_clock": True,
        "maximum_source_frame_lookahead": radius,
        "native_used": False,
        "native_adoption_established": False,
        "production_activation": False,
        "realtime_causality_claim": False,
        "finite_order_proof_established": False,
        "overall_acoustic_acceptance": False,
        "normalization": "none",
        "fitted_parameters": False,
        "delay_fit_used": False,
        "gain_fit_used": False,
        "capture_crop_used": False,
        "weight_search_used": False,
        "window_search_used": False,
        "cutoff_search_used": False,
        "fixture_labels_or_target_outputs_used": False,
        "magnitude_replacement_used": False,
        "independent_band_phase_actions_used": False,
        "risks": (
            "the finite antisymmetric quadrature field is not an ideal analytic signal",
            "the field requires 1023 source frames of lookahead and can generate pre-event output",
            "joint nonlinear phase action does not establish real PCM energy preservation",
            "direct convolution and signed combination roundoff can perturb exact-zero decisions",
            "pooled products can underflow or cancel; the unchanged bank fallback remains",
            "unrelated stereo mixtures do not have a uniquely identified common physical phase",
            "fractional complex and elapsed-phase interpolation approximates continuous phase",
            "no anti-alias filter, shifted-frequency clamp, cutoff or window adjustment is used",
            "complete retained pre-event output and tails can alter nonlinear output background",
            (
                "finite-order proof, realtime safety, Native adoption and listening "
                "remain unestablished"
            ),
        ),
    }


def prepare(source: Pcm) -> Prepared:
    """Prepare the fixed complete symmetric field without rate or pitch intent.

    Args:
        source: Finite real numeric NumPy stereo PCM of shape (frames, 2),
            retaining all original silence and tails at the diagnostic 48000 Hz.

    Returns:
        Immutable float64 source, complex128 signed field, field origin and
        preparation report. Invalid inputs raise ValueError or TypeError.
        Resource and numerical range refusals carry an explicit report.
    """
    frames, input_bytes = base._source_shape(source)
    report = _preparation_report(frames, input_bytes)
    if report["estimated_peak_working_bytes"] > MAX_MEMORY_BYTES:
        _refuse("memory_limit", report)
    if frames > MAX_SOURCE_FRAMES:
        _refuse("direct_convolution_source_length_limit", report)
    direction = "forward"
    try:
        forward = base.prepare(source)
        direction = "backward"
        backward = base.prepare(source[::-1])
    except base.JointResidualPhaseError as error:
        _refuse(
            error.report["refusal_reason"],
            {**report, "one_sided_helper_refusal_direction": direction},
        )
    field = np.zeros((report["complete_complex_field_frames"], 2), dtype=np.complex128)
    if frames:
        radius = FILTER_RADIUS_SOURCE_FRAMES
        with np.errstate(over="raise", invalid="raise"):
            try:
                field.real[radius : radius + frames] = forward.source
                field.imag[radius:] += forward.analytic_pcm.imag
                field.imag[: frames + radius] -= backward.analytic_pcm.imag[::-1]
            except FloatingPointError:
                _refuse("quadrature_numeric_range", report)
    if not base._all_finite(field):
        _refuse("nonfinite_complete_complex_field", report)
    analytic_pcm = base._immutable_copy(field)
    report.update(
        status="prepared",
        last_nonzero_source_frame=forward.report["last_nonzero_source_frame"],
        nonzero_source_frames=forward.report["nonzero_source_frames"],
        source_exactly_silent=forward.report["source_exactly_silent"],
        empty_source=frames == 0,
    )
    return Prepared(
        forward.source,
        analytic_pcm,
        report["field_origin"],
        MappingProxyType(report),
    )


def _capture_support(prepared: Prepared, rate: float) -> tuple[int, int, int]:
    """Calculate signed capture bounds with exact float-ratio integer arithmetic."""
    frames = len(prepared.source)
    if not frames:
        return 0, 0, 0
    numerator, denominator = rate.as_integer_ratio()
    nominal = (frames * denominator + numerator - 1) // numerator
    last = prepared.report["last_nonzero_source_frame"]
    if last is None:
        return 0, nominal, nominal
    radius = FILTER_RADIUS_SOURCE_FRAMES
    origin = min(0, ((-radius - 1) * denominator) // numerator + 1)
    retired = max(nominal, ((last + radius) * denominator) // numerator + 1)
    return origin, nominal, retired


def _validate_prepared(prepared: Prepared) -> None:
    if not isinstance(prepared, Prepared):
        message = "Prepared input must be returned by symmetric residual phase prepare"
        raise TypeError(message)
    source, field = prepared.source, prepared.analytic_pcm
    frames = len(source)
    expected_frames = frames + 2 * FILTER_RADIUS_SOURCE_FRAMES if frames else 0
    expected_origin = -FILTER_RADIUS_SOURCE_FRAMES if frames else 0
    message = "Prepared input must retain its immutable complete source and signed complex field"
    if (
        source.dtype != np.float64
        or source.ndim != 2
        or source.shape[1] != 2
        or source.flags.writeable
    ):
        raise ValueError(message)
    if (
        field.dtype != np.complex128
        or field.shape != (expected_frames, 2)
        or field.flags.writeable
        or prepared.field_origin != expected_origin
        or "last_nonzero_source_frame" not in prepared.report
    ):
        raise ValueError(message)


def render_prepared(prepared: Prepared, rate: float, semitones: float) -> tuple[Pcm, dict]:
    """Apply the unchanged pooled phase rule once to the complete symmetric field.

    Args:
        prepared: Source-only preparation returned by prepare.
        rate: Finite strictly positive source frames per output frame.
        semitones: Finite pitch intent with finite positive h=2**(semitones/12).

    Returns:
        Fresh float64 stereo PCM including all signed pre-event coordinates,
        nominal duration and complete tails, plus an experimental report.
        Exactly silent input may use origin zero and the nominal duration.
        Resource or numerical range refusals publish no partial output.
    """
    _validate_prepared(prepared)
    rate, semitones, shift = base._controls(rate, semitones)
    origin, nominal, retired = _capture_support(prepared, rate)
    output_frames = retired - origin
    field_frames = len(prepared.analytic_pcm)
    estimate = (
        16 * len(prepared.source)
        + 256 * field_frames
        + 256 * output_frames
        + base._MEMORY_RESERVE_BYTES
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
        "global_coordinate": "s=r*(capture_origin+n); field_index=s-field_origin",
        "phase_rule": (
            "Re{L(Q_sym,c,u)*exp(i*(h/r-1)*L(Psi,u))}; u=r*(capture_origin+n)-field_origin"
        ),
        "complex_sampling_before_phase_action": True,
        "phase_reference_omega": 0.0,
        "phase_elapsed_implementation": "unchanged pitch_amfm_model._phase_elapsed, omega=0",
        "render_implementation": "one unchanged pitch_amfm_model._render_band, omega=0",
        "global_phase_semantics": (
            "one pooled stereo phase on the complete signed symmetric field including "
            "negative source coordinates; no band, cue or source-segment phase reset"
        ),
        "unwrap": "principal(arg(sum_c conj(Q_c[j-1])*Q_c[j])); principal in [-pi,pi)",
        "negative_pi_tie_convention": True,
        "birth_rule": "restart only after exact common zeros; no amplitude threshold",
        "birth_gauge": "dominant birth channel argument, left tie; cancels from elapsed Psi",
        "pooled_zero_fallback": "largest-magnitude channel product, left tie; then omega=0",
        "sampler": "complex linear Q interpolation and separate linear elapsed-Psi interpolation",
        "tail_policy": (
            "retain every signed pre-event coordinate, full original nominal duration and "
            "every complete generated positive tail; no output-dependent crop"
        ),
        "support_rule": (
            "nonempty/nonzero: origin=min(0,floor((-1023-1)/r)+1); nominal=ceil(N/r); "
            "retired=max(nominal,floor((last_nonzero+1023)/r)+1); "
            "exact silence uses origin=0 and retired=nominal; empty uses all zero bounds"
        ),
        "capture_bounds_implementation": (
            "exact integers from rate.as_integer_ratio before allocation"
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
        with np.errstate(over="raise", invalid="raise", under="ignore"):
            try:
                positions = (
                    rate * (origin + np.arange(output_frames, dtype=np.float64))
                    - prepared.field_origin
                )
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
        if not base._all_finite(output):
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
    """Prepare finite stereo source and render the separate symmetric experiment."""
    base._controls(rate, semitones)
    return render_prepared(prepare(source), rate, semitones)
