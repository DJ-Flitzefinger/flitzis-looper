"""Frozen source-mode/envelope diagnostic with an entirely transformed residual.

The source-only estimators are independent of requested rate, pitch and scorer
targets. This NumPy experiment is not a productive or realtime backend.
"""

import math
from dataclasses import dataclass
from functools import partial
from typing import TYPE_CHECKING

import numpy as np
from pitch_amfm_model import render as render_residual
from pitch_mode_carriers import FS, estimate_carriers
from pitch_mode_certificate import inspect_window
from pitch_mode_envelopes import fit_affine_causal_envelopes, fit_causal_envelopes, fit_envelopes

if TYPE_CHECKING:
    from numpy.typing import NDArray
    from pitch_mode_carriers import CarrierEstimate

type Pcm = NDArray[np.float64]
type ComplexPcm = NDArray[np.complex128]


class UnsupportedPitchDomainError(ValueError):
    """Reject known admitted carriers outside the representable output interval."""


@dataclass(frozen=True, slots=True)
class Mode:
    """Retain one source-derived stationary carrier and its full analytic PCM."""

    frequency_hz: float
    birth: int
    analytic_pcm: ComplexPcm


@dataclass(frozen=True, slots=True)
class Prepared:
    """Share one source-only decomposition across independent requested profiles."""

    source: Pcm
    modes: tuple[Mode, ...]
    residual: Pcm
    report: dict


def _fit_source_modes(
    pcm: Pcm, *, certified_causal: bool, affine_endpoint: bool
) -> tuple[CarrierEstimate, Pcm, dict]:
    if affine_endpoint and not certified_causal:
        message = "Affine endpoints require certified source-mode preparation"
        raise ValueError(message)
    estimate = estimate_carriers(
        pcm,
        window_inspector=partial(inspect_window, geometry="spread") if certified_causal else None,
    )
    fit = (
        fit_affine_causal_envelopes
        if affine_endpoint
        else (fit_causal_envelopes if certified_causal else fit_envelopes)
    )
    envelopes, report = fit(
        pcm, np.asarray(estimate.frequencies_hz, dtype=np.float64), estimate.stereo_coefficients
    )
    return estimate, envelopes, report


def prepare(
    source: Pcm, *, certified_causal: bool = False, affine_endpoint: bool = False
) -> Prepared:
    """Decompose all original stereo samples without access to output intent."""
    if np.iscomplexobj(source):
        message = "Source must be real stereo PCM"
        raise ValueError(message)
    pcm = np.array(source, dtype=np.float64, copy=True)
    if pcm.ndim != 2 or pcm.shape[1] != 2 or not np.all(np.isfinite(pcm)):
        message = "Source must be finite stereo PCM"
        raise ValueError(message)
    estimate, envelopes, envelope_report = _fit_source_modes(
        pcm, certified_causal=certified_causal, affine_endpoint=affine_endpoint
    )
    activity = np.flatnonzero(np.any(pcm != 0.0, axis=1))
    birth = int(activity[0]) if len(activity) else 0
    modes, reconstruction = [], np.zeros_like(pcm)
    clock = np.arange(len(pcm), dtype=np.float64)
    for index, frequency in enumerate(estimate.frequencies_hz):
        carrier = envelopes[:, index] * np.exp(2.0j * np.pi * frequency / FS * clock)
        analytic = carrier[:, None] * estimate.stereo_coefficients[index]
        reconstruction += analytic.real
        analytic.setflags(write=False)
        modes.append(Mode(float(frequency), birth, analytic))
    residual = pcm - reconstruction
    report = {
        "model_version": (
            "certified-affine-endpoints-v3"
            if affine_endpoint
            else ("certified-causal-envelopes-v2" if certified_causal else "v1")
        ),
        "mode_count": len(modes),
        "frequencies_hz": list(estimate.frequencies_hz),
        "source_birth_frame": birth,
        "carrier_estimation": estimate.report,
        "envelope_fit": envelope_report,
        "complete_decomposition_max_abs_error": float(
            np.max(np.abs(reconstruction + residual - pcm), initial=0.0)
        ),
        "source_energy": float(np.sum(pcm * pcm)),
        "residual_source_energy": float(np.sum(residual * residual)),
        "fitted_wet_parameters_or_scorer_metadata": False,
        "all_residual_samples_will_be_transformed": True,
    }
    pcm.setflags(write=False)
    residual.setflags(write=False)
    return Prepared(pcm, tuple(modes), residual, report)


def sample_complex(values: ComplexPcm, positions: Pcm) -> ComplexPcm:
    """Keep the entire zero-extended linear source interpolation support."""
    padded = np.vstack((np.zeros((1, 2)), values, np.zeros((1, 2))))
    knots = np.arange(-1, len(values) + 1)
    return np.column_stack([
        np.interp(positions, knots, padded[:, channel], left=0.0, right=0.0) for channel in range(2)
    ])


def render_prepared(prepared: Prepared, rate: float, semitones: int) -> tuple[Pcm, dict]:
    """Transform every mode and residual sample on one declared source clock."""
    shift = 2.0 ** (semitones / 12.0)
    if not math.isfinite(rate) or rate <= 0.0 or not math.isfinite(shift) or shift <= 0.0:
        message = "Finite positive rate and pitch are required"
        raise ValueError(message)
    unsupported = [
        mode.frequency_hz for mode in prepared.modes if mode.frequency_hz * shift >= FS / 2
    ]
    if unsupported:
        message = f"Known carriers {unsupported} at pitch ratio {shift} reach or exceed Nyquist"
        raise UnsupportedPitchDomainError(message)
    residual, details = render_residual(prepared.residual, rate, semitones)
    origin = details["capture_origin"]
    positions = rate * (origin + np.arange(len(residual), dtype=np.float64))
    output = residual.copy()
    mode_energy = 0.0
    for mode in prepared.modes:
        correction = np.exp(
            2.0j * np.pi * mode.frequency_hz / FS * (shift / rate - 1.0) * (positions - mode.birth)
        )
        contribution = np.real(sample_complex(mode.analytic_pcm, positions) * correction[:, None])
        mode_energy += float(np.sum(contribution * contribution))
        output += contribution
    assert np.all(np.isfinite(output)), "Nonfinite complete mode/residual output"
    return output, {
        "model": "source-derived-independent-modes-with-complete-amfm-residual-"
        + prepared.report.get("model_version", "v1"),
        "capture_origin": origin,
        "mode_count": len(prepared.modes),
        "source_estimated_frequencies_hz": prepared.report.get("frequencies_hz", []),
        "complete_decomposition_max_abs_error": prepared.report.get(
            "complete_decomposition_max_abs_error", 0.0
        ),
        "fixed_mode_births_source_frames": [mode.birth for mode in prepared.modes],
        "mode_birth_reset_by_unrelated_cue": False,
        "source_energy": float(np.sum(prepared.source * prepared.source)),
        "residual_source_energy": float(np.sum(prepared.residual * prepared.residual)),
        "transformed_residual_output_energy": float(np.sum(residual * residual)),
        "sum_individual_mode_output_energies": mode_energy,
        "residual_renderer_complete_frames": len(residual),
        "residual_processed_bands": details["processed_bands"],
        "residual_full_bank_reconstruction_error": details["maximum_complete_bank_error"],
        "all_modes_and_residual_tails_retained": True,
        "source_only_preparation_independent_of_pitch_rate": True,
        "interpolation": "full zero-extended linear; h=r combined-source identity",
        "mode_nyquist_rejection": "explicit when an admitted carrier*h >= Fs/2",
        "unresolved_residual_aliasing_or_physical_mode_semantics_certified": False,
        "fitted_output_gain_phase_delay_crop": False,
        "native_or_production_or_realtime_acceptance": False,
    }


def render(source: Pcm, rate: float, semitones: int) -> tuple[Pcm, dict]:
    """Prepare and render without any source-target or output-scoring metadata."""
    return render_prepared(prepare(source), rate, semitones)


def integrity_controls() -> dict:
    """Check exact complementary reconstruction and independent mode phase math."""
    try:
        prepare(np.ones((2, 2), dtype=np.complex128))
    except ValueError:
        pass
    else:
        message = "Complex source PCM must be rejected before conversion"
        raise AssertionError(message)
    count, frequency = 257, 749.43
    clock = np.arange(count)
    envelope = np.sin(np.pi * clock / (count - 1)) ** 2
    coefficients = np.array([np.exp(0.17j), 0.6 * np.exp(0.53j)])
    analytic = (
        envelope[:, None] * np.exp(2.0j * np.pi * frequency / FS * clock[:, None]) * coefficients
    )
    mode = Mode(frequency, 0, analytic)
    source = analytic.real.copy()
    prepared = Prepared(source, (mode,), np.zeros_like(source), {})
    actual, _ = render_prepared(prepared, 2.0, 7)
    expected = envelope[::2, None] * np.real(
        coefficients
        * np.exp(2.0j * np.pi * frequency / FS * 2.0 ** (7 / 12) * np.arange(len(actual))[:, None])
    )
    phase_error = float(np.max(np.abs(actual - expected)))
    fractional_error = _fractional_control(mode, source)
    assert phase_error < 1.0e-9
    assert fractional_error < 1.0e-9
    return {
        "stage": "mode_pipeline_integrity",
        "known_nonunity_mode_phase_max_abs_error": phase_error,
        "complete_mode_plus_nonzero_residual_fractional_identity_error": fractional_error,
        "complex_source_rejected_before_real_conversion": True,
        "oracle_parameters_only_in_controls": True,
    }


def _fractional_control(mode: Mode, source: Pcm) -> float:
    noise = 0.03 * np.random.default_rng(91731).normal(size=source.shape)
    prepared = Prepared(source + noise, (mode,), noise, {})
    actual, details = render_prepared(prepared, 0.5, -12)
    coordinates = 0.5 * (details["capture_origin"] + np.arange(len(actual)))
    expected = np.zeros_like(actual)
    for index, position in enumerate(coordinates):
        lower = math.floor(position)
        fraction = position - lower
        if 0 <= lower < len(source):
            expected[index] += (1 - fraction) * prepared.source[lower]
        if 0 <= lower + 1 < len(source):
            expected[index] += fraction * prepared.source[lower + 1]
    return float(np.max(np.abs(actual - expected)))
