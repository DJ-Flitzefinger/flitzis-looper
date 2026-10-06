from contextlib import nullcontext
from typing import TYPE_CHECKING
from unittest.mock import Mock

import numpy as np
import pytest
from imgui_bundle import implot

from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import waveform_editor

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("raw", [False, True])
def test_waveform_plot_preserves_long_frame_times_with_float32_amplitudes(
    monkeypatch: pytest.MonkeyPatch, *, raw: bool
) -> None:
    xs = np.array([2**24 + frame for frame in (1, 2, 3)], dtype=np.float64) / 48_000
    low = np.array([-0.5, -0.25, 0.0], dtype=np.float32)
    high = -low
    monkeypatch.setattr(waveform_editor, "implot_style_color", lambda *_args: nullcontext())
    monkeypatch.setattr(waveform_editor, "implot_style_var", lambda *_args: nullcontext())
    plotted = Mock()
    monkeypatch.setattr(implot, "plot_line" if raw else "plot_shaded", plotted)

    if raw:
        waveform_editor._plot_line(xs, low, show_sample_markers=False)
    else:
        waveform_editor._plot_shaded(xs, low, high)

    args = plotted.call_args.args
    assert args[1] is xs
    assert args[1].dtype == np.float64
    assert np.all(np.diff(args[1]) > 0)
    assert np.array_equal(args[1] * 48_000, np.array([2**24 + n for n in (1, 2, 3)]))
    assert all(amplitude.dtype == np.float64 for amplitude in args[2:])
    assert np.array_equal(args[2], low)
    if not raw:
        assert np.array_equal(args[3], high)
    assert low.dtype == high.dtype == np.float32


def test_waveform_request_and_cache_distinguish_adjacent_long_frames(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    ctx = UiContext(controller)
    controller.project.sample_paths[0] = "samples/long.wav"
    controller.project.sample_durations[0] = 100_000.0
    start_s = (2**24 + 1) / 48_000
    next_s = (2**24 + 2) / 48_000
    end_s = (2**24 + 17) / 48_000
    first = object()
    second = object()
    audio_engine_mock.get_waveform_render_data.side_effect = [first, second]

    assert ctx.ui.waveform.get_render_data(0, 320, start_s, end_s) is first
    assert ctx.ui.waveform.get_render_data(0, 320, start_s, end_s) is first
    assert ctx.ui.waveform.get_render_data(0, 320, next_s, end_s) is second

    calls = audio_engine_mock.get_waveform_render_data.call_args_list
    assert len(calls) == 2
    assert calls[0].args == (0, 320, start_s, end_s)
    assert calls[1].args == (0, 320, next_s, end_s)
