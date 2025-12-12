"""Loop control for flitzis_looper.

Handles loop triggering, stopping, loading, and unloading.
"""

import builtins
import contextlib
import logging
import os
import shutil
from tkinter import filedialog, messagebox

from pyo import Sig

from flitzis_looper.core.state import (
    COLOR_BTN_ACTIVE,
    COLOR_BTN_INACTIVE,
    COLOR_TEXT,
    COLOR_TEXT_ACTIVE,
    STEM_NAMES,
    ensure_stems_structure,
    get_all_banks_data,
    get_bpm_lock_active,
    get_button_data,
    get_buttons,
    get_current_bank,
    get_default_button_data,
    get_key_lock_active,
    get_loaded_loops,
    get_master_bpm_value,
    get_multi_loop_active,
    get_speed_value,
    register_loaded_loop,
    set_selected_stems_button,
    unregister_loaded_loop,
)
from flitzis_looper.utils.threading import io_executor, schedule_gui_update

logger = logging.getLogger(__name__)


def get_current_original_bpm():
    """Ermittelt die Original-BPM des aktuell aktiven (oder zuletzt geladenen) Loops.

    Returns:
        BPM-Wert oder 120.0 als Fallback
    """
    all_banks_data = get_all_banks_data()
    button_data = get_button_data()
    loaded_loops = get_loaded_loops()

    # OPTIMIERUNG: Zuerst in loaded_loops nach aktiven Loops suchen
    for bank_id, btn_id in loaded_loops:
        data = all_banks_data[bank_id][btn_id]
        if data["active"] and data.get("bpm"):
            return data["bpm"]

    # Fallback: Durch alle Daten iterieren
    loaded_in_bank = [
        (btn_id, data) for btn_id, data in button_data.items() if data["file"] and data.get("bpm")
    ]
    if loaded_in_bank:
        return loaded_in_bank[-1][1]["bpm"]
    return 120.0


def calculate_intro_start(button_id):
    """Berechnet den Intro-Startpunkt basierend auf intro_bars und BPM.

    Args:
        button_id: ID des Buttons

    Returns:
        Der Intro-Startpunkt (geclamped auf 0 wenn negativ)
    """
    button_data = get_button_data()
    data = button_data[button_id]
    bpm = data.get("bpm")
    loop_start = data.get("loop_start", 0.0)
    intro_bars = data.get("intro_bars", 4)

    if not bpm or bpm <= 0:
        return loop_start

    # Bar-Dauer berechnen (4 Beats pro Bar)
    bar_duration = 4.0 / (bpm / 60.0)
    intro_duration = bar_duration * intro_bars

    # Intro-Start berechnen (links vom Loop-Start)
    intro_start = loop_start - intro_duration

    # Clampen auf 0 wenn negativ
    return max(0.0, intro_start)


def trigger_loop(button_id, callbacks):
    """Linksklick - triggert/startet Loop von Anfang (mit optionalem Intro und Stems).

    Args:
        button_id: ID des zu triggernden Buttons
        callbacks: Dict mit Callbacks:
            - update_button_label: Callback zum Aktualisieren des Button-Labels
            - update_stem_buttons_state: Callback zum Aktualisieren der Stem-Buttons
            - update_all_stem_indicators: Callback zum Aktualisieren der Stem-Indikatoren
            - initialize_stem_players: Callback zum Initialisieren der Stem-Player
            - update_stem_gains: Callback zum Aktualisieren der Stem-Gains
            - stop_stem_players: Callback zum Stoppen der Stem-Player
            - _cleanup_stem_players: Callback zum Aufräumen der Stem-Player
    """
    button_data = get_button_data()
    buttons = get_buttons()
    all_banks_data = get_all_banks_data()
    loaded_loops = get_loaded_loops()
    current_bank = get_current_bank()
    multi_loop_active = get_multi_loop_active()
    bpm_lock_active = get_bpm_lock_active()
    key_lock_active = get_key_lock_active()
    speed_value = get_speed_value()
    master_bpm_value = get_master_bpm_value()

    try:
        # Stems-Struktur sicherstellen (behebt KeyErrors bei alten Daten)
        ensure_stems_structure(button_data[button_id])

        loop = button_data[button_id].get("pyo")
        bpm = button_data[button_id].get("bpm")
        if loop and bpm:
            if not multi_loop_active.get():
                # OPTIMIERUNG: Nur durch loaded_loops iterieren statt alle Banks
                for (bank_id, btn_id), other_loop in list(loaded_loops.items()):
                    if not (bank_id == current_bank.get() and btn_id == button_id):
                        data = all_banks_data[bank_id][btn_id]
                        if data["active"]:
                            other_loop.stop()
                            # STEMS: Auch Stem-Player des anderen Loops stoppen
                            callbacks["stop_stem_players"](btn_id)
                            data["stems"]["initialized"] = False
                        data["active"] = False
                        if bank_id == current_bank.get():
                            buttons[btn_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
                            callbacks["update_button_label"](btn_id)

            # Stop if playing to retrigger from start
            is_retrigger = button_data[button_id]["active"]
            stems_available = button_data[button_id]["stems"]["available"]

            if is_retrigger:
                loop.stop()
                # STEMS: Bei Retrigger Stems aufräumen (Tables bleiben erhalten!)
                if button_data[button_id]["stems"].get("initialized"):
                    callbacks["_cleanup_stem_players"](button_id)

            # Key Lock Status setzen (vor set_speed!)
            loop.set_key_lock(key_lock_active.get())

            if bpm_lock_active.get():
                speed = master_bpm_value.get() / bpm
                loop.set_speed(speed)
            else:
                loop.set_speed(speed_value.get())

            # Prüfen ob Intro aktiv ist
            intro_active = button_data[button_id].get("intro_active", False)
            loop_start = button_data[button_id].get("loop_start", 0.0)

            # === STEMS-STRATEGIE ===
            # Wenn Stems verfügbar sind, spielen NUR die Stems das Audio!
            # Der PyoLoop wird NICHT gestartet (verhindert Dopplung)
            if stems_available:
                # WICHTIG: PyoLoop-Objekte (amp, speed) initialisieren BEVOR sie verwendet werden!
                # Ohne das ist loop.amp = None → ArithmeticError bei Multiplikation
                # in initialize_stem_players()
                loop._ensure_player()

                # PyoLoop stumm schalten
                loop.set_stem_mute(Sig(0))

                # Stems ZUERST initialisieren (erstellt Phasor + Player)
                callbacks["initialize_stem_players"](button_id)
                callbacks["update_stem_gains"](button_id)

                # PyoLoop nur für Timing starten (ohne Audio-Output)
                # Das ist nötig für die interne Logik (loop_end, etc.)
                loop._is_playing = True

                # AUTO-SELECT: Dieser Loop wird automatisch für Stem-Kontrolle ausgewählt
                # (als ob man auf das kleine S geklickt hätte)
                set_selected_stems_button(button_id)
                callbacks["update_all_stem_indicators"]()

            # Keine Stems - normales Verhalten
            elif intro_active:
                intro_start = calculate_intro_start(button_id)

                # Nur Intro wenn intro_start < loop_start
                if intro_start < loop_start:
                    loop.play_with_intro(intro_start)
                else:
                    loop.play()
            else:
                loop.play()

            buttons[button_id].config(bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE)
            button_data[button_id]["active"] = True
            callbacks["update_button_label"](button_id)

            # STEMS: Stem-Buttons aktualisieren
            callbacks["update_stem_buttons_state"]()

    except Exception:
        logger.exception("Error triggering loop %s", button_id)


def stop_loop(button_id, callbacks):
    """Rechtsklick - stoppt Loop ODER pre-cached bei gestopptem Loop.

    Wenn der Loop läuft: Stoppt den Loop und alle Stem-Player.
    Wenn der Loop gestoppt ist UND Key Lock aktiv ist:
        Pre-cached das pitch-shifted Audio für latenzfreies Triggern.
        Pre-cached auch Stems falls vorhanden.

    Args:
        button_id: ID des zu stoppenden Buttons
        callbacks: Dict mit Callbacks:
            - update_button_label: Callback zum Aktualisieren des Button-Labels
            - update_stem_buttons_state: Callback zum Aktualisieren der Stem-Buttons
            - stop_stem_players: Callback zum Stoppen der Stem-Player
            - precache_pitched_stems_if_needed: Callback zum Precachen der Stems
    """
    button_data = get_button_data()
    buttons = get_buttons()
    key_lock_active = get_key_lock_active()
    bpm_lock_active = get_bpm_lock_active()
    speed_value = get_speed_value()
    master_bpm_value = get_master_bpm_value()

    try:
        # Stems-Struktur sicherstellen (behebt KeyErrors bei alten Daten)
        ensure_stems_structure(button_data[button_id])

        loop = button_data[button_id].get("pyo")
        if not loop:
            return

        if button_data[button_id]["active"]:
            # Loop läuft -> stoppen
            loop.stop()

            # STEMS: Alle Stem-Player stoppen und aufräumen
            callbacks["stop_stem_players"](button_id)
            button_data[button_id]["stems"]["initialized"] = False

            button_data[button_id]["active"] = False
            buttons[button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
            callbacks["update_button_label"](button_id)
            callbacks["update_stem_buttons_state"]()
        # Loop ist gestoppt -> Pre-Caching wenn Key Lock aktiv
        elif key_lock_active.get() and button_data[button_id].get("file"):
            bpm = button_data[button_id].get("bpm")
            if bpm:
                # Setze Key Lock und Speed bevor wir cachen
                loop.set_key_lock(True)
                if bpm_lock_active.get():
                    speed = master_bpm_value.get() / bpm
                    loop.set_speed(speed)
                else:
                    loop.set_speed(speed_value.get())

                # Visuelles Feedback: Button kurz orange färben
                original_bg = buttons[button_id].cget("bg")
                buttons[button_id].config(bg="#ff8800")

                # Pre-Cache in Background Thread
                def do_precache():
                    loop.precache_pitched_audio()

                    # STEMS: Auch Stems precachen falls vorhanden
                    if button_data[button_id]["stems"]["available"]:
                        callbacks["precache_pitched_stems_if_needed"](button_id)

                    def restore_color():
                        if not button_data[button_id]["active"]:
                            buttons[button_id].config(bg=original_bg)

                    schedule_gui_update(restore_color)

                io_executor.submit(do_precache)
    except Exception:
        logger.exception("Error in stop_loop %s", button_id)


def load_loop(button_id, callbacks):
    """Lädt eine Audio-Datei in einen Button.

    Args:
        button_id: ID des Ziel-Buttons
        callbacks: Dict mit Callbacks:
            - update_button_label: Callback zum Aktualisieren des Button-Labels
            - detect_bpm_async: Callback zur asynchronen BPM-Erkennung
            - PyoLoop_class: Die PyoLoop-Klasse
    """
    button_data = get_button_data()
    buttons = get_buttons()
    current_bank = get_current_bank()
    key_lock_active = get_key_lock_active()
    bpm_lock_active = get_bpm_lock_active()
    speed_value = get_speed_value()
    master_bpm_value = get_master_bpm_value()

    try:
        filepath = filedialog.askopenfilename(filetypes=[("Audio", "*.mp3 *.wav *.flac")])
        if not filepath:
            return
        old_file_path = button_data[button_id].get("file")
        if button_data[button_id]["pyo"]:
            button_data[button_id]["pyo"].stop()
            # OPTIMIERUNG: Loop aus Tracking entfernen
            unregister_loaded_loop(current_bank.get(), button_id)
            button_data[button_id]["pyo"] = None
            button_data[button_id]["active"] = False
            buttons[button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
        buttons[button_id].config(text=f"{button_id}\nLoading")

        def background_load():
            try:
                if old_file_path and os.path.exists(old_file_path):
                    # Datei evtl. in Verwendung oder nicht löschbar
                    with contextlib.suppress(OSError):
                        os.remove(old_file_path)
                original_name = os.path.basename(filepath)
                dest_path = os.path.join("loops", original_name)
                base, ext = os.path.splitext(original_name)
                counter = 1
                while os.path.exists(dest_path):
                    dest_path = os.path.join("loops", f"{base}_{counter}{ext}")
                    counter += 1
                shutil.copy(filepath, dest_path)
                loop = callbacks["PyoLoop_class"]()

                def on_audio_loaded(success):
                    if not success:
                        schedule_gui_update(lambda: messagebox.showerror("Error", "Failed to load"))
                        return

                    def update_button_data():
                        loop.set_gain(0.0)
                        loop.set_key_lock(key_lock_active.get())
                        bpm = button_data[button_id].get("bpm")
                        if bpm and bpm_lock_active.get():
                            loop.set_speed(master_bpm_value.get() / bpm)
                        else:
                            loop.set_speed(speed_value.get())
                        button_data[button_id]["pyo"] = loop
                        button_data[button_id]["file"] = dest_path
                        button_data[button_id]["gain_db"] = 0.0
                        button_data[button_id]["loop_start"] = 0.0
                        button_data[button_id]["loop_end"] = None
                        # OPTIMIERUNG: Waveform-Cache invalidieren bei neuem Audio
                        button_data[button_id]["waveform_cache"] = None
                        # OPTIMIERUNG: Loop registrieren
                        register_loaded_loop(current_bank.get(), button_id, loop)
                        line1 = original_name[:16]
                        line2 = original_name[16:32] if len(original_name) > 16 else ""
                        buttons[button_id].config(text=f"{line1}\n{line2}\nAnalyzing")
                        callbacks["detect_bpm_async"](dest_path, button_id, loop)

                    schedule_gui_update(update_button_data)

                loop.load_async(dest_path, callback=on_audio_loaded)
            except Exception:
                schedule_gui_update(lambda: buttons[button_id].config(text=f"{button_id}"))

        io_executor.submit(background_load)
    except Exception:
        logger.exception("Error loading loop:")


def unload_loop(button_id, callbacks):
    """Entlädt einen Loop und gibt alle Ressourcen frei.

    Args:
        button_id: ID des zu entladenden Buttons
        callbacks: Dict mit Callbacks:
            - update_stem_buttons_state: Callback zum Aktualisieren der Stem-Buttons
            - save_config_async: Callback zum asynchronen Speichern der Config
    """
    button_data = get_button_data()
    buttons = get_buttons()
    current_bank = get_current_bank()

    try:
        file_path = button_data[button_id].get("file")
        loop = button_data[button_id].get("pyo")
        if loop:
            # Cache invalidieren (gibt RAM frei)
            if hasattr(loop, "_invalidate_pitch_cache"):
                loop._invalidate_pitch_cache()
            loop.stop()
            button_data[button_id]["pyo"] = None
            # OPTIMIERUNG: Loop aus Tracking entfernen
            unregister_loaded_loop(current_bank.get(), button_id)

        # STEMS: Stem-Player stoppen und Caches freigeben
        for stem in STEM_NAMES:
            player = button_data[button_id]["stems"]["players"].get(stem)
            if player:
                with contextlib.suppress(builtins.BaseException):
                    player.stop()

        button_data[button_id]["active"] = False
        buttons[button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
        if file_path and os.path.exists(file_path):
            # Datei evtl. in Verwendung oder nicht löschbar
            with contextlib.suppress(OSError):
                os.remove(file_path)
        button_data[button_id] = get_default_button_data()
        buttons[button_id].config(text=f"{button_id}")
        callbacks["update_stem_buttons_state"]()
        callbacks["save_config_async"]()
    except Exception:
        logger.exception("Error unloading")
