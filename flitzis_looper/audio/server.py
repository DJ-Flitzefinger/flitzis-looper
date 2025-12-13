"""pyo Audio Server initialization for flitzis_looper.

DEPRECATED: Dieses Modul ist für Backward-Kompatibilität.
Neue Entwicklung sollte audio/engine.py verwenden.

Provides the audio server and master amplitude control.
"""

from pyo import Server, Sig

from flitzis_looper.audio.engine import (
    get_engine,
    get_master_amp,
    init_engine,
    set_master_amp,
)

# Re-export from engine für Backward-Kompatibilität
__all__ = [
    "get_master_amp",
    "get_server",
    "init_master_amp",
    "init_server",
    "set_master_amp",
    "shutdown_server",
]


def init_server(sr=44100, nchnls=2, buffersize=1024, duplex=0) -> Server:
    """Initialize and start the pyo audio server.

    DEPRECATED: Verwende stattdessen AudioEngine.

    Args:
        sr: Sample rate (default 44100)
        nchnls: Number of channels (default 2, stereo)
        buffersize: Buffer size (default 1024)
        duplex: Duplex mode (default 0, output only)

    Returns:
        Server: The initialized and started pyo Server instance
    """
    engine = init_engine(sr=sr, nchnls=nchnls, buffersize=buffersize, duplex=duplex)
    engine.boot()
    engine.start()
    return engine.server


def get_server() -> Server | None:
    """Returns the audio server instance.

    DEPRECATED: Verwende stattdessen get_engine().server.
    """
    try:
        return get_engine().server
    except RuntimeError:
        return None


def init_master_amp(initial_value=1.0) -> Sig:
    """Initialize the master amplitude signal.

    DEPRECATED: AudioEngine erstellt master_amp automatisch bei start().

    Args:
        initial_value: Initial amplitude value (default 1.0)

    Returns:
        Sig: The master amplitude pyo Sig object
    """
    # AudioEngine erstellt master_amp automatisch
    amp = get_master_amp()
    if amp is not None:
        amp.value = initial_value
    return amp


def shutdown_server() -> None:
    """Stop and cleanup the audio server.

    DEPRECATED: Verwende stattdessen get_engine().shutdown().
    """
    try:
        engine = get_engine()
        engine.shutdown()
    except RuntimeError:
        pass
