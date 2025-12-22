from imgui_bundle import hello_imgui, immapp

from flitzis_looper.app import FlitzisLooperApp
from flitzis_looper.ui.constants import BG_RGBA, TITLE, VIEWPORT_PX
from flitzis_looper.ui.main import main_gui


def run_ui() -> None:
    """Start the Dear PyGui UI shell."""
    app = FlitzisLooperApp()
    app.audio_engine.run()

    runner_params = hello_imgui.RunnerParams()
    runner_params.app_window_params.window_title = TITLE
    runner_params.imgui_window_params.menu_app_title = TITLE
    runner_params.app_window_params.window_geometry.size = VIEWPORT_PX
    runner_params.app_window_params.restore_previous_geometry = True
    runner_params.imgui_window_params.background_color = BG_RGBA
    runner_params.callbacks.show_gui = lambda: main_gui(app)

    add_ons_params = immapp.AddOnsParams(with_implot=True)

    # hello_imgui.run(runner_params)
    immapp.run(runner_params, add_ons_params)
