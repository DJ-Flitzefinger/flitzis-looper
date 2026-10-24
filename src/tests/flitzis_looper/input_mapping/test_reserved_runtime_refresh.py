"""Arrangement observer fences preserve successful runtime rows, including empties."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def _publish_original_rows(controller: AppController) -> None:
    for sample_id in (0, 6):
        controller.project.sample_paths[sample_id] = f"samples/original-{sample_id}.wav"
        controller.project.sample_durations[sample_id] = 12.0
        controller.project.manual_bpm[sample_id] = 120.0
    assert controller.input_mapping._refresh_rust_runtime_state()


def test_reserved_loaded_and_empty_rows_survive_refresh_while_outside_row_updates(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _publish_original_rows(controller)
    original = controller.input_mapping._last_input_runtime_state_signature
    assert original is not None
    controller.transport._pair_deferred_sample_ids = frozenset({0, 1})
    # Intermediate project edits must not turn the last loaded row into removal
    # or publish the copy target before the arrangement releases its observers.
    controller.project.sample_paths[0] = None
    controller.project.sample_paths[1] = "samples/incoming.wav"
    controller.project.sample_durations[1] = 12.0
    controller.project.pad_loop_start_s[6] = 0.25
    audio_engine_mock.reset_mock()

    assert controller.input_mapping._refresh_rust_runtime_state()

    signature = controller.input_mapping._last_input_runtime_state_signature
    assert signature is not None
    assert signature[2][0] is original[2][0]
    assert signature[2][1] is original[2][1]
    assert signature[2][6].start_seconds == 0.25
    _, loaded, starts, _, bindings = audio_engine_mock.set_input_runtime_state.call_args.args
    assert loaded[0] is True
    assert loaded[1] is False
    assert bindings[0] is original[2][0].binding
    assert bindings[1] is None
    assert starts[6] == 0.25
    audio_engine_mock.current_input_runtime_pad_binding.assert_called_once_with(6)

    controller.transport._pair_deferred_sample_ids = frozenset()
    audio_engine_mock.reset_mock()
    assert controller.input_mapping._refresh_rust_runtime_state()
    _, loaded, _, _, bindings = audio_engine_mock.set_input_runtime_state.call_args.args
    assert loaded[0] is False
    assert bindings[0] is None
    assert loaded[1] is True
    assert bindings[1] is not None
    audio_engine_mock.reset_mock()
    assert controller.input_mapping._refresh_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_not_called()


def test_first_runtime_publication_waits_for_reserved_observers_without_caching_removal(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/original.wav"
    controller.input_mapping._last_input_runtime_state_signature = None
    controller.transport._pair_deferred_sample_ids = frozenset({0})
    request = Mock(return_value=None)
    audio_engine_mock.request_input_runtime_policy_adoption = request
    audio_engine_mock.reset_mock()

    assert not controller.input_mapping._refresh_rust_runtime_state()
    assert controller.input_mapping._last_input_runtime_state_signature is None
    audio_engine_mock.set_input_runtime_state.assert_not_called()
    audio_engine_mock.current_input_runtime_pad_binding.assert_not_called()
    request.assert_not_called()

    controller.transport._pair_deferred_sample_ids = frozenset()
    assert controller.input_mapping._refresh_rust_runtime_state()
    assert audio_engine_mock.set_input_runtime_state.call_args.args[1][0] is True
    request.assert_called_once_with(service=True)


def test_failed_reserved_refresh_retries_from_last_successful_rows(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _publish_original_rows(controller)
    original = controller.input_mapping._last_input_runtime_state_signature
    assert original is not None
    controller.transport._pair_deferred_sample_ids = frozenset({0})
    controller.project.pad_loop_start_s[6] = 0.5
    audio_engine_mock.set_input_runtime_state.side_effect = RuntimeError("claim changed")

    assert not controller.input_mapping._refresh_rust_runtime_state()
    assert controller.input_mapping._last_input_runtime_state_signature is original

    audio_engine_mock.set_input_runtime_state.side_effect = None
    assert controller.input_mapping._refresh_rust_runtime_state()
    retried = controller.input_mapping._last_input_runtime_state_signature
    assert retried is not None
    assert retried[2][0] is original[2][0]
    assert retried[2][6].start_seconds == 0.5
    assert controller.input_mapping._last_input_runtime_state_error is None


def test_explicit_multi_loop_change_still_reaches_native_while_participants_are_fenced(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _publish_original_rows(controller)
    original = controller.input_mapping._last_input_runtime_state_signature
    assert original is not None
    controller.transport._pair_deferred_sample_ids = frozenset({0, 1})
    controller.project.multi_loop = not original[0]
    audio_engine_mock.reset_mock()

    assert controller.input_mapping._refresh_rust_runtime_state()

    assert audio_engine_mock.set_input_runtime_state.call_args.args[0] is not original[0]
    signature = controller.input_mapping._last_input_runtime_state_signature
    assert signature is not None
    assert signature[2][0] is original[2][0]
    assert signature[2][1] is original[2][1]
