import tkinter as tk
from tkinter import filedialog, simpledialog, messagebox
import os
import threading
import numpy as np
import json
import shutil
import sys
import logging
import queue
import math
import traceback
from dataclasses import dataclass, field
from typing import Dict, Any, Optional, Set, Tuple

os.environ['PYO_GUI_WX'] = '0'

# ============== LOGGING SETUP ==============
# Log-Level kann über Umgebungsvariable gesteuert werden:
# SET LOOPER_DEBUG=1 für Debug-Modus
log_level = logging.DEBUG if os.environ.get('LOOPER_DEBUG') else logging.WARNING
logging.basicConfig(
    level=log_level,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s',
    datefmt='%H:%M:%S'
)
logger = logging.getLogger(__name__)


# ============== SAFE CLEANUP HELPERS ==============
def safe_stop(obj, name: str = "object") -> bool:
    """
    Stoppt ein pyo-Objekt sicher und loggt Fehler.
    
    Args:
        obj: Das zu stoppende pyo-Objekt
        name: Name für Logging (z.B. "master_phasor")
    
    Returns:
        True wenn erfolgreich, False bei Fehler
    """
    if obj is None:
        return True
    try:
        obj.stop()
        return True
    except Exception as e:
        logger.debug(f"Could not stop {name}: {type(e).__name__}: {e}")
        return False


def safe_delete_file(filepath: str, name: str = "file") -> bool:
    """
    Löscht eine Datei sicher und loggt Fehler.
    
    Args:
        filepath: Pfad zur Datei
        name: Name für Logging
    
    Returns:
        True wenn erfolgreich oder Datei nicht existiert, False bei Fehler
    """
    if not filepath:
        return True
    try:
        if os.path.exists(filepath):
            os.unlink(filepath)
        return True
    except OSError as e:
        logger.warning(f"Could not delete {name} '{filepath}': OSError: {e}")
        return False
    except Exception as e:
        logger.error(f"Unexpected error deleting {name} '{filepath}': {type(e).__name__}: {e}")
        return False


def safe_call(func, *args, error_msg: str = "Operation failed", **kwargs) -> Tuple[bool, Any]:
    """
    Führt eine Funktion sicher aus und loggt Fehler.
    
    Args:
        func: Die auszuführende Funktion
        *args: Argumente für die Funktion
        error_msg: Fehlermeldung für Logging
        **kwargs: Keyword-Argumente für die Funktion
    
    Returns:
        Tuple (success: bool, result: Any)
    """
    try:
        result = func(*args, **kwargs)
        return True, result
    except Exception as e:
        logger.warning(f"{error_msg}: {type(e).__name__}: {e}")
        return False, None

from pyo import *
import soundfile as sf
import matplotlib
matplotlib.use('TkAgg')
import matplotlib.pyplot as plt
from matplotlib.backends.backend_tkagg import FigureCanvasTkAgg
from matplotlib.figure import Figure
from concurrent.futures import ThreadPoolExecutor

LOOP_DIR = "loops"
CONFIG_FILE = "config.json"
os.makedirs(LOOP_DIR, exist_ok=True)

s = Server(sr=44100, nchnls=2, buffersize=1024, duplex=0).boot()
s.start()

io_executor = ThreadPoolExecutor(max_workers=2)
bpm_executor = ThreadPoolExecutor(max_workers=1)

gui_update_queue = queue.Queue()

def process_gui_queue():
    try:
        for _ in range(10):
            try:
                callback = gui_update_queue.get_nowait()
                callback()
            except queue.Empty:
                break
            except tk.TclError:
                # Widget wurde zerstört während Callback wartete - ignorieren
                pass
    except Exception as e:
        logger.exception(f"GUI queue processing error: {e}")
    root.after(30, process_gui_queue)

def schedule_gui_update(callback):
    gui_update_queue.put(callback)

def _detect_bpm_worker(filepath):
    """
    Erkennt BPM einer Audio-Datei mittels madmom.
    
    Returns:
        float: Erkannte BPM (gerundet auf 1 Dezimalstelle) oder None bei Fehler
    """
    try:
        from madmom.features.beats import RNNBeatProcessor, DBNBeatTrackingProcessor
        
        # Versuche Prozess-Priorität zu senken (optional, nur auf Unix)
        if sys.platform != 'win32':
            try:
                os.nice(10)
            except OSError as e:
                # nice() nicht verfügbar oder keine Berechtigung - kein Problem
                logger.debug(f"Could not set process priority: {e}")
        
        if not os.path.exists(filepath):
            logger.warning(f"BPM detection: File not found: {filepath}")
            return None
            
        proc = DBNBeatTrackingProcessor(fps=100, min_bpm=60, max_bpm=180)
        act = RNNBeatProcessor()(filepath)
        beats = proc(act)
        
        if len(beats) > 1:
            intervals = np.diff(beats)
            avg_interval = np.mean(intervals)
            bpm = 60.0 / avg_interval
            if 60 <= bpm <= 200:
                return round(bpm, 1)
            else:
                logger.debug(f"BPM {bpm:.1f} outside valid range 60-200")
        return None
        
    except ImportError:
        logger.info("madmom not installed - BPM detection unavailable")
        return None
    except Exception as e:
        logger.warning(f"BPM detection failed for '{filepath}': {type(e).__name__}: {e}")
        return None

def db_to_amp(db):
    return 10 ** (db / 20.0)

def speed_to_semitones(speed):
    """
    Berechnet die Halbtöne-Kompensation für Key Lock.
    Bei speed > 1: Pitch würde steigen -> negative Halbtöne zum Kompensieren
    Bei speed < 1: Pitch würde sinken -> positive Halbtöne zum Kompensieren
    """
    if speed <= 0:
        return 0.0
    return -12.0 * math.log2(speed)

# Main window
root = tk.Tk()
root.title("Dj Flitzefinger's Scratch-Looper")
root.geometry("957x691")
root.resizable(False, False)
root.configure(bg="#1e1e1e")

# Global variables
bpm_lock_active = tk.BooleanVar(value=False)
key_lock_active = tk.BooleanVar(value=False)  # Key Lock für Master Tempo
multi_loop_active = tk.BooleanVar(value=False)
master_bpm_value = tk.DoubleVar(value=0.0)
master_volume = tk.DoubleVar(value=1.0)  # 0.0-1.0
current_bank = tk.IntVar(value=1)
speed_value = tk.DoubleVar(value=1.0)

GRID_SIZE = 6
NUM_BANKS = 6

all_banks_data = {}
button_data = {}
buttons = {}
bank_buttons = {}

last_bpm_display = ""

# OPTIMIERUNG: Tracking für geladene Loops (statt durch alle 216 Buttons zu iterieren)
loaded_loops = {}  # {(bank_id, btn_id): loop_object}

# Tracking für offene Fenster (pro Button nur ein Fenster pro Typ)
open_volume_windows = {}  # {button_id: window}
open_loop_editor_windows = {}  # {button_id: window}

# OPTIMIERUNG: Config-Save Debouncing Timer
_save_config_timer = None

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

# Master volume as amplitude multiplier
master_amp = Sig(1.0)

# Stem-Namen und ihre Anzeigereihenfolge
STEM_NAMES = ["vocals", "melody", "bass", "drums", "instrumental"]
STEM_LABELS = {"vocals": "V", "melody": "M", "bass": "B", "drums": "D", "instrumental": "I"}

# ============== DEFAULT DATA ==============
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
        "stems": get_default_stems_data()
    }

def get_default_stems_data():
    """Gibt die Standard-Stems-Struktur zurück. Wird NUR im RAM gehalten."""
    return {
        "available": False,           # Stems erzeugt?
        "generating": False,          # Gerade am Generieren?
        "states": {                   # GUI-Status/Toggles
            "vocals": False,
            "melody": False,
            "bass": False,
            "drums": False,
            "instrumental": False     # Sonderrolle (exklusiv)
        },
        "saved_states": None,         # Gespeicherte States für Stop-Stem Button
        "dry": {                      # un-gepitchte Arrays (numpy)
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None
        },
        "pitched": {                  # RAM-Caches für aktuelle Speed
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None
        },
        "players": {                  # pyo-Player/Tables je Stem
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None
        },
        "tables": {                   # SndTables für Stems
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None
        },
        "gains": {                    # pyo Sig-Objekte für Gain-Kontrolle (0 oder 1)
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None
        },
        "outputs": {                  # pyo Output-Objekte (mit EQ wenn vorhanden)
            "vocals": None,
            "melody": None,
            "bass": None,
            "drums": None,
            "instrumental": None
        },
        "main_gain": None,            # Gain für Haupt-Loop (wird 0 wenn Stems aktiv)
        "main_player": None,          # Haupt-Loop Player via Pointer (für Sync)
        "main_table": None,           # Haupt-Loop Table für Pointer
        "master_phasor": None,        # Gemeinsamer Phasor für ALLE Player
        "stem_mix": None,             # Mix aller Stem-Player
        "eq_low": None,               # EQ für Stems
        "eq_mid": None,               # EQ für Stems  
        "eq_high": None,              # EQ für Stems
        "final_output": None,         # Finaler Output (nach EQ und Gain)
        "cached_speed": None,         # Für welche Speed wurde gecached?
        "initialized": False,         # Wurden Stem-Player bereits erstellt?
        "stop_active": False          # Ist Stop-Stem Button aktiv?
    }

def ensure_stems_structure(data):
    """
    Stellt sicher dass die Stems-Struktur vollständig ist.
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

def init_banks():
    global all_banks_data, button_data
    for bank_id in range(1, NUM_BANKS + 1):
        all_banks_data[bank_id] = {}
        for btn_id in range(1, GRID_SIZE * GRID_SIZE + 1):
            all_banks_data[bank_id][btn_id] = get_default_button_data()
    button_data = all_banks_data[1]

# ============== LOADED LOOPS MANAGEMENT ==============
def register_loaded_loop(bank_id, btn_id, loop):
    """Registriert einen geladenen Loop für schnelles Tracking"""
    loaded_loops[(bank_id, btn_id)] = loop

def unregister_loaded_loop(bank_id, btn_id):
    """Entfernt einen Loop aus dem Tracking"""
    if (bank_id, btn_id) in loaded_loops:
        del loaded_loops[(bank_id, btn_id)]

def get_loaded_loops():
    """Gibt alle geladenen Loops zurück"""
    return loaded_loops.copy()

# ============== CONFIG ==============
def save_config():
    try:
        config = {
            "global": {
                "master_volume": master_volume.get()
            },
            "banks": {}
        }
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
        with open(CONFIG_FILE, "w") as f:
            json.dump(config, f, indent=2)
    except Exception as e:
        logger.error(f"Error saving config: {type(e).__name__}: {e}")

def save_config_async():
    """OPTIMIERUNG: Debounced config save - wartet 2 Sekunden vor dem Speichern"""
    global _save_config_timer
    try:
        if _save_config_timer:
            root.after_cancel(_save_config_timer)
    except (ValueError, tk.TclError):
        pass  # Timer war ungültig oder bereits abgelaufen
    _save_config_timer = root.after(2000, lambda: io_executor.submit(save_config))

def save_config_immediate():
    """Speichert sofort ohne Debouncing (für wichtige Änderungen wie beim Schließen)"""
    global _save_config_timer
    try:
        if _save_config_timer:
            root.after_cancel(_save_config_timer)
            _save_config_timer = None
    except (ValueError, tk.TclError):
        pass  # Timer war ungültig, bereits abgelaufen, oder Fenster geschlossen
    io_executor.submit(save_config)

def load_config():
    global all_banks_data, button_data
    try:
        if not os.path.exists(CONFIG_FILE):
            return
        with open(CONFIG_FILE, "r") as f:
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
                            if key in ["stems_available", "stems_states"]:
                                continue  # Ignorieren
                            else:
                                data[key] = value
                        
                        # Stems-Struktur sicherstellen (frisch, nicht aus Config)
                        ensure_stems_structure(data)
                        
                        loop = PyoLoop()
                        if loop.load(data["file"], data["loop_start"], data["loop_end"]):
                            loop.set_gain(data["gain_db"])
                            # Apply EQ settings
                            loop.set_eq(data.get("eq_low", 0.0), 
                                       data.get("eq_mid", 0.0), 
                                       data.get("eq_high", 0.0))
                            data["pyo"] = loop
                            # OPTIMIERUNG: Loop registrieren
                            register_loaded_loop(bank_id, btn_id, loop)
        
        button_data = all_banks_data[current_bank.get()]
    except Exception as e:
        logger.error(f"Error loading config: {type(e).__name__}: {e}")

# ============== PYOLOOP ==============
class PyoLoop:
    """
    Audio-Loop-Player mit Key Lock und EQ-Unterstützung.
    Verwendet Pedalboard/Rubberband für hochwertiges Pitch-Shifting.
    
    OPTIMIERUNG FÜR KEY LOCK (RAM-basiert):
    - Gepitchtes Audio wird komplett im RAM gecacht als numpy array
    - SndTable wird direkt aus RAM-Daten erstellt (via tempfile im RAM)
    - Cache bleibt erhalten solange Speed sich nicht ändert
    - Pre-Caching via Rechtsklick auf gestoppte Loops möglich
    - Bei Retrigger wird nur der Player neu erstellt, kein erneutes Pitch-Shifting
    """
    def __init__(self):
        self.path = None
        self.player = None
        self.table = None
        self.amp = None
        self.speed = None
        self.follower = None
        self.loop_start = 0.0
        self.loop_end = None
        self._is_playing = False
        self._duration = 0.0
        self._sample_rate = 44100
        self._is_loaded = False
        self._loading = False
        self._pyo_initialized = False
        self._pending_gain = 0.0
        self._pending_speed = 1.0
        self._use_table = False
        self._loop_base_freq = 1.0
        
        # Key Lock
        self._key_lock = False
        self._current_pitch_shift_semitones = 0.0
        
        # Pitch-shifted Audio Cache (RAM-basiert)
        self._pitched_table = None      # SndTable mit pitch-shifted Audio
        self._pitched_player = None     # Player für pitch-shifted Audio
        self._cached_speed = None       # Für welche Speed wurde gecached?
        self._pitched_audio_cache = None  # numpy array mit gepitchtem Stereo-Audio im RAM
        self._pitched_base_freq = 1.0   # Base frequency für pitched player
        
        # EQ
        self.eq_low = None
        self.eq_mid = None
        self.eq_high = None
        self.output = None
        
        # Stem-Mute: Externer Multiplikator (0=stumm wenn Stems aktiv, 1=normal)
        self._stem_mute = None  # Wird von außen gesetzt (SigTo für smooth fading)
        self._final_output = None  # Output nach Stem-Mute Multiplikation
        
        # EQ values
        self._eq_low_val = 0.0
        self._eq_mid_val = 0.0
        self._eq_high_val = 0.0
        
        # Original Audio Data (für Pitch-Shifting)
        self._audio_data = None
        self._audio_sr = None
        
        # Intro Mode (neues Konzept: ein Player für alles)
        self._intro_start = None  # Wenn gesetzt, startet Wiedergabe hier statt bei loop_start

    def _init_pyo_objects(self):
        if not self._pyo_initialized:
            self.amp = Sig(db_to_amp(float(self._pending_gain)))
            self.speed = Sig(float(self._pending_speed))
            self._pyo_initialized = True

    def load(self, path, loop_start=0.0, loop_end=None):
        """Synchrones Laden"""
        try:
            self.stop()
            if not os.path.exists(path):
                return False
            self.path = path
            self.loop_start = float(loop_start)
            self.loop_end = float(loop_end) if loop_end is not None else None
            info = sf.info(path)
            self._sample_rate = int(info.samplerate)
            self._duration = float(info.duration)
            if self.loop_end is None:
                self.loop_end = float(self._duration)
            
            # Audio-Daten für späteres Pitch-Shifting laden
            self._audio_data, self._audio_sr = sf.read(path)
            
            self._is_loaded = True
            return True
        except Exception as e:
            logger.error(f"Error loading '{self.path}': {type(e).__name__}: {e}")
            return False

    def load_async(self, path, loop_start=0.0, loop_end=None, callback=None):
        """Asynchrones Laden"""
        if self._loading:
            return
        self._loading = True
        
        def do_load():
            try:
                self.stop()
                if not os.path.exists(path):
                    if callback:
                        schedule_gui_update(lambda: callback(False))
                    self._loading = False
                    return
                self.path = path
                self.loop_start = float(loop_start)
                self.loop_end = float(loop_end) if loop_end is not None else None
                info = sf.info(path)
                sample_rate = int(info.samplerate)
                duration = float(info.duration)
                
                # Audio-Daten laden
                audio_data, audio_sr = sf.read(path)
                
                def finish_load():
                    self._sample_rate = sample_rate
                    self._duration = duration
                    self._audio_data = audio_data
                    self._audio_sr = audio_sr
                    if self.loop_end is None:
                        self.loop_end = float(self._duration)
                    self._is_loaded = True
                    self._loading = False
                    if callback:
                        callback(True)
                
                schedule_gui_update(finish_load)
            except Exception as e:
                logger.warning(f"Async load failed for '{path}': {type(e).__name__}: {e}")
                self._loading = False
                if callback:
                    schedule_gui_update(lambda: callback(False))
        
        io_executor.submit(do_load)

    def _ensure_player(self):
        """Erstellt den Player wenn nötig"""
        if self.player is not None:
            return True
        if not self.path or not os.path.exists(self.path):
            return False
        try:
            self._init_pyo_objects()
            self._create_player()
            return self.player is not None
        except Exception as e:
            logger.error(f"Error ensuring player: {type(e).__name__}: {e}")
            return False

    def _create_player(self):
        """
        Erstellt den Audio-Player.
        
        NEUES KONZEPT FÜR INTRO:
        - Ohne Intro: SndTable (loop_start bis loop_end) + Osc (loopt sofort)
        - Mit Intro: SndTable (intro_start bis loop_end) + Looper mit startfromloop=False
          -> Looper spielt erst Intro durch, dann loopt er den Loop-Bereich
        """
        try:
            self._stop_all_objects()
            if not self.path:
                return
            self._init_pyo_objects()
            
            duration = float(self._duration)
            loop_start = float(self.loop_start)
            loop_end = float(self.loop_end) if self.loop_end else duration
            intro_start = float(self._intro_start) if self._intro_start is not None else None
            
            # Hat Intro UND Intro liegt vor Loop-Start?
            has_intro = (intro_start is not None and intro_start < loop_start)
            
            is_full_track = (loop_start < 0.01 and abs(loop_end - duration) < 0.01 and not has_intro)
            
            if is_full_track:
                # Ganzer Track ohne Intro -> einfacher SfPlayer
                self._use_table = False
                self.player = SfPlayer(
                    self.path,
                    loop=True,
                    speed=self.speed,
                    mul=self.amp * master_amp
                )
            elif has_intro:
                # MIT INTRO: Looper-basierter Ansatz
                # Table geht von intro_start bis loop_end
                self._use_table = True
                self.table = SndTable(self.path, start=intro_start, stop=loop_end)
                table_dur = self.table.getDur()
                
                # Loop-Start relativ zur Table (nicht zum Original-File)
                loop_start_in_table = loop_start - intro_start
                loop_dur = loop_end - loop_start
                
                # Looper mit startfromloop=False:
                # - Startet am Anfang der Table (= intro_start)
                # - Spielt bis zum Loop-Ende durch
                # - Springt dann zu 'start' und loopt 'dur' Sekunden
                # WICHTIG: Looper erlaubt keine negativen pitch-Werte!
                self.player = Looper(
                    table=self.table,
                    pitch=abs(self._pending_speed),  # Looper verwendet 'pitch', keine negativen Werte!
                    start=loop_start_in_table,  # Loop-Start relativ zur Table
                    dur=loop_dur,  # Loop-Dauer
                    xfade=0,  # Kein Crossfade für harte Loop-Punkte
                    mode=1,  # 1 = forward loop
                    startfromloop=False,  # WICHTIG: Startet von Table-Anfang (Intro!)
                    interp=4,  # Cubic interpolation
                    mul=self.amp * master_amp
                )
                self._loop_base_freq = 1.0  # Nicht verwendet bei Looper
            else:
                # OHNE INTRO: Osc-basierter Ansatz wie bisher
                self._use_table = True
                self.table = SndTable(self.path, start=loop_start, stop=loop_end)
                table_dur = self.table.getDur()
                base_freq = 1.0 / table_dur if table_dur > 0 else 1.0
                self.player = Osc(
                    table=self.table,
                    freq=base_freq * self.speed,
                    phase=0,
                    interp=4,
                    mul=self.amp * master_amp
                )
                self._loop_base_freq = base_freq
            
            # Bei aktivem Key Lock: Pitched Player erstellen
            if self._key_lock:
                self._create_pitched_player()
            
            self._create_eq_chain()
        except Exception as e:
            logger.error(f"Error creating player for '{self.path}': {type(e).__name__}: {e}")

    def _create_pitched_player(self):
        """
        Erstellt einen Player mit pitch-korrigiertem Audio.
        Verwendet Pedalboard/Rubberband für hohe Qualität.
        
        OPTIMIERUNG (RAM-basiert):
        - Gepitchtes Audio wird im RAM gecacht (_pitched_audio_cache)
        - Bei gleicher Speed wird der Cache wiederverwendet (kein erneutes Pitch-Shifting!)
        - SndTable wird aus RAM-Daten via tempfile erstellt
        """
        try:
            if self._audio_data is None:
                return
            
            # Prüfe ob wir den Cache wiederverwenden können
            if (self._cached_speed == self._pending_speed and 
                self._pitched_audio_cache is not None):
                # Cache ist gültig - erstelle nur neuen Player mit gecachtem Audio
                self._create_pitched_player_from_cache()
                return
            
            # Cache ist ungültig oder nicht vorhanden - neu berechnen
            from pedalboard import PitchShift
            
            # Berechne benötigte Pitch-Korrektur
            semitones = speed_to_semitones(self._pending_speed)
            
            # Audio-Segment extrahieren (für Loop-Bereich)
            start_sample = int(self.loop_start * self._audio_sr)
            end_sample = int(self.loop_end * self._audio_sr) if self.loop_end else len(self._audio_data)
            
            audio_segment = self._audio_data[start_sample:end_sample]
            
            # Stelle sicher, dass es float32 ist
            if audio_segment.dtype != np.float32:
                audio_segment = audio_segment.astype(np.float32)
            
            # Pitch-Shift anwenden mit Pedalboard/Rubberband
            pitch_shifter = PitchShift(semitones=semitones)
            
            # Pedalboard erwartet (channels, samples) Format
            if len(audio_segment.shape) == 1:
                # Mono -> (1, samples)
                audio_segment = audio_segment.reshape(1, -1)
            else:
                # Stereo -> (2, samples) - transponieren von (samples, 2)
                audio_segment = audio_segment.T
            
            # Pitch-Shift anwenden
            pitched_audio = pitch_shifter(audio_segment, self._audio_sr)
            
            # Zurück zu (samples, channels) für pyo
            if pitched_audio.shape[0] <= 2:
                pitched_audio = pitched_audio.T
            
            # Im RAM cachen (numpy array) - BLEIBT ERHALTEN bis Speed sich ändert!
            self._pitched_audio_cache = pitched_audio.copy()
            self._cached_speed = self._pending_speed
            
            # Player aus Cache erstellen
            self._create_pitched_player_from_cache()
                
        except ImportError:
            logger.error("Pedalboard nicht installiert! Installiere mit: pip install pedalboard")
        except Exception as e:
            logger.error(f"Error creating pitched player: {type(e).__name__}: {e}")

    def _create_pitched_player_from_cache(self):
        """
        Erstellt den pitched player aus dem gecachten Audio im RAM.
        Diese Methode ist SCHNELL weil:
        1. Kein Pitch-Shifting nötig ist (Cache wird verwendet)
        2. Nur eine temporäre Datei geschrieben und SndTable geladen wird
        Unterstützt STEREO Audio!
        """
        try:
            if self._pitched_audio_cache is None:
                return
            
            # Alte pitched Objekte stoppen
            if self._pitched_player:
                safe_stop(self._pitched_player, "_pitched_player")
            
            # Temporäre Datei aus RAM-Cache erstellen (für Stereo-Unterstützung)
            import tempfile
            import io
            
            # Schreibe in BytesIO und dann in tempfile
            temp_file = tempfile.NamedTemporaryFile(suffix='.wav', delete=False)
            temp_path = temp_file.name
            temp_file.close()
            
            sf.write(temp_path, self._pitched_audio_cache, self._audio_sr)
            
            # SndTable laden (unterstützt Stereo!)
            self._pitched_table = SndTable(temp_path)
            
            # Temporäre Datei sofort löschen (Daten sind jetzt in SndTable)
            safe_delete_file(temp_path, "temp file")
            
            table_dur = self._pitched_table.getDur()
            base_freq = 1.0 / table_dur if table_dur > 0 else 1.0
            self._pitched_base_freq = base_freq
            
            # Neuen Player erstellen
            self._pitched_player = Osc(
                table=self._pitched_table,
                freq=base_freq * self._pending_speed,
                phase=0,
                interp=4,
                mul=self.amp * master_amp
            )
            
        except Exception as e:
            logger.error(f"Error creating pitched player from cache: {type(e).__name__}: {e}")

    def _invalidate_pitch_cache(self):
        """Invalidiert den Pitch-Cache wenn sich relevante Parameter ändern."""
        self._pitched_audio_cache = None
        self._cached_speed = None
    
    def precache_pitched_audio(self):
        """
        Pre-cached das pitch-shifted Audio für schnelles Triggern.
        Wird aufgerufen wenn auf einen gestoppten Loop mit Rechtsklick geklickt wird.
        Nur aktiv wenn Key Lock aktiviert ist.
        
        WICHTIG: Diese Methode erstellt nur den Cache (numpy array), 
        nicht den Player. Der Player wird erst bei play() erstellt.
        """
        if not self._key_lock:
            return False
        
        if self._audio_data is None:
            return False
        
        # Prüfe ob Cache bereits gültig ist
        if (self._cached_speed == self._pending_speed and 
            self._pitched_audio_cache is not None):
            return True  # Bereits gecacht
        
        # Cache erstellen (nur das Pitch-Shifting, kein Player!)
        try:
            from pedalboard import PitchShift
            
            # Berechne benötigte Pitch-Korrektur
            semitones = speed_to_semitones(self._pending_speed)
            
            # Audio-Segment extrahieren (für Loop-Bereich)
            start_sample = int(self.loop_start * self._audio_sr)
            end_sample = int(self.loop_end * self._audio_sr) if self.loop_end else len(self._audio_data)
            
            audio_segment = self._audio_data[start_sample:end_sample]
            
            # Stelle sicher, dass es float32 ist
            if audio_segment.dtype != np.float32:
                audio_segment = audio_segment.astype(np.float32)
            
            # Pitch-Shift anwenden mit Pedalboard/Rubberband
            pitch_shifter = PitchShift(semitones=semitones)
            
            # Pedalboard erwartet (channels, samples) Format
            if len(audio_segment.shape) == 1:
                audio_segment = audio_segment.reshape(1, -1)
            else:
                audio_segment = audio_segment.T
            
            # Pitch-Shift anwenden
            pitched_audio = pitch_shifter(audio_segment, self._audio_sr)
            
            # Zurück zu (samples, channels) für pyo
            if pitched_audio.shape[0] <= 2:
                pitched_audio = pitched_audio.T
            
            # Im RAM cachen
            self._pitched_audio_cache = pitched_audio.copy()
            self._cached_speed = self._pending_speed
            
            return True
        except Exception as e:
            logger.error(f"Error pre-caching pitched audio: {type(e).__name__}: {e}")
            return False

    def _stop_all_objects(self):
        """Stoppt alle pyo-Objekte"""
        objects_to_stop = [
            self.player, self._pitched_player,
            self.eq_low, self.eq_mid, self.eq_high, self.output
        ]
        
        for obj in objects_to_stop:
            if obj:
                try:
                    obj.stop()
                except (AttributeError, Exception):
                    # Erwartet: Objekt bereits gestoppt, ungültig, oder hat keine stop()-Methode
                    # Dies ist normal bei Cleanup und kein Fehler
                    pass
        
        if self.follower:
            try:
                self.follower.stop()
            except (AttributeError, Exception):
                # Erwartet: Follower bereits gestoppt oder ungültig
                pass
            self.follower = None
        
        self.player = None
        self._pitched_player = None
        self._pitched_table = None
        self.table = None
        self.eq_low = None
        self.eq_mid = None
        self.eq_high = None
        self.output = None

    def _create_eq_chain(self):
        """Erstellt die EQ-Kette mit optionalem Stem-Mute"""
        if not self.player:
            return
        
        # Wähle den richtigen Player basierend auf Key Lock Status
        if self._key_lock and self._pitched_player:
            signal = self._pitched_player
        else:
            signal = self.player
        
        self.eq_low = EQ(signal, freq=200, q=0.7, 
                        boost=self._get_eq_boost(self._eq_low_val), type=1)
        self.eq_mid = EQ(self.eq_low, freq=1000, q=0.7, 
                        boost=self._get_eq_boost(self._eq_mid_val), type=0)
        self.eq_high = EQ(self.eq_mid, freq=4000, q=0.7, 
                        boost=self._get_eq_boost(self._eq_high_val), type=2)
        
        # Stem-Mute Multiplikator anwenden falls vorhanden
        if self._stem_mute is not None:
            self._final_output = self.eq_high * self._stem_mute
            self.output = self._final_output
        else:
            self.output = self.eq_high

    def _get_eq_boost(self, val):
        """Convert -1..1 to dB boost"""
        if val <= -0.98:
            return -80
        if val >= 0:
            return val * 12
        else:
            normalized = (val + 0.98) / 0.98
            if normalized <= 0:
                return -60
            return -60 * (1 - (normalized ** 0.4))

    def set_eq(self, low, mid, high):
        """Update EQ values"""
        self._eq_low_val = low
        self._eq_mid_val = mid
        self._eq_high_val = high
        
        if self.eq_low:
            self.eq_low.boost = self._get_eq_boost(low)
        if self.eq_mid:
            self.eq_mid.boost = self._get_eq_boost(mid)
        if self.eq_high:
            self.eq_high.boost = self._get_eq_boost(high)

    def set_key_lock(self, enabled):
        """Aktiviert oder deaktiviert Key Lock (Tonhöhenkorrektur bei Tempoänderung)."""
        was_playing = self._is_playing
        old_key_lock = self._key_lock
        self._key_lock = enabled
        
        if enabled != old_key_lock:
            if was_playing:
                # Neu starten mit korrektem Player
                self.stop()
                self._create_player()
                self.play()
            # WICHTIG: Cache NICHT invalidieren wenn Key Lock aktiviert wird!
            # Der Cache bleibt gültig solange sich die Speed nicht ändert
            
            # STEM-SYNC: Stems müssen neu initialisiert werden bei Key Lock Änderung
            self._update_stems_key_lock(enabled)
    
    def _update_stems_key_lock(self, key_lock_enabled):
        """
        Aktualisiert die Stems wenn Key Lock ein- oder ausgeschaltet wird.
        Bei Key Lock: Gepitchte Versionen verwenden
        Ohne Key Lock: Dry Versionen verwenden
        """
        # Finde den button_id für diesen Loop
        button_id = None
        for btn_id, data in button_data.items():
            if data.get("pyo") is self:
                button_id = btn_id
                break
        
        if button_id is None:
            return
        
        data = button_data[button_id]
        
        # Nur wenn Stems verfügbar und initialisiert sind
        if not data["stems"].get("available") or not data["stems"].get("initialized"):
            return
        
        # Stems müssen neu initialisiert werden
        # Erst stoppen, dann neu starten
        stop_stem_players(button_id)
        
        # Wenn Loop aktiv ist und Stems genutzt werden sollen
        if data.get("active") and any(data["stems"]["states"].values()):
            initialize_stem_players(button_id)
            update_stem_gains(button_id)

    def set_stem_mute(self, mute_signal):
        """
        Setzt den Stem-Mute Multiplikator.
        Wird verwendet um den Haupt-Loop stumm zu schalten wenn Stems aktiv sind.
        mute_signal: pyo Sig oder SigTo Objekt (0=stumm, 1=normal)
        """
        old_output = self.output
        self._stem_mute = mute_signal
        
        # EQ-Kette neu aufbauen falls Player existiert und läuft
        if self._is_playing and self.player:
            # Alten Output stoppen
            if old_output:
                safe_stop(old_output, "old_output")
            
            # EQ-Kette mit neuem stem_mute aufbauen
            self._create_eq_chain()
            
            # Neuen Output starten
            if self.output:
                self.output.out()

    def play(self):
        """Startet die Wiedergabe"""
        try:
            if not self._ensure_player():
                return
            self._is_playing = True
            if self.output:
                self.output.out()
            elif self._key_lock and self._pitched_player:
                self._pitched_player.out()
            elif self.player:
                self.player.out()
        except Exception as e:
            logger.error(f"Error playing: {type(e).__name__}: {e}")

    def play_with_intro(self, intro_start):
        """
        Spielt den Loop mit Intro ab.
        
        NEUES KONZEPT:
        - Setzt _intro_start, erstellt Player neu und startet
        - Der Looper kümmert sich automatisch um den Übergang zum Loop
        - Kein Timer, kein zweiter Player, keine Unterbrechung!
        """
        try:
            # intro_start zu native Python float konvertieren
            self._intro_start = float(intro_start)
            
            # Player neu erstellen (mit Intro-Konfiguration)
            self._create_player()
            
            # Starten
            self.play()
            
        except Exception as e:
            logger.error(f"Error playing with intro: {type(e).__name__}: {e}")
    
    def clear_intro(self):
        """Entfernt die Intro-Einstellung für den nächsten Play-Aufruf."""
        self._intro_start = None

    def stop(self):
        """Stoppt die Wiedergabe"""
        try:
            self._is_playing = False
            
            # Intro-Start zurücksetzen für nächsten Play-Aufruf
            self._intro_start = None
            
            # Normale Player stoppen
            if self.output:
                self.output.stop()
            if self.player:
                self.player.stop()
            if self._pitched_player:
                self._pitched_player.stop()
            if self.follower:
                self.follower.stop()
                self.follower = None
            
            self.player = None
            self._pitched_player = None
            self._pitched_table = None
            self.table = None
            self.output = None
        except Exception as e:
            # Fehler beim Stop sind oft nicht kritisch (Objekte bereits gestoppt)
            # aber sollten trotzdem sichtbar sein für Debugging
            logger.warning(f"Error during stop: {type(e).__name__}: {e}")

    def set_gain(self, db):
        """Setzt die Lautstärke in dB"""
        try:
            self._pending_gain = float(db)
            if self._pyo_initialized and self.amp:
                self.amp.value = db_to_amp(float(db))
        except (ValueError, AttributeError) as e:
            logger.warning(f"Error setting gain to {db}dB: {type(e).__name__}: {e}")

    def set_speed(self, value):
        """
        Setzt die Abspielgeschwindigkeit.
        Aktualisiert ALLE Player: PyoLoop, Stems, Master-Phasor.
        """
        try:
            old_speed = self._pending_speed
            self._pending_speed = float(value)
            
            if self._pyo_initialized and self.speed:
                self.speed.value = float(value)
            
            # Speed für den Player setzen - unterschiedlich je nach Player-Typ
            if self._use_table and self.player:
                if hasattr(self.player, 'freq'):
                    # Osc-basierter Player
                    self.player.freq = self._loop_base_freq * float(value)
                elif hasattr(self.player, 'pitch'):
                    # Looper-basierter Player
                    self.player.pitch = float(value)
            
            # Bei Key Lock: Pitched Player aktualisieren
            if self._key_lock and self._is_playing and old_speed != value:
                # Neu berechnen und Player neu erstellen
                self._create_pitched_player()
                if self._pitched_player:
                    # EQ-Kette neu aufbauen
                    self._create_eq_chain()
                    self.output.out()
            
            # STEM-SYNC: Master-Phasor und Stem-Player aktualisieren
            if old_speed != value:
                self._update_stems_speed(float(value))
                
        except Exception as e:
            logger.warning(f"Error setting speed to {value}: {type(e).__name__}: {e}")
    
    def _update_stems_speed(self, new_speed):
        """
        Aktualisiert die Speed für alle Stem-Player.
        Wird von set_speed aufgerufen wenn sich die Geschwindigkeit ändert.
        """
        # Finde den button_id für diesen Loop
        button_id = None
        for btn_id, data in button_data.items():
            if data.get("pyo") is self:
                button_id = btn_id
                break
        
        if button_id is None:
            return
        
        data = button_data[button_id]
        
        # Prüfe ob Stems initialisiert sind
        if not data["stems"].get("initialized", False):
            return
        
        # Master-Phasor Frequenz aktualisieren
        master_phasor = data["stems"].get("master_phasor")
        if master_phasor:
            loop_duration = float(self.loop_end - self.loop_start) if self.loop_end else float(self._duration)
            if loop_duration > 0:
                new_freq = float(new_speed) / loop_duration
                master_phasor.freq = new_freq
        
        # Bei Key Lock: Stem-Caches müssen neu erstellt werden
        if self._key_lock and data["stems"]["cached_speed"] != new_speed:
            # Stems im Hintergrund neu pitchen
            self._schedule_stem_repitch(button_id, new_speed)
    
    def _schedule_stem_repitch(self, button_id, new_speed):
        """
        Plant das Neu-Pitchen der Stems im Hintergrund.
        Wird aufgerufen wenn Key Lock aktiv ist und sich die Speed ändert.
        """
        import tempfile
        
        data = button_data[button_id]
        
        def do_repitch():
            try:
                # Für jeden Stem: neu pitchen
                for stem in STEM_NAMES:
                    dry_audio = data["stems"]["dry"].get(stem)
                    if dry_audio is None:
                        continue
                    
                    # Pitch-Shift berechnen
                    semitones = -12 * np.log2(new_speed) if new_speed > 0 else 0
                    
                    from pedalboard import PitchShift
                    pitch_shifter = PitchShift(semitones=semitones)
                    
                    # Audio vorbereiten
                    audio = dry_audio.copy()
                    if len(audio.shape) == 1:
                        audio = audio.reshape(1, -1)
                    else:
                        audio = audio.T
                    
                    # Pitchen
                    pitched = pitch_shifter(audio, self._audio_sr)
                    
                    if pitched.shape[0] <= 2:
                        pitched = pitched.T
                    
                    data["stems"]["pitched"][stem] = pitched.astype(np.float32)
                
                # Jetzt die Player mit dem neuen Audio aktualisieren
                master_phasor = data["stems"].get("master_phasor")
                if not master_phasor:
                    return
                
                for stem in STEM_NAMES:
                    pitched_audio = data["stems"]["pitched"].get(stem)
                    if pitched_audio is None:
                        continue
                    
                    # Alte Player stoppen
                    old_player = data["stems"]["players"].get(stem)
                    if old_player:
                        safe_stop(old_player, "old_player")
                    
                    # Neue Table erstellen
                    temp_file = tempfile.NamedTemporaryFile(suffix='.wav', delete=False)
                    temp_path = temp_file.name
                    temp_file.close()
                    
                    sf.write(temp_path, pitched_audio, self._audio_sr)
                    
                    new_table = SndTable(temp_path)
                    
                    # Neuen Player erstellen
                    stem_gain = data["stems"]["gains"].get(stem)
                    if stem_gain:
                        new_player = Pointer(
                            table=new_table,
                            index=master_phasor,
                            mul=stem_gain * master_amp
                        )
                        new_player.out()
                        
                        data["stems"]["players"][stem] = new_player
                        data["stems"]["tables"][stem] = new_table
                    
                    safe_delete_file(temp_path, "temp file")
                
                # Main Player auch aktualisieren
                if data["stems"].get("main_player"):
                    safe_stop(data["stems"]["main_player"], "data")
                
                # Main Audio neu pitchen
                if self._pitched_audio_cache is not None:
                    temp_file = tempfile.NamedTemporaryFile(suffix='.wav', delete=False)
                    temp_path = temp_file.name
                    temp_file.close()
                    
                    sf.write(temp_path, self._pitched_audio_cache, self._audio_sr)
                    
                    main_table = SndTable(temp_path)
                    main_gain = data["stems"]["main_gain"]
                    
                    if main_gain:
                        main_player = Pointer(
                            table=main_table,
                            index=master_phasor,
                            mul=main_gain * master_amp
                        )
                        main_player.out()
                        
                        data["stems"]["main_player"] = main_player
                        data["stems"]["main_table"] = main_table
                    
                    safe_delete_file(temp_path, "temp file")
                
                data["stems"]["cached_speed"] = new_speed
                
            except Exception as e:
                logger.error(f"Error repitching stems: {type(e).__name__}: {e}")
        
        # Im Hintergrund ausführen
        io_executor.submit(do_repitch)

    def update_loop_points(self, loop_start, loop_end):
        """Aktualisiert die Loop-Punkte"""
        self.loop_start = float(loop_start)
        self.loop_end = float(loop_end) if loop_end is not None else None
        
        # Pitch-Cache invalidieren weil sich die Loop-Punkte geändert haben
        self._invalidate_pitch_cache()
        
        if self._is_playing:
            try:
                current_speed = self._pending_speed
                current_gain_db = self._pending_gain
                current_key_lock = self._key_lock
                self.stop()
                self._create_player()
                self.set_gain(current_gain_db)
                self.set_speed(current_speed)
                self.play()
            except Exception as e:
                logger.error(f"Error updating loop points: {type(e).__name__}: {e}")
        else:
            self._pyo_initialized = False
            self._stop_all_objects()
            self.amp = None
            self.speed = None

    def enable_level_meter(self):
        """Aktiviert das Level-Metering"""
        source = self._pitched_player if (self._key_lock and self._pitched_player) else self.player
        if source and self.follower is None:
            try:
                self.follower = Follower(source, freq=20)
            except Exception as e:
                logger.warning(f"Error enabling level meter: {type(e).__name__}: {e}")

    def disable_level_meter(self):
        """Deaktiviert das Level-Metering"""
        if self.follower:
            try:
                self.follower.stop()
            except Exception:
                pass  # Follower bereits gestoppt
            self.follower = None

    def get_level_db(self):
        """Gibt den aktuellen Pegel in dB zurück"""
        try:
            source = self._pitched_player if (self._key_lock and self._pitched_player) else self.player
            if self.follower and source and self._is_playing:
                amp = self.follower.get()
                if amp > 0.0001:
                    return 20 * np.log10(amp)
            return -80.0
        except Exception:
            return -80.0  # Sicherer Fallback-Wert


# ============== STEM SEPARATION ==============
def generate_stems(button_id):
    """
    Generiert Stems für einen Loop im Hintergrund.
    Verwendet demucs für hochwertige Audio-Separation.
    
    WICHTIG: Stems können nur bei gestopptem Loop generiert werden!
    Das vermeidet Synchronisationsprobleme und macht die Logik viel einfacher.
    """
    import gc
    
    data = button_data[button_id]
    
    # Stems-Struktur sicherstellen
    ensure_stems_structure(data)
    
    # === BLOCKIERUNG: Loop muss gestoppt sein! ===
    if data["active"]:
        messagebox.showwarning("Loop läuft", 
                              "Bitte stoppe den Loop zuerst, bevor du Stems generierst.\n\n"
                              "Stems können nur bei gestopptem Loop erzeugt werden.")
        return
    
    if not data["file"] or not data["bpm"]:
        messagebox.showwarning("Cannot Generate Stems", 
                              "Audio file and BPM must be set first.")
        return
    
    if data["stems"]["generating"]:
        messagebox.showinfo("Already Generating", 
                           "Stems are already being generated for this loop.")
        return
    
    # Bei Regenerate: Alte Stems und Player zuerst freigeben
    if data["stems"]["available"]:
        # Stoppe alle Player
        if data["stems"].get("master_phasor"):
            safe_stop(data["stems"]["master_phasor"], "data")
            data["stems"]["master_phasor"] = None
        
        if data["stems"].get("main_player"):
            safe_stop(data["stems"]["main_player"], "data")
            data["stems"]["main_player"] = None
        data["stems"]["main_table"] = None
        
        for stem in STEM_NAMES:
            player = data["stems"]["players"].get(stem)
            if player:
                safe_stop(player, "player")
                data["stems"]["players"][stem] = None
            data["stems"]["tables"][stem] = None
            data["stems"]["outputs"][stem] = None
            if data["stems"]["gains"].get(stem):
                safe_stop(data["stems"]["gains"][stem], "data")
                data["stems"]["gains"][stem] = None
            data["stems"]["dry"][stem] = None
            data["stems"]["pitched"][stem] = None
        
        if data["stems"].get("main_gain"):
            safe_stop(data["stems"]["main_gain"], "data")
            data["stems"]["main_gain"] = None
        
        data["stems"]["initialized"] = False
        data["stems"]["available"] = False
        gc.collect()
    
    # Setze generating Flag
    data["stems"]["generating"] = True
    
    # Visuelles Feedback
    original_bg = buttons[button_id].cget('bg')
    buttons[button_id].config(bg="#ff8800")
    
    def do_generate():
        try:
            loop = data.get("pyo")
            if not loop or loop._audio_data is None:
                raise Exception("Audio data not loaded")
            
            # Audio-Segment extrahieren (Loop-Bereich)
            audio_data = loop._audio_data
            audio_sr = loop._audio_sr
            loop_start = data.get("loop_start", 0.0)
            loop_end = data.get("loop_end", loop._duration)
            
            start_sample = int(loop_start * audio_sr)
            end_sample = int(loop_end * audio_sr)
            audio_segment = audio_data[start_sample:end_sample]
            
            # Stelle sicher, dass Audio float32 ist
            if audio_segment.dtype != np.float32:
                audio_segment = audio_segment.astype(np.float32)
            
            # Demucs für Separation verwenden
            import torch
            from demucs.pretrained import get_model
            from demucs.apply import apply_model
            
            # Modell laden (htdemucs_6s hat: vocals, drums, bass, guitar, piano, other)
            # Wir verwenden htdemucs für 4-stem: vocals, drums, bass, other
            model = get_model('htdemucs')
            model.eval()
            
            # Audio vorbereiten für demucs
            # Erwartet: (batch, channels, samples)
            if len(audio_segment.shape) == 1:
                # Mono -> Stereo
                audio_tensor = torch.from_numpy(audio_segment).unsqueeze(0).repeat(2, 1)
            else:
                # (samples, channels) -> (channels, samples)
                audio_tensor = torch.from_numpy(audio_segment.T)
            
            # Batch-Dimension hinzufügen
            audio_tensor = audio_tensor.unsqueeze(0)
            
            # Resample falls nötig (demucs erwartet 44100)
            if audio_sr != 44100:
                import torchaudio
                resampler = torchaudio.transforms.Resample(audio_sr, 44100)
                audio_tensor = resampler(audio_tensor)
            
            # Separation durchführen
            with torch.no_grad():
                sources = apply_model(model, audio_tensor, device='cpu', progress=False)
            
            # sources shape: (batch, sources, channels, samples)
            # htdemucs sources: drums, bass, other, vocals
            sources = sources.squeeze(0).numpy()
            
            # Stems extrahieren (htdemucs: drums=0, bass=1, other=2, vocals=3)
            drums = sources[0].T  # (samples, channels)
            bass = sources[1].T
            other = sources[2].T  # "melody" - enthält Melodie-Instrumente
            vocals = sources[3].T
            
            # Instrumental = alles außer Vocals (drums + bass + other)
            instrumental = drums + bass + other
            
            # Stems als float32 speichern
            stems_dry = {
                "vocals": vocals.astype(np.float32),
                "melody": other.astype(np.float32),
                "bass": bass.astype(np.float32),
                "drums": drums.astype(np.float32),
                "instrumental": instrumental.astype(np.float32)
            }
            
            # GUI Update
            def update_gui():
                data["stems"]["dry"] = stems_dry
                data["stems"]["available"] = True
                data["stems"]["generating"] = False
                buttons[button_id].config(bg=original_bg)
                update_button_label(button_id)
                update_stem_buttons_state()
                save_config_async()
                # Keine MessageBox mehr - stört Live-Performance
            
            schedule_gui_update(update_gui)
            
        except ImportError as e:
            def show_error():
                data["stems"]["generating"] = False
                buttons[button_id].config(bg=original_bg)
                messagebox.showerror("Missing Dependencies", 
                    f"Please install required packages:\npip install demucs torch torchaudio\n\nError: {e}")
            schedule_gui_update(show_error)
        except Exception as e:
            def show_error():
                data["stems"]["generating"] = False
                buttons[button_id].config(bg=original_bg)
                messagebox.showerror("Stem Generation Failed", f"Error: {e}")
                logger.error(f"Stem generation failed: {type(e).__name__}: {e}")
            schedule_gui_update(show_error)
    
    io_executor.submit(do_generate)


def delete_stems(button_id):
    """
    Löscht alle Stems eines Loops und gibt RAM frei.
    WICHTIG: Explizit alle Referenzen auf None setzen für Garbage Collection!
    """
    import gc
    
    data = button_data[button_id]
    
    # Stems-Struktur sicherstellen
    ensure_stems_structure(data)
    
    # Master-Phasor stoppen und freigeben
    if data["stems"].get("master_phasor"):
        safe_stop(data["stems"]["master_phasor"], "data")
        data["stems"]["master_phasor"] = None
    
    # Alle Stem-Player stoppen und Referenzen explizit freigeben
    for stem in STEM_NAMES:
        # Player stoppen
        player = data["stems"]["players"].get(stem)
        if player:
            safe_stop(player, "player")
            data["stems"]["players"][stem] = None
        
        # Table freigeben
        table = data["stems"]["tables"].get(stem)
        if table:
            data["stems"]["tables"][stem] = None
        
        # Gain-Signal freigeben
        gain = data["stems"]["gains"].get(stem)
        if gain:
            safe_stop(gain, "gain")
            data["stems"]["gains"][stem] = None
        
        # Output freigeben
        data["stems"]["outputs"][stem] = None
        
        # WICHTIG: Numpy Arrays explizit freigeben
        data["stems"]["dry"][stem] = None
        data["stems"]["pitched"][stem] = None
    
    # Main-Gain freigeben
    if data["stems"].get("main_gain"):
        safe_stop(data["stems"]["main_gain"], "data")
        data["stems"]["main_gain"] = None
    
    # Main Player freigeben (synchroner Original-Player)
    if data["stems"].get("main_player"):
        safe_stop(data["stems"]["main_player"], "data")
        data["stems"]["main_player"] = None
    data["stems"]["main_table"] = None
    
    # Master Phasor freigeben
    if data["stems"].get("master_phasor"):
        safe_stop(data["stems"]["master_phasor"], "data")
        data["stems"]["master_phasor"] = None
    
    # PyoLoop stem_mute zurücksetzen
    loop = data.get("pyo")
    if loop:
        loop._stem_mute = None
    
    # Flags zurücksetzen
    data["stems"]["available"] = False
    data["stems"]["generating"] = False
    data["stems"]["initialized"] = False
    data["stems"]["cached_speed"] = None
    data["stems"]["stop_active"] = False
    data["stems"]["saved_states"] = None
    
    # Alle States zurücksetzen
    for stem in STEM_NAMES:
        data["stems"]["states"][stem] = False
    
    # Garbage Collection erzwingen
    gc.collect()
    
    # Wenn der Loop noch läuft, PyoLoop wieder aktivieren
    loop = data.get("pyo")
    if loop and data.get("active"):
        # Stoppe und starte den Loop neu, damit er wieder hörbar ist
        loop._stem_mute = None
        # EQ-Kette neu aufbauen ohne stem_mute
        if loop._is_playing:
            loop._create_eq_chain()
            if loop.output:
                loop.output.out()
    
    # GUI Update
    update_button_label(button_id)
    update_stem_buttons_state()
    save_config_async()


def on_stem_toggle(stem):
    """
    Handler für Stem-Toggle (Linksklick).
    Vocals, Melody, Bass, Drums sind frei kombinierbar.
    Instrumental hat Sonderrolle (exklusiv).
    
    WICHTIG: Ändert NUR die Gains - Player müssen bereits initialisiert sein!
    Stems werden beim Loop-Start initialisiert, nicht hier.
    """
    # Finde aktiven Loop
    active_button_id = None
    for btn_id, data in button_data.items():
        if data["active"] and data["stems"]["available"]:
            active_button_id = btn_id
            break
    
    if active_button_id is None:
        return
    
    data = button_data[active_button_id]
    states = data["stems"]["states"]
    
    # Prüfe ob Stem-Player initialisiert sind
    # (werden beim Loop-Start automatisch initialisiert)
    if not data["stems"]["initialized"]:
        # Sollte nicht passieren, da Stems beim Start initialisiert werden
        logger.warning("Stems not initialized - this should not happen")
        return
    
    if stem == "instrumental":
        # Instrumental: exklusives Verhalten
        if states["instrumental"]:
            # War aktiv -> alle aus
            for s in STEM_NAMES:
                states[s] = False
        else:
            # War inaktiv -> nur instrumental an, alle anderen aus
            for s in STEM_NAMES:
                states[s] = False
            states["instrumental"] = True
    else:
        # Vocals/Melody/Bass/Drums: frei kombinierbar
        if states["instrumental"]:
            # Instrumental war aktiv -> ausschalten
            states["instrumental"] = False
        # Toggle aktuellen Stem
        states[stem] = not states[stem]
    
    # Gains aktualisieren (NICHT Player neu starten!)
    update_stem_gains(active_button_id)
    update_stem_buttons_state()
    save_config_async()


def on_stem_momentary_activate(stem, activate=True):
    """
    Handler für temporäres Aktivieren/Deaktivieren eines Stems.
    Wird bei Rechtsklick (activate) / Mittelklick (deactivate) verwendet.
    """
    active_button_id = None
    for btn_id, data in button_data.items():
        if data["active"] and data["stems"]["available"]:
            active_button_id = btn_id
            break
    
    if active_button_id is None:
        return
    
    data = button_data[active_button_id]
    
    # Prüfe ob Stem-Player initialisiert sind
    if not data["stems"]["initialized"]:
        return
    
    # Gain direkt setzen ohne State zu ändern
    gain_sig = data["stems"]["gains"].get(stem)
    if gain_sig:
        if activate:
            gain_sig.value = 1.0
            # Bei Stem-Aktivierung: Haupt-Loop stumm
            if data["stems"]["main_gain"]:
                data["stems"]["main_gain"].value = 0.0
        else:
            gain_sig.value = 0.0
            # Prüfe ob noch andere Stems (permanent) aktiv sind oder temporär aktiviert
            # Hier vereinfacht: Nur States prüfen
            any_state_active = any(data["stems"]["states"].values())
            # Wenn kein anderer Stem aktiv: Haupt-Loop wieder an
            if not any_state_active:
                if data["stems"]["main_gain"]:
                    data["stems"]["main_gain"].value = 1.0


def on_stem_momentary_release(stem):
    """
    Handler für Loslassen der temporären Aktivierung.
    Stellt den ursprünglichen State wieder her.
    """
    active_button_id = None
    for btn_id, data in button_data.items():
        if data["active"] and data["stems"]["available"]:
            active_button_id = btn_id
            break
    
    if active_button_id is None:
        return
    
    # Gains basierend auf gespeichertem State wiederherstellen
    update_stem_gains(active_button_id)


def on_stop_stem_toggle():
    """
    Handler für Stop-Stem Button (Linksklick).
    Speichert aktuelle Stem-States und schaltet alle aus (Original spielt).
    Bei erneutem Klick werden die gespeicherten States wiederhergestellt.
    """
    active_button_id = None
    for btn_id, data in button_data.items():
        if data["active"] and data["stems"]["available"]:
            active_button_id = btn_id
            break
    
    if active_button_id is None:
        return
    
    data = button_data[active_button_id]
    
    # Stem-Player initialisieren falls noch nicht geschehen
    if not data["stems"]["initialized"]:
        initialize_stem_players(active_button_id)
    
    if data["stems"].get("stop_active", False):
        # Stop war aktiv -> Gespeicherte States wiederherstellen
        saved = data["stems"].get("saved_states")
        if saved:
            for stem in STEM_NAMES:
                data["stems"]["states"][stem] = saved.get(stem, False)
        data["stems"]["stop_active"] = False
        data["stems"]["saved_states"] = None
    else:
        # Stop aktivieren -> Aktuelle States speichern und alle aus
        data["stems"]["saved_states"] = data["stems"]["states"].copy()
        for stem in STEM_NAMES:
            data["stems"]["states"][stem] = False
        data["stems"]["stop_active"] = True
    
    # Gains aktualisieren
    update_stem_gains(active_button_id)
    update_stem_buttons_state()


def on_stop_stem_momentary(activate=True):
    """
    Handler für temporäres Aktivieren des Stop-Stem (Rechtsklick gedrückt).
    Schaltet alle Stems temporär aus (Original spielt).
    """
    active_button_id = None
    for btn_id, data in button_data.items():
        if data["active"] and data["stems"]["available"]:
            active_button_id = btn_id
            break
    
    if active_button_id is None:
        return
    
    data = button_data[active_button_id]
    
    # Prüfe ob Stem-Player initialisiert sind
    if not data["stems"]["initialized"]:
        initialize_stem_players(active_button_id)
    
    if activate:
        # Alle Stems temporär aus, Original an
        for stem in STEM_NAMES:
            gain_sig = data["stems"]["gains"].get(stem)
            if gain_sig:
                gain_sig.value = 0.0
        if data["stems"]["main_gain"]:
            data["stems"]["main_gain"].value = 1.0
    else:
        # Gespeicherte States wiederherstellen
        update_stem_gains(active_button_id)


def on_stop_stem_momentary_release():
    """
    Handler für Loslassen des Stop-Stem Buttons.
    Stellt die ursprünglichen Stem-States wieder her.
    """
    active_button_id = None
    for btn_id, data in button_data.items():
        if data["active"] and data["stems"]["available"]:
            active_button_id = btn_id
            break
    
    if active_button_id is None:
        return
    
    # Gains basierend auf gespeichertem State wiederherstellen
    update_stem_gains(active_button_id)
    update_stem_buttons_state()


def apply_stem_mix(button_id):
    """
    Wendet den aktuellen Stem-Mix an.
    Initialisiert Stem-Player falls nötig und aktualisiert Gains.
    """
    data = button_data[button_id]
    loop = data.get("pyo")
    
    if not loop or not data["stems"]["available"]:
        return
    
    # Stem-Player initialisieren falls noch nicht geschehen
    if not data["stems"]["initialized"]:
        initialize_stem_players(button_id)
    
    # Gains aktualisieren
    update_stem_gains(button_id)


def update_stem_gains(button_id):
    """
    Aktualisiert die Gain-Werte basierend auf den Stem-States.
    Verwendet SigTo für click-freie Übergänge (10ms Fade).
    """
    data = button_data[button_id]
    states = data["stems"]["states"]
    any_stem_active = any(states.values())
    
    # Haupt-Loop Gain: 0 wenn irgendein Stem aktiv, sonst 1
    main_gain = data["stems"]["main_gain"]
    if main_gain:
        main_gain.value = 0.0 if any_stem_active else 1.0
    
    # Stem-Gains basierend auf States
    for stem in STEM_NAMES:
        gain_sig = data["stems"]["gains"].get(stem)
        if gain_sig:
            gain_sig.value = 1.0 if states.get(stem, False) else 0.0


def update_stem_eq(button_id, low, mid, high):
    """
    Aktualisiert die EQ-Werte für die Stem-Player.
    Wird aufgerufen wenn der EQ-Regler bewegt wird.
    """
    data = button_data[button_id]
    
    if not data["stems"].get("initialized"):
        return
    
    # PyoLoop für _get_eq_boost Methode
    loop = data.get("pyo")
    if not loop:
        return
    
    # EQ-Werte aktualisieren
    if data["stems"].get("eq_low"):
        data["stems"]["eq_low"].boost = loop._get_eq_boost(low)
    if data["stems"].get("eq_mid"):
        data["stems"]["eq_mid"].boost = loop._get_eq_boost(mid)
    if data["stems"].get("eq_high"):
        data["stems"]["eq_high"].boost = loop._get_eq_boost(high)


def initialize_stem_players(button_id):
    """
    Initialisiert alle Stem-Player UND einen synchronen Main-Player.
    
    ARCHITEKTUR:
    - Ein gemeinsamer Phasor dient als Master-Clock für ALLE Player
    - ALLE Player (Stems + Main) nutzen Pointer mit diesem Phasor als Index
    - Alle Outputs werden in einen Mix gesammelt
    - Der Mix geht durch EQ → Button-Gain → Output
    - Dadurch funktionieren EQ und Gain-Regler auch bei Stems!
    
    OPTIMIERUNG:
    - Tables werden im RAM gehalten und wiederverwendet
    - Nur Phasor, Pointer und Output-Kette werden neu erstellt
    """
    import tempfile
    
    data = button_data[button_id]
    loop = data.get("pyo")
    
    # Stems-Struktur sicherstellen
    ensure_stems_structure(data)
    
    if not loop or not data["stems"]["available"]:
        return
    
    current_speed = float(loop._pending_speed)
    use_key_lock = loop._key_lock
    
    # Loop-Dauer berechnen (als Python float für pyo Kompatibilität)
    loop_start_time = float(loop.loop_start)
    loop_end_time = float(loop.loop_end) if loop.loop_end else float(loop._duration)
    loop_duration = loop_end_time - loop_start_time
    
    if loop_duration <= 0:
        logger.error("Invalid loop duration for stems")
        return
    
    # Master-Phasor Frequenz berechnen
    master_freq = current_speed / loop_duration
    
    # Prüfe ob Tables neu erstellt werden müssen
    need_new_tables = False
    
    # Bei Key Lock: Prüfe ob Speed sich geändert hat
    if use_key_lock and current_speed != 1.0:
        if data["stems"]["cached_speed"] != current_speed:
            need_new_tables = True
    else:
        # Ohne Key Lock: Prüfe ob wir von Key Lock kommen (Speed hat sich effektiv geändert)
        if data["stems"]["cached_speed"] is not None and data["stems"]["cached_speed"] != 1.0:
            if use_key_lock != True:  # Wir sind jetzt ohne Key Lock
                need_new_tables = True
    
    # Prüfe ob Main-Table existiert
    if data["stems"].get("main_table") is None:
        need_new_tables = True
    
    # Prüfe ob alle Stem-Tables existieren
    for stem in STEM_NAMES:
        if data["stems"]["dry"].get(stem) is not None:
            if data["stems"]["tables"].get(stem) is None:
                need_new_tables = True
                break
    
    # === CLEANUP: Alte Player stoppen (aber Tables behalten!) ===
    # Final Output stoppen
    if data["stems"].get("final_output"):
        safe_stop(data["stems"]["final_output"], "data")
        data["stems"]["final_output"] = None
    
    # EQ-Kette stoppen
    for eq_name in ["eq_low", "eq_mid", "eq_high"]:
        if data["stems"].get(eq_name):
            safe_stop(data["stems"][eq_name], "data")
            data["stems"][eq_name] = None
    
    # Mix stoppen
    if data["stems"].get("stem_mix"):
        safe_stop(data["stems"]["stem_mix"], "data")
        data["stems"]["stem_mix"] = None
    
    # Phasor stoppen
    if data["stems"].get("master_phasor"):
        safe_stop(data["stems"]["master_phasor"], "data")
        data["stems"]["master_phasor"] = None
    
    # Player stoppen (aber Tables behalten wenn möglich!)
    if data["stems"].get("main_player"):
        safe_stop(data["stems"]["main_player"], "data")
        data["stems"]["main_player"] = None
    
    for stem in STEM_NAMES:
        if data["stems"]["players"].get(stem):
            safe_stop(data["stems"]["players"][stem], "data")
            data["stems"]["players"][stem] = None
        
        # Gains müssen auch neu erstellt werden (da mul sich ändert)
        if data["stems"]["gains"].get(stem):
            safe_stop(data["stems"]["gains"][stem], "data")
            data["stems"]["gains"][stem] = None
    
    if data["stems"].get("main_gain"):
        safe_stop(data["stems"]["main_gain"], "data")
        data["stems"]["main_gain"] = None
    
    # === TABLES ERSTELLEN (nur wenn nötig) ===
    if need_new_tables:
        # Main Table
        main_audio = None
        if use_key_lock and current_speed != 1.0 and loop._pitched_audio_cache is not None:
            main_audio = loop._pitched_audio_cache
        else:
            if loop._audio_data is not None:
                start_sample = int(loop_start_time * loop._audio_sr)
                end_sample = int(loop_end_time * loop._audio_sr)
                main_audio = loop._audio_data[start_sample:end_sample]
                if main_audio.dtype != np.float32:
                    main_audio = main_audio.astype(np.float32)
        
        if main_audio is not None:
            temp_file = tempfile.NamedTemporaryFile(suffix='.wav', delete=False)
            temp_path = temp_file.name
            temp_file.close()
            sf.write(temp_path, main_audio, loop._audio_sr)
            data["stems"]["main_table"] = SndTable(temp_path)
            safe_delete_file(temp_path, "temp file")
        
        # Stem Tables
        for stem in STEM_NAMES:
            dry_audio = data["stems"]["dry"].get(stem)
            if dry_audio is None:
                continue
            
            if use_key_lock and current_speed != 1.0:
                if data["stems"]["pitched"].get(stem) is None:
                    _create_pitched_stem_cache(button_id, stem, current_speed)
                audio_to_use = data["stems"]["pitched"].get(stem)
            else:
                audio_to_use = dry_audio
            
            if audio_to_use is None:
                continue
            
            temp_file = tempfile.NamedTemporaryFile(suffix='.wav', delete=False)
            temp_path = temp_file.name
            temp_file.close()
            sf.write(temp_path, audio_to_use, loop._audio_sr)
            data["stems"]["tables"][stem] = SndTable(temp_path)
            safe_delete_file(temp_path, "temp file")
        
        data["stems"]["cached_speed"] = current_speed
    
    # === PLAYER ERSTELLEN (immer neu) ===
    
    # Neuer Master-Phasor
    data["stems"]["master_phasor"] = Phasor(freq=master_freq)
    master_phasor = data["stems"]["master_phasor"]
    
    # Alle Player sammeln für den Mix
    all_players = []
    
    # Main-Gain
    data["stems"]["main_gain"] = SigTo(value=1.0, time=0.015, init=1.0)
    main_gain = data["stems"]["main_gain"]
    
    # PyoLoop stumm schalten
    loop.set_stem_mute(Sig(0))
    
    # Main Player erstellen (mit existierender Table)
    main_table = data["stems"].get("main_table")
    if main_table:
        main_player = Pointer(
            table=main_table,
            index=master_phasor,
            mul=main_gain
        )
        all_players.append(main_player)
        data["stems"]["main_player"] = main_player
    
    # Stem Player erstellen (mit existierenden Tables)
    for stem in STEM_NAMES:
        table = data["stems"]["tables"].get(stem)
        if table is None:
            continue
        
        # Gain-Signal erstellen
        data["stems"]["gains"][stem] = SigTo(value=0.0, time=0.015, init=0.0)
        stem_gain = data["stems"]["gains"][stem]
        
        # Stem Player erstellen
        player = Pointer(
            table=table,
            index=master_phasor,
            mul=stem_gain
        )
        all_players.append(player)
        data["stems"]["players"][stem] = player
    
    # === MIX → EQ → GAIN → OUTPUT ===
    if all_players:
        # Alle Player mixen
        if len(all_players) == 1:
            stem_mix = all_players[0]
        else:
            stem_mix = Mix(all_players, voices=len(all_players))
        
        # EQ-Kette anwenden (gleiche Werte wie PyoLoop)
        eq_low = EQ(stem_mix, freq=200, q=0.7, 
                    boost=loop._get_eq_boost(loop._eq_low_val), type=1)
        eq_mid = EQ(eq_low, freq=1000, q=0.7, 
                    boost=loop._get_eq_boost(loop._eq_mid_val), type=0)
        eq_high = EQ(eq_mid, freq=4000, q=0.7, 
                    boost=loop._get_eq_boost(loop._eq_high_val), type=2)
        
        # Button-Gain und Master-Gain anwenden
        final_output = eq_high * loop.amp * master_amp
        
        # Output starten
        final_output.out()
        
        # Speichern für spätere Updates
        data["stems"]["stem_mix"] = stem_mix
        data["stems"]["eq_low"] = eq_low
        data["stems"]["eq_mid"] = eq_mid
        data["stems"]["eq_high"] = eq_high
        data["stems"]["final_output"] = final_output
    
    data["stems"]["initialized"] = True
    
    # Initiale Gains setzen
    update_stem_gains(button_id)


def _initialize_stems_while_running(button_id):
    """
    Initialisiert Stems während der Loop bereits läuft.
    
    Das ist der Fall wenn der User Stems lädt während ein Loop spielt,
    und dann einen Stem-Button drückt.
    
    WICHTIG: Der Phasor wird mit einer geschätzten Phase gestartet,
    basierend auf der aktuellen Zeit. Das ist nicht 100% exakt, aber
    gut genug für den Use Case "Stems während des Spielens aktivieren".
    """
    import tempfile
    import time
    
    data = button_data[button_id]
    loop = data.get("pyo")
    
    # Stems-Struktur sicherstellen
    ensure_stems_structure(data)
    
    if not loop or not data["stems"]["available"]:
        return
    
    current_speed = float(loop._pending_speed)
    use_key_lock = loop._key_lock
    
    # Loop-Dauer berechnen
    loop_start_time = float(loop.loop_start)
    loop_end_time = float(loop.loop_end) if loop.loop_end else float(loop._duration)
    loop_duration = loop_end_time - loop_start_time
    
    if loop_duration <= 0:
        return
    
    # Frequenz berechnen
    master_freq = current_speed / loop_duration
    
    # === TABLES ERSTELLEN (wenn nötig) ===
    # Main Table
    if data["stems"].get("main_table") is None:
        main_audio = None
        if use_key_lock and current_speed != 1.0 and loop._pitched_audio_cache is not None:
            main_audio = loop._pitched_audio_cache
        else:
            if loop._audio_data is not None:
                start_sample = int(loop_start_time * loop._audio_sr)
                end_sample = int(loop_end_time * loop._audio_sr)
                main_audio = loop._audio_data[start_sample:end_sample]
                if main_audio.dtype != np.float32:
                    main_audio = main_audio.astype(np.float32)
        
        if main_audio is not None:
            temp_file = tempfile.NamedTemporaryFile(suffix='.wav', delete=False)
            temp_path = temp_file.name
            temp_file.close()
            sf.write(temp_path, main_audio, loop._audio_sr)
            data["stems"]["main_table"] = SndTable(temp_path)
            safe_delete_file(temp_path, "temp file")
    
    # Stem Tables
    for stem in STEM_NAMES:
        if data["stems"]["tables"].get(stem) is not None:
            continue
        
        dry_audio = data["stems"]["dry"].get(stem)
        if dry_audio is None:
            continue
        
        if use_key_lock and current_speed != 1.0:
            if data["stems"]["pitched"].get(stem) is None:
                _create_pitched_stem_cache(button_id, stem, current_speed)
            audio_to_use = data["stems"]["pitched"].get(stem)
        else:
            audio_to_use = dry_audio
        
        if audio_to_use is None:
            continue
        
        temp_file = tempfile.NamedTemporaryFile(suffix='.wav', delete=False)
        temp_path = temp_file.name
        temp_file.close()
        sf.write(temp_path, audio_to_use, loop._audio_sr)
        data["stems"]["tables"][stem] = SndTable(temp_path)
        safe_delete_file(temp_path, "temp file")
    
    data["stems"]["cached_speed"] = current_speed
    
    # === PHASOR UND PLAYER ERSTELLEN ===
    
    # Phasor mit geschätzter Phase starten
    # Wir verwenden time.time() modulo Loop-Periode als Schätzung
    loop_period = loop_duration / current_speed
    estimated_phase = (time.time() % loop_period) / loop_period
    
    data["stems"]["master_phasor"] = Phasor(freq=master_freq, phase=estimated_phase)
    master_phasor = data["stems"]["master_phasor"]
    
    # Alle Player sammeln
    all_players = []
    
    # Main-Gain
    data["stems"]["main_gain"] = SigTo(value=1.0, time=0.015, init=1.0)
    main_gain = data["stems"]["main_gain"]
    
    # PyoLoop stumm schalten
    loop.set_stem_mute(Sig(0))
    
    # Main Player
    main_table = data["stems"].get("main_table")
    if main_table:
        main_player = Pointer(
            table=main_table,
            index=master_phasor,
            mul=main_gain
        )
        all_players.append(main_player)
        data["stems"]["main_player"] = main_player
    
    # Stem Players
    for stem in STEM_NAMES:
        table = data["stems"]["tables"].get(stem)
        if table is None:
            continue
        
        data["stems"]["gains"][stem] = SigTo(value=0.0, time=0.015, init=0.0)
        stem_gain = data["stems"]["gains"][stem]
        
        player = Pointer(
            table=table,
            index=master_phasor,
            mul=stem_gain
        )
        all_players.append(player)
        data["stems"]["players"][stem] = player
    
    # === MIX → EQ → OUTPUT ===
    if all_players:
        if len(all_players) == 1:
            stem_mix = all_players[0]
        else:
            stem_mix = Mix(all_players, voices=len(all_players))
        
        eq_low = EQ(stem_mix, freq=200, q=0.7, 
                    boost=loop._get_eq_boost(loop._eq_low_val), type=1)
        eq_mid = EQ(eq_low, freq=1000, q=0.7, 
                    boost=loop._get_eq_boost(loop._eq_mid_val), type=0)
        eq_high = EQ(eq_mid, freq=4000, q=0.7, 
                    boost=loop._get_eq_boost(loop._eq_high_val), type=2)
        
        final_output = eq_high * loop.amp * master_amp
        final_output.out()
        
        data["stems"]["stem_mix"] = stem_mix
        data["stems"]["eq_low"] = eq_low
        data["stems"]["eq_mid"] = eq_mid
        data["stems"]["eq_high"] = eq_high
        data["stems"]["final_output"] = final_output
    
    data["stems"]["initialized"] = True
    update_stem_gains(button_id)


def _cleanup_stem_players(button_id):
    """
    Hilfsfunktion: Räumt alte Stem-Player auf ohne Stems zu löschen.
    
    WICHTIG: Tables werden NICHT gelöscht! Das ermöglicht schnellen Retrigger,
    da nur neue Pointer/Phasor erstellt werden müssen, nicht die Tables.
    """
    data = button_data[button_id]
    
    # Output stoppen
    if data["stems"].get("final_output"):
        safe_stop(data["stems"]["final_output"], "data")
        data["stems"]["final_output"] = None
    
    # EQ stoppen
    for eq_name in ["eq_low", "eq_mid", "eq_high"]:
        if data["stems"].get(eq_name):
            safe_stop(data["stems"][eq_name], "data")
            data["stems"][eq_name] = None
    
    # Mix stoppen
    if data["stems"].get("stem_mix"):
        safe_stop(data["stems"]["stem_mix"], "data")
        data["stems"]["stem_mix"] = None
    
    # Master-Phasor stoppen
    if data["stems"].get("master_phasor"):
        safe_stop(data["stems"]["master_phasor"], "data")
        data["stems"]["master_phasor"] = None
    
    # Main Player stoppen (TABLE BLEIBT!)
    if data["stems"].get("main_player"):
        safe_stop(data["stems"]["main_player"], "data")
        data["stems"]["main_player"] = None
    # NICHT: data["stems"]["main_table"] = None  # Table behalten!
    
    # Main Gain stoppen
    if data["stems"].get("main_gain"):
        safe_stop(data["stems"]["main_gain"], "data")
        data["stems"]["main_gain"] = None
    
    # Stem Player und Gains stoppen (TABLES BLEIBEN!)
    for stem in STEM_NAMES:
        player = data["stems"]["players"].get(stem)
        if player:
            safe_stop(player, "player")
            data["stems"]["players"][stem] = None
        
        gain = data["stems"]["gains"].get(stem)
        if gain:
            safe_stop(gain, "gain")
            data["stems"]["gains"][stem] = None
        
        # NICHT: data["stems"]["tables"][stem] = None  # Tables behalten!
        data["stems"]["outputs"][stem] = None
    
    data["stems"]["initialized"] = False
    
    data["stems"]["initialized"] = False


def stop_stem_players(button_id):
    """Stoppt alle Stem-Player und gibt Ressourcen frei."""
    data = button_data[button_id]
    
    # Stems-Struktur sicherstellen
    ensure_stems_structure(data)
    
    # Final Output stoppen (wichtig: zuerst!)
    if data["stems"].get("final_output"):
        safe_stop(data["stems"]["final_output"], "data")
        data["stems"]["final_output"] = None
    
    # EQ-Kette stoppen
    for eq_name in ["eq_low", "eq_mid", "eq_high"]:
        if data["stems"].get(eq_name):
            safe_stop(data["stems"][eq_name], "data")
            data["stems"][eq_name] = None
    
    # Mix stoppen
    if data["stems"].get("stem_mix"):
        safe_stop(data["stems"]["stem_mix"], "data")
        data["stems"]["stem_mix"] = None
    
    # Master-Phasor stoppen
    if data["stems"].get("master_phasor"):
        safe_stop(data["stems"]["master_phasor"], "data")
        data["stems"]["master_phasor"] = None
    
    # Main Player stoppen (synchroner Original-Loop Player)
    if data["stems"].get("main_player"):
        safe_stop(data["stems"]["main_player"], "data")
        data["stems"]["main_player"] = None
    data["stems"]["main_table"] = None
    
    for stem in STEM_NAMES:
        player = data["stems"]["players"].get(stem)
        if player:
            safe_stop(player, "player")
            data["stems"]["players"][stem] = None
        
        gain = data["stems"]["gains"].get(stem)
        if gain:
            safe_stop(gain, "gain")
            data["stems"]["gains"][stem] = None
        
        data["stems"]["tables"][stem] = None
        data["stems"]["outputs"][stem] = None
    
    # PyoLoop stem_mute zurücksetzen
    loop = data.get("pyo")
    if loop:
        loop._stem_mute = None
    
    # Main-Gain stoppen
    if data["stems"].get("main_gain"):
        safe_stop(data["stems"]["main_gain"], "data")
    data["stems"]["main_gain"] = None
    data["stems"]["initialized"] = False


def _activate_main_loop(button_id):
    """Aktiviert den Haupt-Loop durch Setzen des Main-Gains auf 1."""
    data = button_data[button_id]
    
    if data["stems"]["main_gain"]:
        data["stems"]["main_gain"].value = 1.0
    
    # Alle Stem-Gains auf 0
    for stem in STEM_NAMES:
        gain_sig = data["stems"]["gains"].get(stem)
        if gain_sig:
            gain_sig.value = 0.0


def _activate_stem_players(button_id):
    """Aktiviert Stem-Player basierend auf States durch Gain-Änderung."""
    data = button_data[button_id]
    
    # Haupt-Loop stumm
    if data["stems"]["main_gain"]:
        data["stems"]["main_gain"].value = 0.0
    
    # Stem-Gains setzen
    for stem in STEM_NAMES:
        gain_sig = data["stems"]["gains"].get(stem)
        if gain_sig:
            gain_sig.value = 1.0 if data["stems"]["states"].get(stem, False) else 0.0


def _restart_stem_phasor(button_id):
    """
    FAST PATH für Retrigger: Aktualisiert nur die Phasor-Frequenz.
    Die Stems laufen "im Takt" weiter - das ist für DJ-Looper erwünscht!
    
    Bei Retrigger wird der Loop von vorne gestartet, aber die Stems
    behalten ihre Phase. Das ermöglicht schnelle, latenzfreie Trigger.
    """
    data = button_data[button_id]
    loop = data.get("pyo")
    
    if not loop or not data["stems"]["initialized"]:
        return
    
    # Loop-Dauer und Frequenz berechnen (als Python float für pyo Kompatibilität)
    loop_start_time = float(loop.loop_start)
    loop_end_time = float(loop.loop_end) if loop.loop_end else float(loop._duration)
    loop_duration = loop_end_time - loop_start_time
    
    if loop_duration <= 0:
        return
    
    current_speed = float(loop._pending_speed)
    master_freq = current_speed / loop_duration
    
    # Nur die Frequenz aktualisieren (falls Speed sich geändert hat)
    if data["stems"].get("master_phasor"):
        data["stems"]["master_phasor"].freq = master_freq
    
    # Stems laufen einfach weiter - nichts weiter zu tun!
    # Das ist der FAST PATH - keine Neuerstellung nötig.


def _create_pitched_stem_cache(button_id, stem, speed):
    """Erstellt den Pitch-Cache für einen Stem."""
    from pedalboard import PitchShift
    
    data = button_data[button_id]
    loop = data.get("pyo")
    dry_audio = data["stems"]["dry"].get(stem)
    
    if dry_audio is None or loop is None:
        return
    
    # Berechne Pitch-Korrektur
    semitones = speed_to_semitones(speed)
    
    audio_segment = dry_audio.copy()
    if audio_segment.dtype != np.float32:
        audio_segment = audio_segment.astype(np.float32)
    
    # Pedalboard erwartet (channels, samples)
    if len(audio_segment.shape) == 1:
        audio_segment = audio_segment.reshape(1, -1)
    else:
        audio_segment = audio_segment.T
    
    # Pitch-Shift anwenden
    pitch_shifter = PitchShift(semitones=semitones)
    pitched_audio = pitch_shifter(audio_segment, loop._audio_sr)
    
    # Zurück zu (samples, channels)
    if pitched_audio.shape[0] <= 2:
        pitched_audio = pitched_audio.T
    
    # Cache speichern
    data["stems"]["pitched"][stem] = pitched_audio.astype(np.float32)
    data["stems"]["cached_speed"] = speed


def precache_pitched_stems_if_needed(button_id):
    """
    Pre-cached gepitchte Stems für latenzfreies Umschalten.
    Wird bei Rechtsklick-Precache aufgerufen.
    """
    data = button_data[button_id]
    
    if not data["stems"]["available"]:
        return
    
    loop = data.get("pyo")
    if not loop or not loop._key_lock:
        return
    
    current_speed = loop._pending_speed
    
    if current_speed == 1.0:
        return  # Kein Pitching nötig
    
    # Für alle Stems Pitch-Cache erstellen
    for stem in STEM_NAMES:
        if data["stems"]["dry"].get(stem) is not None:
            if (data["stems"]["cached_speed"] != current_speed or 
                data["stems"]["pitched"][stem] is None):
                _create_pitched_stem_cache(button_id, stem, current_speed)


def invalidate_stem_caches(button_id):
    """Invalidiert alle Stem-Caches bei Speed-Änderung."""
    data = button_data[button_id]
    data["stems"]["cached_speed"] = None
    for stem in STEM_NAMES:
        data["stems"]["pitched"][stem] = None


def cleanup_stem_caches():
    """Gibt alle Stem-Caches frei beim Beenden."""
    try:
        for (bank_id, btn_id), loop in loaded_loops.items():
            data = all_banks_data[bank_id][btn_id]
            # Stoppe Stem-Player
            for stem in STEM_NAMES:
                player = data["stems"]["players"].get(stem)
                if player:
                    safe_stop(player, "player")
            # Caches löschen
            data["stems"]["pitched"] = {s: None for s in STEM_NAMES}
            data["stems"]["players"] = {s: None for s in STEM_NAMES}
            data["stems"]["tables"] = {s: None for s in STEM_NAMES}
    except Exception as e:
        logger.warning(f"Error cleaning up stem caches: {type(e).__name__}: {e}")


# ============== BANK MANAGEMENT ==============
def switch_bank(new_bank_id):
    global button_data
    if current_bank.get() == new_bank_id:
        return
    current_bank.set(new_bank_id)
    button_data = all_banks_data[new_bank_id]
    update_all_button_labels()
    update_bank_button_colors()
    update_button_colors()

def update_bank_button_colors():
    for bank_id, btn in bank_buttons.items():
        if bank_id == current_bank.get():
            btn.config(bg=COLOR_BANK_ACTIVE, fg=COLOR_TEXT_ACTIVE)
        else:
            btn.config(bg=COLOR_BANK_BTN, fg=COLOR_TEXT)

def update_button_colors():
    """Update all button colors based on their active state"""
    for btn_id, btn in buttons.items():
        data = button_data[btn_id]
        if data["active"] and data["file"]:
            btn.config(bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE)
        else:
            btn.config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)

def update_all_button_labels():
    for btn_id in buttons:
        update_button_label(btn_id)

def update_button_label(button_id):
    """Aktualisiert die Beschriftung eines Buttons."""
    data = button_data[button_id]
    if not data["file"]:
        buttons[button_id].config(text=f"{button_id}")
        buttons[button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
        return
    full_filename = os.path.basename(data["file"])
    line1 = full_filename[:16]
    line2 = full_filename[16:32] if len(full_filename) > 16 else ""
    bpm = data["bpm"]
    bpm_text = f"{bpm:.1f} BPM" if bpm else "BPM ?"
    
    # STEMS: Indikator wenn Stems verfügbar sind
    stems_indicator = ""
    if data.get("stems", {}).get("generating"):
        stems_indicator = " ⏳"  # Generating
    elif data.get("stems", {}).get("available"):
        stems_indicator = " ♪"  # Stems available
    
    label = f"{line1}\n{line2}\n{bpm_text}{stems_indicator}"
    buttons[button_id].config(text=label)
    if data["active"]:
        buttons[button_id].config(bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE)
    else:
        buttons[button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)

# ============== HELPER FUNCTIONS ==============
def get_current_original_bpm():
    # OPTIMIERUNG: Zuerst in loaded_loops nach aktiven Loops suchen
    for (bank_id, btn_id), loop in loaded_loops.items():
        data = all_banks_data[bank_id][btn_id]
        if data["active"] and data.get("bpm"):
            return data["bpm"]
    # Fallback: Durch alle Daten iterieren
    loaded_in_bank = [(btn_id, data) for btn_id, data in button_data.items()
                    if data["file"] and data.get("bpm")]
    if loaded_in_bank:
        return loaded_in_bank[-1][1]["bpm"]
    return 120.0

def detect_bpm(filepath, callback):
    def do_detect():
        try:
            bpm = _detect_bpm_worker(filepath)
            schedule_gui_update(lambda b=bpm: callback(b))
        except Exception as e:
            logger.warning(f"BPM detection error for '{filepath}': {type(e).__name__}: {e}")
            schedule_gui_update(lambda: callback(None))
    bpm_executor.submit(do_detect)

def detect_bpm_async(filepath, button_id, loop):
    def do_detect():
        try:
            bpm = _detect_bpm_worker(filepath)
            schedule_gui_update(lambda: on_bpm_result(bpm))
        except Exception as e:
            logger.warning(f"Async BPM detection error for '{filepath}': {type(e).__name__}: {e}")
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
                    logger.warning(f"Auto-loop calculation error for button {button_id}: {type(e).__name__}: {e}")
            root.after(1000, lambda: update_button_label(button_id))
            root.after(1200, save_config_async)
        else:
            new_bpm = simpledialog.askfloat(
                "BPM Detection Failed",
                f"Could not detect BPM.\nEnter BPM for Button {button_id}:",
                initialvalue=120.0, minvalue=10.0, maxvalue=500.0
            )
            if new_bpm:
                button_data[button_id]["bpm"] = round(new_bpm, 1)
                root.after(1000, lambda: update_button_label(button_id))
                root.after(1200, save_config_async)
            else:
                root.after(1000, lambda: update_button_label(button_id))
    bpm_executor.submit(do_detect)

# ============== LOOP CONTROL ==============
def calculate_intro_start(button_id):
    """
    Berechnet den Intro-Startpunkt basierend auf intro_bars und BPM.
    Gibt den Intro-Startpunkt zurück (geclamped auf 0 wenn negativ).
    """
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

def trigger_loop(button_id):
    """Left click - trigger/play loop from start (mit optionalem Intro und Stems)"""
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
                            stop_stem_players(btn_id)
                            data["stems"]["initialized"] = False
                        data["active"] = False
                        if bank_id == current_bank.get():
                            buttons[btn_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
                            update_button_label(btn_id)
            
            # Stop if playing to retrigger from start
            is_retrigger = button_data[button_id]["active"]
            stems_available = button_data[button_id]["stems"]["available"]
            
            if is_retrigger:
                loop.stop()
                # STEMS: Bei Retrigger Stems aufräumen (Tables bleiben erhalten!)
                if button_data[button_id]["stems"].get("initialized"):
                    _cleanup_stem_players(button_id)
            
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
                # WICHTIG: PyoLoop-Objekte initialisieren (amp, speed) BEVOR sie verwendet werden!
                # Ohne das ist loop.amp = None → ArithmeticError bei Multiplikation
                loop._ensure_player()
                
                # PyoLoop stumm schalten
                loop.set_stem_mute(Sig(0))
                
                # Stems ZUERST initialisieren (erstellt Phasor + Player)
                initialize_stem_players(button_id)
                update_stem_gains(button_id)
                
                # PyoLoop nur für Timing markieren (ohne Audio-Output)
                # Das ist nötig für die interne Logik (loop_end, etc.)
                loop._is_playing = True
                
            else:
                # Keine Stems - normales Verhalten
                if intro_active:
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
            update_button_label(button_id)
            
            # STEMS: Stem-Buttons aktualisieren
            update_stem_buttons_state()
            
    except Exception as e:
        logger.error(f"Error triggering loop {button_id}: {type(e).__name__}: {e}")

def stop_loop(button_id):
    """
    Right click - stop loop ODER pre-cache bei gestopptem Loop.
    
    Wenn der Loop läuft: Stoppt den Loop und alle Stem-Player.
    Wenn der Loop gestoppt ist UND Key Lock aktiv ist: 
        Pre-cached das pitch-shifted Audio für latenzfreies Triggern.
        Pre-cached auch Stems falls vorhanden.
    """
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
            stop_stem_players(button_id)
            button_data[button_id]["stems"]["initialized"] = False
            
            button_data[button_id]["active"] = False
            buttons[button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
            update_button_label(button_id)
            update_stem_buttons_state()
        else:
            # Loop ist gestoppt -> Pre-Caching wenn Key Lock aktiv
            if key_lock_active.get() and button_data[button_id].get("file"):
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
                    original_bg = buttons[button_id].cget('bg')
                    buttons[button_id].config(bg="#ff8800")
                    
                    # Pre-Cache in Background Thread
                    def do_precache():
                        success = loop.precache_pitched_audio()
                        
                        # STEMS: Auch Stems precachen falls vorhanden
                        if button_data[button_id]["stems"]["available"]:
                            precache_pitched_stems_if_needed(button_id)
                        
                        def restore_color():
                            if not button_data[button_id]["active"]:
                                buttons[button_id].config(bg=original_bg)
                        schedule_gui_update(restore_color)
                    
                    io_executor.submit(do_precache)
    except Exception as e:
        logger.error(f"Error in stop_loop {button_id}: {type(e).__name__}: {e}")

def load_loop(button_id):
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
        buttons[button_id].config(text=f"{button_id}\nLoading...")
        
        def background_load():
            try:
                if old_file_path and os.path.exists(old_file_path):
                    try:
                        os.remove(old_file_path)
                    except OSError:
                        pass  # Datei in Verwendung oder nicht löschbar
                original_name = os.path.basename(filepath)
                dest_path = os.path.join("loops", original_name)
                base, ext = os.path.splitext(original_name)
                counter = 1
                while os.path.exists(dest_path):
                    dest_path = os.path.join("loops", f"{base}_{counter}{ext}")
                    counter += 1
                shutil.copy(filepath, dest_path)
                loop = PyoLoop()
                
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
                        buttons[button_id].config(text=f"{line1}\n{line2}\nAnalyzing...")
                        detect_bpm_async(dest_path, button_id, loop)
                    schedule_gui_update(update_button_data)
                loop.load_async(dest_path, callback=on_audio_loaded)
            except Exception as e:
                schedule_gui_update(lambda: buttons[button_id].config(text=f"{button_id}"))
        io_executor.submit(background_load)
    except Exception as e:
        logger.error(f"Error loading loop: {type(e).__name__}: {e}")

def unload_loop(button_id):
    try:
        file_path = button_data[button_id].get("file")
        loop = button_data[button_id].get("pyo")
        if loop:
            # Cache invalidieren (gibt RAM frei)
            if hasattr(loop, '_invalidate_pitch_cache'):
                loop._invalidate_pitch_cache()
            loop.stop()
            button_data[button_id]["pyo"] = None
            # OPTIMIERUNG: Loop aus Tracking entfernen
            unregister_loaded_loop(current_bank.get(), button_id)
        
        # STEMS: Stem-Player stoppen und Caches freigeben
        for stem in STEM_NAMES:
            player = button_data[button_id]["stems"]["players"].get(stem)
            if player:
                safe_stop(player, "player")
        
        button_data[button_id]["active"] = False
        buttons[button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
        if file_path and os.path.exists(file_path):
            try:
                os.remove(file_path)
            except OSError:
                pass  # Datei in Verwendung oder nicht löschbar
        button_data[button_id] = get_default_button_data()
        buttons[button_id].config(text=f"{button_id}")
        update_stem_buttons_state()
        save_config_async()
    except Exception as e:
        logger.error(f"Error unloading: {type(e).__name__}: {e}")

# ============== BPM DISPLAY ==============
# OPTIMIERUNG: Tracking ob aktive Loops vorhanden sind
_has_active_loops = False

def update_bpm_display_once():
    global last_bpm_display, _has_active_loops
    
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
        bpm_display.config(text=new_text)
        last_bpm_display = new_text

def update_bpm_display():
    update_bpm_display_once()
    # OPTIMIERUNG: Seltener updaten wenn nichts aktiv ist
    if _has_active_loops:
        root.after(100, update_bpm_display)
    else:
        root.after(500, update_bpm_display)  # Langsameres Update wenn nichts spielt

# ============== SPEED CONTROL ==============
def on_speed_change(val):
    value = round(float(val), 2)
    speed_value.set(value)
    if bpm_lock_active.get():
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
                        invalidate_stem_caches(btn_id)
                        # Bei aktivem Stem-Mix: Player neu erstellen
                        if data["active"] and any(data["stems"]["states"].values()):
                            apply_stem_mix(btn_id)
                            
            except (ValueError, ZeroDivisionError, AttributeError):
                pass  # Ungültige Werte ignorieren
    
    update_bpm_display_once()
    # Reset-Button Stil aktualisieren
    try:
        update_reset_button_style()
    except NameError:
        pass  # Button existiert noch nicht beim Start

def reset_pitch():
    speed_value.set(1.0)
    speed_slider.set(1.0)
    on_speed_change(1.0)

def adjust_bpm_by_delta(delta_bpm):
    """
    Passt die BPM um einen festen Wert an.
    Nutzt die gleiche Logik wie die manuelle BPM-Eingabe.
    delta_bpm: positive Werte = schneller, negative = langsamer
    """
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
        
        update_bpm_display_once()
        
        # Slider visuell aktualisieren (triggert on_speed_change)
        speed_slider.set(required_speed)
        
        # Reset-Button Farbe IMMER am Ende aktualisieren
        update_reset_button_style()
    except Exception as e:
        logger.error(f"Error adjusting BPM: {type(e).__name__}: {e}")

def on_bpm_up_click(event):
    """
    Handler für Klick auf BPM-Up Button.
    Linksklick: +1 BPM
    Rechtsklick: +0.1 BPM
    """
    if event.num == 1:  # Linksklick
        adjust_bpm_by_delta(1.0)
    elif event.num == 3:  # Rechtsklick
        adjust_bpm_by_delta(0.1)

def on_bpm_down_click(event):
    """
    Handler für Klick auf BPM-Down Button.
    Linksklick: -1 BPM
    Rechtsklick: -0.1 BPM
    """
    if event.num == 1:  # Linksklick
        adjust_bpm_by_delta(-1.0)
    elif event.num == 3:  # Rechtsklick
        adjust_bpm_by_delta(-0.1)

def update_reset_button_style():
    """
    Reset-Button: Grün bei 1.00, sonst rot.
    """
    current = speed_value.get()
    if current == 1.0 or abs(current - 1.0) < 0.001:
        reset_btn.config(bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE,
                        activebackground=COLOR_BTN_ACTIVE, activeforeground=COLOR_TEXT_ACTIVE)
    else:
        reset_btn.config(bg=COLOR_RESET_RED, fg=COLOR_TEXT,
                        activebackground=COLOR_RESET_RED, activeforeground=COLOR_TEXT)

def toggle_key_lock():
    """Toggle Key Lock (Master Tempo) für alle Loops - OHNE Unterbrechung"""
    key_lock_active.set(not key_lock_active.get())
    
    if key_lock_active.get():
        key_lock_btn.config(bg=COLOR_LOCK_ON, fg=COLOR_TEXT_ACTIVE)
    else:
        key_lock_btn.config(bg=COLOR_LOCK_OFF, fg=COLOR_TEXT)
    
    # OPTIMIERUNG: Nur loaded_loops aktualisieren statt alle Banks
    for (bank_id, btn_id), loop in loaded_loops.items():
        loop.set_key_lock(key_lock_active.get())

def toggle_bpm_lock():
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

def toggle_multi_loop():
    multi_loop_active.set(not multi_loop_active.get())
    if multi_loop_active.get():
        multi_loop_btn.config(bg=COLOR_LOCK_ON, fg=COLOR_TEXT_ACTIVE)
    else:
        multi_loop_btn.config(bg=COLOR_LOCK_OFF, fg=COLOR_TEXT)
        # OPTIMIERUNG: Nur loaded_loops durchsuchen
        all_active = []
        for (bank_id, btn_id), loop in loaded_loops.items():
            data = all_banks_data[bank_id][btn_id]
            if data["active"]:
                all_active.append((bank_id, btn_id, data, loop))
        if len(all_active) > 1:
            for bank_id, btn_id, data, loop in all_active[:-1]:
                loop.stop()
                data["active"] = False
                if bank_id == current_bank.get():
                    buttons[btn_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
                    update_button_label(btn_id)

def update_speed_from_master_bpm(*args):
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
            speed_slider.set(max(0.5, min(2.0, required_speed)))
            # OPTIMIERUNG: Nur loaded_loops aktualisieren
            for (bank_id, btn_id), loop in loaded_loops.items():
                loop.set_speed(required_speed)
            update_reset_button_style()
        
        root.focus_set()  # Fokus vom Entry-Feld entfernen
    except (ValueError, ZeroDivisionError, tk.TclError):
        root.focus_set()  # Auch bei Fehler Fokus entfernen

def validate_bpm_entry(new_value):
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

# ============== EQ KNOB FOR VOLUME WINDOW ==============
class EQKnob:
    def __init__(self, parent, label, width=50, height=65):
        self.frame = tk.Frame(parent, bg=COLOR_BG)
        self.label = label
        self.value = 0.0
        self.width = width
        self.height = height
        self.dragging = False
        self.last_y = 0
        self.last_x = 0
        
        self.canvas = tk.Canvas(self.frame, width=width, height=height-15,
                               bg=COLOR_BG, highlightthickness=0)
        self.canvas.pack()
        
        self.text_label = tk.Label(self.frame, text=label, fg=COLOR_TEXT,
                                  bg=COLOR_BG, font=("Arial", 8))
        self.text_label.pack()
        
        self.canvas.bind("<Button-1>", self.start_drag)
        self.canvas.bind("<B1-Motion>", self.on_drag)
        self.canvas.bind("<ButtonRelease-1>", self.end_drag)
        self.canvas.bind("<Button-2>", self.reset)
        self.canvas.bind("<Button-3>", self.kill)
        
        self.draw_knob()
    
    def draw_knob(self):
        self.canvas.delete("all")
        cx, cy = self.width // 2, (self.height - 15) // 2
        radius = min(cx, cy) - 3
        
        self.canvas.create_oval(cx-radius, cy-radius, cx+radius, cy+radius,
                               fill="#333", outline="#555", width=2)
        
        val = self.value
        angle = 250 - (val + 1) * 160
        rad = math.radians(angle)
        x2 = cx + (radius - 4) * math.cos(rad)
        y2 = cy - (radius - 4) * math.sin(rad)
        
        if val <= -0.98:
            color = "#ff0000"
        elif abs(val) < 0.02:
            color = "#00ff00"
        else:
            color = "#ffaa00"
        
        self.canvas.create_line(cx, cy, x2, y2, fill=color, width=3)
        self.canvas.create_oval(cx-2, cy-2, cx+2, cy+2, fill=color, outline="")
    
    def start_drag(self, event):
        self.dragging = True
        self.last_y = event.y
        self.last_x = event.x
    
    def on_drag(self, event):
        if not self.dragging:
            return
        
        dy = self.last_y - event.y
        dx = event.x - self.last_x
        delta = (dy + dx) * 0.012
        
        new_value = self.value + delta
        
        if abs(new_value) < 0.03 and abs(self.value) >= 0.03:
            new_value = 0.0
        
        self.value = max(-1.0, min(1.0, new_value))
        self.last_y = event.y
        self.last_x = event.x
        self.draw_knob()
    
    def end_drag(self, event):
        self.dragging = False
    
    def reset(self, event):
        self.value = 0.0
        self.draw_knob()
    
    def kill(self, event):
        self.value = -1.0
        self.draw_knob()
    
    def set_value(self, val):
        self.value = max(-1.0, min(1.0, val))
        self.draw_knob()
    
    def get_value(self):
        return self.value

# ============== VU METER ==============
class VUMeter:
    def __init__(self, parent, width=20, height=200):
        self.canvas = tk.Canvas(parent, width=width, height=height, bg="#222")
        self.width = width
        self.height = height
        self.segments = []
        self.create_segments()
        
    def create_segments(self):
        segment_height = 6
        gap = 1
        total_segments = (self.height - 20) // (segment_height + gap)
        db_per_segment = 60 / total_segments
        
        for i in range(total_segments):
            y_top = self.height - 10 - (i * (segment_height + gap))
            y_bottom = y_top - segment_height
            
            db_value = -60 + (i * db_per_segment)
            
            if db_value < -18:
                color = "#00ff00"
            elif db_value < -6:
                color = "#ffff00"
            elif db_value < -3:
                color = "#ff8800"
            else:
                color = "#ff0000"
            
            segment = self.canvas.create_rectangle(
                2, y_bottom, self.width-2, y_top,
                fill="#333", outline="#555"
            )
            self.segments.append((segment, color, db_value))
        
    def update_level(self, db_level):
        for segment, color, db_threshold in self.segments:
            if db_level >= db_threshold:
                self.canvas.itemconfig(segment, fill=color)
            else:
                self.canvas.itemconfig(segment, fill="#333")

# ============== VOLUME WINDOW WITH EQ ==============
def set_volume(button_id):
    global open_volume_windows
    
    if button_id in open_volume_windows:
        existing_window = open_volume_windows[button_id]
        try:
            if existing_window.winfo_exists():
                existing_window.lift()
                existing_window.focus_force()
                return
        except tk.TclError:
            pass  # Fenster wurde inzwischen geschlossen - ignorieren und neues erstellen
        del open_volume_windows[button_id]
    
    try:
        vol_win = tk.Toplevel(root)
        vol_win.title(f"Volume + EQ - Button {button_id}")
        vol_win.configure(bg=COLOR_BG)
        vol_win.geometry("600x280")
        vol_win.resizable(False, False)
        
        open_volume_windows[button_id] = vol_win

        root.update_idletasks()
        x = root.winfo_x() + (root.winfo_width() // 2) - 300
        y = root.winfo_y() + (root.winfo_height() // 2) - 140
        vol_win.geometry(f"+{x}+{y}")

        main_frame = tk.Frame(vol_win, bg=COLOR_BG)
        main_frame.pack(fill="both", expand=True, padx=10, pady=10)

        vu_frame = tk.Frame(main_frame, bg=COLOR_BG)
        vu_frame.pack(side="left", padx=(0, 15))
        
        vu_label = tk.Label(vu_frame, text="Level", fg=COLOR_TEXT, bg=COLOR_BG, font=("Arial", 8))
        vu_label.pack()
        
        vu_meter = VUMeter(vu_frame, width=25, height=200)
        vu_meter.canvas.pack()

        controls_frame = tk.Frame(main_frame, bg=COLOR_BG)
        controls_frame.pack(side="left", fill="both", expand=True)

        current_db = button_data[button_id]["gain_db"]

        update_scheduled = [None]
        
        def on_slide(val):
            try:
                value = round(float(val), 1)
                
                if update_scheduled[0]:
                    root.after_cancel(update_scheduled[0])
                
                def do_update():
                    button_data[button_id]["gain_db"] = value
                    loop = button_data[button_id].get("pyo")
                    if loop:
                        loop.set_gain(value)
                    save_config_async()
                
                update_scheduled[0] = root.after(50, do_update)
                
            except Exception as e:
                logger.error(f"Error setting volume: {type(e).__name__}: {e}")

        # Custom Label-Formatierung für den Slider (zeigt "X.X dB")
        class GainScale(tk.Scale):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, **kwargs)
            
            def set(self, value):
                super().set(value)

        scale = tk.Scale(
            controls_frame,
            from_=-20.0,
            to=20.0,
            resolution=0.1,
            orient="horizontal",
            length=400,
            width=30,
            command=on_slide,
            bg=COLOR_BG,
            fg=COLOR_TEXT,
            troughcolor="#444",
            highlightthickness=0,
            showvalue=False
        )
        scale.set(current_db)
        scale.pack(pady=(10, 0))
        
        # dB-Label das dem Slider-Wert folgt
        def update_db_label(*args):
            val = scale.get()
            db_label.config(text=f"{val:.1f} dB")
            # Position des Labels aktualisieren
            slider_length = 400
            slider_min = -20.0
            slider_max = 20.0
            # Berechne relative Position (0-1)
            rel_pos = (val - slider_min) / (slider_max - slider_min)
            # Pixel-Position (mit etwas Offset für den Slider-Rand)
            x_pos = int(15 + rel_pos * (slider_length - 10))
            db_label.place(x=x_pos, y=0, anchor="s")
        
        db_label_frame = tk.Frame(controls_frame, bg=COLOR_BG, height=20, width=430)
        db_label_frame.pack()
        db_label_frame.pack_propagate(False)
        
        db_label = tk.Label(db_label_frame, text=f"{current_db:.1f} dB", fg=COLOR_TEXT, bg=COLOR_BG, font=("Arial", 9))
        db_label.place(x=215, y=0, anchor="s")
        
        # Original on_slide erweitern
        original_on_slide = on_slide
        def on_slide_with_label(val):
            original_on_slide(val)
            update_db_label()
        
        scale.config(command=on_slide_with_label)
        update_db_label()

        midpoint = tk.Label(controls_frame, text="<< quieter       normal       louder >>",
                           fg="#888", bg=COLOR_BG, font=("Arial", 8))
        midpoint.pack()

        reset_vol_btn = tk.Button(controls_frame, text="Reset to 0 dB", 
                             command=lambda: scale.set(0.0), bg="#444", fg=COLOR_TEXT)
        reset_vol_btn.pack(pady=5)

        eq_frame = tk.Frame(controls_frame, bg=COLOR_BG)
        eq_frame.pack(pady=(15, 0))
        
        tk.Label(eq_frame, text="EQ", fg="#ffaa00", bg=COLOR_BG, 
                font=("Arial", 10, "bold")).pack()
        
        eq_knobs_frame = tk.Frame(eq_frame, bg=COLOR_BG)
        eq_knobs_frame.pack(pady=5)
        
        eq_low_knob = EQKnob(eq_knobs_frame, "LOW", width=50, height=70)
        eq_low_knob.set_value(button_data[button_id].get("eq_low", 0.0))
        eq_low_knob.frame.pack(side="left", padx=5)
        
        eq_mid_knob = EQKnob(eq_knobs_frame, "MID", width=50, height=70)
        eq_mid_knob.set_value(button_data[button_id].get("eq_mid", 0.0))
        eq_mid_knob.frame.pack(side="left", padx=5)
        
        eq_high_knob = EQKnob(eq_knobs_frame, "HIGH", width=50, height=70)
        eq_high_knob.set_value(button_data[button_id].get("eq_high", 0.0))
        eq_high_knob.frame.pack(side="left", padx=5)
        
        # OPTIMIERUNG: EQ nur updaten wenn sich Werte ändern
        last_eq_values = [
            button_data[button_id].get("eq_low", 0.0),
            button_data[button_id].get("eq_mid", 0.0),
            button_data[button_id].get("eq_high", 0.0)
        ]
        
        def update_eq():
            try:
                low = eq_low_knob.get_value()
                mid = eq_mid_knob.get_value()
                high = eq_high_knob.get_value()
                
                # OPTIMIERUNG: Nur updaten wenn sich Werte geändert haben
                if (abs(low - last_eq_values[0]) > 0.001 or 
                    abs(mid - last_eq_values[1]) > 0.001 or 
                    abs(high - last_eq_values[2]) > 0.001):
                    
                    last_eq_values[0] = low
                    last_eq_values[1] = mid
                    last_eq_values[2] = high
                    
                    button_data[button_id]["eq_low"] = low
                    button_data[button_id]["eq_mid"] = mid
                    button_data[button_id]["eq_high"] = high
                    
                    loop = button_data[button_id].get("pyo")
                    if loop:
                        loop.set_eq(low, mid, high)
                    
                    # STEMS: Auch Stem-EQs aktualisieren
                    update_stem_eq(button_id, low, mid, high)
                
                if vol_win.winfo_exists():
                    vol_win.after(50, update_eq)
            except tk.TclError:
                pass  # Fenster wurde geschlossen
        
        update_eq()

        loop = button_data[button_id].get("pyo")
        if loop:
            loop.enable_level_meter()

        def update_vu_meter():
            try:
                if vol_win.winfo_exists():
                    loop = button_data[button_id].get("pyo")
                    if loop and button_data[button_id]["active"]:
                        db_level = loop.get_level_db()
                        vu_meter.update_level(db_level)
                    else:
                        vu_meter.update_level(-80)
                    vol_win.after(50, update_vu_meter)
            except tk.TclError:
                pass  # Fenster wurde geschlossen - VU meter update abbrechen
            except Exception as e:
                logger.error(f"Error updating VU meter: {type(e).__name__}: {e}")
        
        def on_volume_window_close():
            global open_volume_windows
            button_data[button_id]["eq_low"] = eq_low_knob.get_value()
            button_data[button_id]["eq_mid"] = eq_mid_knob.get_value()
            button_data[button_id]["eq_high"] = eq_high_knob.get_value()
            save_config_async()
            
            loop = button_data[button_id].get("pyo")
            if loop:
                loop.disable_level_meter()
            
            if button_id in open_volume_windows:
                del open_volume_windows[button_id]
            
            vol_win.destroy()
        
        vol_win.protocol("WM_DELETE_WINDOW", on_volume_window_close)
        update_vu_meter()
        
    except Exception as e:
        logger.error(f"Error opening volume control: {type(e).__name__}: {e}")

# ============== WAVEFORM EDITOR ==============
class WaveformEditor:
    def __init__(self, parent, button_id):
        global open_loop_editor_windows
        
        if button_id in open_loop_editor_windows:
            existing_editor = open_loop_editor_windows[button_id]
            try:
                if existing_editor.window.winfo_exists():
                    existing_editor.window.lift()
                    existing_editor.window.focus_force()
                    return
            except (tk.TclError, AttributeError):
                pass  # Fenster/Editor wurde inzwischen zerstört - ignorieren und neues erstellen
            del open_loop_editor_windows[button_id]
        
        self.parent = parent
        self.button_id = button_id
        self.audio_data = None
        self.sample_rate = None
        self.duration = 0
        self.loop_start = button_data[button_id].get("loop_start", 0.0)
        self.loop_end = button_data[button_id].get("loop_end", None)
        self.zoom_start = 0.0
        self.zoom_end = None
        self._loading = True
        
        # Ursprüngliche Werte speichern für UNDO
        self._original_loop_start = self.loop_start
        self._original_loop_end = self.loop_end
        self._original_auto_loop_active = button_data[button_id].get("auto_loop_active", True)
        self._original_auto_loop_bars = button_data[button_id].get("auto_loop_bars", 8)
        self._original_auto_loop_custom_mode = button_data[button_id].get("auto_loop_custom_mode", False)
        self._original_intro_active = button_data[button_id].get("intro_active", False)
        self._original_intro_bars = button_data[button_id].get("intro_bars", 4)
        self._original_intro_custom_mode = button_data[button_id].get("intro_custom_mode", False)
        
        self.auto_loop_active = tk.BooleanVar(value=button_data[button_id].get("auto_loop_active", True))
        self.auto_loop_bars = tk.IntVar(value=button_data[button_id].get("auto_loop_bars", 8))
        self.auto_loop_custom_mode = tk.BooleanVar(value=button_data[button_id].get("auto_loop_custom_mode", False))
        
        # Intro Variablen
        self.intro_active = tk.BooleanVar(value=button_data[button_id].get("intro_active", False))
        self.intro_bars = tk.DoubleVar(value=button_data[button_id].get("intro_bars", 4))
        self.intro_custom_mode = tk.BooleanVar(value=button_data[button_id].get("intro_custom_mode", False))
        
        self.waveform_cache = {}
        self.cache_levels = [1000, 5000, 20000, 50000]
        
        if self.setup_window():
            open_loop_editor_windows[button_id] = self
            self.show_loading_message()
            self.load_audio_async()

    def setup_window(self):
        try:
            self.window = tk.Toplevel(self.parent)
            self.window.title(f"Loop Editor - Button {self.button_id}")
            self.window.configure(bg=COLOR_BG)
            self.window.geometry("900x650")
            self.window.protocol("WM_DELETE_WINDOW", self.on_window_close)
            return True
        except tk.TclError as e:
            logger.error(f"Error creating window: {type(e).__name__}: {e}")
            return False

    def show_loading_message(self):
        self.loading_label = tk.Label(self.window, text="Loading waveform...",
                                      fg="#ffff00", bg=COLOR_BG, font=("Arial", 14))
        self.loading_label.pack(expand=True)

    def load_audio_async(self):
        def do_load():
            try:
                filepath = button_data[self.button_id]["file"]
                if not os.path.exists(filepath):
                    schedule_gui_update(lambda: self.on_load_error("File not found"))
                    return
                
                # OPTIMIERUNG: Prüfe ob Waveform-Cache vorhanden ist
                cached_waveform = button_data[self.button_id].get("waveform_cache")
                if cached_waveform is not None:
                    # Cache verwenden
                    audio_data = cached_waveform["audio_data"]
                    sample_rate = cached_waveform["sample_rate"]
                    duration = cached_waveform["duration"]
                    waveform_cache = cached_waveform["cache"]
                    schedule_gui_update(lambda: self.on_load_complete(audio_data, sample_rate, duration, waveform_cache))
                    return
                
                audio_data, sample_rate = sf.read(filepath)
                if len(audio_data) == 0:
                    schedule_gui_update(lambda: self.on_load_error("Empty file"))
                    return
                if len(audio_data.shape) > 1:
                    audio_data = np.mean(audio_data, axis=1)
                duration = len(audio_data) / sample_rate
                
                waveform_cache = {}
                for target_samples in self.cache_levels:
                    if len(audio_data) > target_samples:
                        factor = len(audio_data) // target_samples
                        downsampled = audio_data[::factor]
                        time_axis = np.linspace(0, duration, len(downsampled))
                        waveform_cache[target_samples] = (downsampled, time_axis)
                    else:
                        time_axis = np.linspace(0, duration, len(audio_data))
                        waveform_cache[target_samples] = (audio_data.copy(), time_axis)
                
                # OPTIMIERUNG: Waveform-Cache in button_data speichern
                button_data[self.button_id]["waveform_cache"] = {
                    "audio_data": audio_data,
                    "sample_rate": sample_rate,
                    "duration": duration,
                    "cache": waveform_cache
                }
                
                schedule_gui_update(lambda: self.on_load_complete(audio_data, sample_rate, duration, waveform_cache))
            except Exception as e:
                schedule_gui_update(lambda: self.on_load_error(str(e)))
        io_executor.submit(do_load)

    def on_load_complete(self, audio_data, sample_rate, duration, waveform_cache):
        self.audio_data = audio_data
        self.sample_rate = sample_rate
        self.duration = duration
        self.zoom_end = duration
        self.waveform_cache = waveform_cache
        self._loading = False
        if self.loop_end is None:
            self.loop_end = self.duration
        self.loading_label.destroy()
        self.create_controls()
        self.create_waveform()
        self.update_auto_loop_display()
        self.update_intro_display()
        self.update_play_button()

    def on_load_error(self, error_msg):
        self.loading_label.config(text=f"Error: {error_msg}")

    def create_controls(self):
        control_frame = tk.Frame(self.window, bg=COLOR_BG)
        control_frame.pack(fill="x", padx=10, pady=5)
        
        self.play_btn = tk.Button(control_frame, text="> Play", command=self.toggle_playback,
                                  bg="#444", fg=COLOR_TEXT)
        self.play_btn.pack(side="left", padx=5)
        
        tk.Button(control_frame, text="Reset", command=self.reset_loop,
                  bg="#444", fg=COLOR_TEXT).pack(side="left", padx=5)
        tk.Button(control_frame, text="Fit", command=self.zoom_fit,
                  bg="#444", fg=COLOR_TEXT).pack(side="left", padx=5)
        tk.Button(control_frame, text="|<", command=self.jump_to_start,
                  bg="#444", fg=COLOR_TEXT, width=3).pack(side="left", padx=2)
        tk.Button(control_frame, text=">|", command=self.jump_to_end,
                  bg="#444", fg=COLOR_TEXT, width=3).pack(side="left", padx=2)
        tk.Button(control_frame, text="+", command=lambda: self.zoom_by_factor(0.5),
                  bg="#444", fg=COLOR_TEXT, width=3).pack(side="left", padx=2)
        tk.Button(control_frame, text="-", command=lambda: self.zoom_by_factor(2.0),
                  bg="#444", fg=COLOR_TEXT, width=3).pack(side="left", padx=2)
        
        # UNDO und APPLY Buttons in der Mitte
        tk.Button(control_frame, text="UNDO", command=self.undo_and_close,
                  bg=COLOR_LOCK_OFF, fg=COLOR_TEXT, width=6).pack(side="left", padx=(20, 2))
        tk.Button(control_frame, text="APPLY", command=self.apply_and_close,
                  bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE, width=6).pack(side="left", padx=2)
        
        auto_frame = tk.Frame(control_frame, bg=COLOR_BG)
        auto_frame.pack(side="right", padx=10)
        
        self.auto_loop_cb = tk.Checkbutton(auto_frame, text="Auto-Loop",
                                           variable=self.auto_loop_active,
                                           bg=COLOR_BG, fg=COLOR_TEXT, selectcolor="#444",
                                           command=self.on_auto_loop_toggle)
        self.auto_loop_cb.pack(side="left")
        
        tk.Button(auto_frame, text="-", width=2, command=self.bars_down,
                  bg="#444", fg=COLOR_TEXT).pack(side="left", padx=2)
        self.bars_label = tk.Label(auto_frame, text="8 bars", fg=COLOR_TEXT,
                                   bg=COLOR_BG, width=7)
        self.bars_label.pack(side="left")
        tk.Button(auto_frame, text="+", width=2, command=self.bars_up,
                  bg="#444", fg=COLOR_TEXT).pack(side="left", padx=2)
        
        self.custom_cb = tk.Checkbutton(auto_frame, text="Custom",
                                        variable=self.auto_loop_custom_mode,
                                        bg=COLOR_BG, fg=COLOR_TEXT, selectcolor="#444",
                                        command=self.on_custom_mode_toggle)
        self.custom_cb.pack(side="left", padx=(10, 0))
        
        # Intro-Zeile (zweite Reihe rechts)
        intro_frame = tk.Frame(self.window, bg=COLOR_BG)
        intro_frame.pack(fill="x", padx=10, pady=(0, 5))
        
        # Spacer links um Intro-Controls rechtsbündig zu machen
        intro_spacer = tk.Frame(intro_frame, bg=COLOR_BG)
        intro_spacer.pack(side="left", expand=True, fill="x")
        
        intro_controls = tk.Frame(intro_frame, bg=COLOR_BG)
        intro_controls.pack(side="right", padx=10)
        
        self.intro_cb = tk.Checkbutton(intro_controls, text="Intro",
                                       variable=self.intro_active,
                                       bg=COLOR_BG, fg="#ffff00", selectcolor="#444",
                                       activeforeground="#ffff00",
                                       command=self.on_intro_toggle)
        self.intro_cb.pack(side="left")
        
        tk.Button(intro_controls, text="-", width=2, command=self.intro_bars_down,
                  bg="#444", fg=COLOR_TEXT).pack(side="left", padx=2)
        self.intro_bars_label = tk.Label(intro_controls, text="4 bars", fg="#ffff00",
                                         bg=COLOR_BG, width=7)
        self.intro_bars_label.pack(side="left")
        tk.Button(intro_controls, text="+", width=2, command=self.intro_bars_up,
                  bg="#444", fg=COLOR_TEXT).pack(side="left", padx=2)
        
        self.intro_custom_cb = tk.Checkbutton(intro_controls, text="Custom",
                                              variable=self.intro_custom_mode,
                                              bg=COLOR_BG, fg="#ffff00", selectcolor="#444",
                                              activeforeground="#ffff00",
                                              command=self.on_intro_custom_mode_toggle)
        self.intro_custom_cb.pack(side="left", padx=(10, 0))
        
        self.time_label = tk.Label(self.window, text="", fg=COLOR_TEXT, bg=COLOR_BG)
        self.time_label.pack(pady=5)

    def create_waveform(self):
        self.fig = Figure(figsize=(10, 5), dpi=100, facecolor='#1e1e1e')
        self.ax = self.fig.add_subplot(111)
        self.ax.set_facecolor('#1e1e1e')
        self.canvas = FigureCanvasTkAgg(self.fig, self.window)
        self.canvas.get_tk_widget().pack(fill="both", expand=True, padx=10, pady=10)
        self.canvas.mpl_connect('button_press_event', self.on_click)
        self.canvas.mpl_connect('scroll_event', self.on_scroll)
        self._waveform_update_pending = None
        self.update_waveform()

    def get_cached_waveform(self, visible_samples):
        for level in sorted(self.cache_levels, reverse=True):
            if visible_samples <= level * 2:
                return self.waveform_cache.get(level, (self.audio_data, None))
        return self.waveform_cache.get(self.cache_levels[-1], (self.audio_data, None))

    def update_waveform_throttled(self):
        if self._waveform_update_pending:
            self.window.after_cancel(self._waveform_update_pending)
        self._waveform_update_pending = self.window.after(30, self.update_waveform)

    def update_waveform(self):
        if self._loading or self.audio_data is None:
            return
        self.ax.clear()
        start_idx = max(0, int(self.zoom_start * self.sample_rate))
        end_idx = min(len(self.audio_data), int(self.zoom_end * self.sample_rate))
        visible_samples = end_idx - start_idx
        cached_data, cached_time = self.get_cached_waveform(visible_samples)
        mask = (cached_time >= self.zoom_start) & (cached_time <= self.zoom_end)
        zoomed_data = cached_data[mask] if np.any(mask) else cached_data
        zoomed_time = cached_time[mask] if np.any(mask) else cached_time
        
        self.ax.plot(zoomed_time, zoomed_data, color='#00ff00', linewidth=0.5)
        self.ax.fill_between(zoomed_time, zoomed_data, alpha=0.3, color='#00ff00')
        
        # Intro-Linie und Highlight zeichnen (gelb/orange)
        intro_start = self.calculate_intro_start_position()
        if intro_start is not None:
            # Gelbe gestrichelte Linie für Intro-Start
            if self.zoom_start <= intro_start <= self.zoom_end:
                self.ax.axvline(x=intro_start, color='#ffff00', linewidth=2, linestyle='--')
            
            # Orange Highlight für Intro-Bereich (zwischen Intro-Start und Loop-Start)
            if intro_start < self.loop_start and intro_start < self.zoom_end and self.loop_start > self.zoom_start:
                hl_intro_start = max(intro_start, self.zoom_start)
                hl_intro_end = min(self.loop_start, self.zoom_end)
                self.ax.axvspan(hl_intro_start, hl_intro_end, alpha=0.15, color='#ff8800')
        
        # Loop Start/End Linien
        if self.zoom_start <= self.loop_start <= self.zoom_end:
            self.ax.axvline(x=self.loop_start, color='#00ffff', linewidth=2)
        if self.zoom_start <= self.loop_end <= self.zoom_end:
            self.ax.axvline(x=self.loop_end, color='#ff0000', linewidth=2)
        
        # Loop-Highlight (gelb)
        if self.loop_start < self.zoom_end and self.loop_end > self.zoom_start:
            hl_start = max(self.loop_start, self.zoom_start)
            hl_end = min(self.loop_end, self.zoom_end)
            self.ax.axvspan(hl_start, hl_end, alpha=0.2, color='#ffff00')
        
        self.ax.set_xlim(self.zoom_start, self.zoom_end)
        self.ax.set_xlabel('Time (s)', color=COLOR_TEXT)
        self.ax.tick_params(colors=COLOR_TEXT)
        self.ax.grid(True, alpha=0.3)
        self.fig.tight_layout()
        self.canvas.draw_idle()
        
        # Info-Label aktualisieren
        loop_dur = self.loop_end - self.loop_start
        bpm = self.get_bpm()
        bars = loop_dur / self.calculate_bar_duration() if bpm > 0 else 0
        
        # Intro-Info hinzufügen wenn aktiv
        intro_text = ""
        if intro_start is not None:
            intro_bars = self.intro_bars.get()
            # Schöne Darstellung: ganze Zahlen ohne Dezimalpunkt
            if intro_bars == int(intro_bars):
                intro_text = f" | Intro: {intro_start:.3f}s ({int(intro_bars)} bars)"
            else:
                intro_text = f" | Intro: {intro_start:.3f}s ({intro_bars:.3g} bars)"
        
        self.time_label.config(text=f"Loop: {self.loop_start:.3f}s - {self.loop_end:.3f}s | Duration: {loop_dur:.3f}s | {bars:.1f} bars{intro_text}")

    def get_bpm(self):
        return button_data[self.button_id].get("bpm", 120.0) or 120.0

    def calculate_bar_duration(self):
        bpm = self.get_bpm()
        return 4.0 / (bpm / 60.0)

    def calculate_auto_loop_duration(self):
        return self.calculate_bar_duration() * self.auto_loop_bars.get()

    def update_auto_loop_display(self):
        self.bars_label.config(text=f"{self.auto_loop_bars.get()} bars")

    def on_auto_loop_toggle(self):
        button_data[self.button_id]["auto_loop_active"] = self.auto_loop_active.get()
        if self.auto_loop_active.get():
            self.apply_auto_loop_to_current_settings()
        self.update_waveform()

    def on_custom_mode_toggle(self):
        button_data[self.button_id]["auto_loop_custom_mode"] = self.auto_loop_custom_mode.get()

    def apply_auto_loop_to_current_settings(self):
        if not self.auto_loop_active.get() or self._loading:
            return
        new_dur = self.calculate_auto_loop_duration()
        if self.loop_start + new_dur <= self.duration:
            self.loop_end = self.loop_start + new_dur
        else:
            self.loop_start = max(0, self.duration - new_dur)
            self.loop_end = self.duration
        self.apply_loop_changes_realtime()

    def bars_up(self):
        current = self.auto_loop_bars.get()
        if self.auto_loop_custom_mode.get():
            new_val = min(64, current + 1)
        else:
            valid = [4, 8, 16, 32, 64]
            idx = valid.index(current) if current in valid else 1
            new_val = valid[min(len(valid)-1, idx+1)]
        self.auto_loop_bars.set(new_val)
        button_data[self.button_id]["auto_loop_bars"] = new_val
        self.update_auto_loop_display()
        if self.auto_loop_active.get():
            self.apply_auto_loop_to_current_settings()
            self.update_waveform()

    def bars_down(self):
        current = self.auto_loop_bars.get()
        if self.auto_loop_custom_mode.get():
            new_val = max(1, current - 1)
        else:
            valid = [4, 8, 16, 32, 64]
            idx = valid.index(current) if current in valid else 1
            new_val = valid[max(0, idx-1)]
        self.auto_loop_bars.set(new_val)
        button_data[self.button_id]["auto_loop_bars"] = new_val
        self.update_auto_loop_display()
        if self.auto_loop_active.get():
            self.apply_auto_loop_to_current_settings()
            self.update_waveform()

    # ===== INTRO METHODEN =====
    def calculate_intro_start_position(self):
        """Berechnet die Intro-Startposition basierend auf intro_bars"""
        if not self.intro_active.get():
            return None
        intro_duration = self.calculate_bar_duration() * self.intro_bars.get()
        intro_start = self.loop_start - intro_duration
        return max(0, intro_start)  # Nicht vor Trackstart

    def update_intro_display(self):
        """Aktualisiert das Intro-Bars Label"""
        bars = self.intro_bars.get()
        # Schöne Darstellung: ganze Zahlen ohne Dezimalpunkt, Brüche mit einer Stelle
        if bars == int(bars):
            self.intro_bars_label.config(text=f"{int(bars)} bars")
        else:
            self.intro_bars_label.config(text=f"{bars:.3g} bars")

    def on_intro_toggle(self):
        """Callback wenn Intro-Checkbox getoggelt wird"""
        button_data[self.button_id]["intro_active"] = self.intro_active.get()
        self.update_waveform()

    def on_intro_custom_mode_toggle(self):
        """Callback wenn Intro Custom-Mode getoggelt wird"""
        button_data[self.button_id]["intro_custom_mode"] = self.intro_custom_mode.get()

    def intro_bars_up(self):
        """Erhöht die Intro-Bars (Custom: 1/8 Bar Schritte, Normal: 1 Bar Schritte)"""
        current = self.intro_bars.get()
        if self.intro_custom_mode.get():
            # Custom: 1/8 Bar Schritte (0.125)
            new_val = min(64, current + 0.125)
        else:
            # Normal: 1 Bar Schritte
            new_val = min(64, current + 1)
        self.intro_bars.set(new_val)
        button_data[self.button_id]["intro_bars"] = new_val
        self.update_intro_display()
        self.update_waveform()

    def intro_bars_down(self):
        """Verringert die Intro-Bars (Custom: 1/8 Bar Schritte, Normal: 1 Bar Schritte)"""
        current = self.intro_bars.get()
        if self.intro_custom_mode.get():
            # Custom: 1/8 Bar Schritte (0.125), Minimum 0.125
            new_val = max(0.125, current - 0.125)
        else:
            # Normal: 1 Bar Schritte, Minimum 1
            new_val = max(1, current - 1)
        self.intro_bars.set(new_val)
        button_data[self.button_id]["intro_bars"] = new_val
        self.update_intro_display()
        self.update_waveform()

    def jump_to_start(self):
        self.zoom_start = 0.0
        self.zoom_end = min(self.duration, self.zoom_end - self.zoom_start)
        self.update_waveform()

    def jump_to_end(self):
        span = self.zoom_end - self.zoom_start
        self.zoom_end = self.duration
        self.zoom_start = max(0, self.duration - span)
        self.update_waveform()

    def on_click(self, event):
        if event.inaxes and event.xdata is not None:
            click_time = float(event.xdata)
            
            if event.button == 2:
                self.jump_to_position_smart(click_time)
                return
            
            if self.auto_loop_active.get():
                loop_dur = self.calculate_auto_loop_duration()
                
                if event.button == 1:
                    new_start = click_time
                    new_end = new_start + loop_dur
                    
                    if new_end > self.duration:
                        new_end = self.duration
                        new_start = max(0, new_end - loop_dur)
                    
                    self.loop_start = new_start
                    self.loop_end = new_end
                    
                elif event.button == 3:
                    new_end = click_time
                    new_start = new_end - loop_dur
                    
                    if new_start < 0:
                        new_start = 0
                        new_end = min(self.duration, loop_dur)
                    
                    self.loop_start = new_start
                    self.loop_end = new_end
            else:
                if event.button == 1:
                    self.loop_start = max(0, min(click_time, self.duration))
                    if self.loop_start >= self.loop_end:
                        self.loop_end = min(self.duration, self.loop_start + 0.1)
                elif event.button == 3:
                    self.loop_end = max(0, min(click_time, self.duration))
                    if self.loop_end <= self.loop_start:
                        self.loop_start = max(0, self.loop_end - 0.1)
            
            self.apply_loop_changes_realtime()
            self.update_waveform()

    def jump_to_position_smart(self, click_pos):
        """Smart jump: plays to loop end, then loops properly"""
        loop = button_data[self.button_id]["pyo"]
        if not loop or not loop._is_playing:
            return
        
        try:
            original_start = self.loop_start
            original_end = self.loop_end
            
            if click_pos < original_start:
                temp_start = click_pos
                temp_end = original_end
            elif click_pos >= original_start and click_pos < original_end:
                temp_start = click_pos
                temp_end = original_end
            else:
                temp_start = click_pos
                temp_end = self.duration
            
            loop.stop()
            loop.update_loop_points(temp_start, temp_end)
            loop.play()
            
            duration_until_loop = temp_end - temp_start
            speed = loop._pending_speed if loop._pending_speed else 1.0
            wait_ms = int((duration_until_loop / abs(speed)) * 1000)
            
            def restore_loop():
                if loop._is_playing:
                    loop.stop()
                    loop.update_loop_points(original_start, original_end)
                    loop.play()
            
            self.window.after(wait_ms, restore_loop)
            
        except Exception as e:
            logger.error(f"Error in smart jump: {type(e).__name__}: {e}")

    def apply_loop_changes_realtime(self):
        button_data[self.button_id]["loop_start"] = self.loop_start
        button_data[self.button_id]["loop_end"] = self.loop_end
        loop = button_data[self.button_id]["pyo"]
        if loop:
            loop.update_loop_points(self.loop_start, self.loop_end)

    def on_scroll(self, event):
        if event.inaxes and event.xdata:
            mouse_time = event.xdata
            zoom_factor = 0.8 if event.button == 'up' else 1.25
            current_span = self.zoom_end - self.zoom_start
            new_span = current_span * zoom_factor
            mouse_ratio = (mouse_time - self.zoom_start) / current_span
            new_start = mouse_time - (new_span * mouse_ratio)
            new_end = new_start + new_span
            self.zoom_start = max(0, new_start)
            self.zoom_end = min(self.duration, new_end)
            self.update_waveform_throttled()

    def zoom_by_factor(self, factor):
        center = (self.zoom_start + self.zoom_end) / 2
        span = (self.zoom_end - self.zoom_start) * factor
        self.zoom_start = max(0, center - span/2)
        self.zoom_end = min(self.duration, center + span/2)
        self.update_waveform()

    def zoom_fit(self):
        self.zoom_start = 0.0
        self.zoom_end = self.duration
        self.update_waveform()

    def reset_loop(self):
        self.loop_start = 0.0
        self.loop_end = self.duration
        self.apply_loop_changes_realtime()
        self.update_waveform()

    def toggle_playback(self):
        """Startet oder stoppt die Wiedergabe, synchronisiert mit dem Haupt-Button."""
        loop = button_data[self.button_id]["pyo"]
        if loop:
            if button_data[self.button_id]["active"]:
                # Stop
                loop.stop()
                button_data[self.button_id]["active"] = False
                buttons[self.button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
                update_button_label(self.button_id)
            else:
                # Play
                if not multi_loop_active.get():
                    # OPTIMIERUNG: Nur loaded_loops durchsuchen
                    for (bank_id, btn_id), other_loop in list(loaded_loops.items()):
                        if btn_id != self.button_id:
                            data = all_banks_data[bank_id][btn_id]
                            if data["active"]:
                                other_loop.stop()
                            data["active"] = False
                            if bank_id == current_bank.get():
                                buttons[btn_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
                                update_button_label(btn_id)
                loop.set_key_lock(key_lock_active.get())
                loop.play()
                button_data[self.button_id]["active"] = True
                buttons[self.button_id].config(bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE)
                update_button_label(self.button_id)
            self.update_play_button()

    def update_play_button(self):
        """Aktualisiert die Farbe des Play-Buttons basierend auf dem Wiedergabestatus."""
        if button_data[self.button_id]["active"]:
            self.play_btn.config(text="> Play", bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE)
        else:
            self.play_btn.config(text="> Play", bg="#444", fg=COLOR_TEXT)

    def undo_and_close(self):
        """Stellt die ursprünglichen Loop-Einstellungen wieder her und schließt das Fenster"""
        global open_loop_editor_windows
        
        # Ursprüngliche Werte wiederherstellen
        button_data[self.button_id]["loop_start"] = self._original_loop_start
        button_data[self.button_id]["loop_end"] = self._original_loop_end
        button_data[self.button_id]["auto_loop_active"] = self._original_auto_loop_active
        button_data[self.button_id]["auto_loop_bars"] = self._original_auto_loop_bars
        button_data[self.button_id]["auto_loop_custom_mode"] = self._original_auto_loop_custom_mode
        # Intro-Werte wiederherstellen
        button_data[self.button_id]["intro_active"] = self._original_intro_active
        button_data[self.button_id]["intro_bars"] = self._original_intro_bars
        button_data[self.button_id]["intro_custom_mode"] = self._original_intro_custom_mode
        
        # Loop-Punkte im PyoLoop aktualisieren
        loop = button_data[self.button_id].get("pyo")
        if loop:
            loop.update_loop_points(self._original_loop_start, self._original_loop_end)
        
        # Fenster schließen ohne zu speichern
        if self.button_id in open_loop_editor_windows:
            del open_loop_editor_windows[self.button_id]
        
        try:
            plt.close(self.fig)
            self.window.destroy()
        except (tk.TclError, AttributeError):
            pass  # Fenster oder Figure bereits geschlossen/zerstört

    def apply_and_close(self):
        """Speichert die aktuellen Loop-Einstellungen und schließt das Fenster"""
        self.on_window_close()

    def on_window_close(self):
        global open_loop_editor_windows
        save_config_async()
        
        if self.button_id in open_loop_editor_windows:
            del open_loop_editor_windows[self.button_id]
        
        try:
            plt.close(self.fig)
            self.window.destroy()
        except (tk.TclError, AttributeError):
            pass  # Fenster bereits geschlossen

# ============== CONTEXT MENU ==============
def open_context_menu(button_id, event):
    """Middle click - open context menu"""
    # Stems-Struktur sicherstellen (behebt KeyErrors bei alten Daten)
    ensure_stems_structure(button_data[button_id])
    
    menu = tk.Menu(root, tearoff=0, bg=COLOR_BG, fg=COLOR_TEXT)
    menu.add_command(label="Load Audio", command=lambda: load_loop(button_id))
    if button_data[button_id]["file"]:
        menu.add_command(label="Unload Audio", command=lambda: unload_loop(button_id))
        menu.add_separator()
        menu.add_command(label="Re-detect BPM",
            command=lambda: detect_bpm(button_data[button_id]['file'],
                lambda bpm: [button_data[button_id].update({'bpm': bpm}), update_button_label(button_id)]))
        menu.add_command(label="Set BPM manually", command=lambda: set_bpm_manually(button_id))
        menu.add_command(label="Adjust Loop", command=lambda: adjust_loop(button_id))
        menu.add_command(label="Volume + EQ", command=lambda: set_volume(button_id))
        # STEMS: Separator und Stem-Menüpunkte
        menu.add_separator()
        if button_data[button_id]["bpm"]:
            if button_data[button_id]["stems"]["generating"]:
                menu.add_command(label="⏳ Generating Stems...", state="disabled")
            elif button_data[button_id]["stems"]["available"]:
                menu.add_command(label="✓ Stems Available", state="disabled")
                menu.add_command(label="Regenerate Stems", command=lambda: generate_stems(button_id))
                menu.add_command(label="Delete Stems", command=lambda: delete_stems(button_id))
            else:
                menu.add_command(label="Generate Stems...", command=lambda: generate_stems(button_id))
        else:
            menu.add_command(label="Generate Stems (set BPM first)", state="disabled")
    menu.post(event.x_root, event.y_root)

def set_bpm_manually(button_id):
    """
    Öffnet einen Dialog zum manuellen Setzen der BPM.
    Enthält sowohl ein Eingabefeld als auch einen TAP BPM Button.
    """
    import time
    
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
        update_button_label(button_id)
        save_config_async()

def adjust_loop(button_id):
    if not button_data[button_id]["file"]:
        messagebox.showwarning("No Audio", "Load audio first.")
        return
    WaveformEditor(root, button_id)

# ============== GUI LAYOUT ==============
init_banks()

# Right frame for controls
right_frame = tk.Frame(root, bg=COLOR_BG)
right_frame.place(relx=1.0, rely=0.5, anchor="e", x=-20)

# BPM Display
bpm_display = tk.Label(right_frame, text="------", font=("Courier", 28, "bold"), 
                       fg="#ff3333", bg="black", width=6, height=1, anchor="e")
bpm_display.pack(pady=(0, 20))

# Container für Slider + Reset Button nebeneinander
slider_reset_frame = tk.Frame(right_frame, bg=COLOR_BG)
slider_reset_frame.pack()

# Pitch-Slider (links im Container)
speed_slider = tk.Scale(
    slider_reset_frame,
    from_=2.0,
    to=0.5,
    resolution=0.01,
    orient="vertical",
    length=400,
    variable=speed_value,
    command=on_speed_change,
    bg=COLOR_BG,
    fg=COLOR_TEXT,
    troughcolor="#444",
    highlightthickness=0,
    sliderlength=50,
    width=40
)
speed_slider.pack(side="left")

# Middle-click on slider resets pitch
speed_slider.bind("<Button-2>", lambda e: reset_pitch())

# Rechter Bereich neben Slider: BPM Up/Down Buttons + Reset
reset_frame = tk.Frame(slider_reset_frame, bg=COLOR_BG, width=60, height=400)
reset_frame.pack(side="left", padx=(5, 0))
reset_frame.pack_propagate(False)

# Spacer oben bis zu den Buttons (216px für korrekte 1.00 Position)
spacer_top = tk.Frame(reset_frame, bg=COLOR_BG, height=216)
spacer_top.pack()

# BPM Up Button (Pfeil nach oben) - grau
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
bpm_up_btn.bind("<Button-1>", on_bpm_up_click)
bpm_up_btn.bind("<Button-3>", on_bpm_up_click)

# Reset-Button - auf Höhe 1.00, startet grün (da Pitch initial 1.0)
reset_btn = tk.Button(
    reset_frame, 
    text="Reset",
    font=("Arial", 9, "bold"),
    fg=COLOR_TEXT_ACTIVE,
    bg=COLOR_BTN_ACTIVE,
    activebackground=COLOR_BTN_ACTIVE,
    activeforeground=COLOR_TEXT_ACTIVE,
    command=reset_pitch,
    relief="flat",
    bd=2,
    width=5
)
reset_btn.pack(pady=(0, 4))

# BPM Down Button (Pfeil nach unten) - grau
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
bpm_down_btn.bind("<Button-1>", on_bpm_down_click)
bpm_down_btn.bind("<Button-3>", on_bpm_down_click)

# Lock Buttons Frame (unter Slider)
lock_frame = tk.Frame(right_frame, bg=COLOR_BG)
lock_frame.pack(pady=(15, 0))

# KEY LOCK Button
key_lock_btn = tk.Button(lock_frame, text="KEY LOCK", command=toggle_key_lock, 
                         bg=COLOR_LOCK_OFF, fg=COLOR_TEXT, width=12)
key_lock_btn.pack(pady=(0, 5))

# BPM LOCK Button
bpm_lock_btn = tk.Button(lock_frame, text="BPM LOCK", command=toggle_bpm_lock, 
                         bg=COLOR_LOCK_OFF, fg=COLOR_TEXT, width=12)
bpm_lock_btn.pack(pady=(0, 5))

# BPM Entry mit Validierung (nur Zahlen, Punkt, Komma)
bpm_validate_cmd = root.register(validate_bpm_entry)
bpm_entry = tk.Entry(lock_frame, textvariable=master_bpm_value, width=12, 
                     justify="center", bg="#333", fg=COLOR_TEXT,
                     validate="key", validatecommand=(bpm_validate_cmd, "%P"),
                     insertbackground=COLOR_TEXT)
bpm_entry.pack()
bpm_entry.bind("<Return>", lambda e: update_speed_from_master_bpm())

# Master Volume Control at bottom left
volume_frame = tk.Frame(root, bg=COLOR_BG)
volume_frame.place(relx=0.02, rely=0.95, anchor="sw")

volume_label = tk.Label(volume_frame, text="Master Volume 100%", fg=COLOR_TEXT, bg=COLOR_BG, font=("Arial", 10))
volume_label.pack()

def on_master_volume_change(val):
    try:
        volume = float(val)
        master_volume.set(volume)
        master_amp.value = volume
        volume_percent = int(volume * 100)
        volume_label.config(text=f"Master Volume {volume_percent}%")
    except Exception as e:
        logger.error(f"Error setting master volume: {type(e).__name__}: {e}")

master_volume_slider = tk.Scale(
    volume_frame,
    from_=0.0,
    to=1.0,
    resolution=0.01,
    orient="horizontal",
    length=200,
    variable=master_volume,
    command=on_master_volume_change,
    bg=COLOR_BG,
    fg=COLOR_TEXT,
    troughcolor="#444",
    highlightthickness=0,
    showvalue=False
)
master_volume_slider.pack()

def reset_master_volume():
    master_volume_slider.set(1.0)

master_volume_slider.bind("<Double-Button-1>", lambda e: reset_master_volume())

# MULTI LOOP Button
multi_loop_frame = tk.Frame(root, bg=COLOR_BG)
multi_loop_frame.place(relx=0.02, rely=0.95, anchor="sw", x=250)

multi_loop_btn = tk.Button(
    multi_loop_frame, 
    text="MULTI LOOP", 
    command=toggle_multi_loop, 
    bg=COLOR_LOCK_OFF, 
    fg=COLOR_TEXT, 
    width=12
)
multi_loop_btn.pack()

# STEMS: Runde Toggle-Buttons für Stem-Separation
stems_frame = tk.Frame(root, bg=COLOR_BG)
stems_frame.place(relx=0.02, rely=0.95, anchor="sw", x=370)

stem_buttons = {}  # {stem_name: canvas}

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
        on_stem_toggle(stem_name)
    
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
        on_stem_momentary_activate(stem_name, activate=True)
        
        # Visuelles Feedback
        canvas.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
        canvas.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
    
    # Rechtsklick losgelassen: Ursprünglichen Zustand wiederherstellen
    def on_right_release(event):
        if not canvas._temp_state["right_held"]:
            return
        canvas._temp_state["right_held"] = False
        
        # Ursprünglichen Zustand wiederherstellen
        on_stem_momentary_release(stem_name)
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
            initialize_stem_players(active_loop)
        
        # Stem temporär deaktivieren (ohne State zu ändern)
        on_stem_momentary_activate(stem_name, activate=False)
        
        # Visuelles Feedback
        canvas.itemconfig("circle", fill=COLOR_BTN_INACTIVE, outline="#555")
        canvas.itemconfig("text", fill=COLOR_TEXT)
    
    # Mittelklick losgelassen: Ursprünglichen Zustand wiederherstellen
    def on_middle_release(event):
        if not canvas._temp_state["middle_held"]:
            return
        canvas._temp_state["middle_held"] = False
        
        # Ursprünglichen Zustand wiederherstellen
        on_stem_momentary_release(stem_name)
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
    stem_buttons[stem] = btn

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
        on_stop_stem_toggle()
        update_stop_stem_button_state()
    
    # Rechtsklick gedrückt: Temporär aktivieren (Original abspielen)
    def on_right_press(event):
        active_loop = get_active_loop_with_stems()
        if active_loop is None:
            return
        
        canvas._temp_state["right_held"] = True
        on_stop_stem_momentary(activate=True)
        
        # Visuelles Feedback
        canvas.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
        canvas.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
    
    # Rechtsklick losgelassen: Zurück zum ursprünglichen Zustand
    def on_right_release(event):
        if not canvas._temp_state["right_held"]:
            return
        canvas._temp_state["right_held"] = False
        
        on_stop_stem_momentary_release()
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
stop_stem_button = create_stop_stem_button(stems_frame)
stop_stem_button.pack(side="left", padx=(8, 2))  # Etwas mehr Abstand links

def update_stop_stem_button_state():
    """Aktualisiert den Stop-Stem Button basierend auf aktuellem Zustand."""
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


def get_active_loop_with_stems():
    """Gibt den aktiven Loop mit verfügbaren Stems zurück, oder None."""
    for btn_id, data in button_data.items():
        if data["active"] and data["stems"]["available"]:
            return btn_id
    return None


def update_stem_buttons_state():
    """Aktualisiert die Stem-Button-Farben basierend auf aktuellem Zustand."""
    active_loop_id = get_active_loop_with_stems()
    
    for stem, canvas in stem_buttons.items():
        if active_loop_id is None:
            # Kein aktiver Loop mit Stems -> alle grau und disabled-Look
            canvas.itemconfig("circle", fill="#2a2a2a", outline="#444")
            canvas.itemconfig("text", fill="#666")
        else:
            # Aktiver Loop mit Stems -> zeige Zustand
            data = button_data[active_loop_id]
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
    except NameError:
        pass  # Button existiert vielleicht noch nicht beim Startup
    except Exception as e:
        logger.debug(f"Could not update stop-stem button state: {e}")


# Create button grid
for i in range(GRID_SIZE):
    for j in range(GRID_SIZE):
        btn_id = i * GRID_SIZE + j + 1
        btn = tk.Button(
            root,
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
        btn.grid(row=i, column=j, padx=5, pady=5)
        btn.bind("<ButtonPress-1>", lambda e, b=btn_id: trigger_loop(b))
        btn.bind("<Button-2>", lambda e, b=btn_id: open_context_menu(b, e))
        btn.bind("<ButtonPress-3>", lambda e, b=btn_id: stop_loop(b))
        buttons[btn_id] = btn

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
        command=lambda b=bank_id: switch_bank(b)
    )
    btn.pack(side="left", padx=5, expand=True, fill="x")
    bank_buttons[bank_id] = btn

# ============== STARTUP ==============
load_config()
update_all_button_labels()
update_bpm_display()
process_gui_queue()
update_reset_button_style()

master_volume_slider.set(master_volume.get())

def cleanup_pitch_caches():
    """Gibt alle Pitch-Caches frei beim Beenden (RAM-basiert)."""
    try:
        for (bank_id, btn_id), loop in loaded_loops.items():
            if hasattr(loop, '_invalidate_pitch_cache'):
                loop._invalidate_pitch_cache()
    except Exception as e:
        logger.warning(f"Error cleaning up pitch caches: {type(e).__name__}: {e}")

def on_closing():
    """
    Proper shutdown handler - stoppt alle Audio-Objekte bevor das Fenster geschlossen wird.
    """
    try:
        # 1. Alle laufenden Loops stoppen
        for (bank_id, btn_id), loop in list(loaded_loops.items()):
            safe_stop(loop, "loop")
            
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
                            safe_stop(data["stems"][obj_name], "data")
                    # Master Phasor stoppen
                    if data["stems"].get("master_phasor"):
                        data["stems"]["master_phasor"].stop()
                    # Main Player stoppen
                    if data["stems"].get("main_player"):
                        data["stems"]["main_player"].stop()
                    # Stem Players stoppen
                    for stem in STEM_NAMES:
                        if data["stems"]["players"].get(stem):
                            safe_stop(data["stems"]["players"][stem], "data")
                        if data["stems"]["gains"].get(stem):
                            safe_stop(data["stems"]["gains"][stem], "data")
                except Exception as e:
                    logger.warning(f"Error stopping stems for {bank_id}/{btn_id}: {type(e).__name__}: {e}")
        
        # 2. Caches freigeben
        cleanup_pitch_caches()
        cleanup_stem_caches()
        
        # 3. Config speichern
        save_config_immediate()
        
        # 4. pyo Server stoppen
        safe_stop(s, "s")
        
        # 5. Executors herunterfahren
        try:
            io_executor.shutdown(wait=False)
            bpm_executor.shutdown(wait=False)
        except RuntimeError as e:
            # Executor war bereits heruntergefahren
            logger.debug(f"Executor already shut down: {e}")
        except Exception as e:
            logger.warning(f"Error shutting down executors: {type(e).__name__}: {e}")
        
    except Exception as e:
        logger.error(f"Error during shutdown: {type(e).__name__}: {e}")
    finally:
        # 6. Fenster zerstören
        root.destroy()

root.protocol("WM_DELETE_WINDOW", on_closing)
root.mainloop()

# Diese Zeilen werden nicht mehr erreicht, da on_closing bereits s.stop() aufruft
# s.stop()
# io_executor.shutdown(wait=False)
# bpm_executor.shutdown(wait=False)
