"""Independent SYNC intent and truthful observations of the actual mapped voice."""

from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, Literal, cast

from flitzis_looper.models import validate_sample_id
from flitzis_looper.source_sync_intent import next_source_sync_intent
from flitzis_looper.timing_policy import variable_source_timing

if TYPE_CHECKING:
    from flitzis_looper.control_diagnostics import ConsoleDiagnostics
    from flitzis_looper.controller.source_beatmap_selection import SourceBeatmapSelection
    from flitzis_looper.controller.transport import TransportController


@dataclass(frozen=True, slots=True, kw_only=True)
class SourceSyncStatus:
    """Requested control and callback-confirmed sound are deliberately separate."""

    requested: bool
    effective: bool | None = None
    active: bool | None = None
    state: Literal["off", "stopped", "pending", "effective", "refused", "unconfirmed"] = "off"
    detail: str | None = None
    policy_mode: Literal["legacy", "off", "fixed", "mapped"] | None = None
    actual_realtime_bpm: bool | None = None


def _valid_feedback_fields(feedback: dict[str, object], metadata: dict[str, object]) -> bool:
    """Validate source fences and field types before considering actual audio mode."""
    if any(
        type(feedback.get(key)) is not int or feedback[key] != metadata.get(binding_key)
        for key, binding_key in (
            ("source_generation", "source_generation"),
            ("window_revision", "window_revision"),
        )
    ):
        return False
    if type(feedback.get("source_identity")) is not int:
        return False
    if type(feedback.get("active")) is not bool or type(feedback.get("effective")) is not bool:
        return False
    revision, map_revision = feedback.get("intent_revision"), feedback.get("map_revision")
    return (revision is None or (type(revision) is int and 0 <= revision < 1 << 64)) and (
        map_revision is None or isinstance(map_revision, str)
    )


def _valid_feedback(feedback: dict[str, object], metadata: dict[str, object]) -> bool:
    """Check effective-mode invariants only after the common feedback schema is valid."""
    if not _valid_feedback_fields(feedback, metadata):
        return False
    revision, map_revision = feedback.get("intent_revision"), feedback.get("map_revision")
    if not feedback["effective"]:
        return True
    if feedback.get("policy_mode") == "fixed":
        return (
            feedback["active"] is True
            and revision is not None
            and feedback.get("settled") is True
            and map_revision is None
        )
    return feedback["active"] is True and revision is not None and map_revision is not None


def _policy_feedback_valid(feedback: dict[str, object]) -> bool:
    for key in ("policy_sync", "policy_realtime_bpm"):
        if key not in feedback or (feedback[key] is not None and type(feedback[key]) is not bool):
            return False
    revision = feedback.get("policy_revision")
    return (
        feedback.get("policy_mode") in {None, "legacy", "off", "fixed", "mapped"}
        and "policy_mode" in feedback
        and "policy_revision" in feedback
        and (revision is None or (type(revision) is int and 0 <= revision < 1 << 64))
        and "settled" in feedback
        and (feedback["settled"] is None or type(feedback["settled"]) is bool)
    )


def _actual_realtime_bpm(feedback: dict[str, object]) -> bool | None:
    if feedback.get("policy_mode") is None or feedback.get("settled") is not True:
        return None
    return feedback["policy_realtime_bpm"] is not False


class SourceSyncController:
    """Change content intent without replacing effective audio or map jobs."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._audio = transport._audio
        self._maps: SourceBeatmapSelection | None = None
        self._diagnostics: ConsoleDiagnostics | None = None

    def bind_diagnostics(self, diagnostics: ConsoleDiagnostics) -> None:
        """Keep technical refusal details in the existing control-thread console."""
        self._diagnostics = diagnostics

    def bind_source_maps(self, maps: SourceBeatmapSelection) -> None:
        """Reuse the Loader's only qualified map owner."""
        self._maps = maps

    def set_requested(self, sample_id: int, *, requested: bool) -> None:
        """Persist one guarded changed request; actual restart adopts supported tuples."""
        validate_sample_id(sample_id)
        if type(requested) is not bool:
            message = "SYNC request must be a strict boolean."
            raise TypeError(message)
        self._transport.ensure_current_source_assignment_ready(sample_id)
        if self._project.pad_content[sample_id] is None:
            message = "SYNC requires assigned playable content."
            raise ValueError(message)
        current = self._project.pad_source_sync_intent[sample_id]
        if requested == self._project.pad_source_sync_preference(sample_id):
            return
        updated = next_source_sync_intent(current, requested=requested)
        self._project.pad_source_sync_intent[sample_id] = updated
        self._transport._mark_project_changed()
        self._transport.refresh_input_runtime()

    def toggle(self, sample_id: int) -> None:
        """Use one action for a GUI click and its learned device binding."""
        validate_sample_id(sample_id)
        self.set_requested(
            sample_id, requested=not self._project.pad_source_sync_preference(sample_id)
        )

    def requested_mode(self, sample_id: int) -> Literal["legacy", "off", "fixed", "mapped"]:
        """Resolve the same mode matrix as Native without granting adoption."""
        policy = self._project.global_timing_policy
        requested = self._project.resolve_pad_source_sync(sample_id)
        if policy.sync is False or (policy.sync is True and not requested):
            return "off"
        if not requested:
            return "legacy"
        return "mapped" if variable_source_timing(policy) else "fixed"

    def _observation(self, sample_id: int) -> dict[str, object] | None:
        self._transport.ensure_current_source_assignment_ready(sample_id)
        binding = self._audio.current_input_runtime_pad_binding(sample_id)
        observe = getattr(self._audio, "pad_source_sync_status", None)
        if binding is None or not callable(observe) or binding.is_current() is not True:
            return None
        metadata = binding.metadata()
        feedback = observe(sample_id)
        if (
            not isinstance(feedback, dict)
            or binding.is_current() is not True
            or not _valid_feedback(feedback, metadata)
            or (
                not self._project.global_timing_policy.is_neutral
                and not _policy_feedback_valid(feedback)
            )
        ):
            return None
        return feedback

    def status(self, sample_id: int) -> SourceSyncStatus:
        """Expose concise readiness and route changed technical details to diagnostics."""
        status = self._status(sample_id)
        if self._diagnostics is not None:
            reason = status.detail if status.state in {"refused", "unconfirmed"} else None
            self._diagnostics.observe(
                "source_sync",
                reason,
                pad_id=sample_id,
                request=self._project.pad_source_sync_intent[sample_id].revision,
            )
        return status

    def _status(self, sample_id: int) -> SourceSyncStatus:
        """Compare current intent/map with actual callback feedback, never map readiness."""
        validate_sample_id(sample_id)
        intent = self._project.pad_source_sync_intent[sample_id]
        requested = self._project.resolve_pad_source_sync(sample_id)
        try:
            feedback = self._observation(sample_id)
        except (RuntimeError, ValueError, TypeError) as error:
            return SourceSyncStatus(requested=requested, state="unconfirmed", detail=str(error))
        if feedback is None:
            return SourceSyncStatus(
                requested=requested,
                state="unconfirmed",
                detail="Audio confirmation unavailable.",
            )
        if not self._project.global_timing_policy.is_neutral:
            return self._policy_status(sample_id, feedback)
        active, effective = feedback["active"] is True, feedback["effective"] is True
        if not intent.requested:
            return SourceSyncStatus(
                requested=False,
                effective=effective,
                active=active,
                state="pending" if effective else "off",
                detail="Restart to apply SYNC OFF." if effective else None,
            )
        return self._requested_status(sample_id, feedback, active=active, effective=effective)

    def _policy_status(self, sample_id: int, feedback: dict[str, object]) -> SourceSyncStatus:
        requested = self._project.resolve_pad_source_sync(sample_id)
        expected = self.requested_mode(sample_id)
        intent = self._project.pad_source_sync_intent[sample_id]
        policy = self._project.global_timing_policy
        active, effective = feedback["active"] is True, feedback["effective"] is True
        mode = cast("Literal['legacy', 'off', 'fixed', 'mapped'] | None", feedback["policy_mode"])
        matching = (
            mode == expected
            and feedback["policy_revision"] == policy.revision
            and feedback["policy_sync"] is policy.sync
            and feedback["policy_realtime_bpm"] is policy.realtime_bpm
            and feedback["intent_revision"] == intent.revision
            and feedback["settled"] is True
        )
        if expected == "mapped":
            geometry = None if self._maps is None else self._maps.effective(sample_id)
            matching = (
                matching
                and geometry is not None
                and feedback["map_revision"] == geometry.revision()
            )
        if matching and (not requested or effective):
            return SourceSyncStatus(
                requested=requested,
                effective=effective,
                active=active,
                state="effective" if requested else "off",
                policy_mode=mode,
                actual_realtime_bpm=_actual_realtime_bpm(feedback),
            )
        if requested and expected == "mapped":
            return self._pending_mapped_policy(sample_id, feedback)
        status = SourceSyncStatus(
            requested=requested,
            effective=effective,
            active=active,
            state="pending" if active else "stopped" if requested else "off",
            policy_mode=mode,
            actual_realtime_bpm=_actual_realtime_bpm(feedback),
            detail=f"Start/retrigger to apply {expected} timing." if active or requested else None,
        )
        pending, error = self._transport.residency.status(sample_id)
        if error:
            return replace(status, state="refused", detail=error)
        if pending:
            return replace(status, detail=f"Playback {pending}.")
        return status

    def _pending_mapped_policy(
        self, sample_id: int, feedback: dict[str, object]
    ) -> SourceSyncStatus:
        status = self._requested_status(
            sample_id,
            feedback,
            active=feedback["active"] is True,
            effective=feedback["effective"] is True,
        )
        # Matching map/PAD metadata alone cannot acknowledge a different global revision.
        status = replace(
            status,
            policy_mode=cast(
                "Literal['legacy', 'off', 'fixed', 'mapped'] | None", feedback["policy_mode"]
            ),
            actual_realtime_bpm=_actual_realtime_bpm(feedback),
        )
        if status.state == "effective":
            return replace(
                status,
                state="pending",
                detail="Start/retrigger to apply the requested global policy.",
            )
        return status

    def _requested_status(
        self, sample_id: int, feedback: dict[str, object], *, active: bool, effective: bool
    ) -> SourceSyncStatus:
        intent = self._project.pad_source_sync_intent[sample_id]
        maps = self._maps
        geometry = None if maps is None else maps.effective(sample_id)
        matching = (
            effective
            and feedback["intent_revision"] == intent.revision
            and geometry is not None
            and feedback["map_revision"] == geometry.revision()
        )
        if matching:
            return SourceSyncStatus(
                requested=True, effective=True, active=active, state="effective"
            )
        pending, error = self._transport.residency.status(sample_id)
        if error:
            return SourceSyncStatus(
                requested=True, effective=effective, active=active, state="refused", detail=error
            )
        if geometry is None:
            map_status = "unavailable" if maps is None else maps.status(sample_id)
            detail = None if maps is None else maps.errors.get(sample_id)
            return SourceSyncStatus(
                requested=True,
                effective=effective,
                active=active,
                state="refused" if detail else "pending",
                detail=detail or f"Source map {map_status}.",
            )
        try:
            self._transport.loop.requested_region(sample_id)
        except (RuntimeError, ValueError) as error:
            return SourceSyncStatus(
                requested=True,
                effective=effective,
                active=active,
                state="refused",
                detail=str(error),
            )
        detail = f"Playback {pending}." if pending else "Start/retrigger to apply SYNC."
        return SourceSyncStatus(
            requested=True,
            effective=effective,
            active=active,
            state="pending" if active else "stopped",
            detail=detail,
        )
