"""Configuration management for flitzis_looper.
Handles saving and loading configuration to/from JSON file.
"""

import json
import logging
import os

from flitzis_looper.core.state import (
    NUM_BANKS,
    ensure_stems_structure,
    get_all_banks_data,
    get_current_bank,
    get_master_amp,
    get_master_volume,
    get_root,
    get_save_config_timer,
    set_button_data_ref,
    set_save_config_timer,
)
from flitzis_looper.utils.paths import CONFIG_FILE
from flitzis_looper.utils.threading import io_executor

logger = logging.getLogger(__name__)


def save_config():
    """Speichert die aktuelle Konfiguration in die config.json Datei.
    Wird typischerweise über save_config_async() aufgerufen.
    """
    try:
        all_banks_data = get_all_banks_data()
        master_volume = get_master_volume()

        config = {"global": {"master_volume": master_volume.get()}, "banks": {}}
        for bank_id in range(1, NUM_BANKS + 1):
            config["banks"][str(bank_id)] = {}
            for btn_id, data in all_banks_data[bank_id].items():
                if data["file"]:
                    config["banks"][str(bank_id)][str(btn_id)] = {
                        "file": data["file"],
                        "bpm": data["bpm"],
                        "gain_db": data["gain_db"],
                        "loop_start": data.get("loop_start", 0.0),
                        "loop_end": data.get("loop_end"),
                        "auto_loop_active": data.get("auto_loop_active", True),
                        "auto_loop_bars": data.get("auto_loop_bars", 8),
                        "auto_loop_custom_mode": data.get("auto_loop_custom_mode", False),
                        "intro_active": data.get("intro_active", False),
                        "intro_bars": data.get("intro_bars", 4),
                        "intro_custom_mode": data.get("intro_custom_mode", False),
                        "eq_low": data.get("eq_low", 0.0),
                        "eq_mid": data.get("eq_mid", 0.0),
                        "eq_high": data.get("eq_high", 0.0),
                        # STEMS: werden NICHT gespeichert - nur im RAM während der Session!
                        # Die Stems müssen bei jedem Programmstart neu generiert werden.
                    }
        with open(CONFIG_FILE, "w", encoding="utf-8") as f:
            json.dump(config, f, indent=2)
    except Exception as e:
        logger.exception("Error saving config: %s", e)


def save_config_async():
    """OPTIMIERUNG: Debounced config save - wartet 2 Sekunden vor dem Speichern.
    Verhindert zu häufiges Speichern bei schnellen Änderungen.
    """
    import tkinter as tk

    root = get_root()
    save_config_timer = get_save_config_timer()

    try:
        if save_config_timer:
            root.after_cancel(save_config_timer)
    except (ValueError, tk.TclError):
        pass  # Timer war ungültig oder bereits abgelaufen

    new_timer = root.after(2000, lambda: io_executor.submit(save_config))
    set_save_config_timer(new_timer)


def save_config_immediate():
    """Speichert sofort ohne Debouncing (für wichtige Änderungen wie beim Schließen)."""
    import tkinter as tk

    root = get_root()
    save_config_timer = get_save_config_timer()

    try:
        if save_config_timer:
            root.after_cancel(save_config_timer)
            set_save_config_timer(None)
    except (ValueError, tk.TclError):
        pass

    io_executor.submit(save_config)


def load_config(register_loaded_loop_callback, PyoLoop_class):
    """Lädt die Konfiguration aus der config.json Datei.

    Args:
        register_loaded_loop_callback: Funktion zum Registrieren geladener Loops
        PyoLoop_class: Die PyoLoop-Klasse zum Erstellen neuer Loop-Objekte
    """
    all_banks_data = get_all_banks_data()
    master_volume = get_master_volume()
    master_amp = get_master_amp()
    current_bank = get_current_bank()

    try:
        if not os.path.exists(CONFIG_FILE):
            return
        with open(CONFIG_FILE, encoding="utf-8") as f:
            config = json.load(f)

        if "global" in config:
            master_volume.set(config["global"].get("master_volume", 1.0))
            master_amp.value = config["global"].get("master_volume", 1.0)

        if "banks" in config:
            for bank_id_str, bank_config in config["banks"].items():
                bank_id = int(bank_id_str)
                for btn_id_str, btn_config in bank_config.items():
                    btn_id = int(btn_id_str)
                    if btn_config.get("file") and os.path.exists(btn_config["file"]):
                        data = all_banks_data[bank_id][btn_id]
                        for key, value in btn_config.items():
                            # STEMS: Alte stems_available/stems_states aus Config IGNORIEREN
                            # Stems werden nur im RAM gehalten und müssen neu generiert werden
                            if key in {"stems_available", "stems_states"}:
                                continue  # Ignorieren
                            data[key] = value

                        # Stems-Struktur sicherstellen (frisch, nicht aus Config)
                        ensure_stems_structure(data)

                        loop = PyoLoop_class()
                        if loop.load(data["file"], data["loop_start"], data["loop_end"]):
                            loop.set_gain(data["gain_db"])
                            # Apply EQ settings
                            loop.set_eq(
                                data.get("eq_low", 0.0),
                                data.get("eq_mid", 0.0),
                                data.get("eq_high", 0.0),
                            )
                            data["pyo"] = loop
                            # OPTIMIERUNG: Loop registrieren
                            register_loaded_loop_callback(bank_id, btn_id, loop)

        # button_data auf aktuelle Bank setzen
        set_button_data_ref(current_bank.get())

    except Exception as e:
        logger.exception("Error loading config: %s", e)
