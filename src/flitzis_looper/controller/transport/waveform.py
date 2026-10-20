import math
import threading
from dataclasses import dataclass
from typing import TYPE_CHECKING

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.current_timing import (
    UNRESOLVED_TIMING,
    CurrentPadTiming,
    UnresolvedTiming,
)
from flitzis_looper.input_timing import validate_input_timestamp_ns
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from flitzis_looper.control_diagnostics import ConsoleDiagnostics
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import HoldActionTicket, WaveFormRenderData


_TERMINAL_HOLD_STATES = frozenset({
    "no_effect",
    "resumed",
    "retired",
    "superseded",
    "exhausted",
    "rejected",
})


@dataclass(slots=True)
class _HoldOwner:
    ticket: HoldActionTicket
    physical_press_id: int
    release_requested: bool = False
    release_admitted: bool = False
    status: str = "pause_pending"
    error: str | None = None


class WaveformController:
    """Manage waveform editor."""

    _MAX_HOLD_OWNERS = 32

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._audio = transport._audio
        self._readiness: dict[int, tuple[str, str | None]] = {}
        self._view_revision = 0
        self._hold_thread = threading.get_ident()
        self._hold_owners: dict[int, _HoldOwner] = {}
        self._physical_hold_press: int | None = None
        self._next_physical_press = 1
        self._hold_admission_error: str | None = None
        self._diagnostics: ConsoleDiagnostics | None = None

    def bind_diagnostics(self, diagnostics: ConsoleDiagnostics) -> None:
        """Bind optional reporting without adding Native queries or callback work."""
        self._diagnostics = diagnostics

    @property
    def has_pending_holds(self) -> bool:
        """Retain every original ticket until its actual Native terminal status."""
        return bool(self._hold_owners)

    def hold_status(self) -> tuple[str | None, str | None]:
        """Expose cached settlement progress without deriving release authority."""
        owners = tuple(self._hold_owners.values())
        if not owners:
            return None, self._hold_admission_error
        error = next((owner.error for owner in reversed(owners) if owner.error), None)
        status = (
            "release_pending"
            if any(owner.release_requested for owner in owners)
            else owners[-1].status
        )
        return status, error or self._hold_admission_error

    def _ensure_hold_thread(self) -> None:
        if threading.get_ident() != self._hold_thread:
            message = "Waveform Hold owners belong to their original Controller thread"
            raise RuntimeError(message)

    def press_pause_hold(self, pad_id: int, *, received_at_ns: int | None = None) -> None:
        """Admit one original Native hold per physical right-button press.

        Native determines whether a playing cohort was actually paused. Session
        active/paused flags and the current display slot never grant ownership.
        """
        self._ensure_hold_thread()
        validate_sample_id(pad_id)
        if self._physical_hold_press is not None:
            return
        if self._next_physical_press >= 1 << 64:
            self._hold_admission_error = "Waveform physical press identities exhausted"
            return
        press_id = self._next_physical_press
        self._next_physical_press += 1
        self._physical_hold_press = press_id
        self._hold_admission_error = None
        if len(self._hold_owners) >= self._MAX_HOLD_OWNERS:
            self._hold_admission_error = "Earlier waveform pause releases are still pending"
            return
        try:
            timestamp = validate_input_timestamp_ns(received_at_ns)
            if timestamp is None:
                timestamp = validate_input_timestamp_ns(self._audio.capture_input_timestamp_ns())
            ticket = self._audio.pause_waveform_hold(pad_id, press_id, received_at_ns=timestamp)
        except (RuntimeError, ValueError) as error:
            self._hold_admission_error = str(error)
            return
        self._hold_owners[press_id] = _HoldOwner(ticket, press_id)

    def request_hold_release(self, reason: str = "button_up") -> None:
        """Make release sticky and clear only the physical latch, keeping original owners."""
        self._ensure_hold_thread()
        self._physical_hold_press = None
        for owner in self._hold_owners.values():
            if owner.release_requested:
                continue
            owner.release_requested = True
            try:
                owner.ticket.request_release()
            except (RuntimeError, ValueError) as error:
                owner.error = f"{reason}: {error}"

    @staticmethod
    def _observe_hold(owner: _HoldOwner) -> str | None:
        try:
            status = owner.ticket.publication_status()
        except (RuntimeError, ValueError) as error:
            owner.error = str(error)
            return None
        owner.status = status
        return status

    def _poll_hold(self, owner: _HoldOwner) -> bool:
        status = self._observe_hold(owner)
        if status in _TERMINAL_HOLD_STATES:
            return True
        try:
            owner.release_requested |= owner.ticket.release_requested() is True
            owner.release_admitted |= owner.ticket.release_admitted() is True
        except (RuntimeError, ValueError) as error:
            owner.error = str(error)
        if owner.release_requested and not owner.release_admitted:
            try:
                accepted = self._audio.release_waveform_hold(owner.ticket)
            except (RuntimeError, ValueError) as error:
                owner.error = str(error)
            else:
                if accepted is True:
                    # True also permits terminal settlement. Retain this original
                    # owner until publication_status actually reports terminal.
                    owner.release_admitted = True
                    owner.error = None
                else:
                    owner.error = "Waveform pause release is waiting for admission"
        return self._observe_hold(owner) in _TERMINAL_HOLD_STATES

    def _project_hold_display(self) -> None:
        display = None
        for owner in self._hold_owners.values():
            if owner.status not in {"paused_owned", "release_pending"}:
                continue
            try:
                slot = owner.ticket.current_control_slot()
            except (RuntimeError, ValueError) as error:
                owner.error = str(error)
                continue
            if type(slot) is int and 0 <= slot < NUM_SAMPLES:
                display = slot
        # A slot-only field is presentation. It cannot create an owner, remove
        # one, authorize a resume, or mutate Session's independent pause intent.
        self._transport._session.waveform_pause_hold_pad_id = display

    def poll_holds(self) -> None:
        """Attempt each requested release once per control tick, then observe terminal ACK."""
        self._ensure_hold_thread()
        for press_id, owner in tuple(self._hold_owners.items()):
            if self._poll_hold(owner):
                self._hold_owners.pop(press_id)
        self._project_hold_display()

    @property
    def view_revision(self) -> int:
        """Invalidate UI projection caches when any shared editor path releases a view."""
        return self._view_revision

    def open_editor(self, pad_id: int) -> None:
        """Open or retarget the single editor, releasing the previous source view."""
        session = self._transport._session
        if session.waveform_editor_pad_id is not None and session.waveform_editor_pad_id != pad_id:
            self.request_hold_release("editor_retarget")
            self.release_view(session.waveform_editor_pad_id)
        session.waveform_editor_open = True
        session.waveform_editor_pad_id = pad_id

    def close_editor(self) -> None:
        """Close the single editor and release its view without transport mutations."""
        session = self._transport._session
        self.request_hold_release("editor_close")
        if session.waveform_editor_pad_id is not None:
            self.release_view(session.waveform_editor_pad_id)
        session.waveform_editor_open = False
        session.waveform_editor_pad_id = None

    def toggle_editor(self, pad_id: int) -> None:
        """Use the same open/close authority for toolbar, sidebar, and mapped actions."""
        if self._transport._project.sample_paths[pad_id] is None:
            return
        session = self._transport._session
        if session.waveform_editor_open and session.waveform_editor_pad_id == pad_id:
            self.close_editor()
        else:
            self.open_editor(pad_id)

    def readiness(self, pad_id: int) -> tuple[str, str | None]:
        """Return full-source projection readiness without changing playback."""
        return self._readiness.get(pad_id, ("idle", None))

    def source_identity(self, pad_id: int) -> tuple[int, str, int, int] | None:
        """Read the assignment identity used to fence same-path editor caches."""
        identity = self._audio.waveform_source_identity(pad_id)
        return identity if isinstance(identity, tuple) else None

    def retry(self, pad_id: int) -> None:
        """Retry a failed editor read without seeking or changing timing."""
        self._audio.retry_waveform(pad_id)
        self._readiness[pad_id] = ("idle", None)
        if self._diagnostics is not None:
            self._diagnostics.begin_attempt("waveform_read", pad_id)

    def release_view(self, pad_id: int) -> None:
        """Invalidate editor work; its worker owns the reader until read return."""
        self._audio.retry_waveform(pad_id)
        self._readiness.pop(pad_id, None)
        self._view_revision += 1

    def get_render_data(
        self,
        pad_id: int,
        width_px: int,
        start_s: float,
        end_s: float,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> WaveFormRenderData | None:
        if not math.isfinite(start_s) or not math.isfinite(end_s) or end_s <= start_s:
            return None
        source_start_s = max(0.0, start_s)
        if isinstance(timing, UnresolvedTiming):
            timing = self._transport.bpm.current_timing(pad_id)
        duration_s = (
            timing.source_duration_seconds if isinstance(timing, CurrentPadTiming) else None
        )
        if duration_s is None:
            duration_s = self._transport._project.sample_durations[pad_id]
        source_end_s = min(end_s, duration_s) if duration_s is not None else end_s
        if source_end_s <= source_start_s:
            return None
        source_width_px = max(
            1, round(width_px * (source_end_s - source_start_s) / (end_s - start_s))
        )
        try:
            data = self._audio.get_waveform_render_data(
                pad_id, source_width_px, source_start_s, source_end_s
            )
            status = ("ready", None) if data is not None else self._audio.waveform_readiness(pad_id)
        except (RuntimeError, ValueError) as error:
            self._readiness[pad_id] = ("error", str(error))
            if self._diagnostics is not None:
                self._diagnostics.observe("waveform_read", str(error), pad_id=pad_id)
            return None
        self._readiness[pad_id] = status
        if self._diagnostics is not None:
            self._diagnostics.observe("waveform_read", status[1], pad_id=pad_id)
        return data
