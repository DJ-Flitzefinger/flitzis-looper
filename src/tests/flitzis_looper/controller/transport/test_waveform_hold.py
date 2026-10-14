"""Original Controller custody and polling; real callback/cohort proof is Native-tested."""

from collections import deque
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import ProjectState, SessionState

if TYPE_CHECKING:
    from flitzis_looper.controller.transport.waveform import WaveformController
    from flitzis_looper_audio import AudioEngine, HoldActionTicket


@dataclass
class _Ticket:
    status: str = "pause_pending"
    slot: int | None = 0
    requested: bool = False
    admitted: bool = False
    current: bool = True
    request_calls: int = 0
    status_error: bool = False

    def publication_status(self) -> str:
        if self.status_error:
            message = "own placement settlement pending"
            raise RuntimeError(message)
        if self.status == "paused_owned" and self.requested:
            return "release_pending"
        return self.status

    def request_release(self) -> bool:
        self.request_calls += 1
        self.requested = True
        return True

    def release_requested(self) -> bool:
        return self.requested

    def release_admitted(self) -> bool:
        return self.admitted

    def is_current(self) -> bool:
        return self.current

    def current_control_slot(self) -> int | None:
        return self.slot


@dataclass
class _Audio:
    tickets: deque[_Ticket] = field(default_factory=deque)
    results: deque[bool | RuntimeError] = field(default_factory=deque)
    pauses: list[tuple[int, int, int | None]] = field(default_factory=list)
    releases: list[_Ticket] = field(default_factory=list)
    timestamps: int = 0

    def capture_input_timestamp_ns(self) -> int:
        self.timestamps += 1
        return 123456789

    def pause_waveform_hold(
        self, pad_id: int, press_id: int, *, received_at_ns: int | None = None
    ) -> HoldActionTicket:
        self.pauses.append((pad_id, press_id, received_at_ns))
        return cast("HoldActionTicket", self.tickets.popleft())

    def release_waveform_hold(self, ticket: _Ticket) -> bool:
        self.releases.append(ticket)
        result = self.results.popleft() if self.results else False
        if isinstance(result, RuntimeError):
            raise result
        if result:
            ticket.admitted = True
        return result

    def retry_waveform(self, _pad_id: int) -> None:
        pass


def _controller(*tickets: _Ticket) -> tuple[WaveformController, _Audio, SessionState]:
    audio = _Audio(deque(tickets))
    session = SessionState()
    transport = TransportController(ProjectState(), session, cast("AudioEngine", audio))
    return transport.waveform, audio, session


def test_duplicate_press_one_normalized_timestamp_and_true_retains_owner_until_callback() -> None:
    ticket = _Ticket()
    waveform, audio, session = _controller(ticket)
    waveform.press_pause_hold(0)
    waveform.press_pause_hold(215)
    assert audio.pauses == [(0, 1, 123456789)]
    assert audio.timestamps == 1
    assert waveform.has_pending_holds
    assert session.waveform_pause_hold_pad_id is None
    waveform.request_hold_release()
    waveform.request_hold_release()
    assert ticket.request_calls == 1
    audio.results.append(True)
    waveform.poll_holds()
    assert audio.releases == [ticket]
    assert waveform.has_pending_holds
    ticket.status = "paused_owned"
    waveform.poll_holds()
    assert session.waveform_pause_hold_pad_id == 0
    assert waveform.has_pending_holds
    assert audio.releases == [ticket]
    ticket.status = "resumed"
    waveform.poll_holds()
    assert not waveform.has_pending_holds
    assert session.waveform_pause_hold_pad_id is None


def test_pressure_and_status_errors_retry_original_ticket_once_each_tick() -> None:
    ticket = _Ticket(status="paused_owned")
    waveform, audio, _session = _controller(ticket)
    waveform.press_pause_hold(0, received_at_ns=0)
    waveform.request_hold_release("focus_cancel")
    audio.results.extend([False, RuntimeError("buffer may be full"), True])
    waveform.poll_holds()
    assert audio.releases == [ticket]
    assert waveform.hold_status()[0] == "release_pending"
    assert waveform.hold_status()[1] is not None
    ticket.status_error = True
    waveform.poll_holds()
    assert audio.releases == [ticket, ticket]
    assert waveform.has_pending_holds
    waveform.poll_holds()
    assert audio.releases == [ticket, ticket, ticket]
    assert waveform.has_pending_holds
    ticket.status_error = False
    waveform.poll_holds()
    assert audio.releases == [ticket, ticket, ticket]
    ticket.status = "resumed"
    waveform.poll_holds()
    assert not waveform.has_pending_holds
    assert audio.pauses == [(0, 1, 0)]
    assert audio.timestamps == 0


@pytest.mark.parametrize(
    "terminal", ["no_effect", "resumed", "retired", "superseded", "exhausted", "rejected"]
)
def test_only_actual_terminal_status_retires_owner(terminal: str) -> None:
    ticket = _Ticket(status="paused_owned", current=False)
    waveform, audio, _session = _controller(ticket)
    waveform.press_pause_hold(0, received_at_ns=1)
    waveform.poll_holds()
    assert waveform.has_pending_holds
    ticket.status = terminal
    waveform.poll_holds()
    assert not waveform.has_pending_holds
    assert audio.releases == []


@pytest.mark.parametrize("session_paused", [False, True])
def test_native_no_effect_is_truthful_despite_false_session_flags(*, session_paused: bool) -> None:
    ticket = _Ticket(status="no_effect")
    waveform, audio, session = _controller(ticket)
    if session_paused:
        session.active_sample_ids.add(0)
        session.paused_sample_ids.add(0)
    waveform.press_pause_hold(0, received_at_ns=1)
    waveform.press_pause_hold(0, received_at_ns=2)
    waveform.request_hold_release()
    waveform.poll_holds()
    assert audio.pauses == [(0, 1, 1)]
    assert audio.releases == []
    assert not waveform.has_pending_holds
    assert session.waveform_pause_hold_pad_id is None
    assert (0 in session.paused_sample_ids) == session_paused


def test_hold_follows_original_ticket_after_move_swap_and_refill_without_resume_by_slot() -> None:
    ticket = _Ticket(status="paused_owned")
    waveform, audio, session = _controller(ticket)
    waveform.press_pause_hold(0, received_at_ns=5)
    waveform.poll_holds()
    assert session.waveform_pause_hold_pad_id == 0
    ticket.slot = 215
    session.paused_sample_ids.update({0, 36})
    waveform.poll_holds()
    assert session.waveform_pause_hold_pad_id == 215
    waveform.request_hold_release("button_up")
    audio.results.append(True)
    waveform.poll_holds()
    ticket.status = "resumed"
    waveform.poll_holds()
    assert audio.releases == [ticket]
    assert session.paused_sample_ids == {0, 36}
    assert audio.pauses == [(0, 1, 5)]


@pytest.mark.parametrize("cancel_path", ["close", "retarget", "focus", "unload"])
def test_editor_cancel_paths_request_same_owner_before_hidden_poll(cancel_path: str) -> None:
    ticket = _Ticket(status="paused_owned")
    waveform, audio, _session = _controller(ticket)
    waveform.open_editor(0)
    waveform.press_pause_hold(0, received_at_ns=7)
    if cancel_path == "close":
        waveform.close_editor()
    elif cancel_path == "retarget":
        waveform.open_editor(215)
    else:
        waveform.request_hold_release(cancel_path)
    assert ticket.requested
    assert waveform.has_pending_holds
    audio.results.append(True)
    waveform.poll_holds()
    assert audio.releases == [ticket]
    assert waveform.has_pending_holds


def test_bound_retains_all_original_pending_owners_and_rejects_another_press() -> None:
    tickets = tuple(_Ticket() for _ in range(33))
    waveform, audio, _session = _controller(*tickets)
    for _ in range(32):
        waveform.press_pause_hold(0, received_at_ns=1)
        waveform.request_hold_release()
    waveform.press_pause_hold(0, received_at_ns=1)
    waveform.press_pause_hold(0, received_at_ns=1)
    assert len(audio.pauses) == 32
    assert len(waveform._hold_owners) == 32
    assert waveform.hold_status()[1] == "Earlier waveform pause releases are still pending"
    waveform.poll_holds()
    assert len(audio.releases) == 32
    assert all(
        actual is expected for actual, expected in zip(audio.releases, tickets[:32], strict=True)
    )


def test_slot_only_session_changes_cannot_create_remove_or_release_hold_owner() -> None:
    ticket = _Ticket(status="paused_owned", slot=215)
    waveform, audio, session = _controller(ticket)
    session.waveform_pause_hold_pad_id = 36
    waveform.poll_holds()
    assert not waveform.has_pending_holds
    assert session.waveform_pause_hold_pad_id is None
    waveform.press_pause_hold(0, received_at_ns=3)
    session.waveform_pause_hold_pad_id = None
    waveform.poll_holds()
    assert waveform.has_pending_holds
    assert session.waveform_pause_hold_pad_id == 215
    assert audio.releases == []
