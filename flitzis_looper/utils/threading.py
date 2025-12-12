"""Threading utilities for flitzis_looper.

Provides executors for IO and BPM operations, and GUI update queue.
"""

import queue
from concurrent.futures import ThreadPoolExecutor

from flitzis_looper.utils.logging import get_logger

logger = get_logger(__name__)

# Thread pools for background operations
io_executor = ThreadPoolExecutor(max_workers=2)
bpm_executor = ThreadPoolExecutor(max_workers=1)

# Queue for GUI updates from background threads
gui_update_queue = queue.Queue()

# Reference to root window for after() scheduling (set via start_gui_queue_processor)
_root = None


def schedule_gui_update(callback):
    """Schedule a callback to be executed on the GUI thread."""
    gui_update_queue.put(callback)


def _process_gui_queue():
    """Process pending GUI updates from the queue.

    Called periodically via root.after().
    """
    try:
        for _ in range(10):
            try:
                callback = gui_update_queue.get_nowait()
                callback()
            except queue.Empty:
                break
    except Exception as e:
        logger.debug("GUI queue processing error: %s", e)
    if _root is not None:
        _root.after(30, _process_gui_queue)


def start_gui_queue_processor(root):
    """Start the GUI queue processor with the given root window.

    Must be called after tk.Tk() is created.
    """
    global _root
    _root = root
    _process_gui_queue()
