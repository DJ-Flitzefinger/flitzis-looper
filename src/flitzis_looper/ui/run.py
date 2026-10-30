import os
from pathlib import Path
from typing import TYPE_CHECKING

from imgui_bundle import hello_imgui, imgui, immapp

from flitzis_looper.controller import AppController
from flitzis_looper.controller.stem_generation import protected_model_roots
from flitzis_looper.ui.constants import BG_RGBA, TITLE, VIEWPORT_PX
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import render_ui
from flitzis_looper.ui.render.file_dialog import cancel_file_dialog, file_dialog_error

if TYPE_CHECKING:
    from collections.abc import Callable


def prepare_ui_state_directory() -> Path:
    """Keep UI geometry inside its explicit owner before starting an Engine."""
    root = Path.cwd().resolve()
    configured = os.environ.get("FLITZIS_UI_STATE_DIR")
    directory = Path(configured) if configured else root / ".cache/ui"
    if not directory.is_absolute():
        message = "UI state directory must be absolute"
        raise ValueError(message)
    for part in (directory, *directory.parents):
        if part.is_symlink() or part.is_junction():
            message = "UI state directory must not traverse links"
            raise ValueError(message)
        if part.exists() and not part.is_dir():
            message = "UI state directory is occupied by a file"
            raise ValueError(message)
    directory = directory.resolve()
    protected = (*protected_model_roots(root), (root / "models").resolve())
    if (directory.is_relative_to(root) and not directory.is_relative_to(root / ".cache/ui")) or any(
        directory.is_relative_to(path) for path in protected
    ):
        message = "UI state directory overlaps protected project data"
        raise ValueError(message)
    ini = directory / "imgui.ini"
    if ini.is_symlink() or ini.is_junction() or (ini.exists() and not ini.is_file()):
        message = "UI settings leaf must be an ordinary file"
        raise ValueError(message)
    directory.mkdir(parents=True, exist_ok=True)
    return directory


def run_ui(
    controller: AppController | None = None,
    *,
    on_frame: Callable[[], None] | None = None,
    on_exit: Callable[[], None] | None = None,
) -> None:
    """Start the UI shell."""
    ui_directory = prepare_ui_state_directory()
    controller = AppController() if controller is None else controller
    context = UiContext(controller)

    runner_params = hello_imgui.RunnerParams()
    runner_params.ini_folder_type = hello_imgui.IniFolderType.absolute_path
    runner_params.ini_filename = str(ui_directory / "imgui.ini")
    runner_params.ini_filename_use_app_window_title = False
    runner_params.app_window_params.window_title = TITLE
    runner_params.imgui_window_params.menu_app_title = TITLE
    runner_params.app_window_params.window_geometry.size = VIEWPORT_PX
    runner_params.app_window_params.restore_previous_geometry = True
    runner_params.imgui_window_params.background_color = BG_RGBA
    runner_params.imgui_window_params.enable_viewports = True
    close_requested = False
    frame_rendered = False

    def show_gui() -> None:
        nonlocal frame_rendered
        render_ui(context)
        frame_rendered = True
        if on_frame is not None:
            on_frame()
        if close_requested:
            _show_close_pending(controller.pending_shutdown_error)

    def after_swap() -> None:
        nonlocal close_requested, frame_rendered
        deletion_polled = frame_rendered
        frame_rendered = False
        live_params = hello_imgui.get_runner_params()
        _poll_runtime_tick(context, closing=close_requested or bool(live_params.app_shall_exit))
        if not close_requested and not live_params.app_shall_exit:
            return
        close_requested = True
        # Pinned HelloImGui 902dd7d runs AfterSwap after event/menu handling and
        # checks appShallExit between complete frames. Keep clearing a sticky OS
        # close until actual Copy settlement permits BeforeExit teardown.
        context.ui.close_file_dialog()
        picker_closed = cancel_file_dialog()
        _observe_picker_error(controller)
        controller_ready = controller.prepare_shutdown(
            holds_polled=True, deletion_polled=deletion_polled
        )
        live_params.app_shall_exit = controller_ready and picker_closed
        controller.diagnostics.observe("shutdown", controller.pending_shutdown_error)

    runner_params.callbacks.show_gui = show_gui
    runner_params.callbacks.after_swap = after_swap

    runner_params.callbacks.load_additional_fonts = lambda: load_fonts(context)
    add_ons_params = immapp.AddOnsParams(with_implot=True)
    _run_ui_loop(controller, runner_params, add_ons_params, on_exit)


def _observe_picker_error(controller: AppController) -> None:
    error = file_dialog_error()
    controller.diagnostics.observe(
        "file_dialog",
        None if error is None else error[1],
        pad_id=None if error is None else error[0],
    )


def _run_ui_loop(
    controller: AppController,
    params: hello_imgui.RunnerParams,
    addons: immapp.AddOnsParams,
    on_exit: Callable[[], None] | None,
) -> None:
    exit_called = False

    def before_exit() -> None:
        nonlocal exit_called
        exit_called = True
        _finish_ui_shutdown(controller, on_exit)

    params.callbacks.before_exit = before_exit
    try:
        immapp.run(params, addons)
    except BaseException:
        if not exit_called:
            # The controller still enforces its own recording/Copy settlement
            # gate. Never bypass that gate after an unexpected runner failure.
            _finish_ui_shutdown(controller, None)
        raise
    finally:
        cancel_file_dialog(wait=True)
        _observe_picker_error(controller)


def _finish_ui_shutdown(controller: AppController, on_exit: Callable[[], None] | None) -> None:
    try:
        cancel_file_dialog(wait=True)
        _observe_picker_error(controller)
        if on_exit is not None:
            on_exit()
    finally:
        controller.shut_down()


def _poll_runtime_tick(context: UiContext, *, closing: bool = False) -> None:
    """Deliver global release/cancel before the existing always-reachable runtime tick."""
    io = imgui.get_io()
    if io.app_focus_lost:
        context.ui.waveform.pause_selected_pad_hold_on_release("focus_cancel")
    elif not imgui.is_mouse_down(imgui.MouseButton_.right):
        context.ui.waveform.pause_selected_pad_hold_on_release("button_up")
    if context.state.session.settings_open or not context.state.session.waveform_editor_open:
        context.ui.waveform.pause_selected_pad_hold_on_release("editor_hidden")
    if closing:
        context.ui.waveform.pause_selected_pad_hold_on_release("close")
    if context.state.session.global_stop_momentary_mute_active and (
        io.app_focus_lost or closing or not imgui.is_mouse_down(imgui.MouseButton_.middle)
    ):
        context.audio.global_.set_momentary_output_mute(enabled=False)
    context.ui.arrangement.poll(
        left_down=imgui.is_mouse_down(imgui.MouseButton_.left),
        right_down=imgui.is_mouse_down(imgui.MouseButton_.right),
        cancelled=io.app_focus_lost or closing or context.state.session.settings_open,
    )
    context.audio.poll.poll()


def _show_close_pending(_error: str | None) -> None:
    """Show bounded closing progress; the close poll reports errors to the console."""
    label = "Finishing pending work##close-pending"
    imgui.open_popup(label)
    viewport = imgui.get_main_viewport()
    style = imgui.get_style()
    font_size = imgui.get_font_size()
    title_width = (
        imgui.calc_text_size("Finishing pending work").x
        + 2 * style.frame_padding.x
        + 2 * style.window_padding.x
    )
    available_width = max(1.0, viewport.work_size.x - 2 * style.window_padding.x)
    width = min(max(26 * font_size, title_width), available_width)
    imgui.set_next_window_pos(viewport.get_center(), imgui.Cond_.always, (0.5, 0.5))
    imgui.set_next_window_size((width, 0), imgui.Cond_.always)
    visible, _ = imgui.begin_popup_modal(label, flags=imgui.WindowFlags_.always_auto_resize)
    if not visible:
        return
    try:
        imgui.text_wrapped("Please wait while pending work finishes.")
    finally:
        imgui.end_popup()


def load_fonts(ctx: UiContext) -> None:
    font_awesome6 = hello_imgui.DefaultIconFont.font_awesome6
    hello_imgui.get_runner_params().callbacks.default_icon_font = font_awesome6
    hello_imgui.imgui_default_settings.load_default_font_with_font_awesome_icons()

    font_loading_params_bold = hello_imgui.FontLoadingParams()
    font_filename_bold = "fonts/Roboto/Roboto-Bold.ttf"
    ctx.bold_font = hello_imgui.load_font(font_filename_bold, 16.0, font_loading_params_bold)
