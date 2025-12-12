"""flitzis_looper.ui - UI-Module für flitzis_looper.

Subpackages:
- widgets: Wiederverwendbare UI-Widgets (EQKnob, VUMeter)
- dialogs: Dialog-Fenster (volume, waveform, bpm_dialog)

Module:
- main_window: Hauptfenster-Setup
- loop_grid: Button-Grid für Loops
- toolbar: BPM/Speed/Volume Controls
- stems_panel: Stem-Buttons
"""

from .dialogs import WaveformEditor, set_bpm_manually, set_volume
from .loop_grid import (
    create_bank_buttons,
    create_button_grid,
    select_stems_button,
    update_all_stem_indicators,
    update_stem_indicator,
)
from .main_window import (
    WINDOW_GEOMETRY,
    WINDOW_RESIZABLE,
    WINDOW_TITLE,
    create_main_window,
    setup_window_protocol,
)
from .stems_panel import (
    create_stems_panel,
    get_active_loop_with_stems,
    get_selected_or_active_stems_button,
    update_stem_buttons_state,
    update_stop_stem_button_state,
)
from .toolbar import (
    create_master_volume,
    create_multi_loop_button,
    create_toolbar,
    get_bpm_display,
    get_speed_slider,
    update_bpm_lock_button_style,
    update_key_lock_button_style,
    update_multi_loop_button_style,
    update_reset_button_style,
    update_volume_label,
)
from .widgets import EQKnob, VUMeter

__all__ = [
    "WINDOW_GEOMETRY",
    "WINDOW_RESIZABLE",
    "WINDOW_TITLE",
    # widgets
    "EQKnob",
    "VUMeter",
    "WaveformEditor",
    "create_bank_buttons",
    # loop_grid
    "create_button_grid",
    # main_window
    "create_main_window",
    "create_master_volume",
    "create_multi_loop_button",
    # stems_panel
    "create_stems_panel",
    # toolbar
    "create_toolbar",
    "get_active_loop_with_stems",
    "get_bpm_display",
    "get_selected_or_active_stems_button",
    "get_speed_slider",
    "select_stems_button",
    "set_bpm_manually",
    # dialogs
    "set_volume",
    "setup_window_protocol",
    "update_all_stem_indicators",
    "update_bpm_lock_button_style",
    "update_key_lock_button_style",
    "update_multi_loop_button_style",
    "update_reset_button_style",
    "update_stem_buttons_state",
    "update_stem_indicator",
    "update_stop_stem_button_state",
    "update_volume_label",
]
