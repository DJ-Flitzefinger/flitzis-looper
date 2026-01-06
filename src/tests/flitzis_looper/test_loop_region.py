from typing import TYPE_CHECKING

import pytest

from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import LooperController


def test_reset_loop_region_uses_first_downbeat_and_auto_defaults(
    controller: LooperController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.return_value.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0, 18.0], downbeats=[10.0]),
    )

    controller.transport.loop.reset(sample_id)

    assert controller.project.pad_loop_auto[sample_id] is True
    assert controller.project.pad_loop_bars[sample_id] == 4
    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(10.0)
    assert controller.project.pad_loop_end_s[sample_id] == pytest.approx(18.0)


def test_set_loop_start_snaps_when_auto_enabled(
    controller: LooperController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.return_value.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[1.0, 2.0], downbeats=[0.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=True)
    controller.transport.loop.set_start(sample_id, 1.04)

    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(1.0)


def test_set_loop_start_does_not_snap_when_auto_disabled(
    controller: LooperController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.return_value.output_sample_rate.return_value = 100

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[1.0, 2.0], downbeats=[0.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=False)
    controller.transport.loop.set_start(sample_id, 1.04)

    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(1.04)


def test_effective_loop_end_computed_from_bars(
    controller: LooperController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.return_value.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_bars[sample_id] = 4
    controller.transport.set_manual_bpm(sample_id, 120.0)
    controller.transport.loop.set_start(sample_id, 10.0)

    start_s, end_s = controller.transport.loop.effective_region(sample_id)

    assert start_s == pytest.approx(10.0)
    assert end_s == pytest.approx(18.0)
