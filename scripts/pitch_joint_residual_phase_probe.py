"""Measure one frozen joint residual phase equation with unchanged PCM criteria."""

import time
from functools import lru_cache

import numpy as np
from pitch_causal_model_probe import IDENTITY_LIMIT, emit
from pitch_independent_modes_probe import cases
from pitch_joint_residual_phase import prepare, render_prepared
from pitch_joint_residual_phase_controls import integrity_controls
from pitch_overlap_probe import metric_controls, profile

PROFILES = ((1, 0), (2, -7), (2, 0), (2, 7), (2, 12))


def _run_source(fixture, negatives: dict) -> None:
    started = time.perf_counter()
    prepared = prepare(fixture.source)
    emit({
        "stage": "joint_residual_source_preparation",
        "fixture": fixture.name,
        "elapsed_seconds": time.perf_counter() - started,
        **prepared.report,
    })

    @lru_cache(maxsize=1)
    def render_intent(rate: float, semitones: int) -> tuple[np.ndarray, dict]:
        return render_prepared(prepared, rate, semitones)

    def whole_renderer(source: np.ndarray, rate: float, semitones: int) -> tuple[np.ndarray, dict]:
        assert np.array_equal(source, prepared.source)
        return render_intent(rate, semitones)

    for rate, semitones in PROFILES:
        record = profile(fixture, rate, semitones, {}, renderer=whole_renderer)
        if semitones == 12:
            output, details = render_intent(rate, semitones)
            expected = np.zeros_like(output)
            offset = -details["capture_origin"]
            dry = fixture.source[::rate]
            expected[offset : offset + len(dry)] = dry
            error = float(np.max(np.abs(output - expected)))
            assert error <= IDENTITY_LIMIT
            record["full_capture_h_equals_r_identity_max_abs_error"] = error
        emit(record)
        if record["isolated_timing_retention_tail_pass"] is False:
            negatives["timing"].append([fixture.name, rate, semitones])
        if "frequency" in record and not record["frequency"]["tonal_15_cent_pass"]:
            negatives["tonal"].append([fixture.name, rate, semitones])


def main() -> None:
    """Run the declared controls and exactly the first fifteen complete responses."""
    emit({
        "stage": "joint_residual_phase_contract",
        "equation": "Re(L(Q,s)*exp(i*(h/r-1)*L(Psi,s))); Q=sum_all_bands_and_segments",
        "global_pooled_phase_reference_omega": 0,
        "source_cue_phase_resets_used": False,
        "kernel_structural_zero_threshold": None,
        "fitted_gain_weights_phase_delay_or_window_parameters": False,
        "original_sources_targets_and_acoustic_criteria_unchanged": True,
        "native_or_product_acceptance": False,
    })
    emit(integrity_controls())
    emit(metric_controls())
    selected = {"impulse", "mono_900.37", "unseen_noise"}
    negatives = {"timing": [], "tonal": []}
    started = time.perf_counter()
    found = []
    for fixture in cases():
        if fixture.name in selected:
            found.append(fixture.name)
            _run_source(fixture, negatives)
    assert set(found) == selected
    assert len(found) == 3
    emit({
        "stage": "joint_residual_phase_summary",
        "negatives": negatives,
        "first_fifteen_complete_responses_passed": not any(negatives.values()),
        "source_count": len(found),
        "complete_response_count": len(found) * len(PROFILES),
        "elapsed_seconds": time.perf_counter() - started,
        "full_product_gate_closed": False,
    })


if __name__ == "__main__":
    main()
