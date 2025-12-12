"""
flitzis_looper.ui.stems_panel - Stem Toggle Buttons.

Enthält:
- create_stems_panel(): Erstellt die Stem-Buttons (V, M, B, D, I) und Stop-Stem
- update_stem_buttons_state(): Aktualisiert alle Stem-Button-Farben
- update_stop_stem_button_state(): Aktualisiert Stop-Stem Button
"""

import tkinter as tk

from flitzis_looper.core.state import (
    get_root, get_button_data, get_selected_stems_button,
    STEM_NAMES, STEM_LABELS,
    COLOR_BG, COLOR_TEXT, COLOR_TEXT_ACTIVE,
    COLOR_BTN_INACTIVE, COLOR_BTN_ACTIVE,
)


# Module-level references to widgets
_stem_buttons = {}  # {stem_name: canvas}
_stop_stem_button = None


def get_active_loop_with_stems():
    """Gibt den aktiven Loop mit verfügbaren Stems zurück, oder None."""
    button_data = get_button_data()
    for btn_id, data in button_data.items():
        if data["active"] and data["stems"]["available"]:
            return btn_id
    return None


def get_selected_or_active_stems_button():
    """Zentraler Helper für Stem-bezogene Aktionen.

    Bevorzugt den explizit selektierten Button (selected_stems_button),
    fällt sonst auf den ersten aktiven Loop mit verfügbaren Stems zurück.
    Gibt None zurück, wenn kein solcher Button existiert.
    """
    selected_stems_button = get_selected_stems_button()
    target_button_id = selected_stems_button
    if target_button_id is not None:
        return target_button_id
    return get_active_loop_with_stems()


def create_stems_panel(
    on_stem_toggle_callback,
    on_stem_momentary_activate_callback,
    on_stem_momentary_release_callback,
    on_stop_stem_toggle_callback,
    on_stop_stem_momentary_callback,
    on_stop_stem_momentary_release_callback,
    initialize_stem_players_callback,
):
    """
    Erstellt das Stems-Panel mit Toggle-Buttons.
    
    Args:
        on_stem_toggle_callback: Callback für Stem-Toggle (Linksklick)
        on_stem_momentary_activate_callback: Callback für temporäres Aktivieren
        on_stem_momentary_release_callback: Callback für Loslassen temporärer Aktivierung
        on_stop_stem_toggle_callback: Callback für Stop-Stem Toggle
        on_stop_stem_momentary_callback: Callback für temporäres Stop-Stem
        on_stop_stem_momentary_release_callback: Callback für Loslassen Stop-Stem
        initialize_stem_players_callback: Callback für Stem-Player-Initialisierung
    
    Returns:
        dict: Dictionary mit Widget-Referenzen
    """
    global _stem_buttons, _stop_stem_button
    
    root = get_root()
    button_data = get_button_data()
    
    # STEMS: Runde Toggle-Buttons für Stem-Separation
    stems_frame = tk.Frame(root, bg=COLOR_BG)
    stems_frame.place(relx=0.02, rely=0.95, anchor="sw", x=370)
    
    def create_stem_button(parent, stem_name, label):
        """
        Erstellt einen runden Toggle-Button für einen Stem.
        - Linksklick: Toggle (permanent)
        - Rechtsklick gedrückt: Temporär aktivieren
        - Mittelklick gedrückt: Temporär deaktivieren
        """
        size = 32  # Durchmesser
        canvas = tk.Canvas(parent, width=size, height=size, bg=COLOR_BG, 
                           highlightthickness=0, cursor="hand2")
        
        # Kreis zeichnen
        circle = canvas.create_oval(2, 2, size-2, size-2, 
                                   fill=COLOR_BTN_INACTIVE, outline="#555", width=2,
                                   tags="circle")
        # Label
        text = canvas.create_text(size//2, size//2, text=label, 
                                 fill=COLOR_TEXT, font=("Arial", 10, "bold"),
                                 tags="text")
        
        # Speichere temporären Zustand
        canvas._temp_state = {"right_held": False, "middle_held": False, "original_state": False}
        
        # Linksklick: Toggle (permanent)
        def on_left_click(event):
            on_stem_toggle_callback(stem_name)
        
        # Rechtsklick gedrückt: Temporär aktivieren
        def on_right_press(event):
            active_loop = get_active_loop_with_stems()
            if active_loop is None:
                return
            
            data = button_data[active_loop]
            canvas._temp_state["original_state"] = data["stems"]["states"].get(stem_name, False)
            canvas._temp_state["right_held"] = True
            
            # Stem-Player initialisieren falls noch nicht geschehen
            if not data["stems"]["initialized"]:
                initialize_stem_players_callback(active_loop)
            
            # Stem temporär aktivieren (ohne State zu ändern)
            on_stem_momentary_activate_callback(stem_name, activate=True)
            
            # Visuelles Feedback
            canvas.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
            canvas.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
        
        # Rechtsklick losgelassen: Ursprünglichen Zustand wiederherstellen
        def on_right_release(event):
            if not canvas._temp_state["right_held"]:
                return
            canvas._temp_state["right_held"] = False
            
            # Ursprünglichen Zustand wiederherstellen
            on_stem_momentary_release_callback(stem_name)
            update_stem_buttons_state()
        
        # Mittelklick gedrückt: Temporär deaktivieren
        def on_middle_press(event):
            active_loop = get_active_loop_with_stems()
            if active_loop is None:
                return
            
            data = button_data[active_loop]
            canvas._temp_state["original_state"] = data["stems"]["states"].get(stem_name, False)
            canvas._temp_state["middle_held"] = True
            
            # Stem-Player initialisieren falls noch nicht geschehen
            if not data["stems"]["initialized"]:
                initialize_stem_players_callback(active_loop)
            
            # Stem temporär deaktivieren (ohne State zu ändern)
            on_stem_momentary_activate_callback(stem_name, activate=False)
            
            # Visuelles Feedback
            canvas.itemconfig("circle", fill=COLOR_BTN_INACTIVE, outline="#555")
            canvas.itemconfig("text", fill=COLOR_TEXT)
        
        # Mittelklick losgelassen: Ursprünglichen Zustand wiederherstellen
        def on_middle_release(event):
            if not canvas._temp_state["middle_held"]:
                return
            canvas._temp_state["middle_held"] = False
            
            # Ursprünglichen Zustand wiederherstellen
            on_stem_momentary_release_callback(stem_name)
            update_stem_buttons_state()
        
        def on_enter(event):
            # Prüfe ob Stem aktiv
            active_loop = get_active_loop_with_stems()
            if active_loop:
                canvas.config(cursor="hand2")
            else:
                canvas.config(cursor="arrow")
        
        # Event Bindings
        canvas.bind("<Button-1>", on_left_click)
        canvas.bind("<ButtonPress-3>", on_right_press)
        canvas.bind("<ButtonRelease-3>", on_right_release)
        canvas.bind("<ButtonPress-2>", on_middle_press)
        canvas.bind("<ButtonRelease-2>", on_middle_release)
        canvas.bind("<Enter>", on_enter)
        
        return canvas
    
    # Stem-Buttons erstellen
    stem_label_text = tk.Label(stems_frame, text="STEMS:", fg="#888", bg=COLOR_BG, 
                               font=("Arial", 8))
    stem_label_text.pack(side="left", padx=(0, 5))
    
    for stem in STEM_NAMES:
        btn = create_stem_button(stems_frame, stem, STEM_LABELS[stem])
        btn.pack(side="left", padx=2)
        _stem_buttons[stem] = btn
    
    # Stop-Stem Button (S) - schaltet alle Stems aus und merkt sich den Zustand
    def create_stop_stem_button(parent):
        """
        Erstellt den Stop-Stem Button.
        - Linksklick: Toggle (speichert States, alle aus / wiederherstellen)
        - Rechtsklick gedrückt: Temporär alle Stems aus
        """
        size = 32  # Durchmesser
        canvas = tk.Canvas(parent, width=size, height=size, bg=COLOR_BG, 
                           highlightthickness=0, cursor="hand2")
        
        # Kreis zeichnen
        circle = canvas.create_oval(2, 2, size-2, size-2, 
                                   fill=COLOR_BTN_INACTIVE, outline="#555", width=2,
                                   tags="circle")
        # Label - fettgedrucktes S
        text = canvas.create_text(size//2, size//2, text="S", 
                                 fill=COLOR_TEXT, font=("Arial", 11, "bold"),
                                 tags="text")
        
        # Temporärer Zustand
        canvas._temp_state = {"right_held": False}
        
        # Linksklick: Toggle
        def on_left_click(event):
            on_stop_stem_toggle_callback()
            update_stop_stem_button_state()
        
        # Rechtsklick gedrückt: Temporär aktivieren (Original abspielen)
        def on_right_press(event):
            active_loop = get_active_loop_with_stems()
            if active_loop is None:
                return
            
            canvas._temp_state["right_held"] = True
            on_stop_stem_momentary_callback(activate=True)
            
            # Visuelles Feedback
            canvas.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
            canvas.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
        
        # Rechtsklick losgelassen: Zurück zum ursprünglichen Zustand
        def on_right_release(event):
            if not canvas._temp_state["right_held"]:
                return
            canvas._temp_state["right_held"] = False
            
            on_stop_stem_momentary_release_callback()
            update_stop_stem_button_state()
        
        def on_enter(event):
            active_loop = get_active_loop_with_stems()
            if active_loop:
                canvas.config(cursor="hand2")
            else:
                canvas.config(cursor="arrow")
        
        # Event Bindings
        canvas.bind("<Button-1>", on_left_click)
        canvas.bind("<ButtonPress-3>", on_right_press)
        canvas.bind("<ButtonRelease-3>", on_right_release)
        canvas.bind("<Enter>", on_enter)
        
        return canvas
    
    # Stop-Stem Button erstellen
    _stop_stem_button = create_stop_stem_button(stems_frame)
    _stop_stem_button.pack(side="left", padx=(8, 2))  # Etwas mehr Abstand links
    
    return {
        'stem_buttons': _stem_buttons,
        'stop_stem_button': _stop_stem_button,
    }


def update_stop_stem_button_state():
    """Aktualisiert den Stop-Stem Button basierend auf aktuellem Zustand."""
    if _stop_stem_button is None:
        return
    
    button_data = get_button_data()
    active_loop_id = get_active_loop_with_stems()
    
    if active_loop_id is None:
        # Kein aktiver Loop mit Stems -> grau und disabled-Look
        _stop_stem_button.itemconfig("circle", fill="#2a2a2a", outline="#444")
        _stop_stem_button.itemconfig("text", fill="#666")
    else:
        data = button_data[active_loop_id]
        is_stop_active = data["stems"].get("stop_active", False)
        
        if is_stop_active:
            # Stop aktiv (Original spielt) -> grün
            _stop_stem_button.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
            _stop_stem_button.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
        else:
            # Stop nicht aktiv -> grau
            _stop_stem_button.itemconfig("circle", fill=COLOR_BTN_INACTIVE, outline="#555")
            _stop_stem_button.itemconfig("text", fill=COLOR_TEXT)


def update_stem_buttons_state():
    """Aktualisiert die Stem-Button-Farben basierend auf aktuellem Zustand."""
    button_data = get_button_data()
    selected_stems_button = get_selected_stems_button()
    
    # Verwende selected_stems_button wenn gesetzt, sonst finde aktiven Loop
    target_button_id = selected_stems_button
    
    if target_button_id is None:
        target_button_id = get_active_loop_with_stems()
    
    # Prüfe ob target noch gültig ist (Stems verfügbar)
    if target_button_id is not None:
        data = button_data.get(target_button_id)
        if not data or not data.get("stems", {}).get("available"):
            target_button_id = None
    
    for stem, canvas in _stem_buttons.items():
        if target_button_id is None:
            # Kein selektierter/aktiver Loop mit Stems -> alle grau und disabled-Look
            canvas.itemconfig("circle", fill="#2a2a2a", outline="#444")
            canvas.itemconfig("text", fill="#666")
        else:
            # Selektierter Loop mit Stems -> zeige Zustand
            data = button_data[target_button_id]
            is_active = data["stems"]["states"].get(stem, False)
            
            if is_active:
                canvas.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
                canvas.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
            else:
                canvas.itemconfig("circle", fill=COLOR_BTN_INACTIVE, outline="#555")
                canvas.itemconfig("text", fill=COLOR_TEXT)
    
    # Stop-Stem Button auch aktualisieren
    try:
        update_stop_stem_button_state()
    except:
        pass  # Button existiert vielleicht noch nicht
