import math
from decimal import Decimal
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.current_timing import CurrentPadTiming
from flitzis_looper.models import BeatGrid, SampleAnalysis
from tests.flitzis_looper.conftest import current_timing_metadata

if TYPE_CHECKING:
    from collections.abc import Callable
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


@pytest.mark.parametrize("start_frame", [0, 1_151, 480_153])
def test_initialize_loaded_pad_defaults_aligns_grid_and_loop_to_activity_boundary(
    controller: AppController,
    audio_engine_mock: Mock,
    start_frame: int,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_durations[sample_id] = 32.0
    controller.project.manual_bpm[sample_id] = 123.456
    controller.project.pad_grid_offset_samples[sample_id] = -480
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    start_s = start_frame / 48_000

    controller.transport.loop.initialize_loaded_pad_defaults(sample_id, start_s)

    assert controller.project.pad_loop_start_s[sample_id] == start_s
    assert controller.project.pad_loop_auto[sample_id] is True
    assert controller.project.pad_loop_bars[sample_id] == 8.0
    assert controller.project.manual_bpm[sample_id] == 123.456
    assert controller.project.pad_grid_anchor_s[sample_id] == start_s
    assert controller.project.pad_grid_offset_samples[sample_id] == 0
    assert controller.transport.loop.grid_anchor_sec(sample_id) == start_s
    expected_end_s = round((start_s + 32 * 60 / 123.456) * 48_000) / 48_000
    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, start_s, expected_end_s)
    audio_engine_mock.set_pad_timing_metadata.assert_called_with(sample_id, start_s)


def test_initialize_loaded_pad_defaults_uses_activity_candidate_without_bpm(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_durations[sample_id] = 3.0
    start_s = 47_777 / 48_000

    controller.transport.loop.initialize_loaded_pad_defaults(sample_id, start_s)

    assert controller.project.pad_loop_start_s[sample_id] == start_s
    assert controller.project.pad_loop_auto[sample_id] is True
    assert controller.project.pad_loop_bars[sample_id] == 8.0
    assert controller.transport.loop.effective_region(sample_id) == (start_s, None)
    assert controller.project.pad_grid_anchor_s[sample_id] == start_s
    assert controller.transport.loop.grid_anchor_sec(sample_id) == start_s
    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, start_s, None)


@pytest.mark.parametrize("candidate_s", [None, -0.1, math.nan, math.inf, 3.0, 4.0])
def test_initialize_loaded_pad_defaults_falls_back_for_unusable_activity_candidate(
    controller: AppController,
    audio_engine_mock: Mock,
    candidate_s: float | None,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.project.sample_durations[0] = 3.0
    controller.project.pad_grid_anchor_s[0] = 1.0
    controller.project.pad_grid_offset_samples[0] = 480
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.5], downbeats=[0.5], bars=[0.5]),
    )

    controller.transport.loop.initialize_loaded_pad_defaults(0, candidate_s)

    assert controller.project.pad_loop_start_s[0] == 0.0
    assert controller.project.pad_loop_auto[0] is True
    assert controller.project.pad_loop_bars[0] == 8.0
    assert controller.project.pad_grid_anchor_s[0] is None
    assert controller.project.pad_grid_offset_samples[0] == 0
    assert controller.transport.loop.grid_anchor_sec(0) == 0.5


def test_activity_grid_base_survives_bpm_clamp_and_later_loop_edits(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.project.sample_durations[0] = 60.0
    base_s = 480_153 / 48_000
    controller.transport.loop.initialize_loaded_pad_defaults(0, base_s)
    controller.transport.bpm.set_manual_bpm(0, 120.0)
    controller.transport.loop.set_grid_offset_samples(0, 96_000)

    controller.transport.bpm.set_manual_bpm(0, 240.0)

    assert controller.project.pad_grid_anchor_s[0] == base_s
    assert controller.project.pad_grid_offset_samples[0] == 48_000
    assert controller.transport.loop.grid_anchor_sec(0) == base_s + 1.0
    assert controller.project.pad_loop_start_s[0] == base_s
    audio_engine_mock.set_pad_timing_metadata.assert_called_with(0, base_s + 1.0)

    controller.transport.loop.set_start(0, base_s + 4.0)

    assert controller.project.pad_loop_start_s[0] == base_s + 4.0
    assert controller.project.pad_grid_anchor_s[0] == base_s
    assert controller.transport.loop.grid_anchor_sec(0) == base_s + 1.0


def test_activity_grid_supports_negative_signed_offset_without_near_start_snap(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.project.sample_durations[0] = 3.0
    controller.transport.loop.initialize_loaded_pad_defaults(0, 1_151 / 48_000)

    controller.transport.loop.set_grid_offset_samples(0, -1_200)

    assert controller.project.pad_grid_anchor_s[0] == 1_151 / 48_000
    assert controller.transport.loop.grid_anchor_sec(0) == -49 / 48_000
    audio_engine_mock.set_pad_timing_metadata.assert_called_with(0, -49 / 48_000)


@pytest.mark.parametrize("anchor_s", [math.nan, math.inf, -0.1])
def test_invalid_in_place_grid_base_uses_legacy_analysis_fallback(
    controller: AppController,
    audio_engine_mock: Mock,
    anchor_s: float,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.project.pad_grid_anchor_s[0] = anchor_s
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.5], downbeats=[0.5], bars=[0.5]),
    )

    controller.transport.loop.apply_grid_anchor_to_audio(0)

    assert controller.transport.loop.grid_anchor_sec(0) == 0.5
    audio_engine_mock.set_pad_timing_metadata.assert_called_once_with(0, 0.5)


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
        beat_grid=BeatGrid(beats=[0.0], downbeats=[16.0 / 128.0], bars=[0.0]),
    )

    controller.transport.loop.set_auto(sample_id, enabled=True)
    controller.transport.loop.set_start(sample_id, 21.0 / 128.0)

    start_s = controller.project.pad_loop_start_s[sample_id]
    assert start_s == 20.0 / 128.0
    assert start_s * 128 == 20


def test_near_start_analysis_anchor_snaps_to_track_start(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 44_100

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=90.0,
        key="C",
        beat_grid=BeatGrid(
            beats=[1536.0 / 44_100.0],
            downbeats=[1536.0 / 44_100.0],
            bars=[1536.0 / 44_100.0],
        ),
    )

    assert controller.transport.loop.grid_anchor_sec(sample_id) == pytest.approx(0.0)

    controller.transport.loop.apply_grid_anchor_to_audio(sample_id)

    audio_engine_mock.set_pad_timing_metadata.assert_called_once_with(sample_id, 0.0)


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


@pytest.mark.parametrize("sample_rate_hz", [44_100, 48_000])
@pytest.mark.parametrize("offset_samples", [-1, -1_500])
def test_apply_grid_anchor_to_audio_preserves_negative_editor_origin(
    controller: AppController,
    audio_engine_mock: Mock,
    sample_rate_hz: int,
    offset_samples: int,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = sample_rate_hz

    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.pad_grid_offset_samples[sample_id] = offset_samples

    controller.transport.loop.apply_grid_anchor_to_audio(sample_id)

    expected_origin_s = offset_samples / sample_rate_hz
    assert controller.transport.loop.grid_anchor_sec(sample_id) == expected_origin_s
    audio_engine_mock.set_pad_timing_metadata.assert_called_once_with(sample_id, expected_origin_s)


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


def test_negative_grid_origin_drives_editor_snapping_and_native_metadata(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/foo.wav"
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_bars[sample_id] = 8.0
    controller.transport.bpm.set_manual_bpm(sample_id, 120.0)

    controller.transport.loop.set_grid_offset_samples(sample_id, -240)
    controller.transport.loop.set_start(sample_id, 0.031)

    assert controller.transport.loop.grid_anchor_sec(sample_id) == -0.005
    assert controller.project.pad_loop_start_s[sample_id] == 1_260 / 48_000
    audio_engine_mock.set_pad_timing_metadata.assert_called_with(sample_id, -0.005)
    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, 1_260 / 48_000, 16.02625)


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

    actions: list[Callable[[], None]] = [
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

    audio_engine_mock.set_pad_timing_metadata.assert_called_once_with(sample_id, anchor_s)


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


def test_manual_120_snap_and_auto_end_match_every_reference_pulse(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/reference.wav"
    controller.project.sample_durations[0] = 600.0
    controller.project.manual_bpm[0] = 120.0
    controller.project.pad_grid_anchor_s[0] = 0.0
    controller.project.pad_loop_auto[0] = True
    controller.project.pad_loop_bars[0] = 0.5

    for pulse in range(1_200):
        # Pulse locations come from the independently known constant PCM blocks.
        expected_frame = pulse * 24_000
        controller.transport.loop.set_start(0, expected_frame / 48_000 + 1e-6)
        start_s, end_s = controller.transport.loop.effective_region(0)
        assert start_s * 48_000 == expected_frame
        assert end_s is not None
        assert end_s * 48_000 == (pulse + 2) * 24_000


@pytest.mark.parametrize("sample_rate_hz", [44_100, 48_000, 96_000])
@pytest.mark.parametrize("bpm", [119.999, 123.45])
@pytest.mark.parametrize("origin_frame", [-217, 0, 28_776_001])
def test_fractional_auto_end_and_snap_share_absolute_projection(
    controller: AppController,
    audio_engine_mock: Mock,
    sample_rate_hz: int,
    bpm: float,
    origin_frame: int,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = sample_rate_hz
    controller.project.sample_paths[0] = "samples/fractional.wav"
    controller.project.sample_durations[0] = 1_800.0
    controller.project.manual_bpm[0] = bpm
    controller.project.pad_grid_anchor_s[0] = max(origin_frame, 0) / sample_rate_hz
    controller.project.pad_grid_offset_samples[0] = min(origin_frame, 0)
    controller.project.pad_loop_auto[0] = True
    controller.project.pad_loop_bars[0] = 0.5
    raw_start_s = 599.5 + 0.49 / sample_rate_hz
    controller.project.pad_loop_start_s[0] = raw_start_s

    start_s, end_s = controller.transport.loop.effective_region(0)

    assert end_s is not None
    exact_start_frame = Decimal(raw_start_s) * sample_rate_hz
    exact_end_frame = exact_start_frame + Decimal(120 * sample_rate_hz) / Decimal(str(bpm))
    assert round(start_s * sample_rate_hz) == round(exact_start_frame)
    assert round(end_s * sample_rate_hz) == round(exact_end_frame)
    assert abs(Decimal(round(end_s * sample_rate_hz)) - exact_end_frame) <= Decimal("0.5")
    assert controller.project.pad_loop_start_s[0] == raw_start_s
    assert controller.transport.loop.grid_anchor_sec(0) == origin_frame / sample_rate_hz

    exact_tick_frame = Decimal(origin_frame) + Decimal(19_185 * 60 * sample_rate_hz) / (
        Decimal(str(bpm)) * 16
    )
    controller.transport.loop.set_start(0, float(exact_tick_frame / sample_rate_hz))
    assert round(controller.project.pad_loop_start_s[0] * sample_rate_hz) == round(exact_tick_frame)


def test_legacy_fractional_start_does_not_round_before_advancing_auto_end(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.manual_bpm[0] = 119.999
    controller.project.pad_loop_auto[0] = True
    controller.project.pad_loop_bars[0] = 0.5
    controller.project.pad_loop_start_s[0] = 599.5 + 0.49 / 48_000

    start_s, end_s = controller.transport.loop.effective_region(0)

    assert start_s == 28_776_000 / 48_000
    assert end_s == 28_824_001 / 48_000


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


@pytest.mark.parametrize("sample_rate_hz", [44_100, 48_000])
def test_long_source_markers_publish_exact_frames_to_audio_and_direct_input(
    controller: AppController, audio_engine_mock: Mock, sample_rate_hz: int
) -> None:
    audio_engine_mock.output_sample_rate.return_value = sample_rate_hz
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/long-source.wav"
    controller.project.pad_loop_auto[sample_id] = False
    start_frame = 16_777_217
    end_frame = start_frame + 17
    start_s = start_frame / sample_rate_hz
    end_s = end_frame / sample_rate_hz

    controller.transport.loop.set_start(sample_id, start_s)
    controller.transport.loop.set_end(sample_id, end_s)
    controller.input_mapping.on_frame_render()

    assert controller.project.pad_loop_start_s[sample_id] == start_s
    assert controller.project.pad_loop_end_s[sample_id] == end_s
    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, start_s, end_s)
    _, loaded, loop_starts, loop_ends = audio_engine_mock.set_input_runtime_state.call_args.args
    assert loaded[sample_id] is True
    assert loop_starts[sample_id] == start_s
    assert loop_ends[sample_id] == end_s
    assert round(loop_starts[sample_id] * sample_rate_hz) == start_frame
    assert round(loop_ends[sample_id] * sample_rate_hz) == end_frame


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


def test_accepted_loop_projection_uses_signed_origin_period_and_loaded_rate(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.sample_durations[0] = 60.0
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=90.0,
        key="C",
        beat_grid=BeatGrid(beats=[10.0], downbeats=[10.0], bars=[10.0]),
    )
    controller.project.pad_grid_anchor_s[0] = 10.0
    controller.project.pad_grid_offset_samples[0] = 240
    controller.project.pad_loop_auto[0] = True
    controller.project.pad_loop_bars[0] = 0.5
    period = 0.48602673147023145
    origin = -0.010000000000002
    timing = CurrentPadTiming(period, origin, 44_100, "current-accepted-revision")
    monkeypatch.setattr(controller.transport.bpm, "current_timing", lambda _sample_id: timing)

    controller.transport.loop.apply_grid_anchor_to_audio(0)
    controller.transport.loop.set_start(0, origin + period * (16 + 0.04))

    expected_start_frame = round((Decimal(origin) + Decimal(period) * Decimal("16.0625")) * 44_100)
    expected_end_frame = round(
        (Decimal(expected_start_frame) / 44_100 + Decimal(period) * 2) * 44_100
    )
    assert controller.transport.loop.grid_anchor_sec(0) == origin
    assert controller.project.pad_loop_start_s[0] == expected_start_frame / 44_100
    assert controller.transport.loop.effective_region(0) == (
        expected_start_frame / 44_100,
        expected_end_frame / 44_100,
    )
    audio_engine_mock.set_pad_loop_region.assert_called_with(
        0, expected_start_frame / 44_100, expected_end_frame / 44_100
    )
    assert controller.transport.loop.max_auto_loop_bars(0) == (
        60.0 - expected_start_frame / 44_100
    ) / (4 * period)
    assert controller.transport.loop._bar_samples_for_grid_offset_clamp(0) == round(
        4 * period * 44_100
    )
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_effective_loop_region_reuses_explicit_accepted_snapshot(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.pad_loop_auto[0] = True
    controller.project.pad_loop_bars[0] = 0.5
    start_s = 20 + 0.49 / 44_100
    controller.project.pad_loop_start_s[0] = start_s
    period = 0.48602673147023145
    timing = CurrentPadTiming(period, -0.1, 44_100, "snapshot-revision")

    def unexpected_poll(_sample_id: int) -> None:
        pytest.fail("an explicit timing snapshot must not poll a replacement")

    monkeypatch.setattr(controller.transport.bpm, "current_timing", unexpected_poll)
    expected_start_frame = round(Decimal(start_s) * 44_100)
    expected_end_frame = round((Decimal(start_s) + Decimal(period) * 2) * 44_100)
    assert controller.transport.loop.effective_region(0, timing=timing) == (
        expected_start_frame / 44_100,
        expected_end_frame / 44_100,
    )


def test_explicit_unavailable_snapshot_does_not_resolve_a_pending_replacement(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.pad_loop_auto[0] = True
    controller.project.sample_durations[0] = 60.0

    def unexpected_poll(_sample_id: int) -> None:
        pytest.fail("an unavailable snapshot must not poll a pending replacement")

    monkeypatch.setattr(controller.transport.bpm, "current_timing", unexpected_poll)

    assert controller.transport.loop.effective_region(0, timing=None) == (0.0, None)
    assert controller.transport.loop.max_auto_loop_bars(0, timing=None) is None
    assert controller.transport.loop.reclamp_grid_offset_samples(0, timing=None) is False
    controller.transport.loop._apply_effective_pad_loop_region_to_audio(0, timing=None)
    audio_engine_mock.set_pad_loop_region.assert_called_once_with(0, 0.0, None)


def test_explicit_grid_offset_publishes_legacy_intent_and_failed_edit_keeps_saved_offset(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.pad_grid_anchor_s[0] = 10.0
    controller.project.pad_grid_offset_samples[0] = 240
    timing = CurrentPadTiming(0.48602673147023145, -0.1, 44_100, "current-revision")
    monkeypatch.setattr(controller.transport.bpm, "current_timing", lambda _sample_id: timing)
    audio_engine_mock.set_pad_timing_metadata.side_effect = RuntimeError("command queue is full")

    with pytest.raises(RuntimeError, match="queue is full"):
        controller.transport.loop.set_grid_offset_samples(0, 480)

    assert controller.project.pad_grid_offset_samples[0] == 240
    assert controller.transport.loop.grid_anchor_sec(0) == -0.1
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_called_with(0, 10 + 480 / 44_100)


def test_current_accepted_loop_state_is_not_cached_after_resolver_retires_it(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.pad_loop_auto[0] = True
    controller.project.pad_loop_bars[0] = 0.5
    timing = CurrentPadTiming(0.48602673147023145, -0.1, 44_100, "retired-revision")
    monkeypatch.setattr(controller.transport.bpm, "current_timing", lambda _sample_id: timing)
    assert controller.transport.loop.effective_region(0) == (
        0.0,
        round(2 * timing.period_seconds * 44_100) / 44_100,
    )

    monkeypatch.setattr(controller.transport.bpm, "current_timing", lambda _sample_id: None)

    assert controller.transport.loop.effective_region(0) == (0.0, None)
    assert controller.transport.loop.grid_anchor_sec(0) == 0.0
    assert controller.transport.loop.max_auto_loop_bars(0) is None


def test_manual_loop_timing_wins_over_native_accepted_record(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    audio_engine_mock.current_constant_timing.return_value = current_timing_metadata(
        period=0.48602673147023145, origin=-0.1, rate=44_100
    )
    audio_engine_mock.pad_timing_intent.return_value = "automatic"
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.manual_bpm[0] = 150.0
    controller.project.pad_grid_anchor_s[0] = 1.0
    controller.project.pad_grid_offset_samples[0] = -240
    controller.project.pad_loop_auto[0] = True
    controller.project.pad_loop_bars[0] = 0.5

    controller.transport.loop.set_start(0, 1.031)

    assert controller.project.pad_loop_start_s[0] == 1.02
    assert controller.transport.loop.grid_anchor_sec(0) == 0.995
    assert controller.transport.loop.effective_region(0) == (1.02, 1.82)
    audio_engine_mock.current_constant_timing.assert_not_called()


def test_explicit_offset_edit_resumes_legacy_tempo_and_refreshes_locked_master(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    audio_engine_mock.current_constant_timing.return_value = current_timing_metadata(
        period=0.48602673147023145, origin=-0.1
    )
    audio_engine_mock.pad_timing_intent.return_value = "automatic"
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.sample_durations[0] = 60.0
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=90.0, key="C", beat_grid=BeatGrid(beats=[10.0], downbeats=[10.0], bars=[10.0])
    )
    controller.project.pad_loop_auto[0] = True
    controller.project.pad_loop_bars[0] = 0.5
    controller.project.bpm_lock = True
    controller.session.bpm_lock_anchor_pad_id = 0

    def revoke_current(_sample_id: int, _origin: float) -> None:
        audio_engine_mock.current_constant_timing.return_value = None
        audio_engine_mock.pad_timing_intent.return_value = "legacy"

    audio_engine_mock.set_pad_timing_metadata.side_effect = revoke_current
    controller.transport.loop.set_grid_offset_samples(0, 480)

    assert controller.project.pad_grid_offset_samples[0] == 480
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision is None
    assert controller.transport.loop.grid_anchor_sec(0) == 10.01
    assert controller.transport.loop.effective_region(0) == (0.0, 64_000 / 48_000)
    assert controller.session.master_bpm == 90.0
    assert controller.session.bpm_lock_anchor_revision is None
    audio_engine_mock.set_pad_bpm.assert_called_with(0, 90.0)
    audio_engine_mock.set_master_bpm.assert_called_with(90.0)
    audio_engine_mock.bootstrap_transport_from_pad.assert_called_with(0)


@pytest.mark.parametrize("operation", ["start", "end", "auto", "bars", "all", "initialize"])
def test_loop_operation_keeps_one_accepted_snapshot_through_publication(
    controller: AppController, audio_engine_mock: Mock, operation: str
) -> None:
    audio_engine_mock.current_constant_timing.side_effect = [
        current_timing_metadata(),
        RuntimeError("unexpected second current lookup"),
    ]
    audio_engine_mock.pad_timing_intent.return_value = "automatic"
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.sample_durations[0] = 60.0
    controller.project.pad_loop_auto[0] = operation != "auto"
    actions = {
        "start": lambda: controller.transport.loop.set_start(0, 1.0),
        "end": lambda: controller.transport.loop.set_end(0, 2.0),
        "auto": lambda: controller.transport.loop.set_auto(0, enabled=True),
        "bars": lambda: controller.transport.loop.set_bars(0, bars=0.5),
        "all": lambda: controller.transport.loop.set_full_track_region(0),
        "initialize": lambda: controller.transport.loop.initialize_loaded_pad_defaults(0, 0.125),
    }

    actions[operation]()

    audio_engine_mock.current_constant_timing.assert_called_once_with(0)
    audio_engine_mock.set_pad_loop_region.assert_called_once()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_grid_refresh_does_not_revoke_unavailable_automatic_authority(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/current.wav"
    audio_engine_mock.current_constant_timing.return_value = None
    audio_engine_mock.pad_timing_intent.return_value = "automatic"

    controller.transport.loop.apply_grid_anchor_to_audio(0, timing=None)

    audio_engine_mock.current_constant_timing.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


@pytest.mark.parametrize("project_duration_s", [None, 10.0, 900.0])
def test_current_accepted_extent_controls_max_bars_and_all_region(
    controller: AppController, audio_engine_mock: Mock, project_duration_s: float | None
) -> None:
    period = 0.48602673147023145
    audio_engine_mock.output_sample_rate.return_value = 48_000
    audio_engine_mock.current_constant_timing.return_value = current_timing_metadata(
        period=period, rate=44_100, revision="current-source-extent-revision"
    )
    audio_engine_mock.pad_timing_intent.return_value = "automatic"
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.sample_durations[0] = project_duration_s
    start_s = 2.0 + 3 / 44_100
    controller.project.pad_loop_start_s[0] = start_s
    controller.project.pad_loop_auto[0] = True

    assert controller.transport.loop.max_auto_loop_bars(0) == (600.0 - start_s) / (4 * period)
    controller.transport.loop.set_full_track_region(0)

    assert controller.project.pad_loop_auto[0] is False
    assert controller.project.pad_loop_start_s[0] == 0.0
    assert controller.project.pad_loop_end_s[0] == 600.0
    audio_engine_mock.set_pad_loop_region.assert_called_once_with(0, 0.0, 600.0)
    assert controller.project.sample_durations[0] == project_duration_s
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision == "current-source-extent-revision"
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
