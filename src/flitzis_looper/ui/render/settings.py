from typing import TYPE_CHECKING

from imgui_bundle import icons_fontawesome_6, imgui

from flitzis_looper.constants import (
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MAX_KEY_LOCK_DELAY_MIN_SAMPLES,
    MAX_KEY_LOCK_DELAY_RANGE_SAMPLES,
    MAX_KEY_LOCK_DELAY_TOTAL_SAMPLES,
    MAX_KEY_LOCK_HEAD_COUNT,
    MAX_KEY_LOCK_OUTPUT_GAIN,
    MAX_KEY_LOCK_SMOOTHING_STEP,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
    MIN_KEY_LOCK_DELAY_MIN_SAMPLES,
    MIN_KEY_LOCK_DELAY_RANGE_SAMPLES,
    MIN_KEY_LOCK_HEAD_COUNT,
    MIN_KEY_LOCK_OUTPUT_GAIN,
    MIN_KEY_LOCK_SMOOTHING_STEP,
)
from flitzis_looper.models import (
    KEY_LOCK_INTERPOLATION_LABELS,
    KEY_LOCK_INTERPOLATIONS,
    KEY_LOCK_WINDOW_LABELS,
    KEY_LOCK_WINDOWS,
    TRIGGER_QUANTIZATION_STEP_LABELS,
    TRIGGER_QUANTIZATION_STEPS,
    KeyLockInterpolation,
    KeyLockWindow,
)
from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA
from flitzis_looper.ui.contextmanager import button_style, item_width, style_colors, style_var

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext

SETTINGS_TOGGLE_BUTTON_SIZE = 36.0
KEY_LOCK_PARAMETER_HINTS: dict[str, str] = {
    "delay_min": (
        "Higher: more latency/headroom and steadier bass. Lower: lower latency, more artifacts."
    ),
    "delay_range": (
        "Higher: smoother wide pitch moves, more smear. Lower: tighter transients, more warble."
    ),
    "heads": "Higher: more CPU, smoother level. Lower: less CPU, more level movement.",
    "interpolation": (
        "Cubic: more CPU and smoother pitch. Linear: less CPU and rougher transients."
    ),
    "window": "Hann: smoother blend. Triangle: cheaper/tighter, more level movement risk.",
    "smoothing": (
        "Higher: faster Pitch response, more artifact risk. Lower: steadier sound, slower response."
    ),
    "output_gain": (
        "Higher: louder with clipping risk. Lower: more headroom, quieter Key Lock output."
    ),
}


def settings_surface_child_id(*, settings_open: bool) -> str:
    """Return the main surface child ID for tests and rendering."""
    return "settings_overlay" if settings_open else "looper_main"


def settings_toggle_button_label(*, settings_open: bool) -> str:
    """Return the icon label for the bottom-right Settings toggle."""
    icon = icons_fontawesome_6.ICON_FA_XMARK if settings_open else icons_fontawesome_6.ICON_FA_GEAR
    return f"{icon}##settings_toggle"


def settings_toggle_tooltip(*, settings_open: bool) -> str:
    """Return the tooltip for the Settings toggle."""
    return "Close settings" if settings_open else "Open settings"


def settings_toggle_button(ctx: UiContext) -> None:
    """Render the Settings toggle at the caller-provided cursor position."""
    with button_style("regular"):
        if imgui.button(
            settings_toggle_button_label(settings_open=ctx.state.session.settings_open),
            (SETTINGS_TOGGLE_BUTTON_SIZE, SETTINGS_TOGGLE_BUTTON_SIZE),
        ):
            ctx.ui.settings.toggle()

    if imgui.is_item_hovered():
        imgui.set_tooltip(settings_toggle_tooltip(settings_open=ctx.state.session.settings_open))


def clamp_key_lock_smoothing_step_for_settings(value: float) -> float:
    """Clamp Settings-page smoothing values to the controller-supported range."""
    return min(max(float(value), MIN_KEY_LOCK_SMOOTHING_STEP), MAX_KEY_LOCK_SMOOTHING_STEP)


def settings_overlay(ctx: UiContext) -> None:
    """Render the Settings page in place of the main Looper surface."""
    with style_var(imgui.StyleVar_.item_spacing, (SPACING, SPACING)):
        imgui.text_unformatted("Settings")
        imgui.separator()
        imgui.text_colored(TEXT_MUTED_RGBA, "Input Mapping")
        _input_mapping_controls(ctx)
        imgui.separator()
        imgui.text_colored(TEXT_MUTED_RGBA, "Trigger Quantize")
        _trigger_quantization_controls(ctx)
        imgui.separator()
        imgui.text_colored(TEXT_MUTED_RGBA, "Key Lock DSP")
        _key_lock_parameter_controls(ctx)
        imgui.separator()
        imgui.text_colored(TEXT_MUTED_RGBA, "Stem Quality")
        _demucs_quality_controls(ctx)


def _input_mapping_controls(ctx: UiContext) -> None:
    enabled = bool(ctx.state.project.input_mapping_enabled)
    changed, new_enabled = imgui.checkbox("Input Mapping", enabled)
    if changed:
        ctx.ui.settings.set_input_mapping_enabled(enabled=bool(new_enabled))
        ctx.persistence.flush_if_dirty()

    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
        if imgui.button("Delete all Keyboard Mappings", (-1, 0)):
            ctx.ui.settings.delete_all_keyboard_mappings()
        if imgui.button("Delete all MIDI Mappings", (-1, 0)):
            ctx.ui.settings.delete_all_midi_mappings()

    if ctx.state.session.input_mapping_error:
        imgui.text_wrapped(ctx.state.session.input_mapping_error)


def _trigger_quantization_controls(ctx: UiContext) -> None:
    step = ctx.state.project.trigger_quantization_step
    preview = TRIGGER_QUANTIZATION_STEP_LABELS[step]

    with item_width(240):
        if imgui.begin_combo("Quantize Grid", preview):
            for option in TRIGGER_QUANTIZATION_STEPS:
                selected = option == step
                label = TRIGGER_QUANTIZATION_STEP_LABELS[option]
                if imgui.selectable(f"{label}##trigger_quantization_step_{option}", selected)[0]:
                    ctx.ui.settings.set_trigger_quantization_step(option)
                    ctx.persistence.flush_if_dirty()
                if selected:
                    imgui.set_item_default_focus()
            imgui.end_combo()


def _key_lock_parameter_controls(ctx: UiContext) -> None:
    delay_min = float(ctx.state.project.key_lock_delay_min_samples)
    delay_range = float(ctx.state.project.key_lock_delay_range_samples)
    heads = int(ctx.state.project.key_lock_head_count)
    interpolation = ctx.state.project.key_lock_interpolation
    window = ctx.state.project.key_lock_window
    smoothing = clamp_key_lock_smoothing_step_for_settings(
        ctx.state.project.key_lock_smoothing_step
    )
    output_gain = float(ctx.state.project.key_lock_output_gain)

    with item_width(240):
        changed, new_delay_min = imgui.slider_float(
            "Delay min (samples)",
            delay_min,
            MIN_KEY_LOCK_DELAY_MIN_SAMPLES,
            MAX_KEY_LOCK_DELAY_MIN_SAMPLES,
            "%.0f",
        )
    if changed:
        delay_min = float(new_delay_min)
        delay_range = min(delay_range, MAX_KEY_LOCK_DELAY_TOTAL_SAMPLES - delay_min)
        _set_key_lock_parameters(
            ctx, delay_min, delay_range, heads, interpolation, window, smoothing, output_gain
        )
    _parameter_hint(KEY_LOCK_PARAMETER_HINTS["delay_min"])
    _flush_settings_edit_if_completed(ctx)

    delay_range_max = min(
        MAX_KEY_LOCK_DELAY_RANGE_SAMPLES,
        MAX_KEY_LOCK_DELAY_TOTAL_SAMPLES - delay_min,
    )
    delay_range_max = max(MIN_KEY_LOCK_DELAY_RANGE_SAMPLES, delay_range_max)
    delay_range = min(delay_range, delay_range_max)
    with item_width(240):
        changed, new_delay_range = imgui.slider_float(
            "Delay range (samples)",
            delay_range,
            MIN_KEY_LOCK_DELAY_RANGE_SAMPLES,
            delay_range_max,
            "%.0f",
        )
    if changed:
        delay_range = float(new_delay_range)
        _set_key_lock_parameters(
            ctx, delay_min, delay_range, heads, interpolation, window, smoothing, output_gain
        )
    _parameter_hint(KEY_LOCK_PARAMETER_HINTS["delay_range"])
    _flush_settings_edit_if_completed(ctx)

    with item_width(240):
        changed, new_heads = imgui.slider_int(
            "Heads",
            heads,
            MIN_KEY_LOCK_HEAD_COUNT,
            MAX_KEY_LOCK_HEAD_COUNT,
        )
    if changed:
        heads = int(new_heads)
        _set_key_lock_parameters(
            ctx, delay_min, delay_range, heads, interpolation, window, smoothing, output_gain
        )
    _parameter_hint(KEY_LOCK_PARAMETER_HINTS["heads"])
    _flush_settings_edit_if_completed(ctx)

    _key_lock_interpolation_combo(
        ctx, delay_min, delay_range, heads, interpolation, window, smoothing, output_gain
    )
    _parameter_hint(KEY_LOCK_PARAMETER_HINTS["interpolation"])

    _key_lock_window_combo(
        ctx, delay_min, delay_range, heads, interpolation, window, smoothing, output_gain
    )
    _parameter_hint(KEY_LOCK_PARAMETER_HINTS["window"])

    with item_width(240):
        changed, new_smoothing = imgui.slider_float(
            "Smoothing step",
            smoothing,
            MIN_KEY_LOCK_SMOOTHING_STEP,
            MAX_KEY_LOCK_SMOOTHING_STEP,
            "%.3f",
        )
    if changed:
        smoothing = clamp_key_lock_smoothing_step_for_settings(new_smoothing)
        _set_key_lock_parameters(
            ctx, delay_min, delay_range, heads, interpolation, window, smoothing, output_gain
        )
    _parameter_hint(KEY_LOCK_PARAMETER_HINTS["smoothing"])
    _flush_settings_edit_if_completed(ctx)

    with item_width(240):
        changed, new_output_gain = imgui.slider_float(
            "Output gain",
            output_gain,
            MIN_KEY_LOCK_OUTPUT_GAIN,
            MAX_KEY_LOCK_OUTPUT_GAIN,
            "%.2f",
        )
    if changed:
        output_gain = float(new_output_gain)
        _set_key_lock_parameters(
            ctx, delay_min, delay_range, heads, interpolation, window, smoothing, output_gain
        )
    _parameter_hint(KEY_LOCK_PARAMETER_HINTS["output_gain"])
    _flush_settings_edit_if_completed(ctx)


def _key_lock_interpolation_combo(
    ctx: UiContext,
    delay_min: float,
    delay_range: float,
    heads: int,
    interpolation: KeyLockInterpolation,
    window: KeyLockWindow,
    smoothing: float,
    output_gain: float,
) -> None:
    preview = KEY_LOCK_INTERPOLATION_LABELS[interpolation]
    with item_width(240):
        if imgui.begin_combo("Interpolation", preview):
            for option in KEY_LOCK_INTERPOLATIONS:
                selected = option == interpolation
                label = KEY_LOCK_INTERPOLATION_LABELS[option]
                if imgui.selectable(f"{label}##key_lock_interpolation_{option}", selected)[0]:
                    _set_key_lock_parameters(
                        ctx, delay_min, delay_range, heads, option, window, smoothing, output_gain
                    )
                    ctx.persistence.flush_if_dirty()
                if selected:
                    imgui.set_item_default_focus()
            imgui.end_combo()


def _key_lock_window_combo(
    ctx: UiContext,
    delay_min: float,
    delay_range: float,
    heads: int,
    interpolation: KeyLockInterpolation,
    window: KeyLockWindow,
    smoothing: float,
    output_gain: float,
) -> None:
    preview = KEY_LOCK_WINDOW_LABELS[window]
    with item_width(240):
        if imgui.begin_combo("Window", preview):
            for option in KEY_LOCK_WINDOWS:
                selected = option == window
                label = KEY_LOCK_WINDOW_LABELS[option]
                if imgui.selectable(f"{label}##key_lock_window_{option}", selected)[0]:
                    _set_key_lock_parameters(
                        ctx,
                        delay_min,
                        delay_range,
                        heads,
                        interpolation,
                        option,
                        smoothing,
                        output_gain,
                    )
                    ctx.persistence.flush_if_dirty()
                if selected:
                    imgui.set_item_default_focus()
            imgui.end_combo()


def _set_key_lock_parameters(
    ctx: UiContext,
    delay_min: float,
    delay_range: float,
    heads: int,
    interpolation: KeyLockInterpolation,
    window: KeyLockWindow,
    smoothing: float,
    output_gain: float,
) -> None:
    ctx.ui.settings.set_key_lock_parameters(
        delay_min_samples=delay_min,
        delay_range_samples=delay_range,
        head_count=heads,
        interpolation=interpolation,
        window=window,
        smoothing_step=clamp_key_lock_smoothing_step_for_settings(smoothing),
        output_gain=output_gain,
    )


def _parameter_hint(text: str) -> None:
    with style_colors(((imgui.Col_.text, TEXT_MUTED_RGBA),)):
        imgui.text_wrapped(text)


def _demucs_quality_controls(ctx: UiContext) -> None:
    shifts = int(ctx.state.project.demucs_shifts)
    overlap = float(ctx.state.project.demucs_overlap)

    with item_width(240):
        changed, new_shifts = imgui.slider_int(
            "Demucs shifts",
            shifts,
            MIN_DEMUCS_SHIFTS,
            MAX_DEMUCS_SHIFTS,
        )
    if changed:
        shifts = int(new_shifts)
        ctx.ui.settings.set_demucs_quality(shifts=shifts, overlap=overlap)
    _flush_settings_edit_if_completed(ctx)

    with item_width(240):
        changed, new_overlap = imgui.slider_float(
            "Demucs overlap",
            overlap,
            MIN_DEMUCS_OVERLAP,
            MAX_DEMUCS_OVERLAP,
            "%.2f",
        )
    if changed:
        ctx.ui.settings.set_demucs_quality(shifts=shifts, overlap=float(new_overlap))
    _flush_settings_edit_if_completed(ctx)


def _flush_settings_edit_if_completed(ctx: UiContext) -> None:
    if imgui.is_item_deactivated_after_edit():
        ctx.persistence.flush_if_dirty()
