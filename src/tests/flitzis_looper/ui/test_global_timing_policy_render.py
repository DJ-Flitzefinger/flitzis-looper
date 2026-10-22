"""Visible global actions and fixed-grid routing without devices or inference."""

from contextlib import nullcontext
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

from flitzis_looper.controller.current_timing import CurrentPadTiming
from flitzis_looper.controller.transport.timing_policy import GlobalTimingControlStatus
from flitzis_looper.timing_policy import GlobalTimingPolicy
from flitzis_looper.ui.render import timing_policy as render
from flitzis_looper.ui.render import waveform_editor

if TYPE_CHECKING:
    import pytest
    from imgui_bundle import imgui

    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.render.source_beatmap_editor import SourceMapEditorFrame


def test_global_buttons_keep_actions_separate_and_show_pending_actual_audio(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    text: list[str] = []
    sync, realtime = Mock(), Mock()
    status = GlobalTimingControlStatus(
        requested=False,
        preference=False,
        effective=True,
        pending=True,
        detail="Start/retrigger to apply",
    )
    ctx = cast(
        "UiContext",
        SimpleNamespace(
            state=SimpleNamespace(
                global_=SimpleNamespace(timing_policy_status=lambda _control: status)
            ),
            audio=SimpleNamespace(
                global_=SimpleNamespace(toggle_source_sync=sync, toggle_realtime_bpm=realtime)
            ),
        ),
    )
    monkeypatch.setattr(render, "button_style", lambda _style: nullcontext())
    monkeypatch.setattr(
        render,
        "imgui",
        SimpleNamespace(
            button=lambda label, _size: label == "SYNC##global-sync",
            is_item_hovered=lambda: False,
            text_colored=lambda _color, value: text.append(value),
        ),
    )
    render.render_global_timing_policy(ctx)
    sync.assert_called_once_with()
    realtime.assert_not_called()
    assert text == ["Applied ON | pending; start/retrigger", "Audio ON | pending; start/retrigger"]


def test_fixed_global_source_timing_keeps_selected_map_out_of_generic_waveform_grid(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    policy = GlobalTimingPolicy(schema_version=1, sync=True, realtime_bpm=False, revision=2)
    ctx = cast(
        "UiContext",
        SimpleNamespace(
            state=SimpleNamespace(project=SimpleNamespace(global_timing_policy=policy))
        ),
    )
    selected = cast("SourceMapEditorFrame", SimpleNamespace(selected=True))
    mapped, scalar = Mock(), Mock(return_value=None)
    monkeypatch.setattr(waveform_editor, "_plot_source_map_grid", mapped)
    monkeypatch.setattr(waveform_editor, "_plot_px_per_sec", scalar)
    waveform_editor._plot_musical_grid(
        ctx,
        0,
        cast("imgui.ImDrawList", object()),
        0.0,
        4.0,
        timing=CurrentPadTiming(0.5, 0.0, 48000),
        source_map=selected,
    )
    mapped.assert_not_called()
    scalar.assert_called_once_with(start_s=0.0, end_s=4.0)
