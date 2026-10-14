"""Compose real Loader/Controller owners; Native callback effects are tested separately."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController


def _retire_unrelated_assignment(
    controller: AppController, audio_engine_mock: Mock, operation: str
) -> None:
    if operation in {"unload", "failed_unload"}:
        if operation == "failed_unload":
            audio_engine_mock.unload_sample.side_effect = RuntimeError("owned queue pressure")
            with pytest.raises(RuntimeError, match="owned queue pressure"):
                controller.loader.unload_sample(36)
        else:
            controller.loader.unload_sample(36)
            assert controller.project.sample_paths[36] is None
    else:
        audio_engine_mock.load_incoming_sample_async.return_value = 7
        controller.loader.load_sample_async(36, "incoming.wav")
        event = (
            {"type": "error", "id": 36, "request_id": 7, "msg": "owned preparation failed"}
            if operation == "failed_replace"
            else {
                "type": "success",
                "id": 36,
                "request_id": 7,
                "cached_path": "samples/new-36.wav",
                "duration_s": 32.0,
                "incoming_defaults": {
                    "revision": 0,
                    "key_lock": False,
                    "start_s": 0.0,
                    "end_s": None,
                    "period_seconds": None,
                    "grid_origin_s": 0.0,
                    "loop_auto": True,
                    "loop_bars": 8.0,
                },
            }
        )
        audio_engine_mock.poll_loader_events.side_effect = [event, None]
        controller.loader.poll_loader_events()
        expected = (
            "samples/original-36.wav" if operation == "failed_replace" else "samples/new-36.wav"
        )
        assert controller.project.sample_paths[36] == expected


@pytest.mark.parametrize("operation", ["unload", "replace", "failed_unload", "failed_replace"])
def test_unrelated_assignment_retirement_preserves_original_physical_hold(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path, operation: str
) -> None:
    samples = tmp_path / "samples"
    samples.mkdir(exist_ok=True)
    for sample_id in (0, 36):
        path = samples / f"original-{sample_id}.wav"
        write_mono_pcm16_wav(path, 44_100)
        controller.project.sample_paths[sample_id] = f"samples/{path.name}"
        controller.project.sample_durations[sample_id] = 32.0
    controller.session.waveform_editor_open = True
    controller.session.waveform_editor_pad_id = 0
    ticket = Mock(
        spec=[
            "publication_status",
            "request_release",
            "release_requested",
            "release_admitted",
            "current_control_slot",
            "is_current",
        ]
    )
    ticket.publication_status.return_value = "paused_owned"
    ticket.release_requested.return_value = False
    ticket.release_admitted.return_value = False
    ticket.current_control_slot.return_value = 0
    ticket.is_current.return_value = True
    audio_engine_mock.pause_waveform_hold.return_value = ticket
    waveform = controller.transport.waveform
    waveform.press_pause_hold(0, received_at_ns=123456789)
    waveform.poll_holds()
    assert controller.session.waveform_pause_hold_pad_id == 0
    press = waveform._physical_hold_press
    assert press is not None

    _retire_unrelated_assignment(controller, audio_engine_mock, operation)

    waveform.poll_holds()
    ticket.request_release.assert_not_called()
    audio_engine_mock.release_waveform_hold.assert_not_called()
    assert waveform._physical_hold_press == press
    assert waveform._hold_owners[press].ticket is ticket
    assert waveform.has_pending_holds
    assert controller.session.waveform_pause_hold_pad_id == 0
    assert controller.session.waveform_editor_pad_id == 0
    assert controller.project.sample_paths[0] == "samples/original-0.wav"

    waveform.request_hold_release("button_up")
    ticket.request_release.assert_called_once_with()
    audio_engine_mock.release_waveform_hold.return_value = True
    waveform.poll_holds()
    audio_engine_mock.release_waveform_hold.assert_called_once_with(ticket)
    ticket.publication_status.return_value = "resumed"
    waveform.poll_holds()
    assert not waveform.has_pending_holds
