"""Controller membership regressions; doubles prove no Native audio or OwnACK."""

from typing import TYPE_CHECKING
from unittest.mock import call

import pytest

from flitzis_looper.models import PadContentIdentity
from flitzis_looper_audio import AudioMessage
from tests.flitzis_looper.conftest import (
    FakeCapturedPadStart,
    FakeGlobalPlaybackBatchTicket,
)
from tests.flitzis_looper.controller.transport.test_global_batches import _accepted_sources
from tests.flitzis_looper.controller.transport.test_pause_intent import PauseObservation

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding


def _drain(controller: AppController, audio: Mock, messages: list[AudioMessage]) -> None:
    audio.receive_msg.side_effect = [*messages, None]
    try:
        controller._poll_audio_messages()
    finally:
        audio.receive_msg.side_effect = None


def _exclusive_history(
    controller: AppController, audio: Mock, *, deliver_last_start: bool = True
) -> dict[int, FakeInputRuntimePadBinding]:
    _, bindings = _accepted_sources(controller, audio, (0, 1, 2))
    for slot in bindings:
        controller.project.pad_content[slot] = PadContentIdentity(instance_id=f"{slot + 1:032x}")
    controller.project.multi_loop = False
    for slot in bindings:
        controller.transport.playback.trigger_pad(slot, received_at_ns=slot + 1)
        original = audio.publish_pad_start.call_args.args[0]
        assert isinstance(original, FakeCapturedPadStart)
        assert original.original_id == slot
        assert original.exclusive is True
        assert original.state() == "applied"
        messages = [AudioMessage.SampleStopped(slot - 1)] if slot else []
        if slot != 2 or deliver_last_start:
            messages.append(AudioMessage.SampleStarted(slot))
        _drain(controller, audio, messages)
    captured_ids = [entry.args[0] for entry in audio.capture_pad_start.call_args_list]
    assert captured_ids == [0, 1, 2]
    # Native's conservative admission inventory is distinct from effective voices.
    audio.cancel_all_launches.return_value = captured_ids
    return bindings


def _stopped_slots(audio: Mock) -> list[int]:
    return [
        binding.metadata()["pad_id"]
        for binding in audio.stop_global_playback_batch.call_args.args[0]
    ]


def test_off_a_b_c_then_on_stop_covers_history_but_next_start_restores_only_c(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    bindings = _exclusive_history(controller, audio_engine_mock)
    assert controller.session.active_sample_ids == {2}
    controller.transport.global_params.set_multi_loop(enabled=True)
    stop = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.stop_global_playback_batch.return_value = stop

    controller.transport.playback.stop_global_start_stop()

    assert _stopped_slots(audio_engine_mock) == [0, 1, 2]
    assert controller.session.global_stop_restore_sample_ids == set()
    owner = controller.transport.playback._global_playback
    owner.poll()
    assert controller.session.global_stop_restore_sample_ids == set()
    stop.status = "accepted"
    owner.poll()
    assert controller.session.global_stop_restore_sample_ids == {2}
    _drain(controller, audio_engine_mock, [AudioMessage.SampleStopped(2)])
    assert controller.session.active_sample_ids == set()

    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=73)

    audio_engine_mock.capture_global_start.assert_called_once()
    entries = audio_engine_mock.capture_global_start.call_args.args[0]
    assert [binding for binding, *_rest in entries] == [bindings[2]]
    assert audio_engine_mock.capture_global_start.call_args.kwargs == {"received_at_ns": 73}
    audio_engine_mock.publish_captured_global_start.assert_called_once()
    audio_engine_mock.stop_all.assert_not_called()


def test_stop_restore_uses_durable_claimed_voice_after_missing_and_delayed_generic_feedback(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _exclusive_history(controller, audio_engine_mock, deliver_last_start=False)
    assert controller.session.active_sample_ids == set()
    controller.transport.global_params.set_multi_loop(enabled=True)
    observations = {
        slot: PauseObservation(
            sample_id=slot,
            active=slot == 2,
            ordinary_paused=False,
            observation_sequence=2,
            requested_effect=None,
            outcome=None,
        )
        for slot in (0, 1, 2)
    }
    audio_engine_mock._current_ordinary_transport_observation.side_effect = observations.get
    audio_engine_mock._current_ordinary_transport_observation.reset_mock()
    audio_engine_mock.receive_msg.side_effect = [
        AudioMessage.SampleStarted(0),
        AudioMessage.SampleStarted(1),
        None,
    ]
    stop = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.stop_global_playback_batch.return_value = stop
    try:
        controller.transport.playback.stop_global_start_stop()
    finally:
        audio_engine_mock.receive_msg.side_effect = None

    audio_engine_mock._current_ordinary_transport_observation.assert_has_calls([
        call(0),
        call(1),
        call(2),
    ])
    assert controller.session.active_sample_ids == {2}
    assert _stopped_slots(audio_engine_mock) == [0, 1, 2]
    stop.status = "accepted"
    controller.transport.playback._global_playback.poll()
    assert controller.session.global_stop_restore_sample_ids == {2}


@pytest.mark.parametrize("group_state", ["deferred", "pending"])
def test_unclaimed_group_members_need_stop_coverage_but_do_not_join_restart_membership(
    controller: AppController, audio_engine_mock: Mock, group_state: str
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0, 1, 2))
    _drain(controller, audio_engine_mock, [AudioMessage.SampleStarted(2)])
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 1}
    group = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.publish_captured_global_start.side_effect = lambda capture: (
        None if group_state == "deferred" else capture.publish(group)
    )
    controller.transport.playback.start_or_restart_global_start_stop()
    owner = controller.transport.playback._global_playback
    assert (owner._deferred is not None) is (group_state == "deferred")
    assert (owner._pending is not None) is (group_state == "pending")
    assert controller.session.active_sample_ids == {2}
    stop = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.stop_global_playback_batch.return_value = stop

    controller.transport.playback.stop_global_start_stop()

    assert _stopped_slots(audio_engine_mock) == [0, 1, 2]
    assert controller.session.global_stop_restore_sample_ids == {0, 1}
    stop.status = "accepted"
    owner.poll()
    assert controller.session.global_stop_restore_sample_ids == {2}
