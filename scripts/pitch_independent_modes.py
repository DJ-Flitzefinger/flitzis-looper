"""Frozen source-mode/envelope diagnostic with an entirely transformed residual.

The source-only estimators are independent of requested rate, pitch and scorer
targets. This NumPy experiment is not a productive or realtime backend.
"""

import math
from dataclasses import dataclass
from functools import partial
from typing import TYPE_CHECKING

import numpy as np
from pitch_am_groups import group_am_carriers
from pitch_amfm_model import render as render_residual
from pitch_chirplet_carriers import estimate_chirplet
from pitch_finite_am_gate import fit_finite_am_gate
from pitch_joint_finite_gates import fit_joint_finite_gates
from pitch_mode_carriers import FS, CarrierEstimate, estimate_carriers
from pitch_mode_certificate import inspect_window
from pitch_mode_envelopes import (
    fit_affine_causal_envelopes,
    fit_causal_envelopes,
    fit_current_weight_causal_envelopes,
    fit_envelopes,
    fit_modulated_current_weight_envelopes,
    fit_phase_current_weight_envelopes,
    fit_predictive_causal_envelopes,
)
from pitch_source_modulation import apply_source_modulation, modulators_from_estimate

if TYPE_CHECKING:
    from numpy.typing import NDArray
    from pitch_source_modulation import SourceModulator
    from pitch_source_phase import LinearSourcePhase

type Pcm = NDArray[np.float64]
type ComplexPcm = NDArray[np.complex128]


class UnsupportedPitchDomainError(ValueError):
    """Reject known admitted carriers outside the representable output interval."""


@dataclass(frozen=True, slots=True)
class Mode:
    """Retain one source-derived phase owner and its full analytic PCM."""

    frequency_hz: float
    birth: int
    analytic_pcm: ComplexPcm
    phase_law: LinearSourcePhase | None = None


@dataclass(frozen=True, slots=True)
class Prepared:
    """Share one source-only decomposition across independent requested profiles."""

    source: Pcm
    modes: tuple[Mode, ...]
    residual: Pcm
    report: dict


def _validate_mode_policy(
    *,
    certified_causal: bool,
    affine_endpoint: bool,
    predictive_envelope: bool,
    current_weight: bool,
    am_family: bool,
    modulated_basis: bool,
    linear_chirplet: bool = False,
    finite_am_gate: bool = False,
    joint_finite_gates: bool = False,
) -> None:
    if affine_endpoint and not certified_causal:
        message = "Affine endpoints require certified source-mode preparation"
        raise ValueError(message)
    if predictive_envelope and not (certified_causal and affine_endpoint):
        message = "Predictive envelopes require certified affine endpoint preparation"
        raise ValueError(message)
    if current_weight and not (certified_causal and affine_endpoint and predictive_envelope):
        message = "Current weighting requires certified affine predictive preparation"
        raise ValueError(message)
    if am_family and not current_weight:
        message = "AM families require the certified current-weight preparation"
        raise ValueError(message)
    if modulated_basis and not am_family:
        message = "A fixed modulated basis requires certified positive-AM families"
        raise ValueError(message)
    if linear_chirplet and not modulated_basis:
        message = "Linear chirplets require the complete certified v7 preparation policy"
        raise ValueError(message)
    if finite_am_gate and not linear_chirplet:
        message = "Finite AM gates require the complete certified v8 preparation policy"
        raise ValueError(message)
    if joint_finite_gates and not finite_am_gate:
        message = "Joint finite gates require the complete certified v9 preparation policy"
        raise ValueError(message)


def _fit_finite_am_source(
    pcm: Pcm, estimate: CarrierEstimate, modulators: tuple[SourceModulator, ...] | None
) -> tuple[CarrierEstimate, Pcm, dict] | None:
    """Use only an existing unique isolated source-family certificate and reference."""
    grouping = estimate.report.get("am_grouping")
    if (
        len(estimate.frequencies_hz) != 1
        or modulators is None
        or grouping is None
        or grouping["group_count"] != 1
    ):
        estimate.report["finite_am_gate"] = {
            "admitted": False,
            "rejection": "requires_one_certified_AM_family_without_other_retained_carriers",
        }
        return None
    certificates = [candidate for candidate in grouping["candidates"] if candidate["grouped"]]
    if len(certificates) != 1 or certificates[0]["reference_start"] is None:
        message = "A finite gate requires one existing source reference certificate"
        raise ValueError(message)
    fitted = fit_finite_am_gate(
        pcm,
        estimate.frequencies_hz[0],
        estimate.stereo_coefficients[0],
        modulators[0],
        certificates[0]["reference_start"],
    )
    estimate.report["finite_am_gate"] = fitted.report
    if fitted.gate is None:
        return None
    amplitude = abs(fitted.multiplier)
    coefficients = estimate.stereo_coefficients * (fitted.multiplier / amplitude)
    updated = CarrierEstimate(estimate.frequencies_hz, coefficients, estimate.report)
    envelopes = np.empty((len(pcm), 1))
    for start in range(0, len(pcm), 8192):
        stop = min(start + 8192, len(pcm))
        envelopes[start:stop, 0] = amplitude * fitted.gate.values(np.arange(start, stop))
    apply_source_modulation(envelopes, modulators)
    report = {
        **fitted.report,
        "source_finite_am_gate": True,
        "offline_fit": True,
        "offline_noncausal_fit": True,
        "envelope_fit_local_causal": False,
        "complete_pipeline_causality_claim": False,
        "source_modulation_applied_once_to_published_envelopes": True,
        "source_model_phase_birth_frame": fitted.gate.onset_frame,
        "published_envelope_quantity": (
            "conditional finite gate times amplitude and source modulator"
        ),
        "fitted_endpoints_crop_exact_residual": False,
    }
    return updated, envelopes, report


def _fit_joint_source(
    pcm: Pcm, estimate: CarrierEstimate, *, stationary: bool
) -> tuple[CarrierEstimate, Pcm, dict] | None:
    """Reuse source-certified frequencies; discover gates and sparse support from PCM."""
    starts = estimate.report.get("refinement", {}).get("starts", [])
    if len(estimate.frequencies_hz) != 2 or not stationary or not starts:
        estimate.report["joint_finite_gates"] = {
            "accepted": False,
            "refusal_reason": "requires_two_stationary_carriers_and_source_reference",
        }
        return None
    fitted = fit_joint_finite_gates(
        pcm,
        np.asarray(estimate.frequencies_hz),
        estimate.stereo_coefficients,
        min(starts),
    )
    estimate.report["joint_finite_gates"] = fitted.report
    if fitted.gates is None:
        return None
    amplitude = np.abs(fitted.multipliers)
    coefficients = estimate.stereo_coefficients * (fitted.multipliers / amplitude)[:, None]
    updated = CarrierEstimate(estimate.frequencies_hz, coefficients, estimate.report)
    positions = np.arange(len(pcm))
    envelopes = np.column_stack([
        value * gate.values(positions) for value, gate in zip(amplitude, fitted.gates, strict=True)
    ])
    return (
        updated,
        envelopes,
        {
            **fitted.report,
            "source_joint_finite_gates": True,
            "offline_fit": True,
            "envelope_fit_local_causal": False,
            "complete_pipeline_causality_claim": False,
            "source_model_phase_birth_frames": [gate.onset_frame for gate in fitted.gates],
            "published_envelope_quantity": (
                "independent finite gates times joint carrier amplitudes"
            ),
            "fitted_endpoints_crop_exact_residual": False,
        },
    )


def _select_envelope_fit(
    *,
    certified_causal: bool,
    affine_endpoint: bool,
    predictive_envelope: bool,
    current_weight: bool,
    modulators: tuple[SourceModulator, ...] | None,
    phase_laws: tuple[LinearSourcePhase, ...] | None,
):
    """Keep one authoritative dispatch for the existing envelope policies."""
    if phase_laws is not None:
        return partial(fit_phase_current_weight_envelopes, phase_laws=phase_laws)
    if modulators is not None:
        return partial(fit_modulated_current_weight_envelopes, modulators=modulators)
    if current_weight:
        return fit_current_weight_causal_envelopes
    if predictive_envelope:
        return fit_predictive_causal_envelopes
    if affine_endpoint:
        return fit_affine_causal_envelopes
    return fit_causal_envelopes if certified_causal else fit_envelopes


def _fit_source_modes(
    pcm: Pcm,
    *,
    certified_causal: bool,
    affine_endpoint: bool,
    predictive_envelope: bool,
    current_weight: bool,
    am_family: bool,
    modulated_basis: bool,
    linear_chirplet: bool = False,
    finite_am_gate: bool = False,
    joint_finite_gates: bool = False,
) -> tuple[CarrierEstimate, Pcm, dict, tuple[LinearSourcePhase, ...] | None]:
    _validate_mode_policy(
        certified_causal=certified_causal,
        affine_endpoint=affine_endpoint,
        predictive_envelope=predictive_envelope,
        current_weight=current_weight,
        am_family=am_family,
        modulated_basis=modulated_basis,
        linear_chirplet=linear_chirplet,
        finite_am_gate=finite_am_gate,
        joint_finite_gates=joint_finite_gates,
    )
    estimate = estimate_carriers(
        pcm,
        window_inspector=partial(inspect_window, geometry="spread") if certified_causal else None,
    )
    if am_family:
        estimate = group_am_carriers(pcm, estimate)
    phase_laws = None
    if linear_chirplet and not estimate.frequencies_hz:
        chirplet = estimate_chirplet(pcm)
        estimate.report["linear_chirplet"] = chirplet.report
        if chirplet.phase is not None:
            phase_laws = (chirplet.phase,)
            estimate = CarrierEstimate(
                (chirplet.phase.frequency_hz,), chirplet.stereo_coefficients, estimate.report
            )
    modulators = modulators_from_estimate(estimate) if modulated_basis else None
    if finite_am_gate:
        finite = _fit_finite_am_source(pcm, estimate, modulators)
        if finite is not None:
            return *finite, phase_laws
    if joint_finite_gates:
        joint = _fit_joint_source(
            pcm, estimate, stationary=modulators is None and phase_laws is None
        )
        if joint is not None:
            return *joint, phase_laws
    fit = _select_envelope_fit(
        certified_causal=certified_causal,
        affine_endpoint=affine_endpoint,
        predictive_envelope=predictive_envelope,
        current_weight=current_weight,
        modulators=modulators,
        phase_laws=phase_laws,
    )
    envelopes, report = fit(
        pcm, np.asarray(estimate.frequencies_hz, dtype=np.float64), estimate.stereo_coefficients
    )
    return estimate, envelopes, report, phase_laws


def _model_version(
    *,
    certified_causal: bool,
    affine_endpoint: bool,
    predictive_envelope: bool,
    current_weight: bool,
    am_family: bool,
    modulated_basis: bool,
    linear_chirplet: bool = False,
    finite_am_gate: bool = False,
    joint_finite_gates: bool = False,
) -> str:
    for enabled, version in (
        (joint_finite_gates, "conditional-independent-source-gates-v10"),
        (finite_am_gate, "conditional-source-finite-AM-gate-v9"),
        (linear_chirplet, "certified-source-linear-chirplet-v8"),
        (modulated_basis, "certified-fixed-positive-am-basis-v7"),
    ):
        if enabled:
            return version
    if am_family:
        return "certified-positive-am-families-v6"
    if current_weight:
        return "certified-current-weight-prediction-v5"
    if predictive_envelope:
        return "certified-affine-prediction-v4"
    if affine_endpoint:
        return "certified-affine-endpoints-v3"
    return "certified-causal-envelopes-v2" if certified_causal else "v1"


def _build_mode(
    frequency: float,
    birth: int,
    envelope: Pcm,
    coefficients: ComplexPcm,
    clock: Pcm,
    phase_law: LinearSourcePhase | None,
) -> Mode:
    if phase_law is None:
        carrier = envelope * np.exp(2.0j * np.pi * frequency / FS * clock)
    else:
        carrier = envelope * np.exp(2.0j * np.pi * phase_law.cycles(clock))
    analytic = carrier[:, None] * coefficients
    analytic.setflags(write=False)
    return Mode(float(frequency), birth, analytic, phase_law)


def prepare(
    source: Pcm,
    *,
    certified_causal: bool = False,
    affine_endpoint: bool = False,
    predictive_envelope: bool = False,
    current_weight: bool = False,
    am_family: bool = False,
    modulated_basis: bool = False,
    linear_chirplet: bool = False,
    finite_am_gate: bool = False,
    joint_finite_gates: bool = False,
) -> Prepared:
    """Decompose all original stereo samples without access to output intent."""
    if np.iscomplexobj(source):
        message = "Source must be real stereo PCM"
        raise ValueError(message)
    pcm = np.array(source, dtype=np.float64, copy=True)
    if pcm.ndim != 2 or pcm.shape[1] != 2 or not np.all(np.isfinite(pcm)):
        message = "Source must be finite stereo PCM"
        raise ValueError(message)
    estimate, envelopes, envelope_report, phase_laws = _fit_source_modes(
        pcm,
        certified_causal=certified_causal,
        affine_endpoint=affine_endpoint,
        predictive_envelope=predictive_envelope,
        current_weight=current_weight,
        am_family=am_family,
        modulated_basis=modulated_basis,
        linear_chirplet=linear_chirplet,
        finite_am_gate=finite_am_gate,
        joint_finite_gates=joint_finite_gates,
    )
    activity = np.flatnonzero(np.any(pcm != 0.0, axis=1))
    birth = int(activity[0]) if len(activity) else 0
    modes, reconstruction = [], np.zeros_like(pcm)
    clock = np.arange(len(pcm), dtype=np.float64)
    births = envelope_report.get(
        "source_model_phase_birth_frames",
        [int(envelope_report.get("source_model_phase_birth_frame", birth))]
        * len(estimate.frequencies_hz),
    )
    for index, frequency in enumerate(estimate.frequencies_hz):
        mode = _build_mode(
            frequency,
            int(births[index]),
            envelopes[:, index],
            estimate.stereo_coefficients[index],
            clock,
            phase_laws[index] if phase_laws is not None else None,
        )
        reconstruction += mode.analytic_pcm.real
        modes.append(mode)
    residual = pcm - reconstruction
    report = {
        "model_version": _model_version(
            certified_causal=certified_causal,
            affine_endpoint=affine_endpoint,
            predictive_envelope=predictive_envelope,
            current_weight=current_weight,
            am_family=am_family,
            modulated_basis=modulated_basis,
            linear_chirplet=linear_chirplet,
            finite_am_gate=finite_am_gate,
            joint_finite_gates=joint_finite_gates,
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
    if phase_laws is not None:
        report["source_phase_laws"] = [
            {
                "epoch_source_frame": law.epoch_frame,
                "epoch_frequency_hz": law.frequency_hz,
                "slope_hz_per_second": law.slope_hz_per_second,
            }
            for law in phase_laws
        ]
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


def _chirplet_output_bounds(prepared: Prepared, shift: float) -> list[dict]:
    """Validate phase over every fitted sample and its zero-extended interpolation support."""
    bounds = []
    for mode in prepared.modes:
        if mode.phase_law is None:
            continue
        active = np.flatnonzero(np.any(mode.analytic_pcm != 0.0, axis=1))
        if not len(active):
            continue
        support = np.array([float(active[0]) - 1.0, float(active[-1]) + 1.0])
        frequencies = mode.phase_law.frequency_at(support)
        lower, upper = float(np.min(frequencies)), float(np.max(frequencies))
        if lower <= 0.0 or upper >= FS / 2 or upper * shift >= FS / 2:
            message = (
                f"Known chirplet frequency interval [{lower}, {upper}] Hz over complete "
                f"fitted/interpolation support at pitch ratio {shift} crosses DC or Nyquist"
            )
            raise UnsupportedPitchDomainError(message)
        bounds.append({
            "source_interpolation_support_inclusive": support.tolist(),
            "source_frequency_interval_hz": [lower, upper],
            "pitched_frequency_interval_hz": [lower * shift, upper * shift],
        })
    return bounds


def render_prepared(prepared: Prepared, rate: float, semitones: int) -> tuple[Pcm, dict]:
    """Transform every mode and residual sample on one declared source clock."""
    shift = 2.0 ** (semitones / 12.0)
    if not math.isfinite(rate) or rate <= 0.0 or not math.isfinite(shift) or shift <= 0.0:
        message = "Finite positive rate and pitch are required"
        raise ValueError(message)
    unsupported = [
        mode.frequency_hz for mode in prepared.modes if mode.frequency_hz * shift >= FS / 2
    ]
    grouping = prepared.report.get("carrier_estimation", {}).get("am_grouping")
    am_sideband_crossings = []
    if grouping is not None:
        # Preserve every previously explicit source-carrier refusal. AM envelopes additionally
        # follow source rate, so their known modulation bandwidth is not multiplied by h.
        unsupported = sorted(
            set(unsupported)
            | {
                frequency
                for frequency in grouping["original_frequencies_hz"]
                if frequency * shift >= FS / 2
            }
        )
        am_sideband_crossings = [
            group["center_frequency_hz"] * shift + group["modulation_frequency_hz"] * rate
            for group in grouping["accepted_groups"]
            if group["center_frequency_hz"] * shift + group["modulation_frequency_hz"] * rate
            >= FS / 2
        ]
    if unsupported:
        message = f"Known carriers {unsupported} at pitch ratio {shift} reach or exceed Nyquist"
        raise UnsupportedPitchDomainError(message)
    if am_sideband_crossings:
        message = f"Known source-clock AM sidebands {am_sideband_crossings} reach or exceed Nyquist"
        raise UnsupportedPitchDomainError(message)
    chirplet_bounds = _chirplet_output_bounds(prepared, shift)
    residual, details = render_residual(prepared.residual, rate, semitones)
    origin = details["capture_origin"]
    positions = rate * (origin + np.arange(len(residual), dtype=np.float64))
    output = residual.copy()
    mode_energy = 0.0
    for mode in prepared.modes:
        if mode.phase_law is None:
            correction = np.exp(
                2.0j
                * np.pi
                * mode.frequency_hz
                / FS
                * (shift / rate - 1.0)
                * (positions - mode.birth)
            )
        else:
            correction = np.exp(
                1.0j * (shift / rate - 1.0) * mode.phase_law.phase_difference(positions, mode.birth)
            )
        contribution = np.real(sample_complex(mode.analytic_pcm, positions) * correction[:, None])
        mode_energy += float(np.sum(contribution * contribution))
        output += contribution
    assert np.all(np.isfinite(output)), "Nonfinite complete mode/residual output"
    report = {
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
    if grouping is not None:
        report["am_group_count"] = grouping["group_count"]
        report["original_carrier_nyquist_refusals_preserved"] = True
        report["known_source_clock_am_sideband_nyquist_guard"] = True
    if any(mode.phase_law is not None for mode in prepared.modes):
        report["source_chirplet_phase_correction"] = "(h/r-1)*(theta(s)-theta(b))"
        report["chirplet_complete_support_frequency_bounds"] = chirplet_bounds
        report["chirplet_fractional_interpolation_equals_analytic_phase"] = False
    return output, report


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
