"""
pyo Audio Server initialization for flitzis_looper.
Provides the audio server and master amplitude control.
"""

from pyo import Server, Sig

from flitzis_looper.utils.logging import logger

# Module-level server instance (initialized by init_server)
_server = None
_master_amp = None


def init_server(sr=44100, nchnls=2, buffersize=1024, duplex=0):
    """
    Initialize and start the pyo audio server.
    
    Args:
        sr: Sample rate (default 44100)
        nchnls: Number of channels (default 2, stereo)
        buffersize: Buffer size (default 1024)
        duplex: Duplex mode (default 0, output only)
    
    Returns:
        Server: The initialized and started pyo Server instance
    """
    global _server
    try:
        _server = Server(sr=sr, nchnls=nchnls, buffersize=buffersize, duplex=duplex).boot()
        _server.start()
        logger.debug(f"Audio server started: sr={sr}, nchnls={nchnls}")
        return _server
    except Exception as e:
        logger.error(f"Failed to initialize audio server: {e}")
        raise


def get_server():
    """Returns the audio server instance."""
    return _server


def init_master_amp(initial_value=1.0):
    """
    Initialize the master amplitude signal.
    
    Args:
        initial_value: Initial amplitude value (default 1.0)
        
    Returns:
        Sig: The master amplitude pyo Sig object
    """
    global _master_amp
    _master_amp = Sig(initial_value)
    return _master_amp


def get_master_amp():
    """Returns the master amplitude signal."""
    return _master_amp


def set_master_amp(amp_sig):
    """
    Set the master amplitude signal (used for backward compatibility).
    
    Args:
        amp_sig: A pyo Sig object to use as master amplitude
    """
    global _master_amp
    _master_amp = amp_sig


def shutdown_server():
    """Stop and cleanup the audio server."""
    global _server
    if _server is not None:
        try:
            _server.stop()
            logger.debug("Audio server stopped")
        except Exception as e:
            logger.debug(f"Error stopping server: {e}")
        _server = None
