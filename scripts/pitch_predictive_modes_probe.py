"""Evaluate projected causal affine predictions with the complete residual."""

import time
from functools import partial

from pitch_affine_envelope_controls import integrity_controls as affine_controls
from pitch_causal_model_probe import emit
from pitch_independent_modes import integrity_controls, prepare
from pitch_independent_modes_probe import calibrate, cases, run_fixture
from pitch_mode_reference import main as known_mode_reference
from pitch_overlap_probe import RUN_PROFILES
from pitch_predictive_envelope_controls import integrity_controls as prediction_controls


def main() -> None:
    """Keep all original sources, complete outputs and acoustic criteria."""
    emit({
        "stage": "contract",
        "carrier_and_fit_policy": "unchanged v3 certified carriers and joint L/R NNLS",
        "envelope_prediction": "max(0,R+d*(R-L)/255) for next source ages d=0..63",
        "source_grid": "firstactive+64*j through finite input; no lastactive mask/end knot",
        "future_source_samples_used_by_fixed_basis_envelope_fit": False,
        "wet_parameters_used": False,
        "prediction_leverage_is_weighted_target_unconstrained_only": True,
        "complete_residual_all_decay_and_tails_retained": True,
        "original_v1_v2_v3_sources_targets_scoring_gates_unchanged": True,
        "no_native_or_production_or_global_causality_acceptance": True,
    })
    for control in (affine_controls, prediction_controls, integrity_controls):
        emit(control())
    sources = cases()
    calibrate(sources)
    controls, negatives = {}, {"timing": [], "tonal": [], "unsupported": []}
    preparer = partial(
        prepare, certified_causal=True, affine_endpoint=True, predictive_envelope=True
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
    known_mode_reference(affine_endpoint=True, predictive_envelope=True)


if __name__ == "__main__":
    main()
