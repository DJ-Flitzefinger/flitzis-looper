"""Transient mouse ownership for the existing complete Pair and Copy controllers."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from flitzis_looper.constants import NUM_SAMPLES

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper_audio import InputRuntimePadBinding

type ArrangementButton = Literal["left", "right"]
type ArrangementOperation = Literal["Move", "Swap", "Copy", "Copy overwrite"]


@dataclass(frozen=True, slots=True)
class ArrangementPreview:
    """A display projection, never admission authority."""

    source: int
    target: int
    operation: ArrangementOperation

    @property
    def label(self) -> str:
        """Identify the exact source, target and operation before release."""
        return f"{self.operation}: PAD {self.source + 1} → PAD {self.target + 1}"


@dataclass(frozen=True, slots=True)
class _Press:
    source: int
    button: ArrangementButton
    content: str
    path: str
    binding: InputRuntimePadBinding


def _valid_pad(pad: int) -> bool:
    return type(pad) is int and 0 <= pad < NUM_SAMPLES


class PadArrangementGestures:
    """Consume complete mouse presses while retaining their original source binding.

    This UI owner holds no audio state and writes no persistent mode. Admission,
    journal, preparation, Native effects and settlement belong to existing owners.
    """

    def __init__(self, controller: AppController) -> None:
        self._controller = controller
        self._enabled = False
        self._press: _Press | None = None
        self._consumed: set[ArrangementButton] = set()
        self.error: str | None = None

    @property
    def enabled(self) -> bool:
        """Return this UI context's transient mode, initially OFF."""
        return self._enabled

    @property
    def owns_pointer(self) -> bool:
        """Retain consumed release through all controls until the global tick."""
        return bool(self._consumed)

    @property
    def source_id(self) -> int | None:
        """Expose only the original gesture's source for rendering."""
        return None if self._press is None else self._press.source

    def toggle(self) -> None:
        """Change only the mode and cancel a pending gesture without leaking release."""
        self._enabled = not self._enabled
        self.cancel()
        self.error = None

    def cancel(self) -> None:
        """Discard mutation intent while consuming its still-held mouse buttons."""
        self._press = None

    def press(self, source: int, button: ArrangementButton) -> None:
        """Capture the actual original source before any normal trigger or stop."""
        if not self._enabled:
            return
        self._consumed.add(button)
        if self._press is not None:
            return
        self.error = None
        app = self._controller
        if not _valid_pad(source) or app.pairs.active or app.arrangement.active:
            return
        content = app.project.pad_content[source]
        path = app.project.sample_paths[source]
        if content is None or path is None or source in app.session.loading_sample_ids:
            return
        try:
            binding = app._audio.current_input_runtime_pad_binding(source)
            if binding is not None and binding.is_current():
                self._press = _Press(source, button, content.instance_id, path, binding)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            self._controller.diagnostics.begin_attempt("arrangement_gesture", source)
            self._controller.diagnostics.observe("arrangement_gesture", self.error, pad_id=source)

    def _current(self, press: _Press) -> bool:
        app = self._controller
        content = app.project.pad_content[press.source]
        return (
            content is not None
            and content.instance_id == press.content
            and app.project.sample_paths[press.source] == press.path
            and press.source not in app.session.loading_sample_ids
            and not app.pairs.active
            and not app.arrangement.active
            and press.binding.is_current()
        )

    def preview(self, target: int) -> ArrangementPreview | None:
        """Resolve the hovered operation; the controller still performs final admission."""
        press = self._press
        if not self._enabled or press is None or not _valid_pad(target) or target == press.source:
            return None
        app = self._controller
        if target in app.session.loading_sample_ids:
            return None
        try:
            if not self._current(press):
                return None
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            app.diagnostics.observe("arrangement_gesture", self.error, pad_id=press.source)
            return None
        occupied = app.project.sample_paths[target] is not None
        if occupied != (app.project.pad_content[target] is not None):
            return None
        operation: ArrangementOperation
        if press.button == "left":
            operation = "Swap" if occupied else "Move"
        else:
            operation = "Copy overwrite" if occupied else "Copy"
        return ArrangementPreview(press.source, target, operation)

    def release(self, target: int, button: ArrangementButton) -> bool:
        """Dispatch at most one complete operation with the exact press-time binding."""
        press = self._press
        if press is None or press.button != button:
            return False
        preview = self.preview(target)
        self._press = None
        if preview is None:
            return False
        app = self._controller
        operation = (
            "pair_arrangement" if preview.operation in {"Move", "Swap"} else "copy_arrangement"
        )
        app.diagnostics.begin_attempt(operation)
        try:
            if preview.operation in {"Move", "Swap"}:
                accepted = app.pairs.begin(
                    press.source,
                    target,
                    operation="move" if preview.operation == "Move" else "swap",
                    source_binding=press.binding,
                )
                self.error = None if accepted else app.pairs.error
            else:
                copy = (
                    app.arrangement.copy_to_empty
                    if preview.operation == "Copy"
                    else app.arrangement.copy_overwrite
                )
                accepted = copy(press.source, target, source_binding=press.binding)
                self.error = None if accepted else app.arrangement.error
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            app.diagnostics.observe(operation, self.error)
            return False
        app.diagnostics.observe(operation, self.error)
        return accepted

    def poll(self, *, left_down: bool, right_down: bool, cancelled: bool = False) -> None:
        """Cancel outside release/focus/Close after rendering consumed the current frame."""
        if cancelled:
            self.cancel()
        down = {"left": left_down, "right": right_down}
        if self._press is not None and not down[self._press.button]:
            self.cancel()
        self._consumed = {button for button in self._consumed if down[button]}
