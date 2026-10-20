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
        live_params.app_shall_exit = controller.prepare_shutdown(
            holds_polled=True, deletion_polled=deletion_polled
        )
        controller.diagnostics.observe("shutdown", controller.pending_shutdown_error)

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
