"""
flitzis_looper.ui - UI-Module für flitzis_looper.

Subpackages:
- widgets: Wiederverwendbare UI-Widgets (EQKnob, VUMeter)
- dialogs: Dialog-Fenster (volume, waveform, bpm_dialog)

Module:
- main_window: Hauptfenster-Setup
- loop_grid: Button-Grid für Loops
- toolbar: BPM/Speed/Volume Controls
- stems_panel: Stem-Buttons
"""

from .main_window import (
    create_main_window,
    setup_window_protocol,
    WINDOW_TITLE,
    WINDOW_GEOMETRY,
    WINDOW_RESIZABLE,
)
from .widgets import EQKnob, VUMeter
from .dialogs import set_volume, WaveformEditor, set_bpm_manually
from .loop_grid import (
    create_button_grid,
    create_bank_buttons,
    update_stem_indicator,
    update_all_stem_indicators,
    select_stems_button,
)
from .toolbar import (
    create_toolbar,
    create_master_volume,
    create_multi_loop_button,
    update_reset_button_style,
    update_key_lock_button_style,
    update_bpm_lock_button_style,
    update_multi_loop_button_style,
    update_volume_label,
    get_bpm_display,
    get_speed_slider,
)
from .stems_panel import (
    create_stems_panel,
    update_stem_buttons_state,
    update_stop_stem_button_state,
    get_active_loop_with_stems,
    get_selected_or_active_stems_button,
)

__all__ = [
    # main_window
    'create_main_window',
    'setup_window_protocol',
    'WINDOW_TITLE',
    'WINDOW_GEOMETRY',
    'WINDOW_RESIZABLE',
    # widgets
    'EQKnob',
    'VUMeter',
    # dialogs
    'set_volume',
    'WaveformEditor',
    'set_bpm_manually',
    # loop_grid
    'create_button_grid',
    'create_bank_buttons',
    'update_stem_indicator',
    'update_all_stem_indicators',
    'select_stems_button',
    # toolbar
    'create_toolbar',
    'create_master_volume',
    'create_multi_loop_button',
    'update_reset_button_style',
    'update_key_lock_button_style',
    'update_bpm_lock_button_style',
    'update_multi_loop_button_style',
    'update_volume_label',
    'get_bpm_display',
    'get_speed_slider',
    # stems_panel
    'create_stems_panel',
    'update_stem_buttons_state',
    'update_stop_stem_button_state',
    'get_active_loop_with_stems',
    'get_selected_or_active_stems_button',
]
