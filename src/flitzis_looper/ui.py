from __future__ import annotations

import dearpygui.dearpygui as dpg  # type: ignore[import-untyped]

from flitzis_looper.app import FlitzisLooperApp

VIEWPORT_WIDTH_PX = 960
VIEWPORT_HEIGHT_PX = 630
_PRIMARY_WINDOW_TAG = "primary_window"


def run_ui() -> None:
    """Start the Dear PyGui UI shell."""
    app = FlitzisLooperApp()
    _run_dearpygui(app)


def _run_dearpygui(_app: FlitzisLooperApp) -> None:
    """Run the Dear PyGui event loop.

    The application instance is intentionally held for the lifetime of the UI.
    """
    dpg.create_context()
    try:
        dpg.create_viewport(
            title="DJ Flitzefinger's Scratch Looper",
            width=VIEWPORT_WIDTH_PX,
            height=VIEWPORT_HEIGHT_PX,
            resizable=False,
        )

        with dpg.window(
            tag=_PRIMARY_WINDOW_TAG,
            no_title_bar=True,
            no_move=True,
            no_resize=True,
            no_collapse=True,
        ) as main_win:
            dpg.add_text("hello world")

        with dpg.theme() as main_win_theme, dpg.theme_component(dpg.mvAll):
            dpg.add_theme_style(dpg.mvStyleVar_WindowBorderSize, 0)

        dpg.bind_item_theme(main_win, main_win_theme)
        dpg.set_primary_window(_PRIMARY_WINDOW_TAG, value=True)

        dpg.setup_dearpygui()
        dpg.show_viewport()
        dpg.start_dearpygui()
    finally:
        dpg.destroy_context()
