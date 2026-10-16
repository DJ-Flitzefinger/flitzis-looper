"""Evaluate certified carriers with local causal envelopes and the full residual."""

import time
from functools import partial

from pitch_causal_model_probe import emit
from pitch_independent_modes import integrity_controls, prepare
from pitch_independent_modes_probe import calibrate, cases, run_fixture
from pitch_mode_certificate import integrity_controls as certificate_controls
from pitch_mode_envelopes import causal_integrity_controls
from pitch_overlap_probe import RUN_PROFILES


def main() -> None:
    """Keep original scoring and all source data for the independently frozen v2 path."""
    emit({
        "stage": "contract",
        "source_window_admission": "spread-column full-window certificate; original guard reported",
        "carrier_recurrence_refinement_coherence": "unchanged source-only rules",
        "envelopes": "trailing256 symmetricHann, hop64, left-held source knots",
        "global_carrier_discovery_claimed_causal": False,
        "original_centered_candidate_still_selectable": True,
        "complete_residual_and_all_tails_retained": True,
        "original_sources_scorer_targets_gates_unchanged": True,
        "no_native_or_production_or_human_acceptance": True,
    })
    for control in (certificate_controls, causal_integrity_controls, integrity_controls):
        emit(control())
    sources = cases()
    calibrate(sources)
    controls, negatives = {}, {"timing": [], "tonal": [], "unsupported": []}
    preparer = partial(prepare, certified_causal=True)
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
