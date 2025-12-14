"""ContextMenuBuilder - Kontextmenü für Loop-Buttons.

Kapselt die Erstellung und Anzeige des Kontextmenüs.
"""

import tkinter as tk
from collections.abc import Callable
from tkinter import messagebox

from flitzis_looper.core.state import (
    COLOR_BG,
    COLOR_TEXT,
    ensure_stems_structure,
    get_button_data,
    get_root,
)


class LoopContextMenu:
    """Kontextmenü für Loop-Buttons.

    Zeigt Optionen wie Load/Unload Audio, BPM, Loop-Einstellungen, Stems etc.

    Example:
        >>> menu = LoopContextMenu(callbacks)
        >>> menu.show(button_id=5, event=click_event)
    """

    def __init__(self, callbacks: dict[str, Callable]):
        """Initialisiert das Kontextmenü.

        Args:
            callbacks: Dict mit Callback-Funktionen:
                - load_loop: Callable[[int, dict], None]
                - unload_loop: Callable[[int, dict], None]
                - detect_bpm: Callable[[str, Callable], None]
                - set_bpm_manually: Callable[[int, Callable, Callable], None]
                - open_waveform_editor: Callable[[int], None]
                - set_volume: Callable[[int, Callable, Callable], None]
                - generate_stems: Callable[[int, ...], None]
                - delete_stems: Callable[[int, ...], None]
                - update_button_label: Callable[[int], None]
                - update_stem_buttons_state: Callable[[], None]
                - save_config_async: Callable[[], None]
                - loop_callbacks: dict
        """
        self._callbacks = callbacks
        self._button_data = get_button_data()

    def show(self, button_id: int, event: tk.Event) -> None:
        """Zeigt das Kontextmenü an.

        Args:
            button_id: ID des Buttons
            event: Das Klick-Event (für Position)
        """
        root = get_root()
        if root is None:
            return

        # Stems-Struktur sicherstellen
        ensure_stems_structure(self._button_data[button_id])

        menu = tk.Menu(root, tearoff=0, bg=COLOR_BG, fg=COLOR_TEXT)

        # Load Audio
        self._add_load_command(menu, button_id)

        if self._button_data[button_id]["file"]:
            # Unload Audio
            self._add_unload_command(menu, button_id)
            menu.add_separator()

            # BPM-Optionen
            self._add_bpm_commands(menu, button_id)

            # Loop/Volume-Optionen
            self._add_loop_commands(menu, button_id)

            # Stems-Optionen
            menu.add_separator()
            self._add_stems_commands(menu, button_id)

        menu.post(event.x_root, event.y_root)

    def _add_load_command(self, menu: tk.Menu, button_id: int) -> None:
        """Fügt Load Audio Menüpunkt hinzu."""
        load_loop = self._callbacks.get("load_loop")
        loop_callbacks = self._callbacks.get("loop_callbacks", {})

        if load_loop:
            menu.add_command(
                label="Load Audio",
                command=lambda: load_loop(button_id, loop_callbacks),
            )

    def _add_unload_command(self, menu: tk.Menu, button_id: int) -> None:
        """Fügt Unload Audio Menüpunkt hinzu."""
        unload_loop = self._callbacks.get("unload_loop")
        loop_callbacks = self._callbacks.get("loop_callbacks", {})

        if unload_loop:
            menu.add_command(
                label="Unload Audio",
                command=lambda: unload_loop(button_id, loop_callbacks),
            )

    def _add_bpm_commands(self, menu: tk.Menu, button_id: int) -> None:
        """Fügt BPM-bezogene Menüpunkte hinzu."""
        detect_bpm = self._callbacks.get("detect_bpm")
        set_bpm_manually = self._callbacks.get("set_bpm_manually")
        update_button_label = self._callbacks.get("update_button_label")
        save_config_async = self._callbacks.get("save_config_async")

        # Re-detect BPM
        if detect_bpm and update_button_label:
            menu.add_command(
                label="Re-detect BPM",
                command=lambda: detect_bpm(
                    self._button_data[button_id]["file"],
                    lambda bpm: [
                        self._button_data[button_id].update({"bpm": bpm}),
                        update_button_label(button_id),
                    ],
                ),
            )

        # Set BPM manually
        if set_bpm_manually and update_button_label and save_config_async:
            menu.add_command(
                label="Set BPM manually",
                command=lambda: set_bpm_manually(button_id, update_button_label, save_config_async),
            )

    def _add_loop_commands(self, menu: tk.Menu, button_id: int) -> None:
        """Fügt Loop/Volume-bezogene Menüpunkte hinzu."""
        WaveformEditor = self._callbacks.get("WaveformEditor")
        set_volume = self._callbacks.get("set_volume")
        update_button_label = self._callbacks.get("update_button_label")
        update_stem_eq = self._callbacks.get("update_stem_eq")
        save_config_async = self._callbacks.get("save_config_async")

        root = get_root()

        # Adjust Loop
        if WaveformEditor and update_button_label and save_config_async:

            def open_loop_editor():
                if not self._button_data[button_id]["file"]:
                    messagebox.showwarning("No Audio", "Load audio first.")
                    return
                WaveformEditor(root, button_id, update_button_label, save_config_async)

            menu.add_command(label="Adjust Loop", command=open_loop_editor)

        # Volume + EQ
        if set_volume and update_stem_eq and save_config_async:
            menu.add_command(
                label="Volume + EQ",
                command=lambda: set_volume(button_id, update_stem_eq, save_config_async),
            )

    def _add_stems_commands(self, menu: tk.Menu, button_id: int) -> None:
        """Fügt Stems-bezogene Menüpunkte hinzu."""
        generate_stems = self._callbacks.get("generate_stems")
        delete_stems = self._callbacks.get("delete_stems")
        update_button_label = self._callbacks.get("update_button_label")
        update_stem_buttons_state = self._callbacks.get("update_stem_buttons_state")
        save_config_async = self._callbacks.get("save_config_async")

        data = self._button_data[button_id]

        if not data["bpm"]:
            menu.add_command(label="Generate Stems (set BPM first)", state="disabled")
            return

        if data["stems"]["generating"]:
            menu.add_command(label="⏳ Generating Stems", state="disabled")

        elif data["stems"]["available"]:
            menu.add_command(label="✓ Stems Available", state="disabled")

            if generate_stems:
                menu.add_command(
                    label="Regenerate Stems",
                    command=lambda: generate_stems(
                        button_id,
                        update_button_label,
                        update_stem_buttons_state,
                        save_config_async,
                    ),
                )

            if delete_stems:
                menu.add_command(
                    label="Delete Stems",
                    command=lambda: delete_stems(
                        button_id,
                        update_button_label,
                        update_stem_buttons_state,
                        save_config_async,
                    ),
                )

        elif generate_stems:
            menu.add_command(
                label="Generate Stems",
                command=lambda: generate_stems(
                    button_id,
                    update_button_label,
                    update_stem_buttons_state,
                    save_config_async,
                ),
            )
