from typing import TYPE_CHECKING

from imgui_bundle import icons_fontawesome_6, imgui

from flitzis_looper.constants import (
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
)
from flitzis_looper.models import (
    KEY_LOCK_START_BEHAVIOR_LABELS,
    KEY_LOCK_START_BEHAVIORS,
    STEM_SEPARATOR_LABELS,
    STEM_SEPARATORS,
    TRIGGER_QUANTIZATION_STEP_LABELS,
    TRIGGER_QUANTIZATION_STEPS,
)
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA
from flitzis_looper.ui.contextmanager import button_style, item_width, style_var

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_batch import StemBatchSummary
    from flitzis_looper.controller.stem_delete import StemDeleteSummary
    from flitzis_looper.ui.context import StemCleanupStatus, UiContext

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
        _key_lock_start_behavior_controls(ctx)
        imgui.separator()
        imgui.text_colored(TEXT_MUTED_RGBA, "Recording")
        _recording_controls(ctx)
        imgui.separator()
        imgui.text_colored(TEXT_MUTED_RGBA, "Stem Generation")
        _stem_generation_controls(ctx)
        _stem_batch_controls(ctx)
        _stem_delete_controls(ctx)
        imgui.separator()
        imgui.text_colored(TEXT_MUTED_RGBA, "Resident Audio RAM")
        _resident_pcm_controls(ctx)


def _recording_choice(
    ctx: UiContext,
    label: str,
    field: str,
    current: str | int,
    choices: tuple[tuple[str | int, str], ...],
) -> None:
    labels = dict(choices)
    with item_width(240):
        if not imgui.begin_combo(label, labels[current]):
            return
        for value, text in choices:
            selected = value == current
            if imgui.selectable(f"{text}##recording_{field}_{value}", selected)[0]:
                options = RecordingOptions.model_validate(
                    ctx.state.project.recording_options.model_dump() | {field: value}
                )
                ctx.ui.settings.set_recording_options(options)
                ctx.persistence.flush_if_dirty()
            if selected:
                imgui.set_item_default_focus()
        imgui.end_combo()


def _recording_controls(ctx: UiContext) -> None:
    """Edit future take defaults without touching an audio or preparation owner."""
    options = ctx.state.project.recording_options
    _recording_choice(
        ctx,
        "Recording format",
        "format",
        options.format,
        (("wav", "WAV"), ("flac", "FLAC"), ("mp3", "MP3")),
    )
    options = ctx.state.project.recording_options
    if options.format == "wav":
        _recording_choice(
            ctx,
            "WAV precision",
            "wav_encoding",
            options.wav_encoding,
            (("float32", "32-bit float"), ("pcm24", "24-bit PCM"), ("pcm16", "16-bit PCM")),
        )
    elif options.format == "flac":
        _recording_choice(
            ctx,
            "FLAC precision",
            "flac_bit_depth",
            options.flac_bit_depth,
            ((24, "24-bit"), (16, "16-bit")),
        )
        options = ctx.state.project.recording_options
        with item_width(240):
            changed, compression = imgui.slider_int(
                "FLAC compression", options.flac_compression, 0, 8
            )
        if changed:
            ctx.ui.settings.set_recording_options(
                RecordingOptions.model_validate(
                    options.model_dump() | {"flac_compression": compression}
                )
            )
        _flush_settings_edit_if_completed(ctx)
        imgui.text_wrapped(
            "Compression changes file size and encoding time. Audio remains lossless."
        )
    else:
        _recording_choice(
            ctx,
            "MP3 bitrate",
            "mp3_bitrate_kbps",
            options.mp3_bitrate_kbps,
            (
                (320, "320 kbit/s CBR"),
                (256, "256 kbit/s CBR"),
                (192, "192 kbit/s CBR"),
                (128, "128 kbit/s CBR"),
            ),
        )
    imgui.text_wrapped(
        "These defaults apply to future takes. A prepared take keeps its selected settings."
    )
    if ctx.persistence.recording_settings_warning:
        imgui.text_wrapped(ctx.persistence.recording_settings_warning)


def _key_lock_start_behavior_controls(ctx: UiContext) -> None:
    behavior = ctx.state.project.key_lock_start_behavior
    can_change = ctx.ui.settings.can_change_key_lock_start_behavior()
    imgui.begin_disabled(not can_change)
    try:
        with item_width(240):
            if imgui.begin_combo(
                "Key Lock start behavior", KEY_LOCK_START_BEHAVIOR_LABELS[behavior]
            ):
                for option in KEY_LOCK_START_BEHAVIORS:
                    selected = option == behavior
                    label = KEY_LOCK_START_BEHAVIOR_LABELS[option]
                    if imgui.selectable(f"{label}##key_lock_start_behavior_{option}", selected)[
                        0
                    ] and ctx.ui.settings.set_key_lock_start_behavior(option):
                        ctx.persistence.flush_if_dirty()
                    if selected:
                        imgui.set_item_default_focus()
                imgui.end_combo()
    finally:
        imgui.end_disabled()
    if ctx.state.project.key_lock_start_behavior == "attack_at_grid":
        imgui.text_wrapped(
            "The main attack aligns with the grid. "
            "Any required quiet lead-in may be audible before it."
        )
    else:
        imgui.text_wrapped(
            "Playback stays silent before the grid. If the sound has a quiet lead-in, "
            "the main attack follows correspondingly later."
        )
    if not can_change:
        imgui.text_wrapped("Stop playback to change")


def _resident_pcm_budget_controls(ctx: UiContext) -> None:
    automatic = ctx.state.project.resident_pcm_budget_automatic
    changed, value = imgui.checkbox("Automatic memory admission", automatic)
    if changed and ctx.ui.settings.set_resident_pcm_budget_automatic(enabled=bool(value)):
        ctx.persistence.flush_if_dirty()
    automatic = ctx.state.project.resident_pcm_budget_automatic
    budget = int(ctx.state.project.resident_pcm_budget_mib)
    choice_gib = min(16, max(1, round(budget / 1024)))
    imgui.begin_disabled(automatic)
    try:
        with item_width(240):
            changed, budget_value = imgui.slider_int("Choose manual limit (GiB)", choice_gib, 1, 16)
        if changed and not automatic:
            ctx.ui.settings.set_resident_pcm_budget_mib(int(budget_value) * 1024)
        _flush_settings_edit_if_completed(ctx)
    finally:
        imgui.end_disabled()
    budget = int(ctx.state.project.resident_pcm_budget_mib)
    imgui.text_wrapped(f"Retained manual limit: {budget / 1024:.2f} GiB ({budget} MiB).")
    imgui.text_wrapped(
        "Automatic admission uses available host memory without a fixed PCM limit. "
        "Manual mode adds the retained limit above. Both modes check operating-system "
        "physical and commit headroom before new allocations and keep current readers valid."
    )
    if ctx.state.session.resident_pcm_settings_error:
        imgui.text_wrapped(ctx.state.session.resident_pcm_settings_error)


def _resident_host_memory_status(native: dict[str, int | bool | str | None]) -> None:
    physical = native.get("host_available_physical_bytes")
    commit = native.get("host_available_commit_bytes")
    allowance = native.get("host_allocation_allowance_bytes")
    if native.get("host_memory_error"):
        imgui.text_wrapped("Host memory monitoring unavailable; new allocations wait.")
    elif physical is not None and commit is not None and allowance is not None:
        imgui.text_wrapped(
            f"OS headroom snapshot: physical {physical} B; commit {commit} B. "
            f"New-allocation allowance after reserves {allowance} B. "
            "Headroom can change before an allocation is admitted."
        )
    else:
        imgui.text_wrapped("Host memory headroom is unavailable in this status snapshot.")
    if "host_pending_allocation_bytes" in native:
        imgui.text_wrapped(
            f"Outstanding host allocation reservations {native['host_pending_allocation_bytes']} B."
        )


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
    _resident_pcm_budget_controls(ctx)
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
        imgui.text_wrapped("Resident accounting unavailable")
        return
    native = summary.native
    imgui.text_wrapped(
        f"Exact unique resident PCM {native.get('held_unique_pcm_bytes', 0)} B; "
        f"admitted pending PCM {native.get('pending_resident_bytes', 0)} B "
        f"({native.get('pending_reservation_count', 0)} reservations); "
        f"total {native.get('admitted_total_bytes', 0)} B."
    )
    if native.get("automatic"):
        imgui.text_wrapped("Effective admission: automatic; no fixed resident PCM limit.")
    else:
        imgui.text_wrapped(
            f"Effective manual resident PCM limit {native.get('budget_bytes', 0)} B."
        )
    _resident_host_memory_status(native)
    if native.get("over_budget"):
        imgui.text_wrapped(
            "Pinned readers temporarily exceed the allowance. Current audio stays valid; "
            "new allocations wait for safe retirement or a larger budget."
        )
    imgui.text_wrapped(
        "Preparing requests can still have unknown geometry and no admitted PCM reservation. "
        "PCM counters are not process RSS. Timing/analysis and per-job preparation have "
        "separate limits; Native/DSP, inference and other process overhead are not included. "
        "Available host memory is checked separately from these PCM counters."
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


def _stem_batch_controls(ctx: UiContext) -> None:
    summary = ctx.ui.stem_batch.summary
    deletion = ctx.ui.stem_delete.summary
    imgui.begin_disabled(
        (summary is not None and summary.active) or (deletion is not None and deletion.active)
    )
    if imgui.button("Generate Stems of all loaded PADs", (-1, 0)):
        ctx.ui.confirmation.request_all_loaded_stems()
    imgui.end_disabled()
    if summary is not None:
        _stem_batch_progress(ctx, summary)


def _stem_batch_progress(ctx: UiContext, summary: StemBatchSummary) -> None:
    settled = summary.done + summary.skipped + summary.failed + summary.cancelled
    imgui.text_wrapped(f"Settled {settled} of {summary.total} loaded PADs.")
    imgui.text_wrapped(
        f"Queued {summary.queued}; Running {summary.running}; Done {summary.done}; "
        f"Skipped {summary.skipped}; Failed {summary.failed}; "
        f"Cancelling {summary.cancelling}; Cancelled {summary.cancelled}."
    )
    imgui.text_wrapped(
        "Done means stems saved. Playback readiness is reported separately for each PAD; "
        "saved stems may still need audio preparation and adoption."
    )
    height = imgui.get_text_line_height_with_spacing() * 6
    imgui.begin_child("stem_batch_items", (-1, height), imgui.ChildFlags_.borders)
    try:
        for item in summary.items:
            label = "Done - stems saved" if item.state == "done" else item.state.capitalize()
            detail = (
                f" - {item.detail}"
                if item.state != "failed" and item.detail and item.detail != "Stems saved"
                else ""
            )
            imgui.text_wrapped(f"PAD {item.pad_id + 1}: {label}{detail}")
    finally:
        imgui.end_child()
    if summary.cancelling:
        imgui.text_wrapped("Cancellation is waiting for this batch's owned work to settle.")
    if summary.active:
        if imgui.button("Cancel batch", (-1, 0)):
            ctx.ui.stem_batch.cancel()
    elif imgui.button("Dismiss batch results", (-1, 0)):
        ctx.ui.stem_batch.dismiss_result()


def _stem_delete_controls(ctx: UiContext) -> None:
    summary = ctx.ui.stem_delete.summary
    imgui.begin_disabled(summary is not None and summary.active)
    if imgui.button("Delete Stems of all loaded PADs", (-1, 0)):
        ctx.ui.confirmation.request_all_loaded_stem_deletion()
    imgui.end_disabled()
    if summary is not None:
        _stem_delete_progress(ctx, summary)
    _stem_cleanup_progress(ctx.ui.stem_delete.cleanup_status)


def _stem_delete_progress(ctx: UiContext, summary: StemDeleteSummary) -> None:
    settled = summary.removed + summary.noop + summary.skipped + summary.failed
    imgui.text_wrapped(f"Deletion settled {settled} of {summary.total} loaded PADs.")
    imgui.text_wrapped(
        f"Removed {summary.removed}; Noop {summary.noop}; Skipped {summary.skipped}; "
        f"Failed {summary.failed}; Queued {summary.queued}."
    )
    imgui.text_wrapped(
        "Removed means stem choices and pending work were withdrawn. "
        "File cleanup is separate and waits for actual readers."
    )
    height = imgui.get_text_line_height_with_spacing() * 6
    imgui.begin_child("stem_delete_items", (-1, height), imgui.ChildFlags_.borders)
    try:
        for item in summary.items:
            detail = f" - {item.detail}" if item.state != "failed" and item.detail else ""
            imgui.text_wrapped(f"PAD {item.pad_id + 1}: {item.state.capitalize()}{detail}")
    finally:
        imgui.end_child()
    if not summary.active and imgui.button("Dismiss deletion results", (-1, 0)):
        ctx.ui.stem_delete.dismiss_result()


def _stem_cleanup_progress(status: StemCleanupStatus) -> None:
    pending = str(status.pending_targets) if status.pending_targets is not None else "unknown"
    imgui.text_wrapped(
        f"Global file cleanup: Pending {pending}; Errors {len(status.errors)}. "
        "These counts include other cleanup, not just this deletion."
    )


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
