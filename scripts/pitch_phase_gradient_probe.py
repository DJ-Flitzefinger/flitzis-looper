"""Score the frozen full phase-gradient candidate on complete signed PCM.

Original source fixtures and acoustic/frequency criteria are shared with the
previous overlap probe. No Native, device, installation or PCM export is used.
"""

import sys

import numpy as np
from pitch_causal_model_probe import FS, SOURCE_TARGET, H, Pcm, emit, frequency_report
from pitch_overlap_probe import (
    RUN_PROFILES,
    Fixture,
    metric_controls,
    profile,
    response_report,
    short_attacks,
    source_cases,
)
from pitch_phase_gradient_model import gradient_controls, render


def signed_metric_controls() -> dict:
    """Verify that the scorer never drops negative-time energy or shifts targets."""
    fixture = source_cases()[0]
    capture_origin = -67
    dry = np.pad(fixture.source[::2], ((-capture_origin, 960), (0, 0)))
    report = response_report(dry, dry, fixture, 2, capture_origin)
    assert report["raw_peak_frame"] == H
    assert report["q10_q50_q90_frames"] == [H - 10, H, H + 10]
    assert report["q_offsets_frames"] == [0, 0, 0]
    assert report["original_isolated_retention_pass"]
    changed = dry.copy()
    changed[0] = [0.25, 0.125]
    negative = response_report(changed, dry, fixture, 2, capture_origin)
    assert negative["pre_first_event_energy_fraction"] > 0.001
    assert not negative["original_isolated_retention_pass"]
    legacy = profile(fixture, 1, 0, {})
    assert legacy["identity_maximum_absolute_error"] == 0.0
    assert legacy["q_offsets_frames"] == [0, 0, 0]
    tail_fixture = Fixture("retained_renderer_tail", np.ones((32, 2)), events=(0,), isolated=False)
    tail = profile(tail_fixture, 2, 0, {}, renderer=retained_tail_renderer)
    assert tail["final_retired_20ms_energy_fraction"] == 1.0, (
        "Renderer tail hidden by appended silence"
    )
    return {
        "stage": "signed_metric_controls",
        "predetermined_capture_origin": capture_origin,
        "unmoved_original_H": report["raw_peak_frame"],
        "negative_time_energy_fraction_retained": negative["pre_first_event_energy_fraction"],
        "negative_time_retention_correctly_rejected": True,
        "legacy_default_renderer_identity_and_quantiles_unchanged": True,
        "renderer_end_tail_energy_correctly_retained": tail["final_retired_20ms_energy_fraction"],
    }


def retained_tail_renderer(_source: Pcm, _rate: float, _semitones: int) -> tuple[Pcm, dict]:
    """Return a decisive nonzero final sample to expose hidden tail scoring."""
    output = np.zeros((4096, 2))
    output[-1] = [1.0, 0.5]
    return output, {"capture_origin": 0}


def all_sources() -> list[Fixture]:
    """Retain the original matrix and add its short attack over the same background."""
    cases = source_cases()
    background = next(case for case in cases if case.name == "running_background")
    attack = short_attacks()[0]
    busy = Fixture(
        "busy_tonal_attack",
        background.source + attack.source,
        events=(SOURCE_TARGET,),
        isolated=False,
    )
    assert np.array_equal(busy.source[:SOURCE_TARGET], background.source[:SOURCE_TARGET])
    cases.append(busy)
    return cases


def main() -> None:
    """Check integrity before scoring all frozen candidate profiles."""
    emit({
        "stage": "contract",
        "python_version": sys.version,
        "numpy_version": np.__version__,
        "sample_rate_hz": FS,
        "original_source_target": SOURCE_TARGET,
        "original_r2_H": H,
        "profiles_identity_first": RUN_PROFILES,
        "window_and_fft_frames": 4096,
        "analysis_step_source_frames": 256,
        "window": "identical periodic Hann for every global centered frame",
        "alpha": "h/r; unsupported outside (0,1], never clamped",
        "synthesis_centers": "B=floor(alpha*A), explicit fractional-center phase correction",
        "complete_signed_packet_and_SRC_support_retained": True,
        "predetermined_capture_origin_is_response_fit": False,
        "src": "causal radius32 Kaiser beta8.6 sinc; static DC normalization; h1 copy",
        "SRC_filter_group_delay_compensated": False,
        "priority_threshold_discards_coefficients": False,
        "shared_stereo_phase_correction": True,
        "offline_full_source_lookahead": True,
        "native_or_production_or_human_acceptance": False,
        "fitted_gain_delay_crop_threshold_target": False,
    })
    emit(metric_controls())
    emit(signed_metric_controls())
    emit({"stage": "gradient_controls", **gradient_controls()})
    cases = all_sources()
    for fixture in cases:
        if fixture.frequencies:
            calibration = frequency_report(
                fixture.source, SOURCE_TARGET if fixture.isolated else 0, fixture.frequencies
            )
            assert calibration["tonal_15_cent_pass"]
            emit({"stage": "source_calibration", "fixture": fixture.name, **calibration})
    controls = {}
    failed_timing, failed_tonal = [], []
    for rate, semitones in RUN_PROFILES:
        for fixture in cases:
            record = profile(fixture, rate, semitones, controls, renderer=render)
            emit(record)
            key = [fixture.name, rate, semitones]
            if record["isolated_timing_retention_tail_pass"] is False:
                failed_timing.append(key)
            if "frequency" in record and not record["frequency"]["tonal_15_cent_pass"]:
                failed_tonal.append(key)
    emit({
        "stage": "conclusion",
        "profiles_executed": len(cases) * len(RUN_PROFILES),
        "timing_retention_tail_negative_profiles": failed_timing,
        "tonal_15_cent_negative_profiles": failed_tonal,
        "frozen_candidate_rejected": bool(failed_timing or failed_tonal),
        "operational_success_is_acoustic_acceptance": False,
        "general_stretch_range_and_live_crossing_proven": False,
        "full_B5_live_finite_resource_human_gates_remain_open": True,
    })


if __name__ == "__main__":
    main()
