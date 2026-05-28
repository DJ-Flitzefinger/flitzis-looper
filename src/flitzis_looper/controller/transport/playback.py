import math
from typing import TYPE_CHECKING

from flitzis_looper.controller.validation import ensure_finite
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import AudioMessage


class PadPlaybackController:
    """Manage pad playback triggering and stopping."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._audio = transport._audio
        self._loop = transport.loop

    def trigger_pad(self, sample_id: int) -> None:
        """Trigger or retrigger a pad's loop.

        When Multi Loop is disabled, all other active pads are stopped first.

        Args:
            sample_id: Sample slot identifier.
        """
        validate_sample_id(sample_id)

        if self._project.sample_paths[sample_id] is None:
            return

        start_s, end_s = self._loop.effective_region(sample_id)
        self._audio.set_pad_loop_region(sample_id, start_s, end_s)

        if not self._project.multi_loop:
            self._audio.play_sample_exclusive(sample_id, 1.0)
            return

        self._audio.play_sample(sample_id, 1.0)

    def trigger_pad_keep_others(self, sample_id: int) -> None:
        """Trigger or retrigger a pad's loop without stopping other pads.

        This is intended for workflows like the waveform editor where starting
        playback must not affect other currently-playing pads.
        """
        validate_sample_id(sample_id)

        if self._project.sample_paths[sample_id] is None:
            return

        start_s, end_s = self._loop.effective_region(sample_id)
        self._audio.set_pad_loop_region(sample_id, start_s, end_s)
        self._audio.play_sample(sample_id, 1.0)

    def stop_pad(self, sample_id: int) -> None:
        """Stop a pad if it is currently active."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.active_sample_ids:
            return

        self._audio.stop_sample(sample_id)

    def stop_all_pads(self) -> None:
        """Stop all currently active pads."""
        self._audio.stop_all()

    def start_or_restart_global_start_stop(self) -> None:
        """Start remembered loops or restart active loops from their loop starts."""
        if self._session.global_stop_engaged:
            target_sample_ids = sorted(self._session.global_stop_restore_sample_ids)
            self._session.global_stop_engaged = False
            self._session.global_stop_restore_sample_ids = set()
        else:
            target_sample_ids = sorted(
                self._session.active_sample_ids - self._session.paused_sample_ids
            )

        if not target_sample_ids:
            return

        started_sample_ids: set[int] = set()
        for sample_id in target_sample_ids:
            if self._project.sample_paths[sample_id] is None:
                continue
            start_s, end_s = self._loop.effective_region(sample_id)
            self._audio.set_pad_loop_region(sample_id, start_s, end_s)
            self._audio.play_sample(sample_id, 1.0)
            started_sample_ids.add(sample_id)

        self._session.active_sample_ids.update(started_sample_ids)
        self._session.paused_sample_ids.difference_update(started_sample_ids)

    def stop_global_start_stop(self) -> None:
        """Stop active loops from START/STOP right mouse down without starting anything."""
        active_sample_ids = set(self._session.active_sample_ids)
        if not active_sample_ids:
            return

        playing_sample_ids = active_sample_ids - self._session.paused_sample_ids
        self._session.global_stop_restore_sample_ids = playing_sample_ids
        self._session.global_stop_engaged = bool(playing_sample_ids)
        self._audio.stop_all()
        self._session.active_sample_ids.clear()
        self._session.paused_sample_ids.clear()

    def pause_pad(self, sample_id: int) -> None:
        """Pause a pad if it is currently playing.

        The pad remains active but its voice is silenced.
        """
        validate_sample_id(sample_id)
        if sample_id not in self._session.active_sample_ids:
            return
        if sample_id in self._session.paused_sample_ids:
            return  # Already paused

        self._audio.pause_sample(sample_id)
        self._session.paused_sample_ids.add(sample_id)

    def resume_pad(self, sample_id: int) -> None:
        """Resume a paused pad.

        If the pad was paused, its voice continues from the saved position.
        If the pad was not paused, this has no effect.
        """
        validate_sample_id(sample_id)
        if sample_id not in self._session.active_sample_ids:
            return
        if sample_id not in self._session.paused_sample_ids:
            return  # Not paused

        self._audio.resume_sample(sample_id)
        self._session.paused_sample_ids.discard(sample_id)

    def seek_pad(self, sample_id: int, position_s: float) -> None:
        """Seek an active or paused pad voice without changing loop markers."""
        validate_sample_id(sample_id)
        ensure_finite(position_s)

        if self._project.sample_paths[sample_id] is None:
            return
        if sample_id not in self._session.active_sample_ids:
            return

        target_s = max(0.0, float(position_s))
        duration_s = self._project.sample_durations[sample_id]
        if duration_s is not None and math.isfinite(duration_s) and duration_s >= 0.0:
            target_s = min(target_s, float(duration_s))

        self._audio.seek_sample(sample_id, target_s)
        self._session.pad_playhead_s[sample_id] = target_s

    def handle_sample_started_message(self, msg: AudioMessage.SampleStarted) -> None:
        pad_id = msg.sample_id()
        if pad_id is None:
            return

        self._session.active_sample_ids.add(pad_id)

    def handle_sample_stopped_message(self, msg: AudioMessage.SampleStopped) -> None:
        pad_id = msg.sample_id()
        if pad_id is None:
            return

        self._session.active_sample_ids.discard(pad_id)
        self._session.paused_sample_ids.discard(pad_id)
