"""Bank and button management for flitzis_looper.
Handles bank switching, button label updates, and stem indicators.
"""

import logging
import os

from flitzis_looper.core.state import (
    COLOR_BANK_ACTIVE,
    COLOR_BANK_BTN,
    COLOR_BTN_ACTIVE,
    COLOR_BTN_INACTIVE,
    COLOR_STEM_AVAILABLE,
    COLOR_STEM_GENERATING,
    COLOR_STEM_INACTIVE,
    COLOR_STEM_SELECTED,
    COLOR_TEXT,
    COLOR_TEXT_ACTIVE,
    get_bank_buttons,
    get_button_data,
    get_buttons,
    get_current_bank,
    get_selected_stems_button,
    get_stem_indicators,
    set_button_data_ref,
    set_selected_stems_button,
)

logger = logging.getLogger(__name__)


def switch_bank(new_bank_id, update_stem_buttons_state_callback=None):
    """Wechselt zur angegebenen Bank.

    Args:
        new_bank_id: ID der neuen Bank (1-6)
        update_stem_buttons_state_callback: Callback zum Aktualisieren der Stem-Button-States
    """
    current_bank = get_current_bank()

    if current_bank.get() == new_bank_id:
        return

    current_bank.set(new_bank_id)
    set_button_data_ref(new_bank_id)

    # Selected stems button zurücksetzen bei Bank-Wechsel
    set_selected_stems_button(None)

    update_all_button_labels()
    update_bank_button_colors()
    update_button_colors()
    update_all_stem_indicators()

    if update_stem_buttons_state_callback:
        update_stem_buttons_state_callback()


def update_bank_button_colors():
    """Aktualisiert die Farben aller Bank-Buttons."""
    bank_buttons = get_bank_buttons()
    current_bank = get_current_bank()

    for bank_id, btn in bank_buttons.items():
        if bank_id == current_bank.get():
            btn.config(bg=COLOR_BANK_ACTIVE, fg=COLOR_TEXT_ACTIVE)
        else:
            btn.config(bg=COLOR_BANK_BTN, fg=COLOR_TEXT)


def update_button_colors():
    """Aktualisiert alle Button-Farben basierend auf ihrem aktiven Status."""
    buttons = get_buttons()
    button_data = get_button_data()

    for btn_id, btn in buttons.items():
        data = button_data[btn_id]
        if data["active"] and data["file"]:
            btn.config(bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE)
        else:
            btn.config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)


def update_all_button_labels():
    """Aktualisiert die Beschriftungen aller Buttons in der aktuellen Bank."""
    buttons = get_buttons()
    for btn_id in buttons:
        update_button_label(btn_id)


def update_button_label(button_id):
    """Aktualisiert die Beschriftung eines einzelnen Buttons.

    Args:
        button_id: ID des zu aktualisierenden Buttons
    """
    button_data = get_button_data()
    buttons = get_buttons()

    data = button_data[button_id]
    if not data["file"]:
        buttons[button_id].config(text=f"{button_id}")
        buttons[button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
        # Stem-Indikator auch updaten
        update_stem_indicator(button_id)
        return

    full_filename = os.path.basename(data["file"])
    line1 = full_filename[:16]
    line2 = full_filename[16:32] if len(full_filename) > 16 else ""
    bpm = data["bpm"]
    bpm_text = f"{bpm:.1f} BPM" if bpm else "BPM ?"

    # Kein Stems-Indikator mehr im Label - wird jetzt durch S-Quadrat angezeigt
    label = f"{line1}\n{line2}\n{bpm_text}"
    buttons[button_id].config(text=label)

    if data["active"]:
        buttons[button_id].config(bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE)
    else:
        buttons[button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)

    # Stem-Indikator updaten
    update_stem_indicator(button_id)


def update_stem_indicator(button_id):
    """Aktualisiert den Stem-Indikator (kleines S-Quadrat) für einen Button.
    - Ausgegraut wenn keine Stems
    - Orange während Generierung
    - Rot wenn Stems verfügbar
    - Heller Rot wenn selektiert für Stem-Kontrolle.
    """
    stem_indicators = get_stem_indicators()
    button_data = get_button_data()
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
        update_stem_buttons_state_callback: Callback zum Aktualisieren der Stem-Button-States
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
