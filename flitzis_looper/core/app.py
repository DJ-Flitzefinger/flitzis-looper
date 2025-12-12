"""
Main application module for flitzis_looper.
Orchestrates the application startup, shutdown, and coordinates all modules.
"""

import logging
import tkinter as tk
from pyo import Server, Sig

from flitzis_looper.utils.threading import (
    io_executor, bpm_executor, start_gui_queue_processor
)
from flitzis_looper.core.state import (
    STEM_NAMES, NUM_BANKS,
    init_state, init_banks,
    get_root, get_all_banks_data, get_loaded_loops, get_master_amp,
    set_master_amp
)
from flitzis_looper.core.config import save_config_immediate
from flitzis_looper.audio.pitch import cleanup_stem_caches

logger = logging.getLogger(__name__)


def cleanup_pitch_caches():
    """Gibt alle Pitch-Caches frei beim Beenden (RAM-basiert)."""
    loaded_loops = get_loaded_loops()
    
    try:
        for (bank_id, btn_id), loop in loaded_loops.items():
            if hasattr(loop, '_invalidate_pitch_cache'):
                loop._invalidate_pitch_cache()
    except Exception as e:
        logger.debug(f"Error cleaning up pitch caches: {e}")


def on_closing(s):
    """
    Proper shutdown handler - stoppt alle Audio-Objekte bevor das Fenster geschlossen wird.
    
    Args:
        s: pyo Server-Instanz
    """
    root = get_root()
    all_banks_data = get_all_banks_data()
    loaded_loops = get_loaded_loops()
    
    try:
        # 1. Alle laufenden Loops stoppen
        for (bank_id, btn_id), loop in list(loaded_loops.items()):
            try:
                loop.stop()
            except:
                pass
            
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
                            try:
                                data["stems"][obj_name].stop()
                            except:
                                pass
                    # Master Phasor stoppen
                    if data["stems"].get("master_phasor"):
                        data["stems"]["master_phasor"].stop()
                    # Main Player stoppen
                    if data["stems"].get("main_player"):
                        data["stems"]["main_player"].stop()
                    # Stem Players stoppen
                    for stem in STEM_NAMES:
                        if data["stems"]["players"].get(stem):
                            try:
                                data["stems"]["players"][stem].stop()
                            except:
                                pass
                        if data["stems"]["gains"].get(stem):
                            try:
                                data["stems"]["gains"][stem].stop()
                            except:
                                pass
                except Exception as e:
                    logger.debug(f"Error stopping stems for {bank_id}/{btn_id}: {e}")
        
        # 2. Caches freigeben
        cleanup_pitch_caches()
        cleanup_stem_caches()
        
        # 3. Config speichern
        save_config_immediate()
        
        # 4. pyo Server stoppen
        try:
            s.stop()
        except:
            pass
        
        # 5. Executors herunterfahren
        try:
            io_executor.shutdown(wait=False)
            bpm_executor.shutdown(wait=False)
        except:
            pass
        
    except Exception as e:
        logger.error(f"Error during shutdown: {e}")
    finally:
        # 6. Fenster zerstören
        root.destroy()


def main():
    """
    Hauptfunktion - startet die Anwendung.
    Diese Funktion orchestriert den gesamten Startup-Prozess.
    """
    import matplotlib
    matplotlib.use('TkAgg')
    
    # 1. pyo Server starten
    s = Server(sr=44100, nchnls=2, buffersize=1024, duplex=0).boot()
    s.start()
    
    # 2. Tk root erstellen
    root = tk.Tk()
    root.title("Dj Flitzefinger's Scratch-Looper")
    root.geometry("960x630")
    root.resizable(False, False)
    root.configure(bg="#1e1e1e")
    
    # 3. State initialisieren
    init_state(root)
    
    # 4. Master-Amplitude erstellen und registrieren
    master_amp = Sig(1.0)
    set_master_amp(master_amp)
    
    # 5. Banks initialisieren
    init_banks()
    
    # Importiere die benötigten Module
    from flitzis_looper.audio.loop import PyoLoop
    from flitzis_looper.audio.stems_engine import (
        initialize_stem_players, stop_stem_players, update_stem_gains,
        update_stem_eq, apply_stem_mix, _cleanup_stem_players
    )
    from flitzis_looper.audio.stems_separation import generate_stems, delete_stems
    from flitzis_looper.audio.pitch import (
        precache_pitched_stems_if_needed, invalidate_stem_caches
    )
    from flitzis_looper.core.config import load_config, save_config_async
    from flitzis_looper.core.banks import (
        switch_bank, update_all_button_labels, update_button_label,
        update_all_stem_indicators, select_stems_button
    )
    from flitzis_looper.core.loops import (
        trigger_loop, stop_loop, load_loop, unload_loop
    )
    from flitzis_looper.core.stems_control import (
        on_stem_toggle, on_stem_momentary_activate, on_stem_momentary_release,
        on_stop_stem_toggle, on_stop_stem_momentary, on_stop_stem_momentary_release,
        get_active_loop_with_stems, get_selected_or_active_stems_button
    )
    from flitzis_looper.core.bpm_control import (
        update_bpm_display, update_bpm_display_once, on_speed_change,
        reset_pitch, adjust_bpm_by_delta, toggle_key_lock, toggle_bpm_lock,
        update_speed_from_master_bpm, validate_bpm_entry, detect_bpm_async
    )
    from flitzis_looper.core.volume_control import (
        on_master_volume_change, reset_master_volume, toggle_multi_loop
    )
    from flitzis_looper.core.state import (
        GRID_SIZE, STEM_LABELS,
        COLOR_BG, COLOR_BTN_INACTIVE, COLOR_BTN_ACTIVE, COLOR_TEXT,
        COLOR_TEXT_ACTIVE, COLOR_LOCK_OFF, COLOR_LOCK_ON, COLOR_RESET_RED,
        COLOR_BANK_BTN, COLOR_BANK_ACTIVE, COLOR_STEM_INACTIVE,
        get_button_data, get_buttons, get_bank_buttons, get_stem_indicators,
        get_bpm_lock_active, get_key_lock_active, get_multi_loop_active,
        get_master_bpm_value, get_master_volume, get_current_bank, get_speed_value,
        register_button, register_bank_button, register_stem_indicator,
        register_loaded_loop
    )
    from flitzis_looper.ui.widgets import EQKnob, VUMeter
    from flitzis_looper.ui.dialogs import (
        set_volume, WaveformEditor, set_bpm_manually
    )
    
    # Backward-kompatible Aliase
    button_data = get_button_data()
    buttons = get_buttons()
    bank_buttons = get_bank_buttons()
    stem_indicators = get_stem_indicators()
    bpm_lock_active = get_bpm_lock_active()
    key_lock_active = get_key_lock_active()
    multi_loop_active = get_multi_loop_active()
    master_bpm_value = get_master_bpm_value()
    master_volume = get_master_volume()
    current_bank = get_current_bank()
    speed_value = get_speed_value()
    
    # Widget-Referenzen für Callbacks
    widgets = {}
    
    # ============== STEM BUTTON STATE UPDATE ==============
    stem_buttons = {}  # Wird später befüllt
    stop_stem_button = None  # Wird später gesetzt
    
    def update_stem_buttons_state_impl():
        """Aktualisiert die Stem-Button-Farben basierend auf aktuellem Zustand."""
        from flitzis_looper.core.state import get_selected_stems_button
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
        
        for stem, canvas in stem_buttons.items():
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
        update_stop_stem_button_state_impl()
    
    def update_stop_stem_button_state_impl():
        """Aktualisiert den Stop-Stem Button basierend auf aktuellem Zustand."""
        if stop_stem_button is None:
            return
            
        active_loop_id = get_active_loop_with_stems()
        
        if active_loop_id is None:
            # Kein aktiver Loop mit Stems -> grau und disabled-Look
            stop_stem_button.itemconfig("circle", fill="#2a2a2a", outline="#444")
            stop_stem_button.itemconfig("text", fill="#666")
        else:
            data = button_data[active_loop_id]
            is_stop_active = data["stems"].get("stop_active", False)
            
            if is_stop_active:
                # Stop aktiv (Original spielt) -> grün
                stop_stem_button.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
                stop_stem_button.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
            else:
                # Stop nicht aktiv -> grau
                stop_stem_button.itemconfig("circle", fill=COLOR_BTN_INACTIVE, outline="#555")
                stop_stem_button.itemconfig("text", fill=COLOR_TEXT)
    
    # ============== CALLBACK DICTIONARIES ==============
    # Diese Callbacks werden an die verschiedenen Funktionen übergeben
    
    loop_callbacks = {
        'update_button_label': update_button_label,
        'update_stem_buttons_state': update_stem_buttons_state_impl,
        'update_all_stem_indicators': update_all_stem_indicators,
        'initialize_stem_players': initialize_stem_players,
        'update_stem_gains': update_stem_gains,
        'stop_stem_players': stop_stem_players,
        '_cleanup_stem_players': _cleanup_stem_players,
        'precache_pitched_stems_if_needed': precache_pitched_stems_if_needed,
        'save_config_async': save_config_async,
        'detect_bpm_async': lambda fp, bid, loop: detect_bpm_async(fp, bid, loop, {
            'update_button_label': update_button_label,
            'save_config_async': save_config_async
        }),
        'PyoLoop_class': PyoLoop,
    }
    
    stem_callbacks = {
        'update_stem_gains': update_stem_gains,
        'update_stem_buttons_state': update_stem_buttons_state_impl,
        'save_config_async': save_config_async,
        'initialize_stem_players': initialize_stem_players,
    }
    
    # ============== UPDATE RESET BUTTON STYLE ==============
    reset_btn = None  # Wird später gesetzt
    
    def update_reset_button_style_impl():
        """Reset-Button: Grün bei 1.00, sonst rot."""
        if reset_btn is None:
            return
        current = speed_value.get()
        if current == 1.0 or abs(current - 1.0) < 0.001:
            reset_btn.config(bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE,
                            activebackground=COLOR_BTN_ACTIVE, activeforeground=COLOR_TEXT_ACTIVE)
        else:
            reset_btn.config(bg=COLOR_RESET_RED, fg=COLOR_TEXT,
                            activebackground=COLOR_RESET_RED, activeforeground=COLOR_TEXT)
    
    # ============== UI CREATION ==============
    # Create button grid
    for i in range(GRID_SIZE):
        for j in range(GRID_SIZE):
            btn_id = i * GRID_SIZE + j + 1
            
            # Container Frame für Button + Stem-Indikator
            container = tk.Frame(root, bg=COLOR_BG)
            container.grid(row=i, column=j, padx=5, pady=5)
            
            # Haupt-Button
            btn = tk.Button(
                container,
                text=f"{btn_id}",
                width=14,
                height=4,
                font=("Arial", 9, "bold"),
                bg=COLOR_BTN_INACTIVE,
                fg=COLOR_TEXT,
                activeforeground=COLOR_TEXT,
                activebackground="#555",
                justify="center",
                highlightthickness=0
            )
            btn.pack(fill="both", expand=True)
            btn.bind("<ButtonPress-1>", lambda e, b=btn_id: trigger_loop(b, loop_callbacks))
            btn.bind("<Button-2>", lambda e, b=btn_id: open_context_menu(b, e))
            btn.bind("<ButtonPress-3>", lambda e, b=btn_id: stop_loop(b, loop_callbacks))
            register_button(btn_id, btn)
            
            # Stem-Indikator (kleines S-Quadrat unten rechts)
            stem_indicator = tk.Label(
                container,
                text="S",
                font=("Arial", 7, "bold"),
                bg=COLOR_STEM_INACTIVE,
                fg="#888888",
                width=2,
                height=1,
                cursor="hand2"
            )
            # Platziere unten rechts über dem Button
            stem_indicator.place(relx=1.0, rely=1.0, anchor="se", x=-3, y=-3)
            stem_indicator.bind("<Button-1>", lambda e, b=btn_id: select_stems_button(b, update_stem_buttons_state_impl))
            register_stem_indicator(btn_id, stem_indicator)
    
    # Bank buttons row
    bank_frame = tk.Frame(root, bg=COLOR_BG)
    bank_frame.grid(row=GRID_SIZE, column=0, columnspan=GRID_SIZE, pady=(10, 5), sticky="ew")
    
    for bank_id in range(1, NUM_BANKS + 1):
        btn = tk.Button(
            bank_frame,
            text=f"Bank {bank_id}",
            width=14,
            height=2,
            bg=COLOR_BANK_ACTIVE if bank_id == 1 else COLOR_BANK_BTN,
            fg=COLOR_TEXT_ACTIVE if bank_id == 1 else COLOR_TEXT,
            activeforeground=COLOR_TEXT,
            activebackground="#ff9900",
            font=("Arial", 10, "bold"),
            command=lambda b=bank_id: switch_bank(b, update_stem_buttons_state_impl)
        )
        btn.pack(side="left", padx=5, expand=True, fill="x")
        register_bank_button(bank_id, btn)
    
    # ============== RIGHT FRAME CONTROLS ==============
    right_frame = tk.Frame(root, bg=COLOR_BG)
    right_frame.place(relx=1.0, rely=0.0, anchor="ne", x=-20, y=10)
    
    # BPM Display
    bpm_display = tk.Label(right_frame, text="------", font=("Courier", 28, "bold"), 
                           fg="#ff3333", bg="black", width=6, height=1, anchor="e")
    bpm_display.pack(pady=(0, 20))
    widgets['bpm_display'] = bpm_display
    
    # Container für Slider + Reset Button nebeneinander
    slider_reset_frame = tk.Frame(right_frame, bg=COLOR_BG)
    slider_reset_frame.pack(padx=(8, 0))
    
    # Pitch-Slider (links im Container)
    speed_slider = tk.Scale(
        slider_reset_frame,
        from_=2.0,
        to=0.5,
        resolution=0.01,
        orient="vertical",
        length=400,
        variable=speed_value,
        command=lambda v: on_speed_change(v, widgets, {
            'apply_stem_mix': apply_stem_mix,
            'invalidate_stem_caches': invalidate_stem_caches
        }),
        bg=COLOR_BG,
        fg=COLOR_TEXT,
        troughcolor="#444",
        highlightthickness=0,
        sliderlength=50,
        width=40
    )
    speed_slider.pack(side="left")
    widgets['speed_slider'] = speed_slider
    widgets['update_reset_button_style'] = update_reset_button_style_impl
    
    # Middle-click on slider resets pitch
    speed_slider.bind("<Button-2>", lambda e: reset_pitch(widgets))
    
    # Rechter Bereich neben Slider: BPM Up/Down Buttons + Reset
    reset_frame = tk.Frame(slider_reset_frame, bg=COLOR_BG, width=60, height=400)
    reset_frame.pack(side="left", padx=(5, 0))
    reset_frame.pack_propagate(False)
    
    # Spacer oben bis zu den Buttons (216px für korrekte 1.00 Position)
    spacer_top = tk.Frame(reset_frame, bg=COLOR_BG, height=216)
    spacer_top.pack()
    
    # BPM Up Button
    bpm_up_btn = tk.Button(
        reset_frame,
        text="+",
        font=("Arial", 10),
        fg=COLOR_TEXT,
        bg="#444",
        activebackground="#444",
        activeforeground=COLOR_TEXT,
        relief="raised",
        bd=2,
        width=5
    )
    bpm_up_btn.pack(pady=(0, 4))
    bpm_up_btn.bind("<Button-1>", lambda e: adjust_bpm_by_delta(1.0, widgets, {}))
    bpm_up_btn.bind("<Button-3>", lambda e: adjust_bpm_by_delta(0.1, widgets, {}))
    
    # Reset-Button
    reset_btn = tk.Button(
        reset_frame, 
        text="Reset",
        font=("Arial", 9, "bold"),
        fg=COLOR_TEXT_ACTIVE,
        bg=COLOR_BTN_ACTIVE,
        activebackground=COLOR_BTN_ACTIVE,
        activeforeground=COLOR_TEXT_ACTIVE,
        command=lambda: reset_pitch(widgets),
        relief="flat",
        bd=2,
        width=5
    )
    reset_btn.pack(pady=(0, 4))
    
    # BPM Down Button
    bpm_down_btn = tk.Button(
        reset_frame,
        text="-",
        font=("Arial", 10),
        fg=COLOR_TEXT,
        bg="#444",
        activebackground="#444",
        activeforeground=COLOR_TEXT,
        relief="raised",
        bd=2,
        width=5
    )
    bpm_down_btn.pack(pady=(0, 0))
    bpm_down_btn.bind("<Button-1>", lambda e: adjust_bpm_by_delta(-1.0, widgets, {}))
    bpm_down_btn.bind("<Button-3>", lambda e: adjust_bpm_by_delta(-0.1, widgets, {}))
    
    # Lock Buttons Frame
    lock_frame = tk.Frame(right_frame, bg=COLOR_BG)
    lock_frame.pack(pady=(15, 0))
    
    # KEY LOCK Button
    key_lock_btn = tk.Button(lock_frame, text="KEY LOCK", 
                             command=lambda: toggle_key_lock(key_lock_btn), 
                             bg=COLOR_LOCK_OFF, fg=COLOR_TEXT, width=12)
    key_lock_btn.pack(pady=(0, 5))
    
    # BPM LOCK Button
    bpm_lock_btn = tk.Button(lock_frame, text="BPM LOCK", 
                             command=lambda: toggle_bpm_lock(bpm_lock_btn), 
                             bg=COLOR_LOCK_OFF, fg=COLOR_TEXT, width=12)
    bpm_lock_btn.pack(pady=(0, 5))
    
    # BPM Entry
    bpm_validate_cmd = root.register(validate_bpm_entry)
    bpm_entry = tk.Entry(lock_frame, textvariable=master_bpm_value, width=12, 
                         justify="center", bg="#333", fg=COLOR_TEXT,
                         validate="key", validatecommand=(bpm_validate_cmd, "%P"),
                         insertbackground=COLOR_TEXT)
    bpm_entry.pack()
    bpm_entry.bind("<Return>", lambda e: update_speed_from_master_bpm(bpm_entry, widgets))
    
    # ============== MASTER VOLUME ==============
    volume_frame = tk.Frame(root, bg=COLOR_BG)
    volume_frame.place(relx=0.02, rely=0.95, anchor="sw")
    
    volume_label = tk.Label(volume_frame, text="Master Volume 100%", fg=COLOR_TEXT, bg=COLOR_BG, font=("Arial", 10))
    volume_label.pack()
    
    master_volume_slider = tk.Scale(
        volume_frame,
        from_=0.0,
        to=1.0,
        resolution=0.01,
        orient="horizontal",
        length=200,
        variable=master_volume,
        command=lambda v: on_master_volume_change(v, volume_label),
        bg=COLOR_BG,
        fg=COLOR_TEXT,
        troughcolor="#444",
        highlightthickness=0,
        showvalue=False
    )
    master_volume_slider.pack()
    master_volume_slider.bind("<Double-Button-1>", lambda e: reset_master_volume(master_volume_slider))
    
    # ============== MULTI LOOP BUTTON ==============
    multi_loop_frame = tk.Frame(root, bg=COLOR_BG)
    multi_loop_frame.place(relx=0.02, rely=0.95, anchor="sw", x=250)
    
    multi_loop_btn = tk.Button(
        multi_loop_frame, 
        text="MULTI LOOP", 
        command=lambda: toggle_multi_loop(multi_loop_btn, {'update_button_label': update_button_label}), 
        bg=COLOR_LOCK_OFF, 
        fg=COLOR_TEXT, 
        width=12
    )
    multi_loop_btn.pack()
    
    # ============== STEMS PANEL ==============
    stems_frame = tk.Frame(root, bg=COLOR_BG)
    stems_frame.place(relx=0.02, rely=0.95, anchor="sw", x=370)
    
    def create_stem_button(parent, stem_name, label):
        """Erstellt einen runden Toggle-Button für einen Stem."""
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
            on_stem_toggle(stem_name, stem_callbacks)
        
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
                initialize_stem_players(active_loop)
            
            # Stem temporär aktivieren (ohne State zu ändern)
            on_stem_momentary_activate(stem_name, True, {})
            
            # Visuelles Feedback
            canvas.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
            canvas.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
        
        # Rechtsklick losgelassen: Ursprünglichen Zustand wiederherstellen
        def on_right_release(event):
            if not canvas._temp_state["right_held"]:
                return
            canvas._temp_state["right_held"] = False
            
            # Ursprünglichen Zustand wiederherstellen
            on_stem_momentary_release(stem_name, {'update_stem_gains': update_stem_gains})
            update_stem_buttons_state_impl()
        
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
                initialize_stem_players(active_loop)
            
            # Stem temporär deaktivieren (ohne State zu ändern)
            on_stem_momentary_activate(stem_name, False, {})
            
            # Visuelles Feedback
            canvas.itemconfig("circle", fill=COLOR_BTN_INACTIVE, outline="#555")
            canvas.itemconfig("text", fill=COLOR_TEXT)
        
        # Mittelklick losgelassen: Ursprünglichen Zustand wiederherstellen
        def on_middle_release(event):
            if not canvas._temp_state["middle_held"]:
                return
            canvas._temp_state["middle_held"] = False
            
            # Ursprünglichen Zustand wiederherstellen
            on_stem_momentary_release(stem_name, {'update_stem_gains': update_stem_gains})
            update_stem_buttons_state_impl()
        
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
        stem_buttons[stem] = btn
    
    # Stop-Stem Button
    def create_stop_stem_button(parent):
        """Erstellt den Stop-Stem Button."""
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
            on_stop_stem_toggle(stem_callbacks)
            update_stop_stem_button_state_impl()
        
        # Rechtsklick gedrückt: Temporär aktivieren (Original abspielen)
        def on_right_press(event):
            active_loop = get_active_loop_with_stems()
            if active_loop is None:
                return
            
            canvas._temp_state["right_held"] = True
            on_stop_stem_momentary(True, stem_callbacks)
            
            # Visuelles Feedback
            canvas.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
            canvas.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
        
        # Rechtsklick losgelassen: Zurück zum ursprünglichen Zustand
        def on_right_release(event):
            if not canvas._temp_state["right_held"]:
                return
            canvas._temp_state["right_held"] = False
            
            on_stop_stem_momentary_release({'update_stem_gains': update_stem_gains, 'update_stem_buttons_state': update_stem_buttons_state_impl})
            update_stop_stem_button_state_impl()
        
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
    stop_stem_button = create_stop_stem_button(stems_frame)
    stop_stem_button.pack(side="left", padx=(8, 2))  # Etwas mehr Abstand links
    
    # ============== CONTEXT MENU ==============
    def open_context_menu(button_id, event):
        """Middle click - open context menu"""
        from flitzis_looper.core.state import ensure_stems_structure
        from tkinter import messagebox
        
        ensure_stems_structure(button_data[button_id])
        
        menu = tk.Menu(root, tearoff=0, bg=COLOR_BG, fg=COLOR_TEXT)
        menu.add_command(label="Load Audio", command=lambda: load_loop(button_id, loop_callbacks))
        if button_data[button_id]["file"]:
            menu.add_command(label="Unload Audio", command=lambda: unload_loop(button_id, loop_callbacks))
            menu.add_separator()
            
            from flitzis_looper.core.bpm_control import detect_bpm
            menu.add_command(label="Re-detect BPM",
                command=lambda: detect_bpm(button_data[button_id]['file'],
                    lambda bpm: [button_data[button_id].update({'bpm': bpm}), update_button_label(button_id)]))
            
            # set_bpm_manually benötigt Wrapper mit Callbacks
            def open_bpm_dialog():
                set_bpm_manually(button_id, update_button_label, save_config_async)
            menu.add_command(label="Set BPM manually", command=open_bpm_dialog)
            
            # adjust_loop öffnet WaveformEditor
            def open_loop_editor():
                if not button_data[button_id]["file"]:
                    messagebox.showwarning("No Audio", "Load audio first.")
                    return
                WaveformEditor(root, button_id, update_button_label, save_config_async)
            menu.add_command(label="Adjust Loop", command=open_loop_editor)
            
            # Volume + EQ Dialog
            def open_volume_dialog():
                set_volume(button_id, update_stem_eq, save_config_async)
            menu.add_command(label="Volume + EQ", command=open_volume_dialog)
            
            # STEMS: Separator und Stem-Menüpunkte
            menu.add_separator()
            if button_data[button_id]["bpm"]:
                if button_data[button_id]["stems"]["generating"]:
                    menu.add_command(label="⏳ Generating Stems", state="disabled")
                elif button_data[button_id]["stems"]["available"]:
                    menu.add_command(label="✓ Stems Available", state="disabled")
                    menu.add_command(label="Regenerate Stems", 
                        command=lambda: generate_stems(button_id, update_button_label, 
                                                       update_stem_buttons_state_impl, save_config_async))
                    menu.add_command(label="Delete Stems",
                        command=lambda: delete_stems(button_id, update_button_label,
                                                     update_stem_buttons_state_impl, save_config_async))
                else:
                    menu.add_command(label="Generate Stems",
                        command=lambda: generate_stems(button_id, update_button_label,
                                                       update_stem_buttons_state_impl, save_config_async))
            else:
                menu.add_command(label="Generate Stems (set BPM first)", state="disabled")
        menu.post(event.x_root, event.y_root)
    
    # ============== STARTUP ==============
    # 6. Config laden
    load_config(register_loaded_loop, PyoLoop)
    
    # 7. UI aktualisieren
    update_all_button_labels()
    update_all_stem_indicators()
    update_bpm_display(bpm_display)
    start_gui_queue_processor(root)
    update_reset_button_style_impl()
    master_volume_slider.set(master_volume.get())
    
    # 8. Shutdown-Handler registrieren
    root.protocol("WM_DELETE_WINDOW", lambda: on_closing(s))
    
    # 9. Mainloop starten
    root.mainloop()


if __name__ == "__main__":
    main()
