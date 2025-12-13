"""LoopGridWidget - Kapselt das 6x6 Loop-Button-Grid.

Enthält:
- 36 Loop-Buttons mit Stem-Indikatoren
- 6 Bank-Buttons
- Alle zugehörigen Event-Handler
"""

import tkinter as tk
from collections.abc import Callable

from flitzis_looper.core.state import (
    COLOR_BANK_ACTIVE,
    COLOR_BANK_BTN,
    COLOR_BG,
    COLOR_BTN_INACTIVE,
    COLOR_STEM_INACTIVE,
    COLOR_TEXT,
    COLOR_TEXT_ACTIVE,
    GRID_SIZE,
    NUM_BANKS,
    register_bank_button,
    register_button,
    register_stem_indicator,
)


class LoopGridWidget(tk.Frame):
    """Widget für das Loop-Button-Grid mit Bank-Buttons.

    Attributes:
        buttons: Dict der Loop-Buttons {btn_id: tk.Button}
        bank_buttons: Dict der Bank-Buttons {bank_id: tk.Button}
        stem_indicators: Dict der Stem-Indikatoren {btn_id: tk.Label}
    """

    def __init__(
        self,
        parent: tk.Widget,
        callbacks: dict[str, Callable],
        **kwargs,
    ):
        """Initialisiert das LoopGridWidget.

        Args:
            parent: Parent-Widget
            callbacks: Dict mit Callback-Funktionen:
                - trigger_loop: Callable[[int, dict], None]
                - stop_loop: Callable[[int, dict], None]
                - open_context_menu: Callable[[int, tk.Event], None]
                - switch_bank: Callable[[int, Callable], None]
                - select_stems_button: Callable[[int, Callable], None]
                - update_stem_buttons_state: Callable[[], None]
                - loop_callbacks: dict - Callbacks für trigger_loop/stop_loop
        """
        super().__init__(parent, bg=COLOR_BG, **kwargs)

        self._callbacks = callbacks
        self._buttons: dict[int, tk.Button] = {}
        self._bank_buttons: dict[int, tk.Button] = {}
        self._stem_indicators: dict[int, tk.Label] = {}

        self._create_loop_grid()
        self._create_bank_buttons()

    def _create_loop_grid(self) -> None:
        """Erstellt das 6x6 Loop-Button-Grid."""
        trigger_loop = self._callbacks.get("trigger_loop")
        stop_loop = self._callbacks.get("stop_loop")
        open_context_menu = self._callbacks.get("open_context_menu")
        select_stems_button = self._callbacks.get("select_stems_button")
        update_stem_buttons_state = self._callbacks.get("update_stem_buttons_state")
        loop_callbacks = self._callbacks.get("loop_callbacks", {})

        for i in range(GRID_SIZE):
            for j in range(GRID_SIZE):
                btn_id = i * GRID_SIZE + j + 1

                # Container Frame für Button + Stem-Indikator
                container = tk.Frame(self, bg=COLOR_BG)
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
                    highlightthickness=0,
                )
                btn.pack(fill="both", expand=True)

                # Event Bindings
                if trigger_loop:
                    btn.bind(
                        "<ButtonPress-1>",
                        lambda e, b=btn_id: trigger_loop(b, loop_callbacks),
                    )
                if open_context_menu:
                    btn.bind("<Button-2>", lambda e, b=btn_id: open_context_menu(b, e))
                if stop_loop:
                    btn.bind(
                        "<ButtonPress-3>",
                        lambda e, b=btn_id: stop_loop(b, loop_callbacks),
                    )

                # Registriere Button im State
                register_button(btn_id, btn)
                self._buttons[btn_id] = btn

                # Stem-Indikator (kleines S-Quadrat unten rechts)
                stem_indicator = tk.Label(
                    container,
                    text="S",
                    font=("Arial", 7, "bold"),
                    bg=COLOR_STEM_INACTIVE,
                    fg="#888888",
                    width=2,
                    height=1,
                    cursor="hand2",
                )
                stem_indicator.place(relx=1.0, rely=1.0, anchor="se", x=-3, y=-3)

                if select_stems_button and update_stem_buttons_state:
                    stem_indicator.bind(
                        "<Button-1>",
                        lambda e, b=btn_id: select_stems_button(b, update_stem_buttons_state),
                    )

                register_stem_indicator(btn_id, stem_indicator)
                self._stem_indicators[btn_id] = stem_indicator

    def _create_bank_buttons(self) -> None:
        """Erstellt die Bank-Buttons unterhalb des Grids."""
        switch_bank = self._callbacks.get("switch_bank")
        update_stem_buttons_state = self._callbacks.get("update_stem_buttons_state")

        # Bank buttons row
        bank_frame = tk.Frame(self, bg=COLOR_BG)
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
            )

            if switch_bank and update_stem_buttons_state:
                btn.config(command=lambda b=bank_id: switch_bank(b, update_stem_buttons_state))

            btn.pack(side="left", padx=5, expand=True, fill="x")
            register_bank_button(bank_id, btn)
            self._bank_buttons[bank_id] = btn

    @property
    def buttons(self) -> dict[int, tk.Button]:
        """Gibt alle Loop-Buttons zurück."""
        return self._buttons

    @property
    def bank_buttons(self) -> dict[int, tk.Button]:
        """Gibt alle Bank-Buttons zurück."""
        return self._bank_buttons

    @property
    def stem_indicators(self) -> dict[int, tk.Label]:
        """Gibt alle Stem-Indikatoren zurück."""
        return self._stem_indicators
