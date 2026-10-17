"""Score the separately declared mirrored field with unchanged PCM predicates."""

import argparse
import time
from functools import lru_cache

import numpy as np
from pitch_causal_model_probe import SOURCE_TARGET, emit
from pitch_independent_modes_probe import calibrate, cases, dense_frequency_report
from pitch_joint_residual_phase_probe import run_comparison
from pitch_overlap_probe import RUN_PROFILES, metric_controls, profile
from pitch_phase_gradient_probe import signed_metric_controls
from pitch_symmetric_residual_phase import prepare, render_prepared
from pitch_symmetric_residual_phase_controls import integrity_controls


def _full_source(fixture, controls: dict, negatives: dict) -> None:
    prepared = prepare(fixture.source)
    emit({"stage": "symmetric_full_source_preparation", "fixture": fixture.name, **prepared.report})

    @lru_cache(maxsize=1)
    def render_intent(rate: float, semitones: int) -> tuple[np.ndarray, dict]:
        return render_prepared(prepared, rate, semitones)

    def renderer(source: np.ndarray, rate: float, semitones: int) -> tuple[np.ndarray, dict]:
        assert np.array_equal(source, prepared.source)
        return render_intent(rate, semitones)

    for rate, semitones in RUN_PROFILES:
        record = profile(fixture, rate, semitones, controls, renderer=renderer)
        if fixture.name == "unseen_close_carriers":
            output, details = render_intent(rate, semitones)
            record["frequency"] = dense_frequency_report(
                output,
                SOURCE_TARGET // rate - details["capture_origin"],
                [frequency * 2 ** (semitones / 12) for frequency in (521.17, 533.43)],
            )
        record["source_stereo_kind"] = (
            "unrelated" if fixture.name.endswith("unrelated_stereo") else "R=.5L"
        )
        record["AM_physical_vs_sideband_interpretation_certified"] = False
        emit(record)
        label = [fixture.name, rate, semitones]
        if record["isolated_timing_retention_tail_pass"] is False:
            negatives["timing"].append(label)
        if "frequency" in record and not record["frequency"]["tonal_15_cent_pass"]:
            negatives["tonal"].append(label)


def full_comparison() -> None:
    """Measure the unchanged eighteen-by-four matrix after a useful first comparison."""
    emit({
        "stage": "symmetric_full_contract",
        "same_eighteen_sources_and_four_profiles": True,
        "unchanged_symmetric_field_and_phase_action": True,
        "all_signed_pre_event_and_tail_output_scored": True,
        "no_fixture_target_gain_phase_delay_cutoff_or_window_fitting": True,
        "aliasing_is_measured_without_a_new_admission_rule": True,
        "native_or_product_acceptance": False,
    })
    for control in (integrity_controls, metric_controls, signed_metric_controls):
        emit(control())
    sources = cases()
    assert len(sources) == 18
    assert RUN_PROFILES == ((1, 0), (2, -7), (2, 0), (2, 7))
    calibrate(sources)
    controls, negatives = {}, {"timing": [], "tonal": []}
    started = time.perf_counter()
    for fixture in sources:
        _full_source(fixture, controls, negatives)
    emit({
        "stage": "symmetric_full_summary",
        "requested_profiles": len(sources) * len(RUN_PROFILES),
        "negatives": negatives,
        "elapsed_seconds": time.perf_counter() - started,
        "all_measured_criteria_pass": not any(negatives.values()),
        "full_product_gate_closed": False,
    })


def main() -> None:
    """Score complete signed support without causalizing delay or crop."""
    run_comparison(
        prepare_source=prepare,
        render_source=render_prepared,
        controls=integrity_controls,
        stage_prefix="symmetric_residual_phase",
        field_equation="Qsym=x+i*(x*qs), antisymmetric mirror, origin=-1023, lookahead=1023",
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--full", action="store_true", help="Run the existing eighteen-by-four matrix"
    )
    arguments = parser.parse_args()
    if arguments.full:
        full_comparison()
    else:
        main()
