"""AudioEngine - Zentrale Audio-Server-Verwaltung für flitzis_looper.

Kapselt das pyo Audio-Subsystem und master_amp in einer testbaren Klasse.
Keine tkinter- oder state.py-Abhängigkeiten.
"""

from __future__ import annotations

import contextlib

from pyo import Server, Sig, pa_get_default_output

from flitzis_looper.utils.logging import logger


class AudioEngine:
    """Zentrale Audio-Engine für die Anwendung.

    Kapselt pyo Server und Master-Amplitude in einer testbaren Klasse.
    Keine UI-Abhängigkeiten - kann unabhängig getestet werden.

    Attributes:
        server: Der pyo Audio-Server
        master_amp: Master-Amplitude Sig-Objekt (0.0 - 1.0)

    Example:
        >>> engine = AudioEngine()
        >>> engine.boot()
        >>> engine.set_master_volume(0.8)
        >>> engine.shutdown()
    """

    def __init__(
        self,
        sr: int = 44100,
        nchnls: int = 2,
        buffersize: int = 1024,
        duplex: int = 0,
    ):
        """Initialisiert die AudioEngine.

        Args:
            sr: Sample-Rate (default 44100)
            nchnls: Anzahl Kanäle (default 2 = Stereo)
            buffersize: Buffer-Größe (default 1024)
            duplex: Duplex-Modus (default 0 = nur Output)
        """
        self._sr = sr
        self._nchnls = nchnls
        self._buffersize = buffersize
        self._duplex = duplex

        self._server: Server | None = None
        self._master_amp: Sig | None = None
        self._is_booted = False
        self._is_started = False

    @property
    def server(self) -> Server | None:
        """Gibt den pyo Server zurück."""
        return self._server

    @property
    def master_amp(self) -> Sig | None:
        """Gibt das Master-Amplitude Sig-Objekt zurück."""
        return self._master_amp

    @property
    def is_booted(self) -> bool:
        """True wenn der Server gebootet ist."""
        return self._is_booted

    @property
    def is_started(self) -> bool:
        """True wenn der Server gestartet ist."""
        return self._is_started

    def boot(self, output_device: int | None = None) -> AudioEngine:
        """Bootet den Audio-Server.

        Args:
            output_device: Output-Device-ID (None = Default-Device)

        Returns:
            self für Method-Chaining

        Raises:
            RuntimeError: Wenn Boot fehlschlägt
        """
        if self._is_booted:
            logger.warning("AudioEngine already booted")
            return self

        try:
            # Server erstellen
            self._server = Server(
                sr=self._sr,
                nchnls=self._nchnls,
                buffersize=self._buffersize,
                duplex=self._duplex,
            )

            # Output-Device setzen
            if output_device is None:
                output_device = pa_get_default_output()
            self._server.setOutputDevice(output_device)

            # Boot
            self._server.boot()
            self._is_booted = True

            logger.debug(
                f"AudioEngine booted: sr={self._sr}, nchnls={self._nchnls}, "
                f"buffersize={self._buffersize}"
            )
        except Exception as e:
            logger.error(f"Failed to boot AudioEngine: {e}")
            msg = f"AudioEngine boot failed: {e}"
            raise RuntimeError(msg) from e

        return self

    def start(self) -> AudioEngine:
        """Startet den Audio-Server.

        Erstellt auch das master_amp Sig-Objekt.

        Returns:
            self für Method-Chaining

        Raises:
            RuntimeError: Wenn Server nicht gebootet oder Start fehlschlägt
        """
        if not self._is_booted:
            msg = "AudioEngine not booted. Call boot() first."
            raise RuntimeError(msg)

        if self._is_started:
            logger.warning("AudioEngine already started")
            return self

        try:
            self._server.start()
            self._is_started = True

            # Master-Amplitude erstellen
            self._master_amp = Sig(1.0)

            logger.debug("AudioEngine started")
        except Exception as e:
            logger.error(f"Failed to start AudioEngine: {e}")
            msg = f"AudioEngine start failed: {e}"
            raise RuntimeError(msg) from e

        return self

    def stop(self) -> AudioEngine:
        """Stoppt den Audio-Server.

        Returns:
            self für Method-Chaining
        """
        if not self._is_started:
            return self

        with contextlib.suppress(Exception):
            self._server.stop()
        self._is_started = False
        logger.debug("AudioEngine stopped")

        return self

    def shutdown(self) -> None:
        """Fährt den Audio-Server komplett herunter.

        Stoppt den Server und gibt alle Ressourcen frei.
        """
        if self._master_amp is not None:
            with contextlib.suppress(Exception):
                self._master_amp.stop()
            self._master_amp = None

        if self._server is not None:
            try:
                # Prüfe ob Server noch läuft
                get_is_started = getattr(self._server, "getIsStarted", None)
                if callable(get_is_started):
                    if get_is_started():
                        self._server.shutdown()
                else:
                    self._server.shutdown()
            except Exception as e:
                logger.debug(f"Error during AudioEngine shutdown: {e}")
            self._server = None

        self._is_booted = False
        self._is_started = False
        logger.debug("AudioEngine shutdown complete")

    def set_master_volume(self, value: float) -> None:
        """Setzt die Master-Lautstärke.

        Args:
            value: Lautstärke (0.0 - 1.0)
        """
        if self._master_amp is not None:
            self._master_amp.value = max(0.0, min(1.0, value))

    def get_master_volume(self) -> float:
        """Gibt die aktuelle Master-Lautstärke zurück.

        Returns:
            Lautstärke (0.0 - 1.0), oder 1.0 wenn nicht initialisiert
        """
        if self._master_amp is not None:
            return self._master_amp.value
        return 1.0


# ============== GLOBAL ENGINE INSTANCE ==============
_engine_instance: AudioEngine | None = None


def get_engine() -> AudioEngine:
    """Gibt die globale AudioEngine-Instanz zurück.

    Raises:
        RuntimeError: Wenn die Engine nicht initialisiert wurde.
    """
    if _engine_instance is None:
        msg = "AudioEngine not initialized. Call init_engine() first."
        raise RuntimeError(msg)
    return _engine_instance


def init_engine(
    sr: int = 44100,
    nchnls: int = 2,
    buffersize: int = 1024,
    duplex: int = 0,
) -> AudioEngine:
    """Initialisiert die globale AudioEngine-Instanz.

    Args:
        sr: Sample-Rate (default 44100)
        nchnls: Anzahl Kanäle (default 2 = Stereo)
        buffersize: Buffer-Größe (default 1024)
        duplex: Duplex-Modus (default 0 = nur Output)

    Returns:
        Die erstellte AudioEngine-Instanz
    """
    global _engine_instance
    _engine_instance = AudioEngine(sr=sr, nchnls=nchnls, buffersize=buffersize, duplex=duplex)
    return _engine_instance


def get_master_amp() -> Sig | None:
    """Gibt das Master-Amplitude Sig-Objekt zurück.

    Convenience-Funktion für Backward-Kompatibilität.

    Returns:
        Das master_amp Sig-Objekt, oder None wenn Engine nicht initialisiert.
    """
    if _engine_instance is None:
        return None
    return _engine_instance.master_amp


def set_master_amp(amp: Sig) -> None:
    """Setzt das Master-Amplitude Sig-Objekt.

    Für Backward-Kompatibilität während der Migration.

    Args:
        amp: Ein pyo Sig-Objekt für Master-Amplitude
    """
    if _engine_instance is not None:
        _engine_instance._master_amp = amp
