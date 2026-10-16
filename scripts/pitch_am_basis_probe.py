"""Measure a source-certified positive-modulator basis on the full existing matrix."""

import time
from functools import partial

import numpy as np
from pitch_am_basis_controls import integrity_controls as basis_controls
from pitch_am_family_probe import (
    candidate_am_tonal_controls,
    domain_controls,
    known_am_reference,
)
from pitch_am_group_controls import integrity_controls as group_controls
from pitch_causal_model_probe import emit
from pitch_current_weight_controls import integrity_controls as current_controls
from pitch_independent_modes import integrity_controls, prepare
from pitch_independent_modes_probe import calibrate, cases, run_fixture
from pitch_mode_envelopes import (
    fit_current_weight_causal_envelopes,
    fit_modulated_current_weight_envelopes,
)
from pitch_mode_reference import main as known_mode_reference
from pitch_overlap_probe import RUN_PROFILES
from pitch_source_modulation import SourceModulator


def memory_and_policy_controls() -> dict:
    """Reject missing certification and the extra workspace before fitting allocation."""
    try:
        prepare(
            np.zeros((1, 2)),
            certified_causal=True,
            affine_endpoint=True,
            predictive_envelope=True,
            current_weight=True,
            modulated_basis=True,
        )
    except ValueError as error:
        policy_reason = str(error)
    else:
        message = "The fixed modulation policy accepted a missing source certificate"
        raise AssertionError(message)
    assert "positive-AM" in policy_reason
    source = np.zeros((2_680_000, 2))
    frequencies = np.array([1093.73])
    coefficients = np.array([[1.0, -1.0j]]) / np.sqrt(2.0)
    old_envelopes, old_report = fit_current_weight_causal_envelopes(
        source, frequencies, coefficients
    )
    assert old_report["estimated_peak_working_bytes"] < 256 * 1024 * 1024
    assert not np.any(old_envelopes)
    del old_envelopes
    try:
        fit_modulated_current_weight_envelopes(
            source, frequencies, coefficients, (SourceModulator(37.19, 0.17 + 0.09j),)
        )
    except ValueError as error:
        memory_reason = str(error)
    else:
        message = "Additional modulation workspace bypassed the 256 MiB guard"
        raise AssertionError(message)
    assert "256 MiB" in memory_reason
    return {
        "stage": "fixed_modulation_memory_and_policy_controls",
        "missing_certification_refused": policy_reason,
        "silent_input_frames": len(source),
        "unmodulated_accepted_estimated_peak_bytes": old_report["estimated_peak_working_bytes"],
        "additional_modulation_workspace_refused_before_output_allocation": memory_reason,
    }


def main() -> None:
    """Keep original sources, scoring, unsupported requests and all residual tails."""
    emit({
        "stage": "contract",
        "model_change_from_v6": "certified positive source modulator belongs to fitting basis",
        "common_amplitude_fit": "unchanged current-weight affine NNLS and fixed64 prediction",
        "physical_envelope": "nonnegative common prediction times fixed source modulator once",
        "source_without_certified_family_retains_prior_signal_path": True,
        "all_original_sources_targets_scores_and_gates_unchanged": True,
        "all_modes_exact_transformed_residual_and_decay_tails_retained": True,
        "unique_physical_interpretation_or_native_product_realtime_human_acceptance": False,
    })
    for control in (
        basis_controls,
        memory_and_policy_controls,
        group_controls,
        domain_controls,
        current_controls,
        integrity_controls,
    ):
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
    candidate_am_tonal_controls(am_fixture, modulated_basis=True)
    known_am_reference(am_fixture)
    known_mode_reference(
        affine_endpoint=True,
        predictive_envelope=True,
        current_weight=True,
        am_family=True,
        modulated_basis=True,
    )


if __name__ == "__main__":
    main()
