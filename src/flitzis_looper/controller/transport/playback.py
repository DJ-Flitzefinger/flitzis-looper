from typing import TYPE_CHECKING

from flitzis_looper.controller.transport.global_playback import GlobalPlaybackController
from flitzis_looper.controller.validation import ensure_finite
from flitzis_looper.input_timing import validate_input_timestamp_ns
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from collections.abc import Callable

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
        self._global_playback = GlobalPlaybackController(transport)

    def _forget_global_start_stop_restore(self) -> None:
        self._global_playback.forget_restore()

    def discard_global_restore_for_unloaded_pad(self, sample_id: int) -> None:
        """Prevent a late STOP acknowledgement from remembering an unloaded pad."""
        self._global_playback.discard_pad_restore(sample_id)

    def set_global_playback_feedback_poll(self, callback: Callable[[], None]) -> None:
        """Drain native playback feedback before a subsequent global target capture."""
        self._global_playback.set_feedback_poll(callback)

    def trigger_pad(self, sample_id: int, *, received_at_ns: int | None = None) -> None:
        """Trigger or retrigger a pad's loop.

        When Multi Loop is disabled, all other active pads are stopped first.

        Args:
            sample_id: Sample slot identifier.
            received_at_ns: Original input timestamp from the Rust engine epoch.
        """
        validate_sample_id(sample_id)
        received_at_ns = validate_input_timestamp_ns(received_at_ns)

        if self._project.sample_paths[sample_id] is None:
            return

        self._start_pad(
            sample_id,
            exclusive=not self._project.multi_loop,
            received_at_ns=received_at_ns,
        )
        self._forget_global_start_stop_restore()

    def trigger_pad_keep_others(self, sample_id: int, *, received_at_ns: int | None = None) -> None:
        """Trigger or retrigger a pad's loop without stopping other pads.

        This is intended for workflows like the waveform editor where starting
        playback must not affect other currently-playing pads.

        Args:
            sample_id: Sample slot identifier.
            received_at_ns: Original input timestamp from the Rust engine epoch.
        """
        validate_sample_id(sample_id)
        received_at_ns = validate_input_timestamp_ns(received_at_ns)

        if self._project.sample_paths[sample_id] is None:
            return

        self._start_pad(sample_id, received_at_ns=received_at_ns)
        self._forget_global_start_stop_restore()

    def _start_pad(
        self,
        sample_id: int,
        *,
        exclusive: bool = False,
        received_at_ns: int | None = None,
    ) -> None:
        start_s, end_s = self._loop.requested_region(sample_id)
        play = self._audio.play_sample_exclusive if exclusive else self._audio.play_sample
        self._transport.residency.start(
            sample_id,
            start_s,
            end_s,
            lambda: (
                play(sample_id, 1.0)
                if received_at_ns is None
                else play(sample_id, 1.0, received_at_ns=received_at_ns)
            ),
            prepared_play=lambda ticket: self._audio.play_resident_control(
                ticket, exclusive=exclusive, received_at_ns=received_at_ns
            ),
        )

    def stop_pad(self, sample_id: int) -> None:
        """Stop a pad if it is currently active."""
        validate_sample_id(sample_id)
        queued = self._transport.residency.cancel_launch(sample_id)
        if sample_id not in self._session.active_sample_ids and not queued:
            return

        self._audio.stop_sample(sample_id)
        self._transport.residency.cancel_requested(sample_id)
        self._forget_global_start_stop_restore()

    def stop_all_pads(self, *, received_at_ns: int | None = None) -> None:
        """Admit a source-bound stop for the complete active/paused set."""
        self._global_playback.stop(remember=False, received_at_ns=received_at_ns)

    def start_or_restart_global_start_stop(self, *, received_at_ns: int | None = None) -> None:
        """Start remembered loops or restart active loops from their loop starts.

        Args:
            received_at_ns: One captured Rust input timestamp shared by every pad.
        """
        self._global_playback.start(received_at_ns=received_at_ns)

    def stop_global_start_stop(self) -> None:
        """Stop active loops from START/STOP right mouse down without starting anything."""
        self._global_playback.stop(remember=True)

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
        self._transport.residency.seek(sample_id, target_s)

    def handle_sample_started_message(self, msg: AudioMessage.SampleStarted) -> None:
        pad_id = msg.sample_id()
        if pad_id is None:
            return

        self._session.active_sample_ids.add(pad_id)
        self._session.paused_sample_ids.discard(pad_id)

    def handle_sample_stopped_message(self, msg: AudioMessage.SampleStopped) -> None:
        pad_id = msg.sample_id()
        if pad_id is None:
            return

        self._session.active_sample_ids.discard(pad_id)
        self._session.paused_sample_ids.discard(pad_id)
