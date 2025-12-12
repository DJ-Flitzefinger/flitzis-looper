"""
flitzis_looper.ui.dialogs.bpm_dialog - BPM Dialog.

Enthält set_bpm_manually() Funktion für manuelle BPM-Eingabe + TAP BPM.
"""

import tkinter as tk
from tkinter import messagebox
import time

from flitzis_looper.core.state import (
    get_root, get_button_data,
    COLOR_BG, COLOR_TEXT, COLOR_TEXT_ACTIVE, COLOR_LOCK_OFF, COLOR_LOCK_ON,
)


def set_bpm_manually(button_id, update_button_label_callback=None, save_config_async_callback=None):
    """
    Öffnet einen Dialog zum manuellen Setzen der BPM.
    Enthält sowohl ein Eingabefeld als auch einen TAP BPM Button.
    
    Args:
        button_id: ID des Buttons
        update_button_label_callback: Callback für Label-Updates
        save_config_async_callback: Callback für Config-Speicherung
    """
    root = get_root()
    button_data = get_button_data()
    
    current = button_data[button_id].get("bpm", 120.0) or 120.0
    
    # Custom Dialog erstellen
    dialog = tk.Toplevel(root)
    dialog.title("Set BPM")
    dialog.configure(bg=COLOR_BG)
    dialog.transient(root)
    dialog.grab_set()
    
    # Fenstergröße fixieren und nicht veränderbar machen
    dialog.resizable(False, False)
    
    # TAP BPM Tracking Variablen
    tap_times = []
    last_tap_time = [0.0]  # Liste als Container für Mutable-Referenz
    
    # Frame für Inhalt
    content_frame = tk.Frame(dialog, bg=COLOR_BG, padx=20, pady=15)
    content_frame.pack()
    
    # Label
    label = tk.Label(content_frame, text="Enter BPM:", fg=COLOR_TEXT, bg=COLOR_BG, 
                     font=("Arial", 11))
    label.pack(pady=(0, 8))
    
    # Entry mit Validierung (gleiche Logik wie beim Master BPM Eingabefeld)
    bpm_var = tk.StringVar(value=str(current))
    
    def validate_bpm_input(new_value):
        """Erlaubt nur Zahlen, Punkt und Komma im BPM-Eingabefeld"""
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
    
    validate_cmd = dialog.register(validate_bpm_input)
    entry = tk.Entry(content_frame, textvariable=bpm_var, width=12, justify="center",
                     bg="#333", fg=COLOR_TEXT, font=("Arial", 14, "bold"),
                     validate="key", validatecommand=(validate_cmd, "%P"),
                     insertbackground=COLOR_TEXT)
    entry.pack(pady=(0, 15))
    entry.select_range(0, tk.END)
    entry.focus_set()
    
    # Frame für OK und Cancel Buttons
    button_frame = tk.Frame(content_frame, bg=COLOR_BG)
    button_frame.pack(pady=(0, 10))
    
    result = [None]  # Container für Ergebnis
    
    def on_ok():
        try:
            value = bpm_var.get().replace(",", ".")
            bpm = float(value)
            if 10.0 <= bpm <= 500.0:
                result[0] = round(bpm, 1)
                dialog.destroy()
            else:
                messagebox.showwarning("Invalid BPM", "BPM must be between 10 and 500.", parent=dialog)
        except ValueError:
            messagebox.showwarning("Invalid Input", "Please enter a valid number.", parent=dialog)
    
    def on_cancel():
        dialog.destroy()
    
    # Button-Breite so dass beide zusammen die TAP-Button-Breite ergeben
    btn_width = 8
    
    ok_btn = tk.Button(button_frame, text="OK", command=on_ok, width=btn_width,
                       bg="#555", fg=COLOR_TEXT, activebackground="#666",
                       activeforeground=COLOR_TEXT, font=("Arial", 10, "bold"))
    ok_btn.pack(side="left", padx=(0, 5))
    
    cancel_btn = tk.Button(button_frame, text="Cancel", command=on_cancel, width=btn_width,
                           bg="#555", fg=COLOR_TEXT, activebackground="#666",
                           activeforeground=COLOR_TEXT, font=("Arial", 10, "bold"))
    cancel_btn.pack(side="left", padx=(5, 0))
    
    # TAP BPM Button - quadratisch, so breit wie OK + Cancel zusammen
    # Berechne die Breite basierend auf den Button-Breiten
    tap_btn_size = 140  # Pixel-Größe für quadratischen Button
    
    tap_frame = tk.Frame(content_frame, bg=COLOR_BG)
    tap_frame.pack(pady=(5, 0))
    
    tap_btn = tk.Button(tap_frame, text="TAP BPM", 
                        font=("Arial", 14, "bold"),
                        bg=COLOR_LOCK_OFF, fg=COLOR_TEXT,
                        activebackground=COLOR_LOCK_ON,
                        activeforeground=COLOR_TEXT_ACTIVE,
                        width=12, height=4)
    tap_btn.pack()
    
    def on_tap_press(event):
        """Wird aufgerufen wenn TAP Button gedrückt wird"""
        nonlocal tap_times
        current_time = time.time()
        
        # Button grün färben während gedrückt
        tap_btn.configure(bg=COLOR_LOCK_ON, fg=COLOR_TEXT_ACTIVE)
        
        # Prüfe ob mehr als 5 Sekunden seit letztem Tap vergangen sind
        if last_tap_time[0] > 0 and (current_time - last_tap_time[0]) > 5.0:
            # Reset - von vorne beginnen
            tap_times = []
        
        # Aktuelle Zeit hinzufügen
        tap_times.append(current_time)
        last_tap_time[0] = current_time
        
        # BPM berechnen wenn mindestens 2 Taps vorhanden
        if len(tap_times) >= 2:
            # Berechne alle Intervalle zwischen den Taps
            intervals = []
            for i in range(1, len(tap_times)):
                intervals.append(tap_times[i] - tap_times[i-1])
            
            # Mittelwert der Intervalle
            avg_interval = sum(intervals) / len(intervals)
            
            # BPM berechnen: 60 Sekunden / durchschnittliches Intervall
            if avg_interval > 0:
                calculated_bpm = 60.0 / avg_interval
                # Auf vernünftigen Bereich begrenzen
                calculated_bpm = max(10.0, min(500.0, calculated_bpm))
                # In Entry eintragen
                bpm_var.set(f"{calculated_bpm:.1f}")
    
    def on_tap_release(event):
        """Wird aufgerufen wenn TAP Button losgelassen wird"""
        # Button zurück auf rot
        tap_btn.configure(bg=COLOR_LOCK_OFF, fg=COLOR_TEXT)
    
    # Bind Events für Press und Release
    tap_btn.bind("<ButtonPress-1>", on_tap_press)
    tap_btn.bind("<ButtonRelease-1>", on_tap_release)
    
    # Enter-Taste für OK
    entry.bind("<Return>", lambda e: on_ok())
    # Escape für Cancel
    dialog.bind("<Escape>", lambda e: on_cancel())
    
    # Dialog zentrieren auf Hauptfenster
    dialog.update_idletasks()
    x = root.winfo_x() + (root.winfo_width() // 2) - (dialog.winfo_width() // 2)
    y = root.winfo_y() + (root.winfo_height() // 2) - (dialog.winfo_height() // 2)
    dialog.geometry(f"+{x}+{y}")
    
    # Warten bis Dialog geschlossen wird
    dialog.wait_window()
    
    # Ergebnis verarbeiten
    if result[0] is not None:
        button_data[button_id]["bpm"] = result[0]
        if update_button_label_callback:
            update_button_label_callback(button_id)
        if save_config_async_callback:
            save_config_async_callback()
