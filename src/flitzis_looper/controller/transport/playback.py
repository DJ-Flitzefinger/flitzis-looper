from typing import TYPE_CHECKING

from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController


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

        if self._project.multi_loop:
            self.stop_pad(sample_id)
        else:
            self.stop_all_pads()

        start_s, end_s = self._loop.effective_region(sample_id)
        self._audio.set_pad_loop_region(sample_id, start_s, end_s)
        self._audio.play_sample(sample_id, 1.0)
        self._session.active_sample_ids.add(sample_id)

    def stop_pad(self, sample_id: int) -> None:
        """Stop a pad if it is currently active."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.active_sample_ids:
            return

        self._audio.stop_sample(sample_id)
        self._session.active_sample_ids.discard(sample_id)

    def stop_all_pads(self) -> None:
        """Stop all currently active pads."""
        self._audio.stop_all()
        self._session.active_sample_ids.clear()

    def play(self, sample_id: int) -> None:
        # TODO: used only by wave editor, do we really need this to be separate from `trigger_pad`?
        validate_sample_id(sample_id)
        if self._transport._project.sample_paths[sample_id] is None:
            return

        self._transport.loop._apply_effective_pad_loop_region_to_audio(sample_id)
        self._transport._audio.play_sample(sample_id, 1.0)
        self._transport._session.active_sample_ids.add(sample_id)

    def toggle(self, sample_id: int) -> None:
        validate_sample_id(sample_id)
        if sample_id in self._transport._session.active_sample_ids:
            self.stop_pad(sample_id)
        else:
            self.play(sample_id)
