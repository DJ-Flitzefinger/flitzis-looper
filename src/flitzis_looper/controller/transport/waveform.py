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
        return self._audio.get_waveform_render_data(pad_id, width_px, start_s, end_s)
