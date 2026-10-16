"""Evaluate source-certified linear chirplets on the unchanged complete matrix."""

import time
from functools import partial

from pitch_am_family_probe import domain_controls
from pitch_causal_model_probe import emit
from pitch_chirplet_controls import integrity_controls as chirplet_controls
from pitch_independent_modes import integrity_controls, prepare
from pitch_independent_modes_probe import calibrate, cases, run_fixture
from pitch_overlap_probe import RUN_PROFILES


def main() -> None:
    """Keep all sources, score targets, exact complements and finite tails."""
    emit({
        "stage": "contract",
        "model_change_from_v7": "source-certified integrated linear phase after stationary K0",
        "source_estimator": "blind crossings then bounded stereo projected refinement",
        "envelope_solver": "same current-weight affine NNLS and fixed64 prediction",
        "source_phase_owner": "one epoch and integral; no independent support phase resets",
        "source_without_admitted_chirplet_retains_v7_signal_path": True,
        "all_original_sources_targets_scores_and_gates_unchanged": True,
        "complete_transformed_exact_residual_and_all_decay_tails_retained": True,
        "general_mixture_variable_map_native_realtime_or_human_acceptance": False,
    })
    for control in (chirplet_controls, domain_controls, integrity_controls):
        emit(control())
    sources = cases()
    calibrate(sources)
    controls, negatives = {}, {"timing": [], "tonal": [], "unsupported": []}
    preparer = partial(
        prepare,
        certified_causal=True,
        affine_endpoint=True,
        predictive_envelope=True,
        current_weight=True,
        am_family=True,
        modulated_basis=True,
        linear_chirplet=True,
    )
    started = time.perf_counter()
    for fixture in sources:
        run_fixture(fixture, controls, negatives, preparer=preparer)
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
