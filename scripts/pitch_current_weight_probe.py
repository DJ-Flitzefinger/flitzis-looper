"""Evaluate one frozen positive-current window with complete residual output."""

import time
from functools import partial

from pitch_affine_envelope_controls import integrity_controls as affine_controls
from pitch_causal_model_probe import emit
from pitch_current_weight_controls import integrity_controls as current_controls
from pitch_independent_modes import integrity_controls, prepare
from pitch_independent_modes_probe import calibrate, cases, run_fixture
from pitch_mode_reference import main as known_mode_reference
from pitch_overlap_probe import RUN_PROFILES
from pitch_predictive_envelope_controls import integrity_controls as predictive_controls


def main() -> None:
    """Measure unchanged sources, full outputs, raw failures and acoustic gates."""
    emit({
        "stage": "contract",
        "only_signal_change_from_v4": "w[m]=0.5*(1-cos(pi*m/255)), m=0..255",
        "retained_fit": "source-certified carriers, actual 2K bounded L/R NNLS, fixed64 grid",
        "retained_prediction": "max(0,R+d*(R-L)/255), d=0..63",
        "positive_current_weight_certifies_current_amplitude_or_attack_quality": False,
        "weighted_target_and_actual_pcm_row_norms_separate_unconstrained_only": True,
        "source_only_local_fit_complete_residual_all_decay_and_tails": True,
        "original_v1_v2_v3_v4_sources_targets_scoring_gates_unchanged": True,
        "no_native_or_production_or_global_causality_acceptance": True,
    })
    for control in (affine_controls, predictive_controls, current_controls, integrity_controls):
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
    known_mode_reference(affine_endpoint=True, predictive_envelope=True, current_weight=True)


if __name__ == "__main__":
    main()
