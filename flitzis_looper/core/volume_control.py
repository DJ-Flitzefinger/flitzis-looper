"""Volume control for flitzis_looper.
Handles master volume and multi-loop toggle functionality.
"""

import logging

from flitzis_looper.core.state import (
    COLOR_BTN_INACTIVE,
    COLOR_LOCK_OFF,
    COLOR_LOCK_ON,
    COLOR_TEXT,
    COLOR_TEXT_ACTIVE,
    get_all_banks_data,
    get_buttons,
    get_current_bank,
    get_loaded_loops,
    get_master_amp,
    get_master_volume,
    get_multi_loop_active,
)

logger = logging.getLogger(__name__)


def on_master_volume_change(val, volume_label):
    """Handler für Master-Volume-Änderungen.

    Args:
        val: Neuer Volume-Wert (0.0 - 1.0)
        volume_label: Label-Widget für die Prozentanzeige
    """
    try:
        master_volume = get_master_volume()
        master_amp = get_master_amp()

        volume = float(val)
        master_volume.set(volume)
        master_amp.value = volume
        volume_percent = int(volume * 100)
        volume_label.config(text=f"Master Volume {volume_percent}%")
    except Exception as e:
        logger.exception("Error setting master volume: %s", e)


def reset_master_volume(master_volume_slider):
    """Setzt das Master-Volume auf 100% zurück.

    Args:
        master_volume_slider: Der Master-Volume-Slider-Widget
    """
    master_volume_slider.set(1.0)


def toggle_multi_loop(multi_loop_btn, callbacks):
    """Toggle Multi-Loop Modus.
    Wenn deaktiviert, werden alle Loops bis auf den letzten gestoppt.

    Args:
        multi_loop_btn: Der Multi-Loop Button-Widget
        callbacks: Dict mit Callbacks:
            - update_button_label: Callback zum Aktualisieren der Button-Labels
    """
    multi_loop_active = get_multi_loop_active()
    all_banks_data = get_all_banks_data()
    loaded_loops = get_loaded_loops()
    current_bank = get_current_bank()
    buttons = get_buttons()

    multi_loop_active.set(not multi_loop_active.get())

    if multi_loop_active.get():
        multi_loop_btn.config(bg=COLOR_LOCK_ON, fg=COLOR_TEXT_ACTIVE)
    else:
        multi_loop_btn.config(bg=COLOR_LOCK_OFF, fg=COLOR_TEXT)
        # OPTIMIERUNG: Nur loaded_loops durchsuchen
        all_active = []
        for (bank_id, btn_id), loop in loaded_loops.items():
            data = all_banks_data[bank_id][btn_id]
            if data["active"]:
                all_active.append((bank_id, btn_id, data, loop))
        if len(all_active) > 1:
            for bank_id, btn_id, data, loop in all_active[:-1]:
                loop.stop()
                data["active"] = False
                if bank_id == current_bank.get():
                    buttons[btn_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
                    callbacks["update_button_label"](btn_id)
