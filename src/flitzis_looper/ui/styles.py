from typing import Literal

from imgui_bundle import imgui

from flitzis_looper.ui.constants import (
    BANK_BORDER_RGBA,
    BANK_HOVERED_RGBA,
    BANK_PRESSED_RGBA,
    BANK_RGBA,
    CONTROL_ACTIVE_BORDER_RGBA,
    CONTROL_ACTIVE_HOVERED_RGBA,
    CONTROL_ACTIVE_PRESSED_RGBA,
    CONTROL_ACTIVE_RGBA,
    CONTROL_BORDER_RGBA,
    CONTROL_BORDER_SELECTED_RGBA,
    CONTROL_HOVERED_RGBA,
    CONTROL_PRESSED_RGBA,
    CONTROL_RGBA,
    MODE_OFF_BORDER_RGBA,
    MODE_OFF_HOVERED_RGBA,
    MODE_OFF_PRESSED_RGBA,
    MODE_OFF_RGBA,
    MODE_ON_BORDER_RGBA,
    MODE_ON_HOVERED_RGBA,
    MODE_ON_PRESSED_RGBA,
    MODE_ON_RGBA,
    TEXT_ACTIVE_RGBA,
    TEXT_RGBA,
)

type ButtonStyleName = Literal[
    "regular",
    "regular-selected",
    "active",
    "active-selected",
    "bank",
    "bank-active",
    "mode-on",
    "mode-off",
]
type ButtonStyles = dict[ButtonStyleName, dict[int, imgui.ImVec4Like]]

BUTTON_STYLES: ButtonStyles = {
    "regular": {
        imgui.Col_.button: CONTROL_RGBA,
        imgui.Col_.button_active: CONTROL_PRESSED_RGBA,
        imgui.Col_.button_hovered: CONTROL_HOVERED_RGBA,
        imgui.Col_.border: CONTROL_BORDER_RGBA,
        imgui.Col_.text: TEXT_RGBA,
    },
    "regular-selected": {
        imgui.Col_.button: CONTROL_RGBA,
        imgui.Col_.button_active: CONTROL_PRESSED_RGBA,
        imgui.Col_.button_hovered: CONTROL_HOVERED_RGBA,
        imgui.Col_.border: CONTROL_BORDER_SELECTED_RGBA,
        imgui.Col_.text: TEXT_RGBA,
    },
    "active": {
        imgui.Col_.button: CONTROL_ACTIVE_RGBA,
        imgui.Col_.button_active: CONTROL_ACTIVE_PRESSED_RGBA,
        imgui.Col_.button_hovered: CONTROL_ACTIVE_HOVERED_RGBA,
        imgui.Col_.border: CONTROL_ACTIVE_BORDER_RGBA,
        imgui.Col_.text: TEXT_ACTIVE_RGBA,
    },
    "active-selected": {
        imgui.Col_.button: CONTROL_ACTIVE_RGBA,
        imgui.Col_.button_active: CONTROL_ACTIVE_PRESSED_RGBA,
        imgui.Col_.button_hovered: CONTROL_ACTIVE_HOVERED_RGBA,
        imgui.Col_.border: CONTROL_BORDER_SELECTED_RGBA,
        imgui.Col_.text: TEXT_ACTIVE_RGBA,
    },
    "bank": {
        imgui.Col_.button: BANK_RGBA,
        imgui.Col_.button_active: BANK_PRESSED_RGBA,
        imgui.Col_.button_hovered: BANK_HOVERED_RGBA,
        imgui.Col_.border: BANK_BORDER_RGBA,
        imgui.Col_.text: TEXT_RGBA,
    },
    "bank-active": {
        imgui.Col_.button: BANK_PRESSED_RGBA,
        imgui.Col_.button_active: BANK_PRESSED_RGBA,
        imgui.Col_.button_hovered: BANK_HOVERED_RGBA,
        imgui.Col_.border: BANK_BORDER_RGBA,
        imgui.Col_.text: TEXT_ACTIVE_RGBA,
    },
    "mode-on": {
        imgui.Col_.button: MODE_ON_RGBA,
        imgui.Col_.button_active: MODE_ON_PRESSED_RGBA,
        imgui.Col_.button_hovered: MODE_ON_HOVERED_RGBA,
        imgui.Col_.border: MODE_ON_BORDER_RGBA,
        imgui.Col_.text: TEXT_ACTIVE_RGBA,
    },
    "mode-off": {
        imgui.Col_.button: MODE_OFF_RGBA,
        imgui.Col_.button_active: MODE_OFF_PRESSED_RGBA,
        imgui.Col_.button_hovered: MODE_OFF_HOVERED_RGBA,
        imgui.Col_.border: MODE_OFF_BORDER_RGBA,
        imgui.Col_.text: TEXT_RGBA,
    },
}
