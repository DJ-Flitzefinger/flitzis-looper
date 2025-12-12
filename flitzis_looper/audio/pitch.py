"""
Pitch Shifting and Caching for flitzis_looper stems.
Handles pitch correction for stems when Key Lock is enabled.

Uses pedalboard for high-quality pitch shifting.
"""

import numpy as np
from pedalboard import PitchShift

from flitzis_looper.utils.logging import logger
from flitzis_looper.utils.math import speed_to_semitones
from flitzis_looper.core.state import (
    STEM_NAMES,
    get_button_data,
    get_all_banks_data,
    get_loaded_loops,
)


def _create_pitched_stem_cache(button_id, stem, speed):
    """
    Erstellt den Pitch-Cache für einen Stem.
    
    Uses pedalboard PitchShift to correct the pitch when speed changes,
    so the audio stays in tune even at different playback speeds.
    
    Args:
        button_id: The button ID 
        stem: The stem name (vocals, melody, bass, drums, instrumental)
        speed: The current playback speed
    """
    
    button_data = get_button_data()
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
    
    This allows instant switching between stems without audible artifacts
    when Key Lock is active.
    
    Args:
        button_id: The button ID to precache stems for
    """
    button_data = get_button_data()
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
    """
    Invalidiert alle Stem-Caches bei Speed-Änderung.
    
    Called when the playback speed changes and the cached pitched stems
    are no longer valid for the new speed.
    
    Args:
        button_id: The button ID to invalidate caches for
    """
    button_data = get_button_data()
    data = button_data[button_id]
    data["stems"]["cached_speed"] = None
    for stem in STEM_NAMES:
        data["stems"]["pitched"][stem] = None


def cleanup_stem_caches():
    """
    Gibt alle Stem-Caches frei beim Beenden.
    
    Called during application shutdown to free all memory used by
    stem pitch caches across all loaded loops.
    """
    try:
        loaded_loops = get_loaded_loops()
        all_banks_data = get_all_banks_data()
        
        for (bank_id, btn_id), loop in loaded_loops.items():
            data = all_banks_data[bank_id][btn_id]
            # Stoppe Stem-Player
            for stem in STEM_NAMES:
                player = data["stems"]["players"].get(stem)
                if player:
                    try:
                        player.stop()
                    except:
                        pass
            # Caches löschen
            data["stems"]["pitched"] = {s: None for s in STEM_NAMES}
            data["stems"]["players"] = {s: None for s in STEM_NAMES}
            data["stems"]["tables"] = {s: None for s in STEM_NAMES}
    except Exception as e:
        logger.debug(f"Error cleaning up stem caches: {e}")
