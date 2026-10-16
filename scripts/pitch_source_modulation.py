"""Fixed source-coordinate positive modulators for the offline AM experiment."""

from dataclasses import dataclass
from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from numpy.typing import NDArray
    from pitch_mode_carriers import CarrierEstimate

type Real = NDArray[np.float64]

FS = 48_000
BLOCK_FRAMES = 8192


@dataclass(frozen=True, slots=True)
class SourceModulator:
    """Represent 1 + 2*real(ratio*exp(2j*pi*frequency*source_frame/Fs))."""

    frequency_hz: float
    ratio: complex

    def __post_init__(self) -> None:
        object.__setattr__(self, "frequency_hz", float(self.frequency_hz))
        object.__setattr__(self, "ratio", complex(self.ratio))


def validate_modulators(frequencies: Real, modulators: tuple[SourceModulator, ...]) -> None:
    """Require finite positive source models with matching carrier dimensions."""
    if not isinstance(modulators, tuple) or len(modulators) != len(frequencies):
        message = "Source modulators must be a tuple matching every carrier"
        raise ValueError(message)
    for carrier, modulator in zip(frequencies, modulators, strict=True):
        if not isinstance(modulator, SourceModulator):
            message = "Each source modulator must have the declared immutable model"
            raise TypeError(message)
        try:
            frequency, ratio = float(modulator.frequency_hz), complex(modulator.ratio)
        except (ValueError, TypeError, OverflowError) as error:
            message = "Source modulation parameters must be finite real frequency/complex ratio"
            raise ValueError(message) from error
        if not all(np.isfinite(value) for value in (frequency, ratio.real, ratio.imag)):
            message = "Source modulation parameters must be finite"
            raise ValueError(message)
        if frequency < 0.0 or frequency >= FS / 2 or 2.0 * abs(ratio) >= 1.0:
            message = "Source modulation must have in-band frequency and strictly positive envelope"
            raise ValueError(message)
        if ratio != 0.0j and not (carrier - frequency > 0.0 and carrier + frequency < FS / 2):
            message = "Source modulation sidebands must remain strictly between DC and Nyquist"
            raise ValueError(message)


def modulation_values(positions: Real, modulators: tuple[SourceModulator, ...]) -> Real:
    """Evaluate fixed source phase on a bounded caller-supplied position block."""
    result = np.ones((len(positions), len(modulators)))
    for index, modulator in enumerate(modulators):
        if modulator.ratio != 0.0j:
            phase = 2.0 * np.pi * modulator.frequency_hz / FS * positions
            result[:, index] += (
                2.0 * abs(modulator.ratio) * np.cos(phase + np.angle(modulator.ratio))
            )
    return result


def apply_source_modulation(envelopes: Real, modulators: tuple[SourceModulator, ...]) -> None:
    """Publish physical envelopes in place without a full-source modulation copy."""
    for start in range(0, len(envelopes), BLOCK_FRAMES):
        stop = min(start + BLOCK_FRAMES, len(envelopes))
        positions = np.arange(start, stop, dtype=np.float64)
        envelopes[start:stop] *= modulation_values(positions, modulators)


def modulation_workspace_bytes(frames: int, carriers: int) -> int:
    """Include blocked modulation, tile temporaries and physical-norm summaries."""
    knots = (frames + 63) // 64 + 1
    return BLOCK_FRAMES * (8 * carriers + 64) + knots * 64 * carriers + 4096


def modulation_contract(modulators: tuple[SourceModulator, ...]) -> dict:
    """Distinguish fitted common amplitudes from published physical envelopes."""
    return {
        "source_modulated_fitting_basis": True,
        "source_modulator_global_phase_origin_frame": 0,
        "source_modulator_formula": "1+2*real(u*exp(2j*pi*nu*source_frame/Fs))",
        "source_modulators": [
            {"frequency_hz": value.frequency_hz, "ratio": [value.ratio.real, value.ratio.imag]}
            for value in modulators
        ],
        "affine_endpoint_and_slope_quantity": "common amplitude before source modulation",
        "unconstrained_prediction_row_norm_quantity": "common amplitude before source modulation",
        "published_envelope_quantity": "nonnegative common prediction times source modulator",
        "envelope_interpolation": (
            "positive source modulation times nonnegative affine common-amplitude prediction"
        ),
        "published_affine_endpoint": "current common-amplitude R and gate slope before modulation",
        "source_modulation_applied_once_to_published_envelopes": True,
        "full_source_modulation_array_allocated": False,
        "modulator_is_fixed_before_local_fit": True,
        "global_discovery_or_complete_pipeline_causality_claim": False,
    }


def modulators_from_estimate(estimate: CarrierEstimate) -> tuple[SourceModulator, ...] | None:
    """Read only already selected source certificates, never output/scoring data."""
    grouping = estimate.report.get("am_grouping")
    if grouping is None or not grouping["accepted_groups"]:
        return None
    modulators = [SourceModulator(0.0, 0.0j) for _ in estimate.frequencies_hz]
    for group in grouping["accepted_groups"]:
        certificates = [
            value
            for value in grouping["candidates"]
            if value["grouped"] and value["original_indices"] == group["original_indices"]
        ]
        if len(certificates) != 1:
            message = "A selected source family requires one unambiguous modulation certificate"
            raise ValueError(message)
        ratio = complex(*certificates[0]["global_positive_sideband_ratio"])
        index = estimate.frequencies_hz.index(group["center_frequency_hz"])
        modulators[index] = SourceModulator(group["modulation_frequency_hz"], ratio)
    result = tuple(modulators)
    validate_modulators(np.asarray(estimate.frequencies_hz), result)
    return result
