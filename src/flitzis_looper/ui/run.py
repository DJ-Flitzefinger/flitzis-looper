from typing import TYPE_CHECKING

from imgui_bundle import hello_imgui, imgui, immapp

from flitzis_looper.controller import AppController
from flitzis_looper.ui.constants import BG_RGBA, TITLE, VIEWPORT_PX
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import render_ui

if TYPE_CHECKING:
    from collections.abc import Callable


def run_ui(
    controller: AppController | None = None,
    *,
    on_frame: Callable[[], None] | None = None,
    on_exit: Callable[[], None] | None = None,
) -> None:
    """Start the UI shell."""
    controller = AppController() if controller is None else controller
    context = UiContext(controller)

    runner_params = hello_imgui.RunnerParams()
    runner_params.ini_folder_type = hello_imgui.IniFolderType.app_user_config_folder
    runner_params.app_window_params.window_title = TITLE
    runner_params.imgui_window_params.menu_app_title = TITLE
    runner_params.app_window_params.window_geometry.size = VIEWPORT_PX
    runner_params.app_window_params.restore_previous_geometry = True
    runner_params.imgui_window_params.background_color = BG_RGBA
    runner_params.imgui_window_params.enable_viewports = True
    close_requested = False

    def show_gui() -> None:
        render_ui(context)
        if on_frame is not None:
            on_frame()
        if close_requested:
            _show_close_pending(controller.pending_pad_error)

    def after_swap() -> None:
        nonlocal close_requested
        live_params = hello_imgui.get_runner_params()
        _poll_runtime_tick(context, closing=close_requested or bool(live_params.app_shall_exit))
        if not close_requested and not live_params.app_shall_exit:
            return
        close_requested = True
        # Pinned HelloImGui 902dd7d runs AfterSwap after event/menu handling and
        # checks appShallExit between complete frames. Keep clearing a sticky OS
        # close until actual Copy settlement permits BeforeExit teardown.
        live_params.app_shall_exit = controller.prepare_shutdown(holds_polled=True)

    runner_params.callbacks.show_gui = show_gui
    runner_params.callbacks.after_swap = after_swap

    def before_exit() -> None:
        if on_exit is not None:
            on_exit()
        controller.shut_down()

    runner_params.callbacks.before_exit = before_exit
    runner_params.callbacks.load_additional_fonts = lambda: load_fonts(context)
    add_ons_params = immapp.AddOnsParams(with_implot=True)

    immapp.run(runner_params, add_ons_params)


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
    context.audio.poll.poll()


def _show_close_pending(error: str | None) -> None:
    """Keep the pending close and any failed Copy settlement visible."""
    label = "Finishing pad operation##close-pending"
    imgui.open_popup(label)
    visible, _ = imgui.begin_popup_modal(label, flags=imgui.WindowFlags_.always_auto_resize)
    if not visible:
        return
    try:
        imgui.text_wrapped("Finishing pending pad changes and pause release before closing.")
        if error is not None:
            imgui.text_wrapped(f"Copy could not finish: {error}")
    finally:
        imgui.end_popup()


def load_fonts(ctx: UiContext) -> None:
    font_awesome6 = hello_imgui.DefaultIconFont.font_awesome6
    hello_imgui.get_runner_params().callbacks.default_icon_font = font_awesome6
    hello_imgui.imgui_default_settings.load_default_font_with_font_awesome_icons()

    font_loading_params_bold = hello_imgui.FontLoadingParams()
    font_filename_bold = "fonts/Roboto/Roboto-Bold.ttf"
    ctx.bold_font = hello_imgui.load_font(font_filename_bold, 16.0, font_loading_params_bold)
