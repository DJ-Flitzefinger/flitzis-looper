"""Content-bound Native pitch admission and transient receipt projection."""

import threading
from dataclasses import dataclass
from typing import TYPE_CHECKING

from flitzis_looper.input_mapping.actions import (
    describe_pitch_rejection,
    parse_pitch_action,
    pitch_note_action,
    pitch_original_action,
    pitch_step_action,
)
from flitzis_looper.input_timing import validate_input_timestamp_ns
from flitzis_looper.key_intent import PadKeyIntent
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.control_diagnostics import ConsoleDiagnostics
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import CapturedPitchAction


@dataclass(slots=True)
class _PitchOwner:
    capture: CapturedPitchAction
    sequence: int
    lifetime: int
    numeric: PadKeyIntent
    content_id: str | None = None
    published: bool = False


@dataclass(slots=True)
class _PitchProjection:
    observed_sequence: int = 0
    queued_sequence: int = 0
    phase: str | None = None
    error: str | None = None
    effective_sequence: int = 0
    effective_frame: int = -1
    effective_pitch: tuple[int, int] | None = None


class PitchController:
    """Publish each original action once, preserving requested and effective state separately."""

    _MAX_OWNERS = 1024

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._audio = transport._audio
        self._thread = threading.get_ident()
        self._owners: dict[int, _PitchOwner] = {}
        self._projections: dict[str, _PitchProjection] = {}
        self._closed = False
        self._refresh_runtime: Callable[[], bool] | None = None
        self._diagnostics: ConsoleDiagnostics | None = None
        transport._on_frame_render_callbacks.append(self.poll)

    def bind_runtime_refresh(self, refresh: Callable[[], bool]) -> None:
        """Bind control-row synchronization without coupling Transport to its App owner."""
        self._refresh_runtime = refresh

    def bind_diagnostics(self, diagnostics: ConsoleDiagnostics) -> None:
        """Bind optional control-thread reporting without a global logging dependency."""
        self._diagnostics = diagnostics

    def _begin_diagnostic_attempt(self, sample_id: int | None) -> None:
        if self._diagnostics is not None:
            self._diagnostics.begin_attempt("pitch", sample_id)
            self._diagnostics.begin_attempt("input_mapping")

    def _observe_diagnostic(self, sample_id: int, reason: str | None) -> None:
        if self._diagnostics is not None:
            self._diagnostics.observe(
                "pitch",
                reason,
                pad_id=sample_id,
                sequence=self.projection_sequence(sample_id),
            )

    def _ensure_thread(self) -> None:
        if threading.get_ident() != self._thread:
            message = "Pitch action owners belong to their original Controller thread"
            raise RuntimeError(message)
        if self._closed:
            message = "Pitch admission is closed"
            raise RuntimeError(message)

    def status(self, sample_id: int) -> tuple[str | None, str | None]:
        """Read this content's chosen-action progress without certifying audio from Config."""
        validate_sample_id(sample_id)
        content = self._project.pad_content[sample_id]
        projection = self._projections.get(content.instance_id) if content is not None else None
        return (projection.phase, projection.error) if projection is not None else (None, None)

    def projection_sequence(self, sample_id: int) -> int:
        """Return a process-only observed high-water mark for the Native requested shadow."""
        validate_sample_id(sample_id)
        content = self._project.pad_content[sample_id]
        projection = self._projections.get(content.instance_id) if content is not None else None
        return projection.observed_sequence if projection is not None else 0

    def choose_pad_action(
        self, sample_id: int, action_key: str, *, received_at_ns: int | None = None
    ) -> bool:
        """Capture explicit GUI content after its incoming source receipt is projected."""
        self._ensure_thread()
        validate_sample_id(sample_id)
        self._begin_diagnostic_attempt(sample_id)
        timestamp = validate_input_timestamp_ns(received_at_ns)
        if parse_pitch_action(action_key) is None:
            message = "invalid_action"
            raise ValueError(message)
        capture_api = getattr(self._audio, "capture_pad_pitch_action", None)
        if not callable(capture_api):
            self._report_error(sample_id, "Pitch choices require the current audio runtime.")
            return False
        try:
            self._transport.ensure_current_source_assignment_ready(sample_id)
            if self._refresh_runtime is not None and not self._refresh_runtime():
                self._report_error(
                    sample_id, "Audio control state is not ready for this pitch choice."
                )
                return False
            capture = capture_api(sample_id, action_key, received_at_ns=timestamp)
            return self.accept_captured_event(capture)
        except (RuntimeError, ValueError, TypeError) as error:
            self._report_error(sample_id, str(error))
            return False

    def choose_piano_note(
        self, sample_id: int, semitones: int, *, retrigger: bool, received_at_ns: int | None = None
    ) -> None:
        """Choose a distinct octave-bearing piano target without changing source timing."""
        action = pitch_note_action(semitones, retrigger=retrigger)
        self.choose_pad_action(sample_id, action.key, received_at_ns=received_at_ns)

    def original_key(
        self, sample_id: int, *, retrigger: bool, received_at_ns: int | None = None
    ) -> None:
        """Admit one complete pitch reset, including manual correction removal."""
        action = pitch_original_action(retrigger=retrigger)
        self.choose_pad_action(sample_id, action.key, received_at_ns=received_at_ns)

    def step_pitch(
        self,
        sample_id: int,
        semitones: int,
        *,
        piano: bool = False,
        retrigger: bool,
        received_at_ns: int | None = None,
    ) -> None:
        """Resolve a relative step from Native requested state, including rapid repeated inputs."""
        action = pitch_step_action(semitones, piano=piano, retrigger=retrigger)
        self.choose_pad_action(sample_id, action.key, received_at_ns=received_at_ns)

    def accept_selected_action(self, action_key: str, *, received_at_ns: int | None = None) -> bool:
        """Capture the authoritative Native selection for a keyboard semantic action."""
        self._ensure_thread()
        timestamp = validate_input_timestamp_ns(received_at_ns)
        if parse_pitch_action(action_key) is None:
            message = "invalid_action"
            raise ValueError(message)
        capture_api = getattr(self._audio, "capture_selected_pitch_action", None)
        if capture_api is None:
            message = "Pitch choices require the current audio runtime."
            raise RuntimeError(message)
        if not callable(capture_api):
            message = "Native selected pitch capture API is invalid."
            raise TypeError(message)
        capture = capture_api(action_key, received_at_ns=timestamp)
        return self.accept_captured_event(capture)

    @staticmethod
    def _new_owner(capture: CapturedPitchAction) -> _PitchOwner:
        if capture is None:
            message = "Native pitch action capture is unavailable"
            raise TypeError(message)
        sequence = capture.accepted_sequence
        lifetime = capture.content_lifetime
        if any(type(value) is not int or not 0 < value < 1 << 64 for value in (sequence, lifetime)):
            message = "Native pitch action identity is unavailable"
            raise TypeError(message)
        numeric = PadKeyIntent(base_shift=capture.base_shift, extra_shift=capture.extra_shift)
        return _PitchOwner(capture, sequence, lifetime, numeric)

    def accept_captured_event(self, capture: CapturedPitchAction) -> bool:
        """Publish the exact original envelope once; no source, time or target is reconstructed."""
        self._ensure_thread()
        owner = self._new_owner(capture)
        previous = self._owners.get(owner.sequence)
        if previous is not None:
            return previous.published
        if len(self._owners) >= self._MAX_OWNERS and capture.accepted is not True:
            self._cancel_capture(capture)
            message = "capacity"
            raise RuntimeError(message)
        self._owners[owner.sequence] = owner
        self._associate(owner)
        try:
            self._publish_capture(capture)
            owner.published = True
            self._project_requested(owner)
            self._observe(owner)
        except (RuntimeError, ValueError, TypeError) as error:
            self._report_owner_error(owner, str(error))
            if not owner.published:
                self._owners.pop(owner.sequence, None)
            raise
        self._clear_mapping_error()
        return True

    def _publish_capture(self, capture: CapturedPitchAction) -> None:
        if capture.state == "rejected":
            message = capture.reason or "Pitch choice was rejected."
            raise ValueError(message)
        if capture.accepted is True:
            return
        publish = getattr(self._audio, "publish_pitch_action", None)
        if publish is None:
            message = "Pitch choices require the current audio runtime."
            raise RuntimeError(message)
        if not callable(publish):
            message = "Native pitch publication API is invalid."
            raise TypeError(message)
        if publish(capture) is not True:
            self._cancel_capture(capture)
            message = "Pitch choice has not been accepted."
            raise RuntimeError(message)

    def _associate(self, owner: _PitchOwner) -> int | None:
        slot = owner.capture.current_control_slot
        if type(slot) is not int or not 0 <= slot < len(self._project.pad_content):
            return None
        if slot in self._transport._pair_deferred_sample_ids:
            return None
        try:
            self._transport.ensure_current_source_assignment_ready(slot)
            binding = self._audio.current_input_runtime_pad_binding(slot)
            lifetime = binding.metadata().get("content_lifetime") if binding is not None else None
        except RuntimeError, ValueError, TypeError:
            return None
        content = self._project.pad_content[slot]
        if (
            content is None
            or type(lifetime) is not int
            or lifetime != owner.lifetime
            or (owner.content_id is not None and owner.content_id != content.instance_id)
        ):
            return None
        owner.content_id = content.instance_id
        projection = self._projections.setdefault(content.instance_id, _PitchProjection())
        projection.observed_sequence = max(projection.observed_sequence, owner.sequence)
        return slot

    def _project_requested(self, owner: _PitchOwner) -> None:
        slot = self._associate(owner)
        if slot is None or owner.content_id is None or not owner.published:
            return
        projection = self._projections[owner.content_id]
        if owner.sequence <= projection.queued_sequence:
            return
        current = self._project.pad_key_intent[slot]
        correction_updates: dict[str, object] = {}
        if owner.capture.resets_key_correction is True:
            cleared = current.corrected(None)
            correction_updates = {
                "correction": cleared.correction,
                "correction_epoch": cleared.correction_epoch,
            }
        self._transport.pad._set_key_intent(
            slot,
            current.changed(
                base_shift=owner.numeric.base_shift,
                extra_shift=owner.numeric.extra_shift,
                **correction_updates,
            ),
        )
        projection.queued_sequence = owner.sequence
        projection.error = None
        projection.phase = "pending"

    def _observe(self, owner: _PitchOwner) -> bool:
        state = owner.capture.state
        if state == "applied" and not owner.capture.content_is_current:
            return True
        if owner.content_id is None:
            return state == "rejected"
        projection = self._projections[owner.content_id]
        if state == "applied":
            # Native effect can precede the incoming/Pair project receipt. Keep
            # the original until its numeric choice is projected or superseded.
            if projection.queued_sequence < owner.sequence:
                return False
            frame = owner.capture.effective_output_frame
            if type(frame) is int and (frame, owner.sequence) > (
                projection.effective_frame,
                projection.effective_sequence,
            ):
                projection.effective_frame = frame
                projection.effective_sequence = owner.sequence
                projection.effective_pitch = (
                    owner.numeric.base_shift,
                    owner.numeric.extra_shift,
                )
        if owner.sequence == projection.observed_sequence:
            if state == "rejected":
                projection.phase = "error"
                projection.error = describe_pitch_rejection(
                    owner.capture.reason or "Pitch choice was rejected."
                )
            elif projection.error is None:
                projection.phase = "applied" if state == "applied" else "preparing"
            slot = owner.capture.current_control_slot
            if type(slot) is int and 0 <= slot < len(self._project.pad_content):
                self._observe_diagnostic(slot, projection.error)
        return state in {"applied", "rejected"}

    def _report_owner_error(self, owner: _PitchOwner, error: str) -> None:
        if owner.content_id is None:
            return
        projection = self._projections[owner.content_id]
        if owner.sequence == projection.observed_sequence:
            projection.phase = "error"
            projection.error = describe_pitch_rejection(error)
            slot = owner.capture.current_control_slot
            if type(slot) is int and 0 <= slot < len(self._project.pad_content):
                self._observe_diagnostic(slot, projection.error)

    def _report_error(self, sample_id: int, error: str) -> None:
        content = self._project.pad_content[sample_id]
        if content is not None:
            projection = self._projections.setdefault(content.instance_id, _PitchProjection())
            projection.phase = "error"
            projection.error = describe_pitch_rejection(error)
        self._session.input_mapping_error = (
            f"Pitch choice not accepted: {describe_pitch_rejection(error)}"
        )
        self._observe_diagnostic(sample_id, describe_pitch_rejection(error))

    def _clear_mapping_error(self) -> None:
        error = self._session.input_mapping_error
        if error is not None and error.startswith("Pitch choice"):
            self._session.input_mapping_error = None

    def poll(self) -> None:
        """Observe every original owner; older effective ACKs never rewrite newer chosen values."""
        if self._closed:
            return
        self._ensure_thread()
        for sequence, owner in tuple(self._owners.items()):
            try:
                self._project_requested(owner)
                terminal = self._observe(owner)
            except (RuntimeError, ValueError, TypeError) as error:
                self._report_owner_error(owner, str(error))
                continue
            if terminal:
                self._owners.pop(sequence, None)
        live_ids = {
            content.instance_id for content in self._project.pad_content if content is not None
        } | {owner.content_id for owner in self._owners.values() if owner.content_id is not None}
        self._projections = {
            content_id: projection
            for content_id, projection in self._projections.items()
            if content_id in live_ids
        }

    def _cancel_capture(self, capture: CapturedPitchAction) -> None:
        cancel = getattr(self._audio, "cancel_pitch_action", None)
        if cancel is None:
            message = "Pitch cancellation requires the current audio runtime."
            raise RuntimeError(message)
        if not callable(cancel):
            message = "Native pitch cancellation API is invalid."
            raise TypeError(message)
        cancel(capture)

    def shut_down(self) -> None:
        """Fence every retained original action before the Native engine drains its workers."""
        if self._closed:
            return
        self._ensure_thread()
        for owner in self._owners.values():
            self._cancel_capture(owner.capture)
        self._closed = True
