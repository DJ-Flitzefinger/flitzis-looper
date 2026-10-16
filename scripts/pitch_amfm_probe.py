"""Measure a complete finite subband AM/FM candidate on original PCM.

No Native, device, installation or PCM export is used. Source targets belong to
this scorer only; the renderer receives PCM, rate and semitone intent.
"""

import sys
import time

import numpy as np
from pitch_amfm_model import integrity_controls, render
from pitch_causal_model_probe import FS, SOURCE_FRAMES, SOURCE_TARGET, Pcm, emit, frequency_report
from pitch_overlap_probe import RUN_PROFILES, Fixture, metric_controls, profile, stereo
from pitch_phase_gradient_probe import all_sources, signed_metric_controls


def sources() -> list[Fixture]:
    """Retain all previous cases and add independently gated nearby carriers."""
    cases = all_sources()
    left = np.zeros(SOURCE_FRAMES)
    gates = (
        (900.0, SOURCE_TARGET, 48_000, 0.22, 0.37, 96),
        (1100.0, SOURCE_TARGET + 768, 34_800, 0.17, 1.1, 144),
    )
    for frequency, start, count, amplitude, phase, ramp_frames in gates:
        clock = np.arange(count) / FS
        gate = np.ones(count)
        ramp = 0.5 * (1.0 - np.cos(np.pi * np.arange(ramp_frames) / (ramp_frames - 1)))
        gate[:ramp_frames], gate[-ramp_frames:] = ramp, ramp[::-1]
        left[start : start + count] += (
            amplitude * np.sin(2 * np.pi * frequency * clock + phase) * gate
        )
    left[SOURCE_TARGET + 1536] += 0.8
    cases.append(Fixture("independent_gated_900_1100_plus_impulse", stereo(left), (900.0, 1100.0)))
    return cases


def main() -> None:
    """Run independent integrity controls before measuring the frozen matrix."""
    emit({
        "stage": "contract",
        "python_version": sys.version,
        "numpy_version": np.__version__,
        "sample_rate_hz": FS,
        "source_frames": SOURCE_FRAMES,
        "original_source_target": SOURCE_TARGET,
        "profiles_identity_first": RUN_PROFILES,
        "source_only_segmentation": (
            "unchanged overlap detector: 8*past512 difference, 1e-12, refractory240"
        ),
        "transformation": "complete finite bank; envelope source r*n; shared carrier phase slope h",
        "no_unchanged_residual_or_attack_bridge": True,
        "fitted_gain_delay_crop_target_threshold": False,
        "native_or_production_or_human_acceptance": False,
    })
    emit(metric_controls())
    emit(signed_metric_controls())
    emit(integrity_controls())
    if "--controls-only" in sys.argv[1:]:
        return
    cases = sources()
    for fixture in cases:
        if fixture.frequencies:
            origin = SOURCE_TARGET if fixture.isolated else 0
            calibration = frequency_report(fixture.source, origin, fixture.frequencies)
            emit({"stage": "source_calibration", "fixture": fixture.name, **calibration})
            assert calibration["tonal_15_cent_pass"], "Source estimator calibration failed"
    controls: dict[tuple[int, int], Pcm] = {}
    timing_negatives, tonal_negatives = [], []
    started = time.perf_counter()
    for rate, semitones in RUN_PROFILES:
        for fixture in cases:
            tick = time.perf_counter()
            record = profile(fixture, rate, semitones, controls, renderer=render)
            record["profile_elapsed_seconds"] = time.perf_counter() - tick
            emit(record)
            label = [fixture.name, rate, semitones]
            if record["isolated_timing_retention_tail_pass"] is False:
                timing_negatives.append(label)
            if "frequency" in record and not record["frequency"]["tonal_15_cent_pass"]:
                tonal_negatives.append(label)
    emit({
        "stage": "summary",
        "profiles": len(RUN_PROFILES) * len(cases),
        "elapsed_seconds": time.perf_counter() - started,
        "isolated_timing_retention_tail_negatives": timing_negatives,
        "tonal_negatives": tonal_negatives,
        "bounded_candidate_accepted": not timing_negatives and not tonal_negatives,
        "full_product_gate_closed": False,
    })


if __name__ == "__main__":
    main()
