"""Evaluate source-only affine endpoint envelopes with the full transformed residual."""

import time
from functools import partial

from pitch_affine_envelope_controls import integrity_controls as affine_controls
from pitch_causal_model_probe import emit
from pitch_independent_modes import integrity_controls, prepare
from pitch_independent_modes_probe import calibrate, cases, run_fixture
from pitch_mode_reference import main as known_mode_reference
from pitch_overlap_probe import RUN_PROFILES


def main() -> None:
    """Retain the entire original/unseen matrix and its unchanged acoustic gates."""
    emit({
        "stage": "contract",
        "carrier_policy": "unchanged v2 source-certified spread pencil/refinement/coherence",
        "envelope_model": "joint nonnegative L/R endpoints per carrier in trailing256 Hann",
        "knots": "firstactive+64*j through finite input; left hold current R endpoint",
        "future_endpoint_directly_observed": False,
        "complete_residual_all_decay_and_tails_retained": True,
        "conditioning": "actual 2K design; original unpenalized NNLS refusal limits",
        "unconstrained_leverage_certifies_nnls_active_sensitivity": False,
        "original_v1_v2_sources_targets_scoring_gates_unchanged": True,
        "no_native_or_production_or_global_causality_acceptance": True,
    })
    for control in (affine_controls, integrity_controls):
        emit(control())
    sources = cases()
    calibrate(sources)
    controls, negatives = {}, {"timing": [], "tonal": [], "unsupported": []}
    preparer = partial(prepare, certified_causal=True, affine_endpoint=True)
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
    known_mode_reference(affine_endpoint=True)


if __name__ == "__main__":
    main()
