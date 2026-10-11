"""Performer input, content replacement, loader admission and transaction ownership."""

from typing import TYPE_CHECKING
from unittest.mock import call

import pytest

from flitzis_looper.input_mapping import KeyboardBinding, LooperAction
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.transport.test_key_lock_transactions import _mode_source

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.performance_confirmation import ConfirmationAction


def _seed(controller: AppController, audio: Mock) -> UiContext:
    controller.project.sample_paths[0] = "samples/shared.wav"
    controller.project.sample_paths[1] = "samples/other.wav"
    controller.project.pad_content[0] = PadContentIdentity(
        instance_id="1" * 32, material_id="a" * 32
    )
    audio.waveform_source_identity.return_value = (7, "b" * 64, 128, 44_100)
    audio.analyze_sample_async.return_value = 11
    return UiContext(controller)


@pytest.mark.parametrize("action", ["unload", "analyze"])
def test_selection_change_and_duplicate_request_accept_original_content_once(
    controller: AppController, audio_engine_mock: Mock, action: ConfirmationAction
) -> None:
    ctx = _seed(controller, audio_engine_mock)
    confirmation = controller.performance_confirmation
    confirmation.request(action, 0)
    intent = confirmation.pending
    assert intent is not None
    before = controller.project.model_dump()
    controller.project.selected_pad = 1
    confirmation.request(action, 1)
    assert confirmation.pending is intent
    assert controller.project.sample_paths[:2] == before["sample_paths"][:2]
    audio_engine_mock.unload_sample.assert_not_called()
    audio_engine_mock.analyze_sample_async.assert_not_called()
    confirmation.accept(intent)
    confirmation.accept(intent)
    confirmation.dismiss(intent)
    assert confirmation.pending is None
    target = (
        audio_engine_mock.unload_sample
        if action == "unload"
        else audio_engine_mock.analyze_sample_async
    )
    target.assert_called_once_with(0)
    assert controller.project.sample_paths[1] == "samples/other.wav"
    assert ctx.state.project.selected_pad == 1


@pytest.mark.parametrize("action", ["unload", "analyze"])
@pytest.mark.parametrize(
    "replacement",
    ["instance", "generation", "digest", "shape", "path", "missing", "unloaded", "loading"],
)
def test_stale_or_ineligible_acceptance_has_no_audio_or_project_effect(
    controller: AppController,
    audio_engine_mock: Mock,
    action: ConfirmationAction,
    replacement: str,
) -> None:
    _seed(controller, audio_engine_mock)
    confirmation = controller.performance_confirmation
    confirmation.request(action, 0)
    intent = confirmation.pending
    assert intent is not None
    if replacement == "instance":
        controller.project.pad_content[0] = PadContentIdentity(
            instance_id="2" * 32, material_id="a" * 32
        )
    elif replacement == "generation":
        audio_engine_mock.waveform_source_identity.return_value = (8, "b" * 64, 128, 44_100)
    elif replacement == "digest":
        audio_engine_mock.waveform_source_identity.return_value = (7, "c" * 64, 128, 44_100)
    elif replacement == "shape":
        audio_engine_mock.waveform_source_identity.return_value = (7, "b" * 64, 256, 48_000)
    elif replacement == "path":
        controller.project.sample_paths[0] = "samples/rebound.wav"
    elif replacement == "missing":
        audio_engine_mock.waveform_source_identity.side_effect = RuntimeError("source disappeared")
    elif replacement == "unloaded":
        controller.project.sample_paths[0] = None
        controller.project.pad_content[0] = None
    else:
        controller.session.loading_sample_ids.add(0)
    project_before = controller.project.model_dump()
    session_before = controller.session.model_dump()
    audio_engine_mock.reset_mock()
    confirmation.accept(intent)
    assert confirmation.pending is None
    assert controller.project.model_dump() == project_before
    assert controller.session.model_dump() == session_before
    assert all(item == call.waveform_source_identity(0) for item in audio_engine_mock.mock_calls)


@pytest.mark.parametrize("action", ["unload", "analyze"])
def test_dismissed_snapshot_cannot_accept_or_cancel_later_identical_warning(
    controller: AppController, audio_engine_mock: Mock, action: ConfirmationAction
) -> None:
    _seed(controller, audio_engine_mock)
    confirmation = controller.performance_confirmation
    confirmation.request(action, 0)
    old = confirmation.pending
    assert old is not None
    confirmation.dismiss(old)
    confirmation.request(action, 0)
    new = confirmation.pending
    assert new is not None
    assert new is not old
    audio_engine_mock.reset_mock()
    confirmation.accept(old)
    confirmation.dismiss(old)
    assert confirmation.pending is new
    assert audio_engine_mock.mock_calls == []


@pytest.mark.parametrize(
    "identity",
    [
        None,
        (True, "b" * 64, 128, 44_100),
        (7, "b" * 64, 0, 44_100),
        (7, "b" * 64, 128),
        (7, None, 128, 44_100),
    ],
)
def test_unobserved_native_content_cannot_open_warning(
    controller: AppController, audio_engine_mock: Mock, identity: object
) -> None:
    _seed(controller, audio_engine_mock)
    audio_engine_mock.waveform_source_identity.return_value = identity
    controller.performance_confirmation.request("unload", 0)
    controller.performance_confirmation.request("analyze", 0)
    assert controller.performance_confirmation.pending is None
    audio_engine_mock.unload_sample.assert_not_called()
    audio_engine_mock.analyze_sample_async.assert_not_called()


@pytest.mark.parametrize("ineligible", ["loading", "analyzing", "unloaded"])
def test_manual_analysis_ineligible_at_request_and_acceptance_is_noop(
    controller: AppController, audio_engine_mock: Mock, ineligible: str
) -> None:
    _seed(controller, audio_engine_mock)
    confirmation = controller.performance_confirmation
    confirmation.request("analyze", 0)
    intent = confirmation.pending
    assert intent is not None
    if ineligible == "loading":
        controller.session.loading_sample_ids.add(0)
    elif ineligible == "analyzing":
        controller.session.analyzing_sample_ids.add(0)
    else:
        controller.project.sample_paths[0] = None
    project = controller.project.model_dump()
    session = controller.session.model_dump()
    confirmation.accept(intent)
    confirmation.request("analyze", 0)
    assert confirmation.pending is None
    assert controller.project.model_dump() == project
    assert controller.session.model_dump() == session
    audio_engine_mock.analyze_sample_async.assert_not_called()


@pytest.mark.parametrize("source", ["keyboard", "midi", "sidebar"])
@pytest.mark.parametrize("action", ["unload", "analyze"])
def test_performer_inputs_share_warning_and_learn_is_capture_only(
    controller: AppController, audio_engine_mock: Mock, source: str, action: ConfirmationAction
) -> None:
    ctx = _seed(controller, audio_engine_mock)
    mapping = controller.input_mapping
    mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="A")
    looper_action = (
        LooperAction.unload_pad(0) if action == "unload" else LooperAction.analyze_pad(0)
    )
    mapping.save_mapping("keyboard", binding.key, looper_action)
    mapping.save_mapping("midi", "midi:note:1:60", looper_action)

    def deliver() -> None:
        if source == "keyboard":
            assert mapping.capture_keyboard_input(binding, text_input_focused=False)
        elif source == "midi":
            mapping._handle_rust_input_event({
                "source": "midi",
                "binding_key": "midi:note:1:60",
                "action_key": looper_action.key,
            })
        elif action == "unload":
            ctx.audio.pads.unload_sample(0)
        else:
            ctx.audio.pads.analyze_sample_async(0)

    deliver()
    intent = controller.performance_confirmation.pending
    assert intent is not None
    assert (intent.action, intent.pad_id) == (action, 0)
    audio_engine_mock.unload_sample.assert_not_called()
    audio_engine_mock.analyze_sample_async.assert_not_called()
    controller.performance_confirmation.dismiss(intent)
    mapping.toggle_learn()
    deliver()
    assert controller.performance_confirmation.pending is None
    audio_engine_mock.unload_sample.assert_not_called()
    audio_engine_mock.analyze_sample_async.assert_not_called()


@pytest.mark.parametrize("status", ["preparing", "adopting"])
@pytest.mark.parametrize("action", ["unload", "analyze"])
def test_warning_dismiss_and_failed_unload_preserve_keylock_transaction(
    controller: AppController, audio_engine_mock: Mock, status: str, action: ConfirmationAction
) -> None:
    ctx = _seed(controller, audio_engine_mock)
    ticket, feedback = _mode_source(controller, audio_engine_mock)
    ticket.status = status
    controller.session.active_sample_ids.add(0)
    ctx.ui.waveform.open(0)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    residency = controller.transport.residency
    pending = residency._pending[0]
    state_before = residency.key_lock_status(0)
    project_before = controller.project.model_dump()
    session_before = controller.session.model_dump()
    feedback_before = dict(feedback)
    audio_engine_mock.reset_mock()
    ctx.audio.pads.unload_sample(0) if action == "unload" else ctx.audio.pads.analyze_sample_async(
        0
    )
    intent = ctx.ui.confirmation.pending
    assert intent is not None
    ctx.ui.confirmation.dismiss(intent)
    ctx.ui.confirmation.accept(intent)
    assert residency._pending[0] is pending
    assert ticket.status == status
    assert not ticket.launch_cancelled
    assert controller.project.model_dump() == project_before
    assert controller.session.model_dump() == session_before
    assert feedback == feedback_before
    assert residency.key_lock_status(0) == state_before
    assert all(
        item[0]
        in {"waveform_source_identity", "pad_key_lock_status", "current_input_runtime_pad_binding"}
        for item in audio_engine_mock.mock_calls
    )
    if action == "unload":
        ctx.audio.pads.unload_sample(0)
        second = ctx.ui.confirmation.pending
        assert second is not None
        audio_engine_mock.unload_sample.side_effect = RuntimeError("native admission full")
        ctx.ui.confirmation.accept(second)
        failed = ctx.ui.confirmation.pending
        assert failed is not None
        assert failed.error == "native admission full"
        assert not ctx.ui.confirmation.is_current(failed)
        ctx.ui.confirmation.accept(second)
        assert ctx.ui.confirmation.pending is failed
        audio_engine_mock.unload_sample.assert_called_once_with(0)
        ctx.ui.confirmation.dismiss(failed)
        assert residency._pending[0] is pending
        assert ticket.status == status
        assert not ticket.launch_cancelled
        assert controller.project.model_dump() == project_before
        assert controller.session.model_dump() == session_before
        audio_engine_mock.retry_waveform.assert_not_called()


def test_same_content_mode_window_readiness_change_does_not_revoke_warning(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _seed(controller, audio_engine_mock)
    ticket, feedback = _mode_source(controller, audio_engine_mock)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    confirmation = controller.performance_confirmation
    confirmation.request("analyze", 0)
    intent = confirmation.pending
    assert intent is not None
    ticket.status = "accepted"
    feedback.update(effective=True, ready=True, request_id=2, window_revision=2)
    controller.transport.residency.poll()
    assert confirmation.is_current(intent)
    confirmation.accept(intent)
    audio_engine_mock.analyze_sample_async.assert_called_once_with(0)


def test_mapped_adjust_releases_native_view_and_invalidates_ui_projection_cache(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ctx = _seed(controller, audio_engine_mock)
    controller.project.sample_durations[0] = 12.0
    first, second = object(), object()
    audio_engine_mock.get_waveform_render_data.side_effect = [first, second]
    ctx.ui.waveform.open(0)
    assert ctx.ui.waveform.get_render_data(0, 320, 0, 10) is first
    controller.input_mapping.execute_action(LooperAction.adjust_loop(0))
    assert not controller.session.waveform_editor_open
    audio_engine_mock.retry_waveform.assert_called_once_with(0)
    controller.input_mapping.execute_action(LooperAction.adjust_loop(0))
    assert ctx.ui.waveform.get_render_data(0, 320, 0, 10) is second
    assert audio_engine_mock.get_waveform_render_data.call_count == 2


@pytest.mark.parametrize("source", ["keyboard", "midi"])
def test_mapped_adjust_toggle_retarget_and_learn_share_release_authority(
    controller: AppController, audio_engine_mock: Mock, source: str
) -> None:
    ctx = _seed(controller, audio_engine_mock)
    mapping = controller.input_mapping
    mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="W")
    mapping.save_mapping("keyboard", binding.key, LooperAction.adjust_loop(0))
    mapping.save_mapping("midi", "midi:note:1:60", LooperAction.adjust_loop(0))

    def deliver() -> None:
        if source == "keyboard":
            assert mapping.capture_keyboard_input(binding, text_input_focused=False)
        else:
            mapping._handle_rust_input_event({
                "source": "midi",
                "binding_key": "midi:note:1:60",
                "action_key": "pad.adjust_loop:0",
            })

    ctx.ui.waveform.open(1)
    deliver()
    assert controller.session.waveform_editor_pad_id == 0
    audio_engine_mock.retry_waveform.assert_called_once_with(1)
    deliver()
    assert not controller.session.waveform_editor_open
    assert audio_engine_mock.retry_waveform.call_args_list == [call(1), call(0)]
    mapping.toggle_learn()
    deliver()
    assert not controller.session.waveform_editor_open
    assert audio_engine_mock.retry_waveform.call_count == 2


def test_confirmed_unload_releases_only_edited_target_after_native_admission(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ctx = _seed(controller, audio_engine_mock)
    ctx.ui.waveform.open(1)
    ctx.audio.pads.unload_sample(0)
    intent = ctx.ui.confirmation.pending
    assert intent is not None
    ctx.ui.confirmation.accept(intent)
    assert controller.session.waveform_editor_pad_id == 1
    audio_engine_mock.retry_waveform.assert_not_called()
    ctx.audio.pads.unload_sample(1)
    intent = ctx.ui.confirmation.pending
    assert intent is not None
    audio_engine_mock.reset_mock()
    ctx.ui.confirmation.accept(intent)
    assert not controller.session.waveform_editor_open
    assert audio_engine_mock.mock_calls.index(
        call.unload_sample(1)
    ) < audio_engine_mock.mock_calls.index(call.retry_waveform(1))
