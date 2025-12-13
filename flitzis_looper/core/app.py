"""Main application module for flitzis_looper.

Orchestrates the application startup, shutdown, and coordinates all modules.
"""

import builtins
import contextlib
import logging
import threading
import tkinter as tk
from tkinter import messagebox

import matplotlib as mpl

from flitzis_looper.audio.engine import init_engine
from flitzis_looper.audio.pitch import cleanup_stem_caches
from flitzis_looper.core.config import save_config_immediate
from flitzis_looper.core.state import (
    COLOR_BG,
    COLOR_TEXT,
    STEM_NAMES,
    get_all_banks_data,
    get_loaded_loops,
    get_root,
    init_banks,
    init_state,
)
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
                    # Final Output stoppen
                    if data["stems"].get("final_output"):
                        data["stems"]["final_output"].stop()
                    # Mix und EQ stoppen
                    for obj_name in ["stem_mix", "eq_low", "eq_mid", "eq_high"]:
                        if data["stems"].get(obj_name):
                            with contextlib.suppress(builtins.BaseException):
                                data["stems"][obj_name].stop()
                    # Master Phasor stoppen
                    if data["stems"].get("master_phasor"):
                        data["stems"]["master_phasor"].stop()
                    # Main Player stoppen
                    if data["stems"].get("main_player"):
                        data["stems"]["main_player"].stop()
                    # Stem Players stoppen
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
        save_config_immediate()

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
    from flitzis_looper.core.state import set_master_amp as state_set_master_amp

    state_set_master_amp(engine.master_amp)

    # 5. Banks initialisieren
    init_banks()

    # ============== IMPORTS ==============
    from flitzis_looper.audio.loop import PyoLoop
    from flitzis_looper.audio.pitch import invalidate_stem_caches, precache_pitched_stems_if_needed
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
        detect_bpm_async,
        on_speed_change,
        reset_pitch,
        toggle_bpm_lock,
        toggle_key_lock,
        update_bpm_display,
        update_speed_from_master_bpm,
        validate_bpm_entry,
    )
    from flitzis_looper.core.config import load_config, save_config_async
    from flitzis_looper.core.loops import load_loop, stop_loop, trigger_loop, unload_loop
    from flitzis_looper.core.state import (
        get_button_data,
        get_master_volume,
        register_loaded_loop,
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
    from flitzis_looper.ui.dialogs import WaveformEditor, set_bpm_manually, set_volume
    from flitzis_looper.ui.widgets import LoopGridWidget, StemsPanelWidget, ToolbarWidget

    # Data-Referenzen
    button_data = get_button_data()
    master_volume = get_master_volume()

    # ============== FORWARD DECLARATIONS ==============
    # Werden später durch Widget-Methoden ersetzt
    stems_panel: StemsPanelWidget | None = None

    def update_stem_buttons_state_impl():
        """Wrapper für Stem-Button-Update."""
        if stems_panel:
            stems_panel.update_state()

    # ============== CALLBACK DICTIONARIES ==============
    loop_callbacks = {
        "update_button_label": update_button_label,
        "update_stem_buttons_state": update_stem_buttons_state_impl,
        "update_all_stem_indicators": update_all_stem_indicators,
        "initialize_stem_players": initialize_stem_players,
        "update_stem_gains": update_stem_gains,
        "stop_stem_players": stop_stem_players,
        "_cleanup_stem_players": _cleanup_stem_players,
        "precache_pitched_stems_if_needed": precache_pitched_stems_if_needed,
        "save_config_async": save_config_async,
        "detect_bpm_async": lambda fp, bid, loop: detect_bpm_async(
            fp,
            bid,
            loop,
            {"update_button_label": update_button_label, "save_config_async": save_config_async},
        ),
        "PyoLoop_class": PyoLoop,
    }

    stem_callbacks = {
        "update_stem_gains": update_stem_gains,
        "update_stem_buttons_state": update_stem_buttons_state_impl,
        "save_config_async": save_config_async,
        "initialize_stem_players": initialize_stem_players,
    }

    # ============== CONTEXT MENU ==============
    def open_context_menu(button_id, event):
        """Middle click - open context menu."""
        from flitzis_looper.core.state import ensure_stems_structure

        ensure_stems_structure(button_data[button_id])

        menu = tk.Menu(root, tearoff=0, bg=COLOR_BG, fg=COLOR_TEXT)
        menu.add_command(label="Load Audio", command=lambda: load_loop(button_id, loop_callbacks))

        if button_data[button_id]["file"]:
            menu.add_command(
                label="Unload Audio", command=lambda: unload_loop(button_id, loop_callbacks)
            )
            menu.add_separator()

            from flitzis_looper.core.bpm_control import detect_bpm

            menu.add_command(
                label="Re-detect BPM",
                command=lambda: detect_bpm(
                    button_data[button_id]["file"],
                    lambda bpm: [
                        button_data[button_id].update({"bpm": bpm}),
                        update_button_label(button_id),
                    ],
                ),
            )

            def open_bpm_dialog():
                set_bpm_manually(button_id, update_button_label, save_config_async)

            menu.add_command(label="Set BPM manually", command=open_bpm_dialog)

            def open_loop_editor():
                if not button_data[button_id]["file"]:
                    messagebox.showwarning("No Audio", "Load audio first.")
                    return
                WaveformEditor(root, button_id, update_button_label, save_config_async)

            menu.add_command(label="Adjust Loop", command=open_loop_editor)

            def open_volume_dialog():
                set_volume(button_id, update_stem_eq, save_config_async)

            menu.add_command(label="Volume + EQ", command=open_volume_dialog)

            # STEMS
            menu.add_separator()
            if button_data[button_id]["bpm"]:
                if button_data[button_id]["stems"]["generating"]:
                    menu.add_command(label="⏳ Generating Stems", state="disabled")
                elif button_data[button_id]["stems"]["available"]:
                    menu.add_command(label="✓ Stems Available", state="disabled")
                    menu.add_command(
                        label="Regenerate Stems",
                        command=lambda: generate_stems(
                            button_id,
                            update_button_label,
                            update_stem_buttons_state_impl,
                            save_config_async,
                        ),
                    )
                    menu.add_command(
                        label="Delete Stems",
                        command=lambda: delete_stems(
                            button_id,
                            update_button_label,
                            update_stem_buttons_state_impl,
                            save_config_async,
                        ),
                    )
                else:
                    menu.add_command(
                        label="Generate Stems",
                        command=lambda: generate_stems(
                            button_id,
                            update_button_label,
                            update_stem_buttons_state_impl,
                            save_config_async,
                        ),
                    )
            else:
                menu.add_command(label="Generate Stems (set BPM first)", state="disabled")

        menu.post(event.x_root, event.y_root)

    # ============== CREATE WIDGETS ==============

    # 1. Loop Grid Widget (36 Buttons + Bank-Buttons)
    loop_grid_callbacks = {
        "trigger_loop": trigger_loop,
        "stop_loop": stop_loop,
        "open_context_menu": open_context_menu,
        "switch_bank": switch_bank,
        "select_stems_button": select_stems_button,
        "update_stem_buttons_state": update_stem_buttons_state_impl,
        "loop_callbacks": loop_callbacks,
    }

    loop_grid = LoopGridWidget(root, loop_grid_callbacks)
    loop_grid.grid(row=0, column=0, sticky="nw", padx=10, pady=10)

    # 2. Toolbar Widget (BPM, Speed, Volume, Lock-Buttons)
    toolbar_callbacks = {
        "on_speed_change": on_speed_change,
        "reset_pitch": reset_pitch,
        "adjust_bpm_by_delta": adjust_bpm_by_delta,
        "toggle_key_lock": toggle_key_lock,
        "toggle_bpm_lock": toggle_bpm_lock,
        "validate_bpm_entry": validate_bpm_entry,
        "update_speed_from_master_bpm": update_speed_from_master_bpm,
        "on_master_volume_change": on_master_volume_change,
        "reset_master_volume": reset_master_volume,
        "toggle_multi_loop": toggle_multi_loop,
        "apply_stem_mix": apply_stem_mix,
        "invalidate_stem_caches": invalidate_stem_caches,
        "update_button_label": update_button_label,
    }

    toolbar = ToolbarWidget(root, toolbar_callbacks)
    widgets = toolbar.get_widgets()

    # 3. Stems Panel Widget
    stems_panel_callbacks = {
        "on_stem_toggle": on_stem_toggle,
        "on_stem_momentary_activate": on_stem_momentary_activate,
        "on_stem_momentary_release": on_stem_momentary_release,
        "on_stop_stem_toggle": on_stop_stem_toggle,
        "on_stop_stem_momentary": on_stop_stem_momentary,
        "on_stop_stem_momentary_release": on_stop_stem_momentary_release,
        "get_active_loop_with_stems": get_active_loop_with_stems,
        "update_stem_gains": update_stem_gains,
        "stem_callbacks": stem_callbacks,
    }

    stems_panel = StemsPanelWidget(root, stems_panel_callbacks)
    stems_panel.place(relx=0.02, rely=0.95, anchor="sw", x=370)

    # ============== STARTUP ==============

    # 6. Config laden
    load_config(register_loaded_loop, PyoLoop)

    # 7. UI aktualisieren
    update_all_button_labels()
    update_all_stem_indicators()
    update_bpm_display(widgets.get("bpm_display"))
    start_gui_queue_processor(root)
    toolbar.update_reset_button_style()

    # Master Volume auf gespeicherten Wert setzen
    if toolbar.master_volume_slider:
        toolbar.master_volume_slider.set(master_volume.get())

    # 8. Shutdown-Handler registrieren
    root.protocol("WM_DELETE_WINDOW", lambda: on_closing(engine))

    # 9. Mainloop starten
    root.mainloop()


if __name__ == "__main__":
    main()
