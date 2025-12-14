"""StemsPanelWidget - Kapselt die Stem-Kontrolle.

Enthält:
- 5 Stem-Toggle-Buttons (V, M, B, D, I)
- 1 Stop-Stem-Button (S)
- Alle zugehörigen Event-Handler
"""

import tkinter as tk
from collections.abc import Callable

from flitzis_looper.core.state import (
    COLOR_BG,
    COLOR_BTN_ACTIVE,
    COLOR_BTN_INACTIVE,
    COLOR_TEXT,
    COLOR_TEXT_ACTIVE,
    STEM_LABELS,
    STEM_NAMES,
    get_button_data,
    get_selected_stems_button,
)


class StemCanvas(tk.Canvas):
    """Custom Canvas widget for stem buttons with temporary state tracking."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._temp_state: dict[str, bool] = {"right_held": False, "middle_held": False}


class StemsPanelWidget(tk.Frame):
    """Widget für die Stem-Kontrolle.

    Attributes:
        stem_buttons: Dict der Stem-Buttons {stem_name: StemCanvas}
        stop_stem_button: Der Stop-Stem-Button
    """

    def __init__(
        self,
        parent: tk.Widget,
        callbacks: dict[str, Callable],
        **kwargs,
    ):
        """Initialisiert das StemsPanelWidget.

        Args:
            parent: Parent-Widget
            callbacks: Dict mit Callback-Funktionen:
                - on_stem_toggle: Callable[[str, dict], None]
                - on_stem_momentary_activate: Callable[[str, dict], None]
                - on_stem_momentary_release: Callable[[str, dict], None]
                - on_stop_stem_toggle: Callable[[dict], None]
                - on_stop_stem_momentary: Callable[[bool, dict], None]
                - on_stop_stem_momentary_release: Callable[[dict], None]
                - get_active_loop_with_stems: Callable[[], int | None]
                - update_stem_gains: Callable[[int], None]
                - stem_callbacks: dict - Callbacks für Stem-Operationen
        """
        super().__init__(parent, bg=COLOR_BG, **kwargs)

        self._callbacks = callbacks
        self._stem_buttons: dict[str, StemCanvas] = {}
        self._stop_stem_button: StemCanvas | None = None
        self._button_data = get_button_data()

        self._create_stem_label()
        self._create_stem_buttons()
        self._create_stop_stem_button()

    def _create_stem_label(self) -> None:
        """Erstellt das STEMS: Label."""
        label = tk.Label(
            self,
            text="STEMS:",
            fg="#888",
            bg=COLOR_BG,
            font=("Arial", 8),
        )
        label.pack(side="left", padx=(0, 5))

    def _create_stem_buttons(self) -> None:
        """Erstellt die 5 Stem-Toggle-Buttons."""
        for stem in STEM_NAMES:
            btn = self._create_stem_button(stem, STEM_LABELS[stem])
            btn.pack(side="left", padx=2)
            self._stem_buttons[stem] = btn

    def _create_stem_button(self, stem_name: str, label: str) -> StemCanvas:
        """Erstellt einen runden Toggle-Button für einen Stem.

        Args:
            stem_name: Name des Stems (z.B. "vocals")
            label: Anzeige-Label (z.B. "V")

        Returns:
            Das erstellte StemCanvas-Widget
        """
        size = 32  # Durchmesser
        canvas = StemCanvas(
            self,
            width=size,
            height=size,
            bg=COLOR_BG,
            highlightthickness=0,
            cursor="hand2",
        )

        # Kreis zeichnen
        canvas.create_oval(
            2,
            2,
            size - 2,
            size - 2,
            fill=COLOR_BTN_INACTIVE,
            outline="#555",
            width=2,
            tags="circle",
        )
        # Label
        canvas.create_text(
            size // 2,
            size // 2,
            text=label,
            fill=COLOR_TEXT,
            font=("Arial", 10, "bold"),
            tags="text",
        )

        # Event Bindings
        canvas.bind("<Button-1>", lambda e, s=stem_name: self._on_left_click(s))
        canvas.bind("<ButtonPress-3>", lambda e, s=stem_name, c=canvas: self._on_right_press(s, c))
        canvas.bind(
            "<ButtonRelease-3>", lambda e, s=stem_name, c=canvas: self._on_right_release(s, c)
        )
        canvas.bind("<ButtonPress-2>", lambda e, s=stem_name, c=canvas: self._on_middle_press(s, c))
        canvas.bind(
            "<ButtonRelease-2>", lambda e, s=stem_name, c=canvas: self._on_middle_release(s, c)
        )
        canvas.bind("<Enter>", lambda e: self._on_enter(canvas))

        return canvas

    def _on_left_click(self, stem_name: str) -> None:
        """Linksklick: Toggle Stem."""
        on_stem_toggle = self._callbacks.get("on_stem_toggle")
        stem_callbacks_obj: dict | Callable = self._callbacks.get("stem_callbacks", {})
        stem_callbacks: dict = stem_callbacks_obj if isinstance(stem_callbacks_obj, dict) else {}

        if on_stem_toggle:
            on_stem_toggle(stem_name, stem_callbacks)
            self.update_state()

    def _on_right_press(self, stem_name: str, canvas: StemCanvas) -> None:
        """Rechtsklick gedrückt: Temporär Solo."""
        get_active = self._callbacks.get("get_active_loop_with_stems")
        on_momentary = self._callbacks.get("on_stem_momentary_activate")
        stem_callbacks_obj: dict | Callable = self._callbacks.get("stem_callbacks", {})
        stem_callbacks: dict = stem_callbacks_obj if isinstance(stem_callbacks_obj, dict) else {}

        if get_active and get_active() is None:
            return

        canvas._temp_state["right_held"] = True

        if on_momentary:
            on_momentary(stem_name, stem_callbacks)

        # Visuelles Feedback
        canvas.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
        canvas.itemconfig("text", fill=COLOR_TEXT_ACTIVE)

    def _on_right_release(self, stem_name: str, canvas: StemCanvas) -> None:
        """Rechtsklick losgelassen: Solo aufheben."""
        if not canvas._temp_state["right_held"]:
            return
        canvas._temp_state["right_held"] = False

        on_release = self._callbacks.get("on_stem_momentary_release")
        update_gains = self._callbacks.get("update_stem_gains")

        if on_release and update_gains:
            on_release(stem_name, {"update_stem_gains": update_gains})

        self.update_state()

    def _on_middle_press(self, stem_name: str, canvas: StemCanvas) -> None:
        """Mittelklick gedrückt: Temporär Mute."""
        get_active = self._callbacks.get("get_active_loop_with_stems")
        self._callbacks.get("on_stem_momentary_activate")
        stem_callbacks_obj: dict | Callable = self._callbacks.get("stem_callbacks", {})
        stem_callbacks_obj if isinstance(stem_callbacks_obj, dict) else {}

        if not get_active:
            return

        active_loop_id = get_active()
        if active_loop_id is None:
            return

        canvas._temp_state["middle_held"] = True

        # Aktuellen State speichern

        if self._button_data is None:
            return
        data = self._button_data[active_loop_id]
        was_active = data["stems"]["states"].get(stem_name, False)

        # Wenn Stem aktiv war, deaktivieren (mute)
        if was_active:
            data["stems"]["states"][stem_name] = False
            update_gains = self._callbacks.get("update_stem_gains")
            if update_gains:
                update_gains(active_loop_id)

        # Visuelles Feedback - dunkel für mute
        canvas.itemconfig("circle", fill="#2a2a2a", outline="#444")
        canvas.itemconfig("text", fill=COLOR_TEXT)

    def _on_middle_release(self, stem_name: str, canvas: StemCanvas) -> None:
        """Mittelklick losgelassen: Mute aufheben."""
        if not canvas._temp_state["middle_held"]:
            return
        canvas._temp_state["middle_held"] = False

        on_release = self._callbacks.get("on_stem_momentary_release")
        update_gains = self._callbacks.get("update_stem_gains")

        if on_release and update_gains:
            on_release(stem_name, {"update_stem_gains": update_gains})

        self.update_state()

    def _on_enter(self, canvas: StemCanvas) -> None:
        """Mauszeiger betritt Button."""
        get_active = self._callbacks.get("get_active_loop_with_stems")
        if get_active and get_active():
            canvas.config(cursor="hand2")
        else:
            canvas.config(cursor="arrow")

    def _create_stop_stem_button(self) -> None:
        """Erstellt den Stop-Stem Button."""
        size = 32
        canvas = StemCanvas(
            self,
            width=size,
            height=size,
            bg=COLOR_BG,
            highlightthickness=0,
            cursor="hand2",
        )

        # Kreis zeichnen
        canvas.create_oval(
            2,
            2,
            size - 2,
            size - 2,
            fill=COLOR_BTN_INACTIVE,
            outline="#555",
            width=2,
            tags="circle",
        )
        # Label - fettgedrucktes S
        canvas.create_text(
            size // 2,
            size // 2,
            text="S",
            fill=COLOR_TEXT,
            font=("Arial", 11, "bold"),
            tags="text",
        )

        # Event Bindings
        canvas.bind("<Button-1>", lambda e: self._on_stop_left_click())
        canvas.bind("<ButtonPress-3>", lambda e: self._on_stop_right_press(canvas))
        canvas.bind("<ButtonRelease-3>", lambda e: self._on_stop_right_release(canvas))
        canvas.bind("<Enter>", lambda e: self._on_enter(canvas))

        canvas.pack(side="left", padx=(8, 2))
        self._stop_stem_button = canvas

    def _on_stop_left_click(self) -> None:
        """Linksklick auf Stop-Stem: Toggle."""
        on_toggle = self._callbacks.get("on_stop_stem_toggle")
        stem_callbacks_obj: dict | Callable = self._callbacks.get("stem_callbacks", {})
        stem_callbacks: dict = stem_callbacks_obj if isinstance(stem_callbacks_obj, dict) else {}

        if on_toggle:
            on_toggle(stem_callbacks)
            self.update_stop_state()

    def _on_stop_right_press(self, canvas: StemCanvas) -> None:
        """Rechtsklick auf Stop-Stem: Temporär aktivieren."""
        get_active = self._callbacks.get("get_active_loop_with_stems")
        on_momentary = self._callbacks.get("on_stop_stem_momentary")
        stem_callbacks_obj: dict | Callable = self._callbacks.get("stem_callbacks", {})
        stem_callbacks: dict = stem_callbacks_obj if isinstance(stem_callbacks_obj, dict) else {}

        if get_active and get_active() is None:
            return

        canvas._temp_state["right_held"] = True

        if on_momentary:
            on_momentary(True, stem_callbacks)

        # Visuelles Feedback
        canvas.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
        canvas.itemconfig("text", fill=COLOR_TEXT_ACTIVE)

    def _on_stop_right_release(self, canvas: StemCanvas) -> None:
        """Rechtsklick auf Stop-Stem losgelassen."""
        if not canvas._temp_state["right_held"]:
            return
        canvas._temp_state["right_held"] = False

        on_release = self._callbacks.get("on_stop_stem_momentary_release")
        update_gains = self._callbacks.get("update_stem_gains")

        if on_release and update_gains:
            on_release({
                "update_stem_gains": update_gains,
                "update_stem_buttons_state": self.update_state,
            })

        self.update_stop_state()

    def update_state(self) -> None:
        """Aktualisiert alle Stem-Button-Farben basierend auf aktuellem Zustand."""
        selected = get_selected_stems_button()
        get_active = self._callbacks.get("get_active_loop_with_stems")

        # Verwende selected wenn gesetzt, sonst aktiven Loop
        target_id = selected
        if target_id is None and get_active:
            target_id = get_active()

        # Prüfe ob target gültig ist
        if target_id is not None:
            if self._button_data is None:
                target_id = None
            else:
                data = self._button_data.get(target_id)
                if not data or not data.get("stems", {}).get("available"):
                    target_id = None

        for stem, canvas in self._stem_buttons.items():
            if target_id is None:
                # Kein Loop mit Stems -> grau
                canvas.itemconfig("circle", fill="#2a2a2a", outline="#444")
                canvas.itemconfig("text", fill="#666")
            else:
                # Zeige Zustand
                if self._button_data is None:
                    return
                data = self._button_data[target_id]
                is_active = data["stems"]["states"].get(stem, False)

                if is_active:
                    canvas.itemconfig("circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a")
                    canvas.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
                else:
                    canvas.itemconfig("circle", fill=COLOR_BTN_INACTIVE, outline="#555")
                    canvas.itemconfig("text", fill=COLOR_TEXT)

        # Stop-Button auch aktualisieren
        self.update_stop_state()

    def update_stop_state(self) -> None:
        """Aktualisiert den Stop-Stem Button."""
        if self._stop_stem_button is None:
            return

        get_active = self._callbacks.get("get_active_loop_with_stems")
        active_id = get_active() if get_active else None

        if active_id is None:
            # Kein aktiver Loop -> grau
            self._stop_stem_button.itemconfig("circle", fill="#2a2a2a", outline="#444")
            self._stop_stem_button.itemconfig("text", fill="#666")
        else:
            if self._button_data is None:
                return
            data = self._button_data[active_id]
            is_stop_active = data["stems"].get("stop_active", False)

            if is_stop_active:
                self._stop_stem_button.itemconfig(
                    "circle", fill=COLOR_BTN_ACTIVE, outline="#1a9a5a"
                )
                self._stop_stem_button.itemconfig("text", fill=COLOR_TEXT_ACTIVE)
            else:
                self._stop_stem_button.itemconfig("circle", fill=COLOR_BTN_INACTIVE, outline="#555")
                self._stop_stem_button.itemconfig("text", fill=COLOR_TEXT)

    @property
    def stem_buttons(self) -> dict[str, StemCanvas]:
        """Gibt alle Stem-Buttons zurück."""
        return self._stem_buttons

    @property
    def stop_stem_button(self) -> StemCanvas | None:
        """Gibt den Stop-Stem-Button zurück."""
        return self._stop_stem_button
