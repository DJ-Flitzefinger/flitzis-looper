"""flitzis_looper.ui.dialogs - Dialog-Fenster für flitzis_looper.

Module:
- volume: Volume + EQ Dialog (set_volume)
- waveform: WaveformEditor Klasse (Adjust Loop)
- bpm_dialog: BPM Dialog (set_bpm_manually)
"""

from .bpm_dialog import set_bpm_manually
from .volume import set_volume
from .waveform import WaveformEditor

__all__ = [
    "WaveformEditor",
    "set_bpm_manually",
    "set_volume",
]
