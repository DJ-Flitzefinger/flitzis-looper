from typing import TYPE_CHECKING

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

        if not self._project.multi_loop:
            self.stop_all_pads()

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
        self._session.pad_playhead_s[pad_id] = None
