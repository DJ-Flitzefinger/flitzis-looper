"""Evaluate independent source gates on the unchanged complete quality matrix."""

import time
from functools import partial

from pitch_causal_model_probe import emit
from pitch_independent_modes import prepare
from pitch_independent_modes_probe import calibrate, cases, run_fixture
from pitch_joint_finite_gate_controls import integrity_controls
from pitch_overlap_probe import RUN_PROFILES


def run_quality_matrix() -> None:
    """Keep original source samples, targets, full-response criteria and tails."""
    emit({
        "stage": "contract",
        "model_change_from_v9": "independent carrier gates, joint I/Q, one sparse stereo frame",
        "source_policy": "complete source-grid search; all feasible models agree or refuse",
        "source_model_validity_is_conditional_not_unique_physical_interpretation": True,
        "offline_noncausal_fit": True,
        "refusal_preserves_v9_signal_path": True,
        "all_original_sources_targets_scores_and_gates_unchanged": True,
        "all_exact_residual_samples_and_generated_tails_retained": True,
        "general_mixture_variable_rate_native_realtime_or_human_acceptance": False,
    })
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
        finite_am_gate=True,
        joint_finite_gates=True,
    )
    started = time.perf_counter()
    for fixture in sources:
        emit({"stage": "source_started", "fixture": fixture.name})
        run_fixture(fixture, controls, negatives, preparer=preparer)
    emit({
        "stage": "summary",
        "requested_profiles": len(sources) * len(RUN_PROFILES),
        "elapsed_seconds": time.perf_counter() - started,
        "negatives": negatives,
        "all_requested_profiles_accepted_and_measured_criteria_pass": not any(negatives.values()),
        "full_product_gate_closed": False,
    })


def main() -> None:
    """Check independent source/complete-render controls before measuring quality."""
    emit(integrity_controls())
    run_quality_matrix()


if __name__ == "__main__":
    main()
