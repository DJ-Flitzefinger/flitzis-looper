"""Logging configuration for flitzis_looper."""

import logging

# Configure logging
logging.basicConfig(level=logging.ERROR)
logger = logging.getLogger(__name__)


def get_logger(name=None):
    """Returns a logger instance. If name is provided, returns a logger for that module.

    Otherwise returns the default flitzis_looper logger.
    """
    if name:
        return logging.getLogger(name)
    return logger
