"""Main application module for flitzis_looper.

Orchestrates the application startup, shutdown, and coordinates all modules.
Uses CallbackRegistry for centralized callback management.
"""

import builtins
import contextlib
import logging
import threading
import tkinter as tk

import matplotlib as mpl

from flitzis_looper.audio.engine import init_engine
from flitzis_looper.audio.loop import PyoLoop
from flitzis_looper.audio.pitch import (
    cleanup_stem_caches,
    invalidate_stem_caches,
    precache_pitched_stems_if_needed,
)
from flitzis_looper.audio.stems_engine import (
    _cleanup_stem_players,
    apply_stem_mix,
    initialize_stem_players,
    stop_stem_players,
    update_stem_eq,
    update_stem_gains,
)
from flitzis_looper.audio.stems_separation import delete_stems, generate_stems
from flitzis_looper.core.banks import (
    select_stems_button,
    switch_bank,
    update_all_button_labels,
    update_all_stem_indicators,
    update_button_label,
)
from flitzis_looper.core.bpm_control import (
    adjust_bpm_by_delta,
    detect_bpm,
    detect_bpm_async,
    on_speed_change,
    reset_pitch,
    toggle_bpm_lock,
    toggle_key_lock,
    update_bpm_display,
    update_speed_from_master_bpm,
    validate_bpm_entry,
)
from flitzis_looper.core.callbacks import CallbackRegistry, get_registry
from flitzis_looper.core.config import ConfigManager, save_config_async
from flitzis_looper.core.loops import load_loop, stop_loop, trigger_loop, unload_loop
from flitzis_looper.core.state import (
    COLOR_BG,
    STEM_NAMES,
    get_all_banks_data,
    get_loaded_loops,
    get_master_volume,
    get_root,
    init_banks,
    init_state,
    register_loaded_loop,
)
from flitzis_looper.core.state import (
    set_master_amp as state_set_master_amp,
)
from flitzis_looper.core.stems_control import (
    get_active_loop_with_stems,
    on_stem_momentary_activate,
    on_stem_momentary_release,
    on_stem_toggle,
    on_stop_stem_momentary,
    on_stop_stem_momentary_release,
    on_stop_stem_toggle,
)
from flitzis_looper.core.volume_control import (
    on_master_volume_change,
    reset_master_volume,
    toggle_multi_loop,
)
from flitzis_looper.ui.context_menu import LoopContextMenu
from flitzis_looper.ui.dialogs import WaveformEditor, set_bpm_manually, set_volume
from flitzis_looper.ui.widgets import LoopGridWidget, StemsPanelWidget, ToolbarWidget
from flitzis_looper.utils.threading import bpm_executor, io_executor, start_gui_queue_processor

logger = logging.getLogger(__name__)


def cleanup_pitch_caches():
    """Gibt alle Pitch-Caches frei beim Beenden (RAM-basiert)."""
    loaded_loops = get_loaded_loops()

    try:
        for loop in loaded_loops.values():
            if hasattr(loop, "_invalidate_pitch_cache"):
                loop._invalidate_pitch_cache()
    except Exception as e:
        logger.debug("Error cleaning up pitch caches: %s", e)


def on_closing(engine):
    """Proper shutdown handler - stoppt alle Audio-Objekte bevor das Fenster geschlossen wird.

    Args:
        engine: AudioEngine-Instanz
    """
    root = get_root()
    all_banks_data = get_all_banks_data()
    loaded_loops = get_loaded_loops()
    shutdown_thread_started = False

    try:
        # 1. Alle laufenden Loops stoppen
        for (bank_id, btn_id), loop in list(loaded_loops.items()):
            with contextlib.suppress(builtins.BaseException):
                loop.stop()

            # Stem-Player stoppen
            data = all_banks_data[bank_id][btn_id]
            if data["stems"].get("initialized"):
                try:
                    if data["stems"].get("final_output"):
                        data["stems"]["final_output"].stop()
                    for obj_name in ["stem_mix", "eq_low", "eq_mid", "eq_high"]:
                        if data["stems"].get(obj_name):
                            with contextlib.suppress(builtins.BaseException):
                                data["stems"][obj_name].stop()
                    if data["stems"].get("master_phasor"):
                        data["stems"]["master_phasor"].stop()
                    if data["stems"].get("main_player"):
                        data["stems"]["main_player"].stop()
                    for stem in STEM_NAMES:
                        if data["stems"]["players"].get(stem):
                            with contextlib.suppress(builtins.BaseException):
                                data["stems"]["players"][stem].stop()
                        if data["stems"]["gains"].get(stem):
                            with contextlib.suppress(builtins.BaseException):
                                data["stems"]["gains"][stem].stop()
                except Exception as e:
                    logger.debug("Error stopping stems for %s/%s: %s", bank_id, btn_id, e)

        # 2. Caches freigeben
        cleanup_pitch_caches()
        cleanup_stem_caches()

        # 3. Config speichern
        ConfigManager.instance().save_immediate()

        # 4/5/6. Sauber herunterfahren ohne GUI-Freeze
        with contextlib.suppress(Exception):
            root.withdraw()

        def _shutdown_workers_and_close():
            with contextlib.suppress(Exception):
                engine.shutdown()

            try:
                io_executor.shutdown(wait=True, cancel_futures=True)
            except TypeError:
                io_executor.shutdown(wait=True)
            except Exception:
                pass

            try:
                bpm_executor.shutdown(wait=True, cancel_futures=True)
            except TypeError:
                bpm_executor.shutdown(wait=True)
            except Exception:
                pass

            with contextlib.suppress(Exception):
                root.after(0, root.destroy)

        threading.Thread(target=_shutdown_workers_and_close, daemon=True).start()
        shutdown_thread_started = True
        return

    except Exception:
        logger.exception("Error during shutdown")
    finally:
        if not shutdown_thread_started:
            with contextlib.suppress(Exception):
                root.destroy()


def _register_callbacks(registry: CallbackRegistry, stems_panel_ref: list):
    """Registriert alle Callbacks in der zentralen Registry.

    Args:
        registry: Die CallbackRegistry-Instanz
        stems_panel_ref: Liste mit [stems_panel] für späteren Zugriff
    """

    # Wrapper für Stem-Button-Update
    def update_stem_buttons_state_impl():
        if stems_panel_ref and stems_panel_ref[0]:
            stems_panel_ref[0].update_state()

    # ============== LOOP CALLBACKS ==============
    registry.register("trigger_loop", trigger_loop, group="loop")
    registry.register("stop_loop", stop_loop, group="loop")
    registry.register("load_loop", load_loop, group="loop")
    registry.register("unload_loop", unload_loop, group="loop")
    registry.register("update_button_label", update_button_label, group="loop")
    registry.register("update_all_button_labels", update_all_button_labels, group="loop")
    registry.register("update_all_stem_indicators", update_all_stem_indicators, group="loop")
    registry.register("save_config_async", save_config_async, group="loop")
    registry.register("PyoLoop_class", PyoLoop, group="loop")

    # ============== STEM CALLBACKS ==============
    registry.register("initialize_stem_players", initialize_stem_players, group="stem")
    registry.register("stop_stem_players", stop_stem_players, group="stem")
    registry.register("update_stem_gains", update_stem_gains, group="stem")
    registry.register("update_stem_eq", update_stem_eq, group="stem")
    registry.register("_cleanup_stem_players", _cleanup_stem_players, group="stem")
    registry.register("apply_stem_mix", apply_stem_mix, group="stem")
    registry.register("generate_stems", generate_stems, group="stem")
    registry.register("delete_stems", delete_stems, group="stem")
    registry.register(
        "precache_pitched_stems_if_needed", precache_pitched_stems_if_needed, group="stem"
    )
    registry.register("invalidate_stem_caches", invalidate_stem_caches, group="stem")

    # ============== STEM CONTROL CALLBACKS ==============
    registry.register("on_stem_toggle", on_stem_toggle, group="stem_control")
    registry.register(
        "on_stem_momentary_activate", on_stem_momentary_activate, group="stem_control"
    )
    registry.register("on_stem_momentary_release", on_stem_momentary_release, group="stem_control")
    registry.register("on_stop_stem_toggle", on_stop_stem_toggle, group="stem_control")
    registry.register("on_stop_stem_momentary", on_stop_stem_momentary, group="stem_control")
    registry.register(
        "on_stop_stem_momentary_release", on_stop_stem_momentary_release, group="stem_control"
    )
    registry.register(
        "get_active_loop_with_stems", get_active_loop_with_stems, group="stem_control"
    )
    registry.register(
        "update_stem_buttons_state", update_stem_buttons_state_impl, group="stem_control"
    )

    # ============== BPM/SPEED CALLBACKS ==============
    registry.register("on_speed_change", on_speed_change, group="bpm")
    registry.register("reset_pitch", reset_pitch, group="bpm")
    registry.register("adjust_bpm_by_delta", adjust_bpm_by_delta, group="bpm")
    registry.register("toggle_key_lock", toggle_key_lock, group="bpm")
    registry.register("toggle_bpm_lock", toggle_bpm_lock, group="bpm")
    registry.register("validate_bpm_entry", validate_bpm_entry, group="bpm")
    registry.register("update_speed_from_master_bpm", update_speed_from_master_bpm, group="bpm")
    registry.register("update_bpm_display", update_bpm_display, group="bpm")
    registry.register("detect_bpm", detect_bpm, group="bpm")
    registry.register("detect_bpm_async", detect_bpm_async, group="bpm")

    # ============== VOLUME CALLBACKS ==============
    registry.register("on_master_volume_change", on_master_volume_change, group="volume")
    registry.register("reset_master_volume", reset_master_volume, group="volume")
    registry.register("toggle_multi_loop", toggle_multi_loop, group="volume")

    # ============== BANK CALLBACKS ==============
    registry.register("switch_bank", switch_bank, group="bank")
    registry.register("select_stems_button", select_stems_button, group="bank")

    # ============== DIALOG CALLBACKS ==============
    registry.register("WaveformEditor", WaveformEditor, group="dialog")
    registry.register("set_bpm_manually", set_bpm_manually, group="dialog")
    registry.register("set_volume", set_volume, group="dialog")


def main():
    """Hauptfunktion - startet die Anwendung.

    Diese Funktion orchestriert den gesamten Startup-Prozess.
    """
    mpl.use("TkAgg")

    # 1. AudioEngine erstellen und starten
    engine = init_engine(sr=44100, nchnls=2, buffersize=1024, duplex=0)
    engine.boot()
    engine.start()

    # 2. Tk root erstellen
    root = tk.Tk()
    root.title("Dj Flitzefinger's Scratch-Looper")
    root.geometry("960x630")
    root.resizable(width=False, height=False)
    root.configure(bg=COLOR_BG)

    # 3. State initialisieren
    init_state(root)

    # 4. Master-Amplitude aus AudioEngine registrieren
    state_set_master_amp(engine.master_amp)

    # 5. Banks initialisieren
    init_banks()

    # 6. Callbacks registrieren
    registry = get_registry()
    stems_panel_ref = [None]  # Mutable container für späteren Zugriff
    _register_callbacks(registry, stems_panel_ref)

    # Data-Referenzen
    master_volume = get_master_volume()

    # ============== CALLBACK DICTS (für Widgets - aus Registry) ==============
    # Loop-Callbacks als Dict für Widgets die noch Dicts erwarten
    loop_callbacks = {
        "update_button_label": registry.get("update_button_label"),
        "update_stem_buttons_state": registry.get("update_stem_buttons_state"),
        "update_all_stem_indicators": registry.get("update_all_stem_indicators"),
        "initialize_stem_players": registry.get("initialize_stem_players"),
        "update_stem_gains": registry.get("update_stem_gains"),
        "stop_stem_players": registry.get("stop_stem_players"),
        "_cleanup_stem_players": registry.get("_cleanup_stem_players"),
        "precache_pitched_stems_if_needed": registry.get("precache_pitched_stems_if_needed"),
        "save_config_async": registry.get("save_config_async"),
        "detect_bpm_async": lambda fp, bid, loop: registry.call(
            "detect_bpm_async",
            fp,
            bid,
            loop,
            {
                "update_button_label": registry.get("update_button_label"),
                "save_config_async": registry.get("save_config_async"),
            },
        ),
        "PyoLoop_class": registry.get("PyoLoop_class"),
    }

    stem_callbacks = {
        "update_stem_gains": registry.get("update_stem_gains"),
        "update_stem_buttons_state": registry.get("update_stem_buttons_state"),
        "save_config_async": registry.get("save_config_async"),
        "initialize_stem_players": registry.get("initialize_stem_players"),
    }

    # ============== CONTEXT MENU ==============
    context_menu_callbacks = {
        "load_loop": registry.get("load_loop"),
        "unload_loop": registry.get("unload_loop"),
        "detect_bpm": registry.get("detect_bpm"),
        "set_bpm_manually": registry.get("set_bpm_manually"),
        "WaveformEditor": registry.get("WaveformEditor"),
        "set_volume": registry.get("set_volume"),
        "generate_stems": registry.get("generate_stems"),
        "delete_stems": registry.get("delete_stems"),
        "update_button_label": registry.get("update_button_label"),
        "update_stem_buttons_state": registry.get("update_stem_buttons_state"),
        "update_stem_eq": registry.get("update_stem_eq"),
        "save_config_async": registry.get("save_config_async"),
        "loop_callbacks": loop_callbacks,
    }
    context_menu = LoopContextMenu(context_menu_callbacks)

    # ============== CREATE WIDGETS ==============

    # 1. Loop Grid Widget
    loop_grid_callbacks = {
        "trigger_loop": registry.get("trigger_loop"),
        "stop_loop": registry.get("stop_loop"),
        "open_context_menu": context_menu.show,
        "switch_bank": registry.get("switch_bank"),
        "select_stems_button": registry.get("select_stems_button"),
        "update_stem_buttons_state": registry.get("update_stem_buttons_state"),
        "loop_callbacks": loop_callbacks,
    }
    loop_grid = LoopGridWidget(root, loop_grid_callbacks)
    loop_grid.grid(row=0, column=0, sticky="nw", padx=10, pady=10)

    # 2. Toolbar Widget
    toolbar_callbacks = {
        "on_speed_change": registry.get("on_speed_change"),
        "reset_pitch": registry.get("reset_pitch"),
        "adjust_bpm_by_delta": registry.get("adjust_bpm_by_delta"),
        "toggle_key_lock": registry.get("toggle_key_lock"),
        "toggle_bpm_lock": registry.get("toggle_bpm_lock"),
        "validate_bpm_entry": registry.get("validate_bpm_entry"),
        "update_speed_from_master_bpm": registry.get("update_speed_from_master_bpm"),
        "on_master_volume_change": registry.get("on_master_volume_change"),
        "reset_master_volume": registry.get("reset_master_volume"),
        "toggle_multi_loop": registry.get("toggle_multi_loop"),
        "apply_stem_mix": registry.get("apply_stem_mix"),
        "invalidate_stem_caches": registry.get("invalidate_stem_caches"),
        "update_button_label": registry.get("update_button_label"),
    }
    toolbar = ToolbarWidget(root, toolbar_callbacks)
    widgets = toolbar.get_widgets()

    # 3. Stems Panel Widget
    stems_panel_callbacks = {
        "on_stem_toggle": registry.get("on_stem_toggle"),
        "on_stem_momentary_activate": registry.get("on_stem_momentary_activate"),
        "on_stem_momentary_release": registry.get("on_stem_momentary_release"),
        "on_stop_stem_toggle": registry.get("on_stop_stem_toggle"),
        "on_stop_stem_momentary": registry.get("on_stop_stem_momentary"),
        "on_stop_stem_momentary_release": registry.get("on_stop_stem_momentary_release"),
        "get_active_loop_with_stems": registry.get("get_active_loop_with_stems"),
        "update_stem_gains": registry.get("update_stem_gains"),
        "stem_callbacks": stem_callbacks,
    }
    stems_panel = StemsPanelWidget(root, stems_panel_callbacks)
    stems_panel.place(relx=0.02, rely=0.95, anchor="sw", x=370)

    # Update stems_panel_ref für update_stem_buttons_state_impl
    stems_panel_ref[0] = stems_panel

    # ============== STARTUP ==============

    # 4. Config laden
    config_manager = ConfigManager.instance()
    config_manager.load(register_loaded_loop, registry.get("PyoLoop_class"))

    # 5. UI aktualisieren
    registry.call("update_all_button_labels")
    registry.call("update_all_stem_indicators")
    registry.call("update_bpm_display", widgets.get("bpm_display"))
    start_gui_queue_processor(root)
    toolbar.update_reset_button_style()

    # 6. Master Volume auf gespeicherten Wert setzen
    if toolbar.master_volume_slider:
        toolbar.master_volume_slider.set(master_volume.get())

    # 7. Shutdown-Handler registrieren
    root.protocol("WM_DELETE_WINDOW", lambda: on_closing(engine))

    # 8. Mainloop starten
    root.mainloop()


if __name__ == "__main__":
    main()
