"""Zentraler State-Manager für flitzis_looper.

Alle globalen Variablen werden hier verwaltet über die AppState-Klasse.
Für Backward-Kompatibilität existieren weiterhin Funktions-Wrapper.
"""

import tkinter as tk
from typing import Any

# ============== CONSTANTS (Module-Level) ==============
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


# ============== DEFAULT DATA FUNCTIONS ==============
def get_default_stems_data() -> dict:
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


def get_default_button_data() -> dict:
    """Gibt die Standard-Button-Daten zurück."""
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


def ensure_stems_structure(data: dict) -> None:
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


# ============== APP STATE CLASS ==============
class AppState:
    """Zentrale State-Klasse für die Anwendung.

    Kapselt alle Zustandsvariablen als Instanz-Attribute.
    """

    def __init__(self, root: tk.Tk):
        """Initialisiert den AppState.

        Args:
            root: Das Tk-Root-Fenster
        """
        self.root = root

        # Data structures
        self._all_banks_data: dict[int, dict[int, dict]] = {}
        self._button_data: dict[int, dict] = {}
        self._buttons: dict[int, tk.Button] = {}
        self._bank_buttons: dict[int, tk.Button] = {}
        self._stem_indicators: dict[int, tk.Widget] = {}

        # OPTIMIERUNG: Tracking für geladene Loops
        self._loaded_loops: dict[tuple[int, int], Any] = {}

        # Aktuell selektierter Button für Stem-Kontrolle
        self._selected_stems_button: int | None = None

        self._last_bpm_display: str = ""

        # Tracking für offene Fenster
        self._open_volume_windows: dict[int, tk.Toplevel] = {}
        self._open_loop_editor_windows: dict[int, tk.Toplevel] = {}

        # Config-Save Debouncing Timer
        self._save_config_timer: str | None = None

        # Audio globals
        self._master_amp: Any = None

        # Tk-Variablen initialisieren
        self._init_tk_vars()

    def _init_tk_vars(self) -> None:
        """Initialisiert alle Tk-Variablen."""
        self.bpm_lock_active = tk.BooleanVar(value=False)
        self.key_lock_active = tk.BooleanVar(value=False)
        self.multi_loop_active = tk.BooleanVar(value=False)
        self.master_bpm_value = tk.DoubleVar(value=0.0)
        self.master_volume = tk.DoubleVar(value=1.0)
        self.current_bank = tk.IntVar(value=1)
        self.speed_value = tk.DoubleVar(value=1.0)

    def init_banks(self) -> None:
        """Initialisiert die all_banks_data Struktur."""
        for bank_id in range(1, NUM_BANKS + 1):
            self._all_banks_data[bank_id] = {}
            for btn_id in range(1, GRID_SIZE * GRID_SIZE + 1):
                self._all_banks_data[bank_id][btn_id] = get_default_button_data()
        self._button_data = self._all_banks_data[1]

    # ============== PROPERTY ACCESSORS ==============
    @property
    def all_banks_data(self) -> dict[int, dict[int, dict]]:
        """Gibt alle Bank-Daten zurück."""
        return self._all_banks_data

    @property
    def button_data(self) -> dict[int, dict]:
        """Gibt die Button-Daten der aktuellen Bank zurück."""
        return self._button_data

    @property
    def buttons(self) -> dict[int, tk.Button]:
        """Gibt alle Button-Widgets zurück."""
        return self._buttons

    @property
    def bank_buttons(self) -> dict[int, tk.Button]:
        """Gibt alle Bank-Button-Widgets zurück."""
        return self._bank_buttons

    @property
    def stem_indicators(self) -> dict[int, tk.Widget]:
        """Gibt alle Stem-Indikator-Widgets zurück."""
        return self._stem_indicators

    @property
    def loaded_loops(self) -> dict[tuple[int, int], Any]:
        """Gibt alle geladenen Loops zurück."""
        return self._loaded_loops

    @property
    def selected_stems_button(self) -> int | None:
        """Gibt den aktuell für Stem-Kontrolle selektierten Button zurück."""
        return self._selected_stems_button

    @selected_stems_button.setter
    def selected_stems_button(self, value: int | None) -> None:
        """Setzt den aktuell für Stem-Kontrolle selektierten Button."""
        self._selected_stems_button = value

    @property
    def last_bpm_display(self) -> str:
        """Gibt den BPM-Display-Cache zurück."""
        return self._last_bpm_display

    @last_bpm_display.setter
    def last_bpm_display(self, value: str) -> None:
        """Setzt den BPM-Display-Cache."""
        self._last_bpm_display = value

    @property
    def open_volume_windows(self) -> dict[int, tk.Toplevel]:
        """Gibt offene Volume-Fenster zurück."""
        return self._open_volume_windows

    @property
    def open_loop_editor_windows(self) -> dict[int, tk.Toplevel]:
        """Gibt offene Loop-Editor-Fenster zurück."""
        return self._open_loop_editor_windows

    @property
    def save_config_timer(self) -> str | None:
        """Gibt den Config-Save-Timer zurück."""
        return self._save_config_timer

    @save_config_timer.setter
    def save_config_timer(self, value: str | None) -> None:
        """Setzt den Config-Save-Timer."""
        self._save_config_timer = value

    @property
    def master_amp(self) -> Any:
        """Gibt das Master-Amplitude pyo Sig-Objekt zurück."""
        return self._master_amp

    @master_amp.setter
    def master_amp(self, value: Any) -> None:
        """Setzt das Master-Amplitude pyo Sig-Objekt."""
        self._master_amp = value

    # ============== DATA METHODS ==============
    def get_button_data(self, btn_id: int | None = None) -> dict | None:
        """Gibt Button-Daten zurück.

        Args:
            btn_id: Button-ID. Wenn None, wird das gesamte Dict zurückgegeben.

        Returns:
            Button-Daten oder None wenn btn_id nicht existiert.
        """
        if btn_id is None:
            return self._button_data
        return self._button_data.get(btn_id)

    def set_button_data_ref(self, bank_id: int) -> None:
        """Setzt _button_data auf die Referenz des angegebenen Banks.

        Wird bei Bank-Wechsel verwendet.
        """
        self._button_data = self._all_banks_data[bank_id]

    def set_button_data_value(self, btn_id: int, key: str, value: Any) -> None:
        """Setzt einen Wert in button_data für einen bestimmten Button."""
        self._button_data[btn_id][key] = value

    # ============== REGISTRATION METHODS ==============
    def register_button(self, btn_id: int, button_widget: tk.Button) -> None:
        """Registriert einen Button-Widget."""
        self._buttons[btn_id] = button_widget

    def register_bank_button(self, bank_id: int, button_widget: tk.Button) -> None:
        """Registriert einen Bank-Button-Widget."""
        self._bank_buttons[bank_id] = button_widget

    def register_stem_indicator(self, btn_id: int, indicator_widget: tk.Widget) -> None:
        """Registriert einen Stem-Indikator-Widget."""
        self._stem_indicators[btn_id] = indicator_widget

    def register_loaded_loop(self, bank_id: int, btn_id: int, loop: Any) -> None:
        """Registriert einen geladenen Loop für schnelles Tracking."""
        self._loaded_loops[bank_id, btn_id] = loop

    def unregister_loaded_loop(self, bank_id: int, btn_id: int) -> None:
        """Entfernt einen Loop aus dem Tracking."""
        if (bank_id, btn_id) in self._loaded_loops:
            del self._loaded_loops[bank_id, btn_id]

    def register_open_volume_window(self, btn_id: int, window: tk.Toplevel) -> None:
        """Registriert ein offenes Volume-Fenster für einen Button."""
        self._open_volume_windows[btn_id] = window

    def unregister_open_volume_window(self, btn_id: int) -> None:
        """Entfernt ein Volume-Fenster aus dem Tracking."""
        self._open_volume_windows.pop(btn_id, None)

    def register_open_loop_editor_window(self, btn_id: int, window: tk.Toplevel) -> None:
        """Registriert ein offenes Loop-Editor-Fenster für einen Button."""
        self._open_loop_editor_windows[btn_id] = window

    def unregister_open_loop_editor_window(self, btn_id: int) -> None:
        """Entfernt ein Loop-Editor-Fenster aus dem Tracking."""
        self._open_loop_editor_windows.pop(btn_id, None)


# ============== GLOBAL INSTANCE ==============
_state_instance: AppState | None = None


def get_state() -> AppState:
    """Gibt die globale AppState-Instanz zurück.

    Raises:
        RuntimeError: Wenn init_state() noch nicht aufgerufen wurde.
    """
    if _state_instance is None:
        msg = "AppState not initialized. Call init_state(root) first."
        raise RuntimeError(msg)
    return _state_instance


# ============== INITIALIZATION (Backward-Compatible) ==============
def init_state(root: tk.Tk) -> AppState:
    """Initialisiert den globalen AppState.

    Muss NACH root-Erstellung aufgerufen werden.

    Args:
        root: Das Tk-Root-Fenster

    Returns:
        Die erstellte AppState-Instanz
    """
    global _state_instance
    _state_instance = AppState(root)
    return _state_instance


def init_banks() -> None:
    """Initialisiert all_banks_data Struktur.

    Backward-kompatibler Wrapper.
    """
    get_state().init_banks()


# ============== BACKWARD-COMPATIBLE WRAPPER FUNCTIONS ==============
# Diese Funktionen existieren für Backward-Kompatibilität mit bestehendem Code.
# Neue Entwicklung sollte direkt auf AppState zugreifen.


def get_root() -> tk.Tk | None:
    """Gibt das Root-Fenster zurück."""
    if _state_instance is None:
        return None
    return _state_instance.root


def get_bpm_lock_active() -> tk.BooleanVar | None:
    """Gibt die BPM-Lock-Variable zurück."""
    if _state_instance is None:
        return None
    return _state_instance.bpm_lock_active


def get_key_lock_active() -> tk.BooleanVar | None:
    """Gibt die Key-Lock-Variable zurück."""
    if _state_instance is None:
        return None
    return _state_instance.key_lock_active


def get_multi_loop_active() -> tk.BooleanVar | None:
    """Gibt die Multi-Loop-Variable zurück."""
    if _state_instance is None:
        return None
    return _state_instance.multi_loop_active


def get_master_bpm_value() -> tk.DoubleVar | None:
    """Gibt die Master-BPM-Variable zurück."""
    if _state_instance is None:
        return None
    return _state_instance.master_bpm_value


def get_master_volume() -> tk.DoubleVar | None:
    """Gibt die Master-Volume-Variable zurück."""
    if _state_instance is None:
        return None
    return _state_instance.master_volume


def get_current_bank() -> tk.IntVar | None:
    """Gibt die aktuelle Bank-Variable zurück."""
    if _state_instance is None:
        return None
    return _state_instance.current_bank


def get_speed_value() -> tk.DoubleVar | None:
    """Gibt die Speed-Variable zurück."""
    if _state_instance is None:
        return None
    return _state_instance.speed_value


def get_all_banks_data() -> dict:
    """Gibt alle Bank-Daten zurück."""
    return get_state().all_banks_data


def get_button_data(btn_id: int | None = None) -> dict | None:
    """Gibt Button-Daten zurück."""
    return get_state().get_button_data(btn_id)


def get_buttons() -> dict:
    """Gibt alle Button-Widgets zurück."""
    return get_state().buttons


def get_bank_buttons() -> dict:
    """Gibt alle Bank-Button-Widgets zurück."""
    return get_state().bank_buttons


def get_stem_indicators() -> dict:
    """Gibt alle Stem-Indikator-Widgets zurück."""
    return get_state().stem_indicators


def get_loaded_loops() -> dict:
    """Gibt alle geladenen Loops zurück."""
    return get_state().loaded_loops


def get_selected_stems_button() -> int | None:
    """Gibt den aktuell für Stem-Kontrolle selektierten Button zurück."""
    return get_state().selected_stems_button


def get_last_bpm_display() -> str:
    """Gibt den BPM-Display-Cache zurück."""
    return get_state().last_bpm_display


def get_open_volume_windows() -> dict:
    """Gibt offene Volume-Fenster zurück."""
    return get_state().open_volume_windows


def get_open_loop_editor_windows() -> dict:
    """Gibt offene Loop-Editor-Fenster zurück."""
    return get_state().open_loop_editor_windows


def get_save_config_timer() -> str | None:
    """Gibt den Config-Save-Timer zurück."""
    return get_state().save_config_timer


def get_master_amp() -> Any:
    """Gibt das Master-Amplitude pyo Sig-Objekt zurück."""
    return get_state().master_amp


def set_button_data_ref(bank_id: int) -> None:
    """Setzt _button_data auf die Referenz des angegebenen Banks."""
    get_state().set_button_data_ref(bank_id)


def set_button_data_value(btn_id: int, key: str, value: Any) -> None:
    """Setzt einen Wert in button_data für einen bestimmten Button."""
    get_state().set_button_data_value(btn_id, key, value)


def register_button(btn_id: int, button_widget: tk.Button) -> None:
    """Registriert einen Button-Widget."""
    get_state().register_button(btn_id, button_widget)


def register_bank_button(bank_id: int, button_widget: tk.Button) -> None:
    """Registriert einen Bank-Button-Widget."""
    get_state().register_bank_button(bank_id, button_widget)


def register_stem_indicator(btn_id: int, indicator_widget: tk.Widget) -> None:
    """Registriert einen Stem-Indikator-Widget."""
    get_state().register_stem_indicator(btn_id, indicator_widget)


def register_loaded_loop(bank_id: int, btn_id: int, loop: Any) -> None:
    """Registriert einen geladenen Loop für schnelles Tracking."""
    get_state().register_loaded_loop(bank_id, btn_id, loop)


def unregister_loaded_loop(bank_id: int, btn_id: int) -> None:
    """Entfernt einen Loop aus dem Tracking."""
    get_state().unregister_loaded_loop(bank_id, btn_id)


def set_selected_stems_button(btn_id: int | None) -> None:
    """Setzt den aktuell für Stem-Kontrolle selektierten Button."""
    get_state().selected_stems_button = btn_id


def set_last_bpm_display(value: str) -> None:
    """Setzt den BPM-Display-Cache."""
    get_state().last_bpm_display = value


def set_save_config_timer(timer: str | None) -> None:
    """Setzt den Config-Save-Debouncing-Timer."""
    get_state().save_config_timer = timer


def set_master_amp(amp: Any) -> None:
    """Setzt das Master-Amplitude pyo Sig-Objekt."""
    get_state().master_amp = amp


def register_open_volume_window(btn_id: int, window: tk.Toplevel) -> None:
    """Registriert ein offenes Volume-Fenster für einen Button."""
    get_state().register_open_volume_window(btn_id, window)


def unregister_open_volume_window(btn_id: int) -> None:
    """Entfernt ein Volume-Fenster aus dem Tracking."""
    get_state().unregister_open_volume_window(btn_id)


def register_open_loop_editor_window(btn_id: int, window: tk.Toplevel) -> None:
    """Registriert ein offenes Loop-Editor-Fenster für einen Button."""
    get_state().register_open_loop_editor_window(btn_id, window)


def unregister_open_loop_editor_window(btn_id: int) -> None:
    """Entfernt ein Loop-Editor-Fenster aus dem Tracking."""
    get_state().unregister_open_loop_editor_window(btn_id)
