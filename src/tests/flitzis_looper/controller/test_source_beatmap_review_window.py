"""Window intents retain the existing selection, completion and failure owners."""

import json
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.source_beatmap_review import ReviewWindowRow, review_window_operation
from tests.flitzis_looper.controller.test_source_beatmap_persistence import reference
from tests.flitzis_looper.controller.test_source_beatmap_selection import _assigned, _event, _job

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.source_beatmap_selection import SourceBeatmapSelection


def _selected(controller: AppController, audio: Mock) -> SourceBeatmapSelection:
    _assigned(controller, audio)
    maps = controller.loader.source_beatmaps
    job = _job(audio)
    maps.select(0, "genuine Native record boundary")
    event = _event(job)
    geometry = job.resolve_source_beatmap_artifact_completion.return_value.map.return_value
    geometry.revision.return_value = reference().map_revision
    maps.completed(0, event)
    return maps


def _window() -> dict[str, object]:
    return review_window_operation(
        reference().map_revision,
        "d" * 64,
        "replace_interior",
        0,
        3,
        (ReviewWindowRow("existing", 1, 1.5),),
    )


def test_window_uses_current_edit_owner_then_original_completion_and_undo_path(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps = _selected(controller, audio_engine_mock)
    old_geometry, old_delivery = maps.effective(0), maps.playback_delivery(0)
    markers = controller.project.pad_loop_start_s[:], controller.project.pad_loop_end_s[:]
    saved = controller._persistence.config_path.read_bytes()
    edit_job = Mock()
    edit_job.is_cancelled.return_value = False
    audio_engine_mock.begin_source_beatmap_edit_async = Mock(return_value=edit_job)
    maps.edit(0, _window(), expected_revision=reference().map_revision)
    call = audio_engine_mock.begin_source_beatmap_edit_async.call_args
    assert json.loads(call.args[1]) == reference().model_dump(mode="json")
    assert json.loads(call.args[2]) == _window()
    assert maps.effective(0) is old_geometry
    assert maps.playback_delivery(0) is old_delivery
    assert controller._persistence.config_path.read_bytes() == saved
    completion = _event(edit_job, "e")
    new_geometry = edit_job.resolve_source_beatmap_artifact_completion.return_value.map.return_value
    new_geometry.revision.return_value = reference().map_revision
    maps.completed(0, completion)
    selected_intent = controller.project.pad_source_beatmap_intent[0]
    assert selected_intent is not None
    assert selected_intent.selected == reference("e")
    edit_job.resolve_source_beatmap_artifact_completion.assert_called_once_with(
        completion["_native_event_owner"]
    )
    undo_job = Mock()
    undo_job.is_cancelled.return_value = False
    audio_engine_mock.begin_source_beatmap_edit_async.return_value = undo_job
    maps.edit(
        0, {"schema_version": 1, "operation": "undo"}, expected_revision=reference().map_revision
    )
    undo_call = audio_engine_mock.begin_source_beatmap_edit_async.call_args
    assert json.loads(undo_call.args[1]) == reference("e").model_dump(mode="json")
    assert json.loads(undo_call.args[2]) == {"schema_version": 1, "operation": "undo"}
    assert (controller.project.pad_loop_start_s, controller.project.pad_loop_end_s) == markers
    audio_engine_mock.begin_source_beatmap_review_async.assert_not_called()


def test_window_native_admission_refusal_and_save_failure_keep_previous_selection(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    maps = _selected(controller, audio_engine_mock)
    geometry, delivery = maps.effective(0), maps.playback_delivery(0)
    previous = controller.project.pad_source_beatmap_intent[0]
    assert previous is not None
    saved = controller._persistence.config_path.read_bytes()
    audio_engine_mock.begin_source_beatmap_edit_async = Mock(
        side_effect=ValueError("window conflict")
    )
    with pytest.raises(ValueError, match="window conflict"):
        maps.edit(0, _window(), expected_revision=reference().map_revision)
    assert controller.project.pad_source_beatmap_intent[0] is previous
    assert controller._assets._reserved == 0
    edit_job = Mock()
    edit_job.is_cancelled.return_value = False
    audio_engine_mock.begin_source_beatmap_edit_async.side_effect = None
    audio_engine_mock.begin_source_beatmap_edit_async.return_value = edit_job
    maps.edit(0, _window(), expected_revision=reference().map_revision)

    def refuse(*_args: object, **_kwargs: object) -> None:
        message = "window Config write failure"
        raise OSError(message)

    monkeypatch.setattr(controller._persistence, "_atomic_write_text", refuse)
    maps.completed(0, _event(edit_job, "e"))
    assert controller.project.pad_source_beatmap_intent[0].selected == previous.selected
    assert controller._persistence.config_path.read_bytes() == saved
    assert maps.effective(0) is geometry
    assert maps.playback_delivery(0) is delivery
    artifact = edit_job.resolve_source_beatmap_artifact_completion.return_value
    artifact.retire_unselected.assert_called_once()
    assert controller._assets._reserved == 0


def test_stale_window_never_admits_and_clear_cancels_only_original_edit_owner(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps = _selected(controller, audio_engine_mock)
    edit_job = Mock()
    edit_job.is_cancelled.return_value = False
    audio_engine_mock.begin_source_beatmap_edit_async = Mock(return_value=edit_job)
    with pytest.raises(ValueError, match="no longer current"):
        maps.edit(0, _window(), expected_revision="f" * 64)
    audio_engine_mock.begin_source_beatmap_edit_async.assert_not_called()
    maps.edit(0, _window(), expected_revision=reference().map_revision)
    maps.clear(0)
    edit_job.cancel.assert_called_once()
    saved = controller._persistence.config_path.read_bytes()
    maps.completed(0, _event(edit_job, "e"))
    cleared_intent = controller.project.pad_source_beatmap_intent[0]
    assert cleared_intent is not None
    assert cleared_intent.selected is None
    assert controller._persistence.config_path.read_bytes() == saved
    assert maps.effective(0) is None
    edit_job.resolve_source_beatmap_artifact_completion.assert_not_called()
