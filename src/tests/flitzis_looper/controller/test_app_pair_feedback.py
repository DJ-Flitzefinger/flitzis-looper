"""Controller observations use Native resolution and retain the exact queued message."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import pair_arrangement
from flitzis_looper.controller.pair_arrangement_journal import PairArrangementAfterImage

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import AudioMessage


@dataclass(slots=True)
class _Feedback:
    kind: str

    def feedback_kind(self) -> str:
        return self.kind

    @staticmethod
    def sample_id() -> int:
        return 0

    @staticmethod
    def pad_peak() -> float:
        return 0.75

    @staticmethod
    def pad_playhead() -> float:
        return 3.25


def _feedback(kind: str) -> AudioMessage:
    return cast("AudioMessage", _Feedback(kind))


@pytest.mark.parametrize("kind", ["started", "stopped", "peak", "playhead"])
def test_feedback_projects_only_native_resolved_content_slot(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, kind: str
) -> None:
    controller.session.active_sample_ids = {0, 215}
    controller.session.paused_sample_ids = {0, 215}
    controller.session.pad_peak[0] = 0.125
    controller.session.pad_playhead_s[0] = 1.5
    message = _feedback(kind)
    resolve = Mock(return_value=215)
    monkeypatch.setattr(controller._audio, "_resolve_pad_feedback", resolve, raising=False)

    controller._handle_audio_message(message)

    resolve.assert_called_once_with(message)
    assert 0 in controller.session.active_sample_ids
    assert 0 in controller.session.paused_sample_ids
    assert controller.session.pad_peak[0] == 0.125
    assert controller.session.pad_playhead_s[0] == 1.5
    if kind == "started":
        assert 215 in controller.session.active_sample_ids
        assert 215 not in controller.session.paused_sample_ids
    elif kind == "stopped":
        assert 215 not in controller.session.active_sample_ids
        assert 215 not in controller.session.paused_sample_ids
    elif kind == "peak":
        assert controller.session.pad_peak[215] == 0.75
    else:
        assert controller.session.pad_playhead_s[215] == 3.25


def test_pending_native_placement_retains_exact_feedback_for_one_later_projection(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    message = _feedback("started")
    resolve = Mock(side_effect=[RuntimeError("content feedback placement pending"), 215])
    monkeypatch.setattr(controller._audio, "_resolve_pad_feedback", resolve, raising=False)
    monkeypatch.setattr(controller._audio, "receive_msg", Mock(return_value=None))
    controller._handle_audio_message(message)
    assert list(controller._pending_content_feedback) == [message]
    assert not controller.session.active_sample_ids

    controller._poll_audio_messages()
    controller._poll_audio_messages()

    assert controller.session.active_sample_ids == {215}
    assert not controller._pending_content_feedback
    assert resolve.call_count == 2
    assert all(call.args[0] is message for call in resolve.call_args_list)


def test_original_feedback_waits_for_pair_python_mirror_but_unrelated_rows_continue(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    message, unrelated = _feedback("started"), _feedback("peak")
    resolve = Mock(side_effect=[215, 7, 215])
    monkeypatch.setattr(controller._audio, "_resolve_pad_feedback", resolve, raising=False)
    monkeypatch.setattr(controller._audio, "receive_msg", Mock(return_value=None))
    fenced_sample_ids = {0, 215}
    monkeypatch.setattr(
        pair_arrangement.PairArrangementController,
        "observer_fenced_sample_ids",
        property(lambda _self: frozenset(fenced_sample_ids)),
    )
    controller._handle_audio_message(message)
    controller._handle_audio_message(unrelated)
    assert list(controller._pending_content_feedback) == [message]
    assert not controller.session.active_sample_ids
    assert controller.session.pad_peak[7] == 0.75

    fenced_sample_ids.clear()
    controller._poll_audio_messages()

    assert controller.session.active_sample_ids == {215}
    assert not controller._pending_content_feedback


def test_retired_feedback_has_no_current_occupant_authority(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.session.active_sample_ids = {0, 215}
    controller.session.paused_sample_ids = {0, 215}
    monkeypatch.setattr(
        controller._audio, "_resolve_pad_feedback", Mock(return_value=None), raising=False
    )
    controller._handle_audio_message(_feedback("stopped"))
    assert controller.session.active_sample_ids == {0, 215}
    assert controller.session.paused_sample_ids == {0, 215}
    assert not controller._pending_content_feedback


def test_pair_preclaim_schema_failure_releases_only_acquired_writer_and_retirement(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    snapshot = Mock()
    monkeypatch.setattr(pair_arrangement, "capture_pair_snapshot", Mock(return_value=snapshot))
    monkeypatch.setattr(
        controller.persistence,
        "capture_migration",
        Mock(return_value=(0, controller.project, None)),
    )
    release = Mock()
    monkeypatch.setattr(controller.persistence, "release_migration", release)
    retirement = Mock()
    monkeypatch.setattr(controller._assets, "reserve", Mock(return_value=retirement))
    monkeypatch.setattr(
        PairArrangementAfterImage,
        "from_before",
        Mock(side_effect=ValueError("invalid participant seal")),
    )
    capture = Mock()
    monkeypatch.setattr(controller._audio, "_capture_pad_pair", capture, raising=False)

    with pytest.raises(ValueError, match="invalid participant seal"):
        controller.pairs.begin(0, 215, operation="swap")

    retirement.close.assert_called_once_with()
    release.assert_called_once()
    capture.assert_not_called()
    assert not controller.pairs.active
