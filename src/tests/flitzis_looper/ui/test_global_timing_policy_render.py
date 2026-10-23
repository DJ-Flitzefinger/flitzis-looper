"""Visible global actions and fixed-grid routing without devices or inference."""

from contextlib import nullcontext
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.current_timing import CurrentPadTiming
from flitzis_looper.controller.transport.timing_policy import GlobalTimingControlStatus
from flitzis_looper.models import ProjectState
from flitzis_looper.timing_policy import GlobalTimingPolicy
from flitzis_looper.ui.render import timing_policy as render
from flitzis_looper.ui.render import waveform_editor

if TYPE_CHECKING:
    from imgui_bundle import imgui

    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.render.source_beatmap_editor import SourceMapEditorFrame


@pytest.mark.parametrize(
    "detail",
    [
        "Live timing change pending.",
        "Timing change applied; tempo is settling.",
        "Live timing change refused.",
        "Start/retrigger to apply this timing change.",
    ],
)
def test_global_buttons_keep_actions_separate_and_show_pending_actual_audio(
    monkeypatch: pytest.MonkeyPatch, detail: str
) -> None:
    text: list[str] = []
    guidance: list[str] = []
    tooltips: list[str] = []
    sync, realtime = Mock(), Mock()
    status = GlobalTimingControlStatus(
        requested=False,
        preference=False,
        effective=True,
        pending=True,
        detail=detail,
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
            begin_disabled=lambda _disabled: None,
            end_disabled=lambda: None,
            is_item_hovered=lambda: True,
            text_colored=lambda _color, value: text.append(value),
            text_wrapped=guidance.append,
            set_tooltip=tooltips.append,
        ),
    )
    render.render_global_timing_policy(ctx)
    sync.assert_called_once_with()
    realtime.assert_not_called()
    assert text == ["Mode ON | pending | grid phase unconfirmed", "Applied ON | pending"]
    assert guidance == [detail, detail]
    assert all("Requested OFF." in tooltip for tooltip in tooltips)
    assert all(detail in tooltip for tooltip in tooltips)
    if detail != "Start/retrigger to apply this timing change.":
        assert all("Start/retrigger" not in tooltip for tooltip in tooltips)


def test_fixed_global_source_timing_keeps_selected_map_out_of_generic_waveform_grid(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    policy = GlobalTimingPolicy(schema_version=1, sync=True, realtime_bpm=False, revision=2)
    ctx = cast(
        "UiContext",
        SimpleNamespace(state=SimpleNamespace(project=ProjectState(global_timing_policy=policy))),
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
    mapped.assert_called_once()
    assert mapped.call_args.kwargs["draw_grid"] is False
    scalar.assert_called_once_with(start_s=0.0, end_s=4.0)
