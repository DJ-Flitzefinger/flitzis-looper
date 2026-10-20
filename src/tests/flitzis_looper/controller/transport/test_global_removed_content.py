"""An empty current-content diagnostic cannot identify a replacement numeric slot."""

import time
from typing import TYPE_CHECKING

import pytest

from tests.flitzis_looper.conftest import (
    FakeGlobalPlaybackBatchTicket,
    FakeInputRuntimePadBinding,
)
from tests.flitzis_looper.controller.transport.test_global_batches import (
    _accepted_sources,
    _assert_no_unguarded_batch_effect,
    _published_capture,
    _set_start_ticket,
)

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _replace_original_slot(
    controller: AppController, bindings: dict[int, FakeInputRuntimePadBinding]
) -> None:
    """Mirror Loader's context invalidation and an independent replacement assignment."""
    controller.transport.residency.cancel(0)
    controller.project.sample_paths[0] = "samples/replacement.wav"
    bindings[0] = FakeInputRuntimePadBinding(0, authority_revision=2)
    assert controller.transport.residency.status(0)[1] is None


@pytest.mark.parametrize("old_slot_fenced", [False, True])
def test_rejected_published_global_with_no_current_content_never_targets_old_slot(
    controller: AppController, audio_engine_mock: Mock, *, old_slot_fenced: bool
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0}
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    _set_start_ticket(audio_engine_mock, ticket)
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=17)
    capture = _published_capture(audio_engine_mock)
    _replace_original_slot(controller, bindings)
    before_project = controller.project.model_dump()
    before_session = controller.session.model_dump()
    capture.slots = []
    capture.status = "rejected"
    capture.failure = "source_changed"
    ticket.status = "rejected"
    controller.transport._pair_deferred_sample_ids = (
        frozenset({0}) if old_slot_fenced else frozenset()
    )

    controller.transport.playback._global_playback.poll()

    assert controller.transport.playback._global_playback._pending is None
    assert controller.transport.residency.status(0)[1] is None
    assert controller.transport.residency._launch_errors == {}
    assert controller.project.model_dump() == before_project
    assert controller.session.model_dump() == before_session
    audio_engine_mock.capture_global_start.assert_called_once()
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(capture)
    _assert_no_unguarded_batch_effect(audio_engine_mock)


@pytest.mark.parametrize("terminal", ["rejected", "deadline"])
@pytest.mark.parametrize("old_slot_fenced", [False, True])
def test_deferred_global_with_removed_content_retires_without_replacement_error(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    terminal: str,
    *,
    old_slot_fenced: bool,
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0}
    audio_engine_mock.publish_captured_global_start.side_effect = lambda _capture: None
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=23)
    capture = _published_capture(audio_engine_mock)
    control = controller.transport.playback._global_playback
    deferred = control._deferred
    assert deferred is not None
    _replace_original_slot(controller, bindings)
    before_project = controller.project.model_dump()
    before_session = controller.session.model_dump()
    capture.slots = []
    controller.transport._pair_deferred_sample_ids = (
        frozenset({0}) if old_slot_fenced else frozenset()
    )
    if terminal == "rejected":
        capture.status = "rejected"
        capture.failure = "prepared_mismatch"
    else:
        monkeypatch.setattr(time, "monotonic", lambda: deferred.deadline + 1.0)

    control.poll()

    assert control._deferred is None
    assert control._pending is None
    assert capture.state() == "rejected"
    assert controller.transport.residency.status(0)[1] is None
    assert controller.transport.residency._launch_errors == {}
    assert controller.project.model_dump() == before_project
    assert controller.session.model_dump() == before_session
    audio_engine_mock.capture_global_start.assert_called_once()
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(capture)
    _assert_no_unguarded_batch_effect(audio_engine_mock)


@pytest.mark.parametrize("published", [False, True], ids=["deferred", "published"])
def test_temporary_empty_placement_diagnostics_do_not_retire_live_original_capture(
    controller: AppController, audio_engine_mock: Mock, *, published: bool
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0}
    if published:
        _set_start_ticket(audio_engine_mock, FakeGlobalPlaybackBatchTicket("pending"))
    else:
        audio_engine_mock.publish_captured_global_start.side_effect = lambda _capture: None
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=31)
    capture = _published_capture(audio_engine_mock)
    control = controller.transport.playback._global_playback
    owner = control._pending if published else control._deferred
    assert owner is not None
    capture.slots = []
    controller.transport._pair_deferred_sample_ids = frozenset({0})
    before_project = controller.project.model_dump()
    before_session = controller.session.model_dump()

    control.poll()

    assert (control._pending if published else control._deferred) is owner
    assert capture.cancelled is False
    assert capture.received_at_ns == 31
    assert controller.transport.residency._launch_errors == {}
    assert controller.project.model_dump() == before_project
    assert controller.session.model_dump() == before_session
    audio_engine_mock.capture_global_start.assert_called_once()
    assert all(
        invocation.args == (capture,)
        for invocation in audio_engine_mock.publish_captured_global_start.call_args_list
    )
    _assert_no_unguarded_batch_effect(audio_engine_mock)
