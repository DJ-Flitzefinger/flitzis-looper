"""Stem control for flitzis_looper.

Handles stem toggle, momentary activation, and stop-stem functionality.
"""

import logging

from flitzis_looper.core.state import STEM_NAMES, get_button_data, get_selected_stems_button

logger = logging.getLogger(__name__)


def get_selected_or_active_stems_button():
    """Zentraler Helper für Stem-bezogene Aktionen.

    Bevorzugt den explizit selektierten Button (selected_stems_button),
    fällt sonst auf den ersten aktiven Loop mit verfügbaren Stems zurück.
    Gibt None zurück, wenn kein solcher Button existiert.
    """
    selected_stems_button = get_selected_stems_button()

    target_button_id = selected_stems_button
    if target_button_id is not None:
        return target_button_id
    return get_active_loop_with_stems()


def get_active_loop_with_stems():
    """Gibt den aktiven Loop mit verfügbaren Stems zurück, oder None."""
    button_data = get_button_data()

    for btn_id, data in button_data.items():
        if data["active"] and data["stems"]["available"]:
            return btn_id
    return None


def on_stem_toggle(stem, callbacks):
    """Handler für Stem-Toggle (Linksklick).

    Vocals, Melody, Bass, Drums sind frei kombinierbar.
    Instrumental hat Sonderrolle (exklusiv).

    MULTI-LOOP SUPPORT: Verwendet selected_stems_button statt aktiven Loop,
    damit bei Multi-Loop jeder Track individuell gesteuert werden kann.

    Args:
        stem: Name des Stems ("vocals", "melody", "bass", "drums", "instrumental")
        callbacks: Dict mit Callbacks:
            - update_stem_gains: Callback zum Aktualisieren der Stem-Gains
            - update_stem_buttons_state: Callback zum Aktualisieren der Stem-Buttons
            - save_config_async: Callback zum asynchronen Speichern der Config
    """
    button_data = get_button_data()

    target_button_id = get_selected_or_active_stems_button()
    if target_button_id is None:
        return

    data = button_data[target_button_id]

    # Prüfe ob Stems verfügbar
    if not data["stems"]["available"]:
        return

    states = data["stems"]["states"]

    # Prüfe ob Stem-Player initialisiert sind (nur wenn Loop aktiv)
    if data["active"] and not data["stems"]["initialized"]:
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

    # Gains aktualisieren wenn Loop läuft
    if data["active"] and data["stems"]["initialized"]:
        callbacks["update_stem_gains"](target_button_id)

    callbacks["update_stem_buttons_state"]()
    callbacks["save_config_async"]()


def on_stem_momentary_activate(stem, activate, callbacks):
    """Handler für temporäres Aktivieren/Deaktivieren eines Stems.

    Wird bei Rechtsklick (activate=True) / Mittelklick (activate=False) verwendet.

    MULTI-LOOP SUPPORT: Verwendet selected_stems_button.

    Args:
        stem: Name des Stems
        activate: True zum Aktivieren, False zum Deaktivieren
        callbacks: Dict mit Callbacks (optional)
    """
    button_data = get_button_data()
    selected_stems_button = get_selected_stems_button()

    # Verwende selected_stems_button wenn gesetzt
    target_button_id = selected_stems_button

    if target_button_id is None:
        # Fallback: Finde aktiven Loop mit Stems
        target_button_id = get_active_loop_with_stems()

    if target_button_id is None:
        return

    data = button_data[target_button_id]

    # Prüfe ob Loop aktiv und Stem-Player initialisiert sind
    if not data["active"] or not data["stems"]["initialized"]:
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
            if not any_state_active and data["stems"]["main_gain"]:
                data["stems"]["main_gain"].value = 1.0


def on_stem_momentary_release(stem, callbacks):
    """Handler für Loslassen der temporären Aktivierung.

    Stellt den ursprünglichen State wieder her.

    Args:
        stem: Name des Stems
        callbacks: Dict mit Callbacks:
            - update_stem_gains: Callback zum Aktualisieren der Stem-Gains
    """
    button_data = get_button_data()
    selected_stems_button = get_selected_stems_button()

    # Verwende selected_stems_button wenn gesetzt
    target_button_id = selected_stems_button

    if target_button_id is None:
        # Fallback: Finde aktiven Loop mit Stems
        target_button_id = get_active_loop_with_stems()

    if target_button_id is None:
        return

    data = button_data[target_button_id]

    # Nur wenn Loop aktiv
    if not data["active"]:
        return

    # Gains basierend auf gespeichertem State wiederherstellen
    callbacks["update_stem_gains"](target_button_id)


def on_stop_stem_toggle(callbacks):
    """Handler für Stop-Stem Button (Linksklick).

    Speichert aktuelle Stem-States und schaltet alle aus (Original spielt).
    Bei erneutem Klick werden die gespeicherten States wiederhergestellt.

    Args:
        callbacks: Dict mit Callbacks:
            - initialize_stem_players: Callback zum Initialisieren der Stem-Player
            - update_stem_gains: Callback zum Aktualisieren der Stem-Gains
            - update_stem_buttons_state: Callback zum Aktualisieren der Stem-Buttons
    """
    button_data = get_button_data()
    selected_stems_button = get_selected_stems_button()

    # Verwende selected_stems_button wenn gesetzt
    target_button_id = selected_stems_button

    if target_button_id is None:
        # Fallback: Finde aktiven Loop mit Stems
        target_button_id = get_active_loop_with_stems()

    if target_button_id is None:
        return

    data = button_data[target_button_id]

    # Prüfe ob Stems verfügbar
    if not data["stems"]["available"]:
        return

    # Stem-Player initialisieren falls Loop aktiv aber nicht initialisiert
    if data["active"] and not data["stems"]["initialized"]:
        callbacks["initialize_stem_players"](target_button_id)

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

    # Gains aktualisieren wenn Loop aktiv
    if data["active"] and data["stems"]["initialized"]:
        callbacks["update_stem_gains"](target_button_id)
    callbacks["update_stem_buttons_state"]()


def on_stop_stem_momentary(activate, callbacks):
    """Handler für temporäres Aktivieren des Stop-Stem (Rechtsklick gedrückt).

    Schaltet alle Stems temporär aus (Original spielt).

    Args:
        activate: True zum Aktivieren, False zum Wiederherstellen
        callbacks: Dict mit Callbacks:
            - initialize_stem_players: Callback zum Initialisieren der Stem-Player
            - update_stem_gains: Callback zum Aktualisieren der Stem-Gains
    """
    button_data = get_button_data()
    selected_stems_button = get_selected_stems_button()

    # Verwende selected_stems_button wenn gesetzt
    target_button_id = selected_stems_button

    if target_button_id is None:
        # Fallback: Finde aktiven Loop mit Stems
        target_button_id = get_active_loop_with_stems()

    if target_button_id is None:
        return

    data = button_data[target_button_id]

    # Nur wenn Loop aktiv
    if not data["active"]:
        return

    # Prüfe ob Stem-Player initialisiert sind
    if not data["stems"]["initialized"]:
        callbacks["initialize_stem_players"](target_button_id)

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
        callbacks["update_stem_gains"](target_button_id)


def on_stop_stem_momentary_release(callbacks):
    """Handler für Loslassen des Stop-Stem Buttons.

    Stellt die ursprünglichen Stem-States wieder her.

    Args:
        callbacks: Dict mit Callbacks:
            - update_stem_gains: Callback zum Aktualisieren der Stem-Gains
            - update_stem_buttons_state: Callback zum Aktualisieren der Stem-Buttons
    """
    button_data = get_button_data()
    selected_stems_button = get_selected_stems_button()

    # Verwende selected_stems_button wenn gesetzt
    target_button_id = selected_stems_button

    if target_button_id is None:
        # Fallback: Finde aktiven Loop mit Stems
        target_button_id = get_active_loop_with_stems()

    if target_button_id is None:
        return

    data = button_data[target_button_id]

    # Nur wenn Loop aktiv
    if not data["active"]:
        return

    # Gains basierend auf gespeichertem State wiederherstellen
    callbacks["update_stem_gains"](target_button_id)
    callbacks["update_stem_buttons_state"]()
