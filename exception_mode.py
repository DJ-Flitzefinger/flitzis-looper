"""
exception_mode.py - Industrietaugliches, latenzschonendes Exception-Handling und Logging

Konfigurierbare Modi über config.json (logging_mode: 0/1/2):
- OFF (0): Kein Logging, keine Traceback-Formatierung (No-Ops)
- WARN (1): Asynchrones Logging, Level WARNING+ 
- DEBUG (2): Asynchrones Logging, Level DEBUG

Audio-Hot-Paths werden NICHT blockiert - alles läuft über QueueHandler/QueueListener.
"""

import json
import logging
import logging.handlers
import os
import sys
import threading
import queue
import atexit
from concurrent.futures import ThreadPoolExecutor
from typing import Optional, Callable
from functools import wraps

# ============== KONSTANTEN ==============
OFF = 0
WARN = 1
DEBUG = 2

# ============== GLOBALE VARIABLEN ==============
_current_mode: int = WARN
_queue_listener: Optional[logging.handlers.QueueListener] = None
_log_queue: Optional[queue.Queue] = None
_is_initialized: bool = False


def load_mode_from_config(path: str = "config.json", key: str = "logging_mode", default: int = WARN) -> int:
    """
    Lädt den Logging-Modus aus der config.json.
    
    Args:
        path: Pfad zur config.json
        key: Schlüssel für den Logging-Modus
        default: Fallback-Wert bei Fehlern
    
    Returns:
        Modus (0=OFF, 1=WARN, 2=DEBUG)
    """
    try:
        if not os.path.exists(path):
            return default
        
        with open(path, "r") as f:
            config = json.load(f)
        
        mode = config.get(key, default)
        
        # Validierung
        if isinstance(mode, int) and mode in (OFF, WARN, DEBUG):
            return mode
        
        return default
    except (json.JSONDecodeError, IOError, KeyError, TypeError):
        return default


def setup_exception_handling(mode: int, app_name: str = "FlitzisLooper", 
                              log_path: str = "flitzis_looper.log") -> int:
    """
    Richtet das Exception-Handling und Logging basierend auf dem Modus ein.
    
    Args:
        mode: Logging-Modus (OFF/WARN/DEBUG)
        app_name: Name der Applikation (für Log-Format)
        log_path: Pfad zur Log-Datei
    
    Returns:
        Der tatsächlich verwendete Modus
    """
    global _current_mode, _queue_listener, _log_queue, _is_initialized
    
    # Verhindern, dass mehrfach initialisiert wird
    if _is_initialized:
        return _current_mode
    
    _current_mode = mode
    root_logger = logging.getLogger()
    
    # Alle bestehenden Handler entfernen
    for handler in root_logger.handlers[:]:
        root_logger.removeHandler(handler)
        handler.close()
    
    if mode == OFF:
        # OFF-Modus: Alles deaktivieren
        logging.disable(logging.CRITICAL)
        _is_initialized = True
        _setup_global_hooks()
        return mode
    
    # WARN oder DEBUG Modus
    logging.disable(logging.NOTSET)
    
    # Root-Logger auf DEBUG setzen (effektive Filterung via Handler-Level)
    root_logger.setLevel(logging.DEBUG)
    
    # Queue für asynchrones Logging
    _log_queue = queue.Queue(-1)  # Unbegrenzte Queue
    
    # QueueHandler am Root-Logger
    queue_handler = logging.handlers.QueueHandler(_log_queue)
    root_logger.addHandler(queue_handler)
    
    # Handler-Level basierend auf Modus
    handler_level = logging.WARNING if mode == WARN else logging.DEBUG
    
    # RotatingFileHandler für die eigentliche Datei-Ausgabe
    try:
        file_handler = logging.handlers.RotatingFileHandler(
            log_path,
            maxBytes=5 * 1024 * 1024,  # 5 MB
            backupCount=3,
            encoding='utf-8'
        )
        file_handler.setLevel(handler_level)
        
        # Format mit Timestamp und Thread-Info
        formatter = logging.Formatter(
            f'%(asctime)s - {app_name} - %(levelname)s - [%(threadName)s] - %(name)s - %(message)s',
            datefmt='%Y-%m-%d %H:%M:%S'
        )
        file_handler.setFormatter(formatter)
        
        # QueueListener startet den Handler im Hintergrund-Thread
        _queue_listener = logging.handlers.QueueListener(
            _log_queue,
            file_handler,
            respect_handler_level=True
        )
        _queue_listener.start()
        
        # Bei Programmende sauber beenden
        atexit.register(_shutdown_logging)
        
    except (IOError, OSError) as e:
        # Fallback: Nur Console-Logging
        console_handler = logging.StreamHandler()
        console_handler.setLevel(handler_level)
        console_handler.setFormatter(logging.Formatter(
            f'{app_name} - %(levelname)s - %(message)s'
        ))
        root_logger.addHandler(console_handler)
        print(f"Warning: Could not create log file, using console only: {e}", file=sys.stderr)
    
    _is_initialized = True
    _setup_global_hooks()
    
    # Startmeldung loggen
    logger = logging.getLogger(__name__)
    logger.info(f"Logging initialized in {'WARN' if mode == WARN else 'DEBUG'} mode")
    
    return mode


def _shutdown_logging():
    """Beendet das Logging sauber beim Programmende."""
    global _queue_listener
    if _queue_listener:
        try:
            _queue_listener.stop()
        except Exception:
            pass


def _report_exception(exc_type, exc_val, exc_tb, level: int = logging.ERROR, 
                      msg: str = "Exception occurred"):
    """
    Zentrale Funktion zum Melden von Exceptions.
    
    In OFF-Modus: Sofortiges Return (No-Op).
    Sonst: Asynchrones Logging via QueueHandler (keine Formatierung im Hot-Path).
    
    Args:
        exc_type: Exception-Typ
        exc_val: Exception-Wert
        exc_tb: Traceback
        level: Log-Level (ERROR, CRITICAL, etc.)
        msg: Nachricht für das Log
    """
    global _current_mode
    
    # In OFF-Modus: Schneller Return (No-Op)
    if _current_mode == OFF:
        return
    
    try:
        logger = logging.getLogger("exception_handler")
        # exc_info als Tuple übergeben - Formatierung erfolgt asynchron im QueueListener
        logger.log(level, msg, exc_info=(exc_type, exc_val, exc_tb))
    except Exception:
        # Letzte Verteidigungslinie - nie eine Exception aus dem Handler werfen
        pass


def _setup_global_hooks():
    """Richtet die globalen Exception-Hooks ein."""
    global _current_mode
    
    # sys.excepthook für ungefangene Exceptions im Hauptthread
    def custom_excepthook(exc_type, exc_val, exc_tb):
        _report_exception(exc_type, exc_val, exc_tb, 
                         level=logging.CRITICAL, 
                         msg="Uncaught exception in main thread")
        # Original-Verhalten beibehalten (Ausgabe auf stderr)
        sys.__excepthook__(exc_type, exc_val, exc_tb)
    
    sys.excepthook = custom_excepthook
    
    # threading.excepthook für ungefangene Exceptions in Threads (Python >= 3.8)
    if hasattr(threading, 'excepthook'):
        def custom_threading_excepthook(args):
            thread_name = args.thread.name if args.thread else "unknown"
            _report_exception(args.exc_type, args.exc_value, args.exc_traceback,
                             level=logging.CRITICAL,
                             msg=f"Uncaught exception in thread '{thread_name}'")
        
        threading.excepthook = custom_threading_excepthook


def install_tk_hook(root):
    """
    Installiert den Exception-Hook für Tkinter-Callbacks.
    
    Args:
        root: Das Tk-Root-Widget
    """
    global _current_mode
    
    def report_callback_exception(exc_type, exc_val, exc_tb):
        _report_exception(exc_type, exc_val, exc_tb,
                         level=logging.ERROR,
                         msg="Tkinter callback exception")
    
    root.report_callback_exception = report_callback_exception


def wrap_executor(executor: ThreadPoolExecutor, name: str = "executor") -> ThreadPoolExecutor:
    """
    Wrapped einen ThreadPoolExecutor um ungefangene Exceptions aus Tasks zu loggen.
    
    Args:
        executor: Der zu wrappende Executor
        name: Name für Log-Meldungen
    
    Returns:
        Der gleiche Executor (Referenz), aber mit gewrappter submit-Methode
    """
    global _current_mode
    
    original_submit = executor.submit
    
    def wrapped_submit(fn, *args, **kwargs):
        future = original_submit(fn, *args, **kwargs)
        
        def done_callback(f):
            # In OFF-Modus: Schneller Return
            if _current_mode == OFF:
                return
            
            try:
                exc = f.exception()
                if exc is not None:
                    # Exception aus dem Task - loggen
                    try:
                        # Versuche den Traceback zu bekommen
                        import traceback
                        tb = exc.__traceback__
                        _report_exception(type(exc), exc, tb,
                                         level=logging.ERROR,
                                         msg=f"Exception in {name} task")
                    except Exception:
                        # Fallback ohne Traceback
                        logger = logging.getLogger("exception_handler")
                        logger.error(f"Exception in {name} task: {exc}")
            except Exception:
                # Callback darf nie crashen
                pass
        
        future.add_done_callback(done_callback)
        return future
    
    executor.submit = wrapped_submit
    return executor


def get_current_mode() -> int:
    """Gibt den aktuellen Logging-Modus zurück."""
    return _current_mode


def is_logging_enabled() -> bool:
    """Prüft ob Logging aktiviert ist."""
    return _current_mode != OFF
