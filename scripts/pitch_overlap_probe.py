"""Measure a frozen full-PCM overlap candidate without Native or device access.

The renderer receives PCM/rate/pitch only. Targets belong solely to this scorer.
This is an offline diagnostic, not a live renderer or a musical acceptance gate.
"""

import math
import sys
from dataclasses import dataclass
from itertools import starmap
from typing import TYPE_CHECKING

import numpy as np
from pitch_causal_model_probe import (
    EVENT_FRAMES,
    FS,
    IDENTITY_LIMIT,
    PROFILES,
    SOURCE_FRAMES,
    SOURCE_TARGET,
    H,
    Pcm,
    emit,
    fixtures,
    frequency_report,
)
from pitch_overlap_model import render

if TYPE_CHECKING:
    from collections.abc import Callable

type Renderer = Callable[[Pcm, float, int], tuple[Pcm, dict]]

TIMING_FRAMES = FS * 2 // 1000
HALF_WINDOW = round(FS * 0.00025)
RETIREMENT_FRAMES = FS // 50
SECOND_ATTACK = SOURCE_TARGET + 768
RUN_PROFILES = (PROFILES[-1], *PROFILES[:-1])


@dataclass(frozen=True)
class Fixture:
    """Keep scorer metadata separate from the renderer's PCM-only input."""

    name: str
    source: Pcm
    frequencies: tuple[float, ...] = ()
    events: tuple[int, ...] = (SOURCE_TARGET,)
    isolated: bool = True


def stereo(left: Pcm) -> Pcm:
    """Use the original diagnostic's fixed shared stereo relation."""
    return np.column_stack((left, 0.5 * left))


def source_cases() -> list[Fixture]:
    """Build original and dense-background sources before observing output."""
    cases = list(starmap(Fixture, fixtures()))
    cases.extend(short_attacks())
    impulses = np.zeros(SOURCE_FRAMES)
    impulses[SOURCE_TARGET], impulses[SECOND_ATTACK] = 0.8, 0.6
    events = (SOURCE_TARGET, SECOND_ATTACK)
    cases.append(Fixture("nearby_isolated_impulses", stereo(impulses), events=events))
    time = np.arange(SOURCE_FRAMES) / FS
    background = 0.035 * np.sin(2.0 * np.pi * 241.13 * time + 0.31)
    background += 0.027 * np.sin(2.0 * np.pi * 613.71 * time + 0.91)
    ramp = 0.5 * (1.0 - np.cos(np.pi * np.arange(96) / 95))
    background[:96] *= ramp
    background[-96:] *= ramp[::-1]
    cases.extend((
        Fixture("running_background", stereo(background), (241.13, 613.71), (), isolated=False),
        Fixture(
            "busy_nearby_impulses", stereo(background + impulses), events=events, isolated=False
        ),
    ))
    assert np.array_equal(cases[-1].source[:SOURCE_TARGET], cases[-2].source[:SOURCE_TARGET])
    return cases


def component_noise(frame: int) -> np.float32:
    """Mirror the existing Rust fixture's channel0/component0 wrapping noise."""
    value = (frame * 747_796_405 + 131) & 0xFFFFFFFF
    value = (((value >> ((value >> 28) + 4)) ^ value) * 277_803_737) & 0xFFFFFFFF
    value = (value >> 22) ^ value
    return np.float32(value / 0xFFFFFFFF * 2.0 - 1.0) * np.float32(0.075)


def short_attacks() -> list[Fixture]:
    """Reuse the existing eight-ms tonal/percussion source equations and scale."""
    count = FS * 8 // 1000
    time, progress = np.arange(count) / FS, np.arange(count) / count
    tone = np.sin(2.0 * np.pi * 900.0 * time + 0.37)
    tone = (tone * np.sin(np.pi * progress) ** 2).astype(np.float32)
    envelope = np.exp(-7.0 * progress)
    percussion = (envelope * np.sin(2.0 * np.pi * 180.0 * time + 0.7)).astype(np.float32)
    percussion *= np.float32(0.7)
    percussion += (
        np.array([component_noise(frame) for frame in range(count)])
        * envelope.astype(np.float32)
        * np.float32(2.0)
    )
    cases = []
    for name, attack in (
        ("original_8ms_900_attack", tone),
        ("original_8ms_percussion", percussion),
    ):
        left = np.zeros(SOURCE_FRAMES)
        left[SOURCE_TARGET : SOURCE_TARGET + count] = attack
        cases.append(Fixture(name, stereo(left)))
    return cases


def envelope(energy: Pcm) -> Pcm:
    """Mirror the original Rust centered box and its negative-center origin."""
    prefix = np.concatenate(([0.0], np.cumsum(energy)))
    frames = np.arange(len(energy) + 2 * HALF_WINDOW)
    starts = np.clip(frames - 2 * HALF_WINDOW, 0, len(energy))
    ends = np.minimum(frames + 1, len(energy))
    return (prefix[ends] - prefix[starts]) / (2 * HALF_WINDOW + 1)


def energy_times(energy: Pcm) -> list[int]:
    """Return q10/q50/q90 with the unchanged original source timeline."""
    smooth = envelope(energy)
    cumulative = np.cumsum(smooth)
    assert cumulative[-1] > 0.0
    return [
        int(np.searchsorted(cumulative, fraction * cumulative[-1])) - HALF_WINDOW
        for fraction in (0.1, 0.5, 0.9)
    ]


def metric_controls() -> dict:
    """Check the vector envelope against an independent explicit-window sum."""
    maximum_error = 0.0
    random = np.random.default_rng(314159)
    for energy in (np.array([1.0]), random.integers(0, 8, 127).astype(np.float64)):
        width = 2 * HALF_WINDOW + 1
        scalar = np.array([
            sum(
                float(energy[index])
                for index in range(max(0, frame - width + 1), min(frame + 1, len(energy)))
            )
            / width
            for frame in range(len(energy) + width - 1)
        ])
        maximum_error = max(maximum_error, float(np.max(np.abs(envelope(energy) - scalar))))
        reference = []
        for fraction in (0.1, 0.5, 0.9):
            cumulative = 0.0
            target = float(sum(scalar)) * fraction
            for frame, value in enumerate(scalar):
                cumulative += value
                if cumulative >= target:
                    reference.append(frame - HALF_WINDOW)
                    break
        assert energy_times(energy) == reference
    marker = np.zeros(H + 64)
    marker[H] = 1.0
    assert energy_times(marker) == [H - 10, H, H + 10]
    assert maximum_error <= 1.0e-12
    return {
        "stage": "metric_controls",
        "half_window_frames": HALF_WINDOW,
        "vector_vs_explicit_scalar_max_error": maximum_error,
        "impulse_expected_and_actual_q10_q50_q90": energy_times(marker),
        "rust_formula_reviewed": True,
        "fresh_rust_execution": False,
    }


def ranked_temporal_peaks(output: Pcm, capture_origin: int = 0) -> list[dict]:
    """Rank actual local maxima without using the desired attack locations."""
    amplitude = np.max(np.abs(output), axis=1)
    candidates = np.flatnonzero(
        (amplitude > np.concatenate(([-1.0], amplitude[:-1])))
        & (amplitude >= np.concatenate((amplitude[1:], [-1.0])))
    )
    peaks = []
    for index in sorted(candidates.tolist(), key=lambda frame: (-amplitude[frame], frame)):
        if amplitude[index] <= 1.0e-7:
            continue
        if all(abs(index + capture_origin - peak["frame"]) >= TIMING_FRAMES for peak in peaks):
            peaks.append({"frame": index + capture_origin, "amplitude": float(amplitude[index])})
        if len(peaks) == 5:
            break
    return peaks


def timing_report(energy: Pcm, dry_energy: Pcm, capture_origin: int = 0) -> dict:
    """Compare centered quantiles without a response-fitted alignment."""
    actual, reference = energy_times(energy), energy_times(dry_energy)
    actual = [frame + capture_origin for frame in actual]
    reference = [frame + capture_origin for frame in reference]
    offsets = [found - wanted for found, wanted in zip(actual, reference, strict=True)]
    return {
        "q10_q50_q90_frames": actual,
        "canonical_dry_q10_q50_q90_frames": reference,
        "q_offsets_frames": offsets,
        "q10_q50_within_original_2ms": all(abs(offset) <= TIMING_FRAMES for offset in offsets[:2]),
    }


def response_report(
    output: Pcm, dry: Pcm, fixture: Fixture, rate: int, capture_origin: int = 0
) -> dict:
    """Score the full finite response; do not infer isolated attacks from mixtures."""
    energy, dry_energy = np.sum(output * output, axis=1), np.sum(dry * dry, axis=1)
    total = float(np.sum(energy))
    assert total > 0.0
    assert np.all(np.isfinite(output))
    origin = math.ceil(fixture.events[0] / rate) if fixture.events else 0
    peak = int(np.argmax(np.max(np.abs(output), axis=1))) + capture_origin
    timing = timing_report(energy, dry_energy, capture_origin)
    prefix = float(np.sum(energy[: origin - capture_origin])) / total
    tail = float(np.sum(energy[-RETIREMENT_FRAMES:])) / total
    retention = prefix <= 0.001 and peak >= origin
    peaks = ranked_temporal_peaks(output, capture_origin)
    pair_timing = None
    if fixture.name == "nearby_isolated_impulses":
        found = sorted(peak["frame"] for peak in peaks[:2])
        wanted = [math.ceil(frame / rate) for frame in fixture.events]
        pair_timing = len(found) == 2 and all(
            abs(actual - target) <= TIMING_FRAMES
            for actual, target in zip(found, wanted, strict=False)
        )
    return {
        "complete_capture_frames": len(output),
        "canonical_dry_frames": len(fixture.source[::rate]),
        "reference_capture_frames": len(dry),
        "capture_origin": capture_origin,
        "full_energy": total,
        "output_over_source_energy": total / float(np.sum(fixture.source * fixture.source)),
        **timing,
        "raw_peak_frame": peak,
        "raw_peak_amplitude": float(np.max(np.abs(output[peak - capture_origin]))),
        "pre_first_event_energy_fraction": prefix,
        "original_peak_after_first_event": peak >= origin,
        "final_retired_20ms_energy_fraction": tail,
        "actual_top5_temporal_peaks": peaks,
        "nearby_top2_peak_timing_pass": pair_timing,
        "stereo_relation_max_error": float(np.max(np.abs(output[:, 1] - 0.5 * output[:, 0]))),
        "isolated_response": fixture.isolated,
        "original_isolated_retention_pass": retention if fixture.isolated else None,
        "isolated_timing_retention_tail_pass": (
            timing["q10_q50_within_original_2ms"]
            and retention
            and tail <= 1.0e-6
            and pair_timing is not False
            if fixture.isolated
            else None
        ),
    }


def identity_error(output: Pcm, source: Pcm, capture_origin: int) -> float:
    """Place the original source on the predetermined signed capture timeline."""
    assert capture_origin <= 0
    expected = np.zeros_like(output)
    expected[-capture_origin : -capture_origin + len(source)] = source
    return float(np.max(np.abs(output - expected)))


def paired_prefix_report(output: Pcm, control: Pcm, target: int, capture_origin: int) -> dict:
    """Expose complete pre-target changes, including negative-time output."""
    boundary = target - capture_origin
    prefix = np.abs(output[:boundary] - control[:boundary])
    return {
        "source_prefix_exactly_equal": True,
        "first_event_target": target,
        "full_pre_event_output_max_abs_difference": float(np.max(prefix)),
        "changed_pre_event_sample_values": int(np.count_nonzero(prefix)),
        "last_pre_event_stereo_difference": (output[boundary - 1] - control[boundary - 1]).tolist(),
        "is_isolated_attack_response": False,
        "original_retention_certified": False,
    }


def profile(
    fixture: Fixture,
    rate: int,
    semitones: int,
    controls: dict[tuple[int, int], Pcm],
    *,
    renderer: Renderer = render,
) -> dict:
    """Render and measure one unchanged candidate, preserving failed predicates."""
    output, details = renderer(fixture.source, float(rate), semitones)
    capture_origin = int(details.get("capture_origin", 0))
    assert capture_origin <= 0
    dry = np.pad(fixture.source[::rate], ((-capture_origin, 0), (0, 0)))
    report = {
        "stage": "profile",
        "fixture": fixture.name,
        "rate": rate,
        "semitones": semitones,
        "h": 2.0 ** (semitones / 12.0),
        **details,
        **response_report(output, dry, fixture, rate, capture_origin),
    }
    if rate == 1 and semitones == 0:
        error = identity_error(output, fixture.source, capture_origin)
        report["identity_maximum_absolute_error"] = error
        assert error <= IDENTITY_LIMIT, "same-path source identity reconstruction failed"
    if fixture.frequencies:
        origin = SOURCE_TARGET // rate if fixture.isolated else 0
        report["frequency"] = frequency_report(
            output,
            origin - capture_origin,
            [frequency * report["h"] for frequency in fixture.frequencies],
        )
    if fixture.name == "running_background":
        controls[rate, semitones] = output
    if fixture.name.startswith("busy_"):
        report["paired_background_comparison"] = paired_prefix_report(
            output, controls[rate, semitones], math.ceil(fixture.events[0] / rate), capture_origin
        )
    return report


def main() -> None:
    """Run frozen integrity controls and all original PCM quality comparisons."""
    emit({
        "stage": "contract",
        "python_version": sys.version,
        "numpy_version": np.__version__,
        "source_frames": SOURCE_FRAMES,
        "Fs": FS,
        "original_attack_frame": SOURCE_TARGET,
        "second_attack_frame": SECOND_ATTACK,
        "original_r2_H": H,
        "long_tone_frames": EVENT_FRAMES,
        "profiles_identity_first": RUN_PROFILES,
        "window_output_frames": 192,
        "hop_output_frames": 48,
        "lag_bound_original_source_frames": 192,
        "detector": "stereo difference >8*past512 mean (zero-padded), >1e-12, refractory240",
        "sampler": "segment-relative floor/ZOH, no antialias filter; alias/image risk open",
        "grain_rule": "starts -144,-96,-48,0 pinned, then 48*j<D, all positive tails kept",
        "identity_claim": "exact original r1h1 only; odd segment phase at r=h!=1 not certified",
        "source_cues_are_offline_lookahead": True,
        "fitted_parameters_gain_delay_crop_target_threshold": False,
        "native_or_production_or_human_acceptance": False,
    })
    emit(metric_controls())
    cases = source_cases()
    for fixture in cases:
        if fixture.frequencies:
            origin = SOURCE_TARGET if fixture.isolated else 0
            calibration = frequency_report(fixture.source, origin, fixture.frequencies)
            emit({"stage": "source_calibration", "fixture": fixture.name, **calibration})
            assert calibration["tonal_15_cent_pass"], (
                "source frequency estimator calibration failed"
            )
    controls: dict[tuple[int, int], Pcm] = {}
    failed_timing, failed_tonal = [], []
    for rate, semitones in RUN_PROFILES:
        for fixture in cases:
            record = profile(fixture, rate, semitones, controls)
            emit(record)
            key = [fixture.name, rate, semitones]
            if record["isolated_timing_retention_tail_pass"] is False:
                failed_timing.append(key)
            if "frequency" in record and not record["frequency"]["tonal_15_cent_pass"]:
                failed_tonal.append(key)
    emit({
        "stage": "conclusion",
        "profiles_executed": len(cases) * len(PROFILES),
        "timing_retention_tail_negative_profiles": failed_timing,
        "tonal_15_cent_negative_profiles": failed_tonal,
        "frozen_candidate_rejected": bool(failed_timing or failed_tonal),
        "operational_success_is_acoustic_acceptance": False,
        "full_B5_live_finite_resource_human_gates_remain_open": True,
    })


if __name__ == "__main__":
    main()
