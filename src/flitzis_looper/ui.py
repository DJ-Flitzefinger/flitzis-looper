from typing import Any

import dearpygui.dearpygui as dpg  # type: ignore[import-untyped]

from flitzis_looper.app import FlitzisLooperApp

VIEWPORT_WIDTH_PX = 960
VIEWPORT_HEIGHT_PX = 630
_PRIMARY_WINDOW_TAG = "primary_window"

GRID_SIZE = 6
NUM_BANKS = 6

_PAD_BUTTON_WIDTH_PX = 140
_PAD_BUTTON_HEIGHT_PX = 70
_BANK_BUTTON_WIDTH_PX = 140
_BANK_BUTTON_HEIGHT_PX = 32

_BG_RGBA = (30, 30, 30, 255)
_PAD_INACTIVE_RGBA = (58, 58, 58, 255)
_PAD_HOVER_RGBA = (85, 85, 85, 255)
_PAD_ACTIVE_RGBA = (100, 100, 100, 255)

_BANK_INACTIVE_RGBA = (204, 119, 0, 255)
_BANK_HOVER_RGBA = (255, 153, 0, 255)
_BANK_ACTIVE_RGBA = (255, 170, 0, 255)

_TEXT_RGBA = (255, 255, 255, 255)
_TEXT_ACTIVE_RGBA = (0, 0, 0, 255)


def run_ui() -> None:
    """Start the Dear PyGui UI shell."""
    app = FlitzisLooperApp()
    _run_dearpygui(app)


def _pad_tag(pad_id: int) -> str:
    return f"pad_btn_{pad_id:02d}"


def _bank_tag(bank_id: int) -> str:
    return f"bank_btn_{bank_id}"


def _create_base_theme() -> int:
    with dpg.theme() as theme, dpg.theme_component(dpg.mvAll):
        dpg.add_theme_color(dpg.mvThemeCol_WindowBg, _BG_RGBA)
        dpg.add_theme_color(dpg.mvThemeCol_Text, _TEXT_RGBA)
        dpg.add_theme_style(dpg.mvStyleVar_WindowBorderSize, 0)
        dpg.add_theme_style(dpg.mvStyleVar_WindowPadding, 10, 10)
        dpg.add_theme_style(dpg.mvStyleVar_ItemSpacing, 5, 5)
        dpg.add_theme_style(dpg.mvStyleVar_FrameRounding, 0)

    return theme


def _create_pad_theme() -> int:
    with dpg.theme() as theme, dpg.theme_component(dpg.mvButton):
        dpg.add_theme_color(dpg.mvThemeCol_Button, _PAD_INACTIVE_RGBA)
        dpg.add_theme_color(dpg.mvThemeCol_ButtonHovered, _PAD_HOVER_RGBA)
        dpg.add_theme_color(dpg.mvThemeCol_ButtonActive, _PAD_ACTIVE_RGBA)
        dpg.add_theme_color(dpg.mvThemeCol_Text, _TEXT_RGBA)

    return theme


def _create_bank_theme(*, active: bool) -> int:
    if active:
        button = _BANK_ACTIVE_RGBA
        text = _TEXT_ACTIVE_RGBA
    else:
        button = _BANK_INACTIVE_RGBA
        text = _TEXT_RGBA

    with dpg.theme() as theme, dpg.theme_component(dpg.mvButton):
        dpg.add_theme_color(dpg.mvThemeCol_Button, button)
        dpg.add_theme_color(dpg.mvThemeCol_ButtonHovered, _BANK_HOVER_RGBA)
        dpg.add_theme_color(dpg.mvThemeCol_ButtonActive, _BANK_ACTIVE_RGBA)
        dpg.add_theme_color(dpg.mvThemeCol_Text, text)

    return theme


def _update_bank_button_highlight(
    selected_bank: int,
    *,
    bank_active_theme: int,
    bank_inactive_theme: int,
) -> None:
    for bank_id in range(1, NUM_BANKS + 1):
        theme = bank_active_theme if bank_id == selected_bank else bank_inactive_theme
        dpg.bind_item_theme(_bank_tag(bank_id), theme)


def _build_pad_grid(*, pad_theme: int) -> None:
    with dpg.table(header_row=False):
        for _ in range(GRID_SIZE):
            dpg.add_table_column(width_fixed=True)

        for row in range(GRID_SIZE):
            with dpg.table_row():
                for col in range(GRID_SIZE):
                    pad_id = row * GRID_SIZE + col + 1
                    tag = _pad_tag(pad_id)
                    dpg.add_button(
                        label=str(pad_id),
                        tag=tag,
                        width=_PAD_BUTTON_WIDTH_PX,
                        height=_PAD_BUTTON_HEIGHT_PX,
                    )
                    dpg.bind_item_theme(tag, pad_theme)


def _build_bank_row(
    app: FlitzisLooperApp,
    *,
    bank_active_theme: int,
    bank_inactive_theme: int,
) -> None:
    def _on_bank_clicked(_sender: int, _app_data: Any, bank_id: int) -> None:
        app.select_bank(bank_id)
        _update_bank_button_highlight(
            app.selected_bank,
            bank_active_theme=bank_active_theme,
            bank_inactive_theme=bank_inactive_theme,
        )

    with dpg.group(horizontal=True):
        for bank_id in range(1, NUM_BANKS + 1):
            dpg.add_button(
                label=f"Bank {bank_id}",
                tag=_bank_tag(bank_id),
                width=_BANK_BUTTON_WIDTH_PX,
                height=_BANK_BUTTON_HEIGHT_PX,
                callback=_on_bank_clicked,
                user_data=bank_id,
            )

    _update_bank_button_highlight(
        app.selected_bank,
        bank_active_theme=bank_active_theme,
        bank_inactive_theme=bank_inactive_theme,
    )


def _build_performance_view(
    app: FlitzisLooperApp,
    *,
    pad_theme: int,
    bank_active_theme: int,
    bank_inactive_theme: int,
) -> None:
    _build_pad_grid(pad_theme=pad_theme)
    dpg.add_spacer(height=10)
    _build_bank_row(
        app,
        bank_active_theme=bank_active_theme,
        bank_inactive_theme=bank_inactive_theme,
    )


def _run_dearpygui(app: FlitzisLooperApp) -> None:
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

        dpg.bind_theme(_create_base_theme())
        pad_theme = _create_pad_theme()
        bank_active_theme = _create_bank_theme(active=True)
        bank_inactive_theme = _create_bank_theme(active=False)

        with dpg.window(
            tag=_PRIMARY_WINDOW_TAG,
            no_title_bar=True,
            no_move=True,
            no_resize=True,
            no_collapse=True,
            width=VIEWPORT_WIDTH_PX,
            height=VIEWPORT_HEIGHT_PX,
            pos=(0, 0),
        ):
            _build_performance_view(
                app,
                pad_theme=pad_theme,
                bank_active_theme=bank_active_theme,
                bank_inactive_theme=bank_inactive_theme,
            )

        dpg.set_primary_window(_PRIMARY_WINDOW_TAG, value=True)
        dpg.setup_dearpygui()
        dpg.show_viewport()
        dpg.start_dearpygui()
    finally:
        dpg.destroy_context()
