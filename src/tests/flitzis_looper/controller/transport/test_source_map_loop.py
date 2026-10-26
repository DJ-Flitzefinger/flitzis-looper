"""Loop-owner integration with real Native geometry and a selection-boundary double.

The double grants no source/custody acceptance. Native job, Config and source
qualification are separately tested; these cases exercise physical loop editing.
"""

import hashlib
import json
import struct
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, call

import pytest

from flitzis_looper.source_beatmap_intent import PadSourceBeatmapIntent
from flitzis_looper.source_sync_intent import PadSourceSyncIntent
from flitzis_looper_audio import SourceBeatmap

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.source_beatmap_selection import SourceBeatmapSelection


def _digest(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(encoded.encode()).hexdigest()


def _bits(value: float) -> str:
    return struct.pack(">d", value).hex()


def _geometry(
    *, middle_coverage: str = "trusted", meter: str = "quarter_note_4_4_verified"
) -> SourceBeatmap:
    descriptor = {
        "schema_version": 1,
        "original": {"sha256": "a" * 64, "bytes": 1000},
        "pcm": {
            "format": "f32-le-interleaved-v1",
            "channel_layout": "interleaved-channel-index-order-v1",
            "rate_hz": 48000,
            "channels": 2,
            "full_frames": 480000,
            "full_bytes": 3840000,
            "interleaved_sha256": "b" * 64,
            "mono_sha256": "c" * 64,
            "mono_revision": "test-mono-v1",
            "source_zero_bits": _bits(0.0),
            "provenance": {"test_only": True},
        },
    }
    raw = {
        "detector": {
            "detector_id": "manual",
            "code_revision": "loop-fixture-v1",
            "configuration": {},
            "model": None,
            "preprocessing": {},
        },
        "analysis_input": None,
        "timestamp_resolution_seconds_bits": None,
        "beat_seconds_bits": [],
        "downbeat_seconds_bits": [],
        "uncertainty": "Exact geometry fixture, without source or musical acceptance",
    }
    raw["raw_revision"] = _digest(raw)
    accepted = {
        "parent_revision": None,
        "beat_unit": "quarter_note",
        "interpolation_policy": "pwa-source-seconds-quarter-v1",
        "boundary_policy": "right-interior-left-final-gap-unavailable-v1",
        "snap_policy": "nearest-quarter-subdivision-later-tie-v1",
        "anchors": [
            {
                "id": f"anchor-{index}",
                "source_seconds_bits": _bits(seconds),
                "beat_bits": _bits(beat),
                "raw_beat_index": None,
                "review": "reviewed",
                "sample_edit": None,
            }
            for index, (seconds, beat) in enumerate([
                (0.0, 0.0),
                (2.0, 4.0),
                (5.0, 8.0),
                (10.0, 12.0),
            ])
        ],
        "intervals": [
            {
                "coverage": coverage,
                "meter": {"status": meter, "downbeat_label": None, "evidence": "synthetic"},
            }
            for coverage in ["trusted", middle_coverage, "trusted"]
        ],
        "lineage": [],
    }
    record = {
        "source": {"decoder_identity": _digest(descriptor), "decoder_descriptor": descriptor},
        "raw": raw,
        "accepted": accepted,
    }
    accepted["map_revision"] = _digest(record)
    return SourceBeatmap.from_json(
        json.dumps({"schema_version": 1, "encoding": "source-beatmap-pwa-v1", "record": record})
    )


def _bind_map(controller: AppController, geometry: SourceBeatmap | None) -> Mock:
    selection = Mock()
    selection.is_selected.return_value = True
    selection.effective.return_value = geometry
    selection._playback_projection.side_effect = lambda sample_id: (
        selection.effective(sample_id),
        None,
    )
    controller.transport.loop.bind_source_maps(cast("SourceBeatmapSelection", selection))
    return selection


@pytest.fixture
def mapped_loop(controller: AppController, audio_engine_mock: Mock) -> Mock:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/manual-map.wav"
    controller.project.sample_durations[0] = 10.0
    controller.project.manual_bpm[0] = 120.0
    controller.project.pad_loop_start_s[0] = 1.0
    controller.project.pad_loop_end_s[0] = None
    controller.project.pad_loop_bars[0] = 1.0
    controller.project.pad_loop_auto[0] = True
    controller.project.multi_loop = True
    controller.project.bpm_lock = True
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=1)
    return _bind_map(controller, _geometry())


def _intent(controller: AppController) -> tuple[float, float | None, bool, float]:
    project = controller.project
    return (
        project.pad_loop_start_s[0],
        project.pad_loop_end_s[0],
        project.pad_loop_auto[0],
        project.pad_loop_bars[0],
    )


@pytest.mark.parametrize("readiness", ["missing", "uncertain", "unknown-meter"])
def test_unavailable_map_can_be_inspected_and_passively_refreshed_without_publication(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock, readiness: str
) -> None:
    mapped_loop.effective.return_value = (
        None
        if readiness == "missing"
        else _geometry(middle_coverage="uncertain")
        if readiness == "uncertain"
        else _geometry(meter="unknown")
    )
    before = _intent(controller)

    assert controller.transport.loop.effective_region(0) == (1.0, 3.0)
    controller.transport.bpm.on_pad_bpm_changed(0, publish_bpm=False)
    assert _intent(controller) == before
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.prepare_resident_control.assert_not_called()
    with pytest.raises(ValueError, match=r"qualified|supported"):
        controller.transport.loop.requested_region(0)

    mapped_loop.effective.return_value = _geometry()
    assert controller.transport.loop.effective_region(0) == (1.0, 3.5)
    controller.transport.loop.apply_source_map_bars(0)
    audio_engine_mock.set_pad_loop_region.assert_called_once_with(
        0, 1.0, 3.5, expected_source_generation=1
    )


def test_passive_map_refresh_does_not_hide_a_real_audio_publication_failure(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock
) -> None:
    audio_engine_mock.set_pad_loop_region.side_effect = ValueError("stale source generation")
    with pytest.raises(ValueError, match="stale source generation"):
        controller.transport.loop._apply_effective_pad_loop_region_to_audio(
            0, defer_unavailable_map=True
        )


def test_selection_with_available_sync_parents_uses_map_bars_without_changing_source_timing(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock
) -> None:
    assert controller.transport.loop.requested_region(0) == (1.0, 3.5)
    assert _intent(controller) == (1.0, None, True, 1.0)
    audio_engine_mock.set_pad_loop_region.assert_not_called()

    controller.transport.loop.set_bars(0, bars=1.0)

    assert _intent(controller) == (1.0, 3.5, True, 1.0)
    audio_engine_mock.set_pad_loop_region.assert_called_once_with(
        0, 1.0, 3.5, expected_source_generation=1
    )
    assert mapped_loop.effective.call_args_list == [call(0), call(0)]
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_sync_status_projects_its_captured_map_without_a_second_selector_or_mutation(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock
) -> None:
    geometry = _geometry()
    mapped_loop.effective.side_effect = AssertionError("Status already captured its geometry.")
    previous = controller.project.model_dump(), controller.session.model_dump()
    audio_engine_mock.reset_mock()

    region = controller.transport.loop._source_sync_requested_region(0, geometry)

    assert region == (1.0, 3.5)
    assert (controller.project.model_dump(), controller.session.model_dump()) == previous
    mapped_loop.effective.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.set_input_runtime_state.assert_not_called()
    audio_engine_mock.capture_pad_start.assert_not_called()


@pytest.mark.parametrize("invalid", ["uncertain", "gap_unknown_count", "meter", "extent"])
def test_sync_status_captured_map_retains_original_interval_and_meter_refusals(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock, invalid: str
) -> None:
    geometry = _geometry(
        middle_coverage=invalid if invalid in {"uncertain", "gap_unknown_count"} else "trusted",
        meter="unknown" if invalid == "meter" else "quarter_note_4_4_verified",
    )
    controller.project.pad_loop_bars[0] = (
        3.0 if invalid == "extent" else 1.0 if invalid == "meter" else 2.0
    )
    mapped_loop.effective.side_effect = AssertionError("Status must retain its captured map.")
    previous = controller.project.model_dump(), controller.session.model_dump()

    with pytest.raises(ValueError, match="supported"):
        controller.transport.loop._source_sync_requested_region(0, geometry)

    assert (controller.project.model_dump(), controller.session.model_dump()) == previous
    mapped_loop.effective.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.capture_pad_start.assert_not_called()


@pytest.mark.parametrize("invalid", ["revision", "physical", "coordinate", "end"])
def test_sync_status_captured_map_keeps_original_result_qualification(
    controller: AppController, mapped_loop: Mock, invalid: str
) -> None:
    original = _geometry()
    geometry = Mock(wraps=original)
    result = dict(original.source_after_beats(1.0, 4.0, require_four_four=True))
    if invalid == "revision":
        result["map_revision"] = "f" * 64
    elif invalid == "physical":
        result["physical_domain"] = "outside_source"
    elif invalid == "coordinate":
        result["coordinate_status"] = "unsupported"
    else:
        result["source_seconds"] = 1.0
    geometry.source_after_beats.return_value = result
    previous = _intent(controller)

    with pytest.raises(ValueError, match=r"supported|no physical source interval"):
        controller.transport.loop._source_sync_requested_region(0, geometry)

    geometry.source_after_beats.assert_called_once_with(1.0, 4.0, require_four_four=True)
    assert _intent(controller) == previous
    mapped_loop.effective.assert_not_called()


@pytest.mark.parametrize("mode", ["manual", "one-shot", "parent-off"])
def test_status_captured_geometry_does_not_replace_original_manual_or_inactive_path(
    controller: AppController, mapped_loop: Mock, mode: str
) -> None:
    project = controller.project
    project.pad_loop_auto[0] = mode == "parent-off"
    project.pad_loop_end_s[0] = 4.0 if mode == "manual" else None
    if mode == "parent-off":
        project.multi_loop = False
    geometry = Mock()
    geometry.editor_info.side_effect = AssertionError("This path must not use mapped timing.")
    mapped_loop.effective.side_effect = AssertionError("This path needs no source map.")
    previous = project.model_dump(), controller.session.model_dump()

    region = controller.transport.loop._source_sync_requested_region(0, geometry)

    assert region == (1.0, 4.0 if mode == "manual" else 3.0 if mode == "parent-off" else None)
    assert (project.model_dump(), controller.session.model_dump()) == previous
    geometry.editor_info.assert_not_called()
    mapped_loop.effective.assert_not_called()


@pytest.mark.parametrize("operation", ["bars", "end", "snap", "max"])
def test_variable_loop_operation_retains_one_captured_map(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock, operation: str
) -> None:
    geometry = _geometry()
    mapped_loop.effective.side_effect = [geometry, AssertionError("Unexpected second map lookup.")]
    previous = controller.project.model_dump()
    if operation == "max":
        assert controller.transport.loop.max_auto_loop_bars(0) == 2.5
        assert controller.project.model_dump() == previous
        audio_engine_mock.set_pad_loop_region.assert_not_called()
        audio_engine_mock.current_input_runtime_pad_binding.assert_not_called()
    else:
        actions = {
            "bars": lambda: controller.transport.loop.apply_source_map_bars(0, 1.0),
            "end": lambda: controller.transport.loop.set_end(0, 5.0390625),
            "snap": lambda: controller.transport.loop.snap_source_map_markers(0),
        }
        actions[operation]()
        expected = (
            (1.0, 5.078125, False, 1.0)
            if operation == "end"
            else (
                1.0,
                3.5,
                operation == "bars",
                1.0,
            )
        )
        assert _intent(controller) == expected
        audio_engine_mock.set_pad_loop_region.assert_called_once_with(
            0, expected[0], expected[1], expected_source_generation=1
        )
    mapped_loop.effective.assert_called_once_with(0)
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_sync_auto_bars_retain_musical_extent_and_manual_end_clears_auto_intent(
    controller: AppController, mapped_loop: Mock
) -> None:
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=1)

    controller.transport.loop.set_bars(0, bars=1.0)

    assert _intent(controller) == (1.0, 3.5, True, 1.0)
    assert controller.transport.loop.requested_region(0) == (1.0, 3.5)
    controller.transport.loop.set_end(0, 5.0390625)
    assert _intent(controller) == (1.0, 5.078125, False, 1.0)


def test_sync_auto_region_rounds_coverage_without_deriving_bars_from_it(
    controller: AppController, mapped_loop: Mock
) -> None:
    project = controller.project
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=1)
    project.pad_loop_start_s[0] = 1.0 + 1.0 / 48_000
    exact_end = 3.5 + 1.5 / 48_000

    start, physical_end = controller.transport.loop.requested_region(0)

    assert start == project.pad_loop_start_s[0]
    assert physical_end == round(exact_end * 48_000) / 48_000
    assert physical_end != exact_end
    assert project.pad_loop_bars[0] == 1.0
    assert project.pad_loop_auto[0] is True


def test_enabling_sync_auto_preserves_selected_physical_start_and_canonical_bar_end(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock
) -> None:
    project = controller.project
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=1)
    project.pad_loop_auto[0] = False
    project.pad_loop_start_s[0] = 1.012345
    project.pad_loop_end_s[0] = 4.0
    start = round(1.012345 * 48_000) / 48_000
    # The independent fixture has B(s)=2*s here and S(b)=2+(b-4)*3/4 at the end.
    end = round((2.0 + ((2.0 * start + 4.0) - 4.0) * 0.75) * 48_000) / 48_000

    controller.transport.loop.set_auto(0, enabled=True)

    assert _intent(controller) == (start, end, True, 1.0)
    audio_engine_mock.set_pad_loop_region.assert_called_once_with(
        0, start, end, expected_source_generation=1
    )


def test_explicit_map_markers_keep_materialized_region_when_map_is_cleared(
    controller: AppController, mapped_loop: Mock
) -> None:
    controller.transport.loop.apply_source_map_bars(0)
    controller.transport.loop.set_auto(0, enabled=False)
    mapped_loop.effective.return_value = None
    mapped_loop.is_selected.return_value = False

    assert controller.transport.loop.requested_region(0) == (1.0, 3.5)
    assert _intent(controller) == (1.0, 3.5, False, 1.0)


def test_explicit_snap_uses_variable_later_ties_and_materializes_both_markers(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock
) -> None:
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 2.0234375
    controller.project.pad_loop_end_s[0] = 5.0390625

    controller.transport.loop.snap_source_map_markers(0)

    expected = (round(2.046875 * 48_000) / 48_000, 5.078125)
    assert _intent(controller) == (*expected, False, 1.0)
    audio_engine_mock.set_pad_loop_region.assert_called_once_with(
        0, *expected, expected_source_generation=1
    )
    mapped_loop.effective.assert_called_once_with(0)


def test_map_auto_start_edit_snaps_and_uses_variable_bar_endpoint(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock
) -> None:
    controller.transport.loop.set_start(0, 2.0234375)

    start = round(2.046875 * 48_000) / 48_000
    # Beat 8+1/16 lies in the slower third segment, at this independent fixture coordinate.
    end = 5.078125
    assert _intent(controller) == (start, end, True, 1.0)
    audio_engine_mock.set_pad_loop_region.assert_called_once_with(
        0, start, end, expected_source_generation=1
    )
    mapped_loop.effective.assert_called_once_with(0)


def test_map_auto_end_edit_keeps_physical_start_and_uses_native_snap(
    controller: AppController, mapped_loop: Mock
) -> None:
    controller.transport.loop.set_end(0, 5.0390625)

    assert _intent(controller) == (1.0, 5.078125, False, 1.0)
    mapped_loop.effective.assert_called_once_with(0)


def test_map_physical_edits_do_not_snap_when_auto_is_disabled(
    controller: AppController, mapped_loop: Mock
) -> None:
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_end_s[0] = 4.0

    controller.transport.loop.set_start(0, 2.0234375)
    controller.transport.loop.set_end(0, 5.0390625)

    assert controller.project.pad_loop_start_s[0] == round(2.0234375 * 48_000) / 48_000
    assert controller.project.pad_loop_end_s[0] == round(5.0390625 * 48_000) / 48_000
    mapped_loop.effective.assert_not_called()


@pytest.mark.parametrize("operation", ["bars", "snap", "start", "end"])
def test_selected_but_unavailable_map_refuses_without_scalar_fallback(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock, operation: str
) -> None:
    previous = _intent(controller)
    mapped_loop.effective.return_value = None
    actions = {
        "bars": lambda: controller.transport.loop.set_bars(0, bars=1.0),
        "snap": lambda: controller.transport.loop.snap_source_map_markers(0),
        "start": lambda: controller.transport.loop.set_start(0, 2.0),
        "end": lambda: controller.transport.loop.set_end(0, 5.0),
    }

    with pytest.raises(ValueError, match=r"unavailable|preparing"):
        actions[operation]()

    assert _intent(controller) == previous
    audio_engine_mock.set_pad_loop_region.assert_not_called()


@pytest.mark.parametrize("coverage", ["uncertain", "gap_unknown_count"])
def test_map_bar_interval_and_snap_refuse_an_intervening_gap_atomically(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock, coverage: str
) -> None:
    mapped_loop.effective.return_value = _geometry(middle_coverage=coverage)
    previous = _intent(controller)

    with pytest.raises(ValueError, match="supported"):
        controller.transport.loop.apply_source_map_bars(0, 2.0)

    assert _intent(controller) == previous
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_end_s[0] = 7.0
    previous = _intent(controller)
    with pytest.raises(ValueError, match="supported"):
        controller.transport.loop.snap_source_map_markers(0)

    assert _intent(controller) == previous
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_bar_operation_requires_full_four_four_and_actual_source_extent(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock
) -> None:
    previous = _intent(controller)
    mapped_loop.effective.return_value = _geometry(meter="unknown")
    assert controller.transport.loop.max_auto_loop_bars(0) is None
    with pytest.raises(ValueError, match="supported"):
        controller.transport.loop.apply_source_map_bars(0, 1.0)
    assert _intent(controller) == previous

    mapped_loop.effective.return_value = _geometry()
    controller.project.sample_durations[0] = 999.0
    assert controller.transport.loop.max_auto_loop_bars(0) == 2.5
    with pytest.raises(ValueError, match="supported"):
        controller.transport.loop.apply_source_map_bars(0, 3.0)
    assert _intent(controller) == previous
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_native_loop_publication_failure_rolls_back_all_materialized_intent(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock
) -> None:
    previous = _intent(controller)
    audio_engine_mock.set_pad_loop_region.side_effect = ValueError("stale source generation")

    with pytest.raises(ValueError, match="stale source"):
        controller.transport.loop.apply_source_map_bars(0, 2.0)

    assert _intent(controller) == previous
    mapped_loop.effective.assert_called_once_with(0)


def test_newer_map_intent_during_query_cannot_publish_old_geometry(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock
) -> None:
    geometry = _geometry()
    previous = _intent(controller)

    def replace_intent(_sample_id: int) -> SourceBeatmap:
        controller.project.pad_source_beatmap_intent[0] = PadSourceBeatmapIntent(
            schema_version=1, revision=1, selected=None
        )
        return geometry

    mapped_loop.effective.side_effect = replace_intent
    with pytest.raises(ValueError, match="superseded"):
        controller.transport.loop.apply_source_map_bars(0)

    assert _intent(controller) == previous
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_map_actions_require_current_generation_before_mutating_markers(
    controller: AppController, audio_engine_mock: Mock, mapped_loop: Mock
) -> None:
    previous = _intent(controller)
    audio_engine_mock.current_input_runtime_pad_binding.return_value = None
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None

    with pytest.raises(ValueError, match="current source generation"):
        controller.transport.loop.apply_source_map_bars(0)

    assert _intent(controller) == previous
    mapped_loop.effective.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
