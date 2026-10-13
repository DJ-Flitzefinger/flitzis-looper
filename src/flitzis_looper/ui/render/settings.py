from typing import TYPE_CHECKING

from imgui_bundle import icons_fontawesome_6, imgui

from flitzis_looper.constants import (
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
)
from flitzis_looper.models import (
    STEM_SEPARATOR_LABELS,
    STEM_SEPARATORS,
    TRIGGER_QUANTIZATION_STEP_LABELS,
    TRIGGER_QUANTIZATION_STEPS,
)
from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA
from flitzis_looper.ui.contextmanager import button_style, item_width, style_var

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext

SETTINGS_TOGGLE_BUTTON_SIZE = 36.0


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
        imgui.text_colored(TEXT_MUTED_RGBA, "Stem Generation")
        _stem_generation_controls(ctx)
        imgui.separator()
        imgui.text_colored(TEXT_MUTED_RGBA, "Resident Audio RAM")
        _resident_pcm_controls(ctx)


def _resident_pcm_controls(ctx: UiContext) -> None:
    enabled = bool(ctx.state.project.preload_stem_loops_on_startup)
    changed, value = imgui.checkbox("Preload existing stem loops at startup", enabled)
    if changed:
        ctx.ui.settings.set_preload_stem_loops_on_startup(enabled=bool(value))
        ctx.persistence.flush_if_dirty()
    imgui.text_wrapped(
        "Preload requests existing component loops on all eligible occupied pads. "
        "It adds RAM use, preserves each pad's audio mode, and does not generate stems "
        "or guarantee admission. With preload off, saved ALL STEMS waits for an explicit request."
    )
    budget = int(ctx.state.project.resident_pcm_budget_mib)
    with item_width(240):
        changed, budget_value = imgui.slider_int("Resident PCM budget (MiB)", budget, 128, 16384)
    if changed:
        ctx.ui.settings.set_resident_pcm_budget_mib(int(budget_value))
    _flush_settings_edit_if_completed(ctx)
    if ctx.state.session.resident_pcm_settings_error:
        imgui.text_wrapped(str(ctx.state.session.resident_pcm_settings_error))
    summary = ctx.state.stems.residency_summary()
    imgui.text_wrapped(
        f"Conservative all-pad FullMix + eligible components: "
        f"{summary.estimated_pcm_bytes / 1048576:.1f} MiB; "
        f"{summary.unknown_shapes} unknown shapes; "
        f"{summary.full_track_exceptions} complete-track fallbacks. "
        "Sharing can reduce PCM; context, overlap, Native state and process overhead add RAM."
    )
    imgui.text_wrapped(
        f"Stem component requests {summary.requested}: queued {summary.queued}, "
        f"preparing {summary.preparing}, ACKed {summary.acked}, "
        f"deferred {summary.deferred}, errors {summary.errors}, "
        f"unconfirmed in the current context {summary.unconfirmed}."
    )
    if summary.accounting_error:
        imgui.text_wrapped(f"Resident accounting unavailable: {summary.accounting_error}")
        return
    native = summary.native
    imgui.text_wrapped(
        f"Exact unique resident PCM {native.get('held_unique_pcm_bytes', 0)} B; "
        f"admitted pending PCM {native.get('pending_resident_bytes', 0)} B "
        f"({native.get('pending_reservation_count', 0)} reservations); "
        f"total {native.get('admitted_total_bytes', 0)} B / "
        f"budget {native.get('budget_bytes', 0)} B."
    )
    if native.get("over_budget"):
        imgui.text_wrapped(
            "Pinned readers temporarily exceed the allowance. Current audio stays valid; "
            "new allocations wait for safe retirement or a larger budget."
        )
    imgui.text_wrapped(
        "Preparing requests can still have unknown geometry and no admitted PCM reservation. "
        "PCM counters are not process RSS. Timing/analysis and per-job preparation have "
        "separate limits; Native/DSP, inference and other process overhead are not included. "
        "A larger finite allowance does not guarantee available host RAM."
    )


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


def _stem_generation_controls(ctx: UiContext) -> None:
    separator = ctx.state.project.stem_separator
    with item_width(240):
        if imgui.begin_combo("Separator / Model", STEM_SEPARATOR_LABELS[separator]):
            for option in STEM_SEPARATORS:
                selected = option == separator
                label = STEM_SEPARATOR_LABELS[option]
                if imgui.selectable(f"{label}##stem_separator_{option}", selected)[0]:
                    ctx.ui.settings.set_stem_separator(option)
                    ctx.persistence.flush_if_dirty()
                if selected:
                    imgui.set_item_default_focus()
            imgui.end_combo()
    imgui.text_colored(TEXT_MUTED_RGBA, "Applies to new stem generations.")
    if ctx.state.project.stem_separator == "demucs:htdemucs":
        _demucs_quality_controls(ctx)


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
