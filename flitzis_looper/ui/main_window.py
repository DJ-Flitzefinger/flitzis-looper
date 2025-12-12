"""Main Window Setup für flitzis_looper.

Enthält:
- create_main_window(): Erstellt und konfiguriert das Hauptfenster
- Window-Konstanten (Titel, Größe)
"""

import tkinter as tk

from flitzis_looper.core.state import COLOR_BG

# Window constants
WINDOW_TITLE = "Dj Flitzefinger's Scratch-Looper"
WINDOW_GEOMETRY = "960x630"
WINDOW_RESIZABLE = (False, False)


def create_main_window():
    """Erstellt und konfiguriert das Hauptfenster.

    Returns:
        tk.Tk: Das konfigurierte Root-Fenster
    """
    root = tk.Tk()
    root.title(WINDOW_TITLE)
    root.geometry(WINDOW_GEOMETRY)
    root.resizable(*WINDOW_RESIZABLE)
    root.configure(bg=COLOR_BG)
    return root


def setup_window_protocol(root, on_closing_callback):
    """Setzt den Window-Close-Handler.

    Args:
        root: Das Tk-Root-Fenster
        on_closing_callback: Funktion die beim Schließen aufgerufen wird
    """
    root.protocol("WM_DELETE_WINDOW", on_closing_callback)
