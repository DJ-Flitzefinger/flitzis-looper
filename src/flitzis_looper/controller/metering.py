import math
from time import monotonic
from typing import TYPE_CHECKING

from flitzis_looper.controller.base import BaseController

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine, AudioMessage


class MeteringController(BaseController):
    _PAD_PEAK_HALF_LIFE_SEC = 0.25

    def __init__(self, project: ProjectState, session: SessionState, audio: AudioEngine) -> None:
        super().__init__(project, session, audio)

        self._on_frame_render_callbacks.append(self._decay_pad_peaks)

    def handle_pad_peak_message(self, msg: AudioMessage.PadPeak) -> None:
        now = monotonic()

        sample_id = msg.sample_id()
        if sample_id is None or not 0 <= sample_id < len(self._session.pad_peak):
            return

        peak = msg.pad_peak()
        if peak is None or not math.isfinite(peak):
            return

        peak = min(max(peak, 0.0), 1.0)
        self._session.pad_peak[sample_id] = max(self._session.pad_peak[sample_id], peak)
        self._session.pad_peak_updated_at[sample_id] = now

    def handle_pad_playhead_message(self, msg: AudioMessage.PadPlayhead) -> None:
        now = monotonic()

        sample_id = msg.sample_id()
        if sample_id is None or not 0 <= sample_id < len(self._session.pad_peak):
            return

        position_s = msg.pad_playhead()
        if position_s is None or not math.isfinite(position_s) or position_s < 0.0:
            return

        self._session.pad_playhead_s[sample_id] = position_s
        self._session.pad_playhead_updated_at[sample_id] = now

    def _decay_pad_peaks(self) -> None:
        now = monotonic()
        peaks = self._session.pad_peak
        updated = self._session.pad_peak_updated_at
        for idx, peak in enumerate(peaks):
            if peak <= 0.0:
                continue

            last = updated[idx]
            if last <= 0.0:
                updated[idx] = now
                continue

            dt = now - last
            if dt <= 0.0:
                continue

            decay = 0.5 ** (dt / self._PAD_PEAK_HALF_LIFE_SEC)
            decayed = peak * decay
            peaks[idx] = 0.0 if decayed < 1e-4 else decayed
            updated[idx] = now
