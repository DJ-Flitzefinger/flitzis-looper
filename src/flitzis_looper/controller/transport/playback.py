from typing import TYPE_CHECKING, TypedDict

from flitzis_looper.controller.transport.global_playback import GlobalPlaybackController
from flitzis_looper.controller.transport.pause_intent import OrdinaryPauseIntentController
from flitzis_looper.controller.validation import ensure_finite
from flitzis_looper.input_timing import validate_input_timestamp_ns
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.recording import RecordingGroupGate
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import (
        AudioMessage,
        ExplicitPlaybackTarget,
        GlobalPlaybackBatchTicket,
        OrdinaryTransportObservation,
    )


class _ExplicitTargetOptions(TypedDict, total=False):
    explicit_target: ExplicitPlaybackTarget


class PadPlaybackController:
    """Manage pad playback triggering and stopping."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._audio = transport._audio
        self._loop = transport.loop
        self._global_playback = GlobalPlaybackController(transport)
        self._pause_intent = OrdinaryPauseIntentController(transport)
        transport._on_frame_render_callbacks.append(self._pause_intent.poll)

    def forget_pause_intent(self, sample_id: int) -> None:
        """Evict the retired assignment's original requested pause owner."""
        self._pause_intent.forget_pad(sample_id)

    def poll_pause_observations(self) -> None:
        """Project current durable Native state even when a generic lifecycle event was lost."""
        self._pause_intent.poll()

    def set_pause_observation_fence(self, fenced: Callable[[int], bool]) -> None:
        """Use the App's same content/arrangement boundary for current snapshot queries."""
        self._pause_intent.bind_observation_fence(fenced)

    def handle_ordinary_transport_observation(
        self, observation: OrdinaryTransportObservation
    ) -> None:
        """Project a current Native observation through the existing content/mirror boundary."""
        self._pause_intent.project(observation)

    def _forget_global_start_stop_restore(self) -> None:
        self._global_playback.forget_restore()

    def discard_global_restore_for_unloaded_pad(self, sample_id: int) -> None:
        """Prevent a late STOP acknowledgement from remembering an unloaded pad."""
        self._global_playback.discard_pad_restore(sample_id)

    def set_global_playback_feedback_poll(self, callback: Callable[[], None]) -> None:
        """Drain native playback feedback before a subsequent global target capture."""
        self._global_playback.set_feedback_poll(callback)

    def _prepare_fenced_playback_observation(self) -> None:
        """Reserve bounded control observation before a Native STOP/mode admission."""
        self._global_playback.prepare_fenced_observation()

    def _observe_fenced_playback(self, ticket: GlobalPlaybackBatchTicket) -> None:
        """Retain an admitted Native fence without issuing another audio operation."""
        self._global_playback.observe_fenced_playback(ticket)

    def capture_recording_start(
        self, *, received_at_ns: int | None = None
    ) -> RecordingGroupGate | None:
        """Retain this original left press through the existing global-start owner."""
        return self._global_playback.capture_recording_start(received_at_ns=received_at_ns)

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
        explicit_target: ExplicitPlaybackTarget | None = None,
    ) -> None:
        self._transport.refresh_input_runtime((sample_id,))
        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        options = self._transport.source_generation_options(generation)
        target_options: _ExplicitTargetOptions = {}
        if explicit_target is not None:
            target_options["explicit_target"] = explicit_target
        start_s, end_s = self._loop.launch_region(sample_id)
        captured = self._audio.capture_pad_start(
            sample_id,
            start_s,
            end_s,
            1.0,
            exclusive=exclusive,
            received_at_ns=received_at_ns,
            **target_options,
            **options,
        )
        self._transport.residency.start(
            sample_id,
            start_s,
            end_s,
            expected_source_generation=generation,
            captured_start=captured,
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
        """Restart the explicit target, or the existing group when Multi Loop is ON.

        Args:
            received_at_ns: One captured Rust input timestamp shared by every pad.
        """
        if self._project.multi_loop:
            self._global_playback.start(received_at_ns=received_at_ns)
            return
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        target = self._audio.current_explicit_playback_target()
        sample_id = (
            target.current_control_slot() if target is not None else self._project.selected_pad
        )
        if sample_id is None:
            return
        validate_sample_id(sample_id)
        if self._project.sample_paths[sample_id] is None:
            return
        self._global_playback.prepare_explicit_start()
        self._start_pad(
            sample_id, exclusive=True, received_at_ns=received_at_ns, explicit_target=target
        )

    def stop_global_start_stop(self) -> None:
        """Stop active loops from START/STOP right mouse down without starting anything."""
        if self._project.multi_loop:
            self._global_playback.stop(remember=True)
            return
        self._global_playback.prepare_fenced_observation()
        ticket = self._audio.stop_explicit_playback()
        self._global_playback.observe_fenced_playback(ticket)

    def pause_pad(self, sample_id: int) -> None:
        """Pause a pad if it is currently playing.

        The pad remains active but its voice is silenced.
        """
        validate_sample_id(sample_id)
        if sample_id not in self._session.active_sample_ids:
            return
        self._transport.ensure_current_source_assignment_ready(sample_id)
        admission = self._audio.pause_sample(sample_id)
        self._pause_intent.remember(sample_id, admission)

    def resume_pad(self, sample_id: int) -> None:
        """Resume a paused pad.

        If the pad was paused, its voice continues from the saved position.
        If the pad was not paused, this has no effect.
        """
        validate_sample_id(sample_id)
        if sample_id not in self._session.active_sample_ids:
            return
        self._transport.ensure_current_source_assignment_ready(sample_id)
        if not self._pause_intent.requested_paused(sample_id):
            return  # Not paused

        admission = self._audio.resume_sample(sample_id)
        self._pause_intent.remember(sample_id, admission)

    def toggle_pause_pad(self, sample_id: int) -> None:
        """Toggle ordinary requested intent; waveform Hold keeps its separate ownership."""
        validate_sample_id(sample_id)
        self._transport.ensure_current_source_assignment_ready(sample_id)
        paused = self._pause_intent.requested_paused(sample_id)
        if sample_id not in self._session.active_sample_ids:
            return
        if paused:
            self.resume_pad(sample_id)
        else:
            self.pause_pad(sample_id)

    def seek_pad(self, sample_id: int, position_s: float) -> None:
        """Seek an active or paused pad voice without changing loop markers."""
        validate_sample_id(sample_id)
        ensure_finite(position_s)

        if self._project.sample_paths[sample_id] is None:
            return
        if sample_id not in self._session.active_sample_ids:
            return

        generation = self._transport.capture_current_pad_edit_generation(sample_id)
        target_s = max(0.0, float(position_s))
        self._transport.residency.seek(sample_id, target_s, expected_source_generation=generation)

    def handle_sample_started_message(
        self, msg: AudioMessage, *, sample_id: int | None = None
    ) -> None:
        pad_id = msg.sample_id() if sample_id is None else sample_id
        if pad_id is None:
            return

        self._session.active_sample_ids.add(pad_id)
        self._session.paused_sample_ids.discard(pad_id)

    def handle_sample_stopped_message(
        self, msg: AudioMessage, *, sample_id: int | None = None
    ) -> None:
        pad_id = msg.sample_id() if sample_id is None else sample_id
        if pad_id is None:
            return

        self._session.active_sample_ids.discard(pad_id)
        self._session.paused_sample_ids.discard(pad_id)
