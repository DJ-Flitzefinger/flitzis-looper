"""flitzis_looper.ui.widgets - Wiederverwendbare UI-Widgets.

Enthält:
- EQKnob: Drehregler für EQ (Low/Mid/High)
- VUMeter: Level-Anzeige mit Segmenten
- LoopGridWidget: 6x6 Loop-Button-Grid mit Bank-Buttons
- StemsPanelWidget: Stem-Toggle-Buttons (V/M/B/D/I/S)
- ToolbarWidget: BPM/Speed/Volume Controls
"""

from .eq_knob import EQKnob
from .loop_grid import LoopGridWidget
from .stems_panel import StemsPanelWidget
from .toolbar import ToolbarWidget
from .vu_meter import VUMeter

__all__ = [
    "EQKnob",
    "LoopGridWidget",
    "StemsPanelWidget",
    "ToolbarWidget",
    "VUMeter",
]
