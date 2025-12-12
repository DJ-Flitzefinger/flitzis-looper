"""PyoLoop - Audio loop player with Key Lock and EQ support.

Uses Pedalboard/Rubberband for high-quality pitch shifting.

OPTIMIZATION FOR KEY LOCK (RAM-based):
- Pitched audio is cached completely in RAM as numpy array
- SndTable is created directly from RAM data (via tempfile in RAM)
- Cache is preserved as long as Speed doesn't change
- Pre-caching via right-click on stopped loops possible
- On retrigger only the player is recreated, no repeated pitch-shifting
"""

import builtins
import contextlib
import os
import tempfile

import numpy as np
import soundfile as sf
from pedalboard import PitchShift
from pyo import EQ, Follower, Looper, Osc, Pointer, SfPlayer, Sig, SndTable

from flitzis_looper.core.state import STEM_NAMES, get_button_data, get_master_amp
from flitzis_looper.utils.logging import logger
from flitzis_looper.utils.math import db_to_amp, speed_to_semitones
from flitzis_looper.utils.threading import io_executor, schedule_gui_update


class PyoLoop:
    """Audio-Loop-Player mit Key Lock und EQ-Unterstützung.
    Verwendet Pedalboard/Rubberband für hochwertiges Pitch-Shifting.

    OPTIMIERUNG FÜR KEY LOCK (RAM-basiert):
    - Gepitchtes Audio wird komplett im RAM gecacht als numpy array
    - SndTable wird direkt aus RAM-Daten erstellt (via tempfile im RAM)
    - Cache bleibt erhalten solange Speed sich nicht ändert
    - Pre-Caching via Rechtsklick auf gestoppte Loops möglich
    - Bei Retrigger wird nur der Player neu erstellt, kein erneutes Pitch-Shifting
    """

    def __init__(self):
        self.path = None
        self.player = None
        self.table = None
        self.amp = None
        self.speed = None
        self.follower = None
        self.loop_start = 0.0
        self.loop_end = None
        self._is_playing = False
        self._duration = 0.0
        self._sample_rate = 44100
        self._is_loaded = False
        self._loading = False
        self._pyo_initialized = False
        self._pending_gain = 0.0
        self._pending_speed = 1.0
        self._use_table = False
        self._loop_base_freq = 1.0

        # Key Lock
        self._key_lock = False
        self._current_pitch_shift_semitones = 0.0

        # Pitch-shifted Audio Cache (RAM-basiert)
        self._pitched_table = None  # SndTable mit pitch-shifted Audio
        self._pitched_player = None  # Player für pitch-shifted Audio
        self._cached_speed = None  # Für welche Speed wurde gecached?
        self._pitched_audio_cache = None  # numpy array mit gepitchtem Stereo-Audio im RAM
        self._pitched_base_freq = 1.0  # Base frequency für pitched player

        # EQ
        self.eq_low = None
        self.eq_mid = None
        self.eq_high = None
        self.output = None

        # Stem-Mute: Externer Multiplikator (0=stumm wenn Stems aktiv, 1=normal)
        self._stem_mute = None  # Wird von außen gesetzt (SigTo für smooth fading)
        self._final_output = None  # Output nach Stem-Mute Multiplikation

        # EQ values
        self._eq_low_val = 0.0
        self._eq_mid_val = 0.0
        self._eq_high_val = 0.0

        # Original Audio Data (für Pitch-Shifting)
        self._audio_data = None
        self._audio_sr = None

        # Intro Mode (neues Konzept: ein Player für alles)
        self._intro_start = None  # Wenn gesetzt, startet Wiedergabe hier statt bei loop_start

    def _init_pyo_objects(self):
        if not self._pyo_initialized:
            self.amp = Sig(db_to_amp(float(self._pending_gain)))
            self.speed = Sig(float(self._pending_speed))
            self._pyo_initialized = True

    def load(self, path, loop_start=0.0, loop_end=None):
        """Synchrones Laden."""
        try:
            self.stop()
            if not os.path.exists(path):
                return False
            self.path = path
            self.loop_start = float(loop_start)
            self.loop_end = float(loop_end) if loop_end is not None else None
            info = sf.info(path)
            self._sample_rate = int(info.samplerate)
            self._duration = float(info.duration)
            if self.loop_end is None:
                self.loop_end = float(self._duration)

            # Audio-Daten für späteres Pitch-Shifting laden
            self._audio_data, self._audio_sr = sf.read(path)

            self._is_loaded = True
            return True
        except Exception as e:
            logger.error(f"Error loading: {e}")
            return False

    def load_async(self, path, loop_start=0.0, loop_end=None, callback=None):
        """Asynchrones Laden."""
        if self._loading:
            return
        self._loading = True

        def do_load():
            try:
                self.stop()
                if not os.path.exists(path):
                    if callback:
                        schedule_gui_update(lambda: callback(False))
                    self._loading = False
                    return
                self.path = path
                self.loop_start = float(loop_start)
                self.loop_end = float(loop_end) if loop_end is not None else None
                info = sf.info(path)
                sample_rate = int(info.samplerate)
                duration = float(info.duration)

                # Audio-Daten laden
                audio_data, audio_sr = sf.read(path)

                def finish_load():
                    self._sample_rate = sample_rate
                    self._duration = duration
                    self._audio_data = audio_data
                    self._audio_sr = audio_sr
                    if self.loop_end is None:
                        self.loop_end = float(self._duration)
                    self._is_loaded = True
                    self._loading = False
                    if callback:
                        callback(True)

                schedule_gui_update(finish_load)
            except Exception as e:
                logger.debug(f"Async load failed: {e}")
                self._loading = False
                if callback:
                    schedule_gui_update(lambda: callback(False))

        io_executor.submit(do_load)

    def _ensure_player(self):
        """Erstellt den Player wenn nötig."""
        if self.player is not None:
            return True
        if not self.path or not os.path.exists(self.path):
            return False
        try:
            self._init_pyo_objects()
            self._create_player()
            return self.player is not None
        except Exception as e:
            logger.error(f"Error ensuring player: {e}")
            return False

    def _create_player(self):
        """Erstellt den Audio-Player.

        NEUES KONZEPT FÜR INTRO:
        - Ohne Intro: SndTable (loop_start bis loop_end) + Osc (loopt sofort)
        - Mit Intro: SndTable (intro_start bis loop_end) + Looper mit startfromloop=False
          -> Looper spielt erst Intro durch, dann loopt er den Loop-Bereich
        """
        try:
            self._stop_all_objects()
            if not self.path:
                return
            self._init_pyo_objects()

            master_amp = get_master_amp()

            duration = float(self._duration)
            loop_start = float(self.loop_start)
            loop_end = float(self.loop_end) if self.loop_end else duration
            intro_start = float(self._intro_start) if self._intro_start is not None else None

            # Hat Intro UND Intro liegt vor Loop-Start?
            has_intro = intro_start is not None and intro_start < loop_start

            is_full_track = loop_start < 0.01 and abs(loop_end - duration) < 0.01 and not has_intro

            if is_full_track:
                # Ganzer Track ohne Intro -> einfacher SfPlayer
                self._use_table = False
                self.player = SfPlayer(
                    self.path, loop=True, speed=self.speed, mul=self.amp * master_amp
                )
            elif has_intro:
                # MIT INTRO: Looper-basierter Ansatz
                # Table geht von intro_start bis loop_end
                self._use_table = True
                self.table = SndTable(self.path, start=intro_start, stop=loop_end)
                table_dur = self.table.getDur()

                # Loop-Start relativ zur Table (nicht zum Original-File)
                loop_start_in_table = loop_start - intro_start
                loop_dur = loop_end - loop_start

                # Looper mit startfromloop=False:
                # - Startet am Anfang der Table (= intro_start)
                # - Spielt bis zum Loop-Ende durch
                # - Springt dann zu 'start' und loopt 'dur' Sekunden
                # WICHTIG: Looper erlaubt keine negativen pitch-Werte!
                self.player = Looper(
                    table=self.table,
                    pitch=abs(
                        self._pending_speed
                    ),  # Looper verwendet 'pitch', keine negativen Werte!
                    start=loop_start_in_table,  # Loop-Start relativ zur Table
                    dur=loop_dur,  # Loop-Dauer
                    xfade=0,  # Kein Crossfade für harte Loop-Punkte
                    mode=1,  # 1 = forward loop
                    startfromloop=False,  # WICHTIG: Startet von Table-Anfang (Intro!)
                    interp=4,  # Cubic interpolation
                    mul=self.amp * master_amp,
                )
                self._loop_base_freq = 1.0  # Nicht verwendet bei Looper
            else:
                # OHNE INTRO: Osc-basierter Ansatz wie bisher
                self._use_table = True
                self.table = SndTable(self.path, start=loop_start, stop=loop_end)
                table_dur = self.table.getDur()
                base_freq = 1.0 / table_dur if table_dur > 0 else 1.0
                self.player = Osc(
                    table=self.table,
                    freq=base_freq * self.speed,
                    phase=0,
                    interp=4,
                    mul=self.amp * master_amp,
                )
                self._loop_base_freq = base_freq

            # Bei aktivem Key Lock: Pitched Player erstellen
            if self._key_lock:
                self._create_pitched_player()

            self._create_eq_chain()
        except Exception as e:
            logger.error(f"Error creating player: {e}")

    def _create_pitched_player(self):
        """Erstellt einen Player mit pitch-korrigiertem Audio.
        Verwendet Pedalboard/Rubberband für hohe Qualität.

        OPTIMIERUNG (RAM-basiert):
        - Gepitchtes Audio wird im RAM gecacht (_pitched_audio_cache)
        - Bei gleicher Speed wird der Cache wiederverwendet (kein erneutes Pitch-Shifting!)
        - SndTable wird aus RAM-Daten via tempfile erstellt
        """
        try:
            if self._audio_data is None:
                return

            # Prüfe ob wir den Cache wiederverwenden können
            if self._cached_speed == self._pending_speed and self._pitched_audio_cache is not None:
                # Cache ist gültig - erstelle nur neuen Player mit gecachtem Audio
                self._create_pitched_player_from_cache()
                return

            # Cache ist ungültig oder nicht vorhanden - neu berechnen

            # Berechne benötigte Pitch-Korrektur
            semitones = speed_to_semitones(self._pending_speed)

            # Audio-Segment extrahieren (für Loop-Bereich)
            start_sample = int(self.loop_start * self._audio_sr)
            end_sample = (
                int(self.loop_end * self._audio_sr) if self.loop_end else len(self._audio_data)
            )

            audio_segment = self._audio_data[start_sample:end_sample]

            # Stelle sicher, dass es float32 ist
            if audio_segment.dtype != np.float32:
                audio_segment = audio_segment.astype(np.float32)

            # Pitch-Shift anwenden mit Pedalboard/Rubberband
            pitch_shifter = PitchShift(semitones=semitones)

            # Pedalboard erwartet (channels, samples) Format
            if len(audio_segment.shape) == 1:
                # Mono -> (1, samples)
                audio_segment = audio_segment.reshape(1, -1)
            else:
                # Stereo -> (2, samples) - transponieren von (samples, 2)
                audio_segment = audio_segment.T

            # Pitch-Shift anwenden
            pitched_audio = pitch_shifter(audio_segment, self._audio_sr)

            # Zurück zu (samples, channels) für pyo
            if pitched_audio.shape[0] <= 2:
                pitched_audio = pitched_audio.T

            # Im RAM cachen (numpy array) - BLEIBT ERHALTEN bis Speed sich ändert!
            self._pitched_audio_cache = pitched_audio.copy()
            self._cached_speed = self._pending_speed

            # Player aus Cache erstellen
            self._create_pitched_player_from_cache()

        except ImportError:
            logger.error("Pedalboard nicht installiert! Installiere mit: pip install pedalboard")
        except Exception as e:
            logger.error(f"Error creating pitched player: {e}")

    def _create_pitched_player_from_cache(self):
        """Erstellt den pitched player aus dem gecachten Audio im RAM.
        Diese Methode ist SCHNELL weil:
        1. Kein Pitch-Shifting nötig ist (Cache wird verwendet)
        2. Nur eine temporäre Datei geschrieben und SndTable geladen wird
        Unterstützt STEREO Audio!
        """
        try:
            if self._pitched_audio_cache is None:
                return

            master_amp = get_master_amp()

            # Alte pitched Objekte stoppen
            if self._pitched_player:
                with contextlib.suppress(builtins.BaseException):
                    self._pitched_player.stop()

            # Temporäre Datei aus RAM-Cache erstellen (für Stereo-Unterstützung)

            # Schreibe in BytesIO und dann in tempfile
            temp_file = tempfile.NamedTemporaryFile(suffix=".wav", delete=False)
            temp_path = temp_file.name
            temp_file.close()

            sf.write(temp_path, self._pitched_audio_cache, self._audio_sr)

            # SndTable laden (unterstützt Stereo!)
            self._pitched_table = SndTable(temp_path)

            # Temporäre Datei sofort löschen (Daten sind jetzt in SndTable)
            with contextlib.suppress(builtins.BaseException):
                os.unlink(temp_path)

            table_dur = self._pitched_table.getDur()
            base_freq = 1.0 / table_dur if table_dur > 0 else 1.0
            self._pitched_base_freq = base_freq

            # Neuen Player erstellen
            self._pitched_player = Osc(
                table=self._pitched_table,
                freq=base_freq * self._pending_speed,
                phase=0,
                interp=4,
                mul=self.amp * master_amp,
            )

        except Exception as e:
            logger.error(f"Error creating pitched player from cache: {e}")

    def _invalidate_pitch_cache(self):
        """Invalidiert den Pitch-Cache wenn sich relevante Parameter ändern."""
        self._pitched_audio_cache = None
        self._cached_speed = None

    def precache_pitched_audio(self):
        """Pre-cached das pitch-shifted Audio für schnelles Triggern.
        Wird aufgerufen wenn auf einen gestoppten Loop mit Rechtsklick geklickt wird.
        Nur aktiv wenn Key Lock aktiviert ist.

        WICHTIG: Diese Methode erstellt nur den Cache (numpy array),
        nicht den Player. Der Player wird erst bei play() erstellt.
        """
        if not self._key_lock:
            return False

        if self._audio_data is None:
            return False

        # Prüfe ob Cache bereits gültig ist
        if self._cached_speed == self._pending_speed and self._pitched_audio_cache is not None:
            return True  # Bereits gecacht

        # Cache erstellen (nur das Pitch-Shifting, kein Player!)
        try:
            # Berechne benötigte Pitch-Korrektur
            semitones = speed_to_semitones(self._pending_speed)

            # Audio-Segment extrahieren (für Loop-Bereich)
            start_sample = int(self.loop_start * self._audio_sr)
            end_sample = (
                int(self.loop_end * self._audio_sr) if self.loop_end else len(self._audio_data)
            )

            audio_segment = self._audio_data[start_sample:end_sample]

            # Stelle sicher, dass es float32 ist
            if audio_segment.dtype != np.float32:
                audio_segment = audio_segment.astype(np.float32)

            # Pitch-Shift anwenden mit Pedalboard/Rubberband
            pitch_shifter = PitchShift(semitones=semitones)

            # Pedalboard erwartet (channels, samples) Format
            if len(audio_segment.shape) == 1:
                audio_segment = audio_segment.reshape(1, -1)
            else:
                audio_segment = audio_segment.T

            # Pitch-Shift anwenden
            pitched_audio = pitch_shifter(audio_segment, self._audio_sr)

            # Zurück zu (samples, channels) für pyo
            if pitched_audio.shape[0] <= 2:
                pitched_audio = pitched_audio.T

            # Im RAM cachen
            self._pitched_audio_cache = pitched_audio.copy()
            self._cached_speed = self._pending_speed

            return True
        except Exception as e:
            logger.error(f"Error pre-caching pitched audio: {e}")
            return False

    def _stop_all_objects(self):
        """Stoppt alle pyo-Objekte."""
        objects_to_stop = [
            self.player,
            self._pitched_player,
            self.eq_low,
            self.eq_mid,
            self.eq_high,
            self.output,
        ]

        for obj in objects_to_stop:
            if obj:
                try:
                    obj.stop()
                except Exception:
                    pass  # Objekt bereits gestoppt oder ungültig

        if self.follower:
            try:
                self.follower.stop()
            except Exception:
                pass  # Follower bereits gestoppt
            self.follower = None

        self.player = None
        self._pitched_player = None
        self._pitched_table = None
        self.table = None
        self.eq_low = None
        self.eq_mid = None
        self.eq_high = None
        self.output = None

    def _create_eq_chain(self):
        """Erstellt die EQ-Kette mit optionalem Stem-Mute."""
        if not self.player:
            return

        # Wähle den richtigen Player basierend auf Key Lock Status
        signal = self._pitched_player if self._key_lock and self._pitched_player else self.player

        self.eq_low = EQ(
            signal, freq=200, q=0.7, boost=self._get_eq_boost(self._eq_low_val), type=1
        )
        self.eq_mid = EQ(
            self.eq_low, freq=1000, q=0.7, boost=self._get_eq_boost(self._eq_mid_val), type=0
        )
        self.eq_high = EQ(
            self.eq_mid, freq=4000, q=0.7, boost=self._get_eq_boost(self._eq_high_val), type=2
        )

        # Stem-Mute Multiplikator anwenden falls vorhanden
        if self._stem_mute is not None:
            self._final_output = self.eq_high * self._stem_mute
            self.output = self._final_output
        else:
            self.output = self.eq_high

    def _get_eq_boost(self, val):
        """Convert -1..1 to dB boost."""
        if val <= -0.98:
            return -80
        if val >= 0:
            return val * 12
        normalized = (val + 0.98) / 0.98
        if normalized <= 0:
            return -60
        return -60 * (1 - (normalized**0.4))

    def set_eq(self, low, mid, high):
        """Update EQ values."""
        self._eq_low_val = low
        self._eq_mid_val = mid
        self._eq_high_val = high

        if self.eq_low:
            self.eq_low.boost = self._get_eq_boost(low)
        if self.eq_mid:
            self.eq_mid.boost = self._get_eq_boost(mid)
        if self.eq_high:
            self.eq_high.boost = self._get_eq_boost(high)

    def set_key_lock(self, enabled):
        """Aktiviert oder deaktiviert Key Lock (Tonhöhenkorrektur bei Tempoänderung)."""
        was_playing = self._is_playing
        old_key_lock = self._key_lock
        self._key_lock = enabled

        if enabled != old_key_lock:
            if was_playing:
                # Neu starten mit korrektem Player
                self.stop()
                self._create_player()
                self.play()
            # WICHTIG: Cache NICHT invalidieren wenn Key Lock aktiviert wird!
            # Der Cache bleibt gültig solange sich die Speed nicht ändert

            # STEM-SYNC: Stems müssen neu initialisiert werden bei Key Lock Änderung
            self._update_stems_key_lock(enabled)

    def _update_stems_key_lock(self, key_lock_enabled):
        """Aktualisiert die Stems wenn Key Lock ein- oder ausgeschaltet wird.
        Bei Key Lock: Gepitchte Versionen verwenden
        Ohne Key Lock: Dry Versionen verwenden.

        NOTE: This method requires stop_stem_players, initialize_stem_players,
        and update_stem_gains to be available. These will be extracted to
        a separate module in Phase 4. For now, they are provided by the
        main application through function injection.
        """
        try:
            # Get button_data reference
            button_data = get_button_data()

            # Finde den button_id für diesen Loop
            button_id = None
            for btn_id, data in button_data.items():
                if data.get("pyo") is self:
                    button_id = btn_id
                    break

            if button_id is None:
                return

            data = button_data[button_id]

            # Nur wenn Stems verfügbar und initialisiert sind
            if not data["stems"].get("available") or not data["stems"].get("initialized"):
                return

            # Stems müssen neu initialisiert werden
            # These functions are from the main application, accessed via globals
            # They will be properly modularized in Phase 4
            import builtins

            # Check if functions are available in the global context
            stop_stem_players = getattr(builtins, "stop_stem_players", None)
            initialize_stem_players = getattr(builtins, "initialize_stem_players", None)
            update_stem_gains = getattr(builtins, "update_stem_gains", None)

            # Try to get from __main__ if not in builtins
            if stop_stem_players is None:
                import sys

                main_module = sys.modules.get("__main__")
                if main_module:
                    stop_stem_players = getattr(main_module, "stop_stem_players", None)
                    initialize_stem_players = getattr(main_module, "initialize_stem_players", None)
                    update_stem_gains = getattr(main_module, "update_stem_gains", None)

            if stop_stem_players:
                stop_stem_players(button_id)

            # Wenn Loop aktiv ist und Stems genutzt werden sollen
            if data.get("active") and any(data["stems"]["states"].values()):
                if initialize_stem_players:
                    initialize_stem_players(button_id)
                if update_stem_gains:
                    update_stem_gains(button_id)
        except Exception as e:
            logger.debug(f"Could not update stems key lock: {e}")

    def set_stem_mute(self, mute_signal):
        """Setzt den Stem-Mute Multiplikator.
        Wird verwendet um den Haupt-Loop stumm zu schalten wenn Stems aktiv sind.
        mute_signal: pyo Sig oder SigTo Objekt (0=stumm, 1=normal).
        """
        old_output = self.output
        self._stem_mute = mute_signal

        # EQ-Kette neu aufbauen falls Player existiert und läuft
        if self._is_playing and self.player:
            # Alten Output stoppen
            if old_output:
                with contextlib.suppress(builtins.BaseException):
                    old_output.stop()

            # EQ-Kette mit neuem stem_mute aufbauen
            self._create_eq_chain()

            # Neuen Output starten
            if self.output:
                self.output.out()

    def play(self):
        """Startet die Wiedergabe."""
        try:
            if not self._ensure_player():
                return
            self._is_playing = True
            if self.output:
                self.output.out()
            elif self._key_lock and self._pitched_player:
                self._pitched_player.out()
            elif self.player:
                self.player.out()
        except Exception as e:
            logger.error(f"Error playing: {e}")

    def play_with_intro(self, intro_start):
        """Spielt den Loop mit Intro ab.

        NEUES KONZEPT:
        - Setzt _intro_start, erstellt Player neu und startet
        - Der Looper kümmert sich automatisch um den Übergang zum Loop
        - Kein Timer, kein zweiter Player, keine Unterbrechung!
        """
        try:
            # intro_start zu native Python float konvertieren
            self._intro_start = float(intro_start)

            # Player neu erstellen (mit Intro-Konfiguration)
            self._create_player()

            # Starten
            self.play()

        except Exception as e:
            logger.error(f"Error playing with intro: {e}")

    def clear_intro(self):
        """Entfernt die Intro-Einstellung für den nächsten Play-Aufruf."""
        self._intro_start = None

    def stop(self):
        """Stoppt die Wiedergabe."""
        try:
            self._is_playing = False

            # Intro-Start zurücksetzen für nächsten Play-Aufruf
            self._intro_start = None

            # Normale Player stoppen
            if self.output:
                self.output.stop()
            if self.player:
                self.player.stop()
            if self._pitched_player:
                self._pitched_player.stop()
            if self.follower:
                self.follower.stop()
                self.follower = None

            self.player = None
            self._pitched_player = None
            self._pitched_table = None
            self.table = None
            self.output = None
        except Exception as e:
            logger.debug(f"Error during stop: {e}")

    def set_gain(self, db):
        """Setzt die Lautstärke in dB."""
        try:
            self._pending_gain = float(db)
            if self._pyo_initialized and self.amp:
                self.amp.value = db_to_amp(float(db))
        except (ValueError, AttributeError) as e:
            logger.debug(f"Error setting gain: {e}")

    def set_speed(self, value):
        """Setzt die Abspielgeschwindigkeit.
        Aktualisiert ALLE Player: PyoLoop, Stems, Master-Phasor.
        """
        try:
            old_speed = self._pending_speed
            self._pending_speed = float(value)

            if self._pyo_initialized and self.speed:
                self.speed.value = float(value)

            # Speed für den Player setzen - unterschiedlich je nach Player-Typ
            if self._use_table and self.player:
                if hasattr(self.player, "freq"):
                    # Osc-basierter Player
                    self.player.freq = self._loop_base_freq * float(value)
                elif hasattr(self.player, "pitch"):
                    # Looper-basierter Player
                    self.player.pitch = float(value)

            # Bei Key Lock: Pitched Player aktualisieren
            if self._key_lock and self._is_playing and old_speed != value:
                # Neu berechnen und Player neu erstellen
                self._create_pitched_player()
                if self._pitched_player:
                    # EQ-Kette neu aufbauen
                    self._create_eq_chain()
                    self.output.out()

            # STEM-SYNC: Master-Phasor und Stem-Player aktualisieren
            if old_speed != value:
                self._update_stems_speed(float(value))

        except Exception as e:
            logger.debug(f"Error setting speed: {e}")

    def _update_stems_speed(self, new_speed):
        """Aktualisiert die Speed für alle Stem-Player.
        Wird von set_speed aufgerufen wenn sich die Geschwindigkeit ändert.
        """
        try:
            button_data = get_button_data()

            # Finde den button_id für diesen Loop
            button_id = None
            for btn_id, data in button_data.items():
                if data.get("pyo") is self:
                    button_id = btn_id
                    break

            if button_id is None:
                return

            data = button_data[button_id]

            # Prüfe ob Stems initialisiert sind
            if not data["stems"].get("initialized", False):
                return

            # Master-Phasor Frequenz aktualisieren
            master_phasor = data["stems"].get("master_phasor")
            if master_phasor:
                loop_duration = (
                    float(self.loop_end - self.loop_start)
                    if self.loop_end
                    else float(self._duration)
                )
                if loop_duration > 0:
                    new_freq = float(new_speed) / loop_duration
                    master_phasor.freq = new_freq

            # Bei Key Lock: Stem-Caches müssen neu erstellt werden
            if self._key_lock and data["stems"]["cached_speed"] != new_speed:
                # Stems im Hintergrund neu pitchen
                self._schedule_stem_repitch(button_id, new_speed)
        except Exception as e:
            logger.debug(f"Error updating stems speed: {e}")

    def _schedule_stem_repitch(self, button_id, new_speed):
        """Plant das Neu-Pitchen der Stems im Hintergrund.
        Wird aufgerufen wenn Key Lock aktiv ist und sich die Speed ändert.
        """
        try:
            button_data = get_button_data()
            data = button_data[button_id]
            master_amp = get_master_amp()

            def do_repitch():
                try:
                    # Für jeden Stem: neu pitchen
                    for stem in STEM_NAMES:
                        dry_audio = data["stems"]["dry"].get(stem)
                        if dry_audio is None:
                            continue

                        # Pitch-Shift berechnen
                        semitones = -12 * np.log2(new_speed) if new_speed > 0 else 0

                        pitch_shifter = PitchShift(semitones=semitones)

                        # Audio vorbereiten
                        audio = dry_audio.copy()
                        audio = audio.reshape(1, -1) if len(audio.shape) == 1 else audio.T

                        # Pitchen
                        pitched = pitch_shifter(audio, self._audio_sr)

                        if pitched.shape[0] <= 2:
                            pitched = pitched.T

                        data["stems"]["pitched"][stem] = pitched.astype(np.float32)

                    # Jetzt die Player mit dem neuen Audio aktualisieren
                    master_phasor = data["stems"].get("master_phasor")
                    if not master_phasor:
                        return

                    for stem in STEM_NAMES:
                        pitched_audio = data["stems"]["pitched"].get(stem)
                        if pitched_audio is None:
                            continue

                        # Alte Player stoppen
                        old_player = data["stems"]["players"].get(stem)
                        if old_player:
                            with contextlib.suppress(builtins.BaseException):
                                old_player.stop()

                        # Neue Table erstellen
                        temp_file = tempfile.NamedTemporaryFile(suffix=".wav", delete=False)
                        temp_path = temp_file.name
                        temp_file.close()

                        sf.write(temp_path, pitched_audio, self._audio_sr)

                        new_table = SndTable(temp_path)

                        # Neuen Player erstellen
                        stem_gain = data["stems"]["gains"].get(stem)
                        if stem_gain:
                            new_player = Pointer(
                                table=new_table, index=master_phasor, mul=stem_gain * master_amp
                            )
                            new_player.out()

                            data["stems"]["players"][stem] = new_player
                            data["stems"]["tables"][stem] = new_table

                        with contextlib.suppress(builtins.BaseException):
                            os.unlink(temp_path)

                    # Main Player auch aktualisieren
                    if data["stems"].get("main_player"):
                        with contextlib.suppress(builtins.BaseException):
                            data["stems"]["main_player"].stop()

                    # Main Audio neu pitchen
                    if self._pitched_audio_cache is not None:
                        temp_file = tempfile.NamedTemporaryFile(suffix=".wav", delete=False)
                        temp_path = temp_file.name
                        temp_file.close()

                        sf.write(temp_path, self._pitched_audio_cache, self._audio_sr)

                        main_table = SndTable(temp_path)
                        main_gain = data["stems"]["main_gain"]

                        if main_gain:
                            main_player = Pointer(
                                table=main_table, index=master_phasor, mul=main_gain * master_amp
                            )
                            main_player.out()

                            data["stems"]["main_player"] = main_player
                            data["stems"]["main_table"] = main_table

                        with contextlib.suppress(builtins.BaseException):
                            os.unlink(temp_path)

                    data["stems"]["cached_speed"] = new_speed

                except Exception as e:
                    logger.error(f"Error repitching stems: {e}")

            # Im Hintergrund ausführen
            io_executor.submit(do_repitch)
        except Exception as e:
            logger.debug(f"Error scheduling stem repitch: {e}")

    def update_loop_points(self, loop_start, loop_end):
        """Aktualisiert die Loop-Punkte."""
        self.loop_start = float(loop_start)
        self.loop_end = float(loop_end) if loop_end is not None else None

        # Pitch-Cache invalidieren weil sich die Loop-Punkte geändert haben
        self._invalidate_pitch_cache()

        if self._is_playing:
            try:
                current_speed = self._pending_speed
                current_gain_db = self._pending_gain
                self.stop()
                self._create_player()
                self.set_gain(current_gain_db)
                self.set_speed(current_speed)
                self.play()
            except Exception as e:
                logger.error(f"Error updating loop points: {e}")
        else:
            self._pyo_initialized = False
            self._stop_all_objects()
            self.amp = None
            self.speed = None

    def enable_level_meter(self):
        """Aktiviert das Level-Metering."""
        source = self._pitched_player if (self._key_lock and self._pitched_player) else self.player
        if source and self.follower is None:
            try:
                self.follower = Follower(source, freq=20)
            except Exception as e:
                logger.debug(f"Error enabling level meter: {e}")

    def disable_level_meter(self):
        """Deaktiviert das Level-Metering."""
        if self.follower:
            try:
                self.follower.stop()
            except Exception:
                pass  # Follower bereits gestoppt
            self.follower = None

    def get_level_db(self):
        """Gibt den aktuellen Pegel in dB zurück."""
        try:
            source = (
                self._pitched_player if (self._key_lock and self._pitched_player) else self.player
            )
            if self.follower and source and self._is_playing:
                amp = self.follower.get()
                if amp > 0.0001:
                    return 20 * np.log10(amp)
            return -80.0
        except Exception:
            return -80.0  # Sicherer Fallback-Wert
