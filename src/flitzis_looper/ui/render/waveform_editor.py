import math
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

import numpy as np
from imgui_bundle import imgui, imgui_ctx, implot

from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.ui.context import UiContext


@dataclass(slots=True)
class _WaveformCache:
    path: str
    sample_rate_hz: int
    frames: int
    duration_s: float
    samples: np.ndarray
    env_x: np.ndarray
    env_min: np.ndarray
    env_max: np.ndarray


_WAVEFORM_CACHE: dict[str, _WaveformCache] = {}


def _iter_riff_chunks(data: bytes) -> Iterator[tuple[bytes, bytes]]:
    offset = 12
    while offset + 8 <= len(data):
        chunk_id = data[offset : offset + 4]
        chunk_size = int.from_bytes(data[offset + 4 : offset + 8], "little", signed=False)
        chunk_start = offset + 8
        chunk_end = chunk_start + chunk_size
        if chunk_end > len(data):
            break

        yield (chunk_id, data[chunk_start:chunk_end])

        # chunks are word-aligned
        offset = chunk_end + (chunk_size % 2)


def _read_f32_wav_mono(path: Path) -> tuple[int, np.ndarray]:
    """Read an IEEE float WAV and return mono samples."""
    data = path.read_bytes()
    if len(data) < 44 or data[0:4] != b"RIFF" or data[8:12] != b"WAVE":
        msg = "not a WAV file"
        raise ValueError(msg)

    fmt_chunk: bytes | None = None
    data_chunk: bytes | None = None

    for chunk_id, chunk in _iter_riff_chunks(data):
        if chunk_id == b"fmt ":
            fmt_chunk = chunk
        elif chunk_id == b"data":
            data_chunk = chunk

    if fmt_chunk is None or data_chunk is None or len(fmt_chunk) < 16:
        msg = "missing WAV chunks"
        raise ValueError(msg)

    audio_format = int.from_bytes(fmt_chunk[0:2], "little", signed=False)
    channels = int.from_bytes(fmt_chunk[2:4], "little", signed=False)
    sample_rate_hz = int.from_bytes(fmt_chunk[4:8], "little", signed=False)
    bits_per_sample = int.from_bytes(fmt_chunk[14:16], "little", signed=False)

    if audio_format != 3 or bits_per_sample != 32:
        msg = "unsupported WAV format"
        raise ValueError(msg)

    if channels <= 0:
        msg = "invalid WAV channel count"
        raise ValueError(msg)

    sample_count = len(data_chunk) // 4
    frames = sample_count // channels
    if frames <= 0:
        return (sample_rate_hz, np.array([], dtype=np.float32))

    sample_count = frames * channels
    raw = np.frombuffer(data_chunk[: sample_count * 4], dtype="<f4")

    if channels == 1:
        return (sample_rate_hz, raw.astype(np.float32, copy=False))

    mono = raw.reshape(frames, channels).mean(axis=1)
    return (sample_rate_hz, mono.astype(np.float32, copy=False))


def _build_envelope(
    samples: np.ndarray, sample_rate_hz: int, *, bucket: int
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    if sample_rate_hz <= 0 or samples.size == 0:
        empty = np.array([], dtype=np.float64)
        return (empty, empty, empty)

    env_frames = math.ceil(samples.size / bucket)
    env_x = np.empty(env_frames, dtype=np.float64)
    env_min = np.empty(env_frames, dtype=np.float32)
    env_max = np.empty(env_frames, dtype=np.float32)

    idx = 0
    for start in range(0, samples.size, bucket):
        chunk = samples[start : start + bucket]
        if chunk.size == 0:
            continue

        env_min[idx] = float(chunk.min())
        env_max[idx] = float(chunk.max())
        center = start + chunk.size * 0.5
        env_x[idx] = center / sample_rate_hz
        idx += 1

    return (env_x[:idx], env_min[:idx], env_max[:idx])


def _get_or_load_waveform(path: str) -> _WaveformCache | None:
    cached = _WAVEFORM_CACHE.get(path)
    if cached is not None:
        return cached

    try:
        sample_rate_hz, samples = _read_f32_wav_mono(Path(path))
    except FileNotFoundError:
        return None
    except OSError:
        return None
    except ValueError:
        return None

    env_x, env_min, env_max = _build_envelope(samples, sample_rate_hz, bucket=256)

    frames = int(samples.size)
    duration_s = 0.0 if sample_rate_hz <= 0 else frames / sample_rate_hz

    wf = _WaveformCache(
        path=path,
        sample_rate_hz=sample_rate_hz,
        frames=frames,
        duration_s=duration_s,
        samples=samples,
        env_x=env_x,
        env_min=env_min,
        env_max=env_max,
    )
    _WAVEFORM_CACHE[path] = wf
    return wf


def _clamp_view(*, xmin: float, xmax: float, duration_s: float) -> tuple[float, float]:
    xmin = float(xmin)
    xmax = float(xmax)
    if not math.isfinite(xmin) or not math.isfinite(xmax) or xmax <= xmin:
        return (0.0, duration_s)

    xmin = max(0.0, xmin)
    xmax = min(duration_s, xmax)
    if xmax <= xmin:
        return (0.0, duration_s)

    return (xmin, xmax)


def _render_controls(ctx: UiContext, pad_id: int) -> None:
    is_active = ctx.state.pads.is_active(pad_id)
    play_label = "Pause" if is_active else "Play"

    if imgui.button(play_label, (88, 0)):
        if is_active:
            ctx.audio.pads.stop_pad(pad_id)
        else:
            ctx.audio.pads.play_pad(pad_id)

    imgui.same_line(spacing=SPACING)
    if imgui.button("Reset", (88, 0)):
        ctx.audio.pads.reset_pad_loop_region(pad_id)

    imgui.same_line(spacing=SPACING)
    if imgui.button("Zoom In", (88, 0)):
        ctx.ui.waveform.zoom(zoom_in=True)

    imgui.same_line(spacing=SPACING)
    if imgui.button("Zoom Out", (88, 0)):
        ctx.ui.waveform.zoom(zoom_in=False)

    imgui.same_line(spacing=SPACING)
    if imgui.button("Pan Left", (88, 0)):
        ctx.ui.waveform.pan(left=True)

    imgui.same_line(spacing=SPACING)
    if imgui.button("Pan Right", (88, 0)):
        ctx.ui.waveform.pan(left=False)


def _render_loop_controls(ctx: UiContext, pad_id: int) -> None:
    auto_enabled = bool(ctx.state.project.pad_loop_auto[pad_id])

    imgui.same_line(spacing=SPACING)
    changed, new_auto = imgui.checkbox("Auto-loop", auto_enabled)
    if changed:
        ctx.audio.pads.set_pad_loop_auto(pad_id, enabled=bool(new_auto))

    bars = int(ctx.state.project.pad_loop_bars[pad_id])
    bpm = ctx.state.pads.effective_bpm(pad_id)

    imgui.same_line(spacing=SPACING)
    if bpm is None and bool(new_auto):
        imgui.text_colored(TEXT_MUTED_RGBA, f"Bars: {bars} (BPM unavailable)")
        return

    imgui.text_colored(TEXT_MUTED_RGBA, "Bars")
    imgui.same_line(spacing=SPACING / 2)
    imgui.text(str(bars))

    imgui.same_line(spacing=SPACING / 2)
    if imgui.button("-", (24, 0)):
        ctx.audio.pads.set_pad_loop_bars(pad_id, bars=max(1, bars - 1))

    imgui.same_line(spacing=SPACING / 2)
    if imgui.button("+", (24, 0)):
        ctx.audio.pads.set_pad_loop_bars(pad_id, bars=bars + 1)


def _plot_overlay_loop_region(
    loop_start_s: float,
    loop_end_s: float | None,
    playhead_s: float,
) -> None:
    y0 = np.array([-1.0, 1.0], dtype=np.float64)

    if loop_end_s is not None and loop_end_s > loop_start_s:
        xs = np.array([loop_start_s, loop_end_s], dtype=np.float64)
        y_top = np.array([1.0, 1.0], dtype=np.float64)
        y_bot = np.array([-1.0, -1.0], dtype=np.float64)

        push = getattr(implot, "push_style_color", None)
        pop = getattr(implot, "pop_style_color", None)
        fill = getattr(getattr(implot, "ImPlotCol_", None), "fill", None)

        if callable(push) and callable(pop) and fill is not None:
            push(fill.value, (1.0, 1.0, 0.3, 0.18))
            implot.plot_shaded("##loop_region", xs, y_top, y_bot)
            pop()
        else:
            implot.plot_shaded("##loop_region", xs, y_top, y_bot)

    xs = np.array([loop_start_s, loop_start_s], dtype=np.float64)
    implot.plot_line("##loop_start", xs, y0)

    if loop_end_s is not None:
        xs = np.array([loop_end_s, loop_end_s], dtype=np.float64)
        implot.plot_line("##loop_end", xs, y0)

    if math.isfinite(playhead_s):
        xs = np.array([playhead_s, playhead_s], dtype=np.float64)
        implot.plot_line("##playhead", xs, y0)


def _handle_plot_interactions(
    ctx: UiContext,
    pad_id: int,
    wf: _WaveformCache,
) -> None:
    hovered = getattr(implot, "is_plot_hovered", None)
    if not callable(hovered) or not hovered():
        return

    get_mouse_pos = getattr(implot, "get_plot_mouse_pos", None)
    if not callable(get_mouse_pos):
        return

    mouse_pos = get_mouse_pos()
    mouse_x = float(getattr(mouse_pos, "x", 0.0))
    mouse_x = min(max(mouse_x, 0.0), wf.duration_s)

    io = imgui.get_io()
    if io.mouse_wheel != 0.0:
        ctx.ui.waveform.zoom_wheel(float(io.mouse_wheel), mouse_x)

    if imgui.is_mouse_dragging(imgui.MouseButton_.middle, 0.0):
        delta = imgui.get_mouse_drag_delta(imgui.MouseButton_.middle, 0.0)
        dx = float(getattr(delta, "x", 0.0))
        plot_size = imgui.get_item_rect_size()
        width = float(getattr(plot_size, "x", 0.0))
        ctx.ui.waveform.pan_drag(dx, width)
        imgui.reset_mouse_drag_delta(imgui.MouseButton_.middle)

    auto_enabled = bool(ctx.state.project.pad_loop_auto[pad_id])
    if imgui.is_mouse_clicked(imgui.MouseButton_.left):
        ctx.audio.pads.set_pad_loop_start(pad_id, mouse_x)

    if not auto_enabled and imgui.is_mouse_clicked(imgui.MouseButton_.right):
        ctx.audio.pads.set_pad_loop_end(pad_id, mouse_x)


def _render_plot(ctx: UiContext, pad_id: int, wf: _WaveformCache) -> None:
    session = ctx.state.session

    xmin, xmax = _clamp_view(
        xmin=float(session.waveform_editor_view_xmin_s),
        xmax=float(session.waveform_editor_view_xmax_s),
        duration_s=wf.duration_s,
    )
    if (xmin, xmax) != (session.waveform_editor_view_xmin_s, session.waveform_editor_view_xmax_s):
        ctx.ui.waveform.set_view(xmin, xmax)

    if not implot.begin_plot("##waveform", (-1, 320)):
        return

    setup_axes_limits = getattr(implot, "setup_axes_limits", None)
    if callable(setup_axes_limits):
        setup_axes_limits(xmin, xmax, -1.0, 1.0, imgui.Cond_.always)

    loop_start_s, loop_end_s = ctx.state.pads.effective_loop_region(pad_id)
    playhead_s = float(session.pad_playhead_s[pad_id])

    view_samples = int((xmax - xmin) * wf.sample_rate_hz)
    extreme_zoom = 0 < view_samples <= 4000

    if extreme_zoom:
        start = max(0, int(xmin * wf.sample_rate_hz))
        end = min(wf.frames, max(start + 1, int(xmax * wf.sample_rate_hz)))
        xs = np.arange(start, end, dtype=np.float64) / wf.sample_rate_hz
        ys = wf.samples[start:end]
        implot.plot_line("wave", xs, ys)
    else:
        lo = int(np.searchsorted(wf.env_x, xmin, side="left"))
        hi = int(np.searchsorted(wf.env_x, xmax, side="right"))
        xs = wf.env_x[lo:hi]
        ys_min = wf.env_min[lo:hi]
        ys_max = wf.env_max[lo:hi]
        implot.plot_line("min", xs, ys_min)
        implot.plot_line("max", xs, ys_max)

    _plot_overlay_loop_region(loop_start_s, loop_end_s, playhead_s)
    _handle_plot_interactions(ctx, pad_id, wf)

    implot.end_plot()


def waveform_editor(ctx: UiContext) -> None:
    session = ctx.state.session
    pad_id = session.waveform_editor_pad_id

    if not session.waveform_editor_open or pad_id is None:
        return

    if not ctx.state.pads.is_loaded(pad_id):
        return

    cached_path = ctx.state.project.sample_paths[pad_id]
    if cached_path is None or "\\" in cached_path:
        return

    wf = _get_or_load_waveform(cached_path)
    if wf is None or wf.sample_rate_hz <= 0:
        return

    with imgui_ctx.begin(
        "Waveform Editor", p_open=True, flags=imgui.WindowFlags_.no_collapse
    ) as window:
        if window:
            _render_controls(ctx, pad_id)
            _render_loop_controls(ctx, pad_id)
            _render_plot(ctx, pad_id, wf)

        if not window.opened:
            ctx.ui.waveform.close()
