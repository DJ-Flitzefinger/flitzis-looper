from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

from flitzis_looper.ui.context import SettingsActions
from flitzis_looper.ui.render import settings

if TYPE_CHECKING:
    import pytest

    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext


def test_workload_ui_reads_retained_report_and_only_copies_on_click(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    workload = Mock(active=False, report_text='{"capture_status":"complete"}', summary="Complete")
    actions = SettingsActions(cast("AppController", SimpleNamespace(audio_workload=workload)))
    ctx = cast("UiContext", SimpleNamespace(ui=SimpleNamespace(settings=actions)))
    gui = Mock()
    monkeypatch.setattr(settings, "imgui", gui)
    gui.button.return_value = False
    settings._audio_workload_controls(ctx)
    workload.poll.assert_not_called()
    workload.start.assert_not_called()
    gui.set_clipboard_text.assert_not_called()
    gui.button.side_effect = lambda label: label == "Copy workload report"
    settings._audio_workload_controls(ctx)
    gui.set_clipboard_text.assert_called_once_with(workload.report_text)
    gui.button.side_effect = lambda label: label == "Capture audio workload"
    settings._audio_workload_controls(ctx)
    workload.start.assert_called_once_with()
    workload.active = True
    gui.button.side_effect = lambda label: label == "Cancel capture"
    settings._audio_workload_controls(ctx)
    workload.cancel.assert_called_once_with()
