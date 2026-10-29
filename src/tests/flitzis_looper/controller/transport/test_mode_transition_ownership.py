"""Native-admitted mode fences and independent scalar Lock retain their owners."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.input_mapping import LooperAction
from flitzis_looper.models import PadContentIdentity, ProjectState
from tests.flitzis_looper.conftest import FakeGlobalPlaybackBatchTicket
from tests.flitzis_looper.controller.bpm_lock_fixture import _configured
from tests.flitzis_looper.controller.transport.test_global_batches import (
    _accepted_sources,
    _pending_loop_edit,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize(
    ("multi_loop", "lock"), [(False, False), (False, True), (True, False), (True, True)]
)
def test_lock_wish_is_independent_of_multi_loop(*, multi_loop: bool, lock: bool) -> None:
    project = ProjectState(multi_loop=multi_loop, bpm_lock=lock)
    assert project.resolve_bpm_lock() is lock


@pytest.mark.parametrize("pending_group", [False, True])
def test_mode_collapse_retains_pending_acks_and_never_cancels_after_native_admission(
    controller: AppController, audio_engine_mock: Mock, *, pending_group: bool
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    assert controller.input_mapping._refresh_rust_runtime_state()
    owner = controller.transport.playback._global_playback
    if pending_group:
        previous = FakeGlobalPlaybackBatchTicket("pending")
        audio_engine_mock.publish_captured_global_start.side_effect = lambda capture: (
            capture.publish(previous)
        )
        owner.start()
        pending = owner._pending
        assert pending is not None
        assert pending.capture is not None
        capture = pending.capture
        gate = None
    else:
        gate = owner.capture_recording_start()
        assert owner._deferred is not None
        capture = owner._deferred.capture
        pending = None
    capture.cancel = Mock(
        side_effect=AssertionError("No cancellation after Native committed its fence")
    )
    transition = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.set_input_runtime_state.return_value = transition
    audio_engine_mock.reset_mock()

    controller.transport.global_params.set_multi_loop(enabled=False)

    assert controller.project.multi_loop is False
    assert owner._deferred is None
    assert owner._pending is pending
    assert owner._fenced_batches == [transition]
    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.global_stop_restore_sample_ids == set()
    if gate is not None:
        assert not gate.is_current()
    capture.cancel.assert_not_called()
    audio_engine_mock.stop_explicit_playback.assert_not_called()
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    audio_engine_mock.cancel_all_launches.assert_not_called()
    audio_engine_mock.cancel_pad_launches.assert_not_called()
    owner.poll()
    assert owner._fenced_batches == [transition]


def test_refused_mode_collapse_keeps_last_whole_image_mode_and_group_owner(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    assert controller.input_mapping._refresh_rust_runtime_state()
    signature = controller.input_mapping._last_input_runtime_state_signature
    owner = controller.transport.playback._global_playback
    gate = owner.capture_recording_start()
    deferred = owner._deferred
    audio_engine_mock.set_input_runtime_state.side_effect = RuntimeError("queue is full")
    before = controller.session.model_dump()
    with pytest.raises(RuntimeError, match="could not publish"):
        controller.transport.global_params.set_multi_loop(enabled=False)
    assert controller.project.multi_loop is True
    assert controller.input_mapping._last_input_runtime_state_signature is signature
    assert controller.session.model_dump() == before
    assert owner._deferred is deferred
    assert gate is not None
    assert gate.is_current()


def test_fence_retires_only_launch_callback_and_keeps_independent_resident_edit(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    edit = _pending_loop_edit(controller, audio_engine_mock)
    pending = controller.transport.residency._pending[0]
    launch = Mock()
    pending.after_ack = launch
    before = (pending.requested, pending.previous, pending.ticket)
    controller.project.multi_loop = False
    controller.transport.playback.stop_global_start_stop()
    assert controller.transport.residency._pending[0] is pending
    assert (pending.requested, pending.previous, pending.ticket) == before
    assert pending.after_ack is None
    assert not edit.cancelled
    launch.assert_not_called()


def test_observer_capacity_refuses_collapse_before_publication_but_not_ordinary_refresh(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    assert controller.input_mapping._refresh_rust_runtime_state()
    owner = controller.transport.playback._global_playback
    owner._fenced_batches = [FakeGlobalPlaybackBatchTicket("pending") for _ in range(256)]
    audio_engine_mock.reset_mock()
    with pytest.raises(RuntimeError, match="could not publish"):
        controller.transport.global_params.set_multi_loop(enabled=False)
    audio_engine_mock.set_input_runtime_state.assert_not_called()
    assert controller.project.multi_loop is True
    controller.project.pad_loop_start_s[0] = 0.25
    assert controller.input_mapping._refresh_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_called_once()
    assert len(owner._fenced_batches) == 256


def _confirmed_lock(
    controller: AppController, audio: Mock, *, multi_loop: bool
) -> dict[str, object]:
    receipt = _configured(controller, audio)
    controller.project.multi_loop = multi_loop
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    receipt.update(
        state="applied",
        effect_revision=receipt["request_revision"],
        effective_enabled=True,
        execution_frame=64,
    )
    parameters.poll_bpm_lock_application()
    assert parameters._bpm_lock_master_ready()
    return receipt


@pytest.mark.parametrize("multi_loop", [False, True])
def test_lock_reference_application_and_master_survive_pad_start_and_mode_changes(
    controller: AppController, audio_engine_mock: Mock, *, multi_loop: bool
) -> None:
    _confirmed_lock(controller, audio_engine_mock, multi_loop=multi_loop)
    parameters = controller.transport.global_params
    original = parameters._bpm_application
    master = (controller.session.master_bpm, controller.session.master_period_seconds)
    controller.project.sample_paths[2] = "samples/different-tempo.wav"
    controller.project.sample_durations[2] = 10.0
    controller.project.manual_bpm[2] = 90.0
    controller.project.selected_pad = 2
    audio_engine_mock.reset_mock()
    controller.transport.playback.trigger_pad(2)
    parameters.set_multi_loop(enabled=not multi_loop)
    parameters.set_multi_loop(enabled=multi_loop)
    assert parameters._bpm_application is original
    assert original is not None
    assert original.original_content_id == "a" * 32
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert (controller.session.master_bpm, controller.session.master_period_seconds) == master
    audio_engine_mock.set_bpm_lock.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()
    assert controller.project.resolve_bpm_lock()
    assert controller.input_mapping._input_pitch_projection(2).sync is True


def test_hierarchy_requalification_keeps_same_lock_owner_and_replacement_cannot_rebind(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipt = _confirmed_lock(controller, audio_engine_mock, multi_loop=False)
    parameters = controller.transport.global_params
    original = parameters._bpm_application
    master = controller.session.master_period_seconds
    original_binding = audio_engine_mock.set_bpm_lock.call_args.kwargs["anchor_binding"]
    prior_revision = receipt["request_revision"]
    audio_engine_mock.reset_mock()
    receipt.update(state="pending", reason="reference_changed", execution_frame=None)
    parameters.poll_bpm_lock_application()
    assert parameters._bpm_application is original
    assert original is not None
    audio_engine_mock.set_bpm_lock.assert_called_once()
    admission = audio_engine_mock.set_bpm_lock.call_args.kwargs
    assert admission["enabled"] is True
    reference_fields = (
        "source_id",
        "source_generation",
        "source_sha256",
        "content_lifetime",
        "source_identity",
    )
    assert {key: admission["anchor_binding"].metadata().get(key) for key in reference_fields} == {
        key: original_binding.metadata().get(key) for key in reference_fields
    }
    assert receipt["request_revision"] == prior_revision + 1
    assert original.admitted_revision == receipt["request_revision"]
    assert controller.session.master_period_seconds == master
    assert controller.session.bpm_lock_anchor_pad_id == 0
    controller.project.pad_content[0] = PadContentIdentity(instance_id="d" * 32)
    audio_engine_mock.reset_mock()
    parameters.poll_bpm_lock_application()
    assert parameters._bpm_application is original
    assert original is not None
    assert original.original_content_id == "a" * 32
    assert controller.session.master_period_seconds == master
    audio_engine_mock.set_bpm_lock.assert_not_called()


def test_lock_can_be_disabled_while_multi_loop_is_off(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _confirmed_lock(controller, audio_engine_mock, multi_loop=False)
    controller.transport.global_params.set_bpm_lock(enabled=False)
    audio_engine_mock.set_bpm_lock.assert_called_with(enabled=False)
    assert not controller.project.resolve_bpm_lock()


@pytest.mark.parametrize("multi_loop", [False, True])
def test_input_action_can_enable_independent_lock_and_projects_same_pitch_policy(
    controller: AppController, audio_engine_mock: Mock, *, multi_loop: bool
) -> None:
    _configured(controller, audio_engine_mock)
    controller.project.multi_loop = multi_loop
    assert controller.input_mapping.execute_action(LooperAction.toggle_bpm_lock())
    assert controller.project.bpm_lock
    assert audio_engine_mock.set_bpm_lock.call_args.kwargs["enabled"] is True
    assert controller.input_mapping._input_pitch_projection(0).sync is True


def test_empty_lock_reference_qualifies_first_content_once_without_following_selection(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _configured(controller, audio_engine_mock)
    project = controller.project
    project.multi_loop = False
    project.sample_paths[0] = None
    project.pad_content[0] = None
    project.manual_bpm[0] = None
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    application = parameters._bpm_application
    assert application is not None
    assert application.original_content_id is None
    project.selected_pad = 2
    project.sample_paths[0] = "samples/first-qualified-reference.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="c" * 32)
    project.manual_bpm[0] = 120.0
    parameters.poll_bpm_lock_application()
    assert parameters._bpm_application is application
    assert application.original_content_id == "c" * 32
    assert controller.session.bpm_lock_anchor_pad_id == 0
    audio_engine_mock.set_bpm_lock.assert_called_once()
    project.pad_content[0] = PadContentIdentity(instance_id="d" * 32)
    audio_engine_mock.reset_mock()
    parameters.poll_bpm_lock_application()
    assert parameters._bpm_application is application
    assert application.original_content_id == "c" * 32
    audio_engine_mock.set_bpm_lock.assert_not_called()
