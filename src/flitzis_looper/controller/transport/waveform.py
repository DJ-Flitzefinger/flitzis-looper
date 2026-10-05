import math
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import WaveFormRenderData


class WaveformController:
    """Manage waveform editor."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._audio = transport._audio

    def get_render_data(
        self, pad_id: int, width_px: int, start_s: float, end_s: float
    ) -> WaveFormRenderData | None:
        if not math.isfinite(start_s) or not math.isfinite(end_s) or end_s <= start_s:
            return None
        source_start_s = max(0.0, start_s)
        duration_s = self._transport._project.sample_durations[pad_id]
        source_end_s = min(end_s, duration_s) if duration_s is not None else end_s
        if source_end_s <= source_start_s:
            return None
        source_width_px = max(
            1, round(width_px * (source_end_s - source_start_s) / (end_s - start_s))
        )
        return self._audio.get_waveform_render_data(
            pad_id, source_width_px, source_start_s, source_end_s
        )
