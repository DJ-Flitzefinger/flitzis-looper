import math
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.transport.loop import PadLoopController
from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def test_reset_loop_region_uses_first_downbeat_and_auto_defaults(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0, 18.0], downbeats=[10.0], bars=[10.0]),
    )

    controller.transport.loop.reset(sample_id)

    assert controller.project.pad_loop_auto[sample_id] is True
    assert controller.project.pad_loop_bars[sample_id] == 4
    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(10.0)
    assert controller.project.pad_loop_end_s[sample_id] == pytest.approx(18.0)


def test_set_loop_start_snaps_when_auto_enabled(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[1.0, 2.0], downbeats=[0.0], bars=[0.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=True)
    controller.transport.loop.set_start(sample_id, 1.04)

    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(1.0)


def test_set_loop_start_does_not_snap_when_auto_disabled(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 100

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[1.0, 2.0], downbeats=[0.0], bars=[0.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=False)
    controller.transport.loop.set_start(sample_id, 1.04)

    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(1.04)


def test_effective_loop_end_computed_from_bars(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_bars[sample_id] = 4
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)
    controller.transport.loop.set_start(sample_id, 10.0)

    start_s, end_s = controller.transport.loop.effective_region(sample_id)

    assert start_s == pytest.approx(10.0)
    assert end_s == pytest.approx(18.0)


def test_effective_loop_end_uses_effective_bpm_not_beat_grid(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=90.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0, 18.0002], downbeats=[10.0], bars=[10.0]),
    )
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_bars[sample_id] = 4
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)

    controller.transport.loop.set_start(sample_id, 10.0)

    start_s, end_s = controller.transport.loop.effective_region(sample_id)

    assert start_s == pytest.approx(10.0)
    assert end_s == pytest.approx(18.0)


def test_reset_no_analysis(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = None

    controller.transport.loop.reset(sample_id)

    assert controller.project.pad_loop_auto[sample_id] is True
    assert controller.project.pad_loop_bars[sample_id] == 4
    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(0.0)
    assert controller.project.pad_loop_end_s[sample_id] is None


def test_reset_no_beats(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]),
    )

    controller.transport.loop.reset(sample_id)

    assert controller.project.pad_loop_auto[sample_id] is True
    assert controller.project.pad_loop_bars[sample_id] == 4


def test_reset_quantizes_to_samples(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0], downbeats=[10.0], bars=[10.0]),
    )

    controller.transport.loop.reset(sample_id)

    start_s = controller.project.pad_loop_start_s[sample_id]
    assert start_s == pytest.approx(10.0)


def test_set_auto_enable_snaps_start(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0, 1.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.pad_loop_start_s[sample_id] = 0.7

    controller.transport.loop.set_auto(sample_id, enabled=True)

    assert controller.project.pad_loop_auto[sample_id] is True
    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(1.0)


def test_set_auto_disable_no_change(controller: AppController, audio_engine_mock: Mock) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0, 1.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.pad_loop_start_s[sample_id] = 0.5
    controller.project.pad_loop_auto[sample_id] = False

    controller.transport.loop.set_auto(sample_id, enabled=False)

    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(0.5)


def test_set_auto_no_op(controller: AppController) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.pad_loop_auto[sample_id] = True

    controller.transport.loop.set_auto(sample_id, enabled=True)

    assert controller.project.pad_loop_auto[sample_id] is True


def test_set_bars_clamps_to_one(controller: AppController, audio_engine_mock: Mock) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )

    controller.transport.loop.set_bars(sample_id, bars=0)

    assert controller.project.pad_loop_bars[sample_id] == 1


def test_set_bars_no_op(controller: AppController, audio_engine_mock: Mock) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.pad_loop_bars[sample_id] = 4

    controller.transport.loop.set_bars(sample_id, bars=4)

    assert controller.project.pad_loop_bars[sample_id] == 4


def test_set_start_negative_clamps(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0, 1.0], downbeats=[0.0], bars=[0.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=False)
    controller.transport.loop.set_start(sample_id, -10.0)

    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(0.0)


def test_set_start_quantizes(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0, 1.0], downbeats=[0.0], bars=[0.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=False)
    controller.transport.loop.set_start(sample_id, 1.04)

    start_s = controller.project.pad_loop_start_s[sample_id]
    frames = start_s * 48_000
    assert frames == pytest.approx(1.04 * 48_000, 0.5)
    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(1.04, 0.01)


def test_set_end_none(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.pad_loop_end_s[sample_id] = 10.0

    controller.transport.loop.set_end(sample_id, None)

    assert controller.project.pad_loop_end_s[sample_id] is None


def test_set_end_clears_when_past_start(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.pad_loop_start_s[sample_id] = 10.0

    controller.transport.loop.set_end(sample_id, 5.0)

    end_s = controller.project.pad_loop_end_s[sample_id]
    assert end_s is not None, "End should not be None"
    assert end_s > 10.0, "End should be adjusted past start"
    assert end_s <= 10.0001, "End should be start + one sample"


def test_set_end_quantizes(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.pad_loop_start_s[sample_id] = 0.0

    controller.transport.loop.set_end(sample_id, 5.04)

    end_s = controller.project.pad_loop_end_s[sample_id]
    assert end_s is not None, "End should not be None"
    frames = end_s * 48_000
    assert frames == pytest.approx(5.04 * 48_000, 0.5)
    assert end_s == pytest.approx(5.04, 0.01)


def test_set_end_non_finite_raises(controller: AppController) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"

    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.loop.set_end(sample_id, math.nan)


def test_effective_region_manual_mode(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.pad_loop_auto[sample_id] = False
    controller.project.pad_loop_start_s[sample_id] = 5.0
    controller.project.pad_loop_end_s[sample_id] = 15.0

    start_s, end_s = controller.transport.loop.effective_region(sample_id)

    assert start_s == pytest.approx(5.0)
    assert end_s == pytest.approx(15.0)


def test_effective_region_auto_no_bpm(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0, 2.0, 4.0, 6.0, 8.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_start_s[sample_id] = 0.0
    controller.project.pad_loop_end_s[sample_id] = None
    controller.project.pad_loop_bars[sample_id] = 4
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)

    start_s, end_s = controller.transport.loop.effective_region(sample_id)

    assert start_s == pytest.approx(0.0)
    assert end_s == pytest.approx(8.0)


def test_effective_region_auto_no_beats(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]),
    )
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_start_s[sample_id] = 0.0
    controller.project.pad_loop_bars[sample_id] = 4
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)

    start_s, end_s = controller.transport.loop.effective_region(sample_id)

    assert start_s == pytest.approx(0.0)
    assert end_s == pytest.approx(8.0)


def test_snap_to_nearest_beat_empty(controller: AppController) -> None:
    target_s = 5.0
    beats: list[float] = []

    result = PadLoopController._snap_to_nearest_beat(target_s, beats)

    assert result == pytest.approx(5.0)


def test_snap_to_nearest_beat_with_beats(controller: AppController) -> None:
    target_s = 1.5
    beats = [1.0, 2.0, 3.0]

    result = PadLoopController._snap_to_nearest_beat(target_s, beats)

    assert result == pytest.approx(1.0)


def test_quantize_time_none_sample_rate(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = None

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.transport.loop.reset(sample_id)

    start_s = controller.project.pad_loop_start_s[sample_id]
    assert start_s == pytest.approx(0.0)


def test_quantize_time_invalid_sample_rate(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 0

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.transport.loop.reset(sample_id)

    start_s = controller.project.pad_loop_start_s[sample_id]
    assert start_s == pytest.approx(0.0)


def test_apply_effective_region_not_loaded(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = None

    controller.transport.loop.reset(sample_id)

    audio_engine_mock.set_pad_loop_region.assert_not_called()
