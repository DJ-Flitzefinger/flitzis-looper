"""Calibrate close-pair energy timing against declared exact independent modes.

This is an additional impulse-free known-source control, not a replacement for
the original fixture or an oracle passed to the source-only estimator.
"""

import numpy as np
from pitch_causal_model_probe import FS, SOURCE_FRAMES, SOURCE_TARGET, emit
from pitch_independent_modes import Mode, Prepared, prepare, render_prepared
from pitch_independent_modes_probe import dense_frequency_report
from pitch_overlap_probe import Fixture, response_report


def known_source() -> tuple[np.ndarray, tuple[Mode, ...]]:
    """Construct two exact finite analytic modes with one declared common birth."""
    source, modes = np.zeros((SOURCE_FRAMES, 2)), []
    for frequency, start, count, amplitude, phase, ramp in (
        (521.17, SOURCE_TARGET, 48_000, 0.13, 0.19, 145),
        (533.43, SOURCE_TARGET + 1733, 36_000, 0.21, -0.8, 217),
    ):
        envelope = np.ones(count)
        rise = 0.5 * (1 - np.cos(np.pi * np.arange(ramp) / (ramp - 1)))
        envelope[:ramp], envelope[-ramp:] = rise, rise[::-1]
        clock = np.arange(count) / FS
        left = np.zeros(SOURCE_FRAMES, dtype=np.complex128)
        left[start : start + count] = (
            amplitude * envelope * np.exp(1j * (2 * np.pi * frequency * clock + phase - np.pi / 2))
        )
        analytic = np.column_stack((left, 0.5 * left))
        source += analytic.real
        modes.append(Mode(frequency, SOURCE_TARGET + 1, analytic))
    return source, tuple(modes)


def analytic_reference(modes: tuple[Mode, ...], rate: int, semitones: int) -> np.ndarray:
    """Evaluate the same declared phase anchor by an independent output-clock formula."""
    clock = np.arange(len(modes[0].analytic_pcm[::rate]))
    output = np.zeros((len(clock), 2))
    shift = 2.0 ** (semitones / 12)
    for mode in modes:
        correction = np.exp(
            2j * np.pi * mode.frequency_hz / FS * (shift - rate) * (clock - mode.birth / rate)
        )
        output += np.real(mode.analytic_pcm[::rate] * correction[:, None])
    return output


def compare(
    fixture: Fixture, modes: tuple[Mode, ...], estimated: Prepared, rate: int, k: int
) -> None:
    """Keep exact-mode and estimator timing distinct on this same control source."""
    expected = analytic_reference(modes, rate, k)
    actual_known, _ = render_prepared(
        Prepared(fixture.source, modes, np.zeros_like(fixture.source), {}), float(rate), k
    )
    phase_error = float(np.max(np.abs(actual_known - expected)))
    assert phase_error < 1e-9
    output, details = render_prepared(estimated, float(rate), k)
    assert details["capture_origin"] == 0
    reference = np.pad(expected, ((0, len(output) - len(expected)), (0, 0)))
    if rate == 2 and k == 12:
        assert np.array_equal(expected, fixture.source[::2])
    frequency = dense_frequency_report(
        expected, SOURCE_TARGET // rate, [mode.frequency_hz * 2 ** (k / 12) for mode in modes]
    )
    assert frequency["tonal_15_cent_pass"]
    emit({
        "stage": "known_mode_reference",
        "rate": rate,
        "semitones": k,
        "exact_reference": response_report(expected, fixture.source[::rate], fixture, rate),
        "estimated_candidate": response_report(output, fixture.source[::rate], fixture, rate),
        "reference_frequency": frequency,
        "complete_estimator_vs_reference_max_abs_error": float(np.max(np.abs(output - reference))),
        "independent_reference_vs_known_mode_renderer_max_abs_error": phase_error,
        "output_difference_beat_hz": (533.43 - 521.17) * 2 ** (k / 12),
        "canonical_dry_difference_beat_hz": (533.43 - 521.17) * rate,
        "phase_gain_delay_or_crop_fitted": False,
        "original_fixtures_or_gates_changed": False,
    })


def main(
    *,
    affine_endpoint: bool = False,
    predictive_envelope: bool = False,
    current_weight: bool = False,
) -> None:
    """Measure a new known pair without changing any original quality measurement."""
    envelope_model = "affine-endpoint-v3" if affine_endpoint else "constant-v2"
    if predictive_envelope:
        envelope_model = "affine-prediction-v4"
    if current_weight:
        envelope_model = "current-weight-v5"
    emit({
        "stage": "contract",
        "source": "new impulse-free 521.17/533.43 pair with original declared gates/phases",
        "common_birth": SOURCE_TARGET + 1,
        "oracle_frequencies_and_envelopes_only_in_reference": True,
        "estimator_input": "PCM alone; certified source carriers",
        "estimated_envelope_model": envelope_model,
        "reference_is_one_declared_independent_mode_interpretation": True,
        "unique_physical_mode_births_or_universal_impossibility_claim": False,
        "original_quality_gates_unchanged": True,
    })
    source, modes = known_source()
    fixture = Fixture("known_independent_close_pair_without_impulse", source)
    estimated = prepare(
        source,
        certified_causal=True,
        affine_endpoint=affine_endpoint,
        predictive_envelope=predictive_envelope,
        current_weight=current_weight,
    )
    for rate, semitones in ((1, 0), (2, -7), (2, 0), (2, 7), (2, 12)):
        compare(fixture, modes, estimated, rate, semitones)


if __name__ == "__main__":
    main()
