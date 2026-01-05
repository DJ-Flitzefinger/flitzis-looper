from imgui_bundle import hello_imgui, immapp

from flitzis_looper.controller import LooperController
from flitzis_looper.ui.constants import BG_RGBA, TITLE, VIEWPORT_PX
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import render_ui


def run_ui() -> None:
    """Start the UI shell."""
    controller = LooperController()
    context = UiContext(controller)

    runner_params = hello_imgui.RunnerParams()
    runner_params.ini_folder_type = hello_imgui.IniFolderType.app_user_config_folder
    runner_params.app_window_params.window_title = TITLE
    runner_params.imgui_window_params.menu_app_title = TITLE
    runner_params.app_window_params.window_geometry.size = VIEWPORT_PX
    runner_params.app_window_params.restore_previous_geometry = True
    runner_params.imgui_window_params.background_color = BG_RGBA
    runner_params.callbacks.show_gui = lambda: render_ui(context)
    runner_params.callbacks.before_exit = controller.shut_down
    add_ons_params = immapp.AddOnsParams(with_implot=True)

    immapp.run(runner_params, add_ons_params)
