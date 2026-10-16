"""Attribute the frozen noise response to real band autoenergy and signed cross.

This read-only offline experiment changes no synthesis rule. Source cue segments
are summed within each band before squaring; all captures retain nominal silence,
exact interpolation support and complete finite convolution tails. Signed cross
energy is never passed to a quantile function or normalized by its own total.
"""

import math
from dataclasses import dataclass
from itertools import pairwise
from typing import TYPE_CHECKING

import numpy as np
import pitch_amfm_model as bank
from pitch_amfm_model import BAND_COUNT, BANK_FRAMES, render
from pitch_causal_model_probe import FS, IDENTITY_LIMIT, emit
from pitch_independent_modes_probe import cases
from pitch_overlap_model import source_cues
from pitch_overlap_probe import (
    HALF_WINDOW,
    Fixture,
    energy_times,
    envelope,
    metric_controls,
    response_report,
)

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Real = NDArray[np.float64]
type Complex = NDArray[np.complex128]

RATE = 2.0
SEMITONES = (-7, 0, 7, 12)
CONTROL_INDEX = SEMITONES.index(12)
FRACTIONS = (0.1, 0.5, 0.9)
LOGICAL_MEMORY_LIMIT = 256 * 1024**2
RECONSTRUCTION_ABS_LIMIT = IDENTITY_LIMIT


@dataclass(slots=True)
class _Geometry:
    origin: int
    nominal: int
    allocated: int
    retired: int


@dataclass(slots=True)
class _Segment:
    pcm: Real
    birth: int
    start: int
    stop: int
    fft_frames: int
    record: dict


@dataclass(slots=True)
class _Prepared:
    segment: _Segment
    source_fft: Complex
    prototype_fft: Complex
    supported: NDArray[np.bool_]
    positions: Real


@dataclass(slots=True)
class _Audit:
    output: Real
    auto: Real
    source_sum: Real
    source_real_auto: Real
    source_complex_auto: Real


def _geometry(source: Real, rate: float) -> _Geometry:
    capture = bank._new_capture(source, rate)
    return _Geometry(capture.origin, capture.nominal, len(capture.output), capture.retired)


def _segments(source: Real, cues: list[int]) -> list[_Segment]:
    result = []
    for begin, end in pairwise(cues):
        block = source[begin:end]
        nonzero = np.flatnonzero(np.any(block != 0.0, axis=1))
        if not len(nonzero):
            continue
        first, last = int(nonzero[0]), int(nonzero[-1])
        pcm, birth = block[first : last + 1], begin + first
        count = len(pcm) + BANK_FRAMES - 1
        start = math.floor((birth - 1) / RATE) + 1
        stop = math.floor((birth + count - 1) / RATE) + 1
        result.append(
            _Segment(
                pcm,
                birth,
                start,
                stop,
                1 << (max(BANK_FRAMES, count) - 1).bit_length(),
                {
                    "source_begin": begin,
                    "source_end": end,
                    "birth_source_frame": birth,
                    "trimmed_leading_input_zeros": first,
                    "trimmed_trailing_input_zeros": len(block) - last - 1,
                    "nonzero_input_span_frames": len(pcm),
                    "complete_convolution_frames": count,
                    "output_begin": start,
                    "output_end_exclusive": stop,
                },
            )
        )
    return result


def _memory_plan(segments: list[_Segment], source: _Geometry, output: _Geometry) -> dict:
    cached = sum(
        48 * value.fft_frames + len(value.pcm) + BANK_FRAMES - 1 + 8 * (value.stop - value.start)
        for value in segments
    )
    largest_fft = max((value.fft_frames for value in segments), default=BANK_FRAMES)
    # Conservative logical allowance: cached FFTs, persistent/band captures,
    # metrics/reference render and FFT/phase temporaries, plus construction space.
    estimate = cached + 96 * source.allocated + 256 * output.allocated
    estimate += 512 * largest_fft + 64 * 1024**2
    if estimate > LOGICAL_MEMORY_LIMIT:
        message = f"Noise energy audit logical memory {estimate} exceeds {LOGICAL_MEMORY_LIMIT}"
        raise ValueError(message)
    return {
        "estimated_logical_working_bytes": estimate,
        "logical_memory_limit_bytes": LOGICAL_MEMORY_LIMIT,
        "cached_segment_fft_bytes_with_positions_and_support": cached,
        "maximum_segment_fft_frames": largest_fft,
        "all_band_PCM_matrix_allocated": False,
        "logical_allowance_is_not_measured_peak_RSS": True,
    }


def _prepare(segments: list[_Segment]) -> list[_Prepared]:
    result = []
    for segment in segments:
        source_fft, prototype_fft, supported = bank._prepare_fft(segment.pcm)
        assert len(source_fft) == segment.fft_frames
        positions = RATE * np.arange(segment.start, segment.stop, dtype=np.float64)
        result.append(
            _Prepared(segment, source_fft, prototype_fft, supported, positions - segment.birth)
        )
    return result


def _accumulate_band(
    prepared: list[_Prepared],
    band: int,
    source_band: Complex,
    profile_band: Real,
    source_geometry: _Geometry,
    output_geometry: _Geometry,
    statistics: list[bank._Statistics],
) -> None:
    source_band.fill(0.0)
    profile_band.fill(0.0)
    for value in prepared:
        values = bank._band_signal(
            value.segment.pcm, value.source_fft, value.prototype_fft, value.supported, band
        )
        begin = value.segment.birth - source_geometry.origin
        source_band[begin : begin + len(values)] += values
        begin, stop = (
            value.segment.start - output_geometry.origin,
            value.segment.stop - output_geometry.origin,
        )
        for index, semitones in enumerate(SEMITONES):
            profile_band[index, begin:stop] += bank._render_band(
                values,
                value.positions,
                float(bank._OMEGA[band]),
                RATE,
                2.0 ** (semitones / 12.0),
                statistics[index],
            )


def _audit(prepared: list[_Prepared], source: _Geometry, output: _Geometry) -> _Audit:
    result = _Audit(
        np.zeros((len(SEMITONES), output.allocated, 2)),
        np.zeros((len(SEMITONES), output.allocated)),
        np.zeros((source.allocated, 2)),
        np.zeros(source.allocated),
        np.zeros(source.allocated),
    )
    source_band = np.zeros((source.allocated, 2), dtype=np.complex128)
    profile_band = np.zeros_like(result.output)
    statistics = [bank._Statistics() for _ in SEMITONES]
    for band in range(BAND_COUNT):
        _accumulate_band(prepared, band, source_band, profile_band, source, output, statistics)
        result.source_sum += source_band.real
        result.source_real_auto += np.sum(source_band.real * source_band.real, axis=1)
        result.source_complex_auto += np.sum(
            source_band.real * source_band.real + source_band.imag * source_band.imag, axis=1
        )
        result.output += profile_band
        result.auto += np.sum(profile_band * profile_band, axis=2)
    for values in (
        result.output,
        result.auto,
        result.source_sum,
        result.source_real_auto,
        result.source_complex_auto,
    ):
        assert np.isfinite(values).all(), "Nonfinite energy audit capture"
    return result


def _dry(source: Real, geometry: _Geometry, rate: int) -> Real:
    indices = rate * (geometry.origin + np.arange(geometry.retired - geometry.origin))
    inside = (indices >= 0) & (indices < len(source))
    result = np.zeros((len(indices), 2))
    result[inside] = source[indices[inside]]
    return result


def _balances(values: Real, dry_quantiles: list[int]) -> Real:
    cumulative = np.cumsum(envelope(values))
    return np.array([
        cumulative[boundary + HALF_WINDOW] - fraction * cumulative[-1]
        for boundary, fraction in zip(dry_quantiles, FRACTIONS, strict=True)
    ])


def _balance_bound(energy: Real, auto: Real, cross: Real, dry_total: float) -> float:
    # A conservative length-dependent numerical comparison budget for prefix
    # sums, centered-box differences and their cumulative sums. This is not a
    # timing/gain tolerance or a claim of machine-independent exact arithmetic.
    width = 2 * HALF_WINDOW + 1
    length = len(energy) + width
    magnitude = sum(float(np.sum(np.abs(value))) for value in (energy, auto, cross))
    return (
        64.0 * np.finfo(np.float64).eps * length * length / width * max(1.0, magnitude) / dry_total
    )


def _energy_summary(values: Real, reference: Real, origin: int) -> dict:
    assert np.min(values, initial=0.0) >= 0.0
    quantiles, dry_quantiles = energy_times(values), energy_times(reference)
    return {
        "total": float(np.sum(values)),
        "q10_q50_q90_frames": [value + origin for value in quantiles],
        "q_offsets_against_same_clock_dry_frames": [
            actual - dry for actual, dry in zip(quantiles, dry_quantiles, strict=True)
        ],
    }


def _attribution(energy: Real, auto: Real, dry_energy: Real) -> dict:
    cross = energy - auto
    dry_total = float(np.sum(dry_energy))
    boundaries = energy_times(dry_energy)
    balances = np.array([_balances(value, boundaries) for value in (energy, auto, cross)])
    normalized = balances / dry_total
    error = float(np.max(np.abs(normalized[0] - normalized[1] - normalized[2])))
    bound = _balance_bound(energy, auto, cross, dry_total)
    assert error <= bound, "Signed smoothed balance decomposition exceeded float budget"
    return {
        "same_dry_total_energy_denominator": dry_total,
        "fixed_dry_q10_q50_q90_boundaries_frames": boundaries,
        "whole_E_balances_over_same_dry_total": normalized[0].tolist(),
        "band_A_balances_over_same_dry_total": normalized[1].tolist(),
        "signed_C_balances_over_same_dry_total": normalized[2].tolist(),
        "balance_RE_minus_RA_minus_RC_max_abs": error,
        "declared_normalized_float_comparison_bound": bound,
        "whole_E_total_over_same_dry_total": float(np.sum(energy)) / dry_total,
        "band_A_total_over_same_dry_total": float(np.sum(auto)) / dry_total,
        "signed_C_total_over_same_dry_total": float(np.sum(cross)) / dry_total,
        "positive_C_integral_over_same_dry_total": float(np.sum(np.maximum(cross, 0))) / dry_total,
        "negative_C_integral_over_same_dry_total": float(np.sum(np.minimum(cross, 0))) / dry_total,
        "signed_cross_quantiles_or_CDF_computed": False,
    }


def _source_report(source: Real, audit: _Audit, geometry: _Geometry) -> dict:
    stop = geometry.retired - geometry.origin
    dry = _dry(source, geometry, 1)
    reconstructed = audit.source_sum[:stop]
    error = float(np.max(np.abs(reconstructed - dry)))
    assert error <= RECONSTRUCTION_ABS_LIMIT
    dry_energy = np.sum(dry * dry, axis=1)
    return {
        "stage": "noise_energy_source_grid",
        "capture_origin": geometry.origin,
        "complete_capture_frames": stop,
        "complete_source_reconstruction_max_abs_error": error,
        "declared_absolute_reconstruction_comparison_limit": RECONSTRUCTION_ABS_LIMIT,
        "real_band_autoenergy": _energy_summary(
            audit.source_real_auto[:stop], dry_energy, geometry.origin
        ),
        "complex_coefficient_autoenergy": _energy_summary(
            audit.source_complex_auto[:stop], dry_energy, geometry.origin
        ),
        "source_grid_attribution": _attribution(
            np.sum(reconstructed * reconstructed, axis=1), audit.source_real_auto[:stop], dry_energy
        ),
        "complex_norm_statistics_are_descriptive_not_an_allpass_delay_proof": True,
    }


def _profile_report(
    fixture: Fixture, audit: _Audit, geometry: _Geometry, index: int, dry: Real
) -> dict:
    semitones = SEMITONES[index]
    stop = geometry.retired - geometry.origin
    output, auto = audit.output[index, :stop], audit.auto[index, :stop]
    original, details = render(fixture.source, RATE, semitones)
    assert details["capture_origin"] == geometry.origin
    assert original.shape == output.shape
    error = float(np.max(np.abs(output - original)))
    assert error <= RECONSTRUCTION_ABS_LIMIT
    measured = response_report(output, dry, fixture, int(RATE), geometry.origin)
    frozen = response_report(original, dry, fixture, int(RATE), geometry.origin)
    assert measured["q_offsets_frames"] == frozen["q_offsets_frames"]
    dry_energy = np.sum(dry * dry, axis=1)
    return {
        "stage": "noise_energy_profile",
        "fixture": fixture.name,
        "rate": RATE,
        "semitones": semitones,
        "h": 2.0 ** (semitones / 12.0),
        "unwarped_rate_only_control": semitones == 12,
        "audit_vs_frozen_render_whole_PCM_max_abs_error": error,
        "declared_absolute_render_comparison_limit": RECONSTRUCTION_ABS_LIMIT,
        "different_summation_order_bitwise_equality_claimed": False,
        "original_response_report": measured,
        "frozen_render_original_q_offsets_frames": frozen["q_offsets_frames"],
        "real_band_autoenergy": _energy_summary(auto, dry_energy, geometry.origin),
        "attribution": _attribution(np.sum(output * output, axis=1), auto, dry_energy),
    }


def _control_comparison(audit: _Audit, source: _Geometry, output: _Geometry, dry: Real) -> dict:
    stop = output.retired - output.origin
    coordinates = 2 * (output.origin + np.arange(stop)) - source.origin
    reference_auto = np.zeros(stop)
    inside = (coordinates >= 0) & (coordinates < source.retired - source.origin)
    reference_auto[inside] = audit.source_real_auto[coordinates[inside]]
    auto_error = float(np.max(np.abs(audit.auto[CONTROL_INDEX, :stop] - reference_auto)))
    error = float(np.max(np.abs(audit.output[CONTROL_INDEX, :stop] - dry)))
    assert max(error, auto_error) <= RECONSTRUCTION_ABS_LIMIT
    return {
        "rate": RATE,
        "h": RATE,
        "semitones": 12,
        "whole_capture_vs_canonical_dry_max_abs_error": error,
        "real_band_auto_vs_decimated_source_grid_auto_max_abs_error": auto_error,
        "declared_absolute_comparison_limit": RECONSTRUCTION_ABS_LIMIT,
    }


def _control_deltas(records: list[dict]) -> list[dict]:
    reference = records[CONTROL_INDEX]["attribution"]
    keys = (
        "whole_E_balances_over_same_dry_total",
        "band_A_balances_over_same_dry_total",
        "signed_C_balances_over_same_dry_total",
    )
    return [
        {
            "semitones": record["semitones"],
            "fixed_dry_boundary_balance_differences_from_h_equals_r_control": {
                key: (np.asarray(record["attribution"][key]) - reference[key]).tolist()
                for key in keys
            },
        }
        for record in records
    ]


def _scalar_smoothed(values: Real) -> Real:
    width = 2 * HALF_WINDOW + 1
    return np.array([
        sum(
            float(values[index])
            for index in range(max(0, frame - width + 1), min(frame + 1, len(values)))
        )
        / width
        for frame in range(len(values) + width - 1)
    ])


def _segmentation_controls(component: Real) -> dict:
    same_band_auto = float(np.sum((component - component) ** 2))
    segment_atom_auto = float(2 * np.sum(component * component))
    different_band_auto = 2 * component * component
    different_band_cross = -2 * component * component
    assert same_band_auto == 0.0
    assert segment_atom_auto > 0.0
    assert np.array_equal(different_band_auto + different_band_cross, np.zeros(len(component)))
    return {
        "same_band_two_canceling_segments_correct_autoenergy": same_band_auto,
        "incorrect_segment_atom_autoenergy_would_be": segment_atom_auto,
        "incorrect_same_band_cross_integral_would_be": -segment_atom_auto,
        "different_canceling_bands_positive_autoenergy": float(np.sum(different_band_auto)),
        "different_canceling_bands_negative_crossenergy": float(np.sum(different_band_cross)),
    }


def _arithmetic_controls() -> dict:
    first = np.array([1.0, 2.0, -1.0, 0.5, 0.0])
    second = np.array([0.25, -1.0, 0.75, 0.0, 1.0])
    energy, auto, cross = (first + second) ** 2, first**2 + second**2, 2 * first * second
    assert np.array_equal(energy - auto, cross)
    assert np.min(cross) < 0 < np.max(cross)
    dry = np.array([1.0, 0.0, 3.0, 0.0, 2.0])
    boundaries = energy_times(dry)
    actual = np.array([_balances(value, boundaries) for value in (energy, auto, cross)])
    expected = np.array([
        [
            sum(smoothed[: boundary + HALF_WINDOW + 1]) - fraction * sum(smoothed)
            for boundary, fraction in zip(boundaries, FRACTIONS, strict=True)
        ]
        for smoothed in (_scalar_smoothed(value) for value in (energy, auto, cross))
    ])
    error = float(np.max(np.abs(actual - expected)))
    linear_error = float(np.max(np.abs(actual[0] - actual[1] - actual[2])))
    bound = _balance_bound(energy, auto, cross, 1.0)
    assert max(error, linear_error) <= bound
    return {
        "stage": "noise_energy_independent_arithmetic_controls",
        "passed": True,
        **_segmentation_controls(first),
        "signed_cross_has_both_signs": bool(np.min(cross) < 0 < np.max(cross)),
        "scalar_window_balance_max_abs_error": error,
        "signed_linear_balance_max_abs_error": linear_error,
        "declared_float_comparison_bound": bound,
        "signed_cross_quantiles_or_CDF_computed": False,
    }


def run_noise_audit(fixture: Fixture) -> None:
    """Observe only this frozen noise source through unchanged bank operations."""
    # This preliminary frame guard also bounds _new_capture's temporary geometry
    # arrays. The detailed plan precedes cached FFTs and persistent audit arrays.
    if fixture.name != "unseen_noise" or len(fixture.source) * 256 > LOGICAL_MEMORY_LIMIT:
        message = "This bounded audit supports the unchanged unseen_noise source only"
        raise ValueError(message)
    cues = source_cues(fixture.source)
    segments = _segments(fixture.source, cues)
    if not segments:
        message = "The frozen noise audit requires an active source segment"
        raise ValueError(message)
    source, output = _geometry(fixture.source, 1.0), _geometry(fixture.source, RATE)
    source.retired = max([
        source.nominal,
        *(value.birth + len(value.pcm) + BANK_FRAMES - 1 for value in segments),
    ])
    output.retired = max([output.nominal, *(value.stop for value in segments)])
    assert source.retired - source.origin <= source.allocated
    assert output.retired - output.origin <= output.allocated
    emit({
        "stage": "noise_energy_support_and_memory",
        **_memory_plan(segments, source, output),
        "cue_indices": cues,
        "active_segment_count": len(segments),
        "bands_per_active_segment": BAND_COUNT,
        "source_capture_origin": source.origin,
        "source_capture_end_exclusive": source.retired,
        "output_capture_origin": output.origin,
        "output_capture_end_exclusive": output.retired,
        "source_nominal_frames": source.nominal,
        "output_nominal_frames": output.nominal,
        "segments": [value.record for value in segments],
        "per_band_segments_summed_before_real_or_complex_squaring": True,
    })
    audit = _audit(_prepare(segments), source, output)
    emit(_source_report(fixture.source, audit, source))
    dry = _dry(fixture.source, output, 2)
    emit({
        "stage": "noise_energy_h_equals_r_control",
        **_control_comparison(audit, source, output, dry),
    })
    records = [
        _profile_report(fixture, audit, output, index, dry) for index in range(len(SEMITONES))
    ]
    for record in records:
        emit(record)
    emit({
        "stage": "noise_energy_conclusion",
        "profile_differences_against_unwarped_control": _control_deltas(records),
        "source_and_synthesis_and_original_quality_gates_changed": False,
        "crossenergy_causal_uniqueness_or_per_band_allpass_delay_proved": False,
        "complex_magnitude_alone_determines_real_autoenergy": False,
        "native_production_or_overall_acoustic_acceptance": False,
    })


def main() -> None:
    """Run frozen metric/arithmetic controls and the single noise-source audit."""
    emit({
        "stage": "noise_energy_contract",
        "sample_rate_hz": FS,
        "rate": RATE,
        "semitones": list(SEMITONES),
        "h_equals_r_control_semitones": 12,
        "source": "unchanged cases().unseen_noise, original seed/gates/stereo/full PCM",
        "energy_definitions": (
            "u_b=sum_segments u_segment,b; A=sum_b||u_b||^2; E=||sum_bu_b||^2; C=E-A"
        ),
        "balance_definition": "R_X(p)=sum(envelope(X)[:dry_q_p+H+1])-p*sum(envelope(X))",
        "all_balances_and_crossintegrals_use_same_dry_total_denominator": True,
        "no_signed_cross_quantiles_or_CDF": True,
        "no_phase_weight_window_cutoff_gain_delay_or_fixture_fit": True,
    })
    emit(metric_controls())
    emit(_arithmetic_controls())
    fixture = next(value for value in cases() if value.name == "unseen_noise")
    run_noise_audit(fixture)


if __name__ == "__main__":
    main()
