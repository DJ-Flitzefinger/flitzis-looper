import math
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.transport.loop import PadLoopController
from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _configure_source_stable_grid_pad(
    controller: AppController,
    audio_engine_mock: Mock,
    sample_id: int,
) -> tuple[float, float]:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[sample_id] = f"samples/pad-{sample_id}.wav"
    controller.project.sample_durations[sample_id] = 40.0
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0], downbeats=[10.0], bars=[10.0]),
    )
    controller.project.pad_loop_auto[sample_id] = True

    controller.transport.loop.set_grid_offset_samples(sample_id, 240)
    controller.transport.loop.set_start(sample_id, 10.0)

    anchor_s = 10.005
    snapped_start_s = 10.005
    assert controller.transport.loop.grid_anchor_sec(sample_id) == pytest.approx(anchor_s)
    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(snapped_start_s)
    return (anchor_s, snapped_start_s)


def _assert_source_stable_grid_pad(
    controller: AppController,
    sample_id: int,
    *,
    anchor_s: float,
    snapped_start_s: float,
) -> None:
    assert controller.transport.loop.grid_anchor_sec(sample_id) == pytest.approx(anchor_s)
    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(snapped_start_s)
    effective_start_s, _ = controller.transport.loop.effective_region(sample_id)
    assert effective_start_s == pytest.approx(snapped_start_s)


def test_initialize_loaded_pad_defaults_uses_track_start_and_eight_bars(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_durations[sample_id] = 32.0
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0, 18.0], downbeats=[10.0], bars=[10.0]),
    )

    controller.transport.loop.initialize_loaded_pad_defaults(sample_id)

    assert controller.project.pad_loop_auto[sample_id] is True
    assert controller.project.pad_loop_bars[sample_id] == 8.0
    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(0.0)
    assert controller.project.pad_loop_end_s[sample_id] is None
    assert controller.transport.loop.effective_region(sample_id) == pytest.approx((0.0, 16.0))
    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, 0.0, 16.0)


def test_set_loop_start_snaps_to_64th_grid_and_quantizes_to_samples_when_auto_enabled(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    # Spec scenario: BPM=120 -> grid step is 0.03125s (1/32).
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0], downbeats=[10.0], bars=[10.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=True)
    controller.transport.loop.set_start(sample_id, 10.031)

    start_s = controller.project.pad_loop_start_s[sample_id]
    assert start_s == 10.03125
    assert start_s * 48_000 == 481_500


def test_set_loop_end_snaps_to_64th_grid_and_quantizes_to_samples_when_auto_enabled(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    # Spec scenario: BPM=120, anchor=10.0s.
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0], downbeats=[10.0], bars=[10.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=True)
    controller.transport.loop.set_start(sample_id, 10.0)
    controller.transport.loop.set_end(sample_id, 10.062)

    end_s = controller.project.pad_loop_end_s[sample_id]
    assert end_s is not None
    assert end_s == 10.0625
    assert end_s * 48_000 == 483_000


def test_set_loop_start_does_not_snap_when_auto_disabled(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 128

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[1.0 / 128.0], bars=[0.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=False)
    controller.transport.loop.set_start(sample_id, 1.0 / 32.0)

    start_s = controller.project.pad_loop_start_s[sample_id]
    assert start_s == 1.0 / 32.0
    assert start_s * 128 == 4


def test_set_loop_start_snaps_using_default_onset_anchor_when_auto_enabled(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 128

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[1.0 / 128.0], bars=[0.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=True)
    controller.transport.loop.set_start(sample_id, 1.0 / 32.0)

    start_s = controller.project.pad_loop_start_s[sample_id]
    assert start_s == 5.0 / 128.0
    assert start_s * 128 == 5


def test_set_loop_start_snaps_using_shifted_anchor_and_is_sample_accurate(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0], downbeats=[10.0], bars=[10.0]),
    )
    controller.project.pad_grid_offset_samples[sample_id] = 1

    controller.transport.loop.set_auto(sample_id, enabled=True)
    controller.transport.loop.set_start(sample_id, 10.0)

    start_s = controller.project.pad_loop_start_s[sample_id]
    assert start_s == pytest.approx(480_001 / 48_000)
    assert round(start_s * 48_000) == 480_001


def test_set_grid_offset_samples_clamps_to_one_bar_worth_of_samples(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)

    controller.transport.loop.set_grid_offset_samples(sample_id, 100_000)

    assert controller.project.pad_grid_offset_samples[sample_id] == 96_000


def test_set_grid_offset_samples_publishes_shifted_grid_anchor(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0], downbeats=[10.0], bars=[10.0]),
    )

    controller.transport.loop.set_grid_offset_samples(sample_id, 240)

    audio_engine_mock.set_pad_timing_metadata.assert_called_with(sample_id, 10.005)
    audio_engine_mock.set_pad_loop_region.assert_called()


def test_apply_grid_anchor_to_audio_clamps_negative_anchor(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.pad_grid_offset_samples[sample_id] = -1

    controller.transport.loop.apply_grid_anchor_to_audio(sample_id)

    audio_engine_mock.set_pad_timing_metadata.assert_called_once_with(sample_id, 0.0)


def test_apply_grid_anchor_to_audio_skips_unloaded_pad(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = None
    controller.project.pad_grid_offset_samples[sample_id] = -1

    controller.transport.loop.apply_grid_anchor_to_audio(sample_id)

    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_effective_bpm_change_reclamps_grid_offset_samples(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)
    controller.transport.loop.set_grid_offset_samples(sample_id, 96_000)
    assert controller.project.pad_grid_offset_samples[sample_id] == 96_000

    controller.transport.bpm.set_manual_bpm(sample_id, 240.0)

    assert controller.project.pad_grid_offset_samples[sample_id] == 48_000


def test_grid_anchor_and_snapped_start_stay_stable_under_global_modes(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    sample_id = 0
    controller.project.selected_pad = sample_id
    anchor_s, snapped_start_s = _configure_source_stable_grid_pad(
        controller, audio_engine_mock, sample_id
    )
    audio_engine_mock.reset_mock()

    actions = [
        lambda: controller.transport.global_params.set_speed(1.5),
        lambda: controller.transport.global_params.set_bpm_lock(enabled=True),
        lambda: controller.transport.global_params.set_key_lock(enabled=True),
        lambda: controller.transport.global_params.set_trigger_quantization_enabled(enabled=True),
        lambda: controller.transport.global_params.set_trigger_quantization_step("1_32"),
        lambda: controller.transport.global_params.set_trigger_quantization_step("1_64"),
        controller.transport.bpm.recompute_master_bpm,
        lambda: controller.transport.global_params.set_speed(1.25),
        lambda: controller.transport.global_params.set_key_lock(enabled=False),
        lambda: controller.transport.global_params.set_bpm_lock(enabled=False),
    ]

    for action in actions:
        action()
        _assert_source_stable_grid_pad(
            controller,
            sample_id,
            anchor_s=anchor_s,
            snapped_start_s=snapped_start_s,
        )

    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_grid_anchor_and_snapped_start_stay_stable_when_other_pad_plays(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    sample_id = 0
    other_id = 1
    anchor_s, snapped_start_s = _configure_source_stable_grid_pad(
        controller, audio_engine_mock, sample_id
    )
    controller.project.sample_paths[other_id] = "samples/other.wav"
    controller.project.sample_durations[other_id] = 8.0
    controller.project.sample_analysis[other_id] = SampleAnalysis(
        bpm=100.0,
        key="G",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.multi_loop = True
    audio_engine_mock.reset_mock()

    controller.transport.playback.trigger_pad(other_id)
    _assert_source_stable_grid_pad(
        controller,
        sample_id,
        anchor_s=anchor_s,
        snapped_start_s=snapped_start_s,
    )

    controller.session.active_sample_ids.add(other_id)
    controller.transport.playback.stop_pad(other_id)
    _assert_source_stable_grid_pad(
        controller,
        sample_id,
        anchor_s=anchor_s,
        snapped_start_s=snapped_start_s,
    )

    controller.transport.playback.trigger_pad(other_id)
    _assert_source_stable_grid_pad(
        controller,
        sample_id,
        anchor_s=anchor_s,
        snapped_start_s=snapped_start_s,
    )

    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_snapping_uses_effective_bpm_manual_override_over_analysis(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=60.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)

    controller.transport.loop.set_auto(sample_id, enabled=True)
    controller.transport.loop.set_start(sample_id, 0.04)

    start_s = controller.project.pad_loop_start_s[sample_id]
    assert start_s == 0.03125
    assert start_s * 48_000 == 1_500


def test_effective_loop_end_computed_from_bars(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_bars[sample_id] = 4.0
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
    controller.project.pad_loop_bars[sample_id] = 4.0
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)

    controller.transport.loop.set_start(sample_id, 10.0)

    start_s, end_s = controller.transport.loop.effective_region(sample_id)

    assert start_s == pytest.approx(10.0)
    assert end_s == pytest.approx(18.0)


def test_set_full_track_region_disables_auto_and_publishes_duration(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_durations[sample_id] = 42.0
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_bars[sample_id] = 8.0
    controller.project.pad_loop_start_s[sample_id] = 10.0
    controller.project.pad_loop_end_s[sample_id] = None

    controller.transport.loop.reset(sample_id)

    assert controller.project.pad_loop_auto[sample_id] is False
    assert controller.project.pad_loop_bars[sample_id] == 8.0
    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(0.0)
    assert controller.project.pad_loop_end_s[sample_id] == pytest.approx(42.0)
    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, 0.0, 42.0)


def test_set_full_track_region_no_ops_without_loaded_duration(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_durations[sample_id] = None
    controller.project.pad_loop_start_s[sample_id] = 5.0
    controller.project.pad_loop_end_s[sample_id] = 10.0
    controller.project.pad_loop_auto[sample_id] = False

    controller.transport.loop.reset(sample_id)

    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(5.0)
    assert controller.project.pad_loop_end_s[sample_id] == pytest.approx(10.0)
    assert controller.project.pad_loop_auto[sample_id] is False
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_initialize_loaded_pad_defaults_ignores_analysis_onset(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[2.0], downbeats=[float("nan")], bars=[]),
    )

    controller.transport.loop.initialize_loaded_pad_defaults(sample_id)

    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(0.0)
    assert controller.project.pad_loop_bars[sample_id] == 8.0


def test_set_full_track_region_quantizes_end_to_samples(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_durations[sample_id] = 1.0 / 48_000

    controller.transport.loop.reset(sample_id)

    assert controller.project.pad_loop_start_s[sample_id] == pytest.approx(0.0)
    assert controller.project.pad_loop_end_s[sample_id] == pytest.approx(1.0 / 48_000)


def test_set_auto_enable_snaps_start(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.pad_loop_start_s[sample_id] = 0.04
    controller.project.pad_loop_auto[sample_id] = False

    controller.transport.loop.set_auto(sample_id, enabled=True)

    assert controller.project.pad_loop_auto[sample_id] is True

    start_s = controller.project.pad_loop_start_s[sample_id]
    assert start_s == 0.03125
    assert start_s * 48_000 == 1_500


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


def test_set_bars_accepts_half_bar(controller: AppController, audio_engine_mock: Mock) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_durations[sample_id] = 10.0
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )

    controller.transport.loop.set_bars(sample_id, bars=0.5)
    start_s, end_s = controller.transport.loop.effective_region(sample_id)

    assert controller.project.pad_loop_bars[sample_id] == 0.5
    assert start_s == pytest.approx(0.0)
    assert end_s == pytest.approx(1.0)


def test_set_bars_rejects_below_minimum(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"

    with pytest.raises(ValueError, match="bars must be >="):
        controller.transport.loop.set_bars(sample_id, bars=0.0)

    assert controller.project.pad_loop_bars[sample_id] == 8.0


def test_set_bars_no_ops_when_requested_value_cannot_fit(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_durations[sample_id] = 10.0
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.pad_loop_bars[sample_id] = 4.0
    audio_engine_mock.reset_mock()

    controller.transport.loop.set_bars(sample_id, bars=8.0)

    assert controller.project.pad_loop_bars[sample_id] == 4.0
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_max_auto_loop_bars_uses_remaining_duration_and_effective_bpm(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 1_000

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_durations[sample_id] = 10.0
    controller.project.pad_loop_start_s[sample_id] = 2.0
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)

    assert controller.transport.loop.max_auto_loop_bars(sample_id) == pytest.approx(4.0)


def test_set_bars_no_op(controller: AppController, audio_engine_mock: Mock) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.pad_loop_bars[sample_id] = 4.0

    controller.transport.loop.set_bars(sample_id, bars=4.0)

    assert controller.project.pad_loop_bars[sample_id] == 4.0


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
    controller.project.pad_loop_bars[sample_id] = 4.0
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
    controller.project.pad_loop_bars[sample_id] = 4.0
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)

    start_s, end_s = controller.transport.loop.effective_region(sample_id)

    assert start_s == pytest.approx(0.0)
    assert end_s == pytest.approx(8.0)


def test_grid_step_sec_for_120_bpm_is_one_over_32() -> None:
    assert PadLoopController._grid_step_sec(120.0) == 1.0 / 32.0


def test_snap_to_nearest_grid_point_rounds_to_nearest_step_with_anchor() -> None:
    step_s = 1.0 / 32.0
    anchor_s = 1.0 / 128.0

    # (target - anchor) / step = 0.75 -> rounds to 1 step
    target_s = 1.0 / 32.0
    snapped = PadLoopController._snap_to_nearest_grid_point(
        target_s, anchor_s=anchor_s, step_s=step_s
    )

    assert snapped == 5.0 / 128.0


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
