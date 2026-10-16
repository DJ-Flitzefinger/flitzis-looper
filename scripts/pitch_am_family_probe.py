"""Evaluate source-certified positive AM families with every original quality gate."""

import time
from functools import partial

import numpy as np
from pitch_am_group_controls import integrity_controls as group_controls
from pitch_causal_model_probe import FS, SOURCE_FRAMES, SOURCE_TARGET, emit
from pitch_current_weight_controls import integrity_controls as current_controls
from pitch_independent_modes import (
    Mode,
    Prepared,
    UnsupportedPitchDomainError,
    integrity_controls,
    prepare,
    render_prepared,
)
from pitch_independent_modes_probe import calibrate, cases, dense_frequency_report, run_fixture
from pitch_mode_reference import main as known_mode_reference
from pitch_overlap_probe import RUN_PROFILES, Fixture, response_report

AM_CENTER = 713.19
AM_MODULATION = 23.47


def domain_controls() -> dict:
    """Require old carrier and new source-clock sideband refusals before rendering."""
    for original, center, modulation, rate, semitones, reason in (
        ([14_300.0, 15_300.0, 16_300.0], 15_300.0, 1000.0, 0.5, 7, "Known carriers"),
        ([20_500.0, 22_000.0, 23_500.0], 22_000.0, 1500.0, 2.0, 0, "AM sidebands"),
    ):
        pcm = np.zeros((4, 2))
        prepared = Prepared(
            pcm,
            (Mode(center, 0, pcm.astype(np.complex128)),),
            pcm,
            {
                "carrier_estimation": {
                    "am_grouping": {
                        "original_frequencies_hz": original,
                        "accepted_groups": [
                            {
                                "center_frequency_hz": center,
                                "modulation_frequency_hz": modulation,
                            }
                        ],
                    }
                }
            },
        )
        try:
            render_prepared(prepared, rate, semitones)
        except UnsupportedPitchDomainError as error:
            reported_reason = str(error)
        else:
            message = "A known out-of-domain AM request was silently accepted"
            raise AssertionError(message)
        assert reason in reported_reason
    try:
        prepare(np.zeros((1, 2)), am_family=True)
    except ValueError as error:
        reported_reason = str(error)
    else:
        message = "AM family opt-in bypassed its declared preparation policy"
        raise AssertionError(message)
    assert "current-weight" in reported_reason
    return {"stage": "am_family_domain_controls", "explicit_refusals": 3}


def _known_am_components() -> tuple[np.ndarray, list[tuple[float, np.ndarray]]]:
    """Construct equivalent finite complex references from declared source equations."""
    count = 48_000
    clock = np.arange(count) / FS
    gate = np.ones(count)
    fade = 0.5 * (1 - np.cos(np.pi * np.arange(128) / 127))
    gate[:128], gate[-128:] = fade, fade[::-1]
    amplitude = 0.14 * (1 + 0.4 * np.cos(2 * np.pi * AM_MODULATION * clock)) * gate
    analytic = np.zeros(SOURCE_FRAMES, dtype=np.complex128)
    analytic[SOURCE_TARGET : SOURCE_TARGET + count] = amplitude * np.exp(
        1j * (2 * np.pi * AM_CENTER * clock + 0.31 - np.pi / 2)
    )
    sidebands = []
    for hz, weight in (
        (AM_CENTER - AM_MODULATION, 0.2),
        (AM_CENTER, 1),
        (AM_CENTER + AM_MODULATION, 0.2),
    ):
        component = np.zeros_like(analytic)
        component[SOURCE_TARGET : SOURCE_TARGET + count] = (
            0.14 * weight * gate * np.exp(1j * (2 * np.pi * hz * clock + 0.31 - np.pi / 2))
        )
        sidebands.append((hz, component))
    return analytic, sidebands


def known_am_reference(fixture: Fixture) -> None:
    """Keep declared AM and independent-sideband interpretations out of estimation."""
    analytic, sidebands = _known_am_components()
    source_error = float(np.max(np.abs(analytic.real - fixture.source[:, 0])))
    decomposition_error = float(np.max(np.abs(sum(z for _, z in sidebands) - analytic)))
    assert max(source_error, decomposition_error) < 1e-10
    birth = SOURCE_TARGET + 1  # Same predeclared source-frame gauge as prior known-mode controls.
    for rate, semitones in ((1, 0), (2, -7), (2, 0), (2, 7), (2, 12)):
        h = 2.0 ** (semitones / 12)
        output_clock = np.arange(len(analytic[::rate]))
        phase_clock = output_clock - birth / rate
        grouped_left = np.real(
            analytic[::rate] * np.exp(2j * np.pi * AM_CENTER / FS * (h - rate) * phase_clock)
        )
        independent_left = sum(
            np.real(z[::rate] * np.exp(2j * np.pi * hz / FS * (h - rate) * phase_clock))
            for hz, z in sidebands
        )
        grouped = np.column_stack((grouped_left, 0.5 * grouped_left))
        independent = np.column_stack((independent_left, 0.5 * independent_left))
        if h == rate:
            assert float(np.max(np.abs(grouped - fixture.source[::rate]))) < 1e-10
            assert float(np.max(np.abs(independent - fixture.source[::rate]))) < 1e-10
        emit({
            "stage": "known_am_phase_owner_reference",
            "rate": rate,
            "semitones": semitones,
            "common_birth_source_frame": birth,
            "source_reference_max_abs_error": source_error,
            "exact_complex_factorization_max_abs_error": decomposition_error,
            "grouped_carrier_source_envelope": response_report(
                grouped, fixture.source[::rate], fixture, rate
            ),
            "independent_sidebands": response_report(
                independent, fixture.source[::rate], fixture, rate
            ),
            "grouped_frequency": dense_frequency_report(
                grouped, SOURCE_TARGET // rate, [AM_CENTER * h]
            ),
            "output_modulation_hz_grouped": AM_MODULATION * rate,
            "output_modulation_hz_independent": AM_MODULATION * h,
            "oracle_parameters_only_in_reference": True,
            "unique_physical_interpretation_or_product_acceptance": False,
            "phase_gain_offset_crop_or_gate_fitted": False,
        })


def candidate_am_tonal_controls(fixture: Fixture, *, modulated_basis: bool = False) -> None:
    """Measure candidate carrier peaks without supplying an oracle to preparation."""
    estimated = prepare(
        fixture.source,
        certified_causal=True,
        affine_endpoint=True,
        predictive_envelope=True,
        current_weight=True,
        am_family=True,
        modulated_basis=modulated_basis,
    )
    for rate, semitones in ((2, -7), (2, 0), (2, 7)):
        output, details = render_prepared(estimated, float(rate), semitones)
        frequency = dense_frequency_report(
            output,
            SOURCE_TARGET // rate - details["capture_origin"],
            [AM_CENTER * 2.0 ** (semitones / 12)],
        )
        emit({
            "stage": "candidate_AM_independent_center_tonal_control",
            "rate": rate,
            "semitones": semitones,
            "frequency": frequency,
            "declared_center_only_in_independent_measurement": True,
            "original_fixture_frequency_tuple_and_quality_scores_unchanged": True,
            "overall_acoustic_acceptance": False,
        })
        assert frequency["tonal_15_cent_pass"]


def main() -> None:
    """Run independent controls and the entire unchanged source/profile matrix."""
    emit({
        "stage": "contract",
        "model_change_from_v5": "source-certified positive-AM triple -> central phase owner",
        "retained_envelopes": "same 256-frame current-weight affine NNLS and fixed64 prediction",
        "retained_output": "all modes, complete transformed exact residual, all decay/tails",
        "source_only_family_certificate_not_unique_physical_intent": True,
        "original_sources_targets_scoring_gates_unchanged": True,
        "no_native_or_production_or_realtime_or_human_acceptance": True,
    })
    for control in (group_controls, domain_controls, current_controls, integrity_controls):
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
    candidate_am_tonal_controls(am_fixture)
    known_am_reference(am_fixture)
    known_mode_reference(
        affine_endpoint=True, predictive_envelope=True, current_weight=True, am_family=True
    )


if __name__ == "__main__":
    main()
