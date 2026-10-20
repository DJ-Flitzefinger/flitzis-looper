"""Actual Bank-row/modal IO joined to the existing real Bank/Native probe.

Only the App envelope and its polling breadth are fixtures: the delegate polls
the same Bank controller, without running unrelated App observers. UiContext,
Bank buttons, gesture owner, confirmation facade/controller and popup are real.
Observation wrappers retain the existing probe's before/after assertions and
record identities; they do not replace Native admission, workers, ACK or Config.
The enclosing Rust test alone advances the callback. No device is opened.
"""

from contextlib import ExitStack
from typing import TYPE_CHECKING, cast
from unittest.mock import patch

from imgui_bundle import imgui

from flitzis_looper.control_diagnostics import ConsoleDiagnostics
from flitzis_looper.controller.performance_confirmation import (
    BankConfirmation,
    PerformanceConfirmationController,
)
from flitzis_looper.ui import run
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import performance_view
from flitzis_looper.ui.render.performance_confirmation import performance_confirmation
from tests.flitzis_looper.ui.test_waveform_render_stability import _plot_context

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.bank_arrangement import BankRequest
    from tests.flitzis_looper.controller.bank_native_probe import BankControllerProbe


class _UiAppDelegate:
    """Supply absent UI envelope members while sharing every productive owner."""

    def __init__(self, probe: BankControllerProbe) -> None:
        self._probe = probe
        self._close_requested = False
        self.diagnostics = ConsoleDiagnostics()
        self.performance_confirmation = PerformanceConfirmationController(
            cast("AppController", self)
        )

    def __getattr__(self, name: str) -> object:
        return getattr(self._probe.app, name)

    def poll_runtime_events(self) -> None:
        """Keep the real global pointer tick; limit App polling to this Bank owner."""
        self._probe.controller.poll()


class RenderedBankProbe:
    """Drive actual Bank IO while retaining the existing probe's sole owners."""

    def __init__(self, probe: object) -> None:
        self.probe = cast("BankControllerProbe", probe)
        self._stack = ExitStack()
        self._stack.enter_context(_plot_context())
        self.app = _UiAppDelegate(self.probe)
        self.context = UiContext(cast("AppController", self.app))
        # UiContext registered this real view in the original App's shared list.
        # Keep that view bound to the same original owner required by the Bank
        # mirror, while the narrow delegate supplies only missing UI envelope APIs.
        self.context.ui.waveform._controller = cast("AppController", self.probe.app)
        assert self.context.ui.waveform in self.probe.app._pair_waveform_views
        assert self.context.ui.waveform._controller is cast("AppController", self.probe.app)
        self.captures: list[BankRequest] = []
        self.begins: list[BankRequest] = []
        self.buttons: dict[str, tuple[float, float, float, float]] = {}
        self.texts: list[str] = []
        self._capture_request = self.probe.controller.capture_request
        self._begin_request = self.probe.controller.begin
        self._button = imgui.button
        self._text = imgui.text_wrapped
        self._stack.enter_context(patch.object(imgui, "button", self._observe_button))
        self._stack.enter_context(patch.object(imgui, "text_wrapped", self._observe_text))
        self._stack.enter_context(
            patch.object(self.probe.controller, "capture_request", self._capture)
        )
        self._stack.enter_context(patch.object(self.probe.controller, "begin", self._begin))
        self._frame()
        self._frame()
        self._click("Re-Arrange")
        assert self.context.ui.arrangement.enabled
        assert not self.captures
        assert not self.begins

    def _observe_button(self, label: str, size: imgui.ImVec2Like = (0, 0)) -> bool:
        clicked = self._button(label, size)
        a, b = imgui.get_item_rect_min(), imgui.get_item_rect_max()
        self.buttons[label] = (a.x, a.y, b.x, b.y)
        return clicked

    def _observe_text(self, value: str) -> None:
        self._text(value)
        self.texts.append(value)

    def _capture(self, source_bank: int | None, target_bank: int) -> BankRequest:
        # Reuse capture's existing baseline/session fixture and its actual single
        # capture call. Restore the observation wrapper immediately afterwards.
        with patch.object(self.probe.controller, "capture_request", self._capture_request):
            self.probe.capture(source_bank, target_bank)
        request = self.probe.request
        assert request is not None
        assert self.probe.controller._request is request
        self.captures.append(request)
        return request

    def _begin(self, request: BankRequest) -> bool:
        assert request is self.probe.request
        assert request is self.captures[-1]
        accepted = self._begin_request(request)
        self.begins.append(request)
        self.probe.bank = self.probe.controller._bank
        bank = self.probe.bank
        assert accepted, self.probe.controller.error
        assert bank is not None
        assert bank.request is request
        assert bank.request.participants is request.participants
        assert self.probe.controller._request is None
        assert self.probe.persistence._migration_owner == bank.owner
        return accepted

    def _frame(self) -> None:
        self.texts.clear()
        imgui.new_frame()
        imgui.set_next_window_pos((0, 0))
        imgui.set_next_window_size(imgui.get_io().display_size)
        imgui.begin("real Bank Native join", flags=imgui.WindowFlags_.no_saved_settings)
        # The actual Bank row and common popup are sufficient for this bounded
        # join. Pad/sidebar rendering and normal-mode selection have own proofs.
        performance_view._render_banks(self.context)
        performance_confirmation(self.context)
        imgui.end()
        imgui.render()
        data = imgui.get_draw_data()
        assert data.total_vtx_count > 0
        for draw_list in data.cmd_lists:
            for command in draw_list.cmd_buffer:
                if command.elem_count:
                    assert command.idx_offset + command.elem_count <= len(draw_list.idx_buffer)
                    assert command.clip_rect.z >= command.clip_rect.x
                    assert command.clip_rect.w >= command.clip_rect.y
        run._poll_runtime_tick(self.context)

    def _click(self, label: str, button: int = imgui.MouseButton_.left) -> None:
        # A newly appearing auto-sized modal settles its position on later frames.
        # Read the actual stable rectangle before routing the fresh pointer IO.
        self._frame()
        self._frame()
        left, top, right, bottom = self.buttons[label]
        io = imgui.get_io()
        io.add_mouse_pos_event((left + right) / 2, (top + bottom) / 2)
        self._frame()
        pressed = True
        io.add_mouse_button_event(button, pressed)
        self._frame()
        released = False
        io.add_mouse_button_event(button, released)
        self._frame()

    def _warning(self, source: int | None, target: int, text: str) -> BankConfirmation:
        intent = self.context.ui.confirmation.pending
        assert isinstance(intent, BankConfirmation)
        assert intent.source_bank == source
        assert intent.target_bank == target
        assert intent.request is self.captures[-1]
        assert self.probe.request is intent.request
        assert self.probe.controller._request is intent.request
        assert self.context.ui.confirmation.is_current(intent)
        assert text in self.texts
        assert not self.probe.controller.active
        assert self.probe.bank is None
        assert not self.probe.tasks
        assert self.probe.persistence._migration_owner is None
        self.probe._assert_unchanged()
        return intent

    def _started(self, captures: int, begins: int) -> None:
        assert self.context.ui.confirmation.pending is None
        assert self.probe.bank is not None
        request = self.probe.bank.request
        assert self.probe.request is request
        assert self.captures[-1] is request
        assert self.begins[-1] is request
        assert self.probe.project.selected_bank == 0
        assert len(self.probe.tasks) == 1
        # More actual frames cannot recapture, repeat admission or advance ACK.
        self._frame()
        self._frame()
        assert len(self.captures) == captures
        assert len(self.begins) == begins
        assert self.probe.bank.ticket is None
        assert self.probe.bank.acknowledgement is None
        self.probe._assert_unchanged()

    def empty_copy(self) -> None:
        """A fresh other-bank click directly admits its original empty-target request."""
        self._click("Bank 1")
        assert not self.captures, "selected source must not copy itself"
        self._click("Bank 2")
        self._started(1, 1)

    def occupied_copy(self) -> None:
        """Cancel actual overwrite UI, then confirm one fresh original request once."""
        self._click("Bank 2")
        cancelled = self._warning(0, 1, "Overwrite Bank 2?")
        assert len(self.captures) == 2
        assert len(self.begins) == 1
        self._click("CANCEL")
        assert self.context.ui.confirmation.pending is None
        assert self.probe.controller._request is None
        assert not self.probe.controller.active
        self.probe._assert_unchanged()
        self._click("Bank 2")
        fresh = self._warning(0, 1, "Overwrite Bank 2?")
        assert fresh.request is not cancelled.request
        assert len(self.captures) == 3
        self._click("OVERWRITE")
        self._started(3, 2)

    def _clear(self, target: int, captures: int, begins: int) -> None:
        self._click(f"Bank {target + 1}", imgui.MouseButton_.right)
        self._warning(None, target, f"Clear Bank {target + 1}?")
        self._click("CLEAR BANK")
        self._started(captures, begins)

    def clear_other(self) -> None:
        """Right-click and confirm exactly the nonselected target bank."""
        self._clear(1, 4, 3)

    def clear_current(self) -> None:
        """Right-click and confirm the selected bank without changing selection."""
        self._clear(0, 5, 4)

    def close(self) -> None:
        """Release only render contexts and observation wrappers after settlement."""
        assert not self.probe.controller.active
        assert self.context.ui.confirmation.pending is None
        assert len(self.captures) == 5
        assert len(self.begins) == 4
        assert self.probe.project.selected_bank == 0
        self._stack.close()
