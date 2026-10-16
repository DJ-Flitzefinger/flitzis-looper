"""Offline AR2 falsification probe; no Native, production, device, or file access.

This LPC simplification is NOT the complete Kalman/filterbank algorithm in:
https://dafx.de/paper-archive/2003/pdfs/dafx12.pdf (sections 2--3).
Its causal innovations can preserve onset support while its tonal model fails.
"""

import json
import math
import sys
from typing import TYPE_CHECKING

import numpy as np
from numpy.typing import NDArray

if TYPE_CHECKING:
    from collections.abc import Iterator, Sequence

Pcm = NDArray[np.float64]

FS = 48_000
SOURCE_FRAMES = 161_538
SOURCE_TARGET = 32_768
EVENT_FRAMES = 48_000
H = 16_384
WINDOW_START, WINDOW_END = 4096, 20_480
CENTS_LIMIT = 15.0
IDENTITY_LIMIT = 1.0e-9
PROFILES = ((2, -7), (2, 0), (2, 7), (1, 0))


def emit(record: dict) -> None:
    """Write one finite JSON record to the diagnostic's standard output."""
    sys.stdout.write(json.dumps(record, sort_keys=True, allow_nan=False) + "\n")
    sys.stdout.flush()


def fixtures() -> Iterator[tuple[str, Pcm, tuple[float, ...]]]:
    """Generate the frozen stereo sources without observing any output."""
    time = np.arange(EVENT_FRAMES, dtype=np.float64) / FS
    ramp_frames = FS * 2 // 1000
    ramp = 0.5 * (1.0 - np.cos(np.pi * np.arange(ramp_frames) / (ramp_frames - 1)))
    gate = np.ones(EVENT_FRAMES)
    gate[:ramp_frames], gate[-ramp_frames:] = ramp, ramp[::-1]
    for name, frequencies, amplitudes, phases, impulse in (
        ("impulse", (), (), (), 1.0),
        ("mono_900.37", (900.37,), (0.25,), (0.37,), 0.0),
        ("poly_plus_impulse", (331.37, 997.19), (0.22, 0.17), (0.37, 1.1), 0.8),
    ):
        left = np.zeros(SOURCE_FRAMES)
        for frequency, amplitude, phase in zip(frequencies, amplitudes, phases, strict=True):
            left[SOURCE_TARGET : SOURCE_TARGET + EVENT_FRAMES] += (
                amplitude * np.sin(2.0 * np.pi * frequency * time + phase) * gate
            )
        left[SOURCE_TARGET] += impulse
        yield name, np.column_stack((left, 0.5 * left)), frequencies


def fit(source: Pcm) -> dict:
    """Fit one pooled AR2 model and report unsupported poles explicitly."""
    denominator = source.size  # Biased autocorrelation: same denominator for all lags.
    correlations = [
        float(np.sum(source[lag:] * source[: SOURCE_FRAMES - lag]) / denominator)
        for lag in range(3)
    ]
    r0, r1, r2 = correlations
    report = {"stereo_pooled_biased_autocorrelation": correlations}
    try:
        a1, a2 = np.linalg.solve([[r0, r1], [r1, r0]], [-r1, -r2]).tolist()
    except np.linalg.LinAlgError:
        return {**report, "rejection": "singular Yule-Walker system"}
    report.update(a1=a1, a2=a2)
    if not all(math.isfinite(value) for value in (a1, a2)):
        return {**report, "rejection": "nonfinite coefficients"}
    if a1 == 0.0 and a2 == 0.0:
        return {**report, "mode": "zero-mode/residual", "rho": 0.0, "omega": 0.0, "f0_hz": None}
    if a2 <= 0.0 or a1 * a1 >= 4.0 * a2:
        return {**report, "rejection": "not a conjugate pole pair"}
    rho = math.sqrt(a2)
    cosine = -a1 / (2.0 * rho)
    if not 0.0 < rho < 1.0 or not -1.0 < cosine < 1.0:
        return {**report, "rejection": "poles not strictly stable and complex"}
    omega = math.acos(cosine)
    return {
        **report,
        "mode": "AR2",
        "rho": rho,
        "omega": omega,
        "f0_hz": omega * FS / (2.0 * np.pi),
    }


def spectral_peaks(signal: Pcm, origin: int) -> list[dict[str, float]]:
    """Rank local peaks by measured amplitude without requested-frequency hints."""
    segment = signal[origin + WINDOW_START : origin + WINDOW_END, 0]
    assert len(segment) == WINDOW_END - WINDOW_START
    window = np.hanning(len(segment))
    magnitude = np.abs(np.fft.rfft(segment * window, n=4 * len(segment)))
    candidates = (
        np.flatnonzero((magnitude[1:-1] > magnitude[:-2]) & (magnitude[1:-1] >= magnitude[2:])) + 1
    )
    peaks = []
    for index in sorted(candidates.tolist(), key=lambda item: (-magnitude[item], item)):
        frequency = index * FS / (4 * len(segment))
        if frequency < 20.0 or magnitude[index] <= 0.0:
            continue
        delta, height = 0.0, magnitude[index]
        if np.all(magnitude[index - 1 : index + 2] > 0.0):
            low, center, high = np.log(magnitude[index - 1 : index + 2])
            curvature = low - 2.0 * center + high
            if curvature != 0.0:
                delta = float(0.5 * (low - high) / curvature)
                height = math.exp(float(center - 0.25 * (low - high) * delta))
        frequency = (index + delta) * FS / (4 * len(segment))
        if frequency >= 20.0 and all(abs(frequency - peak["hz"]) >= 100.0 for peak in peaks):
            peaks.append({"hz": frequency, "amplitude": float(2.0 * height / np.sum(window))})
        if len(peaks) == 5:
            break
    return peaks


def frequency_report(signal: Pcm, origin: int, expected: Sequence[float]) -> dict:
    """Compare the strongest components with the frozen source-derived targets."""
    peaks = spectral_peaks(signal, origin)
    actual = sorted(peak["hz"] for peak in peaks[: len(expected)])
    cents = [
        1200.0 * math.log2(found / wanted)
        for found, wanted in zip(actual, sorted(expected), strict=False)
    ]
    maximum = max(map(abs, cents), default=None)
    passed = len(actual) == len(expected) and maximum is not None and maximum <= CENTS_LIMIT
    return {
        "top5_actual_peaks": peaks,
        "selected_sorted_hz": actual,
        "expected_sorted_hz": sorted(expected),
        "cents": cents,
        "maximum_absolute_cents": maximum,
        "tonal_15_cent_pass": passed,
    }


def render(source: Pcm, model: dict, rate: int, semitones: int) -> tuple[Pcm | None, dict]:
    """Retain all nonzero innovations and causally synthesize the fixed model."""
    shift = 2.0 ** (semitones / 12.0)
    radius, angle = model["rho"] ** rate, model["omega"] * shift
    if model["mode"] == "AR2" and not 0.0 < angle < math.pi:
        return None, {"rejection": "shifted pole angle outside (0, pi)"}
    residual = source.copy()
    residual[1:] += model["a1"] * source[:-1]
    residual[2:] += model["a2"] * source[:-2]
    indices = (np.arange(SOURCE_FRAMES) + rate - 1) // rate
    count = SOURCE_FRAMES // rate
    inside = indices < count
    assert not np.any(residual[~inside] != 0.0), "nonzero innovation outside declared output"
    excitation = np.zeros((count, 2))
    np.add.at(excitation, indices[inside], residual[inside])
    assert not np.any(excitation[: SOURCE_TARGET // rate] != 0.0)
    b1, b2 = -2.0 * radius * math.cos(angle), radius * radius
    output = np.empty_like(excitation)
    for channel in range(2):
        previous, older = 0.0, 0.0
        for frame in range(count):
            current = float(excitation[frame, channel]) - b1 * previous - b2 * older
            output[frame, channel] = current
            older, previous = previous, current
    assert np.all(np.isfinite(output))
    return output, {
        "b1": b1,
        "b2": b2,
        "synthesis_radius": radius,
        "synthesis_angle": angle,
        "remaining_last_two_output_frames": output[-2:].tolist(),
        "remaining_free_response_energy_upper_bound": None,
        "capture_tail_is_complete_response": radius == 0.0,
        "tail_limit": "last 20ms measured; nonzero-radius IIR may continue; no infinite-tail bound",
    }


def metrics(output: Pcm, source: Pcm, origin: int) -> dict:
    """Measure the complete captured output without claiming an infinite tail."""
    energy = np.sum(output * output, axis=1)
    total, source_total = float(np.sum(energy)), float(np.sum(source * source))
    peak = int(np.argmax(np.max(np.abs(output), axis=1)))
    return {
        "output_frames": len(output),
        "source_frames": len(source),
        "duration_seconds": len(output) / FS,
        "prefix_boundary_frame": origin,
        "raw_energy": total,
        "source_energy": source_total,
        "output_over_source_energy": total / source_total,
        "pre_boundary_energy_fraction": float(np.sum(energy[:origin])) / total if total else None,
        "original_output_peak_frame": peak,
        "original_output_peak_amplitude": float(np.max(np.abs(output[peak]))),
        "original_output_peak_after_boundary": peak >= origin,
        "final_20ms_energy_fraction": float(np.sum(energy[-FS // 50 :])) / total if total else None,
        "all_pre_boundary_samples_zero": not bool(np.any(output[:origin] != 0.0)),
    }


def main() -> None:
    """Run integrity controls and report acceptance separately from execution."""
    emit({
        "stage": "contract",
        "python_version": sys.version,
        "numpy_version": np.__version__,
        "sample_rate_hz": FS,
        "source_frames": SOURCE_FRAMES,
        "source_target_frame": SOURCE_TARGET,
        "r2_output_frames": SOURCE_FRAMES // 2,
        "H": H,
        "source_event_frames": EVENT_FRAMES,
        "source_tones_hz": [900.37, 331.37, 997.19],
        "source_amplitudes": {
            "impulse": 1.0,
            "mono": 0.25,
            "poly": [0.22, 0.17],
            "poly_impulse": 0.8,
        },
        "source_phases": [0.37, 1.1],
        "stereo_right_scale": 0.5,
        "source_envelope": (
            "fixed 96-sample half-Hann rise/fall, including zero and unity endpoints"
        ),
        "source_before_target": "exact zero",
        "model": "one stereo-pooled biased Yule-Walker AR2 fit per fixture",
        "residual": "e[m]=x[m]+a1*x[m-1]+a2*x[m-2]; zero initial history",
        "synthesis": (
            "radius=rho**r; angle=h*omega; b1=-2*radius*cos(angle); "
            "b2=radius**2; y=E-b1*y[-1]-b2*y[-2]"
        ),
        "excitation": (
            "all source innovations scatter at ceil(m/r); collisions sum; no advance or gain"
        ),
        "profiles_rate_semitones": PROFILES,
        "identity_limit": IDENTITY_LIMIT,
        "frequency_window_relative_frames": [WINDOW_START, WINDOW_END],
        "frequency_estimator": (
            "left channel, Hann FFT, 4x zero padding, log-parabolic local peaks >=20Hz, "
            "100Hz separation; top1/top2 by amplitude, no target search"
        ),
        "frequency_limit_cents": CENTS_LIMIT,
        "output_dtype": "float64",
        "native_used": False,
        "production_activation": False,
        "overall_acoustic_acceptance": False,
        "canonical_q10_q50_computed": False,
        "fitted_offset_gain_crop_threshold_target": False,
        "limits": [
            "single pole pair cannot represent general polyphony",
            "residual can force wrong frequency",
            "innovation collisions can cancel or reinforce",
            "pole rotation need not preserve amplitude",
            "no live handover, finite-supply, device, realtime or listening proof",
        ],
    })
    negatives, unsupported = [], []
    for name, source, frequencies in fixtures():
        if frequencies:
            calibration = frequency_report(source, SOURCE_TARGET, frequencies)
            emit({"stage": "source_calibration", "fixture": name, **calibration})
            assert calibration["tonal_15_cent_pass"], "source estimator calibration failed"
        model = fit(source)
        emit({"stage": "fixed_model_fit", "fixture": name, **model})
        for rate, semitones in PROFILES:
            identity = rate == 1 and semitones == 0
            record = {
                "stage": "profile",
                "fixture": name,
                "rate": rate,
                "semitones": semitones,
                "h": 2.0 ** (semitones / 12.0),
                "identity_control": identity,
                "model": model,
                "native_acceptance": False,
                "overall_acoustic_acceptance": False,
            }
            output, details = (
                (None, {"rejection": model["rejection"]})
                if "rejection" in model
                else render(source, model, rate, semitones)
            )
            if output is None:
                emit({**record, **details, "tonal_15_cent_pass": False if frequencies else None})
                unsupported.append([name, rate, semitones])
                assert not identity, "identity control requires an admitted causal model"
                continue
            origin = SOURCE_TARGET // rate
            record.update(details)
            record.update(metrics(output, source, origin))
            if identity:
                error = float(np.max(np.abs(output - source)))
                record["identity_maximum_absolute_error"] = error
                assert error <= IDENTITY_LIMIT, "identity reconstruction failed"
            if name == "impulse":
                expected = np.zeros_like(output)
                expected[origin] = [1.0, 0.5]
                exact = bool(np.array_equal(output, expected))
                record.update(
                    impulse_exact_mapped_pcm=exact,
                    tonal_15_cent_pass=None,
                    zero_mode_is_tonal_pitch_acceptance=False,
                )
                assert exact, "original impulse must be exactly mapped without pre-response or tail"
            else:
                record.update(
                    frequency_report(
                        output, origin, [frequency * record["h"] for frequency in frequencies]
                    )
                )
                if not record["tonal_15_cent_pass"]:
                    negatives.append([name, rate, semitones])
            emit(record)
    emit({
        "stage": "conclusion",
        "fixed_model_rejected_by_tonal_15_cent_negative": bool(negatives),
        "tonal_negative_profiles": negatives,
        "unsupported_profiles": unsupported,
        "fixed_model_rejected": bool(negatives or unsupported),
        "native_acceptance": False,
        "overall_acoustic_acceptance": False,
        "operational_success_does_not_mean_model_acceptance": True,
    })


if __name__ == "__main__":
    main()
