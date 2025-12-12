"""
BPM detection functionality for flitzis_looper.
Uses madmom for beat tracking and BPM detection.
"""

import sys
import os
import numpy as np

from flitzis_looper.utils.logging import logger


def _detect_bpm_worker(filepath):
    """
    Detects BPM from an audio file using madmom's beat tracking.
    
    Args:
        filepath: Path to the audio file
        
    Returns:
        float: Detected BPM rounded to 1 decimal, or None if detection fails
    """
    try:
        # Try to lower process priority on non-Windows systems
        try:
            if sys.platform != 'win32':
                os.nice(10)
        except OSError:
            pass  # nice() nicht verfügbar oder keine Berechtigung
        
        if not os.path.exists(filepath):
            return None
        
        # Import madmom here to handle ImportError gracefully
        from madmom.features.beats import RNNBeatProcessor, DBNBeatTrackingProcessor
        
        proc = DBNBeatTrackingProcessor(fps=100, min_bpm=60, max_bpm=180)
        act = RNNBeatProcessor()(filepath)
        beats = proc(act)
        
        if len(beats) > 1:
            intervals = np.diff(beats)
            avg_interval = np.mean(intervals)
            bpm = 60.0 / avg_interval
            if 60 <= bpm <= 200:
                return round(bpm, 1)
        return None
        
    except ImportError:
        logger.debug("madmom not available for BPM detection")
        return None
    except Exception as e:
        logger.debug(f"BPM detection failed: {e}")
        return None
