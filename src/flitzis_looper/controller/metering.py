import math
from time import monotonic
from typing import TYPE_CHECKING

from flitzis_looper.controller.base import BaseController

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine, AudioMessage


class MeteringController(BaseController):
    _PAD_PEAK_HALF_LIFE_SEC = 0.25
    _PAD_CLIP_HOLD_SEC = 1.0
    _MASTER_PEAK_HALF_LIFE_SEC = 0.25
    _MASTER_CLIP_HOLD_SEC = 1.0

    def __init__(self, project: ProjectState, session: SessionState, audio: AudioEngine) -> None:
        super().__init__(project, session, audio)

        self._on_frame_render_callbacks.append(self._decay_pad_peaks)
        self._on_frame_render_callbacks.append(self._decay_master_peak)

    def handle_pad_peak_message(self, msg: AudioMessage.PadPeak) -> None:
        now = monotonic()

        sample_id = msg.sample_id()
        if sample_id is None or not 0 <= sample_id < len(self._session.pad_peak):
            return

        peak = msg.pad_peak()
        if peak is None or not math.isfinite(peak):
            return

        if peak >= 1.0:
            self._session.pad_clip_hold_until[sample_id] = now + self._PAD_CLIP_HOLD_SEC

        peak = min(max(peak, 0.0), 1.0)
        self._session.pad_peak[sample_id] = max(self._session.pad_peak[sample_id], peak)
        self._session.pad_peak_updated_at[sample_id] = now

    def pad_clip_active(self, sample_id: int) -> bool:
        if not 0 <= sample_id < len(self._session.pad_clip_hold_until):
            return False
        return self._session.pad_clip_hold_until[sample_id] > monotonic()

    def handle_master_peak_message(self, msg: AudioMessage.MasterPeak) -> None:
        now = monotonic()

        peak = msg.master_peak()
        if peak is None or not math.isfinite(peak):
            return

        peak = max(peak, 0.0)
        if peak >= 1.0:
            self._session.master_output_clip_hold_until = now + self._MASTER_CLIP_HOLD_SEC

        self._session.master_output_peak = max(self._session.master_output_peak, peak)
        self._session.master_output_peak_updated_at = now

    def master_clip_active(self) -> bool:
        return self._session.master_output_clip_hold_until > monotonic()

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
            peaks[idx], updated[idx] = self._decayed_peak(
                peak,
                updated[idx],
                now,
                half_life_sec=self._PAD_PEAK_HALF_LIFE_SEC,
            )

    def _decay_master_peak(self) -> None:
        peak, updated_at = self._decayed_peak(
            self._session.master_output_peak,
            self._session.master_output_peak_updated_at,
            monotonic(),
            half_life_sec=self._MASTER_PEAK_HALF_LIFE_SEC,
        )
        self._session.master_output_peak = peak
        self._session.master_output_peak_updated_at = updated_at

    @staticmethod
    def _decayed_peak(
        peak: float,
        updated_at: float,
        now: float,
        *,
        half_life_sec: float,
    ) -> tuple[float, float]:
        if peak <= 0.0:
            return peak, updated_at

        if updated_at <= 0.0:
            return peak, now

        dt = now - updated_at
        if dt <= 0.0:
            return peak, updated_at

        decay = 0.5 ** (dt / half_life_sec)
        decayed = peak * decay
        return (0.0 if decayed < 1e-4 else decayed), now
