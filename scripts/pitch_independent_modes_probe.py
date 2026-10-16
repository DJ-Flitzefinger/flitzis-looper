"""Score source-derived modes plus a complete transformed residual.

Prepare once per source; identity precedes its three shifted profiles. Original
scoring is retained, with target-blind dense peak ranking for one close pair.
"""

import math
import time
from functools import lru_cache

import numpy as np
from pitch_amfm_probe import sources as original_sources
from pitch_causal_model_probe import FS, SOURCE_FRAMES, SOURCE_TARGET, Pcm, emit, frequency_report
from pitch_independent_modes import (
    UnsupportedPitchDomainError,
    integrity_controls,
    prepare,
    render_prepared,
)
from pitch_mode_carriers import integrity_controls as carrier_controls
from pitch_mode_envelopes import integrity_controls as envelope_controls
from pitch_overlap_probe import RUN_PROFILES, Fixture, metric_controls, profile, stereo
from pitch_phase_gradient_probe import signed_metric_controls


def gated(
    frequency: float, start: int, count: int, amplitude: float, phase: float, ramp: int
) -> Pcm:
    """Generate independent scorer inputs without consulting the renderer."""
    result = np.zeros(SOURCE_FRAMES)
    clock = np.arange(count) / FS
    gate = np.ones(count)
    rise = 0.5 * (1 - np.cos(np.pi * np.arange(ramp) / (ramp - 1)))
    gate[:ramp], gate[-ramp:] = rise, rise[::-1]
    result[start : start + count] = amplitude * np.sin(2 * np.pi * frequency * clock + phase) * gate
    return result


def cases() -> list[Fixture]:
    """Keep the original ten and add unseen geometry, mixtures and limits."""
    result = original_sources()
    for name, frequencies, offset in (
        ("unseen_two_carriers", (427.43, 1327.89), 0),
        ("unseen_close_carriers", (521.17, 533.43), 0),
        ("unseen_independent_gates", (371.39, 1381.77), 97),
    ):
        start = SOURCE_TARGET + offset
        left = gated(frequencies[0], start, 48_000, 0.13, 0.19, 145)
        left += gated(frequencies[1], start + 1733, 36_000, 0.21, -0.8, 217)
        left[start + 8191] += 0.37
        scored = () if name == "unseen_close_carriers" else frequencies
        result.append(Fixture(name, stereo(left), scored, (start,)))
    clock = np.arange(48_000) / FS
    chirp = 0.17 * np.sin(2 * np.pi * (400 * clock + 450 * clock * clock) + 0.71)
    modulated = 0.14 * (1 + 0.4 * np.cos(2 * np.pi * 23.47 * clock))
    modulated *= np.sin(2 * np.pi * 713.19 * clock + 0.31)
    noise = 0.04 * np.random.default_rng(28051).normal(size=len(clock))
    for name, event in (
        ("unseen_chirp", chirp),
        ("unseen_AM_interpretation", modulated),
        ("unseen_noise", noise),
    ):
        left = np.zeros(SOURCE_FRAMES)
        fade = 0.5 * (1 - np.cos(np.pi * np.arange(128) / 127))
        event[:128] *= fade
        event[-128:] *= fade[::-1]
        left[SOURCE_TARGET : SOURCE_TARGET + len(event)] = event
        result.append(Fixture(name, stereo(left)))
    high = gated(19_873.23, SOURCE_TARGET, 48_000, 0.17, 0.23, 192)
    result.append(Fixture("unseen_near_nyquist", stereo(high), (19_873.23,)))
    left = gated(583.31, SOURCE_TARGET, 48_000, 0.19, -0.17, 144)
    right = gated(947.73, SOURCE_TARGET + 1337, 37_000, 0.11, 1.21, 173)
    result.append(Fixture("unseen_unrelated_stereo", np.column_stack((left, right)), (583.31,)))
    return result


def dense_frequency_report(output: Pcm, origin: int, expected: list[float]) -> dict:
    """Rank actual local peaks before comparison, without a target-neighborhood search."""
    segment = output[origin + 4096 : origin + 20_480, 0]
    assert len(segment) == 16_384
    magnitude = np.abs(np.fft.rfft(segment * np.hanning(len(segment)), n=4 * len(segment)))
    candidates = (
        np.flatnonzero((magnitude[1:-1] > magnitude[:-2]) & (magnitude[1:-1] >= magnitude[2:])) + 1
    )
    ranked = sorted(candidates.tolist(), key=lambda index: (-magnitude[index], index))
    peaks = []
    for index in ranked:
        if index * FS / (4 * len(segment)) < 20 or magnitude[index] <= 0:
            continue
        delta = 0.0
        if np.all(magnitude[index - 1 : index + 2] > 0.0):
            low, center, high = np.log(magnitude[index - 1 : index + 2])
            curvature = low - 2 * center + high
            delta = 0.5 * (low - high) / curvature if curvature != 0 else 0.0
        peaks.append(float((index + delta) * FS / (4 * len(segment))))
        if len(peaks) == 5:
            break
    selected = sorted(peaks[: len(expected)])
    cents = [
        1200 * math.log2(found / wanted)
        for found, wanted in zip(selected, sorted(expected), strict=False)
    ]
    maximum = max(map(abs, cents), default=None)
    return {
        "actual_top5_hz": peaks,
        "selected_hz": selected,
        "expected_hz": sorted(expected),
        "maximum_absolute_cents": maximum,
        "tonal_15_cent_pass": len(selected) == len(expected)
        and maximum is not None
        and maximum <= 15,
    }


def calibrate(sources: list[Fixture]) -> None:
    """Validate source estimators before observing transformed output."""
    for fixture in sources:
        if fixture.frequencies:
            result = frequency_report(fixture.source, SOURCE_TARGET, fixture.frequencies)
        elif fixture.name == "unseen_close_carriers":
            result = dense_frequency_report(fixture.source, SOURCE_TARGET, [521.17, 533.43])
        else:
            continue
        emit({"stage": "source_calibration", "fixture": fixture.name, **result})
        assert result["tonal_15_cent_pass"], "Source spectral calibration failed"


def run_fixture(fixture: Fixture, controls: dict, negatives: dict) -> None:
    """Use one fixed source decomposition for unity and nonunity intentions."""
    started = time.perf_counter()
    prepared = prepare(fixture.source)
    emit({
        "stage": "source_preparation",
        "fixture": fixture.name,
        "elapsed_seconds": time.perf_counter() - started,
        **prepared.report,
    })

    @lru_cache(maxsize=1)
    def render_profile(rate: float, semitones: int) -> tuple[Pcm, dict]:
        return render_prepared(prepared, rate, semitones)

    def renderer(source: Pcm, rate: float, semitones: int) -> tuple[Pcm, dict]:
        assert np.array_equal(source, prepared.source)
        return render_profile(rate, semitones)

    for rate, semitones in RUN_PROFILES:
        label = [fixture.name, rate, semitones]
        tick = time.perf_counter()
        try:
            record = profile(fixture, rate, semitones, controls, renderer=renderer)
        except UnsupportedPitchDomainError as error:
            emit({
                "stage": "unsupported_domain",
                "profile": label,
                "reason": str(error),
                "request_accepted_or_clamped": False,
            })
            negatives["unsupported"].append(label)
            continue
        if fixture.name == "unseen_close_carriers":
            output, details = renderer(fixture.source, float(rate), semitones)
            record["frequency"] = dense_frequency_report(
                output,
                SOURCE_TARGET // rate - details["capture_origin"],
                [frequency * 2 ** (semitones / 12) for frequency in (521.17, 533.43)],
            )
        record["profile_elapsed_seconds"] = time.perf_counter() - tick
        record["source_stereo_kind"] = (
            "unrelated" if fixture.name.endswith("unrelated_stereo") else "R=.5L"
        )
        record["AM_physical_vs_sideband_interpretation_certified"] = False
        emit(record)
        if record["isolated_timing_retention_tail_pass"] is False:
            negatives["timing"].append(label)
        if "frequency" in record and not record["frequency"]["tonal_15_cent_pass"]:
            negatives["tonal"].append(label)


def main() -> None:
    """Run integrity, source calibration and all original/unseen profiles."""
    emit({
        "stage": "contract",
        "source_only_preparation": True,
        "profile_order": "fixture first; r1 identity then r2 -7/0/+7 using same decomposition",
        "all_residual_transformed": True,
        "original_gates_unchanged": True,
        "physical_AM_vs_sideband_ambiguity_and_residual_aliasing_open": True,
        "no_native_or_production_or_human_acceptance": True,
    })
    for control in (
        metric_controls,
        signed_metric_controls,
        carrier_controls,
        envelope_controls,
        integrity_controls,
    ):
        emit(control())
    sources = cases()
    calibrate(sources)
    controls, negatives = {}, {"timing": [], "tonal": [], "unsupported": []}
    started = time.perf_counter()
    for fixture in sources:
        run_fixture(fixture, controls, negatives)
    emit({
        "stage": "summary",
        "requested_profiles": len(sources) * len(RUN_PROFILES),
        "elapsed_seconds": time.perf_counter() - started,
        "negatives": negatives,
        "all_requested_profiles_accepted_and_measured_criteria_pass": not any(negatives.values()),
        "full_product_gate_closed": False,
    })


if __name__ == "__main__":
    main()
