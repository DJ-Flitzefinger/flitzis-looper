"""flitzis_looper.ui.widgets - Wiederverwendbare UI-Widgets.

Enthält:
- EQKnob: Drehregler für EQ (Low/Mid/High)
- VUMeter: Level-Anzeige mit Segmenten
"""

from .eq_knob import EQKnob
from .vu_meter import VUMeter

__all__ = ["EQKnob", "VUMeter"]
