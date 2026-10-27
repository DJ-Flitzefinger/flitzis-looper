import math
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.transport import bpm as bpm_module
from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("epoch", [0.0, 1_000_000_000.0])
@pytest.mark.parametrize("delayed", [{0, 1, 2, 3}, {3, 12, 25, 39}])
def test_tap_refits_all_original_indices_and_rejects_four_delayed_taps(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    epoch: float,
    delayed: set[int],
) -> None:
    expected = 87.345
    taps = [epoch + i * 60 / expected + (0.2 if i in delayed else 0) for i in range(40)]
    clock = iter(taps)
    monkeypatch.setattr("flitzis_looper.controller.transport.bpm.monotonic", lambda: next(clock))
    result = None
    for _ in taps:
        result = controller.transport.bpm.tap_bpm(0)
    assert result == pytest.approx(expected, abs=1e-6)
    assert controller.session.tap_bpm_timestamps == taps
    assert len(controller.session.tap_bpm_timestamps) == 40


def test_tap_precision_improves_with_long_history_without_rejecting_normal_jitter() -> None:
    expected = 87.345
    taps = [
        i * 60 / expected + 0.012 * math.sin(0.73 * i) + 0.006 * math.cos(1.33 * i)
        for i in range(4000)
    ]
    short = bpm_module._estimate_tap_interval_s(taps[:40])
    long = bpm_module._estimate_tap_interval_s(taps)
    assert short is not None
    assert long is not None
    assert abs(60 / long - expected) < abs(60 / short - expected)
    assert 60 / long == pytest.approx(expected, abs=0.00001)
    # Every normal tap remains an inlier: the robust result equals all-tap OLS.
    mean = sum(taps) / len(taps)
    mx = (len(taps) - 1) / 2
    ordinary = sum((i - mx) * (t - mean) for i, t in enumerate(taps)) / sum(
        (i - mx) ** 2 for i in range(len(taps))
    )
    assert long == pytest.approx(ordinary, abs=1e-12)


def test_four_taps_recover_three_consistent_beats_but_refuse_two_competing_pairs() -> None:
    assert bpm_module._estimate_tap_interval_s([0.0, 0.5, 1.0, 2.0]) == pytest.approx(0.5)
    assert bpm_module._estimate_tap_interval_s([0.0, 0.5, 2.0, 2.5]) is None


@pytest.mark.parametrize("count", [40, 400, 4000])
def test_tap_cyclic_jitter_does_not_bias_the_seed_as_history_grows(count: int) -> None:
    taps = [0.6 * i + 0.01 * (i % 3) for i in range(count)]
    period = bpm_module._estimate_tap_interval_s(taps)
    assert period is not None
    assert 60 / period == pytest.approx(100.0, abs=0.01)


def test_tap_reclassifies_history_and_does_not_turn_gradual_drift_into_local_tempo() -> None:
    taps = [i * 0.6 + (0.16 if i == 1 else 0.0) for i in range(60)]
    before = taps.copy()
    assert bpm_module._estimate_tap_interval_s(taps) == pytest.approx(0.6, abs=1e-12)
    drift = [i * 0.6 + 0.00001 * i * i for i in range(100)]
    period = bpm_module._estimate_tap_interval_s(drift)
    assert period == pytest.approx(0.60099, abs=1e-7)
    assert taps == before


@pytest.mark.parametrize("factor", [0.5, 2.0])
def test_half_double_uses_current_fractional_bpm_and_existing_manual_clear_path(
    controller: AppController,
    audio_engine_mock: Mock,
    factor: float,
) -> None:
    original = 91.89220756855694
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=original, key="Gm", beat_grid=BeatGrid(beats=[0.25], downbeats=[0.25], bars=[0.25])
    )
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 1.0
    controller.project.pad_loop_end_s[0] = 9.0
    controller.project.pad_grid_anchor_s[0] = 0.1
    controller.project.pad_grid_offset_samples[0] = -480
    controller.transport.bpm.scale_bpm(0, factor)
    assert controller.project.manual_bpm[0] == original * factor
    audio_engine_mock.set_pad_bpm.assert_called_with(
        0, original * factor, expected_source_generation=1
    )
    assert controller.project.pad_timing_intent[0] == "manual"
    assert controller.project.pad_loop_start_s[0] == 1.0
    assert controller.project.pad_loop_end_s[0] == 9.0
    assert controller.project.pad_grid_anchor_s[0] == 0.1
    assert controller.project.pad_grid_offset_samples[0] == -480
    audio_engine_mock.set_master_bpm.assert_not_called()
    controller.transport.bpm.scale_bpm(0, 1 / factor)
    assert controller.project.manual_bpm[0] == original
    controller.transport.bpm.clear_manual_bpm(0)
    assert controller.transport.bpm.effective_bpm(0) == original
    assert controller.project.sample_analysis[0].bpm == original


@pytest.mark.parametrize(
    "reason",
    ["missing", "overflow", "pending", "rejected", "changed", "no_binding", "binding_arrives"],
)
def test_half_double_failure_never_changes_the_current_bpm_wish(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    reason: str,
) -> None:
    previous = None if reason == "missing" else 1.7e308 if reason == "overflow" else 94.0
    controller.project.manual_bpm[0] = previous
    if reason == "pending":
        controller.session.loading_sample_ids.add(0)
    if reason == "rejected":
        audio_engine_mock.set_pad_bpm.side_effect = ValueError("current source rejected")
    if reason == "changed":
        generations = iter([1, 2])
        monkeypatch.setattr(
            controller.transport,
            "capture_current_pad_edit_generation",
            lambda _pad: next(generations),
        )
    if reason in {"no_binding", "binding_arrives"}:
        possible = iter([None, 2 if reason == "binding_arrives" else None])
        monkeypatch.setattr(
            controller.transport, "capture_current_pad_edit_generation", lambda _pad: next(possible)
        )
    with pytest.raises(ValueError, match=r"unavailable|pending|rejected|changed"):
        controller.transport.bpm.scale_bpm(0, 2.0)
    assert controller.project.manual_bpm[0] == previous
    if reason != "rejected":
        audio_engine_mock.set_pad_bpm.assert_not_called()
