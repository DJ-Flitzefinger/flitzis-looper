"""Persistent console feedback owned exclusively by the Python control thread."""

import re
import sys
import threading
from datetime import datetime
from pathlib import PureWindowsPath
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable, Iterable, Mapping

    from flitzis_looper.controller import AppController

_ABSOLUTE_PATH = re.compile(r"(?:[A-Za-z]:[\\/]|\\\\|(?<![\w:])/)[^\r\n\"']*")
_MAX_REASON_LENGTH = 1000


class ConsoleDiagnostics:
    """Print new diagnostic observations without repeating unchanged control polls."""

    def __init__(self, private_paths: Callable[[], Iterable[str]] = lambda: ()) -> None:
        self._thread = threading.get_ident()
        self._private_paths = private_paths
        self._observed: dict[tuple[str, int | None], tuple[str, object]] = {}

    def begin_attempt(self, operation: str, pad_id: int | None = None) -> None:
        """Allow the same refusal to be reported for a new deliberate action."""
        self._ensure_control_thread()
        self._observed.pop((operation, pad_id), None)

    def observe(
        self,
        operation: str,
        reason: str | None,
        *,
        pad_id: int | None = None,
        identity: object = None,
        channel: str | None = None,
        request: int | None = None,
        sequence: int | None = None,
    ) -> None:
        """Print a changed failure, or clear its previous observation on recovery."""
        self._ensure_control_thread()
        key = (channel or operation, pad_id)
        if not reason:
            self._observed.pop(key, None)
            return
        safe_reason = self._safe_reason(reason)
        observation = (safe_reason, (identity, request, sequence))
        if self._observed.get(key) == observation:
            return
        self._observed[key] = observation
        timestamp = datetime.now().astimezone().isoformat(timespec="seconds")
        pad = f" PAD={pad_id + 1}" if pad_id is not None else ""
        receipt = f" request={request}" if request is not None else ""
        receipt += f" sequence={sequence}" if sequence is not None else ""
        sys.stderr.write(
            f"[{timestamp}] LOOPER operation={operation}{pad}{receipt} reason={safe_reason}\n"
        )
        sys.stderr.flush()

    def _ensure_control_thread(self) -> None:
        if threading.get_ident() != self._thread:
            message = "Console diagnostics belong to their Python control thread"
            raise RuntimeError(message)

    def observe_pad_errors(
        self, operation: str, messages: Mapping[int, str], *, identity: object = None
    ) -> None:
        """Observe a cached error dictionary without scanning slots or calling Native APIs."""
        previous = {
            pad_id for name, pad_id in self._observed if name == operation and pad_id is not None
        }
        for pad_id in previous | messages.keys():
            self.observe(operation, messages.get(pad_id), pad_id=pad_id, identity=identity)

    def _safe_reason(self, reason: str) -> str:
        safe = reason
        paths = self._private_paths()
        replacements = {
            value
            for path in paths
            if path
            for value in (path, path.replace("\\", "/"), PureWindowsPath(path).name)
            if value
        }
        for value in sorted(replacements, key=len, reverse=True):
            safe = safe.replace(value, "[private source]")
        safe = _ABSOLUTE_PATH.sub("[private path]", safe)
        return " ".join(safe.split())[:_MAX_REASON_LENGTH]


def poll_control_diagnostics(app: AppController) -> None:
    """Observe existing controller projections after ordinary control event polling."""
    sink = app.diagnostics
    session = app.session
    errors = (
        ("load", session.sample_load_errors),
        ("analysis", session.sample_analysis_errors),
        ("stem_generation", session.stem_generation_errors),
    )
    for operation, messages in errors:
        sink.observe_pad_errors(operation, messages)
    sink.observe("input_mapping", session.input_mapping_error)
    sink.observe("project_save", app.persistence.save_error)
    sink.observe("recording_settings", app.persistence.recording_settings_warning)
    sink.observe("resident_pcm_settings", session.resident_pcm_settings_error)
    rate = app.transport.global_params.global_rate_status()
    sink.observe("global_rate", rate.error, request=rate.request_revision)
    sink.observe("pair_arrangement", app.pairs.error)
    sink.observe("copy_arrangement", app.arrangement.error)
    sink.observe("bank_arrangement", app.banks.error)
    sink.observe("waveform_hold", app.transport.waveform.hold_status()[1])
    _poll_stem_operation_diagnostics(app)


def _poll_stem_operation_diagnostics(app: AppController) -> None:
    """Observe retained operation metadata and the existing aggregate cleanup snapshot."""
    batch = app.stem_batch.summary
    # These caller tokens contain only labels/identity, never job or reader owners.
    batch_identity = tuple(row.token for row in app.stem_batch._rows or ())
    app.diagnostics.observe_pad_errors(
        "stem_batch",
        {
            item.pad_id: item.detail or "Stem generation failed"
            for item in batch.items
            if item.state == "failed"
        }
        if batch is not None
        else {},
        identity=batch_identity,
    )
    deletion = app.stem_delete.summary
    # Delete rows contain detached metadata only. Retaining the original tuple
    # prevents address reuse from hiding an identical failure in a later operation.
    delete_rows = app.stem_delete._rows
    app.diagnostics.observe_pad_errors(
        "stem_delete",
        {
            item.pad_id: item.detail or "Stem deletion failed"
            for item in deletion.items
            if item.state == "failed"
        }
        if deletion is not None
        else {},
        identity=(id(delete_rows), delete_rows),
    )
    try:
        _pending, _retained, _deleted, native_errors = app._audio.project_asset_cleanup_status()
        cleanup_errors = tuple(native_errors)
    except (OSError, RuntimeError, TypeError, ValueError) as error:
        cleanup_errors = (f"Cleanup status unavailable: {error}",)
    with app._assets._retirement_lock:
        control_errors = tuple(app._assets.retirement_errors.values())
    reasons = "; ".join(dict.fromkeys((*cleanup_errors, *control_errors)))
    app.diagnostics.observe("stem_cleanup", reasons or None)
