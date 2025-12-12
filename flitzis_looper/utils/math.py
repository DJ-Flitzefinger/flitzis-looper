"""Mathematical utility functions for flitzis_looper."""

import math


def db_to_amp(db):
    return 10 ** (db / 20.0)


def speed_to_semitones(speed):
    """Berechnet die Halbtöne-Kompensation für Key Lock.

    Bei speed > 1: Pitch würde steigen -> negative Halbtöne zum Kompensieren
    Bei speed < 1: Pitch würde sinken -> positive Halbtöne zum Kompensieren.
    """
    if speed <= 0:
        return 0.0
    return -12.0 * math.log2(speed)
