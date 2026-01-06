import math
from time import monotonic
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from flitzis_looper.models import SessionState


class MeteringController:
    _PAD_PEAK_HALF_LIFE_SEC = 0.25

    def __init__(self, session: SessionState, audio: Any) -> None:
        self._session = session
        self._audio = audio

    def poll_audio_messages(self) -> None:
        now = monotonic()
        self._decay_pad_peaks(now)

        while True:
            msg = self._audio.receive_msg()
            if msg is None:
                return

            self._handle_pad_peak_message(msg, now)
            self._handle_pad_playhead_message(msg, now)

    def _handle_pad_peak_message(self, msg: Any, now: float) -> None:
        pad_peak_fn = getattr(msg, "pad_peak", None)
        if pad_peak_fn is None:
            return

        peak_data = pad_peak_fn()
        if peak_data is None:
            return

        sample_id, peak = peak_data
        if not isinstance(sample_id, int) or not 0 <= sample_id < len(self._session.pad_peak):
            return

        peak = float(peak)
        if not math.isfinite(peak):
            return

        peak = min(max(peak, 0.0), 1.0)
        self._session.pad_peak[sample_id] = max(self._session.pad_peak[sample_id], peak)
        self._session.pad_peak_updated_at[sample_id] = now

    def _handle_pad_playhead_message(self, msg: Any, now: float) -> None:
        pad_playhead_fn = getattr(msg, "pad_playhead", None)
        if pad_playhead_fn is None:
            return

        playhead_data = pad_playhead_fn()
        if playhead_data is None:
            return

        sample_id, position_s = playhead_data
        if not isinstance(sample_id, int) or not 0 <= sample_id < len(self._session.pad_playhead_s):
            return

        position_s = float(position_s)
        if not math.isfinite(position_s) or position_s < 0.0:
            return

        self._session.pad_playhead_s[sample_id] = position_s
        self._session.pad_playhead_updated_at[sample_id] = now

    def _decay_pad_peaks(self, now: float) -> None:
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
