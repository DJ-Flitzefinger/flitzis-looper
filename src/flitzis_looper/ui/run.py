from typing import TYPE_CHECKING

from imgui_bundle import hello_imgui, immapp

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

    def show_gui() -> None:
        render_ui(context)
        if on_frame is not None:
            on_frame()

    runner_params.callbacks.show_gui = show_gui

    def before_exit() -> None:
        if on_exit is not None:
            on_exit()
        controller.shut_down()

    runner_params.callbacks.before_exit = before_exit
    runner_params.callbacks.load_additional_fonts = lambda: load_fonts(context)
    add_ons_params = immapp.AddOnsParams(with_implot=True)

    immapp.run(runner_params, add_ons_params)


def load_fonts(ctx: UiContext) -> None:
    font_awesome6 = hello_imgui.DefaultIconFont.font_awesome6
    hello_imgui.get_runner_params().callbacks.default_icon_font = font_awesome6
    hello_imgui.imgui_default_settings.load_default_font_with_font_awesome_icons()

    font_loading_params_bold = hello_imgui.FontLoadingParams()
    font_filename_bold = "fonts/Roboto/Roboto-Bold.ttf"
    ctx.bold_font = hello_imgui.load_font(font_filename_bold, 16.0, font_loading_params_bold)
