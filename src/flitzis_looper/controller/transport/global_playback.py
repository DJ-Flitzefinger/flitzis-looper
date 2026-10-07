"""Controller intent for transactional native GLOBAL START/STOP batches."""

import time
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
    start: bool = False
    start_ids: frozenset[int] = frozenset()


@dataclass(frozen=True)
class _DeferredStart:
    sources: tuple[tuple[int, tuple[object, ...], object], ...]
    received_at_ns: int | None
    restore_revision: int
    deadline: float
    attempts: int = 0


class GlobalPlaybackController:
    """Admit a complete native batch, then observe its acknowledged restore intent.

    Active/paused state follows native playback telemetry. Deferred polling admits
    one prepared source-bound batch; ticket observation does not change timing.
    """

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._session = transport._session
        self._audio = transport._audio
        self._pending: _PendingGlobalBatch | None = None
        self._deferred: _DeferredStart | None = None
        self._restore_revision = 0
        self._poll_feedback: Callable[[], None] | None = None
        transport._on_frame_render_callbacks.append(self.poll)

    def set_feedback_poll(self, callback: Callable[[], None]) -> None:
        """Register the existing app-owned native playback event drain."""
        self._poll_feedback = callback

    def forget_restore(self) -> None:
        """Supersede remembered global intent after another playback gesture."""
        self._restore_revision += 1
        self._deferred = None
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
        if self._pending is None:
            self._poll_deferred()
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

    def _poll_deferred(self) -> None:
        deferred = self._deferred
        if deferred is None:
            return
        if deferred.restore_revision != self._restore_revision:
            self._deferred = None
            return
        if time.monotonic() >= deferred.deadline:
            self._retire_deferred(deferred, "global playback readiness deadline expired")
            return
        entries: list[tuple[InputRuntimePadBinding, float, float | None]] = []
        for sample_id, signature, accepted in deferred.sources:
            status, error = self._transport.residency.status(sample_id)
            if error and status is None:
                self._deferred = None
                return
            timing = self._transport.bpm.current_timing(sample_id)
            snapshot = capture_current_pad_binding(self._audio, sample_id, timing=timing)
            if (
                snapshot is None
                or snapshot.source_signature[:9] != signature
                or snapshot.accepted_timing != accepted
            ):
                self._deferred = None
                return
            if status is not None:
                return
            start_s, end_s = self._transport.loop.effective_region(sample_id, timing=timing)
            entries.append((snapshot.binding, start_s, end_s))
        self._admit_start(entries, deferred, from_poll=True)

    def _retire_deferred(self, deferred: _DeferredStart, error: str) -> None:
        self._deferred = None
        for sample_id, _, _ in deferred.sources:
            self._transport.residency.report_launch_error(sample_id, error)

    def _admit_start(
        self,
        entries: list[tuple[InputRuntimePadBinding, float, float | None]],
        deferred: _DeferredStart,
        *,
        from_poll: bool = False,
    ) -> None:
        try:
            ticket = self._audio.start_global_playback_batch(
                entries, received_at_ns=deferred.received_at_ns
            )
        except (RuntimeError, ValueError) as error:
            attempts = deferred.attempts + 1
            pressure = isinstance(error, RuntimeError) and "buffer may be full" in str(error)
            if not from_poll and not pressure:
                raise
            if pressure and attempts < 8 and time.monotonic() < deferred.deadline:
                self._deferred = replace(deferred, attempts=attempts)
            else:
                self._retire_deferred(deferred, str(error))
        else:
            self._deferred = None
            for sample_id, _, _ in deferred.sources:
                self._transport.residency.report_launch_error(sample_id, None)
            self._pending = _PendingGlobalBatch(
                ticket,
                frozenset(),
                deferred.restore_revision,
                start=True,
                start_ids=frozenset(sample_id for sample_id, _, _ in deferred.sources),
            )

    def start(self, *, received_at_ns: int | None = None) -> None:
        """Start/restart the entire remembered or playing set with one input time."""
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        self._deferred = None
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
        deferred_sources: list[tuple[int, tuple[object, ...], object]] = []
        waiting = False
        for sample_id in sorted(target_ids):
            timing = self._transport.bpm.current_timing(sample_id)
            snapshot = capture_current_pad_binding(self._audio, sample_id, timing=timing)
            if snapshot is None:
                return
            deferred_sources.append((
                sample_id,
                snapshot.source_signature[:9],
                snapshot.accepted_timing,
            ))
            waiting |= self._transport.residency.status(sample_id)[0] is not None
            start_s, end_s = self._transport.loop.effective_region(sample_id, timing=timing)
            entries.append((snapshot.binding, start_s, end_s))
        deferred = _DeferredStart(
            tuple(deferred_sources),
            received_at_ns,
            self._restore_revision,
            time.monotonic() + 30.0,
        )
        if waiting:
            self._deferred = deferred
            return
        self._admit_start(entries, deferred)

    def stop(self, *, remember: bool, received_at_ns: int | None = None) -> None:
        """Stop the complete active/paused set after all current permits agree."""
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        self._deferred = None
        prior_start_ids = (
            self._pending.start_ids
            if self._pending is not None and self._pending.start
            else frozenset()
        ) | self._transport.residency.cancel_all_launches()
        self.poll()
        if self._pending is not None:
            if not self._pending.start:
                return
            # The common native stop revision now fences this prior queued start.
            self._pending = None
        if self._poll_feedback is not None:
            self._poll_feedback()
        # A claimed START can have effective voices before their telemetry arrives.
        # Queue STOP for those targets behind that tail, even with empty UI state.
        active_ids = frozenset(self._session.active_sample_ids) | prior_start_ids
        if not active_ids:
            return
        bindings: list[InputRuntimePadBinding] = []
        for sample_id in sorted(active_ids):
            self._transport.residency.cancel_requested(sample_id)
            timing = self._transport.bpm.current_timing(sample_id)
            snapshot = capture_current_pad_binding(self._audio, sample_id, timing=timing)
            if snapshot is None:
                return
            bindings.append(snapshot.binding)
        restore_ids = active_ids - self._session.paused_sample_ids if remember else frozenset()
        ticket = self._audio.stop_global_playback_batch(bindings, received_at_ns=received_at_ns)
        self._pending = _PendingGlobalBatch(ticket, frozenset(restore_ids), self._restore_revision)
