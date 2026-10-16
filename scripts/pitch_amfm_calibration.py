"""Predict the frozen AM/FM bank's raw synthesis without fitting wet output.

Oracle carrier/gate parameters are confined to these analytic controls. The
unchanged renderer receives only original PCM, rate and semitone intent.
This is a calibration of a rejected model, not a corrected music renderer.
"""

import math
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

import numpy as np
from pitch_amfm_model import BAND_COUNT, BANK_FRAMES, render
from pitch_causal_model_probe import EVENT_FRAMES, FS, SOURCE_TARGET, Pcm, emit
from pitch_overlap_probe import source_cases

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Complex = NDArray[np.complex128]

FREQUENCY = 900.37
OMEGA = 2.0 * np.pi * FREQUENCY / FS
RAMP = 96
PHASOR = 0.25 * np.exp(1j * (0.37 - np.pi / 2.0))
ALPHA = np.pi / (BANK_FRAMES - 1)
SEMITONES = (-7, 0, 7)
SHIFTS = 2.0 ** (np.array(SEMITONES) / 12.0)
Q = SHIFTS / 2.0
SOURCE_COUNT = EVENT_FRAMES + BANK_FRAMES - 1
CLOCK = np.arange(SOURCE_COUNT)
SAMPLE_POSITIONS = np.array([0, 1, 95, 96, 1118, 8192, 40960, 47903, 47999, 49020, 49022])
STEADY_POSITIONS = np.array([8192, 12345, 20480, 32768, 40960])


@dataclass(slots=True)
class Prediction:
    """Accumulate complete predictions and their independent integrity evidence."""

    output: Pcm = field(default_factory=lambda: np.zeros((3, (SOURCE_COUNT + 1) // 2)))
    reconstruction: Pcm = field(default_factory=lambda: np.zeros(SOURCE_COUNT))
    diagonal_energy: Pcm = field(default_factory=lambda: np.zeros(3))
    principal_surrogate: Pcm = field(default_factory=lambda: np.zeros((3, 16_384)))
    direct_state_error: float = 0.0
    minimum_nonzero_magnitude: float = math.inf
    interior_branch_margin: float = math.inf
    total_winding_steps: int = 0
    negative_image_dominant_bands: int = 0


def geometric(angle: float, counts: Pcm) -> Complex:
    """Evaluate finite geometric sums stably, including the exact empty sum."""
    reduced = math.remainder(angle, 2.0 * np.pi)
    return (
        counts
        * np.sinc(counts * reduced / (2.0 * np.pi))
        / np.sinc(reduced / (2.0 * np.pi))
        * np.exp(-0.5j * (counts - 1.0) * reduced)
    )


def response(band: int, frequency: float, counts: Pcm) -> Complex:
    """Calculate partial FIR responses from three finite geometric sums."""
    angle = frequency - 2.0 * np.pi * band / BANK_FRAMES
    weight = 1.0 if band in {0, BAND_COUNT - 1} else 2.0
    return (
        weight
        / BANK_FRAMES
        * (
            0.5 * geometric(angle, counts)
            + 0.25 * geometric(angle - ALPHA, counts)
            + 0.25 * geometric(angle + ALPHA, counts)
        )
    )


def kernels() -> Complex:
    """Construct explicit FIRs independently of the renderer's FFT modulation."""
    offsets = np.arange(BANK_FRAMES)
    window = 0.5 + 0.5 * np.cos(ALPHA * offsets)
    window[-1] = 0.0
    result = np.exp(2.0j * np.pi * np.arange(BAND_COUNT)[:, None] * offsets / BANK_FRAMES)
    result *= window / BANK_FRAMES
    result[1:-1] *= 2.0
    result[[0, -1]] = result[[0, -1]].real
    return result


def response_controls(bank: Complex) -> dict:
    """Cross-check closed responses, real reconstruction and empty tap ranges."""
    error, closure = 0.0, 0.0
    offsets = np.arange(BANK_FRAMES)
    for frequency in (0.0, OMEGA, -OMEGA, np.pi, 2 * np.pi * 1100.0 / FS):
        calculated = np.array([
            response(k, frequency, np.array(BANK_FRAMES)) for k in range(BAND_COUNT)
        ])
        direct = bank @ np.exp(-1j * frequency * offsets)
        error = max(error, float(np.max(np.abs(calculated - direct))))
        opposite = bank @ np.exp(1j * frequency * offsets)
        closure = max(closure, float(abs(0.5 * np.sum(direct + np.conj(opposite)) - 1.0)))
    empty = max(abs(response(k, OMEGA, np.array(0.0))) for k in range(BAND_COUNT))
    assert error < 2.0e-11
    assert closure < 2.0e-11
    assert empty == 0.0
    return {
        "stage": "response_controls",
        "closed_vs_explicit_fir_max_abs_error": error,
        "real_bank_reconstruction_response_max_error": closure,
        "empty_partial_response_max_abs_value": float(empty),
        "oracle_parameters_only_in_analytic_controls": True,
    }


def predict_band(band: int, kernel: Complex, event: Pcm) -> Complex:
    """Predict both finite ramps and the exact finite flat-carrier interval."""
    counts = np.arange(BANK_FRAMES + 1)
    positive = response(band, OMEGA, counts)
    negative = response(band, -OMEGA, counts)
    upper = np.clip(CLOCK - RAMP + 1, 0, BANK_FRAMES)
    lower = np.clip(CLOCK - (EVENT_FRAMES - RAMP - 1), 0, BANK_FRAMES)
    carrier = PHASOR * np.exp(1j * OMEGA * CLOCK)
    result = 0.5 * (
        carrier * (positive[upper] - positive[lower])
        + np.conj(carrier) * (negative[upper] - negative[lower])
    )
    result[: RAMP + BANK_FRAMES - 1] += np.convolve(event[:RAMP], kernel)
    result[-(RAMP + BANK_FRAMES - 1) :] += np.convolve(event[-RAMP:], kernel)
    # Source endpoints and the last FIR tap are identically zero, independently of magnitude.
    result[0] = 0.0
    result[1] = event[1] * kernel[0]
    result[EVENT_FRAMES + BANK_FRAMES - 3 :] = 0.0
    if band in {0, BAND_COUNT - 1}:
        result = result.real.astype(np.complex128)
    return result


def winding_elapsed(values: Complex, carrier: float) -> tuple[Pcm, dict]:
    """Resolve integer phase branches independently from state arguments."""
    arguments = np.angle(values)
    active = values != 0.0
    connected = active[1:] & active[:-1]
    differences = np.diff(arguments) - carrier
    steps = -np.floor((differences + np.pi) / (2.0 * np.pi))
    windings = np.concatenate(([0.0], np.cumsum(np.where(connected, steps, 0.0))))
    unwrapped = arguments + 2.0 * np.pi * windings
    births = np.maximum.accumulate(np.where(active & ~np.r_[False, active[:-1]], CLOCK, 0))
    elapsed = unwrapped - unwrapped[births]
    elapsed[~active] = 0.0
    margins = np.abs((differences[connected] + np.pi) % (2.0 * np.pi) - np.pi)
    return elapsed, {
        "minimum_branch_cut_margin": float(np.min(np.pi - margins)),
        "integer_winding_steps": int(np.sum(np.abs(steps[connected]))),
        "minimum_nonzero_magnitude": float(np.min(np.abs(values[active]))),
    }


def direct_samples(event: Pcm, bank: Complex, positions: Pcm) -> Complex:
    """Evaluate explicit finite convolution at selected original source coordinates."""
    indices = positions[:, None] - np.arange(BANK_FRAMES)
    samples = np.zeros(indices.shape)
    inside = (indices >= 0) & (indices < len(event))
    samples[inside] = event[indices[inside]]
    return bank @ samples.T


def accumulate_band(prediction: Prediction, band: int, values: Complex) -> None:
    """Keep all raw transformed contributions and interference-energy evidence."""
    elapsed, evidence = winding_elapsed(values, 2.0 * np.pi * band / BANK_FRAMES)
    contribution = np.real(values[::2] * np.exp(1j * (Q[:, None] - 1) * elapsed[::2]))
    prediction.output += contribution
    prediction.reconstruction += values.real
    prediction.diagonal_energy += np.sum(contribution * contribution, axis=1)
    prediction.minimum_nonzero_magnitude = min(
        prediction.minimum_nonzero_magnitude, evidence["minimum_nonzero_magnitude"]
    )
    prediction.total_winding_steps += evidence["integer_winding_steps"]
    if band not in {0, BAND_COUNT - 1}:
        prediction.interior_branch_margin = min(
            prediction.interior_branch_margin, evidence["minimum_branch_cut_margin"]
        )
    # Deliberately incomplete surrogate: steady analytic phase with only principal filter phase.
    positions = 2 * np.arange(4096, 20_480)
    principal = OMEGA * positions + np.angle(PHASOR * response(band, OMEGA, np.array(BANK_FRAMES)))
    prediction.principal_surrogate += np.real(
        values[positions] * np.exp(1j * (Q[:, None] - 1) * principal)
    )


def predict(event: Pcm, bank: Complex) -> Prediction:
    """Predict full finite synthesis from ramps, images and onset winding."""
    prediction = Prediction()
    reference = direct_samples(event, bank, SAMPLE_POSITIONS)
    for band, kernel in enumerate(bank):
        values = predict_band(band, kernel, event)
        prediction.direct_state_error = max(
            prediction.direct_state_error,
            float(np.max(np.abs(values[SAMPLE_POSITIONS] - reference[band]))),
        )
        prediction.negative_image_dominant_bands += int(
            abs(response(band, -OMEGA, np.array(BANK_FRAMES)))
            > abs(response(band, OMEGA, np.array(BANK_FRAMES)))
        )
        accumulate_band(prediction, band, values)
    expected = np.pad(event, (0, BANK_FRAMES - 1))
    emit({
        "stage": "finite_prediction_integrity",
        "direct_state_max_abs_error": prediction.direct_state_error,
        "real_source_reconstruction_max_abs_error": float(
            np.max(np.abs(prediction.reconstruction - expected))
        ),
        "minimum_nonzero_band_magnitude": prediction.minimum_nonzero_magnitude,
        "minimum_interior_phase_branch_margin": prediction.interior_branch_margin,
        "total_integer_winding_steps": prediction.total_winding_steps,
        "negative_image_dominant_bands": prediction.negative_image_dominant_bands,
        "all_bands": BAND_COUNT,
        "whole_falling_ramp_and_filter_tail_included": True,
    })
    assert prediction.direct_state_error < 2.0e-11
    return prediction


def source_frequency(source: Pcm) -> dict:
    """Estimate a single-mode recurrence on a source-only maximum-energy window."""
    width = 16_384
    prefix = np.r_[0.0, np.cumsum(np.sum(source * source, axis=1))]
    start = int(np.argmax(prefix[width:] - prefix[:-width]))
    middle = source[start + 1 : start + width - 1]
    adjacent = source[start : start + width - 2] + source[start + 2 : start + width]
    cosine = float(np.sum(middle * adjacent) / (2.0 * np.sum(middle * middle)))
    assert -1.0 < cosine < 1.0, "Source recurrence does not define an interior real frequency"
    residual = adjacent - 2.0 * cosine * middle
    return {
        "estimated_hz": math.acos(cosine) * FS / (2.0 * np.pi),
        "source_window_begin": start,
        "source_window_frames": width,
        "relative_recurrence_residual": float(np.linalg.norm(residual) / np.linalg.norm(adjacent)),
        "renderer_received_estimate": False,
    }


def inverse_controls(bank: Complex, source: Pcm) -> dict:
    """Measure conditioned image inversion and retain an unresolved mixture residual."""
    estimate = source_frequency(source)
    frequency = estimate["estimated_hz"] * 2.0 * np.pi / FS
    basis = 0.5 * np.column_stack([
        [response(k, sign * frequency, np.array(BANK_FRAMES)) for k in range(BAND_COUNT)]
        for sign in (1, -1)
    ])
    event = source[SOURCE_TARGET : SOURCE_TARGET + EVENT_FRAMES, 0]
    observations = direct_samples(event, bank, STEADY_POSITIONS)
    phasors, _, _, singular = np.linalg.lstsq(basis, observations, rcond=None)
    expected = PHASOR * np.exp(1j * OMEGA * STEADY_POSITIONS)
    mixed = next(item for item in source_cases() if item.name == "poly_plus_impulse")
    mixed_event = mixed.source[SOURCE_TARGET : SOURCE_TARGET + EVENT_FRAMES, 0]
    mixed_observations = direct_samples(mixed_event, bank, STEADY_POSITIONS)
    mixed_fit = basis @ np.linalg.lstsq(basis, mixed_observations, rcond=None)[0]
    return {
        "stage": "image_inverse_controls",
        **estimate,
        "source_frequency_error_hz": estimate["estimated_hz"] - FREQUENCY,
        "stacked_image_matrix_condition_number": float(singular[0] / singular[-1]),
        "recovered_positive_mode_max_abs_error": float(np.max(np.abs(phasors[0] - expected))),
        "mono_out_of_span_relative_norm": float(
            np.linalg.norm(observations - basis @ phasors) / np.linalg.norm(observations)
        ),
        "mixed_out_of_span_relative_norm": float(
            np.linalg.norm(mixed_observations - mixed_fit) / np.linalg.norm(mixed_observations)
        ),
        "mixed_remainder_retained_in_measurement": True,
        "mixed_remainder_transformed_or_music_renderer_claim": False,
    }


def compare_actual(source: Pcm, prediction: Prediction) -> None:
    """Compare raw complete PCM/energy using original coordinates and no alignment."""
    source_energy = float(np.sum(source * source))
    errors = []
    for index, semitones in enumerate(SEMITONES):
        actual, details = render(source, 2.0, semitones)
        expected = np.zeros_like(actual)
        begin = SOURCE_TARGET // 2
        expected[begin : begin + prediction.output.shape[1], 0] = prediction.output[index]
        expected[:, 1] = 0.5 * expected[:, 0]
        predicted_energy = float(np.sum(expected * expected))
        actual_energy = float(np.sum(actual * actual))
        diagonal = float(1.25 * prediction.diagonal_energy[index])
        errors.append(float(np.max(np.abs(actual - expected))))
        emit({
            "stage": "actual_comparison",
            "semitones": semitones,
            "all_original_source_and_generated_frames": len(actual),
            "capture_origin": details["capture_origin"],
            "raw_predicted_vs_actual_max_abs_error": errors[-1],
            "predicted_output_over_source_energy": predicted_energy / source_energy,
            "actual_output_over_source_energy": actual_energy / source_energy,
            "energy_prediction_relative_error": abs(predicted_energy - actual_energy)
            / actual_energy,
            "sum_individual_band_energy": diagonal,
            "signed_cross_band_energy": predicted_energy - diagonal,
            "principal_filter_phase_surrogate_fixed_window_max_error": float(
                np.max(
                    np.abs(
                        actual[begin + 4096 : begin + 20_480, 0]
                        - prediction.principal_surrogate[index]
                    )
                )
            ),
            "fitted_gain_phase_delay_or_crop": False,
            "renderer_source_cues": details["cue_indices"],
            "full_product_acceptance": False,
        })
    emit({
        "stage": "summary",
        "whole_pcm_prediction_limit": 1.0e-8,
        "all_three_whole_pcm_predictions_within_limit": max(errors) <= 1.0e-8,
        "maximum_prediction_error": max(errors),
        "all_original_quality_negatives_remain": True,
        "full_product_gate_closed": False,
    })


def main() -> None:
    """Run frozen analytic calibration against the unchanged PCM-only renderer."""
    emit({
        "stage": "contract",
        "bank_frames": BANK_FRAMES,
        "band_count": BAND_COUNT,
        "original_mono_hz_analytic_oracle_only": FREQUENCY,
        "source_target": SOURCE_TARGET,
        "profiles": [(2, value) for value in SEMITONES],
        "finite_response_source_frames": SOURCE_COUNT,
        "phase_method": "independent state arguments plus explicit integer winding",
        "wet_parameter_fits": False,
        "native_or_production_changes": False,
    })
    source = next(item.source for item in source_cases() if item.name == "mono_900.37")
    event = source[SOURCE_TARGET : SOURCE_TARGET + EVENT_FRAMES, 0]
    bank = kernels()
    emit(response_controls(bank))
    prediction = predict(event, bank)
    emit(inverse_controls(bank, source))
    compare_actual(source, prediction)


if __name__ == "__main__":
    main()
