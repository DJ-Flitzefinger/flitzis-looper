"""
Stem Player Engine for flitzis_looper.
Handles stem player initialization, synchronization, and gain control.

ARCHITECTURE:
- A shared Phasor serves as the master clock for ALL players
- ALL players (Stems + Main) use Pointer with this Phasor as index
- All outputs are collected into a Mix
- The Mix goes through EQ → Button-Gain → Output
- This ensures EQ and gain controls work with stems too!
"""

import time
import tempfile
import os
import numpy as np
import soundfile as sf

from pyo import Phasor, Pointer, SigTo, Sig, Mix, EQ, SndTable

from flitzis_looper.utils.logging import logger
from flitzis_looper.core.state import (
    STEM_NAMES,
    get_button_data,
    ensure_stems_structure,
)
from flitzis_looper.audio.server import get_master_amp


# ============== STEM GAIN CONTROL ==============

def update_stem_gains(button_id):
    """
    Aktualisiert die Gain-Werte basierend auf den Stem-States.
    Verwendet SigTo für click-freie Übergänge (10ms Fade).
    """
    button_data = get_button_data()
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
    button_data = get_button_data()
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


# ============== HELPER FUNCTIONS ==============

def _build_stem_mix_and_eq(button_id, loop, all_players):
    """
    Gemeinsame Hilfsfunktion für Stem-Player:
    - Mischt alle Stem-Player
    - Wendet die EQ-Kette an
    - Verknüpft Button-Gain und Master-Gain
    - Startet den finalen Output
    - Aktualisiert den Initialisierungsstatus und die Gains
    """
    button_data = get_button_data()
    data = button_data[button_id]
    master_amp = get_master_amp()
    
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


def _create_main_table_from_audio(button_id, loop, main_audio):
    """
    Hilfsfunktion für die Erstellung der Main-Table aus einem Audio-Array.
    Schreibt das Ergebnis nach data["stems"]["main_table"].
    """
    button_data = get_button_data()
    data = button_data[button_id]
    temp_file = tempfile.NamedTemporaryFile(suffix='.wav', delete=False)
    temp_path = temp_file.name
    temp_file.close()
    sf.write(temp_path, main_audio, loop._audio_sr)
    data["stems"]["main_table"] = SndTable(temp_path)
    try:
        os.unlink(temp_path)
    except:
        pass


def _create_stem_table_from_audio(button_id, stem, audio_data, loop):
    """
    Hilfsfunktion für die Erstellung einer Stem-Table aus einem Audio-Array.
    Schreibt das Ergebnis nach data["stems"]["tables"][stem].
    """
    button_data = get_button_data()
    data = button_data[button_id]
    temp_file = tempfile.NamedTemporaryFile(suffix='.wav', delete=False)
    temp_path = temp_file.name
    temp_file.close()
    sf.write(temp_path, audio_data, loop._audio_sr)
    data["stems"]["tables"][stem] = SndTable(temp_path)
    try:
        os.unlink(temp_path)
    except:
        pass


def _select_stem_audio(button_id, stem, use_key_lock, current_speed):
    """
    Wählt die zu verwendenden Stem-Audiodaten (dry vs. gepitcht) abhängig von Key Lock
    und aktueller Geschwindigkeit. Gibt None zurück, wenn kein Audio verfügbar ist.
    """
    # Import here to avoid circular imports
    from flitzis_looper.audio.pitch import _create_pitched_stem_cache
    
    button_data = get_button_data()
    data = button_data[button_id]

    dry_audio = data["stems"]["dry"].get(stem)
    if dry_audio is None:
        return None

    if use_key_lock and current_speed != 1.0:
        if data["stems"]["pitched"].get(stem) is None:
            _create_pitched_stem_cache(button_id, stem, current_speed)
        return data["stems"]["pitched"].get(stem)

    return dry_audio


def _select_main_audio_for_stems(loop, use_key_lock, current_speed, loop_start_time, loop_end_time):
    """
    Wählt das Haupt-Audiosegment für Stems, abhängig von Key Lock und Geschwindigkeit.
    Nutzt nach Möglichkeit den gepitchten Cache, fällt sonst auf den geschnittenen
    Loop-Ausschnitt zurück. Gibt None zurück, wenn kein Audio verfügbar ist.
    """
    import numpy as np
    
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
    return main_audio


# ============== MAIN INITIALIZATION ==============

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
    
    button_data = get_button_data()
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
    _cleanup_stem_players(button_id)

    # === TABLES ERSTELLEN (nur wenn nötig) ===
    if need_new_tables:
        # Main Table
        main_audio = _select_main_audio_for_stems(
            loop, use_key_lock, current_speed,
            loop_start_time, loop_end_time
        )
        if main_audio is not None:
            _create_main_table_from_audio(button_id, loop, main_audio)
        
        # Stem Tables
        for stem in STEM_NAMES:
            audio_to_use = _select_stem_audio(button_id, stem, use_key_lock, current_speed)
            if audio_to_use is None:
                continue
            _create_stem_table_from_audio(button_id, stem, audio_to_use, loop)
        
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
    _build_stem_mix_and_eq(button_id, loop, all_players)


def _initialize_stems_while_running(button_id):
    """
    Initialisiert Stems während der Loop bereits läuft.
    
    Das ist der Fall wenn der User Stems lädt während ein Loop spielt,
    und dann einen Stem-Button drückt.
    
    WICHTIG: Der Phasor wird mit einer geschätzten Phase gestartet,
    basierend auf der aktuellen Zeit. Das ist nicht 100% exakt, aber
    gut genug für den Use Case "Stems während des Spielens aktivieren".
    """
    
    button_data = get_button_data()
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
        main_audio = _select_main_audio_for_stems(
            loop, use_key_lock, current_speed,
            loop_start_time, loop_end_time
        )
        if main_audio is not None:
            _create_main_table_from_audio(button_id, loop, main_audio)
    
    # Stem Tables
    for stem in STEM_NAMES:
        if data["stems"]["tables"].get(stem) is not None:
            continue
        
        audio_to_use = _select_stem_audio(button_id, stem, use_key_lock, current_speed)
        if audio_to_use is None:
            continue
        _create_stem_table_from_audio(button_id, stem, audio_to_use, loop)
    
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
    _build_stem_mix_and_eq(button_id, loop, all_players)


# ============== CLEANUP FUNCTIONS ==============

def _cleanup_stem_players(button_id):
    """
    Hilfsfunktion: Räumt alte Stem-Player auf ohne Stems zu löschen.
    
    WICHTIG: Tables werden NICHT gelöscht! Das ermöglicht schnellen Retrigger,
    da nur neue Pointer/Phasor erstellt werden müssen, nicht die Tables.
    """
    button_data = get_button_data()
    data = button_data[button_id]
    
    # Output stoppen
    if data["stems"].get("final_output"):
        try:
            data["stems"]["final_output"].stop()
        except:
            pass
        data["stems"]["final_output"] = None
    
    # EQ stoppen
    for eq_name in ["eq_low", "eq_mid", "eq_high"]:
        if data["stems"].get(eq_name):
            try:
                data["stems"][eq_name].stop()
            except:
                pass
            data["stems"][eq_name] = None
    
    # Mix stoppen
    if data["stems"].get("stem_mix"):
        try:
            data["stems"]["stem_mix"].stop()
        except:
            pass
        data["stems"]["stem_mix"] = None
    
    # Master-Phasor stoppen
    if data["stems"].get("master_phasor"):
        try:
            data["stems"]["master_phasor"].stop()
        except:
            pass
        data["stems"]["master_phasor"] = None
    
    # Main Player stoppen (TABLE BLEIBT!)
    if data["stems"].get("main_player"):
        try:
            data["stems"]["main_player"].stop()
        except:
            pass
        data["stems"]["main_player"] = None
    # NICHT: data["stems"]["main_table"] = None  # Table behalten!
    
    # Main Gain stoppen
    if data["stems"].get("main_gain"):
        try:
            data["stems"]["main_gain"].stop()
        except:
            pass
        data["stems"]["main_gain"] = None
    
    # Stem Player und Gains stoppen (TABLES BLEIBEN!)
    for stem in STEM_NAMES:
        player = data["stems"]["players"].get(stem)
        if player:
            try:
                player.stop()
            except:
                pass
            data["stems"]["players"][stem] = None
        
        gain = data["stems"]["gains"].get(stem)
        if gain:
            try:
                gain.stop()
            except:
                pass
            data["stems"]["gains"][stem] = None
        
        # NICHT: data["stems"]["tables"][stem] = None  # Tables behalten!
        data["stems"]["outputs"][stem] = None
    
    data["stems"]["initialized"] = False
    
    data["stems"]["initialized"] = False


def stop_stem_players(button_id):
    """Stoppt alle Stem-Player und gibt Ressourcen frei."""
    button_data = get_button_data()
    data = button_data[button_id]
    
    # Stems-Struktur sicherstellen
    ensure_stems_structure(data)
    
    # Final Output stoppen (wichtig: zuerst!)
    if data["stems"].get("final_output"):
        try:
            data["stems"]["final_output"].stop()
        except:
            pass
        data["stems"]["final_output"] = None
    
    # EQ-Kette stoppen
    for eq_name in ["eq_low", "eq_mid", "eq_high"]:
        if data["stems"].get(eq_name):
            try:
                data["stems"][eq_name].stop()
            except:
                pass
            data["stems"][eq_name] = None
    
    # Mix stoppen
    if data["stems"].get("stem_mix"):
        try:
            data["stems"]["stem_mix"].stop()
        except:
            pass
        data["stems"]["stem_mix"] = None
    
    # Master-Phasor stoppen
    if data["stems"].get("master_phasor"):
        try:
            data["stems"]["master_phasor"].stop()
        except:
            pass
        data["stems"]["master_phasor"] = None
    
    # Main Player stoppen (synchroner Original-Loop Player)
    if data["stems"].get("main_player"):
        try:
            data["stems"]["main_player"].stop()
        except:
            pass
        data["stems"]["main_player"] = None
    data["stems"]["main_table"] = None
    
    for stem in STEM_NAMES:
        player = data["stems"]["players"].get(stem)
        if player:
            try:
                player.stop()
            except:
                pass
            data["stems"]["players"][stem] = None
        
        gain = data["stems"]["gains"].get(stem)
        if gain:
            try:
                gain.stop()
            except:
                pass
            data["stems"]["gains"][stem] = None
        
        data["stems"]["tables"][stem] = None
        data["stems"]["outputs"][stem] = None
    
    # PyoLoop stem_mute zurücksetzen
    loop = data.get("pyo")
    if loop:
        loop._stem_mute = None
    
    # Main-Gain stoppen
    if data["stems"].get("main_gain"):
        try:
            data["stems"]["main_gain"].stop()
        except:
            pass
    data["stems"]["main_gain"] = None
    data["stems"]["initialized"] = False


# ============== ACTIVATION HELPERS ==============

def _activate_main_loop(button_id):
    """Aktiviert den Haupt-Loop durch Setzen des Main-Gains auf 1."""
    button_data = get_button_data()
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
    button_data = get_button_data()
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
    button_data = get_button_data()
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


def apply_stem_mix(button_id):
    """
    Wendet den aktuellen Stem-Mix an.
    Initialisiert Stem-Player falls nötig und aktualisiert Gains.
    """
    button_data = get_button_data()
    data = button_data[button_id]
    loop = data.get("pyo")
    
    if not loop or not data["stems"]["available"]:
        return
    
    # Stem-Player initialisieren falls noch nicht geschehen
    if not data["stems"]["initialized"]:
        initialize_stem_players(button_id)
    
    # Gains aktualisieren
    update_stem_gains(button_id)
