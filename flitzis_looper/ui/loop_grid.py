"""flitzis_looper.ui.loop_grid - Button-Grid für Loop-Buttons.

Enthält:
- create_button_grid(): Erstellt das 6x6 Button-Grid mit Stem-Indikatoren
- create_bank_buttons(): Erstellt die Bank-Wechsel-Buttons
- open_context_menu(): Rechtsklick-Menü für Loop-Buttons
- update_stem_indicator(): Aktualisiert Stem-Indikator-Farben
- update_all_stem_indicators(): Aktualisiert alle Indikatoren
"""

import tkinter as tk

from flitzis_looper.core.state import (
    COLOR_BANK_ACTIVE,
    COLOR_BANK_BTN,
    COLOR_BG,
    COLOR_BTN_INACTIVE,
    COLOR_STEM_AVAILABLE,
    COLOR_STEM_GENERATING,
    COLOR_STEM_INACTIVE,
    COLOR_STEM_SELECTED,
    COLOR_TEXT,
    COLOR_TEXT_ACTIVE,
    GRID_SIZE,
    NUM_BANKS,
    get_bank_buttons,
    get_button_data,
    get_buttons,
    get_root,
    get_selected_stems_button,
    get_stem_indicators,
    register_bank_button,
    register_button,
    register_stem_indicator,
    set_selected_stems_button,
)


def create_button_grid(
    trigger_loop_callback,
    stop_loop_callback,
    open_context_menu_callback,
    select_stems_button_callback,
):
    """Erstellt das Button-Grid mit Stem-Indikatoren.

    Args:
        trigger_loop_callback: Callback für Linksklick (trigger)
        stop_loop_callback: Callback für Rechtsklick (stop)
        open_context_menu_callback: Callback für Mittelklick (context menu)
        select_stems_button_callback: Callback für Klick auf Stem-Indikator
    """
    root = get_root()
    buttons = get_buttons()
    stem_indicators = get_stem_indicators()

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
                highlightthickness=0,
            )
            btn.pack(fill="both", expand=True)
            btn.bind("<ButtonPress-1>", lambda e, b=btn_id: trigger_loop_callback(b))
            btn.bind("<Button-2>", lambda e, b=btn_id: open_context_menu_callback(b, e))
            btn.bind("<ButtonPress-3>", lambda e, b=btn_id: stop_loop_callback(b))
            buttons[btn_id] = btn
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
                cursor="hand2",
            )
            # Platziere unten rechts über dem Button
            stem_indicator.place(relx=1.0, rely=1.0, anchor="se", x=-3, y=-3)
            stem_indicator.bind("<Button-1>", lambda e, b=btn_id: select_stems_button_callback(b))
            stem_indicators[btn_id] = stem_indicator
            register_stem_indicator(btn_id, stem_indicator)


def create_bank_buttons(switch_bank_callback):
    """Erstellt die Bank-Wechsel-Buttons.

    Args:
        switch_bank_callback: Callback für Bank-Wechsel
    """
    root = get_root()
    bank_buttons = get_bank_buttons()

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
            command=lambda b=bank_id: switch_bank_callback(b),
        )
        btn.pack(side="left", padx=5, expand=True, fill="x")
        bank_buttons[bank_id] = btn
        register_bank_button(bank_id, btn)


def update_stem_indicator(button_id):
    """Aktualisiert den Stem-Indikator (kleines S-Quadrat) für einen Button.

    - Ausgegraut wenn keine Stems
    - Orange während Generierung
    - Rot wenn Stems verfügbar
    - Heller Rot wenn selektiert für Stem-Kontrolle.
    """
    button_data = get_button_data()
    stem_indicators = get_stem_indicators()
    selected_stems_button = get_selected_stems_button()

    if button_id not in stem_indicators:
        return

    indicator = stem_indicators[button_id]
    data = button_data.get(button_id, {})

    is_generating = data.get("stems", {}).get("generating", False)
    is_available = data.get("stems", {}).get("available", False)
    is_selected = selected_stems_button == button_id

    if is_generating:
        # Orange während Generierung
        indicator.config(bg=COLOR_STEM_GENERATING, fg=COLOR_TEXT)
    elif is_available:
        if is_selected:
            # Heller Rot wenn selektiert
            indicator.config(bg=COLOR_STEM_SELECTED, fg=COLOR_TEXT)
        else:
            # Rot wenn Stems verfügbar
            indicator.config(bg=COLOR_STEM_AVAILABLE, fg=COLOR_TEXT)
    else:
        # Ausgegraut wenn keine Stems
        indicator.config(bg=COLOR_STEM_INACTIVE, fg="#888888")


def update_all_stem_indicators():
    """Aktualisiert alle Stem-Indikatoren in der aktuellen Bank."""
    buttons = get_buttons()
    for btn_id in buttons:
        update_stem_indicator(btn_id)


def select_stems_button(button_id, update_stem_buttons_state_callback=None):
    """Selektiert einen Button für die Stem-Kontrolle.

    Bei MULTI LOOP kann man damit zwischen verschiedenen Tracks wechseln.

    Args:
        button_id: ID des zu selektierenden Buttons
        update_stem_buttons_state_callback: Callback für Stem-Button-State-Update
    """
    button_data = get_button_data()
    selected_stems_button = get_selected_stems_button()

    data = button_data.get(button_id)
    if not data:
        return

    # Nur selektieren wenn Stems verfügbar
    if not data.get("stems", {}).get("available"):
        return

    # Toggle: Wenn bereits selektiert, deselektieren
    if selected_stems_button == button_id:
        set_selected_stems_button(None)
    else:
        set_selected_stems_button(button_id)

    # Alle Indikatoren updaten
    update_all_stem_indicators()

    # Stem-Buttons updaten (zeigen jetzt States des selektierten Buttons)
    if update_stem_buttons_state_callback:
        update_stem_buttons_state_callback()
