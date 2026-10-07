"""Saved-intent geometry and productive startup admission wiring."""

import struct
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.saved_residency import saved_resident_loop
from flitzis_looper.models import BeatGrid, ProjectState, SampleAnalysis

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _bits(value: float) -> str:
    return struct.pack("!d", value).hex()


def _saved_automatic(project: ProjectState, period: float, origin: float) -> None:
    project.pad_timing_intent[0] = "automatic"
    project.sample_analysis[0] = SampleAnalysis.model_validate({
        "bpm": 120.00128936767578,
        "key": "C",
        "beat_grid": {"beats": [], "downbeats": [], "bars": []},
        "accepted_timing": {
            "schema_version": 1,
            "encoding": "accepted-constant-timing-qm-raw-v1",
            "record": {
                "period_bits": _bits(period),
                "origin": {"seconds_bits": _bits(origin)},
            },
        },
    })


@pytest.mark.parametrize("rate", [44_100, 48_000, 96_000])
def test_explicit_saved_window_keeps_absolute_intent_and_complete_metadata(rate: int) -> None:
    project = ProjectState()
    project.sample_paths[0] = "samples/long.wav"
    project.sample_durations[0] = 600.0
    project.pad_loop_start_s[0] = 321.25 + 0.375 / rate
    project.pad_loop_end_s[0] = 325.25 + 0.375 / rate
    before = project.model_dump()

    resident = saved_resident_loop(project, 0, sample_rate_hz=rate)

    assert resident is not None
    assert resident.start_seconds == 321.25
    assert resident.end_seconds == 325.25
    assert resident.key_lock is False
    assert project.model_dump() == before


def test_saved_automatic_geometry_preserves_period_bits_without_promoting_history() -> None:
    project = ProjectState()
    period = 0.500000000123
    origin = -0.125
    _saved_automatic(project, period, origin)
    project.pad_loop_start_s[0] = 321.25
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 2.0
    before = project.model_dump()

    resident = saved_resident_loop(project, 0, sample_rate_hz=48_000)

    assert resident is not None
    assert resident.start_seconds == 321.25
    assert resident.end_seconds == round((321.25 + 8.0 * period) * 48_000) / 48_000
    assert (
        resident.end_seconds != round((321.25 + 8.0 * 60.0 / 120.00128936767578) * 48_000) / 48_000
    )
    assert project.model_dump() == before


@pytest.mark.parametrize("period", [float("nan"), float("inf"), -1.0, 0.0])
def test_invalid_saved_automatic_geometry_requests_complete_track(period: float) -> None:
    project = ProjectState()
    _saved_automatic(project, period, -0.125)
    project.pad_loop_auto[0] = True
    assert saved_resident_loop(project, 0, sample_rate_hz=48_000) is None


@pytest.mark.parametrize("bad_bits", [None, "invalid", "0" * 15, "x" * 16, " " * 16])
def test_malformed_saved_period_cannot_be_replaced_by_legacy_bpm(bad_bits: str | None) -> None:
    project = ProjectState()
    _saved_automatic(project, 0.5, -0.125)
    analysis = project.sample_analysis[0]
    assert analysis is not None
    assert analysis.accepted_timing is not None
    analysis.accepted_timing.record["period_bits"] = bad_bits
    project.pad_loop_auto[0] = True
    assert saved_resident_loop(project, 0, sample_rate_hz=48_000) is None


@pytest.mark.parametrize("rate", [44_100, 96_000])
def test_manual_auto_window_uses_existing_scalar_projection_and_signed_grid(rate: int) -> None:
    project = ProjectState()
    project.manual_bpm[0] = 123.5
    project.pad_timing_intent[0] = "tap"
    project.pad_loop_start_s[0] = 321.25
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 0.25
    project.pad_grid_anchor_s[0] = 0.03125
    project.pad_grid_offset_samples[0] = -rate // 2

    resident = saved_resident_loop(project, 0, sample_rate_hz=rate)

    assert resident is not None
    assert resident.end_seconds == round((321.25 + 60.0 / 123.5) * rate) / rate


def test_legacy_auto_window_uses_existing_analysis_bpm() -> None:
    project = ProjectState()
    project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0, key="C", beat_grid=BeatGrid(beats=[0.5], downbeats=[0.5], bars=[0.5])
    )
    project.pad_loop_auto[0] = True
    project.pad_loop_start_s[0] = 4.5
    project.pad_loop_bars[0] = 0.25
    resident = saved_resident_loop(project, 0, sample_rate_hz=48_000)
    assert resident is not None
    assert resident.end_seconds == 5.0


@pytest.mark.parametrize("rate", [0, -1])
def test_unavailable_device_rate_cannot_authorize_finite_residency(rate: int) -> None:
    project = ProjectState()
    project.pad_loop_start_s[0] = 1.0
    project.pad_loop_end_s[0] = 1.0
    assert saved_resident_loop(project, 0, sample_rate_hz=rate) is None


@pytest.mark.parametrize("key_lock", [False, True])
def test_productive_restore_admits_saved_finite_loop_before_native_ack(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path, *, key_lock: bool
) -> None:
    original = tmp_path / "samples" / "long.wav"
    original.parent.mkdir()
    original.write_bytes(b"original captured and verified by native loading")
    project = controller.project
    project.sample_paths[0] = "samples/long.wav"
    project.sample_durations[0] = 600.0
    project.pad_loop_start_s[0] = 321.25
    project.pad_loop_end_s[0] = 325.25
    project.pad_key_lock[0] = key_lock
    before = project.model_dump()
    audio_engine_mock.reset_mock()
    audio_engine_mock.load_sample_async.return_value = 7

    controller.loader.restore_samples_from_project_state()

    audio_engine_mock.load_sample_async.assert_called_once_with(
        0,
        "samples/long.wav",
        run_analysis=False,
        replace_assignment=True,
        resident_loop_start_s=321.25,
        resident_loop_end_s=325.25,
        resident_key_lock=key_lock,
    )
    assert project.model_dump() == before
    assert 0 in controller.session.loading_sample_ids
    audio_engine_mock.capture_prepared_source.assert_not_called()
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 7,
            "cached_path": "samples/long.wav",
            "duration_s": 600.0,
        },
        None,
    ]
    controller.loader.poll_loader_events()
    assert project.sample_durations[0] == 600.0
    assert project.pad_loop_start_s[0] == 321.25
    assert project.pad_loop_end_s[0] == 325.25
    assert 0 not in controller.session.loading_sample_ids


def test_deferred_finite_restore_recaptures_latest_loop_intent(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    original = tmp_path / "samples" / "long.wav"
    original.parent.mkdir()
    original.write_bytes(b"original")
    controller.project.sample_paths[0] = "samples/long.wav"
    controller.project.pad_loop_start_s[0] = 10.0
    controller.project.pad_loop_end_s[0] = 14.0
    audio_engine_mock.load_sample_async.side_effect = RuntimeError(
        controller.loader._COLD_QUEUE_FULL
    )
    controller.loader.restore_samples_from_project_state()
    controller.project.pad_loop_start_s[0] = 321.25
    controller.project.pad_loop_end_s[0] = 325.25
    audio_engine_mock.load_sample_async.side_effect = None
    audio_engine_mock.load_sample_async.return_value = 9
    audio_engine_mock.poll_loader_events.return_value = None

    controller.loader.poll_loader_events()

    assert audio_engine_mock.load_sample_async.call_args.kwargs["resident_loop_start_s"] == 321.25
    assert audio_engine_mock.load_sample_async.call_args.kwargs["resident_loop_end_s"] == 325.25
    assert controller.loader._load_request_ids[0] == 9


@pytest.mark.parametrize("rate", [44_100, 48_000, 96_000])
def test_startup_and_apply_project_settings_use_identical_fractional_saved_markers(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path, rate: int
) -> None:
    original = tmp_path / "samples" / "long.wav"
    original.parent.mkdir()
    original.write_bytes(b"original")
    project = controller.project
    project.sample_paths[0] = "samples/long.wav"
    project.sample_durations[0] = 600.0
    project.pad_loop_start_s[0] = 2.5 / rate
    project.pad_loop_end_s[0] = 8.5 / rate
    before = project.model_dump()
    audio_engine_mock.output_sample_rate.return_value = rate
    audio_engine_mock.reset_mock()

    controller.loader.restore_samples_from_project_state()
    controller.transport.apply_project_state_to_audio()

    assert audio_engine_mock.load_sample_async.call_args.kwargs["resident_loop_start_s"] == 2 / rate
    assert audio_engine_mock.load_sample_async.call_args.kwargs["resident_loop_end_s"] == 8 / rate
    audio_engine_mock.set_pad_loop_region.assert_called_once_with(0, 2 / rate, 8 / rate)
    assert project.model_dump() == before
