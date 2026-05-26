from typing import TYPE_CHECKING

import pytest

from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
    from unittest.mock import Mock

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
        beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0], bars=[0.0]),
    )
    assert controller.transport.bpm.effective_bpm(sample_id) == 123.4

    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)
    assert controller.transport.bpm.effective_bpm(sample_id) == 120.0


def test_tap_bpm_computes_on_second_tap(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    times = iter([0.0, 0.5])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    assert controller.transport.bpm.tap_bpm(sample_id) is None
    bpm = controller.transport.bpm.tap_bpm(sample_id)

    assert bpm == pytest.approx(120.0, abs=0.01)
    assert controller.project.manual_bpm[sample_id] == pytest.approx(120.0, abs=0.01)
    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, pytest.approx(120.0, abs=0.01))


def test_tap_bpm_uses_all_taps_until_pause(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    sample_id = 0
    times = iter([0.0, 0.5, 1.0, 1.5, 2.0, 3.0])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    bpm: float | None = None
    for _ in range(6):
        bpm = controller.transport.bpm.tap_bpm(sample_id)

    assert bpm == pytest.approx(100.0, abs=0.01)
    assert controller.session.tap_bpm_pad_id == sample_id
    assert controller.session.tap_bpm_timestamps == [0.0, 0.5, 1.0, 1.5, 2.0, 3.0]


def test_tap_bpm_resets_after_pause_longer_than_three_seconds(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    sample_id = 0
    times = iter([0.0, 0.5, 1.0, 4.1, 4.6])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert controller.transport.bpm.tap_bpm(sample_id) == pytest.approx(120.0, abs=0.01)
    assert controller.transport.bpm.tap_bpm(sample_id) == pytest.approx(120.0, abs=0.01)

    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert controller.session.tap_bpm_timestamps == [4.1]
    assert controller.project.manual_bpm[sample_id] == pytest.approx(120.0, abs=0.01)

    assert controller.transport.bpm.tap_bpm(sample_id) == pytest.approx(120.0, abs=0.01)
    assert controller.session.tap_bpm_timestamps == [4.1, 4.6]


def test_tap_bpm_exact_three_second_pause_continues_series(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    sample_id = 0
    times = iter([0.0, 0.5, 3.5])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert controller.transport.bpm.tap_bpm(sample_id) == pytest.approx(120.0, abs=0.01)
    assert controller.transport.bpm.tap_bpm(sample_id) == pytest.approx(34.2857, abs=0.01)
    assert controller.session.tap_bpm_timestamps == [0.0, 0.5, 3.5]


def test_tap_bpm_switching_target_starts_new_series(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    times = iter([0.0, 0.5, 1.0, 1.5])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    assert controller.transport.bpm.tap_bpm(0) is None
    assert controller.transport.bpm.tap_bpm(0) == pytest.approx(120.0, abs=0.01)

    assert controller.transport.bpm.tap_bpm(1) is None
    assert controller.session.tap_bpm_pad_id == 1
    assert controller.session.tap_bpm_timestamps == [1.0]

    assert controller.transport.bpm.tap_bpm(1) == pytest.approx(120.0, abs=0.01)


@pytest.mark.parametrize("bpm", [0.0, -10.0])
def test_set_manual_bpm_invalid_raises(controller: AppController, bpm: float) -> None:
    with pytest.raises(ValueError, match="bpm must be > 0"):
        controller.transport.bpm.set_manual_bpm(0, bpm)


@pytest.mark.parametrize("bpm", ["nan", "inf", "-inf"])
def test_set_manual_bpm_non_finite_raises(controller: AppController, bpm: str) -> None:
    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.bpm.set_manual_bpm(0, float(bpm))


def test_tap_bpm_single_tap_returns_none(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    sample_id = 0
    times = iter([0.0])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert len(controller.session.tap_bpm_timestamps) == 1


def test_tap_bpm_non_monotonic_timestamps(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    sample_id = 0
    times = iter([0.0, 0.5, 1.0, 1.0, 2.0])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert controller.transport.bpm.tap_bpm(sample_id) == pytest.approx(120.0, abs=0.01)
    assert controller.transport.bpm.tap_bpm(sample_id) == pytest.approx(120.0, abs=0.01)
    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert controller.transport.bpm.tap_bpm(sample_id) == pytest.approx(90.0, abs=0.01)


def test_tap_bpm_negative_intervals(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    sample_id = 0
    times = iter([0.0, 1.0, 0.5])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert controller.transport.bpm.tap_bpm(sample_id) == pytest.approx(60.0, abs=0.01)
    assert controller.transport.bpm.tap_bpm(sample_id) is None


def test_tap_bpm_very_slow_tempo_resets_instead_of_computing(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    sample_id = 0
    times = iter([0.0, 100.0, 200.0])
    mp_target = "flitzis_looper.controller.transport.bpm.monotonic"
    monkeypatch.setattr(mp_target, lambda: next(times))

    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert controller.transport.bpm.tap_bpm(sample_id) is None
    assert controller.project.manual_bpm[sample_id] is None


def test_recompute_master_bpm_unlocked(controller: AppController, audio_engine_mock: Mock) -> None:
    controller.project.bpm_lock = False
    controller.session.master_bpm = 120.0

    controller.transport.bpm.recompute_master_bpm()

    assert controller.session.master_bpm is None
    audio_engine_mock.anchor_transport_phase_from_pad.assert_not_called()


def test_recompute_master_bpm_none_anchor(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.bpm_lock = True
    controller.session.bpm_lock_anchor_bpm = None
    controller.session.master_bpm = 120.0

    controller.transport.bpm.recompute_master_bpm()

    assert controller.session.master_bpm is None
    audio_engine_mock.anchor_transport_phase_from_pad.assert_not_called()


def test_recompute_master_bpm_non_finite_anchor(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.bpm_lock = True
    controller.session.bpm_lock_anchor_bpm = float("nan")
    controller.session.master_bpm = 120.0

    controller.transport.bpm.recompute_master_bpm()

    assert controller.session.master_bpm is None
    audio_engine_mock.anchor_transport_phase_from_pad.assert_not_called()


def test_on_pad_bpm_changed_updates_audio(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.manual_bpm[sample_id] = 120.0

    controller.transport.bpm.on_pad_bpm_changed(sample_id)

    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, 120.0)
    audio_engine_mock.set_pad_timing_metadata.assert_called_with(sample_id, 0.0)


def test_on_pad_bpm_changed_skips_unloaded_pad_timing_metadata(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 44_100
    audio_engine_mock.set_pad_timing_metadata.side_effect = ValueError(
        "phase_anchor_s out of range"
    )
    sample_id = 0
    controller.project.pad_grid_offset_samples[sample_id] = -1

    controller.transport.bpm.on_pad_bpm_changed(sample_id)

    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, None)
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_on_pad_bpm_changed_publishes_downbeat_timing_metadata(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[1.0], downbeats=[2.0], bars=[2.0]),
    )

    controller.transport.bpm.on_pad_bpm_changed(sample_id)

    audio_engine_mock.set_pad_timing_metadata.assert_called_with(sample_id, 2.0)


def test_on_pad_bpm_changed_publishes_shifted_grid_anchor(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.pad_grid_offset_samples[sample_id] = 480
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[1.0], downbeats=[2.0], bars=[2.0]),
    )

    controller.transport.bpm.on_pad_bpm_changed(sample_id)

    audio_engine_mock.set_pad_timing_metadata.assert_called_with(sample_id, 2.01)


def test_on_pad_bpm_changed_publishes_beat_fallback_timing_metadata(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[1.0], downbeats=[], bars=[]),
    )

    controller.transport.bpm.on_pad_bpm_changed(sample_id)

    audio_engine_mock.set_pad_timing_metadata.assert_called_with(sample_id, 1.0)


def test_on_pad_bpm_changed_publishes_zero_for_invalid_timing_metadata(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[float("inf")], downbeats=[float("nan")], bars=[]),
    )

    controller.transport.bpm.on_pad_bpm_changed(sample_id)

    audio_engine_mock.set_pad_timing_metadata.assert_called_with(sample_id, 0.0)


def test_on_pad_bpm_changed_updates_master_bpm(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    controller.project.bpm_lock = True
    controller.session.bpm_lock_anchor_pad_id = sample_id
    controller.session.bpm_lock_anchor_bpm = None
    controller.project.speed = 1.0
    controller.project.manual_bpm[sample_id] = 120.0

    controller.transport.bpm.on_pad_bpm_changed(sample_id)

    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, 120.0)
    audio_engine_mock.set_master_bpm.assert_called_with(120.0)
    audio_engine_mock.anchor_transport_phase_from_pad.assert_not_called()


def test_on_pad_bpm_changed_not_anchor(controller: AppController, audio_engine_mock: Mock) -> None:
    sample_id = 0
    anchor_sample_id = 1
    controller.project.bpm_lock = True
    controller.session.bpm_lock_anchor_pad_id = anchor_sample_id
    controller.session.bpm_lock_anchor_bpm = 100.0
    controller.project.speed = 1.0
    controller.project.manual_bpm[sample_id] = 120.0

    controller.transport.bpm.on_pad_bpm_changed(sample_id)

    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, 120.0)
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.anchor_transport_phase_from_pad.assert_not_called()
