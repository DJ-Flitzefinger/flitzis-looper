"""Transient, content-bound admission for the two deliberate performer actions."""

from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, Literal

from flitzis_looper.controller.key_metadata import loaded_source_identity
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController

type ConfirmationAction = Literal["unload", "analyze"]


@dataclass(frozen=True)
class PerformanceConfirmation:
    """One captured content assignment; it grants no Native audio authority."""

    action: ConfirmationAction
    pad_id: int
    content_instance: str | None
    path: str
    source_identity: tuple[int, str, int, int]
    error: str | None = None


class PerformanceConfirmationController:
    """Keep performer warnings separate from loader and Residency transactions."""

    def __init__(self, app: AppController) -> None:
        self._app = app
        self._pending: PerformanceConfirmation | None = None

    @property
    def pending(self) -> PerformanceConfirmation | None:
        """Return the current immutable warning target, independently of selection."""
        return self._pending

    def request(self, action: ConfirmationAction, pad_id: int) -> None:
        """Capture eligible current content without executing or replacing a warning."""
        validate_sample_id(pad_id)
        if self._pending is not None or not self._eligible(action, pad_id):
            return
        path = self._app.project.sample_paths[pad_id]
        identity = self._source_identity(pad_id)
        if path is None or identity is None:
            return
        content = self._app.project.pad_content[pad_id]
        self._pending = PerformanceConfirmation(
            action, pad_id, content.instance_id if content is not None else None, path, identity
        )

    def is_current(self, intent: PerformanceConfirmation) -> bool:
        """Revalidate content identity and current eligibility without timing/mode fences."""
        if self._pending is not intent:
            return False
        content = self._app.project.pad_content[intent.pad_id]
        return (
            intent.error is None
            and self._eligible(intent.action, intent.pad_id)
            and self._app.project.sample_paths[intent.pad_id] == intent.path
            and (content.instance_id if content is not None else None) == intent.content_instance
            and self._source_identity(intent.pad_id) == intent.source_identity
        )

    def dismiss(self, intent: PerformanceConfirmation) -> None:
        """Discard only this warning; never cancel analysis, loading, or Residency."""
        if self._pending is intent:
            self._pending = None

    def accept(self, intent: PerformanceConfirmation) -> None:
        """Consume once, then dispatch only the still-bound authoritative loader action."""
        if self._pending is not intent:
            return
        valid = self.is_current(intent)
        self._pending = None
        if not valid:
            return
        try:
            if intent.action == "unload":
                self._app.loader.unload_sample(intent.pad_id)
            else:
                self._app.loader.analyze_sample_async(intent.pad_id)
        except (RuntimeError, ValueError) as error:
            # A consumed admission failure remains visible but cannot be dispatched again.
            # In particular, Native unload rejection occurs before any source/UI cleanup.
            self._pending = replace(intent, error=str(error))

    def _eligible(self, action: ConfirmationAction, pad_id: int) -> bool:
        if not self._app.loader.is_sample_loaded(pad_id):
            return False
        if self._app.loader.is_sample_loading(pad_id):
            return False
        return action != "analyze" or pad_id not in self._app.session.analyzing_sample_ids

    def _source_identity(self, pad_id: int) -> tuple[int, str, int, int] | None:
        try:
            return loaded_source_identity(self._app._audio, pad_id)
        except RuntimeError, ValueError:
            # A disappeared Native source grants no warning acceptance authority.
            return None
