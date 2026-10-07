"""Controller intent for transactional native GLOBAL START/STOP batches."""

from dataclasses import dataclass, replace
from typing import TYPE_CHECKING

from flitzis_looper.controller.current_binding import capture_current_pad_binding
from flitzis_looper.input_timing import validate_input_timestamp_ns

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import GlobalPlaybackBatchTicket, InputRuntimePadBinding


@dataclass(frozen=True)
class _PendingGlobalBatch:
    ticket: GlobalPlaybackBatchTicket
    restore_ids: frozenset[int]
    restore_revision: int


class GlobalPlaybackController:
    """Admit a complete native batch, then observe its acknowledged restore intent.

    Active/paused state follows native playback telemetry. Ticket polling never
    launches audio or changes loop/master timing.
    """

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._session = transport._session
        self._audio = transport._audio
        self._pending: _PendingGlobalBatch | None = None
        self._restore_revision = 0
        self._poll_feedback: Callable[[], None] | None = None
        transport._on_frame_render_callbacks.append(self.poll)

    def set_feedback_poll(self, callback: Callable[[], None]) -> None:
        """Register the existing app-owned native playback event drain."""
        self._poll_feedback = callback

    def forget_restore(self) -> None:
        """Supersede remembered global intent after another playback gesture."""
        self._restore_revision += 1
        self._session.global_stop_engaged = False
        self._session.global_stop_restore_sample_ids = set()

    def discard_pad_restore(self, sample_id: int) -> None:
        """Prune an unloaded pad from an acknowledgement still awaiting observation."""
        self._session.global_stop_restore_sample_ids.discard(sample_id)
        self._session.global_stop_engaged = bool(self._session.global_stop_restore_sample_ids)
        if self._pending is not None:
            self._pending = replace(
                self._pending, restore_ids=self._pending.restore_ids - {sample_id}
            )

    def poll(self) -> None:
        """Observe completion without projecting pending/rejected work as effective."""
        pending = self._pending
        if pending is None:
            return
        status = pending.ticket.publication_status()
        if status == "pending":
            return
        self._pending = None
        if status != "accepted" or pending.restore_revision != self._restore_revision:
            return
        self._session.global_stop_restore_sample_ids = set(pending.restore_ids)
        self._session.global_stop_engaged = bool(pending.restore_ids)

    def start(self, *, received_at_ns: int | None = None) -> None:
        """Start/restart the entire remembered or playing set with one input time."""
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        self.poll()
        if self._pending is not None:
            return
        if self._poll_feedback is not None:
            self._poll_feedback()
        target_ids = (
            self._session.global_stop_restore_sample_ids
            if self._session.global_stop_engaged
            else self._session.active_sample_ids - self._session.paused_sample_ids
        )
        if not target_ids:
            return
        entries: list[tuple[InputRuntimePadBinding, float, float | None]] = []
        for sample_id in sorted(target_ids):
            timing = self._transport.bpm.current_timing(sample_id)
            snapshot = capture_current_pad_binding(self._audio, sample_id, timing=timing)
            if snapshot is None:
                return
            start_s, end_s = self._transport.loop.effective_region(sample_id, timing=timing)
            entries.append((snapshot.binding, start_s, end_s))
        ticket = self._audio.start_global_playback_batch(entries, received_at_ns=received_at_ns)
        self._pending = _PendingGlobalBatch(ticket, frozenset(), self._restore_revision)

    def stop(self, *, remember: bool, received_at_ns: int | None = None) -> None:
        """Stop the complete active/paused set after all current permits agree."""
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        self.poll()
        if self._pending is not None:
            return
        if self._poll_feedback is not None:
            self._poll_feedback()
        active_ids = frozenset(self._session.active_sample_ids)
        if not active_ids:
            return
        bindings: list[InputRuntimePadBinding] = []
        for sample_id in sorted(active_ids):
            timing = self._transport.bpm.current_timing(sample_id)
            snapshot = capture_current_pad_binding(self._audio, sample_id, timing=timing)
            if snapshot is None:
                return
            bindings.append(snapshot.binding)
        restore_ids = active_ids - self._session.paused_sample_ids if remember else frozenset()
        ticket = self._audio.stop_global_playback_batch(bindings, received_at_ns=received_at_ns)
        self._pending = _PendingGlobalBatch(ticket, frozenset(restore_ids), self._restore_revision)
