from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("revision", [None, True, 8])
def test_refused_or_stale_midi_selection_cannot_replay_against_current_content(
    controller: AppController, audio_engine_mock: Mock, revision: object
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    audio_engine_mock.select_input_pad = Mock()
    audio_engine_mock.input_selected_projection = Mock(return_value=(7, 0))
    original = (
        controller.project.selected_pad,
        controller.session.pad_selection_revision,
        controller.session.pad_selection_content_id,
    )
    refused = {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "ui.select_pad:1",
        "value": 100,
        "direct": False,
        "dispatched": False,
        "selection_revision": revision,
    }
    for _ in range(2):
        controller.input_mapping._handle_rust_input_event(refused)
        assert (
            controller.project.selected_pad,
            controller.session.pad_selection_revision,
            controller.session.pad_selection_content_id,
        ) == original
    audio_engine_mock.select_input_pad.assert_not_called()

    # A fresh committed receipt projects its exact Native selection without
    # issuing another selection command or replaying the refused gesture.
    audio_engine_mock.input_selected_projection.return_value = (9, 1)
    controller.input_mapping._handle_rust_input_event({
        **refused,
        "dispatched": True,
        "selection_revision": 9,
    })
    assert controller.project.selected_pad == 1
    assert controller.session.pad_selection_revision == original[1] + 1
    audio_engine_mock.select_input_pad.assert_not_called()
