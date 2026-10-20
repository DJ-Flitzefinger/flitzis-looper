"""Render performer warnings outside the selected-pad and settings surfaces."""

from pathlib import Path, PureWindowsPath
from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.constants import NUM_BANKS, NUM_SAMPLES
from flitzis_looper.controller.performance_confirmation import (
    BankConfirmation,
    StemBatchConfirmation,
    StemDeleteConfirmation,
)
from flitzis_looper.models import STEM_SEPARATOR_LABELS

if TYPE_CHECKING:
    from flitzis_looper.controller.performance_confirmation import (
        PendingConfirmation,
        PerformanceConfirmation,
    )
    from flitzis_looper.ui.context import UiContext

_POPUP = "Confirm Audio Action##performance-confirmation"


def _render_audio_warning(intent: PerformanceConfirmation) -> str:
    label = PureWindowsPath(intent.path).name if "\\" in intent.path else Path(intent.path).name
    imgui.text_wrapped(f"Pad {intent.pad_id + 1}: {label}")
    imgui.text_wrapped(intent.path)
    imgui.separator()
    if intent.action == "unload":
        imgui.text_wrapped(
            "Unload this audio? Playback will stop and this pad's audio, loop, and analysis "
            "settings will be cleared."
        )
        return "UNLOAD AUDIO"
    imgui.text_wrapped(
        "Analyze this audio again? Its detected BPM, key, and beat grid will be replaced "
        "when analysis completes. The previous key correction is cleared when admitted. "
        "Manual BPM, grid offset, and loop settings are preserved."
    )
    return "ANALYZE AUDIO"


def _render_bank_warning(intent: BankConfirmation) -> str:
    target = intent.target_bank + 1
    if intent.action == "copy_bank":
        imgui.text_wrapped(f"Overwrite Bank {target}?")
        imgui.separator()
        assert intent.source_bank is not None
        imgui.text_wrapped(
            f"Replace every slot in Bank {target} with Bank {intent.source_bank + 1}, "
            "including empty slots. The source bank stays selected and copied playback is stopped."
        )
        return "OVERWRITE"
    imgui.text_wrapped(f"Clear Bank {target}?")
    imgui.separator()
    imgui.text_wrapped(f"Clear every slot in Bank {target}. Its removed playback will stop.")
    return "CLEAR BANK"


def _render_stem_batch_warning(intent: StemBatchConfirmation) -> str:
    inventory = intent.inventory
    occupied = sum(item is not None for item in inventory.items)
    separator, shifts, overlap = inventory.processing_signature
    imgui.text_wrapped(f"Generate stems for {occupied} loaded PADs?")
    imgui.separator()
    imgui.text_wrapped(
        f"Captured scope: all {NUM_BANKS} banks ({NUM_SAMPLES} slots). "
        "Changed or unavailable originals are Skipped; newly loaded content is excluded."
    )
    imgui.text_wrapped(f"Requested separator / model: {STEM_SEPARATOR_LABELS[separator]}")
    if separator == "demucs:htdemucs" and shifts is not None and overlap is not None:
        imgui.text_wrapped(f"Requested Demucs quality: shifts {shifts}, overlap {overlap:.2f}.")
    imgui.text_wrapped(
        "Requested settings only. Actual producer identity and sharing are verified during "
        "background work. Partial results are retained. Saving stems does not confirm "
        "a PAD's playback readiness."
    )
    return "GENERATE STEMS"


def _render_warning(intent: PendingConfirmation) -> str:
    if isinstance(intent, StemDeleteConfirmation):
        return _render_stem_delete_warning(intent)
    if isinstance(intent, StemBatchConfirmation):
        return _render_stem_batch_warning(intent)
    if isinstance(intent, BankConfirmation):
        return _render_bank_warning(intent)
    return _render_audio_warning(intent)


def _render_stem_delete_warning(intent: StemDeleteConfirmation) -> str:
    inventory = intent.inventory
    occupied = sum(item is not None for item in inventory.items)
    versions = len(inventory.set_identities)
    imgui.text_wrapped(f"Delete stems for {occupied} loaded PADs?")
    imgui.separator()
    imgui.text_wrapped(
        f"Captured scope: all {NUM_BANKS} banks ({NUM_SAMPLES} slots), "
        f"{versions} unique stem set versions. Changed originals are Skipped; "
        "newly loaded content is excluded."
    )
    imgui.text_wrapped(
        "Matching PADs return safely to FullMix and their pending stem work is withdrawn. "
        "Original audio and FullMix sources are kept."
    )
    imgui.text_wrapped(
        "Files remain while playback or other readers still need them. "
        "Logical removal and later file cleanup are reported separately."
    )
    return "DELETE STEMS"


def _render_warning_body(ctx: UiContext, intent: PendingConfirmation) -> None:
    accept_label = _render_warning(intent)
    valid = ctx.ui.confirmation.is_current(intent)
    if not valid:
        imgui.text_wrapped("This content changed or is no longer eligible. Cancel and try again.")
    imgui.spacing()
    control_height = imgui.get_frame_height()
    imgui.begin_disabled(not valid)
    accepted = imgui.button(accept_label, (-1, control_height))
    imgui.end_disabled()
    if accepted:
        ctx.ui.confirmation.accept(intent)
        if ctx.ui.confirmation.pending is None:
            imgui.close_current_popup()
    elif imgui.button("CANCEL", (-1, control_height)):
        ctx.ui.confirmation.dismiss(intent)
        imgui.close_current_popup()


def performance_confirmation(ctx: UiContext) -> None:
    """Display the common captured audio, bank or stem warning with explicit controls."""
    intent = ctx.ui.confirmation.pending
    if intent is None:
        return
    if ctx.ui.confirmation.take_open_request(intent):
        imgui.open_popup(_POPUP)

    viewport = imgui.get_main_viewport()
    imgui.set_next_window_pos(viewport.get_center(), imgui.Cond_.appearing, (0.5, 0.5))
    font_size = imgui.get_font_size()
    width = min(34 * font_size, max(1, viewport.work_size.x - 2 * font_size))
    imgui.set_next_window_size((width, 0), imgui.Cond_.always)
    visible, opened = imgui.begin_popup_modal(
        _POPUP, p_open=True, flags=imgui.WindowFlags_.always_auto_resize
    )
    if not opened or not visible:
        ctx.ui.confirmation.dismiss(intent)
        if visible:
            imgui.end_popup()
        return
    try:
        if imgui.is_key_pressed(imgui.Key.escape, repeat=False):
            ctx.ui.confirmation.dismiss(intent)
            imgui.close_current_popup()
            return
        _render_warning_body(ctx, intent)
    finally:
        imgui.end_popup()
