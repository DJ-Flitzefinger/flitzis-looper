from typing import TYPE_CHECKING

import pytest

from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def test_set_and_clear_manual_bpm(controller: AppController) -> None:
    sample_id = 0

    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)
    assert controller.project.manual_bpm[sample_id] == 120.0

    controller.transport.bpm.clear_manual_bpm(sample_id)
    assert controller.project.manual_bpm[sample_id] is None


def test_effective_bpm_prefers_manual(controller: AppController) -> None:
    sample_id = 0

    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=123.4,
        key="C#m",
        beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0]),
    )
    assert controller.transport.bpm.effective_bpm(sample_id) == 123.4

    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)
    assert controller.transport.bpm.effective_bpm(sample_id) == 120.0


def test_tap_bpm_three_taps(controller: AppController, monkeypatch: pytest.MonkeyPatch) -> None:
    sample_id = 0
    times = iter([0.0, 0.5, 1.0])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert controller.transport.bpm.tap_bpm(sample_id) is None
    bpm = controller.transport.bpm.tap_bpm(sample_id)

    assert bpm == pytest.approx(120.0, abs=0.01)
    assert controller.project.manual_bpm[sample_id] == pytest.approx(120.0, abs=0.01)


def test_tap_bpm_uses_five_most_recent_taps(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    sample_id = 0
    times = iter([0.0, 1.0, 2.0, 3.0, 4.0, 10.0, 10.5, 11.0, 11.5, 12.0])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    bpm: float | None = None
    for _ in range(10):
        bpm = controller.transport.bpm.tap_bpm(sample_id)

    assert bpm == pytest.approx(120.0, abs=0.01)
    assert controller.session.tap_bpm_pad_id == sample_id
    assert len(controller.session.tap_bpm_timestamps) == 5
