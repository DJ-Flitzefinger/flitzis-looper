"""
BPM and speed control for flitzis_looper.
Handles BPM display, speed changes, and lock toggle functions.
"""

import logging
import tkinter as tk

from flitzis_looper.utils.threading import bpm_executor, schedule_gui_update
from flitzis_looper.core.state import (
    get_root, get_button_data, get_all_banks_data,
    get_loaded_loops, get_bpm_lock_active, get_key_lock_active,
    get_speed_value, get_master_bpm_value, get_current_bank,
    get_last_bpm_display, set_last_bpm_display
)
from flitzis_looper.audio.bpm import _detect_bpm_worker

logger = logging.getLogger(__name__)

# OPTIMIERUNG: Tracking ob aktive Loops vorhanden sind
_has_active_loops = False


def detect_bpm(filepath, callback):
    """
    Erkennt BPM einer Audio-Datei asynchron.
    
    Args:
        filepath: Pfad zur Audio-Datei
        callback: Callback-Funktion die mit dem BPM-Wert aufgerufen wird
    """
    def do_detect():
        try:
            bpm = _detect_bpm_worker(filepath)
            schedule_gui_update(lambda b=bpm: callback(b))
        except Exception as e:
            logger.debug(f"BPM detection error: {e}")
            schedule_gui_update(lambda: callback(None))
    bpm_executor.submit(do_detect)


def detect_bpm_async(filepath, button_id, loop, callbacks):
    """
    Erkennt BPM einer Audio-Datei asynchron und aktualisiert die Button-Daten.
    
    Args:
        filepath: Pfad zur Audio-Datei
        button_id: ID des zugehörigen Buttons
        loop: PyoLoop-Objekt
        callbacks: Dict mit Callbacks:
            - update_button_label: Callback zum Aktualisieren des Button-Labels
            - save_config_async: Callback zum asynchronen Speichern der Config
    """
    from tkinter import simpledialog
    
    root = get_root()
    button_data = get_button_data()
    
    def do_detect():
        try:
            bpm = _detect_bpm_worker(filepath)
            schedule_gui_update(lambda: on_bpm_result(bpm))
        except Exception as e:
            logger.debug(f"Async BPM detection error: {e}")
            schedule_gui_update(lambda: on_bpm_result(None))
    
    def on_bpm_result(bpm):
        if bpm is not None:
            button_data[button_id]["bpm"] = bpm
            if button_data[button_id]["auto_loop_active"]:
                try:
                    bar_duration = 4.0 / (bpm / 60.0)
                    auto_loop_duration = bar_duration * button_data[button_id].get("auto_loop_bars", 8)
                    if loop._duration > 0 and auto_loop_duration <= loop._duration:
                        button_data[button_id]["loop_start"] = 0.0
                        button_data[button_id]["loop_end"] = auto_loop_duration
                        loop.loop_start = 0.0
                        loop.loop_end = auto_loop_duration
                except (ZeroDivisionError, AttributeError) as e:
                    logger.debug(f"Auto-loop calculation error: {e}")
            root.after(1000, lambda: callbacks['update_button_label'](button_id))
            root.after(1200, callbacks['save_config_async'])
        else:
            new_bpm = simpledialog.askfloat(
                "BPM Detection Failed",
                f"Could not detect BPM.\nEnter BPM for Button {button_id}:",
                initialvalue=120.0, minvalue=10.0, maxvalue=500.0
            )
            if new_bpm:
                button_data[button_id]["bpm"] = round(new_bpm, 1)
                root.after(1000, lambda: callbacks['update_button_label'](button_id))
                root.after(1200, callbacks['save_config_async'])
            else:
                root.after(1000, lambda: callbacks['update_button_label'](button_id))
    
    bpm_executor.submit(do_detect)


def update_bpm_display_once(bpm_display_widget):
    """
    Aktualisiert die BPM-Anzeige einmalig.
    
    Args:
        bpm_display_widget: Das Label-Widget für die BPM-Anzeige
    """
    global _has_active_loops
    
    all_banks_data = get_all_banks_data()
    loaded_loops = get_loaded_loops()
    bpm_lock_active = get_bpm_lock_active()
    speed_value = get_speed_value()
    master_bpm_value = get_master_bpm_value()
    last_bpm_display = get_last_bpm_display()
    
    # OPTIMIERUNG: Zuerst in loaded_loops nach aktiven Loops suchen
    active_loop_data = None
    for (bank_id, btn_id), loop in loaded_loops.items():
        data = all_banks_data[bank_id][btn_id]
        if data["active"] and data["bpm"]:
            active_loop_data = data
            break
    
    if active_loop_data:
        _has_active_loops = True
        bpm = active_loop_data["bpm"]
        if bpm_lock_active.get():
            try:
                display = master_bpm_value.get()
            except (tk.TclError, ValueError):
                display = 120.0
        else:
            display = bpm * speed_value.get()
        new_text = f"{display:.2f}"
    else:
        _has_active_loops = False
        new_text = "------"
    
    if new_text != last_bpm_display:
        bpm_display_widget.config(text=new_text)
        set_last_bpm_display(new_text)


def update_bpm_display(bpm_display_widget):
    """
    Aktualisiert die BPM-Anzeige kontinuierlich.
    
    Args:
        bpm_display_widget: Das Label-Widget für die BPM-Anzeige
    """
    root = get_root()
    
    update_bpm_display_once(bpm_display_widget)
    # OPTIMIERUNG: Seltener updaten wenn nichts aktiv ist
    if _has_active_loops:
        root.after(100, lambda: update_bpm_display(bpm_display_widget))
    else:
        root.after(500, lambda: update_bpm_display(bpm_display_widget))  # Langsameres Update wenn nichts spielt


def on_speed_change(val, widgets, callbacks):
    """
    Handler für Speed-Slider-Änderungen.
    
    Args:
        val: Neuer Speed-Wert
        widgets: Dict mit Widgets (update_reset_button_style wird dort erwartet)
        callbacks: Dict mit Callbacks:
            - apply_stem_mix: Callback zum Anwenden des Stem-Mixes
            - invalidate_stem_caches: Callback zum Invalidieren der Stem-Caches
    """
    all_banks_data = get_all_banks_data()
    loaded_loops = get_loaded_loops()
    bpm_lock_active = get_bpm_lock_active()
    speed_value = get_speed_value()
    master_bpm_value = get_master_bpm_value()
    
    value = round(float(val), 2)
    speed_value.set(value)
    
    if bpm_lock_active.get():
        from flitzis_looper.core.loops import get_current_original_bpm
        current_bpm = get_current_original_bpm()
        if current_bpm > 0:
            bpm = value * current_bpm
            master_bpm_value.set(round(bpm, 3))
    
    # OPTIMIERUNG: Nur durch loaded_loops iterieren statt alle Banks
    for (bank_id, btn_id), loop in loaded_loops.items():
        data = all_banks_data[bank_id][btn_id]
        bpm = data.get("bpm")
        if bpm:
            try:
                if bpm_lock_active.get():
                    new_speed = float(master_bpm_value.get()) / float(bpm)
                    loop.set_speed(new_speed)
                else:
                    new_speed = float(speed_value.get())
                    loop.set_speed(new_speed)
                
                # STEMS: Bei Speed-Änderung Caches invalidieren und ggf. neu aufbauen
                if data["stems"]["available"]:
                    if data["stems"]["cached_speed"] != new_speed:
                        callbacks['invalidate_stem_caches'](btn_id)
                        # Bei aktivem Stem-Mix: Player neu erstellen
                        if data["active"] and any(data["stems"]["states"].values()):
                            callbacks['apply_stem_mix'](btn_id)
                            
            except (ValueError, ZeroDivisionError, AttributeError):
                pass  # Ungültige Werte ignorieren
    
    update_bpm_display_once(widgets['bpm_display'])
    
    # Reset-Button Stil aktualisieren
    try:
        widgets['update_reset_button_style']()
    except (NameError, KeyError):
        pass  # Button existiert noch nicht beim Start


def reset_pitch(widgets):
    """
    Setzt den Pitch/Speed auf 1.0 zurück.
    
    Args:
        widgets: Dict mit Widgets:
            - speed_slider: Der Speed-Slider
    """
    speed_value = get_speed_value()
    speed_value.set(1.0)
    widgets['speed_slider'].set(1.0)


def adjust_bpm_by_delta(delta_bpm, widgets, callbacks):
    """
    Passt die BPM um einen festen Wert an.
    Nutzt die gleiche Logik wie die manuelle BPM-Eingabe.
    
    Args:
        delta_bpm: positive Werte = schneller, negative = langsamer
        widgets: Dict mit Widgets
        callbacks: Dict mit Callbacks
    """
    all_banks_data = get_all_banks_data()
    loaded_loops = get_loaded_loops()
    bpm_lock_active = get_bpm_lock_active()
    speed_value = get_speed_value()
    master_bpm_value = get_master_bpm_value()
    
    try:
        # OPTIMIERUNG: Zuerst in loaded_loops nach aktivem Loop suchen
        active_loop_bpm = None
        for (bank_id, btn_id), loop in loaded_loops.items():
            data = all_banks_data[bank_id][btn_id]
            if data["active"] and data["bpm"]:
                active_loop_bpm = data["bpm"]
                break
        
        if not active_loop_bpm or active_loop_bpm <= 0:
            return
        
        # Aktuelle BPM berechnen (Original-BPM * aktueller Speed)
        current_speed = speed_value.get()
        current_bpm = active_loop_bpm * current_speed
        
        # Neue Ziel-BPM
        new_bpm = current_bpm + delta_bpm
        
        # Begrenzen auf sinnvollen Bereich
        if not 10 <= new_bpm <= 500:
            return
        
        # Neuen Speed berechnen (wie in update_speed_from_master_bpm)
        required_speed = new_bpm / active_loop_bpm
        
        # Begrenzen auf Slider-Bereich
        required_speed = max(0.5, min(2.0, required_speed))
        
        # Speed setzen (OHNE on_speed_change zu triggern)
        speed_value.set(required_speed)
        
        # OPTIMIERUNG: Nur loaded_loops aktualisieren
        for (bank_id, btn_id), loop in loaded_loops.items():
            loop.set_speed(required_speed)
        
        # BPM-Display und Master-BPM-Feld aktualisieren
        if bpm_lock_active.get():
            master_bpm_value.set(round(new_bpm, 3))
        
        update_bpm_display_once(widgets['bpm_display'])
        
        # Slider visuell aktualisieren (triggert on_speed_change)
        widgets['speed_slider'].set(required_speed)
        
        # Reset-Button Farbe IMMER am Ende aktualisieren
        widgets['update_reset_button_style']()
    except Exception as e:
        logger.error(f"Error adjusting BPM: {e}")


def toggle_key_lock(key_lock_btn):
    """
    Toggle Key Lock (Master Tempo) für alle Loops - OHNE Unterbrechung.
    
    Args:
        key_lock_btn: Der Key Lock Button-Widget
    """
    from flitzis_looper.core.state import COLOR_LOCK_OFF, COLOR_LOCK_ON, COLOR_TEXT, COLOR_TEXT_ACTIVE
    
    key_lock_active = get_key_lock_active()
    loaded_loops = get_loaded_loops()
    
    key_lock_active.set(not key_lock_active.get())
    
    if key_lock_active.get():
        key_lock_btn.config(bg=COLOR_LOCK_ON, fg=COLOR_TEXT_ACTIVE)
    else:
        key_lock_btn.config(bg=COLOR_LOCK_OFF, fg=COLOR_TEXT)
    
    # OPTIMIERUNG: Nur loaded_loops aktualisieren statt alle Banks
    for (bank_id, btn_id), loop in loaded_loops.items():
        loop.set_key_lock(key_lock_active.get())


def toggle_bpm_lock(bpm_lock_btn):
    """
    Toggle BPM Lock.
    
    Args:
        bpm_lock_btn: Der BPM Lock Button-Widget
    """
    from flitzis_looper.core.state import COLOR_LOCK_OFF, COLOR_LOCK_ON, COLOR_TEXT, COLOR_TEXT_ACTIVE
    
    all_banks_data = get_all_banks_data()
    loaded_loops = get_loaded_loops()
    bpm_lock_active = get_bpm_lock_active()
    speed_value = get_speed_value()
    master_bpm_value = get_master_bpm_value()
    
    bpm_lock_active.set(not bpm_lock_active.get())
    
    if bpm_lock_active.get():
        bpm_lock_btn.config(bg=COLOR_LOCK_ON, fg=COLOR_TEXT_ACTIVE)
    else:
        bpm_lock_btn.config(bg=COLOR_LOCK_OFF, fg=COLOR_TEXT)
    
    # OPTIMIERUNG: Zuerst in loaded_loops nach aktivem Loop suchen
    active_loop_data = None
    for (bank_id, btn_id), loop in loaded_loops.items():
        data = all_banks_data[bank_id][btn_id]
        if data["active"] and data["bpm"]:
            active_loop_data = data
            break
    
    if bpm_lock_active.get() and active_loop_data:
        current_track_bpm = active_loop_data["bpm"]
        current_speed = speed_value.get()
        master_bpm_value.set(round(current_track_bpm * current_speed, 3))


def update_speed_from_master_bpm(bpm_entry, widgets):
    """
    Aktualisiert den Speed basierend auf der Master-BPM-Eingabe.
    
    Args:
        bpm_entry: Das BPM-Entry-Widget
        widgets: Dict mit Widgets
    """
    root = get_root()
    all_banks_data = get_all_banks_data()
    loaded_loops = get_loaded_loops()
    speed_value = get_speed_value()
    
    try:
        entry_text = bpm_entry.get().strip()
        if not entry_text or entry_text == "0.000":
            root.focus_set()  # Fokus vom Entry-Feld entfernen
            return
        bpm = float(entry_text.replace(",", "."))
        if not 10 <= bpm <= 500:
            root.focus_set()
            return
        
        # OPTIMIERUNG: Zuerst in loaded_loops nach aktivem Loop suchen
        active_loop_bpm = None
        for (bank_id, btn_id), loop in loaded_loops.items():
            data = all_banks_data[bank_id][btn_id]
            if data["active"] and data["bpm"]:
                active_loop_bpm = data["bpm"]
                break
        
        if active_loop_bpm and active_loop_bpm > 0:
            required_speed = bpm / active_loop_bpm
            speed_value.set(required_speed)
            widgets['speed_slider'].set(max(0.5, min(2.0, required_speed)))
            # OPTIMIERUNG: Nur loaded_loops aktualisieren
            for (bank_id, btn_id), loop in loaded_loops.items():
                loop.set_speed(required_speed)
            widgets['update_reset_button_style']()
        
        root.focus_set()  # Fokus vom Entry-Feld entfernen
    except (ValueError, ZeroDivisionError, tk.TclError):
        root.focus_set()  # Auch bei Fehler Fokus entfernen


def validate_bpm_entry(new_value):
    """
    Validiert Eingaben im BPM-Eingabefeld.
    Erlaubt nur Zahlen, Punkt und Komma.
    
    Args:
        new_value: Der neue Eingabewert
        
    Returns:
        True wenn gültig, False sonst
    """
    if new_value == "":
        return True
    # Erlaube Zahlen, einen Punkt oder ein Komma
    for char in new_value:
        if char not in "0123456789.,":
            return False
    # Maximal ein Dezimaltrennzeichen
    if new_value.count(".") + new_value.count(",") > 1:
        return False
    return True
