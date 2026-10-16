"""Evaluate conditional source-only finite AM gates on the unchanged full matrix."""

import time
from functools import partial

from pitch_am_family_probe import candidate_am_tonal_controls, domain_controls
from pitch_causal_model_probe import emit
from pitch_finite_am_gate_controls import integrity_controls as gate_controls
from pitch_independent_modes import integrity_controls, prepare
from pitch_independent_modes_probe import calibrate, cases, run_fixture
from pitch_overlap_probe import RUN_PROFILES


def run_quality_matrix() -> None:
    """Measure quality after the independent source controls have been verified."""
    emit({
        "stage": "contract",
        "model_change_from_v8": "conditional finite source gate and joint global I/Q",
        "source_policy": "complete bounded integer search; global and edge-local residual tests",
        "source_model_validity_is_conditional_not_unique_physical_interpretation": True,
        "offline_noncausal_fit_not_prior_local_causality": True,
        "refusal_preserves_v8_signal_path": True,
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
    am_fixture = next(f for f in sources if f.name == "unseen_AM_interpretation")
    candidate_am_tonal_controls(
        am_fixture, modulated_basis=True, linear_chirplet=True, finite_am_gate=True
    )


def main() -> None:
    """Run every source control before the unchanged complete quality matrix."""
    for control in (gate_controls, domain_controls, integrity_controls):
        emit(control())
    run_quality_matrix()


if __name__ == "__main__":
    main()
