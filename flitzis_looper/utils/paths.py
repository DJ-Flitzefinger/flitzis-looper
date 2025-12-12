"""
Path constants and directory initialization for flitzis_looper.
"""

import os

LOOP_DIR = "loops"
CONFIG_FILE = "config.json"

# Ensure loop directory exists
os.makedirs(LOOP_DIR, exist_ok=True)
