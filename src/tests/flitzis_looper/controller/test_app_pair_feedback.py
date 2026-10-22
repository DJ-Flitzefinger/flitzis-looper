"""Controller observations use Native resolution and retain the exact queued message."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import pair_arrangement
from flitzis_looper.controller.pair_arrangement_journal import PairArrangementAfterImage
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.controller.transport.test_pause_intent import PauseObservation

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


@dataclass(slots=True)
class _OrdinaryFeedback:
    observation_sequence: int
    owner: int = 1
    cell: int = 0
    lifetime: int = 1
    transport_revision: int = 1

    @staticmethod
    def feedback_kind() -> str:
        return "ordinary_transport"


def _ordinary(sequence: int, *, cell: int = 0, lifetime: int = 1) -> AudioMessage:
    return cast("AudioMessage", _OrdinaryFeedback(sequence, cell=cell, lifetime=lifetime))


def test_ordinary_pending_placement_retains_exact_message_and_uses_distinct_resolver(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.pad_content[215] = PadContentIdentity(instance_id="a" * 32)
    message = _ordinary(1)
    observation = PauseObservation(ordinary_paused=True, sample_id=215)
    resolver = Mock(side_effect=[RuntimeError("content feedback placement pending"), observation])
    monkeypatch.setattr(
        controller._audio, "_resolve_ordinary_transport_feedback", resolver, raising=False
    )
    legacy = Mock()
    monkeypatch.setattr(controller._audio, "_resolve_pad_feedback", legacy, raising=False)

    controller._handle_audio_message(message)
    assert list(controller._pending_ordinary_feedback.values()) == [message]
    assert not controller.session.paused_sample_ids
    controller._poll_audio_messages()

    assert controller.session.paused_sample_ids == {215}
    assert not controller._pending_ordinary_feedback
    legacy.assert_not_called()
    assert all(call.args[0] is message for call in resolver.call_args_list)


@pytest.mark.parametrize("fence", ["pairs", "arrangement", "banks"])
def test_ordinary_observation_waits_for_each_original_python_mirror_fence(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, fence: str
) -> None:
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    controller.session.active_sample_ids.add(0)
    controller.transport.playback.pause_pad(0)
    owner = controller.transport.playback._pause_intent._owners["a" * 32]
    observation = PauseObservation(ordinary_paused=True, settled=owner.admission)
    owner.admission.observation = observation
    blocked = {0}
    name = "fenced_sample_ids" if fence == "arrangement" else "observer_fenced_sample_ids"
    monkeypatch.setattr(
        type(getattr(controller, fence)), name, property(lambda _: frozenset(blocked))
    )
    controller._sync_arrangement_observers()
    monkeypatch.setattr(
        controller._audio,
        "_resolve_ordinary_transport_feedback",
        Mock(return_value=observation),
        raising=False,
    )

    controller._handle_audio_message(_ordinary(1))
    controller.transport.playback.poll_pause_observations()
    assert controller.session.paused_sample_ids == set()
    assert owner.pending is True
    assert len(controller._pending_ordinary_feedback) == 1
    blocked.clear()
    controller._sync_arrangement_observers()
    controller._poll_audio_messages()
    assert controller.session.paused_sample_ids == {0}
    assert owner.pending is False


def test_over_1024_ordinary_pending_events_coalesce_latest_across_lifetimes(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    resolver = Mock(side_effect=RuntimeError("content feedback placement pending"))
    monkeypatch.setattr(
        controller._audio, "_resolve_ordinary_transport_feedback", resolver, raising=False
    )
    latest = None
    for sequence in range(1, 1501):
        latest = _ordinary(sequence, lifetime=1 if sequence < 1000 else 2)
        controller._handle_audio_message(latest)
    controller._handle_audio_message(_ordinary(1499, lifetime=1))
    assert list(controller._pending_ordinary_feedback.values()) == [latest]
    resolver.side_effect = None
    resolver.return_value = PauseObservation(
        ordinary_paused=True, observation_sequence=1500, content_lifetime=2
    )
    controller._poll_audio_messages()
    assert controller.session.paused_sample_ids == {0}
    assert resolver.call_args.args[0] is latest


def test_ordinary_pending_map_is_bounded_to_native_cells_and_resets_on_engine_change(
    controller: AppController,
) -> None:
    for cell in range(217):
        controller._retain_ordinary_transport_feedback(_ordinary(1, cell=cell))
    assert len(controller._pending_ordinary_feedback) == 216
    controller._audio = Mock()
    replacement = cast("AudioMessage", _OrdinaryFeedback(1, owner=2))
    controller._retain_ordinary_transport_feedback(replacement)
    assert list(controller._pending_ordinary_feedback.values()) == [replacement]


def test_first_seek_observation_retains_durable_monitor_for_lost_lifecycle_and_next_toggle(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_paths[0] = "/synthetic/held-seek.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    controller.session.active_sample_ids.add(0)
    # Native certifies that Seek took over a Hold pause. No ordinary admission
    # preceded this first observation, so it must supply its own monitor custody.
    seek = PauseObservation(ordinary_paused=True, requested_effect="seek")
    monkeypatch.setattr(
        controller._audio,
        "_resolve_ordinary_transport_feedback",
        Mock(return_value=seek),
        raising=False,
    )

    controller._handle_audio_message(_ordinary(1))

    owner = controller.transport.playback._pause_intent._owners["a" * 32]
    assert owner.admission is None
    assert owner.monitor is seek
    assert owner.pending is False
    assert controller.session.paused_sample_ids == {0}
    seek.current = PauseObservation(
        ordinary_paused=False,
        observation_sequence=2,
        status_kind="started",
        requested_effect=None,
        outcome=None,
        transport_revision=0,
    )
    audio_engine_mock._current_ordinary_transport_observation.side_effect = lambda sample_id: (
        seek.current_observation() if sample_id == 0 else None
    )

    controller.transport.playback.toggle_pause_pad(0)

    assert controller.session.paused_sample_ids == set()
    audio_engine_mock.pause_sample.assert_called_once_with(0)
    audio_engine_mock.resume_sample.assert_not_called()
