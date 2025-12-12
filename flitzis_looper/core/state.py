"""Zentraler State-Manager für flitzis_looper.

Alle globalen Variablen werden hier verwaltet.
"""

import tkinter as tk

# ============== CONSTANTS ==============
GRID_SIZE = 6
NUM_BANKS = 6

# Stem-Namen und ihre Anzeigereihenfolge
STEM_NAMES = ["vocals", "melody", "bass", "drums", "instrumental"]
STEM_LABELS = {"vocals": "V", "melody": "M", "bass": "B", "drums": "D", "instrumental": "I"}

# Colors
COLOR_BG = "#1e1e1e"
COLOR_BTN_INACTIVE = "#3a3a3a"
COLOR_BTN_ACTIVE = "#2ecc71"
COLOR_TEXT = "#ffffff"
COLOR_TEXT_ACTIVE = "#000000"
COLOR_LOCK_OFF = "#882222"
COLOR_LOCK_ON = "#2ecc71"
COLOR_RESET_RED = "#cc4444"
COLOR_BANK_BTN = "#cc7700"
COLOR_BANK_ACTIVE = "#ffaa00"
COLOR_STEM_INACTIVE = "#555555"  # Ausgegraut wenn keine Stems
COLOR_STEM_AVAILABLE = "#cc2222"  # Rot wenn Stems verfügbar
COLOR_STEM_SELECTED = "#ff4444"  # Heller Rot wenn selektiert
COLOR_STEM_GENERATING = "#ff8800"  # Orange während Generierung

# ============== PRIVATE MODULE VARIABLES ==============
_root: tk.Tk | None = None

# Data structures
_all_banks_data = {}
_button_data = {}
_buttons = {}
_bank_buttons = {}
_stem_indicators = {}  # {button_id: label_widget} - Die kleinen S-Quadrate auf jedem Button

# OPTIMIERUNG: Tracking für geladene Loops (statt durch alle 216 Buttons zu iterieren)
_loaded_loops = {}  # {(bank_id, btn_id): loop_object}

# Aktuell selektierter Button für Stem-Kontrolle (kann unterschiedlich vom aktiven Loop sein!)
_selected_stems_button = None

_last_bpm_display = ""

# Tracking für offene Fenster (pro Button nur ein Fenster pro Typ)
_open_volume_windows = {}  # {button_id: window}
_open_loop_editor_windows = {}  # {button_id: window}

# OPTIMIERUNG: Config-Save Debouncing Timer
_save_config_timer = None

# Audio globals (set later by audio module)
_master_amp = None

# Tk-Variablen (werden nach root-Init erstellt)
_bpm_lock_active = None
_key_lock_active = None
_multi_loop_active = None
_master_bpm_value = None
_master_volume = None
_current_bank = None
_speed_value = None


# ============== INITIALIZATION ==============
def init_state(root):
    """Initialisiert alle State-Variablen.

    Muss NACH root-Erstellung aufgerufen werden.
    """
    global \
        _root, \
        _bpm_lock_active, \
        _key_lock_active, \
        _multi_loop_active, \
        _master_bpm_value, \
        _master_volume, \
        _current_bank, \
        _speed_value

    _root = root
    _bpm_lock_active = tk.BooleanVar(value=False)
    _key_lock_active = tk.BooleanVar(value=False)  # Key Lock für Master Tempo
    _multi_loop_active = tk.BooleanVar(value=False)
    _master_bpm_value = tk.DoubleVar(value=0.0)
    _master_volume = tk.DoubleVar(value=1.0)  # 0.0-1.0
    _current_bank = tk.IntVar(value=1)
    _speed_value = tk.DoubleVar(value=1.0)


def init_banks():
    """Initialisiert all_banks_data Struktur."""
    global _button_data
    for bank_id in range(1, NUM_BANKS + 1):
        _all_banks_data[bank_id] = {}
        for btn_id in range(1, GRID_SIZE * GRID_SIZE + 1):
            _all_banks_data[bank_id][btn_id] = get_default_button_data()
    _button_data = _all_banks_data[1]


# ============== DEFAULT DATA FUNCTIONS ==============
def get_default_button_data():
    return {
        "file": None,
        "pyo": None,
        "bpm": None,
        "gain_db": 0.0,
        "loop_start": 0.0,
        "loop_end": None,
        "active": False,
        "auto_loop_active": True,
        "auto_loop_bars": 8,
        "auto_loop_custom_mode": False,
        # Intro settings
        "intro_active": False,
        "intro_bars": 4,
        "intro_custom_mode": False,
        # EQ settings per loop
        "eq_low": 0.0,
        "eq_mid": 0.0,
        "eq_high": 0.0,
        # OPTIMIERUNG: Waveform-Cache speichern
        "waveform_cache": None,
        # STEMS: Audio-Separation für Vocals, Melody, Bass, Drums, Instrumental
        # NUR IM RAM - wird NICHT in config.json gespeichert!
        "stems": get_default_stems_data(),
    }


def get_default_stems_data():
    """Gibt die Standard-Stems-Struktur zurück. Wird NUR im RAM gehalten."""
    return {
        "available": False,  # Stems erzeugt?
        "generating": False,  # Gerade am Generieren?
        "states": {  # GUI-Status/Toggles
            "vocals": False,
            "melody": False,
            "bass": False,
            "drums": False,
            "instrumental": False,  # Sonderrolle (exklusiv)
        },
        "saved_states": None,  # Gespeicherte States für Stop-Stem Button
        "dry": {  # un-gepitchte Arrays (numpy)
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None,
        },
        "pitched": {  # RAM-Caches für aktuelle Speed
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None,
        },
        "players": {  # pyo-Player/Tables je Stem
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None,
        },
        "tables": {  # SndTables für Stems
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None,
        },
        "gains": {  # pyo Sig-Objekte für Gain-Kontrolle (0 oder 1)
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None,
        },
        "outputs": {  # pyo Output-Objekte (mit EQ wenn vorhanden)
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None,
        },
        "main_gain": None,  # Gain für Haupt-Loop (wird 0 wenn Stems aktiv)
        "main_player": None,  # Haupt-Loop Player via Pointer (für Sync)
        "main_table": None,  # Haupt-Loop Table für Pointer
        "master_phasor": None,  # Gemeinsamer Phasor für ALLE Player
        "stem_mix": None,  # Mix aller Stem-Player
        "eq_low": None,  # EQ für Stems
        "eq_mid": None,  # EQ für Stems
        "eq_high": None,  # EQ für Stems
        "final_output": None,  # Finaler Output (nach EQ und Gain)
        "cached_speed": None,  # Für welche Speed wurde gecached?
        "initialized": False,  # Wurden Stem-Player bereits erstellt?
        "stop_active": False,  # Ist Stop-Stem Button aktiv?
    }


def ensure_stems_structure(data):
    """Stellt sicher dass die Stems-Struktur vollständig ist.

    Behebt KeyErrors bei alten Daten oder nach fehlerhaftem Laden.
    """
    if "stems" not in data:
        data["stems"] = get_default_stems_data()
        return

    stems = data["stems"]
    default = get_default_stems_data()

    # Alle fehlenden Top-Level Keys ergänzen
    for key in default:
        if key not in stems:
            stems[key] = default[key]

    # Alle fehlenden Stem-Namen in den Dicts ergänzen
    for dict_key in ["states", "dry", "pitched", "players", "tables", "gains", "outputs"]:
        if dict_key in stems and isinstance(stems[dict_key], dict):
            for stem_name in STEM_NAMES:
                if stem_name not in stems[dict_key]:
                    stems[dict_key][stem_name] = default[dict_key][stem_name]


# ============== ROOT GETTER ==============
def get_root():
    return _root


# ============== TK VARIABLE GETTERS ==============
def get_bpm_lock_active():
    return _bpm_lock_active


def get_key_lock_active():
    return _key_lock_active


def get_multi_loop_active():
    return _multi_loop_active


def get_master_bpm_value():
    return _master_bpm_value


def get_master_volume():
    return _master_volume


def get_current_bank():
    return _current_bank


def get_speed_value():
    return _speed_value


# ============== DATA STRUCTURE GETTERS ==============
def get_all_banks_data():
    return _all_banks_data


def get_button_data(btn_id=None):
    """Returns button data. If btn_id is None, returns the whole dictionary.

    If btn_id is given, returns the data for that button.
    """
    if btn_id is None:
        return _button_data
    return _button_data.get(btn_id)


def get_buttons():
    return _buttons


def get_bank_buttons():
    return _bank_buttons


def get_stem_indicators():
    return _stem_indicators


def get_loaded_loops():
    """Gibt alle geladenen Loops zurück.

    HINWEIS: Gibt die tatsächliche Dict-Referenz zurück für Backward-Kompatibilität.
    Änderungen sollten über register_loaded_loop/unregister_loaded_loop erfolgen.
    """
    return _loaded_loops


def get_selected_stems_button():
    return _selected_stems_button


def get_last_bpm_display():
    return _last_bpm_display


def get_open_volume_windows():
    return _open_volume_windows


def get_open_loop_editor_windows():
    return _open_loop_editor_windows


def get_save_config_timer():
    return _save_config_timer


def get_master_amp():
    return _master_amp


# ============== DATA STRUCTURE SETTERS ==============
def set_button_data_ref(bank_id):
    """Setzt _button_data auf die Referenz des angegebenen Banks.

    Wird bei Bank-Wechsel verwendet.
    """
    global _button_data
    _button_data = _all_banks_data[bank_id]


def set_button_data_value(btn_id, key, value):
    """Setzt einen Wert in button_data für einen bestimmten Button."""
    _button_data[btn_id][key] = value


def register_button(btn_id, button_widget):
    """Registriert einen Button-Widget."""
    _buttons[btn_id] = button_widget


def register_bank_button(bank_id, button_widget):
    """Registriert einen Bank-Button-Widget."""
    _bank_buttons[bank_id] = button_widget


def register_stem_indicator(btn_id, indicator_widget):
    """Registriert einen Stem-Indikator-Widget."""
    _stem_indicators[btn_id] = indicator_widget


def register_loaded_loop(bank_id, btn_id, loop):
    """Registriert einen geladenen Loop für schnelles Tracking."""
    _loaded_loops[bank_id, btn_id] = loop


def unregister_loaded_loop(bank_id, btn_id):
    """Entfernt einen Loop aus dem Tracking."""
    if (bank_id, btn_id) in _loaded_loops:
        del _loaded_loops[bank_id, btn_id]


def set_selected_stems_button(btn_id):
    """Setzt den aktuell für Stem-Kontrolle selektierten Button."""
    global _selected_stems_button
    _selected_stems_button = btn_id


def set_last_bpm_display(value):
    """Setzt den BPM-Display-Cache."""
    global _last_bpm_display
    _last_bpm_display = value


def set_save_config_timer(timer):
    """Setzt den Config-Save-Debouncing-Timer."""
    global _save_config_timer
    _save_config_timer = timer


def set_master_amp(amp):
    """Setzt das Master-Amplitude pyo Sig-Objekt."""
    global _master_amp
    _master_amp = amp


def register_open_volume_window(btn_id, window):
    """Registriert ein offenes Volume-Fenster für einen Button."""
    _open_volume_windows[btn_id] = window


def unregister_open_volume_window(btn_id):
    """Entfernt ein Volume-Fenster aus dem Tracking."""
    _open_volume_windows.pop(btn_id, None)


def register_open_loop_editor_window(btn_id, window):
    """Registriert ein offenes Loop-Editor-Fenster für einen Button."""
    _open_loop_editor_windows[btn_id] = window


def unregister_open_loop_editor_window(btn_id):
    """Entfernt ein Loop-Editor-Fenster aus dem Tracking."""
    _open_loop_editor_windows.pop(btn_id, None)
